//! DSBSpecFrame integration tests.
//!
//! Covers: construction, attribute defaults, sideband derivation from IF sign,
//! explicit sideband override, Frame trait basics, convert/alignment tests,
//! and serialization round-trips.

use ast_core::coords::PointSet;
use ast_core::frames::{DSBSpecFrame, Frame, FrameRef, FrameSet, SideBand, SpecSystem, StdOfRest};
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
    dsb.set_system(SpecSystem::Freq).unwrap();
    assert_eq!(dsb.system(), SpecSystem::Freq);

    dsb.set_rest_freq(1.42e9);
    assert_eq!(dsb.spec().rest_freq(), 1.42e9);
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
    // The sideband mappings are expressed in the frame's current Unit
    // (the topocentric flip happens in Hz internally). Use Hz so the Hz
    // test inputs/outputs are in the frame's own unit — matching how the
    // FITS reader builds a DSBSpecFrame (CUNIT = Hz).
    dsb.spec_mut().set_unit(1, "Hz").unwrap();
    dsb.set_dsb_centre(220.0e9);
    dsb.set_ifr(4.0e9);
    dsb.set_sideband(SideBand::USB);
    dsb.set_std_of_rest(StdOfRest::Topocentric).unwrap();

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
    dsb.spec_mut().set_unit(1, "Hz").unwrap();
    dsb.set_dsb_centre(220.0e9);
    dsb.set_ifr(4.0e9);
    dsb.set_sideband(SideBand::LSB);
    dsb.set_std_of_rest(StdOfRest::Topocentric).unwrap();

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
    dsb.set_std_of_rest(StdOfRest::Topocentric).unwrap();

    let mapping = dsb.to_usb_mapping().unwrap();
    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    assert!((output.point(0)[0] - 225.0e9).abs() < 1.0);
}

#[test]
fn lo_offset_mapping() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.spec_mut().set_unit(1, "Hz").unwrap();
    dsb.set_dsb_centre(220.0e9);
    dsb.set_ifr(4.0e9);
    dsb.set_sideband(SideBand::USB);
    dsb.set_std_of_rest(StdOfRest::Topocentric).unwrap();

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

/// C `GetDomain` (dsbspecframe.c:531) supplies "DSBSPECTRUM" only as a
/// *default* — TestDomain stays false until Domain is explicitly set.
/// Oracle-verified against the built reference libast.
#[test]
fn domain_default_is_unset() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    assert_eq!(dsb.domain(), "DSBSPECTRUM");
    assert!(!dsb.test_domain());

    dsb.set_domain("FOO").unwrap();
    assert_eq!(dsb.domain(), "FOO");
    assert!(dsb.test_domain());
}

/// DSBSpecFrame does not override Unformat in C (dsbspecframe.c has no
/// `static int Unformat`); it inherits the base SpecFrame/Frame/Axis contract
/// (`astAxisUnformat`, axis.c:1284). Oracle-verified against the built
/// reference libast.
#[test]
fn unformat_base_contract() {
    use ast_core::constants::BAD_VALUE;
    let dsb = DSBSpecFrame::new(SpecSystem::Freq);
    assert_eq!(dsb.unformat(1, "  12.5  ").unwrap(), (12.5, 8));
    assert_eq!(dsb.unformat(1, "<bad>").unwrap(), (BAD_VALUE, 5));
    assert_eq!(dsb.unformat(1, " < b a d > ").unwrap(), (BAD_VALUE, 11));
    assert_eq!(dsb.unformat(1, "12.5xyz").unwrap(), (12.5, 4));
    assert_eq!(dsb.unformat(1, "xyz").unwrap(), (BAD_VALUE, 0));
}

#[test]
fn convert_dsb_to_dsb_same_sideband() {
    let mut from = DSBSpecFrame::new(SpecSystem::Freq);
    from.set_std_of_rest(StdOfRest::Topocentric).unwrap();
    from.set_dsb_centre(220.0e9);
    from.set_ifr(4.0e9);
    from.set_sideband(SideBand::USB);

    let mut to = DSBSpecFrame::new(SpecSystem::Freq);
    to.set_std_of_rest(StdOfRest::Topocentric).unwrap();
    to.set_dsb_centre(220.0e9);
    to.set_ifr(4.0e9);
    to.set_sideband(SideBand::USB);

    let fs = from.convert_to(&to, &[]).unwrap().unwrap();
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    assert!((output.point(0)[0] - 225.0e9).abs() < 1.0);
}

