//! DSBSpecFrame-on-read tests for the FITS-WCS / FITS-CLASS decoder.
//!
//! When a FITS header carries an `IMAGFREQ` keyword, the spectral axis is
//! decoded as a dual-sideband [`DSBSpecFrame`] rather than a plain
//! [`SpecFrame`], and `DSBSetUp` (a port of C `fitschan.c:8930`) computes its
//! IF / DSBCentre / SideBand attributes. Mirrors C `WcsSpectral`
//! (fitschan.c:39485-39590).
//!
//! The expected IF / DSBCentre / SideBand values below were dumped from the C
//! oracle (libast.so) reading the same fixture: a probe built against the
//! reference library calls `astRead`, `astPickAxes`, then prints
//! `astGetC(pick, "DSBCentre"/"IF"/"SideBand")`.

use ast_core::AnyObject;
use ast_core::Channel;
use ast_core::fitschan::{Encoding, FitsChan};
use ast_core::frames::{CmpFrame, DSBSpecFrame, Frame, SideBand};
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("wcsconv")
        .join("inputs")
}

/// Find the first `DSBSpecFrame` within a frame, recursing through any
/// `CmpFrame`.
fn find_dsb(frame: &dyn Frame) -> Option<DSBSpecFrame> {
    if let Some(d) = frame.as_any().downcast_ref::<DSBSpecFrame>() {
        return Some(d.clone());
    }
    if let Some(c) = frame.as_any().downcast_ref::<CmpFrame>() {
        return find_dsb(c.frame1()).or_else(|| find_dsb(c.frame2()));
    }
    None
}

/// Read a fixture header through a FITS-CLASS FitsChan and return the
/// DSBSpecFrame from the current world frame.
fn read_dsb(fixture: &str) -> DSBSpecFrame {
    let text = std::fs::read_to_string(fixtures_dir().join(fixture))
        .unwrap_or_else(|e| panic!("read {fixture}: {e}"));
    let mut fc = FitsChan::from_header(&text).unwrap();
    fc.set_encoding(Encoding::FitsClass);
    let obj = fc
        .read()
        .unwrap()
        .unwrap_or_else(|| panic!("{fixture}: read produced no object"));
    let AnyObject::FrameSet(fs) = obj else {
        panic!("{fixture}: expected a FrameSet");
    };
    find_dsb(fs.current()).unwrap_or_else(|| panic!("{fixture}: current frame has no DSBSpecFrame"))
}

#[test]
fn class_write_builds_dsbspecframe() {
    // class-write.head: CTYPE1=FREQ (world axis 1), IMAGFREQ present.
    let dsb = read_dsb("class-write.head");

    // Oracle (libast.so): IF = -6.4599532127580268 GHz,
    // DSBCentre(topo Hz) = 110201461951, SideBand = USB.
    assert_eq!(dsb.sideband(), SideBand::USB, "SideBand");

    let if_ghz = dsb.if_ghz();
    assert!(
        (if_ghz - (-6.459_953_212_758)).abs() < 1e-6,
        "IF GHz: got {if_ghz}, expected -6.4599532127580268"
    );

    // dsb_centre_cur() mirrors C `astGetC(dsb,"DSBCentre")`: the DSBCentre
    // expressed in the current spectral system/unit (source-rest FREQ Hz).
    // Oracle: 110201354300.00003 Hz.
    let centre = dsb.dsb_centre_cur().expect("dsb_centre_cur");
    assert!(
        (centre - 110_201_354_300.0).abs() < 1.0,
        "DSBCentre (current Hz): got {centre}, expected 110201354300"
    );
}

#[test]
fn jan_builds_dsbspecframe() {
    // jan.fits-class: CTYPE3=FREQ (world axis 3), IMAGFREQ present.
    let dsb = read_dsb("jan.fits-class");

    // Oracle (libast.so): IF = -3.9999795728960574 GHz,
    // DSBCentre(topo Hz) = 220463127594, SideBand = USB.
    assert_eq!(dsb.sideband(), SideBand::USB, "SideBand");

    let if_ghz = dsb.if_ghz();
    assert!(
        (if_ghz - (-3.999_979_572_896)).abs() < 1e-6,
        "IF GHz: got {if_ghz}, expected -3.9999795728960574"
    );

    // Oracle astGetC(dsb,"DSBCentre") = 220398648224.8721 Hz (source FREQ).
    let centre = dsb.dsb_centre_cur().expect("dsb_centre_cur");
    assert!(
        (centre - 220_398_648_224.0).abs() < 10.0,
        "DSBCentre (current Hz): got {centre}, expected 220398648224.8721"
    );
}
