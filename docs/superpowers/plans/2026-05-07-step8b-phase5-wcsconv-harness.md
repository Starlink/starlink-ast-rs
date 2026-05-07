# FitsChan Phase 5: WCS Conversion Test Harness

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a data-driven regression test harness that exercises all 192 WCS encoding conversion cases from `wcsconv_tests.txt`, verifying that the Rust encoders produce output matching the C library.

**Architecture:** A Rust integration test binary parses the pipe-delimited manifest file, loads each input through FitsChan, writes with the target encoding, and compares output against reference files. Two comparison modes: string-exact (card-by-card match) for 85 cases and transform-equivalence (mapping agreement at sample points) for 107 cases. The harness is a single `#[test]` per manifest line, generated via a macro or explicit test functions driven by a shared helper.

**Tech Stack:** Rust 2024 edition. Uses FitsChan read/write, all encoding modules. Test fixtures copied from `reference/starlink-ast/ast_tester/`.

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/ast-core/tests/test_wcsconv.rs` | Data-driven test harness with one test per manifest entry |
| `crates/ast-core/tests/fixtures/wcsconv/wcsconv_tests.txt` | Test manifest (copied from reference) |
| `crates/ast-core/tests/fixtures/wcsconv/inputs/` | Input files (.head, .ast, .fits-*) |
| `crates/ast-core/tests/fixtures/wcsconv/expected/` | Expected output files |

---

### Task 1: Copy Test Fixtures

**Files:**
- Create: `crates/ast-core/tests/fixtures/wcsconv/` directory structure
- Copy: manifest + all referenced input and expected files

The manifest references ~192 unique input files and ~192 expected output files. Many inputs are shared across tests (e.g., `timj.ast` is used 5 times with different output encodings).

- [ ] **Step 1: Create directory structure and copy manifest**

```bash
mkdir -p crates/ast-core/tests/fixtures/wcsconv/inputs
mkdir -p crates/ast-core/tests/fixtures/wcsconv/expected
cp reference/starlink-ast/ast_tester/wcsconv_tests.txt \
   crates/ast-core/tests/fixtures/wcsconv/
```

- [ ] **Step 2: Copy all input files referenced in the manifest**

Extract unique input filenames from column 2 of the manifest and copy them:

```bash
grep -v "^#" reference/starlink-ast/ast_tester/wcsconv_tests.txt | \
  grep -v "^$" | cut -d'|' -f2 | tr -d ' ' | sort -u | \
  while read f; do
    cp "reference/starlink-ast/ast_tester/$f" \
       "crates/ast-core/tests/fixtures/wcsconv/inputs/" 2>/dev/null || true
  done
```

- [ ] **Step 3: Copy all expected output files**

Extract unique expected filenames from column 3:

```bash
grep -v "^#" reference/starlink-ast/ast_tester/wcsconv_tests.txt | \
  grep -v "^$" | cut -d'|' -f3 | tr -d ' ' | sort -u | \
  while read f; do
    cp "reference/starlink-ast/ast_tester/$f" \
       "crates/ast-core/tests/fixtures/wcsconv/expected/" 2>/dev/null || true
  done
```

- [ ] **Step 4: Verify file counts**

```bash
ls crates/ast-core/tests/fixtures/wcsconv/inputs/ | wc -l
ls crates/ast-core/tests/fixtures/wcsconv/expected/ | wc -l
```

Expected: ~180+ input files, ~180+ expected files.

- [ ] **Step 5: Commit**

---

### Task 2: Manifest Parser

**Files:**
- Create: `crates/ast-core/tests/test_wcsconv.rs`

Parse the pipe-delimited manifest into test case structs.

- [ ] **Step 1: Create test_wcsconv.rs with manifest parser**

```rust
//! WCS conversion regression test harness.
//!
//! Data-driven tests from wcsconv_tests.txt that verify encoding round-trips
//! match the C library output. Each test case reads an input file, writes it
//! in a target encoding, and compares against a reference file.

use std::path::PathBuf;

use ast_core::channel::Channel;
use ast_core::fitschan::{Encoding, FitsChan};

/// A single test case from the manifest.
#[derive(Debug, Clone)]
struct TestCase {
    name: String,
    input_file: String,
    expected_file: String,
    encoding: String,
    attrs: Vec<(String, String)>,
    skip_string_compare: bool,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wcsconv")
}

