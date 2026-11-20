//! LiveDub Voice core: real-time voice translation for calls and games on Windows.

pub mod audio;
pub mod commands;
pub mod credentials;
pub mod error;
pub mod logging;
pub mod persist;
pub mod pipeline;
pub mod settings;
pub mod translator;
pub mod usage;

use std::sync::Arc;

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
            let cap = store.get()?.spending_cap;
            app.manage(store);

            // Usage history: written every FLUSH_INTERVAL while it changes (and when a session
            // ends, and on exit).
            let meter = Arc::new(usage::UsageMeter::new(
                Some(config_dir.join(usage::USAGE_FILE_NAME)),
                Arc::new(usage::SystemClock),
                cap,
            ));
            app.manage(Arc::clone(&meter));
            tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(usage::FLUSH_INTERVAL);
                loop {
                    tick.tick().await;
                    let meter = Arc::clone(&meter);
                    let flushed = tauri::async_runtime::spawn_blocking(move || meter.flush()).await;
                    if let Ok(Err(error)) = flushed {
                        tracing::warn!(%error, "could not write the usage history");
                    }
                }
            });

            app.manage(credentials::Credentials::system());
            app.manage(credentials::validation::KeyValidator::new()?);
            app.manage(commands::MonitorState::default());
            app.manage(pipeline::PipelineManager::new());

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

            // Ctrl+C in the console (`tauri dev`) closes the app like its window does, so the
            // translations shut down cleanly instead of the process being killed.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    tracing::info!("Ctrl+C: closing LiveDub Voice");
                    handle.exit(0);
                }
            });
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
            commands::get_incoming_status,
            commands::get_usage,
            commands::override_spending_cap
        ])
        .build(tauri::generate_context!());

    let app = match result {
        Ok(app) => app,
        Err(error) => {
            eprintln!("error while running tauri application: {error}");
            std::process::exit(1);
        }
    };
    app.run(|app, event| match event {
        tauri::RunEvent::ExitRequested { code, .. } => {
            tracing::info!(?code, "exit requested");
        }
        tauri::RunEvent::Exit => {
            // The window closed, Ctrl+C, or anything calling `exit`: close both translations
            // (session.close, devices released, ducked apps restored) within a bounded time.
            commands::shut_down_pipelines(app);
            tracing::info!("LiveDub Voice exiting");
        }
        _ => {}
    });
}
