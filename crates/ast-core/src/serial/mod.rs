//! Serialisation: Node tree, Serializable trait, and Native format.

pub(crate) mod native_read;
pub(crate) mod native_write;
pub(crate) mod node;

pub(crate) use node::{Node, NodeValue};

use std::cell::RefCell;

use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{
    BasicFrame, BasicFrameBuilder, CmpFrame, DSBSpecFrame, DynFrame, FluxFrame, Frame, FrameRef,
    FrameSet, SkyFrame, SpecFluxFrame, SpecFrame, TimeFrame,
};
use crate::mappings::{
    ChebyMap, CmpMap, DssMap, DynMapping, GrismMap, LutMap, Mapping, MathMap, MatrixMap, NormMap,
    PcdMap, PermMap, PolyMap, RateMap, SelectorMap, ShiftMap, SlaMap, SpecMap, SphMap, SplineMap,
    SwitchMap, TimeMap, TranMap, UnitMap, UnitNormMap, WcsMap, WinMap, XphMap, ZoomMap,
};
use crate::mappings::{IntraMap, IntraRegistry};
use crate::regions::data::RegionData;
use crate::regions::{
    BoxRegion, Circle, CmpOperator, CmpRegion, DynRegion, Ellipse, Interval, NullRegion, PointList,
    Polygon, Prism, Region,
};

/// Internal trait for serialising AST objects to/from Node trees.
pub(crate) trait Serializable {
    /// The AST class name for Begin/End markers.
    fn class_name(&self) -> &'static str;

    /// Serialise this object to a Node tree.
    fn to_node(&self) -> AstResult<Node>;

    /// Reconstruct from a Node tree.
    fn from_node(node: &Node) -> AstResult<Self>
    where
        Self: Sized;
}

// ---------------------------------------------------------------------------
// Mapping base helpers
// ---------------------------------------------------------------------------

/// Write the Object-level attributes that precede the Mapping block. C dumps
/// the class chain from the top down, so Object's attributes and its `IsA`
/// marker come before Nin (`object.c:1697`). `Ident` is written active when it
/// is set, even blank, and the `IsA Object` marker only when something was
/// written, matching C's `astWriteString` / `astWriteIsA` rules.
pub(crate) fn write_object_base(node: &mut Node, mapping: &dyn Mapping) {
    if mapping.test_ident() {
        node.push_string(
            "Ident",
            mapping.ident(),
            Some("Permanent Object identification string".to_string()),
        );
        node.push_isa("Object", Some("AST Object".to_string()));
    }
}

/// Write common Mapping attributes (Ident, Nin, Nout, IsSimp, Invert,
/// IsA Mapping).
pub(crate) fn write_mapping_base(node: &mut Node, mapping: &dyn Mapping) {
    write_object_base(node, mapping);
    // Write intrinsic (forward-direction) Nin and Nout, matching the C library
    // which uses `this->nin` / `this->nout` directly (bypassing the Invert
    // attribute). When inverted, nin()/nout() return swapped values, so we
    // undo the swap.
    let (nin, nout) = if mapping.is_inverted() {
        (mapping.nout(), mapping.nin())
    } else {
        (mapping.nin(), mapping.nout())
    };
    node.push_int(
        "Nin",
        nin as i64,
        Some("Number of input coordinates".to_string()),
    );
    if nout != nin {
        node.push_int(
            "Nout",
            nout as i64,
            Some("Number of output coordinates".to_string()),
        );
    }
    if mapping.dump_is_simple() {
        node.push_int("IsSimp", 1, Some("Mapping has been simplified".to_string()));
    }
    // RestrictedSimplify (C `ReSimp`) is a base-Mapping attribute but only
    // CmpMap ever carries a non-default value. Emitted between IsSimp and
    // AlSimp to match the C base Dump order (mapping.c:23764).
    if mapping.restricted_simplify() {
        node.push_int(
            "ReSimp",
            1,
            Some("Simplify restricted to allowed Mappings".to_string()),
        );
    }
    if mapping.allow_simplify() {
        node.push_int(
            "AlSimp",
            1,
            Some("Will be used by restricted Simplify".to_string()),
        );
    }
    if mapping.is_inverted() {
        node.push_int("Invert", 1, Some("Mapping inverted".to_string()));
    }
    // C writes Fwd/Inv from the Mapping base-struct fields, which only
    // CmpMap populates with computed values — dynamic per-class
    // definedness (PolyMap, MatrixMap, TranMap, ...) is never dumped.
    let (intrinsic_fwd, intrinsic_inv) = mapping.dump_tran_flags();
    if !intrinsic_fwd {
        node.push_int(
            "Fwd",
            0,
            Some("Forward transformation not defined".to_string()),
        );
    }
    if !intrinsic_inv {
        node.push_int(
            "Inv",
            0,
            Some("Inverse transformation not defined".to_string()),
        );
    }
    push_report(node, mapping.report_attr());
    node.push_isa(
        "Mapping",
        Some("Mapping between coordinate systems".to_string()),
    );
}

/// Write the `Report` item when it is set, as mapping.c `Dump` does last in
/// the Mapping part.
fn push_report(node: &mut Node, report: Option<bool>) {
    if let Some(report) = report {
        node.push_int(
            "Report",
            i64::from(report),
            Some(if report {
                "Report coordinate transformations".to_string()
            } else {
                "Don't report coordinate transformations".to_string()
            }),
        );
    }
}

/// The Mapping part of a FrameSet or Region dump. It holds only the flags
/// such a Mapping can have set, `ReSimp`, `AlSimp` and `Report` in
/// mapping.c `Dump` order, and is written, with its `IsA Mapping` marker,
/// only when one of them is.
pub(crate) fn write_flags_mapping_part(node: &mut Node, flags: &crate::mappings::MappingFlags) {
    if flags.restricted_simplify {
        node.push_int(
            "ReSimp",
            1,
            Some("Simplify restricted to allowed Mappings".to_string()),
        );
    }
    if flags.allow_simplify {
        node.push_int(
            "AlSimp",
            1,
            Some("Will be used by restricted Simplify".to_string()),
        );
    }
    push_report(node, flags.report);
    if flags.restricted_simplify || flags.allow_simplify || flags.report.is_some() {
        node.push_isa(
            "Mapping",
            Some("Mapping between coordinate systems".to_string()),
        );
    }
}

/// The flags in a description's Mapping part: `Report` if set, and the
/// `AlSimp` and `ReSimp` flags (mapping.c `astLoadMapping`).
pub(crate) fn read_mapping_flags(whole: &Node) -> crate::mappings::MappingFlags {
    let node = whole.section("Mapping");
    crate::mappings::MappingFlags {
        report: node.get("Report").map(|_| node.get_int("Report", 0) != 0),
        allow_simplify: node.get_int("AlSimp", 0) != 0,
        restricted_simplify: node.get_int("ReSimp", 0) != 0,
        merge_tag: 0,
        // C reads Invert with its unset value as the default
        // (mapping.c:24130), so an `Invert = 0` card leaves it set to 0.
        invert_zero: node.get("Invert").is_some() && node.get_int("Invert", 0) == 0,
    }
}

/// Common Mapping attributes read from a serialized node.
#[derive(Debug, Clone)]
pub(crate) struct MappingBaseAttrs {
    pub nin: usize,
    pub nout: usize,
    pub inverted: bool,
    pub has_been_simplified: bool,
    pub has_forward: bool,
    pub has_inverse: bool,
    pub restricted_simplify: bool,
}

/// Read common Mapping attributes from the Mapping part of a description
/// (mapping.c `astLoadMapping` reads them through astReadClassData).
pub(crate) fn read_mapping_base(whole: &Node) -> MappingBaseAttrs {
    let node = &whole.section("Mapping");
    let nin = node.get_int("Nin", 0) as usize;
    let nout_raw = node.get_int("Nout", 0);
    let nout = if nout_raw <= 0 {
        nin
    } else {
        nout_raw as usize
    };
    MappingBaseAttrs {
        nin,
        nout,
        inverted: node.get_int("Invert", 0) != 0,
        has_been_simplified: node.get_int("IsSimp", 0) != 0,
        has_forward: node.get_int("Fwd", 1) != 0,
        has_inverse: node.get_int("Inv", 1) != 0,
        restricted_simplify: node.get_int("ReSimp", 0) != 0,
    }
}

// ---------------------------------------------------------------------------
// Mapping dispatch
// ---------------------------------------------------------------------------

/// Reconstruct a concrete Mapping from a Node tree, dispatching by class name.
pub(crate) fn mapping_from_node(node: &Node) -> AstResult<DynMapping> {
    let mut result = mapping_from_node_inner(node)?;
    apply_mapping_object_items(result.as_mut(), node);
    Ok(result)
}

/// Apply the Object and Mapping items that every Mapping class shares and
/// that the class loaders leave to their caller: AllowSimplify (C `AlSimp`),
/// RestrictedSimplify (`ReSimp`) and Report from the Mapping part, and Ident
/// from the Object part (mapping.c and
/// object.c load them). Frame-derived classes read their own Ident in
/// `from_node`, and their Mapping-side setter is a no-op.
fn apply_mapping_object_items(result: &mut dyn Mapping, node: &Node) {
    let flags = read_mapping_flags(node);
    if flags.allow_simplify {
        result.set_allow_simplify(true);
    }
    if flags.restricted_simplify {
        result.set_restricted_simplify(true);
    }
    if let Some(report) = flags.report {
        result.set_report_attr(Some(report));
    }
    if flags.invert_zero {
        result.mapping_flags_mut().invert_zero = true;
    }
    let object = node.section("Object");
    if let Some(ident) = object.get_string_if_set("Ident") {
        result.set_ident(&ident);
    }
}

/// Load a Mapping class as the top-level object of a description, with the
/// shared items [`mapping_from_node`] applies to a component Mapping.
fn load_mapping<T: Serializable + Mapping>(node: &Node) -> AstResult<T> {
    let mut m = T::from_node(node)?;
    apply_mapping_object_items(&mut m, node);
    Ok(m)
}

