use anyhow::Result;
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::mysql::MySqlPool;
use sqlx::postgres::PgPool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::{Column, Row};
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const SUITECRM_CORE_MODULES: &[&str] = &["accounts", "contacts", "opportunities", "leads", "cases"];

// ── SQLite connection + schema ──

pub async fn connect_sqlite(path: &str) -> Result<SqlitePool> {
    let url = if path == ":memory:" || path.starts_with("sqlite:") {
        path.to_string()
    } else {
        format!("sqlite://{}", path)
    };
    let options = SqliteConnectOptions::from_str(&url)?.create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;
    init_schema(&pool).await?;
    migrate_blob_to_typed(&pool).await?;
    Ok(pool)
}

pub async fn init_schema(pool: &SqlitePool) -> Result<()> {
    // ── Blob store (catch-all for junction tables, unknown modules, relationships) ──
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS records (
            module TEXT NOT NULL, id TEXT NOT NULL, data TEXT NOT NULL,
            PRIMARY KEY (module, id)
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_records_module ON records(module)")
        .execute(pool)
        .await?;

    // ── Module definitions (must precede typed-table creation) ──
    crate::module::init_definitions_table(pool).await?;
    crate::module::seed_builtin_definitions(pool).await?;

    // ── Typed tables (generated from definitions) ──
    let defs = crate::module::get_all_definitions(pool).await?;
    for def in &defs {
        let sql = crate::module::gen_create_table_sql(def);
        sqlx::query(&sql).execute(pool).await?;

        for idx_sql in crate::module::gen_index_sqls(def) {
            sqlx::query(&idx_sql).execute(pool).await?;
        }
    }

    // ── Portability tables ──
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS consent_receipts (
            contact_id TEXT PRIMARY KEY,
            credential_json TEXT NOT NULL,
            issued_at TEXT NOT NULL DEFAULT (datetime('now')),
            expires_at TEXT
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS activity_log (
            id TEXT PRIMARY KEY,
            module TEXT NOT NULL,
            record_id TEXT NOT NULL,
            operation TEXT NOT NULL,
            actor_id TEXT,
            changed_fields TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_activity_log_record
         ON activity_log(module, record_id, created_at)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS record_links (
            id TEXT PRIMARY KEY,
            source_module TEXT NOT NULL,
            source_id TEXT NOT NULL,
            target_module TEXT NOT NULL,
            target_id TEXT NOT NULL,
            relationship_type TEXT NOT NULL DEFAULT 'related',
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_record_links_source
         ON record_links(source_module, source_id)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_record_links_target
         ON record_links(target_module, target_id)",
    )
    .execute(pool)
    .await?;

    // Clean up development debris
    sqlx::query("DROP TABLE IF EXISTS test")
        .execute(pool)
        .await?;

    Ok(())
}

// ── Migration: blob store → typed tables ──

pub async fn migrate_blob_to_typed(pool: &SqlitePool) -> Result<()> {
    let defs = crate::module::get_all_definitions(pool).await?;
    for module in defs.iter().map(|d| &*d.module) {
        let count_sql = format!("SELECT COUNT(*) as cnt FROM {}", module);
        let row: (i64,) = sqlx::query_as(&count_sql).fetch_one(pool).await?;
        if row.0 > 0 {
            continue;
        }

        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, data FROM records WHERE module = ? AND IFNULL(json_extract(data, '$.deleted'), '0') != '1'",
        )
        .bind(module)
        .fetch_all(pool)
        .await?;

        if rows.is_empty() {
            continue;
        }

        let mut count = 0u64;
        for (id, json) in &rows {
            let data: Map<String, Value> = match serde_json::from_str(json) {
                Ok(d) => d,
                Err(_) => continue,
            };
            if let Err(e) = upsert_typed_row(pool, module, id, &data).await {
                tracing::warn!("  failed to migrate {}/{}: {}", module, id, e);
                continue;
            }
            count += 1;
        }
        tracing::info!("  migrated {}: {} records", module, count);
    }
    Ok(())
}

/// Upsert a row into a typed table given a JSON map of column values.
/// Only includes keys valid for the target module; drops unknowns silently.
pub async fn upsert_typed_row(
    pool: &SqlitePool,
    module: &str,
    id: &str,
    data: &Map<String, Value>,
) -> Result<()> {
    let valid_keys = crate::module::typed_column_names(pool, module).await?;

    let mut map = Map::new();
    map.insert("id".into(), Value::String(id.to_string()));
    for key in &valid_keys {
        if key == "id" {
            continue;
        }
        if let Some(val) = data.get(key.as_str()) {
            if val.is_null() {
                continue; // let SQLite DEFAULT handle it
            }
            map.insert(key.clone(), val.clone());
        }
    }

    let keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
    let placeholders: Vec<String> = (1..=keys.len()).map(|i| format!("?{}", i)).collect();
    let sql = format!(
        "INSERT OR REPLACE INTO {} ({}) VALUES ({})",
        module,
        keys.iter()
            .map(|k| format!("\"{}\"", k))
            .collect::<Vec<_>>()
            .join(", "),
        placeholders.join(", ")
    );

    let mut query = sqlx::query(&sql);
    for key in &keys {
        let val = map
            .get(*key)
            .and_then(|v| {
                if v.is_string() {
                    v.as_str().map(|s| s.to_string())
                } else if v.is_number() || v.is_boolean() {
                    Some(v.to_string())
                } else if v.is_null() {
                    None
                } else {
                    Some(v.to_string())
                }
            })
            .unwrap_or_default();
        query = query.bind(val);
    }
    query.execute(pool).await?;
    Ok(())
}

fn sqlite_row_to_map(row: &sqlx::sqlite::SqliteRow) -> Map<String, Value> {
    let mut map = Map::new();
    for col in row.columns() {
        let name = col.name().to_string();
        let value = if let Ok(Some(val)) = row.try_get::<Option<String>, _>(name.as_str()) {
            Value::String(val)
        } else if let Ok(Some(val)) = row.try_get::<Option<i64>, _>(name.as_str()) {
            Value::Number(val.into())
        } else if let Ok(Some(val)) = row.try_get::<Option<f64>, _>(name.as_str()) {
            serde_json::Number::from_f64(val)
                .map(Value::Number)
                .unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        map.insert(name, value);
    }
    map
}

// ── MySQL connection (import source only) ──

#[derive(Debug, Clone, Serialize)]
pub struct SuiteCrmModuleCount {
    pub module: String,
    pub label: String,
    pub records: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SuiteCrmImportSummary {
    pub modules: Vec<String>,
    pub module_counts: Vec<SuiteCrmModuleCount>,
    pub module_count: usize,
    pub records: u64,
    pub relationships: u64,
    pub metadata_relationships: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OdooDetection {
    pub found: bool,
    pub message: String,
    pub database_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SuiteCrmDetection {
    pub found: bool,
    pub message: String,
    pub config_path: Option<String>,
    pub database_url: Option<String>,
    pub database_name: Option<String>,
    pub host: Option<String>,
}

pub async fn connect_mysql(database_url: &str) -> Result<MySqlPool> {
    let pool = MySqlPool::connect(database_url).await?;
    Ok(pool)
}

pub async fn connect_postgres(database_url: &str) -> Result<PgPool> {
    let pool = PgPool::connect(database_url).await?;
    Ok(pool)
}

pub fn detect_local_suitecrm() -> SuiteCrmDetection {
    for candidate in suitecrm_config_candidates() {
        if !candidate.is_file() {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&candidate) else {
            continue;
        };
        if let Some(detection) = detection_from_config(&candidate, &contents) {
            return detection;
        }
    }

    SuiteCrmDetection {
        found: false,
        message: "No local SuiteCRM config.php found in common locations.".into(),
        config_path: None,
        database_url: None,
        database_name: None,
        host: None,
    }
}

fn suitecrm_config_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = env::var("SUITECRM_CONFIG_PATH") {
        candidates.push(PathBuf::from(path));
    }

    if let Ok(cwd) = env::current_dir() {
        add_standard_config_candidates(&mut candidates, &cwd);
        if let Some(parent) = cwd.parent() {
            add_standard_config_candidates(&mut candidates, parent);
        }
    }
    add_standard_config_candidates(&mut candidates, Path::new(env!("CARGO_MANIFEST_DIR")));

    if let Ok(home) = env::var("HOME") {
        let home = PathBuf::from(home);
        for root in [
            home.join("Sites"),
            home.join("www"),
            home.join("htdocs"),
            home.join("code"),
            home.join("develop"),
            home.join("Downloads"),
        ] {
            add_standard_config_candidates(&mut candidates, &root);
            add_child_config_candidates(&mut candidates, &root);
        }
    }

    for path in [
        "/Applications/MAMP/htdocs/suitecrm/config.php",
        "/Applications/MAMP/htdocs/SuiteCRM/config.php",
        "/opt/homebrew/var/www/suitecrm/config.php",
        "/opt/homebrew/var/www/SuiteCRM/config.php",
        "/usr/local/var/www/suitecrm/config.php",
        "/usr/local/var/www/SuiteCRM/config.php",
        "/var/www/html/suitecrm/config.php",
        "/var/www/html/SuiteCRM/config.php",
        "/var/www/suitecrm/config.php",
        "/var/www/SuiteCRM/config.php",
    ] {
        candidates.push(PathBuf::from(path));
    }

    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn add_standard_config_candidates(candidates: &mut Vec<PathBuf>, root: &Path) {
    candidates.push(root.join("config.php"));
    candidates.push(root.join("suitecrm").join("config.php"));
    candidates.push(root.join("SuiteCRM").join("config.php"));
}

fn add_child_config_candidates(candidates: &mut Vec<PathBuf>, root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten().take(80) {
        let path = entry.path();
        if path.is_dir() {
            add_standard_config_candidates(candidates, &path);
        }
    }
}

fn detection_from_config(path: &Path, contents: &str) -> Option<SuiteCrmDetection> {
    if !contents.contains("$sugar_config") || !contents.contains("dbconfig") {
        return None;
    }

    let db_name = php_config_value(contents, "db_name")?;
    let user = php_config_value(contents, "db_user_name")?;
    let password = php_config_value(contents, "db_password").unwrap_or_default();
    let mut host = php_config_value(contents, "db_host_name").unwrap_or_else(|| "localhost".into());
    if host.trim().is_empty() {
        host = "localhost".into();
    }
    let port = php_config_value(contents, "db_port").filter(|value| !value.trim().is_empty());
    let db_type = php_config_value(contents, "db_type").unwrap_or_else(|| "mysql".into());
    if !db_type.to_lowercase().contains("mysql") {
        return None;
    }

    let host_with_port = if host.contains(':') || host.starts_with('/') {
        host.clone()
    } else if let Some(port) = port {
        format!("{host}:{port}")
    } else {
        format!("{host}:3306")
    };
    let database_url = format!(
        "mysql://{}:{}@{}/{}",
        url_component(&user),
        url_component(&password),
        host_with_port,
        url_component(&db_name)
    );

    Some(SuiteCrmDetection {
        found: true,
        message: format!("Detected SuiteCRM config at {}", path.display()),
        config_path: Some(path.display().to_string()),
        database_url: Some(database_url),
        database_name: Some(db_name),
        host: Some(host_with_port),
    })
}

fn php_config_value(contents: &str, key: &str) -> Option<String> {
    let key_pos = contents
        .find(&format!("'{}'", key))
        .or_else(|| contents.find(&format!("\"{}\"", key)))?;
    let after_key = &contents[key_pos + key.len() + 2..];
    let arrow_pos = after_key.find("=>")?;
    let mut value = after_key[arrow_pos + 2..].trim_start();
    if value.starts_with("array") {
        return None;
    }
    let quote = value.chars().next()?;
    if quote == '\'' || quote == '"' {
        value = &value[quote.len_utf8()..];
        let end = value.find(quote)?;
        return Some(value[..end].to_string());
    }
    let end = value.find([',', '\n', '\r']).unwrap_or(value.len());
    Some(value[..end].trim().trim_matches(';').to_string())
}

fn url_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        let keep = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        if keep {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

// ── Import: MySQL → SQLite ──

pub async fn inspect_suitecrm_mysql(source_url: &str) -> Result<SuiteCrmImportSummary> {
    let mysql = connect_mysql(source_url).await?;
    inspect_suitecrm_source(&mysql).await
}

pub async fn import_from_mysql(
    source_url: &str,
    target_path: &str,
) -> Result<SuiteCrmImportSummary> {
    let sqlite = connect_sqlite(target_path).await?;
    import_from_mysql_into_pool(source_url, &sqlite).await
}

pub async fn import_from_mysql_into_pool(
    source_url: &str,
    sqlite: &SqlitePool,
) -> Result<SuiteCrmImportSummary> {
    let mysql = connect_mysql(source_url).await?;

    let mut summary = inspect_suitecrm_source(&mysql).await?;
    let modules = summary.modules.clone();
    tracing::info!("Importing {} modules...", modules.len());

    let mut records = 0u64;
    for module in &modules {
        records += import_module(&mysql, sqlite, module).await?;
    }

    let relationship_definitions = import_relationships(&mysql, sqlite).await?;
    let relationship_links = import_core_relationship_links(&mysql, sqlite).await?;
    summary.records = records;
    summary.relationships = relationship_links;
    summary.metadata_relationships = relationship_definitions;
    Ok(summary)
}

// ── Import: Odoo PostgreSQL -> SQLite ──

pub async fn inspect_odoo_postgres(source_url: &str) -> Result<SuiteCrmImportSummary> {
    let postgres = connect_postgres(source_url).await?;
    inspect_odoo_source(&postgres).await
}

pub async fn import_from_odoo_into_pool(
    source_url: &str,
    sqlite: &SqlitePool,
) -> Result<SuiteCrmImportSummary> {
    let postgres = connect_postgres(source_url).await?;

    let mut summary = inspect_odoo_source(&postgres).await?;
    let mut records = 0u64;
    records += import_odoo_accounts(&postgres, sqlite).await?;
    records += import_odoo_contacts(&postgres, sqlite).await?;
    records += import_odoo_opportunities(&postgres, sqlite).await?;
    records += import_odoo_leads(&postgres, sqlite).await?;
    summary.records = records;
    summary.relationships = import_odoo_relationship_links(&postgres, sqlite).await?;
    Ok(summary)
}

pub fn detect_local_odoo() -> OdooDetection {
    if let Ok(database_url) = env::var("ODOO_DATABASE_URL")
        && !database_url.trim().is_empty()
    {
        return OdooDetection {
            found: true,
            message: "Using ODOO_DATABASE_URL from the environment.".into(),
            database_url: Some(database_url),
        };
    }

    OdooDetection {
        found: true,
        message: "Use the bundled sandbox connection string for dev/odoo.".into(),
        database_url: Some("postgres://odoo:odoo@127.0.0.1:5433/odoo".into()),
    }
}

async fn inspect_odoo_source(postgres: &PgPool) -> Result<SuiteCrmImportSummary> {
    let mut module_counts = Vec::new();
    let mut modules = Vec::new();
    let mut records = 0u64;
    let mut warnings = Vec::new();

    if !pg_table_exists(postgres, "res_partner").await? {
        warnings.push("Odoo table res_partner was not found.".into());
    } else {
        let accounts = count_pg_rows(
            postgres,
            "res_partner",
            "WHERE active IS DISTINCT FROM false AND COALESCE(is_company, false) = true",
        )
        .await?;
        let contacts = count_pg_rows(
            postgres,
            "res_partner",
            "WHERE active IS DISTINCT FROM false AND COALESCE(is_company, false) = false",
        )
        .await?;
        modules.push("accounts".into());
        module_counts.push(SuiteCrmModuleCount {
            module: "accounts".into(),
            label: "Accounts".into(),
            records: accounts,
        });
        modules.push("contacts".into());
        module_counts.push(SuiteCrmModuleCount {
            module: "contacts".into(),
            label: "Contacts".into(),
            records: contacts,
        });
        records += accounts + contacts;
    }

    if !pg_table_exists(postgres, "crm_lead").await? {
        warnings.push("Odoo CRM table crm_lead was not found. Install Odoo CRM before importing leads or opportunities.".into());
    } else {
        let opportunities = count_pg_rows(
            postgres,
            "crm_lead",
            "WHERE active IS DISTINCT FROM false AND type = 'opportunity'",
        )
        .await?;
        let leads = count_pg_rows(
            postgres,
            "crm_lead",
            "WHERE active IS DISTINCT FROM false AND COALESCE(type, 'lead') != 'opportunity'",
        )
        .await?;
        modules.push("opportunities".into());
        module_counts.push(SuiteCrmModuleCount {
            module: "opportunities".into(),
            label: "Opportunities".into(),
            records: opportunities,
        });
        modules.push("leads".into());
        module_counts.push(SuiteCrmModuleCount {
            module: "leads".into(),
            label: "Leads".into(),
            records: leads,
        });
        records += opportunities + leads;
    }

    let relationships = count_odoo_relationship_links(postgres).await?;
    if records == 0 {
        warnings.push("No active Odoo CRM records were found in the selected database.".into());
    }

    Ok(SuiteCrmImportSummary {
        module_count: modules.len(),
        modules,
        module_counts,
        records,
        relationships,
        metadata_relationships: 0,
        warnings,
    })
}

async fn import_odoo_accounts(postgres: &PgPool, sqlite: &SqlitePool) -> Result<u64> {
    if !pg_table_exists(postgres, "res_partner").await? {
        return Ok(0);
    }
    let rows = sqlx::query(
        "SELECT id::text AS source_id, name, email, phone, mobile, website, street, city, zip,
                create_date::text AS create_date, write_date::text AS write_date
         FROM res_partner
         WHERE active IS DISTINCT FROM false AND COALESCE(is_company, false) = true",
    )
    .fetch_all(postgres)
    .await?;

    let mut count = 0u64;
    for row in rows {
        let source_id = pg_string(&row, "source_id").unwrap_or_default();
        if source_id.trim().is_empty() {
            continue;
        }
        let id = odoo_record_id("res_partner", &source_id);
        let mut map = odoo_base_map("res_partner", &source_id);
        put_pg_string(&mut map, &row, "name", "name");
        put_pg_string(&mut map, &row, "email", "email1");
        put_pg_string(&mut map, &row, "phone", "phone_office");
        put_pg_string(&mut map, &row, "mobile", "phone_alternate");
        put_pg_string(&mut map, &row, "website", "website");
        put_pg_string(&mut map, &row, "street", "billing_address_street");
        put_pg_string(&mut map, &row, "city", "billing_address_city");
        put_pg_string(&mut map, &row, "zip", "billing_address_postalcode");
        put_pg_string(&mut map, &row, "create_date", "date_entered");
        put_pg_string(&mut map, &row, "write_date", "date_modified");
        store_imported_record(sqlite, "accounts", &id, &map).await?;
        count += 1;
    }
    Ok(count)
}

async fn import_odoo_contacts(postgres: &PgPool, sqlite: &SqlitePool) -> Result<u64> {
    if !pg_table_exists(postgres, "res_partner").await? {
        return Ok(0);
    }
    let rows = sqlx::query(
        "SELECT id::text AS source_id, parent_id::text AS parent_id, name, email, phone, mobile,
                function AS title, street, city, zip, create_date::text AS create_date,
                write_date::text AS write_date
         FROM res_partner
         WHERE active IS DISTINCT FROM false AND COALESCE(is_company, false) = false",
    )
    .fetch_all(postgres)
    .await?;

    let mut count = 0u64;
    for row in rows {
        let source_id = pg_string(&row, "source_id").unwrap_or_default();
        if source_id.trim().is_empty() {
            continue;
        }
        let id = odoo_record_id("res_partner", &source_id);
        let mut map = odoo_base_map("res_partner", &source_id);
        split_odoo_name(pg_string(&row, "name").as_deref(), &mut map);
        put_pg_string(&mut map, &row, "email", "email1");
        put_pg_string(&mut map, &row, "phone", "phone_work");
        put_pg_string(&mut map, &row, "mobile", "phone_mobile");
        put_pg_string(&mut map, &row, "title", "title");
        put_pg_string(&mut map, &row, "street", "primary_address_street");
        put_pg_string(&mut map, &row, "city", "primary_address_city");
        put_pg_string(&mut map, &row, "zip", "primary_address_postalcode");
        put_pg_string(&mut map, &row, "create_date", "date_entered");
        put_pg_string(&mut map, &row, "write_date", "date_modified");
        if let Some(parent_id) = pg_string(&row, "parent_id") {
            map.insert(
                "account_id".into(),
                Value::String(odoo_record_id("res_partner", &parent_id)),
            );
        }
        store_imported_record(sqlite, "contacts", &id, &map).await?;
        count += 1;
    }
    Ok(count)
}

async fn import_odoo_opportunities(postgres: &PgPool, sqlite: &SqlitePool) -> Result<u64> {
    import_odoo_crm_leads(postgres, sqlite, "opportunities", true).await
}

async fn import_odoo_leads(postgres: &PgPool, sqlite: &SqlitePool) -> Result<u64> {
    import_odoo_crm_leads(postgres, sqlite, "leads", false).await
}

async fn import_odoo_crm_leads(
    postgres: &PgPool,
    sqlite: &SqlitePool,
    module: &str,
    opportunities: bool,
) -> Result<u64> {
    if !pg_table_exists(postgres, "crm_lead").await? {
        return Ok(0);
    }
    let clause = if opportunities {
        "type = 'opportunity'"
    } else {
        "COALESCE(type, 'lead') != 'opportunity'"
    };
    let sql = format!(
        "SELECT lead.id::text AS source_id, lead.partner_id::text AS partner_id, lead.name,
                lead.contact_name, lead.partner_name, lead.email_from, lead.phone, lead.mobile,
                lead.description, lead.expected_revenue::text AS expected_revenue,
                lead.probability::text AS probability, lead.date_deadline::text AS date_deadline,
                lead.create_date::text AS create_date, lead.write_date::text AS write_date,
                stage.name::text AS stage_name
         FROM crm_lead lead
         LEFT JOIN crm_stage stage ON stage.id = lead.stage_id
         WHERE lead.active IS DISTINCT FROM false AND {clause}"
    );
    let rows = sqlx::query(&sql).fetch_all(postgres).await?;

    let mut count = 0u64;
    for row in rows {
        let source_id = pg_string(&row, "source_id").unwrap_or_default();
        if source_id.trim().is_empty() {
            continue;
        }
        let id = odoo_record_id("crm_lead", &source_id);
        let mut map = odoo_base_map("crm_lead", &source_id);
        put_pg_string(&mut map, &row, "name", "name");
        put_pg_string(&mut map, &row, "description", "description");
        put_pg_string(&mut map, &row, "create_date", "date_entered");
        put_pg_string(&mut map, &row, "write_date", "date_modified");
        if let Some(partner_id) = pg_string(&row, "partner_id") {
            map.insert(
                "account_id".into(),
                Value::String(odoo_record_id("res_partner", &partner_id)),
            );
        }

        if module == "opportunities" {
            put_pg_string(&mut map, &row, "expected_revenue", "amount");
            put_pg_string(&mut map, &row, "expected_revenue", "amount_usdollar");
            put_pg_string(&mut map, &row, "probability", "probability");
            put_pg_string(&mut map, &row, "date_deadline", "date_closed");
            if let Some(stage) = pg_string(&row, "stage_name") {
                map.insert("sales_stage".into(), Value::String(clean_odoo_text(&stage)));
            } else {
                map.insert("sales_stage".into(), Value::String("Qualification".into()));
            }
        } else {
            split_odoo_name(pg_string(&row, "contact_name").as_deref(), &mut map);
            if let Some(company) = pg_string(&row, "partner_name") {
                map.insert("account_name".into(), Value::String(company));
            }
            put_pg_string(&mut map, &row, "email_from", "email1");
            put_pg_string(&mut map, &row, "phone", "phone_work");
            put_pg_string(&mut map, &row, "mobile", "phone_mobile");
            if let Some(stage) = pg_string(&row, "stage_name") {
                map.insert("status".into(), Value::String(clean_odoo_text(&stage)));
            } else {
                map.insert("status".into(), Value::String("New".into()));
            }
        }

        store_imported_record(sqlite, module, &id, &map).await?;
        count += 1;
    }
    Ok(count)
}

async fn import_odoo_relationship_links(postgres: &PgPool, sqlite: &SqlitePool) -> Result<u64> {
    let mut count = 0u64;
    if pg_table_exists(postgres, "res_partner").await? {
        let rows = sqlx::query(
            "SELECT child.id::text AS contact_id, child.parent_id::text AS account_id
             FROM res_partner child
             JOIN res_partner parent ON parent.id = child.parent_id
             WHERE child.active IS DISTINCT FROM false
               AND parent.active IS DISTINCT FROM false
               AND COALESCE(child.is_company, false) = false
               AND COALESCE(parent.is_company, false) = true",
        )
        .fetch_all(postgres)
        .await?;
        for row in rows {
            let Some(contact_id) = pg_string(&row, "contact_id") else {
                continue;
            };
            let Some(account_id) = pg_string(&row, "account_id") else {
                continue;
            };
            if link_records(
                sqlite,
                "accounts",
                &odoo_record_id("res_partner", &account_id),
                "contacts",
                &odoo_record_id("res_partner", &contact_id),
                Some("account_contact"),
            )
            .await?
            .is_some()
            {
                count += 1;
            }
        }
    }

    if pg_table_exists(postgres, "crm_lead").await? {
        let rows = sqlx::query(
            "SELECT id::text AS lead_id, partner_id::text AS partner_id
             FROM crm_lead
             WHERE active IS DISTINCT FROM false
               AND type = 'opportunity'
               AND partner_id IS NOT NULL",
        )
        .fetch_all(postgres)
        .await?;
        for row in rows {
            let Some(lead_id) = pg_string(&row, "lead_id") else {
                continue;
            };
            let Some(partner_id) = pg_string(&row, "partner_id") else {
                continue;
            };
            if link_records(
                sqlite,
                "accounts",
                &odoo_record_id("res_partner", &partner_id),
                "opportunities",
                &odoo_record_id("crm_lead", &lead_id),
                Some("account_opportunity"),
            )
            .await?
            .is_some()
            {
                count += 1;
            }
        }
    }

    Ok(count)
}

async fn count_odoo_relationship_links(postgres: &PgPool) -> Result<u64> {
    let mut count = 0u64;
    if pg_table_exists(postgres, "res_partner").await? {
        count += count_pg_rows(
            postgres,
            "res_partner child JOIN res_partner parent ON parent.id = child.parent_id",
            "WHERE child.active IS DISTINCT FROM false
               AND parent.active IS DISTINCT FROM false
               AND COALESCE(child.is_company, false) = false
               AND COALESCE(parent.is_company, false) = true",
        )
        .await?;
    }
    if pg_table_exists(postgres, "crm_lead").await? {
        count += count_pg_rows(
            postgres,
            "crm_lead",
            "WHERE active IS DISTINCT FROM false AND type = 'opportunity' AND partner_id IS NOT NULL",
        )
        .await?;
    }
    Ok(count)
}

async fn pg_table_exists(postgres: &PgPool, table: &str) -> Result<bool> {
    let row = sqlx::query("SELECT to_regclass($1) IS NOT NULL AS exists")
        .bind(format!("public.{table}"))
        .fetch_one(postgres)
        .await?;
    Ok(row.try_get::<bool, _>("exists").unwrap_or(false))
}

async fn count_pg_rows(postgres: &PgPool, table: &str, clause: &str) -> Result<u64> {
    let sql = format!("SELECT COUNT(*) AS cnt FROM {table} {clause}");
    let row = sqlx::query(&sql).fetch_one(postgres).await?;
    let count = row.try_get::<i64, _>("cnt").unwrap_or_default();
    Ok(count.max(0) as u64)
}

fn odoo_record_id(model: &str, source_id: &str) -> String {
    format!("odoo:{model}:{source_id}")
}

fn odoo_base_map(model: &str, source_id: &str) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("deleted".into(), Value::String("0".into()));
    map.insert("source_system".into(), Value::String("odoo".into()));
    map.insert("source_module".into(), Value::String(model.into()));
    map.insert("source_id".into(), Value::String(source_id.into()));
    map
}

async fn store_imported_record(
    sqlite: &SqlitePool,
    module: &str,
    id: &str,
    map: &Map<String, Value>,
) -> Result<()> {
    let json = serde_json::to_string(map)?;
    sqlx::query("INSERT OR REPLACE INTO records (module, id, data) VALUES (?, ?, ?)")
        .bind(module)
        .bind(id)
        .bind(&json)
        .execute(sqlite)
        .await?;
    if crate::module::has_definition(sqlite, module).await? {
        upsert_typed_row(sqlite, module, id, map).await?;
    }
    Ok(())
}

fn pg_string(row: &sqlx::postgres::PgRow, name: &str) -> Option<String> {
    row.try_get::<Option<String>, _>(name)
        .ok()
        .flatten()
        .map(|value| clean_odoo_text(&value))
        .filter(|value| !value.trim().is_empty())
}

fn put_pg_string(
    map: &mut Map<String, Value>,
    row: &sqlx::postgres::PgRow,
    source: &str,
    target: &str,
) {
    if let Some(value) = pg_string(row, source) {
        map.insert(target.into(), Value::String(value));
    }
}

fn clean_odoo_text(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

fn split_odoo_name(value: Option<&str>, map: &mut Map<String, Value>) {
    let Some(name) = value.map(str::trim).filter(|name| !name.is_empty()) else {
        return;
    };
    let mut parts = name.split_whitespace().collect::<Vec<_>>();
    if parts.len() <= 1 {
        map.insert("last_name".into(), Value::String(name.into()));
        return;
    }
    let last = parts.pop().unwrap_or_default();
    map.insert("first_name".into(), Value::String(parts.join(" ")));
    map.insert("last_name".into(), Value::String(last.into()));
}

async fn inspect_suitecrm_source(mysql: &MySqlPool) -> Result<SuiteCrmImportSummary> {
    let modules = discover_modules_mysql(mysql).await?;
    let mut module_counts = Vec::new();
    let mut records = 0u64;
    let mut skipped_deleted = 0u64;

    for module in &modules {
        let total = count_mysql_rows(mysql, module, "").await?;
        let active = count_mysql_rows(mysql, module, "WHERE IFNULL(deleted, 0) = 0").await?;
        skipped_deleted += total.saturating_sub(active);
        records += active;
        module_counts.push(SuiteCrmModuleCount {
            module: module.clone(),
            label: suitecrm_module_label(module).into(),
            records: active,
        });
    }

    let relationships = count_core_relationship_links(mysql).await?;
    let metadata_relationships = count_relationship_definitions(mysql).await?;
    let mut warnings = inspect_suitecrm_warnings(mysql).await?;
    if skipped_deleted > 0 {
        warnings.push(format!(
            "{} deleted SuiteCRM records will be skipped.",
            skipped_deleted
        ));
    }
    if modules.is_empty() {
        warnings.push("No core SuiteCRM modules were found in this database.".into());
    }

    Ok(SuiteCrmImportSummary {
        module_count: modules.len(),
        modules,
        module_counts,
        records,
        relationships,
        metadata_relationships,
        warnings,
    })
}

fn suitecrm_module_label(module: &str) -> &'static str {
    match module {
        "accounts" => "Accounts",
        "contacts" => "Contacts",
        "opportunities" => "Opportunities",
        "leads" => "Leads",
        "cases" => "Cases",
        _ => "Records",
    }
}

async fn count_mysql_rows(mysql: &MySqlPool, table: &str, clause: &str) -> Result<u64> {
    let sql = format!("SELECT COUNT(*) AS cnt FROM `{}` {}", table, clause);
    let row = sqlx::query(&sql).fetch_one(mysql).await?;
    let count = row.try_get::<i64, _>("cnt").unwrap_or_default();
    Ok(count.max(0) as u64)
}

async fn import_module(mysql: &MySqlPool, sqlite: &SqlitePool, module: &str) -> Result<u64> {
    let cstm_exists = table_exists(mysql, &format!("{}_cstm", module)).await?;

    let sql = if cstm_exists {
        format!(
            "SELECT b.*, c.* FROM `{}` b LEFT JOIN `{}_cstm` c ON b.id = c.id_c WHERE IFNULL(b.deleted, 0) = 0",
            module, module
        )
    } else {
        format!("SELECT * FROM `{}` WHERE IFNULL(deleted, 0) = 0", module)
    };

    let rows = sqlx::query(&sql).fetch_all(mysql).await?;
    let mut count = 0u64;

    for row in &rows {
        let mut map = Map::new();
        for col in row.columns() {
            let name = col.name().to_string();
            if name == "id_c" && cstm_exists {
                continue;
            }
            let value = extract_mysql_value(row, &name);
            map.insert(name, value);
        }

        let id = map
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if id.trim().is_empty() {
            continue;
        }

        enrich_suitecrm_core_record(mysql, module, &id, &mut map).await?;

        // Always write blob store
        let json = serde_json::to_string(&map)?;
        sqlx::query("INSERT OR REPLACE INTO records (module, id, data) VALUES (?, ?, ?)")
            .bind(module)
            .bind(&id)
            .bind(&json)
            .execute(sqlite)
            .await?;

        // Write typed table if this module has one
        if crate::module::has_definition(sqlite, module).await? {
            upsert_typed_row(sqlite, module, &id, &map).await?;
        }

        count += 1;
    }

    tracing::info!("  {}: {} records", module, count);
    Ok(count)
}

async fn enrich_suitecrm_core_record(
    mysql: &MySqlPool,
    module: &str,
    id: &str,
    map: &mut Map<String, Value>,
) -> Result<()> {
    match module {
        "contacts" => {
            set_primary_link_field(
                mysql,
                map,
                "account_id",
                "accounts_contacts",
                "contact_id",
                id,
                "account_id",
            )
            .await?;
        }
        "opportunities" => {
            set_primary_link_field(
                mysql,
                map,
                "account_id",
                "accounts_opportunities",
                "opportunity_id",
                id,
                "account_id",
            )
            .await?;
        }
        _ => {}
    }

    let Some(bean_module) = suitecrm_bean_module_name(module) else {
        return Ok(());
    };
    let emails = fetch_suitecrm_emails(mysql, bean_module, id).await?;
    if map_field_is_blank(map, "email1")
        && let Some(email) = emails.first()
    {
        map.insert("email1".into(), Value::String(email.clone()));
    }
    if map_field_is_blank(map, "email2")
        && let Some(email) = emails.get(1)
    {
        map.insert("email2".into(), Value::String(email.clone()));
    }

    Ok(())
}

fn suitecrm_bean_module_name(module: &str) -> Option<&'static str> {
    match module {
        "accounts" => Some("Accounts"),
        "contacts" => Some("Contacts"),
        "leads" => Some("Leads"),
        _ => None,
    }
}

fn map_field_is_blank(map: &Map<String, Value>, key: &str) -> bool {
    match map.get(key) {
        None | Some(Value::Null) => true,
        Some(Value::String(value)) => value.trim().is_empty(),
        _ => false,
    }
}

async fn set_primary_link_field(
    mysql: &MySqlPool,
    map: &mut Map<String, Value>,
    field: &str,
    join_table: &str,
    lookup_column: &str,
    lookup_id: &str,
    target_column: &str,
) -> Result<()> {
    if !map_field_is_blank(map, field) {
        return Ok(());
    }
    if let Some(linked_id) =
        fetch_suitecrm_linked_id(mysql, join_table, lookup_column, lookup_id, target_column).await?
    {
        map.insert(field.into(), Value::String(linked_id));
    }
    Ok(())
}

async fn fetch_suitecrm_linked_id(
    mysql: &MySqlPool,
    join_table: &str,
    lookup_column: &str,
    lookup_id: &str,
    target_column: &str,
) -> Result<Option<String>> {
    if !table_exists(mysql, join_table).await? {
        return Ok(None);
    }

    let sql = format!(
        "SELECT `{}` AS linked_id
         FROM `{}`
         WHERE IFNULL(deleted, 0) = 0
           AND `{}` = ?
           AND `{}` IS NOT NULL
         ORDER BY date_modified DESC
         LIMIT 1",
        target_column, join_table, lookup_column, target_column,
    );
    let row = sqlx::query(&sql)
        .bind(lookup_id)
        .fetch_optional(mysql)
        .await?;
    Ok(row.and_then(|row| {
        row.try_get::<Option<String>, _>("linked_id")
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty())
    }))
}

async fn fetch_suitecrm_emails(
    mysql: &MySqlPool,
    bean_module: &str,
    bean_id: &str,
) -> Result<Vec<String>> {
    if !table_exists(mysql, "email_addr_bean_rel").await?
        || !table_exists(mysql, "email_addresses").await?
    {
        return Ok(Vec::new());
    }

    let rows = sqlx::query(
        "SELECT ea.email_address
         FROM email_addr_bean_rel rel
         JOIN email_addresses ea ON ea.id = rel.email_address_id
         WHERE rel.bean_module = ?
           AND rel.bean_id = ?
           AND IFNULL(rel.deleted, 0) = 0
           AND IFNULL(ea.deleted, 0) = 0
           AND IFNULL(ea.invalid_email, 0) = 0
         ORDER BY rel.primary_address DESC, rel.date_modified DESC
         LIMIT 2",
    )
    .bind(bean_module)
    .bind(bean_id)
    .fetch_all(mysql)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            row.try_get::<Option<String>, _>("email_address")
                .ok()
                .flatten()
        })
        .filter(|email| !email.trim().is_empty())
        .collect())
}

/// Extract a column value from a MySQL row, handling VARBINARY, strings, ints, and floats.
fn extract_mysql_value(row: &sqlx::mysql::MySqlRow, name: &str) -> Value {
    // String
    if let Ok(Some(s)) = row.try_get::<Option<String>, _>(name) {
        return Value::String(s);
    }
    // VARBINARY → try to format as UUID or hex
    if let Ok(Some(bytes)) = row.try_get::<Option<Vec<u8>>, _>(name) {
        if bytes.len() == 16 {
            return Value::String(format!(
                "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                bytes[0],
                bytes[1],
                bytes[2],
                bytes[3],
                bytes[4],
                bytes[5],
                bytes[6],
                bytes[7],
                bytes[8],
                bytes[9],
                bytes[10],
                bytes[11],
                bytes[12],
                bytes[13],
                bytes[14],
                bytes[15],
            ));
        }
        return Value::String(bytes.iter().map(|b| format!("{:02x}", b)).collect());
    }
    // Integer
    if let Ok(Some(n)) = row.try_get::<Option<i64>, _>(name) {
        return Value::String(n.to_string());
    }
    // Float
    if let Ok(Some(n)) = row.try_get::<Option<f64>, _>(name) {
        return Value::String(n.to_string());
    }
    Value::Null
}

async fn import_relationships(mysql: &MySqlPool, sqlite: &SqlitePool) -> Result<u64> {
    let rows = sqlx::query(
        "SELECT relationship_name, lhs_module, lhs_table, rhs_module, rhs_table
         FROM relationships",
    )
    .fetch_all(mysql)
    .await?;

    let mut count = 0u64;
    for row in &rows {
        let rel_name = row
            .try_get::<Option<String>, _>("relationship_name")
            .ok()
            .flatten()
            .unwrap_or_default();
        if rel_name.trim().is_empty() {
            continue;
        }
        let lhs_module = row
            .try_get::<Option<String>, _>("lhs_module")
            .ok()
            .flatten()
            .unwrap_or_default();
        let lhs_table = row
            .try_get::<Option<String>, _>("lhs_table")
            .ok()
            .flatten()
            .unwrap_or_default();
        let rhs_module = row
            .try_get::<Option<String>, _>("rhs_module")
            .ok()
            .flatten()
            .unwrap_or_default();
        let rhs_table = row
            .try_get::<Option<String>, _>("rhs_table")
            .ok()
            .flatten()
            .unwrap_or_default();

        let mut map = Map::new();
        map.insert("relationship_name".into(), Value::String(rel_name.clone()));
        map.insert("lhs_module".into(), Value::String(lhs_module));
        map.insert("lhs_table".into(), Value::String(lhs_table));
        map.insert("rhs_module".into(), Value::String(rhs_module));
        map.insert("rhs_table".into(), Value::String(rhs_table));

        let json = serde_json::to_string(&map)?;
        sqlx::query("INSERT OR REPLACE INTO records (module, id, data) VALUES (?, ?, ?)")
            .bind("_relationships")
            .bind(&rel_name)
            .bind(&json)
            .execute(sqlite)
            .await?;
        count += 1;
    }

    tracing::info!("  _relationships: {} definitions", count);
    Ok(count)
}

struct SuiteCrmLinkTable {
    table: &'static str,
    source_module: &'static str,
    source_column: &'static str,
    target_module: &'static str,
    target_column: &'static str,
    relationship_type: &'static str,
}

const SUITECRM_CORE_LINK_TABLES: &[SuiteCrmLinkTable] = &[
    SuiteCrmLinkTable {
        table: "accounts_contacts",
        source_module: "accounts",
        source_column: "account_id",
        target_module: "contacts",
        target_column: "contact_id",
        relationship_type: "account_contact",
    },
    SuiteCrmLinkTable {
        table: "accounts_opportunities",
        source_module: "accounts",
        source_column: "account_id",
        target_module: "opportunities",
        target_column: "opportunity_id",
        relationship_type: "account_opportunity",
    },
    SuiteCrmLinkTable {
        table: "opportunities_contacts",
        source_module: "opportunities",
        source_column: "opportunity_id",
        target_module: "contacts",
        target_column: "contact_id",
        relationship_type: "opportunity_contact",
    },
    SuiteCrmLinkTable {
        table: "accounts_cases",
        source_module: "accounts",
        source_column: "account_id",
        target_module: "cases",
        target_column: "case_id",
        relationship_type: "account_case",
    },
    SuiteCrmLinkTable {
        table: "contacts_cases",
        source_module: "contacts",
        source_column: "contact_id",
        target_module: "cases",
        target_column: "case_id",
        relationship_type: "contact_case",
    },
];

async fn count_relationship_definitions(mysql: &MySqlPool) -> Result<u64> {
    if !table_exists(mysql, "relationships").await? {
        return Ok(0);
    }
    let row = sqlx::query(
        "SELECT COUNT(*) AS cnt
         FROM relationships
         WHERE relationship_name IS NOT NULL
           AND relationship_name != ''",
    )
    .fetch_one(mysql)
    .await?;
    let count = row.try_get::<i64, _>("cnt").unwrap_or_default();
    Ok(count.max(0) as u64)
}

async fn count_core_relationship_links(mysql: &MySqlPool) -> Result<u64> {
    let mut count = 0u64;
    for relationship in SUITECRM_CORE_LINK_TABLES {
        count += count_valid_core_relationship_links_for(mysql, relationship).await?;
    }
    Ok(count)
}

async fn count_core_link_rows(mysql: &MySqlPool, relationship: &SuiteCrmLinkTable) -> Result<u64> {
    if !table_exists(mysql, relationship.table).await? {
        return Ok(0);
    }
    let sql = format!(
        "SELECT COUNT(*) AS cnt
         FROM `{}`
         WHERE IFNULL(deleted, 0) = 0
           AND `{}` IS NOT NULL
           AND `{}` IS NOT NULL",
        relationship.table, relationship.source_column, relationship.target_column,
    );
    let row = sqlx::query(&sql).fetch_one(mysql).await?;
    let count = row.try_get::<i64, _>("cnt").unwrap_or_default();
    Ok(count.max(0) as u64)
}

async fn count_valid_core_relationship_links_for(
    mysql: &MySqlPool,
    relationship: &SuiteCrmLinkTable,
) -> Result<u64> {
    if !table_exists(mysql, relationship.table).await?
        || !table_exists(mysql, relationship.source_module).await?
        || !table_exists(mysql, relationship.target_module).await?
    {
        return Ok(0);
    }
    let sql = format!(
        "SELECT COUNT(*) AS cnt
         FROM `{}` j
         JOIN `{}` source_record ON source_record.id = j.`{}`
         JOIN `{}` target_record ON target_record.id = j.`{}`
         WHERE IFNULL(j.deleted, 0) = 0
           AND IFNULL(source_record.deleted, 0) = 0
           AND IFNULL(target_record.deleted, 0) = 0",
        relationship.table,
        relationship.source_module,
        relationship.source_column,
        relationship.target_module,
        relationship.target_column,
    );
    let row = sqlx::query(&sql).fetch_one(mysql).await?;
    let count = row.try_get::<i64, _>("cnt").unwrap_or_default();
    Ok(count.max(0) as u64)
}

async fn inspect_suitecrm_warnings(mysql: &MySqlPool) -> Result<Vec<String>> {
    let mut warnings = Vec::new();

    for relationship in SUITECRM_CORE_LINK_TABLES {
        let rows = count_core_link_rows(mysql, relationship).await?;
        let valid = count_valid_core_relationship_links_for(mysql, relationship).await?;
        if rows > valid {
            warnings.push(format!(
                "{} {} rows point to missing or deleted records and will be skipped.",
                rows - valid,
                relationship.relationship_type.replace('_', " ")
            ));
        }
    }

    for (table, bean_module, label) in [
        ("contacts", "Contacts", "contacts"),
        ("leads", "Leads", "leads"),
    ] {
        let missing = count_missing_primary_email(mysql, table, bean_module).await?;
        if missing > 0 {
            warnings.push(format!(
                "{} {} do not have a primary email address.",
                missing, label
            ));
        }
    }

    Ok(warnings)
}

async fn count_missing_primary_email(
    mysql: &MySqlPool,
    table: &str,
    bean_module: &str,
) -> Result<u64> {
    if !table_exists(mysql, table).await?
        || !table_exists(mysql, "email_addr_bean_rel").await?
        || !table_exists(mysql, "email_addresses").await?
    {
        return Ok(0);
    }

    let sql = format!(
        "SELECT COUNT(*) AS cnt
         FROM `{}` bean
         WHERE IFNULL(bean.deleted, 0) = 0
           AND NOT EXISTS (
             SELECT 1
             FROM email_addr_bean_rel rel
             JOIN email_addresses email ON email.id = rel.email_address_id
             WHERE rel.bean_module = ?
               AND rel.bean_id = bean.id
               AND IFNULL(rel.deleted, 0) = 0
               AND IFNULL(email.deleted, 0) = 0
               AND IFNULL(email.invalid_email, 0) = 0
               AND email.email_address IS NOT NULL
               AND email.email_address != ''
           )",
        table
    );
    let row = sqlx::query(&sql).bind(bean_module).fetch_one(mysql).await?;
    let count = row.try_get::<i64, _>("cnt").unwrap_or_default();
    Ok(count.max(0) as u64)
}

async fn import_core_relationship_links(mysql: &MySqlPool, sqlite: &SqlitePool) -> Result<u64> {
    let mut count = 0u64;

    for relationship in SUITECRM_CORE_LINK_TABLES {
        if !table_exists(mysql, relationship.table).await? {
            continue;
        }

        let sql = format!(
            "SELECT `{}` AS source_id, `{}` AS target_id
             FROM `{}`
             WHERE IFNULL(deleted, 0) = 0
               AND `{}` IS NOT NULL
               AND `{}` IS NOT NULL",
            relationship.source_column,
            relationship.target_column,
            relationship.table,
            relationship.source_column,
            relationship.target_column,
        );
        let rows = sqlx::query(&sql).fetch_all(mysql).await?;

        for row in rows {
            let source_id: Option<String> = row.try_get("source_id").ok().flatten();
            let target_id: Option<String> = row.try_get("target_id").ok().flatten();
            let Some(source_id) = source_id.filter(|id| !id.trim().is_empty()) else {
                continue;
            };
            let Some(target_id) = target_id.filter(|id| !id.trim().is_empty()) else {
                continue;
            };

            if link_records(
                sqlite,
                relationship.source_module,
                &source_id,
                relationship.target_module,
                &target_id,
                Some(relationship.relationship_type),
            )
            .await?
            .is_some()
            {
                count += 1;
            }
        }
    }

    tracing::info!("  record_links: {} core SuiteCRM links", count);
    Ok(count)
}

// ── Module discovery ──

pub async fn discover_modules_mysql(pool: &MySqlPool) -> Result<Vec<String>> {
    let mut modules = Vec::new();
    for module in SUITECRM_CORE_MODULES {
        if table_exists(pool, module).await? {
            modules.push((*module).to_string());
        }
    }
    Ok(modules)
}

pub async fn discover_modules_sqlite(pool: &SqlitePool) -> Result<Vec<String>> {
    // Blob store modules
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT module FROM records WHERE module != '_relationships' ORDER BY module",
    )
    .fetch_all(pool)
    .await?;
    let mut modules: Vec<String> = rows.into_iter().map(|(name,)| name).collect();

    // Also include typed modules that have data but not in blob store
    let defs = crate::module::get_all_definitions(pool).await?;
    for module in defs.iter().map(|d| &*d.module) {
        if !modules.contains(&module.to_string()) {
            modules.push(module.to_string());
        }
    }

    modules.sort();
    Ok(modules)
}

