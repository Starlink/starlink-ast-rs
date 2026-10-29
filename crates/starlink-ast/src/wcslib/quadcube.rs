// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from WCSLIB (AST's fork of wcslib 2.9, with fixes from wcslib 8.10): Copyright 1995-2002 Mark Calabretta, GNU Library General Public License version 2 or later; this port is distributed under LGPL-3.0-or-later.

//! The quad-cube projections: TSC, CSC and QSC.
//!
//! Each projection is a set-up function that derives constants from the
//! parameters in a [`PrjPrm`], and a forward and a reverse function generic
//! over the trigonometry [`Trig`].
//! The bounds tests apply unconditionally, as [`super::prj`] explains.
//!
//! # CSC's single precision
//!
//! CSC works in single precision.
//! Its working variables and coefficients are `f32`, and an expression of
//! `f32` operands is evaluated in single precision.
//! Where an `f32` meets an `f64` value, the expression is evaluated in
//! `f64` and rounded to `f32` once, on assignment.
//! A coefficient is converted from its `f64` value rather than parsed
//! directly as `f32`, so it rounds twice.

// The single-precision rules follow the C, whose `float` variables meet
// `double` literals such as `1.0` and so widen those expressions to double.
// The reference build disables floating-point contraction, so no multiply-add
// is fused.

// Lineage: every function below is ported from `reference/starlink-ast/wcslib/proj.c`
// (`reference/starlink-ast` at 3d3f255f); the `flag > 0` tests of the C
// bodies are applied unconditionally.

use super::math;

use super::prj::{PrjPrm, Stage, Status};
use super::trig::{D2R, Kernel, R2D, Trig, sincosd_covers};
use super::zenithal::c_copysign;
use std::f64::consts::PI;

/// `SQRT2INV` of `wcsmath.h`.
const SQRT2INV: f64 = 1.0 / std::f64::consts::SQRT_2;

// The constants the three set-up functions of this module derive: wcslib
// writes the same body three times.
fn quadcube_set(prm: &mut PrjPrm) {
    prm.phi0 = 0.0;
    prm.theta0 = 0.0;

    if prm.r0 == 0.0 {
        prm.r0 = R2D;
        prm.w[0] = 45.0;
        prm.w[1] = 1.0 / 45.0;
    } else {
        prm.w[0] = prm.r0 * PI / 4.0;
        prm.w[1] = 1.0 / prm.w[0];
    }
}

/// The cube face of the direction cosines `(l, m, n)` and the largest of
/// `n`, `l`, `m`, `-l`, `-m`, `-n` (the first of equals wins), generic over
/// the precision C declares them in.
macro_rules! cube_face {
    ($l:expr, $m:expr, $n:expr) => {{
        let (l, m, n) = ($l, $m, $n);
        let mut face = 0;
        let mut rho = n;
        if l > rho {
            face = 1;
            rho = l;
        }
        if m > rho {
            face = 2;
            rho = m;
        }
        if -l > rho {
            face = 3;
            rho = -l;
        }
        if -m > rho {
            face = 4;
            rho = -m;
        }
        if -n > rho {
            face = 5;
            rho = -n;
        }
        (face, rho)
    }};
}

/// The face of the reverse projection's scaled (x, y), the position within
/// it, and `None` for a point outside the projection's bounds.
///
/// The code the three reverse functions share verbatim, up to the type of
/// `xf` and `yf`; the bounds tests and the face tests are C's.
macro_rules! rev_face {
    ($xf:ident, $yf:ident, $t:ty) => {{
        // Check bounds.
        let mut out_of_bounds = false;
        if ($xf as f64).abs() <= 1.0 {
            if ($yf as f64).abs() > 3.0 {
                out_of_bounds = true;
            }
        } else if ($xf as f64).abs() > 7.0 || ($yf as f64).abs() > 1.0 {
            out_of_bounds = true;
        }
        if out_of_bounds {
            None
        } else {
            // Map negative faces to the other side.
            if ($xf as f64) < -1.0 {
                $xf = (($xf as f64) + 8.0) as $t;
            }

            // Determine the face.
            let face;
            if ($xf as f64) > 5.0 {
                face = 4;
                $xf = (($xf as f64) - 6.0) as $t;
            } else if ($xf as f64) > 3.0 {
                face = 3;
                $xf = (($xf as f64) - 4.0) as $t;
            } else if ($xf as f64) > 1.0 {
                face = 2;
                $xf = (($xf as f64) - 2.0) as $t;
            } else if ($yf as f64) > 1.0 {
                face = 0;
                $yf = (($yf as f64) - 2.0) as $t;
            } else if ($yf as f64) < -1.0 {
                face = 5;
                $yf = (($yf as f64) + 2.0) as $t;
            } else {
                face = 1;
            }
            Some(face)
        }
    }};
}