fn mapping_from_node_inner(node: &Node) -> AstResult<DynMapping> {
    match node.class.as_str() {
        "UnitMap" => Ok(Box::new(UnitMap::from_node(node)?)),
        "ZoomMap" => Ok(Box::new(ZoomMap::from_node(node)?)),
        "ShiftMap" => Ok(Box::new(ShiftMap::from_node(node)?)),
        "MatrixMap" => Ok(Box::new(MatrixMap::from_node(node)?)),
        "PermMap" => Ok(Box::new(PermMap::from_node(node)?)),
        "WinMap" => Ok(Box::new(WinMap::from_node(node)?)),
        "LutMap" => Ok(Box::new(LutMap::from_node(node)?)),
        "PolyMap" => Ok(Box::new(PolyMap::from_node(node)?)),
        "ChebyMap" => Ok(Box::new(ChebyMap::from_node(node)?)),
        "CmpMap" => Ok(Box::new(CmpMap::from_node(node)?)),
        "WcsMap" => Ok(Box::new(WcsMap::from_node(node)?)),
        "SlaMap" => Ok(Box::new(SlaMap::from_node(node)?)),
        "SpecMap" => Ok(Box::new(SpecMap::from_node(node)?)),
        "TimeMap" => Ok(Box::new(TimeMap::from_node(node)?)),
        "PcdMap" => Ok(Box::new(PcdMap::from_node(node)?)),
        "SphMap" => Ok(Box::new(SphMap::from_node(node)?)),
        "GrismMap" => Ok(Box::new(GrismMap::from_node(node)?)),
        "UnitNormMap" => Ok(Box::new(UnitNormMap::from_node(node)?)),
        "XphMap" => Ok(Box::new(XphMap::from_node(node)?)),
        "IntraMap" => Ok(Box::new(IntraMap::from_node(node)?)),
        "TranMap" => Ok(Box::new(TranMap::from_node(node)?)),
        "RateMap" => Ok(Box::new(RateMap::from_node(node)?)),
        "SplineMap" => Ok(Box::new(SplineMap::from_node(node)?)),
        "DssMap" => Ok(Box::new(DssMap::from_node(node)?)),
        "NormMap" => Ok(Box::new(NormMap::from_node(node)?)),
        "MathMap" => Ok(Box::new(MathMap::from_node(node)?)),
        "SelectorMap" => Ok(Box::new(SelectorMap::from_node(node)?)),
        "SwitchMap" => Ok(Box::new(SwitchMap::from_node(node)?)),
        "FrameSet" => Ok(Box::new(FrameSet::from_node(node)?)),
        // Regions implement Mapping.
        "Circle" => Ok(Box::new(Circle::from_node(node)?)),
        "Box" => Ok(Box::new(BoxRegion::from_node(node)?)),
        "Ellipse" => Ok(Box::new(Ellipse::from_node(node)?)),
        "Polygon" => Ok(Box::new(Polygon::from_node(node)?)),
        "Interval" => Ok(Box::new(Interval::from_node(node)?)),
        "NullRegion" => Ok(Box::new(NullRegion::from_node(node)?)),
        "CmpRegion" => Ok(Box::new(CmpRegion::from_node(node)?)),
        "PointList" => Ok(Box::new(PointList::from_node(node)?)),
        "Prism" => Ok(Box::new(Prism::from_node(node)?)),
        other => Err(AstError::new(
            ErrorCode::Lderr,
            format!("unknown Mapping class: {other}"),
        )),
    }
}

/// Extension trait enabling `to_node()` on `dyn Mapping`.
///
/// `Serializable` cannot be a supertrait of `Mapping` (it would require
/// `Self: Sized` which prevents object safety). This trait uses `AsAny`
/// to downcast to known concrete types at runtime.
pub(crate) trait MappingToNode {
    /// Serialise this mapping to a Node tree.
    fn to_node(&self) -> AstResult<Node>;
}

impl MappingToNode for dyn Mapping {
    fn to_node(&self) -> AstResult<Node> {
        if let Some(m) = self.as_any().downcast_ref::<UnitMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<ZoomMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<ShiftMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<MatrixMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<PermMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<WinMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<LutMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<PolyMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<ChebyMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<CmpMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<WcsMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<SlaMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<SpecMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<TimeMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<PcdMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<SphMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<GrismMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<UnitNormMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<XphMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<IntraMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<TranMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<RateMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<SplineMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<DssMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<NormMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<MathMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<SelectorMap>() {
            return m.to_node();
        }
        if let Some(m) = self.as_any().downcast_ref::<SwitchMap>() {
            return m.to_node();
        }
        if let Some(f) = self.as_any().downcast_ref::<FrameSet>() {
            return Serializable::to_node(f);
        }
        // Regions implement Mapping.
        if let Some(r) = self.as_any().downcast_ref::<Circle>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<BoxRegion>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<Ellipse>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<Polygon>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<Interval>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<NullRegion>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<CmpRegion>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<PointList>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<Prism>() {
            return Serializable::to_node(r);
        }
        Err(AstError::new(
            ErrorCode::Lderr,
            "unsupported Mapping type for serialisation",
        ))
    }
}

// ---------------------------------------------------------------------------
// Frame dispatch
// ---------------------------------------------------------------------------

/// Extension trait enabling `to_node()` on `dyn Frame`.
pub(crate) trait FrameToNode {
    /// Serialise this frame to a Node tree.
    fn to_node(&self) -> AstResult<Node>;
}

impl FrameToNode for dyn Frame {
    fn to_node(&self) -> AstResult<Node> {
        if let Some(f) = self.as_any().downcast_ref::<BasicFrame>() {
            return Serializable::to_node(f);
        }
        if let Some(f) = self.as_any().downcast_ref::<CmpFrame>() {
            return Serializable::to_node(f);
        }
        if let Some(f) = self.as_any().downcast_ref::<SkyFrame>() {
            return Serializable::to_node(f);
        }
        if let Some(f) = self.as_any().downcast_ref::<DSBSpecFrame>() {
            return Serializable::to_node(f);
        }
        if let Some(f) = self.as_any().downcast_ref::<SpecFrame>() {
            return Serializable::to_node(f);
        }
        if let Some(f) = self.as_any().downcast_ref::<TimeFrame>() {
            return Serializable::to_node(f);
        }
        if let Some(f) = self.as_any().downcast_ref::<FluxFrame>() {
            return Serializable::to_node(f);
        }
        if let Some(f) = self.as_any().downcast_ref::<SpecFluxFrame>() {
            return Serializable::to_node(f);
        }
        if let Some(f) = self.as_any().downcast_ref::<FrameSet>() {
            return Serializable::to_node(f);
        }
        // Regions implement Frame.
        if let Some(r) = self.as_any().downcast_ref::<Circle>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<BoxRegion>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<Ellipse>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<Polygon>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<Interval>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<NullRegion>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<CmpRegion>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<PointList>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<Prism>() {
            return Serializable::to_node(r);
        }
        Err(AstError::new(
            ErrorCode::Lderr,
            "unsupported Frame type for serialisation",
        ))
    }
}

/// Reconstruct a concrete Frame from a Node tree, dispatching by class name.
pub(crate) fn frame_from_node(node: &Node) -> AstResult<DynFrame> {
    match node.class.as_str() {
        "Frame" => Ok(Box::new(BasicFrame::from_node(node)?)),
        "SkyFrame" => Ok(Box::new(SkyFrame::from_node(node)?)),
        "DSBSpecFrame" => Ok(Box::new(DSBSpecFrame::from_node(node)?)),
        "SpecFrame" => Ok(Box::new(SpecFrame::from_node(node)?)),
        "TimeFrame" => Ok(Box::new(TimeFrame::from_node(node)?)),
        "FluxFrame" => Ok(Box::new(FluxFrame::from_node(node)?)),
        "SpecFluxFrame" => Ok(Box::new(SpecFluxFrame::from_node(node)?)),
        "CmpFrame" => Ok(Box::new(CmpFrame::from_node(node)?)),
        "FrameSet" => Ok(Box::new(FrameSet::from_node(node)?)),
        // Regions implement Frame.
        "Circle" => Ok(Box::new(Circle::from_node(node)?)),
        "Box" => Ok(Box::new(BoxRegion::from_node(node)?)),
        "Ellipse" => Ok(Box::new(Ellipse::from_node(node)?)),
        "Polygon" => Ok(Box::new(Polygon::from_node(node)?)),
        "Interval" => Ok(Box::new(Interval::from_node(node)?)),
        "NullRegion" => Ok(Box::new(NullRegion::from_node(node)?)),
        "CmpRegion" => Ok(Box::new(CmpRegion::from_node(node)?)),
        "PointList" => Ok(Box::new(PointList::from_node(node)?)),
        "Prism" => Ok(Box::new(Prism::from_node(node)?)),
        other => Err(AstError::new(
            ErrorCode::Lderr,
            format!("unknown or unsupported Frame class: {other}"),
        )),
    }
}

// ---------------------------------------------------------------------------
// PointSet serialisation helpers
// ---------------------------------------------------------------------------

/// Write a PointSet as a serialised "PointSet" object node.
///
/// Iteration order matches C `pointset.c::Dump`: outer loop over
/// points, inner loop over coords. So for `npoint=2 ncoord=2` the
/// emitted keys are X1=(pt 0, coord 0), X2=(pt 0, coord 1), X3=(pt 1,
/// coord 0), X4=(pt 1, coord 1) — i.e. each point's coordinates are
/// contiguous in the X-key sequence. This is the byte layout `libast`
/// produces and consumes; matching it is required for cross-
/// implementation interop, not just round-trip fidelity within our
/// own writer/reader pair.
fn write_pointset_node(points: &[Vec<f64>]) -> Node {
    let npoint = points.len();
    let ncoord = if npoint > 0 { points[0].len() } else { 0 };

    let mut node = Node::new("PointSet");
    node.class_comment = Some("Container for a set of points".to_string());
    node.push_int(
        "Npoint",
        npoint as i64,
        Some("Number of points".to_string()),
    );
    node.push_int(
        "Ncoord",
        ncoord as i64,
        Some("Number of coordinates per point".to_string()),
    );

    // The C PointSet Dump (pointset.c:2727) omits any axis value equal to
    // AST__BAD, and only attaches the "Coordinate values..." comment to the
    // first *good* value written. Mirror that: skip BAD entries entirely and
    // track whether the comment has been emitted yet.
    let mut idx = 1;
    let mut comment_pending = true;
    #[allow(clippy::needless_range_loop)]
    for pt in 0..npoint {
        for coord in 0..ncoord {
            let value = points[pt][coord];
            if value != crate::constants::BAD_VALUE {
                let key = format!("X{idx}");
                let comment = if comment_pending {
                    comment_pending = false;
                    Some("Coordinate values...".to_string())
                } else {
                    None
                };
                node.push_double(&key, value, comment);
            }
            idx += 1;
        }
    }

    node
}

