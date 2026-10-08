//! Box region: an axis-aligned box defined by a centre and a corner.
//!
//! A [`BoxRegion`] represents a rectangular area with sides parallel to
//! the axes of a [`Frame`](crate::frames::Frame). It is defined by a
//! centre point and one corner point; the box extends symmetrically
//! around the centre so the opposite corner is the mirror of the
//! specified corner through the centre.
//!
//! Named `BoxRegion` (rather than `Box`) to avoid collision with
//! [`std::boxed::Box`]. In the C library this class is called "Box".

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{AxisKind, DynFrame, Frame, FrameRef, FrameSet, simplify_frame_via_mapping};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, Mapping, PermMap, UnitMap, ViaSimplify};

use super::data::{RegionData, RegionGeometry};
use super::merge_util::MergedRegion;
use super::{DynRegion, Interval, NullRegion, NullRegionBuilder, PointList, Region};

/// An axis-aligned box region within a coordinate frame.
///
/// Defined by a centre point and a corner point in the base frame.
/// The box extends symmetrically around the centre: the effective
/// bounds are `[2*centre - corner, corner]` per axis (with appropriate
/// ordering so that `lbnd <= ubnd`).
#[derive(Debug, Clone)]
pub struct BoxRegion {
    /// Shared region data (frameset, defining points, flags).
    pub(crate) data: RegionData,
}

/// Builder for constructing a [`BoxRegion`].
pub struct BoxRegionBuilder {
    frame: DynFrame,
    centre: Vec<f64>,
    corner: Vec<f64>,
    uncertainty: Option<DynRegion>,
}

impl BoxRegionBuilder {
    /// Create a builder for a BoxRegion in the given frame from a centre
    /// point and one corner.
    ///
    /// `centre` and `corner` must each have length equal to `frame.naxes()`.
    /// The box extends symmetrically around `centre`, so the opposite
    /// corner is at `2*centre - corner`.
    pub fn new(frame: impl Frame + 'static, centre: Vec<f64>, corner: Vec<f64>) -> AstResult<Self> {
        let naxes = frame.naxes();
        if centre.len() != naxes {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "BoxRegion centre has {} coordinates but frame has {} axes",
                    centre.len(),
                    naxes
                ),
            ));
        }
        if corner.len() != naxes {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "BoxRegion corner has {} coordinates but frame has {} axes",
                    corner.len(),
                    naxes
                ),
            ));
        }
        Ok(BoxRegionBuilder {
            frame: Box::new(frame),
            centre,
            corner,
            uncertainty: None,
        })
    }

    /// Create a builder from explicit lower and upper bounds.
    ///
    /// The centre is computed as the midpoint `(lbnd + ubnd) / 2` and
    /// the corner is set to `ubnd`. Both must have length equal to
    /// `frame.naxes()`.
    pub fn from_bounds(frame: impl Frame + 'static, lbnd: &[f64], ubnd: &[f64]) -> AstResult<Self> {
        Self::from_dyn_bounds(Box::new(frame), lbnd, ubnd)
    }

    /// [`from_bounds`](Self::from_bounds) for a Frame already boxed, as C's
    /// `astBox( frame, 1, lbnd, ubnd, ... )` takes any Frame.
    pub(crate) fn from_dyn_bounds(frame: DynFrame, lbnd: &[f64], ubnd: &[f64]) -> AstResult<Self> {
        let naxes = frame.naxes();
        if lbnd.len() != naxes {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "BoxRegion lower bounds have {} values but frame has {} axes",
                    lbnd.len(),
                    naxes
                ),
            ));
        }
        if ubnd.len() != naxes {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "BoxRegion upper bounds have {} values but frame has {} axes",
                    ubnd.len(),
                    naxes
                ),
            ));
        }
        let centre: Vec<f64> = lbnd
            .iter()
            .zip(ubnd.iter())
            .map(|(l, u)| 0.5 * (l + u))
            .collect();
        let corner: Vec<f64> = ubnd.to_vec();

        Ok(BoxRegionBuilder {
            frame,
            centre,
            corner,
            uncertainty: None,
        })
    }

    /// Set an uncertainty region.
    pub fn uncertainty(mut self, unc: DynRegion) -> Self {
        self.uncertainty = Some(unc);
        self
    }

    /// Build the BoxRegion.
    pub fn build(self) -> AstResult<BoxRegion> {
        let frameset = FrameSet::new(self.frame);

        // Defining points: points[0] = centre, points[1] = corner.
        let points = vec![self.centre, self.corner];
        let data = RegionData::new(frameset, points);
        let mut region = BoxRegion { data };

        // C's constructor routes a construction-time uncertainty through
        // astSetUnc (region.c:13372), which remaps it into this Region's base
        // Frame, negates it if it is unbounded, omits its FrameSet from the dump
        // when its base->current Mapping is a UnitMap, and re-centres it on this
        // Region's first defining point.
        if let Some(unc) = self.uncertainty {
            region.set_uncertainty(unc);
        }

        Ok(region)
    }
}

// ============================================================================
// Integrity
// ============================================================================

impl BoxRegion {
    /// Build a `BoxRegion` directly from an owned `DynFrame` and explicit
    /// lower/upper bounds, without an intervening `impl Frame` type parameter.
    ///
    /// Used internally where the base Frame is only available as a boxed trait
    /// object (e.g. constructing a default uncertainty Box from a Region's own
    /// base Frame). The centre is the midpoint of the bounds and the corner is
    /// the upper bound, matching [`BoxRegionBuilder::from_bounds`].
    pub(crate) fn from_bounds_frame(
        frame: DynFrame,
        lbnd: &[f64],
        ubnd: &[f64],
    ) -> AstResult<BoxRegion> {
        let centre: Vec<f64> = lbnd
            .iter()
            .zip(ubnd.iter())
            .map(|(l, u)| 0.5 * (l + u))
            .collect();
        let corner: Vec<f64> = ubnd.to_vec();
        Self::from_corner_frame(frame, centre, corner)
    }

    /// Build a `BoxRegion` directly from an owned `DynFrame`, a centre, and a
    /// corner (the native Box parameterisation, `astBox` with `form=0`).
    pub(crate) fn from_corner_frame(
        frame: DynFrame,
        centre: Vec<f64>,
        corner: Vec<f64>,
    ) -> AstResult<BoxRegion> {
        let frameset = FrameSet::new(frame);
        let data = RegionData::new(frameset, vec![centre, corner]);
        Ok(BoxRegion { data })
    }

    /// Mutate the current frame with region integrity enforcement.
    ///
    /// When adaptive (the default), the FrameSet's base-to-current mapping is
    /// updated to account for the change, and defining points remain valid
    /// in the base frame. When non-adaptive, the change is applied to both
    /// frames without remapping.
    ///
    /// `f` makes the change to each Frame it is made to, so it may be called
    /// more than once.
    pub fn with_integrity<F>(&mut self, f: F) -> AstResult<()>
    where
        F: Fn(&mut dyn Frame) -> AstResult<()>,
    {
        super::data::with_region_integrity(self, f)
    }

