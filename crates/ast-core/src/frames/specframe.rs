//! The spectral coordinate frame (SpecFrame).

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::helpers::{ast_equal, is_bad};
use crate::mappings::{DynMapping, SpecCvtType, SpecMap, UnitMap};
use crate::serial::{Node, NodeValue, Serializable};

// ---------------------------------------------------------------------------
// SpecSystem — spectral coordinate system type
// ---------------------------------------------------------------------------

/// Spectral coordinate system type.
///
/// Each variant corresponds to a way of measuring spectral position.
/// The "absolute" systems (Freq, Energy, Wavenum, Wavelen, Airwave) do not
/// require a rest frequency, while the "relative" systems (Vradio, Voptical,
/// Redshift, Beta, Vrel) define coordinates relative to a rest frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpecSystem {
    /// Frequency (Hz).
    Freq,
    /// Energy (J).
    Energy,
    /// Wave number (1/m).
    Wavenum,
    /// Wavelength in vacuum (m).
    Wavelen,
    /// Wavelength in air (m).
    Airwave,
    /// Radio velocity (m/s).
    Vradio,
    /// Optical velocity (m/s).
    Voptical,
    /// Redshift (dimensionless).
    Redshift,
    /// Beta factor v/c (dimensionless).
    Beta,
    /// Relativistic velocity (m/s).
    Vrel,
}

impl SpecSystem {
    /// Return the canonical string representation (FITS-WCS style).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Freq => "FREQ",
            Self::Energy => "ENER",
            Self::Wavenum => "WAVN",
            Self::Wavelen => "WAVE",
            Self::Airwave => "AWAV",
            Self::Vradio => "VRAD",
            Self::Voptical => "VOPT",
            Self::Redshift => "ZOPT",
            Self::Beta => "BETA",
            Self::Vrel => "VELO",
        }
    }

    /// Parse from string (case-insensitive). Accepts both short and long forms.
    pub fn parse(s: &str) -> Option<Self> {
        let upper = s.to_ascii_uppercase();
        match upper.as_str() {
            "FREQ" | "FREQUENCY" => Some(Self::Freq),
            "ENER" | "ENERGY" => Some(Self::Energy),
            "WAVN" | "WAVENUM" | "WAVENUMBER" => Some(Self::Wavenum),
            "WAVE" | "WAVELEN" | "WAVELENGTH" => Some(Self::Wavelen),
            "AWAV" | "AIRWAVE" | "AIRWAV" => Some(Self::Airwave),
            "VRAD" | "VRADIO" => Some(Self::Vradio),
            "VOPT" | "VOPTICAL" => Some(Self::Voptical),
            "ZOPT" | "REDSHIFT" => Some(Self::Redshift),
            "BETA" => Some(Self::Beta),
            "VELO" | "VREL" | "RELATIVISTIC" => Some(Self::Vrel),
            _ => None,
        }
    }

    /// Whether this system is a velocity-type (relative) that requires rest frequency.
    pub fn is_velocity(&self) -> bool {
        matches!(
            self,
            Self::Vradio | Self::Voptical | Self::Redshift | Self::Beta | Self::Vrel
        )
    }

    /// Whether this system is an absolute system (not dependent on rest frequency).
    pub fn is_absolute(&self) -> bool {
        !self.is_velocity()
    }

    /// Default units for SpecMap (SI-like).
    pub fn specmap_units(&self) -> &'static str {
        match self {
            Self::Freq => "Hz",
            Self::Energy => "J",
            Self::Wavenum => "1/m",
            Self::Wavelen | Self::Airwave => "m",
            Self::Vradio | Self::Voptical | Self::Vrel => "m/s",
            Self::Redshift | Self::Beta => "",
        }
    }

    /// Human-readable label for this system.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Freq => "Frequency",
            Self::Energy => "Energy",
            Self::Wavenum => "Wave-number",
            Self::Wavelen => "Wavelength",
            Self::Airwave => "Wavelength",
            Self::Vradio => "Radio velocity",
            Self::Voptical => "Optical velocity",
            Self::Redshift => "Redshift",
            Self::Beta => "Beta factor",
            Self::Vrel => "Velocity",
        }
    }

    /// Native serialisation attribute key for the unit preference (e.g. "UFreq").
    pub(crate) fn unit_attr_key(&self) -> &'static str {
        match self {
            Self::Freq => "UFreq",
            Self::Energy => "UEner",
            Self::Wavenum => "UWavn",
            Self::Wavelen => "UWave",
            Self::Airwave => "UAwav",
            Self::Vradio => "UVrad",
            Self::Voptical => "UVopt",
            Self::Redshift => "UZopt",
            Self::Beta => "UBeta",
            Self::Vrel => "UVelo",
        }
    }

    /// Comment for the unit preference attribute in Native format.
    pub(crate) fn unit_attr_comment(&self) -> &'static str {
        match self {
            Self::Freq => "Preferred units for frequency",
            Self::Energy => "Preferred units for energy",
            Self::Wavenum => "Preferred units for wave-number",
            Self::Wavelen => "Preferred units for wavelength",
            Self::Airwave => "Preferred units for wavelength in air",
            Self::Vradio => "Preferred units for radio velocity",
            Self::Voptical => "Preferred units for optical velocity",
            Self::Redshift => "Preferred units for redshift",
            Self::Beta => "Preferred units for beta factor",
            Self::Vrel => "Preferred units for apparent radial velocity",
        }
    }

    /// Default display units for this system (user-facing, not SI).
    pub(crate) fn default_display_units(&self) -> &'static str {
        match self {
            Self::Freq => "GHz",
            Self::Energy => "J",
            Self::Wavenum => "/m",
            Self::Wavelen | Self::Airwave => "Angstrom",
            Self::Vradio | Self::Voptical | Self::Vrel => "m/s",
            Self::Redshift | Self::Beta => "",
        }
    }
}

