use serde::Serialize;
use ironcrm::module::ModuleDefinition;
use serde_json::{Map, Value};
use sqlx::sqlite::SqlitePool;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::Manager;
use tauri::State;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Serialize)]
struct ModulesResponse { modules: Vec<String>, count: usize, definitions: Vec<ModuleDefinition> }
#[derive(Serialize)]
struct RecordsResponse { module: String, data: Vec<Map<String, Value>>, total: i64, limit: u32, offset: u32 }
#[derive(Serialize)]
struct RecordResponse { module: String, data: Option<Map<String, Value>>, relationships: Vec<Map<String, Value>> }

#[tauri::command]
async fn get_modules(state: State<'_, SqlitePool>) -> Result<ModulesResponse, String> {
    let modules = ironcrm::db::discover_modules_sqlite(&state).await.map_err(|e| e.to_string())?;
    tracing::info!(count = modules.len(), "get_modules");
    Ok(ModulesResponse { modules: modules.clone(), count: modules.len(), definitions: ironcrm::module::get_all_definitions(&state).await.map_err(|e| e.to_string())? })
}

#[tauri::command]
async fn get_module_records(state: State<'_, SqlitePool>, module: String, limit: Option<u32>, offset: Option<u32>) -> Result<RecordsResponse, String> {
    let limit = limit.unwrap_or(25).min(100);
    let offset = offset.unwrap_or(0);
    tracing::info!(%module, limit, offset, "get_module_records");

    let total = ironcrm::db::count_module(&state, &module).await.map_err(|e| {
        tracing::error!(%module, error = %e, "count_module failed");
        e.to_string()
    })?;
    tracing::info!(%module, total, "count_module done");

    let data = ironcrm::db::query_module(&state, &module, limit, offset).await.map_err(|e| {
        tracing::error!(%module, error = %e, "query_module failed");
        e.to_string()
    })?;
    tracing::info!(%module, rows = data.len(), "query_module done");

    Ok(RecordsResponse { module, data, total, limit, offset })
}

#[tauri::command]
async fn get_module_record(state: State<'_, SqlitePool>, module: String, id: String) -> Result<RecordResponse, String> {
    tracing::info!(%module, %id, "get_module_record");
    let data = ironcrm::db::get_module_record(&state, &module, &id).await.map_err(|e| {
        tracing::error!(%module, %id, error = %e, "get_module_record failed");
        e.to_string()
    })?;
    let relationships = ironcrm::db::get_relationships(&state, &module, &id).await.map_err(|e| {
        tracing::error!(%module, %id, error = %e, "get_relationships failed");
        e.to_string()
    })?;
    Ok(RecordResponse { module, data, relationships })
}

#[tauri::command]
fn log_frontend(level: String, message: String) {
    match level.as_str() {
        "error" => tracing::error!("[JS] {}", message),
        "warn" => tracing::warn!("[JS] {}", message),
        "info" => tracing::info!("[JS] {}", message),
        _ => tracing::debug!("[JS] {}", message),
    }
}

fn log_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home).join("Library/Logs/IronCRM");
        let _ = std::fs::create_dir_all(&p);
        return p;
    }
    PathBuf::from(".")
}

pub fn run() {
    let log_dir = log_dir();
    let log_path = log_dir.join("ironcrm.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("failed to open log file");
    let file_writer = Mutex::new(file);

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer()
            .with_writer(file_writer)
            .with_ansi(false))
        .with(tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr))
        .init();

    tracing::info!(log = %log_path.display(), "IronCRM starting");

    let db_path = std::env::var("LOCAL_DB_PATH").unwrap_or_else(|_| "ironcrm.db".to_string());
    tracing::info!(%db_path, "database path");

    tauri::Builder::default()
        .setup(move |app| {
            let pool = tauri::async_runtime::block_on(async {
                ironcrm::db::connect_sqlite(&db_path).await.expect("db connect")
            });
            app.manage(pool);
            tracing::info!("IronCRM ready");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_modules, 
            get_module_records, 
            get_module_record,
            log_frontend
        ])
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
