// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Simplifying a Mapping does not change what it does.
//!
//! Ancestor: `ast_tester/testmapmerge.c`. Each case is a small Mapping for
//! which one class's merge or swap step gave a Mapping that was not
//! equivalent, or reported an error. The case names are C's.
//!
//! Divergences from the C test:
//! - Each case also checks the shape of the simplified Mapping (its class,
//!   and for a CmpMap the kind of composition and the classes and
//!   directions of its two components) against what the C library gives at
//!   the pinned commit, so that a merge C refuses cannot pass by giving an
//!   equivalent Mapping of another shape.
//! - C's PolyMap case checks that simplifying leaves the caller's PolyMaps
//!   with their Invert values. A CmpMap owns its components here, so the
//!   case checks the caller's copies, which simplifying cannot reach.

use starlink_ast::AnyObject;
use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;
use starlink_ast::mappings::{
    CmpMap, Composition, DynMapping, Mapping, MathMap, MatrixMap, PermMap, PolyMap, PolyTerm,
    ShiftMap, SwitchMap, TimeCvtType, TimeMap, UnitMap, WcsMap, WcsProjection, WinMap, ZoomMap,
};

/// The number of positions used to compare two Mappings.
const NPOINT: usize = 3;

/// `ncoord` coordinates of `NPOINT` small positive positions, which every
/// projection used here can project and deproject.
fn positions(ncoord: usize) -> Vec<Vec<f64>> {
    (0..ncoord)
        .map(|c| {
            (0..NPOINT)
                .map(|p| 0.11 + 0.07 * c as f64 + 0.05 * p as f64)
                .collect()
        })
        .collect()
}

/// Both bad, or equal to within a small fraction of their size.
fn agree(a: f64, b: f64) -> bool {
    if a == BAD_VALUE || b == BAD_VALUE {
        return a == b;
    }
    (a - b).abs() <= 1.0e-9 * 1.0f64.max(a.abs().max(b.abs()))
}

/// Compare one direction of `map` and `simp`; nothing if `map` lacks it.
fn compare(
    map: &dyn Mapping,
    simp: &dyn Mapping,
    forward: bool,
    input: Option<&[Vec<f64>]>,
) -> Result<(), String> {
    let dir = if forward { "forward" } else { "inverse" };
    let has = |m: &dyn Mapping| {
        if forward {
            m.has_forward()
        } else {
            m.has_inverse()
        }
    };
    if !has(map) {
        return Ok(());
    }
    if !has(simp) {
        return Err(format!("the {dir} transformation was lost"));
    }
    let (nin, nout) = if forward {
        (map.nin(), map.nout())
    } else {
        (map.nout(), map.nin())
    };
    let default = positions(nin);
    let input = input.unwrap_or(&default);
    let cols: Vec<&[f64]> = input.iter().map(Vec::as_slice).collect();
    let pts = PointSet::from_columns(&cols).unwrap();
    let out1 = map
        .transform(&pts, forward)
        .map_err(|e| format!("the {dir} transformation gave {e}"))?;
    let out2 = simp
        .transform(&pts, forward)
        .map_err(|e| format!("the simplified {dir} transformation gave {e}"))?;
    for c in 0..nout {
        for p in 0..NPOINT {
            let (a, b) = (out1.axis(c)[p], out2.axis(c)[p]);
            if !agree(a, b) {
                return Err(format!(
                    "{dir} coordinate {} of position {} was {a:.17e} and is {b:.17e} after simplifying",
                    c + 1,
                    p + 1
                ));
            }
        }
    }
    Ok(())
}

fn class(m: &dyn Mapping) -> String {
    AnyObject::from_mapping(m)
        .map(|o| o.class_name().to_string())
        .unwrap_or_else(|| "?".to_string())
}

