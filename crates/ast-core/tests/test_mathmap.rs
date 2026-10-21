//! Tests for MathMap — mathematical expression mapping.
//!
//! Ported from reference/starlink-ast/ast_tester/testmathmap.c
//!
//! Variable naming convention: the forward transform's input variable names
//! are taken from the last `nin` inverse expressions' LHS names. The inverse
//! transform's input variable names are taken from the last `nout` forward
//! expressions' LHS names. This cross-referencing is how the C library works.

use ast_core::coords::PointSet;
use ast_core::error::ErrorCode;
use ast_core::mappings::{Mapping, MathMap};

/// Helper: transform a single point through a 1-in, 1-out MathMap.
fn transform_1d(map: &MathMap, x: f64, forward: bool) -> f64 {
    let pts = PointSet::from_rows(vec![[x]]);
    let out = map.transform(&pts, forward).unwrap();
    out.point(0)[0]
}

/// Helper: transform a single point through a 2-in, 2-out MathMap.
fn transform_2d(map: &MathMap, x1: f64, x2: f64, forward: bool) -> (f64, f64) {
    let pts = PointSet::from_rows(vec![[x1, x2]]);
    let out = map.transform(&pts, forward).unwrap();
    let row = out.point(0);
    (row[0], row[1])
}

#[test]
fn test_basic_arithmetic() {
    let map = MathMap::new(1, 1, &["y = x + 1"], &["x = y - 1"]).unwrap();

    assert_eq!(map.nin(), 1);
    assert_eq!(map.nout(), 1);

    let result = transform_1d(&map, 5.0, true);
    assert!((result - 6.0).abs() < 1e-10);

    let result = transform_1d(&map, 6.0, false);
    assert!((result - 5.0).abs() < 1e-10);
}

#[test]
fn test_multiplication() {
    let map = MathMap::new(1, 1, &["y = x * 3"], &["x = y / 3"]).unwrap();

    let result = transform_1d(&map, 4.0, true);
    assert!((result - 12.0).abs() < 1e-10);

    let result = transform_1d(&map, 12.0, false);
    assert!((result - 4.0).abs() < 1e-10);
}

#[test]
fn test_power_operator() {
    let map = MathMap::new(1, 1, &["y = x ** 2"], &["x = sqrt(y)"]).unwrap();

    let result = transform_1d(&map, 5.0, true);
    assert!((result - 25.0).abs() < 1e-10);

    let result = transform_1d(&map, 25.0, false);
    assert!((result - 5.0).abs() < 1e-10);
}

#[test]
fn test_trig_functions() {
    let map = MathMap::new(1, 1, &["y = sin(x)"], &["x = asin(y)"]).unwrap();

    let result = transform_1d(&map, 0.5, true);
    assert!((result - 0.5_f64.sin()).abs() < 1e-10);

    let result = transform_1d(&map, 0.5_f64.sin(), false);
    assert!((result - 0.5).abs() < 1e-10);
}

#[test]
fn test_trig_degrees() {
    let map = MathMap::new(1, 1, &["y = sind(x)"], &["x = asind(y)"]).unwrap();

    let result = transform_1d(&map, 30.0, true);
    assert!((result - 0.5).abs() < 1e-10);

    let result = transform_1d(&map, 0.5, false);
    assert!((result - 30.0).abs() < 1e-10);
}

#[test]
fn test_system_constants() {
    let map = MathMap::new(1, 1, &["y = <pi>"], &["x = <pi>"]).unwrap();

    let result = transform_1d(&map, 0.0, true);
    assert!((result - std::f64::consts::PI).abs() < 1e-14);
}

#[test]
fn test_bad_value_propagation() {
    use ast_core::constants::BAD_VALUE;

    let map = MathMap::new(1, 1, &["y = x + 1"], &["x = y - 1"]).unwrap();

    let result = transform_1d(&map, BAD_VALUE, true);
    assert_eq!(result, BAD_VALUE);
}

#[test]
fn test_isbad_function() {
    use ast_core::constants::BAD_VALUE;

    let map = MathMap::new(1, 1, &["y = isbad(x)"], &["x = y"]).unwrap();

    let result = transform_1d(&map, 42.0, true);
    assert!((result - 0.0).abs() < 1e-10);

    let result = transform_1d(&map, BAD_VALUE, true);
    assert!((result - 1.0).abs() < 1e-10);
}

#[test]
fn test_two_dimensional() {
    // Swap coordinates
    let map = MathMap::new(
        2,
        2,
        &["yout1 = xin2", "yout2 = xin1"],
        &["xin1 = yout2", "xin2 = yout1"],
    )
    .unwrap();

    let (r1, r2) = transform_2d(&map, 3.0, 7.0, true);
    assert!((r1 - 7.0).abs() < 1e-10);
    assert!((r2 - 3.0).abs() < 1e-10);

    let (r1, r2) = transform_2d(&map, 7.0, 3.0, false);
    assert!((r1 - 3.0).abs() < 1e-10);
    assert!((r2 - 7.0).abs() < 1e-10);
}

