//! Independent reference cases for GB repeat-test decisions.
//! Sources: docs/13 / outputs/REQUIREMENTS.md section 4 and the verified a-d
//! decision branches. Threshold lookup for GB/T 13242 is intentionally external.
//!
//! Contract: input order is trial chronology; indices identify original trials.
//! Pending decisions have no selection/result and report the next required total
//! count. Empty/one-trial groups require two trials. Surplus trials after an
//! earlier terminal decision are rejected instead of silently ignored. Results
//! are unrounded; reporting/GB/T 8170 rounding is a separate operation.

use tmh_core::gb::{repeat_decision, sequential_repeat_decision, RepeatDecision};

type Decide = fn(&[f64], f64, f64, f64) -> Result<RepeatDecision, String>;

struct Case {
    name: &'static str,
    values: &'static [f64],
    required_count: usize,
    selected: &'static [usize],
    result: Option<f64>,
}

fn verify_table(decide: Decide, limits: [f64; 3], cases: &[Case]) {
    for case in cases {
        let decision = decide(case.values, limits[0], limits[1], limits[2])
            .unwrap_or_else(|error| panic!("{}: unexpected error: {error}", case.name));
        assert_eq!(
            decision.required_count, case.required_count,
            "{}: required total",
            case.name
        );
        // Result ordering is not part of the API; original sample identity is.
        let mut actual = decision.selected_indices.clone();
        actual.sort_unstable();
        let mut expected = case.selected.to_vec();
        expected.sort_unstable();
        assert_eq!(actual, expected, "{}: original trial indices", case.name);
        match (decision.result, case.result) {
            (None, None) => {}
            (Some(actual), Some(expected)) => assert!(
                actual.is_finite() && (actual - expected).abs() < 1e-10,
                "{}: expected unrounded mean {expected}, got {actual}",
                case.name
            ),
            (actual, expected) => panic!(
                "{}: expected result {expected:?}, got {actual:?}",
                case.name
            ),
        }
    }
}

#[test]
fn branch_based_repeat_decision_matches_verified_reference_cases() {
    // Integer thresholds make exact A/B/C equality independent of cancellation.
    verify_table(
        repeat_decision,
        [3.0, 4.0, 5.0],
        &[
            Case {
                name: "empty group",
                values: &[],
                required_count: 2,
                selected: &[],
                result: None,
            },
            Case {
                name: "one trial",
                values: &[10.0],
                required_count: 2,
                selected: &[],
                result: None,
            },
            Case {
                name: "difference below A",
                values: &[10.0, 12.0],
                required_count: 2,
                selected: &[0, 1],
                result: Some(11.0),
            },
            Case {
                name: "difference exactly A",
                values: &[10.0, 13.0],
                required_count: 2,
                selected: &[0, 1],
                result: Some(11.5),
            },
            Case {
                name: "just above A needs third",
                values: &[0.0, 3.000001],
                required_count: 3,
                selected: &[],
                result: None,
            },
            Case {
                name: "difference exactly B needs third",
                values: &[0.0, 4.0],
                required_count: 3,
                selected: &[],
                result: None,
            },
            Case {
                name: "just above B goes directly to fourth",
                values: &[0.0, 4.000001],
                required_count: 4,
                selected: &[],
                result: None,
            },
            Case {
                name: "difference exactly C needs fourth",
                values: &[0.0, 5.0],
                required_count: 4,
                selected: &[],
                result: None,
            },
            Case {
                name: "just above C needs fourth",
                values: &[0.0, 5.000001],
                required_count: 4,
                selected: &[],
                result: None,
            },
            Case {
                name: "third remains within B",
                values: &[0.0, 3.5, 2.0],
                required_count: 3,
                selected: &[0, 1, 2],
                result: Some(5.5 / 3.0),
            },
            Case {
                name: "third range exactly B",
                values: &[0.0, 3.5, 4.0],
                required_count: 3,
                selected: &[0, 1, 2],
                result: Some(2.5),
            },
            Case {
                name: "third range just above B needs fourth",
                values: &[0.0, 3.5, 4.000001],
                required_count: 4,
                selected: &[],
                result: None,
            },
            Case {
                name: "fourth after failed third range exactly C",
                values: &[0.0, 3.5, 4.5, 5.0],
                required_count: 4,
                selected: &[0, 1, 2, 3],
                result: Some(3.25),
            },
            Case {
                name: "fourth after failed third range above C selects middle",
                values: &[0.0, 3.5, 4.5, 5.000001],
                required_count: 4,
                selected: &[1, 2],
                result: Some(4.0),
            },
            Case {
                name: "direct-four branch remains pending at three",
                values: &[0.0, 4.5, 2.0],
                required_count: 4,
                selected: &[],
                result: None,
            },
            Case {
                name: "direct-four branch range below C",
                values: &[0.0, 4.5, 2.0, 3.0],
                required_count: 4,
                selected: &[0, 1, 2, 3],
                result: Some(2.375),
            },
            Case {
                name: "direct-four branch range exactly C",
                values: &[0.0, 4.5, 2.0, 5.0],
                required_count: 4,
                selected: &[0, 1, 2, 3],
                result: Some(2.875),
            },
            Case {
                name: "initial difference exactly C still permits all-four mean",
                values: &[0.0, 5.0, 2.0, 4.0],
                required_count: 4,
                selected: &[0, 1, 2, 3],
                result: Some(2.75),
            },
            Case {
                name: "direct-four branch range above C preserves unsorted indices",
                values: &[0.0, 4.5, 6.0, 2.0],
                required_count: 4,
                selected: &[1, 3],
                result: Some(3.25),
            },
            Case {
                name: "initial difference above C remains pending at three",
                values: &[20.0, 10.0, 19.0],
                required_count: 4,
                selected: &[],
                result: None,
            },
            Case {
                name: "initial difference above C selects middle of four",
                values: &[20.0, 10.0, 19.0, 12.0],
                required_count: 4,
                selected: &[2, 3],
                result: Some(15.5),
            },
            Case {
                name: "finite negative measurements are not domain-clamped",
                values: &[-10.0, -8.0],
                required_count: 2,
                selected: &[0, 1],
                result: Some(-9.0),
            },
        ],
    );
}