#[test]
fn convert_dsb_to_dsb_different_sideband_with_align() {
    let mut from = DSBSpecFrame::new(SpecSystem::Freq);
    from.spec_mut().set_unit(1, "Hz").unwrap();
    from.set_std_of_rest(StdOfRest::Topocentric).unwrap();
    from.set_dsb_centre(220.0e9);
    from.set_ifr(4.0e9);
    from.set_sideband(SideBand::USB);
    from.set_align_sideband(true);

    let mut to = DSBSpecFrame::new(SpecSystem::Freq);
    to.spec_mut().set_unit(1, "Hz").unwrap();
    to.set_std_of_rest(StdOfRest::Topocentric).unwrap();
    to.set_dsb_centre(220.0e9);
    to.set_ifr(4.0e9);
    to.set_sideband(SideBand::LSB);
    to.set_align_sideband(true);

    let fs = from.convert_to(&to, &[]).unwrap().unwrap();
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
    from.set_std_of_rest(StdOfRest::Topocentric).unwrap();
    from.set_dsb_centre(220.0e9);
    from.set_ifr(4.0e9);
    from.set_sideband(SideBand::USB);

    let mut to = DSBSpecFrame::new(SpecSystem::Freq);
    to.set_std_of_rest(StdOfRest::Topocentric).unwrap();
    to.set_dsb_centre(220.0e9);
    to.set_ifr(4.0e9);
    to.set_sideband(SideBand::LSB);

    let fs = from.convert_to(&to, &[]).unwrap().unwrap();
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();

    // Without AlignSideBand, value passes through unchanged.
    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    assert!((output.point(0)[0] - 225.0e9).abs() < 1.0);
}

#[test]
fn convert_dsb_to_specframe() {
    let mut from = DSBSpecFrame::new(SpecSystem::Freq);
    from.set_std_of_rest(StdOfRest::Topocentric).unwrap();
    from.set_dsb_centre(220.0e9);
    from.set_ifr(4.0e9);
    from.set_sideband(SideBand::USB);

    let mut to = ast_core::frames::SpecFrame::new(SpecSystem::Freq);
    to.set_std_of_rest(StdOfRest::Topocentric).unwrap();

    let fs = from.convert_to(&to, &[]).unwrap().unwrap();
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
    dsb.spec_mut().set_unit(1, "Hz").unwrap();
    dsb.set_std_of_rest(StdOfRest::Topocentric).unwrap();
    dsb.set_dsb_centre(220.0e9);
    dsb.set_ifr(4.0e9);
    dsb.set_sideband(SideBand::USB);
    dsb.set_rest_freq(225.0e9);

    // The rest frequency is interpreted in the source's rest frame
    // (mirrors C `astGetImagFreq`). Pin the source rest frame to be
    // identical to topocentric so the LO reflection yields exactly
    // the analytic answer with no Doppler-shift residue.
    dsb.spec_mut()
        .set_source_vrf(StdOfRest::Topocentric)
        .unwrap();
    dsb.spec_mut().set_source_vel(0.0);

    // ImagFreq = mirror of rest_freq across LO.
    // f_lo = 224 GHz, imag = 2*224 - 225 = 223 GHz.
    let imag = dsb.imag_freq().unwrap();
    assert!((imag - 223.0e9).abs() < 1.0, "expected 223 GHz, got {imag}");
}

/// With no RestFreq set, C uses its default, 1e14 Hz (dsbspecframe.c:673),
/// and reports the image frequency C's `ImagFreq` attribute gives.
#[test]
fn imag_freq_uses_the_default_rest_freq() {
    let dsb = DSBSpecFrame::unset_system();
    let imag = dsb.imag_freq_ghz().unwrap();
    assert!((imag - -99_931.993_345_375_96).abs() < 1e-9, "got {imag}");
}

// ---------------------------------------------------------------------------
// Serialization tests
// ---------------------------------------------------------------------------

#[test]
fn serialization_round_trip() {
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_std_of_rest(StdOfRest::Topocentric).unwrap();
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
    let fixture = include_str!("fixtures/serialisation/jan-dsb.ast");
    let obj = from_native_string(fixture).unwrap();
    let dsb = match obj {
        AnyObject::DSBSpecFrame(d) => d,
        other => panic!("expected DSBSpecFrame, got {other:?}"),
    };
    assert_eq!(dsb.sideband(), SideBand::USB);
    assert!((dsb.dsb_centre() - 220463127593.87817).abs() < 1.0);
    assert!((dsb.ifr() - (-3_999_979_572.896_057)).abs() < 1.0);
}

// ---------------------------------------------------------------------------
// FrameSet integrity and alignment tests
// ---------------------------------------------------------------------------

