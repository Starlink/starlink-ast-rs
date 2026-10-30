// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

#![allow(clippy::disallowed_methods)]
// PAL's constants are transcribed from palmac.h digit for digit.
#![allow(clippy::excessive_precision, clippy::approx_constant)]

//! The crate's positional-astronomy routines, ported from PAL (Positional
//! Astronomy Library) to Rust, bit for bit.
//!
//! SlaMap, SkyFrame, SpecMap, TimeMap and FitsChan call the functions in
//! this module.
//! Each routine whose callers ask for one value of its kind (an epoch
//! conversion, a rotation matrix, a velocity) has its body in a file of its
//! own, `pal/<name>.rs`, which the function here counts and calls; the
//! bodies depend on the ERFA ports in `crate::erfa`, never on mappings,
//! frames or points.
//! Angles are in radians, dates are MJD, and 3-vectors and 3x3 matrices use
//! fixed-size arrays.
//!
//! This module is `pub(crate)`.
//! A status that C PAL returns as an integer is a typed result here: an error
//! is the `Err` of a per-function enum, and a warning that comes with a value
//! is an `Option` beside it.
//! `golden_tests` checks every routine against values captured from the C it
//! was ported from.

use crate::constants::BAD_VALUE;

mod addet;
mod ampqk;
mod dat;
mod deuler;
mod dmat;
mod drange;
mod ecmat;
mod eqgal;
mod etrms;
mod evp;
mod galeq;
mod galsup;
mod geoc;
#[cfg(test)]
mod golden_tests;
mod mappa;
mod mapqkz;
mod prebn;
mod prec;
mod prenut;
mod pvobs;
mod rvgalc;
mod rvlg;
mod rvlsrd;
mod rvlsrk;
mod subet;
mod supgal;

/// PAL's `PAL__DPI` (palmac.h): pi.
pub(crate) const PAL__DPI: f64 = 3.1415926535897932384626433832795028841971693993751;
/// PAL's `PAL__MJD0` (palmac.h): the zero point of the Modified Julian Date, as a
/// Julian Date.
pub(crate) const PAL__MJD0: f64 = 2400000.5;
/// PAL's `PAL__D2PI` (palmac.h): 2 pi.
pub(crate) const PAL__D2PI: f64 = 6.2831853071795864769252867665590057683943387987502;

/// PAL's `PAL__SR` (palmac.h): the Earth's rotation rate in radians per
/// second.
pub(crate) const PAL__SR: f64 = 7.2921150e-5;

/// PAL's `PAL__DAS2R` (palmac.h): arcseconds to radians.
pub(crate) const PAL__DAS2R: f64 = 4.8481368110953599358991410235794797595635330237270e-6;

/// PAL's `PAL__CR` (palmac.h): the light time for 1 au, in seconds.
pub(crate) const PAL__CR: f64 = 499.004782;

/// PAL's `PAL__SPD` (palmac.h): seconds per day.
pub(crate) const PAL__SPD: f64 = 86400.0;

// The astronomical unit in metres, the IAU 2012 value ERFA uses. PAL's routines
// convert metres to au with it, so a caller that converts their output back
// must use the same value.
pub(crate) use crate::erfa::ERFA_DAU;

// ---------------------------------------------------------------------------
// Group 1: Epoch conversions
// ---------------------------------------------------------------------------

/// Convert Modified Julian Date to Julian epoch.
pub(crate) fn epj(date: f64) -> f64 {
    // palEpj is eraEpj.
    crate::call_counts::hit("astPalEpj");
    crate::erfa::epj(PAL__MJD0, date)
}

/// Convert Julian epoch to Modified Julian Date.
pub(crate) fn epj2d(epj: f64) -> f64 {
    // palEpj2d is eraEpj2jd's MJD part.
    crate::call_counts::hit("astPalEpj2d");
    crate::erfa::epj2jd(epj).1
}

/// Convert Modified Julian Date to Besselian epoch.
pub(crate) fn epb(date: f64) -> f64 {
    // palEpb is eraEpb.
    crate::call_counts::hit("astPalEpb");
    crate::erfa::epb(PAL__MJD0, date)
}

/// Convert Besselian epoch to Modified Julian Date.
pub(crate) fn epb2d(epb: f64) -> f64 {
    // palEpb2d is eraEpb2jd's MJD part.
    crate::call_counts::hit("astPalEpb2d");
    crate::erfa::epb2jd(epb).1
}

// ---------------------------------------------------------------------------
// Group 2: Spherical <-> Cartesian
// ---------------------------------------------------------------------------

/// Spherical coordinates (longitude, latitude) to direction-cosine 3-vector.
pub(crate) fn dcs2c(a: f64, b: f64) -> [f64; 3] {
    // palDcs2c is eraS2c.
    crate::call_counts::hit("astPalDcs2c");
    crate::erfa::s2c(a, b)
}

/// Direction-cosine 3-vector to spherical coordinates (longitude, latitude).
///
/// The components are canonicalized so a negative zero becomes a positive
/// zero before `eraC2s`'s `atan2(y, x)`. A point on the −x meridian arrives
/// from the rotation pipeline with `y == ∓0.0` depending only on the
/// summation order of the matrix product; C's compressed-matrix arithmetic
/// yields `+0.0` there, so `atan2(+0, −1) = +π`. Without this, a `−0.0` from
/// our product order would give `−π` — the same physical point but the
/// opposite representation from C (e.g. a Mercator fiducial at RA = 180°).
/// `x + 0.0` is the identity for every value except `−0.0 → +0.0`.
pub(crate) fn dcc2s(v: &[f64; 3]) -> (f64, f64) {
    // palDcc2s is eraC2s.
    crate::call_counts::hit("astPalDcc2s");
    crate::erfa::c2s([v[0] + 0.0, v[1] + 0.0, v[2] + 0.0])
}

/// [`dcs2c`] for every (longitude, latitude) pair of `a` and `b`, into
/// `x`, `y` and `z`: the slice runs a vectorizable kernel whose sines and
/// cosines are each within 1 ULP of the exact value, rounded, and whose `x`
/// and `y` are within 8 ULP of the exact products, rounded; [`dcs2c`] takes
/// its sines and cosines from the platform libm. Each point gets the same
/// bits whatever the slice's length, and whichever build of the loop runs:
/// on x86-64 one for AVX2 and FMA when the CPU has them. A `BAD_VALUE`
/// coordinate gives `BAD_VALUE` outputs, without the cost of computing them.
///
/// Counts no calls: a caller that skips some points afterwards, as C skips
/// them before calling, counts the calls C makes with
/// [`crate::call_counts::hit_n`].
pub(crate) fn dcs2c_slice(a: &[f64], b: &[f64], x: &mut [f64], y: &mut [f64], z: &mut [f64]) {
    #[cfg(target_arch = "x86_64")]
    if crate::vmath::x86_has_avx2_fma() {
        // SAFETY: the CPU has AVX2 and FMA, which is all the function's
        // target features require.
        return unsafe { dcs2c_slice_avx2_fma(a, b, x, y, z) };
    }
    dcs2c_slice_body(a, b, x, y, z);
}

/// [`dcs2c_slice_body`] built for AVX2 and FMA, so its loop runs four wide.
///
/// # Safety
///
/// The CPU must have AVX2 and FMA. (An `unsafe fn` because the crate's MSRV,
/// 1.85, predates safe functions with target features.)
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn dcs2c_slice_avx2_fma(a: &[f64], b: &[f64], x: &mut [f64], y: &mut [f64], z: &mut [f64]) {
    dcs2c_slice_body(a, b, x, y, z);
}

#[inline(always)]
fn dcs2c_slice_body(a: &[f64], b: &[f64], x: &mut [f64], y: &mut [f64], z: &mut [f64]) {
    let n = a.len();
    assert!(b.len() == n && x.len() == n && y.len() == n && z.len() == n);
    // The branchless kernel over every point, so the loop vectorizes, then
    // ERFA's own arithmetic for the points outside the kernel's domain.
    for i in 0..n {
        [x[i], y[i], z[i]] = crate::erfa::s2c_kernel(a[i], b[i]);
    }
    for i in 0..n {
        if a[i] == BAD_VALUE || b[i] == BAD_VALUE {
            [x[i], y[i], z[i]] = [BAD_VALUE; 3];
        } else if crate::erfa::s2c_needs_special(a[i], b[i]) {
            [x[i], y[i], z[i]] = crate::erfa::s2c(a[i], b[i]);
        }
    }
}

