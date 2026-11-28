// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Mapping between grid coordinates in different HEALPix projections.
//!
//! An `XphMap` maps between grid coordinates within different types of
//! HEALPix projection:
//!
//! - `Hpx0`: an HPX projection centered on RA=0h.
//! - `Hpx12`: an HPX projection centered on RA=12h.
//! - `Xphn`: an XPH ("butterfly") projection centered on the north pole.
//! - `Xphs`: an XPH projection centered on the south pole.
//!
//! The forward transformation goes from grid coordinates within the
//! nominated projection to grid coordinates within an HPX12 projection. The
//! HPX projection is described by Calabretta and Roukema (A&A, 2005) and the
//! XPH projection by Calabretta and Lowe (PASA, 2018).
//!
//! Each grid is divided into cells one pixel across, of which only some are
//! used, each cell of the sky being one used cell of each grid.
//! A position is on the sky if it is in a used cell or on its boundary, and
//! is transformed by that cell; a position within the HPX projection's
//! rounding margin of the boundary is on it.
//! A position on the boundary of several used cells is transformed by one of
//! them; a pole, which is four positions in an HPX grid, transforms to a
//! position of the same pole.
//! An HPX grid holds an unused copy of each cell of the facet split between
//! its bottom left and top right corners, and a position in the unused copy
//! is moved to the used copy, in both directions.
//! A position that is not on the sky, including every position outside the
//! grid, gives bad values.
//!
//! An XphMap has no public constructor: the Moc class makes them, and
//! otherwise they are read from a dump.

use crate::constants::BAD_VALUE;
use crate::coords::{Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::helpers::is_bad;
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::unit::UnitMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// The HEALPix projection an [`XphMap`] maps from.
// C: xphmap.h `AST__HPX0` and the rest, in the same order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XphProjection {
    /// An HPX projection centered on RA=0h.
    Hpx0,
    /// An HPX projection centered on RA=12h.
    Hpx12,
    /// An XPH ("butterfly") projection centered on the north pole.
    Xphn,
    /// An XPH ("butterfly") projection centered on the south pole.
    Xphs,
}

impl XphProjection {
    const ALL: [XphProjection; 4] = [Self::Hpx0, Self::Hpx12, Self::Xphn, Self::Xphs];

    /// The name a dump uses.
    // C: xphmap.c `proj_name`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Hpx0 => "HPX0",
            Self::Hpx12 => "HPX12",
            Self::Xphn => "XPHN",
            Self::Xphs => "XPHS",
        }
    }

    /// The comment a dump writes with the name.
    // C: xphmap.c `proj_comm`.
    fn comment(self) -> &'static str {
        match self {
            Self::Hpx0 => "HPX projection centred on RA=0h",
            Self::Hpx12 => "HPX projection centred on RA=12h",
            Self::Xphn => "XPH projection centred on north pole",
            Self::Xphs => "XPH projection centred on south pole",
        }
    }

    /// The projection a name gives, or `None` for a name that is none of them.
    ///
    /// The comparison ignores case and trailing white space.
    pub fn from_name(name: &str) -> Option<Self> {
        let key = crate::mappings::helpers::chr_match_key(name);
        Self::ALL.into_iter().find(|p| p.name() == key)
    }
}

/// A Mapping between grid coordinates within different types of HEALPix
/// projection, at a given HEALPix order.
#[derive(Debug, Clone)]
pub struct XphMap {
    /// The HEALPix order: each facet has `2^order` pixels along each edge.
    order: u32,
    /// The projection the forward transformation maps from.
    projection: XphProjection,
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
}

impl XphMap {
    /// An XphMap from grid coordinates in `projection` to grid coordinates
    /// in an HPX12 projection, at HEALPix order `order`. Crate-private, as
    /// C's `astXphMap` is protected.
    pub(crate) fn new(order: u32, projection: XphProjection) -> Self {
        Self {
            order,
            projection,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        }
    }

    /// The HEALPix order.
    pub fn order(&self) -> u32 {
        self.order
    }

    /// The projection the forward transformation maps from.
    pub fn projection(&self) -> XphProjection {
        self.projection
    }

