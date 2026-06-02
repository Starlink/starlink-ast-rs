//! WCS conversion regression test harness.
//!
//! Data-driven tests from wcsconv_tests.txt that verify encoding round-trips.
//! Each test reads an input, writes in a target encoding, and compares output.
//!
//! The harness is designed to be permissive: individual test failures are
//! reported but do not prevent the suite from completing. Only zero passes
//! (infrastructure failure) causes a panic.

use std::path::PathBuf;

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::{Encoding, FitsChan};

#[derive(Debug)]
struct TestCase {
    name: String,
    input_file: String,
    expected_file: String,
    encoding: String,
    attrs: Vec<(String, String)>,
    skip_string_compare: bool,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wcsconv")
}

fn parse_manifest() -> Vec<TestCase> {
    let path = fixtures_dir().join("wcsconv_tests.txt");
    let text = std::fs::read_to_string(&path).expect("manifest readable");

    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('|').collect();
            if fields.len() < 4 {
                return None;
            }
            let attrs = if fields.len() > 4 && !fields[4].trim().is_empty() {
                fields[4]
                    .trim()
                    .split(',')
                    .filter_map(|kv| {
                        let parts: Vec<&str> = kv.splitn(2, '=').collect();
                        if parts.len() == 2 {
                            Some((parts[0].trim().to_lowercase(), parts[1].trim().to_string()))
                        } else {
                            None
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let skip = fields.len() > 5 && fields[5].trim().eq_ignore_ascii_case("yes");

            Some(TestCase {
                name: fields[0].trim().to_string(),
                input_file: fields[1].trim().to_string(),
                expected_file: fields[2].trim().to_string(),
                encoding: fields[3].trim().to_string(),
                attrs,
                skip_string_compare: skip,
            })
        })
        .collect()
}

fn encoding_from_str(s: &str) -> Option<Encoding> {
    match s.to_lowercase().as_str() {
        "native" | "ast" => Some(Encoding::Native),
        "fits-wcs" => Some(Encoding::FitsWcs),
        "fits-pc" => Some(Encoding::FitsPc),
        "fits-iraf" => Some(Encoding::FitsIraf),
        "fits-aips" => Some(Encoding::FitsAips),
        "fits-aips++" => Some(Encoding::FitsAipsPP),
        "fits-class" => Some(Encoding::FitsClass),
        "dss" => Some(Encoding::Dss),
        _ => None,
    }
}

fn input_encoding_from_ext(filename: &str) -> Option<Encoding> {
    if filename.ends_with(".head") {
        None // Use guess
    } else if filename.ends_with(".ast") {
        // Special sentinel: direct native parse, not through FitsChan.
        Some(Encoding::Native)
    } else if filename.ends_with(".fits-wcs") {
        Some(Encoding::FitsWcs)
    } else if filename.ends_with(".fits-iraf") {
        Some(Encoding::FitsIraf)
    } else if filename.ends_with(".fits-pc") {
        Some(Encoding::FitsPc)
    } else if filename.ends_with(".fits-aips++") {
        Some(Encoding::FitsAipsPP)
    } else if filename.ends_with(".fits-aips") {
        Some(Encoding::FitsAips)
    } else if filename.ends_with(".fits-class") {
        Some(Encoding::FitsClass)
    } else if filename.ends_with(".fits-dss") || filename.ends_with(".dss") {
        Some(Encoding::Dss)
    } else {
        None
    }
}

/// Load an AST object from input. Returns the object or an error description.
fn load_input(case: &TestCase) -> Result<AnyObject, String> {
    let input_path = fixtures_dir().join("inputs").join(&case.input_file);
    let text = std::fs::read_to_string(&input_path).map_err(|e| format!("read input: {e}"))?;

    if case.input_file.ends_with(".ast") {
        // Native AST text -- parse directly via the public API.
        ast_core::from_native_string(&text).map_err(|e| format!("from_native_string: {e}"))
    } else {
        // FITS header -- load via FitsChan.
        let mut fc = FitsChan::from_header(&text).map_err(|e| format!("from_header: {e}"))?;

        // Determine encoding.
        match input_encoding_from_ext(&case.input_file) {
            Some(enc) => fc.set_encoding(enc),
            None => {
                fc.guess_encoding();
            }
        }

        fc.read()
            .map_err(|e| format!("read: {e}"))?
            .ok_or_else(|| "read returned None".to_string())
    }
}

/// Run a single test case. Returns Ok on pass, Err with description on failure.
fn run_test_case(case: &TestCase) -> Result<(), String> {
    // Load input object.
    let obj = load_input(case)?;

    // Determine target encoding.
    let target_enc = encoding_from_str(&case.encoding)
        .ok_or_else(|| format!("unknown encoding: {}", case.encoding))?;

    // Set up output FitsChan. For FITS header inputs, pre-load the input
    // cards so write() can purge old WCS keywords while preserving non-WCS
    // structural keywords (SIMPLE, NAXIS, etc.). This mirrors wcsconverter.
    let mut fc_out = if !case.input_file.ends_with(".ast") {
        let input_path = fixtures_dir().join("inputs").join(&case.input_file);
        let text = std::fs::read_to_string(&input_path).map_err(|e| format!("read input: {e}"))?;
        FitsChan::from_header(&text).map_err(|e| format!("from_header: {e}"))?
    } else {
        FitsChan::new()
    };
    fc_out.set_encoding(target_enc);

    // Apply attributes.
    for (key, val) in &case.attrs {
        match key.as_str() {
            "fitsdigits" => {
                if let Ok(d) = val.parse::<usize>() {
                    fc_out.set_fits_digits(d);
                }
            }
            "cdmatrix" => {
                fc_out.set_cd_matrix(val == "1" || val.eq_ignore_ascii_case("true"));
            }
            "fitsrounding" => {
                if let Ok(r) = val.parse::<usize>() {
                    fc_out.set_fits_rounding(r);
                }
            }
            // carlin, sipreplace, sipok -- not yet supported.
            _ => {}
        }
    }

    // Write.
    fc_out.write(&obj).map_err(|e| format!("write: {e}"))?;

    // Verify output is non-empty.
    if fc_out.ncard() == 0 && target_enc != Encoding::Native {
        return Err("write produced 0 cards".to_string());
    }

    // String comparison (if not skipped).
    if !case.skip_string_compare {
        let expected_path = fixtures_dir().join("expected").join(&case.expected_file);
        let expected_text =
            std::fs::read_to_string(&expected_path).map_err(|e| format!("read expected: {e}"))?;

        let output = fc_out.to_header();

        // Normalize: trim trailing whitespace per line, skip empty lines,
        // and drop the trailing FITS `END` marker (some upstream expected
        // files include it, others omit it; our writer always emits one).
        fn normalize(text: &str) -> Vec<&str> {
            let mut lines: Vec<&str> = text
                .lines()
                .map(|l| l.trim_end())
                .filter(|l| !l.is_empty())
                .collect();
            if lines.last().is_some_and(|l| l.trim() == "END") {
                lines.pop();
            }
            lines
        }
        let exp_lines = normalize(&expected_text);
        let out_lines = normalize(&output);

        // Find first difference.
        for (i, (o, e)) in out_lines.iter().zip(exp_lines.iter()).enumerate() {
            if o != e {
                return Err(format!(
                    "line {} differs:\n  got: '{}'\n  exp: '{}'",
                    i + 1,
                    o,
                    e
                ));
            }
        }
        if out_lines.len() != exp_lines.len() {
            return Err(format!(
                "line count: got {}, expected {}",
                out_lines.len(),
                exp_lines.len()
            ));
        }
    }

    Ok(())
}

/// Look up a case by name in the manifest and run it, panicking on
/// failure. Used by the per-fixture `#[test]` functions generated in
/// `build.rs`.
fn run_named_case(name: &str) {
    let cases = parse_manifest();
    let case = cases
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("manifest has no case named {name:?}"));

    let input_path = fixtures_dir().join("inputs").join(&case.input_file);
    if !input_path.exists() {
        // Skip silently: missing input fixture isn't a failure of the
        // code under test. Use eprintln so the user can see why
        // `cargo test <name>` reported "ok" without doing real work.
        eprintln!("[skip] {}: input fixture missing", case.name);
        return;
    }

    if let Err(msg) = run_test_case(case) {
        panic!("{}: {}", case.name, msg);
    }
}

include!(concat!(env!("OUT_DIR"), "/wcsconv_generated.rs"));
