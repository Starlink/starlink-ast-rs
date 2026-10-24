//! The celestial coordinate frame (SkyFrame).

use std::cell::Cell;

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::axis::FrameAxis;
use crate::frames::sky_convert::{SkyRefIs, SkySystem, make_offset_mapping};
use crate::frames::skyaxis::{SkyAxis, default_dhms_format, dhms_unit, unformat_sky_axis};
use crate::frames::{AxisKind, DynFrame, Frame, FrameRef, LineDef, SkyLineDef};
use crate::mappings::helpers::is_bad;
use crate::mappings::{CmpMap, Composition, DynMapping, UnitMap};
use crate::pal;
use crate::serial::{Node, Serializable};

/// Radians to degrees and back, as C's `AST__DR2D` and `AST__DD2R`.
const AST_DR2D: f64 = 57.295_779_513_082_32;
const AST_DD2R: f64 = 0.017_453_292_519_943_295;

/// A celestial coordinate system description.
///
/// A SkyFrame is a specialized form of Frame which describes celestial
/// longitude/latitude coordinate systems.
/// The particular celestial coordinate system is given by its
/// [`system`](SkyFrame::system) (ICRS unless set), qualified as necessary by
/// a mean [`equinox_mjd`](SkyFrame::equinox_mjd) and/or an
/// [`epoch_mjd`](SkyFrame::epoch_mjd).
/// Distances, angles, offsets and normalization use spherical geometry.
///
/// For each of the supported celestial coordinate systems, a SkyFrame can
/// apply an optional shift of origin to create a coordinate system
/// representing offsets within the celestial coordinate system from some
/// specified reference point.
/// This offset coordinate system can also be rotated to define new
/// longitude and latitude axes; see [`sky_ref`](SkyFrame::sky_ref),
/// [`sky_ref_is`](SkyFrame::sky_ref_is), [`sky_ref_p`](SkyFrame::sky_ref_p)
/// and [`align_offset`](SkyFrame::align_offset).
///
/// All the coordinate values used by a SkyFrame are in radians.
/// [`format`](Frame::format) turns them into the more conventional
/// sexagesimal text that each axis's [`format_str`](SkyFrame::format_str)
/// describes, and [`unformat`](Frame::unformat) reads such text back.
/// The [`unit`](SkyFrame::unit) of a SkyFrame axis therefore describes the
/// formatted value, such as `"h:m:s"` for colon-separated hours, minutes and
/// seconds, while its [`internal_unit`](Frame::internal_unit) is always
/// `"rad"`, the units of the floating point values application code uses.
///
/// A SkyFrame has two axes: longitude first, then latitude, unless
/// [`perm_axes`](Frame::perm_axes) changes the order;
/// [`lon_axis`](SkyFrame::lon_axis) and [`lat_axis`](SkyFrame::lat_axis)
/// give their current indices.
/// Axis indices are 1-based.
/// Each axis has its own label, symbol, unit, format, digits, direction, top
/// and bottom, as every Frame's axes do, with defaults suited to the
/// celestial coordinate system, and [`as_time`](SkyFrame::as_time) chooses
/// whether its values are formatted as angles or times.
///
/// # Attributes
///
/// In addition to the attributes common to all Frames, a SkyFrame has:
///
/// - [`align_offset`](SkyFrame::align_offset): align SkyFrames using the
///   offset coordinate system?
/// - [`as_time`](SkyFrame::as_time): format celestial coordinates as times?
/// - [`equinox_mjd`](SkyFrame::equinox_mjd): epoch of the mean equinox.
/// - [`is_lat_axis`](SkyFrame::is_lat_axis): is the given axis the latitude
///   axis?
/// - [`is_lon_axis`](SkyFrame::is_lon_axis): is the given axis the longitude
///   axis?
/// - [`lat_axis`](SkyFrame::lat_axis): index of the latitude axis.
/// - [`lon_axis`](SkyFrame::lon_axis): index of the longitude axis.
/// - [`neg_lon`](SkyFrame::neg_lon): display longitude values in the range
///   -pi to pi?
/// - [`projection`](SkyFrame::projection): sky projection description.
/// - [`sky_ref`](SkyFrame::sky_ref): position defining the location of the
///   offset coordinate system.
/// - [`sky_ref_is`](SkyFrame::sky_ref_is): selects the nature of the offset
///   coordinate system.
/// - [`sky_ref_p`](SkyFrame::sky_ref_p): position defining the orientation
///   of the offset coordinate system.
/// - [`sky_tol`](SkyFrame::sky_tol): smallest significant shift in sky
///   coordinates.
///
/// # Examples
///
/// ```
/// use starlink_ast::frames::{Frame, SkyFrame, SkySystem};
///
/// let sky = SkyFrame::new(SkySystem::Fk5);
/// assert_eq!(sky.naxes(), 2);
/// assert_eq!(sky.system(), SkySystem::Fk5);
/// ```
// The crate-private SkyAxis class is documented here (and on the Frame
// trait's per-axis attributes), which is why its C documentation ledger row
// is merged into SkyFrame.
#[derive(Debug, Clone)]
pub struct SkyFrame {
    /// The Frame-class attributes.
    attrs: super::frame_attrs::FrameAttrs,
    // Frame-level

    // Observer (Frame trait in C)

    // SkyFrame-specific
    system: SkySystem,
    /// Tracks whether `system` was set via `set_system` or loaded
    /// from a native node (mirrors C `astTestSystem` for SkyFrame).
    system_set: bool,
    equinox: Option<f64>, // MJD of mean equinox
    projection: Option<String>,
    neglon: bool,
    /// Tracks whether `neglon` was explicitly set (mirrors C
    /// `astTestNegLon`).
    neglon_set: bool,
    sky_tol: Option<f64>, // smallest significant distance (arcseconds)
    sky_ref: [f64; 2],    // offset origin/pole [lon, lat] (radians)
    /// Per-axis "explicitly set" flags for `sky_ref` (mirrors C
    /// `astTestSkyRef`).
    sky_ref_set: [bool; 2],
    sky_ref_p: [f64; 2], // offset meridian point [lon, lat]
    /// Per-axis "explicitly set" flags for `sky_ref_p` (mirrors C
    /// `astTestSkyRefP`).
    sky_ref_p_set: [bool; 2],
    sky_ref_is: SkyRefIs,
    sky_ref_is_set: bool,
    align_offset: bool,
    /// Tracks whether `align_offset` was explicitly set (mirrors C
    /// `astTestAlignOffset`).
    align_offset_set: bool,
    align_system: Option<SkySystem>,

    // Per-axis properties
    /// The Axis objects, in physical order: [longitude, latitude]. Each is
    /// normally a SkyAxis, but C's SkyFrame holds whatever Axis objects it
    /// is given, and `astLoadFrame` supplies a plain Axis for a description
    /// with no `Ax<n>` object (frame.c:15227). The methods that need a
    /// SkyAxis check for one (skyframe.c `astIsASkyAxis`).
    axes: [FrameAxis; 2],

    /// Axis permutation: perm[ext_axis] = physical_axis (0-based).
    /// Physical 0 = longitude, physical 1 = latitude.
    /// After swap: perm = [1, 0] means external axis 0 → latitude.
    perm: [usize; 2],
    /// The LAST (radians) at the TDB MJD `eplast`, and the ratio of solar
    /// to sidereal time used to extrapolate from it, or `BAD_VALUE` when
    /// not yet known (skyframe.c `last`, `eplast`, `klast`). A conversion
    /// fills them through `&self`, and a copy keeps them, as C's does.
    last: Cell<f64>,
    eplast: Cell<f64>,
    klast: Cell<f64>,
    /// The magnitude of the diurnal aberration vector, or `BAD_VALUE` when
    /// not yet known (skyframe.c `diurab`).
    diurab: Cell<f64>,
    // Match-control attributes (used by `astFindFrame` / `astMatch`).
    // None = "use the trait default" (which returns 2 for SkyFrame's
    // naxes-based attributes, false/false/false for the booleans
    // overridden below).
}

impl SkyFrame {
    /// Create a SkyFrame whose System is not set, so that it describes the
    /// default celestial coordinate system, ICRS.
    ///
    /// Every attribute takes its default.
    /// Because the System is unset, when the SkyFrame is the template of
    /// [`find_frame_template`](crate::frames::FrameSearch::find_frame_template), the Frame found
    /// keeps the target's System rather than taking the template's.
    /// The default System may change in future as new astrometric standards
    /// evolve, so use [`SkyFrame::new`] with [`SkySystem::Icrs`] if ICRS is
    /// what you need.
    ///
    /// ```
    /// use starlink_ast::frames::{SkyFrame, SkySystem};
    ///
    /// let sky = SkyFrame::unset_system();
    /// assert_eq!(sky.system(), SkySystem::Icrs);
    /// assert!(!sky.test_system());
    /// ```
    // C: `astSkyFrame("")`.
    #[doc(alias = "astSkyFrame")]
    pub fn unset_system() -> Self {
        // ICRS with the System unset. Mirrors C `astSkyFrame("")`.
        Self {
            system_set: false,
            ..Self::new(SkySystem::Icrs)
        }
    }

    /// Create a SkyFrame describing the celestial coordinate system
    /// `system`.
    ///
    /// The System is set, so as a template of
    /// [`find_frame_template`](crate::frames::FrameSearch::find_frame_template) the SkyFrame asks
    /// for that System even when it is the default, ICRS;
    /// [`SkyFrame::unset_system`] makes one whose System is unset.
    /// Every other attribute takes its default: among them the Domain is
    /// `"SKY"`, and the Equinox and Epoch are B1950.0 for the FK4 systems and
    /// J2000.0 for the others.
    /// Qualify the system as necessary with the setters, such as
    /// [`set_equinox_year`](SkyFrame::set_equinox_year) and
    /// [`set_epoch_mjd`](SkyFrame::set_epoch_mjd), and request extra
    /// precision for formatted values with [`set_digits`](Frame::set_digits).
    ///
    /// The SkyFrame has two axes, longitude then latitude, whose order
    /// [`perm_axes`](Frame::perm_axes) can change.
    /// When two SkyFrames are converted with
    /// [`convert_to`](crate::frames::FrameSearch::convert_to), the conversion takes account of
    /// the celestial coordinate systems they represent, with any qualifying
    /// Equinox and Epoch, and of their
    /// [`align_system`](SkyFrame::align_system), so the result fully reflects
    /// the relationship between positions on the sky measured in the two
    /// systems.
    ///
    /// ```
    /// use starlink_ast::frames::{Frame, SkyFrame, SkySystem};
    ///
    /// // FK5 with a mean equinox of J2005.0, and 10 digits of precision.
    /// let mut fk5 = SkyFrame::new(SkySystem::Fk5);
    /// fk5.set_equinox_year(2005.0);
    /// fk5.set_digits(10).unwrap();
    /// assert!((fk5.equinox_year() - 2005.0).abs() < 1e-9);
    /// assert_eq!(fk5.lon_axis(), 1);
    ///
    /// // FK4 takes a default Epoch of B1950.0.
    /// let fk4 = SkyFrame::new(SkySystem::Fk4);
    /// assert!((fk4.epoch_year() - 1950.0).abs() < 1e-9);
    /// ```
    // C: `astSkyFrame("System=...")`.
    #[doc(alias = "astSkyFrame")]
    pub fn new(system: SkySystem) -> Self {
        Self {
            // None = use the default "SKY". Mirrors C's astTestDomain
            // returning false until set_domain is called.
            system,
            system_set: true,
            equinox: None,
            projection: None,
            neglon: false,
            neglon_set: false,
            sky_tol: None,
            sky_ref: [0.0, 0.0],
            sky_ref_set: [false, false],
            sky_ref_p: [0.0, 0.0],
            sky_ref_p_set: [false, false],
            sky_ref_is: SkyRefIs::Ignored,
            sky_ref_is_set: false,
            align_offset: false,
            align_offset_set: false,
            align_system: None,
            axes: [
                FrameAxis::Sky(SkyAxis::new()),
                FrameAxis::Sky(SkyAxis::new()),
            ],
            attrs: super::frame_attrs::FrameAttrs::default(),
            perm: [0, 1],
            last: Cell::new(BAD_VALUE),
            eplast: Cell::new(BAD_VALUE),
            klast: Cell::new(BAD_VALUE),
            diurab: Cell::new(BAD_VALUE),
        }
    }

    /// The axis permutation: `axis_permutation()[i]` is the zero-based
    /// internal axis (0 for longitude, 1 for latitude) presented as the
    /// zero-based axis `i`.
    // C: the Frame `perm` array, as `astPrimaryFrame` uses it.
    #[inline]
    pub fn axis_permutation(&self) -> [usize; 2] {
        self.perm
    }

    /// Is the 1-based `axis` a celestial latitude axis? The read-only
    /// `IsLatAxis(axis)` attribute.
    ///
    /// True if the axis is a celestial latitude axis (declination, galactic
    /// latitude, and so on), false otherwise.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "IsLatAxis")]
    pub fn is_lat_axis(&self, axis: usize) -> AstResult<bool> {
        Ok(self.physical_axis(axis)? == 1)
    }

    /// Is the 1-based `axis` a celestial longitude axis? The read-only
    /// `IsLonAxis(axis)` attribute.
    ///
    /// True if the axis is a celestial longitude axis (right ascension,
    /// galactic longitude, and so on), false otherwise.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    #[doc(alias = "IsLonAxis")]
    pub fn is_lon_axis(&self, axis: usize) -> AstResult<bool> {
        Ok(self.physical_axis(axis)? == 0)
    }

    /// The index of the longitude axis: the read-only LonAxis attribute.
    ///
    /// 1 or 2, taking account of any axis permutation.
    #[doc(alias = "LonAxis")]
    #[inline]
    pub fn lon_axis(&self) -> usize {
        self.lon_pos() + 1
    }

    /// The index of the latitude axis: the read-only LatAxis attribute.
    ///
    /// 1 or 2, taking account of any axis permutation.
    ///
    /// ```
    /// use starlink_ast::frames::{Frame, SkyFrame, SkySystem};
    ///
    /// let mut sky = SkyFrame::new(SkySystem::Galactic);
    /// assert_eq!((sky.lon_axis(), sky.lat_axis()), (1, 2));
    /// sky.perm_axes(&[2, 1]).unwrap();
    /// assert_eq!((sky.lon_axis(), sky.lat_axis()), (2, 1));
    /// assert!(sky.is_lat_axis(1).unwrap());
    /// ```
    #[doc(alias = "LatAxis")]
    #[inline]
    pub fn lat_axis(&self) -> usize {
        self.lat_pos() + 1
    }

    /// Position of longitude in coordinate arrays (0-based).
    #[inline]
    pub(crate) fn lon_pos(&self) -> usize {
        if self.perm[0] == 0 { 0 } else { 1 }
    }

    /// Position of latitude in coordinate arrays (0-based).
    #[inline]
    pub(crate) fn lat_pos(&self) -> usize {
        if self.perm[0] == 1 { 0 } else { 1 }
    }

    // --- Per-axis Label / Symbol (1-based external API) ---

    /// Resolve a 1-based external axis index into 0-based physical
    /// storage (lon=0, lat=1) using the permutation. Returns
    /// `Err(Axiin)` if `axis` is out of range.
    fn physical_axis(&self, axis: usize) -> AstResult<usize> {
        if !(1..=2).contains(&axis) {
            return Err(AstError::new_tracked(
                ErrorCode::Axiin,
                format!("axis index {} is out of range 1..=2", axis),
            ));
        }
        Ok(self.perm[axis - 1])
    }

    /// Effective `AsTime` for the physical axis `phys` (0 = longitude,
    /// 1 = latitude), mirroring C `astGetAsTime` (skyframe.c:2547-2566).
    ///
    /// If the axis has an explicit `AsTime` override it is honored.
    /// Otherwise the longitude axis is formatted as a time only when the
    /// system is equatorial (involves the Earth's equator); the latitude
    /// axis and all non-equatorial longitudes default to angle (degrees).
    fn effective_as_time(&self, phys: usize) -> bool {
        // Only a SkyAxis can have AsTime set (skyframe.c:2563).
        if let FrameAxis::Sky(sky) = &self.axes[phys]
            && let Some(set) = sky.as_time
        {
            return set;
        }
        // skyframe.c:2503 tells the axes apart by their internal index, not
        // by the SkyAxis IsLatitude attribute, which a SkyFrame never sets.
        phys == 0 && is_equatorial(self.system)
    }

    /// The Axis-class attributes of a 1-based external axis.
    fn ax(&self, axis: usize) -> AstResult<&super::axis::Axis> {
        Ok(self.axes[self.physical_axis(axis)?].axis())
    }

    /// The Axis-class attributes of a 1-based external axis, to change.
    fn ax_mut(&mut self, axis: usize) -> AstResult<&mut super::axis::Axis> {
        let phys = self.physical_axis(axis)?;
        Ok(self.axes[phys].axis_mut())
    }

    // --- Per-axis Format / Unit / AsTime / Digits (1-based external API) ---

    /// Format celestial coordinates as times? The `AsTime(axis)` attribute
    /// of the 1-based `axis`, set or default.
    ///
    /// The default style of formatting used (by [`format`](Frame::format),
    /// for instance) for the celestial coordinate values on the axis: false
    /// formats them as angles (degrees, minutes and seconds), true as times
    /// (hours, minutes and seconds).
    /// Each axis has its own value, so the style for latitude values, for
    /// instance, is set on the latitude axis.
    ///
    /// Default: chosen from the [`system`](SkyFrame::system), so that
    /// right ascension, for instance, is formatted as a time, following
    /// normal conventions: true for the longitude axis of an equatorial
    /// system (FK4, FK4-NO-E, FK5, J2000, ICRS or GAPPT), false otherwise.
    ///
    /// AsTime works by changing the default
    /// [`format_str`](SkyFrame::format_str) of the axis, which in turn may
    /// change its default [`unit`](SkyFrame::unit).
    /// Only the default style is affected: a Format set on the axis
    /// overrides AsTime.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    ///
    /// ```
    /// use starlink_ast::frames::{Frame, SkyFrame, SkySystem};
    ///
    /// let mut sky = SkyFrame::new(SkySystem::Fk5);
    /// assert!(sky.as_time(1).unwrap());
    /// assert!(!sky.as_time(2).unwrap());
    /// sky.set_as_time(1, false).unwrap();
    /// assert_eq!(sky.format(1, 0.5).unwrap(), "28:38:52");
    /// ```
    // C: `astGetAsTime` (skyframe.c:2503).
    #[doc(alias = "AsTime")]
    pub fn as_time(&self, axis: usize) -> AstResult<bool> {
        Ok(self.effective_as_time(self.physical_axis(axis)?))
    }

    /// Set `AsTime(axis)`; see [`as_time`](SkyFrame::as_time).
    // C: `astSetAsTime` first replaces a plain Axis with a SkyAxis carrying
    // its attributes (skyframe.c:8851).
    pub fn set_as_time(&mut self, axis: usize, as_time: bool) -> AstResult<()> {
        let phys = self.physical_axis(axis)?;
        self.sky_axis_mut(phys).as_time = Some(as_time);
        Ok(())
    }

    /// Whether `AsTime(axis)` is set; see [`as_time`](SkyFrame::as_time).
    // C: only a SkyAxis can have AsTime set (skyframe.c:10632).
    pub fn test_as_time(&self, axis: usize) -> AstResult<bool> {
        Ok(match &self.axes[self.physical_axis(axis)?] {
            FrameAxis::Sky(sky) => sky.as_time.is_some(),
            FrameAxis::Plain(_) => false,
        })
    }

    /// Clear `AsTime(axis)`; see [`as_time`](SkyFrame::as_time).
    // C: only a SkyAxis can have AsTime set (skyframe.c:1411).
    pub fn clear_as_time(&mut self, axis: usize) -> AstResult<()> {
        let phys = self.physical_axis(axis)?;
        if let FrameAxis::Sky(sky) = &mut self.axes[phys] {
            sky.as_time = None;
        }
        Ok(())
    }

    /// The SkyAxis on the physical axis `phys`, replacing a plain Axis with
    /// a SkyAxis that carries its attributes first, as skyframe.c
    /// `SetAsTime` does with `astAxisOverlay`.
    fn sky_axis_mut(&mut self, phys: usize) -> &mut SkyAxis {
        if let FrameAxis::Plain(_) = &self.axes[phys] {
            let mut upgraded = FrameAxis::Sky(SkyAxis::new());
            upgraded.overlay_from(&self.axes[phys]);
            self.axes[phys] = upgraded;
        }
        match &mut self.axes[phys] {
            FrameAxis::Sky(sky) => sky,
            FrameAxis::Plain(_) => unreachable!("replaced by a SkyAxis above"),
        }
    }

    /// Clear the SkyFrame's Digits, which then defaults to 7.
    pub fn clear_digits(&mut self) {
        self.attrs.digits = None;
    }

    /// Default per-axis Symbol from the current celestial system.
    /// Returns the C library's two-letter / four-letter convention
    /// (e.g. `("RA", "DEC")` for FK5, `("GLON", "GLAT")` for
    /// galactic). Used as the fallback when no override is set.
    fn default_symbols(&self) -> (&'static str, &'static str) {
        // Matches C `astGetSymbol` (skyframe.c:4230-4256).
        match self.system {
            SkySystem::Galactic => ("l", "b"),
            SkySystem::Ecliptic | SkySystem::Helioecliptic => ("Lambda", "Beta"),
            SkySystem::Supergalactic => ("SGL", "SGB"),
            SkySystem::Azel => ("Az", "El"),
            SkySystem::Unknown => ("Lon", "Lat"),
            // FK4/FK5/ICRS/J2000/GAPPT/FK4NoE all use RA/Dec.
            _ => ("RA", "Dec"),
        }
    }

    // --- SkyFrame-specific attributes ---

    /// The celestial coordinate system: the System attribute, set or
    /// default.
    ///
    /// [`Frame::system_string`] gives its name, and [`SkySystem::parse`]
    /// accepts any of the names below, ignoring case.
    /// The systems are:
    ///
    /// - [`SkySystem::Azel`] (`"AZEL"`): horizon coordinates, azimuth zero
    ///   at geographic north and +pi/2 at east, elevation +pi/2 at the
    ///   zenith.
    ///   No correction is applied for atmospheric refraction or polar
    ///   motion, but diurnal aberration is corrected.
    ///   Unlike most celestial systems it is right-handed, and it is
    ///   sensitive to the timescale of the Epoch, since its diurnal rotation
    ///   turns a small change of time into a large rotation: give the Epoch
    ///   of both source and destination SkyFrames as TDB (TT, within 1 to 2
    ///   milliseconds of TDB and equal to TAI + 32.184 seconds, will usually
    ///   do).
    /// - [`SkySystem::Ecliptic`] (`"ECLIPTIC"`): ecliptic coordinates (IAU
    ///   1980), referred to the ecliptic and mean equinox of the
    ///   [`equinox_mjd`](SkyFrame::equinox_mjd).
    /// - [`SkySystem::Fk4`] (`"FK4"`): the old FK4 (barycentric) equatorial
    ///   system, qualified by an Equinox, and by an Epoch for accurate work
    ///   since its model rotates slowly with time.
    /// - [`SkySystem::Fk4NoE`] (`"FK4-NO-E"` or `"FK4_NO_E"`): FK4 without
    ///   the E-terms of aberration (as in some radio catalogs), qualified by
    ///   both an Equinox and an Epoch.
    /// - [`SkySystem::Fk5`] (`"FK5"` or `"EQUATORIAL"`): the modern FK5 (barycentric)
    ///   equatorial system, qualified by an Equinox.
    /// - [`SkySystem::Galactic`] (`"GALACTIC"`): galactic coordinates (IAU
    ///   1958).
    /// - [`SkySystem::Gappt`] (`"GAPPT"`, `"GEOCENTRIC"` or `"APPARENT"`):
    ///   geocentric apparent equatorial coordinates, relative to the true
    ///   equator and equinox at the Epoch (no Equinox is needed).
    ///   They are the apparent right ascension and declination for a
    ///   fictitious observer at the Earth's center, derived from FK5 J2000.0
    ///   by allowing for the Sun's gravitational deflection, the aberration
    ///   from the Earth's motion about the barycenter, and precession and
    ///   nutation; atmospheric refraction and the aberration from the
    ///   Earth's rotation are ignored.
    /// - [`SkySystem::Helioecliptic`] (`"HELIOECLIPTIC"`): ecliptic
    ///   coordinates (IAU 1980) referred to the ecliptic and mean equinox of
    ///   J2000.0, with the longitude offset so that the Sun is at zero
    ///   longitude at the Epoch; any Equinox set is ignored.
    /// - [`SkySystem::Icrs`] (`"ICRS"`): the International Celestial
    ///   Reference System, realized through the Hipparcos catalog; very close
    ///   to FK5 J2000, differing only at the 50 milli-arcsecond level; no
    ///   Equinox is needed.
    /// - [`SkySystem::J2000`] (`"J2000"`): the mean dynamical equator and
    ///   equinox of J2000, which differ slightly from FK5's, so it differs
    ///   slightly from FK5 with Equinox J2000; no Equinox is needed.
    /// - [`SkySystem::Supergalactic`] (`"SUPERGALACTIC"`): de Vaucouleurs
    ///   supergalactic coordinates.
    /// - [`SkySystem::Unknown`] (`"UNKNOWN"`): any other spherical system; no
    ///   Mapping can be made between two SkyFrames if either has this
    ///   System.
    ///
    /// Default: [`SkySystem::Icrs`], which may change as astrometric
    /// standards evolve, so set ICRS explicitly if it is what you need.
    pub fn system(&self) -> SkySystem {
        self.system
    }

