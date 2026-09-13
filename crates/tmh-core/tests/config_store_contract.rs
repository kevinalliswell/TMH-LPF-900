//! Public configuration-store contracts: persistence, revision checks and ownership.
//! Each test owns a real temporary directory; no bus or application runtime is used.

use std::fs;
use std::path::PathBuf;
use tmh_core::config::CommConfig;
use tmh_core::config_store::{ConfigSnapshot, ConfigStore};

const FIXTURE: &str = include_str!("../../../config/comm_config.json");

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("tmh_config_store_{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn config_path(&self) -> PathBuf {
        self.0.join("comm_config.json")
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn config() -> CommConfig {
    let config = CommConfig::from_json(FIXTURE).unwrap();
    config.validate().unwrap();
    config
}

#[test]
fn explicit_missing_file_without_defaults_is_rejected() {
    let directory = TestDirectory::new();
    let path = directory.config_path();

    assert!(ConfigStore::open(path.clone(), None).is_err());

    assert!(
        !path.exists(),
        "an explicit missing file must not be initialized"
    );
}

#[test]
fn defaults_initialize_only_missing_files_and_never_replace_existing_contents() {
    let directory = TestDirectory::new();
    let path = directory.config_path();
    let defaults = config();
    let store = ConfigStore::open(path.clone(), Some(&defaults)).unwrap();
    let initial: ConfigSnapshot = store.snapshot();
    assert_eq!(initial.path, path.canonicalize().unwrap());
    assert_eq!(initial.applied, defaults);
    assert_eq!(initial.saved, defaults);
    assert!(!initial.revision.is_empty());
    assert!(!initial.restart_required);
    assert_eq!(CommConfig::load(&path).unwrap(), defaults);
    drop(store);

    let original_bytes = fs::read(&path).unwrap();
    let mut other_defaults = defaults.clone();
    other_defaults.mfc.port = "COM8".into();
    let store = ConfigStore::open(path.clone(), Some(&other_defaults)).unwrap();
    assert_eq!(store.snapshot().saved, defaults);
    assert_eq!(fs::read(&path).unwrap(), original_bytes);
    drop(store);

    let broken_json = b"{\n  \"COM_RS485_MFC\": this is the operator's damaged file\n".to_vec();
    fs::write(&path, &broken_json).unwrap();
    assert!(ConfigStore::open(path.clone(), Some(&defaults)).is_err());
    assert_eq!(fs::read(&path).unwrap(), broken_json);
}

#[test]
fn save_changes_the_saved_config_and_restart_applies_it() {
    let directory = TestDirectory::new();
    let path = directory.config_path();
    let original = config();
    let mut store = ConfigStore::open(path.clone(), Some(&original)).unwrap();
    let initial = store.snapshot();
    let mut updated = original.clone();
    updated.mfc.port = "COM8".into();

    let saved = store.save(&initial.revision, updated.clone()).unwrap();

    assert_eq!(saved.path, initial.path);
    assert_ne!(saved.revision, initial.revision);
    assert_eq!(saved.applied, original);
    assert_eq!(saved.saved, updated);
    assert!(saved.restart_required);
    let current = store.snapshot();
    assert_eq!(current.revision, saved.revision);
    assert_eq!(current.applied, original);
    assert_eq!(current.saved, updated);
    assert!(current.restart_required);
    assert_eq!(CommConfig::load(&path).unwrap(), updated);
    drop(store);

    let reopened = ConfigStore::open(path, None).unwrap().snapshot();
    assert!(!reopened.revision.is_empty());
    assert_eq!(reopened.applied, updated);
    assert_eq!(reopened.saved, updated);
    assert!(!reopened.restart_required);
}

#[test]
fn stale_revision_and_external_byte_changes_cannot_be_overwritten() {
    let directory = TestDirectory::new();
    let path = directory.config_path();
    let original = config();
    let mut store = ConfigStore::open(path.clone(), Some(&original)).unwrap();
    let initial = store.snapshot();
    store.verify_saved_file().unwrap();
    let mut first_update = original.clone();
    first_update.mfc.port = "COM8".into();
    let saved = store.save(&initial.revision, first_update.clone()).unwrap();
    let saved_bytes = fs::read(&path).unwrap();
    let mut next_update = original.clone();
    next_update.mfc.port = "COM9".into();

    assert!(store.save(&initial.revision, next_update.clone()).is_err());
    assert_eq!(fs::read(&path).unwrap(), saved_bytes);
    assert_eq!(store.snapshot().revision, saved.revision);
    assert_eq!(store.snapshot().saved, first_update);

    // Even an external edit preserving the parsed configuration changes the revision.
    let mut externally_edited = saved_bytes;
    externally_edited.extend_from_slice(b"\n  \n");
    fs::write(&path, &externally_edited).unwrap();
    assert_eq!(CommConfig::load(&path).unwrap(), first_update);

    assert!(store.verify_saved_file().is_err());
    assert!(store.save(&saved.revision, next_update).is_err());
    assert_eq!(fs::read(&path).unwrap(), externally_edited);
    assert_eq!(store.snapshot().revision, saved.revision);
    assert_eq!(store.snapshot().applied, original);
    assert_eq!(store.snapshot().saved, first_update);
}

#[test]
fn canonical_and_symlink_aliases_share_ownership_until_the_store_is_dropped() {
    let directory = TestDirectory::new();
    let path = directory.config_path();
    let defaults = config();
    let store = ConfigStore::open(path.clone(), Some(&defaults)).unwrap();
    let canonical = path.canonicalize().unwrap();
    fs::create_dir(directory.0.join("nested")).unwrap();
    let relative_alias = directory
        .0
        .join("nested")
        .join("..")
        .join("comm_config.json");

    assert!(ConfigStore::open(canonical.clone(), None).is_err());
    assert!(ConfigStore::open(relative_alias.clone(), None).is_err());
    #[cfg(unix)]
    let symlink_alias = {
        let alias = directory.0.join("comm_config_alias.json");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert!(ConfigStore::open(alias.clone(), None).is_err());
        alias
    };
    drop(store);

    let reopened = ConfigStore::open(relative_alias, None).unwrap();
    assert_eq!(reopened.snapshot().path, canonical);
    drop(reopened);

    #[cfg(unix)]
    {
        let reopened = ConfigStore::open(symlink_alias, None).unwrap();
        assert_eq!(reopened.snapshot().path, canonical);
        assert!(ConfigStore::open(canonical.clone(), None).is_err());
        drop(reopened);
    }
    assert!(ConfigStore::open(canonical, None).is_ok());
}

#[test]
fn invalid_configs_are_rejected_during_initialization_load_and_save() {
    let directory = TestDirectory::new();
    let path = directory.config_path();
    let original = config();
    let mut invalid = original.clone();
    invalid.sampling.interval_s = 0.0;
    assert!(invalid.validate().is_err());

    assert!(ConfigStore::open(path.clone(), Some(&invalid)).is_err());
    assert!(
        !path.exists(),
        "invalid defaults must not create a configuration"
    );

    let mut store = ConfigStore::open(path.clone(), Some(&original)).unwrap();
    let initial = store.snapshot();
    let bytes_before = fs::read(&path).unwrap();
    assert!(store.save(&initial.revision, invalid.clone()).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes_before);
    let current = store.snapshot();
    assert_eq!(current.revision, initial.revision);
    assert_eq!(current.applied, original);
    assert_eq!(current.saved, original);
    assert!(!current.restart_required);
    drop(store);

    let invalid_bytes = serde_json::to_vec_pretty(&invalid).unwrap();
    fs::write(&path, &invalid_bytes).unwrap();
    assert!(ConfigStore::open(path.clone(), None).is_err());
    assert_eq!(fs::read(&path).unwrap(), invalid_bytes);
}

#[test]
fn saving_the_applied_config_again_clears_the_restart_requirement() {
    let directory = TestDirectory::new();
    let path = directory.config_path();
    let original = config();
    let mut store = ConfigStore::open(path.clone(), Some(&original)).unwrap();
    let initial = store.snapshot();
    let mut updated = original.clone();
    updated.mfc.port = "COM8".into();
    let changed = store.save(&initial.revision, updated).unwrap();
    assert!(changed.restart_required);

    let restored = store.save(&changed.revision, original.clone()).unwrap();

    assert_eq!(restored.applied, original);
    assert_eq!(restored.saved, original);
    assert!(!restored.restart_required);
    assert!(!store.snapshot().restart_required);
    assert_eq!(CommConfig::load(&path).unwrap(), original);
}

#[test]
fn another_process_is_excluded_and_process_exit_releases_the_lock() {
    let directory = TestDirectory::new();
    let path = directory.config_path();
    let original = config();
    let store = ConfigStore::open(path.clone(), Some(&original)).unwrap();
    let run_child = |mode: &str| {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "config_store_subprocess_lock_probe", "--ignored"])
            .env("TMH_CONFIG_STORE_TEST_PATH", &path)
            .env("TMH_CONFIG_STORE_TEST_MODE", mode)
            .status()
            .expect("configuration lock probe must finish")
    };

    assert!(
        run_child("expect_locked").success(),
        "a child process must reject the store held by its parent"
    );
    drop(store);
    assert!(
        run_child("exit_holding_lock").success(),
        "the child must acquire the released lock before exiting without destructors"
    );

    let reopened = ConfigStore::open(path, None)
        .expect("the operating system must release the exited child's lock");
    assert_eq!(reopened.snapshot().applied, original);
}

#[test]
#[ignore = "invoked only by the cross-process configuration lock test"]
fn config_store_subprocess_lock_probe() {
    let path = PathBuf::from(
        std::env::var_os("TMH_CONFIG_STORE_TEST_PATH").expect("lock probe configuration path"),
    );
    match std::env::var("TMH_CONFIG_STORE_TEST_MODE")
        .expect("lock probe mode")
        .as_str()
    {
        "expect_locked" => assert!(
            ConfigStore::open(path, None).is_err(),
            "the parent still owns this configuration"
        ),
        "exit_holding_lock" => {
            let _store = ConfigStore::open(path, None).expect("parent released its lock");
            // Bypass Rust destructors: release must come from process termination.
            std::process::exit(0);
        }
        mode => panic!("unknown lock probe mode: {mode}"),
    }
}
