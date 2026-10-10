//! `mappings::rebin_seq` on known data.
//!
//! C ancestor: `reference/starlink-ast/ast_tester/testrebinseq.c`, tests 1 to
//! 12, each run in 1, 2 and 3 dimensions with the `GAUSS`, `NEAREST` and
//! `LINEAR` spreading schemes, and test 12 run twice as C runs it.
//!
//! Divergences from C:
//!
//! - C runs every test for one scheme and dimensionality before moving to
//!   the next, and stops at the first failure. Here each C test function is
//!   one Rust test that runs every scheme and dimensionality in C's order,
//!   so a failure in one test does not hide the others.
//! - C sizes its arrays for the largest grid and passes the whole buffer
//!   whatever the dimensionality; `rebin_seq` wants arrays exactly the size
//!   of their grids, so the leading elements C's grid covers are passed.
//!   The grids are in vector order, first axis fastest, so these are the
//!   elements C uses.
//! - Where C passes its output data array as the output variance array too,
//!   with flags that need no output variances (C then ignores it), no
//!   output variance array is passed, since Rust cannot alias the two.
//! - C's `addnoise` writes the Gauss standard deviation into the MathMap
//!   expression with `%.17g`; here it is written with Rust's shortest
//!   round-trip formatting, which parses to the same double.

use ast_core::constants::BAD_VALUE;
use ast_core::mappings::{
    CmpMap, Composition, Mapping, MathMap, RebinInput, RebinOutput, RebinSeqFlags, RebinSeqOptions,
    ShiftMap, Spread, UnitMap, WinMap, ZoomMap, rebin_seq,
};
use ast_core::pixel::RebinValue;

const NX: usize = 100;
const NY: usize = 200;
const NX_IN: usize = 100;
const NY_IN: usize = 200;
const BORDER_OUT: i64 = 10;
const NX_OUT: usize = NX_IN + 2 * BORDER_OUT as usize;
const NY_OUT: usize = NY_IN + 2 * BORDER_OUT as usize;
const SIZE: usize = 20000;
const BUFSIZE: usize = 100;

/// The spreading parameters C passes to every call.
const PARAMS: [f64; 2] = [1.5, 1.0];

/// C's `(float) AST__BAD`.
const BAD_F32: f32 = BAD_VALUE as f32;

/// The dimensionalities and spreading schemes C's `main` runs, in order.
const CASES: [(usize, Spread); 9] = [
    (1, Spread::Gauss),
    (2, Spread::Gauss),
    (3, Spread::Gauss),
    (1, Spread::Nearest),
    (2, Spread::Nearest),
    (3, Spread::Nearest),
    (1, Spread::Linear),
    (2, Spread::Linear),
    (3, Spread::Linear),
];

/// The index into a vector-order array of 1-based pixel `(i, j)` in a grid
/// `nx` pixels wide (C's `idx2`).
fn idx2(nx: usize, i: usize, j: usize) -> usize {
    (j - 1) * nx + (i - 1)
}

/// Input and output grid bounds, for up to three axes.
struct Grid {
    lbnd_in: [i64; 3],
    ubnd_in: [i64; 3],
    lbnd_out: [i64; 3],
    ubnd_out: [i64; 3],
}

/// C's `set_bounds`.
fn set_bounds(nx_in: usize, ny_in: usize, nx_out: usize, ny_out: usize) -> Grid {
    Grid {
        lbnd_in: [0, 1, 1],
        ubnd_in: [nx_in as i64 - 1, ny_in as i64, 1],
        lbnd_out: [0, 1, 1],
        ubnd_out: [nx_out as i64 - 1, ny_out as i64, 1],
    }
}

/// The number of pixels in the first `ndim` axes of a grid.
fn npix(lbnd: &[i64], ubnd: &[i64], ndim: usize) -> usize {
    lbnd[..ndim]
        .iter()
        .zip(&ubnd[..ndim])
        .map(|(l, u)| (u - l + 1) as usize)
        .product()
}

/// C's name for a spreading scheme.
fn spread_name(spread: Spread) -> &'static str {
    match spread {
        Spread::Gauss => "AST__GAUSS",
        Spread::Nearest => "AST__NEAREST",
        Spread::Linear => "AST__LINEAR",
        _ => "UNKNOWN",
    }
}

