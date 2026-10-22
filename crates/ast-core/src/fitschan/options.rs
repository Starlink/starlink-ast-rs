//! The attribute settings a new FitsChan takes from the `FITSCHAN_OPTIONS`
//! environment variable (object.c `astEnvSet`, `VSet` and the `SetAttrib`
//! of the FitsChan, Channel and Object classes).
//!
//! C reads the variable when the first FitsChan of a thread is created and
//! keeps the value for every later one in that thread (it is held in the
//! thread's FitsChan class table), then applies it with `astSet` before the
//! constructor's own options.
//! Only the settings of a FitsChan's attributes are recognised; the parser
//! is private to the crate, since the Rust API sets attributes through
//! their typed setters.

use std::cell::OnceCell;

use super::card_value::{scan_double, scan_int};
use super::{AltAxes, Encoding, FitsChan};
use crate::error::{AstError, AstResult, ErrorCode};

thread_local! {
    /// The value of `FITSCHAN_OPTIONS` when this thread made its first
    /// FitsChan, `None` when it was not set.
    static ENV_OPTIONS: OnceCell<Option<String>> = const { OnceCell::new() };
}

/// The `FITSCHAN_OPTIONS` settings for a new FitsChan in this thread.
// C: `astEnvSet` stores "None" for an unset variable and skips it.
pub(super) fn env_options() -> Option<String> {
    ENV_OPTIONS.with(|cell| {
        cell.get_or_init(|| std::env::var("FITSCHAN_OPTIONS").ok())
            .clone()
    })
}

/// C's `isspace` in the "C" locale.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// C's `VSet` without the `printf` expansion, which a setting from the
/// environment does not have: split `settings` into assignments and apply
/// each to `fc` in turn, stopping at the first that fails.
pub(super) fn apply(fc: &mut FitsChan, settings: &str) -> AstResult<()> {
    if settings.is_empty() {
        return Ok(());
    }
    let mut buf: Vec<u8> = settings.as_bytes().to_vec();
    // Commas separate assignments only when there is more than one `=`;
    // a comma inside double quotes never does.
    if let Some(eq1) = buf.iter().position(|&b| b == b'=')
        && buf[eq1 + 1..].contains(&b'=')
    {
        let mut quoted = false;
        for b in &mut buf {
            if !quoted {
                if *b == b',' {
                    *b = b'\n';
                } else if *b == b'"' {
                    quoted = true;
                }
            } else if *b == b'"' {
                quoted = false;
            }
        }
    }
    for assign in buf.split(|&b| b == b'\n') {
        let mut out = assignment(assign);
        if out.is_empty() {
            continue;
        }
        // "Title=" would not match an attribute name without a value.
        if out.last() == Some(&b'=') {
            out.push(b' ');
        }
        set_attrib(fc, &String::from_utf8_lossy(&out))?;
    }
    Ok(())
}

/// One assignment as C's `VSet` passes it to `astSetAttrib`: the name in
/// lower case without white space, the value unchanged but for delimiting
/// double quotes, and any `\r` restored to the comma it stands for.
fn assignment(assign: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(assign.len());
    let mut lo = true;
    let mut tq: i32 = -1;
    let mut quoted = false;
    for &c in assign {
        if c == b'=' {
            lo = false;
        }
        if !lo || !is_space(c) {
            if c == b'\r' {
                out.push(b',');
            } else if lo {
                out.push(c.to_ascii_lowercase());
            } else {
                out.push(c);
                if tq > 0 && !is_space(c) {
                    if c == b'"' {
                        quoted = true;
                        out.pop();
                    }
                    tq = 0;
                }
            }
        }
        if c == b'=' && tq == -1 {
            tq = 1;
        }
    }
    if quoted {
        // C steps back over trailing white space, then over one closing
        // quote.
        while out.last().is_some_and(|&b| is_space(b)) {
            out.pop();
        }
        if out.last() == Some(&b'"') {
            out.pop();
        }
    }
    out
}

/// C's `astSscanf( setting, "<name>= %d %n" )` with `nc >= len`.
fn int_value(setting: &str, name: &str) -> Option<i32> {
    let rest = setting.strip_prefix(name)?.strip_prefix('=')?;
    let (v, used) = scan_int(rest)?;
    let tail = &rest[used..];
    tail.bytes().all(is_space).then_some(v as i32)
}

/// C's `astSscanf( setting, "<name>= %lg %n" )` with `nc >= len`.
fn double_value(setting: &str, name: &str) -> Option<f64> {
    let rest = setting.strip_prefix(name)?.strip_prefix('=')?;
    let (v, used) = scan_double(rest)?;
    rest[used..].bytes().all(is_space).then_some(v)
}

/// C's `astSscanf( setting, "<name>=%n%*[^\n]%n" )` with `nc >= len`: the
/// value, which is at least one character.
fn string_value<'a>(setting: &'a str, name: &str) -> Option<&'a str> {
    let rest = setting.strip_prefix(name)?.strip_prefix('=')?;
    (!rest.is_empty() && !rest.contains('\n')).then_some(rest)
}

