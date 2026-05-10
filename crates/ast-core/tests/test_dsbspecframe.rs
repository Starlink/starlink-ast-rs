//! DSBSpecFrame integration tests.
//!
//! Covers: construction, attribute defaults, sideband derivation from IF sign,
//! explicit sideband override, Frame trait basics, convert/alignment tests,
//! and serialization round-trips.

use ast_core::coords::PointSet;
use ast_core::frames::{DSBSpecFrame, Frame, FrameRef, SideBand, SpecSystem, StdOfRest};
use ast_core::{AnyObject, from_native_string, to_native_string};

#[test]
fn construction_defaults() {
    let dsb = DSBSpecFrame::new(SpecSystem::Freq);
    assert_eq!(dsb.system(), SpecSystem::Freq);
    assert_eq!(dsb.dsb_centre(), 3.0e10);
    assert_eq!(dsb.ifr(), 4.0e9);
    assert_eq!(dsb.sideband(), SideBand::LSB);
    assert!(!dsb.align_sideband());
}

#[test]
fn sideband_default_from_if_sign() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_ifr(4.0e9);
    assert_eq!(dsb.sideband(), SideBand::LSB);
    dsb.set_ifr(-4.0e9);
    assert_eq!(dsb.sideband(), SideBand::USB);
    dsb.set_ifr(0.0);
    assert_eq!(dsb.sideband(), SideBand::USB);
}

#[test]
fn explicit_sideband_overrides_default() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_ifr(4.0e9);
    dsb.set_sideband(SideBand::USB);
    assert_eq!(dsb.sideband(), SideBand::USB);
}

#[test]
fn set_dsb_centre() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_dsb_centre(1.0e11);
    assert_eq!(dsb.dsb_centre(), 1.0e11);
}

#[test]
fn set_align_sideband() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    assert!(!dsb.align_sideband());
    dsb.set_align_sideband(true);
    assert!(dsb.align_sideband());
}

#[test]
fn sideband_enum_parse_and_str() {
    assert_eq!(SideBand::parse("USB"), Some(SideBand::USB));
    assert_eq!(SideBand::parse("lsb"), Some(SideBand::LSB));
    assert_eq!(SideBand::parse("LO"), Some(SideBand::LO));
    assert_eq!(SideBand::parse("bogus"), None);

    assert_eq!(SideBand::USB.as_str(), "USB");
    assert_eq!(SideBand::LSB.as_str(), "LSB");
    assert_eq!(SideBand::LO.as_str(), "LO");
}

#[test]
fn specframe_delegation() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Vradio);
    assert_eq!(dsb.system(), SpecSystem::Vradio);
    dsb.set_system(SpecSystem::Freq);
    assert_eq!(dsb.system(), SpecSystem::Freq);

    dsb.set_rest_freq(1.42e9);
    assert_eq!(dsb.spec().rest_freq(), Some(1.42e9));
}

#[test]
fn observed_sideband_usb() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_sideband(SideBand::USB);
    dsb.set_dsb_centre(1.0e11);
    dsb.set_ifr(4.0e9);
    // observed = dsb_centre + ifr = 1.04e11
    assert_eq!(dsb.observed_freq(), 1.04e11);
}

#[test]
fn observed_sideband_lsb() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_sideband(SideBand::LSB);
    dsb.set_dsb_centre(1.0e11);
    dsb.set_ifr(4.0e9);
    // observed = dsb_centre - ifr = 9.6e10
    assert_eq!(dsb.observed_freq(), 9.6e10);
}

#[test]
fn observed_sideband_lo() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_sideband(SideBand::LO);
    dsb.set_dsb_centre(1.0e11);
    dsb.set_ifr(4.0e9);
    // LO = dsb_centre - ifr (= lower sideband frequency)
    assert_eq!(dsb.observed_freq(), 9.6e10);
}

#[test]
fn lo_frequency() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_dsb_centre(220.0e9);
    dsb.set_ifr(4.0e9);
    assert_eq!(dsb.lo_freq().unwrap(), 224.0e9);
}

