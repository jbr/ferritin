use fieldwork::Fieldwork;
use rkyv::rancor::Error;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use rustc_hash::FxHashMap;
use rustc_hash::FxHasher;
use rustdoc_types::{Item, ItemEnum, StructKind, Trait};
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fs::File;
use std::fs::OpenOptions;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::Path;
use std::time::SystemTime;

use crate::{
    doc_ref::DocRef,
    navigator::{Navigator, Suggestion},
};
use std::collections::HashMap;

// Newtypes for clarity
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[rkyv(derive(PartialEq, Eq, PartialOrd, Ord))]
#[repr(transparent)]
struct TermHash(u64);

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
)]
#[repr(transparent)]
struct DocumentId(usize);

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[repr(transparent)]
struct DocumentTermCount(usize);

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Archive, RkyvSerialize, RkyvDeserialize,
)]
#[repr(transparent)]
struct DocumentLength(usize);

#[derive(Debug, Clone, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
struct ItemPath(Vec<u32>);

#[derive(Debug, Clone, Copy, Archive, RkyvSerialize, RkyvDeserialize)]
struct Posting {
    document: DocumentId,
    count: DocumentTermCount,
}

#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize)]
struct DocumentInfo {
    path: ItemPath,
    length: DocumentLength,
}

#[derive(Default, Debug, Clone)]
struct Terms<'a> {
    term_docs: BTreeMap<TermHash, BTreeMap<(u64, u32), DocumentTermCount>>,
    shortest_paths: BTreeMap<(u64, u32), Vec<u32>>,
    document_lengths: BTreeMap<(u64, u32), DocumentLength>,
    crate_hashes: FxHashMap<&'a str, TermHash>,
}

impl<'a> Terms<'a> {
    fn add(&mut self, word: &str, count: DocumentTermCount, id: (u64, u32)) {
        let term_hash = hash_term(word);
        let entry = self
            .term_docs
            .entry(term_hash)
            .or_default()
            .entry(id)
            .or_insert(DocumentTermCount(0));
        entry.0 += count.0;
    }

    fn finalize(self) -> SearchableTerms {
        let mut documents = vec![];
        let mut id_set = BTreeMap::new();
        let mut total_document_length = 0;

        for (id, id_path) in self.shortest_paths {
            let doc_length = self
                .document_lengths
                .get(&id)
                .copied()
                .unwrap_or(DocumentLength(0));
            total_document_length += doc_length.0;
            id_set.insert(id, documents.len());
            documents.push(DocumentInfo {
                path: ItemPath(id_path),
                length: doc_length,
            });
        }

        let terms = self
            .term_docs
            .into_iter()
            .map(|(term_hash, doc_counts)| {
                // Store raw counts, not TF-IDF
                let mut postings: Vec<_> = doc_counts
                    .into_iter()
                    .filter_map(|(doc_id, count)| {
                        id_set.get(&doc_id).map(|&id| Posting {
                            document: DocumentId(id),
                            count,
                        })
                    })
                    .collect();

                // Sort by count (descending) for faster retrieval of top results
                postings.sort_by_key(|b| Reverse(b.count.0));

                (term_hash, postings)
            })
            .collect();

        SearchableTerms {
            terms,
            documents,
            total_document_length,
        }
    }

    fn recurse(&mut self, item: DocRef<'a, Item>, ids: &[u32], add_id: bool) {
        let mut ids = ids.to_owned();
        if add_id {
            ids.push(item.id.0);
        }
        let crate_name = item.crate_docs().name();

        let crate_hash = self
            .crate_hashes
            .entry(crate_name)
            .or_insert_with(|| hash_term(crate_name));

        let id = (crate_hash.0, *ids.last().unwrap_or(&item.id.0));

        if let Some(existing_path) = self.shortest_paths.get_mut(&id) {
            if ids.len() < existing_path.len() {
                *existing_path = ids;
            }
            return;
        }

        self.add_for_item(item, id);

        match item.inner() {
            ItemEnum::Struct(struct_item) => match &struct_item.kind {
                StructKind::Unit => {}
                StructKind::Tuple(field_ids) => {
                    for field in field_ids.iter().flatten().filter_map(|id| item.get(id)) {
                        self.add_for_item(field, id);
                    }
                }
                StructKind::Plain { fields, .. } => {
                    for field in item.id_iter(fields) {
                        self.add_for_item(field, id);
                    }
                }
            },
            ItemEnum::Trait(Trait { items, .. }) => {
                for field in item.id_iter(items) {
                    self.recurse(field, &ids, false);
                }
            }
            _ => {}
        };

        for child in item.child_items().with_use() {
            self.recurse(child, &ids, true)
        }

        self.shortest_paths.insert(id, ids);
    }

