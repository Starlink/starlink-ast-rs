// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Mapping that chains time coordinate system conversions.

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
// Physical constants (matching C AST values exactly)
// ---------------------------------------------------------------------------

/// Seconds per day.
const SPD: f64 = 86400.0;

/// Rate of drift of TCG with respect to terrestrial time.
const LG: f64 = 6.969290134e-10;

/// Rate of drift of TCB with respect to terrestrial time.
const LB: f64 = 1.55051976772e-8;

/// TDB-TCB offset constant P0 (seconds).
const P0: f64 = 6.55e-5;

/// TT-TAI offset (seconds).
const TTOFF: f64 = 32.184;

/// 2*pi.
const D2PI: f64 = 2.0 * std::f64::consts::PI;

/// 1 AU in meters.
const AST_AU: f64 = 1.49597870e11;

// ---------------------------------------------------------------------------
// TimeCvtType — conversion step types
// ---------------------------------------------------------------------------

/// A time coordinate conversion that [`TimeMap::add`] can add to a
/// [`TimeMap`].
///
/// The arguments a conversion takes are listed with each variant, in the
/// order they are passed to [`TimeMap::add`]; "MJD" means Modified Julian
/// Date.
/// [`nargs`](TimeCvtType::nargs) gives the number of arguments,
/// [`code_string`](TimeCvtType::code_string) the conversion's string code and
/// [`from_code`](TimeCvtType::from_code) parses one, case-insensitively.
///
/// The values the conversions process have these units:
///
/// - Julian epochs and offsets: Julian years;
/// - Besselian epochs and offsets: tropical years;
/// - Modified Julian Dates and offsets: days;
/// - Julian Dates and offsets: days.
///
/// The offset arguments are zero-points: the values a TimeMap transforms are
/// offsets from them.
/// The arguments are:
///
/// - `MJDOFF`: the zero-point used with MJD values;
/// - `JDOFF`: the zero-point used with Julian Date values;
/// - `BEPOFF`: the zero-point used with Besselian epoch values;
/// - `JEPOFF`: the zero-point used with Julian epoch values;
/// - `OBSLON`: observer longitude in radians (positive westwards);
/// - `OBSLAT`: observer geodetic latitude (IAU 1975) in radians (positive
///   northwards);
/// - `OBSALT`: observer geodetic altitude (IAU 1975) in meters;
/// - `DTAI`: the value of TAI-UTC in seconds; if it is [`BAD_VALUE`], the
///   value from the built-in table of leap seconds is used;
/// - `DUT1`: the value of UT1-UTC in seconds;
/// - `LTOFF`: the offset between Local Time and UTC in hours, positive for
///   time zones east of Greenwich.
///
/// Each conversion has a fixed integer discriminant, which [`from_int`](Self::from_int) maps back to the conversion.
// The discriminants are the C library's integer codes for the conversions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeCvtType {
    /// `MJDTOMJD` (`MJDOFF1`, `MJDOFF2`): convert an MJD from one zero-point
    /// to another.
    MjdToMjd = 1,
    /// `MJDTOJD` (`MJDOFF`, `JDOFF`): convert an MJD to a Julian Date.
    MjdToJd = 2,
    /// `JDTOMJD` (`JDOFF`, `MJDOFF`): convert a Julian Date to an MJD.
    JdToMjd = 3,
    /// `MJDTOBEP` (`MJDOFF`, `BEPOFF`): convert an MJD to a Besselian epoch.
    MjdToBep = 4,
    /// `BEPTOMJD` (`BEPOFF`, `MJDOFF`): convert a Besselian epoch to an MJD.
    BepToMjd = 5,
    /// `MJDTOJEP` (`MJDOFF`, `JEPOFF`): convert an MJD to a Julian epoch.
    MjdToJep = 6,
    /// `JEPTOMJD` (`JEPOFF`, `MJDOFF`): convert a Julian epoch to an MJD.
    JepToMjd = 7,
    /// `TAITOUTC` (`MJDOFF`, `DTAI`): convert a TAI MJD to a UTC MJD.
    TaiToUtc = 8,
    /// `UTCTOTAI` (`MJDOFF`, `DTAI`): convert a UTC MJD to a TAI MJD.
    UtcToTai = 9,
    /// `TTTOTAI` (`MJDOFF`): convert a TT MJD to a TAI MJD.
    TtToTai = 10,
    /// `TAITOTT` (`MJDOFF`): convert a TAI MJD to a TT MJD.
    TaiToTt = 11,
    /// `TDBTOTT` (`MJDOFF`, `OBSLON`, `OBSLAT`, `OBSALT`, `DTAI`): convert a
    /// TDB MJD to a TT MJD.
    TdbToTt = 12,
    /// `TTTOTDB` (`MJDOFF`, `OBSLON`, `OBSLAT`, `OBSALT`, `DTAI`): convert a
    /// TT MJD to a TDB MJD.
    TtToTdb = 13,
    /// `TCGTOTT` (`MJDOFF`): convert a TCG MJD to a TT MJD.
    TcgToTt = 14,
    /// `TTTOTCG` (`MJDOFF`): convert a TT MJD to a TCG MJD.
    TtToTcg = 15,
    /// `TCBTOTDB` (`MJDOFF`): convert a TCB MJD to a TDB MJD.
    TcbToTdb = 16,
    /// `TDBTOTCB` (`MJDOFF`): convert a TDB MJD to a TCB MJD.
    TdbToTcb = 17,
    /// `UTTOGMST` (`MJDOFF`): convert a UT MJD to a GMST MJD.
    UtToGmst = 18,
    /// `GMSTTOUT` (`MJDOFF`): convert a GMST MJD to a UT MJD.
    GmstToUt = 19,
    /// `GMSTTOLMST` (`MJDOFF`, `OBSLON`, `OBSLAT`): convert a GMST MJD to an
    /// LMST MJD.
    GmstToLmst = 20,
    /// `LMSTTOGMST` (`MJDOFF`, `OBSLON`, `OBSLAT`): convert an LMST MJD to a
    /// GMST MJD.
    LmstToGmst = 21,
    /// `LASTTOLMST` (`MJDOFF`, `OBSLON`, `OBSLAT`): convert a LAST MJD to an
    /// LMST MJD.
    LastToLmst = 22,
    /// `LMSTTOLAST` (`MJDOFF`, `OBSLON`, `OBSLAT`): convert an LMST MJD to a
    /// LAST MJD.
    LmstToLast = 23,
    /// `UTTOUTC` (`DUT1`): convert a UT1 MJD to a UTC MJD.
    UtToUtc = 24,
    /// `UTCTOUT` (`DUT1`): convert a UTC MJD to a UT1 MJD.
    UtcToUt = 25,
    /// `LTTOUTC` (`LTOFF`): convert a Local Time MJD to a UTC MJD.
    LtToUtc = 26,
    /// `UTCTOLT` (`LTOFF`): convert a UTC MJD to a Local Time MJD.
    UtcToLt = 27,
}

impl TimeCvtType {
    /// Number of user-supplied floating-point arguments this conversion requires.
    pub fn nargs(&self) -> usize {
        match self {
            Self::MjdToMjd
            | Self::MjdToJd
            | Self::JdToMjd
            | Self::MjdToBep
            | Self::BepToMjd
            | Self::MjdToJep
            | Self::JepToMjd
            | Self::TaiToUtc
            | Self::UtcToTai => 2,
            Self::TaiToTt
            | Self::TtToTai
            | Self::TcgToTt
            | Self::TtToTcg
            | Self::TcbToTdb
            | Self::TdbToTcb
            | Self::UtToGmst
            | Self::GmstToUt
            | Self::UtToUtc
            | Self::UtcToUt
            | Self::LtToUtc
            | Self::UtcToLt => 1,
            Self::GmstToLmst | Self::LmstToGmst | Self::LastToLmst | Self::LmstToLast => 3,
            Self::TdbToTt | Self::TtToTdb => 5,
        }
    }