#[test]
fn sideband_flip_usb_to_lsb() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_dsb_centre(220.0e9);
    dsb.set_ifr(4.0e9);
    dsb.set_sideband(SideBand::USB);
    dsb.set_std_of_rest(StdOfRest::Topocentric);

    let mapping = dsb.to_lsb_mapping().unwrap();

    let f_usb = 225.0e9;
    let f_lo = 224.0e9;
    let expected_lsb = 2.0 * f_lo - f_usb; // 223 GHz

    let input = PointSet::from_columns(&[&[f_usb]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    let result = output.point(0)[0];
    assert!(
        (result - expected_lsb).abs() < 1.0,
        "expected {expected_lsb}, got {result}"
    );
}

#[test]
fn sideband_flip_lsb_to_usb() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_dsb_centre(220.0e9);
    dsb.set_ifr(4.0e9);
    dsb.set_sideband(SideBand::LSB);
    dsb.set_std_of_rest(StdOfRest::Topocentric);

    let mapping = dsb.to_usb_mapping().unwrap();

    let f_lsb = 219.0e9;
    let f_lo = 224.0e9;
    let expected_usb = 2.0 * f_lo - f_lsb; // 229 GHz

    let input = PointSet::from_columns(&[&[f_lsb]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    let result = output.point(0)[0];
    assert!(
        (result - expected_usb).abs() < 1.0,
        "expected {expected_usb}, got {result}"
    );
}

#[test]
fn sideband_identity_when_already_target() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_dsb_centre(220.0e9);
    dsb.set_ifr(4.0e9);
    dsb.set_sideband(SideBand::USB);
    dsb.set_std_of_rest(StdOfRest::Topocentric);

    let mapping = dsb.to_usb_mapping().unwrap();
    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    assert!((output.point(0)[0] - 225.0e9).abs() < 1.0);
}

#[test]
fn lo_offset_mapping() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_dsb_centre(220.0e9);
    dsb.set_ifr(4.0e9);
    dsb.set_sideband(SideBand::USB);
    dsb.set_std_of_rest(StdOfRest::Topocentric);

    let mapping = dsb.to_lo_mapping().unwrap();

    // USB freq 225 GHz, LO = 224 GHz -> offset = 225 - 224 = 1 GHz
    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    let expected = 1.0e9;
    assert!(
        (output.point(0)[0] - expected).abs() < 1.0,
        "expected {expected}, got {}",
        output.point(0)[0]
    );
}

// ---------------------------------------------------------------------------
// Frame trait tests
// ---------------------------------------------------------------------------

#[test]
fn frame_trait_basics() {
    let dsb = DSBSpecFrame::new(SpecSystem::Freq);
    assert_eq!(dsb.naxes(), 1);
}

#[test]
fn convert_dsb_to_dsb_same_sideband() {
    let mut from = DSBSpecFrame::new(SpecSystem::Freq);
    from.set_std_of_rest(StdOfRest::Topocentric);
    from.set_dsb_centre(220.0e9);
    from.set_ifr(4.0e9);
    from.set_sideband(SideBand::USB);

    let mut to = DSBSpecFrame::new(SpecSystem::Freq);
    to.set_std_of_rest(StdOfRest::Topocentric);
    to.set_dsb_centre(220.0e9);
    to.set_ifr(4.0e9);
    to.set_sideband(SideBand::USB);

    let fs = from.convert(&to, &[]).unwrap().unwrap();
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    assert!((output.point(0)[0] - 225.0e9).abs() < 1.0);
}

#[test]
fn convert_dsb_to_dsb_different_sideband_with_align() {
    let mut from = DSBSpecFrame::new(SpecSystem::Freq);
    from.set_std_of_rest(StdOfRest::Topocentric);
    from.set_dsb_centre(220.0e9);
    from.set_ifr(4.0e9);
    from.set_sideband(SideBand::USB);
    from.set_align_sideband(true);

    let mut to = DSBSpecFrame::new(SpecSystem::Freq);
    to.set_std_of_rest(StdOfRest::Topocentric);
    to.set_dsb_centre(220.0e9);
    to.set_ifr(4.0e9);
    to.set_sideband(SideBand::LSB);
    to.set_align_sideband(true);

    let fs = from.convert(&to, &[]).unwrap().unwrap();
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();

    // USB 225 GHz -> LSB: f_lo=224, f_lsb = 2*224 - 225 = 223 GHz
    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    let expected = 223.0e9;
    assert!(
        (output.point(0)[0] - expected).abs() < 1.0,
        "expected {expected}, got {}",
        output.point(0)[0]
    );
}

