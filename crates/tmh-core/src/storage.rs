//! SQLite persistence (docs/06 §6.2). rusqlite with a bundled (statically linked)
//! SQLite so the offline工控机 needs no DLLs; WAL journaling; `PRAGMA user_version`
//! migrations. This is the DB layer — the single-writer task that funnels sampler
//! rows through it is wired in M2 slice 4.

use crate::auth::UserRole;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

const SCHEMA_VERSION: i64 = 7;

const SCHEMA_V1: &str = r#"
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

const MIGRATION_V2: &str = r#"
CREATE TABLE users (
    user_id                 TEXT PRIMARY KEY,
    username                TEXT NOT NULL UNIQUE COLLATE NOCASE,
    display_name            TEXT NOT NULL,
    password_hash           TEXT NOT NULL,
    role                    TEXT NOT NULL CHECK(role IN ('ADMIN', 'USER')),
    is_active               INTEGER NOT NULL DEFAULT 1 CHECK(is_active IN (0, 1)),
    must_change_password    INTEGER NOT NULL DEFAULT 1 CHECK(must_change_password IN (0, 1)),
    failed_login_count      INTEGER NOT NULL DEFAULT 0,
    locked_until            TEXT,
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL,
    password_changed_at     TEXT NOT NULL,
    last_login_at           TEXT
);
ALTER TABLE experiments ADD COLUMN owner_user_id TEXT REFERENCES users(user_id) ON DELETE RESTRICT;
CREATE INDEX idx_users_role_active ON users(role, is_active);
CREATE INDEX idx_experiments_owner_start ON experiments(owner_user_id, start_time DESC);
"#;

/// Storage error (wraps rusqlite).
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("invalid or conflicting experiment completion")]
    InvalidCompletion,
    #[error("custom program changed or revision is invalid; reload before saving")]
    ProgramConflict,
    #[error("invalid stored JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("storage: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("unsupported database schema version {0}")]
    UnsupportedVersion(i64),
}

pub type Result<T> = std::result::Result<T, StorageError>;

/// An `experiments` row.
#[derive(Debug, Clone)]
pub struct ExperimentRow {
    pub sample_id: Option<String>,
    pub project_name: Option<String>,
    pub experiment_time: Option<String>,
    pub program_snapshot_json: Option<String>,
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
    pub owner_user_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UserRow {
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    pub password_hash: String,
    pub role: UserRole,
    pub is_active: bool,
    pub must_change_password: bool,
    pub failed_login_count: i64,
    pub locked_until: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub password_changed_at: String,
    pub last_login_at: Option<String>,
}

/// An `experiment_data` sample row.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SampleRow {
    pub record_json: Option<String>,
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

/// Final samples and terminal evidence are one atomic, replayable unit.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunCompletion {
    pub token: String,
    pub experiment_id: String,
    pub end_time: String,
    pub metadata: serde_json::Value,
    pub rows: Vec<SampleRow>,
}

fn insert_sample_rows(conn: &Connection, rows: &[SampleRow]) -> Result<()> {
    {
        let mut stmt = conn.prepare(
                "INSERT INTO experiment_data (experiment_id, timestamp, experiment_duration, \
                 temperature, weight, weight_loss, co_flow, co2_flow, n2_flow, h2_flow, \
                 experiment_status, system_message, record_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
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
                r.record_json,
            ])?;
        }
    }
    Ok(())
}

