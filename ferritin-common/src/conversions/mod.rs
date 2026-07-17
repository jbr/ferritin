//! Format-version normalization for rustdoc JSON.
//!
//! Most rustdoc JSON format bumps are *read-compatible*: each adds a field or an
//! enum variant without removing, renaming, or retyping anything we read. Such
//! changes need no per-version `rustdoc-types` crate — older JSON deserializes
//! straight into the canonical types, because an added `Option` field defaults
//! to `None` when absent and an added enum variant simply never appears in older
//! data.
//!
//! Across the whole supported range (48..=current) exactly two hops are *not*
//! purely additive, and both are handled with a single `serde_json::Value` walk
//! before the typed parse:
//!
//! - **`ExternalCrate::path`** became a required `PathBuf` in format 57. Pre-57 JSON omits it, so
//!   we inject an empty value ([`add_external_crate_paths`]).
//! - **`Item::attrs`** was retyped from `Vec<String>` to the structured `Vec<Attribute>` in format
//!   54 (the "Structured Attributes" bump). Pre-54 JSON carries plain strings that will not
//!   deserialize into `Attribute`. ferritin never reads `attrs`, so we blank every array to `[]`
//!   ([`blank_attrs`]) — lossless for our purposes.
//!
//! (The one other non-additive change in range, `Path::args` gaining an `Option`
//! wrapper in format 51, needs no patch: serde reads a present value as `Some`,
//! so pre-51 data deserializes unchanged.)
//!
//! **If a future — or older — format makes a genuinely read-breaking change to a
//! field we consume** (unlike `attrs`, which we ignore), neither shortcut
//! applies: pull in a pinned `rustdoc-types` crate for that format, parse with
//! it, and translate to the canonical types here — the approach this module used
//! before the formats turned out to be near-uniformly additive (see git history
//! for the chained-conversion pattern).

use anyhow::{Context, Result};
use rustdoc_types::{Crate, FORMAT_VERSION};
use serde::Deserialize;

/// Oldest rustdoc JSON format version we can normalize.
///
/// 48 is the surveyed floor: every hop from 48 up is either additive or covered
/// by the two JSON-level patches in this module (see the module docs). Older
/// formats are untriaged, not known-broken — lowering this floor is a matter of
/// diffing the intervening `rustdoc-types` releases for a read-breaking change
/// to a field we actually consume.
const MIN_FORMAT_VERSION: u32 = 48;

/// The format version at which [`ExternalCrate::path`] became a required field.
/// JSON older than this must have the field injected before it will parse with
/// the canonical types.
const EXTERNAL_CRATE_PATH_VERSION: u32 = 57;

/// The format version at which [`Item::attrs`] became the structured
/// `Vec<Attribute>` (was `Vec<String>`). JSON older than this carries plain
/// strings that will not deserialize into `Attribute`, so we blank the field —
/// which ferritin never reads — before parsing.
const STRUCTURED_ATTRS_VERSION: u32 = 54;

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
        return parse_crate(json).with_context(|| {
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
        return parse_crate(json).with_context(|| {
            format!("Failed to parse rustdoc JSON (format version {format_version})")
        });
    }

    // Format 48..=56: JSON-level patching before the typed parse. Everything
    // below 57 needs `ExternalCrate::path` injected; everything below 54 also
    // needs `attrs` blanked (see module docs).
    let mut value: serde_json::Value =
        parse_unbounded(json).context("Failed to parse rustdoc JSON")?;
    if format_version < STRUCTURED_ATTRS_VERSION {
        blank_attrs(&mut value);
    }
    add_external_crate_paths(&mut value);
    Crate::deserialize(serde_stacker::Deserializer::new(&value)).with_context(|| {
        format!("Failed to parse normalized rustdoc JSON (was format version {format_version})")
    })
}

