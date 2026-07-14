//! Cache validators derived without rendering a response.
//!
//! [`trillium_caching_headers::CachingHeaders`] validates in `before_send` — by
//! which point the whole handler stack has run and the body exists. For static
//! files that is exactly right: the body is the cheap part, and hashing it is
//! the only way to know what it is. Here it is backwards. Producing the body
//! means resolving a crate, loading and parsing (or mmapping) its rustdoc JSON,
//! walking the item tree, and serializing a model — hundreds of milliseconds and
//! megabytes of cache pressure — and then, if the client already had it, we hash
//! the result only to throw it away.
//!
//! We can do better, because a documentation response is a pure function of
//! inputs we can name far more cheaply than we can evaluate:
//!
//! - **The binary.** All rendering logic — the domain model, the JSON DTOs, the syntax highlighter
//!   — is compiled in, so a redeploy can change the body for unchanged inputs. See [`Build`].
//! - **The exact resolved crate version.** Rustdoc JSON for a released version is immutable (which
//!   is why the Store gives crate data no positive TTL), so the version *is* the identity of the
//!   data. Getting it costs only phase-1 resolution — [`Navigator::lookup_crate`], an in-memory
//!   cache hit once warm — which parses no JSON and loads no crate.
//! - **The request.** The item path, the search query, the result limit.
//!
//! Hash those together and you have an entity tag that changes exactly when the
//! body would, computed before any of the expensive work. A matching
//! `If-None-Match` short-circuits to `304` having touched nothing but a hash map.
//!
//! The soundness argument for the short-circuit is that we only ever *emit* one
//! of these etags alongside a `200`, so a client presenting one is presenting the
//! identity of a body we actually served — and identical inputs to the same
//! binary render an identical body.
//!
//! ## The one input this does not name
//!
//! Cross-crate traversal resolves external crates through the entrypoint's build
//! graph at exact versions, so the entrypoint's version determines them too — but
//! its third fallback, for a name the entrypoint's graph does not pin, is
//! `VersionReq::STAR`, i.e. "latest". A response reaching that fallback can change
//! without the entrypoint version changing, and this etag will not notice. That is
//! a deliberate trade: the alternative is hashing the rendered body, which is the
//! entire cost we are avoiding. The staleness window is one `RESOLUTION_TTL`, and
//! the affected surface is links into crates the entrypoint was not built against.

use ferritin_common::{CratePath, Navigator};
use std::{
    env, fs,
    hash::{DefaultHasher, Hash, Hasher},
    sync::LazyLock,
    time::{Duration, SystemTime},
};
use trillium::{Conn, Handler};
use trillium_caching_headers::{
    CacheControlDirective, CacheControlHeader, CachingHeadersExt, EntityTag,
};

/// How long a content-hashed asset may be cached. A year is the conventional
/// ceiling (RFC 9111 suggests treating larger values as a year anyway).
const ASSET_MAX_AGE: Duration = Duration::from_secs(365 * 24 * 60 * 60);

/// The identity of the running executable.
///
/// The binary is an input to every documentation response — it *is* the
/// rendering logic — so a redeploy must invalidate every etag we have handed
/// out, even though no crate's data changed.
///
/// We identify it by stat'ing our own executable once, rather than by a
/// build-script-stamped commit hash, because the file's length and mtime already
/// distinguish every build that could possibly be running and cost no build
/// machinery to obtain. The mtime does double duty as the `Last-Modified` floor.
///
/// Read once at startup, deliberately: the identity we want is that of the
/// *running* process, which does not change when the file underneath it is
/// replaced by a deploy.
struct Build {
    id: u64,
    mtime: Option<SystemTime>,
}

static BUILD: LazyLock<Build> = LazyLock::new(|| {
    let metadata = env::current_exe().and_then(fs::metadata).ok();

    let mtime = metadata.as_ref().and_then(|meta| meta.modified().ok());

    let mut hasher = hasher();
    // Version-stamp the hash so that a build we cannot stat still differs
    // across releases, and so a stat'able one is never confused with it.
    env!("CARGO_PKG_VERSION").hash(&mut hasher);
    match &metadata {
        Some(meta) => {
            meta.len().hash(&mut hasher);
            mtime.hash(&mut hasher);
        }
        None => log::warn!(
            "could not stat the running executable; cache validators will only distinguish builds \
             by crate version, so a redeploy within a version may serve stale 304s"
        ),
    }

    Build {
        id: hasher.finish(),
        mtime,
    }
});

