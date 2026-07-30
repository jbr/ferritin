//! Scoring and side indexes for searching the crates.io namespace: the
//! typeahead/crate-search engine over the artifacts [`CrateIndex`] holds.
//!
//! [`typeahead_scored`] is the entry point. A query is tokenized and scored
//! additively against four kinds of evidence — name tokens ([`TokenIndex`]),
//! whole-name prefixes, stemmed description words ([`DescriptionIndex`]),
//! and stemmed declared keywords ([`KeywordIndex`]) — with download rank
//! ordering within a tier, and a trigram-driven fuzzy pass
//! ([`TrigramIndex`]) filling underfull pages. The weights and their war
//! stories live on [`TypeaheadWeights`]; the regression battery pinning the
//! settled behaviors is `crate_names::battery`.
//!
//! [`CrateIndex`]: super::CrateIndex

use super::{CrateEntry, Loaded, entry};
use crate::{
    search::QueryCompletion,
    string_utils::{case_aware_jaro_winkler, stem},
};
use crate_names::{CrateNames, Descriptions, Facets, normalize};
use rustc_hash::FxHashMap;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::Instant,
};

/// Additive typeahead scoring weights. A crate's score is
/// `term_match · (query tokens matched) + whole_prefix (for a whole-name-prefix
/// match) + rank · (download rank)`, so matching more of the query dominates,
/// and within a match tier the log-quantized download rank (0..=255, ~8 units
/// per download-doubling) orders similar crates. Tuning knobs — the defaults
/// are the shipped values, and the probe harness in `tests` explores others.
#[derive(Debug, Clone, Copy)]
pub(super) struct TypeaheadWeights {
    /// Per matched query token. Also the penalty for each *missed* token: a
    /// crate matching all terms of `trillium tokio` outscores a one-term match
    /// unless the popularity gap exceeds this many rank units — at 128,
    /// ~65,000×, a gap the namespace barely contains, so in practice full
    /// matches sort first and this stays additive only in the extremes.
    pub(super) term_match: f32,
    /// Nudge for a whole-name-prefix match (the name starts with the query,
    /// whitespace folded to `-`) over an interior-token match of the same
    /// term count — ~1.5 download-doublings between similarly popular crates.
    pub(super) whole_prefix: f32,
    /// Bonus when a *multi-token* query's every token exactly equals one of
    /// the crate's name tokens (no prefixing needed), lifting `tokio-util-*`
    /// over `tokio-utilities` once `util` is fully typed. Single-token queries
    /// are exempt: one token is usually mid-typing, and probing showed the
    /// bonus lifting obscure interior-exact matches (`assert-json-diff` for
    /// `json`) over popular continuations (`jsonwebtoken`). The complete-name
    /// case (`trillium tokio` ≡ `trillium-tokio`) is stronger still: the
    /// service layer hoists it to the front outright.
    pub(super) all_exact: f32,
    /// Per query token matched in the crate's *description* (see
    /// [`DescriptionIndex`]) and **not** in its name. Credit per token is
    /// `max(name, description)`, never the sum: a
    /// token found in both is one piece of evidence seen twice, and summing
    /// it demotes the crate a query actually names in favor of its
    /// neighbors. `serde`'s own description says
    /// "serialization/deserialization framework", never "serde", so under a
    /// summing rule the query `serde` ranked `serde_spanned` and
    /// `serde_urlencoded` — which do say it — above `serde` itself.
    ///
    /// Deliberately a fraction of `term_match`: a description mention is
    /// weaker evidence than a name match, and its job is to reach crates the
    /// name index cannot see at all (`deserialization` → `serde`) rather than
    /// to reorder the ones it can. Zero disables description matching
    /// outright, including building the index.
    pub(super) description_match: f32,
    /// Per query token matched in the crate's declared *keywords* (see
    /// [`KeywordIndex`]) and in neither its name nor its description — the
    /// third tier of the same per-token max rule. Below `description_match`:
    /// a keyword is authorial intent without prose context — the author
    /// tagged the term but didn't say it — and keywords frequently duplicate
    /// name/description tokens (serde declares `serde`, `serialization`),
    /// which the max rule prevents from double-counting. The tier's job is
    /// the token that appears *nowhere else*: `rumqttc` declares `mqtt`,
    /// `clap` declares `cli`. Zero disables keyword matching outright,
    /// including building the index.
    pub(super) keyword_match: f32,
    /// Per-unit contribution of the download rank.
    pub(super) rank: f32,
}

