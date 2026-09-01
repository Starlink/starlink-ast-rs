//! Data-driven simplification regression tests.
//!
//! Reads `simplify_tests.txt` and for each entry: loads the input .map file,
//! simplifies it, and verifies:
//!   1. `equal()` — the simplified result must be structurally equal to the
//!      C reference (.simp file). This is the primary assertion.
//!   2. Transform equivalence — the simplified result must produce the same
//!      numerical output as the C reference on test points.
//!   3. Serialization (optional) — when `skip_string_compare` is not set,
//!      the native serialization of the simplified result must match the
//!      .simp file byte-for-byte.

use std::path::PathBuf;

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::FrameRef;
use ast_core::mappings::Mapping;
use ast_core::{AnyObject, from_native_string, to_native_string};

mod common;

/// Generate deterministic test points for a mapping with `nin` inputs.
/// Points are in the range [0.1, 1.1] to avoid singularities at zero.
fn make_test_points(nin: usize, npoint: usize) -> PointSet {
    let mut rows = Vec::with_capacity(npoint);
    for i in 0..npoint {
        let mut row = Vec::with_capacity(nin);
        for j in 0..nin {
            row.push(0.1 * (i as f64 + 1.0) + 0.01 * (j as f64));
        }
        rows.push(row);
    }
    PointSet::from_rows(rows)
}

/// Check whether two output point sets are equivalent within tolerance.
fn outputs_equivalent(a: &PointSet, b: &PointSet, tol: f64) -> bool {
    if a.npoint() != b.npoint() || a.ncoord() != b.ncoord() {
        return false;
    }
    for i in 0..a.npoint() {
        let pa = a.point(i);
        let pb = b.point(i);
        for j in 0..a.ncoord() {
            let va = pa[j];
            let vb = pb[j];
            if va == BAD_VALUE && vb == BAD_VALUE {
                continue;
            }
            if va == BAD_VALUE || vb == BAD_VALUE {
                return false;
            }
            let diff = (va - vb).abs();
            if diff > tol {
                return false;
            }
        }
    }
    true
}

/// Try to wrap a DynMapping back into an AnyObject for serialization.
/// Downcasts to known concrete types.
/// Cases whose only remaining difference from the C reference is where an
/// `IsSimp` card sits, with the exact card count on each side.
///
/// `IsSimp` records the set of sub-Mappings that were, at some instant, the
/// entire result of a simplification. Two engines that reach the same Mapping by
/// different internal routes disagree on that set, and the Mapping here *is* the
/// same — `equal()` and the transform check both pass. Aligning the routes, not
/// adding a stamp, is what closes a row: `cap_cmpmap_03` closed when the
/// port stopped simplifying the reassembled parallel CmpMap that C hands to the
/// list as built.
///
/// `cap_specmap_02` is short one card: C tags the CmpMap wrapping the
/// SpecMap/ZoomMap pair, which the port never forms because of the WinMap
/// under-merge of PLAN.md §10.3. The over-tagging half of that row is closed —
/// the port no longer simplifies each member of a dimension-matched sublist.
///
/// Measured dead ends, so they are not retried: tagging the reduced leaf in
/// `flatten`; tagging the reconstructed parallel; tagging every
/// `simplify_mapping` result; and deferring leaf reduction entirely. The first
/// three over-tagged `cap_lutmap_01` / `cap_matrixmap_03` / `cap_zoommap_01`;
/// the last broke four unrelated cases.
///
/// The licence is per case, drops only the `IsSimp` lines, and pins both counts
/// — so it cannot absorb a new difference, and the moment the port emits the
/// missing card the count check fails, which is the signal to delete the row.
///
/// Fields: name, cards this port emits, cards C emits, what differs.
const ISSIMP_PLACEMENT_LICENSED: &[(&str, usize, usize, &str)] = &[(
    "cap_specmap_02",
    7,
    8,
    "C tags the CmpMap wrapping the SpecMap/ZoomMap pair, which the port \
         never forms (the WinMap under-merge of PLAN.md §10.3)",
)];

fn mapping_to_any_object(m: &dyn Mapping) -> Option<AnyObject> {
    AnyObject::from_mapping(m)
}

/// Render `m` with the default options (comments on, indent 3).
fn mapping_to_native_string(m: &dyn Mapping) -> Option<String> {
    to_native_string(&mapping_to_any_object(m)?).ok()
}

/// The serialisation options a manifest row asks for.
///
/// C's `Channel` exposes `Comment` and `Indent`, and some references were
/// captured with them off. Reproducing a reference then needs the same
/// settings, not a licence to ignore the difference. See
/// `docs/issues/c-library-quirks.md`, "Two simplify references were captured
/// with Comment and Indent off".
#[derive(Debug, Clone, Copy)]
struct RenderOpts {
    comment: bool,
    indent: usize,
}

impl Default for RenderOpts {
    fn default() -> Self {
        Self {
            comment: true,
            indent: 3,
        }
    }
}

impl RenderOpts {
    /// Parse a manifest field like `comment=0,indent=0`. An unknown key is a
    /// hard error rather than a silent default: a typo must not quietly widen
    /// the comparison.
    fn parse(spec: &str, name: &str) -> Self {
        let mut opts = Self::default();
        for kv in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (k, v) = kv
                .split_once('=')
                .unwrap_or_else(|| panic!("{name}: bad render option {kv:?}, want key=value"));
            match k.trim() {
                "comment" => opts.comment = v.trim() != "0",
                "indent" => {
                    opts.indent = v
                        .trim()
                        .parse()
                        .unwrap_or_else(|_| panic!("{name}: bad indent {v:?}"))
                }
                other => panic!("{name}: unknown render option {other:?}"),
            }
        }
        opts
    }
}

/// [`mapping_to_native_string`] honouring `opts`.
fn mapping_to_native_string_with(m: &dyn Mapping, opts: RenderOpts) -> Option<String> {
    if opts.comment && opts.indent == 3 {
        return mapping_to_native_string(m);
    }
    use ast_core::channel::{Channel, NativeChannel};
    let obj = mapping_to_any_object(m)?;
    let mut ch = NativeChannel::with_string_sink();
    ch.config_mut().comment = opts.comment;
    ch.config_mut().indent = opts.indent;
    ch.write(&obj).ok()?;
    Some(ch.into_string())
}