/// [`dcc2s`] for every vector (`x[i]`, `y[i]`, `z[i]`), into `a` and `b`,
/// signed-zero canonicalization included, within 1 ULP of [`dcc2s`]: the
/// slice runs a vectorizable kernel. Each point gets the same bits whatever
/// the slice's length, and whichever build of the loop runs: on x86-64 one
/// for AVX2 and FMA when the CPU has them. A `BAD_VALUE` coordinate gives
/// `BAD_VALUE` outputs, as for [`dcs2c_slice`], which this also follows in
/// counting no calls.
pub(crate) fn dcc2s_slice(x: &[f64], y: &[f64], z: &[f64], a: &mut [f64], b: &mut [f64]) {
    #[cfg(target_arch = "x86_64")]
    if crate::vmath::x86_has_avx2_fma() {
        // SAFETY: the CPU has AVX2 and FMA, which is all the function's
        // target features require.
        return unsafe { dcc2s_slice_avx2_fma(x, y, z, a, b) };
    }
    dcc2s_slice_body::<{ crate::vmath::HAS_FMA }>(x, y, z, a, b);
}

/// [`dcc2s_slice_body`] built for AVX2 and FMA, so its loop runs four wide
/// and the arctangent's residual is one fused multiply-add.
///
/// # Safety
///
/// The CPU must have AVX2 and FMA. (An `unsafe fn` because the crate's MSRV,
/// 1.85, predates safe functions with target features.)
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn dcc2s_slice_avx2_fma(x: &[f64], y: &[f64], z: &[f64], a: &mut [f64], b: &mut [f64]) {
    dcc2s_slice_body::<true>(x, y, z, a, b);
}

#[inline(always)]
fn dcc2s_slice_body<const FMA: bool>(
    x: &[f64],
    y: &[f64],
    z: &[f64],
    a: &mut [f64],
    b: &mut [f64],
) {
    let n = x.len();
    assert!(y.len() == n && z.len() == n && a.len() == n && b.len() == n);
    // The branchless kernel and its domain test over every point, so the loop
    // vectorizes, marking with NaN each point outside the domain, where the
    // kernel never gives NaN; then ERFA's own arithmetic for those points.
    for i in 0..n {
        let p = [x[i] + 0.0, y[i] + 0.0, z[i] + 0.0];
        let (theta, phi) = crate::erfa::c2s_kernel::<FMA>(p);
        a[i] = if crate::erfa::c2s_needs_special(p) {
            f64::NAN
        } else {
            theta
        };
        b[i] = phi;
    }
    for i in 0..n {
        let p = [x[i] + 0.0, y[i] + 0.0, z[i] + 0.0];
        if p.contains(&BAD_VALUE) {
            (a[i], b[i]) = (BAD_VALUE, BAD_VALUE);
        } else if a[i].is_nan() {
            (a[i], b[i]) = crate::erfa::c2s(p);
        }
    }
}

// ---------------------------------------------------------------------------
// Group 3: Vector / matrix operations
// ---------------------------------------------------------------------------

/// Multiply a 3x3 matrix by a 3-vector (forward transformation).
pub(crate) fn dmxv(dm: &[[f64; 3]; 3], va: &[f64; 3]) -> [f64; 3] {
    // palDmxv is eraRxp.
    crate::call_counts::hit("astPalDmxv");
    crate::erfa::rxp(dm, va)
}

/// Multiply the inverse of a 3x3 matrix by a 3-vector (backward / transpose).
pub(crate) fn dimxv(dm: &[[f64; 3]; 3], va: &[f64; 3]) -> [f64; 3] {
    // palDimxv is eraTrxp.
    crate::call_counts::hit("astPalDimxv");
    crate::erfa::trxp(dm, va)
}

/// Multiply two 3x3 matrices: `c = a * b`.
pub(crate) fn dmxm(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    // palDmxm is eraRxr.
    crate::call_counts::hit("astPalDmxm");
    crate::erfa::rxr(a, b)
}

/// Cross product of two 3-vectors.
pub(crate) fn dvxv(va: &[f64; 3], vb: &[f64; 3]) -> [f64; 3] {
    // palDvxv is eraPxp.
    crate::call_counts::hit("astPalDvxv");
    crate::erfa::pxp(va, vb)
}

/// Dot product of two 3-vectors.
pub(crate) fn dvdv(va: &[f64; 3], vb: &[f64; 3]) -> f64 {
    // palDvdv is eraPdp.
    crate::call_counts::hit("astPalDvdv");
    crate::erfa::pdp(va, vb)
}

/// Normalize a 3-vector, returning (unit_vector, modulus).
pub(crate) fn dvn(v: &[f64; 3]) -> ([f64; 3], f64) {
    // palDvn is eraPn, which returns the modulus first.
    crate::call_counts::hit("astPalDvn");
    let (vm, uv) = crate::erfa::pn(v);
    (uv, vm)
}

/// Matrix inversion and linear-system solve, wrapping `palDmat`.
///
/// `a` is an `n×n` matrix in row-major order and `y` is the right-hand
/// side vector of length `n`. On success (returns `true`), `a` is replaced
/// in place by its inverse and `y` by the solution of `a·x = y` (for the
/// original `a`). Returns `false` when the matrix is singular
/// (`palDmat`'s `jf != 0`, i.e. a pivot magnitude below its internal
/// `SFA = 1e-20`); in that case `a` and `y` hold what `palDmat` leaves
/// in them, the rows it could pivot on having been reduced, which C's
/// fitschan.c `MakeIntWorld` uses regardless. `a.len()` must equal `n*n`
/// and `y.len()` must equal `n`.
pub(crate) fn dmat(n: usize, a: &mut [f64], y: &mut [f64]) -> bool {
    dmat_det(n, a, y).0
}

/// [`dmat`], also returning `palDmat`'s determinant of `a`, which is zero
/// when the matrix is singular.
pub(crate) fn dmat_det(n: usize, a: &mut [f64], y: &mut [f64]) -> (bool, f64) {
    debug_assert_eq!(a.len(), n * n, "matrix must be n*n");
    debug_assert_eq!(y.len(), n, "rhs must have length n");
    crate::call_counts::hit("astPalDmat");
    let (singular, d) = dmat::dmat(n, a, y);
    (!singular, d)
}

// ---------------------------------------------------------------------------
// Group 4: Coordinate conversions
// ---------------------------------------------------------------------------

/// Galactic (l, b) to FK5 J2000 (ra, dec).  All radians.
pub(crate) fn galeq(dl: f64, db: f64) -> (f64, f64) {
    crate::call_counts::hit("astPalGaleq");
    galeq::galeq(dl, db)
}

/// FK5 J2000 (ra, dec) to Galactic (l, b).  All radians.
pub(crate) fn eqgal(dr: f64, dd: f64) -> (f64, f64) {
    crate::call_counts::hit("astPalEqgal");
    eqgal::eqgal(dr, dd)
}

/// Galactic (l, b) to Supergalactic (sl, sb).  All radians.
pub(crate) fn galsup(dl: f64, db: f64) -> (f64, f64) {
    crate::call_counts::hit("astPalGalsup");
    galsup::galsup(dl, db)
}

/// Supergalactic (sl, sb) to Galactic (l, b).  All radians.
pub(crate) fn supgal(dsl: f64, dsb: f64) -> (f64, f64) {
    crate::call_counts::hit("astPalSupgal");
    supgal::supgal(dsl, dsb)
}

/// FK4 B1950 (ra, dec) to FK5 J2000 (ra, dec), assuming zero proper motion.
///
/// ERFA's `eraFk45z`, which the C library's SlaMap calls.
// Called only by tests (no mapping or frame uses it).
#[allow(dead_code)]
pub(crate) fn fk45z(r1950: f64, d1950: f64, bepoch: f64) -> (f64, f64) {
    crate::call_counts::hit("astEraFk45z");
    crate::erfa::fk45z(r1950, d1950, bepoch)
}

/// [`fk45z`] for every position (`r1950[i]`, `d1950[i]`) at the Besselian
/// epoch `bepoch`, into `r2000` and `d2000`: the slice runs a vectorizable
/// kernel whose sines, cosines and arctangents are each within 1 ULP of the
/// exact value, rounded, where [`fk45z`] takes them from the platform libm,
/// and gives the positions outside the kernel's domain to [`fk45z`]. Each
/// position gets the same bits whatever the slice's length, and whichever
/// build of the loop runs: on x86-64 one for AVX2 and FMA when the CPU has
/// them.
///
/// Counts no calls, as [`dcs2c_slice`] does.
pub(crate) fn fk45z_slice(
    bepoch: f64,
    r1950: &[f64],
    d1950: &[f64],
    r2000: &mut [f64],
    d2000: &mut [f64],
) {
    #[cfg(target_arch = "x86_64")]
    if crate::vmath::x86_has_avx2_fma() {
        // SAFETY: the CPU has AVX2 and FMA, which is all the function's
        // target features require.
        return unsafe { fk45z_slice_avx2_fma(bepoch, r1950, d1950, r2000, d2000) };
    }
    fk45z_slice_body::<{ crate::vmath::HAS_FMA }>(bepoch, r1950, d1950, r2000, d2000);
}

