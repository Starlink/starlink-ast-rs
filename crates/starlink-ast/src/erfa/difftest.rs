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
}

/// Whether two values are the same bit for bit, any two NaNs counting as the
/// same: the sign and payload of a NaN from arithmetic depend on operand
/// order, which the C compiler may swap, and carry no meaning.
pub(super) fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// [`same`] for every element of two vectors.
pub(super) fn same3(a: &[f64; 3], b: &[f64; 3]) -> bool {
    (0..3).all(|k| same(a[k], b[k]))
}
