use serde_json::json;
use tmh_core::auth::UserRole;
use tmh_core::storage::{Storage, TestGroupRevision, UserRow};

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

fn record(owner: &str) -> TestGroupRevision {
    TestGroupRevision {
        group_id: "group-storage-test".into(),
        revision: 1,
        owner_user_id: owner.into(),
        recorded_by_user_id: owner.into(),
        created_at: "2026-09-08T12:00:00Z".into(),
        snapshot: json!({"name":"Batch <A>","trials":[{"id":"trial1","analysis":{"ri":42.5}}]}),
    }
}
#[test]
fn group_revisions_are_append_only_and_stale_saves_do_not_overwrite() {
    let mut db = Storage::open_in_memory().unwrap();
    let admin = user("admin-id", "admin", UserRole::Admin);
    db.insert_user(&admin).unwrap();
    let first = record(&admin.user_id);
    db.save_test_group_revision(&first, None).unwrap();
    let mut second = first.clone();
    second.revision = 2;
    second.snapshot["name"] = json!("Edited group");
    assert!(db.save_test_group_revision(&second, None).is_err());
    assert!(db.save_test_group_revision(&first, Some(1)).is_err());
    db.save_test_group_revision(&second, Some(1)).unwrap();
    assert!(db.save_test_group_revision(&second, Some(1)).is_err());
    assert_eq!(
        db.test_group_revision(&first.group_id, None).unwrap(),
        Some(second.clone())
    );
    assert_eq!(
        db.test_group_revision(&first.group_id, Some(1)).unwrap(),
        Some(first.clone())
    );
    assert_eq!(
        db.test_group_history(&first.group_id, None).unwrap(),
        vec![second.clone(), first.clone()]
    );
    assert_eq!(
        db.test_group_history(&first.group_id, Some(2)).unwrap(),
        vec![first]
    );
    assert_eq!(
        db.list_test_groups(Some(&admin.user_id), 0).unwrap(),
        vec![second]
    );
    assert!(db
        .list_test_groups(Some("another-user"), 0)
        .unwrap()
        .is_empty());
    assert!(db.list_test_groups(None, 25).unwrap().is_empty());
}
#[test]
fn migration_reopen_and_failed_insert_preserve_the_prior_complete_snapshot() {
    let path = std::env::temp_dir().join(format!(
        "tmh-group-{}.sqlite",
        tmh_core::auth::new_user_id()
    ));
    let mut db = Storage::open(&path).unwrap();
    let admin = user("admin-id", "admin", UserRole::Admin);
    db.insert_user(&admin).unwrap();
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("DROP TABLE test_group_revisions; PRAGMA user_version=7;")
        .unwrap();
    drop(conn);
    db = Storage::open(&path).unwrap();
    assert_eq!(db.user_version().unwrap(), 8);
    let first = record(&admin.user_id);
    db.save_test_group_revision(&first, None).unwrap();
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER group_write_failure BEFORE INSERT ON test_group_revisions WHEN NEW.revision=2 BEGIN SELECT RAISE(ABORT,'test disk write failure'); END;").unwrap();
    drop(conn);
    db = Storage::open(&path).unwrap();
    let mut second = first.clone();
    second.revision = 2;
    assert!(db.save_test_group_revision(&second, Some(1)).is_err());
    assert_eq!(
        db.test_group_history(&first.group_id, None).unwrap(),
        vec![first]
    );
    drop(db);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn group_identity_owner_and_bounded_json_are_validated_before_writing() {
    let mut db = Storage::open_in_memory().unwrap();
    let admin = user("admin-id", "admin", UserRole::Admin);
    db.insert_user(&admin).unwrap();
    let first = record(&admin.user_id);
    for changed in [0, 2, u64::MAX] {
        let mut bad = first.clone();
        bad.revision = changed;
        assert!(db.save_test_group_revision(&bad, None).is_err());
    }
    for invalid in [
        json!(null),
        json!([]),
        json!({"large":"x".repeat(1_048_577)}),
    ] {
        let mut bad = first.clone();
        bad.snapshot = invalid;
        assert!(db.save_test_group_revision(&bad, None).is_err());
    }
    let mut deep = json!(null);
    for _ in 0..130 {
        deep = json!({"child":deep});
    }
    let mut bad = first.clone();
    bad.snapshot = deep;
    assert!(db.save_test_group_revision(&bad, None).is_err());
    db.save_test_group_revision(&first, None).unwrap();
    let mut bad = first.clone();
    bad.revision = 2;
    bad.owner_user_id = "different-owner".into();
    assert!(db.save_test_group_revision(&bad, Some(1)).is_err());
    assert_eq!(
        db.test_group_history(&first.group_id, None).unwrap(),
        vec![first]
    );
}