    /// Permute the axes of this region's current frame.
    pub fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.data.perm_axes(perm)
    }

    /// Attempt to split this Box into two sub-regions on distinct current-Frame
    /// axis subsets and join them into an equivalent, simplified Prism.
    ///
    /// Faithful port of C `astConvertToPrism_` (prism.c:232). Scans every
    /// non-empty proper subset of the current-Frame axes; for the first subset
    /// where both the selected and the complementary axes pick out same-class
    /// sub-Regions (via [`pick_axes_region`](super::pick_axes_region), i.e.
    /// `astPickAxes` splitting the current→base Mapping) and at least one of
    /// those sub-Regions simplifies, it forms a `Prism` of the two simplified
    /// halves, permutes its axes back into the original Box order, remaps it
    /// into the Box's current Frame, and simplifies the Prism.
    ///
    /// Returns `None` when no subset yields a Prism (the caller then keeps the
    /// original Box). The load-bearing guard `sp1 != pickfrm1 || sp2 != pickfrm2`
    /// (at least one half must actually simplify) is modelled by structural
    /// [`Mapping::equal`] comparison, matching C's pointer-inequality test.
    fn convert_to_prism(&self) -> AstResult<Option<DynRegion>> {
        use super::{PickedAxes, PrismBuilder, pick_axes_region};
        use crate::mappings::UnitMap;

        // Mapping from base to current Frame; nout = number of current-Frame
        // axes (Mapping outputs).
        let map = self
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;
        let nout = map.nout();

        // Scan all subsets of the current-Frame axes (the binary pattern of
        // "bitmask" marks the axes in the subset), stopping at the first
        // subset that yields a Prism.
        let topmask: u32 = 1u32 << nout;
        for bitmask in 1..topmask {
            // Indices of the axes forming the selected subset.
            let ax_sel: Vec<usize> = (0..nout).filter(|&a| bitmask & (1 << a) != 0).collect();

            // Pick the selected axes; proceed only if they form a Region.
            let PickedAxes::Region(pickfrm1) = pick_axes_region(self, &ax_sel)? else {
                continue;
            };

            // Indices of the remaining (unselected) axes.
            let ax_comp: Vec<usize> = (0..nout).filter(|&a| bitmask & (1 << a) == 0).collect();

            // When the subset spans every axis the complement is empty; C's
            // astPickAxes builds a 0-axis Frame there, which astIsARegion
            // rejects, so no Prism forms. Skip it (Rust's Frame builder
            // rejects 0 axes outright).
            if ax_comp.is_empty() {
                continue;
            }

            // Pick the complementary axes; proceed only if they too form a
            // Region.
            let PickedAxes::Region(pickfrm2) = pick_axes_region(self, &ax_comp)? else {
                continue;
            };

            // See if either picked Region can be simplified.
            let sp1_map = <dyn Mapping>::simplify(pickfrm1.as_ref())?;
            let sp2_map = <dyn Mapping>::simplify(pickfrm2.as_ref())?;
            let changed1 = !sp1_map.equal(pickfrm1.as_ref() as &dyn Mapping);
            let changed2 = !sp2_map.equal(pickfrm2.as_ref() as &dyn Mapping);
            if !(changed1 || changed2) {
                continue;
            }

            let sp1 = super::merge_util::mapping_into_region(sp1_map).unwrap_or(pickfrm1);
            let sp2 = super::merge_util::mapping_into_region(sp2_map).unwrap_or(pickfrm2);

            // Form a Prism containing the simplified Regions (selected axes
            // first, complementary axes second).
            let mut prism: DynRegion = Box::new(PrismBuilder::from_dyn(sp1, sp2)?.build()?);

            // Permute the Prism's axes back into the original Box axis order.
            // The Prism holds [selected axes..., complement axes...]; build
            // the C axis-permutation array (perm[box_axis] = prism_axis) and
            // apply it. `ax_comp`/`nsel` mirror C's `ax`/`nax` at this point.
            let nsel = ax_sel.len();
            let mut perm: Vec<i64> = vec![-1; nout];
            for (i, &box_axis) in ax_comp.iter().enumerate() {
                perm[box_axis] = (i + nsel) as i64;
            }
            let mut next = 0i64;
            for slot in perm.iter_mut() {
                if *slot == -1 {
                    *slot = next;
                    next += 1;
                }
            }
            // reg_perm_axes uses 1-based indices (Rust convention); the
            // zero-based `perm` above is C's astPermAxes argument.
            let perm_one_based: Vec<usize> = perm.iter().map(|&p| (p + 1) as usize).collect();
            prism.reg_perm_axes(&perm_one_based)?;

            // Put the original current Frame back in place of the Prism's
            // CmpFrame, then simplify the Prism.
            let frm = self.data.frameset.current().snapshot();
            let um = UnitMap::new(nout);
            let newprism = prism.map_region(&um, frm.as_ref())?;
            let simplified = <dyn Mapping>::simplify(newprism.as_ref())?;
            let result = super::merge_util::mapping_into_region(simplified).unwrap_or(newprism);
            return Ok(Some(result));
        }

        Ok(None)
    }

    /// Try to represent a 2-D Box as a Polygon in its current Frame.
    ///
    /// Faithful port of the Polygon branch of `box.c::Simplify`
    /// (box.c:3921-4024): build a Polygon whose four vertices are the Box
    /// corners transformed into the current Frame, choosing the vertex order
    /// (clockwise or anti-clockwise) that makes the interior the inside. If all
    /// points of the Box's boundary mesh lie on the Polygon boundary (to within
    /// the Region's uncertainty), the Polygon is an exact representation and is
    /// returned; otherwise `None`.
    ///
    /// `map` is the simplified base→current Mapping.
    fn convert_to_polygon(&self, map: &dyn Mapping) -> AstResult<Option<DynRegion>> {
        use super::PolygonBuilder;

        let frm = self.data.frameset.current().snapshot();
        let is_sky = frm.as_any().is::<crate::frames::SkyFrame>();

        // Box centre and per-axis extent in the base Frame.
        let centre = &self.data.points[0];
        let corner = &self.data.points[1];
        let ext0 = (corner[0] - centre[0]).abs();
        let ext1 = (corner[1] - centre[1]).abs();

        // Current-Frame uncertainty, carried onto the Polygon. C uses
        // `astGetUncFrm(new, AST__CURRENT)` (box.c:3891) — the *default*
        // uncertainty mapped into the current Frame when none is set — which
        // supplies the tolerance band astRegPins needs. `get_uncertainty`
        // alone would return None and yield a zero-width band.
        let unc = Some(self.get_unc_frm(FrameRef::Current)?);

        // For a SkyFrame, the winding that makes the interior the inside is
        // determined by transforming the Box centre into the current Frame and
        // testing which vertex order encloses it.
        let sky_centre: Option<Vec<f64>> = if is_sky {
            let ps = PointSet::from_rows([[centre[0], centre[1]]].iter().map(|p| p.as_slice()));
            let t = map.transform(&ps, true)?;
            Some(t.point(0)[..2].to_vec())
        } else {
            None
        };

        // Current-Frame boundary mesh of the original Box (astRegMesh(new)).
        let base_mesh = self.reg_base_mesh()?;
        let base_mesh_ps = PointSet::from_rows(base_mesh.iter().map(|p| p.as_slice()));
        let mesh = map.transform(&base_mesh_ps, true)?;

        // Try clockwise (right_handed = 1) then anti-clockwise (0) vertex
        // orders; keep the first that yields a Polygon whose inside is the
        // interior.
        let mut chosen: Option<super::Polygon> = None;
        for right_handed in [true, false] {
            // Box corners in the base Frame, in the required order.
            let base_corners: [[f64; 2]; 4] = if right_handed {
                [
                    [centre[0] - ext0, centre[1] + ext1],
                    [centre[0] - ext0, centre[1] - ext1],
                    [centre[0] + ext0, centre[1] - ext1],
                    [centre[0] + ext0, centre[1] + ext1],
                ]
            } else {
                [
                    [centre[0] + ext0, centre[1] + ext1],
                    [centre[0] + ext0, centre[1] - ext1],
                    [centre[0] - ext0, centre[1] - ext1],
                    [centre[0] - ext0, centre[1] + ext1],
                ]
            };

            // Transform the corners into the current Frame.
            let corners_ps = PointSet::from_rows(base_corners.iter().map(|p| p.as_slice()));
            let cur = map.transform(&corners_ps, true)?;
            let vertices: Vec<Vec<f64>> = (0..4).map(|ic| cur.point(ic)[..2].to_vec()).collect();

            let mut builder = PolygonBuilder::from_dyn(frm.snapshot(), vertices)?;
            if let Some(u) = &unc {
                builder = builder.uncertainty(u.clone_region());
            }
            let newpoly = builder.build()?;

            // Is the inside of the Polygon its interior? For a SkyFrame, test
            // whether the transformed Box centre falls inside; otherwise use
            // the bounded flag.
            let right_way_round = if let Some(sc) = &sky_centre {
                newpoly.point_in_region(sc)?
            } else {
                newpoly.is_bounded()
            };

            if right_way_round {
                chosen = Some(newpoly);
                break;
            }
        }

        let Some(newpoly) = chosen else {
            return Ok(None);
        };

        // Confirm the Polygon exactly represents the Box: every point of the
        // Box's current-Frame boundary mesh must lie on the Polygon boundary,
        // to within the Region uncertainty.
        let pins = newpoly.reg_pins(&mesh, None)?;
        if pins {
            Ok(Some(Box::new(newpoly)))
        } else {
            Ok(None)
        }
    }

    /// Rebuild this Box over a PermMap linking its base and current Frames.
    ///
    /// Faithful port of the PermMap branch of C `Simplify` (box.c:3722-3866).
    /// Each current-Frame axis takes the limits of the base-Frame axis feeding
    /// it; an axis fed a constant gets equal limits; an axis fed nothing is
    /// unbounded, which makes the result an Interval rather than a Box. A
    /// base-Frame axis fed a constant selects a slice through the coordinate
    /// system, and a slice that misses the Box gives a NullRegion.
    ///
    /// `Ok(None)` reports the one case C declines: a base-Frame axis fed by an
    /// output whose own feed is a different axis, so the two Frames are not
    /// related bi-directionally (box.c:3825).
    fn simplify_over_permmap(&self, pm: &PermMap) -> AstResult<Option<DynRegion>> {
        let neg = self.is_negated();
        let closed = self.is_closed();
        let (base_lbnd, base_ubnd) = self.bounds();

        let nin = pm.nin();
        let nout = pm.nout();
        let inperm = pm.inperm();
        let outperm = pm.outperm();
        let constants = pm.constants();

        let mut lbnd = vec![BAD_VALUE; nout];
        let mut ubnd = vec![BAD_VALUE; nout];
        let mut is_null = false;
        let mut is_interval = false;

        // Each output is an axis of the current Frame. Find the base-Frame axis
        // feeding it under the forward transformation (box.c:3755). `nin` and
        // `nout` follow the PermMap's Invert flag while the permutation arrays
        // do not, so an entry beyond the end of an array is read as the
        // unbounded case rather than off the end of the array.
        for ic in 0..nout {
            let feed = outperm.get(ic).copied().unwrap_or(0);
            if feed < 0 {
                // Fed a constant, so both limits are that constant
                // (box.c:3764).
                let k = constant_at(constants, feed);
                lbnd[ic] = k;
                ubnd[ic] = k;
            } else if feed == 0 || feed as usize > nin {
                // Fed AST__BAD, so the axis is unbounded (box.c:3771).
                lbnd[ic] = BAD_VALUE;
                ubnd[ic] = BAD_VALUE;
            } else {
                // Fed an input, so the output takes that input's limits
                // (box.c:3777).
                lbnd[ic] = base_lbnd[feed as usize - 1];
                ubnd[ic] = base_ubnd[feed as usize - 1];
            }

            // A missing bound makes the result an Interval (box.c:3784).
            if is_bad(lbnd[ic]) || is_bad(ubnd[ic]) {
                is_interval = true;
            }
        }

        // Any base-Frame axis absent from the current Frame means the PermMap
        // selects a slice, which need not meet the Box (box.c:3794).
        for ic in 0..nin {
            let feed = inperm.get(ic).copied().unwrap_or(0);
            if feed < 0 {
                // Fed a constant: the slice misses the Box unless the constant
                // lies within the Box's limits on this axis (box.c:3811). Each
                // constant-fed input is a separate plane the slice must lie in,
                // so any one of them falling outside the Box empties the
                // intersection; the verdict accumulates rather than being
                // replaced.
                let k = constant_at(constants, feed);
                let lb = base_lbnd[ic];
                let ub = base_ubnd[ic];
                let outside = if closed == neg {
                    k <= lb || k >= ub
                } else {
                    k < lb || k > ub
                };
                if outside {
                    is_null = true;
                }
            } else if feed == 0 || feed as usize > nout {
                // Fed AST__BAD: the input axis is unbounded and so extends
                // beyond the Box (box.c:3819).
                is_null = true;
            } else if outperm.get(feed as usize - 1).copied().unwrap_or(0) != ic as i64 + 1 {
                // The relationship is not bi-directional (box.c:3825).
                return Ok(None);
            }
        }

        // Build the replacement Region in the current Frame (box.c:3831). The
        // uncertainty comes from the Box only when it holds an explicit one;
        // the parent Simplify has already simplified it.
        let unc = if self.test_unc() {
            Some(self.get_unc_frm(FrameRef::Current)?)
        } else {
            None
        };
        let frm = self.data.frameset.current().snapshot();

        let mut new: DynRegion = if is_null {
            Box::new(NullRegionBuilder::from_dyn(frm).build()?)
        } else if is_interval {
            Box::new(Interval::from_bounds_frame(frm, lbnd, ubnd)?)
        } else {
            Box::new(BoxRegion::from_bounds_frame(frm, &lbnd, &ubnd)?)
        };
        if let Some(unc) = unc {
            new.set_uncertainty(unc);
        }
        if neg {
            new.negate();
        }

        Ok(Some(new))
    }

    /// Compute the effective lower and upper bounds from centre and corner.
    ///
    /// The box extends symmetrically around the centre: the opposite corner
    /// is at `2*centre - corner`. For each axis, lbnd is the minimum and
    /// ubnd is the maximum of the two corners.
    fn bounds(&self) -> (Vec<f64>, Vec<f64>) {
        let naxes = self.data.points[0].len();
        let centre = &self.data.points[0];
        let corner = &self.data.points[1];

        let mut lbnd = Vec::with_capacity(naxes);
        let mut ubnd = Vec::with_capacity(naxes);

        for i in 0..naxes {
            let c = centre[i];
            let extent = (corner[i] - c).abs();
            lbnd.push(c - extent);
            ubnd.push(c + extent);
        }

        (lbnd, ubnd)
    }
}

