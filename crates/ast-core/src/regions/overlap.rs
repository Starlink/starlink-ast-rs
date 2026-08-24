//! Geometric overlap algorithm for determining how two Regions relate.
//!
//! This module provides the shared [`compute_overlap`] function that is a
//! faithful port of the C library's `region.c` `OverlapX` function
//! (region.c:6996), together with the `NullRegion`-specific table
//! (`nullregion.c:874`) and the `GetSubMesh` helper (region.c:3688).
//! Each concrete Region's `overlap()` method delegates here.

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::FrameRef;
use crate::mappings::helpers::is_bad;
use crate::mappings::{CmpMap, Composition, Mapping};

use super::nullregion::NullRegion;
use super::{Overlap, Region};

/// Map a C `astOverlap` return code (0-6) to the Rust [`Overlap`] enum.
///
/// C code 0 ("the check could not be performed because the second Region
/// could not be mapped into the coordinate system of the first Region")
/// has no distinct variant; it collapses to [`Overlap::None`], matching the
/// enum's inability to represent the un-mappable case.
fn code_to_overlap(code: i32) -> Overlap {
    match code {
        2 => Overlap::FirstInside,
        3 => Overlap::SecondInside,
        4 => Overlap::Partial,
        5 => Overlap::Same,
        6 => Overlap::Negation,
        // 0 (cannot perform) and 1 (no overlap) both map to None.
        _ => Overlap::None,
    }
}

/// Compute the overlap relationship between two Regions.
///
/// `region_a` plays the role of `this` in the public `astOverlap(this, that)`
/// and `region_b` the role of `that`. C's public method swaps the arguments
/// and invokes `OverlapX(that, this)`, dispatching the method on the class of
/// `that` (region.c:6992). We reproduce that dispatch here: when `region_b`
/// (the `that` argument) is a [`NullRegion`], the `NullRegion` override
/// (`nullregion.c:874`) is used; otherwise the generic geometric `OverlapX`
/// (region.c:6996) runs.
pub(crate) fn compute_overlap(region_a: &dyn Region, region_b: &dyn Region) -> AstResult<Overlap> {
    // NullRegion overrides both `Overlap` and `OverlapX` (nullregion.c:317-318),
    // so its special-casing fires when *either* operand is a NullRegion: with
    // `this` a NullRegion, `NullRegion::Overlap` calls its own `OverlapX`; with
    // `that` a NullRegion, the base `Region::Overlap` dispatches `astOverlapX`
    // onto it. `null_overlap` inspects both operands and returns the
    // `astOverlap(this, that)` convention, so the fixed `(region_b, region_a)`
    // argument order yields `astOverlap(region_a, region_b)` in either case.
    if region_a.as_any().is::<NullRegion>() || region_b.as_any().is::<NullRegion>() {
        return Ok(code_to_overlap(null_overlap(region_b, region_a)?));
    }
    // Otherwise the generic geometric OverlapX (region.c:6996) runs, dispatched
    // on the class of `that` (== region_b).
    Ok(code_to_overlap(overlap_x(region_b, region_a)?))
}

/// Faithful port of `NullRegion::OverlapX` (nullregion.c:874).
///
/// Parameters follow the C signature `OverlapX(that, this)`: `that` is the
/// Region the method was dispatched on (always a [`NullRegion`] here), `this`
/// is the other Region. The returned code is expressed in the public
/// `astOverlap(this, that)` convention (2 = `this` inside `that`, etc).
fn null_overlap(that: &dyn Region, this: &dyn Region) -> AstResult<i32> {
    // Check that the Regions can be compared meaningfully (astConvert).
    if this
        .get_region_frame()
        .convert_to(that.get_region_frame().as_ref(), &[])?
        .is_none()
    {
        return Ok(0);
    }

    let this_null = this.as_any().is::<NullRegion>();
    let that_null = that.as_any().is::<NullRegion>();
    let this_neg = this.is_negated();
    let that_neg = that.is_negated();

    let result = if this_null && that_null {
        // Both NullRegions: identical if Negated matches, else mutually
        // exclusive.
        if this_neg == that_neg { 5 } else { 6 }
    } else if (this_null && !this_neg) || (that_null && !that_neg) {
        // An empty NullRegion overlaps nothing.
        1
    } else if that_null && that_neg {
        // "that" is infinite: "this" lies entirely inside it.
        2
    } else if this_null && this_neg {
        // "this" is infinite: "that" lies entirely inside it.
        3
    } else {
        4
    };

    Ok(result)
}

