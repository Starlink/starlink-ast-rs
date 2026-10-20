//! The basic (Cartesian) coordinate frame.

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::DynMapping;
use crate::mappings::helpers::is_bad;
use crate::serial::{Node, Serializable};

use super::axis::{Axis, FrameAxis};
use super::frame_attrs::{
    FrameAttrs, FrameDumpSource, clean_domain, default_epoch_mjd, default_symbol, read_frame_attrs,
    read_system_item, write_frame_cards,
};
use super::{AxisKind, DynFrame, Frame};

/// The coordinate systems a plain Frame can have. C's Frame class knows only
/// one, `AST__CART` (frame.c, `ValidateSystem`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrameSystem {
    /// Cartesian coordinates, the only System of a plain Frame.
    #[default]
    Cartesian,
}

impl FrameSystem {
    /// The name C gives the System (frame.c, `SystemString`).
    pub fn as_str(&self) -> &'static str {
        "Cartesian"
    }
}

/// Coordinate system description: a plain Frame.
///
/// A Frame represents a coordinate system, much as a frame around a graph
/// describes the coordinate space in which data are plotted.
/// It has a Title describing the coordinate space, and axes holding
/// information such as Label and Unit strings for labeling, for instance,
/// graphical output.
/// The number of axes is not restricted to two.
/// The [`Frame`] trait gives every class of Frame functions for formatting
/// coordinate values for display and for calculating distances and offsets
/// between positions, and lets Frames find out how to convert to and from
/// related coordinate systems.
///
/// `BasicFrame` is the plain Frame class: an N-dimensional Cartesian
/// coordinate system with no special axis semantics, whose geometry
/// methods use straight-line Euclidean distance.
/// Construct one with [`BasicFrameBuilder`].
///
/// Every attribute has a getter giving its value, set or default, and a
/// setter, a `test_` and a `clear_` method.
/// The attributes, all on the [`Frame`] trait unless noted:
///
/// - [`align_system`](BasicFrame::align_system): coordinate system used to
///   align Frames.
/// - [`bottom`](Frame::bottom): lowest axis value to display.
/// - [`digits`](Frame::digits) and [`axis_digits`](Frame::axis_digits):
///   number of digits of precision.
/// - [`direction`](Frame::direction): display axis in conventional
///   direction?
/// - [`domain`](Frame::domain): coordinate system domain.
/// - [`dtai`](Frame::dtai): difference between the TAI and UTC timescales.
/// - [`dut1`](Frame::dut1): difference between the UT1 and UTC timescales.
/// - [`epoch_mjd`](Frame::epoch_mjd): epoch of observation.
/// - [`format_str`](Frame::format_str): format specification for axis
///   values.
/// - [`internal_unit`](Frame::internal_unit): physical units for
///   unformatted axis values.
/// - [`label`](Frame::label): axis label.
/// - [`match_end`](Frame::match_end): match trailing axes?
/// - [`max_axes`](Frame::max_axes): maximum number of Frame axes to match.
/// - [`min_axes`](Frame::min_axes): minimum number of Frame axes to match.
/// - [`naxes`](Frame::naxes): number of Frame axes.
/// - [`norm_unit`](Frame::norm_unit): normalized physical units for
///   formatted axis values.
/// - [`obs_alt`](Frame::obs_alt): geodetic altitude of observer.
/// - [`obs_lat`](Frame::obs_lat): geodetic latitude of observer.
/// - [`obs_lon`](Frame::obs_lon): geodetic longitude of observer.
/// - [`permute`](Frame::permute): permute axis order?
/// - [`preserve_axes`](Frame::preserve_axes): preserve axes?
/// - [`symbol`](Frame::symbol): axis symbol.
/// - [`system`](BasicFrame::system): coordinate system used to describe the
///   domain.
/// - [`title`](Frame::title): Frame title.
/// - [`top`](Frame::top): highest axis value to display.
/// - [`unit`](Frame::unit): physical units for formatted axis values.
#[derive(Debug, Clone)]
#[doc(alias = "Frame")]
pub struct BasicFrame {
    naxes: usize,
    attrs: FrameAttrs,
    /// The Axis object on each axis, in internal (unpermuted) order, as
    /// C's Frame holds them. C's `SubFrame` attaches the target's own Axis
    /// objects to the result (frame.c:11023), so a plain Frame built by
    /// picking axes out of a SkyFrame carries SkyAxis objects, which keep
    /// formatting, normalising and dumping as a SkyAxis does.
    axes: Vec<FrameAxis>,
    /// The axis permutation, `perm[external] = internal` (0-based), as C's
    /// `perm` array.
    perm: Vec<usize>,
    system_set: bool,
    align_system_set: bool,
}

/// Builder for [`BasicFrame`].
///
/// # Examples
///
/// ```
/// use ast_core::frames::{BasicFrameBuilder, Frame};
///
/// let frame = BasicFrameBuilder::new(2)
///     .title("Sky Position")
///     .domain("SKY")
///     .label(1, "X")
///     .label(2, "Y")
///     .build()
///     .unwrap();
///
/// assert_eq!(frame.naxes(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct BasicFrameBuilder {
    naxes: usize,
    attrs: FrameAttrs,
    axes: Vec<Axis>,
}