#[test]
fn simplify_regression() {
    let fixture_dir = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/native/reference"
    ));
    let manifest = fixture_dir.join("simplify_tests.txt");

    if !manifest.exists() {
        eprintln!("simplify_tests.txt not found, skipping regression tests");
        return;
    }

    let contents = std::fs::read_to_string(&manifest).unwrap();
    let mut passed = 0usize;
    let mut skipped_missing = 0usize;
    let mut parse_skipped: Vec<String> = Vec::new();
    let mut equal_failures: Vec<String> = Vec::new();
    let mut transform_failures: Vec<String> = Vec::new();
    let mut serial_failures: Vec<String> = Vec::new();
    let mut other_failures: Vec<String> = Vec::new();
    let mut issimp_disagreements: Vec<String> = Vec::new();

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if fields.len() < 3 {
            continue;
        }

        let name = fields[0];
        let input_path = fixture_dir.join(fields[1]);
        let reference_path = fixture_dir.join(fields[2]);
        let skip_string_compare = fields.get(3).map(|s| s.trim()) == Some("yes");
        // Field 5: the Channel settings the reference was captured with, as
        // `comment=0,indent=0`. Blank means C's defaults.
        let render_opts = RenderOpts::parse(fields.get(4).copied().unwrap_or(""), name);
        // A negative fixture names the same file as both input and reference:
        // the assertion is that `simplify` leaves the Mapping alone. C's
        // `astSimplify` still stamps `IsSimple` on whatever it returns
        // (mapping.c:24759), so the result carries an `IsSimp = 1` line the
        // never-simplified reference cannot have. That one token is the only
        // licensed difference for these fixtures; every other line must match.
        let ignore_is_simp_line = fields[1] == fields[2];

        // Skip if files don't exist (expected for any not-yet-imported entry)
        if !input_path.exists() || !reference_path.exists() {
            skipped_missing += 1;
            continue;
        }

        // Parse input .map. A present-but-unparseable fixture is a parse
        // skip: an unported class (IntraMap/XphMap — but those are BLOCKED
        // in the manifest, so should not reach here) or a genuine parser gap.
        let input_text = match std::fs::read_to_string(&input_path) {
            Ok(t) => t,
            Err(_) => {
                parse_skipped.push(name.to_string());
                continue;
            }
        };
        let input_obj = match from_native_string(&input_text) {
            Ok(obj) => obj,
            Err(_) => {
                parse_skipped.push(name.to_string());
                continue;
            }
        };
        // Parse reference .simp
        let ref_text = match std::fs::read_to_string(&reference_path) {
            Ok(t) => t,
            Err(_) => {
                parse_skipped.push(name.to_string());
                continue;
            }
        };
        let ref_obj = match from_native_string(&ref_text) {
            Ok(obj) => obj,
            Err(_) => {
                parse_skipped.push(name.to_string());
                continue;
            }
        };
        // Most FrameSet fixtures were generated as
        // `astSimplify(astGetMapping(fs, AST__BASE, AST__CURRENT))`, so their
        // reference is a Mapping and the input has to be reduced to the same
        // composed Mapping for all three checks to compare like against like.
        // A fixture whose reference is itself a FrameSet was generated as
        // `astSimplify(fs)`, and there the FrameSet *is* the subject.
        let reference_is_frameset = matches!(ref_obj, AnyObject::FrameSet(_));
        let ref_mapping = match ref_obj.into_mapping() {
            Some(m) => m,
            None => {
                parse_skipped.push(name.to_string());
                continue;
            }
        };
        let mapping = match input_obj {
            AnyObject::FrameSet(fs) if !reference_is_frameset => {
                match fs.mapping(FrameRef::Base, FrameRef::Current) {
                    Ok(m) => m,
                    Err(_) => {
                        parse_skipped.push(name.to_string());
                        continue;
                    }
                }
            }
            other => match other.into_mapping() {
                Some(m) => m,
                None => {
                    parse_skipped.push(name.to_string());
                    continue;
                }
            },
        };

        // Simplify (use public simplify() which sets IsSimp flag)
        let simplified = match mapping.simplify() {
            Ok(s) => s,
            Err(e) => {
                other_failures.push(format!("{name}: simplify() failed: {e}"));
                continue;
            }
        };

        // === Check 1: equal() — structural equality with C reference ===
        let structurally_equal = simplified.equal(ref_mapping.as_ref());
        if !structurally_equal {
            equal_failures.push(name.to_string());
        }

        // === Check 2: Transform equivalence with C reference ===
        let nin = mapping.nin();
        let npoints = 10;
        let pts = make_test_points(nin, npoints);

        let out_ref = match ref_mapping.transform(&pts, true) {
            Ok(o) => o,
            Err(_) => {
                // Reference can't be evaluated — skip transform check
                if structurally_equal {
                    passed += 1;
                }
                continue;
            }
        };
        let out_simp = match simplified.transform(&pts, true) {
            Ok(o) => o,
            Err(e) => {
                other_failures.push(format!("{name}: simplified transform failed: {e}"));
                continue;
            }
        };

        if !outputs_equivalent(&out_simp, &out_ref, 1e-6) {
            transform_failures.push(name.to_string());
        }

        // Every fixture whose `IsSimp` card count differs from C's, collected
        // for the assertion after the loop. A negative fixture differs by
        // construction — its reference *is* its input, which was never
        // simplified — so those are excluded: they are not a disagreement
        // about placement.
        let rendered = mapping_to_native_string_with(simplified.as_ref(), render_opts);
        if rendered.is_none() {
            // Not a skip. `AnyObject::from_mapping` has no variant for this
            // class, so neither the card count nor the byte comparison below
            // can run — which is precisely the state that has to be loud.
            other_failures.push(format!(
                "{name}: simplify() produced a {}->{} Mapping that \
                 AnyObject::from_mapping cannot render -- the serialisation \
                 and IsSimp checks did not run for it",
                simplified.nin(),
                simplified.nout()
            ));
        }
        if let Some(s) = rendered {
            let count = |text: &str| {
                text.lines()
                    .filter(|l| l.trim_start().starts_with("IsSimp ="))
                    .count()
            };
            let (g, w) = (count(&s), count(&ref_text));
            if g != w && !ignore_is_simp_line {
                issimp_disagreements.push(format!("{name} ({g} vs C's {w})"));
            }
        }

        // Diagnostic only: `SIMPLIFY_DUMP=<name>` prints both sides of one
        // case, whatever its manifest flags say. Never changes the verdict.
        if std::env::var("SIMPLIFY_DUMP").is_ok_and(|v| v == name)
            && let Some(s) = mapping_to_native_string_with(simplified.as_ref(), render_opts)
        {
            eprintln!("--- got {name} ---\n{s}\n--- want ---\n{ref_text}");
        }

        // === Check 3: Serialization comparison ===
        if !skip_string_compare {
            if let Some(simp_str) = mapping_to_native_string_with(simplified.as_ref(), render_opts)
            {
                let licence = ISSIMP_PLACEMENT_LICENSED
                    .iter()
                    .find(|(n, _, _, _)| *n == name);
                let (got, want) = if ignore_is_simp_line || licence.is_some() {
                    (
                        common::strip_is_simp(&simp_str),
                        common::strip_is_simp(&ref_text),
                    )
                } else {
                    (simp_str.clone(), ref_text.clone())
                };
                // A licensed case still has to carry exactly the recorded
                // number of cards on each side, so the licence covers the
                // diagnosed placement gap and nothing else.
                if let Some((_, want_got, want_ref, _)) = licence {
                    let (n_got, n_ref) = (
                        simp_str.matches("IsSimp").count(),
                        ref_text.matches("IsSimp").count(),
                    );
                    if (n_got, n_ref) != (*want_got, *want_ref) {
                        other_failures.push(format!(
                            "{name}: IsSimp card count is ({n_got}, {n_ref}), \
                             licensed as ({want_got}, {want_ref})"
                        ));
                    }
                }
                // A negative fixture's reference is its own input, which was
                // never simplified and so carries no `IsSimp` at all — but the
                // result of `simplify` always does (mapping.c:24763 stamps
                // every result). Requiring one keeps the strip from hiding a
                // result that is untagged altogether.
                if ignore_is_simp_line && !simp_str.contains("IsSimp") {
                    other_failures.push(format!("{name}: simplify() produced no IsSimp card"));
                }
                if !common::native_strings_equivalent(&got, &want, 4) {
                    serial_failures.push(name.to_string());
                    // Diagnostic only: `SIMPLIFY_DIFF=1` prints the first few
                    // differing lines per failing case. Off by default so the
                    // failure list stays readable; it never changes the verdict.
                    if std::env::var("SIMPLIFY_DIFF").is_ok_and(|v| !v.is_empty()) {
                        let (g, w): (Vec<&str>, Vec<&str>) =
                            (got.lines().collect(), want.lines().collect());
                        eprintln!("=== {name} ===");
                        let mut shown = 0;
                        for i in 0..g.len().max(w.len()) {
                            let a = g.get(i).copied().unwrap_or("<eof>").trim();
                            let b = w.get(i).copied().unwrap_or("<eof>").trim();
                            if a != b {
                                eprintln!("  line {}: got  {a:?}", i + 1);
                                eprintln!("           want {b:?}");
                                shown += 1;
                                if shown >= 4 {
                                    eprintln!("  ...");
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        passed += 1;
    }

    // Report results
    println!(
        "\nSimplify regression: {passed} passed, {skipped_missing} skipped (file absent), \
         {} skipped (parse — unported class or gap), {} equal() failures, \
         {} transform failures, {} serialization failures, {} other failures",
        parse_skipped.len(),
        equal_failures.len(),
        transform_failures.len(),
        serial_failures.len(),
        other_failures.len(),
    );
    // The `IsSimp` card records which sub-Mappings were, at some instant, the
    // whole result of a simplification. Upstream `e40e71b4` made C's answer
    // deterministic — a property of the Mapping tree, not of which nodes some
    // probe happened to touch — so it is now a well-defined target this engine
    // can be held to, where before it could not be reproduced even in
    // principle. These are the fixtures that do not yet hit it. Four of the six
    // are masked by `skip_string_compare` for an unrelated reason and two by the
    // counted licence, so without this list the number would be invisible.

    if !parse_skipped.is_empty() {
        eprintln!(
            "  parse-skipped (fixture present but not parseable — a parser \
             or class-support gap; IntraMap/XphMap are BLOCKED in the \
             manifest and should not appear here):"
        );
        for n in &parse_skipped {
            eprintln!("    {n}");
        }
    }
    if !equal_failures.is_empty() {
        eprintln!("  equal() failures (simplified != C reference structurally):");
        for n in &equal_failures {
            eprintln!("    {n}");
        }
    }
    if !transform_failures.is_empty() {
        eprintln!("  Transform failures (simplified output != C reference output):");
        for n in &transform_failures {
            eprintln!("    {n}");
        }
    }
    if !serial_failures.is_empty() {
        eprintln!("  Serialization mismatches (native string differs from .simp):");
        for n in &serial_failures {
            eprintln!("    {n}");
        }
    }
    if !other_failures.is_empty() {
        for f in &other_failures {
            eprintln!("  FAIL: {f}");
        }
    }

    // Every check is a hard failure. `equal()` mismatches mean the simplified
    // tree is not the one C produces; serialization mismatches mean the dumped
    // form diverges from the `.simp` reference. A fixture whose serialization
    // difference has been diagnosed as a C-side artefact (see
    // `docs/issues/c-library-quirks.md`) opts out per-fixture via the manifest's
    // `skip_string_compare` flag, with the reason recorded there.
    // Byte comparisons that were never running until `AnyObject::from_mapping`
    // replaced this file's hand-rolled dispatch. The old dispatch covered 18
    // Mapping classes and no Region, FrameSet or SwitchMap, and an unhandled
    // class returned `None` from inside an `if let Some(..)` — so every fixture
    // whose simplified result was one of those classes skipped both the byte
    // comparison and the `IsSimp` count check without saying anything.
    //
    // None of these is a regression and none is a stale fixture: the pinned C
    // reproduces every `.simp` file here byte-exactly. They are pre-existing
    // defects that had no gate. Each row names the diagnosed cause, and the set
    // is asserted exactly, so fixing one forces the row out.
    //
    // Note what `equal()` did here: it passed every one of them, including a Prism
    // where C has a Box. Structural equality is far weaker than the byte
    // comparison, which is why losing the byte comparison cost this much.
    const SERIAL_UNGATED: &[(&str, &str)] = &[
        // (B) Region simplify tags a Mapping inside the encapsulated FrameSet
        // that C leaves bare. See PLAN.md §9.1.
        (
            "cefit_circle_conformal",
            "IsSimp on the FrameSet's ZoomMap; C emits none",
        ),
        (
            "cefit_circle_nonconformal_keep",
            "IsSimp on the FrameSet's MathMap; C emits none",
        ),
        // (C) The uncertainty Region does not collapse Prism -> Box.
        (
            "nullregion_unc_fillfactor_merge",
            "emits Prism where C emits Box",
        ),
        // (D) Region-simplify shape divergence, not a card: we keep a FrmSet
        // where C has collapsed to bare Points.
        (
            "cap_zoommap_04",
            "emits FrmSet/FrameSet where C emits Points/PointSet",
        ),
        // (E) Serialisation default: C omits Closed when it holds the default.
        ("cap_interval_01", "emits Closed = 1 where C omits it"),
        // (F) Numeric: X5 to ~3e-15 relative, X6 to ~1.2e-10 -- both past the
        // 4-ulp token tolerance, so real arithmetic divergence.
        (
            "cap_pointlist_01",
            "two mesh coordinates diverge past 4 ulp",
        ),
    ];
    // `simplify` must stamp whatever it returns (mapping.c:24763). It does not
    // for a Region, which is the same defect as (B) seen from the other side.
    const NO_ISSIMP_CARD_UNGATED: &[&str] = &["neg_box_asymmetric_2d"];

    let ungated: Vec<&str> = SERIAL_UNGATED.iter().map(|(n, _)| *n).collect();
    let (licensed, unlicensed): (Vec<String>, Vec<String>) = serial_failures
        .iter()
        .cloned()
        .partition(|n| ungated.contains(&n.as_str()));
    let mut licensed_sorted = licensed.clone();
    licensed_sorted.sort();
    let mut expected_licensed: Vec<String> = ungated.iter().map(|s| s.to_string()).collect();
    expected_licensed.sort();
    assert_eq!(
        licensed_sorted, expected_licensed,
        "the set of ungated serialisation failures has changed. Fewer is \
         progress — delete the row. More means a real regression, because \
         every row here was verified against the pinned C oracle."
    );
    let (no_card_licensed, no_card_other): (Vec<String>, Vec<String>) =
        other_failures.iter().cloned().partition(|m| {
            NO_ISSIMP_CARD_UNGATED
                .iter()
                .any(|n| m.starts_with(&format!("{n}:")))
        });
    assert_eq!(
        no_card_licensed.len(),
        NO_ISSIMP_CARD_UNGATED.len(),
        "the untagged-result licence no longer matches: {no_card_licensed:?}"
    );
    let serial_failures = unlicensed;
    let other_failures = no_card_other;

    // Every row here is a real gap: the pinned C oracle and the committed
    // `.simp` agree, and the port's `IsSimp` count differs from both. The
    // corpus was regenerated against the pin at f6762005, so a stale fixture
    // is no longer a possible explanation for a row appearing in this list.
    // See PLAN.md §6.
    const ISSIMP_DISAGREEMENTS: &[&str] = &[
        // Under-tagging, gated all along.
        "rigby (8 vs C's 9)",
        // Under-tagging: C tags the CmpMap wrapping the SpecMap/ZoomMap pair,
        // which the port never forms. PLAN.md §10.3.
        "cap_specmap_02 (7 vs C's 8)",
        // Newly visible, not new. Both totals agreed with C at 18 by
        // coincidence: the port tagged leaf classes C never tags in these trees
        // — a TranMap and a MathMap in `_01`, a TranMap and a second UnitMap in
        // `_04` — while already missing CmpMap tags C sets. Dropping the
        // per-member simplify of a dimension-matched sublist removed the leaf
        // tags, which is C's shape, and left the CmpMap deficit showing. Both
        // are `skip_string_compare`, so the count was the only gate and the
        // coincidence passed it. PLAN.md §10.3.
        "cap_cmpmap_01 (16 vs C's 18)",
        "cap_cmpmap_04 (15 vs C's 18)",
        // These render to a Region, so the old hand-rolled dispatch returned
        // `None` and the count was never taken. Causes (B) and (D) in
        // SERIAL_UNGATED. Cause (A), the four SwitchMap fixtures, is closed:
        // C reaches each encapsulated Mapping through `astSimplify`, which
        // records what it returns, and `SwitchMap::simplified` now stamps them.
        "cefit_circle_conformal (1 vs C's 0)",
        "cefit_circle_nonconformal_keep (1 vs C's 0)",
        "cap_zoommap_04 (3 vs C's 0)",
    ];
    issimp_disagreements.sort();
    let mut expected: Vec<String> = ISSIMP_DISAGREEMENTS.iter().map(|s| s.to_string()).collect();
    expected.sort();
    assert_eq!(
        issimp_disagreements, expected,
        "the set of fixtures whose IsSimp count differs from C has changed. \
         Fewer is progress — delete the row. More is a regression. Either way \
         update this list and PLAN.md §6 together."
    );

    let total = equal_failures.len()
        + transform_failures.len()
        + serial_failures.len()
        + other_failures.len();
    if total > 0 {
        panic!("{total} simplify regression tests failed");
    }

    // Require at least one test actually ran
    assert!(
        passed > 0,
        "No simplify regression tests passed (all skipped?)"
    );
}

#[test]
fn cmpmap_re_expands_after_merge() {
    use ast_core::mappings::{CmpMap, Composition, Mapping, ShiftMap, WinMap, ZoomMap};

    // Create a chain: ShiftMap ; ZoomMap ; ShiftMap
    // Merging Shift+Zoom produces a WinMap, which should then merge with the final Shift.
    let a = ShiftMap::new([1.0]);
    let b = ZoomMap::new(1, 2.0).unwrap();
    let c = ShiftMap::new([3.0]);

    let ab = CmpMap::new(Box::new(a), Box::new(b), Composition::Series).unwrap();
    let abc = CmpMap::new(Box::new(ab), Box::new(c), Composition::Series).unwrap();

    let s = <dyn Mapping>::simplify(&abc).unwrap();
    // Should reduce to a single WinMap: shift = 2*1 + 3 = 5, scale = 2.
    assert!(s.as_any().is::<WinMap>(), "Expected WinMap but got {:?}", s);
}

#[test]
fn cmpmap_re_expands_merged_cmpmap() {
    use ast_core::mappings::{CmpMap, Composition, Mapping, ShiftMap, UnitMap, ZoomMap};

    // Scenario where a merge produces a CmpMap that should be re-expanded.
    // ZoomMap(2) ; [ShiftMap(1) parallel UnitMap(1)] ; ZoomMap(3)
    // The parallel CmpMap in the middle has 2 axes.
    // ZoomMap(2) can't merge with the parallel CmpMap directly,
    // but after simplification the middle should become a WinMap,
    // which can merge with ZoomMaps on both sides.
    let z1 = ZoomMap::new(2, 2.0).unwrap();
    let inner = CmpMap::new(
        Box::new(ShiftMap::new([1.0])),
        Box::new(UnitMap::new(1)),
        Composition::Parallel,
    )
    .unwrap();
    let z2 = ZoomMap::new(2, 3.0).unwrap();

    let chain1 = CmpMap::new(Box::new(z1), Box::new(inner), Composition::Series).unwrap();
    let chain = CmpMap::new(Box::new(chain1), Box::new(z2), Composition::Series).unwrap();

    let s = <dyn Mapping>::simplify(&chain).unwrap();
    // The result should be simpler than the original 4-mapping chain.
    // At minimum it should not be a 4-element series CmpMap.
    assert_eq!(s.nin(), 2);
    assert_eq!(s.nout(), 2);
}

#[test]
fn has_been_simplified_does_not_prevent_merge() {
    use ast_core::mappings::{CmpMap, Composition, Mapping, ZoomMap};

    let mut z1 = ZoomMap::new(2, 3.0).unwrap();
    z1.set_has_been_simplified(true);
    let z2 = ZoomMap::new(2, 2.0).unwrap();

    // The IsSimple flag (has_been_simplified) does NOT block merging in the
    // nominate loop — C `MapMerge` never consults it (only the oscillation
    // `Frozen` barrier stops merges). Oracle-verified: this simplifies to a
    // single ZoomMap of factor 6.
    let cmp = CmpMap::new(Box::new(z1), Box::new(z2), Composition::Series).unwrap();
    let s = <dyn Mapping>::simplify(&cmp).unwrap();
    assert!(
        s.as_any().is::<ZoomMap>(),
        "IsSimple must not block the ZoomMap merge (oracle: single ZoomMap)"
    );
    // Effective zoom factor is 6 (3 * 2): check via transform on the public API.
    let pts = PointSet::from_rows([[1.0, 1.0]]);
    let out = s.transform(&pts, true).unwrap();
    assert!(
        (out.point(0)[0] - 6.0).abs() < 1e-12,
        "expected zoom 6, got {}",
        out.point(0)[0]
    );
}

#[test]
fn has_been_simplified_default_is_false() {
    use ast_core::mappings::{Mapping, ZoomMap};
    let z = ZoomMap::new(2, 3.0).unwrap();
    assert!(!z.has_been_simplified());
}

#[test]
fn has_been_simplified_preserves_identity_zoom() {
    use ast_core::mappings::{Mapping, ZoomMap};

    // A ZoomMap with factor 1.0 normally simplifies to UnitMap.
    let mut z = ZoomMap::new(2, 1.0).unwrap();
    z.set_has_been_simplified(true);
    let s = <dyn Mapping>::simplify(&z).unwrap();
    // With the flag set, it should stay as ZoomMap.
    assert!(s.as_any().is::<ZoomMap>());
}

#[test]
fn has_been_simplified_second_mapping_does_not_prevent_merge() {
    use ast_core::mappings::{CmpMap, Composition, Mapping, ZoomMap};

    let z1 = ZoomMap::new(2, 3.0).unwrap();
    let mut z2 = ZoomMap::new(2, 2.0).unwrap();
    z2.set_has_been_simplified(true);

    // An IsSimple flag on the second mapping also does not block merging.
    // Oracle-verified: single ZoomMap.
    let cmp = CmpMap::new(Box::new(z1), Box::new(z2), Composition::Series).unwrap();
    let s = <dyn Mapping>::simplify(&cmp).unwrap();
    assert!(s.as_any().is::<ZoomMap>());
}

/// Verify the recursion guard terminates for deeply nested CmpMaps.
///
/// A chain of 20 ZoomMaps(factor=1.0) composed in series with a single
/// ZoomMap(factor=1.5) should simplify to a single ZoomMap without
/// hitting the MAX_MERGE_PASSES limit wastefully or recursing forever.
#[test]
fn simplify_recursion_guard_terminates() {
    use ast_core::mappings::{CmpMap, Composition, Mapping, ZoomMap};

    // Build a deeply nested CmpMap: ZoomMap(1.5) ; ZoomMap(1.0) ; ... ; ZoomMap(1.0)
    let mut m: Box<dyn Mapping> = Box::new(ZoomMap::new(2, 1.5).unwrap());
    for _ in 0..20 {
        let z = ZoomMap::new(2, 1.0).unwrap();
        m = Box::new(CmpMap::new(m, Box::new(z), Composition::Series).unwrap());
    }
    // Should simplify without infinite recursion or excessive time.
    let s = m.simplify().unwrap();
    assert_eq!(s.nin(), 2);
    assert_eq!(s.nout(), 2);
}

/// Verify that a mapping with identical sub-structures does not
/// get stuck in a cycle where simplification reproduces the input.
#[test]
fn simplify_recursion_guard_identical_subtrees() {
    use ast_core::mappings::{CmpMap, Composition, Mapping, ShiftMap, ZoomMap};

    // (Shift ; Zoom) || (Shift ; Zoom)  — two identical parallel arms
    // then (result) ; (result)  — identical halves in series.
    // Each simplification pass produces the same structure, triggering
    // the fingerprint-based cycle detection.
    let arm = || -> Box<dyn Mapping> {
        let a = ShiftMap::new([1.0, 2.0]);
        let b = ZoomMap::new(2, 3.0).unwrap();
        Box::new(CmpMap::new(Box::new(a), Box::new(b), Composition::Series).unwrap())
    };

    let par = CmpMap::new(arm(), arm(), Composition::Parallel).unwrap();
    let doubled = CmpMap::new(Box::new(par.clone()), Box::new(par), Composition::Series).unwrap();

    // Must terminate (no infinite recursion).
    let s = <dyn Mapping>::simplify(&doubled).unwrap();
    // The result's dimensions must be preserved.
    assert_eq!(s.nin(), 4);
    assert_eq!(s.nout(), 4);
}

/// Verify that clone_box produces a functionally equivalent deep copy.
#[test]
fn clone_box_preserves_transform() {
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, ShiftMap, ZoomMap};

    let a = ShiftMap::new([1.0, 2.0]);
    let b = ZoomMap::new(2, 3.0).unwrap();
    let cmp = CmpMap::new(Box::new(a), Box::new(b), Composition::Series).unwrap();

    let cloned = cmp.clone_box();

    let pts = PointSet::from_rows(vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
    let out_orig = cmp.transform(&pts, true).unwrap();
    let out_clone = cloned.transform(&pts, true).unwrap();

    for i in 0..pts.npoint() {
        for j in 0..2 {
            assert_eq!(out_orig.point(i)[j], out_clone.point(i)[j]);
        }
    }
}

/// Verify that the (inv-SphMap, diagonal-scale, fwd-SphMap) pattern
/// simplifies to a WinMap encoding longitude/latitude reflections.
///
/// Matches C sphmap.c:798-950.
#[test]
fn sphmap_three_mapping_pattern_simplifies() {
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, SphMap, ZoomMap};

    // Build: inv-SphMap (sph→cart, 2→3) ; ZoomMap(-1, 3D) ; fwd-SphMap (cart→sph, 3→2)
    //
    // SphMap forward direction: cart→sph (3→2)
    // SphMap inverse direction: sph→cart (2→3)
    //
    // ZoomMap(-1) negates all Cartesian axes = point reflection through origin.
    // In spherical: lon → lon + π, lat → -lat.
    let mut inv_sph = SphMap::new(); // forward = cart→sph (3→2)
    inv_sph.invert(); // now forward = sph→cart (2→3)
    let zoom = ZoomMap::new(3, -1.0).unwrap();
    let fwd_sph = SphMap::new(); // forward = cart→sph (3→2)

    let step1 = CmpMap::new(Box::new(inv_sph), Box::new(zoom), Composition::Series).unwrap();
    let full = CmpMap::new(Box::new(step1), Box::new(fwd_sph), Composition::Series).unwrap();

    // Get the unsimplified transform output for comparison.
    let lon = 0.5_f64;
    let lat = 0.3_f64;
    let pts = PointSet::from_rows([[lon, lat]]);
    let out_orig = full.transform(&pts, true).unwrap();

    let s = <dyn Mapping>::simplify(&full).unwrap();
    // Should simplify — the trio should be replaced by something simpler.
    assert_eq!(s.nin(), 2);
    assert_eq!(s.nout(), 2);

    // Verify the simplified transform matches the original.
    let out = s.transform(&pts, true).unwrap();
    assert!(
        (out.point(0)[0] - out_orig.point(0)[0]).abs() < 1e-10,
        "lon: simplified {} vs original {}",
        out.point(0)[0],
        out_orig.point(0)[0]
    );
    assert!(
        (out.point(0)[1] - out_orig.point(0)[1]).abs() < 1e-10,
        "lat: simplified {} vs original {}",
        out.point(0)[1],
        out_orig.point(0)[1]
    );

    // Verify expected geometric effect: lat should be negated.
    assert!(
        (out_orig.point(0)[1] - (-lat)).abs() < 1e-10,
        "lat: got {}, expected {}",
        out_orig.point(0)[1],
        -lat
    );
}

/// Verify the pattern with a diagonal MatrixMap (non-uniform signs).
#[test]
fn sphmap_three_mapping_matrixmap_pattern() {
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, MatrixMap, SphMap};
    use ndarray::Array2;

    // Diagonal [-1, 1, 1]: negate X only.
    // Expected spherical effect: lon → π - lon (reflection), lat unchanged.
    // C logic: diag[0]<0, diag[1]>0 → shift=π, scale=-1, diag[2]>0 → lat_scale=1
    let mut inv_sph = SphMap::new();
    inv_sph.invert();
    let mut m = Array2::zeros((3, 3));
    m[[0, 0]] = -1.0;
    m[[1, 1]] = 1.0;
    m[[2, 2]] = 1.0;
    let mm = MatrixMap::new(m).unwrap();
    let fwd_sph = SphMap::new();

    let step1 = CmpMap::new(Box::new(inv_sph), Box::new(mm), Composition::Series).unwrap();
    let full = CmpMap::new(Box::new(step1), Box::new(fwd_sph), Composition::Series).unwrap();

    let s = <dyn Mapping>::simplify(&full).unwrap();
    assert_eq!(s.nin(), 2);
    assert_eq!(s.nout(), 2);

    // Verify: (lon, lat) → (π - lon, lat)
    let lon = 0.5_f64;
    let lat = 0.3_f64;
    let pts = PointSet::from_rows([[lon, lat]]);
    let out = s.transform(&pts, true).unwrap();
    let expected_lon = std::f64::consts::PI - lon;
    let expected_lat = lat;
    assert!(
        (out.point(0)[0] - expected_lon).abs() < 1e-10,
        "lon: got {}, expected {}",
        out.point(0)[0],
        expected_lon
    );
    assert!(
        (out.point(0)[1] - expected_lat).abs() < 1e-10,
        "lat: got {}, expected {}",
        out.point(0)[1],
        expected_lat
    );
}

/// Verify that a WinMap (pure scaling, zero shifts) before a non-diagonal
/// MatrixMap is absorbed into the MatrixMap by scaling its columns.
#[test]
fn winmap_pure_scale_before_matrix_simplifies() {
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, MatrixMap, WinMap};
    use ndarray::array;

    // WinMap(shift=[0,0], scale=[2,3]) ; MatrixMap[[1,1],[1,-1]]
    // The WinMap scales should be absorbed into the MatrixMap columns.
    // Result: MatrixMap[[2,3],[2,-3]] (columns scaled by [2,3]).
    let wm = WinMap::new(&[0.0, 0.0], &[2.0, 3.0]).unwrap();
    let m = array![[1.0, 1.0], [1.0, -1.0]];
    let mm = MatrixMap::new(m).unwrap();

    let c = CmpMap::new(
        Box::new(wm.clone()),
        Box::new(mm.clone()),
        Composition::Series,
    )
    .unwrap();
    let s = <dyn Mapping>::simplify(&c).unwrap();

    // Should reduce to a single MatrixMap (WinMap absorbed).
    assert!(
        s.as_any().is::<MatrixMap>(),
        "Expected MatrixMap, got {:?}",
        s
    );

    // Verify transform equivalence.
    let pts = PointSet::from_rows([[5.0, 7.0], [1.0, -2.0]]);
    let wm_out = wm.transform(&pts, true).unwrap();
    let expected = mm.transform(&wm_out, true).unwrap();
    let actual = s.transform(&pts, true).unwrap();
    for i in 0..pts.npoint() {
        for j in 0..2 {
            assert!(
                (actual.point(i)[j] - expected.point(i)[j]).abs() < 1e-10,
                "Point {i} coord {j}: got {}, expected {}",
                actual.point(i)[j],
                expected.point(i)[j]
            );
        }
    }
}

/// Verify that a WinMap (with shifts) before a non-diagonal MatrixMap
/// produces a CmpMap(MatrixMap, ShiftMap) that is numerically equivalent.
#[test]
fn winmap_with_shifts_before_matrix_simplifies() {
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, MatrixMap, WinMap};
    use ndarray::array;

    // WinMap(shift=[1,2], scale=[2,3]) ; MatrixMap[[1,1],[1,-1]]
    // Result: MatrixMap[[2,3],[2,-3]] then ShiftMap([1*1+1*2, 1*1+(-1)*2]) = ShiftMap([3,-1])
    let wm = WinMap::new(&[1.0, 2.0], &[2.0, 3.0]).unwrap();
    let m = array![[1.0, 1.0], [1.0, -1.0]];
    let mm = MatrixMap::new(m).unwrap();

    let c = CmpMap::new(
        Box::new(wm.clone()),
        Box::new(mm.clone()),
        Composition::Series,
    )
    .unwrap();
    let s = <dyn Mapping>::simplify(&c).unwrap();

    // The result should NOT be a single WinMap or MatrixMap (it needs a shift).
    // It should be either a CmpMap(MatrixMap, ShiftMap) or possibly further
    // simplified, but must be numerically equivalent.
    let pts = PointSet::from_rows([[5.0, 7.0], [0.0, 0.0], [-1.0, 3.0]]);
    let wm_out = wm.transform(&pts, true).unwrap();
    let expected = mm.transform(&wm_out, true).unwrap();
    let actual = s.transform(&pts, true).unwrap();
    for i in 0..pts.npoint() {
        for j in 0..2 {
            assert!(
                (actual.point(i)[j] - expected.point(i)[j]).abs() < 1e-10,
                "Point {i} coord {j}: got {}, expected {}",
                actual.point(i)[j],
                expected.point(i)[j]
            );
        }
    }
}

/// Verify that WinMap before MatrixMap enables further chain simplification.
/// ShiftMap ; MatrixMap ; ShiftMap where the first ShiftMap can swap past
/// the matrix to merge with the second ShiftMap.
#[test]
fn winmap_swap_enables_chain_simplification() {
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, MatrixMap, ShiftMap};
    use ndarray::array;

    // ShiftMap([1,0]) ; MatrixMap[[0,1],[1,0]] ; ShiftMap([0,2])
    // The permutation matrix swaps axes.
    // ShiftMap([1,0]) is a WinMap(shifts=[1,0], scales=[1,1]).
    // After swap: MatrixMap[[0,1],[1,0]] ; ShiftMap([0,1]) ; ShiftMap([0,2])
    // = MatrixMap[[0,1],[1,0]] ; ShiftMap([0,3])
    let s1 = ShiftMap::new([1.0, 0.0]);
    let m = array![[0.0, 1.0], [1.0, 0.0]]; // axis swap
    let mm = MatrixMap::new(m).unwrap();
    let s2 = ShiftMap::new([0.0, 2.0]);

    let ab = CmpMap::new(
        Box::new(s1.clone()),
        Box::new(mm.clone()),
        Composition::Series,
    )
    .unwrap();
    let abc = CmpMap::new(Box::new(ab), Box::new(s2.clone()), Composition::Series).unwrap();

    // Compute reference output manually.
    let pts = PointSet::from_rows([[3.0, 5.0], [0.0, -1.0]]);
    let step1 = s1.transform(&pts, true).unwrap();
    let step2 = mm.transform(&step1, true).unwrap();
    let expected = s2.transform(&step2, true).unwrap();

    let s = <dyn Mapping>::simplify(&abc).unwrap();
    let actual = s.transform(&pts, true).unwrap();
    for i in 0..pts.npoint() {
        for j in 0..2 {
            assert!(
                (actual.point(i)[j] - expected.point(i)[j]).abs() < 1e-10,
                "Point {i} coord {j}: got {}, expected {}",
                actual.point(i)[j],
                expected.point(i)[j]
            );
        }
    }
}

