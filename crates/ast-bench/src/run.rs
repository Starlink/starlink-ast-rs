//! Run a set of cases on the chosen engines.

//! Run a set of cases on the chosen engines.

use std::time::Duration;

use ast_core::constants::BAD_VALUE;
use ast_core::coords::{PointSet, Points, PointsMut};
use ast_core::error::AstResult;
use ast_core::mappings::Mapping;

use crate::cases::Case;
use crate::load::{Loaded, load_fixture};
use crate::points::{rows_to_columns, rows_to_pointset, select_rows};
use crate::report::Row;
use crate::timing::{Op, measure};

/// Which engines to time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engines {
    /// The port only.
    Rust,
    /// The reference C library only.
    C,
    /// Both, alternating.
    Both,
}

/// An error if this build counts library calls (`ast-core`'s `call-counts`
/// feature, which `ast-oracle-tests` enables): timing it would measure the
/// counters.
pub fn check_not_counting() -> Result<(), String> {
    if ast_core::CALL_COUNTS_ENABLED {
        return Err(
            "this build counts library calls (call-counts); build ast-bench on its own".into(),
        );
    }
    Ok(())
}

/// The time budget per case, direction, point count and engine.
fn budget(n: usize) -> Duration {
    if n >= 1_000_000 {
        Duration::from_secs(1)
    } else {
        Duration::from_millis(200)
    }
}

/// The fewest samples taken of any timing.
const MIN_SAMPLES: usize = 7;

/// Transform `pts` by the NaN protocol (`transform_nan_protocol`), doing its
/// boundary work in caller-owned buffers: BAD and non-finite inputs become
/// NaN in `nan_in`, the Mapping's `transform_into_nan` writes `out`, and
/// non-finite outputs become BAD. This is what an entry point using the
/// protocol would cost per call once its buffers exist.
pub fn nan_edges(
    map: &dyn Mapping,
    pts: &PointSet,
    forward: bool,
    nan_in: &mut PointSet,
    out: &mut PointSet,
) -> AstResult<()> {
    for j in 0..pts.ncoord() {
        for (d, &v) in nan_in.axis_mut(j).iter_mut().zip(pts.axis(j)) {
            *d = if v == BAD_VALUE || !v.is_finite() {
                f64::NAN
            } else {
                v
            };
        }
    }
    map.transform_into_nan(nan_in.as_points(), forward, out.as_points_mut())?;
    for j in 0..out.ncoord() {
        for v in out.axis_mut(j) {
            if !v.is_finite() {
                *v = BAD_VALUE;
            }
        }
    }
    Ok(())
}

/// Time every case at every point count on `engines`. A case that cannot be
/// timed gives an `Err` naming it and the point count; the rest still run.
pub fn run_set(cases: &[Case], engines: Engines, npoints: &[usize]) -> Vec<Result<Row, String>> {
    run_set_with(cases, engines, npoints, false)
}

