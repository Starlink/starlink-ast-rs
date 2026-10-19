//! One place that knows every way a gate in this crate can be relaxed.
//!
//! Relaxations accumulate quietly: a named exclusion list here, a manifest
//! column there, a comparator that strips a line somewhere else. Each is
//! defensible on its own and the total is what matters, so this test counts
//! them and holds the total to a table. Changing a number is then a deliberate
//! act with a diff, not a discovery someone makes months later.
//!
//! Two assertions:
//!
//! 1. **Every relaxation is the size the table says.** Widening one fails here
//!    even if the gate it relaxes still passes.
//! 2. **No unlisted relaxation exists.** A new `const NAME_EXCLUDED` /
//!    `_LICENSED` / `_SKIPPED` in any test fails until it is added to the
//!    table, which is the point where its reason has to be written down.
//!
//! The table is mirrored in `PLAN.md` §6 for readers; this is the copy that is
//! executable.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn fixtures() -> PathBuf {
    tests_dir().join("fixtures")
}

/// Every named relaxation, its size, and why it exists.
///
/// Fields: source file, const name, expected entry count, reason.
const LEDGER: &[(&str, &str, usize, &str)] = &[
    (
        "test_transform_oracle.rs",
        "UNTRANSFORMABLE",
        11,
        "fixtures that load as a bare Frame, which has no transform in the \
         port; each records whether C's outputs equal its inputs per direction, \
         and the gates check that against the oracle. region_in_cmpframe's \
         forward direction is C masking points through a Region, which the \
         port does not reproduce (PLAN.md 9.6)",
    ),
    (
        "test_fitschan_cursor_oracle.rs",
        "EXCLUDED",
        5,
        "four NATIVE reads the port parses whole where C reads card by card, \
         and native_full_1, whose RefCnt and Nobj comment cards the port has \
         no counterpart for (the rest is compared)",
    ),
    (
        "test_object_id_oracle.rs",
        "EXCLUDED",
        6,
        "C copies or shares an Object where the port moves or clones it, so \
         object IDs differ (feedback-no-clone-copy-emulation)",
    ),
    (
        "test_channel_read_oracle.rs",
        "EXCLUDED",
        2,
        "C reports AST__BADIN for a string read as a number and stops part way \
         through the Object; the port reads the string without error",
    ),
    (
        "test_fixture_read_warnings_oracle.rs",
        "EXCLUDED",
        1,
        "frame_in_map_slot.ast: a CmpFrame in a Mapping slot, refused by design",
    ),
    (
        "test_fixture_full_dump_oracle.rs",
        "EXCLUDED",
        1,
        "frame_in_map_slot.ast: a CmpFrame in a Mapping slot, refused by design",
    ),
    (
        "test_fitschan_sipok_oracle.rs",
        "EXCLUDED",
        0,
        "empty: every SipOK encoding write matches C",
    ),
    (
        "test_fitschan_encodings_oracle.rs",
        "EXCLUDED",
        0,
        "empty: every encoding write matches C",
    ),
    (
        "test_simplify_regression.rs",
        "DEFERRED",
        0,
        "empty: every simplify manifest fixture loads",
    ),
    (
        "../../ast-oracle-tests/tests/call_count_parity.rs",
        "COUNT_PARITY_EXCLUDED",
        53,
        "tier 4, not run in CI: sections whose libm/PAL/ERFA call counts differ \
         from C's because C does work the port does not need (selector Regions \
         re-closed in place, CompressMatrix's per-row sqrt); never the port \
         skipping work",
    ),
    (
        "test_region_stc_oracle.rs",
        "ERROR_ELLIPSE_LICENSED",
        3,
        "off the macOS capture platform only: the sky AstroCoords Error \
         Ellipse, an ill-conditioned fit to an almost circular shape, whose \
         defining points move with last-bit libm differences; the rest of each \
         dump is still compared",
    ),
    (
        "test_fitschan_encodings_oracle.rs",
        "VALUES_LICENSED",
        0,
        "empty: tan_offset_origin and tan_offset_pole, its two entries, match C \
         since MatrixMap products are formed in C's order; kept so a newly \
         diagnosed last-digit difference in a write has a home",
    ),
    (
        "test_fitschan_tab_write_oracle.rs",
        "TAB_WRITE_EXCLUDED",
        0,
        "empty: sparse, the last case, matches C since FrameSet integrity \
         stopped remapping the current Frame with a UnitMap; kept so a newly \
         diagnosed -TAB write difference has a home",
    ),
    (
        "test_regions_c.rs",
        "REGIONS_C_GAP_EXCLUDED",
        1,
        "checkEllipse Errors 15-17: an Ellipse mapped through a full MatrixMap \
         and back. C keeps an unrotated Box uncertainty; the port carries the \
         rotation and fails mapping back. region_gaps_still_differ fails when \
         it closes",
    ),
    (
        "test_fitschan_errors.rs",
        "FITSCHAN_ERRORS_EXCLUDED",
        3,
        "testfitschanerrors.c cases whose check is C's string attribute \
         interface: an unknown Encoding or AltAxes name, and setting the \
         read-only Ncard; typed accessors cannot express them",
    ),
    (
        "test_fitschan_errors.rs",
        "FITSCHAN_ERRORS_GAP_EXCLUDED",
        1,
        "error-bad-radesys: C's WcsFcRead fills the FitsStore card by card, so \
         the last of duplicate keywords wins; the port's keyword lookups take \
         the first. port_gap_cases_still_differ fails when it closes",
    ),
    (
        "test_fitschan_tab_read_oracle.rs",
        "TAB_READ_EXCLUDED",
        0,
        "empty: shared_table, its one entry, matches C since the reader makes \
         a SpecFrame for each spectral axis of a description, as C's \
         WcsSpectral does; kept so a newly diagnosed read difference has a home",
    ),
    (
        "test_fitschan_tab_read_oracle.rs",
        "PROBE_STAMP_LICENSED",
        1,
        "mixed: one IsSimp card C stamps during a merge probe it discards, the \
         same Mapping as fitschan_tab_spectral in the simplify corpus; removed \
         from C's transcript at its line, which must hold that card",
    ),
    (
        "test_fitschan_tab_read_oracle.rs",
        "ULP_LICENSED",
        1,
        "celestial_iwc: one WinMap shift 1 ULP from C's, which the port's \
         simplifier folds in another order; the same Mapping is \
         fitschan_tab_celestial_iwc in the simplify corpus, which passes \
         within that harness's 4 ULP token bound",
    ),
    (
        "test_wcsconv.rs",
        "ISSIMP_PLACEMENT_LICENSED",
        2,
        "C's reader reduces Mappings as it assembles them, so its cards record \
         the order that reader worked in; this port simplifies once, at the \
         end. The Mapping is the same and only which sub-Mappings were \
         momentarily a whole simplification result differs. Counts pinned on \
         both sides against the pinned oracle; every other card, Invert \
         included, is byte-compared",
    ),
    (
        "test_wcsconv.rs",
        "NATIVE_READ_EXCLUDED",
        0,
        "no skip mode: kept empty and gated against refilling",
    ),
    (
        "test_simplify_regression.rs",
        "ABANDONED_PROBE_STAMP_LICENSED",
        5,
        "one reference-only IsSimp card per fixture, which C stamped on a \
         sub-Mapping inside a merge probe it then abandoned. The probe's \
         CmpMaps are assembled by cloning, so the object it marked is one the \
         live tree still holds; owned values give each tree its own children, \
         so no probe in the port can leave a mark that outlives it \
         (docs/issues/c-library-quirks.md). Spent positionally, one line per \
         fixture, with every other line compared",
    ),
    (
        "test_simplify_regression.rs",
        "INVAB_ALIASING_LICENSED",
        2,
        "one node in each of cap_cmpmap_01 and _04 whose dump carries `Invert` \
         in C and `IsSimp` in the port. C's CombineMaps records a shared \
         child's Invert on the CmpMap it builds, which is reference-count \
         history rather than a property of the Mapping \
         (docs/issues/c-library-quirks.md). The two cards are consumed as a \
         pair at one position, so neither can go missing elsewhere unnoticed",
    ),
    (
        "test_simplify_regression.rs",
        "SHARED_INVERT_HISTORY_LICENSED",
        2,
        "two MathMaps in cap_cmpmap_02 whose dump carries `Invert = 1` in C \
         and none in the port. C's CombineMaps sets a shared component's \
         Invert while it builds a CmpMap and restores it after, so which \
         value the result dumps depends on which objects the simplification \
         kept; the port records the direction in InvA/InvB alone. Each card \
         is consumed at one position and must be spent",
    ),
    (
        "test_serialisation.rs",
        "FRAMESET_FIXTURE_EXCLUDED",
        0,
        "every wcsconv object round-trips byte-exact",
    ),
    (
        "test_transform_oracle.rs",
        "SLAMAP_ORACLE_EXCLUDED",
        0,
        "no SlaMap oracle section is excluded",
    ),
    (
        "test_transform_oracle.rs",
        "HEAD_ORACLE_EXCLUDED",
        0,
        "no head oracle section is excluded",
    ),
    (
        "test_transform_oracle.rs",
        "SIMPLIFY_ORACLE_EXCLUDED",
        0,
        "no simplify oracle section is excluded",
    ),
    (
        "test_transform_oracle.rs",
        "RTRIP_REFERENCE_GAPS_LICENSED",
        5,
        "five fixtures, 51 rows, where C's own recorded inverse does not \
         recover the pixel either: tnx-cheb.head and its dump (all 24 rows \
         each -- a ChebyMap has no usable inverse), tsc.head and its dump \
         (row 10, a 360 deg alias) and total_hi.head (row 3, an all-sky \
         corner). \
         Asserted as an exact set against the corpus with no fixture loaded, \
         and reached only after the live result has failed both the direct \
         and the aliased comparison",
    ),
    (
        "test_transform_oracle.rs",
        "FMA_SUBSTITUTIONS_LICENSED",
        0,
        "empty: holds oracle rows a generator host's compiler produced by \
         fusing a multiply-add, replaced by the unfused value C's source \
         prescribes and asserted exactly; a test fails if a recorded value \
         ever stops differing from its replacement",
    ),
    (
        "test_simplify_regression.rs",
        "NUMERIC_PLATFORM_DRIFT",
        4,
        "the two sky half-widths of \
         `cap_interval_01`'s uncertainty Box, and the two sky coordinates of \
         `cap_pointlist_01`'s re-centred uncertainty corner, whose references \
         were captured on arm64: an x86-64 build of the pinned C does not \
         reproduce their values, the port reproduces its own host's capture to \
         the last digit, and the corpus-freshness rule keeps a cross-platform \
         reference rather than replacing it. Each entry names the reference's \
         value exactly and bounds how far the port's own value may sit from \
         it, so each licence covers one line on one fixture on whichever host \
         builds it",
    ),
    (
        "test_simplify_regression.rs",
        "SERIAL_UNGATED",
        0,
        "no fixture opts out of the byte comparison: every simplified dump is \
         compared against C's `.simp` in full, and a difference fails",
    ),
    (
        "test_simplify_regression.rs",
        "ISSIMP_DISAGREEMENTS",
        3,
        "fixtures whose `IsSimp` card count differs from C's. Each differs by \
         one card C wrote inside a merge probe it then abandoned, which \
         survives on a sub-Mapping the probe shares with the live tree \
         (docs/issues/c-library-quirks.md). The byte comparison licenses all \
         three through ABANDONED_PROBE_STAMP_LICENSED, so the count is their \
         only gate — which is why the set is pinned on its own",
    ),
    (
        "test_sky_convert.rs",
        "SINCOS_MERGE_LICENSED",
        3,
        "AZEL results up to five units in the last place from C: Apple clang merges the \
         sin/cos pairs of slamap.c De2h into __sincos_stret, which an unoptimised \
         Rust build does not. A release build matches C exactly; the companion \
         test asserts both",
    ),
    (
        "test_frame_in_map_slot.rs",
        "FRAME_IN_MAP_SLOT_DECLINED",
        1,
        "one C-written dump the reader refuses: a bare Frame in a Map<n> slot. \
         Declined by decision (2026-10-13): a Frame does not act as a Mapping \
         in this port. Question, decision and artifact in \
         docs/issues/frame-in-a-mapping-slot.md",
    ),
    (
        "test_deferred_fixtures.rs",
        "UNPORTED_CLASSES",
        0,
        "empty: every class a fixture names has a loader. Its one row was \
         StcResourceProfile, which landed on 2026-10-09",
    ),
    (
        "test_transform_oracle.rs",
        "DEFERRED_LOAD",
        1,
        "a fixture the reader cannot build, so the oracle's sections for it \
         cannot be checked: a Frame in a Mapping slot, which the Mapping \
         loader refuses. Not a tolerance. Gated by \
         deferred_fixtures_still_fail_to_load, which fails when it starts \
         loading",
    ),
    (
        "test_simplify_regression.rs",
        "C_DEFECT_EXCLUDED",
        0,
        "empty. Its one entry was cheby_linear_lone_reduce, whose reference \
         recorded a C defect: PolyMap's linear MapMerge reduction read a \
         ChebyMap's coefficients without the normalisation that maps its input \
         onto [-1,1], so C returned a Mapping that disagreed with the one it \
         replaced. Upstream folded the normalisation in and the port ported it, \
         so the row is compared in full. The table survives so a newly diagnosed \
         C-side artefact has a home. Gated by c_defect_rows_still_diverge",
    ),
    (
        "test_frame_load.rs",
        "FRAME_LOAD_EXCLUDED",
        0,
        "empty: every hand-written Frame description loads as C loads it. \
         The table survives so a newly found difference has a home. Gated by \
         excluded_descriptions_still_differ",
    ),
    (
        "test_serialisation.rs",
        "ROUND_TRIP_EXCLUDED",
        1,
        "one port gap: a Frame in a Mapping slot, which the Mapping loader has \
         no case for. Gated by round_trip_exclusions_still_fail",
    ),
    (
        "test_map_split_oracle.rs",
        "ISSIMP_COMPONENT_EXCLUDED",
        2,
        "two full permuted splits whose simplified PermMap-and-Mapping CmpMap \
         C dumps with one IsSimp card more, on a component; compared with \
         IsSimp lines set aside, every other line byte for byte. Why C stamps \
         the component is not established. Gated by \
         issimp_component_exclusions_still_differ",
    ),
];