    fn add_for_item(&mut self, item: DocRef<'a, Item>, id: (u64, u32)) {
        let mut doc_length = 0;

        // Item name gets very high weight - when someone searches for "vec",
        // they almost certainly want the Vec struct, not its methods
        if let Some(name) = item.name() {
            doc_length += self.add_terms(name, id, 20);
        }

        if let Some(docs) = &item.docs {
            // First paragraph (up to first blank line) gets extra weight
            // as it's usually the item's summary/description
            if let Some((first_para, rest)) = docs.split_once("\n\n") {
                doc_length += self.add_terms(first_para, id, 3);
                doc_length += self.add_terms(rest, id, 1);
            } else {
                doc_length += self.add_terms(docs, id, 3);
            }
        }

        self.document_lengths.insert(id, DocumentLength(doc_length));
    }

    fn add_terms(&mut self, text: &str, id: (u64, u32), weight: usize) -> usize {
        let words = tokenize(text);
        let doc_length = words.len();

        // Count word frequencies in this document
        let mut word_counts: BTreeMap<&str, usize> = BTreeMap::new();
        for word in &words {
            *word_counts.entry(word).or_insert(0) += 1;
        }

        // Add each unique word to the index with weighted count
        for (word, count) in word_counts {
            let weighted_count = count * weight;
            self.add(word, DocumentTermCount(weighted_count), id);
        }

        doc_length
    }
}

#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize)]
struct SearchableTerms {
    terms: BTreeMap<TermHash, Vec<Posting>>,
    documents: Vec<DocumentInfo>,
    total_document_length: usize,
}

/// A search index for a single crate
#[derive(Debug, Clone, Fieldwork)]
pub struct SearchIndex {
    #[field(get)]
    crate_name: String,
    terms: SearchableTerms,
}

impl SearchableTerms {
    fn search<'a>(&self, query: &'a str) -> SearchResults<'a> {
        let tokens = tokenize(query);

        // Build lookup from hash to original token
        let token_map: HashMap<TermHash, &'a str> = tokens
            .iter()
            .map(|&token| (hash_term(token), token))
            .collect();

        // Collect posting lists for each query term
        let mut term_postings: HashMap<TermHash, &Vec<Posting>> = HashMap::new();
        for &token in &tokens {
            let term_hash = hash_term(token);
            if let Some(postings) = self.terms.get(&term_hash) {
                term_postings.insert(term_hash, postings);
            }
        }

        // Build document frequency map (in borrowed strings for public API)
        let term_doc_freqs: HashMap<&'a str, usize> = term_postings
            .iter()
            .map(|(term_hash, postings)| {
                let term_str = token_map.get(term_hash).unwrap();
                (*term_str, postings.len())
            })
            .collect();

        // Collect all matching documents and aggregate term counts
        let mut doc_term_counts: BTreeMap<DocumentId, HashMap<&'a str, usize>> = BTreeMap::new();
        for (term_hash, postings) in term_postings {
            let term_str = token_map.get(&term_hash).unwrap();
            for posting in postings.iter() {
                doc_term_counts
                    .entry(posting.document)
                    .or_default()
                    .insert(term_str, posting.count.0);
            }
        }

        // Convert to results vec
        let results: Vec<SearchResult<'a>> = doc_term_counts
            .into_iter()
            .filter_map(|(doc_id, term_counts)| {
                self.documents.get(doc_id.0).map(|doc_info| SearchResult {
                    id_path: doc_info.path.0.clone(),
                    doc_length: doc_info.length.0,
                    term_counts,
                })
            })
            .collect();

        SearchResults {
            total_docs: self.documents.len(),
            total_doc_length: self.total_document_length,
            term_doc_freqs,
            results,
        }
    }
}

