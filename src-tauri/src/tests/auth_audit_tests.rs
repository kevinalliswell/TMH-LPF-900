use super::tests::{admin, entries, fixture};
use super::*;

fn file_fixture() -> (AppState, rusqlite::Connection) {
    let state = fixture();
    let path = state.recovery_dir.parent().unwrap().join("auth-test.db");
    *lock(&state.storage) = Storage::open(&path).unwrap();
    let connection = rusqlite::Connection::open(path).unwrap();
    admin(&state);
    (state, connection)
}

fn assert_entry(
    state: &AppState,
    before: usize,
    action: &str,
    result: &str,
    operator: &str,
    code: Option<&str>,
) {
    let log = entries(state);
    assert_eq!(
        log.len(),
        before + 1,
        "one audit describes the complete command result"
    );
    let entry = log.last().unwrap();
    assert_eq!(entry["category"], "AUTH");
    assert_eq!(entry["action"], action);
    assert_eq!(entry["result"], result);
    assert_eq!(entry["operator"], operator);
    assert_eq!(entry["detail"]["code"], json!(code));
    let text = serde_json::to_string(&log).unwrap();
    for secret in [
        "SecretPassword!",
        "ChangedPassword!",
        "wrong-password",
        "$argon2",
        "injected-database-diagnostic",
    ] {
        assert!(
            !text.contains(secret),
            "audit exposed secret or raw database diagnostics"
        );
    }
}

#[test]
fn login_database_write_failure_is_failure_with_stable_code() {
    let (state, db) = file_fixture();
    state.auth.logout();
    db.execute_batch("CREATE TRIGGER fail_login BEFORE UPDATE OF last_login_at ON users BEGIN SELECT RAISE(ABORT, 'injected-database-diagnostic'); END;").unwrap();
    let before = entries(&state).len();
    assert_eq!(
        login(&state, "ADMIN".into(), "SecretPassword!".into()).unwrap_err(),
        "STORAGE_ERROR"
    );
    assert_entry(
        &state,
        before,
        "login",
        "FAILURE",
        "ADMIN",
        Some("STORAGE_ERROR"),
    );
    assert!(state
        .auth
        .snapshot(&lock(&state.storage), epoch_now())
        .unwrap()
        .user
        .is_none());
}

#[test]
fn own_password_database_write_failure_keeps_actor_and_does_not_change_password() {
    let (state, db) = file_fixture();
    db.execute_batch("CREATE TRIGGER fail_password BEFORE UPDATE OF password_hash ON users BEGIN SELECT RAISE(ABORT, 'injected-database-diagnostic'); END;").unwrap();
    let before = entries(&state).len();
    assert_eq!(
        change_own_password(&state, "SecretPassword!".into(), "ChangedPassword!".into())
            .unwrap_err(),
        "STORAGE_ERROR"
    );
    assert_entry(
        &state,
        before,
        "change_password",
        "FAILURE",
        "Administrator",
        Some("STORAGE_ERROR"),
    );
    db.execute_batch("DROP TRIGGER fail_password;").unwrap();
    state.auth.logout();
    assert!(login(&state, "admin".into(), "SecretPassword!".into()).is_ok());
}

#[test]
fn login_and_own_password_normal_outcomes_remain_rejected_or_success_without_secrets() {
    let state = fixture();
    admin(&state);
    state.auth.logout();
    let mut before = entries(&state).len();
    assert_eq!(
        login(&state, "ADMIN".into(), "wrong-password".into()).unwrap_err(),
        "AUTH_FAILED"
    );
    assert_entry(
        &state,
        before,
        "login",
        "REJECTED",
        "ADMIN",
        Some("AUTH_FAILED"),
    );
    before += 1;
    let snapshot = login(&state, "ADMIN".into(), "SecretPassword!".into()).unwrap();
    assert_eq!(snapshot.user.unwrap().display_name, "Administrator");
    assert_entry(&state, before, "login", "SUCCESS", "Administrator", None);
    before += 1;
    assert_eq!(
        change_own_password(&state, "wrong-password".into(), "ChangedPassword!".into())
            .unwrap_err(),
        "AUTH_FAILED"
    );
    assert_entry(
        &state,
        before,
        "change_password",
        "REJECTED",
        "Administrator",
        Some("AUTH_FAILED"),
    );
    before += 1;
    assert_eq!(
        change_own_password(&state, "SecretPassword!".into(), "x".into()).unwrap_err(),
        "INVALID_PARAMS"
    );
    assert_entry(
        &state,
        before,
        "change_password",
        "REJECTED",
        "Administrator",
        Some("INVALID_PARAMS"),
    );
    before += 1;
    change_own_password(&state, "SecretPassword!".into(), "ChangedPassword!".into()).unwrap();
    assert_entry(
        &state,
        before,
        "change_password",
        "SUCCESS",
        "Administrator",
        None,
    );
    before += 1;
    state.auth.lock();
    assert_eq!(
        change_own_password(&state, "ChangedPassword!".into(), "SecretPassword!".into())
            .unwrap_err(),
        "AUTH_REQUIRED"
    );
    assert_entry(
        &state,
        before,
        "change_password",
        "REJECTED",
        "anonymous",
        Some("AUTH_REQUIRED"),
    );
}

#[test]
fn login_final_snapshot_failure_is_one_failure_not_a_prior_success() {
    let state = fixture();
    admin(&state);
    state.auth.logout();
    let before = entries(&state).len();
    state.auth.fail_next_snapshot_for_test();
    assert_eq!(
        login(&state, "ADMIN".into(), "SecretPassword!".into()).unwrap_err(),
        "STORAGE_ERROR"
    );
    // Credentials were verified; this failure is attributed to the actual account,
    // while the audit outcome must follow the complete response sent to the caller.
    assert_entry(
        &state,
        before,
        "login",
        "FAILURE",
        "Administrator",
        Some("STORAGE_ERROR"),
    );
}

#[test]
fn rejected_credentials_with_unwritable_failure_counter_are_storage_failure() {
    let (state, db) = file_fixture();
    state.auth.logout();
    db.execute_batch("CREATE TRIGGER fail_counter BEFORE UPDATE OF failed_login_count ON users BEGIN SELECT RAISE(ABORT, 'injected-database-diagnostic'); END;").unwrap();
    let before = entries(&state).len();
    assert_eq!(
        login(&state, "ADMIN".into(), "wrong-password".into()).unwrap_err(),
        "STORAGE_ERROR"
    );
    assert_entry(
        &state,
        before,
        "login",
        "FAILURE",
        "ADMIN",
        Some("STORAGE_ERROR"),
    );
    assert_eq!(
        lock(&state.storage)
            .get_user_by_username("admin")
            .unwrap()
            .unwrap()
            .failed_login_count,
        0
    );
}
