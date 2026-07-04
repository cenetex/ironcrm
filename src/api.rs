use crate::module::ModuleDefinition;
use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::sqlite::SqlitePool;

// ── Types ──

#[derive(Serialize)]
pub struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
pub struct ModuleListResponse {
    pub definitions: Vec<ModuleDefinition>,
    modules: Vec<String>,
    count: usize,
}

#[derive(Serialize)]
pub struct ModuleRecordsResponse {
    module: String,
    data: Vec<Map<String, Value>>,
    total: i64,
    limit: u32,
    offset: u32,
}

#[derive(Serialize)]
pub struct ModuleRecordResponse {
    module: String,
    data: Option<Map<String, Value>>,
    relationships: Vec<Map<String, Value>>,
    activities: Vec<Map<String, Value>>,
}

#[derive(Serialize)]
pub struct SearchRecordsResponse {
    query: String,
    data: Vec<Map<String, Value>>,
    total: usize,
}

#[derive(Deserialize)]
pub struct SuiteCrmConnectRequest {
    database_url: String,
    mode: String,
}

#[derive(Serialize)]
pub struct SuiteCrmConnectResponse {
    status: &'static str,
    mode: String,
    message: String,
    modules: Vec<String>,
    module_counts: Vec<crate::db::SuiteCrmModuleCount>,
    module_count: usize,
    records: u64,
    relationships: u64,
    metadata_relationships: u64,
    warnings: Vec<String>,
}

#[derive(Deserialize)]
pub struct OdooConnectRequest {
    database_url: String,
    mode: String,
}

#[derive(Deserialize)]
pub struct Pagination {
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    q: String,
    limit: Option<u32>,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    error: String,
}

// ── Handlers ──

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

pub async fn list_modules(State(pool): State<SqlitePool>) -> Json<ModuleListResponse> {
    let modules = crate::db::discover_modules_sqlite(&pool)
        .await
        .unwrap_or_default();
    let count = modules.len();
    let definitions = crate::module::get_all_definitions(&pool)
        .await
        .unwrap_or_default();
    Json(ModuleListResponse {
        modules,
        count,
        definitions,
    })
}

pub async fn list_records(
    State(pool): State<SqlitePool>,
    Path(module): Path<String>,
    Query(pagination): Query<Pagination>,
) -> Result<Json<ModuleRecordsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let limit = pagination.limit.unwrap_or(25).min(100);
    let offset = pagination.offset.unwrap_or(0);

    let modules = crate::db::discover_modules_sqlite(&pool)
        .await
        .unwrap_or_default();
    if !modules.contains(&module) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Module '{}' not found", module),
            }),
        ));
    }

    let data = crate::db::query_module(&pool, &module, limit, offset)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;

    let total = crate::db::count_module(&pool, &module).await.unwrap_or(0);

    Ok(Json(ModuleRecordsResponse {
        module,
        data,
        total,
        limit,
        offset,
    }))
}

pub async fn get_record(
    State(pool): State<SqlitePool>,
    Path((module, id)): Path<(String, String)>,
) -> Result<Json<ModuleRecordResponse>, (StatusCode, Json<ErrorResponse>)> {
    let modules = crate::db::discover_modules_sqlite(&pool)
        .await
        .unwrap_or_default();
    if !modules.contains(&module) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Module '{}' not found", module),
            }),
        ));
    }

    let data = crate::db::get_module_record(&pool, &module, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;

    if data.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Record '{}' not found in module '{}'", id, module),
            }),
        ));
    }

    let relationships = crate::db::get_relationships(&pool, &module, &id)
        .await
        .unwrap_or_default();
    let activities = crate::db::get_record_activities(&pool, &module, &id, 25)
        .await
        .unwrap_or_default();

    Ok(Json(ModuleRecordResponse {
        module,
        data,
        relationships,
        activities,
    }))
}

pub async fn search_records(
    State(pool): State<SqlitePool>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<SearchRecordsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let data = crate::db::search_records(&pool, &query.q, query.limit.unwrap_or(25))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;

    Ok(Json(SearchRecordsResponse {
        query: query.q,
        total: data.len(),
        data,
    }))
}

pub async fn connect_suitecrm(
    State(pool): State<SqlitePool>,
    Json(payload): Json<SuiteCrmConnectRequest>,
) -> Result<Json<SuiteCrmConnectResponse>, (StatusCode, Json<ErrorResponse>)> {
    let database_url = payload.database_url.trim();
    if database_url.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "SuiteCRM connection string is required".into(),
            }),
        ));
    }

    let mode = payload.mode.as_str();
    let summary = match mode {
        "import" => crate::db::import_from_mysql_into_pool(database_url, &pool).await,
        "sync_readonly" => crate::db::inspect_suitecrm_mysql(database_url).await,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Mode must be import or sync_readonly".into(),
                }),
            ));
        }
    }
    .map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;

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

    Ok(Json(SuiteCrmConnectResponse {
        status: "ok",
        mode: mode.into(),
        message,
        modules: summary.modules,
        module_counts: summary.module_counts,
        module_count: summary.module_count,
        records: summary.records,
        relationships: summary.relationships,
        metadata_relationships: summary.metadata_relationships,
        warnings: summary.warnings,
    }))
}

