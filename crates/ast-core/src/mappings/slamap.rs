//! Mapping that chains celestial coordinate system conversions via PAL.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::pal;
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad};
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

// ---------------------------------------------------------------------------
// SlaCvtType — conversion step types
// ---------------------------------------------------------------------------

/// A celestial coordinate conversion type supported by [`SlaMap`].
///
/// Each variant corresponds to a specific PAL (positional astronomy library)
/// operation.  The integer discriminant matches the C AST library's
/// `AST__SLA_*` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlaCvtType {
    /// Add E-terms of aberration.
    Addet = 1,
    /// Remove E-terms of aberration.
    Subet = 2,
    /// Bessel-Newcomb precession (FK4).
    Prebn = 3,
    /// IAU 1976 precession (FK5).
    Prec = 4,
    /// FK4 to FK5 (zero proper motion).
    Fk45z = 5,
    /// FK5 to FK4 (zero proper motion).
    Fk54z = 6,
    /// Geocentric apparent to mean place.
    Amp = 7,
    /// Mean place to geocentric apparent.
    Map = 8,
    /// Ecliptic to equatorial.
    Ecleq = 9,
    /// Equatorial to ecliptic.
    Eqecl = 10,
    /// Galactic to equatorial.
    Galeq = 11,
    /// Equatorial to galactic.
    Eqgal = 12,
    /// Galactic to supergalactic.
    Galsup = 13,
    /// Supergalactic to galactic.
    Supgal = 14,
    /// Helioprojective-Cartesian to J2000 equatorial.
    Hpceq = 15,
    /// J2000 equatorial to helioprojective-Cartesian.
    Eqhpc = 16,
    /// Helioprojective-Radial to J2000 equatorial.
    Hpreq = 17,
    /// J2000 equatorial to helioprojective-Radial.
    Eqhpr = 18,
    /// ICRS (Hipparcos) to FK5.
    Hfk5z = 19,
    /// FK5 to ICRS (Hipparcos).
    Fk5hz = 20,
    /// Helio-ecliptic to equatorial.
    Heeq = 21,
    /// Equatorial to helio-ecliptic.
    Eqhe = 22,
    /// Dynamical J2000 to ICRS.
    J2000h = 23,
    /// ICRS to dynamical J2000.
    Hj2000 = 24,
    /// Horizon (az/el) to equatorial (ha/dec).
    Dh2e = 25,
    /// Equatorial (ha/dec) to horizon (az/el).
    De2h = 26,
    /// RA to hour angle: `ha = last - ra`.
    R2h = 27,
    /// Hour angle to RA: `ra = last - ha`.
    H2r = 28,
}

impl SlaCvtType {
    /// Number of floating-point arguments this conversion requires.
    pub fn nargs(&self) -> usize {
        match self {
            Self::Galeq
            | Self::Eqgal
            | Self::Galsup
            | Self::Supgal
            | Self::J2000h
            | Self::Hj2000 => 0,
            Self::Addet
            | Self::Subet
            | Self::Fk45z
            | Self::Fk54z
            | Self::Ecleq
            | Self::Eqecl
            | Self::Hfk5z
            | Self::Fk5hz
            | Self::Heeq
            | Self::Eqhe
            | Self::R2h
            | Self::H2r => 1,
            Self::Prebn | Self::Prec | Self::Amp | Self::Map | Self::Dh2e | Self::De2h => 2,
            Self::Hpceq | Self::Eqhpc | Self::Hpreq | Self::Eqhpr => 4,
        }
    }

