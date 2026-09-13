//! Exclusive, conflict-checked communication configuration persistence.
//! Saving changes `saved`; only a new `open` changes the process's `applied` value.
use crate::config::{CommConfig, ConfigError};
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

const MAX_CONFIG_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConfigSnapshot {
    pub path: PathBuf,
    pub revision: String,
    pub applied: CommConfig,
    pub saved: CommConfig,
    pub restart_required: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigStoreError {
    #[error("configuration file does not exist: {}", .0.display())]
    MissingFile(PathBuf),
    #[error("configuration path must identify a regular file")]
    InvalidPath,
    #[error("configuration exceeds 1 MiB")]
    TooLarge,
    #[error("configuration is already in use or cannot be locked: {0}")]
    Locked(String),
    #[error("configuration version is stale; reload before saving")]
    StaleRevision,
    #[error("configuration file changed outside this application; reopen and review it")]
    ExternalChange,
    #[error(
        "configuration file was replaced but directory durability could not be confirmed: {0}"
    )]
    DurabilityUnconfirmed(String),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("configuration I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

pub struct ConfigStore {
    path: PathBuf,
    revision: String,
    applied: CommConfig,
    saved: CommConfig,
    bytes: Vec<u8>,
    // A distinct SQLite connection holds an OS-backed lifetime lock, including
    // against another connection in this process. Do not remove the lock file.
    _lock: rusqlite::Connection,
}

impl ConfigStore {
    /// `None` requires an existing explicit file. `Some` initializes only a missing
    /// file; it never repairs or overwrites an existing invalid configuration.
    pub fn open(path: PathBuf, default: Option<&CommConfig>) -> Result<Self, ConfigStoreError> {
        let exists = match fs::symlink_metadata(&path) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        if !exists {
            let Some(default) = default else {
                return Err(ConfigStoreError::MissingFile(path));
            };
            default.validate()?;
        }
        let path = resolve_path(&path, !exists)?;
        let mut lock_name = path
            .file_name()
            .ok_or(ConfigStoreError::InvalidPath)?
            .to_os_string();
        lock_name.push(".lock.sqlite");
        let lock = rusqlite::Connection::open(path.with_file_name(lock_name))
            .map_err(|error| ConfigStoreError::Locked(error.to_string()))?;
        lock.busy_timeout(Duration::ZERO)
            .and_then(|_| lock.execute_batch("PRAGMA journal_mode=DELETE; BEGIN EXCLUSIVE;"))
            .map_err(|error| ConfigStoreError::Locked(error.to_string()))?;
        let bytes = if exists {
            read_config(&path)?
        } else {
            let bytes = serialize(default.expect("missing file requires defaults"))?;
            let temporary = TemporaryConfig::write(&path, &bytes)?;
            // No-clobber publication: an external creator racing our initialization
            // must win, instead of being overwritten by default configuration.
            fs::hard_link(&temporary.0, &path).map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    ConfigStoreError::ExternalChange
                } else {
                    error.into()
                }
            })?;
            sync_directory(parent(&path)?)
                .map_err(|error| ConfigStoreError::DurabilityUnconfirmed(error.to_string()))?;
            bytes
        };
        let saved: CommConfig = serde_json::from_slice(&bytes)
            .map_err(|error| ConfigError::Parse(error.to_string()))?;
        saved.validate()?;
        Ok(Self {
            path,
            revision: Uuid::new_v4().to_string(),
            applied: saved.clone(),
            saved,
            bytes,
            _lock: lock,
        })
    }

    pub fn snapshot(&self) -> ConfigSnapshot {
        ConfigSnapshot {
            path: self.path.clone(),
            revision: self.revision.clone(),
            applied: self.applied.clone(),
            saved: self.saved.clone(),
            restart_required: self.applied != self.saved,
        }
    }

    /// A failed durability confirmation can follow successful replacement. In that
    /// case `snapshot` already reflects the new saved file; it never claims rollback.
    pub fn save(
        &mut self,
        expected_revision: &str,
        config: CommConfig,
    ) -> Result<ConfigSnapshot, ConfigStoreError> {
        self.save_with_io(
            expected_revision,
            config,
            |from, to| fs::rename(from, to),
            sync_directory,
        )
    }

    fn save_with_io(
        &mut self,
        expected_revision: &str,
        config: CommConfig,
        replace: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
        sync: impl FnOnce(&Path) -> std::io::Result<()>,
    ) -> Result<ConfigSnapshot, ConfigStoreError> {
        if expected_revision != self.revision {
            return Err(ConfigStoreError::StaleRevision);
        }
        config.validate()?;
        self.verify_saved_file()?;
        let bytes = serialize(&config)?;
        let temporary = TemporaryConfig::write(&self.path, &bytes)?;
        // File flushing can take time; check external bytes again just before
        // publication. The SQLite lock serializes cooperating application writers.
        self.verify_saved_file()?;
        replace(&temporary.0, &self.path)?;
        self.bytes = bytes;
        self.saved = config;
        self.revision = Uuid::new_v4().to_string();
        sync(parent(&self.path)?)
            .map_err(|error| ConfigStoreError::DurabilityUnconfirmed(error.to_string()))?;
        Ok(self.snapshot())
    }

    /// Verify the saved file before a controlled restart; external changes must
    /// not silently replace the configuration reviewed by the operator.
    pub fn verify_saved_file(&self) -> Result<(), ConfigStoreError> {
        if read_config(&self.path)? != self.bytes {
            return Err(ConfigStoreError::ExternalChange);
        }
        Ok(())
    }
}

fn parent(path: &Path) -> Result<&Path, ConfigStoreError> {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or(ConfigStoreError::InvalidPath)
}

fn resolve_path(path: &Path, create_parent: bool) -> Result<PathBuf, ConfigStoreError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    if !create_parent {
        // Resolves symlinks, including rejecting dangling ones. The target must
        // already be a regular file; a FIFO must never block a configuration read.
        let resolved = fs::canonicalize(absolute)?;
        if !fs::metadata(&resolved)?.is_file() {
            return Err(ConfigStoreError::InvalidPath);
        }
        return Ok(resolved);
    }
    let name = absolute.file_name().ok_or(ConfigStoreError::InvalidPath)?;
    let directory = parent(&absolute)?;
    fs::create_dir_all(directory)?;
    Ok(fs::canonicalize(directory)?.join(name))
}

fn read_config(path: &Path) -> Result<Vec<u8>, ConfigStoreError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(ConfigStoreError::InvalidPath);
    }
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_CONFIG_BYTES as u64 {
        return Err(ConfigStoreError::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take((MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(ConfigStoreError::TooLarge);
    }
    Ok(bytes)
}

fn serialize(config: &CommConfig) -> Result<Vec<u8>, ConfigStoreError> {
    let bytes =
        serde_json::to_vec_pretty(config).map_err(|error| ConfigError::Parse(error.to_string()))?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(ConfigStoreError::TooLarge);
    }
    Ok(bytes)
}

struct TemporaryConfig(PathBuf);
impl TemporaryConfig {
    fn write(target: &Path, bytes: &[u8]) -> Result<Self, ConfigStoreError> {
        let path = parent(target)?.join(format!(".tmh-config-{}.tmp", Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        let temporary = Self(path);
        let result = file.write_all(bytes).and_then(|_| file.sync_all());
        drop(file);
        result?;
        Ok(temporary)
    }
}
impl Drop for TemporaryConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn sync_directory(_path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    File::open(_path)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
#[path = "config_store_tests.rs"]
mod tests;