    /// Transform one position.
    ///
    /// Each grid is divided into square cells, one pixel across, of which
    /// only some are used, and a position is transformed by the used cell
    /// that holds it, within the cell or on its boundary; a position that no
    /// used cell holds is not on the sky and gives bad values.
    /// A position within the HPX projection's rounding margin of the edge of
    /// a cell is on the edge, and a position in the unused copy of a cell of
    /// an HPX grid is moved to the used copy.
    /// Cell and facet indices are whole numbers held in doubles, since a
    /// position may be any distance from the grid.
    // C: xphmap.c Transform
    fn transform_point(&self, forward: bool, x: f64, y: f64) -> (f64, f64) {
        const BAD: (f64, f64) = (BAD_VALUE, BAD_VALUE);
        // C tests for AST__BAD only; NaN and infinity are bad here too, as
        // for every Mapping, rather than taken as a position in a cell.
        if is_bad(x) || is_bad(y) {
            return BAD;
        }
        let nppf = f64::from(1u32 << self.order);
        let (n2, n3, n4, n5, n6, n7) = (
            2.0 * nppf,
            3.0 * nppf,
            4.0 * nppf,
            5.0 * nppf,
            6.0 * nppf,
            7.0 * nppf,
        );

        // A step of one pixel along either axis changes each coordinate in
        // the plane of the projection by 45/nppf degrees, so the HPX
        // projection's rounding margin in grid units is this.
        let tol = HPXTOL * nppf / 45.0;

        let (mut x, mut y) = (x, y);
        if forward {
            match self.projection {
                // HPX12 to HPX12 is a unit transformation of the positions on
                // the sky; HPX0 to HPX12 also swaps the bottom left and top
                // right halves of the grid.
                XphProjection::Hpx12 | XphProjection::Hpx0 => {
                    let Some(s) = find_hpx_cell(&mut x, &mut y, nppf, tol, true) else {
                        return BAD;
                    };
                    if self.projection == XphProjection::Hpx0 {
                        if s <= n5 {
                            x += n2;
                            y += n2;
                        } else {
                            x -= n2;
                            y -= n2;
                        }
                    }
                    (x, y)
                }
                // XPHN or XPHS to HPX12, by the gore of the used cell that
                // holds the position.
                XphProjection::Xphn | XphProjection::Xphs => {
                    let Some(gore) = find_xph_cell(self.projection, &mut x, &mut y, nppf, tol)
                    else {
                        return BAD;
                    };
                    let (xin, yin) = (x, y);
                    if self.projection == XphProjection::Xphn {
                        match gore {
                            0 => (n4 + 1.0 - xin, n6 + 1.0 - yin),
                            3 => (yin - nppf, n5 + 1.0 - xin),
                            2 => (xin - n2, yin),
                            _ => (n5 + 1.0 - yin, n3 + xin),
                        }
                    } else {
                        match gore {
                            0 => (n2 + yin, n4 + 1.0 - xin),
                            3 => (n5 + 1.0 - xin, n3 + 1.0 - yin),
                            2 => (n4 + 1.0 - yin, xin - n2),
                            _ => (n3 + xin, nppf + yin),
                        }
                    }
                }
            }
        } else {
            // HPX12 to the projection, by the sum of the indices of the used
            // cell that holds the position.
            let Some(s) = find_hpx_cell(&mut x, &mut y, nppf, tol, true) else {
                return BAD;
            };
            let (xin, yin) = (x, y);
            match self.projection {
                XphProjection::Hpx12 => (xin, yin),
                XphProjection::Hpx0 => {
                    if s <= n5 {
                        (xin + n2, yin + n2)
                    } else {
                        (xin - n2, yin - n2)
                    }
                }
                XphProjection::Xphn => {
                    if s <= n3 {
                        (n2 + xin, yin)
                    } else if s <= n5 {
                        (n5 + 1.0 - yin, nppf + xin)
                    } else if s <= n7 {
                        (n4 + 1.0 - xin, n6 + 1.0 - yin)
                    } else {
                        (yin - n3, n5 + 1.0 - xin)
                    }
                }
                XphProjection::Xphs => {
                    if s <= n3 {
                        (n2 + yin, n4 + 1.0 - xin)
                    } else if s <= n5 {
                        (n5 + 1.0 - xin, n3 + 1.0 - yin)
                    } else if s <= n7 {
                        (n4 + 1.0 - yin, xin - n2)
                    } else {
                        (xin - n3, yin - nppf)
                    }
                }
            }
        }
    }
}

/// The margin, in degrees, that the HPX projection allows for rounding.
// C: xphmap.h AST__HPXTOL
pub(crate) const HPXTOL: f64 = 1.0e-12;

