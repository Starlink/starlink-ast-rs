//! Intermediate storage for extracted FITS WCS keywords.
//!
//! [`FitsStore`] holds all the FITS-WCS keyword values extracted from a
//! header in a structured form ready for interpretation by the WCS
//! pipeline. This mirrors the C library's `FitsStore` structure.

/// Intermediate storage for extracted FITS WCS keywords.
///
/// All axis-indexed vectors use 0-based indexing internally (i.e. axis 1
/// is at index 0). The `naxis` field records the number of WCS axes.
#[derive(Debug, Clone)]
pub(crate) struct FitsStore {
    /// Number of WCS axes.
    pub naxis: usize,
    /// Reference pixel per axis (CRPIXi).
    pub crpix: Vec<Option<f64>>,
    /// Reference value per axis (CRVALi).
    pub crval: Vec<Option<f64>>,
    /// Pixel scale per axis (CDELTi).
    pub cdelt: Vec<Option<f64>>,
    /// Image dimension per axis (NAXISi), when known. Mirrors the
    /// `dim` array C threads through WcsMapFrm/SIPMapping for the
    /// astPolyTran sampling bounds.
    pub dims: Vec<Option<f64>>,
    /// The reading FitsChan's SipReplace attribute (C default 1).
    pub sip_replace: bool,
    /// The reading FitsChan's CarLin attribute (C default 0): read `-CAR`
    /// projections as a linear pixel-to-sky mapping.
    pub car_lin: bool,
    /// PC rotation matrix, shape [row][col] (PCi_j).
    pub pc: Vec<Vec<Option<f64>>>,
    /// CD matrix, shape [row][col] (CDi_j).
    pub cd: Vec<Vec<Option<f64>>>,
    /// Axis type code per axis (CTYPEi), with distortion codes removed.
    pub ctype: Vec<Option<String>>,
    /// Axis-specific CTYPE comment per axis. When `None`, the writer
    /// falls back to `"Type of co-ordinate on axis {n}"`. Mirrors C's
    /// `store->ctype_com` populated by `OtherAxes` (fitschan.c:24491).
    pub ctype_com: Vec<Option<String>>,
    /// Axis units per axis (CUNITi).
    pub cunit: Vec<Option<String>>,
    /// Axis name per axis (CNAMEi).
    pub cname: Vec<Option<String>>,
    /// Projection parameters per axis, shape [axis][param] (PVi_m).
    pub pv: Vec<Vec<Option<f64>>>,
    /// String parameters per axis, shape [axis][param] (PSi_m): for a
    /// `-TAB` axis, the extension name, coordinate array column and index
    /// vector column of its look-up table.
    pub ps: Vec<Vec<Option<String>>>,
    /// Reference frame for celestial coordinates (RADESYS).
    pub radesys: Option<String>,
    /// Equinox of celestial coordinate system (EQUINOX).
    pub equinox: Option<f64>,
    /// Native longitude of the celestial pole (LONPOLE).
    pub lonpole: Option<f64>,
    /// Native latitude of the celestial pole (LATPOLE).
    pub latpole: Option<f64>,
    /// Modified Julian Date of observation (MJD-OBS).
    pub mjdobs: Option<f64>,
    /// Time scale used for `mjdobs` and `dateobs` (FITS TIMESYS keyword).
    ///
    /// FITS standard default is UTC when the keyword is absent. Stored
    /// as the raw uppercase string (`"UTC"`, `"TAI"`, `"TT"`, `"TDB"`,
    /// etc.). Mirrors fitschan.c:29843 / 6144.
    pub timesys: Option<String>,
    /// Formatted observation date string (DATE-OBS).
    ///
    /// Pre-formatted by `populate_celestial` from the (UTC-converted)
    /// MJD-OBS using the C library's format convention: ISO-8601
    /// `YYYY-MM-DDTHH:MM:SS.SSS` for MJD ≥ 51179 (1999-01-01), legacy
    /// `DD/MM/YY` for earlier dates. Mirrors fitschan.c:3667-3681.
    pub dateobs: Option<String>,
    /// Spectral reference frame (SPECSYS).
    pub specsys: Option<String>,
    /// Rest frequency in Hz (RESTFRQ).
    pub restfrq: Option<f64>,
    /// Rest wavelength in m (RESTWAV).
    pub restwav: Option<f64>,
    /// Topocentric apparent velocity of the rest frame (VELOSYS, m/s).
    pub velosys: Option<f64>,
    /// Number of WCS axes as declared (WCSAXES).
    pub wcsaxes: Option<usize>,
    // -- SIP distortion coefficients --
    /// Set by the read path when both of the first two CTYPE values carried a
    /// `-SIP` distortion code. C has no equivalent field — `DistortMaps` tests
    /// the CTYPE values and calls `SIPMapping` in the same breath, before
    /// truncating them (fitschan.c:8919-8942) — so this records the answer for
    /// the later `distort_maps` step, which sees only the stripped values.
    pub sip: bool,
    /// SIP forward polynomial order for axis 1 (A_ORDER). Write path only; the
    /// read path derives the powers from the coefficient tables as C does.
    pub sip_a_order: Option<usize>,
    /// SIP forward polynomial order for axis 2 (B_ORDER). Write path only.
    pub sip_b_order: Option<usize>,
    /// SIP A coefficients, shape [i][j]. Absent entries hold
    /// [`BAD_VALUE`](crate::constants::BAD_VALUE), mirroring the `AST__BAD`
    /// C `GetItem` reports for an unset `FitsStore` slot.
    pub sip_a: Vec<Vec<f64>>,
    /// SIP B coefficients, shape [i][j].
    pub sip_b: Vec<Vec<f64>>,
    /// SIP inverse polynomial order for axis 1 (AP_ORDER). Write path only.
    pub sip_ap_order: Option<usize>,
    /// SIP inverse polynomial order for axis 2 (BP_ORDER). Write path only.
    pub sip_bp_order: Option<usize>,
    /// SIP AP coefficients (inverse), shape [i][j].
    pub sip_ap: Vec<Vec<f64>>,
    /// SIP BP coefficients (inverse), shape [i][j].
    pub sip_bp: Vec<Vec<f64>>,
    // -- AST-extension SkyFrame offset-coords (fitschan.c::SkySys ~29927) --
    /// `SkyRefIs`-equivalent string: `"POLE"` / `"ORIGIN"` / `"IGNORED"`.
    /// Only set when SkySys is called with `isoff < 0` (describe absolute
    /// coords but include the offset metadata).
    pub skyrefis: Option<String>,
    /// `SkyRef` reference-point coords per axis (radians, FITS axis order).
    pub skyref: Vec<Option<f64>>,
    /// `SkyRefP` reference-point pole coords per axis.
    pub skyrefp: Vec<Option<f64>>,
    /// `AXREF<i>` values in degrees, indexed by 0-based FITS axis. A
    /// non-standard keyword AST uses to communicate the reference position on
    /// axes described by `-TAB` that have no inverse transformation; C writes
    /// it from `store->axref` in `WcsFromStore` (fitschan.c:37367-37379). On
    /// read it overrides the CRVAL-derived sky reference position and forces
    /// SkyRef to be set (fitschan.c:36338-36352).
    pub axref: Vec<Option<f64>>,
    // -- WCSNAME (fitschan.c:30071) --
    /// `WCSNAMEa` from the SkyFrame's `Domain` attribute.
    pub wcsname: Option<String>,
    // -- OBSGEO-X/Y/Z (fitschan.c:30076-30086) --
    /// Geocentric observer position X (meters).
    pub obsgeox: Option<f64>,
    /// Geocentric observer position Y (meters).
    pub obsgeoy: Option<f64>,
    /// Geocentric observer position Z (meters).
    pub obsgeoz: Option<f64>,
    // -- Spectral source-velocity AST extensions
    //    (fitschan.c::SpectralAxes ~30761-30797) --
    /// `ZSOURCE` — source redshift.
    pub zsource: Option<f64>,
    /// `SSYSSRC` — rest frame in which `SourceVel` was specified.
    pub ssyssrc: Option<String>,
    /// `IMAGFREQ` — image-sideband frequency for DSB SpecFrames.
    /// Only populated by `spectral_axes` once the DSBSpecFrame port
    /// lands; the FITS-WCS encoder reads it on output.
    pub imagfreq: Option<f64>,
    /// TAI - UTC offset (seconds). Mirrors the FITS DTAI keyword.
    /// Global per-FrameSet field; emit and read once per header.
    pub dtai: Option<f64>,
    /// UT1 - UTC offset (days). Mirrors the FITS DUT1 keyword.
    /// Global per-FrameSet field; emit and read once per header.
    pub dut1: Option<f64>,
}

