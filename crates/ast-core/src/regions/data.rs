//! Shared data and internal geometry trait for all Region implementations.
//!
//! Every concrete region struct embeds a [`RegionData`] that stores the
//! encapsulated FrameSet, defining points, negation flag, and other
//! common attributes. The [`RegionGeometry`] trait provides the
//! geometry-specific hooks that each concrete region must supply.

use std::cell::RefCell;

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet, SkyFrame};
use crate::mappings::helpers::is_bad;
use crate::mappings::{Mapping, UnitMap};

use super::DynRegion;

/// The default MeshSize for a Region with the given number of axes.
///
/// Mirrors the naxes-dependent default in C `astGetMeshSize`
/// (region.c:12714): 1-D → 2, 2-D → 200, ≥3-D → 2000.
pub(crate) fn default_mesh_size(naxes: usize) -> usize {
    match naxes {
        1 => 2,
        2 => 200,
        _ => 2000,
    }
}

/// Shared data for all Region implementations.
///
/// Every concrete region struct embeds one of these. Fields are
/// `pub(crate)` so sibling modules can access them directly.
#[derive(Debug)]
pub(crate) struct RegionData {
    /// The encapsulated FrameSet: base frame holds the region geometry,
    /// current frame is the user-facing coordinate system.
    pub(crate) frameset: FrameSet,

    /// The Region's own Object-class Ident, which C does not forward to the
    /// encapsulated Frame (object.c).
    pub(crate) ident: Option<String>,
    /// The flags C keeps on the base Mapping class (mapping.c).
    pub(crate) mapping_flags: crate::mappings::MappingFlags,

    /// The Region's own UseDefs, which defaults to that of the
    /// encapsulated Frame (region.c `GetUseDefs`).
    pub(crate) use_defs: Option<bool>,

    /// Defining points in the *base* frame.
    ///
    /// Layout: `points[i]` is the i-th defining point (a Vec of length
    /// naxes). The meaning is region-specific (e.g., for Circle:
    /// points[0] = centre, points[1] = edge point).
    pub(crate) points: Vec<Vec<f64>>,

    /// Whether the region is negated (inside/outside sense flipped).
    pub(crate) negated: bool,

    /// Whether boundary points are considered interior (closed=true)
    /// or exterior (closed=false).
    pub(crate) closed: bool,

    /// Tracks whether `closed` was explicitly set (by the user, by a
    /// simplification path, or by a source dump that had `Closed = …`
    /// uncommented). Mirrors the C library's `TestClosed` predicate
    /// (`this->closed != -INT_MAX`) so dump emission can distinguish
    /// "default true" from "explicitly set" — Interval-simplified-to-Box
    /// fixtures rely on this distinction.
    pub(crate) closed_set: bool,

    /// Number of points used for boundary mesh approximation.
    ///
    /// Only meaningful when [`mesh_size_set`](Self::mesh_size_set) is true.
    /// When unset, the effective value is the naxes-dependent default from
    /// [`default_mesh_size`].
    pub(crate) mesh_size: usize,

    /// Tracks whether `mesh_size` was explicitly set. Mirrors the C
    /// library's `TestMeshSize` predicate (`this->meshsize != -INT_MAX`,
    /// region.c:12713) so dump emission is gated on the set-flag rather than
    /// on the value differing from a default.
    pub(crate) mesh_size_set: bool,

    /// Fraction of the bounding box that is interior.
    ///
    /// Only meaningful when [`fill_factor_set`](Self::fill_factor_set) is
    /// true; the default effective value is `1.0`.
    pub(crate) fill_factor: f64,

    /// Tracks whether `fill_factor` was explicitly set. Mirrors the C
    /// library's `TestFillFactor` predicate (`this->fillfactor != AST__BAD`,
    /// region.c:12650).
    pub(crate) fill_factor_set: bool,

    /// Whether adaptive meshing is enabled.
    pub(crate) adaptive: bool,

    /// The uncertainty region, if set.
    pub(crate) uncertainty: Option<DynRegion>,

