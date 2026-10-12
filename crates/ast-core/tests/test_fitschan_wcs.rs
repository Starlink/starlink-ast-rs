//! FitsChan WCS decoder integration tests — Phase 2.
//!
//! Verifies that FitsChan with FitsWcs encoding can read FITS headers and
//! produce correct FrameSets with working coordinate transformations.

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::coords::PointSet;
use ast_core::fitschan::{Encoding, FitsChan};
use ast_core::frames::{Frame, FrameRef, SkyFrame, SkyRefIs, SkySystem};

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
        "/tests/fixtures/wcsconv/inputs/sip.head"
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
        let sys = fs.current().system_string().unwrap();
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

// ===========================================================================
// SipReplace: reading a SIP header refits the inverse via PolyTran
// ===========================================================================

#[test]
fn sip_read_default_refits_inverse_via_polytran() {
    // With SipReplace at its default (true), reading a SIP header
    // replaces the inverse by an astPolyTran fit over three times the
    // image area (fitschan.c:29357-29394). The fit on this header
    // achieves ~6.5e-3 pixel accuracy (verified bit-compatible with
    // the reference C library's cminpack fit), so pixel→sky→pixel
    // round trips inside the image must land within 0.02 pixels.
    let text = load_sip_header();
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    let obj = fc.read().unwrap().expect("frameset");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("expected FrameSet");
    };

    let xs = [20.0_f64, 128.0, 230.0];
    let ys = [40.0_f64, 128.0, 200.0];
    let pix = PointSet::from_columns(&[&xs, &ys]).unwrap();
    let sky = ast_core::mappings::Mapping::transform(&fs, &pix, true).unwrap();
    let back = ast_core::mappings::Mapping::transform(&fs, &sky, false).unwrap();
    for i in 0..3 {
        assert!(
            (back.point(i)[0] - xs[i]).abs() < 0.02 && (back.point(i)[1] - ys[i]).abs() < 0.02,
            "round trip {i}: got ({}, {}), want ({}, {})",
            back.point(i)[0],
            back.point(i)[1],
            xs[i],
            ys[i]
        );
    }
}

#[test]
fn sip_read_sipreplace_off_keeps_supplied_inverse() {
    // With SipReplace disabled the supplied AP/BP inverse (or the
    // IterInverse default when none is supplied) is used as-is.
    let text = load_sip_header();
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    fc.set_sip_replace(false);
    let obj = fc.read().unwrap().expect("frameset");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("expected FrameSet");
    };

    let pix = PointSet::from_columns(&[&[128.0_f64], &[128.0_f64]]).unwrap();
    let sky = ast_core::mappings::Mapping::transform(&fs, &pix, true).unwrap();
    let back = ast_core::mappings::Mapping::transform(&fs, &sky, false).unwrap();
    // sip.head carries explicit AP/BP approximate inverse coefficients;
    // they are good to better than a pixel near the reference point.
    assert!(
        (back.point(0)[0] - 128.0).abs() < 1.0 && (back.point(0)[1] - 128.0).abs() < 1.0,
        "round trip: got ({}, {})",
        back.point(0)[0],
        back.point(0)[1]
    );
}

// ===========================================================================
// DUT1 unit conversion: FITS keyword is days, SkyFrame attribute is seconds
// ===========================================================================

#[test]
fn serpens_dut1_is_seconds() {
    // serpens.head contains `DUT1 = -1.145603703E-6` in days.
    // C fitschan.c:37899 multiplies by SPD (86400) before calling
    // astSetDut1, so the SkyFrame stores seconds. Verify Rust matches.
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/serpens.head",
    ))
    .expect("serpens.head should be readable");
    let obj = FitsChan::from_header(&text)
        .unwrap()
        .read()
        .unwrap()
        .expect("serpens.head should produce a FrameSet");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("expected FrameSet");
    };
    // DUT1 card value in days; expected SkyFrame value in seconds.
    let dut1_days = -1.145603703e-6_f64;
    let expected = dut1_days * 86_400.0; // -0.09898015993920001
    assert!(fs.test_dut1(), "Dut1 must be set from the DUT1 FITS card");
    let got = fs.dut1();
    assert!(
        (got - expected).abs() < 1e-9,
        "Dut1 should be {expected} s (days × 86400), got {got} s"
    );
}