// ---------------------------------------------------------------------------
// StdOfRest — standard of rest
// ---------------------------------------------------------------------------

/// Standard of rest for spectral observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StdOfRest {
    /// Topocentric (observer on Earth surface).
    Topocentric,
    /// Geocentric (Earth centre).
    Geocentric,
    /// Barycentric (Solar system barycentre).
    Barycentric,
    /// Heliocentric (Sun centre).
    Heliocentric,
    /// Local Standard of Rest (kinematic).
    Lsrk,
    /// Local Standard of Rest (dynamical).
    Lsrd,
    /// Galactic centre.
    Galactic,
    /// Local Group.
    LocalGroup,
    /// Source rest frame (defined by SourceVel).
    Source,
}

impl StdOfRest {
    /// Return the canonical string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Topocentric => "Topocentric",
            Self::Geocentric => "Geocentric",
            Self::Barycentric => "Barycentric",
            Self::Heliocentric => "Heliocentric",
            Self::Lsrk => "LSRK",
            Self::Lsrd => "LSRD",
            Self::Galactic => "Galactic",
            Self::LocalGroup => "Local_group",
            Self::Source => "Source",
        }
    }

    /// Parse from string (case-insensitive). Accepts various abbreviations.
    pub fn parse(s: &str) -> Option<Self> {
        let upper = s.to_ascii_uppercase();
        match upper.as_str() {
            "TOPO" | "TOPOCENT" | "TOPOCENTRIC" => Some(Self::Topocentric),
            "GEO" | "GEOCENTR" | "GEOCENTRIC" => Some(Self::Geocentric),
            "BARY" | "BARYCENT" | "BARYCENTRIC" => Some(Self::Barycentric),
            "HELIO" | "HELIOCEN" | "HELIOCENTRIC" => Some(Self::Heliocentric),
            "LSRK" | "LSR" => Some(Self::Lsrk),
            "LSRD" => Some(Self::Lsrd),
            "GAL" | "GALACTOC" | "GALACTIC" => Some(Self::Galactic),
            "LG" | "LOCALGRP" | "LOCAL_GROUP" | "LOCAL-GROUP" => Some(Self::LocalGroup),
            "SOURCE" | "SRC" => Some(Self::Source),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// SpecFrame
// ---------------------------------------------------------------------------

/// A coordinate frame for spectral coordinates.
///
/// `SpecFrame` is a 1-axis Frame representing spectral position. It supports
/// a range of coordinate systems (frequency, wavelength, velocity, energy,
/// redshift, etc.) and reference frames (heliocentric, barycentric, LSRK, etc.).
///
/// Construct via [`SpecFrame::new`].
///
/// # Examples
///
/// ```
/// use ast_core::frames::{Frame, SpecFrame, SpecSystem, StdOfRest};
///
/// let mut sf = SpecFrame::new(SpecSystem::Freq);
/// assert_eq!(sf.naxes(), 1);
/// assert_eq!(sf.system(), SpecSystem::Freq);
/// assert_eq!(sf.std_of_rest(), StdOfRest::Heliocentric);
/// ```
#[derive(Debug, Clone)]
pub struct SpecFrame {
    // Frame-level attributes
    title: Option<String>,
    domain: Option<String>,
    epoch: Option<f64>, // MJD

    // Observer
    obs_lat: Option<f64>, // geodetic latitude (radians)
    obs_lon: Option<f64>, // geodetic longitude (radians)
    obs_alt: Option<f64>, // altitude (metres)

    // SpecFrame-specific
    system: SpecSystem,
    std_of_rest: StdOfRest,
    sor_set: bool,           // whether std_of_rest was explicitly set (vs defaulting)
    unit: Option<String>,    // user-facing unit for the current system (e.g. "GHz")
    rest_freq: Option<f64>,  // Hz
    ref_ra: Option<f64>,     // FK5 J2000 radians
    ref_dec: Option<f64>,    // FK5 J2000 radians
    source_vel: Option<f64>, // m/s (or redshift/beta depending on source_sys)
    source_vrf: Option<StdOfRest>, // rest frame of source velocity
    source_sys: Option<SpecSystem>, // spectral system of source velocity
    spec_origin: Option<f64>, // spectral origin (in default units for current system)
    align_std_of_rest: Option<StdOfRest>,
    align_spec_offset: bool,
}

impl SpecFrame {
    /// Create a new `SpecFrame` with the given spectral coordinate system.
    ///
    /// Defaults: domain = "SPECTRUM", std_of_rest = Heliocentric.
    pub fn new(system: SpecSystem) -> Self {
        Self {
            title: None,
            domain: Some("SPECTRUM".to_string()),
            epoch: None,
            obs_lat: None,
            obs_lon: None,
            obs_alt: None,
            system,
            std_of_rest: StdOfRest::Heliocentric,
            sor_set: false,
            unit: None,
            rest_freq: None,
            ref_ra: None,
            ref_dec: None,
            source_vel: None,
            source_vrf: None,
            source_sys: None,
            spec_origin: None,
            align_std_of_rest: None,
            align_spec_offset: false,
        }
    }

    // --- SpecFrame-specific attributes ---

    /// Return the spectral coordinate system.
    pub fn system(&self) -> SpecSystem {
        self.system
    }

    /// Set the spectral coordinate system.
    pub fn set_system(&mut self, sys: SpecSystem) {
        self.system = sys;
    }

    /// Return the standard of rest.
    pub fn std_of_rest(&self) -> StdOfRest {
        self.std_of_rest
    }

    /// Set the standard of rest.
    pub fn set_std_of_rest(&mut self, sor: StdOfRest) {
        self.std_of_rest = sor;
        self.sor_set = true;
    }

    /// Return the user-facing unit for the current system (e.g. "GHz" for FREQ).
    ///
    /// Returns `None` if no unit has been explicitly set.
    pub fn unit(&self) -> Option<&str> {
        self.unit.as_deref()
    }

    /// Set the user-facing unit for the current system.
    pub fn set_unit(&mut self, unit: &str) {
        self.unit = Some(unit.to_string());
    }

    /// Return the alignment standard of rest.
    ///
    /// Defaults to Heliocentric if not explicitly set.
    pub fn align_std_of_rest(&self) -> StdOfRest {
        self.align_std_of_rest.unwrap_or(StdOfRest::Heliocentric)
    }

    /// Set the alignment standard of rest.
    pub fn set_align_std_of_rest(&mut self, sor: StdOfRest) {
        self.align_std_of_rest = Some(sor);
    }

    /// Return the rest frequency in Hz.
    pub fn rest_freq(&self) -> Option<f64> {
        self.rest_freq
    }

    /// Set the rest frequency in Hz.
    pub fn set_rest_freq(&mut self, hz: f64) {
        self.rest_freq = Some(hz);
    }

    /// Return the reference RA (FK5 J2000, radians).
    pub fn ref_ra(&self) -> Option<f64> {
        self.ref_ra
    }

    /// Set the reference RA (FK5 J2000, radians).
    pub fn set_ref_ra(&mut self, ra: f64) {
        self.ref_ra = Some(ra);
    }

    /// Return the reference Dec (FK5 J2000, radians).
    pub fn ref_dec(&self) -> Option<f64> {
        self.ref_dec
    }

    /// Set the reference Dec (FK5 J2000, radians).
    pub fn set_ref_dec(&mut self, dec: f64) {
        self.ref_dec = Some(dec);
    }

    /// Return the source velocity (m/s or redshift/beta depending on source_sys).
    pub fn source_vel(&self) -> Option<f64> {
        self.source_vel
    }

    /// Set the source velocity.
    pub fn set_source_vel(&mut self, vel: f64) {
        self.source_vel = Some(vel);
    }

    /// Return the source velocity rest frame.
    ///
    /// Defaults to Heliocentric if not set.
    pub fn source_vrf(&self) -> StdOfRest {
        self.source_vrf.unwrap_or(StdOfRest::Heliocentric)
    }

    /// Set the source velocity rest frame.
    pub fn set_source_vrf(&mut self, sor: StdOfRest) {
        self.source_vrf = Some(sor);
    }

    /// Return the source velocity spectral system.
    ///
    /// Defaults to Vrel if not set.
    pub fn source_sys(&self) -> SpecSystem {
        self.source_sys.unwrap_or(SpecSystem::Vrel)
    }

    /// Set the source velocity spectral system.
    pub fn set_source_sys(&mut self, sys: SpecSystem) {
        self.source_sys = Some(sys);
    }

    /// Return the spectral origin (in default units for current system).
    pub fn spec_origin(&self) -> Option<f64> {
        self.spec_origin
    }

    /// Set the spectral origin.
    pub fn set_spec_origin(&mut self, origin: f64) {
        self.spec_origin = Some(origin);
    }

    /// Return the epoch as MJD.
    ///
    /// Defaults to J2000.0 (MJD 51544.5) if not set.
    pub fn epoch_mjd(&self) -> f64 {
        self.epoch.unwrap_or(51544.5)
    }

    /// Set the epoch as MJD.
    pub fn set_epoch(&mut self, mjd: f64) {
        self.epoch = Some(mjd);
    }

    /// Set the title.
    pub fn set_title(&mut self, title: &str) {
        self.title = Some(title.to_string());
    }

    /// Compute the default title from the system label and standard of rest.
    ///
    /// Format: "{label} ({sor})" — e.g. "Frequency (Heliocentric)".
    fn default_title(&self) -> String {
        let label = self.system.label();
        let sor = self.std_of_rest.as_str();
        format!("{label} ({sor})")
    }

    /// Set the domain.
    pub fn set_domain(&mut self, domain: &str) {
        self.domain = Some(domain.to_string());
    }

    /// Set the observer geodetic latitude (radians).
    pub fn set_obs_lat(&mut self, lat: f64) {
        self.obs_lat = Some(lat);
    }

    /// Set the observer geodetic longitude (radians).
    pub fn set_obs_lon(&mut self, lon: f64) {
        self.obs_lon = Some(lon);
    }

    /// Set the observer altitude (metres).
    pub fn set_obs_alt(&mut self, alt: f64) {
        self.obs_alt = Some(alt);
    }

    /// Whether to align in offset coordinate system.
    pub fn align_spec_offset(&self) -> bool {
        self.align_spec_offset
    }

    /// Set whether to align in offset coordinate system.
    pub fn set_align_spec_offset(&mut self, val: bool) {
        self.align_spec_offset = val;
    }

    // --- Conversion helpers ---

    /// Convert source velocity to heliocentric relativistic velocity (m/s).
    ///
    /// This is used when computing the "Source" rest frame correction.
    /// Follows the C `ConvertSourceVel` algorithm.
    fn convert_source_vel_to_helio_vrel(&self) -> f64 {
        let vel = match self.source_vel {
            Some(v) => v,
            None => return 0.0,
        };

        let sys = self.source_sys();
        let sor = self.source_vrf();

        // If already heliocentric + VREL, return directly.
        if sor == StdOfRest::Heliocentric && sys == SpecSystem::Vrel {
            return vel;
        }

        // Build a SpecMap chain to convert:
        //   source_sys → Vrel → freq → (SOR correction) → freq → Vrel
        let rf = match self.rest_freq {
            Some(rf) => rf,
            None => return 0.0,
        };

        let mut specmap = SpecMap::new();

        // Convert from source_sys to relativistic velocity.
        match sys {
            SpecSystem::Vradio => specmap.add(SpecCvtType::VrToVl, &[]),
            SpecSystem::Voptical => specmap.add(SpecCvtType::VoToVl, &[]),
            SpecSystem::Redshift => specmap.add(SpecCvtType::ZoToVl, &[]),
            SpecSystem::Beta => specmap.add(SpecCvtType::BtToVl, &[]),
            SpecSystem::Vrel => {} // already Vrel
            _ => return 0.0,       // absolute systems invalid for source vel
        }

        // Convert velocity to frequency.
        specmap.add(SpecCvtType::VlToFr, &[rf]);

        // Convert from source_vrf to heliocentric (frequency domain).
        self.add_sor_to_helio(&mut specmap, sor);

        // Convert frequency back to velocity.
        specmap.add(SpecCvtType::FrToVl, &[rf]);

        // Transform the value.
        use crate::coords::PointSet;
        use crate::mappings::Mapping;
        let pts = PointSet::from_rows([[vel]]);
        match specmap.transform(&pts, true) {
            Ok(out) => {
                let v = out.point(0)[0];
                if is_bad(v) { 0.0 } else { v }
            }
            Err(_) => 0.0,
        }
    }

    /// Add SpecMap steps to convert from a given SOR to heliocentric (frequency domain).
    fn add_sor_to_helio(&self, specmap: &mut SpecMap, from: StdOfRest) {
        let ra = self.ref_ra.unwrap_or(0.0);
        let dec = self.ref_dec.unwrap_or(0.0);
        let lon = self.obs_lon.unwrap_or(0.0);
        let lat = self.obs_lat.unwrap_or(0.0);
        let alt = self.obs_alt.unwrap_or(0.0);
        let epoch = self.epoch_mjd();

        match from {
            StdOfRest::Heliocentric => {} // no-op
            StdOfRest::Topocentric => {
                specmap.add(SpecCvtType::TpF2Hl, &[lon, lat, alt, epoch, ra, dec]);
            }
            StdOfRest::Geocentric => {
                specmap.add(SpecCvtType::GeF2Hl, &[epoch, ra, dec]);
            }
            StdOfRest::Barycentric => {
                specmap.add(SpecCvtType::ByF2Hl, &[epoch, ra, dec]);
            }
            StdOfRest::Lsrk => {
                specmap.add(SpecCvtType::LkF2Hl, &[ra, dec]);
            }
            StdOfRest::Lsrd => {
                specmap.add(SpecCvtType::LdF2Hl, &[ra, dec]);
            }
            StdOfRest::LocalGroup => {
                specmap.add(SpecCvtType::LgF2Hl, &[ra, dec]);
            }
            StdOfRest::Galactic => {
                specmap.add(SpecCvtType::GlF2Hl, &[ra, dec]);
            }
            StdOfRest::Source => {
                let src_vel = self.convert_source_vel_to_helio_vrel();
                specmap.add(SpecCvtType::UsF2Hl, &[src_vel, ra, dec]);
            }
        }
    }

    /// Add SpecMap steps to convert from heliocentric to a given SOR (frequency domain).
    fn add_helio_to_sor(&self, specmap: &mut SpecMap, to: StdOfRest) {
        let ra = self.ref_ra.unwrap_or(0.0);
        let dec = self.ref_dec.unwrap_or(0.0);
        let lon = self.obs_lon.unwrap_or(0.0);
        let lat = self.obs_lat.unwrap_or(0.0);
        let alt = self.obs_alt.unwrap_or(0.0);
        let epoch = self.epoch_mjd();

        match to {
            StdOfRest::Heliocentric => {} // no-op
            StdOfRest::Topocentric => {
                specmap.add(SpecCvtType::HlF2Tp, &[lon, lat, alt, epoch, ra, dec]);
            }
            StdOfRest::Geocentric => {
                specmap.add(SpecCvtType::HlF2Ge, &[epoch, ra, dec]);
            }
            StdOfRest::Barycentric => {
                specmap.add(SpecCvtType::HlF2By, &[epoch, ra, dec]);
            }
            StdOfRest::Lsrk => {
                specmap.add(SpecCvtType::HlF2Lk, &[ra, dec]);
            }
            StdOfRest::Lsrd => {
                specmap.add(SpecCvtType::HlF2Ld, &[ra, dec]);
            }
            StdOfRest::LocalGroup => {
                specmap.add(SpecCvtType::HlF2Lg, &[ra, dec]);
            }
            StdOfRest::Galactic => {
                specmap.add(SpecCvtType::HlF2Gl, &[ra, dec]);
            }
            StdOfRest::Source => {
                let src_vel = self.convert_source_vel_to_helio_vrel();
                specmap.add(SpecCvtType::HlF2Us, &[src_vel, ra, dec]);
            }
        }
    }

    /// Add SpecMap steps to convert from the current system to frequency.
    fn add_system_to_freq(specmap: &mut SpecMap, system: SpecSystem, rest_freq: Option<f64>) {
        match system {
            SpecSystem::Freq => {} // already frequency
            SpecSystem::Energy => specmap.add(SpecCvtType::EnToFr, &[]),
            SpecSystem::Wavenum => specmap.add(SpecCvtType::WnToFr, &[]),
            SpecSystem::Wavelen => specmap.add(SpecCvtType::WvToFr, &[]),
            SpecSystem::Airwave => specmap.add(SpecCvtType::AwToFr, &[]),
            SpecSystem::Vradio => {
                specmap.add(SpecCvtType::VrToVl, &[]);
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::VlToFr, &[rf]);
                }
            }
            SpecSystem::Voptical => {
                specmap.add(SpecCvtType::VoToVl, &[]);
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::VlToFr, &[rf]);
                }
            }
            SpecSystem::Redshift => {
                specmap.add(SpecCvtType::ZoToVl, &[]);
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::VlToFr, &[rf]);
                }
            }
            SpecSystem::Beta => {
                specmap.add(SpecCvtType::BtToVl, &[]);
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::VlToFr, &[rf]);
                }
            }
            SpecSystem::Vrel => {
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::VlToFr, &[rf]);
                }
            }
        }
    }

    /// Add SpecMap steps to convert from frequency to the target system.
    fn add_freq_to_system(specmap: &mut SpecMap, system: SpecSystem, rest_freq: Option<f64>) {
        match system {
            SpecSystem::Freq => {} // already frequency
            SpecSystem::Energy => specmap.add(SpecCvtType::FrToEn, &[]),
            SpecSystem::Wavenum => specmap.add(SpecCvtType::FrToWn, &[]),
            SpecSystem::Wavelen => specmap.add(SpecCvtType::FrToWv, &[]),
            SpecSystem::Airwave => specmap.add(SpecCvtType::FrToAw, &[]),
            SpecSystem::Vradio => {
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::FrToVl, &[rf]);
                }
                specmap.add(SpecCvtType::VlToVr, &[]);
            }
            SpecSystem::Voptical => {
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::FrToVl, &[rf]);
                }
                specmap.add(SpecCvtType::VlToVo, &[]);
            }
            SpecSystem::Redshift => {
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::FrToVl, &[rf]);
                }
                specmap.add(SpecCvtType::VlToZo, &[]);
            }
            SpecSystem::Beta => {
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::FrToVl, &[rf]);
                }
                specmap.add(SpecCvtType::VlToBt, &[]);
            }
            SpecSystem::Vrel => {
                if let Some(rf) = rest_freq {
                    specmap.add(SpecCvtType::FrToVl, &[rf]);
                }
            }
        }
    }

    /// Build the SpecMap for converting from `self` (target) to `result`.
    ///
    /// Follows the C `MakeSpecMapping` algorithm:
    ///   Step 2: target system → frequency (in target SOR)
    ///   Step 3: frequency in target SOR → frequency in alignment SOR
    ///   Step 4: frequency → alignment system (if rest freqs differ)
    ///   Step 5: alignment system → frequency (using result rest freq)
    ///   Step 6: frequency in alignment SOR → frequency in result SOR
    ///   Step 7: frequency → result system (in result SOR)
    fn make_spec_mapping(&self, result: &SpecFrame) -> AstResult<DynMapping> {
        let target_system = self.system;
        let result_system = result.system;
        let target_rf = self.rest_freq.unwrap_or(0.0);
        let result_rf = result.rest_freq.unwrap_or(0.0);

        // Alignment system: if absolute → Freq, else → Vrel.
        let align_system = if target_system.is_absolute() && result_system.is_absolute() {
            SpecSystem::Freq
        } else {
            SpecSystem::Vrel
        };

        // Determine which steps are needed (optimisation from C code).
        let mut step2 = target_system != SpecSystem::Freq;
        let mut step3 = self.std_of_rest != result.std_of_rest;
        let step4;
        let step5;
        let mut step6 = self.std_of_rest != result.std_of_rest;
        let mut step7 = result_system != SpecSystem::Freq;

        // Steps 4 and 5: only needed if alignment is in Vrel AND rest freqs differ.
        if align_system == SpecSystem::Freq || ast_equal(result_rf, target_rf) {
            step4 = false;
            step5 = false;
        } else {
            step4 = true;
            step5 = true;
        }

        // Steps 3 and 6: not needed if steps 4/5 not needed AND SOR same.
        if !step4 && !step5 && self.std_of_rest == result.std_of_rest {
            step3 = false;
            step6 = false;
        }

        // Steps 2 and 7: not needed if steps 3-6 not needed AND same system
        // (for relative systems, also need same rest freq).
        if !step3
            && !step4
            && !step5
            && !step6
            && self.std_of_rest == result.std_of_rest
            && target_system == result_system
            && (target_system.is_absolute() || ast_equal(result_rf, target_rf))
        {
            step2 = false;
            step7 = false;
        }

        // Build the SpecMap.
        let mut specmap = SpecMap::new();

        // Step 2: target system → frequency.
        if step2 {
            Self::add_system_to_freq(&mut specmap, target_system, self.rest_freq);
        }

        // Step 3: frequency in target SOR → frequency in alignment SOR.
        if step3 {
            self.add_sor_to_helio(&mut specmap, self.std_of_rest);
            // Now we are in heliocentric; go to the alignment SOR if it differs.
            // For the target side, the alignment frame inherits target properties.
            // But alignment SOR defaults to heliocentric, so typically this is a no-op
            // for step 3 since we go target→helio, and alignment=helio.
            // Actually per the C code: step 3 is SorConvert(target, align_target)
            // which goes from target's SOR to the alignment frame's SOR (heliocentric default).
            // We simplify: go from self.std_of_rest → helio (the alignment pivot).
            // Already done above. If align_std_of_rest != Heliocentric, adjust:
            let align_sor = self.align_std_of_rest();
            if align_sor != StdOfRest::Heliocentric {
                // helio → alignment SOR (using target's properties)
                self.add_helio_to_sor(&mut specmap, align_sor);
            }
        }

        // Step 4: frequency → alignment system (Vrel).
        if step4 && align_system == SpecSystem::Vrel {
            specmap.add(SpecCvtType::FrToVl, &[target_rf]);
        }

        // Step 5: alignment system → frequency (using result rest freq).
        if step5 && align_system == SpecSystem::Vrel {
            specmap.add(SpecCvtType::VlToFr, &[result_rf]);
        }

        // Step 6: frequency in alignment SOR → frequency in result SOR.
        if step6 {
            let align_sor = result.align_std_of_rest();
            if align_sor != StdOfRest::Heliocentric {
                // alignment SOR → helio (using result's properties)
                result.add_sor_to_helio(&mut specmap, align_sor);
            }
            // helio → result SOR
            result.add_helio_to_sor(&mut specmap, result.std_of_rest);
        }

        // Step 7: frequency → result system.
        if step7 {
            Self::add_freq_to_system(&mut specmap, result_system, result.rest_freq);
        }

        // If the specmap is empty (no steps needed), return a UnitMap.
        if specmap.is_empty() {
            return Ok(Box::new(UnitMap::new(1)));
        }

        Ok(Box::new(specmap))
    }
}