    /// Uppercase code string for serialisation (e.g. `"PREC"`, `"EQGAL"`).
    pub fn code_string(&self) -> &'static str {
        match self {
            Self::Addet => "ADDET",
            Self::Subet => "SUBET",
            Self::Prebn => "PREBN",
            Self::Prec => "PREC",
            Self::Fk45z => "FK45Z",
            Self::Fk54z => "FK54Z",
            Self::Amp => "AMP",
            Self::Map => "MAP",
            Self::Ecleq => "ECLEQ",
            Self::Eqecl => "EQECL",
            Self::Galeq => "GALEQ",
            Self::Eqgal => "EQGAL",
            Self::Galsup => "GALSUP",
            Self::Supgal => "SUPGAL",
            Self::Hpceq => "HPCEQ",
            Self::Eqhpc => "EQHPC",
            Self::Hpreq => "HPREQ",
            Self::Eqhpr => "EQHPR",
            Self::Hfk5z => "HFK5Z",
            Self::Fk5hz => "FK5HZ",
            Self::Heeq => "HEEQ",
            Self::Eqhe => "EQHE",
            Self::J2000h => "J2000H",
            Self::Hj2000 => "HJ2000",
            Self::Dh2e => "H2E",
            Self::De2h => "E2H",
            Self::R2h => "R2H",
            Self::H2r => "H2R",
        }
    }

    /// Parse a code string (case-insensitive) into a conversion type.
    pub fn from_code(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "ADDET" => Some(Self::Addet),
            "SUBET" => Some(Self::Subet),
            "PREBN" => Some(Self::Prebn),
            "PREC" => Some(Self::Prec),
            "FK45Z" => Some(Self::Fk45z),
            "FK54Z" => Some(Self::Fk54z),
            "AMP" => Some(Self::Amp),
            "MAP" => Some(Self::Map),
            "ECLEQ" => Some(Self::Ecleq),
            "EQECL" => Some(Self::Eqecl),
            "GALEQ" => Some(Self::Galeq),
            "EQGAL" => Some(Self::Eqgal),
            "GALSUP" => Some(Self::Galsup),
            "SUPGAL" => Some(Self::Supgal),
            "HPCEQ" => Some(Self::Hpceq),
            "EQHPC" => Some(Self::Eqhpc),
            "HPREQ" => Some(Self::Hpreq),
            "EQHPR" => Some(Self::Eqhpr),
            "HFK5Z" => Some(Self::Hfk5z),
            "FK5HZ" => Some(Self::Fk5hz),
            "HEEQ" => Some(Self::Heeq),
            "EQHE" => Some(Self::Eqhe),
            "J2000H" => Some(Self::J2000h),
            "HJ2000" => Some(Self::Hj2000),
            "H2E" | "DH2E" => Some(Self::Dh2e),
            "E2H" | "DE2H" => Some(Self::De2h),
            "R2H" => Some(Self::R2h),
            "H2R" => Some(Self::H2r),
            _ => None,
        }
    }

    /// Return the inverse conversion type.
    ///
    /// For `Prec` and `Prebn` the inverse is itself (with swapped arguments
    /// at call time).  All other types have a distinct inverse.
    pub fn inverse(&self) -> Self {
        match self {
            Self::Addet => Self::Subet,
            Self::Subet => Self::Addet,
            Self::Prebn => Self::Prebn,
            Self::Prec => Self::Prec,
            Self::Fk45z => Self::Fk54z,
            Self::Fk54z => Self::Fk45z,
            Self::Amp => Self::Map,
            Self::Map => Self::Amp,
            Self::Ecleq => Self::Eqecl,
            Self::Eqecl => Self::Ecleq,
            Self::Galeq => Self::Eqgal,
            Self::Eqgal => Self::Galeq,
            Self::Galsup => Self::Supgal,
            Self::Supgal => Self::Galsup,
            Self::Hpceq => Self::Eqhpc,
            Self::Eqhpc => Self::Hpceq,
            Self::Hpreq => Self::Eqhpr,
            Self::Eqhpr => Self::Hpreq,
            Self::Hfk5z => Self::Fk5hz,
            Self::Fk5hz => Self::Hfk5z,
            Self::Heeq => Self::Eqhe,
            Self::Eqhe => Self::Heeq,
            Self::J2000h => Self::Hj2000,
            Self::Hj2000 => Self::J2000h,
            Self::Dh2e => Self::De2h,
            Self::De2h => Self::Dh2e,
            Self::R2h => Self::H2r,
            Self::H2r => Self::R2h,
        }
    }

    /// Construct from the integer code used in the C library.
    pub fn from_int(i: i32) -> Option<Self> {
        match i {
            1 => Some(Self::Addet),
            2 => Some(Self::Subet),
            3 => Some(Self::Prebn),
            4 => Some(Self::Prec),
            5 => Some(Self::Fk45z),
            6 => Some(Self::Fk54z),
            7 => Some(Self::Amp),
            8 => Some(Self::Map),
            9 => Some(Self::Ecleq),
            10 => Some(Self::Eqecl),
            11 => Some(Self::Galeq),
            12 => Some(Self::Eqgal),
            13 => Some(Self::Galsup),
            14 => Some(Self::Supgal),
            15 => Some(Self::Hpceq),
            16 => Some(Self::Eqhpc),
            17 => Some(Self::Hpreq),
            18 => Some(Self::Eqhpr),
            19 => Some(Self::Hfk5z),
            20 => Some(Self::Fk5hz),
            21 => Some(Self::Heeq),
            22 => Some(Self::Eqhe),
            23 => Some(Self::J2000h),
            24 => Some(Self::Hj2000),
            25 => Some(Self::Dh2e),
            26 => Some(Self::De2h),
            27 => Some(Self::R2h),
            28 => Some(Self::H2r),
            _ => None,
        }
    }

    /// Human-readable description for serialisation comments.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Addet => "Add E-terms of aberration",
            Self::Subet => "Remove E-terms of aberration",
            Self::Prebn => "Bessel-Newcomb (FK4) precession",
            Self::Prec => "IAU 1976 (FK5) precession",
            Self::Fk45z => "FK4 to FK5 (no PM)",
            Self::Fk54z => "FK5 to FK4 (no PM)",
            Self::Amp => "Geocentric apparent to mean place",
            Self::Map => "Mean place to geocentric apparent",
            Self::Ecleq => "Ecliptic to equatorial",
            Self::Eqecl => "Equatorial to ecliptic",
            Self::Galeq => "Galactic to equatorial",
            Self::Eqgal => "Equatorial to galactic",
            Self::Galsup => "Galactic to supergalactic",
            Self::Supgal => "Supergalactic to galactic",
            Self::Hpceq => "Helioprojective-Cartesian to J2000.0 equatorial (FK5)",
            Self::Eqhpc => "J2000.0 equatorial (FK5) to Helioprojective-Cartesian",
            Self::Hpreq => "Helioprojective-Radial to J2000.0 equatorial (FK5)",
            Self::Eqhpr => "J2000.0 equatorial (FK5) to Helioprojective-Radial",
            Self::Hfk5z => "ICRS to FK5 (no PM)",
            Self::Fk5hz => "FK5 to ICRS (no PM)",
            Self::Heeq => "Helio-ecliptic to equatorial",
            Self::Eqhe => "Equatorial to helio-ecliptic",
            Self::J2000h => "J2000 equatorial (dynamical) to ICRS",
            Self::Hj2000 => "ICRS to J2000 equatorial (dynamical)",
            Self::Dh2e => "Horizon to equatorial",
            Self::De2h => "Equatorial to horizon",
            Self::R2h => "RA to Hour Angle",
            Self::H2r => "Hour Angle to RA",
        }
    }
}

