//! FitsChan WCS decoder integration tests — Phase 2.
//!
//! Verifies that FitsChan with FitsWcs encoding can read FITS headers and
//! produce correct FrameSets with working coordinate transformations.

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::coords::PointSet;
use ast_core::fitschan::{Encoding, FitsChan};

// ===========================================================================
// Helpers
// ===========================================================================

/// Normalize an angle in degrees to the range [0, 360).
fn normalize_ra(deg: f64) -> f64 {
    let mut r = deg % 360.0;
    if r < 0.0 {
        r += 360.0;
    }
    r
}

// ===========================================================================
// Test data
// ===========================================================================

/// Minimal 2D TAN header for testing.
const MINIMAL_TAN_HEADER: &str = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =              128.000 / Reference pixel
CRPIX2  =              128.000 / Reference pixel
CRVAL1  =          202.4820000 / Reference RA
CRVAL2  =           47.1750000 / Reference Dec
CDELT1  =       -0.00027778000 / Degrees per pixel
CDELT2  =        0.00027778000 / Degrees per pixel
CTYPE1  = 'RA---TAN'           / RA with TAN projection
CTYPE2  = 'DEC--TAN'           / Dec with TAN projection
RADESYS = 'ICRS    '           / Reference frame
EQUINOX =               2000.0 / Equinox
";

/// Load the SIP header fixture text.
fn load_sip_header() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/fits/headers/sip.head"
    ))
    .expect("sip.head fixture should be readable")
}

// ===========================================================================
// 1. Minimal 2D TAN header produces a FrameSet
// ===========================================================================

#[test]
fn read_minimal_tan_header() {
    let mut fc = FitsChan::from_header(MINIMAL_TAN_HEADER).unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    let result = fc.read().unwrap();
    assert!(result.is_some(), "FitsChan should produce a FrameSet");
    let obj = result.unwrap();
    if let AnyObject::FrameSet(ref fs) = obj {
        assert_eq!(
            fs.nframe(),
            2,
            "FrameSet should have 2 frames (pixel + sky)"
        );
        assert_eq!(fs.current().naxes(), 2, "Current frame should have 2 axes");
    } else {
        panic!("Expected AnyObject::FrameSet, got {:?}", obj);
    }
}

// ===========================================================================
// 2. SIP header fixture produces correct FrameSet
// ===========================================================================

#[test]
fn read_sip_header_frameset() {
    let text = load_sip_header();
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    let result = fc.read().unwrap();
    assert!(
        result.is_some(),
        "FitsChan should produce a FrameSet from sip.head"
    );
    let obj = result.unwrap();

    // Verify it is a FrameSet.
    if let AnyObject::FrameSet(ref fs) = obj {
        // Should have 2 frames: pixel + sky.
        assert_eq!(fs.nframe(), 2, "FrameSet should have 2 frames");

        // Current frame should be 2-D sky.
        assert_eq!(fs.current().naxes(), 2, "Current frame should have 2 axes");

        // Current frame should indicate ICRS system.
        let sys = fs.current().system_string();
        assert!(sys.is_some(), "Sky frame should have a system_string");
        let sys_str = sys.unwrap();
        assert!(
            sys_str.contains("ICRS") || sys_str.contains("FK5"),
            "Expected ICRS or FK5 system, got '{sys_str}'"
        );
    } else {
        panic!("Expected AnyObject::FrameSet, got {:?}", obj);
    }
}

// ===========================================================================
// 3. Transform at reference pixel — CRPIX maps to CRVAL
// ===========================================================================

#[test]
fn transform_at_reference_pixel() {
    let mut fc = FitsChan::from_header(MINIMAL_TAN_HEADER).unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    let obj = fc.read().unwrap().unwrap();
    let mapping = obj.as_mapping().expect("FrameSet should be a Mapping");

    // Transform reference pixel (128, 128) → should give approximately CRVAL.
    // Note: AST pixel coordinates are 1-based GRID coords, so CRPIX=128 means
    // the reference pixel is at grid position 128.
    let pts = PointSet::from_rows([[128.0, 128.0]]);
    let out = mapping.transform(&pts, true).unwrap();

    // Output is in radians (SkyFrame convention).
    let ra_deg = normalize_ra(out.point(0)[0].to_degrees());
    let dec_deg = out.point(0)[1].to_degrees();

    // CRVAL = (202.482, 47.175) degrees.
    assert!(
        (ra_deg - 202.482).abs() < 0.01,
        "RA at CRPIX should be near CRVAL1=202.482, got {ra_deg}"
    );
    assert!(
        (dec_deg - 47.175).abs() < 0.01,
        "Dec at CRPIX should be near CRVAL2=47.175, got {dec_deg}"
    );
}

// ===========================================================================
// 4. Round-trip: pixel → sky → pixel
// ===========================================================================

