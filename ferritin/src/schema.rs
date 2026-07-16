//! OpenAPI 3.0 schema emission for the JSON API (development tool).
//!
//! Compiled only under the `schema` feature. The generated document is committed
//! to `assets/openapi.json` and regenerated with:
//!
//! ```sh
//! cargo run -p ferritin --features schema -- schema
//! pnpm --dir client run gen   # regenerate the TypeScript types
//! ```
//!
//! The command *writes the file directly* (defaulting to [`DEFAULT_OUTPUT_PATH`])
//! rather than printing to stdout. That is deliberate: the client build script
//! emits its vite progress to stdout, so a `cargo run ... > openapi.json`
//! redirect would splice that noise into the top of the file and corrupt the
//! JSON. Pass a path to override the destination (`ferritin schema /tmp/x.json`).
//!
//! This is the single source of truth that feeds the TypeScript client codegen
//! (`openapi-typescript` / `openapi-fetch`) and a future Swagger UI site.
//!
//! The response model schemas come from the same `#[derive(schemars::JsonSchema)]`
//! that lives next to `#[derive(Serialize)]` on the [`crate::json`] DTOs. A single
//! `SchemaGenerator` configured for OpenAPI 3.0 collects every reachable type into
//! one shared `components/schemas` map (no per-root duplication), and the hand
//! written `paths` describe the current endpoints. The whole document is assembled
//! from `schemars::Schema` values plus plain structs and serialized with sonic-rs,
//! so no serde_json appears in our code.

use schemars::{Schema, generate::SchemaSettings, json_schema};

/// Version of the JSON API contract, deliberately independent of the binary's
/// `CARGO_PKG_VERSION`. The schema describes the shape of the API, which changes
/// far more slowly than the crate; tying the two together made every release-plz
/// version bump rewrite this document and trip the drift guard below. Bump the
/// minor for additive changes (a new endpoint or optional field) and the major
/// for anything that breaks existing clients.
const SCHEMA_VERSION: &str = "1.0.0";

/// Default destination for `ferritin schema`: the committed asset, resolved
/// relative to this crate's manifest so it works regardless of the current
/// working directory. Baking in the build-tree path is fine — the `schema`
/// feature is a dev tool, never shipped in release binaries.
pub(crate) const DEFAULT_OUTPUT_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/assets/openapi.json");
use crate::json::{JsonItem, JsonList, JsonNotFound, JsonSearch, JsonTypeahead};
use serde::Serialize;
use std::collections::BTreeMap;

/// Build the OpenAPI document and serialize it to pretty JSON.
pub(crate) fn openapi_document() -> String {
    // `for_serialize` so `skip_serializing_if` fields are modeled as optional —
    // these schemas describe responses (serialized output), never request bodies.
    let mut generator = SchemaSettings::openapi3().for_serialize().into_generator();

    // Registering each response root populates the shared definitions map and
    // returns a `$ref` schema we drop into the corresponding response body.
    let item = generator.subschema_for::<JsonItem<'static>>();
    let search = generator.subschema_for::<JsonSearch<'static>>();
    let not_found = generator.subschema_for::<JsonNotFound>();
    let typeahead = generator.subschema_for::<JsonTypeahead>();
    let _ = generator.subschema_for::<JsonList>();

    let schemas: Schema = generator.take_definitions(true).into();

    let doc = OpenApi {
        openapi: "3.0.3",
        info: Info {
            title: "ferritin",
            version: SCHEMA_VERSION,
            description: "Structured Rust documentation as JSON (the semantic IR).",
        },
        // The JSON API is mounted under /api so it never collides with the SPA's
        // own routes; paths below stay resource-relative.
        servers: vec![Server {
            url: "/api",
            description: "JSON API mount point.",
        }],
        paths: BTreeMap::from([
            (
                "/crates/{crate_name}",
                PathItem {
                    get: Operation {
                        summary: "Structured documentation model for an item path.",
                        parameters: vec![Param::path(
                            "crate_name",
                            "Item path, `::`-joined (e.g. `serde::Deserialize`).",
                        )],
                        responses: BTreeMap::from([
                            ("200", Response::json("The resolved item model.", item)),
                            (
                                "404",
                                Response::json(
                                    "No item resolved; \"did you mean\" suggestions returned.",
                                    not_found,
                                ),
                            ),
                        ]),
                    },
                },
            ),
            (
                "/search/{crate_name}",
                PathItem {
                    get: Operation {
                        summary: "Search for items within a single crate.",
                        parameters: vec![
                            Param::path(
                                "crate_name",
                                "Crate to search (optionally `name@version`).",
                            ),
                            Param::query("q", "Search query."),
                        ],
                        responses: BTreeMap::from([(
                            "200",
                            Response::json("Search results.", search),
                        )]),
                    },
                },
            ),
            (
                "/typeahead",
                PathItem {
                    get: Operation {
                        summary: "Crate-name typeahead: the top crates (by download rank) whose \
                                  names start with the query prefix.",
                        parameters: vec![
                            Param::query("q", "Crate name prefix."),
                            Param::optional_int_query(
                                "limit",
                                "Maximum results (default 10, capped at 100).",
                            ),
                        ],
                        responses: BTreeMap::from([(
                            "200",
                            Response::json("Typeahead results, in rank order.", typeahead),
                        )]),
                    },
                },
            ),
        ]),
        components: Components { schemas },
    };

    sonic_rs::to_string_pretty(&doc).expect("OpenAPI document should serialize")
}

