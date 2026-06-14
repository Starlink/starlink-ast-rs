//! Supplementary native-text round-trip check for every Mapping class.
//!
//! The authoritative serializer test is
//! `test_serialisation::native_byte_exact_round_trip_simplify_fixtures`,
//! which reads ~389 C-generated `.map` / `.simp` files and asserts each
//! survives `from_native_string → to_native_string` byte-exact. This file
//! is a supplement: it constructs Rust-side instances with specific
//! non-default attribute values (e.g. non-zero `UnitNormMap` centre,
//! non-trivial `PolyMap` iter-inverse settings) and asserts the Rust↔Rust
//! double-round-trip preserves them.
//!
//! The fixture corpus tests don't cover every possible attribute setting
//! — they only catch cases where a fixture exists. This file catches the
//! "silently drop non-default attribute" class of bug where Rust's
//! serializer omits a field that the corpus's specific fixtures happen
//! not to exercise.
//!
//! When a class has both kinds of failure (corpus + targeted-attribute),
//! fix the corpus failure first — that's the C-byte-exact contract.

use ast_core::frames::BasicFrameBuilder;
use ast_core::mappings::{
    ChebyMap, GrismMap, LutInterp, LutMap, MathMap, NormMap, PcdMap, PolyMap, PolyTerm, RateMap,
    ShiftMap, SlaCvtType, SlaMap, SpecCvtType, SpecMap, SphMap, SplineMap, TimeCvtType, TimeMap,
    TranMap, UnitNormMap, WcsMap, WcsProjection, WinMap, ZoomMap,
};
use ast_core::{AnyObject, from_native_string, to_native_string};

/// Helper: serialise twice and check the two texts match. If they don't,
/// produce a unified diff so the missing/changed lines are easy to spot.
fn assert_double_round_trip(obj: AnyObject, name: &str) {
    let text1 = to_native_string(&obj).unwrap_or_else(|e| {
        panic!("{name}: first to_native_string failed: {e}");
    });
    let restored = from_native_string(&text1).unwrap_or_else(|e| {
        panic!("{name}: from_native_string failed: {e}\ntext:\n{text1}");
    });
    let text2 = to_native_string(&restored).unwrap_or_else(|e| {
        panic!("{name}: second to_native_string failed: {e}");
    });

    if text1 != text2 {
        // Build a line-diff for the panic message. Crude but readable.
        let mut diff = String::new();
        let lines1: Vec<&str> = text1.lines().collect();
        let lines2: Vec<&str> = text2.lines().collect();
        for (i, (a, b)) in lines1.iter().zip(lines2.iter()).enumerate() {
            if a != b {
                diff.push_str(&format!("line {i}: - {a}\n        + {b}\n"));
            }
        }
        if lines1.len() != lines2.len() {
            diff.push_str(&format!(
                "length differs: first={} lines, second={} lines\n",
                lines1.len(),
                lines2.len()
            ));
            if lines1.len() > lines2.len() {
                for (i, line) in lines1.iter().enumerate().skip(lines2.len()) {
                    diff.push_str(&format!("first-only line {i}: {line}\n"));
                }
            } else {
                for (i, line) in lines2.iter().enumerate().skip(lines1.len()) {
                    diff.push_str(&format!("second-only line {i}: {line}\n"));
                }
            }
        }
        panic!("{name}: double round-trip text mismatch:\n{diff}");
    }
}

// ---------------------------------------------------------------------------
// One subtest per class. Classes already covered by existing per-instance
// tests in test_serialisation.rs are not duplicated here (ZoomMap, ShiftMap,
// MatrixMap, PermMap, CmpMap, UnitMap).
// ---------------------------------------------------------------------------

#[test]
fn round_trip_winmap() {
    let m = WinMap::new(&[0.5, -0.25], &[1.5, 0.75]).unwrap();
    assert_double_round_trip(AnyObject::WinMap(m), "WinMap");
}

#[test]
fn round_trip_sphmap() {
    let m = SphMap::new();
    assert_double_round_trip(AnyObject::SphMap(m), "SphMap");
}

#[test]
fn round_trip_lutmap() {
    let lut: Vec<f64> = (0..20)
        .map(|i| i as f64 + 0.1 * (i as f64).powf(1.3))
        .collect();
    let mut m = LutMap::new(&lut, 0.0, 1.0).unwrap();
    m.set_interp(LutInterp::Linear);
    assert_double_round_trip(AnyObject::LutMap(m), "LutMap");
}

#[test]
fn round_trip_polymap_with_iter_inverse() {
    // Match the shape used by bench_per_class: cover iter_inverse +
    // niter + tol attributes that aren't on the default path.
    let mut terms = Vec::new();
    for (px, py, coeff, out) in [
        (1, 0, 1.0, 1),
        (0, 1, 0.05, 1),
        (2, 0, 0.02, 1),
        (0, 1, 1.0, 2),
        (1, 0, 0.05, 2),
        (0, 2, 0.02, 2),
    ] {
        terms.push(PolyTerm {
            coeff,
            powers: vec![px, py],
            out_axis: out,
        });
    }
    let mut m = PolyMap::new(2, 2, &terms, None).unwrap();
    m.set_iter_inverse(true);
    m.set_niter_inverse(10);
    m.set_tol_inverse(1e-7);
    assert_double_round_trip(AnyObject::PolyMap(m), "PolyMap (iter inverse)");
}

#[test]
fn round_trip_wcsmap_tan() {
    let m = WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap();
    assert_double_round_trip(AnyObject::WcsMap(m), "WcsMap TAN");
}