/// Relaxations that are not a `const` at all, counted from their manifest.
///
/// Fields: description, expected count, reason.
fn manifest_relaxations() -> Vec<(&'static str, usize, &'static str)> {
    let simplify = std::fs::read_to_string(fixtures().join("simplify/cases.txt"))
        .expect("simplify manifest readable");
    let wcsconv = std::fs::read_to_string(fixtures().join("wcsconv/cases.txt"))
        .expect("wcsconv manifest readable");

    let rows = |t: &str| -> Vec<Vec<String>> {
        t.lines()
            .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
            .map(|l| l.split('|').map(|f| f.trim().to_string()).collect())
            .collect()
    };

    let simp_rows = rows(&simplify);
    let skip_string_compare = simp_rows
        .iter()
        .filter(|f| f.len() > 3 && f[3] == "yes")
        .count();
    // A negative fixture names the same file as input and reference, so its
    // reference cannot carry the IsSimp card `simplify` always stamps.
    let negative_fixtures = simp_rows
        .iter()
        .filter(|f| f.len() > 2 && !f[1].is_empty() && f[1] == f[2])
        .count();
    let simp_astequal = simp_rows
        .iter()
        .filter(|f| f.len() > 3 && f[3] == "astequal")
        .count();
    let astequal = rows(&wcsconv)
        .iter()
        .filter(|f| f.iter().any(|x| x == "astequal"))
        .count();

    vec![
        (
            "simplify manifest: skip_string_compare",
            skip_string_compare,
            "upstream's own column, shared with its harness. All but two of \
             these rows are negative fixtures, whose reference is the \
             unsimplified input and so can never carry the IsSimp card \
             astSimplify stamps; upstream gives up the whole dump for them and \
             gates them in a separate _noop family, while this harness compares \
             them inline with IsSimp stripped. The two rows with a distinct \
             reference, cap_interval_01 and cap_pointlist_01, are still compared \
             here as well, because NUMERIC_PLATFORM_DRIFT can license the one \
             drifting card each rather than the whole file",
        ),
        (
            "simplify manifest: negative fixtures stripping IsSimp",
            negative_fixtures,
            "structural, not a port gap: the reference *is* the input, which \
             was never simplified. Guarded by requiring the result to carry \
             at least one card",
        ),
        (
            "simplify manifest: astequal compare mode",
            simp_astequal,
            "upstream's own column: two rows. cap_timemap_02, whose uncertainty \
             coordinates move by about 7e-12 relative under FMA contraction, and \
             offset_skyref_integrity, whose MatrixMap is MtrMult's product of two \
             rotations with off-diagonal elements that nearly cancel, so a fused \
             dot product moves them by some 50 ULP. Each dump is compared within \
             astEQUAL rather than byte for byte. The comparator honours the \
             column so the two harnesses cannot diverge on what a row asks for",
        ),
        (
            "wcsconv manifest: astequal compare mode",
            astequal,
            "FMA contraction / libm differences against x86-64 Linux captures \
             that C AST on arm64 does not reproduce either",
        ),
    ]
}

