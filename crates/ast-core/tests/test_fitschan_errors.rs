//! FitsChan warning and error paths, from malformed headers.
//!
//! Ancestor: `reference/starlink-ast/ast_tester/testfitschanerrors.c`. Its
//! bad-header table is `fitschanerrors_oracle/cases.rs`, generated from the
//! C source by `tools/oracle_fixtures/fitschanerrors_table.c`, and
//! [`check_case`] is C's `test_bad_header` step for step: the warning
//! conditions are set, the cards put one at a time and then `END`, the cursor
//! rewound, any attributes set, the header read, and then the expected error,
//! the expected warning text, or a write that the named encoding must refuse
//! and FITS-WCS must accept.
//!
//! The cases C's `main()` runs outside the table (read-only attributes, an
//! unknown warning condition, and the `-TAB` errors) are covered by
//! `test_fitschan_cursor_oracle.rs` and `test_fitschan_tab_read_oracle.rs`.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "fitschanerrors_oracle/cases.rs"]
mod cases;

use ast_core::channel::Channel;
use ast_core::error::ErrorCode;
use ast_core::fitschan::{Encoding, FitsChan};

/// One row of C's `bad_headers` table.
pub(crate) struct BadHeaderTest {
    name: &'static str,
    cards: &'static str,
    warn_cat: Option<&'static str>,
    expect_error: Option<ErrorCode>,
    expect_text: Option<&'static str>,
    skip_read: bool,
    attrs: Option<&'static str>,
    write_enc: Option<&'static str>,
}

/// Cases whose check is C's string attribute interface itself, with the
/// reason each has no counterpart here. Each must still be a row of C's
/// table (`excluded_cases_are_in_the_c_table`).
const FITSCHAN_ERRORS_EXCLUDED: &[(&str, &str)] = &[
    (
        "error-bad-encoding",
        "AST__BADAT from parsing \"Encoding=BADENC\"; Encoding is an enum here, \
         so an unknown name cannot be set",
    ),
    (
        "error-bad-altaxes",
        "AST__BADAT from parsing \"AltAxes=BADVAL\"; AltAxes is an enum here, \
         so an unknown name cannot be set",
    ),
    (
        "error-set-readonly",
        "AST__NOWRT from \"Ncard=5\"; Ncard has no setter here, so the \
         read-only attribute cannot be set",
    ),
];

/// Cases the port does not yet match, each with the gap it exposes. Each is
/// checked to still differ from C (`port_gap_cases_still_differ`), so the
/// list shrinks when the gap closes.
const FITSCHAN_ERRORS_GAP_EXCLUDED: &[(&str, &str)] = &[(
    "error-bad-radesys",
    "the header has RADESYS = 'FK5' and then RADESYS = 'BADVAL'. C's \
     WcsToStore reads every card into the FitsStore in order (WcsFcRead), so \
     the last duplicate wins and the read fails; the port looks each keyword \
     up and takes the first, so it reads FK5 (PLAN.md section 5)",
)];

/// Set the attributes a case names, as C's `astSet` would.
fn set_attrs(fc: &mut FitsChan, attrs: &str) -> ast_core::error::AstResult<()> {
    match attrs.split_once('=') {
        Some(("SourceFile", file)) => fc.set_source_file(file),
        _ => panic!("no Rust counterpart for the attribute setting {attrs:?}"),
    }
}

/// C's `test_bad_header`; returns a description of the first mismatch.
fn check_case(t: &BadHeaderTest) -> Result<(), String> {
    let mut fc = FitsChan::new().unwrap();
    if let Some(cat) = t.warn_cat {
        fc.set_warning_conditions(cat).map_err(|e| e.to_string())?;
    }

    // Cards are put one at a time, leading spaces removed and empty lines
    // dropped, then END. An error here ends the case.
    let mut put = || -> ast_core::error::AstResult<()> {
        for line in t.cards.split('\n') {
            let line = line.trim_start_matches(' ');
            if !line.is_empty() {
                fc.put_fits(line, false)?;
            }
        }
        fc.put_fits("END", false)
    };
    if let Err(e) = put() {
        return match t.expect_error {
            Some(code) if e.code() == code => Ok(()),
            _ => Err(format!("error during PutFits: {e}")),
        };
    }

    fc.clear_card().map_err(|e| e.to_string())?;
    if let Some(attrs) = t.attrs
        && let Err(e) = set_attrs(&mut fc, attrs)
    {
        return match t.expect_error {
            Some(code) if e.code() == code => Ok(()),
            _ => Err(format!("error setting {attrs}: {e}")),
        };
    }

    let read = if t.skip_read { Ok(None) } else { fc.read() };

    if let Some(code) = t.expect_error {
        return match read {
            Ok(_) => Err(format!("expected {code:?} but the read succeeded")),
            Err(e) if e.code() == code => Ok(()),
            Err(e) => Err(format!("expected {code:?}, got {e}")),
        };
    }

    if let Some(cat) = t.warn_cat {
        let Some(km) = fc.warnings() else {
            return Err(format!("no warnings issued (expected '{cat}')"));
        };
        if let Some(text) = t.expect_text {
            let found = (1..=km.map_size()).any(|i| {
                km.get_0c(&format!("Warning_{i}"))
                    .ok()
                    .flatten()
                    .is_some_and(|v| v.contains(text))
            });
            if !found {
                return Err(format!("warning text does not contain '{text}'"));
            }
        }
        return Ok(());
    }

    if let (Some(enc), Ok(Some(obj))) = (t.write_enc, read) {
        let mut wfc = FitsChan::new().unwrap();
        wfc.set_encoding(Encoding::from_name(enc).expect("a known encoding"));
        let nw = wfc.write(&obj).map_err(|e| e.to_string())?;
        if nw != 0 {
            return Err(format!("write to {enc} returned {nw}, expected 0"));
        }
        wfc.empty_fits().map_err(|e| e.to_string())?;
        wfc.set_encoding(Encoding::FitsWcs);
        let nw = wfc.write(&obj).map_err(|e| e.to_string())?;
        if nw != 1 {
            return Err(format!("write to FITS-WCS returned {nw}, expected 1"));
        }
    }
    Ok(())
}

#[test]
fn bad_headers_match_the_c_library() {
    let failures: Vec<String> = cases::CASES
        .iter()
        .filter(|t| !FITSCHAN_ERRORS_EXCLUDED.iter().any(|(n, _)| *n == t.name))
        .filter(|t| {
            !FITSCHAN_ERRORS_GAP_EXCLUDED
                .iter()
                .any(|(n, _)| *n == t.name)
        })
        .filter_map(|t| check_case(t).err().map(|e| format!("{}: {e}", t.name)))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn excluded_cases_are_in_the_c_table() {
    for (name, reason) in FITSCHAN_ERRORS_EXCLUDED {
        assert!(
            cases::CASES.iter().any(|t| t.name == *name),
            "{name} is excluded ({reason}) but is not a row of C's table"
        );
    }
}

#[test]
fn port_gap_cases_still_differ() {
    for (name, reason) in FITSCHAN_ERRORS_GAP_EXCLUDED {
        let t = cases::CASES
            .iter()
            .find(|t| t.name == *name)
            .unwrap_or_else(|| panic!("{name} is not a row of C's table"));
        assert!(
            check_case(t).is_err(),
            "{name} now matches C; remove it from FITSCHAN_ERRORS_GAP_EXCLUDED. Reason on record: {reason}"
        );
    }
}