impl BasicFrameBuilder {
    /// Create a Frame: start a builder for a [`BasicFrame`] with `naxes`
    /// axes.
    ///
    /// `naxes` is the number of dimensions of the coordinate space the
    /// Frame describes.
    /// The builder's methods set attributes of the new Frame, and
    /// [`build`](BasicFrameBuilder::build) creates it.
    ///
    /// # Errors
    ///
    /// This function does not fail; [`build`](BasicFrameBuilder::build)
    /// raises `ErrorCode::Naxin` when `naxes` is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::{BasicFrameBuilder, Frame};
    ///
    /// let frame = BasicFrameBuilder::new(2)
    ///     .title("Energy Spectrum")
    ///     .label(1, "Energy")
    ///     .label(2, "Response")
    ///     .build()
    ///     .unwrap();
    /// assert_eq!(frame.label(1).unwrap(), "Energy");
    /// assert!(BasicFrameBuilder::new(0).build().is_err());
    /// ```
    #[doc(alias = "astFrame")]
    pub fn new(naxes: usize) -> Self {
        Self {
            naxes,
            attrs: FrameAttrs::default(),
            axes: vec![Axis::default(); naxes],
        }
    }

    fn axis_mut(&mut self, axis: usize) -> Option<&mut Axis> {
        axis.checked_sub(1).and_then(|i| self.axes.get_mut(i))
    }

    /// Set the frame title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.attrs.title = Some(title.into());
        self
    }

    /// Set the frame domain, with white space removed and letters
    /// upper-cased as C stores it.
    pub fn domain(mut self, domain: impl Into<String>) -> Self {
        self.attrs.domain = Some(clean_domain(&domain.into()));
        self
    }

    /// Set the frame epoch as a TDB Modified Julian Date.
    pub fn epoch_mjd(mut self, mjd: f64) -> Self {
        self.attrs.epoch = Some(mjd);
        self
    }

    /// Set the label for a 1-based axis.
    pub fn label(mut self, axis: usize, label: impl Into<String>) -> Self {
        if let Some(a) = self.axis_mut(axis) {
            a.label = Some(label.into());
        }
        self
    }

    /// Set the unit for a 1-based axis.
    pub fn unit(mut self, axis: usize, unit: impl Into<String>) -> Self {
        if let Some(a) = self.axis_mut(axis) {
            a.unit = Some(unit.into());
        }
        self
    }

    /// Set the symbol for a 1-based axis.
    pub fn symbol(mut self, axis: usize, symbol: impl Into<String>) -> Self {
        if let Some(a) = self.axis_mut(axis) {
            a.symbol = Some(symbol.into());
        }
        self
    }

    /// Set the format string for a 1-based axis.
    pub fn format_str(mut self, axis: usize, fmt: impl Into<String>) -> Self {
        if let Some(a) = self.axis_mut(axis) {
            a.format = Some(fmt.into());
        }
        self
    }

    /// Set the bottom limit for a 1-based axis.
    pub fn bottom(mut self, axis: usize, bottom: f64) -> Self {
        if let Some(a) = self.axis_mut(axis) {
            a.bottom = Some(bottom);
        }
        self
    }

    /// Set the top limit for a 1-based axis.
    pub fn top(mut self, axis: usize, top: f64) -> Self {
        if let Some(a) = self.axis_mut(axis) {
            a.top = Some(top);
        }
        self
    }

    /// Build the [`BasicFrame`].
    ///
    /// Returns an error if `naxes` is zero.
    pub fn build(self) -> AstResult<BasicFrame> {
        if self.naxes == 0 {
            return Err(AstError::new_tracked(
                ErrorCode::Naxin,
                "number of axes must be at least 1",
            ));
        }
        Ok(BasicFrame {
            naxes: self.naxes,
            attrs: self.attrs,
            axes: self.axes.into_iter().map(FrameAxis::Plain).collect(),
            perm: (0..self.naxes).collect(),
            system_set: false,
            align_system_set: false,
        })
    }
}

/// Generate the get/set/test/clear methods of an optional Frame-class
/// attribute stored in `FrameAttrs`.
macro_rules! frame_attr_methods {
    ($field:ident, $ty:ty, $set:ident, $test:ident, $clear:ident, $what:literal) => {
        #[doc = concat!("Set ", $what, ".")]
        pub fn $set(&mut self, value: $ty) {
            self.attrs.$field = Some(value);
        }

        #[doc = concat!("Test whether ", $what, " has been set.")]
        pub fn $test(&self) -> bool {
            self.attrs.$field.is_some()
        }

        #[doc = concat!("Clear ", $what, ", which then takes its default.")]
        pub fn $clear(&mut self) {
            self.attrs.$field = None;
        }
    };
}

impl BasicFrame {
    /// Attach an Axis object to a 1-based `axis` of this Frame.
    ///
    /// Used when axes are picked out of another Frame: C's `SubFrame` gives
    /// the result the target's own Axis objects (frame.c:11023), so a sky axis
    /// picked into a plain Frame stays a SkyAxis. Mirrors C `astSetAxis`.
    pub(crate) fn set_axis_object(&mut self, axis: usize, object: FrameAxis) {
        if axis >= 1 && axis <= self.naxes {
            let internal = self.perm[axis - 1];
            self.axes[internal] = object;
        }
    }

    /// Overlay the Frame-class attributes set in `template`, as C's
    /// `Frame::Overlay` does for a template of another class.
    pub(crate) fn overlay_frame_attrs(&mut self, template: &FrameAttrs) {
        self.attrs.overlay_from(template);
    }

    /// Replace every Frame-class attribute with those in `attrs`.
    pub(crate) fn set_frame_attrs(&mut self, attrs: FrameAttrs) {
        self.attrs = attrs;
    }

    /// The Axis object of a 1-based external axis.
    fn axis_object(&self, axis: usize) -> AstResult<&FrameAxis> {
        Ok(&self.axes[self.internal(axis)?])
    }

