//! A compound frame combining two child frames.

use std::fmt;

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{CmpMap, Composition, DynMapping, PermMap, UnitMap};
use crate::serial::{FrameToNode, Node, NodeValue, Serializable, frame_from_node};

use super::{DynFrame, Frame, FrameRef, FrameSet};

/// A compound frame that concatenates the axes of two child frames.
///
/// If the first child has N1 axes and the second has N2 axes, the
/// `CmpFrame` has N1 + N2 axes. Axes 1..=N1 are served by the first
/// child and axes N1+1..=N1+N2 by the second.
///
/// Geometry methods (`distance`, `angle`, `offset`) use Euclidean
/// (Cartesian) formulae over the full compound axis space.
/// Per-axis operations (`format`, `unformat`, `norm`) are delegated
/// to the owning child frame.
pub struct CmpFrame {
    frame1: DynFrame,
    frame2: DynFrame,
    /// Axis permutation: perm[ext_axis_0based] = original_axis_0based.
    /// Initially identity.
    perm: Vec<usize>,
    /// When false, unit strings do not affect alignment. Default is true.
    act_unt: Option<bool>,
    /// Permanent object identification string (Object-level attribute).
    ident: Option<String>,
}

impl CmpFrame {
    /// Create a compound frame from two child frames.
    ///
    /// The resulting frame has `frame1.naxes() + frame2.naxes()` axes.
    pub fn new(frame1: DynFrame, frame2: DynFrame) -> Self {
        let naxes = frame1.naxes() + frame2.naxes();
        Self {
            frame1,
            frame2,
            perm: (0..naxes).collect(),
            act_unt: None,
            ident: None,
        }
    }

    /// Return the frame identification string.
    pub fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    /// Set the frame identification string. Mirrors `astSetIdent`.
    pub fn set_ident(&mut self, ident: impl Into<String>) {
        let s = ident.into();
        self.ident = if s.is_empty() { None } else { Some(s) };
    }

    /// Number of axes contributed by the first child frame.
    pub fn naxes1(&self) -> usize {
        self.frame1.naxes()
    }

    /// Access the first child frame.
    pub fn frame1(&self) -> &dyn Frame {
        self.frame1.as_ref()
    }

    /// Access the second child frame.
    pub fn frame2(&self) -> &dyn Frame {
        self.frame2.as_ref()
    }

    /// Mutable access to the first child frame — escape hatch.
    ///
    /// Mutations through this reference do not trigger any
    /// integrity recomputation on enclosing FrameSets. Pair with
    /// `FrameSet::with_integrity` when needed.
    pub fn frame1_mut(&mut self) -> &mut dyn Frame {
        self.frame1.as_mut()
    }

    /// Mutable access to the second child frame — escape hatch.
    pub fn frame2_mut(&mut self) -> &mut dyn Frame {
        self.frame2.as_mut()
    }

    /// Validate that `axis` is in 1..=naxes, returning an error if not.
    fn validate_axis(&self, axis: usize) -> AstResult<()> {
        let n = self.naxes();
        if axis < 1 || axis > n {
            return Err(AstError::new_tracked(
                ErrorCode::Axiin,
                format!("axis index {} is out of range 1..={}", axis, n),
            ));
        }
        Ok(())
    }

    /// Route a 1-based external axis to the owning child frame and its
    /// local 1-based axis index, going through the permutation.
    ///
    /// # Panics
    ///
    /// Panics if `axis` is out of range. Call [`validate_axis`] first.
    fn route_axis(&self, axis: usize) -> (&dyn Frame, usize) {
        let orig = self.perm[axis - 1] + 1; // convert to 1-based original axis
        let n1 = self.frame1.naxes();
        if orig <= n1 {
            (self.frame1.as_ref(), orig)
        } else {
            (self.frame2.as_ref(), orig - n1)
        }
    }
}

impl fmt::Debug for CmpFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CmpFrame")
            .field("naxes1", &self.frame1.naxes())
            .field("naxes2", &self.frame2.naxes())
            .finish()
    }
}

