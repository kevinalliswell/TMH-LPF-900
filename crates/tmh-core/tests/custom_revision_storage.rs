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
