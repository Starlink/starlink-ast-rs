// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Inputs for testing the numerical kernels: deterministic random values,
//! realistic directions and angles, and the special values every kernel must
//! handle as the C it replaces does.
//!
//! Generating inputs is not a computation whose calls are counted, so this
//! uses the `f64` methods directly.
#![allow(clippy::disallowed_methods)]

use std::f64::consts::{FRAC_PI_2, PI};

use crate::constants::BAD_VALUE;

/// A small deterministic generator (SplitMix64), so test inputs are the same
/// on every run and platform without a dependency.
pub(crate) struct SplitMix64(u64);

impl SplitMix64 {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub(crate) fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

/// Values every kernel must treat as the C does: signed zeros, the extremes,
/// subnormals, `BAD_VALUE`, infinities, NaN, and the angles at quadrant
/// boundaries.
pub(crate) fn specials() -> Vec<f64> {
    let mut v = Vec::new();
    for x in [
        0.0,
        1.0,
        f64::MIN_POSITIVE,
        f64::MIN_POSITIVE / 1024.0,
        f64::MAX,
        f64::INFINITY,
        PI,
        FRAC_PI_2,
    ] {
        v.push(x);
        v.push(-x);
    }
    v.push(BAD_VALUE);
    v.push(f64::NAN);
    v
}

/// `n` random directions, unnormalized and at several magnitudes, then the
/// six axis directions with signed zeros in every combination.
pub(crate) fn unit_vectors(n: usize) -> Vec<[f64; 3]> {
    let mut rng = SplitMix64::new(0x5eed_0c25);
    let mut out = Vec::with_capacity(n + 64);
    for i in 0..n {
        let lon = rng.next_f64() * 2.0 * PI;
        let lat = (rng.next_f64() - 0.5) * PI;
        let scale = [1.0, 1e-3, 1e3, 7.25][i % 4];
        out.push([
            scale * lon.cos() * lat.cos(),
            scale * lon.sin() * lat.cos(),
            scale * lat.sin(),
        ]);
    }
    for axis in 0..3 {
        for one in [1.0, -1.0] {
            for zeros in 0..4 {
                let mut v = [0.0; 3];
                v[(axis + 1) % 3] = if zeros & 1 == 0 { 0.0 } else { -0.0 };
                v[(axis + 2) % 3] = if zeros & 2 == 0 { 0.0 } else { -0.0 };
                v[axis] = one;
                out.push(v);
            }
        }
    }
    out
}

/// `n` random (longitude, latitude) pairs in radians, then pairs whose
/// magnitudes span 2^-40 to 2^40 in every sign combination.
pub(crate) fn angle_pairs(n: usize) -> Vec<(f64, f64)> {
    let mut rng = SplitMix64::new(0x5eed_0a2c);
    let mut out = Vec::with_capacity(2 * n);
    for _ in 0..n {
        out.push((rng.next_f64() * 2.0 * PI, (rng.next_f64() - 0.5) * PI));
    }
    for i in 0..n {
        let a = (2.0f64).powf(rng.next_f64() * 80.0 - 40.0);
        let b = (2.0f64).powf(rng.next_f64() * 80.0 - 40.0);
        let (sa, sb) = match i % 4 {
            0 => (1.0, 1.0),
            1 => (-1.0, 1.0),
            2 => (1.0, -1.0),
            _ => (-1.0, -1.0),
        };
        out.push((sa * a, sb * b));
    }
    out
}

/// The distance between two doubles in units in the last place, or
/// `u64::MAX` when their signs differ or either is NaN.
pub(crate) fn ulps(a: f64, b: f64) -> u64 {
    if a.to_bits() == b.to_bits() {
        return 0;
    }
    if a.is_nan() || b.is_nan() || a.is_sign_negative() != b.is_sign_negative() {
        return u64::MAX;
    }
    a.to_bits().abs_diff(b.to_bits())
}

/// `n` random angles in degrees within ±720, then every multiple of 45
/// degrees in that range with each neighbor one ULP away, then
/// [`specials`].
pub(crate) fn degrees(n: usize) -> Vec<f64> {
    let mut rng = SplitMix64::new(0x5eed_de65);
    let mut out = Vec::with_capacity(n + 128);
    for _ in 0..n {
        out.push((rng.next_f64() * 2.0 - 1.0) * 720.0);
    }
    for k in -16..=16 {
        let a = 45.0 * f64::from(k);
        out.push(a);
        out.push(f64::from_bits(a.to_bits().wrapping_add(1)));
        out.push(f64::from_bits(a.to_bits().wrapping_sub(1)));
    }
    out.extend(specials());
    out
}

/// `n` random values in [−1.1, 1.1], then ±1, 0, ±(1 ± `WCSTRIG_TOL`/2) and
/// ±(1 ± 2·`WCSTRIG_TOL`), which straddle the tolerance with which the
/// inverse trigonometric functions snap to the ends of their ranges, then
/// [`specials`].
pub(crate) fn unit_range(n: usize) -> Vec<f64> {
    use crate::wcslib::trig::WCSTRIG_TOL;
    let mut rng = SplitMix64::new(0x5eed_a0c5);
    let mut out = Vec::with_capacity(n + 64);
    for _ in 0..n {
        out.push((rng.next_f64() * 2.0 - 1.0) * 1.1);
    }
    for s in [1.0, -1.0] {
        out.push(s);
        for d in [
            WCSTRIG_TOL / 2.0,
            -WCSTRIG_TOL / 2.0,
            2.0 * WCSTRIG_TOL,
            -2.0 * WCSTRIG_TOL,
        ] {
            out.push(s * (1.0 + d));
        }
    }
    out.push(0.0);
    out.extend(specials());
    out
}

/// a + b as a rounded sum and its exact rounding error (Knuth's two-sum).
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}

