//! Whole-axis kernels: non-finite inputs and outputs longer than the input.

use ast_core::constants::BAD_VALUE;
use ast_core::coords::{Points, PointsMut};
use ast_core::mappings::{Mapping, ShiftMap, UnitMap, ZoomMap};

/// Every non-finite input is bad: BAD, NaN, and both infinities.
const ODD: [f64; 5] = [BAD_VALUE, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 2.0];

/// Transform `ODD` on both axes into outputs one point longer, which must
/// keep their last value.
fn run(map: &dyn Mapping, forward: bool) -> [Vec<f64>; 2] {
    let mut out = [vec![7.0; ODD.len() + 1], vec![7.0; ODD.len() + 1]];
    let [a, b] = &mut out;
    map.transform_into(
        Points::new([&ODD[..], &ODD[..]]).unwrap(),
        forward,
        PointsMut::new([&mut a[..], &mut b[..]]).unwrap(),
    )
    .unwrap();
    out
}

fn assert_bad_then(axis: &[f64], good: f64) {
    assert!(axis[..4].iter().all(|&v| v == BAD_VALUE), "{axis:?}");
    assert_eq!(axis[4], good);
    assert_eq!(axis[5], 7.0, "a point past the input was written");
}

#[test]
fn zoommap_kernel() {
    let m = ZoomMap::new(2, 4.0).unwrap();
    for axis in run(&m, true) {
        assert_bad_then(&axis, 8.0);
    }
    for axis in run(&m, false) {
        assert_bad_then(&axis, 0.5);
    }
}

#[test]
fn shiftmap_kernel() {
    let m = ShiftMap::new(vec![1.0, -1.0]);
    let [a, b] = run(&m, true);
    assert_bad_then(&a, 3.0);
    assert_bad_then(&b, 1.0);
}

#[test]
fn unitmap_kernel() {
    let m = UnitMap::new(2);
    let [a, _] = run(&m, true);
    // UnitMap replaces only NaN; it passes infinities through.
    assert_eq!(
        a[..5],
        [BAD_VALUE, BAD_VALUE, f64::INFINITY, f64::NEG_INFINITY, 2.0]
    );
    assert_eq!(a[5], 7.0);
}

use ast_core::mappings::{PermMap, WinMap};

#[test]
fn winmap_kernel() {
    // Axis 1 is scaled by 2; axis 2 is shifted by 1.
    let m = WinMap::new(&[0.0, 1.0], &[2.0, 1.0]).unwrap();
    let [a, b] = run(&m, true);
    assert_bad_then(&a, 4.0);
    assert_bad_then(&b, 3.0);
    let [a, b] = run(&m, false);
    assert_bad_then(&a, 1.0);
    assert_bad_then(&b, 1.0);
}

#[test]
fn permmap_kernel() {
    // Output 1 is input 2, output 2 is the constant 5.
    let m = PermMap::new(&[2, 1], &[2, -1], &[5.0]).unwrap();
    let [a, b] = run(&m, true);
    assert_bad_then(&a, 2.0);
    assert!(b[..5].iter().all(|&v| v == 5.0), "{b:?}");
    assert_eq!(b[5], 7.0);
}

use ast_core::mappings::MatrixMap;

#[test]
fn diagonal_matrixmap_kernel() {
    let m = MatrixMap::diagonal(2, 2, &[2.0, -1.0]).unwrap();
    let [a, b] = run(&m, true);
    assert_bad_then(&a, 4.0);
    assert_bad_then(&b, -2.0);
}

#[test]
fn full_matrixmap_kernel_ignores_a_bad_input_with_a_zero_coefficient() {
    // Output 1 = 1*x + 0*y ; output 2 = 3*x + 2*y.
    let m = MatrixMap::new(ndarray::array![[1.0, 0.0], [3.0, 2.0]]).unwrap();
    let x = [1.0, 2.0, BAD_VALUE];
    let y = [BAD_VALUE, 1.0, 1.0];
    let mut out = [vec![0.0; 3], vec![0.0; 3]];
    let [a, b] = &mut out;
    m.transform_into(
        Points::new([&x[..], &y[..]]).unwrap(),
        true,
        PointsMut::new([&mut a[..], &mut b[..]]).unwrap(),
    )
    .unwrap();
    assert_eq!(out[0], [1.0, 2.0, BAD_VALUE]);
    assert_eq!(out[1], [BAD_VALUE, 8.0, BAD_VALUE]);
}

