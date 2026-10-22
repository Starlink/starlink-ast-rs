//! FitsChan card functions against the C library: how `put_fits`,
//! `put_cards`, `find_fits`, `del_fits`, `empty_fits`, the typed getters and
//! setters, `read` and `write` use and move the current card, and the `Card`,
//! `CardName`, `CardType`, `CardComm`, `Ncard` and `Nkey` attributes.
//!
//! Each case is a script of operations run through the C library by
//! `tools/oracle_fixtures/fitschan_cursor_probe.c`, which records the result
//! of each and the state after it; the same script is run here through the
//! public API and the two transcripts compared byte for byte. The probe's
//! header describes the operations.

use crate::common::native_dump;
// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "fitschan_cursor_oracle/cases.rs"]
mod cases;

use ast_core::channel::{Sink, Source};
use ast_core::error::AstError;
use ast_core::frames::FrameRef;
use ast_core::mappings::ZoomMap;
use ast_core::{AltAxes, AnyObject, Channel, Encoding, FitsChan};
use std::sync::{Arc, Mutex};

/// The probe's source function: the cards of `NEW|c1~c2~#~c3`, `#` ending
/// a batch.
struct ScriptSource(String);

impl Source for ScriptSource {
    fn read_line(&mut self) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        if let Some(rest) = self.0.strip_prefix('#') {
            self.0 = rest.strip_prefix('~').unwrap_or(rest).to_string();
            return None;
        }
        let (card, rest) = self.0.split_once('~').unwrap_or((&self.0, ""));
        let card = card.to_string();
        self.0 = rest.to_string();
        Some(card)
    }
}

/// The probe's sink function, printing each card as `|sink card`.
struct LogSink(Arc<Mutex<String>>);

impl Sink for LogSink {
    fn write_line(&mut self, line: &str) {
        self.0.lock().unwrap().push_str(&format!("|sink {line}\n"));
    }
}

/// What a case's operations share: the sink's output and the SinkFile.
#[derive(Default)]
struct Harness {
    sink: Arc<Mutex<String>>,
    sink_name: Option<String>,
}

/// The path of `name` in the temporary directory, as the probe's `tmp_path`.
fn tmp_path(name: &str) -> String {
    let dir = std::env::var("TMPDIR")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/tmp".to_string());
    format!("{dir}/{name}")
}

/// Each line of the temporary file `name` as `|file line`.
fn cat_file(name: &str) -> String {
    match std::fs::read_to_string(tmp_path(name)) {
        Ok(text) => text.lines().map(|l| format!("|file {l}\n")).collect(),
        Err(_) => "|nofile\n".to_string(),
    }
}

/// C's name for an error code, as the probe prints it.
fn code_name(e: &AstError) -> &'static str {
    match format!("{:?}", e.code()).as_str() {
        "Bdfts" => "Bdfts",
        "Ftcnv" => "Ftcnv",
        "Fundef" => "Fundef",
        "Bdfmt" => "Bdfmt",
        "Attin" => "Attin",
        "Rderr" => "Rderr",
        "Wrerr" => "Wrerr",
        _ => "Other",
    }
}

/// C's `%.17g`.
fn c_g17(v: f64) -> String {
    if v == 0.0 {
        return if v.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    let e = format!("{v:.16e}");
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let strip = |s: &str| {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s.to_string()
        }
    };
    if !(-4..17).contains(&exp) {
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{sign}{:02}", strip(mant), exp.unsigned_abs())
    } else {
        let decimals = (16 - exp).max(0) as usize;
        strip(&format!("{v:.decimals$}"))
    }
}

fn state(fc: &mut FitsChan, out: &mut String) {
    let icard = fc.card().unwrap();
    out.push_str(&format!(
        "card={icard} ncard={} nkey={} type={} name={} comm=[{}]\n",
        fc.ncard().unwrap(),
        fc.nkey().unwrap(),
        fc.card_type().unwrap().code(),
        fc.card_name().unwrap().unwrap_or_default(),
        fc.card_comm().unwrap()
    ));
    fc.clear_card().unwrap();
    loop {
        match fc.find_fits("%f", true) {
            Ok(Some(card)) => {
                out.push('|');
                out.push_str(&card);
                out.push('\n');
            }
            Ok(None) => break,
            Err(e) => {
                out.push_str(&format!("|err {}\n", code_name(&e)));
                break;
            }
        }
    }
    fc.set_card(icard as i64).unwrap();
}