/// The constant a negative PermMap permutation entry refers to.
///
/// The entry `-j` names `constants[j-1]` (`permmap.c:524`). C indexes the array
/// unchecked; an entry naming a constant that was never supplied is read here as
/// `BAD_VALUE`, which leaves the axis unbounded.
fn constant_at(constants: &[f64], feed: i64) -> f64 {
    constants
        .get((-feed - 1) as usize)
        .copied()
        .unwrap_or(BAD_VALUE)
}

/// Half the geodesic distance along each edge of a box.
///
/// Faithful port of C `GeoLengths` (box.c:975): for each axis, the geodesic
/// distance from the centre to the corner along that axis alone (walking one
/// axis at a time so `distance` sees a single-axis displacement).
fn geo_lengths(frm: &dyn Frame, nc: usize, centre: &[f64], corner: &[f64]) -> AstResult<Vec<f64>> {
    let mut geolen = vec![0.0; nc];
    let mut p1 = centre.to_vec();
    let mut p2 = centre.to_vec();
    for i in 0..nc {
        p2[i] = corner[i];
        geolen[i] = frm.distance(&p1, &p2)?;
        p1[i] = corner[i];
    }
    Ok(geolen)
}

/// The corner position implied by a centre and geodesic half-lengths.
///
/// Faithful port of C `GeoCorner` (box.c:838): starting at the centre, offset
/// away along each axis in turn by the geodesic distance `geolen[i]`, probing
/// the axis direction with a small step. Unlike the C version, the array-swap
/// dance (which only exists to land the result in the caller-supplied buffer)
/// is unnecessary here — we accumulate directly.
fn geo_corner(frm: &dyn Frame, nc: usize, centre: &[f64], geolen: &[f64]) -> AstResult<Vec<f64>> {
    let mut p1 = centre.to_vec();
    for i in 0..nc {
        let mut p2 = p1.clone();
        let mut off = if geolen[i] != 0.0 {
            0.0001 * geolen[i].abs()
        } else {
            1.0e-6
        };
        let off0 = (1.0e-10 * centre[i]).abs();
        if off < off0 {
            off = off0;
        }
        p2[i] += off;
        p1 = frm.offset(&p1, &p2, geolen[i])?;
    }
    Ok(p1)
}

