use tmh_core::engine::{standard_program, ProgramSource};
use tmh_core::provenance::{prepare_custom_revision, ProgramEditor};
use tmh_core::storage::Storage;
fn editor() -> ProgramEditor {
    ProgramEditor {
        user_id: "admin".into(),
        display_name: "Admin".into(),
        edited_at: "2026-09-08T00:00:00Z".into(),
    }
}
#[test]
fn stale_updates_cannot_overwrite_a_new_revision_and_history_survives_deletion() {
    let mut db = Storage::open_in_memory().unwrap();
    let mut p = standard_program("REDUCIBILITY").unwrap();
    p.id = "custom-revisions".into();
    p.source = ProgramSource::Custom;
    let first = prepare_custom_revision(p, None, editor(), "Initial").unwrap();
    db.save_custom_program_revision(&first, None).unwrap();
    let old = db.get_custom_program(&first.id).unwrap().unwrap();
    let mut request = first.clone();
    request.stages[2].target_temp = 850.0;
    let second = prepare_custom_revision(request, Some(&first), editor(), "850 C").unwrap();
    db.save_custom_program_revision(&second, Some(&old))
        .unwrap();
    assert!(db
        .save_custom_program_revision(&second, Some(&old))
        .is_err());
    let latest = db.get_custom_program(&first.id).unwrap().unwrap();
    assert_eq!(latest, serde_json::to_string(&second).unwrap());
    assert_eq!(db.custom_program_revisions(&first.id).unwrap().len(), 2);
    db.delete_custom_program(&first.id).unwrap();
    assert!(db.get_custom_program(&first.id).unwrap().is_none());
    assert_eq!(db.custom_program_revisions(&first.id).unwrap().len(), 2);
}
#[test]
fn upgrading_a_legacy_program_preserves_the_exact_unversioned_json() {
    let mut db = Storage::open_in_memory().unwrap();
    let mut p = standard_program("FREE_SWELLING").unwrap();
    p.id = "custom-old".into();
    p.source = ProgramSource::Custom;
    let old = serde_json::to_string(&p).unwrap();
    db.save_custom_program(&p.id, &old).unwrap();
    let saved = prepare_custom_revision(p.clone(), Some(&p), editor(), "Add traceability").unwrap();
    db.save_custom_program_revision(&saved, Some(&old)).unwrap();
    let history = db.custom_program_revisions(&p.id).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[1], old);
}

#[test]
fn legacy_edits_between_migration_and_version_one_are_all_retained() {
    let path =
        std::env::temp_dir().join(format!("tmh_legacy_{}.db", tmh_core::auth::new_user_id()));
    let db = Storage::open(&path).unwrap();
    let mut p = standard_program("REDUCIBILITY").unwrap();
    p.id = "custom-migrated".into();
    p.source = ProgramSource::Custom;
    let before = serde_json::to_string(&p).unwrap();
    db.save_custom_program(&p.id, &before).unwrap();
    drop(db);
    let old = rusqlite::Connection::open(&path).unwrap();
    old.execute_batch("DROP TABLE test_group_revisions; DROP TABLE custom_program_revisions; PRAGMA user_version=6;")
        .unwrap();
    drop(old);
    let mut db = Storage::open(&path).unwrap();
    p.stages[2].target_temp = 850.0;
    let current = serde_json::to_string(&p).unwrap();
    db.save_custom_program(&p.id, &current).unwrap();
    let versioned =
        prepare_custom_revision(p.clone(), Some(&p), editor(), "Version the edited program")
            .unwrap();
    db.save_custom_program_revision(&versioned, Some(&current))
        .unwrap();
    let history = db.custom_program_revisions(&p.id).unwrap();
    assert_eq!(
        history,
        vec![
            serde_json::to_string(&versioned).unwrap(),
            current,
            before.clone()
        ]
    );
    assert!(db.save_custom_program(&p.id, &before).is_err());
    assert_eq!(db.custom_program_revisions(&p.id).unwrap(), history);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_failed_history_insert_rolls_back_the_current_program_and_old_baseline() {
    let path =
        std::env::temp_dir().join(format!("tmh_revision_{}.db", tmh_core::auth::new_user_id()));
    let mut db = Storage::open(&path).unwrap();
    let mut p = standard_program("REDUCIBILITY").unwrap();
    p.id = "custom-atomic".into();
    p.source = ProgramSource::Custom;
    let raw = serde_json::to_string(&p).unwrap();
    db.save_custom_program(&p.id, &raw).unwrap();
    let first = prepare_custom_revision(p, None, editor(), "Initial").unwrap();
    let fault = rusqlite::Connection::open(&path).unwrap();
    fault.execute_batch("CREATE TRIGGER fail_revision BEFORE INSERT ON custom_program_revisions WHEN NEW.revision=1 BEGIN SELECT RAISE(ABORT,'test write failure'); END;").unwrap();
    assert!(db.save_custom_program_revision(&first, Some(&raw)).is_err());
    assert_eq!(
        db.get_custom_program(&first.id).unwrap().as_deref(),
        Some(raw.as_str())
    );
    assert_eq!(
        db.custom_program_revisions(&first.id).unwrap(),
        vec![raw.clone()]
    );
    fault.execute_batch("DROP TRIGGER fail_revision;").unwrap();
    db.save_custom_program_revision(&first, Some(&raw)).unwrap();
    assert_eq!(db.custom_program_revisions(&first.id).unwrap().len(), 2);
    drop(fault);
    drop(db);
    std::fs::remove_file(path).unwrap();
}
