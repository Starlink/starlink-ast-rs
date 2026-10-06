//! The dual-sideband spectral coordinate frame (DSBSpecFrame).
//!
//! A DSBSpecFrame extends a SpecFrame to represent spectral position in a
//! dual-sideband instrument. It adds the concept of a local oscillator (LO)
//! frequency split into a DSB centre frequency and an intermediate frequency (IF),
//! and tracks which sideband (USB, LSB, or LO) the stored spectral values refer to.

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame, SpecFrame, SpecSystem, StdOfRest};
use crate::mappings::{CmpMap, Composition, DynMapping, UnitMap, WinMap};
use crate::serial::{Node, NodeValue, Serializable};

// ---------------------------------------------------------------------------
// SideBand — which sideband the spectral values represent
// ---------------------------------------------------------------------------

/// Which sideband of a dual-sideband receiver the spectral values represent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SideBand {
    /// Upper sideband (signal frequency above LO).
    USB,
    /// Lower sideband (signal frequency below LO).
    LSB,
    /// Local oscillator frequency itself.
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

    /// Parse from string (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        let upper = s.to_ascii_uppercase();
        match upper.as_str() {
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

/// A coordinate frame for dual-sideband spectral observations.
///
/// `DSBSpecFrame` wraps a [`SpecFrame`] and adds attributes that describe
/// the relationship between the observed frequency and the local oscillator
/// in a dual-sideband (DSB) receiver system. The key additional attributes are:
///
/// - **DSBCentre**: the central frequency of the observation (LO frequency), in Hz.
/// - **IF**: the intermediate frequency (half the sideband separation), in Hz.
///   Positive IF means the stored frequency is in the lower sideband by default.
/// - **SideBand**: which sideband (USB, LSB, or LO) the stored values represent.
/// - **AlignSideBand**: whether sideband flipping is allowed during frame alignment.
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

    /// DSB centre frequency in Hz (LO frequency). Default: 3.0e10.
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

    /// Create a new `DSBSpecFrame` with the given spectral coordinate system.
    ///
    /// The Domain defaults to "DSBSPECTRUM" but is left *unset* (so
    /// `test_domain` reports false), mirroring C `GetDomain`
    /// (dsbspecframe.c:531) which supplies "DSBSPECTRUM" only as a
    /// default. All DSB-specific attributes start at their defaults
    /// (DSBCentre = 30 GHz, IF = 4 GHz, SideBand derived from IF
    /// sign = LSB, AlignSideBand = false).
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

    /// A DSBSpecFrame with no attribute set, as `astDSBSpecFrame(" ")`
    /// makes one.
    pub fn unset_system() -> Self {
        Self::from_spec_frame(SpecFrame::unset_system())
    }

    /// Whether DSBCentre is set.
    pub fn test_dsb_centre(&self) -> bool {
        self.dsb_centre.is_some()
    }

    /// Clear DSBCentre.
    pub fn clear_dsb_centre(&mut self) {
        self.dsb_centre = None;
    }

    /// Whether IF is set.
    pub fn test_if(&self) -> bool {
        self.ifr.is_some()
    }

    /// Clear IF.
    pub fn clear_if(&mut self) {
        self.ifr = None;
    }

    /// Whether SideBand is set.
    pub fn test_sideband(&self) -> bool {
        self.sideband.is_some()
    }

    /// Clear SideBand, which then follows the sign of IF.
    pub fn clear_sideband(&mut self) {
        self.sideband = None;
    }

    /// Whether AlignSideBand is set.
    pub fn test_align_sideband(&self) -> bool {
        self.align_sideband.is_some()
    }

    /// Clear AlignSideBand.
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

    /// Return the DSB centre frequency as a **topocentric frequency in Hz**
    /// (the internal storage unit).
    ///
    /// Defaults to 3.0e10 Hz (30 GHz) if not explicitly set. To obtain the
    /// value expressed in the frame's current spectral System (what C's
    /// `astGetC("DSBCentre")` returns), use [`Self::dsb_centre_cur`].
    pub fn dsb_centre(&self) -> f64 {
        self.dsb_centre.unwrap_or(Self::DEFAULT_DSB_CENTRE)
    }

    /// Return the DSB centre expressed in the frame's **current spectral
    /// System and units**.
    ///
    /// Mirrors C `astGetAttrib(this, "DSBCentre")` (dsbspecframe.c:459-483):
    /// the internal topocentric-frequency value is transformed through the
    /// inverse of `to_topo_mapping` (i.e. topocentric Hz → current
    /// system). This is the value the C string-attribute layer formats; the
    /// ast-capi shim should call this for `astGetC` parity.
    pub fn dsb_centre_cur(&self) -> AstResult<f64> {
        // TopoMap forward is current-system → topocentric Hz; the get-path
        // needs the inverse (topocentric Hz → current system).
        let topo_map = self.to_topo_mapping()?;
        let pts = crate::coords::PointSet::from_rows([[self.dsb_centre()]]);
        let out = topo_map.transform(&pts, false)?;
        Ok(out.point(0)[0])
    }

    /// Set the DSB centre from a value expressed in the frame's **current
    /// spectral System and units**, converting to internal topocentric Hz.
    ///
    /// Mirrors C `astSetAttrib(this, "DSBCentre=...")` (dsbspecframe.c:1355-1370):
    /// the supplied value is transformed through `to_topo_mapping`
    /// (current system → topocentric Hz) before storage. The ast-capi shim
    /// should call this for `astSetC` parity.
    pub fn set_dsb_centre_cur(&mut self, value: f64) -> AstResult<()> {
        let topo_map = self.to_topo_mapping()?;
        let pts = crate::coords::PointSet::from_rows([[value]]);
        let out = topo_map.transform(&pts, true)?;
        self.dsb_centre = Some(out.point(0)[0]);
        Ok(())
    }

    /// Return the intermediate frequency as a **topocentric frequency in Hz**
    /// (the internal storage unit).
    ///
    /// Defaults to 4.0e9 Hz (4 GHz) if not explicitly set. C's
    /// `astGetC("IF")` reports this value in GHz (×1e-9); use
    /// [`Self::if_ghz`] for that external representation.
    pub fn ifr(&self) -> f64 {
        self.ifr.unwrap_or(Self::DEFAULT_IFR)
    }

    /// Return the intermediate frequency in **GHz**, matching C's external
    /// `astGetC("IF")` representation (dsbspecframe.c:487-492, `× 1e-9`).
    pub fn if_ghz(&self) -> f64 {
        self.ifr() * 1.0e-9
    }

    /// Set the intermediate frequency from a value in **GHz**, converting to
    /// internal topocentric Hz (`× 1e9`). Mirrors C's `astSetC("IF=...")`
    /// (dsbspecframe.c, the IF set-path).
    pub fn set_if_ghz(&mut self, ghz: f64) {
        self.ifr = Some(ghz * 1.0e9);
    }

    /// Return the image-sideband frequency in **GHz**, matching C's external
    /// `astGetC("ImagFreq")` representation (dsbspecframe.c:496-501, `× 1e-9`).
    pub fn imag_freq_ghz(&self) -> AstResult<f64> {
        Ok(self.imag_freq()? * 1.0e-9)
    }

    /// Return the sideband that the stored spectral values represent.
    ///
    /// If explicitly set, returns that value. Otherwise, derives the sideband
    /// from the sign of the intermediate frequency: positive IF → LSB,
    /// negative or zero IF → USB.
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

    /// Return whether sideband alignment is enabled.
    ///
    /// Defaults to `false` if not explicitly set.
    pub fn align_sideband(&self) -> bool {
        self.align_sideband.unwrap_or(false)
    }

    // --- DSB-specific attribute setters ---

    /// Set the DSB centre frequency in Hz.
    pub fn set_dsb_centre(&mut self, hz: f64) {
        self.dsb_centre = Some(hz);
    }

    /// Set the intermediate frequency in Hz.
    ///
    /// The sign of the IF value determines the default sideband when no
    /// explicit sideband has been set: positive → LSB, negative/zero → USB.
    pub fn set_ifr(&mut self, hz: f64) {
        self.ifr = Some(hz);
    }

    /// Explicitly set the sideband.
    pub fn set_sideband(&mut self, sb: SideBand) {
        self.sideband = Some(sb);
    }

    /// Set whether sideband alignment is enabled.
    pub fn set_align_sideband(&mut self, val: bool) {
        self.align_sideband = Some(val);
    }

    // --- SpecFrame delegators ---

    /// Return the spectral coordinate system.
    pub fn system(&self) -> SpecSystem {
        self.spec.system()
    }

    /// Set the spectral coordinate system.
    pub fn set_system(&mut self, sys: SpecSystem) {
        self.spec.set_system(sys);
    }

    /// Return the standard of rest.
    pub fn std_of_rest(&self) -> StdOfRest {
        self.spec.std_of_rest()
    }

    /// Set the standard of rest.
    pub fn set_std_of_rest(&mut self, sor: StdOfRest) {
        self.spec.set_std_of_rest(sor);
    }

    /// RestFreq in Hz, set or default.
    pub fn rest_freq(&self) -> f64 {
        self.spec.rest_freq()
    }

    /// Set the rest frequency in Hz.
    pub fn set_rest_freq(&mut self, hz: f64) {
        self.spec.set_rest_freq(hz);
    }

    /// Set the epoch as MJD.
    pub fn set_epoch_mjd(&mut self, mjd: f64) {
        self.spec.set_epoch_mjd(mjd);
    }

    /// Set the epoch from a decimal year (IAU 1984 rule).
    pub fn set_epoch_year(&mut self, year: f64) {
        self.spec.set_epoch_year(year);
    }

    /// Return the epoch as MJD.
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
    /// Mirrors C `DSBSetUp` (fitschan.c:8930-9064). `imagfreq` is the
    /// `IMAGFREQ` keyword value (Hz, measured in the source rest frame);
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
    pub fn dsb_set_up(&mut self, imagfreq: f64, crval: f64) -> AstResult<()> {
        use crate::frames::Frame;

        let bad = crate::constants::BAD_VALUE;

        // Step 1 (C:9000-9015). Build dsb_src = copy of the underlying
        // SpecFrame set to source-rest frequency in Hz, and dsb_topo = the
        // same set to topocentric. astConvert(dsb_src, dsb_topo, "") gives a
        // mapping source-rest-FREQ-Hz -> topocentric-FREQ-Hz.
        let mut dsb_src = self.spec.clone();
        dsb_src.set_std_of_rest(StdOfRest::Source);
        dsb_src.set_system(SpecSystem::Freq);
        dsb_src.set_unit(1, "Hz")?;

        let mut dsb_topo = dsb_src.clone();
        dsb_topo.set_std_of_rest(StdOfRest::Topocentric);

        let Some(fs) = dsb_src.convert_to(&dsb_topo, &[])? else {
            // No conversion found (C:9018 — leaves the frame unchanged).
            return Ok(());
        };
        let map = fs.mapping(
            crate::frames::FrameRef::Base,
            crate::frames::FrameRef::Current,
        )?;

        // Step 2 (C:9020-9025). Transform [RestFreq, imagfreq].
        // fitschan.c:9109 reads RestFreq, set or default.
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
            topo.spec.set_std_of_rest(StdOfRest::Topocentric);
            topo.spec.set_system(SpecSystem::Freq);
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

    /// Return the local oscillator frequency in Hz.
    ///
    /// This is `dsb_centre + ifr`. Validates that the result is physically
    /// sensible (non-negative).
    pub fn lo_freq(&self) -> AstResult<f64> {
        let lo = self.dsb_centre() + self.ifr();
        if lo < 0.0 {
            return Err(AstError::new(
                ErrorCode::Bdfts,
                format!(
                    "DSBSpecFrame LO frequency is negative ({lo} Hz): \
                     dsb_centre={}, ifr={}",
                    self.dsb_centre(),
                    self.ifr()
                ),
            ));
        }
        Ok(lo)
    }

    /// Compute the image-sideband frequency corresponding to the rest
    /// frequency.
    ///
    /// Mirrors `dsbspecframe.c::GetImagFreq` (line 599): the rest
    /// frequency is interpreted as an observed-sideband frequency in
    /// the source's rest frame (Hz). The mapping from observed to
    /// the opposite sideband is built on a temporary copy with
    /// `StdOfRest=Source`, `System=Freq`, `Unit=Hz`,
    /// `SideBand=observed`; that mapping is applied to the rest
    /// frequency to yield the image-sideband frequency.
    ///
    /// Uses RestFreq, set or default, as dsbspecframe.c:673 does.
    pub fn imag_freq(&self) -> AstResult<f64> {
        let rest_freq = self.spec.rest_freq();

        // Copy and reset to canonical "rest-frequency frame":
        // SoR=Source, System=Freq, Unit=Hz, SideBand=observed. This
        // matches C's `astSetStdOfRest(rf_frame, AST__SCSOR);
        // astSetSystem(rf_frame, AST__FREQ); astSetUnit(rf_frame, 0,
        // "Hz"); astSetC(rf_frame, "SideBand", "observed");`.
        let mut rf_frame = self.clone();
        rf_frame.spec.set_std_of_rest(StdOfRest::Source);
        rf_frame.spec.set_system(SpecSystem::Freq);
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

    /// Create a mapping from the current spectral system/SOR to topocentric
    /// frequency in Hz.
    fn to_topo_mapping(&self) -> AstResult<DynMapping> {
        let mut topo_frame = SpecFrame::new(SpecSystem::Freq);
        topo_frame.set_std_of_rest(StdOfRest::Topocentric);
        // Target topocentric frequency in Hz. Mirrors C `TopoMap`
        // (dsbspecframe.c:2124, `astSetUnit(tf2, 0, "Hz")`): the internal
        // DSBCentre / IF attributes are topocentric Hz, so the conversion
        // must land in Hz rather than the FREQ display default (GHz).
        topo_frame.set_unit(1, "Hz")?;

        // Copy observer attributes from self.spec to the topo frame.
        if self.spec.test_obs_lat() {
            let lat = self.spec.obs_lat();
            topo_frame.set_obs_lat(lat);
        }
        if self.spec.test_obs_lon() {
            let lon = self.spec.obs_lon();
            topo_frame.set_obs_lon(lon);
        }
        if self.spec.test_obs_alt() {
            let alt = self.spec.obs_alt();
            topo_frame.set_obs_alt(alt);
        }
        topo_frame.set_epoch_mjd(self.spec.epoch_mjd());
        if self.spec.test_ref_ra() {
            topo_frame.set_ref_ra(self.spec.ref_ra());
        }
        if self.spec.test_ref_dec() {
            topo_frame.set_ref_dec(self.spec.ref_dec());
        }
        if self.spec.test_rest_freq() {
            topo_frame.set_rest_freq(self.spec.rest_freq());
        }

        self.spec.make_spec_mapping(&topo_frame)
    }

    /// Create a mapping that converts frequencies from the current sideband
    /// to the upper sideband (USB).
    ///
    /// If already USB, returns a unit (identity) mapping.
    pub fn to_usb_mapping(&self) -> AstResult<DynMapping> {
        if self.sideband() == SideBand::USB {
            return Ok(Box::new(UnitMap::new(1)));
        }
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

        // Compose: topo_map → flip → inv_topo
        let step1 = CmpMap::new(topo_map, flip, Composition::Series)?;
        let result = CmpMap::new(Box::new(step1), inv_topo, Composition::Series)?;
        Ok(Box::new(result))
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

    fn naxes(&self) -> usize {
        1
    }

    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        (self, axis)
    }

    // Ident is an Object-level attribute (`object.c:5217-5229`), and the
    // component SpecFrame supplies the storage the dump reads back.

    /// Mirrors C `DSBSpecFrame::GetLabel` (dsbspecframe.c:688): the
    /// SpecFrame's Label, followed by the sideband when it is a default.
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

    fn system_string(&self) -> Option<&str> {
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
        // Deserialize the SpecFrame portion.
        let spec = SpecFrame::from_node(node)?;

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
