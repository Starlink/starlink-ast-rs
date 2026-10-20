//! Mapping that chains spectral coordinate system conversions.

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

/// Speed of light in vacuum (m/s).
const C_LIGHT: f64 = 2.99792458e8;

/// Planck's constant (J·s).
const H_PLANCK: f64 = 6.6260755e-34;

/// Conversion factor: AU/s to m/s.
const AU_TO_MS: f64 = 149.597870e9;

/// Conversion factor: AU/s to km/s.
const AU_TO_KMS: f64 = 149.597870e6;

// ---------------------------------------------------------------------------
// SpecCvtType — conversion step types
// ---------------------------------------------------------------------------

/// A spectral coordinate conversion that [`SpecMap::add`] can add to a
/// [`SpecMap`].
///
/// The arguments a conversion takes are listed with each variant, in the
/// order they are passed to [`SpecMap::add`].
/// [`nargs`](SpecCvtType::nargs) gives the number of arguments,
/// [`code_string`](SpecCvtType::code_string) the conversion's string code and
/// [`from_code`](SpecCvtType::from_code) parses one, case-insensitively.
///
/// The values the conversions process have these units:
///
/// - all velocities: meters per second, positive if the source recedes from
///   the observer;
/// - frequency: Hertz;
/// - all wavelengths: meters;
/// - energy: Joules;
/// - wave number: cycles per meter.
///
/// The arguments are:
///
/// - `RF`: rest frequency (Hz);
/// - `OBSALT`: geodetic altitude of the observer (IAU 1975, meters);
/// - `OBSLAT`: geodetic latitude of the observer (IAU 1975, radians);
/// - `OBSLON`: longitude of the observer (radians, positive eastwards);
/// - `EPOCH`: epoch of observation (UT1 as a Modified Julian Date);
/// - `RA`: right ascension of the source (radians, FK5 J2000);
/// - `DEC`: declination of the source (radians, FK5 J2000);
/// - `VOFF`: velocity of the user-defined reference frame towards the
///   position given by `RA` and `DEC`, measured in the heliocentric reference
///   frame (meters per second).
///
/// In a 3-dimensional SpecMap ([`SpecMap::with_nin`]) the source position of
/// each point is given by its second and third coordinates instead of by
/// `RA` and `DEC`.
/// Usable `RA` and `DEC` values are still needed to define the user-defined
/// reference frame of [`UsF2Hl`](SpecCvtType::UsF2Hl) and
/// [`HlF2Us`](SpecCvtType::HlF2Us), but [`BAD_VALUE`] may be given for them
/// when that frame is not needed.
///
/// The integer discriminant is the conversion's integer code in the C
/// library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecCvtType {
    /// `FRTOVL` (`RF`): convert frequency to relativistic velocity.
    FrToVl = 1,
    /// `VLTOFR` (`RF`): convert relativistic velocity to frequency.
    VlToFr = 2,
    /// `ENTOFR`: convert energy to frequency.
    EnToFr = 3,
    /// `FRTOEN`: convert frequency to energy.
    FrToEn = 4,
    /// `WNTOFR`: convert wave number to frequency.
    WnToFr = 5,
    /// `FRTOWN`: convert frequency to wave number.
    FrToWn = 6,
    /// `WVTOFR`: convert wavelength (vacuum) to frequency.
    WvToFr = 7,
    /// `FRTOWV`: convert frequency to wavelength (vacuum).
    FrToWv = 8,
    /// `AWTOFR`: convert wavelength (air) to frequency.
    AwToFr = 9,
    /// `FRTOAW`: convert frequency to wavelength (air).
    FrToAw = 10,
    /// `VRTOVL`: convert radio to relativistic velocity.
    VrToVl = 11,
    /// `VLTOVR`: convert relativistic to radio velocity.
    VlToVr = 12,
    /// `VOTOVL`: convert optical to relativistic velocity.
    VoToVl = 13,
    /// `VLTOVO`: convert relativistic to optical velocity.
    VlToVo = 14,
    /// `ZOTOVL`: convert redshift to relativistic velocity.
    ZoToVl = 15,
    /// `VLTOZO`: convert relativistic velocity to redshift.
    VlToZo = 16,
    /// `BTTOVL`: convert beta factor to relativistic velocity.
    BtToVl = 17,
    /// `VLTOBT`: convert relativistic velocity to beta factor.
    VlToBt = 18,
    /// `USF2HL` (`VOFF`, `RA`, `DEC`): convert frequency from a user-defined
    /// reference frame to heliocentric.
    UsF2Hl = 19,
    /// `HLF2US` (`VOFF`, `RA`, `DEC`): convert frequency from heliocentric
    /// reference frame to user-defined.
    HlF2Us = 20,
    /// `TPF2HL` (`OBSLON`, `OBSLAT`, `OBSALT`, `EPOCH`, `RA`, `DEC`): convert
    /// frequency from topocentric reference frame to heliocentric.
    TpF2Hl = 21,
    /// `HLF2TP` (`OBSLON`, `OBSLAT`, `OBSALT`, `EPOCH`, `RA`, `DEC`): convert
    /// frequency from heliocentric reference frame to topocentric.
    HlF2Tp = 22,
    /// `GEF2HL` (`EPOCH`, `RA`, `DEC`): convert frequency from geocentric
    /// reference frame to heliocentric.
    GeF2Hl = 23,
    /// `HLF2GE` (`EPOCH`, `RA`, `DEC`): convert frequency from heliocentric
    /// reference frame to geocentric.
    HlF2Ge = 24,
    /// `BYF2HL` (`EPOCH`, `RA`, `DEC`): convert frequency from barycentric
    /// reference frame to heliocentric.
    ByF2Hl = 25,
    /// `HLF2BY` (`EPOCH`, `RA`, `DEC`): convert frequency from heliocentric
    /// reference frame to barycentric.
    HlF2By = 26,
    /// `LKF2HL` (`RA`, `DEC`): convert frequency from kinematic LSR reference
    /// frame to heliocentric.
    LkF2Hl = 27,
    /// `HLF2LK` (`RA`, `DEC`): convert frequency from heliocentric reference
    /// frame to kinematic LSR.
    HlF2Lk = 28,
    /// `LDF2HL` (`RA`, `DEC`): convert frequency from dynamical LSR reference
    /// frame to heliocentric.
    LdF2Hl = 29,
    /// `HLF2LD` (`RA`, `DEC`): convert frequency from heliocentric reference
    /// frame to dynamical LSR.
    HlF2Ld = 30,
    /// `LGF2HL` (`RA`, `DEC`): convert frequency from local group reference
    /// frame to heliocentric.
    LgF2Hl = 31,
    /// `HLF2LG` (`RA`, `DEC`): convert frequency from heliocentric reference
    /// frame to local group.
    HlF2Lg = 32,
    /// `GLF2HL` (`RA`, `DEC`): convert frequency from galactic reference
    /// frame to heliocentric.
    GlF2Hl = 33,
    /// `HLF2GL` (`RA`, `DEC`): convert frequency from heliocentric reference
    /// frame to galactic.
    HlF2Gl = 34,
}

impl SpecCvtType {
    /// Number of user-supplied floating-point arguments this conversion requires.
    pub fn nargs(&self) -> usize {
        match self {
            // Types 3-18: no arguments
            Self::EnToFr
            | Self::FrToEn
            | Self::WnToFr
            | Self::FrToWn
            | Self::WvToFr
            | Self::FrToWv
            | Self::AwToFr
            | Self::FrToAw
            | Self::VrToVl
            | Self::VlToVr
            | Self::VoToVl
            | Self::VlToVo
            | Self::ZoToVl
            | Self::VlToZo
            | Self::BtToVl
            | Self::VlToBt => 0,
            // Types 1-2: rest frequency
            Self::FrToVl | Self::VlToFr => 1,
            // Types 27-34: RA, DEC
            Self::LkF2Hl
            | Self::HlF2Lk
            | Self::LdF2Hl
            | Self::HlF2Ld
            | Self::LgF2Hl
            | Self::HlF2Lg
            | Self::GlF2Hl
            | Self::HlF2Gl => 2,
            // Types 19-20, 23-26: velocity/epoch + RA + DEC
            Self::UsF2Hl
            | Self::HlF2Us
            | Self::GeF2Hl
            | Self::HlF2Ge
            | Self::ByF2Hl
            | Self::HlF2By => 3,
            // Types 21-22: obslon + obslat + obsalt + epoch + RA + DEC
            Self::TpF2Hl | Self::HlF2Tp => 6,
        }
    }