fn result<T>(r: Result<T, AstError>, show: impl FnOnce(T) -> String) -> String {
    match r {
        Ok(v) => show(v),
        Err(e) => format!("= err {}\n", code_name(&e)),
    }
}

fn found<T>(v: Option<T>, show: impl FnOnce(T) -> String) -> String {
    match v {
        Some(v) => format!("= {}\n", show(v)),
        None => "= nf\n".to_string(),
    }
}

/// A getter's keyword name; "-" is C's NULL, the current card.
fn name(arg: &str) -> Option<&str> {
    (arg != "-").then_some(arg)
}

fn run(fc: &mut FitsChan, op: &str, out: &mut String, h: &mut Harness) {
    let f: Vec<&str> = op.split('|').collect();
    let arg = |i: usize| f.get(i).copied().unwrap_or("");
    let ow = || arg(1) == "1";
    out.push_str(&format!("> {op}\n"));
    let res = match f[0] {
        "P" => result(fc.put_fits(arg(2), ow()), |_| String::new()),
        "PC" => {
            let parts: Vec<&str> = op[3..].split('~').collect();
            let mut cards = String::new();
            for (i, p) in parts.iter().enumerate() {
                cards.push_str(p);
                if i + 1 < parts.len() {
                    while cards.len() % 80 != 0 {
                        cards.push(' ');
                    }
                }
            }
            result(fc.put_cards(&cards), |_| String::new())
        }
        "SC" => {
            fc.set_card(arg(1).parse().unwrap()).unwrap();
            String::new()
        }
        "CC" => {
            fc.clear_card().unwrap();
            String::new()
        }
        "TC" => format!("= {}\n", i32::from(fc.test_card().unwrap())),
        "F" => result(fc.find_fits(arg(2), arg(1) == "1"), |v| match v {
            Some(card) => format!("= found [{card}]\n"),
            None => "= nf\n".to_string(),
        }),
        "D" => {
            fc.del_fits().unwrap();
            String::new()
        }
        "E" => {
            fc.empty_fits().unwrap();
            String::new()
        }
        "GI" => result(fc.get_fits_i(name(arg(1))), |v| found(v, |v| v.to_string())),
        "GK" => result(fc.get_fits_k(name(arg(1))), |v| found(v, |v| v.to_string())),
        "GF" => result(fc.get_fits_f(name(arg(1))), |v| found(v, c_g17)),
        "GS" => result(fc.get_fits_s(name(arg(1))), |v| {
            found(v, |v| format!("[{v}]"))
        }),
        "GCN" => result(fc.get_fits_cn(name(arg(1))), |v| {
            found(v, |v| format!("[{v}]"))
        }),
        "GL" => result(fc.get_fits_l(name(arg(1))), |v| {
            found(v, |v| i32::from(v).to_string())
        }),
        "GCF" => result(fc.get_fits_cf(name(arg(1))), |v| {
            found(v, |(re, im)| format!("{} {}", c_g17(re), c_g17(im)))
        }),
        "GCI" => result(fc.get_fits_ci(name(arg(1))), |v| {
            found(v, |(re, im)| format!("{re} {im}"))
        }),
        "SK" => result(
            fc.set_fits_k(arg(2), arg(3).parse().unwrap(), arg(4), ow()),
            |_| String::new(),
        ),
        "SCN" => result(fc.set_fits_cn(arg(2), arg(3), arg(4), ow()), |_| {
            String::new()
        }),
        "SCF" => result(
            fc.set_fits_cf(
                arg(2),
                (arg(3).parse().unwrap(), arg(4).parse().unwrap()),
                arg(5),
                ow(),
            ),
            |_| String::new(),
        ),
        "SCI" => result(
            fc.set_fits_ci(
                arg(2),
                (arg(3).parse().unwrap(), arg(4).parse().unwrap()),
                arg(5),
                ow(),
            ),
            |_| String::new(),
        ),
        "SI" => result(
            fc.set_fits_i(arg(2), arg(3).parse().unwrap(), arg(4), ow()),
            |_| String::new(),
        ),
        "SF" => result(
            fc.set_fits_f(arg(2), arg(3).parse().unwrap(), arg(4), ow()),
            |_| String::new(),
        ),
        "SS" => result(fc.set_fits_s(arg(2), arg(3), arg(4), ow()), |_| {
            String::new()
        }),
        "SL" => result(fc.set_fits_l(arg(2), arg(3) != "0", arg(4), ow()), |_| {
            String::new()
        }),
        "SU" => result(fc.set_fits_u(arg(2), arg(3), ow()), |_| String::new()),
        "CM" => result(fc.set_fits_cm(arg(2), ow()), |_| String::new()),
        "ENC" => {
            fc.set_encoding(match arg(1) {
                "FITS-WCS" => Encoding::FitsWcs,
                "FITS-PC" => Encoding::FitsPc,
                "FITS-CLASS" => Encoding::FitsClass,
                "NATIVE" => Encoding::Native,
                other => panic!("no encoding {other}"),
            });
            String::new()
        }
        "CL" => {
            fc.set_clean(arg(1) == "1");
            String::new()
        }
        "PW" => {
            fc.purge_wcs().unwrap();
            String::new()
        }
        "RF" => {
            fc.retain_fits().unwrap();
            String::new()
        }
        "TF" => result(fc.test_fits(name(arg(1))), |v| match v {
            None => "= nf\n".to_string(),
            Some(has) => format!("= {}\n", i32::from(has)),
        }),
        "AI" => {
            let (v, set) = match arg(1) {
                "SipReplace" => (i32::from(fc.sip_replace()), fc.test_sip_replace()),
                "CarLin" => (i32::from(fc.car_lin()), fc.test_car_lin()),
                "FitsRounding" => (fc.fits_rounding(), fc.test_fits_rounding()),
                "FitsDigits" => (fc.fits_digits(), fc.test_fits_digits()),
                "DefB1950" => (i32::from(fc.def_b1950().unwrap()), fc.test_def_b1950()),
                "PolyTan" => (fc.poly_tan(), fc.test_poly_tan()),
                "IgnoreBadAlt" => (i32::from(fc.ignore_bad_alt()), fc.test_ignore_bad_alt()),
                "Iwc" => (i32::from(fc.iwc()), fc.test_iwc()),
                "Skip" => (i32::from(fc.config().skip()), fc.config().test_skip()),
                "Full" => (fc.config().full(), fc.config().test_full()),
                other => panic!("no int attribute {other}"),
            };
            format!("= {v} {}\n", i32::from(set))
        }
        "AD" => {
            let (v, set) = match arg(1) {
                "FitsTol" => (fc.fits_tol(), fc.test_fits_tol()),
                other => panic!("no double attribute {other}"),
            };
            format!("= {} {}\n", c_g17(v), i32::from(set))
        }
        "ASI" => {
            let v: i32 = arg(2).parse().unwrap();
            match arg(1) {
                "SipReplace" => fc.set_sip_replace(v != 0),
                "CarLin" => fc.set_car_lin(v != 0),
                "FitsRounding" => fc.set_fits_rounding(v),
                "FitsDigits" => fc.set_fits_digits(v),
                "DefB1950" => fc.set_def_b1950(v != 0),
                "PolyTan" => fc.set_poly_tan(v),
                "IgnoreBadAlt" => fc.set_ignore_bad_alt(v != 0),
                "Iwc" => fc.set_iwc(v != 0),
                "Skip" => fc.config_mut().set_skip(v != 0),
                "Full" => fc.config_mut().set_full(v),
                "Comment" => fc.config_mut().set_comment(v != 0),
                "Strict" => fc.config_mut().set_strict(v != 0),
                other => panic!("no int attribute {other}"),
            }
            String::new()
        }
        "AC" => {
            let (v, set) = match arg(1) {
                "Warnings" => (fc.warning_conditions(), fc.test_warning_conditions()),
                "AllWarnings" => (fc.all_warning_conditions(), false),
                "FitsAxisOrder" => (fc.fits_axis_order(), fc.test_fits_axis_order()),
                "AltAxes" => (
                    match fc.alt_axes() {
                        AltAxes::All => "ALL",
                        AltAxes::Ident => "IDENT",
                        AltAxes::None => "NONE",
                    },
                    fc.test_alt_axes(),
                ),
                other => panic!("no string attribute {other}"),
            };
            format!("= [{v}] {}\n", i32::from(set))
        }
        "ASC" => match arg(1) {
            "Warnings" => result(fc.set_warning_conditions(arg(2)), |()| String::new()),
            "FitsAxisOrder" => {
                fc.set_fits_axis_order(arg(2));
                String::new()
            }
            "AltAxes" => {
                fc.set_alt_axes(match arg(2).to_ascii_uppercase().as_str() {
                    "ALL" => AltAxes::All,
                    "IDENT" => AltAxes::Ident,
                    "NONE" => AltAxes::None,
                    other => panic!("no AltAxes {other}"),
                });
                String::new()
            }
            other => panic!("no string attribute {other}"),
        },
        "WN" => {
            let texts = fc.warning_texts();
            let mut out = format!("= {}\n", texts.len());
            for text in texts {
                out.push_str(&format!("|{text}\n"));
            }
            out
        }
        "ASD" => {
            match arg(1) {
                "FitsTol" => fc.set_fits_tol(arg(2).parse().unwrap()),
                other => panic!("no double attribute {other}"),
            }
            String::new()
        }
        "AX" => {
            match arg(1) {
                "SipReplace" => fc.clear_sip_replace(),
                "CarLin" => fc.clear_car_lin(),
                "FitsRounding" => fc.clear_fits_rounding(),
                "FitsTol" => fc.clear_fits_tol(),
                "FitsDigits" => fc.clear_fits_digits(),
                "DefB1950" => fc.clear_def_b1950(),
                "PolyTan" => fc.clear_poly_tan(),
                "IgnoreBadAlt" => fc.clear_ignore_bad_alt(),
                "Iwc" => fc.clear_iwc(),
                "Warnings" => fc.clear_warning_conditions(),
                "AltAxes" => fc.clear_alt_axes(),
                "FitsAxisOrder" => fc.clear_fits_axis_order(),
                "SinkFile" => fc.clear_sink_file(),
                other => panic!("no attribute {other}"),
            }
            String::new()
        }
        "RN" => result(fc.read(), |obj| match obj {
            Some(obj) => {
                let class = match obj {
                    AnyObject::FrameSet(_) => "FrameSet",
                    _ => "another class",
                };
                let mut out = format!("= {class}\n");
                for line in native_dump(&obj).unwrap().lines() {
                    out.push_str(&format!("|{line}\n"));
                }
                out
            }
            None => "= null\n".to_string(),
        }),
        "RCIW" => result(fc.read(), |obj| match obj {
            Some(AnyObject::FrameSet(mut fs)) => {
                fs.frame_mut(FrameRef::Index(arg(1).parse().unwrap()))
                    .unwrap()
                    .clear_ident()
                    .unwrap();
                let obj = AnyObject::FrameSet(fs);
                result(fc.write(&obj), |n| format!("= FrameSet {n}\n"))
            }
            Some(_) => "= another class 0\n".to_string(),
            None => "= null 0\n".to_string(),
        }),
        "RSW" => result(fc.read(), |obj| {
            fc.set_warning_conditions(arg(1)).unwrap();
            match obj {
                Some(obj) => {
                    let class = match obj {
                        AnyObject::FrameSet(_) => "FrameSet",
                        _ => "another class",
                    };
                    result(fc.write(&obj), |n| format!("= {class} {n}\n"))
                }
                None => "= null 0\n".to_string(),
            }
        }),
        "RW" => result(fc.read(), |obj| match obj {
            Some(obj) => {
                let class = match obj {
                    AnyObject::FrameSet(_) => "FrameSet",
                    _ => "another class",
                };
                result(fc.write(&obj), |n| format!("= {class} {n}\n"))
            }
            None => "= null 0\n".to_string(),
        }),
        "FD" => {
            fc.set_fits_digits(arg(1).parse().unwrap());
            String::new()
        }
        "QCD" => format!(
            "= {} {}\n",
            i32::from(fc.cd_matrix().unwrap()),
            i32::from(fc.test_cd_matrix())
        ),
        "SCD" => {
            fc.set_cd_matrix(arg(1) == "1");
            String::new()
        }
        "XCD" => {
            fc.clear_cd_matrix();
            String::new()
        }
        "QE" => {
            let name = match fc.encoding().unwrap() {
                Encoding::Native => "NATIVE",
                Encoding::FitsWcs => "FITS-WCS",
                Encoding::FitsPc => "FITS-PC",
                Encoding::FitsIraf => "FITS-IRAF",
                Encoding::FitsAips => "FITS-AIPS",
                Encoding::FitsAipsPP => "FITS-AIPS++",
                Encoding::FitsClass => "FITS-CLASS",
                Encoding::Dss => "DSS",
            };
            format!("= {name}\n")
        }
        "R" => result(fc.read(), |obj| {
            let class = match obj {
                Some(AnyObject::FrameSet(_)) => "FrameSet",
                Some(AnyObject::ZoomMap(_)) => "ZoomMap",
                Some(AnyObject::BasicFrame(_)) => "Frame",
                Some(AnyObject::CmpMap(_)) => "CmpMap",
                Some(_) => "another class",
                None => "null",
            };
            format!("= {class}\n")
        }),
        "WF" => {
            let frame =
                AnyObject::BasicFrame(ast_core::frames::BasicFrameBuilder::new(2).build().unwrap());
            result(fc.write(&frame), |n| format!("= {n}\n"))
        }
        "WFT" => {
            let mut frame = ast_core::frames::BasicFrameBuilder::new(2).build().unwrap();
            frame.set_title(arg(1));
            result(fc.write(&AnyObject::BasicFrame(frame)), |n| {
                format!("= {n}\n")
            })
        }
        "WC" => {
            use ast_core::mappings::{CmpMap, Composition};
            let cm = CmpMap::new(
                Box::new(ZoomMap::new(2, 2.0).unwrap()),
                Box::new(ZoomMap::new(2, 3.0).unwrap()),
                Composition::Series,
            )
            .unwrap();
            result(fc.write(&AnyObject::CmpMap(cm)), |n| format!("= {n}\n"))
        }
        "WCF" => {
            let one = || Box::new(ast_core::frames::BasicFrameBuilder::new(1).build().unwrap());
            let cf = ast_core::frames::CmpFrame::new(one(), one());
            result(fc.write(&AnyObject::CmpFrame(cf)), |n| format!("= {n}\n"))
        }
        "W" => {
            let zm = AnyObject::ZoomMap(ZoomMap::new(2, 2.0).unwrap());
            result(fc.write(&zm), |n| format!("= {n}\n"))
        }
        "RDF" => result(fc.read_fits(), |()| String::new()),
        "WRF" => result(fc.write_fits(), |()| String::new()),
        "MKF" => {
            std::fs::write(tmp_path(arg(1)), op[5 + arg(1).len()..].replace('~', "\n")).unwrap();
            String::new()
        }
        "SRF" => result(fc.set_source_file(&tmp_path(arg(1))), |()| String::new()),
        "SKF" => {
            fc.set_sink_file(&tmp_path(arg(1)));
            h.sink_name = Some(arg(1).to_string());
            String::new()
        }
        "CATF" => cat_file(arg(1)),
        "CPN" => result(fc.clone().ncard(), |n| format!("= {n}\n")),
        other => panic!("unknown op {other}"),
    };
    out.push_str(&std::mem::take(&mut *h.sink.lock().unwrap()));
    out.push_str(&res);
    state(fc, out);
}