    /// The external (1-based) axis of internal axis `internal`, as C's
    /// inverse permutation gives it.
    fn external(&self, internal: usize) -> usize {
        self.perm
            .iter()
            .position(|&p| p == internal)
            .unwrap_or(internal)
            + 1
    }

    /// Validate that `axis` is in 1..=naxes, returning an error if not.
    fn validate_axis(&self, axis: usize) -> AstResult<()> {
        if axis < 1 || axis > self.naxes {
            return Err(AstError::new_tracked(
                ErrorCode::Axiin,
                format!("axis index {} is out of range 1..={}", axis, self.naxes),
            ));
        }
        Ok(())
    }

    /// The internal index of a validated 1-based external axis.
    fn internal(&self, axis: usize) -> AstResult<usize> {
        self.validate_axis(axis)?;
        Ok(self.perm[axis - 1])
    }

    fn ax(&self, axis: usize) -> AstResult<&Axis> {
        Ok(self.axes[self.internal(axis)?].axis())
    }

    fn ax_mut(&mut self, axis: usize) -> AstResult<&mut Axis> {
        let i = self.internal(axis)?;
        Ok(self.axes[i].axis_mut())
    }

    // Defaults of the Frame-class attributes (frame.c), for the
    // `frame_attrs_*` macros.
    fn frame_default_title(&self) -> AstResult<String> {
        Ok(format!("{}-d coordinate system", self.naxes))
    }
    fn frame_default_domain(&self) -> &'static str {
        ""
    }
    fn frame_default_epoch(&self) -> f64 {
        default_epoch_mjd()
    }
    fn frame_default_permute(&self) -> bool {
        true
    }

    // --- Frame-level attributes ---

    /// The Title, set or default (`"<naxes>-d coordinate system"`,
    /// frame.c `GetDefaultTitle`).
    pub fn title(&self) -> String {
        self.attrs
            .title
            .clone()
            .unwrap_or_else(|| format!("{}-d coordinate system", self.naxes))
    }

    /// Set the frame title.
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.attrs.title = Some(title.into());
    }

    /// Test whether the title attribute has been explicitly set.
    pub fn test_title(&self) -> bool {
        self.attrs.title.is_some()
    }

    /// Clear the title attribute, reverting to the default.
    pub fn clear_title(&mut self) {
        self.attrs.title = None;
    }

    /// Return the frame domain, which is blank unless set.
    pub fn domain(&self) -> &str {
        self.attrs.domain.as_deref().unwrap_or("")
    }

    /// Set the frame domain, with white space removed and letters
    /// upper-cased as C stores it (frame.c `CleanDomain`).
    pub fn set_domain(&mut self, domain: impl Into<String>) {
        self.attrs.domain = Some(clean_domain(&domain.into()));
    }

    /// Test whether the domain attribute has been explicitly set.
    pub fn test_domain(&self) -> bool {
        self.attrs.domain.is_some()
    }

    /// Clear the domain attribute, reverting to the default.
    pub fn clear_domain(&mut self) {
        self.attrs.domain = None;
    }

    /// Return the frame identification string.
    pub fn ident(&self) -> &str {
        self.attrs.ident.as_deref().unwrap_or("")
    }

    /// Set the frame identification string. Mirrors `astSetIdent`.
    /// Used by the FITS-WCS reader to label alternate-plane frames
    /// with their version letter (`'A'..'Z'`).
    pub fn set_ident(&mut self, ident: impl Into<String>) {
        let s = ident.into();
        self.attrs.ident = Some(s);
    }

    /// The Epoch as a TDB Modified Julian Date, set or default (J2000.0).
    pub fn epoch_mjd(&self) -> f64 {
        self.attrs.epoch.unwrap_or_else(default_epoch_mjd)
    }

    /// Set the Epoch as a TDB Modified Julian Date.
    pub fn set_epoch_mjd(&mut self, mjd: f64) {
        // A bad Epoch is no Epoch (frame.c `TestEpoch` is `!= AST__BAD`).
        self.attrs.epoch = (mjd != crate::constants::BAD_VALUE).then_some(mjd);
    }

    /// The Epoch as a year, Besselian before 1984 and Julian from then on,
    /// which is how C reports it (frame.c `GetAttrib`).
    pub fn epoch_year(&self) -> f64 {
        let mjd = self.epoch_mjd();
        if mjd < crate::pal::epj2d(1984.0) {
            crate::pal::epb(mjd)
        } else {
            crate::pal::epj(mjd)
        }
    }

    /// Set the Epoch from a year, read as Besselian before 1984 and Julian
    /// from then on, as C reads a bare year (frame.c `astReadDateTime`).
    pub fn set_epoch_year(&mut self, year: f64) {
        self.attrs.epoch = Some(crate::frames::date_time::year_to_mjd_iau1984(year));
    }

    /// Test whether the epoch attribute has been explicitly set.
    pub fn test_epoch(&self) -> bool {
        self.attrs.epoch.is_some()
    }

    /// Clear the epoch attribute.
    pub fn clear_epoch(&mut self) {
        self.attrs.epoch = None;
    }

    /// The coordinate system used to describe positions within the domain:
    /// the System attribute, which for a plain Frame is always Cartesian
    /// and cannot be altered.
    ///
    /// [`Frame::system_string`] describes the attribute and the Systems of
    /// every class.
    ///
    /// Default: [`FrameSystem::Cartesian`].
    #[doc(alias = "System")]
    pub fn system(&self) -> FrameSystem {
        FrameSystem::Cartesian
    }

    /// Set the System, which can only be Cartesian; see
    /// [`system`](BasicFrame::system).
    pub fn set_system(&mut self, _system: FrameSystem) {
        self.system_set = true;
    }

    /// Whether the System is set; see [`system`](BasicFrame::system).
    pub fn test_system(&self) -> bool {
        self.system_set
    }

    /// Clear the System; see [`system`](BasicFrame::system).
    pub fn clear_system(&mut self) {
        self.system_set = false;
    }

    /// The coordinate system in which to align the Frame: the AlignSystem
    /// attribute, which for a plain Frame is always Cartesian and cannot be
    /// altered.
    ///
    /// Controls how the Frame behaves as the template of
    /// [`find_frame_template`](Frame::find_frame_template) or the `to` Frame
    /// of [`convert_to`](Frame::convert_to): it is the coordinate system in
    /// which the two Frames are aligned.
    /// The Mapping found maps positions from the first Frame into this
    /// intermediate system using the attributes of the first Frame, then
    /// from it into the second Frame using the attributes of the second.
    /// In general it takes any value the class's System takes, and its
    /// values and default depend on the class:
    ///
    /// - Frame: always Cartesian.
    /// - CmpFrame: always `"Compound"`.
    /// - FrameSet: the AlignSystem of its current Frame.
    /// - SkyFrame: default ICRS ([`SkyFrame::align_system`]).
    /// - SpecFrame: default WAVE, wavelength
    ///   ([`SpecFrame::align_system`]).
    /// - TimeFrame: default MJD ([`TimeFrame::align_system`]).
    ///
    /// Default: [`FrameSystem::Cartesian`].
    ///
    /// [`SkyFrame::align_system`]: super::SkyFrame::align_system
    /// [`SpecFrame::align_system`]: super::SpecFrame::align_system
    /// [`TimeFrame::align_system`]: super::TimeFrame::align_system
    #[doc(alias = "AlignSystem")]
    pub fn align_system(&self) -> FrameSystem {
        FrameSystem::Cartesian
    }

    /// Set the AlignSystem, which can only be Cartesian; see
    /// [`align_system`](BasicFrame::align_system).
    pub fn set_align_system(&mut self, _system: FrameSystem) {
        self.align_system_set = true;
    }

    /// Whether the AlignSystem is set; see
    /// [`align_system`](BasicFrame::align_system).
    pub fn test_align_system(&self) -> bool {
        self.align_system_set
    }

    /// Clear the AlignSystem; see
    /// [`align_system`](BasicFrame::align_system).
    pub fn clear_align_system(&mut self) {
        self.align_system_set = false;
    }

    /// The observer's geodetic latitude in radians, set or default (0).
    pub fn obs_lat(&self) -> f64 {
        self.attrs.obs_lat.unwrap_or(0.0)
    }
    frame_attr_methods!(
        obs_lat,
        f64,
        set_obs_lat,
        test_obs_lat,
        clear_obs_lat,
        "the observer's geodetic latitude, in radians"
    );

    /// The observer's geodetic longitude in radians, set or default (0).
    pub fn obs_lon(&self) -> f64 {
        self.attrs.obs_lon.unwrap_or(0.0)
    }
    frame_attr_methods!(
        obs_lon,
        f64,
        set_obs_lon,
        test_obs_lon,
        clear_obs_lon,
        "the observer's geodetic longitude, in radians"
    );

    /// The observer's geodetic altitude in metres, set or default (0).
    pub fn obs_alt(&self) -> f64 {
        self.attrs.obs_alt.unwrap_or(0.0)
    }
    frame_attr_methods!(
        obs_alt,
        f64,
        set_obs_alt,
        test_obs_alt,
        clear_obs_alt,
        "the observer's geodetic altitude, in metres"
    );

    /// TAI-UTC in seconds, or `BAD_VALUE` when unset, which means the
    /// value is taken from AST's leap-second table (frame.c `GetDtai`).
    pub fn dtai(&self) -> f64 {
        self.attrs.dtai.unwrap_or(BAD_VALUE)
    }
    frame_attr_methods!(
        dtai,
        f64,
        set_dtai,
        test_dtai,
        clear_dtai,
        "TAI-UTC, in seconds"
    );

    /// UT1-UTC in seconds, set or default (0).
    pub fn dut1(&self) -> f64 {
        self.attrs.dut1.unwrap_or(0.0)
    }
    frame_attr_methods!(
        dut1,
        f64,
        set_dut1,
        test_dut1,
        clear_dut1,
        "UT1-UTC, in seconds"
    );

    /// Frame-level Digits, defaulting to 7. Mirrors `astGetDigits`.
    pub fn digits(&self) -> i32 {
        self.attrs.digits.unwrap_or(7)
    }

    /// Set frame-level Digits, constrained to >= 1 (frame.c:13251).
    pub fn set_digits(&mut self, value: i32) {
        self.attrs.digits = Some(value.max(1));
    }

    /// Test whether frame-level Digits is explicitly set.
    pub fn test_digits(&self) -> bool {
        self.attrs.digits.is_some()
    }

    /// Clear frame-level Digits.
    pub fn clear_digits(&mut self) {
        self.attrs.digits = None;
    }

    /// Clear MinAxes.
    pub fn clear_min_axes(&mut self) {
        self.attrs.min_axes = None;
    }

    /// Clear MaxAxes.
    pub fn clear_max_axes(&mut self) {
        self.attrs.max_axes = None;
    }

    /// Clear MatchEnd.
    pub fn clear_match_end(&mut self) {
        self.attrs.match_end = None;
    }

    /// Clear Permute.
    pub fn clear_permute(&mut self) {
        self.attrs.permute = None;
    }

    /// Clear PreserveAxes.
    pub fn clear_preserve_axes(&mut self) {
        self.attrs.preserve_axes = None;
    }
}

