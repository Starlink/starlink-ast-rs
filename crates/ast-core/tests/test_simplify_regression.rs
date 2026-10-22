//! Data-driven simplification regression tests.
//!
//! Reads `fixtures/simplify/cases.txt` -- the same manifest the upstream C
//! harness reads, with the same paths -- and for each entry: loads the input
//! .map file,
//! simplifies it, and verifies:
//!   1. `equal()` — the simplified result must be equal to the C reference
//!      (.simp file), both read from their dumps, as the C harness's
//!      `ast_astequal` compares them. This is the primary assertion.
//!   2. Transform equivalence — the simplified result must produce the same
//!      numerical output as the C reference on test points.
//!   3. Serialization — unless the manifest's fourth column says otherwise,
//!      the native serialization of the simplified result must match the
//!      .simp file, numeric tokens equal to within 4 ULP.

use crate::common::native_dump;
use std::path::PathBuf;

use crate::common::from_native_string;
use ast_core::AnyObject;
use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::frames::FrameRef;
use ast_core::mappings::Mapping;

use crate::common;

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

/// Cases where the C reference carries one `IsSimp` card the port cannot: C
/// stamped a sub-Mapping inside a merge probe it then abandoned.
///
/// `CmpMap::MapMerge`'s series-in-parallel probe assembles two trial CmpMaps with
/// `CombineMaps` and simplifies each (`cmpmap.c:1608-1609`), keeping the result
/// only if it is simpler (`cmpmap.c:1621`) and annulling every pointer either way
/// (`cmpmap.c:1624-1628`). `CombineMaps` clones rather than copies
/// (`cmpmap.c:626-627`), and `astInitCmpMap_` clones again (`cmpmap.c:4650`,
/// `cmpmap.c:4657`), so a trial CmpMap holds the very Mappings the live tree
/// holds. Simplifying it recurses into `MapMerge`, which stamps whatever
/// `astSimplify` returns (`cmpmap.c:1443`) — and when the probe is thrown away the
/// stamp stays on the shared sub-Mapping. The neighboring parallel-in-series
/// probe guards against exactly this with `SeparateMappings`
/// (`cmpmap.c:1650-1651`); this one does not.
///
/// Owned values give each tree its own children, so no probe in the port can
/// leave a mark on the tree that outlives it. Mirroring the card would mean
/// modeling C's reference counting, which is why this is licensed rather than
/// ported. See `docs/issues/c-library-quirks.md`.
///
/// The comparison drops exactly one reference-only `IsSimp = 1` line, at a
/// position where the port's line is the reference's next line, and requires the
/// drop to happen. Every other line is compared, `IsSimp` cards included, so a
/// second missing card fails the row.
///
/// Fields: name, where the abandoned probe left the card.
const ABANDONED_PROBE_STAMP_LICENSED: &[(&str, &str)] = &[
    (
        "rigby",
        "on the SpecMap inside the celestial/spectral parallel pair",
    ),
    (
        "cap_specmap_02",
        "on the CmpMap wrapping the SpecMap/ZoomMap pair",
    ),
    (
        "cap_cmpmap_01",
        "on the CmpMap holding the differentiated SpecMap",
    ),
    (
        "cap_cmpmap_04",
        "on the CmpMap holding the differentiated SpecMap",
    ),
    (
        "fitschan_tab_spectral",
        "on the CmpMap of the MatrixMap and SphMap of the celestial rotation",
    ),
];

/// Cases where the C reference carries an `Invert = 1` card the port does not,
/// because C restored a shared component's own `Invert` after using it.
///
/// `CombineMaps` sets each component's `Invert` to the direction the new
/// CmpMap applies it in, builds the CmpMap, and then restores the component's
/// original `Invert` (`cmpmap.c:625-662`). The component is shared by pointer,
/// so whether the restored value is the one the result dumps depends on which
/// objects the surrounding simplification kept and which it copied. The port
/// holds components by value and records the direction in the enclosing
/// CmpMap's `InvA`/`InvB` alone; the two dumps describe the same Mapping.
///
/// Each entry consumes one reference-only `Invert = 1` line, at a position
/// where the port's line is the reference's next line, and must be spent.
///
/// Fields: name, which node carries it.
const SHARED_INVERT_HISTORY_LICENSED: &[(&str, &str)] = &[
    (
        "cap_cmpmap_02",
        "the first MathMap of the inverted parallel pair under MapB",
    ),
    (
        "cap_cmpmap_02",
        "the second MathMap of the inverted parallel pair under MapB",
    ),
];

