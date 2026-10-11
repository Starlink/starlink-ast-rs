//! The MOC string, JSON and data forms of a Moc against the C library
//! (moc.c `AddMocString`, `astAddMocText`, `GetMocString`, `astGetMocText`,
//! `AddMocData`, `GetMocData`, `GetMocHeader`): MOCs read from string and
//! JSON text (each combination mode, negated, with MaxOrder given, found
//! from the text or already set, degrading cells above it, an empty JSON
//! MOC, and text with tabs, newlines and spaces) and from NUNIQ data (4 and
//! 8 byte values, degraded, negated), each compared by MaxOrder, MocLength,
//! both text forms, MocType and data; the forms of an empty Moc; the pieces
//! `get_moc_text` passes to its sink with a small buffer; the FITS header;
//! and the errors for malformed text and data, which leave the Moc as it
//! was.
//!
//! C carries on writing after AST__SMBUF, and its own sinks ignore what
//! follows because the status is set; the port stops at the error, so the
//! pieces are compared up to it.
//!
//! The values come from `tools/oracle_fixtures/region_moc_text_probe.c`, run
//! against the library with moc.c fixed upstream (7beab7b1, 7d06331e).

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_moc_text_oracle/cases.rs"]
mod cases;

use ast_core::error::ErrorCode;
use ast_core::regions::{CmpOperator, Moc, MocData};

fn ordered(maxorder: Option<i32>) -> Moc {
    let mut moc = Moc::new();
    if let Some(order) = maxorder {
        moc.set_max_order(order).unwrap();
    }
    moc
}

fn low() -> Moc {
    let mut moc = Moc::new();
    for (cmode, order, npix) in [
        (CmpOperator::Or, 2, 37),
        (CmpOperator::Or, 1, 3),
        (CmpOperator::Or, 2, 100),
        (CmpOperator::Xor, 1, 25),
        (CmpOperator::Or, 0, 7),
    ] {
        moc.add_cell(cmode, order, npix).unwrap();
    }
    moc
}

fn high() -> Moc {
    let mut moc = Moc::new();
    moc.add_cell(CmpOperator::Or, 15, 1234567890).unwrap();
    moc.add_cell(CmpOperator::Or, 10, 5).unwrap();
    moc.add_cell(CmpOperator::Or, 3, 700).unwrap();
    moc
}

fn text(
    maxorder: Option<i32>,
    cmode: CmpOperator,
    negate: bool,
    mxord: i32,
    t: &str,
    base: Option<&Moc>,
) -> Moc {
    let mut moc = base.cloned().unwrap_or_else(|| ordered(maxorder));
    moc.add_moc_string(cmode, negate, mxord, t).unwrap();
    moc
}

fn data(moc: &Moc) -> Vec<i64> {
    match moc.get_moc_data().unwrap() {
        MocData::Int32(v) => v.into_iter().map(i64::from).collect(),
        MocData::Int64(v) => v,
    }
}

/// Each case built as the probe builds it, in the probe's order.
fn built() -> Vec<(&'static str, Moc)> {
    use CmpOperator::{And, Or, Xor};
    let low = low();
    let high = high();
    let mut out = vec![("low", low.clone()), ("high", high.clone())];
    out.push((
        "string",
        text(None, Or, false, -1, "3/1-5,9 4/100-103 5/", None),
    ));
    out.push((
        "string_degraded",
        text(None, Or, false, 4, "2/3 6/1000-1010,2000", None),
    ));
    out.push((
        "json",
        text(
            None,
            Or,
            false,
            -1,
            "{\"1\":[1,2,4], \"3\":[ 100 , 101 ],\"5\":[]}",
            None,
        ),
    ));
    out.push(("json_empty", text(Some(3), Or, false, -1, "{}", None)));
    out.push((
        "json_spaced",
        text(
            None,
            Or,
            false,
            -1,
            " {\n \"2\" : [ 7 ,8 ],\n\t\"4\":[1]\n}\n",
            None,
        ),
    ));
    out.push(("negated", text(Some(3), Or, true, -1, "2/5-10", None)));
    out.push(("and", text(None, And, false, -1, "1/0-20", Some(&low))));
    out.push((
        "xor",
        text(None, Xor, false, -1, "1/0-20 2/150", Some(&low)),
    ));
    out.push((
        "whitespace",
        text(None, Or, false, -1, "  1/3\t\n2/40-45, 47", None),
    ));
    out.push((
        "maxorder_set",
        text(Some(2), Or, false, 5, "4/1000 1/7", None),
    ));
    out.push((
        "low_text",
        text(
            None,
            Or,
            false,
            -1,
            "{\"0\":[7],\"1\":[3,25],\"2\":[37,100]}",
            None,
        ),
    ));

    let MocData::Int32(idata) = low.get_moc_data().unwrap() else {
        panic!("the low Moc has 4 byte data");
    };
    let MocData::Int64(kdata) = high.get_moc_data().unwrap() else {
        panic!("the high Moc has 8 byte data");
    };
    let mut m = ordered(None);
    m.add_moc_data(Or, false, -1, &idata).unwrap();
    out.push(("data_4", m));
    let mut m = ordered(None);
    m.add_moc_data(Or, false, -1, &kdata).unwrap();
    out.push(("data_8", m));
    let mut m = ordered(None);
    m.add_moc_data(Or, false, 2, &kdata).unwrap();
    out.push(("data_degraded", m));
    let mut m = low.clone();
    m.add_moc_data(Xor, true, -1, &idata).unwrap();
    out.push(("data_negated_xor", m));
    let mut m = low.clone();
    m.add_moc_data::<i32>(And, false, -1, &[]).unwrap();
    out.push(("data_and_empty", m));
    out
}