/// Cases the port does not yet match, with the reason. The companion test
/// fails once an entry matches, so the list cannot go stale.
const EXCLUDED: &[(&str, &str)] = &[
    (
        "native_full_1",
        "with Full positive C also writes the unset RefCnt and Nobj of each \
         Object part as comment cards, counts of pointers to the Object and \
         of Objects of its class, which the port does not have; \
         native_full_1_matches_c_but_for_process_counts compares the rest",
    ),
    ("native_unknown_item", NATIVE_WHOLE),
    ("native_unknown_item_strict", NATIVE_WHOLE),
    ("native_bad_value", NATIVE_WHOLE),
    ("native_bad_value_clean", NATIVE_WHOLE),
];

/// The FitsChan's NATIVE read parses the whole description before loading
/// it, rather than reading it as C's Channel does, class part by class part:
/// it reports no item that no loader reads, ignores Strict, takes a string
/// where a number is expected without error, and so cannot know how far C's
/// reader got when Clean removes the cards read before a failure (FA-6).
const NATIVE_WHOLE: &str = "FitsChan NATIVE read parses the whole description at once";

/// The transcript the port gives for a case's operations.
fn transcript(ops: &[&str]) -> String {
    let mut h = Harness::default();
    let mut fc = FitsChan::new();
    let mut ops = ops;
    if let Some(src) = ops[0].strip_prefix("NEW|") {
        fc.set_source(Box::new(ScriptSource(src.to_string())));
        fc.set_sink(Box::new(LogSink(h.sink.clone())));
        ops = &ops[1..];
    }
    let mut got = String::new();
    for op in ops {
        run(&mut fc, op, &mut got, &mut h);
    }
    got.push_str("> annul\n");
    drop(fc);
    got.push_str(&std::mem::take(&mut *h.sink.lock().unwrap()));
    if let Some(name) = h.sink_name {
        got.push_str(&cat_file(&name));
        let _ = std::fs::remove_file(tmp_path(&name));
    }
    got
}

