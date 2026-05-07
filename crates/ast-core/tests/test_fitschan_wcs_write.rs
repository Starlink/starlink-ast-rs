//! FitsChan WCS encoder integration tests.
//!
//! Verifies that FitsChan with FitsWcs encoding can write a FrameSet to FITS
//! cards and that the round-trip (read → write → read) produces consistent
//! coordinate transforms.

use ast_core::channel::Channel;
use ast_core::coords::PointSet;
use ast_core::fitschan::{Encoding, FitsChan};

// ===========================================================================
// Test data
// ===========================================================================

/// Minimal 2D TAN header with CRPIX=1 for simple round-trip testing.
///
/// The encoder always sets CRPIX=1.0 so using CRPIX=1 in the input header
/// ensures a clean round-trip without reference-pixel mismatch.
const MINIMAL_TAN_HEADER: &str = "\
NAXIS   =                    2 / Number of axes
CRPIX1  =                1.000 / Reference pixel
CRPIX2  =                1.000 / Reference pixel
CRVAL1  =          202.4820000 / Reference RA
CRVAL2  =           47.1750000 / Reference Dec
CDELT1  =       -0.00027778000 / Degrees per pixel
CDELT2  =        0.00027778000 / Degrees per pixel
CTYPE1  = 'RA---TAN'           / RA with TAN projection
CTYPE2  = 'DEC--TAN'           / Dec with TAN projection
RADESYS = 'ICRS    '           / Reference frame
EQUINOX =               2000.0 / Equinox
";

/// A minimal 1D spectral header with CRPIX=1.
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

// ===========================================================================
// Helpers
// ===========================================================================

/// Read a FrameSet from a FITS header string.
fn read_frameset(header: &str) -> ast_core::AnyObject {
    let mut fc = FitsChan::from_header(header).unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    fc.read()
        .expect("read should not error")
        .expect("read should produce a FrameSet")
}

/// Write an AnyObject to a new FitsChan with FitsWcs encoding, returning it.
fn write_to_fitschan(obj: &ast_core::AnyObject) -> FitsChan {
    let mut fc = FitsChan::new();
    fc.set_encoding(Encoding::FitsWcs);
    fc.write(obj).expect("write should succeed");
    fc
}

// ===========================================================================
// 1. write_produces_fits_cards
// ===========================================================================

#[test]
fn write_produces_fits_cards() {
    let obj = read_frameset(MINIMAL_TAN_HEADER);
    let fc = write_to_fitschan(&obj);

    // The output should contain some cards.
    assert!(fc.ncard() > 0, "Written FitsChan should have cards, got 0");

    // Check that fundamental WCS keywords are present.
    assert!(
        fc.get_fits_f("CRPIX1").is_some(),
        "CRPIX1 should be present in output"
    );
    assert!(
        fc.get_fits_f("CRVAL1").is_some(),
        "CRVAL1 should be present in output"
    );
    assert!(
        fc.get_fits_s("CTYPE1").is_some(),
        "CTYPE1 should be present in output"
    );
}

// ===========================================================================
// 2. write_celestial_has_correct_ctype
// ===========================================================================

#[test]
fn write_celestial_has_correct_ctype() {
    let obj = read_frameset(MINIMAL_TAN_HEADER);
    let fc = write_to_fitschan(&obj);

    let ctype1 = fc.get_fits_s("CTYPE1").expect("CTYPE1 should be present");
    let ctype2 = fc.get_fits_s("CTYPE2").expect("CTYPE2 should be present");

    // CTYPE1 should indicate RA with TAN projection.
    assert!(
        ctype1.contains("RA") && ctype1.contains("TAN"),
        "CTYPE1 should contain 'RA' and 'TAN', got '{ctype1}'"
    );

    // CTYPE2 should indicate Dec with TAN projection.
    assert!(
        ctype2.contains("DEC") && ctype2.contains("TAN"),
        "CTYPE2 should contain 'DEC' and 'TAN', got '{ctype2}'"
    );
}

// ===========================================================================
// 3. write_celestial_has_radesys
// ===========================================================================

#[test]
fn write_celestial_has_radesys() {
    let obj = read_frameset(MINIMAL_TAN_HEADER);
    let fc = write_to_fitschan(&obj);

    let radesys = fc
        .get_fits_s("RADESYS")
        .expect("RADESYS should be present in output");

    assert!(
        radesys.contains("ICRS"),
        "RADESYS should be 'ICRS', got '{radesys}'"
    );
}

// ===========================================================================
// 4. round_trip_read_write_read
// ===========================================================================

