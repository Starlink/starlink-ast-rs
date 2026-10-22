//! `FrameSearch::convert_to` between a SkyFrame and a FrameSet whose current Frame
//! is GRID and whose base Frame is an ICRS SkyFrame, each way round, against
//! the C library. C dispatches `astConvertX` on the class of the target: to a
//! FrameSet, frameset.c searches all of its Frames; from a FrameSet to a
//! plain Frame, frame.c's driver tries the FrameSet as the template, and its
//! `Match` searches its Frames. Either way the route runs through the
//! FrameSet's SkyFrame.
//!
//! The values come from `tools/oracle_fixtures/convert_frameset_probe.c`.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[allow(clippy::excessive_precision)]
#[path = "convert_frameset_oracle/cases.rs"]
mod cases;

use ast_core::coords::PointSet;
use ast_core::frames::{BasicFrameBuilder, FrameRef, FrameSearch, FrameSet, SkyFrame, SkySystem};
use ast_core::mappings::{Mapping, ZoomMap};

fn frameset() -> FrameSet {
    let mut fs = FrameSet::new(Box::new(SkyFrame::new(SkySystem::Icrs)));
    let mut grid = BasicFrameBuilder::new(2).build().unwrap();
    grid.set_domain("GRID");
    fs.add_frame(
        FrameRef::Base,
        Box::new(ZoomMap::new(2, 0.5).unwrap()),
        Box::new(grid),
    )
    .unwrap();
    fs
}

fn check(cvt: Option<FrameSet>, case: cases::Case, what: &str) {
    let (found, domain, [fwd, inv]) = case;
    assert_eq!(cvt.is_some(), found, "{what}: route found");
    let cvt = cvt.unwrap();
    assert_eq!(cvt.current().domain(), domain, "{what}: current Domain");
    let pts = PointSet::from_rows([[1.0, 0.5], [2.5, -0.3]].iter().map(|p| p.as_slice()));
    for (forward, want) in [(true, fwd), (false, inv)] {
        let got = cvt.transform(&pts, forward).unwrap();
        for (i, w) in want.iter().enumerate() {
            crate::common::assert_c_values(
                &got.point(i),
                w.as_slice(),
                &format!("{what}: forward={forward} point {i}"),
            );
        }
    }
}

#[test]
fn frame_to_frameset_searches_its_frames() {
    let mut galactic = SkyFrame::new(SkySystem::Galactic);
    let cvt = galactic.convert_to(&mut frameset(), &[]).unwrap();
    check(cvt, cases::FRAME_TO_FRAMESET, "Frame to FrameSet");
}

#[test]
fn frameset_to_frame_searches_its_frames() {
    let mut galactic = SkyFrame::new(SkySystem::Galactic);
    let cvt = frameset().convert_to(&mut galactic, &[]).unwrap();
    check(cvt, cases::FRAMESET_TO_FRAME, "FrameSet to Frame");
}