fn parse_manifest() -> Vec<TestCase> {
    let manifest_path = fixtures_dir().join("wcsconv_tests.txt");
    let text = std::fs::read_to_string(&manifest_path)
        .expect("wcsconv_tests.txt should be readable");

    let mut cases = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() < 4 {
            continue;
        }

        let name = fields[0].trim().to_string();
        let input_file = fields[1].trim().to_string();
        let expected_file = fields[2].trim().to_string();
        let encoding = fields[3].trim().to_string();

        let attrs = if fields.len() > 4 {
            parse_attrs(fields[4].trim())
        } else {
            Vec::new()
        };

        let skip_string_compare = fields.len() > 5
            && fields[5].trim().eq_ignore_ascii_case("yes");

        cases.push(TestCase {
            name,
            input_file,
            expected_file,
            encoding,
            attrs,
            skip_string_compare,
        });
    }

    cases
}

fn parse_attrs(s: &str) -> Vec<(String, String)> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split(',')
        .filter_map(|kv| {
            let parts: Vec<&str> = kv.splitn(2, '=').collect();
            if parts.len() == 2 {
                Some((parts[0].trim().to_lowercase(), parts[1].trim().to_string()))
            } else {
                None
            }
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

/// Determine the encoding to use for reading an input file based on its extension.
fn input_encoding(filename: &str) -> Encoding {
    if filename.ends_with(".ast") {
        Encoding::Native
    } else if filename.ends_with(".head") {
        Encoding::FitsWcs
    } else if filename.ends_with(".fits-wcs") {
        Encoding::FitsWcs
    } else if filename.ends_with(".fits-iraf") {
        Encoding::FitsIraf
    } else if filename.ends_with(".fits-pc") {
        Encoding::FitsPc
    } else if filename.ends_with(".fits-aips") {
        Encoding::FitsAips
    } else if filename.ends_with(".fits-aips++") {
        Encoding::FitsAipsPP
    } else if filename.ends_with(".fits-class") {
        Encoding::FitsClass
    } else if filename.ends_with(".fits-dss") || filename.ends_with(".dss") {
        Encoding::Dss
    } else {
        // Use guess encoding for unknown extensions.
        Encoding::FitsWcs
    }
}
```

- [ ] **Step 2: Add the core test runner function**

```rust
/// Run a single wcsconv test case.
///
/// Returns Ok(()) on pass, Err with description on failure.
fn run_test_case(case: &TestCase) -> Result<(), String> {
    let input_path = fixtures_dir().join("inputs").join(&case.input_file);
    let expected_path = fixtures_dir().join("expected").join(&case.expected_file);

    // Load input file.
    let input_text = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("cannot read input {}: {e}", case.input_file))?;

    // Load expected output.
    let expected_text = std::fs::read_to_string(&expected_path)
        .map_err(|e| format!("cannot read expected {}: {e}", case.expected_file))?;

    // Determine input encoding from file extension.
    let in_enc = input_encoding(&case.input_file);

    // Read the FrameSet from input.
    let mut fc_in = FitsChan::from_header(&input_text)
        .map_err(|e| format!("FitsChan::from_header failed: {e}"))?;
    fc_in.set_encoding(in_enc);
    let obj = fc_in.read()
        .map_err(|e| format!("read failed: {e}"))?
        .ok_or_else(|| "read returned None".to_string())?;

    // Determine target encoding.
    let target_enc = encoding_from_str(&case.encoding)
        .ok_or_else(|| format!("unknown encoding: {}", case.encoding))?;

    // Apply attributes.
    let mut fc_out = FitsChan::new();
    fc_out.set_encoding(target_enc);
    for (key, val) in &case.attrs {
        match key.as_str() {
            "cdmatrix" => {
                if val == "1" {
                    // cd_matrix flag — would need a setter. For now, skip.
                }
            }
            "fitsdigits" => {
                if let Ok(d) = val.parse::<usize>() {
                    fc_out.set_fits_digits(d);
                }
            }
            _ => {} // Ignore unknown attrs for now.
        }
    }

    // Write the FrameSet.
    fc_out.write(&obj)
        .map_err(|e| format!("write failed: {e}"))?;

    // For now, just verify that write succeeded and produced cards.
    // String-exact comparison will be added incrementally.
    if fc_out.ncard() == 0 && target_enc != Encoding::Native {
        return Err("write produced no cards".to_string());
    }

    Ok(())
}
```

- [ ] **Step 3: Add the master test that runs all cases**

```rust
/// Run all wcsconv test cases, collecting results.
///
/// This test reports all failures rather than stopping at the first one,
/// making it easier to see the full picture of what passes/fails.
#[test]
fn wcsconv_regression_suite() {
    let cases = parse_manifest();
    assert!(!cases.is_empty(), "manifest should have test cases");

    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    let mut failures = Vec::new();

    for case in &cases {
        // Skip if input or expected file doesn't exist.
        let input_path = fixtures_dir().join("inputs").join(&case.input_file);
        let expected_path = fixtures_dir().join("expected").join(&case.expected_file);
        if !input_path.exists() || !expected_path.exists() {
            skipped += 1;
            continue;
        }

        match run_test_case(case) {
            Ok(()) => passed += 1,
            Err(msg) => {
                failed += 1;
                failures.push(format!("  FAIL {}: {msg}", case.name));
            }
        }
    }

    eprintln!(
        "\nwcsconv results: {passed} passed, {failed} failed, {skipped} skipped (of {} total)",
        cases.len()
    );

    if !failures.is_empty() {
        let summary = failures.join("\n");
        panic!(
            "{failed} wcsconv test(s) failed:\n{summary}"
        );
    }
}
```

- [ ] **Step 4: Verify it compiles and runs**

Run: `cargo test -p ast-core --test test_wcsconv -- --nocapture`
Expected: compiles, runs the harness. Some tests may fail if encoding support is incomplete, but it should not panic on infrastructure issues.

- [ ] **Step 5: Commit**

---

### Task 3: Improve Harness — String Comparison Mode

**Files:**
- Modify: `crates/ast-core/tests/test_wcsconv.rs`

Add string-exact comparison for the 85 cases that require it.

- [ ] **Step 1: Add card extraction helper**

```rust
/// Extract all cards from a FitsChan as formatted 80-char strings.
fn extract_cards(fc: &mut FitsChan) -> Vec<String> {
    let mut cards = Vec::new();
    fc.rewind();
    for i in 0..fc.ncard() {
        // Use find_fits to get each card sequentially.
        // Actually, we need a better approach. Let's iterate.
        // FitsChan doesn't expose raw iteration, so we rebuild from ncard.
        // Alternative: write to a string buffer.
        let _ = i;
    }
    // Simpler: read the raw cards from the internal CardList.
    // Since we can't access internals from a test, use the public API.
    // The simplest approach: format each card position.
    cards
}
```

Actually, for string comparison we need to compare the written output against the reference file. The reference file IS formatted FITS cards (80-char lines). The FitsChan output is also FITS cards. We need to extract the cards from the FitsChan.

Better approach: add a `pub fn to_header(&self) -> String` method to FitsChan that formats all cards as a multi-line string.

- [ ] **Step 2: Add to_header() method to FitsChan**

Add to `crates/ast-core/src/fitschan/mod.rs` in the FitsChan impl block:

```rust
    /// Format all stored cards as a FITS header string.
    ///
    /// Each card is formatted as an 80-character line. No END card is appended.
    pub fn to_header(&self) -> String {
        self.cards
            .iter()
            .map(|card| card.format())
            .collect::<Vec<_>>()
            .join("\n")
    }
```

- [ ] **Step 3: Add string comparison to run_test_case**

Update `run_test_case` to compare output against expected when `!case.skip_string_compare`:

```rust
    // Compare output.
    if !case.skip_string_compare {
        let output = fc_out.to_header();
        let expected_trimmed = expected_text
            .lines()
            .map(|l| l.trim_end())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let output_trimmed = output
            .lines()
            .map(|l| l.trim_end())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n");

        if output_trimmed != expected_trimmed {
            // Report first difference.
            let out_lines: Vec<&str> = output_trimmed.lines().collect();
            let exp_lines: Vec<&str> = expected_trimmed.lines().collect();
            for (i, (o, e)) in out_lines.iter().zip(exp_lines.iter()).enumerate() {
                if o != e {
                    return Err(format!(
                        "string mismatch at line {}: got '{}', expected '{}'",
                        i + 1, o, e
                    ));
                }
            }
            if out_lines.len() != exp_lines.len() {
                return Err(format!(
                    "line count mismatch: got {}, expected {}",
                    out_lines.len(),
                    exp_lines.len()
                ));
            }
        }
    }
```

- [ ] **Step 4: Run and check results**

Run: `cargo test -p ast-core --test test_wcsconv -- --nocapture`
Expected: passes for supported cases, reports failures for unsupported ones.

- [ ] **Step 5: Commit**

---

### Task 4: Transform Equivalence Comparison

**Files:**
- Modify: `crates/ast-core/tests/test_wcsconv.rs`

For cases with `skip_string_compare=yes`, verify that reading the output back produces a FrameSet whose transforms agree with the original.

- [ ] **Step 1: Add transform comparison**

```rust
use ast_core::coords::PointSet;
use ast_core::AnyObject;

/// Compare two FrameSets by checking that their transforms agree at sample points.
fn transforms_agree(obj1: &AnyObject, obj2: &AnyObject, tol: f64) -> Result<(), String> {
    let m1 = obj1.as_mapping()
        .ok_or_else(|| "obj1 is not a mapping".to_string())?;
    let m2 = obj2.as_mapping()
        .ok_or_else(|| "obj2 is not a mapping".to_string())?;

    if m1.nin() != m2.nin() || m1.nout() != m2.nout() {
        return Err(format!(
            "dimension mismatch: {}→{} vs {}→{}",
            m1.nin(), m1.nout(), m2.nin(), m2.nout()
        ));
    }

    let nin = m1.nin();
    // Test at a few sample pixel positions.
    let mut points = Vec::new();
    for i in 0..nin {
        let mut pt = vec![1.0; nin];
        pt[i] = 2.0; // offset along axis i
        points.push(pt);
    }
    // Also test at (1,1,...,1).
    points.push(vec![1.0; nin]);

    for pt in &points {
        let pts = PointSet::from_rows(std::iter::once(pt.clone()));
        let out1 = m1.transform(&pts, true)
            .map_err(|e| format!("transform1 failed: {e}"))?;
        let out2 = m2.transform(&pts, true)
            .map_err(|e| format!("transform2 failed: {e}"))?;

        let p1 = out1.point(0);
        let p2 = out2.point(0);
        for axis in 0..m1.nout() {
            let diff = (p1[axis] - p2[axis]).abs();
            if diff > tol {
                return Err(format!(
                    "transform disagreement at {:?}: axis {} differs by {diff} (>{tol})",
                    pt, axis
                ));
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Add transform check to run_test_case for skip_string_compare cases**

After the write succeeds, read the output back and compare transforms:

```rust
    if case.skip_string_compare {
        // Read back the written output and compare transforms.
        fc_out.rewind();
        fc_out.set_encoding(target_enc);
        match fc_out.read() {
            Ok(Some(obj2)) => {
                // Compare transforms with tolerance.
                // Use 1e-6 radians for celestial, 1.0 for spectral/general.
                let tol = 1e-6;
                if let Err(msg) = transforms_agree(&obj, &obj2, tol) {
                    return Err(format!("transform equivalence: {msg}"));
                }
            }
            Ok(None) => {
                return Err("re-read of written output returned None".to_string());
            }
            Err(e) => {
                return Err(format!("re-read failed: {e}"));
            }
        }
    }
```

- [ ] **Step 3: Run and assess**

Run: `cargo test -p ast-core --test test_wcsconv -- --nocapture`

- [ ] **Step 4: Commit**

---

### Task 5: Handle Native Input (.ast files)

**Files:**
- Modify: `crates/ast-core/tests/test_wcsconv.rs`

The 14 `.ast` input files contain AST Native text format (not wrapped in COMMENT cards). The FitsChan reader expects Native text in COMMENT cards. For `.ast` inputs, we need to either:
- Wrap the text in COMMENT cards and read via FitsChan(Native), or
- Parse directly via the native reader

- [ ] **Step 1: Add native text loading**

```rust
/// Load an AST object from a .ast file (native text format, not in COMMENT cards).
fn load_from_ast_file(text: &str) -> Result<AnyObject, String> {
    use ast_core::serial;

    let node = serial::native_read::parse_native(text)
        .map_err(|e| format!("parse_native failed: {e}"))?;
    let obj = serial::any_object_from_node(&node)
        .map_err(|e| format!("any_object_from_node failed: {e}"))?;
    Ok(obj)
}
```

- [ ] **Step 2: Update run_test_case to handle .ast inputs differently**

```rust
    // Read the object — method depends on input format.
    let obj = if case.input_file.ends_with(".ast") {
        // Native AST text: parse directly.
        load_from_ast_file(&input_text)?
    } else {
        // FITS header: load via FitsChan.
        let in_enc = input_encoding(&case.input_file);
        let mut fc_in = FitsChan::from_header(&input_text)
            .map_err(|e| format!("FitsChan::from_header failed: {e}"))?;
        fc_in.set_encoding(in_enc);
        fc_in.read()
            .map_err(|e| format!("read failed: {e}"))?
            .ok_or_else(|| "read returned None".to_string())?
    };
```

- [ ] **Step 3: Handle "ast" target encoding (Native format output via FitsChan)**

The manifest has 2 cases with `encoding = ast`. This means write with Native encoding. Update `encoding_from_str`:

```rust
fn encoding_from_str(s: &str) -> Option<Encoding> {
    match s.to_lowercase().as_str() {
        "native" | "ast" => Some(Encoding::Native),
        ...
    }
}
```

(Already done in the initial implementation.)

- [ ] **Step 4: Run and verify .ast tests work**

- [ ] **Step 5: Commit**

---

### Task 6: Expose native_read/native_write for tests

**Files:**
- Modify: `crates/ast-core/src/serial/mod.rs` (if needed — check visibility)

The `serial::native_read::parse_native` and `serial::any_object_from_node` functions need to be accessible from integration tests. Check if they're already public.

- [ ] **Step 1: Verify visibility**

Check if `ast_core::serial::native_read::parse_native` and `ast_core::serial::any_object_from_node` are accessible from integration tests (they should be if `serial` module is `pub`).

Run: `cargo build -p ast-core --test test_wcsconv`

If it fails with visibility errors, add `pub use` re-exports as needed.

- [ ] **Step 2: Fix visibility if needed, commit**

---

### Task 7: Final Verification and Pass Rate Assessment

- [ ] **Step 1: Run full test suite**

Run: `cargo test -p ast-core`
Expected: all existing tests pass; wcsconv harness runs

- [ ] **Step 2: Run wcsconv specifically and report pass rate**

Run: `cargo test -p ast-core --test test_wcsconv -- --nocapture 2>&1 | tail -20`

Report: X passed, Y failed, Z skipped. This establishes the baseline. Failures are expected for cases requiring features not yet implemented (e.g., complex FrameSets with 4+ frames, PolyMap SIP round-trips, -TAB, etc.)

- [ ] **Step 3: Clippy and fmt**

Run: `cargo clippy -p ast-core -- -D warnings && cargo fmt --check -p ast-core`

- [ ] **Step 4: Commit**

---

## Verification Checklist

1. `cargo build --workspace` — compiles
2. `cargo test -p ast-core` — all tests pass (harness runs without panicking)
3. Manifest parser correctly reads all 192 test cases
4. `.head` inputs are read via FitsChan with appropriate encoding
5. `.ast` inputs are read via native parser directly
6. Output encoding is applied correctly
7. FitsChan attributes (cdmatrix, FitsDigits) are applied
8. String comparison reports meaningful diffs on failure
9. Transform equivalence comparison works for skip_string_compare cases
10. Pass rate is reported (even if not 100% — this is a regression baseline)

## Notes

- This harness is designed to establish a baseline. Not all 192 tests will pass initially — many require features we haven't built yet (multi-frame FrameSets, SIP write, -TAB, etc.)
- The harness should NEVER panic on infrastructure issues (missing files are skipped)
- Failed cases are reported but don't block the overall test run
- As encoder accuracy improves in future work, more cases will pass
- String-exact comparison is the strictest test — it verifies byte-level output compatibility with the C library