#[derive(Serialize)]
struct OpenApi {
    openapi: &'static str,
    info: Info,
    servers: Vec<Server>,
    paths: BTreeMap<&'static str, PathItem>,
    components: Components,
}

#[derive(Serialize)]
struct Info {
    title: &'static str,
    version: &'static str,
    description: &'static str,
}

#[derive(Serialize)]
struct Server {
    url: &'static str,
    description: &'static str,
}

#[derive(Serialize)]
struct PathItem {
    get: Operation,
}

#[derive(Serialize)]
struct Operation {
    summary: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    parameters: Vec<Param>,
    responses: BTreeMap<&'static str, Response>,
}

#[derive(Serialize)]
struct Param {
    name: &'static str,
    #[serde(rename = "in")]
    location: &'static str,
    required: bool,
    description: &'static str,
    schema: Schema,
}

impl Param {
    fn path(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            location: "path",
            required: true,
            description,
            schema: json_schema!({ "type": "string" }),
        }
    }

    fn query(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            location: "query",
            required: true,
            description,
            schema: json_schema!({ "type": "string" }),
        }
    }

    fn optional_int_query(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            location: "query",
            required: false,
            description,
            schema: json_schema!({ "type": "integer" }),
        }
    }
}

#[derive(Serialize)]
struct Response {
    description: &'static str,
    content: Content,
}

impl Response {
    fn json(description: &'static str, schema: Schema) -> Self {
        Self {
            description,
            content: Content {
                application_json: MediaType { schema },
            },
        }
    }
}

#[derive(Serialize)]
struct Content {
    #[serde(rename = "application/json")]
    application_json: MediaType,
}

#[derive(Serialize)]
struct MediaType {
    schema: Schema,
}

/// The `components` object. Only `schemas` is populated today.
#[derive(Serialize)]
struct Components {
    schemas: Schema,
}

#[cfg(test)]
mod tests {
    /// Drift guard: the committed `assets/openapi.json` must match what the DTOs
    /// currently generate. This module is feature-gated, so the test only exists
    /// under `--features schema`; CI runs it by name in the serve job, since the
    /// default-feature `cargo test --all` job cannot see it.
    #[test]
    fn committed_schema_is_up_to_date() {
        let generated = super::openapi_document();
        let committed = include_str!("../assets/openapi.json");
        assert_eq!(
            generated.trim(),
            committed.trim(),
            "assets/openapi.json is stale; regenerate with `cargo run -p ferritin --features \
             schema -- schema` (it writes the file itself — do not redirect stdout)",
        );
    }
}