/// [`run_set`], also timing the port through [`nan_edges`] as `rust-nan`
/// when `nan` is true and the port is timed.
pub fn run_set_with(
    cases: &[Case],
    engines: Engines,
    npoints: &[usize],
    nan: bool,
) -> Vec<Result<Row, String>> {
    #[cfg(feature = "c-oracle")]
    let c_engine = match engines {
        Engines::Rust => None,
        _ => Some(crate::c_engine::CEngine::open(
            crate::c_engine::Variant::Bench,
        )),
    };
    let mut out = Vec::new();
    for case in cases {
        let s = &case.section;
        let dir = if s.forward { "forward" } else { "inverse" };
        let rust = match load_fixture(&case.root, &s.relpath) {
            Ok(Loaded::Mapping(obj)) => obj,
            Ok(Loaded::Untransformable(why)) => {
                out.push(Err(format!("{} {dir}: {why}", s.relpath)));
                continue;
            }
            Err(e) => {
                out.push(Err(format!("{} {dir}: {e}", s.relpath)));
                continue;
            }
        };
        let map = rust.as_mapping().expect("loaded as a Mapping");
        #[cfg(feature = "c-oracle")]
        let c_map = match &c_engine {
            None => None,
            Some(Err(e)) => {
                out.push(Err(format!("{} {dir}: {e}", s.relpath)));
                continue;
            }
            Some(Ok(eng)) => match eng.load(&case.root, &s.relpath) {
                Ok(m) => Some(m),
                Err(e) => {
                    out.push(Err(format!("{} {dir}: {e}", s.relpath)));
                    continue;
                }
            },
        };
        #[cfg(not(feature = "c-oracle"))]
        if engines != Engines::Rust {
            out.push(Err(format!(
                "{} {dir}: timing C needs ast-bench's c-oracle feature",
                s.relpath
            )));
            continue;
        }
        for &n in npoints {
            let rows = match select_rows(s, n) {
                Ok(r) => r,
                Err(e) => {
                    out.push(Err(e));
                    continue;
                }
            };
            let pts = rows_to_pointset(s, &rows);
            let mut rust_out = PointSet::zeros(n, s.nout);
            let mut time_rust = || -> Result<(), String> {
                map.transform_into(pts.as_points(), s.forward, rust_out.as_points_mut())
                    .map_err(|e| format!("{} {dir} N={n}: {e}", s.relpath))
            };
            let columns = rows_to_columns(s, &rows);
            // A caller holding one array per axis, as C's astTranN takes
            // them: any conversion the port needs is inside the timing.
            let mut cols_out = vec![0.0; s.nout * n];
            let mut time_cols = || -> Result<(), String> {
                let fail = |e: ast_core::error::AstError| format!("{} {dir} N={n}: {e}", s.relpath);
                let input = Points::new(columns.chunks_exact(n)).map_err(fail)?;
                let out = PointsMut::new(cols_out.chunks_exact_mut(n)).map_err(fail)?;
                map.transform_into(input, s.forward, out).map_err(fail)
            };
            let mut nan_in = PointSet::zeros(n, s.nin);
            let mut nan_out = PointSet::zeros(n, s.nout);
            let mut time_nan = || -> Result<(), String> {
                nan_edges(map, &pts, s.forward, &mut nan_in, &mut nan_out)
                    .map_err(|e| format!("{} {dir} N={n}: {e}", s.relpath))
            };
            #[cfg(feature = "c-oracle")]
            let mut c_out = vec![0.0; s.nout * n];
            #[cfg(feature = "c-oracle")]
            let mut time_c = || -> Result<(), String> {
                c_map
                    .as_ref()
                    .expect("loaded when C is timed")
                    .tran(s.forward, n, s.nin, &columns, s.nout, &mut c_out)
                    .map_err(|e| format!("{} {dir} N={n}: {e}", s.relpath))
            };
            let (mut ops, mut names): (Vec<Op<'_>>, Vec<&str>) = match engines {
                Engines::Rust => (
                    vec![&mut time_rust, &mut time_cols],
                    vec!["rust", "rust-cols"],
                ),
                #[cfg(feature = "c-oracle")]
                Engines::C => (vec![&mut time_c], vec!["c"]),
                #[cfg(feature = "c-oracle")]
                Engines::Both => (
                    vec![&mut time_rust, &mut time_cols, &mut time_c],
                    vec!["rust", "rust-cols", "c"],
                ),
                #[cfg(not(feature = "c-oracle"))]
                _ => unreachable!("refused above"),
            };
            if nan && engines != Engines::C {
                ops.push(&mut time_nan);
                names.push("rust-nan");
            }
            match measure(budget(n), MIN_SAMPLES, n, &mut ops) {
                Ok(stats) => {
                    for (engine, st) in names.iter().zip(stats) {
                        out.push(Ok(Row {
                            case: s.relpath.clone(),
                            dir: dir.to_string(),
                            n,
                            engine: engine.to_string(),
                            stats: st,
                        }));
                    }
                }
                Err(e) => out.push(Err(e)),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cases::load_set;

    #[test]
    fn the_rust_engine_also_times_a_c_shaped_caller() {
        let cases = load_set("bench").unwrap();
        let zoom: Vec<_> = cases
            .into_iter()
            .filter(|c| c.section.relpath == "zoommap_2d.map" && c.section.forward)
            .collect();
        let rows: Vec<_> = run_set(&zoom, Engines::Rust, &[10])
            .into_iter()
            .map(Result::unwrap)
            .collect();
        let engines: Vec<&str> = rows.iter().map(|r| r.engine.as_str()).collect();
        assert_eq!(engines, ["rust", "rust-cols"]);
    }

    #[test]
    fn every_named_set_times_at_one_point() {
        for entry in std::fs::read_dir(crate::cases::SETS).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let cases = load_set(&name).unwrap();
            let errors: Vec<String> = run_set(&cases, Engines::Rust, &[1])
                .into_iter()
                .filter_map(Result::err)
                .collect();
            assert!(errors.is_empty(), "set {name}: {errors:?}");
        }
    }

    #[test]
    fn a_failing_case_is_reported_not_fatal() {
        let mut cases = load_set("bench").unwrap();
        cases.truncate(1);
        let mut bad = cases[0].clone();
        bad.section.relpath = "missing.map".into();
        cases.push(bad);
        let rows = run_set(&cases, Engines::Rust, &[1]);
        assert!(rows[0].is_ok());
        assert!(
            rows.last()
                .unwrap()
                .as_ref()
                .unwrap_err()
                .contains("missing.map")
        );
    }

    #[test]
    fn the_nan_engine_runs_only_when_asked() {
        let cases = load_set("bench").unwrap();
        let zoom: Vec<_> = cases
            .into_iter()
            .filter(|c| c.section.relpath == "zoommap_2d.map" && c.section.forward)
            .collect();
        let engines = |nan| -> Vec<String> {
            run_set_with(&zoom, Engines::Rust, &[10], nan)
                .into_iter()
                .map(|r| r.unwrap().engine)
                .collect()
        };
        assert_eq!(engines(false), ["rust", "rust-cols"]);
        assert_eq!(engines(true), ["rust", "rust-cols", "rust-nan"]);
    }

    #[test]
    fn nan_edges_gives_the_nan_protocols_bits() {
        let mut checked = 0;
        for case in crate::sweep::all_cases().unwrap() {
            let s = &case.section;
            let Ok(Loaded::Mapping(obj)) = load_fixture(&case.root, &s.relpath) else {
                continue;
            };
            let map = obj.as_mapping().unwrap();
            let rows: Vec<usize> = (0..s.npoint()).collect();
            let pts = rows_to_pointset(s, &rows);
            let mut nan_in = PointSet::zeros(pts.npoint(), s.nin);
            let mut out = PointSet::zeros(pts.npoint(), s.nout);
            let edges = nan_edges(map, &pts, s.forward, &mut nan_in, &mut out);
            match map.transform_nan_protocol(&pts, s.forward) {
                Ok(want) => {
                    edges.unwrap_or_else(|e| panic!("{} {}: {e}", s.relpath, s.forward));
                    let bits =
                        |p: &PointSet| p.view().iter().map(|v| v.to_bits()).collect::<Vec<_>>();
                    assert_eq!(
                        bits(&out),
                        bits(&want),
                        "{} forward={}",
                        s.relpath,
                        s.forward
                    );
                }
                Err(e) => assert_eq!(
                    edges.map_err(|e| e.code()).err(),
                    Some(e.code()),
                    "{} forward={}",
                    s.relpath,
                    s.forward
                ),
            }
            checked += 1;
        }
        assert!(checked > 2000, "only {checked} sections compared");
    }

    #[test]
    fn transforms_leave_surplus_output_points_and_axes_alone() {
        // Every corpus transform, through both entry points, into an output
        // one point and one axis larger than it needs, pre-filled with BAD so
        // that a BAD-to-NaN pass over the surplus would show. The NaN
        // protocol's input has its bad values as NaN, as `nan_edges` gives.
        let mut checked = 0;
        for case in crate::sweep::all_cases().unwrap() {
            let s = &case.section;
            let Ok(Loaded::Mapping(obj)) = load_fixture(&case.root, &s.relpath) else {
                continue;
            };
            let map = obj.as_mapping().unwrap();
            let rows: Vec<usize> = (0..s.npoint()).collect();
            let pts = rows_to_pointset(s, &rows);
            let n = pts.npoint();
            let mut nan_pts = PointSet::zeros(n, s.nin);
            for j in 0..s.nin {
                for (d, &v) in nan_pts.axis_mut(j).iter_mut().zip(pts.axis(j)) {
                    *d = if v == BAD_VALUE || !v.is_finite() {
                        f64::NAN
                    } else {
                        v
                    };
                }
            }
            for nan in [false, true] {
                let mut out = vec![vec![BAD_VALUE; n + 1]; s.nout + 1];
                let view = PointsMut::new(out.iter_mut().map(|a| &mut a[..])).unwrap();
                let r = if nan {
                    map.transform_into_nan(nan_pts.as_points(), s.forward, view)
                } else {
                    map.transform_into(pts.as_points(), s.forward, view)
                };
                if r.is_err() {
                    continue;
                }
                let surplus_point = out[..s.nout].iter().map(|a| a[n]);
                let surplus_axis = out[s.nout].iter().copied();
                assert!(
                    surplus_point
                        .chain(surplus_axis)
                        .all(|v| v.to_bits() == BAD_VALUE.to_bits()),
                    "{} forward={} nan={nan}: surplus output written",
                    s.relpath,
                    s.forward
                );
                checked += 1;
            }
        }
        assert!(checked > 4000, "only {checked} transforms checked");
    }

    #[test]
    fn a_timed_build_does_not_count_calls() {
        check_not_counting().unwrap();
    }
}