/// Parse JSON into the canonical `Crate` with serde_json, recursion limit
/// disabled, stack grown on demand (serde_stacker) — so nesting depth is
/// bounded by memory rather than a constant or the thread's stack size.
///
/// Deliberately *not* sonic-rs, although it deserializes the rest of the
/// workspace's (shallow) JSON: sonic enforces a hard 255-layer nesting cap
/// with no configuration surface (its error text claims 128, copied from
/// serde_json), and real crates exceed it — typenum's type-level integers
/// (`UInt<UInt<…>>` up to 2⁶³) nest several hundred JSON layers at ~6
/// layers per `Type` level. Benchmarked on real rustdoc JSON (2026-07,
/// typed `Crate` deserialization), sonic's win was only 1.07–1.10x on
/// 10–55 MB crates and 1.59x on 61 MB core.json — for a parse that runs
/// once per (version, schema-tag), not worth a second parser with a
/// correctness cliff on this, the one JSON surface with unbounded depth.
fn parse_crate(json: &[u8]) -> Result<Crate> {
    parse_unbounded(json).map_err(anyhow::Error::from)
}

/// serde_json deserialization with the recursion limit disabled and
/// serde_stacker growing the stack as the recursion descends.
fn parse_unbounded<'de, T: Deserialize<'de>>(json: &'de [u8]) -> serde_json::Result<T> {
    let mut deserializer = serde_json::Deserializer::from_slice(json);
    deserializer.disable_recursion_limit();
    T::deserialize(serde_stacker::Deserializer::new(&mut deserializer))
}

/// Peek the `format_version` field without parsing the whole document.
///
/// A reverse byte search rather than `sonic_rs::get_from_slice`: a pointed
/// lookup still has to *skip* every preceding top-level value, which
/// recurses per nesting layer — on typenum-scale documents that's a stack
/// overflow before parsing even starts. `format_version` is `Crate`'s last
/// field, so rustdoc serializes it at the very end of the document; the
/// last occurrence of the quoted key is the top-level one (any imposter in
/// a doc string would be `\"`-escaped anyway, and sits earlier, inside
/// `index`).
pub(crate) fn peek_format_version(json: &[u8]) -> Result<u32> {
    let key = b"\"format_version\"";
    let start = memchr::memmem::rfind(json, key)
        .with_context(|| "no format_version field found".to_string())?
        + key.len();
    let rest = &json[start..];
    let colon = memchr::memchr(b':', rest).context("format_version key without value")?;
    let digits: Vec<u8> = rest[colon + 1..]
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .take_while(u8::is_ascii_digit)
        .collect();
    anyhow::ensure!(!digits.is_empty(), "format_version is not a number");
    String::from_utf8_lossy(&digits)
        .parse()
        .context("format_version is not a valid u32")
}

/// Peek the top-level `crate_version` string without parsing the document,
/// leniently: absent field, JSON `null`, or an unparseable version all yield
/// `None` (matching the serde path this replaced). Same rationale as
/// [`peek_format_version`] — a pointed sonic lookup recurses over the
/// (potentially typenum-deep) `index` it skips. `crate_version` is `Crate`'s
/// second field, so its *first* occurrence is the top-level one; doc-string
/// imposters live later, inside `index`. Version strings contain no escapes,
/// so scanning to the closing quote suffices (anything weirder fails semver
/// and lands on `None`, which lenient allows).
pub(crate) fn peek_crate_version(json: &[u8]) -> Option<semver::Version> {
    let key = b"\"crate_version\"";
    let start = memchr::memmem::find(json, key)? + key.len();
    let rest = &json[start..];
    let colon = memchr::memchr(b':', rest)?;
    let value = rest[colon + 1..].trim_ascii_start();
    let value = value.strip_prefix(b"\"")?;
    let end = memchr::memchr(b'"', value)?;
    semver::Version::parse(&String::from_utf8_lossy(&value[..end])).ok()
}

/// Blank every item's `attrs` array to `[]`, so pre-54 JSON (where `attrs` is
/// `Vec<String>`) parses against the current `Vec<Attribute>` type. ferritin
/// never reads `attrs`, so discarding it is lossless for our purposes — this is
/// cheaper and more robust than mapping each legacy string into `Attribute`.
fn blank_attrs(value: &mut serde_json::Value) {
    if let Some(index) = value.get_mut("index").and_then(|v| v.as_object_mut()) {
        for item in index.values_mut() {
            if let Some(obj) = item.as_object_mut()
                && obj.contains_key("attrs")
            {
                obj.insert("attrs".to_string(), serde_json::json!([]));
            }
        }
    }
}