#[test]
fn convert_dsb_to_dsb_different_sideband_no_align() {
    let mut from = DSBSpecFrame::new(SpecSystem::Freq);
    from.set_std_of_rest(StdOfRest::Topocentric);
    from.set_dsb_centre(220.0e9);
    from.set_ifr(4.0e9);
    from.set_sideband(SideBand::USB);

    let mut to = DSBSpecFrame::new(SpecSystem::Freq);
    to.set_std_of_rest(StdOfRest::Topocentric);
    to.set_dsb_centre(220.0e9);
    to.set_ifr(4.0e9);
    to.set_sideband(SideBand::LSB);

    let fs = from.convert(&to, &[]).unwrap().unwrap();
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();

    // Without AlignSideBand, value passes through unchanged.
    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    assert!((output.point(0)[0] - 225.0e9).abs() < 1.0);
}

#[test]
fn convert_dsb_to_specframe() {
    let mut from = DSBSpecFrame::new(SpecSystem::Freq);
    from.set_std_of_rest(StdOfRest::Topocentric);
    from.set_dsb_centre(220.0e9);
    from.set_ifr(4.0e9);
    from.set_sideband(SideBand::USB);

    let mut to = ast_core::frames::SpecFrame::new(SpecSystem::Freq);
    to.set_std_of_rest(StdOfRest::Topocentric);

    let fs = from.convert(&to, &[]).unwrap().unwrap();
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    assert!((output.point(0)[0] - 225.0e9).abs() < 1.0);
}

// ---------------------------------------------------------------------------
// ImagFreq computation tests
// ---------------------------------------------------------------------------

#[test]
fn imag_freq_computation() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_std_of_rest(StdOfRest::Topocentric);
    dsb.set_dsb_centre(220.0e9);
    dsb.set_ifr(4.0e9);
    dsb.set_sideband(SideBand::USB);
    dsb.set_rest_freq(225.0e9);

    // ImagFreq = mirror of rest_freq across LO.
    // f_lo = 224 GHz, imag = 2*224 - 225 = 223 GHz.
    let imag = dsb.imag_freq().unwrap();
    assert!((imag - 223.0e9).abs() < 1.0, "expected 223 GHz, got {imag}");
}

#[test]
fn imag_freq_no_rest_freq() {
    let dsb = DSBSpecFrame::new(SpecSystem::Freq);
    assert!(dsb.imag_freq().is_err());
}

// ---------------------------------------------------------------------------
// Serialization tests
// ---------------------------------------------------------------------------

#[test]
fn serialization_round_trip() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_std_of_rest(StdOfRest::Topocentric);
    dsb.set_dsb_centre(220.463127593878e9);
    dsb.set_ifr(-3.99997957289606e9);
    dsb.set_sideband(SideBand::USB);
    dsb.set_rest_freq(220398676500.0);

    let obj = AnyObject::DSBSpecFrame(dsb.clone());
    let text = to_native_string(&obj).unwrap();

    assert!(text.contains("Begin DSBSpecFrame"));
    assert!(text.contains("IsA SpecFrame"));
    assert!(text.contains("DSBCen"));
    assert!(text.contains("IF =") || text.contains("IF="));

    let obj2 = from_native_string(&text).unwrap();
    let dsb2 = match obj2 {
        AnyObject::DSBSpecFrame(d) => d,
        other => panic!("expected DSBSpecFrame, got {other:?}"),
    };

    assert!((dsb2.dsb_centre() - dsb.dsb_centre()).abs() < 1.0);
    assert!((dsb2.ifr() - dsb.ifr()).abs() < 1.0);
    assert_eq!(dsb2.sideband(), SideBand::USB);
}

#[test]
fn deserialize_fixture() {
    let fixture = include_str!("fixtures/jan-dsb.ast");
    let obj = from_native_string(fixture).unwrap();
    let dsb = match obj {
        AnyObject::DSBSpecFrame(d) => d,
        other => panic!("expected DSBSpecFrame, got {other:?}"),
    };
    assert_eq!(dsb.sideband(), SideBand::USB);
    assert!((dsb.dsb_centre() - 220463127593.87817).abs() < 1.0);
    assert!((dsb.ifr() - (-3999979572.8960571)).abs() < 1.0);
}