    /// Set the System; see [`system`](SkyFrame::system).
    ///
    /// A set SkyRef or SkyRefP is re-expressed in the new System, so that it
    /// stays at the same place on the sky, and is cleared if no conversion
    /// to the new System exists.
    // C: skyframe.c `SetSystem`.
    pub fn set_system(&mut self, sys: SkySystem) {
        let saved = self.saved_sky_refs();
        self.system = sys;
        self.system_set = true;
        if let Some(saved) = saved {
            self.reexpress_sky_refs(saved, true);
        }
    }

    /// Whether the System is set; see [`system`](SkyFrame::system).
    pub fn test_system(&self) -> bool {
        self.system_set
    }

    /// Clear the System, which reverts to its default, ICRS; see
    /// [`system`](SkyFrame::system).
    ///
    /// A set SkyRef or SkyRefP is re-expressed in ICRS, as for
    /// [`set_system`](Self::set_system).
    // C: skyframe.c `ClearSystem`.
    pub fn clear_system(&mut self) {
        let saved = self.saved_sky_refs();
        self.system = SkySystem::Icrs;
        self.system_set = false;
        if let Some(saved) = saved {
            self.reexpress_sky_refs(saved, false);
        }
    }

    /// A copy of this SkyFrame, taken before its System changes, when
    /// SkyRef or SkyRefP is set and so will need re-expressing.
    fn saved_sky_refs(&self) -> Option<SkyFrame> {
        let set = self
            .sky_ref_set
            .iter()
            .chain(&self.sky_ref_p_set)
            .any(|&s| s);
        set.then(|| self.clone())
    }

    /// Re-express the SkyRef and SkyRefP of `old`, this SkyFrame before its
    /// System changed, in the current System. `restore_offsets` selects the
    /// `SetSystem` behavior, which aligns the two in absolute coordinates and
    /// then restores AlignOffset and SkyRefIs; `ClearSystem` does neither
    /// (skyframe.c:9635, 1814).
    fn reexpress_sky_refs(&mut self, mut old: SkyFrame, restore_offsets: bool) {
        let skyref_set = old.sky_ref_set[0] || old.sky_ref_set[1];
        let skyrefp_set = old.sky_ref_p_set[0] || old.sky_ref_p_set[1];
        // External-order (axis 1, axis 2) values of SkyRef and SkyRefP,
        // with each one's default where unset, as C's astGetSkyRef gives.
        let xin = [
            old.sky_ref(1).unwrap_or(0.0),
            old.sky_ref_p(1).unwrap_or(0.0),
        ];
        let yin = [
            old.sky_ref(2).unwrap_or(0.0),
            old.sky_ref_p(2).unwrap_or(0.0),
        ];
        let (aloff_set, aloff) = (old.align_offset_set, old.align_offset);
        let (skyrefis_set, skyrefis) = (old.sky_ref_is_set, old.sky_ref_is);

        // Clear them on both, so the conversion below does not recurse
        // into this re-expression.
        if skyref_set {
            old.sky_ref_set = [false; 2];
            old.sky_ref = [0.0; 2];
            self.sky_ref_set = [false; 2];
            self.sky_ref = [0.0; 2];
        }
        if skyrefp_set && restore_offsets {
            old.sky_ref_p_set = [false; 2];
            old.sky_ref_p = [0.0; 2];
            self.sky_ref_p_set = [false; 2];
            self.sky_ref_p = [0.0; 2];
        }
        if restore_offsets {
            // Align in absolute coordinates.
            old.set_align_offset(false);
            old.set_sky_ref_is(SkyRefIs::Ignored);
        }

        let converted = super::find_frame::convert_frames(&old, &*self, "")
            .ok()
            .flatten()
            .and_then(|fs| fs.mapping(FrameRef::Base, FrameRef::Current).ok())
            .and_then(|map| map.tran2(&xin, &yin, true).ok());
        match converted {
            Some((xout, yout)) => {
                if skyref_set {
                    self.set_sky_ref_pair(xout[0], yout[0]);
                }
                if skyrefp_set {
                    self.set_sky_ref_p_pair(xout[1], yout[1]);
                }
                if restore_offsets {
                    if aloff_set {
                        self.set_align_offset(aloff);
                    } else {
                        self.clear_align_offset();
                    }
                    if skyrefis_set {
                        self.set_sky_ref_is(skyrefis);
                    } else {
                        self.clear_sky_ref_is();
                    }
                }
            }
            None => {
                if skyref_set {
                    self.sky_ref_set = [false; 2];
                    self.sky_ref = [0.0; 2];
                }
                if skyrefp_set {
                    self.sky_ref_p_set = [false; 2];
                    self.sky_ref_p = [0.0; 2];
                }
            }
        }
    }

    /// Set both SkyRef values from external-order (axis 1, axis 2) values.
    fn set_sky_ref_pair(&mut self, axis1: f64, axis2: f64) {
        let _ = self.set_sky_ref(1, axis1);
        let _ = self.set_sky_ref(2, axis2);
    }

    /// Set both SkyRefP values from external-order (axis 1, axis 2) values.
    fn set_sky_ref_p_pair(&mut self, axis1: f64, axis2: f64) {
        let _ = self.set_sky_ref_p(1, axis1);
        let _ = self.set_sky_ref_p(2, axis2);
    }

    /// The coordinate system in which SkyFrames are aligned: the
    /// AlignSystem attribute, set or default.
    ///
    /// When two SkyFrames are aligned by
    /// [`convert_to`](crate::frames::FrameSearch::convert_to) or
    /// [`find_frame_template`](crate::frames::FrameSearch::find_frame_template), positions are
    /// mapped through this system (see
    /// [`BasicFrame::align_system`](super::BasicFrame::align_system) for the
    /// general rule), unless both use their offset coordinate systems for
    /// alignment (see [`align_offset`](SkyFrame::align_offset)).
    ///
    /// Default: [`SkySystem::Icrs`].
    // C: skyframe.c:4281 `GetAlignSystem`.
    pub fn align_system(&self) -> SkySystem {
        self.align_system.unwrap_or(SkySystem::Icrs)
    }

    /// Set the AlignSystem; see [`align_system`](SkyFrame::align_system).
    pub fn set_align_system(&mut self, sys: SkySystem) {
        self.align_system = Some(sys);
    }

    /// Whether the AlignSystem is set; see
    /// [`align_system`](SkyFrame::align_system).
    pub fn test_align_system(&self) -> bool {
        self.align_system.is_some()
    }

    /// Clear the AlignSystem; see [`align_system`](SkyFrame::align_system).
    pub fn clear_align_system(&mut self) {
        self.align_system = None;
    }

    /// The epoch of the mean equinox, as a Modified Julian Date: the Equinox
    /// attribute, set or default.
    ///
    /// Qualifies the celestial coordinate systems which are notionally based
    /// on the ecliptic (the plane of the Earth's orbit around the Sun) and/or
    /// the Earth's equator.
    /// Both planes are in motion and their positions are difficult to
    /// specify precisely, so in practice a model ecliptic and/or equator are
    /// used instead.
    /// These, together with the point on the sky that defines the coordinate
    /// origin (the intersection of the two planes, termed the "mean
    /// equinox"), move with time according to a model which removes the more
    /// rapid fluctuations; the FK4 and FK5 models are supported.
    /// The position of a fixed source in any of these systems appears to
    /// change with time because the coordinate system itself moves, so the
    /// system must be qualified by a moment in time, the epoch of the mean
    /// equinox or "equinox" for short, which places the model coordinate
    /// system on the sky.
    /// [`system`](SkyFrame::system) says which systems use it.
    ///
    /// Take care to distinguish the Equinox, which defines a time-dependent
    /// coordinate system (based on solar system reference planes which are
    /// in motion), from the superficially similar
    /// [`epoch_mjd`](SkyFrame::epoch_mjd), which qualifies coordinate
    /// systems in which the positions of sources change with time (or appear
    /// to), for reasons such as the aberration of light caused by the
    /// observer's motion.
    ///
    /// The Equinox has no timescale of its own; where one matters it is
    /// taken as UTC, as when the FITS-WCS writer compares it with the Epoch
    /// converted from TDB to UTC to decide whether MJD-OBS and DATE-OBS can
    /// be left to default to it.
    ///
    /// Default: B1950.0 (Besselian) for the FK4-based systems and J2000.0
    /// (Julian) for all others.
    /// A helio-ecliptic SkyFrame always uses J2000.0.
    // C: fitschan.c `SkySys` makes the UTC comparison.
    #[doc(alias = "Equinox")]
    pub fn equinox_mjd(&self) -> f64 {
        self.equinox
            .unwrap_or_else(|| self.system.default_equinox_mjd())
    }

    /// The Equinox as a decimal year: a Besselian epoch before 1984.0, a
    /// Julian one from then on; see [`equinox_mjd`](SkyFrame::equinox_mjd).
    // C: as frame.c `Dump` writes it.
    pub fn equinox_year(&self) -> f64 {
        let mjd = self.equinox_mjd();
        if mjd < pal::epj2d(1984.0) {
            pal::epb(mjd)
        } else {
            pal::epj(mjd)
        }
    }

    /// Set the Equinox, as a Modified Julian Date; see
    /// [`equinox_mjd`](SkyFrame::equinox_mjd).
    ///
    /// A helio-ecliptic SkyFrame has no Equinox of its own, so for one the
    /// Equinox is cleared instead.
    pub fn set_equinox_mjd(&mut self, mjd: f64) {
        // A helio-ecliptic SkyFrame has no Equinox of its own, so setting
        // one clears it (skyframe.c:11436).
        self.equinox = (self.system != SkySystem::Helioecliptic).then_some(mjd);
    }

    /// The epoch of observation, as a TDB Modified Julian Date: the Epoch
    /// attribute, set or default; see [`Frame::epoch_mjd`].
    ///
    /// The coordinates of sources can change with time through the changing
    /// aberration of light from the observer's motion, changing
    /// gravitational deflection by the Sun, and the fictitious motion of
    /// non-inertial systems such as FK4 (but not the proper motion of a
    /// source, which belongs to the source rather than the coordinate
    /// system).
    /// The Epoch relates to such motion; the similar-sounding
    /// [`equinox_mjd`](SkyFrame::equinox_mjd) qualifies a coordinate system
    /// that is itself in motion because it is referred to a slowly moving
    /// plane such as the equator.
    ///
    /// Default: B1950.0 (Besselian) for the FK4-based systems and J2000.0
    /// (Julian) for the others.
    // C: skyframe.c:3210-3225, the SkyFrame `GetEpoch`.
    pub fn epoch_mjd(&self) -> f64 {
        self.attrs
            .epoch
            .unwrap_or_else(|| self.frame_default_epoch())
    }

    // Defaults of the Frame-class attributes, for the `frame_attrs_*`
    // macros.

