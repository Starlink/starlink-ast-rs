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

// The projections are used by `WcsMap` in a later step; until then only the
// tests use them.
#![allow(dead_code)]

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};

use super::cylindrical::{
    car_fwd, car_rev, car_set, cea_fwd, cea_rev, cea_set, cyp_fwd, cyp_rev, cyp_set, mer_fwd,
    mer_rev, mer_set,
};
use super::pseudocyl::{
    ait_fwd, ait_rev, ait_set, mol_fwd, mol_rev, mol_set, par_fwd, par_rev, par_set, sfl_fwd,
    sfl_rev, sfl_set,
};
use super::tpn::{tpn_fwd, tpn_rev, tpn_set};
use super::trig::Libm;
use super::zenithal::{
    air_fwd, air_rev, air_set, arc_fwd, arc_rev, arc_set, azp_fwd, azp_rev, azp_set, sin_fwd,
    sin_rev, sin_set, stg_fwd, stg_rev, stg_set, szp_fwd, szp_rev, szp_set, tan_fwd, tan_rev,
    tan_set, zea_fwd, zea_rev, zea_set, zpn_fwd, zpn_rev, zpn_set,
};
use super::{PRJ_PARAM_SLOTS, WcsProjection};

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

    /// The projection type.
    pub(crate) fn proj(&self) -> WcsProjection {
        self.proj
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

    /// The status of the set-up.
    pub(crate) fn setup_status(&self) -> Status {
        self.setup
    }

    /// Forward projection of native (`phi`, `theta`) in degrees: the status
    /// and the projected (x, y).
    /// The outputs are meaningful only when the status is [`Status::Ok`].
    #[inline]
    pub(crate) fn fwd_raw(&self, phi: f64, theta: f64) -> (Status, f64, f64) {
        if self.setup != Status::Ok {
            return (self.setup, 0.0, 0.0);
        }
        let prm = &self.prm;
        match self.proj {
            WcsProjection::Azp => azp_fwd::<Libm>(prm, phi, theta),
            WcsProjection::Szp => szp_fwd::<Libm>(prm, phi, theta),
            WcsProjection::Tan => tan_fwd::<Libm>(prm, phi, theta),
            WcsProjection::Stg => stg_fwd::<Libm>(prm, phi, theta),
            WcsProjection::Sin => sin_fwd::<Libm>(prm, phi, theta),
            WcsProjection::Arc => arc_fwd::<Libm>(prm, phi, theta),
            WcsProjection::Zpn => zpn_fwd::<Libm>(prm, phi, theta),
            WcsProjection::Zea => zea_fwd::<Libm>(prm, phi, theta),
            WcsProjection::Air => air_fwd::<Libm>(prm, phi, theta),
            WcsProjection::Tpn => tpn_fwd::<Libm>(prm, phi, theta),
            WcsProjection::Cyp => cyp_fwd::<Libm>(prm, phi, theta),
            WcsProjection::Cea => cea_fwd::<Libm>(prm, phi, theta),
            WcsProjection::Car => car_fwd::<Libm>(prm, phi, theta),
            WcsProjection::Mer => mer_fwd::<Libm>(prm, phi, theta),
            WcsProjection::Sfl => sfl_fwd::<Libm>(prm, phi, theta),
            WcsProjection::Par => par_fwd::<Libm>(prm, phi, theta),
            WcsProjection::Mol => mol_fwd::<Libm>(prm, phi, theta),
            WcsProjection::Ait => ait_fwd::<Libm>(prm, phi, theta),
            other => todo!("{} is ported in a later step", other.code()),
        }
    }

    /// Reverse projection of (`x`, `y`): the status and the native
    /// (phi, theta) in degrees.
    /// The outputs are meaningful only when the status is [`Status::Ok`].
    #[inline]
    pub(crate) fn rev_raw(&self, x: f64, y: f64) -> (Status, f64, f64) {
        if self.setup != Status::Ok {
            return (self.setup, 0.0, 0.0);
        }
        let prm = &self.prm;
        match self.proj {
            WcsProjection::Azp => azp_rev::<Libm>(prm, x, y),
            WcsProjection::Szp => szp_rev::<Libm>(prm, x, y),
            WcsProjection::Tan => tan_rev::<Libm>(prm, x, y),
            WcsProjection::Stg => stg_rev::<Libm>(prm, x, y),
            WcsProjection::Sin => sin_rev::<Libm>(prm, x, y),
            WcsProjection::Arc => arc_rev::<Libm>(prm, x, y),
            WcsProjection::Zpn => zpn_rev::<Libm>(prm, x, y),
            WcsProjection::Zea => zea_rev::<Libm>(prm, x, y),
            WcsProjection::Air => air_rev::<Libm>(prm, x, y),
            WcsProjection::Tpn => tpn_rev::<Libm>(prm, x, y),
            WcsProjection::Cyp => cyp_rev::<Libm>(prm, x, y),
            WcsProjection::Cea => cea_rev::<Libm>(prm, x, y),
            WcsProjection::Car => car_rev::<Libm>(prm, x, y),
            WcsProjection::Mer => mer_rev::<Libm>(prm, x, y),
            WcsProjection::Sfl => sfl_rev::<Libm>(prm, x, y),
            WcsProjection::Par => par_rev::<Libm>(prm, x, y),
            WcsProjection::Mol => mol_rev::<Libm>(prm, x, y),
            WcsProjection::Ait => ait_rev::<Libm>(prm, x, y),
            other => todo!("{} is ported in a later step", other.code()),
        }
    }
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
        other => todo!("{} is ported in a later step", other.code()),
    }
}

/// Fill the parameter arrays from the supplied parameters, giving each
/// parameter that was not supplied its projection's default or zero.
///
/// `InitPrjPrm` in `reference/starlink-ast` at 3d3f255f
/// (`src/wcsmap.c`); the same as `PrjParams::init_prj_prm`.
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
