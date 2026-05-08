//! Mapping that chains time coordinate system conversions.

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

/// 1 AU in metres.
const AST_AU: f64 = 1.49597870e11;

// ---------------------------------------------------------------------------
// TimeCvtType — conversion step types
// ---------------------------------------------------------------------------

/// A time coordinate conversion type supported by [`TimeMap`].
///
/// Each variant corresponds to a specific time system conversion operation.
/// The integer discriminant matches the C AST library's `AST__*` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeCvtType {
    /// Convert MJD between offsets.
    MjdToMjd = 1,
    /// Convert MJD to JD.
    MjdToJd = 2,
    /// Convert JD to MJD.
    JdToMjd = 3,
    /// Convert MJD to Besselian epoch.
    MjdToBep = 4,
    /// Convert Besselian epoch to MJD.
    BepToMjd = 5,
    /// Convert MJD to Julian epoch.
    MjdToJep = 6,
    /// Convert Julian epoch to MJD.
    JepToMjd = 7,
    /// Convert TAI to UTC.
    TaiToUtc = 8,
    /// Convert UTC to TAI.
    UtcToTai = 9,
    /// Convert TAI to TT.
    TaiToTt = 10,
    /// Convert TT to TAI.
    TtToTai = 11,
    /// Convert TDB to TT.
    TdbToTt = 12,
    /// Convert TT to TDB.
    TtToTdb = 13,
    /// Convert TCG to TT.
    TcgToTt = 14,
    /// Convert TT to TCG.
    TtToTcg = 15,
    /// Convert TCB to TDB.
    TcbToTdb = 16,
    /// Convert TDB to TCB.
    TdbToTcb = 17,
    /// Convert UT to GMST.
    UtToGmst = 18,
    /// Convert GMST to UT.
    GmstToUt = 19,
    /// Convert GMST to LMST.
    GmstToLmst = 20,
    /// Convert LMST to GMST.
    LmstToGmst = 21,
    /// Convert LAST to LMST.
    LastToLmst = 22,
    /// Convert LMST to LAST.
    LmstToLast = 23,
    /// Convert UT1 to UTC.
    UtToUtc = 24,
    /// Convert UTC to UT1.
    UtcToUt = 25,
    /// Convert Local Time to UTC.
    LtToUtc = 26,
    /// Convert UTC to Local Time.
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

    /// Uppercase code string for serialisation (e.g. `"MJDTOMJD"`, `"TAITOUTC"`).
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

    /// Parse a code string (case-insensitive) into a conversion type.
    pub fn from_code(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
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

    /// Construct from the integer code used in the C library.
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
            10 => Some(Self::TaiToTt),
            11 => Some(Self::TtToTai),
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

    /// Human-readable descriptions for each serialised argument slot.
    ///
    /// The slice length matches `szargs()`. Used as comments in the Native
    /// serialisation format. Derived (not stored) slots use empty strings.
    pub fn arg_descriptions(&self) -> &'static [&'static str] {
        match self {
            Self::MjdToMjd | Self::MjdToJd | Self::JdToMjd => {
                &["Zero point for input", "Zero point for output", ""]
            }
            Self::MjdToBep | Self::BepToMjd | Self::MjdToJep | Self::JepToMjd => {
                &["Zero point for input", "Zero point for output", "", ""]
            }
            Self::TaiToUtc | Self::UtcToTai => &["UTC offset", "TAI offset"],
            Self::TaiToTt | Self::TtToTai => &["MJD offset"],
            Self::TcgToTt | Self::TtToTcg => &["MJD offset", "TCG offset"],
            Self::TcbToTdb | Self::TdbToTcb => &["MJD offset", "TCB offset"],
            Self::TdbToTt | Self::TtToTdb => &[
                "MJD offset",
                "Observer longitude",
                "Observer latitude",
                "Observer altitude",
                "DTAI",
                "",
                "",
            ],
            Self::UtToGmst | Self::GmstToUt => &["MJD offset"],
            Self::GmstToLmst | Self::LmstToGmst => {
                &["MJD offset", "Observer longitude", "Observer latitude"]
            }
            Self::LastToLmst | Self::LmstToLast => {
                &["MJD offset", "Observer longitude", "Observer latitude"]
            }
            Self::UtToUtc | Self::UtcToUt | Self::LtToUtc | Self::UtcToLt => &["MJD offset"],
        }
    }

    /// Human-readable description for serialisation comments.
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