// ---------------------------------------------------------------------------
// SlaConversion — one step in the chain
// ---------------------------------------------------------------------------

/// A single conversion step in an [`SlaMap`] chain.
#[derive(Debug, Clone)]
struct SlaConversion {
    cvt_type: SlaCvtType,
    args: Vec<f64>,
}

// ---------------------------------------------------------------------------
// SlaMap
// ---------------------------------------------------------------------------

/// A mapping that chains celestial coordinate system conversions.
///
/// `SlaMap` applies a sequence of celestial-coordinate transformations
/// (precession, frame rotations, aberration, etc.) via the PAL library.
/// Each step is one of the [`SlaCvtType`] conversion types.
///
/// Input and output coordinates are (longitude, latitude) in radians.
///
/// # Example
///
/// ```
/// use ast_core::mappings::{Mapping, SlaMap, SlaCvtType};
/// use ast_core::coords::PointSet;
///
/// let mut sla = SlaMap::new();
/// sla.add(SlaCvtType::Eqgal, &[]);
///
/// // RA=0, Dec=0 in FK5 J2000 -> Galactic
/// let pts = PointSet::from_rows([[0.0_f64, 0.0_f64]]);
/// let out = sla.transform(&pts, true).unwrap();
/// // Should give galactic coords for the FK5 origin.
/// ```
#[derive(Debug, Clone)]
pub struct SlaMap {
    conversions: Vec<SlaConversion>,
    inverted: bool,
}

impl SlaMap {
    /// Create a new, empty `SlaMap` (identity transform).
    pub fn new() -> Self {
        Self {
            conversions: Vec::new(),
            inverted: false,
        }
    }

    /// Append a conversion step to the chain.
    ///
    /// `args` must have at least `cvt_type.nargs()` elements.
    /// Extra elements are silently ignored.
    pub fn add(&mut self, cvt_type: SlaCvtType, args: &[f64]) {
        self.conversions.push(SlaConversion {
            cvt_type,
            args: args.to_vec(),
        });
    }

    /// Number of conversion steps in the chain.
    pub fn len(&self) -> usize {
        self.conversions.len()
    }

    /// Whether the chain is empty.
    pub fn is_empty(&self) -> bool {
        self.conversions.is_empty()
    }

    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
    }

    /// Compose in series with another mapping.
    pub fn then(self, next: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(Box::new(self), next, Composition::Series)
    }

    /// Compose in parallel with another mapping.
    pub fn alongside(self, other: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(Box::new(self), other, Composition::Parallel)
    }
}

