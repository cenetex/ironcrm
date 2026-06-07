use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::mysql::MySqlPool;

// ── Types ──

#[derive(Serialize)]
pub(crate) struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
pub(crate) struct ModuleListResponse {
    modules: Vec<String>,
    count: usize,
}

#[derive(Serialize)]
pub(crate) struct ModuleRecordsResponse {
    module: String,
    data: Vec<Map<String, Value>>,
    total: i64,
    limit: u32,
    offset: u32,
}

#[derive(Serialize)]
pub(crate) struct ModuleRecordResponse {
    module: String,
    data: Option<Map<String, Value>>,
    relationships: Vec<Map<String, Value>>,
}

#[derive(Deserialize)]
pub(crate) struct Pagination {
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Serialize)]
pub(crate) struct ErrorResponse {
    error: String,
}

// ── Handlers ──

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

pub async fn list_modules(State(pool): State<MySqlPool>) -> Json<ModuleListResponse> {
    let modules = crate::db::discover_modules(&pool)
        .await
        .unwrap_or_default();
    let count = modules.len();
    Json(ModuleListResponse { modules, count })
}

pub async fn list_records(
    State(pool): State<MySqlPool>,
    Path(module): Path<String>,
    Query(pagination): Query<Pagination>,
) -> Result<Json<ModuleRecordsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let limit = pagination.limit.unwrap_or(25).min(100);
    let offset = pagination.offset.unwrap_or(0);

    let modules = crate::db::discover_modules(&pool).await.unwrap_or_default();
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
    State(pool): State<MySqlPool>,
    Path((module, id)): Path<(String, String)>,
) -> Result<Json<ModuleRecordResponse>, (StatusCode, Json<ErrorResponse>)> {
    let modules = crate::db::discover_modules(&pool).await.unwrap_or_default();
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
