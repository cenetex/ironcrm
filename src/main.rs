use anyhow::Context;
use anyhow::Result;
use std::env;
use std::path::PathBuf;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use ironcrm::licensing::{LicenseKey, Licensor};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    let args: Vec<String> = env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("import") => {
            // ... existing import code ...
            let source_url = env::var("SOURCE_DATABASE_URL").unwrap_or_else(|_| {
                "mysql://suitecrm:suitecrm@localhost:3306/suitecrm".to_string()
            });
            let target_path = local_db_path();
            tracing::info!("Importing from {} -> {}", source_url, target_path);
            ironcrm::db::import_from_mysql(&source_url, &target_path).await?;
            tracing::info!("Import complete.");
        }
        Some("mcp") => {
            let db_path = local_db_path();
            tracing::info!(%db_path, "Starting IronCRM MCP server");
            ironcrm::mcp::run_mcp_server(&db_path).await?;
        }
        Some("license") => {
            run_license_cli(&args[1..]).await?;
        }
        #[cfg(feature = "http-server")]
        _ => {
            let db_path = local_db_path();
            let port: u16 = env::var("PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()?;
            ironcrm::run_server(&db_path, port).await?;
        }
        #[cfg(not(feature = "http-server"))]
        _ => {
            tracing::info!("IronCRM — local-first CRM engine");
            tracing::info!("The desktop app runs via the Tauri shell (cd src-tauri && cargo run).");
            tracing::info!("For the HTTP API plugin: cargo run --features http-server");
            tracing::info!("For SuiteCRM import: cargo run -- import");
        }
    }
    Ok(())
}

fn local_db_path() -> String {
    env::var("LOCAL_DB_PATH")
        .or_else(|_| env::var("IRONCRM_DB"))
        .ok()
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
        .map(expand_home_path)
        .unwrap_or_else(default_native_db_path)
}

fn expand_home_path(path: String) -> String {
    if path == "~" {
        return env::var("HOME").unwrap_or(path);
    }

    if let Some(rest) = path.strip_prefix("~/")
        && let Ok(home) = env::var("HOME")
    {
        return PathBuf::from(home).join(rest).to_string_lossy().to_string();
    }

    path
}

fn default_native_db_path() -> String {
    if let Ok(home) = env::var("HOME") {
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("IronCRM")
            .join("ironcrm.db")
            .to_string_lossy()
            .to_string()
    } else {
        "ironcrm.db".into()
    }
}

async fn run_license_cli(args: &[String]) -> Result<()> {
    let registry_path: PathBuf = env::var("LICENSE_REGISTRY")
        .unwrap_or_else(|_| "keys/registry.json".into())
        .into();
    let chain_path: PathBuf = env::var("LICENSE_CHAIN")
        .unwrap_or_else(|_| "keys/holonet.json".into())
        .into();

    match args.get(1).map(|s| s.as_str()) {
        Some("generate") => {
            let count: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
            let mut licensor = Licensor::new(&registry_path, &chain_path)?;
            for _ in 0..count {
                let key = LicenseKey::generate();
                licensor.issue_key(&key, None)?;
                println!("  {}", key);
            }
            println!(
                "  {} key(s) generated. Registry: {} entries. Chain: {} blocks.",
                count,
                licensor.key_count(),
                licensor.chain_length()
            );
        }
        Some("validate") => {
            let key_str = args
                .get(2)
                .context("usage: cargo run -- license validate <KEY>")?;
            let key = LicenseKey::parse(key_str).context("invalid key format")?;
            let licensor = Licensor::new(&registry_path, &chain_path)?;
            if licensor.validate_key(&key) {
                println!("  {}  [VALID]", key_str);
            } else {
                println!("  {}  [INVALID]", key_str);
            }
        }
        Some("revoke") => {
            let key_str = args
                .get(2)
                .context("usage: cargo run -- license revoke <KEY>")?;
            let key = LicenseKey::parse(key_str).context("invalid key format")?;
            let mut licensor = Licensor::new(&registry_path, &chain_path)?;
            if licensor.revoke_key(&key)? {
                println!("  {}  revoked.", key_str);
            } else {
                println!("  {}  not found in registry.", key_str);
            }
        }
        Some("list") => {
            let licensor = Licensor::new(&registry_path, &chain_path)?;
            println!("  Registry: {} key(s)", licensor.key_count());
            for entry in licensor.registry().keys.iter() {
                println!(
                    "  {}  salt={}...  hash={}...",
                    entry.id,
                    &entry.salt[..8],
                    &entry.hash[..16]
                );
            }
            println!("  Holonet: {} block(s)", licensor.chain_length());
            let (valid, idx, msg) = licensor.verify_chain();
            println!(
                "  Chain integrity: {} (block {}, {})",
                if valid { "OK" } else { "FAIL" },
                idx,
                msg
            );
        }
        Some("verify-chain") => {
            let licensor = Licensor::new(&registry_path, &chain_path)?;
            let (valid, idx, msg) = licensor.verify_chain();
            if valid {
                println!(
                    "  Holonet chain OK — {} blocks verified.",
                    licensor.chain_length()
                );
            } else {
                println!("  Chain integrity FAILED at block {}: {}", idx, msg);
            }
        }
        Some("export-chain") => {
            let licensor = Licensor::new(&registry_path, &chain_path)?;
            println!("{}", licensor.chain_json());
        }
        _ => {
            println!("License CLI commands:");
            println!("  cargo run -- license generate [count]    Generate keys");
            println!("  cargo run -- license validate <KEY>      Validate a key");
            println!("  cargo run -- license revoke <KEY>        Revoke a key");
            println!("  cargo run -- license list                List keys and chain status");
            println!("  cargo run -- license verify-chain        Verify holonet integrity");
            println!("  cargo run -- license export-chain        Export holonet JSON");
        }
    }
    Ok(())
}