impl Default for TypeaheadWeights {
    fn default() -> Self {
        Self {
            term_match: 128.0,
            whole_prefix: 12.0,
            all_exact: 16.0,
            description_match: 96.0,
            keyword_match: 80.0,
            rank: 1.0,
        }
    }
}

/// The scoring core of [`CrateIndex::typeahead`], parameterized by weights so
/// the probe harness can explore the space; production callers pass
/// [`TypeaheadWeights::default`].
pub(super) fn typeahead_scored(
    loaded: &Loaded,
    prefix: &str,
    limit: usize,
    weights: &TypeaheadWeights,
) -> (Vec<CrateEntry>, usize) {
    let token_index = loaded
        .token_index
        .get_or_init(|| TokenIndex::build(&loaded.names));

    let mut query_tokens: Vec<String> = name_tokens(prefix).collect();
    query_tokens.sort_unstable();
    query_tokens.dedup();

    // Per-term credit: each crate is scored by how many distinct query tokens
    // it matched and how well, so a full match ranks above a subset match but
    // both are candidates. Description and keyword matching (when enabled)
    // add candidates the name index cannot reach at all — `deserialization`
    // finds serde, `mqtt` finds rumqttc.
    let description_index = (weights.description_match > 0.0).then(|| loaded.description_index());
    let keyword_index = (weights.keyword_match > 0.0).then(|| loaded.keyword_index());
    let mut candidates = match_counts(
        &query_tokens,
        token_index,
        description_index,
        keyword_index,
        QueryCompletion::AsYouType,
    );

    // The crates where every query token matches a name token *exactly* — a
    // finished-typing signal worth a bonus over prefix-only full matches.
    let all_exact = all_exact_indices(&query_tokens, token_index);

    // Whole-name prefix, with query whitespace folded to `-` so a
    // space-separated query still matches a hyphenated name. Such a name
    // contains the entire query, so it credits every term (a token the
    // tokenizer dropped, e.g. a 1-char segment, still matched textually) plus
    // the whole-prefix nudge. Empty only for an all-whitespace query, which
    // then matches nothing.
    let whole_key = prefix.split_whitespace().collect::<Vec<_>>().join("-");
    let whole = if whole_key.is_empty() {
        0..0
    } else {
        loaded.names.prefix_indices(&whole_key)
    };
    for crate_index in whole.clone() {
        let matched = candidates.entry(crate_index as u32).or_default();
        matched.name = matched.name.max(query_tokens.len() as u32);
    }

    let scored: Vec<(f32, u32)> = candidates
        .iter()
        .filter_map(|(&crate_index, matched)| {
            let found = loaded.names.entry_at(crate_index as usize)?;
            let whole_bonus = if whole.contains(&(crate_index as usize)) {
                weights.whole_prefix
            } else {
                0.0
            };
            let exact_bonus = if all_exact.contains(&crate_index) {
                weights.all_exact
            } else {
                0.0
            };
            let score = weights.term_match * matched.name as f32
                + weights.description_match * matched.description as f32
                + weights.keyword_match * matched.keyword as f32
                + whole_bonus
                + exact_bonus
                + weights.rank * f32::from(found.rank);
            Some((score, crate_index))
        })
        .collect();
    let total = scored.len();
    let mut entries = top_entries(loaded, scored, limit);

    // When prefix/token matching underfills the page, fill the remaining slots
    // with fuzzy matches — so a typo like `tokoi` still surfaces `tokio`. These
    // sort *after* every prefix/token match by construction (they are only
    // appended), and `total` is lifted to cover them so the caller doesn't read
    // the padded page as truncated.
    let mut total = total;
    if entries.len() < limit {
        let mut seen: HashSet<String> = entries.iter().map(|e| normalize(&e.name)).collect();
        for extra in fuzzy_scored(loaded, prefix, limit) {
            if entries.len() >= limit {
                break;
            }
            if seen.insert(normalize(&extra.name)) {
                entries.push(extra);
            }
        }
        total = total.max(entries.len());
    }

    (entries, total)
}