async fn append_activity(
    pool: &SqlitePool,
    module: &str,
    record_id: &str,
    operation: &str,
    changed_fields: &Map<String, Value>,
) -> Result<()> {
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let changed_fields = serde_json::to_string(changed_fields)?;
    sqlx::query(
        "INSERT INTO activity_log
         (id, module, record_id, operation, actor_id, changed_fields, created_at)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6)",
    )
    .bind(id)
    .bind(module)
    .bind(record_id)
    .bind(operation)
    .bind(changed_fields)
    .bind(created_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn add_record_note(
    pool: &SqlitePool,
    module: &str,
    id: &str,
    note: &str,
) -> Result<bool> {
    if get_module_record(pool, module, id).await?.is_none() {
        return Ok(false);
    }

    let mut changed_fields = Map::new();
    changed_fields.insert("note".into(), Value::String(note.trim().to_string()));
    append_activity(pool, module, id, "note", &changed_fields).await?;
    Ok(true)
}

// ── Create / Update / Delete ──

/// Create a record: typed table + blob store. Returns the generated UUID.
pub async fn create_record(
    pool: &SqlitePool,
    module: &str,
    fields: &serde_json::Map<String, serde_json::Value>,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let mut data = fields.clone();
    data.insert("id".into(), serde_json::Value::String(id.clone()));
    if !data.contains_key("date_entered") {
        data.insert(
            "date_entered".into(),
            serde_json::Value::String(now.clone()),
        );
    }
    if !data.contains_key("date_modified") {
        data.insert("date_modified".into(), serde_json::Value::String(now));
    }
    if !data.contains_key("deleted") {
        data.insert("deleted".into(), serde_json::Value::String("0".into()));
    }

    // Blob store
    let json = serde_json::to_string(&data)?;
    sqlx::query("INSERT OR REPLACE INTO records (module, id, data) VALUES (?, ?, ?)")
        .bind(module)
        .bind(&id)
        .bind(&json)
        .execute(pool)
        .await?;

    // Typed table
    if crate::module::has_definition(pool, module).await? {
        upsert_typed_row(pool, module, &id, &data).await?;
    }

    append_activity(pool, module, &id, "create", &data).await?;

    Ok(id)
}

/// Update a record: merges fields, sets date_modified, updates typed + blob.
pub async fn update_record(
    pool: &SqlitePool,
    module: &str,
    id: &str,
    fields: &serde_json::Map<String, serde_json::Value>,
) -> Result<bool> {
    let existing = get_module_record(pool, module, id).await?;
    let mut record = match existing {
        Some(r) => r,
        None => return Ok(false),
    };

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    record.insert("date_modified".into(), serde_json::Value::String(now));

    for (key, val) in fields {
        record.insert(key.clone(), val.clone());
    }
    record.insert("id".into(), serde_json::Value::String(id.to_string()));

    // Blob store
    let json = serde_json::to_string(&record)?;
    sqlx::query("INSERT OR REPLACE INTO records (module, id, data) VALUES (?, ?, ?)")
        .bind(module)
        .bind(id)
        .bind(&json)
        .execute(pool)
        .await?;

    // Typed table
    if crate::module::has_definition(pool, module).await? {
        upsert_typed_row(pool, module, id, &record).await?;
    }

    append_activity(pool, module, id, "update", fields).await?;

    Ok(true)
}

/// Soft-delete a record by setting deleted=1.
pub async fn delete_record(pool: &SqlitePool, module: &str, id: &str) -> Result<bool> {
    let existing = get_module_record(pool, module, id).await?;
    let mut record = match existing {
        Some(r) => r,
        None => return Ok(false),
    };

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    record.insert("deleted".into(), serde_json::Value::String("1".into()));
    record.insert("date_modified".into(), serde_json::Value::String(now));

    let json = serde_json::to_string(&record)?;
    sqlx::query("INSERT OR REPLACE INTO records (module, id, data) VALUES (?, ?, ?)")
        .bind(module)
        .bind(id)
        .bind(&json)
        .execute(pool)
        .await?;

    if crate::module::has_definition(pool, module).await? {
        upsert_typed_row(pool, module, id, &record).await?;
    }

    let mut changed_fields = Map::new();
    changed_fields.insert("deleted".into(), serde_json::Value::String("1".into()));
    if let Some(date_modified) = record.get("date_modified") {
        changed_fields.insert("date_modified".into(), date_modified.clone());
    }
    append_activity(pool, module, id, "delete", &changed_fields).await?;

    Ok(true)
}

/// Restore a soft-deleted record by setting deleted=0.
pub async fn restore_record(pool: &SqlitePool, module: &str, id: &str) -> Result<bool> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT data FROM records WHERE module = ?1 AND id = ?2")
            .bind(module)
            .bind(id)
            .fetch_optional(pool)
            .await?;

    let Some((json,)) = row else {
        return Ok(false);
    };

    let mut record: Map<String, Value> = serde_json::from_str(&json)?;
    let was_deleted = is_deleted_value(record.get("deleted"));
    if !was_deleted {
        return Ok(true);
    }

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    record.insert("id".into(), Value::String(id.to_string()));
    record.insert("deleted".into(), Value::String("0".into()));
    record.insert("date_modified".into(), Value::String(now));

    let json = serde_json::to_string(&record)?;
    sqlx::query("INSERT OR REPLACE INTO records (module, id, data) VALUES (?, ?, ?)")
        .bind(module)
        .bind(id)
        .bind(&json)
        .execute(pool)
        .await?;

    if crate::module::has_definition(pool, module).await? {
        upsert_typed_row(pool, module, id, &record).await?;
    }

    let mut changed_fields = Map::new();
    changed_fields.insert("deleted".into(), Value::String("0".into()));
    if let Some(date_modified) = record.get("date_modified") {
        changed_fields.insert("date_modified".into(), date_modified.clone());
    }
    append_activity(pool, module, id, "restore", &changed_fields).await?;

    Ok(true)
}