pub async fn detect_suitecrm() -> Json<crate::db::SuiteCrmDetection> {
    Json(crate::db::detect_local_suitecrm())
}

pub async fn connect_odoo(
    State(pool): State<SqlitePool>,
    Json(payload): Json<OdooConnectRequest>,
) -> Result<Json<SuiteCrmConnectResponse>, (StatusCode, Json<ErrorResponse>)> {
    let database_url = payload.database_url.trim();
    if database_url.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Odoo PostgreSQL connection string is required".into(),
            }),
        ));
    }

    let mode = payload.mode.as_str();
    let summary = match mode {
        "import" => crate::db::import_from_odoo_into_pool(database_url, &pool).await,
        "sync_readonly" => crate::db::inspect_odoo_postgres(database_url).await,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Mode must be import or sync_readonly".into(),
                }),
            ));
        }
    }
    .map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;

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

    Ok(Json(SuiteCrmConnectResponse {
        status: "ok",
        mode: mode.into(),
        message,
        modules: summary.modules,
        module_counts: summary.module_counts,
        module_count: summary.module_count,
        records: summary.records,
        relationships: summary.relationships,
        metadata_relationships: summary.metadata_relationships,
        warnings: summary.warnings,
    }))
}

pub async fn detect_odoo() -> Json<crate::db::OdooDetection> {
    Json(crate::db::detect_local_odoo())
}

#[cfg(feature = "scanner-module")]
use axum::extract::Multipart;

#[cfg(feature = "scanner-module")]
use crate::vision::{ScanResult, import_scan, scan_image};

#[cfg(feature = "scanner-module")]
pub async fn scan_image_endpoint(
    State(_pool): State<SqlitePool>,
    mut multipart: Multipart,
) -> Result<Json<ScanResult>, (StatusCode, Json<ErrorResponse>)> {
    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        if field.name() == Some("image") {
            let data = field.bytes().await.map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: e.to_string(),
                    }),
                )
            })?;
            let result = scan_image(&data).await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: e.to_string(),
                    }),
                )
            })?;
            return Ok(Json(result));
        }
    }
    Err((
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: "No image uploaded".into(),
        }),
    ))
}

#[cfg(feature = "scanner-module")]
pub async fn import_scan_endpoint(
    State(pool): State<SqlitePool>,
    Path(module): Path<String>,
    Json(data): Json<ScanResult>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let id = import_scan(&pool, &module, &data).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;
    Ok(Json(serde_json::json!({"status": "ok", "id": id})))
}

pub async fn get_module_definition(
    State(pool): State<SqlitePool>,
    Path(module): Path<String>,
) -> Result<Json<ModuleDefinition>, (StatusCode, Json<ErrorResponse>)> {
    crate::module::get_definition(&pool, &module)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?
        .map(Json)
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("No definition for module '{}'", module),
            }),
        ))
}

// ── Create / Update / Delete handlers ──

#[derive(Serialize)]
pub struct CreateRecordResponse {
    pub id: String,
    pub status: &'static str,
}

#[derive(Deserialize)]
pub struct NoteRequest {
    note: String,
}

#[derive(Deserialize)]
pub struct LinkRecordRequest {
    target_module: String,
    target_id: String,
    relationship_type: Option<String>,
}

pub async fn create_record(
    State(pool): State<SqlitePool>,
    Path(module): Path<String>,
    Json(fields): Json<Map<String, Value>>,
) -> Result<Json<CreateRecordResponse>, (StatusCode, Json<ErrorResponse>)> {
    let has_def = crate::module::has_definition(&pool, &module)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;
    if !has_def {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Unknown module '{}'", module),
            }),
        ));
    }

    let id = crate::db::create_record(&pool, &module, &fields)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;

    Ok(Json(CreateRecordResponse { id, status: "ok" }))
}

pub async fn update_record(
    State(pool): State<SqlitePool>,
    Path((module, id)): Path<(String, String)>,
    Json(fields): Json<Map<String, Value>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let has_def = crate::module::has_definition(&pool, &module)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;
    if !has_def {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Unknown module '{}'", module),
            }),
        ));
    }

    let updated = crate::db::update_record(&pool, &module, &id, &fields)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;

    if !updated {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Record '{}' not found in '{}'", id, module),
            }),
        ));
    }

    Ok(Json(serde_json::json!({"status": "ok", "id": id})))
}

