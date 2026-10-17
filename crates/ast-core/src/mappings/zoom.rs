//! Uniform scaling mapping.

use crate::constants::BAD_VALUE;
use crate::coords::{Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, NodeValue, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad};
use super::matrix::MatrixMap;
use super::unit::UnitMap;
use super::winmap::{compose_linear, extract_linear_params};
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// A mapping that scales all input coordinates by a constant factor.
#[derive(Debug, Clone)]
pub struct ZoomMap {
    ncoord: usize,
    factor: f64,
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
}

impl ZoomMap {
    /// Create a new `ZoomMap` that scales `ncoord` coordinates by `factor`.
    ///
    /// Returns an error if `factor` is zero.
    pub fn new(ncoord: usize, factor: f64) -> AstResult<Self> {
        if factor == 0.0 {
            return Err(AstError::new(
                ErrorCode::Zoomi,
                "zoom factor must not be zero",
            ));
        }
        Ok(Self {
            ncoord,
            factor,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        })
    }

    /// The zoom factor — C's `Zoom` attribute.
    ///
    /// C reports 1.0 for an unset attribute (`zoommap.c:1699-1700`, where 0.0 is
    /// the unset sentinel). A constructed `ZoomMap` has its factor set; only
    /// [`ZoomMap::clear_zoom`] unsets it.
    pub fn zoom(&self) -> f64 {
        if self.factor == 0.0 { 1.0 } else { self.factor }
    }

    /// Set the zoom factor — C's `astSetZoom`.
    ///
    /// Zero is rejected with `ErrorCode::Zoomi` and leaves the factor unchanged,
    /// as C does (`zoommap.c:1703-1710`, whose assign expression evaluates to
    /// `this->zoom` after the `astError`).
    ///
    /// `Zoom` is the whole of the transformation and so feeds the first part of
    /// ZoomMap's own `MapMerge`, which can replace the run of ZoomMaps and
    /// UnitMaps it accumulates with a single `UnitMap` (`zoommap.c:747`). Both
    /// simplified records are discarded when the new value differs from the one
    /// held, and the test is on the raw argument, so a rejected zero still
    /// discards them: `astClearIsSimple` is the first term of C's comma
    /// expression and the validation lives in the term that yields the stored
    /// value (`zoommap.c:1705`, the convention at `mapping.h:449-472`).
    pub fn set_zoom(&mut self, factor: f64) -> AstResult<()> {
        if factor != self.factor {
            self.simplified.clear();
        }
        if factor == 0.0 {
            return Err(AstError::new(
                ErrorCode::Zoomi,
                "zoom factor must not be zero",
            ));
        }
        self.factor = factor;
        Ok(())
    }

    /// Whether Zoom is set (zoommap.c `TestZoom`: a non-zero stored value).
    pub fn test_zoom(&self) -> bool {
        self.factor != 0.0
    }

    /// Unset Zoom, so that it reads as 1.0 (zoommap.c `ClearZoom`, which also
    /// discards both simplified records).
    pub fn clear_zoom(&mut self) {
        self.simplified.clear();
        self.factor = 0.0;
    }

