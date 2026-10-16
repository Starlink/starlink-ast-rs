//! Committed records of Rust/C timings (`docs/perf/`), and the advisory
//! warning when a ratio has got worse than the latest record's.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::compare::SLOWDOWN_THRESHOLD;
use crate::fingerprint::Fingerprint;
use crate::report::{Row, parse_tsv, tsv};

/// Where the records are kept.
pub fn perf_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/perf")
}

/// The file name of the record of `set` made on `date` on this platform.
pub fn record_name(date: &str, set: &str) -> String {
    format!(
        "{date}-{}-{}-{set}.tsv",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// Today's date, `YYYY-MM-DD`, in UTC.
pub fn today() -> String {
    std::process::Command::new("date")
        .args(["-u", "+%F"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "undated".into())
}

/// Write the record of `set` with a header naming the commit, date, machine,
/// compilers and C build.
pub fn write_record(set: &str, fp: &Fingerprint, rows: &[Row]) -> Result<PathBuf, String> {
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short=8", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    let date = today();
    let f = |k: &str| fp.fields.get(k).cloned().unwrap_or_default();
    let header = format!(
        "# commit={commit} date={date} set={set} engines=rust,c\n# machine={}; {}\n\
         # rustc={} profile={}\n# c={}\n",
        f("cpu"),
        f("os"),
        f("rustc"),
        f("profile"),
        f("c")
    );
    let path = perf_dir().join(record_name(&date, set));
    std::fs::write(&path, header + &tsv(rows)).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// The newest record of `set` for this platform, if there is one.
pub fn latest_record(set: &str) -> Option<(PathBuf, Vec<Row>)> {
    let suffix = format!(
        "-{}-{}-{set}.tsv",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let mut names: Vec<PathBuf> = std::fs::read_dir(perf_dir())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(&suffix))
        })
        .collect();
    names.sort();
    let path = names.pop()?;
    let rows = parse_tsv(&std::fs::read_to_string(&path).ok()?).ok()?;
    Some((path, rows))
}

/// Rust over C fastest-sample time per case, direction and point count.
fn ratios(rows: &[Row]) -> HashMap<(String, String, usize), f64> {
    let mut c: HashMap<(String, String, usize), f64> = HashMap::new();
    for r in rows.iter().filter(|r| r.engine == "c") {
        c.insert(
            (r.case.clone(), r.dir.clone(), r.n),
            r.stats.min_ns_per_point,
        );
    }
    rows.iter()
        .filter(|r| r.engine == "rust")
        .filter_map(|r| {
            let k = (r.case.clone(), r.dir.clone(), r.n);
            let cv = *c.get(&k)?;
            (cv > 0.0).then(|| (k, r.stats.min_ns_per_point / cv))
        })
        .collect()
}

/// One line per timing whose Rust/C ratio in `current` is worse than in
/// `record` by more than [`SLOWDOWN_THRESHOLD`]. Advisory: the ratio moves
/// with the machine as well as the code.
pub fn ratio_warnings(record: &[Row], current: &[Row]) -> Vec<String> {
    let was = ratios(record);
    let mut out: Vec<String> = ratios(current)
        .into_iter()
        .filter_map(|(k, now)| {
            let w = *was.get(&k)?;
            (now / w - 1.0 > SLOWDOWN_THRESHOLD)
                .then(|| format!("{} {} N={}: Rust/C {w:.2} -> {now:.2}", k.0, k.1, k.2))
        })
        .collect();
    out.sort();
    out
}

/// The `count` largest Rust/C ratios in `rows`, largest first, each with
/// its Rust row.
pub fn worst_ratios(rows: &[Row], count: usize) -> Vec<(f64, &Row)> {
    let r = ratios(rows);
    let mut out: Vec<(f64, &Row)> = rows
        .iter()
        .filter(|row| row.engine == "rust")
        .filter_map(|row| Some((*r.get(&(row.case.clone(), row.dir.clone(), row.n))?, row)))
        .collect();
    out.sort_by(|a, b| b.0.total_cmp(&a.0));
    out.truncate(count);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::Row;
    use crate::timing::Stats;

    fn row(engine: &str, ns: f64) -> Row {
        Row {
            case: "a.map".into(),
            dir: "forward".into(),
            n: 100,
            engine: engine.into(),
            stats: Stats {
                median_ns_per_point: ns,
                min_ns_per_point: ns,
                samples: 7,
            },
        }
    }

    #[test]
    fn ratio_warning_is_advisory_and_thresholded() {
        let record = vec![row("rust", 10.0), row("c", 10.0)];
        // 1.05: within the threshold, no warning.
        assert!(ratio_warnings(&record, &[row("rust", 10.5), row("c", 10.0)]).is_empty());
        // 1.5: worse than the record by more than the threshold.
        let w = ratio_warnings(&record, &[row("rust", 15.0), row("c", 10.0)]);
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("a.map"), "{w:?}");
    }

    #[test]
    fn worst_ratios_are_largest_first() {
        let mut rows = vec![row("rust", 30.0), row("c", 10.0)];
        let mut b = vec![row("rust", 20.0), row("c", 10.0)];
        for r in &mut b {
            r.case = "b.map".into();
        }
        rows.extend(b);
        let w = worst_ratios(&rows, 1);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].1.case, "a.map");
        assert!((w[0].0 - 3.0).abs() < 1e-12);
    }

    #[test]
    fn record_name_is_dated_and_platform_tagged() {
        let n = record_name("2026-10-15", "bench");
        assert!(
            n.starts_with("2026-10-15-") && n.ends_with("-bench.tsv"),
            "{n}"
        );
        assert!(
            n.contains(std::env::consts::OS) && n.contains(std::env::consts::ARCH),
            "{n}"
        );
    }
}
