// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Copyright (C) 2013-2016, NumFOCUS Foundation.
// All rights reserved.
//
// This library is derived, with permission, from the International
// Astronomical Union's "Standards of Fundamental Astronomy" library,
// available from http://www.iausofa.org.
//
// The ERFA version is intended to retain identical functionality to
// the SOFA library, but made distinct through different function and
// file names, as set out in the SOFA license conditions.  The SOFA
// original has a role as a reference standard for the IAU and IERS,
// and consequently redistribution is permitted only in its unaltered
// state.  The ERFA version is not subject to this restriction and
// therefore can be included in distributions which do not support the
// concept of "read only" software.
//
// Although the intent is to replicate the SOFA API (other than
// replacement of prefix names) and results (with the exception of
// bugs;  any that are discovered will be fixed), SOFA is not
// responsible for any errors found in this version of the library.
//
// If you wish to acknowledge the SOFA heritage, please acknowledge
// that you are using a library derived from SOFA, rather than SOFA
// itself.
//
//
// TERMS AND CONDITIONS
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
//
// 1 Redistributions of source code must retain the above copyright
//   notice, this list of conditions and the following disclaimer.
//
// 2 Redistributions in binary form must reproduce the above copyright
//   notice, this list of conditions and the following disclaimer in
//   the documentation and/or other materials provided with the
//   distribution.
//
// 3 Neither the name of the Standards Of Fundamental Astronomy Board,
//   the International Astronomical Union nor the names of its
//   contributors may be used to endorse or promote products derived
//   from this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
// FOR A PARTICULAR PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE
// COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
// INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
// BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
// LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
// LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
// ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
// POSSIBILITY OF SUCH DAMAGE.
//
// Ported from ERFA eraS06 (erfa/s06.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::{
    ERFA_DAS2R, ERFA_DJ00, ERFA_DJC, fad03, fae03, faf03, fal03, falp03, faom03, fapa03, fave03,
    sincos,
};

/// One term of a series for s + XY/2: the multipliers of the fundamental
/// arguments l, l', F, D, Omega, LVe, LE and pA, then the sine and cosine
/// amplitudes in arcseconds.
struct Term {
    nfa: [i8; 8],
    s: f64,
    c: f64,
}

#[allow(clippy::too_many_arguments)]
const fn term(
    n0: i8,
    n1: i8,
    n2: i8,
    n3: i8,
    n4: i8,
    n5: i8,
    n6: i8,
    n7: i8,
    s: f64,
    c: f64,
) -> Term {
    Term {
        nfa: [n0, n1, n2, n3, n4, n5, n6, n7],
        s,
        c,
    }
}

/// Polynomial coefficients.
const SP: [f64; 6] = [
    // 1-6
    94.00e-6,
    3808.65e-6,
    -122.68e-6,
    -72574.11e-6,
    27.98e-6,
    15.62e-6,
];

