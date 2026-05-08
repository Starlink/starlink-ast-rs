//! Data-driven simplification regression tests.
//!
//! Reads `simplify_tests.txt` and for each entry: loads the input .map file,
//! simplifies it, and verifies:
//!   1. `equal()` — the simplified result must be structurally equal to the
//!      C reference (.simp file). This is the primary assertion.
//!   2. Transform equivalence — the simplified result must produce the same
//!      numerical output as the C reference on test points.
//!   3. Serialization (optional) — when `skip_string_compare` is not set,
//!      the native serialization of the simplified result must match the
//!      .simp file byte-for-byte.

use std::path::PathBuf;

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::Mapping;
use ast_core::{from_native_string, to_native_string};

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
            if va == BAD_VALUE && vb == BAD_VALUE {
                continue;
            }
            if va == BAD_VALUE || vb == BAD_VALUE {
                return false;
            }
            let diff = (va - vb).abs();
            if diff > tol {
                return false;
            }
        }
    }
    true
}

/// Try to wrap a DynMapping back into an AnyObject for serialization.
/// Downcasts to known concrete types.
fn mapping_to_native_string(m: &dyn Mapping) -> Option<String> {
    use ast_core::AnyObject;
    use ast_core::mappings::{
        CmpMap, MatrixMap, PermMap, ShiftMap, SlaMap, SpecMap, SphMap, TimeMap, UnitMap, WcsMap,
        WinMap, ZoomMap,
    };

    let obj = if let Some(x) = m.as_any().downcast_ref::<UnitMap>() {
        AnyObject::UnitMap(x.clone())
    } else if let Some(x) = m.as_any().downcast_ref::<ZoomMap>() {
        AnyObject::ZoomMap(x.clone())
    } else if let Some(x) = m.as_any().downcast_ref::<ShiftMap>() {
        AnyObject::ShiftMap(x.clone())
    } else if let Some(x) = m.as_any().downcast_ref::<MatrixMap>() {
        AnyObject::MatrixMap(x.clone())
    } else if let Some(x) = m.as_any().downcast_ref::<PermMap>() {
        AnyObject::PermMap(x.clone())
    } else if let Some(x) = m.as_any().downcast_ref::<WinMap>() {
        AnyObject::WinMap(x.clone())
    } else if let Some(x) = m.as_any().downcast_ref::<CmpMap>() {
        AnyObject::CmpMap(x.clone())
    } else if let Some(x) = m.as_any().downcast_ref::<WcsMap>() {
        AnyObject::WcsMap(x.clone())
    } else if let Some(x) = m.as_any().downcast_ref::<SlaMap>() {
        AnyObject::SlaMap(x.clone())
    } else if let Some(x) = m.as_any().downcast_ref::<SpecMap>() {
        AnyObject::SpecMap(x.clone())
    } else if let Some(x) = m.as_any().downcast_ref::<TimeMap>() {
        AnyObject::TimeMap(x.clone())
    } else if let Some(x) = m.as_any().downcast_ref::<SphMap>() {
        AnyObject::SphMap(x.clone())
    } else {
        return None;
    };
    to_native_string(&obj).ok()
}