/// Add an empty `path` to every `external_crates` entry that lacks one, so
/// pre-57 JSON satisfies the required [`ExternalCrate::path`] field.
fn add_external_crate_paths(value: &mut serde_json::Value) {
    if let Some(external_crates) = value.get_mut("external_crates")
        && let Some(map) = external_crates.as_object_mut()
    {
        for ext_crate in map.values_mut() {
            if let Some(obj) = ext_crate.as_object_mut()
                && !obj.contains_key("path")
            {
                obj.insert("path".to_string(), serde_json::json!(""));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::synth_item;
    use rustdoc_types::{
        GenericArg, GenericArgs, Generics, Id, ItemEnum, Module, Path, Target, Type, TypeAlias,
    };

    /// The peeks lean on `Crate`'s serialized field order: `crate_version`
    /// second (first occurrence wins) and `format_version` last (last
    /// occurrence wins). Valid JSON can contain both key byte-sequences as
    /// imposters — an item *named* `format_version`, or a doc-link key in
    /// `links` (whose `Id` value would even parse as a number). Order is
    /// what defeats them, and order comes from rustdoc-types' field
    /// declarations — so this fixture plants every imposter shape, and a
    /// future rustdoc-types reordering fails here loudly instead of the
    /// peeks silently misreading in production.
    #[test]
    fn peeks_survive_imposter_keys_and_values() {
        let mut imposter = synth_item(
            1,
            Some("format_version"),
            ItemEnum::Module(Module {
                is_crate: false,
                items: vec![],
                is_stripped: false,
            }),
        );
        imposter.docs = Some(r#"see "format_version" and "crate_version" quoted"#.to_string());
        imposter.links.insert("format_version".to_string(), Id(3));
        imposter.links.insert("crate_version".to_string(), Id(4));
        let crate_version_imposter = synth_item(
            2,
            Some("crate_version"),
            ItemEnum::Module(Module {
                is_crate: false,
                items: vec![],
                is_stripped: false,
            }),
        );

        let mut index = rustc_hash::FxHashMap::default();
        index.insert(
            Id(0),
            synth_item(
                0,
                Some("imposters"),
                ItemEnum::Module(Module {
                    is_crate: true,
                    items: vec![Id(1), Id(2)],
                    is_stripped: false,
                }),
            ),
        );
        index.insert(Id(1), imposter);
        index.insert(Id(2), crate_version_imposter);
        let krate = Crate {
            root: Id(0),
            crate_version: Some("9.8.7".to_string()),
            includes_private: false,
            index,
            paths: Default::default(),
            external_crates: Default::default(),
            target: Target {
                triple: String::new(),
                target_features: vec![],
            },
            format_version: FORMAT_VERSION,
        };
        let json = serde_json::to_vec(&krate).unwrap();

        assert_eq!(peek_format_version(&json).unwrap(), FORMAT_VERSION);
        assert_eq!(
            peek_crate_version(&json),
            Some(semver::Version::new(9, 8, 7))
        );
    }

    #[test]
    fn peek_crate_version_handles_present_null_and_absent() {
        let json = br#"{"root":0,"crate_version":"1.20.1","includes_private":false}"#;
        assert_eq!(
            peek_crate_version(json),
            Some(semver::Version::new(1, 20, 1))
        );
        assert_eq!(peek_crate_version(br#"{"crate_version":null}"#), None);
        assert_eq!(peek_crate_version(br#"{"root":0}"#), None);
        assert_eq!(
            peek_crate_version(br#"{"crate_version": "2.0.0"}"#)
                .unwrap()
                .major,
            2
        );
    }

    /// A pre-54 doc carries `attrs` as `Vec<String>`, which cannot deserialize
    /// into the current `Vec<Attribute>`. Normalization must blank the field
    /// (which ferritin never reads) so the doc still parses — the whole point of
    /// supporting formats below the structured-attributes bump.
    #[test]
    fn pre_54_string_attrs_are_blanked_and_parse() {
        let mut item = synth_item(
            1,
            Some("Widget"),
            ItemEnum::Module(Module {
                is_crate: false,
                items: vec![],
                is_stripped: false,
            }),
        );
        item.attrs = vec![]; // canonical shape; we downgrade it in the JSON below

        let mut index = rustc_hash::FxHashMap::default();
        index.insert(
            Id(0),
            synth_item(
                0,
                Some("root"),
                ItemEnum::Module(Module {
                    is_crate: true,
                    items: vec![Id(1)],
                    is_stripped: false,
                }),
            ),
        );
        index.insert(Id(1), item);
        let krate = Crate {
            root: Id(0),
            crate_version: Some("1.2.3".to_string()),
            includes_private: false,
            index,
            paths: Default::default(),
            external_crates: Default::default(),
            target: Target {
                triple: String::new(),
                target_features: vec![],
            },
            format_version: FORMAT_VERSION,
        };

        // Downgrade to a format-53 wire shape: attrs as Vec<String>, older tag.
        let mut value: serde_json::Value =
            serde_json::from_slice(&serde_json::to_vec(&krate).unwrap()).unwrap();
        value["format_version"] = serde_json::json!(53);
        for entry in value["index"].as_object_mut().unwrap().values_mut() {
            entry["attrs"] = serde_json::json!(["#[repr(C)]", "#[inline]"]);
        }
        let json = serde_json::to_vec(&value).unwrap();

        // As-is it must fail (proving the blank is load-bearing)...
        assert!(
            parse_crate(&json).is_err(),
            "format-53 string attrs should not parse against Vec<Attribute>"
        );
        // ...and normalization must recover it.
        let normalized = load_and_normalize(&json, None).expect("pre-54 doc must normalize");
        assert!(normalized.index.contains_key(&Id(1)));
    }

    /// A format below the floor is a definitive "cannot read", not a silent
    /// empty parse.
    #[test]
    fn below_floor_bails() {
        let json = br#"{"root":0,"index":{},"paths":{},"external_crates":{},"format_version":47}"#;
        assert!(load_and_normalize(json, None).is_err());
        assert!(load_and_normalize(json, Some(47)).is_err());
    }

    /// typenum regression: its type-level integers nest hundreds of JSON
    /// layers deep (~6 per `Type` level) — the depth that overflowed the
    /// sonic-rs cap and every default recursion limit. Loading must handle
    /// depth bounded only by memory (see [`parse_unbounded`]).
    ///
    /// Runs on a 16 MB thread to match the smallest production stack (serve
    /// workers; the CLI main thread has 8 MB): the harness's default 2 MiB
    /// test threads overflow just *serializing* this fixture under
    /// debug-build frame sizes, which is not the code path under test.
    #[test]
    fn parses_type_nesting_beyond_sonic_depth_limit() {
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(parse_deeply_nested_crate)
            .unwrap()
            .join()
            .unwrap();
    }

    fn parse_deeply_nested_crate() {
        let mut ty = Type::Generic("T".to_string());
        for _ in 0..300 {
            ty = Type::ResolvedPath(Path {
                path: "Wrap".to_string(),
                id: Id(2),
                args: Some(Box::new(GenericArgs::AngleBracketed {
                    args: vec![GenericArg::Type(ty)],
                    constraints: vec![],
                })),
            });
        }

        let mut index = rustc_hash::FxHashMap::default();
        index.insert(
            Id(0),
            synth_item(
                0,
                Some("deep"),
                ItemEnum::Module(Module {
                    is_crate: true,
                    items: vec![Id(1)],
                    is_stripped: false,
                }),
            ),
        );
        index.insert(
            Id(1),
            synth_item(
                1,
                Some("Deep"),
                ItemEnum::TypeAlias(TypeAlias {
                    type_: ty,
                    generics: Generics {
                        params: vec![],
                        where_predicates: vec![],
                    },
                }),
            ),
        );
        let krate = Crate {
            root: Id(0),
            crate_version: None,
            includes_private: false,
            index,
            paths: Default::default(),
            external_crates: Default::default(),
            target: Target {
                triple: String::new(),
                target_features: vec![],
            },
            format_version: FORMAT_VERSION,
        };
        let json = serde_json::to_vec(&krate).unwrap();
        let parsed = load_and_normalize(&json, None).expect("deep parse must succeed");
        assert!(parsed.index.contains_key(&Id(1)));
    }
}