    fn effective_forward(&self, forward: bool) -> bool {
        forward != self.inverted
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

impl Mapping for ZoomMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    /// Linear, as C's `GetIsLinear` for the class reports.
    fn is_linear(&self) -> bool {
        true
    }

    fn nin(&self) -> usize {
        self.ncoord
    }
    fn nout(&self) -> usize {
        self.ncoord
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
        let fwd = self.effective_forward(forward);
        let scale = if fwd { self.zoom() } else { 1.0 / self.zoom() };
        let n = pts.npoint();
        for j in 0..pts.ncoord() {
            for (o, &v) in out.axis_mut(j)[..n].iter_mut().zip(pts.axis(j)) {
                *o = if is_bad(v) { BAD_VALUE } else { v * scale };
            }
        }
        Ok(())
    }

    /// NaN-as-bad fast path: drop the per-point `is_bad(v)` check and let
    /// the multiplication propagate NaN naturally. `ZoomMap::new` rejects
    /// `factor == 0.0` so the inverse direction's `1.0 / factor` is always
    /// well-defined; a BAD or non-finite `factor` produces garbage in both
    /// the scalar path (which only gates on `v`, not on `scale`) and this
    /// kernel, so behaviour is unchanged on those inputs.
    fn transform_into_nan(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let scale = if fwd { self.zoom() } else { 1.0 / self.zoom() };
        let n = pts.npoint();
        for j in 0..pts.ncoord() {
            for (o, &v) in out.axis_mut(j)[..n].iter_mut().zip(pts.axis(j)) {
                *o = v * scale;
            }
        }
        Ok(())
    }

    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        super::simplify::zoom_map_merge(self, ctx, where_, series)
    }

    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, _at, ax1, ax2)?;
        if ax1 != ax2 {
            return Ok(0.0);
        }
        Ok(if !self.inverted {
            self.zoom()
        } else {
            1.0 / self.zoom()
        })
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<ZoomMap>() else {
            return false;
        };
        if self.ncoord != other.ncoord {
            return false;
        }
        if self.inverted == other.inverted {
            ast_equal(self.zoom(), other.zoom())
        } else {
            ast_equal(self.zoom() * other.zoom(), 1.0)
        }
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
        // zoommap.c MapSplit: any selection of inputs in range splits off a
        // ZoomMap of as many axes with the same Zoom, a repeated one
        // included, whose Invert is set to this one's; one out of range gives
        // no split.
        if in_axes.iter().any(|&ix| ix >= self.nin()) {
            return Ok(None);
        }
        let mut sub = ZoomMap::new(in_axes.len(), self.zoom())?;
        sub.set_invert(self.inverted);
        Ok(Some((in_axes.to_vec(), Box::new(sub))))
    }

    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        if !series {
            // Parallel merge: ZoomMap alongside ZoomMap.
            let other_zoom = other.as_any().downcast_ref::<ZoomMap>()?;
            let f1 = if self_inv {
                1.0 / self.zoom()
            } else {
                self.zoom()
            };
            let f2 = if other_inv {
                1.0 / other_zoom.zoom()
            } else {
                other_zoom.zoom()
            };
            let n1 = self.ncoord;
            let n2 = other_zoom.ncoord;
            let n = n1 + n2;
            if ast_equal(f1, f2) {
                // Same factor → wider ZoomMap.
                return ZoomMap::new(n, f1).ok().map(|z| Box::new(z) as DynMapping);
            }
            // Different factors → diagonal MatrixMap, as C's zoommap.c
            // MapMerge makes it.
            let mut diag = vec![f1; n1];
            diag.extend(std::iter::repeat_n(f2, n2));
            return MatrixMap::diagonal(n, n, &diag)
                .ok()
                .map(|mm| Box::new(mm) as DynMapping);
        }

        // Fast path: ZoomMap + ZoomMap → ZoomMap (preserves type).
        if let Some(other_zoom) = other.as_any().downcast_ref::<ZoomMap>() {
            if self.ncoord != other_zoom.ncoord {
                return None;
            }
            let f1 = if self_inv {
                1.0 / self.zoom()
            } else {
                self.zoom()
            };
            let f2 = if other_inv {
                1.0 / other_zoom.zoom()
            } else {
                other_zoom.zoom()
            };
            let product = f1 * f2;
            if ast_equal(product, 1.0) {
                return Some(Box::new(UnitMap::new(self.ncoord)));
            }
            return ZoomMap::new(self.ncoord, product)
                .ok()
                .map(|m| Box::new(m) as DynMapping);
        }

        // Cross-type: ZoomMap + {WinMap, ShiftMap} → WinMap or UnitMap.
        let f1 = if self_inv {
            1.0 / self.zoom()
        } else {
            self.zoom()
        };
        let s1 = vec![0.0; self.ncoord];
        let c1 = vec![f1; self.ncoord];
        let (s2, c2) = extract_linear_params(other, other_inv)?;
        if s1.len() != s2.len() {
            return None;
        }
        // C's zoommap.c absorbs WinMap/ShiftMap neighbours by multiplying
        // their terms directly (zoommap.c:1013-1057) — no rounding snap.
        compose_linear(&s1, &c1, &s2, &c2, false)
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

impl Serializable for ZoomMap {
    fn class_name(&self) -> &'static str {
        "ZoomMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Zoom about the origin".to_string());
        write_mapping_base(&mut node, self);
        // zoommap.c Dump: Zoom is "helpful", so an unset value is written
        // commented, as its default 1.0.
        if self.test_zoom() {
            node.push_double("Zoom", self.zoom(), Some("Zoom factor".to_string()));
        } else {
            node.push_unset(
                "Zoom",
                NodeValue::Double(self.zoom()),
                Some("Zoom factor".to_string()),
            );
        }
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("ZoomMap");
        let base = read_mapping_base(whole);
        // C reads the raw value against the unset default of zero and then
        // re-applies it through `SetZoom` when `TestZoom` reports it set, i.e.
        // when it is non-zero (zoommap.c:2237-2238, zoommap.c:1712). The
        // re-applied value is the one just constructed with, so the guarded
        // setter changes nothing and the simplified record the parent loader
        // installed from the `IsSimp` card survives.
        let raw = node.get_double("Zoom", 0.0);
        let mut m = ZoomMap::new(base.nin, if raw != 0.0 { raw } else { 1.0 })?;
        // An absent Zoom stays unset, as C's `astReadDouble(..., 0.0)` leaves it.
        m.factor = raw;
        if base.inverted {
            m.invert();
        }
        m.set_has_been_simplified(base.has_been_simplified);
        if raw != 0.0 {
            m.set_zoom(raw)?;
        }
        Ok(m)
    }
}
