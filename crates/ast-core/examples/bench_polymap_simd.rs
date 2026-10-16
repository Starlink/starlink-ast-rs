//! Sweep PolyMap forward-eval performance across point counts, comparing
//! the scalar path against the cross-lane SIMD prototype.
//!
//! Run:
//!   cargo run --example bench_polymap_simd --release --features simd-eval
//!
//! Strategy:
//!   - Extract the standalone PolyMap from the simplified LSST FrameSet so we
//!     measure only the polynomial kernel, not the surrounding CmpMap chain.
//!   - For each N in [16 .. 1M], time scalar forward (`map.transform`) and
//!     SIMD forward (`map.forward_simd_2d_into`) over enough iterations to
//!     yield ~50 ms of measurement per data point.
//!   - Print ns/pt and the SIMD/scalar ratio so the SIMD_MIN_POINTS threshold
//!     can be tuned for the host hardware.

use std::time::Instant;

use ast_core::coords::PointSet;
use ast_core::from_native_string;
use ast_core::mappings::polymap_simd::{SimdPath, find_polymap};
use ast_core::mappings::{Mapping, PolyMap};

fn make_points(n: usize) -> PointSet {
    let rows: Vec<Vec<f64>> = (0..n)
        .map(|i| vec![100.0 + i as f64 * 0.01, 200.0 + i as f64 * 0.005])
        .collect();
    PointSet::from_rows(rows)
}

fn iterations_for(npoint: usize) -> usize {
    // ~50 ms target at the scalar's ~100 ns/pt → 500k pt-iters.
    (500_000 / npoint.max(1)).max(3)
}

fn time_scalar(map: &PolyMap, pts: &PointSet, iters: usize) -> f64 {
    // Warm up
    let _ = map.transform(pts, true).unwrap();
    let start = Instant::now();
    let mut acc = 0.0;
    for _ in 0..iters {
        let out = map.transform(pts, true).unwrap();
        acc += out.point(0)[0]; // prevent dead-code elimination
    }
    let elapsed = start.elapsed().as_secs_f64();
    std::hint::black_box(acc);
    elapsed / iters as f64
}

fn time_simd(map: &PolyMap, pts: &PointSet, iters: usize) -> (f64, bool) {
    let mut out = PointSet::zeros(pts.npoint(), 2);
    // Warm up + confirm SIMD path actually took.
    let path = map
        .forward_simd_2d_into(&pts.as_points(), &mut out.as_points_mut())
        .unwrap();
    let took = path == SimdPath::Took;
    let start = Instant::now();
    let mut acc = 0.0;
    for _ in 0..iters {
        let _ = map
            .forward_simd_2d_into(&pts.as_points(), &mut out.as_points_mut())
            .unwrap();
        acc += out.point(0)[0];
    }
    let elapsed = start.elapsed().as_secs_f64();
    std::hint::black_box(acc);
    (elapsed / iters as f64, took)
}

fn time_scalar_inverse(map: &PolyMap, pts: &PointSet, iters: usize) -> f64 {
    let _ = map.transform(pts, false).unwrap();
    let start = Instant::now();
    let mut acc = 0.0;
    for _ in 0..iters {
        let out = map.transform(pts, false).unwrap();
        acc += out.point(0)[0];
    }
    let elapsed = start.elapsed().as_secs_f64();
    std::hint::black_box(acc);
    elapsed / iters as f64
}

fn time_simd_inverse(map: &PolyMap, pts: &PointSet, iters: usize) -> (f64, bool) {
    let mut out = PointSet::zeros(pts.npoint(), 2);
    let path = map
        .inverse_simd_2d_into(&pts.as_points(), &mut out.as_points_mut())
        .unwrap();
    let took = path == SimdPath::Took;
    let start = Instant::now();
    let mut acc = 0.0;
    for _ in 0..iters {
        let _ = map
            .inverse_simd_2d_into(&pts.as_points(), &mut out.as_points_mut())
            .unwrap();
        acc += out.point(0)[0];
    }
    let elapsed = start.elapsed().as_secs_f64();
    std::hint::black_box(acc);
    (elapsed / iters as f64, took)
}

fn main() {
    // Load the LSST FrameSet, simplify, and extract the inner PolyMap so
    // we measure only the polynomial kernel.
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/lsst.ast"
    ))
    .expect("lsst.ast");
    let fset = from_native_string(&text)
        .expect("parse")
        .into_mapping()
        .expect("not a Mapping");
    let simplified = fset.simplify().expect("simplify");
    let map = find_polymap(simplified.as_ref())
        .expect("no PolyMap in simplified LSST FrameSet")
        .clone();

    println!("Extracted PolyMap: nin={}, nout={}", map.nin(), map.nout());
    println!(
        "  forward terms: {}, max powers: {}, {}",
        map.forward_terms().len(),
        map.forward_terms()
            .iter()
            .map(|t| t.powers[0])
            .max()
            .unwrap_or(0),
        map.forward_terms()
            .iter()
            .map(|t| t.powers[1])
            .max()
            .unwrap_or(0),
    );
    println!();
    println!("=== Forward eval ===");
    println!(
        "{:>10}  {:>10}  {:>10}  {:>8}  {:>6}",
        "N", "scalar ns/pt", "simd ns/pt", "speedup", "took?"
    );

    let sizes: &[usize] = &[
        16, 64, 128, 256, 1024, 4096, 16_384, 65_536, 262_144, 1_048_576,
    ];
    for &n in sizes {
        let pts = make_points(n);
        let iters = iterations_for(n);

        let scalar_per_call = time_scalar(&map, &pts, iters);
        let (simd_per_call, took) = time_simd(&map, &pts, iters);

        let scalar_ns_pt = scalar_per_call * 1e9 / n as f64;
        let simd_ns_pt = simd_per_call * 1e9 / n as f64;
        let speedup = scalar_per_call / simd_per_call;

        println!(
            "{:>10}  {:>10.1}  {:>10.1}  {:>7.2}x  {:>6}",
            n,
            scalar_ns_pt,
            simd_ns_pt,
            speedup,
            if took { "yes" } else { "no" }
        );
    }

    println!();
    println!("=== Inverse (NR iterative) ===");
    println!(
        "{:>10}  {:>10}  {:>10}  {:>8}  {:>6}",
        "N", "scalar ns/pt", "simd ns/pt", "speedup", "took?"
    );

    for &n in sizes {
        let pts = make_points(n);
        // Inverse must be timed against sky-domain inputs (forward of pixel
        // points) so NR converges in-domain; see bench_lsst.rs:32 for why.
        let sky = map.transform(&pts, true).unwrap();
        let iters = iterations_for(n) / 5; // inverse is ~5x slower per call
        let iters = iters.max(2);

        let scalar_per_call = time_scalar_inverse(&map, &sky, iters);
        let (simd_per_call, took) = time_simd_inverse(&map, &sky, iters);

        let scalar_ns_pt = scalar_per_call * 1e9 / n as f64;
        let simd_ns_pt = simd_per_call * 1e9 / n as f64;
        let speedup = scalar_per_call / simd_per_call;

        println!(
            "{:>10}  {:>10.1}  {:>10.1}  {:>7.2}x  {:>6}",
            n,
            scalar_ns_pt,
            simd_ns_pt,
            speedup,
            if took { "yes" } else { "no" }
        );
    }
}