#[test]
fn frameset_integrity_sideband_change() {
    // When SideBand is changed through with_integrity, the internal
    // mapping should update to reflect the sideband flip.
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.spec_mut().set_unit(1, "Hz").unwrap();
    dsb.set_std_of_rest(StdOfRest::Topocentric).unwrap();
    dsb.set_dsb_centre(220.0e9);
    dsb.set_ifr(4.0e9);
    dsb.set_sideband(SideBand::USB);

    // Build a 2-frame FrameSet: base(USB) --identity--> current(USB).
    let base_frame: Box<dyn Frame> = Box::new(dsb.clone());
    let current_frame: Box<dyn Frame> = Box::new(dsb);
    let identity: Box<dyn ast_core::mappings::Mapping> =
        Box::new(ast_core::mappings::UnitMap::new(1));
    let mut fs = FrameSet::new(base_frame);
    fs.add_frame(FrameRef::Base, identity, current_frame)
        .unwrap();

    // Initially the mapping is identity.
    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let mapping_before = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let out_before = mapping_before.transform(&input, true).unwrap();
    assert!((out_before.point(0)[0] - 225.0e9).abs() < 1.0);

    // Change sideband through with_integrity.
    fs.with_integrity(|frame| {
        let dsb_ref = frame.as_any_mut().downcast_mut::<DSBSpecFrame>().unwrap();
        dsb_ref.set_sideband(SideBand::LSB);
        Ok(())
    })
    .unwrap();

    // After integrity restoration, mapping should include the sideband flip.
    // Base(USB) → Current(LSB): f_lsb = 2*f_lo - f_usb = 2*224e9 - 225e9 = 223e9
    let mapping_after = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let out_after = mapping_after.transform(&input, true).unwrap();
    let expected = 223.0e9;
    assert!(
        (out_after.point(0)[0] - expected).abs() < 1.0,
        "expected {expected}, got {}",
        out_after.point(0)[0]
    );
}

