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
    /// PC rotation matrix, shape [row][col] (PCi_j).
    pub pc: Vec<Vec<Option<f64>>>,
    /// CD matrix, shape [row][col] (CDi_j).
    pub cd: Vec<Vec<Option<f64>>>,
    /// Axis type code per axis (CTYPEi), with distortion codes removed.
    pub ctype: Vec<Option<String>>,
    /// Axis units per axis (CUNITi).
    pub cunit: Vec<Option<String>>,
    /// Axis name per axis (CNAMEi).
    pub cname: Vec<Option<String>>,
    /// Projection parameters per axis, shape [axis][param] (PVi_m).
    pub pv: Vec<Vec<Option<f64>>>,
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
    /// Spectral reference frame (SPECSYS).
    pub specsys: Option<String>,
    /// Rest frequency in Hz (RESTFRQ).
    pub restfrq: Option<f64>,
    /// Rest wavelength in m (RESTWAV).
    pub restwav: Option<f64>,
    /// Number of WCS axes as declared (WCSAXES).
    pub wcsaxes: Option<usize>,
    // -- SIP distortion coefficients --
    /// SIP forward polynomial order for axis 1 (A_ORDER).
    pub sip_a_order: Option<usize>,
    /// SIP forward polynomial order for axis 2 (B_ORDER).
    pub sip_b_order: Option<usize>,
    /// SIP A coefficients, shape [i][j].
    pub sip_a: Vec<Vec<f64>>,
    /// SIP B coefficients, shape [i][j].
    pub sip_b: Vec<Vec<f64>>,
    /// SIP inverse polynomial order for axis 1 (AP_ORDER).
    pub sip_ap_order: Option<usize>,
    /// SIP inverse polynomial order for axis 2 (BP_ORDER).
    pub sip_bp_order: Option<usize>,
    /// SIP AP coefficients (inverse), shape [i][j].
    pub sip_ap: Vec<Vec<f64>>,
    /// SIP BP coefficients (inverse), shape [i][j].
    pub sip_bp: Vec<Vec<f64>>,
}

/// Maximum number of PV parameters per axis.
const MAX_PV_PARAMS: usize = 40;

impl FitsStore {
    /// Create a new FitsStore for the given number of axes.
    ///
    /// All vectors are initialised to the appropriate size with `None`.
    pub(crate) fn new(naxis: usize) -> Self {
        Self {
            naxis,
            crpix: vec![None; naxis],
            crval: vec![None; naxis],
            cdelt: vec![None; naxis],
            pc: vec![vec![None; naxis]; naxis],
            cd: vec![vec![None; naxis]; naxis],
            ctype: vec![None; naxis],
            cunit: vec![None; naxis],
            cname: vec![None; naxis],
            pv: vec![vec![None; MAX_PV_PARAMS]; naxis],
            radesys: None,
            equinox: None,
            lonpole: None,
            latpole: None,
            mjdobs: None,
            specsys: None,
            restfrq: None,
            restwav: None,
            wcsaxes: None,
            sip_a_order: None,
            sip_b_order: None,
            sip_a: Vec::new(),
            sip_b: Vec::new(),
            sip_ap_order: None,
            sip_bp_order: None,
            sip_ap: Vec::new(),
            sip_bp: Vec::new(),
        }
    }

    /// Returns `true` if any CD matrix element has been set.
    pub(crate) fn has_cd_matrix(&self) -> bool {
        self.cd.iter().any(|row| row.iter().any(|v| v.is_some()))
    }

    /// Returns `true` if any PC matrix element has been set.
    pub(crate) fn has_pc_matrix(&self) -> bool {
        self.pc.iter().any(|row| row.iter().any(|v| v.is_some()))
    }

    /// Returns `true` if SIP distortion coefficients are present.
    pub(crate) fn has_sip(&self) -> bool {
        self.sip_a_order.is_some() || self.sip_b_order.is_some()
    }

    /// Find the pair of celestial (longitude, latitude) axes from CTYPE values.
    ///
    /// Returns `Some((lon_axis, lat_axis))` using 0-based indices, or `None`
    /// if no recognisable celestial axis pair is found.
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

    /// Extract the projection code from a CTYPE value.
    ///
    /// For CTYPE like `"RA---TAN"` returns `Some("TAN")`. The projection
    /// code is the last 3 characters after the last `'-'` in the 5-8
    /// character projection portion. Returns `None` if no valid projection
    /// code is found.
    pub(crate) fn projection_code(&self, axis: usize) -> Option<String> {
        if axis >= self.naxis {
            return None;
        }
        self.ctype[axis]
            .as_ref()
            .and_then(|ct| extract_projection_code(ct))
    }
}

/// Check if a CTYPE value represents a celestial longitude axis.
///
/// Recognised patterns: starts with "RA--", "GLON", "ELON", "HLON", "SLON".
fn is_longitude_ctype(ctype: &str) -> bool {
    let ct = ctype.trim();
    ct.starts_with("RA--")
        || ct.starts_with("RA-") && ct.len() >= 8 && ct.as_bytes().get(3) == Some(&b'-')
        || ct.starts_with("GLON")
        || ct.starts_with("ELON")
        || ct.starts_with("HLON")
        || ct.starts_with("SLON")
}

