// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

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

/// The git worktree of `git_ref` under `target/perf/base/`, made if it is
/// not already there.
fn base_tree(repo: &Path, git_ref: &str) -> Result<PathBuf, String> {
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
    Ok(tree)
}

/// Build `ast-bench` at `git_ref` in a git worktree under
/// `target/perf/base/`, reusing it if it is already there, and return the
/// binary.
pub(crate) fn build_base(repo: &Path, git_ref: &str) -> Result<PathBuf, String> {
    let tree = base_tree(repo, git_ref)?;
    build_in(&tree, git_ref)
}

/// Build the `ast-bench` whose source is in the worktree `tree` of
/// `git_ref`, and return the binary.
///
/// Each worktree builds into its own target directory. Cargo names a
/// workspace member's build outputs without its path, so two worktrees
/// sharing one would share the build script's output, and cargo would judge
/// it fresh from the files of whichever worktree ran it last: a base that
/// compiles vendored C would then link without it.
fn build_in(tree: &Path, git_ref: &str) -> Result<PathBuf, String> {
    let target = tree.join("target");
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
        .current_dir(tree)
        .status()
        .map_err(|e| format!("cargo build at {git_ref}: {e}"))?;
    if !status.success() {
        return Err(format!("could not build ast-bench at {git_ref}"));
    }
    newest_binary(&target)
}

/// The most recently built `ast-bench` under `target`. `cargo build` puts it
/// under the target triple when one is configured and directly under
/// `release` otherwise, so a binary from an earlier build with the other
/// layout can sit beside the one just built.
fn newest_binary(target: &Path) -> Result<PathBuf, String> {
    let mut candidates = vec![target.join("release/ast-bench")];
    for dir in std::fs::read_dir(target)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        candidates.push(dir.path().join("release/ast-bench"));
    }
    candidates
        .into_iter()
        .filter_map(|p| {
            let t = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
            Some((t, p))
        })
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
        .ok_or_else(|| format!("no ast-bench binary under {}", target.display()))
}

/// Copy the directory `from` and everything under it to `to`.
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

/// The output of this tree's `ast-bench <args>` built against the library
/// at `git_ref`.
///
/// The worktree's `crates/ast-bench` is replaced by this tree's for the
/// build and the run, so the command reads the fixtures its own source
/// expects, and is put back as `git_ref` has it afterwards, whether or not
/// either succeeds, so a later build there is the base's own again.
fn current_bench_output_at(repo: &Path, git_ref: &str, args: &[&str]) -> Result<String, String> {
    let tree = base_tree(repo, git_ref)?;
    let theirs = tree.join("crates/ast-bench");
    let run = || {
        std::fs::remove_dir_all(&theirs)
            .and_then(|()| copy_dir(&repo.join("crates/ast-bench"), &theirs))
            .map_err(|e| format!("copying ast-bench into {}: {e}", tree.display()))?;
        let bin = build_in(&tree, git_ref).map_err(|e| {
            format!("{e}: this tree's ast-bench does not build against the library at {git_ref}")
        })?;
        output_of(&bin, args)
    };
    let out = run();
    git(&["checkout", "--", "crates/ast-bench"], &tree)?;
    git(&["clean", "-fdq", "crates/ast-bench"], &tree)?;
    out
}

/// Run `bin <args>` and return what it prints.
fn output_of(bin: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new(bin)
        .args(args)
        .output()
        .map_err(|e| format!("{}: {e}", bin.display()))?;
    if !out.status.success() {
        return Err(format!(
            "{} {} failed ({}): {}",
            bin.display(),
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The output of `ast-bench <args>` from the build at `git_ref`.
pub fn output_at(git_ref: &str, args: &[&str]) -> Result<String, String> {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    output_of(&build_base(&repo, git_ref)?, args)
}

/// The output of `ast-bench bits` from the build at `git_ref`.
pub fn bits_at(git_ref: &str) -> Result<String, String> {
    output_at(git_ref, &["bits"])
}

/// The output of `ast-bench values` for the library at `git_ref`.
///
/// A base whose own `ast-bench` has the command runs it. An older base does
/// not have it, so this tree's `ast-bench` is built against that base's
/// library and run instead: the values are still the base library's.
pub fn values_at(git_ref: &str) -> Result<String, String> {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tree = base_tree(&repo, git_ref)?;
    if tree.join("crates/ast-bench/src/ulps.rs").exists() {
        output_of(&build_in(&tree, git_ref)?, &["values"])
    } else {
        current_bench_output_at(&repo, git_ref, &["values"])
    }
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

    #[test]
    fn the_newest_binary_wins_over_a_stale_one_under_a_triple() {
        let dir = std::env::temp_dir().join(format!("ast-bench-bin-{}", std::process::id()));
        let stale = dir.join("aarch64-apple-darwin/release/ast-bench");
        let fresh = dir.join("release/ast-bench");
        for p in [&stale, &fresh] {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"").unwrap();
        }
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(86_400);
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let got = newest_binary(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(got.unwrap(), fresh);
    }

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
