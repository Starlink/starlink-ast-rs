// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Safe Rust wrappers for the vendored wcslib projection functions.
//!
//! The C wcslib library is compiled from `vendor/wcslib/{proj,tpn,wcstrig,ast_memory_stub}.c`
//! into `libast_wcslib.a` by `build.rs`.  The C symbols keep their original
//! `ast`-prefixed names (e.g. `astPRJset`, `astTANfwd`).
//!
//! This module is `pub(crate)` and provides a [`WcsProjection`] enum
//! covering all 31 FITS-WCS projection types; `ffi` holds the safe
//! `PrjParams` wrapper around the raw `AstPrjPrm` C struct.

pub(crate) mod ffi;

// ---------------------------------------------------------------------------
// WcsProjection enum
// ---------------------------------------------------------------------------

/// FITS-WCS projection type.
///
/// Each variant corresponds to a 3-letter FITS projection code recognized
/// by the vendored wcslib.  `Ncp` and `Gls` are deprecated aliases for
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

    /// Return the integer type code used by the C wcslib.
    ///
    /// These match the `WCS__*` constants in `vendor/wcslib/proj.c`.
    /// The encoding groups projections by class (zenithal=1xx,
    /// cylindrical=2xx, pseudo-cylindrical=3xx, etc.).
    #[allow(dead_code)] // Used by SlaMap (Step 7a Task 4)
    pub(crate) fn type_code(&self) -> i32 {
        match self {
            // Zenithal (1xx)
            Self::Azp => 101,
            Self::Szp => 102,
            Self::Tan => 103,
            Self::Stg => 104,
            Self::Sin => 105,
            Self::Arc => 106,
            Self::Zpn => 107,
            Self::Zea => 108,
            Self::Air => 109,
            // Cylindrical (2xx)
            Self::Cyp => 201,
            Self::Cea => 202,
            Self::Car => 203,
            Self::Mer => 204,
            // Pseudo-cylindrical (3xx)
            Self::Sfl => 301,
            Self::Par => 302,
            Self::Mol => 303,
            // Pseudo-cylindrical (4xx)
            Self::Ait => 401,
            // Conic (5xx)
            Self::Cop => 501,
            Self::Coe => 502,
            Self::Cod => 503,
            Self::Coo => 504,
            // Polyconic (6xx)
            Self::Bon => 601,
            Self::Pco => 602,
            // Quad-cube (7xx)
            Self::Tsc => 701,
            Self::Csc => 702,
            Self::Qsc => 703,
            // HEALPix (8xx)
            Self::Hpx => 801,
            Self::Xph => 802,
            // Deprecated (use same code as underlying projection)
            Self::Ncp => 105, // -> SIN
            Self::Gls => 301, // -> SFL
            // TPN uses 999 (defined in tpn.c)
            Self::Tpn => 999,
            // Unknown/blank type — no valid type code.
            Self::Unknown => 0,
        }
    }

    /// Convert an integer type code to a projection variant.
    ///
    /// Returns `None` if the code is not recognized.  Note that the
    /// deprecated `Ncp` (105) and `Gls` (301) are not returned — the
    /// canonical `Sin` and `Sfl` are returned instead.
    #[allow(dead_code)] // Used by SlaMap (Step 7a Task 4)
    pub(crate) fn from_type_code(i: i32) -> Option<Self> {
        match i {
            101 => Some(Self::Azp),
            102 => Some(Self::Szp),
            103 => Some(Self::Tan),
            104 => Some(Self::Stg),
            105 => Some(Self::Sin),
            106 => Some(Self::Arc),
            107 => Some(Self::Zpn),
            108 => Some(Self::Zea),
            109 => Some(Self::Air),
            201 => Some(Self::Cyp),
            202 => Some(Self::Cea),
            203 => Some(Self::Car),
            204 => Some(Self::Mer),
            301 => Some(Self::Sfl),
            302 => Some(Self::Par),
            303 => Some(Self::Mol),
            401 => Some(Self::Ait),
            501 => Some(Self::Cop),
            502 => Some(Self::Coe),
            503 => Some(Self::Cod),
            504 => Some(Self::Coo),
            601 => Some(Self::Bon),
            602 => Some(Self::Pco),
            701 => Some(Self::Tsc),
            702 => Some(Self::Csc),
            703 => Some(Self::Qsc),
            801 => Some(Self::Hpx),
            802 => Some(Self::Xph),
            999 => Some(Self::Tpn),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Raw FFI types and extern declarations
// ---------------------------------------------------------------------------

/// Largest projection-parameter index any projection accepts — C's
/// `WCSLIB_MXPAR` (`vendor/wcslib/proj.h:59`), which is the `mxpar` /
/// `mxpar2` column value of `-ZPN` and `-TPN` in the `PrjInfo` table
/// (wcsmap.c:684-716).
const WCSLIB_MXPAR: usize = 100;
