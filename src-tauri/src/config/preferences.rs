use serde::{Deserialize, Serialize};

use super::ConfigFile;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct Preferences {
    pub enable_overlay: bool,
    pub display_daily_clears: bool,
    pub display_timer: bool,
    pub display_weekly_clears: bool,
    pub display_spotify: bool,
    pub display_clear_notifications: bool,
    pub display_milliseconds: bool,

    // Spotify
    pub spotify_client_id: String,
    pub app_gradient_1: String,
    pub app_gradient_2: String,
    pub daily_color: String,
    pub weekly_color: String,
    pub spotify_accent_color: String,
    pub overlay_text_color: String,
    pub overlay_scale: u16,
    pub overlay_opacity: u8,
    pub overlay_position: String,
    pub overlay_offset_x: u16,
    pub overlay_offset_y: u16,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            enable_overlay: false,
            display_daily_clears: true,
            display_timer: true,
            display_weekly_clears: true,
            display_spotify: true,
            display_clear_notifications: true,
            display_milliseconds: true,

            spotify_client_id: String::new(),
            app_gradient_1: "#8e24aa".into(),
            app_gradient_2: "#3f1d4f".into(),
            daily_color: "#a9c6ff".into(),
            weekly_color: "#aa8bf3".into(),
            spotify_accent_color: "#1ed760".into(),
            overlay_text_color: "#f5f7fb".into(),
            overlay_scale: 100,
            overlay_opacity: 100,
            overlay_position: "top-right".into(),
            overlay_offset_x: 22,
            overlay_offset_y: 18,
        }
    }
}

impl ConfigFile for Preferences {
    fn get_filename() -> &'static str {
        "preferences.json"
    }
}