// ---------------------------------------------------------------------------
// Frame trait implementation
// ---------------------------------------------------------------------------

impl Frame for SpecFrame {
    fn naxes(&self) -> usize {
        1
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        if axis != 1 {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("SpecFrame has 1 axis, got axis={axis}"),
            ));
        }
        if is_bad(value) {
            return Ok("<bad>".to_string());
        }
        // Simple numeric formatting with units.
        let units = self.system.specmap_units();
        if units.is_empty() {
            Ok(format!("{value:.6}"))
        } else {
            Ok(format!("{value:.6} {units}"))
        }
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        if axis != 1 {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("SpecFrame has 1 axis, got axis={axis}"),
            ));
        }
        // Parse a floating-point number from the start of the string.
        let trimmed = text.trim_start();
        let offset = text.len() - trimmed.len();
        // Find the end of the numeric portion.
        let mut end = 0;
        for (i, c) in trimmed.char_indices() {
            if c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E' {
                end = i + c.len_utf8();
            } else if end > 0 {
                break;
            }
        }
        if end == 0 {
            return Ok((BAD_VALUE, 0));
        }
        match trimmed[..end].parse::<f64>() {
            Ok(val) => Ok((val, offset + end)),
            Err(_) => Ok((BAD_VALUE, 0)),
        }
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        let va = a[0];
        let vb = b[0];
        if is_bad(va) || is_bad(vb) {
            return Ok(BAD_VALUE);
        }
        Ok((va - vb).abs())
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        // For a 1-D frame, angle at b in triangle a-b-c is either 0 or PI.
        let va = a[0];
        let vb = b[0];
        let vc = c[0];
        if is_bad(va) || is_bad(vb) || is_bad(vc) {
            return Ok(BAD_VALUE);
        }
        let ab = va - vb;
        let cb = vc - vb;
        if ab * cb >= 0.0 {
            Ok(0.0)
        } else {
            Ok(std::f64::consts::PI)
        }
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        let vf = from[0];
        let vt = to[0];
        if is_bad(vf) || is_bad(vt) || is_bad(dist) {
            return Ok(vec![BAD_VALUE]);
        }
        let d = vt - vf;
        if d == 0.0 {
            Ok(vec![vf + dist])
        } else {
            let sign = d.signum();
            Ok(vec![vf + sign * dist])
        }
    }

    fn norm(&self, _coords: &mut [f64]) -> AstResult<()> {
        // Spectral coordinates are unbounded — no normalisation needed.
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        let prev = previous.as_any().downcast_ref::<SpecFrame>();
        match prev {
            Some(prev_spec) => prev_spec.make_spec_mapping(self),
            None => {
                // Previous was not a SpecFrame — return UnitMap.
                Ok(Box::new(UnitMap::new(1)))
            }
        }
    }

    fn convert(&self, to: &dyn Frame, _domains: &[&str]) -> AstResult<Option<FrameSet>> {
        let to_spec = to.as_any().downcast_ref::<SpecFrame>();
        match to_spec {
            Some(to_spec) => {
                let mapping = self.make_spec_mapping(to_spec)?;
                let mut fs = FrameSet::new(self.snapshot());
                fs.add_frame(FrameRef::Base, mapping, to.snapshot())?;
                Ok(Some(fs))
            }
            None => Ok(None),
        }
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        let Some(other) = other.as_any().downcast_ref::<SpecFrame>() else {
            return false;
        };
        if self.system != other.system {
            return false;
        }
        if self.std_of_rest != other.std_of_rest {
            return false;
        }
        let epoch_eq = match (self.epoch, other.epoch) {
            (Some(a), Some(b)) => ast_equal(a, b),
            (None, None) => true,
            _ => false,
        };
        if !epoch_eq {
            return false;
        }
        let rf_eq = match (self.rest_freq, other.rest_freq) {
            (Some(a), Some(b)) => ast_equal(a, b),
            (None, None) => true,
            _ => false,
        };
        if !rf_eq {
            return false;
        }
        self.title == other.title && self.domain == other.domain
    }

    fn obs_lat(&self) -> Option<f64> {
        self.obs_lat
    }

    fn obs_lon(&self) -> Option<f64> {
        self.obs_lon
    }

    fn obs_alt(&self) -> Option<f64> {
        self.obs_alt
    }

    fn system_string(&self) -> Option<&str> {
        Some(self.system.as_str())
    }
}

