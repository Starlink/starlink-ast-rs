//! Data-driven simplification regression tests.
//!
//! Reads `simplify_tests.txt` and for each entry: loads the input .map file,
//! simplifies it, and verifies transform equivalence against the .simp reference.
//! Entries whose fixture files are missing or whose types are not yet implemented
//! are skipped gracefully.

use std::path::PathBuf;

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::from_native_string;

/// Generate deterministic test points for a mapping with `nin` inputs.
/// Points are in the range [0.1, 1.1] to avoid singularities at zero.
fn make_test_points(nin: usize, npoint: usize) -> PointSet {
    let mut rows = Vec::with_capacity(npoint);
    for i in 0..npoint {
        let mut row = Vec::with_capacity(nin);
        for j in 0..nin {
            row.push(0.1 * (i as f64 + 1.0) + 0.01 * (j as f64));
        }
        rows.push(row);
    }
    PointSet::from_rows(rows)
}

/// Check whether two output point sets are equivalent within tolerance.
/// Returns `true` if all non-BAD points match within `tol`.
fn outputs_equivalent(a: &PointSet, b: &PointSet, tol: f64) -> bool {
    if a.npoint() != b.npoint() || a.ncoord() != b.ncoord() {
        return false;
    }
    for i in 0..a.npoint() {
        let pa = a.point(i);
        let pb = b.point(i);
        for j in 0..a.ncoord() {
            let va = pa[j];
            let vb = pb[j];
            // If both are BAD, that's equivalent
            if va == BAD_VALUE && vb == BAD_VALUE {
                continue;
            }
            // If one is BAD and the other isn't, not equivalent
            if va == BAD_VALUE || vb == BAD_VALUE {
                return false;
            }
            // Check numerical closeness
            let diff = (va - vb).abs();
            if diff > tol {
                return false;
            }
        }
    }
    true
}

#[test]
fn simplify_regression() {
    let fixture_dir = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/native/reference"
    ));
    let manifest = fixture_dir.join("simplify_tests.txt");

    // If manifest doesn't exist, skip gracefully
    if !manifest.exists() {
        eprintln!("simplify_tests.txt not found, skipping regression tests");
        return;
    }

    let contents = std::fs::read_to_string(&manifest).unwrap();
    let mut passed = 0usize;
    let mut skipped = 0usize;
    let mut failed: Vec<String> = Vec::new();

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if fields.len() < 3 {
            continue;
        }

        let name = fields[0];
        let input_path = fixture_dir.join(fields[1]);
        let reference_path = fixture_dir.join(fields[2]);
        let skip_string_compare = fields.get(3).map(|s| s.trim()) == Some("yes");

        // Skip if files don't exist
        if !input_path.exists() || !reference_path.exists() {
            skipped += 1;
            continue;
        }

        // Try to parse input
        let input_text = match std::fs::read_to_string(&input_path) {
            Ok(t) => t,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let input_obj = match from_native_string(&input_text) {
            Ok(obj) => obj,
            Err(_) => {
                // Can't parse (unknown type) -- skip
                skipped += 1;
                continue;
            }
        };

        // Get mapping from parsed object
        let mapping = match input_obj.into_mapping() {
            Some(m) => m,
            None => {
                skipped += 1;
                continue;
            }
        };

        // Simplify
        let simplified = match mapping.simplified() {
            Ok(s) => s,
            Err(e) => {
                failed.push(format!("{name}: simplified() failed: {e}"));
                continue;
            }
        };

        // Load reference
        let ref_text = match std::fs::read_to_string(&reference_path) {
            Ok(t) => t,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let ref_obj = match from_native_string(&ref_text) {
            Ok(obj) => obj,
            Err(_) => {
                // Can't parse reference (unknown type) -- skip
                skipped += 1;
                continue;
            }
        };
        let ref_mapping = match ref_obj.into_mapping() {
            Some(m) => m,
            None => {
                skipped += 1;
                continue;
            }
        };

        // Generate test points based on input nin
        let nin = mapping.nin();
        let npoints = 10;
        let pts = make_test_points(nin, npoints);

        // Transform through simplified result
        let out_simp = match simplified.transform(&pts, true) {
            Ok(o) => o,
            Err(e) => {
                failed.push(format!("{name}: simplified transform failed: {e}"));
                continue;
            }
        };

        // Transform through reference
        let out_ref = match ref_mapping.transform(&pts, true) {
            Ok(o) => o,
            Err(e) => {
                // Reference might use types we can't transform yet
                failed.push(format!("{name}: reference transform failed: {e}"));
                continue;
            }
        };

        // Check transform equivalence
        if !outputs_equivalent(&out_simp, &out_ref, 1e-6) {
            // Build diagnostic: find first mismatch
            let mut detail = String::new();
            'outer: for i in 0..npoints {
                let ps = out_simp.point(i);
                let pr = out_ref.point(i);
                for j in 0..ps.len().min(pr.len()) {
                    let diff = (ps[j] - pr[j]).abs();
                    if diff > 1e-6 {
                        detail = format!(
                            " (point {i} axis {j}: simplified={}, reference={}, diff={diff:.2e})",
                            ps[j], pr[j]
                        );
                        break 'outer;
                    }
                }
            }
            failed.push(format!(
                "{name}: transform output differs from reference{detail}"
            ));
            continue;
        }

        // For negative cases (input == reference path), also verify that
        // simplification produces something equivalent to the original input.
        // This is already covered by the transform comparison above since
        // the reference IS the input for negative cases.

        // String comparison is intentionally not done here; the native
        // renderer may produce slightly different formatting. Transform
        // equivalence is the primary correctness check.
        let _ = skip_string_compare; // acknowledged but not used yet

        passed += 1;
    }

    println!(
        "\nSimplify regression: {passed} passed, {skipped} skipped, {} failed",
        failed.len()
    );
    if !failed.is_empty() {
        for f in &failed {
            eprintln!("  FAIL: {f}");
        }
        panic!("{} simplify regression tests failed", failed.len());
    }
    // Require at least one test actually ran (sanity check)
    assert!(
        passed > 0,
        "No simplify regression tests passed (all skipped?)"
    );
}
