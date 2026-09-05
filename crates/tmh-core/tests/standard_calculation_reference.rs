use tmh_core::gb::{free_swelling, rdi, reducibility, reduction_rate_index};
#[test]
fn chemical_correction_and_total_weighing_baseline() {
    let r = reducibility(500.0, 1500.0, 1450.0, 10.0, 60.0).unwrap();
    assert!((r.rt - 43.06201550387597).abs() < 1e-10);
    assert_eq!(r.delta_w, 50.0);
}
#[test]
fn standard_rate_factor_and_low_reduction_fallbacks() {
    assert!((reduction_rate_index(&[(0.0, 30.0), (10.0, 60.0)]).unwrap() - 3.36).abs() < 1e-12);
    assert!((reduction_rate_index(&[(0.0, 30.0), (10.0, 55.0)]).unwrap() - 2.65).abs() < 1e-12);
    assert!((reduction_rate_index(&[(0.0, 30.0), (10.0, 50.0)]).unwrap() - 2.0).abs() < 1e-12);
    assert!(reduction_rate_index(&[(0.0, 31.0), (10.0, 60.0)]).is_none());
}
#[test]
fn fines_by_difference_include_unrecovered_powder() {
    let r = rdi(450.0, 300.0, 100.0, 40.0).unwrap();
    assert!((r.rdi_minus_0_5 - 100.0 / 45.0).abs() < 1e-12);
}
#[test]
fn swelling_is_whole_portion_volume_change() {
    let mut volumes = vec![(1.0, 2.0); 9];
    volumes.extend(vec![(2.0, 2.0); 9]);
    assert!((free_swelling(&volumes).unwrap().swelling_index - 100.0 / 3.0).abs() < 1e-12);
}
#[test]
fn invalid_physical_inputs_are_errors_not_omitted_or_zero() {
    assert!(rdi(450.0, -1.0, 100.0, 40.0).is_none());
    assert!(rdi(450.0, 400.0, 100.0, 40.0).is_none());
    assert!(free_swelling(&[(0.0, 1.0), (1.0, 2.0)]).is_none());
    assert!(free_swelling(&[(1.0, f64::NAN)]).is_none());
    assert!(reducibility(500.0, 1500.0, 1450.0, 10.0, f64::INFINITY).is_none());
}