/// serpens.head has swapped celestial axes: CTYPE1='DEC--TAN' (latitude on
/// FITS axis 1), CTYPE2='RA---TAN' (longitude on FITS axis 2). The pixel→sky
/// forward transform must route the axes into the native→celestial rotation
/// as C `WcsNative` does (fitschan.c:38084), so the projection math always
/// sees (lon, lat) in the correct order while the FITS axis order is
/// preserved on the outputs. Golden values are the repo libast oracle's
/// result for pixel (150, 120).
#[test]
fn serpens_swapped_axis_transform() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/serpens.head",
    ))
    .expect("serpens.head should be readable");
    let obj = FitsChan::from_header(&text)
        .unwrap()
        .read()
        .unwrap()
        .expect("serpens.head should produce a FrameSet");
    let mapping = obj.as_mapping().expect("FrameSet should be a Mapping");

    // Forward pixel → sky. Axis 1 is DEC, axis 2 is RA (radians).
    let pts = PointSet::from_rows([[150.0, 120.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    let got_axis1 = out.point(0)[0];
    let got_axis2 = out.point(0)[1];

    // Repo libast oracle: (0.018049750, -1.440691695) rad on (axis1, axis2).
    let want_axis1 = 0.018_049_750;
    let want_axis2 = -1.440_691_695;
    assert!(
        (got_axis1 - want_axis1).abs() < 1e-6,
        "serpens fwd axis1 (DEC): expected {want_axis1}, got {got_axis1}"
    );
    assert!(
        (got_axis2 - want_axis2).abs() < 1e-6,
        "serpens fwd axis2 (RA): expected {want_axis2}, got {got_axis2}"
    );
}

// ===========================================================================
// SREF alt-plane version letter (C2)
// ===========================================================================

/// Alt-plane SREF* cards carry a version letter suffix (e.g. SREF1A, SREF2A,
/// SREFISA). The extractor must use the version-aware key_for helper rather
/// than hard-coded primary names, so the alt SkyFrame sees its own SREF values
/// rather than the primary plane's.
#[test]
fn skyref_alt_reads_versioned_sref() {
    // skyref-alt.head has primary SREF1=120, SREF2=30, SREFIS='ORIGIN'
    // and alternate-A SREF1A=236.592, SREF2A=50.669, SREFISA='ORIGIN'.
    // The SkyFrame convention stores coordinates in radians.
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/skyref-alt.head",
    ))
    .expect("skyref-alt.head should be readable");
    let obj = FitsChan::from_header(&text)
        .unwrap()
        .read()
        .unwrap()
        .expect("skyref-alt.head should produce a FrameSet");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("expected FrameSet");
    };

    // Frame 3 is the alt-A sky frame (Galactic, Ident="A").
    let alt_frame = fs.frame(FrameRef::Index(3)).expect("frame 3 should exist");
    let sky = alt_frame
        .as_any()
        .downcast_ref::<SkyFrame>()
        .expect("frame 3 should be a SkyFrame");

    // SREF1A=236.592 deg, SREF2A=50.669 deg.  SkyRef stores the raw
    // degree values as read from the FITS card (C does the same: the
    // store->skyref array holds FITS degrees and WcsSkyFrame passes
    // them directly to astSetSkyRef without unit conversion).
    let expected_lon = 236.592_f64;
    let expected_lat = 50.669_f64;
    let got = sky.sky_ref_lon_lat();
    assert!(
        (got[0] - expected_lon).abs() < 1e-9,
        "alt SkyFrame sky_ref lon should be {expected_lon} (SREF1A), got {}",
        got[0]
    );
    assert!(
        (got[1] - expected_lat).abs() < 1e-9,
        "alt SkyFrame sky_ref lat should be {expected_lat} (SREF2A), got {}",
        got[1]
    );
    // SREFISA='ORIGIN'.
    assert_eq!(
        sky.sky_ref_is(),
        SkyRefIs::Origin,
        "alt SkyFrame sky_ref_is should be Origin (SREFISA='ORIGIN')"
    );
}

// ===========================================================================
// A1 LONPOLE default: WcsNatPole must resolve it, not a hard 180° constant
// ===========================================================================

