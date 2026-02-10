use crate::renderer::interactive::UiMode;

use super::InteractiveState;
use super::channels::RequestResponse;

impl<'a> InteractiveState<'a> {
    /// Handle log updates from the log reader (non-blocking)
    /// Updates status bar with latest INFO+ log message
    pub fn handle_log_updates(&mut self) {
        // Try to receive notification (non-blocking)
        if self.log_reader.try_recv_notification().is_ok() {
            // There are new logs, peek at the latest status message
            if let Some(latest) = self.log_reader.peek_latest() {
                // Only update if we're in normal mode (don't override input mode prompts)
                if matches!(self.ui_mode, UiMode::Normal) {
                    self.ui.debug_message = latest.into();
                }
            }
        }
    }

    /// Handle a single response from the request thread
    /// Returns true if the UI should exit
    pub fn handle_response(&mut self, response: RequestResponse<'a>) -> bool {
        self.loading.pending_request = false;
        match response {
            RequestResponse::Document { doc, entry } => {
                self.document.document = doc;
                self.viewport.scroll_offset = 0;

                // Add to history if we got an entry
                if let Some(new_entry) = entry {
                    self.document.history.push(new_entry);
                }
                false
            }

            RequestResponse::Error(err) => {
                self.ui.debug_message = err.into();
                false
            }

            RequestResponse::ShuttingDown => true,
        }
    }
}