/// Terms of order t^0.
const S0: [Term; 33] = [
    // 1-10
    term(0, 0, 0, 0, 1, 0, 0, 0, -2640.73e-6, 0.39e-6),
    term(0, 0, 0, 0, 2, 0, 0, 0, -63.53e-6, 0.02e-6),
    term(0, 0, 2, -2, 3, 0, 0, 0, -11.75e-6, -0.01e-6),
    term(0, 0, 2, -2, 1, 0, 0, 0, -11.21e-6, -0.01e-6),
    term(0, 0, 2, -2, 2, 0, 0, 0, 4.57e-6, 0.00e-6),
    term(0, 0, 2, 0, 3, 0, 0, 0, -2.02e-6, 0.00e-6),
    term(0, 0, 2, 0, 1, 0, 0, 0, -1.98e-6, 0.00e-6),
    term(0, 0, 0, 0, 3, 0, 0, 0, 1.72e-6, 0.00e-6),
    term(0, 1, 0, 0, 1, 0, 0, 0, 1.41e-6, 0.01e-6),
    term(0, 1, 0, 0, -1, 0, 0, 0, 1.26e-6, 0.01e-6),
    // 11-20
    term(1, 0, 0, 0, -1, 0, 0, 0, 0.63e-6, 0.00e-6),
    term(1, 0, 0, 0, 1, 0, 0, 0, 0.63e-6, 0.00e-6),
    term(0, 1, 2, -2, 3, 0, 0, 0, -0.46e-6, 0.00e-6),
    term(0, 1, 2, -2, 1, 0, 0, 0, -0.45e-6, 0.00e-6),
    term(0, 0, 4, -4, 4, 0, 0, 0, -0.36e-6, 0.00e-6),
    term(0, 0, 1, -1, 1, -8, 12, 0, 0.24e-6, 0.12e-6),
    term(0, 0, 2, 0, 0, 0, 0, 0, -0.32e-6, 0.00e-6),
    term(0, 0, 2, 0, 2, 0, 0, 0, -0.28e-6, 0.00e-6),
    term(1, 0, 2, 0, 3, 0, 0, 0, -0.27e-6, 0.00e-6),
    term(1, 0, 2, 0, 1, 0, 0, 0, -0.26e-6, 0.00e-6),
    // 21-30
    term(0, 0, 2, -2, 0, 0, 0, 0, 0.21e-6, 0.00e-6),
    term(0, 1, -2, 2, -3, 0, 0, 0, -0.19e-6, 0.00e-6),
    term(0, 1, -2, 2, -1, 0, 0, 0, -0.18e-6, 0.00e-6),
    term(0, 0, 0, 0, 0, 8, -13, -1, 0.10e-6, -0.05e-6),
    term(0, 0, 0, 2, 0, 0, 0, 0, -0.15e-6, 0.00e-6),
    term(2, 0, -2, 0, -1, 0, 0, 0, 0.14e-6, 0.00e-6),
    term(0, 1, 2, -2, 2, 0, 0, 0, 0.14e-6, 0.00e-6),
    term(1, 0, 0, -2, 1, 0, 0, 0, -0.14e-6, 0.00e-6),
    term(1, 0, 0, -2, -1, 0, 0, 0, -0.14e-6, 0.00e-6),
    term(0, 0, 4, -2, 4, 0, 0, 0, -0.13e-6, 0.00e-6),
    // 31-33
    term(0, 0, 2, -2, 4, 0, 0, 0, 0.11e-6, 0.00e-6),
    term(1, 0, -2, 0, -3, 0, 0, 0, -0.11e-6, 0.00e-6),
    term(1, 0, -2, 0, -1, 0, 0, 0, -0.11e-6, 0.00e-6),
];

/// Terms of order t^1.
const S1: [Term; 3] = [
    // 1 - 3
    term(0, 0, 0, 0, 2, 0, 0, 0, -0.07e-6, 3.57e-6),
    term(0, 0, 0, 0, 1, 0, 0, 0, 1.73e-6, -0.03e-6),
    term(0, 0, 2, -2, 3, 0, 0, 0, 0.00e-6, 0.48e-6),
];

/// Terms of order t^2.
const S2: [Term; 25] = [
    // 1-10
    term(0, 0, 0, 0, 1, 0, 0, 0, 743.52e-6, -0.17e-6),
    term(0, 0, 2, -2, 2, 0, 0, 0, 56.91e-6, 0.06e-6),
    term(0, 0, 2, 0, 2, 0, 0, 0, 9.84e-6, -0.01e-6),
    term(0, 0, 0, 0, 2, 0, 0, 0, -8.85e-6, 0.01e-6),
    term(0, 1, 0, 0, 0, 0, 0, 0, -6.38e-6, -0.05e-6),
    term(1, 0, 0, 0, 0, 0, 0, 0, -3.07e-6, 0.00e-6),
    term(0, 1, 2, -2, 2, 0, 0, 0, 2.23e-6, 0.00e-6),
    term(0, 0, 2, 0, 1, 0, 0, 0, 1.67e-6, 0.00e-6),
    term(1, 0, 2, 0, 2, 0, 0, 0, 1.30e-6, 0.00e-6),
    term(0, 1, -2, 2, -2, 0, 0, 0, 0.93e-6, 0.00e-6),
    // 11-20
    term(1, 0, 0, -2, 0, 0, 0, 0, 0.68e-6, 0.00e-6),
    term(0, 0, 2, -2, 1, 0, 0, 0, -0.55e-6, 0.00e-6),
    term(1, 0, -2, 0, -2, 0, 0, 0, 0.53e-6, 0.00e-6),
    term(0, 0, 0, 2, 0, 0, 0, 0, -0.27e-6, 0.00e-6),
    term(1, 0, 0, 0, 1, 0, 0, 0, -0.27e-6, 0.00e-6),
    term(1, 0, -2, -2, -2, 0, 0, 0, -0.26e-6, 0.00e-6),
    term(1, 0, 0, 0, -1, 0, 0, 0, -0.25e-6, 0.00e-6),
    term(1, 0, 2, 0, 1, 0, 0, 0, 0.22e-6, 0.00e-6),
    term(2, 0, 0, -2, 0, 0, 0, 0, -0.21e-6, 0.00e-6),
    term(2, 0, -2, 0, -1, 0, 0, 0, 0.20e-6, 0.00e-6),
    // 21-25
    term(0, 0, 2, 2, 2, 0, 0, 0, 0.17e-6, 0.00e-6),
    term(2, 0, 2, 0, 2, 0, 0, 0, 0.13e-6, 0.00e-6),
    term(2, 0, 0, 0, 0, 0, 0, 0, -0.13e-6, 0.00e-6),
    term(1, 0, 2, -2, 2, 0, 0, 0, -0.12e-6, 0.00e-6),
    term(0, 0, 2, 0, 0, 0, 0, 0, -0.11e-6, 0.00e-6),
];