/// a + b for |a| >= |b|, as a rounded sum and its exact rounding error.
fn quick_two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    (s, b - (s - a))
}

/// a * b as a rounded product and its exact rounding error, by Dekker's
/// product, which is exact where no partial product overflows or underflows
/// and does not depend on fused multiply-adds.
fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let split = |v: f64| {
        let c = 134_217_729.0 * v; // 2^27 + 1
        let hi = c - (c - v);
        (hi, v - hi)
    };
    let p = a * b;
    let ((ah, al), (bh, bl)) = (split(a), split(b));
    (p, ((ah * bh - p) + ah * bl + al * bh) + al * bl)
}

/// A double-double value: `hi + lo` with |lo| at most half an ULP of `hi`.
type Dd = (f64, f64);

fn dd_add(a: Dd, b: Dd) -> Dd {
    let (s, e) = two_sum(a.0, b.0);
    quick_two_sum(s, e + (a.1 + b.1))
}

fn dd_mul(a: Dd, b: Dd) -> Dd {
    let (p, e) = two_prod(a.0, b.0);
    quick_two_sum(p, e + (a.0 * b.1 + a.1 * b.0))
}

fn dd_div(a: Dd, d: f64) -> Dd {
    let q = a.0 / d;
    let (p, e) = two_prod(q, d);
    quick_two_sum(q, (((a.0 - p) - e) + a.1) / d)
}

/// pi/2 as four doubles, each the rounding of what the ones before leave
/// (the first is `FRAC_PI_2`).
const PIO2_PARTS: [f64; 4] = [
    FRAC_PI_2,
    6.123233995736766e-17,
    -1.4973849048591698e-33,
    5.562271104316826e-50,
];

/// The sine and cosine of `x` as double-doubles, for |x| <= 2^30, accurate
/// to far better than an ULP of a double: `x` is reduced by the nearest
/// multiple `k` of pi/2 exactly, as a sum of exact products of `k` and the
/// parts of pi/2 accumulated without rounding error (Shewchuk's expansion
/// sum), and the reduced argument's sine and cosine are their Taylor series
/// in double-double arithmetic. Platform-independent: it uses no libm and no
/// fused multiply-add.
pub(crate) fn sin_cos_dd(x: f64) -> (Dd, Dd) {
    assert!(x.is_finite() && x.abs() <= 1073741824.0, "{x:e}");
    if x.abs() < 1.0 / 67_108_864.0 {
        // |x| < 2^-26: sin x rounds to x and cos x to 1, the next terms being
        // below a quarter of their ULP.
        return ((x, -x * x * x / 6.0), (1.0, -x * x / 2.0));
    }
    let k = (x * std::f64::consts::FRAC_2_PI).round();
    // The exact sum of x and every -k * part, as a non-overlapping
    // expansion in increasing magnitude.
    let mut partials: Vec<f64> = Vec::new();
    let mut terms = vec![x];
    for p in PIO2_PARTS {
        let (h, l) = two_prod(k, p);
        terms.extend([-h, -l]);
    }
    for mut t in terms {
        let mut kept = Vec::new();
        for &p in &partials {
            let (s, e) = two_sum(t, p);
            if e != 0.0 {
                kept.push(e);
            }
            t = s;
        }
        kept.push(t);
        partials = kept;
    }
    let r = partials
        .iter()
        .rev()
        .fold((0.0, 0.0), |acc, &p| dd_add(acc, (p, 0.0)));
    let r2 = dd_mul(r, r);
    let series = |first: Dd, start: f64| {
        let (mut sum, mut term, mut n) = (first, first, start);
        for _ in 0..40 {
            term = dd_div(dd_mul(term, r2), -(n + 1.0) * (n + 2.0));
            sum = dd_add(sum, term);
            n += 2.0;
            if term.0.abs() < sum.0.abs() * 1e-40 {
                break;
            }
        }
        sum
    };
    let s = series(r, 1.0);
    let c = series((1.0, 0.0), 0.0);
    let neg = |v: Dd| (-v.0, -v.1);
    match (k as i64).rem_euclid(4) {
        0 => (s, c),
        1 => (c, neg(s)),
        2 => (neg(s), neg(c)),
        _ => (neg(c), s),
    }
}

