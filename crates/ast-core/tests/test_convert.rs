//! Acceptance test for `astConvert` / `astFindFrame`, ported from
//! `reference/starlink-ast/ast_tester/testconvert.c`.
//!
//! Exercises the matchsub cast-retry (`Frame[Domain=SKY]` <-> `SkyFrame`
//! => `UnitMap`), domain gating (a non-SKY plain Frame does not convert
//! to a SkyFrame), CmpFrame partition alignment (`Frame` <-> `CmpFrame`
//! and `SkyFrame` <-> `CmpFrame` => `PermMap`), and `astFindFrame`
//! MaxAxes/MinAxes widening.
//!
//! The C ancestor asserts the base/current frame equality with `astEqual`
//! and the connecting Mapping class with `astIsAUnitMap` / `astIsAPermMap`
//! for each pairing; this port asserts the same. The authoritative map
//! classes were confirmed by building and running the C `testconvert.c`
//! against the reference libast ("All astConvert tests passed").

use ast_core::frames::{
    BasicFrameBuilder, CmpFrame, Frame, FrameRef, SkyFrame, SkySystem, SpecFrame, SpecSystem,
};
use ast_core::mappings::{Mapping, PermMap, UnitMap};

fn is_unit(m: &dyn Mapping) -> bool {
    m.as_any().downcast_ref::<UnitMap>().is_some()
}
fn is_perm(m: &dyn Mapping) -> bool {
    m.as_any().downcast_ref::<PermMap>().is_some()
}

/// `astConvert(bf[Domain=SKY], sf)` and the reverse both yield a UnitMap,
/// with the base/current frames preserved (testconvert.c Errors 1-8).
#[test]
fn convert_basic_sky_unitmap() {
    let sf = SkyFrame::new(SkySystem::Fk5);
    let bf = BasicFrameBuilder::new(2).domain("SKY").build().unwrap();

    // astConvert(bf, sf) => base==bf, current==sf, mapping is UnitMap.
    let fs = bf.convert_to(&sf, &[]).unwrap().expect("bf->sf converts");
    assert!(fs.frame(FrameRef::Base).unwrap().equal(&bf));
    assert!(fs.frame(FrameRef::Current).unwrap().equal(&sf));
    assert!(is_unit(
        fs.mapping(FrameRef::Base, FrameRef::Current)
            .unwrap()
            .as_ref()
    ));

    // astConvert(sf, bf) => base==sf, current==bf, mapping is UnitMap.
    let fs = sf.convert_to(&bf, &[]).unwrap().expect("sf->bf converts");
    assert!(fs.frame(FrameRef::Base).unwrap().equal(&sf));
    assert!(fs.frame(FrameRef::Current).unwrap().equal(&bf));
    assert!(is_unit(
        fs.mapping(FrameRef::Base, FrameRef::Current)
            .unwrap()
            .as_ref()
    ));
}

/// A plain Frame whose Domain is set to something other than SKY does not
/// convert to a SkyFrame in either direction (testconvert.c Errors 9-10).
#[test]
fn convert_domain_gate_blocks_mismatch() {
    let sf = SkyFrame::new(SkySystem::Fk5);
    let bf = BasicFrameBuilder::new(2).domain("NOTSKY").build().unwrap();
    assert!(bf.convert_to(&sf, &[]).unwrap().is_none());
    assert!(sf.convert_to(&bf, &[]).unwrap().is_none());
}

/// `astConvert(bf[Domain=SKY], cf)` and the reverse yield a PermMap,
/// with the base/current frames preserved (testconvert.c Errors 29-36).
/// A Frame with no SKY-compatible Domain does not convert to the CmpFrame
/// (Errors 27-28).
#[test]
fn convert_basic_cmp_permmap() {
    let sf = SkyFrame::new(SkySystem::Fk5);
    let df = SpecFrame::new(SpecSystem::Freq);
    let cf = CmpFrame::new(Box::new(df), Box::new(sf));

    // Domain=NOTSKY blocks the conversion in both directions.
    let bf_notsky = BasicFrameBuilder::new(2).domain("NOTSKY").build().unwrap();
    assert!(bf_notsky.convert_to(&cf, &[]).unwrap().is_none());
    assert!(cf.convert_to(&bf_notsky, &[]).unwrap().is_none());

    // Domain=SKY converts; the Mapping is a PermMap (the CmpFrame holds
    // the SkyFrame on axes 2-3, so the alignment permutes axes).
    let bf = BasicFrameBuilder::new(2).domain("SKY").build().unwrap();

    let fs = bf.convert_to(&cf, &[]).unwrap().expect("bf->cf converts");
    assert!(fs.frame(FrameRef::Base).unwrap().equal(&bf));
    assert!(fs.frame(FrameRef::Current).unwrap().equal(&cf));
    assert!(is_perm(
        fs.mapping(FrameRef::Base, FrameRef::Current)
            .unwrap()
            .as_ref()
    ));

    let fs = cf.convert_to(&bf, &[]).unwrap().expect("cf->bf converts");
    assert!(fs.frame(FrameRef::Base).unwrap().equal(&cf));
    assert!(fs.frame(FrameRef::Current).unwrap().equal(&bf));
    assert!(is_perm(
        fs.mapping(FrameRef::Base, FrameRef::Current)
            .unwrap()
            .as_ref()
    ));
}

