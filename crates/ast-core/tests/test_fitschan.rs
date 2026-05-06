//! FitsChan integration tests — Phase 1.
//!
//! Exercises the FitsChan public API: card parsing, typed getters/setters,
//! find/delete/empty operations, iteration, and Native-encoding Channel
//! round-trips for AST objects.

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::{Encoding, FitsChan};
use ast_core::frames::{SkyFrame, SkySystem};
use ast_core::mappings::ZoomMap;

// ===========================================================================
// Helpers
// ===========================================================================

/// Load the SIP header fixture text.
fn load_sip_header() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/fits/headers/sip.head"
    ))
    .expect("sip.head fixture should be readable")
}

// ===========================================================================
// 1. Parse real FITS header
// ===========================================================================

#[test]
fn parse_sip_header() {
    let text = load_sip_header();
    let fc = FitsChan::from_header(&text).unwrap();

    // The SIP header has well over 50 cards (288 lines, many are cards).
    assert!(fc.ncard() > 50, "expected > 50 cards, got {}", fc.ncard());

    // Verify known keywords.
    assert_eq!(fc.get_fits_i("NAXIS"), Some(2));
    assert_eq!(fc.get_fits_i("NAXIS1"), Some(256));
    assert_eq!(fc.get_fits_i("NAXIS2"), Some(256));
    assert_eq!(fc.get_fits_l("SIMPLE"), Some(true));

    // CTYPE1 should be RA---TAN-SIP.
    let ctype1 = fc.get_fits_s("CTYPE1").expect("CTYPE1 should exist");
    assert!(
        ctype1.contains("RA"),
        "CTYPE1 should contain 'RA', got '{ctype1}'"
    );
    assert!(
        ctype1.contains("TAN-SIP"),
        "CTYPE1 should contain 'TAN-SIP', got '{ctype1}'"
    );

    // CRPIX1 = 128.0
    let crpix1 = fc.get_fits_f("CRPIX1").expect("CRPIX1 should exist");
    assert!(
        (crpix1 - 128.0).abs() < 1e-10,
        "CRPIX1 expected 128.0, got {crpix1}"
    );
}

// ===========================================================================
// 2. get_fits_i/f/s/l — typed value extraction
// ===========================================================================

#[test]
fn get_fits_typed_values() {
    let text = load_sip_header();
    let fc = FitsChan::from_header(&text).unwrap();

    // Integer.
    assert_eq!(fc.get_fits_i("NAXIS"), Some(2));
    assert_eq!(fc.get_fits_i("BITPIX"), Some(-32));

    // Float.
    let crval1 = fc.get_fits_f("CRVAL1").expect("CRVAL1 should exist");
    assert!(
        (crval1 - 202.482322805429).abs() < 1e-6,
        "CRVAL1 expected ~202.48, got {crval1}"
    );
    let crpix2 = fc.get_fits_f("CRPIX2").expect("CRPIX2 should exist");
    assert!(
        (crpix2 - 128.0).abs() < 1e-10,
        "CRPIX2 expected 128.0, got {crpix2}"
    );

    // String.
    let ctype1 = fc.get_fits_s("CTYPE1").unwrap();
    assert!(ctype1.contains("RA"));
    let telescop = fc.get_fits_s("TELESCOP").unwrap();
    assert!(
        telescop.contains("Spitzer"),
        "TELESCOP expected 'Spitzer', got '{telescop}'"
    );

    // Logical.
    assert_eq!(fc.get_fits_l("SIMPLE"), Some(true));
    // HDRMODE = F
    assert_eq!(fc.get_fits_l("HDRMODE"), Some(false));

    // Non-existent keyword returns None.
    assert_eq!(fc.get_fits_i("NONEXIST"), None);
    assert_eq!(fc.get_fits_f("NONEXIST"), None);
    assert_eq!(fc.get_fits_s("NONEXIST"), None);
    assert_eq!(fc.get_fits_l("NONEXIST"), None);
}

