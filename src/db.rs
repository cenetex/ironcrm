use anyhow::Result;
use serde_json::{Map, Value};
use sqlx::mysql::MySqlPool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::{Column, Row};
use std::collections::HashSet;
use std::str::FromStr;

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
    let definitions = crate::module::get_all_definitions(pool).await?;
    let modules = discover_modules_sqlite(pool).await?;
    let mut records = Vec::new();

    for module in &modules {
        for record in query_module(pool, module, 100_000, 0).await? {
            records.push(serde_json::json!({
                "module": module,
                "data": record
            }));
        }
    }

    let activity_rows = sqlx::query(
        "SELECT id, module, record_id, operation, actor_id, changed_fields, created_at
         FROM activity_log
         ORDER BY created_at ASC, id ASC",
    )
    .fetch_all(pool)
    .await?;
    let activities: Vec<Value> = activity_rows
        .iter()
        .map(|row| Value::Object(sqlite_row_to_map(row)))
        .collect();

    let link_rows = sqlx::query(
        "SELECT id, source_module, source_id, target_module, target_id, relationship_type, created_at
         FROM record_links
         ORDER BY created_at ASC, id ASC",
    )
    .fetch_all(pool)
    .await?;
    let links: Vec<Value> = link_rows
        .iter()
        .map(|row| Value::Object(sqlite_row_to_map(row)))
        .collect();

    Ok(serde_json::json!({
        "kind": "ironcrm.workspace",
        "version": 1,
        "exported_at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "module_definitions": definitions,
        "records": records,
        "activities": activities,
        "record_links": links
    }))
}

fn value_to_sql_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        Some(Value::Null) | None => String::new(),
        Some(value) => value.to_string(),
    }
}