/// The sine and cosine of `x`, correctly rounded but at a few arguments
/// within a hair of a midpoint between two doubles; see [`sin_cos_dd`].
pub(crate) fn sin_cos_reference(x: f64) -> (f64, f64) {
    let (s, c) = sin_cos_dd(x);
    (dd_round(s), dd_round(c))
}

/// The arctangent of `y / x` in the quadrant of (x, y), correctly rounded but
/// at a few arguments within a hair of a midpoint between two doubles, for
/// finite non-zero arguments whose ratio is a normal number. Libm's value `t` is only a seed: the angle from
/// direction `t` to (x, y) is the arctangent of `(y cos t - x sin t) / (x
/// cos t + y sin t)`, computed from [`sin_cos_dd`] in double-double
/// arithmetic; that angle is within a few ULP of `t`, so its arctangent is
/// the ratio itself to far better than an ULP of the result, and `t` plus the
/// ratio is rounded once. The result does not depend on the platform's libm
/// unless libm is wrong by more than the ratio can correct.
pub(crate) fn atan2_reference(y: f64, x: f64) -> f64 {
    // The arctangent depends only on the ratio, so both arguments are scaled
    // by the power of two that brings the larger near 1, exactly unless the
    // smaller becomes subnormal, keeping the products in range.
    let m = y.abs().max(x.abs());
    let scale = 2.0f64.powi(-(m.log2().floor() as i32));
    let (y, x) = (y * scale, x * scale);
    atan2_from_seed(y, x, y.atan2(x))
}

/// [`atan2_reference`] from the seed `t`.
fn atan2_from_seed(y: f64, x: f64, t: f64) -> f64 {
    let (s, c) = sin_cos_dd(t);
    let neg = |v: Dd| (-v.0, -v.1);
    let num = dd_add(dd_mul((y, 0.0), c), neg(dd_mul((x, 0.0), s)));
    let den = dd_add(dd_mul((x, 0.0), c), dd_mul((y, 0.0), s));
    let ratio = num.0 / den.0;
    dd_round(dd_add((t, 0.0), (ratio, 0.0)))
}

/// Positions for the FK4 and FK5 conversions: [`angle_pairs`], then both
/// poles and the latitudes one 1e-15 radian inside them, at several
/// longitudes.
pub(crate) fn fk_positions(n: usize) -> Vec<(f64, f64)> {
    let mut out = angle_pairs(n);
    for d in [
        FRAC_PI_2,
        -FRAC_PI_2,
        FRAC_PI_2 - 1e-15,
        -(FRAC_PI_2 - 1e-15),
    ] {
        for r in [0.0, 1.0, 3.0, 6.0] {
            out.push((r, d));
        }
    }
    out
}

/// Besselian epochs (years) for the FK4 and FK5 conversions: far from J2000
/// and at the extremes, `BAD_VALUE` and NaN, then random epochs from 1800 to
/// 2200.
pub(crate) fn fk_epochs() -> Vec<f64> {
    let mut rng = SplitMix64::new(0xf4_5e90);
    let mut out = vec![
        1000.0,
        1900.0,
        1950.0,
        2000.0,
        2100.0,
        3000.0,
        BAD_VALUE,
        f64::NAN,
    ];
    for _ in 0..4 {
        out.push(1800.0 + 400.0 * rng.next_f64());
    }
    out
}

/// A double-double rounded to a double, keeping the sign of a zero `hi`
/// where `lo` is zero.
pub(crate) fn dd_round(v: Dd) -> f64 {
    if v.1 == 0.0 { v.0 } else { v.0 + v.1 }
}

