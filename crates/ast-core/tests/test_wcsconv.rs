//! WCS conversion regression test harness.
//!
//! Data-driven tests from `fixtures/wcsconv/cases.txt` that verify encoding
//! round-trips.
//! Each test reads an input, writes in a target encoding, and compares output.
//!
//! `build.rs` generates one `#[test]` per manifest case, each of which panics
//! on any difference from the reference. Every case is compared:
//! [`NATIVE_READ_EXCLUDED`] is empty and gated so it cannot silently refill.

use std::path::PathBuf;

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::{Encoding, FitsChan};

use crate::common;

/// How the produced output is compared against the reference fixture.
///
/// The manifest's 6th column selects the mode per case. The default is
/// tight (FITS encodings byte-exact; NATIVE-family ≤4-ULP per token); cases
/// are relaxed only when they genuinely need it, keeping the comparison
/// strict everywhere else.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CompareMode {
    /// Tight: FITS exact-string, NATIVE-family ≤4-ULP per numeric token.
    Strict,
    /// Floating-point equivalence on numeric tokens, using AST's `astEQUAL`
    /// tolerance — the Rust-side equivalent of the C oracle's `astEqual`
    /// check. For outputs carrying values whose last bits are platform-libm
    /// dependent (e.g. cancellation-amplified geodetic altitude). `astequal`.
    AstEqual,
    /// Upstream runs no string comparison for this row. No wcsconv row carries
    /// it today, and this port treats it as strict anyway: where upstream cannot
    /// compare, this harness still can, because a case it cannot byte-compare is
    /// named in [`NATIVE_READ_EXCLUDED`] with a reason rather than waved through
    /// by a manifest flag.
    NoStringCompare,
}

impl CompareMode {
    /// Parse the manifest's 6th column, which is upstream's
    /// `skip_string_compare` and takes three values.
    ///
    /// Panics on anything else. Mapping an unrecognised value to a default —
    /// even the strict one — means a typo silently decides how a case is
    /// compared, and the manifest is shared with the upstream harness, which
    /// fails its configure on the same input.
    ///
    /// `yes` means upstream runs no string comparison. No wcsconv row carries it
    /// today; a native case this port cannot yet byte-compare is named in
    /// [`NATIVE_READ_EXCLUDED`] instead, with its reason.
    fn parse(field: &str) -> Self {
        match field.trim().to_ascii_lowercase().as_str() {
            "" => CompareMode::Strict,
            "astequal" => CompareMode::AstEqual,
            "yes" => CompareMode::NoStringCompare,
            other => panic!(
                "wcsconv/cases.txt: unknown skip_string_compare {other:?}; \
                 expected blank, \"astequal\" or \"yes\""
            ),
        }
    }
}

#[derive(Debug)]
struct TestCase {
    name: String,
    input_file: String,
    expected_file: String,
    encoding: String,
    attrs: Vec<(String, String)>,
    compare_mode: CompareMode,
}

/// The fixture root. Manifest rows carry paths relative to it -- the same paths
/// the upstream harness reads -- so nothing here prepends a subdirectory.
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// `native`-encoding wcsconv cases whose keyword-card output is not yet
/// byte-exact with the C oracle.
///
/// **Empty, and intended to stay empty**: all 194 wcsconv cases are compared
/// against their C-generated reference. `WCSCONV_NO_EXCLUDE=1` is therefore a
/// no-op, and both runs report the same counts — which is what proves the list
/// is empty rather than that the comparison was turned off.
///
/// A new entry is justified only by a difference that is *not* a port defect:
/// a C artifact the port deliberately does not reproduce, and that the
/// comparator cannot absorb. Aliasing-driven tag placement was the one such
/// case, and it is handled by `common::is_issimp_tag_card` in the comparator
/// instead (see `docs/issues/c-library-quirks.md`), so a new entry needs a
/// reason of that kind, recorded in that document, and not merely a diff
/// nobody has diagnosed yet. A FITS-read gap is a bug to fix, not an entry to
/// add.
const NATIVE_READ_EXCLUDED: &[(&str, &str)] = &[];

