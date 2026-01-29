use fieldwork::Fieldwork;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use rustc_hash::FxHashMap;
use rustc_hash::FxHasher;
use rustdoc_types::{Item, ItemEnum, StructKind, Trait};
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

#[derive(Default, Debug, Clone)]
struct Terms<'a> {
    term_docs: BTreeMap<u64, BTreeMap<(u64, u32), f32>>,
    shortest_paths: BTreeMap<(u64, u32), Vec<u32>>,
    crate_hashes: FxHashMap<&'a str, u64>,
}

impl<'a> Terms<'a> {
    fn add(&mut self, word: &str, tf_score: f32, id: (u64, u32)) {
        let term_hash = hash_term(word);
        *self
            .term_docs
            .entry(term_hash)
            .or_default()
            .entry(id)
            .or_default() += tf_score;
    }

    fn finalize(self) -> SearchableTerms {
        let total_docs = self.shortest_paths.len() as f32;
        let mut ids = vec![];

        let mut id_set = BTreeMap::new();

        for (id, id_path) in self.shortest_paths {
            id_set.insert(id, ids.len());
            ids.push(id_path);
        }

        let terms = self
            .term_docs
            .into_iter()
            .map(|(term_hash, doc_scores)| {
                // Calculate IDF for this term
                let doc_freq = doc_scores.len() as f32;
                let idf = (total_docs / doc_freq).ln();

                // Apply TF-IDF scoring
                let mut tf_idf_scores: Vec<_> = doc_scores
                    .into_iter()
                    .filter_map(|(doc_id, tf_score)| {
                        id_set
                            .get(&doc_id)
                            .map(|id| (*id, (1.0 + tf_score.ln()) * idf))
                    })
                    .collect();

                // Sort by TF-IDF score (descending)
                tf_idf_scores.sort_by(|(_, a), (_, b)| b.total_cmp(a));

                (term_hash, tf_idf_scores)
            })
            .collect();

        SearchableTerms { terms, ids }
    }

    fn recurse(&mut self, item: DocRef<'a, Item>, ids: &[u32], add_id: bool) {
        let mut ids = ids.to_owned();
        if add_id {
            ids.push(item.id.0);
        }
        let crate_name = item.crate_docs().name();

        let crate_hash = *self
            .crate_hashes
            .entry(crate_name)
            .or_insert_with(|| hash_term(crate_name));

        let id = (crate_hash, *ids.last().unwrap_or(&item.id.0));

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
        if let Some(name) = item.name() {
            self.add_terms(name, id, 2.0);
        }

        if let Some(docs) = &item.docs {
            self.add_terms(docs, id, 1.0);
        }
    }

    fn add_terms(&mut self, text: &str, id: (u64, u32), base_score: f32) {
        let words = tokenize(text);

        // Count word frequencies in this document
        let mut word_counts: BTreeMap<&str, usize> = BTreeMap::new();
        for word in &words {
            *word_counts.entry(word).or_insert(0) += 1;
        }

        // Add each unique word to the index
        for (word, count) in word_counts {
            // Simple relevance scoring: term frequency / document length * base score
            let tf_score = (count as f32) * base_score;

            self.add(word, tf_score, id);
        }
    }
}

#[derive(Debug, Clone, Archive, RkyvSerialize, RkyvDeserialize, Fieldwork)]
struct SearchableTerms {
    terms: BTreeMap<u64, Vec<(usize, f32)>>,
    ids: Vec<Vec<u32>>,
}

/// A search index for a single crate
#[derive(Debug, Clone, Fieldwork)]
pub struct SearchIndex {
    #[field(get)]
    crate_name: String,
    terms: SearchableTerms,
}

impl SearchableTerms {
    fn search(&self, term: &str) -> impl Iterator<Item = (&[u32], f32)> {
        let mut results = BTreeMap::<usize, f32>::new();
        for term in tokenize(term)
            .into_iter()
            .map(hash_term)
            .filter_map(|term| self.terms.get(&term))
        {
            for (id, score) in term {
                *results.entry(*id).or_default() += score;
            }
        }

        results
            .into_iter()
            .filter_map(|(id, score)| self.ids.get(id).map(|id| (&id[..], score)))
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
            Ok(Self { crate_name, terms })
        } else {
            let mut terms = Terms::default();
            terms.recurse(item, &[], false);
            let terms = terms.finalize();
            Self::store(&terms, &path);
            Ok(Self { terms, crate_name })
        }
    }

    fn store(terms: &SearchableTerms, path: &Path) {
        if let Ok(mut file) = OpenOptions::new().create_new(true).write(true).open(path) {
            match rkyv::to_bytes::<rkyv::rancor::Error>(terms) {
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
            match rkyv::from_bytes::<SearchableTerms, rkyv::rancor::Error>(&bytes) {
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

    // /// Build a search index from rustdoc data
    // pub(crate) fn build<'a>(
    //     request: &'a Request,
    //     crate_name: &str,
    // ) -> Result<Self, Vec<Suggestion<'a>>> {
    //     let mut terms = Terms::default();
    //     let mut suggestions = vec![];

    //     let item = request
    //         .resolve_path(crate_name, &mut suggestions)
    //         .ok_or(suggestions)?;

    //     let crate_name = item.crate_docs().name().to_string();
    //     terms.recurse(item, &[], false);

    //     let terms = terms.finalize();

    //     Ok(Self { terms, crate_name })
    // }

    /// Search for items containing the given term
    pub fn search(&self, term: &str) -> impl Iterator<Item = (&[u32], f32)> {
        self.terms.search(term)
    }
}

fn add_token<'a>(token: &'a str, tokens: &mut Vec<&'a str>) {
    if let Some(token) = token.strip_suffix('s') {
        tokens.push(token);
    } else {
        tokens.push(token);
    }
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

/// Hash a term for use as a map key
fn hash_term(term: &str) -> u64 {
    let mut hasher = FxHasher::default();
    term.to_lowercase().hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize() {
        assert_eq!(
            tokenize("Hello, worlds! This is a test. CamelCases hyphenate-words snake_words"),
            vec![
                "Hello",
                "world",
                "Thi",
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
