//! Integration tests for alternative FITS WCS encodings.
//!
//! Tests encoding auto-detection (guess) and read for all supported formats:
//! FITS-IRAF, FITS-PC, FITS-AIPS, FITS-CLASS, and DSS.
//!
//! Fixture files are copied from `reference/starlink-ast/ast_tester/`.

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::coords::PointSet;
use ast_core::fitschan::{Encoding, FitsChan};
use ast_core::mappings::Mapping;

// ===========================================================================
// Helpers
// ===========================================================================

fn load_fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/fits/headers/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("fixture {name} not found at {path}"))
}

// ===========================================================================
// 1. Guess encoding tests
// ===========================================================================

#[test]
fn guess_encoding_iraf() {
    // serpens.head has CDi_j matrix keywords + RADECSYS -> FitsIraf
    let text = load_fixture("serpens.head");
    let fc = FitsChan::from_header(&text).unwrap();
    let enc = fc.encoding();
    assert_eq!(
        enc,
        Encoding::FitsIraf,
        "serpens.head (CDi_j + RADECSYS) should guess as FITS-IRAF, got {enc:?}"
    );
}

#[test]
fn guess_encoding_pc() {
    // pc-sparse.head has PC001001 keyword -> FitsPc
    let text = load_fixture("pc-sparse.head");
    let fc = FitsChan::from_header(&text).unwrap();
    let enc = fc.encoding();
    assert_eq!(
        enc,
        Encoding::FitsPc,
        "pc-sparse.head (PC001001) should guess as FITS-PC, got {enc:?}"
    );
}

#[test]
fn guess_encoding_aips() {
    // velo-aips.head has AIPS-convention spectral CTYPE (VELO-LSR) -> FitsAips
    let text = load_fixture("velo-aips.head");
    let fc = FitsChan::from_header(&text).unwrap();
    let enc = fc.encoding();
    assert_eq!(
        enc,
        Encoding::FitsAips,
        "velo-aips.head (VELO-LSR) should guess as FITS-AIPS, got {enc:?}"
    );
}

#[test]
fn guess_encoding_class() {
    // class-write.head has DELTAV + VELO-LSR -> FitsClass
    let text = load_fixture("class-write.head");
    let fc = FitsChan::from_header(&text).unwrap();
    let enc = fc.encoding();
    assert_eq!(
        enc,
        Encoding::FitsClass,
        "class-write.head (DELTAV + VELO-LSR) should guess as FITS-CLASS, got {enc:?}"
    );
}

#[test]
fn guess_encoding_dss() {
    // dss.dss has PLTRAH keyword -> Dss
    let text = load_fixture("dss.dss");
    let fc = FitsChan::from_header(&text).unwrap();
    let enc = fc.encoding();
    assert_eq!(
        enc,
        Encoding::Dss,
        "dss.dss (PLTRAH) should guess as DSS, got {enc:?}"
    );
}

// ===========================================================================
// 2. Read tests — verify each encoding produces a FrameSet
// ===========================================================================

#[test]
fn read_iraf_produces_frameset() {
    let text = load_fixture("serpens.head");
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsIraf);
    let result = fc.read().unwrap();
    assert!(result.is_some(), "IRAF read should produce a FrameSet");
    if let Some(AnyObject::FrameSet(ref fs)) = result {
        assert_eq!(fs.nframe(), 2);
        assert_eq!(fs.current().naxes(), 2);
    } else {
        panic!("Expected FrameSet, got {:?}", result);
    }
}

#[test]
fn read_pc_produces_frameset() {
    let text = load_fixture("pc-sparse.head");
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsPc);
    let result = fc.read().unwrap();
    assert!(result.is_some(), "PC read should produce a FrameSet");
    if let Some(AnyObject::FrameSet(ref fs)) = result {
        assert_eq!(fs.nframe(), 2);
        assert_eq!(fs.current().naxes(), 2);
    } else {
        panic!("Expected FrameSet, got {:?}", result);
    }
}

#[test]
fn read_aips_spectral() {
    let text = load_fixture("velo-aips.head");
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsAips);
    let result = fc.read().unwrap();
    assert!(result.is_some(), "AIPS read should produce a FrameSet");
    if let Some(AnyObject::FrameSet(ref fs)) = result {
        // 1D spectral header: pixel frame + spectral frame.
        assert_eq!(fs.nframe(), 2);
        assert_eq!(fs.current().naxes(), 1);
    } else {
        panic!("Expected FrameSet, got {:?}", result);
    }
}

#[test]
fn read_class_produces_frameset() {
    let text = load_fixture("class-write.head");
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsClass);
    let result = fc.read().unwrap();
    assert!(result.is_some(), "CLASS read should produce a FrameSet");
    if let Some(AnyObject::FrameSet(ref fs)) = result {
        // 3D header: spectral + 2 spatial.
        assert_eq!(fs.nframe(), 2);
        assert_eq!(fs.current().naxes(), 3);
    } else {
        panic!("Expected FrameSet, got {:?}", result);
    }
}

