// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! What each FITS encoding writes for a set of headers, against C's: each
//! header is read into a FrameSet, which is written back to the same
//! FitsChan with each encoding in turn, some cases first setting the
//! FrameSet's Epoch. Each encoding's writer (fitschan.c
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

use starlink_ast::frames::{Frame, SkyFrame, SkyRefIs};
use starlink_ast::{AnyObject, Channel, Encoding, FitsChan, NativeChannel};

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

/// The SkyFrame that is the current Frame of a FrameSet.
fn sky(f: &mut dyn Frame) -> &mut SkyFrame {
    f.as_any_mut().downcast_mut::<SkyFrame>().unwrap()
}

/// The header written with `enc`: the value the write returns, Ncard, the
/// cards not used, and the name of every card.
fn written(header: &[&str], epoch: &str, enc: &str) -> (i32, i32, Vec<String>, Vec<String>) {
    let mut fc = FitsChan::new().unwrap();
    for card in header {
        fc.put_fits(card, false).unwrap();
    }
    fc.clear_card().unwrap();
    let mut fs = fc.read().unwrap();
    if !epoch.is_empty()
        && let Some(AnyObject::FrameSet(fs)) = fs.as_mut()
    {
        if let Some(skyref) = epoch.strip_prefix("SKYREF ") {
            // SkyRef(1), SkyRef(2) and SkyRefIs, each set through the
            // FrameSet as C's astSet sets them, one at a time.
            let words: Vec<&str> = skyref.split_whitespace().collect();
            let lon: f64 = words[0].parse().unwrap();
            let lat: f64 = words[1].parse().unwrap();
            let is = match words[2] {
                "Origin" => SkyRefIs::Origin,
                "Pole" => SkyRefIs::Pole,
                other => panic!("SkyRefIs {other} is not handled"),
            };

            fs.with_integrity(|f| sky(f).set_sky_ref(1, lon)).unwrap();
            fs.with_integrity(|f| sky(f).set_sky_ref(2, lat)).unwrap();
            fs.with_integrity(|f| {
                sky(f).set_sky_ref_is(is);
                Ok(())
            })
            .unwrap();
        } else {
            let mjd: f64 = epoch.strip_prefix("MJD ").unwrap().parse().unwrap();
            fs.set_epoch_mjd(mjd).unwrap();
        }
    }
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
const EXCLUDED: &[(&str, &str, &str)] = &[];

fn excluded(name: &str, enc: &str) -> bool {
    EXCLUDED.iter().any(|&(n, e, _)| n == name && e == enc)
}

/// `(header, encoding, keywords, reason)` for each write whose cards match
/// C's but for the values of the named keywords, which are compared with
/// their values set aside: every card name, the card order, every comment
/// and every other value are still compared byte for byte.
/// `licensed_values_still_differ` fails when one starts to match.
const VALUES_LICENSED: &[(&str, &str, &[&str], &str)] = &[];

/// `cards` with the value of each card whose keyword is in `keywords`
/// replaced by a marker, keeping its keyword and comment.
fn without_values(cards: &[String], keywords: &[&str]) -> Vec<String> {
    cards
        .iter()
        .map(|card| {
            let key = card[..8].trim_end();
            if keywords.contains(&key) {
                let comment = card.find(" / ").map_or("", |i| &card[i..]);
                format!("{:<8}= <licensed>{comment}", key)
            } else {
                card.clone()
            }
        })
        .collect()
}

/// (header, encoding) writes whose FrameSet runs SphMap's forward transform,
/// which is within 1 ULP of C per arctangent, so their numbers differ from
/// C's by rounding; their cards are compared by `sphmap_drift_text_eq`.
const SPHMAP_DRIFT_LICENSED: &[(&str, &str)] = &[
    ("tan_offset_origin", "FITS-WCS"),
    ("tan_offset_pole", "FITS-WCS"),
];

/// The most a card value of a write in [`SPHMAP_DRIFT_LICENSED`] moved from
/// C: 1 ULP (`PC2_1 = -0.215` of `tan_offset_pole`), or 1.3e-17 for
/// `PC2_1 = -0.0016` of `tan_offset_origin`, a small difference of terms of
/// order 1 (2026-10-28).
const MEASURED_BOUND: crate::common::DriftTol = crate::common::DriftTol {
    ulps: 1,
    atol: 2.0e-17,
};

/// Every write of every case, `(header, encoding, differs, report)`.
fn compare_all() -> Vec<(&'static str, &'static str, bool, String)> {
    let mut out = Vec::new();
    for &(name, header, epoch, encodings) in cases::CASES {
        for &(enc, ret, ncard, cards, names) in encodings {
            let got = written(header, epoch, enc);
            let want = (
                ret,
                ncard,
                cards.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
                names.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
            );
            let report = format!("{name} {enc}:\n--- rust\n{got:#?}\n--- C\n{want:#?}");
            let drift = SPHMAP_DRIFT_LICENSED.contains(&(name, enc));
            let text_eq = |g: &[String], w: &[String]| {
                let (g, w) = (g.join("\n"), w.join("\n"));
                if drift {
                    crate::common::sphmap_drift_text_eq(&g, &w, MEASURED_BOUND)
                } else {
                    crate::common::c_text_eq(&g, &w)
                }
            };
            let differs = match VALUES_LICENSED
                .iter()
                .find(|&&(n, e, _, _)| n == name && e == enc)
            {
                Some(&(_, _, keys, _)) => {
                    (got.0, got.1, &got.3) != (want.0, want.1, &want.3)
                        || if drift {
                            !text_eq(
                                &without_values(&got.2, keys),
                                &without_values(&want.2, keys),
                            )
                        } else {
                            without_values(&got.2, keys) != without_values(&want.2, keys)
                        }
                }
                None => {
                    (got.0, got.1, &got.3) != (want.0, want.1, &want.3) || !text_eq(&got.2, &want.2)
                }
            };
            // A licensed write that now matches C exactly is reported, so the
            // list cannot go stale.
            let (differs, report) = if crate::common::on_capture_platform() && drift && got == want
            {
                (
                    true,
                    format!(
                        "{name} {enc}: matches C exactly; remove it from SPHMAP_DRIFT_LICENSED"
                    ),
                )
            } else {
                (differs, report)
            };
            out.push((name, enc, differs, report));
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

#[test]
fn licensed_values_still_differ() {
    for &(name, enc, _, reason) in VALUES_LICENSED {
        let &(_, header, epoch, encodings) = cases::CASES
            .iter()
            .find(|(n, ..)| *n == name)
            .expect("a licensed case is in the corpus");
        let &(_, _, _, cards, _) = encodings
            .iter()
            .find(|(e, ..)| *e == enc)
            .expect("a licensed encoding is in the case");
        let got = written(header, epoch, enc);
        let want: Vec<String> = cards.iter().map(|c| c.to_string()).collect();
        assert_ne!(
            got.2, want,
            "{name} {enc} now matches C ({reason}); remove its licence"
        );
    }
}
