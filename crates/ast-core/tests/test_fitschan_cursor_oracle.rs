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

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "fitschan_cursor_oracle/cases.rs"]
mod cases;

use ast_core::error::AstError;
use ast_core::frames::FrameRef;
use ast_core::mappings::ZoomMap;
use ast_core::{AltAxes, AnyObject, Channel, Encoding, FitsChan};

/// C's name for an error code, as the probe prints it.
fn code_name(e: &AstError) -> &'static str {
    match format!("{:?}", e.code()).as_str() {
        "Bdfts" => "Bdfts",
        "Ftcnv" => "Ftcnv",
        "Fundef" => "Fundef",
        "Bdfmt" => "Bdfmt",
        "Attin" => "Attin",
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
    let icard = fc.card();
    out.push_str(&format!(
        "card={icard} ncard={} nkey={} type={} name={} comm=[{}]\n",
        fc.ncard(),
        fc.nkey(),
        fc.card_type().code(),
        fc.card_name().unwrap_or_default(),
        fc.card_comm()
    ));
    fc.clear_card();
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
    fc.set_card(icard as i64);
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

fn run(fc: &mut FitsChan, op: &str, out: &mut String) {
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
            fc.set_card(arg(1).parse().unwrap());
            String::new()
        }
        "CC" => {
            fc.clear_card();
            String::new()
        }
        "TC" => format!("= {}\n", i32::from(fc.test_card())),
        "F" => result(fc.find_fits(arg(2), arg(1) == "1"), |v| match v {
            Some(card) => format!("= found [{card}]\n"),
            None => "= nf\n".to_string(),
        }),
        "D" => {
            fc.del_fits();
            String::new()
        }
        "E" => {
            fc.empty_fits();
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
            fc.purge_wcs();
            String::new()
        }
        "RF" => {
            fc.retain_fits();
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
                "DefB1950" => (i32::from(fc.def_b1950()), fc.test_def_b1950()),
                "PolyTan" => (fc.poly_tan(), fc.test_poly_tan()),
                "IgnoreBadAlt" => (i32::from(fc.ignore_bad_alt()), fc.test_ignore_bad_alt()),
                "Iwc" => (i32::from(fc.iwc()), fc.test_iwc()),
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
                other => panic!("no int attribute {other}"),
            }
            String::new()
        }
        "AC" => {
            let (v, set) = match arg(1) {
                "Warnings" => (fc.warning_conditions(), fc.test_warning_conditions()),
                "AllWarnings" => (fc.all_warning_conditions(), false),
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
                for line in ast_core::to_native_string(&obj).unwrap().lines() {
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
            i32::from(fc.cd_matrix()),
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
            let name = match fc.encoding() {
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
                Some(_) => "another class",
                None => "null",
            };
            format!("= {class}\n")
        }),
        "W" => {
            let zm = AnyObject::ZoomMap(ZoomMap::new(2, 2.0).unwrap());
            result(fc.write(&zm), |n| format!("= {n}\n"))
        }
        other => panic!("unknown op {other}"),
    };
    out.push_str(&res);
    state(fc, out);
}

/// Cases the port does not yet match, with the reason. The companion test
/// fails once an entry matches, so the list cannot go stale.
const EXCLUDED: &[(&str, &str)] = &[
    (
        "celestial_bare_names",
        "CTYPE values RA and DEC are read, as C reads them, as linear celestial \
         axes; C writes them back as RA---CAR and DEC--CAR, the port's FITS-WCS \
         writer writes nothing",
    ),
    (
        "dist_zpx_three",
        "C cannot implement the spectral algorithm code -ZPN left on CTYPE3 and \
         the read fails with AST__BDFTS; the port's spectral reader reads the \
         axis as linear",
    ),
    (
        "sao_three_ctypes_rw",
        "with NAXIS 2 and CTYPE1 to CTYPE3, C reads three WCS axes, the highest \
         CRPIX index, where the port reads the two NAXIS gives",
    ),
    (
        "alt_axes_alt_offset",
        "the writer passes no input CRVALs to an alternate description, where \
         C's ReadCrval takes them from the header (CRVAL1B 0 at CRPIX1B 1), and \
         it adds an absolute description A for the offset alternate B, which C \
         does not write",
    ),
    (
        "iwc_on_spectral",
        "simplifying the inverse of the pixel to WCS Mapping followed by the \
         pixel to IWC Mapping of a celestial and spectral header gives a \
         Mapping equal to C's but structured differently: the port inverts \
         the outer CmpMap where C inverts its components, and flags a ShiftMap \
         as simplified",
    ),
    (
        "iwc_on_spectral_cunit",
        "as iwc_on_spectral, with CUNIT3 GHz",
    ),
    (
        "sao_not_sao_rw",
        "the writer puts a new card after the last card it overwrote, where \
         C's SetValue restores the cursor after an overwrite and puts a new \
         card in front of the card current when the write began",
    ),
];

/// The transcript the port gives for a case's operations.
fn transcript(ops: &[&str]) -> String {
    let mut fc = FitsChan::new();
    let mut got = String::new();
    for op in ops {
        run(&mut fc, op, &mut got);
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
