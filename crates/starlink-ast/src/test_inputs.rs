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
#[allow(dead_code)] // for the native trig and projection kernels' tests
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
#[allow(dead_code)] // for the native trig and projection kernels' tests
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
