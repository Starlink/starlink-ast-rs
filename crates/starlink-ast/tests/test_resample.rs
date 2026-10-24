//! `mappings::resample` on known data.
//!
//! C ancestor: `reference/starlink-ast/ast_tester/testresample.c`, tests 1
//! to 9; test 10 (`astQuadApprox`) is the `poly` case of
//! `test_tran_grid_oracle.rs`.
//!
//! Divergence from C: the arrays and bounds are passed as slices, so the
//! variance arrays are `Option`s rather than pointers, and `AST__USEVAR`,
//! which C's resampling ignores, has no counterpart.

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::mappings::{
    Interp, Mapping, ResampleFlags, ResampleInput, ResampleOptions, ResampleOutput, ShiftMap,
    UnitMap, resample,
};
use starlink_ast::pixel::PixelValue;

/// Resample `data` (with `var`) on `[lbnd_in, ubnd_in]` through `map` into
/// the whole of the grid `[lbnd, ubnd]`, starting from `out`.
#[allow(clippy::too_many_arguments)]
fn run<T: PixelValue>(
    map: &dyn Mapping,
    lbnd_in: &[i64],
    ubnd_in: &[i64],
    data: &[T],
    var: Option<&[T]>,
    interp: Interp<'_, T>,
    flags: ResampleFlags,
    badval: T,
    lbnd: &[i64],
    ubnd: &[i64],
    out: &mut [T],
    out_var: Option<&mut [T]>,
) -> usize {
    resample(
        map,
        &ResampleInput {
            lbnd: lbnd_in,
            ubnd: ubnd_in,
            data,
            var,
        },
        &mut ResampleOutput {
            lbnd,
            ubnd,
            region_lbnd: lbnd,
            region_ubnd: ubnd,
            data: out,
            var: out_var,
        },
        &ResampleOptions {
            interp,
            params: &[2.0, 2.0],
            flags,
            tol: 0.0,
            maxpix: 100,
        },
        badval,
    )
    .unwrap()
}

#[test]
fn test1_unitmap_is_the_identity() {
    let input: Vec<f64> = (1..=30).map(f64::from).collect();
    for interp in [
        Interp::Nearest,
        Interp::Linear,
        Interp::Sinc,
        Interp::SincSinc,
        Interp::SincCos,
        Interp::SincGauss,
        Interp::SombCos,
    ] {
        let mut out = vec![0.0; 30];
        run(
            &UnitMap::new(1),
            &[1],
            &[30],
            &input,
            None,
            interp,
            ResampleFlags::default(),
            BAD_VALUE,
            &[1],
            &[30],
            &mut out,
            None,
        );
        for i in 3..27 {
            assert!((out[i] - input[i]).abs() <= 1.0e-10, "{interp:?} pixel {i}");
        }
    }
}

/// The smoothing schemes reproduce constant data.
#[test]
fn test1_smoothing_schemes_keep_constant_data() {
    let input = vec![42.0; 30];
    for interp in [Interp::Gauss, Interp::Somb, Interp::BlockAve] {
        let mut out = vec![0.0; 30];
        run(
            &UnitMap::new(1),
            &[1],
            &[30],
            &input,
            None,
            interp,
            ResampleFlags::default(),
            BAD_VALUE,
            &[1],
            &[30],
            &mut out,
            None,
        );
        for (i, v) in out.iter().enumerate().take(27).skip(3) {
            assert!((v - 42.0).abs() <= 1.0e-10, "{interp:?} pixel {i}");
        }
    }
}

#[test]
fn test2_integer_shift_nearest() {
    let input: Vec<f64> = (0..400)
        .map(|k| f64::from(k % 20 + 1 + k / 20 + 1))
        .collect();
    let mut out = vec![0.0; 19 * 19];
    run(
        &ShiftMap::new(vec![1.0, 1.0]),
        &[1, 1],
        &[20, 20],
        &input,
        None,
        Interp::Nearest,
        ResampleFlags::default(),
        BAD_VALUE,
        &[2, 2],
        &[20, 20],
        &mut out,
        None,
    );
    for iy in 0..19 {
        for ix in 0..19 {
            assert_eq!(out[iy * 19 + ix], f64::from(ix as i32 + 1 + iy as i32 + 1));
        }
    }
}

