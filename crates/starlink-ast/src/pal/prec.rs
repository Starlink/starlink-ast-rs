// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palPrec (pal/palPrec.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use crate::erfa::{self, ERFA_DJ00, ERFA_DJY, M3};

/// The Julian (FK5) precession matrix from Julian epoch `ep0` to Julian epoch
/// `ep1`, from the IAU 2006 model.
pub(crate) fn prec(ep0: f64, ep1: f64) -> M3 {
    // Convert supplied dates to days since J2000.
    let ep0_days = (ep0 - 2000.0) * ERFA_DJY;
    let ep1_days = (ep1 - 2000.0) * ERFA_DJY;

    // If the beginning epoch is J2000, just return the rotation matrix from
    // J2000 to EP1.
    if ep0 == 2000.0 {
        erfa::pmat06(ERFA_DJ00, ep1_days)
    // If the end epoch is J2000, get the rotation matrix from J2000 to EP0 and
    // then transpose it to get the rotation matrix from EP0 to J2000.
    } else if ep1 == 2000.0 {
        erfa::tr(&erfa::pmat06(ERFA_DJ00, ep0_days))
    // Otherwise, get the two matrices used above and multiply them together.
    } else {
        let rmatp = erfa::tr(&erfa::pmat06(ERFA_DJ00, ep0_days));
        let rmatq = erfa::pmat06(ERFA_DJ00, ep1_days);
        erfa::rxr(&rmatp, &rmatq)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::same3;
    use crate::test_inputs::{SplitMix64, specials};

    #[test]
    fn prec_matches_vendored_c_bit_for_bit() {
        let mut epochs = vec![1000.0, 1900.0, 1950.0, 2000.0, 2100.0, 3000.0];
        let mut rng = SplitMix64::new(77);
        epochs.extend((0..300).map(|_| 1000.0 + rng.next_f64() * 2000.0));
        epochs.extend(specials());
        let mut pairs = Vec::new();
        for &a in &epochs {
            for &b in &[1000.0, 1950.0, 2000.0, 2100.0, 3000.0, 1999.5, f64::NAN] {
                pairs.push((a, b));
                pairs.push((b, a));
            }
        }
        for k in 0..2_000 {
            pairs.push((epochs[k % epochs.len()], epochs[(k * 7 + 3) % epochs.len()]));
        }
        for (a, b) in pairs {
            let mut c = [[0.0; 3]; 3];
            // SAFETY: nine outputs.
            unsafe { super::super::astPalPrec(a, b, c.as_mut_ptr()) };
            let r = prec(a, b);
            assert!((0..3).all(|i| same3(&r[i], &c[i])), "{a:e} {b:e}");
        }
    }
}
