#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::{collections::HashMap, io};

use anyhow::Context;

use api::{
    responses::{ActivityInfo, BungieProfile, ProfileInfo},
    Api, Source,
};
use config::{
    preferences::Preferences,
    profiles::{Profile, Profiles},
    ConfigManager,
};
use consts::{APP_NAME, APP_VER, NAMED_PIPE};
use pollers::{
    overlay::overlay_poller,
    playerdata::{PlayerDataPoller, PlayerDataStatus},
};
use tauri::{
    api::process::{Command, CommandChild, CommandEvent},
    async_runtime::{self, JoinHandle},
    AppHandle, CustomMenuItem, Manager, RunEvent, State, SystemTray, SystemTrayEvent,
    SystemTrayMenu, SystemTrayMenuItem, WindowBuilder, WindowUrl,
};
use tokio::{
    net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions},
    sync::Mutex,
};

mod api;
mod config;
mod consts;
mod pollers;

struct ConfigContainer(Mutex<ConfigManager>);

#[derive(Default)]
struct PlayerDataPollerContainer(Mutex<PlayerDataPoller>);

#[derive(Default)]
struct OverlayPollerHandle(Mutex<Option<JoinHandle<()>>>);

// The Spotify companion is a separate process. Nothing reaps it when the app
// goes away, so without holding on to it here every run leaks one, and the
// strays keep the sidecar binary locked against the next rebuild.
#[derive(Default)]
struct SpotifyChild(Mutex<Option<CommandChild>>);

// https://github.com/tauri-apps/wry/issues/583
#[tauri::command]
async fn open_preferences(handle: AppHandle) -> Result<(), tauri::Error> {
    open_preferences_window(&handle)
}

// https://github.com/tauri-apps/wry/issues/583
#[tauri::command]
async fn open_profiles(handle: AppHandle) -> Result<(), tauri::Error> {
    open_profiles_window(&handle)
}

#[tauri::command]
async fn get_preferences(container: State<'_, ConfigContainer>) -> Result<Preferences, ()> {
    Ok(container.0.lock().await.get_preferences().clone())
}

#[tauri::command]
async fn set_preferences(
    handle: AppHandle,
    preferences: Preferences,
    container: State<'_, ConfigContainer>,
    poller_handle: State<'_, OverlayPollerHandle>,
) -> Result<(), ()> {
    let mut lock = container.0.lock().await;
    lock.set_preferences(preferences.clone()).unwrap();

    // Broadcast updated preferences to every open Threepole window.
    for (_, window) in handle.windows() {
        let _ = window.emit("preferences_update", preferences.clone());
    }

    if let Some(o) = handle.get_window("overlay") {
        if preferences.enable_overlay {
            o.emit("preferences_update", preferences.clone()).unwrap();
        } else {
            if let Some(h) = poller_handle.0.lock().await.as_ref() {
                h.abort();
            }

            o.close().unwrap();
        }
    } else if preferences.enable_overlay {
        create_overlay(handle).await.unwrap();
    }

    Ok(())
}

#[tauri::command]
async fn get_profiles(container: State<'_, ConfigContainer>) -> Result<Profiles, ()> {
    Ok(container.0.lock().await.get_profiles().clone())
}

#[tauri::command]
async fn set_profiles(
    handle: AppHandle,
    profiles: Profiles,
    config_container: State<'_, ConfigContainer>,
    poller_container: State<'_, PlayerDataPollerContainer>,
) -> Result<(), ()> {
    let mut lock = config_container.0.lock().await;

    let was_no_profile = lock.get_profiles().selected_profile.is_none();

    lock.set_profiles(profiles).unwrap();

    if was_no_profile {
        if handle.get_window("overlay").is_none() && lock.get_preferences().enable_overlay {
            create_overlay(handle.clone()).await.unwrap();
        }

        open_details_window(&handle, true).unwrap();
    }

    poller_container.0.lock().await.reset(handle).await;

    Ok(())
}

#[tauri::command]
async fn get_profile_info(profile: Profile, api: State<'_, Api>) -> Result<ProfileInfo, String> {
    Ok(api
        .profile_info_source
        .lock()
        .await
        .get(&profile)
        .await
        .map_err(|e| e.to_string())?)
}

#[tauri::command]
async fn get_activity_info(
    activity_hash: usize,
    api: State<'_, Api>,
) -> Result<ActivityInfo, String> {
    Ok(api
        .activity_info_source
        .lock()
        .await
        .get(&activity_hash)
        .await
        .map_err(|e| e.to_string())?)
}

#[tauri::command]
async fn search_profile(
    display_name: String,
    display_name_code: usize,
) -> Result<Vec<BungieProfile>, String> {
    Ok(Api::search_profile(&display_name, display_name_code)
        .await
        .map_err(|e| e.to_string())?)
}