// ===========================================================================
// 3. put_fits / find_fits round-trip
// ===========================================================================

#[test]
fn put_fits_find_fits_round_trip() {
    let mut fc = FitsChan::new();

    // Put a card using an 80-char format.
    let card = "TESTKEY =                   42 / A test integer keyword                          ";
    fc.put_fits(card, false).unwrap();
    assert_eq!(fc.ncard(), 1);

    // Find it back.
    fc.rewind();
    let found = fc.find_fits("TESTKEY", false);
    assert!(found.is_some(), "TESTKEY should be found");

    let found_card = found.unwrap();
    // The found card should be 80 chars and contain the keyword and value.
    assert_eq!(found_card.len(), 80);
    assert!(found_card.starts_with("TESTKEY "));
    assert!(found_card.contains("42"));
}

// ===========================================================================
// 4. set_fits_* / get_fits_* round-trip
// ===========================================================================

#[test]
fn set_get_round_trip_integer() {
    let mut fc = FitsChan::new();
    fc.set_fits_i("NAXIS", 4, "Number of axes", false);
    assert_eq!(fc.get_fits_i("NAXIS"), Some(4));
}

#[test]
fn set_get_round_trip_float() {
    let mut fc = FitsChan::new();
    fc.set_fits_f("CRVAL1", 123.456789012345, "Reference value", false);
    let val = fc.get_fits_f("CRVAL1").unwrap();
    assert!(
        (val - 123.456789012345).abs() < 1e-12,
        "Float round-trip failed: got {val}"
    );
}

#[test]
fn set_get_round_trip_string() {
    let mut fc = FitsChan::new();
    fc.set_fits_s("CTYPE1", "RA---TAN-SIP", "Axis type", false);
    assert_eq!(fc.get_fits_s("CTYPE1"), Some("RA---TAN-SIP".to_string()));
}

#[test]
fn set_get_round_trip_logical() {
    let mut fc = FitsChan::new();
    fc.set_fits_l("SIMPLE", true, "Standard FITS", false);
    assert_eq!(fc.get_fits_l("SIMPLE"), Some(true));

    fc.set_fits_l("EXTEND", false, "No extensions", false);
    assert_eq!(fc.get_fits_l("EXTEND"), Some(false));
}

// ===========================================================================
// 5. del_fits
// ===========================================================================

#[test]
fn del_fits_removes_card() {
    let mut fc = FitsChan::new();
    fc.set_fits_i("ALPHA", 1, "First", false);
    fc.set_fits_i("BETA", 2, "Second", false);
    fc.set_fits_i("GAMMA", 3, "Third", false);
    assert_eq!(fc.ncard(), 3);

    // Find BETA and position on it.
    fc.rewind();
    let found = fc.find_fits("BETA", false);
    assert!(found.is_some());

    // Delete it.
    fc.del_fits().unwrap();
    assert_eq!(fc.ncard(), 2);

    // Verify BETA is gone.
    assert_eq!(fc.get_fits_i("BETA"), None);

    // ALPHA and GAMMA still present.
    assert_eq!(fc.get_fits_i("ALPHA"), Some(1));
    assert_eq!(fc.get_fits_i("GAMMA"), Some(3));
}

// ===========================================================================
// 6. empty_fits
// ===========================================================================

#[test]
fn empty_fits_clears_header() {
    let text = load_sip_header();
    let mut fc = FitsChan::from_header(&text).unwrap();
    assert!(fc.ncard() > 0);

    fc.empty_fits();
    assert_eq!(fc.ncard(), 0);

    // All keywords should now return None.
    assert_eq!(fc.get_fits_i("NAXIS"), None);
    assert_eq!(fc.get_fits_s("CTYPE1"), None);
}

// ===========================================================================
// 7. find_fits iteration
// ===========================================================================