/// Read a PointSet from a serialised "PointSet" node.
///
/// Returns `Vec<Vec<f64>>` where `result[i]` is point `i` (length
/// `ncoord`). Iteration matches C `pointset.c::Dump` (point-outer,
/// coord-inner) — see `write_pointset_node` for the byte layout.
fn read_pointset_node(node: &Node) -> AstResult<Vec<Vec<f64>>> {
    let ncoord = node.get_int("Ncoord", 0) as usize;
    let npoint = node.get_int("Npoint", 0) as usize;

    if ncoord == 0 || npoint == 0 {
        return Ok(Vec::new());
    }

    // A missing `X<n>` keyword means AST__BAD (BAD_VALUE), not zero: the C
    // PointSet Dump omits BAD axis values, and the reader defaults them to
    // AST__BAD (pointset.c:3228). Regions such as an unbounded Interval rely
    // on this — an omitted upper/lower bound is "no constraint", not 0.
    let mut points = vec![vec![crate::constants::BAD_VALUE; ncoord]; npoint];

    let mut idx = 1;
    #[allow(clippy::needless_range_loop)]
    for pt in 0..npoint {
        for coord in 0..ncoord {
            points[pt][coord] = node.get_double(&format!("X{idx}"), crate::constants::BAD_VALUE);
            idx += 1;
        }
    }

    Ok(points)
}

// ---------------------------------------------------------------------------
// Region base helpers
// ---------------------------------------------------------------------------

/// Write common Region attributes to a node.
///
/// This writes the Region-level attributes (Negate, Fill, MeshSz, Closed,
/// Adapt), the encapsulated FrameSet or Frame, the defining points as a
/// PointSet object, and the optional uncertainty region.
///
/// The Frame-inheritance attrs (Title, Naxes, Domain, Epoch, Lbl*, System)
/// and the `IsA Frame` marker are written by
/// [`write_frame_inheritance_for_region`] before this is called. This
/// function writes the Region's own instance variables (and the trailing
/// `IsA Region` marker — see [`write_region_isa_marker`]).
pub(crate) fn write_region_base(node: &mut Node, data: &RegionData) {
    // Region attribute emissions mirror `region.c::Dump`. The pattern is:
    //   `set = (ival != 0); astWriteInt(..., set, helpful=0, ival, ...)`
    // so a boolean attr with its non-default value emits uncommented and
    // the default omits entirely (set=0, helpful=0 → `Use` returns false).
    if data.negated {
        node.push_int("Negate", 1, Some("Region negated".to_string()));
    }
    // FillFactor and MeshSize emit only when explicitly set, mirroring C
    // `region.c:13063,13071` where `set = Test<Attr>()` gates emission
    // (helpful = 0), independent of whether the value equals a default.
    if data.fill_factor_set {
        node.push_double(
            "Fill",
            data.fill_factor,
            Some("Region fill factor".to_string()),
        );
    }
    if data.mesh_size_set {
        node.push_int(
            "MeshSz",
            data.mesh_size as i64,
            Some("No. of points used to represent boundary".to_string()),
        );
    }
    // Closed: emit only when explicitly set (mirrors C's
    // `astWriteInt(..., set=TestClosed(), helpful=0, ...)`). The comment
    // depends on the value.
    if data.closed_set {
        node.push_int(
            "Closed",
            if data.closed { 1 } else { 0 },
            Some(
                if data.closed {
                    "Boundary is inside"
                } else {
                    "Boundary is outside"
                }
                .to_string(),
            ),
        );
    }
    // Adapt: C library emits `Adapt = 1` uncommented (matches the
    // `set = (ival != 0)` rule above) when adaptive, and omits entirely
    // when not. This is the opposite polarity from what reads naturally
    // — see `region.c` line 13086.
    if data.adaptive {
        node.push_int(
            "Adapt",
            1,
            Some("Region adapts to coord sys changes".to_string()),
        );
    }

    // Write the FrameSet, mirroring C `region.c:13099-13112`. If the
    // base→current mapping is a UnitMap, the vertices are the same in both
    // Frames, so dump just the current Frame (as `Frm`) — but only when the
    // `RegionFS` attribute is non-zero. When `region_fs` is false (e.g. an
    // uncertainty sub-region whose current Frame equals its parent's base
    // Frame), the Frame can be inferred from the enclosing context, so C
    // omits both `Frm` and the `RegFS` line entirely. If the mapping is not
    // a UnitMap, dump the whole FrameSet and emit `RegFS` uncommented only
    // when RegionFS was explicitly set to 0.
    let mapping = data.frameset.mapping(FrameRef::Base, FrameRef::Current);
    let is_unit = mapping
        .as_ref()
        .map(|m| m.as_any().is::<UnitMap>())
        .unwrap_or(false);

    if is_unit {
        if data.region_fs {
            let frame_node = FrameToNode::to_node(data.frameset.current())
                .unwrap_or_else(|_| Node::new("Frame"));
            node.push_object("Frm", frame_node, Some("Coordinate system".to_string()));
        }
    } else {
        let fs_node =
            Serializable::to_node(&data.frameset).unwrap_or_else(|_| Node::new("FrameSet"));
        node.push_object(
            "FrmSet",
            fs_node,
            Some("Original & current coordinate systems".to_string()),
        );
        if !data.region_fs {
            node.push_int("RegFS", 0, Some("Do not include Frame in dump".to_string()));
        }
    }

    // Defining points (if any) — else emit RegAxes so the loader knows the
    // axis count without a PointSet to count from. Matches `region.c`
    // lines 13124-13135.
    if !data.points.is_empty() {
        let pts_node = write_pointset_node(&data.points);
        node.push_object(
            "Points",
            pts_node,
            Some("Points defining the shape".to_string()),
        );
    } else {
        node.push_int(
            "RegAxes",
            data.frameset.current().naxes() as i64,
            Some("Number of axes spanned by the Region".to_string()),
        );
    }

    // Uncertainty Region (if present).
    if let Some(ref unc) = data.uncertainty {
        let unc_node =
            RegionToNode::to_node(unc.as_ref()).unwrap_or_else(|_| Node::new("NullRegion"));
        node.push_object(
            "Unc",
            unc_node,
            Some("Region defining positional uncertainties.".to_string()),
        );
    }
}

/// Emit the Frame-inheritance commented defaults that the C library's
/// `frame.c::Dump` writes when called as a parent class of a Region. The
/// values come from the Region's encapsulated current frame. Order matches
/// `frame.c` lines 14302-14400: Title, Naxes, Domain, Epoch, Lbl*, System.
///
/// All entries are emitted commented (the `helpful=1` path in C, which
/// gives the human reader the inherited values but omits them on re-read).
pub(crate) fn write_frame_inheritance_for_region(
    node: &mut Node,
    frame: &dyn Frame,
) -> AstResult<()> {
    let frame_node = FrameToNode::to_node(frame).unwrap_or_else(|_| Node::new("Frame"));
    let naxes = frame.naxes();

    // Title — fall back to the C-style default `"N-d coordinate system"`.
    let title_value = frame_node
        .get_any("Title")
        .cloned()
        .unwrap_or_else(|| NodeValue::String(format!("{naxes}-d coordinate system")));
    node.push_unset(
        "Title",
        title_value,
        Some("Title of coordinate system".to_string()),
    );

    // Naxes — always commented at the Region level.
    node.push_unset(
        "Naxes",
        NodeValue::Int(naxes as i64),
        Some("Number of coordinate axes".to_string()),
    );

    // Domain — only emit if the frame has a non-empty domain string.
    if let Some(NodeValue::String(s)) = frame_node.get_any("Domain") {
        if !s.is_empty() {
            node.push_unset(
                "Domain",
                NodeValue::String(s.clone()),
                Some("Coordinate system domain".to_string()),
            );
        }
    }

    // Epoch — always commented at the Region level (`!isFrame = 1`).
    // C's `frame.c::Dump` uses `astGetEpoch(this)` (the *effective* value,
    // never the "if set" one), converted to a Besselian year below J1984 or
    // a Julian year at/above it. Scraping the frame's own serialiser fails
    // for classes that omit their own Epoch (e.g. CmpFrame), so compute the
    // effective epoch directly.
    let (epoch_year, is_besselian) = crate::frames::frame_epoch_effective_year(frame);
    let epoch_comment = if is_besselian {
        "Besselian epoch of observation"
    } else {
        "Julian epoch of observation"
    };
    node.push_unset(
        "Epoch",
        NodeValue::Double(epoch_year),
        Some(epoch_comment.to_string()),
    );

    // Per-axis Labels — pull from the Frame's Lbl* keys; fall back to the
    // standard "Axis N" default if the Frame elided them.
    for i in 1..=naxes {
        let key = format!("Lbl{i}");
        let label_value = frame_node
            .get_any(&key)
            .cloned()
            .unwrap_or_else(|| NodeValue::String(format!("Axis {i}")));
        node.push_unset(&key, label_value, Some(format!("Label for axis {i}")));
    }

    // System — bare Frame and CmpFrame don't emit System in their own dump
    // (`helpful=!isFrame=0` for them), so the value must come from the trait.
    let system_str = frame.system_string()?.unwrap_or("Cartesian").to_string();
    node.push_unset(
        "System",
        NodeValue::String(system_str),
        Some("Coordinate system type".to_string()),
    );

    // Remaining Frame-level attributes (AlSys, Uni*, Dir*, Bot*, Top*,
    // observer attributes, ActUnt, …). `frame.c::Dump` writes them after
    // System, all commented at the Region level (`!isFrame = 1`). The
    // frame's own serialiser already produced these items with the correct
    // comments and in frame.c order; carry over each scalar item that
    // precedes the frame's own `IsA Frame` marker, forcing it commented and
    // skipping the keys emitted explicitly above. Anything after `IsA Frame`
    // (nested component frames, CmpFrame `Axp*` permutation records) belongs
    // to the concrete class, not the inherited Frame block, and is dropped.
    for item in &frame_node.items {
        // Stop at the frame's own `IsA Frame` marker: everything after it
        // (nested component frames, CmpFrame `Axp*` permutation records)
        // belongs to the concrete class, not the inherited Frame block.
        // Earlier IsA markers (e.g. `IsA Object`, which precedes the Frame
        // attributes when an Ident is set) are skipped, not treated as the
        // stopping point.
        if item.key == "IsA" {
            if matches!(&item.value, NodeValue::String(s) if s == "Frame") {
                break;
            }
            continue;
        }
        let is_handled = matches!(
            item.key.as_str(),
            // Emitted explicitly above.
            "Title" | "Naxes" | "Domain" | "Epoch" | "System"
            // Object-level attributes: in a Region dump these come from the
            // Region itself, not the encapsulated frame, so drop the inner
            // frame's copies.
            | "Ident" | "ID" | "UseDfs"
        ) || item.key.starts_with("Lbl")
            // The axis permutation is the current Frame's; the Region's own
            // Frame part, which frame.c dumps here, is never permuted, since
            // region.c `PermAxes` permutes the FrameSet instead.
            || item.key.starts_with("Prm");
        if is_handled {
            continue;
        }
        // Only scalar attributes appear in the inherited Frame block.
        if matches!(item.value, NodeValue::Object(_)) {
            continue;
        }
        let value = match &item.value {
            NodeValue::Unset(inner) => (**inner).clone(),
            other => other.clone(),
        };
        // frame.c `Dump` writes ActUnt as set whenever the Frame's
        // ActiveUnit is set, whichever class is dumping (frame.c:14729).
        if let ("ActUnt", NodeValue::Int(v)) = (item.key.as_str(), &value) {
            node.push_int(&item.key, *v, item.comment.clone());
        } else {
            node.push_unset(&item.key, value, item.comment.clone());
        }
    }
    Ok(())
}