/// The class of `m` and, for a CmpMap, the kind of composition and the
/// classes and directions of its components, as the C probe prints them.
fn shape(m: &dyn Mapping) -> String {
    let name = class(m);
    let Some(cmp) = m.as_any().downcast_ref::<CmpMap>() else {
        return name;
    };
    let (parts, kind) = cmp.decompose();
    let inv = |i: bool| if i { "^-1" } else { "" };
    format!(
        "{name} {}({}{},{}{})",
        if kind == Composition::Series {
            "series"
        } else {
            "parallel"
        },
        class(parts.map1),
        inv(parts.invert1),
        class(parts.map2),
        inv(parts.invert2)
    )
}

/// C's `check`: simplify `map`, and require no error, the same numbers of
/// inputs and outputs, the same transformations, and C's `shape`.
fn check(
    map: DynMapping,
    expected_shape: &str,
    fwd: Option<&[Vec<f64>]>,
    inv: Option<&[Vec<f64>]>,
) -> Result<DynMapping, String> {
    let simp = map.simplify().map_err(|e| format!("simplify gave {e}"))?;
    if simp.nin() != map.nin() || simp.nout() != map.nout() {
        return Err(format!(
            "had {} inputs and {} outputs, and has {} and {} after simplifying",
            map.nin(),
            map.nout(),
            simp.nin(),
            simp.nout()
        ));
    }
    compare(map.as_ref(), simp.as_ref(), true, fwd)?;
    compare(map.as_ref(), simp.as_ref(), false, inv)?;
    let got = shape(simp.as_ref());
    if got != expected_shape {
        return Err(format!(
            "simplified to {got}, where C gives {expected_shape}"
        ));
    }
    Ok(simp)
}

/// C's `check_equal`: `equal` gives `expected` in either order.
fn check_equal(a: DynMapping, b: DynMapping, expected: bool) -> Result<(), String> {
    let (ab, ba) = (a.equal(b.as_ref()), b.equal(a.as_ref()));
    if ab != expected || ba != expected {
        return Err(format!(
            "equal gave {ab} and {ba} but {expected} was expected"
        ));
    }
    Ok(())
}

fn series(a: DynMapping, b: DynMapping) -> DynMapping {
    Box::new(CmpMap::new(a, b, Composition::Series).unwrap())
}

fn parallel(a: DynMapping, b: DynMapping) -> DynMapping {
    Box::new(CmpMap::new(a, b, Composition::Parallel).unwrap())
}

fn inverted(mut m: DynMapping) -> DynMapping {
    m.invert();
    m
}

/// A Mapping with one input that no neighbour can absorb, and an inverse.
fn nonlinear() -> DynMapping {
    Box::new(MathMap::new(1, 1, &["y=exp(x)"], &["x=log(y)"]).unwrap())
}

fn wcs(naxes: usize, proj: WcsProjection) -> WcsMap {
    WcsMap::new(naxes, proj, 1, 2).unwrap()
}

fn mathmap(fwd: &str, inv: &str) -> DynMapping {
    Box::new(MathMap::new(1, 1, &[fwd], &[inv]).unwrap())
}

/// A SwitchMap with one input and one output that always uses `route`.
fn switchmap(route: DynMapping) -> DynMapping {
    Box::new(
        SwitchMap::new(
            Some(mathmap("r=1", "a")),
            Some(mathmap("a", "r=1")),
            vec![route],
        )
        .unwrap(),
    )
}

fn matrix(rows: usize, cols: usize, m: &[f64]) -> DynMapping {
    let a = ndarray::Array2::from_shape_vec((rows, cols), m.to_vec()).unwrap();
    Box::new(MatrixMap::new(a).unwrap())
}

fn diagonal(nin: usize, nout: usize, d: &[f64]) -> DynMapping {
    Box::new(MatrixMap::diagonal(nin, nout, d).unwrap())
}

/// C's unit MatrixMap; a diagonal of ones is stored in the unit form.
fn unit_matrix() -> DynMapping {
    diagonal(2, 2, &[1.0, 1.0])
}

fn permmap(inperm: &[i64], outperm: &[i64], con: &[f64]) -> DynMapping {
    Box::new(PermMap::new(inperm, outperm, con).unwrap())
}