/// GLS (= SFL) is a θ0=0 projection: the correct LONPOLE default is 0°, not
/// 180°. Forcing 180° before calling WcsNatPole flips the pole and mirrors the
/// transform. C passes AST__BAD and lets WcsNatPole choose per projection.
///
/// Golden values sourced from `tests/fixtures/oracle/headers.oracle`, section
/// `[gls.head  nin=2 nout=2 dir=forward]`, rows 1 and 4.
#[test]
#[allow(clippy::excessive_precision)] // oracle values; extra digits are from C reference output
fn gls_lonpole_default_matches_c() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/gls.head",
    ))
    .expect("gls.head should be readable");
    let obj = FitsChan::from_header(&text)
        .unwrap()
        .read()
        .unwrap()
        .expect("gls.head should produce a FrameSet");
    let mapping = obj.as_mapping().expect("FrameSet is a Mapping");

    // Point 1: pixel (1, 1) → oracle forward (radians).
    {
        let pts = PointSet::from_rows([[1.0_f64, 1.0_f64]]);
        let out = mapping.transform(&pts, true).unwrap();
        let lon = out.point(0)[0];
        let lat = out.point(0)[1];
        assert!(
            (lon - 0.20594209297728069_f64).abs() < 1e-9,
            "gls fwd (1,1) lon: expected 0.20594209297728069, got {lon}"
        );
        assert!(
            (lat - 0.071602883102475187_f64).abs() < 1e-9,
            "gls fwd (1,1) lat: expected 0.071602883102475187, got {lat}"
        );
    }

    // Point 4: pixel (180.5, 90.5) → oracle forward (radians).
    {
        let pts = PointSet::from_rows([[180.5_f64, 90.5_f64]]);
        let out = mapping.transform(&pts, true).unwrap();
        let lon = out.point(0)[0];
        let lat = out.point(0)[1];
        assert!(
            (lon - 0.17453292519943275_f64).abs() < 1e-9,
            "gls fwd (180.5,90.5) lon: expected 0.17453292519943275, got {lon}"
        );
        assert!(
            (lat - 0.087266462599716377_f64).abs() < 1e-9,
            "gls fwd (180.5,90.5) lat: expected 0.087266462599716377, got {lat}"
        );
    }
}

// ===========================================================================
// A2 generic celestial CTYPE recognition: xLON/xLAT (1-char) and xyLN/xyLT (2-char)
// ===========================================================================

