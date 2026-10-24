// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! `ast-bench` command-line entry point.

use std::path::Path;
use std::process::ExitCode;

use ast_bench::cases::Case;
use ast_bench::cases::{load_set, load_set_file};
use ast_bench::compare::{
    CONFIRM_RUNS, confirm_slowdowns, describe, load_baseline, save_baseline, slow_rows,
    unmatched_rows,
};
use ast_bench::fingerprint::Fingerprint;
use ast_bench::points::NPOINTS;
use ast_bench::report::{Row, table, tsv};
use ast_bench::run::{Engines, check_not_counting, run_set, run_set_with};
use ast_bench::snapshot::{latest_record, ratio_warnings, today, worst_ratios, write_record};
use ast_bench::sweep::{SWEEP_NPOINTS, all_cases};

const USAGE: &str = "usage: ast-bench <command> [options]
commands:
  run --set <name> | --set-file <path> [--engine rust|c|both] [--nan] [--format table|tsv] [--save]
  compare --set <name> [--engine rust|c|both] [--base <git-ref>]
  sweep [--nan]
  snapshot --set <name>
  bits [--base <git-ref>]
  roman [--engine rust|c|both] [--max-n N] [--reps R] [--markdown] [-o file.csv]
";

/// The value following `flag` in `args`, if present.
fn opt<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn engines(args: &[String]) -> Result<Engines, String> {
    match opt(args, "--engine").unwrap_or("rust") {
        "rust" => Ok(Engines::Rust),
        "c" => Ok(Engines::C),
        "both" => Ok(Engines::Both),
        other => Err(format!("unknown engine {other:?}")),
    }
}

/// The cases of the set the arguments name.
fn cases(args: &[String]) -> Result<Vec<Case>, String> {
    match (opt(args, "--set"), opt(args, "--set-file")) {
        (Some(name), None) => load_set(name),
        (None, Some(path)) => load_set_file(Path::new(path)),
        _ => Err("give one of --set or --set-file".into()),
    }
}

/// Time the set the arguments name; the rows, and the errors of any case
/// that could not be timed.
fn run(args: &[String]) -> Result<(Vec<Row>, Vec<String>), String> {
    check_not_counting()?;
    let cases = cases(args)?;
    let mut rows = Vec::new();
    let mut errors = Vec::new();
    let nan = args.iter().any(|a| a == "--nan");
    for r in run_set_with(&cases, engines(args)?, &NPOINTS, nan) {
        match r {
            Ok(row) => rows.push(row),
            Err(e) => errors.push(e),
        }
    }
    Ok((rows, errors))
}

/// The fingerprint of a run on `engines`: with the C engine's build
/// information when C is timed.
fn fingerprint(engines: Engines) -> Fingerprint {
    #[cfg(feature = "c-oracle")]
    if engines != Engines::Rust {
        let info = ast_bench::c_engine::CEngine::open(ast_bench::c_engine::Variant::Bench)
            .map(|e| e.build_info())
            .ok();
        return Fingerprint::current(info);
    }
    let _ = engines;
    Fingerprint::current(None)
}

