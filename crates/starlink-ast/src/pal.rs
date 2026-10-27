// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

// All wrappers are pub(crate) and will be consumed by SlaMap, SkyFrame,
// etc.  Until those land, suppress dead-code warnings.
#![allow(dead_code)]

//! Safe Rust wrappers for the vendored PAL (Positional Astronomy Library).
//!
//! The C PAL library is compiled from `vendor/palwrap.c` into
//! `libast_pal_erfa.a` by `build.rs`.  All C symbols carry an `astPal`
//! prefix (via the `pal2ast.h` macro header) to avoid clashes with any
//! external PAL installation.
//!
//! This module is `pub(crate)` and provides safe, idiomatic wrappers
//! that hide the raw pointer gymnastics.  Angles are in radians,
//! dates are MJD, and 3-vectors / 3x3 matrices use fixed-size arrays.

use std::ffi::c_char;

use crate::constants::BAD_VALUE;

/// ERFA's `ERFA_DAU` (erfam.h): the astronomical unit in metres, the IAU 2012
/// value. PAL's routines convert metres to au with it, so a caller that
/// converts their output back must use the same value.
pub(crate) const ERFA_DAU: f64 = 149597870.7e3;

// ---------------------------------------------------------------------------
// Raw C extern declarations (astPal-prefixed symbols)
// ---------------------------------------------------------------------------

unsafe extern "C" {
    // -- Epoch conversions --
    fn astPalEpj(date: f64) -> f64;
    fn astPalEpj2d(epj: f64) -> f64;
    fn astPalEpb(date: f64) -> f64;
    fn astPalEpb2d(epb: f64) -> f64;

    // -- Vector / matrix --
    fn astPalDmxv(dm: *const [f64; 3], va: *const f64, vb: *mut f64);
    fn astPalDimxv(dm: *const [f64; 3], va: *const f64, vb: *mut f64);
    fn astPalDmxm(da: *const [f64; 3], db: *const [f64; 3], dc: *mut [f64; 3]);
    fn astPalDvxv(va: *const f64, vb: *const f64, vc: *mut f64);
    fn astPalDvdv(va: *const f64, vb: *const f64) -> f64;
    fn astPalDvn(v: *const f64, uv: *mut f64, vm: *mut f64);
    fn astPalDmat(n: i32, a: *mut f64, y: *mut f64, d: *mut f64, jf: *mut i32, iw: *mut i32);

    // -- Coordinate conversions --
    fn astPalGaleq(dl: f64, db: f64, dr: *mut f64, dd: *mut f64);
    fn astPalEqgal(dr: f64, dd: f64, dl: *mut f64, db: *mut f64);
    fn astPalGalsup(dl: f64, db: f64, dsl: *mut f64, dsb: *mut f64);
    fn astPalSupgal(dsl: f64, dsb: f64, dl: *mut f64, db: *mut f64);
    fn astEraFk45z(r1950: f64, d1950: f64, bepoch: f64, r2000: *mut f64, d2000: *mut f64);
    fn astEraFk54z(
        r2000: f64,
        d2000: f64,
        bepoch: f64,
        r1950: *mut f64,
        d1950: *mut f64,
        dr1950: *mut f64,
        dd1950: *mut f64,
    );
    fn astPalFk5hz(r5: f64, d5: f64, epoch: f64, rh: *mut f64, dh: *mut f64);
    fn astPalHfk5z(
        rh: f64,
        dh: f64,
        epoch: f64,
        r5: *mut f64,
        d5: *mut f64,
        dr5: *mut f64,
        dd5: *mut f64,
    );

    // -- Precession matrices --
    fn astPalPrec(ep0: f64, ep1: f64, rmatp: *mut [f64; 3]);
    fn astPalPrebn(bep0: f64, bep1: f64, rmatp: *mut [f64; 3]);
    fn astPalEcmat(date: f64, rmat: *mut [f64; 3]);

    // -- Euler rotation --
    fn astPalDeuler(order: *const c_char, phi: f64, theta: f64, psi: f64, rmat: *mut [f64; 3]);

    // -- Aberration --
    fn astPalAddet(rm: f64, dm: f64, eq: f64, rc: *mut f64, dc: *mut f64);
    fn astPalSubet(rc: f64, dc: f64, eq: f64, rm: *mut f64, dm: *mut f64);
    fn astPalMappa(eq: f64, date: f64, amprms: *mut f64);
    fn astPalAmpqk(ra: f64, da: f64, amprms: *const f64, rm: *mut f64, dm: *mut f64);
    fn astPalMapqkz(rm: f64, dm: f64, amprms: *const f64, ra: *mut f64, da: *mut f64);

    // -- Angular normalization --
    fn astPalDrange(angle: f64) -> f64;
    fn astPalDranrm(angle: f64) -> f64;
    fn astPalDsep(al: f64, ap: f64, bl: f64, bp: f64) -> f64;
    fn astPalDbear(a1: f64, b1: f64, a2: f64, b2: f64) -> f64;

    // -- Earth / observer --
    fn astPalEvp(date: f64, deqx: f64, dvb: *mut f64, dpb: *mut f64, dvh: *mut f64, dph: *mut f64);
    fn astPalGeoc(p: f64, h: f64, r: *mut f64, z: *mut f64);

    // -- Miscellaneous --
    fn astPalDav2m(axvec: *const f64, rmat: *mut [f64; 3]);

    // -- Time / sidereal --
    fn astPalDat(dju: f64) -> f64;
    fn astPalGmst(ut1: f64) -> f64;
    fn astPalEqeqx(date: f64) -> f64;
    fn astEraDtdb(date1: f64, date2: f64, ut: f64, elong: f64, u: f64, v: f64) -> f64;
    fn astEraGd2gc(n: i32, elong: f64, phi: f64, height: f64, xyz: *mut f64) -> i32;
    fn astEraGc2gd(
        n: i32,
        xyz: *const f64,
        elong: *mut f64,
        phi: *mut f64,
        height: *mut f64,
    ) -> i32;

    // -- Observer / velocity --
    fn astPalPvobs(p: f64, h: f64, stl: f64, pv: *mut f64);
    fn astPalRvgalc(r2000: f64, d2000: f64) -> f64;
    fn astPalRvlg(r2000: f64, d2000: f64) -> f64;
    fn astPalRvlsrd(r2000: f64, d2000: f64) -> f64;
    fn astPalRvlsrk(r2000: f64, d2000: f64) -> f64;

    // -- Calendar / time-of-day conversions --
    fn astPalDjcl(djm: f64, iy: *mut i32, im: *mut i32, id: *mut i32, fd: *mut f64, j: *mut i32);
    fn astPalCldj(iy: i32, im: i32, id: i32, djm: *mut f64, j: *mut i32);
    fn astPalDd2tf(ndp: i32, days: f64, sign: *mut c_char, ihmsf: *mut i32);
    fn astPalDtf2d(ihour: i32, imin: i32, sec: f64, days: *mut f64, j: *mut i32);
}

