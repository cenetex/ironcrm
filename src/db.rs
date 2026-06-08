use anyhow::Result;
use serde_json::{Map, Value};
use sqlx::mysql::MySqlPool;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::{Column, Row};

// ── SQLite connection + schema ──

pub async fn connect_sqlite(path: &str) -> Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&format!("sqlite:{}", path))
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
        "CREATE TABLE IF NOT EXISTS did_registry (
            module TEXT NOT NULL, record_id TEXT NOT NULL,
            did TEXT NOT NULL UNIQUE,
            did_document TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (module, record_id)
        )",
    )
    .execute(pool)
    .await?;

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
                continue;  // let SQLite DEFAULT handle it
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

    import_relationships(&mysql, &sqlite).await?;
    Ok(())
}

async fn import_module(mysql: &MySqlPool, sqlite: &SqlitePool, module: &str) -> Result<()> {
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
            let value = extract_mysql_value(row, &name);
            map.insert(name, value);
        }

        let id = map
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Always write blob store
        let json = serde_json::to_string(&map)?;
        sqlx::query(
            "INSERT OR REPLACE INTO records (module, id, data) VALUES (?, ?, ?)",
        )
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
    Ok(())
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
                bytes[0], bytes[1], bytes[2], bytes[3],
                bytes[4], bytes[5], bytes[6], bytes[7],
                bytes[8], bytes[9], bytes[10], bytes[11],
                bytes[12], bytes[13], bytes[14], bytes[15],
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

async fn import_relationships(mysql: &MySqlPool, sqlite: &SqlitePool) -> Result<()> {
    let rows = sqlx::query(
        "SELECT relationship_name, lhs_module, lhs_table, rhs_module, rhs_table
         FROM relationships",
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
        map.insert(
            "relationship_name".into(),
            Value::String(rel_name.clone()),
        );
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
            let count_sql = format!("SELECT COUNT(*) as cnt FROM {}", module);
            if let Ok((cnt,)) = sqlx::query_as::<_, (i64,)>(&count_sql)
                .fetch_one(pool)
                .await
            {
                if cnt > 0 {
                    modules.push(module.to_string());
                }
            }
        }
    }

    modules.sort();
    Ok(modules)
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

        let results: Vec<Map<String, Value>> = rows
            .iter()
            .map(|row| {
                let mut map = Map::new();
                for col in row.columns() {
                    let name = col.name().to_string();
                    let val: Option<String> = row.try_get(name.as_str()).unwrap_or(None);
                    map.insert(name, val.map(Value::String).unwrap_or(Value::Null));
                }
                map
            })
            .collect();
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
            let mut map = Map::new();
            for col in row.columns() {
                let name = col.name().to_string();
                let val: Option<String> = row.try_get(name.as_str()).unwrap_or(None);
                map.insert(name, val.map(Value::String).unwrap_or(Value::Null));
            }
            return Ok(Some(map));
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
                if let Ok(Some(record)) =
                    get_module_record(pool, target_module, &rid).await
                {
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
