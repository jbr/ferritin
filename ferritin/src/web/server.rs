use super::handlers::*;
use crate::{format_context::FormatContext, request::Request};
use ferritin_common::{
    Navigator,
    sources::{DocsRsSource, LocalSource, StdSource},
};
use std::path::Path;
use std::sync::Arc;
use trillium::{Conn, Init, State};
use trillium_router::{Router, RouterConnExt};

/// Build the API router with all routes
pub fn api_router() -> Router {
    Router::new()
        .get("/api/crates/*", move |conn: Conn| async {
            if let Some(wildcard) = conn.wildcard()
                && !wildcard.is_empty()
            {
                item_handler(conn).await
            } else {
                list_crates_handler(conn).await
            }
        })
        .get("/api/search/:crate", search_handler)
}

/// Build a Request instance for the server
pub fn build_request(manifest_path: &Path) -> Request {
    let local_source = LocalSource::load(manifest_path).ok();
    let std_source = StdSource::from_rustup();
    let docsrs_source = DocsRsSource::from_default_cache();

    let navigator = Navigator::default()
        .with_std_source(std_source)
        .with_local_source(local_source)
        .with_docsrs_source(docsrs_source);

    let format_context = FormatContext::new();
    Request::new(navigator, format_context)
}

/// Run the JSON API server
/// Uses HOST and PORT env vars (defaults to 127.0.0.1:8080)
pub fn run_server(manifest_path: &Path, open: bool) {
    env_logger::init();
    let request = Arc::new(build_request(manifest_path));

    let handler = (
        Init::new(move |info| async move {
            if open && let Some(tcp) = info.tcp_socket_addr() {
                let _ = webbrowser::open(&format!("http://{tcp}"));
            }
        }),
        trillium_logger::logger(),
        State::new(request),
        api_router(),
        #[cfg(feature = "web")]
        trillium_frontend::frontend!("../web")
            .with_client(trillium_smol::ClientConfig::default())
            .with_index_file("index.html"),
    );

    trillium_smol::run(handler);
}