/// Find the best-fitting Box through a mesh of points.
///
/// Faithful port of C `BestBox` (box.c:407). The `mesh` points are given in
/// the Frame represented by `unc` (the current Frame of the Region being
/// simplified). The box is fitted one axis at a time: the mesh extremes on
/// each axis give a first guess of the lower/upper faces, then the values
/// "close to" each face — within 20 uncertainty widths, or one tenth of the
/// box size, whichever is smaller — are averaged with a single 3-sigma clip to
/// reject values that actually lie on other faces. The resulting mean face
/// positions become the box bounds on that axis. Returns `None` if every value
/// on some axis is bad. The returned Box inherits `unc`'s Frame and carries
/// `unc` as its uncertainty.
fn best_box(frm: &dyn Frame, mesh: &PointSet, unc: &dyn Region) -> AstResult<Option<BoxRegion>> {
    let np = mesh.npoint();
    let nc = mesh.ncoord();

    // Bounding box of the uncertainty region, giving the per-axis tolerance.
    let (ulbnd, uubnd) = unc.get_region_bounds()?;

    let mut blbnd = vec![0.0_f64; nc];
    let mut bubnd = vec![0.0_f64; nc];
    let mut axval = vec![0.0_f64; np];

    // Fit the box one axis at a time.
    for ic in 0..nc {
        // Find the first good value on this axis.
        let mut ip = 0;
        let mut org = BAD_VALUE;
        while ip < np {
            org = mesh.point(ip)[ic];
            axval[ip] = org;
            if org != BAD_VALUE {
                break;
            }
            ip += 1;
        }

        // Abort if all the axis values were bad.
        if ip >= np {
            return Ok(None);
        }

        // Find the upper and lower limits of the supplied mesh on this axis by
        // accumulating signed geodesic increments away from the first value.
        let mut lb = 0.0_f64;
        let mut ub = 0.0_f64;
        let mut p0 = org;
        let mut d = 0.0_f64;
        ip += 1;
        while ip < np {
            let p = mesh.point(ip)[ic];
            let dinc = frm.ax_distance(ic + 1, p0, p)?;
            if dinc != BAD_VALUE {
                d += dinc;
                if d < lb {
                    lb = d;
                }
                if d > ub {
                    ub = d;
                }
            }
            axval[ip] = org + d;
            p0 = p;
            ip += 1;
        }

        // Convert relative offsets to actual axis values.
        lb += org;
        ub += org;

        // Scan again for values "close to" the lower or upper bound: within 20
        // uncertainty widths, or one tenth of the box size (smaller wins).
        let mut lim = 20.0 * (uubnd[ic] - ulbnd[ic]);
        let lim2 = 0.1 * (ub - lb);
        if lim2 < lim {
            lim = lim2;
        }

        let mut sxl = 0.0_f64;
        let mut sxl2 = 0.0_f64;
        let mut nxl = 0_usize;
        let mut sxu = 0.0_f64;
        let mut sxu2 = 0.0_f64;
        let mut nxu = 0_usize;

        for &v in axval.iter() {
            if v != BAD_VALUE {
                if (v - lb).abs() <= lim {
                    sxl += v;
                    sxl2 += v * v;
                    nxl += 1;
                } else if (v - ub).abs() <= lim {
                    sxu += v;
                    sxu2 += v * v;
                    nxu += 1;
                }
            }
        }

        // Mean and clipping limit for the lower face.
        let mut mxl;
        let liml;
        if nxl > 0 {
            mxl = sxl / nxl as f64;
            let mut var = if nxl > 1 {
                sxl2 / nxl as f64 - mxl * mxl
            } else {
                0.0
            };
            let eps = 100.0 * mxl * f64::EPSILON;
            if var < eps * eps {
                var = eps;
            } else {
                var = 3.0 * var.sqrt();
            }
            liml = var;
        } else {
            mxl = lb;
            liml = 0.0;
        }

        // Mean and clipping limit for the upper face.
        let mut mxu;
        let limu;
        if nxu > 0 {
            mxu = sxu / nxu as f64;
            let mut var = if nxu > 1 {
                sxu2 / nxu as f64 - mxu * mxu
            } else {
                0.0
            };
            let eps = 100.0 * mxu * f64::EPSILON;
            if var < eps * eps {
                var = eps;
            } else {
                var = 3.0 * var.sqrt();
            }
            limu = var;
        } else {
            mxu = ub;
            limu = 0.0;
        }

        // Single 3-sigma clipping iteration to reject values on other faces.
        sxl = 0.0;
        nxl = 0;
        sxu = 0.0;
        nxu = 0;
        for &v in axval.iter() {
            if v != BAD_VALUE {
                if (v - mxl).abs() <= liml {
                    sxl += v;
                    nxl += 1;
                } else if (v - mxu).abs() <= limu {
                    sxu += v;
                    nxu += 1;
                }
            }
        }

        mxl = if nxl > 0 { sxl / nxl as f64 } else { lb };
        mxu = if nxu > 0 { sxu / nxu as f64 } else { ub };

        blbnd[ic] = mxl;
        bubnd[ic] = mxu;
    }

    // Build the returned Box, inheriting the uncertainty's Frame and carrying
    // `unc` as its own uncertainty (astBox( unc, 1, blbnd, bubnd, unc, ... )).
    let box_frame = frm.snapshot();
    let mut region = BoxRegion::from_bounds_frame(box_frame, &blbnd, &bubnd)?;
    region.data.uncertainty = Some(unc.clone_region());
    Ok(Some(region))
}

// ============================================================================
// RegionGeometry implementation
// ============================================================================

impl RegionGeometry for BoxRegion {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        &mut self.data
    }

    fn reg_base_inside(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        let npoint = pts.npoint();
        let naxes = self.data.points[0].len();
        let (lbnd, ubnd) = self.bounds();
        let closed = self.data.closed_for_base_inside();
        // Each axis value is tested by the base Frame's Axis object, so a
        // sky axis wraps the value into the Box's range (box.c:4268,
        // `astAxIn`).
        let frame = self.data.frameset.base();

        let mut result = vec![false; npoint];

        for (idx, inside) in result.iter_mut().enumerate() {
            let point = pts.point(idx);

            // BAD input coordinates mean the point is outside.
            if point_has_bad(&point[..naxes]) {
                continue;
            }

            // Check each axis constraint.
            let mut all_pass = true;
            for ax in 0..naxes {
                if !frame.ax_in(ax + 1, lbnd[ax], ubnd[ax], point[ax], closed) {
                    all_pass = false;
                    break;
                }
            }

            *inside = all_pass;
        }

        Ok(result)
    }

    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        Ok(self.bounds())
    }

    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        let naxes = self.data.points[0].len();
        let (lbnd, ubnd) = self.bounds();
        let np = self.data.effective_mesh_size();

        // Check if the box occupies a single point.
        let single = (0..naxes).all(|i| ubnd[i] <= lbnd[i]);
        if single {
            let centre = &self.data.points[0];
            return Ok(vec![centre.clone()]);
        }

        if naxes == 1 {
            // 1-D: the boundary is the two extreme points.
            return Ok(vec![vec![lbnd[0]], vec![ubnd[0]]]);
        }

        if naxes == 2 {
            // 2-D: distribute points evenly along 4 edges.
            // Corners: (lbnd[0],lbnd[1]) -> (lbnd[0],ubnd[1]) ->
            //          (ubnd[0],ubnd[1]) -> (ubnd[0],lbnd[1]) -> back
            let corners = [
                [lbnd[0], lbnd[1]],
                [lbnd[0], ubnd[1]],
                [ubnd[0], ubnd[1]],
                [ubnd[0], lbnd[1]],
            ];

            // For flat geometry, compute edge lengths as coordinate differences.
            let mut edge_lengths = [0.0_f64; 4];
            for iedge in 0..4 {
                let next = (iedge + 1) % 4;
                let dx = corners[next][0] - corners[iedge][0];
                let dy = corners[next][1] - corners[iedge][1];
                edge_lengths[iedge] = (dx * dx + dy * dy).sqrt();
            }

            let total_len: f64 = edge_lengths.iter().sum();

            // Compute points per edge proportional to edge length.
            let mut np_edge = [0_usize; 4];
            let mut total_np = 0;
            if total_len > 0.0 {
                let ppd = np as f64 / total_len;
                for iedge in 0..4 {
                    np_edge[iedge] = (edge_lengths[iedge] * ppd) as usize;
                    if np_edge[iedge] == 0 {
                        np_edge[iedge] = 1;
                    }
                    total_np += np_edge[iedge];
                }
            } else {
                // Degenerate: give equal points to each edge.
                let np0 = np / 4;
                for edge_np in &mut np_edge {
                    *edge_np = np0.max(1);
                    total_np += *edge_np;
                }
            }

            let mut mesh = Vec::with_capacity(total_np);

            for iedge in 0..4 {
                let next = (iedge + 1) % 4;
                let n = np_edge[iedge];
                let dx = (corners[next][0] - corners[iedge][0]) / n as f64;
                let dy = (corners[next][1] - corners[iedge][1]) / n as f64;

                for i in 0..n {
                    let x = corners[iedge][0] + dx * i as f64;
                    let y = corners[iedge][1] + dy * i as f64;
                    mesh.push(vec![x, y]);
                }
            }

            return Ok(mesh);
        }

        // N-D (naxes > 2): generate points on each face of the hyper-box.
        // Each face is defined by fixing one axis at either its lower or
        // upper bound and varying all other axes. Following the C approach:
        // compute np_axis samples per axis, yielding np_axis^(naxes-1) points
        // per face, with 2*naxes faces.
        let np_axis =
            1 + ((np as f64 / (2 * naxes) as f64).powf(1.0 / (naxes - 1) as f64)) as usize;
        let np_axis = np_axis.max(2);

        let mut mesh = Vec::new();

        // For each axis, create the upper and lower faces.
        for fixed_axis in 0..naxes {
            for &fixed_val in &[lbnd[fixed_axis], ubnd[fixed_axis]] {
                // Generate a grid over the remaining axes.
                let free_axes: Vec<usize> = (0..naxes).filter(|&a| a != fixed_axis).collect();
                let n_free = free_axes.len();

                // Total number of points on this face: np_axis^n_free
                let n_face = np_axis.pow(n_free as u32);

                for idx in 0..n_face {
                    let mut point = vec![0.0; naxes];
                    point[fixed_axis] = fixed_val;

                    let mut remainder = idx;
                    for &ax in &free_axes {
                        let step = remainder % np_axis;
                        remainder /= np_axis;
                        // Linearly interpolate between lbnd and ubnd for this axis.
                        point[ax] = if np_axis > 1 {
                            lbnd[ax] + (ubnd[ax] - lbnd[ax]) * step as f64 / (np_axis - 1) as f64
                        } else {
                            0.5 * (lbnd[ax] + ubnd[ax])
                        };
                    }

                    mesh.push(point);
                }
            }
        }

        Ok(mesh)
    }

    fn is_bounded_inner(&self) -> bool {
        // Base Region::GetBounded (region.c:3358): a closed-curve region is
        // bounded unless negated, except within a SkyFrame where a finite
        // region always has a finite negation.
        self.data.closed_curve_bounded()
    }

    fn remap_defining_points(&mut self, mapping: &dyn Mapping, forward: bool) -> AstResult<()> {
        // Transform the defining points (centre and corner) through the mapping.
        let naxes = self.data.points[0].len();
        let pts = PointSet::from_rows(self.data.points.iter().map(|p| p.as_slice()));
        let transformed = mapping.transform(&pts, forward)?;

        let mut new_points = Vec::with_capacity(self.data.points.len());
        for i in 0..self.data.points.len() {
            let p = transformed.point(i);
            new_points.push(p[..naxes].to_vec());
        }
        self.data.points = new_points;
        self.data.invalidate_cache();
        Ok(())
    }
}

