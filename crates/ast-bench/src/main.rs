//! `ast-bench` command-line entry point.

use std::path::Path;
use std::process::ExitCode;

use ast_bench::cases::{load_set, load_set_file};
use ast_bench::points::NPOINTS;
use ast_bench::report::{Row, table, tsv};
use ast_bench::run::{Engines, check_not_counting, run_set};

const USAGE: &str = "usage: ast-bench <command> [options]
commands:
  run --set <name> | --set-file <path> [--engine rust|c|both] [--format table|tsv]
  compare --set <name> [--base <git-ref>]
  sweep
  snapshot --set <name>
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

/// Time the set the arguments name; the rows, and the errors of any case
/// that could not be timed.
fn run(args: &[String]) -> Result<(Vec<Row>, Vec<String>), String> {
    check_not_counting()?;
    let cases = match (opt(args, "--set"), opt(args, "--set-file")) {
        (Some(name), None) => load_set(name)?,
        (None, Some(path)) => load_set_file(Path::new(path))?,
        _ => return Err("give one of --set or --set-file".into()),
    };
    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for r in run_set(&cases, engines(args)?, &NPOINTS) {
        match r {
            Ok(row) => rows.push(row),
            Err(e) => errors.push(e),
        }
    }
    Ok((rows, errors))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None | Some("help") | Some("--help") => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("run") => run(&args[1..]).map(|(rows, errors)| {
            match opt(&args, "--format").unwrap_or("table") {
                "tsv" => print!("{}", tsv(&rows)),
                _ => print!("{}", table(&rows)),
            }
            errors
        }),
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