/// Emit the `IsA Region` marker that terminates the Region instance-variable
/// section of a leaf-class dump (Box, Interval, Polygon, etc.).
pub(crate) fn push_region_isa_marker(node: &mut Node) {
    node.push_isa(
        "Region",
        Some("An area within a coordinate system".to_string()),
    );
}

/// Emit the `IsA Frame` marker that terminates the Frame inheritance block
/// of a Region dump.
pub(crate) fn push_frame_isa_marker(node: &mut Node) {
    node.push_isa("Frame", Some("Coordinate system description".to_string()));
}

/// Read common Region attributes from a node, returning a RegionData.
///
/// This reads the Region-level attributes, the FrameSet or Frame, the
/// defining points, and the optional uncertainty region.
pub(crate) fn read_region_base(whole: &Node) -> AstResult<RegionData> {
    // The Region and Object classes' parts of the description, which their
    // loaders read through astReadClassData (region.c, object.c).
    let object = &whole.section("Object");
    let node = &whole.section("Region");
    let negated = node.get_int("Negate", 0) != 0;
    // A present `Fill`/`MeshSz` line means the attribute was explicitly set
    // (C emits them only when Test<Attr> is true). Their absence leaves the
    // effective value at the class default.
    let fill_factor_set = node
        .items
        .iter()
        .find(|i| i.key == "Fill")
        .is_some_and(|i| !matches!(&i.value, NodeValue::Unset(_)));
    let fill_factor = node.get_double("Fill", 1.0);
    let mesh_size_set = node
        .items
        .iter()
        .find(|i| i.key == "MeshSz")
        .is_some_and(|i| !matches!(&i.value, NodeValue::Unset(_)));
    let mesh_size = node.get_int("MeshSz", 0) as usize;
    let closed = node.get_int("Closed", 1) != 0;
    // C tracks Closed via `closed != -INT_MAX` (`region.c` line 12766). For
    // the round-trip we model the same explicit-set state via `closed_set`:
    // true iff the source had an *uncommented* `Closed = …` line.
    let closed_set = node
        .items
        .iter()
        .find(|i| i.key == "Closed")
        .is_some_and(|i| !matches!(&i.value, NodeValue::Unset(_)));
    let adaptive = node.get_int("Adapt", 1) != 0;

    // Read defining points first: their coordinate count sizes a dummy
    // FrameSet when neither `Frm` nor `FrmSet` is present (C uses `naxpt`,
    // `region.c:13543,13597`).
    let points = if let Some(pts_node) = node.get_object("Points") {
        read_pointset_node(pts_node)?
    } else {
        Vec::new()
    };
    // Axis count per point: from the PointSet if present, else from the
    // `RegAxes` attribute (C `region.c:13543,13554` — a Region with no
    // points, e.g. a Prism/NullRegion, records its axis span there).
    let naxpt = match points.first() {
        Some(p) => p.len(),
        None => node.get_int("RegAxes", 0) as usize,
    };

    // Read the FrameSet or Frame. Detect which one was emitted by key
    // presence — the C library only writes `RegFS` when RegionFS was
    // explicitly set (rare), so most dumps have either `Frm` or `FrmSet`
    // and no `RegFS` line. A nested region (an uncertainty sub-region, or
    // a component of a Prism/CmpRegion/SelectorMap) is dumped with neither
    // — it inherits its container's frame. In that case C synthesises a
    // dummy FrameSet with RegionFS=0 (`region.c:13593-13601`) that the
    // parent later replaces via `astSetRegFS`; we mirror that with the
    // `dummy_fs` flag.
    // C loader `region.c:13563-13603`: `Frm` present → RegionFS = 1;
    // `FrmSet` present → read RegFS (default 1); neither → dummy FrameSet
    // with RegionFS = 0.
    let (frameset, dummy_fs, region_fs) = if let Some(fs_node) = node.get_object("FrmSet") {
        let region_fs = node.get_int("RegFS", 1) != 0;
        (FrameSet::from_node(fs_node)?, false, region_fs)
    } else if let Some(frm_node) = node.get_object("Frm") {
        // Only a single Frame stored — create a 2-frame FrameSet with
        // UnitMap connecting base to current (matches the C library and
        // keeps with_integrity working).
        let frame = frame_from_node(frm_node)?;
        let naxes = frame.naxes();
        let current_snap = frame.snapshot();
        let mut fs = FrameSet::new(frame);
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(naxes)), current_snap)
            .expect("adding current frame to region FrameSet");
        (fs, false, true)
    } else {
        // Dummy FrameSet: a default BasicFrame of `naxpt` axes (>= 1),
        // marked dummy so the parent re-associates the real frame.
        // RegionFS = 0 (the Frame is inferred from the enclosing context).
        let nax = if naxpt > 0 { naxpt } else { 1 };
        let frame: DynFrame = Box::new(
            BasicFrameBuilder::new(nax)
                .build()
                .expect("building dummy region frame"),
        );
        let current_snap = frame.snapshot();
        let mut fs = FrameSet::new(frame);
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(nax)), current_snap)
            .expect("adding current frame to dummy region FrameSet");
        (fs, true, false)
    };

    // Read uncertainty.
    let uncertainty = if let Some(unc_node) = node.get_object("Unc") {
        Some(region_from_node(unc_node)?)
    } else {
        None
    };

    let mut data = RegionData {
        frameset,
        ident: object.get_string_if_set("Ident"),
        mapping_flags: read_mapping_flags(whole),
        use_defs: object
            .get("UseDfs")
            .map(|_| object.get_int("UseDfs", 1) != 0),
        points,
        negated,
        closed,
        closed_set,
        mesh_size,
        mesh_size_set,
        fill_factor,
        fill_factor_set,
        adaptive,
        uncertainty,
        dummy_fs,
        region_fs,
        cached_base_mesh: RefCell::new(None),
    };

    // If this region has a real frame but its uncertainty region was
    // dumped frame-less (dummy FS), re-associate the uncertainty with a
    // copy of this region's base frame — mirroring C `astSetRegFS`'s
    // recursive uncertainty fix-up (`region.c:10745-10751`).
    if !data.dummy_fs
        && let Some(unc) = data.uncertainty.as_mut()
        && unc.has_dummy_fs()
    {
        let base_frame = data.frameset.base().snapshot();
        unc.set_region_fs(base_frame.as_ref());
    }

    Ok(data)
}

// ---------------------------------------------------------------------------
// Region dispatch
// ---------------------------------------------------------------------------

/// Extension trait enabling `to_node()` on `dyn Region`.
pub(crate) trait RegionToNode {
    /// Serialise this region to a Node tree.
    fn to_node(&self) -> AstResult<Node>;
}

impl RegionToNode for dyn Region {
    fn to_node(&self) -> AstResult<Node> {
        if let Some(r) = self.as_any().downcast_ref::<Circle>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<BoxRegion>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<Ellipse>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<Polygon>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<Interval>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<NullRegion>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<CmpRegion>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<PointList>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<Prism>() {
            return Serializable::to_node(r);
        }
        Err(AstError::new(
            ErrorCode::Lderr,
            "unsupported Region type for serialisation",
        ))
    }
}

/// Reconstruct a concrete Region from a Node tree, dispatching by class name.
pub(crate) fn region_from_node(node: &Node) -> AstResult<DynRegion> {
    match node.class.as_str() {
        "Circle" => Ok(Box::new(Circle::from_node(node)?)),
        "Box" => Ok(Box::new(BoxRegion::from_node(node)?)),
        "Ellipse" => Ok(Box::new(Ellipse::from_node(node)?)),
        "Polygon" => Ok(Box::new(Polygon::from_node(node)?)),
        "Interval" => Ok(Box::new(Interval::from_node(node)?)),
        "NullRegion" => Ok(Box::new(NullRegion::from_node(node)?)),
        "CmpRegion" => Ok(Box::new(CmpRegion::from_node(node)?)),
        "PointList" => Ok(Box::new(PointList::from_node(node)?)),
        "Prism" => Ok(Box::new(Prism::from_node(node)?)),
        other => Err(AstError::new(
            ErrorCode::Lderr,
            format!("unknown Region class: {other}"),
        )),
    }
}

// ---------------------------------------------------------------------------
// Serializable implementations for Region classes
// ---------------------------------------------------------------------------

/// Build the standard Region serialisation skeleton: class header, the
/// inherited Frame attributes (Title / Naxes / ... commented), the
/// `IsA Frame` marker, the Region instance variables, and the
/// `IsA Region` marker. Used by every leaf Region class except `CmpRegion`,
/// which also writes class-specific attributes after `IsA Region`.
fn region_to_node_skeleton(
    class: &'static str,
    class_comment: &'static str,
    data: &RegionData,
) -> AstResult<Node> {
    let mut node = Node::new(class);
    node.class_comment = Some(class_comment.to_string());
    // The Region's own Object part, Ident and any UseDefs stored on the
    // Region itself, comes first (object.c Dump); a UseDefs set through
    // the Region is the encapsulated Frame's and is written with it.
    crate::frames::frame_attrs::write_object_part(&mut node, data.ident.as_deref(), data.use_defs);
    write_flags_mapping_part(&mut node, &data.mapping_flags);
    write_frame_inheritance_for_region(&mut node, data.frameset.current())?;
    push_frame_isa_marker(&mut node);
    write_region_base(&mut node, data);
    push_region_isa_marker(&mut node);
    Ok(node)
}