    /// B1950.0 for the FK4 systems and J2000.0 otherwise (skyframe.c
    /// `GetEpoch`).
    fn frame_default_epoch(&self) -> f64 {
        match self.system {
            SkySystem::Fk4 | SkySystem::Fk4NoE => pal::epb2d(1950.0),
            _ => pal::epj2d(2000.0),
        }
    }
    fn frame_default_title(&self) -> AstResult<String> {
        Ok(self.default_title())
    }
    fn frame_default_domain(&self) -> &'static str {
        "SKY"
    }
    fn frame_default_permute(&self) -> bool {
        true
    }

    /// The Epoch as a decimal year: a Besselian epoch before 1984.0, a
    /// Julian one from then on; see [`epoch_mjd`](SkyFrame::epoch_mjd).
    // C: as frame.c `Dump` writes it.
    pub fn epoch_year(&self) -> f64 {
        let mjd = self.epoch_mjd();
        if mjd < pal::epj2d(1984.0) {
            pal::epb(mjd)
        } else {
            pal::epj(mjd)
        }
    }

    /// Whether the Epoch is set; see [`epoch_mjd`](SkyFrame::epoch_mjd).
    pub fn test_epoch(&self) -> bool {
        self.attrs.epoch.is_some()
    }

    /// Clear the Epoch; see [`epoch_mjd`](SkyFrame::epoch_mjd).
    pub fn clear_epoch(&mut self) {
        self.attrs.epoch = None;
    }

    /// Clear the Equinox; see [`equinox_mjd`](SkyFrame::equinox_mjd).
    pub fn clear_equinox(&mut self) {
        self.equinox = None;
    }

    /// Set the Epoch, as a TDB Modified Julian Date; see
    /// [`epoch_mjd`](SkyFrame::epoch_mjd).
    /// `BAD_VALUE` clears it.
    pub fn set_epoch_mjd(&mut self, mjd: f64) {
        // A bad Epoch is no Epoch (frame.c `TestEpoch` is `!= AST__BAD`).
        self.attrs.epoch = (mjd != crate::constants::BAD_VALUE).then_some(mjd);
    }

    /// Set the Epoch from a decimal year, such as a FITS `EPOCH` keyword;
    /// see [`epoch_mjd`](SkyFrame::epoch_mjd).
    ///
    /// A year before 1984.0 is a Besselian epoch, a later one Julian.
    // C: `astSet(skyframe, "Epoch=...")` through `astReadDateTime`.
    pub fn set_epoch_year(&mut self, year: f64) {
        self.set_epoch_mjd(crate::frames::date_time::year_to_mjd_iau1984(year));
    }

    /// Set the Equinox from a decimal year, such as a FITS `EQUINOX`
    /// keyword; see [`equinox_mjd`](SkyFrame::equinox_mjd).
    ///
    /// A year before 1984.0 is a Besselian epoch, a later one Julian.
    // C: `astSet(skyframe, "Equinox=...")` through `astReadDateTime`.
    pub fn set_equinox_year(&mut self, year: f64) {
        self.set_equinox_mjd(crate::frames::date_time::year_to_mjd_iau1984(year));
    }

    /// Display negative longitude values? The NegLon attribute, set or
    /// default.
    ///
    /// Controls how longitude values are normalized for display by
    /// [`norm`](Frame::norm): false gives normalized longitudes in the range
    /// zero to 2 pi, true in the range -pi to pi.
    ///
    /// Default: true when [`sky_ref_is`](SkyFrame::sky_ref_is) is
    /// [`SkyRefIs::Origin`], false otherwise.
    ///
    /// ```
    /// use starlink_ast::frames::{Frame, SkyFrame, SkySystem};
    ///
    /// let mut sky = SkyFrame::new(SkySystem::Galactic);
    /// let mut pos = [-0.5, 0.2];
    /// sky.norm(&mut pos).unwrap();
    /// assert!((pos[0] - (2.0 * std::f64::consts::PI - 0.5)).abs() < 1e-12);
    ///
    /// sky.set_neg_lon(true);
    /// let mut pos = [-0.5, 0.2];
    /// sky.norm(&mut pos).unwrap();
    /// assert_eq!(pos[0], -0.5);
    /// ```
    #[doc(alias = "NegLon")]
    pub fn neg_lon(&self) -> bool {
        // skyframe.c:11592: offset coordinates centered on the origin
        // default to negative longitudes.
        if self.neglon_set {
            self.neglon
        } else {
            self.sky_ref_is == SkyRefIs::Origin
        }
    }

    /// Set NegLon; see [`neg_lon`](SkyFrame::neg_lon).
    pub fn set_neg_lon(&mut self, neg: bool) {
        self.neglon = neg;
        self.neglon_set = true;
    }

    /// Whether NegLon is set; see [`neg_lon`](SkyFrame::neg_lon).
    pub fn test_neg_lon(&self) -> bool {
        self.neglon_set
    }

    /// Clear NegLon; see [`neg_lon`](SkyFrame::neg_lon).
    pub fn clear_neg_lon(&mut self) {
        self.neglon = false;
        self.neglon_set = false;
    }

    /// Both values of SkyRef, as `[longitude, latitude]` in radians,
    /// whatever the axis order; see [`sky_ref`](SkyFrame::sky_ref).
    pub fn sky_ref_lon_lat(&self) -> [f64; 2] {
        self.sky_ref
    }

    /// Set both values of SkyRef, longitude then latitude, in radians,
    /// whatever the axis order; see [`sky_ref`](SkyFrame::sky_ref).
    pub fn set_sky_ref_lon_lat(&mut self, lon: f64, lat: f64) {
        self.sky_ref = [lon, lat];
        self.sky_ref_set = [true, true];
    }

    /// The position defining the offset coordinate system: the
    /// `SkyRef(axis)` attribute of the 1-based `axis`, in radians, set or
    /// default.
    ///
    /// Lets a SkyFrame represent offsets, rather than absolute axis values,
    /// within the coordinate system given by its
    /// [`system`](SkyFrame::system).
    /// SkyRef holds the longitude and latitude of a point in that system,
    /// one value per axis ([`lon_axis`](SkyFrame::lon_axis) and
    /// [`lat_axis`](SkyFrame::lat_axis) say which is which;
    /// [`sky_ref_lon_lat`](SkyFrame::sky_ref_lon_lat) gives both in
    /// longitude, latitude order).
    /// The coordinate system the SkyFrame represents is rotated to put that
    /// position at the origin or the pole of the new system, as
    /// [`sky_ref_is`](SkyFrame::sky_ref_is) says, and
    /// [`sky_ref_p`](SkyFrame::sky_ref_p) controls the orientation of the
    /// result.
    ///
    /// When a SkyRef is set and SkyRefIs is not
    /// [`SkyRefIs::Ignored`], the defaults of other attributes change: each
    /// axis [`label`](SkyFrame::label) gains `" offset"` at the end, each
    /// axis [`symbol`](SkyFrame::symbol) gains a `"D"` at the start, and the
    /// default [`title`](SkyFrame::title) describes the origin or pole in
    /// place of the projection.
    /// A set SkyRef is re-expressed in the new System when the System
    /// changes, so it stays at the same place on the sky.
    ///
    /// Default: zero longitude and zero latitude.
    ///
    /// # Aligning SkyFrames with offset coordinate systems
    ///
    /// The offset coordinate system should normally be considered a
    /// superficial "re-badging" of the axes of the coordinate system given by
    /// the System: it merely provides an alternative numerical label for each
    /// position.
    /// The SkyFrame retains full knowledge of the celestial coordinate
    /// system on which the offset system is based, such as the way one
    /// celestial coordinate system drifts with respect to another over time.
    /// Normally, when two SkyFrames are aligned (by
    /// [`convert_to`](crate::frames::FrameSearch::convert_to) or
    /// [`find_frame_template`](crate::frames::FrameSearch::find_frame_template)), the effect of
    /// any offset coordinate system in either is removed, and alignment
    /// takes place in the system given by
    /// [`align_system`](SkyFrame::align_system).
    /// Setting [`align_offset`](SkyFrame::align_offset) true in both changes
    /// this, so that the effect of the offset coordinate systems is kept.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    ///
    /// ```
    /// use starlink_ast::frames::{Frame, SkyFrame, SkyRefIs, SkySystem};
    ///
    /// let mut sky = SkyFrame::new(SkySystem::Fk5);
    /// sky.set_sky_ref(1, 1.0).unwrap();
    /// sky.set_sky_ref(2, 0.5).unwrap();
    /// sky.set_sky_ref_is(SkyRefIs::Origin);
    /// assert_eq!(sky.label(1).unwrap(), "Right ascension offset");
    /// assert_eq!(sky.symbol(2).unwrap(), "DDec");
    /// ```
    // C: `astGetSkyRef` (skyframe.c `MAKE_GET`).
    #[doc(alias = "SkyRef")]
    pub fn sky_ref(&self, axis: usize) -> AstResult<f64> {
        let phys = self.physical_axis(axis)?;
        Ok(if self.sky_ref_set[phys] {
            self.sky_ref[phys]
        } else {
            0.0
        })
    }

    /// Set `SkyRef(axis)`, in radians; see [`sky_ref`](SkyFrame::sky_ref).
    pub fn set_sky_ref(&mut self, axis: usize, value: f64) -> AstResult<()> {
        let phys = self.physical_axis(axis)?;
        self.sky_ref[phys] = value;
        self.sky_ref_set[phys] = true;
        Ok(())
    }

    /// Whether `SkyRef(axis)` is set; see [`sky_ref`](SkyFrame::sky_ref).
    pub fn test_sky_ref(&self, axis: usize) -> AstResult<bool> {
        Ok(self.sky_ref_set[self.physical_axis(axis)?])
    }

    /// Clear `SkyRef(axis)`; see [`sky_ref`](SkyFrame::sky_ref).
    pub fn clear_sky_ref(&mut self, axis: usize) -> AstResult<()> {
        let phys = self.physical_axis(axis)?;
        self.sky_ref[phys] = 0.0;
        self.sky_ref_set[phys] = false;
        Ok(())
    }

    /// Whether either axis of SkyRef is set (internal order).
    fn any_sky_ref_set(&self) -> bool {
        self.sky_ref_set[0] || self.sky_ref_set[1]
    }

    /// The nature of the offset coordinate system: the SkyRefIs attribute,
    /// set or default.
    ///
    /// Controls how the [`sky_ref`](SkyFrame::sky_ref) and
    /// [`sky_ref_p`](SkyFrame::sky_ref_p) values are used.
    /// The three together let a SkyFrame represent offsets relative to some
    /// origin or pole within the coordinate system given by its
    /// [`system`](SkyFrame::system), rather than absolute axis values.
    ///
    /// - [`SkyRefIs::Origin`]: the coordinate system is modified to put the
    ///   origin of longitude and latitude at the SkyRef position.
    /// - [`SkyRefIs::Pole`]: the coordinate system is modified to put the
    ///   north pole at the SkyRef position.
    /// - [`SkyRefIs::Ignored`]: any SkyRef is ignored, and the SkyFrame
    ///   represents the System directly, without rotation.
    ///
    /// Default: [`SkyRefIs::Ignored`].
    #[doc(alias = "SkyRefIs")]
    pub fn sky_ref_is(&self) -> SkyRefIs {
        self.sky_ref_is
    }

    /// Set SkyRefIs; see [`sky_ref_is`](SkyFrame::sky_ref_is).
    pub fn set_sky_ref_is(&mut self, mode: SkyRefIs) {
        self.sky_ref_is = mode;
        self.sky_ref_is_set = true;
    }

    /// Whether SkyRefIs is set; see [`sky_ref_is`](SkyFrame::sky_ref_is).
    pub fn test_sky_ref_is(&self) -> bool {
        self.sky_ref_is_set
    }

    /// Clear SkyRefIs; see [`sky_ref_is`](SkyFrame::sky_ref_is).
    pub fn clear_sky_ref_is(&mut self) {
        self.sky_ref_is = SkyRefIs::Ignored;
        self.sky_ref_is_set = false;
    }

    /// The Mapping from absolute coordinates in this SkyFrame's
    /// [`system`](SkyFrame::system) to the offset coordinates that its
    /// [`sky_ref`](SkyFrame::sky_ref), [`sky_ref_p`](SkyFrame::sky_ref_p) and
    /// [`sky_ref_is`](SkyFrame::sky_ref_is) define.
    ///
    /// Its inverse goes from offset coordinates back to absolute ones.
    /// A 2-axis UnitMap is returned when the SkyFrame defines no offset
    /// coordinate system: when SkyRefIs is [`SkyRefIs::Ignored`] or neither
    /// axis of SkyRef is set.
    /// The Mapping takes and gives (longitude, latitude) pairs in radians,
    /// in that order whatever the axis order of the SkyFrame.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Badoc` when the SkyRefP position coincides with, or is
    ///   opposite to, the SkyRef position.
    ///
    /// # Examples
    ///
    /// ```
    /// use starlink_ast::coords::PointSet;
    /// use starlink_ast::frames::{SkyFrame, SkyRefIs, SkySystem};
    ///
    /// let mut sky = SkyFrame::new(SkySystem::Fk5);
    /// sky.set_sky_ref(1, 1.0).unwrap();
    /// sky.set_sky_ref(2, 0.5).unwrap();
    /// sky.set_sky_ref_is(SkyRefIs::Origin);
    /// let map = sky.sky_offset_map().unwrap();
    /// // The SkyRef position is the origin of the offset coordinates.
    /// let out = map.transform(&PointSet::from_rows([[1.0, 0.5]]), true).unwrap();
    /// assert!(out.point(0)[0].abs() < 1e-12 && out.point(0)[1].abs() < 1e-12);
    /// ```
    // C: skyframe.c `SkyOffsetMap`.
    #[doc(alias = "astSkyOffsetMap")]
    pub fn sky_offset_map(&self) -> AstResult<DynMapping> {
        crate::frames::sky_convert::sky_offset_map(
            self.sky_ref,
            self.sky_ref_p_lon_lat(),
            self.sky_ref_is,
            self.any_sky_ref_set(),
        )
    }

    /// Both values of SkyRefP, set or default, as `[longitude, latitude]`
    /// in radians, whatever the axis order; see
    /// [`sky_ref_p`](SkyFrame::sky_ref_p).
    pub fn sky_ref_p_lon_lat(&self) -> [f64; 2] {
        [self.phys_sky_ref_p(0), self.phys_sky_ref_p(1)]
    }

    /// The position on the primary meridian of the offset coordinate
    /// system: the `SkyRefP(axis)` attribute of the 1-based `axis`, in
    /// radians, set or default.
    ///
    /// Controls the orientation of the offset coordinate system defined by
    /// [`sky_ref`](SkyFrame::sky_ref) and
    /// [`sky_ref_is`](SkyFrame::sky_ref_is).
    /// SkyRefP holds the longitude and latitude of a point in the
    /// coordinate system given by the [`system`](SkyFrame::system), one value
    /// per axis.
    /// The offset coordinate system is rotated, about the axis from the
    /// center of the celestial sphere to the SkyRef position, to put the
    /// SkyRefP position on the zero longitude meridian.
    ///
    /// When SkyRef is the origin of the offset system, two orientations
    /// put SkyRefP on the zero longitude meridian; the one used gives
    /// SkyRefP a positive latitude in the offset system.
    /// A SkyRefP which coincides with SkyRef, or is diametrically opposite
    /// it, cannot define an orientation; the error is reported only when the
    /// offset coordinate system is used, as `ErrorCode::Badoc`.
    /// A set SkyRefP is re-expressed in the new System when the System
    /// changes.
    ///
    /// Default: the north pole (latitude +pi/2 in the System), unless
    /// SkyRef is itself at the north or south pole, when it is the origin
    /// (0, 0).
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not 1 or 2.
    // C: skyframe.c:4089 `GetSkyRefP`.
    #[doc(alias = "SkyRefP")]
    pub fn sky_ref_p(&self, axis: usize) -> AstResult<f64> {
        Ok(self.phys_sky_ref_p(self.physical_axis(axis)?))
    }

    /// SkyRefP of the internal axis `phys`, set or default. The default is
    /// the north pole, `(0, pi/2)`, unless SkyRef is at a pole, when it is
    /// the origin (skyframe.c:4089). C tests the latitude it stores for
    /// SkyRef, which is `AST__BAD` while SkyRef is unset.
    fn phys_sky_ref_p(&self, phys: usize) -> f64 {
        if self.sky_ref_p_set[phys] {
            return self.sky_ref_p[phys];
        }
        let ref_lat = if self.sky_ref_set[1] {
            self.sky_ref[1]
        } else {
            BAD_VALUE
        };
        if phys == 1 && crate::math::cos(ref_lat).abs() > 1.0e-10 {
            std::f64::consts::FRAC_PI_2
        } else {
            0.0
        }
    }

    /// Set both values of SkyRefP, longitude then latitude, in radians,
    /// whatever the axis order; see [`sky_ref_p`](SkyFrame::sky_ref_p).
    pub fn set_sky_ref_p_lon_lat(&mut self, lon: f64, lat: f64) {
        self.sky_ref_p = [lon, lat];
        self.sky_ref_p_set = [true, true];
    }

    /// Set `SkyRefP(axis)`, in radians; see
    /// [`sky_ref_p`](SkyFrame::sky_ref_p).
    pub fn set_sky_ref_p(&mut self, axis: usize, value: f64) -> AstResult<()> {
        let phys = self.physical_axis(axis)?;
        self.sky_ref_p[phys] = value;
        self.sky_ref_p_set[phys] = true;
        Ok(())
    }

    /// Whether `SkyRefP(axis)` is set; see
    /// [`sky_ref_p`](SkyFrame::sky_ref_p).
    pub fn test_sky_ref_p(&self, axis: usize) -> AstResult<bool> {
        Ok(self.sky_ref_p_set[self.physical_axis(axis)?])
    }

    /// Clear `SkyRefP(axis)`; see [`sky_ref_p`](SkyFrame::sky_ref_p).
    pub fn clear_sky_ref_p(&mut self, axis: usize) -> AstResult<()> {
        let phys = self.physical_axis(axis)?;
        self.sky_ref_p[phys] = 0.0;
        self.sky_ref_p_set[phys] = false;
        Ok(())
    }

    /// The smallest significant shift in sky coordinates, in arc-seconds:
    /// the SkyTol attribute, set or default.
    ///
    /// Indicates the accuracy of the axis values the SkyFrame represents.
    /// It is used only when the Mapping between two different SkyFrames is
    /// constructed (by [`convert_to`](crate::frames::FrameSearch::convert_to) or
    /// [`find_frame_template`](crate::frames::FrameSearch::find_frame_template), for instance):
    /// if the transformation between them shifts positions by less than
    /// SkyTol, it is replaced by a [`UnitMap`].
    /// This can allow major simplifications of the transformation between
    /// any pixel grids associated with the two SkyFrames, such as when each
    /// is part of the WCS FrameSet of an image.
    /// The smaller SkyTol of the two SkyFrames is used.
    ///
    /// A common case is two FK5 SkyFrames with slightly different Epochs.
    /// With the default AlignSystem, ICRS, the transformation between them
    /// includes a very small rotation, since FK5 rotates with respect to
    /// ICRS at about 0.0005 arc-seconds per year.
    /// Such a rotation is usually insignificant, and a suitably small
    /// non-zero SkyTol lets it be ignored, so that much simpler
    /// transformations can be used.
    ///
    /// The shift is found by transforming 14 positions spread evenly over
    /// the whole sky and taking the largest.
    ///
    /// Default: 0.001.
    /// The absolute value of a negative value is stored.
    #[doc(alias = "SkyTol")]
    pub fn sky_tol(&self) -> f64 {
        // C default: 0.001 arcsec (skyframe.c:11549).
        self.sky_tol.unwrap_or(0.001)
    }

    /// Set SkyTol, in arc-seconds; see [`sky_tol`](SkyFrame::sky_tol).
    pub fn set_sky_tol(&mut self, tol: f64) {
        // skyframe.c:11658 stores the absolute value.
        self.sky_tol = Some(tol.abs());
    }

    /// Whether SkyTol is set; see [`sky_tol`](SkyFrame::sky_tol).
    pub fn test_sky_tol(&self) -> bool {
        self.sky_tol.is_some()
    }

    /// Clear SkyTol; see [`sky_tol`](SkyFrame::sky_tol).
    pub fn clear_sky_tol(&mut self) {
        self.sky_tol = None;
    }

    /// Whether the Equinox is set; see
    /// [`equinox_mjd`](SkyFrame::equinox_mjd).
    pub fn test_equinox(&self) -> bool {
        self.equinox.is_some()
    }

    /// Align SkyFrames using the offset coordinate system? The AlignOffset
    /// attribute, set or default.
    ///
    /// Controls how a SkyFrame behaves when it is used as a template to
    /// match another (target) SkyFrame, by
    /// [`find_frame_template`](crate::frames::FrameSearch::find_frame_template) or
    /// [`convert_to`](crate::frames::FrameSearch::convert_to): it decides the coordinate system
    /// in which the two are aligned if they match.
    /// If both have an offset coordinate system (a
    /// [`sky_ref_is`](SkyFrame::sky_ref_is) of [`SkyRefIs::Origin`] or
    /// [`SkyRefIs::Pole`]) and both have AlignOffset true, alignment takes
    /// place within the offset coordinate systems, so the two are always
    /// connected by a [`UnitMap`].
    /// If either has AlignOffset false, or SkyRefIs
    /// [`SkyRefIs::Ignored`], alignment takes place in the coordinate system
    /// given by [`align_system`](SkyFrame::align_system).
    ///
    /// Default: false.
    #[doc(alias = "AlignOffset")]
    pub fn align_offset(&self) -> bool {
        self.align_offset
    }

    /// Set AlignOffset; see [`align_offset`](SkyFrame::align_offset).
    pub fn set_align_offset(&mut self, ao: bool) {
        self.align_offset = ao;
        self.align_offset_set = true;
    }

    /// Whether AlignOffset is set; see
    /// [`align_offset`](SkyFrame::align_offset).
    pub fn test_align_offset(&self) -> bool {
        self.align_offset_set
    }

    /// Clear AlignOffset; see [`align_offset`](SkyFrame::align_offset).
    pub fn clear_align_offset(&mut self) {
        self.align_offset = false;
        self.align_offset_set = false;
    }

    /// The sky projection description: the Projection attribute, or `None`
    /// when it is not set.
    ///
    /// A place to describe the type of sky projection used when the
    /// SkyFrame is attached to a 2-dimensional object, such as an image or
    /// plotting surface: `"orthographic"`, `"Hammer-Aitoff"` or
    /// `"cylindrical equal area"`, for instance.
    /// It is purely descriptive and does not affect the celestial coordinate
    /// system the SkyFrame represents.
    /// A non-blank value is used in the default [`title`](SkyFrame::title),
    /// so that it typically appears in graphical output.
    ///
    /// Default: `None`.
    #[doc(alias = "Projection")]
    pub fn projection(&self) -> Option<&str> {
        self.projection.as_deref()
    }

    /// Set the Projection; see [`projection`](SkyFrame::projection).
    pub fn set_projection(&mut self, proj: &str) {
        self.projection = Some(proj.to_string());
    }

    /// Whether the Projection is set; see
    /// [`projection`](SkyFrame::projection).
    pub fn test_projection(&self) -> bool {
        self.projection.is_some()
    }

    /// Clear the Projection; see [`projection`](SkyFrame::projection).
    pub fn clear_projection(&mut self) {
        self.projection = None;
    }

    /// Whether the Title is set; see [`title`](SkyFrame::title).
    pub fn test_title(&self) -> bool {
        self.attrs.title.is_some()
    }

    /// Clear the Title; see [`title`](SkyFrame::title).
    pub fn clear_title(&mut self) {
        self.attrs.title = None;
    }

    // --- Frame-level attributes ---

    /// The Frame title: the Title attribute, set or default; see
    /// [`Frame::title`].
    ///
    /// Default: a description of the celestial coordinate system, with its
    /// Equinox and Epoch where they matter, followed by the
    /// [`projection`](SkyFrame::projection) if one is set; for an offset
    /// coordinate system "coordinates" becomes "offsets" and the origin or
    /// pole is given in place of the projection, as in
    /// `"FK5 equatorial offsets; mean equinox J2000; origin at ..."`.
    /// It never fails for a SkyFrame.
    // C: skyframe.c `GetTitle`.
    pub fn title(&self) -> AstResult<String> {
        Frame::title(self)
    }

    /// Set the Title; see [`title`](SkyFrame::title).
    pub fn set_title(&mut self, title: &str) {
        self.attrs.title = Some(title.to_string());
    }

    /// The coordinate system domain: the Domain attribute, set or default;
    /// see [`Frame::domain`].
    ///
    /// Default: `"SKY"`.
    pub fn domain(&self) -> &str {
        self.attrs.domain.as_deref().unwrap_or("SKY")
    }

    /// Set the Domain, removing white space and upper-casing letters; see
    /// [`domain`](SkyFrame::domain).
    pub fn set_domain(&mut self, domain: &str) {
        self.attrs.domain = Some(super::frame_attrs::clean_domain(domain));
    }

    /// Whether the Domain is set; see [`domain`](SkyFrame::domain).
    pub fn test_domain(&self) -> bool {
        self.attrs.domain.is_some()
    }

    /// Clear the Domain; see [`domain`](SkyFrame::domain).
    pub fn clear_domain(&mut self) {
        self.attrs.domain = None;
    }

    /// The Ident, or an empty string when it is not set; see
    /// [`Frame::ident`].
    pub fn ident(&self) -> &str {
        self.attrs.ident.as_deref().unwrap_or("")
    }

    /// Set the frame identification string.
    pub fn set_ident(&mut self, ident: impl Into<String>) {
        let s = ident.into();
        self.attrs.ident = Some(s);
    }

    // --- Observer attributes ---
    //
    // A SkyFrame caches its LAST and diurnal aberration, so each setter
    // and clearer discards them when the value changes by more than C's
    // threshold (skyframe.c SetObsLat, ClearObsLat and the rest). The
    // Epoch does not discard them: GetLAST extrapolates from the cached
    // LAST instead.

    /// Discard the cached LAST, and the diurnal aberration too if
    /// `diurab`.
    fn discard_last(&self, diurab: bool) {
        self.last.set(BAD_VALUE);
        self.eplast.set(BAD_VALUE);
        self.klast.set(BAD_VALUE);
        if diurab {
            self.diurab.set(BAD_VALUE);
        }
    }

    /// Set the observer geodetic latitude in radians.
    pub fn set_obs_lat(&mut self, lat: f64) {
        let orig = Frame::obs_lat(self);
        self.attrs.obs_lat = Some(lat);
        if (orig - lat).abs() > 1.0e-8 {
            self.discard_last(true);
        }
    }

    /// Set the observer geodetic longitude in radians.
    pub fn set_obs_lon(&mut self, lon: f64) {
        let orig = Frame::obs_lon(self);
        self.attrs.obs_lon = Some(lon);
        if (orig - lon).abs() > 1.0e-8 {
            self.discard_last(false);
        }
    }

    /// Set the observer altitude in meters.
    pub fn set_obs_alt(&mut self, alt: f64) {
        let orig = Frame::obs_alt(self);
        self.attrs.obs_alt = Some(alt);
        if (orig - alt).abs() > 0.001 {
            self.discard_last(true);
        }
    }

    /// Set TAI-UTC in seconds.
    pub fn set_dtai(&mut self, dtai: f64) {
        let orig = Frame::dtai(self);
        self.attrs.dtai = Some(dtai);
        if !super::sky_last::equal(orig, dtai, 1.0e-6) {
            self.discard_last(false);
        }
    }

    /// Set UT1-UTC in seconds.
    pub fn set_dut1(&mut self, dut1: f64) {
        let orig = Frame::dut1(self);
        self.attrs.dut1 = Some(dut1);
        if (orig - dut1).abs() > 1.0e-6 {
            self.discard_last(false);
        }
    }

    fn clear_obs_lat_value(&mut self) {
        let orig = Frame::obs_lat(self);
        self.attrs.obs_lat = None;
        if (orig - Frame::obs_lat(self)).abs() > 1.0e-8 {
            self.discard_last(true);
        }
    }

    fn clear_obs_lon_value(&mut self) {
        let orig = Frame::obs_lon(self);
        self.attrs.obs_lon = None;
        if (orig - Frame::obs_lon(self)).abs() > 1.0e-8 {
            self.discard_last(false);
        }
    }

    fn clear_obs_alt_value(&mut self) {
        let orig = Frame::obs_alt(self);
        self.attrs.obs_alt = None;
        if (orig - Frame::obs_alt(self)).abs() > 0.001 {
            self.discard_last(true);
        }
    }

    fn clear_dtai_value(&mut self) {
        let orig = Frame::dtai(self);
        self.attrs.dtai = None;
        if !super::sky_last::equal(orig, Frame::dtai(self), 1.0e-6) {
            self.discard_last(false);
        }
    }

    fn clear_dut1_value(&mut self) {
        let orig = Frame::dut1(self);
        self.attrs.dut1 = None;
        if (orig - Frame::dut1(self)).abs() > 1.0e-6 {
            self.discard_last(false);
        }
    }

    /// Local Apparent Sidereal Time, in radians, at this SkyFrame's Epoch
    /// and observer (skyframe.c `GetLAST`).
    ///
    /// The SkyFrame keeps the LAST it last computed exactly. For an Epoch
    /// within 0.4 day of that one, the LAST is extrapolated linearly using
    /// the ratio of solar to sidereal time, itself found from an exact LAST
    /// 0.4 day later; C puts the error at under 0.1 arcsec. Otherwise the
    /// LAST is computed afresh through `sky_last::calc_last`, which may
    /// serve it from the table of LAST values already computed.
    pub(crate) fn get_last(&self) -> AstResult<f64> {
        let eplast = self.eplast.get();
        if eplast != BAD_VALUE {
            let delta = self.epoch_mjd() - eplast;
            if delta == 0.0 {
                return Ok(self.last.get());
            }
            if delta.abs() < 0.4 {
                if self.klast.get() == BAD_VALUE {
                    let last1 = self.calc_last(eplast + 0.4)?;
                    let mut dlast = last1 - self.last.get();
                    if dlast < 0.0 {
                        dlast += 2.0 * std::f64::consts::PI;
                    }
                    self.klast.set(2.0 * std::f64::consts::PI * 0.4 / dlast);
                }
                return Ok(self.last.get() + 2.0 * std::f64::consts::PI * delta / self.klast.get());
            }
        }
        self.set_last()?;
        Ok(self.last.get())
    }

    /// Compute the LAST at the Epoch and keep it (skyframe.c `SetLast`).
    fn set_last(&self) -> AstResult<()> {
        let epoch = self.epoch_mjd();
        self.last.set(self.calc_last(epoch)?);
        self.eplast.set(epoch);
        self.klast.set(BAD_VALUE);
        Ok(())
    }

    /// The LAST at TDB MJD `epoch` for this SkyFrame's observer.
    fn calc_last(&self, epoch: f64) -> AstResult<f64> {
        super::sky_last::calc_last(
            epoch,
            Frame::obs_lon(self),
            Frame::obs_lat(self),
            Frame::obs_alt(self),
            Frame::dut1(self),
            Frame::dtai(self),
        )
    }

    /// Diurnal aberration vector magnitude, in radians, at this
    /// SkyFrame's observer location, kept until the latitude or altitude
    /// changes (skyframe.c `GetDiurab`).
    ///
    /// `diurab = 2π · uau · SOLSID / C` with `uau` from
    /// `palGeoc(obs_lat, obs_alt)`, with C's constants `SOLSID =
    /// 1.00273790935` (sidereal/solar ratio) and `C = 173.14463331` (speed
    /// of light in AU/day).
    pub(crate) fn get_diurab(&self) -> f64 {
        const SOLSID: f64 = 1.00273790935;
        const C: f64 = 173.14463331;
        if self.diurab.get() == BAD_VALUE {
            let (uau, _vau) = pal::geoc(Frame::obs_lat(self), Frame::obs_alt(self));
            self.diurab
                .set(2.0 * std::f64::consts::PI * uau * SOLSID / C);
        }
        self.diurab.get()
    }

    // --- Private helpers for serialization defaults ---

    /// Default title for the current system and equinox.
    /// Is an offset coordinate system in use? Mirrors C `GetTitle`/`GetLabel`
    /// (skyframe.c): SkyRef set on either axis and SkyRefIs != IGNORED.
    fn is_offset(&self) -> bool {
        self.any_sky_ref_set() && self.sky_ref_is != SkyRefIs::Ignored
    }

    fn default_title(&self) -> String {
        // Mirrors C `SkyFrame::GetTitle` (skyframe.c:4404). "coordinates" vs
        // "offsets" depending on whether an offset system is in use.
        let offset = self.is_offset();
        let word = if offset { "offsets" } else { "coordinates" };
        // With UseDefs false, an Equinox or Epoch that has not been set is
        // left out (skyframe.c:4598).
        let show_equinox = self.test_equinox() || self.use_defs();
        let show_epoch = self.test_epoch() || self.use_defs();
        let mut base = match self.system {
            SkySystem::Fk5 => {
                let mut t = format!("FK5 equatorial {word}");
                if show_equinox {
                    let eq = fmt_decimal_yr(pal::epj(self.equinox_mjd()));
                    t.push_str(&format!("; mean equinox J{eq}"));
                }
                t
            }
            SkySystem::Fk4 | SkySystem::Fk4NoE => {
                let mut t = if matches!(self.system, SkySystem::Fk4NoE) {
                    format!("FK4 equatorial {word}; no E-terms")
                } else {
                    format!("FK4 equatorial {word}")
                };
                if show_equinox {
                    let eq = fmt_decimal_yr(pal::epb(self.equinox_mjd()));
                    t.push_str(&format!("; mean equinox B{eq}"));
                }
                if show_epoch {
                    let ep = fmt_decimal_yr(pal::epb(self.epoch_mjd()));
                    t.push_str(&format!("; epoch B{ep}"));
                }
                t
            }
            SkySystem::J2000 => format!("J2000 equatorial {word}"),
            SkySystem::Icrs => format!("ICRS {word}"),
            SkySystem::Azel => format!("Horizon (Azimuth/Elevation) {word}"),
            SkySystem::Gappt => {
                // The doubled "; ; epoch" is a verbatim quirk of C's GetTitle
                // (skyframe.c:4568, the format string contains "; ; epoch").
                let ep = fmt_decimal_yr(pal::epj(self.epoch_mjd()));
                format!("Geocentric apparent equatorial {word}; ; epoch J{ep}")
            }
            SkySystem::Ecliptic => {
                let mut t = format!("Ecliptic {word}");
                if show_equinox {
                    let eq = fmt_decimal_yr(pal::epj(self.equinox_mjd()));
                    t.push_str(&format!("; mean equinox J{eq}"));
                }
                t
            }
            SkySystem::Helioecliptic => {
                let mut t = format!("Helio-ecliptic {word}; mean equinox J2000");
                if show_epoch {
                    let ep = fmt_decimal_yr(pal::epj(self.epoch_mjd()));
                    t.push_str(&format!("; epoch J{ep}"));
                }
                t
            }
            SkySystem::Galactic => format!("IAU (1958) galactic {word}"),
            SkySystem::Supergalactic => format!("De Vaucouleurs supergalactic {word}"),
            SkySystem::Unknown => format!("Spherical {word}"),
        };

        // Append either the offset origin/pole description or the projection,
        // mirroring C (skyframe.c:4629): only one is included.
        if offset {
            let word = if self.sky_ref_is == SkyRefIs::Pole {
                "pole"
            } else {
                "origin"
            };
            // C formats external axes 0 and 1 in order via astFormat with
            // their astGetSkyRef values. sky_ref is stored in internal order,
            // so external axis e (0-based) reads sky_ref at the internal slot
            // it maps to (invperm), and is formatted with that external axis —
            // identical to how the SRef cards' comments are produced below.
            let invperm = [self.lon_pos(), self.lat_pos()];
            let internal_of_ext =
                |ext0: usize| invperm.iter().position(|&e| e == ext0).unwrap_or(ext0);
            let v0 = self
                .format(1, self.sky_ref[internal_of_ext(0)])
                .unwrap_or_default();
            let v1 = self
                .format(2, self.sky_ref[internal_of_ext(1)])
                .unwrap_or_default();
            base.push_str(&format!("; {word} at {v0} {v1}"));
        } else if let Some(ref proj) = self.projection {
            let proj = proj.trim_end();
            if !proj.is_empty() {
                base.push_str(&format!("; {proj} projection"));
            }
        }
        base
    }

    /// Default axis labels for the current system. Mirrors C
    /// `SkyFrame::GetLabel` (skyframe.c:3564): the per-system longitude /
    /// latitude names, with " offset" appended on each when an offset
    /// coordinate system is in use.
    fn default_labels(&self) -> (String, String) {
        let (lon, lat) = match self.system {
            SkySystem::Galactic => ("Galactic longitude", "Galactic latitude"),
            SkySystem::Ecliptic => ("Ecliptic longitude", "Ecliptic latitude"),
            SkySystem::Helioecliptic => ("Helio-ecliptic longitude", "Helio-ecliptic latitude"),
            SkySystem::Supergalactic => ("Supergalactic longitude", "Supergalactic latitude"),
            SkySystem::Azel => ("Azimuth", "Elevation"),
            SkySystem::Unknown => ("Longitude", "Latitude"),
            // Equatorial systems (FK4/FK4-no-E/FK5/J2000/ICRS/GAPPT).
            _ => ("Right ascension", "Declination"),
        };
        if self.is_offset() {
            (format!("{lon} offset"), format!("{lat} offset"))
        } else {
            (lon.to_string(), lat.to_string())
        }
    }

    /// The Digits value for the physical axis `phys`: the axis's own if set,
    /// otherwise the SkyFrame's. Mirrors how C's Frame methods give an Axis
    /// with no Digits of its own the Frame's value while they consult it
    /// (frame.c:4295 in `Format`, and `MAKE_GET`).
    fn phys_digits(&self, phys: usize) -> i32 {
        self.axes[phys]
            .axis()
            .digits
            .unwrap_or_else(|| self.attrs.digits.unwrap_or(7))
    }

    /// The Format for the physical axis `phys`. Mirrors C
    /// `SkyFrame::GetFormat` (skyframe.c:3394): a Format that has been set,
    /// or for an equatorial System the SkyAxis default (`GetAxisFormat`,
    /// skyaxis.c:2888), or for any other System decimal degrees (or decimal
    /// hours, when AsTime is set).
    fn phys_format(&self, phys: usize) -> String {
        // The Frame's method answers for a plain Axis, whose Format syntax
        // is not the SkyAxis one (skyframe.c:3475).
        let sky = match &self.axes[phys] {
            FrameAxis::Sky(sky) => sky,
            plain @ FrameAxis::Plain(_) => {
                return plain.format_value(self.attrs.digits.unwrap_or(7));
            }
        };
        if let Some(fmt) = &sky.format {
            return fmt.clone();
        }
        let digits = self.phys_digits(phys);
        let as_time = self.effective_as_time(phys);
        if is_equatorial(self.system) {
            return default_dhms_format(digits, as_time);
        }
        match (as_time, digits) {
            (true, d) if d <= 2 => "h".to_string(),
            (true, d) => format!("h.{}", d - 2),
            (false, d) if d <= 3 => "d".to_string(),
            (false, d) => format!("d.{}", d - 3),
        }
    }

    /// The Unit for the physical axis `phys`. Mirrors C `SkyFrame::GetUnit`
    /// (skyframe.c:4778) and `SkyAxis::GetAxisUnit` (skyaxis.c:3288): a Unit
    /// that has been set, `"rad"` for a `%`-style Format, and otherwise the
    /// description or unit the Format implies (`DHmsUnit`).
    fn phys_unit(&self, phys: usize) -> String {
        if let Some(unit) = &self.axes[phys].axis().unit {
            return unit.clone();
        }
        // A plain Axis has no Unit unless one is set (axis.c GetAxisUnit).
        if let FrameAxis::Plain(_) = &self.axes[phys] {
            return String::new();
        }
        let fmt = self.phys_format(phys);
        if fmt.starts_with('%') {
            "rad".to_string()
        } else {
            dhms_unit(&fmt, self.phys_digits(phys)).0
        }
    }
}