/// A mapping that chains time coordinate system conversions.
///
/// `TimeMap` applies a sequence of time-system transformations (e.g.
/// MJD↔JD, UTC↔TAI, TT↔TDB, etc.). The input and output are each a
/// single coordinate representing time, typically expressed as an offset
/// in days from some MJD zero-point.
///
/// # Example
///
/// ```
/// use ast_core::mappings::{Mapping, TimeMap, TimeCvtType};
/// use ast_core::coords::PointSet;
///
/// let mut tm = TimeMap::new();
/// // Convert MJD to Julian epoch (J2000.0 = MJD 51544.5)
/// tm.add(TimeCvtType::MjdToJep, &[0.0, 0.0]);
///
/// let pts = PointSet::from_rows([[51544.5_f64]]);
/// let out = tm.transform(&pts, true).unwrap();
/// assert!((out.point(0)[0] - 2000.0).abs() < 1.0e-10);
/// ```
#[derive(Debug, Clone)]
pub struct TimeMap {
    conversions: Vec<TimeConversion>,
    inverted: bool,
    do_not_simplify: bool,
}

impl TimeMap {
    /// Create a new, empty `TimeMap` (identity transform).
    pub fn new() -> Self {
        Self {
            conversions: Vec::new(),
            inverted: false,
            do_not_simplify: false,
        }
    }

