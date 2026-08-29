use std::{sync::OnceLock, time::Duration};

pub const TARGET_NAME: &str = "destiny2.exe";
pub const OVERLAY_POLL_INTERVAL: Duration = Duration::from_millis(200);
pub const APP_NAME: &str = "Swift";
pub const APP_VER: &str = env!("CARGO_PKG_VERSION");
/// The Bungie API key, preferring one set in the environment at run time over
/// the one compiled in.
///
/// Released builds carry a baked-in key. But a compiled-in key is invisible
/// once it is wrong -- cargo happily reuses a binary built with a stale one,
/// and the only symptom is "The given Platform API Key is invalid (2101)" at
/// run time. Reading the environment first means testing a different key never
/// depends on whether a rebuild happened to be triggered.
pub fn api_key() -> &'static str {
    static API_KEY: OnceLock<String> = OnceLock::new();

    API_KEY.get_or_init(|| {
        match std::env::var("BUNGIE_API_KEY") {
            Ok(key) if !key.trim().is_empty() => key.trim().to_string(),
            _ => option_env!("BUNGIE_API_KEY").unwrap_or_default().to_string(),
        }
    })
}
pub const API_PATH: &str = "https://www.bungie.net/Platform";
// Both leading backslashes are required; `\.\pipe\...` is rejected as an
// invalid name and the app cannot start.
pub const NAMED_PIPE: &str = r"\\.\pipe\swift-open";
pub const USER_AGENT: &str = concat!("Swift/", env!("CARGO_PKG_VERSION"));

// How often the current activity is re-fetched. This is what decides how
// quickly the timer appears after loading in and disappears after finishing,
// so it runs on its own task and is never blocked by the (much slower)
// activity history fetch.
//
// Two seconds rather than one because this poll bypasses Bungie's CDN cache
// (see `BungieRequest::path`), so every one reaches their origin. Halving the
// rate means the app now makes *fewer* requests than it did while hitting the
// cache, and the freshness gained -- payloads ~1.3s old instead of ~100s --
// dwarfs the extra second of polling gap.
pub const CURRENT_ACTIVITY_POLL_INTERVAL: Duration = Duration::from_secs(2);

// The activity history only feeds the clear counts and the clear popup, so it
// can poll far more slowly. It is also nudged immediately after an activity
// ends, hence the delay below giving Bungie time to publish the new entry.
pub const ACTIVITY_HISTORY_POLL_INTERVAL: Duration = Duration::from_secs(10);
pub const ACTIVITY_HISTORY_NUDGE_DELAY: Duration = Duration::from_secs(3);

// Without these a stalled connection blocks the poll loop indefinitely.
pub const API_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
pub const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

// The current-activity poll gets a much tighter deadline than everything else.
// A stalled request blocks the poll loop for its whole timeout, and on a flaky
// connection that is the single biggest source of missed activity changes. A
// reply older than this is worthless anyway -- the next poll is a second away
// and will carry fresher data -- so give up early and let it try again.
pub const CURRENT_ACTIVITY_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

// How long to wait before retrying the profile fetch that must succeed before
// polling can start at all.
pub const PROFILE_INFO_RETRY_DELAY: Duration = Duration::from_secs(5);

// Number of consecutive failed polls before the error is shown to the user, so
// a single dropped request doesn't flash an error over a running timer.
pub const POLL_ERROR_TOLERANCE: usize = 3;

pub const RAID_ACTIVITY_MODE: usize = 4;
pub const DUNGEON_ACTIVITY_MODE: usize = 82;
pub const STRIKE_ACTIVITY_MODE: usize = 18;
pub const LOSTSECTOR_ACTIVITY_MODE: usize = 87;

pub const RAID_ACTIVITY_HASH: usize = 2043403989;
