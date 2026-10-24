//! FitsChan's SipOK attribute against the C library: a FrameSet with SIP
//! distortion, read from `wcsconv/inputs/sip.head`, written with every
//! encoding and SipOK unset, false and true. Only FITS-WCS writes SIP
//! coefficients; with SipOK false, or another encoding, the write uses a
//! linear approximation if FitsTol allows one.
//!
//! The C side is `tools/oracle_fixtures/fitschan_sipok_probe.c`, which needs
//! the C library fixed upstream in bdc6336c (see c-library-quirks.md). Each
//! case is compared byte for byte: `astWrite`'s return value, then every card.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "fitschan_sipok_oracle/cases.rs"]
mod cases;

use starlink_ast::{Channel, Encoding, FitsChan};

/// Encodings whose write the port does not yet match, with the reason. The
/// companion test fails once an entry matches, so the list cannot go stale.
const EXCLUDED: &[(&str, &str)] = &[];

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

/// What the port writes for one case, in the probe's format.
fn run(enc: &str, sip_ok: &str) -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wcsconv/inputs/sip.head"
    );
    let mut fin = FitsChan::new().unwrap();
    fin.set_sip_replace(false);
    for line in std::fs::read_to_string(path).unwrap().lines() {
        fin.put_fits(line, false).unwrap();
    }
    fin.clear_card().unwrap();
    let obj = fin.read().unwrap().expect("sip.head holds a FrameSet");

    let mut out = FitsChan::new().unwrap();
    out.set_encoding(encoding(enc));
    match sip_ok {
        "unset" => {}
        "0" => out.set_sip_ok(false),
        "1" => out.set_sip_ok(true),
        other => panic!("no SipOK {other}"),
    }
    let n = out.write(&obj).map_or(-1, |n| n as i64);
    let mut got = format!("wrote {n}\n");
    out.clear_card().unwrap();
    while let Some(card) = out.find_fits("%f", true).unwrap() {
        got.push_str(&card);
        got.push('\n');
    }
    got
}

fn excluded(enc: &str) -> bool {
    EXCLUDED.iter().any(|(e, _)| *e == enc)
}

#[test]
fn sip_ok_writes_match_the_c_library() {
    let mut failed = Vec::new();
    for &(enc, sip_ok, want) in cases::CASES {
        if excluded(enc) {
            continue;
        }
        let got = run(enc, sip_ok);
        if got != want {
            failed.push(format!(
                "{enc} SipOK={sip_ok}\n--- rust\n{got}--- C\n{want}"
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn excluded_encodings_still_differ() {
    for &(enc, reason) in EXCLUDED {
        let all_match = cases::CASES
            .iter()
            .filter(|(e, _, _)| *e == enc)
            .all(|&(_, sip_ok, want)| run(enc, sip_ok) == want);
        assert!(
            !all_match,
            "{enc} now matches C; remove it from EXCLUDED ({reason})"
        );
    }
}

#[test]
fn sip_ok_accessors() {
    let mut fc = FitsChan::new().unwrap();
    assert!(fc.sip_ok());
    assert!(!fc.test_sip_ok());
    fc.set_sip_ok(false);
    assert!(!fc.sip_ok());
    assert!(fc.test_sip_ok());
    fc.clear_sip_ok();
    assert!(fc.sip_ok());
    assert!(!fc.test_sip_ok());
}
