//! FitsChan DTAI/DUT1 (time-offset attribute) integration tests.
//!
//! Covers the `WcsMapFrm` DUT1/DTAI stamping (fitschan.c:37943-37952) onto
//! the top-level world Frame, and the TELESCOP gate on reading DTAI at all
//! (fitschan.c:36631-36643): DTAI is only honoured when a TELESCOP card is
//! present and names JCMT or UKIRT.
//!
//! The header mixes a spectral (FREQ) axis with the celestial (RA/Dec) pair
//! so the top-level world Frame is a CmpFrame rather than a bare SkyFrame.
//! C stamps the offsets in two places from two different call sites — on the
//! SkyFrame in `WcsSkyFrame` and on the top-level Frame in `WcsMapFrm` — and
//! only this compound shape distinguishes the two.

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::{Encoding, FitsChan};

/// 3-axis (FREQ, RA, Dec) header, parameterised by an optional TELESCOP
/// card and a DTAI value.
fn header_with_telescop(telescop: Option<&str>, dtai: Option<f64>) -> String {
    let mut text = String::from(
        "\
NAXIS1  =                  100 / Spectral axis
NAXIS2  =                  100 / Spatial axis
NAXIS3  =                  100 / Spatial axis
CTYPE1  = 'FREQ    '           / Frequency axis
CTYPE2  = 'RA---TAN'           / Right Ascension
CTYPE3  = 'DEC--TAN'           / Declination
CRVAL1  =           3.4500E+11 / Reference frequency (Hz)
CRVAL2  =               83.633 / Reference RA (deg)
CRVAL3  =               22.014 / Reference Dec (deg)
CRPIX1  =                 50.0 / Reference pixel
CRPIX2  =                 50.5 / Reference pixel
CRPIX3  =                 50.5 / Reference pixel
CDELT1  =           6.1035E+04 / Frequency increment (Hz)
CDELT2  =           -0.0002778 / Degrees/pixel
CDELT3  =            0.0002778 / Degrees/pixel
CUNIT1  = 'Hz      '           / Frequency units
CUNIT2  = 'deg     '           / Angular units
CUNIT3  = 'deg     '           / Angular units
RADESYS = 'FK5     '           / Reference frame
EQUINOX =               2000.0 / Equinox
SPECSYS = 'LSRK    '           / Spectral reference frame
RESTFRQ =           3.4506E+11 / Rest frequency (Hz)
LONPOLE =              180.000 / Native longitude of celestial pole
LATPOLE =               22.014 / Native latitude of celestial pole
DUT1    =             0.001    / UT1-UTC in days
",
    );
    if let Some(t) = telescop {
        text.push_str(&format!("TELESCOP= '{t:<8}'           / Telescope name\n"));
    }
    if let Some(d) = dtai {
        text.push_str(&format!(
            "DTAI    =           {d:.3}    / TAI-UTC in seconds\n"
        ));
    }
    text
}

fn read_top_frame(text: &str) -> ast_core::frames::DynFrame {
    let mut fc = FitsChan::from_header(text).unwrap();
    let obj = fc.read().unwrap().expect("read should produce a FrameSet");
    let fs = match obj {
        AnyObject::FrameSet(fs) => fs,
        other => panic!("expected FrameSet, got {other:?}"),
    };
    fs.current().snapshot()
}

/// Read `text` into a FrameSet and Native-dump the current (world) Frame.
///
/// The `dtai()`/`dut1()` getters on a CmpFrame fall back to the nested
/// child frame's value (mirroring C `GetDtai` — see
/// `crates/ast-core/src/frames/cmpframe.rs`), so a `Some` result from
/// those getters alone does not prove the *outer* Frame carries the
/// attribute itself. The Native dump only writes a `Dtai`/`Dut1` card for
/// a CmpFrame when the value is set directly on the compound frame
/// (mirroring C, which gates the Frame::Dump write on the class's own
/// Test{Dtai,Dut1} rather than on the Get{Dtai,Dut1} fallback value), so the
/// dump is what distinguishes a stamp on the top-level Frame from one on the
/// nested SkyFrame.
fn read_top_frame_native_dump(text: &str) -> String {
    let mut fc = FitsChan::from_header(text).unwrap();
    let obj = fc.read().unwrap().expect("read should produce a FrameSet");
    let mut out_fc = FitsChan::new();
    out_fc.set_encoding(Encoding::Native);
    out_fc.write(&obj).expect("native write should succeed");
    out_fc.to_header().unwrap()
}

