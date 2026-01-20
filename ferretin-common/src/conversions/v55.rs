//! Conversion from rustdoc-types format version 55 to 56
//!
//! Changes in v56:
//! - Added `ItemKind::Attribute` enum variant
//!
//! Strategy: Parse as v55, serialize to JSON, deserialize as v56
//! Since this is just an enum addition, no JSON patching is needed.

use anyhow::{Context, Result};
use rustdoc_types_55 as v55;
use rustdoc_types_56 as v56;

/// Convert a v55 Crate to v56
///
/// This works by round-tripping through serde_json::Value.
/// Since v56 only adds a new enum variant that won't exist in v55 data,
/// we only need to update the format_version field.
pub fn convert_crate(crate_55: v55::Crate) -> Result<v56::Crate> {
    let mut json_value =
        serde_json::to_value(&crate_55).context("Failed to serialize v55 crate to JSON")?;

    // Update format_version in JSON before deserializing
    if let Some(obj) = json_value.as_object_mut() {
        obj.insert("format_version".to_string(), serde_json::json!(56));
    }

    let crate_56: v56::Crate =
        serde_json::from_value(json_value).context("Failed to deserialize as v56 crate")?;

    Ok(crate_56)
}