fn is_deleted_value(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_i64().unwrap_or_default() != 0,
        Some(Value::String(value)) => matches!(value.trim(), "1" | "true" | "TRUE" | "True"),
        _ => false,
    }
}
// ── SQLite queries (typed-table-aware) ──

pub async fn query_module(
    pool: &SqlitePool,
    module: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<Map<String, Value>>> {
    if crate::module::has_definition(pool, module).await? {
        let sql = format!(
            "SELECT * FROM {} WHERE deleted = 0 ORDER BY id LIMIT ?1 OFFSET ?2",
            module
        );
        tracing::debug!(%module, sql = %sql, limit, offset, "query_module typed");
        let rows = sqlx::query(&sql)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?;
        tracing::debug!(%module, row_count = rows.len(), "query_module typed rows fetched");

        let results: Vec<Map<String, Value>> = rows.iter().map(sqlite_row_to_map).collect();
        return Ok(results);
    }

    // Fallback: blob store
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT data FROM records WHERE module = ?1 AND IFNULL(json_extract(data, \'$.deleted\'), \'0\') != \'1\' ORDER BY id LIMIT ?2 OFFSET ?3",
    )
    .bind(module)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    let results: Vec<Map<String, Value>> = rows
        .into_iter()
        .filter_map(|(json,)| serde_json::from_str(&json).ok())
        .collect();
    Ok(results)
}
pub async fn get_module_record(
    pool: &SqlitePool,
    module: &str,
    id: &str,
) -> Result<Option<Map<String, Value>>> {
    if crate::module::has_definition(pool, module).await? {
        let sql = format!("SELECT * FROM {} WHERE id = ?1 AND deleted = 0", module);
        let row = sqlx::query(&sql).bind(id).fetch_optional(pool).await?;

        if let Some(row) = row {
            return Ok(Some(sqlite_row_to_map(&row)));
        }
        return Ok(None);
    }

    // Fallback: blob store
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT data FROM records WHERE module = ?1 AND id = ?2 AND IFNULL(json_extract(data, '$.deleted'), '0') != '1'",
    )
    .bind(module)
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.and_then(|(json,)| serde_json::from_str(&json).ok()))
}