// ---------------------------------------------------------------------------
// Serialisation
// ---------------------------------------------------------------------------

impl Serializable for SpecFrame {
    fn class_name(&self) -> &'static str {
        "SpecFrame"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Description of spectral coordinate system".to_string());

        // --- Frame-level attributes (before IsA Frame) ---
        // Order: Title, Naxes, Domain, Epoch, Lbl1, System, Uni1, Ax1,
        //        [observer], IsA Frame

        // Title: active if explicitly set, otherwise unset with default.
        if let Some(ref title) = self.title {
            if !title.is_empty() {
                node.push_string(
                    "Title",
                    title,
                    Some("Title of coordinate system".to_string()),
                );
            }
        } else {
            node.push_unset(
                "Title",
                NodeValue::String(self.default_title()),
                Some("Title of coordinate system".to_string()),
            );
        }

        // Naxes = 1 (always active).
        node.push_int("Naxes", 1, Some("Number of coordinate axes".to_string()));

        // Domain: active if explicitly set to non-"SPECTRUM", otherwise unset.
        if let Some(ref domain) = self.domain {
            if !domain.is_empty() && domain != "SPECTRUM" {
                node.push_string(
                    "Domain",
                    domain,
                    Some("Coordinate system domain".to_string()),
                );
            } else {
                node.push_unset(
                    "Domain",
                    NodeValue::String("SPECTRUM".to_string()),
                    Some("Coordinate system domain".to_string()),
                );
            }
        } else {
            node.push_unset(
                "Domain",
                NodeValue::String("SPECTRUM".to_string()),
                Some("Coordinate system domain".to_string()),
            );
        }