#[test]
fn test_intermediate_variables() {
    let map = MathMap::new(
        2,
        2,
        &[
            "r = sqrt(xin1*xin1 + xin2*xin2)",
            "theta = atan2(xin2, xin1)",
            "yout1 = r",
            "yout2 = theta",
        ],
        &["xin1 = yout1 * cos(yout2)", "xin2 = yout1 * sin(yout2)"],
    )
    .unwrap();

    let (r, theta) = transform_2d(&map, 3.0, 4.0, true);
    assert!((r - 5.0).abs() < 1e-10);
    assert!((theta - (4.0_f64).atan2(3.0)).abs() < 1e-10);
}

#[test]
fn test_comparison_operators() {
    let map = MathMap::new(1, 1, &["y = x > 0"], &["x = y"]).unwrap();

    let result = transform_1d(&map, 5.0, true);
    assert!((result - 1.0).abs() < 1e-10);

    let result = transform_1d(&map, -5.0, true);
    assert!(result.abs() < 1e-10);
}

#[test]
fn test_max_min_functions() {
    let map = MathMap::new(1, 1, &["y = max(x, 0, 10)"], &["x = y"]).unwrap();

    let result = transform_1d(&map, -5.0, true);
    assert!((result - 10.0).abs() < 1e-10);

    let result = transform_1d(&map, 5.0, true);
    assert!((result - 10.0).abs() < 1e-10);

    let result = transform_1d(&map, 15.0, true);
    assert!((result - 15.0).abs() < 1e-10);
}

#[test]
fn test_exp_log() {
    let map = MathMap::new(1, 1, &["y = exp(x)"], &["x = log(y)"]).unwrap();

    let result = transform_1d(&map, 1.0, true);
    assert!((result - std::f64::consts::E).abs() < 1e-10);

    let result = transform_1d(&map, std::f64::consts::E, false);
    assert!((result - 1.0).abs() < 1e-10);
}

#[test]
fn test_abs_function() {
    let map = MathMap::new(1, 1, &["y = abs(x)"], &["x = y"]).unwrap();

    let result = transform_1d(&map, -3.0, true);
    assert!((result - 3.0).abs() < 1e-10);

    let result = transform_1d(&map, 3.0, true);
    assert!((result - 3.0).abs() < 1e-10);
}

#[test]
fn test_qif_ternary() {
    let map = MathMap::new(1, 1, &["y = qif(x > 0, x, -x)"], &["x = y"]).unwrap();

    let result = transform_1d(&map, 5.0, true);
    assert!((result - 5.0).abs() < 1e-10);

    let result = transform_1d(&map, -5.0, true);
    assert!((result - 5.0).abs() < 1e-10);
}

#[test]
fn test_simplification_simpfi() {
    // Two identical MathMaps, both with SimpFI set.
    // Applied in series: first forward, then inverse (other_inv=true).
    // With other_inv=true, the simplification check uses other.simp_fi.
    let mut map1 = MathMap::new(1, 1, &["y = x * 2"], &["x = y / 2"]).unwrap();
    map1.set_simp_fi(true);

    let mut map2 = MathMap::new(1, 1, &["y = x * 2"], &["x = y / 2"]).unwrap();
    map2.set_simp_fi(true); // With other_inv=true, checks simp_fi

    let result = map1.try_merge(&map2, true, false, true);
    assert!(
        result.is_some(),
        "Identical MathMaps should simplify (forward then inverse)"
    );
}

#[test]
fn test_invert() {
    let mut map = MathMap::new(1, 1, &["y = x + 10"], &["x = y - 10"]).unwrap();

    let result = transform_1d(&map, 5.0, true);
    assert!((result - 15.0).abs() < 1e-10);

    map.invert();
    let result = transform_1d(&map, 15.0, true);
    assert!((result - 5.0).abs() < 1e-10);
}

#[test]
fn test_multipoint_transform() {
    let map = MathMap::new(1, 1, &["y = x * x"], &["x = sqrt(y)"]).unwrap();

    let pts = PointSet::from_rows(vec![[1.0], [2.0], [3.0], [4.0], [5.0]]);
    let out = map.transform(&pts, true).unwrap();

    let expected = [1.0, 4.0, 9.0, 16.0, 25.0];
    for (i, &exp) in expected.iter().enumerate() {
        assert!((out.point(i)[0] - exp).abs() < 1e-10, "point {} failed", i);
    }
}