/// Complete-word crate search — the agent sibling of [`typeahead_scored`].
/// Same evidence tiers and weights, but tokens match *exactly* rather than as
/// prefixes, and nothing pads the page:
///
/// - No prefix semantics: an agent's words are whole words, and prefix matching misleads there —
///   `cli` prefix-matches every `client` crate and buries clap, while an exact `cli` reaches clap
///   through its declared keyword.
/// - No fuzzy fill and no whole-prefix/all-exact typing nudges: no results is a real answer for an
///   agent, where a page of least-bad matches invites a confident wrong guess.
pub(super) fn crate_search(loaded: &Loaded, query: &str, limit: usize) -> (Vec<CrateEntry>, usize) {
    let weights = TypeaheadWeights::default();
    let token_index = loaded
        .token_index
        .get_or_init(|| TokenIndex::build(&loaded.names));

    let mut query_tokens: Vec<String> = name_tokens(query).collect();
    query_tokens.sort_unstable();
    query_tokens.dedup();
    if query_tokens.is_empty() {
        return (Vec::new(), 0);
    }

    let candidates = match_counts(
        &query_tokens,
        token_index,
        Some(loaded.description_index()),
        Some(loaded.keyword_index()),
        QueryCompletion::Complete,
    );

    let scored: Vec<(f32, u32)> = candidates
        .iter()
        .filter_map(|(&crate_index, matched)| {
            let found = loaded.names.entry_at(crate_index as usize)?;
            let score = weights.term_match * matched.name as f32
                + weights.description_match * matched.description as f32
                + weights.keyword_match * matched.keyword as f32
                + weights.rank * f32::from(found.rank);
            Some((score, crate_index))
        })
        .collect();
    let total = scored.len();
    (top_entries(loaded, scored, limit), total)
}

/// Order candidates best-first and materialize the top `limit` as entries.
/// Descending by score, ties broken by name so the order is total and the
/// page is deterministic.
///
/// Selects the page, then sorts only it. Fully sorting is a real cost here,
/// not a micro-optimization: description matching routinely produces tens
/// of thousands of candidates, `rank` is a `u8` so they pile into 256 score
/// buckets, and the name tie-break that separates them costs two artifact
/// lookups per comparison. Sorting all 15k candidates of `random number
/// generator` to show 8 of them was 18 of the query's 28ms.
fn top_entries(loaded: &Loaded, mut scored: Vec<(f32, u32)>, limit: usize) -> Vec<CrateEntry> {
    let ranked = |a: &(f32, u32), b: &(f32, u32)| {
        b.0.total_cmp(&a.0).then_with(|| {
            let a_name = loaded.names.entry_at(a.1 as usize).map(|e| e.name);
            let b_name = loaded.names.entry_at(b.1 as usize).map(|e| e.name);
            a_name.cmp(&b_name)
        })
    };
    if scored.len() > limit {
        scored.select_nth_unstable_by(limit, ranked);
        scored.truncate(limit);
    }
    scored.sort_by(ranked);

    scored
        .into_iter()
        .filter_map(|(_, crate_index)| entry(loaded, loaded.names.entry_at(crate_index as usize)?))
        .collect()
}

/// Safety ceiling on how many trigram-overlap candidates are scored with
/// [`case_aware_jaro_winkler`] per fuzzy query. Not a tuning knob: it is set far
/// above any real query's candidate set (the commonest trigrams reach ~13k
/// names) purely to bound a pathological query. Candidates are *not* pre-cut by
/// overlap count below this — doing so drops the true match in a boundary
/// transposition of a short name (`tokoi`/`tokio` share only the very common
/// `tok`), and jaro scoring is cheap enough (~200ns each) to run over the whole
/// natural candidate set. If the ceiling ever bites, the highest-overlap
/// candidates are kept, since a near match cannot share *fewer* trigrams than an
/// unrelated one at equal name length.
const FUZZY_CANDIDATE_CEILING: usize = 20_000;

/// Minimum [`case_aware_jaro_winkler`] similarity for a fuzzy match to be
/// offered. The floor exists so that genuine gibberish yields no suggestions at
/// all, rather than the five least-dissimilar random crates.
const FUZZY_THRESHOLD: f64 = 0.7;

