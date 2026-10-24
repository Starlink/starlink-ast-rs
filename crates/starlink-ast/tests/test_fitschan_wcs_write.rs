//! FitsChan WCS encoder integration tests.
//!
//! Verifies that FitsChan with FitsWcs encoding can write a FrameSet to FITS
//! cards and that the round-trip (read → write → read) produces consistent
//! coordinate transforms.

use starlink_ast::channel::Channel;
use starlink_ast::coords::PointSet;
use starlink_ast::fitschan::{Encoding, FitsChan};

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
fn read_frameset(header: &str) -> starlink_ast::AnyObject {
    let mut fc = FitsChan::from_header(header).unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    fc.read()
        .expect("read should not error")
        .expect("read should produce a FrameSet")
}

/// Write an AnyObject to a new FitsChan with FitsWcs encoding, returning it.
fn write_to_fitschan(obj: &starlink_ast::AnyObject) -> FitsChan {
    let mut fc = FitsChan::new().unwrap();
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
    let mut fc = write_to_fitschan(&obj);

    // The output should contain some cards.
    assert!(
        fc.ncard().unwrap() > 0,
        "Written FitsChan should have cards, got 0"
    );

    // Check that fundamental WCS keywords are present.
    assert!(
        fc.get_fits_f(Some("CRPIX1")).unwrap().is_some(),
        "CRPIX1 should be present in output"
    );
    assert!(
        fc.get_fits_f(Some("CRVAL1")).unwrap().is_some(),
        "CRVAL1 should be present in output"
    );
    assert!(
        fc.get_fits_s(Some("CTYPE1")).unwrap().is_some(),
        "CTYPE1 should be present in output"
    );
}

// ===========================================================================
// 2. write_celestial_has_correct_ctype
// ===========================================================================