/// `astConvert(sf, cf)` and the reverse yield a PermMap, with the
/// base/current frames preserved (testconvert.c Errors 37-44).
#[test]
fn convert_sky_cmp_permmap() {
    let sf = SkyFrame::new(SkySystem::Fk5);
    let df = SpecFrame::new(SpecSystem::Freq);
    let cf = CmpFrame::new(Box::new(df), Box::new(sf.clone()));

    let fs = sf.convert_to(&cf, &[]).unwrap().expect("sf->cf converts");
    assert!(fs.frame(FrameRef::Base).unwrap().equal(&sf));
    assert!(fs.frame(FrameRef::Current).unwrap().equal(&cf));
    assert!(is_perm(
        fs.mapping(FrameRef::Base, FrameRef::Current)
            .unwrap()
            .as_ref()
    ));

    let fs = cf.convert_to(&sf, &[]).unwrap().expect("cf->sf converts");
    assert!(fs.frame(FrameRef::Base).unwrap().equal(&cf));
    assert!(fs.frame(FrameRef::Current).unwrap().equal(&sf));
    assert!(is_perm(
        fs.mapping(FrameRef::Base, FrameRef::Current)
            .unwrap()
            .as_ref()
    ));
}

/// `astFindFrame` with a SkyFrame template: it finds the SkyFrame inside
/// a CmpFrame target only after MaxAxes/MinAxes are widened so the 2-axis
/// template tolerates a 3-axis target (testconvert.c Errors 45-50).
#[test]
fn find_frame_cmp_needs_widened_axes() {
    let sf = SkyFrame::new(SkySystem::Fk5);
    let df = SpecFrame::new(SpecSystem::Freq);
    let cf = CmpFrame::new(Box::new(df), Box::new(sf.clone()));

    // astFindFrame(sf, cf): a CmpFrame template against a SkyFrame target
    // is found (the SkyFrame supplies a sky axis pair).
    assert!(sf.find_frame_template(&cf, " ").unwrap().is_some());

    // astFindFrame(cf, sf): a default SkyFrame template (MaxAxes=MinAxes=2)
    // will not match a 3-axis CmpFrame target.
    assert!(cf.find_frame_template(&sf, " ").unwrap().is_none());

    // Widen the SkyFrame template's axis tolerance => match found, base==cf,
    // current==sf, mapping is a PermMap.
    let mut sf2 = SkyFrame::new(SkySystem::Fk5);
    sf2.set_max_axes(3).unwrap();
    sf2.set_min_axes(1).unwrap();
    let fs = cf
        .find_frame_template(&sf2, " ")
        .unwrap()
        .expect("widened template matches");
    assert!(fs.frame(FrameRef::Base).unwrap().equal(&cf));
    assert!(fs.frame(FrameRef::Current).unwrap().equal(&sf2));
    assert!(is_perm(
        fs.mapping(FrameRef::Base, FrameRef::Current)
            .unwrap()
            .as_ref()
    ));
}

/// `astFindFrame(target=Frame[Domain=ARDAPP], template=SkyFrame[GAPPT])`
/// returns None: the plain target's Domain does not match, and the
/// cast-retry does not produce a match (testconvert.c Error 51).
#[test]
fn find_frame_domain_mismatch_returns_none() {
    let target = BasicFrameBuilder::new(2).domain("ARDAPP").build().unwrap();
    let template = SkyFrame::new(SkySystem::Gappt);
    assert!(
        target
            .find_frame_template(&template, " ")
            .unwrap()
            .is_none()
    );
}

/// A constant in a units string is a coefficient wherever it appears:
/// `"2/m"` means "each unit is 2 per metre", the same as `"2*m**-1"`, so a
/// value of 1 per metre is 0.5 in either (testconvert.c Errors 52-53).
#[test]
fn convert_constant_numerator_units() {
    let cases = [
        ("1/m", "2/m", 0.5),
        ("1/m", "2*m**-1", 0.5),
        ("1/m", "0.5/m", 2.0),
        ("Hz", "2/s", 0.5),
    ];
    for (from, to, expect) in cases {
        let mut f1 = BasicFrameBuilder::new(1).unit(1, from).build().unwrap();
        let mut f2 = BasicFrameBuilder::new(1).unit(1, to).build().unwrap();
        f1.set_active_unit(true);
        f2.set_active_unit(true);
        let fs = f1
            .convert_to(&f2, &[])
            .unwrap()
            .unwrap_or_else(|| panic!("no conversion from {from} to {to}"));
        let map = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();
        let out = map.tran1(&[1.0], true).unwrap();
        assert!(
            (out[0] - expect).abs() <= 1.0e-12,
            "1 {from} is {} {to}, expected {expect}",
            out[0]
        );
    }
}
