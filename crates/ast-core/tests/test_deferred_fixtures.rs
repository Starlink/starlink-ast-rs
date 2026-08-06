//! Registry for simplify fixtures staged out of reach because they exercise
//! classes the port has not implemented.
//!
//! The fixtures live in
//! `tests/fixtures/native/reference/simplify_fixtures_unsupported/<class>/`,
//! and both fixture consumers skip that directory: the manifest-driven
//! `test_simplify_regression` has no rows for them, and the auto-discovering
//! byte-exact round-trips read `simplify_fixtures/` non-recursively.
//!
//! Staging them is deliberate. Staging them silently is not — a green summary
//! line would otherwise hide two dozen C-generated fixtures. This test prints
//! the count on every run and fails if a subdirectory appears that is not
//! registered below.

use std::path::PathBuf;

/// `(subdirectory, unported class, file count)`.
///
/// When a class lands, follow the re-enabling procedure in the directory's
/// `README.md` and delete its row.
const DEFERRED_CLASSES: &[(&str, &str, usize)] = &[
    ("intramap", "IntraMap", 13),
    ("stcresourceprofile", "StcResourceProfile", 2),
    ("xphmap", "XphMap", 9),
];

fn deferred_root() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/native/reference/simplify_fixtures_unsupported"
    ))
}

#[test]
fn deferred_fixtures_are_all_registered() {
    let root = deferred_root();
    assert!(root.is_dir(), "{} is not a directory", root.display());

    let mut found: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();

    let mut registered: Vec<String> = DEFERRED_CLASSES
        .iter()
        .map(|(d, _, _)| (*d).to_owned())
        .collect();
    registered.sort();

    assert_eq!(
        found, registered,
        "unregistered or stale deferred-fixture subdirectory: add or remove \
         a DEFERRED_CLASSES row in tests/test_deferred_fixtures.rs"
    );

    let mut total = 0usize;
    for (dir, class, expected) in DEFERRED_CLASSES {
        let n = std::fs::read_dir(root.join(dir))
            .unwrap()
            .filter(|e| e.as_ref().unwrap().path().is_file())
            .count();
        assert_eq!(
            n, *expected,
            "{dir}/ ({class}) holds {n} files, DEFERRED_CLASSES says {expected}"
        );
        total += n;
    }

    println!(
        "deferred simplify fixtures: {total} files across {} unported classes",
        DEFERRED_CLASSES.len()
    );
    for (dir, class, n) in DEFERRED_CLASSES {
        println!("  {dir}/ — {class} unported — {n} files");
    }
}
