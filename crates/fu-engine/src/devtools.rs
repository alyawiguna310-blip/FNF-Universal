//! Read-only in-game developer log model for the F8 console.
//!
//! This module stores diagnostic messages only. It intentionally exposes no
//! command parser, script evaluator, or gameplay mutation API.

use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub timestamp_ms: u64,
    pub level: LogLevel,
    pub category: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct LogBuffer {
    entries: VecDeque<LogEntry>,
    capacity: usize,
}

impl Default for LogBuffer {
    fn default() -> Self { Self::new(2_000) }
}

impl LogBuffer {
    pub fn new(capacity: usize) -> Self {
        Self { entries: VecDeque::new(), capacity: capacity.max(1) }
    }

    /// Append a diagnostic entry, evicting the oldest entry at capacity.
    pub fn push(&mut self, entry: LogEntry) {
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    pub fn entries(&self) -> impl Iterator<Item = &LogEntry> {
        self.entries.iter()
    }

    pub fn entries_at_level(&self, level: LogLevel) -> impl Iterator<Item = &LogEntry> {
        self.entries.iter().filter(move |entry| entry.level == level)
    }

    pub fn clear(&mut self) { self.entries.clear(); }

    pub fn len(&self) -> usize { self.entries.len() }

    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(timestamp_ms: u64, message: &str) -> LogEntry {
        LogEntry { timestamp_ms, level: LogLevel::Info, category: "engine".into(), message: message.into() }
    }

    #[test]
    fn buffer_keeps_newest_entries_within_capacity() {
        let mut logs = LogBuffer::new(2);
        logs.push(entry(1, "one"));
        logs.push(entry(2, "two"));
        logs.push(entry(3, "three"));
        let messages: Vec<_> = logs.entries().map(|entry| entry.message.as_str()).collect();
        assert_eq!(messages, vec!["two", "three"]);
    }

    #[test]
    fn clear_removes_all_entries() {
        let mut logs = LogBuffer::default();
        logs.push(entry(1, "startup"));
        logs.clear();
        assert!(logs.is_empty());
    }

    #[test]
    fn level_filter_only_returns_matching_entries() {
        let mut logs = LogBuffer::default();
        logs.push(entry(1, "info"));
        let mut warning = entry(2, "warning");
        warning.level = LogLevel::Warn;
        logs.push(warning);
        assert_eq!(logs.entries_at_level(LogLevel::Warn).count(), 1);
    }
}
