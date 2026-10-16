//! Mapping between grid coordinates in different HEALPix projections.
//!
//! An `XphMap` maps between grid coordinates within different types of
//! HEALPix projection (xphmap.c):
//!
//! - `Hpx0`: an HPX projection centred on RA=0h.
//! - `Hpx12`: an HPX projection centred on RA=12h.
//! - `Xphn`: an XPH ("butterfly") projection centred on the north pole.
//! - `Xphs`: an XPH projection centred on the south pole.
//!
//! The forward transformation goes from grid coordinates within the
//! nominated projection to grid coordinates within an HPX12 projection; a
//! position outside the projection's used area is bad. The HPX projection is
//! described by Calabretta and Roukema (A&A, 2005) and the XPH projection by
//! Calabretta and Lowe (PASA, 2018).
//!
//! C has no public constructor: XphMaps are made by the Moc class, which is
//! not ported, and otherwise read from a dump, as here.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::unit::UnitMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// The HEALPix projection an [`XphMap`] maps from (xphmap.h `AST__HPX0`
/// and the rest, in the same order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XphProjection {
    /// An HPX projection centred on RA=0h.
    Hpx0,
    /// An HPX projection centred on RA=12h.
    Hpx12,
    /// An XPH ("butterfly") projection centred on the north pole.
    Xphn,
    /// An XPH ("butterfly") projection centred on the south pole.
    Xphs,
}

impl XphProjection {
    const ALL: [XphProjection; 4] = [Self::Hpx0, Self::Hpx12, Self::Xphn, Self::Xphs];

