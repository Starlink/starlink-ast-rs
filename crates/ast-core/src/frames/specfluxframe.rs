//! A compound spectrum/flux frame (SpecFluxFrame).

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, FluxFrame, Frame, SpecFrame};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, UnitMap};
use crate::serial::{Node, Serializable, frame_from_node};

/// A compound 2-axis frame combining a SpecFrame (axis 1) and a FluxFrame (axis 2).
///
/// A `SpecFluxFrame` is used to describe a spectrum plot where the first axis
/// represents spectral position and the second axis represents flux. It is
/// essentially a [`CmpFrame`](crate::frames::CmpFrame) with known component
/// types, enabling type-safe access to the spectral and flux components.
///
/// In the C library, `SpecFluxFrame` inherits from `CmpFrame`. In this Rust
/// port, it stores the two components directly with their concrete types.
///
/// # Examples
///
/// ```
/// use ast_core::frames::{Frame, FluxFrame, FluxSystem, SpecFluxFrame, SpecFrame, SpecSystem};
///
/// let sf = SpecFrame::new(SpecSystem::Freq);
/// let ff = FluxFrame::new(FluxSystem::FluxDen);
/// let sff = SpecFluxFrame::new(sf, ff);
/// assert_eq!(sff.naxes(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct SpecFluxFrame {
    spec_frame: SpecFrame,
    flux_frame: FluxFrame,
    /// External-axis swap flag. `false` (default) leaves axis 0 as
    /// the SpecFrame and axis 1 as the FluxFrame. `true` swaps them.
    /// Mirrors C `astGetPerm` on a SpecFluxFrame whose component
    /// order has been permuted via `astPermAxes`.
    swap: bool,

    // Match-control attributes (used by `astFindFrame` / `astMatch`).
    min_axes_opt: Option<usize>,
    max_axes_opt: Option<usize>,
    match_end_opt: Option<bool>,
    preserve_axes_opt: Option<bool>,
    permute_opt: Option<bool>,
}

impl SpecFluxFrame {
    /// Create a new `SpecFluxFrame` from a SpecFrame and a FluxFrame.
    ///
    /// The SpecFrame forms axis 1, and the FluxFrame forms axis 2.
    pub fn new(spec_frame: SpecFrame, flux_frame: FluxFrame) -> Self {
        Self {
            spec_frame,
            flux_frame,
            swap: false,
            min_axes_opt: None,
            max_axes_opt: None,
            match_end_opt: None,
            preserve_axes_opt: None,
            permute_opt: None,
        }
    }

    /// Whether the external axis order is (Flux, Spec) instead of
    /// the default (Spec, Flux). Mirrors a non-identity perm on a
    /// 2-axis CmpFrame.
    pub fn axes_swapped(&self) -> bool {
        self.swap
    }

    /// Return a reference to the embedded SpecFrame (axis 1).
    pub fn spec_frame(&self) -> &SpecFrame {
        &self.spec_frame
    }

    /// Return a mutable reference to the embedded SpecFrame (axis 1).
    pub fn spec_frame_mut(&mut self) -> &mut SpecFrame {
        &mut self.spec_frame
    }

    /// Return a reference to the embedded FluxFrame (axis 2).
    pub fn flux_frame(&self) -> &FluxFrame {
        &self.flux_frame
    }

    /// Return a mutable reference to the embedded FluxFrame (axis 2).
    pub fn flux_frame_mut(&mut self) -> &mut FluxFrame {
        &mut self.flux_frame
    }

    /// Route a 1-based compound axis to the appropriate child frame
    /// and its local 1-based axis index.
    ///
    /// Axis 1 maps to the SpecFrame; axis 2 maps to the FluxFrame.
    fn route_axis(&self, axis: usize) -> (&dyn Frame, usize) {
        if axis == 1 {
            (&self.spec_frame, 1)
        } else {
            (&self.flux_frame, 1)
        }
    }