    /// Whether `frameset` is a *dummy* FrameSet synthesised at load time
    /// because the serialised region carried neither `Frm` nor `FrmSet`
    /// (a nested region that inherits its container's frame). Mirrors C's
    /// `DUMMY_FS` marker (`region.c:13593-13601`). A dummy FrameSet must be
    /// replaced by the real frame via [`RegionData::set_region_fs`] before
    /// the region is used; the parent compound loader performs that
    /// re-association.
    pub(crate) dummy_fs: bool,

    /// The C `RegionFS` attribute (`region.c:12603-12607`): non-zero means
    /// "include the Frame in the dump". Default (unset) is treated as 1.
    ///
    /// This is distinct from [`dummy_fs`](Self::dummy_fs): a frame-less
    /// nested region loads with `region_fs = false`, and although
    /// `set_region_fs` later gives it a real FrameSet (clearing `dummy_fs`),
    /// `region_fs` stays false — so on re-dump the Frame is still omitted,
    /// matching C's `astSetRegFS` leaving `regionfs` untouched
    /// (`region.c:10738-10747`) and the unit-map dump branch writing `Frm`
    /// only when `regionfs != 0` (`region.c:13099-13107`).
    pub(crate) region_fs: bool,

    /// Cached base-frame boundary mesh (lazily computed).
    pub(crate) cached_base_mesh: RefCell<Option<Vec<Vec<f64>>>>,
}

impl Clone for RegionData {
    fn clone(&self) -> Self {
        RegionData {
            frameset: self.frameset.clone(),
            ident: None,
            mapping_flags: crate::mappings::MappingFlags::default(),
            use_defs: None,
            points: self.points.clone(),
            negated: self.negated,
            closed: self.closed,
            closed_set: self.closed_set,
            mesh_size: self.mesh_size,
            mesh_size_set: self.mesh_size_set,
            fill_factor: self.fill_factor,
            fill_factor_set: self.fill_factor_set,
            adaptive: self.adaptive,
            uncertainty: self.uncertainty.clone(),
            dummy_fs: self.dummy_fs,
            region_fs: self.region_fs,
            cached_base_mesh: RefCell::new(self.cached_base_mesh.borrow().clone()),
        }
    }
}

/// The Frame a Region encapsulates, given the Frame its constructor was
/// supplied (`region.c:13311-13323`).
///
/// A Region contributes the current Frame of its own FrameSet, a FrameSet its
/// current Frame, and any other Frame is used as supplied.
fn resolve_supplied_frame(frame: &dyn Frame) -> DynFrame {
    if let Some(region) = crate::regions::any_as_region(frame.as_any()) {
        region.get_region_frame()
    } else if let Some(fs) = frame.as_any().downcast_ref::<FrameSet>() {
        fs.current().snapshot()
    } else {
        frame.snapshot()
    }
}

