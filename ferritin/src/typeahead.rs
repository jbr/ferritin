//! Crate-name typeahead over the shared [`CrateIndex`].
//!
//! The index — the crates.io namespace as a local artifact, fetched and kept
//! fresh by [`ferritin_common::crate_names`] — is the same one version
//! resolution reads, so the server holds one copy of it rather than two. This
//! module is only the shaping layer on top: it merges in the standard library
//! crates, which are not on crates.io, and hoists an exact match to the front.

use ferritin_common::crate_names::{CrateEntry, CrateIndex, normalize};
use std::sync::Arc;

/// An owned typeahead result.
#[derive(Debug, Clone)]
pub(crate) struct TypeaheadEntry {
    pub(crate) name: String,
    pub(crate) version: String,
}

impl From<CrateEntry> for TypeaheadEntry {
    fn from(entry: CrateEntry) -> Self {
        Self {
            name: entry.name,
            version: entry.version.to_string(),
        }
    }
}

/// What the resident crate namespace knows about one exact crate name.
///
/// The distinction between [`Unknown`](CrateLookup::Unknown) and
/// [`Indeterminate`](CrateLookup::Indeterminate) is the whole point: only the
/// former is evidence of absence, and only evidence of absence may be answered
/// with a `404`.
#[derive(Debug, Clone)]
pub(crate) enum CrateLookup {
    /// A crate this server can serve, as the artifact spells it.
    ///
    /// Deliberately carries no version. The artifact knows the crates.io
    /// *default* version, which is not necessarily the one a path asks for
    /// (`serde@0.9` resolves elsewhere) — and naming it in a preview would be
    /// confidently wrong in exactly the case a reader consults a preview to
    /// settle.
    Known {
        name: String,
        /// `None` for std (not on crates.io) and for crates.io crates that
        /// simply have no description.
        description: Option<String>,
    },
    /// The artifact is loaded and does not contain this name.
    Unknown,
    /// No artifact is loaded, so absence proves nothing about this name.
    Indeterminate,
}

/// The top-ranked matches plus the exact number of crates matching the
/// query (any query token, so a multi-word query counts its union of
/// per-term matches) — `entries.len() < total` means truncation occurred.
#[derive(Debug)]
pub(crate) struct TypeaheadResults {
    pub(crate) entries: Vec<TypeaheadEntry>,
    pub(crate) total: usize,
}

/// Shared server state answering crate-name typeahead queries.
#[derive(Debug)]
pub(crate) struct TypeaheadService {
    /// `None` without a docs.rs source, which is also a server that could not
    /// serve a crates.io crate if it offered one — so it offers only std.
    index: Option<Arc<CrateIndex>>,
    /// The standard library crates, resolved once at startup. They are not on
    /// crates.io, so the artifact cannot know about them, but ferritin serves
    /// their documentation — see [`Self::std_matches`].
    std_crates: Vec<TypeaheadEntry>,
}

impl TypeaheadService {
    /// `std_crates` are the standard library crates this server can actually
    /// serve, with the toolchain's version — resolved at startup so that
    /// answering a query never has to reach for the [`Store`](ferritin_common::Store).
    pub(crate) fn new(index: Option<Arc<CrateIndex>>, std_crates: Vec<TypeaheadEntry>) -> Self {
        Self { index, std_crates }
    }

    /// The standard library crates whose names start with `prefix`, folded the
    /// same way the artifact folds names, so `Std` and `proc-macro` match too.
    ///
    /// These are prepended to the crates.io results rather than ranked among
    /// them: `std` has no download count to rank by, and someone typing `std`
    /// on a Rust documentation site does not mean `stdweb`.
    fn std_matches(&self, prefix: &str) -> Vec<TypeaheadEntry> {
        let key = normalize(prefix);
        self.std_crates
            .iter()
            .filter(|entry| normalize(&entry.name).starts_with(&key))
            .cloned()
            .collect()
    }

