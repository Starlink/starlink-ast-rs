// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Read every file of a corpus with the port and with the C library, and
//! report where they differ.
//!
//! ```text
//! hardening_replay <corpus dir> [--fits]
//! ```
//!
//! For each file, `tools/oracle_fixtures/load_one` (C; see that file for how
//! to build it, and `AST_LOAD_ONE` to name another) and this program run
//! with `--one <file>` (the port) each read every Object from it, as a
//! Native dump or, with `--fits`, as FITS header cards, in a child process
//! limited to 20 seconds, a worker per core sharing the files. Each prints the same lines: the messages of any
//! error as `!! <status> <text>`, each Object read as `OBJECT <class>`, then
//! `ERROR <status>` or `END`, and for FITS the Encoding the cards gave.
//!
//! The report has five headings:
//! - agreement: the same Objects, or the same error with the same messages;
//! - disagreements: on whether the file reads, on the error code, or on the
//!   messages;
//! - port-only limits: files the port refuses by a limit C does not have,
//!   recognized by the limit's message;
//! - C faults: files C crashes or hangs on, each a fault to record;
//! - port faults: files the port panics or hangs on.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use starlink_ast::error::AstError;
use starlink_ast::{Channel, FitsChan, NativeChannel};

/// The most Objects read from one file, as the fuzz targets and `load_one`
/// read.
const MAX_OBJECTS: usize = 16;

/// How long either reader may take with one file.
const LIMIT: Duration = Duration::from_secs(20);

/// The messages of the port's own limits, which C does not have (the quirks
/// file, "Addressed here, and not going upstream"): a file the port refuses
/// with one of these, where C reads it, is not a disagreement.
const PORT_ONLY_LIMITS: &[&str] = &[
    "but a dump cannot hold more than 9999999 values",
    "Objects nested more than 130 deep cannot be read.",
    "is nested more than 100 deep",
    "but a FITS header may hold only printable ASCII characters",
];

/// Print `err` as the C library reports it: its message and then each
/// context, a line each, with the status.
fn print_error(err: &AstError) {
    let status = err.code().as_raw();
    for text in std::iter::once(err.message()).chain(err.context().iter().map(String::as_str)) {
        for line in text.lines() {
            println!("!! {status} {line}");
        }
    }
}

/// Read up to [`MAX_OBJECTS`] Objects from `chan`, printing each.
fn read_all(chan: &mut dyn Channel) {
    for _ in 0..MAX_OBJECTS {
        match chan.read() {
            Ok(Some(obj)) => println!("OBJECT {}", obj.class_name()),
            Ok(None) => break,
            Err(e) => {
                print_error(&e);
                println!("ERROR {}", e.code().as_raw());
                return;
            }
        }
    }
    println!("END");
}

/// The port's side of one file, printed as `load_one` prints C's.
fn one(path: &Path, fits: bool) {
    let text = String::from_utf8_lossy(&std::fs::read(path).expect("readable file")).into_owned();
    if fits {
        let mut fc = FitsChan::new().expect("a FitsChan");
        for line in text.lines() {
            if line.split('\0').next().unwrap_or_default().is_empty() {
                continue;
            }
            if let Err(e) = fc.put_fits(line, false) {
                print_error(&e);
                println!("PUTERR {}", e.code().as_raw());
            }
        }
        let _ = fc.clear_card();
        match fc.encoding() {
            Ok(enc) => println!("ENCODING {}", enc.name()),
            Err(e) => print_error(&e),
        }
        read_all(&mut fc);
    } else {
        read_all(&mut NativeChannel::from_string(&text));
    }
}

/// How a reader's child process ended.
enum Run {
    /// It finished; its output, one line per item.
    Done(Vec<String>),
    /// It was killed by a signal or exited in error; what it printed.
    Crashed(String),
    /// It was stopped after [`LIMIT`].
    Hung,
}

