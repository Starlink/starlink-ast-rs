// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// ====================================================
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Developed at SunPro, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this
// software is freely granted, provided that this notice
// is preserved.
// ====================================================
//
// Algorithm and constants from fdlibm's e_rem_pio2.c, k_sin.c and k_cos.c via
// musl src/math/__rem_pio2.c, __sin.c and __cos.c: the three-part reduction
// by pi/2 done in full for every argument, and the quadrant chosen by selects
// on a floating-point quadrant number rather than an integer.

// The constants are fdlibm's, written as fdlibm writes them so they can be
// checked against it digit for digit.
#![allow(clippy::excessive_precision, clippy::approx_constant)]

/// 1.5 * 2^52: adding and subtracting it rounds to the nearest integer.
const TOINT: f64 = 1.5 / f64::EPSILON;
const INVPIO2: f64 = 6.36619772367581382433e-01;
/// pi/2 in three parts of 33 bits, and what remains after the third.
const PIO2_1: f64 = 1.57079632673412561417e+00;
const PIO2_2: f64 = 6.07710050630396597660e-11;
const PIO2_3: f64 = 2.02226624871116645580e-21;
const PIO2_3T: f64 = 8.47842766036889956997e-32;

const S1: f64 = -1.66666666666666324348e-01;
const S2: f64 = 8.33333333332248946124e-03;
const S3: f64 = -1.98412698298579493134e-04;
const S4: f64 = 2.75573137070700676789e-06;
const S5: f64 = -2.50507602534068634195e-08;
const S6: f64 = 1.58969099521155010221e-10;

const C1: f64 = 4.16666666666666019037e-02;
const C2: f64 = -1.38888888888741095749e-03;
const C3: f64 = 2.48015872894767294178e-05;
const C4: f64 = -2.75573143513906633035e-07;
const C5: f64 = 2.08757232129817482790e-09;
const C6: f64 = -1.13596475577881948265e-11;

/// The largest argument [`sin_cos`] accepts: fdlibm's medium-size reduction,
/// whose quadrant number times each part of pi/2 is exact, holds up to
/// 2^20 * pi/2.
const MAX_ARG: f64 = 1.0e6;

/// a + b as a rounded sum and its exact rounding error (Knuth's two-sum).
#[inline(always)]
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}

/// sin(x + y) for |x + y| <= pi/4, y the low part of the reduced argument.
#[inline(always)]
fn k_sin(x: f64, y: f64) -> f64 {
    let z = x * x;
    let w = z * z;
    let r = S2 + z * (S3 + z * S4) + z * w * (S5 + z * S6);
    let v = z * x;
    x - ((z * (0.5 * y - v * r) - y) - v * S1)
}

/// cos(x + y) for |x + y| <= pi/4.
#[inline(always)]
fn k_cos(x: f64, y: f64) -> f64 {
    let z = x * x;
    let w = z * z;
    let r = z * (C1 + z * (C2 + z * C3)) + w * w * (C4 + z * (C5 + z * C6));
    let hz = 0.5 * z;
    let w = 1.0 - hz;
    w + (((1.0 - w) - hz) + (z * r - x * y))
}

/// (sin x, cos x) for non-zero |x| <= 1e6 (see [`sin_cos_covers`]); any
/// other input gives an unspecified value.
#[inline(always)]
pub(crate) fn sin_cos(x: f64) -> (f64, f64) {
    // The nearest multiple of pi/2, and x reduced by it in three parts.
    // fdlibm takes the second and third rounds only when the first cancels
    // heavily, and its third round relies on the second subtraction then
    // being exact. Taking every round for every argument, each subtraction's
    // rounding error is carried by an exact two-sum instead, which holds
    // whether or not the subtraction cancels. Each product of the quadrant
    // number and a 33-bit part of pi/2 is exact, and so is x minus the first.
    let fnum = x * INVPIO2 + TOINT - TOINT;
    let r1 = x - fnum * PIO2_1;
    let (r2, e2) = two_sum(r1, -(fnum * PIO2_2));
    let (r3, e3) = two_sum(r2, -(fnum * PIO2_3));
    let c = (e2 + e3) - fnum * PIO2_3T;
    let y0 = r3 + c;
    let y1 = (r3 - y0) + c;
    let (s, c) = (k_sin(y0, y1), k_cos(y0, y1));
    // The quadrant, fnum mod 4, as a float so the selects vectorize.
    let q = fnum - 4.0 * (fnum * 0.25).floor();
    let sin = if q == 0.0 {
        s
    } else if q == 1.0 {
        c
    } else if q == 2.0 {
        -s
    } else {
        -c
    };
    let cos = if q == 0.0 {
        c
    } else if q == 1.0 {
        -s
    } else if q == 2.0 {
        -c
    } else {
        s
    };
    (sin, cos)
}

/// Whether [`sin_cos`] is valid for `x`: non-zero, since the reduction does
/// not keep the sign of a zero, and |x| at most 1e6, which excludes
/// infinities and NaN.
#[inline(always)]
pub(crate) fn sin_cos_covers(x: f64) -> bool {
    x != 0.0 && x.abs() <= MAX_ARG
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_inputs::{angle_pairs, sin_cos_reference, specials, ulps};

    /// [`sin_cos`] is within 1 ULP of the exact sine and cosine, rounded,
    /// on every platform. It is compared with a reference computed in
    /// double-double arithmetic rather than with libm, which can itself be
    /// further from the exact values: macOS's on x86-64 is 2.65 ULP from
    /// the sine at -6.452961993231616e5.
    #[test]
    fn sin_cos_is_within_one_ulp_of_the_exact_value() {
        let mut xs: Vec<f64> = angle_pairs(500_000)
            .into_iter()
            .flat_map(|(a, b)| [a, b])
            .collect();
        xs.extend(specials());
        for k in -40i32..=40 {
            let q = f64::from(k) * std::f64::consts::FRAC_PI_4;
            for d in -2i64..=2 {
                xs.push(f64::from_bits(q.to_bits().wrapping_add_signed(d)));
            }
        }
        let mut differ = 0;
        for x in xs.into_iter().filter(|&x| sin_cos_covers(x)) {
            let (s, c) = sin_cos(x);
            let (es, ec) = sin_cos_reference(x);
            let (ds, dc) = (ulps(s, es), ulps(c, ec));
            assert!(
                ds <= 1 && dc <= 1,
                "sin_cos({x:e}) = ({s:e}, {c:e}), exact ({es:e}, {ec:e})"
            );
            differ += usize::from(ds + dc > 0);
        }
        // The kernel is not correctly rounded everywhere, so some differ.
        assert!(
            differ > 0,
            "no input differed from the reference; is the kernel in use?"
        );
    }

    #[test]
    fn sin_cos_covers_rejects_zero_large_non_finite_and_nan() {
        for x in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            1.0e7,
            -1.0e7,
            f64::MAX,
        ] {
            assert!(!sin_cos_covers(x), "{x}");
        }
        for x in [5.0e-324, -1.0e-300, 1.0e6, -1.0e6] {
            assert!(sin_cos_covers(x), "{x}");
        }
    }
}
