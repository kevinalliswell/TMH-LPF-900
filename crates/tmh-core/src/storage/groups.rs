//! Immutable test-group snapshots. No experiment foreign key: retained group
//! evidence must survive a later, separately authorized deletion of a single run.
use super::{Result, Storage, StorageError};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestGroupRevision {
    pub group_id: String,
    pub revision: u64,
    pub owner_user_id: String,
    pub recorded_by_user_id: String,
    pub created_at: String,
    #[cfg_attr(feature = "typescript-export", ts(type = "GroupSnapshot"))]
    pub snapshot: serde_json::Value,
}

const COLUMNS: &str =
    "group_id,revision,owner_user_id,recorded_by_user_id,created_at,snapshot_json";
fn from_row(row: &rusqlite::Row) -> rusqlite::Result<TestGroupRevision> {
    let raw: String = row.get(5)?;
    let snapshot = serde_json::from_str(&raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(TestGroupRevision {
        group_id: row.get(0)?,
        revision: row.get::<_, i64>(1)? as u64,
        owner_user_id: row.get(2)?,
        recorded_by_user_id: row.get(3)?,
        created_at: row.get(4)?,
        snapshot,
    })
}
fn revision_i64(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| StorageError::GroupConflict)
}
impl Storage {
    /// Compare the current revision and append under one writer transaction. The
    /// application authorizes every referenced run and generates actor fields.
    pub fn save_test_group_revision(
        &mut self,
        record: &TestGroupRevision,
        expected_revision: Option<u64>,
    ) -> Result<()> {
        let json = serde_json::to_string(&record.snapshot)?;
        if !record.group_id.starts_with("group-")
            || record.group_id.len() > 128
            || !record
                .group_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
            || record.created_at.is_empty()
            || record.created_at.len() > 64
            || !record.snapshot.is_object()
            || json.len() > 1_048_576
        {
            return Err(StorageError::GroupConflict);
        }
        // Reject snapshots that the normal reader could not deserialize (including
        // its nesting-depth bound), before committing a version to history.
        let _: serde_json::Value = serde_json::from_str(&json)?;
        let revision = revision_i64(record.revision)?;
        let expected = expected_revision.map(revision_i64).transpose()?;
        if revision <= 0 || expected.unwrap_or(0).checked_add(1) != Some(revision) {
            return Err(StorageError::GroupConflict);
        }
        let tx = self.conn.transaction()?;
        let previous:Option<(i64,String)>=tx.query_row(
            "SELECT revision,owner_user_id FROM test_group_revisions WHERE group_id=?1 ORDER BY revision DESC LIMIT 1",
            [&record.group_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if previous.as_ref().map(|r| r.0) != expected
            || previous
                .as_ref()
                .is_some_and(|r| r.1 != record.owner_user_id)
        {
            return Err(StorageError::GroupConflict);
        }
        tx.execute("INSERT INTO test_group_revisions(group_id,revision,owner_user_id,recorded_by_user_id,created_at,snapshot_json) VALUES (?1,?2,?3,?4,?5,?6)",
            params![record.group_id,revision,record.owner_user_id,record.recorded_by_user_id,record.created_at,json])?;
        tx.commit()?;
        Ok(())
    }

    pub fn test_group_revision(
        &self,
        id: &str,
        revision: Option<u64>,
    ) -> Result<Option<TestGroupRevision>> {
        let revision = revision.map(revision_i64).transpose()?;
        Ok(self.conn.query_row(&format!("SELECT {COLUMNS} FROM test_group_revisions WHERE group_id=?1 AND (?2 IS NULL OR revision=?2) ORDER BY revision DESC LIMIT 1"),params![id,revision],from_row).optional()?)
    }

    /// Descending version cursor; at most 25 snapshots per request.
    pub fn test_group_history(
        &self,
        id: &str,
        before_revision: Option<u64>,
    ) -> Result<Vec<TestGroupRevision>> {
        let before = before_revision.map(revision_i64).transpose()?;
        let mut query=self.conn.prepare(&format!("SELECT {COLUMNS} FROM test_group_revisions WHERE group_id=?1 AND (?2 IS NULL OR revision<?2) ORDER BY revision DESC LIMIT 25"))?;
        let rows = query
            .query_map(params![id, before], from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Latest version of each visible group. None is the administrator's view;
    /// callers must authorize that choice. Offset pages contain at most 25 groups.
    pub fn list_test_groups(
        &self,
        owner_user_id: Option<&str>,
        offset: u32,
    ) -> Result<Vec<TestGroupRevision>> {
        let mut query=self.conn.prepare(&format!("SELECT {COLUMNS} FROM test_group_revisions r WHERE (?1 IS NULL OR owner_user_id=?1) AND NOT EXISTS (SELECT 1 FROM test_group_revisions newer WHERE newer.group_id=r.group_id AND newer.revision>r.revision) ORDER BY created_at DESC,group_id DESC LIMIT 25 OFFSET ?2"))?;
        let rows = query
            .query_map(params![owner_user_id, offset], from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}
