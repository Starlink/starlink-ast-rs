// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A dump cannot make the port allocate more memory, or do more work, than
//! its size warrants.
//!
//! Each count read from a dump is held to the limit the C library sets on
//! the counts of its own hardened loaders, and the items of a large Object
//! are found without searching all of them for each one.

use std::time::Duration;

use starlink_ast::error::{AstError, ErrorCode};
use starlink_ast::frames::{BasicFrameBuilder, Frame};
use starlink_ast::mappings::LutMap;
use starlink_ast::plot::Plot;
use starlink_ast::regions::CircleBuilder;
use starlink_ast::{AnyObject, from_native_string};

/// One more than the most values a dump can name: C's `MXLOADEL`.
const TOO_MANY: i64 = 10_000_000;

/// Run `f` on another thread and return what it gives, or an error if it
/// takes more than ten seconds or panics.
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

/// An Object with many items loads in time proportional to their number.
#[test]
fn a_large_object_loads_in_linear_time() {
    let lut: Vec<f64> = (0..200_000).map(f64::from).collect();
    let dump = LutMap::new(&lut, 1.0, 1.0)
        .unwrap()
        .to_native_string()
        .unwrap();
    within_10s(move || from_native_string(&dump).unwrap()).unwrap();
}

/// The error reading `dump` gives, within ten seconds.
fn refusal(dump: String) -> AstError {
    match within_10s(move || from_native_string(&dump).map(|o| o.class_name().to_string())) {
        Ok(Ok(class)) => panic!("read a {class}"),
        Ok(Err(e)) => e,
        Err(why) => panic!("{why}"),
    }
}

/// `dump` is refused because it claims more values than a dump can name.
fn assert_too_many(dump: String) {
    let err = refusal(dump);
    assert_eq!(err.code(), ErrorCode::Lderr, "{err}");
    assert!(err.to_string().contains("9999999"), "{err}");
}

/// `dump` with `item` added just before `marker`.
fn with_item(dump: &str, marker: &str, item: &str) -> String {
    assert!(dump.contains(marker), "{marker}");
    dump.replacen(marker, &format!(" {item}\n{marker}"), 1)
}

#[test]
fn a_lutmap_of_too_many_entries_is_refused() {
    assert_too_many(format!(
        " Begin LutMap\n Nin = 1\n IsA Mapping\n Nlut = {TOO_MANY}\n Start = 1\n Incr = 1\n \
         L1 = 0\n L2 = 1\n End LutMap\n"
    ));
}

#[test]
fn a_polymap_of_too_many_coefficients_is_refused() {
    assert_too_many(format!(
        " Begin PolyMap\n Nin = 1\n IsA Mapping\n MPF1 = 1\n NCF1 = {TOO_MANY}\n End PolyMap\n"
    ));
}

#[test]
fn a_polymap_of_too_high_a_power_is_refused() {
    assert_too_many(format!(
        " Begin PolyMap\n Nin = 1\n IsA Mapping\n MPF1 = {TOO_MANY}\n NCF1 = 1\n \
         CF1 = 1\n PF1 = 1\n End PolyMap\n"
    ));
}

#[test]
fn a_splinemap_of_too_many_coefficients_is_refused() {
    assert_too_many(
        " Begin SplineMap\n Nin = 2\n IsA Mapping\n KX = 3\n KY = 3\n NX = 4000\n \
         NY = 4000\n End SplineMap\n"
            .to_string(),
    );
}

#[test]
fn a_full_or_diagonal_matrix_of_too_many_elements_is_refused() {
    for form in ["Unit", "Diagonal"] {
        assert_too_many(format!(
            " Begin MatrixMap\n Nin = 3163\n Nout = 3162\n IsA Mapping\n \
             Form = \"{form}\"\n End MatrixMap\n"
        ));
    }
}

#[test]
fn a_keymap_entry_of_too_many_elements_is_refused() {
    assert_too_many(format!(
        " Begin KeyMap\n IsA Object\n Key1 = \"a\"\n Typ1 = 2\n Nel1 = {TOO_MANY}\n \
         End KeyMap\n"
    ));
}

#[test]
fn a_table_of_too_many_rows_is_refused() {
    assert_too_many(format!(
        " Begin Table\n IsA KeyMap\n Nrow = {TOO_MANY}\n End Table\n"
    ));
}

#[test]
fn a_plot_of_too_many_tick_marks_is_refused() {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let plot = Plot::new(&frame, [0.0, 0.0, 1.0, 1.0], [0.0, 0.0, 1.0, 1.0]).unwrap();
    let dump = AnyObject::Plot(Box::new(plot)).to_native_string().unwrap();
    for item in ["nmjtk1", "nmntk1"] {
        assert_too_many(with_item(
            &dump,
            " End Plot",
            &format!("{item} = {TOO_MANY}"),
        ));
    }
}

#[test]
fn a_region_of_too_large_a_mesh_is_refused() {
    let circle = CircleBuilder::new(BasicFrameBuilder::new(2).build().unwrap(), vec![0.0, 0.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap();
    let dump = AnyObject::Circle(circle).to_native_string().unwrap();
    assert_too_many(with_item(
        &dump,
        " IsA Region",
        &format!("MeshSz = {TOO_MANY}"),
    ));
}

/// A value formatted with `format` on a one-axis Frame with `digits`,
/// within ten seconds.
fn formatted(format: &'static str, digits: Option<i32>) -> Result<String, AstError> {
    within_10s(move || {
        let mut frame = BasicFrameBuilder::new(1).build().unwrap();
        if let Some(digits) = digits {
            frame.set_digits(digits);
        }
        frame.set_format_str(1, format).unwrap();
        frame.format(1, 0.5)
    })
    .unwrap()
}

/// A Format whose field width or precision makes its result longer than
/// the C library's buffer fails as C's does, without the result being made.
#[test]
fn a_format_too_wide_or_precise_is_refused_before_formatting() {
    for format in [
        "%99999999.1f",
        "%.99999999f",
        "%.99999999e",
        "%.99999999d",
        "%#.99999999g",
        "%.99999999a",
    ] {
        let err = formatted(format, None).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Fmter, "{format}: {err}");
        assert!(
            err.to_string().contains("exceeds 127 characters"),
            "{format}: {err}"
        );
    }
    let err = formatted("%.*f", Some(99_999_999)).unwrap_err();
    assert!(err.to_string().contains("exceeds 127 characters"), "{err}");
    // C's snprintf fails for a width an int cannot hold.
    let err = formatted("%9999999999.1f", None).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Fmter, "{err}");
    assert!(err.to_string().contains("too large for an int"), "{err}");
}

/// `%g` without the `#` flag drops trailing zeros, so a huge precision
/// gives a short result.
#[test]
fn a_huge_g_precision_gives_the_shortest_result() {
    assert_eq!(formatted("%.99999999g", None).unwrap(), "0.5");
    assert_eq!(formatted("%.*G", Some(99_999_999)).unwrap(), "0.5");
}
