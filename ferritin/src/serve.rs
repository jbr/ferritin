//! HTTP server exposing the structured JSON documentation model.
//!
//! Documentation lookups (`resolve_path`, formatting, the `MethodIter`/`TraitIter`
//! index scans) are synchronous and deeply — but finitely — recursive. Run on the
//! async executor's worker threads (~2 MB stacks) they overflow, even though the
//! same work is fine on the CLI's 8 MB main thread. So each request is offloaded to
//! a dedicated [`rayon`] pool whose workers are built with a large stack, and the
//! owned JSON `String` is handed back to the async handler over an
//! [`async_channel`]. Nothing borrowed from the [`Navigator`] crosses the `.await`
//! — the model is serialized to bytes inside the worker. This mirrors the TUI,
//! which likewise owns `Navigator` on a request thread and passes owned results
//! back over a channel.
//!
//! The shared state is an `Arc<Store>` (bounded caches, singleflight, negative
//! TTLs); each request builds its own short-lived [`Navigator`] on the worker,
//! pinning whatever crates it touches for exactly the request's duration. That
//! per-query pinning is what lets the Store evict under memory pressure without
//! invalidating any in-flight request.

use std::sync::Arc;

use ferritin_common::{
    Navigator, Store,
    sources::{DocsRsSource, StdSource},
};
use percent_encoding::percent_decode_str;
use querystrong::QueryStrong;
use rayon::{ThreadPool, ThreadPoolBuilder};
use trillium::{Conn, KnownHeaderName, Status};
use trillium_caching_headers::CachingHeaders;
use trillium_compression::Compression;
use trillium_logger::{Logger, log_format};
use trillium_router::{Router, RouterConnExt};

use crate::{
    commands::{self, get::JsonOutcome},
    format_context::FormatContext,
    json,
    request::Request,
    typeahead::TypeaheadService,
};

/// Worker-thread stack size. The CLI runs the same lookups on the main thread's
/// 8 MB stack; give the pool workers headroom beyond that.
const WORKER_STACK_SIZE: usize = 16 * 1024 * 1024;

/// Default number of results for the crate-scoped search endpoint.
const SEARCH_LIMIT: usize = 10;

/// Default and maximum result counts for the crate-name typeahead endpoint.
const TYPEAHEAD_LIMIT: usize = 10;
const TYPEAHEAD_MAX_LIMIT: usize = 100;