fn zoom(n: usize, z: f64) -> DynMapping {
    Box::new(ZoomMap::new(n, z).unwrap())
}

fn shift(s: &[f64]) -> DynMapping {
    Box::new(ShiftMap::new(s.to_vec()))
}

fn winmap(ina: &[f64], inb: &[f64], outa: &[f64], outb: &[f64]) -> DynMapping {
    Box::new(WinMap::from_corners(ina, inb, outa, outb).unwrap())
}

const WIN_INA: [f64; 3] = [0.0, 0.0, 0.0];
const WIN_INB: [f64; 3] = [1.0, 2.0, 3.0];
const WIN_OUTA: [f64; 3] = [5.0, 6.0, 7.0];
const WIN_OUTB: [f64; 3] = [6.0, 9.0, 12.0];

fn win2() -> DynMapping {
    winmap(&WIN_INA[..2], &WIN_INB[..2], &WIN_OUTA[..2], &WIN_OUTB[..2])
}

fn win3() -> DynMapping {
    winmap(&WIN_INA, &WIN_INB, &WIN_OUTA, &WIN_OUTB)
}

/// The WinMap `test_winmap` puts beside two MathMaps.
fn win_scaled() -> DynMapping {
    winmap(&[0.0, 0.0], &[1.0, 1.0], &[3.0, 4.0], &[4.0, 6.0])
}

type Case = fn() -> Result<(), String>;