    /// Total stored argument slots (includes cached correction factor for frame changes).
    fn szargs(&self) -> usize {
        match self {
            // System change types: no extra cached slot
            Self::FrToVl | Self::VlToFr => 1,
            Self::EnToFr
            | Self::FrToEn
            | Self::WnToFr
            | Self::FrToWn
            | Self::WvToFr
            | Self::FrToWv
            | Self::AwToFr
            | Self::FrToAw
            | Self::VrToVl
            | Self::VlToVr
            | Self::VoToVl
            | Self::VlToVo
            | Self::ZoToVl
            | Self::VlToZo
            | Self::BtToVl
            | Self::VlToBt => 0,
            // Frame change types: nargs + 1 for cached correction factor
            Self::LkF2Hl
            | Self::HlF2Lk
            | Self::LdF2Hl
            | Self::HlF2Ld
            | Self::LgF2Hl
            | Self::HlF2Lg
            | Self::GlF2Hl
            | Self::HlF2Gl => 3,
            Self::UsF2Hl
            | Self::HlF2Us
            | Self::GeF2Hl
            | Self::HlF2Ge
            | Self::ByF2Hl
            | Self::HlF2By => 4,
            Self::TpF2Hl | Self::HlF2Tp => 7,
        }
    }

    /// Whether this type is a frame change (rest-frame Doppler correction).
    fn is_frame_change(&self) -> bool {
        (*self as i32) >= 19
    }

    /// Argument descriptions for serialization comments.
    ///
    /// Returns a slice of descriptions, one per stored argument slot.
    /// Indexed by argument position; indices beyond the slice length have no comment.
    pub fn arg_descriptions(&self) -> &'static [&'static str] {
        match self {
            // 0-arg types
            Self::EnToFr
            | Self::FrToEn
            | Self::WnToFr
            | Self::FrToWn
            | Self::WvToFr
            | Self::FrToWv
            | Self::AwToFr
            | Self::FrToAw
            | Self::VrToVl
            | Self::VlToVr
            | Self::VoToVl
            | Self::VlToVo
            | Self::ZoToVl
            | Self::VlToZo
            | Self::BtToVl
            | Self::VlToBt => &[],
            // 1-arg: rest frequency
            Self::FrToVl | Self::VlToFr => &["Rest frequency (Hz)"],
            // User-defined frame (specmap.c:1167-1190): velocity + RA/DEC.
            Self::UsF2Hl | Self::HlF2Us => &[
                "Velocity offset (m/s)",
                "RA of source (FK5 J2000, radians)",
                "DEC of source (FK5 J2000, radians)",
                "Frequency correction factor",
            ],
            // Topocentric frame
            Self::TpF2Hl | Self::HlF2Tp => &[
                "Longitude (positive eastwards, radians)",
                "Latitude (geodetic, radians)",
                "Altitude (geodetic, metres)",
                "UT1 epoch of observaton (Modified Julian Date)",
                "RA of source (FK5 J2000, radians)",
                "DEC of source (FK5 J2000, radians)",
                "Frequency correction factor",
            ],
            // Geocentric and Barycentric frames (specmap.c:1234-1275): both
            // lead with the UT1 epoch, not RA.
            Self::GeF2Hl | Self::HlF2Ge | Self::ByF2Hl | Self::HlF2By => &[
                "UT1 epoch of observaton (Modified Julian Date)",
                "RA of source (FK5 J2000, radians)",
                "DEC of source (FK5 J2000, radians)",
                "Frequency correction factor",
            ],
            // LSRK, LSRD, Local group, Galactic frames: RA/DEC only.
            Self::LkF2Hl
            | Self::HlF2Lk
            | Self::LdF2Hl
            | Self::HlF2Ld
            | Self::LgF2Hl
            | Self::HlF2Lg
            | Self::GlF2Hl
            | Self::HlF2Gl => &[
                "RA of source (FK5 J2000, radians)",
                "DEC of source (FK5 J2000, radians)",
                "Frequency correction factor",
            ],
        }
    }

    /// Uppercase code string for serialization (e.g. `"FRTOVL"`, `"VLTOFR"`).
    pub fn code_string(&self) -> &'static str {
        match self {
            Self::FrToVl => "FRTOVL",
            Self::VlToFr => "VLTOFR",
            Self::EnToFr => "ENTOFR",
            Self::FrToEn => "FRTOEN",
            Self::WnToFr => "WNTOFR",
            Self::FrToWn => "FRTOWN",
            Self::WvToFr => "WVTOFR",
            Self::FrToWv => "FRTOWV",
            Self::AwToFr => "AWTOFR",
            Self::FrToAw => "FRTOAW",
            Self::VrToVl => "VRTOVL",
            Self::VlToVr => "VLTOVR",
            Self::VoToVl => "VOTOVL",
            Self::VlToVo => "VLTOVO",
            Self::ZoToVl => "ZOTOVL",
            Self::VlToZo => "VLTOZO",
            Self::BtToVl => "BTTOVL",
            Self::VlToBt => "VLTOBT",
            Self::UsF2Hl => "USF2HL",
            Self::HlF2Us => "HLF2US",
            Self::TpF2Hl => "TPF2HL",
            Self::HlF2Tp => "HLF2TP",
            Self::GeF2Hl => "GEF2HL",
            Self::HlF2Ge => "HLF2GE",
            Self::ByF2Hl => "BYF2HL",
            Self::HlF2By => "HLF2BY",
            Self::LkF2Hl => "LKF2HL",
            Self::HlF2Lk => "HLF2LK",
            Self::LdF2Hl => "LDF2HL",
            Self::HlF2Ld => "HLF2LD",
            Self::LgF2Hl => "LGF2HL",
            Self::HlF2Lg => "HLF2LG",
            Self::GlF2Hl => "GLF2HL",
            Self::HlF2Gl => "HLF2GL",
        }
    }

    /// Parse a conversion's string code, such as `"FRTOVL"` or `"frtowv"`,
    /// ignoring case and trailing white space.
    ///
    /// Returns `None` for an unrecognized code.
    pub fn from_code(s: &str) -> Option<Self> {
        // C's CvtCode matches with astChrMatch.
        match crate::mappings::helpers::chr_match_key(s).as_str() {
            "FRTOVL" => Some(Self::FrToVl),
            "VLTOFR" => Some(Self::VlToFr),
            "ENTOFR" => Some(Self::EnToFr),
            "FRTOEN" => Some(Self::FrToEn),
            "WNTOFR" => Some(Self::WnToFr),
            "FRTOWN" => Some(Self::FrToWn),
            "WVTOFR" => Some(Self::WvToFr),
            "FRTOWV" => Some(Self::FrToWv),
            "AWTOFR" => Some(Self::AwToFr),
            "FRTOAW" => Some(Self::FrToAw),
            "VRTOVL" => Some(Self::VrToVl),
            "VLTOVR" => Some(Self::VlToVr),
            "VOTOVL" => Some(Self::VoToVl),
            "VLTOVO" => Some(Self::VlToVo),
            "ZOTOVL" => Some(Self::ZoToVl),
            "VLTOZO" => Some(Self::VlToZo),
            "BTTOVL" => Some(Self::BtToVl),
            "VLTOBT" => Some(Self::VlToBt),
            "USF2HL" => Some(Self::UsF2Hl),
            "HLF2US" => Some(Self::HlF2Us),
            "TPF2HL" => Some(Self::TpF2Hl),
            "HLF2TP" => Some(Self::HlF2Tp),
            "GEF2HL" => Some(Self::GeF2Hl),
            "HLF2GE" => Some(Self::HlF2Ge),
            "BYF2HL" => Some(Self::ByF2Hl),
            "HLF2BY" => Some(Self::HlF2By),
            "LKF2HL" => Some(Self::LkF2Hl),
            "HLF2LK" => Some(Self::HlF2Lk),
            "LDF2HL" => Some(Self::LdF2Hl),
            "HLF2LD" => Some(Self::HlF2Ld),
            "LGF2HL" => Some(Self::LgF2Hl),
            "HLF2LG" => Some(Self::HlF2Lg),
            "GLF2HL" => Some(Self::GlF2Hl),
            "HLF2GL" => Some(Self::HlF2Gl),
            _ => None,
        }
    }

    /// Return the inverse conversion type.
    pub fn inverse(&self) -> Self {
        match self {
            Self::FrToVl => Self::VlToFr,
            Self::VlToFr => Self::FrToVl,
            Self::EnToFr => Self::FrToEn,
            Self::FrToEn => Self::EnToFr,
            Self::WnToFr => Self::FrToWn,
            Self::FrToWn => Self::WnToFr,
            Self::WvToFr => Self::FrToWv,
            Self::FrToWv => Self::WvToFr,
            Self::AwToFr => Self::FrToAw,
            Self::FrToAw => Self::AwToFr,
            Self::VrToVl => Self::VlToVr,
            Self::VlToVr => Self::VrToVl,
            Self::VoToVl => Self::VlToVo,
            Self::VlToVo => Self::VoToVl,
            Self::ZoToVl => Self::VlToZo,
            Self::VlToZo => Self::ZoToVl,
            Self::BtToVl => Self::VlToBt,
            Self::VlToBt => Self::BtToVl,
            Self::UsF2Hl => Self::HlF2Us,
            Self::HlF2Us => Self::UsF2Hl,
            Self::TpF2Hl => Self::HlF2Tp,
            Self::HlF2Tp => Self::TpF2Hl,
            Self::GeF2Hl => Self::HlF2Ge,
            Self::HlF2Ge => Self::GeF2Hl,
            Self::ByF2Hl => Self::HlF2By,
            Self::HlF2By => Self::ByF2Hl,
            Self::LkF2Hl => Self::HlF2Lk,
            Self::HlF2Lk => Self::LkF2Hl,
            Self::LdF2Hl => Self::HlF2Ld,
            Self::HlF2Ld => Self::LdF2Hl,
            Self::LgF2Hl => Self::HlF2Lg,
            Self::HlF2Lg => Self::LgF2Hl,
            Self::GlF2Hl => Self::HlF2Gl,
            Self::HlF2Gl => Self::GlF2Hl,
        }
    }

    /// Construct from the integer code used in the C library.
    pub fn from_int(i: i32) -> Option<Self> {
        match i {
            1 => Some(Self::FrToVl),
            2 => Some(Self::VlToFr),
            3 => Some(Self::EnToFr),
            4 => Some(Self::FrToEn),
            5 => Some(Self::WnToFr),
            6 => Some(Self::FrToWn),
            7 => Some(Self::WvToFr),
            8 => Some(Self::FrToWv),
            9 => Some(Self::AwToFr),
            10 => Some(Self::FrToAw),
            11 => Some(Self::VrToVl),
            12 => Some(Self::VlToVr),
            13 => Some(Self::VoToVl),
            14 => Some(Self::VlToVo),
            15 => Some(Self::ZoToVl),
            16 => Some(Self::VlToZo),
            17 => Some(Self::BtToVl),
            18 => Some(Self::VlToBt),
            19 => Some(Self::UsF2Hl),
            20 => Some(Self::HlF2Us),
            21 => Some(Self::TpF2Hl),
            22 => Some(Self::HlF2Tp),
            23 => Some(Self::GeF2Hl),
            24 => Some(Self::HlF2Ge),
            25 => Some(Self::ByF2Hl),
            26 => Some(Self::HlF2By),
            27 => Some(Self::LkF2Hl),
            28 => Some(Self::HlF2Lk),
            29 => Some(Self::LdF2Hl),
            30 => Some(Self::HlF2Ld),
            31 => Some(Self::LgF2Hl),
            32 => Some(Self::HlF2Lg),
            33 => Some(Self::GlF2Hl),
            34 => Some(Self::HlF2Gl),
            _ => None,
        }
    }

    /// Human-readable description for serialization comments.
    pub fn description(&self) -> &'static str {
        match self {
            Self::FrToVl => "Convert frequency to rel. velocity",
            Self::VlToFr => "Convert rel. velocity to frequency",
            Self::EnToFr => "Convert energy to frequency",
            Self::FrToEn => "Convert frequency to energy",
            Self::WnToFr => "Convert wave number to frequency",
            Self::FrToWn => "Convert frequency to wave number",
            Self::WvToFr => "Convert wavelength (vacuum) to frequency",
            Self::FrToWv => "Convert frequency to wavelength (vacuum)",
            Self::AwToFr => "Convert wavelength (air) to frequency",
            Self::FrToAw => "Convert frequency to wavelength (air)",
            Self::VrToVl => "Convert radio to rel. velocity",
            Self::VlToVr => "Convert relativistic to radio velocity",
            Self::VoToVl => "Convert optical to rel. velocity",
            Self::VlToVo => "Convert relativistic to optical velocity",
            Self::ZoToVl => "Convert redshift to rel. velocity",
            Self::VlToZo => "Convert rel. velocity to redshift",
            Self::BtToVl => "Convert beta factor to rel. velocity",
            Self::VlToBt => "Convert rel. velocity to beta factor",
            Self::UsF2Hl => "Convert from user-defined to heliocentric frequency",
            Self::HlF2Us => "Convert from heliocentric to user-defined frequency",
            Self::TpF2Hl => "Convert from Topocentric to heliocentric frequency",
            Self::HlF2Tp => "Convert from Heliocentric to topocentric frequency",
            Self::GeF2Hl => "Convert from Geocentric to heliocentric frequency",
            Self::HlF2Ge => "Convert from Heliocentric to geocentric frequency",
            Self::ByF2Hl => "Convert from Barycentric to heliocentric frequency",
            Self::HlF2By => "Convert from Heliocentric to barycentric frequency",
            Self::LkF2Hl => "Convert from LSRK to heliocentric frequency",
            Self::HlF2Lk => "Convert from Heliocentric to LSRK frequency",
            Self::LdF2Hl => "Convert from LSRD to heliocentric frequency",
            Self::HlF2Ld => "Convert from Heliocentric to LSRD frequency",
            Self::LgF2Hl => "Convert from Local group to heliocentric frequency",
            Self::HlF2Lg => "Convert from Heliocentric to local group frequency",
            Self::GlF2Hl => "Convert from Galactic to heliocentric frequency",
            Self::HlF2Gl => "Convert from Heliocentric to galactic frequency",
        }
    }
}