// ---------------------------------------------------------------------------
// TSC: tangential spherical cube
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astTSCset; reference/starlink-ast at 3d3f255f.
pub(super) fn tsc_set(prm: &mut PrjPrm) -> Status {
    quadcube_set(prm);
    Status::Ok
}

// astTSCfwd: a coordinate may exceed 1 by this much for rounding.
const TSC_FWD_TOL: f64 = 1.0e-12;

// reference/starlink-ast/wcslib/proj.c, astTSCfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn tsc_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    tsc_fwd_rest(prm, tsc_fwd_front::<T>(prm, phi, theta))
}

/// The first stage of [`tsc_fwd`]: [`Status::Ok`] and the direction cosines
/// `(l, m, n)` of (`phi`, `theta`).
#[inline(always)]
pub(super) fn tsc_fwd_front<T: Trig>(_prm: &PrjPrm, phi: f64, theta: f64) -> Stage {
    let cthe = T::cosd(theta);
    let l = cthe * T::cosd(phi);
    let m = cthe * T::sind(phi);
    let n = T::sind(theta);
    (Status::Ok, l, m, n)
}

/// Whether [`tsc_fwd_front`] or [`csc_fwd_front`] needs
/// [`Libm`](super::trig::Libm) at (`phi`, `theta`): a sine or cosine argument
/// outside the kernels' cover.
// The stage has no status test and no branch; the face, which the drift can
// change, is chosen per point by `tsc_fwd_finish` and `csc_fwd_finish`.
#[inline(always)]
pub(super) fn quadcube_fwd_needs_special<T: Trig>(_prm: &PrjPrm, phi: f64, theta: f64) -> bool {
    !sincosd_covers(theta) | !sincosd_covers(phi)
}

/// The rest of [`tsc_fwd`], given the direction cosines from
/// [`tsc_fwd_front`]: the face, the position on it, and the status.
#[inline(always)]
fn tsc_fwd_rest(prm: &PrjPrm, (_, l, m, n): Stage) -> (Status, f64, f64) {
    let (face, rho) = cube_face!(l, m, n);

    let mut xf;
    let mut yf;
    let x0;
    let y0;
    if face == 0 {
        xf = m / rho;
        yf = -l / rho;
        x0 = 0.0;
        y0 = 2.0;
    } else if face == 1 {
        xf = m / rho;
        yf = n / rho;
        x0 = 0.0;
        y0 = 0.0;
    } else if face == 2 {
        xf = -l / rho;
        yf = n / rho;
        x0 = 2.0;
        y0 = 0.0;
    } else if face == 3 {
        xf = -m / rho;
        yf = n / rho;
        x0 = 4.0;
        y0 = 0.0;
    } else if face == 4 {
        xf = l / rho;
        yf = n / rho;
        x0 = 6.0;
        y0 = 0.0;
    } else {
        xf = m / rho;
        yf = l / rho;
        x0 = 0.0;
        y0 = -2.0;
    }

    if xf.abs() > 1.0 {
        if xf.abs() > 1.0 + TSC_FWD_TOL {
            return (Status::BadPoint, 0.0, 0.0);
        }
        xf = c_copysign(1.0, xf);
    }
    if yf.abs() > 1.0 {
        if yf.abs() > 1.0 + TSC_FWD_TOL {
            return (Status::BadPoint, 0.0, 0.0);
        }
        yf = c_copysign(1.0, yf);
    }

    (Status::Ok, prm.w[0] * (xf + x0), prm.w[0] * (yf + y0))
}

/// [`tsc_fwd`]'s second stage in a slice: [`tsc_fwd_rest`] on the kernels'
/// direction cosines, or `None` where their drift could change the face.
// The face is decided by `cube_face_near_tie`. The bounds tests never pass,
// on either path: `xf` and `yf` are quotients by `rho` of direction cosines
// no larger than `rho` in magnitude, `rho` being one of them, and a correctly
// rounded quotient of such values is at most 1 in magnitude. So on the face
// both paths choose, the status is `Ok` on both.
#[inline(always)]
pub(super) fn tsc_fwd_finish(
    prm: &PrjPrm,
    _phi: f64,
    _theta: f64,
    stage: Stage,
) -> Option<(Status, f64, f64)> {
    if cube_face_near_tie(stage.1, stage.2, stage.3) {
        None
    } else {
        Some(tsc_fwd_rest(prm, stage))
    }
}