/// Match the leading "<bad>" token of `s` using the C scanf format
/// `" < %*1[Bb] %*1[Aa] %*1[Dd] > %n"` (axis.c:1357) and return the
/// `%n` count: the number of bytes consumed up to and including any
/// trailing whitespace. Whitespace is allowed (and skipped) before each
/// of the tokens `<`, `b`, `a`, `d`, `>` and after the closing `>`;
/// matching stops at the first non-whitespace following `>`. Returns
/// `None` if the token does not match (scanf would leave `%n` unset).
fn scan_bad_token(s: &str) -> Option<usize> {
    // The byte at `pos` after skipping any leading whitespace must equal
    // the (case-insensitively matched) expected character. `%*1[Bb]`
    // matches a single B or b; `<` and `>` are literals (a literal in a
    // scanf format does not skip whitespace itself, but the spaces around
    // them do). Returns the byte position just past the matched char.
    fn match_char(s: &str, pos: usize, expected: &[char]) -> Option<usize> {
        let mut p = pos;
        let bytes = s.as_bytes();
        while p < bytes.len() && (bytes[p] as char).is_whitespace() {
            p += 1;
        }
        let c = s[p..].chars().next()?;
        if expected.contains(&c) {
            Some(p + c.len_utf8())
        } else {
            None
        }
    }

    let p = match_char(s, 0, &['<'])?;
    let p = match_char(s, p, &['B', 'b'])?;
    let p = match_char(s, p, &['A', 'a'])?;
    let p = match_char(s, p, &['D', 'd'])?;
    let mut p = match_char(s, p, &['>'])?;

    // Trailing whitespace skipped by the space before `%n` is counted.
    let bytes = s.as_bytes();
    while p < bytes.len() && (bytes[p] as char).is_whitespace() {
        p += 1;
    }
    Some(p)
}