/// Verify that a WinMap (ShiftMap) swaps past a PermMap to reach a merge partner.
///
/// ShiftMap[1,2] ; PermMap[2,1] ; ShiftMap[3,4]
/// The first ShiftMap should swap past the PermMap (becoming ShiftMap[2,1]),
/// then merge with the second ShiftMap to give ShiftMap[5,5].
#[test]
fn winmap_swaps_past_permmap() {
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, PermMap, ShiftMap};

    let s1 = ShiftMap::new([1.0, 2.0]);
    let pm = PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let s2 = ShiftMap::new([3.0, 4.0]);

    let c1 = CmpMap::new(Box::new(s1), Box::new(pm), Composition::Series).unwrap();
    let full = CmpMap::new(Box::new(c1), Box::new(s2), Composition::Series).unwrap();

    let s = <dyn Mapping>::simplify(&full).unwrap();

    // Verify transform: input [5,7] -> shift [6,9] -> permute [9,6] -> shift [12,10]
    let pts = PointSet::from_rows([[5.0, 7.0]]);
    let out = s.transform(&pts, true).unwrap();
    assert!(
        (out.point(0)[0] - 12.0).abs() < 1e-10,
        "axis 0: got {}, expected 12.0",
        out.point(0)[0]
    );
    assert!(
        (out.point(0)[1] - 10.0).abs() < 1e-10,
        "axis 1: got {}, expected 10.0",
        out.point(0)[1]
    );

    // After swap+merge, the result should be simpler than the original 3-map chain.
    // The ShiftMap should have swapped past the PermMap and merged with the second
    // ShiftMap, yielding a 2-element chain (PermMap ; ShiftMap/WinMap) rather than 3.
    // At minimum it must not be a plain ShiftMap or WinMap (still needs PermMap).
    assert!(s.as_any().is::<CmpMap>(), "Expected a CmpMap, got {:?}", s);
}

