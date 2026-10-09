//! The one place that accounts for the simplify corpus's unported classes.
//!
//! Upstream and the port share one fixture tree and one manifest, so a fixture
//! for a class the port has no loader for cannot be staged out of reach -- it
//! sits in `tests/fixtures/simplify/` beside every other fixture and is named in
//! each consumer's exclusion list instead:
//!
//! * `test_simplify_regression::DEFERRED` names the manifest rows.
//! * `test_serialisation::ROUND_TRIP_EXCLUDED` names the files.
//!
//! Each of those has a companion test that fails when one of its entries starts
//! working, so a class landing cannot leave its coverage unclaimed. What neither
//! shows is the size of the surface, which is what this test reports: how many
//! fixtures hold an unported class, and which class. A new fixture for an
//! unported class fails here with the class named, rather than in two other
//! binaries as a parse error.

use crate::common::from_native_string;

use std::path::PathBuf;

/// `(class, number of fixtures naming it)`.
///
/// When a class lands, its fixtures start loading, `unported_classes_still_fail`
/// fails, and the row is deleted along with the matching entries in the two
/// exclusion lists.
const UNPORTED_CLASSES: &[(&str, usize)] = &[("StcResourceProfile", 2)];

fn simplify_dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/simplify"
    ))
}

/// Every fixture in `simplify/`, paired with the unported class it names, if any.
fn fixtures_by_unported_class() -> Vec<(String, &'static str)> {
    let dir = simplify_dir();
    assert!(dir.is_dir(), "{} is not a directory", dir.display());
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "map" && ext != "simp" {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        if let Some((class, _)) = UNPORTED_CLASSES
            .iter()
            .find(|(class, _)| text.contains(&format!("Begin {class}")))
        {
            out.push((
                path.file_name().unwrap().to_string_lossy().into_owned(),
                *class,
            ));
        }
    }
    out.sort();
    out
}

#[test]
fn unported_class_counts_are_accounted_for() {
    let found = fixtures_by_unported_class();
    for (class, expected) in UNPORTED_CLASSES {
        let n = found.iter().filter(|(_, c)| c == class).count();
        assert_eq!(
            n, *expected,
            "{n} simplify fixtures name {class}, UNPORTED_CLASSES says {expected}. \
             A fixture added for an unported class must be excluded in \
             test_simplify_regression::DEFERRED and \
             test_serialisation::ROUND_TRIP_EXCLUDED as well as counted here."
        );
    }
    println!(
        "deferred simplify fixtures: {} files across {} unported classes",
        found.len(),
        UNPORTED_CLASSES.len()
    );
    for (class, n) in UNPORTED_CLASSES {
        println!("  {class} unported — {n} fixtures");
    }
}

#[test]
fn unported_classes_still_fail_to_load() {
    let dir = simplify_dir();
    let mut now_loadable = Vec::new();
    for (name, class) in fixtures_by_unported_class() {
        let text = std::fs::read_to_string(dir.join(&name)).unwrap();
        if from_native_string(&text).is_ok() {
            now_loadable.push(format!("{name} ({class})"));
        }
    }
    assert!(
        now_loadable.is_empty(),
        "these fixtures now load, so their class has landed: delete its \
         UNPORTED_CLASSES row and the matching entries in \
         test_simplify_regression::DEFERRED and \
         test_serialisation::ROUND_TRIP_EXCLUDED: {}",
        now_loadable.join(", ")
    );
}