    /// Total number of argument slots (includes derived/cached values).
    fn szargs(&self) -> usize {
        match self {
            Self::MjdToMjd | Self::MjdToJd | Self::JdToMjd => 3,
            Self::MjdToBep | Self::BepToMjd | Self::MjdToJep | Self::JepToMjd => 4,
            Self::TaiToUtc | Self::UtcToTai => 2,
            Self::TaiToTt | Self::TtToTai | Self::UtToGmst | Self::GmstToUt => 1,
            Self::TtToTcg | Self::TcgToTt | Self::TdbToTcb | Self::TcbToTdb => 2,
            Self::TdbToTt | Self::TtToTdb => 7,
            Self::GmstToLmst | Self::LmstToGmst | Self::LastToLmst | Self::LmstToLast => 3,
            Self::UtToUtc | Self::UtcToUt | Self::LtToUtc | Self::UtcToLt => 1,
        }
    }

    /// Uppercase code string for serialization (e.g. `"MJDTOMJD"`, `"TAITOUTC"`).
    pub fn code_string(&self) -> &'static str {
        match self {
            Self::MjdToMjd => "MJDTOMJD",
            Self::MjdToJd => "MJDTOJD",
            Self::JdToMjd => "JDTOMJD",
            Self::MjdToBep => "MJDTOBEP",
            Self::BepToMjd => "BEPTOMJD",
            Self::MjdToJep => "MJDTOJEP",
            Self::JepToMjd => "JEPTOMJD",
            Self::TaiToUtc => "TAITOUTC",
            Self::UtcToTai => "UTCTOTAI",
            Self::TaiToTt => "TAITOTT",
            Self::TtToTai => "TTTOTAI",
            Self::TdbToTt => "TDBTOTT",
            Self::TtToTdb => "TTTOTDB",
            Self::TcgToTt => "TCGTOTT",
            Self::TtToTcg => "TTTOTCG",
            Self::TcbToTdb => "TCBTOTDB",
            Self::TdbToTcb => "TDBTOTCB",
            Self::UtToGmst => "UTTOGMST",
            Self::GmstToUt => "GMSTTOUT",
            Self::GmstToLmst => "GMSTTOLMST",
            Self::LmstToGmst => "LMSTTOGMST",
            Self::LastToLmst => "LASTTOLMST",
            Self::LmstToLast => "LMSTTOLAST",
            Self::UtToUtc => "UTTOUTC",
            Self::UtcToUt => "UTCTOUT",
            Self::LtToUtc => "LTTOUTC",
            Self::UtcToLt => "UTCTOLT",
        }
    }

    /// Parse a conversion's string code, such as `"MJDTOJD"` or `"taitoutc"`,
    /// ignoring case and trailing white space.
    ///
    /// Returns `None` for an unrecognized code.
    pub fn from_code(s: &str) -> Option<Self> {
        // C's CvtCode matches with astChrMatch.
        match crate::mappings::helpers::chr_match_key(s).as_str() {
            "MJDTOMJD" => Some(Self::MjdToMjd),
            "MJDTOJD" => Some(Self::MjdToJd),
            "JDTOMJD" => Some(Self::JdToMjd),
            "MJDTOBEP" => Some(Self::MjdToBep),
            "BEPTOMJD" => Some(Self::BepToMjd),
            "MJDTOJEP" => Some(Self::MjdToJep),
            "JEPTOMJD" => Some(Self::JepToMjd),
            "TAITOUTC" => Some(Self::TaiToUtc),
            "UTCTOTAI" => Some(Self::UtcToTai),
            "TAITOTT" => Some(Self::TaiToTt),
            "TTTOTAI" => Some(Self::TtToTai),
            "TDBTOTT" => Some(Self::TdbToTt),
            "TTTOTDB" => Some(Self::TtToTdb),
            "TCGTOTT" => Some(Self::TcgToTt),
            "TTTOTCG" => Some(Self::TtToTcg),
            "TCBTOTDB" => Some(Self::TcbToTdb),
            "TDBTOTCB" => Some(Self::TdbToTcb),
            "UTTOGMST" => Some(Self::UtToGmst),
            "GMSTTOUT" => Some(Self::GmstToUt),
            "GMSTTOLMST" => Some(Self::GmstToLmst),
            "LMSTTOGMST" => Some(Self::LmstToGmst),
            "LASTTOLMST" => Some(Self::LastToLmst),
            "LMSTTOLAST" => Some(Self::LmstToLast),
            "UTTOUTC" => Some(Self::UtToUtc),
            "UTCTOUT" => Some(Self::UtcToUt),
            "LTTOUTC" => Some(Self::LtToUtc),
            "UTCTOLT" => Some(Self::UtcToLt),
            _ => None,
        }
    }

    /// Return the inverse conversion type.
    pub fn inverse(&self) -> Self {
        match self {
            Self::MjdToMjd => Self::MjdToMjd,
            Self::MjdToJd => Self::JdToMjd,
            Self::JdToMjd => Self::MjdToJd,
            Self::MjdToBep => Self::BepToMjd,
            Self::BepToMjd => Self::MjdToBep,
            Self::MjdToJep => Self::JepToMjd,
            Self::JepToMjd => Self::MjdToJep,
            Self::TaiToUtc => Self::UtcToTai,
            Self::UtcToTai => Self::TaiToUtc,
            Self::TaiToTt => Self::TtToTai,
            Self::TtToTai => Self::TaiToTt,
            Self::TdbToTt => Self::TtToTdb,
            Self::TtToTdb => Self::TdbToTt,
            Self::TcgToTt => Self::TtToTcg,
            Self::TtToTcg => Self::TcgToTt,
            Self::TcbToTdb => Self::TdbToTcb,
            Self::TdbToTcb => Self::TcbToTdb,
            Self::UtToGmst => Self::GmstToUt,
            Self::GmstToUt => Self::UtToGmst,
            Self::GmstToLmst => Self::LmstToGmst,
            Self::LmstToGmst => Self::GmstToLmst,
            Self::LastToLmst => Self::LmstToLast,
            Self::LmstToLast => Self::LastToLmst,
            Self::UtToUtc => Self::UtcToUt,
            Self::UtcToUt => Self::UtToUtc,
            Self::LtToUtc => Self::UtcToLt,
            Self::UtcToLt => Self::LtToUtc,
        }
    }

    /// The conversion whose discriminant is `i` (`Self::MjdToMjd as i32` is 1), or `None` when no conversion has that value.
    // The discriminants are the C library's integer codes for the conversions.
    pub fn from_int(i: i32) -> Option<Self> {
        match i {
            1 => Some(Self::MjdToMjd),
            2 => Some(Self::MjdToJd),
            3 => Some(Self::JdToMjd),
            4 => Some(Self::MjdToBep),
            5 => Some(Self::BepToMjd),
            6 => Some(Self::MjdToJep),
            7 => Some(Self::JepToMjd),
            8 => Some(Self::TaiToUtc),
            9 => Some(Self::UtcToTai),
            10 => Some(Self::TtToTai),
            11 => Some(Self::TaiToTt),
            12 => Some(Self::TdbToTt),
            13 => Some(Self::TtToTdb),
            14 => Some(Self::TcgToTt),
            15 => Some(Self::TtToTcg),
            16 => Some(Self::TcbToTdb),
            17 => Some(Self::TdbToTcb),
            18 => Some(Self::UtToGmst),
            19 => Some(Self::GmstToUt),
            20 => Some(Self::GmstToLmst),
            21 => Some(Self::LmstToGmst),
            22 => Some(Self::LastToLmst),
            23 => Some(Self::LmstToLast),
            24 => Some(Self::UtToUtc),
            25 => Some(Self::UtcToUt),
            26 => Some(Self::LtToUtc),
            27 => Some(Self::UtcToLt),
            _ => None,
        }
    }

    /// Human-readable descriptions for each serialized argument slot.
    ///
    /// The slice length matches `szargs()`. Used as comments in the Native
    /// serialization format. Derived (not stored) slots use empty strings.
    pub fn arg_descriptions(&self) -> &'static [&'static str] {
        match self {
            Self::MjdToMjd => &["Input MJD offset", "Output MJD offset", "Combined offset"],
            Self::MjdToJd => &["MJD offset", "JD offset", "Combined offset"],
            Self::JdToMjd => &["JD offset", "MJD offset", "Combined offset"],
            Self::MjdToBep => &[
                "MJD offset",
                "Besselian epoch offset",
                "Combined forward offset",
                "Combined inverse offset",
            ],
            Self::BepToMjd => &[
                "Besselian epoch offset",
                "MJD offset",
                "Combined forward offset",
                "Combined inverse offset",
            ],
            Self::MjdToJep => &[
                "MJD offset",
                "Julian epoch offset",
                "Combined forward offset",
                "Combined inverse offset",
            ],
            Self::JepToMjd => &[
                "Julian epoch offset",
                "MJD offset",
                "Combined forward offset",
                "Combined inverse offset",
            ],
            Self::TaiToUtc | Self::UtcToTai => &["MJD offset", "DTAI"],
            Self::TaiToTt | Self::TtToTai => &["MJD offset"],
            Self::TcgToTt | Self::TtToTcg => &["MJD offset", "TCG offset"],
            Self::TcbToTdb | Self::TdbToTcb => &["MJD offset", "TCB offset"],
            // External order (Time1a..Time1g) — internal slots [4]=DTAI and
            // [5]/[6]=derived swap places at the external boundary, see the
            // `order` map in `to_node`.
            Self::TdbToTt | Self::TtToTdb => &[
                "MJD offset",
                "Observer longitude",
                "Observer latitude",
                "Observer altitude",
                "Distance from earth spin axis",
                "Distance north of equatorial plane",
                "DTAI",
            ],
            Self::UtToGmst | Self::GmstToUt => &["MJD offset"],
            Self::GmstToLmst | Self::LmstToGmst => {
                &["MJD offset", "Observer longitude", "Observer latitude"]
            }
            Self::LastToLmst | Self::LmstToLast => {
                &["MJD offset", "Observer longitude", "Observer latitude"]
            }
            Self::UtToUtc | Self::UtcToUt => &["DUT1"],
            Self::LtToUtc | Self::UtcToLt => &["LTOFF"],
        }
    }

    /// Human-readable description for serialization comments.
    pub fn description(&self) -> &'static str {
        match self {
            Self::MjdToMjd => "Convert MJD between offsets",
            Self::MjdToJd => "Convert MJD to JD",
            Self::JdToMjd => "Convert JD to MJD",
            Self::MjdToBep => "Convert MJD to Besselian epoch",
            Self::BepToMjd => "Convert Besselian epoch to MJD",
            Self::MjdToJep => "Convert MJD to Julian epoch",
            Self::JepToMjd => "Convert Julian epoch to MJD",
            Self::TaiToUtc => "Convert TAI to UTC",
            Self::UtcToTai => "Convert UTC to TAI",
            Self::TaiToTt => "Convert TAI to TT",
            Self::TtToTai => "Convert TT to TAI",
            Self::TdbToTt => "Convert TDB to TT",
            Self::TtToTdb => "Convert TT to TDB",
            Self::TcgToTt => "Convert TCG to TT",
            Self::TtToTcg => "Convert TT to TCG",
            Self::TcbToTdb => "Convert TCB to TDB",
            Self::TdbToTcb => "Convert TDB to TCB",
            Self::UtToGmst => "Convert UT to GMST",
            Self::GmstToUt => "Convert GMST to UT",
            Self::GmstToLmst => "Convert GMST to LMST",
            Self::LmstToGmst => "Convert LMST to GMST",
            Self::LastToLmst => "Convert LAST to LMST",
            Self::LmstToLast => "Convert LMST to LAST",
            Self::UtToUtc => "Convert UT1 to UTC",
            Self::UtcToUt => "Convert UTC to UT1",
            Self::LtToUtc => "Convert Local Time to UTC",
            Self::UtcToLt => "Convert UTC to Local Time",
        }
    }
}