/// `compare`: time the set again and list what got slower than this
/// machine's saved baseline; the slowdowns are the errors.
fn compare(args: &[String]) -> Result<Vec<String>, String> {
    let set = opt(args, "--set").ok_or("compare needs --set")?;
    if let Some(git_ref) = opt(args, "--base") {
        let file = Path::new(ast_bench::cases::SETS).join(format!("{set}.txt"));
        let slower = ast_bench::base::compare_with_base(git_ref, &file)?;
        if slower.is_empty() {
            println!("no timing of {set} is more than the threshold slower than at {git_ref}");
        }
        return Ok(slower.into_iter().map(|s| format!("slower: {s}")).collect());
    }
    let fp = fingerprint(engines(args)?);
    let base = load_baseline(set, &fp)?;
    let (rows, mut errors) = run(args)?;
    let all = cases(args)?;
    let engines = engines(args)?;
    errors.extend(unmatched_rows(&base, &rows));
    // Time each suspect again; keep it only while every re-timing is slow.
    let suspects: Vec<Row> = slow_rows(&base, &rows)
        .into_iter()
        .map(|(_, r)| r.clone())
        .collect();
    let suspects = confirm_slowdowns(&base, suspects, CONFIRM_RUNS, |s| {
        let these: Vec<Case> = all
            .iter()
            .filter(|c| {
                c.section.relpath == s.case
                    && (if c.section.forward {
                        "forward"
                    } else {
                        "inverse"
                    }) == s.dir
            })
            .cloned()
            .collect();
        run_set(&these, engines, &[s.n]).into_iter().collect()
    })?;
    if suspects.is_empty() && errors.is_empty() {
        println!("no timing of {set} is more than the threshold slower than its baseline");
    }
    if engines == Engines::Both {
        warn_ratios(set, &rows);
    }
    for s in &suspects {
        let was = slow_rows(&base, std::slice::from_ref(s))[0].0;
        errors.push(format!("slower: {}", describe(was, s)));
    }
    Ok(errors)
}

/// Print, as warnings only, each Rust/C ratio that is worse than in the
/// newest record of `set` for this platform.
fn warn_ratios(set: &str, rows: &[Row]) {
    if let Some((path, record)) = latest_record(set) {
        for w in ratio_warnings(&record, rows) {
            eprintln!("warning: ratio worse than {}: {w}", path.display());
        }
    }
}

/// `snapshot`: time the set on both engines and write a dated record.
fn snapshot(args: &[String]) -> Result<Vec<String>, String> {
    let set = opt(args, "--set").ok_or("snapshot needs --set")?;
    check_not_counting()?;
    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for r in run_set(&load_set(set)?, Engines::Both, &NPOINTS) {
        match r {
            Ok(row) => rows.push(row),
            Err(e) => errors.push(e),
        }
    }
    if !errors.is_empty() {
        return Err(format!(
            "not writing a record of {set}: some timings failed\n  {}",
            errors.join("\n  ")
        ));
    }
    let path = write_record(set, &fingerprint(Engines::Both), &rows)?;
    println!("wrote {}", path.display());
    Ok(errors)
}

