//! Tauri commands exposed to the panel.
//!
//! Every command the panel can `invoke` lives here. Commands that can fail return
//! [`AppResult`](crate::error::AppResult), so errors reach the panel as `{ code, message }`.

use serde::Serialize;
use tauri::State;

use crate::error::AppResult;
use crate::settings::{Settings, SettingsStore};

/// Product name shown to the user.
pub const APP_NAME: &str = "LiveDub Voice";

/// Basic information about the running app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    /// Build profile: `"debug"` or `"release"`.
    pub profile: String,
}

impl AppInfo {
    /// Information about the current build.
    pub fn current() -> Self {
        let profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        Self {
            name: APP_NAME.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            profile: profile.to_owned(),
        }
    }
}

/// Returns the app name, version and build profile.
#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo::current()
}

/// Returns the current settings.
#[tauri::command]
pub fn get_settings(store: State<'_, SettingsStore>) -> AppResult<Settings> {
    store.get()
}

/// Validates and saves the settings, returning what was saved.
#[tauri::command]
pub fn save_settings(store: State<'_, SettingsStore>, settings: Settings) -> AppResult<Settings> {
    store.save(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn app_info_reports_name_version_and_profile() {
        let info = app_info();

        assert_eq!(info.name, "LiveDub Voice");
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        let expected_profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        assert_eq!(info.profile, expected_profile);
    }

    #[test]
    fn app_info_serializes_to_the_panel_shape() {
        let info = app_info();

        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            json!({ "name": info.name, "version": info.version, "profile": info.profile })
        );
    }
}