/// Cases where one node's dump carries `Invert` in C and `IsSimp` in the port,
/// because C recorded a shared child's inversion on the enclosing CmpMap.
///
/// `CombineMaps` writes a child's `Invert` onto the CmpMap it builds when the two
/// share a pointer, so the pair the dump carries reflects the object graph's
/// history rather than the Mapping (`docs/issues/c-library-quirks.md`, "CmpMap
/// dumps InvA/InvB disagreeing with the child's own Invert"). The port holds
/// components by value and has no such history to record.
///
/// The comparison consumes the two as a pair at one position: the reference's
/// `Invert = 1` against the port's `IsSimp = 1`, once, on the same node. A card
/// that moved to another node, or either card appearing alone, leaves the pair
/// unspent and fails the row.
///
/// Fields: name, which node carries it.
const INVAB_ALIASING_LICENSED: &[(&str, &str)] = &[
    (
        "cap_cmpmap_01",
        "the CmpMap under MapB holding the SpecMap/ZoomMap pair",
    ),
    (
        "cap_cmpmap_04",
        "the CmpMap under MapB holding the SpecMap/ZoomMap pair",
    ),
];

/// Reference values whose recapture from the pinned C on another platform
/// differs by more than the 4-ulp token tolerance.
///
/// `tools/oracle_fixtures/check_simplify_fresh.sh` classifies these as numeric
/// drift, and the corpus-freshness rule keeps them: regenerating one on the
/// host that reports the drift would replace a cross-platform reference with
/// that host's libm. So the byte comparison drops exactly the named card, on
/// exactly the named fixture, and only while the reference and the port's own
/// value are closer together than the entry's bound.
///
/// An entry is admissible only when the port reproduces its own host's C
/// capture on that card to the last digit — which is what makes the difference
/// a platform's libm and not the port's arithmetic. Three shapes qualify:
///
/// - **Cancellation residue.** `rigby`'s `IM5` is a matrix element near zero
///   whose row-mates are of order one, so its value is the residue of a
///   subtraction and carries no significant figures at all.
/// - **A trailing-figure spread the reference does not reproduce.**
///   `cap_interval_01`'s `X5` and `X6` are the two sky half-widths of the
///   default uncertainty Box, agreeing with the reference to twelve significant
///   figures. They are nominally equal, and C on x86-64 makes them equal to the
///   last bit; the reference has them differing from *each other*, which is
///   arm64's transformation of the same two widths.
/// - **A geodesic round trip.** `cap_pointlist_01`'s `X5` and `X6` are the sky
///   half of the merged uncertainty Box's corner, which `astSetUnc` regenerates
///   by re-centring the Box and re-offsetting the corner through a
///   `CmpFrame(TimeFrame, SkyFrame)` (region.c:10871-10876, box.c Regcenter).
///   The `atan2`/`asin` pair in that round trip is where the two platforms'
///   libm parts company, twelve significant figures in.
///
/// The bound is what keeps the license from spreading: `rigby` has a second
/// `IM5` card, holding an exact zero, that the same card name would otherwise
/// also have covered, and the bound is far tighter than the gap between the two.
///
/// The reference value is matched byte for byte, because the corpus is fixed
/// bytes. The port's value is matched numerically, within the bound, because
/// the port's is whatever the host's libm gives: the corpus was captured on
/// arm64 macOS, so a build there reproduces the reference exactly while an
/// x86-64 build lands the recorded distance away. Pinning the port's side to
/// one host's digits would make the license fire on that host alone.
///
/// `(fixture, card name, value in the reference, value an x86-64 Linux build
/// produces, bound on the difference between the reference and the port)`.
const NUMERIC_PLATFORM_DRIFT: &[(&str, &str, &str, &str, f64)] = &[
    (
        "cap_interval_01",
        "X5",
        "2.4240684055495722e-06",
        "2.4240684055476816e-06",
        1e-17,
    ),
    (
        "cap_interval_01",
        "X6",
        "2.4240684055430873e-06",
        "2.4240684055476816e-06",
        1e-17,
    ),
    (
        "cap_pointlist_01",
        "X5",
        "3.1241551949160478",
        "3.1241551949160575",
        1e-14,
    ),
    (
        "cap_pointlist_01",
        "X6",
        "-0.20069734795308758",
        "-0.20069734795284888",
        1e-12,
    ),
];