// ---------------------------------------------------------------------------
// SpecConversion — one step in the chain
// ---------------------------------------------------------------------------

/// A single conversion step in a [`SpecMap`] chain.
#[derive(Debug, Clone)]
struct SpecConversion {
    cvt_type: SpecCvtType,
    args: Vec<f64>,
}

// ---------------------------------------------------------------------------
// SpecMap
// ---------------------------------------------------------------------------

/// Sequence of spectral coordinate conversions.
///
/// A SpecMap represents a sequence of conversions between standard spectral
/// coordinate systems: frequency, wavelength and various forms of velocity,
/// and different standards of rest.
///
/// When first created, a SpecMap is a unit (null) Mapping.
/// [`add`](SpecMap::add) then appends coordinate conversion steps, so that
/// multi-step conversions between a variety of spectral coordinate systems
/// can be assembled out of a set of building blocks.
/// [`SpecCvtType`] lists the conversions available.
///
/// The conversions between standards of rest need the source position as an
/// RA and Dec.
/// It can be given as arguments of those conversions, in which case the
/// SpecMap is 1-dimensional ([`new`](SpecMap::new)), transforming only the
/// spectral axis values, and always uses the same source position.
/// That may not describe a 3-D spectral cube accurately, since changes of
/// spatial position can change the Doppler shift between standards of rest
/// significantly.
/// For that case a 3-dimensional SpecMap ([`with_nin`](SpecMap::with_nin))
/// takes the source RA and Dec from its second and third coordinates, which
/// it copies unchanged from input to output, and modifies the first (the
/// spectral axis) appropriately.
///
/// # Example
///
/// ```
/// use ast_core::mappings::{Mapping, SpecMap, SpecCvtType};
/// use ast_core::coords::PointSet;
///
/// let mut spec = SpecMap::new();
/// spec.add(SpecCvtType::FrToWv, &[]).unwrap();
///
/// // Convert 5e14 Hz to vacuum wavelength
/// let pts = PointSet::from_rows([[5.0e14_f64]]);
/// let out = spec.transform(&pts, true).unwrap();
/// // Should give ~6e-7 m (600 nm)
/// let wv = out.axis(0)[0];
/// assert!((wv - 5.99584916e-7).abs() < 1e-13);
/// ```
#[derive(Debug, Clone)]
pub struct SpecMap {
    conversions: Vec<SpecConversion>,
    /// The number of axes: 1, or 3 when the second and third give the RA and
    /// Dec of each point's source (specmap.c `astSpecMap`).
    nin: usize,
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
    /// The frequency-correction factor of each rest-frame step, worked out on
    /// first use and kept, as C keeps it in the step's spare argument slot
    /// (specmap.c `FrameChange`, "If the frequency correction factor has not
    /// been found, find it now"). `BAD_VALUE` until then; a clone keeps it,
    /// as C's Copy copies the slot.
    fcorr: std::cell::RefCell<Vec<f64>>,
}