impl Frame for CmpFrame {
    fn naxes(&self) -> usize {
        self.frame1.naxes() + self.frame2.naxes()
    }

    fn ident(&self) -> &str {
        CmpFrame::ident(self)
    }

    fn system_string(&self) -> Option<&str> {
        Some("Compound")
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        self.validate_axis(axis)?;
        let (child, local_axis) = self.route_axis(axis);
        child.format(local_axis, value)
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        self.validate_axis(axis)?;
        let (child, local_axis) = self.route_axis(axis);
        child.unformat(local_axis, text)
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        if point_has_bad(a) || point_has_bad(b) {
            return Ok(BAD_VALUE);
        }
        let sum: f64 = a
            .iter()
            .zip(b.iter())
            .map(|(&ai, &bi)| (ai - bi) * (ai - bi))
            .sum();
        Ok(sum.sqrt())
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        if point_has_bad(a) || point_has_bad(b) || point_has_bad(c) {
            return Ok(BAD_VALUE);
        }
        let n = self.naxes();
        let mut dot = 0.0;
        let mut mag_ba = 0.0;
        let mut mag_bc = 0.0;
        for i in 0..n {
            let ba = a[i] - b[i];
            let bc = c[i] - b[i];
            dot += ba * bc;
            mag_ba += ba * ba;
            mag_bc += bc * bc;
        }
        let denom = (mag_ba * mag_bc).sqrt();
        if denom == 0.0 {
            return Ok(0.0);
        }
        let cos_angle = (dot / denom).clamp(-1.0, 1.0);
        Ok(cos_angle.acos())
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        let n = self.naxes();
        if point_has_bad(from) || point_has_bad(to) || is_bad(dist) {
            return Ok(vec![BAD_VALUE; n]);
        }
        let total = self.distance(from, to)?;
        if total == 0.0 || is_bad(total) {
            return Ok(from.to_vec());
        }
        let scale = dist / total;
        let result: Vec<f64> = from
            .iter()
            .zip(to.iter())
            .map(|(&f, &t)| f + scale * (t - f))
            .collect();
        Ok(result)
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        let n = self.naxes();
        let n1 = self.frame1.naxes();

        // Unpermute coordinates to original axis order.
        let mut orig = vec![0.0; n];
        for i in 0..n {
            orig[self.perm[i]] = coords[i];
        }

        // Normalize via children in original order.
        self.frame1.norm(&mut orig[..n1])?;
        self.frame2.norm(&mut orig[n1..])?;

        // Re-permute back to external order.
        for i in 0..n {
            coords[i] = orig[self.perm[i]];
        }
        Ok(())
    }

    fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        let n = self.naxes();
        super::validate_permutation(perm, n)?;
        let old = self.perm.clone();
        for i in 0..n {
            self.perm[i] = old[perm[i] - 1];
        }
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        let mut cmp = CmpFrame::new(self.frame1.snapshot(), self.frame2.snapshot());
        cmp.perm = self.perm.clone();
        Box::new(cmp)
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        let prev = match previous.as_any().downcast_ref::<CmpFrame>() {
            Some(p) => p,
            None => return Ok(Box::new(UnitMap::new(self.naxes()))),
        };

        let n1 = self.frame1.naxes();
        let n2 = self.frame2.naxes();
        if prev.frame1.naxes() != n1 || prev.frame2.naxes() != n2 {
            return Ok(Box::new(UnitMap::new(self.naxes())));
        }

        // Per-child correction mappings. Each is computed against the
        // matching child of the snapshot so a System/Equinox/Epoch
        // change on a primary child propagates as the right
        // coordinate-conversion mapping.
        let delta1 = self.frame1.map_from(prev.frame1.as_ref())?;
        let delta2 = self.frame2.map_from(prev.frame2.as_ref())?;

        // Combine in parallel — order: frame1 axes first, then
        // frame2 axes (the internal axis layout, before `perm`).
        let parallel: DynMapping = Box::new(CmpMap::new(delta1, delta2, Composition::Parallel)?);