#[test]
fn write_celestial_has_correct_ctype() {
    let obj = read_frameset(MINIMAL_TAN_HEADER);
    let mut fc = write_to_fitschan(&obj);

    let ctype1 = fc
        .get_fits_s(Some("CTYPE1"))
        .unwrap()
        .expect("CTYPE1 should be present");
    let ctype2 = fc
        .get_fits_s(Some("CTYPE2"))
        .unwrap()
        .expect("CTYPE2 should be present");

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
    let mut fc = write_to_fitschan(&obj);

    let radesys = fc
        .get_fits_s(Some("RADESYS"))
        .unwrap()
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
    fc2.clear_card().unwrap();
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
    let mut fc = write_to_fitschan(&obj);

    // The output should contain cards.
    assert!(
        fc.ncard().unwrap() > 0,
        "Written FitsChan for spectral should have cards"
    );

    // CTYPE1 should contain "FREQ".
    let ctype1 = fc
        .get_fits_s(Some("CTYPE1"))
        .unwrap()
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
    fc2.clear_card().unwrap();
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

// ===========================================================================
// Grism interference order
//
// Ancestor: `reference/starlink-ast/ast_tester/testfitschan.c`, `testgrismorder`.
// ===========================================================================

/// A FITS grism header must preserve its interference order through a
/// read/write round trip. `PVi_1` holds the order, which is commonly negative.
///
/// The positive case is a control: it confirms the chosen grating parameters
/// are physically usable, so a failure in the negative cases cannot be blamed
/// on the parameter choice.
#[test]
fn grism_interference_order_survives_a_round_trip() {
    for morder in [1.0_f64, -1.0, -2.0] {
        let header = format!(
            "CTYPE1  = 'WAVE-GRI'\n\
             CRVAL1  =            5.0E-7\n\
             CRPIX1  =                1.0\n\
             CDELT1  =            1.0E-9\n\
             CUNIT1  = 'm       '\n\
             PV1_0   =            9.0E5\n\
             PV1_1   = {morder:20.1}\n\
             PV1_2   =                0.0\n\
             PV1_3   =                1.0\n\
             PV1_4   =                0.0\n\
             PV1_5   =                0.0\n\
             PV1_6   =                0.0\n\
             END"
        );
        let obj = read_frameset(&header);
        let mut out = write_to_fitschan(&obj);
        let back = out
            .get_fits_f(Some("PV1_1"))
            .unwrap()
            .unwrap_or_else(|| panic!("no PV1_1 written back for order {morder}"));
        assert_eq!(
            back, morder,
            "grism interference order not preserved: in {morder}, out {back}"
        );

        // A grism description is only usable if the linear part came out too:
        // without a CRPIX the header does not locate the axis at all, and a
        // CDELT that lost the input's scale silently rewrites the dispersion.
        // Checking `PV1_1` alone passed while both were wrong.
        assert_eq!(
            out.get_fits_f(Some("CRPIX1")).unwrap(),
            Some(1.0),
            "grism output must carry a reference pixel for order {morder}"
        );
        let cdelt1 = out
            .get_fits_f(Some("CDELT1"))
            .unwrap()
            .unwrap_or_else(|| panic!("no CDELT1 written back for order {morder}"));
        assert!(
            (cdelt1 - 1.0e-9).abs() < 1.0e-12,
            "grism output CDELT1 should be the input's 1e-9 m/pixel, got {cdelt1}"
        );
    }
}

/// A grism axis sampled over a range wide enough that its non-linearity
/// exceeds the linearity tolerance must still write, and the header it writes
/// must reproduce the original axis.
///
/// On this header both the C library and the port find the axis linear enough
/// to describe as a plain `WAVE` axis, so C's own `-GRI`/`-GRA` test and the
/// `844`/`845` checks it guards do not run. The C output was captured with an
/// oracle probe and is asserted here card for card, which is what makes the
/// case a gate rather than a write that happens not to fail.
///
/// Ancestor: `stopit( 842 )` to `stopit( 845 )`.
#[test]
fn a_non_linear_grism_axis_writes_a_header_that_reads_back() {
    let header = "NAXIS   =                    1
NAXIS1  =                10000
CTYPE1  = 'WAVE-GRI'
CRVAL1  =            5.000E-07
CRPIX1  =               5000.0
CDELT1  =            1.000E-10
CUNIT1  = 'm'
RESTFRQ =           5.996E+14
SPECSYS = 'BARYCENT'
PV1_0   =            3.000E-06
PV1_1   =                    1
PV1_2   =                  0.0
END";
    let obj = read_frameset(header);

    let mut fc = FitsChan::new().unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    fc.put_fits("NAXIS   =                    1", false)
        .unwrap();
    fc.put_fits("NAXIS1  =                10000", false)
        .unwrap();
    let n = fc.write(&obj).expect("write should not error");
    assert_eq!(n, 1, "a grism FrameSet should write one description");

    // The cards the C library writes for this header, from an oracle probe
    // against `reference/starlink-ast`.
    let expected = "\
NAXIS   =                    1
NAXIS1  =                10000
CRPIX1  =               5001.0 / Reference pixel on axis 1
CRVAL1  =             5.001E-7 / Value at ref. pixel on axis 1
CTYPE1  = 'WAVE    '           / Type of co-ordinate on axis 1
CDELT1  =              1.0E-10 / Pixel size on axis 1
CUNIT1  = 'm       '           / Units for axis 1
SPECSYS = 'BARYCENT'           / Standard of rest for spectral axis
VELOSYS =   29327.576999871173 / [m/s] Topo. apparent velocity of rest frame
RESTWAV = 4.9998742161440962E-7 / [m] Rest wavelength";
    let written: Vec<String> = fc
        .to_header()
        .unwrap()
        .lines()
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.is_empty() && l != "END")
        .collect();
    let want: Vec<String> = expected.lines().map(|l| l.to_string()).collect();
    assert_eq!(
        written, want,
        "grism write should match the C library's cards"
    );

    fc.clear_card().unwrap();
    let obj2 = fc
        .read()
        .expect("read-back should not error")
        .expect("read-back should produce a FrameSet");

    let pix = PointSet::from_rows([[5000.0]]);
    let w1 = obj
        .as_mapping()
        .expect("FrameSet should be a Mapping")
        .transform(&pix, true)
        .unwrap();
    let w2 = obj2
        .as_mapping()
        .expect("re-read FrameSet should be a Mapping")
        .transform(&pix, true)
        .unwrap();
    let (x1, x2) = (w1.point(0)[0], w2.point(0)[0]);
    assert!(
        (x1 - x2).abs() <= 1.0e-6 * x1.abs(),
        "grism round-trip mismatch at the reference pixel: {x1} vs {x2}"
    );
}

// ===========================================================================
// SIP descriptions must be validated over the image
//
// Ancestor: `reference/starlink-ast/ast_tester/testfitschan.c`, the `905`-`912`
// cases added by upstream commits `c384289f` and `121d12d2`.
// ===========================================================================

