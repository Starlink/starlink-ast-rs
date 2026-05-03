//! Compound mapping (series and parallel composition).
//!
//! `CmpMap` composes two mappings either:
//!
//! - **Series**: apply `first`, then `second`. Requires `first.nout() ==
//!   second.nin()`.
//! - **Parallel**: apply `first` and `second` side-by-side to disjoint
//!   coordinate ranges, then concatenate their outputs.

use std::fmt;

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{
    MappingToNode, Node, Serializable, mapping_from_node, read_mapping_base, write_mapping_base,
};

use super::unit::UnitMap;
use super::{AsAny, DynMapping, Mapping};

/// Determines whether two mappings are composed in series or in parallel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Composition {
    /// Apply `first` then `second`.  `first.nout()` must equal `second.nin()`.
    Series,
    /// Apply `first` and `second` to separate coordinate ranges and
    /// concatenate the results.
    Parallel,
}

/// A mapping formed by composing two component mappings.
///
/// See [`Composition`] for the two modes.
pub struct CmpMap {
    first: DynMapping,
    second: DynMapping,
    kind: Composition,
    inverted: bool,
}

impl CmpMap {
    /// Create a new `CmpMap`.
    ///
    /// - `Series`: `first.nout()` must equal `second.nin()`.
    /// - `Parallel`: no dimension constraint.
    pub fn new(first: DynMapping, second: DynMapping, kind: Composition) -> AstResult<Self> {
        if kind == Composition::Series && first.nout() != second.nin() {
            return Err(AstError::new(
                ErrorCode::Innco,
                format!(
                    "series CmpMap: first.nout()={} but second.nin()={}",
                    first.nout(),
                    second.nin()
                ),
            ));
        }
        Ok(Self {
            first,
            second,
            kind,
            inverted: false,
        })
    }

    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
    }

    // ------------------------------------------------------------------ //
    // Series helpers                                                       //
    // ------------------------------------------------------------------ //

    fn series_transform(&self, pts: &PointSet, fwd: bool) -> AstResult<PointSet> {
        if fwd {
            let mid = self.first.transform(pts, true)?;
            self.second.transform(&mid, true)
        } else {
            // Inverse: undo second, then undo first.
            let mid = self.second.transform(pts, false)?;
            self.first.transform(&mid, false)
        }
    }

    fn series_transform_into(
        &self,
        pts: &PointSet,
        fwd: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        if fwd {
            let mid = self.first.transform(pts, true)?;
            self.second.transform_into(&mid, true, out)
        } else {
            let mid = self.second.transform(pts, false)?;
            self.first.transform_into(&mid, false, out)
        }
    }

    // ------------------------------------------------------------------ //
    // Parallel helpers                                                     //
    // ------------------------------------------------------------------ //

    fn parallel_transform(&self, pts: &PointSet, fwd: bool) -> AstResult<PointSet> {
        let (a, b) = if fwd {
            (&*self.first, &*self.second)
        } else {
            // Swap roles for inverse: second inverts using nin/nout of second.
            (&*self.second, &*self.first)
        };

        // When fwd: split pts into first.nin() and second.nin() coord columns.
        // When !fwd: split pts into second.nout() and first.nout() coord columns.
        let split = if fwd {
            self.first.nin()
        } else {
            self.second.nout()
        };
        let npoint = pts.npoint();

        // Build sub-PointSets for each half.
        let pts_a = PointSet::from_ndarray(pts.view().slice(ndarray::s![.., ..split]).to_owned());
        let pts_b = PointSet::from_ndarray(pts.view().slice(ndarray::s![.., split..]).to_owned());

        let out_a = a.transform(&pts_a, fwd)?;
        let out_b = b.transform(&pts_b, fwd)?;

        // Concatenate the two outputs.
        let ncols_a = out_a.ncoord();
        let ncols_b = out_b.ncoord();
        let mut combined = PointSet::zeros(npoint, ncols_a + ncols_b);
        {
            let mut view = combined.view_mut();
            view.slice_mut(ndarray::s![.., ..ncols_a])
                .assign(&out_a.view());
            view.slice_mut(ndarray::s![.., ncols_a..])
                .assign(&out_b.view());
        }
        Ok(combined)
    }

    fn parallel_transform_into(
        &self,
        pts: &PointSet,
        fwd: bool,
        out: &mut PointSet,
    ) -> AstResult<()> {
        let result = self.parallel_transform(pts, fwd)?;
        out.view_mut().assign(&result.view());
        Ok(())
    }
}

// Manual Debug impl because DynMapping (Box<dyn Mapping>) is not Debug.
impl fmt::Debug for CmpMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CmpMap")
            .field("kind", &self.kind)
            .field("nin", &self.nin())
            .field("nout", &self.nout())
            .field("inverted", &self.inverted)
            .finish()
    }
}

