//! LiveDub Voice core: real-time voice translation for calls and games on Windows.

pub mod audio;
pub mod commands;
pub mod credentials;
pub mod error;
pub mod logging;
pub mod pipeline;
pub mod settings;
pub mod translator;
pub mod usage;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let log_dir = app.path().app_log_dir()?;
            let log_guard = logging::init(&log_dir)?;
            app.manage(log_guard);

            tracing::info!(
                version = env!("CARGO_PKG_VERSION"),
                log_dir = %log_dir.display(),
                "LiveDub Voice starting"
            );

            let settings_path = app
                .path()
                .app_config_dir()?
                .join(settings::SETTINGS_FILE_NAME);
            let store = settings::SettingsStore::load(settings_path);
            tracing::info!(path = %store.path().display(), "settings loaded");
            app.manage(store);

            app.manage(credentials::Credentials::system());
            app.manage(credentials::validation::KeyValidator::new()?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::get_settings,
            commands::save_settings,
            commands::get_api_key_status,
            commands::save_api_key,
            commands::validate_api_key,
            commands::delete_api_key,
            commands::list_audio_devices
        ])
        .run(tauri::generate_context!());

    if let Err(error) = result {
        eprintln!("error while running tauri application: {error}");
        std::process::exit(1);
    }
}