/// One call to `rebin_seq` with C's arguments: the whole input grid as the
/// region, `PARAMS`, a tolerance of 0.01 and a `wlim` of zero. The arrays
/// are cut to the sizes of their grids. An error fails the test, as C's
/// `astWatch` status does.
#[allow(clippy::too_many_arguments)]
fn call<T: RebinValue>(
    map: &dyn Mapping,
    ndim: usize,
    g: &Grid,
    inp: &[T],
    in_var: Option<&[T]>,
    spread: Spread,
    flags: RebinSeqFlags,
    maxpix: usize,
    badval: T,
    out: &mut [T],
    out_var: Option<&mut [T]>,
    weights: &mut [f64],
    nused: &mut i64,
) {
    let n_in = npix(&g.lbnd_in, &g.ubnd_in, ndim);
    let n_out = npix(&g.lbnd_out, &g.ubnd_out, ndim);
    let n_wgt = if flags.gen_var { 2 * n_out } else { n_out };
    rebin_seq(
        map,
        Some(&RebinInput {
            lbnd: &g.lbnd_in[..ndim],
            ubnd: &g.ubnd_in[..ndim],
            region_lbnd: &g.lbnd_in[..ndim],
            region_ubnd: &g.ubnd_in[..ndim],
            data: &inp[..n_in],
            var: in_var.map(|v| &v[..n_in]),
        }),
        &mut RebinOutput {
            lbnd: &g.lbnd_out[..ndim],
            ubnd: &g.ubnd_out[..ndim],
            data: &mut out[..n_out],
            var: out_var.map(|v| &mut v[..n_out]),
        },
        Some(&mut weights[..n_wgt]),
        Some(nused),
        &RebinSeqOptions {
            spread,
            params: &PARAMS,
            flags,
            tol: 0.01,
            maxpix,
            wlim: 0.0,
        },
        badval,
    )
    .unwrap_or_else(|e| {
        panic!(
            "rebin_seq failed: {e:?}\nSpread={} ({ndim}-dimensional)",
            spread_name(spread)
        )
    });
}

/// Run one C test function for every case of C's `main`, failing with C's
/// messages at the first case that fails.
fn tests(name: &str, test: fn(usize, Spread) -> Result<(), String>) {
    for (ndim, spread) in CASES {
        if let Err(msg) = test(ndim, spread) {
            panic!(
                "{msg}\n{name} failed\nSpread={} ({ndim}-dimensional)",
                spread_name(spread)
            );
        }
    }
}

/// C's `addnoise`: add Gaussian noise of standard deviation `sigma` to
/// `array`, drawn in blocks of `BUFSIZE` from a MathMap with `Seed=1`.
fn addnoise(array: &mut [f64], sigma: f64) {
    let fwd = format!("Y=Gauss(0.0,{sigma})");
    let mut mm = MathMap::new(1, 1, &[fwd.as_str()], &["X"]).expect("MathMap for noise");
    mm.set_seed(1);

    let junk = [0.0; BUFSIZE];
    let mut noise = Vec::new();
    let mut nused = BUFSIZE;
    for v in array.iter_mut() {
        if nused == BUFSIZE {
            noise = mm.tran1(&junk, true).expect("noise from MathMap");
            nused = 0;
        }
        *v += noise[nused];
        nused += 1;
    }
}

/// A UnitMap on `f32` data in one call: every output pixel is 1.
fn test1(ndim: usize, spread: Spread) -> Result<(), String> {
    let inp = vec![1.0f32; NX * NY];
    let mut out = vec![0.0f32; NX * NY];
    let mut weights = vec![0.0; NX * NY];
    let mut nused = 0;

    let g = set_bounds(NX, NY, NX, NY);
    let flags = RebinSeqFlags {
        rebin_init: true,
        rebin_end: true,
        ..Default::default()
    };

    let map = UnitMap::new(ndim);
    call(
        &map,
        ndim,
        &g,
        &inp,
        Some(&inp),
        spread,
        flags,
        50,
        BAD_F32,
        &mut out,
        None,
        &mut weights,
        &mut nused,
    );

    let jhi = if ndim == 1 { 1 } else { NY };
    for i in 1..=NX {
        for j in 1..=jhi {
            let v = out[idx2(NX, i, j)];
            if (v - 1.0).abs() > 1.0e-6 {
                return Err(format!("Output pixel ({i},{j}) should be 1.0 but is {v}"));
            }
        }
    }
    Ok(())
}

