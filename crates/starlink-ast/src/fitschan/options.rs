// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The attribute settings a new FitsChan takes from the `FITSCHAN_OPTIONS`
//! environment variable (object.c `astEnvSet`, `VSet` and the `SetAttrib`
//! of the FitsChan, Channel and Object classes).
//!
//! C reads the variable when the first FitsChan of a thread is created and
//! keeps the value for every later one in that thread (it is held in the
//! thread's FitsChan class table), then applies it with `astSet` before the
//! constructor's own options.
//! The settings are applied as an attribute string is, through
//! `crate::attributes`.

use std::cell::OnceCell;

use super::FitsChan;
use crate::attributes::channel::fitschan_set;
use crate::attributes::split_settings;
use crate::error::AstResult;

thread_local! {
    /// The value of `FITSCHAN_OPTIONS` when this thread made its first
    /// FitsChan, `None` when it was not set.
    static ENV_OPTIONS: OnceCell<Option<String>> = const { OnceCell::new() };
}

/// The `FITSCHAN_OPTIONS` settings for a new FitsChan in this thread.
///
/// A variable set to `None` is ignored, as C's `astEnvSet` stores that
/// string for an unset variable and skips any value equal to it.
pub(super) fn env_options() -> Option<String> {
    ENV_OPTIONS.with(|cell| {
        cell.get_or_init(|| {
            std::env::var("FITSCHAN_OPTIONS")
                .ok()
                .filter(|v| v != "None")
        })
        .clone()
    })
}

/// C's `VSet`: split `settings` into assignments and apply each to `fc` in
/// turn, stopping at the first that fails.
///
/// C passes the variable to `astSet` as its format string, so `VSet`
/// expands any format specifiers in it (`%%` becoming `%`, and any other
/// `%` specifier being undefined behavior); the port takes the value as
/// written, without that expansion.
pub(super) fn apply(fc: &mut FitsChan, settings: &str) -> AstResult<()> {
    for assignment in split_settings(settings) {
        fitschan_set(fc, &assignment)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::fitschan::Encoding;

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
    }

    // C probe: with FITSCHAN_OPTIONS=UseDefs=0, astWrite of the FitsChan to
    // a Channel gives "UseDfs = 0" as the only item of its Object part.
    #[test]
    fn use_defs_is_kept_and_dumped() {
        use crate::{AnyObject, Channel, NativeChannel};
        // The C library's dump of a FitsChan made with FITSCHAN_OPTIONS
        // set to "UseDefs=0", written through a default Channel (C probe).
        const C_DUMP: &str = concat!(
            " Begin FitsChan \t# I/O channels to FITS files\n",
            "    UseDfs = 0 \t# Default values cannot be used\n",
            " IsA Object \t# AST Object\n",
            "    Card = 0 \t# Index of current card\n",
            "#   Encod = \"NATIVE\" \t# Encoding system\n",
            "#   FAxOrd = \"<auto>\" \t# Order of WCS axes in new FITS headers\n",
            "#   FitsDg = 17 \t# No. of digits for floating point values\n",
            "#   FitsRn = 10 \t# No. of digits guarded from rounding\n",
            "#   AltAx = \"ALL\" \t# Creation of alternate axes\n",
            "#   DfB1950 = 1 \t# Default to FK4 B1950\n",
            "#   TabOK = 0 \t# Do not support -TAB CTYPE codes\n",
            "#   FrcTab = 0 \t# Only use -TAB if necessary\n",
            "#   IgBdAl = 0 \t# Abort on unreadable alternate axes\n",
            "#   CdMat = 0 \t# Use PC matrix\n",
            "#   CarLin = 0 \t# Use full FITS-WCS CAR projections\n",
            "#   SipRpl = 1 \t# Replace SIP inverse coefficients?\n",
            "#   FitsTol = 0.10000000000000001 \t# [pixel] Max allowed departure from linearity\n",
            "#   Iwc = 0 \t# Do not include an IWC Frame\n",
            "#   Warn = \"BadKeyName BadKeyValue Tnx Zpx BadPV BadCel BadMat BadCTYPE\" \t# Warnings to be reported\n",
            " End FitsChan\n",
        );
        let dump = |obj: &AnyObject| {
            let mut ch = NativeChannel::with_string_sink();
            ch.write(obj).unwrap();
            ch.into_string()
        };
        let text = dump(&AnyObject::FitsChan(Box::new(with("UseDefs=0").unwrap())));
        assert_eq!(text, C_DUMP);
        let back = crate::from_native_string(&text).unwrap();
        assert_eq!(dump(&back), C_DUMP);
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