/// Whether the drift of the kernels' direction cosines `(l, m, n)` could
/// change the cube face chosen from them, the first of the largest of `n`,
/// `l`, `m`, `-l`, `-m` and `-n`.
// `l = cthe cphi` and `m = cthe sphi` are products of two kernel values,
// each within 2 ULP of Libm's (see `Kernel`), at most 2 eps of it (eps =
// 2^-52), with the two paths' roundings of the product, eps between them:
// each differs by at most 5 eps of itself. `n`, a kernel value, differs by 2
// eps of itself, less than 5. Where the chosen candidate `c_f` exceeds every
// other `c_i` by more than 5 eps (|c_f| + |c_i|), Libm's `c_f` exceeds Libm's
// `c_i` too, so Libm chooses the same face whatever the order of the
// candidates. The margin is twice that, which covers its own evaluation from
// the kernels' values rather than Libm's.
#[inline(always)]
pub(super) fn cube_face_near_tie(l: f64, m: f64, n: f64) -> bool {
    let (face, rho) = cube_face!(l, m, n);
    let mut near = false;
    for (i, c) in [n, l, m, -l, -m, -n].into_iter().enumerate() {
        // False for a NaN, which is then near.
        let clear = rho - c > 10.0 * f64::EPSILON * (rho.abs() + c.abs());
        near |= i != face && !clear;
    }
    near
}

// reference/starlink-ast/wcslib/proj.c, astTSCrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn tsc_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let mut xf = x * prm.w[1];
    let mut yf = y * prm.w[1];

    let Some(face) = rev_face!(xf, yf, f64) else {
        return (Status::BadPoint, 0.0, 0.0);
    };

    let l;
    let m;
    let n;
    if face == 4 {
        m = -1.0 / math::sqrt(1.0 + xf * xf + yf * yf);
        l = -m * xf;
        n = -m * yf;
    } else if face == 3 {
        l = -1.0 / math::sqrt(1.0 + xf * xf + yf * yf);
        m = l * xf;
        n = -l * yf;
    } else if face == 2 {
        m = 1.0 / math::sqrt(1.0 + xf * xf + yf * yf);
        l = -m * xf;
        n = m * yf;
    } else if face == 0 {
        n = 1.0 / math::sqrt(1.0 + xf * xf + yf * yf);
        l = -n * yf;
        m = n * xf;
    } else if face == 5 {
        n = -1.0 / math::sqrt(1.0 + xf * xf + yf * yf);
        l = -n * yf;
        m = -n * xf;
    } else {
        l = 1.0 / math::sqrt(1.0 + xf * xf + yf * yf);
        m = l * xf;
        n = l * yf;
    }

    let phi = if l == 0.0 && m == 0.0 {
        0.0
    } else {
        T::atan2d(m, l)
    };
    let theta = T::asind(n);

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// CSC: COBE quadrilateralized spherical cube
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astCSCset; reference/starlink-ast at 3d3f255f.
pub(super) fn csc_set(prm: &mut PrjPrm) -> Status {
    quadcube_set(prm);
    Status::Ok
}

/// A `float` constant as C initialises it: from a `double` literal.
const fn float_of(v: f64) -> f32 {
    v as f32
}

// astCSCfwd: a coordinate may exceed 1 by this much for rounding (a
// `float` in C).
const CSC_FWD_TOL: f32 = float_of(1.0e-7);
// astCSCfwd: the squares below this are taken as zero, "to avoid floating
// underflows" in the C.
const CSC_FWD_UNDERFLOW: f64 = 1.0e-16;
// astCSCfwd: the coefficients of the polynomial that maps the tangent
// plane onto the cube face, `float` in C.
const CSC_GSTAR: f32 = float_of(1.37484847732);
const CSC_MM: f32 = float_of(0.004869491981);
const CSC_GAMMA: f32 = float_of(-0.13161671474);
const CSC_OMEGA1: f32 = float_of(-0.159596235474);
const CSC_D0: f32 = float_of(0.0759196200467);
const CSC_D1: f32 = float_of(-0.0217762490699);
const CSC_C00: f32 = float_of(0.141189631152);
const CSC_C10: f32 = float_of(0.0809701286525);
const CSC_C01: f32 = float_of(-0.281528535557);
const CSC_C11: f32 = float_of(0.15384112876);
const CSC_C20: f32 = float_of(-0.178251207466);
const CSC_C02: f32 = float_of(0.106959469314);