/// `sweep`: time every oracle section on both engines; write the rows and
/// print the worst ratios and the sections that could not be timed.
fn sweep(args: &[String]) -> Result<Vec<String>, String> {
    check_not_counting()?;
    let mut rows = Vec::new();
    let mut untimed = Vec::new();
    let nan = args.iter().any(|a| a == "--nan");
    for r in run_set_with(&all_cases()?, Engines::Both, &SWEEP_NPOINTS, nan) {
        match r {
            Ok(row) => rows.push(row),
            Err(e) => untimed.push(e),
        }
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/perf");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file = dir.join(format!("sweep-{}.tsv", today()));
    std::fs::write(&file, tsv(&rows)).map_err(|e| format!("{}: {e}", file.display()))?;
    println!("wrote {}", file.display());
    println!("worst Rust/C ratios:");
    for (ratio, r) in worst_ratios(&rows, 20) {
        println!("{ratio:8.2}  {} {} N={}", r.case, r.dir, r.n);
    }
    println!("{} sections could not be timed:", untimed.len());
    for u in &untimed {
        println!("  {u}");
    }
    Ok(Vec::new())
}

/// `bits`: print the output-bit hashes of every corpus transform, or with
/// `--base`, list every case whose hashes differ from that commit's; either
/// way, list every case whose two paths (`transform` and the NaN protocol)
/// give different bits.
fn bits(args: &[String]) -> Result<Vec<String>, String> {
    check_not_counting()?;
    let cur = ast_bench::bits::all_bits()?;
    // The NaN protocol must give the same bits as `transform`, here and now.
    let mismatches: Vec<String> = ast_bench::bits::path_mismatches(&cur)?
        .into_iter()
        .map(|m| format!("paths differ: {m}"))
        .collect();
    if !mismatches.is_empty() {
        return Ok(mismatches);
    }
    match opt(args, "--base") {
        None => {
            print!("{cur}");
            Ok(Vec::new())
        }
        Some(git_ref) => {
            let base = ast_bench::base::bits_at(git_ref)?;
            let diff = ast_bench::bits::diff_bits(&base, &cur)?;
            if diff.is_empty() {
                println!("every transform's output bits match {git_ref}");
            }
            Ok(diff
                .into_iter()
                .map(|d| format!("bits differ: {d}"))
                .collect())
        }
    }
}

/// `roman`: Starlink/ast pull request 51's benchmark of the Roman WCS
/// transforms, on the engines named; prints the summary and how the engines'
/// outputs compare, and with `-o` writes the pull request's CSV.
fn roman(args: &[String]) -> Result<Vec<String>, String> {
    use ast_bench::roman;
    check_not_counting()?;
    let default_engine = if cfg!(feature = "c-oracle") {
        "both"
    } else {
        "rust"
    };
    let engines = match opt(args, "--engine").unwrap_or(default_engine) {
        "rust" => roman::Engines::Rust,
        "c" => roman::Engines::C,
        "both" => roman::Engines::Both,
        other => return Err(format!("unknown engine {other:?}")),
    };
    let num = |flag: &str, default: usize| -> Result<usize, String> {
        opt(args, flag).map_or(Ok(default), |v| {
            v.parse().map_err(|e| format!("{flag} {v}: {e}"))
        })
    };
    let max_n = num("--max-n", *roman::N_SWEEP.last().expect("non-empty"))?;
    let reps = num("--reps", roman::DEFAULT_REPS)?.max(1);
    let mut timings = Vec::new();
    let checks = roman::run(engines, max_n, reps, &mut |t| {
        eprintln!(
            "  {:<14} {:<4} N={:<9} median={:12.6} ms ({:8.1} Mpx/s)",
            t.label,
            t.engine,
            t.n,
            roman::median(&t.rep_times) * 1e3,
            t.mpxs()
        );
        timings.push(t.clone());
    })?;
    print!(
        "{}",
        roman::summary(&timings, args.iter().any(|a| a == "--markdown"))
    );
    for (label, c) in &checks {
        if c.differ == 0 {
            println!(
                "{label}: Rust and C outputs identical ({} values)",
                c.values
            );
        } else {
            println!(
                "{label}: {} of {} output values differ between Rust and C, by at most {} ULP",
                c.differ, c.values, c.max_ulps
            );
        }
    }
    if let Some(path) = opt(args, "-o") {
        std::fs::write(path, roman::csv(&timings)).map_err(|e| format!("{path}: {e}"))?;
        eprintln!("wrote {path}");
    }
    Ok(Vec::new())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None | Some("help") | Some("--help") => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("run") => run(&args[1..]).and_then(|(rows, errors)| {
            match opt(&args, "--format").unwrap_or("table") {
                "tsv" => print!("{}", tsv(&rows)),
                _ => print!("{}", table(&rows)),
            }
            if args.iter().any(|a| a == "--save") {
                // compare runs without rust-nan, so a baseline holding it
                // would leave those rows unmatched.
                if args.iter().any(|a| a == "--nan") {
                    return Err("--save cannot be combined with --nan".into());
                }
                let set = opt(&args, "--set").ok_or("--save needs --set")?;
                let file = save_baseline(set, &fingerprint(engines(&args)?), &rows)?;
                eprintln!("saved {}", file.display());
            }
            Ok(errors)
        }),
        Some("compare") => compare(&args[1..]),
        Some("snapshot") => snapshot(&args[1..]),
        Some("sweep") => sweep(&args[1..]),
        Some("bits") => bits(&args[1..]),
        Some("roman") => roman(&args[1..]),
        Some(other) => Err(format!("unknown command {other:?}\n{USAGE}")),
    };
    match result {
        Ok(errors) if errors.is_empty() => ExitCode::SUCCESS,
        Ok(errors) => {
            for e in errors {
                eprintln!("error: {e}");
            }
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
