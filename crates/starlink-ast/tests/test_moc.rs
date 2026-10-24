//! The Moc class: Mocs made from Circles, a pixel mask and cells, their
//! resolution, bounds, mesh, data and FITS header, overlap with Regions,
//! and the MOC string and JSON forms.
//!
//! C ancestor: `reference/starlink-ast/ast_tester/testmoc.c`, including the
//! `check*` functions added upstream on `u/timj/norm-unit`. Each `Error N`
//! of the C test is named in a comment beside its assertion.
//!
//! Divergences from C:
//! - `astOverlap` returns an integer; the port's [`Overlap`] is mapped to it
//!   so the expected values read as C's.
//! - `checkPinsMask` calls the protected `astRegPins`; the port's
//!   `Region::reg_pins_mask` is public.
//! - The MOC text is passed as a `&str`, so the C test's text lengths (100
//!   for a 5-character string in the `Error 42` loop) have no counterpart.

// testmoc.c centers its circles at 3.1415927, which is not quite pi.
#![allow(clippy::approx_constant)]

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;
use starlink_ast::error::ErrorCode;
use starlink_ast::frames::{Frame, FrameSet, SkyFrame, SkySystem};
use starlink_ast::mappings::Mapping;
use starlink_ast::regions::{CircleBuilder, CmpOperator, Moc, MocData, Overlap, PixelOper, Region};
use starlink_ast::{AnyObject, Channel, FitsChan};
use std::f64::consts::{FRAC_PI_2, PI};

const DD2R: f64 = PI / 180.0;

fn icrs() -> SkyFrame {
    SkyFrame::new(SkySystem::Icrs)
}

fn circle(x: f64, y: f64, r: f64) -> Box<dyn Region> {
    Box::new(
        CircleBuilder::new(icrs(), vec![x, y])
            .unwrap()
            .radius(r)
            .build()
            .unwrap(),
    )
}

fn ordered(maxorder: i32, minorder: i32) -> Moc {
    let mut moc = Moc::new();
    moc.set_max_order(maxorder).unwrap();
    moc.set_min_order(minorder);
    moc
}

/// `astEqual`: moc.c `Equal`, which is the Mapping-level equality.
fn equal(a: &Moc, b: &Moc) -> bool {
    Mapping::equal(a, b)
}

/// `astOverlap`'s integer for an [`Overlap`].
fn overlap(a: &dyn Region, b: &dyn Region) -> i32 {
    match a.overlap(b).unwrap() {
        Overlap::Incomparable => 0,
        Overlap::None => 1,
        Overlap::FirstInside => 2,
        Overlap::SecondInside => 3,
        Overlap::Partial => 4,
        Overlap::Same => 5,
        Overlap::Negation => 6,
    }
}

/// `datacheck`: the Moc's NUNIQ data read into a new Moc gives the same Moc.
fn datacheck(moc: &Moc, text: &str) {
    let mut moc2 = Moc::new();
    match moc.get_moc_data().unwrap() {
        MocData::Int32(d) => moc2.add_moc_data(CmpOperator::Or, false, -1, &d),
        MocData::Int64(d) => moc2.add_moc_data(CmpOperator::Or, false, -1, &d),
    }
    .unwrap();
    assert_eq!(overlap(moc, &moc2), 5, "{text} 1");
    assert!(equal(moc, &moc2), "{text} 2");
}