/// Fuzzy crate-name matches for `query`, ranked by similarity then download
/// rank. Candidate generation is a trigram-overlap gather over [`TrigramIndex`]
/// (scored in full, save the [`FUZZY_CANDIDATE_CEILING`] backstop); survivors
/// are scored with [`case_aware_jaro_winkler`] and filtered to
/// [`FUZZY_THRESHOLD`]. Powers both the typeahead fuzzy fill above and
/// crate-name "did you mean" suggestions.
fn fuzzy_scored(loaded: &Loaded, query: &str, limit: usize) -> Vec<CrateEntry> {
    let index = loaded
        .trigram_index
        .get_or_init(|| TrigramIndex::build(&loaded.names));

    let norm = normalize(query);
    let mut grams: Vec<[u8; 3]> = trigrams(&norm).collect();
    grams.sort_unstable();
    grams.dedup();

    // Per-candidate count of how many distinct query trigrams it shares.
    let mut overlap: HashMap<u32, u32> = HashMap::new();
    for gram in &grams {
        for &crate_index in index.indices_with_trigram(gram) {
            *overlap.entry(crate_index).or_insert(0) += 1;
        }
    }

    let mut candidates: Vec<(u32, u32)> = overlap
        .into_iter()
        .map(|(crate_index, count)| (count, crate_index))
        .collect();
    // Only the pathological-query backstop cuts anything; real candidate sets
    // stay well under the ceiling and are scored in full.
    if candidates.len() > FUZZY_CANDIDATE_CEILING {
        candidates.select_nth_unstable_by(FUZZY_CANDIDATE_CEILING, |a, b| b.0.cmp(&a.0));
        candidates.truncate(FUZZY_CANDIDATE_CEILING);
    }

    let mut scored: Vec<(f64, u8, &str, usize)> = candidates
        .into_iter()
        .filter_map(|(_, crate_index)| {
            let found = loaded.names.entry_at(crate_index as usize)?;
            let score = case_aware_jaro_winkler(found.name, query);
            (score >= FUZZY_THRESHOLD).then_some((
                score,
                found.rank,
                found.name,
                crate_index as usize,
            ))
        })
        .collect();

    scored.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.cmp(b.2))
    });
    scored.truncate(limit);

    scored
        .into_iter()
        .filter_map(|(_, _, _, crate_index)| entry(loaded, loaded.names.entry_at(crate_index)?))
        .collect()
}

/// Character trigrams of a normalized name, as fixed-size keys. A name shorter
/// than three bytes yields a single zero-padded key so it stays matchable
/// (crate names are ASCII after [`normalize`], so `0` never occurs in one).
fn trigrams(norm: &str) -> impl Iterator<Item = [u8; 3]> + '_ {
    let bytes = norm.as_bytes();
    let short = (bytes.len() < 3).then(|| {
        let mut key = [0u8; 3];
        key[..bytes.len()].copy_from_slice(bytes);
        key
    });
    let windows = (bytes.len() >= 3)
        .then(|| bytes.windows(3).map(|w| [w[0], w[1], w[2]]))
        .into_iter()
        .flatten();
    short.into_iter().chain(windows)
}

/// A lazily-built inverted index over the character trigrams of each crate's
/// normalized name, mapping each trigram to the [`CrateNames`] line indices it
/// occurs in. The fuzzy analogue of [`TokenIndex`]: same line-index values, same
/// build-once/drop-on-refresh lifecycle, but keyed by 3-byte character windows
/// so it can find near-misses (`tokoi` → `tokio`) that token prefixes cannot.
#[derive(Debug, Default)]
pub(super) struct TrigramIndex {
    /// Sorted by trigram, so a lookup is a binary search. Values are sorted,
    /// deduped crate line indices.
    postings: Vec<([u8; 3], Vec<u32>)>,
}

impl TrigramIndex {
    pub(super) fn build(names: &CrateNames) -> Self {
        let start = Instant::now();
        let mut map: BTreeMap<[u8; 3], Vec<u32>> = BTreeMap::new();
        for index in 0..names.len() {
            let Some(found) = names.entry_at(index) else {
                continue;
            };
            let norm = normalize(found.name);
            for gram in trigrams(&norm) {
                map.entry(gram).or_default().push(index as u32);
            }
        }
        let index = Self {
            postings: map
                .into_iter()
                .map(|(gram, mut indices)| {
                    // A trigram repeated within one name pushes its index
                    // twice adjacently; across names indices ascend. Either way
                    // adjacent-dedup leaves each index once.
                    indices.dedup();
                    (gram, indices)
                })
                .collect(),
        };
        log::debug!(
            "⏱️ built trigram index ({} trigrams) in {:?}",
            index.postings.len(),
            start.elapsed()
        );
        index
    }

    /// The crate line-indices whose name contains `gram` (already sorted and
    /// deduped at build time).
    fn indices_with_trigram(&self, gram: &[u8; 3]) -> &[u32] {
        let start = self.postings.partition_point(|(entry, _)| entry < gram);
        match self.postings.get(start) {
            Some((entry, indices)) if entry == gram => indices,
            _ => &[],
        }
    }
}

