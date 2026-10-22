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
        "/tests/fixtures/wcsconv/inputs/sip.head"
    ))
    .expect("sip.head fixture should be readable")
}

// ===========================================================================
// 1. Parse real FITS header
// ===========================================================================

#[test]
fn parse_sip_header() {
    let text = load_sip_header();
    let mut fc = FitsChan::from_header(&text).unwrap();

    // The SIP header has well over 50 cards (288 lines, many are cards).
    assert!(
        fc.ncard().unwrap() > 50,
        "expected > 50 cards, got {}",
        fc.ncard().unwrap()
    );

    // Verify known keywords.
    assert_eq!(fc.get_fits_i(Some("NAXIS")).unwrap(), Some(2));
    assert_eq!(fc.get_fits_i(Some("NAXIS1")).unwrap(), Some(256));
    assert_eq!(fc.get_fits_i(Some("NAXIS2")).unwrap(), Some(256));
    assert_eq!(fc.get_fits_l(Some("SIMPLE")).unwrap(), Some(true));

    // CTYPE1 should be RA---TAN-SIP.
    let ctype1 = fc
        .get_fits_s(Some("CTYPE1"))
        .unwrap()
        .expect("CTYPE1 should exist");
    assert!(
        ctype1.contains("RA"),
        "CTYPE1 should contain 'RA', got '{ctype1}'"
    );
    assert!(
        ctype1.contains("TAN-SIP"),
        "CTYPE1 should contain 'TAN-SIP', got '{ctype1}'"
    );

    // CRPIX1 = 128.0
    let crpix1 = fc
        .get_fits_f(Some("CRPIX1"))
        .unwrap()
        .expect("CRPIX1 should exist");
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
    let mut fc = FitsChan::from_header(&text).unwrap();

    // Integer.
    assert_eq!(fc.get_fits_i(Some("NAXIS")).unwrap(), Some(2));
    assert_eq!(fc.get_fits_i(Some("BITPIX")).unwrap(), Some(-32));

    // Float.
    let crval1 = fc
        .get_fits_f(Some("CRVAL1"))
        .unwrap()
        .expect("CRVAL1 should exist");
    assert!(
        (crval1 - 202.482322805429).abs() < 1e-6,
        "CRVAL1 expected ~202.48, got {crval1}"
    );
    let crpix2 = fc
        .get_fits_f(Some("CRPIX2"))
        .unwrap()
        .expect("CRPIX2 should exist");
    assert!(
        (crpix2 - 128.0).abs() < 1e-10,
        "CRPIX2 expected 128.0, got {crpix2}"
    );

    // String.
    let ctype1 = fc.get_fits_s(Some("CTYPE1")).unwrap().unwrap();
    assert!(ctype1.contains("RA"));
    let telescop = fc.get_fits_s(Some("TELESCOP")).unwrap().unwrap();
    assert!(
        telescop.contains("Spitzer"),
        "TELESCOP expected 'Spitzer', got '{telescop}'"
    );

    // Logical.
    assert_eq!(fc.get_fits_l(Some("SIMPLE")).unwrap(), Some(true));
    // HDRMODE = F
    assert_eq!(fc.get_fits_l(Some("HDRMODE")).unwrap(), Some(false));

    // Non-existent keyword returns None.
    assert_eq!(fc.get_fits_i(Some("NONEXIST")).unwrap(), None);
    assert_eq!(fc.get_fits_f(Some("NONEXIST")).unwrap(), None);
    assert_eq!(fc.get_fits_s(Some("NONEXIST")).unwrap(), None);
    assert_eq!(fc.get_fits_l(Some("NONEXIST")).unwrap(), None);
}

// ===========================================================================
// 3. put_fits / find_fits round-trip
// ===========================================================================

#[test]
fn put_fits_find_fits_round_trip() {
    let mut fc = FitsChan::new().unwrap();

    // Put a card using an 80-char format.
    let card = "TESTKEY =                   42 / A test integer keyword                          ";
    fc.put_fits(card, false).unwrap();
    assert_eq!(fc.ncard().unwrap(), 1);

    // Find it back.
    fc.clear_card().unwrap();
    let found = fc.find_fits("TESTKEY", false).unwrap();
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
    let mut fc = FitsChan::new().unwrap();
    fc.set_fits_i("NAXIS", 4, "Number of axes", false).unwrap();
    assert_eq!(fc.get_fits_i(Some("NAXIS")).unwrap(), Some(4));
}