impl Serializable for Circle {
    fn class_name(&self) -> &'static str {
        "Circle"
    }

    fn to_node(&self) -> AstResult<Node> {
        region_to_node_skeleton("Circle", "Circular or spherical region", &self.data)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let _base = read_mapping_base(node);
        let data = read_region_base(node)?;
        Ok(Circle { data })
    }
}

impl Serializable for BoxRegion {
    fn class_name(&self) -> &'static str {
        "Box"
    }

    fn to_node(&self) -> AstResult<Node> {
        region_to_node_skeleton("Box", "Axis intervals", &self.data)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let _base = read_mapping_base(node);
        let data = read_region_base(node)?;
        Ok(BoxRegion { data })
    }
}

impl Serializable for Ellipse {
    fn class_name(&self) -> &'static str {
        "Ellipse"
    }

    fn to_node(&self) -> AstResult<Node> {
        region_to_node_skeleton("Ellipse", "Elliptical region", &self.data)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let _base = read_mapping_base(node);
        let data = read_region_base(node)?;
        Ok(Ellipse { data })
    }
}

impl Serializable for Polygon {
    fn class_name(&self) -> &'static str {
        "Polygon"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = region_to_node_skeleton("Polygon", "Polygonal region", &self.data)?;
        // Polygon instance variables (polygon.c::Dump, :6692). SimpVertices is
        // written only when set.
        if let Some(sv) = self.simp_vertices {
            node.push_int(
                "SimpVT",
                sv as i64,
                Some("Simplify by transforming vertices?".to_string()),
            );
        }
        // AST always records the vertex-order convention: a non-zero value
        // indicates the STC system (inside to the left when moving
        // anti-clockwise round the vertices as viewed from the inside of the
        // celestial sphere), which is what AST currently uses.
        node.push_int(
            "Order",
            1,
            Some("Polygon uses STC vertex order convention".to_string()),
        );
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("Polygon");
        let _base = read_mapping_base(whole);
        let data = read_region_base(whole)?;
        // polygon.c:7300 reads SimpVT back as a boolean; an absent card leaves
        // the attribute unset.
        let simp_vertices = node.get_int_any_if_set("SimpVT").map(|v| v != 0);
        let mut polygon = Polygon {
            data,
            simp_vertices,
        };
        // A description without a non-zero Order was written by a version of
        // AST that stored the vertices in the old order, which puts the
        // inside on the other side, so the Polygon is negated to match the
        // current (STC) convention; then the vertices are stored so that the
        // unnegated Polygon is the inside (polygon.c astLoadPolygon).
        if node.get_int("Order", 0) == 0 {
            polygon.negate();
        }
        polygon.ensure_inside()?;
        Ok(polygon)
    }
}

impl Serializable for Interval {
    fn class_name(&self) -> &'static str {
        "Interval"
    }

    fn to_node(&self) -> AstResult<Node> {
        region_to_node_skeleton("Interval", "Axis intervals", &self.data)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let _base = read_mapping_base(node);
        let data = read_region_base(node)?;
        Ok(Interval { data })
    }
}

impl Serializable for NullRegion {
    fn class_name(&self) -> &'static str {
        "NullRegion"
    }

    fn to_node(&self) -> AstResult<Node> {
        region_to_node_skeleton("NullRegion", "Boundless region", &self.data)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let _base = read_mapping_base(node);
        let data = read_region_base(node)?;
        Ok(NullRegion { data })
    }
}

impl Serializable for PointList {
    fn class_name(&self) -> &'static str {
        "PointList"
    }

    fn to_node(&self) -> AstResult<Node> {
        region_to_node_skeleton("PointList", "Collection of points", &self.data)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let _base = read_mapping_base(node);
        let data = read_region_base(node)?;
        Ok(PointList { data })
    }
}

impl Serializable for CmpRegion {
    fn class_name(&self) -> &'static str {
        "CmpRegion"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node =
            region_to_node_skeleton("CmpRegion", "Combination of two Regions", &self.data)?;

        // CmpRegion-specific: operator and sub-regions. These follow the
        // `IsA Region` marker — written after the parent class skeleton.
        let (oper, oper_comment) = match self.operator {
            CmpOperator::And => (1, "Regions combined using Boolean AND"),
            CmpOperator::Or => (2, "Regions combined using Boolean OR"),
        };
        node.push_int("Operator", oper, Some(oper_comment.to_string()));

        let node_a = RegionToNode::to_node(self.region1.as_ref())?;
        let node_b = RegionToNode::to_node(self.region2.as_ref())?;
        node.push_object(
            "RegionA",
            node_a,
            Some("First component Region".to_string()),
        );
        node.push_object(
            "RegionB",
            node_b,
            Some("Second component Region".to_string()),
        );

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("CmpRegion");
        let _base = read_mapping_base(whole);
        let data = read_region_base(whole)?;

        let oper_int = node.get_int("Operator", 1);
        let operator = match oper_int {
            2 => CmpOperator::Or,
            _ => CmpOperator::And,
        };

        let node_a = node
            .get_object("RegionA")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "CmpRegion: missing RegionA"))?;
        let node_b = node
            .get_object("RegionB")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "CmpRegion: missing RegionB"))?;

        let mut region1 = region_from_node(node_a)?;
        let mut region2 = region_from_node(node_b)?;

        // Re-associate any dummy-FS component with the CmpRegion's base
        // frame (both components span the same axes). Mirrors C
        // `cmpregion.c` Load (5087-5093) and `SetRegFS` (3510-3513).
        if !data.dummy_fs {
            let base_frame = data.frameset.base().snapshot();
            if region1.has_dummy_fs() {
                region1.set_region_fs(base_frame.as_ref());
            }
            if region2.has_dummy_fs() {
                region2.set_region_fs(base_frame.as_ref());
            }
        }

        Ok(CmpRegion {
            data,
            region1,
            region2,
            operator,
        })
    }
}

impl Serializable for Prism {
    fn class_name(&self) -> &'static str {
        "Prism"
    }

    fn to_node(&self) -> AstResult<Node> {
        // A Region dump does not chain the Mapping-class Dump; the parent
        // class written by C `region.c::Dump` is the Frame (the commented
        // inheritance block), then the Region instance variables. Use the
        // shared skeleton so Prism matches the leaf classes and C — the
        // Mapping base (`Nin`/`IsA Mapping`) is never emitted.
        let mut node = region_to_node_skeleton(
            "Prism",
            "Region extrusion into higher dimensions",
            &self.data,
        )?;

        // Prism-specific: sub-regions, after the `IsA Region` marker.
        let node_a = RegionToNode::to_node(self.region1.as_ref())?;
        let node_b = RegionToNode::to_node(self.region2.as_ref())?;
        node.push_object(
            "RegionA",
            node_a,
            Some("First component Region".to_string()),
        );
        node.push_object(
            "RegionB",
            node_b,
            Some("Second component Region".to_string()),
        );

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("Prism");
        let _base = read_mapping_base(whole);
        let data = read_region_base(whole)?;

        let node_a = node
            .get_object("RegionA")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "Prism: missing RegionA"))?;
        let node_b = node
            .get_object("RegionB")
            .ok_or_else(|| AstError::new(ErrorCode::Lderr, "Prism: missing RegionB"))?;

        let mut region1 = region_from_node(node_a)?;
        let mut region2 = region_from_node(node_b)?;

        // Either component may carry a dummy FrameSet (dumped frame-less).
        // If this Prism itself has a real frame, re-associate each dummy-FS
        // component with the corresponding axes picked from the Prism's
        // base frame — region1 gets axes [0, nax1), region2 gets
        // [nax1, nax1+nax2). Mirrors C `prism.c` Load (4415-4440) and its
        // `SetRegFS` axis-picking (3123-3200).
        if !data.dummy_fs {
            let base_frame = data.frameset.base().snapshot();
            let nax1 = region1.naxes();
            if region1.has_dummy_fs() {
                let axes: Vec<i32> = (0..nax1 as i32).collect();
                let cfrm = base_frame.pick_axes_snapshot(&axes)?;
                region1.set_region_fs(cfrm.as_ref());
            }
            if region2.has_dummy_fs() {
                let nax2 = region2.naxes();
                let axes: Vec<i32> = (0..nax2 as i32).map(|i| nax1 as i32 + i).collect();
                let cfrm = base_frame.pick_axes_snapshot(&axes)?;
                region2.set_region_fs(cfrm.as_ref());
            }
        }

        Ok(Prism {
            data,
            region1,
            region2,
        })
    }
}

// ---------------------------------------------------------------------------
// AnyObject — type-erased AST object for serialisation round-trips
// ---------------------------------------------------------------------------