// reference/starlink-ast/wcslib/proj.c, astCSCfwd; reference/starlink-ast at 3d3f255f.
// The C works in `float` throughout; see the module documentation.
#[inline(always)]
pub(super) fn csc_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    csc_fwd_rest(prm, csc_fwd_front::<T>(prm, phi, theta))
}

/// The first stage of [`csc_fwd`]: [`Status::Ok`] and the direction cosines
/// `(l, m, n)` of (`phi`, `theta`) before their rounding to single
/// precision, `l` and `m` from the cosine of `theta` in single precision.
#[inline(always)]
pub(super) fn csc_fwd_front<T: Trig>(_prm: &PrjPrm, phi: f64, theta: f64) -> Stage {
    let cthe = T::cosd(theta) as f32;
    let l = cthe as f64 * T::cosd(phi);
    let m = cthe as f64 * T::sind(phi);
    let n = T::sind(theta);
    (Status::Ok, l, m, n)
}

/// The smallest magnitude [`f32_rounding_near`] tests; below it, it reports
/// every value near. 2^-100, far above the least normal `f32`.
const F32_ROUNDING_FLOOR: f64 = f64::from_bits(923 << 52);

/// Whether rounding the double `v` to single precision could give another
/// value for a double within `margin` units in the last place of `v`.
// A double from F32_ROUNDING_FLOOR to 1 in magnitude rounds to a normal
// single-precision value, which keeps the top 24 bits of its 53-bit
// significand, so the rounding depends on the low 29 bits: the double
// rounds down below the halfway pattern 2^28, up above it, and to even on
// it. Two doubles round alike unless that pattern lies between them or on
// one. A double near the top of a binade, whose low bits are near 2^29,
// rounds up to the power of 2 above, as does one just above that power,
// whose low bits are near 0, so a binade's edge is no boundary. So `v` and
// any double within `margin` ULP of it round alike where the low bits are
// more than `margin` from 2^28.
#[inline(always)]
pub(super) fn f32_rounding_near(v: f64, margin: u64) -> bool {
    const LOW_BITS: u64 = (1 << 29) - 1;
    const HALFWAY: u64 = 1 << 28;
    let low = v.to_bits() & LOW_BITS;
    // `low` is outside [HALFWAY - margin, HALFWAY + margin] exactly when its
    // wrapping difference from the lower end exceeds the window's width.
    let outside = low.wrapping_sub(HALFWAY - margin) > 2 * margin;
    // False for a NaN, which is then near.
    let clear = outside & (v.abs() >= F32_ROUNDING_FLOOR);
    !clear
}

/// [`csc_fwd`]'s second stage in a slice: [`csc_fwd_rest`] on the kernels'
/// direction cosines, or `None` where their drift could change a value's
/// rounding to single precision.
// Everything after the rounding is computed from the single-precision
// values, so where they are Libm's, the face, the status and the outputs
// are Libm's bit for bit. The cosine of `theta`, which the stage rounds
// before it multiplies, is recomputed here with the scalar kernel, which
// gives the vectorized one's bits: Rust never contracts a multiply and an
// add into a fused multiply-add, and `vmath::sin_cos` calls no `mul_add`, so
// its operations and their roundings are the same in scalar and vector
// code. It and `n` are kernel values, within 2
// ULP of Libm's (see `Kernel`). `l` and `m` are that cosine in single
// precision, alike on both paths where its rounding is, times a kernel
// value, rounded once on each path: within 3 eps of themselves (eps =
// 2^-52), which is at most 6 ULP, since an ULP of a double is more than eps
// / 2 of it. Each is tested against twice its drift, which covers
// evaluating the test from the kernels' value rather than Libm's.
#[inline(always)]
pub(super) fn csc_fwd_finish(
    prm: &PrjPrm,
    _phi: f64,
    theta: f64,
    stage: Stage,
) -> Option<(Status, f64, f64)> {
    let (_, l, m, n) = stage;
    let cthe = Kernel::<false>::cosd(theta);
    let near = f32_rounding_near(cthe, 4)
        | f32_rounding_near(l, 12)
        | f32_rounding_near(m, 12)
        | f32_rounding_near(n, 4);
    if near {
        None
    } else {
        Some(csc_fwd_rest(prm, stage))
    }
}