impl Default for SlaMap {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Step-level simplification helpers
// ---------------------------------------------------------------------------

/// Check whether two adjacent conversion steps cancel each other out.
///
/// Returns `true` if the pair should be eliminated (they are inverse
/// operations with matching arguments).
fn try_cancel_pair(a: &SlaConversion, b: &SlaConversion) -> bool {
    let at = a.cvt_type;
    let bt = b.cvt_type;

    // Check if the types form an inverse pair.
    let is_inverse_pair = (at.inverse() == bt) && (bt.inverse() == at) && (at != bt);

    if !is_inverse_pair {
        return false;
    }

    // For Amp/Map the argument order reverses between forward and inverse.
    if (at == SlaCvtType::Amp && bt == SlaCvtType::Map)
        || (at == SlaCvtType::Map && bt == SlaCvtType::Amp)
    {
        return a.args.len() >= 2
            && b.args.len() >= 2
            && ast_equal(a.args[0], b.args[1])
            && ast_equal(a.args[1], b.args[0]);
    }

    // For all other inverse pairs, all arguments must match in same order.
    a.args.len() == b.args.len() && a.args.iter().zip(&b.args).all(|(x, y)| ast_equal(*x, *y))
}

// ---------------------------------------------------------------------------
// Core transform logic
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Astronomical unit in metres.
const AU: f64 = 1.49597870e11;

/// Arcsecond to radians.
const AS2R: f64 = std::f64::consts::PI / 648000.0;

/// Degrees to radians.
const D2R: f64 = std::f64::consts::PI / 180.0;

// ---------------------------------------------------------------------------
// STP coordinate system helpers
// ---------------------------------------------------------------------------

/// Return the position of the Earth in HAEC (Heliocentric Aries Ecliptic
/// Cartesian) coordinates at the given MJD, in metres.
fn earth_haec(mjd: f64) -> [f64; 3] {
    // Get heliocentric position of the Earth in equatorial coords (AU)
    let (_dvb, _dpb, _dvh, dph) = pal::evp(mjd, 2000.0);

    // Rotate into ecliptic coordinates using the ecliptic matrix at J2000
    let ecm = pal::ecmat(pal::epj2d(2000.0));
    let earth = pal::dmxv(&ecm, &dph);

    // Convert from AU to metres
    [earth[0] * AU, earth[1] * AU, earth[2] * AU]
}

/// Return a unit vector along the solar north pole at the given date,
/// in HAEC coordinates.
fn solar_pole(mjd: f64) -> [f64; 3] {
    // Ecliptic longitude of the ascending node of the solar equator
    // on the ecliptic (degrees) — based on Explanatory Supplement.
    let omega = 75.76 + 0.01397 * (pal::epj(mjd) - 2000.0);

    // Convert to the ecliptic longitude of the projection of the sun's
    // north pole onto the ecliptic.
    let sproj = (omega - 90.0) * D2R;

    // The inclination of the solar axis to the ecliptic axis is 7.25 degrees.
    let inc = 7.25 * D2R;
    let t1 = inc.sin();
    [t1 * sproj.cos(), t1 * sproj.sin(), inc.cos()]
}

/// Compute the rotation matrix from GSEC (Geocentric Solar Ecliptic) to HAEC.
/// Also returns the offset (position of GSEC origin in HAEC).
fn gsec_to_haec(mjd: f64) -> ([[f64; 3]; 3], [f64; 3]) {
    let earth = earth_haec(mjd);

    // GSEC X axis: from earth toward sun = negative normalized earth position.
    let (xgs, _len) = pal::dvn(&earth);
    let xgs = [-xgs[0], -xgs[1], -xgs[2]];

    // GSEC Y axis: perpendicular to X and ecliptic north pole [0,0,1].
    let pole = [0.0, 0.0, 1.0];
    let ytemp = pal::dvxv(&pole, &xgs);
    let (ygs, _) = pal::dvn(&ytemp);

    // GSEC Z axis: completes right-handed system.
    let zgs = pal::dvxv(&xgs, &ygs);

    // Build rotation matrix (columns are the unit vectors).
    let mat = [
        [xgs[0], ygs[0], zgs[0]],
        [xgs[1], ygs[1], zgs[1]],
        [xgs[2], ygs[2], zgs[2]],
    ];

    (mat, earth)
}

/// Compute the rotation matrix from HAQC (Heliocentric Aries equatorial
/// Cartesian) to HAEC.  The origin is at the Sun in both systems.
fn haqc_to_haec(mjd: f64) -> ([[f64; 3]; 3], [f64; 3]) {
    let _ = mjd;
    // The HAQC-to-HAEC rotation is just the ecliptic matrix at J2000.
    let mat = pal::ecmat(pal::epj2d(2000.0));
    let offset = [0.0, 0.0, 0.0];
    (mat, offset)
}

/// Compute the rotation matrix from HPCC (Helioprojective-Cartesian Cartesian)
/// to HAEC.  Also returns the offset (observer position in HAEC).
fn hpcc_to_haec(mjd: f64, obs: Option<&[f64; 3]>) -> ([[f64; 3]; 3], [f64; 3]) {
    // If no observer position supplied, use Earth.
    let earth = earth_haec(mjd);
    let obs_pos = obs.unwrap_or(&earth);

    // HPCC X axis: from observer toward sun (negative normalized obs position).
    let (xhpc_unit, _) = pal::dvn(obs_pos);
    let xhpc = [-xhpc_unit[0], -xhpc_unit[1], -xhpc_unit[2]];

    // HPCC Y axis: perpendicular to X and solar north pole (left-handed!).
    let pole = solar_pole(mjd);
    let ytemp = pal::dvxv(&xhpc, &pole);
    let (yhpc, _) = pal::dvn(&ytemp);

    // HPCC Z axis: perpendicular to X and Y (left-handed system).
    let zhpc = pal::dvxv(&yhpc, &xhpc);

    // Build rotation matrix (columns are the unit vectors).
    let mat = [
        [xhpc[0], yhpc[0], zhpc[0]],
        [xhpc[1], yhpc[1], zhpc[1]],
        [xhpc[2], yhpc[2], zhpc[2]],
    ];

    (mat, *obs_pos)
}

/// Compute the rotation matrix from HPRC (Helioprojective-Radial Cartesian)
/// to HAEC.  Also returns the offset (observer position in HAEC).
fn hprc_to_haec(mjd: f64, obs: Option<&[f64; 3]>) -> ([[f64; 3]; 3], [f64; 3]) {
    // If no observer position supplied, use Earth.
    let earth = earth_haec(mjd);
    let obs_pos = obs.unwrap_or(&earth);

    // HPRC Z axis: from observer toward sun (negative normalized obs position).
    let (zhpr_unit, _) = pal::dvn(obs_pos);
    let zhpr = [-zhpr_unit[0], -zhpr_unit[1], -zhpr_unit[2]];

    // HPRC Y axis: perpendicular to Z and solar north pole (left-handed!).
    let pole = solar_pole(mjd);
    let ytemp = pal::dvxv(&pole, &zhpr);
    let (yhpr, _) = pal::dvn(&ytemp);

    // HPRC X axis: perpendicular to Y and Z (left-handed system).
    let xhpr = pal::dvxv(&zhpr, &yhpr);

    // Build rotation matrix (columns are the unit vectors).
    let mat = [
        [xhpr[0], yhpr[0], zhpr[0]],
        [xhpr[1], yhpr[1], zhpr[1]],
        [xhpr[2], yhpr[2], zhpr[2]],
    ];

    (mat, *obs_pos)
}

/// Perform the STP coordinate conversion for a single 3-vector.
///
/// Converts a 3D position between two STP coordinate systems,
/// ignoring origins (appropriate for points at infinity, i.e. direction
/// vectors only).
///
/// `in_sys` and `out_sys` identify the systems by enum variant:
/// - Hpceq/Eqhpc use HPCC/HAQC
/// - Hpreq/Eqhpr use HPRC/HAQC
/// - Heeq/Eqhe use GSEC/HAQC
fn stp_convert(
    mjd: f64,
    v: &[f64; 3],
    in_sys: SlaCvtType,
    obs: Option<&[f64; 3]>,
    forward: bool,
) -> [f64; 3] {
    // Determine the coordinate systems based on the conversion type and direction.
    // Forward: from the named "input" system to equatorial (HAQC).
    // The STP converts between a specific system and HAQC (equatorial).
    let (in_mat, in_off, out_mat, out_off) = if forward {
        let (im, io) = match in_sys {
            SlaCvtType::Hpceq => hpcc_to_haec(mjd, obs),
            SlaCvtType::Hpreq => hprc_to_haec(mjd, obs),
            SlaCvtType::Heeq => gsec_to_haec(mjd),
            SlaCvtType::Eqhpc => haqc_to_haec(mjd),
            SlaCvtType::Eqhpr => haqc_to_haec(mjd),
            SlaCvtType::Eqhe => haqc_to_haec(mjd),
            _ => unreachable!(),
        };
        let (om, oo) = match in_sys {
            SlaCvtType::Hpceq | SlaCvtType::Hpreq | SlaCvtType::Heeq => haqc_to_haec(mjd),
            SlaCvtType::Eqhpc => hpcc_to_haec(mjd, obs),
            SlaCvtType::Eqhpr => hprc_to_haec(mjd, obs),
            SlaCvtType::Eqhe => gsec_to_haec(mjd),
            _ => unreachable!(),
        };
        (im, io, om, oo)
    } else {
        // Inverse: swap the roles.
        let (om, oo) = match in_sys {
            SlaCvtType::Hpceq => hpcc_to_haec(mjd, obs),
            SlaCvtType::Hpreq => hprc_to_haec(mjd, obs),
            SlaCvtType::Heeq => gsec_to_haec(mjd),
            SlaCvtType::Eqhpc => haqc_to_haec(mjd),
            SlaCvtType::Eqhpr => haqc_to_haec(mjd),
            SlaCvtType::Eqhe => haqc_to_haec(mjd),
            _ => unreachable!(),
        };
        let (im, io) = match in_sys {
            SlaCvtType::Hpceq | SlaCvtType::Hpreq | SlaCvtType::Heeq => haqc_to_haec(mjd),
            SlaCvtType::Eqhpc => hpcc_to_haec(mjd, obs),
            SlaCvtType::Eqhpr => hprc_to_haec(mjd, obs),
            SlaCvtType::Eqhe => gsec_to_haec(mjd),
            _ => unreachable!(),
        };
        (im, io, om, oo)
    };

    // Convert input spherical to cartesian (using radius = 1 AU).
    let cart = pal::dcs2c(v[0], v[1]);
    let cart = [cart[0] * AU, cart[1] * AU, cart[2] * AU];

    // Compute the combined rotation matrix: transpose(out_mat) * in_mat.
    // Offset from output origin to input origin in HAEC.
    let mut mat3 = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            mat3[i][j] = out_mat[j][i]; // transpose of out_mat
        }
    }

    // Ignore origins (appropriate for distant sources).
    let _off3 = [
        in_off[0] - out_off[0],
        in_off[1] - out_off[1],
        in_off[2] - out_off[2],
    ];

    // Combined matrix: mat3 * in_mat
    let mat4 = pal::dmxm(&mat3, &in_mat);

    // Apply rotation (ignoring origin offset for infinite-distance sources).
    let q = pal::dmxv(&mat4, &cart);

    // Convert back to spherical.
    let (qn, _radius) = pal::dvn(&q);
    let (lng, lat) = pal::dcc2s(&qn);
    [pal::dranrm(lng), lat, 0.0]
}

