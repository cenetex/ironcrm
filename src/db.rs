use anyhow::{Context, Result};

/// SuiteCRM convention: table names are plural, FK columns are singular.
fn singularize(table: &str) -> &str {
    if table.ends_with("ies") {
        // opportunities -> opportunity
        table.strip_suffix("ies").unwrap()
    } else if table.ends_with("ses") {
        // cases -> case  
        table.strip_suffix("es").unwrap()
    } else if table.ends_with('s') && !table.ends_with("ss") {
        // accounts -> account, contacts -> contact
        &table[..table.len()-1]
    } else {
        table
    }
}
use serde_json::{Map, Value};
use sqlx::mysql::MySqlPool;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::{Column, Row};

// ── SQLite connection ──

pub async fn connect_sqlite(path: &str) -> Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&format!("sqlite:{}", path))
        .await?;

    // Create the records table if it doesn't exist
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS records (
            module TEXT NOT NULL,
            id TEXT NOT NULL,
            data TEXT NOT NULL,
            PRIMARY KEY (module, id)
        )",
    )
    .execute(&pool)
    .await?;

    // Create index for module lookups
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_records_module ON records(module)",
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

// ── MySQL connection (import source only) ──

pub async fn connect_mysql(database_url: &str) -> Result<MySqlPool> {
    let pool = MySqlPool::connect(database_url).await?;
    Ok(pool)
}

// ── Import: MySQL → SQLite ──

pub async fn import_from_mysql(source_url: &str, target_path: &str) -> Result<()> {
    let mysql = connect_mysql(source_url).await?;
    let sqlite = connect_sqlite(target_path).await?;

    let modules = discover_modules_mysql(&mysql).await?;
    tracing::info!("Importing {} modules...", modules.len());

    for module in &modules {
        import_module(&mysql, &sqlite, module).await?;
    }

    // Also import relationship metadata
    import_relationships(&mysql, &sqlite).await?;

    Ok(())
}

async fn import_module(mysql: &MySqlPool, sqlite: &SqlitePool, module: &str) -> Result<()> {
    // Check if _cstm table exists
    let cstm_exists = table_exists(mysql, &format!("{}_cstm", module)).await?;

    let sql = if cstm_exists {
        format!(
            "SELECT b.*, c.* FROM `{}` b LEFT JOIN `{}_cstm` c ON b.id = c.id_c",
            module, module
        )
    } else {
        format!("SELECT * FROM `{}`", module)
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
            let value: Value = row
                .try_get::<Option<String>, _>(name.as_str())
                .unwrap_or(None)
                .map(Value::String)
                .unwrap_or(Value::Null);
            map.insert(name, value);
        }

        let id = map
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let json = serde_json::to_string(&map)?;

        sqlx::query(
            "INSERT OR REPLACE INTO records (module, id, data) VALUES (?, ?, ?)",
        )
        .bind(module)
        .bind(&id)
        .bind(&json)
        .execute(sqlite)
        .await?;

        count += 1;
    }

    tracing::info!("  {}: {} records", module, count);
    Ok(())
}

async fn import_relationships(mysql: &MySqlPool, sqlite: &SqlitePool) -> Result<()> {
    let rows = sqlx::query(
        "SELECT relationship_name, lhs_module, lhs_table, rhs_module, rhs_table
         FROM relationships"
    )
    .fetch_all(mysql)
    .await?;

    for row in &rows {
        let rel_name: String = row.get("relationship_name");
        let lhs_module: String = row.get("lhs_module");
        let lhs_table: String = row.get("lhs_table");
        let rhs_module: String = row.get("rhs_module");
        let rhs_table: String = row.get("rhs_table");

        let mut map = Map::new();
        map.insert("relationship_name".into(), Value::String(rel_name.clone()));
        map.insert("lhs_module".into(), Value::String(lhs_module));
        map.insert("lhs_table".into(), Value::String(lhs_table));
        map.insert("rhs_module".into(), Value::String(rhs_module));
        map.insert("rhs_table".into(), Value::String(rhs_table));

        let json = serde_json::to_string(&map)?;
        sqlx::query(
            "INSERT OR REPLACE INTO records (module, id, data) VALUES (?, ?, ?)",
        )
        .bind("_relationships")
        .bind(&rel_name)
        .bind(&json)
        .execute(sqlite)
        .await?;
    }

    tracing::info!("  _relationships: {} definitions", rows.len());
    Ok(())
}

// ── Module discovery ──