/// `makeimage`: a 100x100 image of the distance from its center, and a
/// FrameSet whose current Frame has a celestial and a velocity axis.
fn makeimage() -> (Vec<f32>, [usize; 2], FrameSet) {
    let (nx, ny) = (100_usize, 100_usize);
    let xc = (1.0 + nx as f64) / 2.0;
    let yc = (1.0 + ny as f64) / 2.0;
    let mut data = vec![0.0_f32; nx * ny];
    for j in 0..ny {
        for i in 0..nx {
            let dx = i as f64 + 1.0 - xc;
            let dy = j as f64 + 1.0 - yc;
            data[i + nx * j] = (dx * dx + dy * dy).sqrt() as f32;
        }
    }
    let mut fc = FitsChan::new().unwrap();
    fc.set_fits_f("CRVAL1", 35.0, " ", true).unwrap();
    fc.set_fits_f("CRVAL2", 55.0, " ", true).unwrap();
    fc.set_fits_f("CRPIX1", 50.5, " ", true).unwrap();
    fc.set_fits_f("CRPIX2", 50.5, " ", true).unwrap();
    fc.set_fits_f("CDELT1", -0.01, " ", true).unwrap();
    fc.set_fits_f("CDELT2", 0.01, " ", true).unwrap();
    fc.set_fits_s("CTYPE1", "RA---TAN", " ", true).unwrap();
    fc.set_fits_s("CTYPE2", "DEC--TAN", " ", true).unwrap();
    fc.set_fits_f("CRVAL3", -22.9, " ", true).unwrap();
    fc.set_fits_f("CRPIX3", 1.0, " ", true).unwrap();
    fc.set_fits_f("CDELT3", 1.27, " ", true).unwrap();
    fc.set_fits_s("CTYPE3", "VRAD    ", " ", true).unwrap();
    fc.set_fits_s("CUNIT3", "km/s    ", " ", true).unwrap();
    fc.clear_card().unwrap();
    let Some(AnyObject::FrameSet(fs)) = fc.read().unwrap() else {
        panic!("expected a FrameSet");
    };
    (data, [nx, ny], fs)
}

/// The boundary mesh of `moc` in its current Frame, as (RA, Dec) points.
fn mesh(moc: &Moc) -> Vec<[f64; 2]> {
    let m = moc.region_mesh().unwrap();
    (0..m.npoint())
        .map(|i| [m.point(i)[0], m.point(i)[1]])
        .collect()
}

#[test]
fn small_circle() {
    let moc = ordered(18, 11);
    // Errors 1-3.
    assert_eq!(moc.system_string().unwrap(), Some("ICRS"));

    let mut moc = moc;
    moc.add_region(CmpOperator::Or, circle(1.0, 1.0, 0.0002).as_ref())
        .unwrap();
    // Error 3b.
    assert!(
        (moc.moc_area() - 1.485).abs() <= 1.0e-3,
        "{}",
        moc.moc_area()
    );
    datacheck(&moc, "Error 3A");
}

#[test]
fn larger_circle() {
    let sf = icrs();
    let mxres1 = 824.5167;
    let mut moc = ordered(8, 4);
    let centre = [3.1415927, 0.75];
    let reg1 = circle(centre[0], centre[1], 0.5);
    moc.add_region(CmpOperator::Or, reg1.as_ref()).unwrap();

    // Error 4.
    assert!(
        (moc.max_res() - mxres1).abs() <= 1.0e-4,
        "{}",
        moc.max_res()
    );

    // Error 5.
    let (lbnd, ubnd) = moc.get_region_bounds().unwrap();
    assert!((lbnd[0] - 2.4235144).abs() <= 1.0e-6, "{lbnd:?}");
    assert!((ubnd[0] - 3.8596708).abs() <= 1.0e-6, "{ubnd:?}");
    assert!((lbnd[1] - 0.2499916).abs() <= 1.0e-6, "{lbnd:?}");
    assert!((ubnd[1] - 1.2504847).abs() <= 1.0e-6, "{ubnd:?}");

    // Errors 6-7.
    let m = mesh(&moc);
    assert_eq!(m.len(), 868);
    for p in &m {
        let d = sf.distance(&centre, p).unwrap();
        assert!((d - 0.5).abs() <= 0.7 * DD2R * mxres1 / 3600.0, "{p:?}");
    }

    // Errors 8-13.
    assert_eq!(moc.moc_type().unwrap(), 4);
    let ln = moc.moc_length().unwrap();
    assert_eq!(ln, 832);
    let mut fc = moc.get_moc_header().unwrap();
    assert_eq!(fc.get_fits_i(Some("NAXIS1")).unwrap(), Some(4));
    assert_eq!(fc.get_fits_i(Some("NAXIS2")).unwrap(), Some(ln as i32));
    assert_eq!(
        fc.get_fits_s(Some("TFORM1")).unwrap().as_deref(),
        Some("1J")
    );
    assert_eq!(fc.get_fits_i(Some("MOCORDER")).unwrap(), Some(8));

    datacheck(&moc, "Error 13A");

    // Error 14.
    let moc2 = moc.clone();
    assert!(equal(&moc, &moc2));

    // Errors 15-19.
    assert_eq!(overlap(&moc, reg1.as_ref()), 5);
    assert_eq!(overlap(&moc, circle(3.1415927, 0.5, 0.5).as_ref()), 4);
    assert_eq!(overlap(&moc, circle(3.1415927, -0.5, 0.5).as_ref()), 1);
    let reg1 = circle(3.1415927, 0.75, 0.3);
    assert_eq!(overlap(&moc, reg1.as_ref()), 3);
    assert_eq!(overlap(reg1.as_ref(), &moc), 2);
}

