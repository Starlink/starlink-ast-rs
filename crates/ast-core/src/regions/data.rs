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
    /// The Object-class ID attribute, which a clone does not inherit.
    pub(crate) id: crate::object_id::ObjectId,
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

    /// Tracks whether `negated` was explicitly set, as C's `TestNegated`
    /// (`this->negated != -INT_MAX`). `astNegate` sets it.
    pub(crate) negated_set: bool,

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

    /// Tracks whether `adaptive` was explicitly set, as C's `TestAdaptive`
    /// (`this->adaptive != -INT_MAX`).
    pub(crate) adaptive_set: bool,

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

    /// Tracks whether `region_fs` was explicitly set, as C's `TestRegionFS`
    /// (`this->regionfs != -INT_MAX`). A constructed Region leaves it unset;
    /// a loaded one always has it set (region.c `Load`).
    pub(crate) region_fs_set: bool,

    /// Cached base-frame boundary mesh (lazily computed).
    pub(crate) cached_base_mesh: RefCell<Option<Vec<Vec<f64>>>>,

    /// Values the class derives from the Region and keeps until it changes,
    /// as C keeps them in the class structure under a `stale` flag.
    pub(crate) class_cache: RefCell<Option<ClassCache>>,
}

/// The values a Region class caches between calls (lazily computed, cleared
/// by [`RegionData::invalidate_cache`]). C copies these with the Region, so
/// a clone does too.
#[derive(Debug, Clone)]
pub(crate) enum ClassCache {
    /// interval.c `Cache`: the limits on each axis, and the equivalent Box,
    /// if the Interval has one.
    Interval {
        lbnd: Vec<f64>,
        ubnd: Vec<f64>,
        equivalent: Option<DynRegion>,
        /// The equivalent Box in the base Frame, with the Interval's Negated
        /// and Closed, which the base-Frame tests hand their work to.
        base_box: Option<Box<super::BoxRegion>>,
    },
    /// circle.c `Cache`: the radius, measured from the centre to the
    /// circumference point in the base Frame.
    Circle { radius: f64 },
    /// polygon.c `Cache`: each edge's line description, a point inside the
    /// Polygon, and whether the vertices run anti-clockwise.
    Polygon {
        edges: Vec<crate::frames::LineDef>,
        inside: [f64; 2],
        acw: bool,
    },
}

