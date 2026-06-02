//! Spectral axis decoder for FITS-WCS.
//!
//! If a spectral axis is present in the [`FitsStore`], this module builds the
//! IWC-to-spectral mapping and creates a [`SpecFrame`]. The pipeline follows
//! the C library's `WcsSpectral` function (fitschan.c ~line 39364).
//!
//! For FITS-WCS spectral axes, the IWC value represents the offset from
//! CRVAL in the spectral quantity's native units. The mapping from IWC to
//! spectral coordinate is simply:
//!
//! ```text
//! IWC_spectral → shift by CRVAL → spectral coordinate value
//! ```

use super::fits_store::FitsStore;
use crate::frames::{SpecFrame, SpecSystem, StdOfRest};
use crate::mappings::{DynMapping, ShiftMap};

/// Result of building the spectral axis from a FitsStore.
pub(crate) struct SpectralResult {
    /// Mapping from IWC (spectral subset, 1D) to spectral coordinate.
    pub mapping: DynMapping,
    /// The SpecFrame describing the output spectral coordinate.
    pub frame: SpecFrame,
    /// 0-based axis index in the full IWC space.
    pub axis: usize,
}

/// If a spectral axis is present in the FitsStore, build the IWC-to-spectral
/// mapping and create a SpecFrame.
///
/// Returns `None` if no spectral axis is found.
pub(crate) fn build_spectral(store: &FitsStore) -> Option<SpectralResult> {
    // Step 1: Find the spectral axis.
    let axis = store.spectral_axis()?;

    // Step 2: Determine the spectral system from CTYPE.
    let system = determine_spec_system(store, axis)?;

    // Step 3: Build the mapping.
    // The IWC spectral value needs CRVAL added to give the absolute coordinate.
    let crval = store.crval[axis].unwrap_or(0.0);
    let shift = ShiftMap::new(vec![crval]);
    let mapping: DynMapping = Box::new(shift);

    // Step 4: Create the SpecFrame.
    let mut frame = SpecFrame::new(system);

    // Step 5: Set standard of rest from SPECSYS.
    if let Some(ref specsys) = store.specsys {
        if let Some(sor) = StdOfRest::parse(specsys) {
            frame.set_std_of_rest(sor);
        }
    }

    // Step 6: Set rest frequency/wavelength.
    if let Some(restfrq) = store.restfrq {
        frame.set_rest_freq(restfrq);
    } else if let Some(restwav) = store.restwav {
        // Convert rest wavelength (m) to frequency (Hz): f = c / lambda
        const SPEED_OF_LIGHT: f64 = 299_792_458.0; // m/s
        let restfrq = SPEED_OF_LIGHT / restwav;
        frame.set_rest_freq(restfrq);
    }

    // Step 7: Set epoch if present.
    if let Some(mjd) = store.mjdobs {
        frame.set_epoch(mjd);
    }

    // OBSGEO-X/Y/Z → ObsLon/ObsLat/ObsAlt. Mirrors C
    // fitschan.c:39534-39550.
    if let (Some(x), Some(y), Some(z)) = (store.obsgeox, store.obsgeoy, store.obsgeoz)
        && let Some((geolon, geolat, h)) = crate::pal::gc2gd([x, y, z])
    {
        frame.set_obs_lon(geolon);
        frame.set_obs_lat(geolat);
        frame.set_obs_alt(h);
    }

    // Step 8: Set the user-facing unit from the input CUNIT. The
    // SpecFrame's `unit` attribute records the preferred presentation
    // unit (e.g. "GHz", "km/s"); the SpecMap that connects two
    // SpecFrames handles the SI conversion when systems differ. Mirrors
    // the C SpecFrame attribute machinery (specframe.c SetUnit) so the
    // value flowing through the FrameSet is in the input CUNIT and
    // CUNIT survives a round trip.
    if let Some(ref cunit) = store.cunit[axis] {
        if !cunit.is_empty() {
            frame.set_unit(cunit);
        }
    }

    Some(SpectralResult {
        mapping,
        frame,
        axis,
    })
}

/// Determine the spectral coordinate system from the CTYPE value.
fn determine_spec_system(store: &FitsStore, axis: usize) -> Option<SpecSystem> {
    let ctype = store.ctype[axis].as_ref()?;
    // Extract the first 4 characters (the spectral type code).
    let prefix = if ctype.len() >= 4 {
        &ctype[..4]
    } else {
        ctype.as_str()
    };
    SpecSystem::parse(prefix)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fitschan::fits_store::FitsStore;

    #[test]
    fn detects_freq_axis() {
        let mut store = FitsStore::new(3);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());
        store.ctype[2] = Some("FREQ".to_string());
        store.crval[2] = Some(1.4e9); // 1.4 GHz

        let result = build_spectral(&store);
        assert!(result.is_some());
        let spec = result.unwrap();
        assert_eq!(spec.axis, 2);
        assert_eq!(spec.frame.system(), SpecSystem::Freq);
        assert_eq!(spec.mapping.nin(), 1);
        assert_eq!(spec.mapping.nout(), 1);
    }

    #[test]
    fn detects_wave_axis() {
        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("WAVE".to_string());
        store.crval[0] = Some(5.0e-7); // 500nm in metres

        let result = build_spectral(&store).unwrap();
        assert_eq!(result.frame.system(), SpecSystem::Wavelen);
    }

    #[test]
    fn detects_vrad_axis() {
        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("VRAD".to_string());
        store.crval[0] = Some(0.0);

        let result = build_spectral(&store).unwrap();
        assert_eq!(result.frame.system(), SpecSystem::Vradio);
    }

    #[test]
    fn sets_std_of_rest() {
        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("FREQ".to_string());
        store.crval[0] = Some(1.0e9);
        store.specsys = Some("LSRK".to_string());

        let result = build_spectral(&store).unwrap();
        assert_eq!(result.frame.std_of_rest(), StdOfRest::Lsrk);
    }

    #[test]
    fn sets_rest_freq() {
        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("VRAD".to_string());
        store.crval[0] = Some(0.0);
        store.restfrq = Some(1.420405751786e9);

        let result = build_spectral(&store).unwrap();
        assert!((result.frame.rest_freq().unwrap() - 1.420405751786e9).abs() < 1.0);
    }

    #[test]
    fn no_spectral_returns_none() {
        let mut store = FitsStore::new(2);
        store.ctype[0] = Some("RA---TAN".to_string());
        store.ctype[1] = Some("DEC--TAN".to_string());

        let result = build_spectral(&store);
        assert!(result.is_none());
    }

    #[test]
    fn crval_shift_mapping() {
        use crate::coords::PointSet;

        let mut store = FitsStore::new(1);
        store.ctype[0] = Some("FREQ".to_string());
        store.crval[0] = Some(1.4e9);

        let result = build_spectral(&store).unwrap();
        // IWC = 0 should map to CRVAL.
        let pts = PointSet::from_columns(&[&[0.0]]).unwrap();
        let out = result.mapping.transform(&pts, true).unwrap();
        assert!((out.point(0)[0] - 1.4e9).abs() < 1.0);

        // IWC = 1e6 should map to CRVAL + 1e6.
        let pts2 = PointSet::from_columns(&[&[1.0e6]]).unwrap();
        let out2 = result.mapping.transform(&pts2, true).unwrap();
        assert!((out2.point(0)[0] - (1.4e9 + 1.0e6)).abs() < 1.0);
    }
}
