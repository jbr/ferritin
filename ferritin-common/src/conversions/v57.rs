//! Conversion from rustdoc-types format version 57 to 58
//!
//! Changes in v58:
//! - Added `Item::stability` field (`Option<Box<Stability>>`)
//!
//! Strategy: Parse as v57, serialize to JSON, deserialize as v58.
//! Since the new field is an `Option` that serde defaults to `None` when
//! absent, no JSON patching is needed beyond bumping `format_version`.

use anyhow::{Context, Result};
use rustdoc_types as v58;
use rustdoc_types_57 as v57;
use sonic_rs::JsonValueMutTrait;

/// Convert a v57 Crate to v58
///
/// This works by round-tripping through sonic_rs::Value. The only v58 addition
/// is `Item::stability`, an `Option` field that deserializes to `None` when the
/// v57 JSON omits it, so we only need to update the format_version field.
pub fn convert_crate(crate_57: v57::Crate) -> Result<v58::Crate> {
    let mut json_value =
        sonic_rs::value::to_value(&crate_57).context("Failed to serialize v57 crate to JSON")?;

    // Update format_version in JSON before deserializing
    if let Some(obj) = json_value.as_object_mut() {
        obj.insert("format_version", sonic_rs::json!(58));
    }

    let crate_58: v58::Crate =
        sonic_rs::value::from_value(&json_value).context("Failed to deserialize as v58 crate")?;

    Ok(crate_58)
}