/// Count the entries in a `const NAME: &[...] = &[ ... ];` slice literal.
///
/// Top-level items are counted whatever their shape, so a flat `&[&str]` list
/// counts the same as a list of tuples.
fn count_entries(src: &str, name: &str) -> Option<usize> {
    let decl = src.find(&format!("const {name}"))?;
    // The `=` ends the type annotation, and the slice literal opens after it.
    // Located in two steps rather than by matching `= &[`, because rustfmt puts
    // a newline between the two whenever the declaration is long enough.
    let eq = src[decl..].find('=')? + decl;
    let open = src[eq..].find("&[")? + eq + 1;
    let bytes = src.as_bytes();
    let (mut depth, mut i) = (0usize, open);
    let mut items = 0usize;
    let mut item_started = false;
    let mut in_str = false;
    let mut prev_escape = false;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            if c == '\\' && !prev_escape {
                prev_escape = true;
            } else {
                if c == '"' && !prev_escape {
                    in_str = false;
                }
                prev_escape = false;
            }
        } else if c == '/' && bytes.get(i + 1) == Some(&b'/') {
            // A `//` comment runs to end of line and is not item content. The
            // per-entry justifications are exactly where a comma or a bracket
            // in prose would otherwise be read as structure.
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        } else {
            match c {
                '"' => {
                    in_str = true;
                    item_started = true;
                }
                '[' | '(' => {
                    depth += 1;
                    if depth > 1 {
                        item_started = true;
                    }
                }
                ']' | ')' => {
                    depth -= 1;
                    if depth == 0 {
                        if item_started {
                            items += 1;
                        }
                        return Some(items);
                    }
                }
                ',' if depth == 1 => {
                    if item_started {
                        items += 1;
                    }
                    item_started = false;
                }
                other if !other.is_whitespace() => item_started = true,
                _ => {}
            }
        }
        i += 1;
    }
    None
}