/// The rest of [`csc_fwd`], given the direction cosines from
/// [`csc_fwd_front`]: the face, the position on it, and the status.
#[inline(always)]
fn csc_fwd_rest(prm: &PrjPrm, (_, l, m, n): Stage) -> (Status, f64, f64) {
    let (l, m, n) = (l as f32, m as f32, n as f32);
    let (face, rho) = cube_face!(l, m, n);

    let xi: f32;
    let eta: f32;
    let x0: f32;
    let y0: f32;
    if face == 0 {
        xi = m;
        eta = -l;
        x0 = 0.0;
        y0 = 2.0;
    } else if face == 1 {
        xi = m;
        eta = n;
        x0 = 0.0;
        y0 = 0.0;
    } else if face == 2 {
        xi = -l;
        eta = n;
        x0 = 2.0;
        y0 = 0.0;
    } else if face == 3 {
        xi = -m;
        eta = n;
        x0 = 4.0;
        y0 = 0.0;
    } else if face == 4 {
        xi = l;
        eta = n;
        x0 = 6.0;
        y0 = 0.0;
    } else {
        xi = m;
        eta = l;
        x0 = 0.0;
        y0 = -2.0;
    }

    let a = xi / rho;
    let b = eta / rho;

    let a2 = a * a;
    let b2 = b * b;
    let ca2 = (1.0 - a2 as f64) as f32;
    let cb2 = (1.0 - b2 as f64) as f32;

    // Avoid floating underflows.
    let ab = ((a * b) as f64).abs() as f32;
    let a4 = if a2 as f64 > CSC_FWD_UNDERFLOW {
        a2 * a2
    } else {
        0.0
    };
    let b4 = if b2 as f64 > CSC_FWD_UNDERFLOW {
        b2 * b2
    } else {
        0.0
    };
    let a2b2 = if ab as f64 > CSC_FWD_UNDERFLOW {
        a2 * b2
    } else {
        0.0
    };

    let mut xf = a
        * (a2
            + ca2
                * (CSC_GSTAR
                    + b2 * (CSC_GAMMA * ca2
                        + CSC_MM * a2
                        + cb2
                            * (CSC_C00
                                + CSC_C10 * a2
                                + CSC_C01 * b2
                                + CSC_C11 * a2b2
                                + CSC_C20 * a4
                                + CSC_C02 * b4))
                    + a2 * (CSC_OMEGA1 - ca2 * (CSC_D0 + CSC_D1 * a2))));
    let mut yf = b
        * (b2
            + cb2
                * (CSC_GSTAR
                    + a2 * (CSC_GAMMA * cb2
                        + CSC_MM * b2
                        + ca2
                            * (CSC_C00
                                + CSC_C10 * b2
                                + CSC_C01 * a2
                                + CSC_C11 * a2b2
                                + CSC_C20 * b4
                                + CSC_C02 * a4))
                    + b2 * (CSC_OMEGA1 - cb2 * (CSC_D0 + CSC_D1 * b2))));

    if (xf as f64).abs() > 1.0 {
        if (xf as f64).abs() > 1.0 + CSC_FWD_TOL as f64 {
            return (Status::BadPoint, 0.0, 0.0);
        }
        xf = c_copysign(1.0, xf as f64) as f32;
    }
    if (yf as f64).abs() > 1.0 {
        if (yf as f64).abs() > 1.0 + CSC_FWD_TOL as f64 {
            return (Status::BadPoint, 0.0, 0.0);
        }
        yf = c_copysign(1.0, yf as f64) as f32;
    }

    (
        Status::Ok,
        prm.w[0] * (x0 + xf) as f64,
        prm.w[0] * (y0 + yf) as f64,
    )
}

// astCSCrev: the coefficients of the polynomial that maps the cube face
// onto the tangent plane, `float` in C.
const CSC_P00: f32 = float_of(-0.27292696);
const CSC_P10: f32 = float_of(-0.07629969);
const CSC_P20: f32 = float_of(-0.22797056);
const CSC_P30: f32 = float_of(0.54852384);
const CSC_P40: f32 = float_of(-0.62930065);
const CSC_P50: f32 = float_of(0.25795794);
const CSC_P60: f32 = float_of(0.02584375);
const CSC_P01: f32 = float_of(-0.02819452);
const CSC_P11: f32 = float_of(-0.01471565);
const CSC_P21: f32 = float_of(0.48051509);
const CSC_P31: f32 = float_of(-1.74114454);
const CSC_P41: f32 = float_of(1.71547508);
const CSC_P51: f32 = float_of(-0.53022337);
const CSC_P02: f32 = float_of(0.27058160);
const CSC_P12: f32 = float_of(-0.56800938);
const CSC_P22: f32 = float_of(0.30803317);
const CSC_P32: f32 = float_of(0.98938102);
const CSC_P42: f32 = float_of(-0.83180469);
const CSC_P03: f32 = float_of(-0.60441560);
const CSC_P13: f32 = float_of(1.50880086);
const CSC_P23: f32 = float_of(-0.93678576);
const CSC_P33: f32 = float_of(0.08693841);
const CSC_P04: f32 = float_of(0.93412077);
const CSC_P14: f32 = float_of(-1.41601920);
const CSC_P24: f32 = float_of(0.33887446);
const CSC_P05: f32 = float_of(-0.63915306);
const CSC_P15: f32 = float_of(0.52032238);
const CSC_P06: f32 = float_of(0.14381585);