    /// The name a dump uses (xphmap.c `proj_name`).
    pub fn name(self) -> &'static str {
        match self {
            Self::Hpx0 => "HPX0",
            Self::Hpx12 => "HPX12",
            Self::Xphn => "XPHN",
            Self::Xphs => "XPHS",
        }
    }

    /// The comment a dump writes with the name (xphmap.c `proj_comm`).
    fn comment(self) -> &'static str {
        match self {
            Self::Hpx0 => "HPX projection centred on RA=0h",
            Self::Hpx12 => "HPX projection centred on RA=12h",
            Self::Xphn => "XPH projection centred on north pole",
            Self::Xphs => "XPH projection centred on south pole",
        }
    }

    /// The projection a name gives, matched as C's `astChrMatch` matches.
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

    /// Transform one position, transcribing xphmap.c `Transform` (grid
    /// indices are C `int`s, so integer division truncates toward zero).
    fn transform_point(&self, forward: bool, x: f64, y: f64) -> (f64, f64) {
        const BAD: (f64, f64) = (BAD_VALUE, BAD_VALUE);
        if x == BAD_VALUE || y == BAD_VALUE {
            return BAD;
        }
        let nppf: i64 = 1 << self.order;
        let (n2, n3, n4, n5, n6, n7, n9) = (
            2 * nppf,
            3 * nppf,
            4 * nppf,
            5 * nppf,
            6 * nppf,
            7 * nppf,
            9 * nppf,
        );
        let f = |n: i64| n as f64;

        // The cell holding a position, with the low RA edges in the cell
        // and the high RA edges in the neighbouring one, and its facet;
        // `None` when the facet is not one of the HPX layout's.
        let hpx_cell = || -> Option<i64> {
            let ix = (x - 1.5).ceil() as i64;
            let iy = (y - 1.5).ceil() as i64;
            let (fx, fy) = (ix / nppf, iy / nppf);
            if (0..5).contains(&fx) && (0..5).contains(&fy) && (fx - fy).abs() <= 1 {
                Some(ix + iy + 1)
            } else {
                None
            }
        };

        if forward {
            match self.projection {
                // HPX12 to HPX12 copies positions in the used area.
                XphProjection::Hpx12 => match hpx_cell() {
                    Some(s) if s > nppf && s <= n9 => (x, y),
                    _ => BAD,
                },
                // HPX0 to HPX12 swaps the bottom left and top right halves.
                XphProjection::Hpx0 => match hpx_cell() {
                    Some(s) if s <= nppf || s > n9 => BAD,
                    Some(s) if s <= n5 => (x + f(n2), y + f(n2)),
                    Some(_) => (x - f(n2), y - f(n2)),
                    None => BAD,
                },
                // XPHN to HPX12, by the gore holding the position; a gore
                // holds its low RA boundaries but not its high RA ones.
                XphProjection::Xphn => {
                    let edge = f(n2) + 0.5;
                    if x < edge && y >= edge {
                        let ix = (x - 0.5).floor() as i64;
                        let iy = (y - 0.5).floor() as i64;
                        let s = ix + iy + 1;
                        if s < n5 && s >= n3 {
                            (f(n4) + 1.0 - x, f(n6) + 1.0 - y)
                        } else {
                            BAD
                        }
                    } else if x >= edge && y > edge {
                        let ix = (x - 0.5).floor() as i64;
                        let iy = (y - 1.5).ceil() as i64;
                        let d = ix - iy;
                        if d >= -nppf && d < nppf {
                            (y - f(nppf), f(n5) + 1.0 - x)
                        } else {
                            BAD
                        }
                    } else if x > edge && y <= edge {
                        let ix = (x - 1.5).ceil() as i64;
                        let iy = (y - 1.5).ceil() as i64;
                        let s = ix + iy + 1;
                        if s > n3 && s <= n5 {
                            (x - f(n2), y)
                        } else {
                            BAD
                        }
                    } else {
                        let ix = (x - 1.5).ceil() as i64;
                        let iy = (y - 0.5).floor() as i64;
                        let d = ix - iy;
                        if d > -nppf && d <= nppf {
                            (f(n5) + 1.0 - y, f(n3) + x)
                        } else {
                            BAD
                        }
                    }
                }
                XphProjection::Xphs => {
                    let edge = f(n2) + 0.5;
                    if x < edge && y <= edge {
                        let ix = (x - 0.5).floor() as i64;
                        let iy = (y - 1.5).ceil() as i64;
                        let d = ix - iy;
                        if d >= -nppf && d < nppf {
                            (f(n2) + y, f(n4) + 1.0 - x)
                        } else {
                            BAD
                        }
                    } else if x >= edge && y < edge {
                        let ix = (x - 0.5).floor() as i64;
                        let iy = (y - 0.5).floor() as i64;
                        let s = ix + iy + 1;
                        if s >= n3 && s < n5 {
                            (f(n5) + 1.0 - x, f(n3) + 1.0 - y)
                        } else {
                            BAD
                        }
                    } else if x > edge && y >= edge {
                        let ix = (x - 1.5).ceil() as i64;
                        let iy = (y - 0.5).floor() as i64;
                        let d = ix - iy;
                        if d <= nppf && d > -nppf {
                            (f(n4) + 1.0 - y, x - f(n2))
                        } else {
                            BAD
                        }
                    } else {
                        let ix = (x - 1.5).ceil() as i64;
                        let iy = (y - 1.5).ceil() as i64;
                        let s = ix + iy + 1;
                        if s <= n5 && s > n3 {
                            (f(n3) + x, f(nppf) + y)
                        } else {
                            BAD
                        }
                    }
                }
            }
        } else {
            // Inverse: from HPX12 to the projection, for positions in the
            // used area of the HPX12 layout, by the half or quarter of the
            // sky they lie in.
            let Some(s) = hpx_cell() else {
                return BAD;
            };
            if s <= nppf || s > n9 {
                return BAD;
            }
            match self.projection {
                XphProjection::Hpx12 => (x, y),
                XphProjection::Hpx0 => {
                    if s <= n5 {
                        (x + f(n2), y + f(n2))
                    } else {
                        (x - f(n2), y - f(n2))
                    }
                }
                XphProjection::Xphn => {
                    if s <= n3 {
                        (f(n2) + x, y)
                    } else if s <= n5 {
                        (f(n5) + 1.0 - y, f(nppf) + x)
                    } else if s <= n7 {
                        (f(n4) + 1.0 - x, f(n6) + 1.0 - y)
                    } else {
                        (y - f(n3), f(n5) + 1.0 - x)
                    }
                }
                XphProjection::Xphs => {
                    if s <= n3 {
                        (f(n2) + y, f(n4) + 1.0 - x)
                    } else if s <= n5 {
                        (f(n5) + 1.0 - x, f(n3) + 1.0 - y)
                    } else if s <= n7 {
                        (f(n4) + 1.0 - y, x - f(n2))
                    } else {
                        (x - f(n3), y - f(nppf))
                    }
                }
            }
        }
    }
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

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        let mut out = PointSet::zeros(pts.npoint(), 2);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        crate::mappings::validate_transform(self, pts, forward, Some(out))?;
        let forward = forward != self.inverted;
        for ip in 0..pts.npoint() {
            let p = pts.point(ip);
            let (x, y) = self.transform_point(forward, p[0], p[1]);
            let mut o = out.point_mut(ip);
            o[0] = x;
            o[1] = y;
        }
        Ok(())
    }

    /// C's XphMap inherits the base `Rate`, a numeric estimate.
    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        super::rate::numeric_rate(self, at, ax1, ax2)
    }

    /// xphmap.c `GetIsLinear`: only an HPX12 XphMap is linear, though even
    /// that one makes positions outside the used area bad.
    fn is_linear(&self) -> bool {
        self.projection == XphProjection::Hpx12
    }

    /// xphmap.c `MapMerge`: in series, an XphMap and an adjacent XphMap (the
    /// previous one if that is an XphMap, otherwise the next) that are equal
    /// and opposite in the orientations the merge list gives them become a
    /// UnitMap. Nothing else merges: even an HPX12 XphMap is not a UnitMap,
    /// since it makes positions outside the used area bad.
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
        ctx.replace_pair_with(imap1, Box::new(UnitMap::new(2)), false);
        Some(imap1)
    }

    /// xphmap.c `Equal`: the same Invert flag, projection and order.
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

    /// xphmap.c `astLoadXphMap`: Order defaults to 19 and Type to HPX12;
    /// a Type that is not one of the projection names is AST__OPT.
    fn from_node(node: &Node) -> AstResult<Self> {
        let whole = node;
        let node = &whole.section("XphMap");
        let base = read_mapping_base(whole);
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
        let mut m = XphMap::new(order as u32, projection);
        if base.inverted {
            m.invert();
        }
        m.simplified = SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified);
        Ok(m)
    }
}
