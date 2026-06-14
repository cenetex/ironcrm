use ironcrm::module::ModuleDefinition;
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::sqlite::SqlitePool;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::Manager;
use tauri::State;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Serialize)]
struct ModulesResponse {
    modules: Vec<String>,
    count: usize,
    definitions: Vec<ModuleDefinition>,
}
#[derive(Serialize)]
struct RecordsResponse {
    module: String,
    data: Vec<Map<String, Value>>,
    total: i64,
    limit: u32,
    offset: u32,
}
#[derive(Serialize)]
struct RecordResponse {
    module: String,
    data: Option<Map<String, Value>>,
    relationships: Vec<Map<String, Value>>,
    activities: Vec<Map<String, Value>>,
}
#[derive(Serialize)]
struct SearchRecordsResponse {
    query: String,
    data: Vec<Map<String, Value>>,
    total: usize,
}
#[derive(Serialize)]
struct CreateRecordResponse {
    id: String,
    status: &'static str,
}

#[tauri::command]
async fn get_modules(state: State<'_, SqlitePool>) -> Result<ModulesResponse, String> {
    let modules = ironcrm::db::discover_modules_sqlite(&state)
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!(count = modules.len(), "get_modules");
    Ok(ModulesResponse {
        modules: modules.clone(),
        count: modules.len(),
        definitions: ironcrm::module::get_all_definitions(&state)
            .await
            .map_err(|e| e.to_string())?,
    })
}

