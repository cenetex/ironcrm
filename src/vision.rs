use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::io::Write;
use tempfile::NamedTempFile;
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
/// Falls back to a simpler text extraction if llama.cpp isn't available.
pub async fn scan_image(image_data: &[u8]) -> Result<ScanResult> {
    // Save image to temp file
    let mut tmp = NamedTempFile::new()?;
    tmp.write_all(image_data)?;
    let path = tmp.path().to_string_lossy().to_string();

    // Try llama.cpp first
    if let Ok(result) = try_llama_vision(&path).await {
        return Ok(result);
    }

    // Fallback: basic file info
    tracing::warn!("llama.cpp not available, using fallback scan");
    Ok(ScanResult {
        suggested_module: "contacts".into(),
        raw_text: Some(format!("Image saved: {} bytes", image_data.len())),
        ..Default::default()
    })
}

async fn try_llama_vision(image_path: &str) -> Result<ScanResult> {
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

    // Try common llama.cpp binary names and model paths
    let candidates = [
        ("llama-cli", "models/llava-v1.6-mistral-7b.Q4_K_M.gguf"),
        ("llama-cli", "models/mmproj-llava-v1.6-mistral-7b-f16.gguf"),
    ];

    for (binary, model) in &candidates {
        let output = Command::new(binary)
            .args([
                "-m", model,
                "--image", image_path,
                "-p", prompt,
                "--temp", "0.1",
                "-n", "512",
                "--no-display-prompt",
            ])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);
                let text = extract_json_block(&text);

                if let Ok(result) = serde_json::from_str::<ScanResult>(&text) {
                    return Ok(result);
                }

                // Try to extract fields from raw text
                return Ok(ScanResult {
                    suggested_module: "contacts".into(),
                    raw_text: Some(text),
                    ..Default::default()
                });
            }
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr);
                tracing::debug!("llama.cpp error: {}", err);
            }
            Err(_) => continue,
        }
    }

    anyhow::bail!("llama.cpp not found or model not available. Install llama.cpp and download a vision model (e.g. llava-v1.6-mistral-7b) to models/")
}

fn extract_json_block(text: &str) -> String {
    // Find the first { and last }
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return text[start..=end].to_string();
        }
    }
    text.to_string()
}

/// Insert a scanned record into the database
pub async fn import_scan(pool: &sqlx::sqlite::SqlitePool, module: &str, data: &ScanResult) -> Result<String> {
    let id = Uuid::new_v4().to_string();

    let mut fields = serde_json::Map::new();
    fields.insert("id".into(), serde_json::Value::String(id.clone()));

    if let Some(ref name) = data.name {
        if module == "contacts" || module == "leads" {
            // Split into first/last name
            let parts: Vec<&str> = name.splitn(2, ' ').collect();
            fields.insert("first_name".into(), serde_json::Value::String(parts[0].to_string()));
            if parts.len() > 1 {
                fields.insert("last_name".into(), serde_json::Value::String(parts[1].to_string()));
            }
        } else {
            fields.insert("name".into(), serde_json::Value::String(name.clone()));
        }
    }
    if let Some(ref c) = data.company { fields.insert("account_name".into(), serde_json::Value::String(c.clone())); }
    if let Some(ref t) = data.title { fields.insert("title".into(), serde_json::Value::String(t.clone())); }
    if let Some(ref p) = data.phone { fields.insert("phone_work".into(), serde_json::Value::String(p.clone())); }
    if let Some(ref e) = data.email { fields.insert("email1".into(), serde_json::Value::String(e.clone())); }
    if let Some(ref w) = data.website { fields.insert("website".into(), serde_json::Value::String(w.clone())); }

    let json = serde_json::to_string(&fields)?;
    sqlx::query("INSERT OR REPLACE INTO records (module, id, data) VALUES (?, ?, ?)")
        .bind(module)
        .bind(&id)
        .bind(&json)
        .execute(pool)
        .await?;

    Ok(id)
}
