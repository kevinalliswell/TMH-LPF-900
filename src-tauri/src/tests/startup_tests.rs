//! Composition-root failure paths: none may construct a runnable mock hub on failure.
use super::*;

fn profile() -> PathBuf {
    std::env::temp_dir().join(format!("tmh_startup_{}", tmh_core::auth::new_user_id()))
}
fn failed(mode: DeviceMode, root: &std::path::Path, explicit: Option<PathBuf>) -> CommandError {
    match initialize_profile(mode, root.to_path_buf(), explicit, 1.0, false) {
        Ok(_) => panic!("invalid startup unexpectedly produced AppState"),
        Err(error) => error,
    }
}
#[test]
fn unsupported_real_and_replay_fail_before_any_profile_or_mock_bus_is_created() {
    for (mode, code) in [
        (DeviceMode::Real, "REAL_NOT_READY"),
        (DeviceMode::Replay, "REPLAY_NOT_READY"),
    ] {
        let root = profile();
        assert_eq!(failed(mode, &root, None).code, code);
        assert!(!root.exists());
    }
}
#[test]
fn invalid_config_is_preserved_and_can_be_repaired_after_failed_startup() {
    let root = profile();
    let config = root.join("config/comm_config.json");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, "invalid-json").unwrap();
    assert_eq!(failed(DeviceMode::Mock, &root, None).code, "CONFIG_INVALID");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "invalid-json");
    assert!(!root.join("data/experiments.db").exists());
    std::fs::write(&config, DEFAULT_COMM_CONFIG).unwrap();
    let state = initialize_profile(DeviceMode::Mock, root.clone(), None, 1.0, false)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(state.runtime_mode.requested_mode, DeviceMode::Mock);
    assert_eq!(state.runtime_mode.effective_mode, DeviceMode::Mock);
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn missing_explicit_config_and_unusable_profile_do_not_panic_or_fall_back() {
    let root = profile();
    assert_eq!(
        failed(DeviceMode::Mock, &root, Some(root.join("missing.json"))).code,
        "CONFIG_INVALID"
    );
    assert!(!root.join("config/comm_config.json").exists());
    std::fs::remove_dir_all(&root).unwrap();
    std::fs::write(&root, "file blocks directory").unwrap();
    assert_eq!(
        failed(DeviceMode::Mock, &root, None).code,
        "DATA_DIRECTORY_LOCKED"
    );
    std::fs::remove_file(root).unwrap();
}
