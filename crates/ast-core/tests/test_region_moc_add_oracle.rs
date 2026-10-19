//! `Moc::add_region` and `Moc::add_pixel_mask` against the C library
//! (moc.c `AddRegion`, `AddPixelMask<X>`): Circles, a Polygon, a negated
//! Circle, a Circle round the pole, a Galactic Circle and a Box in a
//! CmpFrame with a spectral axis, added with OR, XOR and AND; MinOrder at and
//! above MaxOrder, the latter also with a negated Circle; Mocs added to Mocs
//! at lower, higher and unset MaxOrder, negated and empty; a MaxOrder chosen
//! from the size of a Circle; pixel masks of double, float and int arrays
//! under each operator, with a bad value, through a FrameSet with its sky
//! axes in either order, with MaxOrder chosen from the pixel size, and
//! selecting nothing. Each Moc is compared by MaxOrder, MocLength, MocArea and
//! its dump: an FNV-1a hash of it, and the dump itself where it is short. The
//! C Box is made from two corners, so the Rust one is made from its bounds.
//!
//! The errors for a Region with no sky axes and for a Region and a pixel mask
//! added with MaxOrder 0 are checked too.
//!
//! The values come from `tools/oracle_fixtures/region_moc_add_probe.c`, run
//! against the library with moc.c's `AddRegion` and `IncorporateCells` fixed
//! upstream (28473801).

use crate::common::native_dump;
// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[allow(clippy::excessive_precision)]
#[path = "region_moc_add_oracle/cases.rs"]
mod cases;

use ast_core::error::ErrorCode;
use ast_core::fitschan::FitsChan;
use ast_core::frames::{CmpFrame, FrameSet, SkyFrame, SkySystem, SpecFrame};
use ast_core::mappings::Mapping;
use ast_core::regions::{
    BoxRegionBuilder, CircleBuilder, CmpOperator, Moc, PixelOper, PolygonBuilder, Region,
};
use ast_core::{AnyObject, Channel};

const NX: usize = 20;
const NY: usize = 15;

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}

fn dump(moc: &Moc) -> String {
    native_dump(&AnyObject::from_mapping(moc as &dyn Mapping).unwrap()).unwrap()
}

fn icrs() -> SkyFrame {
    SkyFrame::new(SkySystem::Icrs)
}

fn circle(frame: SkyFrame, x: f64, y: f64, r: f64) -> Box<dyn Region> {
    Box::new(
        CircleBuilder::new(frame, vec![x, y])
            .unwrap()
            .radius(r)
            .build()
            .unwrap(),
    )
}

fn ordered(maxorder: Option<i32>, minorder: Option<i32>) -> Moc {
    let mut moc = Moc::new();
    if let Some(order) = maxorder {
        moc.set_max_order(order).unwrap();
    }
    if let Some(order) = minorder {
        moc.set_min_order(order);
    }
    moc
}

fn wcs(swapped: bool) -> FrameSet {
    let mut header = cases::HEADER.to_string();
    if swapped {
        for (a, b) in [
            ("CTYPE1  = 'RA---TAN'", "CTYPE1  = 'DEC--TAN'"),
            ("CTYPE2  = 'DEC--TAN'", "CTYPE2  = 'RA---TAN'"),
            ("CRVAL1  = 30.0", "CRVAL1  = 20.0"),
            ("CRVAL2  = 20.0", "CRVAL2  = 30.0"),
        ] {
            header = header.replacen(a, b, 1);
        }
    }
    header.push_str("END\n");
    let mut fc = FitsChan::from_header(&header).unwrap();
    match fc.read().unwrap().unwrap() {
        AnyObject::FrameSet(fs) => fs,
        _ => panic!("expected a FrameSet"),
    }
}

struct Arrays {
    d: Vec<f64>,
    f: Vec<f32>,
    i: Vec<i32>,
}

fn arrays() -> Arrays {
    let mut a = Arrays {
        d: vec![0.0; NX * NY],
        f: vec![0.0; NX * NY],
        i: vec![0; NX * NY],
    };
    for iy in 0..NY {
        for ix in 0..NX {
            let k = ix + iy * NX;
            let (x, y) = (ix as f64, iy as f64);
            a.d[k] = (x - 8.0) * (x - 8.0) / 16.0 + (y - 6.0) * (y - 6.0) / 9.0;
            a.f[k] = (0.3 * x - 0.2 * y) as f32;
            a.i[k] = ((ix + 2 * iy) % 5) as i32;
        }
    }
    a.d[3 + 4 * NX] = -999.0;
    a.d[9 + 6 * NX] = -999.0;
    a
}