/// A lazily-built inverted index over crate-name tokens — the `-`/`_` separated
/// segments of each name — mapping each token to the [`CrateNames`] line indices
/// whose name contains it. Values are line indices, so a lookup refers back into
/// the artifact without copying names. Built on the first typeahead query and
/// dropped with each artifact refresh, so it never goes stale and the CLI —
/// which only calls [`CrateIndex::get`] — never pays to build it.
#[derive(Debug, Default)]
pub(super) struct TokenIndex {
    /// Sorted by token, so a prefix range is a binary search.
    postings: Vec<(String, Vec<u32>)>,
}

impl TokenIndex {
    pub(super) fn build(names: &CrateNames) -> Self {
        let mut map: BTreeMap<String, Vec<u32>> = BTreeMap::new();
        for index in 0..names.len() {
            let Some(found) = names.entry_at(index) else {
                continue;
            };
            for token in name_tokens(found.name) {
                map.entry(token).or_default().push(index as u32);
            }
        }
        Self {
            postings: map
                .into_iter()
                .map(|(token, mut indices)| {
                    // Indices were pushed in ascending order, so a token
                    // repeated within one name leaves adjacent dupes.
                    indices.dedup();
                    (token, indices)
                })
                .collect(),
        }
    }

    /// The crate line-indices having exactly this token (already sorted and
    /// deduped at build time).
    fn indices_with_token(&self, token: &str) -> &[u32] {
        token_postings(&self.postings, token)
    }

    /// The distinct crate line-indices having a token that begins with `prefix`,
    /// sorted and deduped.
    fn indices_with_prefix(&self, prefix: &str) -> Vec<u32> {
        prefix_postings(&self.postings, prefix)
    }
}

/// The posting list for exactly `term`, over a term-sorted postings table —
/// already sorted and deduped at build time. The shared exact lookup of
/// [`TokenIndex`], [`DescriptionIndex`], and [`KeywordIndex`], for
/// complete-word queries.
fn token_postings<'a>(postings: &'a [(String, Vec<u32>)], term: &str) -> &'a [u32] {
    let start = postings.partition_point(|(entry, _)| entry.as_str() < term);
    match postings.get(start) {
        Some((entry, indices)) if entry == term => indices,
        _ => &[],
    }
}

/// The sorted, deduped union of every posting list whose term begins with
/// `prefix`, over a term-sorted postings table. The shared lookup of
/// [`TokenIndex`], [`DescriptionIndex`], and [`KeywordIndex`]; iteration
/// stops at the first non-matching term in the binary-searched range.
fn prefix_postings(postings: &[(String, Vec<u32>)], prefix: &str) -> Vec<u32> {
    let start = postings.partition_point(|(term, _)| term.as_str() < prefix);

    let mut indices = Vec::new();
    for (term, crate_indices) in &postings[start..] {
        if !term.starts_with(prefix) {
            break;
        }
        indices.extend_from_slice(crate_indices);
    }
    indices.sort_unstable();
    indices.dedup();
    indices
}

/// Words shorter than this are not indexed from a description. Two-letter
/// words are almost entirely function words (`in`, `of`, `to`), and the
/// meaningful exceptions people search for (`io`, `os`) are crate *names*,
/// which the name index already covers.
const DESCRIPTION_MIN_CHARS: usize = 3;

/// A stem occurring in more than this fraction of all descriptions is dropped
/// from the index rather than kept: `rust`, `librari`, `implement`, `use` and
/// their kin match so much of the namespace that they only add noise and
/// postings. A frequency cut is preferred to a hand-written stopword list
/// because it adapts to what this corpus actually looks like — crates.io
/// descriptions are not general English — and needs no maintenance.
const DESCRIPTION_MAX_DOCUMENT_FREQUENCY: f32 = 0.05;

/// A lazily-built inverted index over the *stemmed* words of each crate's
/// crates.io description, mapping each stem to the [`CrateNames`] line indices
/// whose description contains it — the same line-index values as
/// [`TokenIndex`], so description matches and name matches score against one
/// candidate map.
///
/// Stemming is what makes this worth having: descriptions are prose, so a
/// query for `deserialize` must reach a description that says
/// `deserialization`, which exact token matching cannot do. Names get no such
/// treatment — a crate name is not an English word, and stemming would turn
/// `serde` into `serd`.
///
/// Same build-once/drop-on-refresh lifecycle as the other side indexes: it is
/// built on the first typeahead query and dropped with each artifact refresh,
/// so it is never stale, and the CLI — which only calls [`CrateIndex::get`] —
/// never pays to build it.
#[derive(Debug, Default)]
pub(super) struct DescriptionIndex {
    /// Sorted by stem, so a prefix range is a binary search.
    pub(super) postings: Vec<(String, Vec<u32>)>,
}