/// The cache validators for one response: what a conditional request is checked
/// against, and what we send so the next one can be.
#[derive(Debug, Clone)]
pub(crate) struct Validators {
    etag: EntityTag,
    last_modified: Option<SystemTime>,
}

impl Validators {
    /// Whether `if_none_match` names this exact representation — i.e. whether
    /// the client already holds the body we are about to spend real work
    /// producing.
    ///
    /// Weak comparison, because our etags are weak: see [`Self::apply`].
    pub(crate) fn matches(&self, if_none_match: Option<&EntityTag>) -> bool {
        if_none_match.is_some_and(|if_none_match| self.etag.weak_eq(if_none_match))
    }

    /// Attach these validators to a response.
    ///
    /// This sets identity only, not policy: `Cache-Control` is a property of the
    /// endpoint rather than of the body, so [`no_cache`] applies it to every API
    /// response, including the ones we decline to validate.
    ///
    /// The etag is **weak** because `trillium_compression` re-encodes the body
    /// after us: one etag stands for several byte-level encodings of the same
    /// representation, which is exactly what weak comparison means. (Compression
    /// adds `Accept-Encoding` to `Vary`, so caches keep the encodings apart.)
    pub(crate) fn apply(&self, conn: Conn) -> Conn {
        let conn = conn.with_etag(&self.etag);

        match self.last_modified {
            Some(last_modified) => conn.with_last_modified(last_modified),
            None => conn,
        }
    }
}

/// `Cache-Control: no-cache` — the default for everything but a hashed asset.
///
/// `no-cache` is not "do not cache". It is "cache, but revalidate before every
/// reuse", which is precisely the conditional request this module makes cheap.
/// Without it a client applies *heuristic* freshness — a fraction of the age
/// implied by `Last-Modified` — and may reuse a stale response without asking us
/// at all. For a floating version (`/api/crates/serde::Serialize` always means the
/// newest serde) that would silently pin a reader to an old release.
///
/// Applied even to responses carrying no [`Validators`] of ours, since
/// `CachingHeaders` gives those a body-hash etag to revalidate against anyway.
fn no_cache(conn: Conn) -> Conn {
    conn.with_cache_control(CacheControlDirective::NoCache)
}

/// The validators for a documentation response, derived **without loading or
/// rendering anything**.
///
/// `route` distinguishes endpoints that could otherwise agree on every other
/// input; `path` is the requested item path (`tokio@1::runtime::Runtime`), parsed
/// for its crate segment by the same [`CratePath`] the resolver uses, so the
/// crate we validate against is the crate that would be loaded; `varies_on` is
/// everything else the body depends on (a search query, a result limit).
///
/// `None` means "no cheap identity available" — the crate did not resolve, so
/// there is nothing to validate against and the caller should just do the work.
/// A response with no validators is one we will never 304, which is the safe
/// direction to fail in.
pub(crate) fn documentation(
    navigator: &Navigator,
    route: &str,
    path: &str,
    varies_on: &[&str],
) -> Option<Validators> {
    let CratePath {
        name, version_req, ..
    } = CratePath::parse(path);

    // Phase-1 resolution only: which exact version this request means. No JSON
    // is parsed and no crate is loaded.
    let info = navigator.lookup_crate(name, &version_req)?;

    let mut hasher = hasher();
    BUILD.id.hash(&mut hasher);
    route.hash(&mut hasher);
    info.name().hash(&mut hasher);
    info.version().hash(&mut hasher);
    path.hash(&mut hasher);
    varies_on.hash(&mut hasher);

    // The rustdoc JSON on disk is the data behind the response, so its mtime is
    // the honest `Last-Modified` — but the binary is an input too, and a redeploy
    // must move the timestamp forward or a client revalidating with
    // `If-Modified-Since` would be told nothing changed. Hence the later of the
    // two. Absent when the crate has not been fetched yet, in which case we send
    // no `Last-Modified` at all rather than an invented one.
    let json_mtime = info
        .json_path()
        .and_then(|path| fs::metadata(path).ok())
        .and_then(|meta| meta.modified().ok());

    Some(Validators {
        etag: etag(hasher.finish()),
        last_modified: [BUILD.mtime, json_mtime].into_iter().flatten().max(),
    })
}