/// Read a formatted coordinate value, mirroring C `astAxisUnformat`
/// (axis.c:1284). This is the base Frame/Axis contract inherited by every
/// leaf Frame that does not override Unformat in C (i.e. all but CmpFrame,
/// TimeFrame and SkyFrame).
///
/// Returns `(value, nread)` where `nread` is the number of characters
/// consumed. The string is matched against `"%lf %n"`: leading whitespace is
/// skipped, the longest numeric prefix is read, and any trailing whitespace
/// is included in `nread`. Failing that, the `"<bad>"` token (case-insensitive,
/// embedded whitespace allowed) yields `BAD_VALUE`. If neither matches, the
/// result is `(BAD_VALUE, 0)` — no error is raised (axis.c:1366).
pub(crate) fn axis_unformat(text: &str) -> (f64, usize) {
    // `%lf` skips leading white space and reads as strtod does; the space
    // before `%n` then skips trailing white space, which `%n` counts.
    let bytes = text.as_bytes();
    let is_space = super::axis_format::is_c_space;
    let leading_ws = bytes.iter().take_while(|&&b| is_space(b)).count();
    let after_ws = &text[leading_ws..];
    if let Some((v, len)) = super::axis_format::c_strtod(after_ws.as_bytes()) {
        let end = leading_ws + len;
        let trailing_ws = bytes[end..].iter().take_while(|&&b| is_space(b)).count();
        return (v, end + trailing_ws);
    }

    // Recognise "<bad>" via the C scanf format
    // `" < %*1[Bb] %*1[Aa] %*1[Dd] > %n"` (axis.c:1357). Each space in that
    // format skips optional whitespace, so the tokens `<`, `b`, `a`, `d`, `>`
    // may be separated by whitespace. The trailing space before `%n` skips
    // trailing whitespace, so `%n` (nread) counts that trailing whitespace
    // too — it stops at the first non-whitespace after `>`.
    if let Some(end) = scan_bad_token(after_ws) {
        return (BAD_VALUE, leading_ws + end);
    }

    // Parse failure: nread = 0, no error (axis.c:1366).
    (BAD_VALUE, 0)
}

impl Frame for BasicFrame {
    fn axis_perm(&self) -> Vec<usize> {
        self.perm.clone()
    }

    fn naxes(&self) -> usize {
        self.naxes
    }