    /// Validate that `axis` is 1 or 2.
    fn validate_axis(&self, axis: usize) -> AstResult<()> {
        if !(1..=2).contains(&axis) {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("SpecFluxFrame has 2 axes, got axis={axis}"),
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Frame trait implementation
// ---------------------------------------------------------------------------

impl Frame for SpecFluxFrame {
    fn naxes(&self) -> usize {
        2
    }

    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        (self, axis)
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
        // Euclidean distance in the 2-D compound space.
        let d0 = a[0] - b[0];
        let d1 = a[1] - b[1];
        Ok((d0 * d0 + d1 * d1).sqrt())
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        if point_has_bad(a) || point_has_bad(b) || point_has_bad(c) {
            return Ok(BAD_VALUE);
        }
        let ba0 = a[0] - b[0];
        let ba1 = a[1] - b[1];
        let bc0 = c[0] - b[0];
        let bc1 = c[1] - b[1];
        let dot = ba0 * bc0 + ba1 * bc1;
        let mag_ba = (ba0 * ba0 + ba1 * ba1).sqrt();
        let mag_bc = (bc0 * bc0 + bc1 * bc1).sqrt();
        let denom = mag_ba * mag_bc;
        if denom == 0.0 {
            return Ok(0.0);
        }
        let cos_angle = (dot / denom).clamp(-1.0, 1.0);
        Ok(cos_angle.acos())
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        if point_has_bad(from) || point_has_bad(to) || is_bad(dist) {
            return Ok(vec![BAD_VALUE; 2]);
        }
        let total = self.distance(from, to)?;
        if total == 0.0 || is_bad(total) {
            return Ok(from.to_vec());
        }
        let scale = dist / total;
        Ok(vec![
            from[0] + scale * (to[0] - from[0]),
            from[1] + scale * (to[1] - from[1]),
        ])
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        // Delegate axis 1 to SpecFrame, axis 2 to FluxFrame.
        self.spec_frame.norm(&mut coords[..1])?;
        self.flux_frame.norm(&mut coords[1..2])?;
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    fn map_from(&self, _previous: &dyn Frame) -> AstResult<DynMapping> {
        Ok(Box::new(UnitMap::new(2)))
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        let Some(other) = other.as_any().downcast_ref::<SpecFluxFrame>() else {
            return false;
        };
        self.spec_frame.equal(&other.spec_frame) && self.flux_frame.equal(&other.flux_frame)
    }

    fn system_string(&self) -> Option<&str> {
        // SpecFluxFrame is a compound — no single system string.
        None
    }

    fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        super::validate_permutation(perm, 2)?;
        // Two-element permutation: identity is [1,2]; swap is [2,1].
        if perm[0] == 2 && perm[1] == 1 {
            self.swap = !self.swap;
        }
        Ok(())
    }

    // -----------------------------------------------------------------
    // Match-control attributes (mirror C `astGet/SetMinAxes` etc.).
    // -----------------------------------------------------------------

    fn min_axes(&self) -> usize {
        self.min_axes_opt.unwrap_or(2)
    }
    fn set_min_axes(&mut self, n: usize) {
        self.min_axes_opt = Some(n);
    }
    fn test_min_axes(&self) -> bool {
        self.min_axes_opt.is_some()
    }
    fn max_axes(&self) -> usize {
        self.max_axes_opt.unwrap_or(2)
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

    // -----------------------------------------------------------------
    // Match / SubFrame / Overlay (specfluxframe.c:1216 / 1428).
    // -----------------------------------------------------------------

    fn match_template(
        &self,
        target: &dyn Frame,
        _match_sub: bool,
    ) -> AstResult<Option<super::find_frame::MatchResult>> {
        // Mirrors specfluxframe.c:1216. For non-SpecFluxFrame targets
        // C falls back to the CmpFrame parent's Match. The Rust port
        // does not currently implement a CmpFrame parent for
        // SpecFluxFrame (it stores its components directly), so for
        // non-SpecFluxFrame targets we return None — matching the
        // observable behaviour for any case the C code would have
        // delegated to a parent that knew nothing about flux/spec
        // pairings.
        let Some(target_sff) = target.as_any().downcast_ref::<SpecFluxFrame>() else {
            return Ok(None);
        };

        // Axis-swap detection (specfluxframe.c:1363-1375).
        let swap1 = self.swap;
        let swap2 = target_sff.swap;
        let swap = swap1 != swap2;
        if swap && !self.permute() {
            return Ok(None);
        }

        // Build axis association arrays (specfluxframe.c:1383-1401).
        let (template_axes_vec, target_axes_vec) = if self.preserve_axes() {
            (
                vec![if swap { 1 } else { 0 }, if swap { 0 } else { 1 }],
                vec![0i32, 1],
            )
        } else {
            (
                vec![0i32, 1],
                vec![if swap { 1 } else { 0 }, if swap { 0 } else { 1 }],
            )
        };

        // Defer to target's SubFrame (specfluxframe.c:1408).
        let Some((map, result)) =
            target.sub_frame(Some(self), 2, &target_axes_vec, &template_axes_vec)?
        else {
            return Ok(None);
        };

        Ok(Some(super::find_frame::MatchResult {
            template_axes: template_axes_vec,
            target_axes: target_axes_vec,
            map,
            result,
        }))
    }

    fn sub_frame(
        &self,
        template: Option<&dyn Frame>,
        result_naxes: usize,
        target_axes: &[i32],
        template_axes: &[i32],
    ) -> AstResult<Option<(DynMapping, DynFrame)>> {
        specfluxframe_sub_frame(self, template, result_naxes, target_axes, template_axes)
    }

    fn overlay_template(&mut self, template: &dyn Frame, template_axes: &[i32]) -> AstResult<()> {
        // SpecFluxFrame Overlay forwards to the underlying SpecFrame
        // and FluxFrame Overlays for whichever axis the template
        // covers. Mirrors specfluxframe.c which inherits Overlay from
        // CmpFrame. Without a Rust CmpFrame parent we approximate by
        // pattern-matching the template type.
        if let Some(t_sff) = template.as_any().downcast_ref::<SpecFluxFrame>() {
            // Spec axis: pass template SpecFrame to result SpecFrame.
            self.spec_frame
                .overlay_template(&t_sff.spec_frame, &[template_axes[0]])?;
            // Flux axis: pass template FluxFrame to result FluxFrame.
            self.flux_frame
                .overlay_template(&t_sff.flux_frame, &[template_axes[1]])?;
            return Ok(());
        }
        // Other template types: rely on each component's overlay
        // behaviour with the bare template (each will downcast and
        // skip if the type doesn't match).
        if !template_axes.is_empty() {
            self.spec_frame
                .overlay_template(template, &[template_axes[0]])?;
        }
        if template_axes.len() > 1 {
            self.flux_frame
                .overlay_template(template, &[template_axes[1]])?;
        }
        Ok(())
    }

    fn pick_axes_snapshot(&self, target_axes: &[i32]) -> AstResult<DynFrame> {
        // Full pick of both axes in canonical order: identity.
        if target_axes.len() == 2 && target_axes[0] == 0 && target_axes[1] == 1 {
            return Ok(Box::new(self.clone()));
        }
        // Reverse pick: swap axes.
        if target_axes.len() == 2 && target_axes[0] == 1 && target_axes[1] == 0 {
            let mut snap = self.clone();
            snap.swap = !snap.swap;
            return Ok(Box::new(snap));
        }
        // Any other selection collapses to a BasicFrame snapshot.
        let bf = super::basic::BasicFrameBuilder::new(target_axes.len()).build()?;
        Ok(Box::new(bf))
    }
}

// ---------------------------------------------------------------------------
// SubFrame helper
// ---------------------------------------------------------------------------

/// Mirrors C `SubFrame` for SpecFluxFrame (specfluxframe.c:1428).
fn specfluxframe_sub_frame(
    target: &SpecFluxFrame,
    template: Option<&dyn Frame>,
    result_naxes: usize,
    target_axes: &[i32],
    template_axes: &[i32],
) -> AstResult<Option<(DynMapping, DynFrame)>> {
    // Result-is-a-SpecFluxFrame branch (specfluxframe.c:1554+): only
    // when template is itself a SpecFluxFrame and we're picking 2
    // axes from a 2-axis target.
    let template_is_sff = template
        .as_ref()
        .map(|t| t.as_any().downcast_ref::<SpecFluxFrame>().is_some())
        .unwrap_or(false);

    if template_is_sff && result_naxes == 2 {
        // Form result from a copy of the target.
        let mut result = target.clone();
        // Apply the target_axes permutation (specfluxframe.c:1562).
        if target_axes.len() == 2 && target_axes[0] == 1 && target_axes[1] == 0 {
            result.swap = !result.swap;
        }

        // Overlay the template attributes (specfluxframe.c:1565).
        if let Some(t) = template {
            let mut result_dyn: DynFrame = Box::new(result);
            result_dyn.overlay_template(t, template_axes)?;
            // Recover the result SpecFluxFrame after overlay.
            let result = result_dyn
                .as_any()
                .downcast_ref::<SpecFluxFrame>()
                .expect("overlay preserves type")
                .clone();

            // Build the conversion Mapping. The full MakeSFMapping
            // (specfluxframe.c) requires a flux-frequency Mapping
            // which depends on both component conversions plus the
            // SpecFrame defining the spectral position. Without that
            // infrastructure ported, we fall back to identity for
            // matching systems and None for cross-system, mirroring
            // the existing FluxFrame::sub_frame and SpecFluxFrame's
            // own convert behaviour.
            if target.spec_frame.system() == result.spec_frame.system()
                && target.flux_frame.system() == result.flux_frame.system()
            {
                let map: DynMapping = Box::new(UnitMap::new(2));
                return Ok(Some((map, Box::new(result))));
            }
            return Ok(None);
        }

        // No template — identity Mapping over the (possibly
        // permuted) result.
        let map: DynMapping = Box::new(UnitMap::new(2));
        return Ok(Some((map, Box::new(result))));
    }

    // parent_subframe branch — produce a BasicFrame-shaped result
    // via the default Frame::SubFrame.
    super::find_frame::default_sub_frame(target, template, target_axes, template_axes)
}

// ---------------------------------------------------------------------------
// Serialisation
// ---------------------------------------------------------------------------

impl Serializable for SpecFluxFrame {
    fn class_name(&self) -> &'static str {
        "SpecFluxFrame"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Compound spectrum/flux Frame".to_string());

        // SpecFluxFrame inherits from CmpFrame in the C library, so we
        // serialise the IsA chain appropriately. The frame data is stored
        // as CmpFrame-style FrameA and FrameB child objects.
        node.push_isa("Frame", Some("Coordinate Frame".to_string()));

        // Component frames as nested objects (same as CmpFrame).
        let node_a = Serializable::to_node(&self.spec_frame)?;
        let node_b = Serializable::to_node(&self.flux_frame)?;
        node.push_object("FrameA", node_a, Some("First component Frame".to_string()));
        node.push_object("FrameB", node_b, Some("Second component Frame".to_string()));

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let node_a = node
            .get_object("FrameA")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "SpecFluxFrame: missing FrameA"))?;
        let node_b = node
            .get_object("FrameB")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "SpecFluxFrame: missing FrameB"))?;

        // FrameA must be a SpecFrame, FrameB must be a FluxFrame.
        let frame_a = frame_from_node(node_a)?;
        let frame_b = frame_from_node(node_b)?;

        let spec_frame = frame_a
            .as_any()
            .downcast_ref::<SpecFrame>()
            .ok_or_else(|| {
                AstError::new(ErrorCode::Lderr, "SpecFluxFrame: FrameA is not a SpecFrame")
            })?
            .clone();

        let flux_frame = frame_b
            .as_any()
            .downcast_ref::<FluxFrame>()
            .ok_or_else(|| {
                AstError::new(ErrorCode::Lderr, "SpecFluxFrame: FrameB is not a FluxFrame")
            })?
            .clone();

        Ok(SpecFluxFrame::new(spec_frame, flux_frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::{FluxSystem, SpecSystem};

    fn sff() -> SpecFluxFrame {
        SpecFluxFrame::new(
            SpecFrame::new(SpecSystem::Freq),
            FluxFrame::new(FluxSystem::FluxDen),
        )
    }

    #[test]
    fn match_template_specflux_self() {
        let template = sff();
        let target = sff();
        let m = template
            .match_template(&target, false)
            .unwrap()
            .expect("self-match");
        assert_eq!(m.template_axes.len(), 2);
        assert_eq!(m.target_axes.len(), 2);
        assert_eq!(m.result.naxes(), 2);
    }

    #[test]
    fn match_template_specflux_against_basicframe_fails() {
        let template = sff();
        let target = crate::frames::BasicFrameBuilder::new(2).build().unwrap();
        let result = template.match_template(&target, false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn match_template_specflux_swap_blocked_when_permute_false() {
        let mut template = sff();
        template.set_permute(false);
        let mut target = sff();
        // Swap the target axes to force a permutation.
        target.perm_axes(&[2, 1]).unwrap();
        let result = template.match_template(&target, false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn match_template_specflux_swap_allowed_when_permute_true() {
        let mut template = sff();
        template.set_permute(true);
        let mut target = sff();
        target.perm_axes(&[2, 1]).unwrap();
        let m = template
            .match_template(&target, false)
            .unwrap()
            .expect("permute=true allows swap");
        assert_eq!(m.template_axes.len(), 2);
    }

    #[test]
    fn sub_frame_specflux_to_specflux_identity() {
        let target = sff();
        let template = sff();
        let (map, result) = target
            .sub_frame(Some(&template), 2, &[0, 1], &[0, 1])
            .unwrap()
            .expect("identity sub_frame");
        assert_eq!(map.nin(), 2);
        assert_eq!(map.nout(), 2);
        let r = result.as_any().downcast_ref::<SpecFluxFrame>().unwrap();
        assert!(!r.axes_swapped());
    }

    #[test]
    fn sub_frame_specflux_axis_swap() {
        let target = sff();
        let template = sff();
        let (_map, result) = target
            .sub_frame(Some(&template), 2, &[1, 0], &[0, 1])
            .unwrap()
            .expect("swap sub_frame");
        let r = result.as_any().downcast_ref::<SpecFluxFrame>().unwrap();
        assert!(r.axes_swapped());
    }

    #[test]
    fn pick_axes_snapshot_full_returns_specflux() {
        let target = sff();
        let snap = target.pick_axes_snapshot(&[0, 1]).unwrap();
        assert!(snap.as_any().downcast_ref::<SpecFluxFrame>().is_some());
    }

    #[test]
    fn pick_axes_snapshot_partial_returns_basicframe() {
        let target = sff();
        let snap = target.pick_axes_snapshot(&[0]).unwrap();
        assert_eq!(snap.naxes(), 1);
        assert!(snap.as_any().downcast_ref::<SpecFluxFrame>().is_none());
    }
}