#[test]
fn text_and_data_forms_match_c() {
    let got = built();
    assert_eq!(got.len(), cases::CASES.len());
    for ((name, moc), case) in got.iter().zip(cases::CASES) {
        let (want_name, maxorder, length, string, json, mtype, values) = *case;
        assert_eq!(*name, want_name);
        assert_eq!(moc.max_order(), maxorder, "{name}: MaxOrder");
        assert_eq!(
            moc.moc_length().unwrap() as i32,
            length,
            "{name}: MocLength"
        );
        assert_eq!(
            moc.get_moc_string(false).unwrap(),
            string,
            "{name}: string form"
        );
        assert_eq!(moc.get_moc_string(true).unwrap(), json, "{name}: JSON form");
        assert_eq!(moc.moc_type().unwrap(), mtype, "{name}: MocType");
        assert_eq!(data(moc), values, "{name}: data");
    }
}

#[test]
fn reading_reports_json() {
    let mut moc = Moc::new();
    assert!(
        moc.add_moc_string(CmpOperator::Or, false, -1, " {\"1\":[2]}")
            .unwrap()
    );
    assert!(
        !moc.add_moc_string(CmpOperator::Or, false, -1, "1/3")
            .unwrap()
    );
}

#[test]
fn empty_forms_match_c() {
    let got = [
        ordered(None).get_moc_string(false).unwrap(),
        ordered(None).get_moc_string(true).unwrap(),
        ordered(Some(4)).get_moc_string(false).unwrap(),
        ordered(Some(4)).get_moc_string(true).unwrap(),
    ];
    assert_eq!(got, cases::EMPTY);
}

fn pieces(moc: &Moc, json: bool, buflen: usize) -> (Vec<String>, Option<ErrorCode>) {
    let mut out = Vec::new();
    let result = moc.get_moc_text(
        json,
        buflen,
        &mut |piece| {
            out.push(piece.to_string());
            Ok(())
        },
        "probe",
    );
    (out, result.err().map(|e| e.code()))
}

#[test]
fn pieces_match_c() {
    for (json, (want, status)) in [(false, cases::PIECES_STRING), (true, cases::PIECES_JSON)] {
        let (got, err) = pieces(&high(), json, 16);
        assert_eq!(err.map_or(0, |c| c as i32), status, "json={json}: status");
        // After an error C passes on what remains; its sinks ignore it.
        assert_eq!(got.as_slice(), &want[..got.len()], "json={json}: pieces");
        if err.is_none() {
            assert_eq!(got.len(), want.len(), "json={json}: piece count");
        }
    }
    let (_, err) = pieces(&high(), false, 3);
    assert_eq!(err.map_or(0, |c| c as i32), cases::SMALL_BUFFER);
}

#[test]
fn headers_match_c() {
    for (moc, want) in [(low(), cases::HEADER_LOW), (high(), cases::HEADER_HIGH)] {
        // to_header ends with the END card a FitsChan built from setters
        // lacks; C's card-by-card read does not see one.
        let header = moc.get_moc_header().unwrap().to_header().unwrap();
        let got: Vec<&str> = header
            .lines()
            .filter(|card| card.trim_end() != "END")
            .collect();
        assert_eq!(got, want);
    }
}

#[test]
fn malformed_text_is_reported_and_leaves_the_moc() {
    let low = low();
    for &(t, status, unchanged) in cases::BAD_TEXT {
        let mut m = low.clone();
        let err = m.add_moc_string(CmpOperator::Or, false, -1, t).err();
        assert_eq!(err.map_or(0, |e| e.code() as i32), status, "{t:?}: status");
        assert!(unchanged, "{t:?}: C changed the Moc");
        assert_eq!(
            m.get_moc_string(false).unwrap(),
            low.get_moc_string(false).unwrap(),
            "{t:?}"
        );
        assert_eq!(m.max_order(), low.max_order(), "{t:?}: MaxOrder");
    }
}

#[test]
fn malformed_data_is_reported_and_leaves_the_moc() {
    let mut m = ordered(Some(5));
    let err = m
        .add_moc_data(CmpOperator::Or, false, -1, &[20_i32, 0])
        .unwrap_err();
    assert_eq!(err.code() as i32, cases::BAD_DATA_4);
    let err = m
        .add_moc_data(CmpOperator::Or, false, -1, &[20_i64, -5])
        .unwrap_err();
    assert_eq!(err.code() as i32, cases::BAD_DATA_8);
    assert_eq!(m.moc_length().unwrap() as i32, cases::BAD_DATA_LENGTH);
}
