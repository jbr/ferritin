//! A point-probe harness for the typeahead scoring weights, not a regression
//! test: it loads the real on-disk crate-names artifacts and prints ranked
//! results for a battery of queries under several [`TypeaheadWeights`]
//! settings, for eyeballing how the parameter space behaves.
//!
//! Run with:
//! `cargo test -p ferritin-common --lib crate_names::probe -- --ignored --nocapture`

use super::*;
use crate::string_utils::stem;
use std::collections::HashMap;

/// Load the artifacts the CLI/server already cached on this machine. Panics
/// with a hint if they are absent — the probe is only meaningful over real
/// data.
fn load() -> Loaded {
    let dir = crate::ferritin_home::resolve()
        .expect("ferritin home")
        .join("crate-names");
    let read = |file: &str| {
        std::fs::read(dir.join(file)).unwrap_or_else(|e| {
            panic!(
                "{file} not found in {} ({e}); run a typeahead or version resolution once to \
                 populate it",
                dir.display()
            )
        })
    };
    parse(
        &read(crate_names::NAMES_FILE_V2),
        &read(crate_names::DESCRIPTIONS_FILE_V2),
        &read(crate_names::FACETS_FILE_V1),
        Etags::default(),
        None,
    )
    .expect("parsing cached artifacts")
}

fn show(loaded: &Loaded, query: &str, weights: &TypeaheadWeights, limit: usize) -> String {
    let (entries, total) = typeahead_scored(loaded, query, limit, weights);
    let names: Vec<String> = entries
        .iter()
        .map(|e| format!("{}({})", e.name, e.rank))
        .collect();
    format!(
        "{:>28} ({total:>6}) {}",
        format!("{query:?}"),
        names.join(" ")
    )
}

/// Queries that name a *concept* rather than a crate — the case description
/// matching exists for. None of these are answerable from crate names alone.
pub(super) const CONCEPT_QUERIES: [&str; 12] = [
    "deserialize",
    "deserialization",
    "terminal colors",
    "date parsing",
    "connection pool",
    "command line arguments",
    "template engine",
    "image resizing",
    "websocket client",
    "random number generator",
    "error handling",
    "zero copy",
];

/// Where the per-query time in a description match actually goes: the prefix
/// walk over the stem dictionary, or the size of the candidate set it yields.
/// The answer decides whether a prebuilt artifact would help query latency or
/// only build cost.
#[test]
#[ignore]
fn description_cost_probe() {
    let loaded = load();
    let index = DescriptionIndex::build(&loaded.names, &loaded.descriptions);

    // Stage timings for a whole query, to locate the rest of the wall clock.
    let token_index = loaded
        .token_index
        .get_or_init(|| TokenIndex::build(&loaded.names));
    for query in ["random number generator", "date parsing", "serde"] {
        let mut query_tokens: Vec<String> = name_tokens(query).collect();
        query_tokens.sort_unstable();
        query_tokens.dedup();

        let start = Instant::now();
        let counts = match_counts(
            &query_tokens,
            token_index,
            Some(&index),
            None,
            crate::search::QueryCompletion::AsYouType,
        );
        let counts_time = start.elapsed();

        let start = Instant::now();
        let exact = all_exact_indices(&query_tokens, token_index);
        let exact_time = start.elapsed();

        let start = Instant::now();
        let scored: Vec<(f32, u32)> = counts
            .iter()
            .filter_map(|(&i, m)| {
                let found = loaded.names.entry_at(i as usize)?;
                Some((128.0 * m.name as f32 + f32::from(found.rank), i))
            })
            .collect();
        let score_time = start.elapsed();

        let start = Instant::now();
        let mut sorted = scored.clone();
        sorted.sort_by(|a, b| {
            b.0.total_cmp(&a.0).then_with(|| {
                let a_name = loaded.names.entry_at(a.1 as usize).map(|e| e.name);
                let b_name = loaded.names.entry_at(b.1 as usize).map(|e| e.name);
                a_name.cmp(&b_name)
            })
        });
        let sort_time = start.elapsed();

        println!(
            "{query:>26}: {} candidates | counts {counts_time:>10?} | all_exact {exact_time:>10?} \
             | score {score_time:>10?} | sort {sort_time:>10?} | exact_set {}",
            counts.len(),
            exact.len()
        );
    }

    for query in [
        "number",
        "date",
        "generator",
        "websocket",
        "deserialization",
    ] {
        let stemmed = stem(query).into_owned();

        let start = Instant::now();
        let prefixed = index.indices_with_prefix(&stemmed);
        let prefix_time = start.elapsed();

        // What the candidate map costs once the postings are in hand.
        let start = Instant::now();
        let mut counts: HashMap<u32, u32> = HashMap::new();
        for &crate_index in &prefixed {
            *counts.entry(crate_index).or_insert(0) += 1;
        }
        let map_time = start.elapsed();

        println!(
            "{query:>16} -> {stemmed:<12} prefix {:>7} in {prefix_time:>10?} | map {map_time:>10?}",
            prefixed.len()
        );
    }
}

