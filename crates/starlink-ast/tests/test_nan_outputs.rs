//! No transformation gives a NaN output: a bad output is always `BAD_VALUE`,
//! and a NaN input is taken to be bad (the design rule in CLAUDE.md). C does
//! the same when the `AST_REPLACE_NAN` environment variable is `1`, which
//! makes `astTransform` replace every NaN in its result (mapping.c
//! `astTransform_`, pointset.c `ReplaceNaN`).
//!
//! Each Mapping is given points with a NaN on one input axis at a time, and
//! transformed in each direction it defines.

use ndarray::array;
use starlink_ast::coords::PointSet;
use starlink_ast::mappings::{
    CmpMap, Composition, LutMap, Mapping, MathMap, MatrixMap, PcdMap, PermMap, PolyMap, PolyTerm,
    ShiftMap, SlaCvtType, SlaMap, SpecCvtType, SpecMap, SphMap, TimeCvtType, TimeMap, TranMap,
    UnitMap, UnitNormMap, WcsMap, WcsProjection, WinMap, ZoomMap,
};

fn mappings() -> Vec<(&'static str, Box<dyn Mapping>)> {
    let mut sla = SlaMap::new();
    sla.add(SlaCvtType::Eqgal, &[]).unwrap();
    let mut spec = SpecMap::new();
    spec.add(SpecCvtType::FrToWv, &[]).unwrap();
    let mut time = TimeMap::new();
    time.add(TimeCvtType::MjdToJd, &[0.0, 0.0]).unwrap();
    let poly = PolyMap::new(
        2,
        2,
        &[
            PolyTerm {
                coeff: 1.0,
                powers: vec![1, 0],
                out_axis: 1,
            },
            PolyTerm {
                coeff: 2.0,
                powers: vec![0, 1],
                out_axis: 2,
            },
        ],
        None,
    )
    .unwrap();
    vec![
        ("UnitMap", Box::new(UnitMap::new(2))),
        ("ZoomMap", Box::new(ZoomMap::new(2, 2.0).unwrap())),
        ("ShiftMap", Box::new(ShiftMap::new(vec![1.0, 2.0]))),
        (
            "WinMap",
            Box::new(WinMap::new(&[1.0, -2.0], &[2.0, 3.0]).unwrap()),
        ),
        (
            "PermMap",
            Box::new(PermMap::new(&[2, 1], &[2, 1], &[]).unwrap()),
        ),
        (
            "MatrixMap diagonal",
            Box::new(MatrixMap::new(array![[2.0, 0.0], [0.0, 3.0]]).unwrap()),
        ),
        (
            "MatrixMap full",
            Box::new(MatrixMap::new(array![[1.0, 0.5], [0.25, 2.0]]).unwrap()),
        ),
        (
            "LutMap",
            Box::new(LutMap::new(&[1.0, 2.0, 4.0], 0.0, 1.0).unwrap()),
        ),
        ("PolyMap", Box::new(poly)),
        (
            "MathMap",
            Box::new(MathMap::new(2, 2, &["y1=x1", "y2=x2*2"], &["x1=y1", "x2=y2/2"]).unwrap()),
        ),
        ("SphMap", Box::new(SphMap::new())),
        (
            "UnitNormMap",
            Box::new(UnitNormMap::new(vec![0.5, 0.5]).unwrap()),
        ),
        (
            "WcsMap",
            Box::new(WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap()),
        ),
        ("SlaMap", Box::new(sla)),
        ("SpecMap", Box::new(spec)),
        ("TimeMap", Box::new(time)),
        ("PcdMap", Box::new(PcdMap::new(1.0e-3, [0.5, 0.5]))),
        (
            "TranMap",
            Box::new(
                TranMap::new(
                    Box::new(ZoomMap::new(2, 2.0).unwrap()),
                    Box::new(UnitMap::new(2)),
                )
                .unwrap(),
            ),
        ),
        (
            "CmpMap",
            Box::new(
                CmpMap::new(
                    Box::new(ZoomMap::new(1, 2.0).unwrap()),
                    Box::new(UnitMap::new(1)),
                    Composition::Parallel,
                )
                .unwrap(),
            ),
        ),
    ]
}

#[test]
fn no_transformation_gives_nan() {
    let mut failures = Vec::new();
    for (name, map) in mappings() {
        for forward in [true, false] {
            let defined = if forward {
                map.has_forward()
            } else {
                map.has_inverse()
            };
            if !defined {
                continue;
            }
            let n = if forward { map.nin() } else { map.nout() };
            for axis in 0..n {
                let mut point: Vec<f64> = (0..n).map(|i| 0.1 + 0.2 * i as f64).collect();
                point[axis] = f64::NAN;
                let out = map
                    .transform(&PointSet::from_rows([point]), forward)
                    .unwrap();
                if out.point(0).iter().any(|v| v.is_nan()) {
                    failures.push(format!(
                        "{name} forward={forward} NaN on axis {}: {:?}",
                        axis + 1,
                        out.point(0)
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
