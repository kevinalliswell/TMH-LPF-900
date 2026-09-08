use tmh_core::repeat_group::{evaluate, GroupPolicy, LimitSpec, MaterialKind, TrialIndices};

fn policy(method: &str) -> GroupPolicy {
    GroupPolicy {
        method: method.into(),
        custom: false,
        material: MaterialKind::Pellet,
        manual_limits: None,
    }
}
fn trial(id: &str, value: f64) -> TrialIndices {
    TrialIndices {
        experiment_id: id.into(),
        primary: value,
        rvi: None,
        rdi_plus_6_3: None,
        rdi_minus_0_5: None,
    }
}
#[test]
fn ri_reference_rate_uses_the_same_selected_trials_and_missing_is_not_zero() {
    let trials: Vec<_> = [40.0, 44.0, 42.0, 43.0]
        .into_iter()
        .enumerate()
        .map(|(i, v)| {
            let mut t = trial(&format!("e{i}"), v);
            t.rvi = Some([1.0, 99.0, 2.0, 4.0][i]);
            t
        })
        .collect();
    let result = evaluate(&policy("REDUCIBILITY"), &trials).unwrap();
    assert_eq!(result.decision.selected_indices, vec![2, 3]);
    assert_eq!(result.reported["RI"].as_deref(), Some("42.5"));
    assert_eq!(result.reported["RVI"].as_deref(), Some("3.00"));
    let mut missing = trials;
    missing[2].rvi = None;
    let result = evaluate(&policy("REDUCIBILITY"), &missing).unwrap();
    assert_eq!(result.reported["RI"].as_deref(), Some("42.5"));
    assert_eq!(result.reported["RVI"], None);
}
#[test]
fn fsi_requests_a_third_trial_before_four_while_ri_goes_directly_to_four() {
    let trials = [trial("first", 10.0), trial("second", 14.0)];
    let result = evaluate(&policy("FREE_SWELLING"), &trials).unwrap();
    assert_eq!(result.decision.required_count, 3);
    assert!(result.reported.is_empty());
    assert_eq!(
        evaluate(&policy("REDUCIBILITY"), &trials)
            .unwrap()
            .decision
            .required_count,
        4
    );
}
#[test]
fn rdi_uses_one_decimal_trial_results_and_keeps_the_limit_basis() {
    let mut p = policy("LOW_TEMP_DEGRADATION");
    assert!(evaluate(&p, &[trial("a", 88.65)]).is_err());
    p.manual_limits = Some(LimitSpec {
        a: 0.2,
        b: 0.3,
        c: 0.4,
        basis: "Lab agreement for this batch".into(),
    });
    let mut a = trial("a", 88.65);
    a.rdi_plus_6_3 = Some(66.65);
    a.rdi_minus_0_5 = Some(2.25);
    let mut b = trial("b", 88.75);
    b.rdi_plus_6_3 = Some(70.75);
    b.rdi_minus_0_5 = Some(2.35);
    let result = evaluate(&p, &[a, b]).unwrap();
    assert_eq!(result.decision_values, vec![88.6, 88.8]);
    assert_eq!(result.reported["RDI+3.15"].as_deref(), Some("88.7"));
    assert_eq!(result.reported["RDI+6.3"].as_deref(), Some("68.7"));
    assert_eq!(result.reported["RDI−0.5"].as_deref(), Some("2.3"));
    assert_eq!(result.limits.basis, "Lab agreement for this batch");
}
#[test]
fn natural_ore_requires_agreed_limits_and_fixed_material_rules_cannot_be_overridden() {
    let trials = [trial("a", 10.0), trial("b", 13.5)];
    let mut p = policy("REDUCIBILITY");
    p.material = MaterialKind::Sinter;
    assert_eq!(evaluate(&p, &trials).unwrap().decision.required_count, 2);
    p.material = MaterialKind::Natural;
    assert!(evaluate(&p, &trials).is_err());
    p.manual_limits = Some(LimitSpec {
        a: 4.0,
        b: 5.0,
        c: 6.0,
        basis: "Supplier/customer agreement R-1".into(),
    });
    assert_eq!(
        evaluate(&p, &trials).unwrap().reported["RI"].as_deref(),
        Some("11.8")
    );
    p.material = MaterialKind::Pellet;
    assert!(evaluate(&p, &trials).is_err());
    p.material = MaterialKind::Natural;
    p.manual_limits.as_mut().unwrap().basis = " ".into();
    assert!(evaluate(&p, &trials).is_err());
    assert!(evaluate(&policy("UNKNOWN"), &trials).is_err());
}
#[test]
fn custom_results_keep_research_labels_and_duplicate_or_surplus_trials_are_rejected() {
    let mut p = policy("REDUCIBILITY");
    p.custom = true;
    let trials = [trial("a", 10.0), trial("b", 12.0)];
    let result = evaluate(&p, &trials).unwrap();
    assert_eq!(result.reported["Rt"].as_deref(), Some("11.0"));
    assert!(!result.reported.contains_key("RI"));
    assert!(evaluate(&p, &[trial("a", 10.0), trial("a", 12.0)]).is_err());
    assert!(evaluate(&p, &[trial("a", 10.0), trial("b", 12.0), trial("c", 99.0)]).is_err());
    assert!(evaluate(&p, &[trial("a", f64::NAN)]).is_err());
}
