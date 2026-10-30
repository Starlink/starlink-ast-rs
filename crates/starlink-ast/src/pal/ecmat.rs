// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palEcmat (pal/palEcmat.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::PAL__MJD0;
use super::deuler::deuler;
use crate::erfa::{self, M3};

/// The rotation matrix from mean equatorial to mean ecliptic coordinates of the
/// TDB date `date` (a modified Julian date), IAU 2006 model.
pub(crate) fn ecmat(date: f64) -> M3 {
    // Mean obliquity (the angle between the ecliptic and mean equator of date).
    let eps0 = erfa::obl06(PAL__MJD0, date);

    // Matrix.
    deuler("X", eps0, 0.0, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::same3;
    use crate::test_inputs::{SplitMix64, specials};

    #[test]
    fn ecmat_matches_vendored_c_bit_for_bit() {
        let mut dates = vec![-313_000.0, 15_020.0, 33_282.0, 51_544.5, 88_069.0, 1.4e6];
        let mut rng = SplitMix64::new(79);
        dates.extend((0..20_000).map(|_| -3.0e5 + rng.next_f64() * 1.4e6));
        dates.extend(specials());
        for d in dates {
            let mut c = [[0.0; 3]; 3];
            // SAFETY: nine outputs.
            unsafe { super::super::astPalEcmat(d, c.as_mut_ptr()) };
            let r = ecmat(d);
            assert!((0..3).all(|i| same3(&r[i], &c[i])), "{d:e}");
        }
    }
}