#[test]
fn full_matrixmap_kernel_crosses_its_block_boundary() {
    let m = MatrixMap::new(ndarray::array![[1.0, 2.0], [3.0, 4.0]]).unwrap();
    let n = 1000;
    let x: Vec<f64> = (0..n).map(f64::from).collect();
    let y: Vec<f64> = (0..n).map(|i| f64::from(i) * 0.5).collect();
    let mut out = [vec![0.0; x.len()], vec![0.0; x.len()]];
    let [a, b] = &mut out;
    m.transform_into(
        Points::new([&x[..], &y[..]]).unwrap(),
        true,
        PointsMut::new([&mut a[..], &mut b[..]]).unwrap(),
    )
    .unwrap();
    for i in 0..x.len() {
        assert_eq!(out[0][i], 0.0 + 1.0 * x[i] + 2.0 * y[i]);
        assert_eq!(out[1][i], 0.0 + 3.0 * x[i] + 4.0 * y[i]);
    }
}

use ast_core::mappings::{CmpMap, Composition};

#[test]
fn a_parallel_cmpmap_splits_its_output_at_the_first_childs_output_count() {
    // First child: 1 input -> 2 outputs (the input and the constant 9).
    let first = PermMap::new(&[1], &[1, -1], &[9.0]).unwrap();
    let second = ZoomMap::new(1, 2.0).unwrap();
    let m = CmpMap::new(Box::new(first), Box::new(second), Composition::Parallel).unwrap();
    let (x, y) = ([1.0, 2.0], [3.0, 4.0]);
    let mut out = [vec![0.0; 2], vec![0.0; 2], vec![0.0; 2]];
    let [a, b, c] = &mut out;
    m.transform_into(
        Points::new([&x[..], &y[..]]).unwrap(),
        true,
        PointsMut::new([&mut a[..], &mut b[..], &mut c[..]]).unwrap(),
    )
    .unwrap();
    assert_eq!(out, [vec![1.0, 2.0], vec![9.0, 9.0], vec![6.0, 8.0]]);
}

#[test]
fn a_series_cmpmap_handles_a_partial_last_batch() {
    let m = CmpMap::new(
        Box::new(ZoomMap::new(1, 2.0).unwrap()),
        Box::new(ShiftMap::new(vec![1.0])),
        Composition::Series,
    )
    .unwrap();
    let n = 8192 + 5;
    let x: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let mut o = vec![0.0; n];
    m.transform_into(
        Points::new([&x[..]]).unwrap(),
        true,
        PointsMut::new([&mut o[..]]).unwrap(),
    )
    .unwrap();
    assert!(o.iter().zip(&x).all(|(&o, &x)| o == 2.0 * x + 1.0));
}

#[test]
fn a_series_cmpmap_of_no_points_still_checks_its_direction() {
    // A non-square MatrixMap has no inverse, so neither has the series.
    let first = MatrixMap::new(ndarray::array![[1.0], [2.0]]).unwrap();
    let m = CmpMap::new(
        Box::new(first),
        Box::new(UnitMap::new(2)),
        Composition::Series,
    )
    .unwrap();
    let (x, y): ([f64; 0], [f64; 0]) = ([], []);
    let mut a: [f64; 0] = [];
    let r = m.transform_into(
        Points::new([&x[..], &y[..]]).unwrap(),
        false,
        PointsMut::new([&mut a[..]]).unwrap(),
    );
    assert!(r.is_err(), "{r:?}");
}

use ast_core::mappings::UnitNormMap;