/// Default byte cap for the in-memory crate cache, overridable via
/// `FERRITIN_CACHE_BYTES` (weight proxy: JSON file size at load).
const DEFAULT_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// The crate-cache byte cap: `FERRITIN_CACHE_BYTES` or the default.
fn cache_bytes() -> u64 {
    std::env::var("FERRITIN_CACHE_BYTES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_CACHE_BYTES)
}

/// The RSS guard's anonymous-RSS trip point in bytes:
/// `FERRITIN_RSS_HIGH_WATER_BYTES`. No default — unset means no guard, so
/// localhost servers keep the weight cap as their only (and portable)
/// memory bound. The guard reads Linux-only `/proc` state and exists for the
/// public deployment, where the cache's byte-weight proxies drifting from
/// real memory use must mean shed crates, not an OOM kill.
fn rss_high_water_bytes() -> Option<u64> {
    std::env::var("FERRITIN_RSS_HIGH_WATER_BYTES")
        .ok()
        .and_then(|value| value.parse().ok())
}

pub fn serve() {
    let cache_bytes = cache_bytes();
    let mut store = Store::default()
        .with_std_source(StdSource::from_rustup())
        .with_docsrs_source(DocsRsSource::from_default_cache())
        .with_weight_cap(cache_bytes)
        // Search indexes are far smaller than crate JSON; give them a
        // proportional slice.
        .with_search_weight_cap(cache_bytes / 8);
    if let Some(high_water) = rss_high_water_bytes() {
        store = store.with_rss_high_water(high_water);
    }
    let store = Arc::new(store);

    let pool = Arc::new(
        ThreadPoolBuilder::new()
            .thread_name(|i| format!("ferritin-docs-{i}"))
            .stack_size(WORKER_STACK_SIZE)
            .build()
            .expect("failed to build documentation thread pool"),
    );

    #[cfg(feature = "acme")]
    if let Some(env) = acme::AcmeEnv::from_env() {
        return acme::serve_tls(store, pool, env);
    }

    let server_handle = trillium_smol::config()
        .with_shared_state(store)
        .with_shared_state(pool)
        .spawn(handler());

    server_handle.block();
}

/// Automatic HTTPS (Let's Encrypt via tls-alpn-01) plus HTTP/3, behind the
/// `acme` cargo feature and activated at runtime by environment variables:
///
/// - `FERRITIN_ACME_DOMAIN` (required to activate; comma-separated for multiple
///   domains — the first is the canonical authority insecure requests redirect to)
/// - `FERRITIN_ACME_CACHE_DIR` (required when active): directory persisting the
///   ACME account key and certificates across restarts. Without a cache every
///   restart would re-issue, and Let's Encrypt rate limits re-issuance.
/// - `FERRITIN_ACME_CONTACT` (optional): a contact email; a bare address is
///   prefixed with `mailto:`.
/// - `FERRITIN_ACME_PRODUCTION` (optional, `1`/`true`): use the production
///   Let's Encrypt directory. Defaults to the staging environment, whose
///   certificates are untrusted but generously rate-limited — right for
///   verifying a deployment before flipping to production.
///
/// When active, the server binds TLS on tcp/443 and QUIC on udp/443 (the
/// listener builder auto-advertises `alt-svc` for the matching pair, which is
/// how clients discover h3), plus a cleartext tcp/80 listener that redirects
/// to the canonical https authority. `HOST` overrides the bind address
/// (default `0.0.0.0`). When `FERRITIN_ACME_DOMAIN` is absent, `serve` runs
/// cleartext exactly as it does without this feature.
#[cfg(feature = "acme")]
mod acme {
    use std::path::PathBuf;
    use std::sync::Arc;

    use ferritin_common::Store;
    use rayon::ThreadPool;
    use trillium::{Conn, KnownHeaderName, Status};
    use trillium_acme::{AcmeConfig, rustls_acme::caches::DirCache};
    use trillium_quinn::QuicConfig;

    /// The ACME deployment configuration, parsed from environment variables.
    pub(super) struct AcmeEnv {
        domains: Vec<String>,
        contact: Option<String>,
        cache_dir: PathBuf,
        production: bool,
    }

    impl AcmeEnv {
        /// Returns `None` when `FERRITIN_ACME_DOMAIN` is unset or empty (the
        /// cleartext fallback); panics on a partial configuration, since
        /// silently serving cleartext on a host that meant to serve TLS is
        /// worse than failing to start.
        pub(super) fn from_env() -> Option<Self> {
            let domains: Vec<String> = std::env::var("FERRITIN_ACME_DOMAIN")
                .ok()?
                .split(',')
                .map(|domain| domain.trim().to_string())
                .filter(|domain| !domain.is_empty())
                .collect();
            if domains.is_empty() {
                return None;
            }

            let cache_dir = PathBuf::from(std::env::var("FERRITIN_ACME_CACHE_DIR").expect(
                "FERRITIN_ACME_CACHE_DIR is required when FERRITIN_ACME_DOMAIN is set: \
                 it persists the ACME account key and certificates across restarts, \
                 and Let's Encrypt rate-limits re-issuance",
            ));

            let contact = std::env::var("FERRITIN_ACME_CONTACT").ok().map(|contact| {
                if contact.contains(':') {
                    contact
                } else {
                    format!("mailto:{contact}")
                }
            });

            let production = std::env::var("FERRITIN_ACME_PRODUCTION")
                .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));

            Some(Self {
                domains,
                contact,
                cache_dir,
                production,
            })
        }
    }

    /// Serve h1/h2 over TLS on tcp/443, h3 over QUIC on udp/443 (sharing the
    /// ACME-managed certificate resolver), and an https redirect on tcp/80.
    pub(super) fn serve_tls(store: Arc<Store>, pool: Arc<ThreadPool>, env: AcmeEnv) {
        let AcmeEnv {
            domains,
            contact,
            cache_dir,
            production,
        } = env;
        let authority = domains[0].clone();

        let mut acme_config = AcmeConfig::new(&domains)
            .cache(DirCache::new(cache_dir))
            .directory_lets_encrypt(production);
        if let Some(contact) = &contact {
            acme_config = acme_config.contact_push(contact);
        }

        let (acceptor, acme_future) = trillium_acme::new(acme_config);
        let quic = QuicConfig::from_cert_resolver(acceptor.resolver());
        let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());

        let handle = trillium_smol::config()
            .with_shared_state(store)
            .with_shared_state(pool)
            .with_nodelay()
            .listeners()
            .bind_tcp((&*host, 80))
            .expect("failed to bind the https-redirect listener on tcp/80")
            .bind_tls((&*host, 443), acceptor)
            .expect("failed to bind the TLS listener on tcp/443")
            .bind_quic((&*host, 443), quic)
            .expect("failed to bind the QUIC listener on udp/443")
            .spawn((redirect_insecure(authority), super::handler()));

        let acme_future = handle.swansong().interrupt(acme_future);
        handle.runtime().spawn(acme_future);
        handle.block();
    }

    /// Redirect any request arriving over a non-TLS transport (the tcp/80
    /// listener) to the canonical https authority, preserving path and query.
    fn redirect_insecure(authority: String) -> impl trillium::Handler {
        let authority: Arc<str> = authority.into();
        move |conn: Conn| {
            let authority = authority.clone();
            async move {
                if conn.is_secure() {
                    return conn;
                }
                let path = conn.path();
                let querystring = conn.querystring();
                let location = if querystring.is_empty() {
                    format!("https://{authority}{path}")
                } else {
                    format!("https://{authority}{path}?{querystring}")
                };
                conn.with_status(Status::MovedPermanently)
                    .with_response_header(KnownHeaderName::Location, location)
                    .halt()
            }
        }
    }
}