/// The value a card line carries, as written, with any trailing comment removed.
fn card_value<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix(prefix)?;
    Some(rest.split('\t').next().unwrap_or(rest).trim())
}

/// Drop the lines one `NUMERIC_PLATFORM_DRIFT` entry licenses, from both sides.
///
/// A reference line is dropped only when its card name matches the entry and it
/// carries the value the entry records, written exactly as the corpus writes
/// it. The port's line is dropped only when its card name matches and its value
/// sits within the entry's bound of the reference's, since which digits the port
/// produces is the host's libm's business. Exactly one line must be dropped from
/// each side; any other count is returned as a failure, so the license cannot
/// widen to cover a card that grew, moved or multiplied.
fn drop_licensed_residue(name: &str, got: &str, want: &str) -> (String, String, Vec<String>) {
    let mut problems = Vec::new();
    let mut got = got.to_string();
    let mut want = want.to_string();

    for (fixture, card, ref_value, local_value, bound) in NUMERIC_PLATFORM_DRIFT {
        if *fixture != name {
            continue;
        }
        // The license covers a difference, so it lapses the moment the two
        // named values are further apart than the entry admits.
        let spread = match (ref_value.parse::<f64>(), local_value.parse::<f64>()) {
            (Ok(r), Ok(l)) => (r - l).abs(),
            _ => f64::INFINITY,
        };
        if spread >= *bound {
            problems.push(format!(
                "{name}: NUMERIC_PLATFORM_DRIFT names {card} values {spread:e} apart, \
                 which its {bound:e} bound does not admit"
            ));
        }

        let prefix = format!("{card} =");
        let reference: Option<f64> = ref_value.parse().ok();
        // The reference is fixed bytes, so it is matched as written. The port's
        // value is whatever this host's libm gives, so it is matched by
        // distance from the reference instead.
        let want_residue =
            |line: &str| -> bool { card_value(line, &prefix).is_some_and(|v| v == *ref_value) };
        let got_residue = |line: &str| -> bool {
            match (
                card_value(line, &prefix).and_then(|v| v.parse::<f64>().ok()),
                reference,
            ) {
                (Some(v), Some(r)) => (v - r).abs() < *bound,
                _ => false,
            }
        };
        for (side, matches, text) in [
            ("got", &got_residue as &dyn Fn(&str) -> bool, &mut got),
            ("want", &want_residue as &dyn Fn(&str) -> bool, &mut want),
        ] {
            let kept: Vec<&str> = text.lines().filter(|l| !matches(l)).collect();
            let dropped = text.lines().count() - kept.len();
            if dropped != 1 {
                let seen: Vec<&str> = text
                    .lines()
                    .filter_map(|l| card_value(l, &prefix))
                    .collect();
                problems.push(format!(
                    "{name}: NUMERIC_PLATFORM_DRIFT licenses one {card} within \
                     {bound:e} of {ref_value}, but dropped {dropped} from the \
                     {side} side, which carries [{}]",
                    seen.join(", ")
                ));
            }
            *text = kept.join("\n");
        }
    }

    (got, want, problems)
}

/// The license must hold on any host, including the arm64 one the corpus was
/// captured on, where the port reproduces the reference to the last digit.
///
/// `rigby`'s second `IM5` card, an exact zero, stands in for what the bound has
/// to keep out: a card-name-only rule would consume it too.
#[test]
fn drift_licence_fires_wherever_the_port_lands_inside_the_bound() {
    let want = "X5 = 0\nX5 = 2.4240684055495722e-06\nX6 = 2.4240684055430873e-06";
    for local in ["2.4240684055495722e-06", "2.4240684055476816e-06"] {
        let got = format!("X5 = 0\nX5 = {local}\nX6 = 2.4240684055430873e-06");
        let (g, w, problems) = drop_licensed_residue("cap_interval_01", &got, want);
        assert!(problems.is_empty(), "{local}: {problems:?}");
        assert_eq!(g, w, "{local}: the two sides must be left identical");
        assert!(g.contains("X5 = 0"), "{local}: the exact zero must survive");
    }
}

/// And it must lapse outside the bound, rather than absorbing the difference.
#[test]
fn drift_licence_lapses_outside_the_bound() {
    let want = "X5 = 0\nX5 = 2.4240684055495722e-06\nX6 = 2.4240684055430873e-06";
    let got = "X5 = 0\nX5 = 2.5e-06\nX6 = 2.4240684055430873e-06";
    let (_, _, problems) = drop_licensed_residue("cap_interval_01", got, want);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("dropped 0 from the got side"),
        "{problems:?}"
    );
}