/// Verify that a WinMap distributes into a parallel CmpMap's branches.
///
/// WinMap(4 axes) ; CmpMap(ZoomMap(2) || ZoomMap(2)) should distribute
/// the WinMap into sub-WinMaps per branch, enabling further merges.
/// Matches C winmap.c:1268-1416.
#[test]
fn winmap_distributes_into_parallel_cmpmap() {
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, WinMap, ZoomMap};

    // WinMap(4 axes, shift=[1,2,3,4] scale=[1,1,1,1]) ;
    // CmpMap(ZoomMap(2, factor=2) || ZoomMap(2, factor=3))
    // Should distribute: the WinMap splits into [shift=1,2] and [shift=3,4],
    // each getting pushed into the parallel branches.
    let wm = WinMap::new(&[1.0, 2.0, 3.0, 4.0], &[1.0, 1.0, 1.0, 1.0]).unwrap();
    let z1 = ZoomMap::new(2, 2.0).unwrap();
    let z2 = ZoomMap::new(2, 3.0).unwrap();
    let par = CmpMap::new(Box::new(z1), Box::new(z2), Composition::Parallel).unwrap();

    let full = CmpMap::new(Box::new(wm), Box::new(par), Composition::Series).unwrap();
    let s = <dyn Mapping>::simplify(&full).unwrap();

    // Verify transform: [5,6,7,8] → shift [6,8,10,12] → zoom [12,16,30,36]
    let pts = PointSet::from_rows([[5.0, 6.0, 7.0, 8.0]]);
    let out = s.transform(&pts, true).unwrap();
    assert!(
        (out.point(0)[0] - 12.0).abs() < 1e-10,
        "axis 0: got {}, expected 12.0",
        out.point(0)[0]
    );
    assert!(
        (out.point(0)[1] - 16.0).abs() < 1e-10,
        "axis 1: got {}, expected 16.0",
        out.point(0)[1]
    );
    assert!(
        (out.point(0)[2] - 30.0).abs() < 1e-10,
        "axis 2: got {}, expected 30.0",
        out.point(0)[2]
    );
    assert!(
        (out.point(0)[3] - 36.0).abs() < 1e-10,
        "axis 3: got {}, expected 36.0",
        out.point(0)[3]
    );
}

