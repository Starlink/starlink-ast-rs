//! Focused profiling target: run the simplified LSST inverse for many seconds
//! so a sampling profiler (`samply record cargo run --release --example
//! profile_lsst_inverse`) sees the hot loop, not bench setup.

use ast_core::coords::PointSet;
use ast_core::from_native_string;

fn main() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/native/reference/lsst.ast"
    ))
    .expect("lsst.ast");
    let map = from_native_string(&text)
        .expect("parse lsst.ast")
        .into_mapping()
        .expect("not a Mapping")
        .simplify()
        .expect("simplify");

    // Build 10k input points.
    let rows: Vec<Vec<f64>> = (0..10_000)
        .map(|i| vec![100.0 + i as f64 * 0.01, 200.0 + i as f64 * 0.005])
        .collect();
    let pts = PointSet::from_rows(rows);

    // Get a forward output so we have valid targets for the inverse.
    let fwd = map.transform(&pts, true).expect("forward");

    // Run inverse in a tight loop for ~5 seconds of wall time.
    let target_secs = 5.0_f64;
    let start = std::time::Instant::now();
    let mut iters: u64 = 0;
    let mut sum_x = 0.0_f64;
    while start.elapsed().as_secs_f64() < target_secs {
        let out = map.transform(&fwd, false).expect("inverse");
        sum_x += out.point(0)[0];
        iters += 1;
    }
    let elapsed = start.elapsed().as_secs_f64();
    eprintln!(
        "inverse-only loop: {iters} iterations of 10k pts in {elapsed:.2}s, \
         {} ns/pt (checksum {sum_x})",
        (elapsed * 1e9 / (iters as f64 * 10_000.0)) as u64
    );
}