/// Maximum number of PV parameters per axis.
const MAX_PV_PARAMS: usize = 40;

impl FitsStore {
    /// Create a new FitsStore for the given number of axes.
    ///
    /// All vectors are initialized to the appropriate size with `None`.
    pub(crate) fn new(naxis: usize) -> Self {
        Self {
            naxis,
            crpix: vec![None; naxis],
            crval: vec![None; naxis],
            cdelt: vec![None; naxis],
            dims: vec![None; naxis],
            sip_replace: true,
            car_lin: false,
            pc: vec![vec![None; naxis]; naxis],
            cd: vec![vec![None; naxis]; naxis],
            ctype: vec![None; naxis],
            ctype_com: vec![None; naxis],
            cunit: vec![None; naxis],
            cname: vec![None; naxis],
            pv: vec![vec![None; MAX_PV_PARAMS]; naxis],
            ps: vec![vec![None; MAX_PV_PARAMS]; naxis],
            radesys: None,
            equinox: None,
            lonpole: None,
            latpole: None,
            mjdobs: None,
            timesys: None,
            dateobs: None,
            specsys: None,
            restfrq: None,
            restwav: None,
            velosys: None,
            wcsaxes: None,
            sip: false,
            sip_a_order: None,
            sip_b_order: None,
            sip_a: Vec::new(),
            sip_b: Vec::new(),
            sip_ap_order: None,
            sip_bp_order: None,
            sip_ap: Vec::new(),
            sip_bp: Vec::new(),
            skyrefis: None,
            skyref: vec![None; naxis],
            skyrefp: vec![None; naxis],
            axref: vec![None; naxis],
            wcsname: None,
            obsgeox: None,
            obsgeoy: None,
            obsgeoz: None,
            zsource: None,
            ssyssrc: None,
            imagfreq: None,
            dtai: None,
            dut1: None,
        }
    }