#[test]
fn read_dss_produces_frameset() {
    let text = load_fixture("dss.dss");
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::Dss);
    let result = fc.read().unwrap();
    assert!(result.is_some(), "DSS read should produce a FrameSet");
    if let Some(AnyObject::FrameSet(ref fs)) = result {
        assert_eq!(fs.nframe(), 2);
        assert_eq!(fs.current().naxes(), 2);
    } else {
        panic!("Expected FrameSet, got {:?}", result);
    }
}

// ===========================================================================
// 3. Verify FrameSet structure
// ===========================================================================

#[test]
fn iraf_frameset_has_sky() {
    // IRAF header (serpens.head) should produce a 2D FrameSet with a SkyFrame.
    let text = load_fixture("serpens.head");
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsIraf);
    let obj = fc.read().unwrap().expect("should read IRAF header");
    if let AnyObject::FrameSet(ref fs) = obj {
        assert_eq!(
            fs.nframe(),
            2,
            "FrameSet should have 2 frames (pixel + sky)"
        );
        assert_eq!(fs.current().naxes(), 2, "Current frame should be 2D");
        // Current frame should be a SkyFrame with FK5 system.
        let sys = fs.current().system_string();
        assert!(sys.is_some(), "Current frame should have a system string");
        let sys_str = sys.unwrap();
        assert!(
            sys_str.contains("FK5"),
            "Expected FK5 system from RADECSYS=FK5, got '{sys_str}'"
        );
    } else {
        panic!("Expected FrameSet, got {:?}", obj);
    }
}

#[test]
fn pc_frameset_has_sky() {
    // PC header should produce a 2D FrameSet with sky coordinates.
    let text = load_fixture("pc-sparse.head");
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsPc);
    let obj = fc.read().unwrap().expect("should read PC header");
    if let AnyObject::FrameSet(ref fs) = obj {
        assert_eq!(
            fs.nframe(),
            2,
            "FrameSet should have 2 frames (pixel + sky)"
        );
        assert_eq!(fs.current().naxes(), 2, "Current frame should be 2D");
        let sys = fs.current().system_string();
        assert!(sys.is_some(), "Current frame should have a system string");
        let sys_str = sys.unwrap();
        assert!(
            sys_str.contains("FK5"),
            "Expected FK5 system from RADECSYS=FK5, got '{sys_str}'"
        );
    } else {
        panic!("Expected FrameSet, got {:?}", obj);
    }
}

#[test]
fn dss_frameset_has_sky() {
    // DSS should produce a 2D FrameSet with FK5 coordinates.
    let text = load_fixture("dss.dss");
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::Dss);
    let obj = fc.read().unwrap().expect("should read DSS header");
    if let AnyObject::FrameSet(ref fs) = obj {
        assert_eq!(
            fs.nframe(),
            2,
            "FrameSet should have 2 frames (pixel + sky)"
        );
        assert_eq!(fs.current().naxes(), 2, "Current frame should be 2D");
        let sys = fs.current().system_string();
        assert!(sys.is_some(), "Current frame should have a system string");
        let sys_str = sys.unwrap();
        assert!(
            sys_str.contains("FK5"),
            "DSS plates use FK5; got '{sys_str}'"
        );
    } else {
        panic!("Expected FrameSet, got {:?}", obj);
    }
}

// ===========================================================================
// 4. -LOG spectral axis: LogWcs MathMap
// ===========================================================================

/// Verify that a FITS-WCS FREQ-LOG header builds a logarithmic IWC→S mapping,
/// matching the C oracle (fitschan.c LogWcs). The inverse transform takes
/// spectral frequency S back to pixel coordinate; with a linear ShiftMap the
/// result would be wrong (~1.4e9 range error).
///
/// Oracle goldens from `crates/ast-core/tests/fixtures/oracle/headers.oracle`
/// section `[freq-log.head  nin=1 nout=1 dir=inverse]`.
#[test]
#[allow(clippy::excessive_precision)]
fn freq_log_builds_log_mapping() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/freq-log.head"
    ))
    .unwrap();
    let mut fc = FitsChan::from_header(&text).unwrap();
    let obj = fc.read().unwrap().expect("freq-log.head should read");
    let AnyObject::FrameSet(ref fs) = obj else {
        panic!("Expected FrameSet, got {:?}", obj);
    };

    // The FrameSet acts as a Mapping: forward = pixel→spectral, inverse =
    // spectral→pixel. Run the inverse using the C oracle golden pairs from
    // headers.oracle section [freq-log.head nin=1 nout=1 dir=inverse].
    let s_inputs: &[f64] = &[
        1420399999.4884999,
        1420400000.5114999,
        1420399999.4884999,
        1420400000.0,
        1420400000.0,
        1420399999.7442501,
        1420400000.2557499,
        1420399999.616375,
        1420400000.1278751,
        1420399999.8721251,
        1420400000.383625,
        1420399999.5524375,
        1420400000.0639377,
        1420399999.8081875,
        1420400000.3196874,
        1420399999.6803126,
        1420400000.1918123,
        1420399999.9360623,
        1420400000.4475627,
        1420399999.5204687,
        1420400000.0319688,
        1420399999.7762187,
        1420400000.2877188,
        1420399999.6483438,
    ];
    // C oracle pixel outputs (exact C output; do not modify).
    let expected_pixels: &[f64] = &[
        0.99983848147604526,
        1023.999845942171,
        0.99983848147604526,
        512.5,
        512.5,
        256.75007695984095,
        768.24992299410997,
        128.87495772641466,
        640.37511919888948,
        384.62519618175503,
        896.12504216997502,
        64.937555801462807,
        576.43755960088379,
        320.68747887615859,
        832.1874825834816,
        192.8126750406453,
        704.31236340186047,
        448.5622827001597,
        960.06260175359046,
        32.968775989868405,
        544.46877980080171,
        288.71869907032033,
        800.21870278915549,
        160.84373753585044,
    ];

    // Tolerance: pixel coordinates are O(1)–O(1000), so 1e-9 relative is safe.
    let rtol = 1e-9_f64;
    let atol = 1e-9_f64;

    let rows: Vec<Vec<f64>> = s_inputs.iter().map(|&s| vec![s]).collect();
    let pts = PointSet::from_rows(rows);
    // false = inverse direction (world→pixel).
    let out = fs.transform(&pts, false).expect("inverse transform");

    for (i, (&expected, &s)) in expected_pixels.iter().zip(s_inputs.iter()).enumerate() {
        let got = out.point(i)[0];
        let tol = atol + rtol * expected.abs();
        assert!(
            (got - expected).abs() <= tol,
            "point {i}: S={s} → pixel got={got}, expected={expected}, diff={diff:.3e}",
            diff = (got - expected).abs(),
        );
    }
}

