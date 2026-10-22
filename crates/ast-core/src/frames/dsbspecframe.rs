//! The dual-sideband spectral coordinate frame (DSBSpecFrame).
//!
//! A DSBSpecFrame extends a SpecFrame to represent spectral position in a
//! dual-sideband instrument.
//! It adds a local oscillator (LO) frequency, given by a central position
//! (DSBCentre) and an intermediate frequency (IF) measured from it, and
//! records which sideband (USB, LSB, or an offset from the LO) the spectral
//! values refer to.

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame, FrameRef, SpecFrame, SpecSystem, StdOfRest};
use crate::mappings::{CmpMap, Composition, DynMapping, UnitMap, WinMap};
use crate::serial::{Node, NodeValue, Serializable};

// ---------------------------------------------------------------------------
// SideBand — which sideband the spectral values represent
// ---------------------------------------------------------------------------

/// Which sideband of a dual-sideband spectrum a [`DSBSpecFrame`] represents;
/// see [`DSBSpecFrame::sideband`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SideBand {
    /// The upper sideband, above the local oscillator frequency.
    USB,
    /// The lower sideband, below the local oscillator frequency.
    LSB,
    /// The topocentric offset from the local oscillator frequency.
    LO,
}

impl SideBand {
    /// Return the canonical string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::USB => "USB",
            Self::LSB => "LSB",
            Self::LO => "LO",
        }
    }

    /// Parse from string, ignoring case and trailing white space.
    // C: dsbspecframe.c `SetAttrib`, which matches with `astChrMatch`.
    pub fn parse(s: &str) -> Option<Self> {
        match crate::mappings::helpers::chr_match_key(s).as_str() {
            "USB" => Some(Self::USB),
            "LSB" => Some(Self::LSB),
            "LO" => Some(Self::LO),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// DSBSpecFrame
// ---------------------------------------------------------------------------

/// A dual sideband spectral coordinate system description.
///
/// A DSBSpecFrame is a specialized form of [`SpecFrame`] which represents
/// positions in a spectrum obtained using a dual sideband instrument.
/// Such an instrument produces a spectrum in which each point contains
/// contributions from two distinctly different frequencies, one from the
/// "lower side band" (LSB) and one from the "upper side band" (USB).
/// Corresponding LSB and USB frequencies are an equal distance on either
/// side of a fixed central frequency, the "local oscillator" (LO) frequency.
///
/// A position within such a spectrum must say whether it is the USB position
/// or the corresponding LSB position; the [`sideband`](DSBSpecFrame::sideband)
/// does this, and can also represent a spectral position by its topocentric
/// offset from the LO frequency.
///
/// In practice the LO frequency is given as the distance from it to some
/// "central" spectral position, typically that of an interesting spectral
/// feature ([`dsb_centre_cur`](DSBSpecFrame::dsb_centre_cur)).
/// That distance is the "intermediate frequency" (IF,
/// [`if_ghz`](DSBSpecFrame::if_ghz)), which is signed to say whether the LO
/// frequency is above or below the central position.
///
/// The SpecFrame attributes are reached through
/// [`spec`](DSBSpecFrame::spec) and [`spec_mut`](DSBSpecFrame::spec_mut),
/// with delegating methods for the commonest; the Domain defaults to
/// `"DSBSPECTRUM"`.
///
/// # Attributes
///
/// In addition to the attributes of a SpecFrame, a DSBSpecFrame has:
///
/// - [`align_sideband`](DSBSpecFrame::align_sideband): should alignment
///   occur between sidebands?
/// - [`dsb_centre_cur`](DSBSpecFrame::dsb_centre_cur): the central position
///   of interest.
/// - [`if_ghz`](DSBSpecFrame::if_ghz): the intermediate frequency used to
///   define the LO frequency.
/// - [`imag_freq_ghz`](DSBSpecFrame::imag_freq_ghz): the image sideband
///   equivalent of the rest frequency.
/// - [`sideband`](DSBSpecFrame::sideband): which sideband the DSBSpecFrame
///   represents.
///
/// # Examples
///
/// ```
/// use ast_core::frames::{DSBSpecFrame, SideBand, SpecSystem};
///
/// let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
/// assert_eq!(dsb.sideband(), SideBand::LSB);
/// dsb.set_sideband(SideBand::USB);
/// assert_eq!(dsb.sideband(), SideBand::USB);
/// ```
#[derive(Debug, Clone)]
pub struct DSBSpecFrame {
    /// The underlying spectral frame.
    spec: SpecFrame,

    /// DSBCentre as a topocentric frequency in Hz. Default: 3.0e10.
    dsb_centre: Option<f64>,

    /// Intermediate frequency in Hz. Default: 4.0e9.
    /// Sign determines the default sideband: positive → LSB, negative or zero → USB.
    ifr: Option<f64>,

    /// Explicitly set sideband. If None, derived from sign of IF.
    sideband: Option<SideBand>,

    /// Whether sideband alignment is enabled. Default: false.
    align_sideband: Option<bool>,
}

impl DSBSpecFrame {
    /// Default DSB centre frequency in Hz (30 GHz).
    const DEFAULT_DSB_CENTRE: f64 = 3.0e10;

    /// Default intermediate frequency in Hz (4 GHz).
    const DEFAULT_IFR: f64 = 4.0e9;

    /// Create a DSBSpecFrame describing the spectral coordinate system
    /// `system`.
    ///
    /// The System is set, as [`SpecFrame::new`] sets it; every other
    /// attribute takes its default, so DSBCentre is 30 GHz, IF is 4 GHz, the
    /// SideBand is the observed sideband (LSB) and AlignSideBand is false.
    ///
    /// ```
    /// use ast_core::frames::{DSBSpecFrame, SideBand, SpecSystem};
    ///
    /// let dsb = DSBSpecFrame::new(SpecSystem::Freq);
    /// assert_eq!(dsb.if_ghz(), 4.0);
    /// assert_eq!(dsb.sideband(), SideBand::LSB);
    /// // The LO is IF above the central position: 34 GHz.
    /// assert_eq!(dsb.lo_freq().unwrap(), 3.4e10);
    /// ```
    // C: `astDSBSpecFrame("System=...")`; dsbspecframe.c:531 `GetDomain`
    // supplies "DSBSPECTRUM" only as a default.
    #[doc(alias = "astDSBSpecFrame")]
    pub fn new(system: SpecSystem) -> Self {
        let spec = SpecFrame::new(system);
        Self {
            spec,
            dsb_centre: None,
            ifr: None,
            sideband: None,
            align_sideband: None,
        }
    }

    /// Create a DSBSpecFrame with no attribute set; see
    /// [`SpecFrame::unset_system`].
    // C: `astDSBSpecFrame("")`.
    #[doc(alias = "astDSBSpecFrame")]
    pub fn unset_system() -> Self {
        Self::from_spec_frame(SpecFrame::unset_system())
    }

    /// Whether DSBCentre is set; see
    /// [`dsb_centre_cur`](DSBSpecFrame::dsb_centre_cur).
    pub fn test_dsb_centre(&self) -> bool {
        self.dsb_centre.is_some()
    }

    /// Clear DSBCentre; see
    /// [`dsb_centre_cur`](DSBSpecFrame::dsb_centre_cur).
    pub fn clear_dsb_centre(&mut self) {
        self.dsb_centre = None;
    }

    /// Whether IF is set; see [`if_ghz`](DSBSpecFrame::if_ghz).
    pub fn test_if(&self) -> bool {
        self.ifr.is_some()
    }

    /// Clear IF; see [`if_ghz`](DSBSpecFrame::if_ghz).
    pub fn clear_if(&mut self) {
        self.ifr = None;
    }

    /// Whether SideBand is set; see [`sideband`](DSBSpecFrame::sideband).
    pub fn test_sideband(&self) -> bool {
        self.sideband.is_some()
    }

    /// Clear SideBand, which then follows the sign of IF; see
    /// [`sideband`](DSBSpecFrame::sideband).
    pub fn clear_sideband(&mut self) {
        self.sideband = None;
    }

    /// Whether AlignSideBand is set; see
    /// [`align_sideband`](DSBSpecFrame::align_sideband).
    pub fn test_align_sideband(&self) -> bool {
        self.align_sideband.is_some()
    }

    /// Clear AlignSideBand; see
    /// [`align_sideband`](DSBSpecFrame::align_sideband).
    pub fn clear_align_sideband(&mut self) {
        self.align_sideband = None;
    }

    /// Create a `DSBSpecFrame` wrapping an existing [`SpecFrame`].
    ///
    /// All DSB-specific attributes (DSBCentre, IF, SideBand, AlignSideBand)
    /// start at their defaults; the supplied `SpecFrame` provides the
    /// spectral System / StdOfRest / RestFreq / observer / Epoch attributes.
    pub fn from_spec_frame(spec: SpecFrame) -> Self {
        Self {
            spec,
            dsb_centre: None,
            ifr: None,
            sideband: None,
            align_sideband: None,
        }
    }

    // --- DSB-specific attribute getters ---

    /// DSBCentre as it is stored, a topocentric frequency in Hz, set or
    /// default; see [`dsb_centre_cur`](DSBSpecFrame::dsb_centre_cur).
    ///
    /// Default: 3.0E10 Hz (30 GHz).
    #[doc(alias = "DSBCentre")]
    pub fn dsb_centre(&self) -> f64 {
        self.dsb_centre.unwrap_or(Self::DEFAULT_DSB_CENTRE)
    }

    /// The central position of interest in a dual sideband spectrum: the
    /// DSBCentre attribute, set or default, in the current spectral system
    /// and units.
    ///
    /// Its sole use is to determine the local oscillator frequency, the
    /// frequency which marks the boundary between the lower and upper
    /// sidebands; [`if_ghz`](DSBSpecFrame::if_ghz) says how.
    /// The sideband containing this central position is the "observed"
    /// sideband, and the other is the "image" sideband.
    ///
    /// The value is a position in the spectral system the SpecFrame
    /// attributes describe, but is stored as a topocentric frequency
    /// ([`dsb_centre`](DSBSpecFrame::dsb_centre)).
    /// If the System is radio velocity, the Unit `"m/s"` and the StdOfRest
    /// LSRK, for instance, the value is a radio velocity in m/s relative to
    /// the kinematic LSR; if the Unit then changes to `"km/s"`, the stored
    /// frequency is converted back to LSRK radio velocity in km/s.
    /// Give the attributes which define the transformation to or from
    /// topocentric frequency their correct values before using this
    /// attribute: potentially System, Unit, StdOfRest, ObsLon, ObsLat,
    /// ObsAlt, Epoch, RefRA, RefDec and RestFreq.
    ///
    /// Default: 30 GHz, as a topocentric frequency.
    ///
    /// # Errors
    ///
    /// Passes on any error from converting between the spectral system and
    /// topocentric frequency, such as `ErrorCode::Inter` when no conversion
    /// is found.
    // C: `astGetAttrib(this, "DSBCentre")` (dsbspecframe.c:459-483).
    #[doc(alias = "DSBCentre")]
    pub fn dsb_centre_cur(&self) -> AstResult<f64> {
        // TopoMap forward is current-system → topocentric Hz; the get-path
        // needs the inverse (topocentric Hz → current system).
        let topo_map = self.to_topo_mapping()?;
        let pts = crate::coords::PointSet::from_rows([[self.dsb_centre()]]);
        let out = topo_map.transform(&pts, false)?;
        Ok(out.point(0)[0])
    }

    /// Set DSBCentre from a value in the current spectral system and units,
    /// stored as a topocentric frequency; see
    /// [`dsb_centre_cur`](DSBSpecFrame::dsb_centre_cur).
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Attin` when the value has no topocentric frequency (a
    ///   redshift of -1, for instance); DSBCentre is then unchanged.
    /// - Passes on any error from converting the spectral system to
    ///   topocentric frequency, such as `ErrorCode::Inter` when no
    ///   conversion is found.
    // C: `astSetAttrib(this, "DSBCentre=...")` (dsbspecframe.c:1355-1370).
    pub fn set_dsb_centre_cur(&mut self, value: f64) -> AstResult<()> {
        let topo_map = self.to_topo_mapping()?;
        let pts = crate::coords::PointSet::from_rows([[value]]);
        let out = topo_map.transform(&pts, true)?;
        let centre = out.point(0)[0];
        // A value with no topocentric frequency is rejected
        // (dsbspecframe.c:1366).
        if crate::mappings::helpers::is_bad(centre) {
            return Err(AstError::new(
                ErrorCode::Attin,
                format!(
                    "astSetAttrib(DSBSpecFrame): The setting \"dsbcentre={value}\" is invalid \
                     for a DSBSpecFrame."
                ),
            ));
        }
        self.dsb_centre = Some(centre);
        Ok(())
    }

    /// IF in Hz, set or default; see [`if_ghz`](DSBSpecFrame::if_ghz).
    ///
    /// Default: 4.0E9 Hz (4 GHz).
    #[doc(alias = "IF")]
    pub fn ifr(&self) -> f64 {
        self.ifr.unwrap_or(Self::DEFAULT_IFR)
    }

    /// The intermediate frequency in a dual sideband spectrum, in GHz: the
    /// IF attribute, set or default.
    ///
    /// The (topocentric) intermediate frequency, whose sole use is to
    /// determine the local oscillator (LO) frequency, the frequency which
    /// marks the boundary between the lower and upper sidebands.
    /// The LO frequency is the sum of the center frequency and the IF, where
    /// the center frequency is the topocentric frequency corresponding to
    /// [`dsb_centre_cur`](DSBSpecFrame::dsb_centre_cur);
    /// [`lo_freq`](DSBSpecFrame::lo_freq) gives it.
    /// A positive IF puts the LO frequency above the central frequency, a
    /// negative one below it, and the sign decides the default
    /// [`sideband`](DSBSpecFrame::sideband).
    /// [`ifr`](DSBSpecFrame::ifr) gives the value in Hz.
    ///
    /// Default: 4 GHz.
    // C: dsbspecframe.c:487-492.
    #[doc(alias = "IF")]
    pub fn if_ghz(&self) -> f64 {
        self.ifr() * 1.0e-9
    }

    /// Set IF, in GHz; see [`if_ghz`](DSBSpecFrame::if_ghz).
    pub fn set_if_ghz(&mut self, ghz: f64) {
        self.ifr = Some(ghz * 1.0e9);
    }

    /// The image sideband equivalent of the rest frequency, in GHz: the
    /// read-only ImagFreq attribute.
    ///
    /// The frequency which corresponds to the rest frequency but is in the
    /// opposite sideband.
    /// The rest frequency ([`SpecFrame::rest_freq`]) is transformed from the
    /// standard of rest of the source (given by SourceVel and SourceVRF) to
    /// the topocentric standard of rest.
    /// That topocentric frequency is taken to be in the observed sideband,
    /// the one containing [`dsb_centre_cur`](DSBSpecFrame::dsb_centre_cur),
    /// and is transformed to the image sideband.
    /// The result is converted back to the standard of rest of the source.
    /// [`imag_freq`](DSBSpecFrame::imag_freq) gives it in Hz.
    ///
    /// # Errors
    ///
    /// As [`imag_freq`](DSBSpecFrame::imag_freq).
    // C: dsbspecframe.c:496-501.
    #[doc(alias = "ImagFreq")]
    pub fn imag_freq_ghz(&self) -> AstResult<f64> {
        Ok(self.imag_freq()? * 1.0e-9)
    }

    /// Which sideband the DSBSpecFrame represents: the SideBand attribute,
    /// set or default.
    ///
    /// [`SideBand::USB`] for the upper sideband, [`SideBand::LSB`] for the
    /// lower sideband, or [`SideBand::LO`] for an offset from the local
    /// oscillator frequency.
    /// The "observed" sideband is whichever of the upper and lower contains
    /// the central position [`dsb_centre_cur`](DSBSpecFrame::dsb_centre_cur),
    /// and the "image" sideband is the other; the sign of the IF decides
    /// which is which.
    /// [`observed_sideband`](DSBSpecFrame::observed_sideband) gives the
    /// observed one, to set it by name, and the image sideband is the other
    /// of [`SideBand::USB`] and [`SideBand::LSB`]:
    ///
    /// ```
    /// use ast_core::frames::{DSBSpecFrame, SideBand, SpecSystem};
    ///
    /// let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    /// let image = match dsb.observed_sideband() {
    ///     SideBand::LSB => SideBand::USB,
    ///     _ => SideBand::LSB,
    /// };
    /// dsb.set_sideband(image);
    /// assert_eq!(dsb.sideband(), SideBand::USB);
    /// ```
    ///
    /// Default: the observed sideband: [`SideBand::LSB`] for a positive IF,
    /// [`SideBand::USB`] otherwise.
    #[doc(alias = "SideBand")]
    pub fn sideband(&self) -> SideBand {
        if let Some(sb) = self.sideband {
            return sb;
        }
        // Default: derive from IF sign.
        let ifr = self.ifr();
        if ifr > 0.0 {
            SideBand::LSB
        } else {
            SideBand::USB
        }
    }

    /// Should the SideBand be taken into account when aligning this
    /// DSBSpecFrame with another? The AlignSideBand attribute, set or
    /// default.
    ///
    /// Controls how a DSBSpecFrame behaves when it is aligned with another
    /// DSBSpecFrame by [`find_frame_template`](Frame::find_frame_template)
    /// or [`convert_to`](Frame::convert_to).
    /// If both have AlignSideBand true, the [`sideband`](DSBSpecFrame::sideband)
    /// of each is used so that alignment occurs between sidebands: if one
    /// represents USB and the other LSB, the Mapping takes the difference
    /// into account.
    /// If either has it false, the SideBands are ignored and the two align
    /// as plain SpecFrames, so that a frequency in one maps onto the same
    /// frequency in the other even though they refer to different sidebands.
    ///
    /// The Mapping between two DSBSpecFrames is made up of these steps in
    /// order:
    ///
    /// 1. If both have AlignSideBand true, map values from the target's
    ///    current sideband to its observed sideband (a [`UnitMap`] if the
    ///    target already represents the observed sideband).
    /// 2. Map the values from the spectral system of the target to that of
    ///    the template, taking account of all the SpecFrame attributes, such
    ///    as System, StdOfRest and Unit.
    /// 3. If both have AlignSideBand true, map values from the result's
    ///    observed sideband to its current sideband.
    ///
    /// Default: false.
    #[doc(alias = "AlignSideBand")]
    pub fn align_sideband(&self) -> bool {
        self.align_sideband.unwrap_or(false)
    }

    // --- DSB-specific attribute setters ---

    /// Set DSBCentre, as a topocentric frequency in Hz; see
    /// [`dsb_centre_cur`](DSBSpecFrame::dsb_centre_cur).
    pub fn set_dsb_centre(&mut self, hz: f64) {
        self.dsb_centre = Some(hz);
    }

    /// Set IF, in Hz; see [`if_ghz`](DSBSpecFrame::if_ghz).
    pub fn set_ifr(&mut self, hz: f64) {
        self.ifr = Some(hz);
    }

    /// Set the SideBand; see [`sideband`](DSBSpecFrame::sideband).
    pub fn set_sideband(&mut self, sb: SideBand) {
        self.sideband = Some(sb);
    }

    /// Set AlignSideBand; see
    /// [`align_sideband`](DSBSpecFrame::align_sideband).
    pub fn set_align_sideband(&mut self, val: bool) {
        self.align_sideband = Some(val);
    }

    // --- SpecFrame delegators ---

    /// Return the spectral coordinate system.
    pub fn system(&self) -> SpecSystem {
        self.spec.system()
    }

    /// Set the spectral coordinate system. Fails as
    /// [`SpecFrame::set_system`] does.
    pub fn set_system(&mut self, sys: SpecSystem) -> AstResult<()> {
        self.spec.set_system(sys)
    }

    /// Return the standard of rest.
    pub fn std_of_rest(&self) -> StdOfRest {
        self.spec.std_of_rest()
    }

    /// Set the standard of rest. Fails as
    /// [`SpecFrame::set_std_of_rest`] does.
    pub fn set_std_of_rest(&mut self, sor: StdOfRest) -> AstResult<()> {
        self.spec.set_std_of_rest(sor)
    }

    /// RestFreq in Hz, set or default.
    pub fn rest_freq(&self) -> f64 {
        self.spec.rest_freq()
    }

    /// Set the rest frequency in Hz.
    pub fn set_rest_freq(&mut self, hz: f64) {
        self.spec.set_rest_freq(hz);
    }

    /// Set the epoch as a TDB Modified Julian Date.
    pub fn set_epoch_mjd(&mut self, mjd: f64) {
        self.spec.set_epoch_mjd(mjd);
    }

    /// Set the epoch from a decimal year (IAU 1984 rule).
    pub fn set_epoch_year(&mut self, year: f64) {
        self.spec.set_epoch_year(year);
    }

    /// Return the epoch as a TDB Modified Julian Date.
    pub fn epoch_mjd(&self) -> f64 {
        self.spec.epoch_mjd()
    }

    /// Return the epoch as a decimal year (Besselian / Julian per
    /// the IAU 1984 rule).
    pub fn epoch_year(&self) -> f64 {
        self.spec.epoch_year()
    }

    /// Set the observer geodetic latitude (radians).
    pub fn set_obs_lat(&mut self, lat: f64) {
        self.spec.set_obs_lat(lat);
    }

    /// Set the observer geodetic longitude (radians).
    pub fn set_obs_lon(&mut self, lon: f64) {
        self.spec.set_obs_lon(lon);
    }

    /// Set the observer altitude (metres).
    pub fn set_obs_alt(&mut self, alt: f64) {
        self.spec.set_obs_alt(alt);
    }

    /// Set TAI-UTC (seconds).
    pub fn set_dtai(&mut self, dtai: f64) {
        self.spec.set_dtai(dtai);
    }

    /// Set UT1-UTC (seconds).
    pub fn set_dut1(&mut self, dut1: f64) {
        self.spec.set_dut1(dut1);
    }

    // --- Internal accessors ---

    /// Return a reference to the underlying `SpecFrame`.
    pub fn spec(&self) -> &SpecFrame {
        &self.spec
    }

    /// Return a mutable reference to the underlying `SpecFrame`.
    pub fn spec_mut(&mut self) -> &mut SpecFrame {
        &mut self.spec
    }

    /// Set the reference position (RefRA/RefDec) from a celestial frame.
    ///
    /// Delegates to the underlying [`SpecFrame::set_ref_pos`].
    pub fn set_ref_pos(
        &mut self,
        frm: Option<&super::SkyFrame>,
        lon: f64,
        lat: f64,
    ) -> AstResult<()> {
        self.spec.set_ref_pos(frm, lon, lat)
    }

    /// RefRA (FK5 J2000, radians), set or default.
    pub fn ref_ra(&self) -> f64 {
        self.spec.ref_ra()
    }

    /// RefDec (FK5 J2000, radians), set or default.
    pub fn ref_dec(&self) -> f64 {
        self.spec.ref_dec()
    }

    // --- DSBSetUp (FITS-WCS / FITS-CLASS read path) ---

    /// Configure this DSBSpecFrame's IF / DSBCentre / SideBand from a FITS
    /// header's image frequency and spectral CRVAL.
    ///
    /// `imagfreq` is the `IMAGFREQ` keyword value (Hz, measured in the
    /// source rest frame);
    /// `crval` is the spectral CRVAL value expressed in this frame's current
    /// spectral System and Unit (the CTYPE system).
    ///
    /// The algorithm:
    /// 1. Build a mapping from frequency in the source's rest frame to
    ///    topocentric frequency, by converting between a copy of this frame
    ///    set to (StdOfRest=Source, System=FREQ, Unit=Hz) and the same copy
    ///    set to StdOfRest=Topocentric.
    /// 2. Transform `[RestFreq, imagfreq]` through that mapping to obtain the
    ///    topocentric rest and image frequencies.
    /// 3. If both are finite, store `crval` as the DSBCentre, read it back as
    ///    a topocentric frequency, take the local-oscillator frequency to be
    ///    half-way between the topocentric rest and image frequencies, set
    ///    `IF = lo - dsbcentre`, and set the SideBand to the observed sideband.
    ///
    /// # Errors
    ///
    /// Passes on any error from converting between the spectral systems.
    // C: fitschan.c:9024-9158 `DSBSetUp`.
    pub fn dsb_set_up(&mut self, imagfreq: f64, crval: f64) -> AstResult<()> {
        use crate::frames::Frame;

        let bad = crate::constants::BAD_VALUE;

        // Step 1 (C:9000-9015). Build dsb_src = copy of the underlying
        // SpecFrame set to source-rest frequency in Hz, and dsb_topo = the
        // same set to topocentric. astConvert(dsb_src, dsb_topo, "") gives a
        // mapping source-rest-FREQ-Hz -> topocentric-FREQ-Hz.
        let mut dsb_src = self.spec.clone();
        dsb_src.set_std_of_rest(StdOfRest::Source)?;
        dsb_src.set_system(SpecSystem::Freq)?;
        dsb_src.set_unit(1, "Hz")?;

        let mut dsb_topo = dsb_src.clone();
        dsb_topo.set_std_of_rest(StdOfRest::Topocentric)?;

        let Some(fs) = crate::frames::find_frame::convert(&dsb_src, &dsb_topo, &[])? else {
            // No conversion found (C:9018 — leaves the frame unchanged).
            return Ok(());
        };
        let map = fs.mapping(
            crate::frames::FrameRef::Base,
            crate::frames::FrameRef::Current,
        )?;

        // Step 2 (C:9020-9025). Transform [RestFreq, imagfreq].
        // fitschan.c:9203 reads RestFreq, set or default.
        let rest_freq = self.spec.rest_freq();
        // A 1-input/1-output mapping applied to two points (rest, image),
        // mirroring C `astTran1(fs, 2, in, 1, out)`. The single coordinate
        // axis holds both input values.
        let input = PointSet::from_columns(&[&[rest_freq, imagfreq]])?;
        let result = map.transform(&input, true)?;
        let out0 = result.point(0)[0];
        let out1 = result.point(1)[0];

        // Step 3 (C:9030-9059). Only proceed when both outputs are good.
        if out0 != bad && out1 != bad && out0.is_finite() && out1.is_finite() {
            // Store CRVAL as the DSBCentre, interpreting it in the current
            // spectral system (C:9036 astSetD(dsb,"DSBCentre",crval)).
            self.set_dsb_centre_cur(crval)?;

            // Topocentric-frequency equivalent of the DSBCentre: copy the
            // frame, set it to topocentric FREQ in Hz, read DSBCentre back
            // (C:9041-9046).
            let mut topo = self.clone();
            topo.spec.set_std_of_rest(StdOfRest::Topocentric)?;
            topo.spec.set_system(SpecSystem::Freq)?;
            topo.spec.set_unit(1, "Hz")?;
            let dsbcentre = topo.dsb_centre_cur()?;

            // Local-oscillator frequency: half-way between topocentric rest
            // and image frequencies (C:9051).
            let lo = 0.5 * (out1 + out0);

            // IF = lo - dsbcentre (C:9055). Internal IF is stored in Hz.
            self.set_ifr(lo - dsbcentre);

            // SideBand=observed (C:9058): IF>0 -> LSB, else USB.
            self.set_sideband(self.observed_sideband());
        }

        Ok(())
    }

    // --- Helpers ---

    /// Compute the observed frequency based on the current sideband, DSB centre, and IF.
    ///
    /// - USB: observed = dsb_centre + |ifr|
    /// - LSB or LO: observed = dsb_centre - |ifr|
    pub fn observed_freq(&self) -> f64 {
        let centre = self.dsb_centre();
        let ifr_abs = self.ifr().abs();
        match self.sideband() {
            SideBand::USB => centre + ifr_abs,
            SideBand::LSB | SideBand::LO => centre - ifr_abs,
        }
    }

    /// Return which sideband the DSB centre frequency is in.
    ///
    /// If IF > 0 the DSB centre is in the lower sideband (LO is above);
    /// otherwise it is in the upper sideband.
    pub fn observed_sideband(&self) -> SideBand {
        if self.ifr() > 0.0 {
            SideBand::LSB
        } else {
            SideBand::USB
        }
    }

    /// The local oscillator frequency, in Hz: the topocentric
    /// [`dsb_centre`](DSBSpecFrame::dsb_centre) plus the
    /// [`ifr`](DSBSpecFrame::ifr).
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Attin` when the result is less than the size of the
    ///   IF, which a bad IF or DSBCentre can cause.
    // C: dsbspecframe.c `GetLO`.
    pub fn lo_freq(&self) -> AstResult<f64> {
        let f_if = self.ifr();
        let lo = self.dsb_centre() + f_if;
        if lo < f_if.abs() {
            return Err(AstError::new(
                ErrorCode::Attin,
                format!(
                    "astGetLO(DSBSpecFrame): The local oscillator frequency ({lo} Hz) is too \
                     low (less than the intermediate frequency: {} Hz). This could be \
                     caused by a bad value for either the IF attribute (currently {f_if} Hz) \
                     or the DSBCentre attribute (currently {} Hz).",
                    f_if.abs(),
                    self.dsb_centre()
                ),
            ));
        }
        Ok(lo)
    }

    /// ImagFreq in Hz; see [`imag_freq_ghz`](DSBSpecFrame::imag_freq_ghz).
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Noval` when UseDefs is false and IF or DSBCentre is
    ///   not set.
    /// - `ErrorCode::Attin` when the LO frequency is less than the size of
    ///   the IF ([`lo_freq`](DSBSpecFrame::lo_freq)).
    /// - Passes on any error from converting between the spectral system
    ///   and topocentric frequency.
    // C: dsbspecframe.c:599 `GetImagFreq`. The rest frequency (set or
    // default, dsbspecframe.c:673) is taken as an observed-sideband
    // frequency in the source's rest frame, and transformed by the
    // observed-to-image Mapping of a copy with StdOfRest=Source, System=Freq,
    // Unit=Hz and SideBand=observed.
    #[doc(alias = "ImagFreq")]
    pub fn imag_freq(&self) -> AstResult<f64> {
        let rest_freq = self.spec.rest_freq();

        // Copy and reset to canonical "rest-frequency frame":
        // SoR=Source, System=Freq, Unit=Hz, SideBand=observed. This
        // matches C's `astSetStdOfRest(rf_frame, AST__SCSOR);
        // astSetSystem(rf_frame, AST__FREQ); astSetUnit(rf_frame, 0,
        // "Hz"); astSetC(rf_frame, "SideBand", "observed");`.
        let mut rf_frame = self.clone();
        rf_frame.spec.set_std_of_rest(StdOfRest::Source)?;
        rf_frame.spec.set_system(SpecSystem::Freq)?;
        // C sets Unit explicitly to "Hz" (dsbspecframe.c:653); the
        // sideband flip and topocentric mappings all work in Hz, so the
        // current unit must be Hz (not the FREQ display default of GHz).
        rf_frame.spec.set_unit(1, "Hz")?;
        rf_frame.set_sideband(self.observed_sideband());

        // Mapping from observed sideband to the opposite sideband.
        let map = match self.observed_sideband() {
            SideBand::USB => rf_frame.to_lsb_mapping()?,
            SideBand::LSB => rf_frame.to_usb_mapping()?,
            SideBand::LO => {
                return Err(AstError::new(
                    ErrorCode::Inter,
                    "DSBSpecFrame::imag_freq: observed sideband cannot be LO",
                ));
            }
        };

        let input = PointSet::from_columns(&[&[rest_freq]])?;
        let result = map.transform(&input, true)?;
        Ok(result.point(0)[0])
    }

    /// Check that the IF and DSBCentre named in `attrs` are set, where
    /// UseDefs is false, reporting AST__NOVAL for `purp` otherwise. Port of
    /// `VerifyAttrs` (dsbspecframe.c:2286), which `GetLO` calls before the
    /// sideband Mappings use the LO (dsbspecframe.c:821).
    fn verify_attrs(&self, purp: &str, attrs: &str) -> AstResult<()> {
        if Frame::use_defs(self) {
            return Ok(());
        }
        for attr in attrs.split_whitespace() {
            let (set, desc) = match attr {
                "DSBCentre" => (self.test_dsb_centre(), "central position of interest"),
                "IF" => (self.test_if(), "intermediate frequency"),
                other => unreachable!("VerifyAttrs has no attribute {other}"),
            };
            if !set {
                return Err(AstError::new(
                    ErrorCode::Noval,
                    format!(
                        "astMatch(DSBSpecFrame): Cannot {purp}. No value has been set for the \
                         AST \"{attr}\" attribute ({desc})."
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Create a mapping from the current spectral system/SOR to topocentric
    /// frequency in Hz. Mirrors C `TopoMap` (dsbspecframe.c:2051): the
    /// SpecFrame attributes of this DSBSpecFrame are overlaid onto a new
    /// SpecFrame, which is aligned in the topocentric rest frame and
    /// converted to a copy of itself set to topocentric frequency in Hz.
    /// The copy keeps the SpecOrigin, re-expressed for its System and
    /// StdOfRest, so the origin and AlignSpecOffset take part.
    fn to_topo_mapping(&self) -> AstResult<DynMapping> {
        let mut tf1 = SpecFrame::unset_system();
        self.spec.overlay(&[0], &mut tf1)?;
        tf1.set_align_std_of_rest(StdOfRest::Topocentric);
        let mut tf2 = tf1.clone();
        tf2.set_system(SpecSystem::Freq)?;
        tf2.set_std_of_rest(StdOfRest::Topocentric)?;
        tf2.set_unit(1, "Hz")?;
        let Some(fs) = crate::frames::find_frame::convert(&tf1, &tf2, &[])? else {
            return Err(AstError::new(
                ErrorCode::Inter,
                "Cannot convert DSBCentre value from the supplied system to \
                 topocentric frequency (internal AST programming error).",
            ));
        };
        fs.mapping(FrameRef::Base, FrameRef::Current)
    }

    /// Create a mapping that converts frequencies from the current sideband
    /// to the upper sideband (USB).
    ///
    /// If already USB, returns a unit (identity) mapping.
    pub fn to_usb_mapping(&self) -> AstResult<DynMapping> {
        if self.sideband() == SideBand::USB {
            return Ok(Box::new(UnitMap::new(1)));
        }
        self.verify_attrs("create a Mapping to another sideband", "IF DSBCentre")?;
        let f_lo = self.lo_freq()?;
        let topo_map = self.to_topo_mapping()?;
        let flip: DynMapping = match self.sideband() {
            SideBand::LSB => {
                // LSB→USB: output = 2*f_lo - input
                Box::new(WinMap::new(&[2.0 * f_lo], &[-1.0])?)
            }
            SideBand::LO => {
                // LO offset→USB: output = f_lo + offset
                Box::new(WinMap::new(&[f_lo], &[1.0])?)
            }
            SideBand::USB => unreachable!(),
        };
        self.compose_sideband_mapping(topo_map, flip)
    }

    /// Create a mapping that converts frequencies from the current sideband
    /// to the lower sideband (LSB).
    ///
    /// If already LSB, returns a unit (identity) mapping.
    pub fn to_lsb_mapping(&self) -> AstResult<DynMapping> {
        if self.sideband() == SideBand::LSB {
            return Ok(Box::new(UnitMap::new(1)));
        }
        self.verify_attrs("create a Mapping to another sideband", "IF DSBCentre")?;
        let f_lo = self.lo_freq()?;
        let topo_map = self.to_topo_mapping()?;
        let flip: DynMapping = match self.sideband() {
            SideBand::USB => {
                // USB→LSB: output = 2*f_lo - input
                Box::new(WinMap::new(&[2.0 * f_lo], &[-1.0])?)
            }
            SideBand::LO => {
                // LO offset→LSB: output = f_lo - offset
                Box::new(WinMap::new(&[f_lo], &[-1.0])?)
            }
            SideBand::LSB => unreachable!(),
        };
        self.compose_sideband_mapping(topo_map, flip)
    }

    /// Create a mapping that converts frequencies from the current sideband
    /// to the LO offset representation.
    ///
    /// If already LO, returns a unit (identity) mapping.
    pub fn to_lo_mapping(&self) -> AstResult<DynMapping> {
        if self.sideband() == SideBand::LO {
            return Ok(Box::new(UnitMap::new(1)));
        }
        self.verify_attrs("create a Mapping to another sideband", "IF DSBCentre")?;
        let f_lo = self.lo_freq()?;
        let topo_map = self.to_topo_mapping()?;
        let flip: DynMapping = match self.sideband() {
            SideBand::USB => {
                // USB→LO: output = freq - f_lo
                Box::new(WinMap::new(&[-f_lo], &[1.0])?)
            }
            SideBand::LSB => {
                // LSB→LO: output = f_lo - freq
                Box::new(WinMap::new(&[f_lo], &[-1.0])?)
            }
            SideBand::LO => unreachable!(),
        };
        self.compose_sideband_mapping(topo_map, flip)
    }

    /// Compose a sideband conversion mapping: topo_map → flip → topo_map⁻¹.
    ///
    /// This converts from the current spectral system to topocentric frequency,
    /// applies the sideband flip in topocentric frequency space, then converts
    /// back to the original system.
    fn compose_sideband_mapping(
        &self,
        topo_map: DynMapping,
        flip: DynMapping,
    ) -> AstResult<DynMapping> {
        // Build the inverse topo mapping (topo freq → current system).
        let mut inv_topo = self.to_topo_mapping()?;
        inv_topo.invert();

        // Compose: topo_map → flip → inv_topo, simplified as ToLOMapping,
        // ToLSBMapping and ToUSBMapping do (dsbspecframe.c:1916, 2034, 2255).
        let step1 = CmpMap::new(topo_map, flip, Composition::Series)?;
        let result: DynMapping =
            Box::new(CmpMap::new(Box::new(step1), inv_topo, Composition::Series)?);
        result.simplify()
    }

    /// Mapping from current sideband to the "observed" sideband.
    ///
    /// The observed sideband is determined by the sign of the IF:
    /// positive IF → observed is LSB, negative/zero IF → observed is USB.
    fn to_observed_mapping(&self) -> AstResult<DynMapping> {
        match self.observed_sideband() {
            SideBand::LSB => self.to_lsb_mapping(),
            SideBand::USB => self.to_usb_mapping(),
            SideBand::LO => self.to_lo_mapping(),
        }
    }
}

// ---------------------------------------------------------------------------
// Frame trait implementation
// ---------------------------------------------------------------------------

impl Frame for DSBSpecFrame {
    // A DSBSpecFrame is a SpecFrame in C, so its Frame-class attributes,
    // and its Object-class Ident, are the SpecFrame's; only the Domain
    // default is its own (dsbspecframe.c `GetDomain`).
    forward_frame_attrs!(@all_but_domain [spec]);
    fn frame_flags(&self) -> u32 {
        self.spec.frame_flags()
    }
    fn set_frame_flags(&mut self, flags: u32) {
        self.spec.set_frame_flags(flags);
    }
    /// The coordinate system domain; see [`Frame::domain`].
    ///
    /// Default: `"DSBSPECTRUM"`.
    fn domain(&self) -> &str {
        if self.spec.test_domain() {
            Frame::domain(&self.spec)
        } else {
            "DSBSPECTRUM"
        }
    }
    fn set_domain(&mut self, domain: &str) -> AstResult<()> {
        Frame::set_domain(&mut self.spec, domain)
    }
    fn test_domain(&self) -> bool {
        Frame::test_domain(&self.spec)
    }
    fn clear_domain(&mut self) -> AstResult<()> {
        Frame::clear_domain(&mut self.spec)
    }
    fn ident(&self) -> &str {
        Frame::ident(&self.spec)
    }
    fn set_ident(&mut self, ident: &str) -> AstResult<()> {
        Frame::set_ident(&mut self.spec, ident)
    }
    fn clear_ident(&mut self) -> AstResult<()> {
        Frame::clear_ident(&mut self.spec)
    }
    fn test_ident(&self) -> bool {
        Frame::test_ident(&self.spec)
    }

    fn id(&self) -> &str {
        Frame::id(&self.spec)
    }
    fn set_id(&mut self, id: &str) -> AstResult<()> {
        Frame::set_id(&mut self.spec, id)
    }
    fn clear_id(&mut self) -> AstResult<()> {
        Frame::clear_id(&mut self.spec)
    }
    fn test_id(&self) -> bool {
        Frame::test_id(&self.spec)
    }
    fn stored_use_defs(&self) -> Option<bool> {
        self.spec.stored_use_defs()
    }
    fn store_use_defs(&mut self, value: Option<bool>) {
        self.spec.store_use_defs(value);
    }

    fn naxes(&self) -> usize {
        1
    }

    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        (self, axis)
    }

    // Ident is an Object-level attribute (`object.c:5217-5229`), and the
    // component SpecFrame supplies the storage the dump reads back.

    /// The label of the axis; see [`Frame::label`].
    ///
    /// The default is the SpecFrame's, followed by the sideband, as in
    /// `"Frequency (LSB)"`.
    // C: dsbspecframe.c:688 `GetLabel`.
    fn label(&self, axis: usize) -> AstResult<String> {
        let label = Frame::label(&self.spec, axis)?;
        Ok(if Frame::test_label(&self.spec, axis)? {
            label
        } else {
            format!("{label} ({})", self.sideband().as_str())
        })
    }

    forward_axis_attrs!(@all_but_label [spec]);

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        self.spec.distance(a, b)
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        self.spec.angle(a, b, c)
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        self.spec.offset(from, to, dist)
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        self.spec.norm(coords)
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        super::find_frame::integrity_mapping(previous, self)
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        // C Frame equality is conversion-based (frame.c:3549); DSBSpecFrame
        // does not override it. The base→current conversion depends only on
        // the spectral System/StdOfRest/etc — the DSB-specific attributes
        // (DSBCentre, IF, SideBand, AlignSideBand) do NOT enter that
        // mapping, so the oracle reports two DSBSpecFrames differing only in
        // those attributes as equal. We match C: route through the shared
        // conversion-based helper rather than comparing the DSB fields.
        super::find_frame::frames_equal(self, other)
    }

    fn system_string(&self) -> AstResult<Option<&str>> {
        self.spec.system_string()
    }

    // -----------------------------------------------------------------
    // Match-control attributes — delegate to the underlying SpecFrame
    // (same MinAxes / MaxAxes / Permute defaults).
    // -----------------------------------------------------------------

    fn cast(&self, other: &dyn Frame) -> AstResult<Option<DynFrame>> {
        // DSBSpecFrame -> SpecFrame (truncate to the parent), or
        // -> BasicFrame (plain Frame).
        if other
            .as_any()
            .downcast_ref::<crate::frames::SpecFrame>()
            .is_some()
        {
            Ok(Some(Box::new(SpecFrame::clone(self.spec()))))
        } else if other
            .as_any()
            .downcast_ref::<crate::frames::BasicFrame>()
            .is_some()
        {
            Ok(Some(Box::new(crate::frames::cast_to_basic_frame(
                self,
                self.spec.frame_attrs(),
            )?)))
        } else {
            Ok(None)
        }
    }

    // -----------------------------------------------------------------
    // astFindFrame / astMatch / astSubFrame trampoline.
    // Mirrors dsbspecframe.c:976 (Match), 1470 (SubFrame), 1154 (Overlay).
    // -----------------------------------------------------------------

    fn match_template(
        &self,
        target: &dyn Frame,
        match_sub: bool,
    ) -> AstResult<Option<crate::frames::find_frame::MatchResult>> {
        // dsbspecframe.c:976 `Match`: the parent SpecFrame Match, which
        // accepts any target containing a SpecFrame axis. C then repeats
        // the target's SubFrame with this template when the matched axis
        // belongs to a DSBSpecFrame; the port's parent already passes this
        // DSBSpecFrame as the template to that SubFrame, so its result is
        // the one C's repeat produces.
        super::specframe::spec_match(self, target, match_sub)
    }

    fn sub_frame(
        &self,
        template: Option<&dyn Frame>,
        result_naxes: usize,
        target_axes: &[i32],
        template_axes: &[i32],
    ) -> AstResult<Option<(DynMapping, DynFrame)>> {
        // Mirrors dsbspecframe.c:1470. First defer to the parent
        // SpecFrame::SubFrame to build a result Frame of the right
        // shape and the SpecFrame-level Mapping. If `self` is being
        // used as the target and the template is a DSBSpecFrame, the
        // result will be a DSBSpecFrame and we may need to extend
        // the Mapping with sideband-specific transforms.
        let parent = self
            .spec
            .sub_frame(template, result_naxes, target_axes, template_axes)?;
        let Some((mut map, result)) = parent else {
            return Ok(None);
        };

        // The parent SubFrame forms its result from a copy of the target,
        // which in C is this DSBSpecFrame, and overlays the template onto
        // it (specframe.c:4843-4854). The port's parent works on the
        // SpecFrame part, so put its result back into a copy of `self`,
        // then add the DSBSpecFrame attributes a DSBSpecFrame template
        // overlays (dsbspecframe.c:1154 `Overlay`).
        let Some(spec_result) = result.as_any().downcast_ref::<SpecFrame>() else {
            // The parent picked axes into a plain Frame; nothing more to do.
            return Ok(Some((map, result)));
        };
        let mut dsb = self.clone();
        dsb.spec = spec_result.clone();
        let t_dsb = template.and_then(|t| t.as_any().downcast_ref::<DSBSpecFrame>());
        if let Some(t_dsb) = t_dsb {
            if let Some(v) = t_dsb.dsb_centre {
                dsb.set_dsb_centre(v);
            }
            if let Some(v) = t_dsb.ifr {
                dsb.set_ifr(v);
            }
            if let Some(v) = t_dsb.sideband {
                dsb.set_sideband(v);
            }
            if let Some(v) = t_dsb.align_sideband {
                dsb.set_align_sideband(v);
            }

            // Restoring FrameSet integrity always aligns sidebands;
            // otherwise both AlignSideBand flags must be set
            // (dsbspecframe.c:1628).
            let alignsb = self.frame_flags() & super::INTFLAG != 0
                || (self.align_sideband() && t_dsb.align_sideband());
            if alignsb {
                // Map the target to its observed sideband, apply the
                // parent SubFrame's spectral Mapping, then map from the
                // observed sideband to the result's (dsbspecframe.c:1690).
                let map1 = self.to_observed_mapping()?;
                let mut map2 = dsb.to_observed_mapping()?;
                map2.invert();
                let map3 = CmpMap::new(map1, map, Composition::Series)?;
                let joined: DynMapping =
                    Box::new(CmpMap::new(Box::new(map3), map2, Composition::Series)?);
                map = joined.simplify()?;
            }
        }
        let final_result: DynFrame = Box::new(dsb);

        Ok(Some((map, final_result)))
    }

    fn pick_axes_snapshot(&self, target_axes: &[i32]) -> AstResult<DynFrame> {
        // DSBSpecFrame is 1-axis. If target_axes is [0] (or [-1]),
        // return a DSBSpecFrame snapshot. Otherwise fall back to the
        // SpecFrame snapshot (which itself falls back to BasicFrame
        // for non-trivial axis layouts).
        if target_axes == [0] {
            Ok(Box::new(self.clone()))
        } else {
            self.spec.pick_axes_snapshot(target_axes)
        }
    }

    fn overlay(&self, template_axes: &[i32], result: &mut dyn Frame) -> AstResult<()> {
        // dsbspecframe.c:1154 `Overlay`: the parent SpecFrame Overlay, then
        // the DSBSpecFrame attributes set on this template, onto a
        // DSBSpecFrame result only.
        self.spec.overlay(template_axes, result)?;
        if let Some(r) = result.as_any_mut().downcast_mut::<DSBSpecFrame>() {
            if let Some(v) = self.dsb_centre {
                r.set_dsb_centre(v);
            }
            if let Some(v) = self.ifr {
                r.set_ifr(v);
            }
            if let Some(v) = self.sideband {
                r.set_sideband(v);
            }
            if let Some(v) = self.align_sideband {
                r.set_align_sideband(v);
            }
        }
        Ok(())
    }

    fn axis_overlay_source(&self, axis: usize) -> Option<super::overlay::AxisOverlaySource> {
        self.spec.axis_overlay_source(axis)
    }

    fn apply_axis_overlay(&mut self, axis: usize, source: &super::overlay::AxisOverlaySource) {
        self.spec.apply_axis_overlay(axis, source);
    }

    fn overlay_system_from(&mut self, template: &dyn std::any::Any) -> AstResult<()> {
        self.spec.overlay_system_from(template)
    }
}

// ---------------------------------------------------------------------------
// Serializable trait implementation
// ---------------------------------------------------------------------------

impl Serializable for DSBSpecFrame {
    fn class_name(&self) -> &'static str {
        "DSBSpecFrame"
    }

    fn to_node(&self) -> AstResult<Node> {
        // Get the SpecFrame's node (has all Frame + SpecFrame attrs).
        let mut node = self.spec.to_node()?;

        // Override class name and comment.
        node.class = "DSBSpecFrame".to_string();
        node.class_comment = Some("Dual sideband spectral axis".to_string());

        // The aggregate Lbl1 carries the sideband suffix for a default label
        // (DSBSpecFrame::GetLabel, dsbspecframe.c:688): the SpecFrame node
        // emitted the bare system label ("Frequency"), so replace it with
        // this frame's effective label ("Frequency (LSB)"). Stays commented.
        if !self.spec.test_label(1)? {
            node.replace_or_push(
                "Lbl1",
                NodeValue::Unset(Box::new(NodeValue::String(self.label(1)?))),
            );
        }

        // The Domain default for a DSBSpecFrame is "DSBSPECTRUM"
        // (dsbspecframe.c GetDomain), not the SpecFrame "SPECTRUM". When
        // the Domain is unset, replace the parent's commented "SPECTRUM"
        // default with the DSBSPECTRUM default so the dump matches C.
        if !self.spec.test_domain() {
            node.replace_or_push(
                "Domain",
                NodeValue::Unset(Box::new(NodeValue::String("DSBSPECTRUM".to_string()))),
            );
        }

        // Add IsA SpecFrame marker.
        node.push_isa(
            "SpecFrame",
            Some("Description of spectral coordinate system".to_string()),
        );

        // DSBCen
        if let Some(dsb_cen) = self.dsb_centre {
            node.push_double(
                "DSBCen",
                dsb_cen,
                Some("Central frequency (Hz topo)".to_string()),
            );
        } else {
            node.push_unset(
                "DSBCen",
                NodeValue::Double(Self::DEFAULT_DSB_CENTRE),
                Some("Central frequency (Hz topo)".to_string()),
            );
        }

        // IF
        if let Some(ifr) = self.ifr {
            node.push_double("IF", ifr, Some("Intermediate frequency (Hz)".to_string()));
        } else {
            node.push_unset(
                "IF",
                NodeValue::Double(Self::DEFAULT_IFR),
                Some("Intermediate frequency (Hz)".to_string()),
            );
        }

        // SideBn — active when explicitly set, else commented with the
        // IF-sign-derived default (C dumps with helpful=1, dsbspecframe.c).
        let sb = self.sideband();
        let comment = match sb {
            SideBand::USB => "Represents upper sideband",
            SideBand::LSB => "Represents lower sideband",
            SideBand::LO => "Represents offset from LO frequency",
        };
        if self.sideband.is_some() {
            node.push_string("SideBn", sb.as_str(), Some(comment.to_string()));
        } else {
            node.push_unset(
                "SideBn",
                NodeValue::String(sb.as_str().to_string()),
                Some(comment.to_string()),
            );
        }

        // AlSdBn — active when explicitly set, else commented with the
        // default (0). C dumps with helpful=1.
        match self.align_sideband {
            Some(v) => node.push_int(
                "AlSdBn",
                if v { 1 } else { 0 },
                Some("Align sidebands?".to_string()),
            ),
            None => node.push_unset(
                "AlSdBn",
                NodeValue::Int(0),
                Some("Align sidebands?".to_string()),
            ),
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("DSBSpecFrame");
        // Deserialize the SpecFrame portion.
        let spec = SpecFrame::from_node(whole)?;

        // DSBCen
        let dsb_centre = if node.get("DSBCen").is_some() {
            Some(node.get_double("DSBCen", Self::DEFAULT_DSB_CENTRE))
        } else {
            None
        };

        // IF
        let ifr = if node.get("IF").is_some() {
            Some(node.get_double("IF", Self::DEFAULT_IFR))
        } else {
            None
        };

        // SideBn
        let sideband = {
            let sb_str = node.get_string("SideBn", "");
            if sb_str.is_empty() {
                None
            } else {
                SideBand::parse(&sb_str)
            }
        };

        // AlSdBn
        let align_sideband = {
            let val = node.get_int("AlSdBn", -1);
            if val < 0 { None } else { Some(val != 0) }
        };

        Ok(DSBSpecFrame {
            spec,
            dsb_centre,
            ifr,
            sideband,
            align_sideband,
        })
    }
}

#[cfg(test)]
mod find_frame_tests {
    use super::*;
    use crate::frames::{BasicFrameBuilder, SpecFrame};

    #[test]
    fn dsb_overlay_template_copies_set_attributes() {
        let mut template = DSBSpecFrame::new(SpecSystem::Freq);
        template.set_dsb_centre(2.5e10);
        template.set_ifr(3.0e9);
        template.set_sideband(SideBand::USB);
        template.set_align_sideband(true);

        let mut target = DSBSpecFrame::new(SpecSystem::Freq);
        template.overlay(&[0], &mut target).expect("overlay ok");
        assert_eq!(target.dsb_centre(), 2.5e10);
        assert_eq!(target.ifr(), 3.0e9);
        assert_eq!(target.sideband(), SideBand::USB);
        assert!(target.align_sideband());
    }

    #[test]
    fn dsb_overlay_template_skips_unset_attributes() {
        // Unset template — target's existing values must survive.
        let template = DSBSpecFrame::new(SpecSystem::Freq);
        let mut target = DSBSpecFrame::new(SpecSystem::Freq);
        target.set_dsb_centre(7.7e10);
        target.set_ifr(1.5e9);
        template.overlay(&[0], &mut target).expect("overlay ok");
        assert_eq!(target.dsb_centre(), 7.7e10);
        assert_eq!(target.ifr(), 1.5e9);
    }

    #[test]
    fn dsb_match_self_succeeds() {
        let template = DSBSpecFrame::new(SpecSystem::Freq);
        let target = DSBSpecFrame::new(SpecSystem::Freq);
        let m = template
            .match_template(&target, false)
            .expect("match ok")
            .expect("match found");
        assert_eq!(m.target_axes, vec![0]);
        assert_eq!(m.template_axes, vec![0]);
        assert_eq!(m.result.naxes(), 1);
    }

    #[test]
    fn dsb_match_against_specframe_target() {
        // A DSBSpecFrame template has its Domain *unset* by default (the
        // "DSBSPECTRUM" default is supplied by GetDomain, not stored), so
        // the parent Frame::Match Domain check is skipped and a plain
        // SpecFrame target matches. Verified against the C oracle:
        // astFindFrame(target=spec, template=dsb) and astConvert in both
        // directions all succeed (dsbspecframe.c GetDomain leaves
        // astTestDomain false).
        let template = DSBSpecFrame::new(SpecSystem::Freq);
        let target = SpecFrame::new(SpecSystem::Freq);
        assert!(
            template
                .match_template(&target, true)
                .expect("match ok")
                .is_some()
        );
    }

    #[test]
    fn dsb_match_against_basicframe_target_returns_none() {
        let template = DSBSpecFrame::new(SpecSystem::Freq);
        let target = BasicFrameBuilder::new(1).build().unwrap();
        assert!(
            template
                .match_template(&target, false)
                .expect("match ok")
                .is_none()
        );
    }

    #[test]
    fn dsb_pick_axes_snapshot_full() {
        let dsb = DSBSpecFrame::new(SpecSystem::Freq);
        let snap = dsb.pick_axes_snapshot(&[0]).expect("pick ok");
        assert!(snap.as_any().is::<DSBSpecFrame>());
        assert_eq!(snap.naxes(), 1);
    }
}