pub fn handler() -> impl trillium::Handler {
    (
        Logger::new().with_formatter(log_format!(
            "<- {version} {method} {url} {response_time} {status} {body_len_human} {content_encoding}",
            content_encoding =
                trillium_logger::formatters::response_header(KnownHeaderName::ContentEncoding)
        )),
        Compression::new(),
        CachingHeaders::new(),
        trillium::state(Arc::new(TypeaheadService::new())),
        Router::new()
            .get("/api/crates/:crate_name", get_crate)
            .get("/api/search/:crate_name", search_crate)
            .get("/api/typeahead", typeahead),
        frontend(),
    )
}

/// The frontend handler, single-origin with the API.
///
/// `client` is a symlink to the workspace-root client directory, and the path is
/// deliberately *inside* the package rather than `../client`: cargo cannot
/// package files above the package root, so an escaping path would leave the
/// published crate unable to build this feature at all.
///
/// Which mode `frontend!` expands to is decided by what it finds there, and both
/// callers get what they want for free. A repo build sees the client's
/// `package.json` through the symlink and rebuilds the assets at compile time
/// (`--features dev-proxy` instead spawns and proxies the Vite dev server, with
/// HMR). A build from the published tarball sees only the `dist` directory —
/// `include` ships nothing else — and embeds those prebuilt assets, so `serve`
/// compiles from crates.io with no node toolchain present.
fn frontend() -> impl trillium::Handler {
    use trillium_client::Client;
    use trillium_compression::client::Compression;
    use trillium_logger::client::{ClientLogger, client_log_format};
    use trillium_smol::ClientConfig;
    trillium_frontend::frontend!("client")
        .with_client(Client::new(ClientConfig::default()).with_handler((
            ClientLogger::new().with_formatter(client_log_format!(
                "-> {version} {method} {url} {response_time} {status} {body_len_human}"
            )),
            Compression::new(),
        )))
        .with_index_file("index.html")
}

/// Run a synchronous, stack-hungry job on a big-stack pool worker and await the
/// owned result. Returns `None` if the worker panicked (surfaced as a 500 by the
/// caller) rather than aborting the process.
async fn run_blocking<F, T>(pool: &ThreadPool, job: F) -> Option<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = async_channel::bounded(1);
    pool.spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).ok();
        let _ = tx.send_blocking(outcome);
    });
    rx.recv().await.ok().flatten()
}

