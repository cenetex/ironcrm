use crate::api;
use crate::db;
use anyhow::Result;
use axum::{
    Router,
    routing::{delete, get, post, put},
};
use std::path::PathBuf;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

pub async fn run_server(db_path: &str, port: u16) -> Result<()> {
    let pool = db::connect_sqlite(db_path).await?;

    let modules = db::discover_modules_sqlite(&pool).await?;
    tracing::info!("Serving {} modules on port {}", modules.len(), port);

    let frontend_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("frontend")))
        .filter(|d| d.exists())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("frontend"));

    let mut app = Router::new()
        .route("/health", get(api::health))
        .route("/api/v1/modules", get(api::list_modules))
        .route("/api/v1/search", get(api::search_records))
        .route("/api/v1/suitecrm/detect", get(api::detect_suitecrm))
        .route("/api/v1/suitecrm/connect", post(api::connect_suitecrm))
        .route("/api/v1/odoo/detect", get(api::detect_odoo))
        .route("/api/v1/odoo/connect", post(api::connect_odoo))
        .route("/api/v1/export", get(api::export_workspace))
        .route("/api/v1/restore", post(api::restore_workspace))
        .route("/api/v1/restore/exact", post(api::restore_workspace_exact))
        .route("/api/v1/links/{link_id}", delete(api::delete_record_link))
        .route(
            "/api/v1/{module}/definition",
            get(api::get_module_definition),
        )
        .route("/api/v1/{module}", get(api::list_records))
        .route("/api/v1/{module}", post(api::create_record))
        .route("/api/v1/{module}/{id}", get(api::get_record))
        .route("/api/v1/{module}/{id}", put(api::update_record))
        .route("/api/v1/{module}/{id}", delete(api::delete_record))
        .route("/api/v1/{module}/{id}/notes", post(api::add_record_note))
        .route("/api/v1/{module}/{id}/links", post(api::link_record));

    #[cfg(feature = "scanner-module")]
    {
        app = app
            .route(
                "/api/v1/modules/scanner/scan",
                post(api::scan_image_endpoint),
            )
            .route(
                "/api/v1/modules/scanner/{module}/import",
                post(api::import_scan_endpoint),
            );
    }

    if frontend_dir.exists() {
        app = app.fallback_service(ServeDir::new(&frontend_dir));
    }

    let app = app
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(pool);

    let addr = format!("127.0.0.1:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("IronCRM HTTP API on {}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}
