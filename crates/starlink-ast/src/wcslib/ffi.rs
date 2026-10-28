// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The raw `AstPrjPrm` C struct and its safe [`PrjParams`] wrapper, through
//! which the vendored wcslib projection functions are called.

use std::ffi::c_int;

use std::f64::consts::FRAC_PI_2;

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};

use super::{WCSLIB_MXPAR, WcsProjection};

/// Number of slots in a projection-parameter array.
///
/// C allocates `mxpar + 1` doubles per axis (wcsmap.c:5862-5863) so that
/// index `mxpar` itself is addressable; `astGetPVMax` reports that allocation
/// minus one (wcsmap.c:2205-2212). Every array here is sized for the largest
/// projection, so the same indices are legal.
const PRJ_PARAM_SLOTS: usize = WCSLIB_MXPAR + 1;

/// FFI representation of the C `struct AstPrjPrm` from `vendor/wcslib/proj.h`.
///
/// The `p` and `p2` pointers point into owned `Box<[f64; PRJ_PARAM_SLOTS]>`
/// storage managed by [`PrjParams`].  The `fwd` and `rev` function
/// pointers come from C's `PrjInfo` table (see `WcsProjection::prj_fns`).
#[repr(C)]
struct AstPrjPrm {
    code: [u8; 4],
    flag: c_int,
    phi0: f64,
    theta0: f64,
    r0: f64,
    p: *mut f64,
    p2: *mut f64,
    w: [f64; 20],
    n: c_int,
    fwd: Option<PrjFn>,
    rev: Option<PrjFn>,
}

/// A wcslib projection function: forward or reverse, as listed in C's
/// `PrjInfo` table (wcsmap.c:703-737).
type PrjFn = unsafe extern "C" fn(f64, f64, *mut AstPrjPrm, *mut f64, *mut f64) -> c_int;

unsafe extern "C" {
    fn astAZPfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astAZPrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astSZPfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astSZPrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astTANfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astTANrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astSTGfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astSTGrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astSINfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astSINrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astARCfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astARCrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astZPNfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astZPNrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astZEAfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astZEArev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astAIRfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astAIRrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astCYPfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astCYPrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astCEAfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astCEArev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astCARfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astCARrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astMERfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astMERrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astSFLfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astSFLrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astPARfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astPARrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astMOLfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astMOLrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astAITfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astAITrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astCOPfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astCOPrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astCOEfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astCOErev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astCODfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astCODrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astCOOfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astCOOrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astBONfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astBONrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astPCOfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astPCOrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astTSCfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astTSCrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astCSCfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astCSCrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astQSCfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astQSCrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astHPXfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astHPXrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astXPHfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astXPHrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
    fn astTPNfwd(phi: f64, theta: f64, prj: *mut AstPrjPrm, x: *mut f64, y: *mut f64) -> c_int;
    fn astTPNrev(x: f64, y: f64, prj: *mut AstPrjPrm, phi: *mut f64, theta: *mut f64) -> c_int;
}

