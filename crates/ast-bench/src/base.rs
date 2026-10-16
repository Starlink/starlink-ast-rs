//! Compare the current build with a base commit, both timed on this machine
//! in the same session, alternating, so no saved baseline is needed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::compare::{slowdowns, unmatched_rows};
use crate::report::{Row, parse_tsv};
use crate::timing::Stats;

/// How many times each build times the set, alternating.
pub const ROUNDS: usize = 3;

/// One row per case, direction, point count and engine across `rounds`: the
/// median of the rounds' medians and the fastest sample of any round.
pub fn combine_rounds(rounds: &[Vec<Row>]) -> Vec<Row> {
    let mut by: BTreeMap<(String, String, usize, String), Vec<&Row>> = BTreeMap::new();
    for r in rounds.iter().flatten() {
        by.entry((r.case.clone(), r.dir.clone(), r.n, r.engine.clone()))
            .or_default()
            .push(r);
    }
    by.into_values()
        .map(|rs| {
            let mut medians: Vec<f64> = rs.iter().map(|r| r.stats.median_ns_per_point).collect();
            medians.sort_by(f64::total_cmp);
            let min = rs
                .iter()
                .map(|r| r.stats.min_ns_per_point)
                .fold(f64::INFINITY, f64::min);
            Row {
                stats: Stats {
                    median_ns_per_point: medians[medians.len() / 2],
                    min_ns_per_point: min,
                    samples: rs.iter().map(|r| r.stats.samples).sum(),
                },
                ..rs[0].clone()
            }
        })
        .collect()
}

fn git(args: &[&str], dir: &Path) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Build `ast-bench` at `git_ref` in a git worktree under
/// `target/perf/base/`, reusing it if it is already there, and return the
/// binary.
pub(crate) fn build_base(repo: &Path, git_ref: &str) -> Result<PathBuf, String> {
    let sha = git(
        &["rev-parse", "--verify", &format!("{git_ref}^{{commit}}")],
        repo,
    )?;
    let tree = repo.join("target/perf/base").join(&sha[..12]);
    if !tree.exists() {
        git(
            &["worktree", "add", "--detach", &tree.to_string_lossy(), &sha],
            repo,
        )?;
    }
    if !tree.join("crates/ast-bench").exists() {
        return Err(format!("{git_ref} predates ast-bench; choose a later base"));
    }
    let target = repo.join("target/perf/base/target");
    let status = Command::new("cargo")
        .args([
            "build",
            "--release",
            "-q",
            "-p",
            "ast-bench",
            "--target-dir",
        ])
        .arg(&target)
        .current_dir(&tree)
        .status()
        .map_err(|e| format!("cargo build at {git_ref}: {e}"))?;
    if !status.success() {
        return Err(format!("could not build ast-bench at {git_ref}"));
    }
    // `cargo build` puts the binary under the target triple when one is
    // configured and directly under `release` otherwise.
    for dir in std::fs::read_dir(&target)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let bin = dir.path().join("release/ast-bench");
        if bin.exists() {
            return Ok(bin);
        }
    }
    let bin = target.join("release/ast-bench");
    bin.exists()
        .then_some(bin)
        .ok_or_else(|| format!("no ast-bench binary under {}", target.display()))
}

/// The output of `ast-bench bits` from the build at `git_ref`.
pub fn bits_at(git_ref: &str) -> Result<String, String> {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bin = build_base(&repo, git_ref)?;
    let out = Command::new(&bin)
        .arg("bits")
        .output()
        .map_err(|e| format!("{}: {e}", bin.display()))?;
    if !out.status.success() {
        return Err(format!(
            "{} bits failed ({}): {}",
            bin.display(),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Run `bin` on the set file and read its TSV.
fn time(bin: &Path, set_file: &Path) -> Result<Vec<Row>, String> {
    let out = Command::new(bin)
        .args(["run", "--engine", "rust", "--format", "tsv", "--set-file"])
        .arg(set_file)
        .output()
        .map_err(|e| format!("{}: {e}", bin.display()))?;
    if !out.status.success() {
        return Err(format!(
            "{} failed ({}): {}",
            bin.display(),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    parse_tsv(&String::from_utf8_lossy(&out.stdout))
}

/// Time the set in `set_file` with this build and with the build at
/// `git_ref`, alternating them for [`ROUNDS`] rounds, and return each timing
/// the current build is slower in.
pub fn compare_with_base(git_ref: &str, set_file: &Path) -> Result<Vec<String>, String> {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let base_bin = build_base(&repo, git_ref)?;
    let cur_bin = std::env::current_exe().map_err(|e| e.to_string())?;
    let (mut base, mut cur) = (Vec::new(), Vec::new());
    for _ in 0..ROUNDS {
        base.push(time(&base_bin, set_file)?);
        cur.push(time(&cur_bin, set_file)?);
    }
    let (base, cur) = (combine_rounds(&base), combine_rounds(&cur));
    let unmatched = unmatched_rows(&base, &cur);
    if !unmatched.is_empty() {
        return Err(format!(
            "cannot compare every timing:\n  {}",
            unmatched.join("\n  ")
        ));
    }
    Ok(slowdowns(&base, &cur))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::Row;
    use crate::timing::Stats;

    fn row(median: f64, min: f64) -> Row {
        Row {
            case: "a.map".into(),
            dir: "forward".into(),
            n: 1,
            engine: "rust".into(),
            stats: Stats {
                median_ns_per_point: median,
                min_ns_per_point: min,
                samples: 7,
            },
        }
    }

    #[test]
    fn combine_takes_the_median_round_and_the_fastest_sample() {
        let c = combine_rounds(&[
            vec![row(100.0, 90.0)],
            vec![row(300.0, 95.0)],
            vec![row(110.0, 80.0)],
        ]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].stats.median_ns_per_point, 110.0);
        assert_eq!(c[0].stats.min_ns_per_point, 80.0);
    }

    #[test]
    fn a_failing_child_is_an_error() {
        let e = time(Path::new("/usr/bin/false"), Path::new("unused.txt")).unwrap_err();
        assert!(e.contains("false"), "{e}");
    }
}
