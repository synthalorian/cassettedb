//! Beta testing feedback integration for CassetteDB.
//!
//! Collects structured feedback (bugs, feature requests, general notes)
//! and stores them locally.  In a production system this could upload
//! to a telemetry endpoint; for now we append to a local JSONL file.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use anyhow::{Context, Result};

/// Categories of user feedback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FeedbackCategory {
    Bug,
    FeatureRequest,
    Performance,
    Documentation,
    General,
}

/// A single feedback entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackEntry {
    pub timestamp: String,
    pub category: FeedbackCategory,
    pub message: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,
}

/// Append feedback to the local feedback log.
pub fn submit_feedback(
    log_path: &Path,
    category: FeedbackCategory,
    message: String,
    version: String,
    contact: Option<String>,
) -> Result<()> {
    let entry = FeedbackEntry {
        timestamp: Utc::now().to_rfc3339(),
        category,
        message,
        version,
        contact,
    };

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .with_context(|| format!("opening feedback log {}", log_path.display()))?;

    let line = serde_json::to_string(&entry)?;
    writeln!(file, "{}", line)
        .with_context(|| format!("writing feedback to {}", log_path.display()))?;

    Ok(())
}

/// Read all feedback entries from the local log.
pub fn read_feedback(log_path: &Path) -> Result<Vec<FeedbackEntry>> {
    if !log_path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(log_path)
        .with_context(|| format!("reading feedback log {}", log_path.display()))?;
    let mut entries = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<FeedbackEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(e) => eprintln!("Warning: skipping malformed feedback line: {}", e),
        }
    }
    Ok(entries)
}
