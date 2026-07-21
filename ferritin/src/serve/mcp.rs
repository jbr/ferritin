//! Stateless [Model Context Protocol](https://modelcontextprotocol.io) endpoint.
//!
//! A single HTTP endpoint (`/mcp`, a sibling to `/api`) implementing the
//! **Streamable HTTP** transport in its simplest conformant form: stateless,
//! JSON-only, no SSE and no sessions. A `POST` carrying one JSON-RPC *request*
//! gets a single `application/json` response; a *notification* gets `202
//! Accepted` with no body; a `GET` gets `405` (this server offers no
//! server-initiated stream). The spec permits all of this — SSE and
//! `Mcp-Session-Id` sessions are optional, and a documentation lookup needs
//! neither.
//!
//! Two tools are exposed, mirroring the JSON API: [`Get`] (`GET /api/crates/…`)
//! and [`Search`] (`GET /api/search/:crate`). Both return the **agent** render
//! of the same `Document` the CLI produces — token-efficient Markdown, exactly
//! what an LLM tool result wants.
//!
//! The message is decoded and executed *inside* the big-stack [`run_blocking`]
//! worker, for the same reason [`get_crate`](super::get_crate) and
//! [`search_crate`](super::search_crate) run there: the documentation lookups
//! recurse deeply enough to overflow an async worker's 2 MB stack. Nothing
//! borrowed from the per-request `Navigator` crosses the `.await` — the response
//! is serialized to an owned `String` in the worker.

mod protocol;

use super::{context, run_blocking};
use crate::{
    commands::Commands, format_context::FormatContext, renderer, request::Request,
    serve::RATELIMIT_BURST_DIVISOR,
};
use ferritin_common::{Navigator, Store};
use protocol::{AsToolSchema, Example, Info, McpMessage, Tool, ToolSchema, WithExamples};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{env, net::IpAddr, sync::Arc};
use trillium::{Conn, KnownHeaderName, Status};
use trillium_ratelimit::{Quota, RateLimiter};

/// Default result count for the `search` tool — matches the JSON API's
/// [`SEARCH_LIMIT`](super::SEARCH_LIMIT).
const SEARCH_LIMIT: usize = super::SEARCH_LIMIT;

/// Guidance handed to the client in the `initialize` response's `instructions`.
const INSTRUCTIONS: &str = "Look up Rust documentation. `get` shows an item by path (e.g. \
                            `serde::Deserialize`, `std::vec::Vec`, `tokio@1::runtime::Runtime`). \
                            `search` finds items within a single crate by name or prose. The part \
                            after `@` is a semver requirement, not an exact version — `tokio@1` \
                            serves the newest 1.x; use `tokio@=1.40` to pin an exact release. \
                            Output is token-efficient Markdown.";

/// Per-request state for a tool call: the shared crate [`Store`]. Each tool
/// builds its own short-lived [`Navigator`] over it, pinning only what the
/// lookup touches, exactly as the JSON API handlers do.
struct McpState {
    store: Arc<Store>,
}

impl McpState {
    /// Run a documentation [`Commands`] and render its `Document` in agent mode
    /// to an owned `String`. Borrows from the `Navigator` stay inside this call.
    fn render(&self, command: Commands) -> String {
        let navigator = Navigator::new(Arc::clone(&self.store));
        let mut request = Request::new(&navigator, FormatContext::new());
        let (document, _is_error, _entry) = command.execute(&mut request);
        let mut output = String::new();
        // Agent rendering only ever fails if the writer fails; a `String` never
        // does, so the result is infallible here.
        let _ = renderer::render_agent(&document, &mut output);
        output
    }
}

/// Show documentation for a Rust item by path.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename = "get")]
struct Get {
    /// Item path, e.g. `serde::Deserialize` or `std::vec::Vec`. A crate segment
    /// may carry an `@`-suffixed semver requirement (not an exact version):
    /// `tokio@1::runtime::Runtime` serves the newest 1.x, while `tokio@=1.40`
    /// pins an exact release.
    path: String,
}

impl WithExamples for Get {
    fn examples() -> Vec<Example<Self>> {
        vec![Example {
            description: "Look up a trait",
            item: Self {
                path: "serde::Deserialize".into(),
            },
        }]
    }
}

impl Tool<McpState> for Get {
    fn execute(self, state: &mut McpState) -> anyhow::Result<String> {
        Ok(state.render(Commands::get(self.path)))
    }
}

/// Search for items within a single crate by name or documentation.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename = "search")]
struct Search {
    /// Crate to search, optionally with an `@`-suffixed semver requirement (not
    /// an exact version): `serde`, `tokio@1` (newest 1.x), or `tokio@=1.40` to
    /// pin an exact release.
    #[serde(rename = "crate")]
    crate_: String,
    /// Search query — one or more words matched against item names and prose.
    query: String,
}

impl WithExamples for Search {
    fn examples() -> Vec<Example<Self>> {
        vec![Example {
            description: "Find deserialization items in serde",
            item: Self {
                crate_: "serde".into(),
                query: "deserialize".into(),
            },
        }]
    }
}

impl Tool<McpState> for Search {
    fn execute(self, state: &mut McpState) -> anyhow::Result<String> {
        let command = Commands::search(self.query)
            .in_crate(self.crate_)
            .with_limit(SEARCH_LIMIT);
        Ok(state.render(command))
    }
}