    /// Returns `true` if any PC matrix element has been set.
    #[cfg(test)]
    pub(crate) fn has_pc_matrix(&self) -> bool {
        self.pc.iter().any(|row| row.iter().any(|v| v.is_some()))
    }

    /// Returns `true` if the header declared `-SIP` distortion on both of the
    /// first two axes. Note this is independent of whether any coefficient
    /// cards were present: C `DistortMaps` calls `SIPMapping` on the strength
    /// of the CTYPE codes alone (fitschan.c:8931), and `SIPMapping` answers
    /// with a UnitMap when the store holds no coefficients.
    pub(crate) fn has_sip(&self) -> bool {
        self.sip
    }

    /// Find the pair of celestial (longitude, latitude) axes from CTYPE values.
    ///
    /// Returns `Some((lon_axis, lat_axis))` using 0-based indices, or `None`
    /// if no recognizable celestial axis pair is found.
    pub(crate) fn celestial_axes(&self) -> Option<(usize, usize)> {
        let mut lon_axis = None;
        let mut lat_axis = None;

        for (i, ct) in self.ctype.iter().enumerate() {
            if let Some(ctype) = ct {
                let ctype_upper = ctype.to_uppercase();
                if is_longitude_ctype(&ctype_upper) {
                    lon_axis = Some(i);
                } else if is_latitude_ctype(&ctype_upper) {
                    lat_axis = Some(i);
                }
            }
        }

        match (lon_axis, lat_axis) {
            (Some(lon), Some(lat)) => Some((lon, lat)),
            _ => None,
        }
    }

    /// C's `GetMaxJM( &(store->crpix), ... )`: the highest axis index with
    /// a CRPIX, or `None` when there is none.
    pub(crate) fn max_crpix_axis(&self) -> Option<usize> {
        self.crpix.iter().rposition(|v| v.is_some())
    }

    /// C's `GetMaxJM( &(store->pv), ... )`: the highest PV parameter index
    /// stored for any axis, or `None` when there is none.
    pub(crate) fn max_pv_m(&self) -> Option<usize> {
        self.pv
            .iter()
            .filter_map(|axis| axis.iter().rposition(|v| v.is_some()))
            .max()
    }

    /// The CD matrix element `(i, j)` as C's writers form it, `PCi_j` times
    /// `CDELTi`, with the unset values defaulting to the unit matrix and a
    /// CDELT of 1.
    pub(crate) fn cd_element(&self, i: usize, j: usize) -> f64 {
        let cdelt = self.cdelt.get(i).copied().flatten().unwrap_or(1.0);
        let pc = self
            .pc
            .get(i)
            .and_then(|row| row.get(j))
            .copied()
            .flatten()
            .unwrap_or(if i == j { 1.0 } else { 0.0 });
        pc * cdelt
    }

