//! Interval membership, Bounded and boundary mesh against the C library
//! (interval.c `Transform`, `GetBounded`, `RegBaseMesh`): an Interval that
//! is equivalent to a Box is tested, bounded and meshed as that Box; one
//! that is not tests each axis, separating equal limits by the width of its
//! uncertainty; and an Interval excluding every axis is not bounded.
//!
//! The values come from `tools/oracle_fixtures/region_interval_probe.c`.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_interval_oracle/cases.rs"]
mod cases;

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::frames::{BasicFrameBuilder, Frame, SkyFrame, SkySystem};
use starlink_ast::regions::{DynRegion, IntervalBuilder, Region};

fn interval(frame: impl Frame + 'static, lbnd: [f64; 2], ubnd: [f64; 2]) -> DynRegion {
    Box::new(
        IntervalBuilder::new(frame, lbnd.to_vec(), ubnd.to_vec())
            .unwrap()
            .build()
            .unwrap(),
    )
}

fn f2() -> impl Frame + 'static {
    BasicFrameBuilder::new(2).build().unwrap()
}

fn membership(reg: &dyn Region, x: &[f64], y: &[f64]) -> String {
    let (xo, _) = reg.tran2(x, y, true).unwrap();
    xo.iter()
        .map(|&v| if v == BAD_VALUE { '0' } else { '1' })
        .collect()
}

#[test]
fn membership_and_bounded_match_the_c_library() {
    let x15 = [1.5; 4];
    for &(name, want, bounded) in cases::CASES {
        let (reg, got) = match name {
            "bounded_slice" => {
                let r = interval(f2(), [1.0, 5.0], [2.0, 5.0]);
                let m = membership(r.as_ref(), &x15, &cases::ZERO_Y);
                (r, m)
            }
            "unbounded_slice" => {
                let r = interval(f2(), [1.0, 5.0], [BAD_VALUE, 5.0]);
                let m = membership(r.as_ref(), &x15, &cases::ZERO_Y);
                (r, m)
            }
            "unbounded_slice_negated_open" => {
                let mut r = interval(f2(), [1.0, 5.0], [BAD_VALUE, 5.0]);
                r.set_closed(false);
                r.negate();
                let m = membership(r.as_ref(), &x15, &cases::ZERO_Y);
                (r, m)
            }
            "sky" => {
                let r = interval(SkyFrame::new(SkySystem::Icrs), [6.2, 0.1], [6.4, 0.3]);
                let m = membership(r.as_ref(), &cases::SKY_X, &cases::SKY_Y);
                (r, m)
            }
            "all_axes_excluded" => {
                let r = interval(f2(), [2.0, 2.0], [1.0, 1.0]);
                let m = membership(r.as_ref(), &cases::EXCLUDED_XY, &cases::EXCLUDED_XY);
                (r, m)
            }
            other => panic!("no Rust construction for case {other}"),
        };
        assert_eq!(got, want, "{name}: membership");
        assert_eq!(reg.is_bounded().unwrap(), bounded, "{name}: Bounded");
    }
}

#[test]
fn mesh_matches_the_c_library() {
    let reg = interval(f2(), [0.0, 0.0], [1.0, 1.0]);
    let mesh = reg.region_mesh().unwrap();
    let n = mesh.npoint();
    let (first, last) = (mesh.point(0), mesh.point(n - 1));
    assert_eq!((n, [first[0], first[1], last[0], last[1]]), cases::MESH);
}