/// Verify that an unknown 1-char celestial system (XLON-TAN / XLAT-TAN)
/// produces a SkyFrame with System=Unknown and a transform matching the
/// C oracle.
///
/// Golden values from `tests/fixtures/oracle/headers.oracle`, section
/// `[unknown-cel.head  nin=2 nout=2 dir=forward]`, rows 1 and 4.
#[test]
#[allow(clippy::excessive_precision)] // oracle literals; extra digits are exact C reference output
fn unknown_cel_builds_unknown_skyframe() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/unknown-cel.head"
    ))
    .expect("unknown-cel.head fixture should be readable");
    let obj = FitsChan::from_header(&text)
        .unwrap()
        .read()
        .unwrap()
        .expect("unknown-cel.head should produce an object");
    let AnyObject::FrameSet(ref fs) = obj else {
        panic!("expected FrameSet, got {:?}", obj);
    };

    // The current (world) frame must be a SkyFrame with System=Unknown.
    let sky = fs
        .current()
        .as_any()
        .downcast_ref::<SkyFrame>()
        .expect("current frame should be a SkyFrame for unknown-cel.head");
    assert_eq!(
        sky.system(),
        SkySystem::Unknown,
        "unknown-cel SkyFrame system should be Unknown"
    );

    // Forward transform: pixel (1,1) → C oracle row 1, pixel (50.5,50.5) → row 4.
    let mapping = obj.as_mapping().expect("FrameSet is a Mapping");

    let pts = PointSet::from_rows([[1.0_f64, 1.0_f64]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert!(
        (out.point(0)[0] - (-3.1412533210012499_f64)).abs() < 1e-9,
        "fwd (1,1) lon: expected -3.1412533210012499, got {}",
        out.point(0)[0]
    );
    assert!(
        (out.point(0)[1] - 0.78515813264463685_f64).abs() < 1e-9,
        "fwd (1,1) lat: expected 0.78515813264463685, got {}",
        out.point(0)[1]
    );

    let pts = PointSet::from_rows([[50.5_f64, 50.5_f64]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert!(
        (out.point(0)[0] - std::f64::consts::PI).abs() < 1e-9,
        "fwd (50.5,50.5) lon: expected PI (3.1415926535897931), got {}",
        out.point(0)[0]
    );
    assert!(
        (out.point(0)[1] - 0.78539816339744839_f64).abs() < 1e-9,
        "fwd (50.5,50.5) lat: expected 0.78539816339744839, got {}",
        out.point(0)[1]
    );
}

/// Verify that an unknown 2-char celestial system (XYLN-TAN / XYLT-TAN)
/// produces a SkyFrame with System=Unknown and a transform matching the
/// C oracle.
///
/// Golden values from `tests/fixtures/oracle/headers.oracle`, section
/// `[unknown-cel2.head  nin=2 nout=2 dir=forward]`, rows 1 and 4.
#[test]
#[allow(clippy::excessive_precision)] // oracle literals; extra digits are exact C reference output
fn unknown_cel2_builds_unknown_skyframe() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/unknown-cel2.head"
    ))
    .expect("unknown-cel2.head fixture should be readable");
    let obj = FitsChan::from_header(&text)
        .unwrap()
        .read()
        .unwrap()
        .expect("unknown-cel2.head should produce an object");
    let AnyObject::FrameSet(ref fs) = obj else {
        panic!("expected FrameSet, got {:?}", obj);
    };

    // The current (world) frame must be a SkyFrame with System=Unknown.
    let sky = fs
        .current()
        .as_any()
        .downcast_ref::<SkyFrame>()
        .expect("current frame should be a SkyFrame for unknown-cel2.head");
    assert_eq!(
        sky.system(),
        SkySystem::Unknown,
        "unknown-cel2 SkyFrame system should be Unknown"
    );

    // Forward transform: pixel (1,1) → C oracle row 1, pixel (50.5,50.5) → row 4.
    let mapping = obj.as_mapping().expect("FrameSet is a Mapping");

    let pts = PointSet::from_rows([[1.0_f64, 1.0_f64]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert!(
        (out.point(0)[0] - (-3.1412533210012499_f64)).abs() < 1e-9,
        "fwd (1,1) lon: expected -3.1412533210012499, got {}",
        out.point(0)[0]
    );
    assert!(
        (out.point(0)[1] - 0.78515813264463685_f64).abs() < 1e-9,
        "fwd (1,1) lat: expected 0.78515813264463685, got {}",
        out.point(0)[1]
    );

    let pts = PointSet::from_rows([[50.5_f64, 50.5_f64]]);
    let out = mapping.transform(&pts, true).unwrap();
    assert!(
        (out.point(0)[0] - std::f64::consts::PI).abs() < 1e-9,
        "fwd (50.5,50.5) lon: expected PI (3.1415926535897931), got {}",
        out.point(0)[0]
    );
    assert!(
        (out.point(0)[1] - 0.78539816339744839_f64).abs() < 1e-9,
        "fwd (50.5,50.5) lat: expected 0.78539816339744839, got {}",
        out.point(0)[1]
    );
}

// ===========================================================================
// A3 FITS-CLASS: bare celestial CTYPE defaults to -GLS (→ -SFL)
// ===========================================================================

/// FITS-CLASS headers carry bare celestial CTYPEs with no projection code
/// (`RA--`, `DEC-`). C `ClassTrans` (fitschan.c:7108-7129) appends `-GLS` to
/// any celestial CTYPE whose projection-suffix bytes are all blank, then
/// SpecTrans translation #11 rewrites `-GLS` to `-SFL`. The decoded projection
/// is therefore sinusoidal (SFL), not linear (CAR).
///
/// Golden values from `tests/fixtures/oracle/headers.oracle:1044-1094`.
#[test]
#[allow(clippy::excessive_precision)] // oracle literals; extra digits are exact C reference output
fn class_noproj_builds_sfl() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/class-noproj.head",
    ))
    .expect("class-noproj.head fixture should be readable");
    let obj = FitsChan::from_header(&text)
        .unwrap()
        .read()
        .unwrap()
        .expect("class-noproj.head should produce a FrameSet");
    let mapping = obj.as_mapping().expect("FrameSet is a Mapping");

    // Forward. Oracle rows (headers.oracle:1045-1068): pix2=pix3=1 are fixed;
    // celestial output is constant SFL (0.71082041703064114, 1.0984753171674055).
    // Spectral axis (freq) varies with pix1.
    let fwd_cases: &[([f64; 3], [f64; 3])] = &[
        (
            [1.0, 1.0, 1.0],
            [110181373831.25, 0.71082041703064114, 1.0984753171674055],
        ),
        (
            [512.5, 1.0, 1.0],
            [110201354300.0, 0.71082041703064114, 1.0984753171674055],
        ),
        (
            [1024.0, 1.0, 1.0],
            [110221334768.75, 0.71082041703064114, 1.0984753171674055],
        ),
        (
            [256.75, 1.0, 1.0],
            [110191364065.625, 0.71082041703064114, 1.0984753171674055],
        ),
    ];
    for (pix, want) in fwd_cases {
        let out = mapping
            .transform(&PointSet::from_rows([*pix]), true)
            .unwrap();
        for (k, &expected) in want.iter().enumerate() {
            let got = out.point(0)[k];
            // Spectral axis (~1e11 Hz) tolerates 1e-3 absolute; celestial uses 1e-9.
            let tol: f64 = if k == 0 { 1e-3 } else { 1e-9 };
            assert!(
                (got - expected).abs() <= tol.max(expected.abs() * 1e-12),
                "class-noproj fwd {pix:?} axis {k}: expected {expected}, got {got}"
            );
        }
    }

    // Inverse. Oracle rows (headers.oracle:1071-1094): world → pixel.
    let inv_cases: &[([f64; 3], [f64; 3])] = &[
        (
            [110201354300.0, 0.71082041703064114, 1.0984753171674055],
            [512.5, 1.0000000000000495, 1.0000000000005198],
        ),
        (
            [110181373831.25, 0.71082041703064114, 1.0984753171674055],
            [1.0000000000000231, 1.0000000000000495, 1.0000000000005198],
        ),
    ];
    for (world, want) in inv_cases {
        let out = mapping
            .transform(&PointSet::from_rows([*world]), false)
            .unwrap();
        for (k, &expected) in want.iter().enumerate() {
            let got = out.point(0)[k];
            assert!(
                (got - expected).abs() < 1e-6,
                "class-noproj inv {world:?} axis {k}: expected {expected}, got {got}"
            );
        }
    }
}