/// Verify that oscillation detection in merge_loop does not interfere with
/// normal operation. A deeply nested chain of alternating ShiftMap and
/// ZoomMap mappings should simplify correctly and terminate without
/// hitting MAX_MERGE_PASSES.
#[test]
fn merge_loop_terminates_with_oscillation_guard() {
    use ast_core::mappings::{CmpMap, Composition, Mapping, ShiftMap, ZoomMap};

    // Build a deeply nested CmpMap: ZoomMap(1.5) ; ShiftMap ; ZoomMap(1.0) ; ShiftMap ; ...
    let mut m: Box<dyn Mapping> = Box::new(ZoomMap::new(2, 1.5).unwrap());
    for i in 0..30 {
        let next: Box<dyn Mapping> = if i % 2 == 0 {
            Box::new(ShiftMap::new([1.0, 1.0]))
        } else {
            Box::new(ZoomMap::new(2, 1.0).unwrap())
        };
        m = Box::new(CmpMap::new(m, next, Composition::Series).unwrap());
    }
    // Should terminate in reasonable time without hitting MAX_MERGE_PASSES.
    let s = m.simplify().unwrap();
    assert_eq!(s.nin(), 2);
    assert_eq!(s.nout(), 2);
}

/// Verify that the pattern does NOT fire when magnitudes differ (non-uniform scaling).
#[test]
fn sphmap_three_mapping_unequal_magnitudes_no_simplify() {
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, MatrixMap, SphMap};
    use ndarray::Array2;

    // Diagonal [2, 1, 1]: magnitudes differ, pattern should not fire.
    let mut inv_sph = SphMap::new();
    inv_sph.invert();
    let mut m = Array2::zeros((3, 3));
    m[[0, 0]] = 2.0;
    m[[1, 1]] = 1.0;
    m[[2, 2]] = 1.0;
    let mm = MatrixMap::new(m).unwrap();
    let fwd_sph = SphMap::new();

    let step1 = CmpMap::new(Box::new(inv_sph), Box::new(mm), Composition::Series).unwrap();
    let full = CmpMap::new(Box::new(step1), Box::new(fwd_sph), Composition::Series).unwrap();

    // Even without simplification the transform should still be valid.
    let lon = 0.5_f64;
    let lat = 0.3_f64;
    let pts = PointSet::from_rows([[lon, lat]]);
    let out_before = full.transform(&pts, true).unwrap();
    let s = <dyn Mapping>::simplify(&full).unwrap();
    let out_after = s.transform(&pts, true).unwrap();

    // The outputs should match — simplification must preserve semantics
    // even if it doesn't reduce the mapping.
    assert!(
        (out_before.point(0)[0] - out_after.point(0)[0]).abs() < 1e-10,
        "transform changed after simplification"
    );
    assert!(
        (out_before.point(0)[1] - out_after.point(0)[1]).abs() < 1e-10,
        "transform changed after simplification"
    );
}

