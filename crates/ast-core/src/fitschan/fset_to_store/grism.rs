//! Port of `fitschan.c::ExtractGrismMap` (~10161).
//!
//! Walks a Mapping chain looking for a `GrismMap` that produces a
//! specified output coordinate directly (i.e. its output is not
//! subsequently modified by any other Mapping). When found, returns
//! a clone of the GrismMap plus a new chain in which the GrismMap
//! is replaced by a `UnitMap` — so the caller can test the
//! GrismMap-free chain for linearity in pixel coords.
//!
//! Used by `spectral_axes` to detect FITS-WCS paper III's `-GRI` /
//! `-GRA` (grism) algorithm encoding.

#![allow(dead_code)]

use crate::error::AstResult;
use crate::mappings::{CmpMap, Composition, DynMapping, GrismMap, Mapping, UnitMap};

/// Try to extract a GrismMap from the end of `map` for the
/// specified output axis `iax` (0-based).
///
/// On success, returns `Some((grism_clone, replacement_chain))`
/// where `replacement_chain` is a copy of `map` with the GrismMap
/// substituted by a `UnitMap(1)`. On failure (no GrismMap found at
/// the end of the chain for output `iax`, or the GrismMap is
/// inverted, or the structure isn't a leaf-or-CmpMap), returns
/// `None`.
///
/// Mirrors `fitschan.c::ExtractGrismMap`.
pub(crate) fn extract_grism_map(
    map: &dyn Mapping,
    iax: usize,
) -> AstResult<Option<(GrismMap, DynMapping)>> {
    // Case 1: leaf GrismMap (un-inverted). Mirrors C:10236-10240.
    if let Some(g) = map.as_any().downcast_ref::<GrismMap>() {
        if !g.is_inverted() {
            return Ok(Some((g.clone(), Box::new(UnitMap::new(1)))));
        }
        return Ok(None);
    }

    // Case 2: CmpMap. Recurse into the child that produces output
    // iax. Mirrors C:10246-10293.
    if let Some(cmp) = map.as_any().downcast_ref::<CmpMap>() {
        // We don't handle a CmpMap whose top-level is inverted —
        // that would require inverting the structure first. C also
        // doesn't handle this case (its astSetInvert dance restores
        // the construction-time orientation, but the outer Mapping
        // is consulted in its current state). For our writer
        // pipeline the chain is always built up forward, so this
        // restriction is fine.
        if cmp.is_inverted() {
            return Ok(None);
        }

        // CmpMap composes its children using the `inv_a`/`inv_b`
        // snapshots captured at construction time, so a child that
        // has been independently inverted post-construction still
        // walks in its captured direction. Extract effective clones
        // of each child whose `is_inverted()` matches the snapshot —
        // otherwise the recursion sees the wrong nout (parallel
        // axis split) or the wrong "tail" mapping (series).
        let mapa = extract_child_effective(cmp, true);
        let mapb = extract_child_effective(cmp, false);
        let composition = cmp.composition();

        match composition {
            Composition::Series => {
                // Series: only the *second* child contributes the
                // GrismMap "at the end". The first child is left
                // untouched.
                if let Some((g, new_mapb)) = extract_grism_map(mapb.as_ref(), iax)? {
                    let new_chain: DynMapping =
                        Box::new(CmpMap::new(mapa, new_mapb, Composition::Series)?);
                    return Ok(Some((g, new_chain)));
                }
            }
            Composition::Parallel => {
                // Parallel: the child producing output `iax` is the
                // candidate. `mapa` contributes outputs
                // 0..mapa.nout(); `mapb` contributes outputs
                // mapa.nout()..total. We use the effective-clone
                // `nout()` (post-inversion) so the axis split
                // matches the CmpMap's runtime layout.
                let na = mapa.nout();
                if iax < na {
                    if let Some((g, new_mapa)) = extract_grism_map(mapa.as_ref(), iax)? {
                        let new_chain: DynMapping =
                            Box::new(CmpMap::new(new_mapa, mapb, Composition::Parallel)?);
                        return Ok(Some((g, new_chain)));
                    }
                } else if let Some((g, new_mapb)) = extract_grism_map(mapb.as_ref(), iax - na)? {
                    let new_chain: DynMapping =
                        Box::new(CmpMap::new(mapa, new_mapb, Composition::Parallel)?);
                    return Ok(Some((g, new_chain)));
                }
            }
        }
    }

    Ok(None)
}