impl RegionData {
    /// Create a new `RegionData` with default attribute values.
    ///
    /// - `frameset`: the encapsulated FrameSet (base = geometry frame,
    ///   current = user frame).
    /// - `points`: defining points in the base frame.
    pub(crate) fn new(frameset: FrameSet, points: Vec<Vec<f64>>) -> Self {
        let mut fs = frameset;
        // Ensure 2-frame structure: base + current connected by UnitMap.
        // This matches the C library and ensures with_integrity works
        // (fuse_into_current_edges needs at least one edge to compose the delta).
        //
        // A single-frame FrameSet is the frame a Region constructor was handed,
        // which C resolves before storing: a Region contributes the Frame it
        // encapsulates and a FrameSet its current Frame (region.c:13311-13323).
        if fs.nframe() == 1 {
            let resolved = resolve_supplied_frame(fs.current());
            let naxes = resolved.naxes();
            fs = FrameSet::new(resolved);
            let current_snap = fs.current().snapshot();
            fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(naxes)), current_snap)
                .expect("adding current frame to region FrameSet");
        }
        RegionData {
            frameset: fs,
            ident: None,
            mapping_flags: crate::mappings::MappingFlags::default(),
            use_defs: None,
            points,
            negated: false,
            closed: true,
            closed_set: false,
            mesh_size: 0,
            mesh_size_set: false,
            fill_factor: 1.0,
            fill_factor_set: false,
            adaptive: true,
            uncertainty: None,
            dummy_fs: false,
            region_fs: true,
            cached_base_mesh: RefCell::new(None),
        }
    }

    /// Replace this region's (dummy) FrameSet with a real one built from
    /// `frame`, mirroring C `astSetRegFS` (`region.c:10674`).
    ///
    /// The new FrameSet holds two independent copies of `frame` connected
    /// by a `UnitMap` (base = current = the supplied frame), and the
    /// `dummy_fs` flag is cleared. If this region carries an uncertainty
    /// region that is itself dummy-FS, it is recursively re-associated
    /// with the same frame (C `region.c:10745-10751`).
    pub(crate) fn set_region_fs(&mut self, frame: &dyn Frame) {
        let naxes = frame.naxes();
        let mut fs = FrameSet::new(frame.snapshot());
        let current_snap = frame.snapshot();
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(naxes)), current_snap)
            .expect("adding current frame to region FrameSet");
        self.frameset = fs;
        self.dummy_fs = false;
        self.invalidate_cache();

        // Recursively fix a dummy-FS uncertainty region with the same frame.
        if let Some(unc) = self.uncertainty.as_mut()
            && unc.has_dummy_fs()
        {
            unc.set_region_fs(frame);
        }
    }

    /// Set the boolean `RegionFS` attribute (C `astSetRegionFS`,
    /// region.c:12603). When `false`, the encapsulated FrameSet is omitted
    /// from a dump. Unlike [`set_region_fs`](Self::set_region_fs), this does
    /// not install a new FrameSet — it only toggles the dump flag.
    pub(crate) fn set_region_fs_flag(&mut self, value: bool) {
        self.region_fs = value;
    }

    /// The effective MeshSize: the explicitly-set value if any, otherwise
    /// the naxes-dependent default (`astGetMeshSize`, region.c:12714): 1-D →
    /// 2, 2-D → 200, ≥3-D → 2000. The naxes count is that of the current
    /// Frame.
    pub(crate) fn effective_mesh_size(&self) -> usize {
        if self.mesh_size_set {
            self.mesh_size
        } else {
            default_mesh_size(self.frameset.current().naxes())
        }
    }

    /// The effective FillFactor: the explicitly-set value if any, otherwise
    /// `1.0` (`astGetFillFactor`, region.c:12648).
    pub(crate) fn effective_fill_factor(&self) -> f64 {
        if self.fill_factor_set {
            self.fill_factor
        } else {
            1.0
        }
    }

    /// Set the MeshSize, mirroring C `SetMeshSize` (region.c:12712): values
    /// are clamped to be greater than 5, and the set-flag is raised.
    pub(crate) fn set_mesh_size(&mut self, value: usize) {
        self.mesh_size = if value > 5 { value } else { 5 };
        self.mesh_size_set = true;
        self.invalidate_cache();
    }

    /// Set the FillFactor and raise the set-flag. Callers supply a value in
    /// the C `SetFillFactor` valid range `0.0..=1.0` (region.c:12651).
    pub(crate) fn set_fill_factor(&mut self, value: f64) {
        self.fill_factor = value;
        self.fill_factor_set = true;
    }

    /// Clear the MeshSize attribute, mirroring C `astClearMeshSize`.
    pub(crate) fn clear_mesh_size(&mut self) {
        self.mesh_size_set = false;
        self.invalidate_cache();
    }

    /// Clear the FillFactor attribute, mirroring C `astClearFillFactor`.
    pub(crate) fn clear_fill_factor(&mut self) {
        self.fill_factor_set = false;
    }

    /// Invalidate the cached mesh (call after geometry changes).
    #[allow(dead_code)]
    pub(crate) fn invalidate_cache(&self) {
        *self.cached_base_mesh.borrow_mut() = None;
    }

    /// The boundary-strictness that
    /// [`RegionGeometry::reg_base_inside`](super::RegionGeometry::reg_base_inside)
    /// must use, given that its caller applies the Negated attribute afterwards.
    ///
    /// In C, a point exactly on the boundary belongs to the region iff Closed,
    /// *independent of Negated* — a region and its negation both contain their
    /// shared boundary. Each class encodes that invariant differently:
    /// `polygon.c:6423-6444` states it directly, assigning `in_region = closed`
    /// for an `ON` point in both the negated and un-negated arms, while
    /// `circle.c:2078-2092`, `ellipse.c:2196-2210`, `box.c:4225/4263` (via the
    /// `astAxIn` strictness argument) and `interval.c:3666-3890` reach the same
    /// result by pairing each arm with a complementary comparison.
    ///
    /// `reg_base_inside` reports membership of the *un-negated* region and
    /// [`region_transform`] inverts it when Negated is set. Boolean inversion is
    /// correct for the interior but would move the boundary to the wrong side,
    /// so the boundary must be reported inverted here in order to come back out
    /// the right way — hence Closed XOR Negated rather than Closed.
    ///
    /// This is only ever the right question for `reg_base_inside`; the Closed
    /// attribute itself is [`RegionGeometry::is_closed`](super::RegionGeometry::is_closed).
    pub(crate) fn closed_for_base_inside(&self) -> bool {
        self.closed != self.negated
    }

    /// Whether a closed-curve region (Circle, Ellipse, Box, …) is bounded.
    ///
    /// Faithful port of the base `Region::GetBounded` (region.c:3358): a
    /// region defined by one or more closed curves is bounded so long as it
    /// has not been negated. The exception is a region defined within a
    /// SkyFrame (i.e. on a sphere), which is always bounded because a finite
    /// region has a finite negation.
    pub(crate) fn closed_curve_bounded(&self) -> bool {
        if self.frameset.base().as_any().is::<SkyFrame>() {
            true
        } else {
            !self.negated
        }
    }

    /// Permute the axes of the encapsulated FrameSet's current frame.
    ///
    /// Delegates to [`FrameSet::perm_axes`] which permutes the current
    /// frame and composes a PermMap onto the edge mapping.
    pub(crate) fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.frameset.perm_axes(perm)?;
        self.invalidate_cache();
        Ok(())
    }
}