/// [`fk45z_slice_body`] built for AVX2 and FMA, so its loop runs four wide
/// and the arctangent's residual is one fused multiply-add.
///
/// # Safety
///
/// The CPU must have AVX2 and FMA. (An `unsafe fn` because the crate's MSRV,
/// 1.85, predates safe functions with target features.)
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn fk45z_slice_avx2_fma(
    bepoch: f64,
    r1950: &[f64],
    d1950: &[f64],
    r2000: &mut [f64],
    d2000: &mut [f64],
) {
    fk45z_slice_body::<true>(bepoch, r1950, d1950, r2000, d2000);
}

#[inline(always)]
fn fk45z_slice_body<const FMA: bool>(
    bepoch: f64,
    r1950: &[f64],
    d1950: &[f64],
    r2000: &mut [f64],
    d2000: &mut [f64],
) {
    let n = r1950.len();
    assert!(d1950.len() == n && r2000.len() == n && d2000.len() == n);
    let e = crate::erfa::Fk45zEpoch::new(bepoch);
    // The branchless kernel over every position, so the loop vectorizes,
    // marking with NaN each position outside its domain, where the kernel
    // never gives NaN; then ERFA's own arithmetic for those positions.
    for i in 0..n {
        let (r, d, special) = crate::erfa::fk45z_kernel::<FMA>(r1950[i], d1950[i], &e);
        r2000[i] = if special { f64::NAN } else { r };
        d2000[i] = d;
    }
    for i in 0..n {
        if r2000[i].is_nan() {
            (r2000[i], d2000[i]) = crate::erfa::fk45z(r1950[i], d1950[i], bepoch);
        }
    }
}

/// FK5 J2000 (ra, dec) to FK4 B1950 (ra, dec) plus proper-motion terms.
///
/// Returns `(r1950, d1950, dr1950, dd1950)`. ERFA's `eraFk54z`, which the C
/// library's SlaMap calls.
// Called only by tests (no mapping or frame uses it).
#[allow(dead_code)]
pub(crate) fn fk54z(r2000: f64, d2000: f64, bepoch: f64) -> (f64, f64, f64, f64) {
    crate::call_counts::hit("astEraFk54z");
    crate::erfa::fk54z(r2000, d2000, bepoch)
}

/// [`fk54z`]'s (R.A., Dec) for every position (`r2000[i]`, `d2000[i]`) at
/// the Besselian epoch `bepoch`, into `r1950` and `d1950`: the slice runs a
/// vectorizable kernel whose sines, cosines and arctangents are each within
/// 1 ULP of the exact value, rounded, where [`fk54z`] takes them from the
/// platform libm, and gives the positions outside the kernel's domain to
/// [`fk54z`]. Each position gets the same bits whatever the slice's length,
/// and whichever build of the loop runs: on x86-64 one for AVX2 and FMA when
/// the CPU has them.
///
/// Counts no calls, as [`dcs2c_slice`] does.
pub(crate) fn fk54z_slice(
    bepoch: f64,
    r2000: &[f64],
    d2000: &[f64],
    r1950: &mut [f64],
    d1950: &mut [f64],
) {
    #[cfg(target_arch = "x86_64")]
    if crate::vmath::x86_has_avx2_fma() {
        // SAFETY: the CPU has AVX2 and FMA, which is all the function's
        // target features require.
        return unsafe { fk54z_slice_avx2_fma(bepoch, r2000, d2000, r1950, d1950) };
    }
    fk54z_slice_body::<{ crate::vmath::HAS_FMA }>(bepoch, r2000, d2000, r1950, d1950);
}

/// [`fk54z_slice_body`] built for AVX2 and FMA, so its loop runs four wide
/// and the arctangent's residual is one fused multiply-add.
///
/// # Safety
///
/// The CPU must have AVX2 and FMA. (An `unsafe fn` because the crate's MSRV,
/// 1.85, predates safe functions with target features.)
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn fk54z_slice_avx2_fma(
    bepoch: f64,
    r2000: &[f64],
    d2000: &[f64],
    r1950: &mut [f64],
    d1950: &mut [f64],
) {
    fk54z_slice_body::<true>(bepoch, r2000, d2000, r1950, d1950);
}

#[inline(always)]
fn fk54z_slice_body<const FMA: bool>(
    bepoch: f64,
    r2000: &[f64],
    d2000: &[f64],
    r1950: &mut [f64],
    d1950: &mut [f64],
) {
    let n = r2000.len();
    assert!(d2000.len() == n && r1950.len() == n && d1950.len() == n);
    // As fk45z_slice_body.
    for i in 0..n {
        let (r, d, special) = crate::erfa::fk54z_kernel::<FMA>(r2000[i], d2000[i], bepoch);
        r1950[i] = if special { f64::NAN } else { r };
        d1950[i] = d;
    }
    for i in 0..n {
        if r1950[i].is_nan() {
            let (r, d, _, _) = crate::erfa::fk54z(r2000[i], d2000[i], bepoch);
            (r1950[i], d1950[i]) = (r, d);
        }
    }
}

/// FK5 J2000 to ICRS (Hipparcos frame), assuming zero proper motion.
pub(crate) fn fk5hz(r5: f64, d5: f64, epoch: f64) -> (f64, f64) {
    crate::call_counts::hit("astPalFk5hz");
    // palFk5hz converts the epoch to a Julian date, then calls eraFk5hz.
    let (date1, date2) = crate::erfa::epj2jd(epoch);
    crate::erfa::fk5hz(r5, d5, date1, date2)
}

/// ICRS (Hipparcos frame) to FK5 J2000, assuming zero proper motion.
///
/// Returns `(r5, d5, dr5, dd5)`.
pub(crate) fn hfk5z(rh: f64, dh: f64, epoch: f64) -> (f64, f64, f64, f64) {
    crate::call_counts::hit("astPalHfk5z");
    // palHfk5z converts the epoch to a Julian date, then calls eraHfk5z.
    let (date1, date2) = crate::erfa::epj2jd(epoch);
    crate::erfa::hfk5z(rh, dh, date1, date2)
}

// ---------------------------------------------------------------------------
// Group 5: Precession matrices
// ---------------------------------------------------------------------------

/// Julian (FK5) precession matrix from epoch `ep0` to `ep1` (Julian epochs).
pub(crate) fn prec(ep0: f64, ep1: f64) -> [[f64; 3]; 3] {
    crate::call_counts::hit("astPalPrec");
    prec::prec(ep0, ep1)
}

/// Besselian (FK4) precession matrix from epoch `bep0` to `bep1`.
pub(crate) fn prebn(bep0: f64, bep1: f64) -> [[f64; 3]; 3] {
    crate::call_counts::hit("astPalPrebn");
    prebn::prebn(bep0, bep1)
}

/// Equatorial to ecliptic rotation matrix for a given TDB date (MJD).
pub(crate) fn ecmat(date: f64) -> [[f64; 3]; 3] {
    crate::call_counts::hit("astPalEcmat");
    ecmat::ecmat(date)
}

// ---------------------------------------------------------------------------
// Group 6: Euler rotation
// ---------------------------------------------------------------------------

/// Form a rotation matrix from Euler angles.
///
/// `order` specifies the rotation axes as a 1-3 character string,
/// e.g. `"XYZ"`, `"ZYZ"`.  Each character selects the axis about which
/// the corresponding rotation is applied.
pub(crate) fn deuler(order: &str, phi: f64, theta: f64, psi: f64) -> [[f64; 3]; 3] {
    crate::call_counts::hit("astPalDeuler");
    deuler::deuler(order, phi, theta, psi)
}

// ---------------------------------------------------------------------------
// Group 7: Aberration
// ---------------------------------------------------------------------------

/// Add E-terms of aberration.
///
/// Returns `(rc, dc)` — the apparent (ra, dec) with E-terms added.
pub(crate) fn addet(rm: f64, dm: f64, eq: f64) -> (f64, f64) {
    crate::call_counts::hit("astPalAddet");
    addet::addet(rm, dm, eq)
}

