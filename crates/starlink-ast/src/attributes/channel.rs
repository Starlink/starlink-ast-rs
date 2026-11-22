// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The attributes of a FitsChan (Encoding, Card, Warnings and the rest, and
//! the read-only NCard, NKey, CardName, CardType, CardComm and
//! AllWarnings) and those it inherits from the Channel class (Comment,
//! Full, Indent, ReportLevel, Skip, Strict, SinkFile and SourceFile).
//!
//! Several of them work on the cards: setting Card moves the current card,
//! the read-only ones describe the cards, Encoding when not set is guessed
//! from them, and a FitsChan with a SourceFile reads its cards before any of
//! them.
// C: fitschan.c and channel.c SetAttrib, GetAttrib, TestAttrib and
// ClearAttrib; FitsChan's parent is Channel and Channel's is Object. Only a
// FitsChan reaches Channel's functions here: the other Channel classes have
// no attribute strings yet.

use super::object::{ObjectAttrs, ObjectRead, object_clear, object_get, object_set, object_test};
use super::{
    AstError, abbreviates, double_value, fmt_double, fmt_int, int_value, is_read_only,
    read_only_clear, read_only_set, string_value,
};
use crate::channel::Channel;
use crate::error::{AstResult, ErrorCode};
use crate::fitschan::{AltAxes, Encoding, FitsChan};

impl ObjectRead for FitsChan {
    fn obj_class(&self) -> &'static str {
        "FitsChan"
    }
    fn obj_id(&self) -> String {
        self.id().to_string()
    }
    fn obj_test_id(&self) -> bool {
        self.test_id()
    }
    fn obj_ident(&self) -> String {
        self.ident().to_string()
    }
    fn obj_test_ident(&self) -> bool {
        self.test_ident()
    }
    fn obj_use_defs(&self) -> bool {
        self.stored_use_defs().unwrap_or(true)
    }
    fn obj_test_use_defs(&self) -> bool {
        self.stored_use_defs().is_some()
    }
}

impl ObjectAttrs for FitsChan {
    fn obj_set_id(&mut self, id: &str) -> AstResult<()> {
        self.set_id(id);
        Ok(())
    }
    fn obj_clear_id(&mut self) -> AstResult<()> {
        self.clear_id();
        Ok(())
    }
    fn obj_set_ident(&mut self, ident: &str) -> AstResult<()> {
        self.set_ident(ident);
        Ok(())
    }
    fn obj_clear_ident(&mut self) -> AstResult<()> {
        self.clear_ident();
        Ok(())
    }
    fn obj_set_use_defs(&mut self, value: bool) -> AstResult<()> {
        // Nothing a FitsChan does reads UseDefs, but its dump holds it.
        self.store_use_defs(Some(value));
        Ok(())
    }
    fn obj_clear_use_defs(&mut self) -> AstResult<()> {
        self.store_use_defs(None);
        Ok(())
    }
}

/// The encodings by the names C reads and writes, in the order C tries
/// them.
const ENCODINGS: [(&str, Encoding); 8] = [
    ("NATIVE", Encoding::Native),
    ("FITS-PC", Encoding::FitsPc),
    ("FITS-WCS", Encoding::FitsWcs),
    ("FITS-IRAF", Encoding::FitsIraf),
    ("FITS-AIPS", Encoding::FitsAips),
    ("FITS-AIPS++", Encoding::FitsAipsPP),
    ("FITS-CLASS", Encoding::FitsClass),
    ("DSS", Encoding::Dss),
];

/// The read-only attributes of a FitsChan.
const FITS_READ_ONLY: [&str; 6] = [
    "ncard",
    "cardtype",
    "cardcomm",
    "cardname",
    "nkey",
    "allwarnings",
];

/// A setter of a FitsChan flag.
type FlagSetter = fn(&mut FitsChan, bool);
/// A setter of a FitsChan integer.
type IntSetter = fn(&mut FitsChan, i32);

