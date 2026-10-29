// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Rust ports of the ERFA functions the crate uses, replacing the vendored C.
//!
//! One file per ERFA function. Each carries ERFA's copyright and licence
//! notice, which its BSD terms require of redistributed source, and names
//! the C function and version it was ported from.
//!
//! The per-value form of each function keeps ERFA's operations in ERFA's
//! order, so one-off callers get ERFA's results. A function that callers
//! loop over may also have a vectorizable kernel, within a stated bound of
//! the per-value form on a stated domain, which the slice entry point runs
//! over every element before giving the elements outside that domain to the
//! per-value form. A slice gives each element the same bits whatever the
//! slice's length, so a caller that sends every point through the slice,
//! even a single point, gets results that do not depend on how many points
//! are transformed together.
//!
//! Calls made inside these functions are past the boundary where the
//! `call-counts` feature counts calls, as the C library's counts stop at its
//! calls into ERFA, so they use the `f64` methods directly rather than the
//! counted wrappers in `crate::math`.
#![allow(clippy::disallowed_methods)]
// Ports are `pub(crate)` for callers elsewhere in the crate, and the
// differential tests exercise each of them.
#![allow(dead_code, unused_imports)]
// The constants are `erfam.h`'s literals character for character.
#![allow(clippy::excessive_precision, clippy::approx_constant)]

mod anp;
mod anpm;
mod c2s;
#[cfg(test)]
mod difftest;
mod pas;
mod pdp;
mod pm;
mod pmp;
mod pn;
mod ppp;
mod ppsp;
mod pv2s;
mod pvu;
mod pxp;
mod rm2v;
mod rv2m;
mod rx;
mod rxp;
mod rxpv;
mod rxr;
mod ry;
mod rz;
mod s2c;
mod s2pv;
mod sepp;
mod seps;
mod sxp;
mod tr;
mod trxp;

pub(crate) use anp::anp;
pub(crate) use anpm::anpm;
pub(crate) use pas::pas;
pub(crate) use pdp::pdp;
pub(crate) use pm::pm;
pub(crate) use pmp::pmp;
pub(crate) use pn::pn;
pub(crate) use ppp::ppp;
pub(crate) use ppsp::ppsp;
pub(crate) use pv2s::pv2s;
pub(crate) use pvu::pvu;
pub(crate) use pxp::pxp;
pub(crate) use rm2v::rm2v;
pub(crate) use rv2m::rv2m;
pub(crate) use rx::rx;
pub(crate) use rxp::rxp;
pub(crate) use rxpv::rxpv;
pub(crate) use rxr::rxr;
pub(crate) use ry::ry;
pub(crate) use rz::rz;
pub(crate) use s2pv::s2pv;
pub(crate) use sepp::sepp;
pub(crate) use seps::seps;
pub(crate) use sxp::sxp;
pub(crate) use tr::tr;
pub(crate) use trxp::trxp;

/// A 3-vector.
pub(crate) type V3 = [f64; 3];
/// A 3x3 matrix, row by row.
pub(crate) type M3 = [[f64; 3]; 3];
/// A pv-vector: position, then velocity.
pub(crate) type PV = [[f64; 3]; 2];

pub(crate) const ERFA_DPI: f64 = 3.141592653589793238462643;
pub(crate) const ERFA_D2PI: f64 = 6.283185307179586476925287;
pub(crate) const ERFA_DR2D: f64 = 57.29577951308232087679815;
pub(crate) const ERFA_DD2R: f64 = 1.745329251994329576923691e-2;
pub(crate) const ERFA_DR2AS: f64 = 206264.8062470963551564734;
pub(crate) const ERFA_DAS2R: f64 = 4.848136811095359935899141e-6;
pub(crate) const ERFA_DS2R: f64 = 7.272205216643039903848712e-5;
pub(crate) const ERFA_TURNAS: f64 = 1296000.0;
pub(crate) const ERFA_DMAS2R: f64 = ERFA_DAS2R / 1e3;
pub(crate) const ERFA_DTY: f64 = 365.242198781;
pub(crate) const ERFA_DAYSEC: f64 = 86400.0;
pub(crate) const ERFA_DJY: f64 = 365.25;
pub(crate) const ERFA_DJC: f64 = 36525.0;
pub(crate) const ERFA_DJM: f64 = 365250.0;
pub(crate) const ERFA_DJ00: f64 = 2451545.0;
pub(crate) const ERFA_DJM0: f64 = 2400000.5;
pub(crate) const ERFA_DJM00: f64 = 51544.5;
pub(crate) const ERFA_DJM77: f64 = 43144.0;
pub(crate) const ERFA_TTMTAI: f64 = 32.184;
pub(crate) const ERFA_DAU: f64 = 149597870.7e3;
pub(crate) const ERFA_CMPS: f64 = 299792458.0;
pub(crate) const ERFA_AULT: f64 = ERFA_DAU / ERFA_CMPS;
pub(crate) const ERFA_DC: f64 = ERFA_DAYSEC / ERFA_AULT;
pub(crate) const ERFA_ELG: f64 = 6.969290134e-10;
pub(crate) const ERFA_ELB: f64 = 1.550519768e-8;
pub(crate) const ERFA_TDB0: f64 = -6.55e-5;
pub(crate) const ERFA_SRS: f64 = 1.97412574336e-8;

/// Truncates towards zero.
#[inline]
pub(crate) fn dint(a: f64) -> f64 {
    if a < 0.0 { a.ceil() } else { a.floor() }
}

/// Rounds to the nearest whole number, halves away from zero.
#[inline]
pub(crate) fn dnint(a: f64) -> f64 {
    if a.abs() < 0.5 {
        0.0
    } else if a < 0.0 {
        (a - 0.5).ceil()
    } else {
        (a + 0.5).floor()
    }
}

/// The magnitude of `a` with the sign of `b`; a zero `b` of either sign
/// gives a positive result.
#[inline]
pub(crate) fn dsign(a: f64, b: f64) -> f64 {
    if b < 0.0 { -a.abs() } else { a.abs() }
}

pub(crate) use c2s::{c2s, c2s_kernel, c2s_needs_special};
#[cfg(test)]
pub(crate) use s2c::{S2C_PRODUCT_ULPS, s2c_reference};
pub(crate) use s2c::{s2c, s2c_kernel, s2c_needs_special};

/// The sine and cosine of `x`, as an optimizing C compiler computes them for
/// C code that calls `sin(x)` and `cos(x)`; see [`crate::math::sincos_as_c_uncounted`].
#[inline(always)]
pub(super) fn sincos(x: f64) -> (f64, f64) {
    crate::math::sincos_as_c_uncounted(x)
}
