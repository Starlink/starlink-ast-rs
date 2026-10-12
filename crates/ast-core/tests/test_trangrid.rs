//! `Mapping::tran_grid` over the Mappings of `testtrangrid.c`.
//!
//! C ancestor: `reference/starlink-ast/ast_tester/testtrangrid.c`. Each
//! Mapping transforms a grid with `tran_grid`, then the results are
//! transformed back and must land on the grid positions. This is done
//! forward over a fixed grid, and inverse over the integer box that
//! `map_box` gives for the forward grid, each with and without a linear-fit
//! tolerance and with initial scale sizes of 100 and 4.
//!
//! Divergences from C:
//! - Results come back as a `PointSet` and are transformed back with
//!   `Mapping::transform`, rather than through `astTranN` into fixed arrays.
//! - The unit MatrixMap is built from an identity matrix, as there is no
//!   public constructor for C's unit form; the transformation is the same.

use ast_core::coords::PointSet;
use ast_core::mappings::{
    CmpMap, Composition, DynMapping, Mapping, MathMap, MatrixMap, PermMap, ShiftMap, TranMap,
    UnitMap, WinMap, ZoomMap,
};
use ast_core::{AnyObject, Channel, FitsChan};
use ndarray::array;

const TAN_CARDS: [&str; 9] = [
    "CTYPE1  = 'RA---TAN'",
    "CTYPE2  = 'DEC--TAN'",
    "CRPIX1  = 20",
    "CRPIX2  = 20",
    "CRVAL1  = 0.0",
    "CRVAL2  = 0.0",
    "CROTA1  = 30.0",
    "CDELT1  = -0.001",
    "CDELT2  = 0.001",
];

fn read_tan() -> ast_core::error::AstResult<Option<AnyObject>> {
    let mut fc = FitsChan::new();
    for card in TAN_CARDS {
        fc.put_fits(card, false).unwrap();
    }
    fc.clear_card().unwrap();
    fc.read()
}

/// `testgrid`: transform the grid `[lbnd, ubnd]` in the given direction, then
/// transform the results the other way and check they give the grid back.
fn testgrid(
    map: &dyn Mapping,
    lbnd: &[i64],
    ubnd: &[i64],
    tol: f64,
    maxpix: usize,
    forward: bool,
    itest: i32,
) {
    let out = map.tran_grid(lbnd, ubnd, tol, maxpix, forward).unwrap();
    let back = map.transform(&out, !forward).unwrap();
    let nin = lbnd.len();
    let mut pos = lbnd.to_vec();
    for j in 0..back.npoint() {
        for (i, (&val, &p)) in back.point(j).iter().zip(&pos).enumerate() {
            let want = p as f64;
            let ok = if p != 0 {
                (val - want).abs() <= 1.0e-6 * (0.5 * (val + want)).abs()
            } else {
                val.abs() <= 1.0e-5
            };
            assert!(
                ok,
                "Test {itest} failed at point {j} axis {i}: {val} should be {p}"
            );
        }
        // Step to the next grid position, first axis fastest.
        for i in 0..nin {
            pos[i] += 1;
            if pos[i] <= ubnd[i] {
                break;
            }
            pos[i] = lbnd[i];
        }
    }
}

/// `testgridpair`: the forward grid, then the inverse over the integer
/// bounds of its outputs.
fn testgridpair(map: &dyn Mapping, tol: f64, maxpix: usize, itest: i32) {
    let lbnd_all = [-6_i64, 0, 10];
    let ubnd_all = [15_i64, 30, 40];
    let nin = map.nin();
    let lbnd = &lbnd_all[..nin];
    let ubnd = &ubnd_all[..nin];
    testgrid(map, lbnd, ubnd, tol, maxpix, true, itest);

    let dlbnd: Vec<f64> = lbnd.iter().map(|&v| v as f64).collect();
    let dubnd: Vec<f64> = ubnd.iter().map(|&v| v as f64).collect();
    let mut lbndi = Vec::new();
    let mut ubndi = Vec::new();
    for i in 0..map.nout() {
        let b = map.map_box(&dlbnd, &dubnd, true, i + 1).unwrap();
        // C assigns the bounds to ints, truncating toward zero.
        lbndi.push(b.lbnd_out.trunc() as i64);
        ubndi.push(b.ubnd_out.trunc() as i64);
    }
    testgrid(map, &lbndi, &ubndi, tol, maxpix, false, itest + 1);
}

/// `testmap`: both scale sizes, with and without a tolerance.
fn testmap(map: &dyn Mapping, itest: i32, tol: f64) {
    testgridpair(map, tol, 100, itest * 10);
    testgridpair(map, 0.0, 100, itest * 10 + 2);
    testgridpair(map, tol, 4, itest * 10 + 4);
    testgridpair(map, 0.0, 4, itest * 10 + 6);
}

#[test]
fn unitmap() {
    testmap(&UnitMap::new(2), 1, 0.5);
}

#[test]
fn zoommap() {
    testmap(&ZoomMap::new(2, 2.0).unwrap(), 2, 0.5);
}

#[test]
fn unit_matrixmap() {
    testmap(
        &MatrixMap::new(array![[1.0, 0.0], [0.0, 1.0]]).unwrap(),
        3,
        0.5,
    );
}

#[test]
fn permmap() {
    testmap(&PermMap::new(&[2, 1], &[2, 1], &[0.0]).unwrap(), 4, 0.5);
}

#[test]
fn tranmap() {
    let c1 = WinMap::from_corners(&[0.0, 0.0], &[1.0, 1.0], &[0.5, 0.5], &[2.5, 2.5]).unwrap();
    let c2 = c1.clone();
    testmap(&TranMap::new(Box::new(c1), Box::new(c2)).unwrap(), 5, 0.5);
}

#[test]
fn cmpmap_3d() {
    let c1: DynMapping = Box::new(ShiftMap::new(vec![-1.0]));
    let c2: DynMapping = Box::new(MatrixMap::new(array![[1.0, 2.0], [-2.0, 3.0]]).unwrap());
    let c3 = CmpMap::new(c1, c2, Composition::Parallel).unwrap();
    let c1: DynMapping = Box::new(PermMap::new(&[3, 2, 1], &[3, 2, 1], &[0.0]).unwrap());
    let mut c2 = ZoomMap::new(3, 0.25).unwrap();
    c2.invert();
    let mut c4 = CmpMap::new(c1, Box::new(c2), Composition::Series).unwrap();
    c4.invert();
    let m = CmpMap::new(Box::new(c3), Box::new(c4), Composition::Series).unwrap();
    testmap(&m, 6, 0.5);
}

#[test]
fn nonlinear_mathmap_1d() {
    let m = MathMap::new(1, 1, &["y=x**3"], &["x=sign((abs(y)**(1/3)),y)"]).unwrap();
    testmap(&m, 7, 0.0001);
}

#[test]
fn fits_wcs_pixel_to_sky() {
    let Some(AnyObject::FrameSet(fs)) = read_tan().unwrap() else {
        panic!("expected a FrameSet");
    };
    testmap(&fs, 8, 0.0001);
}

#[test]
fn grid_round_trip_uses_pointset() {
    // The results hold one row per grid position, first axis fastest.
    let out = ZoomMap::new(2, 2.0)
        .unwrap()
        .tran_grid(&[1, 1], &[2, 3], 0.0, 100, true)
        .unwrap();
    let want = PointSet::from_rows([
        [2.0, 2.0],
        [4.0, 2.0],
        [2.0, 4.0],
        [4.0, 4.0],
        [2.0, 6.0],
        [4.0, 6.0],
    ]);
    assert_eq!(out.view(), want.view());
}
