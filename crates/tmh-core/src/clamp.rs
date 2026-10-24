//! Gas setpoint safety clamp — the ONE choke point (docs/03 §safety, docs/04 §4.2,
//! docs/05 §5.5). Every setpoint (manual, experiment program, custom, future) must
//! pass through [`clamp_setpoint`] before an MFC frame is built. Config limits can
//! only make it MORE restrictive; a compile-time absolute ceiling backs the config
//! so a corrupted file can never raise the limit. Zero always passes (emergency
//! shutoff must never be blocked).

/// Compile-time absolute ceiling for H2 (L/min). Config cannot exceed this.
pub const H2_ABS_MAX: f64 = 5.0;
/// Compile-time absolute ceiling for CO (L/min). Config cannot exceed this.
pub const CO_ABS_MAX: f64 = 5.0;

/// The four gas channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gas {
    H2,
    N2,
    Co2,
    Co,
}

/// Result of clamping a setpoint (in L/min).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct ClampResult {
    /// Value that will actually be sent.
    pub applied: f64,
    /// True if the request was reduced.
    pub clamped: bool,
    /// The effective limit (config ∧ absolute ceiling); `INFINITY` for unlimited gases.
    pub limit: f64,
}

fn effective_limit(configured: f64, absolute: f64) -> f64 {
    if configured.is_finite() {
        configured.clamp(0.0, absolute)
    } else {
        0.0
    }
}

/// Clamp a requested setpoint for `gas`. `cfg_h2` / `cfg_co` are the configured
/// `GAS_SAFETY_LIMITS`. Only H2 and CO are limited; N2/CO2 pass through. The
/// effective limit is `min(config, absolute_ceiling)`.
pub fn clamp_setpoint(gas: Gas, requested: f64, cfg_h2: f64, cfg_co: f64) -> ClampResult {
    let limit = match gas {
        Gas::H2 => effective_limit(cfg_h2, H2_ABS_MAX),
        Gas::Co => effective_limit(cfg_co, CO_ABS_MAX),
        // N2 and CO2 are unlimited; INFINITY means `requested > limit` is never true.
        Gas::N2 | Gas::Co2 => f64::INFINITY,
    };
    // Non-finite (NaN/Inf) or negative requests are never valid setpoints; floor them to
    // 0.0 (which always passes) so a negative never reaches the wire and NaN never poisons
    // telemetry. Zero still passes for every gas — emergency shutoff must never be blocked.
    if !requested.is_finite() || requested < 0.0 {
        return ClampResult {
            applied: 0.0,
            clamped: false,
            limit,
        };
    }
    // requested <= limit (limit > 0) covers "zero always passes".
    if requested > limit {
        ClampResult {
            applied: limit,
            clamped: true,
            limit,
        }
    } else {
        ClampResult {
            applied: requested,
            clamped: false,
            limit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_flammable_limits_match_the_documented_five_litre_ceiling() {
        for gas in [Gas::H2, Gas::Co] {
            let result = clamp_setpoint(gas, 8.0, 999.0, 999.0);
            assert_eq!(result.applied, 5.0);
            assert!(result.clamped);
        }
    }

    #[test]
    fn h2_over_config_limit_is_clamped() {
        // docs/10 M2: request H2=8.0 with limit 5.0 -> applied 5.0, clamped.
        let r = clamp_setpoint(Gas::H2, 8.0, 5.0, 5.0);
        assert_eq!(r.applied, 5.0);
        assert!(r.clamped);
        assert_eq!(r.limit, 5.0);
    }

    #[test]
    fn zero_shutoff_survives_negative_corrupt_limits() {
        for gas in [Gas::H2, Gas::Co] {
            let result = clamp_setpoint(gas, 0.0, -1.0, f64::NEG_INFINITY);
            assert_eq!(result.applied, 0.0);
            assert!(!result.clamped);
        }
    }

    #[test]
    fn zero_always_passes() {
        let r = clamp_setpoint(Gas::H2, 0.0, 5.0, 5.0);
        assert_eq!(r.applied, 0.0);
        assert!(!r.clamped);
        // even with an absurd config, zero passes
        let r2 = clamp_setpoint(Gas::Co, 0.0, 0.001, 0.001);
        assert_eq!(r2.applied, 0.0);
        assert!(!r2.clamped);
    }

    #[test]
    fn corrupt_config_bounded_by_absolute_ceiling() {
        // config H2=999 must not raise the limit above the compile-time ceiling.
        let r = clamp_setpoint(Gas::H2, 12.0, 999.0, 999.0);
        assert_eq!(r.applied, H2_ABS_MAX); // 5.0
        assert!(r.clamped);
        // a request under the ceiling still passes even with the corrupt config
        let r2 = clamp_setpoint(Gas::H2, 4.5, 999.0, 999.0);
        assert_eq!(r2.applied, 4.5);
        assert!(!r2.clamped);
    }

    #[test]
    fn exactly_at_limit_not_clamped() {
        let r = clamp_setpoint(Gas::Co, 5.0, 5.0, 5.0);
        assert_eq!(r.applied, 5.0);
        assert!(!r.clamped);
    }

    #[test]
    fn n2_and_co2_pass_through() {
        assert!(!clamp_setpoint(Gas::N2, 15.0, 5.0, 5.0).clamped);
        assert!(!clamp_setpoint(Gas::Co2, 15.0, 5.0, 5.0).clamped);
    }

    #[test]
    fn negative_and_non_finite_floor_to_zero() {
        // Negatives must never reach the wire; NaN/Inf must never poison telemetry (#4).
        for gas in [Gas::H2, Gas::Co, Gas::N2, Gas::Co2] {
            assert_eq!(clamp_setpoint(gas, -3.0, 5.0, 5.0).applied, 0.0);
            assert_eq!(clamp_setpoint(gas, f64::NAN, 5.0, 5.0).applied, 0.0);
            assert_eq!(clamp_setpoint(gas, f64::INFINITY, 5.0, 5.0).applied, 0.0);
            assert_eq!(
                clamp_setpoint(gas, f64::NEG_INFINITY, 5.0, 5.0).applied,
                0.0
            );
        }
        // a reject is not a clamp (it's floored, not reduced to the limit)
        assert!(!clamp_setpoint(Gas::H2, -3.0, 5.0, 5.0).clamped);
    }
}