/// Return a clone of the requested child (`first=true` for
/// `cmp.first()`, false for `cmp.second()`) with its `inverted` flag
/// normalized to the snapshot captured when the CmpMap was
/// constructed. CmpMap's runtime transform composes children using
/// those snapshots, so a child that was independently inverted
/// post-construction still walks in its captured direction. Walks
/// that recurse into children must mirror the same convention to
/// keep dimensions and orientation aligned with the CmpMap's
/// effective transform.
///
/// Duplicated from `super::celestial_axes::extract_child_effective`;
/// the helper would belong in a shared module if grism.rs and
/// celestial_axes.rs were both retained long-term.
fn extract_child_effective(cmp: &CmpMap, first: bool) -> DynMapping {
    let (child, captured_inv) = if first {
        (cmp.first(), cmp.inv_a())
    } else {
        (cmp.second(), cmp.inv_b())
    };
    let mut clone = child.clone_box();
    if clone.is_inverted() != captured_inv {
        clone.invert();
    }
    clone
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mappings::{ShiftMap, UnitMap};

    fn make_grism() -> GrismMap {
        // Default GrismMap parameters give a non-trivial mapping.
        GrismMap::default_map()
    }

    #[test]
    fn leaf_grism_extracted() {
        let g = make_grism();
        let (extracted, replacement) = extract_grism_map(&g, 0).unwrap().unwrap();
        // The replacement chain should be a UnitMap (1 → 1).
        assert!(replacement.as_any().is::<UnitMap>());
        assert_eq!(replacement.nin(), 1);
        assert_eq!(replacement.nout(), 1);
        // The extracted GrismMap shares parameters with the input.
        assert_eq!(extracted.grism_g(), g.grism_g());
    }

    #[test]
    fn inverted_grism_not_extracted() {
        let mut g = make_grism();
        g.invert();
        assert!(extract_grism_map(&g, 0).unwrap().is_none());
    }

    #[test]
    fn shift_then_grism_in_series_extracts() {
        // Series: ShiftMap → GrismMap. The GrismMap is at the end ⇒
        // extractable. The replacement chain becomes ShiftMap → UnitMap.
        let shift = ShiftMap::new(vec![3.5]);
        let g = make_grism();
        let chain = CmpMap::new(Box::new(shift), Box::new(g.clone()), Composition::Series).unwrap();

        let (extracted, replacement) = extract_grism_map(&chain, 0).unwrap().unwrap();
        assert_eq!(extracted.grism_g(), g.grism_g());
        // Replacement chain is still 1→1.
        assert_eq!(replacement.nin(), 1);
        assert_eq!(replacement.nout(), 1);
    }

    #[test]
    fn grism_then_shift_in_series_does_not_extract() {
        // Series: GrismMap → ShiftMap. The GrismMap output IS modified
        // by the trailing shift, so extraction must fail. Mirrors the
        // "GrismMap must be at the end" requirement.
        let g = make_grism();
        let shift = ShiftMap::new(vec![3.5]);
        let chain = CmpMap::new(Box::new(g), Box::new(shift), Composition::Series).unwrap();

        assert!(extract_grism_map(&chain, 0).unwrap().is_none());
    }

    #[test]
    fn parallel_grism_on_axis_one_extracts() {
        // Parallel: UnitMap (axis 0) || GrismMap (axis 1). Asking for
        // output 1 returns the GrismMap; the replacement is
        // UnitMap || UnitMap.
        let unit = UnitMap::new(1);
        let g = make_grism();
        let chain =
            CmpMap::new(Box::new(unit), Box::new(g.clone()), Composition::Parallel).unwrap();

        let (extracted, replacement) = extract_grism_map(&chain, 1).unwrap().unwrap();
        assert_eq!(extracted.grism_g(), g.grism_g());
        assert_eq!(replacement.nin(), 2);
        assert_eq!(replacement.nout(), 2);
    }

    #[test]
    fn parallel_grism_wrong_axis_does_not_extract() {
        // Asking for output 0 from UnitMap || GrismMap returns None
        // (UnitMap doesn't carry a GrismMap).
        let unit = UnitMap::new(1);
        let g = make_grism();
        let chain = CmpMap::new(Box::new(unit), Box::new(g), Composition::Parallel).unwrap();

        assert!(extract_grism_map(&chain, 0).unwrap().is_none());
    }

    #[test]
    fn no_grism_anywhere_returns_none() {
        let chain = CmpMap::new(
            Box::new(ShiftMap::new(vec![1.0])),
            Box::new(UnitMap::new(1)),
            Composition::Series,
        )
        .unwrap();
        assert!(extract_grism_map(&chain, 0).unwrap().is_none());
    }
}