#[test]
fn test3_half_pixel_shift_linear() {
    let input: Vec<f64> = (0..400)
        .map(|k| f64::from(k % 20 + 1 + k / 20 + 1))
        .collect();
    let mut out = vec![0.0; 18 * 18];
    run(
        &ShiftMap::new(vec![0.5, 0.5]),
        &[1, 1],
        &[20, 20],
        &input,
        None,
        Interp::Linear,
        ResampleFlags::default(),
        BAD_VALUE,
        &[2, 2],
        &[19, 19],
        &mut out,
        None,
    );
    for iy in 0..18 {
        for ix in 0..18 {
            let expected = f64::from(ix as i32 + 2 + iy as i32 + 2 - 1);
            assert!((out[iy * 18 + ix] - expected).abs() <= 1.0e-10);
        }
    }
}

#[test]
fn test4_kernels_keep_constant_data() {
    let input = vec![42.0; 400];
    for interp in [
        Interp::Sinc,
        Interp::SincSinc,
        Interp::SincCos,
        Interp::SincGauss,
        Interp::Gauss,
        Interp::Somb,
        Interp::SombCos,
        Interp::BlockAve,
    ] {
        let mut out = vec![0.0; 16 * 16];
        run(
            &ShiftMap::new(vec![0.5, 0.5]),
            &[1, 1],
            &[20, 20],
            &input,
            None,
            interp,
            ResampleFlags::default(),
            BAD_VALUE,
            &[3, 3],
            &[18, 18],
            &mut out,
            None,
        );
        for (i, v) in out.iter().enumerate() {
            assert!((v - 42.0).abs() <= 1.0e-8, "{interp:?} pixel {i}");
        }
    }
}

#[test]
fn test5_integer_data() {
    let input: Vec<i32> = (1..=20).map(|i| i * 100).collect();
    for interp in [Interp::Nearest, Interp::Linear] {
        let mut out = vec![0; 20];
        run(
            &UnitMap::new(1),
            &[1],
            &[20],
            &input,
            None,
            interp,
            ResampleFlags::default(),
            i32::MIN,
            &[1],
            &[20],
            &mut out,
            None,
        );
        assert_eq!(out, input, "{interp:?}");
    }
    let mut out = vec![0; 19];
    run(
        &ShiftMap::new(vec![1.0]),
        &[1],
        &[20],
        &input,
        None,
        Interp::Nearest,
        ResampleFlags::default(),
        i32::MIN,
        &[2],
        &[20],
        &mut out,
        None,
    );
    assert_eq!(out, input[..19]);
}

#[test]
fn test6_float_data() {
    let input: Vec<f32> = (1..=20).map(|i| i as f32).collect();
    for interp in [Interp::Nearest, Interp::Linear] {
        let mut out = vec![0.0_f32; 20];
        run(
            &UnitMap::new(1),
            &[1],
            &[20],
            &input,
            None,
            interp,
            ResampleFlags::default(),
            -f32::MAX,
            &[1],
            &[20],
            &mut out,
            None,
        );
        assert_eq!(out, input, "{interp:?}");
    }
}

#[test]
fn test7_bad_pixels() {
    let mut input: Vec<f64> = (1..=20).map(f64::from).collect();
    input[9] = BAD_VALUE;
    let use_bad = ResampleFlags {
        use_bad: true,
        ..Default::default()
    };
    let mut out = vec![0.0; 20];
    let nbad = run(
        &UnitMap::new(1),
        &[1],
        &[20],
        &input,
        None,
        Interp::Nearest,
        use_bad,
        BAD_VALUE,
        &[1],
        &[20],
        &mut out,
        None,
    );
    assert_eq!(nbad, 1);
    assert_eq!(out, input);

    let mut out = vec![0.0; 18];
    let nbad = run(
        &ShiftMap::new(vec![0.5]),
        &[1],
        &[20],
        &input,
        None,
        Interp::Linear,
        use_bad,
        BAD_VALUE,
        &[2],
        &[19],
        &mut out,
        None,
    );
    assert!(nbad >= 1);
}

