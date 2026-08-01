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
use crate::frames::{DynFrame, Frame, FrameRef, FrameSet};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, Mapping, UnitMap};

use super::data::{RegionData, RegionGeometry};
use super::merge_util::MergedRegion;
use super::{DynRegion, Interval, NullRegion, Overlap, PointList, Region};

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
            frame: Box::new(frame),
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
        let mut data = RegionData::new(frameset, points);
        data.uncertainty = self.uncertainty;

        Ok(BoxRegion { data })
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
    pub fn with_integrity<F>(&mut self, f: F) -> AstResult<()>
    where
        F: FnOnce(&mut dyn Frame) -> AstResult<()>,
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
            let sp1_map = Mapping::simplified(pickfrm1.as_ref())?;
            let sp2_map = Mapping::simplified(pickfrm2.as_ref())?;
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
            let simplified = Mapping::simplified(newprism.as_ref())?;
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
        let closed = self.data.closed;

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
                let p = point[ax];
                let lb = lbnd[ax];
                let ub = ubnd[ax];

                let pass = if closed {
                    lb <= p && p <= ub
                } else {
                    lb < p && p < ub
                };

                if !pass {
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
        let np = self.data.mesh_size;

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

    fn reg_centre(&self) -> AstResult<Vec<f64>> {
        Ok(self.data.points[0].clone())
    }

    fn is_bounded_inner(&self) -> bool {
        // A box is always bounded (it has finite extent in all axes).
        true
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

    fn overlap(&self, other: &dyn Region) -> AstResult<Overlap> {
        super::overlap::compute_overlap(self, other)
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
        let ps1 = super::data::region_transform(&large_box, pts, true)?;
        let ps2 = super::data::region_transform(&small_box, &ps1, true)?;

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

    fn fill_factor(&self) -> f64 {
        self.data.fill_factor
    }

    fn set_fill_factor(&mut self, factor: f64) {
        self.data.fill_factor = factor;
    }

    fn mesh_size(&self) -> usize {
        self.data.mesh_size
    }

    fn set_mesh_size(&mut self, size: usize) {
        self.data.mesh_size = size;
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
        self.reg_base_box()
    }

    fn get_region_disc(&self) -> AstResult<(Vec<f64>, f64)> {
        let centre = self.data.points[0].clone();
        let (lbnd, ubnd) = self.bounds();

        // Radius = half-diagonal of the bounding box.
        let naxes = centre.len();
        let mut sum_sq = 0.0;
        for i in 0..naxes {
            let half_width = 0.5 * (ubnd[i] - lbnd[i]);
            sum_sq += half_width * half_width;
        }
        let radius = sum_sq.sqrt();

        Ok((centre, radius))
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

    fn get_region_grid(&self, npoints: usize) -> AstResult<Vec<Vec<f64>>> {
        self.reg_base_grid(npoints)
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

    fn set_uncertainty(&mut self, unc: DynRegion) {
        self.data.uncertainty = Some(unc);
    }

    fn get_uncertainty(&self) -> Option<DynRegion> {
        self.data.uncertainty.clone()
    }

    fn reg_centre_set(&mut self, cen: &[f64]) -> AstResult<()> {
        let bfrm = self.data.frameset.base().snapshot();
        let ncb = self.data.points[0].len();

        // Geodesic half-lengths of the current box (GeoLengths, box.c:975):
        // distance along each axis from the centre to the corner.
        let centre = self.data.points[0].clone();
        let corner = self.data.points[1].clone();
        let geolen = geo_lengths(bfrm.as_ref(), ncb, &centre, &corner)?;

        // Store the new base-Frame centre, skipping BAD components (box.c:2915).
        let mut bc = centre;
        for ic in 0..ncb {
            if !is_bad(cen[ic]) {
                bc[ic] = cen[ic];
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

        // Combine the two current Frames into a CmpFrame in the requested
        // order.
        let f_this = self.data.frameset.current().snapshot();
        let f_reg = other_reg.get_region_frame();
        let cmp_frame = if intfirst {
            crate::frames::CmpFrame::new(f_this, f_reg)
        } else {
            crate::frames::CmpFrame::new(f_reg, f_this)
        };
        let frameset = FrameSet::new(Box::new(cmp_frame) as DynFrame);

        // Combine uncertainties in the requested order when both are present.
        let unc = super::merge_util::combine_uncertainties(
            self.data.uncertainty.clone(),
            other_reg.get_uncertainty(),
            intfirst,
        );

        super::merge_util::finish_region_merge(
            built,
            frameset,
            self.data.closed,
            self.data.closed_set,
            self.data.adaptive,
            unc,
        )
    }

    fn clone_region(&self) -> DynRegion {
        Box::new(self.clone())
    }
}

// ============================================================================
// Frame trait — delegate to encapsulated FrameSet's current frame
// ============================================================================

impl Frame for BoxRegion {
    fn naxes(&self) -> usize {
        self.data.frameset.current().naxes()
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

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
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
    fn nin(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    fn nout(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    fn is_inverted(&self) -> bool {
        false
    }

    fn invert(&mut self) {
        // No-op for now.
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        super::data::region_transform(self, pts, forward)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
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

    fn simplified(&self) -> AstResult<DynMapping> {
        let map = self
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;
        let simple_map = map.simplified()?;
        if simple_map.as_ref().as_any().is::<UnitMap>() {
            return Ok(self.clone_box());
        }
        // Transform defining points to current frame to form a candidate
        // remapped Box.
        let pts = PointSet::from_rows(self.data.points.iter().map(|p| p.as_slice()));
        let transformed = simple_map.transform(&pts, true)?;
        let current_frame = self.data.frameset.current().snapshot();
        let naxes = current_frame.naxes();
        let new_points: Vec<Vec<f64>> = (0..self.data.points.len())
            .map(|i| transformed.point(i)[..naxes].to_vec())
            .collect();
        let frameset = FrameSet::new(current_frame);
        let mut data = RegionData::new(frameset, new_points);
        data.negated = self.data.negated;
        data.closed = self.data.closed;
        data.closed_set = self.data.closed_set;
        data.mesh_size = self.data.mesh_size;
        data.fill_factor = self.data.fill_factor;
        data.adaptive = self.data.adaptive;
        let candidate = BoxRegion { data };

        // A direct point transform only yields a valid axis-aligned Box when
        // the base->current Mapping preserves axis-alignment (diagonal
        // scale, shift, permutation). For a shear or rotation the candidate
        // box is strictly larger than the true region; verify via the
        // boundary round-trip (box.c::Simplify, 3905). If the candidate is
        // not exact, no simplification is possible — return the original Box
        // unchanged (keeping its FrameSet).
        let candidate_mesh = candidate.reg_base_mesh()?;
        if !self.verify_remap_exact(simple_map.as_ref(), &candidate_mesh)? {
            // The Box could not be re-defined as a Box in its current Frame.
            // For a 2-D→2-D Box, try representing it as a Polygon whose four
            // vertices are the transformed Box corners (box.c::Simplify, 3921).
            if simple_map.nin() == 2 && simple_map.nout() == 2 {
                if let Some(poly) = self.convert_to_polygon(simple_map.as_ref())? {
                    return Ok(poly.clone_box());
                }
            }
            // Otherwise try splitting it into two sub-regions on distinct axis
            // subsets and joining them into a Prism (box.c::Simplify, 4036,
            // calls astConvertToPrism when no simpler single Region was found).
            if let Some(prism) = self.convert_to_prism()? {
                return Ok(prism.clone_box());
            }
            return Ok(self.clone_box());
        }
        Ok(Box::new(candidate))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<BoxRegion>() else {
            return false;
        };
        self.data.negated == other.data.negated
            && self.data.closed == other.data.closed
            && self.data.points == other.data.points
            && Frame::equal(self.data.frameset.current(), other.data.frameset.current())
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
