//! Mapping that chains celestial coordinate system conversions via PAL.

use crate::constants::BAD_VALUE;
use crate::coords::{Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::pal;
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad};
use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

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

    /// Parse a code string into a conversion type, as slamap.c `CvtCode`
    /// does: case-insensitive and ignoring trailing white space.
    pub fn from_code(s: &str) -> Option<Self> {
        // C's CvtCode matches with astChrMatch.
        match crate::mappings::helpers::chr_match_key(s).as_str() {
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
            "H2E" => Some(Self::Dh2e),
            "E2H" => Some(Self::De2h),
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

    /// Human-readable descriptions for each serialised argument.
    ///
    /// The slice length matches `nargs()`. Used as comments in the Native
    /// serialisation format.
    pub fn arg_descriptions(&self) -> &'static [&'static str] {
        match self {
            Self::Prec => &["From Julian epoch", "To Julian epoch"],
            Self::Prebn => &["From Besselian epoch", "To Besselian epoch"],
            Self::Fk45z => &["Besselian epoch of FK4 coordinates"],
            Self::Fk54z => &["Besselian epoch of FK4 system"],
            Self::Amp => &[
                "TDB of apparent place (as MJD)",
                "Julian epoch of mean equinox (FK5)",
            ],
            Self::Map => &[
                "Julian epoch of mean equinox (FK5)",
                "TDB of apparent place (as MJD)",
            ],
            Self::Ecleq | Self::Eqecl => &["TDB of mean ecliptic (as MJD)"],
            Self::Addet | Self::Subet => &["Besselian epoch of mean equinox (FK4)"],
            Self::Hpceq | Self::Eqhpc | Self::Hpreq | Self::Eqhpr => &[
                "Modified Julian Date of observation",
                "Heliocentric-Aries-Ecliptic X value at observer",
                "Heliocentric-Aries-Ecliptic Y value at observer",
                "Heliocentric-Aries-Ecliptic Z value at observer",
            ],
            Self::Hfk5z | Self::Fk5hz => &["Julian epoch of FK5 coordinates"],
            Self::Heeq | Self::Eqhe => &["Modified Julian Date of observation"],
            Self::Dh2e | Self::De2h => &[
                "Geodetic latitude of observer",
                "Magnitude of diurnal aberration vector",
            ],
            Self::R2h | Self::H2r => &["Local apparent sidereal time (radians)"],
            _ => &[],
        }
    }

    /// Human-readable description for serialisation comments.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Addet => "Add E-terms of aberration",
            Self::Subet => "Subtract E-terms of aberration",
            Self::Prebn => "Apply Bessel-Newcomb (FK4) precession",
            Self::Prec => "Apply IAU 1975 (FK5) precession",
            Self::Fk45z => "FK4 to FK5 J2000.0 (no PM or parallax)",
            Self::Fk54z => "FK5 J2000.0 to FK4 (no PM or parallax)",
            Self::Amp => "Geocentric apparent to mean place (FK5)",
            Self::Map => "Mean place (FK5) to geocentric apparent",
            Self::Ecleq => "Ecliptic (IAU 1980) to J2000.0 equatorial (FK5)",
            Self::Eqecl => "Equatorial J2000.0 (FK5) to ecliptic (IAU 1980)",
            Self::Galeq => "Galactic (IAU 1958) to J2000.0 equatorial (FK5)",
            Self::Eqgal => "J2000.0 equatorial (FK5) to galactic (IAU 1958)",
            Self::Galsup => "Galactic (IAU 1958) to supergalactic",
            Self::Supgal => "Supergalactic to galactic (IAU 1958)",
            Self::Hpceq => "Helioprojective-Cartesian to J2000.0 equatorial (FK5)",
            Self::Eqhpc => "J2000.0 equatorial (FK5) to Helioprojective-Cartesian",
            Self::Hpreq => "Helioprojective-Radial to J2000.0 equatorial (FK5)",
            Self::Eqhpr => "J2000.0 equatorial (FK5) to Helioprojective-Radial",
            Self::Hfk5z => "ICRS to J2000.0 FK5 (no PM or parallax)",
            Self::Fk5hz => "J2000.0 FK5 to ICRS (no PM or parallax)",
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
/// sla.add(SlaCvtType::Eqgal, &[]).unwrap();
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
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
    /// C's `cvtextra`: the `palMappa` parameters of each MAP or AMP step,
    /// worked out on first use and kept with the SlaMap (slamap.c:3805-3825).
    /// A clone keeps them, as C's Copy copies `cvtextra`.
    extra: std::cell::RefCell<Vec<Option<[f64; 21]>>>,
}

