// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palGalsup (pal/palGalsup.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use crate::erfa::{self, M3};

/// The galactic to supergalactic rotation matrix.
const RMAT: M3 = [
    [-0.735742574804, 0.677261296414, 0.000000000000],
    [-0.074553778365, -0.080991471307, 0.993922590400],
    [0.673145302109, 0.731271165817, 0.110081262225],
];

/// Galactic (l, b) to supergalactic (sl, sb), in radians.
pub(crate) fn galsup(dl: f64, db: f64) -> (f64, f64) {
    let v1 = erfa::s2c(dl, db);
    let v2 = erfa::rxp(&RMAT, &v1);
    let (a, b) = erfa::c2s(v2);
    (erfa::anp(a), erfa::anpm(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::same;
    use crate::test_inputs::{angle_pairs, specials};

    #[test]
    fn galsup_matches_vendored_c_bit_for_bit() {
        let mut pairs = angle_pairs(20_000);
        for &a in &specials() {
            for &b in &specials() {
                pairs.push((a, b));
            }
        }
        let pole = std::f64::consts::FRAC_PI_2;
        for d in [pole, -pole, pole - 1e-15, -(pole - 1e-15)] {
            for a in [0.0, 1.0, 3.0, 6.0] {
                pairs.push((a, d));
            }
        }
        for (a, b) in pairs {
            let (mut c1, mut c2) = (0.0, 0.0);
            // SAFETY: scalar outputs.
            unsafe { super::super::astPalGalsup(a, b, &mut c1, &mut c2) };
            let r = galsup(a, b);
            assert!(
                same(r.0, c1) && same(r.1, c2),
                "{a:e} {b:e}: {r:?} vs C ({c1:e}, {c2:e})"
            );
        }
    }
}
