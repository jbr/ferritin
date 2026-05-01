pub mod indexer;

use crate::{Navigator, navigator::Suggestion};
use rayon::prelude::*;

pub use indexer::*;

impl Navigator {
    /// Search across multiple crates with BM25 scoring
    ///
    /// Returns results sorted by score (descending). Empty crate list returns empty results.
    /// Empty query triggers index loading but returns no matches (useful for prewarming).
    ///
    /// Returns Err with suggestions if no crates could be loaded/indexed.
    pub fn search<'nav, 'query>(
        &'nav self,
        query: &'query str,
        crate_names: &'query [&'query str],
    ) -> Result<Vec<ScoredResult<'query>>, Vec<Suggestion<'nav>>> {
        if crate_names.is_empty() {
            return Ok(vec![]);
        }

        // Load indexes and search in parallel
        let results: Vec<_> = crate_names
            .par_iter()
            .map(|&crate_name| {
                let bare_name = crate_name.split_once('@').map_or(crate_name, |(n, _)| n);
                self.get_or_build_search_index(crate_name)
                    .map(|index| (bare_name, index.search(query)))
            })
            .collect();

        // Separate successes from failures
        let mut crate_results = Vec::new();
        let mut first_error = None;

        for result in results {
            match result {
                Ok(data) => crate_results.push(data),
                Err(suggestions) if first_error.is_none() => first_error = Some(suggestions),
                Err(_) => {}
            }
        }

        // If no crates succeeded, return the first error
        if crate_results.is_empty()
            && let Some(err) = first_error
        {
            return Err(err);
        }

        // Aggregate results with BM25 scoring
        let mut scorer = BM25Scorer::new();
        for (crate_name, results) in crate_results {
            scorer.add(crate_name, results);
        }

        Ok(scorer.score())
    }

    /// Get or build a search index for the given crate
    ///
    /// Returns Err with suggestions if the crate cannot be found
    fn get_or_build_search_index<'nav>(
        &'nav self,
        crate_name: &str,
    ) -> Result<&'nav SearchIndex, Vec<Suggestion<'nav>>> {
        let crate_name = self.canonicalize(crate_name);

        if let Some(cached) = self.search_indexes.get(&crate_name) {
            if let Some(index) = cached.as_ref() {
                return Ok(index);
            } else {
                // Permanent failure cached - return empty suggestions
                return Err(vec![]);
            }
        }

        log::info!("Loading search index for {}", crate_name);

        // Use existing SearchIndex::load_or_build which handles disk caching
        let result = SearchIndex::load_or_build(self, crate_name.as_ref());

        match result {
            Ok(index) => {
                let index_ref = self
                    .search_indexes
                    .insert(crate_name, Box::new(Some(index)))
                    .as_ref()
                    .unwrap();
                Ok(index_ref)
            }
            Err(suggestions) => {
                // Cache the failure
                self.search_indexes.insert(crate_name, Box::new(None));
                Err(suggestions)
            }
        }
    }
}
