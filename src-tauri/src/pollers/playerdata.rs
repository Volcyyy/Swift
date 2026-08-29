use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Datelike, Utc};
use serde::Serialize;
use tauri::{
    async_runtime::{self, JoinHandle},
    AppHandle, Manager,
};
use tokio::{
    sync::{mpsc, Mutex},
    time::MissedTickBehavior,
};

use crate::{
    api::{
        requests::{clock_offset_millis, BungieResponseError},
        responses::{ActivityInfo, CompletedActivity, LatestCharacterActivity, ProfileInfo},
        Api, ApiError, Source,
    },
    config::profiles::Profile,
    consts::{
        ACTIVITY_HISTORY_NUDGE_DELAY, ACTIVITY_HISTORY_POLL_INTERVAL,
        CURRENT_ACTIVITY_POLL_INTERVAL, DUNGEON_ACTIVITY_MODE, LOSTSECTOR_ACTIVITY_MODE,
        POLL_ERROR_TOLERANCE, PROFILE_INFO_RETRY_DELAY, RAID_ACTIVITY_MODE, STRIKE_ACTIVITY_MODE,
    },
    ConfigContainer,
};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlayerData {
    current_activity: CurrentActivity,
    activity_history: Vec<CompletedActivity>,
    profile_info: ProfileInfo,
}

#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlayerDataStatus {
    last_update: Option<PlayerData>,
    error: Option<String>,
    /// Milliseconds to add to the local clock to line it up with the Bungie
    /// clock that `start_date` is measured against.
    clock_offset_millis: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CurrentActivity {
    start_date: DateTime<Utc>,
    activity_hash: usize,
    activity_info: Option<ActivityInfo>,
}

#[derive(Default)]
pub struct PlayerDataPoller {
    task_handle: Option<JoinHandle<()>>,
    current_playerdata: Arc<Mutex<PlayerDataStatus>>,
}

impl PlayerDataPoller {
    pub async fn reset(&mut self, app_handle: AppHandle) {
        if let Some(t) = self.task_handle.as_ref() {
            t.abort();
        }

        {
            let mut lock = self.current_playerdata.lock().await;
            *lock = PlayerDataStatus::default();

            send_data_update(&app_handle, lock.clone());
        }

        let playerdata_clone = self.current_playerdata.clone();

        self.task_handle = Some(async_runtime::spawn(async move {
            let profile = {
                let container = app_handle.state::<ConfigContainer>();
                let lock = container.0.lock().await;

                match &lock.get_profiles().selected_profile {
                    Some(p) => p.clone(),
                    None => {
                        let mut lock = playerdata_clone.lock().await;
                        lock.error = Some("No profile set".to_string());

                        send_data_update(&app_handle, lock.clone());
                        return;
                    }
                }
            };

            // Retried rather than given up on. Nothing can poll until this
            // succeeds, so bailing out would leave the app dead for the whole
            // session over one timeout -- and this is the request most likely
            // to catch a blip, being the first one after launch. A genuinely
            // bad key just keeps failing, and keeps the error on screen.
            let profile_info = loop {
                let result = {
                    let api = app_handle.state::<Api>();
                    let mut lock = api.profile_info_source.lock().await;

                    lock.get(&profile).await
                };

                match result {
                    Ok(p) => break p,
                    Err(e) => {
                        {
                            let mut lock = playerdata_clone.lock().await;
                            lock.error = Some(format!("Failed to get profile info: {e}"));

                            send_data_update(&app_handle, lock.clone());
                        }

                        tokio::time::sleep(PROFILE_INFO_RETRY_DELAY).await;
                    }
                }
            };

            let mut current_activity = CurrentActivity {
                start_date: DateTime::<Utc>::MIN_UTC,
                activity_hash: 0,
                activity_info: None,
            };
            let mut activity_history = Vec::new();

            // Both fetches are allowed to fail here. The loops below retry on
            // their own, so one timed-out request while starting up must not
            // strand the app on an error screen for the rest of the session.
            let current_res = update_current(&app_handle, &mut current_activity, &profile).await;
            let history_res = update_history(&app_handle, &mut activity_history, &profile).await;

            {
                let mut lock = playerdata_clone.lock().await;

                // Seeded unconditionally: the poll loops read the current
                // activity back out of here, so it has to exist before they run.
                lock.last_update = Some(PlayerData {
                    current_activity,
                    activity_history,
                    profile_info,
                });

                // Only a failed current-activity fetch is worth showing. The
                // history just backs the clear counts, and the history loop
                // fills those in within its first tick.
                lock.error = current_res.err().map(|e| e.to_string());

                if let Err(e) = history_res {
                    eprintln!("Initial activity history fetch failed, retrying shortly: {e}");
                }

                send_data_update(&app_handle, lock.clone());
            }

            // The two polls run as separate loops so that fetching the activity
            // history -- which walks every character and every page back to
            // weekly reset -- can never delay noticing that an activity has
            // started or ended. Joining them here keeps a single abortable
            // handle: cancelling it cancels both.
            let (nudge_tx, nudge_rx) = mpsc::channel(1);

            tokio::join!(
                poll_current(&app_handle, &profile, &playerdata_clone, nudge_tx),
                poll_history(&app_handle, &profile, &playerdata_clone, nudge_rx),
            );
        }));
    }

