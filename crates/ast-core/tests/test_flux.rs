//! FluxFrame: the default SpecVal, the System its Unit implies, and the
//! conversions between flux systems.
//!
//! Ported from `reference/starlink-ast/ast_tester/testflux.c`, one test per
//! block of its checks, with C's error numbers in the assertion messages.
//! C builds each FluxFrame with `astFluxFrame( specval, specfrm, "unit=..." )`;
//! here that is [`FluxFrame::with_spec_val`] followed by `set_unit`, which is
//! what the attribute string does in C. `astGetMapping` returns a simplified
//! Mapping, so the Mappings are simplified before they are examined.

use ast_core::frames::{FluxFrame, FluxSystem, Frame, FrameRef, SpecFrame, SpecSystem};
use ast_core::mappings::{DynMapping, Mapping, UnitMap, ZoomMap};
use ast_core::{AnyObject, from_native_string, to_native_string};

fn spec(system: SpecSystem, unit: &str) -> SpecFrame {
    let mut sf = SpecFrame::new(system);
    sf.set_unit(1, unit).unwrap();
    sf
}

fn flux(spec_val: f64, sf: &SpecFrame, unit: Option<&str>) -> FluxFrame {
    let mut ff = FluxFrame::with_spec_val(spec_val, Some(sf.clone()));
    if let Some(unit) = unit {
        ff.set_unit(1, unit).unwrap();
    }
    ff
}

/// The simplified Mapping from `from` to `to`, as `astConvert` and
/// `astGetMapping` give it, or `None` when they do not convert.
fn conversion(from: &FluxFrame, to: &FluxFrame) -> Option<DynMapping> {
    let fs = from.convert_to(to, &[]).unwrap()?;
    Some(
        fs.mapping(FrameRef::Base, FrameRef::Current)
            .unwrap()
            .simplify()
            .unwrap(),
    )
}

fn zoom_of(map: &dyn Mapping) -> f64 {
    map.as_any()
        .downcast_ref::<ZoomMap>()
        .expect("a ZoomMap")
        .zoom()
}

#[test]
fn spec_val_defaults_to_the_constructor_value() {
    let sf = spec(SpecSystem::Freq, "GHz");
    let mut ff = flux(123.0, &sf, None);
    assert_eq!(ff.spec_val(), 123.0, "Error 1");
    assert!(!ff.test_spec_val(), "Error 2");
    ff.set_spec_val(333.3);
    assert_eq!(ff.spec_val(), 333.3, "Error 3");
    assert!(ff.test_spec_val(), "Error 4");
    ff.clear_spec_val();
    assert_eq!(ff.spec_val(), 123.0, "Error 5");
    assert!(!ff.test_spec_val(), "Error 6");

    // CheckDump 1: the default SpecVal survives a dump.
    let text = to_native_string(&AnyObject::FluxFrame(ff.clone())).unwrap();
    let AnyObject::FluxFrame(read) = from_native_string(&text).unwrap() else {
        panic!("not a FluxFrame");
    };
    assert_eq!(read.spec_val(), ff.spec_val(), "CheckDump 1");
}

#[test]
fn similar_spec_vals_convert_with_a_unit_map() {
    let sf = spec(SpecSystem::Freq, "GHz");
    let ff = flux(123.0, &sf, None);
    let ff2 = flux(123.1, &sf, None);
    let map = conversion(&ff, &ff2).expect("Error 8");
    assert!(map.as_any().is::<UnitMap>(), "Error 9");
}

#[test]
fn per_frequency_units_scale() {
    let sf = spec(SpecSystem::Freq, "GHz");
    let ff = flux(123.0, &sf, Some("W/m^2/Hz"));
    assert_eq!(ff.system().as_str(), "FLXDN", "Error 10");
    let ff2 = flux(123.0, &sf, Some("W/m^2/GHz"));
    assert_eq!(ff2.system().as_str(), "FLXDN", "Error 11");
    let map = conversion(&ff2, &ff).expect("Error 12");
    assert!(
        (zoom_of(map.as_ref()) - 1.0e-9).abs() <= 1.0e-24,
        "Error 14"
    );
}