        // Epoch: active if explicitly set, otherwise unset with default 2000.
        if let Some(epoch) = self.epoch {
            node.push_double(
                "Epoch",
                epoch,
                Some("Julian epoch of observation".to_string()),
            );
        } else {
            node.push_unset(
                "Epoch",
                NodeValue::Double(2000.0),
                Some("Julian epoch of observation".to_string()),
            );
        }

        // Lbl1: axis label (always unset with system-appropriate default).
        node.push_unset(
            "Lbl1",
            NodeValue::String(self.system.label().to_string()),
            Some("Label for axis 1".to_string()),
        );

        // System (always active).
        node.push_string(
            "System",
            self.system.as_str(),
            Some("Coordinate system type".to_string()),
        );

        // Uni1: axis unit (always unset with current unit or system default).
        let display_unit = self
            .unit
            .as_deref()
            .unwrap_or(self.system.default_display_units());
        if !display_unit.is_empty() {
            node.push_unset(
                "Uni1",
                NodeValue::String(display_unit.to_string()),
                Some("Units for axis 1".to_string()),
            );
        }

        // Ax1: axis child object with unit (if unit is set or non-empty default).
        let mut axis_node = Node::new("Axis");
        axis_node.class_comment = Some("Coordinate axis".to_string());
        if let Some(ref unit) = self.unit {
            axis_node.push_string("Unit", unit, Some("Axis units".to_string()));
        }
        node.push_object("Ax1", axis_node, Some("Axis number 1".to_string()));