impl SpecMap {
    /// Create a 1-dimensional SpecMap with no conversion steps, which
    /// performs a unit Mapping until steps are added with
    /// [`add`](SpecMap::add).
    ///
    /// The nature and units of the values supplied to the SpecMap must suit
    /// the first conversion step it applies, and those it returns are set by
    /// the last: if the first step is [`SpecCvtType::FrToVl`] the input is a
    /// frequency in Hz, and if the last is [`SpecCvtType::VlToVo`] the output
    /// is an optical velocity in meters per second.
    #[doc(alias = "astSpecMap")]
    pub fn new() -> Self {
        Self {
            conversions: Vec::new(),
            nin: 1,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
            fcorr: Default::default(),
        }
    }

    /// Create a SpecMap with `nin` input and output coordinates and no
    /// conversion steps.
    ///
    /// `nin` is 1, or 3 for a SpecMap whose second and third coordinates give
    /// the RA and Dec (FK5 J2000, radians) of each point's source.
    /// The first coordinate is the spectral axis, as for
    /// [`new`](SpecMap::new).
    /// In a 3-dimensional SpecMap a conversion between standards of rest uses
    /// each point's position in place of the `RA` and `DEC` arguments, and
    /// the RA and Dec are copied unchanged to the output.
    ///
    /// # Errors
    ///
    /// `ErrorCode::Badni` if `nin` is neither 1 nor 3.
    #[doc(alias = "astSpecMap")]
    pub fn with_nin(nin: usize) -> AstResult<Self> {
        // specmap.c Transform and FrameChange use the point's position.
        if nin != 1 && nin != 3 {
            return Err(AstError::new(
                ErrorCode::Badni,
                format!("SpecMap: the number of axes ({nin}) must be 1 or 3"),
            ));
        }
        Ok(Self { nin, ..Self::new() })
    }