impl Clone for RegionData {
    fn clone(&self) -> Self {
        RegionData {
            frameset: self.frameset.clone(),
            ident: self.ident.clone(),
            id: Default::default(),
            mapping_flags: self.mapping_flags.clone(),
            use_defs: self.use_defs,
            points: self.points.clone(),
            negated: self.negated,
            negated_set: self.negated_set,
            closed: self.closed,
            closed_set: self.closed_set,
            mesh_size: self.mesh_size,
            mesh_size_set: self.mesh_size_set,
            fill_factor: self.fill_factor,
            fill_factor_set: self.fill_factor_set,
            adaptive: self.adaptive,
            adaptive_set: self.adaptive_set,
            uncertainty: self.uncertainty.clone(),
            dummy_fs: self.dummy_fs,
            region_fs: self.region_fs,
            region_fs_set: self.region_fs_set,
            cached_base_mesh: RefCell::new(self.cached_base_mesh.borrow().clone()),
            class_cache: RefCell::new(self.class_cache.borrow().clone()),
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
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
            use_defs: None,
            points,
            negated: false,
            negated_set: false,
            closed: true,
            closed_set: false,
            mesh_size: 0,
            mesh_size_set: false,
            fill_factor: 1.0,
            fill_factor_set: false,
            adaptive: true,
            adaptive_set: false,
            uncertainty: None,
            dummy_fs: false,
            region_fs: true,
            region_fs_set: false,
            cached_base_mesh: RefCell::new(None),
            class_cache: RefCell::new(None),
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

    /// C `astNegate`: `astSetNegated( this, !astGetNegated( this ) )`, which
    /// leaves Negated set.
    pub(crate) fn negate(&mut self) {
        self.negated = !self.negated;
        self.negated_set = true;
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

    /// Set the FillFactor and raise the set-flag, or fail with `AST__ATSER`
    /// and leave it unchanged when `value` is outside `0.0..=1.0`
    /// (region.c:12669).
    pub(crate) fn set_fill_factor(&mut self, value: f64) -> AstResult<()> {
        check_fill_factor(value)?;
        self.fill_factor = value;
        self.fill_factor_set = true;
        Ok(())
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

    /// Invalidate the cached mesh and class values (call after geometry
    /// changes).
    pub(crate) fn invalidate_cache(&self) {
        *self.cached_base_mesh.borrow_mut() = None;
        *self.class_cache.borrow_mut() = None;
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

    /// Which base-Frame points lie in the Region, allowing for Negated: what
    /// the class's `Transform` decides once the points are in the base Frame,
    /// which is where region.c `BTransform` hands them to it. The default is
    /// the test every class but Stc makes: inside the un-negated Region
    /// unless Negated, and outside wherever an axis value is bad. A class
    /// whose `Transform` decides differently overrides this.
    fn reg_base_mask(&self, base_pts: &PointSet) -> AstResult<Vec<bool>> {
        let negated = self.region_data().negated;
        let inside = self.reg_base_inside(base_pts)?;
        Ok(inside
            .iter()
            .enumerate()
            .map(|(i, &is_in)| {
                let has_bad = base_pts.point(i).iter().any(|&v| is_bad(v));
                !has_bad && (is_in != negated)
            })
            .collect())
    }

    /// Return the bounding box of the un-negated region in the base frame.
    ///
    /// Returns `(lower, upper)`, each of length `naxes`.
    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)>;

    /// Return a mesh of points on the region boundary in the base frame.
    ///
    /// The mesh should contain approximately `mesh_size` points.
    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>>;

    /// Whether this region is bounded (finite extent in all axes).
    fn is_bounded_inner(&self) -> AstResult<bool>;

    /// Remap defining points through a mapping.
    ///
    /// Called when the region's coordinate system changes (e.g., via
    /// `reg_map_region`). The implementation should transform `self.points`
    /// through the given mapping and update any derived quantities.
    fn remap_defining_points(&mut self, mapping: &dyn Mapping, forward: bool) -> AstResult<()>;
}

/// Whether two Regions' defining points are equal, as pointset.c `Equal`
/// compares them: the same number of points and axes, bad in the same
/// places, and otherwise each pair within the two values' accuracies
/// combined in quadrature. A Region's PointSet never has a PointAccuracy
/// set, so each value's accuracy is C's default, 1000 times the larger of
/// its magnitude times DBL_EPSILON and the square root of DBL_MIN.
fn points_equal(a: &[Vec<f64>], b: &[Vec<f64>]) -> bool {
    let small = crate::math::sqrt(f64::MIN_POSITIVE);
    let accuracy = |v: f64| (v.abs() * f64::EPSILON).max(small) * 1.0e3;
    a.len() == b.len()
        && a.iter().zip(b).all(|(p, q)| {
            p.len() == q.len()
                && p.iter()
                    .zip(q)
                    .all(|(&x, &y)| match (is_bad(x), is_bad(y)) {
                        (true, true) => true,
                        (false, false) => {
                            let (ax, ay) = (accuracy(x), accuracy(y));
                            (x - y).abs() <= crate::math::sqrt(ax * ax + ay * ay)
                        }
                        _ => false,
                    })
        })
}

/// region.c `Equal`, which each Region class's equality starts from:
/// `that` is the same class as `this`, with equal defining points, equal
/// base and current Frames, an equal base->current Mapping, and the same
/// Negated and Closed values as the class reports them. Returns `that` as
/// the class, for a class that goes on to compare what it holds.
pub(crate) fn region_equal<'a, R>(this: &R, that: &'a dyn Mapping) -> Option<&'a R>
where
    R: RegionGeometry + super::Region + 'static,
{
    let that = that.as_any().downcast_ref::<R>()?;
    let (fs1, fs2) = (&this.region_data().frameset, &that.region_data().frameset);
    let equal = points_equal(&this.region_data().points, &that.region_data().points)
        && Frame::equal(fs1.base(), fs2.base())
        && Frame::equal(fs1.current(), fs2.current())
        && match (
            fs1.mapping(FrameRef::Base, FrameRef::Current),
            fs2.mapping(FrameRef::Base, FrameRef::Current),
        ) {
            (Ok(m1), Ok(m2)) => m1.equal(m2.as_ref()),
            _ => false,
        }
        && this.is_negated() == that.is_negated()
        && this.is_closed() == that.is_closed();
    equal.then_some(that)
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
    clear: bool,
}

impl<'a> FrameSetting<'a> {
    /// A change to the attribute of `axis` (1-based), or to a Frame-wide
    /// attribute when `axis` is `None`, made by `apply`, which is passed the
    /// axis (0 for a Frame-wide attribute).
    pub(crate) fn new(
        axis: Option<usize>,
        apply: &'a dyn Fn(&mut dyn Frame, usize) -> AstResult<()>,
    ) -> Self {
        FrameSetting {
            axis,
            apply,
            clear: false,
        }
    }

    /// A clearing of the attribute, which C makes through `RegClearAttrib`
    /// rather than `RegSetAttrib`.
    pub(crate) fn clearing(
        axis: Option<usize>,
        apply: &'a dyn Fn(&mut dyn Frame, usize) -> AstResult<()>,
    ) -> Self {
        FrameSetting {
            axis,
            apply,
            clear: true,
        }
    }

    /// Whether this is a clearing (`RegClearAttrib`) rather than a setting.
    pub(crate) fn is_clear(&self) -> bool {
        self.clear
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
            clear: self.clear,
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
/// Fail with `AST__ATSER` unless `value` is a valid FillFactor, `0.0..=1.0`
/// (region.c:12669). The loader applies the same check, as C's `Load` sets
/// a read value through `SetFillFactor` (region.c:13536).
pub(crate) fn check_fill_factor(value: f64) -> AstResult<()> {
    if (0.0..=1.0).contains(&value) {
        return Ok(());
    }
    Err(AstError::new(
        ErrorCode::Atser,
        format!("astSetFillFactor: Invalid value ({value}) supplied for attribute FillFactor."),
    )
    .with_context("FillFactor values should be in the range 0.0 to 1.0"))
}

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
/// Frame ([`RegionGeometry::reg_base_mask`]). This is what region.c
/// `BTransform` applies to base-Frame points.
pub(crate) fn region_base_mask<R: RegionGeometry + ?Sized>(
    region: &R,
    base_pts: &PointSet,
) -> AstResult<Vec<bool>> {
    region.reg_base_mask(base_pts)
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
    if !region.is_bounded()? {
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
