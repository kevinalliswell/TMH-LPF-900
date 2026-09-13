use super::*;

fn fixture() -> (PathBuf, CommConfig) {
    let path = std::env::temp_dir()
        .join(format!("tmh_config_failure_{}", Uuid::new_v4()))
        .join("comm_config.json");
    let config = CommConfig::from_json(include_str!("../../../config/comm_config.json")).unwrap();
    (path, config)
}

#[test]
fn failed_replace_preserves_file_and_version() {
    let (path, original) = fixture();
    let mut store = ConfigStore::open(path.clone(), Some(&original)).unwrap();
    let initial = store.snapshot();
    let old_bytes = fs::read(&path).unwrap();
    let mut updated = original.clone();
    updated.mfc.port = "COM9".into();
    let result = store.save_with_io(
        &initial.revision,
        updated,
        |_, _| Err(std::io::Error::other("injected replace failure")),
        sync_directory,
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&path).unwrap(), old_bytes);
    assert_eq!(store.snapshot(), initial);
    assert!(!fs::read_dir(path.parent().unwrap()).unwrap().any(|entry| {
        entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|e| e == "tmp")
    }));
    drop(store);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn failed_directory_sync_reports_the_replaced_file_and_new_version_truthfully() {
    let (path, original) = fixture();
    let mut store = ConfigStore::open(path.clone(), Some(&original)).unwrap();
    let initial = store.snapshot();
    let mut updated = original.clone();
    updated.mfc.port = "COM9".into();
    let result = store.save_with_io(
        &initial.revision,
        updated.clone(),
        |from, to| fs::rename(from, to),
        |_| Err(std::io::Error::other("injected directory sync failure")),
    );
    assert!(matches!(
        result,
        Err(ConfigStoreError::DurabilityUnconfirmed(_))
    ));
    assert_eq!(CommConfig::load(&path).unwrap(), updated);
    let current = store.snapshot();
    assert_ne!(current.revision, initial.revision);
    assert_eq!(current.applied, original);
    assert_eq!(current.saved, updated);
    assert!(current.restart_required);
    assert!(store.save(&initial.revision, original).is_err());
    drop(store);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