/// Consume the lines the two aliasing licenses allow, walking both dumps in step.
///
/// Each license is spent positionally, not by searching for a card name: a
/// reference-only line counts only where the port's line is the reference's *next*
/// line, and a substituted pair counts only where both halves sit at the same
/// position. That is what stops a license naming a common card from absorbing a
/// divergence somewhere else in the tree.
///
/// Lines are matched with the same 4-ulp token comparison the verdict uses, so a
/// numeric residue inside tolerance does not look like a divergence and shift the
/// walk. A genuine divergence is kept on both sides and the walk carries on
/// aligned, so the verdict reports it and the diagnostic prints it.
///
/// Returns the trimmed pair and one problem per license entry left unspent —
/// which is how a row that has started matching C becomes visible.
fn drop_aliasing_lines(name: &str, got: &str, want: &str) -> (String, String, Vec<String>) {
    // (reference card, port card or none, reason, spent)
    let mut budget: Vec<(&str, Option<&str>, &str, bool)> = Vec::new();
    for (fixture, reason) in ABANDONED_PROBE_STAMP_LICENSED {
        if *fixture == name {
            budget.push(("IsSimp = 1", None, reason, false));
        }
    }
    for (fixture, reason) in SHARED_INVERT_HISTORY_LICENSED {
        if *fixture == name {
            budget.push(("Invert = 1", None, reason, false));
        }
    }
    for (fixture, reason) in INVAB_ALIASING_LICENSED {
        if *fixture == name {
            budget.push(("Invert = 1", Some("IsSimp = 1"), reason, false));
        }
    }
    if budget.is_empty() {
        return (got.to_string(), want.to_string(), Vec::new());
    }

    let same = |a: &str, b: &str| common::line_tokens_equivalent(a, b, 4);
    let (g, w): (Vec<&str>, Vec<&str>) = (got.lines().collect(), want.lines().collect());
    let (mut kept_got, mut kept_want): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
    let (mut i, mut j) = (0usize, 0usize);

    while i < g.len() && j < w.len() {
        if same(g[i], w[j]) {
            kept_got.push(g[i]);
            kept_want.push(w[j]);
            i += 1;
            j += 1;
            continue;
        }

        // A line the reference carries alone, licensed as an abandoned probe's
        // stamp: the port's line here is the reference's next line.
        let insertion = w
            .get(j + 1)
            .is_some_and(|next| same(g[i], next))
            .then(|| {
                budget.iter_mut().find(|(r, p, _, spent)| {
                    p.is_none() && !*spent && w[j].trim_start().starts_with(*r)
                })
            })
            .flatten();
        if let Some(entry) = insertion {
            entry.3 = true;
            j += 1;
            continue;
        }

        // One node's card written differently on each side.
        let substitution = budget.iter_mut().find(|(r, p, _, spent)| {
            !*spent
                && p.is_some_and(|p| g[i].trim_start().starts_with(p))
                && w[j].trim_start().starts_with(*r)
        });
        if let Some(entry) = substitution {
            entry.3 = true;
            i += 1;
            j += 1;
            continue;
        }

        kept_got.push(g[i]);
        kept_want.push(w[j]);
        i += 1;
        j += 1;
    }
    kept_got.extend_from_slice(&g[i..]);
    kept_want.extend_from_slice(&w[j..]);

    let problems = budget
        .iter()
        .filter(|(_, _, _, spent)| !*spent)
        .map(|(r, p, reason, _)| match p {
            Some(p) => format!(
                "{name}: INVAB_ALIASING_LICENSED covers a reference `{r}` against a \
                 port `{p}` on {reason}, and found no such pair"
            ),
            None => format!(
                "{name}: {} covers one reference-only `{r}` {reason}, and found none",
                if *r == "Invert = 1" {
                    "SHARED_INVERT_HISTORY_LICENSED"
                } else {
                    "ABANDONED_PROBE_STAMP_LICENSED"
                }
            ),
        })
        .collect();

    (kept_got.join("\n"), kept_want.join("\n"), problems)
}

/// Try to wrap a DynMapping back into an AnyObject for serialization.
/// Downcasts to known concrete types.
fn mapping_to_any_object(m: &dyn Mapping) -> Option<AnyObject> {
    AnyObject::from_mapping(m)
}

/// Render `m` with the default options (comments on, indent 3).
fn mapping_to_native_string(m: &dyn Mapping) -> Option<String> {
    native_dump(&mapping_to_any_object(m)?).ok()
}