/// The cells that hold a position on one axis of a grid.
///
/// `v` is the grid coordinate, with the center of the first cell at 1.0 and
/// its lower edge at 0.5; a cell owns its upper edge if `upper`, and its
/// lower edge otherwise.
/// Returns the number of cells, two if `v` is within `tol` of a cell edge
/// and one otherwise, their zero-based indices with the owner first, and the
/// coordinate to use with each: `v`, then the edge, which lies in the second
/// cell.
/// The second elements are 0.0 when there is one cell.
// C: xphmap.c EdgeCells
fn edge_cells(v: f64, tol: f64, upper: bool) -> (usize, [f64; 2], [f64; 2]) {
    let owner = if upper {
        (v - 1.5).ceil()
    } else {
        (v - 0.5).floor()
    };

    // The edges are at half-integer coordinates; the cells either side of
    // the nearest one have zero-based indices `edge - 1.5` and `edge - 0.5`.
    let edge = v.floor() + 0.5;
    if (v - edge).abs() <= tol {
        let other = if owner < edge - 1.0 {
            edge - 0.5
        } else {
            edge - 1.5
        };
        return (2, [owner, other], [v, edge]);
    }
    (1, [owner, 0.0], [v, 0.0])
}

/// Whether the cell with zero-based indices `ix`, `iy` of an HPX grid is
/// used.
///
/// The grid holds five facets along each axis, and those on its diagonal and
/// either side of it cover the sky.
/// The bottom left and top right facets are the same facet, and only one
/// copy of each of its cells is used: the copy in the top right half of the
/// bottom left facet or in the bottom left half of the top right facet, with
/// the cells on the diagonal between the halves used in the top right facet.
/// Returns `Some(shift)` if the cell is used, or is an unused copy and
/// `wrap`; adding `shift` to both grid coordinates of a position in the cell
/// gives the position in the used copy, and it is 0.0 unless an unused copy
/// was accepted.
// C: xphmap.c HpxCellUsed
fn hpx_cell_used(ix: f64, iy: f64, nppf: f64, wrap: bool) -> Option<f64> {
    // The facet indices, counted from the bottom left facet.
    let fx = (ix / nppf).floor();
    let fy = (iy / nppf).floor();
    if (0.0..5.0).contains(&fx) && (0.0..5.0).contains(&fy) && (fx - fy).abs() <= 1.0 {
        // The sum of the cell indices, plus one.
        let s = ix + iy + 1.0;

        // The unused copies are in the bottom left half of the bottom left
        // facet and the top right half of the top right facet, four facets
        // from the used copies on both axes.
        if s > nppf && s <= 9.0 * nppf {
            return Some(0.0);
        } else if wrap {
            return Some(if s <= nppf { 4.0 * nppf } else { -4.0 * nppf });
        }
    }
    None
}

/// The used cell of an HPX grid that holds the position `x`, `y`.
///
/// A cell owns its low RA edges, its upper edges on both axes; if the owner
/// of the position is not used, the cells on the other side of any edge
/// within `tol` of the position are tried.
/// Returns the sum of the zero-based indices of the used cell plus one, with
/// `x`, `y` moved onto the edge of the used cell if it is not the owner, and
/// to the used copy of the cell if `wrap` accepted an unused copy.
/// `None` leaves `x`, `y` unchanged.
// C: xphmap.c FindHpxCell
fn find_hpx_cell(x: &mut f64, y: &mut f64, nppf: f64, tol: f64, wrap: bool) -> Option<f64> {
    let (nx, ix, xs) = edge_cells(*x, tol, true);
    let (ny, iy, ys) = edge_cells(*y, tol, true);

    // The cell that owns the position first.
    for j in 0..ny {
        for i in 0..nx {
            if let Some(shift) = hpx_cell_used(ix[i], iy[j], nppf, wrap) {
                *x = xs[i] + shift;
                *y = ys[j] + shift;
                return Some(ix[i] + iy[j] + 1.0 + 2.0 * shift);
            }
        }
    }
    None
}

