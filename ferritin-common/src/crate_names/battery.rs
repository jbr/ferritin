//! An asserted regression battery for typeahead scoring — the ratchet that
//! keeps settled ranking behavior settled while weights are tuned.
//!
//! Unlike [`probe`](super::probe), which eyeballs the live cache, this runs
//! against a *pinned* snapshot of the crate-names artifacts (a dated release
//! on the crate-names repo, sha256s checked in here), so its assertions are
//! deterministic: a failure means the *code* changed behavior, never that
//! crates.io drifted. Assertions are deliberately coarse — top-N membership
//! and relative order, not exact pages — so they survive weight adjustments
//! that don't reverse a settled question.
//!
//! The fixtures download once into `tests/fixtures/` (gitignored) and are
//! verified against their pinned hashes on every read, so a clobbered or
//! truncated cache file is re-fetched rather than trusted.

use super::*;
use sha2::{Digest, Sha256};
use std::{fs, sync::OnceLock as StdOnceLock};

/// The dated snapshot release these tests pin against. Snapshot releases are
/// immutable by convention (unlike the daily-clobbered `artifacts` release);
/// re-pinning to a newer build means updating the tag, the hashes, and
/// whatever assertions the new data breaks — a deliberate act.
const FIXTURES_TAG: &str = "fixtures-2026-07-30";

/// Every artifact in the pinned snapshot, with its sha256. The facets
/// artifact is pinned and cached from day one even though the battery only
/// reads it once the keywords index exists, so the snapshot stays one
/// coherent triple.
const FIXTURES: [(&str, &str); 3] = [
    (
        crate_names::NAMES_FILE_V2,
        "e8800ae219ae261e0750bb15c73a2d618d017f204cc6a2e37465678175f633a6",
    ),
    (
        crate_names::DESCRIPTIONS_FILE_V2,
        "94c4135fa8d4ac5dcb4b997f18e3c7cae394eda14c73c376200a75a32e77bcf3",
    ),
    (
        crate_names::FACETS_FILE_V1,
        "552f1b48499991b41e78c239fc0fa4d2f6c84771a6a015a2ddfe57a7302eebf7",
    ),
];

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(FIXTURES_TAG)
}

/// One blocking GET, following the release-asset redirect.
fn fetch(url: &str) -> Vec<u8> {
    trillium_smol::async_global_executor::block_on(async {
        let client = Client::new(RustlsConfig::<ClientConfig>::default())
            .with_handler(FollowRedirects::new())
            .with_timeout(Duration::from_secs(60))
            .with_default_header(KnownHeaderName::UserAgent, crate::FERRITIN_USER_AGENT);
        let mut conn = client
            .get(url)
            .await
            .unwrap_or_else(|error| panic!("fetching {url}: {error}"))
            .success()
            .unwrap_or_else(|error| panic!("fetching {url}: {error}"));
        conn.response_body()
            .read_bytes()
            .await
            .unwrap_or_else(|error| panic!("reading {url}: {error}"))
    })
}

/// The verified bytes of one pinned fixture: from the local cache when its
/// hash checks out, re-downloaded otherwise. A hash mismatch on a fresh
/// download is fatal — either the snapshot was tampered with or the pin here
/// is wrong, and neither should be papered over.
fn fixture_bytes(name: &str, sha256: &str) -> Vec<u8> {
    let path = fixtures_dir().join(name);
    if let Ok(bytes) = fs::read(&path)
        && sha256_hex(&bytes) == sha256
    {
        return bytes;
    }

    let url = format!("https://github.com/jbr/crate-names/releases/download/{FIXTURES_TAG}/{name}");
    let bytes = fetch(&url);
    let actual = sha256_hex(&bytes);
    assert_eq!(
        actual, sha256,
        "{url} does not match its pinned sha256 — snapshot altered, or the pin needs updating"
    );
    fs::create_dir_all(fixtures_dir()).unwrap();
    fs::write(&path, &bytes).unwrap();
    bytes
}

/// The pinned artifacts, parsed once and shared by every test in the process.
/// The description and keyword indexes are built as part of initialization
/// (they are what most of the battery exercises), so individual tests never
/// race to build them.
fn loaded() -> &'static Loaded {
    static LOADED: StdOnceLock<Loaded> = StdOnceLock::new();
    LOADED.get_or_init(|| {
        let [names, descriptions, facets] =
            FIXTURES.map(|(name, sha256)| fixture_bytes(name, sha256));
        let loaded = parse(&names, &descriptions, &facets, Etags::default(), None)
            .expect("parsing pinned artifacts");
        loaded.description_index();
        loaded.keyword_index();
        loaded
    })
}

/// The ranked page for `query` under the given weights, as names.
fn page(query: &str, weights: &TypeaheadWeights, limit: usize) -> Vec<String> {
    let (entries, _) = typeahead_scored(loaded(), query, limit, weights);
    entries.into_iter().map(|entry| entry.name).collect()
}

/// The ranked page under the shipped defaults (description matching on, as
/// the server runs it).
fn top(query: &str, limit: usize) -> Vec<String> {
    page(query, &TypeaheadWeights::default(), limit)
}

#[track_caller]
fn assert_top_contains(query: &str, limit: usize, expected: &str) {
    let names = top(query, limit);
    assert!(
        names.iter().any(|name| name == expected),
        "{query:?}: expected {expected} in top {limit}, got {names:?}"
    );
}

#[track_caller]
fn assert_precedes(query: &str, first: &str, second: &str) {
    // A page big enough that both names must appear if they rank at all
    // comparably; the assertion is about their relative order.
    let names = top(query, 50);
    let position = |needle: &str| names.iter().position(|name| name == needle);
    let (Some(first_at), Some(second_at)) = (position(first), position(second)) else {
        panic!("{query:?}: expected both {first} and {second} in top 50, got {names:?}");
    };
    assert!(
        first_at < second_at,
        "{query:?}: expected {first} (at {first_at}) before {second} (at {second_at})"
    );
}