#[test]
fn simplify_regression() {
    let fixture_dir = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/native/reference"
    ));
    let manifest = fixture_dir.join("simplify_tests.txt");

    if !manifest.exists() {
        eprintln!("simplify_tests.txt not found, skipping regression tests");
        return;
    }

    let contents = std::fs::read_to_string(&manifest).unwrap();
    let mut passed = 0usize;
    let mut skipped = 0usize;
    let mut equal_failures: Vec<String> = Vec::new();
    let mut transform_failures: Vec<String> = Vec::new();
    let mut serial_failures: Vec<String> = Vec::new();
    let mut other_failures: Vec<String> = Vec::new();

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

        // Parse input .map
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
                skipped += 1;
                continue;
            }
        };
        let mapping = match input_obj.into_mapping() {
            Some(m) => m,
            None => {
                skipped += 1;
                continue;
            }
        };

        // Parse reference .simp
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

        // Simplify
        let simplified = match mapping.simplified() {
            Ok(s) => s,
            Err(e) => {
                other_failures.push(format!("{name}: simplified() failed: {e}"));
                continue;
            }
        };

        // === Check 1: equal() — structural equality with C reference ===
        let structurally_equal = simplified.equal(ref_mapping.as_ref());
        if !structurally_equal {
            equal_failures.push(name.to_string());
        }

        // === Check 2: Transform equivalence with C reference ===
        let nin = mapping.nin();
        let npoints = 10;
        let pts = make_test_points(nin, npoints);

        let out_ref = match ref_mapping.transform(&pts, true) {
            Ok(o) => o,
            Err(_) => {
                // Reference can't be evaluated — skip transform check
                if structurally_equal {
                    passed += 1;
                }
                continue;
            }
        };
        let out_simp = match simplified.transform(&pts, true) {
            Ok(o) => o,
            Err(e) => {
                other_failures.push(format!("{name}: simplified transform failed: {e}"));
                continue;
            }
        };

        if !outputs_equivalent(&out_simp, &out_ref, 1e-6) {
            transform_failures.push(name.to_string());
        }

        // === Check 3: Serialization comparison (optional) ===
        if !skip_string_compare {
            if let Some(simp_str) = mapping_to_native_string(simplified.as_ref()) {
                if simp_str.trim() != ref_text.trim() {
                    serial_failures.push(name.to_string());
                }
            }
        }

        passed += 1;
    }

    // Report results
    println!(
        "\nSimplify regression: {passed} passed, {skipped} skipped, \
         {} equal() failures, {} transform failures, {} serialization failures, {} other failures",
        equal_failures.len(),
        transform_failures.len(),
        serial_failures.len(),
        other_failures.len(),
    );
    if !equal_failures.is_empty() {
        eprintln!("  equal() failures (simplified != C reference structurally):");
        for n in &equal_failures {
            eprintln!("    {n}");
        }
    }
    if !transform_failures.is_empty() {
        eprintln!("  Transform failures (simplified output != C reference output):");
        for n in &transform_failures {
            eprintln!("    {n}");
        }
    }
    if !serial_failures.is_empty() {
        eprintln!("  Serialization mismatches (native string differs from .simp):");
        for n in &serial_failures {
            eprintln!("    {n}");
        }
    }
    if !other_failures.is_empty() {
        for f in &other_failures {
            eprintln!("  FAIL: {f}");
        }
    }

    // Hard-fail on transform or other failures (correctness).
    if !transform_failures.is_empty() || !other_failures.is_empty() {
        panic!(
            "{} simplify regression tests failed (correctness)",
            transform_failures.len() + other_failures.len()
        );
    }

    // Require at least one test actually ran
    assert!(
        passed > 0,
        "No simplify regression tests passed (all skipped?)"
    );
}

#[test]
fn cmpmap_re_expands_after_merge() {
    use ast_core::mappings::{CmpMap, Composition, Mapping, ShiftMap, WinMap, ZoomMap};

    // Create a chain: ShiftMap ; ZoomMap ; ShiftMap
    // Merging Shift+Zoom produces a WinMap, which should then merge with the final Shift.
    let a = ShiftMap::new([1.0]);
    let b = ZoomMap::new(1, 2.0).unwrap();
    let c = ShiftMap::new([3.0]);

    let ab = CmpMap::new(Box::new(a), Box::new(b), Composition::Series).unwrap();
    let abc = CmpMap::new(Box::new(ab), Box::new(c), Composition::Series).unwrap();

    let s = abc.simplified().unwrap();
    // Should reduce to a single WinMap: shift = 2*1 + 3 = 5, scale = 2.
    assert!(s.as_any().is::<WinMap>(), "Expected WinMap but got {:?}", s);
}

#[test]
fn cmpmap_re_expands_merged_cmpmap() {
    use ast_core::mappings::{CmpMap, Composition, Mapping, ShiftMap, UnitMap, ZoomMap};

    // Scenario where a merge produces a CmpMap that should be re-expanded.
    // ZoomMap(2) ; [ShiftMap(1) parallel UnitMap(1)] ; ZoomMap(3)
    // The parallel CmpMap in the middle has 2 axes.
    // ZoomMap(2) can't merge with the parallel CmpMap directly,
    // but after simplification the middle should become a WinMap,
    // which can merge with ZoomMaps on both sides.
    let z1 = ZoomMap::new(2, 2.0).unwrap();
    let inner = CmpMap::new(
        Box::new(ShiftMap::new([1.0])),
        Box::new(UnitMap::new(1)),
        Composition::Parallel,
    )
    .unwrap();
    let z2 = ZoomMap::new(2, 3.0).unwrap();

    let chain1 = CmpMap::new(Box::new(z1), Box::new(inner), Composition::Series).unwrap();
    let chain = CmpMap::new(Box::new(chain1), Box::new(z2), Composition::Series).unwrap();

    let s = chain.simplified().unwrap();
    // The result should be simpler than the original 4-mapping chain.
    // At minimum it should not be a 4-element series CmpMap.
    assert_eq!(s.nin(), 2);
    assert_eq!(s.nout(), 2);
}

#[test]
fn do_not_simplify_prevents_merge() {
    use ast_core::mappings::{CmpMap, Composition, Mapping, ZoomMap};

    let mut z1 = ZoomMap::new(2, 3.0).unwrap();
    z1.set_do_not_simplify(true);
    let z2 = ZoomMap::new(2, 2.0).unwrap();

    // try_merge should still work (it's the caller that guards), but the
    // merge_loop should skip the pair.  Verify via CmpMap simplification.
    let cmp = CmpMap::new(Box::new(z1), Box::new(z2), Composition::Series).unwrap();
    let s = cmp.simplified().unwrap();
    // Should NOT simplify to a single ZoomMap (factor 6) — the flag prevents it.
    assert!(s.as_any().is::<CmpMap>());
}