impl Frame for SkyFrame {
    fn axis_perm(&self) -> Vec<usize> {
        self.perm.to_vec()
    }

    frame_attrs_title!();
    frame_attrs_domain!();
    frame_attrs_epoch!();
    frame_attrs_digits!();
    // The observer attributes, whose setters discard the cached LAST and
    // diurnal aberration (skyframe.c overrides them for that).
    fn obs_lat(&self) -> f64 {
        self.attrs.obs_lat.unwrap_or(0.0)
    }
    fn set_obs_lat(&mut self, value: f64) -> AstResult<()> {
        SkyFrame::set_obs_lat(self, value);
        Ok(())
    }
    fn test_obs_lat(&self) -> bool {
        self.attrs.obs_lat.is_some()
    }
    fn clear_obs_lat(&mut self) -> AstResult<()> {
        self.clear_obs_lat_value();
        Ok(())
    }
    fn obs_lon(&self) -> f64 {
        self.attrs.obs_lon.unwrap_or(0.0)
    }
    fn set_obs_lon(&mut self, value: f64) -> AstResult<()> {
        SkyFrame::set_obs_lon(self, value);
        Ok(())
    }
    fn test_obs_lon(&self) -> bool {
        self.attrs.obs_lon.is_some()
    }
    fn clear_obs_lon(&mut self) -> AstResult<()> {
        self.clear_obs_lon_value();
        Ok(())
    }
    fn obs_alt(&self) -> f64 {
        self.attrs.obs_alt.unwrap_or(0.0)
    }
    fn set_obs_alt(&mut self, value: f64) -> AstResult<()> {
        SkyFrame::set_obs_alt(self, value);
        Ok(())
    }
    fn test_obs_alt(&self) -> bool {
        self.attrs.obs_alt.is_some()
    }
    fn clear_obs_alt(&mut self) -> AstResult<()> {
        self.clear_obs_alt_value();
        Ok(())
    }
    fn dtai(&self) -> f64 {
        self.attrs.dtai.unwrap_or(BAD_VALUE)
    }
    fn set_dtai(&mut self, value: f64) -> AstResult<()> {
        SkyFrame::set_dtai(self, value);
        Ok(())
    }
    fn test_dtai(&self) -> bool {
        self.attrs.dtai.is_some()
    }
    fn clear_dtai(&mut self) -> AstResult<()> {
        self.clear_dtai_value();
        Ok(())
    }
    fn dut1(&self) -> f64 {
        self.attrs.dut1.unwrap_or(0.0)
    }
    fn set_dut1(&mut self, value: f64) -> AstResult<()> {
        SkyFrame::set_dut1(self, value);
        Ok(())
    }
    fn test_dut1(&self) -> bool {
        self.attrs.dut1.is_some()
    }
    fn clear_dut1(&mut self) -> AstResult<()> {
        self.clear_dut1_value();
        Ok(())
    }
    frame_attrs_ident!();
    frame_attrs_match!();

    fn naxes(&self) -> usize {
        2
    }

