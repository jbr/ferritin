use super::json_document::JsonDocument;
use crate::commands::Commands;
use crate::document::Document;
use crate::request::Request;
//use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use querystrong::QueryStrong;
use std::sync::Arc;
use trillium::{Conn, Status};
use trillium_router::RouterConnExt;

/// List all available crates
pub async fn list_crates_handler(conn: Conn) -> Conn {
    let request = conn.state::<Arc<Request>>().unwrap().clone();
    let document = Commands::List.execute(&request);

    if document.is_error() {
        conn.with_status(Status::NotFound)
            .with_body("Failed to list crates")
    } else {
        let json_doc = request.render_to_json(document);
        json_response(conn, &json_doc)
    }
}

/// Get item documentation or search within a crate
pub async fn item_handler(conn: Conn) -> Conn {
    let request = conn.state::<Arc<Request>>().unwrap().clone();
    let item_path = conn.wildcard().unwrap_or("").replace("/", "::");

    let document = Commands::get(&item_path).execute(&request);
    render_document(conn, document)
}

fn render_document(conn: Conn, document: Document<'_>) -> Conn {
    let request = conn.state::<Arc<Request>>().unwrap().clone();

    // Extract actual resolved version from the returned item
    // let canonical_url = document.item().map(|item| {
    //     let crate_name = item.crate_docs().name();
    //     let version = item.crate_docs().version();
    //     let path = item.summary().map(|s| s.path.join("/"));
    //     match (version, path) {
    //         (None, None) => crate_name.to_string(),
    //         (Some(version), None) => format!(
    //             "{crate_name}@{}",
    //             utf8_percent_encode(&version.to_string(), NON_ALPHANUMERIC)
    //         ),
    //         (Some(version), Some(path)) => format!(
    //             "{crate_name}@{}::{path}",
    //             utf8_percent_encode(&version.to_string(), NON_ALPHANUMERIC)
    //         ),
    //         (None, Some(path)) => format!("{crate_name}/{path}"),
    //     }
    // });

    let json_doc = request.render_to_json(document);
    json_response(conn, &json_doc)
}

/// Search within a specific crate
pub(crate) async fn search_handler(conn: Conn) -> Conn {
    let request = conn.state::<Arc<Request>>().unwrap().clone();
    let crate_name = conn.param("crate").unwrap().to_string();
    let querystring = QueryStrong::parse(conn.querystring());
    let Some(query) = querystring.get_str("q") else {
        return conn.with_status(Status::NotFound);
    };

    let document = Commands::search(query)
        .in_crate(&crate_name)
        .execute(&request);

    render_document(conn, document)
}

/// Helper: JSON response with proper content-type
fn json_response(conn: Conn, data: &JsonDocument<'_>) -> Conn {
    match sonic_rs::to_string(data) {
        Ok(json) => conn
            .with_response_header("content-type", "application/json")
            .ok(json)
            .halt(),
        Err(e) => {
            log::error!("JSON serialization failed: {}", e);
            error_response(conn, Status::InternalServerError, "Serialization failed")
        }
    }
}

/// Helper: Error response as JSON
fn error_response(conn: Conn, status: Status, message: &str) -> Conn {
    let error_json = format!(r#"{{"error":"{}"}}"#, message);
    conn.with_status(status)
        .with_response_header("content-type", "application/json")
        .with_body(error_json)
        .halt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{format_context::FormatContext, web::server::api_router};
    use ferritin_common::{Navigator, sources::StdSource};
    use trillium::{Handler, State};
    use trillium_testing::prelude::*;

    fn test_handler() -> impl Handler {
        let request = Arc::new(build_test_request());
        (State::new(request), api_router())
    }

    fn build_test_request() -> Request {
        let std_source = StdSource::from_rustup();
        let navigator = Navigator::default().with_std_source(std_source);
        let format_context = FormatContext::new();
        Request::new(navigator, format_context)
    }

    #[test]
    fn test_list_crates() {
        let mut conn = get("/api/crates").on(&test_handler());

        assert_eq!(conn.status(), Some(Status::Ok));
        assert_eq!(
            conn.response_headers()
                .get("content-type")
                .map(|h| h.as_ref()),
            Some(b"application/json".as_ref())
        );

        let body = conn.take_response_body_string().unwrap();
        let doc: JsonDocument = sonic_rs::from_str(&body).unwrap();

        // Should have nodes
        assert!(!doc.nodes().is_empty());
    }

    #[test]
    fn test_get_nonexistent_crate() {
        let mut conn = get("/api/crates/nonexistent@1.0.0").on(&test_handler());

        assert_eq!(conn.status(), Some(Status::NotFound));
        assert_eq!(
            conn.response_headers()
                .get("content-type")
                .map(|h| h.as_ref()),
            Some(b"application/json".as_ref())
        );

        let body = conn.take_response_body_string().unwrap();
        assert!(body.contains("error"));
    }

    #[test]
    fn test_get_std_crate() {
        let mut conn = get("/api/crates/std").on(&test_handler());

        assert_eq!(conn.status(), Some(Status::Ok));

        let body = conn.take_response_body_string().unwrap();
        let doc: JsonDocument = sonic_rs::from_str(&body).unwrap();

        assert!(!doc.nodes().is_empty());
    }

    #[test]
    fn test_get_std_item() {
        let mut conn = get("/api/crates/std::vec::Vec").on(&test_handler());

        assert_eq!(conn.status(), Some(Status::Ok));

        let body = conn.take_response_body_string().unwrap();
        let doc: JsonDocument = sonic_rs::from_str(&body).unwrap();

        assert!(!doc.nodes().is_empty());
    }

    #[test]
    fn test_search() {
        let mut conn = get("/api/crates/std?q=vec").on(&test_handler());

        assert_eq!(conn.status(), Some(Status::Ok));

        let body = conn.take_response_body_string().unwrap();
        let doc: JsonDocument = sonic_rs::from_str(&body).unwrap();

        // Should have search results
        assert!(!doc.nodes().is_empty());
    }
}
