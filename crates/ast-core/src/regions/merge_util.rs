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

use crate::frames::FrameSet;
use crate::mappings::{CmpMap, Composition, DynMapping, Mapping};

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

/// Finish a cross-type Region merge: build the concrete merged Region in
/// the combined `frameset`, propagate the `this`-side attributes, clear
/// the Negated flag, attach the combined uncertainty, apply the
/// FillFactor/MeshSize scaling, and (for an Interval result) re-simplify so
/// a fully-bounded Interval reclassifies to a Box.
///
/// Mirrors the shared tail of `MergeInterval`/`MergeBox`
/// (interval.c:1537, box.c:1983): `astRegOverlay(new, this, 1)` copies
/// Negated/Closed/Adaptive from `this`, `astClearNegated` then forces the
/// result un-negated, `astSetUnc` assigns the combined uncertainty, and the
/// FillFactor/MeshSize scaling block (box.c:2049-2085) sets the merged
/// FillFactor to the product of the two and the merged MeshSize to the
/// result default scaled by the product of each source's used/default
/// ratio.
pub(super) fn finish_region_merge(
    built: MergedRegion,
    frameset: FrameSet,
    this_closed: bool,
    this_closed_set: bool,
    this_adaptive: bool,
    uncertainty: Option<DynRegion>,
    scaling: MergeScaling,
) -> Option<DynMapping> {
    let apply_attrs = |data: &mut RegionData| {
        // astRegOverlay copies Closed and Adaptive from `this`; Negated is
        // then cleared (the merged Region is always un-negated).
        data.closed = this_closed;
        data.closed_set = this_closed_set;
        data.adaptive = this_adaptive;
        data.negated = false;
        data.uncertainty = uncertainty.clone();

        apply_merge_scaling(data, &scaling);
    };

    match built {
        MergedRegion::Interval(lbnd, ubnd) => {
            let mut data = RegionData::new(frameset, vec![lbnd, ubnd]);
            apply_attrs(&mut data);
            let merged = Interval { data };
            // A fully-bounded Interval reclassifies to a Box (interval.c
            // Cache / Simplify); an unbounded one stays an Interval.
            <dyn Mapping>::simplify(&merged).ok()
        }
        MergedRegion::Box(centre, corner) => {
            let mut data = RegionData::new(frameset, vec![centre, corner]);
            apply_attrs(&mut data);
            Some(Box::new(BoxRegion { data }))
        }
        MergedRegion::PointList(points) => {
            let mut data = RegionData::new(frameset, points);
            apply_attrs(&mut data);
            Some(Box::new(PointList { data }))
        }
    }
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
/// (interval.c:1547, box.c:1993): build a Prism from the two base-Frame
/// uncertainties in the correct order, then simplify it. Rather than
/// calling `Prism::simplified` (which does not yet fuse Box+Box
/// components), we follow `prism.c::Simplify` (3305-3336) directly:
/// combine the two uncertainty Regions into a parallel CmpMap, simplify
/// that, and if the result is a Region use it; otherwise fall back to an
/// unsimplified Prism (transform-equivalent — a later simplify pass
/// reduces it).
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

    // Try prism.c::Simplify's route first: a parallel CmpMap of the two
    // uncertainty Regions, simplified. If the two same-class boxes merge,
    // the result is a single Region of the combined dimensionality.
    if let Ok(cmp) = CmpMap::new(r1.clone_region(), r2.clone_region(), Composition::Parallel)
        && let Ok(simp) = <dyn Mapping>::simplify(&cmp)
        && let Some(region) = mapping_into_region(simp)
    {
        return Some(region);
    }

    // Fall back to an unsimplified Prism.
    PrismBuilder::from_dyn(r1, r2)
        .and_then(|b| b.build())
        .ok()
        .map(|p| Box::new(p) as DynRegion)
}