/// Cases where the port's `IsSimp` placement differs from C's in the FitsChan
/// read path, with the card count pinned on each side.
///
/// C's reader reduces Mappings as it assembles them, so its cards record the
/// order that reader worked in. This port simplifies once, at the end. The
/// resulting Mapping is the same; only which sub-Mappings were momentarily a
/// whole simplification result differs. That is an internal detail of C's
/// reader, not a port gap, and is the one place the design permits a residue.
/// The mechanism, with the `fitschan.c` call sites, is in
/// `docs/issues/c-library-quirks.md`, "`IsSimp` placement in a FITS-WCS read
/// records the order the reader simplified in".
///
/// Every other card, `Invert` included, is byte-compared, which is what makes
/// this safe to leave: a difference in the Mapping's *shape* could not hide
/// behind a licence that only drops tag lines and pins their count.
///
/// Every row must name a C behaviour. A row may never cite a stale reference:
/// the corpus is checked against the pinned oracle by
/// `tools/oracle_fixtures/check_corpus_fresh.sh`.
///
/// Fields: case name, cards this port emits, cards C emits.
const ISSIMP_PLACEMENT_LICENSED: &[(&str, usize, usize)] = &[
    // C emits more cards than the port in each of these: its reader reached a
    // reduced form in stages, and every stage's result carried a card. Which
    // stage produced which card has not been attributed per case; doing so
    // needs the instrumented oracle in
    // tools/oracle_fixtures/build_issimp_instrumented.sh. The counts are
    // measured against the pinned C, so the residue cannot grow unnoticed.
    ("axes-swap_native", 3, 4),
    ("vopt-f2w_native", 4, 5),
    ("wave-f2w_native", 4, 5),
];