/// Read the DP2 difference-image FrameSet whose pixel→sky Mapping contains a
/// SplineMap, so has no exact FITS-WCS representation.
///
/// Its SIP reference point lies some 20000 pixels outside the image, which is
/// what made the historic defect reachable: `SIPIntWorld` tested the Mapping
/// following the SIP polynomial over a box of plus and minus the image
/// dimensions about that point, and every sample fell outside the spline
/// knots. The SplineMap sets `OutUnit`, so those points passed through
/// unchanged and the check saw an exactly linear Mapping.
fn read_lsst2_frameset() -> starlink_ast::AnyObject {
    let path = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/lsst2.ast"
    ));
    let mut ch =
        starlink_ast::channel::NativeChannel::from_file(path).expect("lsst2.ast should open");
    let obj = ch
        .read()
        .expect("lsst2.ast should not error")
        .expect("lsst2.ast should hold an object");
    assert!(
        matches!(obj, starlink_ast::AnyObject::FrameSet(_)),
        "lsst2.ast should hold a FrameSet"
    );
    obj
}

/// A FitsChan carrying a 1000x1000 image size, so the pixel box is known.
fn chan_with_dims(tol: Option<f64>) -> FitsChan {
    let mut fc = FitsChan::new().unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    if let Some(tol) = tol {
        fc.set_fits_tol(tol);
    }
    fc.put_fits("NAXIS1  =                 1000", false)
        .unwrap();
    fc.put_fits("NAXIS2  =                 1000", false)
        .unwrap();
    fc
}

/// Collect the keywords a FitsChan holds, in card order.
fn keywords(fc: &FitsChan) -> Vec<String> {
    fc.to_header()
        .unwrap()
        .lines()
        .map(|line| line[..8.min(line.len())].trim_end().to_string())
        .filter(|kw| !kw.is_empty() && kw != "END")
        .collect()
}

/// At a FitsTol tight enough that no linear approximation spans the image, the
/// FrameSet has no FITS-WCS representation at all, so the write reports that
/// it wrote nothing and leaves the header carrying only what it was given.
///
/// Ancestor: `stopit( 906, ... )`.
#[test]
fn a_splinemap_frameset_writes_nothing_at_a_tight_fits_tol() {
    let obj = read_lsst2_frameset();
    let mut fc = chan_with_dims(Some(1.0e-4));
    let n = fc.write(&obj).expect("write should not error");
    assert_eq!(
        n, 0,
        "a FrameSet with no adequate linear fit should write nothing"
    );
    assert_eq!(
        keywords(&fc),
        vec!["NAXIS1", "NAXIS2"],
        "a failed write should leave only the cards it was given"
    );
}

/// The same FrameSet in the shape `lsst.images`' `Transform.as_fits_wcs`
/// actually writes it: wrapped in an outer FrameSet whose base frame carries
/// the FITS one-based origin shift.
///
/// Ancestor: `stopit( 907, ... )`.
#[test]
fn the_as_fits_wcs_wrapping_of_a_splinemap_frameset_also_writes_nothing() {
    use starlink_ast::frames::{BasicFrameBuilder, FrameRef, FrameSet, SkyFrame};
    use starlink_ast::mappings::ShiftMap;

    let inner = match read_lsst2_frameset() {
        starlink_ast::AnyObject::FrameSet(fs) => fs,
        _ => unreachable!(),
    };

    let mut detector = BasicFrameBuilder::new(2).build().unwrap();
    detector.set_ident("detector");
    let mut outer = FrameSet::new(Box::new(detector));
    outer
        .add_frame(
            FrameRef::Base,
            Box::new(inner),
            Box::new(SkyFrame::unset_system()),
        )
        .unwrap();
    let sky_idx = outer.nframe();

    outer
        .add_frame(
            FrameRef::Base,
            Box::new(ShiftMap::new(vec![1.0, 1.0])),
            Box::new(BasicFrameBuilder::new(2).domain("GRID").build().unwrap()),
        )
        .unwrap();
    outer.set_base(FrameRef::Index(outer.nframe())).unwrap();
    outer.set_current(FrameRef::Index(sky_idx)).unwrap();

    let mut fc = chan_with_dims(Some(1.0e-4));
    fc.set_cd_matrix(true);
    let n = fc
        .write(&starlink_ast::AnyObject::FrameSet(outer))
        .expect("write should not error");
    assert_eq!(n, 0, "the as_fits_wcs wrapping should write nothing either");
}

