use super::channels::RequestResponse;

use super::InteractiveState;

impl<'a> InteractiveState<'a> {
    pub fn handle_messages(&mut self) -> bool {
        // Check for responses from request thread (non-blocking)
        while let Ok(response) = self.resp_rx.try_recv() {
            self.loading.pending_request = false;
            match response {
                RequestResponse::Document { doc, entry } => {
                    self.document.document = doc;
                    self.viewport.scroll_offset = 0;

                    // Add to history if we got an entry
                    if let Some(new_entry) = entry {
                        self.document.history.push(new_entry);
                        if let Some(history_entry) = self.document.history.current() {
                            self.ui.debug_message = format!("Loaded: {history_entry}",);
                        }
                    }
                }

                RequestResponse::Error(err) => {
                    self.ui.debug_message = err;
                }

                RequestResponse::ShuttingDown => return true,
            }
        }
        false
    }
}
