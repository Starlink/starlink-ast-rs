// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
// Derived from WCSLIB (AST's fork of wcslib 2.9, with fixes from wcslib 8.10): Copyright 1995-2002 Mark Calabretta, GNU Library General Public License version 2 or later; this port is distributed under LGPL-3.0-or-later.

//! The native projection state, [`Prj`], and its dispatch to the projection
//! functions of the family modules.
//!
//! # How wcslib's lazily set-up struct maps onto an owned value
//!
//! wcslib keeps a projection's parameters, its derived constants and a
//! `flag` that says whether the constants are current in one struct, and
//! every `fwd` and `rev` function calls the projection's set-up function
//! first whenever `flag` does not name the projection.
//! AST clears `flag` whenever a parameter changes and never sets it to a
//! negative value, so after set-up `flag` is always positive and every test
//! of the form `prj->flag > 0` in the projection bodies passes.
//! Here [`Prj::new`] runs the set-up once, eagerly, and the bodies apply
//! those tests unconditionally.
//! A set-up that fails leaves its status in [`Prj`], and every call returns
//! it, so the error surfaces at the first point that reaches the projection
//! as it does in C.
//!
//! wcslib's set-up functions write `flag` before they can fail, so after a
//! failed set-up C runs the projection bodies on half-initialized constants
//! at the second and later calls.
//! The native [`Prj`] keeps returning the set-up status instead.
//!
//! The function pointers, the projection `code` and `flag` of C's struct are
//! not stored: the projection is the [`WcsProjection`] and the dispatch is a
//! `match` on it.
//!
//! # Per-value and slice entries
//!
//! The per-value entries, [`Prj::fwd_raw`] and [`Prj::rev_raw`], run the
//! projection bodies with [`Libm`] and return wcslib's bits.
//! The slice entries, [`Prj::fwd_slice`] and [`Prj::rev_slice`], do the same
//! for most projections. For a projection that opts in, they run its body,
//! or the stage of it that holds the trigonometry, with the vectorizable
//! [`Kernel`] trigonometry over every element, finish any later stage one
//! element at a time, then redo with [`Libm`] each element whose
//! trigonometric arguments the kernels do not cover, so each output is
//! within the kernels' drift of the per-value entry's and a point is bad in
//! one exactly when it is bad in the other.
//!
//! A projection opts in one direction at a time, with:
//!
//! - a predicate, `<code>_fwd_needs_special` or `<code>_rev_needs_special`,
//!   written with `|` rather than `||` so that it has no branch, and true
//!   for every input where either
//!   - an argument of a [`Kernel`] function the body would call is outside
//!     that function's cover, or
//!   - a status decision, or the choice of a branch of the body, depends on
//!     a value derived from a kernel that could land on the other side of
//!     its threshold within the kernels' drift; the body can instead make
//!     that decision from a value computed exactly as [`Libm`] computes it.
//!
//!   A difference of an ULP or two in a kernel must never turn a bad point
//!   good or a good one bad, or change which branch of the body runs: only
//!   the outputs may move, and only by the kernels' drift;
//! - a body with no branch once [`Kernel`] is inlined into it, so that the
//!   loop over the elements vectorizes (a value the body uses on one side of
//!   a test only is computed before the test, or, where computing it costs
//!   the per-value entry a library call, computed when `T::BRANCH_FREE` is
//!   set or the test needs it);
//! - for a body with a stage that cannot vectorize, such as an iteration
//!   whose number of steps depends on the point, a split of the body into
//!   two `#[inline(always)]` functions that both the body and the slice
//!   call, so no arithmetic is written twice: the kernels run the first, and
//!   a `finish` function runs the second per element and declines, so that
//!   the element is redone with [`Libm`], wherever the kernels' drift could
//!   change its status; a `//` comment on it argues why every element it
//!   accepts has the per-value entry's status;
//! - a `kernel_body!` line declaring the pair, and an arm in that slice
//!   entry's `match` that passes it to `kernel_slice`;
//! - where the parameters choose between forms of the body for every point,
//!   a body and predicate per form, each with the choice made, and an arm
//!   per form guarded on the parameters, so that no loop tests them.
//!
//! A direction opts in only when that is faster than its [`Libm`] loop at a
//! large number of points, on the smooth inputs of a real image as well as
//! on random ones, and no slower at one point than the C library.
//!
//! TAN's forward projection opts in: on Apple silicon, on a 1000 by 1000
//! grid of a one-degree image it ran at 22 ns per point against 60 for its
//! [`Libm`] loop, and at 27 against 67 on random points.
//! TAN's reverse projection does not: its two arctangents, each about three
//! divisions in the kernel, ran at 20 ns per point against 18 on that grid,
//! where libm's arctangent follows the same branches from point to point,
//! and against 51 only on random points; at one point it took 40 ns more.
//!
//! SIN's forward projection opts in, its orthographic and synthesis forms
//! each with its own body. The synthesis form's status compares the
//! latitude with a limit, the arc tangent of the kernels' sine and cosine of
//! the longitude, and a point within a margin of it, twice the bound on how
//! far the kernels' limit can be from [`Libm`]'s, is redone with [`Libm`].
//! On Apple silicon, on a 1000 by 1000 grid of a one-degree image the
//! orthographic form ran at 16 ns per point against 39 for its [`Libm`]
//! loop, and at 16 against 48 on random points of the hemisphere; the
//! synthesis form, with NCP's parameters for a field at declination 30
//! degrees, at 34 against 59 on the grid and 35 against 79 on random
//! points. At one point the forms took 67 and 96 ns against 41 and 60, the
//! set-up of the slice's loops outweighing the faster trigonometry.
//! SIN's reverse projection does not opt in. Its orthographic form's
//! [`Libm`] loop ran at 12 ns per point on the grid, where libm's arc
//! tangent and arc cosine follow the same branches from point to point;
//! with the kernels' arc tangent for the longitude it ran at 16.5 (32
//! against 46 on random points of a hundred-degree field), and with the
//! kernels' arc sine and arc cosine as well at 27 (28 against 46).
//! Those arc sine and arc cosine, the kernel arc tangent of `v` and
//! `sqrt((1 - v)(1 + v))`, were up to 2 ULP from libm's in radians over two
//! million values in [-1.1, 1.1], for about 18% of them, and again 2 ULP
//! with 41,600 values added at distances from 2^-52 to nearly 1 from -1, 0
//! and 1: the arc sine 2 ULP even within 0.5 of 0, the arc cosine 1 ULP
//! within 0.9 of 0 and 2 nearer -1 and 1. The cause is the rounding of
//! `(1 - v)(1 + v)`, which takes them beyond the kernels' rule of 1 ULP of
//! the exact value, so [`Kernel`]'s `asind` and `acosd` are [`Libm`]'s. A
//! correctly rounded `1 - v^2`, one fused multiply-add, brought both within
//! 1 ULP, but a build without fused multiply-adds would need the same bits.
//!
//! ZPN's forward projection opts in its only trigonometry, the sine and
//! cosine of the longitude; the polynomial in the colatitude, and the
//! status, which depends on the latitude alone, are computed per element on
//! the result. On Apple silicon, with the cubic of the benchmarks, on a 1000
//! by 1000 grid of a one-degree image it ran at 11 ns per point against 33
//! for its [`Libm`] loop, and at 11 against 36 on random points of the
//! hemisphere; at one point it took 60 ns against 33.
//! ZPN's reverse projection does not opt in. Its only trigonometry is the
//! arc tangent that gives the longitude, after a root search that decides
//! the status from the radius alone, so the kernel could not change a
//! status; but on the grid it ran at 52.4 ns per point against 52.0 for its
//! [`Libm`] loop, and at 81 against 80 on a twenty-degree image, faster only
//! on random points (63 against 83), and at one point it took 90 ns against
//! 48.
//!
//! TPN's forward projection opts in its front end, the gnomonic projection
//! to (xi, eta); the Newton iteration that inverts the polynomial runs per
//! element on the result, with one step fewer than the per-value entry, and
//! an element that has not converged by then, or whose result has a
//! coordinate so near zero that rounding decides the iteration's
//! convergence test, is redone with [`Libm`]. On
//! Apple silicon, with the parameters of a DSS plate, on a 1000 by 1000 grid
//! of the plate it ran at 168 ns per point against 208 for its [`Libm`]
//! loop, and at 164 against 213 on random points on the plate; the
//! iteration takes most of that time. At one point it took 249 ns against
//! 205, the set-up of the slice's loops and the bound on the polynomials'
//! rounding outweighing the faster trigonometry, which is lost within the
//! 1.7 microseconds of a one-point transform through the plate's FrameSet.
//! A point whose iteration does not converge runs it twice, since only
//! [`Libm`]'s iteration can say it is bad: on the TNX FrameSet of the
//! benchmarks, whose inverse has one such point in 24, the inverse at a
//! million points ran at 995 ns per point against 915, while on its good
//! points alone it ran at 724 against 776.
//! TPN's reverse projection does not: it is the polynomial then TAN's two
//! arctangents, and at one point it ran at 135 ns against 54 for its
//! [`Libm`] loop on the plate grid and at 141 against 89 on random points,
//! from the kernel arctangents and the set-up of the vectorized loop for the
//! polynomial's coefficients. Its loop vectorized only once the body
//! computed the polynomial and both arctangents for every parameter set,
//! and then ran at 39 ns per point against 54 on the grid and at 38 against
//! 88 on random points.
//!
//! The other zenithal projections were timed on Apple silicon on 1000 by
//! 1000 grids of a one-degree and a twenty-degree image and on a million
//! random points of the hemisphere or their projections, against their
//! [`Libm`] loops; the C library takes about 620 ns for one point.
//!
//! ARC's, ZEA's and STG's forward projections opt in, each a whole body.
//! On the one-degree grid they ran at 9.1, 14.5 and 14.6 ns per point
//! against 28.6, 34.7 and 48.6 for their [`Libm`] loops, and on random
//! points at 8.9, 14.1 and 14.6 against 31.1, 37.3 and 56.5; at one point
//! they took 56, 63 and 63 ns against 26, 33 and 50. STG's status, whether
//! one plus the sine of the latitude is zero, is decided by the kernels'
//! sine; a point where that sine is at most -1 + 2^-52, two ULP above -1,
//! is redone with [`Libm`].
//!
//! AZP's and SZP's forward projections opt in their projection, whose
//! status where the denominator is zero is decided by the kernels' values:
//! a point whose denominator is within twice the bound on its drift of zero
//! is redone with [`Libm`]. Their divergence and overlap tests, which most
//! parameters need for no point, run per element with [`Libm`]'s sine and
//! cosine of the longitude. With the default parameters AZP ran at 27.2 ns
//! per point against 41.1 on the grid and at 34.5 against 48.4 on random
//! points, its divergence test running for every point; with a perspective
//! from twice the sphere's radius, tilted by 30 degrees, which needs no
//! divergence test, at 17.2 against 38.5 and at 17.1 against 45.6. SZP ran
//! at 16.8 against 51.9 and 59.1 with the default parameters, and at 69.9
//! against 74.8 and 85.5 against 89.7 with its overlap test running for
//! every point. At one point they took 73 and 68 ns against 42 and 53.
//!
//! AIR's forward projection opts in the sine and cosine of the longitude;
//! the radius, from the cosine, the logarithm and the square root of the
//! colatitude, and the status, which depends on the latitude alone, are
//! computed per element with [`Libm`]. It ran at 32.8 ns per point against
//! 41.4 on the grid (29.2 against 41.1 with its parameter at 45 degrees)
//! and at 29.0 against 48.3 on random points, and at one point took 74 ns
//! against 41.
//!
//! SZP's reverse projection opts in. All of it but its trigonometry is
//! branch-free and computed alike on both paths, and decides every status;
//! the kernels' arc tangent gives the longitude, and [`Libm`]'s arc sine,
//! per element, the latitude, so the latitude is the per-value entry's. It ran
//! at 29.6 ns per point against 31.4 for its [`Libm`] loop on the grid, at
//! 28.7 against 30.5 on the twenty-degree grid and at 32.7 against 66.3 on
//! random points, the [`Libm`] loop's quadratic and its branches costing as
//! much as the trigonometry; at one point it took 87 ns against 32.
//!
//! The other zenithal reverse projections do not opt in: their only kernel
//! function is the arc tangent that gives the longitude (and, for AZP, one
//! that gives a term of the latitude), and on the grid libm's arc tangent
//! follows the same branches from point to point and costs less than the
//! kernel's. On the one-degree grid, with their arc tangents run by the
//! kernels, ARC ran at 12.1 ns per point against 8.0, ZEA at 15.3 against
//! 11.4, STG at 19.5 against 11.1 and AZP at 27.6 against 18.1, though
//! faster on random points (20.5 against 26.3, 30.5 against 38.6, 34.8
//! against 39.1 and 48.8 against 55.8). AIR's reverse projection is a root
//! search of a hundred steps at most, each with a logarithm, before its arc
//! tangent, and ran at 270 ns per point against 273 on the grid but 270
//! against 265 with its parameter at 45 degrees, and at one point took 299
//! ns against 272.

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};

use super::conic::{
    cod_fwd, cod_rev, cod_set, coe_fwd, coe_rev, coe_set, coo_fwd, coo_rev, coo_set, cop_fwd,
    cop_rev, cop_set,
};
use super::cylindrical::{
    car_fwd, car_rev, car_set, cea_fwd, cea_rev, cea_set, cyp_fwd, cyp_rev, cyp_set, mer_fwd,
    mer_rev, mer_set,
};
use super::healpix::{hpx_fwd, hpx_rev, hpx_set, xph_fwd, xph_rev, xph_set};
use super::polyconic::{bon_fwd, bon_rev, bon_set, pco_fwd, pco_rev, pco_set};
use super::pseudocyl::{
    ait_fwd, ait_rev, ait_set, mol_fwd, mol_rev, mol_set, par_fwd, par_rev, par_set, sfl_fwd,
    sfl_rev, sfl_set,
};
use super::quadcube::{
    csc_fwd, csc_rev, csc_set, qsc_fwd, qsc_rev, qsc_set, tsc_fwd, tsc_rev, tsc_set,
};
use super::tpn::{tpn_fwd, tpn_fwd_finish, tpn_fwd_front, tpn_fwd_needs_special, tpn_rev, tpn_set};
use super::trig::{Kernel, Libm, Trig};
use super::zenithal::{
    air_fwd, air_fwd_finish, air_fwd_front, air_fwd_needs_special, air_rev, air_set, arc_fwd,
    arc_fwd_needs_special, arc_rev, arc_set, azp_fwd, azp_fwd_finish, azp_fwd_front,
    azp_fwd_needs_special, azp_rev, azp_set, sin_fwd, sin_fwd_orthographic,
    sin_fwd_orthographic_needs_special, sin_fwd_synthesis, sin_fwd_synthesis_needs_special,
    sin_rev, sin_set, stg_fwd, stg_fwd_needs_special, stg_rev, stg_set, szp_fwd, szp_fwd_finish,
    szp_fwd_front, szp_fwd_needs_special, szp_rev, szp_rev_finish, szp_rev_front,
    szp_rev_needs_special, szp_set, tan_fwd, tan_fwd_needs_special, tan_rev, tan_set, zea_fwd,
    zea_fwd_needs_special, zea_rev, zea_set, zpn_fwd, zpn_fwd_finish, zpn_fwd_front,
    zpn_fwd_needs_special, zpn_rev, zpn_set,
};
use super::{PRJ_PARAM_SLOTS, WcsProjection};