/// Run `cmd` with the [`LIMIT`], collecting its output.
fn run(mut cmd: Command) -> Run {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the reader starts");
    let mut stdout = child.stdout.take().expect("piped stdout");
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let start = Instant::now();
    loop {
        match child.try_wait().expect("the reader can be waited for") {
            Some(status) => {
                let out = String::from_utf8_lossy(&reader.join().unwrap_or_default()).into_owned();
                if !status.success() {
                    let mut err = String::new();
                    if let Some(mut e) = child.stderr.take() {
                        let _ = e.read_to_string(&mut err);
                    }
                    let first = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                    return Run::Crashed(format!("{status}: {first}"));
                }
                return Run::Done(out.lines().map(str::to_string).collect());
            }
            None if start.elapsed() > LIMIT => {
                let _ = child.kill();
                let _ = child.wait();
                return Run::Hung;
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

/// A reader's output with each message cut into lines: a C message holding
/// a newline arrives as one `!!` item, where the port may hold the same text
/// as a message and a context.
fn normalized(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut status = String::new();
    for line in lines {
        if let Some(rest) = line.strip_prefix("!! ") {
            let (code, text) = rest.split_once(' ').unwrap_or((rest, ""));
            status = code.to_string();
            out.push(format!("!! {status} {}", text.trim_end()));
        } else if ["OBJECT ", "ERROR ", "PUTERR ", "ENCODING "]
            .iter()
            .any(|p| line.starts_with(p))
            || line == "END"
        {
            out.push(line.clone());
        } else {
            out.push(format!("!! {status} {}", line.trim_end()));
        }
    }
    out
}

/// Whether a line of C's output and of the port's say the same. C cuts text
/// at a byte count, which can split a character, shown here as U+FFFD; the
/// port stops before the character, so a C line that differs only by such
/// a character before `...` or at its end is the same.
fn same_line(c: &str, port: &str) -> bool {
    c == port || c.replace("\u{fffd}...", "...").trim_end_matches('\u{fffd}') == port
}

/// The final outcome line of a reader's output.
fn outcome(lines: &[String]) -> &str {
    lines
        .iter()
        .rev()
        .find(|l| l.starts_with("ERROR ") || *l == "END")
        .map_or("", String::as_str)
}

/// The kinds of result, as the report heads them.
#[derive(Default)]
struct Report {
    agree: usize,
    disagree: Vec<String>,
    port_limit: Vec<String>,
    c_fault: Vec<String>,
    port_fault: Vec<String>,
}

fn compare(file: &Path, c: Run, port: Run, report: &mut Report) {
    let name = file.display().to_string();
    let (c, port) = match (c, port) {
        (Run::Done(c), Run::Done(port)) => (normalized(&c), normalized(&port)),
        (c, port) => {
            let show = |r: &Run| match r {
                Run::Done(l) => outcome(l).to_string(),
                Run::Crashed(why) => format!("crashed ({why})"),
                Run::Hung => format!("took more than {} s", LIMIT.as_secs()),
            };
            let line = format!("{name}: C {}; port {}", show(&c), show(&port));
            if !matches!(port, Run::Done(_)) {
                report.port_fault.push(line);
            } else {
                report.c_fault.push(line);
            }
            return;
        }
    };
    if c.len() == port.len() && c.iter().zip(&port).all(|(a, b)| same_line(a, b)) {
        report.agree += 1;
        return;
    }
    let first = c
        .iter()
        .zip(&port)
        .position(|(a, b)| !same_line(a, b))
        .unwrap_or(c.len().min(port.len()));
    let at = |v: &[String]| v.get(first).cloned().unwrap_or_else(|| "(nothing)".into());
    if PORT_ONLY_LIMITS.iter().any(|m| at(&port).contains(m)) {
        report.port_limit.push(name);
        return;
    }
    let kind = match (outcome(&c), outcome(&port)) {
        (a, b) if a.starts_with("ERROR ") != b.starts_with("ERROR ") => "acceptance",
        (a, b) if a != b => "code",
        _ => "messages or Objects",
    };
    report.disagree.push(format!(
        "{name} [{kind}]\n    C:    {}\n    port: {}",
        at(&c),
        at(&port)
    ));
}

/// The C reader: `AST_LOAD_ONE`, or `tools/oracle_fixtures/load_one`.
fn load_one() -> PathBuf {
    std::env::var_os("AST_LOAD_ONE").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/oracle_fixtures/load_one"),
        PathBuf::from,
    )
}

fn heading(title: &str, items: &[String]) {
    println!("\n{title}: {}", items.len());
    for item in items {
        println!("  {item}");
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fits = args.iter().any(|a| a == "--fits");
    let mut rest = args.iter().filter(|a| *a != "--fits");
    match (rest.next().map(String::as_str), rest.next()) {
        (Some("--one"), Some(file)) => {
            one(Path::new(file), fits);
            return ExitCode::SUCCESS;
        }
        (Some(dir), None) if !dir.starts_with("--") => {
            let c_reader = load_one();
            if !c_reader.is_file() {
                eprintln!(
                    "{} is missing; build it as tools/oracle_fixtures/load_one.c says",
                    c_reader.display()
                );
                return ExitCode::FAILURE;
            }
            let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
                .expect("a readable corpus directory")
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.is_file()
                        && !p
                            .file_name()
                            .is_some_and(|n| n.to_string_lossy().starts_with('.'))
                })
                .collect();
            files.sort();
            let me = std::env::current_exe().expect("this program's path");
            let mut report = Report::default();
            // Each file's two readers run one after the other, and the
            // files are shared among a worker per core; the results are
            // reported in file order.
            let next = std::sync::atomic::AtomicUsize::new(0);
            let results: Vec<std::sync::Mutex<Option<(Run, Run)>>> =
                files.iter().map(|_| std::sync::Mutex::new(None)).collect();
            let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
            std::thread::scope(|scope| {
                for _ in 0..workers {
                    scope.spawn(|| {
                        loop {
                            let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(file) = files.get(i) else { break };
                            let mut c = Command::new(&c_reader);
                            c.arg(if fits { "fits" } else { "native" }).arg(file);
                            let mut port = Command::new(&me);
                            port.arg("--one").arg(file);
                            if fits {
                                port.arg("--fits");
                            }
                            *results[i].lock().expect("unpoisoned") = Some((run(c), run(port)));
                        }
                    });
                }
            });
            for (file, result) in files.iter().zip(results) {
                let (c, port) = result
                    .into_inner()
                    .expect("unpoisoned")
                    .expect("every file run");
                compare(file, c, port, &mut report);
            }
            println!("{} files from {dir}", files.len());
            println!("\nagreement: {}", report.agree);
            heading("disagreements", &report.disagree);
            heading("port-only limits", &report.port_limit);
            heading("C faults", &report.c_fault);
            heading("port faults", &report.port_fault);
            return if report.disagree.is_empty() && report.port_fault.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
        }
        _ => {}
    }
    eprintln!(
        "usage: hardening_replay <corpus dir> [--fits]\n       hardening_replay --one <file> [--fits]"
    );
    ExitCode::FAILURE
}