#[test]
fn series_in_parallel_different_lengths() {
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, ShiftMap, ZoomMap};

    // Build two parallel branches with different series lengths:
    // branch1 = ZoomMap(2, 3.0) ; ShiftMap([1.0, 1.0])  (2 components, 2D)
    // branch2 = ZoomMap(1, 2.0)                          (1 component, 1D)
    //
    // The parallel composition (branch1 || branch2) is 3D → 3D.
    // Without the relaxation, the restructuring (A;B)||(C) → (A||C);(B||unit)
    // is rejected because the series lists have different lengths.
    let branch1 = CmpMap::new(
        Box::new(ZoomMap::new(2, 3.0).unwrap()),
        Box::new(ShiftMap::new([1.0, 1.0])),
        Composition::Series,
    )
    .unwrap();
    let branch2 = ZoomMap::new(1, 2.0).unwrap();
    let par = CmpMap::new(Box::new(branch1), Box::new(branch2), Composition::Parallel).unwrap();

    // Transform before simplification.
    let pts = PointSet::from_rows([[1.0, 2.0, 3.0]]);
    let out_before = par.transform(&pts, true).unwrap();

    let s = <dyn Mapping>::simplify(&par).unwrap();

    // Dimensions must be preserved.
    assert_eq!(s.nin(), 3);
    assert_eq!(s.nout(), 3);

    // Transform after simplification must give the same result.
    let out_after = s.transform(&pts, true).unwrap();
    for j in 0..3 {
        assert!(
            (out_before.point(0)[j] - out_after.point(0)[j]).abs() < 1e-10,
            "axis {j}: transform changed after simplification: {} vs {}",
            out_before.point(0)[j],
            out_after.point(0)[j],
        );
    }
}

/// Verify that two MatrixMaps separated by a WinMap (pure scale) can find
/// each other via the existing swap infrastructure: the WinMap is absorbed
/// into the second MatrixMap (by `try_swap_winmap_past_matrixmap`), leaving
/// the two MatrixMaps adjacent so they multiply in a subsequent merge pass.
///
/// This is the "swap-and-seek" strategy from C matrixmap.c:2063-2273.
#[test]
fn matrixmap_seek_through_winmap() {
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, MatrixMap, WinMap};
    use ndarray::array;

    // MatrixMap1 ; WinMap(pure scale) ; MatrixMap2 (non-diagonal)
    // The WinMap at position 1 is before MatrixMap2 at position 2 (non-diagonal).
    // The swap absorbs the WinMap's scales into MatrixMap2's columns, leaving
    // [MatrixMap1, MatrixMap2'] adjacent. They then multiply into a single matrix.
    let m1 = array![[1.0, 2.0], [3.0, 4.0]];
    let mm1 = MatrixMap::new(m1).unwrap();
    let wm = WinMap::new(&[0.0, 0.0], &[2.0, 3.0]).unwrap(); // pure scale
    let m2 = array![[1.0, 1.0], [0.0, 1.0]]; // upper triangular (non-diagonal)
    let mm2 = MatrixMap::new(m2).unwrap();

    let c1 = CmpMap::new(
        Box::new(mm1.clone()),
        Box::new(wm.clone()),
        Composition::Series,
    )
    .unwrap();
    let full = CmpMap::new(Box::new(c1), Box::new(mm2.clone()), Composition::Series).unwrap();

    let s = <dyn Mapping>::simplify(&full).unwrap();

    // Verify transform equivalence.
    let pts = PointSet::from_rows([[5.0, 7.0]]);
    let step1 = mm1.transform(&pts, true).unwrap();
    let step2 = wm.transform(&step1, true).unwrap();
    let expected = mm2.transform(&step2, true).unwrap();
    let actual = s.transform(&pts, true).unwrap();
    assert!(
        (actual.point(0)[0] - expected.point(0)[0]).abs() < 1e-10,
        "axis 0: got {}, expected {}",
        actual.point(0)[0],
        expected.point(0)[0]
    );
    assert!(
        (actual.point(0)[1] - expected.point(0)[1]).abs() < 1e-10,
        "axis 1: got {}, expected {}",
        actual.point(0)[1],
        expected.point(0)[1]
    );

    // Should simplify to a single MatrixMap (WinMap absorbed, then matrices multiply).
    assert!(
        s.as_any().is::<MatrixMap>(),
        "Expected MatrixMap, got {:?}",
        s
    );
}

/// Verify that two MatrixMaps separated by a ShiftMap can find each other.
/// The ShiftMap is absorbed into the second MatrixMap (producing a MatrixMap +
/// ShiftMap pair), then the two MatrixMaps become adjacent and multiply.
#[test]
fn matrixmap_seek_through_shiftmap() {
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, MatrixMap, ShiftMap};
    use ndarray::array;

    // MatrixMap1 (non-diagonal) ; ShiftMap ; MatrixMap2 (non-diagonal)
    // The ShiftMap can be swapped past MatrixMap2 (absorbed via WinMap-MatrixMap logic)
    // because there is a MatrixMap at position i-1 that serves as a merge partner.
    let m1 = array![[1.0, 2.0], [0.0, 1.0]]; // upper triangular (non-diagonal)
    let mm1 = MatrixMap::new(m1).unwrap();
    let sm = ShiftMap::new([1.0, 2.0]);
    let m2 = array![[1.0, 0.0], [1.0, 1.0]]; // lower triangular (non-diagonal)
    let mm2 = MatrixMap::new(m2).unwrap();

    let c1 = CmpMap::new(
        Box::new(mm1.clone()),
        Box::new(sm.clone()),
        Composition::Series,
    )
    .unwrap();
    let full = CmpMap::new(Box::new(c1), Box::new(mm2.clone()), Composition::Series).unwrap();

    let s = <dyn Mapping>::simplify(&full).unwrap();

    // Verify transform equivalence.
    let pts = PointSet::from_rows([[5.0, 7.0]]);
    let step1 = mm1.transform(&pts, true).unwrap();
    let step2 = sm.transform(&step1, true).unwrap();
    let expected = mm2.transform(&step2, true).unwrap();
    let actual = s.transform(&pts, true).unwrap();
    assert!(
        (actual.point(0)[0] - expected.point(0)[0]).abs() < 1e-10,
        "axis 0: got {}, expected {}",
        actual.point(0)[0],
        expected.point(0)[0]
    );
    assert!(
        (actual.point(0)[1] - expected.point(0)[1]).abs() < 1e-10,
        "axis 1: got {}, expected {}",
        actual.point(0)[1],
        expected.point(0)[1]
    );
}