pub async fn restore_workspace(pool: &SqlitePool, bundle: &Value) -> Result<Map<String, Value>> {
    let kind = bundle.get("kind").and_then(Value::as_str).unwrap_or("");
    let version = bundle.get("version").and_then(Value::as_i64).unwrap_or(0);
    anyhow::ensure!(kind == "ironcrm.workspace", "Not an IronCRM workspace bundle");
    anyhow::ensure!(version == 1, "Unsupported IronCRM bundle version {}", version);

    let mut restored_modules = 0u64;
    let mut restored_records = 0u64;
    let mut restored_activities = 0u64;
    let mut restored_links = 0u64;

    if let Some(definitions) = bundle.get("module_definitions").and_then(Value::as_array) {
        for def_value in definitions {
            let def: crate::module::ModuleDefinition = serde_json::from_value(def_value.clone())?;
            let json = serde_json::to_string(&def)?;
            sqlx::query(
                "INSERT OR REPLACE INTO module_definitions
                 (module, label, label_plural, def_json)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .bind(&def.module)
            .bind(&def.label)
            .bind(&def.label_plural)
            .bind(&json)
            .execute(pool)
            .await?;
            sqlx::query(&crate::module::gen_create_table_sql(&def))
                .execute(pool)
                .await?;
            for idx_sql in crate::module::gen_index_sqls(&def) {
                sqlx::query(&idx_sql).execute(pool).await?;
            }
            restored_modules += 1;
        }
    }

    if let Some(records) = bundle.get("records").and_then(Value::as_array) {
        for record_value in records {
            let Some(module) = record_value.get("module").and_then(Value::as_str) else {
                continue;
            };
            let Some(data) = record_value.get("data").and_then(Value::as_object) else {
                continue;
            };
            let Some(id) = data.get("id").and_then(Value::as_str) else {
                continue;
            };
            let json = serde_json::to_string(data)?;
            sqlx::query("INSERT OR REPLACE INTO records (module, id, data) VALUES (?1, ?2, ?3)")
                .bind(module)
                .bind(id)
                .bind(json)
                .execute(pool)
                .await?;
            if crate::module::has_definition(pool, module).await? {
                upsert_typed_row(pool, module, id, data).await?;
            }
            restored_records += 1;
        }
    }

    if let Some(activities) = bundle.get("activities").and_then(Value::as_array) {
        for activity in activities {
            let Some(activity) = activity.as_object() else {
                continue;
            };
            let id = value_to_sql_string(activity.get("id"));
            if id.is_empty() {
                continue;
            }
            let changed_fields = match activity.get("changed_fields") {
                Some(Value::String(value)) => value.clone(),
                Some(value) => serde_json::to_string(value)?,
                None => "{}".into(),
            };
            sqlx::query(
                "INSERT OR REPLACE INTO activity_log
                 (id, module, record_id, operation, actor_id, changed_fields, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .bind(id)
            .bind(value_to_sql_string(activity.get("module")))
            .bind(value_to_sql_string(activity.get("record_id")))
            .bind(value_to_sql_string(activity.get("operation")))
            .bind(value_to_sql_string(activity.get("actor_id")))
            .bind(changed_fields)
            .bind(
                activity
                    .get("created_at")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                    }),
            )
            .execute(pool)
            .await?;
            restored_activities += 1;
        }
    }

    if let Some(links) = bundle.get("record_links").and_then(Value::as_array) {
        for link in links {
            let Some(link) = link.as_object() else {
                continue;
            };
            let id = value_to_sql_string(link.get("id"));
            if id.is_empty() {
                continue;
            }
            sqlx::query(
                "INSERT OR REPLACE INTO record_links
                 (id, source_module, source_id, target_module, target_id, relationship_type, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .bind(id)
            .bind(value_to_sql_string(link.get("source_module")))
            .bind(value_to_sql_string(link.get("source_id")))
            .bind(value_to_sql_string(link.get("target_module")))
            .bind(value_to_sql_string(link.get("target_id")))
            .bind(
                link.get("relationship_type")
                    .and_then(Value::as_str)
                    .unwrap_or("related"),
            )
            .bind(
                link.get("created_at")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        chrono::Utc::now()
                            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                    }),
            )
            .execute(pool)
            .await?;
            restored_links += 1;
        }
    }

    let mut result = Map::new();
    result.insert("modules".into(), serde_json::json!(restored_modules));
    result.insert("records".into(), serde_json::json!(restored_records));
    result.insert("activities".into(), serde_json::json!(restored_activities));
    result.insert("record_links".into(), serde_json::json!(restored_links));
    Ok(result)
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

        assert_eq!(operations.len(), 4);
        assert!(operations.contains(&"create".to_string()));
        assert!(operations.contains(&"update".to_string()));
        assert!(operations.contains(&"note".to_string()));
        assert!(operations.contains(&"delete".to_string()));
    }

    #[tokio::test]
    async fn links_search_and_bundle_restore_round_trip() {
        let pool = test_pool().await;

        let mut contact = Map::new();
        contact.insert("first_name".into(), json!("Ada"));
        contact.insert("last_name".into(), json!("Lovelace"));
        contact.insert("email1".into(), json!("ada@example.com"));
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
        let restored_pool = test_pool().await;
        let summary = restore_workspace(&restored_pool, &bundle)
            .await
            .expect("restore workspace");
        assert_eq!(summary.get("records").and_then(Value::as_u64), Some(2));
        assert_eq!(summary.get("record_links").and_then(Value::as_u64), Some(1));

        let restored_contact = get_module_record(&restored_pool, "contacts", &contact_id)
            .await
            .expect("get restored contact")
            .expect("restored contact exists");
        assert_eq!(
            restored_contact.get("email1").and_then(Value::as_str),
            Some("ada@example.com")
        );

        let restored_relationships = get_relationships(&restored_pool, "contacts", &contact_id)
            .await
            .expect("restored relationships");
        assert_eq!(restored_relationships.len(), 1);
        assert_eq!(
            restored_relationships[0].get("name").and_then(Value::as_str),
            Some("Analytical Engines Ltd")
        );
    }

    #[tokio::test]
    async fn restore_rejects_non_workspace_bundle() {
        let pool = test_pool().await;
        let result = restore_workspace(&pool, &json!({"records": []})).await;
        assert!(result.is_err());
    }
}
