use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::{Mapping, WinMap};
use ast_core::{AnyObject, from_native_string, to_native_string};

#[test]
fn winmap_dimensions() {
    let m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();
    assert_eq!(m.nin(), 2);
    assert_eq!(m.nout(), 2);
}

#[test]
fn winmap_forward() {
    // output = shift + scale * input
    // axis 1: 10 + 2 * 1 = 12
    // axis 2: 20 + 3 * 2 = 26
    let m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[12.0, 26.0]);
}

#[test]
fn winmap_inverse() {
    // inverse: (input - shift) / scale
    // axis 1: (12 - 10) / 2 = 1
    // axis 2: (26 - 20) / 3 = 2
    let m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();
    let pts = PointSet::from_rows([[12.0, 26.0]]);
    let out = m.transform(&pts, false).unwrap();
    assert_eq!(out.point(0), &[1.0, 2.0]);
}

#[test]
fn winmap_from_corners() {
    // [0,0]->[10,10] maps to [100,200]->[200,400]
    // axis 1: scale = (200-100)/(10-0) = 10, shift = 100 - 10*0 = 100
    // axis 2: scale = (400-200)/(10-0) = 20, shift = 200 - 20*0 = 200
    let m =
        WinMap::from_corners(&[0.0, 0.0], &[10.0, 10.0], &[100.0, 200.0], &[200.0, 400.0]).unwrap();

    let pts = PointSet::from_rows([[5.0, 5.0]]);
    let out = m.transform(&pts, true).unwrap();
    // axis 1: 100 + 10 * 5 = 150
    // axis 2: 200 + 20 * 5 = 300
    assert_eq!(out.point(0), &[150.0, 300.0]);

    // Check the corners themselves
    let pts_c1 = PointSet::from_rows([[0.0, 0.0]]);
    let out_c1 = m.transform(&pts_c1, true).unwrap();
    assert_eq!(out_c1.point(0), &[100.0, 200.0]);

    let pts_c2 = PointSet::from_rows([[10.0, 10.0]]);
    let out_c2 = m.transform(&pts_c2, true).unwrap();
    assert_eq!(out_c2.point(0), &[200.0, 400.0]);
}

#[test]
fn winmap_from_corners_zero_range() {
    // If input range is zero, shift and scale become BAD_VALUE
    let m =
        WinMap::from_corners(&[5.0, 0.0], &[5.0, 10.0], &[100.0, 200.0], &[200.0, 400.0]).unwrap();

    let pts = PointSet::from_rows([[5.0, 5.0]]);
    let out = m.transform(&pts, true).unwrap();
    // axis 1 is BAD (zero input range), axis 2 is fine
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], 300.0);
}

#[test]
fn winmap_zero_scale_inverse() {
    // A zero scale makes the inverse undefined on that axis
    let m = WinMap::new(&[10.0, 20.0], &[0.0, 3.0]).unwrap();
    let pts = PointSet::from_rows([[12.0, 26.0]]);
    let out = m.transform(&pts, false).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], 2.0);
}

#[test]
fn winmap_bad_input_propagation() {
    let m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();

    // BAD_VALUE in input -> all outputs BAD
    let pts = PointSet::from_rows([[BAD_VALUE, 2.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], BAD_VALUE);

    // NaN in input -> all outputs BAD
    let pts_nan = PointSet::from_rows([[f64::NAN, 2.0]]);
    let out_nan = m.transform(&pts_nan, true).unwrap();
    assert_eq!(out_nan.point(0)[0], BAD_VALUE);
    assert_eq!(out_nan.point(0)[1], BAD_VALUE);
}

#[test]
fn winmap_bad_shift_propagation() {
    // Axis 1 has BAD shift -> output axis 1 is BAD, axis 2 is fine
    let m = WinMap::new(&[BAD_VALUE, 20.0], &[2.0, 3.0]).unwrap();
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out = m.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
    assert_eq!(out.point(0)[1], 26.0);
}

#[test]
fn winmap_rate_diagonal() {
    let m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();
    // On-diagonal: rate = scale for that axis
    let r1 = m.rate(&[1.0, 1.0], 1, 1).unwrap();
    assert!((r1 - 2.0).abs() < 1e-10);
    let r2 = m.rate(&[1.0, 1.0], 2, 2).unwrap();
    assert!((r2 - 3.0).abs() < 1e-10);
}

#[test]
fn winmap_rate_off_diagonal() {
    let m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();
    let r = m.rate(&[1.0, 1.0], 1, 2).unwrap();
    assert_eq!(r, 0.0);
    let r2 = m.rate(&[1.0, 1.0], 2, 1).unwrap();
    assert_eq!(r2, 0.0);
}

#[test]
fn winmap_rate_inverted() {
    let mut m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();
    m.invert();
    // When inverted, rate = 1/scale
    let r1 = m.rate(&[1.0, 1.0], 1, 1).unwrap();
    assert!((r1 - 0.5).abs() < 1e-10);
    let r2 = m.rate(&[1.0, 1.0], 2, 2).unwrap();
    assert!((r2 - 1.0 / 3.0).abs() < 1e-10);
}

#[test]
fn winmap_invert_flag() {
    let mut m = WinMap::new(&[10.0], &[2.0]).unwrap();
    assert!(!m.is_inverted());
    m.invert();
    assert!(m.is_inverted());

    // After invert, forward applies the inverse formula
    let pts = PointSet::from_rows([[12.0]]);
    let out = m.transform(&pts, true).unwrap();
    // "forward" on inverted map = inverse: (12 - 10) / 2 = 1
    assert_eq!(out.point(0), &[1.0]);
}

#[test]
fn winmap_serialisation_round_trip() {
    let m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();
    let text = to_native_string(&AnyObject::WinMap(m.clone())).unwrap();
    let restored = from_native_string(&text).unwrap();
    let m2 = restored.into_mapping().unwrap();

    // Verify the restored mapping produces the same result
    let pts = PointSet::from_rows([[1.0, 2.0]]);
    let out1 = m.transform(&pts, true).unwrap();
    let out2 = m2.transform(&pts, true).unwrap();
    assert_eq!(out1.point(0), out2.point(0));
}

#[test]
fn winmap_serialisation_text_content() {
    let m = WinMap::new(&[10.0, 20.0], &[2.0, 3.0]).unwrap();
    let text = to_native_string(&AnyObject::WinMap(m)).unwrap();
    assert!(text.contains("Begin WinMap"));
    assert!(text.contains("Sft1"));
    assert!(text.contains("Sft2"));
    assert!(text.contains("Scl1"));
    assert!(text.contains("Scl2"));
    assert!(text.contains("End WinMap"));
}

#[test]
fn winmap_serialisation_default_omission() {
    // Zero shifts and unit scales are omitted from native output
    let m = WinMap::new(&[0.0, 0.0], &[1.0, 1.0]).unwrap();
    let text = to_native_string(&AnyObject::WinMap(m)).unwrap();
    assert!(text.contains("Begin WinMap"));
    assert!(!text.contains("Sft1"));
    assert!(!text.contains("Sft2"));
    assert!(!text.contains("Scl1"));
    assert!(!text.contains("Scl2"));
}

#[test]
fn winmap_validation_mismatched_lengths() {
    let result = WinMap::new(&[1.0, 2.0], &[3.0]);
    assert!(result.is_err());
}

#[test]
fn winmap_validation_empty() {
    let result = WinMap::new(&[], &[]);
    assert!(result.is_err());
}
