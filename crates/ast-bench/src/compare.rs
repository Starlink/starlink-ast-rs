//! Compare a run with an earlier one and report what got slower.

//! Compare a run with an earlier one and report what got slower.

use std::path::PathBuf;

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

/// Where baselines are kept, one directory per fingerprint.
fn baseline_dir(fp: &Fingerprint) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/perf/baselines")
        .join(fp.id())
}

/// Save `rows` as the baseline of `set` for this fingerprint.
pub fn save_baseline(set: &str, fp: &Fingerprint, rows: &[Row]) -> Result<PathBuf, String> {
    let dir = baseline_dir(fp);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::write(dir.join("fingerprint.txt"), fp.to_text()).map_err(|e| e.to_string())?;
    let file = dir.join(format!("{set}.tsv"));
    std::fs::write(&file, tsv(rows)).map_err(|e| e.to_string())?;
    Ok(file)
}

/// The saved baseline of `set` for this fingerprint.
pub fn load_baseline(set: &str, fp: &Fingerprint) -> Result<Vec<Row>, String> {
    let dir = baseline_dir(fp);
    let saved = std::fs::read_to_string(dir.join("fingerprint.txt")).map_err(|_| {
        format!("no baseline for this machine; run `ast-bench run --set {set} --save` first")
    })?;
    check_same_machine(&Fingerprint::parse(&saved), fp)?;
    let text = std::fs::read_to_string(dir.join(format!("{set}.tsv")))
        .map_err(|_| format!("no {set} baseline here; run `ast-bench run --set {set} --save`"))?;
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
}