#[test]
fn dtai_set_on_world_frame_for_jcmt() {
    let text = header_with_telescop(Some("JCMT"), Some(37.0));
    let frame = read_top_frame(&text);
    assert!(frame.test_dtai(), "Dtai should be set for a JCMT header");
    let dtai = frame.dtai();
    assert!((dtai - 37.0).abs() < 1e-9, "expected Dtai=37.0, got {dtai}");

    // The top-level Frame is a CmpFrame (FREQ + RA/Dec). Confirm Dtai is
    // stamped on the CmpFrame itself, not merely inherited by fallback
    // from the nested SkyFrame — the Native dump emits a `DTAI_<n>` card
    // (the Native encoder's disambiguating suffix on the `Dtai` keyword)
    // right after the CmpFrame's `BEGAST` line only when the compound
    // frame carries the attribute directly; without it, `DTAI_<n>` first
    // appears only inside the nested SkyFrame's block.
    let dump = read_top_frame_native_dump(&text);
    let begast_cmpframe = dump
        .lines()
        .position(|l| l.contains("'CmpFrame'"))
        .expect("dump should contain a CmpFrame block");
    let first_dtai = dump
        .lines()
        .position(|l| l.starts_with("DTAI_"))
        .expect("dump should contain a DTAI card somewhere");
    assert!(
        first_dtai < begast_cmpframe + 5,
        "expected the CmpFrame's own DTAI card near line {begast_cmpframe}, \
         but the first DTAI card is at line {first_dtai}:\n{dump}"
    );
}

#[test]
fn dtai_set_on_world_frame_for_ukirt() {
    let text = header_with_telescop(Some("UKIRT"), Some(37.0));
    let frame = read_top_frame(&text);
    assert!(frame.test_dtai(), "Dtai should be set for a UKIRT header");
    let dtai = frame.dtai();
    assert!((dtai - 37.0).abs() < 1e-9, "expected Dtai=37.0, got {dtai}");
}

#[test]
fn dtai_ignored_for_other_telescope() {
    let text = header_with_telescop(Some("HST"), Some(37.0));
    let frame = read_top_frame(&text);
    assert!(
        !frame.test_dtai(),
        "Dtai should not be set when TELESCOP names a telescope other than JCMT/UKIRT"
    );
}

#[test]
fn dtai_ignored_when_telescop_absent() {
    let text = header_with_telescop(None, Some(37.0));
    let frame = read_top_frame(&text);
    assert!(
        !frame.test_dtai(),
        "Dtai should not be set when there is no TELESCOP card at all"
    );
}

#[test]
fn dut1_set_on_world_frame_regardless_of_telescop() {
    // DUT1 has no TELESCOP gate in C: it is set on the world Frame
    // unconditionally when present, converted from FITS days to AST
    // seconds (fitschan.c:37943-37948, SPD = 86400.0).
    let text = header_with_telescop(None, None);
    let frame = read_top_frame(&text);
    assert!(
        frame.test_dut1(),
        "Dut1 should be set regardless of TELESCOP"
    );
    let dut1 = frame.dut1();
    assert!(
        (dut1 - 0.001 * 86_400.0).abs() < 1e-6,
        "expected Dut1=86.4s, got {dut1}"
    );

    // As with Dtai above, confirm the CmpFrame itself carries the
    // attribute (not merely a fallback from the nested SkyFrame): the
    // dump's first `DUT1_<n>` card must be near the CmpFrame's own
    // `BEGAST` line, not first appearing inside the nested SkyFrame.
    let dump = read_top_frame_native_dump(&text);
    let begast_cmpframe = dump
        .lines()
        .position(|l| l.contains("'CmpFrame'"))
        .expect("dump should contain a CmpFrame block");
    let first_dut1 = dump
        .lines()
        .position(|l| l.starts_with("DUT1_"))
        .expect("dump should contain a DUT1 card somewhere");
    assert!(
        first_dut1 < begast_cmpframe + 5,
        "expected the CmpFrame's own DUT1 card near line {begast_cmpframe}, \
         but the first DUT1 card is at line {first_dut1}:\n{dump}"
    );
}