// ============================================================================
// Region trait
// ============================================================================

impl Region for BoxRegion {
    fn point_in_region(&self, point: &[f64]) -> AstResult<bool> {
        // Check for BAD coordinates in the input.
        if point_has_bad(point) {
            return Ok(false);
        }

        // Transform point from current frame to base frame.
        let naxes = self.data.frameset.base().naxes();
        let pts = PointSet::from_rows(std::iter::once(point));
        let base_pts = self.data.frameset.transform(&pts, false)?;
        let base_point = base_pts.point(0);

        // Check the transformed point for BAD values.
        if base_point[..naxes].iter().any(|&v| is_bad(v)) {
            return Ok(false);
        }

        // Test raw geometry.
        let inside = self.reg_base_inside_point(&base_point[..naxes])?;

        // Apply negation.
        if self.data.negated {
            Ok(!inside)
        } else {
            Ok(inside)
        }
    }

    fn reg_pins_mask(
        &self,
        pts: &PointSet,
        unc: Option<&dyn Region>,
    ) -> AstResult<(bool, Vec<bool>)> {
        // Faithful port of box.c::RegPins (box.c:2952): a point lies on the
        // boundary of the Box iff it falls inside a Box widened by the joint
        // uncertainty band and outside a Box narrowed by the same band.
        let (lbnd_box, ubnd_box) = self.bounds();
        let naxes = lbnd_box.len();
        if pts.ncoord() != naxes {
            return Err(AstError::new(
                ErrorCode::Inter,
                format!(
                    "reg_pins(Box): PointSet has {} coordinates per point, expected {}",
                    pts.ncoord(),
                    naxes
                ),
            ));
        }

        // Box centre and half-extent per axis (this->centre / this->extent).
        let centre = &self.data.points[0];
        let extent: Vec<f64> = (0..naxes)
            .map(|i| 0.5 * (ubnd_box[i] - lbnd_box[i]))
            .collect();

        let bfrm = self.data.frameset.base();

        // Border half-width per axis. `tunc` is this Box's own uncertainty in
        // the base Frame (explicit or default); `unc` describes the supplied
        // points' uncertainty. Re-centring `tunc`/`unc` on the region's safe
        // interior point (box.c:3078,3088) is omitted: it exists only to fix
        // bounds when an uncertainty straddles a discontinuity, and a Box's
        // bound width — the only quantity used below — is centre-invariant.
        // The reachable default uncertainty is a tiny box already centred at
        // the region centre, so the re-centre is a proven no-op (tracked as
        // the RCENT / R-UNC parity gaps).
        let tunc = self.get_unc_frm(FrameRef::Base)?;
        let (lbnd_tunc, ubnd_tunc) = tunc.get_region_bounds()?;

        let wid: Vec<f64> = if let Some(unc) = unc {
            let (lbnd_unc, ubnd_unc) = unc.get_region_bounds()?;
            (0..naxes)
                .map(|i| -> AstResult<f64> {
                    let dt = bfrm.ax_distance(i + 1, lbnd_tunc[i], ubnd_tunc[i])?;
                    let du = bfrm.ax_distance(i + 1, lbnd_unc[i], ubnd_unc[i])?;
                    Ok(0.5 * (dt.abs() + du.abs()))
                })
                .collect::<AstResult<Vec<f64>>>()?
        } else {
            (0..naxes)
                .map(|i| -> AstResult<f64> {
                    let dt = bfrm.ax_distance(i + 1, lbnd_tunc[i], ubnd_tunc[i])?;
                    Ok((0.5 * dt).abs())
                })
                .collect::<AstResult<Vec<f64>>>()?
        };

        // Build the larger and smaller corner arrays (box.c:3118-3123).
        let mut large = Vec::with_capacity(naxes);
        let mut small = Vec::with_capacity(naxes);
        for i in 0..naxes {
            large.push(centre[i] + extent[i] + wid[i]);
            let mut s = extent[i] - wid[i];
            if s < 0.0 {
                s = 0.0;
            }
            small.push(centre[i] + s);
        }

        let large_box = BoxRegion::from_corner_frame(bfrm.snapshot(), centre.clone(), large)?;
        let mut small_box = BoxRegion::from_corner_frame(bfrm.snapshot(), centre.clone(), small)?;
        small_box.data.negated = true;

        // Points on the boundary of "this" are inside both the large box and
        // the negated small box (box.c:3134-3135): chain the two forward
        // transforms; a point is on the boundary iff the result has no BAD.
        let ps1 = super::data::region_transform(&large_box, pts)?;
        let ps2 = super::data::region_transform(&small_box, &ps1)?;

        let mask: Vec<bool> = (0..ps2.npoint())
            .map(|j| !ps2.point(j).iter().any(|&v| is_bad(v)))
            .collect();
        let result = mask.iter().all(|&b| b);
        Ok((result, mask))
    }

    fn negated_copy(&self) -> DynRegion {
        let mut copy = self.clone();
        copy.data.negated = !copy.data.negated;
        Box::new(copy)
    }

    fn negate(&mut self) {
        self.data.negated = !self.data.negated;
    }

    fn is_negated(&self) -> bool {
        self.data.negated
    }

    fn is_closed(&self) -> bool {
        self.data.closed
    }

    fn set_closed(&mut self, closed: bool) {
        self.data.closed = closed;
        self.data.closed_set = true;
    }

    fn test_closed(&self) -> bool {
        self.data.closed_set
    }

    fn set_closed_raw(&mut self, closed: bool, set: bool) {
        self.data.closed = closed;
        self.data.closed_set = set;
    }

    fn fill_factor(&self) -> f64 {
        self.data.effective_fill_factor()
    }

    fn set_fill_factor(&mut self, factor: f64) {
        self.data.set_fill_factor(factor);
    }

    fn test_fill_factor(&self) -> bool {
        self.data.fill_factor_set
    }

    fn clear_fill_factor(&mut self) {
        self.data.clear_fill_factor();
    }

    fn mesh_size(&self) -> usize {
        self.data.effective_mesh_size()
    }

    fn set_mesh_size(&mut self, size: usize) {
        self.data.set_mesh_size(size);
    }

    fn test_mesh_size(&self) -> bool {
        self.data.mesh_size_set
    }

    fn clear_mesh_size(&mut self) {
        self.data.clear_mesh_size();
    }

    fn is_bounded(&self) -> bool {
        self.is_bounded_inner()
    }

    fn is_adaptive(&self) -> bool {
        self.data.adaptive
    }

    fn set_adaptive(&mut self, adaptive: bool) {
        self.data.adaptive = adaptive;
    }

    fn has_dummy_fs(&self) -> bool {
        self.data.dummy_fs
    }

    fn set_region_fs(&mut self, frame: &dyn Frame) {
        self.data.set_region_fs(frame);
    }

    fn get_region_bounds(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        super::region_bounds_current(self)
    }

