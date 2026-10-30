// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Rust ports of the ERFA functions the crate uses, replacing the C library AST once compiled.
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
// The constants are `erfam.h`'s literals character for character.
#![allow(clippy::excessive_precision, clippy::approx_constant)]

mod anp;
mod anpm;
mod bpn2xy;
mod c2s;
mod cal2jd;
mod d2tf;
mod dat;
mod ee06a;
mod eform;
mod eors;
mod epb;
mod epb2jd;
mod epj;
mod epj2jd;
mod epv00;
mod era00;
mod fad03;
mod fae03;
mod faf03;
mod faju03;
mod fal03;
mod falp03;
mod fama03;
mod fame03;
mod faom03;
mod fapa03;
mod fasa03;
mod faur03;
mod fave03;
mod fk45z;
mod fk524;
mod fk54z;
mod fk5hip;
mod fk5hz;
mod fw2m;
mod gc2gd;
mod gc2gde;
mod gd2gc;
mod gd2gce;
mod gmst06;
mod gst06;
mod gst06a;
mod hfk5z;
mod jd2cal;
mod nut00a;
mod nut06a;
mod obl06;
mod p06e;
mod pas;
mod pdp;
mod pfw06;
mod pm;
mod pmat06;
mod pmp;
mod pn;
mod pnm06a;
mod ppp;
mod ppsp;
mod pv2s;
mod pvu;
mod pxp;
mod rv2m;
mod rx;
mod rxp;
mod rxpv;
mod rxr;
mod ry;
mod rz;
mod s06;
mod s2c;
mod s2pv;
mod sepp;
mod seps;
mod sxp;
mod tf2d;
mod tr;
mod trxp;

pub(crate) use anp::anp;
pub(crate) use anpm::anpm;
pub(crate) use bpn2xy::bpn2xy;
pub(crate) use cal2jd::{BadDay, Cal2jdError, cal2jd};
pub(crate) use d2tf::d2tf;
#[cfg(test)]
pub(crate) use dat::CHANGES;
pub(crate) use dat::{DRIFT, LEAP_SECONDS, NERA1, dat};
pub(crate) use ee06a::ee06a;
pub(crate) use eform::{Ellipsoid, eform};
pub(crate) use eors::eors;
pub(crate) use epb::epb;
pub(crate) use epb2jd::epb2jd;
pub(crate) use epj::epj;
pub(crate) use epj2jd::epj2jd;
pub(crate) use epv00::epv00;
pub(crate) use era00::era00;
pub(crate) use fad03::fad03;
pub(crate) use fae03::fae03;
pub(crate) use faf03::faf03;
pub(crate) use faju03::faju03;
pub(crate) use fal03::fal03;
pub(crate) use falp03::falp03;
pub(crate) use fama03::fama03;
pub(crate) use fame03::fame03;
pub(crate) use faom03::faom03;
pub(crate) use fapa03::fapa03;
pub(crate) use fasa03::fasa03;
pub(crate) use faur03::faur03;
pub(crate) use fave03::fave03;
pub(crate) use fk5hip::fk5hip;
pub(crate) use fk5hz::fk5hz;
pub(crate) use fk45z::{Fk45zEpoch, fk45z, fk45z_kernel};
pub(crate) use fk54z::{fk54z, fk54z_kernel};
pub(crate) use fk524::fk524;
pub(crate) use fw2m::fw2m;
pub(crate) use gc2gd::gc2gd;
pub(crate) use gc2gde::{Gc2gdeError, gc2gde};
pub(crate) use gd2gc::gd2gc;
pub(crate) use gd2gce::{Gd2gceError, gd2gce};
pub(crate) use gmst06::gmst06;
pub(crate) use gst06::gst06;
pub(crate) use gst06a::gst06a;
pub(crate) use hfk5z::hfk5z;
pub(crate) use jd2cal::{Jd2calError, jd2cal};
pub(crate) use nut00a::nut00a;
pub(crate) use nut06a::nut06a;
pub(crate) use obl06::obl06;
pub(crate) use p06e::p06e;
pub(crate) use pas::pas;
pub(crate) use pdp::pdp;
pub(crate) use pfw06::pfw06;
pub(crate) use pm::pm;
pub(crate) use pmat06::pmat06;
pub(crate) use pmp::pmp;
pub(crate) use pn::pn;
pub(crate) use pnm06a::pnm06a;
pub(crate) use ppp::ppp;
pub(crate) use ppsp::ppsp;
pub(crate) use pv2s::pv2s;
pub(crate) use pvu::pvu;
pub(crate) use pxp::pxp;
pub(crate) use rv2m::rv2m;
pub(crate) use rx::rx;
pub(crate) use rxp::rxp;
pub(crate) use rxpv::rxpv;
pub(crate) use rxr::rxr;
pub(crate) use ry::ry;
pub(crate) use rz::rz;
pub(crate) use s2pv::s2pv;
pub(crate) use s06::s06;
pub(crate) use sepp::sepp;
pub(crate) use seps::seps;
pub(crate) use sxp::sxp;
pub(crate) use tf2d::{Tf2dWarning, tf2d};
pub(crate) use tr::tr;
pub(crate) use trxp::trxp;

