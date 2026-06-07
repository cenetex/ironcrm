use anyhow::Result;
use axum::{routing::{get, post}, Router};
use sqlx::sqlite::SqlitePoolOptions;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

pub mod api;
pub mod db;
pub mod models;
mod vision;

pub async fn run_server(db_path: &str, port: u16) -> Result<()> {
    let db_url = format!("sqlite:{}", db_path);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;

    // Ensure schema
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS records (
            module TEXT NOT NULL, id TEXT NOT NULL, data TEXT NOT NULL,
            PRIMARY KEY (module, id)
        )"
    ).execute(&pool).await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_records_module ON records(module)"
    ).execute(&pool).await?;

    let modules = db::discover_modules_sqlite(&pool).await?;
    tracing::info!("Serving {} modules on port {}", modules.len(), port);

    let app = Router::new()
        .route("/health", get(api::health))
        .route("/api/v1/modules", get(api::list_modules))
        .route("/api/v1/{module}", get(api::list_records))
        .route("/api/v1/{module}/{id}", get(api::get_record))
        .route("/api/v1/scan", post(api::scan_image_endpoint))
        .route("/api/v1/{module}/import", post(api::import_scan_endpoint))
        .fallback_service(ServeDir::new("/Users/ratimics/develop/ironcrm/frontend"))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(pool);

    let addr = format!("127.0.0.1:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("IronCRM API on {}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}