#[test]
fn test_division_by_zero() {
    use ast_core::constants::BAD_VALUE;

    let map = MathMap::new(1, 1, &["y = 1.0 / x"], &["x = 1.0 / y"]).unwrap();

    let result = transform_1d(&map, 0.0, true);
    assert_eq!(result, BAD_VALUE);
}

#[test]
fn test_sqrt_negative() {
    use ast_core::constants::BAD_VALUE;

    let map = MathMap::new(1, 1, &["y = sqrt(x)"], &["x = y * y"]).unwrap();

    let result = transform_1d(&map, -1.0, true);
    assert_eq!(result, BAD_VALUE);
}

#[test]
fn test_mod_function() {
    let map = MathMap::new(1, 1, &["y = mod(x, 3)"], &["x = y"]).unwrap();

    let result = transform_1d(&map, 7.0, true);
    assert!((result - 1.0).abs() < 1e-10);

    let result = transform_1d(&map, -7.0, true);
    assert!((result - (-1.0)).abs() < 1e-10);
}

#[test]
fn test_floor_ceil_nint() {
    let map_floor = MathMap::new(1, 1, &["y = floor(x)"], &["x = y"]).unwrap();
    let map_ceil = MathMap::new(1, 1, &["y = ceil(x)"], &["x = y"]).unwrap();
    let map_nint = MathMap::new(1, 1, &["y = nint(x)"], &["x = y"]).unwrap();

    assert!((transform_1d(&map_floor, 2.7, true) - 2.0).abs() < 1e-10);
    assert!((transform_1d(&map_floor, -2.7, true) - (-3.0)).abs() < 1e-10);
    assert!((transform_1d(&map_ceil, 2.3, true) - 3.0).abs() < 1e-10);
    assert!((transform_1d(&map_ceil, -2.3, true) - (-2.0)).abs() < 1e-10);
    assert!((transform_1d(&map_nint, 2.3, true) - 2.0).abs() < 1e-10);
    assert!((transform_1d(&map_nint, 2.7, true) - 3.0).abs() < 1e-10);
}

#[test]
fn test_boolean_and_tristate() {
    use ast_core::constants::BAD_VALUE;

    // Three-valued AND: BAD .and. 0 = 0
    let map = MathMap::new(1, 1, &["y = x && 0"], &["x = y"]).unwrap();
    let result = transform_1d(&map, BAD_VALUE, true);
    assert!((result - 0.0).abs() < 1e-10);

    // Three-valued OR: BAD .or. 1 = 1
    let map = MathMap::new(1, 1, &["y = x || 1"], &["x = y"]).unwrap();
    let result = transform_1d(&map, BAD_VALUE, true);
    assert!((result - 1.0).abs() < 1e-10);
}

/// Evaluate `expr` as the forward transformation of a 1-in, 1-out MathMap at
/// `x`.
fn eval_at(expr: &str, x: f64) -> f64 {
    let fwd = format!("y = {expr}");
    let map = MathMap::new(1, 1, &[fwd.as_str()], &["x"]).unwrap();
    transform_1d(&map, x, true)
}

/// The binary operators bind with the priorities of C's operator table
/// (mathmap.c, the `symbol` array): `.neqv.` and `.xor.` compute what `^^`
/// computes but bind as loosely as `.eqv.`. Expected values are from the C
/// library at x = 2 and x = 1.
#[test]
fn binary_operator_priorities_match_c() {
    let cases: &[(&str, f64, f64)] = &[
        ("x .or. x .xor. x", 0.0, 0.0),
        ("x .or. x .neqv. x", 0.0, 0.0),
        ("x || x ^^ x", 1.0, 1.0),
        ("x ^^ x .eqv. x", 0.0, 0.0),
        ("x .and. x .xor. x", 0.0, 0.0),
        ("x .xor. x .eqv. 0", 1.0, 1.0),
        ("x < 3 == 1", 1.0, 1.0),
        ("x << 1 < 3", 0.0, 1.0),
        ("-x * 3", -6.0, -3.0),
    ];
    for &(expr, at2, at1) in cases {
        assert_eq!(eval_at(expr, 2.0), at2, "{expr} at x = 2");
        assert_eq!(eval_at(expr, 1.0), at1, "{expr} at x = 1");
    }
}