// ===========================================================================
// C3 PolyTan auto-detect: TAN + PVi_m → TPN (distorted TAN)
// ===========================================================================

/// TAN header carrying PVi_m distortion coefficients must auto-promote to TPN.
///
/// The C `GetUsedPolyTan` function defaults `PolyTan=-1` (auto): if any PVi_m
/// are present on the latitude axis the header uses distorted TAN (TPN). Here
/// the header has `PV2_1=1.0, PV2_2=0.0001` (latitude axis 2) so C promotes
/// to TPN. Transforms computed with plain TAN (ignoring PVs) will not match
/// the oracle, making this a discriminating test.
///
/// Golden values from `tests/fixtures/oracle/headers.oracle`, section
/// `[tan-polytan.head  nin=2 nout=2 dir=forward]`, rows 1, 2, 5, 6.
#[test]
#[allow(clippy::excessive_precision)] // oracle literals; extra digits are exact C reference output
fn tan_polytan_promotes_to_tpn() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/tan-polytan.head",
    ))
    .expect("tan-polytan.head fixture should be readable");
    let obj = FitsChan::from_header(&text)
        .unwrap()
        .read()
        .unwrap()
        .expect("tan-polytan.head should produce a FrameSet");
    let mapping = obj.as_mapping().expect("FrameSet is a Mapping");

    // Row 1: pixel (1, 1) → oracle forward.
    {
        let pts = PointSet::from_rows([[1.0_f64, 1.0_f64]]);
        let out = mapping.transform(&pts, true).unwrap();
        let lon = out.point(0)[0];
        let lat = out.point(0)[1];
        assert!(
            (lon - (-3.1379125794819318_f64)).abs() < 1e-9,
            "tan-polytan fwd (1,1) lon: expected -3.1379125794819318, got {lon}"
        );
        assert!(
            (lat - 0.78278577731432675_f64).abs() < 1e-9,
            "tan-polytan fwd (1,1) lat: expected 0.78278577731432675, got {lat}"
        );
    }

    // Row 2: pixel (300, 300) → oracle forward.
    {
        let pts = PointSet::from_rows([[300.0_f64, 300.0_f64]]);
        let out = mapping.transform(&pts, true).unwrap();
        let lon = out.point(0)[0];
        let lat = out.point(0)[1];
        assert!(
            (lon - 3.1378933267530766_f64).abs() < 1e-9,
            "tan-polytan fwd (300,300) lon: expected 3.1378933267530766, got {lon}"
        );
        assert!(
            (lat - 0.78800374256667105_f64).abs() < 1e-9,
            "tan-polytan fwd (300,300) lat: expected 0.78800374256667105, got {lat}"
        );
    }

    // Row 5: pixel (150.5, 100.666...) → oracle forward.
    {
        let pts = PointSet::from_rows([[150.5_f64, 100.66666666666666_f64]]);
        let out = mapping.transform(&pts, true).unwrap();
        let lon = out.point(0)[0];
        let lat = out.point(0)[1];
        assert!(
            (lon - 3.1415925306946453_f64).abs() < 1e-9,
            "tan-polytan fwd (150.5,100.667) lon: expected 3.1415925306946453, got {lon}"
        );
        assert!(
            (lat - 0.78452840787285039_f64).abs() < 1e-9,
            "tan-polytan fwd (150.5,100.667) lat: expected 0.78452840787285039, got {lat}"
        );
    }

    // Row 6: pixel (75.75, 200.333...) → oracle forward.
    {
        let pts = PointSet::from_rows([[75.75_f64, 200.33333333333331_f64]]);
        let out = mapping.transform(&pts, true).unwrap();
        let lon = out.point(0)[0];
        let lat = out.point(0)[1];
        assert!(
            (lon - (-3.139745895662664_f64)).abs() < 1e-9,
            "tan-polytan fwd (75.75,200.333) lon: expected -3.139745895662664, got {lon}"
        );
        assert!(
            (lat - 0.78626719675739976_f64).abs() < 1e-9,
            "tan-polytan fwd (75.75,200.333) lat: expected 0.78626719675739976, got {lat}"
        );
    }
}