/// astCSCrev's polynomial in `u = s*s`, `v = t*t`: the series C writes out
/// twice, once with the roles of `xx` and `yy` exchanged.
#[inline(always)]
fn csc_series(u: f32, v: f32) -> f32 {
    let z0 = CSC_P00
        + u * (CSC_P10
            + u * (CSC_P20 + u * (CSC_P30 + u * (CSC_P40 + u * (CSC_P50 + u * CSC_P60)))));
    let z1 = CSC_P01 + u * (CSC_P11 + u * (CSC_P21 + u * (CSC_P31 + u * (CSC_P41 + u * CSC_P51))));
    let z2 = CSC_P02 + u * (CSC_P12 + u * (CSC_P22 + u * (CSC_P32 + u * CSC_P42)));
    let z3 = CSC_P03 + u * (CSC_P13 + u * (CSC_P23 + u * CSC_P33));
    let z4 = CSC_P04 + u * (CSC_P14 + u * CSC_P24);
    let z5 = CSC_P05 + u * CSC_P15;
    let z6 = CSC_P06;

    z0 + v * (z1 + v * (z2 + v * (z3 + v * (z4 + v * (z5 + v * z6)))))
}

// reference/starlink-ast/wcslib/proj.c, astCSCrev; reference/starlink-ast at 3d3f255f.
// The C works in `float` throughout; see the module documentation.
#[inline(always)]
pub(super) fn csc_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let mut xf = (x * prm.w[1]) as f32;
    let mut yf = (y * prm.w[1]) as f32;

    let Some(face) = rev_face!(xf, yf, f32) else {
        return (Status::BadPoint, 0.0, 0.0);
    };

    let xx = xf * xf;
    let yy = yf * yf;

    // `xf + xf*(1.0 - xx)*a` mixes in double literals: it is evaluated in
    // double precision and rounded once.
    let mut a = csc_series(xx, yy);
    a = (xf as f64 + xf as f64 * (1.0 - xx as f64) * a as f64) as f32;

    let mut b = csc_series(yy, xx);
    b = (yf as f64 + yf as f64 * (1.0 - yy as f64) * b as f64) as f32;

    // `a*a + b*b` is single precision; adding 1.0 makes the rest double.
    let root = math::sqrt((a * a + b * b) as f64 + 1.0);
    let mut l: f32 = 0.0;
    let mut m: f32 = 0.0;
    let mut n: f32 = 0.0;
    if face == 0 {
        n = (1.0 / root) as f32;
        l = -b * n;
        m = a * n;
    } else if face == 1 {
        l = (1.0 / root) as f32;
        m = a * l;
        n = b * l;
    } else if face == 2 {
        m = (1.0 / root) as f32;
        l = -a * m;
        n = b * m;
    } else if face == 3 {
        l = (-1.0 / root) as f32;
        m = a * l;
        n = -b * l;
    } else if face == 4 {
        m = (-1.0 / root) as f32;
        l = -a * m;
        n = -b * m;
    } else if face == 5 {
        n = (-1.0 / root) as f32;
        l = -b * n;
        m = -a * n;
    }

    let phi = if l == 0.0 && m == 0.0 {
        0.0
    } else {
        T::atan2d(m as f64, l as f64)
    };
    let theta = T::asind(n as f64);

    (Status::Ok, phi, theta)
}

// ---------------------------------------------------------------------------
// QSC: quadrilateralized spherical cube
// ---------------------------------------------------------------------------

// reference/starlink-ast/wcslib/proj.c, astQSCset; reference/starlink-ast at 3d3f255f.
pub(super) fn qsc_set(prm: &mut PrjPrm) -> Status {
    quadcube_set(prm);
    Status::Ok
}

