//! What each FITS encoding writes for a set of headers, against C's: each
//! header is read into a FrameSet, which is written back to the same
//! FitsChan with each encoding in turn. Each encoding's writer (fitschan.c
//! `WcsFromStore`, `PCFromStore`, `IRAFFromStore`, `AIPSFromStore`,
//! `AIPSPPFromStore`, `CLASSFromStore`, `DSSFromStore`) writes nothing for a
//! description it cannot hold, and the write then leaves every card the
//! FitsChan had, those the read consumed included.
//!
//! For each header and encoding, compared with C: the value the write
//! returns, Ncard, the cards not used, and the name of every card, from a
//! dump of the FitsChan. `fitschan_encodings_oracle/cases.rs` is generated
//! by `tools/oracle_fixtures/fitschan_encodings_probe.c`.
//!
//! [`EXCLUDED`] names the writes that still differ, each with its reason;
//! `excluded_writes_still_differ` fails when one of them starts matching.

#[rustfmt::skip]
#[path = "fitschan_encodings_oracle/cases.rs"]
mod cases;

use ast_core::{AnyObject, Channel, Encoding, FitsChan, NativeChannel};

fn encoding(name: &str) -> Encoding {
    match name {
        "FITS-WCS" => Encoding::FitsWcs,
        "FITS-PC" => Encoding::FitsPc,
        "FITS-IRAF" => Encoding::FitsIraf,
        "FITS-AIPS" => Encoding::FitsAips,
        "FITS-AIPS++" => Encoding::FitsAipsPP,
        "FITS-CLASS" => Encoding::FitsClass,
        "DSS" => Encoding::Dss,
        other => panic!("no encoding {other}"),
    }
}

/// The header written with `enc`: the value the write returns, Ncard, the
/// cards not used, and the name of every card.
fn written(header: &[&str], enc: &str) -> (i32, i32, Vec<String>, Vec<String>) {
    let mut fc = FitsChan::new();
    for card in header {
        fc.put_fits(card, false).unwrap();
    }
    fc.clear_card().unwrap();
    let fs = fc.read().unwrap();
    fc.set_encoding(encoding(enc));
    let ret = match &fs {
        Some(fs) => fc.write(fs).map_or(0, |n| n as i32),
        None => -1,
    };
    let ncard = fc.ncard().unwrap() as i32;
    fc.clear_card().unwrap();
    let mut cards = Vec::new();
    while let Some(card) = fc.find_fits("%f", true).unwrap() {
        cards.push(card);
    }
    let mut ch = NativeChannel::with_string_sink();
    ch.write(&AnyObject::FitsChan(Box::new(fc))).unwrap();
    let names = ch
        .into_string()
        .lines()
        .filter(|l| l.starts_with("    Nm"))
        .filter_map(|l| l.split('"').nth(1).map(str::to_string))
        .collect();
    (ret, ncard, cards, names)
}

/// `(header, encoding, reason)` for each write that does not yet match C.
const EXCLUDED: &[(&str, &str, &str)] = &[
    ("class_cube", "FITS-WCS", NO_VELOSYS),
    ("class_lonpole_180", "FITS-WCS", NO_VELOSYS),
    ("class_lonpole_90", "FITS-WCS", NO_VELOSYS),
];

/// C's `SpectralAxes` (fitschan.c:31208) always stores VELOSYS, the
/// topocentric velocity of the standard of rest; the port's
/// `spectral_axes` does not.
const NO_VELOSYS: &str = "port's spectral_axes writes no VELOSYS";

fn excluded(name: &str, enc: &str) -> bool {
    EXCLUDED.iter().any(|&(n, e, _)| n == name && e == enc)
}

/// Every write of every case, `(header, encoding, differs, report)`.
fn compare_all() -> Vec<(&'static str, &'static str, bool, String)> {
    let mut out = Vec::new();
    for &(name, header, encodings) in cases::CASES {
        for &(enc, ret, ncard, cards, names) in encodings {
            let got = written(header, enc);
            let want = (
                ret,
                ncard,
                cards.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
                names.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
            );
            let report = format!("{name} {enc}:\n--- rust\n{got:#?}\n--- C\n{want:#?}");
            out.push((name, enc, got != want, report));
        }
    }
    out
}

#[test]
fn writes_match_c() {
    let failed: Vec<String> = compare_all()
        .into_iter()
        .filter(|(name, enc, differs, _)| *differs && !excluded(name, enc))
        .map(|(_, _, _, report)| report)
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn excluded_writes_still_differ() {
    let all = compare_all();
    for &(name, enc, reason) in EXCLUDED {
        let entry = all.iter().find(|(n, e, _, _)| *n == name && *e == enc);
        let Some((_, _, differs, _)) = entry else {
            panic!("EXCLUDED names {name} {enc}, which is not a case");
        };
        assert!(
            *differs,
            "{name} {enc} now matches C, delete its exclusion ({reason})"
        );
    }
}