/// Transform `inp` with `map` into outputs one point longer, which must keep
/// their last value, and check every point bit for bit against `expect`.
fn assert_points(
    map: &dyn Mapping,
    forward: bool,
    inp: &[Vec<f64>],
    nout: usize,
    expect: impl Fn(&[f64]) -> Vec<f64>,
) {
    let n = inp[0].len();
    let mut out = vec![vec![7.0; n + 1]; nout];
    map.transform_into(
        Points::new(inp.iter().map(|a| &a[..])).unwrap(),
        forward,
        PointsMut::new(out.iter_mut().map(|a| &mut a[..])).unwrap(),
    )
    .unwrap();
    for i in 0..n {
        let point: Vec<f64> = inp.iter().map(|a| a[i]).collect();
        let got: Vec<f64> = out.iter().map(|a| a[i]).collect();
        let want = expect(&point);
        assert_eq!(
            got.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            want.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            "point {i} {point:?}: got {got:?}, want {want:?}"
        );
    }
    assert!(
        out.iter().all(|a| a[n] == 7.0),
        "a point past the input was written"
    );
}

const CENTRE: [f64; 2] = [1.0, -2.0];

/// One point of UnitNormMap's forward transformation (unitnormmap.c).
fn unit_norm(p: &[f64]) -> Vec<f64> {
    if p.iter().any(|v| !v.is_finite() || *v == BAD_VALUE) {
        return vec![BAD_VALUE; 3];
    }
    let rel: Vec<f64> = p.iter().zip(CENTRE).map(|(v, c)| v - c).collect();
    let max = rel
        .iter()
        .fold(0.0_f64, |m, r| if r.abs() > m { r.abs() } else { m });
    if max == 0.0 {
        return vec![BAD_VALUE, BAD_VALUE, 0.0];
    }
    let sum = rel.iter().fold(0.0, |s, r| s + (r / max) * (r / max));
    let norm = max * sum.sqrt();
    vec![rel[0] / norm, rel[1] / norm, norm]
}

#[test]
fn unitnormmap_kernel_keeps_each_points_state_across_blocks() {
    let m = UnitNormMap::new(CENTRE.to_vec()).unwrap();
    // Cycle through a good point, a bad first coordinate followed by a larger
    // good one, a bad second coordinate, the centre itself and an infinity,
    // over enough points to cross the kernel's block boundaries.
    let n = 1030;
    let mut inp: Vec<Vec<f64>> = (0..2).map(|_| Vec::with_capacity(n)).collect();
    for i in 0..n {
        let t = f64::from(u32::try_from(i).unwrap());
        let p = match i % 5 {
            0 => [t, -t * 0.5],
            1 => [BAD_VALUE, 1.0e6 + t],
            2 => [3.0 * t, f64::NAN],
            3 => CENTRE,
            _ => [f64::INFINITY, t],
        };
        for (axis, v) in inp.iter_mut().zip(p) {
            axis.push(v);
        }
    }
    assert_points(&m, true, &inp, 3, unit_norm);
}

#[test]
fn unitnormmap_inverse_kernel_keeps_each_points_state() {
    let m = UnitNormMap::new(CENTRE.to_vec()).unwrap();
    // A good point, a bad norm, a zero norm, a bad component, and a bad
    // component with a zero norm, which gives the centre.
    let n = 1030;
    let mut inp: Vec<Vec<f64>> = (0..3).map(|_| Vec::with_capacity(n)).collect();
    for i in 0..n {
        let t = f64::from(u32::try_from(i).unwrap());
        let p = match i % 5 {
            0 => [0.6, 0.8, t],
            1 => [0.6, 0.8, BAD_VALUE],
            2 => [0.6, 0.8, 0.0],
            3 => [f64::NAN, 0.8, t],
            _ => [BAD_VALUE, 0.8, 0.0],
        };
        for (axis, v) in inp.iter_mut().zip(p) {
            axis.push(v);
        }
    }
    assert_points(&m, false, &inp, 2, |p| match p[2] {
        norm if norm == BAD_VALUE => vec![BAD_VALUE; 2],
        0.0 => CENTRE.to_vec(),
        norm => p[..2]
            .iter()
            .zip(CENTRE)
            .map(|(&v, c)| {
                if v.is_finite() {
                    v * norm + c
                } else {
                    BAD_VALUE
                }
            })
            .collect(),
    });
}

