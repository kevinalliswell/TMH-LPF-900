//! Independent integration-test fragment for the planned public rounding API.
//! Rounding rules: GB/T 8170 §§3.2–3.3, as specified for this task.
//! Repeat branches/limits: docs/13 §4 and the user-supplied standard PDFs.
//! Exact arithmetic on each f64's shortest decimal representation, fixed-scale
//! strings and the supported input limits are software contract choices.

use tmh_core::gb::{repeat_decision, sequential_repeat_decision, RepeatDecision};
use tmh_core::rounding::rounded_mean;

#[test]
fn single_values_follow_half_even_including_sign_and_carry() {
    for (value, places, expected) in [
        (1.25, 1, "1.2"),
        (1.35, 1, "1.4"),
        (1.25001, 1, "1.3"),
        (1.24999, 1, "1.2"),
        (-1.25, 1, "-1.2"),
        (-1.35, 1, "-1.4"),
        (-1.25001, 1, "-1.3"),
        (-1.24999, 1, "-1.2"),
        (9.95, 1, "10.0"),
        (-9.95, 1, "-10.0"),
        (2.5, 0, "2"),
        (3.5, 0, "4"),
        (-2.5, 0, "-2"),
        (2.675, 2, "2.68"),
        (1.2, 2, "1.20"),
    ] {
        assert_eq!(
            rounded_mean(&[value], places).unwrap(),
            expected,
            "value={value}, places={places}"
        );
    }
}

#[test]
fn averaging_is_exact_until_one_final_rounding_step() {
    let cases: &[(&[f64], u32, &str)] = &[
        (&[1.0, 1.0, 1.75], 1, "1.2"), // exact mean 1.25
        (&[1.0, 1.0, 2.0], 6, "1.333333"),
        (&[1.0, 1.0, 3.0], 6, "1.666667"),
        (&[1.24, 1.24, 1.29], 1, "1.3"), // rounding inputs first gives 1.2
        (&[1.25, 1.25, 1.2500000000000002], 1, "1.3"),
        (&[-1.25, -1.25, -1.2500000000000002], 1, "-1.3"),
        (&[1.35, 1.35, 1.3499999999999999], 1, "1.3"),
        (&[1e20, 1.0, -1e20], 6, "0.333333"),
        (&[-1e20, 1e20, 1.0], 6, "0.333333"),
        (&[1.0, 2.0, 3.0, 4.0], 0, "2"),
    ];
    for &(values, places, expected) in cases {
        assert_eq!(
            rounded_mean(values, places).unwrap(),
            expected,
            "values={values:?}, places={places}"
        );
    }
}

#[test]
fn extreme_finite_inputs_do_not_overflow_or_erase_a_nonzero_decimal_tail() {
    // f64::MAX has shortest decimal representation 1.7976931348623157e308.
    let maximum_decimal = format!("17976931348623157{}", "0".repeat(292));
    assert_eq!(rounded_mean(&[f64::MAX; 4], 0).unwrap(), maximum_decimal);
    assert_eq!(
        rounded_mean(&[-f64::MAX; 4], 0).unwrap(),
        format!("-{maximum_decimal}")
    );
    assert_eq!(
        rounded_mean(&[f64::MAX, f64::MAX, -f64::MAX, -f64::MAX], 6).unwrap(),
        "0.000000"
    );
    assert_eq!(
        rounded_mean(&[f64::MAX, 1.0, -f64::MAX], 6).unwrap(),
        "0.333333"
    );
    let smallest = f64::from_bits(1);
    assert_eq!(rounded_mean(&[smallest], 6).unwrap(), "0.000000");
    assert_eq!(rounded_mean(&[f64::MIN_POSITIVE], 6).unwrap(), "0.000000");
    assert_eq!(rounded_mean(&[1e-6, 0.0], 6).unwrap(), "0.000000");
    assert_eq!(rounded_mean(&[1e-6, smallest], 6).unwrap(), "0.000001");
    assert_eq!(rounded_mean(&[-1e-6, -smallest], 6).unwrap(), "-0.000001");
}