// astQSCfwd: a coordinate may exceed 1 by this much for rounding.
const QSC_FWD_TOL: f64 = 1.0e-12;
// astQSCfwd: below this `1 - rho` is computed from the angles instead, "small
// angle formula" in the C.
const QSC_SMALL_ANGLE: f64 = 1.0e-8;

// reference/starlink-ast/wcslib/proj.c, astQSCfwd; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn qsc_fwd<T: Trig>(prm: &PrjPrm, phi: f64, theta: f64) -> (Status, f64, f64) {
    if theta.abs() == 90.0 {
        return (Status::Ok, 0.0, c_copysign(2.0 * prm.w[0], theta));
    }

    let cthe = T::cosd(theta);
    let l = cthe * T::cosd(phi);
    let m = cthe * T::sind(phi);
    let n = T::sind(theta);

    let (face, rho) = cube_face!(l, m, n);

    let mut rhu = 1.0 - rho;

    let (xi, eta, x0, y0) = if face == 0 {
        if rhu < QSC_SMALL_ANGLE {
            // Small angle formula.
            let t = (90.0 - theta) * D2R;
            rhu = t * t / 2.0;
        }
        (m, -l, 0.0, 2.0)
    } else if face == 1 {
        if rhu < QSC_SMALL_ANGLE {
            // Small angle formula.
            let t = theta * D2R;
            let mut p = phi % 360.0;
            if p < -180.0 {
                p += 360.0;
            }
            if p > 180.0 {
                p -= 360.0;
            }
            p *= D2R;
            rhu = (p * p + t * t) / 2.0;
        }
        (m, n, 0.0, 0.0)
    } else if face == 2 {
        if rhu < QSC_SMALL_ANGLE {
            // Small angle formula.
            let t = theta * D2R;
            let mut p = phi % 360.0;
            if p < -180.0 {
                p += 360.0;
            }
            p = (90.0 - p) * D2R;
            rhu = (p * p + t * t) / 2.0;
        }
        (-l, n, 2.0, 0.0)
    } else if face == 3 {
        if rhu < QSC_SMALL_ANGLE {
            // Small angle formula.
            let t = theta * D2R;
            let mut p = phi % 360.0;
            if p < 0.0 {
                p += 360.0;
            }
            p = (180.0 - p) * D2R;
            rhu = (p * p + t * t) / 2.0;
        }
        (-m, n, 4.0, 0.0)
    } else if face == 4 {
        if rhu < QSC_SMALL_ANGLE {
            // Small angle formula.
            let t = theta * D2R;
            let mut p = phi % 360.0;
            if p > 180.0 {
                p -= 360.0;
            }
            p = (90.0 + p) * D2R;
            rhu = (p * p + t * t) / 2.0;
        }
        (l, n, 6.0, 0.0)
    } else {
        if rhu < QSC_SMALL_ANGLE {
            // Small angle formula.
            let t = (90.0 + theta) * D2R;
            rhu = t * t / 2.0;
        }
        (m, l, 0.0, -2.0)
    };

    let mut xf = 0.0;
    let mut yf = 0.0;
    if xi == 0.0 && eta == 0.0 {
        xf = 0.0;
        yf = 0.0;
    } else if -xi >= eta.abs() {
        let omega = eta / xi;
        let tau = 1.0 + omega * omega;
        xf = -math::sqrt(rhu / (1.0 - 1.0 / math::sqrt(1.0 + tau)));
        yf = (xf / 15.0) * (T::atand(omega) - T::asind(omega / math::sqrt(tau + tau)));
    } else if xi >= eta.abs() {
        let omega = eta / xi;
        let tau = 1.0 + omega * omega;
        xf = math::sqrt(rhu / (1.0 - 1.0 / math::sqrt(1.0 + tau)));
        yf = (xf / 15.0) * (T::atand(omega) - T::asind(omega / math::sqrt(tau + tau)));
    } else if -eta > xi.abs() {
        let omega = xi / eta;
        let tau = 1.0 + omega * omega;
        yf = -math::sqrt(rhu / (1.0 - 1.0 / math::sqrt(1.0 + tau)));
        xf = (yf / 15.0) * (T::atand(omega) - T::asind(omega / math::sqrt(tau + tau)));
    } else if eta > xi.abs() {
        let omega = xi / eta;
        let tau = 1.0 + omega * omega;
        yf = math::sqrt(rhu / (1.0 - 1.0 / math::sqrt(1.0 + tau)));
        xf = (yf / 15.0) * (T::atand(omega) - T::asind(omega / math::sqrt(tau + tau)));
    }

    if xf.abs() > 1.0 {
        if xf.abs() > 1.0 + QSC_FWD_TOL {
            return (Status::BadPoint, 0.0, 0.0);
        }
        xf = c_copysign(1.0, xf);
    }
    if yf.abs() > 1.0 {
        if yf.abs() > 1.0 + QSC_FWD_TOL {
            return (Status::BadPoint, 0.0, 0.0);
        }
        yf = c_copysign(1.0, yf);
    }

    (Status::Ok, prm.w[0] * (xf + x0), prm.w[0] * (yf + y0))
}

