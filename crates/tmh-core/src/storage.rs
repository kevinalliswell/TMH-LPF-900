//! SQLite persistence (docs/06 §6.2). rusqlite with a bundled (statically linked)
//! SQLite so the offline工控机 needs no DLLs; WAL journaling; `PRAGMA user_version`
//! migrations. This is the DB layer — the single-writer task that funnels sampler
//! rows through it is wired in M2 slice 4.

use rusqlite::{params, Connection};
use std::path::Path;

const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS experiments (
    experiment_id          TEXT PRIMARY KEY,
    experiment_name        TEXT NOT NULL,
    sample_name            TEXT NOT NULL,
    sample_weight          REAL NOT NULL,
    start_time             TEXT NOT NULL,
    end_time               TEXT,
    description            TEXT,
    operator               TEXT,
    experiment_type        TEXT,
    analysis_results_json  TEXT,
    created_at             TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS experiment_data (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    experiment_id       TEXT NOT NULL REFERENCES experiments(experiment_id),
    timestamp           TEXT NOT NULL,
    experiment_duration TEXT,
    temperature         REAL NOT NULL,
    weight              REAL NOT NULL,
    weight_loss         REAL NOT NULL,
    co_flow             REAL NOT NULL,
    co2_flow            REAL NOT NULL,
    n2_flow             REAL NOT NULL,
    h2_flow             REAL NOT NULL,
    experiment_status   TEXT,
    system_message      TEXT
);
CREATE INDEX IF NOT EXISTS idx_experiments_name ON experiments(experiment_name);
CREATE INDEX IF NOT EXISTS idx_experiment_data_ts ON experiment_data(experiment_id, timestamp);
"#;

/// Storage error (wraps rusqlite).
#[derive(Debug, thiserror::Error)]
#[error("storage: {0}")]
pub struct StorageError(#[from] rusqlite::Error);

pub type Result<T> = std::result::Result<T, StorageError>;

/// An `experiments` row.
#[derive(Debug, Clone)]
pub struct ExperimentRow {
    pub experiment_id: String,
    pub experiment_name: String,
    pub sample_name: String,
    pub sample_weight: f64,
    pub start_time: String,
    pub end_time: Option<String>,
    pub description: Option<String>,
    pub operator: Option<String>,
    pub experiment_type: Option<String>,
    pub analysis_results_json: Option<String>,
    pub created_at: String,
}

/// An `experiment_data` sample row.
#[derive(Debug, Clone)]
pub struct SampleRow {
    pub experiment_id: String,
    pub timestamp: String,
    pub experiment_duration: Option<String>,
    pub temperature: f64,
    pub weight: f64,
    pub weight_loss: f64,
    pub co_flow: f64,
    pub co2_flow: f64,
    pub n2_flow: f64,
    pub h2_flow: f64,
    pub experiment_status: Option<String>,
    pub system_message: Option<String>,
}

/// Owns the SQLite connection. Single-writer by construction — hold one and route
/// all writes through it (docs/03).
pub struct Storage {
    conn: Connection,
}

impl Storage {
    /// Open (creating if needed) the database at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    /// Open a throwaway in-memory database (tests).
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        // WAL is a no-op for in-memory DBs but harmless.
        let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
        conn.execute_batch(SCHEMA)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < SCHEMA_VERSION {
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(Self { conn })
    }

    /// The schema version stamped in `PRAGMA user_version`.
    pub fn user_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    /// The active journal mode (e.g. "wal" for on-disk databases).
    pub fn journal_mode(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))?)
    }

    /// Insert an experiment metadata row.
    pub fn insert_experiment(&self, e: &ExperimentRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO experiments (experiment_id, experiment_name, sample_name, sample_weight, \
             start_time, end_time, description, operator, experiment_type, analysis_results_json, \
             created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                e.experiment_id,
                e.experiment_name,
                e.sample_name,
                e.sample_weight,
                e.start_time,
                e.end_time,
                e.description,
                e.operator,
                e.experiment_type,
                e.analysis_results_json,
                e.created_at,
            ],
        )?;
        Ok(())
    }

    /// Insert a batch of sample rows inside one transaction (the storage writer's
    /// unit of work).
    pub fn insert_samples(&mut self, rows: &[SampleRow]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO experiment_data (experiment_id, timestamp, experiment_duration, \
                 temperature, weight, weight_loss, co_flow, co2_flow, n2_flow, h2_flow, \
                 experiment_status, system_message) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            )?;
            for r in rows {
                stmt.execute(params![
                    r.experiment_id,
                    r.timestamp,
                    r.experiment_duration,
                    r.temperature,
                    r.weight,
                    r.weight_loss,
                    r.co_flow,
                    r.co2_flow,
                    r.n2_flow,
                    r.h2_flow,
                    r.experiment_status,
                    r.system_message,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Count sample rows for an experiment.
    pub fn count_samples(&self, experiment_id: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM experiment_data WHERE experiment_id = ?1",
            params![experiment_id],
            |r| r.get(0),
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn experiment(id: &str) -> ExperimentRow {
        ExperimentRow {
            experiment_id: id.into(),
            experiment_name: "exp".into(),
            sample_name: "s1".into(),
            sample_weight: 100.0,
            start_time: "2026-07-05T10:00:00".into(),
            end_time: None,
            description: None,
            operator: Some("op".into()),
            experiment_type: Some("REDUCIBILITY".into()),
            analysis_results_json: None,
            created_at: "2026-07-05T10:00:00".into(),
        }
    }

    fn sample(id: &str, t: f64) -> SampleRow {
        SampleRow {
            experiment_id: id.into(),
            timestamp: "2026-07-05T10:00:01".into(),
            experiment_duration: Some("00:00:01".into()),
            temperature: t,
            weight: 99.0,
            weight_loss: 1.0,
            co_flow: 4.5,
            co2_flow: 0.0,
            n2_flow: 10.5,
            h2_flow: 0.0,
            experiment_status: Some("RUNNING".into()),
            system_message: None,
        }
    }

    #[test]
    fn schema_inits_at_version_1() {
        let s = Storage::open_in_memory().unwrap();
        assert_eq!(s.user_version().unwrap(), 1);
    }

    #[test]
    fn insert_experiment_and_batch_samples() {
        let mut s = Storage::open_in_memory().unwrap();
        s.insert_experiment(&experiment("e1")).unwrap();
        let rows: Vec<SampleRow> = (0..100).map(|i| sample("e1", 250.0 + i as f64)).collect();
        s.insert_samples(&rows).unwrap();
        assert_eq!(s.count_samples("e1").unwrap(), 100);
    }

    #[test]
    fn on_disk_db_uses_wal() {
        let dir = std::env::temp_dir().join(format!("tmh_db_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("experiments.db");
        let s = Storage::open(&path).unwrap();
        assert_eq!(s.journal_mode().unwrap().to_lowercase(), "wal");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reopen_keeps_version_and_data() {
        let dir = std::env::temp_dir().join(format!("tmh_db2_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("experiments.db");
        {
            let mut s = Storage::open(&path).unwrap();
            s.insert_experiment(&experiment("e2")).unwrap();
            s.insert_samples(&[sample("e2", 300.0)]).unwrap();
        }
        let s = Storage::open(&path).unwrap();
        assert_eq!(s.user_version().unwrap(), 1);
        assert_eq!(s.count_samples("e2").unwrap(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
