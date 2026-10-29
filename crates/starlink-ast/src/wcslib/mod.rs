// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! FITS-WCS projections.
//!
//! This module is `pub(crate)` and provides the [`WcsProjection`] enum
//! covering all 31 FITS-WCS projection types and its per-projection tables.
//! `trig` holds the degree trigonometry the projections share, `prj` the
//! projection state and dispatch, and `zenithal` the zenithal
//! projections, `cylindrical` and `pseudocyl` the cylindrical and
//! pseudo-cylindrical ones, `conic` and `polyconic` the conic and
//! polyconic ones, `quadcube` and `healpix` the quad-cube and HEALPix
//! ones, and `tpn` the polynomial-corrected gnomonic projection; `math`
//! holds the uncounted library functions they call.

use std::f64::consts::FRAC_PI_2;

use crate::constants::BAD_VALUE;

mod conic;
mod cylindrical;
#[cfg(test)]
mod golden_tests;
mod healpix;
mod math;
mod polyconic;
pub(crate) mod prj;
mod pseudocyl;
mod quadcube;
mod tpn;
pub(crate) mod trig;
mod zenithal;

// ---------------------------------------------------------------------------
// WcsProjection enum
// ---------------------------------------------------------------------------

/// FITS-WCS projection type.
///
/// Each variant corresponds to a 3-letter FITS projection code recognized
/// by wcslib.  `Ncp` and `Gls` are deprecated aliases for
/// `Sin` and `Sfl` respectively — they are handled at the `WcsMap` level
/// by setting appropriate projection parameters before delegating to the
/// underlying standard projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WcsProjection {
    /// Zenithal/azimuthal perspective
    Azp,
    /// Slant zenithal perspective
    Szp,
    /// Gnomonic (tangent plane)
    Tan,
    /// Stereographic
    Stg,
    /// Orthographic / synthesis
    Sin,
    /// Zenithal/azimuthal equidistant
    Arc,
    /// Zenithal/azimuthal polynomial
    Zpn,
    /// Zenithal/azimuthal equal-area
    Zea,
    /// Airy
    Air,
    /// Cylindrical perspective
    Cyp,
    /// Cylindrical equal-area
    Cea,
    /// Plate carree
    Car,
    /// Mercator
    Mer,
    /// Sanson-Flamsteed (global sinusoidal)
    Sfl,
    /// Parabolic
    Par,
    /// Mollweide
    Mol,
    /// Hammer-Aitoff
    Ait,
    /// Conic perspective
    Cop,
    /// Conic equal-area
    Coe,
    /// Conic equidistant
    Cod,
    /// Conic orthomorphic
    Coo,
    /// Bonne
    Bon,
    /// Polyconic
    Pco,
    /// Tangential spherical cube
    Tsc,
    /// COBE spherical cube
    Csc,
    /// Quadrilateralized spherical cube
    Qsc,
    /// North celestial pole (deprecated; alias for SIN)
    Ncp,
    /// Global sinusoidal (deprecated; alias for SFL)
    Gls,
    /// TAN with polynomial correction terms
    Tpn,
    /// HEALPix
    Hpx,
    /// HEALPix polar
    Xph,
    /// Unknown / blank projection type (simplifies to UnitMap)
    Unknown,
}