/// A 3-vector.
pub(crate) type V3 = [f64; 3];
/// A 3x3 matrix, row by row.
pub(crate) type M3 = [[f64; 3]; 3];
/// A pv-vector: position, then velocity.
pub(crate) type PV = [[f64; 3]; 2];

// The constants of `erfam.h` the ports use, and the ones beside them that
// no port uses yet, which carry `allow(dead_code)`.
pub(crate) const ERFA_DPI: f64 = 3.141592653589793238462643;
pub(crate) const ERFA_D2PI: f64 = 6.283185307179586476925287;
#[allow(dead_code)]
pub(crate) const ERFA_DR2D: f64 = 57.29577951308232087679815;
#[allow(dead_code)]
pub(crate) const ERFA_DD2R: f64 = 1.745329251994329576923691e-2;
pub(crate) const ERFA_DR2AS: f64 = 206264.8062470963551564734;
pub(crate) const ERFA_DAS2R: f64 = 4.848136811095359935899141e-6;
#[allow(dead_code)]
pub(crate) const ERFA_DS2R: f64 = 7.272205216643039903848712e-5;
pub(crate) const ERFA_TURNAS: f64 = 1296000.0;
#[allow(dead_code)]
pub(crate) const ERFA_DMAS2R: f64 = ERFA_DAS2R / 1e3;
pub(crate) const ERFA_DTY: f64 = 365.242198781;
pub(crate) const ERFA_DAYSEC: f64 = 86400.0;
pub(crate) const ERFA_DJY: f64 = 365.25;
pub(crate) const ERFA_DJC: f64 = 36525.0;
#[allow(dead_code)]
pub(crate) const ERFA_DJM: f64 = 365250.0;
pub(crate) const ERFA_DJ00: f64 = 2451545.0;
pub(crate) const ERFA_DJM0: f64 = 2400000.5;
pub(crate) const ERFA_DJM00: f64 = 51544.5;
#[allow(dead_code)]
pub(crate) const ERFA_DJM77: f64 = 43144.0;
#[allow(dead_code)]
pub(crate) const ERFA_TTMTAI: f64 = 32.184;
pub(crate) const ERFA_DAU: f64 = 149597870.7e3;
#[allow(dead_code)]
pub(crate) const ERFA_CMPS: f64 = 299792458.0;
#[allow(dead_code)]
pub(crate) const ERFA_AULT: f64 = ERFA_DAU / ERFA_CMPS;
#[allow(dead_code)]
pub(crate) const ERFA_DC: f64 = ERFA_DAYSEC / ERFA_AULT;
#[allow(dead_code)]
pub(crate) const ERFA_ELG: f64 = 6.969290134e-10;
#[allow(dead_code)]
pub(crate) const ERFA_ELB: f64 = 1.550519768e-8;
#[allow(dead_code)]
pub(crate) const ERFA_TDB0: f64 = -6.55e-5;
#[allow(dead_code)]
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

/// The elementary functions a kernel body takes from outside, so the body
/// is written once and its tests can check every value it uses.
pub(crate) trait KernelTrig {
    /// The sine and cosine of `x`.
    fn sin_cos(x: f64) -> (f64, f64);
    /// The arctangent of `y / x` in the quadrant of (x, y).
    fn atan2(y: f64, x: f64) -> f64;
}