/// NULL means no authenticated revision number. Row ids preserve the order of
/// legacy edits without inventing authors, dates or formal revision numbers.
fn retain_unversioned_program(conn: &Connection, id: &str, json: &str) -> Result<()> {
    let latest: Option<String> = conn.query_row("SELECT program_json FROM custom_program_revisions WHERE program_id=?1 ORDER BY id DESC LIMIT 1", [id], |r| r.get(0)).optional()?;
    if latest.as_deref() != Some(json) {
        conn.execute("INSERT INTO custom_program_revisions(program_id, revision, program_json) VALUES (?1,NULL,?2)", params![id, json])?;
    }
    Ok(())
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

    fn init(mut conn: Connection) -> Result<Self> {
        // WAL is a no-op for in-memory DBs but harmless.
        let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        let mut version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(StorageError::UnsupportedVersion(version));
        }
        if version == 0 {
            let tx = conn.transaction()?;
            tx.execute_batch(SCHEMA_V1)?;
            tx.pragma_update(None, "user_version", 1)?;
            tx.commit()?;
            version = 1;
        }
        if version == 1 {
            let tx = conn.transaction()?;
            tx.execute_batch(MIGRATION_V2)?;
            tx.pragma_update(None, "user_version", 2)?;
            tx.commit()?;
            version = 2;
        }
        if version == 2 {
            let tx = conn.transaction()?;
            tx.execute_batch(
                "ALTER TABLE experiments ADD COLUMN sample_id TEXT;
                ALTER TABLE experiments ADD COLUMN project_name TEXT;
                ALTER TABLE experiments ADD COLUMN experiment_time TEXT;
                ALTER TABLE experiments ADD COLUMN program_snapshot_json TEXT;",
            )?;
            tx.pragma_update(None, "user_version", 3)?;
            tx.commit()?;
        }
        if version <= 3 {
            let tx = conn.transaction()?;
            tx.execute_batch(
                "CREATE TABLE custom_programs (id TEXT PRIMARY KEY, program_json TEXT NOT NULL);",
            )?;
            tx.pragma_update(None, "user_version", 4)?;
            tx.commit()?;
            version = 4;
        }
        if version <= 4 {
            let tx = conn.transaction()?;
            tx.execute_batch("ALTER TABLE experiment_data ADD COLUMN record_json TEXT;
              ALTER TABLE experiments ADD COLUMN runtime_metadata TEXT NOT NULL DEFAULT '{}';
              CREATE TABLE analysis_revisions (id INTEGER PRIMARY KEY AUTOINCREMENT, experiment_id TEXT NOT NULL REFERENCES experiments(experiment_id), result_json TEXT NOT NULL);")?;
            tx.pragma_update(None, "user_version", 5)?;
            tx.commit()?;
        }
        if version <= 5 {
            let tx = conn.transaction()?;
            tx.execute_batch("CREATE TABLE run_completions (token TEXT PRIMARY KEY, experiment_id TEXT NOT NULL REFERENCES experiments(experiment_id), completion_json TEXT NOT NULL);")?;
            tx.pragma_update(None, "user_version", 6)?;
            tx.commit()?;
            version = 6;
        }
        if version == 6 {
            let tx = conn.transaction()?;
            tx.execute_batch(
                "CREATE TABLE custom_program_revisions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                program_id TEXT NOT NULL, revision INTEGER CHECK(revision > 0),
                program_json TEXT NOT NULL, UNIQUE(program_id, revision));
                INSERT INTO custom_program_revisions(program_id, revision, program_json)
                SELECT id, NULL, program_json FROM custom_programs;",
            )?;
            tx.pragma_update(None, "user_version", 7)?;
            tx.commit()?;
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

    pub fn list_custom_programs(&self) -> Result<Vec<String>> {
        let mut statement = self
            .conn
            .prepare("SELECT program_json FROM custom_programs ORDER BY id")?;
        let rows = statement
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_custom_program(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT program_json FROM custom_programs WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Store the current revision and its immutable history in one transaction.
    /// Compare the original JSON too, so concurrent editors cannot overwrite changes.
    pub fn save_custom_program_revision(
        &mut self,
        program: &crate::program::Program,
        expected_json: Option<&str>,
    ) -> Result<()> {
        let revision = program
            .provenance
            .as_ref()
            .map(|p| p.revision)
            .and_then(|r| i64::try_from(r).ok())
            .filter(|r| *r > 0)
            .ok_or(StorageError::ProgramConflict)?;
        let json = serde_json::to_string(program)?;
        let tx = self.conn.transaction()?;
        let current: Option<String> = tx
            .query_row(
                "SELECT program_json FROM custom_programs WHERE id=?1",
                [&program.id],
                |r| r.get(0),
            )
            .optional()?;
        if current.as_deref() != expected_json {
            return Err(StorageError::ProgramConflict);
        }
        let previous_revision = match current.as_deref() {
            Some(raw) => {
                let previous: crate::program::Program = serde_json::from_str(raw)?;
                let previous_revision = previous
                    .provenance
                    .as_ref()
                    .map(|p| p.revision)
                    .unwrap_or(0);
                let previous_revision =
                    i64::try_from(previous_revision).map_err(|_| StorageError::ProgramConflict)?;
                if previous_revision == 0 {
                    retain_unversioned_program(&tx, &program.id, raw)?;
                } else {
                    let stored: Option<String> = tx.query_row("SELECT program_json FROM custom_program_revisions WHERE program_id=?1 AND revision=?2", params![program.id, previous_revision], |r| r.get(0)).optional()?;
                    if stored.as_deref() != Some(raw) {
                        return Err(StorageError::ProgramConflict);
                    }
                }
                previous_revision
            }
            None => {
                let exists: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM custom_program_revisions WHERE program_id=?1)",
                    [&program.id],
                    |r| r.get(0),
                )?;
                if exists {
                    return Err(StorageError::ProgramConflict);
                }
                0
            }
        };
        if previous_revision.checked_add(1) != Some(revision) {
            return Err(StorageError::ProgramConflict);
        }
        tx.execute("INSERT INTO custom_program_revisions(program_id, revision, program_json) VALUES (?1,?2,?3)", params![program.id, revision, json])?;
        tx.execute("INSERT INTO custom_programs(id, program_json) VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET program_json=excluded.program_json", params![program.id, json])?;
        tx.commit()?;
        Ok(())
    }

    pub fn custom_program_revisions(&self, id: &str) -> Result<Vec<String>> {
        let mut statement = self.conn.prepare("SELECT program_json FROM custom_program_revisions WHERE program_id=?1 ORDER BY id DESC")?;
        let rows = statement
            .query_map([id], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Compatibility for unversioned imports and the older IPC. It cannot rewrite
    /// a versioned program; every intervening legacy edit is preserved atomically.
    pub fn save_custom_program(&self, id: &str, json: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let versioned: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM custom_program_revisions WHERE program_id=?1 AND revision IS NOT NULL)", [id], |r| r.get(0))?;
        if versioned {
            return Err(StorageError::ProgramConflict);
        }
        let current: Option<String> = tx
            .query_row(
                "SELECT program_json FROM custom_programs WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(raw) = current {
            retain_unversioned_program(&tx, id, &raw)?;
        }
        retain_unversioned_program(&tx, id, json)?;
        tx.execute("INSERT INTO custom_programs(id, program_json) VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET program_json=excluded.program_json", params![id, json])?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_custom_program(&self, id: &str) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM custom_programs WHERE id=?1", [id])?
            > 0)
    }

    pub fn user_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?)
    }

    pub fn active_admin_count(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM users WHERE role = 'ADMIN' AND is_active = 1",
            [],
            |r| r.get(0),
        )?)
    }

    pub fn insert_user(&self, user: &UserRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO users (user_id, username, display_name, password_hash, role, is_active, \
             must_change_password, failed_login_count, locked_until, created_at, updated_at, \
             password_changed_at, last_login_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                user.user_id,
                user.username,
                user.display_name,
                user.password_hash,
                user.role.as_str(),
                user.is_active,
                user.must_change_password,
                user.failed_login_count,
                user.locked_until,
                user.created_at,
                user.updated_at,
                user.password_changed_at,
                user.last_login_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_user(&self, user_id: &str) -> Result<Option<UserRow>> {
        self.conn
            .query_row(
                &format!("{USER_SELECT} WHERE user_id = ?1"),
                params![user_id],
                user_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn get_user_by_username(&self, username: &str) -> Result<Option<UserRow>> {
        self.conn
            .query_row(
                &format!("{USER_SELECT} WHERE username = ?1 COLLATE NOCASE"),
                params![username],
                user_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_users(&self) -> Result<Vec<UserRow>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{USER_SELECT} ORDER BY username COLLATE NOCASE"))?;
        let rows = stmt
            .query_map([], user_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn update_user(
        &self,
        user_id: &str,
        display_name: &str,
        role: UserRole,
        is_active: bool,
        updated_at: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE users SET display_name = ?2, role = ?3, is_active = ?4, updated_at = ?5 WHERE user_id = ?1",
            params![user_id, display_name, role.as_str(), is_active, updated_at],
        )?;
        Ok(())
    }

    pub fn record_login_failure(
        &self,
        user_id: &str,
        failed_count: i64,
        locked_until: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE users SET failed_login_count = ?2, locked_until = ?3 WHERE user_id = ?1",
            params![user_id, failed_count, locked_until],
        )?;
        Ok(())
    }

    pub fn record_login_success(&self, user_id: &str, at: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE users SET failed_login_count = 0, locked_until = NULL, last_login_at = ?2 WHERE user_id = ?1",
            params![user_id, at],
        )?;
        Ok(())
    }

    pub fn set_user_password(
        &self,
        user_id: &str,
        password_hash: &str,
        must_change_password: bool,
        at: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE users SET password_hash = ?2, must_change_password = ?3, \
             failed_login_count = 0, locked_until = NULL, password_changed_at = ?4, updated_at = ?4 \
             WHERE user_id = ?1",
            params![user_id, password_hash, must_change_password, at],
        )?;
        Ok(())
    }

    /// Insert an experiment metadata row.
    pub fn insert_experiment(&self, e: &ExperimentRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO experiments (experiment_id, experiment_name, sample_name, sample_weight, \
             start_time, end_time, description, operator, experiment_type, analysis_results_json, \
             created_at, owner_user_id, sample_id, project_name, experiment_time, program_snapshot_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
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
                e.owner_user_id,
                e.sample_id,
                e.project_name,
                e.experiment_time,
                e.program_snapshot_json,
            ],
        )?;
        Ok(())
    }

    /// Insert a batch of sample rows inside one transaction (the storage writer's
    /// unit of work).
    pub fn insert_samples(&mut self, rows: &[SampleRow]) -> Result<()> {
        let tx = self.conn.transaction()?;
        insert_sample_rows(&tx, rows)?;
        tx.commit()?;
        Ok(())
    }

    /// Commit rows, metadata, terminal time and replay receipt together. An identical
    /// retry is a no-op; a reused token with changed data or a second completion fails.
    pub fn commit_completion(&mut self, completion: &RunCompletion) -> Result<()> {
        if completion.token.is_empty()
            || completion.end_time.is_empty()
            || !completion.metadata.is_object()
            || completion
                .rows
                .iter()
                .any(|r| r.experiment_id != completion.experiment_id)
        {
            return Err(StorageError::InvalidCompletion);
        }
        let json = serde_json::to_string(completion)?;
        let tx = self.conn.transaction()?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT completion_json FROM run_completions WHERE token=?1",
                [&completion.token],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            return if existing == json {
                Ok(())
            } else {
                Err(StorageError::InvalidCompletion)
            };
        }
        let end: Option<String> = tx.query_row(
            "SELECT end_time FROM experiments WHERE experiment_id=?1",
            [&completion.experiment_id],
            |r| r.get(0),
        )?;
        if end.is_some() {
            return Err(StorageError::InvalidCompletion);
        }
        insert_sample_rows(&tx, &completion.rows)?;
        tx.execute("UPDATE experiments SET runtime_metadata=json_patch(runtime_metadata,?2),end_time=?3 WHERE experiment_id=?1",params![completion.experiment_id,completion.metadata.to_string(),completion.end_time])?;
        tx.execute(
            "INSERT INTO run_completions(token,experiment_id,completion_json) VALUES (?1,?2,?3)",
            params![completion.token, completion.experiment_id, json],
        )?;
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

    /// Stamp an experiment's `end_time` (idempotent — a no-op if the id is unknown).
    pub fn finish_experiment(&self, experiment_id: &str, end_time: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE experiments SET end_time = ?2 WHERE experiment_id = ?1",
            params![experiment_id, end_time],
        )?;
        Ok(())
    }

    pub fn runtime_metadata(&self, id: &str) -> Result<String> {
        Ok(self.conn.query_row(
            "SELECT runtime_metadata FROM experiments WHERE experiment_id=?1",
            [id],
            |r| r.get(0),
        )?)
    }

    pub fn patch_runtime_metadata(&self, id: &str, patch: &str) -> Result<()> {
        self.conn.execute("UPDATE experiments SET runtime_metadata=json_patch(runtime_metadata, ?2) WHERE experiment_id=?1", params![id,patch])?;
        Ok(())
    }

    pub fn save_report_revision(&mut self, id: &str, patch: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO analysis_revisions(experiment_id,result_json) VALUES (?1,?2)",
            params![id, patch],
        )?;
        tx.execute("UPDATE experiments SET runtime_metadata=json_patch(runtime_metadata, ?2) WHERE experiment_id=?1", params![id,patch])?;
        tx.commit()?;
        Ok(())
    }

    pub fn analysis_revisions(&self, id: &str) -> Result<Vec<serde_json::Value>> {
        let mut stmt = self.conn.prepare(
            "SELECT result_json FROM analysis_revisions WHERE experiment_id=?1 ORDER BY id DESC",
        )?;
        let values = stmt
            .query_map([id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        values
            .into_iter()
            .map(|v| serde_json::from_str(&v).map_err(Into::into))
            .collect()
    }

    /// Store the analysis-results JSON blob for an experiment.
    pub fn set_analysis_results(&mut self, experiment_id: &str, json: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO analysis_revisions(experiment_id,result_json) VALUES (?1,?2)",
            params![experiment_id, json],
        )?;
        tx.execute(
            "UPDATE experiments SET analysis_results_json=?2 WHERE experiment_id=?1",
            params![experiment_id, json],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// All experiments, newest first (history list).
    pub fn list_experiments(&self) -> Result<Vec<ExperimentRow>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{EXPERIMENT_SELECT} ORDER BY start_time DESC"))?;
        let rows = stmt
            .query_map([], experiment_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_experiments_for_owner(&self, owner_user_id: &str) -> Result<Vec<ExperimentRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "{EXPERIMENT_SELECT} WHERE owner_user_id = ?1 ORDER BY start_time DESC"
        ))?;
        let rows = stmt
            .query_map(params![owner_user_id], experiment_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_experiment_owner(
        &self,
        experiment_id: &str,
        owner_user_id: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE experiments SET owner_user_id = ?2 WHERE experiment_id = ?1",
            params![experiment_id, owner_user_id],
        )?;
        Ok(())
    }

    /// Experiments still open (no `end_time`) — used for crash recovery on startup.
    pub fn unfinished_experiments(&self) -> Result<Vec<ExperimentRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "{EXPERIMENT_SELECT} WHERE end_time IS NULL ORDER BY start_time"
        ))?;
        let rows = stmt
            .query_map([], experiment_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// One experiment by id.
    pub fn get_experiment(&self, experiment_id: &str) -> Result<Option<ExperimentRow>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{EXPERIMENT_SELECT} WHERE experiment_id = ?1"))?;
        let mut rows = stmt.query_map(params![experiment_id], experiment_from_row)?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    /// Sample rows for an experiment, decimated to about `max_points` (evenly spaced,
    /// oldest→newest, with the first and last sample always included). `max_points == 0`
    /// returns every row. A huge run must not flood the chart, so the read thins
    /// server-side via a window-function stride.
    pub fn get_samples(&self, experiment_id: &str, max_points: usize) -> Result<Vec<SampleRow>> {
        let total = self.count_samples(experiment_id)? as usize;
        let stride = if max_points == 0 || total <= max_points {
            1
        } else {
            total.div_ceil(max_points)
        };
        // Always include the final sample (rn = total-1): it is the endpoint the chart and
        // GB analysis care about most, and plain `rn % stride == 0` usually drops it (#14).
        let last_rn = total.saturating_sub(1) as i64;
        let mut stmt = self.conn.prepare(
            "SELECT experiment_id, timestamp, experiment_duration, temperature, weight, \
             weight_loss, co_flow, co2_flow, n2_flow, h2_flow, experiment_status, system_message, record_json \
             FROM (SELECT *, ROW_NUMBER() OVER (ORDER BY id) - 1 AS rn FROM experiment_data \
             WHERE experiment_id = ?1) WHERE rn % ?2 = 0 OR rn = ?3 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(
                params![experiment_id, stride as i64, last_rn],
                sample_from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Delete an experiment and all its samples (FK order: samples first).
    pub fn delete_experiment(&mut self, experiment_id: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM run_completions WHERE experiment_id=?1",
            [experiment_id],
        )?;
        tx.execute(
            "DELETE FROM analysis_revisions WHERE experiment_id=?1",
            [experiment_id],
        )?;
        tx.execute(
            "DELETE FROM experiment_data WHERE experiment_id = ?1",
            params![experiment_id],
        )?;
        tx.execute(
            "DELETE FROM experiments WHERE experiment_id = ?1",
            params![experiment_id],
        )?;
        tx.commit()?;
        Ok(())
    }
}

/// Column list shared by every `experiments` SELECT (keeps `experiment_from_row` in sync).
const EXPERIMENT_SELECT: &str = "SELECT experiment_id, experiment_name, sample_name, \
     sample_weight, start_time, end_time, description, operator, experiment_type, \
     analysis_results_json, created_at, owner_user_id, sample_id, project_name, experiment_time, program_snapshot_json FROM experiments";

const USER_SELECT: &str =
    "SELECT user_id, username, display_name, password_hash, role, is_active, \
     must_change_password, failed_login_count, locked_until, created_at, updated_at, \
     password_changed_at, last_login_at FROM users";

fn experiment_from_row(r: &rusqlite::Row) -> rusqlite::Result<ExperimentRow> {
    Ok(ExperimentRow {
        experiment_id: r.get(0)?,
        experiment_name: r.get(1)?,
        sample_name: r.get(2)?,
        sample_weight: r.get(3)?,
        start_time: r.get(4)?,
        end_time: r.get(5)?,
        description: r.get(6)?,
        operator: r.get(7)?,
        experiment_type: r.get(8)?,
        analysis_results_json: r.get(9)?,
        created_at: r.get(10)?,
        owner_user_id: r.get(11)?,
        sample_id: r.get(12)?,
        project_name: r.get(13)?,
        experiment_time: r.get(14)?,
        program_snapshot_json: r.get(15)?,
    })
}

fn user_from_row(r: &rusqlite::Row) -> rusqlite::Result<UserRow> {
    let role_value: String = r.get(4)?;
    let role = role_value.parse::<UserRole>().map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(UserRow {
        user_id: r.get(0)?,
        username: r.get(1)?,
        display_name: r.get(2)?,
        password_hash: r.get(3)?,
        role,
        is_active: r.get(5)?,
        must_change_password: r.get(6)?,
        failed_login_count: r.get(7)?,
        locked_until: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
        password_changed_at: r.get(11)?,
        last_login_at: r.get(12)?,
    })
}

fn sample_from_row(r: &rusqlite::Row) -> rusqlite::Result<SampleRow> {
    Ok(SampleRow {
        record_json: r.get(12)?,
        experiment_id: r.get(0)?,
        timestamp: r.get(1)?,
        experiment_duration: r.get(2)?,
        temperature: r.get(3)?,
        weight: r.get(4)?,
        weight_loss: r.get(5)?,
        co_flow: r.get(6)?,
        co2_flow: r.get(7)?,
        n2_flow: r.get(8)?,
        h2_flow: r.get(9)?,
        experiment_status: r.get(10)?,
        system_message: r.get(11)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::UserRole;

    fn user(id: &str, username: &str, role: UserRole) -> UserRow {
        UserRow {
            user_id: id.into(),
            username: username.into(),
            display_name: username.into(),
            password_hash: "$argon2id$test".into(),
            role,
            is_active: true,
            must_change_password: false,
            failed_login_count: 0,
            locked_until: None,
            created_at: "2026-09-01T08:00:00".into(),
            updated_at: "2026-09-01T08:00:00".into(),
            password_changed_at: "2026-09-01T08:00:00".into(),
            last_login_at: None,
        }
    }

    fn experiment(id: &str) -> ExperimentRow {
        ExperimentRow {
            sample_id: Some(format!("{id}-001")),
            project_name: Some("project".into()),
            experiment_time: Some("2026-07-05T10:00:00".into()),
            program_snapshot_json: Some(r#"{"id":"REDUCIBILITY"}"#.into()),
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
            owner_user_id: None,
        }
    }

    fn sample(id: &str, t: f64) -> SampleRow {
        SampleRow {
            record_json: None,
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
    fn schema_inits_at_version_3() {
        let s = Storage::open_in_memory().unwrap();
        assert_eq!(s.user_version().unwrap(), 7);
        assert_eq!(s.user_count().unwrap(), 0);
    }

    #[test]
    fn users_roundtrip_and_usernames_are_case_insensitively_unique() {
        let s = Storage::open_in_memory().unwrap();
        s.insert_user(&user("u1", "admin", UserRole::Admin))
            .unwrap();
        let found = s.get_user_by_username("ADMIN").unwrap().unwrap();
        assert_eq!(found.user_id, "u1");
        assert_eq!(found.role, UserRole::Admin);
        assert!(s.insert_user(&user("u2", "Admin", UserRole::User)).is_err());
        assert_eq!(s.active_admin_count().unwrap(), 1);
    }

    #[test]
    fn login_counters_and_password_reset_are_persisted() {
        let s = Storage::open_in_memory().unwrap();
        s.insert_user(&user("u1", "operator", UserRole::User))
            .unwrap();
        s.record_login_failure("u1", 5, Some("2026-09-01T08:15:00"))
            .unwrap();
        let failed = s.get_user("u1").unwrap().unwrap();
        assert_eq!(failed.failed_login_count, 5);
        assert_eq!(failed.locked_until.as_deref(), Some("2026-09-01T08:15:00"));

        s.set_user_password("u1", "$argon2id$new", true, "2026-09-01T09:00:00")
            .unwrap();
        let reset = s.get_user("u1").unwrap().unwrap();
        assert!(reset.must_change_password);
        assert_eq!(reset.failed_login_count, 0);
        assert!(reset.locked_until.is_none());
    }

    #[test]
    fn v1_database_migrates_without_assigning_legacy_experiments() {
        let dir = std::env::temp_dir().join(format!("tmh_migrate_{}", crate::auth::new_user_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("experiments.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(SCHEMA_V1).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO experiments (experiment_id, experiment_name, sample_name, sample_weight, start_time, created_at) VALUES ('legacy', 'legacy', 'sample', 1.0, '2026-01-01', '2026-01-01')",
                [],
            )
            .unwrap();
        }
        let s = Storage::open(&path).unwrap();
        assert_eq!(s.user_version().unwrap(), 7);
        assert!(s
            .get_experiment("legacy")
            .unwrap()
            .unwrap()
            .owner_user_id
            .is_none());
        std::fs::remove_dir_all(&dir).ok();
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
    fn finish_and_list_and_get() {
        let s = Storage::open_in_memory().unwrap();
        s.insert_experiment(&experiment("e1")).unwrap();
        s.insert_experiment(&experiment("e2")).unwrap();
        // Both unfinished at first.
        assert_eq!(s.unfinished_experiments().unwrap().len(), 2);

        s.finish_experiment("e1", "2026-07-05T11:00:00").unwrap();
        let open = s.unfinished_experiments().unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].experiment_id, "e2");

        let all = s.list_experiments().unwrap();
        assert_eq!(all.len(), 2);
        let e1 = s.get_experiment("e1").unwrap().unwrap();
        assert_eq!(e1.end_time.as_deref(), Some("2026-07-05T11:00:00"));
        assert!(s.get_experiment("nope").unwrap().is_none());
    }

    #[test]
    fn set_analysis_results_roundtrips() {
        let mut s = Storage::open_in_memory().unwrap();
        s.insert_experiment(&experiment("e1")).unwrap();
        s.set_analysis_results("e1", r#"{"rt":50.0}"#).unwrap();
        s.save_report_revision("e1", r#"{"report_details":{"lab_name":"Lab A"}}"#)
            .unwrap();
        s.save_report_revision("e1", r#"{"report_details":{"lab_name":"Lab B"}}"#)
            .unwrap();
        let revisions = s.analysis_revisions("e1").unwrap();
        assert_eq!(revisions.len(), 3);
        assert_eq!(revisions[1]["report_details"]["lab_name"], "Lab A");
        let e = s.get_experiment("e1").unwrap().unwrap();
        assert_eq!(e.analysis_results_json.as_deref(), Some(r#"{"rt":50.0}"#));
    }

    #[test]
    fn get_samples_decimates_to_max_points() {
        let mut s = Storage::open_in_memory().unwrap();
        s.insert_experiment(&experiment("e1")).unwrap();
        let rows: Vec<SampleRow> = (0..1000).map(|i| sample("e1", i as f64)).collect();
        s.insert_samples(&rows).unwrap();

        let all = s.get_samples("e1", 0).unwrap();
        assert_eq!(all.len(), 1000);
        assert_eq!(all[0].temperature, 0.0);
        assert_eq!(all[999].temperature, 999.0);

        let thin = s.get_samples("e1", 100).unwrap();
        // ~max_points, plus the always-included final sample (#14).
        assert!(
            thin.len() <= 101,
            "decimated to about 100, got {}",
            thin.len()
        );
        assert!(thin.len() >= 90, "still representative, got {}", thin.len());
        // First AND last point preserved; ordered oldest→newest.
        assert_eq!(thin[0].temperature, 0.0);
        assert_eq!(thin.last().unwrap().temperature, 999.0);
        assert!(thin.windows(2).all(|w| w[0].temperature < w[1].temperature));
    }

    #[test]
    fn delete_experiment_removes_samples() {
        let mut s = Storage::open_in_memory().unwrap();
        s.insert_experiment(&experiment("e1")).unwrap();
        s.insert_samples(&[sample("e1", 1.0), sample("e1", 2.0)])
            .unwrap();
        s.delete_experiment("e1").unwrap();
        assert_eq!(s.count_samples("e1").unwrap(), 0);
        assert!(s.get_experiment("e1").unwrap().is_none());
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
        assert_eq!(s.user_version().unwrap(), 7);
        assert_eq!(s.count_samples("e2").unwrap(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v2_migration_preserves_owners_and_roundtrips_traceability() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute_batch(MIGRATION_V2).unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        let s = Storage { conn };
        s.insert_user(&user("owner", "owner", UserRole::User))
            .unwrap();
        s.conn.execute("INSERT INTO experiments (experiment_id, experiment_name, sample_name, sample_weight, start_time, created_at, owner_user_id) VALUES ('old', 'old', 'sample', 100, '2026-01-01', '2026-01-01', 'owner')", []).unwrap();
        let s = Storage::init(s.conn).unwrap();
        let old = s.get_experiment("old").unwrap().unwrap();
        assert_eq!(old.owner_user_id.as_deref(), Some("owner"));
        assert!(old.sample_id.is_none());
        let mut new = experiment("new");
        new.owner_user_id = Some("owner".into());
        s.insert_experiment(&new).unwrap();
        let s = Storage::init(s.conn).unwrap();
        let saved = s.get_experiment("new").unwrap().unwrap();
        assert_eq!(saved.sample_id, new.sample_id);
        assert_eq!(saved.project_name, new.project_name);
        assert_eq!(saved.experiment_time, new.experiment_time);
        assert_eq!(saved.program_snapshot_json, new.program_snapshot_json);
        assert_eq!(saved.owner_user_id, new.owner_user_id);
        assert_eq!(s.list_experiments_for_owner("owner").unwrap().len(), 2);
    }
}
