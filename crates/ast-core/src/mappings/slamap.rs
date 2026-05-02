//! Mapping that chains celestial coordinate system conversions via PAL.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::pal;
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::is_bad;
use super::rate::numeric_rate;
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
    /// ICRS (Hipparcos) to FK5.
    Hfk5z = 19,
    /// FK5 to ICRS (Hipparcos).
    Fk5hz = 20,
    /// Horizon (az/el) to equatorial (ha/dec).
    Dh2e = 25,
    /// Equatorial (ha/dec) to horizon (az/el).
    De2h = 26,
}

impl SlaCvtType {
    /// Number of floating-point arguments this conversion requires.
    pub fn nargs(&self) -> usize {
        match self {
            Self::Galeq | Self::Eqgal | Self::Galsup | Self::Supgal => 0,
            Self::Addet
            | Self::Subet
            | Self::Fk45z
            | Self::Fk54z
            | Self::Ecleq
            | Self::Eqecl
            | Self::Hfk5z
            | Self::Fk5hz
            | Self::Dh2e
            | Self::De2h => 1,
            Self::Prebn | Self::Prec | Self::Amp | Self::Map => 2,
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
            Self::Hfk5z => "HFK5Z",
            Self::Fk5hz => "FK5HZ",
            Self::Dh2e => "DH2E",
            Self::De2h => "DE2H",
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
            "HFK5Z" => Some(Self::Hfk5z),
            "FK5HZ" => Some(Self::Fk5hz),
            "DH2E" => Some(Self::Dh2e),
            "DE2H" => Some(Self::De2h),
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
            Self::Hfk5z => Self::Fk5hz,
            Self::Fk5hz => Self::Hfk5z,
            Self::Dh2e => Self::De2h,
            Self::De2h => Self::Dh2e,
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
            19 => Some(Self::Hfk5z),
            20 => Some(Self::Fk5hz),
            25 => Some(Self::Dh2e),
            26 => Some(Self::De2h),
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
            Self::Hfk5z => "ICRS to FK5 (no PM)",
            Self::Fk5hz => "FK5 to ICRS (no PM)",
            Self::Dh2e => "Horizon to equatorial",
            Self::De2h => "Equatorial to horizon",
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
// Core transform logic
// ---------------------------------------------------------------------------

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
            let (lon, lat) = pal::dcc2s(&v);
            let (az, el) = pal::de2h(lon, lat, cvt.args[0]);
            pal::dcs2c(az, el)
        }
        SlaCvtType::Dh2e => {
            let (lon, lat) = pal::dcc2s(&v);
            let (ha, dec) = pal::dh2e(lon, lat, cvt.args[0]);
            pal::dcs2c(ha, dec)
        }
    }
}

/// Apply a single conversion step in the inverse direction.
///
/// For most types this calls the inverse type's forward function.
/// For Prec/Prebn, it swaps the epoch arguments.
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
        Ok(Box::new(self.clone()))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        numeric_rate(self, at, ax1, ax2)
    }
}

// ---------------------------------------------------------------------------
// Serialisation
// ---------------------------------------------------------------------------

/// Argument index to letter: 0 -> 'A', 1 -> 'B', etc.
fn arg_letter(j: usize) -> char {
    (b'A' + j as u8) as char
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