    frame_attrs_title!();
    frame_attrs_domain!();
    frame_attrs_epoch!();
    frame_attrs_digits!();
    frame_attrs_observer!();
    frame_attrs_ident!();
    frame_attrs_match!();

    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        (self, axis)
    }

    fn cast(&self, other: &dyn Frame) -> AstResult<Option<DynFrame>> {
        // Casting a plain Frame to another plain Frame is a same-class
        // copy (object.c Cast: generation_gap == 0 -> astCopy). Any other
        // target class is below the Frame base, so the cast fails.
        if other.as_any().downcast_ref::<BasicFrame>().is_some() {
            Ok(Some(Box::new(BasicFrame::clone(self))))
        } else {
            Ok(None)
        }
    }

    fn system_string(&self) -> AstResult<Option<&str>> {
        Ok(Some("Cartesian"))
    }

    /// The Label of a 1-based axis, set or default (`"Axis <n>"`, with `n`
    /// the internal axis number, as C's `MAKE_GET` computes it).
    fn label(&self, axis: usize) -> AstResult<String> {
        let i = self.internal(axis)?;
        Ok(self.axes[i]
            .axis()
            .label
            .clone()
            .unwrap_or_else(|| format!("Axis {}", i + 1)))
    }

    /// The Symbol of a 1-based axis, set or default: the Domain if set,
    /// else `"x"`, followed by the internal axis number (frame.c
    /// `GetDefaultSymbol`).
    fn symbol(&self, axis: usize) -> AstResult<String> {
        let i = self.internal(axis)?;
        Ok(self.axes[i]
            .axis()
            .symbol
            .clone()
            .unwrap_or_else(|| default_symbol(self.attrs.domain.as_deref(), i)))
    }

    // The Frame class's getters (frame.c `MAKE_GET`) ask the Axis object,
    // lending it the Frame's Digits.
    fn unit(&self, axis: usize) -> AstResult<String> {
        Ok(self.axis_object(axis)?.unit_value(self.digits()))
    }
    fn format_str(&self, axis: usize) -> AstResult<String> {
        Ok(self.axis_object(axis)?.format_value(self.digits()))
    }
    fn set_format_str(&mut self, axis: usize, format: &str) -> AstResult<()> {
        let i = self.internal(axis)?;
        *self.axes[i].format_slot_mut() = Some(format.to_string());
        Ok(())
    }
    fn test_format_str(&self, axis: usize) -> AstResult<bool> {
        Ok(self.axis_object(axis)?.format_slot().is_some())
    }
    fn clear_format_str(&mut self, axis: usize) -> AstResult<()> {
        let i = self.internal(axis)?;
        *self.axes[i].format_slot_mut() = None;
        Ok(())
    }
    fn direction(&self, axis: usize) -> AstResult<bool> {
        Ok(self.axis_object(axis)?.direction_value())
    }
    fn top(&self, axis: usize) -> AstResult<f64> {
        Ok(self.axis_object(axis)?.top_value())
    }
    fn bottom(&self, axis: usize) -> AstResult<f64> {
        Ok(self.axis_object(axis)?.bottom_value())
    }
    axis_attrs_base_get!(digits);
    axis_attrs_store!(label, symbol, unit, digits, direction, top, bottom);

    /// Mirrors C `Frame::Format` (frame.c:4260), which formats with the
    /// Axis object, lending it the Frame's Digits: axis.c `AxisFormat` or
    /// skyaxis.c `AxisFormat`.
    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        let digits = self.digits();
        match self.axis_object(axis)? {
            FrameAxis::Plain(a) => {
                if is_bad(value) {
                    return Ok("<bad>".to_string());
                }
                let d = a.digits.unwrap_or(digits);
                let fmt = self.axes[self.internal(axis)?].format_value(digits);
                super::axis_format::format_value(&fmt, d, value)
            }
            FrameAxis::Sky(s) => s.format_axis_value(digits, value),
        }
    }

    /// Mirrors C `Frame::GetNormUnit` (frame.c:13149): a SkyAxis supplies its
    /// own (skyaxis.c:3101).
    fn norm_unit(&self, axis: usize) -> String {
        match self
            .axis_object(axis)
            .ok()
            .and_then(|a| a.sky_norm_unit(self.digits()))
        {
            Some(u) => u,
            None => crate::units::norm_unit(&self.unit(axis).unwrap_or_default()),
        }
    }

    fn get_active_unit(&self) -> bool {
        // C `Frame::GetActiveUnit` (frame.c:4669) loops over the axes with
        // `astGetAxis` and reports 0 if any is a SkyAxis, whatever the stored
        // flag says. Otherwise the stored value, defaulting to false.
        if self.axes.iter().any(|a| matches!(a, FrameAxis::Sky(_))) {
            return false;
        }
        self.attrs.active_unit.unwrap_or(false)
    }

    /// The Axis attached to a 1-based `axis`. Mirrors C `astGetAxis`
    /// (frame.c:5309), which returns the Frame's own Axis object.
    fn get_axis(&self, axis: usize) -> AxisKind {
        self.internal(axis)
            .map(|i| self.axes[i].kind())
            .unwrap_or(AxisKind::Plain)
    }

    fn set_active_unit(&mut self, value: bool) {
        self.attrs.active_unit = Some(value);
    }

    fn test_active_unit(&self) -> bool {
        self.attrs.active_unit.is_some()
    }

    /// Mirrors C `Frame::Unformat` (frame.c:11819): axis.c `AxisUnformat`,
    /// or skyaxis.c `AxisUnformat` for a SkyAxis.
    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        match self.axis_object(axis)? {
            FrameAxis::Plain(_) => Ok(axis_unformat(text)),
            FrameAxis::Sky(s) => {
                let digits = self.digits();
                super::skyaxis::unformat_sky_axis(
                    text,
                    &s.format_value(digits),
                    s.digits_value(digits),
                )
            }
        }
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        let n = self.naxes;
        super::assert_points_len("distance", n, &[a, b]);
        Ok(super::cartesian_distance(&a[..n], &b[..n]))
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        // Frame-class algorithm (frame.c:1230-1320): signed in 2-D,
        // AST__BAD for bad inputs or zero-length vectors.
        let n = self.naxes;
        super::assert_points_len("angle", n, &[a, b, c]);
        Ok(super::cartesian_angle(&a[..n], &b[..n], &c[..n]))
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        let n = self.naxes;
        super::assert_points_len("offset", n, &[from, to]);
        Ok(super::cartesian_offset(&from[..n], &to[..n], dist))
    }

    /// frame.c `Norm`: each axis value is normalised by its own Axis
    /// object, so a SkyAxis picked out of a SkyFrame wraps its angle and a
    /// plain Axis leaves its value alone.
    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        super::assert_points_len("norm", self.naxes, &[&*coords]);
        for (value, &phys) in coords.iter_mut().zip(&self.perm) {
            if let FrameAxis::Sky(sky) = &self.axes[phys] {
                *value = sky.axis_norm(*value);
            }
        }
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    // Match-control attributes — mirror C `astGet/SetMinAxes` etc.
    /// Compose `perm` with the existing permutation, as C's `PermAxes`
    /// does (frame.c): the axes themselves stay in internal order.
    fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        super::validate_permutation(perm, self.naxes)?;
        let old = self.perm.clone();
        for (i, &p) in perm.iter().enumerate() {
            self.perm[i] = old[p - 1];
        }
        Ok(())
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        super::find_frame::integrity_mapping(previous, self)
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        // C Frame equality is conversion-based (frame.c:3549): same class,
        // astConvert succeeds, and the mapping simplifies to a UnitMap.
        super::find_frame::frames_equal(self, other)
    }

    fn match_template(
        &self,
        target: &dyn Frame,
        match_sub: bool,
    ) -> AstResult<Option<super::find_frame::MatchResult>> {
        super::find_frame::default_match(self, target, match_sub)
    }

    fn sub_frame(
        &self,
        template: Option<&dyn Frame>,
        _result_naxes: usize,
        target_axes: &[i32],
        template_axes: &[i32],
    ) -> AstResult<Option<(DynMapping, DynFrame)>> {
        super::find_frame::default_sub_frame(self, template, target_axes, template_axes)
    }

    fn pick_axes_snapshot(&self, target_axes: &[i32]) -> AstResult<DynFrame> {
        let mut snap = BasicFrameBuilder::new(target_axes.len()).build()?;
        if self.test_domain() {
            snap.set_domain(self.domain());
        }
        if let Some(ref title) = self.attrs.title {
            snap.set_title(title.clone());
        }
        for (i, &t) in target_axes.iter().enumerate() {
            if t < 0 || (t as usize) >= self.naxes {
                continue;
            }
            // C's SubFrame attaches the target's Axis object (frame.c:11023).
            snap.axes[i] = self.axes[self.perm[t as usize]].clone();
        }
        Ok(Box::new(snap))
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
        // Cartesian, the only System of a plain Frame, is valid only for a
        // plain Frame (frame.c `ValidateSystem`).
        if let Some(t) = template.downcast_ref::<BasicFrame>() {
            if t.system_set {
                self.system_set = true;
            }
            if t.align_system_set {
                self.align_system_set = true;
            }
        }
        Ok(())
    }
}