/// Apply the AST De2h conversion: apparent (ha, dec) to topocentric (az, el)
/// including diurnal aberration correction.
fn de2h_with_aberration(ha: f64, dec: f64, phi: f64, diurab: f64) -> (f64, f64) {
    let sh = ha.sin();
    let ch = ha.cos();
    let sd = dec.sin();
    let cd = dec.cos();
    let sp = phi.sin();
    let cp = phi.cos();

    // Components of Cartesian (-ha, dec) vector.
    let xhd = ch * cd;
    let yhd = -sh * cd;
    let zhd = sd;

    // Modify for diurnal aberration.
    let f = 1.0 - diurab * yhd;
    let xhdt = f * xhd;
    let yhdt = f * (yhd + diurab);
    let zhdt = f * zhd;

    // Convert to Cartesian (az, el).
    let x = -xhdt * sp + zhdt * cp;
    let y = yhdt;
    let z = xhdt * cp + zhdt * sp;

    // Convert to spherical (az, el).
    let r = (x * x + y * y).sqrt();
    let mut a = if r == 0.0 { 0.0 } else { y.atan2(x) };
    while a < 0.0 {
        a += 2.0 * std::f64::consts::PI;
    }

    (a, z.atan2(r))
}

/// Apply the AST Dh2e conversion: topocentric (az, el) to apparent (ha, dec)
/// including diurnal aberration correction.
fn dh2e_with_aberration(az: f64, el: f64, phi: f64, diurab: f64) -> (f64, f64) {
    let sa = az.sin();
    let ca = az.cos();
    let se = el.sin();
    let ce = el.cos();
    let sp = phi.sin();
    let cp = phi.cos();

    // Cartesian (az, el) to Cartesian (ha, dec) — note, +ha not -ha.
    let xmhda = -ca * ce * sp + se * cp;
    let ymhda = -sa * ce;
    let zmhda = ca * ce * cp + se * sp;

    // Correct for diurnal aberration.
    let f = 1.0 - diurab * ymhda;
    let x = f * xmhda;
    let y = f * (ymhda + diurab);
    let z = f * zmhda;

    // Cartesian (ha, dec) to spherical (ha, dec).
    let r = (x * x + y * y).sqrt();
    let ha = if r == 0.0 { 0.0 } else { y.atan2(x) };
    let dec = z.atan2(r);

    (ha, dec)
}