impl SlaMap {
    /// Create a new, empty `SlaMap` (identity transform).
    pub fn new() -> Self {
        Self {
            conversions: Vec::new(),
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
            extra: Default::default(),
        }
    }

    /// The constants for applying conversion `ci` in the given direction,
    /// with a MAP or AMP step's parameters taken from `extra` once known.
    fn step_consts(&self, ci: usize, cvt: &SlaConversion, fwd: bool) -> StepConsts {
        let mut extra = self.extra.borrow_mut();
        if extra.len() != self.conversions.len() {
            *extra = vec![None; self.conversions.len()];
        }
        let step = StepConsts::new(cvt, fwd, extra[ci]);
        if extra[ci].is_none() {
            extra[ci] = step.amprms;
        }
        step
    }

    /// Append a conversion step to the chain, as C's `astSlaAdd` does.
    ///
    /// `args` must hold exactly `cvt_type.nargs()` values; any other number is
    /// an `AST__TIMIN` error (slamap.c AddSlaCvt).
    pub fn add(&mut self, cvt_type: SlaCvtType, args: &[f64]) -> AstResult<()> {
        if args.len() != cvt_type.nargs() {
            return Err(AstError::new(
                ErrorCode::Timin,
                format!(
                    "SlaAdd: invalid number of arguments for SLALIB sky coordinate conversion \
                     {}: {} supplied, {} required",
                    cvt_type.code_string(),
                    args.len(),
                    cvt_type.nargs()
                ),
            ));
        }
        self.add_conversion(cvt_type, args);
        Ok(())
    }

