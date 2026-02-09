//! Logging infrastructure for ferritin interactive mode
//!
//! Provides a log backend that captures logs from ferritin-common and makes them
//! available for display in the TUI status bar and dev log screen.

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// A single log entry
#[derive(Clone, Debug)]
pub struct LogEntry {
    pub timestamp: Instant,
    pub level: Level,
    pub target: String,
    pub message: String,
}

/// Shared state between log backend and readers
#[derive(Debug)]
struct LogState {
    /// Latest message for status bar
    latest_status: Option<String>,

    max_level: LevelFilter,
    max_status_level: LevelFilter,

    /// Full history for dev log (with capacity limit)
    history: VecDeque<LogEntry>,
    max_history: usize,
}

/// Log backend that implements log::Log
pub struct StatusLogBackend {
    state: Arc<Mutex<LogState>>,

    /// Optional notification channel for reactive updates
    /// If Some, sends a non-blocking notification when logs arrive
    notify_tx: Option<Sender<()>>,
}

impl StatusLogBackend {
    /// Create a new log backend with a given history size
    ///
    /// Returns the backend (to install) and a reader (to consume logs)
    pub fn new(max_history: usize) -> (Self, LogReader) {
        let state = Arc::new(Mutex::new(LogState {
            latest_status: None,
            history: VecDeque::new(),
            max_history,
            max_level: LevelFilter::Debug,
            max_status_level: LevelFilter::Info,
        }));

        // Bounded channel with capacity 1 - we only care that "something changed"
        // Multiple log calls will coalesce into a single notification
        let (notify_tx, notify_rx) = crossbeam_channel::bounded(1);

        let backend = Self {
            state: state.clone(),
            notify_tx: Some(notify_tx),
        };

        let reader = LogReader { state, notify_rx };

        (backend, reader)
    }

    /// Install this backend as the global logger
    pub fn install(self) -> Result<(), SetLoggerError> {
        log::set_max_level(self.state.lock().unwrap().max_level);
        log::set_boxed_logger(Box::new(self))?;
        Ok(())
    }
}

impl Log for StatusLogBackend {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.state.lock().unwrap().max_level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let entry = LogEntry {
            timestamp: Instant::now(),
            level: record.level(),
            target: record.target().to_string(),
            message: format!("{}", record.args()),
        };

        let mut state = self.state.lock().unwrap();

        if record.level() <= state.max_status_level {
            state.latest_status = Some(entry.message.clone());
        }

        // Append to history (with capacity limit)
        state.history.push_back(entry);
        if state.history.len() > state.max_history {
            state.history.pop_front();
        }

        // Notify (non-blocking - if channel is full, just drop the notification)
        if let Some(tx) = &self.notify_tx {
            let _ = tx.try_send(());
        }
    }

    fn flush(&self) {}
}

/// Reader handle for consuming logs from UI thread
#[derive(Debug)]
pub struct LogReader {
    state: Arc<Mutex<LogState>>,
    notify_rx: Receiver<()>,
}

impl LogReader {
    /// Peek at the latest status message (non-consuming)
    /// Returns None if no INFO+ messages have been logged yet
    pub fn peek_latest(&self) -> Option<String> {
        self.state.lock().unwrap().latest_status.clone()
    }

    /// Get a snapshot of current history (non-consuming)
    /// Returns all accumulated log entries
    pub fn snapshot_history(&self) -> Vec<LogEntry> {
        self.state.lock().unwrap().history.iter().cloned().collect()
    }

    /// Get notification receiver for reactive updates
    /// UI thread can use this in event::poll or select! to be notified of new logs
    pub fn notify_receiver(&self) -> &Receiver<()> {
        &self.notify_rx
    }

    /// Try to receive a notification (non-blocking)
    /// Returns Ok(()) if there are new logs, Err if no notification pending
    pub fn try_recv_notification(&self) -> Result<(), TryRecvError> {
        self.notify_rx.try_recv()
    }
}
