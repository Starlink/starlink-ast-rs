//! A compound spectrum/flux frame (SpecFluxFrame).

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, FluxFrame, Frame, FrameRef, FrameSet, SpecFrame};
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
}

impl SpecFluxFrame {
    /// Create a new `SpecFluxFrame` from a SpecFrame and a FluxFrame.
    ///
    /// The SpecFrame forms axis 1, and the FluxFrame forms axis 2.
    pub fn new(spec_frame: SpecFrame, flux_frame: FluxFrame) -> Self {
        Self {
            spec_frame,
            flux_frame,
        }
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

    fn convert(&self, to: &dyn Frame, _domains: &[&str]) -> AstResult<Option<FrameSet>> {
        // For now, only convert to another SpecFluxFrame with matching sub-frames.
        if let Some(to_sff) = to.as_any().downcast_ref::<SpecFluxFrame>() {
            // Check if spec systems and flux systems match.
            if self.spec_frame.system() == to_sff.spec_frame.system()
                && self.flux_frame.system() == to_sff.flux_frame.system()
            {
                let mut fs = FrameSet::new(self.snapshot());
                fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(2)), to.snapshot())?;
                Ok(Some(fs))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
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
