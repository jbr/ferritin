//! Conversion from rustdoc-types format version 56 to 57
//!
//! Changes in v57:
//! - Added `ExternalCrate::path` field (PathBuf) - defaults to empty
//!
//! Strategy: Parse as v56, serialize to JSON, deserialize as v57 with defaults

use anyhow::{Context, Result};
use rustdoc_types as v57;
use rustdoc_types_56 as v56;

/// Convert a v56 Crate to v57
///
/// This works by round-tripping through serde_json::Value:
/// 1. We already have parsed v56::Crate
/// 2. Serialize it to serde_json::Value
/// 3. Patch the JSON to add v57-specific fields with defaults
/// 4. Deserialize as v57::Crate
pub fn convert_crate(crate_56: v56::Crate) -> Result<v57::Crate> {
    let mut json_value =
        serde_json::to_value(&crate_56).context("Failed to serialize v56 crate to JSON")?;

    // Patch: Add `path` field to all ExternalCrate entries (defaults to empty PathBuf)
    if let Some(external_crates) = json_value.get_mut("external_crates") {
        if let Some(map) = external_crates.as_object_mut() {
            for (_id, ext_crate) in map.iter_mut() {
                if let Some(obj) = ext_crate.as_object_mut() {
                    obj.insert("path".to_string(), serde_json::json!(""));
                }
            }
        }
    }

    // Update format_version in JSON before deserializing
    if let Some(obj) = json_value.as_object_mut() {
        obj.insert("format_version".to_string(), serde_json::json!(57));
    }

    let crate_57: v57::Crate =
        serde_json::from_value(json_value).context("Failed to deserialize as v57 crate")?;

    Ok(crate_57)
}