    // For overlay / detail window to get initial data instead of waiting for poll
    pub fn get_data(&mut self) -> Option<PlayerDataStatus> {
        return match &self.current_playerdata.try_lock() {
            Ok(p) => {
                let mut data = (*p).clone(); // If playerdata doesn't exist, meaning poller isn't initialized
                data.clock_offset_millis = clock_offset_millis();
                Some(data)
            }
            Err(_) => None, // If lock currently in use, meaning stat update is in progress
        };
    }
}

// Polls the current activity, which is what decides when the timer appears,
// what it counts from, and when it goes away.
async fn poll_current(
    handle: &AppHandle,
    profile: &Profile,
    playerdata: &Arc<Mutex<PlayerDataStatus>>,
    nudge_history: mpsc::Sender<()>,
) {
    let mut ticker = tokio::time::interval(CURRENT_ACTIVITY_POLL_INTERVAL);

    // Keep a steady cadence rather than sleeping a fixed amount *after* each
    // request finishes, which used to add the request latency to every gap.
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker.tick().await; // Resolves immediately; the initial fetch already ran.

    let mut consecutive_errors = 0;

    loop {
        ticker.tick().await;

        let mut current = match playerdata.lock().await.last_update.as_ref() {
            Some(u) => u.current_activity.clone(),
            None => continue,
        };

        match update_current(handle, &mut current, profile).await {
            Ok(changed) => {
                consecutive_errors = 0;

                if changed {
                    // No activity info means the activity just ended, which is
                    // exactly when a new history entry is about to show up.
                    let ended = current.activity_info.is_none();

                    {
                        let mut lock = playerdata.lock().await;
                        lock.error = None;

                        if let Some(u) = lock.last_update.as_mut() {
                            u.current_activity = current;
                        }

                        send_data_update(handle, lock.clone());
                    }

                    if ended {
                        let _ = nudge_history.try_send(());
                    }
                } else {
                    clear_error(handle, playerdata).await;
                }
            }
            Err(e) => {
                consecutive_errors += 1;

                if consecutive_errors >= POLL_ERROR_TOLERANCE {
                    report_error(handle, playerdata, e.to_string()).await;
                }
            }
        }
    }
}

