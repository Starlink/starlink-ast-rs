//! Run a set of cases on the chosen engines.

//! Run a set of cases on the chosen engines.

use std::time::Duration;

use ast_core::coords::PointSet;

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

/// Time every case at every point count on `engines`. A case that cannot be
/// timed gives an `Err` naming it and the point count; the rest still run.
pub fn run_set(cases: &[Case], engines: Engines, npoints: &[usize]) -> Vec<Result<Row, String>> {
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
                map.transform_into(&pts, s.forward, &mut rust_out)
                    .map_err(|e| format!("{} {dir} N={n}: {e}", s.relpath))
            };
            let columns = rows_to_columns(s, &rows);
            // A caller holding one array per axis, as C's astTranN takes
            // them: any conversion the port needs is inside the timing.
            let mut cols_out = vec![0.0; s.nout * n];
            let mut time_cols = || -> Result<(), String> {
                let fail = |e: ast_core::error::AstError| format!("{} {dir} N={n}: {e}", s.relpath);
                let axes: Vec<&[f64]> = columns.chunks_exact(n).collect();
                let pts = PointSet::from_columns(&axes).map_err(fail)?;
                let mut o = PointSet::zeros(n, s.nout);
                map.transform_into(&pts, s.forward, &mut o).map_err(fail)?;
                for (j, dst) in cols_out.chunks_exact_mut(n).enumerate() {
                    for (d, v) in dst.iter_mut().zip(o.axis_iter(j)) {
                        *d = v;
                    }
                }
                Ok(())
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
            let (mut ops, names): (Vec<Op<'_>>, Vec<&str>) = match engines {
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
    fn a_timed_build_does_not_count_calls() {
        check_not_counting().unwrap();
    }
}