/// An alternate axis description qualifies a primary one and cannot stand on
/// its own, so a FrameSet whose primary description cannot be written must
/// write nothing at all — even when its alternates could each be described
/// perfectly well.
///
/// Here the alternates are trivially describable (a UnitMap onto a 2-axis
/// Frame), while the primary is the SplineMap chain at a FitsTol no linear
/// approximation meets. At the default tolerance the same FrameSet writes all
/// three descriptions, which is what makes the tight-tolerance case a check on
/// the primary gate rather than on the alternates being undescribable.
///
/// C's own test suite has no case for this; the gate is C's `if( !primok )
/// break;` at `fitschan.c:37941`, added by `c384289f` together with the
/// `primok` flag that feeds it.
#[test]
fn alternate_descriptions_are_withheld_when_the_primary_cannot_be_written() {
    use starlink_ast::frames::{BasicFrameBuilder, FrameRef};
    use starlink_ast::mappings::UnitMap;

    let mut fs = match read_lsst2_frameset() {
        starlink_ast::AnyObject::FrameSet(fs) => fs,
        _ => unreachable!(),
    };
    let sky = fs.nframe();
    let mut alt = BasicFrameBuilder::new(2).domain("MYALT").build().unwrap();
    alt.set_ident("A");
    fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), Box::new(alt))
        .unwrap();
    fs.set_current(FrameRef::Index(sky)).unwrap();
    let obj = starlink_ast::AnyObject::FrameSet(fs);

    // At the default tolerance every description is written, alternates
    // included, so the alternates are known to be describable.
    let mut fc = chan_with_dims(None);
    let n = fc.write(&obj).expect("write should not error");
    assert_eq!(n, 1, "the default FitsTol should describe this FrameSet");
    let kw = keywords(&fc);
    assert!(
        kw.iter().any(|k| k == "CTYPE1A"),
        "the alternate description should be present, got {kw:?}"
    );

    // At a tolerance the primary cannot meet, the describable alternates go
    // with it.
    let mut fc = chan_with_dims(Some(1.0e-4));
    let n = fc.write(&obj).expect("write should not error");
    assert_eq!(n, 0, "a failed primary description should write nothing");
    assert_eq!(
        keywords(&fc),
        vec!["NAXIS1", "NAXIS2"],
        "no alternate description may survive a failed primary"
    );
}

/// A FrameSet whose pixel-to-world Mapping has no inverse cannot be described
/// in FITS-WCS at all, because the encoder derives CRPIX by transforming the
/// IWC origin back into grid coordinates. `astWrite` must report that it wrote
/// nothing rather than emit a partial header.
///
/// Ancestor: `stopit( 904, ... )`.
#[test]
fn a_frameset_whose_mapping_has_no_inverse_writes_nothing() {
    use starlink_ast::frames::{BasicFrameBuilder, FrameRef, FrameSet};
    use starlink_ast::mappings::{Mapping, PolyMap, PolyTerm};

    let term = |coeff: f64, powers: [usize; 2], out_axis: usize| PolyTerm {
        coeff,
        powers: powers.to_vec(),
        out_axis,
    };
    // Forward only: no inverse coefficients and IterInverse left off, so the
    // Mapping cannot be inverted numerically either.
    let mut poly = PolyMap::new(2, 2, &[term(1.0, [1, 0], 1), term(1.0, [0, 1], 2)], None).unwrap();
    poly.set_iter_inverse(false).unwrap();
    assert!(
        !poly.has_inverse(),
        "the probe needs a Mapping with no inverse"
    );

    let mut fs = FrameSet::new(Box::new(
        BasicFrameBuilder::new(2).domain("PIXEL").build().unwrap(),
    ));
    fs.add_frame(
        FrameRef::Base,
        Box::new(poly),
        Box::new(BasicFrameBuilder::new(2).build().unwrap()),
    )
    .unwrap();

    let mut fc = FitsChan::new().unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    fc.put_fits("NAXIS1  =                  100", false)
        .unwrap();
    fc.put_fits("NAXIS2  =                  100", false)
        .unwrap();
    let n = fc
        .write(&starlink_ast::AnyObject::FrameSet(fs))
        .expect("write should not error");
    assert_eq!(n, 0, "a non-invertable FrameSet should write nothing");
    assert_eq!(
        keywords(&fc),
        vec!["NAXIS1", "NAXIS2"],
        "a failed write should leave only the cards it was given"
    );
}

/// At the default FitsTol the same FrameSet does have an adequate linear
/// approximation, so the write succeeds — and the header it produces must be
/// usable. `astWrite` used to report success while emitting SIP coefficients
/// and no primary axis description at all, because `SIPIntWorld` returned bad
/// CRPIX values that `MakeIntWorld` stored as the whole description.
///
/// Ancestor: `stopit( 908, ... )` and `stopit( 909, ... )`.
#[test]
fn a_splinemap_frameset_writes_a_complete_header_at_the_default_fits_tol() {
    let obj = read_lsst2_frameset();
    let mut fc = chan_with_dims(None);
    let n = fc.write(&obj).expect("write should not error");
    assert_eq!(
        n, 1,
        "the default FitsTol should admit a linear approximation"
    );

    let kw = keywords(&fc);
    assert!(
        kw.iter().any(|k| k == "CTYPE1") && kw.iter().any(|k| k == "CTYPE2"),
        "a successful write must carry a primary axis description, got {kw:?}"
    );
    assert!(
        !kw.iter().any(|k| k == "A_ORDER"),
        "no SIP description should be offered here, got {kw:?}"
    );
}