/// A UnitMap on `f64` data in three calls, the first two both starting the
/// sequence: every output pixel is 1.
fn test2(ndim: usize, spread: Spread) -> Result<(), String> {
    let inp = vec![1.0f64; NX * NY];
    let mut out = vec![0.0f64; NX * NY];
    let mut weights = vec![0.0; NX * NY];
    let mut nused = 0;

    let g = set_bounds(NX, NY, NX, NY);
    let mut flags = RebinSeqFlags {
        rebin_init: true,
        ..Default::default()
    };

    let map = UnitMap::new(ndim);
    for i in 1..=3 {
        if i == 3 {
            flags = RebinSeqFlags {
                rebin_end: true,
                ..Default::default()
            };
        }
        call(
            &map,
            ndim,
            &g,
            &inp,
            Some(&inp),
            spread,
            flags,
            50,
            BAD_VALUE,
            &mut out,
            None,
            &mut weights,
            &mut nused,
        );
    }

    let jhi = if ndim == 1 { 1 } else { NY };
    for i in 1..=NX {
        for j in 1..=jhi {
            let v = out[idx2(NX, i, j)];
            if (v - 1.0).abs() > 1.0e-6 {
                return Err(format!("Output pixel ({i},{j}) should be 1.0 but is {v}"));
            }
        }
    }
    Ok(())
}

/// A UnitMap on `i32` data in three unnormalised calls: every output pixel
/// is 3.
fn test3(ndim: usize, spread: Spread) -> Result<(), String> {
    let inp = vec![1i32; NX * NY];
    let mut out = vec![0i32; NX * NY];
    let mut weights = vec![0.0; NX * NY];
    let mut nused = 0;

    let g = set_bounds(NX, NY, NX, NY);
    let mut flags = RebinSeqFlags {
        rebin_init: true,
        no_norm: true,
        ..Default::default()
    };

    let map = UnitMap::new(ndim);
    for _ in 1..=3 {
        call(
            &map,
            ndim,
            &g,
            &inp,
            Some(&inp),
            spread,
            flags,
            50,
            i32::MIN,
            &mut out,
            None,
            &mut weights,
            &mut nused,
        );
        flags = RebinSeqFlags {
            no_norm: true,
            ..Default::default()
        };
    }

    let jhi = if ndim == 1 { 1 } else { NY };
    for i in 1..=NX {
        for j in 1..=jhi {
            let v = out[idx2(NX, i, j)];
            if v != 3 {
                return Err(format!("Output pixel ({i},{j}) should be 3 but is {v}"));
            }
        }
    }
    Ok(())
}

