//! Compare a run with an earlier one and report what got slower.

//! Compare a run with an earlier one and report what got slower.

use std::path::{Path, PathBuf};

use crate::fingerprint::Fingerprint;
use crate::report::{Row, parse_tsv, tsv};

/// A timing more than this fraction slower than its baseline is a slowdown.
/// Timings are compared by their fastest sample, which other work on the
/// machine can only lengthen; on a quiet machine a build's fastest samples
/// agree with themselves to within about 5% between runs.
pub const SLOWDOWN_THRESHOLD: f64 = 0.10;

/// Cases allowed a larger threshold, each with the reason:
/// `(case, threshold, reason)`. Counted in `PLAN.md` §6.
pub const SLOWDOWN_LICENSED: &[(&str, f64, &str)] = &[];

/// How many times a suspected slowdown is timed again before it is
/// reported: it is reported only if every re-timing is slow too, as noise
/// rarely repeats and a real slowdown always does.
pub const CONFIRM_RUNS: usize = 2;

/// One line per timing in `cur` slower than the same timing in `base` by more
/// than its threshold.
pub fn slowdowns(base: &[Row], cur: &[Row]) -> Vec<String> {
    slow_rows(base, cur)
        .into_iter()
        .map(|(was, c)| describe(was, c))
        .collect()
}

/// A slowdown as one line.
pub fn describe(was: f64, c: &Row) -> String {
    let now = c.stats.min_ns_per_point;
    format!(
        "{} {} N={} {}: {was:.2} -> {now:.2} ns/pt (+{:.0}%)",
        c.case,
        c.dir,
        c.n,
        c.engine,
        100.0 * (now / was - 1.0)
    )
}

/// Each timing in `cur` slower than the same timing in `base` by more than
/// its threshold, with the baseline's time.
pub fn slow_rows<'a>(base: &[Row], cur: &'a [Row]) -> Vec<(f64, &'a Row)> {
    let mut out = Vec::new();
    for c in cur {
        let Some(b) = base
            .iter()
            .find(|b| b.case == c.case && b.dir == c.dir && b.n == c.n && b.engine == c.engine)
        else {
            continue;
        };
        let threshold = SLOWDOWN_LICENSED
            .iter()
            .find(|(case, ..)| *case == c.case)
            .map_or(SLOWDOWN_THRESHOLD, |&(_, t, _)| t);
        let (was, now) = (b.stats.min_ns_per_point, c.stats.min_ns_per_point);
        if was > 0.0 && now / was - 1.0 > threshold {
            out.push((was, c));
        }
    }
    out
}

/// Each timing present in only one of `base` and `cur`: a case that stopped
/// (or started) timing cannot be compared, so it is reported rather than
/// skipped.
pub fn unmatched_rows(base: &[Row], cur: &[Row]) -> Vec<String> {
    let key = |r: &Row| (r.case.clone(), r.dir.clone(), r.n, r.engine.clone());
    let b: std::collections::BTreeSet<_> = base.iter().map(key).collect();
    let c: std::collections::BTreeSet<_> = cur.iter().map(key).collect();
    let name = |(case, dir, n, engine): &(String, String, usize, String)| {
        format!("{case} {dir} N={n} {engine}")
    };
    b.difference(&c)
        .map(|k| format!("only in the base: {}", name(k)))
        .chain(
            c.difference(&b)
                .map(|k| format!("only in the current build: {}", name(k))),
        )
        .collect()
}

/// The suspects that stay slower than `base` through `rounds` re-timings by
/// `retime`, which returns the fresh rows for one suspect. A re-timing that
/// fails is an error: it cannot clear a suspect.
pub fn confirm_slowdowns(
    base: &[Row],
    mut suspects: Vec<Row>,
    rounds: usize,
    mut retime: impl FnMut(&Row) -> Result<Vec<Row>, String>,
) -> Result<Vec<Row>, String> {
    for _ in 0..rounds {
        if suspects.is_empty() {
            break;
        }
        let mut again = Vec::new();
        for s in &suspects {
            let rows = retime(s)?;
            if !rows.iter().any(|r| r.engine == s.engine) {
                return Err(format!(
                    "re-timing {} {} N={} gave no {} row",
                    s.case, s.dir, s.n, s.engine
                ));
            }
            again.extend(rows.into_iter().filter(|r| r.engine == s.engine));
        }
        suspects = slow_rows(base, &again)
            .into_iter()
            .map(|(_, r)| r.clone())
            .collect();
    }
    Ok(suspects)
}

/// An error naming what differs when a baseline was saved under another
/// fingerprint.
pub fn check_same_machine(saved: &Fingerprint, current: &Fingerprint) -> Result<(), String> {
    let diff = saved.diff(current);
    if diff.is_empty() {
        return Ok(());
    }
    Err(format!(
        "the baseline was saved on a different machine or toolchain ({}); re-run with --save",
        diff.join(", ")
    ))
}

