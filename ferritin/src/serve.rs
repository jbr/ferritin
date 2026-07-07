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

use std::sync::Arc;

use ferritin_common::{
    Navigator,
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
};

/// Worker-thread stack size. The CLI runs the same lookups on the main thread's
/// 8 MB stack; give the pool workers headroom beyond that.
const WORKER_STACK_SIZE: usize = 16 * 1024 * 1024;

/// Default number of results for the crate-scoped search endpoint.
const SEARCH_LIMIT: usize = 10;

pub fn serve() {
    let navigator = Navigator::default()
        .with_std_source(StdSource::from_rustup())
        .with_docsrs_source(DocsRsSource::from_default_cache());

    let pool = Arc::new(
        ThreadPoolBuilder::new()
            .thread_name(|i| format!("ferritin-docs-{i}"))
            .stack_size(WORKER_STACK_SIZE)
            .build()
            .expect("failed to build documentation thread pool"),
    );

    let server_handle = trillium_smol::config()
        .with_shared_state(Arc::new(navigator))
        .with_shared_state(pool)
        .spawn(handler());

    server_handle.block();
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
        Router::new()
            .get("/api/crates/:crate_name", get_crate)
            .get("/api/search/:crate_name", search_crate),
        frontend(),
    )
}

/// The frontend handler, single-origin with the API. The default build embeds the
/// client assets built at compile time (`../client`); `--features dev-proxy` instead
/// spawns and proxies the Vite dev server (with HMR).
fn frontend() -> impl trillium::Handler {
    use trillium_client::Client;
    use trillium_compression::client::Compression;
    use trillium_logger::client::{ClientLogger, client_log_format};
    use trillium_smol::ClientConfig;
    trillium_frontend::frontend!("../client")
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
fn context(conn: &Conn) -> Option<(Arc<Navigator>, Arc<ThreadPool>)> {
    let navigator = conn.shared_state::<Arc<Navigator>>().cloned()?;
    let pool = conn.shared_state::<Arc<ThreadPool>>().cloned()?;
    Some((navigator, pool))
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
    let Some((navigator, pool)) = context(&conn) else {
        return conn.with_status(Status::InternalServerError).halt();
    };

    let Some(path) = decoded_param(&conn, "crate_name") else {
        return conn.with_status(Status::InternalServerError).halt();
    };

    let outcome = run_blocking(&pool, move || {
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

async fn search_crate(conn: Conn) -> Conn {
    let Some((navigator, pool)) = context(&conn) else {
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
        let mut request = Request::new(&navigator, FormatContext::new());
        let model = commands::search::model(&mut request, &q, SEARCH_LIMIT, Some(&crate_name));
        json::search_to_string(&model).map(|body| (Status::Ok, body))
    })
    .await;

    respond_json(conn, outcome)
}