// Polls the completed activity history, which feeds the clear counts and the
// clear popup. Nothing here is on the timer's critical path.
async fn poll_history(
    handle: &AppHandle,
    profile: &Profile,
    playerdata: &Arc<Mutex<PlayerDataStatus>>,
    mut nudge: mpsc::Receiver<()>,
) {
    let mut ticker = tokio::time::interval(ACTIVITY_HISTORY_POLL_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker.tick().await; // Resolves immediately; the initial fetch already ran.

    let mut consecutive_errors = 0;

    loop {
        tokio::select! {
            _ = ticker.tick() => (),
            Some(_) = nudge.recv() => {
                // Bungie needs a moment to publish a just-finished activity, so
                // give it a beat before asking rather than wasting the request.
                tokio::time::sleep(ACTIVITY_HISTORY_NUDGE_DELAY).await;
            }
        }

        let mut history = match playerdata.lock().await.last_update.as_ref() {
            Some(u) => u.activity_history.clone(),
            None => continue,
        };

        match update_history(handle, &mut history, profile).await {
            Ok(changed) => {
                consecutive_errors = 0;

                if changed {
                    let mut lock = playerdata.lock().await;
                    lock.error = None;

                    if let Some(u) = lock.last_update.as_mut() {
                        u.activity_history = history;
                    }

                    send_data_update(handle, lock.clone());
                } else {
                    clear_error(handle, playerdata).await;
                }
            }
            Err(e) => {
                consecutive_errors += 1;

                if consecutive_errors >= POLL_ERROR_TOLERANCE {
                    report_error(handle, playerdata, e.to_string()).await;
                }
            }
        }
    }
}

async fn clear_error(handle: &AppHandle, playerdata: &Arc<Mutex<PlayerDataStatus>>) {
    let mut lock = playerdata.lock().await;

    if lock.error.take().is_some() {
        send_data_update(handle, lock.clone());
    }
}

async fn report_error(handle: &AppHandle, playerdata: &Arc<Mutex<PlayerDataStatus>>, error: String) {
    let mut lock = playerdata.lock().await;

    // Don't re-emit an error that is already on screen.
    if lock.error.as_deref() == Some(error.as_str()) {
        return;
    }

    lock.error = Some(error);
    send_data_update(handle, lock.clone());
}

fn send_data_update(handle: &AppHandle, mut data: PlayerDataStatus) {
    // Stamped at send time so the windows always time against the freshest
    // estimate of the Bungie clock.
    data.clock_offset_millis = clock_offset_millis();

    if let Some(o) = handle.get_window("overlay") {
        o.emit("playerdata_update", data.clone()).unwrap();
    }

    if let Some(o) = handle.get_window("details") {
        o.emit("playerdata_update", data).unwrap();
    }
}

async fn update_current(
    handle: &AppHandle,
    last_activity: &mut CurrentActivity,
    profile: &Profile,
) -> Result<bool> {
    let current_activities = Api::get_profile_activities(profile).await?;

    let activities = match current_activities.activities {
        Some(a) => a,
        None => bail!("Profile is private"),
    };

    let (characters, activities): (Vec<String>, Vec<LatestCharacterActivity>) =
        activities.into_iter().unzip();

    let latest_activity = activities
        .into_iter()
        .max()
        .ok_or(anyhow!("No character data for profile"))?;

    match last_activity
        .start_date
        .cmp(&latest_activity.date_activity_started)
    {
        std::cmp::Ordering::Less => {
            last_activity.start_date = latest_activity.date_activity_started
        }
        std::cmp::Ordering::Equal => {
            if last_activity.activity_info.is_none() {
                return Ok(false);
                // Return here, as once activity_info becomes None
                // for a given activity start_date, it should
                // stay None until start_date changes again
            }

            if last_activity.activity_hash == latest_activity.current_activity_hash {
                return Ok(false);
                // Return if the activity hash and time are the same
            }
        }
        std::cmp::Ordering::Greater => return Ok(false),
        // Only return if our last-fetched activity is more recent,
        // as current_hash can change without start_date changing
    }

    let api = handle.state::<Api>();

    api.profile_info_source
        .lock()
        .await
        .set_characters(profile, characters);

    if latest_activity.current_activity_hash == 0 {
        last_activity.activity_info = None;
        return Ok(true);
    }

    let current_activity_info = {
        let activity = api
            .activity_info_source
            .lock()
            .await
            .get(&latest_activity.current_activity_hash)
            .await;

        match activity {
            Ok(a) => a,
            Err(ApiError::ResponseError(BungieResponseError::ResponseMissing)) => {
                last_activity.activity_info = None;
                return Ok(true);
            }
            Err(e) => return Err(e.into()),
        }
    };

    if current_activity_info.name.is_empty() {
        last_activity.activity_info = None;
        return Ok(true);
    }

    last_activity.activity_hash = latest_activity.current_activity_hash;
    last_activity.activity_info = Some(current_activity_info);

    Ok(true)
}

async fn update_history(
    handle: &AppHandle,
    last_history: &mut Vec<CompletedActivity>,
    profile: &Profile,
) -> Result<bool> {
    let api = handle.state::<Api>();

    let profile_info = api.profile_info_source.lock().await.get(profile).await?;

    let mut past_activities: Vec<CompletedActivity> = Vec::new();

  let cutoff = {
    let now = Utc::now();

    let naive_reset = now
        .date_naive()
        .and_hms_opt(17, 0, 0)
        .ok_or(anyhow!("There is no 5PM UTC today?"))?;

    let mut reset = DateTime::<Utc>::from_utc(naive_reset, Utc);

    // Use the most recent daily reset.
    if reset > now {
        reset -= chrono::Duration::days(1);
    }

    // Go backwards to the most recent Tuesday reset.
    let days_since_tuesday =
        (reset.weekday().num_days_from_monday() + 6) % 7;

    reset - chrono::Duration::days(days_since_tuesday as i64)
};

    for character_id in profile_info.character_ids.iter() {
        let mut page = 0;

        loop {
            // The walk makes one request per page per character, and any single
            // failure discards every page fetched so far. One retry turns a
            // transient timeout into a hiccup instead of a lost cycle.
            let history = match Api::get_activity_history(profile, character_id, page).await {
                Ok(h) => h,
                Err(_) => Api::get_activity_history(profile, character_id, page).await?,
            };

            let activities = match history.activities {
                Some(a) => a,
                None => break,
            };

            let mut includes_past_cutoff = false;

            for activity in activities.into_iter() {
                if activity.period < cutoff {
                    includes_past_cutoff = true;
                } else if activity.modes.iter().any(|m| {
                    *m == RAID_ACTIVITY_MODE
                        || *m == DUNGEON_ACTIVITY_MODE
                        || *m == STRIKE_ACTIVITY_MODE
                        || *m == LOSTSECTOR_ACTIVITY_MODE
                }) {
                    past_activities.push(activity);
                }
            }

            if includes_past_cutoff {
                break;
            }

            page += 1;
        }
    }

    if let Some(last) = last_history.into_iter().max() {
        if let Some(new) = (&mut past_activities).into_iter().max() {
            if last >= new {
                return Ok(false);
            }
        }
    }

    past_activities.sort();

    let sorted_activities = past_activities.into_iter().rev().collect();

    *last_history = sorted_activities;

    Ok(true)
}