/// Evaluate `$body` with `$f` bound to the forward projection function of
/// the projection `$proj`, so that `$body` is compiled once per projection
/// and calls it statically.
macro_rules! with_fwd {
    ($proj:expr, |$f:ident| $body:expr) => {
        match $proj {
            WcsProjection::Azp => {
                let $f = azp_fwd::<Libm>;
                $body
            }
            WcsProjection::Szp => {
                let $f = szp_fwd::<Libm>;
                $body
            }
            WcsProjection::Tan => {
                let $f = tan_fwd::<Libm>;
                $body
            }
            WcsProjection::Stg => {
                let $f = stg_fwd::<Libm>;
                $body
            }
            WcsProjection::Sin => {
                let $f = sin_fwd::<Libm>;
                $body
            }
            WcsProjection::Arc => {
                let $f = arc_fwd::<Libm>;
                $body
            }
            WcsProjection::Zpn => {
                let $f = zpn_fwd::<Libm>;
                $body
            }
            WcsProjection::Zea => {
                let $f = zea_fwd::<Libm>;
                $body
            }
            WcsProjection::Air => {
                let $f = air_fwd::<Libm>;
                $body
            }
            WcsProjection::Tpn => {
                let $f = tpn_fwd::<Libm>;
                $body
            }
            WcsProjection::Cyp => {
                let $f = cyp_fwd::<Libm>;
                $body
            }
            WcsProjection::Cea => {
                let $f = cea_fwd::<Libm>;
                $body
            }
            WcsProjection::Car => {
                let $f = car_fwd::<Libm>;
                $body
            }
            WcsProjection::Mer => {
                let $f = mer_fwd::<Libm>;
                $body
            }
            WcsProjection::Sfl => {
                let $f = sfl_fwd::<Libm>;
                $body
            }
            WcsProjection::Par => {
                let $f = par_fwd::<Libm>;
                $body
            }
            WcsProjection::Mol => {
                let $f = mol_fwd::<Libm>;
                $body
            }
            WcsProjection::Ait => {
                let $f = ait_fwd::<Libm>;
                $body
            }
            WcsProjection::Cop => {
                let $f = cop_fwd::<Libm>;
                $body
            }
            WcsProjection::Coe => {
                let $f = coe_fwd::<Libm>;
                $body
            }
            WcsProjection::Cod => {
                let $f = cod_fwd::<Libm>;
                $body
            }
            WcsProjection::Coo => {
                let $f = coo_fwd::<Libm>;
                $body
            }
            WcsProjection::Bon => {
                let $f = bon_fwd::<Libm>;
                $body
            }
            WcsProjection::Pco => {
                let $f = pco_fwd::<Libm>;
                $body
            }
            WcsProjection::Tsc => {
                let $f = tsc_fwd::<Libm>;
                $body
            }
            WcsProjection::Csc => {
                let $f = csc_fwd::<Libm>;
                $body
            }
            WcsProjection::Qsc => {
                let $f = qsc_fwd::<Libm>;
                $body
            }
            WcsProjection::Hpx => {
                let $f = hpx_fwd::<Libm>;
                $body
            }
            WcsProjection::Xph => {
                let $f = xph_fwd::<Libm>;
                $body
            }
            WcsProjection::Ncp | WcsProjection::Gls | WcsProjection::Unknown => {
                unreachable!("{} is rejected by Prj::new", $proj.code())
            }
        }
    };
}

/// [`with_fwd`] for the reverse projection functions.
macro_rules! with_rev {
    ($proj:expr, |$f:ident| $body:expr) => {
        match $proj {
            WcsProjection::Azp => {
                let $f = azp_rev::<Libm>;
                $body
            }
            WcsProjection::Szp => {
                let $f = szp_rev::<Libm>;
                $body
            }
            WcsProjection::Tan => {
                let $f = tan_rev::<Libm>;
                $body
            }
            WcsProjection::Stg => {
                let $f = stg_rev::<Libm>;
                $body
            }
            WcsProjection::Sin => {
                let $f = sin_rev::<Libm>;
                $body
            }
            WcsProjection::Arc => {
                let $f = arc_rev::<Libm>;
                $body
            }
            WcsProjection::Zpn => {
                let $f = zpn_rev::<Libm>;
                $body
            }
            WcsProjection::Zea => {
                let $f = zea_rev::<Libm>;
                $body
            }
            WcsProjection::Air => {
                let $f = air_rev::<Libm>;
                $body
            }
            WcsProjection::Tpn => {
                let $f = tpn_rev::<Libm>;
                $body
            }
            WcsProjection::Cyp => {
                let $f = cyp_rev::<Libm>;
                $body
            }
            WcsProjection::Cea => {
                let $f = cea_rev::<Libm>;
                $body
            }
            WcsProjection::Car => {
                let $f = car_rev::<Libm>;
                $body
            }
            WcsProjection::Mer => {
                let $f = mer_rev::<Libm>;
                $body
            }
            WcsProjection::Sfl => {
                let $f = sfl_rev::<Libm>;
                $body
            }
            WcsProjection::Par => {
                let $f = par_rev::<Libm>;
                $body
            }
            WcsProjection::Mol => {
                let $f = mol_rev::<Libm>;
                $body
            }
            WcsProjection::Ait => {
                let $f = ait_rev::<Libm>;
                $body
            }
            WcsProjection::Cop => {
                let $f = cop_rev::<Libm>;
                $body
            }
            WcsProjection::Coe => {
                let $f = coe_rev::<Libm>;
                $body
            }
            WcsProjection::Cod => {
                let $f = cod_rev::<Libm>;
                $body
            }
            WcsProjection::Coo => {
                let $f = coo_rev::<Libm>;
                $body
            }
            WcsProjection::Bon => {
                let $f = bon_rev::<Libm>;
                $body
            }
            WcsProjection::Pco => {
                let $f = pco_rev::<Libm>;
                $body
            }
            WcsProjection::Tsc => {
                let $f = tsc_rev::<Libm>;
                $body
            }
            WcsProjection::Csc => {
                let $f = csc_rev::<Libm>;
                $body
            }
            WcsProjection::Qsc => {
                let $f = qsc_rev::<Libm>;
                $body
            }
            WcsProjection::Hpx => {
                let $f = hpx_rev::<Libm>;
                $body
            }
            WcsProjection::Xph => {
                let $f = xph_rev::<Libm>;
                $body
            }
            WcsProjection::Ncp | WcsProjection::Gls | WcsProjection::Unknown => {
                unreachable!("{} is rejected by Prj::new", $proj.code())
            }
        }
    };
}

/// The status wcslib's projection functions return.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Status {
    /// The point was projected.
    Ok = 0,
    /// The projection parameters are unusable.
    BadParams = 1,
    /// The point cannot be projected.
    BadPoint = 2,
}

/// The parameters of a projection and the constants derived from them:
/// wcslib's `AstPrjPrm` without its bookkeeping (see the module
/// documentation).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PrjPrm {
    /// Native longitude of the fiducial point, in degrees.
    pub(crate) phi0: f64,
    /// Native latitude of the fiducial point, in degrees.
    pub(crate) theta0: f64,
    /// The radius of the generating sphere; the set-up resets 0 to 180/pi.
    pub(crate) r0: f64,
    /// The latitude-axis projection parameters.
    pub(crate) p: [f64; PRJ_PARAM_SLOTS],
    /// The longitude-axis projection parameters (TPN only).
    pub(crate) p2: [f64; PRJ_PARAM_SLOTS],
    /// Constants the set-up derives from the parameters.
    pub(crate) w: [f64; 20],
    /// The projection's integer constant: ZPN's polynomial degree, HPX's
    /// `n`, TPN's "include the TAN projection" flag.
    pub(crate) n: i32,
}

/// A projection with its parameters, set up.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Prj {
    proj: WcsProjection,
    prm: PrjPrm,
    /// The status of the set-up, returned by every call when it failed.
    setup: Status,
}

impl Prj {
    /// A projection of the given type with the parameters `params` and
    /// defaults for those not supplied, set up.
    ///
    /// `params[0]` holds the latitude-axis parameters and `params[1]` the
    /// longitude-axis parameters; a `None` entry is a parameter that was not
    /// supplied.
    /// `tpn_tan` is consulted only for TPN.
    ///
    /// A failed set-up is not an error here: the status is returned by the
    /// first call that reaches the projection.
    ///
    /// # Errors
    ///
    /// `Err` for NCP, GLS and the unknown projection, which a `WcsMap`
    /// converts to another projection before it gets here.
    pub(crate) fn new(
        proj: WcsProjection,
        params: &[Vec<Option<f64>>],
        tpn_tan: bool,
    ) -> AstResult<Prj> {
        if matches!(
            proj,
            WcsProjection::Ncp | WcsProjection::Gls | WcsProjection::Unknown
        ) {
            return Err(AstError::new(
                ErrorCode::Wcsty,
                format!(
                    "Projection {} cannot be initialised directly and must be handled at the WcsMap level",
                    proj.code()
                ),
            ));
        }
        let mut prm = PrjPrm {
            phi0: 0.0,
            theta0: 0.0,
            r0: 0.0,
            p: [0.0; PRJ_PARAM_SLOTS],
            p2: [0.0; PRJ_PARAM_SLOTS],
            w: [0.0; 20],
            n: 0,
        };
        init_params(proj, &mut prm, params, tpn_tan);
        let setup = run_setup(proj, &mut prm);
        Ok(Prj { proj, prm, setup })
    }

    /// Read a projection parameter: `axis` is 0 for the latitude axis and 1
    /// for the longitude axis, `m` the parameter index.
    /// `None` if either is out of range.
    pub(crate) fn get_param(&self, axis: usize, m: usize) -> Option<f64> {
        if m >= PRJ_PARAM_SLOTS {
            return None;
        }
        match axis {
            0 => Some(self.prm.p[m]),
            1 => Some(self.prm.p2[m]),
            _ => None,
        }
    }

    /// Forward projection of native (`phi`, `theta`) in degrees: the status
    /// and the projected (x, y).
    /// The outputs are whatever the projection wrote before returning, which
    /// callers such as the longitude-range probe read even on
    /// [`Status::BadPoint`]; a failed set-up gives zeros.
    #[inline]
    pub(crate) fn fwd_raw(&self, phi: f64, theta: f64) -> (Status, f64, f64) {
        if self.setup != Status::Ok {
            return (self.setup, 0.0, 0.0);
        }
        let prm = &self.prm;
        with_fwd!(self.proj, |f| f(prm, phi, theta))
    }

    /// Reverse projection of (`x`, `y`): the status and the native
    /// (phi, theta) in degrees.
    /// The outputs are whatever the projection wrote before returning, which
    /// callers such as the longitude-range probe read even on
    /// [`Status::BadPoint`]; a failed set-up gives zeros.
    #[inline]
    pub(crate) fn rev_raw(&self, x: f64, y: f64) -> (Status, f64, f64) {
        if self.setup != Status::Ok {
            return (self.setup, 0.0, 0.0);
        }
        let prm = &self.prm;
        with_rev!(self.proj, |f| f(prm, x, y))
    }

    /// [`fwd_raw`](Prj::fwd_raw) in place for every pair (`a[i]`, `b[i]`),
    /// in one pass over the slices: `prep` gives the native (phi, theta) in
    /// degrees to project from the pair on entry, and `finish` the pair to
    /// store from the projected (x, y).
    ///
    /// A pair that `prep` gives as [`BAD_VALUE`] or NaN, and a point the
    /// projection cannot project, give `finish(BAD_VALUE, BAD_VALUE)`.
    /// Each pair projected counts one call of `wcs_fwd`.
    ///
    /// # Errors
    ///
    /// `ErrorCode::Wcspa` if the projection parameters are unusable for a
    /// pair that is not bad; the slices then hold partial results, whose
    /// contents are unspecified.
    pub(crate) fn fwd_slice(
        &self,
        a: &mut [f64],
        b: &mut [f64],
        prep: impl Fn(f64, f64) -> (f64, f64),
        finish: impl Fn(f64, f64) -> (f64, f64),
    ) -> AstResult<()> {
        if self.setup != Status::Ok {
            // A set-up fails only for unusable parameters; any other status
            // here would turn every good point bad without an error.
            debug_assert_eq!(self.setup, Status::BadParams);
            let setup = self.setup;
            return slice_apply(a, b, "wcs_fwd", prep, |_, _| (setup, 0.0, 0.0), finish);
        }
        let prm = &self.prm;
        match self.proj {
            WcsProjection::Azp => kernel_slice::<AzpFwd>(prm, a, b, "wcs_fwd", prep, finish),
            WcsProjection::Szp => kernel_slice::<SzpFwd>(prm, a, b, "wcs_fwd", prep, finish),
            WcsProjection::Tan => kernel_slice::<TanFwd>(prm, a, b, "wcs_fwd", prep, finish),
            WcsProjection::Stg => kernel_slice::<StgFwd>(prm, a, b, "wcs_fwd", prep, finish),
            WcsProjection::Sin if prm.w[1] == 0.0 => {
                kernel_slice::<SinFwdOrthographic>(prm, a, b, "wcs_fwd", prep, finish)
            }
            WcsProjection::Sin => {
                kernel_slice::<SinFwdSynthesis>(prm, a, b, "wcs_fwd", prep, finish)
            }
            WcsProjection::Arc => kernel_slice::<ArcFwd>(prm, a, b, "wcs_fwd", prep, finish),
            WcsProjection::Zpn => kernel_slice::<ZpnFwd>(prm, a, b, "wcs_fwd", prep, finish),
            WcsProjection::Zea => kernel_slice::<ZeaFwd>(prm, a, b, "wcs_fwd", prep, finish),
            WcsProjection::Air => kernel_slice::<AirFwd>(prm, a, b, "wcs_fwd", prep, finish),
            WcsProjection::Tpn => kernel_slice::<TpnFwd>(prm, a, b, "wcs_fwd", prep, finish),
            proj => with_fwd!(proj, |f| slice_apply(
                a,
                b,
                "wcs_fwd",
                prep,
                |phi, theta| f(prm, phi, theta),
                finish
            )),
        }
    }

