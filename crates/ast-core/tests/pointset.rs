use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;

#[test]
fn zeros_creates_correct_shape() {
    let ps = PointSet::zeros(10, 3);
    assert_eq!(ps.npoint(), 10);
    assert_eq!(ps.ncoord(), 3);
    assert_eq!(ps.point(0), &[0.0, 0.0, 0.0]);
}

#[test]
fn from_rows_basic() {
    let ps = PointSet::from_rows([[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]]);
    assert_eq!(ps.npoint(), 3);
    assert_eq!(ps.ncoord(), 2);
    assert_eq!(ps.point(0), &[1.0, 2.0]);
    assert_eq!(ps.point(2), &[5.0, 6.0]);
}

#[test]
fn from_columns_basic() {
    let x = [1.0, 2.0, 3.0];
    let y = [4.0, 5.0, 6.0];
    let ps = PointSet::from_columns(&[&x, &y]).unwrap();
    assert_eq!(ps.npoint(), 3);
    assert_eq!(ps.ncoord(), 2);
    assert_eq!(ps.point(0), &[1.0, 4.0]);
    assert_eq!(ps.point(1), &[2.0, 5.0]);
}

#[test]
fn from_columns_mismatched_lengths() {
    let x = [1.0, 2.0, 3.0];
    let y = [4.0, 5.0];
    let result = PointSet::from_columns(&[&x, &y]);
    assert!(result.is_err());
}

#[test]
fn from_columns_empty() {
    let result = PointSet::from_columns(&[]);
    assert!(result.is_err());
}

#[test]
fn set_point_modifies_in_place() {
    let mut ps = PointSet::zeros(2, 3);
    ps.set_point(0, &[0.0, 42.0, 0.0]);
    assert_eq!(ps.point(0), &[0.0, 42.0, 0.0]);
}

#[test]
fn each_axis_is_one_contiguous_slice() {
    let ps = PointSet::from_rows([[1.0, 10.0], [2.0, 20.0], [3.0, 30.0]]);
    assert_eq!(ps.axis(0), &[1.0, 2.0, 3.0]);
    assert_eq!(ps.axis(1), &[10.0, 20.0, 30.0]);
    assert_eq!(ps.view().shape(), &[2, 3]);
}

#[test]
fn a_point_set_and_its_views_agree() {
    let mut ps = PointSet::from_rows([[1.0, 10.0], [2.0, 20.0]]);
    assert_eq!(ps.as_points().point(1), &[2.0, 20.0]);
    ps.as_points_mut().axis_mut(1)[0] = 5.0;
    assert_eq!(ps.point(0), &[1.0, 5.0]);
    let copy = PointSet::from_points(&ps.as_points());
    assert_eq!(copy.point(0), &[1.0, 5.0]);
}

#[test]
fn a_point_set_of_no_points_has_empty_axes() {
    let mut ps = PointSet::zeros(0, 2);
    assert_eq!(ps.as_points().ncoord(), 2);
    assert_eq!(ps.as_points_mut().npoint(), 0);
}

#[test]
fn axis_iter_returns_column() {
    let ps = PointSet::from_rows([[1.0, 10.0], [2.0, 20.0], [3.0, 30.0]]);
    let col0: Vec<f64> = ps.axis_iter(0).collect();
    let col1: Vec<f64> = ps.axis_iter(1).collect();
    assert_eq!(col0, vec![1.0, 2.0, 3.0]);
    assert_eq!(col1, vec![10.0, 20.0, 30.0]);
}

#[test]
fn view_and_into_ndarray() {
    let ps = PointSet::from_rows([[1.0, 2.0]]);
    let view = ps.view();
    assert_eq!(view[[0, 0]], 1.0);
    assert_eq!(view[[1, 0]], 2.0);

    let arr = ps.into_ndarray();
    assert_eq!(arr.shape(), &[2, 1]);
}

#[test]
#[allow(clippy::assertions_on_constants)] // entire purpose is to verify the constant's properties
fn bad_value_constant() {
    assert_eq!(BAD_VALUE, -f64::MAX);
    assert!(BAD_VALUE.is_finite());
    assert!(BAD_VALUE < 0.0);
}