#[test]
fn sinter_and_pellet_thresholds_are_explicit_inputs() {
    verify_table(
        repeat_decision,
        [5.0, 6.0, 6.6],
        &[
            Case {
                name: "sinter exactly A",
                values: &[0.0, 5.0],
                required_count: 2,
                selected: &[0, 1],
                result: Some(2.5),
            },
            Case {
                name: "sinter exactly B",
                values: &[0.0, 6.0],
                required_count: 3,
                selected: &[],
                result: None,
            },
            Case {
                name: "sinter fourth range exactly C",
                values: &[0.0, 6.2, 4.0, 6.6],
                required_count: 4,
                selected: &[0, 1, 2, 3],
                result: Some(4.2),
            },
        ],
    );
    verify_table(
        repeat_decision,
        [3.0, 3.6, 3.9],
        &[
            Case {
                name: "pellet exactly A",
                values: &[0.0, 3.0],
                required_count: 2,
                selected: &[0, 1],
                result: Some(1.5),
            },
            Case {
                name: "pellet exactly B",
                values: &[0.0, 3.6],
                required_count: 3,
                selected: &[],
                result: None,
            },
            Case {
                name: "pellet fourth range exactly C",
                values: &[0.0, 3.7, 2.0, 3.9],
                required_count: 4,
                selected: &[0, 1, 2, 3],
                result: Some(2.4),
            },
        ],
    );
}

#[test]
fn free_swelling_uses_sequential_two_three_four_rule() {
    verify_table(
        sequential_repeat_decision,
        [3.0, 3.6, 3.9],
        &[
            Case {
                name: "empty group",
                values: &[],
                required_count: 2,
                selected: &[],
                result: None,
            },
            Case {
                name: "one trial",
                values: &[10.0],
                required_count: 2,
                selected: &[],
                result: None,
            },
            Case {
                name: "two difference below r",
                values: &[0.0, 2.0],
                required_count: 2,
                selected: &[0, 1],
                result: Some(1.0),
            },
            Case {
                name: "two difference exactly r",
                values: &[0.0, 3.0],
                required_count: 2,
                selected: &[0, 1],
                result: Some(1.5),
            },
            Case {
                name: "just above r needs third",
                values: &[0.0, 3.000001],
                required_count: 3,
                selected: &[],
                result: None,
            },
            Case {
                name: "difference above C still needs third first",
                values: &[0.0, 6.0],
                required_count: 3,
                selected: &[],
                result: None,
            },
            Case {
                name: "third range below 1.2r",
                values: &[0.0, 3.5, 2.0],
                required_count: 3,
                selected: &[0, 1, 2],
                result: Some(5.5 / 3.0),
            },
            Case {
                name: "third range exactly 1.2r",
                values: &[0.0, 3.5, 3.6],
                required_count: 3,
                selected: &[0, 1, 2],
                result: Some(7.1 / 3.0),
            },
            Case {
                name: "third range above 1.2r needs fourth",
                values: &[0.0, 3.5, 3.600001],
                required_count: 4,
                selected: &[],
                result: None,
            },
            Case {
                name: "fourth range below 1.3r",
                values: &[0.0, 3.5, 3.8, 2.0],
                required_count: 4,
                selected: &[0, 1, 2, 3],
                result: Some(2.325),
            },
            Case {
                name: "fourth range exactly 1.3r",
                values: &[0.0, 3.5, 3.8, 3.9],
                required_count: 4,
                selected: &[0, 1, 2, 3],
                result: Some(2.8),
            },
            Case {
                name: "fourth range above 1.3r takes median",
                values: &[0.0, 3.5, 3.8, 3.900001],
                required_count: 4,
                selected: &[1, 2],
                result: Some(3.65),
            },
            Case {
                name: "median preserves original out-of-order identities",
                values: &[20.0, 10.0, 19.0, 12.0],
                required_count: 4,
                selected: &[2, 3],
                result: Some(15.5),
            },
        ],
    );
}

