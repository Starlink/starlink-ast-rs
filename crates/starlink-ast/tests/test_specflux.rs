//! SpecFluxFrame: its SpecVal, conversions between flux and spectral
//! systems with permuted axes and logarithmic units, and equality.
//!
//! Ported from `reference/starlink-ast/ast_tester/testspecflux.c`, one test
//! per block of its checks, with C's error numbers in the assertion messages.
//! C reads and sets `SpecVal` on the SpecFluxFrame, which hands the
//! attribute to its FluxFrame; here that is [`SpecFluxFrame::flux_frame`].
//! C builds each FluxFrame with `astFluxFrame( specval, specfrm, "..." )`;
//! here that is [`FluxFrame::with_spec_val`] followed by the setters the
//! attribute string calls.

use crate::common::native_dump;
use starlink_ast::frames::FrameSearch;
use starlink_ast::frames::{
    FluxFrame, FluxSystem, Frame, FrameRef, FrameSet, SpecFluxFrame, SpecFrame, SpecSystem,
};
use starlink_ast::{AnyObject, from_native_string};

fn ghz() -> SpecFrame {
    let mut sf = SpecFrame::new(SpecSystem::Freq);
    sf.set_unit(1, "GHz").unwrap();
    sf
}

fn flux_unit(spec_val: f64, sf: Option<&SpecFrame>, unit: &str) -> FluxFrame {
    let mut ff = FluxFrame::with_spec_val(spec_val, sf.cloned());
    ff.set_unit(1, unit).unwrap();
    ff
}

fn convert(from: &SpecFluxFrame, to: &SpecFluxFrame) -> Option<FrameSet> {
    from.clone().convert_to(&mut to.clone(), &[]).unwrap()
}

/// `astTran2` through a FrameSet's base-to-current Mapping.
fn tran2(fs: &FrameSet, x: f64, y: f64, forward: bool) -> (f64, f64) {
    let map = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let (xo, yo) = map.tran2(&[x], &[y], forward).unwrap();
    (xo[0], yo[0])
}

/// The SpecFluxFrame most of C's checks start from.
fn jansky() -> SpecFluxFrame {
    let sf = ghz();
    SpecFluxFrame::new(sf.clone(), flux_unit(123.0, Some(&sf), "Jy"))
}

/// A SpecFluxFrame measuring flux per unit wavelength.
fn per_wavelength() -> SpecFluxFrame {
    let sf = ghz();
    let mut ff2 = FluxFrame::with_spec_val(123.1, Some(sf.clone()));
    ff2.set_system(FluxSystem::FluxDenW).unwrap();
    assert_eq!(Frame::unit(&ff2, 1).unwrap(), "W/m^2/Angstrom", "Error 6B");
    assert_eq!(ff2.system().unwrap().as_str(), "FLXDNW", "Error 6C");
    SpecFluxFrame::new(sf, ff2)
}

#[test]
fn spec_val_defaults_to_the_flux_frame_value() {
    let mut sff = jansky();
    assert_eq!(sff.flux_frame().spec_val(), 123.0, "Error 1");
    assert!(!sff.flux_frame().test_spec_val(), "Error 2");
    sff.flux_frame_mut().set_spec_val(333.3);
    assert_eq!(sff.flux_frame().spec_val(), 333.3, "Error 3");
    assert!(sff.flux_frame().test_spec_val(), "Error 4");
    sff.flux_frame_mut().clear_spec_val();
    assert_eq!(sff.flux_frame().spec_val(), 123.0, "Error 5");
    assert!(!sff.flux_frame().test_spec_val(), "Error 6");

    // CheckDump 1.
    let text = native_dump(&AnyObject::SpecFluxFrame(Box::new(sff.clone()))).unwrap();
    let AnyObject::SpecFluxFrame(read) = from_native_string(&text).unwrap() else {
        panic!("CheckDump 1: not a SpecFluxFrame");
    };
    assert_eq!(
        read.flux_frame().spec_val(),
        sff.flux_frame().spec_val(),
        "Object has changed"
    );
}

