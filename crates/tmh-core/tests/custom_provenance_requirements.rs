//! Independent tests for the agreed custom-revision API. The submitted Program
//! is untrusted; `previous` and ProgramEditor are supplied by the server.
//! Changes describe conditions relative to the preserved standard parent, not
//! merely the most recent edit. Stage insertion/removal policy is intentionally
//! outside these tests.

use serde_json::json;
use tmh_core::engine::{standard_program, Program, ProgramSource, StageKind};
use tmh_core::provenance::{prepare_custom_revision, ProgramEditor};

fn editor(user_id: &str, name: &str) -> ProgramEditor {
    ProgramEditor {
        user_id: user_id.into(),
        display_name: name.into(),
        edited_at: "2026-09-08T10:00:00Z".into(),
    }
}

fn draft() -> Program {
    let mut program = standard_program("REDUCIBILITY").unwrap();
    program.id = "custom-provenance-test".into();
    program.name = "Adjusted reduction conditions".into();
    program.category = "custom".into();
    program.source = ProgramSource::Custom;
    let index = reduction_index(&program);
    program.stages[index].target_temp = 850.0;
    program
}

fn reduction_index(program: &Program) -> usize {
    program
        .stages
        .iter()
        .position(|stage| stage.kind == StageKind::Reducing)
        .unwrap()
}

fn first_revision() -> Program {
    prepare_custom_revision(
        draft(),
        None,
        editor("editor-a", "Operator A"),
        "Compare reduction at 850 C",
    )
    .unwrap()
}

#[test]
fn a_new_revision_replaces_forged_provenance_with_server_identity_and_real_changes() {
    let parent = standard_program("REDUCIBILITY").unwrap();
    let mut request_json = serde_json::to_value(draft()).unwrap();
    let mut forged_parent = parent.clone();
    forged_parent.stages[reduction_index(&parent)].target_temp = 850.0;
    request_json["provenance"] = json!({
        "revision": 999,
        "parent_template": forged_parent,
        "edited_by_user_id": "forged-admin",
        "edited_by": "Forged administrator",
        "edited_at": "1900-01-01T00:00:00Z",
        "reason": "forged reason",
        "changes": []
    });
    let request: Program = serde_json::from_value(request_json).unwrap();
    let saved = prepare_custom_revision(
        request,
        None,
        editor("editor-a", "Operator A"),
        "Compare reduction at 850 C",
    )
    .unwrap();
    let provenance = saved.provenance.as_ref().unwrap();
    assert_eq!(provenance.revision, 1);
    assert_eq!(provenance.edited_by_user_id, "editor-a");
    assert_eq!(provenance.edited_by, "Operator A");
    assert_eq!(provenance.edited_at, "2026-09-08T10:00:00Z");
    assert_eq!(provenance.reason, "Compare reduction at 850 C");
    assert_eq!(provenance.parent_template.as_ref(), &parent);
    assert_eq!(provenance.changes.len(), 1);
    let change = &provenance.changes[0];
    assert_eq!(change.stage_index, reduction_index(&parent));
    assert_eq!(change.field, "target_temp");
    assert_eq!(change.before, json!(900.0));
    assert_eq!(change.after, json!(850.0));
}

#[test]
fn updating_increments_the_revision_and_recomputes_differences_from_the_original_parent() {
    let previous = first_revision();
    let historical_snapshot = serde_json::to_value(&previous).unwrap();
    let mut request = previous.clone();
    let index = reduction_index(&request);
    request.stages[index].target_temp = 800.0;
    let submitted_provenance = request.provenance.as_mut().unwrap();
    submitted_provenance.edited_by_user_id = "forged-admin".into();
    submitted_provenance.edited_by = "Forged administrator".into();
    submitted_provenance.parent_template.stages[index].target_temp = 800.0;
    submitted_provenance.changes[0].field = "forged-field".into();
    submitted_provenance.changes[0].before = json!(800.0);
    submitted_provenance.changes[0].after = json!(800.0);

    let saved = prepare_custom_revision(
        request,
        Some(&previous),
        editor("editor-b", "Operator B"),
        "Compare reduction at 800 C",
    )
    .unwrap();
    let provenance = saved.provenance.as_ref().unwrap();
    assert_eq!(provenance.revision, 2);
    assert_eq!(provenance.edited_by_user_id, "editor-b");
    assert_eq!(provenance.edited_by, "Operator B");
    assert_eq!(
        provenance.parent_template,
        previous.provenance.as_ref().unwrap().parent_template
    );
    assert_eq!(provenance.changes.len(), 1);
    assert_eq!(provenance.changes[0].field, "target_temp");
    assert_eq!(provenance.changes[0].before, json!(900.0));
    assert_eq!(provenance.changes[0].after, json!(800.0));
    assert_eq!(
        serde_json::to_value(&previous).unwrap(),
        historical_snapshot
    );
}

#[test]
fn missing_or_stale_update_revisions_are_rejected_without_rewriting_history() {
    let previous = first_revision();
    let historical_snapshot = serde_json::to_value(&previous).unwrap();
    for revision in [None, Some(0), Some(2)] {
        let mut request = previous.clone();
        if let Some(revision) = revision {
            request.provenance.as_mut().unwrap().revision = revision;
        } else {
            request.provenance = None;
        }
        assert!(
            prepare_custom_revision(
                request,
                Some(&previous),
                editor("editor-b", "Operator B"),
                "Attempt an update",
            )
            .is_err(),
            "request revision {revision:?} must not overwrite saved revision 1"
        );
    }
    assert_eq!(
        serde_json::to_value(&previous).unwrap(),
        historical_snapshot
    );
}