/// Every case of the C test, by C's description.
const CASES: &[(&str, Case)] = &[
    // PermMaps.
    ("a PermMap with no outperm array", || {
        let perm = PermMap::with_null_perms(1, Some(&[-1]), 2, None, &[9.0]).unwrap();
        let out = perm
            .transform(&PointSet::from_columns(&[&[5.0]]).unwrap(), true)
            .map_err(|e| e.to_string())?;
        if out.axis(0)[0] != 5.0 || out.axis(1)[0] != BAD_VALUE {
            return Err(format!(
                "gave ({}, {}) for an input of 5, not (5, <bad>)",
                out.axis(0)[0],
                out.axis(1)[0]
            ));
        }
        Ok(())
    }),
    ("PermMap with no outperm array, then a PermMap", || {
        let perm = PermMap::with_null_perms(1, Some(&[-1]), 2, None, &[9.0]).unwrap();
        let map = series(Box::new(perm), permmap(&[2, 1], &[2, 1], &[]));
        check(map, "PermMap", None, None).map(drop)
    }),
    ("PermMap with no inperm array", || {
        let map = PermMap::with_null_perms(2, None, 1, Some(&[-1]), &[9.0]).unwrap();
        check(Box::new(map), "PermMap", None, None).map(drop)
    }),
    (
        "inverted CmpMap of an inverted PermMap and a ZoomMap",
        || {
            let perm = inverted(permmap(&[1, 2, 0], &[1, 2], &[]));
            let map = inverted(series(perm, zoom(3, 2.0)));
            check(map, "CmpMap series(ZoomMap,PermMap)", None, None).map(drop)
        },
    ),
    // A PermMap followed by two Mappings in parallel.
    (
        "constant PermMap, then a WcsMap and ZoomMap in parallel",
        || {
            let par = parallel(Box::new(wcs(2, WcsProjection::Tan)), zoom(1, 3.0));
            let perm = permmap(&[-1, -2, -3], &[-4, -5, -6], &CON3);
            check(
                series(perm, par),
                "CmpMap series(PermMap,CmpMap)",
                None,
                None,
            )
            .map(drop)
        },
    ),
    (
        "constant PermMap with bad inputs, then a WcsMap and ZoomMap in parallel",
        || {
            let par = parallel(Box::new(wcs(2, WcsProjection::Tan)), zoom(1, 3.0));
            let perm = permmap(&[0, 0, 0], &[-4, -5, -6], &CON3);
            check(
                series(perm, par),
                "CmpMap series(PermMap,CmpMap)",
                None,
                None,
            )
            .map(drop)
        },
    ),
    (
        "constant PermMap, then a ZoomMap and WcsMap in parallel",
        || {
            let par = parallel(zoom(1, 3.0), Box::new(wcs(2, WcsProjection::Tan)));
            let perm = permmap(&[-1, -2, -3], &[-4, -5, -6], &CON3);
            check(
                series(perm, par),
                "CmpMap series(PermMap,CmpMap)",
                None,
                None,
            )
            .map(drop)
        },
    ),
    ("constant PermMap, then two WcsMaps in parallel", || {
        let par = parallel(
            Box::new(wcs(2, WcsProjection::Tan)),
            Box::new(wcs(2, WcsProjection::Sin)),
        );
        let con4 = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
        let perm = permmap(&[-1, -2, -3, -4], &[-5, -6, -7, -8], &con4);
        check(
            series(perm, par),
            "CmpMap series(PermMap,CmpMap)",
            None,
            None,
        )
        .map(drop)
    }),
    (
        "PermMap whose inverse copies a constant output, then two MathMaps in parallel",
        || {
            let par = parallel(nonlinear(), nonlinear());
            let perm = permmap(&[2, 1], &[-1, 1], &[5.0]);
            check(
                series(perm, par),
                "CmpMap series(PermMap,CmpMap)",
                None,
                None,
            )
            .map(drop)
        },
    ),
    // WcsMaps.
    (
        "AZP projection, then the inverse of a different AZP projection",
        || {
            let mut wcs2 = wcs(2, WcsProjection::Azp);
            wcs2.set_pv(2, 1, 0.5).map_err(|e| e.to_string())?;
            let map = series(
                Box::new(wcs(2, WcsProjection::Azp)),
                inverted(Box::new(wcs2)),
            );
            let plane = [vec![0.1, 0.2, 0.3], vec![1.2, 1.1, 1.0]];
            check(map, "CmpMap series(WcsMap,WcsMap^-1)", Some(&plane), None).map(drop)
        },
    ),
    ("inverse AZP projection, then the same projection", || {
        let mut wcs1 = wcs(2, WcsProjection::Azp);
        wcs1.set_pv(2, 1, 0.5).map_err(|e| e.to_string())?;
        let wcs2 = wcs1.clone();
        check(
            series(inverted(Box::new(wcs1)), Box::new(wcs2)),
            "UnitMap",
            None,
            None,
        )
        .map(drop)
    }),
    ("PermMap then a WcsMap", || {
        let map = series(
            permmap(&[1, 2], &[1, 2, -1], &[5.0]),
            Box::new(wcs(3, WcsProjection::Tan)),
        );
        check(map, "CmpMap series(WcsMap,PermMap)", None, None).map(drop)
    }),
    ("inverted CmpMap of a PermMap and a WcsMap", || {
        let map = series(
            permmap(&[1, 2], &[1, 2, -1], &[5.0]),
            Box::new(wcs(3, WcsProjection::Tan)),
        );
        check(
            inverted(map),
            "CmpMap series(PermMap,WcsMap^-1)",
            None,
            None,
        )
        .map(drop)
    }),
    // A PolyMap and its inverse.
    ("a PolyMap beside its own inverse", || {
        let fwd = [
            PolyTerm {
                coeff: 1.0,
                powers: vec![2],
                out_axis: 1,
            },
            PolyTerm {
                coeff: 3.0,
                powers: vec![1],
                out_axis: 1,
            },
        ];
        let inv = [PolyTerm {
            coeff: 0.5,
            powers: vec![1],
            out_axis: 1,
        }];
        let p = PolyMap::new(1, 1, &fwd, Some(&inv)).unwrap();
        let mut q = p.clone();
        q.invert();
        let map = inverted(series(Box::new(p.clone()), Box::new(q.clone())));
        let simp = map.simplify().map_err(|e| e.to_string())?;
        if !simp.as_any().is::<UnitMap>() {
            return Err(format!(
                "simplified to {}, not a UnitMap",
                shape(simp.as_ref())
            ));
        }
        if p.is_inverted() || !q.is_inverted() {
            return Err("simplifying changed the caller's PolyMaps".to_string());
        }
        Ok(())
    }),
    // A TimeMap.
    ("TimeMap from TDB to TCB and back", || {
        let mut map = TimeMap::new();
        map.add(TimeCvtType::TdbToTcb, &[2000.0])
            .map_err(|e| e.to_string())?;
        map.add(TimeCvtType::TcbToTdb, &[2000.0])
            .map_err(|e| e.to_string())?;
        let mjd = [vec![55000.0, 55000.25, 56000.5]];
        check(Box::new(map), "UnitMap", Some(&mjd), Some(&mjd)).map(drop)
    }),
    // MatrixMaps.
    ("inverted full MatrixMap that is diagonal", || {
        let full = " Begin MatrixMap\n Nin = 2\n Invert = 1\n IsA Mapping\n M0 = 2\n \
                    M1 = 0\n M2 = 0\n M3 = 4\n IM0 = 0.5\n IM1 = 0\n IM2 = 0\n \
                    IM3 = 0.25\n Form = \"Full\"\n End MatrixMap\n";
        let map = starlink_ast::from_native_string(full)
            .map_err(|e| e.to_string())?
            .into_mapping()
            .ok_or_else(|| "not a Mapping".to_string())?;
        check(map, "MatrixMap", None, None).map(drop)
    }),
    ("inverted singular MatrixMap, then a MatrixMap", || {
        let map = series(inverted(matrix(2, 2, &SINGULAR)), matrix(2, 2, &GENERAL));
        check(map, "CmpMap series(MatrixMap^-1,MatrixMap)", None, None).map(drop)
    }),
    ("ZoomMap, then an inverted singular MatrixMap", || {
        let map = series(zoom(2, 2.0), inverted(matrix(2, 2, &SINGULAR)));
        check(map, "MatrixMap", None, None).map(drop)
    }),
    ("ShiftMap, then an inverted singular MatrixMap", || {
        let map = series(shift(&[2.0, 3.0]), inverted(matrix(2, 2, &SINGULAR)));
        check(map, "CmpMap series(ShiftMap,MatrixMap^-1)", None, None).map(drop)
    }),
    ("inverted MatrixMap of zeros, then a MatrixMap", || {
        let map = series(
            inverted(diagonal(2, 2, &[0.0, 0.0])),
            matrix(2, 2, &GENERAL),
        );
        check(map, "CmpMap series(MatrixMap^-1,MatrixMap)", None, None).map(drop)
    }),
    ("inverted MatrixMap of zeros, then a ShiftMap", || {
        let map = series(inverted(diagonal(2, 2, &[0.0, 0.0])), shift(&[2.0, 3.0]));
        check(map, "CmpMap series(MatrixMap^-1,ShiftMap)", None, None).map(drop)
    }),
    // WinMaps beside MatrixMaps.
    ("ShiftMap and a unit MatrixMap in parallel", || {
        check(
            parallel(shift(&[3.0]), unit_matrix()),
            "ShiftMap",
            None,
            None,
        )
        .map(drop)
    }),
    ("inverted CmpMap of a unit MatrixMap and a WinMap", || {
        check(
            inverted(series(unit_matrix(), win2())),
            "WinMap",
            None,
            None,
        )
        .map(drop)
    }),
    (
        "WinMap, then a diagonal MatrixMap with fewer outputs",
        || {
            let map = series(win3(), diagonal(3, 2, &DIAG2));
            check(map, "CmpMap series(WinMap,MatrixMap)", None, None).map(drop)
        },
    ),
    (
        "diagonal MatrixMap with more outputs, then a WinMap",
        || {
            let map = series(diagonal(2, 3, &DIAG2), win3());
            check(map, "CmpMap series(MatrixMap,WinMap)", None, None).map(drop)
        },
    ),
    (
        "diagonal MatrixMap with fewer outputs, then a WinMap",
        || {
            let map = series(diagonal(3, 2, &DIAG2), win2());
            check(map, "CmpMap series(MatrixMap,WinMap)", None, None).map(drop)
        },
    ),
    (
        "WinMap, then a diagonal MatrixMap with more outputs",
        || {
            let map = series(win2(), diagonal(2, 3, &DIAG2));
            check(map, "CmpMap series(WinMap,MatrixMap)", None, None).map(drop)
        },
    ),
    (
        "ShiftMap, a MatrixMap that discards an axis, and a ShiftMap",
        || {
            let pair = series(shift(&[1.0, 2.0, 3.0]), diagonal(3, 2, &DIAG2));
            let map = series(pair, shift(&[3.0, 4.0]));
            check(map, "CmpMap series(CmpMap,ShiftMap)", None, None).map(drop)
        },
    ),
    ("WinMap, then two MathMaps in parallel", || {
        let map = series(win_scaled(), parallel(nonlinear(), nonlinear()));
        check(map, "CmpMap series(WinMap,CmpMap)", None, None).map(drop)
    }),
    ("two MathMaps in parallel, then a WinMap", || {
        let map = series(parallel(nonlinear(), nonlinear()), win_scaled());
        check(map, "CmpMap parallel(CmpMap,CmpMap)", None, None).map(drop)
    }),
    ("inverted WinMap with a scale of zero", || {
        let map = inverted(winmap(&[0.0], &[1.0], &[0.0], &[0.0]));
        check(map, "WinMap", None, None).map(drop)
    }),
    // MathMaps that are nearly the same.
    ("the same MathMap twice", || {
        check_equal(mathmap("y=x+1", "x=y-1"), mathmap("y=x+1", "x=y-1"), true)
    }),
    ("MathMaps that add and subtract", || {
        check_equal(mathmap("y=x+1", "x=y-1"), mathmap("y=x-1", "x=y+1"), false)
    }),
    ("MathMaps that multiply and divide", || {
        check_equal(mathmap("y=x*2", "x=y/2"), mathmap("y=x/2", "x=y*2"), false)
    }),
    ("MathMaps that take a sine and a cosine", || {
        check_equal(mathmap("y=sin(x)", "x"), mathmap("y=cos(x)", "x"), false)
    }),
    ("MathMaps that add different constants", || {
        check_equal(mathmap("y=x+1", "x=y-1"), mathmap("y=x+2", "x=y-2"), false)
    }),
    ("MathMaps that take a maximum and a minimum", || {
        check_equal(
            mathmap("y=max(x,1,2)", "x"),
            mathmap("y=min(x,1,2)", "x"),
            false,
        )
    }),
    ("MathMaps that take different maxima", || {
        check_equal(
            mathmap("y=max(x,1,2)", "x"),
            mathmap("y=max(x,1,3)", "x"),
            false,
        )
    }),
    ("MathMaps that take the same maximum", || {
        check_equal(
            mathmap("y=max(x,1,2)", "x"),
            mathmap("y=max(x,1,2)", "x"),
            true,
        )
    }),
    ("a MathMap and the inverse of its opposite", || {
        check_equal(
            mathmap("y=x+1", "x=y-1"),
            inverted(mathmap("y=x-1", "x=y+1")),
            true,
        )
    }),
    ("MathMaps that give the same constant", || {
        check_equal(mathmap("y=<pi>", "x"), mathmap("y=<pi>", "x"), true)
    }),
    ("MathMaps that give different constants", || {
        check_equal(mathmap("y=<pi>", "x"), mathmap("y=<e>", "x"), false)
    }),
    (
        "a MathMap that copies its input and one that gives a constant",
        || check_equal(mathmap("y=x", "x=y"), mathmap("y=<pi>", "x=y"), false),
    ),
    (
        "SwitchMap, then the inverse of a different SwitchMap",
        || {
            let first = switchmap(mathmap("y=x+1", "x=y-1"));
            let second = inverted(switchmap(mathmap("y=x-1", "x=y+1")));
            check(
                series(first, second),
                "CmpMap series(SwitchMap,SwitchMap)",
                None,
                None,
            )
            .map(drop)
        },
    ),
];