/// Percent-decode a route param. trillium-router hands params back raw, but the
/// client percent-encodes `::` in item paths (`serde%3A%3ADeserialize`).
fn decoded_param(conn: &Conn, name: &str) -> Option<String> {
    conn.param(name)
        .map(|raw| percent_decode_str(raw).decode_utf8_lossy().into_owned())
}

/// The two shared-state handles every documentation handler needs.
fn context(conn: &Conn) -> Option<(Arc<Store>, Arc<ThreadPool>)> {
    let store = conn.shared_state::<Arc<Store>>().cloned()?;
    let pool = conn.shared_state::<Arc<ThreadPool>>().cloned()?;
    Some((store, pool))
}

/// Apply a worker outcome to the response: the JSON body on success, a 500 when
/// the worker panicked or serialization failed.
fn respond_json(conn: Conn, outcome: Option<sonic_rs::Result<(Status, String)>>) -> Conn {
    // `halt()` so the trailing frontend handler doesn't overwrite the JSON body
    // with the SPA index.
    if let Some(Ok((status, body))) = outcome {
        conn.with_status(status)
            .with_response_header("content-type", "application/json")
            .with_body(body)
            .halt()
    } else {
        conn.with_status(Status::InternalServerError).halt()
    }
}

async fn get_crate(conn: Conn) -> Conn {
    let Some((store, pool)) = context(&conn) else {
        return conn.with_status(Status::InternalServerError).halt();
    };

    let Some(path) = decoded_param(&conn, "crate_name") else {
        return conn.with_status(Status::InternalServerError).halt();
    };

    let outcome = run_blocking(&pool, move || {
        let navigator = Navigator::new(store);
        let mut request = Request::new(&navigator, FormatContext::new());
        match commands::get::model(&mut request, &path, false, false) {
            JsonOutcome::Found {
                model,
                canonical_url,
            } => json::to_string(&model, Some(canonical_url)).map(|body| (Status::Ok, body)),

            JsonOutcome::NotFound(not_found) => {
                json::not_found_to_string(&not_found).map(|body| (Status::NotFound, body))
            }
        }
    })
    .await;

    respond_json(conn, outcome)
}

/// Crate-name typeahead: the top crates (by download rank) whose names start
/// with `q`. Unlike the documentation endpoints this never touches the Store
/// or the worker pool — the query is a pair of binary searches over the
/// in-memory artifact, so it runs inline on the async thread.
async fn typeahead(conn: Conn) -> Conn {
    let Some(service) = conn.state::<Arc<TypeaheadService>>().cloned() else {
        return conn.with_status(Status::InternalServerError).halt();
    };

    let query = QueryStrong::parse(conn.querystring());
    let Some(q) = query.get_str("q").map(str::to_string) else {
        return conn.with_status(Status::BadRequest).halt();
    };
    let limit = query
        .get_str("limit")
        .and_then(|limit| limit.parse().ok())
        .unwrap_or(TYPEAHEAD_LIMIT)
        .min(TYPEAHEAD_MAX_LIMIT);

    let Some(results) = service.typeahead(&q, limit).await else {
        return conn.with_status(Status::ServiceUnavailable).halt();
    };

    respond_json(
        conn,
        Some(json::typeahead_to_string(&q, results).map(|body| (Status::Ok, body))),
    )
}

async fn search_crate(conn: Conn) -> Conn {
    let Some((store, pool)) = context(&conn) else {
        return conn.with_status(Status::InternalServerError).halt();
    };

    let Some(crate_name) = decoded_param(&conn, "crate_name") else {
        return conn.with_status(Status::InternalServerError).halt();
    };

    let query = QueryStrong::parse(conn.querystring());
    let Some(q) = query.get_str("q").map(str::to_string) else {
        return conn.with_status(Status::BadRequest).halt();
    };

    let outcome = run_blocking(&pool, move || {
        let navigator = Navigator::new(store);
        let mut request = Request::new(&navigator, FormatContext::new());
        let model = commands::search::model(&mut request, &q, SEARCH_LIMIT, Some(&crate_name));
        json::search_to_string(&model).map(|body| (Status::Ok, body))
    })
    .await;

    respond_json(conn, outcome)
}
