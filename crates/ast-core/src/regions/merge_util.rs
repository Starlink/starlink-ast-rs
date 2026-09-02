//! Shared helpers for cross-type Region parallel merges.
//!
//! `Interval` and `Box` can absorb a neighbouring `NullRegion` or
//! `PointList` in a parallel `CmpMap` to form a higher-dimensional
//! Region. The two owning classes (`interval.c::MergeInterval` and
//! `box.c::MergeBox` in the C reference) share the same tail logic:
//! propagate attributes, combine the two uncertainty Regions into a
//! (simplified) Prism, and clear the Negated flag. These helpers factor
//! out the parts common to both so each class only has to build the
//! per-class bounds/points.

use crate::frames::{DynFrame, FrameSet};
use crate::mappings::{DynMapping, Mapping};

use super::data::{RegionData, default_mesh_size};
use super::{
    BoxRegion, Circle, CmpRegion, DynRegion, Ellipse, Interval, NullRegion, PointList, Polygon,
    Prism, PrismBuilder,
};

/// The base-Frame geometry of a merged Region, before it is placed in a
/// combined CmpFrame. `MergeInterval`/`MergeBox` produce one of a small
/// set of concrete Region kinds depending on the neighbour type.
pub(super) enum MergedRegion {
    /// An [`Interval`] defined by lower/upper bounds (BAD = unbounded).
    Interval(Vec<f64>, Vec<f64>),
    /// A [`super::BoxRegion`] defined by centre/corner points.
    Box(Vec<f64>, Vec<f64>),
    /// A [`PointList`] defined by one point per row.
    PointList(Vec<Vec<f64>>),
}

/// The FillFactor and MeshSize scaling inputs from the two merging
/// Regions, threaded from `MergeBox`/`MergeInterval` into the shared tail.
///
/// Each `(value, set)` pair mirrors the C `astGetMeshSize`/`astTestMeshSize`
/// (and FillFactor) queries taken on the two supplied Regions before the
/// merge (box.c:2049-2085).
pub(super) struct MergeScaling {
    /// `(effective_mesh_size, was_set, naxes)` for `this`.
    pub(super) msz_this: (usize, bool, usize),
    /// `(effective_mesh_size, was_set, naxes)` for `reg`.
    pub(super) msz_reg: (usize, bool, usize),
    /// `(effective_fill_factor, was_set)` for `this`.
    pub(super) fill_this: (f64, bool),
    /// `(effective_fill_factor, was_set)` for `reg`.
    pub(super) fill_reg: (f64, bool),
}

/// Apply the FillFactor/MeshSize scaling of a parallel Region merge to the
/// merged Region's `data`, given the two source Regions' scaling inputs.
///
/// Faithful port of the shared scaling block in `MergeBox`/`MergeInterval`/
/// `MergePointList`/`MergeNullRegion` (box.c:2049-2085 and identical
/// elsewhere): the FillFactor is the product of the two supplied
/// FillFactors when either was set, and the MeshSize (when either was set)
/// is the result's default scaled by the product of each source's
/// used/default ratio. The per-source and result defaults are the
/// naxes-dependent `astGetMeshSize` values.
pub(super) fn apply_merge_scaling(data: &mut RegionData, scaling: &MergeScaling) {
    if scaling.fill_this.1 || scaling.fill_reg.1 {
        data.set_fill_factor(scaling.fill_this.0 * scaling.fill_reg.0);
    }

    if scaling.msz_this.1 || scaling.msz_reg.1 {
        let def_this = default_mesh_size(scaling.msz_this.2) as f64;
        let def_reg = default_mesh_size(scaling.msz_reg.2) as f64;
        let fac_this = scaling.msz_this.0 as f64 / def_this;
        let fac_reg = scaling.msz_reg.0 as f64 / def_reg;
        let def_result = default_mesh_size(data.frameset.current().naxes()) as f64;
        let scaled = (fac_this * fac_reg * def_result).round() as usize;
        data.set_mesh_size(scaled);
    }
}

/// [`apply_merge_scaling`] for a Region that already exists, rather than for the
/// `RegionData` a Region is about to be built from.
///
/// `MergeNullRegion` applies the FillFactor product and the MeshSize ratio to
/// the Region `astMapRegion` returned (nullregion.c:740-776), not to the Region
/// built before it, so the result's own axis count is what sets the MeshSize
/// default.
pub(super) fn apply_merge_scaling_to(region: &mut dyn super::Region, scaling: &MergeScaling) {
    if scaling.fill_this.1 || scaling.fill_reg.1 {
        region.set_fill_factor(scaling.fill_this.0 * scaling.fill_reg.0);
    }

    if scaling.msz_this.1 || scaling.msz_reg.1 {
        let def_this = default_mesh_size(scaling.msz_this.2) as f64;
        let def_reg = default_mesh_size(scaling.msz_reg.2) as f64;
        let fac_this = scaling.msz_this.0 as f64 / def_this;
        let fac_reg = scaling.msz_reg.0 as f64 / def_reg;
        let def_result = default_mesh_size(region.naxes()) as f64;
        region.set_mesh_size((fac_this * fac_reg * def_result).round() as usize);
    }
}