/// Where baselines are kept: one TSV per set, each beside the fingerprint of
/// the machine and toolchain that saved it.
fn baseline_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/perf/baselines")
}

/// Save `rows` as the baseline of `set` for this fingerprint.
pub fn save_baseline(set: &str, fp: &Fingerprint, rows: &[Row]) -> Result<PathBuf, String> {
    save_baseline_in(&baseline_dir(), set, fp, rows)
}

/// The saved baseline of `set`, refused if it was saved under another
/// fingerprint.
pub fn load_baseline(set: &str, fp: &Fingerprint) -> Result<Vec<Row>, String> {
    load_baseline_in(&baseline_dir(), set, fp)
}

/// [`save_baseline`] under `dir`.
pub fn save_baseline_in(
    dir: &Path,
    set: &str,
    fp: &Fingerprint,
    rows: &[Row],
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::write(dir.join(format!("{set}.fingerprint")), fp.to_text())
        .map_err(|e| e.to_string())?;
    let file = dir.join(format!("{set}.tsv"));
    std::fs::write(&file, tsv(rows)).map_err(|e| e.to_string())?;
    Ok(file)
}

/// [`load_baseline`] under `dir`.
pub fn load_baseline_in(dir: &Path, set: &str, fp: &Fingerprint) -> Result<Vec<Row>, String> {
    let missing = || format!("no {set} baseline; run `ast-bench run --set {set} --save` first");
    let saved =
        std::fs::read_to_string(dir.join(format!("{set}.fingerprint"))).map_err(|_| missing())?;
    check_same_machine(&Fingerprint::parse(&saved), fp)?;
    let text = std::fs::read_to_string(dir.join(format!("{set}.tsv"))).map_err(|_| missing())?;
    parse_tsv(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::Fingerprint;
    use crate::report::Row;
    use crate::timing::Stats;

    fn row(case: &str, min: f64) -> Row {
        Row {
            case: case.into(),
            dir: "forward".into(),
            n: 1,
            engine: "rust".into(),
            stats: Stats {
                median_ns_per_point: min,
                min_ns_per_point: min,
                samples: 7,
            },
        }
    }

    #[test]
    fn flags_only_slowdowns_over_threshold() {
        let base = vec![row("a.map", 100.0), row("b.map", 100.0)];
        let cur = vec![row("a.map", 109.0), row("b.map", 115.0)];
        let s = slowdowns(&base, &cur);
        assert_eq!(s.len(), 1);
        assert!(s[0].contains("b.map"), "{s:?}");
    }

    #[test]
    fn refuses_a_baseline_from_another_machine() {
        let mut saved = Fingerprint::current(None);
        saved.fields.insert("cpu".into(), "some other cpu".into());
        let e = check_same_machine(&saved, &Fingerprint::current(None)).unwrap_err();
        assert!(e.contains("cpu") && e.contains("--save"), "{e}");
    }

    #[test]
    fn a_row_on_one_side_only_is_reported() {
        let base = vec![row("a.map", 100.0), row("gone.map", 100.0)];
        let cur = vec![row("a.map", 100.0), row("new.map", 100.0)];
        let u = unmatched_rows(&base, &cur);
        assert_eq!(u.len(), 2, "{u:?}");
        assert!(
            u.iter().any(|s| s.contains("gone.map")) && u.iter().any(|s| s.contains("new.map"))
        );
        assert!(unmatched_rows(&base, &base).is_empty());
    }

    #[test]
    fn a_baseline_saved_under_another_toolchain_names_the_field() {
        let root = std::env::temp_dir().join(format!("ast-bench-baseline-{}", std::process::id()));
        let mut saved = Fingerprint::current(None);
        saved.fields.insert("rustc".into(), "rustc 0.0.0".into());
        save_baseline_in(&root, "s", &saved, &[row("a.map", 1.0)]).unwrap();
        let e = load_baseline_in(&root, "s", &Fingerprint::current(None)).unwrap_err();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(e.contains("rustc"), "{e}");
    }

    #[test]
    fn a_retiming_error_is_an_error_not_a_pass() {
        let base = vec![row("a.map", 100.0)];
        let suspects = vec![row("a.map", 200.0)];
        let e = confirm_slowdowns(&base, suspects.clone(), 2, |_| Err("boom".into())).unwrap_err();
        assert!(e.contains("boom"), "{e}");
        let still = confirm_slowdowns(&base, suspects, 2, |r| Ok(vec![r.clone()])).unwrap();
        assert_eq!(still.len(), 1);
    }
}
