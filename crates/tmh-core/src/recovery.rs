//! Pending final commits survive process restart. A file is removed only after
//! SQLite confirms the complete transaction or its identical replay receipt.
use crate::storage::RunCompletion;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Keep an exclusive lock on a separate SQLite file for this data directory.
/// A second process must not recover the first process's active run even if an
/// OS single-instance notification fails. SQLite releases the lock after a crash.
pub struct DataDirectoryLock {
    _connection: std::sync::Mutex<rusqlite::Connection>,
}
impl DataDirectoryLock {
    pub fn acquire(dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let connection = rusqlite::Connection::open(dir.join("instance-lock.sqlite"))
            .map_err(|e| e.to_string())?;
        connection
            .busy_timeout(std::time::Duration::ZERO)
            .map_err(|e| e.to_string())?;
        connection
            .execute_batch("BEGIN EXCLUSIVE;")
            .map_err(|e| format!("数据目录已被使用或无法锁定：{e}"))?;
        Ok(Self {
            _connection: std::sync::Mutex::new(connection),
        })
    }
}

#[cfg(test)]
mod ownership_tests {
    use super::*;
    #[test]
    fn only_one_process_connection_can_own_recovery_at_a_time() {
        let dir = std::env::temp_dir().join(format!("tmh_lock_{}", uuid::Uuid::new_v4()));
        let first = DataDirectoryLock::acquire(&dir).unwrap();
        assert!(DataDirectoryLock::acquire(&dir).is_err());
        drop(first);
        drop(DataDirectoryLock::acquire(&dir).unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[derive(Default)]
pub struct RecoveryScan {
    pub pending: Vec<(PathBuf, RunCompletion)>,
    pub errors: Vec<String>,
}

pub fn persist(dir: &Path, completion: &RunCompletion) -> Result<PathBuf, String> {
    // Tokens form filenames; never accept a path supplied through stored JSON.
    if completion.token.is_empty()
        || !completion
            .token
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        return Err("invalid recovery token".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.json", completion.token));
    let bytes = serde_json::to_vec(completion).map_err(|e| e.to_string())?;
    // A recovery payload must be readable before it can be advertised as durable.
    serde_json::from_slice::<RunCompletion>(&bytes).map_err(|e| e.to_string())?;
    if path.exists() {
        return if std::fs::read(&path).map_err(|e| e.to_string())? == bytes {
            sync_directory(dir)?;
            Ok(path)
        } else {
            Err("conflicting pending recovery file".into())
        };
    }
    let temporary = dir.join(format!("{}.tmp", completion.token));
    if temporary.exists() {
        let previous = std::fs::read(&temporary).map_err(|e| e.to_string())?;
        match serde_json::from_slice::<RunCompletion>(&previous) {
            Ok(existing) => {
                if serde_json::to_vec(&existing).map_err(|e| e.to_string())? != bytes {
                    return Err("conflicting temporary recovery file".into());
                }
                std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&temporary)
                    .and_then(|f| f.sync_all())
                    .map_err(|e| e.to_string())?;
                std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
                sync_directory(dir)?;
                return Ok(path);
            }
            Err(_) => {
                // A disk-full write can leave our own incomplete temp file.
                // Preserve it for inspection without blocking a new complete copy.
                let partial = dir.join(format!(
                    "{}-{}.partial",
                    completion.token,
                    uuid::Uuid::new_v4()
                ));
                std::fs::rename(&temporary, partial).map_err(|e| e.to_string())?;
            }
        }
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
    sync_directory(dir)?;
    Ok(path)
}

fn sync_directory(_dir: &Path) -> Result<(), String> {
    #[cfg(unix)]
    std::fs::File::open(_dir)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn scan(dir: &Path) -> RecoveryScan {
    let mut result = RecoveryScan::default();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return result,
        Err(e) => {
            result.errors.push(e.to_string());
            return result;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                result.errors.push(e.to_string());
                continue;
            }
        };
        let path = entry.path();
        if !matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("json" | "tmp")
        ) {
            continue;
        }
        let read = || -> Result<RunCompletion, String> {
            let mut file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
            if file.metadata().map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
                return Err("recovery file exceeds 16 MiB".into());
            }
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())
        };
        match read() {
            Ok(completion) => result.pending.push((path, completion)),
            Err(e) => {
                // An incomplete temporary file was never committed. Keep it for
                // inspection; interrupted experiments remain explicitly incomplete.
                if path.extension().and_then(|v| v.to_str()) == Some("json") {
                    result.errors.push(format!("{}: {e}", path.display()));
                }
            }
        }
    }
    result
}

pub fn remove(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn clear(dir: &Path, token: &str) -> Result<(), String> {
    if token.is_empty()
        || !token
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        return Err("invalid recovery token".into());
    }
    remove(&dir.join(format!("{token}.json")))?;
    remove(&dir.join(format!("{token}.tmp")))
}
