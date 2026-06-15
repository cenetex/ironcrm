pub mod account;
pub mod case;
pub mod contact;
pub mod lead;
pub mod opportunity;

pub use account::Account;
pub use case::Case;
pub use contact::Contact;
pub use lead::Lead;
pub use opportunity::Opportunity;

use crate::module::{FieldDefinition, ModuleDefinition, ScanMapping};

/// Structs that carry enough schema knowledge to produce a full ModuleDefinition.
/// Used to seed the module_definitions table at startup and to simplify import
/// pipelines that need to create typed tables from external schemas.
pub trait ModuleSchema {
    fn definition() -> ModuleDefinition;
}

/// Helpers for building definitions from Rust struct field knowledge.
pub(crate) fn field(name: &str, sql_type: &str, pos: u32) -> FieldDefinition {
    FieldDefinition {
        name: name.into(),
        field_type: sql_type.into(),
        position: pos,
        label: None,
        scan_source: None,
        default_value: None,
    }
}

pub(crate) fn field_default(
    name: &str,
    sql_type: &str,
    pos: u32,
    default: &str,
) -> FieldDefinition {
    FieldDefinition {
        name: name.into(),
        field_type: sql_type.into(),
        position: pos,
        label: None,
        scan_source: None,
        default_value: Some(default.into()),
    }
}

pub(crate) fn field_scan(
    name: &str,
    sql_type: &str,
    pos: u32,
    scan_source: &str,
) -> FieldDefinition {
    FieldDefinition {
        name: name.into(),
        field_type: sql_type.into(),
        position: pos,
        label: None,
        scan_source: Some(scan_source.into()),
        default_value: None,
    }
}

pub(crate) fn scan(scan: &str, col: &str) -> (String, String) {
    (scan.into(), col.into())
}