impl DescriptionIndex {
    /// Both artifacts are sorted by the same folded name key and the
    /// descriptions are a subset of the names, so one merge walk translates
    /// each description into the names line index it belongs to without
    /// building a 300k-entry name→index map.
    ///
    /// This walks the whole namespace, so it is written to allocate only when
    /// it has to: names are compared folded in place, each word is lowercased
    /// into one reusable buffer, and a stem is turned into a `String` only the
    /// first time it is seen.
    pub(super) fn build(names: &CrateNames, descriptions: &Descriptions) -> Self {
        let start = Instant::now();
        let mut map: FxHashMap<String, Vec<u32>> = FxHashMap::default();
        let mut word = String::new();

        let mut cursor = 0;
        let mut cursor_name = names.entry_at(0).map(|entry| entry.name);
        for (name, description) in descriptions.iter() {
            while cursor_name.is_some_and(|current| folded_cmp(current, name).is_lt()) {
                cursor += 1;
                cursor_name = names.entry_at(cursor).map(|entry| entry.name);
            }
            if !cursor_name.is_some_and(|current| folded_cmp(current, name).is_eq()) {
                // A description for a crate the names artifact doesn't have.
                // The two are published together so this shouldn't happen, but
                // skipping is the only sane response and keeps the walk in step.
                continue;
            }
            for raw in description.split(|c: char| !c.is_alphanumeric()) {
                if raw.chars().count() < DESCRIPTION_MIN_CHARS {
                    continue;
                }
                word.clear();
                word.extend(raw.chars().flat_map(char::to_lowercase));
                let stemmed = stem(&word);
                match map.get_mut(stemmed.as_ref()) {
                    Some(indices) => indices.push(cursor as u32),
                    None => {
                        map.insert(stemmed.into_owned(), vec![cursor as u32]);
                    }
                }
            }
        }

        let ceiling = (names.len() as f32 * DESCRIPTION_MAX_DOCUMENT_FREQUENCY) as usize;
        let indexed = map.len();
        let mut postings: Vec<(String, Vec<u32>)> = map
            .into_iter()
            .filter_map(|(word, mut indices)| {
                // A stem repeated within one description pushes its index twice
                // adjacently; across crates indices ascend.
                indices.dedup();
                (indices.len() <= ceiling).then_some((word, indices))
            })
            .collect();
        postings.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));

        log::debug!(
            "⏱️ built description index ({} stems, {} dropped above df {ceiling}) in {:?}",
            postings.len(),
            indexed - postings.len(),
            start.elapsed()
        );
        Self { postings }
    }

    /// The distinct crate line-indices whose description contains a stem
    /// beginning with `prefix`, sorted and deduped.
    pub(super) fn indices_with_prefix(&self, prefix: &str) -> Vec<u32> {
        prefix_postings(&self.postings, prefix)
    }

    /// The crate line-indices whose description contains exactly this stem
    /// (already sorted and deduped at build time).
    fn indices_with_token(&self, stem: &str) -> &[u32] {
        token_postings(&self.postings, stem)
    }
}

/// A lazily-built inverted index over the *stemmed* declared keywords of each
/// crate — its Cargo.toml `keywords`, from the facets artifact — mapping each
/// stem to the [`CrateNames`] line indices that declared it. The same
/// line-index values as [`TokenIndex`] and [`DescriptionIndex`], so all three
/// evidence tiers score against one candidate map.
///
/// Keywords are stemmed exactly like description words and looked up with the
/// same already-stemmed query token. Verbatim indexing was the first design
/// and was reversed in review: many keywords are English words
/// (`serialization`, `websockets`), and verbatim matching misses the
/// morphological variants a query actually uses (`serialize` does not prefix
/// `serialization`), while stemming is a no-op on the tag-shaped keywords
/// (`mqtt`, `grpc`) the tier exists for. The name-stemming objection
/// (`serde` → `serd`) does not apply here because both sides of the
/// comparison are stemmed consistently.
///
/// Hyphenated keywords (`command-line`) are split into segments so they meet
/// query tokens on their own terms. Segments keep the 2-char floor of
/// [`name_tokens`] rather than the 3-char prose floor
/// ([`DESCRIPTION_MIN_CHARS`]): curated tags like `io` or `cli` are
/// meaningful where 2-letter prose words are function-word noise. There is no
/// document-frequency ceiling — the vocabulary is author-curated and capped
/// at five keywords per crate, so nothing approaches prose stopword
/// frequency.
#[derive(Debug, Default)]
pub(super) struct KeywordIndex {
    /// Sorted by stem, so a prefix range is a binary search.
    postings: Vec<(String, Vec<u32>)>,
}