const CON3: [f64; 6] = [0.1, 0.2, 5.0, 0.3, 0.4, 7.0];
const GENERAL: [f64; 4] = [1.0, 2.0, 3.0, 4.0];
const SINGULAR: [f64; 4] = [1.0, 2.0, 2.0, 4.0];
const DIAG2: [f64; 2] = [2.0, 3.0];

/// Cases the port does not yet match, each with the gap it exposes. Each is
/// checked to still differ (`excluded_cases_still_differ`), so the list
/// shrinks as the gaps close.
const MAPMERGE_GAP_EXCLUDED: &[(&str, &str)] = &[
    (
        "constant PermMap, then a WcsMap and ZoomMap in parallel",
        "the PermMap swap with parallel Mappings that upstream f8c7723e refuses: the port merges to one PermMap",
    ),
    (
        "constant PermMap with bad inputs, then a WcsMap and ZoomMap in parallel",
        "the PermMap swap with parallel Mappings that upstream f8c7723e refuses: the port merges to one PermMap",
    ),
    (
        "constant PermMap, then a ZoomMap and WcsMap in parallel",
        "the PermMap swap with parallel Mappings that upstream f8c7723e refuses: the port's inverse loses coordinate 2",
    ),
    (
        "constant PermMap, then two WcsMaps in parallel",
        "the PermMap swap with parallel Mappings that upstream f8c7723e refuses: the port merges to one PermMap",
    ),
    (
        "PermMap whose inverse copies a constant output, then two MathMaps in parallel",
        "the PermMap swap with parallel Mappings that upstream f8c7723e refuses: the port copies an output index into the new inperm",
    ),
    (
        "inverted full MatrixMap that is diagonal",
        "MatrixMap and WinMap merges that upstream b9cf680b limits to equivalent, usable results: the port stores the diagonal replacement uninverted",
    ),
    (
        "inverted singular MatrixMap, then a MatrixMap",
        "MatrixMap and WinMap merges that upstream b9cf680b limits to equivalent, usable results: the port multiplies a matrix with no forward transformation",
    ),
    (
        "ShiftMap, then an inverted singular MatrixMap",
        "MatrixMap and WinMap merges that upstream b9cf680b limits to equivalent, usable results: the port multiplies a matrix with no forward transformation",
    ),
    (
        "inverted MatrixMap of zeros, then a MatrixMap",
        "MatrixMap and WinMap merges that upstream b9cf680b limits to equivalent, usable results: the port merges where C keeps the CmpMap",
    ),
    (
        "inverted MatrixMap of zeros, then a ShiftMap",
        "MatrixMap and WinMap merges that upstream b9cf680b limits to equivalent, usable results: the port's WinMap has no usable scales",
    ),
    (
        "ShiftMap, a MatrixMap that discards an axis, and a ShiftMap",
        "MatrixMap and WinMap merges that upstream b9cf680b limits to equivalent, usable results: the port moves a WinMap past a MatrixMap that discards its axis",
    ),
    (
        "inverted WinMap with a scale of zero",
        "MatrixMap and WinMap merges that upstream b9cf680b limits to equivalent, usable results: the port builds a matrix with no usable element",
    ),
];

fn excluded(name: &str) -> bool {
    MAPMERGE_GAP_EXCLUDED.iter().any(|(n, _)| *n == name)
}

#[test]
fn simplifying_does_not_change_a_mapping() {
    let failures: Vec<String> = CASES
        .iter()
        .filter(|(name, _)| !excluded(name))
        .filter_map(|(name, case)| case().err().map(|e| format!("{name}: {e}")))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn excluded_cases_still_differ() {
    for (name, reason) in MAPMERGE_GAP_EXCLUDED {
        let (_, case) = CASES
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("{name} is excluded but is not a case"));
        assert!(
            case().is_err(),
            "{name} now matches C; remove it from MAPMERGE_GAP_EXCLUDED. Reason on record: {reason}"
        );
    }
}