/// Internal trait providing geometry-specific hooks for a Region.
///
/// Each concrete region implements this to supply its shape-specific
/// logic. The shared infrastructure in `RegionData` and the default
/// `Region` method implementations delegate to these hooks.
#[allow(dead_code)]
pub(crate) trait RegionGeometry {
    /// Access the shared region data.
    fn region_data(&self) -> &RegionData;

    /// Mutable access to the shared region data.
    fn region_data_mut(&mut self) -> &mut RegionData;

    /// Batch predicate: test which points (in the *base* frame) are inside
    /// the un-negated region boundary.
    ///
    /// `pts` is a PointSet with coordinates in the base frame. Returns a
    /// Vec<bool> with one entry per point (true = inside raw geometry).
    /// The negation flag is handled by the caller.
    ///
    /// This is the primary performance-critical method. Implementations
    /// should process all points in a tight loop to allow vectorisation.
    fn reg_base_inside(&self, pts: &PointSet) -> AstResult<Vec<bool>>;

    /// Test whether a single point (in the *base* frame) is inside the
    /// un-negated region boundary.
    ///
    /// Default implementation calls `reg_base_inside` with a 1-point PointSet.
    /// Override for efficiency if single-point queries are common.
    fn reg_base_inside_point(&self, point: &[f64]) -> AstResult<bool> {
        let pts = PointSet::from_rows(std::iter::once(point));
        let result = self.reg_base_inside(&pts)?;
        Ok(result[0])
    }

