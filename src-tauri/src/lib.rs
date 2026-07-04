use ironcrm::module::ModuleDefinition;
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::sqlite::SqlitePool;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::Manager;
use tauri::State;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

const APP_NAME: &str = "White Warren";
const LEGACY_APP_NAME: &str = "IronCRM";
const DB_FILENAME: &str = "white-warren.db";
const LEGACY_DB_FILENAME: &str = "ironcrm.db";

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
async fn connect_suitecrm(
    state: State<'_, SqlitePool>,
    database_url: String,
    mode: String,
) -> Result<Value, String> {
    let database_url = database_url.trim();
    if database_url.is_empty() {
        return Err("SuiteCRM connection string is required".into());
    }

    let summary = match mode.as_str() {
        "import" => ironcrm::db::import_from_mysql_into_pool(database_url, &state).await,
        "sync_readonly" => ironcrm::db::inspect_suitecrm_mysql(database_url).await,
        _ => return Err("Mode must be import or sync_readonly".into()),
    }
    .map_err(|e| e.to_string())?;

    let message = if mode == "import" {
        format!(
            "Imported {} records from {} SuiteCRM modules.",
            summary.records, summary.module_count
        )
    } else {
        format!(
            "Connected read-only to {} SuiteCRM modules.",
            summary.module_count
        )
    };

    Ok(serde_json::json!({
        "status": "ok",
        "mode": mode,
        "message": message,
        "modules": summary.modules,
        "module_counts": summary.module_counts,
        "module_count": summary.module_count,
        "records": summary.records,
        "relationships": summary.relationships,
        "metadata_relationships": summary.metadata_relationships,
        "warnings": summary.warnings
    }))
}

#[tauri::command]
fn detect_suitecrm() -> Value {
    serde_json::to_value(ironcrm::db::detect_local_suitecrm()).unwrap_or_else(|_| {
        serde_json::json!({
            "found": false,
            "message": "Could not inspect local SuiteCRM locations."
        })
    })
}

#[tauri::command]
async fn connect_odoo(
    state: State<'_, SqlitePool>,
    database_url: String,
    mode: String,
) -> Result<Value, String> {
    let database_url = database_url.trim();
    if database_url.is_empty() {
        return Err("Odoo PostgreSQL connection string is required".into());
    }

    let summary = match mode.as_str() {
        "import" => ironcrm::db::import_from_odoo_into_pool(database_url, &state).await,
        "sync_readonly" => ironcrm::db::inspect_odoo_postgres(database_url).await,
        _ => return Err("Mode must be import or sync_readonly".into()),
    }
    .map_err(|e| e.to_string())?;

    let message = if mode == "import" {
        format!(
            "Imported {} records from {} Odoo modules.",
            summary.records, summary.module_count
        )
    } else {
        format!(
            "Connected read-only to {} Odoo modules.",
            summary.module_count
        )
    };

    Ok(serde_json::json!({
        "status": "ok",
        "mode": mode,
        "message": message,
        "modules": summary.modules,
        "module_counts": summary.module_counts,
        "module_count": summary.module_count,
        "records": summary.records,
        "relationships": summary.relationships,
        "metadata_relationships": summary.metadata_relationships,
        "warnings": summary.warnings
    }))
}