impl FrameDumpSource for BasicFrame {
    fn attrs(&self) -> &FrameAttrs {
        &self.attrs
    }
    fn dump_digits(&self) -> i32 {
        self.digits()
    }
    fn dump_obs(&self) -> [f64; 5] {
        [
            self.obs_lat(),
            self.obs_lon(),
            self.obs_alt(),
            self.dtai(),
            self.dut1(),
        ]
    }
    fn dump_naxes(&self) -> usize {
        self.naxes
    }
    fn dump_perm(&self) -> Vec<usize> {
        self.perm.clone()
    }
    fn dump_is_frame(&self) -> bool {
        true
    }
    fn dump_title(&self) -> AstResult<String> {
        Ok(self.title())
    }
    fn dump_domain(&self) -> String {
        self.domain().to_string()
    }
    fn dump_epoch(&self) -> f64 {
        self.epoch_mjd()
    }
    fn dump_system(&self) -> (bool, String) {
        (self.system_set, FrameSystem::Cartesian.as_str().to_string())
    }
    fn dump_align_system(&self) -> (bool, String) {
        (
            self.align_system_set,
            FrameSystem::Cartesian.as_str().to_string(),
        )
    }
    // The Frame dump reads each value with the Frame's getter on the
    // external axis of each internal one (frame.c:14358).
    fn dump_label(&self, axis: usize) -> String {
        self.label(self.external(axis)).unwrap_or_default()
    }
    fn dump_symbol(&self, axis: usize) -> String {
        self.symbol(self.external(axis)).unwrap_or_default()
    }
    fn dump_unit(&self, axis: usize) -> String {
        self.unit(self.external(axis)).unwrap_or_default()
    }
    fn dump_format(&self, axis: usize) -> String {
        self.format_str(self.external(axis)).unwrap_or_default()
    }
    fn dump_axis_digits(&self, axis: usize) -> Option<i32> {
        self.axes[axis].axis().digits
    }
    fn dump_direction(&self, axis: usize) -> bool {
        self.axes[axis].direction_value()
    }
    fn dump_bottom(&self, axis: usize) -> f64 {
        self.axes[axis].bottom_value()
    }
    fn dump_top(&self, axis: usize) -> f64 {
        self.axes[axis].top_value()
    }
    fn dump_preserve_axes(&self) -> bool {
        self.preserve_axes()
    }
    fn dump_permute(&self) -> bool {
        self.permute()
    }
    fn dump_min_axes(&self) -> usize {
        self.min_axes()
    }
    fn dump_max_axes(&self) -> usize {
        self.max_axes()
    }
    fn dump_match_end(&self) -> bool {
        self.match_end()
    }
    fn dump_active_unit(&self) -> Option<bool> {
        self.attrs.active_unit
    }
}

impl Serializable for BasicFrame {
    fn class_name(&self) -> &'static str {
        "Frame"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Coordinate system description".to_string());

        // Object part: Ident, written when set and non-empty (object.c
        // Dump), followed by the IsA Object marker when it was written.
        super::frame_attrs::write_object_part(
            &mut node,
            &self.attrs.id,
            self.attrs.ident.as_deref(),
            self.attrs.use_defs,
            crate::frames::Frame::use_defs(self),
        );