/// A prefix operator binds less tightly than a `**` on its right, as in C's
/// operator table: `-x**2` is `-(x**2)`. A prefix operator on the right of
/// `**` still applies to its operand. Expected values are from the C library
/// at x = 2 and x = 1.
#[test]
fn prefix_operators_bind_below_power() {
    let cases: &[(&str, f64, f64)] = &[
        ("-x**2", -4.0, -1.0),
        ("+x**2", 4.0, 1.0),
        (".not. x**2", 0.0, 0.0),
        ("!x**2", 0.0, 0.0),
        ("-x**2**2", -16.0, -1.0),
        ("x**-2", 0.25, 1.0),
        ("2**-x**2", 0.0625, 0.5),
        ("!x == 0", 1.0, 1.0),
        ("-x < -1", 1.0, 0.0),
    ];
    for &(expr, at2, at1) in cases {
        assert_eq!(eval_at(expr, 2.0), at2, "{expr} at x = 2");
        assert_eq!(eval_at(expr, 1.0), at1, "{expr} at x = 1");
    }
}

/// The error code `MathMap::new` reports for a malformed forward expression.
fn expression_error(expr: &str) -> ErrorCode {
    let fwd = format!("y = {expr}");
    match MathMap::new(1, 1, &[fwd.as_str()], &["x"]) {
        Ok(_) => panic!("\"{expr}\" compiled"),
        Err(e) => e.code(),
    }
}

/// Each symbol is read as an operator or as an operand according to what
/// precedes it, as in C: after an operand only an operator may follow, so a
/// constant ending in `.` followed by a dotted operator's name is a missing
/// operator, and text that reads as no operator is a missing operator.
/// Expected codes are from the C library.
#[test]
fn malformed_expressions_report_c_error_codes() {
    let cases: &[(&str, ErrorCode)] = &[
        ("1.or.0", ErrorCode::Miopr),
        ("1 .and. 1", ErrorCode::Miopr),
        ("x .not. 1", ErrorCode::Miopr),
        ("2x", ErrorCode::Miopr),
        ("x(1)", ErrorCode::Miopr),
        ("x!", ErrorCode::Miopr),
        ("(x)(x)", ErrorCode::Miopr),
        ("2.eq.2", ErrorCode::Conin),
        (".eq.x", ErrorCode::Conin),
        ("1.e", ErrorCode::Conin),
        ("1.0.5", ErrorCode::Conin),
        ("x<bad>", ErrorCode::Udvof),
        ("sin", ErrorCode::Udvof),
        ("q", ErrorCode::Udvof),
        ("*x", ErrorCode::Miopa),
        ("max(1,)", ErrorCode::Miopa),
        ("x\u{b7}2", ErrorCode::Miopr),
    ];
    for &(expr, code) in cases {
        assert_eq!(expression_error(expr), code, "{expr}");
    }
    assert_eq!(eval_at("x.or.1", 0.0), 1.0);
    assert_eq!(eval_at("1.e1 + 1.d1 + .5e-1 + 2.5d+1", 0.0), 45.05);
}

/// The bit-wise operators read each value as a fixed-point binary number
/// with negative values in two's complement, as C does, so integer arguments
/// give the integer results and fractional bits take part too. Expected
/// values are from the C library.
#[test]
fn bitwise_operators_act_on_fixed_point_values() {
    let cases: &[(&str, f64)] = &[
        ("2^3", 1.0),
        ("2|1", 3.0),
        ("1&3", 1.0),
        ("-1&3", 3.0),
        ("-2|1", -1.0),
        ("-1^-1", 0.0),
        ("0.5|0.25", 0.75),
        ("1.5^0.75", 1.25),
        ("-0.5&1", 1.0),
        ("3&-1e300", 0.0),
        ("<max>|0", f64::MAX),
        ("1e-310|1", 1.0),
        ("1e-320|1e-310", 1.000_000_000_022_13e-310),
        ("1e308&1e308", 1e308),
        ("-<max>|1", -f64::MAX),
        ("0&0", 0.0),
        ("-0^1", 1.0),
        ("3.75&-2.5", 1.5),
        ("1e20|1", 1e20),
        ("1e-20^-1", -1.0),
        ("<min>|-<min>", -f64::MIN_POSITIVE),
        ("x|2", 2.0),
        ("-1&-1", -1.0),
    ];
    for &(expr, expected) in cases {
        assert_eq!(eval_at(expr, 0.0), expected, "{expr}");
    }
}

/// The shift operators scale by a power of two with a single rounding, so
/// they reach through the subnormal numbers and up to the largest double.
/// Expected values are from the C library.
#[test]
fn shift_operators_scale_exactly() {
    let cases: &[(&str, f64)] = &[
        ("1e-310<<1", 2e-310),
        ("<max>>>1<<1", f64::MAX),
        ("1<<-1074", 5e-324),
        ("1<<-1075", 0.0),
        ("1.5<<-1074", 1e-323),
        ("<min>>>3", 2.781_342_323_134e-309),
        ("0.75<<1024", 1.348_269_851_146_737e308),
    ];
    for &(expr, expected) in cases {
        assert_eq!(eval_at(expr, 0.0), expected, "{expr}");
    }
}
