mod api;
mod db;
mod models;

use anyhow::Result;
use axum::{routing::get, Router};
use std::env;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args: Vec<String> = env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("import") => {
            let source_url = env::var("SOURCE_DATABASE_URL")
                .unwrap_or_else(|_| "mysql://suitecrm:suitecrm@localhost:3306/suitecrm".to_string());
            let target_path = env::var("LOCAL_DB_PATH")
                .unwrap_or_else(|_| "ironcrm.db".to_string());

            tracing::info!("Importing from {} -> {}", source_url, target_path);
            db::import_from_mysql(&source_url, &target_path).await?;
            tracing::info!("Import complete.");
            return Ok(());
        }
        _ => {
            // Serve mode
            let db_path = env::var("LOCAL_DB_PATH")
                .unwrap_or_else(|_| "ironcrm.db".to_string());

            tracing::info!("Opening local database: {}", db_path);
            let pool = db::connect_sqlite(&db_path).await?;

            tracing::info!("Discovering modules...");
            let modules = db::discover_modules_sqlite(&pool).await?;
            tracing::info!("Found {} modules: {:?}", modules.len(), modules);

            let app = Router::new()
                .route("/health", get(api::health))
                .route("/api/v1/modules", get(api::list_modules))
                .route("/api/v1/{module}", get(api::list_records))
                .route("/api/v1/{module}/{id}", get(api::get_record))
                .layer(CorsLayer::permissive())
                .layer(TraceLayer::new_for_http())
                .with_state(pool);

            let addr = "0.0.0.0:3000";
            tracing::info!("IronCRM listening on {}", addr);

            let listener = tokio::net::TcpListener::bind(addr).await?;
            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}