impl WcsProjection {
    /// The `WcsFwd` and `WcsRev` columns of C's `PrjInfo` table
    /// (wcsmap.c:703-737): the wcslib functions that evaluate this
    /// projection. Each sets itself up on first use from the parameters in
    /// the `AstPrjPrm` it is handed, so nothing calls a set-up routine in
    /// advance. `None` for NCP, which C handles as SIN, and for the unknown
    /// projection.
    fn prj_fns(self) -> Option<(PrjFn, PrjFn)> {
        Some(match self {
            WcsProjection::Azp => (astAZPfwd, astAZPrev),
            WcsProjection::Szp => (astSZPfwd, astSZPrev),
            WcsProjection::Tan => (astTANfwd, astTANrev),
            WcsProjection::Stg => (astSTGfwd, astSTGrev),
            WcsProjection::Sin => (astSINfwd, astSINrev),
            WcsProjection::Arc => (astARCfwd, astARCrev),
            WcsProjection::Zpn => (astZPNfwd, astZPNrev),
            WcsProjection::Zea => (astZEAfwd, astZEArev),
            WcsProjection::Air => (astAIRfwd, astAIRrev),
            WcsProjection::Cyp => (astCYPfwd, astCYPrev),
            WcsProjection::Cea => (astCEAfwd, astCEArev),
            WcsProjection::Car => (astCARfwd, astCARrev),
            WcsProjection::Mer => (astMERfwd, astMERrev),
            WcsProjection::Sfl => (astSFLfwd, astSFLrev),
            WcsProjection::Par => (astPARfwd, astPARrev),
            WcsProjection::Mol => (astMOLfwd, astMOLrev),
            WcsProjection::Ait => (astAITfwd, astAITrev),
            WcsProjection::Cop => (astCOPfwd, astCOPrev),
            WcsProjection::Coe => (astCOEfwd, astCOErev),
            WcsProjection::Cod => (astCODfwd, astCODrev),
            WcsProjection::Coo => (astCOOfwd, astCOOrev),
            WcsProjection::Bon => (astBONfwd, astBONrev),
            WcsProjection::Pco => (astPCOfwd, astPCOrev),
            WcsProjection::Tsc => (astTSCfwd, astTSCrev),
            WcsProjection::Csc => (astCSCfwd, astCSCrev),
            WcsProjection::Qsc => (astQSCfwd, astQSCrev),
            WcsProjection::Hpx => (astHPXfwd, astHPXrev),
            WcsProjection::Xph => (astXPHfwd, astXPHrev),
            WcsProjection::Tpn => (astTPNfwd, astTPNrev),
            WcsProjection::Gls => (astSFLfwd, astSFLrev),
            WcsProjection::Ncp | WcsProjection::Unknown => return None,
        })
    }

    /// The `theta0` column of C's `PrjInfo` table (wcsmap.c:703-737): the
    /// native latitude of the fiducial point in radians, which C's
    /// `GetNatLat` returns. `BAD_VALUE` for the conics, whose fiducial
    /// latitude is a projection parameter.
    pub(crate) fn table_theta0(self) -> f64 {
        use WcsProjection::*;
        match self {
            Azp | Szp | Tan | Stg | Sin | Arc | Zpn | Zea | Air | Xph | Tpn => FRAC_PI_2,
            Cop | Coe | Cod | Coo => BAD_VALUE,
            Cyp | Cea | Car | Mer | Sfl | Par | Mol | Ait | Bon | Pco | Tsc | Csc | Qsc | Ncp
            | Gls | Hpx | Unknown => 0.0,
        }
    }