/// `m` written to a dump and read back, as the C harness reads the
/// simplified Mapping it compares with the reference.
fn reread(m: &dyn Mapping) -> Option<Box<dyn Mapping>> {
    from_native_string(&mapping_to_native_string(m)?)
        .ok()?
        .into_mapping()
}

/// How a manifest row asks for the serialized dump to be compared -- the
/// fourth column of `fixtures/simplify/cases.txt`, shared verbatim with the
/// upstream C harness (`ast_tester/CMakeLists.txt`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Compare {
    /// Blank: numeric tokens equal to within 4 ULP. The default.
    Ulp,
    /// `astequal`: numeric tokens equal to within AST's own `astEQUAL`
    /// tolerance. For a value amplified by cancellation, where the row above
    /// it in the manifest names the value.
    AstEqual,
    /// `yes`: no tolerance-based comparison. Upstream uses this for a negative
    /// fixture, whose reference is its own unsimplified `.map` and so can
    /// never carry the `IsSimp` card `astSimplify` stamps on its result. This
    /// harness still compares those, with `IsSimp` stripped from both sides --
    /// the job upstream gives its `_noop` ctest family. The flag therefore
    /// suppresses the comparison only for a row whose reference is a distinct
    /// file, which is a genuine opt-out.
    None,
}

impl Compare {
    fn parse(field: &str, name: &str) -> Self {
        match field.trim() {
            "" => Self::Ulp,
            "astequal" => Self::AstEqual,
            "yes" => Self::None,
            other => panic!(
                "{name}: unknown skip_string_compare value {other:?}; want blank, \
                 `astequal` or `yes`"
            ),
        }
    }
}

/// Manifest rows whose reference records a C defect the port deliberately does
/// not reproduce, each with the reason. Empty.
///
/// The upstream harness compares these, because upstream's own output is the
/// reference. Reproducing one here would mean returning a Mapping that does not
/// agree with the one it replaces, so such a row is excluded from the three
/// checks and pinned instead by `c_defect_rows_still_diverge`, which fails if
/// the port ever starts matching -- whether because upstream fixed the defect or
/// because the port acquired it. `cheby_linear_lone_reduce` was the one entry,
/// and that gate is what reported the fix: upstream folded the ChebyMap
/// normalization into the reduction, the port ported it, and the row is compared
/// in full like any other.
///
/// The table survives empty so a newly diagnosed C-side artifact has a home.
const C_DEFECT_EXCLUDED: &[(&str, &str)] = &[];

/// Manifest rows whose fixture the Rust reader cannot load yet, each with the
/// class that blocks it. Named here rather than skipped by a predicate so that
/// the list shrinks only by deliberate edit, and so `deferred_rows_still_fail`
/// can assert that every one of them really does still fail to load.
const DEFERRED: &[(&str, &str)] = &[];

