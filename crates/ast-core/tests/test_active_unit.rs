//! ActiveUnit unit adaptation in SubFrame/Match (frame.c:11062-11108).

use ast_core::frames::{BasicFrameBuilder, Frame};

#[test]
fn active_unit_inserts_scale_for_km_to_m() {
    // Template in metres, target in km, both ActiveUnit -> the match
    // mapping (target -> result) scales km values into the result's
    // metres. astUnitMapper("km","m") transforms km into m, so the
    // forward (target -> result) maps 2 km to 2000 m.
    let mut target = BasicFrameBuilder::new(1).unit(1, "km").build().unwrap();
    target.set_active_unit(true);
    let mut template = BasicFrameBuilder::new(1).unit(1, "m").build().unwrap();
    template.set_active_unit(true);

    let m = template.match_template(&target, false).unwrap().unwrap();
    // Apply forward: 2 km -> 2000 m.
    let pts = ast_core::coords::PointSet::from_rows([[2.0_f64]]);
    let out = m.map.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 2000.0).abs() < 1e-9);
}

#[test]
fn active_unit_off_ignores_units() {
    // ActiveUnit not set -> units ignored, mapping is identity.
    let target = BasicFrameBuilder::new(1).unit(1, "km").build().unwrap();
    let template = BasicFrameBuilder::new(1).unit(1, "m").build().unwrap();
    let m = template.match_template(&target, false).unwrap().unwrap();
    let pts = ast_core::coords::PointSet::from_rows([[2.0_f64]]);
    let out = m.map.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 2.0).abs() < 1e-12);
}

#[test]
fn active_unit_multi_axis_scales_per_axis() {
    // Template in m/s, target in km/ms, both ActiveUnit. Both axes have
    // units that map: astUnitMapper("km","m") scales by 1e3 and
    // astUnitMapper("ms","s") scales by 1e-3. The per-axis units tail of
    // SubFrame (frame.c:11062-11154) builds a mapping that applies each
    // axis scale independently, so the forward maps (2, 2) -> (2000,
    // 0.002).
    let mut target = BasicFrameBuilder::new(2)
        .unit(1, "km")
        .unit(2, "ms")
        .build()
        .unwrap();
    target.set_active_unit(true);
    let mut template = BasicFrameBuilder::new(2)
        .unit(1, "m")
        .unit(2, "s")
        .build()
        .unwrap();
    template.set_active_unit(true);

    let m = template.match_template(&target, false).unwrap().unwrap();
    let pts = ast_core::coords::PointSet::from_rows([[2.0_f64, 2.0_f64]]);
    let out = m.map.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 2000.0).abs() < 1e-9);
    assert!((out.point(0)[1] - 0.002).abs() < 1e-12);
}

#[test]
fn active_unit_aborts_whole_match_when_one_axis_unmappable() {
    // Two-axis case, both ActiveUnit. Axis 1 maps (km -> m), but axis 2
    // is dimensionally incompatible: target "m" (length) cannot map to
    // template "s" (time). Per frame.c:11062-11154 a single failed
    // astUnitMapper aborts the WHOLE match, so match_template returns
    // Ok(None) rather than a partial mapping.
    let mut target = BasicFrameBuilder::new(2)
        .unit(1, "km")
        .unit(2, "m")
        .build()
        .unwrap();
    target.set_active_unit(true);
    let mut template = BasicFrameBuilder::new(2)
        .unit(1, "m")
        .unit(2, "s")
        .build()
        .unwrap();
    template.set_active_unit(true);

    let res = template.match_template(&target, false).unwrap();
    assert!(res.is_none());
}

#[test]
fn overlay_transfers_active_unit() {
    let mut result = BasicFrameBuilder::new(1).build().unwrap();
    let mut template = BasicFrameBuilder::new(1).build().unwrap();
    template.set_active_unit(true);
    result.overlay_template(&template, &[0]).unwrap();
    assert!(result.get_active_unit());
}