impl SearchIndex {
    pub fn load_or_build<'a>(
        navigator: &'a Navigator,
        crate_name: &str,
    ) -> Result<Self, Vec<Suggestion<'a>>> {
        let mut suggestions = vec![];

        let item = navigator
            .resolve_path(crate_name, &mut suggestions)
            .ok_or(suggestions)?;

        let crate_docs = item.crate_docs();
        let crate_name = crate_docs.name().to_string();

        let mtime = crate_docs
            .fs_path()
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok());

        let mut path = crate_docs.fs_path().to_path_buf();
        path.set_extension("index");

        if let Some(terms) = Self::load(&path, mtime) {
            log::debug!("Loaded cached index from disk for {crate_name}");
            Ok(Self { crate_name, terms })
        } else {
            log::debug!("Building new index for {crate_name}");
            let mut terms = Terms::default();
            terms.recurse(item, &[], false);
            let terms = terms.finalize();
            log::debug!("Finished building index for {crate_name}");
            Self::store(&terms, &path);
            Ok(Self { terms, crate_name })
        }
    }

    fn store(terms: &SearchableTerms, path: &Path) {
        if let Ok(mut file) = OpenOptions::new().create_new(true).write(true).open(path) {
            match rkyv::to_bytes::<Error>(terms) {
                Ok(bytes) => {
                    if file.write_all(&bytes).is_err() {
                        let _ = std::fs::remove_file(path);
                    }
                }
                Err(_) => {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }

    fn load(path: &Path, mtime: Option<SystemTime>) -> Option<SearchableTerms> {
        let mut file = File::open(path).ok()?;
        let index_mtime = file.metadata().ok().and_then(|m| m.modified().ok())?;

        let mtime = mtime?;
        if index_mtime.duration_since(mtime).is_ok() {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).ok()?;
            match rkyv::from_bytes::<SearchableTerms, Error>(&bytes) {
                Ok(terms) => Some(terms),
                Err(_) => {
                    let _ = std::fs::remove_file(path);
                    None
                }
            }
        } else {
            let _ = std::fs::remove_file(path);
            None
        }
    }

    pub fn len(&self) -> usize {
        self.terms.documents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.terms.documents.is_empty()
    }

    /// Search for items containing the given term
    /// Returns components needed for BM25 scoring across multiple crates
    pub fn search<'a>(&self, query: &'a str) -> SearchResults<'a> {
        self.terms.search(query)
    }
}

// Public API types for BM25 scoring

/// Results from searching a single crate
pub struct SearchResults<'a> {
    /// Total number of documents in this crate's index
    pub total_docs: usize,
    /// Sum of all document lengths (for calculating average)
    pub total_doc_length: usize,
    /// How many documents contain each query term
    pub term_doc_freqs: HashMap<&'a str, usize>,
    /// Matching documents with their term counts
    pub results: Vec<SearchResult<'a>>,
}

/// A single document that matches the search query
pub struct SearchResult<'a> {
    /// Path to the item (rustdoc IDs)
    pub id_path: Vec<u32>,
    /// Length of this document in tokens
    pub doc_length: usize,
    /// Which query terms matched and their weighted counts
    pub term_counts: HashMap<&'a str, usize>,
}

/// A scored search result from BM25 scoring
pub struct ScoredResult<'a> {
    /// Which crate this result is from
    pub crate_name: &'a str,
    /// Path to the item (rustdoc IDs)
    pub id_path: Vec<u32>,
    /// BM25 score
    pub score: f32,
}

/// BM25 scorer for combining results from multiple crates
pub struct BM25Scorer<'a> {
    k1: f32,
    b: f32,
    crate_results: Vec<(&'a str, SearchResults<'a>)>,
}

impl<'a> BM25Scorer<'a> {
    /// Create a new BM25 scorer with default parameters
    pub fn new() -> Self {
        Self {
            // k1 controls term frequency saturation (1.2 is standard)
            k1: 1.2,
            // b controls document length normalization
            // Set to 0 to disable length penalty entirely.
            // In documentation, longer documents (like Vec's comprehensive docs)
            // are often MORE relevant than short focused docs (like methods).
            b: 0.0,
            crate_results: Vec::new(),
        }
    }

    /// Add search results from a crate
    pub fn add(&mut self, crate_name: &'a str, results: SearchResults<'a>) {
        self.crate_results.push((crate_name, results));
    }

    /// Compute BM25 scores for all results and return them sorted by score
    pub fn score(self) -> Vec<ScoredResult<'a>> {
        log::debug!("Computing global statistics");

        // Aggregate global statistics
        let global_total_docs: usize = self.crate_results.iter().map(|(_, r)| r.total_docs).sum();
        let global_total_length: usize = self
            .crate_results
            .iter()
            .map(|(_, r)| r.total_doc_length)
            .sum();

        if global_total_docs == 0 {
            return vec![];
        }

        let avgdl = global_total_length as f32 / global_total_docs as f32;

        // Aggregate document frequencies across all crates
        let mut global_term_doc_freqs: HashMap<&str, usize> = HashMap::new();
        for (_, results) in &self.crate_results {
            for (term, doc_freq) in &results.term_doc_freqs {
                *global_term_doc_freqs.entry(term).or_default() += doc_freq;
            }
        }

        log::debug!(
            "Computing global IDF for {} terms",
            global_term_doc_freqs.len()
        );

