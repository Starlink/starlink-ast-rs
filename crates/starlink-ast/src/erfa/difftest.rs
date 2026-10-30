// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Declarations of the vendored C ERFA functions (`astEra`-prefixed) that the
//! ports' differential tests call, and the comparisons those tests make.

unsafe extern "C" {
    pub(super) fn astEraAnp(a: f64) -> f64;
    pub(super) fn astEraAnpm(a: f64) -> f64;
    pub(super) fn astEraPas(al: f64, ap: f64, bl: f64, bp: f64) -> f64;
    pub(super) fn astEraSeps(al: f64, ap: f64, bl: f64, bp: f64) -> f64;
    pub(super) fn astEraSepp(a: *const f64, b: *const f64) -> f64;
    pub(super) fn astEraPdp(a: *const f64, b: *const f64) -> f64;
    pub(super) fn astEraPm(p: *const f64) -> f64;
    pub(super) fn astEraPmp(a: *const f64, b: *const f64, amb: *mut f64);
    pub(super) fn astEraPn(p: *const f64, r: *mut f64, u: *mut f64);
    pub(super) fn astEraPpp(a: *const f64, b: *const f64, apb: *mut f64);
    pub(super) fn astEraPpsp(a: *const f64, s: f64, b: *const f64, apsb: *mut f64);
    pub(super) fn astEraPxp(a: *const f64, b: *const f64, axb: *mut f64);
    pub(super) fn astEraSxp(s: f64, p: *const f64, sp: *mut f64);
    pub(super) fn astEraPv2s(
        pv: *const [f64; 3],
        theta: *mut f64,
        phi: *mut f64,
        r: *mut f64,
        td: *mut f64,
        pd: *mut f64,
        rd: *mut f64,
    );
    pub(super) fn astEraS2pv(
        theta: f64,
        phi: f64,
        r: f64,
        td: f64,
        pd: f64,
        rd: f64,
        pv: *mut [f64; 3],
    );
    pub(super) fn astEraPvu(dt: f64, pv: *const [f64; 3], upv: *mut [f64; 3]);
    pub(super) fn astEraRx(phi: f64, r: *mut [f64; 3]);
    pub(super) fn astEraRy(theta: f64, r: *mut [f64; 3]);
    pub(super) fn astEraRz(psi: f64, r: *mut [f64; 3]);
    pub(super) fn astEraRxp(r: *const [f64; 3], p: *const f64, rp: *mut f64);
    pub(super) fn astEraRxpv(r: *const [f64; 3], pv: *const [f64; 3], rpv: *mut [f64; 3]);
    pub(super) fn astEraRxr(a: *const [f64; 3], b: *const [f64; 3], atb: *mut [f64; 3]);
    pub(super) fn astEraTr(r: *const [f64; 3], rt: *mut [f64; 3]);
    pub(super) fn astEraTrxp(r: *const [f64; 3], p: *const f64, trp: *mut f64);
    pub(super) fn astEraRv2m(w: *const f64, r: *mut [f64; 3]);
    pub(super) fn astEraRm2v(r: *const [f64; 3], w: *mut f64);
    pub(super) fn astEraCal2jd(iy: i32, im: i32, id: i32, djm0: *mut f64, djm: *mut f64) -> i32;
    pub(super) fn astEraJd2cal(
        dj1: f64,
        dj2: f64,
        iy: *mut i32,
        im: *mut i32,
        id: *mut i32,
        fd: *mut f64,
    ) -> i32;
    pub(super) fn astEraD2tf(ndp: i32, days: f64, sign: *mut std::ffi::c_char, ihmsf: *mut i32);
    pub(super) fn astEraTf2d(
        s: std::ffi::c_char,
        ihour: i32,
        imin: i32,
        sec: f64,
        days: *mut f64,
    ) -> i32;
    pub(super) fn astEraEpb(dj1: f64, dj2: f64) -> f64;
    pub(super) fn astEraEpb2jd(epb: f64, djm0: *mut f64, djm: *mut f64);
    pub(super) fn astEraEpj(dj1: f64, dj2: f64) -> f64;
    pub(super) fn astEraEpj2jd(epj: f64, djm0: *mut f64, djm: *mut f64);
    pub(super) fn astEraDtdb(date1: f64, date2: f64, ut: f64, elong: f64, u: f64, v: f64) -> f64;
    pub(super) fn astEraGd2gc(n: i32, elong: f64, phi: f64, height: f64, xyz: *mut f64) -> i32;
    pub(super) fn astEraGd2gce(
        a: f64,
        f: f64,
        elong: f64,
        phi: f64,
        height: f64,
        xyz: *mut f64,
    ) -> i32;
    pub(super) fn astEraGc2gd(
        n: i32,
        xyz: *const f64,
        elong: *mut f64,
        phi: *mut f64,
        height: *mut f64,
    ) -> i32;
    pub(super) fn astEraGc2gde(
        a: f64,
        f: f64,
        xyz: *const f64,
        elong: *mut f64,
        phi: *mut f64,
        height: *mut f64,
    ) -> i32;
    pub(super) fn astEraDat(iy: i32, im: i32, id: i32, fd: f64, deltat: *mut f64) -> i32;
}