#[tauri::command]
fn detect_odoo() -> Value {
    serde_json::to_value(ironcrm::db::detect_local_odoo()).unwrap_or_else(|_| {
        serde_json::json!({
            "found": false,
            "message": "Could not prepare a local Odoo connection string."
        })
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
async fn restore_record(
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

    let restored = ironcrm::db::restore_record(&state, &module, &id)
        .await
        .map_err(|e| e.to_string())?;
    if !restored {
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
async fn export_workspace_archive(state: State<'_, SqlitePool>) -> Result<Vec<u8>, String> {
    ironcrm::db::export_workspace_archive(&state)
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
async fn restore_workspace_archive(
    state: State<'_, SqlitePool>,
    archive: Vec<u8>,
) -> Result<Value, String> {
    ironcrm::db::restore_workspace_archive(&state, &archive, false)
        .await
        .map(Value::Object)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn restore_workspace_exact(
    state: State<'_, SqlitePool>,
    bundle: Value,
) -> Result<Value, String> {
    ironcrm::db::restore_workspace_exact(&state, &bundle)
        .await
        .map(Value::Object)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn restore_workspace_archive_exact(
    state: State<'_, SqlitePool>,
    archive: Vec<u8>,
) -> Result<Value, String> {
    ironcrm::db::restore_workspace_archive(&state, &archive, true)
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
        let p = PathBuf::from(home).join("Library/Logs").join(APP_NAME);
        let _ = std::fs::create_dir_all(&p);
        return p;
    }
    PathBuf::from(".")
}

fn app_support_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join(APP_NAME);
        let _ = std::fs::create_dir_all(&p);
        return p;
    }

    let p = std::env::temp_dir().join(APP_NAME);
    let _ = std::fs::create_dir_all(&p);
    p
}

fn default_db_path() -> PathBuf {
    let target = app_support_dir().join(DB_FILENAME);
    migrate_legacy_app_db(&target);
    migrate_legacy_dev_db(&target);
    target
}

fn migrate_legacy_app_db(target: &Path) {
    if target.exists() {
        return;
    }

    let Ok(home) = std::env::var("HOME") else {
        return;
    };

    let legacy = PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join(LEGACY_APP_NAME)
        .join(LEGACY_DB_FILENAME);

    if !legacy.is_file() {
        return;
    }

    if let Some(parent) = target.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            tracing::warn!(
                legacy = %legacy.display(),
                target = %target.display(),
                error = %error,
                "could not prepare branded database directory"
            );
            return;
        }
    }

    match std::fs::copy(&legacy, target) {
        Ok(_) => tracing::info!(
            legacy = %legacy.display(),
            target = %target.display(),
            "migrated legacy desktop database into White Warren"
        ),
        Err(error) => tracing::warn!(
            legacy = %legacy.display(),
            target = %target.display(),
            error = %error,
            "could not migrate legacy desktop database"
        ),
    }
}

fn migrate_legacy_dev_db(target: &Path) {
    if target.exists() {
        return;
    }

    let Ok(cwd) = std::env::current_dir() else {
        return;
    };
    let legacy = cwd.join(LEGACY_DB_FILENAME);
    if legacy == target || !legacy.is_file() {
        return;
    }

    if let Some(parent) = target.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            tracing::warn!(
                legacy = %legacy.display(),
                target = %target.display(),
                error = %error,
                "could not prepare native database directory"
            );
            return;
        }
    }

    match std::fs::copy(&legacy, target) {
        Ok(_) => tracing::info!(
            legacy = %legacy.display(),
            target = %target.display(),
            "migrated native database out of watched source tree"
        ),
        Err(error) => tracing::warn!(
            legacy = %legacy.display(),
            target = %target.display(),
            error = %error,
            "could not migrate native database"
        ),
    }
}

pub fn run() {
    let log_dir = log_dir();
    let log_path = log_dir.join("white-warren.log");
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

    tracing::info!(log = %log_path.display(), "White Warren starting");

    let db_path = std::env::var("LOCAL_DB_PATH")
        .or_else(|_| std::env::var("WHITE_WARREN_DB"))
        .or_else(|_| std::env::var("IRONCRM_DB"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_db_path());
    let db_path_display = db_path.display().to_string();
    let db_path = db_path.to_string_lossy().to_string();
    tracing::info!(db_path = %db_path_display, "database path");

    tauri::Builder::default()
        .setup(move |app| {
            let pool = tauri::async_runtime::block_on(async {
                ironcrm::db::connect_sqlite(&db_path)
                    .await
                    .expect("db connect")
            });
            app.manage(pool);
            tracing::info!("White Warren ready");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_modules,
            get_module_records,
            get_module_record,
            search_records,
            detect_suitecrm,
            connect_suitecrm,
            detect_odoo,
            connect_odoo,
            create_record,
            update_record,
            delete_record,
            restore_record,
            add_record_note,
            link_record,
            delete_record_link,
            export_workspace,
            export_workspace_archive,
            restore_workspace,
            restore_workspace_archive,
            restore_workspace_exact,
            restore_workspace_archive_exact,
            log_frontend
        ])
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
