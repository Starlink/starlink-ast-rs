//! Integration tests for alternative FITS WCS encodings.
//!
//! Tests encoding auto-detection (guess) and read for all supported formats:
//! FITS-IRAF, FITS-PC, FITS-AIPS, FITS-CLASS, and DSS.
//!
//! Fixture files are copied from `reference/starlink-ast/ast_tester/`.

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::{Encoding, FitsChan};

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
    let mut fc = FitsChan::from_header(&text).unwrap();
    let enc = fc.guess_encoding();
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
    let mut fc = FitsChan::from_header(&text).unwrap();
    let enc = fc.guess_encoding();
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
    let mut fc = FitsChan::from_header(&text).unwrap();
    let enc = fc.guess_encoding();
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
    let mut fc = FitsChan::from_header(&text).unwrap();
    let enc = fc.guess_encoding();
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
    let mut fc = FitsChan::from_header(&text).unwrap();
    let enc = fc.guess_encoding();
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
