// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palRvlsrd (pal/palRvlsrd.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use crate::erfa;

/// The peculiar solar motion of Delhaye (1965), (+9, +12, +7) km/s in Galactic Cartesian coordinates, as a J2000.0 equatorial vector in km/s.
const VA: [f64; 3] = [0.63823, 14.58542, -7.80116];

/// The component, in km/s, of the dynamical LSR velocity towards the J2000.0 position `(r2000, d2000)`.
pub(crate) fn rvlsrd(r2000: f64, d2000: f64) -> f64 {
    let vb = erfa::s2c(r2000, d2000);
    erfa::pdp(&VA, &vb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::same;
    use crate::test_inputs::{angle_pairs, specials};

    #[test]
    fn rvlsrd_matches_vendored_c_bit_for_bit() {
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
        for (a, d) in pairs {
            // SAFETY: scalar arguments.
            let c = unsafe { super::super::astPalRvlsrd(a, d) };
            assert!(same(rvlsrd(a, d), c), "{a:e} {d:e}");
        }
    }
}