#[test]
fn find_fits_iteration_with_inc() {
    let mut fc = FitsChan::new();
    fc.set_fits_i("KEY1", 10, "", false);
    fc.set_fits_i("KEY2", 20, "", false);
    fc.set_fits_i("KEY3", 30, "", false);
    fc.set_fits_i("KEY4", 40, "", false);

    // Iterate through all cards from beginning using find_fits with a
    // common prefix search won't work directly since each keyword differs.
    // Instead, verify sequential find operations work properly.
    fc.rewind();

    let card1 = fc.find_fits("KEY1", true);
    assert!(card1.is_some());
    assert!(card1.unwrap().contains("10"));

    let card2 = fc.find_fits("KEY2", true);
    assert!(card2.is_some());
    assert!(card2.unwrap().contains("20"));

    let card3 = fc.find_fits("KEY3", true);
    assert!(card3.is_some());
    assert!(card3.unwrap().contains("30"));

    let card4 = fc.find_fits("KEY4", true);
    assert!(card4.is_some());
    assert!(card4.unwrap().contains("40"));
}

#[test]
fn find_fits_wraps_around() {
    let mut fc = FitsChan::new();
    fc.set_fits_i("FIRST", 1, "", false);
    fc.set_fits_i("SECOND", 2, "", false);
    fc.set_fits_i("THIRD", 3, "", false);

    // Advance past SECOND.
    fc.rewind();
    fc.find_fits("SECOND", true);

    // Now search for FIRST — should wrap around and find it.
    let found = fc.find_fits("FIRST", true);
    assert!(
        found.is_some(),
        "find_fits should wrap around to find FIRST"
    );
    assert!(found.unwrap().contains("1"));
}

// ===========================================================================
// 8. Channel trait: Native encoding round-trip (ZoomMap)
// ===========================================================================

#[test]
fn native_round_trip_zoommap() {
    let zm = ZoomMap::new(3, 2.5).unwrap();
    let obj = AnyObject::ZoomMap(zm);

    // Write through FitsChan with Native encoding.
    let mut fc = FitsChan::new();
    fc.set_encoding(Encoding::Native);
    fc.write(&obj).expect("write should succeed");

    // Should have stored COMMENT cards.
    assert!(fc.ncard() > 0, "FitsChan should have cards after write");

    // Read it back.
    fc.rewind();
    let result = fc.read().expect("read should succeed");
    assert!(result.is_some(), "read should return an object");

    let read_obj = result.unwrap();
    let mapping = read_obj.as_mapping().expect("should be a Mapping");
    assert_eq!(mapping.nin(), 3);
    assert_eq!(mapping.nout(), 3);

    // Verify the zoom factor by transforming a point.
    let pts = ast_core::coords::PointSet::from_rows([[1.0, 2.0, 3.0]]);
    let out = mapping.transform(&pts, true).unwrap();
    let p = out.point(0);
    assert!((p[0] - 2.5).abs() < 1e-10);
    assert!((p[1] - 5.0).abs() < 1e-10);
    assert!((p[2] - 7.5).abs() < 1e-10);
}

// ===========================================================================
// 9. Channel trait: Write SkyFrame, read back
// ===========================================================================

#[test]
fn native_round_trip_skyframe() {
    let mut sky = SkyFrame::new(SkySystem::Fk5);
    sky.set_epoch(51544.0); // J2000.0
    sky.set_equinox(51544.0);

    let obj = AnyObject::SkyFrame(sky);

    // Write through FitsChan.
    let mut fc = FitsChan::new();
    fc.set_encoding(Encoding::Native);
    fc.write(&obj).expect("write SkyFrame should succeed");
    assert!(fc.ncard() > 0);

    // Read back.
    fc.rewind();
    let result = fc.read().expect("read should succeed");
    assert!(result.is_some(), "read should return an object");

    let read_obj = result.unwrap();
    let frame = read_obj.as_frame().expect("should be a Frame");

    // Verify attributes.
    assert_eq!(frame.naxes(), 2);

    // Verify it's a SkyFrame by checking system_string.
    let sys = frame.system_string();
    assert!(sys.is_some(), "SkyFrame should have a system string");
    assert!(
        sys.unwrap().contains("FK5") || sys.unwrap().contains("Fk5"),
        "system should be FK5, got {:?}",
        sys
    );
}