    /// Rather than matching each axis's primary Frame, the whole SkyFrame is
    /// used as the target.
    /// The two template axes the match associates with it are reported, so a
    /// permuted SkyFrame reports its axes in its own order.
    // C: `SkyFrame::MatchAxesX` (skyframe.c:6977).
    fn match_axes_x(&self, frm1: &dyn Frame) -> AstResult<Vec<i32>> {
        let mut probe = frm1.snapshot();
        probe.set_preserve_axes(true)?;
        probe.set_max_axes(10000)?;
        probe.set_min_axes(1)?;
        Ok(
            match super::find_frame::match_with_subclass(probe.as_ref(), self, true)? {
                Some(m) => vec![m.template_axes[0] + 1, m.template_axes[1] + 1],
                None => vec![0, 0],
            },
        )
    }

    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        // Apply the SkyFrame's external->internal axis permutation so
        // the returned axis is the underlying physical axis
        // (0 = lon, 1 = lat).
        (self, self.axis_permutation()[axis])
    }

    /// Display an axis in the conventional direction? See
    /// [`Frame::direction`].
    ///
    /// The default is false for the longitude axis (right ascension, for
    /// instance), so that it is plotted in reverse, as every celestial
    /// system is left-handed, and true for the latitude axis.
    // C: skyframe.c:2836 `GetDirection`.
    fn direction(&self, axis: usize) -> AstResult<bool> {
        let phys = self.physical_axis(axis)?;
        Ok(self.axes[phys].axis().direction.unwrap_or(phys != 0))
    }

    /// The highest axis value to display; see [`Frame::top`].
    ///
    /// The default is pi/2 (+90 degrees) for the latitude axis; the
    /// longitude axis displays all values.
    // C: skyframe.c:3242 `GetTop`.
    fn top(&self, axis: usize) -> AstResult<f64> {
        let phys = self.physical_axis(axis)?;
        Ok(self.axes[phys].axis().top.unwrap_or(if phys == 1 {
            std::f64::consts::FRAC_PI_2
        } else {
            f64::MAX
        }))
    }

    /// The lowest axis value to display; see [`Frame::bottom`].
    ///
    /// The default is -pi/2 (-90 degrees) for the latitude axis; the
    /// longitude axis displays all values.
    // C: skyframe.c:2915 `GetBottom`.
    fn bottom(&self, axis: usize) -> AstResult<f64> {
        let phys = self.physical_axis(axis)?;
        Ok(self.axes[phys].axis().bottom.unwrap_or(if phys == 1 {
            -std::f64::consts::FRAC_PI_2
        } else {
            -f64::MAX
        }))
    }

    /// A SkyAxis's own Top default follows its IsLatitude attribute
    /// (skyaxis.c:2755), which a SkyFrame never sets on its axes: it sets it
    /// only while formatting and on a picked copy (skyframe.c:3503, 10282).
    fn axis_object_top(&self, axis: usize) -> AstResult<f64> {
        Ok(self.ax(axis)?.top.unwrap_or(f64::MAX))
    }

    /// As [`axis_object_top`](Frame::axis_object_top), for Bottom.
    fn axis_object_bottom(&self, axis: usize) -> AstResult<f64> {
        Ok(self.ax(axis)?.bottom.unwrap_or(-f64::MAX))
    }

    /// The label of an axis; see [`Frame::label`].
    ///
    /// The default suits the celestial coordinate system, such as
    /// `"Right ascension"` or `"Galactic latitude"`, with `" offset"`
    /// appended in an offset coordinate system (see
    /// [`sky_ref`](SkyFrame::sky_ref)).
    // C: skyframe.c:3564 `GetLabel`.
    fn label(&self, axis: usize) -> AstResult<String> {
        let phys = self.physical_axis(axis)?;
        if let Some(s) = &self.axes[phys].axis().label {
            return Ok(s.clone());
        }
        let (lon, lat) = self.default_labels();
        Ok(if phys == 0 { lon } else { lat })
    }

    /// The symbol of an axis; see [`Frame::symbol`].
    ///
    /// The default suits the celestial coordinate system, such as `"RA"`
    /// or `"Dec"`, with a `"D"` in front in an offset coordinate system (see
    /// [`sky_ref`](SkyFrame::sky_ref)).
    // C: skyframe.c:4230 `GetSymbol`.
    fn symbol(&self, axis: usize) -> AstResult<String> {
        let phys = self.physical_axis(axis)?;
        if let Some(s) = &self.axes[phys].axis().symbol {
            return Ok(s.clone());
        }
        let (lon, lat) = self.default_symbols();
        let base = if phys == 0 { lon } else { lat };
        Ok(if self.is_offset() {
            format!("D{base}")
        } else {
            base.to_string()
        })
    }

    /// The units of formatted values on an axis; see [`Frame::unit`].
    ///
    /// The default describes the string [`format`](Frame::format)
    /// returns, such as `"hh:mm:ss.sss"` for an axis formatted as a time,
    /// following the axis's [`format_str`](SkyFrame::format_str); it is
    /// `"rad"` for a Format starting with `%`.
    /// The floating point values themselves are always in radians.
    // C: skyframe.c:4778 `GetUnit`.
    fn unit(&self, axis: usize) -> AstResult<String> {
        Ok(self.phys_unit(self.physical_axis(axis)?))
    }

    /// The format specification for values on an axis; see
    /// [`Frame::format_str`].
    ///
    /// The default suits the celestial coordinate system: sexagesimal
    /// hours, minutes and seconds or degrees, minutes and seconds (as
    /// [`as_time`](SkyFrame::as_time) chooses) for an equatorial system,
    /// and decimal degrees or hours for the others, with a precision that
    /// follows the axis's [`axis_digits`](Frame::axis_digits).
    ///
    /// # SkyFrame formats
    ///
    /// A SkyFrame Format holds zero or more of these characters, in any
    /// order (the order given here is clearest), case-insensitive:
    ///
    /// - `+`: prefix a plus sign to positive values (none by default).
    /// - `z`: prefix leading zeros so the first field has a constant width,
    ///   as for a fixed-width table (later fields always have them); none by
    ///   default.
    /// - `i`: separate fields with the ISO separator, a colon; the default.
    /// - `b`: separate fields with a blank.
    /// - `l`: separate fields with a letter (`h`/`d`, `m` or `s`).
    /// - `g`: separate fields with letters and symbols marked up by escape
    ///   sequences for graphical output.
    ///   Escape sequences are not produced, so `g` behaves as `l`.
    /// - `d`: include a degrees field; expressing the angle purely in
    ///   degrees is also the default when none of `h`, `m`, `s` or `t` is
    ///   given.
    /// - `h`: express the angle as a time (24 hours to 360 degrees) and
    ///   include an hours field; purely hours is also the default when `t`
    ///   is given without `m` or `s`.
    /// - `m`: include a minutes field (not by default).
    /// - `s`: include a seconds field (not by default); ignored with `d` or
    ///   `h` unless a minutes field is included too.
    /// - `t`: express the angle as a time with no hours field, purely in
    ///   minutes and/or seconds; ignored with `d` or `h`, and equivalent to
    ///   `h` when none of `d`, `h`, `m` or `s` is given.
    /// - `.`: give decimal places in the last field, followed by the number
    ///   of places or by `*`, which takes the number from Digits.
    ///
    /// Where characters conflict (`i` and `b`, say) the last wins, except
    /// that `d` and `h` always override `t`.
    /// A Format starting with `%` follows the Frame syntax, and formats the
    /// value as decimal radians.
    ///
    /// ```
    /// use starlink_ast::frames::{Frame, SkyFrame, SkySystem};
    ///
    /// let mut sky = SkyFrame::new(SkySystem::Fk5);
    /// sky.set_format_str(1, "lhms.1").unwrap();
    /// assert_eq!(sky.format(1, 1.0).unwrap(), "3h49m11.0s");
    /// sky.set_format_str(2, "+zbdms").unwrap();
    /// assert_eq!(sky.format(2, 0.1).unwrap(), "+005 43 46");
    /// ```
    // C: skyframe.c:3394 `GetFormat`.
    fn format_str(&self, axis: usize) -> AstResult<String> {
        Ok(self.phys_format(self.physical_axis(axis)?))
    }

    /// Set `Format(axis)`, in the syntax described under
    /// [`format_str`](SkyFrame::format_str).
    // C: a SkyAxis keeps its own Format (skyaxis.c `SetAxisFormat`).
    fn set_format_str(&mut self, axis: usize, fmt: &str) -> AstResult<()> {
        let phys = self.physical_axis(axis)?;
        *self.axes[phys].format_slot_mut() = Some(fmt.to_string());
        Ok(())
    }

    fn test_format_str(&self, axis: usize) -> AstResult<bool> {
        Ok(self.axes[self.physical_axis(axis)?].format_slot().is_some())
    }

    fn clear_format_str(&mut self, axis: usize) -> AstResult<()> {
        let phys = self.physical_axis(axis)?;
        *self.axes[phys].format_slot_mut() = None;
        Ok(())
    }

    fn axis_digits(&self, axis: usize) -> AstResult<i32> {
        Ok(self.phys_digits(self.physical_axis(axis)?))
    }

    axis_attrs_store!(label, symbol, unit, digits, direction, top, bottom);

    /// Always false for a SkyFrame; see [`Frame::get_active_unit`].
    fn get_active_unit(&self) -> bool {
        false // SkyFrame always reports 0 (frame.c:4651, SkyAxis present).
    }

    /// Has no effect on a SkyFrame, whose ActiveUnit flag is always false;
    /// see [`Frame::set_active_unit`].
    fn set_active_unit(&mut self, _value: bool) {}

    fn test_active_unit(&self) -> bool {
        false
    }

    fn internal_unit(&self, axis: usize) -> AstResult<String> {
        // SkyFrame axes are angles stored internally in radians. Mirrors
        // C `astGetInternalUnit` for a SkyAxis returning "rad", after the
        // axis is validated.
        super::frame_attrs::validate_axis(axis, self.naxes())?;
        Ok("rad".to_string())
    }

    /// A Unit that describes the axis values, such as `"hh:mm:ss.s"`, has no
    /// normalized form and is returned unchanged, whether it is the default
    /// or has been set to that same description.
    /// This has to be decided here rather than by the units parser, which
    /// reads the spaces in a description such as `"hh mm ss"` as
    /// multiplication.
    /// Any other Unit, and every Unit of an axis with a `%`-style Format, is
    /// normalized.
    // C: `SkyFrame::GetNormUnit` (skyframe.c:4349), which asks the SkyAxis
    // (`SkyAxis::GetAxisNormUnit`, skyaxis.c:3095).
    fn norm_unit(&self, axis: usize) -> AstResult<String> {
        super::frame_attrs::validate_axis(axis, self.naxes())?;
        let phys = self.physical_axis(axis)?;
        // A plain Axis normalizes the Unit it reports (axis.c).
        if let FrameAxis::Plain(_) = &self.axes[phys] {
            return Ok(crate::units::norm_unit(&self.phys_unit(phys)));
        }
        let fmt = self.phys_format(phys);
        if !fmt.starts_with('%') {
            let (description, describes_values) = dhms_unit(&fmt, self.phys_digits(phys));
            if describes_values
                && self.axes[phys]
                    .axis()
                    .unit
                    .as_ref()
                    .is_none_or(|unit| *unit == description)
            {
                return Ok(description);
            }
        }
        Ok(crate::units::norm_unit(&self.phys_unit(phys)))
    }

    /// The axis Format, set or default, formats the value as a sky axis does.
    // C: `SkyFrame::Format` (skyframe.c:2025) formats as `SkyAxis::AxisFormat`
    // does.
    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        let phys = self.physical_axis(axis)?;
        if is_bad(value) {
            return Ok("<bad>".to_string());
        }
        // The Format is lent to the Axis object, which formats the value as
        // its own class does (skyframe.c:2025, frame.c `Format`).
        let (fmt, digits) = (self.phys_format(phys), self.phys_digits(phys));
        match &self.axes[phys] {
            FrameAxis::Plain(_) => super::axis_format::format_value(&fmt, digits, value),
            FrameAxis::Sky(_) => super::skyaxis::sky_format(&fmt, digits, value),
        }
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        if !(1..=2).contains(&axis) {
            return Err(AstError::new_tracked(
                ErrorCode::Axiin,
                format!("axis index {} is out of range 1..=2", axis),
            ));
        }
        // skyframe.c:10892 reads with the SkyFrame's Format, set or default.
        let phys = self.perm[axis - 1];
        match &self.axes[phys] {
            FrameAxis::Plain(_) => Ok(super::basic::axis_unformat(text)),
            FrameAxis::Sky(_) => {
                unformat_sky_axis(text, &self.phys_format(phys), self.phys_digits(phys))
            }
        }
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        super::assert_points_len("distance", self.naxes(), &[a, b]);
        if is_bad(a[0]) || is_bad(a[1]) || is_bad(b[0]) || is_bad(b[1]) {
            return Ok(BAD_VALUE);
        }
        let lp = self.lon_pos();
        let bp = self.lat_pos();
        Ok(pal::dsep(a[lp], a[bp], b[lp], b[bp]))
    }

    /// The vector from point 1 to point 3 is resolved along and across the
    /// great circle from point 1 to point 2.
    /// `d1` is the signed arc from point 1 to point 4, the closest approach,
    /// and `d2` the unsigned arc from point 4 to point 3, both in radians.
    /// Everything is BAD for a bad input, for points 1 and 2 that coincide or
    /// are opposite, or for a point 3 at the pole of the great circle.
    // C: `SkyFrame::Resolve` (skyframe.c:8321).
    fn resolve(
        &self,
        point1: &[f64],
        point2: &[f64],
        point3: &[f64],
    ) -> AstResult<(Vec<f64>, f64, f64)> {
        super::assert_points_len("resolve", self.naxes(), &[point1, point2, point3]);
        let mut point4 = vec![BAD_VALUE, BAD_VALUE];
        let (mut d1, mut d2) = (BAD_VALUE, BAD_VALUE);
        if [point1, point2, point3]
            .iter()
            .any(|p| is_bad(p[0]) || is_bad(p[1]))
        {
            return Ok((point4, d1, d2));
        }
        // (longitude, latitude) order.
        let perm = self.perm;
        let internal = |p: &[f64]| {
            let mut q = [0.0; 2];
            q[perm[0]] = p[0];
            q[perm[1]] = p[1];
            q
        };
        let (p1, p2, p3) = (internal(point1), internal(point2), internal(point3));
        let v1 = pal::dcs2c(p1[0], p1[1]);
        let v2 = pal::dcs2c(p2[0], p2[1]);
        let v3 = pal::dcs2c(p3[0], p3[1]);
        // The unit normal to the great circle of the basis vector.
        let (n1, vmod) = pal::dvn(&pal::dvxv(&v2, &v1));
        if vmod > 0.0 {
            // The point 90 degrees from point 1 towards point 2.
            let v5 = pal::dvxv(&v1, &n1);
            // The unit normal to the great circle through point 3
            // perpendicular to the first.
            let (n2, vmod) = pal::dvn(&pal::dvxv(&v3, &n1));
            if vmod > 0.0 {
                // Their intersection, already a unit vector.
                let v4 = pal::dvxv(&n1, &n2);
                d1 = crate::math::atan2(pal::dvdv(&v4, &v5), pal::dvdv(&v4, &v1));
                d2 = crate::math::atan2(pal::dvdv(&v3, &n1), pal::dvdv(&v3, &v4)).abs();
                let (lon, lat) = pal::dcc2s(&v4);
                let p4 = [pal::dranrm(lon), lat];
                point4 = vec![p4[perm[0]], p4[perm[1]]];
            }
        }
        Ok((point4, d1, d2))
    }

    /// Where the great circle through `a1` and `a2` crosses the one through
    /// `b1` and `b2`, choosing of the two crossings the one nearer `a1`.
    /// BAD for a bad input or great circles that coincide.
    // C: `SkyFrame::Intersect` (skyframe.c).
    fn intersect(&self, a1: &[f64], a2: &[f64], b1: &[f64], b2: &[f64]) -> AstResult<Vec<f64>> {
        super::assert_points_len("intersect", 2, &[a1, a2, b1, b2]);
        let mut cross = vec![BAD_VALUE, BAD_VALUE];
        if [a1, a2, b1, b2]
            .iter()
            .any(|p| is_bad(p[0]) || is_bad(p[1]))
        {
            return Ok(cross);
        }
        let perm = self.perm;
        let vector = |p: &[f64]| {
            let mut q = [0.0; 2];
            q[perm[0]] = p[0];
            q[perm[1]] = p[1];
            pal::dcs2c(q[0], q[1])
        };
        let (va1, va2, vb1, vb2) = (vector(a1), vector(a2), vector(b1), vector(b2));
        let na = pal::dvxv(&va1, &va2);
        let nb = pal::dvxv(&vb1, &vb2);
        let (mut vpn, vmod) = pal::dvn(&pal::dvxv(&na, &nb));
        if vmod != 0.0 {
            // The crossing nearer a1: the larger cosine of the distance.
            let d1 = pal::dvdv(&vpn, &va1);
            let opposite = [-vpn[0], -vpn[1], -vpn[2]];
            let d2 = pal::dvdv(&opposite, &va1);
            if d1 <= d2 {
                vpn = opposite;
            }
            let (lon, lat) = pal::dcc2s(&vpn);
            let cc = [pal::dranrm(lon), lat];
            cross = vec![cc[perm[0]], cc[perm[1]]];
        }
        Ok(cross)
    }

    fn resolve_points(
        &self,
        point1: &[f64],
        point2: &[f64],
        input: &crate::coords::PointSet,
    ) -> AstResult<crate::coords::PointSet> {
        super::assert_points_len("resolve_points", self.naxes(), &[point1, point2]);
        // Faithful port of `skyframe.c::ResolvePoints` (8399). Unlike the
        // planar base-Frame implementation, this resolves each vector using
        // spherical (great-circle) geometry: the parallel component is the
        // signed arc distance measured along the great circle through
        // `point1`->`point2`, and the perpendicular component is the signed
        // arc distance from that great circle to the point.
        let npoint = input.npoint();
        let mut d1v = vec![BAD_VALUE; npoint];
        let mut d2v = vec![BAD_VALUE; npoint];

        // Axis permutation → (longitude,latitude) order. If the axes have been
        // swapped, the sign of the returned perpendicular distances flips
        // (skyframe.c:8575).
        let perm = self.perm;
        let sign = if perm[0] == 0 { -1.0 } else { 1.0 };

        // Permute the two basis-vector endpoints into (lon,lat) order.
        let mut p1 = [0.0f64; 2];
        let mut p2 = [0.0f64; 2];
        p1[perm[0]] = point1[0];
        p1[perm[1]] = point1[1];
        p2[perm[0]] = point2[0];
        p2[perm[1]] = point2[1];

        // Unit 3-vectors for the basis endpoints.
        let v1 = pal::dcs2c(p1[0], p1[1]);
        let v2 = pal::dcs2c(p2[0], p2[1]);

        // Unit normal to the great-circle plane through v1 and v2. Undefined
        // (bad results) when the endpoints coincide or are antipodal.
        let (n1, vmod) = pal::dvn(&pal::dvxv(&v2, &v1));
        if vmod > 0.0 {
            // v5 is 90 degrees from v1 along the great circle, towards v2.
            let v5 = pal::dvxv(&v1, &n1);

            for ip in 0..npoint {
                let point = input.point(ip);
                let (px, py) = (point[0], point[1]);
                if is_bad(px) || is_bad(py) {
                    continue;
                }

                // Permute the outlying point into (lon,lat) order.
                let mut p3 = [0.0f64; 2];
                p3[perm[0]] = px;
                p3[perm[1]] = py;
                let v3 = pal::dcs2c(p3[0], p3[1]);

                // Unit normal to the great circle through v3 perpendicular to
                // n1. Undefined when v3 lies on the pole of the basis circle.
                let (n2, vmod2) = pal::dvn(&pal::dvxv(&v3, &n1));
                if vmod2 <= 0.0 {
                    continue;
                }

                // Point of closest approach: intersection of the two great
                // circles = n1 x n2 (already unit length).
                let v4 = pal::dvxv(&n1, &n2);

                // Parallel distance (signed, -PI..PI).
                d1v[ip] = crate::math::atan2(pal::dvdv(&v4, &v5), pal::dvdv(&v4, &v1));
                // Perpendicular distance (signed by axis order).
                d2v[ip] = sign * crate::math::atan2(pal::dvdv(&v3, &n1), pal::dvdv(&v3, &v4));
            }
        }

        crate::coords::PointSet::from_columns(&[&d1v, &d2v])
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        super::assert_points_len("angle", self.naxes(), &[a, b, c]);
        if a.iter().chain(b.iter()).chain(c.iter()).any(|&v| is_bad(v)) {
            return Ok(BAD_VALUE);
        }
        // The angle is undefined when either end point coincides with
        // the vertex (skyframe.c:1182-1189).
        if (a[0] == b[0] && a[1] == b[1]) || (c[0] == b[0] && c[1] == b[1]) {
            return Ok(BAD_VALUE);
        }
        let lp = self.lon_pos();
        let bp = self.lat_pos();
        // Bearings of A and C as seen *from the vertex B*; the result
        // is their signed difference, corrected from "angle from
        // north" to "angle from axis 2" when the axes are permuted
        // (skyframe.c:1193-1209).
        let anga = pal::dbear(b[lp], b[bp], a[lp], a[bp]);
        let angc = pal::dbear(b[lp], b[bp], c[lp], c[bp]);
        let mut result = angc - anga;
        if self.perm[0] != 0 {
            result = std::f64::consts::FRAC_PI_2 - result;
        }
        Ok(pal::drange(result))
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        super::assert_points_len("offset", self.naxes(), &[from, to]);
        if from.iter().chain(to.iter()).any(|&v| is_bad(v)) || is_bad(dist) {
            return Ok(vec![BAD_VALUE; 2]);
        }

        let lp = self.lon_pos();
        let bp = self.lat_pos();

        let v1 = pal::dcs2c(from[lp], from[bp]);
        let v2 = pal::dcs2c(to[lp], to[bp]);

        // C reverses the vector order (palDvxv(v2, v1) — skyframe.c:7697)
        // and rotates by +offset; keep that exact form so results match
        // bit-for-bit even where rounding noise dominates the cross
        // product (nearly coincident points).
        let axis_vec = pal::dvxv(&v2, &v1);
        let (axis_norm, axis_len) = pal::dvn(&axis_vec);

        // Coincident or diametrically opposite points define no great
        // circle. C generates output only when the offset is an exact
        // multiple of pi (rotating v1 by cos(offset)); otherwise the
        // result is bad (skyframe.c:7709-7727).
        if axis_len == 0.0 {
            if crate::math::sin(dist) == 0.0 {
                let scale = crate::math::cos(dist);
                let v3 = [v1[0] * scale, v1[1] * scale, v1[2] * scale];
                // C constrains the longitude to 0..2*pi, which palDcc2s does
                // not do itself (skyframe.c:7718-7720).
                let (lon, lat) = pal::dcc2s(&v3);
                let mut result = vec![0.0; 2];
                result[lp] = pal::dranrm(lon);
                result[bp] = lat;
                return Ok(result);
            }
            return Ok(vec![BAD_VALUE; 2]);
        }

        let rot_vec = [
            axis_norm[0] * dist,
            axis_norm[1] * dist,
            axis_norm[2] * dist,
        ];

        let rmat = pal::dav2m(&rot_vec);
        let v_out = pal::dmxv(&rmat, &v1);
        // C constrains the longitude to 0..2*pi (skyframe.c:7744-7746).
        let (lon, lat) = pal::dcc2s(&v_out);

        let mut result = vec![0.0; 2];
        result[lp] = pal::dranrm(lon);
        result[bp] = lat;
        Ok(result)
    }

    /// Longitude into 0..2pi (or -pi..pi with NegLon), latitude into
    /// -pi/2..pi/2, moving the longitude through the pole when the latitude
    /// passes it.
    /// A value that is bad leaves the other to be normalized alone.
    // C: skyframe.c `Norm`.
    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        use std::f64::consts::PI;
        super::assert_points_len("norm", self.naxes(), &[&*coords]);
        let lp = self.lon_pos();
        let bp = self.lat_pos();
        let mut lon = coords[lp];
        let mut lat = coords[bp];
        if !is_bad(lon) && !is_bad(lat) {
            lon = pal::dranrm(lon);
            lat = pal::drange(lat);
            if lat > PI / 2.0 {
                lon += if lon < PI { PI } else { -PI };
                lat = PI - lat;
            } else if lat < -(PI / 2.0) {
                lon += if lon < PI { PI } else { -PI };
                lat = -PI - lat;
            }
        } else if !is_bad(lon) {
            lon = pal::dranrm(lon);
        } else if !is_bad(lat) {
            lat = pal::drange(lat);
            if lat > PI / 2.0 {
                lat = PI - lat;
            } else if lat < -(PI / 2.0) {
                lat = -PI - lat;
            }
        }
        // A longitude a rounding error short of 2pi becomes zero
        // (skyframe.c:7195, DBL_EPSILON * FLT_RADIX).
        if (lon - 2.0 * PI).abs() <= 2.0 * PI * (f64::EPSILON * 2.0) {
            lon = 0.0;
        }
        if !is_bad(lon) && self.neg_lon() {
            lon = pal::drange(lon);
        }
        coords[lp] = lon;
        coords[bp] = lat;
        Ok(())
    }

    /// About `size` points on parallels through the centers of roughly square
    /// cells of equal area, the longitude bounds taken into 0 to 2*pi and the
    /// upper one raised a turn if it is not above the lower.
    // C: skyframe.c `FrameGrid`.
    fn frame_grid(
        &self,
        size: i32,
        lbnd: &[f64],
        ubnd: &[f64],
    ) -> AstResult<crate::coords::PointSet> {
        let ilon = self.perm[0];
        let ilat = 1 - ilon;
        let (lolat, hilat) = if lbnd[ilat] <= ubnd[ilat] {
            (lbnd[ilat], ubnd[ilat])
        } else {
            (ubnd[ilat], lbnd[ilat])
        };
        let (lolon, hilon) = (lbnd[ilon], ubnd[ilon]);
        if size <= 0 || [lolat, hilat, lolon, hilon].contains(&BAD_VALUE) {
            return Err(AstError::new(
                ErrorCode::Attin,
                if size < 1 {
                    format!(
                        "astFrameGrid(SkyFrame): The supplied grid size ({size}) is \
                         invalid (programming error)."
                    )
                } else {
                    "astFrameGrid(SkyFrame): One of more of the supplied bounds is \
                     AST__BAD (programming error)."
                        .to_string()
                },
            ));
        }
        let turn = 2.0 * std::f64::consts::PI;
        let lolon = pal::dranrm(lolon);
        let mut hilon = pal::dranrm(hilon);
        if hilon <= lolon && ubnd[ilon] != lbnd[ilon] {
            hilon += turn;
        }
        let dlon = hilon - lolon;
        let box_area = (dlon * (crate::math::sin(hilat) - crate::math::sin(lolat))).abs();
        let mut lat_size = crate::math::sqrt(box_area / size as f64);
        // C's `(int)( 0.5 + ... )`.
        let npar = ((0.5 + (hilat - lolat) / lat_size) as i32).max(2);
        lat_size = (hilat - lolat) / npar as f64;
        let mut totlen = 0.0;
        let mut lat = lolat + 0.5 * lat_size;
        for _ in 0..npar {
            totlen += dlon * crate::math::cos(lat);
            lat += lat_size;
        }
        let lon_size = totlen / size as f64;
        let ipmax = 2 * size;
        let mut points: Vec<[f64; 2]> = Vec::new();
        let mut ip = 0i32;
        lat = lolat + 0.5 * lat_size;
        for _ in 0..npar {
            let cl = crate::math::cos(lat);
            let mut inclon = if cl != 0.0 { lon_size / cl } else { 0.0 };
            let mut nmer = (dlon / inclon) as i32;
            if ip + nmer >= ipmax {
                nmer = ipmax - ip;
            }
            inclon = dlon / nmer as f64;
            let mut lon = lolon + 0.5 * inclon;
            for _ in 0..nmer {
                let mut p = [0.0; 2];
                p[ilon] = lon;
                p[ilat] = lat;
                points.push(p);
                lon += inclon;
                ip += 1;
            }
            lat += lat_size;
        }
        Ok(crate::coords::PointSet::from_rows(points))
    }

    /// Normalize each point, then, when `contig` is set, move the longitudes
    /// by whole turns so that they lie within half a turn of the point
    /// opposite the widest gap between them, found from a histogram of
    /// one-degree bins.
    // C: skyframe.c `NormalPoints`.
    fn normal_points(
        &self,
        points: &crate::coords::PointSet,
        contig: bool,
    ) -> AstResult<crate::coords::PointSet> {
        let mut out = super::norm_each_point(self, points)?;
        if !contig {
            return Ok(out);
        }
        let lon = self.perm[0];
        let mut hist = [0usize; 360];
        for i in 0..out.npoint() {
            let v = out.point(i)[lon];
            if v != BAD_VALUE {
                // C's `(int)` truncation of the angle in degrees.
                let ihist = (v * AST_DR2D) as i64;
                if (0..360).contains(&ihist) {
                    hist[ihist as usize] += 1;
                }
            }
        }
        // The widest run of empty bins, and the first and last full ones.
        let (mut ingap, mut maxgap, mut maxgap_centre) = (false, 0i64, 0i64);
        let (mut gap_start, mut ifirst, mut ilast) = (0i64, -1i64, -1i64);
        for (ihist, &count) in hist.iter().enumerate() {
            let ihist = ihist as i64;
            if count == 0 {
                if !ingap {
                    ingap = true;
                    gap_start = ihist;
                }
            } else {
                if ingap {
                    ingap = false;
                    let gap_end = ihist - 1;
                    let gap_size = gap_end - gap_start + 1;
                    if gap_size > maxgap {
                        maxgap = gap_size;
                        maxgap_centre = (gap_start + gap_end) / 2;
                    }
                }
                if ifirst == -1 {
                    ifirst = ihist;
                }
                ilast = ihist;
            }
        }
        // The gap made by joining the empty bins at the two ends.
        let gap_start = ilast - 360;
        let gap_end = ifirst;
        let gap_size = gap_end - gap_start + 1;
        if gap_size > maxgap {
            maxgap_centre = (gap_start + gap_end) / 2;
        }
        // C's integer division truncates towards zero, as Rust's does.
        let centre = if maxgap_centre > 0 {
            maxgap_centre - 180
        } else {
            maxgap_centre + 180
        };
        let lo = (centre - 180) as f64 * AST_DD2R;
        let hi = lo + 2.0 * std::f64::consts::PI;
        for i in 0..out.npoint() {
            let v = &mut out.point_mut(i)[lon];
            if *v != BAD_VALUE {
                while *v < lo {
                    *v += 2.0 * std::f64::consts::PI;
                }
                while *v > hi {
                    *v -= 2.0 * std::f64::consts::PI;
                }
            }
        }
        Ok(out)
    }

    /// Every SkyFrame axis carries a SkyAxis, so the per-axis methods get
    /// celestial wrapping.
    /// Which SkyAxis depends on the axis permutation.
    // C: the Axis array a SkyFrame inherits from Frame, reached through
    // `astGetAxis` (frame.c:5309) after `astValidateAxis` permutes.
    fn get_axis(&self, axis: usize) -> AxisKind {
        self.axes[self.perm[axis - 1]].kind()
    }

    /// Offset along a great circle at a given bearing.
    /// Builds the reference vectors, offsets along the great circle,
    /// normalizes, and returns the bearing of the great circle at the end
    /// point.
    // C: `SkyFrame::Offset2` (skyframe.c:7937), through `Shcal` and `Shapp`.
    fn offset2(&self, point1: &[f64], angle: f64, offset: f64) -> AstResult<(Vec<f64>, f64)> {
        super::assert_points_len("offset2", 2, &[point1]);
        let perm = self.perm;

        // Bad inputs give all-BAD output and a BAD returned angle.
        if is_bad(point1[0]) || is_bad(point1[1]) || is_bad(angle) || is_bad(offset) {
            return Ok((vec![BAD_VALUE, BAD_VALUE], BAD_VALUE));
        }

        let piby2 = std::f64::consts::FRAC_PI_2;

        // Apply the axis permutation to obtain (longitude, latitude) order.
        let mut p1 = [0.0f64; 2];
        p1[perm[0]] = point1[0];
        p1[perm[1]] = point1[1];

        // If the axes are permuted, convert the supplied angle into a
        // position angle (measured from north).
        let pa = if perm[0] == 0 { angle } else { piby2 - angle };

        // Shcal: reference vector R0 and the point 90 degrees along the
        // great circle R3.
        let (r0, r3) = shcal(p1[0], p1[1], pa);

        // Shapp: the new position offset by `offset` along the great circle.
        let mut p2 = shapp(offset, &r0, &r3, p1[0]);

        // Normalize the result. `p2` is in physical (lon, lat) order;
        // `norm` reads/writes lon at lon_pos and lat at lat_pos, so map
        // through the permutation and back.
        {
            let mut ext = vec![0.0; 2];
            ext[self.lon_pos()] = p2[0];
            ext[self.lat_pos()] = p2[1];
            self.norm(&mut ext)?;
            p2[0] = ext[self.lon_pos()];
            p2[1] = ext[self.lat_pos()];
        }

        // Compute the bearing of the great circle at the required point.
        let sina1 = crate::math::sin(p2[0]);
        let cosa1 = crate::math::cos(p2[0]);
        let sinb1 = crate::math::sin(p2[1]);
        let cosb1 = crate::math::cos(p2[1]);

        // Q1: point in the meridian of p2, 90 deg further north.
        let q1 = [-sinb1 * cosa1, -sinb1 * sina1, cosb1];
        // Q2: point on the equator, 90 deg west of p2.
        let q2 = [-sina1, cosa1, 0.0];
        // Q3: point 90 deg away from p2 along the great circle.
        let cosoff = crate::math::cos(offset);
        let sinoff = crate::math::sin(offset);
        let q3 = [
            -sinoff * r0[0] + cosoff * r3[0],
            -sinoff * r0[1] + cosoff * r3[1],
            -sinoff * r0[2] + cosoff * r3[2],
        ];

        let pa_end = crate::math::atan2(pal::dvdv(&q3, &q2), pal::dvdv(&q3, &q1));
        let result = if perm[0] == 0 { pa_end } else { piby2 - pa_end };
        let result = pal::dranrm(result);

        // Undo the axis permutation on the output coordinates.
        let mut point2 = vec![0.0; 2];
        point2[0] = p2[perm[0]];
        point2[1] = p2[perm[1]];

        Ok((point2, result))
    }

    /// Extend a box to include the effect of the celestial poles.
    /// Tests whether the box (via the supplied region-membership predicate)
    /// includes either pole and, if so, expands the longitude range to
    /// 0..2*PI and pins the latitude limit to the pole.
    ///
    /// If testing a pole fails, the error is returned and the limits are left
    /// as they arrived.
    // C: `SkyFrame::NormBox` (skyframe.c:7244). C tests its error status after
    // transforming the two poles and treats a failed transformation as finding
    // neither pole, because the values it would otherwise read were never
    // written (skyframe.c:7338-7342).
    fn norm_box(
        &self,
        lbnd: &mut [f64],
        ubnd: &mut [f64],
        inside: &dyn Fn(&[f64]) -> AstResult<bool>,
    ) -> AstResult<()> {
        let perm = self.perm;
        let piby2 = std::f64::consts::FRAC_PI_2;

        // Obtain the sky longitude and latitude limits, allowing for any
        // axis permutation.
        let mut lb = [0.0f64; 2];
        let mut ub = [0.0f64; 2];
        lb[perm[0]] = lbnd[0];
        lb[perm[1]] = lbnd[1];
        ub[perm[0]] = ubnd[0];
        ub[perm[1]] = ubnd[1];

        // Coordinates of the north and south poles, in external axis order.
        let (npole, spole) = if perm[0] == 0 {
            ([0.0, piby2], [0.0, -piby2])
        } else {
            ([piby2, 0.0], [-piby2, 0.0])
        };

        // Test whether the box includes each pole.
        let north_in = inside(&npole)?;
        let south_in = inside(&spole)?;

        // If the box includes the north pole...
        if north_in {
            let t = if !is_bad(ub[1]) && !is_bad(lb[1]) {
                let t = pal::drange(ub[1]);
                let t2 = pal::drange(lb[1]);
                if t2 < t { t2 } else { t }
            } else {
                BAD_VALUE
            };
            lb[1] = t;
            ub[1] = piby2;
            lb[0] = 0.0;
            ub[0] = 2.0 * std::f64::consts::PI;
        }

        // If the box includes the south pole...
        if south_in {
            let t = if !is_bad(ub[1]) && !is_bad(lb[1]) {
                let t = pal::drange(ub[1]);
                let t2 = pal::drange(lb[1]);
                if t2 > t { t2 } else { t }
            } else {
                BAD_VALUE
            };
            lb[1] = -piby2;
            ub[1] = t;
            lb[0] = 0.0;
            ub[0] = 2.0 * std::f64::consts::PI;
        }

        // Return the modified limits, undoing the axis permutation.
        lbnd[0] = lb[perm[0]];
        lbnd[1] = lb[perm[1]];
        ubnd[0] = ub[perm[0]];
        ubnd[1] = ub[perm[1]];
        Ok(())
    }

    // --- Great-circle line geometry (skyframe.c LineDef/LineOffset/
    // LineCrossing/LineContains) ---
    //
    // These override the planar Frame defaults with spherical geometry. A line
    // is a great-circle arc; positions are held as 3-D unit vectors in the
    // returned `LineDef`'s `sky` field. All four require a `SkyLineDef` built
    // by `line_def`.

    fn line_def(&self, start: &[f64], end: &[f64]) -> AstResult<Option<LineDef>> {
        if is_bad(start[0]) || is_bad(start[1]) || is_bad(end[0]) || is_bad(end[1]) {
            return Ok(None);
        }

        let perm = self.perm;

        // Permute external axis values into physical (longitude, latitude)
        // order before converting to Cartesian.
        let mut p1 = [0.0; 2];
        let mut p2 = [0.0; 2];
        p1[perm[0]] = start[0];
        p1[perm[1]] = start[1];
        p2[perm[0]] = end[0];
        p2[perm[1]] = end[1];

        let sv = pal::dcs2c(p1[0], p1[1]);
        let ev = pal::dcs2c(p2[0], p2[1]);

        // Great-circle length in radians. `palDvdv` gives the raw dot product;
        // correct for palDcs2c rounding so the vectors are treated as unit
        // length before taking the arc-cosine.
        let dot = pal::dvdv(&sv, &ev);
        let ls = sv[0] * sv[0] + sv[1] * sv[1] + sv[2] * sv[2];
        let le = ev[0] * ev[0] + ev[1] * ev[1] + ev[2] * ev[2];
        let coslength = dot / crate::math::sqrt(ls * le);
        let length = if coslength < -1.0 {
            std::f64::consts::PI
        } else if coslength > 1.0 {
            0.0
        } else {
            crate::math::acos(coslength)
        };

        // Pole of the great circle, oriented so it lies to the left of the
        // line travelling start→end. Undefined for a zero-length or 180° arc,
        // where an arbitrary nearby direction is used.
        let pole_raw = if length == 0.0 || length > std::f64::consts::PI - 5.0e-11 {
            let temp = pal::dcs2c(p1[0] + 0.01, p1[1] + 0.01);
            pal::dvxv(&temp, &sv)
        } else {
            pal::dvxv(&ev, &sv)
        };
        let (q, _len) = pal::dvn(&pole_raw);

        // Quarter point: 90° along the great circle from the start.
        let dir = pal::dvxv(&sv, &q);

        // Normalized external-order end points (mirrors C start_2d/end_2d).
        let mut start_2d = [start[0], start[1]];
        let mut end_2d = [end[0], end[1]];
        self.norm(&mut start_2d)?;
        self.norm(&mut end_2d)?;

        Ok(Some(LineDef {
            length,
            infinite: false,
            start: start_2d,
            end: end_2d,
            dir: [0.0, 0.0],
            q: [0.0, 0.0],
            sky: Some(SkyLineDef {
                start: sv,
                end: ev,
                dir,
                q,
            }),
        }))
    }

    fn line_offset(&self, line: &LineDef, par: f64, prp: f64) -> AstResult<[f64; 2]> {
        let sl = line.sky.as_ref().ok_or_else(|| {
            AstError::new(ErrorCode::Inter, "line_offset: line is not a SkyFrame line")
        })?;

        // Move a distance `par` along the great circle from the start.
        let c = crate::math::cos(par);
        let s = crate::math::sin(par);
        let nx = c * sl.start[0] + s * sl.dir[0];
        let ny = c * sl.start[1] + s * sl.dir[1];
        let nz = c * sl.start[2] + s * sl.dir[2];

        // Move a distance `prp` at right angles, towards the pole.
        let v = if prp != 0.0 {
            let c2 = crate::math::cos(prp);
            let s2 = crate::math::sin(prp);
            [
                c2 * nx + s2 * sl.q[0],
                c2 * ny + s2 * sl.q[1],
                c2 * nz + s2 * sl.q[2],
            ]
        } else {
            [nx, ny, nz]
        };

        let (lon, lat) = pal::dcc2s(&v);
        let p = [lon, lat];
        let perm = self.perm;
        Ok([p[perm[0]], p[perm[1]]])
    }

    fn line_crossing(&self, l1: &LineDef, l2: &LineDef) -> AstResult<(bool, [f64; 2], f64)> {
        let sl1 = l1.sky.as_ref().ok_or_else(|| {
            AstError::new(
                ErrorCode::Inter,
                "line_crossing: line 1 is not a SkyFrame line",
            )
        })?;
        let sl2 = l2.sky.as_ref().ok_or_else(|| {
            AstError::new(
                ErrorCode::Inter,
                "line_crossing: line 2 is not a SkyFrame line",
            )
        })?;

        // The two great circles intersect along the direction perpendicular to
        // both poles.
        let temp = pal::dvxv(&sl1.q, &sl2.q);
        let (mut b, _len) = pal::dvn(&temp);

        let mut dist = 0.0;
        let mut result = false;

        // See if the intersection point lies within both arcs; the `dist`
        // side-effect on the `l1` test mirrors C's LineIncludes writing through
        // its `dist` pointer whenever it is called.
        if sky_line_includes(sl2, l2.length, l2.infinite, &b).0 {
            let (r1, t2) = sky_line_includes(sl1, l1.length, l1.infinite, &b);
            dist = t2;
            if r1 {
                result = true;
            }
        }
        if !result {
            b = [-b[0], -b[1], -b[2]];
            if sky_line_includes(sl2, l2.length, l2.infinite, &b).0 {
                let (r1, t2) = sky_line_includes(sl1, l1.length, l1.infinite, &b);
                dist = t2;
                if r1 {
                    result = true;
                }
            }
        }

        let (lon, lat) = pal::dcc2s(&b);
        let p = [lon, lat];
        let perm = self.perm;
        Ok((result, [p[perm[0]], p[perm[1]]], dist))
    }

    fn line_contains(&self, l: &LineDef, point: &[f64]) -> AstResult<bool> {
        let sl = l.sky.as_ref().ok_or_else(|| {
            AstError::new(
                ErrorCode::Inter,
                "line_contains: line is not a SkyFrame line",
            )
        })?;
        if is_bad(point[0]) || is_bad(point[1]) {
            return Ok(false);
        }

        let perm = self.perm;
        let mut p1 = [0.0; 2];
        p1[perm[0]] = point[0];
        p1[perm[1]] = point[1];
        let b = pal::dcs2c(p1[0], p1[1]);

        // The point of closest approach must lie within the arc, and the point
        // must be within a small angle of the great circle (90° from the pole).
        if sky_line_includes(sl, l.length, l.infinite, &b).0 {
            let t1 = pal::dvdv(&sl.q, &b);
            let mut t2 = 1.0e-7 * l.length;
            if t2 < 1.0e-10 {
                t2 = 1.0e-10;
            }
            if t1.abs() <= t2 {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        super::validate_permutation(perm, 2)?;
        let old = self.perm;
        self.perm[0] = old[perm[0] - 1];
        self.perm[1] = old[perm[1] - 1];
        Ok(())
    }

    // Match-control attributes — mirror C `astGet/SetMinAxes` etc.
    // SkyFrame defaults Permute to false (mirrors skyframe.c at the
    // SetPermute line); the others inherit the trait default
    // (`naxes()` for cardinality, `false` for booleans).

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        super::find_frame::integrity_mapping(previous, self)
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        // C Frame equality is conversion-based (frame.c:3549) and SkyFrame
        // does not override it: same class, astConvert succeeds, mapping
        // simplifies to a UnitMap. Two SkyFrames whose System/Epoch/Equinox/
        // SkyRef/NegLon differ produce a non-UnitMap conversion and so
        // compare unequal, exactly as the old attribute checks intended.
        super::find_frame::frames_equal(self, other)
    }

    fn system_string(&self) -> AstResult<Option<&str>> {
        Ok(Some(self.system.as_str()))
    }

    // -----------------------------------------------------------------
    // Match / SubFrame / Overlay (skyframe.c:6604 / 9869 / 8010).
    // -----------------------------------------------------------------

    fn overlay(&self, template_axes: &[i32], result: &mut dyn Frame) -> AstResult<()> {
        skyframe_overlay(self, template_axes, result)
    }

    fn axis_overlay_source(&self, axis: usize) -> Option<super::overlay::AxisOverlaySource> {
        let phys = *self.perm.get(axis.checked_sub(1)?)?;
        Some(super::overlay::AxisOverlaySource(self.axes[phys].clone()))
    }

    fn apply_axis_overlay(&mut self, axis: usize, source: &super::overlay::AxisOverlaySource) {
        if let Some(&phys) = axis.checked_sub(1).and_then(|a| self.perm.get(a)) {
            source.apply_to(&mut self.axes[phys]);
        }
    }

    fn overlay_system_from(&mut self, template: &dyn std::any::Any) -> AstResult<()> {
        if let Some(t) = template.downcast_ref::<SkyFrame>() {
            if t.test_system() {
                self.set_system(t.system());
            }
            if t.test_align_system() {
                self.set_align_system(t.align_system());
            }
        }
        Ok(())
    }

    fn cast(&self, other: &dyn Frame) -> AstResult<Option<DynFrame>> {
        // A specialized leaf frame can only be cast down to a plain
        // Frame (BasicFrame). Same-class is handled by the direct Match
        // (no cast needed), so only the BasicFrame target matters here.
        if other
            .as_any()
            .downcast_ref::<crate::frames::BasicFrame>()
            .is_some()
        {
            Ok(Some(Box::new(crate::frames::cast_to_basic_frame(
                self,
                &self.attrs,
            )?)))
        } else {
            Ok(None)
        }
    }

    fn match_template(
        &self,
        target: &dyn Frame,
        match_sub: bool,
    ) -> AstResult<Option<super::find_frame::MatchResult>> {
        // Mirrors skyframe.c:6604. Step 1: defer to the parent (Frame
        // class) Match for axis-count + Domain pre-checks. We only
        // need the boolean from that call; the returned objects are
        // discarded just as the C does (the axis-association arrays
        // are reconstructed below).
        let parent = super::find_frame::default_match(self, target, match_sub)?;
        if parent.is_none() {
            return Ok(None);
        }

        let target_naxes = target.naxes();

        // Step 2: locate two axes in the target that are derived from
        // a single underlying SkyFrame (in either physical order).
        // Mirrors skyframe.c:6759-6803: walk astPrimaryFrame on each
        // target axis and take the first whose primary Frame is a
        // SkyFrame, then search the remaining axes for one from that
        // same SkyFrame, passing over the axes of any other. The two
        // axes need not be adjacent in the target's external order.
        let mut target_sky_owned: Option<&SkyFrame> = None;
        let mut target_axis0: Option<usize> = None;
        let mut target_axis1: Option<usize> = None;
        let mut iaxis0: usize = 0;
        let mut iaxis1: usize = 0;
        for iaxis in 0..target_naxes {
            let (primary, local_axis) = crate::frames::specframe::primary_frame_walk(target, iaxis);
            if let Some(sky) = primary.as_any().downcast_ref::<SkyFrame>() {
                if target_axis0.is_none() {
                    target_sky_owned = Some(sky);
                    target_axis0 = Some(iaxis);
                    iaxis0 = local_axis;
                } else {
                    if !std::ptr::eq(target_sky_owned.unwrap() as *const _, sky as *const _) {
                        continue;
                    }
                    target_axis1 = Some(iaxis);
                    iaxis1 = local_axis;
                    break;
                }
            }
        }
        let (Some(target_axis0), Some(target_axis1), Some(_target_sky)) =
            (target_axis0, target_axis1, target_sky_owned)
        else {
            return Ok(None);
        };
        // Sanity check: the underlying primary axes must be 0 and 1
        // in either order (skyframe.c:6800-6802).
        if !((iaxis0 == 0 && iaxis1 == 1) || (iaxis0 == 1 && iaxis1 == 0)) {
            return Ok(None);
        }

        // Step 3: axis-swap detection (skyframe.c:6813-6827).
        // swap1: does the template's permutation reverse axis 0?
        let swap1 = self.perm[0] != 0;
        // swap2: are the underlying primary axes reversed in the target?
        let swap2 = iaxis0 != 0;
        let swap = swap1 != swap2;
        if swap && !self.permute() {
            return Ok(None);
        }

        // Step 4: build the association arrays (skyframe.c:6831-6849).
        let preserve_axes = self.preserve_axes();
        let (template_axes_vec, target_axes_vec) = if preserve_axes {
            (
                vec![if swap { 1 } else { 0 }, if swap { 0 } else { 1 }],
                vec![target_axis0 as i32, target_axis1 as i32],
            )
        } else {
            (
                vec![0i32, 1],
                if swap {
                    vec![target_axis1 as i32, target_axis0 as i32]
                } else {
                    vec![target_axis0 as i32, target_axis1 as i32]
                },
            )
        };

        // Step 5: defer to the target's SubFrame to build the result
        // and Mapping (skyframe.c:6856-6857).
        let Some((map, result)) =
            target.sub_frame(Some(self), 2, &target_axes_vec, &template_axes_vec)?
        else {
            return Ok(None);
        };
        Ok(Some(super::find_frame::MatchResult {
            template_axes: template_axes_vec,
            target_axes: target_axes_vec,
            map,
            result,
        }))
    }

    fn sub_frame(
        &self,
        template: Option<&dyn Frame>,
        result_naxes: usize,
        target_axes: &[i32],
        template_axes: &[i32],
    ) -> AstResult<Option<(DynMapping, DynFrame)>> {
        skyframe_sub_frame(self, template, result_naxes, target_axes, template_axes)
    }

    fn pick_axes_snapshot(&self, target_axes: &[i32]) -> AstResult<DynFrame> {
        // Whole-frame extraction: a copy with the requested axis
        // permutation (mirrors the SkyFrame branch of skyframe.c
        // SubFrame at line 10029).
        if target_axes.len() == 2
            && ((target_axes[0] == 0 && target_axes[1] == 1)
                || (target_axes[0] == 1 && target_axes[1] == 0))
        {
            let mut snap = self.clone();
            // Apply axis permutation: target_axes is zero-based.
            if target_axes[0] != 0 {
                let _ = snap.perm_axes(&[2, 1]);
            }
            return Ok(Box::new(snap));
        }

        // Partial selection: a plain Frame holding the picked SkyAxis
        // objects. Mirrors skyframe.c SubFrame (skyframe.c:10230): on a copy
        // of the SkyFrame it sets Domain and Title, and on each axis the
        // AsTime, Format, Label, Symbol and Unit whose defaults the SkyFrame
        // class supplies, then IsLatitude and Centerzero on the SkyAxis
        // (skyframe.c:10280), and clears System and AlignSystem. The Frame
        // class's SubFrame then attaches those Axis objects to a new Frame
        // and overlays the copy's Frame attributes onto it (frame.c:11023).
        // The Mapping is built by the caller (default_sub_frame).
        let mut temp = self.clone();
        if !temp.test_domain() {
            let domain = temp.domain().to_string();
            temp.attrs.domain = Some(domain);
        }
        if !Frame::test_title(&temp) {
            temp.attrs.title = Some(Frame::title(&temp)?);
        }
        let mut bf = super::basic::BasicFrameBuilder::new(target_axes.len()).build()?;
        bf.overlay_frame_attrs(&temp.attrs);
        bf.set_active_unit(false);
        for (i, &t) in target_axes.iter().enumerate() {
            if t < 0 || (t as usize) >= 2 {
                continue;
            }
            let ext = t as usize + 1;
            let phys = temp.perm[t as usize];
            // C sets each attribute the SkyFrame gives a default to, in the
            // order AsTime, Format, Label, Symbol, Unit (skyframe.c:10260).
            // Setting AsTime makes a plain Axis a SkyAxis (`SetAsTime`), so
            // the Format that follows is the SkyAxis one.
            let as_time = temp.effective_as_time(phys);
            temp.sky_axis_mut(phys).as_time.get_or_insert(as_time);
            let format = temp.phys_format(phys);
            let label = Frame::label(&temp, ext)?;
            let symbol = Frame::symbol(&temp, ext)?;
            let mut ax = temp.sky_axis_mut(phys).clone();
            ax.format.get_or_insert(format);
            ax.axis.label.get_or_insert(label);
            ax.axis.symbol.get_or_insert(symbol);
            let unit = super::skyaxis::SkyAxis::unit_value(&ax, temp.digits());
            ax.axis.unit.get_or_insert(unit);
            let latitude = phys == 1;
            ax.is_latitude = Some(latitude);
            ax.centre_zero = Some(latitude || temp.neg_lon());
            bf.set_axis_object(i + 1, super::axis::FrameAxis::Sky(ax));
        }
        Ok(Box::new(bf))
    }
}