/// FitsChan's `SetAttrib`.
pub(crate) fn fitschan_set(fc: &mut FitsChan, setting: &str) -> AstResult<()> {
    let flags: [(&str, FlagSetter); 7] = [
        ("cdmatrix", FitsChan::set_cd_matrix),
        ("defb1950", FitsChan::set_def_b1950),
        ("ignorebadalt", FitsChan::set_ignore_bad_alt),
        ("carlin", FitsChan::set_car_lin),
        ("sipreplace", FitsChan::set_sip_replace),
        ("sipok", FitsChan::set_sip_ok),
        ("iwc", FitsChan::set_iwc),
    ];
    let ints: [(&str, IntSetter); 5] = [
        ("fitsdigits", FitsChan::set_fits_digits),
        ("fitsrounding", FitsChan::set_fits_rounding),
        ("tabok", FitsChan::set_tab_ok),
        ("forcetab", FitsChan::set_force_tab),
        ("polytan", FitsChan::set_poly_tan),
    ];
    if let Some(v) = int_value(setting, "card") {
        return fc.set_card(i64::from(v));
    }
    if let Some(v) = string_value(setting, "encoding") {
        // C tries the names in this order, each with `-` and with `_`.
        let enc = ENCODINGS.iter().find_map(|&(name, enc)| {
            (abbreviates(v, name) || abbreviates(v, &name.replace('-', "_"))).then_some(enc)
        });
        return match enc {
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
        };
    }
    if let Some(v) = string_value(setting, "altaxes") {
        let alt = [AltAxes::None, AltAxes::Ident, AltAxes::All]
            .into_iter()
            .find(|a| abbreviates(v, a.name()));
        return match alt {
            Some(a) => {
                fc.set_alt_axes(a);
                Ok(())
            }
            None => Err(AstError::new(
                ErrorCode::Badat,
                format!("astSet(FitsChan): Illegal value '{v}' supplied for attribute AltAxes."),
            )),
        };
    }
    for (name, set) in ints {
        if let Some(v) = int_value(setting, name) {
            set(fc, v);
            return Ok(());
        }
    }
    if let Some(v) = string_value(setting, "fitsaxisorder") {
        fc.set_fits_axis_order(v);
        return Ok(());
    }
    for (name, set) in flags {
        if let Some(v) = int_value(setting, name) {
            set(fc, v != 0);
            return Ok(());
        }
    }
    if let Some(v) = double_value(setting, "fitstol") {
        fc.set_fits_tol(v);
        return Ok(());
    }
    if let Some(v) = int_value(setting, "clean") {
        fc.set_clean(v != 0);
        return Ok(());
    }
    if let Some(v) = string_value(setting, "warnings") {
        return fc.set_warning_conditions(v);
    }
    if FITS_READ_ONLY
        .iter()
        .any(|name| is_read_only(setting, name))
    {
        return Err(read_only_set(setting, "FitsChan"));
    }
    channel_set(fc, setting)
}

/// Channel's `SetAttrib`.
fn channel_set(fc: &mut FitsChan, setting: &str) -> AstResult<()> {
    let config = fc.config_mut();
    if let Some(v) = int_value(setting, "comment") {
        config.set_comment(v != 0);
    } else if let Some(v) = int_value(setting, "full") {
        config.set_full(v);
    } else if let Some(v) = int_value(setting, "indent") {
        config.set_indent(v);
    } else if let Some(v) = int_value(setting, "reportlevel") {
        config.set_report_level(v);
    } else if let Some(v) = int_value(setting, "skip") {
        config.set_skip(v != 0);
    } else if let Some(v) = string_value(setting, "sinkfile") {
        fc.set_sink_file(v);
    } else if let Some(v) = string_value(setting, "sourcefile") {
        return fc.set_source_file(v);
    } else if let Some(v) = int_value(setting, "strict") {
        config.set_strict(v != 0);
    } else {
        return object_set(fc, setting);
    }
    Ok(())
}

/// FitsChan's `GetAttrib`.
#[cfg_attr(not(feature = "attribute-strings"), allow(dead_code))]
pub(crate) fn fitschan_get(fc: &mut FitsChan, name: &str) -> AstResult<String> {
    let flag = |v: bool| fmt_int(i32::from(v));
    let count = |v: usize| fmt_int(i32::try_from(v).unwrap_or(i32::MAX));
    Ok(match name {
        "card" => count(fc.card()?),
        "cardcomm" => fc.card_comm()?,
        "cardname" => fc.card_name()?.unwrap_or_default(),
        "cardtype" => fmt_int(fc.card_type()?.code()),
        "encoding" => {
            let enc = fc.encoding()?;
            ENCODINGS
                .iter()
                .find(|(_, e)| *e == enc)
                .map_or("UNKNOWN", |(n, _)| n)
                .to_string()
        }
        "cdmatrix" => flag(fc.cd_matrix()?),
        "defb1950" => flag(fc.def_b1950()?),
        "tabok" => fmt_int(fc.tab_ok()),
        "forcetab" => fmt_int(fc.force_tab()),
        "ignorebadalt" => flag(fc.ignore_bad_alt()),
        "carlin" => flag(fc.car_lin()),
        "sipreplace" => flag(fc.sip_replace()),
        "fitstol" => fmt_double(fc.fits_tol()),
        "polytan" => fmt_int(fc.poly_tan()),
        "sipok" => flag(fc.sip_ok()),
        "iwc" => flag(fc.iwc()),
        "clean" => flag(fc.clean()),
        "fitsaxisorder" => fc.fits_axis_order().to_string(),
        "fitsdigits" => fmt_int(fc.fits_digits()),
        "fitsrounding" => fmt_int(fc.fits_rounding()),
        "altaxes" => fc.alt_axes().name().to_string(),
        "ncard" => count(fc.ncard()?),
        "nkey" => count(fc.nkey()?),
        "allwarnings" => fc.all_warning_conditions().to_string(),
        "warnings" => fc.warning_conditions().to_string(),
        _ => return channel_get(fc, name),
    })
}