pub async fn delete_record(
    State(pool): State<SqlitePool>,
    Path((module, id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let has_def = crate::module::has_definition(&pool, &module)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;
    if !has_def {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Unknown module '{}'", module),
            }),
        ));
    }

    let deleted = crate::db::delete_record(&pool, &module, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;

    if !deleted {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Record '{}' not found in '{}'", id, module),
            }),
        ));
    }

    Ok(Json(serde_json::json!({"status": "ok", "id": id})))
}

pub async fn restore_record(
    State(pool): State<SqlitePool>,
    Path((module, id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let has_def = crate::module::has_definition(&pool, &module)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;
    if !has_def {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Unknown module '{}'", module),
            }),
        ));
    }

    let restored = crate::db::restore_record(&pool, &module, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;

    if !restored {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Record '{}' not found in '{}'", id, module),
            }),
        ));
    }

    Ok(Json(serde_json::json!({"status": "ok", "id": id})))
}

pub async fn add_record_note(
    State(pool): State<SqlitePool>,
    Path((module, id)): Path<(String, String)>,
    Json(payload): Json<NoteRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    if payload.note.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Note cannot be empty".into(),
            }),
        ));
    }

    let added = crate::db::add_record_note(&pool, &module, &id, &payload.note)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;

    if !added {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Record '{}' not found in '{}'", id, module),
            }),
        ));
    }

    Ok(Json(serde_json::json!({"status": "ok", "id": id})))
}

pub async fn link_record(
    State(pool): State<SqlitePool>,
    Path((module, id)): Path<(String, String)>,
    Json(payload): Json<LinkRecordRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let link_id = crate::db::link_records(
        &pool,
        &module,
        &id,
        &payload.target_module,
        &payload.target_id,
        payload.relationship_type.as_deref(),
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;

    if let Some(link_id) = link_id {
        return Ok(Json(serde_json::json!({
            "status": "ok",
            "id": link_id
        })));
    }

    Err((
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: "Could not link those records".into(),
        }),
    ))
}

pub async fn delete_record_link(
    State(pool): State<SqlitePool>,
    Path(link_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let deleted = crate::db::delete_record_link(&pool, &link_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;

    if !deleted {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Link '{}' not found", link_id),
            }),
        ));
    }

    Ok(Json(serde_json::json!({"status": "ok", "id": link_id})))
}

pub async fn export_workspace(
    State(pool): State<SqlitePool>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    crate::db::export_workspace(&pool)
        .await
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })
}

pub async fn export_workspace_archive(
    State(pool): State<SqlitePool>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    crate::db::export_workspace_archive(&pool)
        .await
        .map(|bytes| {
            (
                [
                    (header::CONTENT_TYPE, crate::bundle::archive_content_type()),
                    (
                        header::CONTENT_DISPOSITION,
                        "attachment; filename=\"white-warren-workspace.ironcrm\"",
                    ),
                ],
                bytes,
            )
                .into_response()
        })
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })
}

pub async fn restore_workspace(
    State(pool): State<SqlitePool>,
    Json(bundle): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    crate::db::restore_workspace(&pool, &bundle)
        .await
        .map(|summary| Json(Value::Object(summary)))
        .map_err(|e| {
            let message = e.to_string();
            let status = if message.contains("White Warren workspace bundle")
                || message.contains("Unsupported White Warren bundle version")
            {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(ErrorResponse { error: message }))
        })
}

pub async fn restore_workspace_archive(
    State(pool): State<SqlitePool>,
    body: Bytes,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    restore_workspace_archive_bytes(pool, body, false).await
}

pub async fn restore_workspace_archive_exact(
    State(pool): State<SqlitePool>,
    body: Bytes,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    restore_workspace_archive_bytes(pool, body, true).await
}

async fn restore_workspace_archive_bytes(
    pool: SqlitePool,
    body: Bytes,
    exact: bool,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    crate::db::restore_workspace_archive(&pool, &body, exact)
        .await
        .map(|summary| Json(Value::Object(summary)))
        .map_err(|e| {
            let message = e.to_string();
            let status = if message.contains("White Warren archive")
                || message.contains("White Warren workspace bundle")
                || message.contains("Unsupported White Warren bundle version")
            {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(ErrorResponse { error: message }))
        })
}

pub async fn restore_workspace_exact(
    State(pool): State<SqlitePool>,
    Json(bundle): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    crate::db::restore_workspace_exact(&pool, &bundle)
        .await
        .map(|summary| Json(Value::Object(summary)))
        .map_err(|e| {
            let message = e.to_string();
            let status = if message.contains("White Warren workspace bundle")
                || message.contains("Unsupported White Warren bundle version")
            {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(ErrorResponse { error: message }))
        })
}
