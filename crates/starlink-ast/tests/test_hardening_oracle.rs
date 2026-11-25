// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Every corrupt dump the C hardening tests read must be refused with the C
//! library's error code and its whole ordered list of messages, or loaded
//! where C loads it, and never crash, hang or exhaust memory.
//!
//! The cases are every `astFromString` call of
//! `reference/starlink-ast/ast_tester/` `testcorruptinput.c`,
//! `testloaderhardening.c`, `testframeperm.c`, `testframesetload.c` and
//! `testdssmap.c`, recorded with the C library's status and messages by
//! `tools/oracle_fixtures/hardening_probe.c` (see `hardening_oracle/cases.rs`
//! for the commit).
//! Divergences from the C:
//!
//! - The C tests check only the status; the messages are compared too.
//! - C's first message, "AST: Error in routine ...", names the calling C
//!   function and is not compared.
//! - The C tests' other checks (constructors, transformations, documents) are
//!   in the Rust ports of each test.

use std::process::Command;
use std::time::{Duration, Instant};

use starlink_ast::error::{AstError, AstResult};
use starlink_ast::from_native_string;

/// One dump a C test read, with what the C library reported.
pub(crate) struct Case {
    /// The C test.
    pub(crate) test: &'static str,
    /// Which `astFromString` call of the test, from 0.
    pub(crate) ordinal: usize,
    /// The dump.
    pub(crate) dump: &'static str,
    /// The status C was left with, or 0 when it returned an object.
    pub(crate) code: u32,
    /// The messages C reported, in order.
    pub(crate) messages: &'static [&'static str],
}

#[path = "hardening_oracle/cases.rs"]
mod cases;
use cases::CASES;

