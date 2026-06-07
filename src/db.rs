use anyhow::{Context, Result};
use serde_json::{Map, Value};
use sqlx::mysql::MySqlPool;
use sqlx::{Column, Row, TypeInfo};

/// Create a connection pool to the MySQL database.
pub async fn connect(database_url: &str) -> Result<MySqlPool> {
    let pool = MySqlPool::connect(database_url).await?;
    Ok(pool)
}

/// Discover SuiteCRM modules by inspecting the database schema.
pub async fn discover_modules(pool: &MySqlPool) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT TABLE_NAME
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

    Ok(rows.into_iter().map(|(name,)| name).collect())
}

/// Query any module table, merging custom fields from its `_cstm` table if it exists.
pub async fn query_module(
    pool: &MySqlPool,
    module: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<Map<String, Value>>> {
    let cstm_exists = table_exists(pool, &format!("{}_cstm", module)).await?;

    let sql = if cstm_exists {
        format!(
            "SELECT b.*, c.* \
             FROM `{}` b \
             LEFT JOIN `{}_cstm` c ON b.id = c.id_c \
             WHERE b.deleted = 0 \
             ORDER BY b.date_entered DESC \
             LIMIT {} OFFSET {}",
            module, module, limit, offset
        )
    } else {
        format!(
            "SELECT * FROM `{}` WHERE deleted = 0 \
             ORDER BY date_entered DESC \
             LIMIT {} OFFSET {}",
            module, limit, offset
        )
    };

    let rows = sqlx::query(&sql).fetch_all(pool).await?;

    let results: Vec<Map<String, Value>> = rows
        .iter()
        .map(|row| {
            let mut map = Map::new();
            for col in row.columns() {
                let name = col.name().to_string();
                let value: Value = match col.type_info().name() {
                    "VARCHAR" | "TEXT" | "CHAR" | "ENUM" | "LONGTEXT" | "MEDIUMTEXT" => row
                        .try_get::<Option<String>, _>(name.as_str())
                        .unwrap_or(None)
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                    "INT" | "TINYINT" | "SMALLINT" | "MEDIUMINT" | "BIGINT" => {
                        if col.type_info().name() == "TINYINT" && col.name().starts_with("deleted") {
                            row.try_get::<Option<i8>, _>(name.as_str())
                                .unwrap_or(None)
                                .map(|v| Value::Bool(v != 0))
                                .unwrap_or(Value::Null)
                        } else {
                            row.try_get::<Option<i64>, _>(name.as_str())
                                .unwrap_or(None)
                                .map(|v| Value::Number(v.into()))
                                .unwrap_or(Value::Null)
                        }
                    }
                    "FLOAT" | "DOUBLE" | "DECIMAL" => row
                        .try_get::<Option<f64>, _>(name.as_str())
                        .unwrap_or(None)
                        .map(|v| {
                            serde_json::Number::from_f64(v)
                                .map(Value::Number)
                                .unwrap_or(Value::Null)
                        })
                        .unwrap_or(Value::Null),
                    "DATETIME" | "TIMESTAMP" => row
                        .try_get::<Option<String>, _>(name.as_str())
                        .unwrap_or(None)
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                    _ => Value::Null,
                };
                // Skip duplicate id_c column from _cstm join
                if name != "id_c" || !cstm_exists {
                    map.insert(name, value);
                }
            }
            map
        })
        .collect();

    Ok(results)
}

/// Get a single record by ID.
pub async fn get_module_record(pool: &MySqlPool, module: &str, id: &str) -> Result<Option<Map<String, Value>>> {
    let cstm_exists = table_exists(pool, &format!("{}_cstm", module)).await?;

    let sql = if cstm_exists {
        format!(
            "SELECT b.*, c.* FROM `{}` b \
             LEFT JOIN `{}_cstm` c ON b.id = c.id_c \
             WHERE b.id = ?",
            module, module
        )
    } else {
        format!("SELECT * FROM `{}` WHERE id = ?", module)
    };

    let rows = sqlx::query(&sql).bind(id).fetch_all(pool).await?;

    if rows.is_empty() {
        return Ok(None);
    }

    let row = &rows[0];
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

    Ok(Some(map))
}

/// Count non-deleted records in a module.
pub async fn count_module(pool: &MySqlPool, module: &str) -> Result<i64> {
    let sql = format!(
        "SELECT COUNT(*) as cnt FROM `{}` WHERE deleted = 0",
        module
    );
    let row = sqlx::query(&sql)
        .fetch_one(pool)
        .await
        .with_context(|| format!("Failed to count module '{}'", module))?;
    Ok(row.get::<i64, _>("cnt"))
}

/// List relationships for a module record.
pub async fn get_relationships(
    pool: &MySqlPool,
    module: &str,
    id: &str,
) -> Result<Vec<Map<String, Value>>> {
    #[derive(sqlx::FromRow)]
    struct RelationshipRow {
        relationship_name: String,
        lhs_module: String,
        lhs_table: String,
        lhs_key: String,
        rhs_module: String,
        rhs_table: String,
        rhs_key: String,
    }

    let rows = sqlx::query_as::<_, RelationshipRow>(
        r#"
        SELECT
            relationship_name,
            lhs_module, lhs_table, lhs_key,
            rhs_module, rhs_table, rhs_key
        FROM relationships
        WHERE lhs_module = ? OR rhs_module = ?
        "#,
    )
    .bind(module)
    .bind(module)
    .fetch_all(pool)
    .await?;

    let mut results = Vec::new();
    for r in rows {
        let rel_name = r.relationship_name;
        let lhs_mod = r.lhs_module;
        let lhs_tbl = r.lhs_table;
        let lhs_key = r.lhs_key;
        let _rhs_mod = r.rhs_module;
        let rhs_tbl = r.rhs_table;
        let rhs_key = r.rhs_key;
        let (target_table, join_key, other_key) = if lhs_mod == module {
            (&rhs_tbl, &rhs_key, &lhs_key)
        } else {
            (&lhs_tbl, &lhs_key, &rhs_key)
        };

        let sql = format!(
            "SELECT t.* FROM `{}` t \
             INNER JOIN `{}` r ON t.id = r.{} \
             WHERE r.{} = ? AND t.deleted = 0 \
             LIMIT 100",
            target_table, rel_name, join_key, other_key
        );

        if let Ok(related_rows) = sqlx::query(&sql).bind(id).fetch_all(pool).await {
            for row in related_rows {
                let mut map = Map::new();
                for col in row.columns() {
                    let name = col.name().to_string();
                    let value: Value = row
                        .try_get::<Option<String>, _>(name.as_str())
                        .unwrap_or(None)
                        .map(Value::String)
                        .unwrap_or(Value::Null);
                    map.insert(name, value);
                }
                results.push(map);
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