#[test]
fn every_relaxation_is_the_size_the_ledger_says() {
    let mut wrong = Vec::new();
    for (file, name, want, _reason) in LEDGER {
        let src = std::fs::read_to_string(tests_dir().join(file))
            .unwrap_or_else(|e| panic!("{file}: {e}"));
        match count_entries(&src, name) {
            Some(got) if got == *want => {}
            Some(got) => wrong.push(format!("{file}::{name}: {got} entries, ledger says {want}")),
            None => wrong.push(format!(
                "{file}::{name}: not found — was it renamed or removed?"
            )),
        }
    }
    for (what, got, _reason) in manifest_relaxations() {
        let want = match what {
            "simplify manifest: skip_string_compare" => 156,
            "simplify manifest: negative fixtures stripping IsSimp" => 158,
            "simplify manifest: astequal compare mode" => 2,
            "wcsconv manifest: astequal compare mode" => 12,
            other => panic!("unledgered manifest relaxation: {other}"),
        };
        if got != want {
            wrong.push(format!("{what}: {got}, ledger says {want}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "the verification ledger is out of date. Every line below is a \
         relaxation that changed size without the ledger changing with it. \
         Update this table *and* PLAN.md §6 together, with a reason:\n  {}",
        wrong.join("\n  ")
    );
}

/// Every test source the scan reads, with the name a `LEDGER` entry uses for
/// it: this crate's top-level tests by file name, and the transform oracle
/// tests of `ast-oracle-tests` and `ast-bench` by their path from here.
fn scanned_test_files() -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for (dir, prefix) in [
        (tests_dir(), ""),
        (
            tests_dir().join("../../ast-oracle-tests/tests"),
            "../../ast-oracle-tests/tests/",
        ),
        (
            tests_dir().join("../../ast-bench/tests"),
            "../../ast-bench/tests/",
        ),
    ] {
        for entry in std::fs::read_dir(&dir).expect("tests dir readable") {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let name = path.file_name().unwrap().to_str().unwrap();
            out.push((path.clone(), format!("{prefix}{name}")));
        }
    }
    out
}

#[test]
fn no_relaxation_hides_outside_the_ledger() {
    let listed: BTreeMap<(&str, &str), ()> =
        LEDGER.iter().map(|(f, n, _, _)| ((*f, *n), ())).collect();
    // Names that read as a bypass. A `const` matching one of these must be in
    // the ledger, which is where its reason gets written down.
    // Matched as substrings, so a bare `EXCLUDED` counts as well as a
    // `NAME_EXCLUDED`.
    const MARKERS: &[&str] = &[
        "EXCLUDED",
        "LICENSED",
        "SKIPPED",
        "IGNORED",
        "DEFERRED",
        "RELAXED",
        "ALLOWED",
        "UNGATED",
        "DISAGREEMENTS",
        "DRIFT",
        "DECLINED",
        "UNTRANSFORMABLE",
    ];
    let mut unlisted = Vec::new();
    for (path, file) in scanned_test_files() {
        let src = std::fs::read_to_string(&path).expect("test source readable");
        for line in src.lines() {
            let Some(rest) = line.trim_start().strip_prefix("const ") else {
                continue;
            };
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                .collect();
            if name.is_empty() || !MARKERS.iter().any(|m| name.contains(m)) {
                continue;
            }
            if !listed.contains_key(&(file.as_str(), name.as_str())) {
                unlisted.push(format!("{file}::{name}"));
            }
        }
    }
    assert!(
        unlisted.is_empty(),
        "these look like gate relaxations and are not in the ledger. Add each \
         to LEDGER with its size and the reason it exists, and mirror it in \
         PLAN.md §6:\n  {}",
        unlisted.join("\n  ")
    );
}