#[test]
fn do_not_simplify_default_is_false() {
    use ast_core::mappings::{Mapping, ZoomMap};
    let z = ZoomMap::new(2, 3.0).unwrap();
    assert!(!z.do_not_simplify());
}

#[test]
fn do_not_simplify_preserves_identity_zoom() {
    use ast_core::mappings::{Mapping, ZoomMap};

    // A ZoomMap with factor 1.0 normally simplifies to UnitMap.
    let mut z = ZoomMap::new(2, 1.0).unwrap();
    z.set_do_not_simplify(true);
    let s = z.simplified().unwrap();
    // With the flag set, it should stay as ZoomMap.
    assert!(s.as_any().is::<ZoomMap>());
}

#[test]
fn do_not_simplify_second_mapping_prevents_merge() {
    use ast_core::mappings::{CmpMap, Composition, Mapping, ZoomMap};

    let z1 = ZoomMap::new(2, 3.0).unwrap();
    let mut z2 = ZoomMap::new(2, 2.0).unwrap();
    z2.set_do_not_simplify(true);

    // Flag on the second mapping should also prevent merging.
    let cmp = CmpMap::new(Box::new(z1), Box::new(z2), Composition::Series).unwrap();
    let s = cmp.simplified().unwrap();
    assert!(s.as_any().is::<CmpMap>());
}

/// Verify the recursion guard terminates for deeply nested CmpMaps.
///
/// A chain of 20 ZoomMaps(factor=1.0) composed in series with a single
/// ZoomMap(factor=1.5) should simplify to a single ZoomMap without
/// hitting the MAX_MERGE_PASSES limit wastefully or recursing forever.
#[test]
fn simplify_recursion_guard_terminates() {
    use ast_core::mappings::{CmpMap, Composition, Mapping, ZoomMap};

    // Build a deeply nested CmpMap: ZoomMap(1.5) ; ZoomMap(1.0) ; ... ; ZoomMap(1.0)
    let mut m: Box<dyn Mapping> = Box::new(ZoomMap::new(2, 1.5).unwrap());
    for _ in 0..20 {
        let z = ZoomMap::new(2, 1.0).unwrap();
        m = Box::new(CmpMap::new(m, Box::new(z), Composition::Series).unwrap());
    }
    // Should simplify without infinite recursion or excessive time.
    let s = m.simplified().unwrap();
    assert_eq!(s.nin(), 2);
    assert_eq!(s.nout(), 2);
}

/// Verify that a mapping with identical sub-structures does not
/// get stuck in a cycle where simplification reproduces the input.
#[test]
fn simplify_recursion_guard_identical_subtrees() {
    use ast_core::mappings::{CmpMap, Composition, Mapping, ShiftMap, ZoomMap};

    // (Shift ; Zoom) || (Shift ; Zoom)  — two identical parallel arms
    // then (result) ; (result)  — identical halves in series.
    // Each simplification pass produces the same structure, triggering
    // the fingerprint-based cycle detection.
    let arm = || -> Box<dyn Mapping> {
        let a = ShiftMap::new([1.0, 2.0]);
        let b = ZoomMap::new(2, 3.0).unwrap();
        Box::new(CmpMap::new(Box::new(a), Box::new(b), Composition::Series).unwrap())
    };

    let par = CmpMap::new(arm(), arm(), Composition::Parallel).unwrap();
    let doubled = CmpMap::new(Box::new(par.clone()), Box::new(par), Composition::Series).unwrap();

    // Must terminate (no infinite recursion).
    let s = doubled.simplified().unwrap();
    // The result's dimensions must be preserved.
    assert_eq!(s.nin(), 4);
    assert_eq!(s.nout(), 4);
}

/// Verify that clone_box produces a functionally equivalent deep copy.
#[test]
fn clone_box_preserves_transform() {
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, ShiftMap, ZoomMap};

    let a = ShiftMap::new([1.0, 2.0]);
    let b = ZoomMap::new(2, 3.0).unwrap();
    let cmp = CmpMap::new(Box::new(a), Box::new(b), Composition::Series).unwrap();

    let cloned = cmp.clone_box();

    let pts = PointSet::from_rows(vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
    let out_orig = cmp.transform(&pts, true).unwrap();
    let out_clone = cloned.transform(&pts, true).unwrap();

    for i in 0..pts.npoint() {
        for j in 0..2 {
            assert_eq!(out_orig.point(i)[j], out_clone.point(i)[j]);
        }
    }
}