// ---------------------------------------------------------------------------
// TimeConversion — one step in the chain
// ---------------------------------------------------------------------------

/// A single conversion step in a [`TimeMap`] chain.
#[derive(Debug, Clone)]
struct TimeConversion {
    cvt_type: TimeCvtType,
    /// Full argument array including derived values.
    args: Vec<f64>,
}

// ---------------------------------------------------------------------------
// TimeMap
// ---------------------------------------------------------------------------

/// Sequence of time coordinate conversions.
///
/// A TimeMap is a 1-dimensional Mapping that represents a sequence of
/// conversions between standard time coordinate systems, such as MJD to
/// Julian Date, UTC to TAI or TT to TDB.
///
/// When first created, a TimeMap is a unit (null) Mapping.
/// [`add`](TimeMap::add) then appends coordinate conversion steps, so that
/// multi-step conversions between a variety of time coordinate systems can be
/// assembled out of a set of building blocks.
/// [`TimeCvtType`] lists the conversions available.
///
/// # Example
///
/// ```
/// use starlink_ast::mappings::{Mapping, TimeMap, TimeCvtType};
/// use starlink_ast::coords::PointSet;
///
/// let mut tm = TimeMap::new();
/// // Convert MJD to Julian epoch (J2000.0 = MJD 51544.5)
/// tm.add(TimeCvtType::MjdToJep, &[0.0, 0.0]).unwrap();
///
/// let pts = PointSet::from_rows([[51544.5_f64]]);
/// let out = tm.transform(&pts, true).unwrap();
/// assert!((out.axis(0)[0] - 2000.0).abs() < 1.0e-10);
/// ```
#[derive(Debug, Clone)]
pub struct TimeMap {
    conversions: Vec<TimeConversion>,
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
}

impl TimeMap {
    /// Create a TimeMap with no conversion steps, which performs a unit
    /// Mapping until steps are added with [`add`](TimeMap::add).
    ///
    /// The nature and units of the values supplied to the TimeMap must suit
    /// the first conversion step it applies, and those it returns are set by
    /// the last: if the first step is [`TimeCvtType::MjdToBep`], the input is
    /// a Modified Julian Date in days.
    #[doc(alias = "astTimeMap")]
    pub fn new() -> Self {
        Self {
            conversions: Vec::new(),
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        }
    }

