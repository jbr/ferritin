//! Version conversions for rustdoc-types formats
//!
//! Each module (e.g., `v56`) handles conversion from that version to the next (v56 -> v57).
//! Conversions can be chained: v55 -> v56 -> v57

pub mod v55;
pub mod v56;

use anyhow::{Context, Result};
use rustdoc_types::{Crate, FORMAT_VERSION};
use sonic_rs::JsonValueTrait;

/// Load rustdoc JSON and normalize to the current format version
///
/// This function:
/// 1. Parses the JSON to determine the format version
/// 2. Parses with the appropriate rustdoc-types version
/// 3. Converts through intermediate versions to reach FORMAT_VERSION (57)
pub fn load_and_normalize(json: &[u8], format_version: Option<u32>) -> Result<Crate> {
    // First, peek at the format version without parsing the entire JSON
    let format_version = if let Some(format_version) = format_version {
        format_version
    } else {
        let format_version_value = sonic_rs::get_from_slice(json, &["format_version"])
            .context("Failed to extract format_version")?;

        format_version_value
            .as_u64()
            .context("format_version is not a valid u64")? as u32
    };

    match format_version {
        FORMAT_VERSION => {
            // Already current version, parse directly
            sonic_rs::serde::from_slice(json).context("Failed to parse as current format")
        }
        56 => {
            // Parse as v56, convert to v57
            let crate_56: rustdoc_types_56::Crate = sonic_rs::serde::from_slice(json)
                .context("Failed to parse as format version 56")?;
            v56::convert_crate(crate_56)
        }
        55 => {
            // Parse as v55, convert to v56, then to v57
            let crate_55: rustdoc_types_55::Crate = sonic_rs::serde::from_slice(json)
                .context("Failed to parse as format version 55")?;
            let crate_56 = v55::convert_crate(crate_55).context("Failed to convert v55 to v56")?;
            v56::convert_crate(crate_56)
        }
        v if v < 55 => {
            anyhow::bail!(
                "Format version {} is too old. Minimum supported version: 55, current version: {}",
                v,
                FORMAT_VERSION
            )
        }
        v => {
            anyhow::bail!(
                "Format version {} is too new. Maximum supported version: {}",
                v,
                FORMAT_VERSION
            )
        }
    }
}