/// Remove E-terms of aberration.
///
/// Returns `(rm, dm)` — the mean (ra, dec) with E-terms removed.
pub(crate) fn subet(rc: f64, dc: f64, eq: f64) -> (f64, f64) {
    crate::call_counts::hit("astPalSubet");
    subet::subet(rc, dc, eq)
}

/// Compute mean-to-apparent parameters for a given equinox and date.
///
/// Returns a 21-element array of star-independent parameters used by
/// [`ampqk`] and [`mapqkz`].
pub(crate) fn mappa(eq: f64, date: f64) -> [f64; 21] {
    crate::call_counts::hit("astPalMappa");
    mappa::mappa(eq, date)
}

/// Quick apparent (ra, dec) to mean (ra, dec).
///
/// Uses pre-computed parameters from [`mappa`].
pub(crate) fn ampqk(ra: f64, da: f64, amprms: &[f64; 21]) -> (f64, f64) {
    crate::call_counts::hit("astPalAmpqk");
    ampqk::ampqk(ra, da, amprms)
}

/// Quick mean (ra, dec) to apparent (ra, dec), assuming zero proper motion.
///
/// Uses pre-computed parameters from [`mappa`].
pub(crate) fn mapqkz(rm: f64, dm: f64, amprms: &[f64; 21]) -> (f64, f64) {
    crate::call_counts::hit("astPalMapqkz");
    mapqkz::mapqkz(rm, dm, amprms)
}

// ---------------------------------------------------------------------------
// Group 8: Angular normalization
// ---------------------------------------------------------------------------

/// Normalize an angle to the range [-pi, pi].
pub(crate) fn drange(angle: f64) -> f64 {
    crate::call_counts::hit("astPalDrange");
    drange::drange(angle)
}

/// Normalize an angle to the range [0, 2*pi].
pub(crate) fn dranrm(angle: f64) -> f64 {
    // palDranrm is eraAnp.
    crate::call_counts::hit("astPalDranrm");
    crate::erfa::anp(angle)
}

/// Angular separation between two points on a sphere (radians).
pub(crate) fn dsep(al: f64, ap: f64, bl: f64, bp: f64) -> f64 {
    // palDsep is eraSeps.
    crate::call_counts::hit("astPalDsep");
    crate::erfa::seps(al, ap, bl, bp)
}

/// Bearing (position angle) of point B as seen from point A (radians).
pub(crate) fn dbear(a1: f64, b1: f64, a2: f64, b2: f64) -> f64 {
    // palDbear is eraPas.
    crate::call_counts::hit("astPalDbear");
    crate::erfa::pas(a1, b1, a2, b2)
}

// ---------------------------------------------------------------------------
// Group 9: Earth / observer
// ---------------------------------------------------------------------------

/// Earth velocity and position (barycentric and heliocentric).
///
/// Returns `(dvb, dpb, dvh, dph)` — four 3-vectors:
/// - `dvb`: barycentric velocity (AU/s)
/// - `dpb`: barycentric position (AU)
/// - `dvh`: heliocentric velocity (AU/s)
/// - `dph`: heliocentric position (AU)
pub(crate) fn evp(date: f64, deqx: f64) -> ([f64; 3], [f64; 3], [f64; 3], [f64; 3]) {
    crate::call_counts::hit("astPalEvp");
    evp::evp(date, deqx)
}

/// Geodetic latitude/height to geocentric distance from spin axis and
/// distance from equatorial plane.
///
/// - `p`: geodetic latitude (radians)
/// - `h`: height above reference spheroid (meters)
///
/// Returns `(r, z)` in meters.
pub(crate) fn geoc(p: f64, h: f64) -> (f64, f64) {
    crate::call_counts::hit("astPalGeoc");
    geoc::geoc(p, h)
}

// ---------------------------------------------------------------------------
// Group 10: Miscellaneous
// ---------------------------------------------------------------------------

/// Axial vector to rotation matrix.
///
/// The direction of the axial vector is the Euler axis, and its
/// magnitude is the rotation angle in radians.
pub(crate) fn dav2m(axvec: &[f64; 3]) -> [[f64; 3]; 3] {
    // palDav2m is eraRv2m.
    crate::call_counts::hit("astPalDav2m");
    crate::erfa::rv2m(axvec)
}

// ---------------------------------------------------------------------------
// Group 11: Time / sidereal
// ---------------------------------------------------------------------------

/// TAI-UTC leap seconds for a given UTC date (MJD).
// Called only by tests (no mapping or frame uses it).
#[allow(dead_code)]
pub(crate) fn dat(mjd: f64) -> f64 {
    crate::call_counts::hit("astPalDat");
    dat::dat(mjd)
}

/// Greenwich Mean Sidereal Time (radians) for a given UT1 date (MJD).
pub(crate) fn gmst(ut1: f64) -> f64 {
    // palGmst is eraGmst06 with UT1 standing in for TT.
    crate::call_counts::hit("astPalGmst");
    crate::erfa::gmst06(PAL__MJD0, ut1, PAL__MJD0, ut1)
}

/// Equation of the equinoxes (radians) for a given TDB date (MJD).
pub(crate) fn eqeqx(date: f64) -> f64 {
    // palEqeqx is eraEe06a.
    crate::call_counts::hit("astPalEqeqx");
    crate::erfa::ee06a(PAL__MJD0, date)
}

// ---------------------------------------------------------------------------
// Group 12: Observer / velocity
// ---------------------------------------------------------------------------

/// Convert geodetic position (longitude, latitude, altitude) to a
/// geocentric Cartesian XYZ position vector in meters. Mirrors
/// `eraGd2gc(1, ...)` (WGS84 reference ellipsoid).
///
/// - `lon`: geodetic longitude (radians, east-positive)
/// - `lat`: geodetic latitude (radians)
/// - `height`: altitude above reference ellipsoid (meters)
///
/// Returns the 3-vector `[x, y, z]` in meters, or `None` if the ERFA
/// routine signaled an error (e.g. latitude out of range).
pub(crate) fn gd2gc(lon: f64, lat: f64, height: f64) -> Option<[f64; 3]> {
    crate::call_counts::hit("astEraGd2gc");
    crate::erfa::gd2gc(crate::erfa::Ellipsoid::Wgs84, lon, lat, height).ok()
}

/// Geocentric Cartesian → geodetic longitude/latitude/height. Mirrors
/// `eraGc2gd(1, ...)` (WGS84 reference ellipsoid).
///
/// Returns `Some((lon_radians, lat_radians, height_metres))` or
/// `None` on error.
pub(crate) fn gc2gd(xyz: [f64; 3]) -> Option<(f64, f64, f64)> {
    crate::call_counts::hit("astEraGc2gd");
    crate::erfa::gc2gd(crate::erfa::Ellipsoid::Wgs84, &xyz).ok()
}

/// Observer position and velocity as a 6-vector.
///
/// - `lat`: geodetic latitude (radians)
/// - `alt`: altitude above reference spheroid (meters)
/// - `lst`: local apparent sidereal time (radians)
///
/// Returns a 6-element array: [x, y, z, xdot, ydot, zdot] in AU and AU/s.
pub(crate) fn pvobs(lat: f64, alt: f64, lst: f64) -> [f64; 6] {
    crate::call_counts::hit("astPalPvobs");
    pvobs::pvobs(lat, alt, lst)
}

/// Component of galactic rotation velocity toward the source (km/s).
///
/// - `ra`: J2000 right ascension (radians)
/// - `dec`: J2000 declination (radians)
pub(crate) fn rvgalc(ra: f64, dec: f64) -> f64 {
    crate::call_counts::hit("astPalRvgalc");
    rvgalc::rvgalc(ra, dec)
}

/// Velocity component of the Local Group motion toward the source (km/s).
///
/// - `ra`: J2000 right ascension (radians)
/// - `dec`: J2000 declination (radians)
pub(crate) fn rvlg(ra: f64, dec: f64) -> f64 {
    crate::call_counts::hit("astPalRvlg");
    rvlg::rvlg(ra, dec)
}

/// Velocity component of the dynamical LSR toward the source (km/s).
///
/// - `ra`: J2000 right ascension (radians)
/// - `dec`: J2000 declination (radians)
pub(crate) fn rvlsrd(ra: f64, dec: f64) -> f64 {
    crate::call_counts::hit("astPalRvlsrd");
    rvlsrd::rvlsrd(ra, dec)
}

