//! Which classes may implement the per-class simplify seam.
//!
//! C overrides the `Simplify` vtab slot in 11 of this port's classes. For every
//! other class all reduction lives in `MapMerge`, and the base `Simplify`
//! (`mapping.c:16752`) reaches it by nominating the Mapping in a list of one
//! (`mapping.c:16871-16883`).
//!
//! The companion rule — that the seam has one caller — is a compile error rather
//! than a test: `Mapping::simplified` takes a `ViaSimplify`, which only
//! `mappings::simplify_entry` can construct.

use std::path::{Path, PathBuf};

/// Port files whose C counterpart overrides the `Simplify` vtab slot.
///
/// Derived, not chosen:
/// `grep -l 'static AstMapping \*Simplify' reference/starlink-ast/src/*.c`
/// returns `box circle cmpframe cmpmap cmpregion ellipse frameset interval
/// mapping nullregion pointlist polygon prism region stc`. `mapping` is the base
/// every other class inherits, and `cmpframe` and `stc` have no port counterpart.
const C_SIMPLIFY_CLASSES: &[&str] = &[
    "mappings/cmpmap.rs",
    "frames/frameset.rs",
    "regions/box_region.rs",
    "regions/circle.rs",
    "regions/cmpregion.rs",
    "regions/ellipse.rs",
    "regions/interval.rs",
    "regions/nullregion.rs",
    "regions/pointlist.rs",
    "regions/polygon.rs",
    "regions/prism.rs",
];

fn src_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
#[ignore = "fails until the 25 stray bodies have moved; see \
            docs/superpowers/plans/2026-08-23-simplify-seam-realignment.md"]
fn only_c_simplify_classes_implement_simplified() {
    let root = src_root();
    let mut stray = Vec::new();
    let mut stack = vec![root.clone()];

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("source dir readable") {
            let path = entry.expect("readable entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("source readable");
            if !text.contains("fn simplified(&self, _gate: ViaSimplify)") {
                continue;
            }
            let rel = path
                .strip_prefix(&root)
                .expect("under src")
                .to_string_lossy()
                .replace('\\', "/");
            // `mappings/mod.rs` declares the method; it does not implement it.
            if rel == "mappings/mod.rs" || C_SIMPLIFY_CLASSES.contains(&rel.as_str()) {
                continue;
            }
            stray.push(rel);
        }
    }

    stray.sort();
    assert!(
        stray.is_empty(),
        "these classes implement `simplified()` and their C counterpart does not \
         override the `Simplify` slot, so the reduction belongs in `map_merge` \
         where the base `Simplify` can reach it: {stray:#?}"
    );
}