#[test]
fn set_get_round_trip_float() {
    let mut fc = FitsChan::new().unwrap();
    fc.set_fits_f("CRVAL1", 123.456789012345, "Reference value", false)
        .unwrap();
    let val = fc.get_fits_f(Some("CRVAL1")).unwrap().unwrap();
    assert!(
        (val - 123.456789012345).abs() < 1e-12,
        "Float round-trip failed: got {val}"
    );
}

#[test]
fn set_get_round_trip_string() {
    let mut fc = FitsChan::new().unwrap();
    fc.set_fits_s("CTYPE1", "RA---TAN-SIP", "Axis type", false)
        .unwrap();
    assert_eq!(
        fc.get_fits_s(Some("CTYPE1")).unwrap(),
        Some("RA---TAN-SIP".to_string())
    );
}

#[test]
fn set_get_round_trip_logical() {
    let mut fc = FitsChan::new().unwrap();
    fc.set_fits_l("SIMPLE", true, "Standard FITS", false)
        .unwrap();
    assert_eq!(fc.get_fits_l(Some("SIMPLE")).unwrap(), Some(true));

    fc.set_fits_l("EXTEND", false, "No extensions", false)
        .unwrap();
    assert_eq!(fc.get_fits_l(Some("EXTEND")).unwrap(), Some(false));
}

// ===========================================================================
// 5. del_fits
// ===========================================================================

#[test]
fn del_fits_removes_card() {
    let mut fc = FitsChan::new().unwrap();
    fc.set_fits_i("ALPHA", 1, "First", false).unwrap();
    fc.set_fits_i("BETA", 2, "Second", false).unwrap();
    fc.set_fits_i("GAMMA", 3, "Third", false).unwrap();
    assert_eq!(fc.ncard().unwrap(), 3);

    // Find BETA and position on it.
    fc.clear_card().unwrap();
    let found = fc.find_fits("BETA", false).unwrap();
    assert!(found.is_some());

    // Delete it.
    fc.del_fits().unwrap();
    assert_eq!(fc.ncard().unwrap(), 2);

    // Verify BETA is gone.
    assert_eq!(fc.get_fits_i(Some("BETA")).unwrap(), None);

    // ALPHA and GAMMA still present.
    assert_eq!(fc.get_fits_i(Some("ALPHA")).unwrap(), Some(1));
    assert_eq!(fc.get_fits_i(Some("GAMMA")).unwrap(), Some(3));
}

// ===========================================================================
// 6. empty_fits
// ===========================================================================

#[test]
fn empty_fits_clears_header() {
    let text = load_sip_header();
    let mut fc = FitsChan::from_header(&text).unwrap();
    assert!(fc.ncard().unwrap() > 0);

    fc.empty_fits().unwrap();
    assert_eq!(fc.ncard().unwrap(), 0);

    // All keywords should now return None.
    assert_eq!(fc.get_fits_i(Some("NAXIS")).unwrap(), None);
    assert_eq!(fc.get_fits_s(Some("CTYPE1")).unwrap(), None);
}

// ===========================================================================
// 7. find_fits iteration
// ===========================================================================

#[test]
fn find_fits_iteration_with_inc() {
    let mut fc = FitsChan::new().unwrap();
    fc.set_fits_i("KEY1", 10, "", false).unwrap();
    fc.set_fits_i("KEY2", 20, "", false).unwrap();
    fc.set_fits_i("KEY3", 30, "", false).unwrap();
    fc.set_fits_i("KEY4", 40, "", false).unwrap();

    // Iterate through all cards from beginning using find_fits with a
    // common prefix search won't work directly since each keyword differs.
    // Instead, verify sequential find operations work properly.
    fc.clear_card().unwrap();

    let card1 = fc.find_fits("KEY1", true).unwrap();
    assert!(card1.is_some());
    assert!(card1.unwrap().contains("10"));

    let card2 = fc.find_fits("KEY2", true).unwrap();
    assert!(card2.is_some());
    assert!(card2.unwrap().contains("20"));

    let card3 = fc.find_fits("KEY3", true).unwrap();
    assert!(card3.is_some());
    assert!(card3.unwrap().contains("30"));

    let card4 = fc.find_fits("KEY4", true).unwrap();
    assert!(card4.is_some());
    assert!(card4.unwrap().contains("40"));
}

