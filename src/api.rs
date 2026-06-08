use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::sqlite::SqlitePool;
use crate::module::ModuleDefinition;

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
}

#[derive(Deserialize)]
pub struct Pagination {
    limit: Option<u32>,
    offset: Option<u32>,
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
    let definitions = crate::module::get_all_definitions(&pool).await.unwrap_or_default();
    Json(ModuleListResponse { modules, count, definitions })
}

pub async fn list_records(
    State(pool): State<SqlitePool>,
    Path(module): Path<String>,
    Query(pagination): Query<Pagination>,
) -> Result<Json<ModuleRecordsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let limit = pagination.limit.unwrap_or(25).min(100);
    let offset = pagination.offset.unwrap_or(0);

    let modules = crate::db::discover_modules_sqlite(&pool).await.unwrap_or_default();
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
    let modules = crate::db::discover_modules_sqlite(&pool).await.unwrap_or_default();
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

    Ok(Json(ModuleRecordResponse {
        module,
        data,
        relationships,
    }))
}

use axum::extract::Multipart;
use crate::vision::{scan_image, import_scan, ScanResult};

pub async fn scan_image_endpoint(
    State(_pool): State<SqlitePool>,
    mut multipart: Multipart,
) -> Result<Json<ScanResult>, (StatusCode, Json<ErrorResponse>)> {
    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        if field.name() == Some("image") {
            let data = field.bytes().await.map_err(|e| (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse { error: e.to_string() })
            ))?;
            let result = scan_image(&data).await.map_err(|e| (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e.to_string() })
            ))?;
            return Ok(Json(result));
        }
    }
    Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "No image uploaded".into() })))
}

pub async fn import_scan_endpoint(
    State(pool): State<SqlitePool>,
    Path(module): Path<String>,
    Json(data): Json<ScanResult>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let id = import_scan(&pool, &module, &data).await.map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse { error: e.to_string() })
    ))?;
    Ok(Json(serde_json::json!({"status": "ok", "id": id})))
}

pub async fn get_module_definition(
    State(pool): State<SqlitePool>,
    Path(module): Path<String>,
) -> Result<Json<ModuleDefinition>, (StatusCode, Json<ErrorResponse>)> {
    crate::module::get_definition(&pool, &module)
        .await
        .map_err(|e| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e.to_string() }),
        ))?
        .map(Json)
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse { error: format!("No definition for module '{}'", module) }),
        ))
}