        write_frame_cards(&mut node, self)?;

        // Axis objects in internal order, each with only its set attributes
        // (frame.c:14660 dumps them with Full = -1).
        for (i, axis) in self.axes.iter().enumerate() {
            let axis_node = axis.to_dump_node();
            node.push_full_capped_object(
                &format!("Ax{}", i + 1),
                axis_node,
                Some(format!("Axis number {}", i + 1)),
            );
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // The Frame class's items come from its own part of the description,
        // as frame.c astLoadFrame reads them through astReadClassData.
        let whole = node;
        let node = &whole.section("Frame");
        // A missing or negative Naxes loads a Frame with no axes
        // (frame.c:15199), which the public builder does not make.
        let naxes = node.get_int("Naxes", 0).max(0) as usize;
        let mut frame = BasicFrame {
            naxes,
            attrs: FrameAttrs::default(),
            axes: vec![FrameAxis::default(); naxes],
            perm: (0..naxes).collect(),
            system_set: false,
            align_system_set: false,
        };
        frame.attrs = read_frame_attrs(whole);
        let cartesian = |name: &str| {
            name.eq_ignore_ascii_case(FrameSystem::Cartesian.as_str())
                .then_some(())
        };
        frame.system_set = read_system_item(node, "System", "System", cartesian)?.is_some();
        frame.align_system_set =
            read_system_item(node, "AlSys", "AlignSystem", cartesian)?.is_some();

        for (i, axis) in frame.axes.iter_mut().enumerate() {
            if let Some(axis_node) = node.get_object(&format!("Ax{}", i + 1)) {
                *axis = FrameAxis::read(axis_node)?;
            }
            axis.apply_frame_axis_keys(node, i + 1);
        }

        // The permutation, `Prm{n}` being the 1-based internal axis of
        // external axis n.
        for ext in 0..naxes {
            let p = node.get_int(&format!("Prm{}", ext + 1), ext as i64 + 1);
            if p >= 1 && (p as usize) <= naxes {
                frame.perm[ext] = p as usize - 1;
            }
        }
        super::validate_permutation(&frame.perm.iter().map(|p| p + 1).collect::<Vec<_>>(), naxes)?;

        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::PointSet;

    fn bf(naxes: usize) -> BasicFrame {
        BasicFrameBuilder::new(naxes).build().unwrap()
    }

    #[test]
    fn sub_frame_select_2_of_3_axes() {
        // Pick axes (1, 3) (zero-based 0, 2) from a 3-axis BasicFrame.
        let target = bf(3);
        let target_axes: Vec<i32> = vec![0, 2];
        let template_axes: Vec<i32> = vec![0, 1];
        let (map, frame) = target
            .sub_frame(None, 2, &target_axes, &template_axes)
            .unwrap()
            .unwrap();
        assert_eq!(frame.naxes(), 2);
        assert_eq!(map.nin(), 3);
        assert_eq!(map.nout(), 2);
        let pts = PointSet::from_rows([[10.0_f64, 20.0, 30.0]]);
        let out = map.transform(&pts, true).unwrap();
        let row = out.point(0);
        assert!((row[0] - 10.0).abs() < 1e-12);
        assert!((row[1] - 30.0).abs() < 1e-12);
    }

    #[test]
    fn match_template_basic_2axes_2axes() {
        let template = bf(2);
        let target = bf(2);
        let m = template.match_template(&target, false).unwrap().unwrap();
        assert_eq!(m.template_axes, vec![0, 1]);
        assert_eq!(m.target_axes, vec![0, 1]);
        assert_eq!(m.result.naxes(), 2);
        assert_eq!(m.map.nin(), 2);
        assert_eq!(m.map.nout(), 2);
    }

    #[test]
    fn match_template_basic_axis_mismatch() {
        let template = bf(3);
        let target = bf(2);
        let m = template.match_template(&target, false).unwrap();
        assert!(m.is_none(), "MaxAxes/MinAxes default to template's naxes");
    }

    #[test]
    fn match_template_basic_domain_mismatch() {
        let mut template = bf(2);
        template.set_domain("A");
        let mut target = bf(2);
        target.set_domain("B");
        let m = template.match_template(&target, false).unwrap();
        assert!(m.is_none(), "Domains differ; should fail to match");
    }

    #[test]
    fn digits_direction_round_trip() {
        let mut f = BasicFrameBuilder::new(2).build().unwrap();
        f.set_digits(9);
        f.set_axis_digits(1, 4).unwrap();
        f.set_direction(2, false).unwrap();

        let node = f.to_node().unwrap();
        let back = BasicFrame::from_node(&node).unwrap();
        assert_eq!(back.digits(), 9);
        assert_eq!(back.axis_digits(1).unwrap(), 4);
        assert!(!back.direction(2).unwrap());
    }

    #[test]
    fn sky_axis_forces_active_unit_off() {
        // C `Frame::GetActiveUnit` (frame.c:4669) loops over the axes with
        // `astGetAxis` and reports 0 if any of them is a SkyAxis, even after
        // the flag has been explicitly set.
        let mut f = BasicFrameBuilder::new(1).build().unwrap();
        f.set_active_unit(true);
        assert!(f.get_active_unit(), "plain frame honours the set flag");

        f.set_axis_object(1, FrameAxis::Sky(crate::frames::skyaxis::SkyAxis::new()));
        assert!(
            !f.get_active_unit(),
            "a frame carrying a SkyAxis must report ActiveUnit=0 regardless of the stored flag"
        );

        f.set_axis_object(1, FrameAxis::default());
        assert!(f.get_active_unit(), "clearing the axis restores the value");
    }
}