fn parse_manifest() -> Vec<TestCase> {
    let path = fixtures_dir().join("wcsconv/cases.txt");
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
            let compare_mode = if fields.len() > 5 {
                CompareMode::parse(fields[5])
            } else {
                CompareMode::Strict
            };

            Some(TestCase {
                name: fields[0].trim().to_string(),
                input_file: fields[1].trim().to_string(),
                expected_file: fields[2].trim().to_string(),
                encoding: fields[3].trim().to_string(),
                attrs,
                compare_mode,
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

#[allow(dead_code)]
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

/// Load an AST object from input. Returns the object plus, for FITS-encoded
/// inputs, the FitsChan it was read through. Mirrors `ast_tester/wcsconverter.c`
/// which reuses the same FitsChan for both read and write.
fn load_input(case: &TestCase) -> Result<(AnyObject, Option<FitsChan>), String> {
    let input_path = fixtures_dir().join(&case.input_file);
    let text = std::fs::read_to_string(&input_path).map_err(|e| format!("read input: {e}"))?;

    if case.input_file.ends_with(".ast") {
        // Native AST text -- parse directly via the public API.
        let obj =
            ast_core::from_native_string(&text).map_err(|e| format!("from_native_string: {e}"))?;
        Ok((obj, None))
    } else {
        // FITS header -- load via FitsChan. Mirror C wcsconverter.c
        // (lines 86-113) which creates the FitsChan with no explicit
        // encoding, applies attrs from the test, and reads. The
        // FitsChan auto-detects the encoding from the cards on read
        // when no explicit encoding has been set — same semantics as
        // C `astGetEncoding` (fitschan.c:12466).
        let fc = FitsChan::from_header(&text).map_err(|e| format!("from_header: {e}"))?;
        let mut fc = fc;
        // C wcsconverter applies the attribute string *before* astRead
        // (wcsconverter.c:87-113); SipReplace and CarLin affect the read.
        for (key, val) in &case.attrs {
            let on = val == "1" || val.eq_ignore_ascii_case("true");
            match key.as_str() {
                "sipreplace" => fc.set_sip_replace(on),
                "carlin" => fc.set_car_lin(on),
                _ => {}
            }
        }
        let obj = fc
            .read()
            .map_err(|e| format!("read: {e}"))?
            .ok_or_else(|| "read returned None".to_string())?;
        Ok((obj, Some(fc)))
    }
}

/// Run a single test case. Returns Ok on pass, Err with description on failure.
fn run_test_case(case: &TestCase) -> Result<(), String> {
    run_case(case, false)
}

/// Run a case with the [`NATIVE_READ_EXCLUDED`] bypass forced on, whatever
/// the environment says. Used by the stale-entry gate, which must ask "does
/// this excluded case actually still differ?".
fn run_with_comparison_forced(case: &TestCase) -> Result<(), String> {
    run_case(case, true)
}

fn run_case(case: &TestCase, force_comparison: bool) -> Result<(), String> {
    // Load input object plus the FitsChan it was read through.
    let (obj, fc_in) = load_input(case)?;

    // Determine target encoding.
    let target_enc = encoding_from_str(&case.encoding)
        .ok_or_else(|| format!("unknown encoding: {}", case.encoding))?;

    // Reuse the input FitsChan for write, mirroring
    // `ast_tester/wcsconverter.c`. wcsconverter reads cards, calls
    // astRead (which marks consumed cards as USED on the FitsChan),
    // then sets ENCODING and writes back to the same chan. Reusing
    // the chan ensures the post-read USED flags and any cards
    // spec_trans translated *into the same chan* (RESTFREQ→RESTFRQ
    // etc.) are visible to the write path's mark/sweep logic.
    let mut fc_out = fc_in.unwrap_or_else(FitsChan::new);
    fc_out.set_encoding(target_enc);

    // Apply attributes.
    for (key, val) in &case.attrs {
        match key.as_str() {
            "fitsdigits" => fc_out.set_fits_digits(val.parse().expect("FitsDigits is an integer")),
            "cdmatrix" => {
                fc_out.set_cd_matrix(val == "1" || val.eq_ignore_ascii_case("true"));
            }
            "fitsrounding" => {
                if let Ok(r) = val.parse::<usize>() {
                    fc_out.set_fits_rounding(r);
                }
            }
            "sipok" => fc_out.set_sip_ok(val == "1" || val.eq_ignore_ascii_case("true")),
            // Read-side, applied in load_input.
            "sipreplace" | "carlin" => {}
            other => panic!("wcsconv/cases.txt: unknown attribute {other:?}"),
        }
    }

    // The manifest distinguishes two NATIVE-family targets, mirroring C
    // `wcsconverter.c` (which branches on `strcasecmp(encoding, "AST")`):
    //   - `ast`    → an astChannel text dump (`.ast`, plain "Begin FrameSet")
    //   - `native` → a FitsChan write under NATIVE encoding (`.native`, the
    //                COMMENT-banner + BEGAST/ENDAST/`<NAME>_xx` keyword-card
    //                form). Both decode to `Encoding::Native`, so the manifest
    //                encoding string is the discriminator.
    let is_ast_text = case.encoding.eq_ignore_ascii_case("ast");
    let text_output = if is_ast_text {
        Some(ast_core::to_native_string(&obj).map_err(|e| format!("to_native_string: {e}"))?)
    } else {
        None
    };

    // Write into the FitsChan for every FITS encoding and for `native`.
    // `wcsconverter.c` clears Card first, so the cards go at the top.
    if text_output.is_none() {
        fc_out.clear_card();
        fc_out.write(&obj).map_err(|e| format!("write: {e}"))?;
    }

    // Verify output is non-empty.
    if fc_out.ncard() == 0 && !is_ast_text {
        return Err("write produced 0 cards".to_string());
    }

    // String comparison.
    //
    // NATIVE-family outputs (the `ast` text dump and the `native` keyword-card
    // form) carry full-precision (`AST__DBL_DIG`) numeric tokens, so a 1-ULP
    // libm difference between the Rust port and the C oracle shows up as a
    // last-digit string difference. These are compared with a ≤4-ULP
    // tolerance on each numeric token — the same comparator the simplify
    // regression harness uses (factored into `tests/common`).
    //
    // FITS encodings stay byte-exact: their numeric tokens are already rounded
    // to FitsDigits during stringification, so a sub-ULP input difference
    // rounds to the identical printed string (exact match), while any genuine
    // difference shows up in the last printed digit (~1e-8, far beyond 4 ULP)
    // which the tolerant comparator would reject anyway. Tolerance buys
    // nothing for FITS and would only risk loosening the fixed-column card
    // check.
    let is_native = target_enc == Encoding::Native;
    // Native cases are byte-compared by default; only those still failing the
    // FITS-read round-trip are excluded (explicitly, with a reason, in
    // NATIVE_READ_EXCLUDED). This keeps the comparison visible instead of
    // hidden behind a manifest skip flag. Set WCSCONV_NO_EXCLUDE=1 to compare
    // every native case (to measure progress against the exclusion list).
    let native_excluded = !force_comparison
        && !std::env::var("WCSCONV_NO_EXCLUDE").is_ok_and(|v| !v.is_empty())
        && is_native
        && NATIVE_READ_EXCLUDED.iter().any(|(n, _)| *n == case.name);
    // Every case is compared. `AstEqual` only relaxes the comparator, and is
    // not gated by the exclusion list (which tracks FITS-read bugs, not
    // tolerance); a native case is skipped only while it is named in
    // NATIVE_READ_EXCLUDED.
    let compare = case.compare_mode == CompareMode::AstEqual || !native_excluded;
    if compare {
        let expected_path = fixtures_dir().join(&case.expected_file);
        let expected_text =
            std::fs::read_to_string(&expected_path).map_err(|e| format!("read expected: {e}"))?;

        let output = match &text_output {
            Some(s) => s.clone(),
            None => fc_out.to_header().unwrap(),
        };

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
        let mut exp_lines = normalize(&expected_text);
        let mut out_lines = normalize(&output);
        if let Some((_, want_got, want_exp)) = ISSIMP_PLACEMENT_LICENSED
            .iter()
            .find(|(n, _, _)| *n == case.name)
        {
            let n_got = out_lines
                .iter()
                .filter(|l| common::is_issimp_tag_card(l))
                .count();
            let n_exp = exp_lines
                .iter()
                .filter(|l| common::is_issimp_tag_card(l))
                .count();
            if (n_got, n_exp) != (*want_got, *want_exp) {
                return Err(format!(
                    "{}: IsSimp card count is ({n_got}, {n_exp}), licensed as \
                     ({want_got}, {want_exp})",
                    case.name
                ));
            }
            exp_lines.retain(|l| !common::is_issimp_tag_card(l));
            out_lines.retain(|l| !common::is_issimp_tag_card(l));
        }

        // Find first difference. Comparator by mode/encoding:
        //   AstEqual → astEQUAL float-equivalence on numeric tokens;
        //   native Strict → ≤4-ULP numeric drift per token;
        //   FITS Strict → exact match (tokens already rounded to FitsDigits).
        for (i, (o, e)) in out_lines.iter().zip(exp_lines.iter()).enumerate() {
            let equivalent = if case.compare_mode == CompareMode::AstEqual {
                common::line_tokens_astequal(o, e)
            } else if is_native {
                common::line_tokens_equivalent(o, e, 4)
            } else {
                o == e
            };
            if !equivalent {
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

    let input_path = fixtures_dir().join(&case.input_file);
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

/// Every `NATIVE_READ_EXCLUDED` entry must still fail. An entry that has
/// started passing is a stale exclusion: the comparison it was suppressing
/// now succeeds, so the entry hides nothing and must be deleted.
///
/// This is the other half of the exclusion list's bookkeeping. The generated
/// per-case tests catch a case that regresses; this catches one that has been
/// fixed without the list being updated. Together they mean the list can only
/// be wrong in a way some test reports.
#[test]
fn native_read_excluded_entries_still_fail() {
    let cases = parse_manifest();
    let mut stale = Vec::new();
    let mut unknown = Vec::new();

    for (name, reason) in NATIVE_READ_EXCLUDED {
        let Some(case) = cases.iter().find(|c| c.name == *name) else {
            unknown.push(*name);
            continue;
        };
        if !fixtures_dir().join(&case.input_file).exists() {
            unknown.push(*name);
            continue;
        }
        // Compare unconditionally: `run_test_case` consults the exclusion
        // list itself, so ask it the question with the list bypassed.
        if run_with_comparison_forced(case).is_ok() {
            stale.push(format!("{name}  (recorded reason: {reason})"));
        }
    }

    assert!(
        unknown.is_empty(),
        "NATIVE_READ_EXCLUDED names cases the manifest does not have, or whose \
         input fixture is missing:\n{unknown:?}",
    );
    assert!(
        stale.is_empty(),
        "NATIVE_READ_EXCLUDED entries now pass — delete them:\n{}",
        stale.join("\n"),
    );
}

include!(concat!(env!("OUT_DIR"), "/wcsconv_generated.rs"));