        // Calculate global IDF for each term
        let global_idf: HashMap<&str, f32> = global_term_doc_freqs
            .iter()
            .map(|(term, doc_freq)| {
                // BM25 IDF formula
                let idf = ((global_total_docs as f32 - *doc_freq as f32 + 0.5)
                    / (*doc_freq as f32 + 0.5))
                    .ln();
                (*term, idf)
            })
            .collect();

        // Count total results to score
        let total_results: usize = self
            .crate_results
            .iter()
            .map(|(_, r)| r.results.len())
            .sum();
        log::debug!("Scoring {} results", total_results);

        // Score all results
        let mut scored: Vec<ScoredResult<'a>> = Vec::new();
        for (crate_name, results) in self.crate_results {
            for result in results.results {
                let doc_len_norm = result.doc_length as f32 / avgdl;

                let score: f32 = result
                    .term_counts
                    .iter()
                    .map(|(term, count)| {
                        let idf = global_idf.get(term).copied().unwrap_or(0.0);
                        let tf = *count as f32;
                        let numerator = tf * (self.k1 + 1.0);
                        let denominator = tf + self.k1 * (1.0 - self.b + self.b * doc_len_norm);
                        idf * (numerator / denominator)
                    })
                    .sum();

                scored.push(ScoredResult {
                    crate_name,
                    id_path: result.id_path,
                    score,
                });
            }
        }

        log::debug!("Sorting {} scored results", scored.len());

        // Sort by score descending
        scored.sort_by(|a, b| b.score.total_cmp(&a.score));

        scored
    }
}

impl<'a> Default for BM25Scorer<'a> {
    fn default() -> Self {
        Self::new()
    }
}

fn add_token<'a>(token: &'a str, tokens: &mut Vec<&'a str>) {
    tokens.push(token);
}

/// Simple tokenizer: split on whitespace and punctuation, lowercase, filter short words
fn tokenize(text: &str) -> Vec<&str> {
    let mut tokens = vec![];
    let min_chars = 2;
    let mut last_case = None;
    let mut word_start = 0;
    let mut subword_start = 0;
    let mut word_start_next_char = true;
    let mut subword_start_next_char = true;

    for (i, c) in text.char_indices() {
        if word_start_next_char {
            word_start = i;
            subword_start = i;
            word_start_next_char = false;
            subword_start_next_char = false;
        }

        if subword_start_next_char {
            subword_start = i;
            subword_start_next_char = false;
        }

        let current_case = c.is_alphabetic().then(|| c.is_uppercase());
        let case_change = last_case == Some(false) && current_case == Some(true);
        last_case = current_case;

        if c == '-' || c == '_' {
            if i.saturating_sub(subword_start) > min_chars {
                add_token(&text[subword_start..i], &mut tokens);
            }
            subword_start_next_char = true;
        } else if !c.is_alphabetic() {
            if i.saturating_sub(subword_start) > min_chars && subword_start != word_start {
                add_token(&text[subword_start..i], &mut tokens);
            }
            if i.saturating_sub(word_start) > min_chars {
                add_token(&text[word_start..i], &mut tokens);
            }
            word_start_next_char = true;
        } else if case_change {
            if i.saturating_sub(subword_start) > min_chars {
                add_token(&text[subword_start..i], &mut tokens);
            }
            subword_start = i;
        }
    }

    if !word_start_next_char {
        let last_subword = &text[subword_start..];

        if word_start != subword_start && last_subword.len() > min_chars {
            add_token(last_subword, &mut tokens);
        }

        let last_word = &text[word_start..];
        if last_word.len() > min_chars {
            add_token(last_word, &mut tokens);
        }
    }

    tokens
}

/// Hash a term for use as a map key (case-insensitive)
fn hash_term(term: &str) -> TermHash {
    let mut hasher = FxHasher::default();
    // Hash lowercased chars without allocating
    for c in term.chars() {
        for lower_c in c.to_lowercase() {
            lower_c.hash(&mut hasher);
        }
    }
    TermHash(hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize() {
        assert_eq!(
            tokenize("Hello, world! This is a test. CamelCase hyphenate-word snake_word"),
            vec![
                "Hello",
                "world",
                "This",
                "test",
                "Camel",
                "Case",
                "CamelCase",
                "hyphenate",
                "word",
                "hyphenate-word",
                "snake",
                "word",
                "snake_word"
            ]
        );
    }

    #[test]
    fn test_hash_term() {
        // Should be case insensitive
        assert_eq!(hash_term("Hello"), hash_term("HELLO"));
        assert_eq!(hash_term("Hello"), hash_term("hello"));
    }
}