/// A WinMap that halves the grid, conserving flux: the bounding box of the
/// good output pixels and the total flux.
fn test4(ndim: usize, spread: Spread) -> Result<(), String> {
    let inp = vec![1.0f32; NX * NY];
    let mut out = vec![0.0f32; NX * NY];
    let mut weights = vec![0.0; NX * NY];
    let mut nused = 0;

    let g = set_bounds(NX, NY, NX, NY);
    let flags = RebinSeqFlags {
        rebin_init: true,
        rebin_end: true,
        conserve_flux: true,
        ..Default::default()
    };

    let ina = [
        g.lbnd_in[0] as f64,
        g.lbnd_in[1] as f64,
        g.lbnd_in[2] as f64,
    ];
    let inb = [
        g.ubnd_in[0] as f64,
        g.ubnd_in[1] as f64,
        g.ubnd_in[2] as f64 + 1.0,
    ];
    let outa = [
        0.75 * g.lbnd_out[0] as f64 + 0.25 * g.ubnd_out[0] as f64,
        0.75 * g.lbnd_out[1] as f64 + 0.25 * g.ubnd_out[1] as f64,
        ina[2],
    ];
    let outb = [
        0.25 * g.lbnd_out[0] as f64 + 0.75 * g.ubnd_out[0] as f64,
        0.25 * g.lbnd_out[1] as f64 + 0.75 * g.ubnd_out[1] as f64,
        inb[2],
    ];

    let map = WinMap::from_corners(&ina[..ndim], &inb[..ndim], &outa[..ndim], &outb[..ndim])
        .expect("WinMap");
    call(
        &map,
        ndim,
        &g,
        &inp,
        Some(&inp),
        spread,
        flags,
        1000,
        BAD_F32,
        &mut out,
        None,
        &mut weights,
        &mut nused,
    );

    // C's `INT_MAX` and `INT_MIN` starting values, with its `else if`.
    let mut lboxg = [usize::MAX; 2];
    let mut uboxg = [usize::MIN; 2];
    let mut sum = 0.0;

    let jhi = if ndim == 1 { 1 } else { NY };
    for i in 1..=NX {
        for j in 1..=jhi {
            let v = out[idx2(NX, i, j)];
            if v != BAD_F32 {
                sum += f64::from(v);
                if i < lboxg[0] {
                    lboxg[0] = i;
                } else if i > uboxg[0] {
                    uboxg[0] = i;
                }
                if j < lboxg[1] {
                    lboxg[1] = j;
                } else if j > uboxg[1] {
                    uboxg[1] = j;
                }
            }
        }
    }

    let answer = if ndim == 1 {
        let want = match spread {
            Spread::Gauss => (24, 77),
            Spread::Nearest => (26, 75),
            Spread::Linear => (25, 76),
            _ => unreachable!(),
        };
        if (lboxg[0], uboxg[0]) != want {
            return Err(format!(
                "{} {}\nGood pixel bounding box is wrong",
                lboxg[0], uboxg[0]
            ));
        }
        match spread {
            Spread::Nearest => 100.0,
            Spread::Gauss => 108.0,
            Spread::Linear => 104.0,
            _ => -1.0,
        }
    } else {
        let want = match spread {
            Spread::Gauss => [24, 49, 77, 152],
            Spread::Nearest => [26, 51, 75, 150],
            Spread::Linear => [25, 50, 76, 151],
            _ => unreachable!(),
        };
        if [lboxg[0], lboxg[1], uboxg[0], uboxg[1]] != want {
            return Err(format!(
                "{} {} {} {}\nGood pixel bounding box is wrong",
                lboxg[0], lboxg[1], uboxg[0], uboxg[1]
            ));
        }
        match spread {
            Spread::Nearest => 20000.0,
            Spread::Gauss => 22464.0,
            Spread::Linear => 21216.0,
            _ => -1.0,
        }
    };

    if (sum - answer).abs() > 0.01 {
        return Err(format!(
            "Total output data sum is {sum} (should be {answer})."
        ));
    }
    Ok(())
}

/// Grids for tests 5 and 6: the output grid is the input grid with a
/// border of `BORDER_OUT` pixels on the first two axes.
fn bordered_grid() -> Grid {
    let lbnd_in = [0, 1, 1];
    let ubnd_in = [NX_IN as i64 - 1, NY_IN as i64, 1];
    Grid {
        lbnd_in,
        ubnd_in,
        lbnd_out: [lbnd_in[0] - BORDER_OUT, lbnd_in[1] - BORDER_OUT, 1],
        ubnd_out: [ubnd_in[0] + BORDER_OUT, ubnd_in[1] + BORDER_OUT, 1],
    }
}

/// The WinMap of tests 5 and 6 for pass `k`: it takes the input grid to
/// half its size, starting a quarter of the way further along each axis
/// with each pass.
fn shrink_map(g: &Grid, ndim: usize, k: usize) -> WinMap {
    let ina = [
        g.lbnd_in[0] as f64,
        g.lbnd_in[1] as f64,
        g.lbnd_in[2] as f64,
    ];
    let inb = [
        g.ubnd_in[0] as f64,
        g.ubnd_in[1] as f64,
        g.ubnd_in[2] as f64 + 1.0,
    ];
    let va = (k - 1) as f64 * 0.25;
    let vb = va + 0.5;
    let outa = [
        vb * g.lbnd_in[0] as f64 + va * g.ubnd_in[0] as f64,
        vb * g.lbnd_in[1] as f64 + va * g.ubnd_in[1] as f64,
        ina[2],
    ];
    let outb = [
        va * g.lbnd_in[0] as f64 + vb * g.ubnd_in[0] as f64,
        va * g.lbnd_in[1] as f64 + vb * g.ubnd_in[1] as f64,
        inb[2],
    ];
    WinMap::from_corners(&ina[..ndim], &inb[..ndim], &outa[..ndim], &outb[..ndim]).expect("WinMap")
}