/// Channel's `GetAttrib`.
#[cfg_attr(not(feature = "attribute-strings"), allow(dead_code))]
fn channel_get(fc: &FitsChan, name: &str) -> AstResult<String> {
    let config = fc.config();
    let flag = |v: bool| fmt_int(i32::from(v));
    Ok(match name {
        "comment" => flag(config.comment()),
        "full" => fmt_int(config.full()),
        "indent" => fmt_int(config.indent()),
        "reportlevel" => fmt_int(config.report_level()),
        "skip" => flag(config.skip()),
        "sourcefile" => fc.source_file().unwrap_or("").to_string(),
        "sinkfile" => fc.sink_file().unwrap_or("").to_string(),
        "strict" => flag(config.strict()),
        _ => return object_get(fc, name),
    })
}

/// FitsChan's `TestAttrib`.
#[cfg_attr(not(feature = "attribute-strings"), allow(dead_code))]
pub(crate) fn fitschan_test(fc: &FitsChan, name: &str) -> AstResult<bool> {
    Ok(match name {
        // Reading the Card reads any source first, which a copy does.
        "card" => fc.clone().test_card()?,
        "encoding" => fc.test_encoding(),
        "fitsaxisorder" => fc.test_fits_axis_order(),
        "altaxes" => fc.test_alt_axes(),
        "fitsdigits" => fc.test_fits_digits(),
        "fitsrounding" => fc.test_fits_rounding(),
        "defb1950" => fc.test_def_b1950(),
        "tabok" => fc.test_tab_ok(),
        "forcetab" => fc.test_force_tab(),
        "ignorebadalt" => fc.test_ignore_bad_alt(),
        "cdmatrix" => fc.test_cd_matrix(),
        "carlin" => fc.test_car_lin(),
        "sipreplace" => fc.test_sip_replace(),
        "fitstol" => fc.test_fits_tol(),
        "polytan" => fc.test_poly_tan(),
        "sipok" => fc.test_sip_ok(),
        "iwc" => fc.test_iwc(),
        "clean" => fc.test_clean(),
        "warnings" => fc.test_warning_conditions(),
        _ if FITS_READ_ONLY.contains(&name) => false,
        _ => return channel_test(fc, name),
    })
}

/// Channel's `TestAttrib`.
#[cfg_attr(not(feature = "attribute-strings"), allow(dead_code))]
fn channel_test(fc: &FitsChan, name: &str) -> AstResult<bool> {
    let config = fc.config();
    Ok(match name {
        "comment" => config.test_comment(),
        "full" => config.test_full(),
        "indent" => config.test_indent(),
        "reportlevel" => config.test_report_level(),
        "skip" => config.test_skip(),
        "sourcefile" => fc.test_source_file(),
        "sinkfile" => fc.test_sink_file(),
        "strict" => config.test_strict(),
        _ => return object_test(fc, name),
    })
}

/// FitsChan's `ClearAttrib`.
// C treats only NCard and AllWarnings as read-only here; a clear of the
// other read-only names goes on to the Object class's error.
#[cfg_attr(not(feature = "attribute-strings"), allow(dead_code))]
pub(crate) fn fitschan_clear(fc: &mut FitsChan, name: &str) -> AstResult<()> {
    match name {
        "card" => return fc.clear_card(),
        "altaxes" => fc.clear_alt_axes(),
        "encoding" => fc.clear_encoding(),
        "cdmatrix" => fc.clear_cd_matrix(),
        "fitsaxisorder" => fc.clear_fits_axis_order(),
        "fitsdigits" => fc.clear_fits_digits(),
        "fitsrounding" => fc.clear_fits_rounding(),
        "defb1950" => fc.clear_def_b1950(),
        "tabok" => fc.clear_tab_ok(),
        "forcetab" => fc.clear_force_tab(),
        "ignorebadalt" => fc.clear_ignore_bad_alt(),
        "carlin" => fc.clear_car_lin(),
        "sipreplace" => fc.clear_sip_replace(),
        "fitstol" => fc.clear_fits_tol(),
        "polytan" => fc.clear_poly_tan(),
        "sipok" => fc.clear_sip_ok(),
        "iwc" => fc.clear_iwc(),
        "clean" => fc.clear_clean(),
        "warnings" => fc.clear_warning_conditions(),
        "ncard" | "allwarnings" => return Err(read_only_clear(name, "FitsChan")),
        _ => return channel_clear(fc, name),
    }
    Ok(())
}

/// Channel's `ClearAttrib`.
#[cfg_attr(not(feature = "attribute-strings"), allow(dead_code))]
fn channel_clear(fc: &mut FitsChan, name: &str) -> AstResult<()> {
    let config = fc.config_mut();
    match name {
        "comment" => config.clear_comment(),
        "full" => config.clear_full(),
        "indent" => config.clear_indent(),
        "reportlevel" => config.clear_report_level(),
        "skip" => config.clear_skip(),
        "sourcefile" => fc.clear_source_file(),
        "sinkfile" => fc.clear_sink_file(),
        "strict" => config.clear_strict(),
        _ => return object_clear(fc, name),
    }
    Ok(())
}
