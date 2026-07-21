//! Transport-agnostic MCP (Model Context Protocol) types and traits.
//!
//! This is a trimmed copy of the [`mcplease`](https://github.com/jbr/mcplease)
//! crate's `types` and `traits` modules — the JSON-RPC message shapes plus the
//! `Tool` / `AsToolsList` machinery — with the stdio transport, the clap-based
//! `tools!` macro, and the session store left behind. Only the parts an HTTP
//! endpoint needs are kept: [`McpMessage::execute`](McpRequest::execute) turns a
//! decoded `initialize` / `tools/list` / `tools/call` request into an
//! [`McpResponse`] with no I/O of its own, so the HTTP glue in the parent module
//! stays thin. Copying rather than depending keeps ferritin's dependency delta
//! small (mcplease would pull in clap, notify, dirs, env_logger and more).

use schemars::{
    JsonSchema, Schema,
    generate::SchemaSettings,
    transform::{RecursiveTransform, Transform},
};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{borrow::Cow, collections::HashMap, fmt::Debug};

/// The MCP protocol version this server speaks when a client sends no
/// `protocolVersion` in its `initialize` request. When the client does send one,
/// [`McpRequest::execute`] echoes it back (version negotiation).
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";

/// A single decoded JSON-RPC message from the client: either a *request* (has an
/// `id`, expects a response) or a *notification* (no `id`, no response).
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpMessage {
    #[serde(deserialize_with = "deserialize_request")]
    Request(McpRequest),
    Notification(McpNotification),
}

fn deserialize_request<'de, D>(deserializer: D) -> Result<McpRequest, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Value = Deserialize::deserialize(deserializer)?;
    if value.get("id").is_some() {
        serde_json::from_value(value).map_err(serde::de::Error::custom)
    } else {
        Err(serde::de::Error::custom("Not a request"))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    pub params: Option<Value>,
}