/// The gore, 0 to 3, that holds the cell with zero-based indices `ix`, `iy`
/// of an XPH grid, if the cell is used.
///
/// The grid holds four facets along each axis and has a gore in each
/// quadrant.
/// The used cells of a gore form a band two facets wide along the diagonal
/// of the quadrant, from the center of the grid to its corner, and the cells
/// on each edge of the band, cut in half by the boundary between two gores,
/// are used in the gore on their low RA side.
// C: xphmap.c XphCellGore
fn xph_cell_gore(projection: XphProjection, ix: f64, iy: f64, nppf: f64) -> Option<usize> {
    if !(ix >= 0.0 && ix < 4.0 * nppf && iy >= 0.0 && iy < 4.0 * nppf) {
        return None;
    }

    // The quadrant that holds the cell.
    let left = ix < 2.0 * nppf;
    let bottom = iy < 2.0 * nppf;

    // The band runs along a diagonal of constant index sum in the top left
    // and bottom right quadrants, and of constant index difference in the
    // other two.
    let n3 = 3.0 * nppf;
    let n5 = 5.0 * nppf;
    let s = ix + iy + 1.0;
    let d = ix - iy;

    let (gore, used) = if projection == XphProjection::Xphn {
        if left && !bottom {
            (0, s >= n3 && s < n5)
        } else if !left && !bottom {
            (3, d >= -nppf && d < nppf)
        } else if !left && bottom {
            (2, s > n3 && s <= n5)
        } else {
            (1, d > -nppf && d <= nppf)
        }
    } else if left && bottom {
        (0, d >= -nppf && d < nppf)
    } else if !left && bottom {
        (3, s >= n3 && s < n5)
    } else if !left && !bottom {
        (2, d > -nppf && d <= nppf)
    } else {
        (1, s > n3 && s <= n5)
    };
    used.then_some(gore)
}

/// The gore, 0 to 3, of the used cell of an XPH grid that holds the position
/// `x`, `y`.
///
/// The grid's four quadrants meet at the pole at its center and each holds
/// one gore; a gore contains its low RA boundaries but not its high RA ones,
/// and a cell owns its low RA edges, which depend on the gore.
/// If the owner of the position is not a used cell of the gore, the cells on
/// the other side of any edge within `tol` of the position are tried, a cell
/// in the same gore before a cell in another.
/// `x`, `y` are moved onto the edge of the used cell if it is not the owner;
/// `None` leaves them unchanged.
// C: xphmap.c FindXphCell
fn find_xph_cell(
    projection: XphProjection,
    x: &mut f64,
    y: &mut f64,
    nppf: f64,
    tol: f64,
) -> Option<usize> {
    // The gore that holds the position, and which cell edges are its low RA
    // edges; the pole at the center of the grid is in gore 1.
    let mid = 2.0 * nppf + 0.5;
    let (gore, upx, upy) = if projection == XphProjection::Xphn {
        if *x < mid && *y >= mid {
            (0, false, false)
        } else if *x >= mid && *y > mid {
            (3, false, true)
        } else if *x > mid && *y <= mid {
            (2, true, true)
        } else {
            (1, true, false)
        }
    } else if *x < mid && *y <= mid {
        (0, false, true)
    } else if *x >= mid && *y < mid {
        (3, false, false)
    } else if *x > mid && *y >= mid {
        (2, true, false)
    } else {
        (1, true, true)
    };

    let (nx, ix, xs) = edge_cells(*x, tol, upx);
    let (ny, iy, ys) = edge_cells(*y, tol, upy);

    // The cell that owns the position first; the first pass accepts only a
    // cell in the gore found above, and the second a cell in any gore.
    for pass in 0..2 {
        for j in 0..ny {
            for i in 0..nx {
                if let Some(g) = xph_cell_gore(projection, ix[i], iy[j], nppf)
                    && (pass == 1 || g == gore)
                {
                    *x = xs[i];
                    *y = ys[j];
                    return Some(g);
                }
            }
        }
    }
    None
}