/// A FrameSet whose SIP reference point lies 20000 pixels outside the image,
/// with the Mapping between the SIP polynomial and the CD matrix made slightly
/// non-linear by a cubic term that is negligible near the reference point but
/// not over the image.
///
/// The SIP linearity test used to run over a box of plus and minus the image
/// dimensions about the reference point, and passed FitsTol — which is in
/// pixels — straight to the linear-approximation fit, which wants a
/// displacement in the Mapping's output space. Both were wrong here, so the
/// SIP description was accepted with CRPIX and CD fitted where the image is
/// not. The write must instead fall back to a plain linear approximation,
/// which the FitsTol check does validate over the image.
///
/// Ancestor: `stopit( 911, ... )` and `stopit( 912, ... )`.
#[test]
fn a_sip_description_is_rejected_when_it_is_not_linear_over_the_image() {
    use starlink_ast::frames::{BasicFrameBuilder, FrameRef, FrameSet};
    use starlink_ast::mappings::{
        CmpMap, Composition, DynMapping, MathMap, PolyMap, PolyTerm, ShiftMap,
    };

    let base_header = "\
CTYPE1  = 'RA---TAN'
CTYPE2  = 'DEC--TAN'
CRPIX1  =                  0.0
CRPIX2  =                  0.0
CRVAL1  =                180.0
CRVAL2  =                  0.0
CD1_1   =             -5.5E-05
CD1_2   =                  0.0
CD2_1   =                  0.0
CD2_2   =              5.5E-05
RADESYS = 'ICRS'
";
    let tan = match read_frameset(base_header) {
        starlink_ast::AnyObject::FrameSet(fs) => fs,
        _ => unreachable!(),
    };

    // A SIP-shaped PolyMap: a unit linear term per axis plus one quadratic
    // cross term, as the SIP conventions require.
    let term = |coeff: f64, powers: [usize; 2], out_axis: usize| PolyTerm {
        coeff,
        powers: powers.to_vec(),
        out_axis,
    };
    let poly = PolyMap::new(
        2,
        2,
        &[
            term(1.0, [1, 0], 1),
            term(1.0e-9, [2, 0], 1),
            term(1.0, [0, 1], 2),
            term(1.0e-9, [0, 2], 2),
        ],
        Some(&[
            term(1.0, [1, 0], 1),
            term(-1.0e-9, [2, 0], 1),
            term(1.0, [0, 1], 2),
            term(-1.0e-9, [0, 2], 2),
        ]),
    )
    .unwrap();

    // The cubic term sits between the PolyMap and the CD matrix, so it is
    // part of the Mapping whose linearity the SIP test judges.
    let cubic = MathMap::new(
        2,
        2,
        &["u = x + 1.0E-15*x*x*x", "v = y"],
        &["x = u - 1.0E-15*u*u*u", "y = v"],
    )
    .unwrap();

    let series = |a: DynMapping, b: DynMapping| -> DynMapping {
        Box::new(CmpMap::new(a, b, Composition::Series).unwrap())
    };
    let total = series(
        series(
            series(
                Box::new(ShiftMap::new(vec![20000.0, 20000.0])),
                Box::new(poly),
            ),
            Box::new(cubic),
        ),
        tan.mapping(FrameRef::Base, FrameRef::Current).unwrap(),
    );

    let mut fs = FrameSet::new(Box::new(
        BasicFrameBuilder::new(2).domain("GRID").build().unwrap(),
    ));
    fs.add_frame(
        FrameRef::Base,
        total,
        tan.frame(FrameRef::Current).unwrap().snapshot(),
    )
    .unwrap();

    let mut fc = FitsChan::new().unwrap();
    fc.set_encoding(Encoding::FitsWcs);
    fc.set_cd_matrix(true);
    fc.put_fits("NAXIS1  =                 4000", false)
        .unwrap();
    fc.put_fits("NAXIS2  =                 4000", false)
        .unwrap();
    let n = fc
        .write(&starlink_ast::AnyObject::FrameSet(fs))
        .expect("write should not error");
    assert_eq!(
        n, 1,
        "the plain linear approximation is good enough, so the write should succeed"
    );
    assert!(
        !keywords(&fc).iter().any(|k| k == "A_ORDER"),
        "a SIP description must not be accepted when the Mapping following the \
         SIP polynomial is not linear over the image"
    );
}