/// [`KernelTrig`] by the vectorizable kernels of [`crate::vmath`], each
/// within 1 ULP of the exact value, rounded, where its `*_covers` predicate
/// holds. `FMA` is passed to [`crate::vmath::atan2`].
pub(crate) struct VmathTrig<const FMA: bool>;

impl<const FMA: bool> KernelTrig for VmathTrig<FMA> {
    #[inline(always)]
    fn sin_cos(x: f64) -> (f64, f64) {
        crate::vmath::sin_cos(x)
    }

    #[inline(always)]
    fn atan2(y: f64, x: f64) -> f64 {
        crate::vmath::atan2::<FMA>(y, x)
    }
}

/// [`VmathTrig`] that asserts, at every argument its kernels cover, that
/// each value is within 1 ULP of the exact value, rounded: for checking the
/// elementary functions inside a kernel body over its test inputs. The
/// references use no libm except to seed the arctangent's refinement.
#[cfg(test)]
pub(crate) struct CheckedTrig;

#[cfg(test)]
impl KernelTrig for CheckedTrig {
    fn sin_cos(x: f64) -> (f64, f64) {
        use crate::test_inputs::{sin_cos_reference, ulps};
        let (s, c) = crate::vmath::sin_cos(x);
        if crate::vmath::sin_cos_covers(x) {
            let (rs, rc) = sin_cos_reference(x);
            assert!(
                ulps(s, rs) <= 1 && ulps(c, rc) <= 1,
                "sin_cos({x:e}) = ({s:e}, {c:e}), exact ({rs:e}, {rc:e})"
            );
        }
        (s, c)
    }

    fn atan2(y: f64, x: f64) -> f64 {
        use crate::test_inputs::{atan2_reference, ulps};
        let a = crate::vmath::atan2::<{ crate::vmath::HAS_FMA }>(y, x);
        if crate::vmath::atan2_covers(y, x) {
            let r = atan2_reference(y, x);
            assert!(ulps(a, r) <= 1, "atan2({y:e}, {x:e}) = {a:e}, exact {r:e}");
        }
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `dint`, `dnint` and `dsign` against `erfam.h`'s macros, written out
    /// with the C library functions' semantics.
    #[test]
    fn rounding_macros_match_erfam_h() {
        let values = [
            0.0,
            -0.0,
            0.5,
            -0.5,
            0.49999999999999994,
            -0.49999999999999994,
            1.5,
            -1.5,
            2.5,
            -2.5,
            1e15 + 0.5,
            4503599627370497.5,
            1e300,
            -1e300,
            f64::MAX,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ];
        for &a in &values {
            // ERFA_DINT: ((A)<0.0?ceil(A):floor(A))
            let dint_c = if a < 0.0 { a.ceil() } else { a.floor() };
            // ERFA_DNINT: (fabs(A)<0.5?0.0:(A)<0.0?ceil((A)-0.5):floor((A)+0.5))
            let dnint_c = if a.abs() < 0.5 {
                0.0
            } else if a < 0.0 {
                (a - 0.5).ceil()
            } else {
                (a + 0.5).floor()
            };
            assert_eq!(dint(a).to_bits(), dint_c.to_bits(), "dint {a:e}");
            assert_eq!(dnint(a).to_bits(), dnint_c.to_bits(), "dnint {a:e}");
            for &b in &values {
                // ERFA_DSIGN: ((B)<0.0?-fabs(A):fabs(A))
                let c = if b < 0.0 { -a.abs() } else { a.abs() };
                assert_eq!(dsign(a, b).to_bits(), c.to_bits(), "dsign {a:e} {b:e}");
            }
        }
        // Spot values with their known results.
        assert_eq!(dint(-2.7), -2.0);
        assert_eq!(dint(2.7), 2.0);
        assert_eq!(dnint(2.5), 3.0);
        assert_eq!(dnint(-2.5), -3.0);
        assert_eq!(dnint(-0.4).to_bits(), 0.0_f64.to_bits());
        // The sum a + 0.5 rounds up for the largest double below 0.5, so a
        // value just under a half-integer boundary shows the macro's form.
        assert_eq!(dnint(0.49999999999999994), 0.0);
        assert_eq!(dnint(1.4999999999999998), 1.0);
        assert_eq!(dsign(3.0, -0.0), 3.0);
        assert_eq!(dsign(-3.0, -1.0), -3.0);
    }
}