#[tauri::command]
async fn get_module_records(
    state: State<'_, SqlitePool>,
    module: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<RecordsResponse, String> {
    let limit = limit.unwrap_or(25).min(100);
    let offset = offset.unwrap_or(0);
    tracing::info!(%module, limit, offset, "get_module_records");

    let total = ironcrm::db::count_module(&state, &module)
        .await
        .map_err(|e| {
            tracing::error!(%module, error = %e, "count_module failed");
            e.to_string()
        })?;
    tracing::info!(%module, total, "count_module done");

    let data = ironcrm::db::query_module(&state, &module, limit, offset)
        .await
        .map_err(|e| {
            tracing::error!(%module, error = %e, "query_module failed");
            e.to_string()
        })?;
    tracing::info!(%module, rows = data.len(), "query_module done");

    Ok(RecordsResponse {
        module,
        data,
        total,
        limit,
        offset,
    })
}

#[tauri::command]
async fn get_module_record(
    state: State<'_, SqlitePool>,
    module: String,
    id: String,
) -> Result<RecordResponse, String> {
    tracing::info!(%module, %id, "get_module_record");
    let data = ironcrm::db::get_module_record(&state, &module, &id)
        .await
        .map_err(|e| {
            tracing::error!(%module, %id, error = %e, "get_module_record failed");
            e.to_string()
        })?;
    let relationships = ironcrm::db::get_relationships(&state, &module, &id)
        .await
        .map_err(|e| {
            tracing::error!(%module, %id, error = %e, "get_relationships failed");
            e.to_string()
        })?;
    let activities = ironcrm::db::get_record_activities(&state, &module, &id, 25)
        .await
        .map_err(|e| {
            tracing::error!(%module, %id, error = %e, "get_record_activities failed");
            e.to_string()
        })?;
    Ok(RecordResponse {
        module,
        data,
        relationships,
        activities,
    })
}

#[tauri::command]
async fn search_records(
    state: State<'_, SqlitePool>,
    query: String,
    limit: Option<u32>,
) -> Result<SearchRecordsResponse, String> {
    let data = ironcrm::db::search_records(&state, &query, limit.unwrap_or(25))
        .await
        .map_err(|e| e.to_string())?;
    Ok(SearchRecordsResponse {
        query,
        total: data.len(),
        data,
    })
}

#[tauri::command]
async fn create_record(
    state: State<'_, SqlitePool>,
    module: String,
    fields: Map<String, Value>,
) -> Result<CreateRecordResponse, String> {
    let has_def = ironcrm::module::has_definition(&state, &module)
        .await
        .map_err(|e| e.to_string())?;
    if !has_def {
        return Err(format!("Unknown module '{}'", module));
    }

    let id = ironcrm::db::create_record(&state, &module, &fields)
        .await
        .map_err(|e| e.to_string())?;
    Ok(CreateRecordResponse { id, status: "ok" })
}

#[tauri::command]
async fn update_record(
    state: State<'_, SqlitePool>,
    module: String,
    id: String,
    fields: Map<String, Value>,
) -> Result<serde_json::Value, String> {
    let has_def = ironcrm::module::has_definition(&state, &module)
        .await
        .map_err(|e| e.to_string())?;
    if !has_def {
        return Err(format!("Unknown module '{}'", module));
    }

    let updated = ironcrm::db::update_record(&state, &module, &id, &fields)
        .await
        .map_err(|e| e.to_string())?;
    if !updated {
        return Err(format!("Record '{}' not found in '{}'", id, module));
    }

    Ok(serde_json::json!({"status": "ok", "id": id}))
}

#[tauri::command]
async fn delete_record(
    state: State<'_, SqlitePool>,
    module: String,
    id: String,
) -> Result<serde_json::Value, String> {
    let has_def = ironcrm::module::has_definition(&state, &module)
        .await
        .map_err(|e| e.to_string())?;
    if !has_def {
        return Err(format!("Unknown module '{}'", module));
    }

    let deleted = ironcrm::db::delete_record(&state, &module, &id)
        .await
        .map_err(|e| e.to_string())?;
    if !deleted {
        return Err(format!("Record '{}' not found in '{}'", id, module));
    }

    Ok(serde_json::json!({"status": "ok", "id": id}))
}

#[tauri::command]
async fn add_record_note(
    state: State<'_, SqlitePool>,
    module: String,
    id: String,
    note: String,
) -> Result<serde_json::Value, String> {
    if note.trim().is_empty() {
        return Err("Note cannot be empty".into());
    }

    let added = ironcrm::db::add_record_note(&state, &module, &id, &note)
        .await
        .map_err(|e| e.to_string())?;
    if !added {
        return Err(format!("Record '{}' not found in '{}'", id, module));
    }

    Ok(serde_json::json!({"status": "ok", "id": id}))
}

#[tauri::command]
async fn link_record(
    state: State<'_, SqlitePool>,
    module: String,
    id: String,
    target_module: String,
    target_id: String,
    relationship_type: Option<String>,
) -> Result<serde_json::Value, String> {
    let link_id = ironcrm::db::link_records(
        &state,
        &module,
        &id,
        &target_module,
        &target_id,
        relationship_type.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())?;

    link_id
        .map(|id| serde_json::json!({"status": "ok", "id": id}))
        .ok_or_else(|| "Could not link those records".to_string())
}

#[tauri::command]
async fn delete_record_link(
    state: State<'_, SqlitePool>,
    link_id: String,
) -> Result<serde_json::Value, String> {
    let deleted = ironcrm::db::delete_record_link(&state, &link_id)
        .await
        .map_err(|e| e.to_string())?;
    if !deleted {
        return Err(format!("Link '{}' not found", link_id));
    }
    Ok(serde_json::json!({"status": "ok", "id": link_id}))
}

#[tauri::command]
async fn export_workspace(state: State<'_, SqlitePool>) -> Result<Value, String> {
    ironcrm::db::export_workspace(&state)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn restore_workspace(state: State<'_, SqlitePool>, bundle: Value) -> Result<Value, String> {
    ironcrm::db::restore_workspace(&state, &bundle)
        .await
        .map(Value::Object)
        .map_err(|e| e.to_string())
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
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(file_writer)
                .with_ansi(false),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    tracing::info!(log = %log_path.display(), "IronCRM starting");

    let db_path = std::env::var("LOCAL_DB_PATH").unwrap_or_else(|_| "ironcrm.db".to_string());
    tracing::info!(%db_path, "database path");

    tauri::Builder::default()
        .setup(move |app| {
            let pool = tauri::async_runtime::block_on(async {
                ironcrm::db::connect_sqlite(&db_path)
                    .await
                    .expect("db connect")
            });
            app.manage(pool);
            tracing::info!("IronCRM ready");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_modules,
            get_module_records,
            get_module_record,
            search_records,
            create_record,
            update_record,
            delete_record,
            add_record_note,
            link_record,
            delete_record_link,
            export_workspace,
            restore_workspace,
            log_frontend
        ])
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
