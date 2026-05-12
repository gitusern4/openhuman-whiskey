//! Append-only JSONL audit writer — §7 of EXECUTION_LAYER_RESEARCH.md.
//!
//! Opens `<openhuman_dir>/audit/audit-YYYY-MM-DD.jsonl` in append-only mode.
//! Each `record()` call serializes one `AuditEntry`, writes a newline-terminated
//! JSON line, then calls `sync_all()` for durability. Correctness beats latency
//! on the audit path.
//!
//! Rotates automatically when the calendar date crosses midnight — the writer
//! checks the current date on every `record()` call and reopens if needed.
//!
//! The audit file is the only place where account numbers and position size
//! appear in full. Console logging of sensitive fields is prohibited.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

/// Every execution event that Whiskey can emit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    Proposal,
    ProposalRejected,
    Confirm,
    Send,
    Fill,
    PartialFill,
    Cancel,
    Kill,
    SessionStart,
    SessionEnd,
    CovenantCheck,
}

/// Every participant that can produce an audit event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuditActor {
    Whiskey,
    User,
    System,
}

/// One line in the JSONL audit file. All optional fields match broker response
/// availability — e.g. `broker_response` is `None` for proposal events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp_utc: DateTime<Utc>,
    pub actor: AuditActor,
    pub action: AuditAction,
    pub instrument: Option<String>,
    pub qty: Option<u32>,
    /// Entry price (limit) or last price at submission.
    pub price: Option<f64>,
    pub stop: Option<f64>,
    pub target: Option<f64>,
    pub r_estimate: Option<f64>,
    pub confidence_pct: Option<u8>,
    pub playbook_match_id: Option<String>,
    /// UUID v4 per order attempt — ties proposal → confirm → send → fill.
    pub idempotency_key: Option<String>,
    pub broker_response: Option<serde_json::Value>,
    pub session_loss_count: Option<u32>,
    pub daily_pnl_at_action: Option<f64>,
    pub kill_engaged: bool,
    /// Free text, LLM reasoning summary, max 500 chars.
    pub notes: Option<String>,
    /// Covenant hash at the time of this event.
    pub covenant_hash: Option<String>,
}