/// Each case built as the probe builds it, in the probe's order.
fn built() -> Vec<(&'static str, Moc)> {
    let mut out = Vec::new();
    let or = CmpOperator::Or;

    let mut c1 = ordered(Some(7), None);
    c1.add_region(or, circle(icrs(), 1.2, 0.3, 0.2).as_ref())
        .unwrap();
    out.push(("circle", c1.clone()));

    let mut c2 = c1.clone();
    let poly = PolygonBuilder::new(icrs(), vec![vec![1.0, 0.2], vec![1.5, 0.1], vec![1.4, 0.6]])
        .unwrap()
        .build()
        .unwrap();
    c2.add_region(CmpOperator::Xor, &poly).unwrap();
    out.push(("circle_polygon_xor", c2.clone()));

    let mut m = c1.clone();
    let mut neg = circle(icrs(), 1.3, 0.35, 0.1);
    neg.negate();
    m.add_region(CmpOperator::And, neg.as_ref()).unwrap();
    out.push(("and_negated", m));

    let mut c4 = ordered(Some(6), None);
    c4.add_region(or, circle(icrs(), 0.5, 1.45, 0.2).as_ref())
        .unwrap();
    out.push(("pole", c4.clone()));

    let mut m = ordered(Some(6), None);
    let gal = SkyFrame::new(SkySystem::Galactic);
    m.add_region(or, circle(gal, 2.0, -0.5, 0.3).as_ref())
        .unwrap();
    out.push(("galactic", m));

    let mut m = ordered(Some(6), Some(3));
    let cf = CmpFrame::new(Box::new(SpecFrame::unset_system()), Box::new(icrs()));
    let bx = BoxRegionBuilder::from_bounds(cf, &[5000.0, 1.0, 0.2], &[5100.0, 1.1, 0.3])
        .unwrap()
        .build()
        .unwrap();
    m.add_region(or, &bx).unwrap();
    out.push(("cmpframe_box", m));

    for (name, max, min) in [("oversample", 5, 6), ("equal_orders", 5, 5)] {
        let mut m = ordered(Some(max), Some(min));
        m.add_region(or, circle(icrs(), 1.2, 0.3, 0.2).as_ref())
            .unwrap();
        out.push((name, m));
    }

    let mut m = ordered(Some(5), Some(6));
    let mut neg = circle(icrs(), 1.2, 0.3, 0.2);
    neg.negate();
    m.add_region(or, neg.as_ref()).unwrap();
    out.push(("oversample_negated", m));

    let mut m = ordered(Some(5), None);
    m.add_region(or, &c2).unwrap();
    out.push(("moc_lower", m));

    let mut m = ordered(Some(9), None);
    m.add_region(or, &c1).unwrap();
    let mut neg = c4.clone();
    neg.negate();
    m.add_region(CmpOperator::Xor, &neg).unwrap();
    out.push(("moc_higher_negated", m));

    let mut m = ordered(None, None);
    m.add_region(or, &c2).unwrap();
    out.push(("moc_unset", m));

    let mut m = c1.clone();
    m.add_region(CmpOperator::And, &Moc::new()).unwrap();
    out.push(("moc_and_empty", m));

    let mut m = ordered(None, None);
    m.add_region(or, circle(icrs(), 4.0, -0.6, 0.05).as_ref())
        .unwrap();
    out.push(("auto_order", m));

    let fs = wcs(false);
    let fsw = wcs(true);
    let a = arrays();
    let dims = [NX, NY];

    let mut m = ordered(None, None);
    m.add_pixel_mask(or, &fs, 4.0, PixelOper::Gt, None, &a.d, dims)
        .unwrap();
    out.push(("mask_gt_d_auto", m));

    let mut m = ordered(Some(11), Some(8));
    m.add_pixel_mask(or, &fs, 3.0, PixelOper::Le, Some(-999.0), &a.d, dims)
        .unwrap();
    out.push(("mask_le_d_bad", m));

    let mut m = ordered(Some(13), Some(12));
    m.add_pixel_mask(or, &fs, 2, PixelOper::Eq, None, &a.i, dims)
        .unwrap();
    out.push(("mask_eq_i", m.clone()));
    m.add_pixel_mask(CmpOperator::And, &fs, 0, PixelOper::Ne, None, &a.i, dims)
        .unwrap();
    out.push(("mask_eq_i_and_ne", m));

    let mut m = ordered(Some(10), Some(9));
    m.add_pixel_mask(
        CmpOperator::Xor,
        &fs,
        1.0_f32,
        PixelOper::Lt,
        None,
        &a.f,
        dims,
    )
    .unwrap();
    out.push(("mask_lt_f_xor", m));

    let mut m = ordered(Some(11), Some(8));
    m.add_pixel_mask(or, &fsw, 5.0, PixelOper::Ge, None, &a.d, dims)
        .unwrap();
    out.push(("mask_ge_d_swapped", m));

    let mut m = c1.clone();
    m.add_pixel_mask(
        CmpOperator::And,
        &fs,
        1.0e9,
        PixelOper::Gt,
        None,
        &a.d,
        dims,
    )
    .unwrap();
    out.push(("mask_and_none", m));

    out
}

#[test]
fn added_mocs_match_c() {
    let got = built();
    assert_eq!(got.len(), cases::CASES.len());
    for ((name, moc), case) in got.iter().zip(cases::CASES) {
        let (want_name, maxorder, length, area, hash, want_dump) = *case;
        assert_eq!(*name, want_name);
        assert_eq!(moc.max_order(), maxorder, "{name}: MaxOrder");
        let d = dump(moc);
        if !want_dump.is_empty() {
            assert_eq!(d, want_dump, "{name}: dump");
        }
        assert_eq!(
            moc.moc_length().unwrap() as i32,
            length,
            "{name}: MocLength"
        );
        assert_eq!(moc.moc_area(), area, "{name}: MocArea");
        assert_eq!(fnv(&d), hash, "{name}: dump hash");
    }
}

#[test]
fn errors_match_c() {
    let frame = ast_core::frames::BasicFrameBuilder::new(2).build().unwrap();
    let plain = CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();
    let err = Moc::new().add_region(CmpOperator::Or, &plain).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Nosky);
    assert_eq!(err.code() as i32, cases::NO_SKY);

    let mut m = ordered(Some(0), None);
    let err = m
        .add_region(CmpOperator::Or, circle(icrs(), 1.2, 0.3, 0.2).as_ref())
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::Invar);
    assert_eq!(err.code() as i32, cases::REGION_ORDER_ZERO);

    let a = arrays();
    let mut m = ordered(Some(0), None);
    let err = m
        .add_pixel_mask(
            CmpOperator::Or,
            &wcs(false),
            4.0,
            PixelOper::Gt,
            None,
            &a.d,
            [NX, NY],
        )
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::Invar);
    assert_eq!(err.code() as i32, cases::MASK_ORDER_ZERO);
}