/// Three overlapping unnormalised passes of constant data through shifted
/// WinMaps: the total and the values where they overlap. C checks nothing
/// in 3 dimensions.
fn test5(ndim: usize, spread: Spread) -> Result<(), String> {
    let mut inp = vec![0.0f64; NX_IN * NY_IN];
    let mut out = vec![0.0f64; NX_OUT * NY_OUT];
    let mut weights = vec![0.0; NX_OUT * NY_OUT];
    let mut nused = 0;

    let g = bordered_grid();
    let mut flags = RebinSeqFlags {
        rebin_init: true,
        no_norm: true,
        ..Default::default()
    };

    for k in 1..=3 {
        inp.fill(k as f64);
        let map = shrink_map(&g, ndim, k);
        call(
            &map,
            ndim,
            &g,
            &inp,
            Some(&inp),
            spread,
            flags,
            50,
            BAD_VALUE,
            &mut out,
            None,
            &mut weights,
            &mut nused,
        );
        flags = RebinSeqFlags {
            no_norm: true,
            ..Default::default()
        };
    }

    let jhi = if ndim == 1 { 1 } else { NY_OUT };
    let mut sum = 0.0;
    for i in 1..=NX_OUT {
        for j in 1..=jhi {
            let v = out[idx2(NX_OUT, i, j)];
            if v != BAD_VALUE {
                sum += v;
            }
        }
    }

    let pixel = |i, j, want: f64| (out[idx2(NX_OUT, i, j)] - want).abs() > 1.0e-6;
    if ndim == 1 {
        if (sum - 600.0).abs() > 1.0e-3 {
            return Err(format!("Total output data sum is {sum} (should be 600)."));
        } else if pixel(20, 1, 2.0) {
            return Err("Output pixel (20) should be 2.".into());
        } else if pixel(50, 1, 6.0) {
            return Err("Output pixel (50) should be 6.".into());
        } else if pixel(70, 1, 10.0) {
            return Err("Output pixel (70) should be 10.".into());
        } else if pixel(100, 1, 6.0) {
            return Err("Output pixel (100) should be 6.".into());
        }
    } else if ndim == 2 {
        if (sum - 120000.0).abs() > 1.0e-3 {
            return Err(format!(
                "Total output data sum is {sum} (should be 120000)."
            ));
        } else if pixel(40, 40, 4.0) {
            return Err("Output pixel (40,40) should be 4.".into());
        } else if pixel(50, 90, 12.0) {
            return Err("Output pixel (50,90) should be 12.".into());
        } else if pixel(70, 80, 8.0) {
            return Err("Output pixel (70,80) should be 8.".into());
        } else if pixel(70, 130, 20.0) {
            return Err("Output pixel (70,130) should be 20.".into());
        } else if pixel(20, 130, 0.0) {
            return Err("Output pixel (20,130) should be 0.".into());
        }
    }
    Ok(())
}

/// The passes of test 5, normalised and conserving flux: the total and the
/// largest and smallest values. C checks nothing in 3 dimensions.
fn test6(ndim: usize, spread: Spread) -> Result<(), String> {
    const NK: usize = 3;
    let mut inp = vec![0.0f64; NX_IN * NY_IN];
    let mut out = vec![0.0f64; NX_OUT * NY_OUT];
    let mut weights = vec![0.0; NX_OUT * NY_OUT];
    let mut nused = 0;

    let g = bordered_grid();

    for k in 1..=NK {
        let flags = RebinSeqFlags {
            conserve_flux: true,
            rebin_init: k == 1,
            rebin_end: k == NK,
            ..Default::default()
        };
        inp.fill(k as f64);
        let map = shrink_map(&g, ndim, k);
        call(
            &map,
            ndim,
            &g,
            &inp,
            Some(&inp),
            spread,
            flags,
            50,
            BAD_VALUE,
            &mut out,
            None,
            &mut weights,
            &mut nused,
        );
    }

    let jhi = if ndim == 1 { 1 } else { NY_OUT };
    let mut sum = 0.0;
    let mut mxval = -1.0e300;
    let mut mnval = 1.0e300;
    for i in 1..=NX_OUT {
        for j in 1..=jhi {
            let v = out[idx2(NX_OUT, i, j)];
            if v != BAD_VALUE {
                sum += v;
                if v > mxval {
                    mxval = v;
                }
                if v < mnval {
                    mnval = v;
                }
            }
        }
    }

    let (answer, max, min) = match ndim {
        1 => (
            match spread {
                Spread::Gauss => 414.0,
                Spread::Nearest => 399.4,
                Spread::Linear => 400.0,
                _ => -1.0,
            },
            6.0,
            2.0,
        ),
        2 => (
            match spread {
                Spread::Gauss => 109011.729592723,
                Spread::Nearest => 100716.666666667,
                Spread::Linear => 102816.0,
                _ => -1.0,
            },
            12.0,
            4.0,
        ),
        _ => return Ok(()),
    };
    if (sum - answer).abs() > 1.0e-3 {
        return Err(format!(
            "Total output data sum is {sum} (should be {answer})."
        ));
    } else if (mxval - max).abs() > 1.0e-6 {
        return Err(format!("Max value should be {max}."));
    } else if (mnval - min).abs() > 1.0e-6 {
        return Err(format!("Min value should be {min}."));
    }
    Ok(())
}