#[test]
fn round_trip_wcsmap_zpn() {
    let mut m = WcsMap::new(2, WcsProjection::Zpn, 1, 2).unwrap();
    m.set_pv(2, 1, 1.0).unwrap();
    m.set_pv(2, 3, 0.05).unwrap();
    assert_double_round_trip(AnyObject::WcsMap(m), "WcsMap ZPN with PV");
}

#[test]
fn round_trip_chebymap() {
    // Non-default bounds so they participate in serialisation.
    let mut terms = Vec::new();
    for (px, py, coeff) in [(1, 0, 1.0), (0, 1, 0.05), (2, 0, 0.02)] {
        terms.push(PolyTerm {
            coeff,
            powers: vec![px, py],
            out_axis: 1,
        });
        terms.push(PolyTerm {
            coeff,
            powers: vec![py, px],
            out_axis: 2,
        });
    }
    let m = ChebyMap::new(2, 2, &terms, None, &[-2.0, -2.0], &[2.0, 2.0], None, None).unwrap();
    assert_double_round_trip(
        AnyObject::ChebyMap(m),
        "ChebyMap (no inv, non-default bounds)",
    );
}

#[test]
fn round_trip_pcdmap() {
    let m = PcdMap::new(1.0e-3, [0.1, -0.2]);
    assert_double_round_trip(AnyObject::PcdMap(m), "PcdMap");
}

#[test]
fn round_trip_grismmap() {
    let m = GrismMap::new(1.5, 0.0, 5.0e-7, 0.1, 1.0e7, 1, 1.0, 0.0);
    assert_double_round_trip(AnyObject::GrismMap(m), "GrismMap");
}

#[test]
fn round_trip_unitnormmap() {
    let m = UnitNormMap::new(vec![0.5, -0.5]);
    assert_double_round_trip(AnyObject::UnitNormMap(m.clone()), "UnitNormMap");
    // Also assert the serialised text *mentions* the non-zero centre.
    // (Double round-trip passes for both [0,0] and [0.5,-0.5] because
    // Rust's parser defaults centre to [0,0] if absent — symmetric loss.
    // The C library has no such default, so the bench harness crashes.)
    let text = to_native_string(&AnyObject::UnitNormMap(m)).unwrap();
    let mentions_centre = text.contains("0.5") && text.contains("-0.5");
    assert!(
        mentions_centre,
        "UnitNormMap serialisation should mention the centre values \
         (0.5, -0.5) but the output is:\n{text}"
    );
}

#[test]
fn round_trip_normmap() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let m = NormMap::new(Box::new(frame));
    assert_double_round_trip(AnyObject::NormMap(m), "NormMap");
}

#[test]
fn round_trip_tranmap() {
    let z = ZoomMap::new(2, 2.0).unwrap();
    let s = ShiftMap::new([0.5, 0.5]);
    let m = TranMap::new(Box::new(z), Box::new(s)).unwrap();
    assert_double_round_trip(AnyObject::TranMap(m), "TranMap");
}

#[test]
fn round_trip_ratemap() {
    let mut terms = Vec::new();
    for (px, py, coeff, out) in [
        (1, 0, 1.0, 1),
        (0, 1, 0.1, 1),
        (1, 0, 0.1, 2),
        (0, 1, 1.0, 2),
    ] {
        terms.push(PolyTerm {
            coeff,
            powers: vec![px, py],
            out_axis: out,
        });
    }
    let inner = PolyMap::new(2, 2, &terms, None).unwrap();
    let m = RateMap::new(Box::new(inner), 1, 1).unwrap();
    assert_double_round_trip(AnyObject::RateMap(m), "RateMap");
}

#[test]
fn round_trip_slamap() {
    let mut m = SlaMap::new();
    m.add(SlaCvtType::Eqgal, &[]);
    assert_double_round_trip(AnyObject::SlaMap(m), "SlaMap eqgal");
}

#[test]
fn round_trip_timemap() {
    let mut m = TimeMap::new();
    m.add(TimeCvtType::TaiToUtc, &[50000.0]);
    assert_double_round_trip(AnyObject::TimeMap(m), "TimeMap tai->utc");
}

#[test]
fn round_trip_specmap() {
    let mut m = SpecMap::new();
    m.add(SpecCvtType::WnToFr, &[]);
    assert_double_round_trip(AnyObject::SpecMap(m), "SpecMap wn->fr");
}

// DssMap has a crate-private constructor (mirroring C's protected astDssMap),
// so its native round-trip is covered in-crate by the `dssmap` unit tests
// (`dssmap_serialization_roundtrip`) rather than here.

#[test]
fn round_trip_splinemap() {
    let kx = 4;
    let ky = 4;
    let nx = 5;
    let ny = 5;
    let tx: Vec<f64> = vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0];
    let ty = tx.clone();
    let mut cu = vec![0.0; nx * ny];
    let mut cv = vec![0.0; nx * ny];
    for i in 0..nx {
        for j in 0..ny {
            cu[j * nx + i] = i as f64 / (nx - 1) as f64 + 0.05 * j as f64;
            cv[j * nx + i] = j as f64 / (ny - 1) as f64 + 0.05 * i as f64;
        }
    }
    let m = SplineMap::new(kx, ky, nx, ny, tx, ty, cu, cv);
    assert_double_round_trip(AnyObject::SplineMap(m), "SplineMap");
}

#[test]
fn round_trip_mathmap() {
    // A modest 2-in / 2-out MathMap with paired forward and inverse
    // expressions. SimpFI/SimpIF stay at their defaults so the round-trip
    // exercises the full attribute set, including the parsed expression
    // lists.
    let m = MathMap::new(
        2,
        2,
        &["u = x + y", "v = x - y"],
        &["x = 0.5 * (u + v)", "y = 0.5 * (u - v)"],
    )
    .unwrap();
    assert_double_round_trip(AnyObject::MathMap(m), "MathMap");
}