pub async fn count_module(pool: &SqlitePool, module: &str) -> Result<i64> {
    if crate::module::has_definition(pool, module).await? {
        let sql = format!("SELECT COUNT(*) as cnt FROM {} WHERE deleted = 0", module);
        let row: (i64,) = sqlx::query_as(&sql).fetch_one(pool).await?;
        return Ok(row.0);
    }

    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) as cnt FROM records WHERE module = ?1 AND IFNULL(json_extract(data, '$.deleted'), '0') != '1'",
    )
    .bind(module)
    .fetch_one(pool)
    .await?;

    Ok(row.0)
}

pub async fn get_record_activities(
    pool: &SqlitePool,
    module: &str,
    id: &str,
    limit: u32,
) -> Result<Vec<Map<String, Value>>> {
    let rows = sqlx::query(
        "SELECT id, module, record_id, operation, actor_id, changed_fields, created_at
         FROM activity_log
         WHERE module = ?1 AND record_id = ?2
         ORDER BY created_at DESC, id DESC
         LIMIT ?3",
    )
    .bind(module)
    .bind(id)
    .bind(limit.min(100))
    .fetch_all(pool)
    .await?;

    let mut activities = Vec::new();
    for row in rows {
        activities.push(sqlite_row_to_map(&row));
    }
    Ok(activities)
}