// ===========================================================================
// MSX CAR CRPIX longitude wrap with a CD matrix (car4).
//
// car4 is a coded `GLON-CAR`/`GLAT-CAR` header carrying a CD matrix and no
// CDELT. C `SpecTrans` converts CD→PC/CDELT (CDELT = row-vector length,
// fitschan.c:31342) *before* the MSX-CAR block (fitschan.c:32020), so the MSX
// block sees an effective CDELT and rewrites CRPIX via `palDrange` to bring
// the image's native longitude back into the CAR domain [-180,180]. Without
// that CRPIX wrap the reference longitude is ~351.79°, which the CAR reverse
// projection rejects (out of primary range) → BAD. The transform-oracle
// harness reads this fixture bare (CarLin default 0 → spherical CAR), so the
// golden below is the spherical-CAR decode.
// ===========================================================================

#[test]
#[allow(clippy::excessive_precision)] // oracle literals; extra digits are exact C reference output
fn car4_cd_matrix_wraps_crpix_for_car() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/car4.head",
    ))
    .expect("car4.head fixture should be readable");
    let obj = FitsChan::from_header(&text)
        .unwrap()
        .read()
        .unwrap()
        .expect("car4.head should produce a FrameSet");
    let mapping = obj.as_mapping().expect("FrameSet is a Mapping");

    // Forward: pixel → celestial (radians). Verbatim from headers.oracle
    // car4 forward section.
    let fwd: &[([f64; 2], [f64; 2])] = &[
        ([1.0, 1.0], [-0.14326231996941075, -0.013799562362167875]),
        (
            [951.0, 1851.0],
            [-0.17089670034534191, 0.040014757317276994],
        ),
    ];
    for (pix, want) in fwd {
        let out = mapping
            .transform(&PointSet::from_rows([*pix]), true)
            .unwrap();
        for (k, want_k) in want.iter().enumerate() {
            let got = out.point(0)[k];
            assert!(
                (got - want_k).abs() < 1e-12,
                "car4 fwd {pix:?} axis {k}: expected {want_k}, got {got}"
            );
        }
    }

    // Inverse: celestial → pixel. Verbatim from headers.oracle car4 inverse
    // section.
    let inv: &[([f64; 2], [f64; 2])] = &[
        (
            [-0.14326231996941075, -0.013799562362167875],
            [0.99999999999925548, 1.0000000000000284],
        ),
        (
            [-0.17089670034534191, 0.040014757317276994],
            [950.99999999999966, 1851.0],
        ),
    ];
    for (world, want) in inv {
        let out = mapping
            .transform(&PointSet::from_rows([*world]), false)
            .unwrap();
        for (k, want_k) in want.iter().enumerate() {
            let got = out.point(0)[k];
            assert!(
                (got - want_k).abs() < 1e-6,
                "car4 inv {world:?} axis {k}: expected {want_k}, got {got}"
            );
        }
    }
}

