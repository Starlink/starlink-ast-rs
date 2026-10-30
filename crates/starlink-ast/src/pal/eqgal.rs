// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palEqgal (pal/palEqgal.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use crate::erfa::{self, M3};

/// The equatorial-to-galactic rotation matrix (J2000.0), obtained by applying the
/// standard FK4 to FK5 transformation, for zero proper motion in FK5, to the
/// columns of the B1950 equatorial to galactic rotation matrix.
const RMAT: M3 = [
    [-0.054875539726, -0.873437108010, -0.483834985808],
    [0.494109453312, -0.444829589425, 0.746982251810],
    [-0.867666135858, -0.198076386122, 0.455983795705],
];

/// FK5 J2000 (ra, dec) to galactic (l, b), in radians, with l in 0 to 2 pi and b in -pi to pi.
pub(crate) fn eqgal(dr: f64, dd: f64) -> (f64, f64) {
    let v1 = erfa::s2c(dr, dd);
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
    fn eqgal_matches_vendored_c_bit_for_bit() {
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
            unsafe { super::super::astPalEqgal(a, b, &mut c1, &mut c2) };
            let r = eqgal(a, b);
            assert!(
                same(r.0, c1) && same(r.1, c2),
                "{a:e} {b:e}: {r:?} vs C ({c1:e}, {c2:e})"
            );
        }
    }
}