/// The product of two double-doubles, rounded to a double. A product of the
/// high parts that underflows to zero is the result, with its sign: the low
/// parts cannot lift it to the least subnormal.
pub(crate) fn dd_product(a: Dd, b: Dd) -> f64 {
    let p = a.0 * b.0;
    if p == 0.0 { p } else { dd_round(dd_mul(a, b)) }
}

/// The next double above `x` (not for NaN or infinities).
pub(crate) fn up(x: f64) -> f64 {
    if x == 0.0 {
        f64::from_bits(1)
    } else if x > 0.0 {
        f64::from_bits(x.to_bits() + 1)
    } else {
        f64::from_bits(x.to_bits() - 1)
    }
}

/// The next double below `x` (not for NaN or infinities).
pub(crate) fn down(x: f64) -> f64 {
    -up(-x)
}

/// The MJD of the start of each leap-second table entry.
pub(crate) fn leap_second_mjds() -> Vec<f64> {
    crate::erfa::CHANGES
        .iter()
        .map(|&(iy, im, _)| crate::erfa::cal2jd(iy, im, 1).unwrap().0.1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`sin_cos_reference`] gives the correctly rounded sine and cosine,
    /// computed with 80-digit decimal arithmetic, at arguments that include
    /// the ends of `vmath::sin_cos`'s domain, doubles next to multiples of
    /// pi/2, where the reduction cancels, and the argument where macOS's
    /// x86-64 libm is 2.65 ULP from the sine.
    #[test]
    fn sin_cos_reference_is_correctly_rounded() {
        let cases: [(f64, u64, u64); 14] = [
            (1.0, 0x3feaed548f090cee, 0x3fe14a280fb5068c),
            (0.5, 0x3fdeaee8744b05f0, 0x3fec1528065b7d50),
            (-645296.1993231616, 0xbfdeca4990e2e33f, 0x3fec0da923e3f094),
            (-814073.5473789521, 0x3fec20c429930d03, 0x3fde8441193b7612),
            (1000000.0, 0xbfd6664b2568d867, 0x3fedf9df9906d32c),
            (-1000000.0, 0x3fd6664b2568d867, 0x3fedf9df9906d32c),
            (3e-09, 0x3e29c511dc3a41df, 0x3ff0000000000000),
            (7.0, 0x3fe50608c26d0a08, 0x3fe81ff79ed92017),
            (355.0, 0xbeff9bd0307d1de3, 0xbfefffffffc18e4c),
            (103993.0, 0xbef40efdf1eb8de7, 0x3fefffffffe6da77),
            (
                std::f64::consts::FRAC_PI_4,
                0x3fe6a09e667f3bcc,
                0x3fe6a09e667f3bcd,
            ),
            (FRAC_PI_2, 0x3ff0000000000000, 0x3c91a62633145c07),
            (PI, 0x3ca1a62633145c07, 0xbff0000000000000),
            (20.0 * PI, 0xbce60fafbfd97309, 0x3ff0000000000000),
        ];
        for (x, s, c) in cases {
            let (rs, rc) = sin_cos_reference(x);
            assert_eq!((rs.to_bits(), rc.to_bits()), (s, c), "{x:e}");
        }
    }

    /// [`atan2_reference`] gives the correctly rounded arctangent, computed
    /// with 90-digit decimal arithmetic, from libm's seed and from seeds
    /// several ULP away from it in either direction.
    #[test]
    fn atan2_reference_is_correctly_rounded() {
        let cases: [(f64, f64, u64); 8] = [
            (1.0, 2.0, 0x3fddac670561bb4f),
            (0.3, -0.7, 0x4005e4c36ca0118a),
            (-1e-05, 0.999, 0xbee4fe154ccb19c2),
            (0.999, 1e-07, 0x3ff921fb39655983),
            (-0.5, -0.5, 0xc002d97c7f3321d2),
            (2.0, 3.0, 0x3fe2d0ead6066395),
            (1e-10, -1.0, 0x400921fb5440bd7c),
            (
                std::f64::consts::FRAC_1_SQRT_2,
                0.7071067811865475,
                0x3fe921fb54442d19,
            ),
        ];
        for (y, x, bits) in cases {
            assert_eq!(atan2_reference(y, x).to_bits(), bits, "({y:e}, {x:e})");
            let exact = f64::from_bits(bits);
            for step in [-5i64, -1, 1, 5] {
                let seed = f64::from_bits(exact.to_bits().wrapping_add_signed(step));
                assert_eq!(
                    atan2_from_seed(y, x, seed).to_bits(),
                    bits,
                    "({y:e}, {x:e}) from {seed:e}"
                );
            }
        }
    }
}
