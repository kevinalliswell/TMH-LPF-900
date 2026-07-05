//! Thin Tauri glue. M0: bare window, no plugins, no commands (those land in M3).

/// Build and run the Tauri application.
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
