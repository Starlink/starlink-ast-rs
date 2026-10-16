//! Where a Mapping with its `Report` attribute set writes the effect of
//! each transformation (mapping.c `ReportPoints`).
//!
//! C prints the report to standard output. Here it goes to a writer that
//! can be replaced, standard output by default. The writer is per thread,
//! so reporting takes no lock: Mappings are `Send` but not `Sync`, and each
//! thread reports to its own writer.

use std::cell::RefCell;
use std::io::Write;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;

thread_local! {
    static REPORT_SINK: RefCell<Option<Box<dyn Write>>> = const { RefCell::new(None) };
}

/// Send this thread's transformation reports to `writer` instead of
/// standard output.
pub fn set_report_sink(writer: Box<dyn Write>) {
    REPORT_SINK.with_borrow_mut(|sink| *sink = Some(writer));
}

/// Send this thread's transformation reports to standard output again,
/// returning the writer that was receiving them, if one was set.
pub fn reset_report_sink() -> Option<Box<dyn Write>> {
    REPORT_SINK.with_borrow_mut(Option::take)
}

/// Write `text` to this thread's report writer. As with C's `printf`, a
/// failure to write is not reported.
pub(crate) fn write_report(text: &str) {
    REPORT_SINK.with_borrow_mut(|sink| match sink {
        Some(writer) => {
            let _ = writer.write_all(text.as_bytes());
        }
        None => {
            let _ = std::io::stdout().write_all(text.as_bytes());
        }
    });
}

/// One line per point, `(in, ...) --> (out, ...)`, each value written by
/// `fmt_in` or `fmt_out` given its 0-based coordinate index; the shorter of
/// the two point counts is reported (mapping.c `ReportPoints`).
pub(crate) fn report_lines(
    inp: &PointSet,
    out: &PointSet,
    fmt_in: impl Fn(usize, f64) -> String,
    fmt_out: impl Fn(usize, f64) -> String,
) -> String {
    let mut text = String::new();
    for p in 0..inp.npoint().min(out.npoint()) {
        let side = |pt: &[f64], fmt: &dyn Fn(usize, f64) -> String| {
            pt.iter()
                .enumerate()
                .map(|(c, &v)| fmt(c, v))
                .collect::<Vec<_>>()
                .join(", ")
        };
        text.push('(');
        text.push_str(&side(&inp.point(p), &fmt_in));
        text.push_str(") --> (");
        text.push_str(&side(&out.point(p), &fmt_out));
        text.push_str(")\n");
    }
    text
}

/// A value as mapping.c `ReportPoints` writes it: `<bad>`, or `%.*g` with
/// `AST__DBL_DIG` (17) digits.
pub(crate) fn plain_value(_coord: usize, value: f64) -> String {
    if value == BAD_VALUE {
        "<bad>".to_string()
    } else {
        crate::serial::native_write::format_double_g_signed(
            value,
            crate::serial::native_write::DBL_DIG,
        )
    }
}