// astQSCrev: `rho` may fall below -1 by this much for rounding.
const QSC_REV_TOL: f64 = 1.0e-12;

// reference/starlink-ast/wcslib/proj.c, astQSCrev; reference/starlink-ast at 3d3f255f.
#[inline(always)]
pub(super) fn qsc_rev<T: Trig>(prm: &PrjPrm, x: f64, y: f64) -> (Status, f64, f64) {
    let mut xf = x * prm.w[1];
    let mut yf = y * prm.w[1];

    let Some(face) = rev_face!(xf, yf, f64) else {
        return (Status::BadPoint, 0.0, 0.0);
    };

    let direct = xf.abs() > yf.abs();
    let omega;
    let tau;
    let mut rho;
    let rhu;
    if direct {
        if xf == 0.0 {
            omega = 0.0;
            tau = 1.0;
            rho = 1.0;
            rhu = 0.0;
        } else {
            let w = 15.0 * yf / xf;
            omega = T::sind(w) / (T::cosd(w) - SQRT2INV);
            tau = 1.0 + omega * omega;
            rhu = xf * xf * (1.0 - 1.0 / math::sqrt(1.0 + tau));
            rho = 1.0 - rhu;
        }
    } else if yf == 0.0 {
        omega = 0.0;
        tau = 1.0;
        rho = 1.0;
        rhu = 0.0;
    } else {
        let w = 15.0 * xf / yf;
        omega = T::sind(w) / (T::cosd(w) - SQRT2INV);
        tau = 1.0 + omega * omega;
        rhu = yf * yf * (1.0 - 1.0 / math::sqrt(1.0 + tau));
        rho = 1.0 - rhu;
    }

    let w;
    if rho < -1.0 {
        if rho < -1.0 - QSC_REV_TOL {
            return (Status::BadPoint, 0.0, 0.0);
        }

        // C also sets `rhu` to 2.0 here; nothing reads it afterwards.
        rho = -1.0;
        w = 0.0;
    } else {
        w = math::sqrt(rhu * (2.0 - rhu) / tau);
    }

    let mut l = 0.0;
    let mut m = 0.0;
    let mut n = 0.0;
    if face == 0 {
        n = rho;
        if direct {
            m = w;
            if xf < 0.0 {
                m = -m;
            }
            l = -m * omega;
        } else {
            l = w;
            if yf > 0.0 {
                l = -l;
            }
            m = -l * omega;
        }
    } else if face == 1 {
        l = rho;
        if direct {
            m = w;
            if xf < 0.0 {
                m = -m;
            }
            n = m * omega;
        } else {
            n = w;
            if yf < 0.0 {
                n = -n;
            }
            m = n * omega;
        }
    } else if face == 2 {
        m = rho;
        if direct {
            l = w;
            if xf > 0.0 {
                l = -l;
            }
            n = -l * omega;
        } else {
            n = w;
            if yf < 0.0 {
                n = -n;
            }
            l = -n * omega;
        }
    } else if face == 3 {
        l = -rho;
        if direct {
            m = w;
            if xf > 0.0 {
                m = -m;
            }
            n = -m * omega;
        } else {
            n = w;
            if yf < 0.0 {
                n = -n;
            }
            m = -n * omega;
        }
    } else if face == 4 {
        m = -rho;
        if direct {
            l = w;
            if xf < 0.0 {
                l = -l;
            }
            n = l * omega;
        } else {
            n = w;
            if yf < 0.0 {
                n = -n;
            }
            l = n * omega;
        }
    } else if face == 5 {
        n = -rho;
        if direct {
            m = w;
            if xf < 0.0 {
                m = -m;
            }
            l = m * omega;
        } else {
            l = w;
            if yf < 0.0 {
                l = -l;
            }
            m = l * omega;
        }
    }

    let phi = if l == 0.0 && m == 0.0 {
        0.0
    } else {
        T::atan2d(m, l)
    };
    let theta = T::asind(n);

    (Status::Ok, phi, theta)
}