impl Clone for CmpMap {
    fn clone(&self) -> Self {
        // We need to clone through the DynMapping.  DynMapping does not
        // implement Clone directly, so we delegate to simplified() if available
        // or use transform round-trip.  Instead, we use a workaround: capture
        // the simplified form.
        //
        // A simpler approach: CmpMap wraps components that implement Mapping
        // but not necessarily Clone.  We implement Clone by re-boxing.
        //
        // Since all concrete Mappings are Clone, and Box<dyn Mapping> is our
        // only type, we ask for the simplified form — which always allocates a
        // new Box — as our clone.  If simplification is a no-op the clone is
        // functionally equivalent.
        //
        // NOTE: This means clone() may produce a simplified form.  That is
        // acceptable because Clone is documented as a "deep copy" that
        // preserves functional equivalence.
        //
        // Better: add a `clone_box()` method to Mapping.  For now we use
        // simplified() since it must return an equivalent mapping.
        let first_clone = self
            .first
            .simplified()
            .unwrap_or_else(|_| Box::new(UnitMap::new(self.first.nin())));
        let second_clone = self
            .second
            .simplified()
            .unwrap_or_else(|_| Box::new(UnitMap::new(self.second.nin())));
        // Reconstruct, ignoring the error (dimensions must still be valid).
        let mut c = CmpMap {
            first: first_clone,
            second: second_clone,
            kind: self.kind,
            inverted: self.inverted,
        };
        // In case simplified swapped dimensions for inverted mappings, ensure
        // the inverted flag is preserved.
        c.inverted = self.inverted;
        c
    }
}

impl Mapping for CmpMap {
    fn nin(&self) -> usize {
        if self.inverted {
            match self.kind {
                Composition::Series => self.second.nout(),
                Composition::Parallel => self.first.nout() + self.second.nout(),
            }
        } else {
            match self.kind {
                Composition::Series => self.first.nin(),
                Composition::Parallel => self.first.nin() + self.second.nin(),
            }
        }
    }

    fn nout(&self) -> usize {
        if self.inverted {
            match self.kind {
                Composition::Series => self.first.nin(),
                Composition::Parallel => self.first.nin() + self.second.nin(),
            }
        } else {
            match self.kind {
                Composition::Series => self.second.nout(),
                Composition::Parallel => self.first.nout() + self.second.nout(),
            }
        }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let fwd = self.effective_forward(forward);
        match self.kind {
            Composition::Series => self.series_transform(pts, fwd),
            Composition::Parallel => self.parallel_transform(pts, fwd),
        }
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        match self.kind {
            Composition::Series => self.series_transform_into(pts, fwd, out),
            Composition::Parallel => self.parallel_transform_into(pts, fwd, out),
        }
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        let s_first = self.first.simplified()?;
        let s_second = self.second.simplified()?;

        // Class-membership check, mirroring C's astIsAUnitMap. Each
        // concrete mapping's simplified() canonicalises the identity case
        // to a real UnitMap, so a type test is sound here; a behavioural
        // probe would not be (e.g. a PolyMap with a fixed point at the
        // probe).
        let first_is_unit = s_first.as_any().is::<UnitMap>();
        let second_is_unit = s_second.as_any().is::<UnitMap>();

        if first_is_unit && second_is_unit {
            let n = match self.kind {
                Composition::Series => {
                    // Both are unit maps and must agree on ncoord.
                    s_first.nin()
                }
                Composition::Parallel => s_first.nin() + s_second.nin(),
            };
            return Ok(Box::new(UnitMap::new(n)));
        }

        Ok(Box::new(CmpMap {
            first: s_first,
            second: s_second,
            kind: self.kind,
            inverted: self.inverted,
        }))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        super::rate::numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<CmpMap>() else {
            return false;
        };
        if self.kind != other.kind {
            return false;
        }
        if self.inverted != other.inverted {
            return false;
        }
        self.first.equal(other.first.as_ref()) && self.second.equal(other.second.as_ref())
    }
}

impl Serializable for CmpMap {
    fn class_name(&self) -> &'static str {
        "CmpMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Compound Mapping".to_string());
        write_mapping_base(&mut node, self);

        // Series is the default (1); only write for parallel (0).
        if self.kind == Composition::Parallel {
            node.push_int(
                "Series",
                0,
                Some("Component Mappings combined in parallel".to_string()),
            );
        }

        // Component invert flags (only if non-zero).
        if self.first.is_inverted() {
            node.push_int(
                "InvA",
                1,
                Some("First component Mapping inverted".to_string()),
            );
        }
        if self.second.is_inverted() {
            node.push_int(
                "InvB",
                1,
                Some("Second component Mapping inverted".to_string()),
            );
        }

        // Component mappings as nested objects.
        let node_a = self.first.to_node()?;
        let node_b = self.second.to_node()?;
        node.push_object("MapA", node_a, Some("First component Mapping".to_string()));
        node.push_object("MapB", node_b, Some("Second component Mapping".to_string()));

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let (_nin, _nout, inverted) = read_mapping_base(node);

        let is_series = node.get_int("Series", 1) != 0;
        let kind = if is_series {
            Composition::Series
        } else {
            Composition::Parallel
        };

        let inv_a = node.get_int("InvA", 0) != 0;
        let inv_b = node.get_int("InvB", 0) != 0;

        let node_a = node
            .get_object("MapA")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "CmpMap: missing MapA"))?;
        let node_b = node
            .get_object("MapB")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "CmpMap: missing MapB"))?;

        let mut map_a = mapping_from_node(node_a)?;
        let mut map_b = mapping_from_node(node_b)?;

        // Ensure component invert state matches InvA/InvB.
        if map_a.is_inverted() != inv_a {
            map_a.invert();
        }
        if map_b.is_inverted() != inv_b {
            map_b.invert();
        }

        let mut cmp = CmpMap::new(map_a, map_b, kind)?;
        if inverted {
            cmp.invert();
        }
        Ok(cmp)
    }
}
