//! Benchmark: LSST FrameSet forward/inverse transform performance.
//!
//! Loads the lsst.ast fixture, transforms 1, 10_000, and 100_000 points,
//! and prints timing results.

use std::time::Instant;

use ast_core::coords::PointSet;
use ast_core::from_native_string;
use ast_core::mappings::Mapping;

fn load_lsst_frameset() -> Box<dyn Mapping> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/native/reference/lsst.ast"
    ))
    .expect("failed to read lsst.ast");
    let obj = from_native_string(&text).expect("failed to parse lsst.ast");
    obj.into_mapping().expect("lsst.ast is not a Mapping")
}

fn make_points(n: usize) -> PointSet {
    let mut rows = Vec::with_capacity(n);
    for i in 0..n {
        let x = 100.0 + (i as f64) * 0.01;
        let y = 200.0 + (i as f64) * 0.005;
        rows.push(vec![x, y]);
    }
    PointSet::from_rows(rows)
}

fn bench_nan_protocol(mapping: &dyn Mapping, npoint: usize, forward: bool) -> std::time::Duration {
    let pixel_pts = make_points(npoint);
    let label = if forward { "fwd-nan" } else { "inv-nan" };

    let pts = if forward {
        pixel_pts
    } else {
        mapping.transform(&pixel_pts, true).expect("forward warmup")
    };

    let _ = mapping.transform_nan_protocol(&pts, forward);

    let iterations = if npoint >= 100_000 {
        3
    } else if npoint >= 10_000 {
        10
    } else {
        100
    };
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = mapping.transform_nan_protocol(&pts, forward).unwrap();
    }
    let elapsed = start.elapsed();
    let per_call = elapsed / iterations;
    let per_point = if npoint > 0 {
        per_call / npoint as u32
    } else {
        per_call
    };

    println!(
        "  {label:>7} {npoint:>7} pts: {per_call:>10.3?} total, {per_point:>8.1?}/pt  ({iterations} iters)"
    );
    per_call
}

fn bench_transform(mapping: &dyn Mapping, npoint: usize, forward: bool) -> std::time::Duration {
    let pixel_pts = make_points(npoint);
    let label = if forward { "forward" } else { "inverse" };

    // Inverse must be benchmarked on inputs in the *output* domain, not pixel
    // coords. Producing those by running forward on the same points keeps the
    // bench aligned with how the inverse is used in practice (round-tripping
    // sky positions) and matches what bench_lsst_c.c does. Using pixel coords
    // for the inverse forces NR to iterate on out-of-domain inputs and was
    // hiding the actual transform performance by an order of magnitude.
    let pts = if forward {
        pixel_pts
    } else {
        mapping.transform(&pixel_pts, true).expect("forward warmup")
    };

    // Warm up
    let _ = mapping.transform(&pts, forward);

    // Time 10 iterations (or fewer for large N)
    let iterations = if npoint >= 100_000 {
        3
    } else if npoint >= 10_000 {
        10
    } else {
        100
    };
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = mapping.transform(&pts, forward).unwrap();
    }
    let elapsed = start.elapsed();
    let per_call = elapsed / iterations;
    let per_point = if npoint > 0 {
        per_call / npoint as u32
    } else {
        per_call
    };

    println!(
        "  {label:>7} {npoint:>7} pts: {per_call:>10.3?} total, {per_point:>8.1?}/pt  ({iterations} iters)"
    );
    per_call
}

#[test]
fn benchmark_lsst_frameset() {
    println!("\n=== LSST FrameSet Transform Benchmark ===\n");

    let mapping = load_lsst_frameset();
    println!("Loaded: nin={}, nout={}\n", mapping.nin(), mapping.nout());

    println!("--- Original mapping ---");
    for &n in &[1, 10_000, 100_000] {
        println!("N = {n}:");
        bench_transform(mapping.as_ref(), n, true);
        bench_transform(mapping.as_ref(), n, false);
        println!();
    }

    println!("--- Simplified mapping ---");
    let simplified = mapping.simplified().unwrap();
    println!(
        "Simplified: nin={}, nout={}\n",
        simplified.nin(),
        simplified.nout()
    );

    // Verify equivalence
    let pts = make_points(100);
    let out_orig = mapping.transform(&pts, true).unwrap();
    let out_simp = simplified.transform(&pts, true).unwrap();
    for i in 0..100 {
        for j in 0..2 {
            let diff = (out_orig.point(i)[j] - out_simp.point(i)[j]).abs();
            assert!(diff < 1e-8, "point {i} axis {j}: diff={diff}");
        }
    }
    println!("Transform equivalence verified (100 points, < 1e-8 rad)\n");

    for &n in &[1, 10_000, 100_000] {
        println!("N = {n}:");
        bench_transform(simplified.as_ref(), n, true);
        bench_transform(simplified.as_ref(), n, false);
        bench_nan_protocol(simplified.as_ref(), n, true);
        bench_nan_protocol(simplified.as_ref(), n, false);
        println!();
    }
}
