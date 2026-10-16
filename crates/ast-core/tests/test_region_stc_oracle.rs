//! `StcResourceProfile` against the C library (stc.c, stcresourceprofile.c):
//! the dumps of a plain, a negated and a sky Stc whose AstroCoords Error
//! Region is given in another SkyFrame, of an Stc made from an Stc whose
//! base->current Mapping is a ZoomMap, and of that mapped Stc simplified; two
//! test points transformed by each and whether it is bounded; its overlap with
//! a Circle each way round; the AstroCoords element `get_stc_coord` gives; and
//! the errors for an AstroCoords index out of range and an unknown key. The
//! other Stc classes too: an `StcSearchLocation`, an `StcCatalogEntryLocation`
//! with the sky AstroCoords, and an `StcObsDataLocation` with an observatory
//! position and without one.
//!
//! Each Stc is built through the public API and compared with C's, and each of
//! C's dumps is read back and written again as C writes it again: a Region in
//! an AstroCoords element loaded without a Frame of its own is stored again
//! with its comment, which changes the KeyMap's dump. Each is also written
//! with Full = 1, which writes every unset value, less C's unset `RefCnt` and
//! `Nobj`, counts the port does not have. The values come from
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
    CircleBuilder, DynRegion, Overlap, PointListBuilder, Region, STC_ERROR, STC_NAME,
    StcCatalogEntryLocationBuilder, StcObsDataLocationBuilder, StcResourceProfile,
    StcResourceProfileBuilder, StcSearchLocationBuilder,
};
use ast_core::{AnyObject, Channel, NativeChannel, from_native_string, to_native_string};

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

fn plain_circle() -> DynRegion {
    circle(BasicFrameBuilder::new(2).build().unwrap(), [0.0, 0.0], 1.0)
}

fn sky_coord() -> KeyMap {
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
    coord
}

fn sky_circle() -> DynRegion {
    circle(SkyFrame::unset_system(), [1.0, 0.5], 0.01)
}

fn sky() -> StcResourceProfile {
    StcResourceProfileBuilder::from_dyn(sky_circle())
        .coords(vec![sky_coord()])
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
        "search" => Box::new(
            StcSearchLocationBuilder::from_dyn(plain_circle())
                .build()
                .unwrap(),
        ),
        "catalog_entry" => Box::new(
            StcCatalogEntryLocationBuilder::from_dyn(sky_circle())
                .coords(vec![sky_coord()])
                .build()
                .unwrap(),
        ),
        "obs_data_without_obs" => Box::new(
            StcObsDataLocationBuilder::from_dyn(plain_circle())
                .build()
                .unwrap(),
        ),
        "obs_data" => {
            let mut stc = StcObsDataLocationBuilder::from_dyn(plain_circle())
                .build()
                .unwrap();
            let mut geo = BasicFrameBuilder::new(3).build().unwrap();
            Frame::set_domain(&mut geo, "GEO").unwrap();
            let obs = PointListBuilder::new(geo, vec![vec![1.0, 2.0, 3.0]])
                .unwrap()
                .build()
                .unwrap();
            stc.stc_set_obs(Some(obs));
            Box::new(stc)
        }
        other => panic!("no case {other}"),
    }
}

/// Cases whose dump holds the sky AstroCoords Error Region: a 1e-4 rad Circle
/// in Galactic coordinates that the Stc re-expresses in ICRS as the Ellipse
/// best fitting its transformed mesh. The shape is all but circular, so the
/// fitted axis direction is ill-conditioned, and the last-bit libm differences
/// between platforms move the Ellipse's defining points by about 1e-5
/// relative: C would move them as much. Off the platform the C values were
/// captured on, those points are left out of the comparison and the rest of
/// the dump is compared as usual; on it, everything is compared exactly.
const ERROR_ELLIPSE_LICENSED: &[(&str, &str)] = &[
    ("sky", "ill-conditioned Error Ellipse fit"),
    ("catalog_entry", "ill-conditioned Error Ellipse fit"),
    ("sky_coord", "ill-conditioned Error Ellipse fit"),
];

/// `dump` without the values of the PointSets inside Ellipses.
fn without_ellipse_points(dump: &str) -> String {
    let mut in_ellipse = false;
    dump.lines()
        .filter(|l| {
            let t = l.trim_start();
            if t.starts_with("Begin Ellipse") {
                in_ellipse = true;
            } else if t.starts_with("End Ellipse") {
                in_ellipse = false;
            }
            !(in_ellipse && t.starts_with('X') && t.contains(" = "))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Compare the dump of case `name` with C's, under `ERROR_ELLIPSE_LICENSED`.
#[track_caller]
fn assert_dump(name: &str, got: &str, want: &str, what: &str) {
    if !crate::common::on_capture_platform()
        && ERROR_ELLIPSE_LICENSED.iter().any(|(n, _)| *n == name)
    {
        crate::common::assert_c_text(
            &without_ellipse_points(got),
            &without_ellipse_points(want),
            what,
        );
    } else {
        crate::common::assert_c_text(got, want, what);
    }
}

fn dump(region: &dyn Region) -> String {
    to_native_string(&AnyObject::from_mapping(region as &dyn Mapping).unwrap()).unwrap()
}

#[test]
fn stc_matches_c() {
    for &(name, want, _, points, bounded) in cases::CASES {
        let stc = built(name);
        assert_dump(name, &dump(stc.as_ref()), want, &format!("{name}: dump"));

        let input = PointSet::from_rows(points.iter().map(|p| &p[..2]));
        let out = stc.transform(&input, true).unwrap();
        for (i, p) in points.iter().enumerate() {
            crate::common::assert_c_values(&out.point(i), &p[2..], &format!("{name}: point {i}"));
        }
        assert_eq!(stc.is_bounded().unwrap(), bounded, "{name}: bounded");
    }
}

#[test]
fn full_dumps_match_c() {
    for &(name, want) in cases::FULL_DUMPS {
        let stc = built(name);
        let object = AnyObject::from_mapping(stc.as_ref() as &dyn Mapping).unwrap();
        let mut ch = NativeChannel::with_string_sink();
        ch.config_mut().set_full(1);
        ch.write(&object).unwrap();
        assert_dump(name, &ch.into_string(), want, name);
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

/// Off the capture platform, the licensed Ellipse points really do differ from
/// C's, so `ERROR_ELLIPSE_LICENSED` is still needed. On it they match exactly
/// and are compared in full by the tests above.
#[test]
fn error_ellipse_licence_is_still_needed() {
    // The real platform, not `on_capture_platform`: forcing the tolerant
    // comparisons on macOS does not move the points.
    if cfg!(target_os = "macos") {
        return;
    }
    let coord = sky().get_stc_coord(1).unwrap();
    let got = to_native_string(&AnyObject::KeyMap(Box::new(coord))).unwrap();
    assert!(
        !crate::common::c_text_eq(&got, cases::SKY_COORD),
        "the sky Error Ellipse now matches C here; remove ERROR_ELLIPSE_LICENSED"
    );
}

#[test]
fn astro_coords_match_c() {
    let stc = sky();
    assert_eq!(stc.get_stc_n_coord() as i32, cases::SKY_NCOORD);
    let coord = stc.get_stc_coord(1).unwrap();
    let got = to_native_string(&AnyObject::KeyMap(Box::new(coord))).unwrap();
    assert_dump("sky_coord", &got, cases::SKY_COORD, "sky coord");
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