#[test]
fn alignment_through_frameset() {
    let mut dsb1 = DSBSpecFrame::new(SpecSystem::Freq);
    dsb1.spec_mut().set_unit(1, "Hz").unwrap();
    dsb1.set_std_of_rest(StdOfRest::Topocentric).unwrap();
    dsb1.set_dsb_centre(220.0e9);
    dsb1.set_ifr(4.0e9);
    dsb1.set_sideband(SideBand::USB);
    dsb1.set_align_sideband(true);

    let mut dsb2 = DSBSpecFrame::new(SpecSystem::Freq);
    dsb2.spec_mut().set_unit(1, "Hz").unwrap();
    dsb2.set_std_of_rest(StdOfRest::Topocentric).unwrap();
    dsb2.set_dsb_centre(220.0e9);
    dsb2.set_ifr(4.0e9);
    dsb2.set_sideband(SideBand::LSB);
    dsb2.set_align_sideband(true);

    let fs = dsb1.convert_to(&dsb2, &[]).unwrap().unwrap();
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();

    // USB 226 GHz → LSB: f_lo=224, f_lsb = 2*224-226 = 222 GHz
    let input = PointSet::from_columns(&[&[226.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    let expected = 222.0e9;
    assert!(
        (output.point(0)[0] - expected).abs() < 1.0,
        "expected {expected}, got {}",
        output.point(0)[0]
    );

    // Inverse: LSB 222 GHz → USB 226 GHz
    let inv_output = mapping.transform(&output, false).unwrap();
    assert!(
        (inv_output.point(0)[0] - 226.0e9).abs() < 1.0,
        "inverse expected 226e9, got {}",
        inv_output.point(0)[0]
    );
}

#[test]
fn alignment_dsb_to_dsb_different_lo() {
    // Two DSBSpecFrames with different LO frequencies.
    let mut dsb1 = DSBSpecFrame::new(SpecSystem::Freq);
    dsb1.spec_mut().set_unit(1, "Hz").unwrap();
    dsb1.set_std_of_rest(StdOfRest::Topocentric).unwrap();
    dsb1.set_dsb_centre(220.0e9);
    dsb1.set_ifr(4.0e9); // LO = 224 GHz
    dsb1.set_sideband(SideBand::USB);
    dsb1.set_align_sideband(true);

    let mut dsb2 = DSBSpecFrame::new(SpecSystem::Freq);
    dsb2.spec_mut().set_unit(1, "Hz").unwrap();
    dsb2.set_std_of_rest(StdOfRest::Topocentric).unwrap();
    dsb2.set_dsb_centre(230.0e9);
    dsb2.set_ifr(5.0e9); // LO = 235 GHz
    dsb2.set_sideband(SideBand::USB);
    dsb2.set_align_sideband(true);

    let fs = dsb1.convert_to(&dsb2, &[]).unwrap().unwrap();
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();

    // dsb1 USB 225 GHz:
    //   → dsb1 observed (LSB, since IF>0): 2*224-225 = 223 GHz
    //   → parent alignment (topo Freq → topo Freq): identity → 223 GHz
    //   → dsb2 observed (LSB, since IF>0) → dsb2 USB: 2*235-223 = 247 GHz
    let input = PointSet::from_columns(&[&[225.0e9]]).unwrap();
    let output = mapping.transform(&input, true).unwrap();
    let expected = 247.0e9;
    assert!(
        (output.point(0)[0] - expected).abs() < 1.0,
        "expected {expected}, got {}",
        output.point(0)[0]
    );
}

// ===========================================================================
// Current-system / GHz unit conversions for the DSBCentre and IF attributes.
// These mirror the C string-attribute layer (dsbspecframe.c:459-501,
// 1355-1370), which the ast-capi shim will call for astGetC/astSetC parity.
// Internal storage is topocentric Hz throughout.
// ===========================================================================

#[test]
fn if_ghz_conversions() {
    // C: astGetC("IF") reports the internal Hz value x 1e-9; astSetC("IF=g")
    // stores g x 1e9. Internal ifr() stays in Hz.
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_ifr(4.0e9);
    assert_eq!(dsb.if_ghz(), 4.0);
    dsb.set_if_ghz(6.0);
    assert_eq!(dsb.ifr(), 6.0e9);
    assert_eq!(dsb.if_ghz(), 6.0);
}

#[test]
fn dsb_centre_cur_roundtrips_topocentric() {
    // For a topocentric FREQ frame the current-system value equals the
    // internal topocentric-Hz value (the TopoMap is an identity on a
    // topocentric FREQ frame), so dsb_centre_cur round-trips set_dsb_centre.
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.spec_mut().set_unit(1, "Hz").unwrap();
    dsb.set_std_of_rest(StdOfRest::Topocentric).unwrap();
    dsb.set_dsb_centre(3.0e10);
    let cur = dsb.dsb_centre_cur().unwrap();
    assert!(
        (cur - 3.0e10).abs() < 1.0,
        "topocentric current-system DSBCentre should equal internal Hz, got {cur}"
    );

    // set_dsb_centre_cur is the inverse of dsb_centre_cur: setting the
    // current-system value then reading it back is a round-trip.
    let mut dsb2 = DSBSpecFrame::new(SpecSystem::Freq);
    dsb2.spec_mut().set_unit(1, "Hz").unwrap();
    dsb2.set_std_of_rest(StdOfRest::Topocentric).unwrap();
    dsb2.set_dsb_centre_cur(2.5e10).unwrap();
    assert!((dsb2.dsb_centre_cur().unwrap() - 2.5e10).abs() < 1.0);
    // Internal storage is topocentric Hz; for a topocentric frame it equals
    // the supplied current-system value.
    assert!((dsb2.dsb_centre() - 2.5e10).abs() < 1.0);
}

#[test]
fn dsb_centre_cur_differs_from_internal_for_nontopo_rest() {
    // For a non-topocentric rest frame the current-system value differs from
    // the internal topocentric-Hz storage (the velocity correction the C
    // oracle shows, e.g. 30 GHz set -> ~29.997 GHz read back). We only assert
    // the conversion is applied (values differ), since the exact shift
    // depends on the observer geometry.
    let mut dsb = DSBSpecFrame::new(SpecSystem::Freq);
    dsb.set_std_of_rest(StdOfRest::Lsrk).unwrap();
    dsb.set_obs_lat(0.3);
    dsb.set_obs_lon(-1.0);
    dsb.set_epoch_year(2000.0);
    dsb.spec_mut().set_ref_ra(1.2);
    dsb.spec_mut().set_ref_dec(0.5);
    dsb.set_dsb_centre(3.0e10);
    let cur = dsb.dsb_centre_cur().unwrap();
    assert!(cur.is_finite());
    assert!(
        (cur - 3.0e10).abs() > 1.0,
        "non-topocentric current-system DSBCentre should differ from internal Hz"
    );
}

/// The local oscillator frequency must be at least the size of the IF
/// (dsbspecframe.c `GetLO`), so a negative IF larger than the DSBCentre is
/// an error rather than a frequency.
#[test]
fn lo_below_the_if_is_an_error() {
    use ast_core::error::ErrorCode;
    use ast_core::frames::{DSBSpecFrame, SpecSystem};

    let mut f = DSBSpecFrame::new(SpecSystem::Freq);
    f.set_dsb_centre(5.0e9);
    f.set_ifr(-4.0e9);
    assert_eq!(f.lo_freq().unwrap_err().code(), ErrorCode::Attin);
    f.set_ifr(-2.0e9);
    assert_eq!(f.lo_freq().unwrap(), 3.0e9);
}