    /// The `tol` column of C's `PrjInfo` table (wcsmap.c:711-743): how far
    /// past a limit of the accepted range, in degrees, a native longitude or
    /// latitude from the reverse projection is set to the limit rather than
    /// rejected. These are wcslib's per-projection tolerances (its
    /// `prjbchk` calls).
    pub(crate) fn table_tol(self) -> f64 {
        use WcsProjection::*;
        match self {
            Azp | Tan | Stg | Sin | Arc | Zpn | Zea | Air | Cyp | Cea | Car | Mer | Ait | Cop
            | Coe | Cod | Coo | Tsc | Csc | Qsc | Tpn => 1.0e-13,
            Szp => 2.0e-13,
            Sfl | Par | Pco | Gls | Hpx | Xph => 1.0e-12,
            Mol | Bon => 1.0e-11,
            Ncp | Unknown => 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// PrjParams safe wrapper
// ---------------------------------------------------------------------------

/// Safe wrapper around the C `AstPrjPrm` struct.
///
/// Owns the backing storage for the `p` and `p2` parameter arrays,
/// ensuring the pointers inside the C struct remain valid for the
/// lifetime of the wrapper.
///
/// This type is not `Clone` because cloning would create dangling
/// internal pointers.
pub(crate) struct PrjParams {
    inner: AstPrjPrm,
    /// The projection type, retained so the wcslib setup routine can be
    /// re-run when a parameter is changed.
    proj: WcsProjection,
    /// Backing storage for `inner.p` (latitude-axis projection parameters).
    p_storage: Box<[f64; PRJ_PARAM_SLOTS]>,
    /// Backing storage for `inner.p2` (longitude-axis projection parameters, TPN only).
    p2_storage: Box<[f64; PRJ_PARAM_SLOTS]>,
}

// SAFETY: PrjParams is fully self-contained — the internal pointers
// reference only the co-owned Box allocations, and nothing is shared
// with other threads.
unsafe impl Send for PrjParams {}

impl PrjParams {
    /// Create a new projection parameter set for the given projection type.
    ///
    /// The C struct is zeroed, the parameter storage is allocated and
    /// pointed to, and the projection's functions are installed from C's
    /// `PrjInfo` table.
    ///
    /// NCP and GLS are deprecated projections with no wcslib functions of
    /// their own.  They must be mapped to SIN
    /// and SFL (with appropriate parameters) at the `WcsMap` level before
    /// calling this constructor.
    pub(crate) fn new(proj: WcsProjection) -> AstResult<Self> {
        // NCP and GLS have no wcslib functions of their own — they must be
        // converted to SIN/SFL with parameters at a higher level.
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

        let mut p_storage = Box::new([0.0f64; PRJ_PARAM_SLOTS]);
        let mut p2_storage = Box::new([0.0f64; PRJ_PARAM_SLOTS]);

        // Build the null-terminated 3-letter code.
        let code_str = proj.code().as_bytes();
        let mut code = [0u8; 4];
        code[..code_str.len()].copy_from_slice(code_str);
        // code[3] is already 0 (null terminator)

        let mut inner = AstPrjPrm {
            code,
            flag: 0,
            phi0: 0.0,
            theta0: 0.0,
            r0: 0.0,
            p: p_storage.as_mut_ptr(),
            p2: p2_storage.as_mut_ptr(),
            w: [0.0; 20],
            n: 0,
            fwd: None,
            rev: None,
        };

        // Install the projection's functions from C's table. As in C, nothing
        // is set up in advance: `flag` is clear, so the first call runs the
        // projection's own set-up from the parameters then present, and a
        // set-up failure is that call's status.
        let (fwd, rev) = proj.prj_fns().ok_or_else(|| {
            AstError::new(
                ErrorCode::Wcsty,
                format!("Projection {} has no wcslib functions", proj.code()),
            )
        })?;
        inner.fwd = Some(fwd);
        inner.rev = Some(rev);

        Ok(Self {
            inner,
            proj,
            p_storage,
            p2_storage,
        })
    }

    /// Initialize the projection parameter arrays from the values supplied
    /// on the owning `WcsMap`, assigning defaults for any parameter that was
    /// not supplied.
    ///
    /// Port of C `InitPrjPrm` (wcsmap.c:2219). `params` is indexed by
    /// projection role: `params[0]` holds the latitude-axis parameters
    /// (wcslib `p`) and `params[1]` the longitude-axis parameters (wcslib
    /// `p2`, used by the TPN projection). A `None` entry means the parameter
    /// was not supplied — C stores `AST__BAD` in that slot and later replaces
    /// it with a per-projection default (or zero).
    ///
    /// `tpn_tan` is the owning WcsMap's effective `TPNTan` value; it is only
    /// consulted for a TPN projection.
    pub(crate) fn init_prj_prm(&mut self, params: &[Vec<Option<f64>>], tpn_tan: bool) {
        let proj = self.proj;

        // Tell the wcslib "proj.c" routines to re-calculate intermediate
        // values on the next call.
        self.inner.flag = 0;
        self.inner.r0 = 0.0;

        // If this is a TPN projection, indicate whether the transformation
        // should include the TAN projection or just the polynomial
        // correction (wcsmap.c:2274). For every other projection type `n` is
        // left for the per-projection setup routine to fill (e.g. astHPXset
        // sets it from p[2]).
        if matches!(proj, WcsProjection::Tpn) {
            self.inner.n = tpn_tan as c_int;
        }

        // Initially set all parameters to the bad value.
        self.p_storage.fill(BAD_VALUE);
        self.p2_storage.fill(BAD_VALUE);

        // Copy the supplied parameters into the arrays.
        if let Some(pv) = params.first() {
            for (m, v) in pv.iter().enumerate().take(PRJ_PARAM_SLOTS) {
                if let Some(v) = v {
                    self.p_storage[m] = *v;
                }
            }
        }
        if let Some(pv) = params.get(1) {
            for (m, v) in pv.iter().enumerate().take(PRJ_PARAM_SLOTS) {
                if let Some(v) = v {
                    self.p2_storage[m] = *v;
                }
            }
        }

        // Supply default values for any missing projection parameters which
        // do not default to zero.
        let p = &mut self.p_storage;
        let p2 = &mut self.p2_storage;
        // One arm per projection, each testing its own parameters for
        // BAD_VALUE, mirroring the `switch` in wcslib's `prjset` / AST's
        // `InitPrjPrm`. Clippy would fold the single-test arms into match
        // guards, which cannot be done for the arms that supply two defaults
        // (Cyp, Tpn) — the result would be a block where structurally
        // identical projections are written two different ways, and no longer
        // line up against the C source.
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

        // Use a default of zero for any remaining unspecified values, except
        // for the un-defaultable parameter (index 1) of the Bonne and conic
        // projections, which retain the bad value.
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

    /// Read a projection parameter.
    ///
    /// - `axis`: 0 for the latitude axis (`p`), 1 for the longitude axis (`p2`).
    /// - `m`: parameter index (0-based).
    ///
    /// Returns `None` if `m >= PRJ_PARAM_SLOTS` or `axis > 1`.
    #[allow(dead_code)] // Used by SkyFrame (Step 7b)
    pub(crate) fn get_param(&self, axis: usize, m: usize) -> Option<f64> {
        if m >= PRJ_PARAM_SLOTS || axis > 1 {
            return None;
        }
        match axis {
            0 => Some(self.p_storage[m]),
            1 => Some(self.p2_storage[m]),
            _ => None,
        }
    }

    #[cfg(test)]
    /// Forward projection: spherical (phi, theta) in degrees to
    /// intermediate world coordinates (x, y).
    ///
    /// Returns an error if the function pointer is not set or the C
    /// function returns a non-zero status.
    pub(crate) fn forward(&mut self, phi: f64, theta: f64) -> AstResult<(f64, f64)> {
        let fwd = self.inner.fwd.ok_or_else(|| {
            AstError::new(
                ErrorCode::Inter,
                "wcslib forward projection function pointer is NULL",
            )
        })?;

        let mut x = 0.0;
        let mut y = 0.0;

        crate::call_counts::hit("wcs_fwd");
        // SAFETY: the function pointer comes from C's PrjInfo table and operates
        // on the AstPrjPrm struct.  We pass valid mutable pointers.
        let status = unsafe { fwd(phi, theta, &mut self.inner, &mut x, &mut y) };

        match status {
            0 => Ok((x, y)),
            1 => Err(AstError::new(
                ErrorCode::Wcspa,
                "wcslib forward projection: invalid projection parameters",
            )),
            2 => Err(AstError::new(
                ErrorCode::Wcspa,
                format!("wcslib forward projection: invalid (phi, theta) = ({phi}, {theta})"),
            )),
            _ => Err(AstError::new(
                ErrorCode::Inter,
                format!("wcslib forward projection: unexpected status {status}"),
            )),
        }
    }

    #[cfg(test)]
    /// Reverse projection: intermediate world coordinates (x, y) to
    /// spherical (phi, theta) in degrees.
    ///
    /// Returns an error if the function pointer is not set or the C
    /// function returns a non-zero status.
    pub(crate) fn reverse(&mut self, x: f64, y: f64) -> AstResult<(f64, f64)> {
        let rev = self.inner.rev.ok_or_else(|| {
            AstError::new(
                ErrorCode::Inter,
                "wcslib reverse projection function pointer is NULL",
            )
        })?;

        let mut phi = 0.0;
        let mut theta = 0.0;

        crate::call_counts::hit("wcs_rev");
        // SAFETY: the function pointer comes from C's PrjInfo table and operates
        // on the AstPrjPrm struct.  We pass valid mutable pointers.
        let status = unsafe { rev(x, y, &mut self.inner, &mut phi, &mut theta) };

        match status {
            0 => Ok((phi, theta)),
            1 => Err(AstError::new(
                ErrorCode::Wcspa,
                "wcslib reverse projection: invalid projection parameters",
            )),
            2 => Err(AstError::new(
                ErrorCode::Wcspa,
                format!("wcslib reverse projection: invalid (x, y) = ({x}, {y})"),
            )),
            _ => Err(AstError::new(
                ErrorCode::Inter,
                format!("wcslib reverse projection: unexpected status {status}"),
            )),
        }
    }

    /// Forward projection, returning the raw wcslib status alongside the
    /// output (x, y) in degrees.
    ///
    /// Unlike [`forward`](Self::forward), this never converts a non-zero
    /// status into an error — it returns whatever the C function wrote to
    /// the output slots together with the status code. This mirrors C call
    /// sites (e.g. `LongRange`, wcsmap.c:2675) that read the projected
    /// coordinates regardless of the returned status.
    pub(crate) fn forward_raw(&mut self, phi: f64, theta: f64) -> (c_int, f64, f64) {
        let Some(fwd) = self.inner.fwd else {
            return (1, 0.0, 0.0);
        };
        let mut x = 0.0;
        let mut y = 0.0;
        crate::call_counts::hit("wcs_fwd");
        // SAFETY: the function pointer comes from C's PrjInfo table and operates
        // on the AstPrjPrm struct. We pass valid mutable pointers.
        let status = unsafe { fwd(phi, theta, &mut self.inner, &mut x, &mut y) };
        (status, x, y)
    }

    /// Reverse projection, returning the raw wcslib status alongside the
    /// output (longitude, latitude) in degrees.
    ///
    /// The status-agnostic counterpart to [`reverse`](Self::reverse); see
    /// [`forward_raw`](Self::forward_raw). Used by `LongRange`
    /// (wcsmap.c:2664) which inspects the returned longitude only when the
    /// status is zero.
    pub(crate) fn reverse_raw(&mut self, x: f64, y: f64) -> (c_int, f64, f64) {
        let Some(rev) = self.inner.rev else {
            return (1, 0.0, 0.0);
        };
        let mut phi = 0.0;
        let mut theta = 0.0;
        crate::call_counts::hit("wcs_rev");
        // SAFETY: the function pointer comes from C's PrjInfo table and operates
        // on the AstPrjPrm struct. We pass valid mutable pointers.
        let status = unsafe { rev(x, y, &mut self.inner, &mut phi, &mut theta) };
        (status, phi, theta)
    }
}

impl std::fmt::Debug for PrjParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Show the projection code and flag; omit the raw C internals.
        let code = std::str::from_utf8(&self.inner.code)
            .unwrap_or("???")
            .trim_end_matches('\0');
        f.debug_struct("PrjParams")
            .field("code", &code)
            .field("flag", &self.inner.flag)
            .field("phi0", &self.inner.phi0)
            .field("theta0", &self.inner.theta0)
            .field("r0", &self.inner.r0)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The `mxpar` / `mxpar2` columns of C's `PrjInfo` table
    /// (wcsmap.c:684-716), as `astGetPVMax` reports them. Golden values come
    /// from a probe against the reference `libast`; `mxpar2` is 4 everywhere
    /// except `-TPN`.
    #[test]
    fn pv_limits_match_the_c_projection_table() {
        let cases: &[(WcsProjection, usize, usize)] = &[
            (WcsProjection::Azp, 2, 4),
            (WcsProjection::Szp, 3, 4),
            (WcsProjection::Tan, 0, 4),
            (WcsProjection::Stg, 0, 4),
            (WcsProjection::Sin, 2, 4),
            (WcsProjection::Arc, 0, 4),
            (WcsProjection::Zpn, WCSLIB_MXPAR, 4),
            (WcsProjection::Zea, 0, 4),
            (WcsProjection::Air, 1, 4),
            (WcsProjection::Cyp, 2, 4),
            (WcsProjection::Cea, 1, 4),
            (WcsProjection::Car, 0, 4),
            (WcsProjection::Mer, 0, 4),
            (WcsProjection::Sfl, 0, 4),
            (WcsProjection::Par, 0, 4),
            (WcsProjection::Mol, 0, 4),
            (WcsProjection::Ait, 0, 4),
            (WcsProjection::Cop, 2, 4),
            (WcsProjection::Coe, 2, 4),
            (WcsProjection::Cod, 2, 4),
            (WcsProjection::Coo, 2, 4),
            (WcsProjection::Bon, 1, 4),
            (WcsProjection::Pco, 0, 4),
            (WcsProjection::Tsc, 0, 4),
            (WcsProjection::Csc, 0, 4),
            (WcsProjection::Qsc, 0, 4),
            (WcsProjection::Ncp, 2, 4),
            (WcsProjection::Gls, 0, 4),
            (WcsProjection::Hpx, 2, 4),
            (WcsProjection::Xph, 0, 4),
            (WcsProjection::Tpn, WCSLIB_MXPAR, WCSLIB_MXPAR),
        ];
        for &(proj, lat, lon) in cases {
            assert_eq!(proj.pv_max_lat(), lat, "mxpar for {}", proj.code());
            assert_eq!(proj.pv_max_lon(), lon, "mxpar2 for {}", proj.code());
        }
    }

    #[test]
    fn tan_reference_point() {
        let mut prj = PrjParams::new(WcsProjection::Tan).unwrap();
        // TAN reference point is (0, 90) degrees — projects to (0, 0).
        let (x, y) = prj.forward(0.0, 90.0).unwrap();
        assert!(x.abs() < 1e-10, "x = {x}");
        assert!(y.abs() < 1e-10, "y = {y}");
    }

    #[test]
    fn tan_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Tan).unwrap();
        let (x, y) = prj.forward(10.0, 80.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 10.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 80.0).abs() < 1e-10, "theta = {theta}");
    }

