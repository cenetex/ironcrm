use std::sync::OnceLock;
use tauri::Manager;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

static SERVER_PORT: OnceLock<u16> = OnceLock::new();

#[tauri::command]
fn get_api_url() -> String {
    let port = SERVER_PORT.get().copied().unwrap_or(3000);
    format!("http://127.0.0.1:{}", port)
}

fn find_open_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr().map(|a| a.port()))
        .unwrap_or(3000)
}

pub fn run() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let port = find_open_port();
    SERVER_PORT.set(port).ok();

    let db_path = std::env::var("LOCAL_DB_PATH")
        .unwrap_or_else(|_| "ironcrm.db".to_string());

    // Spawn the API server in the background
    let db = db_path.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = ironcrm::run_server(&db, port).await {
            tracing::error!("Server error: {}", e);
        }
    });

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_api_url])
        .setup(move |app| {
            let window = app.get_webview_window("main").unwrap();
            // Give server a moment to start, then navigate
            let api = format!("http://127.0.0.1:{}", port);
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let _ = window.eval(&format!("window.location.href = '{}'", api));
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