    /// Return the bounding box of the un-negated region in the base frame.
    ///
    /// Returns `(lower, upper)`, each of length `naxes`.
    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)>;

    /// Return a mesh of points on the region boundary in the base frame.
    ///
    /// The mesh should contain approximately `mesh_size` points.
    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>>;

    /// Return the centre of the region in the base frame.
    fn reg_centre(&self) -> AstResult<Vec<f64>>;

    /// Whether this region is bounded (finite extent in all axes).
    fn is_bounded_inner(&self) -> bool;

    /// Remap defining points through a mapping.
    ///
    /// Called when the region's coordinate system changes (e.g., via
    /// `map_region`). The implementation should transform `self.points`
    /// through the given mapping and update any derived quantities.
    fn remap_defining_points(&mut self, mapping: &dyn Mapping, forward: bool) -> AstResult<()>;

    /// Verify that a candidate remapped Region (defined in this Region's
    /// *current* Frame by the boundary points `candidate_mesh`) is an exact
    /// representation of `self` after the base→current `map`.
    ///
    /// Mirrors the inverse-direction check in `box.c::Simplify`
    /// (3905-3907): map the candidate's boundary mesh back into this
    /// Region's base Frame with the inverse of `map`, then verify every
    /// point lies on `self`'s boundary (its `reg_base_inside` is true for a closed
    /// Region). When the base->current Mapping does not preserve
    /// axis-alignment (e.g. a shear MatrixMap), a candidate axis-aligned Box
    /// or Interval is strictly larger than the true region, so some
    /// back-mapped boundary points fall outside `self` and the check fails —
    /// signalling that no simplification is possible and the original Region
    /// (with its FrameSet) must be returned unchanged.
    fn verify_remap_exact(
        &self,
        map: &dyn Mapping,
        candidate_mesh: &[Vec<f64>],
    ) -> AstResult<bool> {
        if candidate_mesh.is_empty() {
            // No boundary to test (e.g. an unbounded candidate) — cannot
            // confirm exactness, so treat as not simplifiable.
            return Ok(false);
        }
        let mesh = PointSet::from_rows(candidate_mesh.iter().map(|p| p.as_slice()));
        // Inverse-transform the candidate boundary into the base Frame.
        let base_mesh = map.transform(&mesh, false)?;
        let pins = self.reg_base_inside(&base_mesh)?;
        Ok(pins.iter().all(|&inside| inside))
    }
}

/// One change to a Frame attribute of a Region: what C's `RegSetAttrib` and
/// `RegClearAttrib` receive as a setting string or attribute name.
///
/// The change is applied to several Frames (the Region's current and base
/// Frames, its uncertainty, a compound Region's components), and an axis
/// index in it is translated for each, so it is a reusable closure that is
/// handed the 1-based axis to use in the Frame it is applied to.
#[doc(hidden)]
pub struct FrameSetting<'a> {
    axis: Option<usize>,
    apply: &'a dyn Fn(&mut dyn Frame, usize) -> AstResult<()>,
}

impl<'a> FrameSetting<'a> {
    /// A change to the attribute of `axis` (1-based), or to a Frame-wide
    /// attribute when `axis` is `None`, made by `apply`, which is passed the
    /// axis (0 for a Frame-wide attribute).
    pub(crate) fn new(
        axis: Option<usize>,
        apply: &'a dyn Fn(&mut dyn Frame, usize) -> AstResult<()>,
    ) -> Self {
        FrameSetting { axis, apply }
    }

    /// The 1-based axis the attribute belongs to, if any.
    pub(crate) fn axis(&self) -> Option<usize> {
        self.axis
    }

    /// The same change, to the attribute of `axis` instead.
    pub(crate) fn with_axis(&self, axis: Option<usize>) -> FrameSetting<'a> {
        FrameSetting {
            axis,
            apply: self.apply,
        }
    }

    /// Make the change to `frame`.
    pub(crate) fn apply_to(&self, frame: &mut dyn Frame) -> AstResult<()> {
        (self.apply)(frame, self.axis.unwrap_or(0))
    }
}

/// Whether an error is C's AST__BADAT, the "attribute unknown" error that
/// `RegSetAttrib` and its overrides annul when a Frame or component does
/// not have the attribute.
pub(crate) fn is_bad_attribute(err: &AstError) -> bool {
    err.code() == ErrorCode::Badat
}