/// Concept queries — prose describing a capability, never naming a crate —
/// must surface a crate a person would call a canonical answer. This is the
/// case description matching exists for. Only settled wins are pinned;
/// each expectation is a crate whose loss from the top 8 would mean a real
/// regression, not a page snapshot.
///
/// Known gaps, observed against this snapshot and deliberately *not* pinned
/// (pinning today's misses would ratchet the wrong thing):
/// - `date parsing`: chrono/time/jiff never appear — their descriptions don't say "parsing" and
///   their keywords (`date`, `time`) duplicate tokens the description already matched, so the max
///   rule adds nothing.
/// - `websocket client`: tungstenite/tokio-tungstenite never appear — the description says
///   "WebSocket implementation", not "client", and "client" is not among the declared keywords
///   either.
/// - `zero copy`: rkyv ("Zero-copy deserialization framework") sits just below the top 8 behind
///   equally-matched, more-downloaded crates.
/// - `cli` → clap is *typeahead*-unreachable by design: `cli` is a name prefix of `client`, so
///   client crates fill the page at name-tier credit. The agent surface's exact-token semantics
///   won't share this; assert it there, not here.
#[test]
fn concept_queries_reach_their_crate() {
    for (query, expected) in [
        ("deserialize", "serde"),
        ("deserialization", "serde"),
        ("command line arguments", "clap"),
        ("terminal colors", "termcolor"),
        ("terminal colors", "colored"),
        ("connection pool", "bb8"),
        ("connection pool", "r2d2"),
        ("template engine", "tera"),
        ("template engine", "minijinja"),
        ("image resizing", "image"),
        ("random number generator", "rand"),
        ("error handling", "snafu"),
        ("grpc", "tonic"),
        ("kafka", "rdkafka"),
    ] {
        assert_top_contains(query, 8, expected);
    }
}

/// Crates reachable only through their declared keywords — the token appears
/// in neither their name nor their description. This is the facets tier's
/// reason to exist: thiserror and anyhow were unfindable for `error
/// handling` before it (their descriptions say "derive(Error)" and "Flexible
/// concrete Error type"; only their `error-handling` keyword says
/// "handling"), and rumqttc's `mqtt` keyword backs up the one mention in its
/// description.
#[test]
fn keyword_evidence_reaches_undescribed_crates() {
    assert_top_contains("error handling", 8, "thiserror");
    assert_top_contains("error handling", 8, "anyhow");
    assert_top_contains("mqtt", 8, "rumqttc");
}

/// The max rule (credit each token once, at its best evidence) keeps a
/// query that *names a crate* identical whether description matching is on
/// or off: the named crate must not be displaced by neighbors whose
/// descriptions mention it. This was the sum-rule failure: `serde` ranked
/// `serde_spanned` and `serde_urlencoded` above serde itself.
///
/// Deliberately scoped to single-token name queries. Multi-token queries are
/// *not* neutral, by observation against this snapshot: for `trillium
/// router`, `matchit` — a popular router whose name matches neither token —
/// enters the page on description credit alone (96 + high rank outscores
/// low-rank name matches like `routerify`). That interleaving below the full
/// matches is accepted: the IDF floor-sweep that tried to demote it degraded
/// every concept-query page and was removed (see [`MatchCounts`]).
#[test]
fn name_queries_unchanged_by_description_matching() {
    let off = TypeaheadWeights {
        description_match: 0.0,
        ..TypeaheadWeights::default()
    };
    for query in ["serde", "tokio", "postgres"] {
        assert_eq!(
            page(query, &TypeaheadWeights::default(), 8),
            page(query, &off, 8),
            "{query:?}: description matching changed a name query's page"
        );
    }
}

/// Settled precedence pairs, each the residue of a war story.
#[test]
fn precedence_pairs() {
    // The max rule: serde itself above the neighbors that say "serde" in
    // their descriptions.
    assert_precedes("serde", "serde", "serde_spanned");
    // whole_prefix stays small: the popular continuation still beats the
    // whole-name-prefix crowd's tail, but serde_json tops jsonwebtoken.
    assert_precedes("json", "serde_json", "jsonwebtoken");
    // Full matches before subset matches, even against a ~65k× popularity gap.
    assert_precedes("trillium tokio", "trillium-tokio", "tokio");
}

/// The query names the crate exactly: it must come first at the scoring
/// layer, before the service-level exact hoist ever sees it.
#[test]
fn exact_names_rank_first() {
    for name in ["serde", "tokio", "clap"] {
        assert_eq!(top(name, 8).first().map(String::as_str), Some(name));
    }
}

/// Typos still land via the trigram/fuzzy fill.
#[test]
fn fuzzy_fill_catches_typos() {
    assert_top_contains("srde", 8, "serde");
    assert_top_contains("tokoi", 8, "tokio");
}

/// Not an assertion: prints the pinned-fixture page for the whole probe
/// battery, for choosing which behaviors are settled enough to ratchet.
/// The probe harness serves the same purpose against the *live* cache; this
/// one is deterministic.
///
/// `cargo test -p ferritin-common --lib crate_names::battery::dump -- --ignored --nocapture`
#[test]
#[ignore]
fn dump() {
    for query in super::probe::CONCEPT_QUERIES.into_iter().chain([
        "trillium router",
        "mqtt",
        "cli",
        "orm",
        "http server",
    ]) {
        println!("{query:>28}: {}", top(query, 8).join(" "));
    }
}
