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
    // Match-control attributes (used by `astFindFrame` / `astMatch`).
    min_axes_opt: Option<usize>,
    max_axes_opt: Option<usize>,
    match_end_opt: Option<bool>,
    preserve_axes_opt: Option<bool>,
    permute_opt: Option<bool>,
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
            min_axes_opt: None,
            max_axes_opt: None,
            match_end_opt: None,
            preserve_axes_opt: None,
            permute_opt: None,
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

    /// External-to-internal axis permutation. `perm[ext]` returns the
    /// zero-based original (internal, pre-`perm_axes`) axis index that
    /// external axis `ext` references. Mirrors C `astGetPerm`.
    pub fn perm(&self) -> &[usize] {
        &self.perm
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

    // Match-control attributes — mirror C `astGet/SetMinAxes` etc.
    // CmpFrame's MinAxes / MaxAxes default in C are derived from
    // the components' MinAxes / MaxAxes (sum for max, max for min);
    // here we use naxes() as the default if not explicitly set,
    // matching the simpler convention sufficient for `astFindFrame`'s
    // use of these attributes from the *template* side.
    fn min_axes(&self) -> usize {
        self.min_axes_opt.unwrap_or_else(|| self.naxes())
    }
    fn set_min_axes(&mut self, n: usize) {
        self.min_axes_opt = Some(n);
    }
    fn test_min_axes(&self) -> bool {
        self.min_axes_opt.is_some()
    }
    fn max_axes(&self) -> usize {
        self.max_axes_opt.unwrap_or_else(|| self.naxes())
    }
    fn set_max_axes(&mut self, n: usize) {
        self.max_axes_opt = Some(n);
    }
    fn test_max_axes(&self) -> bool {
        self.max_axes_opt.is_some()
    }
    fn match_end(&self) -> bool {
        self.match_end_opt.unwrap_or(false)
    }
    fn set_match_end(&mut self, v: bool) {
        self.match_end_opt = Some(v);
    }
    fn test_match_end(&self) -> bool {
        self.match_end_opt.is_some()
    }
    fn preserve_axes(&self) -> bool {
        self.preserve_axes_opt.unwrap_or(false)
    }
    fn set_preserve_axes(&mut self, v: bool) {
        self.preserve_axes_opt = Some(v);
    }
    fn test_preserve_axes(&self) -> bool {
        self.preserve_axes_opt.is_some()
    }
    fn permute(&self) -> bool {
        self.permute_opt.unwrap_or(true)
    }
    fn set_permute(&mut self, v: bool) {
        self.permute_opt = Some(v);
    }
    fn test_permute(&self) -> bool {
        self.permute_opt.is_some()
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

        // Determine each child's axis count to split the compound axis
        // attributes (Lbl{n}, Uni{n}, Sym{n}, Fmt{n}, Dir{n}). C's
        // CmpFrame::SetAttrib forwards an indexed attribute to the
        // primary Frame containing that axis (cmpframe.c:8396+);
        // we materialise the same effect by injecting per-axis
        // overrides into the child node before parsing.
        let n1 = node_a.get_int("Naxes", 0) as usize;
        let n2 = node_b.get_int("Naxes", 0) as usize;
        let mut node_a = node_a.clone();
        let mut node_b = node_b.clone();
        for prefix in ["Lbl", "Uni", "Sym", "Fmt", "Dir"] {
            for ax in 1..=(n1 + n2) {
                let cmp_key = format!("{prefix}{ax}");
                // Forward both set and commented (default) values: the
                // CmpFrame's `to_node` emits these as `push_unset`, but
                // when the underlying child has no representation of
                // this attribute (e.g. a SpecFrame inside a DSBSpecFrame
                // that lacks a Unit field), the only place we can find
                // its value on read is on the CmpFrame itself.
                let Some(value) = node.get_any(&cmp_key).cloned() else {
                    continue;
                };
                let (child, local_ax) = if ax <= n1 {
                    (&mut node_a, ax)
                } else {
                    (&mut node_b, ax - n1)
                };
                let child_key = format!("{prefix}{local_ax}");
                child.replace_or_push(&child_key, value);
            }
        }

        let frame1 = frame_from_node(&node_a)?;
        let frame2 = frame_from_node(&node_b)?;

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

/// Generate the next axis selection in lexicographic order.
///
/// Mirrors C `GenAxisSelection` (cmpframe.c:204). On entry, `axes`
/// holds `nselect` distinct zero-based axis indices in
/// strictly-increasing order; on exit the indices are advanced to the
/// lexicographically-next selection of the same size from `0..naxes`.
///
/// Initialise with `axes = [0, 1, …, nselect-1]`. Repeated invocation
/// then enumerates every `nselect`-choose-`naxes` subset.
///
/// Returns `true` if the array now holds a valid successor selection,
/// `false` once every selection has been emitted (in which case the
/// array contents are exhausted and must not be used).
#[allow(dead_code)] // wired in by CmpFrame::Match port (subsequent commit).
pub(crate) fn gen_axis_selection(naxes: usize, nselect: usize, axes: &mut [i32]) -> bool {
    // C returns 0 when `nselect == 0` regardless of the loop body —
    // its final check is `nselect > 0 && axes[nselect-1] < naxes`. We
    // short-circuit here both for clarity and to avoid an out-of-range
    // read of `axes[nselect+1]` in the empty case.
    if nselect == 0 {
        return false;
    }
    debug_assert_eq!(axes.len(), nselect);

    let mut iselect: usize = 0;
    loop {
        // Increment the current axis index if it is the final one, or
        // it can be incremented without equalling the one which follows
        // (this ensures the indices remain in increasing order).
        let can_increment = iselect == nselect - 1 || axes[iselect + 1] > axes[iselect] + 1;
        if can_increment {
            axes[iselect] += 1;
            // After incrementing an index, reset all previous indices
            // to their starting values 0, 1, …, iselect-1.
            for (i, slot) in axes.iter_mut().enumerate().take(iselect) {
                *slot = i as i32;
            }
            break;
        }
        // If this axis index can't be incremented, consider the next
        // one. Quit if we go beyond the end of the selection array.
        iselect += 1;
        if iselect >= nselect {
            break;
        }
    }

    axes[nselect - 1] < naxes as i32
}

/// Replace `-1` slots in `axes[i1..=i2]` with new indices that locate
/// the missing axes relative to neighbouring valid ones. Mirrors C
/// `AddExtraAxes` (cmpframe.c:929).
///
/// `following` chooses the side: `false` associates each new index
/// with the *preceding* valid index (numbered one greater); `true`
/// associates it with the *following* valid index (numbered one less,
/// shifting subsequent valid indices upward).
///
/// All entries of `axes` are candidates for renumbering even outside
/// `i1..=i2`, since duplicates anywhere must be eliminated. Some
/// indices may end up unused.
#[allow(dead_code)] // wired in by part_match below.
pub(crate) fn add_extra_axes(axes: &mut [i32], i1: usize, i2: usize, following: bool) {
    let naxes = axes.len();
    if naxes == 0 || i1 > i2 || i2 >= naxes {
        return;
    }

    // Establish a default "extra" index by scanning in the direction
    // dictated by `following`, looking for the first valid axis index
    // (anywhere in the full array, not just i1..=i2 — see C's loop
    // bounds at cmpframe.c:1020-1029).
    let mut extra: i32 = 0;
    let mut found = false;
    let scan = |from_back: bool| -> Option<(usize, i32)> {
        if from_back {
            for i in (0..naxes).rev() {
                if axes[i] != -1 {
                    return Some((i, axes[i]));
                }
            }
        } else {
            for (i, &v) in axes.iter().enumerate() {
                if v != -1 {
                    return Some((i, v));
                }
            }
        }
        None
    };

    // First scan in the natural direction.
    if let Some((_, v)) = scan(following) {
        extra = v + if following { 1 } else { 0 };
        found = true;
    }
    // Fallback scan in the opposite direction (handles arrays that
    // start with -1's in the natural direction).
    if !found {
        if let Some((_, v)) = scan(!following) {
            extra = v + if !following { 1 } else { 0 };
        }
        // If still nothing was found, `extra` stays at 0 (an array
        // entirely filled with -1).
    }

    // Now scan only the slice [i1..=i2] in the natural direction.
    let indices: Box<dyn Iterator<Item = usize>> = if following {
        Box::new((i1..=i2).rev())
    } else {
        Box::new(i1..=i2)
    };

    for i in indices {
        if axes[i] == -1 {
            // Bump every valid index >= extra to dodge duplicates.
            for v in axes.iter_mut() {
                if *v != -1 && *v >= extra {
                    *v += 1;
                }
            }
            axes[i] = extra;
            // Step extra: forward direction → +1 so subsequent extras
            // come after; backward direction → 0 so subsequent extras
            // shift the previously-assigned one upward.
            if !following {
                extra += 1;
            }
        } else {
            // Reset extra relative to the just-seen valid index.
            extra = axes[i] + if following { 0 } else { 1 };
        }
    }
}

/// Replace each entry of `axes` by its rank when the values are
/// sorted in ascending order. Eliminates "missing" indices while
/// preserving relative order. Mirrors C `RenumberAxes`
/// (cmpframe.c:7651).
#[allow(dead_code)] // wired in by part_match below.
pub(crate) fn renumber_axes(axes: &mut [i32]) {
    let naxes = axes.len();
    if naxes == 0 {
        return;
    }
    // Compute the rank of each entry by sort order.
    let mut order: Vec<usize> = (0..naxes).collect();
    order.sort_by_key(|&i| axes[i]);
    // After this pass, the i-th entry in sort order is at axes[order[i]];
    // its new value should be i.
    let mut new_values = vec![0i32; naxes];
    for (rank, &orig_idx) in order.iter().enumerate() {
        new_values[orig_idx] = rank as i32;
    }
    axes.copy_from_slice(&new_values);
}

/// Return `true` if a `PermMap(inperm, outperm)` would have a
/// non-trivial effect (i.e. it differs from a [`UnitMap`] of the
/// same size). Mirrors C `GoodPerm` (cmpframe.c:4562).
#[allow(dead_code)] // wired in by part_match below.
pub(crate) fn good_perm(inperm: &[i64], outperm: &[i64]) -> bool {
    if inperm.len() != outperm.len() {
        return true;
    }
    for (axis, &v) in inperm.iter().enumerate() {
        if v != (axis + 1) as i64 {
            return true;
        }
    }
    for (axis, &v) in outperm.iter().enumerate() {
        if v != (axis + 1) as i64 {
            return true;
        }
    }
    false
}

/// Match a CmpFrame template against a target Frame using a specific
/// partitioning of the target axes between the two template
/// components. Mirrors C `PartMatch` (cmpframe.c:6591).
///
/// The caller must supply `axes1` and `axes2`, two disjoint lists of
/// (zero-based) target axis indices that together cover every target
/// axis. `axes1` is matched against `template.frame1`; `axes2` is
/// matched against `template.frame2`. The function tries the match
/// and, if both child matches succeed, builds the full result Frame +
/// Mapping by parallel composition plus prefix / suffix PermMaps to
/// align target and result axis ordering.
///
/// Per the C contract, the result Frame's axis count is governed by
/// the template's `PreserveAxes` attribute: when set, it equals the
/// target's axis count; otherwise it equals the template's.
/// `MatchEnd` controls which subset is selected when those counts
/// differ. Both attributes are inherited by the children when they
/// have not been explicitly set on those children individually
/// (cmpframe.c:233-294); the prior values are restored before
/// returning so that the template's child attributes are not
/// permanently modified.
#[allow(dead_code)] // wired in by CmpFrame::Match port.
pub(crate) fn part_match(
    template: &CmpFrame,
    target: &dyn Frame,
    matchsub: bool,
    axes1: &[i32],
    axes2: &[i32],
) -> AstResult<Option<crate::frames::find_frame::MatchResult>> {
    use crate::frames::find_frame::MatchResult;
    use crate::mappings::{CmpMap, Composition, PermMap};

    // Step 1: select the two sub-Frames from the target via SubFrame.
    // We pass `template = None` to obtain a snapshot without overlaying
    // attributes (matches `astSubFrame(target, NULL, ...)` in C). The
    // returned Mapping is discarded.
    let frame1 = match target.sub_frame(None, axes1.len(), axes1, &[])? {
        Some((_map, f)) => f,
        None => return Ok(None),
    };
    let frame2 = match target.sub_frame(None, axes2.len(), axes2, &[])? {
        Some((_map, f)) => f,
        None => return Ok(None),
    };

    // Step 2: temporarily push the template's component attributes
    // before calling each child match. C does this by mutating the
    // template's children in-place and restoring them afterwards; we
    // achieve the same effect by snapshotting the children and
    // mutating the snapshot.
    let prepare_child = |child: &dyn Frame| -> DynFrame {
        let mut snap = child.snapshot();
        if !child.test_match_end() {
            snap.set_match_end(template.match_end());
        }
        if !child.test_preserve_axes() {
            snap.set_preserve_axes(template.preserve_axes());
        }
        // Always force Permute=1 on the snapshot so the child Match
        // considers axis permutations; the CmpFrame-level Permute is
        // applied in the parent CmpFrame::Match (cmpframe.c:5343).
        snap.set_permute(true);
        snap
    };

    let frame1_template = prepare_child(template.frame1());
    let frame2_template = prepare_child(template.frame2());

    let m1 = frame1_template.match_template(frame1.as_ref(), matchsub)?;
    let m2 = frame2_template.match_template(frame2.as_ref(), matchsub)?;
    let (Some(m1), Some(m2)) = (m1, m2) else {
        return Ok(None);
    };

    // Step 3: lay out the result Frame.
    let target_naxes = target.naxes();
    let result_naxes1 = m1.result.naxes();
    let result_naxes2 = m2.result.naxes();
    let result_naxes = result_naxes1 + result_naxes2;
    let template_naxes1 = template.frame1().naxes();
    let template_naxes = template.naxes();

    let match_end1 = frame1_template.match_end();
    let match_end2 = frame2_template.match_end();
    let match_end = template.match_end();

    // Inverse of the template's external→internal permutation: maps
    // internal axis index back to external (cmpframe.c:323-329).
    let perm = template.perm();
    let mut invperm = vec![0usize; template_naxes];
    for (template_axis, &p) in perm.iter().enumerate() {
        invperm[p] = template_axis;
    }

    // Step 4: per-result-axis association arrays (cmpframe.c:339-405).
    let mut template_assoc = vec![0i32; result_naxes];
    let mut target_assoc = vec![0i32; result_naxes];
    for result_axis in 0..result_naxes {
        let first = result_axis < result_naxes1;
        let part_result_axis = if first {
            result_axis
        } else {
            result_axis - result_naxes1
        };
        let part_template_axis = if first {
            m1.template_axes[part_result_axis]
        } else {
            m2.template_axes[part_result_axis]
        };
        let part_target_axis = if first {
            m1.target_axes[part_result_axis]
        } else {
            m2.target_axes[part_result_axis]
        };

        template_assoc[result_axis] = if part_template_axis == -1 {
            -1
        } else {
            let internal = if first {
                part_template_axis as usize
            } else {
                part_template_axis as usize + template_naxes1
            };
            invperm[internal] as i32
        };

        target_assoc[result_axis] = if part_target_axis == -1 {
            -1
        } else if first {
            axes1[part_target_axis as usize]
        } else {
            axes2[part_target_axis as usize]
        };
    }

    // Step 5: build the basic result CmpFrame from the two child
    // results in parallel (cmpframe.c:419-420).
    let mut result_frame: DynFrame = Box::new(CmpFrame::new(m1.result, m2.result));

    // Step 6: permute the result axes to match the reference Frame
    // (template if PreserveAxes==0, target if PreserveAxes==1).
    let preserve_axes = template.preserve_axes();
    let ref_naxes = if preserve_axes {
        target_naxes
    } else {
        template_naxes
    };

    let mut result_order = if preserve_axes {
        target_assoc.clone()
    } else {
        template_assoc.clone()
    };

    // Replace -1 entries with synthesised positions (cmpframe.c:458-461).
    add_extra_axes(
        &mut result_order,
        0,
        result_naxes1.saturating_sub(1),
        match_end1,
    );
    add_extra_axes(
        &mut result_order,
        result_naxes1,
        result_naxes.saturating_sub(1),
        match_end2,
    );

    // Compress out gaps (cmpframe.c:467).
    renumber_axes(&mut result_order);

    // Build the inverse permutation (cmpframe.c:474-480).
    let mut result_perm = vec![0usize; result_naxes];
    for (axis, &order) in result_order.iter().enumerate() {
        result_perm[order as usize] = axis;
    }

    // Apply (1-based perm_axes interface).
    let perm_args: Vec<usize> = result_perm.iter().map(|&p| p + 1).collect();
    result_frame.perm_axes(&perm_args)?;

    // Step 7: when the reference axis count differs, pick a
    // sub-/super-set (cmpframe.c:486-525).
    if ref_naxes != result_naxes {
        let mut pick = vec![-1i32; ref_naxes];
        for (output_axis, slot) in pick.iter_mut().enumerate() {
            let full_axis: i32 = if match_end {
                (output_axis as i32) + (result_naxes as i32 - ref_naxes as i32)
            } else {
                output_axis as i32
            };
            if (0..result_naxes as i32).contains(&full_axis) {
                *slot = full_axis;
            }
        }
        // Pick the requested axes from result_frame.
        result_frame = pick_result_axes(result_frame.as_ref(), &pick)?;
    }

    // Step 8: build the final association arrays for the picked /
    // permuted axes (cmpframe.c:533-565).
    let mut out_template_axes = vec![0i32; ref_naxes];
    let mut out_target_axes = vec![0i32; ref_naxes];
    for output_axis in 0..ref_naxes {
        let full_axis: i32 = if match_end {
            (output_axis as i32) + (result_naxes as i32 - ref_naxes as i32)
        } else {
            output_axis as i32
        };
        if (0..result_naxes as i32).contains(&full_axis) {
            let result_axis = result_perm[full_axis as usize];
            out_template_axes[output_axis] = template_assoc[result_axis];
            out_target_axes[output_axis] = target_assoc[result_axis];
        } else {
            out_template_axes[output_axis] = -1;
            out_target_axes[output_axis] = -1;
        }
    }

    // Step 9: build the output Mapping (cmpframe.c:572-687).
    // Inner Mapping: parallel composition of the two child Maps, with
    // input order (axes1 ‖ axes2) and output order (m1.result ‖ m2.result).
    let inner_map: DynMapping = Box::new(CmpMap::new(m1.map, m2.map, Composition::Parallel)?);

    // Prefix PermMap: align the target's natural axis order with the
    // (axes1 ‖ axes2) order expected by inner_map.
    let mut inperm = vec![0i64; target_naxes];
    let mut outperm = vec![0i64; target_naxes];
    #[allow(clippy::needless_range_loop)]
    for target_axis in 0..target_naxes {
        let first = target_axis < axes1.len();
        let part_target_axis = if first {
            target_axis
        } else {
            target_axis - axes1.len()
        };
        let original = if first {
            axes1[part_target_axis]
        } else {
            axes2[part_target_axis]
        };
        // outperm[target_axis] = original (1-based)
        outperm[target_axis] = (original as i64) + 1;
        // inperm[original] = target_axis + 1 (1-based)
        inperm[original as usize] = (target_axis as i64) + 1;
    }

    let mut current_map: DynMapping = inner_map;
    if good_perm(&inperm, &outperm) {
        let prefix = PermMap::new(&inperm, &outperm, &[])?;
        current_map = Box::new(CmpMap::new(
            Box::new(prefix),
            current_map,
            Composition::Series,
        )?);
    }

    // Suffix PermMap: align inner output (result_naxes axes in the
    // pre-perm_axes order) with the requested ref_naxes order.
    let mut inperm2 = vec![0i64; result_naxes];
    let mut outperm2 = vec![0i64; ref_naxes];
    #[allow(clippy::needless_range_loop)]
    for output_axis in 0..ref_naxes {
        let full_axis: i32 = if match_end {
            (output_axis as i32) + (result_naxes as i32 - ref_naxes as i32)
        } else {
            output_axis as i32
        };
        if (0..result_naxes as i32).contains(&full_axis) {
            let result_axis = result_perm[full_axis as usize];
            outperm2[output_axis] = (result_axis as i64) + 1;
            inperm2[result_axis] = (output_axis as i64) + 1;
        }
        // else outperm2[output_axis] stays 0 = "BAD" sentinel.
    }
    // C tests good_perm with `target_naxes` for both lengths
    // (cmpframe.c:672) — that's a deliberate choice to skip the
    // suffix when both lists are shorter than target_naxes (which
    // suggests the suffix would amount to a near-identity). We
    // reproduce it bit-for-bit: the function returns true iff the
    // permutation differs from identity within the shared prefix.
    let differs = inperm2
        .iter()
        .enumerate()
        .any(|(axis, &v)| axis < target_naxes && v != (axis + 1) as i64)
        || outperm2
            .iter()
            .enumerate()
            .any(|(axis, &v)| axis < target_naxes && v != (axis + 1) as i64)
        || inperm2.len() != target_naxes
        || outperm2.len() != target_naxes;
    if differs {
        let suffix = PermMap::new(&inperm2, &outperm2, &[])?;
        current_map = Box::new(CmpMap::new(
            current_map,
            Box::new(suffix),
            Composition::Series,
        )?);
    }

    Ok(Some(MatchResult {
        template_axes: out_template_axes,
        target_axes: out_target_axes,
        map: current_map,
        result: result_frame,
    }))
}

/// Pick `picks.len()` axes (1-based after `+1`) from `frame`, with
/// `-1` entries producing default axes. Mirrors `astPickAxes` in the
/// PartMatch flow at cmpframe.c:519. Used only for the result-frame
/// shrink/expand step; we delegate to [`pick_axes`] for the natural
/// case and synthesise a wider Frame manually when `picks` selects
/// extra unused axes.
fn pick_result_axes(frame: &dyn Frame, picks: &[i32]) -> AstResult<DynFrame> {
    // The current Rust `pick_axes` helper only handles the strict
    // pick path (no -1 / new-axis synthesis). For PartMatch we may
    // see -1 entries when `ref_naxes > result_naxes` — i.e. when the
    // template asks for more axes than the matching produced. Use
    // pick_axes_snapshot which already accepts -1 entries (frame.c
    // PickAxes default impl).
    frame.pick_axes_snapshot(picks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::find_frame::apply_template_overlay;
    use crate::frames::specframe::SpecSystem;
    use crate::frames::{BasicFrameBuilder, SkyFrame, SkySystem, SpecFrame};

    /// Enumerate every selection produced by repeated calls to
    /// `gen_axis_selection`, starting from the canonical seed
    /// `[0, 1, …, k-1]`. Returns the list of selections, each as a
    /// fresh Vec.
    fn enumerate_selections(naxes: usize, nselect: usize) -> Vec<Vec<i32>> {
        let mut axes: Vec<i32> = (0..nselect as i32).collect();
        let mut out = vec![axes.clone()];
        while gen_axis_selection(naxes, nselect, &mut axes) {
            out.push(axes.clone());
        }
        out
    }

    #[test]
    fn gen_axis_selection_choose_2_of_4() {
        // 4 choose 2 = 6, in lexicographic order.
        let got = enumerate_selections(4, 2);
        assert_eq!(
            got,
            vec![
                vec![0, 1],
                vec![0, 2],
                vec![1, 2],
                vec![0, 3],
                vec![1, 3],
                vec![2, 3],
            ]
        );
    }

    #[test]
    fn gen_axis_selection_choose_3_of_5() {
        // 5 choose 3 = 10.
        let got = enumerate_selections(5, 3);
        assert_eq!(got.len(), 10);
        assert_eq!(got[0], vec![0, 1, 2]);
        assert_eq!(got[9], vec![2, 3, 4]);
        // Every entry is strictly increasing and within range.
        for sel in &got {
            assert!(sel.windows(2).all(|w| w[0] < w[1]));
            for &a in sel {
                assert!((0..5).contains(&a));
            }
        }
    }

    #[test]
    fn gen_axis_selection_choose_1_of_3() {
        // 3 choose 1 = 3 (each single element on its own).
        assert_eq!(enumerate_selections(3, 1), vec![vec![0], vec![1], vec![2]]);
    }

    #[test]
    fn gen_axis_selection_choose_n_of_n() {
        // The single full-set selection.
        let got = enumerate_selections(3, 3);
        assert_eq!(got, vec![vec![0, 1, 2]]);
    }

    #[test]
    fn gen_axis_selection_zero_select_returns_false_immediately() {
        let mut axes: Vec<i32> = vec![];
        assert!(!gen_axis_selection(5, 0, &mut axes));
    }

    /// Replace -1 slots with new indices preceding the surrounding
    /// valid entries (`following=false`).
    #[test]
    fn add_extra_axes_following_false() {
        // Start: [0, -1, 2]. Default (following=false) places the new
        // index *after* the preceding valid one (0), bumping all
        // valid indices >= 1 upward to make room.
        let mut axes = vec![0i32, -1, 2];
        add_extra_axes(&mut axes, 0, 2, false);
        // Slot 1 takes the slot just after the preceding valid (0)
        // → extra=1; existing valid 2 gets bumped to 3.
        assert_eq!(axes, vec![0, 1, 3]);
    }

    /// Same input, `following=true`: new index displaces the
    /// following valid index upward.
    #[test]
    fn add_extra_axes_following_true() {
        let mut axes = vec![0i32, -1, 2];
        add_extra_axes(&mut axes, 0, 2, true);
        // Following the next valid (2): extra = 2; existing valid 2
        // gets bumped to 3, the slot takes 2.
        assert_eq!(axes, vec![0, 2, 3]);
    }

    #[test]
    fn renumber_axes_compresses_gaps() {
        // [3, 7, 1] → ranks [1, 2, 0].
        let mut axes = vec![3i32, 7, 1];
        renumber_axes(&mut axes);
        assert_eq!(axes, vec![1, 2, 0]);
    }

    #[test]
    fn good_perm_identity_returns_false() {
        assert!(!good_perm(&[1, 2, 3], &[1, 2, 3]));
    }

    #[test]
    fn good_perm_swap_returns_true() {
        assert!(good_perm(&[2, 1], &[2, 1]));
    }

    #[test]
    fn good_perm_size_mismatch_returns_true() {
        assert!(good_perm(&[1, 2], &[1, 2, 3]));
    }

    /// Smoke test for `part_match` using BasicFrame components, which
    /// have working `match_template` / `sub_frame` via the trait
    /// default. Template = CmpFrame(BasicFrame(2), BasicFrame(1)),
    /// Target = BasicFrame(3). Partitioning axes1=[0,1], axes2=[2]
    /// matches both children and produces a 3-axis result.
    #[test]
    fn part_match_basicframe_partition_succeeds() {
        let template = CmpFrame::new(
            Box::new(BasicFrameBuilder::new(2).build().unwrap()),
            Box::new(BasicFrameBuilder::new(1).build().unwrap()),
        );
        let target = BasicFrameBuilder::new(3).build().unwrap();
        let m = part_match(&template, &target, false, &[0, 1], &[2])
            .unwrap()
            .expect("3-axis partition match");
        assert_eq!(m.result.naxes(), 3);
        assert_eq!(m.template_axes.len(), 3);
        assert_eq!(m.target_axes.len(), 3);
    }

    /// When the partitioning gives a child too few axes
    /// (axes1=[0] vs a 2-axis template.frame1), part_match returns
    /// None because the inner `BasicFrame::Match` rejects.
    #[test]
    fn part_match_returns_none_on_axis_count_mismatch() {
        let template = CmpFrame::new(
            Box::new(BasicFrameBuilder::new(2).build().unwrap()),
            Box::new(BasicFrameBuilder::new(1).build().unwrap()),
        );
        let target = BasicFrameBuilder::new(3).build().unwrap();
        let result = part_match(&template, &target, false, &[0], &[1, 2]).unwrap();
        assert!(result.is_none());
    }

    // Tests exercising part_match with CmpFrame *targets* land
    // alongside CmpFrame::sub_frame (Task 6). Until then,
    // target.sub_frame() on a CmpFrame target returns None (the
    // trait default), so part_match() against a CmpFrame target
    // unconditionally returns None.

    /// Clean-partition path (cmpframe.c:262-380): template
    /// `CmpFrame(SkyFrame(Galactic), SpecFrame)` overlaid onto a result
    /// `CmpFrame(SkyFrame(FK5), SpecFrame)` with identity axis
    /// associations should propagate the template's explicitly-set
    /// system-driven attributes onto the matching result component.
    #[test]
    fn overlay_template_clean_partition_copies_skyframe_system() {
        let mut sky_template = SkyFrame::new(SkySystem::Galactic);
        sky_template.set_title("custom-galactic");
        let template = CmpFrame::new(
            Box::new(sky_template),
            Box::new(SpecFrame::new(SpecSystem::Freq)),
        );

        let result_sky = SkyFrame::new(SkySystem::Fk5);
        let mut result: DynFrame = Box::new(CmpFrame::new(
            Box::new(result_sky),
            Box::new(SpecFrame::new(SpecSystem::Freq)),
        ));

        // Identity association: result axis i ↔ template axis i.
        let template_axes: Vec<i32> = (0..result.naxes() as i32).collect();
        apply_template_overlay(&template, &template_axes, result.as_mut()).unwrap();

        let result_cmp = result.as_any().downcast_ref::<CmpFrame>().unwrap();
        let result_sky = result_cmp
            .frame1()
            .as_any()
            .downcast_ref::<SkyFrame>()
            .unwrap();
        assert_eq!(result_sky.system(), SkySystem::Galactic);
        assert_eq!(result_sky.title(), "custom-galactic");
    }

    /// Fall-back path (cmpframe.c:382-427): template is a CmpFrame but
    /// the result is *not* a CmpFrame. Each template component is
    /// overlaid onto the whole result with the slots that don't match
    /// that component marked as `-1`.
    #[test]
    fn overlay_template_fallback_non_cmpframe_result() {
        let template = CmpFrame::new(
            Box::new(SkyFrame::new(SkySystem::Galactic)),
            Box::new(SpecFrame::new(SpecSystem::Freq)),
        );

        // Result is a plain BasicFrame with same axis count. Without
        // any matching downcast the SkyFrame / SpecFrame component
        // overlays will be no-ops; the call must still succeed.
        let mut result: DynFrame =
            Box::new(BasicFrameBuilder::new(template.naxes()).build().unwrap());
        let template_axes: Vec<i32> = (0..template.naxes() as i32).collect();
        apply_template_overlay(&template, &template_axes, result.as_mut()).unwrap();
        assert_eq!(result.naxes(), template.naxes());
    }

    /// When the template's permutation makes the first result subframe
    /// see attributes from *both* of the template's components, the
    /// clean-partition heuristic must give up and fall through to the
    /// per-axis branch (cmpframe.c:305-322).
    #[test]
    fn overlay_template_falls_through_when_partition_unclean() {
        // Template: CmpFrame(SkyFrame(2 axes), SpecFrame(1 axis))
        //   internal order = (lon, lat, freq).
        // Permute to external order (lon, freq, lat):
        //   external 0 -> internal 0 (lon, in frame1)
        //   external 1 -> internal 2 (freq, in frame2)
        //   external 2 -> internal 1 (lat, in frame1)
        let mut template = CmpFrame::new(
            Box::new(SkyFrame::new(SkySystem::Galactic)),
            Box::new(SpecFrame::new(SpecSystem::Freq)),
        );
        template.perm_axes(&[1, 3, 2]).unwrap();

        // Result: identity-permuted CmpFrame(SkyFrame, SpecFrame).
        // result.frame1 = SkyFrame (2 axes), result.frame2 = SpecFrame (1 axis).
        // Identity template_axes maps result external axis i to template
        // external axis i.
        let mut result: DynFrame = Box::new(CmpFrame::new(
            Box::new(SkyFrame::new(SkySystem::Fk5)),
            Box::new(SpecFrame::new(SpecSystem::Freq)),
        ));
        let template_axes: Vec<i32> = vec![0, 1, 2];
        // First result subframe (SkyFrame, 2 axes) receives (lon, freq) from
        // the template, which crosses the partition boundary, so the
        // clean-partition path must abort and the fall-back path runs.
        apply_template_overlay(&template, &template_axes, result.as_mut()).unwrap();
    }
}