impl KeywordIndex {
    /// The same merge walk as [`DescriptionIndex::build`]: the facets
    /// artifact is sorted by the same folded key as the names and is a subset
    /// of them, so each facets line translates to a names line index without
    /// a 300k-entry map.
    pub(super) fn build(names: &CrateNames, facets: &Facets) -> Self {
        let start = Instant::now();
        let mut map: FxHashMap<String, Vec<u32>> = FxHashMap::default();
        let mut word = String::new();

        let mut cursor = 0;
        let mut cursor_name = names.entry_at(0).map(|entry| entry.name);
        for facet in facets.iter() {
            while cursor_name.is_some_and(|current| folded_cmp(current, facet.name).is_lt()) {
                cursor += 1;
                cursor_name = names.entry_at(cursor).map(|entry| entry.name);
            }
            if !cursor_name.is_some_and(|current| folded_cmp(current, facet.name).is_eq()) {
                continue;
            }
            for raw in facet
                .keywords()
                .flat_map(|keyword| keyword.split(|c: char| !c.is_alphanumeric()))
            {
                if raw.chars().count() < 2 {
                    continue;
                }
                word.clear();
                word.extend(raw.chars().flat_map(char::to_lowercase));
                let stemmed = stem(&word);
                match map.get_mut(stemmed.as_ref()) {
                    Some(indices) => indices.push(cursor as u32),
                    None => {
                        map.insert(stemmed.into_owned(), vec![cursor as u32]);
                    }
                }
            }
        }

        let mut postings: Vec<(String, Vec<u32>)> = map
            .into_iter()
            .map(|(stemmed, mut indices)| {
                // A stem repeated across one crate's keywords pushes its
                // index twice adjacently; across crates indices ascend.
                indices.dedup();
                (stemmed, indices)
            })
            .collect();
        postings.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));

        log::debug!(
            "⏱️ built keyword index ({} stems) in {:?}",
            postings.len(),
            start.elapsed()
        );
        Self { postings }
    }

    /// The distinct crate line-indices declaring a keyword stem beginning
    /// with `prefix`, sorted and deduped.
    fn indices_with_prefix(&self, prefix: &str) -> Vec<u32> {
        prefix_postings(&self.postings, prefix)
    }

    /// The crate line-indices declaring exactly this keyword stem (already
    /// sorted and deduped at build time).
    fn indices_with_token(&self, stem: &str) -> &[u32] {
        token_postings(&self.postings, stem)
    }
}

/// Order two crate names by the folded key the artifacts are sorted under —
/// ASCII case, with `-` and `_` equivalent — without allocating. Mirrors
/// [`crate_names::normalize`], of which the reader exposes only the allocating
/// form; the merge walk in [`DescriptionIndex::build`] does this ~600k times,
/// which is worth not allocating for.
fn folded_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    fn fold(byte: u8) -> u8 {
        match byte {
            b'_' => b'-',
            other => other.to_ascii_lowercase(),
        }
    }
    left.bytes().map(fold).cmp(right.bytes().map(fold))
}

/// How many distinct query tokens a crate matched, split by where. A token is
/// counted in exactly one of the two — the name if it matched there, the
/// description otherwise — so a crate is never paid twice for one token.
///
/// Credit is deliberately *flat* per token, not IDF-graded. Scaling
/// description credit by stem rarity was implemented and removed
/// (2026-07-30): a concept query's description matches all share the same
/// common stems, so IDF demoted them uniformly against the name tier —
/// which for a concept query is literal-named junk — and every graded
/// variant made the battery's pages worse (`rand` behind `random-number`,
/// `snafu` behind `error_handling`). What actually separates an incidental
/// mention from a canonical crate is evidence df cannot see; declared
/// keywords carry it instead.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct MatchCounts {
    /// Tokens prefixing one of the crate's *name* tokens.
    pub(super) name: u32,
    /// Tokens found only in the crate's *description*.
    pub(super) description: u32,
    /// Tokens found only in the crate's declared *keywords* — in neither its
    /// name nor its description.
    pub(super) keyword: u32,
}

