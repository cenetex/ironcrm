use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScanResult {
    pub name: Option<String>,
    pub company: Option<String>,
    pub title: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub website: Option<String>,
    pub address: Option<String>,
    pub suggested_module: String,
    pub raw_text: Option<String>,
}

/// Scan an image file using llama.cpp with a vision model.
/// Runs inference in a blocking thread to keep the async runtime responsive.
pub async fn scan_image(image_data: &[u8]) -> Result<ScanResult> {
    let mut tmp = tempfile::NamedTempFile::new().context("failed to create temp file")?;
    tmp.write_all(image_data)
        .context("failed to write image to temp file")?;
    let path = tmp.path().to_string_lossy().to_string();

    // Offload blocking llama.cpp call to a dedicated thread
    let result = tokio::task::spawn_blocking(move || try_llama_vision(&path)).await;

    match result {
        Ok(Ok(scan)) => return Ok(scan),
        Ok(Err(e)) => tracing::warn!("llama.cpp scan error: {}", e),
        Err(join_err) => tracing::warn!("llama.cpp thread panic: {}", join_err),
    }

    tracing::warn!("llama.cpp not available, using fallback scan");
    Ok(ScanResult {
        suggested_module: "contacts".into(),
        raw_text: Some(format!("Image saved: {} bytes", image_data.len())),
        ..Default::default()
    })
}

fn try_llama_vision(image_path: &str) -> Result<ScanResult> {
    let prompt = r#"Analyze this image. If it's a business card, receipt, or document, extract the following information as JSON. Return ONLY valid JSON, no other text.

{
  "name": "person or business name",
  "company": "company name if present",
  "title": "job title if present",
  "phone": "phone number if present",
  "email": "email if present",
  "website": "website if present",
  "address": "address if present",
  "suggested_module": "contacts if person, accounts if business, opportunities if deal-related"
}"#;

    let model_path = "models/llava-v1.6-mistral-7b.Q4_K_M.gguf";
    let mmproj_path = "models/mmproj-llava-v1.6-mistral-7b-f16.gguf";
    let has_mmproj = std::fs::metadata(mmproj_path).is_ok();

    let mut cmd = std::process::Command::new("llama-cli");
    cmd.arg("-m")
        .arg(model_path)
        .arg("--image")
        .arg(image_path)
        .arg("-p")
        .arg(prompt)
        .arg("--temp")
        .arg("0.1")
        .arg("-n")
        .arg("512")
        .arg("--no-display-prompt");

    if has_mmproj {
        cmd.arg("--mmproj").arg(mmproj_path);
    }

    let output = cmd.output().context("failed to run llama-cli")?;

    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout);
        let json_text = extract_json_block(&text);

        if let Ok(result) = serde_json::from_str::<ScanResult>(&json_text) {
            return Ok(result);
        }

        return Ok(ScanResult {
            suggested_module: "contacts".into(),
            raw_text: Some(json_text),
            ..Default::default()
        });
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "llama-cli exited non-zero. Is the model at {}?\n{}",
        model_path,
        stderr
    )
}

fn extract_json_block(text: &str) -> String {
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return text[start..=end].to_string();
        }
    }
    text.to_string()
}

/// Insert a scanned record into the database (typed table + blob store).
pub async fn import_scan(
    pool: &sqlx::sqlite::SqlitePool,
    module: &str,
    data: &ScanResult,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();

    let mut fields = serde_json::Map::new();
    fields.insert("id".into(), serde_json::Value::String(id.clone()));

    // Use module definition's scan mapping, falling back to schema-aware defaults
    let def = crate::module::get_definition(pool, module).await?;
    let mapping = def.as_ref().and_then(|d| d.scan_mapping.as_ref());

    if let Some(ref name) = data.name {
        if mapping.map_or(false, |m| m.split_name) {
            let parts: Vec<&str> = name.splitn(2, ' ').collect();
            fields.insert(
                "first_name".into(),
                serde_json::Value::String(parts[0].to_string()),
            );
            if parts.len() > 1 {
                fields.insert(
                    "last_name".into(),
                    serde_json::Value::String(parts[1].to_string()),
                );
            }
        } else {
            fields.insert("name".into(), serde_json::Value::String(name.clone()));
        }
    }

    // Apply scan_source → column_name from the definition mapping
    let field_map = |scan_key: &str| -> Option<&str> {
        mapping
            .and_then(|m| m.sources.get(scan_key))
            .map(|s| s.as_str())
    };

    let mut insert_if = |scan_key: &str, scan_val: Option<&String>, default_col: &str| {
        if let Some(val) = scan_val {
            let col = field_map(scan_key).unwrap_or(default_col);
            fields.insert(col.to_string(), serde_json::Value::String(val.clone()));
        }
    };

    insert_if("title", data.title.as_ref(), "title");
    insert_if("phone", data.phone.as_ref(), "phone_work");
    insert_if("email", data.email.as_ref(), "email1");
    insert_if("website", data.website.as_ref(), "website");
    insert_if("company", data.company.as_ref(), "account_name");

    let json = serde_json::to_string(&fields)?;

    // Blob store
    sqlx::query("INSERT OR REPLACE INTO records (module, id, data) VALUES (?, ?, ?)")
        .bind(module)
        .bind(&id)
        .bind(&json)
        .execute(pool)
        .await?;

    // Typed table
    if crate::module::has_definition(pool, module).await? {
        crate::db::upsert_typed_row(pool, module, &id, &fields).await?;
    }

    Ok(id)
}