#[test]
fn whitespace_only_reasons_are_rejected_for_both_creation_and_update() {
    let previous = first_revision();
    for reason in ["", " \n\t "] {
        assert!(prepare_custom_revision(draft(), None, editor("a", "A"), reason).is_err());
        assert!(prepare_custom_revision(
            previous.clone(),
            Some(&previous),
            editor("b", "B"),
            reason,
        )
        .is_err());
    }
}

#[test]
fn unknown_standard_or_forged_year_or_template_revision_is_rejected() {
    for field in [
        "base_standard_id",
        "base_standard_version",
        "template_revision",
    ] {
        let mut request = draft();
        match field {
            "base_standard_id" => request.base_standard_id = Some("UNKNOWN-STANDARD".into()),
            "base_standard_version" => request.base_standard_version = Some("2099".into()),
            "template_revision" => request.template_revision = Some("forged-template".into()),
            _ => unreachable!(),
        }
        assert!(
            prepare_custom_revision(request, None, editor("a", "A"), "Changed conditions").is_err(),
            "forged {field} must not produce an authenticated provenance record"
        );
    }
}

#[test]
fn an_update_cannot_silently_change_its_standard_lineage() {
    let previous = first_revision();
    let other_standard = standard_program("LOW_TEMP_DEGRADATION").unwrap();
    let mut request = previous.clone();
    request.base_standard_id = other_standard.base_standard_id;
    request.base_standard_version = other_standard.base_standard_version;
    request.template_revision = other_standard.template_revision;
    assert!(prepare_custom_revision(
        request,
        Some(&previous),
        editor("editor-b", "Operator B"),
        "Attempt to change the base method",
    )
    .is_err());
}

#[test]
fn an_update_preserves_a_stored_parent_snapshot_after_the_bundled_template_changes() {
    // `previous` is trusted storage input. Model a revision made against a prior
    // bundled template; it must not be rebound to today's template on edit.
    let mut previous = first_revision();
    let index = reduction_index(&previous);
    previous.template_revision = Some("historical-template-revision".into());
    let parent = &mut previous.provenance.as_mut().unwrap().parent_template;
    parent.template_revision = previous.template_revision.clone();
    parent.stages[index].target_temp = 910.0;
    previous.provenance.as_mut().unwrap().changes[0].before = json!(910.0);
    let old_parent = previous
        .provenance
        .as_ref()
        .unwrap()
        .parent_template
        .clone();

    let mut request = previous.clone();
    request.stages[index].target_temp = 800.0;
    let saved = prepare_custom_revision(
        request,
        Some(&previous),
        editor("editor-b", "Operator B"),
        "Continue the existing research series",
    )
    .unwrap();
    let provenance = saved.provenance.as_ref().unwrap();
    assert_eq!(saved.template_revision, previous.template_revision);
    assert_eq!(provenance.revision, 2);
    assert_eq!(provenance.parent_template, old_parent);
    assert_eq!(provenance.changes[0].before, json!(910.0));
    assert_eq!(provenance.changes[0].after, json!(800.0));
}

#[test]
fn execution_requires_current_parent_and_preserves_protected_ending_steps() {
    use tmh_core::provenance::validate_executable_program;
    assert!(validate_executable_program(&draft()).is_err());
    assert!(validate_executable_program(&first_revision()).is_ok());
    let mut request = draft();
    request.stages[3].gas[1] = 0.0;
    assert!(prepare_custom_revision(request, None, editor("a", "A"), "Change purge").is_err());
    let mut request = first_revision();
    request
        .provenance
        .as_mut()
        .unwrap()
        .parent_template
        .template_revision = Some("old".into());
    assert!(validate_executable_program(&request).is_err());
}

#[test]
fn execution_limits_include_absolute_ceiling_and_wire_resolution() {
    use tmh_core::provenance::{validate_flow_resolution, validate_gas_limits};
    let mut program = draft();
    program.stages[2].gas[0] = 11.0;
    assert!(validate_gas_limits(&program, 999.0, 999.0).is_err());
    program.stages[2].gas[0] = 0.0;
    assert!(validate_gas_limits(&program, 5.0, 4.0).is_err());
    assert!(validate_gas_limits(&program, 5.0, 5.0).is_ok());
    let steps = [0.01, 0.1, 0.01, 0.01];
    assert!(validate_flow_resolution(&program, steps).is_ok());
    program.stages[2].gas[1] = 10.55;
    assert!(validate_flow_resolution(&program, steps).is_err());
    program.stages[2].gas[1] = 10.6;
    assert!(validate_flow_resolution(&program, steps).is_ok());
    assert!(validate_flow_resolution(&program, [0.0; 4]).is_err());
}

#[test]
fn declaring_standard_source_cannot_bypass_the_bundled_template() {
    use tmh_core::provenance::validate_executable_program;
    let standard = standard_program("REDUCIBILITY").unwrap();
    assert!(validate_executable_program(&standard).is_ok());
    let mut forged = standard.clone();
    forged.stages[2].target_temp = 850.0;
    assert!(validate_executable_program(&forged).is_err());
    forged = standard.clone();
    forged.id = "custom-disguised".into();
    assert!(validate_executable_program(&forged).is_err());
    forged = standard;
    forged.template_revision = Some("outdated".into());
    assert!(validate_executable_program(&forged).is_err());
}