/// Faithful port of the generic `Region::OverlapX` (region.c:6996).
///
/// Parameters follow the C signature `OverlapX(that, this)`. The returned
/// code is in the public `astOverlap(this, that)` convention.
// `result` is declared without an initialiser and assigned from four nested
// branches that mirror C's control flow (`region.c:7204-7409`). That is
// deliberate: it makes "every path assigns a result" a compile-time
// requirement. Giving it a placeholder to satisfy the lint would let a path
// that missed its assignment return a value outside this function's 1..=6
// domain silently, and hoisting the four assignments into expression position
// would restructure a direct port of C's branch structure.
#[allow(clippy::needless_late_init)]
fn overlap_x(that: &dyn Region, this: &dyn Region) -> AstResult<i32> {
    // Return 5 if the two Regions are equal, or 6 if they are equal after
    // temporarily negating the first (region.c:7072-7083).
    if (this as &dyn Mapping).equal(that as &dyn Mapping) {
        return Ok(5);
    }
    let this_neg = this.negated_copy();
    if (this_neg.as_ref() as &dyn Mapping).equal(that as &dyn Mapping) {
        return Ok(6);
    }

    // Get the Mapping from the Frame of the second Region (that) to the Frame
    // of the first Region (this). astConvert(that, this) yields a FrameSet
    // whose base->current Mapping runs that_current -> this_current
    // (region.c:7088).
    let fs0 = match that
        .get_region_frame()
        .convert_to(this.get_region_frame().as_ref(), &[])?
    {
        Some(fs) => fs,
        None => return Ok(0),
    };
    // cmap maps reg2_current -> reg1_current. This invariant is preserved
    // across the region swap below by inverting cmap.
    let mut cmap = fs0.mapping(FrameRef::Base, FrameRef::Current)?;

    // reg1 = this, reg2 = that initially; `reg2_is_that` tracks whether reg2
    // is still the original `that` (used to orient codes 2/3). `first` guards
    // the single retry with the regions swapped (region.c:7097-7099).
    let mut reg1: &dyn Region = this;
    let mut reg2: &dyn Region = that;
    let mut reg2_is_that = true;
    let mut first = true;

    // Bounds of the transformed reg2 mesh, in reg1's base Frame. Filled in the
    // loop; needed for the unbounded-reg1 fallback in the FALSE branch.
    let nc0 = this.get_region_frameset().base().naxes().max(that.naxes());
    let mut lbnd = vec![f64::MAX; nc0];
    let mut ubnd = vec![-f64::MAX; nc0];

    // The transformed reg2 mesh (reg1 base Frame), reg2 mesh (reg2 current
    // Frame), and the reg2_current -> reg1_base Mapping, resolved when the
    // L1 loop breaks on a usable mesh.
    //
    // L1 (region.c:7100): try to mesh reg2 and transform it into reg1's base
    // Frame; on failure (no mesh, or no good transformed points) swap the
    // regions once and retry.
    let (ps1, reg2_mesh, map, map_reg1) = loop {
        // astRegMesh(reg2): reg2's boundary mesh in its *current* Frame. The
        // Rust get_region_mesh returns *base* Frame points, so map them
        // base->current to match C.
        let base_mesh = reg2.get_region_mesh()?;
        let cur_mesh = reg2.get_region_frameset().transform(&base_mesh, true)?;

        let usable = mesh_has_good(&cur_mesh);
        if !usable {
            if first {
                std::mem::swap(&mut reg1, &mut reg2);
                reg2_is_that = !reg2_is_that;
                cmap.invert();
                first = false;
                continue;
            }
            // Neither Region generated a usable mesh: cannot determine
            // overlap (region.c:7179).
            return Err(AstError::new(
                ErrorCode::Inter,
                "astOverlap(Region): neither of the two supplied Regions has a finite boundary"
                    .to_string(),
            ));
        }

        // map_reg1: reg1 current -> base (region.c:7125).
        let m_reg1 = reg1
            .get_region_frameset()
            .mapping(FrameRef::Current, FrameRef::Base)?;
        // map: reg2 current -> reg1 base (region.c:7129).
        let m = CmpMap::new(cmap.clone_box(), m_reg1.clone_box(), Composition::Series)?;
        let m: crate::mappings::DynMapping = Box::new(m);

        // Transform the mesh into reg1's base Frame (region.c:7132).
        let transformed = m.transform(&cur_mesh, true)?;

        // Check for good points and find the bounds (region.c:7136-7154).
        let nc = transformed.ncoord();
        let np = transformed.npoint();
        let mut good = false;
        for i in 0..nc {
            lbnd[i] = f64::MAX;
            ubnd[i] = -f64::MAX;
            for j in 0..np {
                let v = transformed.point(j)[i];
                if !is_bad(v) {
                    good = true;
                    if v > ubnd[i] {
                        ubnd[i] = v;
                    }
                    if v < lbnd[i] {
                        lbnd[i] = v;
                    }
                }
            }
        }

        if !good && first {
            std::mem::swap(&mut reg1, &mut reg2);
            reg2_is_that = !reg2_is_that;
            cmap.invert();
            first = false;
            continue;
        }

        break (transformed, cur_mesh, m, m_reg1);
    };

    let mut ps1 = ps1;
    let nc = ps1.ncoord();
    let np = ps1.npoint();

    // Transform the uncertainty of reg2 into reg1's base Frame (region.c:7193).
    let unc = reg2.get_unc_frm(FrameRef::Current)?;
    let bfrm_reg1 = reg1.get_region_frameset().base().snapshot();
    let unc1 = unc.map_region(map.as_ref(), bfrm_reg1.as_ref())?;

    // See if every transformed mesh point lies on reg1's boundary, to within
    // the joint uncertainty. If so the two boundaries are equivalent
    // (region.c:7202).
    let (pins, mask) = reg1.reg_pins_mask(&ps1, Some(unc1.as_ref()))?;

    let mut touch = false;
    let result: i32;

    if pins {
        // Equivalent boundaries: the Regions are either identical or mutually
        // exclusive. Distinguish by transforming the mean pin position and
        // testing membership of both Regions (region.c:7204-7258).
        let mut ps4 = PointSet::zeros(1, nc);
        for i in 0..nc {
            let mut good = 0usize;
            let mut axsum = 0.0;
            for j in 0..np {
                let v = ps1.point(j)[i];
                if !is_bad(v) {
                    good += 1;
                    axsum += v;
                }
            }
            ps4.point_mut(0)[i] = if good > 0 {
                axsum / good as f64
            } else {
                crate::constants::BAD_VALUE
            };
        }

        // Is the mean position inside reg1? Transform reg1 base -> current
        // (map_reg1 inverse), then mask with reg1 as a Mapping. C uses
        // forward=0 for both transforms; for a Region the forward and inverse
        // transforms are identical (both mask by membership), so the masking
        // forward direction reproduces C's behaviour (region.c:7236-7239).
        let ps3 = map_reg1.transform(&ps4, false)?;
        let ps5 = (reg1 as &dyn Mapping).transform(&ps3, true)?;
        let inside1 = !is_bad(ps5.point(0)[0]);

        // Is the mean position inside reg2? Transform reg1 base -> reg2
        // current (map inverse), then mask with reg2 (region.c:7246-7249).
        let ps3 = map.transform(&ps4, false)?;
        let ps5 = (reg2 as &dyn Mapping).transform(&ps3, true)?;
        let inside2 = !is_bad(ps5.point(0)[0]);

        result = if inside1 == inside2 { 5 } else { 6 };
    } else {
        // The boundaries are not equivalent. Build a sub-mesh of the reg2
        // points that are NOT on reg1's boundary (region.c:7266).
        let reg2_submesh = get_sub_mesh(&mask, &reg2_mesh);

        // Transform the submesh into reg1's current Frame, then mask with reg1
        // as a Mapping (region.c:7271-7275).
        let ps1_sub = cmap.transform(&reg2_submesh, true)?;
        let ps2 = (reg1 as &dyn Mapping).transform(&ps1_sub, true)?;
        let np2 = ps2.npoint();

        // Any common boundary points? (region.c:7283).
        touch = reg2_mesh.npoint() != np2;

        // Are the submesh points all inside reg1, all outside, or mixed?
        // (region.c:7290-7306).
        let mut allbad = true;
        let mut allgood = true;
        for iax in 0..ps2.ncoord() {
            for ip in 0..np2 {
                if is_bad(ps2.point(ip)[iax]) {
                    allgood = false;
                } else {
                    allbad = false;
                }
            }
        }

        if !allgood && !allbad {
            // Some in, some out: partial overlap (region.c:7310).
            result = 4;
        } else {
            // Use a mesh on reg1's boundary to differentiate the remaining
            // cases (region.c:7315-7409).
            let ps3 = reg1.get_region_mesh()?; // base Frame mesh
            let ps3 = if mesh_has_good(&ps3) {
                Some(ps3)
            } else {
                // reg1 unbounded: use the part of its boundary overlapping a
                // box twice the size of reg2's bounding box (region.c:7329).
                let mut lb2 = lbnd.clone();
                let mut ub2 = ubnd.clone();
                for i in 0..nc {
                    lb2[i] *= 2.0;
                    ub2[i] *= 2.0;
                }
                bnd_base_mesh(reg1, &lb2[..nc], &ub2[..nc])?
            };

            let ps3 = ps3.ok_or_else(|| {
                AstError::new(
                    ErrorCode::Inter,
                    "astOverlap: cannot determine the overlap between the supplied Regions"
                        .to_string(),
                )
            })?;

            // Transform reg1's boundary mesh into reg2's current Frame
            // (map inverse), then mask with reg2 (region.c:7352-7355).
            let ps4 = map.transform(&ps3, false)?;
            let ps5 = (reg2 as &dyn Mapping).transform(&ps4, true)?;

            // Count points of reg1's boundary that lie inside reg2
            // (region.c:7360-7371).
            let np5 = ps5.npoint();
            let mut inside_count = 0usize;
            for ip in 0..np5 {
                if !is_bad(ps5.point(ip)[0]) {
                    inside_count += 1;
                }
            }
            let inside1 = inside_count > np5 / 2;

            if allgood {
                // Entire reg2 submesh inside reg1 (region.c:7382-7392).
                result = if inside1 {
                    4
                } else if reg2_is_that {
                    3
                } else {
                    2
                };
            } else {
                // Entire reg2 submesh outside reg1 (region.c:7398-7408).
                result = if inside1 {
                    if reg2_is_that { 2 } else { 3 }
                } else {
                    1
                };
            }
        }

        // If the regions do not overlap but touch, treat as intersecting when
        // either is closed (region.c:7421-7425).
        let result = if touch && result == 1 && (this.is_closed() || that.is_closed()) {
            4
        } else {
            result
        };
        return Ok(result);
    }

    // Silence unused warning on `ps1`/`touch` in the pins branch.
    let _ = (&mut ps1, touch);
    Ok(result)
}