/// Check if a CTYPE value represents a celestial latitude axis.
///
/// Recognised patterns: starts with "DEC-", "GLAT", "ELAT", "HLAT", "SLAT".
fn is_latitude_ctype(ctype: &str) -> bool {
    let ct = ctype.trim();
    ct.starts_with("DEC-")
        || ct.starts_with("DEC--")
        || ct.starts_with("GLAT")
        || ct.starts_with("ELAT")
        || ct.starts_with("HLAT")
        || ct.starts_with("SLAT")
}

/// Check if a CTYPE value represents a spectral axis.
///
/// Recognised patterns: "FREQ", "ENER", "WAVN", "VOPT", "ZOPT",
/// "VELO", "VRAD", "WAVE", "AWAV", "BETA".
fn is_spectral_ctype(ctype: &str) -> bool {
    let ct = ctype.trim().to_uppercase();
    let prefix = if ct.len() >= 4 { &ct[..4] } else { &ct };
    matches!(
        prefix,
        "FREQ" | "ENER" | "WAVN" | "VOPT" | "ZOPT" | "VELO" | "VRAD" | "WAVE" | "AWAV" | "BETA"
    )
}

/// Extract the 3-character projection code from a CTYPE string.
///
/// FITS-WCS convention: CTYPE is `"XXXX-YYY"` where XXXX is a 4-char type
/// and YYY is a 3-char projection code separated by a dash. For example,
/// `"RA---TAN"` has projection code `"TAN"`.
fn extract_projection_code(ctype: &str) -> Option<String> {
    let ct = ctype.trim();
    // Must be at least 8 chars for 4-char type + '-' + 3-char projection.
    if ct.len() < 8 {
        return None;
    }
    // Check for the dash separator at position 4 (or find the last dash
    // followed by exactly 3 characters).
    let bytes = ct.as_bytes();
    // Standard: char 4 (0-based) should be '-', chars 5-7 are the code.
    if bytes[4] == b'-' {
        let code = &ct[5..8];
        // Skip if the code is all dashes (like "RA---" → looking at "---" → not a projection).
        if code.chars().all(|c| c == '-') {
            // Look deeper: e.g. "RA---TAN" has dashes at 4,5,6 and code at 5..8 = "--T"
            // Actually the standard is: first 4 chars are type, char 4 is '-',
            // then we need to find non-dash chars. Let's use the last-dash approach.
        } else {
            // Remove leading dashes from the code.
            let trimmed = code.trim_start_matches('-');
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    // Alternative: find the last group of 3 non-dash alphabetic characters
    // after position 4. For "RA---TAN", this is "TAN" at positions 5..8.
    // Walk backwards to find a 3-char suffix that is all alphabetic.
    if ct.len() >= 8 {
        let suffix = &ct[ct.len() - 3..];
        if suffix.chars().all(|c| c.is_ascii_alphabetic()) {
            // Verify there's a dash before it.
            if ct.as_bytes()[ct.len() - 4] == b'-' {
                return Some(suffix.to_uppercase());
            }
        }
    }

    None
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
    fn has_cd_matrix_detection() {
        let mut store = FitsStore::new(2);
        assert!(!store.has_cd_matrix());
        store.cd[0][0] = Some(1.0);
        assert!(store.has_cd_matrix());
    }

    #[test]
    fn has_pc_matrix_detection() {
        let mut store = FitsStore::new(2);
        assert!(!store.has_pc_matrix());
        store.pc[0][0] = Some(1.0);
        assert!(store.has_pc_matrix());
    }

    #[test]
    fn has_sip_detection() {
        let mut store = FitsStore::new(2);
        assert!(!store.has_sip());
        store.sip_a_order = Some(3);
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

    #[test]
    fn projection_code_extraction() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        assert_eq!(store.projection_code(0), Some("TAN".to_string()));
        assert_eq!(store.projection_code(1), Some("TAN".to_string()));
    }

    #[test]
    fn projection_code_car() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("GLON-CAR".to_string());
        assert_eq!(store.projection_code(0), Some("CAR".to_string()));
    }

    #[test]
    fn projection_code_none() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("FREQ".to_string());
        assert_eq!(store.projection_code(0), None);
    }

    #[test]
    fn projection_code_out_of_bounds() {
        let store = FitsStore::new(2);
        assert_eq!(store.projection_code(5), None);
    }

    #[test]
    fn extract_projection_code_standard() {
        assert_eq!(extract_projection_code("RA---TAN"), Some("TAN".to_string()));
        assert_eq!(extract_projection_code("DEC--TAN"), Some("TAN".to_string()));
        assert_eq!(extract_projection_code("GLON-CAR"), Some("CAR".to_string()));
        assert_eq!(extract_projection_code("GLAT-CAR"), Some("CAR".to_string()));
        assert_eq!(extract_projection_code("RA---SIN"), Some("SIN".to_string()));
    }

    #[test]
    fn extract_projection_code_short() {
        assert_eq!(extract_projection_code("FREQ"), None);
        assert_eq!(extract_projection_code("RA"), None);
    }
}