        // Observer attributes (active only if explicitly set).
        if let Some(obs_lat) = self.obs_lat {
            node.push_double(
                "ObsLat",
                obs_lat,
                Some("Observers geodetic latitude (rads)".to_string()),
            );
        }
        if let Some(obs_lon) = self.obs_lon {
            node.push_double(
                "ObsLon",
                obs_lon,
                Some("Observers geodetic longitude (rads)".to_string()),
            );
        }
        if let Some(obs_alt) = self.obs_alt {
            node.push_double(
                "ObsAlt",
                obs_alt,
                Some("Observers geodetic altitude (m)".to_string()),
            );
        }

        // IsA Frame marker.
        node.push_isa("Frame", Some("Coordinate system description".to_string()));

        // --- SpecFrame-specific attributes (after IsA Frame) ---

        // SoR (StdOfRest) — only write if explicitly set.
        if self.sor_set {
            node.push_string(
                "SoR",
                self.std_of_rest.as_str(),
                Some("Standard of rest".to_string()),
            );
        }

        // Unit preference (UFreq, UWave, etc.) — only if explicitly set.
        if let Some(ref unit) = self.unit {
            node.push_string(
                self.system.unit_attr_key(),
                unit,
                Some(self.system.unit_attr_comment().to_string()),
            );
        }