#[test]
fn annulus() {
    // Error 20: an annulus, as a Circle ANDed with a negated smaller one.
    let mut moc = ordered(8, 4);
    let reg1 = circle(0.0, 1.57, 0.3);
    moc.add_region(CmpOperator::Or, reg1.as_ref()).unwrap();
    let mut reg2 = circle(0.0, 1.57, 0.2);
    reg2.negate();
    moc.add_region(CmpOperator::And, reg2.as_ref()).unwrap();
    assert!(
        (moc.moc_area() - 1.826e6).abs() <= 0.001e6,
        "{}",
        moc.moc_area()
    );

    // Errors 21-22: the same by AddMocData.
    let mut moc2 = ordered(8, 4);
    moc2.add_region(CmpOperator::Or, reg1.as_ref()).unwrap();
    let mut moc3 = ordered(8, 4);
    moc3.add_region(CmpOperator::Or, reg2.as_ref()).unwrap();
    match moc3.get_moc_data().unwrap() {
        MocData::Int32(d) => moc2.add_moc_data(CmpOperator::And, false, -1, &d),
        MocData::Int64(d) => moc2.add_moc_data(CmpOperator::And, false, -1, &d),
    }
    .unwrap();
    assert_eq!(overlap(&moc, &moc2), 5);
    assert!(equal(&moc, &moc2));
}

#[test]
fn pixel_mask() {
    let sf = icrs();
    let (data, dims, iwcs) = makeimage();
    let mut moc = Moc::new();
    moc.add_pixel_mask(
        CmpOperator::Or,
        &iwcs,
        10.0_f32,
        PixelOper::Lt,
        None,
        &data,
        dims,
    )
    .unwrap();

    // Errors 23A, 23.
    assert_eq!(moc.system_string().unwrap(), Some("ICRS"));
    assert!(
        (moc.max_res() - 12.883).abs() <= 1.0e-3,
        "{}",
        moc.max_res()
    );

    // Errors 24-25.
    let m = mesh(&moc);
    assert_eq!(m.len(), 294);
    let centre = [35.0 * DD2R, 55.0 * DD2R];
    for p in &m {
        let d = sf.distance(&centre, p).unwrap();
        assert!((d - 1.745e-3).abs() <= DD2R * 0.01, "{p:?}");
    }
}

#[test]
fn mocs_of_mocs_and_cells() {
    let mut moc = ordered(8, 4);
    moc.add_region(CmpOperator::Or, circle(0.0, 1.57, 0.3).as_ref())
        .unwrap();

    // Errors 26-27.
    let mut moc2 = Moc::new();
    moc2.add_region(CmpOperator::Or, &moc).unwrap();
    assert!(equal(&moc, &moc2));
    assert_eq!(overlap(&moc, &moc2), 5);

    // Errors 28-29: AND with Mocs of other orders.
    let mut reg2 = circle(0.0, 1.57, 0.2);
    reg2.negate();
    let mut moc2 = ordered(9, 4);
    moc2.add_region(CmpOperator::Or, reg2.as_ref()).unwrap();
    moc.add_region(CmpOperator::And, &moc2).unwrap();
    assert!(
        (moc.moc_area() - 1.843466e6).abs() <= 1.0,
        "{}",
        moc.moc_area()
    );

    let mut moc2 = ordered(7, 4);
    moc2.add_region(CmpOperator::Or, reg2.as_ref()).unwrap();
    moc.add_region(CmpOperator::And, &moc2).unwrap();
    assert!(
        (moc.moc_area() - 1.803054e6).abs() <= 1.0,
        "{}",
        moc.moc_area()
    );

    // Errors 29B-31: the cells, tested and added to a new Moc.
    let mut moc3 = Moc::new();
    moc3.set_max_order(moc.max_order()).unwrap();
    for i in 0..moc.moc_length().unwrap() {
        let (order, npix) = moc.get_cell(i as i32).unwrap();
        assert!(moc.test_cell(order, npix, false).unwrap(), "cell {i}");
        moc3.add_cell(CmpOperator::Or, order, npix).unwrap();
    }
    assert!(equal(&moc, &moc3));
    assert_eq!(overlap(&moc, &moc3), 5);

    // Error 32.
    assert!(!moc.test_cell(8, 123456, false).unwrap());
}

