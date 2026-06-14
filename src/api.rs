use crate::module::ModuleDefinition;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
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

pub async fn restore_workspace(
    State(pool): State<SqlitePool>,
    Json(bundle): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    crate::db::restore_workspace(&pool, &bundle)
        .await
        .map(|summary| Json(Value::Object(summary)))
        .map_err(|e| {
            let message = e.to_string();
            let status = if message.contains("IronCRM workspace bundle")
                || message.contains("Unsupported IronCRM bundle version")
            {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (
                status,
                Json(ErrorResponse {
                    error: message,
                }),
            )
        })
}
