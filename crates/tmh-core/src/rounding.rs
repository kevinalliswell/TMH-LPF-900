//! GB/T 8170 half-even reporting, with one rounding after exact decimal averaging.
//! Inputs are the shortest round-trip decimal representation of stored finite f64s.
//! This avoids adding binary cancellation error; it does not remove measurement error.
use num_bigint::{BigInt, BigUint, Sign};

pub const ROUNDING_VERSION: &str = "GB8170-2008-decimal-v1";

fn decimal(value: f64) -> (BigInt, u32) {
    let text = value.to_string();
    let (mantissa, exponent) = text
        .split_once(['e', 'E'])
        .map(|(m, e)| (m, e.parse::<i32>().expect("f64 exponent")))
        .unwrap_or((&text, 0));
    let fraction = mantissa.split_once('.').map_or(0, |(_, f)| f.len()) as i32;
    let mut integer = BigInt::parse_bytes(mantissa.replace('.', "").as_bytes(), 10)
        .expect("finite f64 decimal digits");
    let scale = fraction - exponent;
    if scale < 0 {
        integer *= BigInt::from(10u8).pow((-scale) as u32);
    }
    (integer, scale.max(0) as u32)
}

fn decimal_sum(values: &[f64]) -> (BigInt, u32) {
    let parts: Vec<_> = values.iter().map(|v| decimal(*v)).collect();
    let scale = parts.iter().map(|(_, s)| *s).max().unwrap_or(0);
    let sum = parts
        .into_iter()
        .map(|(v, s)| v * BigInt::from(10u8).pow(scale - s))
        .sum();
    (sum, scale)
}

/// Caller has already validated finite observations and a nonnegative finite limit.
/// Compare the exact decimal range, including equality, without an epsilon window.
pub(crate) fn range_within(values: &[f64], limit: f64) -> bool {
    let low = values.iter().copied().fold(f64::INFINITY, f64::min);
    let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let (difference, _) = decimal_sum(&[high, -low, -limit]);
    difference.sign() != Sign::Plus
}

/// A fixed-scale string, retaining trailing zeros. Rounds only once, after averaging.
/// At most four trials and six decimal places keep this scientific-report API bounded.
pub fn rounded_mean(values: &[f64], places: u32) -> Result<String, String> {
    if values.is_empty() || values.len() > 4 || places > 6 || values.iter().any(|v| !v.is_finite())
    {
        return Err("修约需要 1—4 个有限数值和 0—6 位小数".into());
    }
    let (sum, scale) = decimal_sum(values);
    let numerator = sum.magnitude() * BigUint::from(10u8).pow(places);
    let denominator = BigUint::from(values.len()) * BigUint::from(10u8).pow(scale);
    let mut quotient = &numerator / &denominator;
    let twice_remainder = (&numerator % &denominator) * 2u8;
    if twice_remainder > denominator
        || (twice_remainder == denominator && &quotient % 2u8 == BigUint::from(1u8))
    {
        quotient += 1u8;
    }
    let negative = sum.sign() == Sign::Minus && quotient != BigUint::from(0u8);
    let mut text = quotient.to_string();
    if places > 0 {
        let places = places as usize;
        if text.len() <= places {
            text = format!("{}{}", "0".repeat(places + 1 - text.len()), text);
        }
        text.insert(text.len() - places, '.');
    }
    Ok(if negative { format!("-{text}") } else { text })
}
