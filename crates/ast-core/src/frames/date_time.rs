//! Frame-level date/time parsing helpers.
//!
//! Mirrors the decimal-year branch of `astReadDateTime` (frame.c:8892);
//! the rest of that function (B/J/MJD/JD/Gregorian formats) is not yet
//! ported because no caller needs string parsing here. Once a Rust
//! analog of `astReadDateTime` lands, the IAU 1984 rule below should
//! be folded into it.

use crate::pal;

/// Convert a decimal-year epoch (e.g. an `EQUINOX = 1950.0` FITS
/// value) to Modified Julian Date using the IAU 1984 convention.
///
/// Per `astReadDateTime` (frame.c:9031): values strictly less than
/// 1984.0 are interpreted as Besselian (`palEpb2d`); values >= 1984.0
/// are interpreted as Julian (`palEpj2d`).
pub(crate) fn year_to_mjd_iau1984(year: f64) -> f64 {
    if year < 1984.0 {
        pal::epb2d(year)
    } else {
        pal::epj2d(year)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn besselian_round_trip() {
        // FK4 default: B1950.0 -> MJD -> Besselian year should round-trip.
        let mjd = year_to_mjd_iau1984(1950.0);
        assert!((pal::epb(mjd) - 1950.0).abs() < 1.0e-9);
    }

    #[test]
    fn julian_round_trip() {
        // FK5 default: J2000.0 -> MJD -> Julian year should round-trip.
        let mjd = year_to_mjd_iau1984(2000.0);
        assert!((pal::epj(mjd) - 2000.0).abs() < 1.0e-9);
    }

    #[test]
    fn threshold_is_inclusive_for_julian() {
        // Exactly 1984.0 is treated as Julian (per the C >= test).
        assert_eq!(year_to_mjd_iau1984(1984.0), pal::epj2d(1984.0));
        // Just below 1984 is Besselian.
        assert_eq!(year_to_mjd_iau1984(1983.999), pal::epb2d(1983.999));
    }
}