/// Apply a single conversion step to a 3-vector, in the forward direction.
///
/// Returns the updated 3-vector.
fn apply_step_forward(cvt: &SlaConversion, v: [f64; 3]) -> [f64; 3] {
    match cvt.cvt_type {
        SlaCvtType::Galeq => {
            let (lon, lat) = pal::dcc2s(&v);
            let (ra, dec) = pal::galeq(lon, lat);
            pal::dcs2c(ra, dec)
        }
        SlaCvtType::Eqgal => {
            let (lon, lat) = pal::dcc2s(&v);
            let (gl, gb) = pal::eqgal(lon, lat);
            pal::dcs2c(gl, gb)
        }
        SlaCvtType::Galsup => {
            let (lon, lat) = pal::dcc2s(&v);
            let (sl, sb) = pal::galsup(lon, lat);
            pal::dcs2c(sl, sb)
        }
        SlaCvtType::Supgal => {
            let (lon, lat) = pal::dcc2s(&v);
            let (gl, gb) = pal::supgal(lon, lat);
            pal::dcs2c(gl, gb)
        }
        SlaCvtType::Prec => {
            let m = pal::prec(cvt.args[0], cvt.args[1]);
            pal::dmxv(&m, &v)
        }
        SlaCvtType::Prebn => {
            let m = pal::prebn(cvt.args[0], cvt.args[1]);
            pal::dmxv(&m, &v)
        }
        SlaCvtType::Fk45z => {
            let (lon, lat) = pal::dcc2s(&v);
            let (ra, dec) = pal::fk45z(lon, lat, cvt.args[0]);
            pal::dcs2c(ra, dec)
        }
        SlaCvtType::Fk54z => {
            let (lon, lat) = pal::dcc2s(&v);
            let (ra, dec, _, _) = pal::fk54z(lon, lat, cvt.args[0]);
            pal::dcs2c(ra, dec)
        }
        SlaCvtType::Fk5hz => {
            let (lon, lat) = pal::dcc2s(&v);
            let (rh, dh) = pal::fk5hz(lon, lat, cvt.args[0]);
            pal::dcs2c(rh, dh)
        }
        SlaCvtType::Hfk5z => {
            let (lon, lat) = pal::dcc2s(&v);
            let (r5, d5, _, _) = pal::hfk5z(lon, lat, cvt.args[0]);
            pal::dcs2c(r5, d5)
        }
        SlaCvtType::Ecleq => {
            let m = pal::ecmat(cvt.args[0]);
            // Ecliptic to equatorial = inverse (transpose) of the ecliptic matrix
            pal::dimxv(&m, &v)
        }
        SlaCvtType::Eqecl => {
            let m = pal::ecmat(cvt.args[0]);
            pal::dmxv(&m, &v)
        }
        SlaCvtType::Addet => {
            let (lon, lat) = pal::dcc2s(&v);
            let (rc, dc) = pal::addet(lon, lat, cvt.args[0]);
            pal::dcs2c(rc, dc)
        }
        SlaCvtType::Subet => {
            let (lon, lat) = pal::dcc2s(&v);
            let (rm, dm) = pal::subet(lon, lat, cvt.args[0]);
            pal::dcs2c(rm, dm)
        }
        SlaCvtType::Map => {
            let amprms = pal::mappa(cvt.args[0], cvt.args[1]);
            let (lon, lat) = pal::dcc2s(&v);
            let (ra, da) = pal::mapqkz(lon, lat, &amprms);
            pal::dcs2c(ra, da)
        }
        SlaCvtType::Amp => {
            let amprms = pal::mappa(cvt.args[0], cvt.args[1]);
            let (lon, lat) = pal::dcc2s(&v);
            let (rm, dm) = pal::ampqk(lon, lat, &amprms);
            pal::dcs2c(rm, dm)
        }
        SlaCvtType::De2h => {
            let (ha, dec) = pal::dcc2s(&v);
            let (az, el) = de2h_with_aberration(ha, dec, cvt.args[0], cvt.args[1]);
            pal::dcs2c(az, el)
        }
        SlaCvtType::Dh2e => {
            let (az, el) = pal::dcc2s(&v);
            let (ha, dec) = dh2e_with_aberration(az, el, cvt.args[0], cvt.args[1]);
            pal::dcs2c(ha, dec)
        }
        // R2H and H2R both perform: longitude' = LAST - longitude.
        // The operation is self-inverse (applying it twice returns to
        // the original coordinates), so forward and inverse are the
        // same for each type.  R2H and H2R are each other's inverse()
        // in the SlaCvtType sense but the computation is identical.
        SlaCvtType::R2h | SlaCvtType::H2r => {
            let (lon, lat) = pal::dcc2s(&v);
            let new_lon = cvt.args[0] - lon;
            pal::dcs2c(new_lon, lat)
        }
        // J2000H: dynamical J2000 to ICRS via frame-tie rotation.
        SlaCvtType::J2000h => {
            let rmat = pal::deuler("XYZ", -0.0068192 * AS2R, 0.0166172 * AS2R, 0.0146000 * AS2R);
            pal::dmxv(&rmat, &v)
        }
        // HJ2000: ICRS to dynamical J2000 (inverse of J2000H).
        SlaCvtType::Hj2000 => {
            let rmat = pal::deuler("XYZ", -0.0068192 * AS2R, 0.0166172 * AS2R, 0.0146000 * AS2R);
            pal::dimxv(&rmat, &v)
        }
        // STP coordinate conversions (solar coordinate systems).
        SlaCvtType::Hpceq | SlaCvtType::Hpreq | SlaCvtType::Heeq => {
            let (lon, lat) = pal::dcc2s(&v);
            let obs = if cvt.args.len() >= 4 {
                Some([cvt.args[1], cvt.args[2], cvt.args[3]])
            } else {
                None
            };
            let result = stp_convert(
                cvt.args[0],
                &[lon, lat, 0.0],
                cvt.cvt_type,
                obs.as_ref(),
                true,
            );
            pal::dcs2c(result[0], result[1])
        }
        SlaCvtType::Eqhpc | SlaCvtType::Eqhpr | SlaCvtType::Eqhe => {
            let (lon, lat) = pal::dcc2s(&v);
            let obs = if cvt.args.len() >= 4 {
                Some([cvt.args[1], cvt.args[2], cvt.args[3]])
            } else {
                None
            };
            let result = stp_convert(
                cvt.args[0],
                &[lon, lat, 0.0],
                cvt.cvt_type,
                obs.as_ref(),
                true,
            );
            pal::dcs2c(result[0], result[1])
        }
    }
}

