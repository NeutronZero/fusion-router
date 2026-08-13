use std::fmt;
use std::ops::{Add, AddAssign, Sub, SubAssign};
use serde::{Deserialize, Serialize};

/// Canonical monetary representation: NanoUSD ($10^-9 USD).
/// 1 NanoUSD = 0.000000001 USD.
/// 1 MicroUSD = 1,000 NanoUSD.
/// 1 USD = 1,000,000,000 NanoUSD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
pub struct NanoUSD(pub u64);

impl NanoUSD {
    pub const ZERO: NanoUSD = NanoUSD(0);
    pub const ONE_MICRO: NanoUSD = NanoUSD(1_000);
    pub const ONE_CENT: NanoUSD = NanoUSD(10_000_000);
    pub const ONE_DOLLAR: NanoUSD = NanoUSD(1_000_000_000);

    #[inline]
    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    #[inline]
    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    #[inline]
    pub fn from_micros(micros: u64) -> Option<Self> {
        micros.checked_mul(1_000).map(Self)
    }

    #[inline]
    pub fn as_micros(self) -> u64 {
        self.0 / 1_000
    }

    #[inline]
    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        self.0.checked_add(rhs.0).map(Self)
    }

    #[inline]
    pub fn saturating_add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }

    #[inline]
    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        self.0.checked_sub(rhs.0).map(Self)
    }

    #[inline]
    pub fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }

    #[inline]
    pub fn checked_mul(self, rhs: u64) -> Option<Self> {
        self.0.checked_mul(rhs).map(Self)
    }

    #[inline]
    pub fn saturating_mul(self, rhs: u64) -> Self {
        Self(self.0.saturating_mul(rhs))
    }

    /// Parse exact decimal USD string (e.g., "0.05", "12.345678901") into NanoUSD.
    /// Rejects strings with more than 9 decimal places or invalid format.
    pub fn checked_from_decimal_usd(usd_str: &str) -> Result<Self, String> {
        let s = usd_str.trim();
        if s.is_empty() {
            return Err("empty string".to_string());
        }
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() > 2 {
            return Err("multiple decimal points".to_string());
        }
        let dollars_str = parts[0];
        let dollars: u64 = if dollars_str.is_empty() {
            0
        } else {
            dollars_str.parse::<u64>().map_err(|e| format!("invalid integer part: {e}"))?
        };

        let mut nanos_from_frac: u64 = 0;
        if parts.len() == 2 {
            let frac_str = parts[1];
            if frac_str.len() > 9 {
                return Err(format!("decimal precision exceeds 9 places: '{frac_str}'"));
            }
            if !frac_str.chars().all(|c| c.is_ascii_digit()) {
                return Err("invalid characters in fractional part".to_string());
            }
            let padded_frac = format!("{:0<9}", frac_str);
            nanos_from_frac = padded_frac.parse::<u64>().map_err(|e| format!("invalid fractional part: {e}"))?;
        }

        let total_nanos = dollars
            .checked_mul(1_000_000_000)
            .and_then(|d_nanos| d_nanos.checked_add(nanos_from_frac))
            .ok_or_else(|| "overflow converting to NanoUSD".to_string())?;

        Ok(Self(total_nanos))
    }

    pub fn to_decimal_usd(self) -> String {
        let dollars = self.0 / 1_000_000_000;
        let nanos = self.0 % 1_000_000_000;
        if nanos == 0 {
            format!("{dollars}.00")
        } else {
            let s = format!("{:09}", nanos);
            let trimmed = s.trim_end_matches('0');
            let frac = if trimmed.len() < 2 {
                format!("{:0<2}", trimmed)
            } else {
                trimmed.to_string()
            };
            format!("{dollars}.{frac}")
        }
    }

    pub fn to_usd_f64(self) -> f64 {
        (self.0 as f64) / 1_000_000_000.0
    }
}

impl Add for NanoUSD {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        self.checked_add(rhs).expect("NanoUSD overflow in Add")
    }
}

impl AddAssign for NanoUSD {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for NanoUSD {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        self.checked_sub(rhs).expect("NanoUSD underflow in Sub")
    }
}

impl SubAssign for NanoUSD {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl fmt::Display for NanoUSD {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${}", self.to_decimal_usd())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nanousd_parsing() {
        assert_eq!(NanoUSD::checked_from_decimal_usd("0.05").unwrap(), NanoUSD(50_000_000));
        assert_eq!(NanoUSD::checked_from_decimal_usd("1.0").unwrap(), NanoUSD(1_000_000_000));
        assert_eq!(NanoUSD::checked_from_decimal_usd("0.000000001").unwrap(), NanoUSD(1));
        assert!(NanoUSD::checked_from_decimal_usd("0.0000000001").is_err());
    }

    #[test]
    fn test_nanousd_formatting() {
        assert_eq!(NanoUSD(50_000_000).to_decimal_usd(), "0.05");
        assert_eq!(NanoUSD(1_000_000_000).to_decimal_usd(), "1.00");
        assert_eq!(NanoUSD(12345).to_decimal_usd(), "0.000012345");
    }
}
