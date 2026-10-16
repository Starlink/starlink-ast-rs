//! Borrowed per-axis views of points.

use ast_core::coords::{PointBuf, Points, PointsMut};
use ast_core::error::ErrorCode;

#[test]
fn a_view_over_columns_reads_points_and_axes() {
    let (x, y) = ([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]);
    let p = Points::new([&x[..], &y[..]]).unwrap();
    assert_eq!((p.npoint(), p.ncoord()), (3, 2));
    assert_eq!(p.point(1), &[2.0, 5.0]);
    assert_eq!(p.axis(1), &[4.0, 5.0, 6.0]);
}

#[test]
fn no_axes_is_ncoin() {
    let e = Points::new(std::iter::empty::<&[f64]>()).unwrap_err();
    assert_eq!(e.code(), ErrorCode::Ncoin);
}

#[test]
fn one_short_axis_is_ncpin() {
    let (x, y) = ([1.0, 2.0, 3.0], [4.0, 5.0]);
    let e = Points::new([&x[..], &y[..]]).unwrap_err();
    assert_eq!(e.code(), ErrorCode::Ncpin);
}

#[test]
fn the_trann_layout_reads_axes_at_a_stride() {
    // Two axes of 3 points in a buffer whose axis dimension (indim) is 5.
    let buf = [1.0, 2.0, 3.0, -9.0, -9.0, 4.0, 5.0, 6.0];
    let p = Points::from_strided(&buf, 2, 3, 5).unwrap();
    assert_eq!(p.axis(0), &[1.0, 2.0, 3.0]);
    assert_eq!(p.axis(1), &[4.0, 5.0, 6.0]);
}

#[test]
fn a_stride_below_npoint_is_dimin() {
    let buf = [0.0; 8];
    assert_eq!(
        Points::from_strided(&buf, 2, 3, 2).unwrap_err().code(),
        ErrorCode::Dimin
    );
    let mut buf = [0.0; 8];
    assert_eq!(
        PointsMut::from_strided(&mut buf, 2, 3, 2)
            .unwrap_err()
            .code(),
        ErrorCode::Dimin
    );
}

#[test]
fn a_buffer_too_short_for_its_stride_is_dimin() {
    let buf = [0.0; 7];
    assert_eq!(
        Points::from_strided(&buf, 2, 3, 5).unwrap_err().code(),
        ErrorCode::Dimin
    );
}

#[test]
fn writable_strided_views_write_only_their_points() {
    let mut buf = [0.0; 8];
    {
        let mut o = PointsMut::from_strided(&mut buf, 2, 3, 5).unwrap();
        o.set_point(1, &[7.0, 8.0]);
        o.axis_mut(0)[2] = 9.0;
    }
    assert_eq!(buf, [0.0, 7.0, 9.0, 0.0, 0.0, 0.0, 8.0, 0.0]);
}

#[test]
fn point_and_axis_ranges_are_views_of_the_same_data() {
    let (x, y, z) = ([1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]);
    let p = Points::new([&x[..], &y[..], &z[..]]).unwrap();
    let q = p.points(1..3).axes(1..3);
    assert_eq!((q.npoint(), q.ncoord()), (2, 2));
    assert_eq!(q.point(0), &[5.0, 8.0]);
}

#[test]
fn splitting_writable_axes_gives_disjoint_views() {
    let (mut a, mut b, mut c) = ([0.0; 2], [0.0; 2], [0.0; 2]);
    {
        let o = PointsMut::new([&mut a[..], &mut b[..], &mut c[..]]).unwrap();
        let (mut left, mut right) = o.split_axes_at(1);
        left.axis_mut(0)[0] = 1.0;
        right.axis_mut(1)[1] = 2.0;
    }
    assert_eq!((a, b, c), ([1.0, 0.0], [0.0; 2], [0.0, 2.0]));
}

#[test]
fn zero_points_is_a_valid_view() {
    let (x, y): ([f64; 0], [f64; 0]) = ([], []);
    let p = Points::new([&x[..], &y[..]]).unwrap();
    assert_eq!((p.npoint(), p.ncoord()), (0, 2));
}

#[test]
fn nine_axes_spill_to_the_heap_with_the_same_results() {
    let cols: Vec<Vec<f64>> = (0..9).map(|j| vec![j as f64; 2]).collect();
    let p = Points::new(cols.iter().map(Vec::as_slice)).unwrap();
    assert_eq!(p.point(1), (0..9).map(f64::from).collect::<Vec<_>>());
}

#[test]
fn point_bufs_compare_with_slices_arrays_and_vectors() {
    let b: PointBuf = [1.0, 2.0].into_iter().collect();
    assert_eq!(b, [1.0, 2.0]);
    assert_eq!(b, &[1.0, 2.0]);
    assert_eq!(b, &[1.0, 2.0][..]);
    assert_eq!(b, vec![1.0, 2.0]);
    assert_eq!(b[1], 2.0);
}

use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, ZoomMap};

#[test]
fn a_transform_can_write_into_a_callers_trann_buffers() {
    let zoom = ZoomMap::new(2, 2.0).unwrap();
    let input = [1.0, 2.0, 3.0, 0.0, 10.0, 20.0, 30.0];
    let mut output = [-1.0; 8];
    zoom.transform_into(
        Points::from_strided(&input, 2, 3, 4).unwrap(),
        true,
        PointsMut::from_strided(&mut output, 2, 3, 5).unwrap(),
    )
    .unwrap();
    assert_eq!(output, [2.0, 4.0, 6.0, -1.0, -1.0, 20.0, 40.0, 60.0]);
}

#[test]
fn tran2_matches_transform() {
    let zoom = ZoomMap::new(2, 1.5).unwrap();
    let (x, y) = (vec![1.0, -2.0, 3.5], vec![4.0, 0.25, -6.0]);
    let (xo, yo) = zoom.tran2(&x, &y, true).unwrap();
    let out = zoom
        .transform(&PointSet::from_columns(&[&x, &y]).unwrap(), true)
        .unwrap();
    assert_eq!(xo, out.axis(0));
    assert_eq!(yo, out.axis(1));
}

#[test]
fn a_transform_of_no_points_still_checks_the_axes() {
    let zoom = ZoomMap::new(2, 2.0).unwrap();
    let (x, y, z): ([f64; 0], [f64; 0], [f64; 0]) = ([], [], []);
    let mut out: [[f64; 0]; 2] = [[], []];
    let [a, b] = &mut out;
    let e = zoom
        .transform_into(
            Points::new([&x[..], &y[..], &z[..]]).unwrap(),
            true,
            PointsMut::new([&mut a[..], &mut b[..]]).unwrap(),
        )
        .unwrap_err();
    assert_eq!(e.code(), ErrorCode::Ncpin);
}
