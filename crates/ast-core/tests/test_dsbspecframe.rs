//! DSBSpecFrame integration tests.
//!
//! Covers: construction, attribute defaults, sideband derivation from IF sign,
//! explicit sideband override.

use ast_core::frames::{DSBSpecFrame, SideBand, SpecSystem};

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