    #[test]
    fn car_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Car).unwrap();
        let (x, y) = prj.forward(45.0, 30.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 45.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 30.0).abs() < 1e-10, "theta = {theta}");
    }

    #[test]
    fn sin_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Sin).unwrap();
        let (x, y) = prj.forward(5.0, 85.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 5.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 85.0).abs() < 1e-10, "theta = {theta}");
    }

    #[test]
    fn ait_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Ait).unwrap();
        let (x, y) = prj.forward(30.0, 45.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 30.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 45.0).abs() < 1e-10, "theta = {theta}");
    }

    #[test]
    fn arc_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Arc).unwrap();
        let (x, y) = prj.forward(15.0, 60.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 15.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 60.0).abs() < 1e-10, "theta = {theta}");
    }

    #[test]
    fn stg_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Stg).unwrap();
        let (x, y) = prj.forward(20.0, 70.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 20.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 70.0).abs() < 1e-10, "theta = {theta}");
    }

    #[test]
    fn mer_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Mer).unwrap();
        let (x, y) = prj.forward(25.0, 40.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 25.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 40.0).abs() < 1e-10, "theta = {theta}");
    }

    #[test]
    fn projection_code_round_trip() {
        for proj in [
            WcsProjection::Tan,
            WcsProjection::Sin,
            WcsProjection::Ait,
            WcsProjection::Car,
            WcsProjection::Azp,
            WcsProjection::Hpx,
            WcsProjection::Tpn,
            WcsProjection::Ncp,
            WcsProjection::Gls,
        ] {
            let code = proj.code();
            let restored = WcsProjection::from_code(code).unwrap();
            assert_eq!(proj, restored, "round-trip failed for {code}");
        }
    }

    #[test]
    fn projection_code_case_insensitive() {
        assert_eq!(WcsProjection::from_code("tan"), Some(WcsProjection::Tan));
        assert_eq!(WcsProjection::from_code("Ait"), Some(WcsProjection::Ait));
        assert_eq!(WcsProjection::from_code("XYZ"), None);
    }

    #[test]
    fn type_code_round_trip() {
        // All non-deprecated projections should round-trip through type_code.
        let projections = [
            WcsProjection::Azp,
            WcsProjection::Szp,
            WcsProjection::Tan,
            WcsProjection::Stg,
            WcsProjection::Sin,
            WcsProjection::Arc,
            WcsProjection::Zpn,
            WcsProjection::Zea,
            WcsProjection::Air,
            WcsProjection::Cyp,
            WcsProjection::Cea,
            WcsProjection::Car,
            WcsProjection::Mer,
            WcsProjection::Sfl,
            WcsProjection::Par,
            WcsProjection::Mol,
            WcsProjection::Ait,
            WcsProjection::Cop,
            WcsProjection::Coe,
            WcsProjection::Cod,
            WcsProjection::Coo,
            WcsProjection::Bon,
            WcsProjection::Pco,
            WcsProjection::Tsc,
            WcsProjection::Csc,
            WcsProjection::Qsc,
            WcsProjection::Hpx,
            WcsProjection::Xph,
            WcsProjection::Tpn,
        ];
        for proj in projections {
            let tc = proj.type_code();
            let restored = WcsProjection::from_type_code(tc).unwrap();
            assert_eq!(
                proj, restored,
                "type_code round-trip failed for {:?} (code {})",
                proj, tc
            );
        }
    }

    #[test]
    fn ncp_gls_rejected_by_new() {
        // NCP and GLS must be mapped to SIN/SFL at a higher level.
        assert!(PrjParams::new(WcsProjection::Ncp).is_err());
        assert!(PrjParams::new(WcsProjection::Gls).is_err());
    }

    #[test]
    fn init_prj_prm_keeps_projection_callable() {
        let mut prj = PrjParams::new(WcsProjection::Tan).unwrap();
        // After init the projection functions are installed but the flag is
        // clear, as C's InitPrjPrm leaves it, so the first call sets up.
        assert_eq!(prj.inner.flag, 0);
        // Re-initializing with parameters leaves the projection callable.
        prj.init_prj_prm(&[vec![Some(1.0)], Vec::new()], true);
        assert_eq!(prj.inner.flag, 0);
        let result = prj.forward(0.0, 90.0);
        assert!(result.is_ok());
        assert_ne!(prj.inner.flag, 0);
    }

    #[test]
    fn zpn_callable_only_after_parameter_set() {
        // ZPN's set-up, which its forward function runs on first use, fails
        // while its coefficients are all zero, so a freshly constructed ZPN
        // has no usable transform.
        let mut prj = PrjParams::new(WcsProjection::Zpn).unwrap();
        assert!(prj.forward(0.0, 80.0).is_err());
        // Supplying the linear coefficient makes the polynomial well-defined;
        // init_prj_prm clears the flag so the next call sets up again.
        prj.init_prj_prm(&[vec![None, Some(1.0)], Vec::new()], true);
        assert!(prj.forward(0.0, 80.0).is_ok());
    }

    #[test]
    fn get_param_out_of_range() {
        let prj = PrjParams::new(WcsProjection::Tan).unwrap();
        assert!(prj.get_param(0, PRJ_PARAM_SLOTS).is_none());
        assert!(prj.get_param(2, 0).is_none());
    }

    #[test]
    #[allow(clippy::approx_constant)] // -3.14 is opaque test parameter, not π
    fn init_prj_prm_copies_supplied_params() {
        let mut prj = PrjParams::new(WcsProjection::Tan).unwrap();
        prj.init_prj_prm(
            &[
                vec![None, None, None, None, None, Some(42.0)],
                vec![
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(-3.14),
                ],
            ],
            true,
        );
        assert_eq!(prj.get_param(0, 5), Some(42.0));
        assert_eq!(prj.get_param(1, 10), Some(-3.14));
    }

    #[test]
    fn tpn_initialises() {
        // TPN is handled by tpn.c, not proj.c.  Verify it initializes.
        let prj = PrjParams::new(WcsProjection::Tpn);
        assert!(prj.is_ok(), "TPN should initialise successfully");
    }

    #[test]
    fn tpn_tan_selects_polynomial_only_form() {
        // `astTPNfwd` branches on `prj->n` (tpn.c:87) to decide whether the
        // TAN part is applied before the polynomial; `init_prj_prm` sets `n`
        // from the owning WcsMap's TPNTan attribute (wcsmap.c:2274). With the
        // default unit coefficients the polynomial is the identity, so
        // clearing TPNTan makes the whole projection an identity while
        // setting it applies the gnomonic projection.
        let params = [Vec::new(), Vec::new()];
        let mut with_tan = PrjParams::new(WcsProjection::Tpn).unwrap();
        with_tan.init_prj_prm(&params, true);
        let mut poly_only = PrjParams::new(WcsProjection::Tpn).unwrap();
        poly_only.init_prj_prm(&params, false);

        let (_, y_tan) = with_tan.forward(0.0, 60.0).unwrap();
        let (x_poly, y_poly) = poly_only.forward(0.0, 60.0).unwrap();
        assert!(
            x_poly.abs() < 1e-10 && (y_poly - 60.0).abs() < 1e-10,
            "polynomial-only TPN should pass its inputs through: ({x_poly}, {y_poly})"
        );
        assert!(
            (y_tan - y_poly).abs() > 1.0,
            "TPNTan should change the projection: {y_tan} vs {y_poly}"
        );
    }

    #[test]
    fn debug_format() {
        let prj = PrjParams::new(WcsProjection::Tan).unwrap();
        let dbg = format!("{prj:?}");
        assert!(
            dbg.contains("TAN"),
            "debug output should contain 'TAN': {dbg}"
        );
    }

    #[test]
    fn zea_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Zea).unwrap();
        let (x, y) = prj.forward(12.0, 55.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 12.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 55.0).abs() < 1e-10, "theta = {theta}");
    }

    #[test]
    fn mol_round_trip() {
        let mut prj = PrjParams::new(WcsProjection::Mol).unwrap();
        let (x, y) = prj.forward(35.0, 50.0).unwrap();
        let (phi, theta) = prj.reverse(x, y).unwrap();
        assert!((phi - 35.0).abs() < 1e-10, "phi = {phi}");
        assert!((theta - 50.0).abs() < 1e-10, "theta = {theta}");
    }
}

