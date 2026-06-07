use anyhow::Result;
use std::env;
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
            ironcrm::db::import_from_mysql(&source_url, &target_path).await?;
            tracing::info!("Import complete.");
            return Ok(());
        }
        _ => {
            let db_path = env::var("LOCAL_DB_PATH").unwrap_or_else(|_| "ironcrm.db".to_string());
            let port: u16 = env::var("PORT").unwrap_or_else(|_| "3000".to_string()).parse()?;
            ironcrm::run_server(&db_path, port).await?;
        }
    }
    Ok(())
}