/// region.c `RegSetAttrib` and `RegClearAttrib`: make `setting` to the
/// current Frame and then the base Frame of `region`, neither remapping the
/// other, so the Region keeps its defining points in the changed Frames.
///
/// An attribute of a current-Frame axis is made to the base-Frame axis the
/// base->current Mapping connects it to, and it is an error if there is no
/// single such axis. The change is also made to any uncertainty Region,
/// whose current Frame is the Region's base Frame. An attribute the base
/// Frame or the uncertainty does not have (AST__BADAT) is ignored there.
/// Returns the setting as made to the base Frame, for a class that passes
/// it on to Regions it holds.
pub(crate) fn region_frame_attrib<'a, R>(
    region: &mut R,
    setting: &FrameSetting<'a>,
) -> AstResult<FrameSetting<'a>>
where
    R: RegionGeometry + super::Region,
{
    setting.apply_to(
        region
            .region_data_mut()
            .frameset
            .frame_mut(FrameRef::Current)?,
    )?;

    // The base-Frame axis connected to the current-Frame axis.
    let base_setting = match setting.axis() {
        None => setting.with_axis(None),
        Some(axis) => {
            let map = region
                .region_data()
                .frameset
                .mapping(FrameRef::Current, FrameRef::Base)?;
            match map.map_split_zero_based(&[axis - 1])? {
                Some((outs, split)) if split.nout() == 1 => setting.with_axis(Some(outs[0] + 1)),
                _ => {
                    return Err(AstError::new(
                        ErrorCode::Inter,
                        format!(
                            "astRegSetAttrib(Region): Unable to apply the attribute setting to \
                             the base Frame in the Region. There is no base Frame axis \
                             corresponding to current Frame axis {axis}"
                        ),
                    ));
                }
            }
        }
    };

    // The base Frame, then the uncertainty; C skips the uncertainty once the
    // base Frame has reported an error, and then annuls AST__BADAT.
    let made = base_setting
        .apply_to(
            region
                .region_data_mut()
                .frameset
                .frame_mut(FrameRef::Base)?,
        )
        .and_then(|()| {
            if region.test_unc() {
                let mut unc = region.get_unc_frm(FrameRef::Base)?;
                unc.reg_frame_attrib(&base_setting)?;
                region.store_uncertainty(Some(unc));
            }
            Ok(())
        });
    match made {
        Err(e) if !is_bad_attribute(&e) => return Err(e),
        _ => {}
    }

    region.region_data().invalidate_cache();
    Ok(base_setting)
}

/// Make `setting` to a Region as C's public `astSet` and `astClear` do
/// (region.c `SetAttrib`, `ClearAttrib`): through the FrameSet, remapping
/// the Region into the changed current Frame, when the Region is Adaptive,
/// and otherwise through [`Region::reg_frame_attrib`](super::Region::reg_frame_attrib),
/// which changes both Frames and leaves the Region where it was.
pub(crate) fn set_region_frame_attr<R>(region: &mut R, setting: &FrameSetting) -> AstResult<()>
where
    R: RegionGeometry + super::Region,
{
    if region.region_data().adaptive {
        let data = region.region_data_mut();
        data.frameset.with_integrity(|f| setting.apply_to(f))?;
        data.invalidate_cache();
    } else {
        region.reg_frame_attrib(setting)?;
    }
    Ok(())
}

/// A Frame-wide change made by `f`, applied to a Region as
/// [`set_region_frame_attr`] applies it: the inherent `with_integrity` of
/// every Region class.
pub(crate) fn with_region_integrity<R, F>(region: &mut R, f: F) -> AstResult<()>
where
    R: RegionGeometry + super::Region,
    F: Fn(&mut dyn Frame) -> AstResult<()>,
{
    let apply = |frame: &mut dyn Frame, _: usize| f(frame);
    set_region_frame_attr(region, &FrameSetting::new(None, &apply))
}