/// The Frames and Mapping a cross-type merge builds its result through: the
/// combined base Frame the merged Region is constructed on, the combined current
/// Frame it ends in, and the parallel base->current Mapping between them
/// (interval.c:1335-1343 and 1573-1598, box.c:1764-1773 and 2019-2044).
pub(super) struct MergeFrames {
    /// `astCmpFrame` of the two base Frames, in merge order.
    pub(super) bfrm: DynFrame,
    /// `astCmpFrame` of the two current Frames, in merge order.
    pub(super) cfrm: DynFrame,
    /// Parallel `astCmpMap` of the two base->current Mappings, in merge order.
    pub(super) bcmap: DynMapping,
}

/// The attributes `astRegOverlay(new, this, 1)` copies from the `this` Region
/// onto the merged one.
pub(super) struct MergeOverlay {
    /// `this`'s Closed value.
    pub(super) closed: bool,
    /// Whether `this` has Closed set explicitly.
    pub(super) closed_set: bool,
    /// `this`'s Adaptive value.
    pub(super) adaptive: bool,
}

/// Finish a cross-type Region merge: build the concrete merged Region on the
/// combined base Frame, propagate the `this`-side attributes, clear the Negated
/// flag, install the combined uncertainty, map the result into the combined
/// current Frame, and apply the FillFactor/MeshSize scaling.
///
/// Mirrors the shared tail of `MergeInterval`/`MergeBox` (interval.c:1537-1639,
/// box.c:1983-2085). The order is C's: `astRegOverlay(new, this, 1)` copies
/// Closed/Adaptive from `this`, `astClearNegated` forces the result un-negated,
/// `astSetUnc` installs the combined uncertainty, `astMapRegion(new, bcmap,
/// cfrm)` carries the Region from `bfrm` into the combined current Frame, and
/// the scaling block sets the FillFactor to the product of the two and the
/// MeshSize to the result default scaled by the product of each source's
/// used/default ratio.
///
/// Building on `bfrm` rather than on the current Frame is what the defining
/// points require — they are base-Frame coordinates — and the `astMapRegion`
/// that follows is also what restores the `Frm` block on the uncertainty, since
/// its deep copy clears `RegionFS` throughout (region.c:12936-12940).
pub(super) fn finish_region_merge(
    built: MergedRegion,
    frames: MergeFrames,
    overlay: MergeOverlay,
    uncertainty: Option<DynRegion>,
    scaling: MergeScaling,
) -> Option<DynMapping> {
    let MergeFrames { bfrm, cfrm, bcmap } = frames;

    let apply_attrs = |data: &mut RegionData| {
        // astRegOverlay copies Closed and Adaptive from `this`; Negated is
        // then cleared (the merged Region is always un-negated).
        data.closed = overlay.closed;
        data.closed_set = overlay.closed_set;
        data.adaptive = overlay.adaptive;
        data.negated = false;
    };

    let frameset = FrameSet::new(bfrm);

    let mut new: DynRegion = match built {
        MergedRegion::Interval(lbnd, ubnd) => {
            let mut data = RegionData::new(frameset, vec![lbnd, ubnd]);
            apply_attrs(&mut data);
            Box::new(Interval { data })
        }
        MergedRegion::Box(centre, corner) => {
            let mut data = RegionData::new(frameset, vec![centre, corner]);
            apply_attrs(&mut data);
            Box::new(BoxRegion { data })
        }
        MergedRegion::PointList(points) => {
            let mut data = RegionData::new(frameset, points);
            apply_attrs(&mut data);
            Box::new(PointList { data })
        }
    };

    if let Some(unc) = uncertainty {
        new.set_uncertainty(unc);
    }

    let mut result = new.map_region(bcmap.as_ref(), cfrm.as_ref()).ok()?;
    apply_merge_scaling_to(result.as_mut(), &scaling);

    // C returns the mapped Region as it stands. A fully-bounded Interval
    // reclassifies to a Box on the simplify loop's next visit to this list
    // entry, through `Interval::Simplify`'s equivalent-Box head.
    Some(result.as_dyn_mapping().clone_box())
}