// ===========================================================================
// 10. Encoding attribute
// ===========================================================================

#[test]
fn encoding_default_is_fitswcs() {
    let fc = FitsChan::new();
    assert_eq!(fc.encoding(), Encoding::FitsWcs);
}

#[test]
fn encoding_set_and_get() {
    let mut fc = FitsChan::new();

    fc.set_encoding(Encoding::Native);
    assert_eq!(fc.encoding(), Encoding::Native);

    fc.set_encoding(Encoding::FitsPc);
    assert_eq!(fc.encoding(), Encoding::FitsPc);

    fc.set_encoding(Encoding::FitsIraf);
    assert_eq!(fc.encoding(), Encoding::FitsIraf);

    fc.set_encoding(Encoding::FitsAips);
    assert_eq!(fc.encoding(), Encoding::FitsAips);

    fc.set_encoding(Encoding::Dss);
    assert_eq!(fc.encoding(), Encoding::Dss);
}

#[test]
fn encoding_from_header_defaults_to_fitswcs() {
    let text = load_sip_header();
    let fc = FitsChan::from_header(&text).unwrap();
    assert_eq!(fc.encoding(), Encoding::FitsWcs);
}

// ===========================================================================
// Additional: fits_digits attribute
// ===========================================================================

#[test]
fn fits_digits_default_and_set() {
    let mut fc = FitsChan::new();
    // Default is 8.
    assert_eq!(fc.fits_digits(), 8);

    fc.set_fits_digits(15);
    assert_eq!(fc.fits_digits(), 15);

    // Clamped to valid range [1, 17].
    fc.set_fits_digits(0);
    assert_eq!(fc.fits_digits(), 1);
    fc.set_fits_digits(20);
    assert_eq!(fc.fits_digits(), 17);
}

// ===========================================================================
// Additional: overwrite behaviour
// ===========================================================================

#[test]
fn overwrite_true_replaces_card() {
    let mut fc = FitsChan::new();
    fc.set_fits_i("NAXIS", 2, "Original", false);
    assert_eq!(fc.ncard(), 1);

    // Overwrite with new value.
    fc.set_fits_i("NAXIS", 3, "Updated", true);
    assert_eq!(fc.ncard(), 1); // Still only one card.
    assert_eq!(fc.get_fits_i("NAXIS"), Some(3));
}

#[test]
fn overwrite_false_appends_duplicate() {
    let mut fc = FitsChan::new();
    fc.set_fits_i("NAXIS", 2, "First", false);
    fc.set_fits_i("NAXIS", 3, "Second", false);
    // Both cards stored (first occurrence wins for get).
    assert_eq!(fc.ncard(), 2);
}

// ===========================================================================
// Additional: rewind and re-read yields same object
// ===========================================================================

#[test]
fn native_rewind_and_reread() {
    let zm = ZoomMap::new(2, 3.0).unwrap();

    let mut fc = FitsChan::new();
    fc.set_encoding(Encoding::Native);
    fc.write(&AnyObject::ZoomMap(zm)).unwrap();

    // Read the object.
    fc.rewind();
    let obj1 = fc.read().unwrap().expect("first read");
    let m1 = obj1.as_mapping().unwrap();
    assert_eq!(m1.nin(), 2);

    // Rewind and read again — same object.
    fc.rewind();
    let obj2 = fc.read().unwrap().expect("second read");
    let m2 = obj2.as_mapping().unwrap();
    assert_eq!(m2.nin(), 2);
}
