use anyhow::{Context, Result, anyhow};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use sqlx::sqlite::SqlitePool;
use sqlx::{Column, Row};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;

use crate::module::ModuleDefinition;

const BUNDLE_KIND: &str = "ironcrm.workspace";
const BUNDLE_VERSION: u32 = 1;
const ARCHIVE_CONTENT_TYPE: &str = "application/vnd.ironcrm.workspace+zip";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceBundle {
    pub kind: String,
    pub version: u32,
    pub exported_at: String,
    pub manifest: BundleManifest,
    #[serde(default)]
    pub schemas: BundleSchemas,
    #[serde(default)]
    pub records: Vec<BundleRecord>,
    #[serde(default)]
    pub activities: Vec<Value>,
    #[serde(default)]
    pub record_links: Vec<Value>,
    #[serde(default)]
    pub consent_receipts: Vec<Value>,
    #[serde(default)]
    pub sources: Vec<SourceMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleManifest {
    pub format: String,
    pub format_version: u32,
    pub app_version: String,
    pub generated_at: String,
    pub counts: BundleCounts,
    pub modules: Vec<BundleModuleCount>,
    pub feature_flags: Vec<String>,
    pub hashes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BundleCounts {
    pub modules: u64,
    pub records: u64,
    pub activities: u64,
    pub record_links: u64,
    pub consent_receipts: u64,
    pub sources: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleModuleCount {
    pub module: String,
    pub label: String,
    pub records: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BundleSchemas {
    #[serde(default)]
    pub modules: Vec<ModuleDefinition>,
    #[serde(default)]
    pub custom_field_definitions: Vec<Value>,
    #[serde(default)]
    pub stage_definitions: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleRecord {
    pub module: String,
    pub data: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceMetadata {
    pub module: String,
    pub record_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported_at: Option<String>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub custom_fields: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub source_fields: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleArchiveSummary {
    pub kind: String,
    pub version: u32,
    pub app_version: String,
    pub generated_at: String,
    pub modules: u64,
    pub records: u64,
    pub activities: u64,
    pub record_links: u64,
    pub consent_receipts: u64,
    pub sources: u64,
}

pub fn archive_content_type() -> &'static str {
    ARCHIVE_CONTENT_TYPE
}

pub async fn export_workspace(pool: &SqlitePool) -> Result<Value> {
    let bundle = build_bundle(pool).await?;
    Ok(serde_json::to_value(bundle)?)
}

pub async fn export_workspace_archive(pool: &SqlitePool) -> Result<Vec<u8>> {
    let bundle = build_bundle(pool).await?;
    bundle_to_archive(&bundle)
}

pub async fn restore_workspace(pool: &SqlitePool, bundle: &Value) -> Result<Map<String, Value>> {
    let bundle = parse_and_validate_bundle(pool, bundle, false).await?;
    restore_bundle(pool, &bundle).await
}

pub async fn restore_workspace_archive(
    pool: &SqlitePool,
    bytes: &[u8],
    exact: bool,
) -> Result<Map<String, Value>> {
    let value = archive_to_bundle_value(bytes)?;
    if exact {
        restore_workspace_exact(pool, &value).await
    } else {
        restore_workspace(pool, &value).await
    }
}

pub async fn restore_workspace_exact(
    pool: &SqlitePool,
    bundle: &Value,
) -> Result<Map<String, Value>> {
    let bundle = parse_and_validate_bundle(pool, bundle, true).await?;
    clear_workspace_data(pool, &bundle).await?;
    restore_bundle(pool, &bundle).await
}

pub fn bundle_to_archive(bundle: &WorkspaceBundle) -> Result<Vec<u8>> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    write_archive_json(&mut writer, "manifest.json", &bundle.manifest, options)?;
    write_archive_json(
        &mut writer,
        "schemas/modules.json",
        &bundle.schemas.modules,
        options,
    )?;
    write_archive_json(
        &mut writer,
        "schemas/custom_fields.json",
        &bundle.schemas.custom_field_definitions,
        options,
    )?;
    write_archive_json(
        &mut writer,
        "schemas/stages.json",
        &bundle.schemas.stage_definitions,
        options,
    )?;
    write_archive_json(
        &mut writer,
        "records/records.json",
        &bundle.records,
        options,
    )?;
    write_archive_json(
        &mut writer,
        "relationships/record_links.json",
        &bundle.record_links,
        options,
    )?;
    write_archive_json(
        &mut writer,
        "activities/activity_log.json",
        &bundle.activities,
        options,
    )?;
    write_archive_json(
        &mut writer,
        "consent/consent_receipts.json",
        &bundle.consent_receipts,
        options,
    )?;
    write_archive_json(
        &mut writer,
        "sources/source_metadata.json",
        &bundle.sources,
        options,
    )?;

    Ok(writer.finish()?.into_inner())
}

pub fn archive_to_bundle_value(bytes: &[u8]) -> Result<Value> {
    let bundle = archive_to_bundle(bytes)?;
    Ok(serde_json::to_value(bundle)?)
}

pub fn archive_to_bundle(bytes: &[u8]) -> Result<WorkspaceBundle> {
    let cursor = Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).context("could not open White Warren archive")?;

    let manifest: BundleManifest = read_archive_json(&mut archive, "manifest.json")?;
    let modules: Vec<ModuleDefinition> = read_archive_json(&mut archive, "schemas/modules.json")?;
    let custom_field_definitions: Vec<Value> =
        read_archive_json_optional(&mut archive, "schemas/custom_fields.json")?.unwrap_or_default();
    let stage_definitions: Vec<Value> =
        read_archive_json_optional(&mut archive, "schemas/stages.json")?.unwrap_or_default();
    let records: Vec<BundleRecord> = read_archive_json(&mut archive, "records/records.json")?;
    let record_links: Vec<Value> =
        read_archive_json_optional(&mut archive, "relationships/record_links.json")?
            .unwrap_or_default();
    let activities: Vec<Value> =
        read_archive_json_optional(&mut archive, "activities/activity_log.json")?
            .unwrap_or_default();
    let consent_receipts: Vec<Value> =
        read_archive_json_optional(&mut archive, "consent/consent_receipts.json")?
            .unwrap_or_default();
    let sources: Vec<SourceMetadata> =
        read_archive_json_optional(&mut archive, "sources/source_metadata.json")?
            .unwrap_or_default();

    Ok(WorkspaceBundle {
        kind: manifest.format.clone(),
        version: manifest.format_version,
        exported_at: manifest.generated_at.clone(),
        manifest,
        schemas: BundleSchemas {
            modules,
            custom_field_definitions,
            stage_definitions,
        },
        records,
        activities,
        record_links,
        consent_receipts,
        sources,
    })
}

pub fn validate_archive_bytes(bytes: &[u8]) -> Result<BundleArchiveSummary> {
    let value = archive_to_bundle_value(bytes)?;
    validate_bundle_value(&value, true).map(|bundle| archive_summary(&bundle))
}

pub fn validate_bundle_json(value: &Value) -> Result<BundleArchiveSummary> {
    validate_bundle_value(value, true).map(|bundle| archive_summary(&bundle))
}

fn archive_summary(bundle: &WorkspaceBundle) -> BundleArchiveSummary {
    BundleArchiveSummary {
        kind: bundle.kind.clone(),
        version: bundle.version,
        app_version: bundle.manifest.app_version.clone(),
        generated_at: bundle.manifest.generated_at.clone(),
        modules: bundle.manifest.counts.modules,
        records: bundle.manifest.counts.records,
        activities: bundle.manifest.counts.activities,
        record_links: bundle.manifest.counts.record_links,
        consent_receipts: bundle.manifest.counts.consent_receipts,
        sources: bundle.manifest.counts.sources,
    }
}

fn write_archive_json<T: Serialize, W: Write + std::io::Seek>(
    writer: &mut zip::ZipWriter<W>,
    name: &str,
    value: &T,
    options: SimpleFileOptions,
) -> Result<()> {
    writer.start_file(name, options)?;
    let json = serde_json::to_vec_pretty(value)?;
    writer.write_all(&json)?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn read_archive_json<T: DeserializeOwned>(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<T> {
    read_archive_json_optional(archive, name)?
        .ok_or_else(|| anyhow!("White Warren archive is missing {name}"))
}

fn read_archive_json_optional<T: DeserializeOwned>(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Option<T>> {
    let mut file = match archive.by_name(name) {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(Some(serde_json::from_str(&contents).with_context(
        || format!("could not parse archive entry {name}"),
    )?))
}

async fn build_bundle(pool: &SqlitePool) -> Result<WorkspaceBundle> {
    let definitions = crate::module::get_all_definitions(pool).await?;
    let definition_map: HashMap<String, ModuleDefinition> = definitions
        .iter()
        .map(|definition| (definition.module.clone(), definition.clone()))
        .collect();
    let exported_at = now_timestamp();

    let records = export_records(pool, &definitions).await?;
    let record_ids = exported_record_ids(&records);
    let record_links = export_record_links(pool, &record_ids).await?;
    let activities = fetch_rows(
        pool,
        "activity_log",
        &[
            "id",
            "module",
            "record_id",
            "operation",
            "actor_id",
            "changed_fields",
            "created_at",
        ],
        "created_at ASC, id ASC",
    )
    .await?;
    let consent_receipts = fetch_rows(
        pool,
        "consent_receipts",
        &["contact_id", "credential_json", "issued_at", "expires_at"],
        "contact_id ASC",
    )
    .await?;
    let sources = derive_source_metadata(&records, &definition_map);
    let schemas = BundleSchemas {
        modules: definitions.clone(),
        custom_field_definitions: derive_custom_field_definitions(&sources),
        stage_definitions: derive_stage_definitions(&definitions),
    };
    let module_counts = derive_module_counts(&definitions, &records);
    let counts = BundleCounts {
        modules: module_counts.len() as u64,
        records: records.len() as u64,
        activities: activities.len() as u64,
        record_links: record_links.len() as u64,
        consent_receipts: consent_receipts.len() as u64,
        sources: sources.len() as u64,
    };

    let mut hashes = BTreeMap::new();
    insert_section_hash(&mut hashes, "schemas", &schemas)?;
    insert_section_hash(&mut hashes, "records", &records)?;
    insert_section_hash(&mut hashes, "activities", &activities)?;
    insert_section_hash(&mut hashes, "record_links", &record_links)?;
    insert_section_hash(&mut hashes, "consent_receipts", &consent_receipts)?;
    insert_section_hash(&mut hashes, "sources", &sources)?;

    Ok(WorkspaceBundle {
        kind: BUNDLE_KIND.into(),
        version: BUNDLE_VERSION,
        exported_at: exported_at.clone(),
        manifest: BundleManifest {
            format: BUNDLE_KIND.into(),
            format_version: BUNDLE_VERSION,
            app_version: env!("CARGO_PKG_VERSION").into(),
            generated_at: exported_at,
            counts,
            modules: module_counts,
            feature_flags: vec![
                "schemas".into(),
                "activity_log".into(),
                "record_links".into(),
                "consent_receipts".into(),
                "source_metadata".into(),
                "section_hashes".into(),
            ],
            hashes,
        },
        schemas,
        records,
        activities,
        record_links,
        consent_receipts,
        sources,
    })
}

async fn export_records(
    pool: &SqlitePool,
    definitions: &[ModuleDefinition],
) -> Result<Vec<BundleRecord>> {
    let mut modules: BTreeSet<String> = crate::db::discover_modules_sqlite(pool)
        .await?
        .into_iter()
        .collect();
    for definition in definitions {
        modules.insert(definition.module.clone());
    }

    let mut records = Vec::new();
    let mut seen = HashSet::new();

    for module in modules {
        let blob_rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, data
             FROM records
             WHERE module = ?1
               AND IFNULL(json_extract(data, '$.deleted'), '0') != '1'
             ORDER BY id",
        )
        .bind(&module)
        .fetch_all(pool)
        .await?;

        for (id, raw) in blob_rows {
            let Ok(data) = serde_json::from_str::<Map<String, Value>>(&raw) else {
                continue;
            };
            seen.insert((module.clone(), id));
            records.push(BundleRecord {
                module: module.clone(),
                data,
            });
        }

        if !crate::module::has_definition(pool, &module).await? {
            continue;
        }

        let Some(identifier) = sqlite_identifier(&module) else {
            continue;
        };
        if !sqlite_table_exists(pool, &module).await? {
            continue;
        }

        let sql = format!("SELECT * FROM {identifier} WHERE deleted = 0 ORDER BY id");
        let rows = sqlx::query(&sql).fetch_all(pool).await?;
        for row in rows {
            let data = sqlite_row_to_map(&row);
            let Some(id) = data.get("id").and_then(Value::as_str).map(str::to_string) else {
                continue;
            };
            if seen.insert((module.clone(), id)) {
                records.push(BundleRecord {
                    module: module.clone(),
                    data,
                });
            }
        }
    }

    Ok(records)
}

async fn export_record_links(
    pool: &SqlitePool,
    record_ids: &HashSet<(String, String)>,
) -> Result<Vec<Value>> {
    let rows = fetch_rows(
        pool,
        "record_links",
        &[
            "id",
            "source_module",
            "source_id",
            "target_module",
            "target_id",
            "relationship_type",
            "created_at",
        ],
        "created_at ASC, id ASC",
    )
    .await?;

    Ok(rows
        .into_iter()
        .filter(|row| {
            let Some(object) = row.as_object() else {
                return false;
            };
            let source = (
                value_string(object.get("source_module")),
                value_string(object.get("source_id")),
            );
            let target = (
                value_string(object.get("target_module")),
                value_string(object.get("target_id")),
            );
            record_ids.contains(&source) && record_ids.contains(&target)
        })
        .collect())
}

fn exported_record_ids(records: &[BundleRecord]) -> HashSet<(String, String)> {
    records
        .iter()
        .filter_map(|record| {
            record
                .data
                .get("id")
                .and_then(Value::as_str)
                .map(|id| (record.module.clone(), id.to_string()))
        })
        .collect()
}

fn derive_source_metadata(
    records: &[BundleRecord],
    definitions: &HashMap<String, ModuleDefinition>,
) -> Vec<SourceMetadata> {
    let mut sources = Vec::new();

    for record in records {
        let Some(record_id) = record.data.get("id").and_then(Value::as_str) else {
            continue;
        };

        let valid_fields: HashSet<&str> = definitions
            .get(&record.module)
            .map(|definition| {
                definition
                    .fields
                    .iter()
                    .map(|field| field.name.as_str())
                    .collect()
            })
            .unwrap_or_default();

        let mut custom_fields = Map::new();
        let mut source_fields = Map::new();

        for (key, value) in &record.data {
            if key == "id" {
                continue;
            }
            if key.ends_with("_c") {
                custom_fields.insert(key.clone(), value.clone());
            }
            if !valid_fields.is_empty()
                && !valid_fields.contains(key.as_str())
                && !key.starts_with("source_")
                && key != "imported_at"
            {
                source_fields.insert(key.clone(), value.clone());
            }
        }

        let metadata = SourceMetadata {
            module: record.module.clone(),
            record_id: record_id.to_string(),
            source_system: string_field(&record.data, "source_system"),
            source_module: string_field(&record.data, "source_module"),
            source_id: string_field(&record.data, "source_id"),
            imported_at: string_field(&record.data, "imported_at"),
            custom_fields,
            source_fields,
        };

        if metadata.source_system.is_some()
            || metadata.source_module.is_some()
            || metadata.source_id.is_some()
            || metadata.imported_at.is_some()
            || !metadata.custom_fields.is_empty()
            || !metadata.source_fields.is_empty()
        {
            sources.push(metadata);
        }
    }

    sources
}

fn derive_custom_field_definitions(sources: &[SourceMetadata]) -> Vec<Value> {
    let mut fields = BTreeMap::<(String, String), Value>::new();
    for source in sources {
        for (name, value) in source
            .custom_fields
            .iter()
            .chain(source.source_fields.iter())
        {
            fields
                .entry((source.module.clone(), name.clone()))
                .or_insert_with(|| {
                    json!({
                        "module": source.module,
                        "name": name,
                        "type": inferred_json_type(value),
                        "source": "record_overflow"
                    })
                });
        }
    }
    fields.into_values().collect()
}

fn derive_stage_definitions(definitions: &[ModuleDefinition]) -> Vec<Value> {
    definitions
        .iter()
        .filter_map(|definition| {
            Some(json!({
                "module": definition.module,
                "field": definition.board_field.as_ref()?,
                "stages": definition.board_stages.clone().unwrap_or_default()
            }))
        })
        .collect()
}

fn derive_module_counts(
    definitions: &[ModuleDefinition],
    records: &[BundleRecord],
) -> Vec<BundleModuleCount> {
    let mut counts = BTreeMap::<String, u64>::new();
    for record in records {
        *counts.entry(record.module.clone()).or_default() += 1;
    }

    let labels: HashMap<&str, &str> = definitions
        .iter()
        .map(|definition| (definition.module.as_str(), definition.label.as_str()))
        .collect();

    let mut module_counts: Vec<BundleModuleCount> = definitions
        .iter()
        .map(|definition| BundleModuleCount {
            module: definition.module.clone(),
            label: definition.label.clone(),
            records: *counts.get(&definition.module).unwrap_or(&0),
        })
        .collect();

    for (module, records) in counts {
        if !labels.contains_key(module.as_str()) {
            module_counts.push(BundleModuleCount {
                label: titleize(&module),
                module,
                records,
            });
        }
    }

    module_counts
}

async fn parse_and_validate_bundle(
    pool: &SqlitePool,
    value: &Value,
    exact: bool,
) -> Result<WorkspaceBundle> {
    let bundle = validate_bundle_value(value, false)?;
    validate_links(pool, &bundle, exact).await?;
    Ok(bundle)
}

fn validate_bundle_value(value: &Value, self_contained: bool) -> Result<WorkspaceBundle> {
    let bundle: WorkspaceBundle = serde_json::from_value(value.clone())
        .map_err(|error| invalid_bundle(format!("could not parse bundle: {error}")))?;

    if bundle.kind != BUNDLE_KIND {
        return Err(anyhow!("Not a White Warren workspace bundle"));
    }
    if bundle.version != BUNDLE_VERSION || bundle.manifest.format_version != BUNDLE_VERSION {
        return Err(anyhow!(
            "Unsupported White Warren bundle version {}",
            bundle.version
        ));
    }
    if bundle.manifest.format != BUNDLE_KIND {
        return Err(invalid_bundle("manifest format does not match bundle kind"));
    }

    validate_hashes(&bundle)?;
    validate_counts(&bundle)?;
    validate_records(&bundle)?;
    if self_contained {
        validate_links_self_contained(&bundle)?;
    }

    Ok(bundle)
}

fn validate_hashes(bundle: &WorkspaceBundle) -> Result<()> {
    if bundle.manifest.hashes.is_empty() {
        return Err(invalid_bundle("manifest hashes are required"));
    }

    let mut expected = BTreeMap::new();
    insert_section_hash(&mut expected, "schemas", &bundle.schemas)?;
    insert_section_hash(&mut expected, "records", &bundle.records)?;
    insert_section_hash(&mut expected, "activities", &bundle.activities)?;
    insert_section_hash(&mut expected, "record_links", &bundle.record_links)?;
    insert_section_hash(&mut expected, "consent_receipts", &bundle.consent_receipts)?;
    insert_section_hash(&mut expected, "sources", &bundle.sources)?;

    for (section, hash) in expected {
        let Some(actual) = bundle.manifest.hashes.get(&section) else {
            return Err(invalid_bundle(format!(
                "missing hash for section {section}"
            )));
        };
        if actual != &hash {
            return Err(invalid_bundle(format!(
                "hash mismatch for section {section}"
            )));
        }
    }
    Ok(())
}

fn validate_counts(bundle: &WorkspaceBundle) -> Result<()> {
    let counts = &bundle.manifest.counts;
    let checks = [
        (
            "modules",
            counts.modules,
            bundle.manifest.modules.len() as u64,
        ),
        ("records", counts.records, bundle.records.len() as u64),
        (
            "activities",
            counts.activities,
            bundle.activities.len() as u64,
        ),
        (
            "record_links",
            counts.record_links,
            bundle.record_links.len() as u64,
        ),
        (
            "consent_receipts",
            counts.consent_receipts,
            bundle.consent_receipts.len() as u64,
        ),
        ("sources", counts.sources, bundle.sources.len() as u64),
    ];

    for (name, declared, actual) in checks {
        if declared != actual {
            return Err(invalid_bundle(format!(
                "manifest count for {name} is {declared}, expected {actual}"
            )));
        }
    }
    Ok(())
}

fn validate_records(bundle: &WorkspaceBundle) -> Result<()> {
    for record in &bundle.records {
        validate_sqlite_identifier("record module", &record.module)?;
        let Some(id) = record.data.get("id").and_then(Value::as_str) else {
            return Err(invalid_bundle(format!(
                "record in module {} is missing string id",
                record.module
            )));
        };
        if id.trim().is_empty() {
            return Err(invalid_bundle(format!(
                "record in module {} has an empty id",
                record.module
            )));
        }
    }

    for definition in &bundle.schemas.modules {
        validate_sqlite_identifier("module definition", &definition.module)?;
    }

    Ok(())
}

async fn validate_links(pool: &SqlitePool, bundle: &WorkspaceBundle, exact: bool) -> Result<()> {
    let records = exported_record_ids(&bundle.records);
    for link in &bundle.record_links {
        let (source_module, source_id, target_module, target_id) = validated_link_endpoints(link)?;

        if !record_available(pool, &records, &source_module, &source_id, exact).await? {
            return Err(invalid_bundle(format!(
                "dangling link source {source_module}/{source_id}"
            )));
        }
        if !record_available(pool, &records, &target_module, &target_id, exact).await? {
            return Err(invalid_bundle(format!(
                "dangling link target {target_module}/{target_id}"
            )));
        }
    }
    Ok(())
}

fn validate_links_self_contained(bundle: &WorkspaceBundle) -> Result<()> {
    let records = exported_record_ids(&bundle.records);
    for link in &bundle.record_links {
        let (source_module, source_id, target_module, target_id) = validated_link_endpoints(link)?;

        if !records.contains(&(source_module.clone(), source_id.clone())) {
            return Err(invalid_bundle(format!(
                "dangling link source {source_module}/{source_id}"
            )));
        }
        if !records.contains(&(target_module.clone(), target_id.clone())) {
            return Err(invalid_bundle(format!(
                "dangling link target {target_module}/{target_id}"
            )));
        }
    }
    Ok(())
}

fn validated_link_endpoints(link: &Value) -> Result<(String, String, String, String)> {
    let Some(link) = link.as_object() else {
        return Err(invalid_bundle("record link must be an object"));
    };
    let source_module = required_string(link, "source_module")?;
    let source_id = required_string(link, "source_id")?;
    let target_module = required_string(link, "target_module")?;
    let target_id = required_string(link, "target_id")?;
    validate_sqlite_identifier("link source module", &source_module)?;
    validate_sqlite_identifier("link target module", &target_module)?;
    Ok((source_module, source_id, target_module, target_id))
}

async fn record_available(
    pool: &SqlitePool,
    bundle_records: &HashSet<(String, String)>,
    module: &str,
    id: &str,
    exact: bool,
) -> Result<bool> {
    if bundle_records.contains(&(module.to_string(), id.to_string())) {
        return Ok(true);
    }
    if exact {
        return Ok(false);
    }
    Ok(crate::db::get_module_record(pool, module, id)
        .await?
        .is_some())
}

async fn restore_bundle(pool: &SqlitePool, bundle: &WorkspaceBundle) -> Result<Map<String, Value>> {
    let mut restored_modules = 0u64;
    let mut restored_records = 0u64;
    let mut restored_activities = 0u64;
    let mut restored_links = 0u64;
    let mut restored_consent_receipts = 0u64;

    for definition in &bundle.schemas.modules {
        let json = serde_json::to_string(definition)?;
        sqlx::query(
            "INSERT OR REPLACE INTO module_definitions
             (module, label, label_plural, def_json)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(&definition.module)
        .bind(&definition.label)
        .bind(&definition.label_plural)
        .bind(&json)
        .execute(pool)
        .await?;

        sqlx::query(&crate::module::gen_create_table_sql(definition))
            .execute(pool)
            .await?;
        for idx_sql in crate::module::gen_index_sqls(definition) {
            sqlx::query(&idx_sql).execute(pool).await?;
        }
        restored_modules += 1;
    }

    for record in &bundle.records {
        let Some(id) = record.data.get("id").and_then(Value::as_str) else {
            continue;
        };
        let json = serde_json::to_string(&record.data)?;
        sqlx::query("INSERT OR REPLACE INTO records (module, id, data) VALUES (?1, ?2, ?3)")
            .bind(&record.module)
            .bind(id)
            .bind(json)
            .execute(pool)
            .await?;
        if crate::module::has_definition(pool, &record.module).await? {
            crate::db::upsert_typed_row(pool, &record.module, id, &record.data).await?;
        }
        restored_records += 1;
    }

    for activity in &bundle.activities {
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
        .bind(optional_sql_string(activity.get("actor_id")))
        .bind(changed_fields)
        .bind(
            activity
                .get("created_at")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(now_timestamp),
        )
        .execute(pool)
        .await?;
        restored_activities += 1;
    }

    for link in &bundle.record_links {
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
                .unwrap_or_else(now_timestamp),
        )
        .execute(pool)
        .await?;
        restored_links += 1;
    }

    for receipt in &bundle.consent_receipts {
        let Some(receipt) = receipt.as_object() else {
            continue;
        };
        let contact_id = value_to_sql_string(receipt.get("contact_id"));
        if contact_id.is_empty() {
            continue;
        }
        sqlx::query(
            "INSERT OR REPLACE INTO consent_receipts
             (contact_id, credential_json, issued_at, expires_at)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(contact_id)
        .bind(value_to_sql_string(receipt.get("credential_json")))
        .bind(
            receipt
                .get("issued_at")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(now_timestamp),
        )
        .bind(optional_sql_string(receipt.get("expires_at")))
        .execute(pool)
        .await?;
        restored_consent_receipts += 1;
    }

    Ok(Map::from_iter([
        ("modules".into(), json!(restored_modules)),
        ("records".into(), json!(restored_records)),
        ("activities".into(), json!(restored_activities)),
        ("record_links".into(), json!(restored_links)),
        ("consent_receipts".into(), json!(restored_consent_receipts)),
        ("sources".into(), json!(bundle.sources.len())),
        ("warnings".into(), json!([] as [String; 0])),
    ]))
}

async fn clear_workspace_data(pool: &SqlitePool, bundle: &WorkspaceBundle) -> Result<()> {
    let mut modules = HashSet::new();
    for definition in crate::module::get_all_definitions(pool).await? {
        modules.insert(definition.module);
    }
    for definition in &bundle.schemas.modules {
        modules.insert(definition.module.clone());
    }
    for record in &bundle.records {
        modules.insert(record.module.clone());
    }

    sqlx::query("DELETE FROM record_links")
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM activity_log")
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM consent_receipts")
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM records").execute(pool).await?;

    for module in modules {
        let Some(identifier) = sqlite_identifier(&module) else {
            continue;
        };
        if sqlite_table_exists(pool, &module).await? {
            let sql = format!("DELETE FROM {identifier}");
            sqlx::query(&sql).execute(pool).await?;
        }
    }

    Ok(())
}

async fn fetch_rows(
    pool: &SqlitePool,
    table: &str,
    columns: &[&str],
    order_by: &str,
) -> Result<Vec<Value>> {
    validate_sqlite_identifier("table", table)?;
    for column in columns {
        validate_sqlite_identifier("column", column)?;
    }

    if !sqlite_table_exists(pool, table).await? {
        return Ok(Vec::new());
    }

    let table = sqlite_identifier(table).expect("validated table");
    let columns_sql = columns
        .iter()
        .map(|column| sqlite_identifier(column).expect("validated column"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("SELECT {columns_sql} FROM {table} ORDER BY {order_by}");
    let rows = sqlx::query(&sql).fetch_all(pool).await?;
    Ok(rows
        .iter()
        .map(|row| Value::Object(sqlite_row_to_map(row)))
        .collect())
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

fn validate_sqlite_identifier(label: &str, identifier: &str) -> Result<()> {
    if sqlite_identifier(identifier).is_none() {
        return Err(invalid_bundle(format!(
            "{label} '{identifier}' is not a safe SQLite identifier"
        )));
    }
    Ok(())
}

fn sqlite_identifier(identifier: &str) -> Option<String> {
    if identifier.is_empty()
        || !identifier
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return None;
    }
    Some(format!("\"{}\"", identifier))
}

async fn sqlite_table_exists(pool: &SqlitePool, table_name: &str) -> Result<bool> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1")
            .bind(table_name)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some())
}

fn insert_section_hash<T: Serialize>(
    hashes: &mut BTreeMap<String, String>,
    section: &str,
    value: &T,
) -> Result<()> {
    hashes.insert(section.into(), section_hash(value)?);
    Ok(())
}

fn section_hash<T: Serialize>(value: &T) -> Result<String> {
    let value = serde_json::to_value(value)?;
    let bytes = canonical_json(&value)?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn canonical_json(value: &Value) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    write_canonical_json(value, &mut out)?;
    Ok(out)
}

fn write_canonical_json(value: &Value, out: &mut Vec<u8>) -> Result<()> {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(number) => out.extend_from_slice(number.to_string().as_bytes()),
        Value::String(string) => out.extend_from_slice(serde_json::to_string(string)?.as_bytes()),
        Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_canonical_json(item, out)?;
            }
            out.push(b']');
        }
        Value::Object(object) => {
            out.push(b'{');
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                out.extend_from_slice(serde_json::to_string(key)?.as_bytes());
                out.push(b':');
                write_canonical_json(&object[key], out)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

fn required_string(object: &Map<String, Value>, key: &str) -> Result<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| invalid_bundle(format!("missing required string field {key}")))
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

fn optional_sql_string(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(value)) if !value.is_empty() => Some(value.clone()),
        Some(Value::Number(value)) => Some(value.to_string()),
        Some(Value::Bool(value)) => Some(value.to_string()),
        Some(value @ Value::Array(_)) | Some(value @ Value::Object(_)) => Some(value.to_string()),
        _ => None,
    }
}

fn value_string(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or("").to_string()
}

fn string_field(data: &Map<String, Value>, key: &str) -> Option<String> {
    data.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn inferred_json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn now_timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn titleize(value: &str) -> String {
    value
        .replace('_', " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn invalid_bundle(message: impl Into<String>) -> anyhow::Error {
    anyhow!("Invalid White Warren workspace bundle: {}", message.into())
}
