// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The generic overlap algorithm of region.c, and NullRegion's.
//!
//! C's `astOverlap(this, that)` is two per-class methods: `Overlap`,
//! dispatched on the class of `this`, whose default (region.c) calls
//! `OverlapX(that, this)`, dispatched on the class of `that`. The defaults
//! of [`Region::overlap`] and [`Region::overlap_x`] are that dispatch, and
//! this module holds the generic `OverlapX` and `BndBaseMesh` (region.c)
//! they fall back on, the `NullRegion` `OverlapX` (nullregion.c), and the
//! `GetSubMesh` helper (region.c).

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::FrameRef;
use crate::mappings::helpers::is_bad;
use crate::mappings::{CmpMap, Composition, Mapping};

use super::nullregion::NullRegion;
use super::{Overlap, Region};

/// Map a C `astOverlap` return code (0-6) to the Rust [`Overlap`] enum.
pub(crate) fn code_to_overlap(code: i32) -> Overlap {
    match code {
        1 => Overlap::None,
        2 => Overlap::FirstInside,
        3 => Overlap::SecondInside,
        4 => Overlap::Partial,
        5 => Overlap::Same,
        6 => Overlap::Negation,
        _ => Overlap::Incomparable,
    }
}

/// The C `astOverlap` code of an [`Overlap`].
pub(crate) fn overlap_to_code(overlap: Overlap) -> i32 {
    match overlap {
        Overlap::Incomparable => 0,
        Overlap::None => 1,
        Overlap::FirstInside => 2,
        Overlap::SecondInside => 3,
        Overlap::Partial => 4,
        Overlap::Same => 5,
        Overlap::Negation => 6,
    }
}

