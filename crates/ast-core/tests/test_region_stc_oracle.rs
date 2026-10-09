//! `StcResourceProfile` against the C library (stc.c, stcresourceprofile.c):
//! the dumps of a plain, a negated and a sky Stc whose AstroCoords Error
//! Region is given in another SkyFrame, of an Stc made from an Stc whose
//! base->current Mapping is a ZoomMap, and of that mapped Stc simplified; two
//! test points transformed by each and whether it is bounded; its overlap with
//! a Circle each way round; the AstroCoords element `get_stc_coord` gives; and
//! the errors for an AstroCoords index out of range and an unknown key.
//!
//! Each Stc is built through the public API and compared with C's, and each of
//! C's dumps is read back and written again as C writes it again: a Region in
//! an AstroCoords element loaded without a Frame of its own is stored again
//! with its comment, which changes the KeyMap's dump. The values come from
//! `tools/oracle_fixtures/region_stc_probe.c`.

// Generated; formatted by its generator rather than rustfmt. Its values are
// C's to 17 significant digits, which is more than some of them need.
#[rustfmt::skip]
#[allow(clippy::excessive_precision)]
#[path = "region_stc_oracle/cases.rs"]
mod cases;

use ast_core::coords::PointSet;
use ast_core::error::ErrorCode;
use ast_core::frames::{BasicFrameBuilder, Frame, SkyFrame, SkySystem};
use ast_core::keymap::KeyMap;
use ast_core::mappings::{Mapping, ZoomMap};
use ast_core::regions::{
    CircleBuilder, DynRegion, Overlap, Region, STC_ERROR, STC_NAME, StcResourceProfile,
    StcResourceProfileBuilder,
};
use ast_core::{AnyObject, from_native_string, to_native_string};

fn circle(frame: impl Frame + 'static, centre: [f64; 2], radius: f64) -> DynRegion {
    Box::new(
        CircleBuilder::new(frame, centre.to_vec())
            .unwrap()
            .radius(radius)
            .build()
            .unwrap(),
    )
}

fn plain() -> StcResourceProfile {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    StcResourceProfileBuilder::from_dyn(circle(frame, [0.0, 0.0], 1.0))
        .build()
        .unwrap()
}

fn sky() -> StcResourceProfile {
    let err = circle(SkyFrame::new(SkySystem::Galactic), [2.0, -0.3], 0.0001);
    let mut coord = KeyMap::new();
    coord.put_1c(STC_NAME, &["RA", "Dec"], None).unwrap();
    coord
        .put_0a(
            STC_ERROR,
            AnyObject::from_mapping(err.as_ref() as &dyn Mapping).unwrap(),
            None,
        )
        .unwrap();
    let region = circle(SkyFrame::unset_system(), [1.0, 0.5], 0.01);
    StcResourceProfileBuilder::from_dyn(region)
        .coords(vec![coord])
        .build()
        .unwrap()
}

fn mapped() -> DynRegion {
    let mut zoomed = BasicFrameBuilder::new(2).build().unwrap();
    Frame::set_domain(&mut zoomed, "ZOOMED").unwrap();
    plain()
        .reg_map_region(&ZoomMap::new(2, 2.0).unwrap(), &zoomed)
        .unwrap()
}

/// The Stc each case names, built through the public API.
fn built(name: &str) -> DynRegion {
    match name {
        "plain" => Box::new(plain()),
        "negated" => {
            // C negates a copy (astCopy) of the plain Stc.
            let mut stc = plain().copy_region();
            stc.negate();
            stc
        }
        "sky" => Box::new(sky()),
        "mapped" => mapped(),
        "from_mapped" => Box::new(
            StcResourceProfileBuilder::from_dyn(mapped())
                .build()
                .unwrap(),
        ),
        "mapped_simplified" => {
            let simplified = (mapped().as_ref() as &dyn Mapping).simplify().unwrap();
            AnyObject::from_mapping(simplified.as_ref())
                .unwrap()
                .into_region()
                .unwrap()
        }
        other => panic!("no case {other}"),
    }
}

fn dump(region: &dyn Region) -> String {
    to_native_string(&AnyObject::from_mapping(region as &dyn Mapping).unwrap()).unwrap()
}

#[test]
fn stc_matches_c() {
    for &(name, want, _, points, bounded) in cases::CASES {
        let stc = built(name);
        assert_eq!(dump(stc.as_ref()), want, "{name}: dump");

        let input = PointSet::from_rows(points.iter().map(|p| &p[..2]));
        let out = stc.transform(&input, true).unwrap();
        for (i, p) in points.iter().enumerate() {
            assert_eq!(out.point(i), &p[2..], "{name}: point {i}");
        }
        assert_eq!(stc.is_bounded().unwrap(), bounded, "{name}: bounded");
    }
}

#[test]
fn c_dumps_read_back_as_c_reads_them() {
    for &(name, first, want, _, _) in cases::CASES {
        let region = from_native_string(first).unwrap().into_region().unwrap();
        assert_eq!(dump(region.as_ref()), want, "{name}");
    }
}

fn code(overlap: Overlap) -> i32 {
    match overlap {
        Overlap::Incomparable => 0,
        Overlap::None => 1,
        Overlap::FirstInside => 2,
        Overlap::SecondInside => 3,
        Overlap::Partial => 4,
        Overlap::Same => 5,
        Overlap::Negation => 6,
    }
}

#[test]
fn overlap_with_a_circle_matches_c() {
    let stc = plain();
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let big = circle(frame, [0.0, 0.0], 2.0);
    assert_eq!(
        code(stc.overlap(big.as_ref()).unwrap()),
        cases::OVERLAP_STC_CIRCLE
    );
    assert_eq!(code(big.overlap(&stc).unwrap()), cases::OVERLAP_CIRCLE_STC);
}

#[test]
fn astro_coords_match_c() {
    let stc = sky();
    assert_eq!(stc.get_stc_n_coord() as i32, cases::SKY_NCOORD);
    let coord = stc.get_stc_coord(1).unwrap();
    let got = to_native_string(&AnyObject::KeyMap(Box::new(coord))).unwrap();
    assert_eq!(got, cases::SKY_COORD);
    assert_eq!(stc.region_class(), "Circle");

    let err = stc.get_stc_coord(2).unwrap_err();
    assert_eq!(err.code() as i32, cases::COORD_INDEX);
    assert_eq!(
        err.message(),
        "astGetStcCoord(StcResourceProfile): Supplied AstroCoords index (2) is invalid."
    );
}

#[test]
fn unknown_astro_coords_key_is_stckey() {
    let mut bad = KeyMap::new();
    let err_region = circle(SkyFrame::new(SkySystem::Galactic), [2.0, -0.3], 0.0001);
    bad.put_0a(
        "Colour",
        AnyObject::from_mapping(err_region.as_ref() as &dyn Mapping).unwrap(),
        None,
    )
    .unwrap();
    let region = circle(SkyFrame::unset_system(), [1.0, 0.5], 0.01);
    let err = StcResourceProfileBuilder::from_dyn(region)
        .coords(vec![bad])
        .build()
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::Stckey);
    assert_eq!(err.code() as i32, cases::COORD_KEY);
    assert_eq!(
        err.message(),
        "astInitStc(StcResourceProfile): Unknown key \"Colour\" supplied in an AstroCoords list."
    );
}