/// What description matching costs and what it changes. Prints the index build
/// time, then every probe query with the feature off and on.
#[test]
#[ignore]
fn description_probe() {
    let loaded = load();

    let start = Instant::now();
    let index = DescriptionIndex::build(&loaded.names, &loaded.descriptions);
    println!(
        "built description index: {} stems in {:?}",
        index.postings.len(),
        start.elapsed()
    );
    let _ = loaded.description_index.set(index);

    let off = TypeaheadWeights {
        description_match: 0.0,
        ..TypeaheadWeights::default()
    };
    for weights in [
        off,
        TypeaheadWeights::default(),
        TypeaheadWeights {
            description_match: 96.0,
            ..TypeaheadWeights::default()
        },
    ] {
        println!("\n=== description_match = {}", weights.description_match);
        for query in CONCEPT_QUERIES.iter().chain(&[
            "postgres",
            "serde",
            "tokio",
            "http",
            "json",
            "se",
            "toki",
            "trillium router",
            "serde json",
        ]) {
            let start = Instant::now();
            let line = show(&loaded, query, &weights, 8);
            println!("{line} [{:?}]", start.elapsed());
        }
    }
}

#[test]
#[ignore]
fn typeahead_probe() {
    let loaded = load();

    let queries = [
        // single-term: prefix vs interior-token balance
        "postgres",
        "serde",
        "tokio",
        "http",
        "json",
        "async",
        "router",
        "regex",
        "web",
        // as-you-type partials
        "se",
        "toki",
        "postg",
        "rout",
        // multi-term: full-match vs subset-match balance
        "trillium router",
        "trillium tokio",
        "async postgres",
        "serde json",
        "tokio util",
        "http client",
        "web socket",
        "derive builder",
        "actix web middleware",
    ];

    let weight_settings = [
        (
            "term=64",
            TypeaheadWeights {
                term_match: 64.0,
                ..TypeaheadWeights::default()
            },
        ),
        (
            "term=96",
            TypeaheadWeights {
                term_match: 96.0,
                ..TypeaheadWeights::default()
            },
        ),
        ("term=128 (default)", TypeaheadWeights::default()),
        (
            "exact=0 (no all-terms-exact bonus)",
            TypeaheadWeights {
                all_exact: 0.0,
                ..TypeaheadWeights::default()
            },
        ),
        (
            "exact=48",
            TypeaheadWeights {
                all_exact: 48.0,
                ..TypeaheadWeights::default()
            },
        ),
        (
            "term=192",
            TypeaheadWeights {
                term_match: 192.0,
                ..TypeaheadWeights::default()
            },
        ),
        (
            "term=4096 (lexicographic: matched count strictly first)",
            TypeaheadWeights {
                term_match: 4096.0,
                ..TypeaheadWeights::default()
            },
        ),
    ];

    for (label, weights) in &weight_settings {
        println!("\n=== {label}: {weights:?}");
        for query in &queries {
            println!("{}", show(&loaded, query, weights, 8));
        }
    }
}
