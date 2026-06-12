//! Tests for [`FluxFrame`] and [`SpecFluxFrame`].
//!
//! Based on `reference/starlink-ast/src/fluxframe.c` and
//! `reference/starlink-ast/src/specfluxframe.c`.
//! Uses the public Rust API only.

use ast_core::frames::{FluxFrame, FluxSystem, Frame, SpecFluxFrame, SpecFrame, SpecSystem};
use ast_core::{AnyObject, from_native_string, to_native_string};

// ===========================================================================
// FluxFrame tests
// ===========================================================================

// ---------------------------------------------------------------------------
// Construction and defaults
// ---------------------------------------------------------------------------

#[test]
fn test_fluxframe_new_fluxden() {
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    assert_eq!(ff.naxes(), 1);
    assert_eq!(ff.system(), FluxSystem::FluxDen);
    assert_eq!(ff.system_string(), Some("FLXDN"));
}

#[test]
fn test_fluxframe_new_fluxdenw() {
    let ff = FluxFrame::new(FluxSystem::FluxDenW);
    assert_eq!(ff.naxes(), 1);
    assert_eq!(ff.system(), FluxSystem::FluxDenW);
    assert_eq!(ff.system_string(), Some("FLXDNW"));
}

#[test]
fn test_fluxframe_new_sbright() {
    let ff = FluxFrame::new(FluxSystem::SBright);
    assert_eq!(ff.naxes(), 1);
    assert_eq!(ff.system(), FluxSystem::SBright);
    assert_eq!(ff.system_string(), Some("SFCBR"));
}

#[test]
fn test_fluxframe_new_sbrightw() {
    let ff = FluxFrame::new(FluxSystem::SBrightW);
    assert_eq!(ff.naxes(), 1);
    assert_eq!(ff.system(), FluxSystem::SBrightW);
    assert_eq!(ff.system_string(), Some("SFCBRW"));
}

// ---------------------------------------------------------------------------
// FluxSystem parsing
// ---------------------------------------------------------------------------

#[test]
fn test_flux_system_parse() {
    assert_eq!(FluxSystem::parse("FLXDN"), Some(FluxSystem::FluxDen));
    assert_eq!(FluxSystem::parse("flxdn"), Some(FluxSystem::FluxDen));
    assert_eq!(FluxSystem::parse("FLXDNW"), Some(FluxSystem::FluxDenW));
    assert_eq!(FluxSystem::parse("SFCBR"), Some(FluxSystem::SBright));
    assert_eq!(FluxSystem::parse("SFCBRW"), Some(FluxSystem::SBrightW));
    assert_eq!(FluxSystem::parse("FLUXDEN"), Some(FluxSystem::FluxDen));
    assert_eq!(FluxSystem::parse("FLUXDENW"), Some(FluxSystem::FluxDenW));
    assert_eq!(FluxSystem::parse("SBRIGHT"), Some(FluxSystem::SBright));
    assert_eq!(FluxSystem::parse("SBRIGHTW"), Some(FluxSystem::SBrightW));
    assert_eq!(FluxSystem::parse("NONSENSE"), None);
}

// ---------------------------------------------------------------------------
// FluxSystem properties
// ---------------------------------------------------------------------------

#[test]
fn test_flux_system_properties() {
    assert!(FluxSystem::FluxDen.is_per_frequency());
    assert!(!FluxSystem::FluxDenW.is_per_frequency());
    assert!(FluxSystem::SBright.is_per_frequency());
    assert!(!FluxSystem::SBrightW.is_per_frequency());

    assert_eq!(FluxSystem::FluxDen.default_units(), "W/m^2/Hz");
    assert_eq!(FluxSystem::FluxDenW.default_units(), "W/m^2/Angstrom");
    assert_eq!(FluxSystem::SBright.default_units(), "W/m^2/Hz/arcmin**2");
    assert_eq!(
        FluxSystem::SBrightW.default_units(),
        "W/m^2/Angstrom/arcmin**2"
    );
}

// ---------------------------------------------------------------------------
// SpecVal attribute
// ---------------------------------------------------------------------------

#[test]
fn test_fluxframe_spec_val() {
    let mut ff = FluxFrame::new(FluxSystem::FluxDen);
    // Default is BAD_VALUE.
    assert!(ff.spec_val() < -1.0e300);

    ff.set_spec_val(1.4e9); // 1.4 GHz
    assert_eq!(ff.spec_val(), 1.4e9);

    ff.clear_spec_val();
    assert!(ff.spec_val() < -1.0e300);
}

// ---------------------------------------------------------------------------
// Equality
// ---------------------------------------------------------------------------

