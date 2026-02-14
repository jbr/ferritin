use crate::document::{Document, DocumentNode, HeadingLevel, ListItem, Span};
use crate::logging::LogEntry;
use crate::renderer::interactive::InteractiveState;
use log::Level;
use std::fs::File;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

impl<'a> InteractiveState<'a> {
    /// Create a document showing the debug log
    pub(super) fn create_dev_log_document(&self) -> Document<'static> {
        let history = self.log_reader.snapshot_history();

        if history.is_empty() {
            return Document::from(vec![
                DocumentNode::heading(
                    HeadingLevel::Title,
                    vec![Span::plain("Debug Log (Ctrl+L to close)")],
                ),
                DocumentNode::paragraph(vec![Span::plain("No log entries yet.")]),
            ]);
        }

        let mut last_ts = history[0].timestamp;

        let items: Vec<ListItem<'static>> = history
            .iter()
            .map(|entry| {
                let elapsed_time = entry.timestamp.duration_since(last_ts);
                last_ts = entry.timestamp;

                // Color-code log level
                let level_span = match entry.level {
                    Level::Error => Span::strong("[ERROR] "),
                    Level::Warn => Span::emphasis("[WARN]  "),
                    Level::Info => Span::type_name("[INFO]  "),
                    Level::Debug => Span::comment("[DEBUG] "),
                    Level::Trace => Span::comment("[TRACE] "),
                };

                ListItem::new(vec![DocumentNode::paragraph(vec![
                    Span::plain(format!("+{elapsed_time:?} ")),
                    level_span,
                    Span::plain(entry.message.clone()),
                ])])
            })
            .collect();

        Document::from(vec![
            DocumentNode::heading(
                HeadingLevel::Title,
                vec![Span::plain(format!(
                    "Debug Log ({} entries) - Ctrl+L to close",
                    items.len()
                ))],
            ),
            DocumentNode::list(items),
        ])
    }

    /// Dump logs to a file in the current directory
    /// Returns the filename on success
    pub(super) fn dump_logs_to_disk(&self) -> Result<String, std::io::Error> {
        let history = self.log_reader.snapshot_history();

        // Generate filename with timestamp
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let filename = format!("ferritin-{}.log", timestamp);

        let mut file = File::create(&filename)?;

        if history.is_empty() {
            writeln!(file, "No log entries")?;
            return Ok(filename);
        }

        let mut last_ts = history[0].timestamp;

        writeln!(file, "Ferritin Debug Log")?;
        writeln!(file, "==================")?;
        writeln!(file)?;

        for LogEntry {
            timestamp,
            level,
            target,
            message,
        } in &history
        {
            let elapsed_time = timestamp.duration_since(last_ts);
            last_ts = *timestamp;

            let level_str = match level {
                Level::Error => "ERROR",
                Level::Warn => "WARN ",
                Level::Info => "INFO ",
                Level::Debug => "DEBUG",
                Level::Trace => "TRACE",
            };

            writeln!(file, "+{elapsed_time:?} [{level_str}] {target}: {message}",)?;
        }

        Ok(filename)
    }
}
