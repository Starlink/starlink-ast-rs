// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// ====================================================
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Developed at SunSoft, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this
// software is freely granted, provided that this notice
// is preserved.
// ====================================================
//
// Algorithm and constants from fdlibm's s_atan.c and e_atan2.c via musl
// src/math/atan.c and atan2.c; branches replaced by selects.

// The constants are fdlibm's, written as fdlibm writes them so they can be
// checked against it digit for digit.
#![allow(clippy::excessive_precision, clippy::approx_constant)]

use std::f64::consts::PI;

/// atan of the interval boundaries' reference points, high and low parts.
const ATANHI: [f64; 4] = [
    4.63647609000806093515e-01, // atan(0.5)hi
    7.85398163397448278999e-01, // atan(1.0)hi
    9.82793723247329054082e-01, // atan(1.5)hi
    1.57079632679489655800e+00, // atan(inf)hi
];
const ATANLO: [f64; 4] = [
    2.26987774529616870924e-17,
    3.06161699786838301793e-17,
    1.39033110312309984516e-17,
    6.12323399573676603587e-17,
];
/// The polynomial approximating atan on the reduced interval.
const AT: [f64; 11] = [
    3.33333333333329318027e-01,
    -1.99999999998764832476e-01,
    1.42857142725034663711e-01,
    -1.11111104054623557880e-01,
    9.09088713343650656196e-02,
    -7.69187620504482999495e-02,
    6.66107313738753120669e-02,
    -5.83357013379057348645e-02,
    4.97687799461593236017e-02,
    -3.65315727442169155270e-02,
    1.62858201153657823623e-02,
];
/// The low part of pi: pi = PI + PI_LO to more than double precision.
const PI_LO: f64 = 1.2246467991473531772e-16;

/// a + b as a rounded sum and its exact rounding error (Knuth's two-sum).
#[inline(always)]
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}

/// y - q * x rounded once, by a fused multiply-add: one instruction where
/// the code it is inlined into is built with FMA, a library call elsewhere.
#[inline(always)]
fn residual_fma(y: f64, q: f64, x: f64) -> f64 {
    (-q).mul_add(x, y)
}

/// [`residual_fma`] without a fused multiply-add, for a `q` within a few ULP
/// of `y / x`: Dekker's product gives q * x as p + e exactly, `y - p` is then
/// exact, and `(y - p) - e` is the one rounding of the exact residual.
#[inline(always)]
fn residual_dekker(y: f64, q: f64, x: f64) -> f64 {
    const SPLIT: f64 = 134_217_729.0; // 2^27 + 1
    let split = |a: f64| {
        let c = SPLIT * a;
        let hi = c - (c - a);
        (hi, a - hi)
    };
    let p = q * x;
    let (qh, ql) = split(q);
    let (xh, xl) = split(x);
    let e = ((qh * xh - p) + qh * xl + ql * xh) + ql * xl;
    (y - p) - e
}

/// The exact rounding error of the quotient `q = y / x`, times `x`: by a
/// fused multiply-add when `FMA` is set, and otherwise by Dekker's product,
/// which gives the same bits, since without FMA hardware `mul_add` is a
/// library call that stops the loop vectorizing.
#[inline(always)]
fn quotient_residual<const FMA: bool>(y: f64, q: f64, x: f64) -> f64 {
    if FMA {
        residual_fma(y, q, x)
    } else {
        residual_dekker(y, q, x)
    }
}

/// atan(a + da) for a >= 0 and a small `da`, as an unevaluated sum
/// `(head, tail)`: fdlibm's reduction to one of five intervals, with the
/// interval chosen by selects rather than branches, and the last addition
/// left to the caller so that it rounds once.
#[inline(always)]
fn atan_nonneg(a: f64, da: f64) -> (f64, f64) {
    let (c0, c1, c2, c3) = (a < 0.4375, a < 0.6875, a < 1.1875, a < 2.4375);
    let (num, den) = if c0 {
        (a, 1.0)
    } else if c1 {
        (2.0 * a - 1.0, 2.0 + a)
    } else if c2 {
        (a - 1.0, a + 1.0)
    } else if c3 {
        (a - 1.5, 1.0 + 1.5 * a)
    } else {
        (-1.0, a)
    };
    let x = num / den;
    // Selected, not indexed: an index computed per element is a gather,
    // which stops the loop vectorizing.
    let hi = if c1 {
        ATANHI[0]
    } else if c2 {
        ATANHI[1]
    } else if c3 {
        ATANHI[2]
    } else {
        ATANHI[3]
    };
    let lo = if c1 {
        ATANLO[0]
    } else if c2 {
        ATANLO[1]
    } else if c3 {
        ATANLO[2]
    } else {
        ATANLO[3]
    };
    let z = x * x;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));
    let p = x * (s1 + s2);
    // atan(a + da) - atan(a) is da / (1 + a^2) to far more than da's own
    // precision; it is zero where da or the ratio is not finite.
    let d = da / (1.0 + a * a);
    let d = if d.is_finite() { d } else { 0.0 };
    if c0 {
        (x, d - p)
    } else {
        // fdlibm's hi - ((p - lo) - x) rounds twice; hi + x is kept exact.
        let (h, e) = two_sum(hi, x);
        (h, e + ((lo + d) - p))
    }
}