pub async fn link_records(
    pool: &SqlitePool,
    source_module: &str,
    source_id: &str,
    target_module: &str,
    target_id: &str,
    relationship_type: Option<&str>,
) -> Result<Option<String>> {
    if source_module == target_module && source_id == target_id {
        return Ok(None);
    }
    if get_module_record(pool, source_module, source_id)
        .await?
        .is_none()
        || get_module_record(pool, target_module, target_id)
            .await?
            .is_none()
    {
        return Ok(None);
    }

    let relationship_type = relationship_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("related");

    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM record_links
         WHERE relationship_type = ?1
           AND (
             (source_module = ?2 AND source_id = ?3 AND target_module = ?4 AND target_id = ?5)
             OR
             (source_module = ?4 AND source_id = ?5 AND target_module = ?2 AND target_id = ?3)
           )
         LIMIT 1",
    )
    .bind(relationship_type)
    .bind(source_module)
    .bind(source_id)
    .bind(target_module)
    .bind(target_id)
    .fetch_optional(pool)
    .await?;

    if let Some((id,)) = existing {
        return Ok(Some(id));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    sqlx::query(
        "INSERT INTO record_links
         (id, source_module, source_id, target_module, target_id, relationship_type, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .bind(&id)
    .bind(source_module)
    .bind(source_id)
    .bind(target_module)
    .bind(target_id)
    .bind(relationship_type)
    .bind(created_at)
    .execute(pool)
    .await?;

    let mut changed_fields = Map::new();
    changed_fields.insert("target_module".into(), Value::String(target_module.into()));
    changed_fields.insert("target_id".into(), Value::String(target_id.into()));
    changed_fields.insert(
        "relationship_type".into(),
        Value::String(relationship_type.into()),
    );
    append_activity(pool, source_module, source_id, "link", &changed_fields).await?;

    Ok(Some(id))
}

pub async fn delete_record_link(pool: &SqlitePool, link_id: &str) -> Result<bool> {
    let row: Option<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT source_module, source_id, target_module, target_id, relationship_type
         FROM record_links
         WHERE id = ?1",
    )
    .bind(link_id)
    .fetch_optional(pool)
    .await?;

    let Some((source_module, source_id, target_module, target_id, relationship_type)) = row else {
        return Ok(false);
    };

    sqlx::query("DELETE FROM record_links WHERE id = ?1")
        .bind(link_id)
        .execute(pool)
        .await?;

    let mut changed_fields = Map::new();
    changed_fields.insert("target_module".into(), Value::String(target_module));
    changed_fields.insert("target_id".into(), Value::String(target_id));
    changed_fields.insert("relationship_type".into(), Value::String(relationship_type));
    append_activity(pool, &source_module, &source_id, "unlink", &changed_fields).await?;

    Ok(true)
}

fn row_contains_query(row: &Map<String, Value>, query: &str) -> bool {
    row.values().any(|value| {
        let text = match value {
            Value::String(value) => value.clone(),
            Value::Number(value) => value.to_string(),
            Value::Bool(value) => value.to_string(),
            _ => String::new(),
        };
        text.to_lowercase().contains(query)
    })
}

pub async fn search_records(
    pool: &SqlitePool,
    query: &str,
    limit: u32,
) -> Result<Vec<Map<String, Value>>> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let limit = limit.clamp(1, 100);
    let modules = discover_modules_sqlite(pool).await?;
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    for module in modules {
        if results.len() >= limit as usize {
            break;
        }

        let rows = query_module(pool, &module, 500, 0)
            .await
            .unwrap_or_default();
        for mut row in rows {
            if results.len() >= limit as usize {
                break;
            }
            let id = row
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if id.is_empty() || !row_contains_query(&row, &query) {
                continue;
            }

            seen.insert((module.clone(), id));
            row.insert("_module".into(), Value::String(module.clone()));
            row.insert("_match".into(), Value::String("record".into()));
            results.push(row);
        }
    }

    if results.len() < limit as usize {
        let like = format!("%{}%", query);
        let rows = sqlx::query(
            "SELECT module, record_id, changed_fields
             FROM activity_log
             WHERE changed_fields LIKE ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind(like)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        for row in rows {
            if results.len() >= limit as usize {
                break;
            }
            let module: String = row.get("module");
            let id: String = row.get("record_id");
            if seen.contains(&(module.clone(), id.clone())) {
                continue;
            }
            if let Some(mut record) = get_module_record(pool, &module, &id).await? {
                record.insert("_module".into(), Value::String(module.clone()));
                record.insert("_match".into(), Value::String("activity".into()));
                if let Ok(changed_fields) = row.try_get::<String, _>("changed_fields") {
                    record.insert("_activity".into(), Value::String(changed_fields));
                }
                seen.insert((module, id));
                results.push(record);
            }
        }
    }

    Ok(results)
}

pub async fn export_workspace(pool: &SqlitePool) -> Result<Value> {
    crate::bundle::export_workspace(pool).await
}

pub async fn export_workspace_archive(pool: &SqlitePool) -> Result<Vec<u8>> {
    crate::bundle::export_workspace_archive(pool).await
}

pub async fn restore_workspace(pool: &SqlitePool, bundle: &Value) -> Result<Map<String, Value>> {
    crate::bundle::restore_workspace(pool, bundle).await
}

pub async fn restore_workspace_archive(
    pool: &SqlitePool,
    bytes: &[u8],
    exact: bool,
) -> Result<Map<String, Value>> {
    crate::bundle::restore_workspace_archive(pool, bytes, exact).await
}

pub async fn restore_workspace_exact(
    pool: &SqlitePool,
    bundle: &Value,
) -> Result<Map<String, Value>> {
    crate::bundle::restore_workspace_exact(pool, bundle).await
}

// ── Relationships (blob store) ──

/// SuiteCRM convention: table names are plural, FK columns are singular.
fn singularize(table: &str) -> &str {
    if table.ends_with("ies") {
        table.strip_suffix("ies").unwrap()
    } else if table.ends_with("ses") {
        table.strip_suffix("es").unwrap()
    } else if table.ends_with('s') && !table.ends_with("ss") {
        &table[..table.len() - 1]
    } else {
        table
    }
}

pub async fn get_relationships(
    pool: &SqlitePool,
    module: &str,
    id: &str,
) -> Result<Vec<Map<String, Value>>> {
    let link_rows = sqlx::query(
        "SELECT id, source_module, source_id, target_module, target_id, relationship_type, created_at
         FROM record_links
         WHERE (source_module = ?1 AND source_id = ?2)
            OR (target_module = ?1 AND target_id = ?2)
         ORDER BY created_at DESC",
    )
    .bind(module)
    .bind(id)
    .fetch_all(pool)
    .await?;

    let mut results = Vec::new();
    for row in link_rows {
        let link_id: String = row.get("id");
        let source_module: String = row.get("source_module");
        let source_id: String = row.get("source_id");
        let target_module: String = row.get("target_module");
        let target_id: String = row.get("target_id");
        let relationship_type: String = row.get("relationship_type");

        let (related_module, related_id) = if source_module == module && source_id == id {
            (target_module, target_id)
        } else {
            (source_module, source_id)
        };

        if let Some(mut record) = get_module_record(pool, &related_module, &related_id).await? {
            record.insert("_module".into(), Value::String(related_module));
            record.insert("_link_id".into(), Value::String(link_id));
            record.insert(
                "_relationship_type".into(),
                Value::String(relationship_type),
            );
            results.push(record);
        }
    }

    let rel_rows: Vec<(String,)> =
        sqlx::query_as("SELECT data FROM records WHERE module = '_relationships'")
            .fetch_all(pool)
            .await?;

    for (json,) in rel_rows {
        let rel: Map<String, Value> = match serde_json::from_str(&json) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let rel_name = rel
            .get("relationship_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let lhs_module = rel.get("lhs_module").and_then(|v| v.as_str()).unwrap_or("");
        let rhs_module = rel.get("rhs_module").and_then(|v| v.as_str()).unwrap_or("");
        let lhs_table = rel.get("lhs_table").and_then(|v| v.as_str()).unwrap_or("");
        let rhs_table = rel.get("rhs_table").and_then(|v| v.as_str()).unwrap_or("");

        if !lhs_module.eq_ignore_ascii_case(module) && !rhs_module.eq_ignore_ascii_case(module) {
            continue;
        }

        let (target_module, my_fk, their_fk) = if lhs_module.eq_ignore_ascii_case(module) {
            (
                rhs_table,
                format!("{}_id", singularize(lhs_table)),
                format!("{}_id", singularize(rhs_table)),
            )
        } else {
            (
                lhs_table,
                format!("{}_id", singularize(rhs_table)),
                format!("{}_id", singularize(lhs_table)),
            )
        };

        // Parameterized query: find related IDs from the junction table
        let jn_sql = format!(
            "SELECT json_extract(data, '$.{}') as related_id FROM records WHERE module = ?1 AND json_extract(data, '$.{}') = ?2",
            their_fk, my_fk
        );
        let jn_rows: Vec<(Option<String>,)> = sqlx::query_as(&jn_sql)
            .bind(rel_name)
            .bind(id)
            .fetch_all(pool)
            .await?;

        for (related_id,) in jn_rows {
            if let Some(rid) = related_id {
                // Use get_module_record to benefit from typed tables
                if let Ok(Some(mut record)) = get_module_record(pool, target_module, &rid).await {
                    record.insert("_module".into(), Value::String(target_module.to_string()));
                    record.insert("_relationship_type".into(), Value::String(rel_name.into()));
                    results.push(record);
                }
            }
        }
    }

    Ok(results)
}

async fn table_exists(pool: &MySqlPool, table_name: &str) -> Result<bool> {
    let result: Option<(String,)> = sqlx::query_as(
        "SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?",
    )
    .bind(table_name)
    .fetch_optional(pool)
    .await?;

    Ok(result.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");
        init_schema(&pool).await.expect("init schema");
        pool
    }

    #[tokio::test]
    async fn built_in_modules_are_discoverable_when_empty() {
        let pool = test_pool().await;
        let modules = discover_modules_sqlite(&pool)
            .await
            .expect("discover modules");

        assert!(modules.contains(&"accounts".to_string()));
        assert!(modules.contains(&"contacts".to_string()));
        assert!(modules.contains(&"opportunities".to_string()));
        assert!(modules.contains(&"leads".to_string()));
        assert!(modules.contains(&"cases".to_string()));
    }

    #[tokio::test]
    async fn connect_sqlite_accepts_absolute_paths() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("ironcrm.db");
        let path = db_path.to_str().expect("utf-8 path");

        let pool = connect_sqlite(path).await.expect("connect absolute sqlite");
        let modules = discover_modules_sqlite(&pool)
            .await
            .expect("discover modules");

        assert!(db_path.exists());
        assert!(modules.contains(&"contacts".to_string()));
    }

    #[tokio::test]
    async fn create_update_delete_writes_activity_log() {
        let pool = test_pool().await;
        let mut fields = Map::new();
        fields.insert("first_name".into(), json!("Ada"));
        fields.insert("last_name".into(), json!("Lovelace"));
        fields.insert("email1".into(), json!("ada@example.com"));

        let id = create_record(&pool, "contacts", &fields)
            .await
            .expect("create record");
        let created = get_module_record(&pool, "contacts", &id)
            .await
            .expect("get created record")
            .expect("created record exists");
        assert_eq!(
            created.get("first_name").and_then(Value::as_str),
            Some("Ada")
        );
        assert_eq!(created.get("deleted").and_then(Value::as_i64), Some(0));

        let mut update = Map::new();
        update.insert("title".into(), json!("Founder"));
        assert!(
            update_record(&pool, "contacts", &id, &update)
                .await
                .expect("update record")
        );
        assert!(
            add_record_note(&pool, "contacts", &id, "Met at the launch event")
                .await
                .expect("add note")
        );

        assert!(
            delete_record(&pool, "contacts", &id)
                .await
                .expect("delete record")
        );
        assert!(
            get_module_record(&pool, "contacts", &id)
                .await
                .expect("get deleted record")
                .is_none()
        );
        assert!(
            restore_record(&pool, "contacts", &id)
                .await
                .expect("restore record")
        );
        let restored = get_module_record(&pool, "contacts", &id)
            .await
            .expect("get restored record")
            .expect("restored record exists");
        assert_eq!(restored.get("deleted").and_then(Value::as_i64), Some(0));

        let activities = get_record_activities(&pool, "contacts", &id, 10)
            .await
            .expect("get activities");
        let operations: Vec<String> = activities
            .iter()
            .filter_map(|row| {
                row.get("operation")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect();

        assert_eq!(operations.len(), 5);
        assert!(operations.contains(&"create".to_string()));
        assert!(operations.contains(&"update".to_string()));
        assert!(operations.contains(&"note".to_string()));
        assert!(operations.contains(&"delete".to_string()));
        assert!(operations.contains(&"restore".to_string()));
    }

    #[tokio::test]
    async fn links_search_and_bundle_restore_round_trip() {
        let pool = test_pool().await;

        let mut contact = Map::new();
        contact.insert("first_name".into(), json!("Ada"));
        contact.insert("last_name".into(), json!("Lovelace"));
        contact.insert("email1".into(), json!("ada@example.com"));
        contact.insert("source_system".into(), json!("suitecrm"));
        contact.insert("source_module".into(), json!("Contacts"));
        contact.insert("source_id".into(), json!("suite-contact-1"));
        contact.insert("imported_at".into(), json!("2026-06-01T00:00:00Z"));
        contact.insert("favorite_engine_c".into(), json!("Difference Engine"));
        let contact_id = create_record(&pool, "contacts", &contact)
            .await
            .expect("create contact");

        let mut account = Map::new();
        account.insert("name".into(), json!("Analytical Engines Ltd"));
        account.insert("industry".into(), json!("Computing"));
        let account_id = create_record(&pool, "accounts", &account)
            .await
            .expect("create account");

        let link_id = link_records(
            &pool,
            "contacts",
            &contact_id,
            "accounts",
            &account_id,
            Some("employer"),
        )
        .await
        .expect("link records")
        .expect("link id");
        assert!(!link_id.is_empty());

        sqlx::query(
            "INSERT INTO consent_receipts (contact_id, credential_json, issued_at, expires_at)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(&contact_id)
        .bind(r#"{"legal_basis":"legitimate_interest","scope":"launch planning"}"#)
        .bind("2026-06-01T00:00:00Z")
        .bind("2028-06-01T00:00:00Z")
        .execute(&pool)
        .await
        .expect("insert consent receipt");

        let relationships = get_relationships(&pool, "contacts", &contact_id)
            .await
            .expect("get relationships");
        assert_eq!(relationships.len(), 1);
        assert_eq!(
            relationships[0].get("_module").and_then(Value::as_str),
            Some("accounts")
        );
        assert_eq!(
            relationships[0]
                .get("_relationship_type")
                .and_then(Value::as_str),
            Some("employer")
        );

        add_record_note(&pool, "contacts", &contact_id, "Discussed the launch plan")
            .await
            .expect("add note");
        let note_hits = search_records(&pool, "launch plan", 10)
            .await
            .expect("search notes");
        assert!(note_hits.iter().any(|row| {
            row.get("id").and_then(Value::as_str) == Some(contact_id.as_str())
                && row.get("_match").and_then(Value::as_str) == Some("activity")
        }));

        let bundle = export_workspace(&pool).await.expect("export workspace");
        assert_eq!(
            bundle.get("kind").and_then(Value::as_str),
            Some("ironcrm.workspace")
        );
        assert_eq!(
            bundle
                .pointer("/manifest/counts/records")
                .and_then(Value::as_u64),
            Some(2)
        );
        assert_eq!(
            bundle
                .pointer("/manifest/counts/consent_receipts")
                .and_then(Value::as_u64),
            Some(1)
        );
        assert!(bundle.get("module_definitions").is_none());
        assert!(bundle.get("relationships").is_none());
        assert!(bundle.pointer("/manifest/counts/did_registry").is_none());
        assert!(
            bundle
                .pointer("/manifest/counts/relationship_definitions")
                .is_none()
        );
        assert!(
            bundle
                .pointer("/schemas/modules")
                .and_then(Value::as_array)
                .is_some_and(|modules| !modules.is_empty())
        );
        assert!(
            bundle
                .pointer("/manifest/hashes/records")
                .and_then(Value::as_str)
                .is_some_and(|hash| hash.starts_with("sha256:"))
        );
        assert!(
            bundle
                .get("records")
                .and_then(Value::as_array)
                .expect("bundle records")
                .iter()
                .any(|record| {
                    record.get("module").and_then(Value::as_str) == Some("contacts")
                        && record.pointer("/data/id").and_then(Value::as_str)
                            == Some(contact_id.as_str())
                        && record
                            .pointer("/data/favorite_engine_c")
                            .and_then(Value::as_str)
                            == Some("Difference Engine")
                })
        );
        assert!(
            bundle
                .get("sources")
                .and_then(Value::as_array)
                .expect("bundle sources")
                .iter()
                .any(|source| {
                    source.get("record_id").and_then(Value::as_str) == Some(contact_id.as_str())
                        && source.get("source_system").and_then(Value::as_str) == Some("suitecrm")
                        && source
                            .pointer("/custom_fields/favorite_engine_c")
                            .and_then(Value::as_str)
                            == Some("Difference Engine")
                })
        );

        let archive = export_workspace_archive(&pool)
            .await
            .expect("export workspace archive");
        let archive_summary =
            crate::bundle::validate_archive_bytes(&archive).expect("validate archive");
        assert_eq!(archive_summary.records, 2);
        assert_eq!(archive_summary.record_links, 1);
        assert_eq!(archive_summary.consent_receipts, 1);

        let archive_pool = test_pool().await;
        let archive_restore = restore_workspace_archive(&archive_pool, &archive, true)
            .await
            .expect("restore workspace archive exactly");
        assert_eq!(
            archive_restore.get("records").and_then(Value::as_u64),
            Some(2)
        );
        let archive_contact = get_module_record(&archive_pool, "contacts", &contact_id)
            .await
            .expect("get archive-restored contact")
            .expect("archive-restored contact exists");
        assert_eq!(
            archive_contact.get("email1").and_then(Value::as_str),
            Some("ada@example.com")
        );

        let mut tampered_bundle = bundle.clone();
        tampered_bundle["records"][0]["data"]["email1"] = json!("tampered@example.com");
        let tampered_pool = test_pool().await;
        let tampered_restore = restore_workspace(&tampered_pool, &tampered_bundle).await;
        assert!(tampered_restore.is_err());

        let restored_pool = test_pool().await;
        let summary = restore_workspace(&restored_pool, &bundle)
            .await
            .expect("restore workspace");
        assert_eq!(summary.get("records").and_then(Value::as_u64), Some(2));
        assert_eq!(summary.get("record_links").and_then(Value::as_u64), Some(1));
        assert_eq!(
            summary.get("consent_receipts").and_then(Value::as_u64),
            Some(1)
        );
        assert!(summary.get("did_registry").is_none());

        let restored_contact = get_module_record(&restored_pool, "contacts", &contact_id)
            .await
            .expect("get restored contact")
            .expect("restored contact exists");
        assert_eq!(
            restored_contact.get("email1").and_then(Value::as_str),
            Some("ada@example.com")
        );
        let restored_blob: (String,) =
            sqlx::query_as("SELECT data FROM records WHERE module = 'contacts' AND id = ?1")
                .bind(&contact_id)
                .fetch_one(&restored_pool)
                .await
                .expect("restored blob exists");
        let restored_blob: Map<String, Value> =
            serde_json::from_str(&restored_blob.0).expect("restored blob json");
        assert_eq!(
            restored_blob
                .get("favorite_engine_c")
                .and_then(Value::as_str),
            Some("Difference Engine")
        );

        let restored_relationships = get_relationships(&restored_pool, "contacts", &contact_id)
            .await
            .expect("restored relationships");
        assert_eq!(restored_relationships.len(), 1);
        assert_eq!(
            restored_relationships[0]
                .get("name")
                .and_then(Value::as_str),
            Some("Analytical Engines Ltd")
        );

        let mut extra_lead = Map::new();
        extra_lead.insert("first_name".into(), json!("Temporary"));
        extra_lead.insert("last_name".into(), json!("Lead"));
        create_record(&restored_pool, "leads", &extra_lead)
            .await
            .expect("create extra lead before exact restore");
        sqlx::query(
            "INSERT OR REPLACE INTO consent_receipts (contact_id, credential_json)
             VALUES (?1, ?2)",
        )
        .bind("temporary-contact")
        .bind(r#"{"legal_basis":"temporary"}"#)
        .execute(&restored_pool)
        .await
        .expect("insert temporary consent");

        let exact_summary = restore_workspace_exact(&restored_pool, &bundle)
            .await
            .expect("exact restore workspace");
        assert_eq!(
            exact_summary.get("records").and_then(Value::as_u64),
            Some(2)
        );
        assert_eq!(
            count_module(&restored_pool, "leads")
                .await
                .expect("count leads"),
            0
        );
        let consent_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM consent_receipts")
            .fetch_one(&restored_pool)
            .await
            .expect("count restored consent");
        assert_eq!(consent_count.0, 1);
    }

    #[tokio::test]
    async fn restore_rejects_non_workspace_bundle() {
        let pool = test_pool().await;
        let result = restore_workspace(&pool, &json!({"records": []})).await;
        assert!(result.is_err());
    }
}