/// Velocity component of the kinematic LSR toward the source (km/s).
///
/// - `ra`: J2000 right ascension (radians)
/// - `dec`: J2000 declination (radians)
pub(crate) fn rvlsrk(ra: f64, dec: f64) -> f64 {
    crate::call_counts::hit("astPalRvlsrk");
    rvlsrk::rvlsrk(ra, dec)
}

// ---------------------------------------------------------------------------
// Group 13: Calendar / time-of-day conversions
// ---------------------------------------------------------------------------

/// Modified Julian Date to Gregorian calendar fields.
///
/// Returns `(year, month, day, day_fraction)`, or the error when `djm` is
/// outside the range the conversion supports. A NaN date is accepted, as
/// in ERFA, and gives a NaN fraction.
pub(crate) fn djcl(djm: f64) -> Result<(i32, i32, i32, f64), crate::erfa::Jd2calError> {
    // palDjcl is eraJd2cal with the MJD zero point.
    crate::call_counts::hit("astPalDjcl");
    crate::erfa::jd2cal(PAL__MJD0, djm)
}

/// Gregorian calendar date to Modified Julian Date.
///
/// Returns the MJD, with the warning [`crate::erfa::Cal2jdWarning`] when the day is
/// not a day of that month, or the error for an invalid year or month.
pub(crate) fn cldj(
    iy: i32,
    im: i32,
    id: i32,
) -> Result<(f64, Option<crate::erfa::Cal2jdWarning>), crate::erfa::Cal2jdError> {
    // palCldj is eraCal2jd's MJD part.
    crate::call_counts::hit("astPalCldj");
    let ((_, djm), warning) = crate::erfa::cal2jd(iy, im, id)?;
    Ok((djm, warning))
}

/// Day fraction to hours, minutes, seconds.
///
/// `ndp` is the number of decimal places of seconds. Returns
/// `(sign, [hours, minutes, seconds, fraction])` where `sign` is `'+'` or
/// `'-'` and the fraction is the `ndp`-digit fractional seconds field as an
/// integer.
pub(crate) fn dd2tf(ndp: i32, days: f64) -> (char, [i32; 4]) {
    // palDd2tf is eraD2tf.
    crate::call_counts::hit("astPalDd2tf");
    crate::erfa::d2tf(ndp, days)
}

