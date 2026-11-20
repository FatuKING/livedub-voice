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

            let config_dir = app.path().app_config_dir()?;
            let settings_path = config_dir.join(settings::SETTINGS_FILE_NAME);
            let store = settings::SettingsStore::load(settings_path);
            tracing::info!(path = %store.path().display(), "settings loaded");
            app.manage(store);

            app.manage(credentials::Credentials::system());
            app.manage(credentials::validation::KeyValidator::new()?);
            app.manage(commands::MonitorState::default());
            app.manage(commands::OutgoingState::default());
            app.manage(commands::IncomingState::default());

            // App volumes a previous run left ducked (it crashed while translating incoming
            // audio) get their volume back.
            let journal = config_dir.join(audio::ducking::JOURNAL_FILE_NAME);
            let recovery = journal.clone();
            std::thread::spawn(move || {
                if let Err(error) = audio::ducking::recover(&recovery) {
                    tracing::warn!(%error, "could not restore app volumes left ducked");
                }
            });
            app.manage(commands::DuckingJournal(journal));
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
            commands::list_audio_devices,
            commands::list_audio_apps,
            commands::start_monitor,
            commands::stop_monitor,
            commands::start_outgoing,
            commands::stop_outgoing,
            commands::get_outgoing_status,
            commands::start_incoming,
            commands::stop_incoming,
            commands::get_incoming_status
        ])
        .build(tauri::generate_context!());

    let app = match result {
        Ok(app) => app,
        Err(error) => {
            eprintln!("error while running tauri application: {error}");
            std::process::exit(1);
        }
    };
    app.run(|app, event| {
        if let tauri::RunEvent::Exit = event {
            // Never leave the captured apps ducked: restore them before the process ends.
            app.state::<commands::IncomingState>().release_ducking();
            let journal = &app.state::<commands::DuckingJournal>().0;
            if let Err(error) = audio::ducking::recover(journal) {
                tracing::warn!(%error, "could not restore the ducked apps on exit");
            }
        }
    });
}
