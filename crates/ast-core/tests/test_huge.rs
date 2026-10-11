//! Port of `reference/starlink-ast/ast_tester/testhuge.c`: the 64-bit pixel
//! index paths of `resample`, `Polygon::outline`, `mask` and
//! `Moc::add_pixel_mask` on a 60001 x 60001 `f32` grid, more pixels than a
//! 32-bit index can count.
//!
//! The test needs about 45 GB of memory and runs for many minutes, so it is
//! `#[ignore]`d, as C's runs only under `make check TEST_HUGE=1`. Run it in
//! a release build:
//!
//! ```text
//! cargo test --release -p ast-core --test regions test_huge:: -- --ignored
//! ```
//!
//! Divergences from the C ancestor:
//! - C's `stopit` records only the first failure and carries on; the port
//!   asserts each check in turn, so it stops at the first.
//! - `astFlushMemory(1)` is omitted.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::FrameSet;
use ast_core::mappings::{
    Interp, Mapping, ResampleFlags, ResampleInput, ResampleOptions, ResampleOutput, ShiftMap,
    resample,
};
use ast_core::regions::{CmpOperator, Moc, PixelOper, Polygon, Region, mask};
use ast_core::{AnyObject, Channel, FitsChan};

const NX: i64 = 60001;
const NY: i64 = 60001;

/// Offset of zero-based pixel `(ix, iy)` in an `NX`-wide array.
fn off(ix: i64, iy: i64) -> usize {
    (ix + iy * NX) as usize
}

/// Each pixel holds the sum of its one-based indices, except the central
/// pixel, which is zero.
fn fill_array(array: &mut [f32]) {
    for iy in 0..NY {
        for ix in 0..NX {
            array[off(ix, iy)] = (ix + 1 + iy + 1) as f32;
        }
    }
    array[off(NX / 2, NY / 2)] = 0.0;
}

/// The resampled array is the input less one, except where it is zero:
/// outside the resampled region, and at and around the central pixel.
fn test_arrays(input: &[f32], out: &[f32]) {
    let ux = NX - 99;
    let uy = NY - 99;
    assert_eq!(out[off(NX / 2, NY / 2)], 0.0, "error 2");
    assert_eq!(out[off(99, 99)], 0.0, "error 3");
    assert_eq!(input[off(100, 100)] - out[off(100, 100)], 1.0, "error 4");
    assert_eq!(
        input[off(ux - 2, uy - 2)] - out[off(ux - 2, uy - 2)],
        1.0,
        "error 5"
    );
    assert_eq!(out[off(ux - 1, uy - 1)], 0.0, "error 6");
    for iy in 0..NY {
        for ix in 0..NX {
            let o = off(ix, iy);
            if out[o] != 0.0
                && input[o] - out[o] != 1.0
                && ((ix - NX / 2).abs() > 1 || (iy - NY / 2).abs() > 1)
            {
                panic!("error 7: {} {} {} {}", ix + 1, iy + 1, input[o], out[o]);
            }
        }
    }
}

/// A TAN projection covering 10 degrees on each axis of a `dims` grid.
fn make_wcs(dims: [usize; 2]) -> FrameSet {
    let mut fc = FitsChan::new();
    for card in [
        "NAXIS   = 2",
        "RADECSYS= 'ICRS    '",
        "CRVAL1  = 0.0",
        "CRVAL2  = 0.0",
        "CTYPE1  = 'RA---TAN'",
        "CTYPE2  = 'DEC--TAN'",
    ] {
        fc.put_fits(card, false).unwrap();
    }
    // As C, NAXIS2 is set from the first dimension.
    fc.set_fits_i("NAXIS1", dims[0] as i32, " ", false).unwrap();
    fc.set_fits_i("NAXIS2", dims[0] as i32, " ", false).unwrap();
    fc.set_fits_f("CRPIX1", 0.5 * (1.0 + dims[0] as f64), " ", false)
        .unwrap();
    fc.set_fits_f("CRPIX2", 0.5 * (1.0 + dims[1] as f64), " ", false)
        .unwrap();
    fc.set_fits_f("CDELT1", 10.0 / dims[0] as f64, " ", false)
        .unwrap();
    fc.set_fits_f("CDELT2", 10.0 / dims[1] as f64, " ", false)
        .unwrap();
    fc.clear_card().unwrap();
    let Some(AnyObject::FrameSet(wcs)) = fc.read().unwrap() else {
        panic!("expected a FrameSet");
    };
    wcs
}

