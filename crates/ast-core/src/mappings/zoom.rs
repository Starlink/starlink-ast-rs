//! Uniform scaling mapping.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::cmpmap::{CmpMap, Composition};
use super::helpers::{ast_equal, is_bad};
use super::matrix::MatrixMap;
use super::unit::UnitMap;
use super::winmap::{compose_linear, extract_linear_params};
use super::{DynMapping, Mapping};

/// A mapping that scales all input coordinates by a constant factor.
#[derive(Debug, Clone)]
pub struct ZoomMap {
    ncoord: usize,
    factor: f64,
    inverted: bool,
    do_not_simplify: bool,
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
            do_not_simplify: false,
        })
    }

    /// The zoom factor.
    pub(crate) fn factor(&self) -> f64 {
        self.factor
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
    fn nin(&self) -> usize {
        self.ncoord
    }
    fn nout(&self) -> usize {
        self.ncoord
    }
    fn is_inverted(&self) -> bool {
        self.inverted
    }
    fn invert(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        let mut out = PointSet::zeros(pts.npoint(), self.ncoord);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        let fwd = self.effective_forward(forward);
        let scale = if fwd { self.factor } else { 1.0 / self.factor };
        for i in 0..pts.npoint() {
            let inp = pts.point(i);
            let outp = out.point_mut(i);
            for (o, &v) in outp.iter_mut().zip(inp.iter()) {
                *o = if is_bad(v) { BAD_VALUE } else { v * scale };
            }
        }
        Ok(())
    }

    fn simplified(&self) -> AstResult<DynMapping> {
        if self.do_not_simplify {
            return Ok(Box::new(self.clone()));
        }
        let effective_factor = if self.inverted {
            1.0 / self.factor
        } else {
            self.factor
        };
        if effective_factor == 1.0 {
            Ok(Box::new(UnitMap::new(self.ncoord)))
        } else {
            Ok(Box::new(ZoomMap {
                ncoord: self.ncoord,
                factor: effective_factor,
                inverted: false,
                do_not_simplify: false,
            }))
        }
    }

    fn rate(&self, _at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        if ax1 != ax2 {
            return Ok(0.0);
        }
        Ok(if !self.inverted {
            self.factor
        } else {
            1.0 / self.factor
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
            ast_equal(self.factor, other.factor)
        } else {
            ast_equal(self.factor * other.factor, 1.0)
        }
    }

    fn map_split(&self, first_out: usize, nout_wanted: usize) -> Option<DynMapping> {
        if nout_wanted == 0 || first_out + nout_wanted > self.ncoord {
            return None;
        }
        ZoomMap::new(nout_wanted, self.factor).ok().map(|mut z| {
            if self.inverted {
                z.invert();
            }
            Box::new(z) as DynMapping
        })
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
                1.0 / self.factor
            } else {
                self.factor
            };
            let f2 = if other_inv {
                1.0 / other_zoom.factor
            } else {
                other_zoom.factor
            };
            let n1 = self.ncoord;
            let n2 = other_zoom.ncoord;
            let n = n1 + n2;
            if ast_equal(f1, f2) {
                // Same factor → wider ZoomMap.
                return ZoomMap::new(n, f1).ok().map(|z| Box::new(z) as DynMapping);
            }
            // Different factors → diagonal MatrixMap.
            use ndarray::Array2;
            let mut diag = Array2::<f64>::zeros((n, n));
            for i in 0..n1 {
                diag[[i, i]] = f1;
            }
            for i in 0..n2 {
                diag[[n1 + i, n1 + i]] = f2;
            }
            return MatrixMap::new(diag)
                .ok()
                .map(|mm| Box::new(mm) as DynMapping);
        }

        // Fast path: ZoomMap + ZoomMap → ZoomMap (preserves type).
        if let Some(other_zoom) = other.as_any().downcast_ref::<ZoomMap>() {
            if self.ncoord != other_zoom.ncoord {
                return None;
            }
            let f1 = if self_inv {
                1.0 / self.factor
            } else {
                self.factor
            };
            let f2 = if other_inv {
                1.0 / other_zoom.factor
            } else {
                other_zoom.factor
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
            1.0 / self.factor
        } else {
            self.factor
        };
        let s1 = vec![0.0; self.ncoord];
        let c1 = vec![f1; self.ncoord];
        let (s2, c2) = extract_linear_params(other, other_inv)?;
        if s1.len() != s2.len() {
            return None;
        }
        compose_linear(&s1, &c1, &s2, &c2)
    }

    fn do_not_simplify(&self) -> bool {
        self.do_not_simplify
    }

    fn set_do_not_simplify(&mut self, flag: bool) {
        self.do_not_simplify = flag;
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
        if self.factor != 1.0 {
            node.push_double("Zoom", self.factor, Some("Zoom factor".to_string()));
        }
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let (nin, _nout, inverted, do_not_simplify, _has_forward, _has_inverse) =
            read_mapping_base(node);
        let factor = node.get_double("Zoom", 1.0);
        let mut m = ZoomMap::new(nin, factor)?;
        m.set_do_not_simplify(do_not_simplify);
        if inverted {
            m.invert();
        }
        Ok(m)
    }
}
