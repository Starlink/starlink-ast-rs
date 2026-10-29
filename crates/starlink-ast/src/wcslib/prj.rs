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
//! for most projections. For a projection that opts in, they run its body
//! with the vectorizable [`Kernel`] trigonometry over every element, then
//! redo with [`Libm`] each element whose trigonometric arguments the
//! kernels do not cover, so each output is within the kernels' drift of the
//! per-value entry's and a point is bad in one exactly when it is bad in the
//! other.
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
//!   a test only is computed before the test);
//! - a `kernel_body!` line declaring the pair, and an arm in that slice
//!   entry's `match` that passes it to `kernel_slice`; `fwd_slice` has the
//!   `match`, and the first reverse opt-in adds one to `rev_slice` in the
//!   same form.
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
use super::tpn::{tpn_fwd, tpn_rev, tpn_set};
use super::trig::{Kernel, Libm, Trig};
use super::zenithal::{
    air_fwd, air_rev, air_set, arc_fwd, arc_rev, arc_set, azp_fwd, azp_rev, azp_set, sin_fwd,
    sin_rev, sin_set, stg_fwd, stg_rev, stg_set, szp_fwd, szp_rev, szp_set, tan_fwd,
    tan_fwd_needs_special, tan_rev, tan_set, zea_fwd, zea_rev, zea_set, zpn_fwd, zpn_rev, zpn_set,
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

    /// [`fwd_raw`](Prj::fwd_raw) in place for every pair (`a[i]`, `b[i]`):
    /// native (phi, theta) in degrees on entry, (x, y) on return.
    ///
    /// A pair holding [`BAD_VALUE`] or NaN, and a point the projection cannot
    /// project, give [`BAD_VALUE`] in both.
    /// Each pair projected counts one call of `wcs_fwd`.
    ///
    /// # Errors
    ///
    /// `ErrorCode::Wcspa` if the projection parameters are unusable for a
    /// pair that is not bad; the slices then hold partial results, whose
    /// contents are unspecified.
    pub(crate) fn fwd_slice(&self, a: &mut [f64], b: &mut [f64]) -> AstResult<()> {
        if self.setup != Status::Ok {
            // A set-up fails only for unusable parameters; any other status
            // here would turn every good point bad without an error.
            debug_assert_eq!(self.setup, Status::BadParams);
            let setup = self.setup;
            return slice_apply(a, b, "wcs_fwd", |_, _| (setup, 0.0, 0.0));
        }
        let prm = &self.prm;
        match self.proj {
            WcsProjection::Tan => kernel_slice::<TanFwd>(prm, a, b, "wcs_fwd"),
            proj => with_fwd!(proj, |f| slice_apply(a, b, "wcs_fwd", |phi, theta| f(
                prm, phi, theta
            ))),
        }
    }

    /// [`rev_raw`](Prj::rev_raw) in place for every pair (`a[i]`, `b[i]`):
    /// (x, y) on entry, native (phi, theta) in degrees on return; bad pairs,
    /// errors and counting (of `wcs_rev`) as for
    /// [`fwd_slice`](Prj::fwd_slice).
    ///
    /// # Errors
    ///
    /// `ErrorCode::Wcspa` if the projection parameters are unusable for a
    /// pair that is not bad; the slices then hold partial results, whose
    /// contents are unspecified.
    pub(crate) fn rev_slice(&self, a: &mut [f64], b: &mut [f64]) -> AstResult<()> {
        if self.setup != Status::Ok {
            // A set-up fails only for unusable parameters; any other status
            // here would turn every good point bad without an error.
            debug_assert_eq!(self.setup, Status::BadParams);
            let setup = self.setup;
            return slice_apply(a, b, "wcs_rev", |_, _| (setup, 0.0, 0.0));
        }
        let prm = &self.prm;
        with_rev!(self.proj, |f| slice_apply(a, b, "wcs_rev", |x, y| f(
            prm, x, y
        )))
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
/// `b[i]`) that holds neither [`BAD_VALUE`] nor NaN, writing [`BAD_VALUE`]
/// to both for those pairs and for a point the call cannot project, and
/// counting one call of `counter` per pair projected.
#[inline(always)]
fn slice_apply(
    a: &mut [f64],
    b: &mut [f64],
    counter: &'static str,
    project: impl Fn(f64, f64) -> (Status, f64, f64),
) -> AstResult<()> {
    let n = a.len();
    assert_eq!(b.len(), n, "the two slices must have the same length");
    let b = &mut b[..n];
    let mut calls = 0u64;
    let mut result = Ok(());
    for i in 0..n {
        let (u, v) = (a[i], b[i]);
        if u == BAD_VALUE || v == BAD_VALUE || u.is_nan() || v.is_nan() {
            a[i] = BAD_VALUE;
            b[i] = BAD_VALUE;
            continue;
        }
        calls += 1;
        match project(u, v) {
            (Status::Ok, p, q) => {
                a[i] = p;
                b[i] = q;
            }
            (Status::BadPoint, ..) => {
                a[i] = BAD_VALUE;
                b[i] = BAD_VALUE;
            }
            (Status::BadParams, ..) => {
                result = Err(bad_params());
                break;
            }
        }
    }
    crate::call_counts::hit_n(counter, calls);
    result
}

/// A projection body in one direction that a slice entry runs through the
/// vectorizable kernels.
///
/// `eval` is the body, generic over the trigonometry. `needs_special` is
/// true where an argument of a trigonometric function the body would call
/// lies outside that function's kernel cover, and where a status decision
/// or a branch of the body depends on a kernel-derived value that the
/// kernels' drift could carry across its threshold; the element is then
/// redone with [`Libm`], so the kernels never change whether a point is bad
/// or which branch runs. Both run in the vectorized loop, so neither may
/// branch; the module documentation says how a projection meets that.
trait KernelBody {
    /// The projection body.
    fn eval<T: Trig>(prm: &PrjPrm, u: f64, v: f64) -> (Status, f64, f64);
    /// Whether the body at (`u`, `v`) needs [`Libm`].
    fn needs_special(prm: &PrjPrm, u: f64, v: f64) -> bool;
}

/// Declare the [`KernelBody`] `$name` for the body `$body` and its predicate
/// `$special`.
macro_rules! kernel_body {
    ($name:ident, $body:ident, $special:ident) => {
        struct $name;
        impl KernelBody for $name {
            #[inline(always)]
            fn eval<T: Trig>(prm: &PrjPrm, u: f64, v: f64) -> (Status, f64, f64) {
                $body::<T>(prm, u, v)
            }
            #[inline(always)]
            fn needs_special(prm: &PrjPrm, u: f64, v: f64) -> bool {
                $special(prm, u, v)
            }
        }
    };
}

kernel_body!(TanFwd, tan_fwd, tan_fwd_needs_special);

/// How many elements [`kernel_slice_body`] evaluates with the kernels
/// before fixing them up, so the kernels' outputs stay in the first-level
/// cache and on the stack.
const KERNEL_BLOCK: usize = 32;

/// [`slice_apply`] for the body `K` run through the vectorizable kernels:
/// the same bad pairs, errors and counting, with each output within the
/// kernels' drift of [`Libm`]'s. Each element gets the same bits whatever the
/// slice's length, and whichever build of the loop runs: on x86-64 one for
/// AVX2 and FMA when the CPU has them.
fn kernel_slice<K: KernelBody>(
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
/// scattered back, redone with [`Libm`] where `K::needs_special` says, and
/// its status mapped as [`slice_apply`] does.
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
    'blocks: for (a, b) in a.chunks_mut(KERNEL_BLOCK).zip(b.chunks_mut(KERNEL_BLOCK)) {
        let b = &mut b[..a.len()];
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
            special[j] = K::needs_special(prm, u[j], v[j]);
        }
        for j in 0..good {
            calls += 1;
            let out = if special[j] {
                K::eval::<Libm>(prm, u[j], v[j])
            } else {
                (status[j], p[j], q[j])
            };
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
    use crate::wcslib::trig::R2D;

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
            p.fwd_slice(&mut a, &mut b).unwrap();
        } else {
            p.rev_slice(&mut a, &mut b).unwrap();
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

    /// The largest ULP distance of each output of a slice entry from the
    /// per-value entry where it projects the point, and how many outputs
    /// differ; a bad point must be bad in both.
    fn slice_drift(p: &Prj, pairs: &[(f64, f64)], fwd: bool) -> ([u64; 2], usize) {
        let got = through_slice(p, pairs, fwd);
        let mut max = [0u64; 2];
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
            }
        }
        (max, differ)
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
        let (fwd_max, fwd_differ) = slice_drift(&p, &tan_fwd_inputs(), true);
        assert!(
            fwd_max.iter().all(|&m| m <= TAN_FWD_SLICE_ULPS),
            "forward: {fwd_max:?} ULP"
        );
        assert!(
            fwd_differ > 0,
            "forward: no output differs; is the kernel in use?"
        );
        let (rev_max, rev_differ) = slice_drift(&p, &tan_rev_inputs(), false);
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

    #[test]
    fn tan_slices_give_each_point_the_same_bits_at_any_length() {
        let p = prj(WcsProjection::Tan);
        let pairs = interleaved_inputs();
        for fwd in [true, false] {
            let whole = through_slice(&p, &pairs, fwd);
            for len in [1, 2, 3, 7, 1001] {
                for (start, chunk) in pairs.chunks(len).enumerate() {
                    let got = through_slice(&p, chunk, fwd);
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

    /// Both builds of the kernel loop, with and without fused multiply-adds,
    /// give the same bits.
    #[test]
    fn tan_slice_builds_agree() {
        let p = prj(WcsProjection::Tan);
        let pairs = interleaved_inputs();
        let run = |fma: bool| {
            let mut a: Vec<f64> = pairs.iter().map(|q| q.0).collect();
            let mut b: Vec<f64> = pairs.iter().map(|q| q.1).collect();
            let prm = &p.prm;
            if fma {
                kernel_slice_body::<TanFwd, true>(prm, &mut a, &mut b, "wcs_fwd").unwrap();
            } else {
                kernel_slice_body::<TanFwd, false>(prm, &mut a, &mut b, "wcs_fwd").unwrap();
            }
            (a, b)
        };
        let (a0, b0) = run(false);
        let (a1, b1) = run(true);
        for i in 0..a0.len() {
            assert!(
                same_bits_or_both_nan(a0[i], a1[i]) && same_bits_or_both_nan(b0[i], b1[i]),
                "{:?}",
                pairs[i]
            );
        }
    }

    #[test]
    fn tan_exact_points_stay_exact_in_a_slice() {
        let p = prj(WcsProjection::Tan);
        let mut phi = [10.0, 20.0, 30.0, 0.0, 40.0, 50.0, 60.0];
        let mut theta = [80.0, 70.0, 60.0, 90.0, 50.0, 40.0, 30.0];
        p.fwd_slice(&mut phi, &mut theta).unwrap();
        // Exactly zero, with the signs the per-value entry gives.
        let (_, wx, wy) = p.fwd_raw(0.0, 90.0);
        assert_eq!((phi[3], theta[3]), (0.0, 0.0));
        assert_eq!(
            (phi[3].to_bits(), theta[3].to_bits()),
            (wx.to_bits(), wy.to_bits())
        );
        let mut x = [1.0, 2.0, 3.0, 0.0, 4.0, 5.0, 6.0];
        let mut y = [-1.0, -2.0, -3.0, 0.0, -4.0, -5.0, -6.0];
        p.rev_slice(&mut x, &mut y).unwrap();
        assert_eq!(
            (x[3].to_bits(), y[3].to_bits()),
            (0.0f64.to_bits(), 90.0f64.to_bits())
        );
    }
}