/// Count each query token's best match per crate. The tokens are pre-deduped
/// and each index dedupes per token, so counting is exact; an empty query
/// yields no candidates.
///
/// Description and keyword tokens are stemmed before lookup so they meet
/// those indexes on their own terms. Under `AsYouType` every lookup then
/// matches as a prefix, so a word still being typed can match — the residual
/// gap is a *partly*-typed word whose stem diverges from the whole word's
/// (`deserializ` does not prefix `deseri`), which the name and fuzzy passes
/// still cover. Under `Complete` every lookup is exact: the tokens are whole
/// words, and prefix matching would reach names the caller didn't say (`cli`
/// prefixes `client`).
pub(super) fn match_counts(
    query_tokens: &[String],
    names: &TokenIndex,
    descriptions: Option<&DescriptionIndex>,
    keywords: Option<&KeywordIndex>,
    completion: QueryCompletion,
) -> HashMap<u32, MatchCounts> {
    let prefix = completion == QueryCompletion::AsYouType;
    let mut counts: HashMap<u32, MatchCounts> = HashMap::new();
    for token in query_tokens {
        let named = if prefix {
            names.indices_with_prefix(token)
        } else {
            names.indices_with_token(token).to_vec()
        };
        for &crate_index in &named {
            counts.entry(crate_index).or_default().name += 1;
        }

        if descriptions.is_none() && keywords.is_none() {
            continue;
        }
        let stemmed = stem(token);

        let described = match descriptions {
            Some(descriptions) if token.chars().count() >= DESCRIPTION_MIN_CHARS => {
                if prefix {
                    descriptions.indices_with_prefix(&stemmed)
                } else {
                    descriptions.indices_with_token(&stemmed).to_vec()
                }
            }
            _ => Vec::new(),
        };
        for &crate_index in &described {
            // `named` is sorted and deduped, so this is the cheap half of the
            // max: a token already credited to the name is not credited again.
            if named.binary_search(&crate_index).is_err() {
                counts.entry(crate_index).or_default().description += 1;
            }
        }

        let Some(keywords) = keywords else {
            continue;
        };
        let keyworded = if prefix {
            keywords.indices_with_prefix(&stemmed)
        } else {
            keywords.indices_with_token(&stemmed).to_vec()
        };
        for crate_index in keyworded {
            // The third rung of the same max rule: a token already credited
            // to the name or description is not credited again.
            if named.binary_search(&crate_index).is_err()
                && described.binary_search(&crate_index).is_err()
            {
                counts.entry(crate_index).or_default().keyword += 1;
            }
        }
    }
    counts
}

/// The crates where *every* query token exactly equals one of the name's
/// tokens (the intersection of the exact posting lists, which are sorted).
/// Empty for queries of fewer than two tokens: the bonus is about *combining*
/// terms, and a lone token is usually still being typed (see
/// [`TypeaheadWeights::all_exact`]).
pub(super) fn all_exact_indices(query_tokens: &[String], index: &TokenIndex) -> HashSet<u32> {
    if query_tokens.len() < 2 {
        return HashSet::new();
    }
    let mut postings = query_tokens
        .iter()
        .map(|token| index.indices_with_token(token));
    let Some(first) = postings.next() else {
        return HashSet::new();
    };
    let mut intersection: HashSet<u32> = first.iter().copied().collect();
    for posting in postings {
        let set: HashSet<u32> = posting.iter().copied().collect();
        intersection.retain(|index| set.contains(index));
    }
    intersection
}

/// Split a crate name into its lowercased alphanumeric segments
/// (`tokio-postgres` -> `tokio`, `postgres`), dropping single characters. The
/// whole-name prefix is handled by the sorted names table, so only interior
/// segments matter here.
///
/// Dropping 1-char segments is what keeps a single-character *query* cheap and
/// sensible now that there is no length floor: it tokenizes to nothing, so it
/// contributes no interior-token candidates and is answered by the whole-name
/// prefix alone. Admitting 1-char tokens would instead fan `s` out over every
/// name containing an `s`-initial segment, for no gain — a lone character is
/// mid-typing, and its whole-name prefix is the useful reading of it.
pub(super) fn name_tokens(name: &str) -> impl Iterator<Item = String> + '_ {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|segment| segment.len() >= 2)
        .map(str::to_ascii_lowercase)
}
