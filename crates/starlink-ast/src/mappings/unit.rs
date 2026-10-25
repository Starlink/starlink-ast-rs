// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The identity mapping.

use crate::constants::BAD_VALUE;
use crate::coords::{Points, PointsMut};
use crate::error::AstResult;
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// Unit (null) Mapping.
///
/// A UnitMap is a Mapping that has no effect on the coordinates supplied to
/// it: they are simply copied.
/// This is useful where a Mapping is required, for example to pass to another
/// function, but it should have no effect.
///
/// Its [`nin`](Mapping::nin) and [`nout`](Mapping::nout) are always equal
/// and are fixed when it is created.
#[derive(Debug, Clone)]
pub struct UnitMap {
    ncoord: usize,
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
}

impl UnitMap {
    /// Create a UnitMap with `ncoord` input and output coordinates.
    ///
    /// ```
    /// use starlink_ast::mappings::{Mapping, UnitMap};
    ///
    /// let map = UnitMap::new(2);
    /// let (x, y) = map.tran2(&[1.0, 2.0], &[3.0, 4.0], true).unwrap();
    /// assert_eq!((x, y), (vec![1.0, 2.0], vec![3.0, 4.0]));
    /// ```
    #[doc(alias = "astUnitMap")]
    pub fn new(ncoord: usize) -> Self {
        Self {
            ncoord,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        }
    }

    /// Compose in series with another mapping.
    pub fn then(self, next: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(Box::new(self), next, Composition::Series)
    }

    /// Compose in parallel with another mapping.
    pub fn alongside(self, other: DynMapping) -> AstResult<CmpMap> {
        CmpMap::new(Box::new(self), other, Composition::Parallel)
    }
}

impl Mapping for UnitMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    /// Always true: a UnitMap is linear.
    // C's GetIsLinear for the class reports it linear.
    fn is_linear(&self) -> bool {
        true
    }

    fn nin(&self) -> usize {
        self.ncoord
    }
    fn nout(&self) -> usize {
        self.ncoord
    }
    /// Whether the UnitMap is inverted.
    ///
    /// Inverting a UnitMap has no effect on how it behaves.
    /// See [`Mapping::is_inverted`].
    fn is_inverted(&self) -> bool {
        self.inverted
    }
    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    /// A copy of the input, with a NaN, which is a bad value, made
    /// `BAD_VALUE`.
    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        let n = pts.npoint();
        for j in 0..pts.ncoord() {
            for (o, &v) in out.axis_mut(j)[..n].iter_mut().zip(pts.axis(j)) {
                *o = if v.is_nan() { BAD_VALUE } else { v };
            }
        }
        Ok(())
    }

    /// The NaN protocol carries bad values as NaN, so this is a plain copy.
    fn transform_into_nan(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        let n = pts.npoint();
        for j in 0..pts.ncoord() {
            out.axis_mut(j)[..n].copy_from_slice(pts.axis(j));
        }
        Ok(())
    }

    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, _at, ax1, ax2)?;
        Ok(if ax1 == ax2 { 1.0 } else { 0.0 })
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<UnitMap>() else {
            return false;
        };
        self.ncoord == other.ncoord
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        // C unitmap.c::MapMerge (unitmap.c:357).
        if ctx.len() == 1 {
            // A lone UnitMap: clear a set Invert flag, reporting the change
            // (unitmap.c:508). Matches C returning `result = 0` when the
            // single-map Invert flag was set.
            let (map, eff, orig) = ctx.entry(0);
            if !*eff {
                return None;
            }
            let entry = (map.clone(), false, *orig);
            ctx.set_pair_single(0, entry);
            return Some(0);
        }

        // In series with any other Mapping a UnitMap is simply eliminated
        // (unitmap.c:516-538): the following elements shuffle down and the index
        // of the first modified element is `where_`. Removing it in the loop
        // is what advances the nominate cursor as C does: the map that
        // shuffled into `where_` is nominated next, which is how C reaches
        // e.g. `MatrixMap::MapMerge` (MatWin2) for a Win-Unit-Matrix run.
        if series {
            ctx.remove_at(where_);
            return Some(where_);
        }

        // In parallel, the UnitMap merges with the run of UnitMaps on either
        // side of it (unitmap.c:539-571). C tests the class name, so a
        // subclass would not join the run.
        let is_unit = |i: usize| ctx.entry(i).0.as_ref().as_any().is::<UnitMap>();
        let (mut i1, mut i2) = (where_, where_);
        let mut nin = ctx.entry(where_).0.nin();
        while i1 > 0 && is_unit(i1 - 1) {
            i1 -= 1;
            nin += ctx.entry(i1).0.nin();
        }
        while i2 + 1 < ctx.len() && is_unit(i2 + 1) {
            i2 += 1;
            nin += ctx.entry(i2).0.nin();
        }

        if i1 == i2 {
            // Nothing to merge with: clear the nominated UnitMap's Invert
            // flag (unitmap.c:572-587). Only the flag the list applies it
            // with changes; the UnitMap keeps its own Invert attribute, which
            // is why C's result dumps `Invert = 1` on the UnitMap and no
            // `InvA`/`InvB` on the CmpMap.
            let (map, eff, orig) = ctx.entry(where_);
            if !*eff {
                return None;
            }
            let entry = (map.clone(), false, *orig);
            ctx.set_pair_single(where_, entry);
            return Some(where_);
        }

        // Replace the run with one UnitMap applied forward (unitmap.c:588-624).
        ctx.replace_range_with(i1, i2, Box::new(UnitMap::new(nin)));
        Some(i1)
    }

    fn map_split_zero_based(
        &self,
        in_axes: &[usize],
    ) -> AstResult<Option<(Vec<usize>, DynMapping)>> {
        // C allocates the returned output indices with astMalloc, which
        // gives NULL for none, so an empty selection gives no split.
        if in_axes.is_empty() {
            return Ok(None);
        }
        // unitmap.c MapSplit: any selection of inputs in range splits off a
        // UnitMap of as many axes, a repeated one included; one out of range
        // gives no split.
        if in_axes.iter().any(|&ix| ix >= self.nin()) {
            return Ok(None);
        }
        Ok(Some((
            in_axes.to_vec(),
            Box::new(UnitMap::new(in_axes.len())),
        )))
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

impl Serializable for UnitMap {
    fn class_name(&self) -> &'static str {
        "UnitMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Unit (null) Mapping".to_string());
        write_mapping_base(&mut node, self);
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let base = read_mapping_base(node);
        Ok(UnitMap {
            ncoord: base.nin,
            inverted: base.inverted,
            simplified: SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        })
    }
}