/// A ZoomMap that halves the grid: every good output pixel is 1.
fn test7(ndim: usize, spread: Spread) -> Result<(), String> {
    let inp = vec![1.0f32; NX * NY];
    let mut out = vec![0.0f32; NX * NY];
    let mut weights = vec![0.0; NX * NY];
    let mut nused = 0;

    let g = set_bounds(NX, NY, NX, NY);
    let flags = RebinSeqFlags {
        rebin_init: true,
        rebin_end: true,
        ..Default::default()
    };

    let map = ZoomMap::new(ndim, 0.5).expect("ZoomMap");
    call(
        &map,
        ndim,
        &g,
        &inp,
        Some(&inp),
        spread,
        flags,
        50,
        BAD_F32,
        &mut out,
        None,
        &mut weights,
        &mut nused,
    );

    let jhi = if ndim == 1 { 1 } else { NY };
    for i in 1..=NX {
        for j in 1..=jhi {
            let v = out[idx2(NX, i, j)];
            if v != BAD_F32 && (v - 1.0).abs() > 1.0e-6 {
                return Err(format!("Output pixel ({i},{j}) should be 1.0 but is {v}"));
            }
        }
    }
    Ok(())
}

/// A ZoomMap then a ShiftMap, unnormalised: the total is the number of
/// input pixels.
fn test8(ndim: usize, spread: Spread) -> Result<(), String> {
    let inp = vec![1.0f32; NX * NY];
    let mut out = vec![0.0f32; NX * NY];
    let mut weights = vec![0.0; NX * NY];
    let mut nused = 0;
    let shifts = [5.0, 5.0, 0.0];

    let g = set_bounds(NX, NY, NX, NY);
    let flags = RebinSeqFlags {
        rebin_init: true,
        rebin_end: true,
        no_norm: true,
        ..Default::default()
    };

    let map = if ndim < 3 {
        CmpMap::new(
            Box::new(ZoomMap::new(ndim, 0.5).expect("ZoomMap")),
            Box::new(ShiftMap::new(&shifts[..ndim])),
            Composition::Series,
        )
        .expect("CmpMap")
    } else {
        CmpMap::new(
            Box::new(
                CmpMap::new(
                    Box::new(ZoomMap::new(2, 0.5).expect("ZoomMap")),
                    Box::new(ShiftMap::new(&shifts[..2])),
                    Composition::Series,
                )
                .expect("CmpMap"),
            ),
            Box::new(UnitMap::new(1)),
            Composition::Parallel,
        )
        .expect("CmpMap")
    };

    call(
        &map,
        ndim,
        &g,
        &inp,
        Some(&inp),
        spread,
        flags,
        50,
        BAD_F32,
        &mut out,
        None,
        &mut weights,
        &mut nused,
    );

    let jhi = if ndim == 1 { 1 } else { NY };
    let mut sum = 0.0;
    for i in 1..=NX {
        for j in 1..=jhi {
            let v = out[idx2(NX, i, j)];
            if v != BAD_F32 {
                sum += f64::from(v);
            }
        }
    }

    let want = (NX * jhi) as f64;
    if sum.is_nan() {
        return Err("Total output data sum is NaN".into());
    } else if (sum - want).abs() > sum * 1.0e-7 {
        return Err(format!(
            "Total output data sum is {sum} (should be {want})."
        ));
    }
    Ok(())
}