async fn create_overlay(handle: AppHandle) -> Result<(), tauri::Error> {
    let overlay = WindowBuilder::new(
        &handle,
        "overlay",
        WindowUrl::App("./src/overlay/overlay.html".into()),
    )
    .title(APP_NAME)
    .transparent(true)
    .decorations(false)
    .inner_size(400.0, 500.0)
    .resizable(false)
    .always_on_top(true)
    .inner_size(0.0, 0.0)
    .position(0.0, 0.0)
    .visible(false)
    .skip_taskbar(true)
    .build()?;

    overlay.set_ignore_cursor_events(true).unwrap();

    #[cfg(debug_assertions)]
    overlay.open_devtools();

    let handle_clone = handle.clone();
    let poller_handle = handle.state::<OverlayPollerHandle>();
    let mut lock = poller_handle.0.lock().await;

    if let Some(h) = lock.as_ref() {
        h.abort();
    }

    let handle = async_runtime::spawn(async move { overlay_poller(handle_clone).await });

    *lock = Some(handle);

    Ok(())
}

#[tauri::command]
async fn get_playerdata(
    poller_container: State<'_, PlayerDataPollerContainer>,
) -> Result<Option<PlayerDataStatus>, ()> {
    Ok(poller_container.0.lock().await.get_data())
}

fn open_preferences_window(handle: &AppHandle) -> Result<(), tauri::Error> {
    if let Some(w) = handle.get_window("preferences") {
        w.unminimize()?;
        return w.set_focus();
    }

    WindowBuilder::new(
        handle,
        "preferences",
        WindowUrl::App("./src/window/window.html#preferences".into()),
    )
    .title(APP_NAME)
    .decorations(false)
    .inner_size(400.0, 500.0)
    .resizable(false)
    .visible(false)
    .build()?;

    Ok(())
}

fn open_profiles_window(handle: &AppHandle) -> Result<(), tauri::Error> {
    if let Some(w) = handle.get_window("profiles") {
        w.unminimize()?;
        return w.set_focus();
    }

    WindowBuilder::new(
        handle,
        "profiles",
        WindowUrl::App("./src/window/window.html#profiles".into()),
    )
    .title(APP_NAME)
    .decorations(false)
    .inner_size(400.0, 500.0)
    .resizable(false)
    .visible(false)
    .build()?;

    Ok(())
}

fn open_details_window(handle: &AppHandle, welcome: bool) -> Result<(), tauri::Error> {
    if let Some(w) = handle.get_window("details") {
        w.unminimize()?;
        return w.set_focus();
    }

    WindowBuilder::new(
        handle,
        "details",
        WindowUrl::App(
            format!(
                "./src/window/window.html{}#details",
                if welcome { "?welcome" } else { "" }
            )
            .into(),
        ),
    )
    .title(APP_NAME)
    .decorations(false)
    .inner_size(600.0, 600.0)
    .resizable(false)
    .visible(false)
    .build()?;

    Ok(())
}

async fn activate(handle: &AppHandle) -> Result<(), tauri::Error> {
    let config_container = handle.state::<ConfigContainer>();
    let lock = config_container.0.lock().await;

    if lock.get_profiles().selected_profile.is_none() {
        open_profiles_window(&handle)
    } else {
        open_details_window(&handle, false)
    }
}