/// crota-3d.head is a 3-axis header (WAVE, RA---TAN, DEC--TAN) with a `CROTA3`
/// rotation coupling the two celestial axes. It is a plain-AIPS-style header,
/// so the read path runs SpecTrans (which turns `CROTA3` into PC keywords on
/// the celestial axes 2/3 and deletes the CROTA cards) before the AIPS
/// keyword reader consumes the store. Two things must hold, both taken from
/// the repo's own libast:
///
///  1. Forward (pixel → world) must reproduce the C golden. If the reader
///     dropped the CROTA3 rotation (the pre-fix bug), the RA is off by
///     ~7.6e-5 rad.
///  2. The inverse (world → pixel) must be **undefined**, matching C's
///     `astGetTranInverse == 0`. The CD-fused pixel→IWC MatrixMap that C
///     builds for this header is near-singular (a degenerate wavelength axis
///     coupled with the rotation), so C marks the inverse undefined and
///     serialises it with `INV_A=0` ("Inverse transformation not defined").
#[test]
#[allow(clippy::excessive_precision)] // oracle literals; extra digits are exact C reference output
fn crota_3d_forward_matches_oracle_inverse_undefined() {
    let obj = read_wcsconv_fixture("crota-3d.head");
    let mapping = obj.as_mapping().expect("FrameSet is a Mapping");
    assert_eq!(mapping.nin(), 3);
    assert_eq!(mapping.nout(), 3);

    // Forward: pixel → world (wavelength/m, RA/rad, Dec/rad). Verbatim from
    // headers.oracle crota-3d forward section.
    let fwd: &[([f64; 3], [f64; 3])] = &[
        (
            [1.0, 1.0, 1.0],
            [
                4.9509999999999993e-07,
                -3.1411770285409264,
                0.78522841319200321,
            ],
        ),
        (
            [100.0, 100.0, 100.0],
            [
                5.0499999999999993e-07,
                3.1411768874480201,
                0.78556782720147966,
            ],
        ),
        (
            [50.5, 34.0, 20.800000000000001],
            [
                5.0004999999999998e-07,
                -3.1414306817462232,
                0.78527976807175792,
            ],
        ),
    ];
    for (pix, want) in fwd {
        let out = mapping
            .transform(&PointSet::from_rows([*pix]), true)
            .unwrap();
        for (k, want_k) in want.iter().enumerate() {
            let got = out.point(0)[k];
            assert!(
                (got - want_k).abs() < 1e-12,
                "crota-3d fwd {pix:?} axis {k}: expected {want_k}, got {got}"
            );
        }
    }

    // The pixel→IWC MatrixMap is near-singular, so C leaves the inverse
    // undefined. The Rust reader must agree.
    assert!(
        mapping.has_forward(),
        "crota-3d forward transform should be defined"
    );
    assert!(
        !mapping.has_inverse(),
        "crota-3d inverse transform must be undefined (C astGetTranInverse == 0)"
    );
}

