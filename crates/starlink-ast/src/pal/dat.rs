// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palDat (pal/palDat.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::PAL__MJD0;
use crate::erfa;

/// TAI minus UTC, in seconds, at a UTC date given as an MJD.
///
/// A date that ERFA cannot convert to a calendar date, or a calendar date it
/// has no leap-second value for, gives 0.0. PAL ignores both statuses and
/// returns ERFA's `deltat`, which `eraDat` sets to 0.0 before any check and
/// leaves there on every error, so those cases are defined. Where
/// `eraJd2cal` fails it writes none of the year, month, day and fraction,
/// and PAL passes whatever was in them to `eraDat`, so C's result there
/// depends on the stack; 0.0 is what `eraDat` gives for the year, month and
/// day fields `eraJd2cal` leaves zero, and it is what this returns.
pub(crate) fn dat(dju: f64) -> f64 {
    let Ok((iy, im, id, fd)) = erfa::jd2cal(PAL__MJD0, dju) else {
        return 0.0;
    };
    match erfa::dat(iy, im, id, fd) {
        Ok((deltat, _)) => deltat,
        Err(_) => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::*;

    #[test]
    fn dat_matches_vendored_c_where_c_is_defined() {
        let mut defined = 0;
        let mut undefined = 0;
        for m in mjd_cases() {
            // SAFETY: a pure function of its argument.
            let c = unsafe { super::super::astPalDat(m) };
            if erfa::jd2cal(PAL__MJD0, m).is_ok() {
                assert!(same(dat(m), c), "{m:e}: {:e} vs C {c:e}", dat(m));
                defined += 1;
            } else {
                // C reads the calendar fields `eraJd2cal` did not write, so
                // its result is whatever the stack held; the port gives 0.0.
                assert_eq!(dat(m).to_bits(), 0.0_f64.to_bits(), "{m:e}");
                undefined += 1;
            }
        }
        assert!(defined > 270_000 && undefined > 10);
    }

    #[test]
    fn dat_gives_zero_for_dates_no_calendar_holds() {
        for m in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            crate::constants::BAD_VALUE,
            1e300,
            -1e300,
        ] {
            assert_eq!(dat(m).to_bits(), 0.0_f64.to_bits(), "{m:e}");
        }
    }
}