/// The validators for a typeahead response.
///
/// Typeahead has no expensive path to skip — a query is two binary searches over
/// an already-resident artifact — so unlike [`documentation`] this exists purely
/// to keep repeat answers off the wire. It is derived *after* the query rather
/// than before it, and the 304 is left to
/// [`trillium_caching_headers::Etag`], which honors an etag we set and so
/// never hashes the body.
///
/// `artifact_etag` is the upstream etag of the crate-names artifact the answer
/// came from — the service already tracks it to make its own conditional GETs,
/// and it is the only thing that moves when the data does. `None` (an artifact
/// server that sent no etag) means no validators: we decline to invent an
/// identity we cannot verify.
///
/// Deliberately **no `Last-Modified`**: we have no timestamp that moves when the
/// artifact does, and a plausible-but-static one would let a client's
/// `If-Modified-Since` win a 304 for data that had in fact changed.
pub(crate) fn typeahead(
    artifact_etag: Option<&str>,
    query: &str,
    limit: usize,
) -> Option<Validators> {
    let artifact_etag = artifact_etag?;

    let mut hasher = hasher();
    BUILD.id.hash(&mut hasher);
    artifact_etag.hash(&mut hasher);
    query.hash(&mut hasher);
    limit.hash(&mut hasher);

    Some(Validators {
        etag: etag(hasher.finish()),
        last_modified: None,
    })
}

/// The hasher every etag is built with.
///
/// [`DefaultHasher::new`] is **fixed-seed**, and that is load-bearing rather than
/// incidental: an etag must mean the same thing in the process that checks it as
/// in the one that emitted it, or a restart would invalidate every cache we have
/// handed out. Do not swap this for a `RandomState`-seeded hasher (the one
/// `HashMap` uses by default) — the etags would still look perfectly valid, and
/// every client would silently re-download everything on every restart.
///
/// It need not be stable across *Rust versions*: a new compiler means a new
/// binary, which moves [`BUILD`] and so changes every etag anyway.
fn hasher() -> DefaultHasher {
    DefaultHasher::new()
}

/// A weak entity tag over a 64-bit identity. Hex, so it is always a valid tag.
fn etag(identity: u64) -> EntityTag {
    EntityTag::weak(&format!("{identity:016x}"))
}

/// The server's `Cache-Control` policy, in one rule: **revalidate everything
/// except a content-hashed asset.**
///
/// Vite emits its bundles as `/assets/index-{contenthash}.{js,css}`, so those
/// URLs change whenever their bytes do and can never go stale. They get
/// `max-age=1y, immutable` — the one case where we want *no* conditional request
/// at all, rather than a cheap one.
///
/// Everything else gets [`no_cache`], which is what makes the etags on it load-
/// bearing. `index.html` above all: it is not hashed, and it is what *names* the
/// current bundle, so a heuristically-cached copy would keep serving the previous
/// deployment's asset URLs. A response with no `Cache-Control` is not "uncached";
/// it is cached on a guess.
///
/// The immutable rule is deliberately narrow — successful responses only. A
/// year-long cached `404` would be its own kind of outage.
pub(crate) struct CachingPolicy;

/// Set during `run`, where the path is still the full request path. `before_send`
/// cannot re-derive it: by then the router may have consumed prefixes off the
/// path.
struct HashedAsset;

impl Handler for CachingPolicy {
    async fn run(&self, conn: Conn) -> Conn {
        if conn.path().starts_with("/assets/") {
            conn.with_state(HashedAsset)
        } else {
            conn
        }
    }

    async fn before_send(&self, conn: Conn) -> Conn {
        if conn.state::<HashedAsset>().is_some()
            && conn.status().is_some_and(|status| status.is_success())
        {
            conn.with_cache_control(
                [
                    CacheControlDirective::MaxAge(ASSET_MAX_AGE),
                    CacheControlDirective::Immutable,
                ]
                .into_iter()
                .collect::<CacheControlHeader>(),
            )
        } else {
            no_cache(conn)
        }
    }
}