/// The arctangent of `y / x` in the quadrant of (x, y), in (-pi, pi], for
/// finite non-zero `y` and `x` (see [`atan2_covers`]); any other input gives
/// an unspecified value. Within 1 ULP of libm. fdlibm's `atan(y / x)` lets
/// the rounding of the quotient carry into the result, which takes it to 2
/// ULP; here the quotient's exact rounding error is carried as a correction
/// instead, and the final additions, `pi - atan` included, round once. On
/// random directions it differs from macOS's libm for 1.6% of longitudes
/// and 4.2% of latitudes, against 12% and 18% for fdlibm's arithmetic, and
/// it is correctly rounded at least as often as that libm.
///
/// `FMA` says whether the code this is inlined into may use fused
/// multiply-adds: [`super::HAS_FMA`] where the build guarantees them, `true`
/// inside a function built for an FMA target feature. The result is the same
/// either way.
#[inline(always)]
pub(crate) fn atan2<const FMA: bool>(y: f64, x: f64) -> f64 {
    let q = y / x;
    let eq = quotient_residual::<FMA>(y, q, x) / x;
    let eq = if eq.is_finite() { eq } else { 0.0 };
    let (h, t) = atan_nonneg(q.abs(), if q < 0.0 { -eq } else { eq });
    let r = if x > 0.0 {
        h + t
    } else {
        let (s, e) = two_sum(PI, -h);
        s + (e + (PI_LO - t))
    };
    if y < 0.0 { -r } else { r }
}

/// Whether [`atan2`] is valid for (`y`, `x`): both finite and non-zero.
#[inline(always)]
pub(crate) fn atan2_covers(y: f64, x: f64) -> bool {
    y.is_finite() && x.is_finite() && y != 0.0 && x != 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math;
    use crate::test_inputs::{angle_pairs, ulps};
    use crate::vmath::HAS_FMA;

    /// Ratios at each of the reduction's interval boundaries, and the three
    /// doubles either side of each.
    fn boundary_ratios() -> Vec<f64> {
        let mut out = Vec::new();
        for b in [7.0f64 / 16.0, 11.0 / 16.0, 19.0 / 16.0, 39.0 / 16.0, 1.0] {
            for k in -3i64..=3 {
                out.push(f64::from_bits(b.to_bits().wrapping_add_signed(k)));
            }
        }
        out
    }

    #[test]
    fn atan2_is_within_one_ulp_of_libm() {
        let mut pairs = angle_pairs(1_000_000);
        for r in boundary_ratios() {
            for (sy, sx) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                pairs.push((sy * r, sx));
                pairs.push((sy * r * 3.5, sx * 3.5));
            }
        }
        pairs.extend([
            (1e-300, -1e300),
            (1e300, 1e-300),
            (-1e300, -1e-300),
            (5e-324, 1.0),
        ]);
        // Pairs where the rounding of y / x would carry into the result.
        pairs.extend([
            (0.23205091756292417, 0.9253975077794019),
            (4.531536751418496e5, 7.249072995150625e6),
        ]);
        let mut worst = 0;
        for (y, x) in pairs.into_iter().filter(|&(y, x)| atan2_covers(y, x)) {
            let d = ulps(atan2::<HAS_FMA>(y, x), math::atan2(y, x));
            assert!(
                d <= 1,
                "atan2({y:e}, {x:e}) = {:e}, libm {:e}",
                atan2::<HAS_FMA>(y, x),
                math::atan2(y, x)
            );
            worst = worst.max(d);
        }
        assert!(
            worst >= 1,
            "no input differed from libm; is the kernel in use?"
        );
    }

    /// The residual without a fused multiply-add gives the same bits as with
    /// one, so a target without FMA gets the same results.
    #[test]
    fn dekker_residual_matches_the_fused_one() {
        for (y, x) in angle_pairs(1_000_000)
            .into_iter()
            .filter(|&(y, x)| atan2_covers(y, x))
        {
            let q = y / x;
            let (f, d) = (residual_fma(y, q, x), residual_dekker(y, q, x));
            assert_eq!(f.to_bits(), d.to_bits(), "({y:e}, {x:e}): {f:e} vs {d:e}");
        }
    }

    #[test]
    fn atan2_covers_rejects_zeros_non_finite_and_nan() {
        for (y, x) in [
            (0.0, 1.0),
            (-0.0, 1.0),
            (1.0, 0.0),
            (1.0, -0.0),
            (1.0, f64::INFINITY),
            (f64::NAN, 1.0),
            (1.0, f64::NEG_INFINITY),
            (f64::INFINITY, 1.0),
        ] {
            assert!(!atan2_covers(y, x), "({y}, {x})");
        }
        assert!(atan2_covers(1e-300, -1e300));
    }
}