#[test]
fn inverse_transform_round_trip() {
    let mut fc = FitsChan::from_header(MINIMAL_TAN_HEADER).unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    let obj = fc.read().unwrap().unwrap();
    let mapping = obj.as_mapping().expect("FrameSet should be a Mapping");

    // Start at an offset from the reference pixel.
    let pix_in = PointSet::from_rows([[100.0, 150.0]]);

    // Forward: pixel → sky.
    let sky = mapping.transform(&pix_in, true).unwrap();

    // Inverse: sky → pixel.
    let pix_out = mapping.transform(&sky, false).unwrap();

    // Should recover the original pixel to within floating-point tolerance.
    let x_err = (pix_out.point(0)[0] - 100.0).abs();
    let y_err = (pix_out.point(0)[1] - 150.0).abs();
    assert!(
        x_err < 1e-6,
        "Round-trip x error too large: {x_err} (got {})",
        pix_out.point(0)[0]
    );
    assert!(
        y_err < 1e-6,
        "Round-trip y error too large: {y_err} (got {})",
        pix_out.point(0)[1]
    );
}

// ===========================================================================
// 5. Non-celestial header: FREQ axis → SpecFrame
// ===========================================================================

/// A minimal 1D spectral header.
const SPECTRAL_HEADER: &str = "\
NAXIS   =                    1 / Number of axes
CRPIX1  =                  1.0 / Reference pixel
CRVAL1  =          1.42040575E9 / Reference frequency (Hz)
CDELT1  =           1000000.00 / Frequency step (Hz)
CTYPE1  = 'FREQ    '           / Frequency axis
CUNIT1  = 'Hz      '           / Units
SPECSYS = 'BARYCENT'           / Reference frame
RESTFRQ =          1.42040575E9 / Rest frequency (Hz)
";

#[test]
fn read_spectral_header() {
    let mut fc = FitsChan::from_header(SPECTRAL_HEADER).unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    let result = fc.read().unwrap();
    assert!(
        result.is_some(),
        "FitsChan should produce a FrameSet for spectral header"
    );
    let obj = result.unwrap();

    if let AnyObject::FrameSet(ref fs) = obj {
        assert_eq!(fs.nframe(), 2, "FrameSet should have 2 frames");
        // Current frame should be 1-D.
        assert_eq!(fs.current().naxes(), 1, "Spectral frame should have 1 axis");
    } else {
        panic!("Expected AnyObject::FrameSet, got {:?}", obj);
    }
}

// ===========================================================================
// 6. SIP header transform at reference pixel
// ===========================================================================

#[test]
fn sip_header_transform_at_crpix() {
    let text = load_sip_header();
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    let obj = fc.read().unwrap().unwrap();
    let mapping = obj.as_mapping().expect("FrameSet should be a Mapping");

    // SIP header: CRPIX = (128, 128), CRVAL = (202.482322805429, 47.1751189300101)
    let pts = PointSet::from_rows([[128.0, 128.0]]);
    let out = mapping.transform(&pts, true).unwrap();

    let ra_deg = normalize_ra(out.point(0)[0].to_degrees());
    let dec_deg = out.point(0)[1].to_degrees();

    // At the reference pixel, the SIP distortion is zero (by definition),
    // so the result should match CRVAL closely.
    assert!(
        (ra_deg - 202.4823).abs() < 0.01,
        "SIP RA at CRPIX should be near 202.4823, got {ra_deg}"
    );
    assert!(
        (dec_deg - 47.1751).abs() < 0.01,
        "SIP Dec at CRPIX should be near 47.1751, got {dec_deg}"
    );
}

// ===========================================================================
// Tier-1 A2/A3/B1 regression tests — three reader gaps that block
// wcsconv fixtures with unusual headers (no NAXIS, non-adjacent
// celestial axes, NAXIS-less CLASS encoding).
// ===========================================================================

/// Load a fixture from the wcsconv/inputs directory.
fn load_wcsconv_fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/wcsconv/inputs/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {name}: {e}"))
}

/// Read a wcsconv fixture as an AnyObject.
fn read_wcsconv_fixture(name: &str) -> AnyObject {
    let text = load_wcsconv_fixture(name);
    let mut fc = FitsChan::from_header(&text).unwrap_or_else(|e| panic!("from_header {name}: {e}"));
    fc.guess_encoding();
    fc.read()
        .unwrap_or_else(|e| panic!("read {name}: {e}"))
        .unwrap_or_else(|| panic!("read {name}: returned None"))
}

#[test]
fn car3_naxis_inferred_from_ctype_indices() {
    // car3.head has CTYPE1/CTYPE2/CRVAL1/CRVAL2/CDELT1/CDELT2/CRPIX1/CRPIX2
    // and no NAXIS, no NAXISn, no WCSAXES. Reader must infer naxis = 2.
    let _obj = read_wcsconv_fixture("car3.head");
}

#[test]
fn axes_swap_non_adjacent_celestial_axes() {
    // axes-swap.head has CTYPE1=DEC--TAN, CTYPE2=WAVE, CTYPE3=RA---TAN,
    // CTYPE4=STOKES — celestial pair at indices 0 and 2 (separated by
    // spectral). Reader must produce a working pixel-to-world mapping.
    let _obj = read_wcsconv_fixture("axes-swap.head");
}

#[test]
fn jan_class_no_naxis_keyword() {
    // jan.fits-class has CTYPE1=RA, CTYPE2=DEC, CTYPE3=FREQ but no NAXIS.
    // CLASS reader must infer naxis = 3 from CTYPE indices.
    let _obj = read_wcsconv_fixture("jan.fits-class");
}