#[test]
fn simplify_regression() {
    let fixture_dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"));
    let manifest = fixture_dir.join("simplify/cases.txt");

    assert!(
        manifest.exists(),
        "{} not found. The manifest is committed, so its absence is a broken \
         checkout rather than a reason for this gate to pass.",
        manifest.display()
    );

    let contents = std::fs::read_to_string(&manifest).unwrap();
    let mut passed = 0usize;
    let mut skipped_missing = 0usize;
    let mut parse_skipped: Vec<String> = Vec::new();
    let mut equal_failures: Vec<String> = Vec::new();
    let mut transform_failures: Vec<String> = Vec::new();
    let mut serial_failures: Vec<String> = Vec::new();
    let mut other_failures: Vec<String> = Vec::new();
    let mut issimp_disagreements: Vec<String> = Vec::new();
    let mut deferred_seen = 0usize;
    let mut defect_seen = 0usize;

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        assert_eq!(
            fields.len(),
            4,
            "malformed manifest row (want `name | input | reference | skip`): {line}"
        );

        let name = fields[0];
        if DEFERRED.iter().any(|(n, _)| *n == name) {
            deferred_seen += 1;
            continue;
        }
        if C_DEFECT_EXCLUDED.iter().any(|(n, _)| *n == name) {
            defect_seen += 1;
            continue;
        }
        let input_path = fixture_dir.join(fields[1]);
        let reference_path = fixture_dir.join(fields[2]);
        let compare = Compare::parse(fields[3], name);
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
        // skip: an unported class (those are in `DEFERRED`, so should not
        // reach here) or a genuine parser gap.
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

        // === Check 1: equal() — equality with the C reference ===
        // The C harness compares the two dumps read back (`ast_astequal`),
        // and so does this: a dump does not carry everything equality looks
        // at, such as a base Frame written only through `Frm`.
        let structurally_equal =
            reread(simplified.as_ref()).is_some_and(|m| m.equal(ref_mapping.as_ref()));
        if !structurally_equal {
            equal_failures.push(name.to_string());
        }

        // === Check 2: Transform equivalence with C reference ===
        let nin = mapping.nin();
        let npoints = 10;
        let pts = make_test_points(nin, npoints);

        // A Mapping that defines no forward transformation cannot be
        // evaluated on either side, as in C (mapping.c `Transform`); the
        // checks of the dump below still run for it.
        match (
            ref_mapping.transform(&pts, true),
            simplified.transform(&pts, true),
        ) {
            (Ok(out_ref), Ok(out_simp)) => {
                if !outputs_equivalent(&out_simp, &out_ref, 1e-6) {
                    transform_failures.push(name.to_string());
                }
            }
            (Err(_), Err(_)) => {}
            (Ok(_), Err(e)) => {
                other_failures.push(format!("{name}: simplified transform failed: {e}"));
            }
            (Err(e), Ok(_)) => {
                other_failures.push(format!(
                    "{name}: the reference cannot transform ({e}) but the simplified \
                     Mapping can"
                ));
            }
        }

        // Every fixture whose `IsSimp` card count differs from C's, collected
        // for the assertion after the loop. A negative fixture differs by
        // construction — its reference *is* its input, which was never
        // simplified — so those are excluded: they are not a disagreement
        // about placement.
        let rendered = mapping_to_native_string(simplified.as_ref());
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
            && let Some(s) = mapping_to_native_string(simplified.as_ref())
        {
            eprintln!("--- got {name} ---\n{s}\n--- want ---\n{ref_text}");
        }

        // === Check 3: Serialization comparison ===
        // `Compare::None` on a negative fixture is upstream's `_noop` gate,
        // which this harness runs inline (`ignore_is_simp_line`), so such a row
        // is still compared. A row with a distinct reference opts out unless a
        // `NUMERIC_PLATFORM_DRIFT` entry names the drifting card: upstream has
        // no card-level license and so gives up the whole dump, while this
        // harness can license the one value and keep comparing the rest.
        let licensed_per_card = NUMERIC_PLATFORM_DRIFT.iter().any(|(f, ..)| *f == name);
        if compare != Compare::None || ignore_is_simp_line || licensed_per_card {
            if let Some(simp_str) = mapping_to_native_string(simplified.as_ref()) {
                let (got, want) = if ignore_is_simp_line {
                    (
                        common::strip_is_simp(&simp_str),
                        common::strip_is_simp(&ref_text),
                    )
                } else {
                    (simp_str.clone(), ref_text.clone())
                };
                let (got, want, residue_problems) = drop_licensed_residue(name, &got, &want);
                other_failures.extend(residue_problems);
                let (got, want, aliasing_problems) = drop_aliasing_lines(name, &got, &want);
                other_failures.extend(aliasing_problems);
                // A negative fixture's reference is its own input, which was
                // never simplified and so carries no `IsSimp` at all, so the
                // strip above would hide a result tagged wrongly at the top
                // level. `astSimplify` stamps whatever it returns
                // (mapping.c:24783), and the card is written when
                // `astGetIsSimple` reports it (mapping.c:23781) — which a
                // Frame, a FrameSet and a Region never do, because the getter
                // is overridden on the Frame class to return zero
                // (frame.c:5590). So the card is required exactly when the
                // result is not Frame-kind, and forbidden when it is.
                if ignore_is_simp_line {
                    let card = has_top_level_is_simp(&simp_str);
                    let frame_kind = result_is_frame_kind(simplified.as_ref());
                    if card && frame_kind {
                        other_failures.push(format!(
                            "{name}: IsSimp card on a Frame-kind result (frame.c:5590)"
                        ));
                    } else if !card && !frame_kind {
                        other_failures.push(format!("{name}: simplify() produced no IsSimp card"));
                    }
                }
                let matched = match compare {
                    Compare::AstEqual => common::native_strings_astequal(&got, &want),
                    Compare::Ulp | Compare::None => {
                        common::native_strings_equivalent(&got, &want, 4)
                    }
                };
                if !matched {
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
    // whole result of a simplification. Upstream `e40e71b4` made that
    // reproducible for the stamps a surviving simplification leaves, so it is a
    // target this engine can be held to; these are the fixtures that do not hit
    // it. Three of the four differ by one card that C wrote during a merge probe
    // it then abandoned — the probe shares its sub-Mappings with the live tree,
    // so the stamp outlives it, and owned values cannot reproduce that
    // (`docs/issues/c-library-quirks.md`). The fourth is a structural
    // divergence of its own. Without this list the number would be invisible,
    // because a licensed or skipped string comparison never reaches the count.

    if !parse_skipped.is_empty() {
        eprintln!(
            "  parse-skipped (fixture present but not parseable — a parser \
             or class-support gap; unported classes are in `DEFERRED` and \
             should not appear here):"
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
    // difference has been diagnosed as a C-side artifact (see
    // `docs/issues/c-library-quirks.md`) opts out per-fixture via the manifest's
    // `skip_string_compare` flag, with the reason recorded there.
    //
    // Serialization failures the byte comparison is allowed to carry, each
    // diagnosed against the pinned C oracle and named with its cause. The set is
    // asserted exactly, so a row cannot be added without a diagnosis and cannot
    // survive being fixed.
    //
    // The table is empty: every fixture whose simplified form is dumped is
    // byte-compared, and any difference fails. Card-level licenses that cover a
    // single value rather than a whole fixture live in
    // `NUMERIC_PLATFORM_DRIFT`, `INVAB_ALIASING_LICENSED` and
    // `ABANDONED_PROBE_STAMP_LICENSED` instead, which is where a newly diagnosed
    // C-side artifact belongs unless it defeats the comparison entirely.
    const SERIAL_UNGATED: &[(&str, &str)] = &[];
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
    let serial_failures = unlicensed;

    // Every row here is a real gap: the pinned C oracle and the committed
    // `.simp` agree, and the port's `IsSimp` count differs from both.
    // `tools/oracle_fixtures/check_simplify_fresh.sh` is what establishes the
    // first half of that, by recapturing the whole corpus from the pinned C, so
    // a row may be added here only once that check reports the fixture fresh.
    // See PLAN.md §6.
    const ISSIMP_DISAGREEMENTS: &[&str] = &[
        // Under-tagging on a component C stamped inside a merge probe it then
        // threw away. The probe's CmpMaps are assembled by cloning, so they
        // share their sub-Mappings with the live tree and the stamp outlives the
        // probe; owned values give each tree its own children, so the port
        // cannot reproduce it. See `docs/issues/c-library-quirks.md`.
        "rigby (8 vs C's 9)",
        "cap_specmap_02 (7 vs C's 8)",
        // Shrunk from `cap_specmap_02`, and carries the same signature.
        // A FitsChan read of a `-TAB` spectral axis beside celestial axes;
        // the stamp is made by a series-in-parallel probe C discards.
        "fitschan_tab_spectral (5 vs C's 6)",
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

    // Every manifest case is accounted for: it passed, or it was counted as a
    // failure above. A skip would be a case that did neither, which is what the
    // three assertions below rule out.
    assert_eq!(
        skipped_missing, 0,
        "{skipped_missing} manifest cases have no fixture file. The corpus is \
         committed, so a missing fixture is a broken checkout or a manifest \
         entry added without its data, not a case to pass over."
    );
    assert!(
        parse_skipped.is_empty(),
        "{} fixtures failed to parse: {}. A fixture the reader cannot load is \
         an unported class or a read-path gap, and either way it is a failure \
         of this gate rather than a case outside it.",
        parse_skipped.len(),
        parse_skipped.join(", ")
    );
    assert_eq!(
        defect_seen,
        C_DEFECT_EXCLUDED.len(),
        "the manifest carries {defect_seen} of the {} C-defect rows. A row that \
         leaves the manifest must leave `C_DEFECT_EXCLUDED` in the same change.",
        C_DEFECT_EXCLUDED.len()
    );
    assert_eq!(
        deferred_seen,
        DEFERRED.len(),
        "the manifest carries {deferred_seen} of the {} deferred rows. A row \
         that leaves the manifest must leave `DEFERRED` in the same change, or \
         the entry silently stops excluding anything.",
        DEFERRED.len()
    );
    assert_eq!(
        passed, 517,
        "{passed} cases passed, where the manifest holds 517: a change means \
         cases stopped being run, which no failure count would show, or were \
         added without this count"
    );
}

/// Every `C_DEFECT_EXCLUDED` row still diverges from its reference.
///
/// The exclusion says the port refuses to reproduce the reference. That claim
/// has to be checked, or an upstream fix -- or a regression that gave the port
/// the same defect -- would leave the row silently unexercised.
#[test]
fn c_defect_rows_still_diverge() {
    let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"));
    let manifest = std::fs::read_to_string(root.join("simplify/cases.txt")).unwrap();
    let mut now_agree = Vec::new();
    for (name, reason) in C_DEFECT_EXCLUDED {
        let row = manifest
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .find(|l| l.split('|').next().map(str::trim) == Some(*name))
            .unwrap_or_else(|| panic!("{name} is in C_DEFECT_EXCLUDED but not in the manifest"));
        let mut fields = row.split('|').skip(1).map(str::trim);
        let input = fields.next().unwrap();
        let reference = fields.next().unwrap();

        let input_obj = from_native_string(&std::fs::read_to_string(root.join(input)).unwrap())
            .unwrap_or_else(|e| panic!("{name}: input did not load: {e}"));
        let ref_obj = from_native_string(&std::fs::read_to_string(root.join(reference)).unwrap())
            .unwrap_or_else(|e| panic!("{name}: reference did not load: {e}"));
        let Some(input_map) = input_obj.into_mapping() else {
            panic!("{name}: input is not a Mapping");
        };
        let Some(ref_map) = ref_obj.into_mapping() else {
            panic!("{name}: reference is not a Mapping");
        };
        let simplified = <dyn Mapping>::simplify(input_map.as_ref())
            .unwrap_or_else(|e| panic!("{name}: simplify failed: {e}"));
        if reread(simplified.as_ref()).is_some_and(|m| m.equal(ref_map.as_ref())) {
            now_agree.push(format!("{name} ({reason})"));
        }
    }
    assert!(
        now_agree.is_empty(),
        "these rows now match their reference, so the exclusion is stale -- \
         either upstream fixed the defect and the row belongs in the gate, or \
         the port acquired it: {}",
        now_agree.join("; ")
    );
}

/// Every `DEFERRED` row really is still unloadable.
///
/// Without this, a class landing in the port would leave its rows excluded and
/// the new coverage would go unclaimed.
#[test]
fn deferred_rows_still_fail_to_load() {
    let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"));
    let manifest = std::fs::read_to_string(root.join("simplify/cases.txt")).unwrap();
    let mut now_loadable = Vec::new();
    for (name, blocker) in DEFERRED {
        let row = manifest
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .find(|l| l.split('|').next().map(str::trim) == Some(*name))
            .unwrap_or_else(|| panic!("{name} is in DEFERRED but not in the manifest"));
        let input = row.split('|').nth(1).unwrap().trim();
        let text = std::fs::read_to_string(root.join(input)).unwrap();
        if from_native_string(&text).is_ok() {
            now_loadable.push(format!("{name} (was blocked on {blocker})"));
        }
    }
    assert!(
        now_loadable.is_empty(),
        "these fixtures now load, so they belong in the gate rather than in \
         `DEFERRED`: {}",
        now_loadable.join(", ")
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
    native_dump(&obj).unwrap_or_else(|e| format!("(serialize failed: {e})"))
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
    sm1.add(SpecCvtType::FrToVl, &[330_000_000_000.0]).unwrap();
    let mut sm2 = SpecMap::new();
    sm2.add(SpecCvtType::FrToVl, &[140_000_000_000.0]).unwrap();

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

/// Whether `m` is a Frame, a FrameSet or a Region.
///
/// C's `IsSimple` getter is overridden on the Frame class to return zero
/// unconditionally (`frame.c:5590`), and FrameSet and every Region inherit
/// that override, so `mapping.c:23781` writes no `IsSimp` card for any of
/// them. `AnyObject::as_frame` is exactly that set of classes.
fn result_is_frame_kind(m: &dyn Mapping) -> bool {
    AnyObject::from_mapping(m).is_some_and(|obj| obj.as_frame().is_some())
}

/// Whether a native dump carries an `IsSimp` card on its *outermost* object.
///
/// Native output indents a top-level attribute by exactly four spaces and
/// anything nested by more, so the prefix distinguishes the outermost object's
/// own cards from those of the Mappings it encapsulates.
fn has_top_level_is_simp(dump: &str) -> bool {
    dump.lines().any(|line| {
        line.strip_prefix("    ")
            .is_some_and(|rest| rest.starts_with("IsSimp"))
    })
}
