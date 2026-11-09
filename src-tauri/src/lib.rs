//! LiveDub Voice core: real-time voice translation for calls and games on Windows.

pub mod audio;
pub mod commands;
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![commands::app_info])
        .run(tauri::generate_context!());

    if let Err(error) = result {
        eprintln!("error while running tauri application: {error}");
        std::process::exit(1);
    }
}
