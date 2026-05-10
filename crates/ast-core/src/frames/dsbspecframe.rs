//! The dual-sideband spectral coordinate frame (DSBSpecFrame).
//!
//! A DSBSpecFrame extends a SpecFrame to represent spectral position in a
//! dual-sideband instrument. It adds the concept of a local oscillator (LO)
//! frequency split into a DSB centre frequency and an intermediate frequency (IF),
//! and tracks which sideband (USB, LSB, or LO) the stored spectral values refer to.

use crate::frames::{SpecFrame, SpecSystem, StdOfRest};

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
    /// The domain is set to "DSBSPECTRUM". All DSB-specific attributes start
    /// at their defaults (DSBCentre = 30 GHz, IF = 4 GHz, SideBand derived
    /// from IF sign = LSB, AlignSideBand = false).
    pub fn new(system: SpecSystem) -> Self {
        let mut spec = SpecFrame::new(system);
        spec.set_domain("DSBSPECTRUM");
        Self {
            spec,
            dsb_centre: None,
            ifr: None,
            sideband: None,
            align_sideband: None,
        }
    }

    // --- DSB-specific attribute getters ---

    /// Return the DSB centre frequency in Hz.
    ///
    /// Defaults to 3.0e10 Hz (30 GHz) if not explicitly set.
    pub fn dsb_centre(&self) -> f64 {
        self.dsb_centre.unwrap_or(Self::DEFAULT_DSB_CENTRE)
    }

    /// Return the intermediate frequency in Hz.
    ///
    /// Defaults to 4.0e9 Hz (4 GHz) if not explicitly set.
    pub fn ifr(&self) -> f64 {
        self.ifr.unwrap_or(Self::DEFAULT_IFR)
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

    /// Return the rest frequency in Hz.
    pub fn rest_freq(&self) -> Option<f64> {
        self.spec.rest_freq()
    }

    /// Set the rest frequency in Hz.
    pub fn set_rest_freq(&mut self, hz: f64) {
        self.spec.set_rest_freq(hz);
    }

    /// Set the epoch as MJD.
    pub fn set_epoch(&mut self, mjd: f64) {
        self.spec.set_epoch(mjd);
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

    // --- Internal accessors ---

    /// Return a reference to the underlying `SpecFrame`.
    pub fn spec(&self) -> &SpecFrame {
        &self.spec
    }

    /// Return a mutable reference to the underlying `SpecFrame`.
    pub fn spec_mut(&mut self) -> &mut SpecFrame {
        &mut self.spec
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
}