/// One output of a full MatrixMap (matrixmap.c `Transform`): BAD if any input
/// with a non-zero coefficient is bad, otherwise the sum, in input order, of
/// coefficient times value over the good inputs.
fn matrix_row(coeffs: &[f64], p: &[f64]) -> f64 {
    let bad = |v: f64| !v.is_finite() || v == BAD_VALUE;
    if coeffs.iter().zip(p).any(|(&c, &v)| c != 0.0 && bad(v)) {
        return BAD_VALUE;
    }
    let mut sum = 0.0;
    for (&c, &v) in coeffs.iter().zip(p) {
        if !bad(v) {
            sum += c * v;
        }
    }
    sum
}

#[test]
fn full_matrixmap_kernel_matches_c_for_each_input_count() {
    // Each input count has its own kernel up to four, and a blocked one
    // above; 150 points cross the blocked kernel's block boundary.
    let odd = [BAD_VALUE, f64::NAN, f64::INFINITY, -0.0];
    for nin in 1..=6 {
        // Row 0 has a zero coefficient on its last input, and a negative one
        // so that -0.0 terms arise.
        let rows: Vec<Vec<f64>> = (0..2)
            .map(|r| {
                (0..nin)
                    .map(|c| {
                        if r == 0 && c == nin - 1 {
                            0.0
                        } else {
                            (r as f64 + 1.0) * 0.5 - c as f64 * 0.75
                        }
                    })
                    .collect()
            })
            .collect();
        let m = MatrixMap::new(ndarray::Array2::from_shape_vec((2, nin), rows.concat()).unwrap())
            .unwrap();
        let inp: Vec<Vec<f64>> = (0..nin)
            .map(|j| {
                (0..150)
                    .map(|i| {
                        if i % (5 + j) == 2 {
                            odd[(i + j) % odd.len()]
                        } else {
                            i as f64 * 0.37 - 20.0 + j as f64
                        }
                    })
                    .collect()
            })
            .collect();
        assert_points(&m, true, &inp, 2, |p| {
            rows.iter().map(|r| matrix_row(r, p)).collect()
        });
    }
}

/// One point of UnitNormMap's forward transformation (unitnormmap.c) about
/// any centre.
fn unit_norm_about(centre: &[f64], p: &[f64]) -> Vec<f64> {
    let n = centre.len();
    if p.iter().any(|v| !v.is_finite() || *v == BAD_VALUE) {
        return vec![BAD_VALUE; n + 1];
    }
    let rel: Vec<f64> = p.iter().zip(centre).map(|(v, c)| v - c).collect();
    let max = rel
        .iter()
        .fold(0.0_f64, |m, r| if r.abs() > m { r.abs() } else { m });
    let mut out = vec![BAD_VALUE; n + 1];
    if max == 0.0 {
        out[n] = 0.0;
        return out;
    }
    let sum = rel.iter().fold(0.0, |s, r| s + (r / max) * (r / max));
    let norm = max * sum.sqrt();
    for (o, r) in out.iter_mut().zip(&rel) {
        *o = r / norm;
    }
    out[n] = norm;
    out
}

#[test]
fn unitnormmap_forward_kernel_matches_c_for_each_axis_count() {
    // Each small axis count has its own kernel and larger ones a blocked
    // kernel; 1030 points cross its block boundary. Every seventh point is
    // the centre, giving a zero norm.
    let odd = [BAD_VALUE, f64::NAN, f64::INFINITY, -0.0];
    for ncoord in 1..=5 {
        let centre: Vec<f64> = (0..ncoord).map(|j| j as f64 * 1.5 - 2.0).collect();
        let m = UnitNormMap::new(centre.clone()).unwrap();
        let inp: Vec<Vec<f64>> = (0..ncoord)
            .map(|j| {
                (0..1030)
                    .map(|i| {
                        if i % 7 == 3 {
                            centre[j]
                        } else if i % (5 + j) == 2 {
                            odd[(i + j) % odd.len()]
                        } else {
                            i as f64 * 0.37 - 20.0 + j as f64
                        }
                    })
                    .collect()
            })
            .collect();
        assert_points(&m, true, &inp, ncoord + 1, |p| unit_norm_about(&centre, p));
    }
}
