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
fn overlay_transfers_active_unit() {
    let mut result = BasicFrameBuilder::new(1).build().unwrap();
    let mut template = BasicFrameBuilder::new(1).build().unwrap();
    template.set_active_unit(true);
    result.overlay_template(&template, &[0]).unwrap();
    assert!(result.get_active_unit());
}