#[test]
fn string_round_trip() {
    let mocstr1 = "1/1-2,4 2/12-14,21,23,25 8/";
    let mocstr2 = " 1/1,2,4   2/12-14, 21,23,25 8/ ";
    let mut moc = Moc::new();
    moc.add_moc_string(CmpOperator::Or, false, -1, mocstr1)
        .unwrap();
    // Errors 33-35.
    let text = moc.get_moc_string(false).unwrap();
    assert_eq!(text.len(), 27);
    assert_eq!(text, mocstr1);

    // Errors 36-37.
    let mut moc2 = Moc::new();
    moc2.add_moc_string(CmpOperator::Or, false, -1, mocstr2)
        .unwrap();
    assert!(equal(&moc, &moc2));
    assert_eq!(moc.max_order(), 8);
}

#[test]
fn json_round_trip() {
    let mocjson1 = "{\"1\":[1,2,4], \"2\":[12,13,14,21,23,25],\"8\":[]}";
    let mocjson2 = "{\"1\":[1,2,4],\"2\":[12,13,14,21,23,25],\"8\":[]}";
    let mut moc = Moc::new();
    // Error 38.
    assert!(
        moc.add_moc_string(CmpOperator::Or, false, -1, mocjson1)
            .unwrap()
    );
    // Errors 39-41.
    let text = moc.get_moc_string(true).unwrap();
    assert_eq!(text.len(), 44);
    assert_eq!(text, mocjson2);
}

#[test]
fn normalisation_at_each_maxorder() {
    // Error 42.
    let expected = [
        "0/",
        "1/",
        "2/",
        "3/",
        "4/1",
        "4/1 5/1-2",
        "4/1 5/1-2 6/",
        "4/1 5/1-2 7/",
        "4/1 5/1-2 8/",
        "4/1 5/1-2 9/",
        "4/1 5/1-2 10/",
    ];
    for (i, want) in expected.iter().enumerate() {
        let mut moc = Moc::new();
        moc.add_moc_string(CmpOperator::Or, false, i as i32, "4/0-1")
            .unwrap();
        moc.add_moc_string(CmpOperator::And, true, -1, "5/0,3")
            .unwrap();
        assert_eq!(moc.get_moc_string(false).unwrap(), *want, "maxorder {i}");
    }
}

#[test]
fn check_empty_bounds() {
    let mut moc = Moc::new();
    let (lbnd, ubnd) = moc.get_region_bounds().unwrap();
    assert!(ubnd[0] < lbnd[0] && ubnd[1] < lbnd[1], "{lbnd:?} {ubnd:?}");
    for npix in 0..12 {
        moc.add_cell(CmpOperator::Or, 0, npix).unwrap();
    }
    let (lbnd, ubnd) = moc.get_region_bounds().unwrap();
    assert_eq!(lbnd[1], -FRAC_PI_2);
    assert_eq!(ubnd[1], FRAC_PI_2);
    assert_eq!(ubnd[0] - lbnd[0], 2.0 * PI);
}

#[test]
fn check_pins_mask() {
    let mut moc = Moc::new();
    moc.add_cell(CmpOperator::Or, 2, 37).unwrap();
    let m = mesh(&moc);
    // The mesh in reverse order, then one point far from the boundary.
    let mut rows: Vec<[f64; 2]> = m.iter().rev().copied().collect();
    rows.push([3.0, -1.4]);
    let pins = PointSet::from_rows(&rows);
    let (_, mask) = moc.reg_pins_mask(&pins, None).unwrap();
    assert!(!mask[m.len()], "the far point is flagged");
    assert!(mask[0], "the first mesh point is not flagged");
}

