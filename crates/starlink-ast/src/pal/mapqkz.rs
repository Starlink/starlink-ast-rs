// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palMapqkz (pal/palMapqkz.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::mappa::npb_matrix;
use crate::erfa;

/// Mean place `(rm, dm)` to geocentric apparent `(ra, da)`, in radians with
/// `ra` in 0 to 2 pi, for a star with no proper motion, parallax or radial
/// velocity, using the parameters from [`super::mappa`]. Light deflection is
/// restrained within the Sun's disc.
pub(crate) fn mapqkz(rm: f64, dm: f64, amprms: &[f64; 21]) -> (f64, f64) {
    // Unpack scalar and vector parameters.
    let ab1 = amprms[11];
    let gr2e = amprms[7];
    let abv = [amprms[8], amprms[9], amprms[10]];
    let ehn = [amprms[4], amprms[5], amprms[6]];

    // Spherical to x,y,z.
    let p = erfa::s2c(rm, dm);

    // Light deflection (restrained within the Sun's disc).
    let pde = erfa::pdp(&p, &ehn);
    let pdep1 = pde + 1.0;
    let w = gr2e / (if pdep1 > 1.0e-5 { pdep1 } else { 1.0e-5 });
    let mut p1 = [0.0; 3];
    for i in 0..3 {
        p1[i] = p[i] + w * (ehn[i] - pde * p[i]);
    }

    // Aberration.
    let p1dv = erfa::pdp(&p1, &abv);
    let w = 1.0 + p1dv / (ab1 + 1.0);
    let mut p2 = [0.0; 3];
    for i in 0..3 {
        p2[i] = (ab1 * p1[i]) + (w * abv[i]);
    }

    // Precession and nutation.
    let p3 = erfa::rxp(&npb_matrix(amprms), &p2);

    // Geocentric apparent RA,dec.
    let (ra, da) = erfa::c2s(p3);
    (erfa::anp(ra), da)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::erfa::difftest::same;
    use crate::pal::mappa::tests::c_amprms;
    use crate::test_inputs::specials;

    /// The shared angle pairs, both poles and the latitudes 1e-15 radian
    /// inside them at several longitudes, then the special values paired
    /// with each other and with ordinary angles.
    pub(in crate::pal) fn place_inputs() -> Vec<(f64, f64)> {
        let mut out = crate::erfa::difftest::fk_positions();
        let s = specials();
        for (i, &a) in s.iter().enumerate() {
            out.push((a, 0.5));
            out.push((1.0, a));
            out.push((a, s[(i * 7 + 3) % s.len()]));
        }
        out
    }

    #[test]
    fn mapqkz_matches_vendored_c_bit_for_bit() {
        for amprms in c_amprms() {
            for &(rm, dm) in &place_inputs() {
                let (mut c1, mut c2) = (0.0, 0.0);
                // SAFETY: 21 inputs and scalar outputs.
                unsafe { super::super::astPalMapqkz(rm, dm, amprms.as_ptr(), &mut c1, &mut c2) };
                let r = mapqkz(rm, dm, &amprms);
                assert!(
                    same(r.0, c1) && same(r.1, c2),
                    "{rm:e} {dm:e} {amprms:?}: {r:?} vs C ({c1:e}, {c2:e})"
                );
            }
        }
    }
}
