//! Gas setpoint safety clamp — the ONE choke point (docs/03 §safety, docs/04 §4.2,
//! docs/05 §5.5). Every setpoint (manual, experiment program, custom, future) must
//! pass through [`clamp_setpoint`] before an MFC frame is built. Config limits can
//! only make it MORE restrictive; a compile-time absolute ceiling backs the config
//! so a corrupted file can never raise the limit. Zero always passes (emergency
//! shutoff must never be blocked).

/// Compile-time absolute ceiling for H2 (L/min). Config cannot exceed this.
pub const H2_ABS_MAX: f64 = 10.0;
/// Compile-time absolute ceiling for CO (L/min). Config cannot exceed this.
pub const CO_ABS_MAX: f64 = 10.0;

/// The four gas channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gas {
    H2,
    N2,
    Co2,
    Co,
}

/// Result of clamping a setpoint (in L/min).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClampResult {
    /// Value that will actually be sent.
    pub applied: f64,
    /// True if the request was reduced.
    pub clamped: bool,
    /// The effective limit (config ∧ absolute ceiling); `INFINITY` for unlimited gases.
    pub limit: f64,
}

/// Clamp a requested setpoint for `gas`. `cfg_h2` / `cfg_co` are the configured
/// `GAS_SAFETY_LIMITS`. Only H2 and CO are limited; N2/CO2 pass through. The
/// effective limit is `min(config, absolute_ceiling)`.
pub fn clamp_setpoint(gas: Gas, requested: f64, cfg_h2: f64, cfg_co: f64) -> ClampResult {
    let limit = match gas {
        Gas::H2 => cfg_h2.min(H2_ABS_MAX),
        Gas::Co => cfg_co.min(CO_ABS_MAX),
        Gas::N2 | Gas::Co2 => {
            return ClampResult {
                applied: requested,
                clamped: false,
                limit: f64::INFINITY,
            };
        }
    };
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
    fn h2_over_config_limit_is_clamped() {
        // docs/10 M2: request H2=8.0 with limit 5.0 -> applied 5.0, clamped.
        let r = clamp_setpoint(Gas::H2, 8.0, 5.0, 5.0);
        assert_eq!(r.applied, 5.0);
        assert!(r.clamped);
        assert_eq!(r.limit, 5.0);
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
        assert_eq!(r.applied, H2_ABS_MAX); // 10.0
        assert!(r.clamped);
        // a request under the ceiling still passes even with the corrupt config
        let r2 = clamp_setpoint(Gas::H2, 8.0, 999.0, 999.0);
        assert_eq!(r2.applied, 8.0);
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
}