#[test]
fn check_mesh_tie_break() {
    let mut moc = Moc::new();
    for (cmode, order, npix) in [
        (CmpOperator::Or, 2, 37),
        (CmpOperator::Or, 1, 3),
        (CmpOperator::Or, 2, 100),
        (CmpOperator::Xor, 1, 25),
        (CmpOperator::Xor, 2, 101),
        (CmpOperator::And, 2, 13),
        (CmpOperator::Or, 0, 7),
        (CmpOperator::And, 2, 30),
    ] {
        moc.add_cell(cmode, order, npix).unwrap();
    }
    let m = mesh(&moc);
    assert!(
        !m.iter()
            .any(|p| (p[1] - 0.5236026246043699).abs() < 1.0e-14),
        "the mesh kept the higher of two tied corners"
    );
    assert!(
        m.iter()
            .any(|p| (p[1] - 0.5235949266007811).abs() < 1.0e-14),
        "expected mesh corner missing"
    );
}

#[test]
fn check_empty_transform() {
    let mut moc = Moc::new();
    let x = [1.0, 4.0];
    let y = [0.5, -0.5];
    let (xo, yo) = moc.tran2(&x, &y, true).unwrap();
    assert_eq!(xo[0], BAD_VALUE);
    assert_eq!(yo[1], BAD_VALUE);
    moc.negate();
    let (xo, yo) = moc.tran2(&x, &y, true).unwrap();
    assert_eq!(xo[0], x[0]);
    assert_eq!(yo[1], y[1]);
}

#[test]
fn check_add_region_order_zero() {
    let mut moc = Moc::new();
    moc.set_max_order(0).unwrap();
    let err = moc
        .add_region(CmpOperator::Or, circle(1.2, 0.3, 0.2).as_ref())
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::Invar);
}

#[test]
fn check_oversample_negate() {
    let mut reg = circle(1.2, 0.3, 0.2);
    reg.negate();
    let mut over = ordered(5, 6);
    over.add_region(CmpOperator::Or, reg.as_ref()).unwrap();
    let sky = 4.0 * PI * (180.0 * 60.0 / PI).powi(2);
    assert!(over.moc_area() <= sky, "{} > {sky}", over.moc_area());
}

#[test]
fn check_moc_string_errors() {
    for bad in [
        "5",
        "5 6",
        "3/10-5",
        "4294967301/1",
        "0/12",
        "{\"0\":[12]}",
        "3/18446744073709551621",
    ] {
        let err = Moc::new()
            .add_moc_string(CmpOperator::Or, false, -1, bad)
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::Inmoc, "{bad}");
    }
}

#[test]
fn check_empty_json() {
    assert_eq!(Moc::new().get_moc_string(true).unwrap(), "{}");
    let mut moc = Moc::new();
    moc.set_max_order(3).unwrap();
    assert!(
        moc.add_moc_string(CmpOperator::Or, false, -1, "{}")
            .unwrap()
    );
    assert_eq!(moc.moc_length().unwrap(), 0);
}

#[test]
fn check_parse_error_leaves_moc() {
    let mut moc = Moc::new();
    moc.set_max_order(3).unwrap();
    moc.add_cell(CmpOperator::Or, 2, 7).unwrap();
    let before = moc.clone();
    let err = moc
        .add_moc_string(CmpOperator::Or, false, -1, "{\"1\":[3]")
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::Inmoc);
    assert!(equal(&moc, &before));
}

#[test]
fn check_bad_nuniq() {
    let mut moc = Moc::new();
    moc.set_max_order(5).unwrap();
    let err = moc
        .add_moc_data(CmpOperator::Or, false, -1, &[20_i64, 0])
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::Inmoc);
    let err = moc
        .add_moc_data(CmpOperator::Or, false, -1, &[20_i32, -5])
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::Inmoc);
    assert_eq!(moc.moc_length().unwrap(), 0);
}