    fn region_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        self.reg_base_box()
    }

    fn get_region_mesh(&self) -> AstResult<PointSet> {
        // Check for a cached mesh first.
        {
            let cached = self.data.cached_base_mesh.borrow();
            if let Some(mesh) = cached.as_ref() {
                return Ok(PointSet::from_rows(mesh.iter().map(|p| p.as_slice())));
            }
        }

        // Compute the mesh.
        let mesh = self.reg_base_mesh()?;
        let result = PointSet::from_rows(mesh.iter().map(|p| p.as_slice()));

        // Cache it.
        *self.data.cached_base_mesh.borrow_mut() = Some(mesh);

        Ok(result)
    }

    /// box.c `RegBaseGrid`: the base Frame's grid of MeshSize points over
    /// the Box's bounds.
    fn get_region_grid(&self) -> AstResult<PointSet> {
        if !self.is_bounded() {
            return Err(AstError::new(
                ErrorCode::Mbbnf,
                "astRegBaseGrid(Box): The Box is unbounded so no grid can be created \
                 to cover it.",
            ));
        }
        let (lbnd, ubnd) = self.reg_base_box()?;
        self.data
            .frameset
            .base()
            .frame_grid(self.data.effective_mesh_size() as i32, &lbnd, &ubnd)
    }

    fn get_region_points(&self) -> AstResult<PointSet> {
        Ok(PointSet::from_rows(
            self.data.points.iter().map(|p| p.as_slice()),
        ))
    }

    fn get_region_frame(&self) -> DynFrame {
        self.data.frameset.current().snapshot()
    }

    fn get_region_frameset(&self) -> FrameSet {
        self.data.frameset.clone()
    }

    fn region_frameset_mut(&mut self) -> &mut FrameSet {
        &mut self.data.frameset
    }

    fn store_uncertainty(&mut self, unc: Option<DynRegion>) {
        self.data.uncertainty = unc;
        self.data.invalidate_cache();
    }

    fn set_region_fs_flag(&mut self, value: bool) {
        self.data.set_region_fs_flag(value);
    }

    fn region_fs_flag(&self) -> bool {
        self.data.region_fs
    }

    fn get_uncertainty(&self) -> Option<DynRegion> {
        self.data.uncertainty.clone()
    }

    fn reg_centre_query(&self, ifrm: FrameRef) -> AstResult<Option<Vec<f64>>> {
        // Box always has a centre (box.c:2872-2881). Return the base-Frame
        // centre, or transform it into the current Frame when requested.
        let centre = self.data.points[0].clone();
        match ifrm {
            FrameRef::Current => Ok(Some(self.reg_tran_point(&centre, true)?)),
            _ => Ok(Some(centre)),
        }
    }

    fn reg_centre_set(&mut self, cen: &[f64], ifrm: FrameRef) -> AstResult<()> {
        let bfrm = self.data.frameset.base().snapshot();
        let ncb = self.data.points[0].len();

        // Geodesic half-lengths of the current box (GeoLengths, box.c:975):
        // distance along each axis from the centre to the corner.
        let centre = self.data.points[0].clone();
        let corner = self.data.points[1].clone();
        let geolen = geo_lengths(bfrm.as_ref(), ncb, &centre, &corner)?;

        // Resolve the supplied centre into a base-Frame position (box.c:2893).
        // When supplied in the current Frame, transform it to the base Frame
        // and replace any BAD component with the existing centre value.
        let mut bc = centre.clone();
        match ifrm {
            FrameRef::Current => {
                let tc = self.reg_tran_point(cen, false)?;
                for ic in 0..ncb {
                    if !is_bad(tc[ic]) {
                        bc[ic] = tc[ic];
                    }
                }
            }
            _ => {
                for ic in 0..ncb {
                    if !is_bad(cen[ic]) {
                        bc[ic] = cen[ic];
                    }
                }
            }
        }

        // Recompute the corner from the new centre and the geodesic
        // half-lengths (GeoCorner, box.c:838).
        let new_corner = geo_corner(bfrm.as_ref(), ncb, &bc, &geolen)?;

        self.data.points[0] = bc;
        self.data.points[1] = new_corner;
        self.data.invalidate_cache();
        Ok(())
    }

    fn reg_base_pick_axes(&self, axes: &[usize]) -> AstResult<Option<DynRegion>> {
        // Port of box.c::RegBasePick (box.c:2655): build a Box spanned by the
        // selected base-Frame axes by slicing the centre/corner defining
        // points to those axes.
        let bfrm = self.data.frameset.base();
        let target: Vec<i32> = axes.iter().map(|&a| a as i32).collect();
        let frm = bfrm.pick_axes_snapshot(&target)?;

        // Slice the centre (points[0]) and corner (points[1]) to the selected
        // base-Frame axes.
        let centre: Vec<f64> = axes.iter().map(|&a| self.data.points[0][a]).collect();
        let corner: Vec<f64> = axes.iter().map(|&a| self.data.points[1][a]).collect();

        // Select the same axes from the base-Frame uncertainty (if any); drop
        // it if picking does not yield a Region (box.c:2727-2736).
        let unc = match &self.data.uncertainty {
            Some(u) => match super::pick_axes_region(u.as_ref(), axes)? {
                super::PickedAxes::Region(r) => Some(r),
                super::PickedAxes::Frame(_) => None,
            },
            None => None,
        };

        let frameset = FrameSet::new(frm);
        let mut data = RegionData::new(frameset, vec![centre, corner]);
        data.uncertainty = unc;
        Ok(Some(Box::new(BoxRegion { data })))
    }

    fn reg_perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.perm_axes(perm)
    }

    fn region_merge(&self, other_reg: &dyn Region, intfirst: bool) -> Option<DynMapping> {
        // Faithful port of `box.c::MergeBox` (1658). `intfirst` selects the
        // axis order: when true this Box's axes go first and `other`'s second;
        // when false `other`'s axes go first.
        let other = other_reg.as_any();

        // Guard shared by every branch (box.c:1749): the two Regions must have
        // the same Closed attribute, and either both or neither must have a
        // defined uncertainty.
        if self.is_closed() != other_reg.is_closed()
            || self.data.uncertainty.is_some() != other_reg.get_uncertainty().is_some()
        {
            return None;
        }

        let neg_this = self.data.negated;
        let nax_this = self.naxes();
        let nax_reg = other_reg.naxes();
        let centre_this = &self.data.points[0];

        // Place the two axis-value slices in the order given by `intfirst`.
        let concat = |this_v: &[f64], reg_v: &[f64]| -> Vec<f64> {
            let mut v = Vec::with_capacity(nax_this + nax_reg);
            if intfirst {
                v.extend_from_slice(this_v);
                v.extend_from_slice(reg_v);
            } else {
                v.extend_from_slice(reg_v);
                v.extend_from_slice(this_v);
            }
            v
        };

        let built: Option<MergedRegion> = if let Some(other) = other.downcast_ref::<BoxRegion>() {
            // Box + Box → Box (box.c:1783). Both un-negated.
            if neg_this || other.data.negated {
                None
            } else {
                let centre = concat(centre_this, &other.data.points[0]);
                let corner = concat(&self.data.points[1], &other.data.points[1]);
                Some(MergedRegion::Box(centre, corner))
            }
        } else if let Some(other) = other.downcast_ref::<Interval>() {
            // Box + Interval → Interval (box.c:1809). Both un-negated.
            if neg_this || other.is_negated() {
                None
            } else {
                let (blb, bub) = self.bounds();
                let lbnd = concat(&blb, &other.data.points[0]);
                let ubnd = concat(&bub, &other.data.points[1]);
                Some(MergedRegion::Interval(lbnd, ubnd))
            }
        } else if let Some(other) = other.downcast_ref::<NullRegion>() {
            // Box + NullRegion → Interval (box.c:1855). The NullRegion must be
            // negated and the Box must not.
            if neg_this || !other.is_negated() {
                None
            } else {
                let (blb, bub) = self.bounds();
                let bad = vec![BAD_VALUE; nax_reg];
                let lbnd = concat(&blb, &bad);
                let ubnd = concat(&bub, &bad);
                Some(MergedRegion::Interval(lbnd, ubnd))
            }
        } else if let Some(other) = other.downcast_ref::<PointList>() {
            // Box + PointList → PointList (box.c:1907). Both un-negated, and
            // the Box must be a single point (zero width relative to its
            // uncertainty).
            if neg_this || other.is_negated() {
                None
            } else {
                let (blb, bub) = self.bounds();
                if !super::merge_util::region_is_point(&blb, &bub, self.data.uncertainty.as_ref()) {
                    None
                } else {
                    let merged: Vec<Vec<f64>> = other
                        .data
                        .points
                        .iter()
                        .map(|p| concat(centre_this, p))
                        .collect();
                    Some(MergedRegion::PointList(merged))
                }
            }
        } else {
            None
        };

        let built = built?;

        // The merged Region is built on a CmpFrame of the two *base* Frames and
        // mapped into the combined current Frame at the end (box.c:1764-1773).
        let bfrm_this = self.data.frameset.base().snapshot();
        let bfrm_reg = other_reg.get_region_frameset().base().snapshot();
        let bfrm: DynFrame = if intfirst {
            Box::new(crate::frames::CmpFrame::new(bfrm_this, bfrm_reg))
        } else {
            Box::new(crate::frames::CmpFrame::new(bfrm_reg, bfrm_this))
        };

        // The two current Frames, combined in the same order (box.c:2019-2030).
        let f_this = self.data.frameset.current().snapshot();
        let f_reg = other_reg.get_region_frame();
        let cfrm: DynFrame = if intfirst {
            Box::new(crate::frames::CmpFrame::new(f_this, f_reg))
        } else {
            Box::new(crate::frames::CmpFrame::new(f_reg, f_this))
        };

        // The two base->current Mappings, in parallel (box.c:2032-2044).
        let map_this = self
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)
            .ok()?;
        let map_reg = other_reg
            .get_region_frameset()
            .mapping(FrameRef::Base, FrameRef::Current)
            .ok()?;
        let bcmap: DynMapping = if intfirst {
            Box::new(
                crate::mappings::CmpMap::new(
                    map_this,
                    map_reg,
                    crate::mappings::Composition::Parallel,
                )
                .ok()?,
            )
        } else {
            Box::new(
                crate::mappings::CmpMap::new(
                    map_reg,
                    map_this,
                    crate::mappings::Composition::Parallel,
                )
                .ok()?,
            )
        };

        // Combine uncertainties in the requested order when both are present.
        let unc = super::merge_util::combine_uncertainties(
            self.data.uncertainty.clone(),
            other_reg.get_uncertainty(),
            intfirst,
        );

        let scaling = super::merge_util::MergeScaling {
            msz_this: (
                self.data.effective_mesh_size(),
                self.data.mesh_size_set,
                nax_this,
            ),
            msz_reg: (other_reg.mesh_size(), other_reg.test_mesh_size(), nax_reg),
            fill_this: (self.data.effective_fill_factor(), self.data.fill_factor_set),
            fill_reg: (other_reg.fill_factor(), other_reg.test_fill_factor()),
        };

        super::merge_util::finish_region_merge(
            built,
            super::merge_util::MergeFrames { bfrm, cfrm, bcmap },
            super::merge_util::MergeOverlay {
                closed: self.data.closed,
                closed_set: self.data.closed_set,
                adaptive: self.data.adaptive,
                negated: false,
            },
            unc,
            scaling,
        )
    }

    fn reg_frame_attrib<'a>(
        &mut self,
        setting: &super::FrameSetting<'a>,
    ) -> AstResult<super::FrameSetting<'a>> {
        super::data::region_frame_attrib(self, setting)
    }

    fn clone_region(&self) -> DynRegion {
        Box::new(self.clone())
    }
}

