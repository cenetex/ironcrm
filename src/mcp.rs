use anyhow::{Result, anyhow};
use serde_json::{Map, Value, json};
use sqlx::sqlite::SqlitePool;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

const SERVER_NAME: &str = "ironcrm";

pub async fn run_mcp_server(db_path: &str) -> Result<()> {
    let pool = crate::db::connect_sqlite(db_path).await?;
    tracing::info!(db_path, "White Warren MCP server ready");

    let stdin = io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = BufWriter::new(io::stdout());

    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<Value>(line) {
            Ok(message) => {
                if message.get("id").is_none() {
                    continue;
                }
                handle_request(&pool, message).await
            }
            Err(error) => {
                error_response(Value::Null, -32700, "Parse error", Some(error.to_string()))
            }
        };

        stdout.write_all(response.to_string().as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }

    Ok(())
}

async fn handle_request(pool: &SqlitePool, message: Value) -> Value {
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));

    let result = match method {
        "initialize" => Ok(initialize_result(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => call_tool(pool, &params).await,
        "resources/list" => Ok(json!({ "resources": resources() })),
        "resources/read" => read_resource(pool, &params).await,
        "prompts/list" => Ok(json!({ "prompts": [] })),
        _ => return error_response(id, -32601, "Method not found", Some(method.to_string())),
    };

    match result {
        Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
        Err(error) => tool_error_response(id, error.to_string()),
    }
}

fn initialize_result(params: &Value) -> Value {
    let protocol_version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("2024-11-05");

    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": { "listChanged": false },
            "resources": { "subscribe": false, "listChanged": false }
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn tools() -> Vec<Value> {
    vec![
        tool(
            "ironcrm_workspace_summary",
            "Summarize the local White Warren workspace, including modules and record counts.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool(
            "ironcrm_list_modules",
            "List CRM modules and their field definitions.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool(
            "ironcrm_list_records",
            "List records from one module.",
            json!({
                "type": "object",
                "properties": {
                    "module": { "type": "string", "description": "Module name, for example contacts or accounts." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 25 },
                    "offset": { "type": "integer", "minimum": 0, "default": 0 }
                },
                "required": ["module"],
                "additionalProperties": false
            }),
        ),
        tool(
            "ironcrm_search_records",
            "Search records and note activity across all CRM modules.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 25 }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        ),
        tool(
            "ironcrm_get_record",
            "Get one record, including relationships and activity timeline.",
            json!({
                "type": "object",
                "properties": {
                    "module": { "type": "string" },
                    "id": { "type": "string" }
                },
                "required": ["module", "id"],
                "additionalProperties": false
            }),
        ),
        tool(
            "ironcrm_create_record",
            "Create a local White Warren record.",
            json!({
                "type": "object",
                "properties": {
                    "module": { "type": "string" },
                    "fields": { "type": "object", "description": "Field values keyed by CRM field name." }
                },
                "required": ["module", "fields"],
                "additionalProperties": false
            }),
        ),
        tool(
            "ironcrm_update_record",
            "Update fields on a local White Warren record.",
            json!({
                "type": "object",
                "properties": {
                    "module": { "type": "string" },
                    "id": { "type": "string" },
                    "fields": { "type": "object", "description": "Replacement field values keyed by CRM field name." }
                },
                "required": ["module", "id", "fields"],
                "additionalProperties": false
            }),
        ),
        tool(
            "ironcrm_restore_record",
            "Restore a soft-deleted White Warren record.",
            json!({
                "type": "object",
                "properties": {
                    "module": { "type": "string" },
                    "id": { "type": "string" }
                },
                "required": ["module", "id"],
                "additionalProperties": false
            }),
        ),
        tool(
            "ironcrm_add_note",
            "Add an activity note to a record.",
            json!({
                "type": "object",
                "properties": {
                    "module": { "type": "string" },
                    "id": { "type": "string" },
                    "note": { "type": "string" }
                },
                "required": ["module", "id", "note"],
                "additionalProperties": false
            }),
        ),
        tool(
            "ironcrm_link_records",
            "Create a relationship between two records.",
            json!({
                "type": "object",
                "properties": {
                    "source_module": { "type": "string" },
                    "source_id": { "type": "string" },
                    "target_module": { "type": "string" },
                    "target_id": { "type": "string" },
                    "relationship_type": { "type": "string", "default": "related" }
                },
                "required": ["source_module", "source_id", "target_module", "target_id"],
                "additionalProperties": false
            }),
        ),
        tool(
            "ironcrm_detect_suitecrm",
            "Detect a local SuiteCRM config.php and return a read-only connection summary.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool(
            "ironcrm_export_workspace",
            "Export a complete White Warren workspace bundle as JSON.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
    ]
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

fn resources() -> Vec<Value> {
    vec![
        json!({
            "uri": "ironcrm://workspace/summary",
            "name": "White Warren workspace summary",
            "description": "Module names and local record counts.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "ironcrm://modules",
            "name": "White Warren module definitions",
            "description": "CRM module schemas and field definitions.",
            "mimeType": "application/json"
        }),
    ]
}

async fn read_resource(pool: &SqlitePool, params: &Value) -> Result<Value> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("resources/read requires uri"))?;

    let value = match uri {
        "ironcrm://workspace/summary" => workspace_summary(pool).await?,
        "ironcrm://modules" => modules_payload(pool).await?,
        _ => return Err(anyhow!("Unknown resource URI: {uri}")),
    };

    Ok(json!({
        "contents": [{
            "uri": uri,
            "mimeType": "application/json",
            "text": pretty(&value)
        }]
    }))
}

async fn call_tool(pool: &SqlitePool, params: &Value) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("tools/call requires name"))?;
    let arguments = params
        .get("arguments")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let payload = match name {
        "ironcrm_workspace_summary" => workspace_summary(pool).await?,
        "ironcrm_list_modules" => modules_payload(pool).await?,
        "ironcrm_list_records" => {
            let module = string_arg(&arguments, "module")?;
            let limit = u32_arg(&arguments, "limit", 25, 1, 100)?;
            let offset = u32_arg(&arguments, "offset", 0, 0, u32::MAX)?;
            let data = crate::db::query_module(pool, &module, limit, offset).await?;
            let total = crate::db::count_module(pool, &module).await?;
            json!({ "module": module, "data": data, "total": total, "limit": limit, "offset": offset })
        }
        "ironcrm_search_records" => {
            let query = string_arg(&arguments, "query")?;
            let limit = u32_arg(&arguments, "limit", 25, 1, 100)?;
            let data = crate::db::search_records(pool, &query, limit).await?;
            json!({ "query": query, "total": data.len(), "data": data })
        }
        "ironcrm_get_record" => {
            let module = string_arg(&arguments, "module")?;
            let id = string_arg(&arguments, "id")?;
            let data = crate::db::get_module_record(pool, &module, &id).await?;
            let relationships = crate::db::get_relationships(pool, &module, &id).await?;
            let activities = crate::db::get_record_activities(pool, &module, &id, 25).await?;
            json!({ "module": module, "id": id, "data": data, "relationships": relationships, "activities": activities })
        }
        "ironcrm_create_record" => {
            let module = string_arg(&arguments, "module")?;
            let fields = object_arg(&arguments, "fields")?;
            ensure_module(pool, &module).await?;
            let id = crate::db::create_record(pool, &module, &fields).await?;
            json!({ "status": "ok", "module": module, "id": id })
        }
        "ironcrm_update_record" => {
            let module = string_arg(&arguments, "module")?;
            let id = string_arg(&arguments, "id")?;
            let fields = object_arg(&arguments, "fields")?;
            ensure_module(pool, &module).await?;
            let updated = crate::db::update_record(pool, &module, &id, &fields).await?;
            if !updated {
                return Err(anyhow!("Record '{id}' not found in '{module}'"));
            }
            json!({ "status": "ok", "module": module, "id": id })
        }
        "ironcrm_restore_record" => {
            let module = string_arg(&arguments, "module")?;
            let id = string_arg(&arguments, "id")?;
            ensure_module(pool, &module).await?;
            let restored = crate::db::restore_record(pool, &module, &id).await?;
            if !restored {
                return Err(anyhow!("Record '{id}' not found in '{module}'"));
            }
            json!({ "status": "ok", "module": module, "id": id })
        }
        "ironcrm_add_note" => {
            let module = string_arg(&arguments, "module")?;
            let id = string_arg(&arguments, "id")?;
            let note = string_arg(&arguments, "note")?;
            let added = crate::db::add_record_note(pool, &module, &id, &note).await?;
            if !added {
                return Err(anyhow!("Record '{id}' not found in '{module}'"));
            }
            json!({ "status": "ok", "module": module, "id": id })
        }
        "ironcrm_link_records" => {
            let source_module = string_arg(&arguments, "source_module")?;
            let source_id = string_arg(&arguments, "source_id")?;
            let target_module = string_arg(&arguments, "target_module")?;
            let target_id = string_arg(&arguments, "target_id")?;
            let relationship_type = arguments
                .get("relationship_type")
                .and_then(Value::as_str)
                .unwrap_or("related");
            let id = crate::db::link_records(
                pool,
                &source_module,
                &source_id,
                &target_module,
                &target_id,
                Some(relationship_type),
            )
            .await?
            .ok_or_else(|| anyhow!("Could not link those records"))?;
            json!({ "status": "ok", "id": id })
        }
        "ironcrm_detect_suitecrm" => serde_json::to_value(crate::db::detect_local_suitecrm())?,
        "ironcrm_export_workspace" => crate::db::export_workspace(pool).await?,
        _ => return Err(anyhow!("Unknown tool: {name}")),
    };

    Ok(json!({
        "content": [{ "type": "text", "text": pretty(&payload) }],
        "isError": false
    }))
}

async fn workspace_summary(pool: &SqlitePool) -> Result<Value> {
    let modules = crate::db::discover_modules_sqlite(pool).await?;
    let mut module_counts = Vec::new();
    let mut total_records = 0i64;
    for module in &modules {
        let count = crate::db::count_module(pool, module).await?;
        total_records += count;
        module_counts.push(json!({ "module": module, "records": count }));
    }
    Ok(json!({
        "kind": "ironcrm.workspace.summary",
        "version": env!("CARGO_PKG_VERSION"),
        "modules": modules,
        "module_counts": module_counts,
        "total_records": total_records
    }))
}

async fn modules_payload(pool: &SqlitePool) -> Result<Value> {
    let modules = crate::db::discover_modules_sqlite(pool).await?;
    let definitions = crate::module::get_all_definitions(pool).await?;
    Ok(json!({
        "modules": modules,
        "count": modules.len(),
        "definitions": definitions
    }))
}

async fn ensure_module(pool: &SqlitePool, module: &str) -> Result<()> {
    if crate::module::has_definition(pool, module).await? {
        Ok(())
    } else {
        Err(anyhow!("Unknown module '{module}'"))
    }
}

fn string_arg(args: &Map<String, Value>, key: &str) -> Result<String> {
    let value = args
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("Missing required argument '{key}'"))?;
    Ok(value.to_string())
}

fn object_arg(args: &Map<String, Value>, key: &str) -> Result<Map<String, Value>> {
    args.get(key)
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| anyhow!("Missing required object argument '{key}'"))
}

fn u32_arg(args: &Map<String, Value>, key: &str, default: u32, min: u32, max: u32) -> Result<u32> {
    let Some(value) = args.get(key) else {
        return Ok(default);
    };
    let number = value
        .as_u64()
        .ok_or_else(|| anyhow!("Argument '{key}' must be an integer"))?;
    Ok((number as u32).clamp(min, max))
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn error_response(id: Value, code: i64, message: &str, data: Option<String>) -> Value {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = Value::String(data);
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

fn tool_error_response(id: Value, message: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{ "type": "text", "text": message }],
            "isError": true
        }
    })
}
