//! Read-path parity test for offset-sky FITS headers (`SREFIS = 'ORIGIN'`).
//!
//! Ported goal: `skyref.head` describes an offset SkyFrame (SREF sets the
//! offset origin, SREFIS selects ORIGIN). C `TidyOffsets`
//! (reference/starlink-ast `src/fitschan.c:34533-34537`) applies `SkyRefIs`
//! *through the FrameSet*, so the FrameSet integrity machinery re-maps the
//! SkyFrame to offset coordinates and the resulting pixel->sky Mapping carries
//! the offset-rotation (the direction-cosine MatrixMap seen in the C dump).
//! This test transforms the golden's input pixel coordinates through the live
//! Rust pixel->sky Mapping and asserts the results match the C oracle.
//!
//! The golden values are copied verbatim from the `[skyref.head ... forward]`
//! section of `crates/ast-core/tests/fixtures/oracle/headers.oracle`.

// Golden literals carry the oracle's full printed precision; trimming digits
// to the nearest representable f64 would no longer be a verbatim copy.
#![allow(clippy::excessive_precision)]

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::coords::PointSet;
use ast_core::fitschan::FitsChan;
use ast_core::frames::FrameRef;

const SKYREF_HEAD: &str = include_str!("fixtures/wcsconv/inputs/skyref.head");

/// (pixel_x, pixel_y) -> (sky_lon_rad, sky_lat_rad), verbatim from the
/// `[skyref.head  nin=2 nout=2 dir=forward]` oracle section.
const GOLDEN_FWD: &[(f64, f64, f64, f64)] = &[
    (1.0, 1.0, 1.3572788122720236, -0.93489573571002094),
    (100.0, 100.0, 1.3582689494065399, -0.93455683719985727),
    (50.5, 50.5, 1.3577739944402782, -0.93472634501576091),
    (50.5, 34.0, 1.3576439450668489, -0.93470556754749412),
    (25.75, 67.0, 1.3578516070937678, -0.93486300178244763),
];

#[test]
fn skyref_offset_mapping_matches_oracle() {
    let mut fc = FitsChan::from_header(SKYREF_HEAD).expect("from_header");
    let obj = fc.read().expect("read").expect("non-null object");
    let fs = match obj {
        AnyObject::FrameSet(fs) => fs,
        other => panic!("expected FrameSet, got {other:?}"),
    };

    // Base (GRID) -> Current pixel->sky Mapping. After TidyOffsets the offset
    // Frame's SkyRefIs is applied through the FrameSet, so this Mapping must
    // rotate into the offset sky system.
    let map = fs
        .mapping(FrameRef::Base, FrameRef::Current)
        .expect("base->current mapping");

    let rows: Vec<Vec<f64>> = GOLDEN_FWD.iter().map(|(x, y, _, _)| vec![*x, *y]).collect();
    let pts = PointSet::from_rows(rows);
    let out = map.transform(&pts, true).expect("forward transform");

    for (i, (x, y, want_lon, want_lat)) in GOLDEN_FWD.iter().enumerate() {
        let p = out.point(i);
        let dl = (p[0] - want_lon).abs();
        let db = (p[1] - want_lat).abs();
        assert!(
            dl < 1e-9 && db < 1e-9,
            "pixel ({x},{y}): got ({}, {}) want ({want_lon}, {want_lat}) \
             (dlon={dl:.3e} dlat={db:.3e})",
            p[0],
            p[1],
        );
    }
}
