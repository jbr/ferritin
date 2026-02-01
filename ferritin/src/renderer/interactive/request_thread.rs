//! Request thread - handles Navigator operations and document formatting

use super::channels::{RequestResponse, UiCommand};
use super::history::HistoryEntry;
use crate::commands::{list, search};
use crate::{request::Request, styled_string::Document};
use std::sync::mpsc::{Receiver, Sender};

/// Request thread loop - processes commands from UI thread
pub(super) fn request_thread_loop<'a>(
    request: &'a Request,
    cmd_rx: Receiver<UiCommand<'a>>,
    resp_tx: Sender<RequestResponse<'a>>,
) {
    for cmd in cmd_rx {
        match cmd {
            UiCommand::Navigate(doc_ref) => {
                // Format the already-resolved item (e.g., from clicking a link)
                let doc_nodes = request.format_item(doc_ref);
                let doc = Document::from(doc_nodes);
                let entry = HistoryEntry::Item(doc_ref);

                let _ = resp_tx.send(RequestResponse::Document {
                    doc,
                    entry: Some(entry),
                });
            }

            UiCommand::NavigateToPath(path) => {
                let mut suggestions = vec![];
                if let Some(item) = request.resolve_path(path.as_ref(), &mut suggestions) {
                    let doc_nodes = request.format_item(item);
                    let doc = Document::from(doc_nodes);
                    let entry = HistoryEntry::Item(item);

                    let _ = resp_tx.send(RequestResponse::Document {
                        doc,
                        entry: Some(entry),
                    });
                } else {
                    let _ = resp_tx.send(RequestResponse::Error(format!("Not found: {}", path)));
                }
            }

            UiCommand::Search {
                query,
                crate_name,
                limit,
            } => {
                let (search_doc, is_error) = search::execute(
                    request,
                    query.as_ref(),
                    limit,
                    crate_name.as_ref().map(|c| c.as_ref()),
                );

                if is_error {
                    let _ =
                        resp_tx.send(RequestResponse::Error(format!("No results for: {}", query)));
                } else {
                    let entry = HistoryEntry::Search {
                        query: query.into_owned(),
                        crate_name: crate_name.map(|c| c.into_owned()),
                    };

                    let _ = resp_tx.send(RequestResponse::Document {
                        doc: search_doc,
                        entry: Some(entry),
                    });
                }
            }

            UiCommand::List => {
                let (list_doc, _is_error) = list::execute(request);
                let entry = HistoryEntry::List;

                let _ = resp_tx.send(RequestResponse::Document {
                    doc: list_doc,
                    entry: Some(entry),
                });
            }

            UiCommand::ToggleSource {
                include_source,
                current_item,
            } => {
                request.format_context().set_include_source(include_source);
                if let Some(current_item) = current_item {
                    let _ = resp_tx.send(RequestResponse::Document {
                        doc: Document::from(request.format_item(current_item)),
                        entry: None,
                    });
                }
            }

            UiCommand::Shutdown => {
                let _ = resp_tx.send(RequestResponse::ShuttingDown);
                break;
            }
        }
    }
}
