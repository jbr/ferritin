pub mod indexer;

use crate::{Navigator, Suggestion};
pub use indexer::*;
use rayon::prelude::*;

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
        completion: QueryCompletion,
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
                    .map(|index| (bare_name, index.search(query, completion)))
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

    /// Get or build a search index for the given crate spec (a crate name
    /// with an optional `@version-req` suffix).
    ///
    /// Returns Err with suggestions if the crate cannot be found.
    ///
    /// The index comes from the Store's search cache, which is keyed by exact
    /// version — only a load can determine that, so the crate is loaded (or
    /// its pin hit) first. The index is pinned in this Navigator, keyed by
    /// bare name: like crate data, one search index per name per query.
    /// Suggestions borrow this Navigator, so they can't live in the shared
    /// slot: only the caller whose closure ran the failing build gets them —
    /// a caller hitting a cached failure gets an empty list, as cached
    /// failures always did.
    fn get_or_build_search_index<'nav>(
        &'nav self,
        crate_spec: &str,
    ) -> Result<&'nav SearchIndex, Vec<Suggestion<'nav>>> {
        use crate::store::LoadFailure;
        use semver::VersionReq;
        use std::sync::Arc;

        let (bare_name, version_req) = match crate_spec.find('@') {
            Some(at) => (
                &crate_spec[..at],
                VersionReq::parse(&crate_spec[at + 1..]).unwrap_or(VersionReq::STAR),
            ),
            None => (crate_spec, VersionReq::STAR),
        };
        let crate_name = self.canonicalize(bare_name);

        if let Some(index) = self.search_indexes.get(&crate_name) {
            return Ok(index);
        }

        if self.load_crate(bare_name, &version_req).is_none() {
            // Same suggestions a failed path resolution produces; the crate
            // cache has already recorded why the load failed.
            return Err(Suggestion::for_crate_name(self, bare_name));
        }
        let version = self
            .pinned_version(&crate_name)
            .expect("load_crate records the version of every pin")
            .clone();

        let mut fresh_suggestions = None;
        let result = self
            .store()
            .search_index(&(crate_name.clone(), version), || {
                log::info!("Loading search index for {}", crate_name);
                // Use existing SearchIndex::load_or_build which handles disk caching
                match SearchIndex::load_or_build(self, crate_spec) {
                    Ok(index) => Ok(Arc::new(index)),
                    Err(suggestions) => {
                        fresh_suggestions = Some(suggestions);
                        // A failed build means the crate didn't resolve; the crate
                        // cache already remembers that with its own kind-specific
                        // TTL, so keep this one short and let retries stay cheap.
                        Err(LoadFailure::Transient)
                    }
                }
            });

        match result {
            Ok(index) => Ok(self.search_indexes.insert(crate_name, index)),
            Err(_) => Err(fresh_suggestions.unwrap_or_default()),
        }
    }
}