/// The noisy data, flags and checks shared by tests 9 to 12: unit data with
/// noise of standard deviation 0.1 and matching input variances, through a
/// ZoomMap that halves the grid. The variance of the output values within
/// the region the input covers must agree with their mean output variance
/// to 5%.
fn noise_test(ndim: usize, spread: Spread, flags: RebinSeqFlags) -> Result<(), String> {
    const SIGMA: f64 = 0.1;
    let mut inp = vec![1.0f64; SIZE];
    let vin = vec![SIGMA * SIGMA; SIZE];
    let mut out = vec![0.0f64; SIZE];
    let mut vout = vec![0.0f64; SIZE];
    let mut weights = vec![0.0; if flags.gen_var { 2 * SIZE } else { SIZE }];
    let mut nused = 0;

    addnoise(&mut inp, SIGMA);

    let (nx, ny) = if ndim == 1 {
        (SIZE, 1)
    } else {
        (SIZE / 200, 200)
    };

    let g = set_bounds(nx, ny, nx, ny);
    let map = ZoomMap::new(ndim, 0.5).expect("ZoomMap");
    call(
        &map,
        ndim,
        &g,
        &inp,
        Some(&vin),
        spread,
        flags,
        50,
        BAD_VALUE,
        &mut out,
        Some(&mut vout),
        &mut weights,
        &mut nused,
    );

    let (ilo, ihi, jlo, jhi) = if ndim == 1 {
        (6, (0.45 * SIZE as f64).round() as usize, 1, 1)
    } else {
        (6, 41, 8, 91)
    };

    let mut sum = 0.0;
    let mut sum2 = 0.0;
    let mut sum3 = 0.0;
    let mut nval = 0i32;
    for i in ilo..=ihi {
        for j in jlo..=jhi {
            let k = idx2(nx, i, j);
            if out[k] != BAD_VALUE {
                sum += out[k];
                sum2 += out[k] * out[k];
                sum3 += vout[k];
                nval += 1;
            }
        }
    }

    let nval = f64::from(nval);
    sum /= nval;
    let realvar = sum2 / nval - sum * sum;
    let meanvar = sum3 / nval;
    if (realvar - meanvar).abs() > 0.05 * (realvar + meanvar) {
        return Err(format!(
            "Real variance is {realvar} - estimate is {meanvar}."
        ));
    }
    Ok(())
}

/// Output variances from the input variances, unnormalised.
fn test9(ndim: usize, spread: Spread) -> Result<(), String> {
    noise_test(
        ndim,
        spread,
        RebinSeqFlags {
            rebin_init: true,
            rebin_end: true,
            no_norm: true,
            use_var: true,
            ..Default::default()
        },
    )
}

/// Output variances from the input variances, conserving flux.
fn test10(ndim: usize, spread: Spread) -> Result<(), String> {
    noise_test(
        ndim,
        spread,
        RebinSeqFlags {
            rebin_init: true,
            rebin_end: true,
            conserve_flux: true,
            use_var: true,
            ..Default::default()
        },
    )
}

/// Output variances from the spread of the input values.
fn test11(ndim: usize, spread: Spread) -> Result<(), String> {
    noise_test(
        ndim,
        spread,
        RebinSeqFlags {
            rebin_init: true,
            rebin_end: true,
            gen_var: true,
            ..Default::default()
        },
    )
}

/// Output variances from the spread of the input values, conserving flux
/// and weighting by the input variances.
fn test12(ndim: usize, spread: Spread) -> Result<(), String> {
    noise_test(
        ndim,
        spread,
        RebinSeqFlags {
            rebin_init: true,
            rebin_end: true,
            gen_var: true,
            conserve_flux: true,
            var_wgt: true,
            ..Default::default()
        },
    )
}

#[test]
fn test1_unitmap_f32() {
    tests("test1", test1);
}

#[test]
fn test2_unitmap_f64_three_calls() {
    tests("test2", test2);
}

#[test]
fn test3_unitmap_i32_unnormalised() {
    tests("test3", test3);
}

#[test]
fn test4_winmap_conserve_flux() {
    tests("test4", test4);
}

#[test]
fn test5_winmap_passes_unnormalised() {
    tests("test5", test5);
}

#[test]
fn test6_winmap_passes_conserve_flux() {
    tests("test6", test6);
}

#[test]
fn test7_zoommap() {
    tests("test7", test7);
}

#[test]
fn test8_zoom_shift_unnormalised() {
    tests("test8", test8);
}

#[test]
fn test9_variance_unnormalised() {
    tests("test9", test9);
}

#[test]
fn test10_variance_conserve_flux() {
    tests("test10", test10);
}

#[test]
fn test11_generated_variance() {
    tests("test11", test11);
}

/// C's `tests` runs test 12 twice.
#[test]
fn test12_generated_variance_weighted() {
    tests("test12", test12);
    tests("test12", test12);
}
