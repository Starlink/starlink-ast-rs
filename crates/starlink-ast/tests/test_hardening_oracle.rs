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

use std::time::Duration;

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

/// The cases the port does not yet match, each with the reason.
const HARDENING_EXCLUDED: &[(&str, usize, &str)] = &[
    (
        "testcorruptinput",
        23,
        "Task 9: Table and FitsTable components (1e1ae0fe)",
    ),
    (
        "testcorruptinput",
        24,
        "Task 9: Table and FitsTable components (1e1ae0fe)",
    ),
    (
        "testcorruptinput",
        25,
        "Task 9: Table and FitsTable components (1e1ae0fe)",
    ),
    (
        "testcorruptinput",
        26,
        "Task 9: Table and FitsTable components (1e1ae0fe)",
    ),
    (
        "testcorruptinput",
        71,
        "the port has no PointSet object to load; the dump is refused as an unknown class (PLAN.md section 1)",
    ),
    (
        "testcorruptinput",
        120,
        "Task 9: Table and KeyMap loaders (e384383c, 8c542c65)",
    ),
    ("testloaderhardening", 45, "Plot3D is not ported"),
    ("testloaderhardening", 46, "Plot3D is not ported"),
    ("testloaderhardening", 47, "Plot3D is not ported"),
    ("testloaderhardening", 48, "Plot3D is not ported"),
    ("testloaderhardening", 49, "Plot3D is not ported"),
    ("testloaderhardening", 50, "Plot3D is not ported"),
    ("testloaderhardening", 51, "Plot3D is not ported"),
    (
        "testloaderhardening",
        53,
        "Task 9: Moc ranges and MaxOrder (d3f5b711)",
    ),
    (
        "testloaderhardening",
        54,
        "Task 9: Moc ranges and MaxOrder (d3f5b711)",
    ),
    (
        "testloaderhardening",
        55,
        "Task 9: Moc ranges and MaxOrder (d3f5b711)",
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

fn excluded(case: &Case) -> Option<&'static str> {
    HARDENING_EXCLUDED
        .iter()
        .find(|(t, o, _)| *t == case.test && *o == case.ordinal)
        .map(|(_, _, why)| *why)
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

/// The case named by `AST_HARDENING_CASE` (`<test>#<ordinal>`), failing
/// with how the port differs from C, so that one case can be looked at
/// alone; otherwise nothing.
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

#[test]
fn excluded_cases_still_differ() {
    let now_matching: Vec<String> = CASES
        .iter()
        .filter_map(|c| {
            let reason = excluded(c)?;
            difference(c)
                .is_none()
                .then(|| format!("{} ({reason})", name(c)))
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