#[test]
fn test8_variances() {
    let input: Vec<f64> = (1..=20).map(f64::from).collect();
    let var: Vec<f64> = (1..=20).map(|i| f64::from(i) * 0.1).collect();
    let mut out = vec![0.0; 20];
    let mut out_var = vec![0.0; 20];
    run(
        &UnitMap::new(1),
        &[1],
        &[20],
        &input,
        Some(&var),
        Interp::Nearest,
        ResampleFlags::default(),
        BAD_VALUE,
        &[1],
        &[20],
        &mut out,
        Some(&mut out_var),
    );
    assert_eq!(out, input);
    assert_eq!(out_var, var);
}

#[test]
fn test9_no_bad_leaves_bad_output_alone() {
    let mut input: Vec<f64> = (1..=20).map(f64::from).collect();
    input[9] = BAD_VALUE;
    let mut out = vec![-999.0; 20];
    let nbad = run(
        &UnitMap::new(1),
        &[1],
        &[20],
        &input,
        None,
        Interp::Nearest,
        ResampleFlags {
            use_bad: true,
            no_bad: true,
            ..Default::default()
        },
        BAD_VALUE,
        &[1],
        &[20],
        &mut out,
        None,
    );
    assert_eq!(nbad, 1);
    assert_eq!(out[9], -999.0);
    for i in (0..20).filter(|&i| i != 9) {
        assert_eq!(out[i], input[i]);
    }
}

/// The four user flags reach a caller's interpolator and kernel unchanged,
/// as C passes the whole flags value, AST__URESAMP1-4 included, to the
/// caller's functions, and the predefined schemes ignore them.
#[test]
fn user_flags_reach_the_callers_interpolator_and_kernel() {
    use std::cell::Cell;
    let flags = ResampleFlags {
        user2: true,
        user4: true,
        ..Default::default()
    };
    let seen = Cell::new(None);
    let interp = |a: &starlink_ast::mappings::InterpArgs<'_, f64>,
                  out: &mut [f64],
                  _var: Option<&mut [f64]>| {
        seen.set(Some(a.flags));
        for &off in a.offset {
            out[off as usize] = 1.0;
        }
        Ok(0)
    };
    let data = [1.0, 2.0, 3.0, 4.0];
    let mut out = [0.0; 4];
    run(
        &UnitMap::new(1),
        &[1],
        &[4],
        &data,
        None,
        Interp::UInterp(&interp),
        flags,
        BAD_VALUE,
        &[1],
        &[4],
        &mut out,
        None,
    );
    assert_eq!(seen.get(), Some(flags));

    let kernel_seen = Cell::new(None);
    let kernel = |offset: f64, _params: &[f64], f: ResampleFlags| {
        kernel_seen.set(Some(f));
        Ok(if offset == 0.0 { 1.0 } else { 0.0 })
    };
    let mut out = [0.0; 4];
    run(
        &UnitMap::new(1),
        &[1],
        &[4],
        &data,
        None,
        Interp::UKern1(&kernel),
        flags,
        BAD_VALUE,
        &[1],
        &[4],
        &mut out,
        None,
    );
    assert_eq!(kernel_seen.get(), Some(flags));
    assert_eq!(out, data);

    // A predefined scheme ignores them.
    let mut plain = [0.0; 4];
    let mut flagged = [0.0; 4];
    for (f, out) in [
        (ResampleFlags::default(), &mut plain),
        (flags, &mut flagged),
    ] {
        run(
            &ShiftMap::new(vec![0.3]),
            &[1],
            &[4],
            &data,
            None,
            Interp::Linear,
            f,
            BAD_VALUE,
            &[1],
            &[4],
            out,
            None,
        );
    }
    assert_eq!(plain, flagged);
}