    /// The top `limit` crates matching `prefix` (by name prefix or interior
    /// name token, scored by match tier and download rank — see
    /// [`CrateIndex::typeahead`](ferritin_common::crate_names::CrateIndex::typeahead)),
    /// plus the total match count. `None` means no data is available (cold
    /// start fetch failed or hasn't succeeded yet) — the endpoint maps it to a
    /// 503.
    ///
    /// There is no query-length floor: a single character already names a
    /// useful answer, since a 1-char query tokenizes to nothing (see
    /// [`name_tokens`](ferritin_common::crate_names)) and so falls through to
    /// the whole-name prefix alone, ranked by downloads — `s` means serde. An
    /// *empty* query is still nothing, though: it prefixes every name, so the
    /// only thing it could rank is the whole namespace.
    pub(crate) async fn typeahead(&self, prefix: &str, limit: usize) -> Option<TypeaheadResults> {
        if prefix.is_empty() {
            return Some(TypeaheadResults {
                entries: Vec::new(),
                total: 0,
            });
        }

        let (mut entries, mut total) = match &self.index {
            Some(index) => {
                let (entries, total) = index.typeahead(prefix, limit).await?;
                (
                    entries.into_iter().map(TypeaheadEntry::from).collect(),
                    total,
                )
            }
            None => (Vec::new(), 0),
        };

        // An exact match always sorts first, regardless of rank: typing
        // `trillium` must offer `trillium` ahead of the more-downloaded
        // `trillium-http`. The comparison folds whitespace the way the query
        // tokenizer does (plus `-`/`_` and case via `normalize`), so
        // `trillium tokio` exact-matches `trillium-tokio`. If it did not make
        // the top `limit` by rank, it is fetched and inserted.
        let folded = prefix.split_whitespace().collect::<Vec<_>>().join("-");
        let key = normalize(&folded);
        if let Some(position) = entries
            .iter()
            .position(|entry| normalize(&entry.name) == key)
        {
            entries[..=position].rotate_right(1);
        } else if let Some(index) = &self.index
            && let Some(exact) = index.get(&folded).await
        {
            entries.insert(0, exact.into());
            entries.truncate(limit);
        }

        // The std crates are absent from the artifact but present on this
        // server, so they are added here rather than being matched by the
        // binary search. They count toward `total` for the same reason.
        let std_matches = self.std_matches(prefix);
        if !std_matches.is_empty() {
            total += std_matches.len();
            entries.splice(0..0, std_matches);
            entries.truncate(limit);
        }
        Some(TypeaheadResults { entries, total })
    }

    /// What this server knows about one exact crate name, from resident data
    /// only — no crate is loaded, no network is touched, nothing is downloaded.
    ///
    /// Names are folded the way the artifact folds them, so `serde_json` and
    /// `serde-json` are the same lookup. std is checked first, since those
    /// crates are absent from the artifact but servable here.
    pub(crate) async fn lookup(&self, name: &str) -> CrateLookup {
        let key = normalize(name);

        if let Some(entry) = self
            .std_crates
            .iter()
            .find(|entry| normalize(&entry.name) == key)
        {
            return CrateLookup::Known {
                name: entry.name.clone(),
                description: None,
            };
        }

        let Some(index) = &self.index else {
            return CrateLookup::Indeterminate;
        };

        match index.get(name).await {
            Some(entry) => CrateLookup::Known {
                name: entry.name,
                description: entry.description,
            },

            // A miss against an artifact that never loaded is not absence. Only
            // a loaded artifact can testify that a name is not a crate.
            None if index.identity().is_none() => CrateLookup::Indeterminate,
            None => CrateLookup::Unknown,
        }
    }

    /// The identity of the artifact data backing every answer this service
    /// gives — see [`CrateIndex::identity`]. Read *after* a query, so it
    /// describes the data that query actually saw.
    pub(crate) fn artifact_etag(&self) -> Option<String> {
        self.index.as_ref()?.identity()
    }
}
