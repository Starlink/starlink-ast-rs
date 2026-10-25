// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The fixture subset the published archive carries.
//!
//! `tests/fixtures/DIST_MANIFEST` names the fixtures the C library's default
//! `make dist` ships: enough for someone building the library to see that it
//! works, without the simplification and serialization corpora that exist to
//! catch library defects. The archive `cargo package` builds carries the same
//! subset through the `include` list in `Cargo.toml`, and `build.rs` tells the
//! two trees apart by `tests/DIST_TIER`, committed but left out of the archive
//! (`declare_dist_tier` there).
//!
//! These are the checks the C dist hook makes as it cuts the tarball
//! (`ast_tester/Makefile.am`, `dist-fixtures.sh`) and the CMake suite makes
//! when it configures from one, run on whichever tree this is: every manifest
//! entry names a file that exists, the `include` list is the manifest entry
//! for entry, a shipped oracle names no fixture that is not shipped, and a
//! cut-down tree holds nothing the manifest does not name.

use std::path::{Path, PathBuf};

const CUT_DOWN: bool = cfg!(ast_dist_tier = "cutdown");

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixtures() -> PathBuf {
    crate_dir().join("tests/fixtures")
}

/// The manifest's entries: `#` starts a comment, blank lines are dropped.
fn manifest_patterns() -> Vec<String> {
    let path = fixtures().join("DIST_MANIFEST");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .inspect(|l| {
            // The shell that cuts the tarball expands an entry as a glob,
            // while the CMake suite and this file treat only `*` specially.
            // Keeping entries to `*` is what keeps the three readings equal.
            assert!(
                !l.contains(['?', '[', ']']),
                "DIST_MANIFEST: only `*` is a wildcard in {l:?}"
            );
        })
        .map(String::from)
        .collect()
}

/// Whether `rel`, a `/`-separated path below `fixtures/`, is one a manifest
/// entry names. An entry ending in `/` names everything beneath that
/// directory; any other entry is a glob in which `*` does not cross a `/`.
fn covered_by(pattern: &str, rel: &str) -> bool {
    if pattern.ends_with('/') {
        rel.starts_with(pattern)
    } else {
        glob_matches(pattern, rel)
    }
}

fn glob_matches(pattern: &str, s: &str) -> bool {
    let Some(star) = pattern.find('*') else {
        return pattern == s;
    };
    let (literal, rest) = (&pattern[..star], &pattern[star + 1..]);
    let Some(s) = s.strip_prefix(literal) else {
        return false;
    };
    let within_component = s.find('/').unwrap_or(s.len());
    (0..=within_component)
        .filter(|&n| s.is_char_boundary(n))
        .any(|n| glob_matches(rest, &s[n..]))
}

/// Every file below `fixtures/`, as a `/`-separated path relative to it,
/// dotfiles left out as the dist hook leaves them out.
fn fixture_files() -> Vec<String> {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let rel = if rel.is_empty() {
                name
            } else {
                format!("{rel}/{name}")
            };
            if entry.file_type().unwrap().is_dir() {
                walk(&entry.path(), &rel, out);
            } else {
                out.push(rel);
            }
        }
    }
    let mut out = Vec::new();
    walk(&fixtures(), "", &mut out);
    out.sort();
    out
}

/// `dist-fixtures.sh`: an entry that names nothing means the manifest no
/// longer describes the tree, and shipping fewer fixtures than intended is
/// the failure it exists to prevent. In a cut-down tree the same check says
/// the archive left out a fixture the manifest ships.
#[test]
fn every_manifest_entry_names_a_file() {
    let files = fixture_files();
    let unmatched: Vec<String> = manifest_patterns()
        .into_iter()
        .filter(|p| !files.iter().any(|f| covered_by(p, f)))
        .collect();
    assert!(
        unmatched.is_empty(),
        "DIST_MANIFEST: {} nothing: {unmatched:?}",
        if CUT_DOWN {
            "the archive left out what these entries name; they match"
        } else {
            "these entries match"
        }
    );
}

/// The archive carries what the manifest names and nothing else. Only a
/// cut-down tree can show this, so a run from the repository reports it as
/// ignored; CI's `package` job runs it from the archive.
#[test]
#[cfg_attr(
    ast_dist_tier = "full",
    ignore = "checks a cut-down tree; run from the archive"
)]
fn cut_down_tree_holds_only_what_the_manifest_names() {
    let patterns = manifest_patterns();
    let extra: Vec<String> = fixture_files()
        .into_iter()
        .filter(|f| !patterns.iter().any(|p| covered_by(p, f)))
        .collect();
    assert!(
        extra.is_empty(),
        "fixtures the archive carries that DIST_MANIFEST does not name: {extra:?}"
    );
}

/// The `include` list in `Cargo.toml` is where the manifest takes effect, so
/// its fixture entries must be the manifest entry for entry: a directory
/// entry `dir/` as `/tests/fixtures/dir/**`, anything else as
/// `/tests/fixtures/<entry>`. The two negations are what keep the rest of
/// the tree and the tier marker out.
#[test]
fn include_list_is_the_manifest() {
    // The archive's Cargo.toml is cargo's normalized rewrite; the authored
    // one travels beside it.
    let authored = crate_dir().join("Cargo.toml.orig");
    let manifest = if authored.is_file() {
        authored
    } else {
        crate_dir().join("Cargo.toml")
    };
    let text = std::fs::read_to_string(&manifest).unwrap();
    let start = text
        .find("\ninclude = [")
        .expect("Cargo.toml has an include list")
        + 1;
    let end = start + text[start..].find("\n]\n").expect("include list closes");
    let entries: Vec<&str> = text[start..end]
        .lines()
        .skip(1)
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(|l| {
            l.strip_prefix('"')
                .and_then(|l| l.strip_suffix("\","))
                .unwrap_or_else(|| panic!("include entry {l:?} is not a quoted string"))
        })
        .collect();

    for required in ["!/tests/DIST_TIER", "!/tests/fixtures/**"] {
        assert!(
            entries.contains(&required),
            "include list lacks {required:?}"
        );
    }
    let from_include: Vec<&str> = entries
        .iter()
        .copied()
        .filter(|e| e.starts_with("/tests/fixtures/"))
        .collect();
    let from_manifest: Vec<String> = manifest_patterns()
        .iter()
        .map(|p| match p.strip_suffix('/') {
            Some(dir) => format!("/tests/fixtures/{dir}/**"),
            None => format!("/tests/fixtures/{p}"),
        })
        .collect();
    assert_eq!(
        from_include, from_manifest,
        "Cargo.toml include (left) is not DIST_MANIFEST (right)"
    );
}

/// A shipped oracle must not name a fixture that was not shipped. Its
/// sections are keyed by fixture path, so this is a text check; the dist
/// hook makes it a failure rather than a filter so that the manifest and the
/// oracle have to be reconciled instead of coverage quietly shrinking.
#[test]
fn shipped_oracles_name_only_shipped_fixtures() {
    let patterns = manifest_patterns();
    let covered = |rel: &str| patterns.iter().any(|p| covered_by(p, rel));
    let mut problems = Vec::new();
    for oracle in fixture_files()
        .into_iter()
        .filter(|f| f.ends_with(".oracle") && covered(f))
    {
        let text = std::fs::read_to_string(fixtures().join(&oracle)).unwrap();
        for section in text.lines().filter_map(|l| l.strip_prefix('[')) {
            let named = section.split([' ', ']']).next().unwrap_or("");
            if !covered(named) {
                problems.push(format!("{oracle} names undistributed fixture {named}"));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