/// Hours, minutes, seconds to a day fraction.
///
/// Returns the days, with the warning for the first out-of-range field.
pub(crate) fn dtf2d(ihour: i32, imin: i32, sec: f64) -> (f64, Option<crate::erfa::Tf2dWarning>) {
    // palDtf2d is eraTf2d with a blank sign.
    crate::call_counts::hit("astPalDtf2d");
    crate::erfa::tf2d(' ', ihour, imin, sec)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_inputs::{angle_pairs, specials, unit_vectors};
    use std::f64::consts::{FRAC_PI_2, PI};

    /// Directions and special-value triples, interleaved so special values
    /// sit among ordinary ones.
    fn mixed_vectors() -> Vec<[f64; 3]> {
        let s = specials();
        let mut out = Vec::new();
        for (i, v) in unit_vectors(997).into_iter().enumerate() {
            out.push(v);
            out.push([s[i % s.len()], s[(i / 3) % s.len()], s[(i / 7) % s.len()]]);
        }
        out.push([-1.0, -0.0, 0.0]);
        // Components whose arctangent quotients are tiny or subnormal, or
        // whose magnitudes are near the top of the range, where the
        // arctangent kernel's two builds would differ.
        for (i, &tiny) in [1e-300, 1e-305, 1e-310, 3e-320, 5e-324].iter().enumerate() {
            for &x in &[1.0, -1.0, 0.75] {
                out.push(unit_vectors(997)[i * 7]);
                out.push([x, tiny, 0.5]);
                out.push([x, 0.5, tiny]);
                out.push([x, -tiny, -tiny]);
                out.push([tiny, x, 1e-300]);
            }
        }
        for &big in &[1e300, -1e302, 1.7e308] {
            out.push([big, 1.0, 1.0]);
            out.push([1.0, big, 0.5]);
            out.push([big, big / 3.0, 1e-10]);
        }
        out
    }

    /// `dcc2s_slice` over `v`, one point per row.
    fn slice_c2s(v: &[[f64; 3]]) -> Vec<(f64, f64)> {
        let x: Vec<f64> = v.iter().map(|p| p[0]).collect();
        let y: Vec<f64> = v.iter().map(|p| p[1]).collect();
        let z: Vec<f64> = v.iter().map(|p| p[2]).collect();
        let (mut a, mut b) = (vec![0.0; v.len()], vec![0.0; v.len()]);
        dcc2s_slice(&x, &y, &z, &mut a, &mut b);
        a.into_iter().zip(b).collect()
    }

    #[test]
    fn dcc2s_slice_gives_each_point_the_same_bits_at_any_length() {
        let all = mixed_vectors();
        let whole = slice_c2s(&all);
        for n in [1, 2, 3, 7, 1001] {
            let start = all.len() - n;
            for (i, r) in slice_c2s(&all[start..]).into_iter().enumerate() {
                let w = whole[start + i];
                assert_eq!(
                    (r.0.to_bits(), r.1.to_bits()),
                    (w.0.to_bits(), w.1.to_bits()),
                    "{:?}",
                    all[start + i]
                );
            }
        }
    }

    #[test]
    fn dcc2s_slice_is_within_one_ulp_of_dcc2s() {
        let all = mixed_vectors();
        for (p, (a, b)) in all.iter().zip(slice_c2s(&all)) {
            if p.contains(&crate::constants::BAD_VALUE) {
                continue;
            }
            let (ea, eb) = dcc2s(p);
            assert!(
                crate::test_inputs::ulps(a, ea) <= 1 && crate::test_inputs::ulps(b, eb) <= 1,
                "{p:?}: ({a:e}, {b:e}) vs ({ea:e}, {eb:e})"
            );
        }
    }

    /// `dcs2c_slice` over `v`, one point per row.
    fn slice_s2c(v: &[(f64, f64)]) -> Vec<[f64; 3]> {
        let a: Vec<f64> = v.iter().map(|p| p.0).collect();
        let b: Vec<f64> = v.iter().map(|p| p.1).collect();
        let n = v.len();
        let (mut x, mut y, mut z) = (vec![0.0; n], vec![0.0; n], vec![0.0; n]);
        dcs2c_slice(&a, &b, &mut x, &mut y, &mut z);
        (0..n).map(|i| [x[i], y[i], z[i]]).collect()
    }

    /// Angle pairs and special-value pairs, interleaved.
    fn mixed_angles() -> Vec<(f64, f64)> {
        let s = specials();
        let mut all = Vec::new();
        for (i, p) in angle_pairs(500).into_iter().enumerate() {
            all.push(p);
            all.push((s[i % s.len()], s[(i / 5) % s.len()]));
        }
        all
    }

    /// Every build of the slice loops gives the same bits: with and without
    /// a fused multiply-add in the arctangent everywhere, and on an x86-64 CPU
    /// with AVX2 and FMA, the copies built for them against the baseline.
    #[test]
    fn every_build_of_the_slice_loops_gives_the_same_bits() {
        let v = mixed_vectors();
        let (x, y, z): (Vec<f64>, Vec<f64>, Vec<f64>) = (
            v.iter().map(|p| p[0]).collect(),
            v.iter().map(|p| p[1]).collect(),
            v.iter().map(|p| p[2]).collect(),
        );
        let n = v.len();
        let c2s_with = |run: &dyn Fn(&mut [f64], &mut [f64])| {
            let (mut a, mut b) = (vec![0.0; n], vec![0.0; n]);
            run(&mut a, &mut b);
            a.iter()
                .chain(&b)
                .map(|f| f.to_bits())
                .collect::<Vec<u64>>()
        };
        let unfused = c2s_with(&|a, b| dcc2s_slice_body::<false>(&x, &y, &z, a, b));
        let fused = c2s_with(&|a, b| dcc2s_slice_body::<true>(&x, &y, &z, a, b));
        assert_eq!(fused, unfused, "dcc2s with and without FMA");

        #[cfg(target_arch = "x86_64")]
        if crate::vmath::x86_has_avx2_fma() {
            // SAFETY: the CPU has AVX2 and FMA, checked just now.
            let wide = c2s_with(&|a, b| unsafe { dcc2s_slice_avx2_fma(&x, &y, &z, a, b) });
            assert_eq!(wide, unfused, "dcc2s built for AVX2 and FMA");

            let angles = mixed_angles();
            let (a, b): (Vec<f64>, Vec<f64>) = angles.iter().copied().unzip();
            let m = angles.len();
            let s2c_with = |run: &dyn Fn(&mut [f64], &mut [f64], &mut [f64])| {
                let (mut x, mut y, mut z) = (vec![0.0; m], vec![0.0; m], vec![0.0; m]);
                run(&mut x, &mut y, &mut z);
                x.iter()
                    .chain(&y)
                    .chain(&z)
                    .map(|f| f.to_bits())
                    .collect::<Vec<u64>>()
            };
            let base = s2c_with(&|x, y, z| dcs2c_slice_body(&a, &b, x, y, z));
            // SAFETY: as above.
            let wide = s2c_with(&|x, y, z| unsafe { dcs2c_slice_avx2_fma(&a, &b, x, y, z) });
            assert_eq!(wide, base, "dcs2c built for AVX2 and FMA");
        }
    }

    #[test]
    fn slices_give_bad_outputs_for_a_bad_coordinate() {
        use crate::constants::BAD_VALUE;
        let (mut a, mut b) = ([0.0; 3], [0.0; 3]);
        dcc2s_slice(
            &[1.0, BAD_VALUE, 1.0],
            &[2.0, 0.5, 2.0],
            &[3.0, 0.5, BAD_VALUE],
            &mut a,
            &mut b,
        );
        assert_eq!(
            (a[1], b[1], a[2], b[2]),
            (BAD_VALUE, BAD_VALUE, BAD_VALUE, BAD_VALUE)
        );
        assert_eq!((a[0], b[0]), dcc2s(&[1.0, 2.0, 3.0]));
        let (mut x, mut y, mut z) = ([0.0; 2], [0.0; 2], [0.0; 2]);
        dcs2c_slice(
            &[BAD_VALUE, 0.5],
            &[0.25, BAD_VALUE],
            &mut x,
            &mut y,
            &mut z,
        );
        assert_eq!([x, y, z], [[BAD_VALUE; 2]; 3]);
    }

    #[test]
    fn dcs2c_slice_gives_each_point_the_same_bits_at_any_length() {
        let all = mixed_angles();
        let whole = slice_s2c(&all);
        for n in [1, 2, 3, 7, 1001] {
            let start = all.len() - n;
            for (i, r) in slice_s2c(&all[start..]).into_iter().enumerate() {
                assert_eq!(
                    r.map(f64::to_bits),
                    whole[start + i].map(f64::to_bits),
                    "{:?}",
                    all[start + i]
                );
            }
        }
    }

    /// Where the kernel covers a pair, the slice is within the kernel's
    /// bound of the exact direction cosines, rounded; elsewhere it is
    /// [`dcs2c`] bit for bit.
    #[test]
    fn dcs2c_slice_is_within_the_s2c_kernel_bound_of_the_exact_value() {
        let all = mixed_angles();
        let bad = crate::constants::BAD_VALUE;
        let same = |a: f64, b: f64| a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan());
        for (p, r) in all.iter().zip(slice_s2c(&all)) {
            if p.0 == bad || p.1 == bad {
                continue;
            }
            if crate::erfa::s2c_needs_special(p.0, p.1) {
                let e = dcs2c(p.0, p.1);
                assert!((0..3).all(|k| same(r[k], e[k])), "{p:?}: {r:?} vs {e:?}");
                continue;
            }
            let e = crate::erfa::s2c_reference(p.0, p.1);
            for k in 0..3 {
                let d = crate::test_inputs::ulps(r[k], e[k]);
                let bound = if k == 2 {
                    1
                } else {
                    crate::erfa::S2C_PRODUCT_ULPS
                };
                assert!(d <= bound, "{p:?} axis {k}: {:e} vs {:e}", r[k], e[k]);
            }
        }
    }

    /// An FK slice entry: epoch, input longitudes and latitudes, output
    /// longitudes and latitudes.
    type FkSliceFn = fn(f64, &[f64], &[f64], &mut [f64], &mut [f64]);

    /// FK test positions with special values interleaved among them.
    fn mixed_fk_positions() -> Vec<(f64, f64)> {
        let s = specials();
        let mut all = Vec::new();
        for (i, p) in crate::test_inputs::fk_positions(500)
            .into_iter()
            .enumerate()
        {
            all.push(p);
            if i % 3 == 0 {
                all.push((s[i % s.len()], s[(i / 5) % s.len()]));
            }
        }
        all
    }

    /// An FK slice entry over `v` at `bepoch`, one position per row.
    fn through_fk_slice(slice: FkSliceFn, bepoch: f64, v: &[(f64, f64)]) -> Vec<(u64, u64)> {
        let (a, b): (Vec<f64>, Vec<f64>) = v.iter().copied().unzip();
        let (mut c, mut d) = (vec![0.0; v.len()], vec![0.0; v.len()]);
        slice(bepoch, &a, &b, &mut c, &mut d);
        c.into_iter()
            .zip(d)
            .map(|(c, d)| (c.to_bits(), d.to_bits()))
            .collect()
    }

    /// Each position gets the same bits from an FK slice entry, at every test
    /// epoch, as its own one-element slice and as part of slices of every
    /// length from 1 to 17 and of 1,000.
    fn assert_fk_slice_same_bits_whatever_its_length(slice: FkSliceFn) {
        let all = mixed_fk_positions();
        for bepoch in crate::test_inputs::fk_epochs() {
            let alone: Vec<(u64, u64)> = all
                .iter()
                .map(|&p| through_fk_slice(slice, bepoch, &[p])[0])
                .collect();
            for len in (1..=17).chain([1000]) {
                for (c, chunk) in all.chunks(len).enumerate() {
                    for (i, got) in through_fk_slice(slice, bepoch, chunk)
                        .into_iter()
                        .enumerate()
                    {
                        assert_eq!(
                            got,
                            alone[c * len + i],
                            "length {len} at {bepoch}: {:?}",
                            chunk[i]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn fk45z_slice_gives_each_element_the_same_bits_whatever_its_length() {
        assert_fk_slice_same_bits_whatever_its_length(fk45z_slice);
    }

    /// Where the kernel declines a position, [`fk45z_slice`] gives
    /// [`fk45z`]'s bits; the kernel's own bound is tested with it.
    #[test]
    fn fk45z_slice_gives_fk45z_outside_the_kernel_domain() {
        let all = mixed_fk_positions();
        for bepoch in crate::test_inputs::fk_epochs() {
            let e = crate::erfa::Fk45zEpoch::new(bepoch);
            let got = through_fk_slice(fk45z_slice, bepoch, &all);
            let mut declined = 0;
            for (&(r, d), g) in all.iter().zip(got) {
                if crate::erfa::fk45z_kernel::<{ crate::vmath::HAS_FMA }>(r, d, &e).2 {
                    let (er, ed) = crate::erfa::fk45z(r, d, bepoch);
                    assert_eq!(
                        g,
                        (er.to_bits(), ed.to_bits()),
                        "({r:e}, {d:e}) at {bepoch}"
                    );
                    declined += 1;
                }
            }
            assert!(declined > 0, "no position declined at {bepoch}");
        }
    }

    #[test]
    fn fk54z_slice_gives_each_element_the_same_bits_whatever_its_length() {
        assert_fk_slice_same_bits_whatever_its_length(fk54z_slice);
    }

    /// Where the kernel declines a position, [`fk54z_slice`] gives
    /// [`fk54z`]'s bits; the kernel's own bound is tested with it.
    #[test]
    fn fk54z_slice_gives_fk54z_outside_the_kernel_domain() {
        let all = mixed_fk_positions();
        for bepoch in crate::test_inputs::fk_epochs() {
            let got = through_fk_slice(fk54z_slice, bepoch, &all);
            let mut declined = 0;
            for (&(r, d), g) in all.iter().zip(got) {
                if crate::erfa::fk54z_kernel::<{ crate::vmath::HAS_FMA }>(r, d, bepoch).2 {
                    let (er, ed, _, _) = crate::erfa::fk54z(r, d, bepoch);
                    assert_eq!(
                        g,
                        (er.to_bits(), ed.to_bits()),
                        "({r:e}, {d:e}) at {bepoch}"
                    );
                    declined += 1;
                }
            }
            assert!(declined > 0, "no position declined at {bepoch}");
        }
    }

    /// Every build of the FK slice loops gives the same bits: with and
    /// without a fused multiply-add in the arctangent everywhere, and on an
    /// x86-64 CPU with AVX2 and FMA, the copy built for them.
    #[test]
    fn every_build_of_the_fk_slice_loops_gives_the_same_bits() {
        let all = mixed_fk_positions();
        for bepoch in crate::test_inputs::fk_epochs() {
            let fk45z_unfused = through_fk_slice(fk45z_slice_body::<false>, bepoch, &all);
            let fk45z_fused = through_fk_slice(fk45z_slice_body::<true>, bepoch, &all);
            assert_eq!(fk45z_fused, fk45z_unfused, "fk45z with and without FMA");
            #[cfg(target_arch = "x86_64")]
            if crate::vmath::x86_has_avx2_fma() {
                let wide = through_fk_slice(
                    // SAFETY: the CPU has AVX2 and FMA, checked just now.
                    |e, a, b, c, d| unsafe { fk45z_slice_avx2_fma(e, a, b, c, d) },
                    bepoch,
                    &all,
                );
                assert_eq!(wide, fk45z_unfused, "fk45z built for AVX2 and FMA");
            }
            let fk54z_unfused = through_fk_slice(fk54z_slice_body::<false>, bepoch, &all);
            let fk54z_fused = through_fk_slice(fk54z_slice_body::<true>, bepoch, &all);
            assert_eq!(fk54z_fused, fk54z_unfused, "fk54z with and without FMA");
            #[cfg(target_arch = "x86_64")]
            if crate::vmath::x86_has_avx2_fma() {
                let wide = through_fk_slice(
                    // SAFETY: the CPU has AVX2 and FMA, checked just now.
                    |e, a, b, c, d| unsafe { fk54z_slice_avx2_fma(e, a, b, c, d) },
                    bepoch,
                    &all,
                );
                assert_eq!(wide, fk54z_unfused, "fk54z built for AVX2 and FMA");
            }
        }
    }

    const TOL: f64 = 1.0e-10;

    /// Helper: assert two floats are close.
    fn assert_close(a: f64, b: f64, tol: f64) {
        assert!(
            (a - b).abs() < tol,
            "expected {a} ~= {b} (diff = {})",
            (a - b).abs()
        );
    }

    // -- Epoch conversions --

    #[test]
    fn test_epj_j2000() {
        // MJD 51544.5 is J2000.0
        assert_close(epj(51544.5), 2000.0, TOL);
    }

    #[test]
    fn test_epj_round_trip() {
        let mjd = 53000.0;
        assert_close(epj2d(epj(mjd)), mjd, TOL);
    }

    #[test]
    fn test_epb_round_trip() {
        assert_close(epb(epb2d(1950.0)), 1950.0, TOL);
    }

    #[test]
    fn test_epb2d_b1950() {
        // B1950 corresponds to a known MJD
        let mjd = epb2d(1950.0);
        assert_close(epb(mjd), 1950.0, TOL);
    }

    // -- Spherical / Cartesian --

    #[test]
    fn test_dcs2c_pole() {
        let v = dcs2c(0.0, FRAC_PI_2);
        assert_close(v[0], 0.0, TOL);
        assert_close(v[1], 0.0, TOL);
        assert_close(v[2], 1.0, TOL);
    }

    #[test]
    fn test_dcs2c_dcc2s_round_trip() {
        let a = 1.234;
        let b = 0.567;
        let v = dcs2c(a, b);
        let (a2, b2) = dcc2s(&v);
        assert_close(a2, a, TOL);
        assert_close(b2, b, TOL);
    }

    // -- Vector/matrix --

    #[test]
    fn test_dvdv_orthogonal() {
        let va = [1.0, 0.0, 0.0];
        let vb = [0.0, 1.0, 0.0];
        assert_close(dvdv(&va, &vb), 0.0, TOL);
    }

    #[test]
    fn test_dvdv_parallel() {
        let v = [1.0, 2.0, 3.0];
        assert_close(dvdv(&v, &v), 14.0, TOL);
    }

    #[test]
    fn test_dvxv_cross_product() {
        let va = [1.0, 0.0, 0.0];
        let vb = [0.0, 1.0, 0.0];
        let vc = dvxv(&va, &vb);
        assert_close(vc[0], 0.0, TOL);
        assert_close(vc[1], 0.0, TOL);
        assert_close(vc[2], 1.0, TOL);
    }

    #[test]
    fn test_dvn_normalize() {
        let v = [3.0, 4.0, 0.0];
        let (uv, vm) = dvn(&v);
        assert_close(vm, 5.0, TOL);
        assert_close(uv[0], 0.6, TOL);
        assert_close(uv[1], 0.8, TOL);
        assert_close(uv[2], 0.0, TOL);
    }

    #[test]
    fn test_dmxv_identity() {
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let v = [1.0, 2.0, 3.0];
        let result = dmxv(&identity, &v);
        for i in 0..3 {
            assert_close(result[i], v[i], TOL);
        }
    }

    #[test]
    fn test_dimxv_inverse() {
        // For an orthogonal matrix, dimxv (transpose multiply) should undo dmxv.
        let angle = 0.3;
        let m = deuler("Z", angle, 0.0, 0.0);
        let v = [1.0, 0.0, 0.0];
        let rotated = dmxv(&m, &v);
        let back = dimxv(&m, &rotated);
        for i in 0..3 {
            assert_close(back[i], v[i], TOL);
        }
    }

    #[test]
    fn test_dmxm_identity() {
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let a = deuler("Z", 0.5, 0.0, 0.0);
        let c = dmxm(&a, &identity);
        for i in 0..3 {
            for j in 0..3 {
                assert_close(c[i][j], a[i][j], TOL);
            }
        }
    }

    // -- Coordinate conversions --

    #[test]
    fn test_galeq_center() {
        // Galactic center (l=0, b=0) should be near RA~266.4 deg, Dec~-28.9 deg
        let (dr, dd) = galeq(0.0, 0.0);
        let ra_deg = dr.to_degrees();
        let dec_deg = dd.to_degrees();
        assert_close(ra_deg, 266.405, 0.01);
        assert_close(dec_deg, -28.936, 0.01);
    }

    #[test]
    fn test_eqgal_galeq_round_trip() {
        let ra = 3.0;
        let dec = 0.5;
        let (dl, db) = eqgal(ra, dec);
        let (ra2, dec2) = galeq(dl, db);
        assert_close(ra2, ra, TOL);
        assert_close(dec2, dec, TOL);
    }

    #[test]
    fn test_galsup_supgal_round_trip() {
        let dl = 1.5;
        let db = 0.3;
        let (dsl, dsb) = galsup(dl, db);
        let (dl2, db2) = supgal(dsl, dsb);
        assert_close(dl2, dl, TOL);
        assert_close(db2, db, TOL);
    }

    #[test]
    fn test_fk45z_fk54z_round_trip() {
        let r1950 = 1.0;
        let d1950 = 0.5;
        let bepoch = 1984.0;
        let (r2000, d2000) = fk45z(r1950, d1950, bepoch);
        let (r1950b, d1950b, _, _) = fk54z(r2000, d2000, bepoch);
        assert_close(r1950b, r1950, 1.0e-6);
        assert_close(d1950b, d1950, 1.0e-6);
    }

    #[test]
    fn test_fk5hz_hfk5z_round_trip() {
        let r5 = 2.0;
        let d5 = 0.4;
        let epoch = 2000.0;
        let (rh, dh) = fk5hz(r5, d5, epoch);
        let (r5b, d5b, _, _) = hfk5z(rh, dh, epoch);
        assert_close(r5b, r5, 1.0e-9);
        assert_close(d5b, d5, 1.0e-9);
    }

    // -- Precession --

    #[test]
    fn test_prec_det_one() {
        // A precession matrix is a proper rotation: det = 1.
        let m = prec(2000.0, 2020.0);
        let det = determinant_3x3(&m);
        assert_close(det, 1.0, 1.0e-12);
    }

    #[test]
    fn test_prec_identity_same_epoch() {
        // Same-epoch precession should be very close to identity.
        // The IAU 1976 model (eraPmat76) has small numerical noise
        // at the ~1e-7 level due to epoch→MJD→JD round-trips.
        let m = prec(2000.0, 2000.0);
        for (i, row) in m.iter().enumerate() {
            for (j, &val) in row.iter().enumerate() {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert_close(val, expected, 1.0e-7);
            }
        }
    }

    #[test]
    fn test_prebn_det_one() {
        let m = prebn(1950.0, 2000.0);
        let det = determinant_3x3(&m);
        assert_close(det, 1.0, 1.0e-12);
    }

    #[test]
    fn test_ecmat_det_one() {
        let m = ecmat(51544.5);
        let det = determinant_3x3(&m);
        assert_close(det, 1.0, 1.0e-12);
    }

    // -- Euler --

    #[test]
    fn test_deuler_identity() {
        let m = deuler("Z", 0.0, 0.0, 0.0);
        for (i, row) in m.iter().enumerate() {
            for (j, &val) in row.iter().enumerate() {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert_close(val, expected, TOL);
            }
        }
    }

    #[test]
    fn test_deuler_z_rotation() {
        let angle = FRAC_PI_2;
        let m = deuler("Z", angle, 0.0, 0.0);
        // PAL uses passive (frame-rotation) convention: rotating the
        // frame by +90 deg about Z maps [1,0,0] to [0,-1,0].
        let v = dmxv(&m, &[1.0, 0.0, 0.0]);
        assert_close(v[0], 0.0, TOL);
        assert_close(v[1], -1.0, TOL);
        assert_close(v[2], 0.0, TOL);
    }

    // -- Aberration --

    #[test]
    fn test_addet_subet_round_trip() {
        let rm = 1.0;
        let dm = 0.5;
        let eq = 1984.0;
        let (rc, dc) = addet(rm, dm, eq);
        let (rm2, dm2) = subet(rc, dc, eq);
        assert_close(rm2, rm, 1.0e-9);
        assert_close(dm2, dm, 1.0e-9);
    }

    #[test]
    fn test_mappa_ampqk_mapqkz_round_trip() {
        let rm = 2.0;
        let dm = 0.3;
        let amprms = mappa(2000.0, 51544.5);
        let (ra, da) = mapqkz(rm, dm, &amprms);
        let (rm2, dm2) = ampqk(ra, da, &amprms);
        assert_close(rm2, rm, 1.0e-9);
        assert_close(dm2, dm, 1.0e-9);
    }

    // -- Angular normalization --

    #[test]
    fn test_drange_4pi() {
        assert_close(drange(4.0 * PI), 0.0, TOL);
    }

    #[test]
    fn test_drange_pi() {
        // pi should map to pi (or -pi, they are equivalent)
        let result = drange(PI);
        assert!((result - PI).abs() < TOL || (result + PI).abs() < TOL);
    }

    #[test]
    fn test_dranrm_negative() {
        assert_close(dranrm(-0.5), 2.0 * PI - 0.5, TOL);
    }

    #[test]
    fn test_dranrm_positive() {
        assert_close(dranrm(0.5), 0.5, TOL);
    }

    #[test]
    fn test_dsep_zero() {
        assert_close(dsep(1.0, 0.5, 1.0, 0.5), 0.0, TOL);
    }

    #[test]
    fn test_dsep_poles() {
        // Separation between north pole and south pole is pi.
        assert_close(dsep(0.0, FRAC_PI_2, 0.0, -FRAC_PI_2), PI, TOL);
    }

    #[test]
    fn test_dbear_east() {
        // Point due east on the equator: bearing should be ~pi/2.
        let pa = dbear(0.0, 0.0, 0.1, 0.0);
        assert_close(pa, FRAC_PI_2, 0.01);
    }

    // -- Earth / observer --

    #[test]
    fn test_evp_returns_nonzero() {
        let (dvb, dpb, dvh, dph) = evp(51544.5, 2000.0);
        // Just check the vectors are non-trivially populated.
        let mag_dpb: f64 = crate::math::sqrt(dpb.iter().map(|x| x * x).sum::<f64>());
        let mag_dph: f64 = crate::math::sqrt(dph.iter().map(|x| x * x).sum::<f64>());
        assert!(mag_dpb > 0.9, "barycentric position magnitude too small");
        assert!(mag_dph > 0.9, "heliocentric position magnitude too small");
        // Velocities should also be non-zero.
        let mag_dvb: f64 = crate::math::sqrt(dvb.iter().map(|x| x * x).sum::<f64>());
        let mag_dvh: f64 = crate::math::sqrt(dvh.iter().map(|x| x * x).sum::<f64>());
        assert!(mag_dvb > 0.0);
        assert!(mag_dvh > 0.0);
    }

    #[test]
    fn test_geoc_equator() {
        // At equator and sea level, r should be ~Earth radius in meters-ish
        // (actually geoc returns values in units of Earth radii, and z should be ~0).
        let (r, z) = geoc(0.0, 0.0);
        assert!(r > 0.0, "r should be positive at equator");
        assert_close(z, 0.0, 1.0e-6);
    }

    // -- Miscellaneous --

    #[test]
    fn test_dav2m_zero() {
        // Zero axial vector -> identity matrix.
        let m = dav2m(&[0.0, 0.0, 0.0]);
        for (i, row) in m.iter().enumerate() {
            for (j, &val) in row.iter().enumerate() {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert_close(val, expected, TOL);
            }
        }
    }

    #[test]
    fn test_dav2m_z_rotation() {
        // Rotation of pi/2 about Z.
        let angle = FRAC_PI_2;
        let m = dav2m(&[0.0, 0.0, angle]);
        // Verify the matrix is a proper rotation (det = 1).
        let det = determinant_3x3(&m);
        assert_close(det, 1.0, TOL);
        // Verify M * M^T = I (orthogonal matrix).
        let v = dmxv(&m, &[1.0, 0.0, 0.0]);
        let back = dimxv(&m, &v);
        assert_close(back[0], 1.0, TOL);
        assert_close(back[1], 0.0, TOL);
        assert_close(back[2], 0.0, TOL);
    }

    // -- Time / sidereal --

    #[test]
    fn dat_j2000() {
        // TAI-UTC at J2000.0 (MJD 51544.5) = 32 seconds
        assert!((super::dat(51544.5) - 32.0).abs() < 0.5);
    }

    #[test]
    fn dat_2017() {
        // After 2017 Jan 1 (MJD ~57754): TAI-UTC = 37 seconds
        assert!((super::dat(58000.0) - 37.0).abs() < 0.5);
    }

    #[test]
    fn gmst_range() {
        // GMST should be in range [0, 2π]
        let g = super::gmst(51544.5);
        assert!((0.0..2.0 * std::f64::consts::PI).contains(&g));
    }

    #[test]
    fn rvlsrk_nonzero() {
        // LSRK velocity should be non-zero for arbitrary direction
        let v = super::rvlsrk(1.0, 0.5);
        assert!(v.abs() > 0.1); // km/s
    }

    #[test]
    fn djcl_known_date() {
        // MJD 51544.0 is 2000-01-01 (J2000.0 epoch is 51544.5 = noon).
        let (iy, im, id, fd) = super::djcl(51544.0).unwrap();
        assert_eq!((iy, im, id), (2000, 1, 1));
        assert!(fd.abs() < 1e-9);
        // The 0.5-day fraction gives midday.
        let (_, _, _, fd2) = super::djcl(51544.5).unwrap();
        assert!((fd2 - 0.5).abs() < 1e-9);
    }

    #[test]
    fn cldj_round_trips_djcl() {
        let (mjd, warning) = super::cldj(2000, 1, 1).unwrap();
        assert_eq!(warning, None);
        assert!((mjd - 51544.0).abs() < 1e-9);
        let (iy, im, id, _) = super::djcl(mjd).unwrap();
        assert_eq!((iy, im, id), (2000, 1, 1));
    }

    #[test]
    fn cldj_rejects_bad_month() {
        assert_eq!(
            super::cldj(2000, 13, 1),
            Err(crate::erfa::Cal2jdError::BadMonth)
        );
    }

    #[test]
    fn dd2tf_and_dtf2d_round_trip() {
        // 0.5 days = 12:00:00.
        let (sign, ihmsf) = super::dd2tf(3, 0.5);
        assert_eq!(sign, '+');
        assert_eq!([ihmsf[0], ihmsf[1], ihmsf[2]], [12, 0, 0]);
        // Reconstruct the day fraction from h:m:s.
        let (days, warning) = super::dtf2d(ihmsf[0], ihmsf[1], ihmsf[2] as f64);
        assert_eq!(warning, None);
        assert!((days - 0.5).abs() < 1e-9);
    }

    #[test]
    fn dd2tf_negative_sign() {
        let (sign, _) = super::dd2tf(0, -0.25);
        assert_eq!(sign, '-');
    }

    // -- Helper --

    fn determinant_3x3(m: &[[f64; 3]; 3]) -> f64 {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }
}

#[cfg(test)]
mod count_tests {
    #[cfg(feature = "call-counts")]
    #[test]
    fn eqgal_counts_by_symbol() {
        crate::call_counts::reset();
        let _ = super::eqgal(1.0, 0.5);
        assert_eq!(crate::call_counts::snapshot().get("astPalEqgal"), Some(&1));
    }
}