/// Apply a single conversion step in the inverse direction.
///
/// For most types this calls the inverse type's forward function.
/// For Prec/Prebn, it swaps the epoch arguments.
/// For J2000H/HJ2000, uses the transpose directly.
/// For STP types, passes forward=false to the STP converter.
fn apply_step_inverse(cvt: &SlaConversion, v: [f64; 3]) -> [f64; 3] {
    match cvt.cvt_type {
        SlaCvtType::Prec => {
            // Inverse precession: swap epoch arguments
            let m = pal::prec(cvt.args[1], cvt.args[0]);
            pal::dmxv(&m, &v)
        }
        SlaCvtType::Prebn => {
            // Inverse Besselian precession: swap epoch arguments
            let m = pal::prebn(cvt.args[1], cvt.args[0]);
            pal::dmxv(&m, &v)
        }
        // J2000H inverse = transpose multiplication (same matrix, dimxv)
        SlaCvtType::J2000h => {
            let rmat = pal::deuler("XYZ", -0.0068192 * AS2R, 0.0166172 * AS2R, 0.0146000 * AS2R);
            pal::dimxv(&rmat, &v)
        }
        // HJ2000 inverse = forward multiplication (same matrix, dmxv)
        SlaCvtType::Hj2000 => {
            let rmat = pal::deuler("XYZ", -0.0068192 * AS2R, 0.0166172 * AS2R, 0.0146000 * AS2R);
            pal::dmxv(&rmat, &v)
        }
        // STP types: pass forward=false to the converter.
        SlaCvtType::Hpceq
        | SlaCvtType::Eqhpc
        | SlaCvtType::Hpreq
        | SlaCvtType::Eqhpr
        | SlaCvtType::Heeq
        | SlaCvtType::Eqhe => {
            let (lon, lat) = pal::dcc2s(&v);
            let obs = if cvt.args.len() >= 4 {
                Some([cvt.args[1], cvt.args[2], cvt.args[3]])
            } else {
                None
            };
            let result = stp_convert(
                cvt.args[0],
                &[lon, lat, 0.0],
                cvt.cvt_type,
                obs.as_ref(),
                false,
            );
            pal::dcs2c(result[0], result[1])
        }
        _ => {
            // For all other types, use the inverse type's forward operation
            let inv_cvt = SlaConversion {
                cvt_type: cvt.cvt_type.inverse(),
                args: cvt.args.clone(),
            };
            apply_step_forward(&inv_cvt, v)
        }
    }
}

// ---------------------------------------------------------------------------
// Mapping trait
// ---------------------------------------------------------------------------

impl Mapping for SlaMap {
    fn nin(&self) -> usize {
        2
    }