    /// The longitude, latitude and spectral axes among the first WCSAXES
    /// (or NAXIS) CTYPE values: C's `FindLonLatSpecAxes`
    /// (fitschan.c:10822). A spectral axis is one `IsSpectral` or
    /// `IsAIPSSpectral` recognizes; a celestial axis has `-` as its fifth
    /// character, and is a longitude axis for `RA--`, `AZ--`, `xLON` and
    /// `xyLN`, and a latitude axis for `DEC-`, `EL--`, `xLAT` and `xyLT`.
    /// The last axis of each kind is the one given.
    pub(crate) fn lon_lat_spec_axes(&self) -> (Option<usize>, Option<usize>, Option<usize>) {
        let (mut axlon, mut axlat, mut axspec) = (None, None, None);
        let wcsaxes = self.wcsaxes.unwrap_or(self.naxis);
        for i in 0..wcsaxes {
            let Some(ctype) = self.ctype.get(i).and_then(|c| c.as_deref()) else {
                continue;
            };
            let b = ctype.as_bytes();
            if is_spectral(ctype).is_some()
                || crate::fitschan::guess_encoding::is_aips_spectral(ctype).is_some()
            {
                axspec = Some(i);
            } else if b.len() > 4 && b[4] == b'-' {
                if b.starts_with(b"RA--")
                    || b.starts_with(b"AZ--")
                    || &b[1..4] == b"LON"
                    || &b[2..4] == b"LN"
                {
                    axlon = Some(i);
                } else if b.starts_with(b"DEC-")
                    || b.starts_with(b"EL--")
                    || &b[1..4] == b"LAT"
                    || &b[2..4] == b"LT"
                {
                    axlat = Some(i);
                }
            }
        }
        (axlon, axlat, axspec)
    }

    /// Find the spectral axis from CTYPE values.
    ///
    /// Returns the 0-based axis index, or `None` if no spectral axis found.
    pub(crate) fn spectral_axis(&self) -> Option<usize> {
        for (i, ct) in self.ctype.iter().enumerate() {
            if let Some(ctype) = ct {
                if is_spectral_ctype(ctype) {
                    return Some(i);
                }
            }
        }
        None
    }
}

/// C's `EQUALANG` (fitschan.c:1596): two angles equal, both bad, or within
/// the larger of 1.0E5 times their summed magnitudes times the machine
/// precision and 1.0E-9.
pub(crate) fn equal_ang(a: f64, b: f64) -> bool {
    use crate::constants::BAD_VALUE;
    if a == BAD_VALUE || b == BAD_VALUE {
        return a == BAD_VALUE && b == BAD_VALUE;
    }
    (a - b).abs() <= (1.0e5 * (a.abs() + b.abs()) * f64::EPSILON).max(1.0e-9)
}

/// C's `!strncmp( cval, target, astChrLen( cval ) )`: whether `cval`,
/// without its trailing blanks, is the start of `target`.
pub(crate) fn names_start_of(cval: &str, target: &str) -> bool {
    let n = cval.trim_end_matches(' ').len();
    n <= target.len() && cval.as_bytes()[..n] == target.as_bytes()[..n]
}

/// C's `IsSpectral` (fitschan.c:20576): the default unit of the spectral
/// system a CTYPE names in its first four characters (trailing spaces
/// ignored), when any algorithm code after them (at most four
/// characters, trailing spaces ignored) is blank or starts with `-`.
pub(crate) fn is_spectral(ctype: &str) -> Option<&'static str> {
    if ctype.len() < 4 || !ctype.is_char_boundary(4) {
        return None;
    }
    let stype = ctype[..4].trim_end_matches(' ');
    let rest = &ctype[4..];
    let algcode = rest.get(..4).unwrap_or(rest).trim_end_matches(' ');
    let unit = match stype {
        "FREQ" => "Hz",
        "ENER" => "J",
        "WAVN" => "/m",
        "VRAD" | "VOPT" | "VELO" => "m/s",
        "WAVE" | "AWAV" => "m",
        "ZOPT" | "BETA" => "",
        _ => return None,
    };
    (algcode.is_empty() || algcode.starts_with('-')).then_some(unit)
}