    /// [`rev_raw`](Prj::rev_raw) in place for every pair (`a[i]`, `b[i]`),
    /// in one pass over the slices: `prep` gives the (x, y) to project from
    /// the pair on entry, and `finish` the pair to store from the native
    /// (phi, theta) in degrees; bad pairs, errors and counting (of
    /// `wcs_rev`) as for [`fwd_slice`](Prj::fwd_slice).
    ///
    /// # Errors
    ///
    /// `ErrorCode::Wcspa` if the projection parameters are unusable for a
    /// pair that is not bad; the slices then hold partial results, whose
    /// contents are unspecified.
    pub(crate) fn rev_slice(
        &self,
        a: &mut [f64],
        b: &mut [f64],
        prep: impl Fn(f64, f64) -> (f64, f64),
        finish: impl Fn(f64, f64) -> (f64, f64),
    ) -> AstResult<()> {
        if self.setup != Status::Ok {
            // A set-up fails only for unusable parameters; any other status
            // here would turn every good point bad without an error.
            debug_assert_eq!(self.setup, Status::BadParams);
            let setup = self.setup;
            return slice_apply(a, b, "wcs_rev", prep, |_, _| (setup, 0.0, 0.0), finish);
        }
        let prm = &self.prm;
        match self.proj {
            WcsProjection::Szp => kernel_slice::<SzpRev>(prm, a, b, "wcs_rev", prep, finish),
            proj => with_rev!(proj, |f| slice_apply(
                a,
                b,
                "wcs_rev",
                prep,
                |x, y| f(prm, x, y),
                finish
            )),
        }
    }
}

/// The error for unusable projection parameters.
fn bad_params() -> AstError {
    AstError::new(
        ErrorCode::Wcspa,
        "WcsMap: the projection parameter values are unusable",
    )
}

/// Apply the projection call `project` in place to every pair (`a[i]`,
/// `b[i]`), in one pass: `prep` gives the pair to project, a pair holding
/// [`BAD_VALUE`] or NaN and a point the call cannot project give
/// ([`BAD_VALUE`], [`BAD_VALUE`]), and `finish` gives the pair stored from
/// the result. One call of `counter` is counted per pair projected.
#[inline(always)]
fn slice_apply(
    a: &mut [f64],
    b: &mut [f64],
    counter: &'static str,
    prep: impl Fn(f64, f64) -> (f64, f64),
    project: impl Fn(f64, f64) -> (Status, f64, f64),
    finish: impl Fn(f64, f64) -> (f64, f64),
) -> AstResult<()> {
    let n = a.len();
    assert_eq!(b.len(), n, "the two slices must have the same length");
    let b = &mut b[..n];
    let mut calls = 0u64;
    let mut result = Ok(());
    for i in 0..n {
        let (u, v) = prep(a[i], b[i]);
        let (p, q) = if u == BAD_VALUE || v == BAD_VALUE || u.is_nan() || v.is_nan() {
            (BAD_VALUE, BAD_VALUE)
        } else {
            calls += 1;
            match project(u, v) {
                (Status::Ok, p, q) => (p, q),
                (Status::BadPoint, ..) => (BAD_VALUE, BAD_VALUE),
                (Status::BadParams, ..) => {
                    result = Err(bad_params());
                    break;
                }
            }
        };
        (a[i], b[i]) = finish(p, q);
    }
    crate::call_counts::hit_n(counter, calls);
    result
}

/// A projection body in one direction that a slice entry runs through the
/// vectorizable kernels.
///
/// `eval` is the stage of the body the kernels run, generic over the
/// trigonometry: for most bodies the whole body. `needs_special` is true
/// where an argument of a trigonometric function the stage would call lies
/// outside that function's kernel cover, and where a status decision or a
/// branch of the stage depends on a kernel-derived value that the kernels'
/// drift could carry across its threshold; the element is then redone with
/// [`Libm`], so the kernels never change whether a point is bad or which
/// branch runs. Both run in the vectorized loop, so neither may branch; the
/// module documentation says how a projection meets that.
///
/// `finish` completes a point from its inputs and its stage's result, one
/// point at a time, and gives `None` where it cannot rule out a different
/// status from the one [`Libm`] would give; `libm` is the whole body as the
/// per-value entry runs it, which redoes those points and the special ones.
trait KernelBody {
    /// The stage of the body the kernels run.
    fn eval<T: Trig>(prm: &PrjPrm, u: f64, v: f64) -> (Status, f64, f64);
    /// Whether the stage at (`u`, `v`) needs [`Libm`], where the stage runs
    /// with the trigonometry `T`.
    fn needs_special<T: Trig>(prm: &PrjPrm, u: f64, v: f64) -> bool;
    /// The rest of the body at (`u`, `v`) on the stage's result, or `None`
    /// to redo the point with [`Libm`].
    fn finish(
        prm: &PrjPrm,
        u: f64,
        v: f64,
        stage: (Status, f64, f64),
    ) -> Option<(Status, f64, f64)>;
    /// The whole body with [`Libm`].
    fn libm(prm: &PrjPrm, u: f64, v: f64) -> (Status, f64, f64);
}

/// Declare the [`KernelBody`] `$name` for the body `$body` and its predicate
/// `$special`, the kernels running the whole body; or, with `$finish` and
/// `$whole`, for the stage `$body` of the body `$whole`, completed per point
/// by `$finish`.
macro_rules! kernel_body {
    ($name:ident, $body:ident, $special:ident) => {
        kernel_body!($name, $body, $special, kernel_stage_is_whole_body, $body);
    };
    ($name:ident, $body:ident, $special:ident, $finish:ident, $whole:ident) => {
        struct $name;
        impl KernelBody for $name {
            #[inline(always)]
            fn eval<T: Trig>(prm: &PrjPrm, u: f64, v: f64) -> (Status, f64, f64) {
                $body::<T>(prm, u, v)
            }
            #[inline(always)]
            fn needs_special<T: Trig>(prm: &PrjPrm, u: f64, v: f64) -> bool {
                $special::<T>(prm, u, v)
            }
            #[inline(always)]
            fn finish(
                prm: &PrjPrm,
                u: f64,
                v: f64,
                stage: (Status, f64, f64),
            ) -> Option<(Status, f64, f64)> {
                $finish(prm, u, v, stage)
            }
            #[inline(always)]
            fn libm(prm: &PrjPrm, u: f64, v: f64) -> (Status, f64, f64) {
                $whole::<Libm>(prm, u, v)
            }
        }
    };
}

/// The `finish` of a [`KernelBody`] whose kernels run the whole body.
#[inline(always)]
#[allow(clippy::unnecessary_wraps)]
fn kernel_stage_is_whole_body(
    _prm: &PrjPrm,
    _u: f64,
    _v: f64,
    stage: (Status, f64, f64),
) -> Option<(Status, f64, f64)> {
    Some(stage)
}

kernel_body!(TanFwd, tan_fwd, tan_fwd_needs_special);
kernel_body!(
    SinFwdOrthographic,
    sin_fwd_orthographic,
    sin_fwd_orthographic_needs_special
);
kernel_body!(
    SinFwdSynthesis,
    sin_fwd_synthesis,
    sin_fwd_synthesis_needs_special
);
kernel_body!(
    ZpnFwd,
    zpn_fwd_front,
    zpn_fwd_needs_special,
    zpn_fwd_finish,
    zpn_fwd
);
kernel_body!(
    AzpFwd,
    azp_fwd_front,
    azp_fwd_needs_special,
    azp_fwd_finish,
    azp_fwd
);
kernel_body!(
    SzpFwd,
    szp_fwd_front,
    szp_fwd_needs_special,
    szp_fwd_finish,
    szp_fwd
);
kernel_body!(
    SzpRev,
    szp_rev_front,
    szp_rev_needs_special,
    szp_rev_finish,
    szp_rev
);
kernel_body!(StgFwd, stg_fwd, stg_fwd_needs_special);
kernel_body!(ArcFwd, arc_fwd, arc_fwd_needs_special);
kernel_body!(ZeaFwd, zea_fwd, zea_fwd_needs_special);
kernel_body!(
    AirFwd,
    air_fwd_front,
    air_fwd_needs_special,
    air_fwd_finish,
    air_fwd
);
kernel_body!(
    TpnFwd,
    tpn_fwd_front,
    tpn_fwd_needs_special,
    tpn_fwd_finish,
    tpn_fwd
);

/// How many elements [`kernel_slice_body`] evaluates with the kernels
/// before fixing them up, so the kernels' outputs stay in the first-level
/// cache and on the stack.
const KERNEL_BLOCK: usize = 32;

/// [`slice_apply`] for the body `K` run through the vectorizable kernels:
/// the same `prep` and `finish`, bad pairs, errors and counting, with each
/// output within the kernels' drift of [`Libm`]'s. Each element gets the same
/// bits whatever the slice's length, and whichever build of the loop runs: on
/// x86-64 one for AVX2 and FMA when the CPU has them.
///
/// `prep` and `finish` run in passes of their own, before and after the
/// kernel loops, which costs less at small lengths than running them inside
/// the block loops.
fn kernel_slice<K: KernelBody>(
    prm: &PrjPrm,
    a: &mut [f64],
    b: &mut [f64],
    counter: &'static str,
    prep: impl Fn(f64, f64) -> (f64, f64),
    finish: impl Fn(f64, f64) -> (f64, f64),
) -> AstResult<()> {
    let n = a.len();
    assert_eq!(b.len(), n, "the two slices must have the same length");
    let b = &mut b[..n];
    for i in 0..n {
        (a[i], b[i]) = prep(a[i], b[i]);
    }
    kernel_loops::<K>(prm, a, b, counter)?;
    for i in 0..n {
        (a[i], b[i]) = finish(a[i], b[i]);
    }
    Ok(())
}

/// The kernel loops of [`kernel_slice`], in the build of
/// [`kernel_slice_body`] the CPU runs.
#[inline(always)]
fn kernel_loops<K: KernelBody>(
    prm: &PrjPrm,
    a: &mut [f64],
    b: &mut [f64],
    counter: &'static str,
) -> AstResult<()> {
    #[cfg(target_arch = "x86_64")]
    if crate::vmath::x86_has_avx2_fma() {
        // SAFETY: the CPU has AVX2 and FMA, which is all the function's
        // target features require.
        return unsafe { kernel_slice_avx2_fma::<K>(prm, a, b, counter) };
    }
    kernel_slice_body::<K, { crate::vmath::HAS_FMA }>(prm, a, b, counter)
}

/// [`kernel_slice_body`] built for AVX2 and FMA, so its kernel loop runs
/// four wide and the arctangent's residual is one fused multiply-add.
///
/// # Safety
///
/// The CPU must have AVX2 and FMA. (An `unsafe fn` because the crate's MSRV,
/// 1.85, predates safe functions with target features.)
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn kernel_slice_avx2_fma<K: KernelBody>(
    prm: &PrjPrm,
    a: &mut [f64],
    b: &mut [f64],
    counter: &'static str,
) -> AstResult<()> {
    kernel_slice_body::<K, true>(prm, a, b, counter)
}

/// The loops of [`kernel_slice`], block by block: the pairs of the block
/// that are not bad are gathered, writing [`BAD_VALUE`] for the others, so
/// the kernels run on good pairs only; `K` with [`Kernel`] runs over every
/// gathered pair, branch-free so the loop vectorizes; then each result is
/// completed by `K::finish`, redone with [`Libm`] where `K::needs_special`
/// says or `K::finish` declines, scattered back, and its status mapped as
/// [`slice_apply`] does.
#[inline(always)]
fn kernel_slice_body<K: KernelBody, const FMA: bool>(
    prm: &PrjPrm,
    a: &mut [f64],
    b: &mut [f64],
    counter: &'static str,
) -> AstResult<()> {
    let n = a.len();
    assert_eq!(b.len(), n, "the two slices must have the same length");
    let b = &mut b[..n];
    let mut calls = 0u64;
    let mut result = Ok(());
    let mut index = [0usize; KERNEL_BLOCK];
    let mut u = [0.0; KERNEL_BLOCK];
    let mut v = [0.0; KERNEL_BLOCK];
    let mut status = [Status::Ok; KERNEL_BLOCK];
    let mut special = [false; KERNEL_BLOCK];
    let mut p = [0.0; KERNEL_BLOCK];
    let mut q = [0.0; KERNEL_BLOCK];
    // Blocks by index: zipped `chunks_mut` iterators divide by the block
    // length at run time wherever their constructor is not inlined.
    'blocks: for start in (0..n).step_by(KERNEL_BLOCK) {
        let end = n.min(start + KERNEL_BLOCK);
        let (a, b) = (&mut a[start..end], &mut b[start..end]);
        let mut good = 0;
        for i in 0..a.len() {
            let (x, y) = (a[i], b[i]);
            if x == BAD_VALUE || y == BAD_VALUE || x.is_nan() || y.is_nan() {
                a[i] = BAD_VALUE;
                b[i] = BAD_VALUE;
            } else {
                index[good] = i;
                u[good] = x;
                v[good] = y;
                good += 1;
            }
        }
        let (index, u, v, status, special, p, q) = (
            &index[..good],
            &u[..good],
            &v[..good],
            &mut status[..good],
            &mut special[..good],
            &mut p[..good],
            &mut q[..good],
        );
        for j in 0..good {
            (status[j], p[j], q[j]) = K::eval::<Kernel<FMA>>(prm, u[j], v[j]);
            special[j] = K::needs_special::<Kernel<FMA>>(prm, u[j], v[j]);
        }
        for j in 0..good {
            calls += 1;
            let out = if special[j] {
                None
            } else {
                K::finish(prm, u[j], v[j], (status[j], p[j], q[j]))
            }
            .unwrap_or_else(|| K::libm(prm, u[j], v[j]));
            let i = index[j];
            match out {
                (Status::Ok, x, y) => {
                    a[i] = x;
                    b[i] = y;
                }
                (Status::BadPoint, ..) => {
                    a[i] = BAD_VALUE;
                    b[i] = BAD_VALUE;
                }
                (Status::BadParams, ..) => {
                    result = Err(bad_params());
                    break 'blocks;
                }
            }
        }
    }
    crate::call_counts::hit_n(counter, calls);
    result
}

/// Run the set-up function of a projection.
fn run_setup(proj: WcsProjection, prm: &mut PrjPrm) -> Status {
    match proj {
        WcsProjection::Azp => azp_set(prm),
        WcsProjection::Szp => szp_set(prm),
        WcsProjection::Tan => tan_set(prm),
        WcsProjection::Stg => stg_set(prm),
        WcsProjection::Sin => sin_set(prm),
        WcsProjection::Arc => arc_set(prm),
        WcsProjection::Zpn => zpn_set(prm),
        WcsProjection::Zea => zea_set(prm),
        WcsProjection::Air => air_set(prm),
        WcsProjection::Tpn => tpn_set(prm),
        WcsProjection::Cyp => cyp_set(prm),
        WcsProjection::Cea => cea_set(prm),
        WcsProjection::Car => car_set(prm),
        WcsProjection::Mer => mer_set(prm),
        WcsProjection::Sfl => sfl_set(prm),
        WcsProjection::Par => par_set(prm),
        WcsProjection::Mol => mol_set(prm),
        WcsProjection::Ait => ait_set(prm),
        WcsProjection::Cop => cop_set(prm),
        WcsProjection::Coe => coe_set(prm),
        WcsProjection::Cod => cod_set(prm),
        WcsProjection::Coo => coo_set(prm),
        WcsProjection::Bon => bon_set(prm),
        WcsProjection::Pco => pco_set(prm),
        WcsProjection::Tsc => tsc_set(prm),
        WcsProjection::Csc => csc_set(prm),
        WcsProjection::Qsc => qsc_set(prm),
        WcsProjection::Hpx => hpx_set(prm),
        WcsProjection::Xph => xph_set(prm),
        WcsProjection::Ncp | WcsProjection::Gls | WcsProjection::Unknown => {
            unreachable!("{} is rejected by Prj::new", proj.code())
        }
    }
}