async fn pipe_loop(handle: AppHandle, mut pipe_server: NamedPipeServer) -> io::Result<()> {
    loop {
        pipe_server.connect().await?;
        pipe_server = ServerOptions::new().create(NAMED_PIPE)?;
        pipe_server.disconnect()?;

        activate(&handle).await.unwrap();
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Creating the pipe fails when another instance already holds it, in which
    // case connecting as a client is how we hand the activation over and exit.
    let pipe_server = match ServerOptions::new()
        .first_pipe_instance(true)
        .create(NAMED_PIPE)
    {
        Ok(s) => s,
        Err(create_err) => {
            ClientOptions::new().open(NAMED_PIPE).with_context(|| {
                format!(
 "Could not create the named pipe {NAMED_PIPE} ({create_err}), and could not reach an existing instance through it either. If no other copy of {APP_NAME} is running, this is unexpected."
                )
            })?;

            return Ok(());
        }
    };

    tauri::async_runtime::set(tokio::runtime::Handle::current());

    tauri::Builder::new()
        .manage(ConfigContainer(Mutex::new(
            ConfigManager::load().context("Failed to load config; see above for the path")?,
        )))
        .manage(Api::default())
        .manage(PlayerDataPollerContainer::default())
        .manage(OverlayPollerHandle::default())
        .manage(SpotifyChild::default())
        .system_tray(
            SystemTray::new().with_menu(
                SystemTrayMenu::new()
                    .add_item(
                        CustomMenuItem::new("version_info", format!("{APP_NAME} v{}", APP_VER))
                            .disabled(),
                    )
                    .add_native_item(SystemTrayMenuItem::Separator)
                    .add_item(CustomMenuItem::new("preferences", "Preferences"))
                    .add_item(CustomMenuItem::new("set_profile", "Set profile"))
                    .add_native_item(SystemTrayMenuItem::Separator)
                    .add_item(CustomMenuItem::new("exit", "Exit")),
            ),
        )
        .on_system_tray_event(|handle, event| {
            if let SystemTrayEvent::MenuItemClick { id, .. } = event {
                match id.as_str() {
                    "exit" => handle.exit(0),
                    "set_profile" => open_profiles_window(&handle).unwrap(),
                    "preferences" => open_preferences_window(&handle).unwrap(),
                    _ => (),
                }
            } else if let SystemTrayEvent::LeftClick { .. } = event {
                let handle_clone = handle.clone();
                async_runtime::spawn(async move { activate(&handle_clone).await.unwrap() });
            }
        })
        .invoke_handler(tauri::generate_handler![
            open_preferences,
            open_profiles,
            get_preferences,
            set_preferences,
            get_profiles,
            set_profiles,
            get_profile_info,
            get_activity_info,
            search_profile,
            get_playerdata,
        ])
        .setup(|app| {
            let handle = app.handle();

            // Start the bundled Spotify companion automatically.
            // The Spotify Client ID is supplied by the user in Preferences.
            {
                let spotify_handle = handle.clone();

                async_runtime::spawn(async move {
                    let config_container = spotify_handle.state::<ConfigContainer>();
                    let spotify_client_id = {
                        let lock = config_container.0.lock().await;
                        lock.get_preferences().spotify_client_id.clone()
                    };

                    if !spotify_client_id.trim().is_empty() {
                        match Command::new_sidecar("spotify-companion") {
                            Ok(command) => {
                                let mut envs = HashMap::new();
                                envs.insert(
                                    "SPOTIPY_CLIENT_ID".to_string(),
                                    spotify_client_id,
                                );

                                let command = command.envs(envs);

                                match command.spawn() {
                                    Ok((mut rx, child)) => {
                                        // Held in app state so it can be killed on exit, and
                                        // so Tauri's event receiver below keeps being drained
                                        // rather than having its process pipes abandoned.
                                        *spotify_handle.state::<SpotifyChild>().0.lock().await =
                                            Some(child);

                                        while let Some(event) = rx.recv().await {
                                            match event {
                                                CommandEvent::Stdout(line) => {
                                                    println!("[spotify] {}", line);
                                                }
                                                CommandEvent::Stderr(line) => {
                                                    eprintln!("[spotify] {}", line);
                                                }
                                                CommandEvent::Error(error) => {
                                                    eprintln!(
                                                        "Spotify companion process error: {}",
                                                        error
                                                    );
                                                }
                                                CommandEvent::Terminated(payload) => {
                                                    eprintln!(
                                                        "Spotify companion terminated: {:?}",
                                                        payload
                                                    );
                                                    break;
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    Err(error) => {
                                        eprintln!("Failed to start Spotify companion: {error}");
                                    }
                                }
                            }
                            Err(error) => {
                                eprintln!("Failed to create Spotify sidecar command: {error}");
                            }
                        }
                    } else {
                        eprintln!("Spotify Client ID is not configured.");
                    }
                });
            }
            let pipe_handle = handle.clone();

            async_runtime::spawn(async move { pipe_loop(pipe_handle, pipe_server).await });

            async_runtime::spawn(async move {
                let config_container = handle.state::<ConfigContainer>();
                let lock = config_container.0.lock().await;

                if lock.get_profiles().selected_profile.is_none() {
                    open_profiles_window(&handle).unwrap();
                } else {
                    if lock.get_preferences().enable_overlay {
                        create_overlay(handle.clone()).await.unwrap();
                    }

                    open_details_window(&handle, false).unwrap();
                }

                let poller_container = handle.state::<PlayerDataPollerContainer>();

                poller_container.0.lock().await.reset(handle.clone()).await;
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .context("Failed to build the Tauri app")?
        .run(|handle, event| match event {
            RunEvent::ExitRequested { api, .. } => {
                api.prevent_exit();
            }
            RunEvent::Exit => {
                // Closing windows does not exit (see above), so this only runs
                // on a real quit from the tray -- the one chance to take the
                // companion process down with us.
                if let Ok(mut lock) = handle.state::<SpotifyChild>().0.try_lock() {
                    if let Some(child) = lock.take() {
                        let _ = child.kill();
                    }
                }
            }
            _ => (),
        });

    Ok(())
}
