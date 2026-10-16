//! Benchmark results: one row per case, direction, point count and engine,
//! printed as a table or as TSV, and read back from TSV.

//! Benchmark results: one row per case, direction, point count and engine,
//! printed as a table or as TSV, and read back from TSV.

use std::collections::BTreeMap;

use crate::timing::Stats;

/// One timing.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The fixture's relpath.
    pub case: String,
    /// `forward` or `inverse`.
    pub dir: String,
    /// The number of points.
    pub n: usize,
    /// `rust` or `c`.
    pub engine: String,
    /// The timing.
    pub stats: Stats,
}

const TSV_HEADER: &str = "case\tdir\tn\tengine\tmedian_ns\tmin_ns\tsamples";

/// The rows as TSV, with a header line; values are written in full so
/// [`parse_tsv`] reads them back exactly.
pub fn tsv(rows: &[Row]) -> String {
    let mut out = format!("{TSV_HEADER}\n");
    for r in rows {
        out += &format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            r.case,
            r.dir,
            r.n,
            r.engine,
            r.stats.median_ns_per_point,
            r.stats.min_ns_per_point,
            r.stats.samples
        );
    }
    out
}

/// Rows from [`tsv`] output; lines starting `#` and the header are skipped.
pub fn parse_tsv(text: &str) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') || line == TSV_HEADER {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        let [case, dir, n, engine, median, min, samples] = f[..] else {
            return Err(format!("bad TSV line {line:?}"));
        };
        let num = |s: &str| s.parse::<f64>().map_err(|e| format!("{line:?}: {e}"));
        rows.push(Row {
            case: case.to_string(),
            dir: dir.to_string(),
            n: n.parse().map_err(|e| format!("{line:?}: {e}"))?,
            engine: engine.to_string(),
            stats: Stats {
                median_ns_per_point: num(median)?,
                min_ns_per_point: num(min)?,
                samples: samples.parse().map_err(|e| format!("{line:?}: {e}"))?,
            },
        });
    }
    Ok(rows)
}

/// The rows as a table, one line per case, direction and point count, with
/// the median ns per point of each engine (`rust-cols` in the `cols` column)
/// and, when Rust and C both ran, Rust / C.
pub fn table(rows: &[Row]) -> String {
    type Key = (String, String, usize);
    // The Rust, Rust-from-columns and C medians.
    type Medians = (Option<f64>, Option<f64>, Option<f64>);
    let mut by: BTreeMap<Key, Medians> = BTreeMap::new();
    for r in rows {
        let e = by.entry((r.case.clone(), r.dir.clone(), r.n)).or_default();
        let v = Some(r.stats.median_ns_per_point);
        match r.engine.as_str() {
            "rust" => e.0 = v,
            "rust-cols" => e.1 = v,
            "c" => e.2 = v,
            other => unreachable!("unknown engine {other}"),
        }
    }
    let mut out = format!(
        "{:<32} {:<8} {:>8} {:>12} {:>12} {:>12} {:>8}\n",
        "case", "dir", "n", "rust ns/pt", "cols ns/pt", "c ns/pt", "rust/c"
    );
    let cell = |v: Option<f64>| v.map_or(String::from("-"), |v| format!("{v:.2}"));
    for ((case, dir, n), (rust, cols, c)) in by {
        let ratio = match (rust, c) {
            (Some(r), Some(c)) if c > 0.0 => format!("{:.2}", r / c),
            _ => String::from("-"),
        };
        out += &format!(
            "{case:<32} {dir:<8} {n:>8} {:>12} {:>12} {:>12} {ratio:>8}\n",
            cell(rust),
            cell(cols),
            cell(c)
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsv_round_trips() {
        let rows = vec![Row {
            case: "zoommap_2d.map".into(),
            dir: "forward".into(),
            n: 100,
            engine: "rust".into(),
            stats: crate::timing::Stats {
                median_ns_per_point: 1.5,
                min_ns_per_point: 1.25,
                samples: 7,
            },
        }];
        assert_eq!(parse_tsv(&tsv(&rows)).unwrap(), rows);
    }

    #[test]
    fn table_shows_the_ratio_when_both_engines_ran() {
        let row = |engine: &str, ns: f64| Row {
            case: "a.map".into(),
            dir: "forward".into(),
            n: 1,
            engine: engine.into(),
            stats: crate::timing::Stats {
                median_ns_per_point: ns,
                min_ns_per_point: ns,
                samples: 7,
            },
        };
        let t = table(&[row("rust", 30.0), row("c", 20.0)]);
        assert!(t.contains("1.50"), "{t}");
    }

    #[test]
    fn table_shows_the_c_shaped_timing_in_its_own_column() {
        let row = |engine: &str, ns: f64| Row {
            case: "a.map".into(),
            dir: "forward".into(),
            n: 10,
            engine: engine.into(),
            stats: crate::timing::Stats {
                median_ns_per_point: ns,
                min_ns_per_point: ns,
                samples: 7,
            },
        };
        let t = table(&[row("rust", 1.0), row("rust-cols", 4.0), row("c", 2.0)]);
        assert!(t.contains("cols ns/pt"), "{t}");
        let line = t.lines().nth(1).unwrap();
        assert!(line.contains("4.00") && line.contains("2.00"), "{t}");
    }
}