        // If both perm vectors are identical to the internal layout
        // (the common case — perm preserves internal ordering), the
        // parallel mapping is the answer directly.
        let identity_perm: Vec<usize> = (0..self.naxes()).collect();
        if self.perm == identity_perm && prev.perm == identity_perm {
            return Ok(parallel);
        }

        // General case: sandwich the parallel mapping between two
        // PermMaps that move external→internal on input and
        // internal→external on output. Both perms must be identical
        // for this to be lossless; if they differ, fall back to
        // identity (a System/Epoch change should not also reorder
        // axes — that would have come through `perm_axes` separately).
        if self.perm != prev.perm {
            return Ok(Box::new(UnitMap::new(self.naxes())));
        }

        // PermMap encoding (1-based): external axis k holds
        // self.perm[k]; internal axis i holds the inverse.
        let n = self.naxes();
        let mut inv = vec![0usize; n];
        for (k, &orig) in self.perm.iter().enumerate() {
            inv[orig] = k;
        }
        // ext→int: internal[i] = external[inv[i]]   → outperm[i] = inv[i]+1, inperm[k] = perm[k]+1
        let outperm_e2i: Vec<i64> = (0..n).map(|i| inv[i] as i64 + 1).collect();
        let inperm_e2i: Vec<i64> = (0..n).map(|k| self.perm[k] as i64 + 1).collect();
        let pre = PermMap::new(&inperm_e2i, &outperm_e2i, &[])?;

        // int→ext: external[k] = internal[perm[k]] → outperm[k] = perm[k]+1, inperm[i] = inv[i]+1
        let outperm_i2e: Vec<i64> = (0..n).map(|k| self.perm[k] as i64 + 1).collect();
        let inperm_i2e: Vec<i64> = (0..n).map(|i| inv[i] as i64 + 1).collect();
        let post = PermMap::new(&inperm_i2e, &outperm_i2e, &[])?;

        let m1 = CmpMap::new(Box::new(pre), parallel, Composition::Series)?;
        let full = CmpMap::new(Box::new(m1), Box::new(post), Composition::Series)?;
        Ok(Box::new(full))
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        let Some(other) = other.as_any().downcast_ref::<CmpFrame>() else {
            return false;
        };
        self.frame1.equal(other.frame1.as_ref()) && self.frame2.equal(other.frame2.as_ref())
    }

    fn convert(&self, to: &dyn Frame, _domains: &[&str]) -> AstResult<Option<FrameSet>> {
        if self.naxes() == to.naxes() {
            let mut fs = FrameSet::new(self.snapshot());
            fs.add_frame(
                FrameRef::Base,
                Box::new(UnitMap::new(self.naxes())),
                to.snapshot(),
            )?;
            Ok(Some(fs))
        } else {
            Ok(None)
        }
    }
}

impl Serializable for CmpFrame {
    fn class_name(&self) -> &'static str {
        "CmpFrame"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Compound coordinate system description".to_string());

        // --- Object-level attributes (before IsA Object) ---
        if let Some(ref ident) = self.ident {
            node.push_string(
                "Ident",
                ident,
                Some("Permanent Object identification string".to_string()),
            );
        }
        if self.ident.is_some() {
            node.push_isa("Object", Some("AST Object".to_string()));
        }

        let naxes = self.naxes();
        let n1 = self.frame1.naxes();

        // Serialize child frames to get their Lbl/Uni/Dir info.
        let node_a = FrameToNode::to_node(self.frame1.as_ref())?;
        let node_b = FrameToNode::to_node(self.frame2.as_ref())?;

        // Compute default title: "N-d compound coordinate system"
        let default_title = format!("{naxes}-d compound coordinate system");
        node.push_unset(
            "Title",
            NodeValue::String(default_title),
            Some("Title of coordinate system".to_string()),
        );

        // Naxes (commented for CmpFrame — derivable from components)
        node.push_unset(
            "Naxes",
            NodeValue::Int(naxes as i64),
            Some("Number of coordinate axes".to_string()),
        );