pub async fn discover_modules_mysql(pool: &MySqlPool) -> Result<Vec<String>> {
    let rows = sqlx::query(
        r#"
        SELECT CAST(TABLE_NAME AS CHAR) as table_name
        FROM information_schema.TABLES
        WHERE TABLE_SCHEMA = DATABASE()
          AND TABLE_TYPE = 'BASE TABLE'
          AND TABLE_NAME NOT LIKE '%\_cstm'
          AND TABLE_NAME NOT LIKE '%\_audit'
          AND TABLE_NAME NOT IN (
            'aow_conditions', 'aow_actions', 'aow_processed',
            'aor_conditions', 'aor_fields', 'aor_charts',
            'fields_meta_data', 'relationships',
            'tracker', 'tracker_sessions', 'tracker_perf',
            'schedulers', 'schedulers_times', 'job_queue',
            'config', 'versions', 'upgrade_history',
            'address_book', 'email_cache', 'email_addr_bean_rel',
            'email_addresses', 'email_man', 'email_templates',
            'campaign_log', 'campaign_trkrs',
            'oauth_tokens', 'oauth_nonce',
            'address_book_locked', 'inbound_email',
            'outbound_email', 'user_preferences',
            'roles', 'role_modules', 'acl_roles', 'acl_actions',
            'saved_search', 'favorites', 'subscriptions',
            'eapm', 'oauth2tokens', 'oauth2clients'
          )
        ORDER BY TABLE_NAME
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| r.get::<String, _>("table_name"))
        .collect())
}

pub async fn discover_modules_sqlite(pool: &SqlitePool) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT module FROM records WHERE module != '_relationships' ORDER BY module",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(|(name,)| name).collect())
}

// ── SQLite queries ──

pub async fn query_module(
    pool: &SqlitePool,
    module: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<Map<String, Value>>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT data FROM records WHERE module = ? ORDER BY id LIMIT ? OFFSET ?",
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
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT data FROM records WHERE module = ? AND id = ?",
    )
    .bind(module)
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.and_then(|(json,)| serde_json::from_str(&json).ok()))
}

pub async fn count_module(pool: &SqlitePool, module: &str) -> Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) as cnt FROM records WHERE module = ?",
    )
    .bind(module)
    .fetch_one(pool)
    .await?;

    Ok(row.0)
}

pub async fn get_relationships(
    pool: &SqlitePool,
    module: &str,
    id: &str,
) -> Result<Vec<Map<String, Value>>> {
    // Get relationship definitions for this module
    let rel_rows: Vec<(String,)> = sqlx::query_as(
        "SELECT data FROM records WHERE module = '_relationships'",
    )
    .fetch_all(pool)
    .await?;

    let mut results = Vec::new();

    for (json,) in rel_rows {
        let rel: Map<String, Value> = match serde_json::from_str(&json) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let rel_name = rel.get("relationship_name").and_then(|v| v.as_str()).unwrap_or("");
        let lhs_module = rel.get("lhs_module").and_then(|v| v.as_str()).unwrap_or("");
        let rhs_module = rel.get("rhs_module").and_then(|v| v.as_str()).unwrap_or("");
        let lhs_table = rel.get("lhs_table").and_then(|v| v.as_str()).unwrap_or("");
        let rhs_table = rel.get("rhs_table").and_then(|v| v.as_str()).unwrap_or("");

        if !lhs_module.eq_ignore_ascii_case(module) && !rhs_module.eq_ignore_ascii_case(module) {
            continue;
        }

        // Determine which side we are and the FK column names (singularized)
        let (target_module, my_fk, their_fk) = if lhs_module.eq_ignore_ascii_case(module) {
            (rhs_table, format!("{}_id", singularize(lhs_table)), format!("{}_id", singularize(rhs_table)))
        } else {
            (lhs_table, format!("{}_id", singularize(rhs_table)), format!("{}_id", singularize(lhs_table)))
        };

        // Step 1: Query the junction table for related record IDs
        let jn_sql = format!(
            "SELECT json_extract(data, '$.{}') as related_id FROM records WHERE module = '{}' AND json_extract(data, '$.{}') = '{}'",
            their_fk, rel_name, my_fk, id
        );
        let jn_rows: Vec<(Option<String>,)> = sqlx::query_as(&jn_sql)
            .fetch_all(pool)
            .await?;

        // Step 2: Look up each related record
        for (related_id,) in jn_rows {
            if let Some(rid) = related_id {
                let rec_sql = format!(
                    "SELECT data FROM records WHERE module = '{}' AND id = '{}'",
                    target_module, rid
                );
                if let Ok(Some((rec_json,))) = sqlx::query_as::<_, (String,)>(&rec_sql)
                    .fetch_optional(pool)
                    .await
                {
                    if let Ok(record) = serde_json::from_str::<Map<String, Value>>(&rec_json) {
                        results.push(record);
                    }
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