// ---------------------------------------------------------------------------
// Match / SubFrame / Overlay helpers
// ---------------------------------------------------------------------------

/// Compute Local Apparent Sidereal Time, in radians, at the given
/// epoch and observer location.
///
/// Mirrors `skyframe.c::CalcLAST`: builds a TDB-MJD TimeFrame and a
/// LAST-MJD TimeFrame, applies the observer parameters to both,
/// Format an epoch expressed in decimal years, mirroring C's
/// `astFmtDecimalYr(year, 9)` (frame.c:4189): `sprintf("%#.9g")` followed by
/// trailing-zero removal, but keeping one zero after the decimal point so the
/// result always reads like a year (e.g. "2000.0", "1999.80005"). The `#`
/// flag forces a decimal point; `format_double_g` already strips trailing
/// zeros and the point, so we re-append ".0" when no point remains.
fn fmt_decimal_yr(year: f64) -> String {
    let s = crate::serial::native_write::format_double_g(year, 9);
    if s.contains('.') || s.contains('e') {
        s
    } else {
        format!("{s}.0")
    }
}

/// Determine whether a great-circle segment includes a point known to lie on
/// its great circle. Returns `(included, dist)` where `dist` is the distance in
/// radians from the line's start to the point, measured forwards along the
/// line. The end point is not part of the segment. Mirrors C `LineIncludes`
/// (skyframe.c:5659).
fn sky_line_includes(
    sl: &SkyLineDef,
    length: f64,
    infinite: bool,
    point: &[f64; 3],
) -> (bool, f64) {
    // Unsigned distance of the point from the start, 0 - 180 degs.
    let t1 = pal::dvdv(&sl.start, point);
    let mut t2 = crate::math::acos(t1);

    // If the point is more than 90 degrees from the quarter point it is in the
    // reverse half of the great circle; convert to the forward distance.
    let t3 = pal::dvdv(&sl.dir, point);
    if t3 < -1.0e-10 {
        t2 = 2.0 * std::f64::consts::PI - t2;
    }

    let result = if infinite {
        true
    } else if length > 0.0 {
        t2 < length
    } else {
        t2 == 0.0
    };
    (result, t2)
}

/// C's SkyFrame `Overlay` (skyframe.c:8010), a method of the template.
///
/// When `result` is a SkyFrame whose System would change, or whose SkyRef
/// differs, its Title, and if the change crosses between equatorial and
/// other systems its axis attributes, are cleared first so the new system's
/// defaults apply. The Frame-class part follows, which sets the System on a
/// SkyFrame result only, and then the SkyFrame attributes.
fn skyframe_overlay(
    template_sky: &SkyFrame,
    template_axes: &[i32],
    result: &mut dyn Frame,
) -> AstResult<()> {
    if let Some(result_sky) = result.as_any_mut().downcast_mut::<SkyFrame>() {
        let old_system = result_sky.system;
        let new_system = template_sky.system;
        let skyref_changed = template_sky.sky_ref[0] != result_sky.sky_ref[0]
            || template_sky.sky_ref[1] != result_sky.sky_ref[1];

        // If the System will change (and is set on the template), or
        // SkyRef differs, the result Frame's Title is cleared
        // (skyframe.c:8117-8120).
        if (new_system != old_system && template_sky.test_system()) || skyref_changed {
            result_sky.clear_title();

            // If the equatorial-ness changes, clear per-axis overrides
            // so the new system's defaults take over (skyframe.c:
            // 8124-8140).
            if is_equatorial(new_system) != is_equatorial(old_system) || skyref_changed {
                // C clears AsTime, Direction, Format, Label, Symbol and
                // Unit, and keeps Digits.
                for axis in 0..2 {
                    let ax = &mut result_sky.axes[axis];
                    let attrs = ax.axis_mut();
                    attrs.label = None;
                    attrs.symbol = None;
                    attrs.direction = None;
                    attrs.unit = None;
                    *ax.format_slot_mut() = None;
                    if let FrameAxis::Sky(sky) = ax {
                        sky.as_time = None;
                    }
                }
            }
        }
    }

    super::overlay::frame_overlay(template_sky, template_axes, result)?;

    if let Some(result_sky) = result.as_any_mut().downcast_mut::<SkyFrame>() {
        // SkyFrame-specific OVERLAY block (skyframe.c:8198-8205).
        if template_sky.test_equinox() {
            result_sky.set_equinox_mjd(template_sky.equinox_mjd());
        }
        if template_sky.test_projection() {
            if let Some(p) = template_sky.projection() {
                result_sky.set_projection(p);
            }
        }
        if template_sky.test_neg_lon() {
            result_sky.set_neg_lon(template_sky.neg_lon());
        }
        if template_sky.test_sky_tol() {
            result_sky.set_sky_tol(template_sky.sky_tol());
        }
        if template_sky.test_align_offset() {
            result_sky.set_align_offset(template_sky.align_offset());
        }
        if template_sky.test_sky_ref_is() {
            result_sky.set_sky_ref_is(template_sky.sky_ref_is());
        }

        // OVERLAY2 macro: per-axis SkyRef / SkyRefP entries indexed
        // by template_axes (skyframe.c:8204-8205).
        let (tax0, tax1) = if !template_axes.is_empty() {
            (
                template_axes[0],
                if template_axes.len() > 1 {
                    template_axes[1]
                } else {
                    -1
                },
            )
        } else {
            (0, 1)
        };
        // C addresses both Frames by external axis: template axis `tax0`
        // or `tax1` onto result axis 1 or 2 (0-based in C).
        let template_axis = |t: i32| (0..2).contains(&t).then(|| t as usize + 1);
        for (result_axis, t) in [(1, tax0), (2, tax1)] {
            if let Some(t) = template_axis(t) {
                if template_sky.test_sky_ref(t)? {
                    result_sky.set_sky_ref(result_axis, template_sky.sky_ref(t)?)?;
                }
            }
        }
        for (result_axis, t) in [(1, tax0), (2, tax1)] {
            if let Some(t) = template_axis(t) {
                if template_sky.test_sky_ref_p(t)? {
                    result_sky.set_sky_ref_p(result_axis, template_sky.sky_ref_p(t)?)?;
                }
            }
        }
    }
    Ok(())
}

/// Check that `target` (for `which` below 3) and `result` (for `which`
/// above 1) have values set for the attributes in `attrs` that a conversion
/// step needs, where its UseDefs is false. Port of `VerifyMSMAttrs`
/// (skyframe.c:11092), which also clears both Titles before reporting;
/// the port leaves the caller's Frames unchanged.
fn verify_msm_attrs(target: &SkyFrame, result: &SkyFrame, which: u8, attrs: &str) -> AstResult<()> {
    let (usedef1, usedef2) = (target.use_defs(), result.use_defs());
    if usedef1 && usedef2 {
        return Ok(());
    }
    for attr in attrs.split_whitespace() {
        let (set1, set2, desc) = match attr {
            "Equinox" => (
                target.test_equinox(),
                result.test_equinox(),
                "reference equinox",
            ),
            "Epoch" => (
                target.test_epoch(),
                result.test_epoch(),
                "epoch of observation",
            ),
            "ObsLon" => (
                Frame::test_obs_lon(target),
                Frame::test_obs_lon(result),
                "longitude of observer",
            ),
            "ObsLat" => (
                Frame::test_obs_lat(target),
                Frame::test_obs_lat(result),
                "latitude of observer",
            ),
            other => unreachable!("VerifyMSMAttrs has no attribute {other}"),
        };
        let side = if !usedef1 && !set1 && which < 3 {
            "input"
        } else if !usedef2 && !set2 && which > 1 {
            "output"
        } else {
            continue;
        };
        return Err(AstError::new(
            ErrorCode::Noval,
            format!(
                "astMatch(SkyFrame): Cannot convert celestial coordinates from {} to {}. No \
                 value has been set for the \"{attr}\" attribute ({desc}) in the {side} SkyFrame.",
                target.default_title(),
                result.default_title()
            ),
        ));
    }
    Ok(())
}

/// The Mapping from `target` to `result` that `MakeSkyMapping`
/// (skyframe.c:5956) returns: the conversion chain, replaced by a UnitMap
/// when its effect is below the SkyTol of both frames, between the
/// offset-system Mappings of the two frames. `None` when no conversion
/// exists.
fn make_sky_frame_mapping(
    target: &SkyFrame,
    result: &SkyFrame,
    align_sys: SkySystem,
    exempt: bool,
) -> AstResult<Option<DynMapping>> {
    // Two UNKNOWN systems are assumed to be the same, and C returns its
    // UnitMap before forming the offset-system Mappings (skyframe.c:6077),
    // so that a change such as a new Title keeps an UNKNOWN FrameSet intact.
    if target.system == SkySystem::Unknown && result.system == SkySystem::Unknown {
        return Ok(Some(Box::new(UnitMap::new(2))));
    }
    // C obtains the LAST and diurnal aberration of both frames for every
    // conversion, whether or not a step uses them, the target's LAST first
    // (skyframe.c:6177, :6416); computing a LAST adds it to the table of
    // LAST values that later conversions draw on.
    let target_last = target.get_last()?;
    let target_diurab = target.get_diurab();
    let result_diurab = result.get_diurab();
    let result_last = result.get_last()?;
    let verify = |which: u8, attrs: &str| {
        if exempt {
            Ok(())
        } else {
            verify_msm_attrs(target, result, which, attrs)
        }
    };
    let Some(mut map) = crate::frames::sky_convert::make_sky_mapping_verified(
        target.system,
        target.equinox_mjd(),
        target.epoch_mjd(),
        result.system,
        result.equinox_mjd(),
        result.epoch_mjd(),
        align_sys,
        target.attrs.obs_lat.unwrap_or(0.0),
        target_last,
        target_diurab,
        result.attrs.obs_lat.unwrap_or(0.0),
        result_last,
        result_diurab,
        &verify,
    )?
    else {
        return Ok(None);
    };

    // Replace the system-conversion chain by a UnitMap when its
    // effect everywhere on the sphere is below the frames' SkyTol
    // (skyframe.c:6546 TestSlaUnit).
    if crate::frames::sky_convert::is_effectively_unit(
        map.as_ref(),
        target.sky_tol().min(result.sky_tol()),
    ) {
        map = Box::new(UnitMap::new(2));
    }

    // Prepend source offset removal (offset→standard).
    if let Some(off_map) = make_offset_mapping(
        target.sky_ref,
        target.sky_ref_p_lon_lat(),
        target.sky_ref_is,
        target.sky_ref_set[0] || target.sky_ref_set[1],
    )? {
        map = Box::new(CmpMap::new(off_map, map, Composition::Series)?);
    }

    // Append target offset application (standard→offset = inverse).
    if let Some(mut off_map) = make_offset_mapping(
        result.sky_ref,
        result.sky_ref_p_lon_lat(),
        result.sky_ref_is,
        result.sky_ref_set[0] || result.sky_ref_set[1],
    )? {
        off_map.invert();
        map = Box::new(CmpMap::new(map, off_map, Composition::Series)?);
    }
    Ok(Some(map))
}

/// Returns true if `system` is one of the equatorial SkyFrame systems
/// (FK4, FK4-no-E, FK5, ICRS, J2000, GAPPT). Mirrors C `IsEquatorial`
/// (skyframe.c).
fn is_equatorial(system: SkySystem) -> bool {
    matches!(
        system,
        SkySystem::Fk4
            | SkySystem::Fk4NoE
            | SkySystem::Fk5
            | SkySystem::Icrs
            | SkySystem::J2000
            | SkySystem::Gappt
    )
}