    fn nout(&self) -> usize {
        2
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let mut out = PointSet::zeros(pts.npoint(), 2);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);

        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);

            let lon = inp[0];
            let lat = inp[1];

            if is_bad(lon) || is_bad(lat) {
                outp[0] = BAD_VALUE;
                outp[1] = BAD_VALUE;
                continue;
            }

            // Convert to 3-vector
            let mut v = pal::dcs2c(lon, lat);

            if fwd {
                // Forward: iterate conversions in order
                for cvt in &self.conversions {
                    v = apply_step_forward(cvt, v);
                }
            } else {
                // Inverse: iterate conversions in REVERSE order, applying inverse
                for cvt in self.conversions.iter().rev() {
                    v = apply_step_inverse(cvt, v);
                }
            }

            // Convert back to spherical
            let (out_lon, out_lat) = pal::dcc2s(&v);
            outp[0] = out_lon;
            outp[1] = out_lat;
        }

        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        // Perform step-level simplification within the SlaMap:
        // 1. Eliminate redundant precession (same start and end epoch).
        // 2. Cancel adjacent inverse pairs with matching arguments.
        // 3. Combine adjacent precession steps with a common epoch.
        let mut steps: Vec<SlaConversion> = self.conversions.clone();

        let mut done = false;
        while !done {
            done = true;
            let mut new_steps: Vec<SlaConversion> = Vec::with_capacity(steps.len());
            let mut i = 0;
            while i < steps.len() {
                // Check for redundant precession (same start and end epoch).
                let ct = steps[i].cvt_type;
                if (ct == SlaCvtType::Prec || ct == SlaCvtType::Prebn)
                    && steps[i].args.len() >= 2
                    && ast_equal(steps[i].args[0], steps[i].args[1])
                {
                    // Skip this step entirely.
                    done = false;
                    i += 1;
                    continue;
                }

                // Check for adjacent pair cancellation.
                if i + 1 < steps.len() && try_cancel_pair(&steps[i], &steps[i + 1]) {
                    // Both steps cancel — skip them.
                    done = false;
                    i += 2;
                    continue;
                }

                // Check for adjacent precession combination.
                if i + 1 < steps.len() {
                    let ct_next = steps[i + 1].cvt_type;
                    if ct == ct_next
                        && (ct == SlaCvtType::Prec || ct == SlaCvtType::Prebn)
                        && steps[i].args.len() >= 2
                        && steps[i + 1].args.len() >= 2
                        && ast_equal(steps[i].args[1], steps[i + 1].args[0])
                    {
                        // Combine: keep second step with first step's start epoch.
                        let mut combined = steps[i + 1].clone();
                        combined.args[0] = steps[i].args[0];
                        new_steps.push(combined);
                        done = false;
                        i += 2;
                        continue;
                    }
                }

                new_steps.push(steps[i].clone());
                i += 1;
            }
            steps = new_steps;
        }

        if steps.is_empty() {
            Ok(Box::new(UnitMap::new(2)))
        } else if steps.len() == self.conversions.len() {
            // No simplification occurred.
            Ok(Box::new(self.clone()))
        } else {
            let mut result = SlaMap::new();
            for step in &steps {
                result.add(step.cvt_type, &step.args);
            }
            if self.inverted {
                result.invert();
            }
            Ok(Box::new(result))
        }
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }

    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        if !series {
            return None;
        }
        // Only handle inverse cancellation: same SlaMap in opposite directions.
        if self_inv == other_inv {
            return None;
        }
        let other_sla = other.as_any().downcast_ref::<SlaMap>()?;
        if self.conversions.len() == other_sla.conversions.len()
            && self
                .conversions
                .iter()
                .zip(&other_sla.conversions)
                .all(|(a, b)| {
                    a.cvt_type == b.cvt_type
                        && a.args.len() == b.args.len()
                        && a.args.iter().zip(&b.args).all(|(x, y)| ast_equal(*x, *y))
                })
        {
            // SlaMap always maps 2 coords (lon, lat) to 2 coords.
            Some(Box::new(UnitMap::new(2)))
        } else {
            None
        }
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<SlaMap>() else {
            return false;
        };
        if self.inverted != other.inverted {
            return false;
        }
        if self.conversions.len() != other.conversions.len() {
            return false;
        }
        self.conversions
            .iter()
            .zip(&other.conversions)
            .all(|(a, b)| {
                a.cvt_type == b.cvt_type
                    && a.args.len() == b.args.len()
                    && a.args.iter().zip(&b.args).all(|(x, y)| ast_equal(*x, *y))
            })
    }
}

// ---------------------------------------------------------------------------
// Serialisation
// ---------------------------------------------------------------------------

/// Argument index to letter: 0 -> 'a', 1 -> 'b', etc.
///
/// The native serialisation format uses lowercase letters for argument
/// suffixes (e.g. `Sla1a`, `Sla1b`).
fn arg_letter(j: usize) -> char {
    (b'a' + j as u8) as char
}

impl Serializable for SlaMap {
    fn class_name(&self) -> &'static str {
        "SlaMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Sequence of celestial coordinate conversions".to_string());

        // Mapping base attributes.
        write_mapping_base(&mut node, self);

        let nsla = self.conversions.len();
        if nsla > 0 {
            node.push_int(
                "Nsla",
                nsla as i64,
                Some("Number of conversion steps".to_string()),
            );

            for (i, cvt) in self.conversions.iter().enumerate() {
                let idx = i + 1; // 1-based
                node.push_string(
                    &format!("Sla{idx}"),
                    cvt.cvt_type.code_string(),
                    Some(cvt.cvt_type.description().to_string()),
                );
                for (j, &arg) in cvt.args.iter().enumerate() {
                    let letter = arg_letter(j);
                    node.push_double(&format!("Sla{idx}{letter}"), arg, None::<String>);
                }
            }
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let (_nin, _nout, inverted) = read_mapping_base(node);

        let nsla = node.get_int("Nsla", 0) as usize;

        let mut sla = SlaMap::new();

        for i in 0..nsla {
            let idx = i + 1;
            let code_str = node.get_string(&format!("Sla{idx}"), "");
            let cvt_type = SlaCvtType::from_code(&code_str).ok_or_else(|| {
                AstError::new(
                    ErrorCode::Slain,
                    format!("unknown SlaMap conversion type: '{code_str}'"),
                )
            })?;

            let nargs = cvt_type.nargs();
            let mut args = Vec::with_capacity(nargs);
            for j in 0..nargs {
                let letter = arg_letter(j);
                let val = node.get_double(&format!("Sla{idx}{letter}"), 0.0);
                args.push(val);
            }

            sla.add(cvt_type, &args);
        }

        if inverted {
            sla.invert();
        }

        Ok(sla)
    }
}
