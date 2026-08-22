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
        "test_wcsconv.rs",
        "ISSIMP_PLACEMENT_LICENSED",
        4,
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
        "ISSIMP_PLACEMENT_LICENSED",
        2,
        "cap_cmpmap_03 and cap_specmap_02 emit one IsSimp card too few; the \
         Mapping is identical and six fixes were measured and reverted",
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
        "test_deferred_fixtures.rs",
        "DEFERRED_CLASSES",
        3,
        "IntraMap, XphMap and StcResourceProfile are unported classes, not \
         relaxations of a gate that could otherwise pass",
    ),
];

/// Relaxations that are not a `const` at all, counted from their manifest.
///
/// Fields: description, expected count, reason.
fn manifest_relaxations() -> Vec<(&'static str, usize, &'static str)> {
    let simplify = std::fs::read_to_string(fixtures().join("native/reference/simplify_tests.txt"))
        .expect("simplify manifest readable");
    let wcsconv = std::fs::read_to_string(fixtures().join("wcsconv/wcsconv_tests.txt"))
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
    let astequal = rows(&wcsconv)
        .iter()
        .filter(|f| f.iter().any(|x| x == "astequal"))
        .count();

    vec![
        (
            "simplify manifest: skip_string_compare",
            skip_string_compare,
            "rigby (an IsSimp count difference plus a near-zero inverse-matrix \
             element differing by ~1%), cap_cmpmap_01 and cap_cmpmap_04 (the \
             open InvA/InvB quirk), cap_winmap_03 (a sub-ulp shift residue C \
             emits and we compute as zero). lsst retired its skip once the \
             manifest could name the Channel settings its reference was \
             captured with",
        ),
        (
            "simplify manifest: negative fixtures stripping IsSimp",
            negative_fixtures,
            "structural, not a port gap: the reference *is* the input, which \
             was never simplified. Guarded by requiring the result to carry \
             at least one card",
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
fn count_entries(src: &str, name: &str) -> Option<usize> {
    let decl = src.find(&format!("const {name}"))?;
    let open = src[decl..].find("= &[")? + decl + 3;
    let bytes = src.as_bytes();
    let (mut depth, mut i) = (0usize, open);
    let mut top_level_items = 0usize;
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
            // A `//` comment runs to end of line. Without this, a parenthesis
            // in a per-entry justification counts as an entry — the reason
            // strings above are exactly where parentheses belong.
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        } else {
            match c {
                '"' => in_str = true,
                '[' | '(' => {
                    depth += 1;
                    if depth == 2 {
                        top_level_items += 1;
                    }
                }
                ']' | ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(top_level_items);
                    }
                }
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
            "simplify manifest: skip_string_compare" => 4,
            "simplify manifest: negative fixtures stripping IsSimp" => 134,
            "wcsconv manifest: astequal compare mode" => 8,
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

#[test]
fn no_relaxation_hides_outside_the_ledger() {
    let listed: BTreeMap<(&str, &str), ()> =
        LEDGER.iter().map(|(f, n, _, _)| ((*f, *n), ())).collect();
    // Names that read as a bypass. A `const` matching one of these must be in
    // the ledger, which is where its reason gets written down.
    const MARKERS: &[&str] = &[
        "_EXCLUDED",
        "_LICENSED",
        "_SKIPPED",
        "_IGNORED",
        "_DEFERRED",
        "_RELAXED",
        "_ALLOWED",
        "DEFERRED_",
    ];
    let mut unlisted = Vec::new();
    for entry in std::fs::read_dir(tests_dir()).expect("tests dir readable") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let file = path.file_name().unwrap().to_str().unwrap().to_string();
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