#[test]
fn find_fits_searches_forward_only() {
    let mut fc = FitsChan::new().unwrap();
    fc.set_fits_i("FIRST", 1, "", false).unwrap();
    fc.set_fits_i("SECOND", 2, "", false).unwrap();
    fc.set_fits_i("THIRD", 3, "", false).unwrap();

    // Advance past SECOND.
    fc.clear_card().unwrap();
    fc.find_fits("SECOND", true).unwrap();

    // C's astFindFits searches forward from the current card only, so FIRST
    // is not found and the FitsChan is left at end-of-file.
    assert_eq!(fc.find_fits("FIRST", true).unwrap(), None);
    assert_eq!(fc.card().unwrap(), fc.ncard().unwrap() + 1);
}

// ===========================================================================
// 8. Channel trait: Native encoding round-trip (ZoomMap)
// ===========================================================================

#[test]
fn native_round_trip_zoommap() {
    let zm = ZoomMap::new(3, 2.5).unwrap();
    let obj = AnyObject::ZoomMap(zm);

    // Write through FitsChan with Native encoding.
    let mut fc = FitsChan::new().unwrap();
    fc.set_encoding(Encoding::Native);
    fc.write(&obj).expect("write should succeed");

    // Should have stored COMMENT cards.
    assert!(
        fc.ncard().unwrap() > 0,
        "FitsChan should have cards after write"
    );

    // Read it back.
    fc.clear_card().unwrap();
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
    sky.set_epoch_mjd(51544.0); // J2000.0
    sky.set_equinox_mjd(51544.0);

    let obj = AnyObject::SkyFrame(sky);

    // Write through FitsChan.
    let mut fc = FitsChan::new().unwrap();
    fc.set_encoding(Encoding::Native);
    fc.write(&obj).expect("write SkyFrame should succeed");
    assert!(fc.ncard().unwrap() > 0);

    // Read back.
    fc.clear_card().unwrap();
    let result = fc.read().expect("read should succeed");
    assert!(result.is_some(), "read should return an object");

    let read_obj = result.unwrap();
    let frame = read_obj.as_frame().expect("should be a Frame");

    // Verify attributes.
    assert_eq!(frame.naxes(), 2);

    // Verify it's a SkyFrame by checking system_string.
    let sys = frame.system_string().unwrap();
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
fn encoding_default_for_empty_chan_is_native() {
    // Mirrors C `astGetEncoding` (fitschan.c:12603): when none of the
    // recognized WCS keywords are present, Native encoding is assumed.
    let mut fc = FitsChan::new().unwrap();
    assert_eq!(fc.encoding().unwrap(), Encoding::Native);
}

#[test]
fn encoding_set_and_get() {
    let mut fc = FitsChan::new().unwrap();

    fc.set_encoding(Encoding::Native);
    assert_eq!(fc.encoding().unwrap(), Encoding::Native);

    fc.set_encoding(Encoding::FitsPc);
    assert_eq!(fc.encoding().unwrap(), Encoding::FitsPc);

    fc.set_encoding(Encoding::FitsIraf);
    assert_eq!(fc.encoding().unwrap(), Encoding::FitsIraf);

    fc.set_encoding(Encoding::FitsAips);
    assert_eq!(fc.encoding().unwrap(), Encoding::FitsAips);

    fc.set_encoding(Encoding::Dss);
    assert_eq!(fc.encoding().unwrap(), Encoding::Dss);
}

#[test]
fn encoding_from_header_auto_detects() {
    // SIP header has CDi_j but no RADECSYS / PROJP / CmVAL, so
    // C's `astGetEncoding` (fitschan.c:12596) skips case 6 (FITS-IRAF
    // requires CDi_j AND one of RADECSYS/PROJP/CmVAL) and falls
    // through to case 9 (CRVALi → FITS-WCS). Mirrors C's lazy
    // auto-detection when no explicit encoding has been set.
    let text = load_sip_header();
    let mut fc = FitsChan::from_header(&text).unwrap();
    assert_eq!(fc.encoding().unwrap(), Encoding::FitsWcs);
}

// ===========================================================================
// Additional: fits_digits attribute
// ===========================================================================

#[test]
fn fits_digits_default_and_set() {
    let mut fc = FitsChan::new().unwrap();
    // Default is AST__DBL_DIG (17), matching C fitschan.c:43079.
    assert_eq!(fc.fits_digits(), 17);

    fc.set_fits_digits(15);
    assert_eq!(fc.fits_digits(), 15);

    // Any integer is kept, as C keeps it; a negative one is a maximum.
    fc.set_fits_digits(0);
    assert_eq!(fc.fits_digits(), 0);
    fc.set_fits_digits(-5);
    assert_eq!(fc.fits_digits(), -5);
    fc.set_fits_digits(25);
    assert_eq!(fc.fits_digits(), 25);
    assert!(fc.test_fits_digits());
    fc.clear_fits_digits();
    assert_eq!(fc.fits_digits(), 17);
    assert!(!fc.test_fits_digits());
    // C tests the value, not whether one was set.
    fc.set_fits_digits(17);
    assert!(!fc.test_fits_digits());
}

// ===========================================================================
// Additional: overwrite behavior
// ===========================================================================

#[test]
fn overwrite_true_replaces_card() {
    let mut fc = FitsChan::new().unwrap();
    fc.set_fits_i("NAXIS", 2, "Original", false).unwrap();
    assert_eq!(fc.ncard().unwrap(), 1);

    // Overwriting replaces the current card, so make NAXIS current first;
    // at end-of-file the card would be appended instead.
    fc.clear_card().unwrap();
    fc.set_fits_i("NAXIS", 3, "Updated", true).unwrap();
    assert_eq!(fc.ncard().unwrap(), 1); // Still only one card.
    assert_eq!(fc.get_fits_i(Some("NAXIS")).unwrap(), Some(3));
}

#[test]
fn overwrite_false_appends_duplicate() {
    let mut fc = FitsChan::new().unwrap();
    fc.set_fits_i("NAXIS", 2, "First", false).unwrap();
    fc.set_fits_i("NAXIS", 3, "Second", false).unwrap();
    // Both cards stored (first occurrence wins for get).
    assert_eq!(fc.ncard().unwrap(), 2);
}

// ===========================================================================
// Additional: rewinding after a native read finds nothing to re-read
// ===========================================================================

#[test]
fn native_rewind_and_reread() {
    // A native read consumes the cards it read: C `Read` calls
    // `FixUsed( this, 0, 1, remove )` with `remove` set for NATIVE encoding
    // (fitschan.c:25923-25929), which physically deletes them rather than
    // merely marking them used. So a rewind after the read has nothing left
    // to find a `BEGAST` in, and the second read returns `None`. Verified
    // against the reference library with a standalone oracle probe: the
    // first `astRead` on a fresh native FitsChan succeeds and leaves one
    // blank card behind; a second `astClearCard` + `astRead` on the same
    // FitsChan returns a null Object pointer.
    let zm = ZoomMap::new(2, 3.0).unwrap();

    let mut fc = FitsChan::new().unwrap();
    fc.set_encoding(Encoding::Native);
    fc.write(&AnyObject::ZoomMap(zm)).unwrap();

    // Read the object.
    fc.clear_card().unwrap();
    let obj1 = fc.read().unwrap().expect("first read");
    let m1 = obj1.as_mapping().unwrap();
    assert_eq!(m1.nin(), 2);

    // Rewind and read again — the cards are gone, so there is nothing left
    // to read.
    fc.clear_card().unwrap();
    let obj2 = fc.read().unwrap();
    assert!(obj2.is_none(), "second read should find no object");
}

// ===========================================================================
// Additional: PROJP cards survive a FITS-PC read
// ===========================================================================

#[test]
fn translate_projp_retains_source_cards() {
    // C's PROJP block (fitschan.c:31578-31631) is only entered when
    // the PROJP-vs-PV disagreement heuristic (fitschan.c:31583-31617)
    // decides PROJP should be trusted over a conflicting PV set; when
    // it decides PV wins instead, PROJP is left completely alone —
    // never read, never marked, never deleted — so it survives into
    // the output header. A full, self-consistent PV1_0/1/2 set
    // alongside PROJP0/1/2 on the longitude axis is exactly the case
    // that makes C prefer PV (confirmed against the reference
    // library's own libast with a standalone oracle probe reading
    // this header and dumping the post-read, Native-re-encoded
    // output).
    let header = "\
SIMPLE  =                    T / Fits standard
NAXIS   =                    2 / Number of dimensions
CTYPE1  = 'RA---TAN'           / Axis type
CTYPE2  = 'DEC--TAN'           / Axis type
CRVAL1  =                 40.0 / Reference value
CRVAL2  =                 60.0 / Reference value
CRPIX1  =                256.0 / Reference pixel
CRPIX2  =                256.0 / Reference pixel
CDELT1  =                -0.01 / Pixel size
CDELT2  =                 0.01 / Pixel size
PROJP0  =                999.0 / Projection parameter
PROJP1  =                999.0 / Projection parameter
PROJP2  =                999.0 / Projection parameter
PV1_0   =                  0.0 / Standard PV
PV1_1   =                  0.0 / Standard PV
PV1_2   =                  0.0 / Standard PV
END
";
    let mut fc = FitsChan::from_header(header).unwrap();
    let _obj = fc.read().unwrap().unwrap();
    fc.set_encoding(Encoding::Native);
    fc.write(&_obj).unwrap();
    let out = fc.to_header().unwrap();
    for key in ["PROJP0", "PROJP1", "PROJP2"] {
        assert!(out.contains(key), "{key} should survive the read");
    }
}

// ===========================================================================
// FitsRounding formatting (testfitschan.c test_fitsrounding)
// ===========================================================================

/// One step of `test_fitsrounding`: an optional card to put, the
/// FitsRounding to set (`None` clears it) and the card text C expects.
type RoundingStep = (Option<&'static str>, Option<i32>, &'static str);

/// The steps of `test_fitsrounding` in C's order. Each step builds on the
/// FitsChan state the previous one left, as the C test does.
const ROUNDING_STEPS: &[RoundingStep] = &[
    (
        Some("DUMMY   =  1.0000010001"),
        None,
        "DUMMY   =         1.0000010001",
    ),
    (
        Some("DUMMY   =  1.00000100001"),
        None,
        "DUMMY   =             1.000001",
    ),
    (
        Some("DUMMY   =  1.0000019991"),
        None,
        "DUMMY   =         1.0000019991",
    ),
    (
        Some("DUMMY   =  1.00000199991"),
        None,
        "DUMMY   =             1.000002",
    ),
    (
        Some("DUMMY   =  -1.0000010000001"),
        None,
        "DUMMY   =            -1.000001",
    ),
    (None, Some(5), "DUMMY   =                 -1.0"),
    (
        Some("DUMMY   =  1.9999969999993E-02"),
        None,
        "DUMMY   =           0.01999997",
    ),
    (None, Some(5), "DUMMY   =                 0.02"),
    (
        Some("DUMMY   =  1.9999969999993E-08"),
        Some(10),
        "DUMMY   =          1.999997E-8",
    ),
    (None, Some(5), "DUMMY   =               2.0E-8"),
    (
        Some("DUMMY   =  9.999999E-6"),
        Some(5),
        "DUMMY   =              10.0E-6",
    ),
    (
        Some("DUMMY   =  -9.999999"),
        Some(5),
        "DUMMY   =                -10.0",
    ),
    (None, None, "DUMMY   =            -9.999999"),
    (None, Some(6), "DUMMY   =                -10.0"),
    (None, Some(7), "DUMMY   =            -9.999999"),
    (
        Some("DUMMY   =  -0.0019999912"),
        Some(6),
        "DUMMY   =        -0.0019999912",
    ),
    (None, Some(5), "DUMMY   =               -0.002"),
];

#[test]
fn fits_rounding_formats_found_cards() {
    let mut fc = FitsChan::new().unwrap();
    assert_eq!(fc.fits_rounding(), 10);
    for (i, &(card, rounding, expected)) in ROUNDING_STEPS.iter().enumerate() {
        match rounding {
            Some(r) => fc.set_fits_rounding(r),
            None => fc.clear_fits_rounding(),
        }
        if let Some(card) = card {
            fc.put_fits(card, false).unwrap();
        }
        fc.clear_card().unwrap();
        let found = fc
            .find_fits("DUMMY", false)
            .unwrap()
            .unwrap_or_else(|| panic!("step {}: DUMMY not found", i + 1));
        assert_eq!(found.trim_end(), expected, "step {}", i + 1);
    }
    fc.clear_fits_rounding();
    assert_eq!(fc.fits_rounding(), 10);
}

// ===========================================================================
// CardList::put
// ===========================================================================

/// `CardList::put` stores a card as `put_fits` does: with `overwrite` it
/// replaces the current card whatever its keyword, and a blank comment keeps
/// the comment of the card replaced.
// C probe: cards A, B, C with the second current, then astPutFits of X=9
// (insert), Y=8 (overwrite) and A=7 (overwrite) gives A / first, X, Y /
// second, A / third, with Card 5.
#[test]
fn card_list_put_overwrites_the_current_card() {
    use ast_core::fitschan::card::FitsValue;
    use ast_core::fitschan::card_list::CardList;

    let one = |text: &str| {
        CardList::from_header(text)
            .unwrap()
            .iter()
            .next()
            .unwrap()
            .clone()
    };
    let mut list = CardList::from_header(
        "A       =                    1 / first\n\
         B       =                    2 / second\n\
         C       =                    3 / third",
    )
    .unwrap();
    list.set_position(1);
    list.put(one("X       =                    9"), false);
    list.put(one("Y       =                    8"), true);
    list.put(one("A       =                    7"), true);
    let got: Vec<(String, FitsValue, Option<String>)> = list
        .iter()
        .map(|c| (c.keyword.clone(), c.value.clone(), c.comment.clone()))
        .collect();
    assert_eq!(
        got,
        vec![
            (
                "A".to_string(),
                FitsValue::Int(1),
                Some("first".to_string())
            ),
            ("X".to_string(), FitsValue::Int(9), None),
            (
                "Y".to_string(),
                FitsValue::Int(8),
                Some("second".to_string())
            ),
            (
                "A".to_string(),
                FitsValue::Int(7),
                Some("third".to_string())
            ),
        ]
    );
    assert_eq!(list.position(), 4);
}

// ===========================================================================
// Defaults chosen from a source not yet read
// ===========================================================================

/// A FITS-WCS header with a CD matrix, given through a source.
const CD_WCS_SOURCE: &str = "NAXIS   =                    2\n\
NAXIS1  =                  100\n\
NAXIS2  =                  100\n\
CTYPE1  = 'RA---TAN'\n\
CTYPE2  = 'DEC--TAN'\n\
CRPIX1  =                   50\n\
CRPIX2  =                   50\n\
CRVAL1  =                   10\n\
CRVAL2  =                   20\n\
CD1_1   =               -0.001\n\
CD2_2   =                0.001";

fn fitschan_with_cd_source() -> FitsChan {
    let mut fc = FitsChan::new().unwrap();
    fc.set_source(Box::new(ast_core::StringSource::new(CD_WCS_SOURCE)));
    fc
}

/// Encoding, CDMatrix and DefB1950 are the first use of a FitsChan with a
/// source: each reads the source before choosing its default.
// C probe: on a fresh astFitsChan with this source, astGetC "Encoding" is
// FITS-WCS, astGetI "CDMatrix" 1 and astGetI "DefB1950" 0.
#[test]
fn defaults_read_a_source_not_yet_read() {
    assert_eq!(
        fitschan_with_cd_source().encoding().unwrap(),
        Encoding::FitsWcs
    );
    assert!(fitschan_with_cd_source().cd_matrix().unwrap());
    assert!(!fitschan_with_cd_source().def_b1950().unwrap());
}

/// Asked first, CDMatrix leaves the FitsChan at end-of-file, since the card
/// it puts back is the one current before the source was read, while
/// Encoding and DefB1950 leave the first source card current.
// C probe: CDMatrix first gives Card 12 of 11 cards, and a following
// astRead returns NULL with no error, a foreign read needing the first card
// current; Encoding or DefB1950 first gives Card 1.
#[test]
fn cd_matrix_from_a_source_leaves_end_of_file_as_c_does() {
    let mut fc = fitschan_with_cd_source();
    assert!(fc.cd_matrix().unwrap());
    assert_eq!(fc.card().unwrap(), 12);
    assert!(fc.read().unwrap().is_none());

    let mut fc = fitschan_with_cd_source();
    fc.encoding().unwrap();
    assert_eq!(fc.card().unwrap(), 1);
    let mut fc = fitschan_with_cd_source();
    fc.def_b1950().unwrap();
    assert_eq!(fc.card().unwrap(), 1);
}

/// A set Encoding is given without reading a source.
// C probe: with Encoding=FITS-WCS set, astGetC "Encoding" pulls no card
// from the source.
#[test]
fn set_encoding_reads_no_source() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Counting(Arc<AtomicUsize>);
    impl ast_core::channel::Source for Counting {
        fn read_line(&mut self) -> Option<String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            None
        }
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let mut fc = FitsChan::new().unwrap();
    fc.set_source(Box::new(Counting(Arc::clone(&calls))));
    fc.set_encoding(Encoding::FitsWcs);
    assert_eq!(fc.encoding().unwrap(), Encoding::FitsWcs);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

// ===========================================================================
// FITSCHAN_OPTIONS
// ===========================================================================

/// The variable that tells a copy of this test program that it is the child
/// [`fitschan_options_applies_to_every_new_fitschan`] started.
const OPTIONS_CHILD: &str = "AST_TEST_FITSCHAN_OPTIONS_CHILD";

/// Every new FitsChan takes the attribute settings in the `FITSCHAN_OPTIONS`
/// environment variable.
///
/// The variable is set only for a copy of this test program that runs this
/// test alone, so no other test in this process sees it.
// C: astFitsChan applies the variable through astEnvSet before its own
// options, so Encoding and FitsDigits set there are what a new FitsChan
// gives, and a bad setting makes the constructor fail; a C probe with the
// variable set to "None" makes a FitsChan with no setting.
#[test]
fn fitschan_options_applies_to_every_new_fitschan() {
    let name = "test_fitschan::fitschan_options_applies_to_every_new_fitschan";
    match std::env::var(OPTIONS_CHILD).as_deref() {
        Ok("good") => {
            for _ in 0..2 {
                let mut fc = FitsChan::new().unwrap();
                assert_eq!(fc.encoding().unwrap(), Encoding::FitsWcs);
                assert!(fc.test_encoding());
                assert_eq!(fc.fits_digits(), 8);
                assert_eq!(fc.warning_conditions(), "badval");
            }
            let fc = FitsChan::from_header("NAXIS   =                    2").unwrap();
            assert_eq!(fc.fits_digits(), 8);
        }
        Ok("bad") => {
            let e = FitsChan::new().unwrap_err();
            assert_eq!(e.code(), ast_core::error::ErrorCode::Badat);
            assert!(e.message().contains("\"bogus=1\""), "{e}");
        }
        Ok("none") => {
            let fc = FitsChan::new().unwrap();
            assert_eq!(fc.fits_digits(), 17);
        }
        Ok(other) => panic!("unknown child mode {other}"),
        Err(_) => {
            // The suite's byte-exact results assume no settings; cargo
            // sets the variable empty for every process it runs
            // (.cargo/config.toml).
            assert!(
                std::env::var("FITSCHAN_OPTIONS").is_ok_and(|v| v.is_empty())
                    || std::env::var_os("FITSCHAN_OPTIONS").is_none(),
                "FITSCHAN_OPTIONS is set for the test process; unset it to run the tests"
            );
            for (mode, options) in [
                (
                    "good",
                    "Encoding=fits_wcs, FitsDigits = 8 , Warnings=\"badval\"",
                ),
                ("bad", "FitsDigits=8, Bogus=1"),
                // C's `astEnvSet` stores "None" for an unset variable, so a
                // variable set to it is ignored.
                ("none", "None"),
            ] {
                let out = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([name, "--exact", "--test-threads=1"])
                    .env(OPTIONS_CHILD, mode)
                    .env("FITSCHAN_OPTIONS", options)
                    .output()
                    .unwrap();
                let stdout = String::from_utf8_lossy(&out.stdout);
                assert!(
                    out.status.success() && stdout.contains("1 passed"),
                    "{mode}: {stdout}{}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }
    }
}