/// Shared implementation of `Mapping::transform` for all Regions.
///
/// Points inside the region pass through unchanged; points outside are
/// set to [`BAD_VALUE`]. The negation flag is respected. Points with
/// any BAD input coordinate always produce BAD output.
///
/// The forward and inverse transformations are identical, as every C
/// Region class says of its `Transform`: interior points pass through and
/// exterior points become BAD in either direction.
///
/// Uses the batch `reg_base_inside` predicate for efficient processing of
/// large point sets (tens of thousands of points at a time).
pub(crate) fn region_transform<R: RegionGeometry>(
    region: &R,
    pts: &PointSet,
) -> AstResult<PointSet> {
    let npoint = pts.npoint();
    let ncoord = pts.ncoord();

    // Batch-test all points for membership.
    let data = region.region_data();

    // Transform input points from current frame to base frame for testing.
    // The region's frameset maps base→current (forward), so we need inverse.
    let base_pts = data.frameset.transform(pts, false)?;

    let inside = region_base_mask(region, &base_pts)?;

    // Build output: passthrough for inside, BAD for outside.
    let mut out = PointSet::zeros(npoint, ncoord);
    for (i, &effective_inside) in inside.iter().enumerate() {
        if effective_inside {
            let src = pts.point(i);
            let dst = out.point_mut(i);
            dst.copy_from_slice(src);
        } else {
            let dst = out.point_mut(i);
            dst.fill(BAD_VALUE);
        }
    }

    Ok(out)
}

/// Which base-Frame points lie in the Region, allowing for Negated: the
/// test the Region's transformation makes once its points are in the base
/// Frame. A point with a bad axis value is outside. This is what region.c
/// `BTransform` applies to base-Frame points.
pub(crate) fn region_base_mask<R: RegionGeometry + ?Sized>(
    region: &R,
    base_pts: &PointSet,
) -> AstResult<Vec<bool>> {
    let negated = region.region_data().negated;
    let inside = region.reg_base_inside(base_pts)?;
    Ok(inside
        .iter()
        .enumerate()
        .map(|(i, &is_in)| {
            let has_bad = base_pts.point(i).iter().any(|&v| is_bad(v));
            !has_bad && (is_in != negated)
        })
        .collect())
}

/// region.c `RegBaseGrid`: a grid of about MeshSize points spread through
/// the Region in its base Frame. A Box covering the Region's base-Frame
/// bounding box makes a grid (its own `RegBaseGrid`), and the points
/// outside the Region are dropped. The Box's grid size is adjusted by the
/// ratio by which the count missed MeshSize, and the grid is remade, until
/// the count is within 5% of MeshSize or three grids have been made. Box
/// has its own `RegBaseGrid` and does not come here.
pub(crate) fn region_base_grid<R>(region: &R) -> AstResult<PointSet>
where
    R: RegionGeometry + crate::regions::Region + ?Sized,
{
    if !region.is_bounded() {
        return Err(AstError::new(
            crate::error::ErrorCode::Mbbnf,
            "astRegBaseGrid: The Region is unbounded so no grid can be created to \
             cover it.",
        ));
    }
    let data = region.region_data();
    let meshsize = data.effective_mesh_size() as i32;
    let (lbnd, ubnd) = region.reg_base_box()?;
    let frmb = data.frameset.base().snapshot();
    let mut bx = crate::regions::BoxRegionBuilder::from_dyn_bounds(frmb, &lbnd, &ubnd)?.build()?;
    let mut np = meshsize;
    let mut result = PointSet::zeros(0, lbnd.len());
    let mut ntry = 0;
    while ntry < 3 {
        ntry += 1;
        crate::regions::Region::set_mesh_size(&mut bx, np as usize);
        let ps1 = crate::regions::Region::get_region_grid(&bx)?;
        let inside = region_base_mask(region, &ps1)?;
        let kept: Vec<usize> = (0..ps1.npoint()).filter(|&i| inside[i]).collect();
        let ipr = kept.len() as i32;
        result = PointSet::zeros(kept.len(), lbnd.len());
        for (k, &i) in kept.iter().enumerate() {
            result.point_mut(k).copy_from_slice(ps1.point(i));
        }
        if ((ipr - meshsize) as f64).abs() / (meshsize as f64) < 0.05 {
            break;
        }
        if ntry < 3 {
            np = if ipr == 0 {
                np * 10
            } else {
                // C's `np *= (double)meshsize/(double)ipr` truncates.
                (np as f64 * (meshsize as f64 / ipr as f64)) as i32
            };
        }
    }
    // C truncates the grid with astSetNpoint, which rejects a count of 0.
    if result.npoint() == 0 {
        return Err(AstError::new(
            crate::error::ErrorCode::Nptin,
            "astSetNpoint(PointSet): Number of points (0) is not valid.",
        ));
    }
    Ok(result)
}