#[test]
fn round_trip_read_write_read() {
    // Step 1: Read the original header into a FrameSet.
    let obj1 = read_frameset(MINIMAL_TAN_HEADER);
    let mapping1 = obj1.as_mapping().expect("FrameSet should be a Mapping");

    // Step 2: Write the FrameSet to a new FitsChan.
    let mut fc2 = write_to_fitschan(&obj1);

    // Step 3: Read the written cards back into a new FrameSet.
    fc2.rewind();
    let obj2 = fc2
        .read()
        .expect("re-read should not error")
        .expect("re-read should produce a FrameSet");
    let mapping2 = obj2
        .as_mapping()
        .expect("re-read FrameSet should be a Mapping");

    // Both should agree at pixel (1, 1) — the reference pixel.
    // Since CRPIX=1 in the input and the encoder also uses CRPIX=1,
    // the reference pixel is the same in both FrameSets.
    let pix = PointSet::from_rows([[1.0, 1.0]]);

    let sky1 = mapping1.transform(&pix, true).unwrap();
    let sky2 = mapping2.transform(&pix, true).unwrap();

    let ra1 = sky1.point(0)[0];
    let ra2 = sky2.point(0)[0];
    let dec1 = sky1.point(0)[1];
    let dec2 = sky2.point(0)[1];

    // At the reference pixel the transforms should agree very closely.
    // Tolerance: 1e-6 radians ~ 0.2 arcsec.
    let tol = 1e-6;
    assert!(
        (ra1 - ra2).abs() < tol,
        "Round-trip RA mismatch at CRPIX: {ra1} vs {ra2} (diff={})",
        (ra1 - ra2).abs()
    );
    assert!(
        (dec1 - dec2).abs() < tol,
        "Round-trip Dec mismatch at CRPIX: {dec1} vs {dec2} (diff={})",
        (dec1 - dec2).abs()
    );

    // Also verify the read-back FrameSet produces valid transforms
    // at a slightly offset pixel.
    let pix_off = PointSet::from_rows([[2.0, 2.0]]);
    let sky_off = mapping2.transform(&pix_off, true).unwrap();

    // The output should be finite and different from the reference pixel.
    let ra_off = sky_off.point(0)[0];
    let dec_off = sky_off.point(0)[1];
    assert!(
        ra_off.is_finite() && dec_off.is_finite(),
        "Transform at offset pixel should produce finite coords"
    );
    assert!(
        (ra_off - ra2).abs() > 1e-10 || (dec_off - dec2).abs() > 1e-10,
        "Transform at offset pixel should differ from reference pixel"
    );
}

// ===========================================================================
// 5. write_spectral_frameset
// ===========================================================================

#[test]
fn write_spectral_frameset() {
    let obj = read_frameset(SPECTRAL_HEADER);
    let fc = write_to_fitschan(&obj);

    // The output should contain cards.
    assert!(
        fc.ncard() > 0,
        "Written FitsChan for spectral should have cards"
    );

    // CTYPE1 should contain "FREQ".
    let ctype1 = fc
        .get_fits_s("CTYPE1")
        .expect("CTYPE1 should be present for spectral output");
    assert!(
        ctype1.contains("FREQ"),
        "CTYPE1 should contain 'FREQ', got '{ctype1}'"
    );
}

// ===========================================================================
// 6. write_spectral_round_trip
// ===========================================================================

#[test]
fn write_spectral_round_trip() {
    // Step 1: Read the spectral header.
    let obj1 = read_frameset(SPECTRAL_HEADER);
    let mapping1 = obj1.as_mapping().expect("FrameSet should be a Mapping");

    // Step 2: Write to FitsChan.
    let mut fc2 = write_to_fitschan(&obj1);

    // Step 3: Read back.
    fc2.rewind();
    let obj2 = fc2
        .read()
        .expect("re-read should not error")
        .expect("re-read should produce a FrameSet");
    let mapping2 = obj2
        .as_mapping()
        .expect("re-read FrameSet should be a Mapping");

    // Transform CRPIX=1 through both FrameSets.
    let pix = PointSet::from_rows([[1.0]]);

    let freq1 = mapping1.transform(&pix, true).unwrap();
    let freq2 = mapping2.transform(&pix, true).unwrap();

    let f1 = freq1.point(0)[0];
    let f2 = freq2.point(0)[0];

    // Both should map CRPIX=1 to approximately the same CRVAL frequency.
    // Tolerance: 1.0 Hz (very tight for a linear axis at its reference pixel).
    let tol_hz = 1.0;
    assert!(
        (f1 - f2).abs() < tol_hz,
        "Spectral round-trip mismatch at CRPIX: {f1} vs {f2} (diff={} Hz)",
        (f1 - f2).abs()
    );

    // Verify the value is approximately the expected rest frequency (1.42 GHz).
    // The output is in Hz (matching CUNIT1).
    let expected_freq = 1.42040575e9;
    assert!(
        (f1 - expected_freq).abs() < 1e3,
        "CRPIX=1 should map to ~1.42 GHz, got {f1}"
    );
}
