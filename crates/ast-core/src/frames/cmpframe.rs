//! A compound frame combining two child frames.

use std::fmt;

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, UnitMap};
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
}

impl CmpFrame {
    /// Create a compound frame from two child frames.
    ///
    /// The resulting frame has `frame1.naxes() + frame2.naxes()` axes.
    pub fn new(frame1: DynFrame, frame2: DynFrame) -> Self {
        Self { frame1, frame2 }
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

    /// Route a 1-based compound axis to the owning child frame and its
    /// local 1-based axis index.
    ///
    /// Axes 1..=N1 map to `(frame1, axis)`.
    /// Axes N1+1..=N1+N2 map to `(frame2, axis - N1)`.
    ///
    /// # Panics
    ///
    /// Panics if `axis` is out of range. Call [`validate_axis`] first.
    fn route_axis(&self, axis: usize) -> (&dyn Frame, usize) {
        let n1 = self.frame1.naxes();
        if axis <= n1 {
            (self.frame1.as_ref(), axis)
        } else {
            (self.frame2.as_ref(), axis - n1)
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
        let n1 = self.frame1.naxes();
        let (first, second) = coords.split_at_mut(n1);
        self.frame1.norm(first)?;
        self.frame2.norm(second)?;
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(CmpFrame::new(
            self.frame1.snapshot(),
            self.frame2.snapshot(),
        ))
    }

    fn map_from(&self, _previous: &dyn Frame) -> AstResult<DynMapping> {
        Ok(Box::new(UnitMap::new(self.naxes())))
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

        // Domain: join component domains with "-"
        let domain_a = node_a.get_string_any("Domain").unwrap_or("").to_string();
        let domain_b = node_b.get_string_any("Domain").unwrap_or("").to_string();
        let compound_domain = if domain_a.is_empty() && domain_b.is_empty() {
            String::new()
        } else {
            format!("{}-{}", domain_a, domain_b)
        };
        if !compound_domain.is_empty() {
            node.push_unset(
                "Domain",
                NodeValue::String(compound_domain),
                Some("Coordinate system domain".to_string()),
            );
        }

        // Lbl{n} and Uni{n} from child frames
        for ax in 1..=naxes {
            let (child_node, local_ax) = if ax <= n1 {
                (&node_a, ax)
            } else {
                (&node_b, ax - n1)
            };
            let lbl_key = format!("Lbl{local_ax}");
            if let Some(lbl) = child_node.get_string_any(&lbl_key) {
                node.push_unset(
                    &format!("Lbl{ax}"),
                    NodeValue::String(lbl.to_string()),
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

        Ok(CmpFrame::new(frame1, frame2))
    }
}