#[test]
fn flux_density_converts_to_per_wavelength() {
    let fs = convert(&jansky(), &per_wavelength()).expect("Error 8");
    let (x, y) = tran2(&fs, 2.0, 1.0, true);
    assert!((y - 1.33425638e-26).abs() <= 1.0e-32, "Error 9: {y}");
    assert_eq!(x, 2.0, "Error 10");
}

#[test]
fn conversion_follows_permuted_axes() {
    let sff = jansky();
    let mut sff2 = per_wavelength();
    sff2.perm_axes(&[2, 1]).unwrap();
    let fs = convert(&sff, &sff2).expect("Error 11");
    let (x, y) = tran2(&fs, 2.0, 1.0, true);
    assert!((x - 1.33425638e-26).abs() <= 1.0e-32, "Error 12: {x}");
    assert_eq!(y, 2.0, "Error 13");

    let mut sff = sff;
    sff.perm_axes(&[2, 1]).unwrap();
    let fs = convert(&sff, &sff2).expect("Error 14");
    let (x, y) = tran2(&fs, 1.0, 2.0, true);
    assert!((x - 1.33425638e-26).abs() <= 1.0e-32, "Error 15: {x}");
    assert_eq!(y, 2.0, "Error 16");
}

#[test]
fn logarithmic_flux_units_convert() {
    let ff2 = flux_unit(starlink_ast::constants::BAD_VALUE, None, "log(W/m2/nm)");
    assert_eq!(ff2.system().unwrap().as_str(), "FLXDNW", "Error 17");
    let sff2 = SpecFluxFrame::new(ghz(), ff2);
    let fs = convert(&jansky(), &sff2).expect("Error 18");
    let (x, y) = tran2(&fs, 2.0, 1.0, true);
    assert!((y + 24.8747607).abs() <= 0.000001, "Error 19: {y}");
    assert_eq!(x, 2.0, "Error 20");
    let (xi, yi) = tran2(&fs, x, y, false);
    assert!((xi - 2.0).abs() <= 1.0e-9, "Error 21");
    assert!((yi - 1.0).abs() <= 1.0e-9, "Error 22");
}

#[test]
fn surface_brightness_converts() {
    let ff2 = flux_unit(starlink_ast::constants::BAD_VALUE, None, "log(W/m2/nm/sr)");
    assert_eq!(ff2.system().unwrap().as_str(), "SFCBRW", "Error 23");
    let sff2 = SpecFluxFrame::new(ghz(), ff2);
    let sf = ghz();
    let ff = flux_unit(123.0, Some(&sf), "Jy/deg**2");
    assert_eq!(ff.system().unwrap().as_str(), "SFCBR", "Error 24");
    let sff = SpecFluxFrame::new(sf, ff);
    let fs = convert(&sff, &sff2).expect("Error 25");
    let (x, y) = tran2(&fs, 2.0, 1.0, true);
    assert!((y + 21.3585154).abs() <= 0.000001, "Error 26: {y}");
    assert_eq!(x, 2.0, "Error 27");
    let (xi, yi) = tran2(&fs, x, y, false);
    assert!((xi - 2.0).abs() <= 1.0e-9, "Error 28");
    assert!((yi - 1.0).abs() <= 1.0e-9, "Error 29");
}

#[test]
fn equal_when_and_only_when_the_components_are() {
    let esf = SpecFrame::new(SpecSystem::Freq);
    let mut eff = FluxFrame::unset_system();
    eff.set_system(FluxSystem::FluxDen).unwrap();
    let mut eff2 = FluxFrame::unset_system();
    eff2.set_system(FluxSystem::FluxDenW).unwrap();
    let ea = SpecFluxFrame::new(esf.clone(), eff.clone());
    let eb = SpecFluxFrame::new(esf.clone(), eff);
    let ec = ea.clone();
    let ed = SpecFluxFrame::new(esf, eff2);
    assert!(Frame::equal(&ea, &eb), "Error 30");
    assert!(Frame::equal(&ea, &ec), "Error 31");
    assert!(!Frame::equal(&ea, &ed), "Error 32");
}