/// When swapping a PermMap with constant outputs past a parallel CmpMap,
/// those constants must be transformed through the appropriate CmpMap
/// component. Previously the code used BAD_VALUE as a placeholder.
/// Matches C cmpmap.c:1929-1949.
#[test]
fn permmap_swap_transforms_constants() {
    use ast_core::constants::BAD_VALUE;
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, PermMap, ZoomMap};

    // Build a PermMap with block-swap structure and constants on the B side.
    // 3 inputs, 3 outputs.
    // outperm = [3, -1, -2]: out[0]=in[2], out[1]=const[0]=5.0, out[2]=const[1]=7.0
    // inperm = [-1, -2, 1]: in[0]=const[0] (inverse), in[1]=const[1] (inverse), in[2]→out[0]
    //
    // This is a block-swap where:
    //  - Block A (first nin_a=1 outputs): out[0] comes from input[2] (the last 1 input)
    //  - Block B (last nin_b=2 outputs): out[1..3] are constants (bconstants=true)
    //
    // Following: ZoomMap(1, 2.0) || ZoomMap(2, 3.0) (parallel, 1+2=3 inputs)
    // Constants [5.0, 7.0] feed into the 2-input ZoomMap(2,3.0) component.
    // After transformation, the constants should become [15.0, 21.0].
    let pm = PermMap::new(&[-1, -2, 1], &[3, -1, -2], &[5.0, 7.0]).unwrap();
    let z1 = ZoomMap::new(1, 2.0).unwrap();
    let z2 = ZoomMap::new(2, 3.0).unwrap();
    let par = CmpMap::new(Box::new(z1), Box::new(z2), Composition::Parallel).unwrap();

    let chain = CmpMap::new(Box::new(pm), Box::new(par), Composition::Series).unwrap();

    // Verify the original transform: input [10.0, 20.0, 30.0]
    // PermMap forward: out[0]=in[2]=30.0, out[1]=5.0, out[2]=7.0
    // Parallel CmpMap: z1(30.0)=60.0, z2(5.0,7.0)=(15.0, 21.0)
    // Expected output: [60.0, 15.0, 21.0]
    let pts = PointSet::from_rows([[10.0, 20.0, 30.0]]);
    let expected = chain.transform(&pts, true).unwrap();

    assert!((expected.point(0)[0] - 60.0).abs() < 1e-10);
    assert!((expected.point(0)[1] - 15.0).abs() < 1e-10);
    assert!((expected.point(0)[2] - 21.0).abs() < 1e-10);

    // After simplification, the result should still produce the same output.
    let s = <dyn Mapping>::simplify(&chain).unwrap();
    let actual = s.transform(&pts, true).unwrap();

    for j in 0..3 {
        assert!(
            (actual.point(0)[j] - expected.point(0)[j]).abs() < 1e-10,
            "axis {j}: transform changed after simplification: {} vs {}",
            actual.point(0)[j],
            expected.point(0)[j],
        );
    }
    // None of the outputs should be BAD_VALUE.
    for j in 0..3 {
        assert_ne!(
            actual.point(0)[j],
            BAD_VALUE,
            "axis {j}: output is BAD_VALUE after simplification"
        );
    }
}

/// Count "Begin <ClassName>" occurrences in a native serialization string.
fn count_class(serialized: &str, class: &str) -> usize {
    let pattern = format!("Begin {class}");
    serialized.matches(&pattern).count()
}

/// Render a Mapping back to native form via the AnyObject wrapper, for
/// structural inspection. Falls back to a debug string if downcast fails.
fn render_mapping(m: &dyn Mapping) -> String {
    use ast_core::AnyObject;
    use ast_core::mappings::{CmpMap, MatrixMap, PermMap, ShiftMap, WinMap, ZoomMap};
    use ast_core::to_native_string;

    let obj = if let Some(x) = m.as_any().downcast_ref::<CmpMap>() {
        AnyObject::CmpMap(x.clone())
    } else if let Some(x) = m.as_any().downcast_ref::<WinMap>() {
        AnyObject::WinMap(x.clone())
    } else if let Some(x) = m.as_any().downcast_ref::<PermMap>() {
        AnyObject::PermMap(x.clone())
    } else if let Some(x) = m.as_any().downcast_ref::<MatrixMap>() {
        AnyObject::MatrixMap(x.clone())
    } else if let Some(x) = m.as_any().downcast_ref::<ShiftMap>() {
        AnyObject::ShiftMap(x.clone())
    } else if let Some(x) = m.as_any().downcast_ref::<ZoomMap>() {
        AnyObject::ZoomMap(x.clone())
    } else {
        return format!("{m:?}");
    };
    to_native_string(&obj).unwrap_or_else(|e| format!("(serialize failed: {e})"))
}

/// WinMap before a parallel CmpMap whose branches each contain a non-linear
/// mapping that cannot merge with a prepended WinMap. The simplifier must
/// NOT distribute the WinMap into the branches (matching C winmap.c:1358-1375
/// which only commits the distribution if at least one branch actually
/// simplifies as a result).
#[test]
fn winmap_does_not_distribute_when_branches_dont_simplify() {
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, SpecCvtType, SpecMap, WinMap};

    // 2-input WinMap ; (SpecMap || SpecMap), each branch 1-axis non-linear.
    let wm = WinMap::new(&[1.0, 2.0], &[3.0, 5.0]).unwrap();
    let mut sm1 = SpecMap::new();
    sm1.add(SpecCvtType::FrToVl, &[330_000_000_000.0]);
    let mut sm2 = SpecMap::new();
    sm2.add(SpecCvtType::FrToVl, &[140_000_000_000.0]);

    let par = CmpMap::new(Box::new(sm1), Box::new(sm2), Composition::Parallel).unwrap();
    let full = CmpMap::new(Box::new(wm), Box::new(par), Composition::Series).unwrap();

    // Numerical correctness: simplified must give the same forward result.
    let pts = PointSet::from_rows([[1.0e6, 2.0e6], [-3.0e6, 7.5e5]]);
    let expected = full.transform(&pts, true).unwrap();

    let s = <dyn Mapping>::simplify(&full).unwrap();
    let actual = s.transform(&pts, true).unwrap();
    for i in 0..pts.npoint() {
        for j in 0..2 {
            assert!(
                (actual.point(i)[j] - expected.point(i)[j]).abs() < 1e-6,
                "point {i} axis {j}: got {}, expected {}",
                actual.point(i)[j],
                expected.point(i)[j]
            );
        }
    }

    // Structural: the WinMap must NOT have been distributed. Only one
    // WinMap should remain in the serialized form.
    let rendered = render_mapping(s.as_ref());
    assert_eq!(
        count_class(&rendered, "WinMap"),
        1,
        "WinMap should not have distributed when neither branch simplifies\n\
         Got:\n{rendered}"
    );
}

/// WinMap (2D) ; PermMap (2→3 with one constant axis) ; WinMap (3D)
///
/// The rigby simplification pattern: a WinMap precedes a dimension-changing
/// PermMap that injects a constant. The C simplifier (winmap.c WinPerm)
/// pushes the WinMap past the PermMap by building a new WinMap on the
/// PermMap's output side: connected axes copy the WinMap's scale/shift,
/// constant axes get scale=1 / shift=0. Then the new (3D) WinMap merges
/// with the trailing WinMap.
///
/// Result: 3-mapping chain → 2-mapping chain (PermMap ; WinMap_merged).
#[test]
fn winmap_swaps_past_permmap_with_constants() {
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, PermMap, WinMap};

    // outperm=[1, -1, 2]: out0 ← in[0], out1 = const[0]=99.0, out2 ← in[1]
    // inperm=[1, 3]: bidirectional links for the connected axes.
    let wm1 = WinMap::new(&[1.0, 2.0], &[3.0, 5.0]).unwrap();
    let pm = PermMap::new(&[1, 3], &[1, -1, 2], &[99.0]).unwrap();
    let wm2 = WinMap::new(&[10.0, 20.0, 30.0], &[2.0, 4.0, 6.0]).unwrap();

    let c1 = CmpMap::new(Box::new(wm1), Box::new(pm), Composition::Series).unwrap();
    let full = CmpMap::new(Box::new(c1), Box::new(wm2), Composition::Series).unwrap();

    // Reference forward transform on test points.
    let pts = PointSet::from_rows([[7.0, 11.0], [-1.0, 0.5]]);
    let expected = full.transform(&pts, true).unwrap();

    let s = <dyn Mapping>::simplify(&full).unwrap();
    let actual = s.transform(&pts, true).unwrap();

    for i in 0..pts.npoint() {
        for j in 0..3 {
            assert!(
                (actual.point(i)[j] - expected.point(i)[j]).abs() < 1e-10,
                "forward point {i} axis {j}: got {}, expected {}",
                actual.point(i)[j],
                expected.point(i)[j]
            );
        }
    }

    // Structural check: only ONE WinMap should remain (the leading 2D
    // WinMap was pushed past the PermMap and merged with the trailing
    // 3D WinMap).
    let rendered = render_mapping(s.as_ref());
    assert_eq!(
        count_class(&rendered, "WinMap"),
        1,
        "Expected one WinMap after simplification, got:\n{rendered}"
    );
    assert_eq!(
        count_class(&rendered, "PermMap"),
        1,
        "Expected one PermMap after simplification, got:\n{rendered}"
    );
}