/// Mirrors C `SubFrame` for SkyFrame (skyframe.c:9869).
fn skyframe_sub_frame(
    target: &SkyFrame,
    template: Option<&dyn Frame>,
    result_naxes: usize,
    target_axes: &[i32],
    template_axes: &[i32],
) -> AstResult<Option<(DynMapping, DynFrame)>> {
    // Result-is-a-SkyFrame branch (skyframe.c:10003-10117).
    if result_naxes == 2
        && target_axes.len() == 2
        && ((target_axes[0] == 0 && target_axes[1] == 1)
            || (target_axes[0] == 1 && target_axes[1] == 0))
    {
        // Picking axes, with no template or the target as its own, needs
        // no attribute checks: C clears a false UseDefs on the target for
        // the conversion and sets it to 0 again on the target and the
        // result afterwards (skyframe.c:10125, :10185).
        let exempt = template
            .is_none_or(|t| std::ptr::addr_eq(t as *const dyn Frame, target as *const SkyFrame));
        // Form the result from a copy of the target (skyframe.c:10029)
        // and apply the requested axis permutation.
        let mut result = target.clone();
        if target_axes[0] != 0 {
            result.perm_axes(&[2, 1])?;
        }

        // If a template was supplied, overlay it onto the result and
        // pick the alignment system from the template
        // (skyframe.c:10036-10042). We call the overlay helper
        // directly on `&mut result` to avoid round-tripping through a
        // DynFrame box.
        let align_sys = if let Some(t) = template {
            t.overlay(template_axes, &mut result)?;
            if let Some(template_sky) = t.as_any().downcast_ref::<SkyFrame>() {
                template_sky.align_system()
            } else {
                target.align_system()
            }
        } else {
            target.align_system()
        };

        // Two SkyFrames that both align in offset coordinates are
        // connected by a UnitMap, except while restoring a FrameSet's
        // integrity, when the offsets must follow the new System
        // (skyframe.c:10163).
        let mut aligned_in_offsets = false;
        if target.frame_flags() & super::INTFLAG == 0
            && target.align_offset()
            && target.sky_ref_is() != SkyRefIs::Ignored
            && let Some(t) = template.and_then(|t| t.as_any().downcast_ref::<SkyFrame>())
        {
            aligned_in_offsets = t.align_offset() && t.sky_ref_is() != SkyRefIs::Ignored;
        }

        // Otherwise build the Mapping that takes account of changes
        // between the target and result systems (skyframe.c:10179).
        let mut map: DynMapping = if aligned_in_offsets {
            Box::new(UnitMap::new(2))
        } else {
            match make_sky_frame_mapping(target, &result, align_sys, exempt)? {
                Some(map) => map,
                None => return Ok(None),
            }
        };

        // Coordinate-order swaps (skyframe.c:10082-10116). The
        // Mapping built above operates in (longitude, latitude) order
        // — if either the target or the result presents axes in a
        // different external order, prepend / append a 2×2 PermMap.
        // target_swap: axis-0 of the target points at latitude.
        let target_swap = target.perm[0] != 0;
        // result_swap: was the target presented in a different order
        // than the result requires? When target_axes selects axis 1
        // first, the resulting coordinate pair flows through
        // longitude, latitude; result_swap mirrors the C expression
        // `target_swap != (target_axes[0] != 0)`.
        let result_swap = target_swap != (target_axes[0] != 0);
        if target_swap || result_swap {
            let perm_inperm: [i64; 2] = [2, 1];
            let perm_outperm: [i64; 2] = [2, 1];
            let permmap = crate::mappings::PermMap::new(&perm_inperm, &perm_outperm, &[])?;
            if target_swap {
                let pm = Box::new(permmap.clone());
                map = Box::new(CmpMap::new(pm, map, Composition::Series)?);
            }
            if result_swap {
                let pm = Box::new(permmap);
                map = Box::new(CmpMap::new(map, pm, Composition::Series)?);
            }
        }

        let result_box: DynFrame = Box::new(result);
        return Ok(Some((map, result_box)));
    }

    // Result is not a SkyFrame (skyframe.c:10128-10199). Build a
    // BasicFrame snapshot whose per-axis attributes follow the picked
    // SkyFrame axes, then defer to the default Frame-level
    // SubFrame to construct the Mapping.
    super::find_frame::default_sub_frame(target, template, target_axes, template_axes)
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

impl Serializable for SkyFrame {
    fn class_name(&self) -> &'static str {
        "SkyFrame"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Description of celestial coordinate system".to_string());

        // --- Object-level attributes (before IsA Object) ---
        // Ident: written if explicitly set.
        super::frame_attrs::write_object_part(
            &mut node,
            &self.attrs.id,
            self.attrs.ident.as_deref(),
            self.attrs.use_defs,
            crate::frames::Frame::use_defs(self),
        );

        // The Frame class's part, read through the SkyFrame's getters
        // (frame.c:14250).
        super::frame_attrs::write_frame_cards(
            &mut node,
            &super::frame_attrs::TraitDump {
                frame: self,
                attrs: &self.attrs,
                system: (self.system_set, self.system.dump_str().to_string()),
                align_system: (
                    self.align_system.is_some(),
                    self.align_system().dump_str().to_string(),
                ),
                perm: self.perm.to_vec(),
                is_frame: false,
                own_naxes: 2,
                mapping: None,
            },
        )?;

        // Per-axis SkyAxis children, in internal order (0 = lon, 1 = lat).
        // An explicit Label or Symbol round-trips as an active card inside
        // the Axis sub-object, while the Frame-level `Lbl{n}` aggregate
        // stays commented, as C's Axis class Dump writes them (axis.c).
        for (i, axis) in self.axes.iter().enumerate() {
            let axis_node = axis.to_dump_node();
            let key = format!("Ax{}", i + 1);
            let comment = format!("Axis number {}", i + 1);
            node.push_full_capped_object(&key, axis_node, Some(comment));
        }

        // IsA Frame marker.
        node.push_isa("Frame", Some("Coordinate system description".to_string()));

        // --- SkyFrame-specific attributes (after IsA Frame) ---

        // skyframe.c `Dump`: each value active when set, otherwise the
        // default its getter gives.
        node.write_string(
            "Proj",
            self.projection.is_some(),
            false,
            self.projection().unwrap_or(""),
            "Description of sky projection",
        );
        let neglon = self.neg_lon();
        node.write_int(
            "NegLon",
            self.neglon_set,
            false,
            i64::from(neglon),
            if neglon {
                "Display negative longitude values"
            } else {
                "Display positive longitude values"
            },
        );
        node.write_double(
            "SkyTol",
            self.sky_tol.is_some(),
            true,
            self.sky_tol(),
            "Smallest significant separation [arc-sec]",
        );

        // The Equinox as a Besselian epoch before 1984 and a Julian one
        // after, helpful only for the systems that use it.
        let eq_helpful = matches!(
            self.system,
            SkySystem::Fk4 | SkySystem::Fk4NoE | SkySystem::Fk5 | SkySystem::Ecliptic
        );
        let eqnox_mjd = self.equinox_mjd();
        let bessyr = eqnox_mjd < pal::epj2d(1984.0);
        node.write_double(
            "Eqnox",
            self.equinox.is_some(),
            eq_helpful,
            if bessyr {
                pal::epb(eqnox_mjd)
            } else {
                pal::epj(eqnox_mjd)
            },
            if bessyr {
                "Besselian epoch of mean equinox"
            } else {
                "Julian epoch of mean equinox"
            },
        );

        let (srefis, comment) = match self.sky_ref_is {
            SkyRefIs::Pole => ("Pole", "Rotated to put pole at ref. pos."),
            SkyRefIs::Ignored => ("Ignored", "Not rotated (ref. pos. is ignored)"),
            SkyRefIs::Origin => ("Origin", "Rotated to put origin at ref. pos."),
        };
        node.write_string("SRefIs", self.sky_ref_is_set, false, srefis, comment);

        // SRef{axis_p+1} and SRefP{axis_p+1} hold the values of internal
        // axis `axis_p`, with the Symbol and Format of the external axis it
        // appears as (skyframe.c `Dump`).
        let invperm = [self.lon_pos(), self.lat_pos()]; // invperm[internal] = external (0-based)
        for (axis_p, &ext0) in invperm.iter().enumerate() {
            let ext_axis = ext0 + 1;
            let dval = self.sky_ref(ext_axis)?;
            let symbol = self.symbol(ext_axis).unwrap_or_default();
            let fmt = self.format(ext_axis, dval).unwrap_or_default();
            node.write_double(
                &format!("SRef{}", axis_p + 1),
                self.sky_ref_set[axis_p],
                false,
                dval,
                &format!("Ref. pos. {symbol} {fmt}"),
            );
        }
        for (axis_p, &ext0) in invperm.iter().enumerate() {
            let ext_axis = ext0 + 1;
            let dval = self.sky_ref_p(ext_axis)?;
            let symbol = self.symbol(ext_axis).unwrap_or_default();
            let fmt = self.format(ext_axis, dval).unwrap_or_default();
            node.write_double(
                &format!("SRefP{}", axis_p + 1),
                self.sky_ref_p_set[axis_p],
                false,
                dval,
                &format!("Ref. north {symbol} {fmt}"),
            );
        }

        let aloff = self.align_offset();
        node.write_int(
            "AlOff",
            self.align_offset_set,
            false,
            i64::from(aloff),
            if aloff {
                "Align in offset coords"
            } else {
                "Align in system coords"
            },
        );

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let frame = &whole.section("Frame");
        let node = &whole.section("SkyFrame");

        // System is a Frame item. A SkyFrame section may also carry one,
        // which skyframe.c astLoadSkyFrame reads only when the Frame part
        // left System unset, so that dumps from older versions of AST load.
        let read =
            super::frame_attrs::read_system_item(frame, "System", "System", SkySystem::parse)?;
        let read = match read {
            Some(system) => Some(system),
            None => {
                super::frame_attrs::read_system_item(node, "System", "System", SkySystem::parse)?
            }
        };
        // Default is ICRS to match the C library (`skyframe.c`: AST__ICRS is
        // the default System).
        let mut sky = SkyFrame::new(read.unwrap_or(SkySystem::Icrs));
        // Set only when the dump has an active System item.
        sky.system_set = read.is_some();
        sky.attrs = super::frame_attrs::read_frame_attrs(whole);

        // Epoch, under the same older-version rule as System. The value is
        // a year, Besselian before 1984 and Julian from then on.
        if sky.attrs.epoch.is_none() && node.get("Epoch").is_some() {
            sky.set_epoch_year(node.get_double("Epoch", 0.0));
        }

        // Equinox is stored as a decimal year and resolved to an MJD with
        // the IAU 1984 rule, as astReadDateTime does.
        if node.get("Eqnox").is_some() {
            sky.set_equinox_year(node.get_double("Eqnox", 0.0));
        }

        // SkyFrame-specific.
        let proj = node.get_string("Proj", "");
        if !proj.is_empty() {
            sky.projection = Some(proj);
        }

        if node.get("NegLon").is_some() {
            sky.neglon = node.get_int("NegLon", 0) != 0;
            sky.neglon_set = true;
        }

        if node.get("SkyTol").is_some() {
            sky.sky_tol = Some(node.get_double("SkyTol", 0.0));
        }

        // SRefIs: skyframe.c `astLoadSkyFrame` matches the value with
        // `astChrMatch`, leaves a blank one unset, and reports any other as
        // AST__INTER.
        if node.get("SRefIs").is_some() {
            let srefis_str = node.get_string("SRefIs", " ");
            if let Some(mode) = SkyRefIs::parse(&srefis_str) {
                sky.sky_ref_is = mode;
                sky.sky_ref_is_set = true;
            } else if !crate::mappings::helpers::chr_match_key(&srefis_str).is_empty() {
                return Err(AstError::new(
                    ErrorCode::Inter,
                    format!(
                        "astRead(SkyFrame): Corrupt SkyFrame contains invalid SkyRefIs \
                         attribute value ({srefis_str})."
                    ),
                ));
            }
        }

        // SRef1, SRef2.
        if node.get("SRef1").is_some() {
            sky.sky_ref[0] = node.get_double("SRef1", 0.0);
            sky.sky_ref_set[0] = true;
        }
        if node.get("SRef2").is_some() {
            sky.sky_ref[1] = node.get_double("SRef2", 0.0);
            sky.sky_ref_set[1] = true;
        }

        // SRefP1, SRefP2.
        if node.get("SRefP1").is_some() {
            sky.sky_ref_p[0] = node.get_double("SRefP1", 0.0);
            sky.sky_ref_p_set[0] = true;
        }
        if node.get("SRefP2").is_some() {
            sky.sky_ref_p[1] = node.get_double("SRefP2", 0.0);
            sky.sky_ref_p_set[1] = true;
        }

        if node.get("AlOff").is_some() {
            sky.align_offset = node.get_int("AlOff", 0) != 0;
            sky.align_offset_set = true;
        }

        // AlSys (alignment system).
        if let Some(system) =
            super::frame_attrs::read_system_item(frame, "AlSys", "AlignSystem", SkySystem::parse)?
        {
            sky.align_system = Some(system);
        }

        // Read SkyAxis children. Axes are stored in internal (physical)
        // order (0 = lon, 1 = lat).
        for i in 0..2 {
            let key = format!("Ax{}", i + 1);
            // An Axis object of either class; with none, astLoadFrame
            // supplies a plain Axis (frame.c:15227).
            sky.axes[i] = match frame.get_object(&key) {
                Some(axis_node) => FrameAxis::read(axis_node)?,
                None => FrameAxis::Plain(super::axis::Axis::default()),
            };
            sky.axes[i].apply_frame_axis_keys(frame, i + 1);
        }

        // Axis permutation `Prm{n}` (1-based internal axis for each
        // external axis). Mirrors C `Frame::Dump`/load. Apply last so it
        // is not clobbered by earlier per-axis state, and only when it
        // differs from the identity. `perm_axes` takes a 1-based
        // external→internal mapping, which is exactly the `Prm{n}` values.
        let prm: Vec<usize> = (1..=2)
            .map(|n| frame.get_int(&format!("Prm{n}"), n as i64) as usize)
            .collect();
        if prm != [1, 2] {
            Frame::perm_axes(&mut sky, &prm)?;
        }

        Ok(sky)
    }
}

/// Calculate the reference vectors used by `Offset2`. Mirrors C `Shcal`
/// (skyframe.c:9766): returns `(r0, r3)` where `r0` is the 3-vector of the
/// sky position `(a0, b0)` and `r3` is the 3-vector 90 degrees away along
/// the great circle passing through `r0` at position angle `angle`
/// (measured from north to east). The XY plane is the equator, Z is north,
/// X towards longitude 0, Y towards longitude 90 degrees.
fn shcal(a0: f64, b0: f64, angle: f64) -> ([f64; 3], [f64; 3]) {
    let sina0 = crate::math::sin(a0);
    let cosa0 = crate::math::cos(a0);
    let sinb0 = crate::math::sin(b0);
    let cosb0 = crate::math::cos(b0);
    let sinpa = crate::math::sin(angle);
    let cospa = crate::math::cos(angle);

    // R0: the given point.
    let r0 = [cosb0 * cosa0, cosb0 * sina0, sinb0];

    // R1: the point in the meridian of R0, 90 deg further north.
    let r1 = [-sinb0 * cosa0, -sinb0 * sina0, cosb0];

    // R2: the point on the equator, 90 deg west of R0.
    let r2 = [-sina0, cosa0, 0.0];

    // R3: the point 90 deg away from R0 along the required great circle.
    let r3 = [
        cospa * r1[0] + sinpa * r2[0],
        cospa * r1[1] + sinpa * r2[1],
        cospa * r1[2] + sinpa * r2[2],
    ];

    (r0, r3)
}