#[test]
fn rounded_zero_never_carries_a_minus_sign_and_all_supported_scales_work() {
    for places in 0..=6 {
        let zero = if places == 0 {
            "0".to_string()
        } else {
            format!("0.{}", "0".repeat(places as usize))
        };
        for values in [&[-0.0][..], &[-1e-7][..], &[1.25, -1.25][..]] {
            assert_eq!(rounded_mean(values, places).unwrap(), zero);
        }
    }
    assert_eq!(rounded_mean(&[-0.0000005], 6).unwrap(), "0.000000");
}

#[test]
fn rounding_rejects_invalid_counts_nonfinite_values_and_unsupported_scales() {
    assert!(rounded_mean(&[], 1).is_err());
    assert!(rounded_mean(&[1.0; 5], 1).is_err());
    for places in [7, u32::MAX] {
        assert!(rounded_mean(&[1.0], places).is_err());
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for index in 0..4 {
            let mut values = [1.0; 4];
            values[index] = invalid;
            assert!(rounded_mean(&values, 1).is_err());
        }
    }
}

type DecimalDecide = fn(&[f64], f64, f64, f64) -> Result<RepeatDecision, String>;

fn assert_repeat_branch(
    decide: DecimalDecide,
    values: &[f64],
    required_count: usize,
    selected: &[usize],
) {
    let result = decide(values, 3.0, 3.6, 3.9).unwrap();
    assert_eq!(result.required_count, required_count, "values={values:?}");
    let mut indices = result.selected_indices;
    indices.sort_unstable();
    assert_eq!(indices, selected, "values={values:?}");
    assert_eq!(result.result.is_some(), !selected.is_empty());
}

#[test]
fn decimal_repeatability_equalities_are_inclusive_at_nonzero_baselines() {
    // Branch-based 13241 may go directly from two trials to four; 13240
    // sequentially requests a third. Both must accept decimal equalities.
    assert_repeat_branch(repeat_decision, &[85.0, 88.6], 3, &[]);
    assert_repeat_branch(repeat_decision, &[85.1, 88.7], 3, &[]);
    for decide in [repeat_decision as DecimalDecide, sequential_repeat_decision] {
        assert_repeat_branch(decide, &[85.0, 88.0], 2, &[0, 1]);
        assert_repeat_branch(decide, &[85.1, 88.5, 88.7], 3, &[0, 1, 2]);
        assert_repeat_branch(decide, &[-88.7, -85.1, -87.0], 3, &[0, 1, 2]);
        assert_repeat_branch(decide, &[85.0, 88.7, 87.0, 88.9], 4, &[0, 1, 2, 3]);
        assert_repeat_branch(decide, &[85.0, 88.9, 86.0, 87.0], 4, &[0, 1, 2, 3]);
    }
}

#[test]
fn genuine_one_ulp_excess_is_never_accepted_by_an_epsilon_boundary() {
    // Next representable inputs also have shortest decimal values strictly
    // above the limits. They must not be treated as equality by a tolerance.
    let above_a = f64::from_bits(88.0_f64.to_bits() + 1);
    let above_b = f64::from_bits(88.7_f64.to_bits() + 1);
    let above_c = f64::from_bits(88.9_f64.to_bits() + 1);
    assert_repeat_branch(repeat_decision, &[85.1, above_b], 4, &[]);
    for decide in [repeat_decision as DecimalDecide, sequential_repeat_decision] {
        assert_repeat_branch(decide, &[85.0, above_a], 3, &[]);
        assert_repeat_branch(decide, &[85.1, 88.5, above_b], 4, &[]);
        assert_repeat_branch(decide, &[85.0, 88.7, 87.0, above_c], 4, &[1, 2]);
    }
}