/// Mirror case (NOT triggered by rigby): PermMap (2→3 with constants) ; WinMap (3D)
///
/// The PermMap is followed by a WinMap. The C simplifier's `iwm == 1`
/// branch pushes the WinMap *backwards* past the PermMap, producing
/// a new WinMap on the input side (2D). Connected axes copy the
/// WinMap's scale/shift indexed by `inperm`; constant input axes get
/// scale=1 / shift=0. The forward-direction constants in the new
/// PermMap must be adjusted: since there is no longer a forward
/// WinMap to scale them, the constant becomes (a + c*b).
///
/// This case fires only when there's a merge partner reachable
/// before the PermMap (e.g. another linear mapping).
#[test]
fn permmap_with_constants_followed_by_winmap_swaps_back() {
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, PermMap, WinMap};

    // Pre-pend a WinMap so the swap has a merge target on the LHS.
    // Structure: WinMap_2D ; PermMap_2to3_with_const ; WinMap_3D
    //   Pass 1 might fire the forward-swap; we want to specifically
    //   exercise the reverse swap. So build a chain where the LHS
    //   WinMap is identity (forces only the RHS WinMap to swap back).
    //   Actually identity is trivially merged; instead use a UnitMap
    //   on the LHS, and a non-identity 2D WinMap that needs to merge
    //   on the LHS via the swap.
    //
    // Cleaner construction: PermMap_3to2 ; WinMap_2D   (3 → 2 → 2)
    // and a leading WinMap_3D that merges via swap.
    //
    // Chain: WinMap_3D ; PermMap_3to2_with_const ; WinMap_2D
    //   - C will swap WinMap_2D backwards through the PermMap, producing
    //     a 3D WinMap, which then merges with the leading WinMap_3D.
    let wm_left = WinMap::new(&[1.0, 2.0, 3.0], &[2.0, 3.0, 5.0]).unwrap();
    // outperm=[1, -1]: out0 ← in[0], out1 = const[0]=42.0
    // inperm=[1, 3, -2]: in0 ← out[0], in1 ← out[2] (BAD via 0-based 2),
    //                    in2 = const[1]=11.0
    // Wait: nout=2, so inperm entries can be 1 or 2 (1-based).
    // Use inperm=[1, 2, -1]: in0 ← out[0], in1 ← out[1], in2 = const[0].
    // But then bidirectionality: out[0] ↔ in[0] ✓, out[1] = const so no link, in[2]=const so no link.
    // outperm=[1, 2]: out0 ← in[0], out1 ← in[1]. But inperm has 3 entries (nin=3) and outperm has 2 (nout=2).
    // outperm[0]=1 → in[0], inperm[0]=1 → out[0]. Bidirectional. ✓
    // outperm[1]=2 → in[1], inperm[1]=2 → out[1]. Bidirectional. ✓
    // inperm[2]=-1 → const[0]=42.0 (used only when going backwards through forward direction; doesn't link).
    //
    // So this is a "lossy" PermMap: forward 3→2 drops in[2], inverse 2→3 supplies in[2]=42.0.
    let pm = PermMap::new(&[1, 2, -1], &[1, 2], &[42.0]).unwrap();
    let wm_right = WinMap::new(&[7.0, 13.0], &[2.0, 4.0]).unwrap();

    let c1 = CmpMap::new(Box::new(wm_left), Box::new(pm), Composition::Series).unwrap();
    let full = CmpMap::new(Box::new(c1), Box::new(wm_right), Composition::Series).unwrap();

    // Forward transform reference.
    let pts_3d = PointSet::from_rows([[1.0, 2.0, 3.0], [-1.0, 0.0, 5.5]]);
    let expected_fwd = full.transform(&pts_3d, true).unwrap();

    let s = <dyn Mapping>::simplify(&full).unwrap();
    let actual_fwd = s.transform(&pts_3d, true).unwrap();

    assert_eq!(actual_fwd.npoint(), expected_fwd.npoint());
    assert_eq!(actual_fwd.ncoord(), 2);
    for i in 0..pts_3d.npoint() {
        for j in 0..2 {
            assert!(
                (actual_fwd.point(i)[j] - expected_fwd.point(i)[j]).abs() < 1e-10,
                "forward point {i} axis {j}: got {}, expected {}",
                actual_fwd.point(i)[j],
                expected_fwd.point(i)[j]
            );
        }
    }

    // The simplified chain should be 2 mappings: WinMap ; PermMap (only one
    // WinMap remains; the leading 3D WinMap merged with the new 3D WinMap
    // produced by swapping the trailing 2D WinMap backwards through the
    // PermMap).
    let rendered = render_mapping(s.as_ref());
    assert_eq!(
        count_class(&rendered, "WinMap"),
        1,
        "Expected one WinMap after simplification, got:\n{rendered}"
    );
    assert_eq!(
        count_class(&rendered, "PermMap"),
        1,
        "Expected one PermMap after simplification, got:\n{rendered}"
    );
}

/// Inverse-constants adjustment (NOT triggered by rigby): PermMap with a
/// constant on the inperm side.
///
/// When `inperm[i] < 0`, the inverse PermMap injects a constant on input
/// axis `i`. After swapping a forward-direction WinMap past the PermMap,
/// any inverse-direction constant must be adjusted to absorb the inverse
/// WinMap that no longer exists: `consts[k] = (c - a[i]) / b[i]`
/// (winmap.c:3127). The test verifies the inverse transform of the
/// simplified mapping matches the original.
#[test]
fn winmap_swaps_past_permmap_inverse_constants() {
    use ast_core::coords::PointSet;
    use ast_core::mappings::{CmpMap, Composition, Mapping, PermMap, WinMap};

    // Forward: 2D in → 3D out
    //   outperm=[1, -1, 2]: out0 ← in[0], out1 = const[0]=99.0, out2 ← in[1]
    //   inperm=[-2, 3]: in0 = const[1]=7.0, in1 ← out[2]
    //   constants = [99.0, 7.0]
    //
    // Forward path (with WinMap before): a WinMap-2D ; PermMap_2to3.
    // Bidirectional check: out[0] ← in[0]; inperm[0] = -2 (constant), so
    // out[0] is NOT linked back to anything — the link from in[0] to out[0]
    // is uni-directional (forward only).
    //
    // For the swap to be valid, all links must be bidirectional. So
    // outperm[0] = 1 means in[0] → out[0]; for this to be bi-directional,
    // inperm[0] should be 1 (= out[0]→in[0]). But inperm[0] = -2 (const).
    //
    // So we need a different PermMap where ALL connected axes are
    // bi-directional, but at least one inperm entry is negative
    // (a constant on the inverse side).
    //
    // Construction: PermMap 3→3 (square)
    //   outperm = [1, 2, 3]
    //   inperm  = [1, 2, -1]   ← in[2] is a constant on the inverse side
    //   constants = [55.0]
    // Wait: outperm[2]=3 means out[2] ← in[2]. inperm[2]=-1 (const) is
    // INconsistent — out[2] is sourced from in[2] forward, but inperm
    // says in[2] = const (the inverse path doesn't read out[2]).
    //
    // Bi-directionality: out[2]↔in[2] requires inperm[2]=3 (1-based).
    // For an UNCONNECTED axis with no forward link, outperm[k] would
    // be -j (constant) or 0 (BAD).
    //
    // Realistic case: 3→2 PermMap.
    //   outperm = [1, 2]: out0 ← in[0], out1 ← in[1]
    //   inperm  = [1, 2, -1]: in0 ← out[0], in1 ← out[1], in2 = const[0]=55.0
    //   The "in2=const" only matters in the inverse direction (output 3 of
    //   inverse PermMap).
    //
    // Bidirectionality check (for connected axes):
    //   out[0] ← in[0], inperm[0] = 1 → out[0]. ✓
    //   out[1] ← in[1], inperm[1] = 2 → out[1]. ✓
    //   in[2] = const, no link.
    //
    // So this PermMap is bi-directional and has a constant on the
    // inperm side. Forward: 3D → 2D. Inverse: 2D → 3D with const on axis 2.
    let wm = WinMap::new(&[1.0, 2.0, 3.0], &[2.0, 3.0, 5.0]).unwrap();
    let pm = PermMap::new(&[1, 2, -1], &[1, 2], &[55.0]).unwrap();
    let wm_right = WinMap::new(&[7.0, 11.0], &[4.0, 6.0]).unwrap();

    // Build the chain. To make the swap fire on the forward (LHS) WinMap,
    // we need a merge partner on the RHS of the PermMap. wm_right serves
    // that purpose.
    let c1 = CmpMap::new(Box::new(wm), Box::new(pm), Composition::Series).unwrap();
    let full = CmpMap::new(Box::new(c1), Box::new(wm_right), Composition::Series).unwrap();

    // Forward transform on 3D points.
    let pts_3d = PointSet::from_rows([[1.0, 2.0, 3.0], [0.0, -1.0, 4.0]]);
    let expected_fwd = full.transform(&pts_3d, true).unwrap();

    let s = <dyn Mapping>::simplify(&full).unwrap();
    let actual_fwd = s.transform(&pts_3d, true).unwrap();
    for i in 0..pts_3d.npoint() {
        for j in 0..2 {
            assert!(
                (actual_fwd.point(i)[j] - expected_fwd.point(i)[j]).abs() < 1e-10,
                "forward point {i} axis {j}: got {}, expected {}",
                actual_fwd.point(i)[j],
                expected_fwd.point(i)[j]
            );
        }
    }

    // Inverse transform on 2D points: takes 2D back to 3D, with axis 2
    // injected as a constant scaled through the inverse WinMap.
    let pts_2d = PointSet::from_rows([[10.0, 20.0], [-3.0, 5.0]]);
    let expected_inv = full.transform(&pts_2d, false).unwrap();
    let actual_inv = s.transform(&pts_2d, false).unwrap();
    assert_eq!(actual_inv.ncoord(), 3);
    for i in 0..pts_2d.npoint() {
        for j in 0..3 {
            assert!(
                (actual_inv.point(i)[j] - expected_inv.point(i)[j]).abs() < 1e-10,
                "inverse point {i} axis {j}: got {}, expected {}",
                actual_inv.point(i)[j],
                expected_inv.point(i)[j]
            );
        }
    }
}

/// C's `IsSimple` flag (Rust `has_been_simplified`) is set by `simplify()`
/// and cleared by `invert()` — mirroring C `astSetInvert`, which clears
/// `IsSimple` because the inverse may simplify further (mapping.c:23229).
/// Oracle-verified against the reference library (1 after simplify, 0 after
/// invert).
#[test]
fn invert_clears_has_been_simplified() {
    use ast_core::mappings::{CmpMap, Composition, DynMapping, ZoomMap};

    let z1 = ZoomMap::new(1, 2.0).unwrap();
    let z2 = ZoomMap::new(1, 3.0).unwrap();
    let cm: DynMapping =
        Box::new(CmpMap::new(Box::new(z1), Box::new(z2), Composition::Series).unwrap());

    let mut s = cm.as_ref().simplify().unwrap();
    assert!(s.has_been_simplified(), "simplify() should set the flag");

    s.invert();
    assert!(
        !s.has_been_simplified(),
        "invert() should clear the flag (C clears IsSimple)"
    );
}