impl Mapping for XphMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    fn nin(&self) -> usize {
        2
    }

    fn nout(&self) -> usize {
        2
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        let forward = forward != self.inverted;
        let (xs, ys) = (pts.axis(0), pts.axis(1));
        let Some([ox, oy]) = out.first_axes_mut() else {
            unreachable!("an XphMap has two outputs")
        };
        for ip in 0..pts.npoint() {
            let (x, y) = self.transform_point(forward, xs[ip], ys[ip]);
            ox[ip] = x;
            oy[ip] = y;
        }
        Ok(())
    }

    /// A numerical estimate of the rate, as [`Mapping::rate`] describes for any
    /// Mapping without an analytic one.
    // C's XphMap inherits the base Rate.
    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        super::rate::numeric_rate(self, at, ax1, ax2)
    }

    /// Only an HPX12 XphMap is linear, though even
    /// that one makes positions outside the used area bad.
    // C: xphmap.c `GetIsLinear`.
    fn is_linear(&self) -> bool {
        self.projection == XphProjection::Hpx12
    }

    /// In series, an XphMap and an adjacent XphMap (the
    /// previous one if that is an XphMap, otherwise the next) that are equal
    /// and opposite in the orientations the merge list gives them become a
    /// UnitMap. Nothing else merges: even an HPX12 XphMap is not a UnitMap,
    /// since it makes positions outside the used area bad.
    // C: xphmap.c `MapMerge`.
    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        if !series {
            return None;
        }
        let is_xph = |i: usize| ctx.entry(i).0.as_any().is::<XphMap>();
        let imap1 = if where_ > 0 && is_xph(where_ - 1) {
            where_ - 1
        } else if where_ + 1 < ctx.len() && is_xph(where_ + 1) {
            where_
        } else {
            return None;
        };
        let oriented = |i: usize| -> DynMapping {
            let (map, inv, _) = ctx.entry(i);
            let mut m = map.clone_box();
            m.set_invert(*inv);
            m
        };
        let mut first = oriented(imap1);
        first.invert();
        if !first.equal(oriented(imap1 + 1).as_ref()) {
            return None;
        }
        ctx.replace_pair_with(imap1, Box::new(UnitMap::within_limit(2)), false);
        Some(imap1)
    }

    /// The same Invert flag, projection and order.
    // C: xphmap.c `Equal`.
    fn equal(&self, other: &dyn Mapping) -> bool {
        other.as_any().downcast_ref::<XphMap>().is_some_and(|o| {
            self.inverted == o.inverted && self.projection == o.projection && self.order == o.order
        })
    }

    fn has_forward(&self) -> bool {
        true
    }

    fn has_inverse(&self) -> bool {
        true
    }

    fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    fn set_ident(&mut self, ident: &str) {
        self.ident = Some(ident.to_string());
    }

    fn clear_ident(&mut self) {
        self.ident = None;
    }

    fn test_ident(&self) -> bool {
        self.ident.is_some()
    }

    fn id(&self) -> &str {
        self.id.get()
    }
    fn set_id(&mut self, id: &str) {
        self.id.set(id);
    }
    fn clear_id(&mut self) {
        self.id.clear();
    }
    fn test_id(&self) -> bool {
        self.id.test()
    }

    fn has_been_simplified(&self) -> bool {
        self.simplified.get(self.inverted)
    }

    fn set_has_been_simplified(&mut self, flag: bool) {
        self.simplified.set(self.inverted, flag);
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

impl Serializable for XphMap {
    fn class_name(&self) -> &'static str {
        "XphMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("HPX variant mapping".to_string());
        write_mapping_base(&mut node, self);
        node.push_int(
            "Order",
            self.order as i64,
            Some("HEALPix order".to_string()),
        );
        node.push_string(
            "Type",
            self.projection.name(),
            Some(self.projection.comment().to_string()),
        );
        Ok(node)
    }

    /// Order defaults to 19 and Type to HPX12;
    /// a Type that is not one of the projection names is AST__OPT.
    // C: xphmap.c `astLoadXphMap`.
    fn from_node(node: &Node) -> AstResult<Self> {
        let whole = node;
        let node = &whole.section("XphMap");
        let base = read_mapping_base(whole);
        crate::serial::check_loaded_coords(&whole.class, &base, 2, 2)?;
        let order = node.get_int("Order", 19);
        let name = node
            .get_string_if_set("Type")
            .unwrap_or_else(|| XphProjection::Hpx12.name().to_string());
        let projection = XphProjection::from_name(&name).ok_or_else(|| {
            AstError::new(
                ErrorCode::Opt,
                format!(
                    "astRead(XphMap): Illegal value '{name}' supplied for the XphMap \
                     component 'Type'."
                ),
            )
        })?;
        // The order is a shift count, so it must be one the constructor
        // accepts (xphmap.c `astLoadXphMap`).
        let max = crate::regions::MOC_MAX_ORDER;
        if !(0..=i64::from(max)).contains(&order) {
            return Err(AstError::new(
                ErrorCode::Lderr,
                format!(
                    "astLoad({0}): The HEALPix order read for a {0} ({order}) is not in the \
                     range 0 to {max}.",
                    whole.class
                ),
            ));
        }
        let mut m = XphMap::new(order as u32, projection);
        if base.inverted {
            m.invert();
        }
        m.simplified = SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified);
        Ok(m)
    }
}