impl WcsProjection {
    /// Return the 3-letter FITS projection code (uppercase).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Azp => "AZP",
            Self::Szp => "SZP",
            Self::Tan => "TAN",
            Self::Stg => "STG",
            Self::Sin => "SIN",
            Self::Arc => "ARC",
            Self::Zpn => "ZPN",
            Self::Zea => "ZEA",
            Self::Air => "AIR",
            Self::Cyp => "CYP",
            Self::Cea => "CEA",
            Self::Car => "CAR",
            Self::Mer => "MER",
            Self::Sfl => "SFL",
            Self::Par => "PAR",
            Self::Mol => "MOL",
            Self::Ait => "AIT",
            Self::Cop => "COP",
            Self::Coe => "COE",
            Self::Cod => "COD",
            Self::Coo => "COO",
            Self::Bon => "BON",
            Self::Pco => "PCO",
            Self::Tsc => "TSC",
            Self::Csc => "CSC",
            Self::Qsc => "QSC",
            Self::Ncp => "NCP",
            Self::Gls => "GLS",
            Self::Tpn => "TPN",
            Self::Hpx => "HPX",
            Self::Xph => "XPH",
            Self::Unknown => "   ",
        }
    }

    /// Return the textual projection description used for a SkyFrame's
    /// `Projection` attribute.
    // C: `astWcsPrjDesc` (wcsmap.c:4419) and the `desc` column of the
    // `PrjInfo` table (wcsmap.c:685-716).
    pub fn description(&self) -> &'static str {
        match self {
            Self::Azp => "zenithal perspective",
            Self::Szp => "slant zenithal perspective",
            Self::Tan => "gnomonic",
            Self::Stg => "stereographic",
            Self::Sin => "orthographic",
            Self::Arc => "zenithal equidistant",
            Self::Zpn => "zenithal polynomial",
            Self::Zea => "zenithal equal area",
            Self::Air => "Airy",
            Self::Cyp => "cylindrical perspective",
            Self::Cea => "cylindrical equal area",
            Self::Car => "Cartesian",
            Self::Mer => "Mercator",
            Self::Sfl => "Sanson-Flamsteed",
            Self::Par => "parabolic",
            Self::Mol => "Mollweide",
            Self::Ait => "Hammer-Aitoff",
            Self::Cop => "conical perspective",
            Self::Coe => "conical equal area",
            Self::Cod => "conical equidistant",
            Self::Coo => "conical orthomorphic",
            Self::Bon => "Bonne's equal area",
            Self::Pco => "polyconic",
            Self::Tsc => "tangential spherical cube",
            Self::Csc => "cobe quadrilateralized spherical cube",
            Self::Qsc => "quadrilateralized spherical cube",
            Self::Ncp => "AIPS north celestial pole",
            Self::Gls => "sinusoidal",
            Self::Hpx => "HEALPix",
            Self::Xph => "polar HEALPix",
            Self::Tpn => "gnomonic polynomial",
            Self::Unknown => "<null>",
        }
    }

    /// Largest legal `m` in `PVi_m` on the *latitude* axis.
    ///
    /// Each projection allocates one more parameter slot than its
    /// `mxpar` limit, so the largest legal index is `mxpar` itself.
    // C: the `mxpar` column of the `PrjInfo` table (wcsmap.c:684-716). C
    // sizes `params.p` as `mxpar + 1` doubles (wcsmap.c:5862) and
    // `astGetPVMax` reports that allocation minus one for the latitude axis
    // (wcsmap.c:2208-2212).
    pub fn pv_max_lat(&self) -> usize {
        match self {
            Self::Azp
            | Self::Sin
            | Self::Cyp
            | Self::Cop
            | Self::Coe
            | Self::Cod
            | Self::Coo
            | Self::Ncp
            | Self::Hpx => 2,
            Self::Szp => 3,
            Self::Air | Self::Cea | Self::Bon => 1,
            Self::Zpn | Self::Tpn => WCSLIB_MXPAR,
            Self::Tan
            | Self::Stg
            | Self::Arc
            | Self::Zea
            | Self::Car
            | Self::Mer
            | Self::Sfl
            | Self::Par
            | Self::Mol
            | Self::Ait
            | Self::Pco
            | Self::Tsc
            | Self::Csc
            | Self::Qsc
            | Self::Gls
            | Self::Xph
            | Self::Unknown => 0,
        }
    }

    /// Largest legal `m` in `PVi_m` on the *longitude* axis.
    ///
    /// The `mxpar2` column of the same table, which is 4 for every projection
    /// except `-TPN`, whose longitude axis also carries polynomial
    /// coefficients.
    pub fn pv_max_lon(&self) -> usize {
        match self {
            Self::Tpn => WCSLIB_MXPAR,
            _ => 4,
        }
    }

    /// Parse a 3-letter FITS projection code (case-insensitive).
    ///
    /// Returns `None` if the code is not recognized.
    pub fn from_code(s: &str) -> Option<Self> {
        // Fast path: check length first.
        if s.len() != 3 {
            return None;
        }
        // Uppercase the 3 bytes for comparison.
        let mut buf = [0u8; 3];
        for (i, b) in s.bytes().enumerate() {
            buf[i] = b.to_ascii_uppercase();
        }
        match &buf {
            b"AZP" => Some(Self::Azp),
            b"SZP" => Some(Self::Szp),
            b"TAN" => Some(Self::Tan),
            b"STG" => Some(Self::Stg),
            b"SIN" => Some(Self::Sin),
            b"ARC" => Some(Self::Arc),
            b"ZPN" => Some(Self::Zpn),
            b"ZEA" => Some(Self::Zea),
            b"AIR" => Some(Self::Air),
            b"CYP" => Some(Self::Cyp),
            b"CEA" => Some(Self::Cea),
            b"CAR" => Some(Self::Car),
            b"MER" => Some(Self::Mer),
            b"SFL" => Some(Self::Sfl),
            b"PAR" => Some(Self::Par),
            b"MOL" => Some(Self::Mol),
            b"AIT" => Some(Self::Ait),
            b"COP" => Some(Self::Cop),
            b"COE" => Some(Self::Coe),
            b"COD" => Some(Self::Cod),
            b"COO" => Some(Self::Coo),
            b"BON" => Some(Self::Bon),
            b"PCO" => Some(Self::Pco),
            b"TSC" => Some(Self::Tsc),
            b"CSC" => Some(Self::Csc),
            b"QSC" => Some(Self::Qsc),
            b"NCP" => Some(Self::Ncp),
            b"GLS" => Some(Self::Gls),
            b"TPN" => Some(Self::Tpn),
            b"HPX" => Some(Self::Hpx),
            b"XPH" => Some(Self::Xph),
            _ => None,
        }
    }

    /// The native latitude of the fiducial point, in radians.
    ///
    /// `BAD_VALUE` for the conics, whose fiducial latitude is a projection
    /// parameter.
    // C: the `theta0` column of the `PrjInfo` table (wcsmap.c:703-737),
    // which `GetNatLat` returns.
    pub(crate) fn table_theta0(self) -> f64 {
        use WcsProjection::*;
        match self {
            Azp | Szp | Tan | Stg | Sin | Arc | Zpn | Zea | Air | Xph | Tpn => FRAC_PI_2,
            Cop | Coe | Cod | Coo => BAD_VALUE,
            Cyp | Cea | Car | Mer | Sfl | Par | Mol | Ait | Bon | Pco | Tsc | Csc | Qsc | Ncp
            | Gls | Hpx | Unknown => 0.0,
        }
    }

    /// How far past a limit of the accepted range, in degrees, a native
    /// longitude or latitude from the reverse projection is set to the limit
    /// rather than rejected.
    // C: the `tol` column of the `PrjInfo` table (wcsmap.c:711-743): the
    // per-projection tolerances of wcslib's `prjbchk` calls.
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
// Projection parameter limits
// ---------------------------------------------------------------------------

/// Largest projection-parameter index any projection accepts: the `mxpar` /
/// `mxpar2` value of `-ZPN` and `-TPN`.
// C: `WCSLIB_MXPAR` (proj.h:59 in reference/starlink-ast/wcslib), used in the
// `PrjInfo` table (wcsmap.c:684-716).
const WCSLIB_MXPAR: usize = 100;

/// Number of slots in a projection-parameter array.
///
/// C allocates `mxpar + 1` doubles per axis (wcsmap.c:5862-5863) so that
/// index `mxpar` itself is addressable; `astGetPVMax` reports that allocation
/// minus one (wcsmap.c:2205-2212). Every array here is sized for the largest
/// projection, so the same indices are legal.
pub(crate) const PRJ_PARAM_SLOTS: usize = WCSLIB_MXPAR + 1;

#[cfg(test)]
mod tests {
    use super::*;

    // The `mxpar` / `mxpar2` columns of C's `PrjInfo` table
    // (wcsmap.c:684-716), as `astGetPVMax` reports them. Golden values come
    // from a probe against the reference `libast`; `mxpar2` is 4 everywhere
    // except `-TPN`.
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
}
