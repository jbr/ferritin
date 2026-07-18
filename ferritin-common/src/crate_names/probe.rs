//! A point-probe harness for the typeahead scoring weights, not a regression
//! test: it loads the real on-disk crate-names artifacts and prints ranked
//! results for a battery of queries under several [`TypeaheadWeights`]
//! settings, for eyeballing how the parameter space behaves.
//!
//! Run with:
//! `cargo test -p ferritin-common --lib crate_names::probe -- --ignored --nocapture`

use super::*;

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