/// C's `Ustrncmp( value, name, ChrLen( value ) ) == 0`: `value`, without
/// trailing white space, is `name` or a leading part of it, ignoring case.
fn abbreviates(value: &str, name: &str) -> bool {
    let v = value.trim_end_matches(|c: char| c.is_ascii() && is_space(c as u8));
    v.len() <= name.len() && name.as_bytes()[..v.len()].eq_ignore_ascii_case(v.as_bytes())
}

/// The error C's `SetAttrib` gives for a setting of a read-only attribute.
fn read_only(setting: &str) -> AstError {
    AstError::new(
        ErrorCode::Nowrt,
        format!("astSet: The setting \"{setting}\" is invalid for a FitsChan."),
    )
    .with_context("This is a read-only attribute.")
}

/// The FitsChan, Channel and Object `SetAttrib` methods in turn: apply one
/// `name=value` setting, the name in lower case.
fn set_attrib(fc: &mut FitsChan, setting: &str) -> AstResult<()> {
    let is = |name: &str| string_value(setting, name).is_some();
    // FitsChan.
    if let Some(v) = int_value(setting, "card") {
        fc.set_card(i64::from(v))
    } else if let Some(v) = string_value(setting, "encoding") {
        // C tries the names in this order, each with `-` and with `_`.
        const NAMES: [(&str, Encoding); 8] = [
            ("NATIVE", Encoding::Native),
            ("FITS-PC", Encoding::FitsPc),
            ("FITS-WCS", Encoding::FitsWcs),
            ("FITS-IRAF", Encoding::FitsIraf),
            ("FITS-AIPS", Encoding::FitsAips),
            ("FITS-AIPS++", Encoding::FitsAipsPP),
            ("FITS-CLASS", Encoding::FitsClass),
            ("DSS", Encoding::Dss),
        ];
        let enc = NAMES.iter().find_map(|&(name, enc)| {
            (abbreviates(v, name) || abbreviates(v, &name.replace('-', "_"))).then_some(enc)
        });
        match enc {
            Some(enc) => {
                fc.set_encoding(enc);
                Ok(())
            }
            None => Err(AstError::new(
                ErrorCode::Badat,
                format!(
                    "astSet(FitsChan): Unknown encoding system '{v}' requested for a FitsChan."
                ),
            )),
        }
    } else if let Some(v) = string_value(setting, "altaxes") {
        let alt = [AltAxes::None, AltAxes::Ident, AltAxes::All]
            .into_iter()
            .find(|a| abbreviates(v, a.name()));
        match alt {
            Some(a) => {
                fc.set_alt_axes(a);
                Ok(())
            }
            None => Err(AstError::new(
                ErrorCode::Badat,
                format!("astSet(FitsChan): Illegal value '{v}' supplied for attribute AltAxes."),
            )),
        }
    } else if let Some(v) = int_value(setting, "fitsdigits") {
        fc.set_fits_digits(v);
        Ok(())
    } else if let Some(v) = int_value(setting, "fitsrounding") {
        fc.set_fits_rounding(v);
        Ok(())
    } else if let Some(v) = string_value(setting, "fitsaxisorder") {
        fc.set_fits_axis_order(v);
        Ok(())
    } else if let Some(v) = int_value(setting, "cdmatrix") {
        fc.set_cd_matrix(v != 0);
        Ok(())
    } else if let Some(v) = int_value(setting, "defb1950") {
        fc.set_def_b1950(v != 0);
        Ok(())
    } else if let Some(v) = int_value(setting, "tabok") {
        fc.set_tab_ok(v);
        Ok(())
    } else if let Some(v) = int_value(setting, "forcetab") {
        fc.set_force_tab(v);
        Ok(())
    } else if let Some(v) = int_value(setting, "ignorebadalt") {
        fc.set_ignore_bad_alt(v != 0);
        Ok(())
    } else if let Some(v) = int_value(setting, "carlin") {
        fc.set_car_lin(v != 0);
        Ok(())
    } else if let Some(v) = int_value(setting, "sipreplace") {
        fc.set_sip_replace(v != 0);
        Ok(())
    } else if let Some(v) = double_value(setting, "fitstol") {
        fc.set_fits_tol(v);
        Ok(())
    } else if let Some(v) = int_value(setting, "polytan") {
        fc.set_poly_tan(v);
        Ok(())
    } else if let Some(v) = int_value(setting, "sipok") {
        fc.set_sip_ok(v != 0);
        Ok(())
    } else if let Some(v) = int_value(setting, "iwc") {
        fc.set_iwc(v != 0);
        Ok(())
    } else if let Some(v) = int_value(setting, "clean") {
        fc.set_clean(v != 0);
        Ok(())
    } else if let Some(v) = string_value(setting, "warnings") {
        fc.set_warning_conditions(v)
    } else if [
        "ncard",
        "cardtype",
        "cardcomm",
        "cardname",
        "nkey",
        "allwarnings",
    ]
    .into_iter()
    .any(is)
    {
        Err(read_only(setting))
    // Channel.
    } else if let Some(v) = int_value(setting, "comment") {
        fc.config.set_comment(v != 0);
        Ok(())
    } else if let Some(v) = int_value(setting, "full") {
        fc.config.set_full(v);
        Ok(())
    } else if let Some(v) = int_value(setting, "indent") {
        fc.config.set_indent(v);
        Ok(())
    } else if let Some(v) = int_value(setting, "reportlevel") {
        fc.config.set_report_level(v);
        Ok(())
    } else if let Some(v) = int_value(setting, "skip") {
        fc.config.set_skip(v != 0);
        Ok(())
    } else if let Some(v) = string_value(setting, "sinkfile") {
        fc.set_sink_file(v);
        Ok(())
    } else if let Some(v) = string_value(setting, "sourcefile") {
        fc.set_source_file(v)
    } else if let Some(v) = int_value(setting, "strict") {
        fc.config.set_strict(v != 0);
        Ok(())
    // Object.
    } else if let Some(v) = string_value(setting, "id") {
        fc.set_id(v);
        Ok(())
    } else if let Some(v) = string_value(setting, "ident") {
        fc.set_ident(v);
        Ok(())
    } else if int_value(setting, "usedefs").is_some() {
        // UseDefs is held only by Frames, the only classes it affects, so
        // a FitsChan accepts the setting and nothing reads it.
        Ok(())
    } else if ["class", "nobject", "objsize", "refcount"]
        .into_iter()
        .any(is)
    {
        Err(read_only(setting))
    } else {
        Err(AstError::new(
            ErrorCode::Badat,
            format!("astSet: The attribute setting \"{setting}\" is invalid for a FitsChan."),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A FitsChan with `settings` applied, or the error.
    fn with(settings: &str) -> AstResult<FitsChan> {
        let mut fc = FitsChan::bare();
        apply(&mut fc, settings)?;
        Ok(fc)
    }

    // The cases are those of a C probe setting FITSCHAN_OPTIONS before
    // astFitsChan, each in a new process.
    #[test]
    fn values_are_taken_as_written_after_the_equals_sign() {
        let mut fc = with("Encoding=FITS-WCS").unwrap();
        assert_eq!(fc.encoding().unwrap(), Encoding::FitsWcs);
        let e = with(" Encoding = FITS-WCS").unwrap_err();
        assert_eq!(e.code(), ErrorCode::Badat);
        assert_eq!(
            e.message(),
            "astSet(FitsChan): Unknown encoding system ' FITS-WCS' requested for a FitsChan."
        );
        // A single setting keeps its commas.
        let e = with("Encoding=FITS-WCS,").unwrap_err();
        assert!(e.message().contains("'FITS-WCS,'"), "{e}");
    }

    #[test]
    fn encoding_names_may_be_abbreviated_and_use_underscores() {
        let mut fc = with("encoding=fits").unwrap();
        assert_eq!(fc.encoding().unwrap(), Encoding::FitsPc);
        let mut fc = with("Encoding=").unwrap();
        assert!(fc.test_encoding());
        assert_eq!(fc.encoding().unwrap(), Encoding::Native);
        let mut fc = with("Encoding=fits_wcs,FitsDigits=8").unwrap();
        assert_eq!(fc.encoding().unwrap(), Encoding::FitsWcs);
        assert_eq!(fc.fits_digits(), 8);
    }

    #[test]
    fn quotes_protect_commas_and_are_removed() {
        let fc = with("Warnings=\"badval\"").unwrap();
        assert_eq!(fc.warning_conditions(), "badval");
        let e = with("FitsDigits = 8 , Warnings=\"badval, x\"").unwrap_err();
        assert_eq!(e.code(), ErrorCode::Attin);
        assert!(e.message().contains("'badval,'"), "{e}");
        let fc = with("Warnings=badval tnx").unwrap();
        assert_eq!(fc.warning_conditions(), "badval tnx");
    }

    #[test]
    fn channel_attributes_and_numbers() {
        let fc = with("FitsTol=1e-3,strict=1").unwrap();
        assert_eq!(fc.fits_tol(), 1e-3);
        assert!(fc.config.strict());
        // UseDefs is accepted and has no effect.
        assert!(with("UseDefs=0").is_ok());
    }

    #[test]
    fn bad_settings_are_errors() {
        let e = with("FitsDigits=8, Bogus=1").unwrap_err();
        assert_eq!(e.code(), ErrorCode::Badat);
        assert_eq!(
            e.message(),
            "astSet: The attribute setting \"bogus=1\" is invalid for a FitsChan."
        );
        let e = with("Ncard=3").unwrap_err();
        assert_eq!(e.code(), ErrorCode::Nowrt);
        assert_eq!(
            e.message(),
            "astSet: The setting \"ncard=3\" is invalid for a FitsChan."
        );
        let e = with("cdmatrix=1 x").unwrap_err();
        assert_eq!(e.code(), ErrorCode::Badat);
    }
}