#[test]
fn test_fluxframe_equal_same() {
    let ff1 = FluxFrame::new(FluxSystem::FluxDen);
    let ff2 = FluxFrame::new(FluxSystem::FluxDen);
    assert!(ff1.equal(&ff2));
}

#[test]
fn test_fluxframe_equal_different_system() {
    let ff1 = FluxFrame::new(FluxSystem::FluxDen);
    let ff2 = FluxFrame::new(FluxSystem::FluxDenW);
    assert!(!ff1.equal(&ff2));
}

#[test]
fn test_fluxframe_equal_different_specval() {
    let mut ff1 = FluxFrame::new(FluxSystem::FluxDen);
    let mut ff2 = FluxFrame::new(FluxSystem::FluxDen);
    ff1.set_spec_val(1.0e9);
    ff2.set_spec_val(2.0e9);
    assert!(!ff1.equal(&ff2));
}

// ---------------------------------------------------------------------------
// Frame trait: format, unformat, distance
// ---------------------------------------------------------------------------

#[test]
fn test_fluxframe_format() {
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    let s = ff.format(1, 1.5).unwrap();
    assert!(s.contains("1.5"));

    // Very small values still produce a valid string.
    let s2 = ff.format(1, 1.23e-3).unwrap();
    assert!(!s2.is_empty());
    let val: f64 = s2.trim().parse().unwrap();
    assert!((val - 1.23e-3).abs() < 1.0e-9);
}

#[test]
fn test_fluxframe_unformat() {
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    let (val, nc) = ff.unformat(1, "1.5e-3 Jy").unwrap();
    assert!((val - 1.5e-3).abs() < 1.0e-15);
    assert!(nc > 0);
}

#[test]
fn test_fluxframe_distance() {
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    let d = ff.distance(&[1.0], &[3.0]).unwrap();
    assert_eq!(d, 2.0);
}

// ---------------------------------------------------------------------------
// Conversion (same system = identity)
// ---------------------------------------------------------------------------

#[test]
fn test_fluxframe_convert_same_system() {
    let ff1 = FluxFrame::new(FluxSystem::FluxDen);
    let ff2 = FluxFrame::new(FluxSystem::FluxDen);
    let fs = ff1.convert_to(&ff2, &[]).unwrap();
    assert!(fs.is_some());
}

#[test]
fn test_fluxframe_convert_different_system_returns_none() {
    let ff1 = FluxFrame::new(FluxSystem::FluxDen);
    let ff2 = FluxFrame::new(FluxSystem::FluxDenW);
    let fs = ff1.convert_to(&ff2, &[]).unwrap();
    // Different systems — full conversion not yet implemented.
    assert!(fs.is_none());
}

// ---------------------------------------------------------------------------
// Serialisation round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_fluxframe_serialisation_roundtrip_fluxden() {
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    let obj = AnyObject::FluxFrame(ff.clone());
    let text = to_native_string(&obj).unwrap();
    assert!(text.contains("FluxFrame"));
    assert!(text.contains("FLXDN"));

    let obj2 = from_native_string(&text).unwrap();
    let ff2 = obj2.as_frame().unwrap();
    assert!(ff.equal(ff2));
}

#[test]
fn test_fluxframe_serialisation_roundtrip_sbrightw() {
    let mut ff = FluxFrame::new(FluxSystem::SBrightW);
    ff.set_spec_val(5.0e14); // ~600 nm
    let obj = AnyObject::FluxFrame(ff.clone());
    let text = to_native_string(&obj).unwrap();
    assert!(text.contains("SFCBRW"));
    assert!(text.contains("SpcVl"));

    let obj2 = from_native_string(&text).unwrap();
    let ff2 = obj2.as_frame().unwrap();
    assert!(ff.equal(ff2));
}

#[test]
fn test_fluxframe_serialisation_with_title() {
    let mut ff = FluxFrame::new(FluxSystem::FluxDen);
    ff.set_title("Test Title");
    let obj = AnyObject::FluxFrame(ff.clone());
    let text = to_native_string(&obj).unwrap();
    assert!(text.contains("Test Title"));

    let obj2 = from_native_string(&text).unwrap();
    let ff2 = obj2.as_frame().unwrap();
    assert!(ff.equal(ff2));
}

// ===========================================================================
// SpecFluxFrame tests
// ===========================================================================

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[test]
fn test_specfluxframe_new() {
    let sf = SpecFrame::new(SpecSystem::Freq);
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    let sff = SpecFluxFrame::new(sf, ff);
    assert_eq!(sff.naxes(), 2);
}

// ---------------------------------------------------------------------------
// Component access
// ---------------------------------------------------------------------------