#[cfg(test)]
mod count_tests {
    /// Every call into wcslib in this file is counted: the projection
    /// functions, reached through pointers as in C (wcsmap.c:2855,2883), as
    /// `wcs_fwd` and `wcs_rev`.
    #[test]
    fn every_ffi_call_is_counted() {
        let src = include_str!("ffi.rs");
        let start = src.find("\n}\n").expect("end of the extern block");
        let end = src.find("mod count_tests").expect("this module");
        let lines: Vec<&str> = src[start..end].lines().collect();
        let mut uncounted = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") || t.contains("fn ast") {
                continue;
            }
            for (pat, key) in [
                ("unsafe { fwd(", "hit(\"wcs_fwd\")"),
                ("unsafe { rev(", "hit(\"wcs_rev\")"),
            ] {
                if t.contains(pat) && !lines[..i].iter().rev().take(4).any(|l| l.contains(key)) {
                    uncounted.push(*line);
                }
            }
        }
        assert!(
            uncounted.is_empty(),
            "uncounted wcslib calls: {uncounted:#?}"
        );
    }

    #[cfg(feature = "call-counts")]
    #[test]
    fn tan_forward_counts_one_evaluation() {
        let mut prj = super::PrjParams::new(super::WcsProjection::Tan).unwrap();
        crate::call_counts::reset();
        let _ = prj.forward_raw(10.0, 80.0);
        assert_eq!(crate::call_counts::snapshot().get("wcs_fwd"), Some(&1));
    }
}
