//! Tests for [`FluxFrame`] and [`SpecFluxFrame`].
//!
//! Based on `reference/starlink-ast/src/fluxframe.c` and
//! `reference/starlink-ast/src/specfluxframe.c`.
//! Uses the public Rust API only.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
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
fn test_fluxframe_equal_different_specval_is_equal() {
    // C Frame equality is conversion-based (frame.c:3549). Within one flux
    // System the conversion is a UnitMap regardless of SpecVal (SpecVal
    // only matters for cross-System flux conversion via MakeFluxMapping),
    // so two FLUXDEN FluxFrames differing only in SpecVal compare EQUAL.
    // Oracle-verified: astEqual(FluxFrame 1e9, FluxFrame 2e9) == 1 in FLXDN.
    let mut ff1 = FluxFrame::new(FluxSystem::FluxDen);
    let mut ff2 = FluxFrame::new(FluxSystem::FluxDen);
    ff1.set_spec_val(1.0e9);
    ff2.set_spec_val(2.0e9);
    assert!(ff1.equal(&ff2));
}

// ---------------------------------------------------------------------------
// Frame trait: format, unformat, distance
// ---------------------------------------------------------------------------

#[test]
fn test_fluxframe_format() {
    // FluxFrame does not override Format in C (fluxframe.c has no
    // `static int Format`); it inherits the base Frame/Axis `%1.7G` contract
    // (axis.c), with no units appended. Oracle-verified against the built
    // reference libast.
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    assert_eq!(ff.format(1, 1.5).unwrap(), "1.5");
    assert_eq!(ff.format(1, 1.23e-3).unwrap(), "0.00123");
    assert_eq!(ff.format(1, 1.0e-26).unwrap(), "1E-26");
    assert_eq!(ff.format(1, 1.4e9).unwrap(), "1.4E+09");
    assert_eq!(ff.format(1, BAD_VALUE).unwrap(), "<bad>");
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
fn test_fluxframe_make_flux_mapping_same_system_is_unit() {
    // Same flux system → zoom factor 1 (fluxframe.c:2134).
    let ff1 = FluxFrame::new(FluxSystem::FluxDen);
    let ff2 = FluxFrame::new(FluxSystem::FluxDen);
    let map = ff1
        .make_flux_mapping(&ff2)
        .unwrap()
        .expect("same-system map");
    let pts = PointSet::from_rows([[3.0]]);
    let out = map.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 3.0).abs() < 1e-12);
}

#[test]
fn test_fluxframe_make_flux_mapping_without_specframe_returns_none() {
    // FLXDN (per Hz) → FLXDNW (per Angstrom) needs a consistent spectral
    // position. Without an encapsulated SpecFrame, SpecVal in FLXDN is Hz and
    // in FLXDNW is Angstrom, so the standardised positions don't match and no
    // mapping is formed. Oracle: astConvert returns NULL in this case.
    let mut ff1 = FluxFrame::new(FluxSystem::FluxDen);
    ff1.set_spec_val(1.4e9);
    let mut ff2 = FluxFrame::new(FluxSystem::FluxDenW);
    ff2.set_spec_val(1.4e9);
    assert!(ff1.make_flux_mapping(&ff2).unwrap().is_none());
}

#[test]
fn test_fluxframe_make_flux_mapping_cross_system_with_specframe() {
    // With a FREQ/Hz SpecFrame attached, SpecVal=1.4e9 Hz is a well-defined
    // spectral position interpretable in both density systems. Oracle (the
    // reference C library, astConvert FLXDN->FLXDNW at 1.4e9 Hz): the flux
    // scale factor is 0.653785630413752.
    let mut sf1 = SpecFrame::new(SpecSystem::Freq);
    sf1.set_unit(1, "Hz").unwrap();
    let mut ff1 = FluxFrame::new(FluxSystem::FluxDen);
    ff1.set_spec_frame(sf1);
    ff1.set_spec_val(1.4e9);

    let mut sf2 = SpecFrame::new(SpecSystem::Freq);
    sf2.set_unit(1, "Hz").unwrap();
    let mut ff2 = FluxFrame::new(FluxSystem::FluxDenW);
    ff2.set_spec_frame(sf2);
    ff2.set_spec_val(1.4e9);

    let map = ff1
        .make_flux_mapping(&ff2)
        .unwrap()
        .expect("cross-system map");
    let pts = PointSet::from_rows([[1.0]]);
    let out = map.transform(&pts, true).unwrap();
    assert!(
        (out.point(0)[0] - 0.653785630413752).abs() < 1e-9,
        "expected oracle scale 0.653785630413752, got {}",
        out.point(0)[0]
    );
}