// ============================================================================
// Frame trait — delegate to encapsulated FrameSet's current frame
// ============================================================================

impl Frame for BoxRegion {
    region_frame_attrs!();

    fn naxes(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    /// Mirrors C `Region::MatchAxesX` (region.c:6413): the axes of the
    /// current Frame of the Region's FrameSet are matched.
    fn match_axes_x(&self, frm1: &dyn Frame) -> AstResult<Vec<i32>> {
        self.data.frameset.current().match_axes_x(frm1)
    }

    /// Mirrors C `Region::Match` (region.c:6214): the current Frame of the
    /// Region's FrameSet is matched, through `astMatch` and so with its
    /// cast retry.
    fn match_template(
        &self,
        target: &dyn Frame,
        match_sub: bool,
    ) -> AstResult<Option<crate::frames::find_frame::MatchResult>> {
        crate::frames::find_frame::match_with_subclass(
            self.data.frameset.current(),
            target,
            match_sub,
        )
    }

    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        self.data.frameset.current().primary_frame(axis)
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        self.data.frameset.current().format(axis, value)
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        self.data.frameset.current().unformat(axis, text)
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        self.data.frameset.current().distance(a, b)
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        self.data.frameset.current().angle(a, b, c)
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        self.data.frameset.current().offset(from, to, dist)
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        self.data.frameset.current().norm(coords)
    }

    /// A Region's axes are its encapsulated current Frame's axes. Mirrors
    /// C `Region::AxDistance` / `AxOffset` / `AxNorm` (region.c:1457, 1610,
    /// 1536), which all forward to that Frame.
    fn get_axis(&self, axis: usize) -> AxisKind {
        self.data.frameset.current().get_axis(axis)
    }