    /// Add a spectral coordinate conversion to the SpecMap.
    ///
    /// `cvt_type` selects the conversion, and `args` holds its arguments in
    /// the order listed for that [`SpecCvtType`] variant, in the units listed
    /// there; it is empty for a conversion that takes none.
    ///
    /// The forward transformation of the SpecMap applies its conversions in
    /// the order they were added, the most recently added last.
    /// The inverse transformation, or the forward transformation of an
    /// inverted SpecMap, applies the inverse of each conversion in the
    /// reverse order, the inverse of the most recently added first.
    ///
    /// No check is made that the sequence of conversions is physically
    /// meaningful.
    /// When assembling a multi-step conversion, it is often simplest to
    /// include every step that is logically needed (converting to the
    /// heliocentric reference frame as an intermediate stage, for instance)
    /// and then simplify the SpecMap (`simplify` on `dyn Mapping`), which
    /// removes the steps that turn out not to be needed.
    ///
    /// ```
    /// use ast_core::mappings::{Mapping, SpecCvtType, SpecMap};
    ///
    /// // Frequency (Hz) to optical velocity (m/s) for a rest frequency of
    /// // 1.42 GHz.
    /// let mut map = SpecMap::new();
    /// map.add(SpecCvtType::FrToVl, &[1.42e9]).unwrap();
    /// map.add(SpecCvtType::VlToVo, &[]).unwrap();
    /// assert_eq!(map.tran1(&[1.42e9], true).unwrap(), vec![0.0]);
    /// ```
    ///
    /// # Errors
    ///
    /// `ErrorCode::Timin` if `args` does not hold exactly
    /// [`cvt_type.nargs()`](SpecCvtType::nargs) values.
    #[doc(alias = "astSpecAdd")]
    pub fn add(&mut self, cvt_type: SpecCvtType, args: &[f64]) -> AstResult<()> {
        // The argument count check is specmap.c AddSpecCvt's.
        if args.len() != cvt_type.nargs() {
            return Err(AstError::new(
                ErrorCode::Timin,
                format!(
                    "SpecAdd: invalid number of arguments for spectral coordinate conversion \
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
    pub(crate) fn add_conversion(&mut self, cvt_type: SpecCvtType, args: &[f64]) {
        let szargs = cvt_type.szargs();
        let mut stored_args = Vec::with_capacity(szargs);
        // Copy user-supplied args (up to nargs)
        let nargs = cvt_type.nargs();
        for &arg in args.iter().take(nargs) {
            stored_args.push(arg);
        }
        // Pad with zeros if user supplied fewer than nargs
        while stored_args.len() < nargs {
            stored_args.push(0.0);
        }
        // For frame changes, add the cached correction factor slot (BAD = not yet computed)
        if cvt_type.is_frame_change() {
            stored_args.push(BAD_VALUE);
        }
        self.conversions.push(SpecConversion {
            cvt_type,
            args: stored_args,
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

    /// Transform the points of a 3-dimensional SpecMap: each rest-frame
    /// conversion uses the point's own RA and Dec, and the RA and Dec are
    /// copied to the output (specmap.c `Transform` with `map3d` set).
    fn transform_3d(&self, pts: &Points<'_>, forward: bool, out: &mut PointsMut<'_>) {
        let n = pts.npoint();
        let (spec, ras, decs) = (pts.axis(0), pts.axis(1), pts.axis(2));
        let Some([o_spec, o_ra, o_dec]) = out.first_axes_mut() else {
            unreachable!("a 3-D SpecMap has three outputs")
        };
        let nconv = self.conversions.len();
        for i in 0..n {
            let (ra, dec) = (ras[i], decs[i]);
            let mut val = spec[i];
            for k in 0..nconv {
                let cvt = &self.conversions[if forward { k } else { nconv - 1 - k }];
                if is_bad(val) {
                    break;
                }
                val = if cvt.cvt_type.is_frame_change() {
                    frame_change_at(cvt, val, ra, dec, forward)
                } else if forward {
                    system_change_forward(cvt, val)
                } else {
                    system_change_inverse(cvt, val)
                };
            }
            if !val.is_finite() {
                val = BAD_VALUE;
            }
            o_spec[i] = val;
            o_ra[i] = ra;
            o_dec[i] = dec;
        }
    }

    /// The frequency-correction factor of rest-frame step `ci`: the value in
    /// its spare argument slot if one is stored there, otherwise the one
    /// worked out on first use and kept in `fcorr` (specmap.c `FrameChange`).
    fn frame_factor(&self, ci: usize, cvt: &SpecConversion) -> f64 {
        let stored = cvt.args[cvt.args.len() - 1];
        if stored != BAD_VALUE {
            return stored;
        }
        let mut cache = self.fcorr.borrow_mut();
        if cache.len() != self.conversions.len() {
            *cache = vec![BAD_VALUE; self.conversions.len()];
        }
        if cache[ci] == BAD_VALUE {
            cache[ci] = compute_frame_correction(cvt, frame_change_sign(cvt.cvt_type));
        }
        cache[ci]
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

impl Default for SpecMap {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Refractive index
// ---------------------------------------------------------------------------

/// Refractive index of dry air at STP for a given wavelength.
///
/// Uses the formula from FITS-WCS Paper III (Greisen et al).
/// `wavelen` is in metres (air wavelength).
fn refrac(wavelen: f64) -> f64 {
    if wavelen == 0.0 {
        return 1.0;
    }
    // Squared wave number in (per micrometre)^2
    let w2 = 1.0e-12 / (wavelen * wavelen);
    1.0 + 1.0e-6 * (287.6155 + 1.62887 * w2 + 0.01360 * w2 * w2)
}

// ---------------------------------------------------------------------------
// Velocity computation functions for frame changes
// ---------------------------------------------------------------------------

// The four `*_vel` helpers below mirror `specmap.c`'s `LsrkVel`,
// `LsrdVel`, `LgVel`, `GalVel`. They pass the f64 RA/Dec straight to
// the double-precision `palRv*` routines. Upstream C AST (specmap.c,
// commit 409f1311) removed the historical `(float)` casts that
// truncated ~20 mas of precision (a SLALIB-Fortran inheritance); the
// Rust port matches that. The sibling bary/topo/geo/user helpers never
// cast, so they were already correct.

/// Velocity of the kinematic LSR away from the source (m/s, heliocentric).
///
/// Mirrors `specmap.c` `LsrkVel`.
pub(crate) fn lsrk_vel(ra: f64, dec: f64) -> f64 {
    -1000.0 * pal::rvlsrk(ra, dec)
}

/// Velocity of the dynamical LSR away from the source (m/s, heliocentric).
///
/// Mirrors `specmap.c` `LsrdVel`.
pub(crate) fn lsrd_vel(ra: f64, dec: f64) -> f64 {
    -1000.0 * pal::rvlsrd(ra, dec)
}

/// Velocity of the Local Group away from the source (m/s, heliocentric).
///
/// Mirrors `specmap.c` `LgVel`.
pub(crate) fn lg_vel(ra: f64, dec: f64) -> f64 {
    -1000.0 * pal::rvlg(ra, dec)
}

/// Velocity of the galactic centre away from the source (m/s, heliocentric).
///
/// Mirrors `specmap.c` `GalVel`.
pub(crate) fn gal_vel(ra: f64, dec: f64) -> f64 {
    let s1 = pal::rvlsrd(ra, dec);
    let s2 = pal::rvgalc(ra, dec);
    -1000.0 * (s1 + s2)
}

/// Velocity of the earth away from the source (m/s, heliocentric).
pub(crate) fn geo_vel(ra: f64, dec: f64, epoch: f64) -> f64 {
    let v = pal::dcs2c(ra, dec);
    let (_dvb, _dpb, dvh, _dph) = pal::evp(epoch, 2000.0);
    -pal::dvdv(&v, &dvh) * AU_TO_MS
}

/// Velocity of the earth-sun barycentre away from the source (m/s, heliocentric).
pub(crate) fn bary_vel(ra: f64, dec: f64, epoch: f64) -> f64 {
    let v = pal::dcs2c(ra, dec);
    let (dvb, _dpb, dvh, _dph) = pal::evp(epoch, 2000.0);
    // Heliocentric velocity of the barycentre = dvh - dvb
    let dvb_helio = [dvh[0] - dvb[0], dvh[1] - dvb[1], dvh[2] - dvb[2]];
    -pal::dvdv(&v, &dvb_helio) * AU_TO_MS
}

/// Velocity of the observer (topocentric) away from the source (m/s, heliocentric).
pub(crate) fn topo_vel(
    ra: f64,
    dec: f64,
    obslon: f64,
    obslat: f64,
    obsalt: f64,
    epoch: f64,
) -> f64 {
    // Get mean-to-apparent parameters
    let amprms = pal::mappa(2000.0, epoch);

    // Convert source position to apparent RA/Dec
    let (raa, deca) = pal::mapqkz(ra, dec, &amprms);

    // Local Apparent Sidereal Time
    let last = pal::gmst(epoch) + pal::eqeqx(epoch) + obslon;

    // Observer velocity relative to Earth centre (using Rverot approach)
    let v_dir = pal::dcs2c(raa, deca);
    let pv = pal::pvobs(obslat, obsalt, last);
    // Component away from source, AU/s -> km/s
    let vobs = -pal::dvdv(&v_dir, &[pv[3], pv[4], pv[5]]) * AU_TO_KMS;

    // Earth velocity relative to Sun
    let vearth = geo_vel(ra, dec, epoch);

    // Total velocity (convert observer part from km/s to m/s)
    vobs * 1000.0 + vearth
}

/// Velocity of the user-defined frame away from the source (m/s, heliocentric).
fn user_vel(ra: f64, dec: f64, veluser: f64, refra: f64, refdec: f64) -> f64 {
    // Express user velocity as J2000 x,y,z vector
    let vuser = [
        veluser * crate::math::cos(refra) * crate::math::cos(refdec),
        veluser * crate::math::sin(refra) * crate::math::cos(refdec),
        veluser * crate::math::sin(refdec),
    ];
    // Direction to observed source
    let vb = pal::dcs2c(ra, dec);
    // Dot product, inverted (veluser is positive if source recedes from observer)
    -pal::dvdv(&vuser, &vb)
}

// ---------------------------------------------------------------------------
// Core transform logic
// ---------------------------------------------------------------------------

/// Apply the system-change conversion `cvt_type`, with arguments `args`, to
/// one value. Returns the transformed value, or BAD_VALUE on error.
#[inline(always)]
fn system_change(cvt_type: SpecCvtType, args: &[f64], val: f64) -> f64 {
    match cvt_type {
        SpecCvtType::FrToVl => {
            // Frequency to relativistic velocity:
            //   v = c * ((rf² - f²) / (rf² + f²))
            // C `specmap.c::SystemChange` (line 3143) groups the
            // division before the multiplication: `AST__C * ((temp -
            // f2) / d)`. f64 is not associative, so the equivalent
            // grouping `c * (rf2 - f2) / d` (left-to-right) drifts
            // by ~6e-5 in the output for typical mm-wave inputs,
            // surfacing as a ~30 mm/s VELO-LSR error after the
            // VlToVr step. Match C's grouping exactly.
            if args[0] == BAD_VALUE {
                return BAD_VALUE;
            }
            let rf2 = args[0] * args[0];
            let f2 = val * val;
            let d = rf2 + f2;
            if d > 0.0 {
                let result = C_LIGHT * ((rf2 - f2) / d);
                if result.is_finite() {
                    result
                } else {
                    BAD_VALUE
                }
            } else {
                BAD_VALUE
            }
        }
        SpecCvtType::VlToFr => {
            // Relativistic velocity to frequency: f = rf * sqrt((c-v)/(c+v))
            if args[0] == BAD_VALUE {
                return BAD_VALUE;
            }
            let d = C_LIGHT + val;
            if d != 0.0 {
                let ratio = (C_LIGHT - val) / d;
                if ratio >= 0.0 {
                    let result = args[0] * crate::math::sqrt(ratio);
                    if result.is_finite() {
                        result
                    } else {
                        BAD_VALUE
                    }
                } else {
                    BAD_VALUE
                }
            } else {
                BAD_VALUE
            }
        }
        SpecCvtType::EnToFr => {
            // Energy to frequency: f = E / h
            val / H_PLANCK
        }
        SpecCvtType::FrToEn => {
            // Frequency to energy: E = f * h
            val * H_PLANCK
        }
        SpecCvtType::WnToFr => {
            // Wave number to frequency: f = wn * c
            val * C_LIGHT
        }
        SpecCvtType::FrToWn => {
            // Frequency to wave number: wn = f / c
            val / C_LIGHT
        }
        SpecCvtType::WvToFr => {
            // Wavelength (vacuum) to frequency: f = c / wv
            if val != 0.0 {
                let result = C_LIGHT / val;
                if result.is_finite() {
                    result
                } else {
                    BAD_VALUE
                }
            } else {
                BAD_VALUE
            }
        }
        SpecCvtType::FrToWv => {
            // Frequency to wavelength (vacuum): wv = c / f
            if val != 0.0 {
                let result = C_LIGHT / val;
                if result.is_finite() {
                    result
                } else {
                    BAD_VALUE
                }
            } else {
                BAD_VALUE
            }
        }
        SpecCvtType::AwToFr => {
            // Wavelength (air) to frequency: f = c / (wv_air * n(wv_air))
            if val != 0.0 {
                let result = C_LIGHT / (val * refrac(val));
                if result.is_finite() {
                    result
                } else {
                    BAD_VALUE
                }
            } else {
                BAD_VALUE
            }
        }
        SpecCvtType::FrToAw => {
            // Frequency to wavelength (air): iterate to find air wavelength
            if val != 0.0 {
                let temp = C_LIGHT / val; // vacuum wavelength
                let mut result = temp;
                for _ in 0..3 {
                    result = temp / refrac(result);
                    if !result.is_finite() {
                        return BAD_VALUE;
                    }
                }
                result
            } else {
                BAD_VALUE
            }
        }
        SpecCvtType::VrToVl => {
            // Radio velocity to relativistic velocity
            // temp = 1 - vr/c; v = c*(1 - temp^2)/(1 + temp^2)
            let temp = 1.0 - val / C_LIGHT;
            let temp2 = temp * temp;
            let result = C_LIGHT * (1.0 - temp2) / (1.0 + temp2);
            if result.is_finite() {
                result
            } else {
                BAD_VALUE
            }
        }
        SpecCvtType::VlToVr => {
            // Relativistic to radio velocity
            // vr = c * (1 - sqrt((c-v)/(c+v)))
            let denom = C_LIGHT + val;
            if denom != 0.0 {
                let ratio = (C_LIGHT - val) / denom;
                if ratio >= 0.0 {
                    let result = C_LIGHT * (1.0 - crate::math::sqrt(ratio));
                    if result.is_finite() {
                        result
                    } else {
                        BAD_VALUE
                    }
                } else {
                    BAD_VALUE
                }
            } else {
                BAD_VALUE
            }
        }
        SpecCvtType::VoToVl => {
            // Optical velocity to relativistic velocity
            // temp = (1 + vo/c)^2; v = c*(temp - 1)/(temp + 1)
            let temp = 1.0 + val / C_LIGHT;
            let temp2 = temp * temp;
            let result = C_LIGHT * (temp2 - 1.0) / (temp2 + 1.0);
            if result.is_finite() {
                result
            } else {
                BAD_VALUE
            }
        }
        SpecCvtType::VlToVo => {
            // Relativistic to optical velocity
            // vo = c*(sqrt((c+v)/(c-v)) - 1)
            let denom = C_LIGHT - val;
            if denom != 0.0 {
                let ratio = (C_LIGHT + val) / denom;
                if ratio >= 0.0 {
                    let result = C_LIGHT * (crate::math::sqrt(ratio) - 1.0);
                    if result.is_finite() {
                        result
                    } else {
                        BAD_VALUE
                    }
                } else {
                    BAD_VALUE
                }
            } else {
                BAD_VALUE
            }
        }
        SpecCvtType::ZoToVl => {
            // Redshift to relativistic velocity
            // temp = (1+z)^2; v = c*(temp - 1)/(temp + 1)
            let temp = 1.0 + val;
            let temp2 = temp * temp;
            let result = C_LIGHT * (temp2 - 1.0) / (temp2 + 1.0);
            if result.is_finite() {
                result
            } else {
                BAD_VALUE
            }
        }
        SpecCvtType::VlToZo => {
            // Relativistic velocity to redshift
            // z = sqrt((c+v)/(c-v)) - 1
            let denom = C_LIGHT - val;
            if denom != 0.0 {
                let ratio = (C_LIGHT + val) / denom;
                if ratio >= 0.0 {
                    let result = crate::math::sqrt(ratio) - 1.0;
                    if result.is_finite() {
                        result
                    } else {
                        BAD_VALUE
                    }
                } else {
                    BAD_VALUE
                }
            } else {
                BAD_VALUE
            }
        }
        SpecCvtType::BtToVl => {
            // Beta to relativistic velocity: v = beta * c
            val * C_LIGHT
        }
        SpecCvtType::VlToBt => {
            // Relativistic velocity to beta: beta = v / c
            val / C_LIGHT
        }
        // Frame changes are handled separately
        _ => val,
    }
}

/// Apply a system change in the inverse direction.
///
/// The C code handles inverse by calling the forward function of the inverse type.
fn system_change_inverse(cvt: &SpecConversion, val: f64) -> f64 {
    system_change(cvt.cvt_type.inverse(), &cvt.args, val)
}

/// Apply a system change in the forward direction.
fn system_change_forward(cvt: &SpecConversion, val: f64) -> f64 {
    system_change(cvt.cvt_type, &cvt.args, val)
}

/// Apply the system change `cvt_type` with arguments `args` to every good
/// value of `o`, as C's `SystemChange` does over its array (specmap.c:3065):
/// the conversion is chosen once, outside the loop over values. Each arm
/// passes its type as a constant, so the match inside `system_change` folds
/// away and a cheap conversion's loop vectorizes.
fn system_change_slice(cvt_type: SpecCvtType, args: &[f64], o: &mut [f64]) {
    macro_rules! unswitched {
        ($($t:ident),*) => {
            match cvt_type {
                $(SpecCvtType::$t => {
                    for v in o.iter_mut() {
                        *v = if is_bad(*v) {
                            *v
                        } else {
                            system_change(SpecCvtType::$t, args, *v)
                        };
                    }
                })*
                // Frame changes are applied by the caller.
                _ => {}
            }
        };
    }
    unswitched!(
        FrToVl, VlToFr, EnToFr, FrToEn, WnToFr, FrToWn, WvToFr, FrToWv, AwToFr, FrToAw, VrToVl,
        VlToVr, VoToVl, VlToVo, ZoToVl, VlToZo, BtToVl, VlToBt
    );
}

/// The component away from a source at `ra`, `dec` of the velocity of the
/// frame a rest-frame conversion goes to or from, relative to the sun (m/s);
/// C's per-conversion `cvtFunc`. `None` for a conversion that is not a change
/// of rest frame.
fn source_velocity(cvt: &SpecConversion, ra: f64, dec: f64) -> Option<f64> {
    let a = &cvt.args;
    Some(match cvt.cvt_type {
        // args: [veluser, refra, refdec, (cached)]
        SpecCvtType::UsF2Hl | SpecCvtType::HlF2Us => user_vel(ra, dec, a[0], a[1], a[2]),
        // args: [obslon, obslat, obsalt, epoch, ra, dec, (cached)]
        SpecCvtType::TpF2Hl | SpecCvtType::HlF2Tp => topo_vel(ra, dec, a[0], a[1], a[2], a[3]),
        // args: [epoch, ra, dec, (cached)]
        SpecCvtType::GeF2Hl | SpecCvtType::HlF2Ge => geo_vel(ra, dec, a[0]),
        SpecCvtType::ByF2Hl | SpecCvtType::HlF2By => bary_vel(ra, dec, a[0]),
        // args: [ra, dec, (cached)]
        SpecCvtType::LkF2Hl | SpecCvtType::HlF2Lk => lsrk_vel(ra, dec),
        SpecCvtType::LdF2Hl | SpecCvtType::HlF2Ld => lsrd_vel(ra, dec),
        SpecCvtType::LgF2Hl | SpecCvtType::HlF2Lg => lg_vel(ra, dec),
        SpecCvtType::GlF2Hl | SpecCvtType::HlF2Gl => gal_vel(ra, dec),
        _ => return None,
    })
}

/// The source position in a rest-frame conversion's arguments.
fn source_position(cvt: &SpecConversion) -> (f64, f64) {
    let a = &cvt.args;
    match cvt.cvt_type {
        SpecCvtType::UsF2Hl
        | SpecCvtType::HlF2Us
        | SpecCvtType::GeF2Hl
        | SpecCvtType::HlF2Ge
        | SpecCvtType::ByF2Hl
        | SpecCvtType::HlF2By => (a[1], a[2]),
        SpecCvtType::TpF2Hl | SpecCvtType::HlF2Tp => (a[4], a[5]),
        _ => (a[0], a[1]),
    }
}

/// Compute the velocity correction for a frame change at the source position
/// in its arguments and return the frequency correction factor.
///
/// `sign` is -1 for X2HL (frame moving toward heliocentric = frequency increases)
/// and +1 for HL2X (heliocentric to frame).
fn compute_frame_correction(cvt: &SpecConversion, sign: f64) -> f64 {
    let (ra, dec) = source_position(cvt);
    let Some(v) = source_velocity(cvt, ra, dec) else {
        return BAD_VALUE;
    };
    let s = sign * v;

    // Compute correction factor: f_out = f_in * sqrt((c - s)/(c + s))
    if s < C_LIGHT && s > -C_LIGHT {
        crate::math::sqrt((C_LIGHT - s) / (C_LIGHT + s))
    } else {
        BAD_VALUE
    }
}

/// Apply a frame-change conversion to a frequency whose source is at `ra`,
/// `dec`, as a 3-dimensional SpecMap does (specmap.c `FrameChange`): the
/// inverse negates the velocity rather than inverting the factor, and a bad
/// position gives a bad frequency.
fn frame_change_at(cvt: &SpecConversion, val: f64, ra: f64, dec: f64, forward: bool) -> f64 {
    if is_bad(ra) || is_bad(dec) || is_bad(val) {
        return BAD_VALUE;
    }
    let sign = if forward {
        frame_change_sign(cvt.cvt_type)
    } else {
        -frame_change_sign(cvt.cvt_type)
    };
    let Some(v) = source_velocity(cvt, ra, dec) else {
        return BAD_VALUE;
    };
    let s = sign * v;
    if s < C_LIGHT && s > -C_LIGHT {
        val * crate::math::sqrt((C_LIGHT - s) / (C_LIGHT + s))
    } else {
        BAD_VALUE
    }
}

/// Get the sign for a given frame change type.
/// -1 for X2HL (converting from some frame to heliocentric),
/// +1 for HL2X (converting from heliocentric to some frame).
fn frame_change_sign(cvt_type: SpecCvtType) -> f64 {
    match cvt_type {
        SpecCvtType::UsF2Hl
        | SpecCvtType::TpF2Hl
        | SpecCvtType::GeF2Hl
        | SpecCvtType::ByF2Hl
        | SpecCvtType::LkF2Hl
        | SpecCvtType::LdF2Hl
        | SpecCvtType::LgF2Hl
        | SpecCvtType::GlF2Hl => -1.0,
        _ => 1.0,
    }
}

/// The factor a frame-change conversion step whose frequency-correction
/// factor is `fcorr` (`SpecMap::frame_factor`) multiplies each good value
/// by, or `BAD_VALUE` when the step makes every value bad.
fn frame_change_factor(fcorr: f64, forward: bool) -> f64 {
    if fcorr == BAD_VALUE || fcorr == 0.0 {
        BAD_VALUE
    } else if forward {
        fcorr
    } else {
        1.0 / fcorr
    }
}

// ---------------------------------------------------------------------------
// Step-level simplification helpers
// ---------------------------------------------------------------------------

/// Check whether two conversion steps cancel each other.
///
/// Two steps cancel if one is the inverse of the other and they share
/// the same user-supplied arguments (ignoring the cached correction factor
/// slot that frame-change steps store in their last arg position).
fn steps_cancel(a: &SpecConversion, b: &SpecConversion) -> bool {
    if a.cvt_type.inverse() != b.cvt_type {
        return false;
    }
    // Compare the first nargs() values (user-supplied arguments).
    let nargs = a.cvt_type.nargs();
    for i in 0..nargs {
        let av = a.args.get(i).copied().unwrap_or(0.0);
        let bv = b.args.get(i).copied().unwrap_or(0.0);
        if !ast_equal(av, bv) {
            return false;
        }
    }
    true
}

/// Produce a normalized step list for a SpecMap, accounting for inversion.
///
/// When `inv` is false, clones the conversions as-is.
/// When `inv` is true, reverses the step order and swaps each step's
/// `cvt_type` to its inverse. For frame-change steps the cached correction
/// factor (last arg slot) is reciprocated (1.0 / value) if it is a valid,
/// non-zero, non-BAD value.
fn normalized_steps(spec: &SpecMap, inv: bool) -> Vec<SpecConversion> {
    if !inv {
        return spec.conversions.clone();
    }
    spec.conversions
        .iter()
        .rev()
        .map(|step| {
            let inv_type = step.cvt_type.inverse();
            let mut args = step.args.clone();
            // For frame-change steps, reciprocate the cached correction factor
            if inv_type.is_frame_change() && !args.is_empty() {
                let last = args.len() - 1;
                let v = args[last];
                if v != BAD_VALUE && v != 0.0 {
                    args[last] = 1.0 / v;
                }
            }
            SpecConversion {
                cvt_type: inv_type,
                args,
            }
        })
        .collect()
}

/// Reset every cached (pre-calculated) argument slot to `BAD_VALUE`.
///
/// Mirrors C `specmap.c::AddSpecCvt` (line 708): when a conversion step is
/// (re-)added to a SpecMap, every array element beyond the user-supplied
/// arguments (`nargs..szargs`) is set to `AST__BAD`. Those slots hold values
/// computed lazily at transform time (e.g. a frame-change frequency
/// correction factor), so a freshly rebuilt SpecMap must not carry a cached
/// value inherited from the component it was merged from — otherwise the
/// stale factor is emitted in the native dump (`Spec<n>g` etc.), where C emits
/// nothing. The value is recomputed on demand.
fn reset_cached_args(steps: &mut [SpecConversion]) {
    for step in steps.iter_mut() {
        let nargs = step.cvt_type.nargs();
        for slot in step.args.iter_mut().skip(nargs) {
            *slot = BAD_VALUE;
        }
    }
}

/// Scan adjacent step pairs and remove cancelling pairs.
///
/// Loops until no further cancellations are found.
/// Returns `true` if any steps were eliminated.
fn simplify_steps(steps: &mut Vec<SpecConversion>) -> bool {
    let mut any_removed = false;
    loop {
        let mut removed_this_pass = false;
        let mut i = 0;
        while i + 1 < steps.len() {
            if steps_cancel(&steps[i], &steps[i + 1]) {
                steps.remove(i + 1);
                steps.remove(i);
                removed_this_pass = true;
                any_removed = true;
                // Don't advance i — new neighbours may cancel
            } else {
                i += 1;
            }
        }
        if !removed_this_pass {
            break;
        }
    }
    any_removed
}

/// `map_merge` for a nominated `SpecMap`, mirroring C `specmap.c::MapMerge`
/// (2089). Series only.
///
/// Gathers the run of adjacent `SpecMap`s at `where_`, concatenates their
/// invert-resolved step lists, cancels adjacent inverse pairs, and — per
/// C's condition (d) `simpler = ngone || (nstep < nstep0) || unit ||
/// invert_list[where]` (specmap.c:2570) — replaces the run with a single
/// **forward** `SpecMap` (invert flag cleared) whenever a lone SpecMap was
/// applied inverted, even if no cancellation occurred. This is why a lone
/// inverted SpecMap never surfaces as an `InvA`/`InvB` on a parent CmpMap.
pub(crate) fn spec_map_merge(
    ctx: &mut super::simplify::MergeCtx,
    where_: usize,
    series: bool,
) -> Option<usize> {
    if !series {
        return super::simplify::default_map_merge(ctx, where_, series);
    }

    // The run stops at a SpecMap with a different number of axes (specmap.c
    // MapMerge).
    let nin = ctx.entry(where_).0.nin();
    let is_spec =
        |entry: &(DynMapping, bool, bool)| entry.0.as_any().is::<SpecMap>() && entry.0.nin() == nin;

    // Gather the contiguous run of SpecMaps around `where_`.
    let n = ctx.len();
    let mut imap1 = where_;
    while imap1 >= 1 && is_spec(ctx.entry(imap1 - 1)) {
        imap1 -= 1;
    }
    let mut imap2 = where_;
    while imap2 + 1 < n && is_spec(ctx.entry(imap2 + 1)) {
        imap2 += 1;
    }

    // Concatenate the invert-resolved steps of every SpecMap in the run,
    // and count the original steps (nstep0) for the `simpler` test.
    let mut steps: Vec<SpecConversion> = Vec::new();
    let mut nstep0 = 0usize;
    let where_inv = ctx.entry(where_).1;
    for idx in imap1..=imap2 {
        let entry = ctx.entry(idx);
        let sm = entry.0.as_any().downcast_ref::<SpecMap>()?;
        // Effective inversion = stored invert XOR applied-direction bool.
        let eff_inv = entry.1 != sm.inverted;
        let s = normalized_steps(sm, eff_inv);
        nstep0 += s.len();
        steps.extend(s);
    }

    let ngone = imap2 - imap1;
    simplify_steps(&mut steps);
    let unit = steps.is_empty();
    // C condition (d): a lone inverted SpecMap is still normalised forward.
    let simpler = ngone > 0 || steps.len() < nstep0 || unit || where_inv;
    if !simpler {
        return super::simplify::default_map_merge(ctx, where_, series);
    }

    let new: DynMapping = if unit {
        Box::new(UnitMap::new(nin))
    } else {
        // C rebuilds the merged SpecMap via `AddSpecCvt` for each surviving
        // step, which blanks the cached (pre-calculated) argument slots.
        reset_cached_args(&mut steps);
        Box::new(SpecMap {
            conversions: steps,
            nin,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
            fcorr: Default::default(),
        })
    };
    ctx.replace_range_with(imap1, imap2, new);
    Some(imap1)
}

// ---------------------------------------------------------------------------
// Mapping trait
// ---------------------------------------------------------------------------

impl Mapping for SpecMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    fn nin(&self) -> usize {
        self.nin
    }

    fn nout(&self) -> usize {
        self.nin
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    /// NaN-as-bad fast path. Same approach as TimeMap: SpecMap conversion
    /// steps have per-step `BAD_VALUE` emit conditions (singular Doppler
    /// shift, light-time domain failures, etc.). Delegate-and-post-process
    /// is the safest minimal change; per-step audit can come later if a
    /// hot-path workload motivates it. SpecMap is 1D so the post-pass is
    /// N comparisons.
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
        if self.nin == 3 {
            self.transform_3d(&pts, fwd, &mut out);
            return Ok(());
        }

        // As C's Transform: each conversion in turn over every point
        // (specmap.c:135), a bad value staying bad and a non-finite result
        // becoming bad at the end.
        let npoint = pts.npoint();
        let o = &mut out.axis_mut(0)[..npoint];
        o.copy_from_slice(pts.axis(0));
        let nconv = self.conversions.len();
        for k in 0..nconv {
            let ci = if fwd { k } else { nconv - 1 - k };
            let cvt = &self.conversions[ci];
            if cvt.cvt_type.is_frame_change() {
                let factor = frame_change_factor(self.frame_factor(ci, cvt), fwd);
                for v in o.iter_mut() {
                    *v = if is_bad(*v) {
                        *v
                    } else if factor == BAD_VALUE {
                        BAD_VALUE
                    } else {
                        *v * factor
                    };
                }
            } else {
                let cvt_type = if fwd {
                    cvt.cvt_type
                } else {
                    cvt.cvt_type.inverse()
                };
                system_change_slice(cvt_type, &cvt.args, o);
            }
        }
        for v in o.iter_mut() {
            if !v.is_finite() {
                *v = BAD_VALUE;
            }
        }

        Ok(())
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        // SpecMap overrides the base finite-difference `astRate` with an
        // analytic derivative for a single frequency<->wavelength conversion,
        // mirroring `Rate` in `reference/starlink-ast/src/specmap.c:2640`.
        //
        // A SpecMap with no conversions is the identity, so its rate is 1.0
        // (specmap.c:2710). For a single WVTOFR/FRTOWV step the analytic
        // expression `d/dx (c/x) = -c/x^2` holds in both directions, because
        // frequency<->wavelength is the reciprocal relation `y = c/x` either
        // way. C reads the *raw* first conversion type (`cvttype[0]`) and
        // ignores the invert flag for exactly this reason (specmap.c:2713).
        // Any other shape (more than one step, or a first step that is not a
        // freq<->wave pair) defers to the parent finite-difference estimator
        // (specmap.c:2718-2720).
        if self.conversions.is_empty() {
            return Ok(1.0);
        }
        let cvt = self.conversions[0].cvt_type;
        if self.nin == 1
            && self.conversions.len() == 1
            && matches!(cvt, SpecCvtType::WvToFr | SpecCvtType::FrToWv)
        {
            let x = at[ax2 - 1];
            if is_bad(x) {
                return Ok(BAD_VALUE);
            }
            return Ok(-C_LIGHT / (x * x));
        }
        numeric_rate(self, at, ax1, ax2)
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        spec_map_merge(ctx, where_, series)
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
        let other_spec = other.as_any().downcast_ref::<SpecMap>()?;
        if other_spec.nin != self.nin {
            return None;
        }

        // Normalize both SpecMaps accounting for their inversion flags.
        let self_steps = normalized_steps(self, self_inv != self.inverted);
        let other_steps = normalized_steps(other_spec, other_inv != other_spec.inverted);

        // Concatenate: self is applied first, then other.
        let mut steps = self_steps;
        steps.extend(other_steps);

        // Try to cancel adjacent inverse pairs.  Always merge even if no
        // cancellation occurred — the combined list enables future passes to
        // find new cancellations when a third SpecMap appears (C specmap.c:2259-2288).
        simplify_steps(&mut steps);

        if steps.is_empty() {
            Some(Box::new(UnitMap::new(self.nin)))
        } else {
            // C rebuilds via `AddSpecCvt`, blanking cached argument slots.
            reset_cached_args(&mut steps);
            Some(Box::new(SpecMap {
                conversions: steps,
                nin: self.nin,
                inverted: false,
                simplified: SimplifiedRecord::default(),
                ident: None,
                id: Default::default(),
                mapping_flags: crate::mappings::MappingFlags::default(),
                fcorr: Default::default(),
            }))
        }
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<SpecMap>() else {
            return false;
        };
        if self.nin != other.nin || self.inverted != other.inverted {
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
                    && a.args.iter().zip(&b.args).enumerate().all(|(idx, (x, y))| {
                        // Skip cached correction factor
                        if a.cvt_type.is_frame_change() && idx == a.args.len() - 1 {
                            true
                        } else {
                            ast_equal(*x, *y)
                        }
                    })
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

/// Argument index to letter: 0 -> 'a', 1 -> 'b', etc. (lowercase for SpecMap).
fn arg_letter(j: usize) -> char {
    (b'a' + j as u8) as char
}

impl Serializable for SpecMap {
    fn class_name(&self) -> &'static str {
        "SpecMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Conversion between spectral coordinate systems".to_string());

        // Mapping base attributes.
        write_mapping_base(&mut node, self);

        let nspec = self.conversions.len();
        if nspec > 0 {
            node.push_int(
                "Nspec",
                nspec as i64,
                Some("Number of conversion steps".to_string()),
            );

            for (i, cvt) in self.conversions.iter().enumerate() {
                let idx = i + 1; // 1-based
                node.push_string(
                    &format!("Spec{idx}"),
                    cvt.cvt_type.code_string(),
                    Some(cvt.cvt_type.description().to_string()),
                );
                // Write all stored args (szargs), skipping BAD values
                let descs = cvt.cvt_type.arg_descriptions();
                for (j, &arg) in cvt.args.iter().enumerate() {
                    if arg != BAD_VALUE {
                        let letter = arg_letter(j);
                        let comment = descs.get(j).map(|d| d.to_string());
                        node.push_double(&format!("Spec{idx}{letter}"), arg, comment);
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
        let node = &whole.section("SpecMap");
        let base = read_mapping_base(whole);

        let nspec = node.get_int("Nspec", 0) as usize;

        let mut spec = SpecMap::with_nin(base.nin)?;

        for i in 0..nspec {
            let idx = i + 1;
            let code_str = node.get_string(&format!("Spec{idx}"), "");
            let cvt_type = SpecCvtType::from_code(&code_str).ok_or_else(|| {
                AstError::new(
                    ErrorCode::Spcin,
                    format!("unknown SpecMap conversion type: '{code_str}'"),
                )
            })?;

            let szargs = cvt_type.szargs();
            let mut args = Vec::with_capacity(szargs);
            for j in 0..szargs {
                let letter = arg_letter(j);
                let val = node.get_double(&format!("Spec{idx}{letter}"), BAD_VALUE);
                args.push(val);
            }

            // Store directly (bypass `add` to preserve cached correction factors from file)
            spec.conversions.push(SpecConversion { cvt_type, args });
        }

        if base.inverted {
            spec.invert();
        }

        spec.set_has_been_simplified(base.has_been_simplified);

        Ok(spec)
    }
}