#[test]
fn test_fluxframe_make_flux_mapping_surface_brightness_mismatch_none() {
    // Exactly one system is surface brightness → no mapping (fluxframe.c:2129).
    let mut ff1 = FluxFrame::new(FluxSystem::FluxDen);
    ff1.set_spec_val(1.4e9);
    let mut ff2 = FluxFrame::new(FluxSystem::SBright);
    ff2.set_spec_val(1.4e9);
    assert!(ff1.make_flux_mapping(&ff2).unwrap().is_none());
}

// ---------------------------------------------------------------------------
// Per-instance Unit support (C MakeFluxMapping map1/map3 unit bracketing)
// ---------------------------------------------------------------------------

#[test]
fn test_fluxframe_unit_accessors() {
    let mut ff = FluxFrame::new(FluxSystem::FluxDen);
    assert!(!ff.test_unit(1).unwrap());
    assert_eq!(ff.unit_or_default(), "W/m^2/Hz");
    ff.set_unit(1, "Jy").unwrap();
    assert!(ff.test_unit(1).unwrap());
    assert_eq!(ff.unit(1).unwrap(), "Jy");
    assert_eq!(ff.unit_or_default(), "Jy");
    ff.clear_unit(1).unwrap();
    assert!(!ff.test_unit(1).unwrap());
    assert_eq!(ff.unit_or_default(), "W/m^2/Hz");
}

#[test]
fn test_fluxframe_system_derived_from_unit() {
    // C GetSystem (fluxframe.c:1514): when System is unset but Unit is set,
    // the default System is the first flux system whose default units are
    // unit-compatible. Oracle: unit=Jy -> FLXDN; W/m^2/Angstrom -> FLXDNW.
    let mut ff = FluxFrame::new(FluxSystem::FluxDen); // unset System
    assert!(!ff.test_system());
    ff.set_unit(1, "Jy").unwrap();
    assert_eq!(ff.system(), FluxSystem::FluxDen);
    // System remains "unset" (the default is unit-derived, not explicit).
    assert!(!ff.test_system());

    let mut ff = FluxFrame::new(FluxSystem::FluxDen);
    ff.set_unit(1, "W/m^2/Angstrom").unwrap();
    assert_eq!(ff.system(), FluxSystem::FluxDenW);
}

#[test]
fn test_fluxframe_jy_to_default_units() {
    // Jy = 1e-26 W/m^2/Hz. A FLXDN frame in Jy converting to a default-unit
    // FLXDN frame scales 1.0 Jy -> 1e-26 (oracle-verified astConvert).
    let mut jy = FluxFrame::new(FluxSystem::FluxDen);
    jy.set_unit(1, "Jy").unwrap();
    let def = FluxFrame::new(FluxSystem::FluxDen);

    let map = jy
        .make_flux_mapping(&def)
        .unwrap()
        .expect("Jy->default map");
    let pts = PointSet::from_rows([[1.0]]);
    let out = map.transform(&pts, true).unwrap();
    assert!(
        (out.point(0)[0] - 1.0e-26).abs() < 1.0e-32,
        "1 Jy should be 1e-26 W/m^2/Hz, got {}",
        out.point(0)[0]
    );

    // Reverse: 1e-26 -> 1.0 Jy.
    let map = def
        .make_flux_mapping(&jy)
        .unwrap()
        .expect("default->Jy map");
    let out = map
        .transform(&PointSet::from_rows([[1.0e-26]]), true)
        .unwrap();
    assert!(
        (out.point(0)[0] - 1.0).abs() < 1.0e-9,
        "got {}",
        out.point(0)[0]
    );
}

#[test]
fn test_fluxframe_cross_system_with_units() {
    // Jy FLXDN -> W/m^2/Angstrom FLXDNW at 1.4 GHz. This combines the density
    // zoom (0.653785630413752) with the 1e-26 Jy factor (map1). Oracle
    // (astConvert): 1.0 -> 6.5378563041e-27.
    let mut sf1 = SpecFrame::new(SpecSystem::Freq);
    sf1.set_unit(1, "Hz").unwrap();
    let mut jy = FluxFrame::new(FluxSystem::FluxDen);
    jy.set_unit(1, "Jy").unwrap();
    jy.set_spec_frame(sf1);
    jy.set_spec_val(1.4e9);

    let mut sf2 = SpecFrame::new(SpecSystem::Freq);
    sf2.set_unit(1, "Hz").unwrap();
    let mut w = FluxFrame::new(FluxSystem::FluxDenW); // default W/m^2/Angstrom
    w.set_spec_frame(sf2);
    w.set_spec_val(1.4e9);

    let map = jy.make_flux_mapping(&w).unwrap().expect("cross-system map");
    let out = map.transform(&PointSet::from_rows([[1.0]]), true).unwrap();
    assert!(
        (out.point(0)[0] - 6.5378563041e-27).abs() < 1.0e-37,
        "expected 6.5378563041e-27, got {}",
        out.point(0)[0]
    );
}