use crate::call_counts::counted;

// ---------------------------------------------------------------------------
// Group 1: Epoch conversions
// ---------------------------------------------------------------------------

/// Convert Modified Julian Date to Julian epoch.
pub(crate) fn epj(date: f64) -> f64 {
    counted!(astPalEpj(date))
}

/// Convert Julian epoch to Modified Julian Date.
pub(crate) fn epj2d(epj: f64) -> f64 {
    counted!(astPalEpj2d(epj))
}

/// Convert Modified Julian Date to Besselian epoch.
pub(crate) fn epb(date: f64) -> f64 {
    counted!(astPalEpb(date))
}

/// Convert Besselian epoch to Modified Julian Date.
pub(crate) fn epb2d(epb: f64) -> f64 {
    counted!(astPalEpb2d(epb))
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
/// `x`, `y` and `z`, within 3 ULP of [`dcs2c`]: the slice runs a
/// vectorizable kernel whose sines and cosines are each within 1 ULP. Each point gets the same bits whatever the slice's
/// length. A `BAD_VALUE` coordinate gives `BAD_VALUE` outputs, without the
/// cost of computing them.
///
/// Counts no calls: a caller that skips some points afterwards, as C skips
/// them before calling, counts the calls C makes with
/// [`crate::call_counts::hit_n`].
pub(crate) fn dcs2c_slice(a: &[f64], b: &[f64], x: &mut [f64], y: &mut [f64], z: &mut [f64]) {
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
/// the slice's length. A `BAD_VALUE` coordinate gives `BAD_VALUE` outputs,
/// as for [`dcs2c_slice`], which this also follows in counting no calls.
pub(crate) fn dcc2s_slice(x: &[f64], y: &[f64], z: &[f64], a: &mut [f64], b: &mut [f64]) {
    let n = x.len();
    assert!(y.len() == n && z.len() == n && a.len() == n && b.len() == n);
    // The branchless kernel over every point, so the loop vectorizes, then
    // ERFA's own arithmetic for the points outside the kernel's domain.
    for i in 0..n {
        (a[i], b[i]) = crate::erfa::c2s_kernel([x[i] + 0.0, y[i] + 0.0, z[i] + 0.0]);
    }
    for i in 0..n {
        let p = [x[i] + 0.0, y[i] + 0.0, z[i] + 0.0];
        if p.contains(&BAD_VALUE) {
            (a[i], b[i]) = (BAD_VALUE, BAD_VALUE);
        } else if crate::erfa::c2s_needs_special(p) {
            (a[i], b[i]) = crate::erfa::c2s(p);
        }
    }
}

// ---------------------------------------------------------------------------
// Group 3: Vector / matrix operations
// ---------------------------------------------------------------------------

/// Multiply a 3x3 matrix by a 3-vector (forward transformation).
pub(crate) fn dmxv(dm: &[[f64; 3]; 3], va: &[f64; 3]) -> [f64; 3] {
    let mut vb = [0.0; 3];
    counted!(astPalDmxv(dm.as_ptr(), va.as_ptr(), vb.as_mut_ptr()));
    vb
}

/// Multiply the inverse of a 3x3 matrix by a 3-vector (backward / transpose).
pub(crate) fn dimxv(dm: &[[f64; 3]; 3], va: &[f64; 3]) -> [f64; 3] {
    let mut vb = [0.0; 3];
    counted!(astPalDimxv(dm.as_ptr(), va.as_ptr(), vb.as_mut_ptr()));
    vb
}

/// Multiply two 3x3 matrices: `c = a * b`.
pub(crate) fn dmxm(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut c = [[0.0; 3]; 3];
    counted!(astPalDmxm(a.as_ptr(), b.as_ptr(), c.as_mut_ptr()));
    c
}

/// Cross product of two 3-vectors.
pub(crate) fn dvxv(va: &[f64; 3], vb: &[f64; 3]) -> [f64; 3] {
    let mut vc = [0.0; 3];
    counted!(astPalDvxv(va.as_ptr(), vb.as_ptr(), vc.as_mut_ptr()));
    vc
}

/// Dot product of two 3-vectors.
pub(crate) fn dvdv(va: &[f64; 3], vb: &[f64; 3]) -> f64 {
    counted!(astPalDvdv(va.as_ptr(), vb.as_ptr()))
}

/// Normalize a 3-vector, returning (unit_vector, modulus).
pub(crate) fn dvn(v: &[f64; 3]) -> ([f64; 3], f64) {
    let mut uv = [0.0; 3];
    let mut vm = 0.0;
    counted!(astPalDvn(v.as_ptr(), uv.as_mut_ptr(), &mut vm));
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
    let mut d = 0.0_f64;
    let mut jf = 0_i32;
    // Pivot bookkeeping scratch used internally by palDmat.
    let mut iw = vec![0_i32; n];
    counted!(astPalDmat(
        n as i32,
        a.as_mut_ptr(),
        y.as_mut_ptr(),
        &mut d,
        &mut jf,
        iw.as_mut_ptr(),
    ));
    (jf == 0, d)
}

// ---------------------------------------------------------------------------
// Group 4: Coordinate conversions
// ---------------------------------------------------------------------------

/// Galactic (l, b) to FK5 J2000 (ra, dec).  All radians.
pub(crate) fn galeq(dl: f64, db: f64) -> (f64, f64) {
    let mut dr = 0.0;
    let mut dd = 0.0;
    counted!(astPalGaleq(dl, db, &mut dr, &mut dd));
    (dr, dd)
}

/// FK5 J2000 (ra, dec) to Galactic (l, b).  All radians.
pub(crate) fn eqgal(dr: f64, dd: f64) -> (f64, f64) {
    let mut dl = 0.0;
    let mut db = 0.0;
    counted!(astPalEqgal(dr, dd, &mut dl, &mut db));
    (dl, db)
}

/// Galactic (l, b) to Supergalactic (sl, sb).  All radians.
pub(crate) fn galsup(dl: f64, db: f64) -> (f64, f64) {
    let mut dsl = 0.0;
    let mut dsb = 0.0;
    counted!(astPalGalsup(dl, db, &mut dsl, &mut dsb));
    (dsl, dsb)
}

/// Supergalactic (sl, sb) to Galactic (l, b).  All radians.
pub(crate) fn supgal(dsl: f64, dsb: f64) -> (f64, f64) {
    let mut dl = 0.0;
    let mut db = 0.0;
    counted!(astPalSupgal(dsl, dsb, &mut dl, &mut db));
    (dl, db)
}

/// FK4 B1950 (ra, dec) to FK5 J2000 (ra, dec), assuming zero proper motion.
///
/// ERFA's `eraFk45z`, which the C library's SlaMap calls.
pub(crate) fn fk45z(r1950: f64, d1950: f64, bepoch: f64) -> (f64, f64) {
    let mut r2000 = 0.0;
    let mut d2000 = 0.0;
    counted!(astEraFk45z(r1950, d1950, bepoch, &mut r2000, &mut d2000));
    (r2000, d2000)
}

/// FK5 J2000 (ra, dec) to FK4 B1950 (ra, dec) plus proper-motion terms.
///
/// Returns `(r1950, d1950, dr1950, dd1950)`. ERFA's `eraFk54z`, which the C
/// library's SlaMap calls.
pub(crate) fn fk54z(r2000: f64, d2000: f64, bepoch: f64) -> (f64, f64, f64, f64) {
    let mut r1950 = 0.0;
    let mut d1950 = 0.0;
    let mut dr1950 = 0.0;
    let mut dd1950 = 0.0;
    counted!(astEraFk54z(
        r2000,
        d2000,
        bepoch,
        &mut r1950,
        &mut d1950,
        &mut dr1950,
        &mut dd1950,
    ));
    (r1950, d1950, dr1950, dd1950)
}

/// FK5 J2000 to ICRS (Hipparcos frame), assuming zero proper motion.
pub(crate) fn fk5hz(r5: f64, d5: f64, epoch: f64) -> (f64, f64) {
    let mut rh = 0.0;
    let mut dh = 0.0;
    counted!(astPalFk5hz(r5, d5, epoch, &mut rh, &mut dh));
    (rh, dh)
}

/// ICRS (Hipparcos frame) to FK5 J2000, assuming zero proper motion.
///
/// Returns `(r5, d5, dr5, dd5)`.
pub(crate) fn hfk5z(rh: f64, dh: f64, epoch: f64) -> (f64, f64, f64, f64) {
    let mut r5 = 0.0;
    let mut d5 = 0.0;
    let mut dr5 = 0.0;
    let mut dd5 = 0.0;
    counted!(astPalHfk5z(
        rh, dh, epoch, &mut r5, &mut d5, &mut dr5, &mut dd5
    ));
    (r5, d5, dr5, dd5)
}

// ---------------------------------------------------------------------------
// Group 5: Precession matrices
// ---------------------------------------------------------------------------

/// Julian (FK5) precession matrix from epoch `ep0` to `ep1` (Julian epochs).
pub(crate) fn prec(ep0: f64, ep1: f64) -> [[f64; 3]; 3] {
    let mut m = [[0.0; 3]; 3];
    counted!(astPalPrec(ep0, ep1, m.as_mut_ptr()));
    m
}

/// Besselian (FK4) precession matrix from epoch `bep0` to `bep1`.
pub(crate) fn prebn(bep0: f64, bep1: f64) -> [[f64; 3]; 3] {
    let mut m = [[0.0; 3]; 3];
    counted!(astPalPrebn(bep0, bep1, m.as_mut_ptr()));
    m
}

/// Equatorial to ecliptic rotation matrix for a given TDB date (MJD).
pub(crate) fn ecmat(date: f64) -> [[f64; 3]; 3] {
    let mut m = [[0.0; 3]; 3];
    counted!(astPalEcmat(date, m.as_mut_ptr()));
    m
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
    // Stack-allocate a small null-terminated buffer for the order string.
    let mut buf = [0u8; 4];
    let bytes = order.as_bytes();
    let len = bytes.len().min(3);
    buf[..len].copy_from_slice(&bytes[..len]);
    // buf[len..] is already zero-filled (null terminator)

    let mut m = [[0.0; 3]; 3];
    counted!(astPalDeuler(
        buf.as_ptr().cast::<c_char>(),
        phi,
        theta,
        psi,
        m.as_mut_ptr(),
    ));
    m
}

// ---------------------------------------------------------------------------
// Group 7: Aberration
// ---------------------------------------------------------------------------

/// Add E-terms of aberration.
///
/// Returns `(rc, dc)` — the apparent (ra, dec) with E-terms added.
pub(crate) fn addet(rm: f64, dm: f64, eq: f64) -> (f64, f64) {
    let mut rc = 0.0;
    let mut dc = 0.0;
    counted!(astPalAddet(rm, dm, eq, &mut rc, &mut dc));
    (rc, dc)
}

/// Remove E-terms of aberration.
///
/// Returns `(rm, dm)` — the mean (ra, dec) with E-terms removed.
pub(crate) fn subet(rc: f64, dc: f64, eq: f64) -> (f64, f64) {
    let mut rm = 0.0;
    let mut dm = 0.0;
    counted!(astPalSubet(rc, dc, eq, &mut rm, &mut dm));
    (rm, dm)
}

/// Compute mean-to-apparent parameters for a given equinox and date.
///
/// Returns a 21-element array of star-independent parameters used by
/// [`ampqk`] and [`mapqkz`].
pub(crate) fn mappa(eq: f64, date: f64) -> [f64; 21] {
    let mut amprms = [0.0; 21];
    counted!(astPalMappa(eq, date, amprms.as_mut_ptr()));
    amprms
}

/// Quick apparent (ra, dec) to mean (ra, dec).
///
/// Uses pre-computed parameters from [`mappa`].
pub(crate) fn ampqk(ra: f64, da: f64, amprms: &[f64; 21]) -> (f64, f64) {
    let mut rm = 0.0;
    let mut dm = 0.0;
    counted!(astPalAmpqk(ra, da, amprms.as_ptr(), &mut rm, &mut dm));
    (rm, dm)
}

/// Quick mean (ra, dec) to apparent (ra, dec), assuming zero proper motion.
///
/// Uses pre-computed parameters from [`mappa`].
pub(crate) fn mapqkz(rm: f64, dm: f64, amprms: &[f64; 21]) -> (f64, f64) {
    let mut ra = 0.0;
    let mut da = 0.0;
    counted!(astPalMapqkz(rm, dm, amprms.as_ptr(), &mut ra, &mut da));
    (ra, da)
}

// ---------------------------------------------------------------------------
// Group 8: Angular normalization
// ---------------------------------------------------------------------------

/// Normalize an angle to the range [-pi, pi].
pub(crate) fn drange(angle: f64) -> f64 {
    counted!(astPalDrange(angle))
}

/// Normalize an angle to the range [0, 2*pi].
pub(crate) fn dranrm(angle: f64) -> f64 {
    counted!(astPalDranrm(angle))
}

/// Angular separation between two points on a sphere (radians).
pub(crate) fn dsep(al: f64, ap: f64, bl: f64, bp: f64) -> f64 {
    counted!(astPalDsep(al, ap, bl, bp))
}

/// Bearing (position angle) of point B as seen from point A (radians).
pub(crate) fn dbear(a1: f64, b1: f64, a2: f64, b2: f64) -> f64 {
    counted!(astPalDbear(a1, b1, a2, b2))
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
    let mut dvb = [0.0; 3];
    let mut dpb = [0.0; 3];
    let mut dvh = [0.0; 3];
    let mut dph = [0.0; 3];
    counted!(astPalEvp(
        date,
        deqx,
        dvb.as_mut_ptr(),
        dpb.as_mut_ptr(),
        dvh.as_mut_ptr(),
        dph.as_mut_ptr(),
    ));
    (dvb, dpb, dvh, dph)
}

/// Geodetic latitude/height to geocentric distance from spin axis and
/// distance from equatorial plane.
///
/// - `p`: geodetic latitude (radians)
/// - `h`: height above reference spheroid (meters)
///
/// Returns `(r, z)` in meters.
pub(crate) fn geoc(p: f64, h: f64) -> (f64, f64) {
    let mut r = 0.0;
    let mut z = 0.0;
    counted!(astPalGeoc(p, h, &mut r, &mut z));
    (r, z)
}

// ---------------------------------------------------------------------------
// Group 10: Miscellaneous
// ---------------------------------------------------------------------------

/// Axial vector to rotation matrix.
///
/// The direction of the axial vector is the Euler axis, and its
/// magnitude is the rotation angle in radians.
pub(crate) fn dav2m(axvec: &[f64; 3]) -> [[f64; 3]; 3] {
    let mut m = [[0.0; 3]; 3];
    counted!(astPalDav2m(axvec.as_ptr(), m.as_mut_ptr()));
    m
}

// ---------------------------------------------------------------------------
// Group 11: Time / sidereal
// ---------------------------------------------------------------------------

/// TAI-UTC leap seconds for a given UTC date (MJD).
pub(crate) fn dat(mjd: f64) -> f64 {
    counted!(astPalDat(mjd))
}

/// Greenwich Mean Sidereal Time (radians) for a given UT1 date (MJD).
pub(crate) fn gmst(ut1: f64) -> f64 {
    counted!(astPalGmst(ut1))
}

/// Equation of the equinoxes (radians) for a given TDB date (MJD).
pub(crate) fn eqeqx(date: f64) -> f64 {
    counted!(astPalEqeqx(date))
}

/// Convert an MJD between the TDB timescale and one of TAI / TT /
/// UTC / TDB. Mirrors `fitschan.c::TDBConv` for the subset of
/// timescales that the FITS-WCS writer actually consults.
///
/// - `mjd`: input MJD value.
/// - `to_scale`: the timescale of the output (or input, when
///   `from_tdb = false`).
/// - `from_tdb`: when `true`, `mjd` is in TDB and the result is in
///   `to_scale`. When `false`, `mjd` is in `to_scale` and the result
///   is in TDB.
///
/// Conversion chain: `TDB ↔ TT ↔ TAI ↔ UTC` via:
///   - `TDB → TT`: subtract `dtdb_geocentre(mjd)` seconds.
///   - `TT → TAI`: subtract 32.184 seconds.
///   - `TAI → UTC`: subtract `dat(tai_mjd)` leap-second offset.
///
/// For other timescales (`UT1`, `LT`, sidereal, `TCB`, `TCG`) this
/// returns `None`. Those cases would need the full TimeFrame
/// conversion machinery.
pub(crate) fn tdb_conv(mjd: f64, to_scale: TdbConvScale, from_tdb: bool) -> Option<f64> {
    if !mjd.is_finite() {
        return None;
    }
    if matches!(to_scale, TdbConvScale::Tdb) {
        return Some(mjd);
    }
    if from_tdb {
        // TDB → TT
        let tt = mjd - dtdb_geocentre(mjd) / 86_400.0;
        if matches!(to_scale, TdbConvScale::Tt) {
            return Some(tt);
        }
        // TT → TAI
        let tai = tt - 32.184 / 86_400.0;
        if matches!(to_scale, TdbConvScale::Tai) {
            return Some(tai);
        }
        // TAI → UTC. dat() expects a UTC MJD; near the TAI/UTC step
        // the leap-second offset is a constant integer for the day,
        // so feeding tai_mjd in once gives the right answer except
        // exactly on a leap-second day boundary. The single-step
        // approximation matches what AST does internally.
        let leap = dat(tai);
        Some(tai - leap / 86_400.0)
    } else {
        // Reverse: target_scale → TAI → TT → TDB.
        let tai = match to_scale {
            TdbConvScale::Tai => mjd,
            TdbConvScale::Tt => mjd + 32.184 / 86_400.0, // TT → TAI is `-`, so reverse
            TdbConvScale::Utc => mjd + dat(mjd) / 86_400.0,
            TdbConvScale::Tdb => unreachable!(),
        };
        // TAI → TT
        let tt = match to_scale {
            TdbConvScale::Tt => mjd, // already TT
            TdbConvScale::Tai => mjd + 32.184 / 86_400.0,
            TdbConvScale::Utc => tai + 32.184 / 86_400.0,
            TdbConvScale::Tdb => unreachable!(),
        };
        // TT → TDB. We need dtdb at the TDB epoch, not TT — but
        // the offset is small enough (<2 ms) that dtdb(tt) ≈
        // dtdb(tdb). Mirrors C's iterative-1-pass approximation.
        Some(tt + dtdb_geocentre(tt) / 86_400.0)
    }
}

/// Restricted timescale enum for [`tdb_conv`]. Mirrors the subset of
/// the TimeFrame `TimeScale` enum that the FITS-WCS writer needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TdbConvScale {
    /// Coordinated Universal Time.
    Utc,
    /// International Atomic Time.
    Tai,
    /// Terrestrial Time.
    Tt,
    /// Barycentric Dynamical Time.
    Tdb,
}

/// Difference TDB − TT in seconds at the geocenter.
///
/// Wraps ERFA `eraDtdb`, the full Fairhead & Bretagnon (1990) series
/// (≤±3 ns absolute accuracy 1950-2050). At the geocenter the
/// observer-position arguments (`elong`, `u`, `v`) are zero; UT1 is
/// only required to ~1-day precision so the UTC fractional day is a
/// safe approximation. Mirrors the way AST's `Rcc` is invoked from
/// `timemap.c::AST__TDBTOTT` for a barycentric observer.
///
/// `tdb` is given as an MJD; the date is split into integer +
/// fraction parts as ERFA prefers (Note 1 of `eraDtdb`).
pub(crate) fn dtdb_geocentre(tdb_mjd: f64) -> f64 {
    let date1 = 2_400_000.5_f64;
    let date2 = tdb_mjd;
    let ut_frac = tdb_mjd - tdb_mjd.floor();
    counted!(astEraDtdb(date1, date2, ut_frac, 0.0, 0.0, 0.0))
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
    let mut xyz = [0.0_f64; 3];
    let status = counted!(astEraGd2gc(1, lon, lat, height, xyz.as_mut_ptr()));
    if status == 0 { Some(xyz) } else { None }
}

/// Geocentric Cartesian → geodetic longitude/latitude/height. Mirrors
/// `eraGc2gd(1, ...)` (WGS84 reference ellipsoid).
///
/// Returns `Some((lon_radians, lat_radians, height_metres))` or
/// `None` on error.
pub(crate) fn gc2gd(xyz: [f64; 3]) -> Option<(f64, f64, f64)> {
    let mut elong = 0.0_f64;
    let mut phi = 0.0_f64;
    let mut height = 0.0_f64;
    let status = counted!(astEraGc2gd(
        1,
        xyz.as_ptr(),
        &mut elong,
        &mut phi,
        &mut height
    ));
    if status == 0 {
        Some((elong, phi, height))
    } else {
        None
    }
}

/// Observer position and velocity as a 6-vector.
///
/// - `lat`: geodetic latitude (radians)
/// - `alt`: altitude above reference spheroid (meters)
/// - `lst`: local apparent sidereal time (radians)
///
/// Returns a 6-element array: [x, y, z, xdot, ydot, zdot] in AU and AU/s.
pub(crate) fn pvobs(lat: f64, alt: f64, lst: f64) -> [f64; 6] {
    let mut pv = [0.0; 6];
    counted!(astPalPvobs(lat, alt, lst, pv.as_mut_ptr()));
    pv
}

/// Component of galactic rotation velocity toward the source (km/s).
///
/// - `ra`: J2000 right ascension (radians)
/// - `dec`: J2000 declination (radians)
pub(crate) fn rvgalc(ra: f64, dec: f64) -> f64 {
    counted!(astPalRvgalc(ra, dec))
}

/// Velocity component of the Local Group motion toward the source (km/s).
///
/// - `ra`: J2000 right ascension (radians)
/// - `dec`: J2000 declination (radians)
pub(crate) fn rvlg(ra: f64, dec: f64) -> f64 {
    counted!(astPalRvlg(ra, dec))
}

/// Velocity component of the dynamical LSR toward the source (km/s).
///
/// - `ra`: J2000 right ascension (radians)
/// - `dec`: J2000 declination (radians)
pub(crate) fn rvlsrd(ra: f64, dec: f64) -> f64 {
    counted!(astPalRvlsrd(ra, dec))
}

/// Velocity component of the kinematic LSR toward the source (km/s).
///
/// - `ra`: J2000 right ascension (radians)
/// - `dec`: J2000 declination (radians)
pub(crate) fn rvlsrk(ra: f64, dec: f64) -> f64 {
    counted!(astPalRvlsrk(ra, dec))
}

// ---------------------------------------------------------------------------
// Group 13: Calendar / time-of-day conversions
// ---------------------------------------------------------------------------

/// Modified Julian Date to Gregorian calendar fields.
///
/// Wraps `palDjcl`. Returns `(year, month, day, day_fraction, status)`
/// where `status` is 0 on success or non-zero if `djm` is out of the
/// range supported by the underlying ERFA conversion.
pub(crate) fn djcl(djm: f64) -> (i32, i32, i32, f64, i32) {
    let mut iy = 0;
    let mut im = 0;
    let mut id = 0;
    let mut fd = 0.0;
    let mut j = 0;
    counted!(astPalDjcl(djm, &mut iy, &mut im, &mut id, &mut fd, &mut j));
    (iy, im, id, fd, j)
}

/// Gregorian calendar date to Modified Julian Date.
///
/// Wraps `palCldj`. Returns `(mjd, status)` where `status` is 0 on
/// success, or non-zero for an invalid year/month/day.
pub(crate) fn cldj(iy: i32, im: i32, id: i32) -> (f64, i32) {
    let mut djm = 0.0;
    let mut j = 0;
    counted!(astPalCldj(iy, im, id, &mut djm, &mut j));
    (djm, j)
}

/// Day fraction to hours, minutes, seconds.
///
/// Wraps `palDd2tf`. `ndp` is the number of decimal places of seconds.
/// Returns `(sign, [hours, minutes, seconds, fraction])` where `sign`
/// is `'+'` or `'-'` and the fraction is the `ndp`-digit fractional
/// seconds field as an integer.
pub(crate) fn dd2tf(ndp: i32, days: f64) -> (char, [i32; 4]) {
    let mut sign: c_char = 0;
    let mut ihmsf = [0i32; 4];
    counted!(astPalDd2tf(ndp, days, &mut sign, ihmsf.as_mut_ptr()));
    let sign = if sign == b'-' as c_char { '-' } else { '+' };
    (sign, ihmsf)
}

/// Hours, minutes, seconds to a day fraction.
///
/// Wraps `palDtf2d`. Returns `(days, status)` where `status` is 0 on
/// success, or non-zero for out-of-range hours/minutes/seconds.
pub(crate) fn dtf2d(ihour: i32, imin: i32, sec: f64) -> (f64, i32) {
    let mut days = 0.0;
    let mut j = 0;
    counted!(astPalDtf2d(ihour, imin, sec, &mut days, &mut j));
    (days, j)
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

    #[test]
    fn dcs2c_slice_is_within_three_ulp_of_dcs2c() {
        let all = mixed_angles();
        let bad = crate::constants::BAD_VALUE;
        for (p, r) in all.iter().zip(slice_s2c(&all)) {
            if p.0 == bad || p.1 == bad {
                continue;
            }
            let e = dcs2c(p.0, p.1);
            for k in 0..3 {
                let d = crate::test_inputs::ulps(r[k], e[k]);
                assert!(d <= 3, "{p:?} axis {k}: {:e} vs {:e}", r[k], e[k]);
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
        let (iy, im, id, fd, j) = super::djcl(51544.0);
        assert_eq!(j, 0);
        assert_eq!((iy, im, id), (2000, 1, 1));
        assert!(fd.abs() < 1e-9);
        // The 0.5-day fraction gives midday.
        let (_, _, _, fd2, _) = super::djcl(51544.5);
        assert!((fd2 - 0.5).abs() < 1e-9);
    }

    #[test]
    fn cldj_round_trips_djcl() {
        let (mjd, j) = super::cldj(2000, 1, 1);
        assert_eq!(j, 0);
        assert!((mjd - 51544.0).abs() < 1e-9);
        let (iy, im, id, _, _) = super::djcl(mjd);
        assert_eq!((iy, im, id), (2000, 1, 1));
    }

    #[test]
    fn cldj_rejects_bad_month() {
        let (_, j) = super::cldj(2000, 13, 1);
        assert_ne!(j, 0);
    }

    #[test]
    fn dd2tf_and_dtf2d_round_trip() {
        // 0.5 days = 12:00:00.
        let (sign, ihmsf) = super::dd2tf(3, 0.5);
        assert_eq!(sign, '+');
        assert_eq!([ihmsf[0], ihmsf[1], ihmsf[2]], [12, 0, 0]);
        // Reconstruct the day fraction from h:m:s.
        let (days, j) = super::dtf2d(ihmsf[0], ihmsf[1], ihmsf[2] as f64);
        assert_eq!(j, 0);
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
    /// Every FFI call in this file goes through `counted!`, so the
    /// `call-counts` feature sees every call into PAL and ERFA.
    #[test]
    fn every_ffi_call_is_counted() {
        let src = include_str!("pal.rs");
        let body = &src[src.find("\n}\n").expect("end of the extern block")..];
        let mut uncounted = Vec::new();
        for prefix in ["astPal", "astEra"] {
            for (i, _) in body.match_indices(prefix) {
                let name_end = body[i..]
                    .find(|c: char| !c.is_ascii_alphanumeric())
                    .map_or(body.len(), |e| i + e);
                let is_call = body[name_end..].starts_with('(');
                let counted = body[..i].trim_end().ends_with("counted!(");
                if is_call && !counted {
                    uncounted.push(&body[i..name_end]);
                }
            }
        }
        assert!(
            uncounted.is_empty(),
            "uncounted PAL/ERFA calls: {uncounted:?}"
        );
    }

    #[cfg(feature = "call-counts")]
    #[test]
    fn eqgal_counts_by_symbol() {
        crate::call_counts::reset();
        let _ = super::eqgal(1.0, 0.5);
        assert_eq!(crate::call_counts::snapshot().get("astPalEqgal"), Some(&1));
    }
}