/// A type-erased AST object for serialisation round-trips.
///
/// Every concrete AST class that can be serialised has a variant here.
/// The convenience methods [`as_mapping`](AnyObject::as_mapping),
/// [`as_frame`](AnyObject::as_frame), [`into_mapping`](AnyObject::into_mapping),
/// and [`into_frame`](AnyObject::into_frame) provide trait-object access
/// without requiring the caller to match on variants.
#[derive(Debug, Clone)]
pub enum AnyObject {
    /// A [`UnitMap`] (identity mapping).
    UnitMap(UnitMap),
    /// A [`ZoomMap`] (scalar zoom mapping).
    ZoomMap(ZoomMap),
    /// A [`ShiftMap`] (per-axis constant shift).
    ShiftMap(ShiftMap),
    /// A [`MatrixMap`] (linear matrix transformation).
    MatrixMap(MatrixMap),
    /// A [`PermMap`] (axis permutation / constant mapping).
    PermMap(PermMap),
    /// A [`WinMap`] (per-axis linear window mapping).
    WinMap(WinMap),
    /// A [`LutMap`] (1-D lookup table mapping).
    LutMap(LutMap),
    /// A [`PolyMap`] (polynomial transformation).
    PolyMap(PolyMap),
    /// A [`ChebyMap`] (Chebyshev polynomial transformation).
    ChebyMap(ChebyMap),
    /// A [`CmpMap`] (compound mapping — series or parallel).
    CmpMap(CmpMap),
    /// A [`WcsMap`] (FITS-WCS sky projection).
    WcsMap(WcsMap),
    /// An [`SlaMap`] (celestial coordinate conversion chain).
    SlaMap(SlaMap),
    /// A [`SpecMap`] (spectral coordinate conversion chain).
    SpecMap(SpecMap),
    /// A [`TimeMap`] (time coordinate conversion chain).
    TimeMap(TimeMap),
    /// A [`PcdMap`] (pincushion/barrel distortion mapping).
    PcdMap(PcdMap),
    /// A [`GrismMap`] (spectral dispersion mapping).
    GrismMap(GrismMap),
    /// A [`SphMap`] (Cartesian to spherical coordinate mapping).
    SphMap(SphMap),
    /// A [`UnitNormMap`] (unit vector + norm decomposition).
    UnitNormMap(UnitNormMap),
    /// An [`XphMap`] (between HEALPix projections).
    XphMap(XphMap),
    /// An [`IntraMap`] (a transformation supplied by the caller).
    IntraMap(IntraMap),
    /// A [`TranMap`] (compound transformation mapping).
    TranMap(TranMap),
    /// A [`RateMap`] (differential mapping — Jacobian element).
    RateMap(RateMap),
    /// A [`MathMap`] (transformation defined by parsed expressions).
    MathMap(MathMap),
    /// A [`SplineMap`] (bivariate B-spline mapping).
    SplineMap(SplineMap),
    /// A [`DssMap`] (DSS plate fit mapping).
    DssMap(DssMap),
    /// A [`NormMap`] (coordinate normalisation mapping).
    NormMap(NormMap),
    /// A [`SelectorMap`] (region-membership classifier mapping).
    SelectorMap(SelectorMap),
    /// A [`SwitchMap`] (selector-dispatched route mapping).
    SwitchMap(SwitchMap),
    /// A [`BasicFrame`] (generic coordinate frame).
    BasicFrame(BasicFrame),
    /// A [`SkyFrame`] (celestial coordinate frame).
    SkyFrame(SkyFrame),
    /// A [`SpecFrame`] (spectral coordinate frame).
    SpecFrame(SpecFrame),
    /// A [`DSBSpecFrame`] (dual-sideband spectral coordinate frame).
    DSBSpecFrame(DSBSpecFrame),
    /// A [`TimeFrame`] (time coordinate frame).
    TimeFrame(TimeFrame),
    /// A [`FluxFrame`] (flux/brightness coordinate frame).
    FluxFrame(FluxFrame),
    /// A [`SpecFluxFrame`] (compound spectrum/flux frame).
    SpecFluxFrame(Box<SpecFluxFrame>),
    /// A [`CmpFrame`] (compound frame).
    CmpFrame(CmpFrame),
    /// A [`FrameSet`] (frame + inter-frame mappings).
    FrameSet(FrameSet),
    /// A [`Circle`] (circular / spherical-cap region).
    Circle(Circle),
    /// A [`BoxRegion`] (axis-aligned box region).
    BoxRegion(BoxRegion),
    /// An [`Ellipse`] (elliptical region).
    Ellipse(Ellipse),
    /// A [`Polygon`] (polygonal region).
    Polygon(Polygon),
    /// An [`Interval`] (axis interval region).
    Interval(Interval),
    /// A [`NullRegion`] (empty / universal region).
    NullRegion(NullRegion),
    /// A [`CmpRegion`] (compound region — AND/OR).
    CmpRegion(CmpRegion),
    /// A [`PointList`] (collection of discrete points).
    PointList(PointList),
    /// A [`Prism`] (extruded region).
    Prism(Prism),
    /// A [`crate::keymap::KeyMap`] (string-keyed heterogeneous map).
    ///
    /// Implements neither [`Mapping`] nor [`Frame`]; [`as_mapping`](AnyObject::as_mapping),
    /// [`as_frame`](AnyObject::as_frame), [`into_mapping`](AnyObject::into_mapping)
    /// and [`into_frame`](AnyObject::into_frame) all give `None` for it.
    KeyMap(Box<crate::keymap::KeyMap>),
}