/// Convert a `DynMapping` to a `DynRegion` if it is a concrete Region
/// type, else return `None`.
///
/// Mirrors the `astIsARegion` check used throughout the C merge code:
/// the simplified result of a CmpMap of two Regions may itself be a
/// Region (when the two merged) or remain a CmpMap (when they did not).
pub(super) fn mapping_into_region(map: DynMapping) -> Option<DynRegion> {
    // Deref the Box to `&dyn Mapping` before `as_any`: calling `as_any` on the
    // `Box<dyn Mapping>` directly resolves the blanket `AsAny` impl for the Box
    // itself, whose `Any` downcasts never match the inner concrete Region.
    let a = (*map).as_any();
    if let Some(r) = a.downcast_ref::<Circle>() {
        Some(Box::new(r.clone()))
    } else if let Some(r) = a.downcast_ref::<BoxRegion>() {
        Some(Box::new(r.clone()))
    } else if let Some(r) = a.downcast_ref::<Ellipse>() {
        Some(Box::new(r.clone()))
    } else if let Some(r) = a.downcast_ref::<Polygon>() {
        Some(Box::new(r.clone()))
    } else if let Some(r) = a.downcast_ref::<Interval>() {
        Some(Box::new(r.clone()))
    } else if let Some(r) = a.downcast_ref::<NullRegion>() {
        Some(Box::new(r.clone()))
    } else if let Some(r) = a.downcast_ref::<CmpRegion>() {
        Some(Box::new(r.clone()))
    } else if let Some(r) = a.downcast_ref::<PointList>() {
        Some(Box::new(r.clone()))
    } else if let Some(r) = a.downcast_ref::<Prism>() {
        Some(Box::new(r.clone()))
    } else {
        None
    }
}

/// Test whether a Region that occupies the axis extents `[lbnd, ubnd]`
/// (its base bounding box) is effectively a point — i.e. its width on
/// every axis is at most a quarter of its uncertainty on that axis.
///
/// Mirrors the "zero width" test in the PointList branch of
/// `box.c::MergeBox` (1921-1932): `fabs(width) > 0.25*unc_width` fails.
/// The uncertainty width is taken from the explicit uncertainty Region's
/// base bounding box when one is present, otherwise from the default
/// uncertainty (`region.c::GetDefUnc`, 3560-3575): a box spanning
/// `1.0E-6` of the Region's own bounding box, or `|centre|*0.5E-6`
/// half-width where an axis has zero extent.
pub(super) fn region_is_point(
    lbnd: &[f64],
    ubnd: &[f64],
    explicit_unc: Option<&DynRegion>,
) -> bool {
    let nax = lbnd.len();

    // Per-axis uncertainty half-width.
    let unc_full: Vec<f64> = if let Some(unc) = explicit_unc {
        // Explicit uncertainty: use its base bounding box width.
        match unc.get_region_bounds() {
            Ok((ulb, uub)) => (0..nax).map(|i| (uub[i] - ulb[i]).abs()).collect(),
            Err(_) => return false,
        }
    } else {
        // Default uncertainty width per GetDefUnc.
        (0..nax)
            .map(|i| {
                let hw = (0.5e-6 * (ubnd[i] - lbnd[i])).abs();
                let hw = if hw == 0.0 {
                    (0.5 * (ubnd[i] + lbnd[i])) * 0.5e-6
                } else {
                    hw
                };
                // Full width = 2 * half-width.
                2.0 * hw
            })
            .collect()
    };

    for i in 0..nax {
        if (ubnd[i] - lbnd[i]).abs() > 0.25 * unc_full[i] {
            return false;
        }
    }
    true
}

/// Combine the uncertainties of two merging Regions into a single
/// uncertainty Region for the merged result, in the axis order given by
/// `first_this` (true → `this` axes come first).
///
/// Mirrors the uncertainty half of `MergeInterval`/`MergeBox`
/// (interval.c:1547-1565, box.c:1993-2011): build a Prism from the two
/// base-Frame uncertainties in the correct order, then simplify that Prism.
///
/// Going through the Prism rather than straight to the parallel CmpMap its
/// `Simplify` starts from is load-bearing, not ceremony. `Prism::Simplify`
/// ends with `astRegOverlay( result, this, 0 )` (prism.c:3513), which copies
/// the Prism's own Closed, Adaptive, RegionFS, MeshSize and FillFactor — all
/// at their constructor defaults, and all unset — over whatever the reduced
/// Region carried. Skipping the Prism leaves an explicit `Closed` behind on a
/// combined uncertainty whose components reclassified through an equivalent
/// Box, and that card then shows up in the merged Region's `Unc` block.
///
/// Returns `None` if either Region lacks an uncertainty (the caller only
/// assigns an uncertainty when both are present, matching C's
/// `astTestUnc(this) && astTestUnc(reg)` guard).
pub(super) fn combine_uncertainties(
    unc_this: Option<DynRegion>,
    unc_other: Option<DynRegion>,
    this_first: bool,
) -> Option<DynRegion> {
    let (u1, u2) = (unc_this?, unc_other?);
    let (r1, r2) = if this_first { (u1, u2) } else { (u2, u1) };

    let prism = PrismBuilder::from_dyn(r1, r2)
        .and_then(|b| b.build())
        .ok()?;
    let simplified = <dyn Mapping>::simplify(&prism).ok()?;

    // `astSimplify` of a Region always yields a Region; keep the Prism itself
    // if the concrete simplified form is not a plain Region class.
    mapping_into_region(simplified).or_else(|| Some(Box::new(prism) as DynRegion))
}
