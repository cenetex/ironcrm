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

    let database_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "mysql://suitecrm:suitecrm@localhost:3306/suitecrm".to_string());

    tracing::info!("Connecting to database...");
    let pool = db::connect(&database_url).await?;

    tracing::info!("Discovering modules...");
    let modules = db::discover_modules(&pool).await?;
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

    Ok(())
}
