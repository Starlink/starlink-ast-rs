// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palAddet (pal/palAddet.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::etrms::etrms;
use crate::erfa;

/// Adds the E-terms of aberration for equinox `eq` to the mean place
/// `(rm, dm)`, giving the apparent `(rc, dc)`, in radians with `rc` in 0 to
/// 2 pi.
pub(crate) fn addet(rm: f64, dm: f64, eq: f64) -> (f64, f64) {
    let a = etrms(eq);
    let mut v = erfa::s2c(rm, dm);
    for i in 0..3 {
        v[i] += a[i];
    }
    let (rc, dc) = erfa::c2s(v);
    (erfa::anp(rc), dc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::same;
    use crate::test_inputs::{angle_pairs, specials};

    #[test]
    fn addet_matches_vendored_c_bit_for_bit() {
        for (a, b, e) in super::super::subet::tests::et_inputs(angle_pairs(1_000), specials()) {
            let (mut c1, mut c2) = (0.0, 0.0);
            // SAFETY: scalar outputs.
            unsafe { super::super::astPalAddet(a, b, e, &mut c1, &mut c2) };
            let r = addet(a, b, e);
            assert!(
                same(r.0, c1) && same(r.1, c2),
                "{a:e} {b:e} {e:e}: {r:?} vs C ({c1:e}, {c2:e})"
            );
        }
    }
}
