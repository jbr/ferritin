//! Request thread - handles Navigator operations and document formatting

use super::channels::{RequestResponse, UiCommand};
use super::history::HistoryEntry;
use crate::commands::{list, search};
use crate::{document::Document, request::Request};
use crossbeam_channel::{Receiver, Sender};

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
                let doc = Document::from(doc_nodes)
                    .with_history_entry(HistoryEntry::Item(doc_ref))
                    .with_item(doc_ref);

                let _ = resp_tx.send(RequestResponse::Document(doc));
            }

            UiCommand::NavigateToPath(path) => {
                let mut suggestions = vec![];
                if let Some(item) = request.resolve_path(path.as_ref(), &mut suggestions) {
                    let doc_nodes = request.format_item(item);
                    let doc = Document::from(doc_nodes)
                        .with_item(item)
                        .with_history_entry(HistoryEntry::Item(item));

                    let _ = resp_tx.send(RequestResponse::Document(doc));
                } else {
                    let _ = resp_tx.send(RequestResponse::Error(format!("Not found: {}", path)));
                }
            }

            UiCommand::Search {
                query,
                crate_name,
                limit,
            } => {
                let search_doc = search::execute(
                    request,
                    query.as_ref(),
                    limit,
                    crate_name.as_ref().map(|c| c.as_ref()),
                );

                let _ = resp_tx.send(RequestResponse::Document(search_doc));
            }

            UiCommand::List => {
                let doc = list::execute(request);
                let _ = resp_tx.send(RequestResponse::Document(doc));
            }

            UiCommand::ToggleSource {
                include_source,
                current_item,
            } => {
                request.format_context().set_include_source(include_source);
                if let Some(current_item) = current_item {
                    let _ = resp_tx.send(RequestResponse::Document(Document::from(
                        request.format_item(current_item),
                    )));
                }
            }

            UiCommand::Shutdown => {
                let _ = resp_tx.send(RequestResponse::ShuttingDown);
                break;
            }
        }
    }
}
