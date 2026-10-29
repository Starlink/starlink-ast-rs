// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The floating-point library functions the projections call.
//!
//! Calls made inside the projections are past the boundary where the
//! `call-counts` feature counts calls, as the C library's counts stop at its
//! calls into wcslib (`wcs_fwd` and `wcs_rev`), so these are the `f64`
//! methods, uncounted, under the names of the counted wrappers in
//! `crate::math`. Each is the method it wraps, so results are bit-identical.
#![allow(clippy::disallowed_methods)]

#[inline(always)]
pub(super) fn sin(x: f64) -> f64 {
    x.sin()
}

#[inline(always)]
pub(super) fn cos(x: f64) -> f64 {
    x.cos()
}

#[inline(always)]
pub(super) fn tan(x: f64) -> f64 {
    x.tan()
}

#[inline(always)]
pub(super) fn asin(x: f64) -> f64 {
    x.asin()
}

#[inline(always)]
pub(super) fn acos(x: f64) -> f64 {
    x.acos()
}

#[inline(always)]
pub(super) fn atan(x: f64) -> f64 {
    x.atan()
}

#[inline(always)]
pub(super) fn atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}

#[inline(always)]
pub(super) fn exp(x: f64) -> f64 {
    x.exp()
}

#[inline(always)]
pub(super) fn ln(x: f64) -> f64 {
    x.ln()
}

#[inline(always)]
pub(super) fn powf(x: f64, y: f64) -> f64 {
    x.powf(y)
}

#[inline(always)]
pub(super) fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// The sine and cosine of `x`, as an optimizing C compiler computes them for
/// C code that calls `sin(x)` and `cos(x)`; see
/// [`crate::math::sincos_as_c_uncounted`].
#[inline(always)]
pub(super) fn sincos_as_c(x: f64) -> (f64, f64) {
    crate::math::sincos_as_c_uncounted(x)
}