/// Fill the parameter arrays from the supplied parameters, giving each
/// parameter that was not supplied its projection's default or zero.
///
/// `InitPrjPrm` in `reference/starlink-ast` at 3d3f255f
/// (`src/wcsmap.c`).
fn init_params(proj: WcsProjection, prm: &mut PrjPrm, params: &[Vec<Option<f64>>], tpn_tan: bool) {
    // Make the set-up compute its constants afresh.
    prm.r0 = 0.0;

    // TPN's `n` says whether the transformation includes the TAN projection
    // or only the polynomial correction; for the other projections the
    // set-up fills it.
    if matches!(proj, WcsProjection::Tpn) {
        prm.n = i32::from(tpn_tan);
    }

    // Start with every parameter unset, then copy those supplied.
    prm.p.fill(BAD_VALUE);
    prm.p2.fill(BAD_VALUE);
    if let Some(pv) = params.first() {
        for (m, v) in pv.iter().enumerate().take(PRJ_PARAM_SLOTS) {
            if let Some(v) = v {
                prm.p[m] = *v;
            }
        }
    }
    if let Some(pv) = params.get(1) {
        for (m, v) in pv.iter().enumerate().take(PRJ_PARAM_SLOTS) {
            if let Some(v) = v {
                prm.p2[m] = *v;
            }
        }
    }

    // Defaults for the parameters that do not default to zero, one arm per
    // projection as in C.
    let p = &mut prm.p;
    let p2 = &mut prm.p2;
    #[allow(clippy::collapsible_match)]
    match proj {
        WcsProjection::Szp => {
            if p[3] == BAD_VALUE {
                p[3] = 90.0;
            }
        }
        WcsProjection::Air => {
            if p[1] == BAD_VALUE {
                p[1] = 90.0;
            }
        }
        WcsProjection::Cyp => {
            if p[1] == BAD_VALUE {
                p[1] = 1.0;
            }
            if p[2] == BAD_VALUE {
                p[2] = 1.0;
            }
        }
        WcsProjection::Cea => {
            if p[1] == BAD_VALUE {
                p[1] = 1.0;
            }
        }
        WcsProjection::Tpn => {
            if p[1] == BAD_VALUE {
                p[1] = 1.0;
            }
            if p2[1] == BAD_VALUE {
                p2[1] = 1.0;
            }
        }
        WcsProjection::Hpx => {
            if p[1] == BAD_VALUE {
                p[1] = 4.0;
            }
            if p[2] == BAD_VALUE {
                p[2] = 3.0;
            }
        }
        _ => {}
    }

    // Zero for the rest, except parameter 1 of Bonne and the conics, which
    // has no default.
    let undefaultable = matches!(
        proj,
        WcsProjection::Bon
            | WcsProjection::Cop
            | WcsProjection::Coe
            | WcsProjection::Cod
            | WcsProjection::Coo
    );
    for (i, slot) in p.iter_mut().enumerate() {
        if !(undefaultable && i == 1) && *slot == BAD_VALUE {
            *slot = 0.0;
        }
    }
    for (i, slot) in p2.iter_mut().enumerate() {
        if !(undefaultable && i == 1) && *slot == BAD_VALUE {
            *slot = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_PARAMS: [Vec<Option<f64>>; 2] = [Vec::new(), Vec::new()];

    fn prj(proj: WcsProjection) -> Prj {
        Prj::new(proj, &NO_PARAMS, true).unwrap()
    }

    #[test]
    fn tan_reference_point() {
        // TAN reference point is (0, 90) degrees: it projects to (0, 0).
        let (status, x, y) = prj(WcsProjection::Tan).fwd_raw(0.0, 90.0);
        assert_eq!(status, Status::Ok);
        assert!(x.abs() < 1e-10, "x = {x}");
        assert!(y.abs() < 1e-10, "y = {y}");
    }

    #[test]
    fn round_trips() {
        use WcsProjection::*;
        for (proj, phi0, theta0) in [
            (Tan, 10.0, 80.0),
            (Car, 45.0, 30.0),
            (Sin, 5.0, 85.0),
            (Ait, 30.0, 45.0),
            (Arc, 15.0, 60.0),
            (Stg, 20.0, 70.0),
            (Mer, 25.0, 40.0),
            (Zea, 12.0, 55.0),
            (Mol, 35.0, 50.0),
        ] {
            let p = prj(proj);
            let (s, x, y) = p.fwd_raw(phi0, theta0);
            assert_eq!(s, Status::Ok, "{} forward", proj.code());
            let (s, phi, theta) = p.rev_raw(x, y);
            assert_eq!(s, Status::Ok, "{} reverse", proj.code());
            assert!((phi - phi0).abs() < 1e-10, "{} phi = {phi}", proj.code());
            assert!(
                (theta - theta0).abs() < 1e-10,
                "{} theta = {theta}",
                proj.code()
            );
        }
    }

    #[test]
    fn ncp_gls_and_unknown_are_rejected() {
        // These must be mapped to another projection at the WcsMap level.
        for proj in [
            WcsProjection::Ncp,
            WcsProjection::Gls,
            WcsProjection::Unknown,
        ] {
            assert!(Prj::new(proj, &NO_PARAMS, true).is_err());
        }
    }

    #[test]
    fn zpn_set_up_fails_until_a_coefficient_is_supplied() {
        // ZPN's set-up fails while its coefficients are all zero, and every
        // call returns that status.
        let p = prj(WcsProjection::Zpn);
        assert_ne!(p.fwd_raw(0.0, 80.0).0, Status::Ok);
        assert_ne!(p.rev_raw(0.0, 0.1).0, Status::Ok);
        // The linear coefficient makes the polynomial well defined.
        let p = Prj::new(
            WcsProjection::Zpn,
            &[vec![None, Some(1.0)], Vec::new()],
            true,
        )
        .unwrap();
        assert_eq!(p.fwd_raw(0.0, 80.0).0, Status::Ok);
    }

    #[test]
    fn get_param_out_of_range() {
        let p = prj(WcsProjection::Tan);
        assert!(p.get_param(0, PRJ_PARAM_SLOTS).is_none());
        assert!(p.get_param(2, 0).is_none());
    }

    #[test]
    #[allow(clippy::approx_constant)] // -3.14 is an opaque test parameter, not pi
    fn supplied_parameters_are_copied() {
        let mut lat = vec![None; 6];
        lat[5] = Some(42.0);
        let mut lon = vec![None; 11];
        lon[10] = Some(-3.14);
        let p = Prj::new(WcsProjection::Tan, &[lat, lon], true).unwrap();
        assert_eq!(p.get_param(0, 5), Some(42.0));
        assert_eq!(p.get_param(1, 10), Some(-3.14));
        assert_eq!(p.get_param(0, 4), Some(0.0));
    }

    /// `tpn_tan` selects whether the transformation includes the TAN
    /// projection as well as the polynomial.
    /// With the default unit coefficients the polynomial is the identity,
    /// so clearing it makes the whole projection an identity.
    #[test]
    fn tpn_tan_selects_polynomial_only_form() {
        let with_tan = Prj::new(WcsProjection::Tpn, &NO_PARAMS, true).unwrap();
        let poly_only = Prj::new(WcsProjection::Tpn, &NO_PARAMS, false).unwrap();
        let (_, _, y_tan) = with_tan.fwd_raw(0.0, 60.0);
        let (_, x_poly, y_poly) = poly_only.fwd_raw(0.0, 60.0);
        assert!(
            x_poly.abs() < 1e-10 && (y_poly - 60.0).abs() < 1e-10,
            "polynomial-only TPN should pass its inputs through: ({x_poly}, {y_poly})"
        );
        assert!(
            (y_tan - y_poly).abs() > 1.0,
            "TPNTan should change the projection: {y_tan} vs {y_poly}"
        );
    }

    use crate::test_inputs::{angle_pairs, degrees, specials, ulps};
    use crate::wcslib::trig::{D2R, R2D};

    /// The pair as given: the `prep` and `finish` of a slice entry that
    /// stores the projection's own inputs and outputs.
    fn unchanged(u: f64, v: f64) -> (f64, f64) {
        (u, v)
    }

    /// Bit equality, with every NaN equal to every other.
    fn same_bits_or_both_nan(a: f64, b: f64) -> bool {
        (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
    }

    /// The pairs through `fwd_slice` (or `rev_slice` when `!fwd`) in one
    /// slice.
    fn through_slice(p: &Prj, pairs: &[(f64, f64)], fwd: bool) -> Vec<(f64, f64)> {
        let mut a: Vec<f64> = pairs.iter().map(|q| q.0).collect();
        let mut b: Vec<f64> = pairs.iter().map(|q| q.1).collect();
        if fwd {
            p.fwd_slice(&mut a, &mut b, unchanged, unchanged).unwrap();
        } else {
            p.rev_slice(&mut a, &mut b, unchanged, unchanged).unwrap();
        }
        a.into_iter().zip(b).collect()
    }

    /// Native (phi, theta) for TAN forward: random angles within ±720
    /// degrees paired out of order, random directions converted to degrees
    /// at magnitudes from 2^-40 to 2^40, and every pairing of the specials
    /// and the multiples of 45 degrees.
    fn tan_fwd_inputs() -> Vec<(f64, f64)> {
        let d = degrees(1_000_000);
        let mut out: Vec<(f64, f64)> = d.iter().copied().zip(d.iter().rev().copied()).collect();
        out.extend(
            angle_pairs(500_000)
                .into_iter()
                .map(|(u, v)| (u * R2D, v * R2D)),
        );
        let mut edges = specials();
        edges.extend((-8..=8).map(|k| 45.0 * f64::from(k)));
        for &u in &edges {
            for &v in &edges {
                out.push((u, v));
            }
        }
        out
    }

    /// Projected (x, y) for TAN reverse: random values within ±720
    /// degrees paired out of order, pairs with magnitudes from 2^-40 to
    /// 2^40, and every pairing of the specials.
    fn tan_rev_inputs() -> Vec<(f64, f64)> {
        let d = degrees(1_000_000);
        let mut out: Vec<(f64, f64)> = d.iter().copied().zip(d.iter().rev().copied()).collect();
        out.extend(angle_pairs(500_000));
        let edges = specials();
        for &u in &edges {
            for &v in &edges {
                out.push((u, v));
            }
        }
        out
    }

    /// The largest ULP distance and relative difference of each output of a
    /// slice entry from the per-value entry where it projects the point, and
    /// how many outputs differ; a bad point must be bad in both.
    fn slice_drift(p: &Prj, pairs: &[(f64, f64)], fwd: bool) -> ([u64; 2], [f64; 2], usize) {
        let got = through_slice(p, pairs, fwd);
        let mut max = [0u64; 2];
        let mut rel = [0.0f64; 2];
        let mut differ = 0;
        for (&(u, v), &(x, y)) in pairs.iter().zip(&got) {
            let bad_in = u == BAD_VALUE || v == BAD_VALUE || u.is_nan() || v.is_nan();
            let (status, wx, wy) = if fwd {
                p.fwd_raw(u, v)
            } else {
                p.rev_raw(u, v)
            };
            if bad_in || status == Status::BadPoint {
                assert!(
                    x == BAD_VALUE && y == BAD_VALUE,
                    "({u:e}, {v:e}): slice gives ({x:e}, {y:e}) for a bad point"
                );
                continue;
            }
            assert_eq!(status, Status::Ok, "({u:e}, {v:e})");
            assert!(
                !(x == BAD_VALUE && y == BAD_VALUE),
                "({u:e}, {v:e}): slice gives a bad point for ({wx:e}, {wy:e})"
            );
            for (k, (g, w)) in [(x, wx), (y, wy)].into_iter().enumerate() {
                if same_bits_or_both_nan(g, w) {
                    continue;
                }
                differ += 1;
                let d = ulps(g, w);
                assert!(d != u64::MAX, "({u:e}, {v:e}): output {k} {g:e} vs {w:e}");
                max[k] = max[k].max(d);
                // Equal values with different bits are zeros of either sign.
                let r = if g == w { 0.0 } else { (g - w).abs() / w.abs() };
                assert!(!r.is_nan(), "({u:e}, {v:e}): output {k} {g:e} vs {w:e}");
                rel[k] = rel[k].max(r);
            }
        }
        (max, rel, differ)
    }

    /// How far TAN's forward slice outputs may be from the per-value
    /// entry's, in ULP. Each output is `r0 * cosd(theta) / sind(theta)`
    /// times `sind(phi)` or `cosd(phi)`, three operations on three kernel
    /// values. Each kernel value is within 2 ULP of libm's (see [`Kernel`]),
    /// at most 2 * 2^-52 of it, so the exact results of the operations on
    /// the two paths' values differ by at most 3 * 2 * 2^-52 relative; each
    /// path rounds its three operations once each, at most 2^-53 relative
    /// apiece, 3 * 2^-52 between the two paths, since their operands differ
    /// and they need not round alike. That is a relative difference of up
    /// to 9 * 2^-52, to first order. An ULP of the output is at least 2^-53
    /// of it, so the bound is 18 ULP. Measured (2026-10-28): 5 against
    /// macOS libm on arm64 (4 in x, 5 in y), and 6 in both against macOS's
    /// x86-64 libm.
    const TAN_FWD_SLICE_ULPS: u64 = 18;

    /// TAN's forward slice runs the kernels and is within their drift of the
    /// per-value entry; its reverse slice runs [`Libm`] and is bit for bit
    /// the per-value entry.
    #[test]
    fn tan_slices_are_within_kernel_drift_of_the_per_value_body() {
        let p = prj(WcsProjection::Tan);
        let (fwd_max, _, fwd_differ) = slice_drift(&p, &tan_fwd_inputs(), true);
        assert!(
            fwd_max.iter().all(|&m| m <= TAN_FWD_SLICE_ULPS),
            "forward: {fwd_max:?} ULP"
        );
        assert!(
            fwd_differ > 0,
            "forward: no output differs; is the kernel in use?"
        );
        let (rev_max, _, rev_differ) = slice_drift(&p, &tan_rev_inputs(), false);
        assert_eq!((rev_max, rev_differ), ([0, 0], 0), "reverse");
    }

    /// Inputs with specials and multiples of 90 degrees interleaved among
    /// random angles.
    fn interleaved_inputs() -> Vec<(f64, f64)> {
        let d = degrees(3000);
        let mut edges = specials();
        edges.extend((-8..=8).map(|k| 90.0 * f64::from(k)));
        let mut out = Vec::new();
        for (i, &u) in d.iter().enumerate() {
            let v = d[(i * 7 + 3) % d.len()];
            match i % 5 {
                0 => out.push((edges[i % edges.len()], v)),
                1 => out.push((u, edges[(i / 5) % edges.len()])),
                _ => out.push((u, v)),
            }
        }
        out
    }

    /// Each pair gets the same bits through either slice entry of `p`
    /// whatever the length of the slice it is in.
    fn assert_same_bits_at_any_length(p: &Prj, pairs: &[(f64, f64)]) {
        for fwd in [true, false] {
            let whole = through_slice(p, pairs, fwd);
            for len in [1, 2, 3, 7, 1001] {
                for (start, chunk) in pairs.chunks(len).enumerate() {
                    let got = through_slice(p, chunk, fwd);
                    for (i, (g, w)) in got.iter().zip(&whole[start * len..]).enumerate() {
                        assert!(
                            same_bits_or_both_nan(g.0, w.0) && same_bits_or_both_nan(g.1, w.1),
                            "fwd {fwd}, length {len}: {:?} gives {g:?} alone, {w:?} in the whole",
                            chunk[i]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn tan_slices_give_each_point_the_same_bits_at_any_length() {
        assert_same_bits_at_any_length(&prj(WcsProjection::Tan), &interleaved_inputs());
    }

    /// Both builds of `K`'s kernel loop, with and without fused
    /// multiply-adds, give the same bits.
    fn assert_builds_agree<K: KernelBody>(p: &Prj, pairs: &[(f64, f64)]) {
        let run = |fma: bool| {
            let mut a: Vec<f64> = pairs.iter().map(|q| q.0).collect();
            let mut b: Vec<f64> = pairs.iter().map(|q| q.1).collect();
            let prm = &p.prm;
            if fma {
                kernel_slice_body::<K, true>(prm, &mut a, &mut b, "wcs_fwd").unwrap();
            } else {
                kernel_slice_body::<K, false>(prm, &mut a, &mut b, "wcs_fwd").unwrap();
            }
            (a, b)
        };
        let (a0, b0) = run(false);
        let (a1, b1) = run(true);
        for i in 0..a0.len() {
            assert!(
                same_bits_or_both_nan(a0[i], a1[i]) && same_bits_or_both_nan(b0[i], b1[i]),
                "{}: {:?} gives ({:e}, {:e}) and ({:e}, {:e})",
                p.proj.code(),
                pairs[i],
                a0[i],
                b0[i],
                a1[i],
                b1[i]
            );
        }
    }

    #[test]
    fn tan_slice_builds_agree() {
        assert_builds_agree::<TanFwd>(&prj(WcsProjection::Tan), &interleaved_inputs());
    }

    #[test]
    fn tan_exact_points_stay_exact_in_a_slice() {
        let p = prj(WcsProjection::Tan);
        let mut phi = [10.0, 20.0, 30.0, 0.0, 40.0, 50.0, 60.0];
        let mut theta = [80.0, 70.0, 60.0, 90.0, 50.0, 40.0, 30.0];
        p.fwd_slice(&mut phi, &mut theta, unchanged, unchanged)
            .unwrap();
        // Exactly zero, with the signs the per-value entry gives.
        let (_, wx, wy) = p.fwd_raw(0.0, 90.0);
        assert_eq!((phi[3], theta[3]), (0.0, 0.0));
        assert_eq!(
            (phi[3].to_bits(), theta[3].to_bits()),
            (wx.to_bits(), wy.to_bits())
        );
        let mut x = [1.0, 2.0, 3.0, 0.0, 4.0, 5.0, 6.0];
        let mut y = [-1.0, -2.0, -3.0, 0.0, -4.0, -5.0, -6.0];
        p.rev_slice(&mut x, &mut y, unchanged, unchanged).unwrap();
        assert_eq!(
            (x[3].to_bits(), y[3].to_bits()),
            (0.0f64.to_bits(), 90.0f64.to_bits())
        );
    }

    /// SIN with the given latitude-axis parameters `p1` and `p2`: the
    /// orthographic projection when both are zero, otherwise the synthesis
    /// projection.
    fn sin_prj(p1: f64, p2: f64) -> Prj {
        Prj::new(
            WcsProjection::Sin,
            &[vec![None, Some(p1), Some(p2)], Vec::new()],
            true,
        )
        .unwrap()
    }

    /// The SIN parameter sets the tests use: orthographic; NCP's, for a
    /// field at declination 30 degrees; and a general synthesis projection.
    fn sin_prjs() -> [Prj; 3] {
        [
            sin_prj(0.0, 0.0),
            sin_prj(0.0, 1.0 / crate::math::tan(30.0f64.to_radians())),
            sin_prj(0.125, -0.2),
        ]
    }

    /// Native (phi, theta) for SIN forward: TAN's, and random longitudes at
    /// latitudes within 1e-9 to 1e-1 degrees of either pole, at logarithmic
    /// intervals, which straddle the change to the small angle formulae.
    fn sin_fwd_inputs() -> Vec<(f64, f64)> {
        let mut out = tan_fwd_inputs();
        let d = degrees(40_000);
        for (i, &u) in d.iter().enumerate() {
            let t = (i % 1001) as f64 / 1000.0;
            let delta = crate::math::powf(10.0, -9.0 + 8.0 * t);
            let pole = if i % 2 == 0 { 90.0 } else { -90.0 };
            out.push((u, pole - delta.copysign(pole)));
        }
        out
    }

    /// The largest ratio, over the outputs of SIN's forward slice that differ
    /// from the per-value entry's, of the difference to its bound.
    /// Each output is `r0 (a + p z)`, with `a` the product of a sine or
    /// cosine of `phi` and `cthe`, `z` `1 - sind(theta)` and `p` the
    /// parameter `p1` or `-p2` (away from the pole; nearer it, `cthe` and
    /// `z` are from `theta` alone and differ less). The kernels' sine and
    /// cosine are within 2 ULP, at most 2 * 2^-52 relative, of Libm's (see
    /// [`Kernel`]), and each path rounds every operation once, at most
    /// 2^-53 relative, 2^-52 between the two paths. So `a` differs by at
    /// most 5 * 2^-52 |a| (2 for each factor and 1 for the product's
    /// rounding); `z` by at most 2 * 2^-52 from the sine, which is at most
    /// 1 in magnitude, and 2^-52 |z| from the subtraction, so `p z` by at
    /// most 2^-52 |p| (2 + 2 |z|) with its product's rounding; and the sum
    /// and the product with `r0` by at most 2^-52 of the output each. To
    /// first order the bound is `2^-52 (r0 (5 |a| + |p| (2 + 2 |z|)) +
    /// 2 |out|)`.
    fn sin_fwd_drift_over_bound(p: &Prj, pairs: &[(f64, f64)]) -> f64 {
        let got = through_slice(p, pairs, true);
        let prm = &p.prm;
        let mut worst = 0.0f64;
        for (&(u, v), &(x, y)) in pairs.iter().zip(&got) {
            let bad_in = u == BAD_VALUE || v == BAD_VALUE || u.is_nan() || v.is_nan();
            let (status, wx, wy) = p.fwd_raw(u, v);
            if bad_in || status != Status::Ok {
                continue;
            }
            // `cthe` and `z` as the body has them, and whether `z` is from a
            // sine.
            let t = (90.0 - v.abs()) * D2R;
            let (cthe, z, z_from_sine) = if t < 1.0e-5 {
                let z = if v > 0.0 {
                    t * t / 2.0
                } else {
                    2.0 - t * t / 2.0
                };
                (t, z, 0.0)
            } else {
                (Libm::cosd(v), 1.0 - Libm::sind(v), 1.0)
            };
            let terms = [
                (x, wx, cthe * Libm::sind(u), prm.p[1]),
                (y, wy, cthe * Libm::cosd(u), prm.p[2]),
            ];
            for (g, w, a, pz) in terms {
                if same_bits_or_both_nan(g, w) {
                    continue;
                }
                let bound = f64::EPSILON
                    * (prm.r0 * (5.0 * a.abs() + z_from_sine * pz.abs() * (2.0 + 2.0 * z.abs()))
                        + 2.0 * w.abs());
                let ratio = (g - w).abs() / bound;
                assert!(!ratio.is_nan(), "({u:e}, {v:e}): {g:e} vs {w:e}");
                worst = worst.max(ratio);
            }
        }
        worst
    }

    /// SIN's forward slice runs the kernels and is within their drift of the
    /// per-value entry, with the same bad points, for the orthographic and
    /// the synthesis projections.
    #[test]
    fn sin_slices_are_within_kernel_drift_of_the_per_value_body() {
        let inputs = sin_fwd_inputs();
        for p in sin_prjs() {
            let (fwd_ulps, _, fwd_differ) = slice_drift(&p, &inputs, true);
            let over = sin_fwd_drift_over_bound(&p, &inputs);
            assert!(
                over <= 1.0,
                "forward {:?}: {over} of the bound ({fwd_ulps:?} ULP)",
                &p.prm.p[1..3]
            );
            assert!(
                fwd_differ > 0,
                "forward: no output differs; is the kernel in use?"
            );
            let (rev_ulps, _, rev_differ) = slice_drift(&p, &tan_rev_inputs(), false);
            assert_eq!((rev_ulps, rev_differ), ([0, 0], 0), "reverse");
        }
    }

    #[test]
    fn sin_slices_give_each_point_the_same_bits_at_any_length() {
        for p in sin_prjs() {
            assert_same_bits_at_any_length(&p, &interleaved_inputs());
        }
    }

    #[test]
    fn sin_slice_builds_agree() {
        let [ortho, ncp, synthesis] = sin_prjs();
        assert_builds_agree::<SinFwdOrthographic>(&ortho, &interleaved_inputs());
        assert_builds_agree::<SinFwdSynthesis>(&ncp, &interleaved_inputs());
        assert_builds_agree::<SinFwdSynthesis>(&synthesis, &interleaved_inputs());
    }

    /// The pole projects to exactly the per-value entry's bits in a slice.
    #[test]
    fn sin_exact_points_stay_exact_in_a_slice() {
        for p in sin_prjs() {
            let mut phi = [10.0, 20.0, 30.0, 0.0, 40.0, 50.0, 60.0];
            let mut theta = [80.0, 70.0, 60.0, 90.0, 50.0, 40.0, 30.0];
            p.fwd_slice(&mut phi, &mut theta, unchanged, unchanged)
                .unwrap();
            let (_, wx, wy) = p.fwd_raw(0.0, 90.0);
            assert_eq!(
                (phi[3].to_bits(), theta[3].to_bits()),
                (wx.to_bits(), wy.to_bits())
            );
        }
    }

    /// Points at and a few ULP either side of the synthesis projection's
    /// lowest valid latitude, as the per-value entry computes it, are bad in
    /// the forward slice exactly when they are bad in the per-value entry.
    #[test]
    fn sin_synthesis_limit_points_agree_on_bad_or_good() {
        let [_, ncp, synthesis] = sin_prjs();
        for p in [ncp, synthesis] {
            let prm = &p.prm;
            let mut pairs = Vec::new();
            let mut bad = 0;
            for &phi in &degrees(20_000) {
                let limit = -Libm::atand(prm.p[1] * Libm::sind(phi) - prm.p[2] * Libm::cosd(phi));
                // A few ULP, a few multiples of 2^-46, the ULP of a limit
                // between 64 and 90 degrees, and up to a few times the
                // margin within which the slice redoes the point.
                for n in -8i32..=8 {
                    let n64 = f64::from(n);
                    for theta in [
                        ulps_away(limit, i64::from(n)),
                        limit + n64 * crate::math::powf(2.0, -46.0),
                        limit + n64 * 1e-13,
                    ] {
                        bad += usize::from(p.fwd_raw(phi, theta).0 != Status::Ok);
                        pairs.push((phi, theta));
                    }
                }
            }
            assert!(bad > 0 && bad < pairs.len(), "{bad} of {} bad", pairs.len());
            slice_drift(&p, &pairs, true);
            let over = sin_fwd_drift_over_bound(&p, &pairs);
            assert!(over <= 1.0, "{over} of the bound");
        }
    }

    /// ZPN with the latitude-axis parameters `p`, from `p[0]`.
    fn zpn_prj(p: &[f64]) -> Prj {
        let pv: Vec<Option<f64>> = p.iter().map(|&v| Some(v)).collect();
        Prj::new(WcsProjection::Zpn, &[pv, Vec::new()], true).unwrap()
    }

    /// The ZPN parameter sets the tests use, one per branch of the reverse
    /// projection's root search: linear; quadratic, bad beyond 2.5 radians
    /// from the pole; the cubic of the benchmarks, with no point of
    /// inflection; and a quintic with a point of inflection.
    fn zpn_prjs() -> [Prj; 4] {
        [
            zpn_prj(&[0.001, 1.0]),
            zpn_prj(&[0.0, 1.0, -0.1]),
            zpn_prj(&[0.0, 1.0, 0.0, 0.05]),
            zpn_prj(&[0.0, 1.0, 0.0, -0.08, 0.0, 0.002]),
        ]
    }

    /// Projected (x, y) for ZPN reverse: random values within ±720 degrees
    /// paired out of order, pairs with magnitudes from 2^-40 to 2^40, and
    /// every pairing of the specials and the multiples of 45 degrees.
    fn zpn_rev_inputs() -> Vec<(f64, f64)> {
        let d = degrees(200_000);
        let mut out: Vec<(f64, f64)> = d.iter().copied().zip(d.iter().rev().copied()).collect();
        out.extend(angle_pairs(100_000));
        let mut edges = specials();
        edges.extend((-8..=8).map(|k| 45.0 * f64::from(k)));
        for &u in &edges {
            for &v in &edges {
                out.push((u, v));
            }
        }
        out
    }

    /// How far ZPN's forward slice outputs may be from the per-value
    /// entry's, in ULP. Each output is the polynomial, computed alike on
    /// both paths, times one kernel sine or cosine, which is within 2 ULP of
    /// libm's (see [`Kernel`]), at most 2 * 2^-52 relative. So the exact
    /// products differ by at most 2 * 2^-52 of the output, which is at most
    /// 4 ULP of it, and the two paths' roundings add at most 1 ULP between
    /// them: the bound is 5 ULP. Measured (2026-10-29): 2 against macOS libm
    /// on arm64, and 4 against its x86-64 libm.
    const ZPN_FWD_SLICE_ULPS: u64 = 5;

    /// ZPN's forward slice runs the kernels and is within
    /// [`ZPN_FWD_SLICE_ULPS`] of the per-value entry, with the same bad
    /// points; its reverse slice runs [`Libm`] and is bit for bit the
    /// per-value entry.
    #[test]
    fn zpn_slices_are_within_kernel_drift_of_the_per_value_body() {
        let fwd_inputs = tan_fwd_inputs();
        let rev_inputs = zpn_rev_inputs();
        for p in zpn_prjs() {
            let n = p.prm.n;
            let (fwd_max, _, fwd_differ) = slice_drift(&p, &fwd_inputs, true);
            assert!(
                fwd_max.iter().all(|&m| m <= ZPN_FWD_SLICE_ULPS),
                "degree {n} forward: {fwd_max:?} ULP"
            );
            assert!(fwd_differ > 0, "degree {n} forward: no output differs");
            let (rev_max, _, rev_differ) = slice_drift(&p, &rev_inputs, false);
            assert_eq!((rev_max, rev_differ), ([0, 0], 0), "degree {n} reverse");
        }
    }

    #[test]
    fn zpn_slices_give_each_point_the_same_bits_at_any_length() {
        for p in zpn_prjs() {
            assert_same_bits_at_any_length(&p, &interleaved_inputs());
        }
    }

    #[test]
    fn zpn_slice_builds_agree() {
        for p in zpn_prjs() {
            assert_builds_agree::<ZpnFwd>(&p, &interleaved_inputs());
        }
    }

    /// The pole, and longitudes at multiples of 90 degrees, which the
    /// kernels do not cover, get the per-value entry's bits in a slice.
    #[test]
    fn zpn_exact_points_stay_exact_in_a_slice() {
        for p in zpn_prjs() {
            let mut pairs = vec![(0.0, 90.0), (-0.0, 90.0), (10.0, 80.0)];
            for k in -4..=4 {
                let a = 90.0 * f64::from(k);
                pairs.extend([(a, 80.0), (a, 90.0), (a, -10.0)]);
            }
            let got = through_slice(&p, &pairs, true);
            for (&(u, v), &(x, y)) in pairs.iter().zip(&got) {
                let (status, wx, wy) = p.fwd_raw(u, v);
                let want = if status == Status::Ok {
                    (wx, wy)
                } else {
                    (BAD_VALUE, BAD_VALUE)
                };
                assert_eq!(
                    (x.to_bits(), y.to_bits()),
                    (want.0.to_bits(), want.1.to_bits()),
                    "({u}, {v})"
                );
            }
        }
    }

    use crate::wcslib::tpn::{
        NEWTON_MAX_ITERATIONS, NEWTON_RELATIVE_TOLERANCE, tpn_fwd_solve, tpn_fwd_step_noise,
    };

    /// The TPN projection of the DSS plate solution in
    /// `tests/fixtures/wcsconv/inputs/dss.ast`: its `PV1_m`, of the
    /// longitude axis, and `PV2_m`, of the latitude axis.
    #[allow(clippy::excessive_precision)] // the fixture's values, as written there
    fn dss_prj(tpn_tan: bool) -> Prj {
        let pv1 = [
            (0, 0.00037777813768480556),
            (1, 0.018675372165510556),
            (2, 1.4659181119170556e-05),
            (4, -5.6541834490241662e-09),
            (5, -1.6598619578175834e-10),
            (6, 3.324645548432778e-09),
            (7, 6.8029375162963896e-10),
            (8, -1.0315391309210556e-11),
            (9, 6.5770184096316667e-10),
            (10, 4.6843790588691666e-11),
        ];
        let pv2 = [
            (0, 0.00020734395690532499),
            (1, 0.018675089806542779),
            (2, -1.6578391725152224e-05),
            (4, -5.1378767937980552e-09),
            (5, -1.7623932712259446e-09),
            (6, 2.7161547313251387e-10),
            (7, 7.088907407099166e-10),
            (8, 1.8432618513145277e-11),
            (9, 6.8491061989569442e-10),
            (10, 7.3325859634708332e-13),
        ];
        let axis = |pv: &[(usize, f64)]| {
            let mut out = vec![None; 22];
            for &(m, v) in pv {
                out[m] = Some(v);
            }
            for m in [17, 19, 21] {
                out[m] = Some(0.0);
            }
            out
        };
        Prj::new(WcsProjection::Tpn, &[axis(&pv2), axis(&pv1)], tpn_tan).unwrap()
    }

    /// Native (phi, theta) for TPN forward: random angles within ±720
    /// degrees paired out of order; random points on a six-degree plate
    /// about the pole; random points between the pole and 9e-11 degrees
    /// above the equator at logarithmic intervals, where the Newton
    /// iteration takes up to 49 steps; and every pairing of the specials
    /// and the multiples of 45 degrees.
    fn tpn_fwd_inputs(p: &Prj) -> Vec<(f64, f64)> {
        let d = degrees(100_000);
        let mut out: Vec<(f64, f64)> = d.iter().copied().zip(d.iter().rev().copied()).collect();
        let e = degrees(100_000);
        out.extend(d.iter().zip(&e).map(|(&u, &v)| (u, 90.0 - v.abs() / 120.0)));
        out.extend(
            d.iter()
                .zip(&e)
                .take(20_000)
                .map(|(&u, &v)| (u, 90.0 * crate::math::powf(10.0, -12.0 * v.abs() / 720.0))),
        );
        out.extend(tpn_axis_images(p, &d[..20_000]));
        let mut edges = specials();
        edges.extend((-8..=8).map(|k| 45.0 * f64::from(k)));
        for &u in &edges {
            for &v in &edges {
                out.push((u, v));
            }
        }
        out
    }

    /// The native (phi, theta) of the points (0, c) and (c, 0) of the
    /// projection plane, for each `c`: points whose forward projection has a
    /// coordinate at zero, below the rounding floor of the Newton
    /// iteration's convergence test.
    fn tpn_axis_images(p: &Prj, coords: &[f64]) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        for &c in coords {
            for (x, y) in [(0.0, c), (c, 0.0)] {
                let (status, phi, theta) = p.rev_raw(x, y);
                assert_eq!(status, Status::Ok);
                out.push((phi, theta));
            }
        }
        out
    }

    /// The largest ratio, over the outputs of TPN's forward slice that
    /// differ from the per-value entry's, of the difference to its bound,
    /// `NEWTON_RELATIVE_TOLERANCE^2 |x| + 2 nu`, where nu is
    /// `tpn_fwd_step_noise` at the per-value entry's last step.
    /// The slice runs the same Newton iteration as the per-value entry on a
    /// front end within a few ULP of it, and accepts a point only where both
    /// coordinates are above the floor below which rounding decides the
    /// convergence test. Where the two iterations take the same number of
    /// steps, the outputs differ by the two last steps' rounding, at most
    /// nu, since Newton's method cancels an earlier difference to first
    /// order. Where a convergence test at its threshold passes on one path
    /// and fails on the other, the path that goes on takes one more step,
    /// of order `NEWTON_RELATIVE_TOLERANCE^2 |x|` by quadratic convergence,
    /// with its own rounding, at most nu.
    fn tpn_drift_over_bound(p: &Prj, pairs: &[(f64, f64)]) -> f64 {
        let got = through_slice(p, pairs, true);
        pairs
            .iter()
            .zip(&got)
            .map(|(&(u, v), &(x, y))| tpn_drift_ratio(p, u, v, x, y))
            .fold(0.0, f64::max)
    }

    /// [`tpn_drift_over_bound`] for one point (`u`, `v`) given (`x`, `y`)
    /// for it; 0 where either output is bit for bit the per-value entry's,
    /// or where the per-value entry says the point is bad.
    fn tpn_drift_ratio(p: &Prj, u: f64, v: f64, x: f64, y: f64) -> f64 {
        if u == BAD_VALUE || v == BAD_VALUE || u.is_nan() || v.is_nan() {
            return 0.0;
        }
        let (status, xi, eta) = tpn_fwd_front::<Libm>(&p.prm, u, v);
        if status != Status::Ok {
            return 0.0;
        }
        let solved = tpn_fwd_solve(&p.prm, xi, eta, NEWTON_MAX_ITERATIONS);
        if solved.status != Status::Ok {
            return 0.0;
        }
        let (wx, wy) = (solved.x, solved.y);
        let jacobian = solved.jacobian.expect("TPN with a polynomial iterates");
        let ([nu_x, nu_y], det) = tpn_fwd_step_noise(&p.prm, wx, wy, jacobian);
        let tol2 = NEWTON_RELATIVE_TOLERANCE * NEWTON_RELATIVE_TOLERANCE;
        let mut worst = 0.0f64;
        for (g, w, nu) in [(x, wx, nu_x / det), (y, wy, nu_y / det)] {
            if same_bits_or_both_nan(g, w) {
                continue;
            }
            let ratio = (g - w).abs() / (tol2 * w.abs() + 2.0 * nu);
            assert!(!ratio.is_nan(), "({u:e}, {v:e}): {g:e} vs {w:e}");
            worst = worst.max(ratio);
        }
        worst
    }

    /// TPN's forward slice runs its front end with the kernels and is within
    /// the Newton iteration's bound of the per-value entry, with the same bad
    /// points; without the TAN term it has no trigonometry, and its reverse
    /// slice runs [`Libm`], so those are bit for bit the per-value entry.
    #[test]
    fn tpn_slices_are_within_kernel_drift_of_the_per_value_body() {
        let p = dss_prj(true);
        let inputs = tpn_fwd_inputs(&p);
        let (fwd_ulps, fwd_rel, fwd_differ) = slice_drift(&p, &inputs, true);
        let over = tpn_drift_over_bound(&p, &inputs);
        assert!(
            over <= 1.0,
            "forward: {over} of the bound ({fwd_rel:?} relative, {fwd_ulps:?} ULP)"
        );
        assert!(
            fwd_differ > 0,
            "forward: no output differs; is the kernel in use?"
        );
        let (rev_ulps, _, rev_differ) = slice_drift(&p, &tan_rev_inputs(), false);
        assert_eq!((rev_ulps, rev_differ), ([0, 0], 0), "reverse");
        let (ulps, _, differ) = slice_drift(&dss_prj(false), &inputs, true);
        assert_eq!((ulps, differ), ([0, 0], 0), "forward without the TAN term");
    }

    /// Points on the axes of the projection plane are below the floor of the
    /// convergence test and are redone with [`Libm`], so they get the
    /// per-value entry's bits, bad or good.
    #[test]
    fn tpn_axis_points_match_the_per_value_entry_in_a_slice() {
        let p = dss_prj(true);
        let pairs = tpn_axis_images(&p, &degrees(20_000));
        let got = through_slice(&p, &pairs, true);
        let mut bad = 0;
        for (&(u, v), &(x, y)) in pairs.iter().zip(&got) {
            let (status, wx, wy) = p.fwd_raw(u, v);
            let want = if status == Status::Ok {
                (wx, wy)
            } else {
                bad += 1;
                (BAD_VALUE, BAD_VALUE)
            };
            assert_eq!(
                (x.to_bits(), y.to_bits()),
                (want.0.to_bits(), want.1.to_bits()),
                "({u:e}, {v:e})"
            );
        }
        // Some of these points are bad in the per-value entry, its iteration
        // stepping about a coordinate that is rounding noise.
        assert!(bad > 0, "no axis point is bad; is the class exercised?");
    }

    #[test]
    fn tpn_slices_give_each_point_the_same_bits_at_any_length() {
        assert_same_bits_at_any_length(&dss_prj(true), &interleaved_inputs());
    }

    #[test]
    fn tpn_slice_builds_agree() {
        assert_builds_agree::<TpnFwd>(&dss_prj(true), &interleaved_inputs());
    }

    /// `v` moved `n` ULP away from zero (towards it for negative `n`).
    fn ulps_away(v: f64, n: i64) -> f64 {
        f64::from_bits(v.to_bits().wrapping_add_signed(n))
    }

    /// Points just off an axis of the projection plane, from 1e-16 to 1e-6
    /// of a plane unit from it, near the floor below which the slice redoes
    /// a point: bad in the slice exactly when bad in the per-value entry, and
    /// each output either the per-value entry's bits or within the bound.
    #[test]
    fn tpn_points_near_the_floor_agree_and_stay_within_the_bound() {
        let p = dss_prj(true);
        let c = degrees(4000);
        let mut pairs = Vec::new();
        for (i, &c) in c.iter().enumerate() {
            let t = (i % 101) as f64 / 100.0;
            let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
            let delta = sign * crate::math::powf(10.0, -6.0 - 10.0 * t);
            for (x, y) in [(delta, c), (c, delta)] {
                let (status, phi, theta) = p.rev_raw(x, y);
                assert_eq!(status, Status::Ok);
                pairs.push((phi, theta));
            }
        }
        slice_drift(&p, &pairs, true);
        let over = tpn_drift_over_bound(&p, &pairs);
        assert!(over <= 1.0, "{over} of the bound");
    }

    /// Points whose per-value forward projection with [`dss_prj`]'s
    /// parameters converges at the given Newton iteration, found by searching
    /// random points near the equator for high iteration counts. Those
    /// converging at the 51st are bad: the per-value entry stops at the 50th.
    const TPN_CAP_POINTS: [(usize, f64, f64); 9] = [
        (49, 28.458582279307073, 4.709692032133452e-11),
        (49, -158.55330477812421, 3.7952685308572407e-11),
        (49, -134.31793310169147, 6.35517181385545e-11),
        (50, 55.45920210984241, 2.801028035332779e-11),
        (50, 90.91227084307462, 3.30257334268539e-11),
        (50, -21.350194372456656, 3.669788568433079e-11),
        (51, -113.96782653838751, 1.1827860341910004e-11),
        (51, -160.2045647649468, 1.5607410389929512e-11),
        (51, -154.86670272665023, 1.827352429797668e-11),
    ];

    /// A point whose iteration converges at or next to the per-value entry's
    /// limit is bad in the forward slice exactly when it is bad in the
    /// per-value entry.
    #[test]
    fn tpn_cap_points_agree_on_bad_or_good() {
        let p = dss_prj(true);
        for &(k, phi, theta) in &TPN_CAP_POINTS {
            let (status, xi, eta) = tpn_fwd_front::<Libm>(&p.prm, phi, theta);
            assert_eq!(status, Status::Ok, "({phi}, {theta})");
            assert_eq!(
                (
                    tpn_fwd_solve(&p.prm, xi, eta, k - 1).status,
                    tpn_fwd_solve(&p.prm, xi, eta, k).status
                ),
                (Status::BadPoint, Status::Ok),
                "({phi}, {theta}) should converge at iteration {k}"
            );
            assert_eq!(
                p.fwd_raw(phi, theta).0 == Status::Ok,
                k <= NEWTON_MAX_ITERATIONS,
                "({phi}, {theta})"
            );
            // The slice keeps the kernel-fed iteration's result for a point
            // that converges within its limit, so the test covers that
            // branch and not only the redo. A front end 2 ULP from Libm's in
            // each output stands in for the kernels', whose difference from
            // Libm's depends on the platform's libm.
            if k < NEWTON_MAX_ITERATIONS {
                for (sx, sy) in [(2, 2), (2, -2), (-2, 2), (-2, -2)] {
                    let nudged = (Status::Ok, ulps_away(xi, sx), ulps_away(eta, sy));
                    let (s, x, y) = tpn_fwd_finish(&p.prm, phi, theta, nudged)
                        .unwrap_or_else(|| panic!("({phi}, {theta}): {nudged:?} is not accepted"));
                    assert_eq!(s, Status::Ok);
                    let over = tpn_drift_ratio(&p, phi, theta, x, y);
                    assert!(over <= 1.0, "({phi}, {theta}): {over} of the bound");
                }
            }
        }
        // Alone, together, and among other points and points on the axes of
        // the projection plane; `slice_drift` asserts the agreement on bad
        // and good.
        let cap: Vec<(f64, f64)> = TPN_CAP_POINTS.iter().map(|&(_, u, v)| (u, v)).collect();
        let mut mixed = interleaved_inputs();
        mixed.extend(tpn_axis_images(&p, &degrees(2000)));
        for (i, &q) in cap.iter().enumerate() {
            mixed.insert(i * 97 + 5, q);
        }
        for pairs in cap.chunks(1).chain([&cap[..], &mixed[..]]) {
            slice_drift(&p, pairs, true);
            let over = tpn_drift_over_bound(&p, pairs);
            assert!(over <= 1.0, "{over} of the bound");
        }
    }

    /// A point with an angle at a multiple of 90 degrees, which the kernels
    /// do not cover, gets the per-value entry's bits in a slice.
    #[test]
    fn tpn_points_the_kernels_do_not_cover_are_exact_in_a_slice() {
        let p = dss_prj(true);
        let mut pairs = Vec::new();
        for k in -4..=4 {
            let a = 90.0 * f64::from(k);
            pairs.extend([
                (a, 89.5),
                (10.0, a),
                (a, a),
                (-170.0, 89.0 + f64::from(k) / 8.0),
            ]);
        }
        let got = through_slice(&p, &pairs, true);
        for (&(u, v), &(x, y)) in pairs.iter().zip(&got) {
            if tpn_fwd_needs_special::<Kernel<{ crate::vmath::HAS_FMA }>>(&p.prm, u, v) {
                let (status, wx, wy) = p.fwd_raw(u, v);
                let want = if status == Status::Ok {
                    (wx, wy)
                } else {
                    (BAD_VALUE, BAD_VALUE)
                };
                assert_eq!(
                    (x.to_bits(), y.to_bits()),
                    (want.0.to_bits(), want.1.to_bits()),
                    "({u}, {v})"
                );
            }
        }
    }

    use crate::wcslib::zenithal::{azp_fwd_t_near_zero, stg_fwd_s_near_zero, szp_fwd_t_near_zero};

    /// The largest ratio, over the outputs of `p`'s forward slice (reverse
    /// slice when `!fwd`) that differ from the per-value entry's, of the
    /// difference to `bound(u, v, x, y)`, the bound on each output's drift at
    /// the input (`u`, `v`) whose per-value outputs are (`x`, `y`).
    fn drift_over_bound(
        p: &Prj,
        pairs: &[(f64, f64)],
        fwd: bool,
        bound: impl Fn(f64, f64, f64, f64) -> [f64; 2],
    ) -> f64 {
        let got = through_slice(p, pairs, fwd);
        let mut worst = 0.0f64;
        for (&(u, v), &(x, y)) in pairs.iter().zip(&got) {
            let bad_in = u == BAD_VALUE || v == BAD_VALUE || u.is_nan() || v.is_nan();
            let (status, wx, wy) = if fwd {
                p.fwd_raw(u, v)
            } else {
                p.rev_raw(u, v)
            };
            if bad_in || status != Status::Ok {
                continue;
            }
            let mut b = None;
            for (k, (g, w)) in [(x, wx), (y, wy)].into_iter().enumerate() {
                if same_bits_or_both_nan(g, w) {
                    continue;
                }
                let b = b.get_or_insert_with(|| bound(u, v, wx, wy));
                let ratio = (g - w).abs() / b[k];
                assert!(!ratio.is_nan(), "({u:e}, {v:e}): output {k} {g:e} vs {w:e}");
                worst = worst.max(ratio);
            }
        }
        worst
    }

    /// Each pair the predicate of `K` sends to [`Libm`] gets the per-value
    /// entry's bits in a slice, bad or good, among others the kernels run.
    fn assert_special_points_exact<K: KernelBody>(p: &Prj, pairs: &[(f64, f64)], fwd: bool) {
        let got = through_slice(p, pairs, fwd);
        let mut special = 0;
        for (&(u, v), &(x, y)) in pairs.iter().zip(&got) {
            if !K::needs_special::<Kernel<{ crate::vmath::HAS_FMA }>>(&p.prm, u, v) {
                continue;
            }
            special += 1;
            let (status, wx, wy) = if fwd {
                p.fwd_raw(u, v)
            } else {
                p.rev_raw(u, v)
            };
            let want = if status == Status::Ok {
                (wx, wy)
            } else {
                (BAD_VALUE, BAD_VALUE)
            };
            assert!(
                same_bits_or_both_nan(x, want.0) && same_bits_or_both_nan(y, want.1),
                "({u}, {v}): ({x:e}, {y:e}) against ({:e}, {:e})",
                want.0,
                want.1
            );
        }
        assert!(special > 0, "no point is special; is the class exercised?");
    }

    /// Pairs with a coordinate at a multiple of 90 degrees, which the
    /// kernels do not cover, among others they do.
    fn right_angle_pairs() -> Vec<(f64, f64)> {
        let mut pairs = vec![(0.0, 90.0), (-0.0, 90.0), (10.0, 80.0)];
        for k in -8..=8 {
            let a = 90.0 * f64::from(k);
            pairs.extend([(a, 80.0), (a, 89.5), (a, -10.0), (10.0, a), (a, a)]);
            pairs.push((-170.0, 89.0 + f64::from(k) / 8.0));
        }
        pairs
    }

    /// `prj` with the latitude-axis parameters `p`, from `p[0]`.
    fn with_params(proj: WcsProjection, p: &[f64]) -> Prj {
        let pv: Vec<Option<f64>> = p.iter().map(|&v| Some(v)).collect();
        Prj::new(proj, &[pv, Vec::new()], true).unwrap()
    }

    /// The most by which the two paths' roundings of one operation can differ
    /// where its result is subnormal, 2^-1074: there each path rounds to
    /// within half the least subnormal of the exact result, whatever its
    /// relative error. Every bound written as a relative one adds this per
    /// operation.
    const SUBNORMAL_ROUNDING: f64 = f64::from_bits(1);

    /// How far ARC's and AIR's forward slice outputs may be from the
    /// per-value entry's, in ULP. Each output is a radius, computed alike on
    /// both paths (AIR's per point with [`Libm`]), times one kernel sine or
    /// cosine, which is within 2 ULP of libm's (see [`Kernel`]), at most
    /// 2 * 2^-52 relative. So the exact products differ by at most
    /// 2 * 2^-52 of the output, which is at most 4 ULP of it, and the two
    /// paths' roundings add at most 1 ULP between them: the bound is 5 ULP.
    const RADIUS_TIMES_KERNEL_SLICE_ULPS: u64 = 5;

    /// How far ZEA's forward slice outputs may be from the per-value entry's,
    /// in ULP. Each output is `w0 * sind((90 - theta) / 2)` times `sind(phi)`
    /// or `cosd(phi)`, two operations on two kernel values. Each kernel value
    /// is within 2 ULP of libm's (see [`Kernel`]), at most 2 * 2^-52 of it,
    /// so the exact results differ by at most 2 * 2 * 2^-52 relative; each
    /// path rounds its two operations once each, at most 2^-53 relative
    /// apiece, 2 * 2^-52 between the two paths. That is a relative
    /// difference of up to 6 * 2^-52, to first order. An ULP of the output is
    /// at least 2^-53 of it, so the bound is 12 ULP.
    const ZEA_FWD_SLICE_ULPS: u64 = 12;

    /// How far SZP's reverse slice's longitude may be from the per-value
    /// entry's, in ULP: the arguments of its arc tangent are computed alike
    /// on both paths, and [`Kernel`]'s `atan2d` is within 2 ULP of
    /// [`Libm`]'s. The latitude is [`Libm`]'s on both paths, bit for bit.
    const SZP_REV_PHI_ULPS: u64 = 2;

    /// ARC's and AIR's forward slices run the kernels and are within
    /// [`RADIUS_TIMES_KERNEL_SLICE_ULPS`] of the per-value entry, ZEA's
    /// within [`ZEA_FWD_SLICE_ULPS`], with the same bad points; their reverse
    /// slices run [`Libm`] and are bit for bit the per-value entry.
    #[test]
    fn arc_zea_and_air_slices_are_within_kernel_drift_of_the_per_value_body() {
        use WcsProjection::{Air, Arc, Zea};
        let fwd_inputs = tan_fwd_inputs();
        let rev_inputs = zpn_rev_inputs();
        for (p, bound) in [
            (prj(Arc), RADIUS_TIMES_KERNEL_SLICE_ULPS),
            (prj(Zea), ZEA_FWD_SLICE_ULPS),
            (prj(Air), RADIUS_TIMES_KERNEL_SLICE_ULPS),
            (
                with_params(Air, &[0.0, 45.0]),
                RADIUS_TIMES_KERNEL_SLICE_ULPS,
            ),
        ] {
            let code = p.proj.code();
            let (fwd_max, _, fwd_differ) = slice_drift(&p, &fwd_inputs, true);
            assert!(
                fwd_max.iter().all(|&m| m <= bound),
                "{code} forward: {fwd_max:?} ULP"
            );
            assert!(fwd_differ > 0, "{code} forward: no output differs");
            let (rev_max, _, rev_differ) = slice_drift(&p, &rev_inputs, false);
            assert_eq!((rev_max, rev_differ), ([0, 0], 0), "{code} reverse");
        }
    }

    /// The bound on the drift of STG's forward slice outputs at (`phi`,
    /// `theta`), whose per-value outputs are (`x`, `y`). Each output is
    /// `w0 * cosd(theta) / s` times `sind(phi)` or `cosd(phi)`, with `s = 1 +
    /// sind(theta)`. With each kernel sine and cosine within 2 ULP, at most
    /// 2 eps relative (eps = 2^-52), of Libm's (see [`Kernel`]), and each
    /// operation rounded once on each path, eps between the paths: `s`
    /// differs by at most `D = eps (2 |sind(theta)| + |s|)`, which is
    /// relative `D / (|s| - D)` of the kernels' `s`; the two other kernel
    /// values add 2 eps each and the three operations eps each. The bound is
    /// `(7 eps + D / (|s| - D)) |out|`, to first order, plus
    /// [`SUBNORMAL_ROUNDING`] for each operation.
    fn stg_fwd_bound(_phi: f64, theta: f64, x: f64, y: f64) -> [f64; 2] {
        let eps = f64::EPSILON;
        let sthe = Libm::sind(theta);
        let s = 1.0 + sthe;
        let d = eps * (2.0 * sthe.abs() + s.abs());
        let rel = 7.0 * eps + d / (s.abs() - d);
        [
            rel * x.abs() + 3.0 * SUBNORMAL_ROUNDING,
            rel * y.abs() + 3.0 * SUBNORMAL_ROUNDING,
        ]
    }

    /// STG's forward slice runs the kernels and is within [`stg_fwd_bound`]
    /// of the per-value entry, with the same bad points, among them points
    /// within 1e-4 degrees of the pole of the projection, where the sine of
    /// the latitude rounds to -1; its reverse slice runs [`Libm`] and is bit
    /// for bit the per-value entry.
    #[test]
    fn stg_slices_are_within_kernel_drift_of_the_per_value_body() {
        let p = prj(WcsProjection::Stg);
        let mut fwd_inputs = tan_fwd_inputs();
        let d = degrees(40_000);
        for (i, &u) in d.iter().enumerate() {
            let t = (i % 1001) as f64 / 1000.0;
            fwd_inputs.push((u, -90.0 + crate::math::powf(10.0, -12.0 + 8.0 * t)));
        }
        let (fwd_ulps, _, fwd_differ) = slice_drift(&p, &fwd_inputs, true);
        let over = drift_over_bound(&p, &fwd_inputs, true, stg_fwd_bound);
        assert!(
            over <= 1.0,
            "forward: {over} of the bound ({fwd_ulps:?} ULP)"
        );
        assert!(fwd_differ > 0, "forward: no output differs");
        let (rev_max, _, rev_differ) = slice_drift(&p, &zpn_rev_inputs(), false);
        assert_eq!((rev_max, rev_differ), ([0, 0], 0), "reverse");
    }

    /// The bound on the drift of AZP's forward slice outputs at (`phi`,
    /// `theta`), whose per-value outputs are (`x`, `y`). The denominator `t`
    /// differs by at most `D = eps (2 |sthe| + |p1 + sthe| + 6 |cthe s| +
    /// |t|)` (eps = 2^-52), as `azp_fwd_t_near_zero` derives, which is
    /// relative `D / (|t| - D)` of the kernels' `t`. `x` is `w0 cthe / t`
    /// times `sphi`, two more kernel values and three operations, 7 eps;
    /// `y` is `-(w0 cthe / t) cphi w2`, two more kernel values and four
    /// operations, 8 eps. The bound is `(7 eps + D / (|t| - D)) |x|` and
    /// `(8 eps + D / (|t| - D)) |y|`, to first order, plus
    /// [`SUBNORMAL_ROUNDING`] for each operation.
    fn azp_fwd_bound(prm: &PrjPrm, phi: f64, theta: f64, x: f64, y: f64) -> [f64; 2] {
        let eps = f64::EPSILON;
        let sthe = Libm::sind(theta);
        let a = prm.p[1] + sthe;
        let b = Libm::cosd(theta) * (prm.w[1] * Libm::cosd(phi));
        let t = a + b;
        let d = eps * (2.0 * sthe.abs() + a.abs() + 6.0 * b.abs() + t.abs());
        let q = d / (t.abs() - d);
        [
            (7.0 * eps + q) * x.abs() + 3.0 * SUBNORMAL_ROUNDING,
            (8.0 * eps + q) * y.abs() + 4.0 * SUBNORMAL_ROUNDING,
        ]
    }

    /// The AZP parameter sets the tests use: the default; a perspective
    /// from beyond the sphere, tilted, without the divergence test; one from
    /// within it, tilted, with it; and an untilted one at the sphere's
    /// centre whose divergence limit is where the denominator is zero.
    fn azp_prjs() -> [Prj; 4] {
        use WcsProjection::Azp;
        [
            prj(Azp),
            with_params(Azp, &[0.0, 2.0, 30.0]),
            with_params(Azp, &[0.0, 0.5, 20.0]),
            with_params(Azp, &[0.0, 0.0, 30.0]),
        ]
    }

    /// AZP's forward slice runs the kernels and is within [`azp_fwd_bound`]
    /// of the per-value entry, with the same bad points; its reverse slice
    /// runs [`Libm`] and is bit for bit the per-value entry.
    #[test]
    fn azp_slices_are_within_kernel_drift_of_the_per_value_body() {
        let fwd_inputs = tan_fwd_inputs();
        let rev_inputs = zpn_rev_inputs();
        for p in azp_prjs() {
            let prm = p.prm.clone();
            let params = &prm.p[1..3];
            let (fwd_ulps, _, fwd_differ) = slice_drift(&p, &fwd_inputs, true);
            let over = drift_over_bound(&p, &fwd_inputs, true, |u, v, x, y| {
                azp_fwd_bound(&prm, u, v, x, y)
            });
            assert!(
                over <= 1.0,
                "{params:?} forward: {over} of the bound ({fwd_ulps:?} ULP)"
            );
            assert!(fwd_differ > 0, "{params:?} forward: no output differs");
            let (rev_max, _, rev_differ) = slice_drift(&p, &rev_inputs, false);
            assert_eq!((rev_max, rev_differ), ([0, 0], 0), "{params:?} reverse");
        }
    }

    /// Points at and a few ULP either side of AZP's divergence limit, as the
    /// per-value entry computes it, where its denominator is near zero for
    /// the untilted perspective from the centre: bad in the forward slice
    /// exactly when bad in the per-value entry, and within the bound.
    #[test]
    fn azp_limit_points_agree_on_bad_or_good() {
        for p in azp_prjs() {
            let prm = p.prm.clone();
            let divergence_test = prm.w[7] > 0.0;
            if !divergence_test {
                continue;
            }
            let mut pairs = Vec::new();
            let mut bad = 0;
            for &phi in &degrees(20_000) {
                let s = prm.w[1] * Libm::cosd(phi);
                let t = prm.p[1] / crate::math::sqrt(1.0 + s * s);
                let (s, t) = (Libm::atand(-s), Libm::asind(t));
                let (mut a, mut b) = (s - t, s + t + 180.0);
                if a > 90.0 {
                    a -= 360.0;
                }
                if b > 90.0 {
                    b -= 360.0;
                }
                let limit = if a > b { a } else { b };
                for n in -8i32..=8 {
                    let n64 = f64::from(n);
                    for theta in [
                        ulps_away(limit, i64::from(n)),
                        limit + n64 * crate::math::powf(2.0, -46.0),
                        limit + n64 * 1e-13,
                    ] {
                        bad += usize::from(p.fwd_raw(phi, theta).0 != Status::Ok);
                        pairs.push((phi, theta));
                    }
                }
            }
            assert!(bad > 0 && bad < pairs.len(), "{bad} of {} bad", pairs.len());
            slice_drift(&p, &pairs, true);
            let over = drift_over_bound(&p, &pairs, true, |u, v, x, y| {
                azp_fwd_bound(&prm, u, v, x, y)
            });
            assert!(over <= 1.0, "{:?}: {over} of the bound", &prm.p[1..3]);
        }
    }

    /// The bound on the drift of SZP's forward slice outputs at (`phi`,
    /// `theta`), whose per-value outputs are (`x`, `y`). `s = 1 - sthe`
    /// differs by at most `Ds = eps (2 |sthe| + |s|)` (eps = 2^-52) and the
    /// denominator `t = w3 - s` by at most `Dt = Ds + eps |t|`, as
    /// `szp_fwd_t_near_zero` derives. Each numerator, `w6 cthe sphi - w4 s`
    /// or `w6 cthe cphi + w5 s`, differs by at most
    /// `DN = 6 eps |A| + |w| Ds + eps |B| + eps |N|`, where `A` is its first
    /// term, two kernel values and two operations, `B` its second, `w` the
    /// parameter in it and `N` the numerator. The quotient then differs by
    /// at most `(DN + |out| Dt) / (|t| - Dt) + eps |out|`, to first order,
    /// plus [`SUBNORMAL_ROUNDING`] for each of its six operations.
    fn szp_fwd_bound(prm: &PrjPrm, phi: f64, theta: f64, x: f64, y: f64) -> [f64; 2] {
        let eps = f64::EPSILON;
        let sthe = Libm::sind(theta);
        let cthe = Libm::cosd(theta);
        let s = 1.0 - sthe;
        let ds = eps * (2.0 * sthe.abs() + s.abs());
        let t = prm.w[3] - s;
        let dt = ds + eps * t.abs();
        let t_min = t.abs() - dt;
        let mut out = [0.0; 2];
        for (k, (trig, w, sign, value)) in [
            (Libm::sind(phi), prm.w[4], -1.0, x),
            (Libm::cosd(phi), prm.w[5], 1.0, y),
        ]
        .into_iter()
        .enumerate()
        {
            let a = prm.w[6] * cthe * trig;
            let b = w * s;
            let n = a + sign * b;
            let dn = 6.0 * eps * a.abs() + w.abs() * ds + eps * b.abs() + eps * n.abs();
            out[k] = (dn + value.abs() * dt) / t_min + eps * value.abs() + 6.0 * SUBNORMAL_ROUNDING;
        }
        out
    }

    /// The SZP parameter sets the tests use: the default, a perspective from
    /// beyond the sphere with the overlap test, and one from within it whose
    /// divergence limit is where the denominator is zero.
    fn szp_prjs() -> [Prj; 3] {
        use WcsProjection::Szp;
        [
            prj(Szp),
            with_params(Szp, &[0.0, 2.0, 30.0, 60.0]),
            with_params(Szp, &[0.0, 0.5, 0.0, -30.0]),
        ]
    }

    /// SZP's forward slice runs the kernels and is within [`szp_fwd_bound`]
    /// of the per-value entry; its reverse slice runs the kernels' arc
    /// tangent, so its longitude is within [`SZP_REV_PHI_ULPS`] and its
    /// latitude bit for bit the per-value entry's; both with the same bad
    /// points.
    #[test]
    fn szp_slices_are_within_kernel_drift_of_the_per_value_body() {
        let fwd_inputs = tan_fwd_inputs();
        let rev_inputs = tan_rev_inputs();
        for p in szp_prjs() {
            let prm = p.prm.clone();
            let params = &prm.p[1..4];
            let (fwd_ulps, _, fwd_differ) = slice_drift(&p, &fwd_inputs, true);
            let over = drift_over_bound(&p, &fwd_inputs, true, |u, v, x, y| {
                szp_fwd_bound(&prm, u, v, x, y)
            });
            assert!(
                over <= 1.0,
                "{params:?} forward: {over} of the bound ({fwd_ulps:?} ULP)"
            );
            assert!(fwd_differ > 0, "{params:?} forward: no output differs");
            let ([phi_max, theta_max], _, rev_differ) = slice_drift(&p, &rev_inputs, false);
            assert!(
                phi_max <= SZP_REV_PHI_ULPS && theta_max == 0,
                "{params:?} reverse: {phi_max} ULP in phi, {theta_max} in theta"
            );
            assert!(rev_differ > 0, "{params:?} reverse: no output differs");
        }
    }

    /// Points at and a few ULP either side of SZP's divergence limit, where
    /// its denominator is near zero for the perspective from within the
    /// sphere: bad in the forward slice exactly when bad in the per-value
    /// entry, and within the bound.
    #[test]
    fn szp_limit_points_agree_on_bad_or_good() {
        let p = szp_prjs()[2].clone();
        let prm = p.prm.clone();
        let limit = prm.w[8];
        let mut pairs = Vec::new();
        let mut bad = 0;
        for &phi in &degrees(20_000) {
            for n in -8i32..=8 {
                let n64 = f64::from(n);
                for theta in [
                    ulps_away(limit, i64::from(n)),
                    limit + n64 * crate::math::powf(2.0, -46.0),
                    limit + n64 * 1e-13,
                ] {
                    bad += usize::from(p.fwd_raw(phi, theta).0 != Status::Ok);
                    pairs.push((phi, theta));
                }
            }
        }
        assert!(bad > 0 && bad < pairs.len(), "{bad} of {} bad", pairs.len());
        slice_drift(&p, &pairs, true);
        let over = drift_over_bound(&p, &pairs, true, |u, v, x, y| {
            szp_fwd_bound(&prm, u, v, x, y)
        });
        assert!(over <= 1.0, "{over} of the bound");
    }

    /// The double `n` places from `v` in order, crossing zero from one sign
    /// to the other as the doubles do (with -0 and +0 one place).
    fn step_ulps(v: f64, n: i64) -> f64 {
        let magnitude = (v.to_bits() & !(1 << 63)) as i64;
        let o = if v.is_sign_negative() {
            -magnitude
        } else {
            magnitude
        } + n;
        let magnitude = f64::from_bits(o.unsigned_abs());
        if o < 0 { -magnitude } else { magnitude }
    }

    /// The kernel values within 2 ULP of `v`, the premise of every margin
    /// (see [`Kernel`]).
    fn within_two_ulps(v: f64) -> impl Iterator<Item = f64> {
        (-2i64..=2).map(move |n| step_ulps(v, n))
    }

    /// Wherever a sine within 2 ULP of [`Libm`]'s could put STG's `s` on the
    /// other side of zero from [`Libm`]'s, `stg_fwd_s_near_zero` redoes the
    /// point; and a margin of zero would not.
    #[test]
    fn stg_margin_covers_the_kernels_drift_and_a_zero_margin_does_not() {
        let mut missed_by_zero_margin = 0;
        for k in 0..16 {
            let libm = -1.0 + f64::from(k) * crate::math::powf(2.0, -53.0);
            for kernel in within_two_ulps(libm) {
                let (s_libm, s_kernel) = (1.0 + libm, 1.0 + kernel);
                if (s_libm == 0.0) != (s_kernel == 0.0) {
                    assert!(stg_fwd_s_near_zero(s_kernel), "{libm:e}, {kernel:e}");
                    missed_by_zero_margin += usize::from(s_kernel > 0.0);
                }
            }
        }
        assert!(missed_by_zero_margin > 0);
    }

    /// Wherever sines and cosines within 2 ULP of [`Libm`]'s could put AZP's
    /// denominator on the other side of zero from [`Libm`]'s,
    /// `azp_fwd_t_near_zero` redoes the point; and a margin of zero would
    /// not. The values are chosen so that [`Libm`]'s denominator is exactly
    /// zero, or random.
    #[test]
    fn azp_margin_covers_the_kernels_drift_and_a_zero_margin_does_not() {
        let mut prm = prj(WcsProjection::Azp).prm.clone();
        prm.p[1] = 0.5;
        prm.w[1] = 0.5;
        let t_of = |sthe: f64, cthe: f64, cphi: f64| (prm.p[1] + sthe) + cthe * (prm.w[1] * cphi);
        let mut values = vec![(-0.25, 0.5, -1.0), (-0.75, 0.5, 1.0), (-0.5, 0.75, 0.0)];
        let mut rng = crate::test_inputs::SplitMix64::new(0x5eed_a2d0);
        for _ in 0..2000 {
            let mut r = || 2.0 * rng.next_f64() - 1.0;
            values.push((r(), r(), r()));
        }
        let mut missed_by_zero_margin = 0;
        for (sthe, cthe, cphi) in values {
            let t_libm = t_of(sthe, cthe, cphi);
            for s in within_two_ulps(sthe) {
                for c in within_two_ulps(cthe) {
                    for cp in within_two_ulps(cphi) {
                        let t_kernel = t_of(s, c, cp);
                        if (t_libm == 0.0) != (t_kernel == 0.0) {
                            assert!(azp_fwd_t_near_zero(&prm, s, c, cp), "{s:e}, {c:e}, {cp:e}");
                            missed_by_zero_margin += usize::from(t_kernel != 0.0);
                        }
                    }
                }
            }
        }
        assert!(missed_by_zero_margin > 0);
    }

    /// Wherever a sine within 2 ULP of [`Libm`]'s could put SZP's
    /// denominator on the other side of zero from [`Libm`]'s,
    /// `szp_fwd_t_near_zero` redoes the point; and a margin of zero would
    /// not. The sines are those at which [`Libm`]'s denominator is zero or
    /// next to it, for the default parameters, where it is the sine itself
    /// rounded through `1 - sthe`, and for a perspective from within the
    /// sphere.
    #[test]
    fn szp_margin_covers_the_kernels_drift_and_a_zero_margin_does_not() {
        let [default, _, inside] = szp_prjs();
        let mut missed_by_zero_margin = 0;
        for p in [default, inside] {
            let prm = &p.prm;
            let zero_at = 1.0 - prm.w[3];
            let mut sines = Vec::new();
            for base in [
                zero_at,
                crate::math::powf(2.0, -54.0),
                crate::math::powf(2.0, -53.0),
            ] {
                sines.extend((-6i64..=6).map(|n| step_ulps(base, n)));
            }
            for libm in sines {
                let t_libm = prm.w[3] - (1.0 - libm);
                for kernel in within_two_ulps(libm) {
                    let t_kernel = prm.w[3] - (1.0 - kernel);
                    if (t_libm == 0.0) != (t_kernel == 0.0) {
                        assert!(szp_fwd_t_near_zero(prm, kernel), "{libm:e}, {kernel:e}");
                        missed_by_zero_margin += usize::from(t_kernel != 0.0);
                    }
                }
            }
        }
        assert!(missed_by_zero_margin > 0);
    }

    /// The zenithal projections whose forward slices run the kernels, with
    /// their parameter sets, and SZP, whose reverse slice does too.
    fn zenithal_kernel_prjs() -> Vec<Prj> {
        use WcsProjection::{Air, Arc, Stg, Zea};
        let mut out = vec![
            prj(Arc),
            prj(Zea),
            prj(Stg),
            prj(Air),
            with_params(Air, &[0.0, 45.0]),
        ];
        out.extend(azp_prjs());
        out.extend(szp_prjs());
        out
    }

    #[test]
    fn zenithal_slices_give_each_point_the_same_bits_at_any_length() {
        for p in zenithal_kernel_prjs() {
            assert_same_bits_at_any_length(&p, &interleaved_inputs());
        }
    }

    #[test]
    fn zenithal_slice_builds_agree() {
        let inputs = interleaved_inputs();
        for p in zenithal_kernel_prjs() {
            match p.proj {
                WcsProjection::Azp => assert_builds_agree::<AzpFwd>(&p, &inputs),
                WcsProjection::Szp => {
                    assert_builds_agree::<SzpFwd>(&p, &inputs);
                    assert_builds_agree::<SzpRev>(&p, &inputs);
                }
                WcsProjection::Stg => assert_builds_agree::<StgFwd>(&p, &inputs),
                WcsProjection::Arc => assert_builds_agree::<ArcFwd>(&p, &inputs),
                WcsProjection::Zea => assert_builds_agree::<ZeaFwd>(&p, &inputs),
                WcsProjection::Air => assert_builds_agree::<AirFwd>(&p, &inputs),
                proj => unreachable!("{}", proj.code()),
            }
        }
    }

    /// The pole, and coordinates at multiples of 90 degrees, which the
    /// kernels do not cover, get the per-value entry's bits in a slice.
    #[test]
    fn zenithal_points_the_kernels_do_not_cover_are_exact_in_a_slice() {
        let sky = right_angle_pairs();
        let plane: Vec<(f64, f64)> = sky
            .iter()
            .flat_map(|&(u, v)| [(u, v), (0.0, v), (u, 0.0), (0.0, 0.0)])
            .collect();
        for p in zenithal_kernel_prjs() {
            match p.proj {
                WcsProjection::Azp => assert_special_points_exact::<AzpFwd>(&p, &sky, true),
                WcsProjection::Szp => {
                    assert_special_points_exact::<SzpFwd>(&p, &sky, true);
                    assert_special_points_exact::<SzpRev>(&p, &plane, false);
                }
                WcsProjection::Stg => assert_special_points_exact::<StgFwd>(&p, &sky, true),
                WcsProjection::Arc => assert_special_points_exact::<ArcFwd>(&p, &sky, true),
                WcsProjection::Zea => assert_special_points_exact::<ZeaFwd>(&p, &sky, true),
                WcsProjection::Air => assert_special_points_exact::<AirFwd>(&p, &sky, true),
                proj => unreachable!("{}", proj.code()),
            }
        }
    }
}