/// Use the vectors from `shcal` to find the sky position offset by `dist`
/// along the great circle. Mirrors C `Shapp` (skyframe.c:9680). Returns
/// `[lon, lat]` in radians; a point at a pole is assigned the reference
/// longitude `a0`.
fn shapp(dist: f64, r0: &[f64; 3], r3: &[f64; 3], a0: f64) -> [f64; 2] {
    let sindst = crate::math::sin(dist);
    let cosdst = crate::math::cos(dist);

    // R4: the required position, a linear sum of R0 and R3.
    let r4 = [
        cosdst * r0[0] + sindst * r3[0],
        cosdst * r0[1] + sindst * r3[1],
        cosdst * r0[2] + sindst * r3[2],
    ];

    let lon = if r4[0] != 0.0 || r4[1] != 0.0 {
        crate::math::atan2(r4[1], r4[0])
    } else {
        a0
    };

    let z = r4[2].clamp(-1.0, 1.0);
    let lat = crate::math::asin(z);

    [lon, lat]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::FrameSearch;
    use crate::mappings::Mapping;
    use std::f64::consts::{FRAC_PI_2, PI};

    const TOL: f64 = 1.0e-10;

    fn assert_close(a: f64, b: f64, tol: f64) {
        assert!(
            (a - b).abs() < tol,
            "expected {a} ~= {b} (diff = {:.2e})",
            (a - b).abs()
        );
    }

    // -----------------------------------------------------------------------
    // Construction and defaults
    // -----------------------------------------------------------------------

    #[test]
    fn new_defaults() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        assert_eq!(sky.naxes(), 2);
        assert_eq!(sky.system(), SkySystem::Fk5);
        assert_eq!(sky.domain(), "SKY");
        assert_close(sky.epoch_mjd(), 51544.5, TOL);
        assert_close(sky.equinox_mjd(), 51544.5, TOL);
        assert!(!sky.neg_lon());
        assert_eq!(sky.sky_ref_is(), SkyRefIs::Ignored);
        assert!(!sky.align_offset());
        assert_close(sky.sky_ref_lon_lat()[0], 0.0, TOL);
        assert_close(sky.sky_ref_lon_lat()[1], 0.0, TOL);
        assert_close(sky.sky_ref_p_lon_lat()[0], 0.0, TOL);
        assert_close(sky.sky_ref_p_lon_lat()[1], FRAC_PI_2, TOL);
        assert_close(sky.sky_tol(), 0.001, TOL);
        assert!(sky.projection().is_none());
        // The default C reports (skyframe.c `GetTitle`).
        assert!(!sky.test_title());
        assert_eq!(
            sky.title().unwrap(),
            "FK5 equatorial coordinates; mean equinox J2000.0"
        );
    }

    #[test]
    fn fk4_default_equinox() {
        let sky = SkyFrame::new(SkySystem::Fk4);
        let b1950_mjd = pal::epb2d(1950.0);
        assert_close(sky.equinox_mjd(), b1950_mjd, 1.0e-6);
    }

    // -----------------------------------------------------------------------
    // Attribute setters
    // -----------------------------------------------------------------------

    #[test]
    fn set_system() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_system(SkySystem::Galactic);
        assert_eq!(sky.system(), SkySystem::Galactic);
    }

    #[test]
    fn set_epoch() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_epoch_mjd(53000.0);
        assert_close(sky.epoch_mjd(), 53000.0, TOL);
    }

    #[test]
    fn set_equinox() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_equinox_mjd(53000.0);
        assert_close(sky.equinox_mjd(), 53000.0, TOL);
    }

    #[test]
    fn set_neg_lon() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_neg_lon(true);
        assert!(sky.neg_lon());
    }

    #[test]
    fn set_sky_ref() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_sky_ref_lon_lat(1.0, 0.5);
        assert_close(sky.sky_ref_lon_lat()[0], 1.0, TOL);
        assert_close(sky.sky_ref_lon_lat()[1], 0.5, TOL);
    }

    #[test]
    fn set_sky_ref_is() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_sky_ref_is(SkyRefIs::Origin);
        assert_eq!(sky.sky_ref_is(), SkyRefIs::Origin);
    }

    #[test]
    fn set_sky_ref_p() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_sky_ref_p_lon_lat(1.0, 0.5);
        assert_close(sky.sky_ref_p_lon_lat()[0], 1.0, TOL);
        assert_close(sky.sky_ref_p_lon_lat()[1], 0.5, TOL);
    }

    #[test]
    fn set_sky_tol() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_sky_tol(1.0e-6);
        assert_close(sky.sky_tol(), 1.0e-6, TOL);
    }

    #[test]
    fn set_align_offset() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_align_offset(true);
        assert!(sky.align_offset());
    }

    #[test]
    fn set_projection() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_projection("TAN");
        assert_eq!(sky.projection(), Some("TAN"));
    }

    #[test]
    fn set_observer() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_obs_lat(0.5);
        sky.set_obs_lon(1.0);
        sky.set_obs_alt(4200.0);
        sky.set_dtai(37.0);
        sky.set_dut1(0.3);
        assert_close(
            {
                assert!(sky.test_obs_lat());
                sky.obs_lat()
            },
            0.5,
            TOL,
        );
        assert_close(
            {
                assert!(sky.test_obs_lon());
                sky.obs_lon()
            },
            1.0,
            TOL,
        );
        assert_close(
            {
                assert!(sky.test_obs_alt());
                sky.obs_alt()
            },
            4200.0,
            TOL,
        );
        assert_close(
            {
                assert!(sky.test_dtai());
                sky.dtai()
            },
            37.0,
            TOL,
        );
        assert_close(
            {
                assert!(sky.test_dut1());
                sky.dut1()
            },
            0.3,
            TOL,
        );
    }

    // -----------------------------------------------------------------------
    // Frame trait: format / unformat
    // -----------------------------------------------------------------------

    #[test]
    fn format_longitude_hms() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        // PI/2 = 6h
        let s = sky.format(1, FRAC_PI_2).unwrap();
        assert_eq!(s, "6:00:00.0");
    }

    #[test]
    fn format_latitude_dms() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        // PI/4 = 45 degrees. C `astFormat` emits no leading '+' for
        // positive sexagesimal values.
        let s = sky.format(2, PI / 4.0).unwrap();
        assert_eq!(s, "45:00:00");
    }

    #[test]
    fn format_bad_axis() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        assert!(sky.format(0, 0.0).is_err());
        assert!(sky.format(3, 0.0).is_err());
    }

    #[test]
    fn unformat_hms() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let (val, _nc) = sky.unformat(1, "6:00:00.0").unwrap();
        assert_close(val, FRAC_PI_2, 1.0e-6);
    }

    #[test]
    fn unformat_dms() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let (val, _nc) = sky.unformat(2, "+45:00:00").unwrap();
        assert_close(val, PI / 4.0, 1.0e-6);
    }

    // -----------------------------------------------------------------------
    // Frame trait: distance
    // -----------------------------------------------------------------------

    #[test]
    fn distance_same_point() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let d = sky.distance(&[1.0, 0.5], &[1.0, 0.5]).unwrap();
        assert_close(d, 0.0, TOL);
    }

    #[test]
    fn distance_poles() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let d = sky.distance(&[0.0, FRAC_PI_2], &[0.0, -FRAC_PI_2]).unwrap();
        assert_close(d, PI, TOL);
    }

    #[test]
    fn distance_bad_value() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let d = sky.distance(&[BAD_VALUE, 0.0], &[1.0, 0.5]).unwrap();
        assert_eq!(d, BAD_VALUE);
    }

    // -----------------------------------------------------------------------
    // Frame trait: resolve_points (spherical / great-circle geometry)
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_points_along_equator() {
        // Basis vector runs east along the equator from (0,0) to (0.1,0). A
        // point just north of it resolves to a parallel component equal to its
        // longitude arc and a perpendicular component equal to its latitude
        // arc. Per skyframe.c:8457, a positive perpendicular means rotation
        // from the basis vector to the point matches the lon->lat sense, so a
        // northward point is positive.
        let sky = SkyFrame::new(SkySystem::Fk5);
        let pts = crate::coords::PointSet::from_rows([[0.05, 0.02]].iter().map(|p| &p[..]));
        let res = sky.resolve_points(&[0.0, 0.0], &[0.1, 0.0], &pts).unwrap();
        let comp = res.point(0);
        assert_close(comp[0], 0.05, 1.0e-6);
        assert_close(comp[1], 0.02, 1.0e-6);
    }

    #[test]
    fn resolve_points_differs_from_planar() {
        // At high declination the great-circle perpendicular component is
        // markedly smaller than the naive planar delta-latitude, because a
        // parallel of latitude is a small circle. This divergence is what the
        // planar base-Frame implementation gets wrong for sky coordinates.
        let sky = SkyFrame::new(SkySystem::Fk5);
        let lat = 1.2_f64; // ~68.75 deg
        let pts =
            crate::coords::PointSet::from_rows([[0.001, lat + 1.0e-5]].iter().map(|p| &p[..]));
        let res = sky
            .resolve_points(&[0.0, lat], &[0.002, lat], &pts)
            .unwrap();
        let comp = res.point(0);
        // The spherical perpendicular differs from the planar 1e-5 by more
        // than the sagitta of the small-circle chord over this longitude span.
        assert!(comp[1].abs() < 1.0e-5, "prp={:.3e}", comp[1]);
        assert!(comp[1].is_finite());
    }

    #[test]
    fn resolve_points_bad_input() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let pts = crate::coords::PointSet::from_rows([[BAD_VALUE, 0.02]].iter().map(|p| &p[..]));
        let res = sky.resolve_points(&[0.0, 0.0], &[0.1, 0.0], &pts).unwrap();
        let comp = res.point(0);
        assert_eq!(comp[0], BAD_VALUE);
        assert_eq!(comp[1], BAD_VALUE);
    }

    // -----------------------------------------------------------------------
    // Frame trait: angle
    // -----------------------------------------------------------------------

    #[test]
    fn angle_bad_value() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let ang = sky
            .angle(&[BAD_VALUE, 0.0], &[0.0, 0.0], &[1.0, 0.0])
            .unwrap();
        assert_eq!(ang, BAD_VALUE);
    }

    #[test]
    fn angle_is_signed_and_matches_oracle() {
        // Oracle values from the reference C library (astAngle on a
        // FK5 SkyFrame): the angle at b is signed, measured from the
        // direction of north (positive towards east).
        let sky = SkyFrame::new(SkySystem::Fk5);
        let north = [0.0, 0.1];
        let b = [0.0, 0.0];
        let east = [0.1, 0.0];
        let west = [-0.1, 0.0];
        let half_pi = std::f64::consts::FRAC_PI_2;
        assert!((sky.angle(&north, &b, &east).unwrap() - half_pi).abs() < 1e-9);
        assert!((sky.angle(&north, &b, &west).unwrap() + half_pi).abs() < 1e-9);
        assert!((sky.angle(&east, &b, &north).unwrap() + half_pi).abs() < 1e-9);

        // General-position oracle value.
        let p = [0.3, 0.4];
        let q = [0.25, -0.2];
        let t = [0.6, 0.1];
        assert!(
            (sky.angle(&p, &q, &t).unwrap() - 0.796037127730).abs() < 1e-9,
            "got {}",
            sky.angle(&p, &q, &t).unwrap()
        );
    }

    #[test]
    fn angle_coincident_points_return_bad() {
        // C returns AST__BAD when a or c coincides with the vertex b.
        let sky = SkyFrame::new(SkySystem::Fk5);
        let b = [0.0, 0.0];
        let east = [0.1, 0.0];
        assert_eq!(sky.angle(&b, &b, &east).unwrap(), BAD_VALUE);
        assert_eq!(sky.angle(&east, &b, &b).unwrap(), BAD_VALUE);
    }

    #[test]
    fn angle_respects_axis_permutation() {
        // With the axes swapped (axis 1 = latitude) C applies a
        // pi/2 - result correction (skyframe.c:1205). Oracle values
        // from the reference library via astPermAxes.
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        Frame::perm_axes(&mut sky, &[2, 1]).unwrap();
        // Same geometry as the unpermuted N/b/E case, coords (lat, lon).
        let north = [0.1, 0.0];
        let b = [0.0, 0.0];
        let east = [0.0, 0.1];
        assert!(sky.angle(&north, &b, &east).unwrap().abs() < 1e-9);

        let p = [0.4, 0.3];
        let q = [-0.2, 0.25];
        let t = [0.1, 0.6];
        assert!(
            (sky.angle(&p, &q, &t).unwrap() - 0.774759199065).abs() < 1e-9,
            "got {}",
            sky.angle(&p, &q, &t).unwrap()
        );
    }

    // -----------------------------------------------------------------------
    // SkyTol
    // -----------------------------------------------------------------------

    #[test]
    fn sky_tol_default_is_one_milliarcsec() {
        // C default: 0.001 arcsec (skyframe.c:11549).
        let sky = SkyFrame::new(SkySystem::Fk5);
        assert!((sky.sky_tol() - 0.001).abs() < 1e-15);
    }

    #[test]
    fn convert_collapses_chain_within_sky_tol() {
        // Oracle: astConvert(ICRS, FK5 J2000) with SkyTol=100 on both
        // frames simplifies to a UnitMap (the ~23 mas frame bias is
        // within tolerance); at the default 1 mas it does not.
        let mut icrs = SkyFrame::new(SkySystem::Icrs);
        icrs.set_epoch_year(2000.0);
        icrs.set_sky_tol(100.0);
        let mut fk5 = SkyFrame::new(SkySystem::Fk5);
        fk5.set_epoch_year(2000.0);
        fk5.set_sky_tol(100.0);

        let fs = FrameSearch::convert_to(&mut icrs, &mut fk5, &[])
            .unwrap()
            .unwrap();
        let map = fs
            .mapping(FrameRef::Base, FrameRef::Current)
            .unwrap()
            .simplify()
            .unwrap();
        assert!(
            map.as_any().is::<crate::mappings::UnitMap>(),
            "expected UnitMap within SkyTol, got {map:?}"
        );

        let mut icrs2 = SkyFrame::new(SkySystem::Icrs);
        icrs2.set_epoch_year(2000.0);
        let mut fk52 = SkyFrame::new(SkySystem::Fk5);
        fk52.set_epoch_year(2000.0);
        let fs2 = FrameSearch::convert_to(&mut icrs2, &mut fk52, &[])
            .unwrap()
            .unwrap();
        let map2 = fs2
            .mapping(FrameRef::Base, FrameRef::Current)
            .unwrap()
            .simplify()
            .unwrap();
        assert!(
            !map2.as_any().is::<crate::mappings::UnitMap>(),
            "the 23 mas ICRS-FK5 bias exceeds the default 1 mas SkyTol"
        );
    }

    // -----------------------------------------------------------------------
    // Frame trait: offset
    // -----------------------------------------------------------------------

    #[test]
    fn offset_zero_distance() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let result = sky.offset(&[1.0, 0.5], &[2.0, 0.3], 0.0).unwrap();
        // Zero distance should return the start point.
        assert_close(result[0], 1.0, TOL);
        assert_close(result[1], 0.5, TOL);
    }

    #[test]
    fn offset_full_distance() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let from = [0.0, 0.0];
        let to = [FRAC_PI_2, 0.0];
        let sep = sky.distance(&from, &to).unwrap();
        let result = sky.offset(&from, &to, sep).unwrap();
        assert_close(result[0], FRAC_PI_2, 1.0e-8);
        assert_close(result[1], 0.0, 1.0e-8);
    }

    #[test]
    fn offset_bad_value() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let result = sky.offset(&[BAD_VALUE, 0.0], &[1.0, 0.0], 0.5).unwrap();
        assert_eq!(result[0], BAD_VALUE);
        assert_eq!(result[1], BAD_VALUE);
    }

    // -----------------------------------------------------------------------
    // Frame trait: norm
    // -----------------------------------------------------------------------

    #[test]
    fn norm_no_op() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let mut coords = [1.0, 0.5];
        sky.norm(&mut coords).unwrap();
        assert_close(coords[0], 1.0, TOL);
        assert_close(coords[1], 0.5, TOL);
    }

    #[test]
    fn norm_wraps_longitude() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        // Default neglon=false, so longitude normalizes to [0, 2pi].
        let mut coords = [-0.5, 0.3];
        sky.norm(&mut coords).unwrap();
        assert_close(coords[0], 2.0 * PI - 0.5, 1.0e-8);
        assert_close(coords[1], 0.3, TOL);
    }

    #[test]
    fn norm_neglon_wraps_to_minus_pi_pi() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_neg_lon(true);
        let mut coords = [4.0, 0.3];
        sky.norm(&mut coords).unwrap();
        // 4.0 radians normalized to [-pi, pi].
        let expected_lon = pal::drange(4.0);
        assert_close(coords[0], expected_lon, 1.0e-8);
    }

    #[test]
    fn norm_folds_latitude_through_pole() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        // Latitude > pi/2 folds through the north pole.
        let mut coords = [0.0, FRAC_PI_2 + 0.1];
        sky.norm(&mut coords).unwrap();
        // Latitude should become pi - (pi/2 + 0.1) = pi/2 - 0.1
        assert_close(coords[1], FRAC_PI_2 - 0.1, 1.0e-8);
        // Longitude shifts by pi.
        assert_close(coords[0], PI, 1.0e-8);
    }

    #[test]
    fn norm_bad_value_no_op() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let mut coords = [BAD_VALUE, 0.5];
        sky.norm(&mut coords).unwrap();
        assert_eq!(coords[0], BAD_VALUE);
        assert_close(coords[1], 0.5, TOL);
    }

    // -----------------------------------------------------------------------
    // Frame trait: convert
    // -----------------------------------------------------------------------

    #[test]
    fn convert_fk5_to_galactic() {
        let mut fk5 = SkyFrame::new(SkySystem::Fk5);
        let mut gal = SkyFrame::new(SkySystem::Galactic);

        let fs = fk5.convert_to(&mut gal, &[]).unwrap();
        assert!(fs.is_some(), "FK5 -> Galactic conversion should succeed");

        let fs = fs.unwrap();
        // The FrameSet should be usable as a mapping.
        let pts = crate::coords::PointSet::from_rows([[0.0, 0.0]]);
        let out = fs.transform(&pts, true).unwrap();
        let p = out.point(0);
        // RA=0, Dec=0 in FK5 should give a specific Galactic coordinate.
        // Just verify we get something reasonable (l, b).
        assert!(p[0].is_finite());
        assert!(p[1].is_finite());
    }

    #[test]
    fn convert_skyframe_to_blank_domain_frame_is_unitmap() {
        // `astConvert(SkyFrame, Frame[Domain=""])` succeeds with a
        // UnitMap: a blank-domain plain Frame matches any Frame, and the
        // generic ConvertX driver casts the SkyFrame down to a plain
        // Frame for the alignment, leaving the axes unchanged. Confirmed
        // against the C oracle (ast_tester/testconvert.c errors 4/8 fire
        // if no FrameSet, 3/7 if not a UnitMap).
        use crate::frames::{BasicFrameBuilder, FrameSearch};
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        let mut basic = BasicFrameBuilder::new(2).build().unwrap();

        let fs = sky
            .convert_to(&mut basic, &[])
            .unwrap()
            .expect("SkyFrame -> blank-domain Frame converts");
        let map = fs
            .mapping(super::FrameRef::Base, super::FrameRef::Current)
            .unwrap();
        assert!(
            map.as_any().is::<UnitMap>(),
            "SkyFrame -> blank-domain Frame is a UnitMap"
        );
    }

    // -----------------------------------------------------------------------
    // Frame trait: system_string
    // -----------------------------------------------------------------------

    #[test]
    fn system_string() {
        let sky = SkyFrame::new(SkySystem::Galactic);
        assert_eq!(sky.system_string().unwrap(), Some("GALACTIC"));
    }

    // -----------------------------------------------------------------------
    // Frame trait: map_from
    // -----------------------------------------------------------------------

    #[test]
    fn map_from_same() {
        let sky = SkyFrame::new(SkySystem::Fk5);
        let prev = sky.clone();
        let mapping = sky.map_from(&prev).unwrap();
        // Same system, equinox, epoch -> UnitMap.
        assert!(mapping.as_any().is::<UnitMap>());
    }

    // -----------------------------------------------------------------------
    // Clone / Send
    // -----------------------------------------------------------------------

    #[test]
    fn clone_is_independent() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        sky.set_title("original");
        let mut clone = sky.clone();
        clone.set_title("clone");
        assert_eq!(sky.title().unwrap(), "original");
        assert_eq!(clone.title().unwrap(), "clone");
    }

    fn _assert_send<T: Send>() {}

    #[test]
    fn skyframe_is_send() {
        _assert_send::<SkyFrame>();
    }

    // -----------------------------------------------------------------------
    // Offset coordinate integration (convert / map_from)
    // -----------------------------------------------------------------------

    #[test]
    fn convert_with_origin_offset() {
        // Source frame has offset coordinates; target is standard FK5.
        let mut src = SkyFrame::new(SkySystem::Fk5);
        src.set_sky_ref_lon_lat(1.0, 0.5);
        src.set_sky_ref_p_lon_lat(1.0, 0.5 + 0.1);
        src.set_sky_ref_is(SkyRefIs::Origin);

        let mut dst = SkyFrame::new(SkySystem::Fk5);

        let fs = src.convert_to(&mut dst, &[]).unwrap().unwrap();

        // (0, 0) in offset should map to sky_ref in standard.
        let pts = crate::coords::PointSet::from_rows([[0.0, 0.0]]);
        let out = fs.transform(&pts, true).unwrap();
        let p = out.point(0);

        // Compare with wrapping tolerance.
        let lon_diff = (p[0] - 1.0).rem_euclid(2.0 * PI);
        let lon_err = lon_diff.min(2.0 * PI - lon_diff);
        assert!(lon_err < 1.0e-10, "lon mismatch: {:.2e}", lon_err);
        assert_close(p[1], 0.5, 1.0e-10);
    }

    #[test]
    fn convert_to_offset_frame() {
        // Source is standard FK5; target has offset coordinates.
        let mut src = SkyFrame::new(SkySystem::Fk5);

        let mut dst = SkyFrame::new(SkySystem::Fk5);
        dst.set_sky_ref_lon_lat(1.0, 0.5);
        dst.set_sky_ref_p_lon_lat(1.0, 0.5 + 0.1);
        dst.set_sky_ref_is(SkyRefIs::Origin);

        let fs = src.convert_to(&mut dst, &[]).unwrap().unwrap();

        // sky_ref in standard should map to (0, 0) in offset.
        let pts = crate::coords::PointSet::from_rows([[1.0, 0.5]]);
        let out = fs.transform(&pts, true).unwrap();
        let p = out.point(0);

        // The longitude at (0,0) may come out as 0 or +-2pi.
        let lon_diff = p[0].rem_euclid(2.0 * PI);
        let lon_err = lon_diff.min(2.0 * PI - lon_diff);
        assert!(lon_err < 1.0e-10, "lon mismatch: {:.2e}", lon_err);
        assert_close(p[1], 0.0, 1.0e-10);
    }

    #[test]
    fn convert_between_offset_frames_round_trip() {
        // Both source and target have offsets; round-trip through convert.
        let mut src = SkyFrame::new(SkySystem::Fk5);
        src.set_sky_ref_lon_lat(0.5, 0.3);
        src.set_sky_ref_p_lon_lat(0.5, 0.3 + 0.1);
        src.set_sky_ref_is(SkyRefIs::Origin);

        let mut dst = SkyFrame::new(SkySystem::Fk5);
        dst.set_sky_ref_lon_lat(2.0, -0.4);
        dst.set_sky_ref_p_lon_lat(2.0, -0.4 + 0.1);
        dst.set_sky_ref_is(SkyRefIs::Origin);

        let fs_fwd = src.convert_to(&mut dst, &[]).unwrap().unwrap();

        // Pick a point in source offset coords, convert to dst offset,
        // then convert back.
        let test_lon = 0.1;
        let test_lat = 0.2;
        let pts = crate::coords::PointSet::from_rows([[test_lon, test_lat]]);
        let mid = fs_fwd.transform(&pts, true).unwrap();
        let back = fs_fwd.transform(&mid, false).unwrap();
        let p = back.point(0);

        assert_close(p[0], test_lon, 1.0e-9);
        assert_close(p[1], test_lat, 1.0e-9);
    }

    #[test]
    fn map_from_with_offsets() {
        // Test map_from with offset source.
        let mut prev = SkyFrame::new(SkySystem::Fk5);
        prev.set_sky_ref_lon_lat(1.0, 0.5);
        prev.set_sky_ref_p_lon_lat(1.0, 0.5 + 0.1);
        prev.set_sky_ref_is(SkyRefIs::Origin);

        let cur = SkyFrame::new(SkySystem::Fk5);

        let mapping = cur.map_from(&prev).unwrap();

        // (0,0) in prev (offset) should map to sky_ref in standard.
        let pts = crate::coords::PointSet::from_rows([[0.0, 0.0]]);
        let out = mapping.transform(&pts, true).unwrap();
        let p = out.point(0);

        let lon_diff = (p[0] - 1.0).rem_euclid(2.0 * PI);
        let lon_err = lon_diff.min(2.0 * PI - lon_diff);
        assert!(lon_err < 1.0e-10, "lon mismatch: {:.2e}", lon_err);
        assert_close(p[1], 0.5, 1.0e-10);
    }

    #[test]
    fn skyframe_set_ident_round_trip() {
        let mut sky = SkyFrame::new(SkySystem::Fk5);
        assert_eq!(sky.ident(), "");
        sky.set_ident("A");
        assert_eq!(sky.ident(), "A");
        sky.set_ident("");
        assert_eq!(sky.ident(), "");
    }

    // -----------------------------------------------------------------------
    // Match / SubFrame / Overlay (skyframe.c:6604 / 9869 / 8010)
    // -----------------------------------------------------------------------

    #[test]
    fn overlay_template_copies_set_system() {
        // Template has System explicitly set to Galactic; result starts
        // life as FK5. After overlay the result must be Galactic and
        // its Title cleared (since the system changed and the template
        // had System set).
        let mut template = SkyFrame::new(SkySystem::Galactic);
        // SkyFrame::new for Galactic auto-sets system_set = true.
        assert!(template.test_system());
        let target = SkyFrame::new(SkySystem::Fk5);
        let mut result: DynFrame = Box::new(target);
        template.overlay(&[0, 1], result.as_mut()).unwrap();
        let result_sky = result.as_any().downcast_ref::<SkyFrame>().unwrap();
        assert_eq!(result_sky.system(), SkySystem::Galactic);
        // Set a known title on the template; after overlay it should
        // copy through.
        template.set_title("custom");
        let mut r2: DynFrame = Box::new(SkyFrame::new(SkySystem::Fk5));
        template.overlay(&[0, 1], r2.as_mut()).unwrap();
        let r2_sky = r2.as_any().downcast_ref::<SkyFrame>().unwrap();
        assert_eq!(r2_sky.title().unwrap(), "custom");
    }

    #[test]
    fn overlay_template_skips_unset_attrs() {
        // A template whose System is unset. Overlaying onto a Galactic
        // target must not overwrite the target's System.
        let template = SkyFrame::unset_system();
        assert!(!template.test_system());
        let target = SkyFrame::new(SkySystem::Galactic);
        let mut result: DynFrame = Box::new(target);
        template.overlay(&[0, 1], result.as_mut()).unwrap();
        let r = result.as_any().downcast_ref::<SkyFrame>().unwrap();
        assert_eq!(r.system(), SkySystem::Galactic);
    }

    #[test]
    fn sub_frame_skyframe_to_skyframe() {
        // FK5 target → Galactic template via SubFrame must produce a
        // 2-axis Mapping and a Galactic result Frame.
        let target = SkyFrame::new(SkySystem::Fk5);
        let template = SkyFrame::new(SkySystem::Galactic);
        let template_axes = [0i32, 1];
        let target_axes = [0i32, 1];
        let (map, result) = target
            .sub_frame(Some(&template), 2, &target_axes, &template_axes)
            .unwrap()
            .expect("should match");
        assert_eq!(map.nin(), 2);
        assert_eq!(map.nout(), 2);
        let result_sky = result.as_any().downcast_ref::<SkyFrame>().unwrap();
        assert_eq!(result_sky.system(), SkySystem::Galactic);
    }

    #[test]
    fn match_template_skyframe_self() {
        let target = SkyFrame::new(SkySystem::Fk5);
        let template = SkyFrame::new(SkySystem::Galactic);
        let m = template
            .match_template(&target, false)
            .unwrap()
            .expect("should match");
        // Both axes accounted for.
        assert_eq!(m.template_axes.len(), 2);
        assert_eq!(m.target_axes.len(), 2);
        // Result must be a SkyFrame in Galactic.
        let r = m.result.as_any().downcast_ref::<SkyFrame>().unwrap();
        assert_eq!(r.system(), SkySystem::Galactic);
    }

    #[test]
    fn match_template_skyframe_swap_blocked_when_permute_false() {
        // Target has its axes swapped; with Permute explicitly cleared
        // to false the swap-detection logic (skyframe.c:6826
        // `match = !swap || astGetPermute(template)`) refuses the match.
        let mut target = SkyFrame::new(SkySystem::Fk5);
        target.perm_axes(&[2, 1]).unwrap();
        let mut template = SkyFrame::new(SkySystem::Galactic);
        template.set_permute(false).unwrap();
        assert!(!template.permute());
        let result = template.match_template(&target, false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn match_template_skyframe_permute_defaults_true() {
        // Permute defaults to 1 (true) — SkyFrame inherits the parent
        // Frame default (frame.c:13459-13460); it does not override the
        // getter. An axis-swapped target therefore matches by default.
        let template = SkyFrame::new(SkySystem::Galactic);
        assert!(template.permute());
        let mut target = SkyFrame::new(SkySystem::Fk5);
        target.perm_axes(&[2, 1]).unwrap();
        assert!(
            template.match_template(&target, false).unwrap().is_some(),
            "axis-swapped SkyFrame matches with the default Permute=true"
        );
    }

    #[test]
    fn match_template_skyframe_swap_allowed_when_permute_true() {
        let mut target = SkyFrame::new(SkySystem::Fk5);
        target.perm_axes(&[2, 1]).unwrap();
        let mut template = SkyFrame::new(SkySystem::Galactic);
        template.set_permute(true).unwrap();
        let m = template
            .match_template(&target, false)
            .unwrap()
            .expect("should match with permute on");
        assert_eq!(m.template_axes.len(), 2);
    }

    #[test]
    fn pick_axes_snapshot_skyframe_full() {
        // Picking both axes in canonical order yields a SkyFrame copy.
        let target = SkyFrame::new(SkySystem::Galactic);
        let snap = target.pick_axes_snapshot(&[0, 1]).unwrap();
        let s = snap.as_any().downcast_ref::<SkyFrame>().unwrap();
        assert_eq!(s.system(), SkySystem::Galactic);
    }

    #[test]
    fn pick_axes_snapshot_skyframe_partial() {
        // Picking just one axis yields a 1-axis BasicFrame (no longer
        // a SkyFrame).
        let target = SkyFrame::new(SkySystem::Galactic);
        let snap = target.pick_axes_snapshot(&[1]).unwrap();
        assert_eq!(snap.naxes(), 1);
        assert!(snap.as_any().downcast_ref::<SkyFrame>().is_none());
    }

    #[test]
    fn cast_skyframe_to_basic_frame() {
        let sf = SkyFrame::new(crate::frames::SkySystem::Fk5);
        let bf = crate::frames::BasicFrameBuilder::new(2).build().unwrap();
        let casted = sf.cast(&bf).unwrap();
        assert!(casted.is_some());
        let casted = casted.unwrap();
        assert!(
            casted
                .as_any()
                .downcast_ref::<crate::frames::BasicFrame>()
                .is_some()
        );
        assert_eq!(casted.naxes(), 2);
    }

    #[test]
    fn cast_skyframe_preserves_per_axis_metadata() {
        // C astCast copies each source Axis into the basic Frame
        // (cmpframe.c:1335-1338) so per-axis Label/Unit/Symbol carry
        // through. Set explicit Label and Symbol overrides on a
        // SkyFrame, cast it down to a plain Frame, and confirm both the
        // overrides and the system-derived Unit survive on the result.
        let mut sf = SkyFrame::new(crate::frames::SkySystem::Galactic);
        sf.set_label(1, "MyLon").unwrap();
        sf.set_label(2, "MyLat").unwrap();
        sf.set_symbol(1, "ML").unwrap();
        sf.set_symbol(2, "MB").unwrap();

        let template = crate::frames::BasicFrameBuilder::new(2).build().unwrap();
        let casted = sf.cast(&template).unwrap().unwrap();
        let out = casted
            .as_any()
            .downcast_ref::<crate::frames::BasicFrame>()
            .expect("cast result should be a BasicFrame");

        assert_eq!(out.naxes(), 2);
        // Explicit per-axis Label overrides survive.
        assert_eq!(out.label(1).unwrap(), "MyLon");
        assert_eq!(out.label(2).unwrap(), "MyLat");
        // Explicit per-axis Symbol overrides survive.
        assert_eq!(out.symbol(1).unwrap(), "ML");
        assert_eq!(out.symbol(2).unwrap(), "MB");
        // System-derived per-axis Unit survives (galactic -> degrees).
        assert_eq!(out.unit(1).unwrap(), "degrees");
        assert_eq!(out.unit(2).unwrap(), "degrees");
    }
}
