//! Load an oracle fixture as the transform-oracle gate does
//! (`crates/ast-core/tests/test_transform_oracle.rs`, `load_fixture`).

use std::path::Path;

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::FitsChan;

/// A loaded oracle fixture.
pub enum Loaded {
    /// An object that implements `Mapping`.
    Mapping(Box<AnyObject>),
    /// Resolved and read, but there is no transform to time; the reason.
    Untransformable(&'static str),
}

/// Fixtures the port refuses to load, with the reason; mirrors
/// `DEFERRED_LOAD` in `crates/ast-core/tests/test_transform_oracle.rs`.
pub const DEFERRED_LOAD: &[(&str, &str)] = &[(
    "serialisation/frame_in_map_slot.ast",
    "CmpFrame in a Map<n> slot, refused",
)];

/// Load the fixture `relpath` under `root`: `.head` through a `FitsChan`,
/// everything else as Native text with the corpus's IntraMap transformations
/// registered. A fixture in `DEFERRED_LOAD` that fails to load is
/// untransformable rather than an error.
pub fn load_fixture(root: &Path, relpath: &str) -> Result<Loaded, String> {
    match load_raw(root, relpath) {
        Err(e) => match DEFERRED_LOAD.iter().find(|(f, _)| *f == relpath) {
            Some((_, reason)) => Ok(Loaded::Untransformable(reason)),
            None => Err(e),
        },
        ok => ok,
    }
}

fn load_raw(root: &Path, relpath: &str) -> Result<Loaded, String> {
    let path = root.join(relpath);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read {relpath}: {e}"))?;
    let obj = if relpath.ends_with(".head") {
        let mut fc =
            FitsChan::from_header(&text).map_err(|e| format!("from_header {relpath}: {e}"))?;
        fc.read()
            .map_err(|e| format!("read {relpath}: {e}"))?
            .ok_or_else(|| format!("FitsChan read returned None: {relpath}"))?
    } else {
        ast_core::from_native_string_with(&text, &intra_registry())
            .map_err(|e| format!("from_native_string {relpath}: {e}"))?
    };
    if obj.as_mapping().is_some() {
        return Ok(Loaded::Mapping(Box::new(obj)));
    }
    Ok(Loaded::Untransformable(if obj.as_frame().is_some() {
        "bare Frame: not a Mapping in the Rust port; C treats it as a unit transform"
    } else {
        "not a Mapping in C either"
    }))
}

/// The registry of IntraMap transformations the corpus fixtures name.
pub fn intra_registry() -> ast_core::mappings::IntraRegistry {
    let mut registry = ast_core::mappings::IntraRegistry::new();
    for t in test_intra_transforms() {
        registry
            .register(t)
            .expect("the four test IntraMap names are distinct");
    }
    registry
}

// Mirrors `crates/ast-core/tests/common/mod.rs` (`TestIntraTran`,
// `test_intra_transforms`).
/// The identity transformation the C test programs register for IntraMap
/// fixtures (`TestIntraTran` in `ast_tester/simplify.c`, `roundtrip.c` and
/// `ast_astequal.c`): each output coordinate copies the input coordinate of
/// the same index, bad where there is none.
struct TestIntraTran {
    name: &'static str,
    flags: ast_core::mappings::IntraFlags,
    purpose: &'static str,
}

impl ast_core::mappings::IntraTransform for TestIntraTran {
    fn name(&self) -> &str {
        self.name
    }
    fn nin(&self) -> Option<usize> {
        Some(1)
    }
    fn nout(&self) -> Option<usize> {
        Some(1)
    }
    fn flags(&self) -> ast_core::mappings::IntraFlags {
        self.flags
    }
    fn purpose(&self) -> &str {
        self.purpose
    }
    fn author(&self) -> &str {
        "AST test suite"
    }
    fn contact(&self) -> &str {
        "starlink-ast"
    }
    fn transform(
        &self,
        _map: &ast_core::mappings::IntraMap,
        input: &ast_core::coords::PointSet,
        _forward: bool,
        out: &mut ast_core::coords::PointSet,
    ) -> ast_core::error::AstResult<()> {
        for ip in 0..input.npoint() {
            let row = input.point(ip);
            for (ic, v) in out.point_mut(ip).iter_mut().enumerate() {
                *v = row
                    .get(ic)
                    .copied()
                    .unwrap_or(ast_core::constants::BAD_VALUE);
            }
        }
        Ok(())
    }
}

/// The four transformations the C test programs register for the IntraMap
/// simplify fixtures, with the same names, flags and descriptions.
fn test_intra_transforms() -> Vec<std::sync::Arc<dyn ast_core::mappings::IntraTransform>> {
    use ast_core::mappings::IntraFlags;
    let both = IntraFlags {
        simp_fi: true,
        simp_if: true,
        ..IntraFlags::default()
    };
    [
        (
            "simplifyidentity",
            both,
            "Identity IntraMap for simplify fixtures",
        ),
        (
            "simplifyidentity2",
            both,
            "Second identity IntraMap for simplify fixtures",
        ),
        (
            "nosimpfi",
            IntraFlags {
                simp_if: true,
                ..IntraFlags::default()
            },
            "IntraMap without SIMPFI for simplify fixtures",
        ),
        (
            "nosimpif",
            IntraFlags {
                simp_fi: true,
                ..IntraFlags::default()
            },
            "IntraMap without SIMPIF for simplify fixtures",
        ),
    ]
    .into_iter()
    .map(|(name, flags, purpose)| {
        std::sync::Arc::new(TestIntraTran {
            name,
            flags,
            purpose,
        }) as std::sync::Arc<dyn ast_core::mappings::IntraTransform>
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deferred_fixtures_still_fail_to_load() {
        let root = std::path::Path::new(crate::oracle::CORE_FIXTURES);
        for (file, _) in DEFERRED_LOAD {
            assert!(
                load_raw(root, file).is_err(),
                "{file} now loads; delete its DEFERRED_LOAD row"
            );
        }
    }

    #[test]
    fn every_core_section_loads_or_names_why_not() {
        let root = std::path::Path::new(crate::oracle::CORE_FIXTURES);
        for name in crate::oracle::CORE_ORACLES {
            let text = std::fs::read_to_string(root.join("oracle").join(name)).unwrap();
            for s in crate::oracle::parse_oracle(&text).unwrap() {
                load_fixture(root, &s.relpath).unwrap_or_else(|e| panic!("{}: {e}", s.relpath));
            }
        }
    }
}