/// Why a case is excluded.
enum Why {
    /// The port refuses or loads it otherwise than C does.
    Differs(&'static str),
    /// The port crashes, hangs or exhausts memory on it, so it is run in a
    /// child process.
    Crashes(&'static str),
}

/// The cases the port does not yet match, each with the reason.
const HARDENING_EXCLUDED: &[(&str, usize, Why)] = &[
    (
        "testcorruptinput",
        23,
        Why::Differs("Task 9: Table and FitsTable components (1e1ae0fe)"),
    ),
    (
        "testcorruptinput",
        24,
        Why::Differs("Task 9: Table and FitsTable components (1e1ae0fe)"),
    ),
    (
        "testcorruptinput",
        25,
        Why::Differs("Task 9: Table and FitsTable components (1e1ae0fe)"),
    ),
    (
        "testcorruptinput",
        26,
        Why::Differs("Task 9: Table and FitsTable components (1e1ae0fe)"),
    ),
    (
        "testcorruptinput",
        28,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        29,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        30,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        31,
        Why::Crashes("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        32,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        33,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        34,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        35,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        37,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        38,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        39,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        40,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        42,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        43,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        45,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        47,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        49,
        Why::Crashes("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        50,
        Why::Crashes("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        51,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        52,
        Why::Crashes("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        54,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        56,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        57,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        58,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        59,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        60,
        Why::Crashes("Task 3: AST__MXAXES (b3a1143b)"),
    ),
    (
        "testcorruptinput",
        62,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        63,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        64,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        65,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        67,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        68,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        69,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        70,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        71,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        72,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        73,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        74,
        Why::Differs("Task 8: Region and PointSet loaders (8078ddd8, aa29054e, 63c9f42f)"),
    ),
    (
        "testcorruptinput",
        100,
        Why::Differs("Task 7: PermMap constants (c6932471)"),
    ),
    (
        "testcorruptinput",
        101,
        Why::Differs("Task 7: PermMap constants (c6932471)"),
    ),
    (
        "testcorruptinput",
        102,
        Why::Crashes("Task 7: PermMap constants (c6932471)"),
    ),
    (
        "testcorruptinput",
        107,
        Why::Crashes("Task 7: MathMap functions (aa915bed)"),
    ),
    (
        "testcorruptinput",
        108,
        Why::Crashes("Task 7: MathMap functions (aa915bed)"),
    ),
    (
        "testcorruptinput",
        110,
        Why::Crashes("Task 7: MatrixMap elements (da7d3330)"),
    ),
    (
        "testcorruptinput",
        111,
        Why::Differs("Task 7: MatrixMap elements (da7d3330)"),
    ),
    (
        "testcorruptinput",
        112,
        Why::Differs("Task 7: WcsMap axes (639232d1, cfdbbe8d)"),
    ),
    (
        "testcorruptinput",
        120,
        Why::Differs("Task 9: Table and KeyMap loaders (e384383c, 8c542c65)"),
    ),
    (
        "testloaderhardening",
        1,
        Why::Differs("Task 7: PolyMap powers (2ae11f85)"),
    ),
    (
        "testloaderhardening",
        2,
        Why::Differs("Task 7: PolyMap powers (2ae11f85)"),
    ),
    (
        "testloaderhardening",
        3,
        Why::Differs("Task 7: PolyMap powers (2ae11f85)"),
    ),
    (
        "testloaderhardening",
        4,
        Why::Differs("Task 7: PolyMap powers (2ae11f85)"),
    ),
    (
        "testloaderhardening",
        6,
        Why::Differs("Task 7: LutMap table (cda35a7b)"),
    ),
    (
        "testloaderhardening",
        7,
        Why::Differs("Task 7: LutMap table (cda35a7b)"),
    ),
    (
        "testloaderhardening",
        15,
        Why::Differs("Task 7: MathMap functions (aa915bed)"),
    ),
    (
        "testloaderhardening",
        16,
        Why::Differs("Task 7: MathMap functions (aa915bed)"),
    ),
    (
        "testloaderhardening",
        17,
        Why::Differs("Task 7: MathMap functions (aa915bed)"),
    ),
    (
        "testloaderhardening",
        18,
        Why::Differs("Task 7: MathMap functions (aa915bed)"),
    ),
    (
        "testloaderhardening",
        19,
        Why::Differs("Task 7: MathMap functions (aa915bed)"),
    ),
    (
        "testloaderhardening",
        20,
        Why::Differs("Task 7: MathMap functions (aa915bed)"),
    ),
    (
        "testloaderhardening",
        22,
        Why::Differs("Task 7: WcsMap axes (639232d1)"),
    ),
    (
        "testloaderhardening",
        23,
        Why::Differs("Task 7: WcsMap axes (639232d1)"),
    ),
    (
        "testloaderhardening",
        24,
        Why::Differs("Task 7: WcsMap axes (639232d1)"),
    ),
    (
        "testloaderhardening",
        25,
        Why::Differs("Task 7: WcsMap axes (639232d1)"),
    ),
    (
        "testloaderhardening",
        27,
        Why::Differs("Task 7: WcsMap axes (639232d1)"),
    ),
    (
        "testloaderhardening",
        28,
        Why::Differs("Task 7: WcsMap axes (639232d1)"),
    ),
    (
        "testloaderhardening",
        29,
        Why::Differs("Task 7: WcsMap axes (639232d1)"),
    ),
    (
        "testloaderhardening",
        31,
        Why::Differs("Task 7: SplineMap sizes and knots (ca7c350e)"),
    ),
    (
        "testloaderhardening",
        32,
        Why::Differs("Task 7: SplineMap sizes and knots (ca7c350e)"),
    ),
    (
        "testloaderhardening",
        33,
        Why::Crashes("Task 7: SplineMap sizes and knots (ca7c350e)"),
    ),
    (
        "testloaderhardening",
        34,
        Why::Differs("Task 7: SplineMap sizes and knots (ca7c350e)"),
    ),
    (
        "testloaderhardening",
        35,
        Why::Differs("Task 7: SplineMap sizes and knots (ca7c350e)"),
    ),
    (
        "testloaderhardening",
        45,
        Why::Differs("Plot3D is not ported"),
    ),
    (
        "testloaderhardening",
        46,
        Why::Differs("Plot3D is not ported"),
    ),
    (
        "testloaderhardening",
        47,
        Why::Differs("Plot3D is not ported"),
    ),
    (
        "testloaderhardening",
        48,
        Why::Differs("Plot3D is not ported"),
    ),
    (
        "testloaderhardening",
        49,
        Why::Differs("Plot3D is not ported"),
    ),
    (
        "testloaderhardening",
        50,
        Why::Differs("Plot3D is not ported"),
    ),
    (
        "testloaderhardening",
        51,
        Why::Differs("Plot3D is not ported"),
    ),
    (
        "testloaderhardening",
        53,
        Why::Differs("Task 9: Moc ranges and MaxOrder (d3f5b711)"),
    ),
    (
        "testloaderhardening",
        54,
        Why::Differs("Task 9: Moc ranges and MaxOrder (d3f5b711)"),
    ),
    (
        "testloaderhardening",
        55,
        Why::Differs("Task 9: Moc ranges and MaxOrder (d3f5b711)"),
    ),
    (
        "testdssmap",
        6,
        Why::Differs("Task 7: DssMap plate centre and offsets (cba0743a)"),
    ),
    (
        "testdssmap",
        7,
        Why::Differs("Task 7: DssMap plate centre and offsets (cba0743a)"),
    ),
    (
        "testdssmap",
        8,
        Why::Differs("Task 7: DssMap plate centre and offsets (cba0743a)"),
    ),
    (
        "testdssmap",
        9,
        Why::Differs("Task 7: DssMap plate centre and offsets (cba0743a)"),
    ),
];

/// Cases whose first message C cuts to fit its 1023-character buffer, which
/// the port does not mirror: the port's message must begin with C's and go
/// on with the rest of the value C left out.
const CUT_BY_C: &[(&str, usize, &str)] = &[
    (
        "testcorruptinput",
        117,
        "a Naxes of 5000 digits, which the message quotes",
    ),
    (
        "testcorruptinput",
        118,
        "a System of 5000 characters, which the message quotes",
    ),
];

/// Whether the port's `got` messages are C's `want`, allowing for C's cut
/// of the first when `case` is one of [`CUT_BY_C`].
fn same_messages(case: &Case, got: &[&str], want: &[&str]) -> bool {
    if got == want {
        return true;
    }
    let cut = CUT_BY_C
        .iter()
        .any(|(t, o, _)| *t == case.test && *o == case.ordinal);
    cut && got.len() == want.len()
        && got[0].len() > want[0].len()
        && want[0].len() >= 1023
        && got[0].starts_with(want[0])
        && got[1..] == want[1..]
}

/// The messages of `err` in C's order: its message, a line at a time, then
/// its context, oldest first.
pub(crate) fn messages(err: &AstError) -> Vec<&str> {
    err.message()
        .split('\n')
        .chain(err.context().iter().map(String::as_str))
        .collect()
}

/// Run `f` on its own thread, giving `Err` with what went wrong if it panics
/// or takes more than 10 s.
fn within_10s<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(v) => Ok(v),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err("took more than 10 s".into()),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err("panicked".into()),
    }
}

fn excluded(case: &Case) -> Option<&'static Why> {
    HARDENING_EXCLUDED
        .iter()
        .find(|(t, o, _)| *t == case.test && *o == case.ordinal)
        .map(|(_, _, why)| why)
}

/// Read `case` and describe how the port's result differs from C's, or
/// `None` if it matches.
fn difference(case: &'static Case) -> Option<String> {
    let result: AstResult<()> = match within_10s(move || from_native_string(case.dump).map(|_| ()))
    {
        Ok(r) => r,
        Err(what) => return Some(what),
    };
    match (result, case.code) {
        (Ok(()), 0) => None,
        (Ok(()), code) => Some(format!("loaded, where C gave {code} {:?}", case.messages)),
        (Err(e), 0) => Some(format!("refused with {e}, where C loads it")),
        (Err(e), code) => {
            let got = messages(&e);
            (e.code().as_raw() != code || !same_messages(case, &got, case.messages)).then(|| {
                format!(
                    "gave {} {got:?}, where C gave {code} {:?}",
                    e.code().as_raw(),
                    case.messages
                )
            })
        }
    }
}

fn name(case: &Case) -> String {
    format!("{}#{}", case.test, case.ordinal)
}

#[test]
fn cases_match_the_c_library() {
    let failures: Vec<String> = CASES
        .iter()
        .filter(|c| excluded(c).is_none())
        .filter_map(|c| difference(c).map(|d| format!("{}: {d}", name(c))))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The case named by `AST_HARDENING_CASE`, when `excluded_cases_still_differ`
/// runs this test in a child process; otherwise nothing.
#[test]
fn run_one_case() {
    let Ok(wanted) = std::env::var("AST_HARDENING_CASE") else {
        return;
    };
    let case = CASES
        .iter()
        .find(|c| name(c) == wanted)
        .unwrap_or_else(|| panic!("no case {wanted}"));
    if let Some(d) = difference(case) {
        panic!("{d}");
    }
}

/// Start the case in a child process, in which `run_one_case` runs it.
fn spawn_case(case: &Case) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "test_hardening_oracle::run_one_case",
            "--test-threads=1",
        ])
        .env("AST_HARDENING_CASE", name(case))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap()
}

/// Of `cases`, each run at once in its own child process, those that
/// matched C: a child that fails, or is still running after 20 s and is
/// killed, still crashes.
fn matching_in_children<'a>(cases: &[&'a Case]) -> Vec<&'a Case> {
    let mut children: Vec<_> = cases.iter().map(|c| (*c, spawn_case(c), None)).collect();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20)
        && children.iter().any(|(_, _, done)| done.is_none())
    {
        for (_, child, done) in &mut children {
            if done.is_none() {
                *done = child.try_wait().unwrap().map(|s| s.success());
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    children
        .into_iter()
        .filter_map(|(case, mut child, done)| {
            if done.is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
            (done == Some(true)).then_some(case)
        })
        .collect()
}

#[test]
fn excluded_cases_still_differ() {
    let crashing: Vec<&Case> = CASES
        .iter()
        .filter(|c| matches!(excluded(c), Some(Why::Crashes(_))))
        .collect();
    let crashing_now_match = matching_in_children(&crashing);
    let now_matching: Vec<String> = CASES
        .iter()
        .filter_map(|c| {
            let (matches, reason) = match excluded(c)? {
                Why::Differs(reason) => (difference(c).is_none(), reason),
                Why::Crashes(reason) => (
                    crashing_now_match.iter().any(|m| std::ptr::eq(*m, c)),
                    reason,
                ),
            };
            matches.then(|| format!("{} ({reason})", name(c)))
        })
        .collect();
    assert!(
        now_matching.is_empty(),
        "now match C; remove from HARDENING_EXCLUDED: {now_matching:?}"
    );
}

#[test]
fn excluded_cases_are_in_the_corpus() {
    for (test, ordinal, _) in HARDENING_EXCLUDED {
        assert!(
            CASES
                .iter()
                .any(|c| c.test == *test && c.ordinal == *ordinal),
            "{test}#{ordinal} is not a case"
        );
    }
}

/// Each case in [`CUT_BY_C`] is one whose message C cut: were it not, the
/// relaxation would be hiding a difference.
#[test]
fn cut_cases_are_cut_by_c() {
    for (test, ordinal, what) in CUT_BY_C {
        let case = CASES
            .iter()
            .find(|c| c.test == *test && c.ordinal == *ordinal)
            .unwrap_or_else(|| panic!("{test}#{ordinal} is not a case"));
        assert!(
            case.messages.first().is_some_and(|m| m.len() >= 1023),
            "{test}#{ordinal} ({what}): C did not cut its message"
        );
    }
}