// ===========================================================================
// CarLin: a -CAR projection read as a linear pixel-to-sky mapping
// ===========================================================================

/// Read `car1.head` with the CarLin attribute set as given, and return the
/// resulting FrameSet.
fn read_car1(car_lin: bool) -> ast_core::frames::FrameSet {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/car1.head",
    ))
    .expect("car1.head should be readable");

    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    fc.set_car_lin(car_lin);
    let obj = fc.read().unwrap().expect("car1.head should read");
    let AnyObject::FrameSet(fs) = obj else {
        panic!("expected FrameSet");
    };
    fs
}

#[test]
fn carlin_car_header_yields_a_single_winmap() {
    // With CarLin set, C `WcsCelestial` (fitschan.c:35919-35964) builds
    // WcsMap(AST__WCSBAD) + a CRVAL-shift WinMap instead of calling
    // WcsNative. The unknown-projection WcsMap collapses to a UnitMap on
    // simplify, so the whole pixel-to-sky chain reduces to one WinMap with
    // scale = CDELT*d2r and shift = CRVAL*d2r - CRPIX*CDELT*d2r.
    use ast_core::mappings::WinMap;

    let fs = read_car1(true);
    let map = fs
        .mapping(FrameRef::Base, FrameRef::Current)
        .expect("base-to-current mapping");
    let win = map
        .as_any()
        .downcast_ref::<WinMap>()
        .expect("CarLin CAR should give a bare WinMap, not a CmpMap");

    // Transform the reference pixel: it must land on CRVAL in radians.
    let d2r = std::f64::consts::PI / 180.0;
    let pix = PointSet::from_columns(&[&[1.0_f64], &[1.0_f64]]).unwrap();
    let sky = ast_core::mappings::Mapping::transform(win, &pix, true).unwrap();
    assert!(
        (sky.point(0)[0] - 185.0 * d2r).abs() < 1e-12,
        "lon at CRPIX: got {}, want {}",
        sky.point(0)[0],
        185.0 * d2r
    );
    assert!(
        (sky.point(0)[1] - -35.0 * d2r).abs() < 1e-12,
        "lat at CRPIX: got {}, want {}",
        sky.point(0)[1],
        -35.0 * d2r
    );

    // One pixel along each axis must move by exactly CDELT*d2r.
    let pix2 = PointSet::from_columns(&[&[2.0_f64], &[2.0_f64]]).unwrap();
    let sky2 = ast_core::mappings::Mapping::transform(win, &pix2, true).unwrap();
    assert!(
        (sky2.point(0)[0] - sky.point(0)[0] - -0.125 * d2r).abs() < 1e-15,
        "lon step: got {}",
        sky2.point(0)[0] - sky.point(0)[0]
    );
    assert!(
        (sky2.point(0)[1] - sky.point(0)[1] - 0.125 * d2r).abs() < 1e-15,
        "lat step: got {}",
        sky2.point(0)[1] - sky.point(0)[1]
    );
}

#[test]
fn carlin_off_car_header_keeps_the_spherical_projection() {
    // The two-sided guard: without CarLin the same header must still build
    // the spherical CAR pipeline (a CmpMap), not the linear one. C takes the
    // `else` branch at fitschan.c:36013 and calls WcsNative.
    let fs = read_car1(false);
    let map = fs
        .mapping(FrameRef::Base, FrameRef::Current)
        .expect("base-to-current mapping");
    assert!(
        map.as_any()
            .downcast_ref::<ast_core::mappings::WinMap>()
            .is_none(),
        "CarLin off must not collapse to a bare WinMap"
    );
}

#[test]
fn carlin_keeps_the_cartesian_projection_description() {
    // C passes the real `prj` (AST__CAR) to WcsSkyFrame (fitschan.c:36088),
    // not the AST__WCSBAD substitute given to astWcsMap, so the SkyFrame's
    // Projection is still "Cartesian".
    let fs = read_car1(true);
    let sky = fs
        .current()
        .as_any()
        .downcast_ref::<SkyFrame>()
        .expect("current frame should be a SkyFrame");
    assert_eq!(sky.projection(), Some("Cartesian"));
}