/// Faithful port of `astBndBaseMesh` (region.c:1683): a mesh on the part of
/// `reg1`'s boundary that overlaps the current-Frame box `[lbnd, ubnd]`,
/// returned in `reg1`'s base Frame. Returns `None` when the box does not
/// partially overlap the Region.
fn bnd_base_mesh(reg1: &dyn Region, lbnd: &[f64], ubnd: &[f64]) -> AstResult<Option<PointSet>> {
    use super::box_region::BoxRegion;
    use super::{CmpOperator, CmpRegionBuilder};

    // Form a Box describing the required box in reg1's current Frame.
    let boxreg = BoxRegion::from_bounds_frame(reg1.get_region_frame(), lbnd, ubnd)?;

    // Only proceed if there is partial (or greater) overlap: astOverlap > 3.
    let ov = reg1.overlap(&boxreg)?;
    let code = match ov {
        Overlap::Partial => 4,
        Overlap::Same => 5,
        Overlap::Negation => 6,
        _ => 0,
    };
    if code > 3 {
        // Intersection of reg1 and the box; its boundary mesh.
        let cmpreg =
            CmpRegionBuilder::from_dyn(reg1.clone_region(), Box::new(boxreg), CmpOperator::And)?
                .build()?;
        Ok(Some(cmpreg.get_region_mesh()?))
    } else {
        Ok(None)
    }
}

/// Faithful port of `GetSubMesh` (region.c:3688): copy the points of `input`
/// whose mask entry is `false` (C's mask value 0) into a new PointSet.
fn get_sub_mesh(mask: &[bool], input: &PointSet) -> PointSet {
    let nc = input.ncoord();
    let mut rows: Vec<Vec<f64>> = Vec::new();
    for ip in 0..input.npoint() {
        if !mask.get(ip).copied().unwrap_or(false) {
            rows.push(input.point(ip)[..nc].to_vec());
        }
    }
    if rows.is_empty() {
        PointSet::zeros(0, nc)
    } else {
        PointSet::from_rows(rows.iter().map(|r| r.as_slice()))
    }
}

/// Whether a PointSet contains at least one point with all-good coordinates.
fn mesh_has_good(pts: &PointSet) -> bool {
    for ip in 0..pts.npoint() {
        if !pts.point(ip).iter().any(|&v| is_bad(v)) {
            return true;
        }
    }
    false
}