impl AnyObject {
    /// Wrap a `&dyn Mapping` in its matching variant — the inverse of
    /// [`as_mapping`](AnyObject::as_mapping).
    ///
    /// `None` means the concrete class has no variant here. Callers that
    /// serialise arbitrary Mappings must treat that as a failure rather than
    /// skip the object: a test that renders through this and ignores `None`
    /// stops checking whole classes without saying so.
    ///
    /// Every variant for which `as_mapping` yields `Some` appears below, and
    /// `any_object_from_mapping_covers_as_mapping` in `serial`'s tests asserts
    /// the two lists stay the same length.
    pub fn from_mapping(m: &dyn Mapping) -> Option<Self> {
        if let Some(x) = m.as_any().downcast_ref::<UnitMap>() {
            return Some(AnyObject::UnitMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<ZoomMap>() {
            return Some(AnyObject::ZoomMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<ShiftMap>() {
            return Some(AnyObject::ShiftMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<MatrixMap>() {
            return Some(AnyObject::MatrixMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<PermMap>() {
            return Some(AnyObject::PermMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<WinMap>() {
            return Some(AnyObject::WinMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<LutMap>() {
            return Some(AnyObject::LutMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<PolyMap>() {
            return Some(AnyObject::PolyMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<ChebyMap>() {
            return Some(AnyObject::ChebyMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<CmpMap>() {
            return Some(AnyObject::CmpMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<WcsMap>() {
            return Some(AnyObject::WcsMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<SlaMap>() {
            return Some(AnyObject::SlaMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<SpecMap>() {
            return Some(AnyObject::SpecMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<TimeMap>() {
            return Some(AnyObject::TimeMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<PcdMap>() {
            return Some(AnyObject::PcdMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<GrismMap>() {
            return Some(AnyObject::GrismMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<SphMap>() {
            return Some(AnyObject::SphMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<UnitNormMap>() {
            return Some(AnyObject::UnitNormMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<XphMap>() {
            return Some(AnyObject::XphMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<IntraMap>() {
            return Some(AnyObject::IntraMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<TranMap>() {
            return Some(AnyObject::TranMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<RateMap>() {
            return Some(AnyObject::RateMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<MathMap>() {
            return Some(AnyObject::MathMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<SplineMap>() {
            return Some(AnyObject::SplineMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<DssMap>() {
            return Some(AnyObject::DssMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<NormMap>() {
            return Some(AnyObject::NormMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<SelectorMap>() {
            return Some(AnyObject::SelectorMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<SwitchMap>() {
            return Some(AnyObject::SwitchMap(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<FrameSet>() {
            return Some(AnyObject::FrameSet(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<Circle>() {
            return Some(AnyObject::Circle(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<BoxRegion>() {
            return Some(AnyObject::BoxRegion(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<Ellipse>() {
            return Some(AnyObject::Ellipse(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<Polygon>() {
            return Some(AnyObject::Polygon(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<Interval>() {
            return Some(AnyObject::Interval(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<NullRegion>() {
            return Some(AnyObject::NullRegion(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<CmpRegion>() {
            return Some(AnyObject::CmpRegion(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<PointList>() {
            return Some(AnyObject::PointList(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<Prism>() {
            return Some(AnyObject::Prism(x.clone()));
        }
        None
    }

    /// Return a reference to the contained object as a `&dyn Mapping`,
    /// or `None` if the variant does not implement [`Mapping`].
    pub fn as_mapping(&self) -> Option<&dyn Mapping> {
        match self {
            AnyObject::UnitMap(m) => Some(m),
            AnyObject::ZoomMap(m) => Some(m),
            AnyObject::ShiftMap(m) => Some(m),
            AnyObject::MatrixMap(m) => Some(m),
            AnyObject::PermMap(m) => Some(m),
            AnyObject::WinMap(m) => Some(m),
            AnyObject::LutMap(m) => Some(m),
            AnyObject::PolyMap(m) => Some(m),
            AnyObject::ChebyMap(m) => Some(m),
            AnyObject::CmpMap(m) => Some(m),
            AnyObject::WcsMap(m) => Some(m),
            AnyObject::SlaMap(m) => Some(m),
            AnyObject::SpecMap(m) => Some(m),
            AnyObject::TimeMap(m) => Some(m),
            AnyObject::PcdMap(m) => Some(m),
            AnyObject::GrismMap(m) => Some(m),
            AnyObject::SphMap(m) => Some(m),
            AnyObject::UnitNormMap(m) => Some(m),
            AnyObject::XphMap(m) => Some(m),
            AnyObject::IntraMap(m) => Some(m),
            AnyObject::TranMap(m) => Some(m),
            AnyObject::RateMap(m) => Some(m),
            AnyObject::MathMap(m) => Some(m),
            AnyObject::SplineMap(m) => Some(m),
            AnyObject::DssMap(m) => Some(m),
            AnyObject::NormMap(m) => Some(m),
            AnyObject::SelectorMap(m) => Some(m),
            AnyObject::SwitchMap(m) => Some(m),
            AnyObject::FrameSet(f) => Some(f),
            // Regions implement Mapping.
            AnyObject::Circle(r) => Some(r),
            AnyObject::BoxRegion(r) => Some(r),
            AnyObject::Ellipse(r) => Some(r),
            AnyObject::Polygon(r) => Some(r),
            AnyObject::Interval(r) => Some(r),
            AnyObject::NullRegion(r) => Some(r),
            AnyObject::CmpRegion(r) => Some(r),
            AnyObject::PointList(r) => Some(r),
            AnyObject::Prism(r) => Some(r),
            AnyObject::BasicFrame(_)
            | AnyObject::SkyFrame(_)
            | AnyObject::SpecFrame(_)
            | AnyObject::DSBSpecFrame(_)
            | AnyObject::TimeFrame(_)
            | AnyObject::FluxFrame(_)
            | AnyObject::SpecFluxFrame(_)
            | AnyObject::CmpFrame(_)
            | AnyObject::KeyMap(_) => None,
        }
    }

    /// Return a reference to the contained object as a `&dyn Frame`,
    /// or `None` if the variant does not implement [`Frame`].
    pub fn as_frame(&self) -> Option<&dyn Frame> {
        match self {
            AnyObject::BasicFrame(f) => Some(f),
            AnyObject::SkyFrame(f) => Some(f),
            AnyObject::SpecFrame(f) => Some(f),
            AnyObject::DSBSpecFrame(f) => Some(f),
            AnyObject::TimeFrame(f) => Some(f),
            AnyObject::FluxFrame(f) => Some(f),
            AnyObject::SpecFluxFrame(f) => Some(f.as_ref()),
            AnyObject::CmpFrame(f) => Some(f),
            AnyObject::FrameSet(f) => Some(f),
            // Regions implement Frame.
            AnyObject::Circle(r) => Some(r),
            AnyObject::BoxRegion(r) => Some(r),
            AnyObject::Ellipse(r) => Some(r),
            AnyObject::Polygon(r) => Some(r),
            AnyObject::Interval(r) => Some(r),
            AnyObject::NullRegion(r) => Some(r),
            AnyObject::CmpRegion(r) => Some(r),
            AnyObject::PointList(r) => Some(r),
            AnyObject::Prism(r) => Some(r),
            AnyObject::UnitMap(_)
            | AnyObject::ZoomMap(_)
            | AnyObject::ShiftMap(_)
            | AnyObject::MatrixMap(_)
            | AnyObject::PermMap(_)
            | AnyObject::WinMap(_)
            | AnyObject::LutMap(_)
            | AnyObject::PolyMap(_)
            | AnyObject::ChebyMap(_)
            | AnyObject::CmpMap(_)
            | AnyObject::WcsMap(_)
            | AnyObject::SlaMap(_)
            | AnyObject::SpecMap(_)
            | AnyObject::TimeMap(_)
            | AnyObject::PcdMap(_)
            | AnyObject::GrismMap(_)
            | AnyObject::SphMap(_)
            | AnyObject::UnitNormMap(_)
            | AnyObject::XphMap(_)
            | AnyObject::IntraMap(_)
            | AnyObject::TranMap(_)
            | AnyObject::RateMap(_)
            | AnyObject::MathMap(_)
            | AnyObject::SplineMap(_)
            | AnyObject::DssMap(_)
            | AnyObject::NormMap(_)
            | AnyObject::SelectorMap(_)
            | AnyObject::SwitchMap(_)
            | AnyObject::KeyMap(_) => None,
        }
    }

    /// Consume `self` and return a [`DynMapping`] (`Box<dyn Mapping>`),
    /// or `None` if the variant does not implement [`Mapping`].
    pub fn into_mapping(self) -> Option<DynMapping> {
        match self {
            AnyObject::UnitMap(m) => Some(Box::new(m)),
            AnyObject::ZoomMap(m) => Some(Box::new(m)),
            AnyObject::ShiftMap(m) => Some(Box::new(m)),
            AnyObject::MatrixMap(m) => Some(Box::new(m)),
            AnyObject::PermMap(m) => Some(Box::new(m)),
            AnyObject::WinMap(m) => Some(Box::new(m)),
            AnyObject::LutMap(m) => Some(Box::new(m)),
            AnyObject::PolyMap(m) => Some(Box::new(m)),
            AnyObject::ChebyMap(m) => Some(Box::new(m)),
            AnyObject::CmpMap(m) => Some(Box::new(m)),
            AnyObject::WcsMap(m) => Some(Box::new(m)),
            AnyObject::SlaMap(m) => Some(Box::new(m)),
            AnyObject::SpecMap(m) => Some(Box::new(m)),
            AnyObject::TimeMap(m) => Some(Box::new(m)),
            AnyObject::PcdMap(m) => Some(Box::new(m)),
            AnyObject::GrismMap(m) => Some(Box::new(m)),
            AnyObject::SphMap(m) => Some(Box::new(m)),
            AnyObject::UnitNormMap(m) => Some(Box::new(m)),
            AnyObject::XphMap(m) => Some(Box::new(m)),
            AnyObject::IntraMap(m) => Some(Box::new(m)),
            AnyObject::TranMap(m) => Some(Box::new(m)),
            AnyObject::RateMap(m) => Some(Box::new(m)),
            AnyObject::MathMap(m) => Some(Box::new(m)),
            AnyObject::SplineMap(m) => Some(Box::new(m)),
            AnyObject::DssMap(m) => Some(Box::new(m)),
            AnyObject::NormMap(m) => Some(Box::new(m)),
            AnyObject::SelectorMap(m) => Some(Box::new(m)),
            AnyObject::SwitchMap(m) => Some(Box::new(m)),
            AnyObject::FrameSet(f) => Some(Box::new(f)),
            // Regions implement Mapping.
            AnyObject::Circle(r) => Some(Box::new(r)),
            AnyObject::BoxRegion(r) => Some(Box::new(r)),
            AnyObject::Ellipse(r) => Some(Box::new(r)),
            AnyObject::Polygon(r) => Some(Box::new(r)),
            AnyObject::Interval(r) => Some(Box::new(r)),
            AnyObject::NullRegion(r) => Some(Box::new(r)),
            AnyObject::CmpRegion(r) => Some(Box::new(r)),
            AnyObject::PointList(r) => Some(Box::new(r)),
            AnyObject::Prism(r) => Some(Box::new(r)),
            AnyObject::BasicFrame(_)
            | AnyObject::SkyFrame(_)
            | AnyObject::SpecFrame(_)
            | AnyObject::DSBSpecFrame(_)
            | AnyObject::TimeFrame(_)
            | AnyObject::FluxFrame(_)
            | AnyObject::SpecFluxFrame(_)
            | AnyObject::CmpFrame(_)
            | AnyObject::KeyMap(_) => None,
        }
    }

    /// Consume `self` and return a [`DynFrame`] (`Box<dyn Frame>`),
    /// or `None` if the variant does not implement [`Frame`].
    pub fn into_frame(self) -> Option<DynFrame> {
        match self {
            AnyObject::BasicFrame(f) => Some(Box::new(f)),
            AnyObject::SkyFrame(f) => Some(Box::new(f)),
            AnyObject::SpecFrame(f) => Some(Box::new(f)),
            AnyObject::DSBSpecFrame(f) => Some(Box::new(f)),
            AnyObject::TimeFrame(f) => Some(Box::new(f)),
            AnyObject::FluxFrame(f) => Some(Box::new(f)),
            AnyObject::SpecFluxFrame(f) => Some(f),
            AnyObject::CmpFrame(f) => Some(Box::new(f)),
            AnyObject::FrameSet(f) => Some(Box::new(f)),
            // Regions implement Frame.
            AnyObject::Circle(r) => Some(Box::new(r)),
            AnyObject::BoxRegion(r) => Some(Box::new(r)),
            AnyObject::Ellipse(r) => Some(Box::new(r)),
            AnyObject::Polygon(r) => Some(Box::new(r)),
            AnyObject::Interval(r) => Some(Box::new(r)),
            AnyObject::NullRegion(r) => Some(Box::new(r)),
            AnyObject::CmpRegion(r) => Some(Box::new(r)),
            AnyObject::PointList(r) => Some(Box::new(r)),
            AnyObject::Prism(r) => Some(Box::new(r)),
            AnyObject::UnitMap(_)
            | AnyObject::ZoomMap(_)
            | AnyObject::ShiftMap(_)
            | AnyObject::MatrixMap(_)
            | AnyObject::PermMap(_)
            | AnyObject::WinMap(_)
            | AnyObject::LutMap(_)
            | AnyObject::PolyMap(_)
            | AnyObject::ChebyMap(_)
            | AnyObject::CmpMap(_)
            | AnyObject::WcsMap(_)
            | AnyObject::SlaMap(_)
            | AnyObject::SpecMap(_)
            | AnyObject::TimeMap(_)
            | AnyObject::PcdMap(_)
            | AnyObject::GrismMap(_)
            | AnyObject::SphMap(_)
            | AnyObject::UnitNormMap(_)
            | AnyObject::XphMap(_)
            | AnyObject::IntraMap(_)
            | AnyObject::TranMap(_)
            | AnyObject::RateMap(_)
            | AnyObject::MathMap(_)
            | AnyObject::SplineMap(_)
            | AnyObject::DssMap(_)
            | AnyObject::NormMap(_)
            | AnyObject::SelectorMap(_)
            | AnyObject::SwitchMap(_)
            | AnyObject::KeyMap(_) => None,
        }
    }

    /// Return a reference to the contained object as a `&dyn Region`,
    /// or `None` if the variant does not implement [`Region`].
    pub fn as_region(&self) -> Option<&dyn Region> {
        match self {
            AnyObject::Circle(r) => Some(r),
            AnyObject::BoxRegion(r) => Some(r),
            AnyObject::Ellipse(r) => Some(r),
            AnyObject::Polygon(r) => Some(r),
            AnyObject::Interval(r) => Some(r),
            AnyObject::NullRegion(r) => Some(r),
            AnyObject::CmpRegion(r) => Some(r),
            AnyObject::PointList(r) => Some(r),
            AnyObject::Prism(r) => Some(r),
            _ => None,
        }
    }

    /// Consume `self` and return a [`DynRegion`] (`Box<dyn Region>`),
    /// or `None` if the variant does not implement [`Region`].
    pub fn into_region(self) -> Option<DynRegion> {
        match self {
            AnyObject::Circle(r) => Some(Box::new(r)),
            AnyObject::BoxRegion(r) => Some(Box::new(r)),
            AnyObject::Ellipse(r) => Some(Box::new(r)),
            AnyObject::Polygon(r) => Some(Box::new(r)),
            AnyObject::Interval(r) => Some(Box::new(r)),
            AnyObject::NullRegion(r) => Some(Box::new(r)),
            AnyObject::CmpRegion(r) => Some(Box::new(r)),
            AnyObject::PointList(r) => Some(Box::new(r)),
            AnyObject::Prism(r) => Some(Box::new(r)),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// AnyObject dispatch: Node ↔ AnyObject
// ---------------------------------------------------------------------------

/// Reconstruct an [`AnyObject`] from a [`Node`], dispatching by class name.
pub(crate) fn any_object_from_node(node: &Node) -> AstResult<AnyObject> {
    match node.class.as_str() {
        "UnitMap" => Ok(AnyObject::UnitMap(load_mapping::<UnitMap>(node)?)),
        "ZoomMap" => Ok(AnyObject::ZoomMap(load_mapping::<ZoomMap>(node)?)),
        "ShiftMap" => Ok(AnyObject::ShiftMap(load_mapping::<ShiftMap>(node)?)),
        "MatrixMap" => Ok(AnyObject::MatrixMap(load_mapping::<MatrixMap>(node)?)),
        "PermMap" => Ok(AnyObject::PermMap(load_mapping::<PermMap>(node)?)),
        "WinMap" => Ok(AnyObject::WinMap(load_mapping::<WinMap>(node)?)),
        "LutMap" => Ok(AnyObject::LutMap(load_mapping::<LutMap>(node)?)),
        "PolyMap" => Ok(AnyObject::PolyMap(load_mapping::<PolyMap>(node)?)),
        "ChebyMap" => Ok(AnyObject::ChebyMap(load_mapping::<ChebyMap>(node)?)),
        "CmpMap" => Ok(AnyObject::CmpMap(load_mapping::<CmpMap>(node)?)),
        "WcsMap" => Ok(AnyObject::WcsMap(load_mapping::<WcsMap>(node)?)),
        "SlaMap" => Ok(AnyObject::SlaMap(load_mapping::<SlaMap>(node)?)),
        "SpecMap" => Ok(AnyObject::SpecMap(load_mapping::<SpecMap>(node)?)),
        "TimeMap" => Ok(AnyObject::TimeMap(load_mapping::<TimeMap>(node)?)),
        "PcdMap" => Ok(AnyObject::PcdMap(load_mapping::<PcdMap>(node)?)),
        "GrismMap" => Ok(AnyObject::GrismMap(load_mapping::<GrismMap>(node)?)),
        "SphMap" => Ok(AnyObject::SphMap(load_mapping::<SphMap>(node)?)),
        "UnitNormMap" => Ok(AnyObject::UnitNormMap(load_mapping::<UnitNormMap>(node)?)),
        "XphMap" => Ok(AnyObject::XphMap(load_mapping::<XphMap>(node)?)),
        "IntraMap" => Ok(AnyObject::IntraMap(load_mapping::<IntraMap>(node)?)),
        "TranMap" => Ok(AnyObject::TranMap(load_mapping::<TranMap>(node)?)),
        "RateMap" => Ok(AnyObject::RateMap(load_mapping::<RateMap>(node)?)),
        "MathMap" => Ok(AnyObject::MathMap(load_mapping::<MathMap>(node)?)),
        "SplineMap" => Ok(AnyObject::SplineMap(load_mapping::<SplineMap>(node)?)),
        "DssMap" => Ok(AnyObject::DssMap(load_mapping::<DssMap>(node)?)),
        "NormMap" => Ok(AnyObject::NormMap(load_mapping::<NormMap>(node)?)),
        "SelectorMap" => Ok(AnyObject::SelectorMap(load_mapping::<SelectorMap>(node)?)),
        "SwitchMap" => Ok(AnyObject::SwitchMap(load_mapping::<SwitchMap>(node)?)),
        "Frame" => Ok(AnyObject::BasicFrame(BasicFrame::from_node(node)?)),
        "SkyFrame" => Ok(AnyObject::SkyFrame(SkyFrame::from_node(node)?)),
        "SpecFrame" => Ok(AnyObject::SpecFrame(SpecFrame::from_node(node)?)),
        "DSBSpecFrame" => Ok(AnyObject::DSBSpecFrame(DSBSpecFrame::from_node(node)?)),
        "TimeFrame" => Ok(AnyObject::TimeFrame(TimeFrame::from_node(node)?)),
        "FluxFrame" => Ok(AnyObject::FluxFrame(FluxFrame::from_node(node)?)),
        "SpecFluxFrame" => Ok(AnyObject::SpecFluxFrame(Box::new(
            SpecFluxFrame::from_node(node)?,
        ))),
        "CmpFrame" => Ok(AnyObject::CmpFrame(CmpFrame::from_node(node)?)),
        "FrameSet" => Ok(AnyObject::FrameSet(FrameSet::from_node(node)?)),
        // Regions.
        "Circle" => Ok(AnyObject::Circle(Circle::from_node(node)?)),
        "Box" => Ok(AnyObject::BoxRegion(BoxRegion::from_node(node)?)),
        "Ellipse" => Ok(AnyObject::Ellipse(Ellipse::from_node(node)?)),
        "Polygon" => Ok(AnyObject::Polygon(Polygon::from_node(node)?)),
        "Interval" => Ok(AnyObject::Interval(Interval::from_node(node)?)),
        "NullRegion" => Ok(AnyObject::NullRegion(NullRegion::from_node(node)?)),
        "CmpRegion" => Ok(AnyObject::CmpRegion(CmpRegion::from_node(node)?)),
        "PointList" => Ok(AnyObject::PointList(PointList::from_node(node)?)),
        "Prism" => Ok(AnyObject::Prism(Prism::from_node(node)?)),
        "KeyMap" => Ok(AnyObject::KeyMap(Box::new(
            crate::keymap::KeyMap::from_node(node)?,
        ))),
        other => Err(AstError::new(
            ErrorCode::Lderr,
            format!("unknown AST class: {other}"),
        )),
    }
}

// ---------------------------------------------------------------------------
// AnyObject → Node helper (used by Channel and to_native_string)
// ---------------------------------------------------------------------------

/// Convert an [`AnyObject`] reference to a [`Node`] tree.
pub(crate) fn any_object_to_node(obj: &AnyObject) -> AstResult<Node> {
    match obj {
        AnyObject::UnitMap(m) => Serializable::to_node(m),
        AnyObject::ZoomMap(m) => Serializable::to_node(m),
        AnyObject::ShiftMap(m) => Serializable::to_node(m),
        AnyObject::MatrixMap(m) => Serializable::to_node(m),
        AnyObject::PermMap(m) => Serializable::to_node(m),
        AnyObject::WinMap(m) => Serializable::to_node(m),
        AnyObject::LutMap(m) => Serializable::to_node(m),
        AnyObject::PolyMap(m) => Serializable::to_node(m),
        AnyObject::ChebyMap(m) => Serializable::to_node(m),
        AnyObject::CmpMap(m) => Serializable::to_node(m),
        AnyObject::WcsMap(m) => Serializable::to_node(m),
        AnyObject::SlaMap(m) => Serializable::to_node(m),
        AnyObject::SpecMap(m) => Serializable::to_node(m),
        AnyObject::TimeMap(m) => Serializable::to_node(m),
        AnyObject::PcdMap(m) => Serializable::to_node(m),
        AnyObject::GrismMap(m) => Serializable::to_node(m),
        AnyObject::SphMap(m) => Serializable::to_node(m),
        AnyObject::UnitNormMap(m) => Serializable::to_node(m),
        AnyObject::XphMap(m) => Serializable::to_node(m),
        AnyObject::IntraMap(m) => Serializable::to_node(m),
        AnyObject::TranMap(m) => Serializable::to_node(m),
        AnyObject::RateMap(m) => Serializable::to_node(m),
        AnyObject::MathMap(m) => Serializable::to_node(m),
        AnyObject::SplineMap(m) => Serializable::to_node(m),
        AnyObject::DssMap(m) => Serializable::to_node(m),
        AnyObject::NormMap(m) => Serializable::to_node(m),
        AnyObject::SelectorMap(m) => Serializable::to_node(m),
        AnyObject::SwitchMap(m) => Serializable::to_node(m),
        AnyObject::BasicFrame(f) => Serializable::to_node(f),
        AnyObject::SkyFrame(f) => Serializable::to_node(f),
        AnyObject::SpecFrame(f) => Serializable::to_node(f),
        AnyObject::DSBSpecFrame(f) => Serializable::to_node(f),
        AnyObject::TimeFrame(f) => Serializable::to_node(f),
        AnyObject::FluxFrame(f) => Serializable::to_node(f),
        AnyObject::SpecFluxFrame(f) => Serializable::to_node(f.as_ref()),
        AnyObject::CmpFrame(f) => Serializable::to_node(f),
        AnyObject::FrameSet(f) => Serializable::to_node(f),
        // Regions.
        AnyObject::Circle(r) => Serializable::to_node(r),
        AnyObject::BoxRegion(r) => Serializable::to_node(r),
        AnyObject::Ellipse(r) => Serializable::to_node(r),
        AnyObject::Polygon(r) => Serializable::to_node(r),
        AnyObject::Interval(r) => Serializable::to_node(r),
        AnyObject::NullRegion(r) => Serializable::to_node(r),
        AnyObject::CmpRegion(r) => Serializable::to_node(r),
        AnyObject::PointList(r) => Serializable::to_node(r),
        AnyObject::Prism(r) => Serializable::to_node(r),
        AnyObject::KeyMap(m) => Serializable::to_node(m.as_ref()),
    }
}

// ---------------------------------------------------------------------------
// Public API: to_native_string / from_native_string
// ---------------------------------------------------------------------------

/// Serialise any AST object to Native text format.
///
/// The returned string is in the AST Native serialisation format, compatible
/// with the C library's `astWrite` / `astRead` channel functions.
///
/// This delegates to [`NativeChannel`](crate::channel::NativeChannel) with
/// default configuration (comments enabled, full=0).
pub fn to_native_string(obj: &AnyObject) -> AstResult<String> {
    use crate::channel::{Channel, NativeChannel};
    let mut ch = NativeChannel::with_string_sink();
    ch.write(obj)?;
    Ok(ch.into_string())
}

/// Deserialise an AST object from Native text format.
///
/// Parses the AST Native serialisation format and returns a type-erased
/// [`AnyObject`]. Use [`AnyObject::as_mapping`], [`AnyObject::as_frame`],
/// [`AnyObject::into_mapping`], or [`AnyObject::into_frame`] to extract
/// the concrete trait object.
///
/// This delegates to [`NativeChannel`](crate::channel::NativeChannel), which
/// knows no IntraMap transformations, so text holding an IntraMap fails with
/// AST__URITF; read it with [`from_native_string_with`].
pub fn from_native_string(s: &str) -> AstResult<AnyObject> {
    from_native_string_with(s, &IntraRegistry::new())
}

/// Deserialise an AST object from Native text format, giving any IntraMap in
/// it, however deeply nested, its transformation from `registry`.
///
/// This is [`from_native_string`] reading through a
/// [`NativeChannel`](crate::channel::NativeChannel) that has had each
/// transformation in `registry` registered with
/// [`Channel::register_intra`](crate::channel::Channel::register_intra).
pub fn from_native_string_with(s: &str, registry: &IntraRegistry) -> AstResult<AnyObject> {
    use crate::channel::{Channel, NativeChannel};
    let mut ch = NativeChannel::from_string_with(s, registry.clone());
    ch.read()?
        .ok_or_else(|| AstError::new(ErrorCode::Rderr, "no object found in input"))
}
