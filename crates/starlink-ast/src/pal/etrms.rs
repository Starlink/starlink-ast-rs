// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palEtrms (pal/palEtrms.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::PAL__DAS2R;
use crate::erfa::{self, V3};

/// The E-terms of aberration at Besselian-style epoch `ep` (a Julian year
/// number such as 1950.0), as a vector in the mean equator and equinox of
/// that epoch.
pub(crate) fn etrms(ep: f64) -> V3 {
    // The J2000 aberration constant.
    const ABCONST: f64 = 20.49552;

    // Julian centuries since B1950.
    let t = (ep - 1950.) * 0.0100002135903;

    // Eccentricity.
    let e = 0.01673011 - (t * 1.26e-7 + 4.193e-5) * t;

    // Mean obliquity.
    let e0 = (84404.836 - ((t * 0.00181 + 0.00319) * t + 46.8495) * t) * PAL__DAS2R;

    // Mean longitude of perihelion.
    let p = (((t * 0.012 + 1.65) * t + 6190.67) * t + 1015489.951) * PAL__DAS2R;

    // E-terms. PAL calls `sin` and `cos` of each of `p` and `e0`.
    let ek = e * ABCONST * PAL__DAS2R;
    let (sp, cp) = erfa::sincos(p);
    let (se0, ce0) = erfa::sincos(e0);
    [ek * sp, -ek * cp * ce0, -ek * cp * se0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::same3;
    use crate::test_inputs::angles;

    #[test]
    fn etrms_matches_vendored_c_bit_for_bit() {
        let mut eps = angles(2_000);
        eps.extend([
            1000.0,
            1900.0,
            1950.0,
            2000.0,
            2100.0,
            3000.0,
            crate::constants::BAD_VALUE,
        ]);
        for k in 0..2_000 {
            eps.push(1000.0 + f64::from(k) * 1.0037);
        }
        for ep in eps {
            let mut c = [0.0; 3];
            // SAFETY: the output holds three doubles.
            unsafe { super::super::ffi::astPalEtrms(ep, c.as_mut_ptr()) };
            assert!(same3(&etrms(ep), &c), "{ep:e}: {:?} vs C {c:?}", etrms(ep));
        }
    }
}