#[test]
#[ignore = "allocates about 45 GB and runs for many minutes; run with --release -- --ignored"]
fn huge() {
    let total = (NX * NY) as usize;
    let mut ip1 = vec![0.0f32; total];
    let mut ip2 = vec![0.0f32; total];

    println!("   Test array dimensions ({NX},{NY})");
    println!("   Filling...");
    fill_array(&mut ip1);

    let shifts = [0.5, 0.5];
    let map = ShiftMap::new(shifts.to_vec());

    let lbnd = [-10, -10];
    let ubnd = [lbnd[0] - 1 + NX, lbnd[1] - 1 + NY];
    let lbnd2 = [lbnd[0] + 100, lbnd[1] + 100];
    let ubnd2 = [ubnd[0] - 100, ubnd[1] - 100];

    println!("   Resampling...");
    let nbad = resample(
        &map as &dyn Mapping,
        &ResampleInput {
            lbnd: &lbnd,
            ubnd: &ubnd,
            data: &ip1,
            var: None,
        },
        &mut ResampleOutput {
            lbnd: &lbnd,
            ubnd: &ubnd,
            region_lbnd: &lbnd2,
            region_ubnd: &ubnd2,
            data: &mut ip2,
            var: None,
        },
        &ResampleOptions {
            interp: Interp::Linear,
            params: &shifts,
            flags: ResampleFlags {
                use_bad: true,
                ..Default::default()
            },
            tol: 0.1,
            maxpix: 50,
        },
        0.0f32,
    )
    .unwrap();
    assert_eq!(nbad, 1, "Error 1");

    println!("   Testing...");
    test_arrays(&ip1, &ip2);

    println!("   Finding outline polygon...");
    let inside = [lbnd[0] - 1, lbnd[1] - 1];
    let val = (2 + NX / 2 + NY / 2) as f32;
    let poly =
        Polygon::outline(val, PixelOper::Ge, &ip2, lbnd, ubnd, 1.0, 10, inside, false).unwrap();
    drop(ip2);

    let (plbnd, pubnd) = poly.get_region_bounds().unwrap();
    assert!((plbnd[0] - lbnd2[0] as f64).abs() <= 1.0, "Error 8");
    assert!((plbnd[1] - lbnd2[1] as f64).abs() <= 1.0, "Error 9");
    assert!((pubnd[0] - ubnd2[0] as f64).abs() <= 1.0, "Error 10");
    assert!((pubnd[1] - ubnd2[1] as f64).abs() <= 1.0, "Error 11");

    // As C, both coordinates are offset by the first axis's lower bound.
    let at = |a: i64, b: i64| -> [f64; 2] {
        [(a - 2 + lbnd[0] - 1) as f64, (b - 2 + lbnd[0] - 1) as f64]
    };
    let at2 = |a: i64, b: i64| -> [f64; 2] {
        [(a + 2 + lbnd[0] - 1) as f64, (b + 2 + lbnd[0] - 1) as f64]
    };
    let pts = [
        at(NX / 4, 3 * NY / 4),
        at2(NX / 4, 3 * NY / 4),
        at(3 * NX / 4, NY / 4),
        at2(3 * NX / 4, NY / 4),
    ];
    let out = poly
        .transform(&PointSet::from_rows(pts.iter().map(|p| p.as_slice())), true)
        .unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE, "Error 12");
    assert_ne!(out.point(1)[0], BAD_VALUE, "Error 13");
    assert_eq!(out.point(2)[0], BAD_VALUE, "Error 14");
    assert_ne!(out.point(3)[0], BAD_VALUE, "Error 15");

    println!("   Masking using polygon...");
    mask(&poly, None, false, &lbnd, &ubnd, &mut ip1, -999.0f32).unwrap();

    println!("   Creating MOC from mask...");
    let mut moc = Moc::new();
    let dims = [
        (ubnd[0] - lbnd[0] + 1) as usize,
        (ubnd[1] - lbnd[1] + 1) as usize,
    ];
    let wcs = make_wcs(dims);
    moc.add_pixel_mask(
        CmpOperator::Or,
        &wcs,
        -999.0f32,
        PixelOper::Ne,
        None,
        &ip1,
        dims,
    )
    .unwrap();
    let moc_area = moc.moc_area();
    let cell16 = moc.test_cell(20, 4854871195359, false).unwrap();
    let cell17 = moc.test_cell(20, 4854871195530, false).unwrap();
    let cell18 = moc.test_cell(5, 4525, false).unwrap();
    let cell19 = moc.test_cell(5, 4523, false).unwrap();

    // C keeps this a coarse check: changes to MOC boundary tracing since the
    // 2019 Fortran test shift the area by a few square degrees, and the
    // cell tests below are the exact check.
    assert!(
        (moc_area - 177458.13535844377).abs() <= 5.0,
        "Error 15: MocArea {moc_area:.17} cells 16={cell16} 17={cell17} 18={cell18} 19={cell19}"
    );
    assert!(cell16, "Error 16");
    assert!(!cell17, "Error 17");
    assert!(!cell18, "Error 18");
    assert!(cell19, "Error 19");
    println!("All Huge tests passed");
}