        // AlSoR (AlignStdOfRest, only if explicitly set).
        if let Some(ref asor) = self.align_std_of_rest {
            node.push_string(
                "AlSoR",
                asor.as_str(),
                Some("Alignment standard of rest".to_string()),
            );
        }

        // RefRA (if set).
        if let Some(ref_ra) = self.ref_ra {
            node.push_double(
                "RefRA",
                ref_ra,
                Some("Reference RA (rads, FK5 J2000)".to_string()),
            );
        }

        // RefDec (if set).
        if let Some(ref_dec) = self.ref_dec {
            node.push_double(
                "RefDec",
                ref_dec,
                Some("Reference Dec (rads, FK5 J2000)".to_string()),
            );
        }

        // RstFrq (RestFreq, if set).
        if let Some(rest_freq) = self.rest_freq {
            node.push_double("RstFrq", rest_freq, Some("Rest frequency (Hz)".to_string()));
        }

        // SrcVel (SourceVel, if set).
        if let Some(source_vel) = self.source_vel {
            node.push_double(
                "SrcVel",
                source_vel,
                Some("Source velocity (m/s)".to_string()),
            );
        }

        // SrcVRF (SourceVRF, if set).
        if let Some(ref svrf) = self.source_vrf {
            node.push_string(
                "SrcVRF",
                svrf.as_str(),
                Some("Source velocity rest frame".to_string()),
            );
        }