#[test]
fn per_wavelength_units_scale() {
    let sf = spec(SpecSystem::Freq, "GHz");
    let ff = flux(123.0, &sf, Some("W/m^2/m"));
    assert_eq!(ff.system().as_str(), "FLXDNW", "Error 15");
    let sf2 = spec(SpecSystem::Freq, "Hz");
    let ff2 = flux(123.0e9, &sf2, Some("W/m^2/Angstrom"));
    assert_eq!(ff2.system().as_str(), "FLXDNW", "Error 16");
    let map = conversion(&ff2, &ff).expect("Error 17");
    assert_eq!(zoom_of(map.as_ref()), 1.0e10, "Error 19");
}

#[test]
fn per_wavelength_units_scale_across_spectral_systems() {
    let sf = spec(SpecSystem::Freq, "GHz");
    let ff = flux(123.0, &sf, Some("W/m^2/m"));
    assert_eq!(ff.system().as_str(), "FLXDNW", "Error 20");
    let sf2 = spec(SpecSystem::Wavelen, "nm");
    let ff2 = flux(2437337.06, &sf2, Some("W/m^2/Angstrom"));
    assert_eq!(ff2.system().as_str(), "FLXDNW", "Error 21");
    let map = conversion(&ff, &ff2).expect("Error 22");
    assert_eq!(zoom_of(map.as_ref()), 1.0e-10, "Error 24");
}

#[test]
fn frequency_density_converts_to_wavelength_density() {
    let sf = spec(SpecSystem::Freq, "GHz");
    let ff = flux(123.0, &sf, Some("W/m^2/Hz"));
    let sf2 = spec(SpecSystem::Wavelen, "nm");
    let ff2 = flux(2437337.06, &sf2, Some("W/m^2/m"));
    let map = conversion(&ff, &ff2).expect("Error 25");
    let out = map.tran1(&[1.0e-13], true).unwrap()[0];
    assert!((out - 5.04649119).abs() <= 1.0e-6, "Error 26: {out}");
}

#[test]
fn surface_brightness_converts() {
    let sf = spec(SpecSystem::Freq, "GHz");
    let ff = flux(123.0, &sf, Some("W/m^2/Hz/arcsec**2"));
    assert_eq!(ff.system().as_str(), "SFCBR", "Error 27a");
    let sf2 = spec(SpecSystem::Wavelen, "nm");
    let ff2 = flux(2437337.06, &sf2, Some("W/m^2/m/deg**2"));
    assert_eq!(ff2.system().as_str(), "SFCBRW", "Error 27b");
    let map = conversion(&ff, &ff2).expect("Error 27");
    let out = map.tran1(&[1.0e-13], true).unwrap()[0];
    assert!((out - 65402525.8).abs() <= 1.0, "Error 28: {out}");
}

#[test]
fn surface_brightness_does_not_convert_to_flux_density() {
    let sf = spec(SpecSystem::Freq, "GHz");
    let ff = flux(123.0, &sf, Some("W/m^2/Hz/arcsec**2"));
    let sf2 = spec(SpecSystem::Wavelen, "nm");
    let ff2 = flux(2437337.06, &sf2, Some("W/m^2/m"));
    assert!(conversion(&ff, &ff2).is_none(), "Error 29");
}

#[test]
fn per_wavelength_surface_brightness_reads_back() {
    let mut ff = FluxFrame::unset_system();
    ff.set_system(FluxSystem::parse("SFCBRW").expect("Error 30"));
    assert_eq!(ff.system().as_str(), "SFCBRW", "Error 31");
    let text = to_native_string(&AnyObject::FluxFrame(ff)).unwrap();
    let AnyObject::FluxFrame(read) = from_native_string(&text).unwrap() else {
        panic!("Error 32: not a FluxFrame");
    };
    assert_eq!(read.system().as_str(), "SFCBRW", "Error 32");
}