// ===========================================================================
// 5. -GRI/-GRA grism spectral axis: GrismMap-based mapping
// ===========================================================================

/// Verify that a FITS-WCS WAVE-GRI header builds a GrismMap-based IWC→S
/// mapping, matching the C oracle (fitschan.c GrismSpecWcs). The inverse
/// transform takes spectral wavelength back to pixel coordinate; with a
/// linear ShiftMap the result would be wrong.
///
/// Oracle goldens from `crates/ast-core/tests/fixtures/oracle/headers.oracle`
/// section `[grism.head  nin=1 nout=1 dir=inverse]`.
#[test]
#[allow(clippy::excessive_precision)]
fn grism_builds_grism_mapping() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/grism.head"
    ))
    .unwrap();
    let mut fc = FitsChan::from_header(&text).unwrap();
    let obj = fc.read().unwrap().expect("grism.head should read");
    let AnyObject::FrameSet(ref fs) = obj else {
        panic!("Expected FrameSet, got {:?}", obj);
    };

    // The FrameSet acts as a Mapping: forward = pixel→spectral, inverse =
    // spectral→pixel. Run the inverse using the C oracle golden pairs from
    // headers.oracle section [grism.head nin=1 nout=1 dir=inverse].
    let s_inputs: &[f64] = &[
        5.0009999999999998e-07,
        5.0999999999999999e-07,
        5.0009999999999998e-07,
        5.0505000000000004e-07,
        5.0505000000000004e-07,
        5.0257500000000001e-07,
        5.0752500000000007e-07,
        5.0133749999999995e-07,
        5.062875e-07,
        5.0381249999999997e-07,
        5.0876250000000003e-07,
        5.0071874999999997e-07,
        5.0566875000000002e-07,
        5.0319374999999999e-07,
        5.0814375000000005e-07,
        5.0195625000000003e-07,
        5.0690624999999998e-07,
        5.0443124999999995e-07,
        5.0938125000000001e-07,
        5.0040937500000003e-07,
        5.0535937499999998e-07,
        5.0288437499999995e-07,
        5.07834375e-07,
        5.0164687499999999e-07,
    ];
    // C oracle pixel outputs (exact C output; do not modify).
    let expected_pixels: &[f64] = &[
        0.99999999999969269,
        99.999999999999559,
        0.99999999999969269,
        50.499999999999964,
        50.499999999999964,
        25.749999999999829,
        75.250000000000767,
        13.374999999999087,
        62.875000000000036,
        38.124999999999893,
        87.625000000000171,
        7.1874999999997264,
        56.6875,
        31.937499999999861,
        81.437500000000128,
        19.562500000000469,
        69.062499999999389,
        44.312499999999261,
        93.812500000000199,
        4.0937500000003828,
        53.593749999999979,
        28.843749999999172,
        78.343750000000114,
        16.468749999999776,
    ];

    let rtol = 1e-9_f64;
    let atol = 1e-9_f64;

    let rows: Vec<Vec<f64>> = s_inputs.iter().map(|&s| vec![s]).collect();
    let pts = PointSet::from_rows(rows);
    // false = inverse direction (world→pixel).
    let out = fs.transform(&pts, false).expect("inverse transform");

    for (i, (&expected, &s)) in expected_pixels.iter().zip(s_inputs.iter()).enumerate() {
        let got = out.point(i)[0];
        let tol = atol + rtol * expected.abs();
        assert!(
            (got - expected).abs() <= tol,
            "point {i}: S={s} → pixel got={got}, expected={expected}, diff={diff:.3e}",
            diff = (got - expected).abs(),
        );
    }
}