    /// Append a conversion step whose arguments the caller has already
    /// checked, such as one rebuilt from a stored conversion: arguments beyond
    /// `cvt_type.nargs()` are ignored.
    pub(crate) fn add_conversion(&mut self, cvt_type: SlaCvtType, args: &[f64]) {
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

    // For Prec/Prebn: the inverse is itself with swapped epoch arguments.
    // So Prec(e1,e2) followed by Prec(e2,e1) cancels.
    if matches!(at, SlaCvtType::Prec | SlaCvtType::Prebn) && at == bt {
        return a.args.len() >= 2
            && b.args.len() >= 2
            && ast_equal(a.args[0], b.args[1])
            && ast_equal(a.args[1], b.args[0]);
    }

    // Check if the types form an inverse pair (different types).
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

/// Return the normalized step list for a SlaMap, accounting for inversion.
///
/// When inverted, reverses the step order and inverts each step type.
/// Four conversions also exchange their first two arguments: `Prec` and
/// `Prebn` swap the starting and ending epochs, and `Amp` and `Map` swap the
/// date and equinox, which they take in opposite orders.
fn sla_normalized_steps(sla: &SlaMap, inv: bool) -> Vec<SlaConversion> {
    if !inv {
        return sla.conversions.clone();
    }
    sla.conversions
        .iter()
        .rev()
        .map(|cvt| {
            let inv_type = cvt.cvt_type.inverse();
            let args = if matches!(
                cvt.cvt_type,
                SlaCvtType::Prec | SlaCvtType::Prebn | SlaCvtType::Amp | SlaCvtType::Map
            ) {
                let mut a = cvt.args.clone();
                if a.len() >= 2 {
                    a.swap(0, 1);
                }
                a
            } else {
                cvt.args.clone()
            };
            SlaConversion {
                cvt_type: inv_type,
                args,
            }
        })
        .collect()
}

/// Simplify a step list by cancelling adjacent inverse pairs and combining
/// adjacent precession steps.
///
/// Loops until no further simplifications are found.
/// Returns `true` if any steps were eliminated or combined.
fn sla_simplify_steps(steps: &mut Vec<SlaConversion>) -> bool {
    let mut any_changed = false;
    loop {
        let mut changed_this_pass = false;
        let mut i = 0;
        while i + 1 < steps.len() {
            // Check for inverse pair cancellation.
            if try_cancel_pair(&steps[i], &steps[i + 1]) {
                steps.remove(i + 1);
                steps.remove(i);
                changed_this_pass = true;
                any_changed = true;
                // Don't advance — new neighbours may cancel.
                i = i.saturating_sub(1);
                continue;
            }

            // Precession combination: Prec(e1,e2) + Prec(e2,e3) -> Prec(e1,e3)
            let combine_prec = {
                let a = &steps[i];
                let b = &steps[i + 1];
                a.cvt_type == b.cvt_type
                    && matches!(a.cvt_type, SlaCvtType::Prec | SlaCvtType::Prebn)
                    && a.args.len() >= 2
                    && b.args.len() >= 2
                    && ast_equal(a.args[1], b.args[0])
            };
            if combine_prec {
                let e1 = steps[i].args[0];
                let e3 = steps[i + 1].args[1];
                if ast_equal(e1, e3) {
                    // Prec(e,e) is identity — remove both.
                    steps.remove(i + 1);
                    steps.remove(i);
                } else {
                    steps[i].args[1] = e3;
                    steps.remove(i + 1);
                }
                changed_this_pass = true;
                any_changed = true;
                i = i.saturating_sub(1);
                continue;
            }

            i += 1;
        }

        // Also remove identity precession steps: Prec(e,e).
        let mut j = 0;
        while j < steps.len() {
            let ct = steps[j].cvt_type;
            if matches!(ct, SlaCvtType::Prec | SlaCvtType::Prebn)
                && steps[j].args.len() >= 2
                && ast_equal(steps[j].args[0], steps[j].args[1])
            {
                steps.remove(j);
                changed_this_pass = true;
                any_changed = true;
            } else {
                j += 1;
            }
        }

        if !changed_this_pass {
            break;
        }
    }
    any_changed
}

/// `map_merge` for a nominated `SlaMap`, mirroring C `slamap.c::MapMerge`.
/// Series only.
///
/// Gathers the run of adjacent `SlaMap`s at `where_`, concatenates their
/// invert-resolved step lists, cancels adjacent inverse pairs (and combines
/// precessions), and — per C's `simpler` condition — replaces the run with a
/// single **forward** `SlaMap` (invert cleared) whenever a lone SlaMap was
/// applied inverted, even if no cancellation occurred.
pub(crate) fn sla_map_merge(
    ctx: &mut super::simplify::MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    if !series {
        return super::simplify::default_map_merge(ctx, where_, series);
    }

    let is_sla = |entry: &(DynMapping, bool, bool)| entry.0.as_any().is::<SlaMap>();

    let n = ctx.len();
    let mut imap1 = where_;
    while imap1 >= 1 && is_sla(ctx.entry(imap1 - 1)) {
        imap1 -= 1;
    }
    let mut imap2 = where_;
    while imap2 + 1 < n && is_sla(ctx.entry(imap2 + 1)) {
        imap2 += 1;
    }

    let mut steps: Vec<SlaConversion> = Vec::new();
    let mut nstep0 = 0usize;
    let where_inv = ctx.entry(where_).1;
    for idx in imap1..=imap2 {
        let entry = ctx.entry(idx);
        let sm = entry.0.as_any().downcast_ref::<SlaMap>()?;
        let eff_inv = entry.1 != sm.inverted;
        let s = sla_normalized_steps(sm, eff_inv);
        nstep0 += s.len();
        steps.extend(s);
    }

    let ngone = imap2 - imap1;
    sla_simplify_steps(&mut steps);
    let unit = steps.is_empty();
    let simpler = ngone > 0 || steps.len() < nstep0 || unit || where_inv;
    if !simpler {
        return super::simplify::default_map_merge(ctx, where_, series);
    }

    let new: DynMapping = if unit {
        Box::new(UnitMap::new(2))
    } else {
        let mut result = SlaMap::new();
        for cvt in steps {
            result.add_conversion(cvt.cvt_type, &cvt.args);
        }
        Box::new(result)
    };
    ctx.replace_range_with(imap1, imap2, new);
    Some(imap1)
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
    let t1 = crate::math::sin(inc);
    [
        t1 * crate::math::cos(sproj),
        t1 * crate::math::sin(sproj),
        crate::math::cos(inc),
    ]
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
    // If no observer position supplied, use Earth (computed only then, as
    // C's Hpcc and Hprc do).
    let earth;
    let obs_pos = match obs {
        Some(o) => o,
        None => {
            earth = earth_haec(mjd);
            &earth
        }
    };

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
    // If no observer position supplied, use Earth (computed only then, as
    // C's Hpcc and Hprc do).
    let earth;
    let obs_pos = match obs {
        Some(o) => o,
        None => {
            earth = earth_haec(mjd);
            &earth
        }
    };

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

/// The matrix rotating STP direction vectors for one conversion step: C's
/// `mat4` in `STPConv` (slamap.c:2096), which C builds once per call before
/// its point loops. Origins are ignored, as C's SlaMap does for points at
/// infinity (slamap.c:4133).
///
/// `in_sys` and `out_sys` identify the systems by enum variant:
/// - Hpceq/Eqhpc use HPCC/HAQC
/// - Hpreq/Eqhpr use HPRC/HAQC
/// - Heeq/Eqhe use GSEC/HAQC
fn stp_matrix(
    mjd: f64,
    in_sys: SlaCvtType,
    obs: Option<&[f64; 3]>,
    forward: bool,
) -> [[f64; 3]; 3] {
    // Determine the coordinate systems based on the conversion type and direction.
    // Forward: from the named "input" system to equatorial (HAQC).
    // The STP converts between a specific system and HAQC (equatorial).
    let (in_mat, _in_off, out_mat, _out_off) = if forward {
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

    // Compute the combined rotation matrix: transpose(out_mat) * in_mat.
    // Offset from output origin to input origin in HAEC.
    let mut mat3 = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            mat3[i][j] = out_mat[j][i]; // transpose of out_mat
        }
    }

    // Combined matrix: mat3 * in_mat
    pal::dmxm(&mat3, &in_mat)
}

/// Apply an STP rotation (`stp_matrix`) to one direction, as C's
/// `STPConv` point loops do: to Cartesian at a radius of 1 AU, rotate, then
/// back to spherical with the longitude in `[0, 2*pi)` (slamap.c:2240-2430).
fn stp_apply(mat4: &[[f64; 3]; 3], alpha: f64, delta: f64) -> (f64, f64) {
    let cart = pal::dcs2c(alpha, delta);
    let cart = [cart[0] * AU, cart[1] * AU, cart[2] * AU];
    let q = pal::dmxv(mat4, &cart);
    let (qn, _radius) = pal::dvn(&q);
    let (lng, lat) = pal::dcc2s(&qn);
    (pal::dranrm(lng), lat)
}

/// Apply the AST De2h conversion: apparent (ha, dec) to topocentric (az, el)
/// including diurnal aberration correction.
fn de2h_with_aberration(ha: f64, dec: f64, phi: f64, diurab: f64) -> (f64, f64) {
    let sh = crate::math::sin(ha);
    let ch = crate::math::cos(ha);
    let sd = crate::math::sin(dec);
    let cd = crate::math::cos(dec);
    let sp = crate::math::sin(phi);
    let cp = crate::math::cos(phi);

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
    let r = crate::math::sqrt(x * x + y * y);
    let mut a = if r == 0.0 {
        0.0
    } else {
        crate::math::atan2(y, x)
    };
    while a < 0.0 {
        a += 2.0 * std::f64::consts::PI;
    }

    (a, crate::math::atan2(z, r))
}

/// Apply the AST Dh2e conversion: topocentric (az, el) to apparent (ha, dec)
/// including diurnal aberration correction.
fn dh2e_with_aberration(az: f64, el: f64, phi: f64, diurab: f64) -> (f64, f64) {
    let sa = crate::math::sin(az);
    let ca = crate::math::cos(az);
    let se = crate::math::sin(el);
    let ce = crate::math::cos(el);
    let sp = crate::math::sin(phi);
    let cp = crate::math::cos(phi);

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
    let r = crate::math::sqrt(x * x + y * y);
    let ha = if r == 0.0 {
        0.0
    } else {
        crate::math::atan2(y, x)
    };
    let dec = crate::math::atan2(z, r);

    (ha, dec)
}

/// Frame-tie rotation matrix from dynamical J2000.0 to ICRS (slamap.c:1855).
fn j2000h_matrix() -> [[f64; 3]; 3] {
    pal::deuler("XYZ", -0.0068192 * AS2R, 0.0166172 * AS2R, 0.0146000 * AS2R)
}

/// The point-independent values one conversion step needs, built once per
/// transform as C's `Transform` builds them before its point loop
/// (slamap.c:3510): the rotation matrix of a matrix conversion, and the
/// mean-to-apparent parameters of a MAP or AMP step.
struct StepConsts {
    matrix: Option<[[f64; 3]; 3]>,
    amprms: Option<[f64; 21]>,
}

impl StepConsts {
    /// The constants for applying `cvt` in the given direction; `extra` is
    /// a MAP or AMP step's stored `palMappa` parameters, if already known.
    fn new(cvt: &SlaConversion, fwd: bool, extra: Option<[f64; 21]>) -> Self {
        let a = &cvt.args;
        let matrix = match cvt.cvt_type {
            // The inverse precesses with the epochs swapped.
            SlaCvtType::Prec => Some(if fwd {
                pal::prec(a[0], a[1])
            } else {
                pal::prec(a[1], a[0])
            }),
            SlaCvtType::Prebn => Some(if fwd {
                pal::prebn(a[0], a[1])
            } else {
                pal::prebn(a[1], a[0])
            }),
            // Precess J2000 equatorial to the date, then rotate to ecliptic
            // (slamap.c:3888); ECLEQ applies the inverse of the same matrix.
            SlaCvtType::Ecleq | SlaCvtType::Eqecl => {
                let precess = pal::prec(2000.0, pal::epj(a[0]));
                let rotate = pal::ecmat(a[0]);
                Some(pal::dmxm(&rotate, &precess))
            }
            SlaCvtType::J2000h | SlaCvtType::Hj2000 => Some(j2000h_matrix()),
            SlaCvtType::Hpceq
            | SlaCvtType::Hpreq
            | SlaCvtType::Heeq
            | SlaCvtType::Eqhpc
            | SlaCvtType::Eqhpr
            | SlaCvtType::Eqhe => {
                // The observer position, for the conversions that take one.
                let obs = (a.len() >= 4).then(|| [a[1], a[2], a[3]]);
                Some(stp_matrix(a[0], cvt.cvt_type, obs.as_ref(), fwd))
            }
            _ => None,
        };
        let amprms = extra.or_else(|| match cvt.cvt_type {
            // MAP args are (EQ, DATE): palMappa(eq=args[0], ep=args[1]).
            SlaCvtType::Map => Some(mappa_cached(a[0], a[1])),
            // AMP args are (DATE, EQ): palMappa(eq=args[1], ep=args[0])
            // (slamap.c:3815). MAP and AMP order their args oppositely.
            SlaCvtType::Amp => Some(mappa_cached(a[1], a[0])),
            _ => None,
        });
        StepConsts { matrix, amprms }
    }

    fn matrix(&self) -> &[[f64; 3]; 3] {
        self.matrix
            .as_ref()
            .expect("a matrix conversion has its matrix")
    }

    fn amprms(&self) -> &[f64; 21] {
        self.amprms
            .as_ref()
            .expect("a MAP or AMP conversion has its parameters")
    }
}

thread_local! {
    /// C's `eq_cache`, `ep_cache` and `amprms_cache` (slamap.c:248-250, a
    /// per-thread global under `astGLOBAL`): the last `palMappa` arguments
    /// and result, reused while the arguments repeat.
    static MAPPA_CACHE: std::cell::Cell<(f64, f64, [f64; 21])> =
        const { std::cell::Cell::new((BAD_VALUE, BAD_VALUE, [0.0; 21])) };
}

/// `palMappa(eq, ep)`, reusing the previous result when the arguments are
/// the same, as C's MAP and AMP arms do (slamap.c:3815-3820).
fn mappa_cached(eq: f64, ep: f64) -> [f64; 21] {
    MAPPA_CACHE.with(|c| {
        let (ceq, cep, amprms) = c.get();
        if eq == ceq && ep == cep {
            return amprms;
        }
        let amprms = pal::mappa(eq, ep);
        c.set((eq, ep, amprms));
        amprms
    })
}

/// Apply a single conversion step to a scalar `(alpha, delta)` pair, in the
/// forward direction.
///
/// Mirrors the forward arms of `slamap.c` `Transform`, carrying scalar
/// longitude/latitude exactly as C does.  The four matrix conversions
/// (`Prec`, `Prebn`, `Ecleq`, `Eqecl`) re-normalize the longitude into
/// `[0, 2*pi)` with `palDranrm` after the round-trip; the other arms do not.
fn apply_step_forward(
    cvt_type: SlaCvtType,
    args: &[f64],
    alpha: f64,
    delta: f64,
    step: &StepConsts,
) -> (f64, f64) {
    match cvt_type {
        SlaCvtType::Galeq => pal::galeq(alpha, delta),
        SlaCvtType::Eqgal => pal::eqgal(alpha, delta),
        SlaCvtType::Galsup => pal::galsup(alpha, delta),
        SlaCvtType::Supgal => pal::supgal(alpha, delta),
        // Precession: convert to Cartesian, apply the precession matrix,
        // convert back to polar, then constrain longitude to [0, 2*pi)
        // (palDcc2s does not; slamap.c:3762).
        SlaCvtType::Prec | SlaCvtType::Prebn => {
            let v = pal::dcs2c(alpha, delta);
            let (a, d) = pal::dcc2s(&pal::dmxv(step.matrix(), &v));
            (pal::dranrm(a), d)
        }
        SlaCvtType::Fk45z => pal::fk45z(alpha, delta, args[0]),
        SlaCvtType::Fk54z => {
            let (ra, dec, _, _) = pal::fk54z(alpha, delta, args[0]);
            (ra, dec)
        }
        SlaCvtType::Fk5hz => pal::fk5hz(alpha, delta, args[0]),
        SlaCvtType::Hfk5z => {
            let (r5, d5, _, _) = pal::hfk5z(alpha, delta, args[0]);
            (r5, d5)
        }
        // Ecliptic (of date) to equatorial J2000.0.  C builds the combined
        // matrix precessing J2000 equatorial to the date then rotating to
        // ecliptic (slamap.c:3888); ECLEQ applies its inverse (palDimxv).
        // Longitude is wrapped into [0, 2*pi) afterwards (slamap.c:3906).
        SlaCvtType::Ecleq => {
            let v = pal::dcs2c(alpha, delta);
            let (a, d) = pal::dcc2s(&pal::dimxv(step.matrix(), &v));
            (pal::dranrm(a), d)
        }
        // Equatorial J2000.0 to ecliptic (of date).  Forward applies the
        // matrix directly (palDmxv) (slamap.c:3941).
        SlaCvtType::Eqecl => {
            let v = pal::dcs2c(alpha, delta);
            let (a, d) = pal::dcc2s(&pal::dmxv(step.matrix(), &v));
            (pal::dranrm(a), d)
        }
        SlaCvtType::Addet => pal::addet(alpha, delta, args[0]),
        SlaCvtType::Subet => pal::subet(alpha, delta, args[0]),
        SlaCvtType::Map => pal::mapqkz(alpha, delta, step.amprms()),
        SlaCvtType::Amp => pal::ampqk(alpha, delta, step.amprms()),
        SlaCvtType::De2h => de2h_with_aberration(alpha, delta, args[0], args[1]),
        SlaCvtType::Dh2e => dh2e_with_aberration(alpha, delta, args[0], args[1]),
        // R2H and H2R are identical scalar operations (self-inverse): the
        // longitude is reflected about the local sidereal time, un-normalized
        // (slamap.c:4206).
        SlaCvtType::R2h | SlaCvtType::H2r => (args[0] - alpha, delta),
        // J2000H: dynamical J2000 to ICRS via frame-tie rotation.  The C
        // J2000H helper does not normalize the longitude (slamap.c:1872).
        SlaCvtType::J2000h => {
            let v = pal::dcs2c(alpha, delta);
            pal::dcc2s(&pal::dmxv(step.matrix(), &v))
        }
        // HJ2000: ICRS to dynamical J2000 (inverse of J2000H).
        SlaCvtType::Hj2000 => {
            let v = pal::dcs2c(alpha, delta);
            pal::dcc2s(&pal::dimxv(step.matrix(), &v))
        }
        // STP coordinate conversions (solar coordinate systems).
        SlaCvtType::Hpceq
        | SlaCvtType::Hpreq
        | SlaCvtType::Heeq
        | SlaCvtType::Eqhpc
        | SlaCvtType::Eqhpr
        | SlaCvtType::Eqhe => stp_apply(step.matrix(), alpha, delta),
    }
}

/// Apply a single conversion step to a scalar `(alpha, delta)` pair in the
/// inverse direction.
///
/// For most types this calls the inverse type's forward function.
/// For Prec/Prebn, it swaps the epoch arguments.
/// For J2000H/HJ2000, uses the transpose directly.
/// For STP types, applies the inverse rotation `StepConsts` built.
fn apply_step_inverse(
    cvt_type: SlaCvtType,
    args: &[f64],
    alpha: f64,
    delta: f64,
    step: &StepConsts,
) -> (f64, f64) {
    match cvt_type {
        // Inverse precession: the matrix was built with the epochs swapped.
        SlaCvtType::Prec | SlaCvtType::Prebn => {
            let v = pal::dcs2c(alpha, delta);
            let (a, d) = pal::dcc2s(&pal::dmxv(step.matrix(), &v));
            (pal::dranrm(a), d)
        }
        // J2000H inverse = transpose multiplication (same matrix, dimxv).
        SlaCvtType::J2000h => {
            let v = pal::dcs2c(alpha, delta);
            pal::dcc2s(&pal::dimxv(step.matrix(), &v))
        }
        // HJ2000 inverse = forward multiplication (same matrix, dmxv).
        SlaCvtType::Hj2000 => {
            let v = pal::dcs2c(alpha, delta);
            pal::dcc2s(&pal::dmxv(step.matrix(), &v))
        }
        // STP types: the step's matrix is the inverse rotation.
        SlaCvtType::Hpceq
        | SlaCvtType::Eqhpc
        | SlaCvtType::Hpreq
        | SlaCvtType::Eqhpr
        | SlaCvtType::Heeq
        | SlaCvtType::Eqhe => stp_apply(step.matrix(), alpha, delta),
        // For all other types, use the inverse type's forward operation. The
        // step constants were built for that type (`StepConsts::new`), and
        // Map.inverse()=Amp and Amp.inverse()=Map share the same mappa
        // parameters.
        other => apply_step_forward(other.inverse(), args, alpha, delta, step),
    }
}

// ---------------------------------------------------------------------------
// Mapping trait
// ---------------------------------------------------------------------------

impl Mapping for SlaMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    fn nin(&self) -> usize {
        2
    }

    fn nout(&self) -> usize {
        2
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform(self, &pts, forward, Some(&out))?;
        let fwd = self.effective_forward(forward);

        // Initialise the output by copying the input (slamap.c:3630).
        let npoint = pts.npoint();
        let [oa, od, ..] = out.axes_mut() else {
            unreachable!("a SlaMap has two outputs")
        };
        let (oa, od) = (&mut oa[..npoint], &mut od[..npoint]);
        oa.copy_from_slice(pts.axis(0));
        od.copy_from_slice(pts.axis(1));

        // Apply each conversion in order (reverse for inverse), mutating the
        // output arrays in place — the scalar (alpha, delta) carrier.
        let order: Vec<usize> = if fwd {
            (0..self.conversions.len()).collect()
        } else {
            (0..self.conversions.len()).rev().collect()
        };

        for &ci in &order {
            let cvt = &self.conversions[ci];

            // Bad-args guard (slamap.c:3672-3687): if any of this conversion's
            // nargs arguments is bad, the whole step outputs BAD for every
            // point (subsequent conversions then propagate the bad values).
            let nargs = cvt.cvt_type.nargs();
            if cvt.args.iter().take(nargs).any(|&a| is_bad(a)) {
                oa.fill(BAD_VALUE);
                od.fill(BAD_VALUE);
                continue;
            }

            let step = self.step_consts(ci, cvt, fwd);

            // TRAN_ARRAY (slamap.c:3652): per point, either axis bad -> both
            // bad, else apply the conversion.
            for i in 0..npoint {
                let (alpha, delta) = (oa[i], od[i]);
                let (a, d) = if is_bad(alpha) || is_bad(delta) {
                    (BAD_VALUE, BAD_VALUE)
                } else if fwd {
                    apply_step_forward(cvt.cvt_type, &cvt.args, alpha, delta, &step)
                } else {
                    apply_step_inverse(cvt.cvt_type, &cvt.args, alpha, delta, &step)
                };
                oa[i] = a;
                od[i] = d;
            }
        }

        Ok(())
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        numeric_rate(self, at, ax1, ax2)
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        sla_map_merge(ctx, where_, series)
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
        let other_sla = other.as_any().downcast_ref::<SlaMap>()?;

        // Normalize: account for both the mapping's own inverted flag
        // AND the external inversion from the simplification engine.
        let self_steps = sla_normalized_steps(self, self_inv != self.inverted);
        let other_steps = sla_normalized_steps(other_sla, other_inv != other_sla.inverted);

        // Concatenate: self is applied first, then other.
        let mut steps = self_steps;
        steps.extend(other_steps);

        // Always merge (concatenate) even if no simplification occurs.
        // This matches C slamap.c behaviour where all adjacent SlaMaps are
        // combined, enabling further simplification in subsequent passes.
        sla_simplify_steps(&mut steps);

        if steps.is_empty() {
            Some(Box::new(UnitMap::new(2)))
        } else {
            let mut result = SlaMap::new();
            for cvt in steps {
                result.add_conversion(cvt.cvt_type, &cvt.args);
            }
            Some(Box::new(result))
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

    fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    fn set_ident(&mut self, ident: &str) {
        self.ident = Some(ident.to_string());
    }

    fn clear_ident(&mut self) {
        self.ident = None;
    }

    fn test_ident(&self) -> bool {
        self.ident.is_some()
    }

    fn id(&self) -> &str {
        self.id.get()
    }
    fn set_id(&mut self, id: &str) {
        self.id.set(id);
    }
    fn clear_id(&mut self) {
        self.id.clear();
    }
    fn test_id(&self) -> bool {
        self.id.test()
    }

    fn has_been_simplified(&self) -> bool {
        self.simplified.get(self.inverted)
    }

    fn set_has_been_simplified(&mut self, flag: bool) {
        self.simplified.set(self.inverted, flag);
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
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
        node.class_comment = Some("Conversion between sky coordinate systems".to_string());

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
                let arg_descs = cvt.cvt_type.arg_descriptions();
                // C SlaMap Dump writes every argument unconditionally
                // (slamap.c:4448); the native writer renders an AST__BAD value
                // as the unquoted `<bad>` token, matching `astWriteDouble`.
                for (j, &arg) in cvt.args.iter().enumerate() {
                    let letter = arg_letter(j);
                    let comment = arg_descs.get(j).map(|s| s.to_string());
                    node.push_double(&format!("Sla{idx}{letter}"), arg, comment);
                }
            }
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("SlaMap");
        let base = read_mapping_base(whole);

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
                // A missing argument defaults to AST__BAD, matching C
                // `astLoadSlaMap` (slamap.c:4986, `astReadDouble(..., AST__BAD)`)
                // — not 0.0. SpecMap/TimeMap already use this default.
                let val = node.get_double(&format!("Sla{idx}{letter}"), BAD_VALUE);
                args.push(val);
            }

            sla.add_conversion(cvt_type, &args);
        }

        if base.inverted {
            sla.invert();
        }

        sla.set_has_been_simplified(base.has_been_simplified);

        Ok(sla)
    }
}