#[test]
fn test_specfluxframe_components() {
    let sf = SpecFrame::new(SpecSystem::Wavelen);
    let ff = FluxFrame::new(FluxSystem::SBright);
    let sff = SpecFluxFrame::new(sf, ff);

    assert_eq!(sff.spec_frame().system(), SpecSystem::Wavelen);
    assert_eq!(sff.flux_frame().system(), FluxSystem::SBright);
}

#[test]
fn test_specfluxframe_component_mutation() {
    let sf = SpecFrame::new(SpecSystem::Freq);
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    let mut sff = SpecFluxFrame::new(sf, ff);

    sff.flux_frame_mut().set_system(FluxSystem::SBright);
    assert_eq!(sff.flux_frame().system(), FluxSystem::SBright);
}

// ---------------------------------------------------------------------------
// Frame trait delegation
// ---------------------------------------------------------------------------

#[test]
fn test_specfluxframe_format_axes() {
    let sf = SpecFrame::new(SpecSystem::Freq);
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    let sff = SpecFluxFrame::new(sf, ff);

    // Axis 1 (SpecFrame) and axis 2 (FluxFrame) should both format.
    let s1 = sff.format(1, 1.4e9).unwrap();
    assert!(!s1.is_empty());
    let s2 = sff.format(2, 1.0e-26).unwrap();
    assert!(!s2.is_empty());

    // Axis 3 should error.
    assert!(sff.format(3, 1.0).is_err());
}

#[test]
fn test_specfluxframe_distance() {
    let sf = SpecFrame::new(SpecSystem::Freq);
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    let sff = SpecFluxFrame::new(sf, ff);

    let d = sff.distance(&[0.0, 0.0], &[3.0, 4.0]).unwrap();
    assert!((d - 5.0).abs() < 1.0e-10);
}

// ---------------------------------------------------------------------------
// Equality
// ---------------------------------------------------------------------------

#[test]
fn test_specfluxframe_equal() {
    let sf1 = SpecFrame::new(SpecSystem::Freq);
    let ff1 = FluxFrame::new(FluxSystem::FluxDen);
    let sff1 = SpecFluxFrame::new(sf1.clone(), ff1.clone());
    let sff2 = SpecFluxFrame::new(sf1, ff1);
    assert!(sff1.equal(&sff2));
}

#[test]
fn test_specfluxframe_not_equal_different_spec() {
    let sf1 = SpecFrame::new(SpecSystem::Freq);
    let sf2 = SpecFrame::new(SpecSystem::Wavelen);
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    let sff1 = SpecFluxFrame::new(sf1, ff.clone());
    let sff2 = SpecFluxFrame::new(sf2, ff);
    assert!(!sff1.equal(&sff2));
}

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

#[test]
fn test_specfluxframe_convert_same() {
    let sf = SpecFrame::new(SpecSystem::Freq);
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    let sff1 = SpecFluxFrame::new(sf.clone(), ff.clone());
    let sff2 = SpecFluxFrame::new(sf, ff);
    let fs = sff1.convert_to(&sff2, &[]).unwrap();
    assert!(fs.is_some());
}

#[test]
fn test_specfluxframe_convert_different_returns_none() {
    let sf1 = SpecFrame::new(SpecSystem::Freq);
    let sf2 = SpecFrame::new(SpecSystem::Wavelen);
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    let sff1 = SpecFluxFrame::new(sf1, ff.clone());
    let sff2 = SpecFluxFrame::new(sf2, ff);
    let fs = sff1.convert_to(&sff2, &[]).unwrap();
    assert!(fs.is_none());
}

// ---------------------------------------------------------------------------
// Serialisation round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_specfluxframe_serialisation_roundtrip() {
    let sf = SpecFrame::new(SpecSystem::Freq);
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    let sff = SpecFluxFrame::new(sf, ff);
    let obj = AnyObject::SpecFluxFrame(sff.clone());
    let text = to_native_string(&obj).unwrap();
    assert!(text.contains("SpecFluxFrame"));
    assert!(text.contains("SpecFrame"));
    assert!(text.contains("FluxFrame"));

    let obj2 = from_native_string(&text).unwrap();
    let sff2 = obj2.as_frame().unwrap();
    assert!(sff.equal(sff2));
}

#[test]
fn test_specfluxframe_serialisation_with_attributes() {
    let mut sf = SpecFrame::new(SpecSystem::Wavelen);
    sf.set_rest_freq(4.57e14); // red light
    let mut ff = FluxFrame::new(FluxSystem::SBrightW);
    ff.set_spec_val(4.57e14);
    let sff = SpecFluxFrame::new(sf, ff);
    let obj = AnyObject::SpecFluxFrame(sff.clone());
    let text = to_native_string(&obj).unwrap();

    let obj2 = from_native_string(&text).unwrap();
    let sff2 = obj2.as_frame().unwrap();
    assert!(sff.equal(sff2));
}