#[test]
fn excluded_cases_still_differ() {
    for &(name, reason) in EXCLUDED {
        let &(_, ops, want) = cases::CASES
            .iter()
            .find(|(n, _, _)| *n == name)
            .unwrap_or_else(|| panic!("no case {name}"));
        assert_ne!(
            transcript(ops),
            want,
            "{name} now matches C; remove it from EXCLUDED ({reason})"
        );
    }
}

/// `native_full_1` with C's `REFCNT` and `NOBJ` comment cards left out, and
/// the card counts that include them: everything else C writes with Full
/// positive, every unset value and every `IsA` marker, matches.
#[test]
fn native_full_1_matches_c_but_for_process_counts() {
    let &(_, ops, want) = cases::CASES
        .iter()
        .find(|(n, _, _)| *n == "native_full_1")
        .unwrap();
    let keep = |line: &&str| {
        !line.starts_with("card=")
            && !line.starts_with("|COMMENT REFCNT_")
            && !line.starts_with("|COMMENT NOBJ_")
    };
    // The Frame and its two Axes each have an Object part.
    let refcnt = want.lines().filter(|l| l.starts_with("|COMMENT REFCNT_"));
    assert_eq!(refcnt.count(), 3);
    let want: Vec<&str> = want.lines().filter(keep).collect();
    let got = transcript(ops);
    let got: Vec<&str> = got.lines().filter(keep).collect();
    assert_eq!(got, want);
}

#[test]
fn card_functions_match_the_c_library() {
    let mut failed = Vec::new();
    for &(name, ops, want) in cases::CASES {
        if EXCLUDED.iter().any(|(e, _)| *e == name) {
            continue;
        }
        let got = transcript(ops);
        if got != want {
            let first = got
                .lines()
                .zip(want.lines())
                .position(|(g, w)| g != w)
                .unwrap_or(got.lines().count().min(want.lines().count()));
            let ctx = |t: &str| {
                t.lines()
                    .skip(first.saturating_sub(3))
                    .take(6)
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            failed.push(format!(
                "{name}: first difference at line {}\n--- rust\n{}\n--- C\n{}",
                first + 1,
                ctx(&got),
                ctx(want)
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n\n"));
}