    /// Append a conversion step to the chain.
    ///
    /// `args` must have at least `cvt_type.nargs()` elements.
    /// Extra elements are silently ignored.
    pub fn add(&mut self, cvt_type: TimeCvtType, args: &[f64]) {
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
            -(4.2131700 + (mjd - 39126.0) * 0.002592) / 1.02592
        } else if mjd >= 39126.0 + (4.3131700 + (39126.0 - 39126.0) * 0.002592) / SPD {
            -(4.2131700 + (mjd - 39126.0) * 0.002592) / 1.02592
        } else if mjd >= 39004.0 + (3.8401300 + (39004.0 - 38761.0) * 0.001296) / SPD {
            -(3.8401300 + (mjd - 38761.0) * 0.001296) / 1.001296
        } else if mjd >= 38942.0 + (3.7401300 + (38942.0 - 38761.0) * 0.001296) / SPD {
            -(3.7401300 + (mjd - 38761.0) * 0.001296) / 1.01296
        } else if mjd >= 38820.0 + (3.6401300 + (38820.0 - 38761.0) * 0.001296) / SPD {
            -(3.6401300 + (mjd - 38761.0) * 0.001296) / 1.001296
        } else if mjd >= 38761.0 + (3.5401300 + (38761.0 - 38761.0) * 0.001296) / SPD {
            -(3.5401300 + (mjd - 38761.0) * 0.001296) / 1.001296
        } else if mjd >= 38639.0 + (3.4401300 + (38639.0 - 38761.0) * 0.001296) / SPD {
            -(3.4401300 + (mjd - 38761.0) * 0.001296) / 1.001296
        } else if mjd >= 38486.0 + (3.3401300 + (38486.0 - 38761.0) * 0.001296) / SPD {
            -(3.3401300 + (mjd - 38761.0) * 0.001296) / 1.001296
        } else if mjd >= 38395.0 + (3.2401300 + (38395.0 - 38761.0) * 0.001296) / SPD {
            -(3.2401300 + (mjd - 38761.0) * 0.001296) / 1.001296
        } else if mjd >= 38334.0 + (1.9458580 + (38334.0 - 37665.0) * 0.0011232) / SPD {
            -(1.9458580 + (mjd - 37665.0) * 0.0011232) / 1.0011232
        } else if mjd >= 37665.0 + (1.8458580 + (37665.0 - 37665.0) * 0.0011232) / SPD {
            -(1.8458580 + (mjd - 37665.0) * 0.0011232) / 1.0011232
        } else if mjd >= 37512.0 + (1.3728180 + (37512.0 - 37300.0) * 0.001296) / SPD {
            -(1.3728180 + (mjd - 37300.0) * 0.001296) / 1.001296
        } else if mjd >= 37300.0 + (1.4228180 + (37300.0 - 37300.0) * 0.001296) / SPD {
            -(1.4228180 + (mjd - 37300.0) * 0.001296) / 1.001296
        } else {
            -(1.4178180 + (mjd - 37300.0) * 0.001296) / 1.001296
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

    let wt = 0.00029e-10 * u * (tsol + elsun - els).sin()
        + 0.00100e-10 * u * (tsol - 2.0 * emsun).sin()
        + 0.00133e-10 * u * (tsol - d).sin()
        + 0.00133e-10 * u * (tsol + elsun - elj).sin()
        - 0.00229e-10 * u * (tsol + 2.0 * elsun + emsun).sin()
        - 0.0220e-10 * v * (elsun + emsun).cos()
        + 0.05312e-10 * u * (tsol - emsun).sin()
        - 0.13677e-10 * u * (tsol + 2.0 * elsun).sin()
        - 1.3184e-10 * v * elsun.cos()
        + 3.17679e-10 * u * tsol.sin();

    // ---- Fairhead model ----
    // t**0
    let mut w0 = 0.0;
    for i in (0..=473).rev() {
        w0 += FAIRHD[i][0] * (FAIRHD[i][1] * t + FAIRHD[i][2]).sin();
    }

    // t**1
    let mut w1 = 0.0;
    for i in (474..=678).rev() {
        w1 += FAIRHD[i][0] * (FAIRHD[i][1] * t + FAIRHD[i][2]).sin();
    }

    // t**2
    let mut w2 = 0.0;
    for i in (679..=763).rev() {
        w2 += FAIRHD[i][0] * (FAIRHD[i][1] * t + FAIRHD[i][2]).sin();
    }

    // t**3
    let mut w3 = 0.0;
    for i in (764..=783).rev() {
        w3 += FAIRHD[i][0] * (FAIRHD[i][1] * t + FAIRHD[i][2]).sin();
    }

    // t**4
    let mut w4 = 0.0;
    for i in (784..=786).rev() {
        w4 += FAIRHD[i][0] * (FAIRHD[i][1] * t + FAIRHD[i][2]).sin();
    }

    // Multiply by powers of T and combine.
    let wf = t * (t * (t * (t * w4 + w3) + w2) + w1) + w0;

    // Adjustments to use JPL planetary masses instead of IAU.
    let wj = 0.00065e-6 * (6069.776754 * t + 4.021194).sin()
        + 0.00033e-6 * (213.299095 * t + 5.543132).sin()
        + (-0.00196e-6 * (6208.294251 * t + 5.696701).sin())
        + (-0.00173e-6 * (74.781599 * t + 2.435900).sin())
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
            // TT -> TCG: tcg = tt + tt*LG + offset
            time + time * LG + args[1]
        }
        TimeCvtType::TcgToTt => {
            // TCG -> TT: tt = (tcg - offset) / (1 + LG)
            (time - args[1]) / (1.0 + LG)
        }
        TimeCvtType::TdbToTcb => {
            // TDB -> TCB: tcb = tdb + tdb*LB + offset
            time + time * LB + args[1]
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
            time + time * LG + args[1]
        }
        TimeCvtType::TdbToTcb => {
            // Inverse of TDB->TCB is TCB->TDB
            (time - args[1]) / (1.0 + LB)
        }
        TimeCvtType::TcbToTdb => {
            // Inverse of TCB->TDB is TDB->TCB
            time + time * LB + args[1]
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
    if cvt.cvt_type == TimeCvtType::MjdToMjd && cvt.args.len() >= 2 {
        return ast_equal(cvt.args[0], cvt.args[1]);
    }
    false
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
                // Don't advance — new neighbours may cancel.
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

// ---------------------------------------------------------------------------
// Mapping trait
// ---------------------------------------------------------------------------

impl Mapping for TimeMap {
    fn nin(&self) -> usize {
        1
    }

    fn nout(&self) -> usize {
        1
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let mut out = PointSet::zeros(pts.npoint(), 1);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);

        for i in 0..pts.npoint() {
            let mut time = pts.point(i)[0];

            if is_bad(time) {
                out.point_mut(i)[0] = BAD_VALUE;
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

            out.point_mut(i)[0] = time;
        }

        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.do_not_simplify {
            return Ok(Box::new(self.clone()));
        }

        // Normalize steps (accounting for the inverted flag) and simplify
        // (cancel adjacent inverse pairs, remove no-effect steps).
        let mut steps = time_normalized_steps(self, self.inverted);
        let changed = time_simplify_steps(&mut steps);

        if !changed && !self.inverted {
            // No simplification possible — return self unchanged.
            return Ok(Box::new(self.clone()));
        }

        if steps.is_empty() {
            Ok(Box::new(UnitMap::new(1)))
        } else {
            let mut tm = TimeMap::new();
            tm.conversions = steps;
            Ok(Box::new(tm))
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
        let other_tm = other.as_any().downcast_ref::<TimeMap>()?;

        // Normalize: account for both the mapping's own inverted flag
        // AND the external inversion from the simplification engine.
        let self_steps = time_normalized_steps(self, self_inv != self.inverted);
        let other_steps = time_normalized_steps(other_tm, other_inv != other_tm.inverted);

        // Concatenate: self is applied first, then other.
        let mut steps = self_steps;
        steps.extend(other_steps);

        // Always merge (concatenate) even if no simplification occurs.
        // This matches C timemap.c behaviour where all adjacent TimeMaps are
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

    fn do_not_simplify(&self) -> bool {
        self.do_not_simplify
    }

    fn set_do_not_simplify(&mut self, flag: bool) {
        self.do_not_simplify = flag;
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

// ---------------------------------------------------------------------------
// Serialisation
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
        let (_nin, _nout, inverted, do_not_simplify) = read_mapping_base(node);

        let ntime = node.get_int("Ntime", 0) as usize;

        let mut tm = TimeMap::new();

        for i in 0..ntime {
            let idx = i + 1;
            let code_str = node.get_string(&format!("Time{idx}"), "");
            let cvt_type = TimeCvtType::from_code(&code_str).ok_or_else(|| {
                AstError::new(
                    ErrorCode::Timin,
                    format!("unknown TimeMap conversion type: '{code_str}'"),
                )
            })?;

            let nargs = cvt_type.nargs();
            // Read user-supplied arguments from the external format.
            // For most types the first nargs external slots correspond directly
            // to internal slots 0..nargs. For TT<->TDB the external layout
            // re-orders args for backward compatibility (dtai moved to end).
            let mut user_args = vec![BAD_VALUE; nargs];
            match cvt_type {
                TimeCvtType::TtToTdb | TimeCvtType::TdbToTt => {
                    // External: [a]=mjd_off, [b]=obslon, [c]=obslat, [d]=obsalt,
                    //           [e]=u(derived), [f]=v(derived), [g]=dtai
                    // Internal user: [0]=mjd_off, [1]=obslon, [2]=obslat, [3]=obsalt, [4]=dtai
                    user_args[0] = node.get_double(&format!("Time{idx}a"), BAD_VALUE);
                    user_args[1] = node.get_double(&format!("Time{idx}b"), BAD_VALUE);
                    user_args[2] = node.get_double(&format!("Time{idx}c"), BAD_VALUE);
                    user_args[3] = node.get_double(&format!("Time{idx}d"), BAD_VALUE);
                    // dtai is at external position 'g' (index 6)
                    user_args[4] = node.get_double(&format!("Time{idx}g"), BAD_VALUE);
                }
                _ => {
                    for (j, arg) in user_args.iter_mut().enumerate() {
                        let letter = arg_letter(j);
                        *arg = node.get_double(&format!("Time{idx}{letter}"), BAD_VALUE);
                    }
                }
            }

            // Use `add` which recomputes derived values from user args.
            tm.add(cvt_type, &user_args);
        }

        tm.set_do_not_simplify(do_not_simplify);
        if inverted {
            tm.invert();
        }

        Ok(tm)
    }
}