        // SrcSys (SourceSys, if set).
        if let Some(ref ssys) = self.source_sys {
            node.push_string(
                "SrcSys",
                ssys.as_str(),
                Some("Source velocity spectral system".to_string()),
            );
        }

        // AlSpOf (AlignSpecOffset, if true).
        if self.align_spec_offset {
            node.push_int("AlSpOf", 1, Some("Align in offset coords".to_string()));
        }

        // SpOrg (SpecOrigin, if set).
        if let Some(spec_origin) = self.spec_origin {
            if !is_bad(spec_origin) {
                node.push_double("SpOrg", spec_origin, Some("Spec offset".to_string()));
            }
        }

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Read System.
        let sys_str = node.get_string("System", "FREQ");
        let system = SpecSystem::parse(&sys_str).unwrap_or(SpecSystem::Freq);

        let mut sf = SpecFrame::new(system);

        // Title.
        let title = node.get_string("Title", "");
        if !title.is_empty() {
            sf.set_title(&title);
        }

        // Domain.
        let domain = node.get_string("Domain", "");
        if !domain.is_empty() {
            sf.domain = Some(domain);
        }

        // Epoch (MJD).
        if node.get("Epoch").is_some() {
            sf.epoch = Some(node.get_double("Epoch", 0.0));
        }

        // Observer attributes.
        if node.get("ObsLat").is_some() {
            sf.obs_lat = Some(node.get_double("ObsLat", 0.0));
        }
        if node.get("ObsLon").is_some() {
            sf.obs_lon = Some(node.get_double("ObsLon", 0.0));
        }
        if node.get("ObsAlt").is_some() {
            sf.obs_alt = Some(node.get_double("ObsAlt", 0.0));
        }

        // SoR (StdOfRest).
        let sor_str = node.get_string("SoR", "Heliocentric");
        if let Some(sor) = StdOfRest::parse(&sor_str) {
            sf.std_of_rest = sor;
            if node.get("SoR").is_some() {
                sf.sor_set = true;
            }
        }

        // Unit preference (UFreq, UWave, etc.).
        let unit_key = system.unit_attr_key();
        let unit_str = node.get_string(unit_key, "");
        if !unit_str.is_empty() {
            sf.unit = Some(unit_str);
        } else {
            // Fall back to Axis child Unit attribute.
            if let Some(axis_node) = node.get_object("Ax1") {
                let axis_unit = axis_node.get_string("Unit", "");
                if !axis_unit.is_empty() {
                    sf.unit = Some(axis_unit);
                }
            }
        }

        // AlSoR (AlignStdOfRest).
        let alsor_str = node.get_string("AlSoR", "");
        if !alsor_str.is_empty() {
            sf.align_std_of_rest = StdOfRest::parse(&alsor_str);
        }

        // RefRA.
        if node.get("RefRA").is_some() {
            sf.ref_ra = Some(node.get_double("RefRA", 0.0));
        }

        // RefDec.
        if node.get("RefDec").is_some() {
            sf.ref_dec = Some(node.get_double("RefDec", 0.0));
        }

        // RstFrq (RestFreq).
        if node.get("RstFrq").is_some() {
            sf.rest_freq = Some(node.get_double("RstFrq", 0.0));
        }

        // SrcVel (SourceVel).
        if node.get("SrcVel").is_some() {
            sf.source_vel = Some(node.get_double("SrcVel", 0.0));
        }

        // SrcVRF (SourceVRF).
        let svrf_str = node.get_string("SrcVRF", "");
        if !svrf_str.is_empty() {
            sf.source_vrf = StdOfRest::parse(&svrf_str);
        }

        // SrcSys (SourceSys).
        let ssys_str = node.get_string("SrcSys", "");
        if !ssys_str.is_empty() {
            sf.source_sys = SpecSystem::parse(&ssys_str);
        }

        // AlSpOf (AlignSpecOffset).
        if node.get_int("AlSpOf", 0) != 0 {
            sf.align_spec_offset = true;
        }

        // SpOrg (SpecOrigin).
        if node.get("SpOrg").is_some() {
            sf.spec_origin = Some(node.get_double("SpOrg", 0.0));
        }

        Ok(sf)
    }
}