#[test]
fn extra_trials_after_an_already_terminal_branch_are_rejected() {
    for decide in [
        repeat_decision as Decide,
        sequential_repeat_decision as Decide,
    ] {
        for values in [
            &[0.0, 2.0, 99.0][..],
            &[0.0, 2.0, 99.0, 100.0],
            &[0.0, 3.5, 2.0, 99.0],
        ] {
            assert!(
                decide(values, 3.0, 3.6, 3.9).is_err(),
                "surplus data must not be silently ignored: {values:?}"
            );
        }
    }
}

#[test]
fn no_group_may_contain_more_than_four_trials() {
    for decide in [
        repeat_decision as Decide,
        sequential_repeat_decision as Decide,
    ] {
        assert!(decide(&[0.0, 6.0, 3.0, 4.0, 5.0], 3.0, 3.6, 3.9).is_err());
    }
}

#[test]
fn nonfinite_measurements_are_rejected_in_every_input_position() {
    for decide in [
        repeat_decision as Decide,
        sequential_repeat_decision as Decide,
    ] {
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for position in 0..4 {
                let mut values = [0.0, 6.0, 4.0, 2.0];
                values[position] = invalid;
                assert!(
                    decide(&values, 3.0, 3.6, 3.9).is_err(),
                    "nonfinite position {position}: {values:?}"
                );
            }
        }
    }
}

#[test]
fn invalid_thresholds_are_rejected_even_before_two_trials_exist() {
    let invalid_limits = [
        [-1.0, 3.6, 3.9],
        [3.0, -1.0, 3.9],
        [3.0, 3.6, -1.0],
        [4.0, 3.6, 3.9],
        [3.0, 4.0, 3.9],
        [f64::NAN, 3.6, 3.9],
        [3.0, f64::NAN, 3.9],
        [3.0, 3.6, f64::NAN],
        [f64::INFINITY, 3.6, 3.9],
        [3.0, f64::INFINITY, 3.9],
        [3.0, 3.6, f64::INFINITY],
        [f64::NEG_INFINITY, 3.6, 3.9],
        [3.0, f64::NEG_INFINITY, 3.9],
        [3.0, 3.6, f64::NEG_INFINITY],
    ];
    for decide in [
        repeat_decision as Decide,
        sequential_repeat_decision as Decide,
    ] {
        for limits in invalid_limits {
            for values in [&[][..], &[1.0], &[0.0, 6.0, 4.0, 2.0]] {
                assert!(
                    decide(values, limits[0], limits[1], limits[2]).is_err(),
                    "invalid limits {limits:?}, values {values:?}"
                );
            }
        }
    }
}

#[test]
fn zero_and_equal_thresholds_are_valid() {
    for decide in [
        repeat_decision as Decide,
        sequential_repeat_decision as Decide,
    ] {
        verify_table(
            decide,
            [0.0, 0.0, 0.0],
            &[Case {
                name: "identical values with zero thresholds",
                values: &[4.0, 4.0],
                required_count: 2,
                selected: &[0, 1],
                result: Some(4.0),
            }],
        );
        verify_table(
            decide,
            [3.0, 3.0, 3.0],
            &[Case {
                name: "equal thresholds median branch",
                values: &[0.0, 6.0, 4.0, 2.0],
                required_count: 4,
                selected: &[2, 3],
                result: Some(3.0),
            }],
        );
    }
}

#[test]
fn tied_extremes_select_two_distinct_original_trials_with_correct_median() {
    for decide in [
        repeat_decision as Decide,
        sequential_repeat_decision as Decide,
    ] {
        let values = [0.0, 6.0, 0.0, 6.0];
        let decision = decide(&values, 3.0, 3.6, 3.9).unwrap();
        assert_eq!(decision.required_count, 4);
        assert_eq!(decision.result, Some(3.0));
        assert_eq!(decision.selected_indices.len(), 2);
        let [first, second] = [decision.selected_indices[0], decision.selected_indices[1]];
        assert!(first < values.len() && second < values.len());
        assert_ne!(first, second);
        assert_eq!((values[first] + values[second]) / 2.0, 3.0);
        // Equal-valued trial identity ties may be resolved in either order.
    }
}