impl McpRequest {
    /// Dispatch a decoded request to the appropriate MCP method, producing the
    /// response to send back. Transport-agnostic: it neither reads nor writes any
    /// I/O, it just maps `(method, params, state)` to an [`McpResponse`].
    pub fn execute<State, Tools: Debug + AsToolsList + Tool<State>>(
        self,
        state: &mut State,
        instructions: Option<&'static str>,
        server_info: &Info,
    ) -> McpResponse {
        let Self {
            id, method, params, ..
        } = self;
        match method.as_str() {
            "initialize" => {
                // Version negotiation: echo the client's requested protocol
                // version when it sends one, else fall back to our default.
                let protocol_version = params
                    .as_ref()
                    .and_then(|params| params.get("protocolVersion"))
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_PROTOCOL_VERSION)
                    .to_string();
                McpResponse::success(
                    id,
                    InitializeResponse::new(server_info.clone(), protocol_version)
                        .with_instructions(instructions),
                )
            }
            "tools/list" => {
                let tools = Tools::tools_list();
                McpResponse::success(id, ToolsListResponse { tools })
            }
            "tools/call" => match serde_json::from_value::<Tools>(params.unwrap_or(Value::Null)) {
                Ok(tool) => {
                    log::info!("{tool:?}");
                    match tool.execute(state) {
                        Ok(string) => {
                            log::debug!("{string}");
                            McpResponse::success(id, ContentResponse::text(string))
                        }
                        Err(e) => {
                            log::error!("{e}");
                            McpResponse::error(id, e.to_string())
                        }
                    }
                }
                Err(e) => {
                    log::error!("{e}");
                    McpResponse::error(id, e.to_string())
                }
            },
            _ => McpResponse::error(id, format!("Unknown method: {method}")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    protocol_version: String,
    capabilities: Capabilities,
    server_info: Info,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<&'static str>,
}

impl InitializeResponse {
    pub fn new(server_info: Info, protocol_version: String) -> Self {
        Self {
            protocol_version,
            capabilities: Capabilities::default(),
            server_info,
            instructions: None,
        }
    }

    pub fn with_instructions(mut self, instructions: Option<&'static str>) -> Self {
        self.instructions = instructions;
        self
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct Example<T> {
    pub description: &'static str,
    #[serde(flatten)]
    pub item: T,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Info {
    pub name: Cow<'static, str>,
    pub version: Cow<'static, str>,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Capabilities {
    /// Serializes as `{}` — declares the tools capability with no sub-options.
    pub tools: HashMap<(), ()>,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct ToolsListResponse {
    pub tools: Vec<ToolSchema>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSchema {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: InputSchema,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InputSchema {
    // Union types (check these first)
    AnyOf {
        #[serde(rename = "anyOf")]
        any_of: Vec<InputSchema>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    OneOf {
        #[serde(rename = "oneOf")]
        one_of: Vec<InputSchema>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        examples: Option<Vec<Value>>,
    },
    Tagged(Tagged),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Tagged {
    #[serde(rename = "object")]
    Object {
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default)]
        properties: HashMap<String, Box<InputSchema>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        required: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        additional_properties: Option<Box<InputSchema>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        examples: Option<Vec<Value>>,
    },
    #[serde(rename = "string")]
    String {
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        r#enum: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        examples: Option<Vec<String>>,
    },

    #[serde(rename = "boolean")]
    Boolean {
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },

    #[serde(rename = "integer")]
    Integer {
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },

    #[serde(rename = "array")]
    Array {
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        items: Box<InputSchema>,
    },

    #[serde(rename = "null")]
    Null,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct ContentResponse {
    content: Vec<TextContent>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TextContent {
    pub r#type: &'static str,
    pub text: String,
}

impl ContentResponse {
    pub fn text(text: String) -> Self {
        Self {
            content: vec![TextContent {
                r#type: "text",
                text,
            }],
        }
    }
}

impl McpResponse {
    pub fn success(id: Value, result: impl Serialize) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(serde_json::to_value(result).unwrap()),
            error: None,
        }
    }

    pub fn error(id: Value, message: String) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(McpError {
                code: -32601,
                message,
                data: None,
            }),
        }
    }
}

/// Optional worked examples for a tool's input, surfaced in its JSON schema.
pub trait WithExamples: Sized + Serialize {
    fn examples() -> Vec<Example<Self>> {
        vec![]
    }
}

/// A callable MCP tool over some server `State`.
pub trait Tool<State>: Serialize + DeserializeOwned {
    fn execute(self, state: &mut State) -> anyhow::Result<String>;
}

/// Derive a [`ToolSchema`] from a tool's `JsonSchema`.
pub trait AsToolSchema {
    fn schema() -> ToolSchema;
}

/// The full set of tools a server exposes, for `tools/list`.
pub trait AsToolsList {
    fn tools_list() -> Vec<ToolSchema>;
}

/// Drop `"null"` from the `type`/`enum` of an `Option<T>` field's schema, so an
/// optional string presents as `"type": "string"` rather than `["string",
/// "null"]` (which the MCP `InputSchema` shape does not model).
fn remove_null(schema: &mut Schema) {
    if let Some(a @ Value::Array(_)) = schema.get_mut("type") {
        let arr = a.as_array_mut().unwrap();
        arr.retain(|v| matches!(v, Value::String(s) if s != "null"));
        if arr.len() == 1 {
            *a = arr.pop().unwrap();
        }
    }

    if let Some(a @ Value::Array(_)) = schema.get_mut("enum") {
        let arr = a.as_array_mut().unwrap();
        arr.retain(|v| matches!(v, Value::String(s) if s != "null"));
    }
}

impl<T> AsToolSchema for T
where
    T: JsonSchema + WithExamples,
{
    fn schema() -> ToolSchema {
        let settings = SchemaSettings::draft2020_12().with(|s| {
            s.meta_schema = None;
            s.inline_subschemas = true;
        });

        let generator = settings.into_generator();
        let mut schema = generator.into_root_schema_for::<Self>();

        RecursiveTransform(remove_null).transform(&mut schema);

        // The tool's name is its schema title (from the struct's `#[serde(rename
        // = "...")]`); its description is the struct's doc comment.
        let name = schema
            .remove("title")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        let description = schema
            .remove("description")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        schema.remove("$schema");

        let examples = Self::examples();
        if !examples.is_empty() {
            schema.insert(
                "examples".to_string(),
                serde_json::to_value(examples).unwrap(),
            );
        }

        let value: Value = schema.into();
        let input_schema = match serde_json::from_value(value.clone()) {
            Ok(input_schema) => input_schema,
            Err(e) => {
                let json = serde_json::to_string_pretty(&value).unwrap();
                log::error!("could not parse input schema:\n{e}\n\n{json}");
                panic!("{e}");
            }
        };

        ToolSchema {
            name,
            description: Some(description),
            input_schema,
        }
    }
}