        // Domain default mirrors C's `CmpFrame::GetDomain` (cmpframe.c
        // line 3603+): if either child has a non-empty Domain, return
        // "dom1-dom2"; if both are empty, return the literal "CMP".
        let domain_a = node_a.get_string_any("Domain").unwrap_or("").to_string();
        let domain_b = node_b.get_string_any("Domain").unwrap_or("").to_string();
        let compound_domain = if domain_a.is_empty() && domain_b.is_empty() {
            "CMP".to_string()
        } else {
            format!("{}-{}", domain_a, domain_b)
        };
        node.push_unset(
            "Domain",
            NodeValue::String(compound_domain),
            Some("Coordinate system domain".to_string()),
        );

        // Lbl{n} and Uni{n} from child frames
        for ax in 1..=naxes {
            let (child_node, local_ax) = if ax <= n1 {
                (&node_a, ax)
            } else {
                (&node_b, ax - n1)
            };
            let lbl_key = format!("Lbl{local_ax}");
            if let Some(lbl) = child_node.get_string_any(&lbl_key) {
                // Renumber generic "Axis N" labels to compound axis position.
                let lbl_str = if lbl == format!("Axis {local_ax}") && local_ax != ax {
                    format!("Axis {ax}")
                } else {
                    lbl.to_string()
                };
                node.push_unset(
                    &format!("Lbl{ax}"),
                    NodeValue::String(lbl_str),
                    Some(format!("Label for axis {ax}")),
                );
            }
        }
        for ax in 1..=naxes {
            let (child_node, local_ax) = if ax <= n1 {
                (&node_a, ax)
            } else {
                (&node_b, ax - n1)
            };
            let uni_key = format!("Uni{local_ax}");
            if let Some(uni) = child_node.get_string_any(&uni_key) {
                node.push_unset(
                    &format!("Uni{ax}"),
                    NodeValue::String(uni.to_string()),
                    Some(format!("Units for axis {ax}")),
                );
            }
        }

        // Dir{n} — only write if value is 0 (reverse direction)
        for ax in 1..=naxes {
            let (child_node, local_ax) = if ax <= n1 {
                (&node_a, ax)
            } else {
                (&node_b, ax - n1)
            };
            let dir_key = format!("Dir{local_ax}");
            if let Some(0) = child_node.get_int_any(&dir_key) {
                node.push_unset(
                    &format!("Dir{ax}"),
                    NodeValue::Int(0),
                    Some(format!("Plot axis {ax} in reverse direction")),
                );
            }
        }

        // ActUnt: active if explicitly set to false.
        if let Some(false) = self.act_unt {
            node.push_int(
                "ActUnt",
                0,
                Some("Unit strings do not affect alignment".to_string()),
            );
        }

        // IsA Frame marker
        node.push_isa("Frame", Some("Coordinate system description".to_string()));

        // Component frames as nested objects (strip unset attributes for minimal output).
        let mut node_a = node_a;
        let mut node_b = node_b;
        node_a.strip_unset();
        node_b.strip_unset();
        node.push_object("FrameA", node_a, Some("First component Frame".to_string()));
        node.push_object("FrameB", node_b, Some("Second component Frame".to_string()));

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let node_a = node
            .get_object("FrameA")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "CmpFrame: missing FrameA"))?;
        let node_b = node
            .get_object("FrameB")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "CmpFrame: missing FrameB"))?;

        let frame1 = frame_from_node(node_a)?;
        let frame2 = frame_from_node(node_b)?;

        let mut cmp = CmpFrame::new(frame1, frame2);

        // ActUnt (default is 1 = true; only store when 0).
        let act_unt_val = node.get_int("ActUnt", 1);
        if act_unt_val == 0 {
            cmp.act_unt = Some(false);
        }

        // Ident (Object-level identification string).
        let ident = node.get_string("Ident", "");
        if !ident.is_empty() {
            cmp.ident = Some(ident);
        }

        Ok(cmp)
    }
}
