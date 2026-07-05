//! GB national-standard index calculations (docs/05 §5.6). Pure functions with
//! golden-value tests. Results serialize into `experiments.analysis_results_json`.
//! ⚠ The old system used the O2 percentage as a fraction (100× error) — fixed here.

use serde::Serialize;

/// GB/T 13241 reducibility result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reducibility {
    /// Reduction degree Rt (%).
    pub rt: f64,
    /// Mass loss ΔW = W0 − Wt (g).
    pub delta_w: f64,
}

/// GB/T 13241: `Rt = ΔW / (W0 × O2%/100) × 100`. `o2_pct` is a percentage
/// (e.g. 28.5), NOT a fraction. `None` for non-positive W0 or O2%.
pub fn reducibility(w0: f64, wt: f64, o2_pct: f64) -> Option<Reducibility> {
    if w0 <= 0.0 || o2_pct <= 0.0 {
        return None;
    }
    let delta_w = w0 - wt;
    Some(Reducibility {
        rt: delta_w / (w0 * (o2_pct / 100.0)) * 100.0,
        delta_w,
    })
}

/// Reduction-rate index: slope of a least-squares fit over `(t_min, rt)` points
/// (%/min). `None` for fewer than 2 points or a degenerate fit.
pub fn reduction_rate_index(points: &[(f64, f64)]) -> Option<f64> {
    if points.len() < 2 {
        return None;
    }
    let n = points.len() as f64;
    let sx: f64 = points.iter().map(|p| p.0).sum();
    let sy: f64 = points.iter().map(|p| p.1).sum();
    let sxx: f64 = points.iter().map(|p| p.0 * p.0).sum();
    let sxy: f64 = points.iter().map(|p| p.0 * p.1).sum();
    let denom = n * sxx - sx * sx;
    if denom == 0.0 {
        return None;
    }
    Some((n * sxy - sx * sy) / denom)
}

/// GB/T 13242 low-temperature degradation RDI result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rdi {
    pub rdi_plus_6_3: f64,
    pub rdi_plus_3_15: f64,
    pub rdi_minus_0_5: f64,
    /// True if the sieved total differs from m0 by more than 2%.
    pub sieve_warning: bool,
}

/// GB/T 13242. Sieve fractions (g): `>6.3`, `3.15–6.3`, `0.5–3.15`, `<0.5`; `m0`
/// initial mass. RDI+3.15 is cumulative (`>6.3` + `3.15–6.3`).
pub fn rdi(m0: f64, m_gt63: f64, m_315_63: f64, m_05_315: f64, m_lt05: f64) -> Option<Rdi> {
    if m0 <= 0.0 {
        return None;
    }
    let total = m_gt63 + m_315_63 + m_05_315 + m_lt05;
    Some(Rdi {
        rdi_plus_6_3: m_gt63 / m0 * 100.0,
        rdi_plus_3_15: (m_gt63 + m_315_63) / m0 * 100.0,
        rdi_minus_0_5: m_lt05 / m0 * 100.0,
        sieve_warning: ((total - m0).abs() / m0) > 0.02,
    })
}

/// GB/T 13240 free-swelling index result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FreeSwelling {
    /// Per-pellet SI (%).
    pub per_pellet: Vec<f64>,
    pub average: f64,
}

/// GB/T 13240: per pellet `SI = (V1 − V0) / V0 × 100` (mL); average over pellets.
/// Pellets with non-positive V0 are skipped. `None` if no valid pellet.
pub fn free_swelling(volumes: &[(f64, f64)]) -> Option<FreeSwelling> {
    let per: Vec<f64> = volumes
        .iter()
        .filter(|(v0, _)| *v0 > 0.0)
        .map(|(v0, v1)| (v1 - v0) / v0 * 100.0)
        .collect();
    if per.is_empty() {
        return None;
    }
    let average = per.iter().sum::<f64>() / per.len() as f64;
    Some(FreeSwelling {
        per_pellet: per,
        average,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gb13241_golden_value() {
        // docs/05 §5.6: W0=100, Wt=85, O2=30% -> Rt = 50.0% (NOT 0.5 or 5000).
        let r = reducibility(100.0, 85.0, 30.0).unwrap();
        assert!((r.rt - 50.0).abs() < 1e-9);
        assert!((r.delta_w - 15.0).abs() < 1e-9);
    }

    #[test]
    fn gb13241_default_o2_and_rejects() {
        // O2 default 28.5 gives a distinct, finite result.
        let r = reducibility(100.0, 85.0, 28.5).unwrap();
        assert!((r.rt - (15.0 / 28.5 * 100.0)).abs() < 1e-9);
        assert!(reducibility(0.0, 0.0, 30.0).is_none());
        assert!(reducibility(100.0, 85.0, 0.0).is_none());
    }

    #[test]
    fn reduction_rate_index_slope() {
        let idx = reduction_rate_index(&[(0.0, 0.0), (1.0, 10.0), (2.0, 20.0)]).unwrap();
        assert!((idx - 10.0).abs() < 1e-9);
        assert!(reduction_rate_index(&[(0.0, 0.0)]).is_none());
    }

    #[test]
    fn gb13242_rdi_cumulative_and_warning() {
        // total == m0 -> no warning
        let r = rdi(500.0, 300.0, 100.0, 60.0, 40.0).unwrap();
        assert!((r.rdi_plus_6_3 - 60.0).abs() < 1e-9);
        assert!((r.rdi_plus_3_15 - 80.0).abs() < 1e-9); // (300+100)/500*100
        assert!((r.rdi_minus_0_5 - 8.0).abs() < 1e-9);
        assert!(!r.sieve_warning);
        // sieved total 5% short -> warning
        let w = rdi(500.0, 300.0, 100.0, 50.0, 25.0).unwrap();
        assert!(w.sieve_warning);
        assert!(rdi(0.0, 1.0, 1.0, 1.0, 1.0).is_none());
    }

    #[test]
    fn gb13240_free_swelling_average() {
        let r = free_swelling(&[(10.0, 12.0), (10.0, 11.0)]).unwrap();
        assert_eq!(r.per_pellet, vec![20.0, 10.0]);
        assert!((r.average - 15.0).abs() < 1e-9);
        assert!(free_swelling(&[]).is_none());
    }
}
