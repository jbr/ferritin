//! Format-version normalization for rustdoc JSON.
//!
//! Every rustdoc JSON format bump in the range we support has been
//! *read-compatible*: each one adds a field or an enum variant without
//! removing, renaming, or retyping anything we read. Such changes need no
//! per-version `rustdoc-types` crate — older JSON deserializes straight into
//! the canonical types, because an added `Option` field defaults to `None` when
//! absent and an added enum variant simply never appears in older data.
//!
//! The sole exception across formats 55..=60 is [`ExternalCrate::path`], a
//! required `PathBuf` added in format 57. Pre-57 JSON omits it, so we inject an
//! empty value before parsing. That single JSON-level patch is the entire
//! normalization story today.
//!
//! **If a future format makes a genuinely read-breaking change** (removing,
//! renaming, or retyping a field we read), the additive shortcut no longer
//! works for that hop: pull in a pinned `rustdoc-types` crate for the older
//! format, parse with it, and translate to the canonical types here — the
//! approach this module used before the formats turned out to be uniformly
//! additive (see git history for the chained-conversion pattern).

use anyhow::{Context, Result};
use rustdoc_types::{Crate, FORMAT_VERSION};
use sonic_rs::{JsonValueMutTrait, JsonValueTrait};

/// Oldest rustdoc JSON format version we can normalize.
const MIN_FORMAT_VERSION: u32 = 55;

/// The format version at which [`ExternalCrate::path`] became a required field.
/// JSON older than this must have the field injected before it will parse with
/// the canonical types.
const EXTERNAL_CRATE_PATH_VERSION: u32 = 57;

/// Load rustdoc JSON and normalize it to the canonical [`FORMAT_VERSION`].
///
/// The format version is taken from the `format_version` argument when known
/// (the docs.rs cache records it), otherwise peeked from the JSON itself.
pub fn load_and_normalize(json: &[u8], format_version: Option<u32>) -> Result<Crate> {
    let format_version = match format_version {
        Some(v) => v,
        None => peek_format_version(json)?,
    };

    if format_version < MIN_FORMAT_VERSION {
        anyhow::bail!(
            "Format version {format_version} is too old. Minimum supported version: \
             {MIN_FORMAT_VERSION}, current version: {FORMAT_VERSION}"
        );
    }

    if format_version > FORMAT_VERSION {
        // Newer than the rustdoc-types we were built against. Format bumps are
        // usually additive and the types don't `deny_unknown_fields`, so a
        // direct parse normally succeeds (the extra fields are ignored). A
        // genuinely breaking change surfaces as a parse error.
        return sonic_rs::serde::from_slice(json).with_context(|| {
            format!(
                "Format version {format_version} is newer than supported ({FORMAT_VERSION}) and \
                 could not be parsed with the current rustdoc-types. ferritin needs to be updated \
                 to read this format."
            )
        });
    }

    if format_version >= EXTERNAL_CRATE_PATH_VERSION {
        // Format 57..=current: the only fields the canonical types require are
        // already present; anything newer than 57 added was optional. Parse
        // directly with no intermediate `Value`.
        return sonic_rs::serde::from_slice(json).with_context(|| {
            format!("Failed to parse rustdoc JSON (format version {format_version})")
        });
    }

    // Format 55..=56: inject the required `ExternalCrate::path` (added in 57),
    // then parse with the canonical types.
    let mut value: sonic_rs::Value =
        sonic_rs::from_slice(json).context("Failed to parse rustdoc JSON")?;
    add_external_crate_paths(&mut value);
    sonic_rs::value::from_value(&value).with_context(|| {
        format!("Failed to parse normalized rustdoc JSON (was format version {format_version})")
    })
}

/// Peek the `format_version` field without parsing the whole document.
fn peek_format_version(json: &[u8]) -> Result<u32> {
    sonic_rs::get_from_slice(json, &["format_version"])
        .context("Failed to extract format_version")?
        .as_u64()
        .context("format_version is not a valid u64")
        .map(|v| v as u32)
}

/// Add an empty `path` to every `external_crates` entry that lacks one, so
/// pre-57 JSON satisfies the required [`ExternalCrate::path`] field.
fn add_external_crate_paths(value: &mut sonic_rs::Value) {
    if let Some(external_crates) = value.get_mut("external_crates")
        && let Some(map) = external_crates.as_object_mut()
    {
        for (_id, ext_crate) in map.iter_mut() {
            if let Some(obj) = ext_crate.as_object_mut()
                && obj.get(&"path").is_none()
            {
                obj.insert("path", sonic_rs::json!(""));
            }
        }
    }
}