    /// Add a time coordinate conversion to the TimeMap.
    ///
    /// `cvt_type` selects the conversion, and `args` holds its arguments in
    /// the order listed for that [`TimeCvtType`] variant, in the units listed
    /// there.
    ///
    /// The forward transformation of the TimeMap applies its conversions in
    /// the order they were added, the most recently added last.
    /// The inverse transformation, or the forward transformation of an
    /// inverted TimeMap, applies the inverse of each conversion in the
    /// reverse order, the inverse of the most recently added first.
    ///
    /// No check is made that the sequence of conversions is physically
    /// meaningful.
    /// When assembling a multi-step conversion, it is often simplest to
    /// include every step that is logically needed and then simplify the
    /// TimeMap (`simplify` on `dyn Mapping`), which removes the steps that
    /// turn out not to be needed.
    ///
    /// ```
    /// use starlink_ast::mappings::{Mapping, TimeCvtType, TimeMap};
    ///
    /// // MJD offsets from MJD 50000 to Julian Date offsets from JD 2450000.
    /// let mut map = TimeMap::new();
    /// map.add(TimeCvtType::MjdToJd, &[50000.0, 2450000.0]).unwrap();
    /// assert_eq!(map.tran1(&[1.0], true).unwrap(), vec![1.5]);
    /// ```
    ///
    /// # Errors
    ///
    /// `ErrorCode::Timin` if `args` does not hold exactly
    /// [`cvt_type.nargs()`](TimeCvtType::nargs) values.
    #[doc(alias = "astTimeAdd")]
    pub fn add(&mut self, cvt_type: TimeCvtType, args: &[f64]) -> AstResult<()> {
        // The argument count check is timemap.c AddTimeCvt's.
        if args.len() != cvt_type.nargs() {
            return Err(AstError::new(
                ErrorCode::Timin,
                format!(
                    "TimeAdd: invalid number of arguments for time coordinate conversion \
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
    pub(crate) fn add_conversion(&mut self, cvt_type: TimeCvtType, args: &[f64]) {
        let szargs = cvt_type.szargs();
        let nargs = cvt_type.nargs();
        let mut full_args = vec![BAD_VALUE; szargs];
        // Copy user-supplied arguments.
        let n_copy = nargs.min(args.len());
        full_args[..n_copy].copy_from_slice(&args[..n_copy]);
        // Compute derived arguments.
        add_args(cvt_type, &mut full_args);
        self.conversions.push(TimeConversion {
            cvt_type,
            args: full_args,
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

impl Default for TimeMap {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Compute derived argument values (matching C AddArgs function)
// ---------------------------------------------------------------------------

/// Compute derived argument slots from user-supplied arguments.
fn add_args(cvt_type: TimeCvtType, args: &mut [f64]) {
    match cvt_type {
        TimeCvtType::MjdToMjd => {
            // args[2] = combined offset = input_offset - output_offset
            args[2] = args[0] - args[1];
        }
        TimeCvtType::MjdToJd => {
            // args[2] = input_offset - jd_offset + 2400000.5
            args[2] = args[0] - args[1] + 2_400_000.5;
        }
        TimeCvtType::JdToMjd => {
            // args[2] = jd_offset - mjd_offset - 2400000.5
            args[2] = args[0] - args[1] - 2_400_000.5;
        }
        TimeCvtType::MjdToBep => {
            // Forward: bep = epb(mjd + mjd_offset) - bep_offset
            //   with pre-computed offset = epb(mjd_offset) - epb(0) - bep_offset
            // Inverse: mjd = epb2d(bep + bep_offset) - mjd_offset
            //   with pre-computed offset = epb2d(bep_offset) - epb2d(0) - mjd_offset
            args[2] = pal::epb(args[0]) - pal::epb(0.0) - args[1];
            args[3] = pal::epb2d(args[1]) - pal::epb2d(0.0) - args[0];
        }
        TimeCvtType::BepToMjd => {
            args[2] = pal::epb2d(args[0]) - pal::epb2d(0.0) - args[1];
            args[3] = pal::epb(args[1]) - pal::epb(0.0) - args[0];
        }
        TimeCvtType::MjdToJep => {
            args[2] = pal::epj(args[0]) - pal::epj(0.0) - args[1];
            args[3] = pal::epj2d(args[1]) - pal::epj2d(0.0) - args[0];
        }
        TimeCvtType::JepToMjd => {
            args[2] = pal::epj2d(args[0]) - pal::epj2d(0.0) - args[1];
            args[3] = pal::epj(args[1]) - pal::epj(0.0) - args[0];
        }
        TimeCvtType::TtToTdb | TimeCvtType::TdbToTt => {
            // args: [0]=mjd_offset, [1]=obslon, [2]=obslat, [3]=obsalt, [4]=dtai
            // Compute [5]=u (dist from earth axis in km), [6]=v (dist north of eq plane in km)
            let (r, z) = pal::geoc(args[2], args[3]);
            args[5] = 0.001 * r * AST_AU;
            args[6] = 0.001 * z * AST_AU;
        }
        TimeCvtType::TdbToTcb | TimeCvtType::TcbToTdb => {
            // args[1] = LB * (mjd_offset - TTOFF/SPD - 43144.0) + P0/SPD
            args[1] = LB * (args[0] - (TTOFF / SPD) - 43144.0) + P0 / SPD;
        }
        TimeCvtType::TtToTcg | TimeCvtType::TcgToTt => {
            // args[1] = LG * (mjd_offset - TTOFF/SPD - 43144.0)
            args[1] = LG * (args[0] - (TTOFF / SPD) - 43144.0);
        }
        // All other types don't need derived arguments.
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// astDat — TAI-UTC / UTC-TAI conversion (matching C astDat_ exactly)
// ---------------------------------------------------------------------------

/// Return TAI-UTC (in seconds) given UTC as an absolute MJD (`forward=true`),
/// or UTC-TAI (in seconds) given TAI as an absolute MJD (`forward=false`).
///
/// This is the Rust equivalent of the C `astDat_` function.
#[allow(clippy::eq_op, clippy::if_same_then_else)]
fn ast_dat(mjd: f64, forward: bool) -> f64 {
    if is_bad(mjd) {
        return BAD_VALUE;
    }
    if forward {
        // TAI-UTC at a given UTC
        if mjd >= 57754.0 {
            37.0
        } else if mjd >= 57204.0 {
            36.0
        } else if mjd >= 56109.0 {
            35.0
        } else if mjd >= 54832.0 {
            34.0
        } else if mjd >= 53736.0 {
            33.0
        } else if mjd >= 51179.0 {
            32.0
        } else if mjd >= 50630.0 {
            31.0
        } else if mjd >= 50083.0 {
            30.0
        } else if mjd >= 49534.0 {
            29.0
        } else if mjd >= 49169.0 {
            28.0
        } else if mjd >= 48804.0 {
            27.0
        } else if mjd >= 48257.0 {
            26.0
        } else if mjd >= 47892.0 {
            25.0
        } else if mjd >= 47161.0 {
            24.0
        } else if mjd >= 46247.0 {
            23.0
        } else if mjd >= 45516.0 {
            22.0
        } else if mjd >= 45151.0 {
            21.0
        } else if mjd >= 44786.0 {
            20.0
        } else if mjd >= 44239.0 {
            19.0
        } else if mjd >= 43874.0 {
            18.0
        } else if mjd >= 43509.0 {
            17.0
        } else if mjd >= 43144.0 {
            16.0
        } else if mjd >= 42778.0 {
            15.0
        } else if mjd >= 42413.0 {
            14.0
        } else if mjd >= 42048.0 {
            13.0
        } else if mjd >= 41683.0 {
            12.0
        } else if mjd >= 41499.0 {
            11.0
        } else if mjd >= 41317.0 {
            10.0
        } else if mjd >= 39887.0 {
            4.2131700 + (mjd - 39126.0) * 0.002592
        } else if mjd >= 39126.0 {
            4.3131700 + (mjd - 39126.0) * 0.002592
        } else if mjd >= 39004.0 {
            3.8401300 + (mjd - 38761.0) * 0.001296
        } else if mjd >= 38942.0 {
            3.7401300 + (mjd - 38761.0) * 0.001296
        } else if mjd >= 38820.0 {
            3.6401300 + (mjd - 38761.0) * 0.001296
        } else if mjd >= 38761.0 {
            3.5401300 + (mjd - 38761.0) * 0.001296
        } else if mjd >= 38639.0 {
            3.4401300 + (mjd - 38761.0) * 0.001296
        } else if mjd >= 38486.0 {
            3.3401300 + (mjd - 38761.0) * 0.001296
        } else if mjd >= 38395.0 {
            3.2401300 + (mjd - 38761.0) * 0.001296
        } else if mjd >= 38334.0 {
            1.9458580 + (mjd - 37665.0) * 0.0011232
        } else if mjd >= 37665.0 {
            1.8458580 + (mjd - 37665.0) * 0.0011232
        } else if mjd >= 37512.0 {
            1.3728180 + (mjd - 37300.0) * 0.001296
        } else if mjd >= 37300.0 {
            1.4228180 + (mjd - 37300.0) * 0.001296
        } else {
            1.4178180 + (mjd - 37300.0) * 0.001296
        }
    } else {
        // UTC-TAI at a given TAI
        if mjd >= 57754.0 + 37.0 / SPD {
            -37.0
        } else if mjd >= 57204.0 + 36.0 / SPD {
            -36.0
        } else if mjd >= 56109.0 + 35.0 / SPD {
            -35.0
        } else if mjd >= 54832.0 + 34.0 / SPD {
            -34.0
        } else if mjd >= 53736.0 + 33.0 / SPD {
            -33.0
        } else if mjd >= 51179.0 + 32.0 / SPD {
            -32.0
        } else if mjd >= 50630.0 + 31.0 / SPD {
            -31.0
        } else if mjd >= 50083.0 + 30.0 / SPD {
            -30.0
        } else if mjd >= 49534.0 + 29.0 / SPD {
            -29.0
        } else if mjd >= 49169.0 + 28.0 / SPD {
            -28.0
        } else if mjd >= 48804.0 + 27.0 / SPD {
            -27.0
        } else if mjd >= 48257.0 + 26.0 / SPD {
            -26.0
        } else if mjd >= 47892.0 + 25.0 / SPD {
            -25.0
        } else if mjd >= 47161.0 + 24.0 / SPD {
            -24.0
        } else if mjd >= 46247.0 + 23.0 / SPD {
            -23.0
        } else if mjd >= 45516.0 + 22.0 / SPD {
            -22.0
        } else if mjd >= 45151.0 + 21.0 / SPD {
            -21.0
        } else if mjd >= 44786.0 + 20.0 / SPD {
            -20.0
        } else if mjd >= 44239.0 + 19.0 / SPD {
            -19.0
        } else if mjd >= 43874.0 + 18.0 / SPD {
            -18.0
        } else if mjd >= 43509.0 + 17.0 / SPD {
            -17.0
        } else if mjd >= 43144.0 + 16.0 / SPD {
            -16.0
        } else if mjd >= 42778.0 + 15.0 / SPD {
            -15.0
        } else if mjd >= 42413.0 + 14.0 / SPD {
            -14.0
        } else if mjd >= 42048.0 + 13.0 / SPD {
            -13.0
        } else if mjd >= 41683.0 + 12.0 / SPD {
            -12.0
        } else if mjd >= 41499.0 + 11.0 / SPD {
            -11.0
        } else if mjd >= 41317.0 + 10.0 / SPD {
            -10.0
        } else if mjd >= 39887.0 + (4.2131700 + (39887.0 - 39126.0) * 0.002592) / SPD {
            -(4.2131700 + (mjd - 39126.0) * 0.002592) / (1.0 + 0.002592 / SPD)
        } else if mjd >= 39126.0 + (4.3131700 + (39126.0 - 39126.0) * 0.002592) / SPD {
            -(4.3131700 + (mjd - 39126.0) * 0.002592) / (1.0 + 0.002592 / SPD)
        } else if mjd >= 39004.0 + (3.8401300 + (39004.0 - 38761.0) * 0.001296) / SPD {
            -(3.8401300 + (mjd - 38761.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else if mjd >= 38942.0 + (3.7401300 + (38942.0 - 38761.0) * 0.001296) / SPD {
            -(3.7401300 + (mjd - 38761.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else if mjd >= 38820.0 + (3.6401300 + (38820.0 - 38761.0) * 0.001296) / SPD {
            -(3.6401300 + (mjd - 38761.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else if mjd >= 38761.0 + (3.5401300 + (38761.0 - 38761.0) * 0.001296) / SPD {
            -(3.5401300 + (mjd - 38761.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else if mjd >= 38639.0 + (3.4401300 + (38639.0 - 38761.0) * 0.001296) / SPD {
            -(3.4401300 + (mjd - 38761.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else if mjd >= 38486.0 + (3.3401300 + (38486.0 - 38761.0) * 0.001296) / SPD {
            -(3.3401300 + (mjd - 38761.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else if mjd >= 38395.0 + (3.2401300 + (38395.0 - 38761.0) * 0.001296) / SPD {
            -(3.2401300 + (mjd - 38761.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else if mjd >= 38334.0 + (1.9458580 + (38334.0 - 37665.0) * 0.0011232) / SPD {
            -(1.9458580 + (mjd - 37665.0) * 0.0011232) / (1.0 + 0.0011232 / SPD)
        } else if mjd >= 37665.0 + (1.8458580 + (37665.0 - 37665.0) * 0.0011232) / SPD {
            -(1.8458580 + (mjd - 37665.0) * 0.0011232) / (1.0 + 0.0011232 / SPD)
        } else if mjd >= 37512.0 + (1.3728180 + (37512.0 - 37300.0) * 0.001296) / SPD {
            -(1.3728180 + (mjd - 37300.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else if mjd >= 37300.0 + (1.4228180 + (37300.0 - 37300.0) * 0.001296) / SPD {
            -(1.4228180 + (mjd - 37300.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        } else {
            -(1.4178180 + (mjd - 37300.0) * 0.001296) / (1.0 + 0.001296 / SPD)
        }
    }
}

// ---------------------------------------------------------------------------
// Gmsta — UT1 <-> GMST conversion (matching C Gmsta function)
// ---------------------------------------------------------------------------

/// Convert between UT1 and GMST.
///
/// Both timescales are represented by an MJD offset (in days) rather than
/// as an angle. The input is offset from `off`, meaning the absolute MJD
/// is `in_val + off`.
///
/// - `forward=true`: input is UT1, output is GMST.
/// - `forward=false`: input is GMST, output is UT1.
fn gmsta(in_val: f64, off: f64, forward: bool) -> f64 {
    if is_bad(in_val) || is_bad(off) {
        return BAD_VALUE;
    }

    if forward {
        // UT1 -> GMST
        // Julian centuries since J2000.
        let t = (off + in_val - 51544.5) / 36525.0;
        // GMST at this UT1.
        in_val + (24110.54841 + (8640184.812866 + (0.093104 - 6.2e-6 * t) * t) * t) / 86400.0
    } else {
        // GMST -> UT1 (iterative)
        // Initial guess using inverse of linear approximation.
        let mut result = 0.996997348638869 * in_val + 154.49194372222 - 0.00300265136113098 * off;
        let mut utl = BAD_VALUE;
        let mut nit = 0;
        while result != utl && nit < 10 {
            let t = (off + result - 51544.5) / 36525.0;
            let g = result
                + (24110.54841 + (8640184.812866 + (0.093104 - 6.2e-6 * t) * t) * t) / 86400.0;
            let dgdu = 1.0 + (8640184.812866 + (0.186208 - 12.4e-6 * t) * t) / (36525.0 * 86400.0);
            utl = result;
            result -= (g - in_val) / dgdu;
            nit += 1;
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Rcc — Relativistic clock correction TDB-TT (Fairhead & Bretagnon 1990)
// ---------------------------------------------------------------------------

/// Relativistic clock correction: TDB-TT in seconds.
///
/// This is the Rust translation of the C `Rcc` function.
///
/// - `tdb`: TDB as an absolute MJD
/// - `ut1`: UT1 as an absolute MJD (only fraction of day matters)
/// - `wl`: observer west longitude (radians)
/// - `u`: observer distance from Earth spin axis (km)
/// - `v`: observer distance north of Earth equatorial plane (km)
#[allow(clippy::inconsistent_digit_grouping)]
fn rcc(tdb: f64, ut1: f64, wl: f64, u: f64, v: f64) -> f64 {
    // Fairhead and Bretagnon canonical coefficients (787 sets of 3).
    // Included as a separate constant to keep the function readable.
    #[allow(clippy::excessive_precision)]
    static FAIRHD: &[[f64; 3]] = &include!("timemap_fairhd.inc");

    // Time since J2000.0 in Julian millennia.
    let t = (tdb - 51544.5) / 365250.0;

    // ---- Topocentric terms (Moyer 1981, Murray 1983) ----
    let tsol = (ut1 % 1.0) * D2PI - wl;
    let w = t / 3600.0;
    let elsun = ((280.46645683 + 1296027711.03429 * w) % 360.0).to_radians();
    let emsun = ((357.52910918 + 1295965810.481 * w) % 360.0).to_radians();
    let d = ((297.85019547 + 16029616012.090 * w) % 360.0).to_radians();
    let elj = ((34.35151874 + 109306899.89453 * w) % 360.0).to_radians();
    let els = ((50.07744430 + 44046398.47038 * w) % 360.0).to_radians();

    let wt = 0.00029e-10 * u * crate::math::sin(tsol + elsun - els)
        + 0.00100e-10 * u * crate::math::sin(tsol - 2.0 * emsun)
        + 0.00133e-10 * u * crate::math::sin(tsol - d)
        + 0.00133e-10 * u * crate::math::sin(tsol + elsun - elj)
        - 0.00229e-10 * u * crate::math::sin(tsol + 2.0 * elsun + emsun)
        - 0.0220e-10 * v * crate::math::cos(elsun + emsun)
        + 0.05312e-10 * u * crate::math::sin(tsol - emsun)
        - 0.13677e-10 * u * crate::math::sin(tsol + 2.0 * elsun)
        - 1.3184e-10 * v * crate::math::cos(elsun)
        + 3.17679e-10 * u * crate::math::sin(tsol);

    // ---- Fairhead model ----
    // t**0
    let mut w0 = 0.0;
    for i in (0..=473).rev() {
        w0 += FAIRHD[i][0] * crate::math::sin(FAIRHD[i][1] * t + FAIRHD[i][2]);
    }

    // t**1
    let mut w1 = 0.0;
    for i in (474..=678).rev() {
        w1 += FAIRHD[i][0] * crate::math::sin(FAIRHD[i][1] * t + FAIRHD[i][2]);
    }

    // t**2
    let mut w2 = 0.0;
    for i in (679..=763).rev() {
        w2 += FAIRHD[i][0] * crate::math::sin(FAIRHD[i][1] * t + FAIRHD[i][2]);
    }

    // t**3
    let mut w3 = 0.0;
    for i in (764..=783).rev() {
        w3 += FAIRHD[i][0] * crate::math::sin(FAIRHD[i][1] * t + FAIRHD[i][2]);
    }

    // t**4
    let mut w4 = 0.0;
    for i in (784..=786).rev() {
        w4 += FAIRHD[i][0] * crate::math::sin(FAIRHD[i][1] * t + FAIRHD[i][2]);
    }

    // Multiply by powers of T and combine.
    let wf = t * (t * (t * (t * w4 + w3) + w2) + w1) + w0;

    // Adjustments to use JPL planetary masses instead of IAU.
    let wj = 0.00065e-6 * crate::math::sin(6069.776754 * t + 4.021194)
        + 0.00033e-6 * crate::math::sin(213.299095 * t + 5.543132)
        + (-0.00196e-6 * crate::math::sin(6208.294251 * t + 5.696701))
        + (-0.00173e-6 * crate::math::sin(74.781599 * t + 2.435900))
        + 0.03638e-6 * t * t;

    // Final result: TDB-TT in seconds.
    wt + wf + wj
}

// ---------------------------------------------------------------------------
// Core transform logic
// ---------------------------------------------------------------------------

/// Apply a single conversion step in the forward direction.
fn apply_step_forward(cvt: &TimeConversion, time: f64) -> f64 {
    let args = &cvt.args;
    match cvt.cvt_type {
        TimeCvtType::MjdToMjd | TimeCvtType::MjdToJd | TimeCvtType::JdToMjd => time + args[2],
        TimeCvtType::MjdToBep => pal::epb(time) + args[2],
        TimeCvtType::BepToMjd => pal::epb2d(time) + args[2],
        TimeCvtType::MjdToJep => pal::epj(time) + args[2],
        TimeCvtType::JepToMjd => pal::epj2d(time) + args[2],
        TimeCvtType::TaiToUtc => {
            // TAI->UTC: utc = tai + (UTC-TAI)/SPD
            let delta = if is_bad(args[1]) {
                ast_dat(time + args[0], false)
            } else {
                -args[1]
            };
            time + delta / SPD
        }
        TimeCvtType::UtcToTai => {
            // UTC->TAI: tai = utc + (TAI-UTC)/SPD
            let delta = if is_bad(args[1]) {
                ast_dat(time + args[0], true)
            } else {
                args[1]
            };
            time + delta / SPD
        }
        TimeCvtType::TaiToTt => time + (TTOFF / SPD),
        TimeCvtType::TtToTai => time - (TTOFF / SPD),
        TimeCvtType::TtToTdb => {
            // TT -> TDB: add relativistic correction
            let tt = time + args[0];
            let tai = tt - (TTOFF / SPD);
            let utc = tai
                + if is_bad(args[4]) {
                    ast_dat(tai, false)
                } else {
                    -args[4]
                } / SPD;
            time + rcc(tt, utc, args[1], args[5], args[6]) / SPD
        }
        TimeCvtType::TdbToTt => {
            // TDB -> TT: subtract relativistic correction
            let tdb = time + args[0];
            let tai = tdb - (TTOFF / SPD);
            let utc = tai
                + if is_bad(args[4]) {
                    ast_dat(tai, false)
                } else {
                    -args[4]
                } / SPD;
            time - rcc(tdb, utc, args[1], args[5], args[6]) / SPD
        }
        TimeCvtType::TtToTcg => {
            // TT -> TCG: C adds the correction as one term (`+=`), which rounds once
            time + (time * LG + args[1])
        }
        TimeCvtType::TcgToTt => {
            // TCG -> TT: tt = (tcg - offset) / (1 + LG)
            (time - args[1]) / (1.0 + LG)
        }
        TimeCvtType::TdbToTcb => {
            // TDB -> TCB: C adds the correction as one term (`+=`), which rounds once
            time + (time * LB + args[1])
        }
        TimeCvtType::TcbToTdb => {
            // TCB -> TDB: tdb = (tcb - offset) / (1 + LB)
            (time - args[1]) / (1.0 + LB)
        }
        TimeCvtType::UtToGmst => gmsta(time, args[0], true),
        TimeCvtType::GmstToUt => gmsta(time, args[0], false),
        TimeCvtType::GmstToLmst => {
            // GMST -> LMST: subtract longitude/2pi
            time - args[1] / D2PI
        }
        TimeCvtType::LmstToGmst => {
            // LMST -> GMST: add longitude/2pi
            time + args[1] / D2PI
        }
        TimeCvtType::LmstToLast => {
            // LMST -> LAST: add equation of equinoxes
            // Need to estimate TDB for eqeqx: convert LMST->GMST->UT1->~TDB
            let gmstx = time + args[1] / D2PI;
            let tdb = gmsta(gmstx, args[0], false) + args[0] + (32.0 + TTOFF) / SPD;
            time + pal::eqeqx(tdb) / D2PI
        }
        TimeCvtType::LastToLmst => {
            // LAST -> LMST: subtract equation of equinoxes
            let gmstx = time + args[1] / D2PI;
            let tdb = gmsta(gmstx, args[0], false) + args[0] + (32.0 + TTOFF) / SPD;
            time - pal::eqeqx(tdb) / D2PI
        }
        TimeCvtType::UtToUtc => {
            // UT1 -> UTC: subtract DUT1/86400
            time - args[0] / 86400.0
        }
        TimeCvtType::UtcToUt => {
            // UTC -> UT1: add DUT1/86400
            time + args[0] / 86400.0
        }
        TimeCvtType::LtToUtc => {
            // LT -> UTC: subtract LTOFF/24 (hours to days)
            time - args[0] / 24.0
        }
        TimeCvtType::UtcToLt => {
            // UTC -> LT: add LTOFF/24 (hours to days)
            time + args[0] / 24.0
        }
    }
}

/// Apply a single conversion step in the inverse direction.
fn apply_step_inverse(cvt: &TimeConversion, time: f64) -> f64 {
    let args = &cvt.args;
    match cvt.cvt_type {
        TimeCvtType::MjdToMjd | TimeCvtType::MjdToJd | TimeCvtType::JdToMjd => time - args[2],
        TimeCvtType::MjdToBep => pal::epb2d(time) + args[3],
        TimeCvtType::BepToMjd => pal::epb(time) + args[3],
        TimeCvtType::MjdToJep => pal::epj2d(time) + args[3],
        TimeCvtType::JepToMjd => pal::epj(time) + args[3],
        TimeCvtType::TaiToUtc => {
            // Inverse of TAI->UTC is UTC->TAI
            let delta = if is_bad(args[1]) {
                ast_dat(time + args[0], true)
            } else {
                args[1]
            };
            time + delta / SPD
        }
        TimeCvtType::UtcToTai => {
            // Inverse of UTC->TAI is TAI->UTC
            let delta = if is_bad(args[1]) {
                ast_dat(time + args[0], false)
            } else {
                -args[1]
            };
            time + delta / SPD
        }
        TimeCvtType::TaiToTt => time - (TTOFF / SPD),
        TimeCvtType::TtToTai => time + (TTOFF / SPD),
        TimeCvtType::TtToTdb => {
            // Inverse of TT->TDB is TDB->TT
            let tdb = time + args[0];
            let tai = tdb - (TTOFF / SPD);
            let utc = tai
                + if is_bad(args[4]) {
                    ast_dat(tai, false)
                } else {
                    -args[4]
                } / SPD;
            time - rcc(tdb, utc, args[1], args[5], args[6]) / SPD
        }
        TimeCvtType::TdbToTt => {
            // Inverse of TDB->TT is TT->TDB
            let tt = time + args[0];
            let tai = tt - (TTOFF / SPD);
            let utc = tai
                + if is_bad(args[4]) {
                    ast_dat(tai, false)
                } else {
                    -args[4]
                } / SPD;
            time + rcc(tt, utc, args[1], args[5], args[6]) / SPD
        }
        TimeCvtType::TtToTcg => {
            // Inverse of TT->TCG is TCG->TT
            (time - args[1]) / (1.0 + LG)
        }
        TimeCvtType::TcgToTt => {
            // Inverse of TCG->TT is TT->TCG
            time + (time * LG + args[1])
        }
        TimeCvtType::TdbToTcb => {
            // Inverse of TDB->TCB is TCB->TDB
            (time - args[1]) / (1.0 + LB)
        }
        TimeCvtType::TcbToTdb => {
            // Inverse of TCB->TDB is TDB->TCB
            time + (time * LB + args[1])
        }
        TimeCvtType::UtToGmst => gmsta(time, args[0], false),
        TimeCvtType::GmstToUt => gmsta(time, args[0], true),
        TimeCvtType::GmstToLmst => time + args[1] / D2PI,
        TimeCvtType::LmstToGmst => time - args[1] / D2PI,
        TimeCvtType::LmstToLast => {
            // Inverse of LMST->LAST is LAST->LMST
            let gmstx = time + args[1] / D2PI;
            let tdb = gmsta(gmstx, args[0], false) + args[0] + (32.0 + TTOFF) / SPD;
            time - pal::eqeqx(tdb) / D2PI
        }
        TimeCvtType::LastToLmst => {
            // Inverse of LAST->LMST is LMST->LAST
            let gmstx = time + args[1] / D2PI;
            let tdb = gmsta(gmstx, args[0], false) + args[0] + (32.0 + TTOFF) / SPD;
            time + pal::eqeqx(tdb) / D2PI
        }
        TimeCvtType::UtToUtc => time + args[0] / 86400.0,
        TimeCvtType::UtcToUt => time - args[0] / 86400.0,
        TimeCvtType::LtToUtc => time + args[0] / 24.0,
        TimeCvtType::UtcToLt => time - args[0] / 24.0,
    }
}

// ---------------------------------------------------------------------------
// Step normalization and simplification (matching C timemap.c:2099-2428)
// ---------------------------------------------------------------------------

/// Return the normalized step list for a TimeMap, accounting for inversion.
///
/// When inverted, reverses the step order and inverts each step type.
/// For MJD-offset conversions (MjdToMjd, MjdToJd/JdToMjd, MjdToBep/BepToMjd,
/// MjdToJep/JepToMjd), inversion swaps the two offset arguments.
fn time_normalized_steps(tm: &TimeMap, inv: bool) -> Vec<TimeConversion> {
    if !inv {
        return tm.conversions.clone();
    }
    tm.conversions
        .iter()
        .rev()
        .map(|cvt| {
            let inv_type = cvt.cvt_type.inverse();
            let needs_swap = matches!(
                cvt.cvt_type,
                TimeCvtType::MjdToMjd
                    | TimeCvtType::MjdToJd
                    | TimeCvtType::JdToMjd
                    | TimeCvtType::MjdToBep
                    | TimeCvtType::BepToMjd
                    | TimeCvtType::MjdToJep
                    | TimeCvtType::JepToMjd
            );
            let nargs = inv_type.nargs();
            let szargs = inv_type.szargs();
            let mut new_args = vec![BAD_VALUE; szargs];
            if needs_swap && cvt.args.len() >= 2 {
                // Swap user args [0] and [1] for offset-based conversions.
                new_args[0] = cvt.args[1];
                new_args[1] = cvt.args[0];
            } else {
                // Copy user args as-is.
                let copy_len = nargs.min(cvt.args.len());
                new_args[..copy_len].copy_from_slice(&cvt.args[..copy_len]);
            }
            // Recompute derived arguments.
            add_args(inv_type, &mut new_args);
            TimeConversion {
                cvt_type: inv_type,
                args: new_args,
            }
        })
        .collect()
}

/// Check whether two adjacent normalized steps cancel each other.
fn time_steps_cancel(a: &TimeConversion, b: &TimeConversion) -> bool {
    let at = a.cvt_type;
    let bt = b.cvt_type;

    // Types must form an inverse pair.
    if at.inverse() != bt {
        return false;
    }

    // For offset-based MJD conversions, the inverse swaps args[0] and args[1].
    // MjdToMjd(a,b) followed by MjdToMjd(b,a) cancels.
    // MjdToJd(a,b) followed by JdToMjd(b,a) cancels. Etc.
    let needs_swap = matches!(
        (at, bt),
        (TimeCvtType::MjdToMjd, TimeCvtType::MjdToMjd)
            | (TimeCvtType::MjdToJd, TimeCvtType::JdToMjd)
            | (TimeCvtType::JdToMjd, TimeCvtType::MjdToJd)
            | (TimeCvtType::MjdToBep, TimeCvtType::BepToMjd)
            | (TimeCvtType::BepToMjd, TimeCvtType::MjdToBep)
            | (TimeCvtType::MjdToJep, TimeCvtType::JepToMjd)
            | (TimeCvtType::JepToMjd, TimeCvtType::MjdToJep)
    );

    if needs_swap {
        return a.args.len() >= 2
            && b.args.len() >= 2
            && ast_equal(a.args[0], b.args[1])
            && ast_equal(a.args[1], b.args[0]);
    }

    // For all other inverse pairs, user args must match in same order.
    let na = at.nargs();
    a.args
        .iter()
        .take(na)
        .zip(b.args.iter().take(na))
        .all(|(x, y)| ast_equal(*x, *y))
}

/// Check whether a step is a no-effect operation that can be removed.
///
/// Currently recognizes: MjdToMjd where args[0] == args[1] (zero net offset).
fn time_step_is_no_effect(cvt: &TimeConversion) -> bool {
    // C tests the derived net offset in `args[2]` rather than the two supplied
    // offsets, and applies the rule to all three change-of-system conversions
    // (timemap.c:2260-2266). For MjdToMjd that third value is
    // `args[0] - args[1]`, so the two tests agree; for the other two it carries
    // the ±2400000.5 constant, so an offset pair that absorbs the constant is a
    // no-op only `args[2]` can see.
    matches!(
        cvt.cvt_type,
        TimeCvtType::MjdToMjd | TimeCvtType::MjdToJd | TimeCvtType::JdToMjd
    ) && cvt.args.len() >= 3
        && ast_equal(cvt.args[2], 0.0)
}

/// Simplify a step list by cancelling adjacent inverse pairs and removing
/// no-effect steps.
///
/// Loops until no further simplifications are found.
/// Returns `true` if any steps were eliminated.
fn time_simplify_steps(steps: &mut Vec<TimeConversion>) -> bool {
    let mut any_changed = false;
    loop {
        let mut changed_this_pass = false;

        // Cancel adjacent inverse pairs.
        let mut i = 0;
        while i + 1 < steps.len() {
            if time_steps_cancel(&steps[i], &steps[i + 1]) {
                steps.remove(i + 1);
                steps.remove(i);
                changed_this_pass = true;
                any_changed = true;
                // Don't advance — new neighbors may cancel.
                i = i.saturating_sub(1);
                continue;
            }
            i += 1;
        }

        // Remove no-effect steps.
        let mut j = 0;
        while j < steps.len() {
            if time_step_is_no_effect(&steps[j]) {
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

/// `map_merge` for a nominated `TimeMap`, mirroring C `timemap.c::MapMerge`.
/// Series only.
///
/// Gathers the run of adjacent `TimeMap`s at `where_`, concatenates their
/// invert-resolved step lists, cancels adjacent inverse pairs, and — per
/// C's `simpler` condition — replaces the run with a single **forward**
/// `TimeMap` (invert cleared) whenever a lone TimeMap was applied inverted,
/// even if no cancellation occurred.
pub(crate) fn time_map_merge(
    ctx: &mut super::simplify::MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    if !series {
        return super::simplify::default_map_merge(ctx, where_, series);
    }

    let is_time = |entry: &(DynMapping, bool, bool)| entry.0.as_any().is::<TimeMap>();

    let n = ctx.len();
    let mut imap1 = where_;
    while imap1 >= 1 && is_time(ctx.entry(imap1 - 1)) {
        imap1 -= 1;
    }
    let mut imap2 = where_;
    while imap2 + 1 < n && is_time(ctx.entry(imap2 + 1)) {
        imap2 += 1;
    }

    let mut steps: Vec<TimeConversion> = Vec::new();
    let mut nstep0 = 0usize;
    let where_inv = ctx.entry(where_).1;
    for idx in imap1..=imap2 {
        let entry = ctx.entry(idx);
        let tm = entry.0.as_any().downcast_ref::<TimeMap>()?;
        let eff_inv = entry.1 != tm.inverted;
        let s = time_normalized_steps(tm, eff_inv);
        nstep0 += s.len();
        steps.extend(s);
    }

    let ngone = imap2 - imap1;
    time_simplify_steps(&mut steps);
    let unit = steps.is_empty();
    let simpler = ngone > 0 || steps.len() < nstep0 || unit || where_inv;
    if !simpler {
        return super::simplify::default_map_merge(ctx, where_, series);
    }

    let new: DynMapping = if unit {
        Box::new(UnitMap::new(1))
    } else {
        let mut result = TimeMap::new();
        result.conversions = steps;
        Box::new(result)
    };
    ctx.replace_range_with(imap1, imap2, new);
    Some(imap1)
}

// ---------------------------------------------------------------------------
// Mapping trait
// ---------------------------------------------------------------------------

impl Mapping for TimeMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    fn nin(&self) -> usize {
        1
    }

    fn nout(&self) -> usize {
        1
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    /// NaN-as-bad fast path. TimeMap's `apply_step_forward`/`apply_step_inverse`
    /// helpers each emit `BAD_VALUE` for several intermediate failure
    /// conditions (invalid arguments to log/asin/etc. in specific time-system
    /// conversions). Auditing each step is out of scope; delegate to
    /// `transform_into` and post-process `BAD_VALUE → NaN` so downstream
    /// NaN-aware stages see the protocol-canonical sentinel. TimeMap is 1D,
    /// so the post-pass is N comparisons.
    fn transform_into_nan(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        let npoint = pts.npoint();
        self.transform_into(pts, forward, out.reborrow())?;
        crate::mappings::bad_to_nan(self, forward, npoint, &mut out);
        Ok(())
    }

    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        let fwd = self.effective_forward(forward);

        let n = pts.npoint();
        for (o, &t) in out.axis_mut(0)[..n].iter_mut().zip(pts.axis(0)) {
            let mut time = t;

            if is_bad(time) {
                *o = BAD_VALUE;
                continue;
            }

            if fwd {
                for cvt in &self.conversions {
                    if is_bad(time) {
                        break;
                    }
                    time = apply_step_forward(cvt, time);
                }
            } else {
                for cvt in self.conversions.iter().rev() {
                    if is_bad(time) {
                        break;
                    }
                    time = apply_step_inverse(cvt, time);
                }
            }

            *o = time;
        }

        Ok(())
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        // timemap.c Rate: the product of each conversion's slope. The epoch
        // conversions are linear; GMST is not, so its presence defers to the
        // parent's numeric rate. Heuristic, as in C: every other time-scale
        // conversion is taken to have unit slope, which it has only nearly.
        let mut result = 1.0;
        for conversion in &self.conversions {
            match conversion.cvt_type {
                TimeCvtType::MjdToBep => result *= 1.0 / 365.242198781,
                TimeCvtType::BepToMjd => result *= 365.242198781,
                TimeCvtType::MjdToJep => result *= 1.0 / 365.25,
                TimeCvtType::JepToMjd => result *= 365.25,
                TimeCvtType::UtToGmst | TimeCvtType::GmstToUt => {
                    return numeric_rate(self, at, ax1, ax2);
                }
                _ => {}
            }
        }
        // The product is the forward slope, so an inverted TimeMap's is its
        // reciprocal.
        Ok(if self.inverted { 1.0 / result } else { result })
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        time_map_merge(ctx, where_, series)
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
        let other_tm = other.as_any().downcast_ref::<TimeMap>()?;

        // Normalize: account for both the mapping's own inverted flag
        // AND the external inversion from the simplification engine.
        let self_steps = time_normalized_steps(self, self_inv != self.inverted);
        let other_steps = time_normalized_steps(other_tm, other_inv != other_tm.inverted);

        // Concatenate: self is applied first, then other.
        let mut steps = self_steps;
        steps.extend(other_steps);

        // Always merge (concatenate) even if no simplification occurs.
        // This matches C timemap.c behavior where all adjacent TimeMaps are
        // combined, enabling further simplification in subsequent passes.
        time_simplify_steps(&mut steps);

        if steps.is_empty() {
            Some(Box::new(UnitMap::new(1)))
        } else {
            let mut result = TimeMap::new();
            result.conversions = steps;
            Some(Box::new(result))
        }
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<TimeMap>() else {
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
// Serialization
// ---------------------------------------------------------------------------

/// Argument index to letter: 0 -> 'a', 1 -> 'b', etc.
fn arg_letter(j: usize) -> char {
    (b'a' + j as u8) as char
}

impl Serializable for TimeMap {
    fn class_name(&self) -> &'static str {
        "TimeMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Conversion between time coordinate systems".to_string());

        // Mapping base attributes.
        write_mapping_base(&mut node, self);

        let ntime = self.conversions.len();
        if ntime > 0 {
            node.push_int(
                "Ntime",
                ntime as i64,
                Some("Number of conversion steps".to_string()),
            );

            for (i, cvt) in self.conversions.iter().enumerate() {
                let idx = i + 1; // 1-based
                node.push_string(
                    &format!("Time{idx}"),
                    cvt.cvt_type.code_string(),
                    Some(cvt.cvt_type.description().to_string()),
                );
                // Write all szargs arguments.
                // For TT<->TDB, the external order is different from internal:
                // external [0,1,2,3,5,6,4] maps to internal [0,1,2,3,4,5,6]
                let order: Option<&[usize]> = match cvt.cvt_type {
                    TimeCvtType::TtToTdb | TimeCvtType::TdbToTt => Some(&[0, 1, 2, 3, 5, 6, 4]),
                    _ => None,
                };
                let szargs = cvt.cvt_type.szargs();
                let arg_descs = cvt.cvt_type.arg_descriptions();
                for ext_j in 0..szargs {
                    let int_j = order.map_or(ext_j, |o| o[ext_j]);
                    let val = cvt.args[int_j];
                    if !is_bad(val) {
                        let letter = arg_letter(ext_j);
                        let comment = arg_descs
                            .get(ext_j)
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string());
                        node.push_double(&format!("Time{idx}{letter}"), val, comment);
                    }
                }
            }
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("TimeMap");
        let base = read_mapping_base(whole);

        let ntime = node.get_int("Ntime", 0) as usize;

        let mut tm = TimeMap::new();

        for i in 0..ntime {
            let idx = i + 1;
            let Some(code_str) = node.get_string_if_set(&format!("Time{idx}")) else {
                return Err(AstError::new(
                    ErrorCode::Badin,
                    "astRead(Channel): A time coordinate conversion type is missing from \
                     the input TimeMap data.",
                ));
            };
            let cvt_type = TimeCvtType::from_code(&code_str).ok_or_else(|| {
                AstError::new(
                    ErrorCode::Badin,
                    format!(
                        "astRead(Channel): Invalid time conversion type \"{code_str}\" in \
                         TimeMap data."
                    ),
                )
            })?;

            // Every argument, derived ones included, as written and without
            // recomputing them (timemap.c Load). For TT<->TDB the written
            // order puts DTAI last: written argument j is internal argument
            // order[j].
            let order: Option<&[usize]> = match cvt_type {
                TimeCvtType::TtToTdb | TimeCvtType::TdbToTt => Some(&[0, 1, 2, 3, 5, 6, 4]),
                _ => None,
            };
            let mut args = vec![BAD_VALUE; cvt_type.szargs()];
            for ext_j in 0..args.len() {
                let int_j = order.map_or(ext_j, |o| o[ext_j]);
                args[int_j] =
                    node.get_double(&format!("Time{idx}{}", arg_letter(ext_j)), BAD_VALUE);
            }
            tm.conversions.push(TimeConversion { cvt_type, args });
        }

        if base.inverted {
            tm.invert();
        }

        tm.set_has_been_simplified(base.has_been_simplified);

        Ok(tm)
    }
}