impl AuditEntry {
    pub fn new_minimal(actor: AuditActor, action: AuditAction) -> Self {
        Self {
            timestamp_utc: Utc::now(),
            actor,
            action,
            instrument: None,
            qty: None,
            price: None,
            stop: None,
            target: None,
            r_estimate: None,
            confidence_pct: None,
            playbook_match_id: None,
            idempotency_key: None,
            broker_response: None,
            session_loss_count: None,
            daily_pnl_at_action: None,
            kill_engaged: false,
            notes: None,
            covenant_hash: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

/// Append-only JSONL writer with daily rotation.
pub struct AuditWriter {
    dir: PathBuf,
    current_date: chrono::NaiveDate,
    file: std::fs::File,
}

impl AuditWriter {
    /// Open (or create) today's audit file. The `audit/` directory is created
    /// if it does not exist.
    pub fn open(openhuman_dir: &Path) -> Result<Self, String> {
        let dir = openhuman_dir.join("audit");
        fs::create_dir_all(&dir)
            .map_err(|e| format!("failed to create audit dir {}: {}", dir.display(), e))?;
        let today = Utc::now().date_naive();
        let file = Self::open_day_file(&dir, today)?;
        Ok(Self {
            dir,
            current_date: today,
            file,
        })
    }

    fn day_path(dir: &Path, date: chrono::NaiveDate) -> PathBuf {
        dir.join(format!("audit-{}.jsonl", date.format("%Y-%m-%d")))
    }

    fn open_day_file(dir: &Path, date: chrono::NaiveDate) -> Result<std::fs::File, String> {
        let path = Self::day_path(dir, date);
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("failed to open audit file {}: {}", path.display(), e))
    }

    /// Serialize `entry` → JSON line → append → fsync.
    /// Rotates to a new day-file when the calendar date has advanced.
    pub fn record(&mut self, entry: &AuditEntry) -> Result<(), String> {
        // Rotate if date has advanced.
        let today = Utc::now().date_naive();
        if today != self.current_date {
            self.file = Self::open_day_file(&self.dir, today)?;
            self.current_date = today;
        }
        let mut line =
            serde_json::to_string(entry).map_err(|e| format!("audit serialize error: {}", e))?;
        line.push('\n');
        self.file
            .write_all(line.as_bytes())
            .map_err(|e| format!("audit write error: {}", e))?;
        self.file
            .sync_all()
            .map_err(|e| format!("audit fsync error: {}", e))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;

    fn sample_entry() -> AuditEntry {
        AuditEntry {
            timestamp_utc: Utc::now(),
            actor: AuditActor::Whiskey,
            action: AuditAction::Proposal,
            instrument: Some("MESH5".to_string()),
            qty: Some(1),
            price: Some(5200.25),
            stop: Some(5195.50),
            target: Some(5210.00),
            r_estimate: Some(125.0),
            confidence_pct: Some(78),
            playbook_match_id: Some("breakout-orb-v2".to_string()),
            idempotency_key: Some(uuid::Uuid::new_v4().to_string()),
            broker_response: None,
            session_loss_count: Some(0),
            daily_pnl_at_action: Some(0.0),
            kill_engaged: false,
            notes: Some("test entry".to_string()),
            covenant_hash: Some("abc123".to_string()),
        }
    }

    #[test]
    fn record_and_read_back() {
        let tmp = tempfile::tempdir().unwrap();
        let mut writer = AuditWriter::open(tmp.path()).unwrap();
        let entry = sample_entry();
        writer.record(&entry).unwrap();

        // Read back and verify
        let today = Utc::now().date_naive();
        let path = tmp
            .path()
            .join("audit")
            .join(format!("audit-{}.jsonl", today.format("%Y-%m-%d")));
        let f = std::fs::File::open(&path).unwrap();
        let lines: Vec<String> = std::io::BufReader::new(f)
            .lines()
            .map(|l| l.unwrap())
            .collect();
        assert_eq!(lines.len(), 1);
        let parsed: AuditEntry = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(parsed.instrument, Some("MESH5".to_string()));
        assert_eq!(parsed.qty, Some(1));
        assert!(!parsed.kill_engaged);
    }

    #[test]
    fn multiple_records_accumulate() {
        let tmp = tempfile::tempdir().unwrap();
        let mut writer = AuditWriter::open(tmp.path()).unwrap();
        for _ in 0..5 {
            writer.record(&sample_entry()).unwrap();
        }
        let today = Utc::now().date_naive();
        let path = tmp
            .path()
            .join("audit")
            .join(format!("audit-{}.jsonl", today.format("%Y-%m-%d")));
        let f = std::fs::File::open(&path).unwrap();
        let count = std::io::BufReader::new(f).lines().count();
        assert_eq!(count, 5);
    }

    #[test]
    fn audit_dir_created_automatically() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a").join("b").join("c");
        // AuditWriter creates the dir
        // But only one level is guaranteed by create_dir_all.
        // Use tmp.path() directly.
        let result = AuditWriter::open(tmp.path());
        assert!(result.is_ok());
        assert!(tmp.path().join("audit").is_dir());
    }

    #[test]
    fn file_is_append_only_mode() {
        // Verify two separate AuditWriter::open calls both append to the same file.
        let tmp = tempfile::tempdir().unwrap();
        {
            let mut w = AuditWriter::open(tmp.path()).unwrap();
            w.record(&sample_entry()).unwrap();
        }
        {
            let mut w = AuditWriter::open(tmp.path()).unwrap();
            w.record(&sample_entry()).unwrap();
        }
        let today = Utc::now().date_naive();
        let path = tmp
            .path()
            .join("audit")
            .join(format!("audit-{}.jsonl", today.format("%Y-%m-%d")));
        let f = std::fs::File::open(&path).unwrap();
        let count = std::io::BufReader::new(f).lines().count();
        assert_eq!(count, 2);
    }

    #[test]
    fn each_line_is_valid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let mut writer = AuditWriter::open(tmp.path()).unwrap();
        writer.record(&sample_entry()).unwrap();
        let today = Utc::now().date_naive();
        let path = tmp
            .path()
            .join("audit")
            .join(format!("audit-{}.jsonl", today.format("%Y-%m-%d")));
        let content = std::fs::read_to_string(&path).unwrap();
        for line in content.lines() {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(v.is_object());
        }
    }

    #[test]
    fn minimal_entry_serializes_without_panic() {
        let tmp = tempfile::tempdir().unwrap();
        let mut writer = AuditWriter::open(tmp.path()).unwrap();
        let entry = AuditEntry::new_minimal(AuditActor::System, AuditAction::SessionStart);
        assert!(writer.record(&entry).is_ok());
    }
}
