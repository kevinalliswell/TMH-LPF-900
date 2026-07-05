//! Structured audit log (docs/06 §6.5) — independent of the diagnostic tracing
//! pipeline. One JSON object per line; append-only; date-rotated files
//! (`audit.log.YYYY-MM-DD`, 180-day retention). **Writes never throw into control
//! flow** — I/O failures are counted, not propagated (safety invariant, docs/05 §5.5).

use serde::Serialize;
use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Audit category (docs/06 §6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Category {
    App,
    Experiment,
    Gas,
    Balance,
    Export,
    Auth,
    Config,
}

impl Category {
    /// GAS/EXPERIMENT get an extra fsync (power-loss protection).
    fn needs_sync(self) -> bool {
        matches!(self, Category::Gas | Category::Experiment)
    }
}

/// Outcome of the audited action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AuditResult {
    Success,
    Failure,
    Rejected,
}

/// One audit record. Field order matches docs/06 §6.5.
#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
    pub ts: String,
    pub category: Category,
    pub action: String,
    pub result: AuditResult,
    pub operator: String,
    #[serde(skip_serializing_if = "Value::is_null")]
    pub detail: Value,
}

impl AuditEntry {
    /// New entry stamped with the current local time; operator defaults to "system".
    pub fn new(category: Category, action: impl Into<String>, result: AuditResult) -> Self {
        Self {
            ts: now_ts(),
            category,
            action: action.into(),
            result,
            operator: "system".into(),
            detail: Value::Null,
        }
    }

    /// Set the operator (empty falls back to "system").
    pub fn operator(mut self, op: impl Into<String>) -> Self {
        let o = op.into();
        if !o.trim().is_empty() {
            self.operator = o;
        }
        self
    }

    /// Attach a detail object; null-valued keys are filtered out.
    pub fn detail(mut self, d: Value) -> Self {
        self.detail = filter_nulls(d);
        self
    }

    /// Override the timestamp (deterministic tests / imported records).
    pub fn at(mut self, ts: impl Into<String>) -> Self {
        self.ts = ts.into();
        self
    }

    fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// Append-only audit sink over a directory of date-rotated files.
pub struct AuditLog {
    dir: PathBuf,
    failures: AtomicU64,
}

impl AuditLog {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            failures: AtomicU64::new(0),
        }
    }

    /// Append `entry`. NEVER throws — an I/O failure bumps `failure_count()`.
    pub fn write(&self, entry: &AuditEntry) {
        if self.try_write(entry).is_err() {
            self.failures.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn try_write(&self, entry: &AuditEntry) -> std::io::Result<()> {
        let date = entry.ts.get(0..10).unwrap_or("unknown");
        let path = self.dir.join(format!("audit.log.{date}"));
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(f, "{}", entry.to_line())?;
        f.flush()?;
        if entry.category.needs_sync() {
            f.sync_data()?;
        }
        Ok(())
    }

    /// Count of writes that failed (and were swallowed).
    pub fn failure_count(&self) -> u64 {
        self.failures.load(Ordering::Relaxed)
    }

    /// Delete `audit.log.YYYY-MM-DD` files older than `retain_days` from today.
    pub fn prune(&self, retain_days: i64) {
        let cutoff = chrono::Local::now().date_naive() - chrono::Duration::days(retain_days);
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for e in entries.flatten() {
            let name = e.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(date_str) = name.strip_prefix("audit.log.") else {
                continue;
            };
            if let Ok(date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                if date < cutoff {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
}

fn now_ts() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn filter_nulls(v: Value) -> Value {
    match v {
        Value::Object(map) => {
            Value::Object(map.into_iter().filter(|(_, val)| !val.is_null()).collect())
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn line_format_matches_spec() {
        let e = AuditEntry::new(Category::Gas, "set_flow", AuditResult::Success)
            .operator("张三")
            .at("2026-07-02 15:04:05")
            .detail(json!({"gas": "H2", "value": 3.0}));
        assert_eq!(
            e.to_line(),
            r#"{"ts":"2026-07-02 15:04:05","category":"GAS","action":"set_flow","result":"SUCCESS","operator":"张三","detail":{"gas":"H2","value":3.0}}"#
        );
    }

    #[test]
    fn detail_nulls_are_filtered() {
        let e = AuditEntry::new(Category::Config, "update", AuditResult::Success)
            .at("2026-07-02 00:00:00")
            .detail(json!({"a": 1, "b": null}));
        assert!(!e.to_line().contains("\"b\""));
        assert!(e.to_line().contains("\"a\":1"));
    }

    #[test]
    fn empty_operator_defaults_to_system() {
        let e = AuditEntry::new(Category::App, "start", AuditResult::Success).operator("");
        assert_eq!(e.operator, "system");
    }

    #[test]
    fn writes_rotate_by_date_and_append() {
        let dir = std::env::temp_dir().join(format!("tmh_audit_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = AuditLog::new(&dir);
        log.write(
            &AuditEntry::new(Category::App, "a", AuditResult::Success).at("2026-07-02 01:00:00"),
        );
        log.write(
            &AuditEntry::new(Category::App, "b", AuditResult::Success).at("2026-07-02 02:00:00"),
        );
        log.write(
            &AuditEntry::new(Category::App, "c", AuditResult::Success).at("2026-07-03 01:00:00"),
        );
        let d2 = std::fs::read_to_string(dir.join("audit.log.2026-07-02")).unwrap();
        assert_eq!(d2.lines().count(), 2, "same-day entries append to one file");
        assert!(dir.join("audit.log.2026-07-03").exists());
        assert_eq!(log.failure_count(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_failure_is_swallowed_and_counted() {
        // Point the log at a regular file so opening a child path fails (ENOTDIR).
        let not_a_dir = std::env::temp_dir().join(format!("tmh_audit_file_{}", std::process::id()));
        std::fs::write(&not_a_dir, b"x").unwrap();
        let log = AuditLog::new(&not_a_dir);
        log.write(
            &AuditEntry::new(Category::Gas, "set_flow", AuditResult::Success)
                .at("2026-07-02 00:00:00"),
        );
        assert_eq!(log.failure_count(), 1, "io failure counted, not thrown");
        std::fs::remove_file(&not_a_dir).ok();
    }
}
