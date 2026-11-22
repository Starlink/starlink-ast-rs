// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The demo's command line, which names a FITS header to draw, with its
//! Plot and FitsChan attributes and plot box, in the order the C library's
//! `testgrid` program takes them.

use std::path::Path;

use crate::cases::DemoCase;

/// How to run the demo.
pub const USAGE: &str =
    "Usage: ast-plot-demo [<fits file> [<attrs> [<fattrs> [<xlo> <ylo> <xhi> <yhi>]]]]

  <fits file>  a FITS header to draw, as text with one card per line
  <attrs>      Plot attributes, such as \"Grid=1,Labelling=interior\"
  <fattrs>     FitsChan attributes for reading the header (\"carlin=1\")
  <xlo> ...    the plot box, in the header's base coordinates;
               the header's pixel grid if omitted

With no arguments, the window opens on the built-in grid test cases.";

/// A command-line case lives for the whole run, so its strings are leaked
/// rather than giving every case owned strings.
fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// The case the arguments after the program name describe, or `None` when
/// there are none; `read` reads the header file.
///
/// # Errors
///
/// A message for the user, with the usage when the arguments are malformed,
/// when the plot box has other than four values or a value that is not a
/// number, or when the header cannot be read.
pub fn case_from_args(
    args: &[String],
    read: impl FnOnce(&str) -> std::io::Result<String>,
) -> Result<Option<DemoCase>, String> {
    let Some(file) = args.first() else {
        return Ok(None);
    };
    if !matches!(args.len(), 1..=3 | 7) {
        return Err(format!(
            "ast-plot-demo: expected 1, 2, 3 or 7 arguments, got {}.\n\n{USAGE}",
            args.len()
        ));
    }
    let pbox = if args.len() == 7 {
        let mut pbox = [0.0; 4];
        for (v, arg) in pbox.iter_mut().zip(&args[3..]) {
            *v = arg.trim().parse().map_err(|_| {
                format!("ast-plot-demo: the plot box value \"{arg}\" is not a number.\n\n{USAGE}")
            })?;
        }
        Some(pbox)
    } else {
        None
    };
    let header = read(file).map_err(|e| format!("ast-plot-demo: cannot read {file}: {e}"))?;
    let name = Path::new(file)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file);
    let name = leak(name.to_owned());
    let arg = |i: usize| leak(args.get(i).cloned().unwrap_or_default());
    Ok(Some(DemoCase {
        name,
        head: name,
        header: leak(header),
        attr: arg(1),
        fattr: arg(2),
        pbox,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }

    fn read_ok(_: &str) -> std::io::Result<String> {
        Ok("NAXIS1  =                  300\nNAXIS2  =                  200\n".to_owned())
    }

    #[test]
    fn no_arguments_name_no_case() {
        assert!(case_from_args(&[], read_ok).unwrap().is_none());
    }

    #[test]
    fn a_header_alone_is_a_case() {
        let case = case_from_args(&args(&["dir/my.head"]), read_ok)
            .unwrap()
            .unwrap();
        assert_eq!(case.name, "my.head");
        assert_eq!(case.head, "my.head");
        assert!(case.header.starts_with("NAXIS1"));
        assert_eq!((case.attr, case.fattr, case.pbox), ("", "", None));
    }

    #[test]
    fn every_argument_reaches_the_case() {
        let a = args(&["my.head", "Grid=1", "carlin=1", "10", "-10", "300", "280.5"]);
        let case = case_from_args(&a, read_ok).unwrap().unwrap();
        assert_eq!((case.attr, case.fattr), ("Grid=1", "carlin=1"));
        assert_eq!(case.pbox, Some([10.0, -10.0, 300.0, 280.5]));
    }

    #[test]
    fn a_partial_box_is_an_error() {
        let a = args(&["my.head", "", "", "10", "-10", "300"]);
        let err = case_from_args(&a, read_ok).unwrap_err();
        assert!(err.contains(USAGE), "{err}");
    }

    #[test]
    fn a_box_value_that_is_not_a_number_is_an_error() {
        let a = args(&["my.head", "", "", "10", "-10", "x", "280"]);
        let err = case_from_args(&a, read_ok).unwrap_err();
        assert!(err.contains("\"x\""), "{err}");
    }

    #[test]
    fn an_unreadable_header_is_an_error() {
        let fail = |_: &str| Err(std::io::Error::other("no such file"));
        let err = case_from_args(&args(&["gone.head"]), fail).unwrap_err();
        assert!(
            err.contains("gone.head") && err.contains("no such file"),
            "{err}"
        );
    }
}
