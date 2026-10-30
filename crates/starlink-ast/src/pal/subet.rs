// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palSubet (pal/palSubet.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

use super::etrms::etrms;
use crate::erfa;

/// Removes the E-terms of aberration for equinox `eq` from the apparent place
/// `(rc, dc)`, giving the mean `(rm, dm)`, in radians with `rm` in 0 to
/// 2 pi.
pub(crate) fn subet(rc: f64, dc: f64, eq: f64) -> (f64, f64) {
    let a = etrms(eq);
    let mut v = erfa::s2c(rc, dc);
    let f = 1.0 + erfa::pdp(&v, &a);
    for i in 0..3 {
        v[i] = f * v[i] - a[i];
    }
    let (rm, dm) = erfa::c2s(v);
    (erfa::anp(rm), dm)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::erfa::difftest::same;
    use crate::test_inputs::{angle_pairs, specials};

    /// Positions crossed with the epochs, the poles and the specials that
    /// the E-term tests share.
    pub(in crate::pal) fn et_inputs(pairs: Vec<(f64, f64)>, sp: Vec<f64>) -> Vec<(f64, f64, f64)> {
        let epochs = [
            1000.0,
            1900.0,
            1950.0,
            2000.0,
            2100.0,
            3000.0,
            crate::constants::BAD_VALUE,
        ];
        let pole = std::f64::consts::FRAC_PI_2;
        let mut ps = pairs;
        for d in [pole, -pole, pole - 1e-15, -(pole - 1e-15)] {
            for a in [0.0, 1.0, 3.0, 6.0] {
                ps.push((a, d));
            }
        }
        let mut out = Vec::new();
        for (i, &(a, b)) in ps.iter().enumerate() {
            out.push((a, b, epochs[i % epochs.len()]));
            out.push((a, b, 1950.0 + (i % 200) as f64));
        }
        for &a in &sp {
            for &b in &sp {
                for e in [1950.0, 2000.0, f64::NAN] {
                    out.push((a, b, e));
                }
            }
        }
        out
    }

    #[test]
    fn subet_matches_vendored_c_bit_for_bit() {
        for (a, b, e) in et_inputs(angle_pairs(1_000), specials()) {
            let (mut c1, mut c2) = (0.0, 0.0);
            // SAFETY: scalar outputs.
            unsafe { super::super::astPalSubet(a, b, e, &mut c1, &mut c2) };
            let r = subet(a, b, e);
            assert!(
                same(r.0, c1) && same(r.1, c2),
                "{a:e} {b:e} {e:e}: {r:?} vs C ({c1:e}, {c2:e})"
            );
        }
    }
}
