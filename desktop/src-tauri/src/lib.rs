pub mod commands;
pub mod db;
pub mod error;
pub mod state;

use state::LauncherState;

/// Run the Tauri application.
pub fn run() {
    tauri::Builder::default()
        .manage(LauncherState::default())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_sql::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::query_registry,
            commands::list_instances,
            commands::get_settings,
            commands::set_settings,
        ])
        .setup(|app| {
            tauri::async_runtime::block_on(async {
                if let Err(e) = db::init_local_state(app.handle()).await {
                    eprintln!("Failed to initialize local state: {}", e);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
