use crate::models::ModuleSchema;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePool;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleDefinition {
    pub module: String,
    pub label: String,
    pub label_plural: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default = "default_view")]
    pub default_view: String,
    #[serde(default)]
    pub board_field: Option<String>,
    #[serde(default)]
    pub board_stages: Option<Vec<String>>,
    #[serde(default)]
    pub list_columns: Vec<String>,
    #[serde(default)]
    pub detail_summary: Vec<String>,
    #[serde(default)]
    pub primary_fields: Vec<String>,
    #[serde(default)]
    pub fields: Vec<FieldDefinition>,
    #[serde(default)]
    pub scan_mapping: Option<ScanMapping>,
    #[serde(default)]
    pub indexes: Vec<String>,
}

fn default_view() -> String {
    "table".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDefinition {
    pub name: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(rename = "type", default = "default_field_type")]
    pub field_type: String,
    #[serde(default)]
    pub position: u32,
    #[serde(default)]
    pub scan_source: Option<String>,
    #[serde(default)]
    pub default_value: Option<String>,
}

fn default_field_type() -> String {
    "TEXT".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScanMapping {
    #[serde(default)]
    pub sources: HashMap<String, String>,
    #[serde(default)]
    pub split_name: bool,
}

// ── Schema ──

pub async fn init_definitions_table(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS module_definitions (
            module TEXT PRIMARY KEY,
            label TEXT NOT NULL,
            label_plural TEXT NOT NULL,
            def_json TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn seed_builtin_definitions(pool: &SqlitePool) -> Result<()> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM module_definitions")
        .fetch_one(pool)
        .await?;

    if count.0 > 0 {
        tracing::info!("module_definitions already seeded ({} rows)", count.0);
        return Ok(());
    }

    let builtins = builtins();
    for def in &builtins {
        let json = serde_json::to_string(def)?;
        sqlx::query(
            "INSERT OR REPLACE INTO module_definitions (module, label, label_plural, def_json) VALUES (?, ?, ?, ?)",
        )
        .bind(&def.module)
        .bind(&def.label)
        .bind(&def.label_plural)
        .bind(&json)
        .execute(pool)
        .await?;
    }

    tracing::info!("Seeded {} built-in module definitions", builtins.len());
    Ok(())
}

pub async fn get_definition(pool: &SqlitePool, module: &str) -> Result<Option<ModuleDefinition>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT def_json FROM module_definitions WHERE module = ?")
            .bind(module)
            .fetch_optional(pool)
            .await?;

    Ok(match row {
        Some((json,)) => Some(serde_json::from_str(&json)?),
        None => None,
    })
}

pub async fn get_all_definitions(pool: &SqlitePool) -> Result<Vec<ModuleDefinition>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT def_json FROM module_definitions ORDER BY module")
            .fetch_all(pool)
            .await?;

    let mut defs = Vec::new();
    for (json,) in rows {
        defs.push(serde_json::from_str(&json)?);
    }
    Ok(defs)
}

pub async fn has_definition(pool: &SqlitePool, module: &str) -> Result<bool> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM module_definitions WHERE module = ?")
        .bind(module)
        .fetch_optional(pool)
        .await?;

    Ok(row.is_some())
}

pub async fn typed_column_names(pool: &SqlitePool, module: &str) -> Result<Vec<String>> {
    if let Some(def) = get_definition(pool, module).await? {
        Ok(def.fields.iter().map(|f| f.name.clone()).collect())
    } else {
        Ok(Vec::new())
    }
}

// ── Schema generation ──

pub fn gen_create_table_sql(def: &ModuleDefinition) -> String {
    let cols: Vec<String> = def
        .fields
        .iter()
        .map(|f| {
            let mut col = format!("\"{}\" {}", f.name, f.field_type);
            if f.name == "id" {
                col.push_str(" PRIMARY KEY");
            }
            if let Some(ref dv) = f.default_value {
                col.push_str(&format!(" DEFAULT {}", dv));
            }
            col
        })
        .collect();
    format!(
        "CREATE TABLE IF NOT EXISTS {} (\n  {}\n)",
        def.module,
        cols.join(",\n  ")
    )
}

pub fn gen_index_sqls(def: &ModuleDefinition) -> Vec<String> {
    def.indexes
        .iter()
        .map(|idx| {
            let safe_name = idx.replace(',', "_").replace(' ', "");
            let cols_quoted: Vec<String> = idx
                .split(',')
                .map(|c| format!("\"{}\"", c.trim()))
                .collect();
            format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_{} ON {}({})",
                def.module,
                safe_name,
                def.module,
                cols_quoted.join(", ")
            )
        })
        .collect()
}

// ── Built-in definitions ──

fn builtins() -> Vec<ModuleDefinition> {
    vec![
        crate::models::Account::definition(),
        crate::models::Contact::definition(),
        crate::models::Lead::definition(),
        crate::models::Opportunity::definition(),
        crate::models::Case::definition(),
    ]
}
