use std::process::Command;
use tauri::Manager;

#[tauri::command]
fn get_api_url() -> String {
    "http://localhost:3000".to_string()
}

pub fn run() {
    // Spawn the IronCRM API server
    let server_path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("ironcrm-server");

    // Try to start the server; fall back to PATH
    let _server = if server_path.exists() {
        std::process::Command::new(&server_path)
            .env("LOCAL_DB_PATH", "ironcrm.db")
            .spawn()
    } else {
        std::process::Command::new("ironcrm")
            .env("LOCAL_DB_PATH", "ironcrm.db")
            .spawn()
    };

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_api_url])
        .setup(|app| {
            let _window = app.get_webview_window("main").unwrap();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