    /// region.c `RemoveRegions`: a Region becomes its Frame.
    fn frame_without_regions(&self) -> Option<DynFrame> {
        Some(self.get_region_frame())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    /// C has one `Simplify` slot, so a Region simplifies through its Mapping
    /// side and the caller keeps the result because it is still a Frame
    /// (`astIsAFrame`, `cmpframe.c:9024`).
    fn simplify_frame(&self) -> AstResult<Option<DynFrame>> {
        simplify_frame_via_mapping(self)
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        self.data.frameset.current().map_from(previous)
    }

    fn convert_to(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>> {
        self.data.frameset.current().convert_to(to, domains)
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        if let Some(other_box) = other.as_any().downcast_ref::<BoxRegion>() {
            self.data
                .frameset
                .current()
                .equal(other_box.data.frameset.current())
        } else {
            false
        }
    }
}

// ============================================================================
// Mapping trait — delegate to region_transform
// ============================================================================

impl Mapping for BoxRegion {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.data.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.data.mapping_flags
    }

    /// A Region is a Frame, which C's frame.c `GetIsLinear` reports as
    /// linear.
    fn is_linear(&self) -> bool {
        true
    }

    /// region.c `RemoveRegions`: a Region becomes its Frame, which as a
    /// Mapping is a UnitMap with its number of axes.
    fn mapping_without_regions(&self) -> Option<crate::mappings::DynMapping> {
        Some(Box::new(crate::mappings::UnitMap::new(self.nin())))
    }

    /// region.c `ReportPoints`: the current Frame formats the values.
    fn report_points(&self, _forward: bool, inp: &PointSet, out: &PointSet) {
        let frame = self.data.frameset.current();
        crate::mappings::report::write_report(&crate::frames::frame_report_lines(
            frame, frame, inp, out,
        ));
    }

    region_mapping_ident!();

    // C's `Frame` overrides `astDoNotSimplify` to zero (frame.c:3511-3554): a
    // set `Ident` is a user label on a coordinate system, not a request to keep
    // a Mapping intact, so a named Frame is still simplified.
    fn do_not_simplify(&self) -> bool {
        false
    }

    fn nin(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    fn nout(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    fn is_inverted(&self) -> bool {
        false
    }

    fn invert_orientation(&mut self) {
        // A Region's forward and inverse transforms are identical (membership
        // is direction-independent), so inverting has no effect on behaviour.
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        super::data::region_transform(self, pts)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        crate::mappings::validate_transform(self, pts, forward, Some(out))?;
        let result = self.transform(pts, forward)?;
        let npoint = result.npoint();
        let ncoord = result.ncoord();
        for i in 0..npoint {
            let src = result.point(i);
            let dst = out.point_mut(i);
            dst[..ncoord].copy_from_slice(src);
        }
        Ok(())
    }

    fn simplified(&self, _gate: ViaSimplify) -> AstResult<(DynMapping, bool)> {
        // Invoke the parent Region simplify first (box.c:3705); it simplifies
        // the encapsulated FrameSet and any explicit uncertainty Region. All
        // subsequent work is done on the returned copy. `simpler` tracks
        // whether any simplification took place (box.c:3710).
        let parent = self.parent_region_simplify()?;
        let mut simpler = parent.is_some();
        let parent_box;
        let this: &BoxRegion = match &parent {
            Some(p) => {
                parent_box = p
                    .as_ref()
                    .as_any()
                    .downcast_ref::<BoxRegion>()
                    .expect("parent_region_simplify preserves the Region class")
                    .clone();
                &parent_box
            }
            None => self,
        };

        let map = this
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;
        let simple_map = map.simplify()?;

        // `new` holds the simplified Region (initially the parent-simplified
        // copy). Any of the re-fit branches below replaces it and sets
        // `simpler`. At the single exit the original Region's attributes are
        // overlaid via astRegOverlay (box.c:4061).
        let mut new: DynRegion = this.clone_region();

        if let Some(pm) = simple_map.as_ref().as_any().downcast_ref::<PermMap>() {
            // The permutation is known exactly, so the Box's axes are swapped
            // explicitly rather than fitted through a mesh (box.c:3722). C sets
            // `simpler` unconditionally on entry to this branch (box.c:3748) and
            // clears it only when the permutation is not bi-directional, which
            // discards the parent simplification along with it (box.c:4064).
            match this.simplify_over_permmap(pm)? {
                Some(replacement) => {
                    new = replacement;
                    simpler = true;
                }
                None => simpler = false,
            }
        } else if !simple_map.as_ref().as_any().is::<UnitMap>() {
            // Transform defining points to current frame to form a candidate
            // remapped Box.
            let pts = PointSet::from_rows(this.data.points.iter().map(|p| p.as_slice()));
            let transformed = simple_map.transform(&pts, true)?;
            let current_frame = this.data.frameset.current().snapshot();
            let naxes = current_frame.naxes();
            let new_points: Vec<Vec<f64>> = (0..this.data.points.len())
                .map(|i| transformed.point(i)[..naxes].to_vec())
                .collect();
            let frameset = FrameSet::new(current_frame);
            let mut data = RegionData::new(frameset, new_points);
            data.negated = this.data.negated;
            data.closed = this.data.closed;
            data.closed_set = this.data.closed_set;
            data.mesh_size = this.data.mesh_size;
            data.mesh_size_set = this.data.mesh_size_set;
            data.fill_factor = this.data.fill_factor;
            data.fill_factor_set = this.data.fill_factor_set;
            data.adaptive = this.data.adaptive;
            data.uncertainty = this.data.uncertainty.clone();
            let candidate = BoxRegion { data };

            // A direct point transform only yields a valid axis-aligned Box
            // when the base->current Mapping preserves axis-alignment
            // (diagonal scale, shift, permutation). For a shear or rotation
            // the candidate box is strictly larger than the true region;
            // verify via the boundary round-trip (box.c::Simplify, 3905).
            let candidate_mesh = candidate.reg_base_mesh()?;
            if this.verify_remap_exact(simple_map.as_ref(), &candidate_mesh)? {
                new = Box::new(candidate);
                simpler = true;
            } else {
                // The Box could not be re-defined as an axis-aligned Box by a
                // direct corner transform. Fit the best Box through the
                // current-Frame mesh (box.c::Simplify, 3895 → BestBox): a mesh
                // whose points all fall on the fitted Box boundary (to within
                // the uncertainty) means the current-Frame region really is a
                // Box even though the corner transform was not exact — this
                // keeps a nonlinear-but-separable remap (e.g. u=x*x) a Box
                // rather than degrading it.
                let base_mesh = this.reg_base_mesh()?;
                let base_mesh_ps = PointSet::from_rows(base_mesh.iter().map(|p| p.as_slice()));
                let mesh = simple_map.transform(&base_mesh_ps, true)?;
                let unc = this.get_unc_frm(FrameRef::Current)?;
                let cur_frame = this.data.frameset.current().snapshot();
                let mut replaced = false;
                if let Some(newbox) = best_box(cur_frame.as_ref(), &mesh, unc.as_ref())?
                    && newbox.reg_pins(&mesh, None)?
                {
                    // Confirm the inverse: transform the fitted Box's own
                    // boundary mesh back into the original base Frame and check
                    // it lies on the original Box's boundary
                    // (box.c::Simplify, 3905).
                    let nb_base_mesh = newbox.reg_base_mesh()?;
                    let nb_mesh_ps = PointSet::from_rows(nb_base_mesh.iter().map(|p| p.as_slice()));
                    let nb_basemesh = simple_map.transform(&nb_mesh_ps, false)?;
                    if this.reg_pins(&nb_basemesh, None)? {
                        new = Box::new(newbox);
                        simpler = true;
                        replaced = true;
                    }
                }
                // The Box could not be re-defined as a Box in its current
                // Frame. For a 2-D→2-D Box, try representing it as a Polygon
                // whose four vertices are the transformed Box corners
                // (box.c::Simplify, 3921).
                if !replaced && simple_map.nin() == 2 && simple_map.nout() == 2 {
                    if let Some(poly) = this.convert_to_polygon(simple_map.as_ref())? {
                        new = poly;
                        simpler = true;
                        replaced = true;
                    }
                }
                // Otherwise try splitting it into two sub-regions on distinct
                // axis subsets and joining them into a Prism (box.c::Simplify,
                // 4036, calls astConvertToPrism when no simpler single Region
                // was found).
                if !replaced {
                    if let Some(prism) = this.convert_to_prism()? {
                        new = prism;
                        simpler = true;
                    }
                }
            }
        }

        // If any simplification could be performed, copy Region attributes
        // from the original Region to the returned Region (box.c:4060). The
        // unc=true flag clears the result's uncertainty when the original
        // Region had none, so a Box fitted through a default-uncertainty mesh
        // does not acquire a spurious explicit uncertainty.
        if simpler {
            new.reg_overlay(self, true);
            Ok((new.clone_box(), true))
        } else {
            // C's `else`: `result = astClone( this )` (box.c:4066).
            Ok((self.clone_box(), false))
        }
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    /// region.c `Equal`, which this class does not extend.
    fn equal(&self, other: &dyn Mapping) -> bool {
        super::data::region_equal(self, other).is_some()
    }

    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        // Parallel merge only (series of two Regions is not a Region). The
        // direct pairwise path always has `self` as the lower-index (left)
        // component, so the Box axes go first (`intfirst = 1`). The nominate
        // loop reaches the lower-neighbour (`intfirst = 0`) case via
        // `Region::region_merge`, not through `try_merge`.
        if series || self_inv || other_inv {
            return None;
        }
        let other_reg: &dyn Region = super::as_region(other)?;
        Region::region_merge(self, other_reg, true)
    }

    fn map_merge(
        &self,
        ctx: &mut crate::mappings::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        crate::mappings::simplify::region_map_merge(self, ctx, where_, series)
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::BasicFrameBuilder;

    /// A PermMap that feeds one of its outputs with AST__BAD leaves that axis
    /// unbounded, so the Box simplifies to an Interval rather than a Box
    /// (box.c:3771, 3784).
    ///
    /// The base->current edge is installed here directly rather than through
    /// [`Region::map_region`], because `map_region` rejects a Mapping whose
    /// forward transformation yields a bad axis value (region.c:5539) — the C
    /// library does the same, so no fixture generated through a public C
    /// constructor can reach this exit.
    #[test]
    fn permmap_bad_output_simplifies_to_unbounded_interval() {
        let base = BasicFrameBuilder::new(2).build().unwrap();
        let bx = BoxRegionBuilder::from_bounds(base, &[1.0, 3.0], &[5.0, 9.0])
            .unwrap()
            .build()
            .unwrap();

        // Forward: output 1 <- input 2, output 2 <- AST__BAD.
        // Inverse: input 1 <- the constant 3.0, input 2 <- output 1.
        // Input 2 and output 1 are bi-directional, and the constant feeding
        // input 1 lies inside the Box's first-axis bounds, so the slice meets
        // the Box and the result is not a NullRegion (box.c:3806, 3825).
        let pm = PermMap::new(&[-1, 1], &[2, 0], &[3.0]).unwrap();
        let curr = BasicFrameBuilder::new(2).build().unwrap();

        let mut region: DynRegion = Box::new(bx);
        {
            let fs = region.region_frameset_mut();
            let icurr = fs.current_idx() + 1;
            fs.add_frame(FrameRef::Current, Box::new(pm), Box::new(curr))
                .unwrap();
            fs.remove_frame(FrameRef::Index(icurr)).unwrap();
        }

        let simple = <dyn Mapping>::simplify(region.as_ref()).unwrap();
        let interval = simple
            .as_any()
            .downcast_ref::<Interval>()
            .expect("a PermMap output fed AST__BAD makes the Box an Interval");

        // The Interval's defining points hold the lower then upper bound: the
        // first axis takes the Box's second-axis bounds, the second axis is
        // unbounded in both directions.
        assert_eq!(interval.data.points[0][0], 3.0);
        assert_eq!(interval.data.points[1][0], 9.0);
        assert!(is_bad(interval.data.points[0][1]));
        assert!(is_bad(interval.data.points[1][1]));
    }
}