/// Terms of order t^3.
const S3: [Term; 4] = [
    // 1-4
    term(0, 0, 0, 0, 1, 0, 0, 0, 0.30e-6, -23.42e-6),
    term(0, 0, 2, -2, 2, 0, 0, 0, -0.03e-6, -1.46e-6),
    term(0, 0, 2, 0, 2, 0, 0, 0, -0.01e-6, -0.25e-6),
    term(0, 0, 0, 0, 2, 0, 0, 0, 0.00e-6, 0.23e-6),
];

/// Terms of order t^4.
const S4: [Term; 1] = [
    // 1-1
    term(0, 0, 0, 0, 1, 0, 0, 0, -0.26e-6, -0.01e-6),
];

/// The sum of one series at the fundamental arguments `fa`, added to `w`
/// from the last term to the first.
#[inline(always)]
fn series(mut w: f64, terms: &[Term], fa: &[f64; 8]) -> f64 {
    for term in terms.iter().rev() {
        let mut a = 0.0;
        for (&n, &f) in term.nfa.iter().zip(fa) {
            a += f64::from(n) * f;
        }
        let (sa, ca) = sincos(a);
        w += term.s * sa + term.c * ca;
    }
    w
}

/// The CIO locator s, in radians, for the TT date `date1 + date2` (a two-part
/// Julian Date), given the CIP coordinates `x` and `y`, consistent with the
/// IAU 2006 precession and IAU 2000A nutation.
pub(crate) fn s06(date1: f64, date2: f64, x: f64, y: f64) -> f64 {
    // Interval between fundamental epoch J2000.0 and current date (JC).
    let t = ((date1 - ERFA_DJ00) + date2) / ERFA_DJC;

    // Fundamental Arguments (from IERS Conventions 2003): mean anomaly of the
    // Moon, mean anomaly of the Sun, mean longitude of the Moon minus that of
    // the ascending node, mean elongation of the Moon from the Sun, mean
    // longitude of the ascending node of the Moon, mean longitude of Venus,
    // mean longitude of Earth, general precession in longitude.
    let fa = [
        fal03(t),
        falp03(t),
        faf03(t),
        fad03(t),
        faom03(t),
        fave03(t),
        fae03(t),
        fapa03(t),
    ];

    // Evaluate s.
    let w0 = series(SP[0], &S0, &fa);
    let w1 = series(SP[1], &S1, &fa);
    let w2 = series(SP[2], &S2, &fa);
    let w3 = series(SP[3], &S3, &fa);
    let w4 = series(SP[4], &S4, &fa);
    let w5 = SP[5];

    (w0 + (w1 + (w2 + (w3 + (w4 + w5 * t) * t) * t) * t) * t) * ERFA_DAS2R - x * y / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erfa::difftest::*;
    use crate::test_inputs::*;

    #[test]
    fn s06_matches_vendored_c_bit_for_bit() {
        let mut rng = SplitMix64::new(0x5eed_5060);
        let sp = specials();
        for (k, (d1, d2)) in date_pairs().into_iter().enumerate() {
            // CIP coordinates of a few milliradians, and the specials.
            let (x, y) = if k % 7 == 0 {
                (sp[k % sp.len()], sp[(k / 7) % sp.len()])
            } else {
                ((rng.next_f64() - 0.5) * 0.02, (rng.next_f64() - 0.5) * 0.02)
            };
            // SAFETY: a pure function of its arguments.
            let c = unsafe { astEraS06(d1, d2, x, y) };
            assert!(same(s06(d1, d2, x, y), c), "{d1:e} {d2:e} {x:e} {y:e}");
        }
    }
}