/// Whether two values are the same bit for bit, any two NaNs counting as the
/// same: the sign and payload of a NaN from arithmetic depend on operand
/// order, which the C compiler may swap, and carry no meaning.
pub(crate) fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// [`same`] for every element of two vectors.
pub(crate) fn same3(a: &[f64; 3], b: &[f64; 3]) -> bool {
    (0..3).all(|k| same(a[k], b[k]))
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
    super::CHANGES
        .iter()
        .map(|&(iy, im, _)| super::cal2jd(iy, im, 1).unwrap().0.1)
        .collect()
}

/// MJDs for the calendar tests: a regular grid over 0 to 100,000 days, every
/// leap-second instant with one and two ULP either side, the days around
/// the range limits, and the values no date can be.
pub(crate) fn mjd_cases() -> Vec<f64> {
    let mut v: Vec<f64> = (0..270_271).map(|k| f64::from(k) * 0.37).collect();
    for m in leap_second_mjds() {
        for x in [m, m - 1.0, m + 1.0] {
            v.extend([x, up(x), down(x), up(up(x)), down(down(x))]);
        }
        // Just under the next midnight: the fraction of the day is within an
        // ULP of 1.0, where the day rounding decides the month.
        v.push(down(m));
    }
    v.extend([
        -68569.5 - 2400000.5,
        -68569.5 - 2400000.5 - 1e-9,
        1e9 - 2400000.5,
        1e9 - 2400000.5 + 1e-3,
        -1e9,
        1e9,
        2.0e9,
        0.0,
        -0.0,
        crate::constants::BAD_VALUE,
        1e300,
        -1e300,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MAX,
        f64::MIN_POSITIVE,
    ]);
    v
}

/// Whether C's `eraJd2cal` is defined on a two-part date: it is not when a
/// NaN sum reaches the `(long)` casts or a part is too large for a `long`,
/// where the result depends on the platform (aarch64 saturates, x86-64 gives
/// the most negative value).
pub(crate) fn jd2cal_defined(dj1: f64, dj2: f64) -> bool {
    let dj = dj1 + dj2;
    if dj.is_nan() {
        return false;
    }
    if !(-68569.5..=1e9).contains(&dj) {
        // Rejected before any cast.
        return true;
    }
    dj1.abs() < 9.2e18 && dj2.abs() < 9.2e18
}

/// Whether C's `eraD2tf` is defined: the `int` unit count `10^ndp` must not
/// overflow, and every field must be finite and fit an `int` for the `(int)`
/// casts.
pub(crate) fn d2tf_defined(ndp: i32, days: f64) -> bool {
    (-9..=9).contains(&ndp) && days.is_finite() && days.abs() < 1e7
}

/// Geodetic latitudes: random over plus and minus pi/2, the poles and the
/// points just short of them, the equator, and the special angles.
pub(crate) fn geodetic_latitudes(n: usize) -> Vec<f64> {
    use crate::test_inputs::{SplitMix64, specials};
    use std::f64::consts::FRAC_PI_2;
    let mut rng = SplitMix64::new(0x5eed_6e0d);
    let mut v: Vec<f64> = (0..n)
        .map(|_| (rng.next_f64() - 0.5) * 2.0 * FRAC_PI_2)
        .collect();
    for d in [0.0, 1e-15, 1e-12, 1e-9, 1e-6, 1e-3] {
        v.extend([FRAC_PI_2 - d, -(FRAC_PI_2 - d)]);
    }
    v.push(0.0);
    v.push(-0.0);
    v.extend(specials());
    v
}

/// Geodetic longitudes over plus and minus 2 pi, with the special angles.
pub(crate) fn geodetic_longitudes(n: usize) -> Vec<f64> {
    use crate::test_inputs::{SplitMix64, specials};
    let mut rng = SplitMix64::new(0x5eed_10a9);
    let mut v: Vec<f64> = (0..n)
        .map(|_| (rng.next_f64() - 0.5) * 4.0 * std::f64::consts::PI)
        .collect();
    v.extend(specials());
    v
}

/// Heights from -1e4 to 1e7 metres, and values no height should be.
pub(crate) fn geodetic_heights() -> Vec<f64> {
    vec![
        -1e4,
        -100.0,
        -1.0,
        0.0,
        -0.0,
        1.0,
        4205.0,
        1e4,
        1e5,
        1e6,
        1e7,
        f64::NAN,
        f64::INFINITY,
        crate::constants::BAD_VALUE,
    ]
}

/// Geocentric vectors in metres: the shared vectors scaled by 1e3 to 1e8,
/// points on the polar axis, in the equatorial plane, and the centre.
pub(crate) fn geocentric_vectors(n: usize) -> Vec<[f64; 3]> {
    use crate::test_inputs::vectors;
    let mut out = Vec::new();
    let vs = vectors(n);
    for (i, v) in vs.iter().enumerate() {
        let scale = 10f64.powi(3 + (i % 6) as i32);
        out.push([v[0] * scale, v[1] * scale, v[2] * scale]);
    }
    for scale in [1e3, 1e5, 6.356752e6, 6.378137e6, 1e7, 1e8] {
        for z in [scale, -scale, 0.0, -0.0] {
            out.push([0.0, 0.0, z]);
            out.push([-0.0, 0.0, z]);
        }
        out.push([scale, 0.0, 0.0]);
        out.push([0.0, -scale, 0.0]);
    }
    // Within the polar-case threshold of the axis, and just outside it.
    for p in [1e-17, 1e-12, 1e-9, 1e-6, 1e-3] {
        for z in [6.3e6, -6.3e6] {
            out.push([p, 0.0, z]);
            out.push([0.0, p, z]);
        }
    }
    out.push([0.0; 3]);
    out
}