/// The set of tools this server exposes, decoded from a `tools/call` params
/// object (`{ "name": …, "arguments": { … } }`).
#[derive(Debug)]
enum Tools {
    Get(Get),
    Search(Search),
}

impl<'de> Deserialize<'de> for Tools {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        let value = serde_json::Value::deserialize(deserializer)?;
        let name = value
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| D::Error::missing_field("name"))?;
        let arguments = value
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        match name {
            "get" => serde_json::from_value(arguments)
                .map(Tools::Get)
                .map_err(D::Error::custom),
            "search" => serde_json::from_value(arguments)
                .map(Tools::Search)
                .map_err(D::Error::custom),
            other => Err(D::Error::unknown_variant(other, &["get", "search"])),
        }
    }
}

impl Serialize for Tools {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Tools", 2)?;
        match self {
            Tools::Get(tool) => {
                state.serialize_field("name", "get")?;
                state.serialize_field("arguments", tool)?;
            }
            Tools::Search(tool) => {
                state.serialize_field("name", "search")?;
                state.serialize_field("arguments", tool)?;
            }
        }
        state.end()
    }
}

impl Tool<McpState> for Tools {
    fn execute(self, state: &mut McpState) -> anyhow::Result<String> {
        match self {
            Tools::Get(tool) => tool.execute(state),
            Tools::Search(tool) => tool.execute(state),
        }
    }
}

impl protocol::AsToolsList for Tools {
    fn tools_list() -> Vec<ToolSchema> {
        vec![Get::schema(), Search::schema()]
    }
}

/// This server's identity, reported in the `initialize` response.
fn server_info() -> Info {
    Info {
        name: "ferritin".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

/// What decoding-and-executing one client message produced.
enum Outcome {
    /// A JSON-RPC response body to return as `application/json`.
    Response(String),
    /// A notification we accepted — `202 Accepted`, no body.
    Accepted,
    /// The body was not a message we can act on — `400 Bad Request`.
    BadRequest,
}

/// Decode one client message and, if it is a request, execute it. Runs on a
/// big-stack worker (see the module docs).
fn handle_message(store: Arc<Store>, body: &str) -> Outcome {
    match serde_json::from_str::<McpMessage>(body) {
        Ok(McpMessage::Request(request)) => {
            let mut state = McpState { store };
            let response =
                request.execute::<McpState, Tools>(&mut state, Some(INSTRUCTIONS), &server_info());
            match serde_json::to_string(&response) {
                Ok(json) => Outcome::Response(json),
                Err(error) => {
                    log::error!("failed to serialize MCP response: {error}");
                    Outcome::BadRequest
                }
            }
        }
        // A notification (no `id`) expects no reply.
        Ok(McpMessage::Notification(_)) => Outcome::Accepted,
        Err(error) => {
            log::debug!("unparseable MCP message: {error}");
            Outcome::BadRequest
        }
    }
}

/// `POST /mcp`: the MCP endpoint. Reads the JSON-RPC body, executes it on the
/// big-stack pool, and returns the response.
pub(super) async fn post(mut conn: Conn) -> Conn {
    let Some((store, pool)) = context(&conn) else {
        return conn.with_status(Status::InternalServerError).halt();
    };

    let body = match conn.request_body_string().await {
        Ok(body) => body,
        Err(_) => return conn.with_status(Status::BadRequest).halt(),
    };

    let outcome = run_blocking(&pool, move || handle_message(store, &body)).await;

    // `halt()` so the trailing frontend handler doesn't overwrite the body with
    // the SPA index.
    match outcome {
        Some(Outcome::Response(body)) => conn
            .with_status(Status::Ok)
            .with_response_header(KnownHeaderName::ContentType, "application/json")
            .with_body(body)
            .halt(),
        Some(Outcome::Accepted) => conn.with_status(Status::Accepted).halt(),
        Some(Outcome::BadRequest) => conn.with_status(Status::BadRequest).halt(),
        // The worker panicked.
        None => conn.with_status(Status::InternalServerError).halt(),
    }
}

/// `GET /mcp`: this server offers no server-initiated SSE stream, so per the
/// Streamable HTTP spec a `GET` is answered `405 Method Not Allowed`.
pub(super) async fn get(conn: Conn) -> Conn {
    conn.with_status(Status::MethodNotAllowed).halt()
}

/// The MCP endpoint's rate limiter, or `None` when `FERRITIN_MCP_RATELIMIT` is
/// unset. Separate from the `/api` limiter so agent traffic — whose profile
/// (few, expensive, non-browser calls) differs from a browser's — is tuned on
/// its own quota. Keyed on the client network, like [`api_limiter`](super::api_limiter).
pub(super) fn limiter() -> Option<RateLimiter<IpAddr>> {
    let per_minute = env::var("FERRITIN_MCP_RATELIMIT")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|&per_minute| per_minute > 0)?;
    Some(
        RateLimiter::by_network(
            Quota::per_minute(per_minute)
                .allow_burst((per_minute / RATELIMIT_BURST_DIVISOR).max(1)),
        )
        .with_policy_name("mcp"),
    )
}