/// `NullRegion::OverlapX` (nullregion.c), which NullRegion's `Overlap`
/// also calls with its arguments swapped.
///
/// Parameters follow the C signature `OverlapX(that, this)`, and the result
/// is `astOverlap(this, that)`.
pub(crate) fn null_region_overlap_x(that: &dyn Region, this: &dyn Region) -> AstResult<Overlap> {
    // Check that the Regions can be compared meaningfully (astConvert).
    if crate::frames::find_frame::convert(
        this.get_region_frame().as_ref(),
        that.get_region_frame().as_ref(),
        &[],
    )?
    .is_none()
    {
        return Ok(Overlap::Incomparable);
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

    Ok(code_to_overlap(result))
}

/// The generic `OverlapX` (region.c), the default of
/// [`Region::overlap_x`].
///
/// Parameters follow the C signature `OverlapX(that, this)`, and the result
/// is `astOverlap(this, that)`.
pub(crate) fn region_overlap_x(that: &dyn Region, this: &dyn Region) -> AstResult<Overlap> {
    overlap_x(that, this).map(code_to_overlap)
}

/// The generic `OverlapX` as C writes it, returning C's code.
// `result` is declared without an initializer and assigned from four nested
// branches that mirror C's control flow (region.c `OverlapX`). That is
// deliberate: it makes "every path assigns a result" a compile-time
// requirement. Giving it a placeholder to satisfy the lint would let a path
// that missed its assignment return a value outside this function's 1..=6
// domain silently, and hoisting the four assignments into expression position
// would restructure a direct port of C's branch structure.
#[allow(clippy::needless_late_init)]
fn overlap_x(that: &dyn Region, this: &dyn Region) -> AstResult<i32> {
    // Return 5 if the two Regions are equal, or 6 if they are equal after
    // temporarily negating the first.
    if (this as &dyn Mapping).equal(that as &dyn Mapping) {
        return Ok(5);
    }
    let this_neg = this.negated_copy();
    if (this_neg.as_ref() as &dyn Mapping).equal(that as &dyn Mapping) {
        return Ok(6);
    }

    // Get the Mapping from the Frame of the second Region (that) to the Frame
    // of the first Region (this). astConvert(that, this) yields a FrameSet
    // whose base->current Mapping runs that_current -> this_current.
    let fs0 = match crate::frames::find_frame::convert(
        that.get_region_frame().as_ref(),
        this.get_region_frame().as_ref(),
        &[],
    )? {
        Some(fs) => fs,
        None => return Ok(0),
    };
    // cmap maps reg2_current -> reg1_current. This invariant is preserved
    // across the region swap below by inverting cmap.
    let mut cmap = fs0.mapping(FrameRef::Base, FrameRef::Current)?;

    // reg1 = this, reg2 = that initially; `reg2_is_that` tracks whether reg2
    // is still the original `that` (used to orient codes 2/3). `first` guards
    // the single retry with the regions swapped.
    let mut reg1: &dyn Region = this;
    let mut reg2: &dyn Region = that;
    let mut reg2_is_that = true;
    let mut first = true;

    // The transformed reg2 mesh (reg1 base Frame), reg2 mesh (reg2 current
    // Frame), and the reg2_current -> reg1_base Mapping, resolved when the
    // L1 loop breaks on a usable mesh.
    //
    // L1 (region.c `OverlapX`): try to mesh reg2 and transform it into reg1's base
    // Frame; on failure (no mesh, or no good transformed points) swap the
    // regions once and retry.
    let neither = || {
        AstError::new(
            ErrorCode::Inter,
            format!(
                "astOverlap(Region): Neither of the two supplied Regions (classes {} and {}) \
                 has a finite boundary.\n\
                 The current implementation of astOverlap cannot determine the overlap \
                 between two Regions unless at least one of them has a finite boundary.",
                super::class_name(this.as_any()),
                super::class_name(that.as_any())
            ),
        )
    };
    let (ps1, reg2_mesh, map, map_reg1) = loop {
        // astRegMesh(reg2): reg2's boundary mesh in its current Frame. C
        // suppresses any error, which means no mesh.
        let Ok(cur_mesh) = reg2.reg_mesh() else {
            if first {
                std::mem::swap(&mut reg1, &mut reg2);
                reg2_is_that = !reg2_is_that;
                cmap.invert();
                first = false;
                continue;
            }
            return Err(neither());
        };

        // map_reg1: reg1 current -> base.
        let m_reg1 = reg1
            .get_region_frameset()
            .mapping(FrameRef::Current, FrameRef::Base)?;
        // map: reg2 current -> reg1 base.
        let m = CmpMap::new(cmap.clone_box(), m_reg1.clone_box(), Composition::Series)?;
        let m: crate::mappings::DynMapping = Box::new(m);

        // Transform the mesh into reg1's base Frame.
        let transformed = m.transform(&cur_mesh, true)?;

        // Check for good points.
        let good =
            (0..transformed.ncoord()).any(|i| !transformed.axis(i).iter().all(|&v| is_bad(v)));

        // A mesh with no good points is discarded: swap and retry on the
        // first attempt, and on the second there is no mesh at all.
        if !good {
            if first {
                std::mem::swap(&mut reg1, &mut reg2);
                reg2_is_that = !reg2_is_that;
                cmap.invert();
                first = false;
                continue;
            }
            return Err(neither());
        }

        break (transformed, cur_mesh, m, m_reg1);
    };

    let mut ps1 = ps1;
    let nc = ps1.ncoord();
    let np = ps1.npoint();

    // Transform the uncertainty of reg2 into reg1's base Frame.
    let unc = reg2.get_unc_frm(FrameRef::Current)?;
    let bfrm_reg1 = reg1.get_region_frameset().base().snapshot();
    let unc1 = unc.reg_map_region(map.as_ref(), bfrm_reg1.as_ref())?;

    // See if every transformed mesh point lies on reg1's boundary, to within
    // the joint uncertainty. If so the two boundaries are equivalent.
    let (pins, mask) = reg1.reg_pins_mask(&ps1, Some(unc1.as_ref()))?;

    let mut touch = false;
    let result: i32;

    if pins {
        // Equivalent boundaries: the Regions are either identical or mutually
        // exclusive. Distinguish by transforming the mean pin position and
        // testing membership of both Regions.
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
        // forward direction reproduces C's behavior.
        let ps3 = map_reg1.transform(&ps4, false)?;
        let ps5 = (reg1 as &dyn Mapping).transform(&ps3, true)?;
        let inside1 = !is_bad(ps5.point(0)[0]);

        // Is the mean position inside reg2? Transform reg1 base -> reg2
        // current (map inverse), then mask with reg2.
        let ps3 = map.transform(&ps4, false)?;
        let ps5 = (reg2 as &dyn Mapping).transform(&ps3, true)?;
        let inside2 = !is_bad(ps5.point(0)[0]);

        result = if inside1 == inside2 { 5 } else { 6 };
    } else {
        // The boundaries are not equivalent. Build a sub-mesh of the reg2
        // points that are NOT on reg1's boundary.
        let reg2_submesh = get_sub_mesh(&mask, &reg2_mesh);

        // Transform the submesh into reg1's current Frame, then mask with reg1
        // as a Mapping.
        let ps1_sub = cmap.transform(&reg2_submesh, true)?;
        let ps2 = (reg1 as &dyn Mapping).transform(&ps1_sub, true)?;
        let np2 = ps2.npoint();

        // Any common boundary points?
        touch = reg2_mesh.npoint() != np2;

        // Are the submesh points all inside reg1, all outside, or mixed?.
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
            // Some in, some out: partial overlap.
            result = 4;
        } else {
            // Use a mesh on reg1's boundary to differentiate the remaining
            // cases.
            // reg1's boundary mesh in its base Frame. C suppresses any error,
            // which means no mesh; an unbounded reg1 then gives the part of
            // its boundary inside a box twice the size of reg2's bounding box,
            // again with errors suppressed.
            // The box is needed in reg1's current Frame, so its bounds are
            // those of reg2's mesh transformed into that Frame.
            let ps3 = match reg1.base_mesh() {
                Ok(mesh) => mesh,
                Err(_) => {
                    let ps6 = cmap.transform(&reg2_mesh, true)?;
                    let (lbnd, ubnd): (Vec<f64>, Vec<f64>) = (0..ps6.ncoord())
                        .map(|i| {
                            let (lo, hi) = ps6
                                .axis(i)
                                .iter()
                                .filter(|&&v| !is_bad(v))
                                .fold((f64::MAX, -f64::MAX), |(lo, hi), &v| (lo.min(v), hi.max(v)));
                            (lo * 2.0, hi * 2.0)
                        })
                        .unzip();
                    reg1.bnd_base_mesh(&lbnd, &ubnd).map_err(|_| {
                        AstError::new(
                            ErrorCode::Inter,
                            format!(
                                "astOverlap({this_class}): The current implementation of \
                                 astOverlap cannot determine the overlap between the supplied \
                                 Regions (a {this_class} and a {}).",
                                super::class_name(that.as_any()),
                                this_class = super::class_name(this.as_any()),
                            ),
                        )
                    })?
                }
            };

            // Transform reg1's boundary mesh into reg2's current Frame
            // (map inverse), then mask with reg2.
            let ps4 = map.transform(&ps3, false)?;
            let ps5 = (reg2 as &dyn Mapping).transform(&ps4, true)?;

            // Count points of reg1's boundary that lie inside reg2.
            let np5 = ps5.npoint();
            let mut inside_count = 0usize;
            for ip in 0..np5 {
                if !is_bad(ps5.point(ip)[0]) {
                    inside_count += 1;
                }
            }
            let inside1 = inside_count > np5 / 2;

            if allgood {
                // Entire reg2 submesh inside reg1.
                result = if inside1 {
                    4
                } else if reg2_is_that {
                    3
                } else {
                    2
                };
            } else {
                // Entire reg2 submesh outside reg1.
                result = if inside1 {
                    if reg2_is_that { 2 } else { 3 }
                } else {
                    1
                };
            }
        }

        // If the regions do not overlap but touch, treat as intersecting when
        // either is closed.
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

/// The generic `BndBaseMesh` (region.c), the default of
/// [`Region::bnd_base_mesh`]: the boundary mesh of the intersection of
/// `reg` with the current-Frame box `[lbnd, ubnd]`, in `reg`'s base Frame,
/// or one bad point when the two do not partially overlap.
pub(crate) fn region_bnd_base_mesh(
    reg: &dyn Region,
    lbnd: &[f64],
    ubnd: &[f64],
) -> AstResult<PointSet> {
    use super::box_region::BoxRegion;
    use super::{CmpOperator, CmpRegionBuilder};

    // The mesh is taken from the intersection of the Region and the box,
    // which needs the Region to have a boundary mesh of its own: it has one
    // if either the Region or its negation is bounded. Without one, the
    // intersection would ask for this same mesh again, without end.
    if !reg.is_bounded()? && !reg.negated_copy().is_bounded()? {
        let class = super::class_name(reg.as_any());
        return Err(AstError::new(
            ErrorCode::Inter,
            format!(
                "astBndBaseMesh({class}): The part of the boundary of the {class} that is \
                 within a box cannot be found, since neither the {class} nor its negation is \
                 bounded."
            ),
        ));
    }

    // A Box describing the required box in reg's current Frame.
    let boxreg = BoxRegion::from_bounds_frame(reg.get_region_frame(), lbnd, ubnd)?;

    // Partial overlap or more: astOverlap > 3.
    if overlap_to_code(reg.overlap(&boxreg)?) > 3 {
        let cmpreg =
            CmpRegionBuilder::from_dyn(reg.clone_region(), Box::new(boxreg), CmpOperator::And)?
                .build()?;
        cmpreg.base_mesh()
    } else {
        let nc = reg.get_region_frameset().base().naxes();
        Ok(PointSet::from_rows(std::iter::once(
            vec![crate::constants::BAD_VALUE; nc].as_slice(),
        )))
    }
}

/// Faithful port of `GetSubMesh` (region.c): copy the points of `input`
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