#[test]
fn test_fluxframe_incompatible_unit_no_conversion() {
    // A dimensionally-incompatible target unit cannot bracket the zoom, so no
    // conversion is formed (C: map1 NULL -> no match).
    let mut bad = FluxFrame::new(FluxSystem::FluxDen);
    bad.set_unit(1, "m").unwrap(); // length, not a flux density
    let def = FluxFrame::new(FluxSystem::FluxDen);
    assert!(bad.make_flux_mapping(&def).unwrap().is_none());
}

#[test]
fn test_fluxframe_unit_serialisation_round_trip() {
    let mut ff = FluxFrame::new(FluxSystem::FluxDen);
    ff.set_unit(1, "Jy").unwrap();
    let text = to_native_string(&AnyObject::FluxFrame(ff)).unwrap();
    assert!(text.contains("Unit = \"Jy\""), "missing Unit: {text}");

    let parsed = from_native_string(&text).unwrap();
    let ff2 = parsed.as_frame().unwrap();
    let ff2 = ff2.as_any().downcast_ref::<FluxFrame>().expect("FluxFrame");
    assert_eq!(ff2.unit(1).unwrap(), "Jy");
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

/// FluxFrame does not override Unformat in C (fluxframe.c has no
/// `static int Unformat`); it inherits the base Frame/Axis contract
/// (`astAxisUnformat`, axis.c:1284). Oracle-verified against the built
/// reference libast.
#[test]
fn test_fluxframe_unformat_base_contract() {
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    assert_eq!(ff.unformat(1, "12.5").unwrap(), (12.5, 4));
    assert_eq!(ff.unformat(1, "  12.5  ").unwrap(), (12.5, 8));
    assert_eq!(ff.unformat(1, "12.5xyz").unwrap(), (12.5, 4));
    assert_eq!(ff.unformat(1, "<bad>").unwrap(), (BAD_VALUE, 5));
    assert_eq!(ff.unformat(1, " < b a d > ").unwrap(), (BAD_VALUE, 11));
    assert_eq!(ff.unformat(1, "xyz").unwrap(), (BAD_VALUE, 0));
    assert_eq!(ff.unformat(1, "").unwrap(), (BAD_VALUE, 0));
}

/// SpecFluxFrame inherits CmpFrame's per-axis Unformat routing in C
/// (cmpframe.c:10041); each external axis is unformatted by its owning
/// component. Both components here use the base contract, so both axes apply
/// it. Oracle-verified (built reference libast, both axes identical).
#[test]
fn test_specfluxframe_unformat_base_contract() {
    let sf = SpecFrame::new(SpecSystem::Freq);
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    let sff = SpecFluxFrame::new(sf, ff);
    for axis in 1..=2 {
        assert_eq!(sff.unformat(axis, "  12.5  ").unwrap(), (12.5, 8));
        assert_eq!(sff.unformat(axis, "<bad>").unwrap(), (BAD_VALUE, 5));
        assert_eq!(sff.unformat(axis, " < b a d > ").unwrap(), (BAD_VALUE, 11));
        assert_eq!(sff.unformat(axis, "12.5xyz").unwrap(), (12.5, 4));
        assert_eq!(sff.unformat(axis, "xyz").unwrap(), (BAD_VALUE, 0));
    }
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

    // SpecFluxFrame inherits CmpFrame's per-axis Format routing in C
    // (cmpframe.c); each external axis is formatted by its owning component,
    // which inherits the base `%1.7G` contract. Oracle-verified.
    assert_eq!(sff.format(1, 1.4e9).unwrap(), "1.4E+09");
    assert_eq!(sff.format(2, 1.0e-26).unwrap(), "1E-26");

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
    // NOTE: this asserts a DELIBERATE divergence from C. The reference
    // library's astEqual returns 0 for two identical SpecFluxFrames (the
    // conversion's flux-scaling RateMap never simplifies to a UnitMap), which
    // looks like a C bug. Rust treats genuinely-identical SpecFluxFrames as
    // equal via the make_sf_mapping identity short-circuit. See
    // docs/issues/c-library-quirks.md ("SpecFluxFrame astEqual ...").
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
fn test_specfluxframe_convert_different_spec_system_converts() {
    // Two SpecFluxFrames differing only in spectral system (Freq vs Wavelen)
    // ARE convertible — the spectral conversion is well-defined and the flux
    // (FLXDN, per Hz) is unchanged. Oracle (astConvert) returns a FrameSet.
    let sf1 = SpecFrame::new(SpecSystem::Freq);
    let sf2 = SpecFrame::new(SpecSystem::Wavelen);
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    let sff1 = SpecFluxFrame::new(sf1, ff.clone());
    let sff2 = SpecFluxFrame::new(sf2, ff);
    let fs = sff1.convert_to(&sff2, &[]).unwrap();
    assert!(fs.is_some());
}

// ---------------------------------------------------------------------------
// Serialisation round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_specfluxframe_serialisation_roundtrip() {
    let sf = SpecFrame::new(SpecSystem::Freq);
    let ff = FluxFrame::new(FluxSystem::FluxDen);
    let sff = SpecFluxFrame::new(sf, ff);
    let obj = AnyObject::SpecFluxFrame(Box::new(sff.clone()));
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
    let obj = AnyObject::SpecFluxFrame(Box::new(sff.clone()));
    let text = to_native_string(&obj).unwrap();

    let obj2 = from_native_string(&text).unwrap();
    let sff2 = obj2.as_frame().unwrap();
    assert!(sff.equal(sff2));
}

// ===========================================================================
// SpecFluxFrame cross-system conversion (MakeSFMapping)
// Ported from reference/starlink-ast/ast_tester/testspecflux.c.
// ===========================================================================

use ast_core::frames::FrameRef;

/// Transform a 2-point (x, v) through the target→result SpecFluxFrame
/// conversion. Returns `(x_out, v_out)`. Panics if no conversion exists.
fn sff_convert(target: &SpecFluxFrame, result: &SpecFluxFrame, x: f64, v: f64) -> (f64, f64) {
    let fs = target
        .convert_to(result, &[])
        .unwrap()
        .expect("SpecFluxFrame conversion");
    let m = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let out = m.transform(&PointSet::from_rows([[x, v]]), true).unwrap();
    (out.point(0)[0], out.point(0)[1])
}

/// The canonical testspecflux.c pair: SpecFrame freq/GHz, target FluxFrame
/// Jy (FLXDN), result FluxFrame default FLXDNW. `spec_frame` axis 1, flux
/// axis 2.
fn jy_sff_pair() -> (SpecFluxFrame, SpecFluxFrame) {
    let mut sf = SpecFrame::new(SpecSystem::Freq);
    sf.set_unit(1, "GHz").unwrap();
    let mut ff = FluxFrame::new(FluxSystem::FluxDen);
    ff.set_unit(1, "Jy").unwrap();
    ff.set_spec_frame(sf.clone());
    ff.set_spec_val(123.0);
    let sff = SpecFluxFrame::new(sf.clone(), ff);

    let mut ff2 = FluxFrame::new(FluxSystem::FluxDenW);
    ff2.set_spec_frame(sf.clone());
    ff2.set_spec_val(123.1);
    let sff2 = SpecFluxFrame::new(sf, ff2);
    (sff, sff2)
}

#[test]
fn test_specflux_jy_to_fluxdenw() {
    // testspecflux.c error 8-10: (x=2 GHz, v=1 Jy) -> (x=2, v=1.33425638e-26).
    let (sff, sff2) = jy_sff_pair();
    let (x, v) = sff_convert(&sff, &sff2, 2.0, 1.0);
    assert!((x - 2.0).abs() < 1.0e-9, "spectral axis changed: {x}");
    assert!(
        (v - 1.33425638e-26).abs() < 1.0e-32,
        "expected 1.33425638e-26, got {v:e}"
    );
}

#[test]
fn test_specflux_inverse() {
    // The inverse maps (x=2, v=1.33425638e-26) back to (2, 1).
    let (sff, sff2) = jy_sff_pair();
    let fs = sff.convert_to(&sff2, &[]).unwrap().unwrap();
    let m = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
    let fwd = m
        .transform(&PointSet::from_rows([[2.0, 1.0]]), true)
        .unwrap();
    let back = m.transform(&fwd, false).unwrap();
    assert!((back.point(0)[0] - 2.0).abs() < 1.0e-9);
    // The flux scaling on the inverse is recovered through a RateMap (finite
    // differencing of the spectral conversion), so the round-trip carries
    // that method's precision (~1e-8 here), not bit-exactness.
    assert!(
        (back.point(0)[1] - 1.0).abs() < 1.0e-6,
        "round-trip flux {} != 1.0",
        back.point(0)[1]
    );
}

#[test]
fn test_specflux_log_units() {
    // testspecflux.c error 18-20: result FluxFrame Unit=log(W/m2/nm) (FLXDNW).
    // (x=2, v=1) -> (2, -24.8747607).
    let mut sf = SpecFrame::new(SpecSystem::Freq);
    sf.set_unit(1, "GHz").unwrap();
    let mut ff = FluxFrame::new(FluxSystem::FluxDen);
    ff.set_unit(1, "Jy").unwrap();
    ff.set_spec_frame(sf.clone());
    ff.set_spec_val(123.0);
    let sff = SpecFluxFrame::new(sf.clone(), ff);

    let mut ff2 = FluxFrame::new(FluxSystem::FluxDenW);
    ff2.set_unit(1, "log(W/m2/nm)").unwrap();
    ff2.set_spec_frame(sf.clone());
    let sff2 = SpecFluxFrame::new(sf, ff2);

    let (x, v) = sff_convert(&sff, &sff2, 2.0, 1.0);
    assert!((x - 2.0).abs() < 1.0e-9);
    assert!(
        (v + 24.8747607).abs() < 1.0e-6,
        "expected -24.8747607, got {v}"
    );
}

#[test]
fn test_specflux_surface_brightness() {
    // testspecflux.c error 24-27: SFCBR (Jy/deg**2) -> SFCBRW (log(W/m2/nm/sr)).
    // (x=2, v=1) -> (2, -21.3585154).
    let mut sf = SpecFrame::new(SpecSystem::Freq);
    sf.set_unit(1, "GHz").unwrap();
    let mut ff = FluxFrame::new(FluxSystem::SBright);
    ff.set_unit(1, "Jy/deg**2").unwrap();
    ff.set_spec_frame(sf.clone());
    ff.set_spec_val(123.0);
    let sff = SpecFluxFrame::new(sf.clone(), ff);

    let mut ff2 = FluxFrame::new(FluxSystem::SBrightW);
    ff2.set_unit(1, "log(W/m2/nm/sr)").unwrap();
    ff2.set_spec_frame(sf.clone());
    let sff2 = SpecFluxFrame::new(sf, ff2);

    let (x, v) = sff_convert(&sff, &sff2, 2.0, 1.0);
    assert!((x - 2.0).abs() < 1.0e-9);
    assert!(
        (v + 21.3585154).abs() < 1.0e-6,
        "expected -21.3585154, got {v}"
    );
}

#[path = "flux_unit_system_oracle/cases.rs"]
mod flux_unit_system_cases;

mod unit_system {
    //! A FluxFrame whose System follows from its Unit, checked against the C
    //! library (`flux_unit_system_oracle/cases.rs`, generated by
    //! `tools/oracle_fixtures/flux_unit_system_probe.c`). C reads SpecVal in
    //! the units of the density System derived from the Unit
    //! (fluxframe.c:1235 `GetDensitySystem`).

    use super::flux_unit_system_cases as cases;
    use ast_core::coords::PointSet;
    use ast_core::frames::{FluxFrame, FluxSystem, Frame, FrameRef};

    #[test]
    fn system_from_unit_matches_the_c_library() {
        // astFluxFrame( 5000.0, NULL, "Unit=W/m^2/Angstrom" ): a default
        // SpecVal, and no System.
        let mut a = FluxFrame::with_spec_val(5000.0, None);
        a.set_unit(1, "W/m^2/Angstrom").unwrap();
        assert_eq!(a.system_string(), Some(cases::SYSTEM));
        assert_eq!(a.title(), cases::TITLE);

        // astFluxFrame( 5.99584916e14, NULL, "System=FLXDN" ).
        let mut b = FluxFrame::with_spec_val(5.99584916e14, None);
        b.set_system(FluxSystem::FluxDen);
        let fs = a.convert_to(&b, &[]).unwrap().expect("converts");
        let map = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
        let out = map
            .transform(&PointSet::from_columns(&[&[1.0, 3.5]]).unwrap(), true)
            .unwrap();
        assert_eq!([out.point(0)[0], out.point(1)[0]], cases::CONVERTED);
    }
}