/// Check if a CTYPE value represents a celestial longitude axis.
///
/// Mirrors C `GetSkyAxes` (fitschan.c:10809-10819): a celestial axis has a
/// `-` as its fifth character; a longitude axis is `RA--`, `AZ--`, `xLON`
/// (chars 1..4 == "LON"), or `xyLN` (chars 2..4 == "LN").
fn is_longitude_ctype(ctype: &str) -> bool {
    let ct = ctype.trim();
    let b = ct.as_bytes();
    if b.len() <= 4 || b[4] != b'-' {
        return false;
    }
    ct.starts_with("RA--") || ct.starts_with("AZ--") || &ct[1..4] == "LON" || &ct[2..4] == "LN"
}

/// Check if a CTYPE value represents a celestial latitude axis.
///
/// Mirrors C `GetSkyAxes` (fitschan.c:10821-10825): latitude is `DEC-`,
/// `EL--`, `xLAT` (chars 1..4 == "LAT"), or `xyLT` (chars 2..4 == "LT").
fn is_latitude_ctype(ctype: &str) -> bool {
    let ct = ctype.trim();
    let b = ct.as_bytes();
    if b.len() <= 4 || b[4] != b'-' {
        return false;
    }
    ct.starts_with("DEC-") || ct.starts_with("EL--") || &ct[1..4] == "LAT" || &ct[2..4] == "LT"
}

/// Check if a CTYPE value represents a spectral axis.
///
/// Recognized patterns: "FREQ", "ENER", "WAVN", "VOPT", "ZOPT",
/// "VELO", "VRAD", "WAVE", "AWAV", "BETA".
pub(crate) fn is_spectral_ctype(ctype: &str) -> bool {
    let ct = ctype.trim().to_uppercase();
    let prefix = if ct.len() >= 4 { &ct[..4] } else { &ct };
    matches!(
        prefix,
        "FREQ" | "ENER" | "WAVN" | "VOPT" | "ZOPT" | "VELO" | "VRAD" | "WAVE" | "AWAV" | "BETA"
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_correct_sizes() {
        let store = FitsStore::new(2);
        assert_eq!(store.naxis, 2);
        assert_eq!(store.crpix.len(), 2);
        assert_eq!(store.crval.len(), 2);
        assert_eq!(store.cdelt.len(), 2);
        assert_eq!(store.pc.len(), 2);
        assert_eq!(store.pc[0].len(), 2);
        assert_eq!(store.cd.len(), 2);
        assert_eq!(store.cd[0].len(), 2);
        assert_eq!(store.ctype.len(), 2);
        assert_eq!(store.cunit.len(), 2);
        assert_eq!(store.cname.len(), 2);
        assert_eq!(store.pv.len(), 2);
        assert_eq!(store.pv[0].len(), MAX_PV_PARAMS);
    }

    #[test]
    fn new_creates_larger_store() {
        let store = FitsStore::new(4);
        assert_eq!(store.naxis, 4);
        assert_eq!(store.pc.len(), 4);
        assert_eq!(store.pc[0].len(), 4);
    }

    #[test]
    fn has_pc_matrix_detection() {
        let mut store = FitsStore::new(2);
        assert!(!store.has_pc_matrix());
        store.pc[0][1] = Some(0.5);
        assert!(store.has_pc_matrix());
    }

    #[test]
    fn has_sip_detection() {
        let mut store = FitsStore::new(2);
        assert!(!store.has_sip());
        store.sip = true;
        assert!(store.has_sip());
    }

    #[test]
    fn celestial_axes_ra_dec() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        let (lon, lat) = store.celestial_axes().unwrap();
        assert_eq!(lon, 0);
        assert_eq!(lat, 1);
    }

    #[test]
    fn celestial_axes_galactic() {
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("FREQ".to_string());
        store.ctype[1] = Some("GLON-CAR".to_string());
        store.ctype[2] = Some("GLAT-CAR".to_string());
        let (lon, lat) = store.celestial_axes().unwrap();
        assert_eq!(lon, 1);
        assert_eq!(lat, 2);
    }

    #[test]
    fn celestial_axes_none() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("FREQ".to_string());
        store.ctype[1] = Some("STOKES".to_string());
        assert!(store.celestial_axes().is_none());
    }

    #[test]
    fn spectral_axis_detection() {
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.ctype[2] = Some("FREQ".to_string());
        assert_eq!(store.spectral_axis(), Some(2));
    }

    #[test]
    fn spectral_axis_none() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        assert!(store.spectral_axis().is_none());
    }
}
