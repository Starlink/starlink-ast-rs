//! Serialisation: Node tree, Serializable trait, and Native format.

pub(crate) mod native_read;
pub(crate) mod native_stream;
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
use crate::regions::data::{RegionData, check_fill_factor};
use crate::regions::{
    BoxRegion, Circle, CmpOperator, CmpRegion, CmpRegionBuilder, DynRegion, Ellipse, Interval, Moc,
    NullRegion, PointList, Polygon, Prism, Region, StcCatalogEntryLocation, StcObsDataLocation,
    StcResourceProfile, StcSearchLocation,
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
/// marker come before Nin (`object.c:1697`). `ID` and `Ident` are written
/// active when set, even blank, and the `IsA Object` marker only when
/// something was written, matching C's `astWriteString` / `astWriteIsA`
/// rules.
pub(crate) fn write_object_base(node: &mut Node, mapping: &dyn Mapping) {
    let mut id = crate::object_id::ObjectId::default();
    if mapping.test_id() {
        id.set(mapping.id());
    }
    let ident = mapping.test_ident().then(|| mapping.ident());
    crate::frames::frame_attrs::write_object_part(node, &id, ident, None, true);
}

/// The values mapping.c `Dump` writes for a Mapping.
#[derive(Clone)]
pub(crate) struct MappingPart {
    /// The forward-direction Nin, and whether it is set: C sets it for a
    /// Mapping, and leaves it zero for a Frame, which takes it from its
    /// axes.
    pub nin: (bool, usize),
    /// The forward-direction Nout, set when it differs from Nin.
    pub nout: (bool, usize),
    /// IsSimp.
    pub simple: bool,
    /// ReSimp.
    pub restricted: bool,
    /// AlSimp.
    pub allow: bool,
    /// Invert.
    pub inverted: bool,
    /// Fwd: set when the Mapping's own flag says the transformation is not
    /// defined (only CmpMap computes one), with the forward-direction value.
    pub fwd: (bool, bool),
    /// Inv, as Fwd.
    pub inv: (bool, bool),
    /// Report, if set.
    pub report: Option<bool>,
}

impl MappingPart {
    /// The Mapping part of a Frame, FrameSet or Region: no Nin or Nout of
    /// its own, never simplified, with the flags it holds and the given
    /// definedness.
    pub(crate) fn of_frame(
        flags: &crate::mappings::MappingFlags,
        nin: usize,
        nout: usize,
        inverted: bool,
        fwd: bool,
        inv: bool,
    ) -> Self {
        Self {
            nin: (false, nin),
            nout: (false, nout),
            simple: false,
            restricted: flags.restricted_simplify,
            allow: flags.allow_simplify,
            inverted,
            fwd: (false, fwd),
            inv: (false, inv),
            report: flags.report,
        }
    }
}

/// Write the Object part and then the Mapping part, as mapping.c `Dump`
/// does.
pub(crate) fn write_mapping_base(node: &mut Node, mapping: &dyn Mapping) {
    write_object_base(node, mapping);
    // C writes the intrinsic (forward-direction) Nin and Nout, the instance
    // variables Invert does not swap.
    let inverted = mapping.is_inverted();
    let (nin, nout) = if inverted {
        (mapping.nout(), mapping.nin())
    } else {
        (mapping.nin(), mapping.nout())
    };
    // C writes Fwd and Inv as set from the Mapping's own flags, which only
    // CmpMap computes, and otherwise gives the class's definedness in the
    // forward direction.
    let (intrinsic_fwd, intrinsic_inv) = mapping.dump_tran_flags();
    let (fwd, inv) = if inverted {
        (mapping.has_inverse(), mapping.has_forward())
    } else {
        (mapping.has_forward(), mapping.has_inverse())
    };
    write_mapping_part(
        node,
        &MappingPart {
            nin: (true, nin),
            nout: (nout != nin, nout),
            simple: mapping.dump_is_simple(),
            restricted: mapping.restricted_simplify(),
            allow: mapping.allow_simplify(),
            inverted,
            fwd: (!intrinsic_fwd, intrinsic_fwd && fwd),
            inv: (!intrinsic_inv, intrinsic_inv && inv),
            report: mapping.report_attr(),
        },
    );
}

/// Write the Mapping part of mapping.c `Dump`: Nin, Nout, IsSimp, ReSimp,
/// AlSimp, Invert, Fwd, Inv and Report, each active when set and otherwise
/// not helpful, then the `IsA Mapping` marker.
pub(crate) fn write_mapping_part(node: &mut Node, part: &MappingPart) {
    node.write_int(
        "Nin",
        part.nin.0,
        false,
        part.nin.1 as i64,
        "Number of input coordinates",
    );
    node.write_int(
        "Nout",
        part.nout.0,
        false,
        part.nout.1 as i64,
        "Number of output coordinates",
    );
    node.write_int(
        "IsSimp",
        part.simple,
        false,
        i64::from(part.simple),
        if part.simple {
            "Mapping has been simplified"
        } else {
            "Mapping has not been simplified"
        },
    );
    write_simplify_flags(node, part.restricted, part.allow);
    node.write_int(
        "Invert",
        part.inverted,
        false,
        i64::from(part.inverted),
        if part.inverted {
            "Mapping inverted"
        } else {
            "Mapping not inverted"
        },
    );
    let (set, fwd) = part.fwd;
    node.write_int(
        "Fwd",
        set,
        false,
        i64::from(fwd),
        if fwd {
            "Forward transformation defined"
        } else {
            "Forward transformation not defined"
        },
    );
    let (set, inv) = part.inv;
    node.write_int(
        "Inv",
        set,
        false,
        i64::from(inv),
        if inv {
            "Inverse transformation defined"
        } else {
            "Inverse transformation not defined"
        },
    );
    write_report(node, part.report);
    node.push_isa(
        "Mapping",
        Some("Mapping between coordinate systems".to_string()),
    );
}

/// The `ReSimp` and `AlSimp` items of mapping.c `Dump`, each set when true.
fn write_simplify_flags(node: &mut Node, restricted: bool, allow: bool) {
    node.write_int(
        "ReSimp",
        restricted,
        false,
        i64::from(restricted),
        if restricted {
            "Simplify restricted to allowed Mappings"
        } else {
            "Simplify uses all Mappings"
        },
    );
    node.write_int(
        "AlSimp",
        allow,
        false,
        i64::from(allow),
        if allow {
            "Will be used by restricted Simplify"
        } else {
            "Will not be used by restricted Simplify"
        },
    );
}

/// The `Report` item, last in the Mapping part of mapping.c `Dump`.
fn write_report(node: &mut Node, report: Option<bool>) {
    let value = report.unwrap_or(false);
    node.write_int(
        "Report",
        report.is_some(),
        false,
        i64::from(value),
        if value {
            "Report coordinate transformations"
        } else {
            "Don't report coordinate transformations"
        },
    );
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
    if let Some(id) = object.get_string_if_set("ID") {
        result.set_id(&id);
    }
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
        "Moc" => Ok(Box::new(Moc::from_node(node)?)),
        "StcResourceProfile" => Ok(Box::new(StcResourceProfile::from_node(node)?)),
        "StcSearchLocation" => Ok(Box::new(StcSearchLocation::from_node(node)?)),
        "StcCatalogEntryLocation" => Ok(Box::new(StcCatalogEntryLocation::from_node(node)?)),
        "StcObsDataLocation" => Ok(Box::new(StcObsDataLocation::from_node(node)?)),
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
        if let Some(r) = self.as_any().downcast_ref::<Moc>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<StcResourceProfile>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<StcSearchLocation>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<StcCatalogEntryLocation>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<StcObsDataLocation>() {
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
        if let Some(r) = self.as_any().downcast_ref::<Moc>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<StcResourceProfile>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<StcSearchLocation>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<StcCatalogEntryLocation>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<StcObsDataLocation>() {
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
        "Moc" => Ok(Box::new(Moc::from_node(node)?)),
        "StcResourceProfile" => Ok(Box::new(StcResourceProfile::from_node(node)?)),
        "StcSearchLocation" => Ok(Box::new(StcSearchLocation::from_node(node)?)),
        "StcCatalogEntryLocation" => Ok(Box::new(StcCatalogEntryLocation::from_node(node)?)),
        "StcObsDataLocation" => Ok(Box::new(StcObsDataLocation::from_node(node)?)),
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
    crate::frames::frame_attrs::write_object_part(
        &mut node,
        &crate::object_id::ObjectId::default(),
        None,
        None,
        true,
    );
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
    // A PointSet holding no values is empty (pointset.c `Dump`).
    let empty = npoint == 0;
    node.write_int(
        "Empty",
        empty,
        false,
        i64::from(empty),
        if empty {
            "PointSet is empty"
        } else {
            "PointSet contains data"
        },
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
pub(crate) fn write_region_base(
    node: &mut Node,
    region: &dyn crate::regions::Region,
    data: &RegionData,
) {
    // region.c `Dump`: each value active when set, otherwise the one the
    // class's getter gives, none of them helpful. Negate is the Region's
    // own value when that is set, and is written as set when true; Adapt is
    // written as set when true or set.
    let negated = if data.negated_set {
        data.negated
    } else {
        region.is_negated()
    };
    node.write_int(
        "Negate",
        negated,
        false,
        i64::from(negated),
        if negated {
            "Region negated"
        } else {
            "Region not negated"
        },
    );
    node.write_double(
        "Fill",
        data.fill_factor_set,
        false,
        region.fill_factor(),
        "Region fill factor",
    );
    node.write_int(
        "MeshSz",
        data.mesh_size_set,
        false,
        region.mesh_size() as i64,
        "No. of points used to represent boundary",
    );
    let closed = region.is_closed();
    node.write_int(
        "Closed",
        data.closed_set,
        false,
        i64::from(closed),
        if closed {
            "Boundary is inside"
        } else {
            "Boundary is outside"
        },
    );
    let adaptive = region.is_adaptive();
    node.write_int(
        "Adapt",
        adaptive || data.adaptive_set,
        false,
        i64::from(adaptive),
        if adaptive {
            "Region adapts to coord sys changes"
        } else {
            "Region does not adapt to coord sys changes"
        },
    );

    // Write the FrameSet, mirroring C `region.c:13099-13112`. If the
    // base→current mapping is a UnitMap, the vertices are the same in both
    // Frames, so dump just the current Frame (as `Frm`) — but only when the
    // `RegionFS` attribute is non-zero. When `region_fs` is false (e.g. an
    // uncertainty sub-region whose current Frame equals its parent's base
    // Frame), the Frame can be inferred from the enclosing context, so C
    // omits both `Frm` and the `RegFS` line entirely. If the mapping is not
    // a UnitMap, dump the whole FrameSet and emit `RegFS` uncommented only
    // when RegionFS was explicitly set to 0.
    // C tests `astRegMapping`, the simplified base->current Mapping.
    let is_unit = data
        .frameset
        .mapping(FrameRef::Base, FrameRef::Current)
        .and_then(|m| <dyn Mapping>::simplify(m.as_ref()))
        .map(|m| m.as_any().is::<UnitMap>())
        .unwrap_or(false);

    let regfs_set = if is_unit {
        if data.region_fs {
            let frame_node = FrameToNode::to_node(data.frameset.current())
                .unwrap_or_else(|_| Node::new("Frame"));
            node.push_object("Frm", frame_node, Some("Coordinate system".to_string()));
        }
        false
    } else {
        let fs_node =
            Serializable::to_node(&data.frameset).unwrap_or_else(|_| Node::new("FrameSet"));
        node.push_object(
            "FrmSet",
            fs_node,
            Some("Original & current coordinate systems".to_string()),
        );
        !data.region_fs
    };
    node.write_int(
        "RegFS",
        regfs_set,
        false,
        i64::from(data.region_fs),
        if data.region_fs {
            "Include Frame in dump"
        } else {
            "Do not include Frame in dump"
        },
    );

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

/// Write the Mapping and Frame parts of a Region's dump: frame.c `Dump` run
/// on the Region, whose getters give the values of its encapsulated
/// `frame`. The Region's own Frame structure holds no attribute values and
/// no axes, so every value is unset, and there are no `Prm` items or Axis
/// objects.
pub(crate) fn write_frame_inheritance_for_region(
    node: &mut Node,
    frame: &dyn Frame,
    mapping: MappingPart,
) -> AstResult<()> {
    let frame_node = FrameToNode::to_node(frame)?;
    let frame_part = frame_node.section("Frame");
    let system_of = |key: &str| match frame_part.get_any(key) {
        Some(NodeValue::String(s)) => s.clone(),
        _ => String::new(),
    };
    crate::frames::frame_attrs::write_frame_cards(
        node,
        &crate::frames::frame_attrs::TraitDump {
            frame,
            attrs: &crate::frames::frame_attrs::FrameAttrs::default(),
            system: (false, system_of("System")),
            align_system: (false, system_of("AlSys")),
            perm: frame.axis_perm(),
            is_frame: false,
            own_naxes: 0,
            mapping: Some(mapping),
        },
    )
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
    // C reads Negate and Adapt with a default of the unset sentinel, so each
    // is set only when the dump has an uncommented line for it
    // (region.c:13530, 13550).
    let is_set = |key: &str| {
        node.items
            .iter()
            .find(|i| i.key == key)
            .is_some_and(|i| !matches!(&i.value, NodeValue::Unset(_)))
    };
    let negated_set = is_set("Negate");
    let adaptive_set = is_set("Adapt");
    // A present `Fill`/`MeshSz` line means the attribute was explicitly set
    // (C emits them only when Test<Attr> is true). Their absence leaves the
    // effective value at the class default.
    let fill_factor_set = node
        .items
        .iter()
        .find(|i| i.key == "Fill")
        .is_some_and(|i| !matches!(&i.value, NodeValue::Unset(_)));
    let fill_factor = node.get_double("Fill", 1.0);
    // C's `Load` passes a read value through `SetFillFactor` and
    // `SetMeshSize` (region.c:13535-13541), so an out-of-range Fill is an
    // error and a MeshSz below 5 is raised to 5.
    if fill_factor_set {
        check_fill_factor(fill_factor)?;
    }
    let mesh_size_set = node
        .items
        .iter()
        .find(|i| i.key == "MeshSz")
        .is_some_and(|i| !matches!(&i.value, NodeValue::Unset(_)));
    let mesh_size = if mesh_size_set {
        node.get_int("MeshSz", 0).max(5) as usize
    } else {
        0
    };
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
    // C loader (region.c `Load`): `Frm` present → RegionFS = 1; otherwise
    // `FrmSet` present → read RegFS (default 1); neither → dummy FrameSet
    // with RegionFS = 0. `nax` is the number of base Frame axes.
    let (frameset, dummy_fs, region_fs, nax) = if let Some(frm_node) = node.get_object("Frm") {
        // Only a single Frame stored — create a 2-frame FrameSet with
        // UnitMap connecting base to current (matches the C library and
        // keeps with_integrity working).
        let frame = frame_from_node(frm_node)?;
        let naxes = frame.naxes();
        let current_snap = frame.snapshot();
        let mut fs = FrameSet::new(frame);
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(naxes)), current_snap)
            .expect("adding current frame to region FrameSet");
        (fs, false, true, naxes)
    } else if let Some(fs_node) = node.get_object("FrmSet") {
        let region_fs = node.get_int("RegFS", 1) != 0;
        let fs = FrameSet::from_node(fs_node)?;
        let nin = fs.base().naxes();
        (fs, false, region_fs, nin)
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
        (fs, true, false, nax)
    };

    // The points must have one value per base Frame axis (region.c `Load`).
    if !points.is_empty() && naxpt != nax {
        let class = &whole.class;
        return Err(AstError::new(
            ErrorCode::Regin,
            format!(
                "astLoadRegion({class}): Corrupt {class} contains  incorrect number of \
                 coordinate values per point ({naxpt})."
            ),
        )
        .with_context(format!(
            "The {class} requires {nax} coordinate value(s) for each point."
        )));
    }

    // Read uncertainty.
    let uncertainty = if let Some(unc_node) = node.get_object("Unc") {
        Some(region_from_node(unc_node)?)
    } else {
        None
    };

    let mut data = RegionData {
        frameset,
        ident: object.get_string_if_set("Ident"),
        id: crate::object_id::ObjectId::from_object_part(object),
        mapping_flags: read_mapping_flags(whole),
        use_defs: object
            .get("UseDfs")
            .map(|_| object.get_int("UseDfs", 1) != 0),
        points,
        negated,
        negated_set,
        closed,
        closed_set,
        mesh_size,
        mesh_size_set,
        fill_factor,
        fill_factor_set,
        adaptive,
        adaptive_set,
        uncertainty,
        dummy_fs,
        region_fs,
        // Load always sets RegionFS: to 1 with a Frame, to the value read
        // with a FrameSet, and to 0 with neither (region.c:13589-13620).
        region_fs_set: true,
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
        if let Some(r) = self.as_any().downcast_ref::<Moc>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<StcResourceProfile>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<StcSearchLocation>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<StcCatalogEntryLocation>() {
            return Serializable::to_node(r);
        }
        if let Some(r) = self.as_any().downcast_ref::<StcObsDataLocation>() {
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
        "Moc" => Ok(Box::new(Moc::from_node(node)?)),
        "StcResourceProfile" => Ok(Box::new(StcResourceProfile::from_node(node)?)),
        "StcSearchLocation" => Ok(Box::new(StcSearchLocation::from_node(node)?)),
        "StcCatalogEntryLocation" => Ok(Box::new(StcCatalogEntryLocation::from_node(node)?)),
        "StcObsDataLocation" => Ok(Box::new(StcObsDataLocation::from_node(node)?)),
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
    region: &dyn crate::regions::Region,
    data: &RegionData,
) -> AstResult<Node> {
    let mut node = Node::new(class);
    node.class_comment = Some(class_comment.to_string());
    // The Region's own Object part, Ident and any UseDefs stored on the
    // Region itself, comes first (object.c Dump); a UseDefs set through
    // the Region is the encapsulated Frame's and is written with it.
    crate::frames::frame_attrs::write_object_part(
        &mut node,
        &data.id,
        data.ident.as_deref(),
        data.use_defs,
        true,
    );
    let naxes = data.frameset.current().naxes();
    write_frame_inheritance_for_region(
        &mut node,
        data.frameset.current(),
        MappingPart::of_frame(&data.mapping_flags, naxes, naxes, false, true, true),
    )?;
    push_frame_isa_marker(&mut node);
    write_region_base(&mut node, region, data);
    push_region_isa_marker(&mut node);
    Ok(node)
}

impl Serializable for Circle {
    fn class_name(&self) -> &'static str {
        "Circle"
    }

    fn to_node(&self) -> AstResult<Node> {
        region_to_node_skeleton("Circle", "Circular or spherical region", self, &self.data)
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
        region_to_node_skeleton("Box", "Axis intervals", self, &self.data)
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
        region_to_node_skeleton("Ellipse", "Elliptical region", self, &self.data)
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
        let mut node = region_to_node_skeleton("Polygon", "Polygonal region", self, &self.data)?;
        // Polygon instance variables (polygon.c::Dump, :6692): SimpVertices,
        // active when set and otherwise its default.
        node.write_int(
            "SimpVT",
            self.simp_vertices.is_some(),
            false,
            i64::from(self.simp_vertices()),
            "Simplify by transforming vertices?",
        );
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
        region_to_node_skeleton("Interval", "Axis intervals", self, &self.data)
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
        region_to_node_skeleton("NullRegion", "Boundless region", self, &self.data)
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
        region_to_node_skeleton("PointList", "Collection of points", self, &self.data)
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
            region_to_node_skeleton("CmpRegion", "Combination of two Regions", self, &self.data)?;

        // CmpRegion-specific: operator and sub-regions. These follow the
        // `IsA Region` marker — written after the parent class skeleton.
        // A CmpRegion made with XOR, or equivalent to an XOR, is written as
        // the two Regions XORed, as cmpregion.c `Dump` writes `xor1` and
        // `xor2` after `XORCheck`.
        let (oper, oper_comment) = match self.operator() {
            CmpOperator::And => (1, "Regions combined using Boolean AND"),
            CmpOperator::Or => (2, "Regions combined using Boolean OR"),
            CmpOperator::Xor => (3, "Regions combined using Boolean XOR"),
        };
        node.push_int("Operator", oper, Some(oper_comment.to_string()));

        let xor = self.xor_regions();
        let (reg_a, reg_b) = match &xor {
            Some((a, b)) => (a.as_ref(), b.as_ref()),
            None => (self.region1.as_ref(), self.region2.as_ref()),
        };
        let node_a = RegionToNode::to_node(reg_a)?;
        let node_b = RegionToNode::to_node(reg_b)?;
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

        // An XOR is rebuilt from the two Regions XORed, as the OR of each
        // ANDed with the negation of the other (cmpregion.c Load).
        let (region1, region2, operator, xor) = match oper_int {
            3 => {
                let and = |a: DynRegion, b: DynRegion| -> AstResult<DynRegion> {
                    Ok(Box::new(
                        CmpRegionBuilder::from_dyn(a, b, CmpOperator::And)?.build()?,
                    ))
                };
                let new1 = and(region1.negated_copy(), region2.clone_region())?;
                let new2 = and(region1.clone_region(), region2.negated_copy())?;
                (
                    new1,
                    new2,
                    crate::regions::CmpOper::Or,
                    Some((region1, region2)),
                )
            }
            2 => (region1, region2, crate::regions::CmpOper::Or, None),
            _ => (region1, region2, crate::regions::CmpOper::And, None),
        };

        Ok(CmpRegion {
            data,
            region1,
            region2,
            operator,
            xor,
        })
    }
}

impl<K: crate::regions::StcKind> Serializable for crate::regions::Stc<K> {
    fn class_name(&self) -> &'static str {
        K::CLASS
    }

    /// stc.c `Dump` after region.c's: the encapsulated Region, the number of
    /// AstroCoords elements when there are any, and each element's KeyMap.
    fn to_node(&self) -> AstResult<Node> {
        let mut node = region_to_node_skeleton(K::CLASS, K::COMMENT, self, &self.data)?;
        node.push_object(
            "Region",
            RegionToNode::to_node(self.region.as_ref())?,
            Some("STC Region".to_string()),
        );
        node.write_int(
            "Ncoord",
            !self.coords.is_empty(),
            false,
            self.coords.len() as i64,
            "Number of AstroCoords elements",
        );
        for (i, coord) in self.coords.iter().enumerate() {
            node.push_object(
                &format!("Coord{}", i + 1),
                Serializable::to_node(coord)?,
                Some(format!("AstroCoords number {}", i + 1)),
            );
        }
        node.push_isa("Stc", Some("An IVOA Space-Time-Coords object".to_string()));
        // stcobsdatalocation.c `Dump`: the observatory position, where there
        // is one, which only an StcObsDataLocation can hold.
        if let Some(obs) = &self.obs {
            node.push_object(
                "ObsLoc",
                Serializable::to_node(obs)?,
                Some("Observatory position".to_string()),
            );
        }
        Ok(node)
    }

    /// stc.c `astLoadStc`: a Region with no Frame of its own, the
    /// encapsulated Region or one in an AstroCoords element, is given the
    /// Stc's base Frame unless the Stc itself has none yet; an AstroCoords
    /// Region given it is stored again with its comment.
    fn from_node(node: &Node) -> AstResult<Self> {
        let whole = node;
        let node = &whole.section("Stc");
        let _base = read_mapping_base(whole);
        let data = read_region_base(whole)?;

        let region_node = node.get_object("Region").ok_or_else(|| {
            AstError::new(ErrorCode::Lderr, format!("{}: missing Region", K::CLASS))
        })?;
        let mut region = region_from_node(region_node)?;
        let f1 = data.frameset.base().snapshot();
        if !data.dummy_fs && region.has_dummy_fs() {
            region.set_region_fs(f1.as_ref());
        }

        let ncoord = node.get_int("Ncoord", 0).max(0) as usize;
        let mut coords = Vec::with_capacity(ncoord);
        for ico in 1..=ncoord {
            let key = format!("Coord{ico}");
            let km_node = node.get_object(&key).ok_or_else(|| {
                AstError::new(ErrorCode::Lderr, format!("{}: missing {key}", K::CLASS))
            })?;
            let mut keymap = <crate::keymap::KeyMap as Serializable>::from_node(km_node)?;
            if !data.dummy_fs {
                crate::regions::stc_set_coord_frames(&mut keymap, f1.as_ref())?;
            }
            coords.push(keymap);
        }
        // stcobsdatalocation.c `astLoadStcObsDataLocation`: the observatory
        // position, a PointList, read as optional.
        let obs = match whole.section(K::CLASS).get_object("ObsLoc") {
            Some(obs_node) => Some(PointList::from_node(obs_node)?),
            None => None,
        };
        Ok(crate::regions::Stc::from_parts(data, region, coords, obs))
    }
}

impl Serializable for Moc {
    fn class_name(&self) -> &'static str {
        "Moc"
    }

    /// moc.c `Dump`: MaxOrder and MinOrder where set, the number of ranges,
    /// and each range's bounds, each 64 bit value as its low and high 32 bit
    /// halves (`I` and `J` for the lower bound, `K` and `L` for the upper),
    /// each written only when non-zero, and the upper bound only when it
    /// differs from the lower.
    fn to_node(&self) -> AstResult<Node> {
        let mut node =
            region_to_node_skeleton("Moc", "IVOA Multi-Order Coverage map", self, &self.data)?;
        node.write_int(
            "MaxOrder",
            self.test_max_order(),
            false,
            i64::from(self.max_order()),
            "Maximum HEALPix order",
        );
        node.write_int(
            "MinOrder",
            self.test_min_order(),
            false,
            i64::from(self.min_order()),
            "Minimum HEALPix order",
        );
        let nrange = self.range.len() / 2;
        node.push_int(
            "NRange",
            nrange as i64,
            Some("Number of ranges".to_string()),
        );
        let halves = |v: i64| (v as u64 as u32 as i32, ((v as u64) >> 32) as u32 as i32);
        for (irange, r) in self.range.chunks(2).enumerate() {
            let (lo, hi) = halves(r[0]);
            if lo != 0 {
                node.push_int(&format!("I{irange}"), i64::from(lo), None);
            }
            if hi != 0 {
                node.push_int(&format!("J{irange}"), i64::from(hi), None);
            }
            if r[1] != r[0] {
                let (lo, hi) = halves(r[1]);
                if lo != 0 {
                    node.push_int(&format!("K{irange}"), i64::from(lo), None);
                }
                if hi != 0 {
                    node.push_int(&format!("L{irange}"), i64::from(hi), None);
                }
            }
        }
        Ok(node)
    }

    /// moc.c `astLoadMoc`: MaxOrder through its setter, MinOrder clamped,
    /// and the ranges, each an increasing pair of indices below the number
    /// of cells at MaxOrder and above the previous range (AST__LDERR
    /// otherwise).
    fn from_node(node: &Node) -> AstResult<Self> {
        let whole = node;
        let node = &whole.section("Moc");
        let _base = read_mapping_base(whole);
        let data = read_region_base(whole)?;
        let is_set = |key: &str| {
            node.items.iter().any(|i| {
                i.key.eq_ignore_ascii_case(key) && !matches!(&i.value, NodeValue::Unset(_))
            })
        };
        let mut moc = Moc::from_parts(data, None, None, Vec::new());
        if is_set("MaxOrder") {
            moc.store_max_order(node.get_int("MaxOrder", 0) as i32)?;
        }
        if is_set("MinOrder") {
            moc.set_min_order(node.get_int("MinOrder", 0) as i32);
        }
        let max_nest = if moc.test_max_order() {
            12 * (1_i64 << (2 * moc.max_order()))
        } else {
            i64::MAX
        };
        let join = |lo: i64, hi: i64| ((hi as u32 as u64) << 32 | (lo as u32 as u64)) as i64;
        let ill_formed = |detail: String| {
            AstError::new(ErrorCode::Lderr, "astLoadMoc(Moc): Ill-formed Moc.").with_context(detail)
        };
        let nrange = node.get_int("NRange", 0).max(0) as usize;
        let mut range = Vec::with_capacity(2 * nrange);
        let (mut llast, mut ulast) = (0_i64, 0_i64);
        for irange in 0..nrange {
            let lo = join(
                node.get_int(&format!("I{irange}"), 0),
                node.get_int(&format!("J{irange}"), 0),
            );
            let hi = join(
                node.get_int(&format!("K{irange}"), 0),
                node.get_int(&format!("L{irange}"), 0),
            );
            let hi = if hi > 0 { hi } else { lo };
            if hi >= max_nest {
                return Err(ill_formed(format!(
                    "Upper bound ({hi}) is too high for order {}.",
                    moc.max_order()
                )));
            } else if lo > hi {
                return Err(ill_formed(format!(
                    "Upper bound ({hi}) lower than lower bound ({lo})."
                )));
            } else if lo <= ulast && irange != 0 {
                return Err(ill_formed(format!(
                    "Range [{lo}:{hi}] overlaps range [{llast}:{ulast}]."
                )));
            }
            range.extend([lo, hi]);
            llast = lo;
            ulast = hi;
        }
        moc.range = range;
        Ok(moc)
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
            self,
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
    /// A [`Moc`] (HEALPix multi-order coverage).
    Moc(Moc),
    /// A [`StcResourceProfile`] (STC resource coverage).
    StcResourceProfile(StcResourceProfile),
    /// A [`StcSearchLocation`] (STC search location).
    StcSearchLocation(StcSearchLocation),
    /// A [`StcCatalogEntryLocation`] (STC catalogue entry location).
    StcCatalogEntryLocation(StcCatalogEntryLocation),
    /// A [`StcObsDataLocation`] (STC observation location).
    StcObsDataLocation(StcObsDataLocation),
    /// A [`crate::keymap::KeyMap`] (string-keyed heterogeneous map).
    ///
    /// Implements neither [`Mapping`] nor [`Frame`]; [`as_mapping`](AnyObject::as_mapping),
    /// [`as_frame`](AnyObject::as_frame), [`into_mapping`](AnyObject::into_mapping)
    /// and [`into_frame`](AnyObject::into_frame) all give `None` for it.
    KeyMap(Box<crate::keymap::KeyMap>),
    /// A [`crate::table::Table`], a two-dimensional table of values held as
    /// a KeyMap. Implements neither [`Mapping`] nor [`Frame`].
    Table(Box<crate::table::Table>),
    /// A [`crate::fitstable::FitsTable`], a FITS binary table held as a
    /// Table and a FitsChan header. Implements neither [`Mapping`] nor
    /// [`Frame`].
    FitsTable(Box<crate::fitstable::FitsTable>),
    /// A [`crate::channel::NativeChannel`], C's `Channel`. Implements neither
    /// [`Mapping`] nor [`Frame`]. It shares its source and sink with the
    /// channel it was cloned from; one read from a dump has neither.
    NativeChannel(Box<crate::channel::NativeChannel>),
    /// A [`crate::channel::MocChan`]. Implements neither [`Mapping`] nor
    /// [`Frame`]; its source and sink are shared as a `NativeChannel`'s are.
    MocChan(Box<crate::channel::MocChan>),
    /// A [`crate::fitschan::FitsChan`]. Implements neither [`Mapping`] nor
    /// [`Frame`]. A clone has no source or sink, as C's `astCopy` gives
    /// none, and nor has one read from a dump.
    FitsChan(Box<crate::fitschan::FitsChan>),
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
        if let Some(x) = m.as_any().downcast_ref::<Moc>() {
            return Some(AnyObject::Moc(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<StcResourceProfile>() {
            return Some(AnyObject::StcResourceProfile(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<StcSearchLocation>() {
            return Some(AnyObject::StcSearchLocation(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<StcCatalogEntryLocation>() {
            return Some(AnyObject::StcCatalogEntryLocation(x.clone()));
        }
        if let Some(x) = m.as_any().downcast_ref::<StcObsDataLocation>() {
            return Some(AnyObject::StcObsDataLocation(x.clone()));
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
            AnyObject::Moc(r) => Some(r),
            AnyObject::StcResourceProfile(r) => Some(r),
            AnyObject::StcSearchLocation(r) => Some(r),
            AnyObject::StcCatalogEntryLocation(r) => Some(r),
            AnyObject::StcObsDataLocation(r) => Some(r),
            AnyObject::BasicFrame(_)
            | AnyObject::SkyFrame(_)
            | AnyObject::SpecFrame(_)
            | AnyObject::DSBSpecFrame(_)
            | AnyObject::TimeFrame(_)
            | AnyObject::FluxFrame(_)
            | AnyObject::SpecFluxFrame(_)
            | AnyObject::CmpFrame(_)
            | AnyObject::KeyMap(_)
            | AnyObject::Table(_)
            | AnyObject::FitsTable(_)
            | AnyObject::NativeChannel(_)
            | AnyObject::MocChan(_)
            | AnyObject::FitsChan(_) => None,
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
            AnyObject::Moc(r) => Some(r),
            AnyObject::StcResourceProfile(r) => Some(r),
            AnyObject::StcSearchLocation(r) => Some(r),
            AnyObject::StcCatalogEntryLocation(r) => Some(r),
            AnyObject::StcObsDataLocation(r) => Some(r),
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
            | AnyObject::KeyMap(_)
            | AnyObject::Table(_)
            | AnyObject::FitsTable(_)
            | AnyObject::NativeChannel(_)
            | AnyObject::MocChan(_)
            | AnyObject::FitsChan(_) => None,
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
            AnyObject::Moc(r) => Some(Box::new(r)),
            AnyObject::StcResourceProfile(r) => Some(Box::new(r)),
            AnyObject::StcSearchLocation(r) => Some(Box::new(r)),
            AnyObject::StcCatalogEntryLocation(r) => Some(Box::new(r)),
            AnyObject::StcObsDataLocation(r) => Some(Box::new(r)),
            AnyObject::BasicFrame(_)
            | AnyObject::SkyFrame(_)
            | AnyObject::SpecFrame(_)
            | AnyObject::DSBSpecFrame(_)
            | AnyObject::TimeFrame(_)
            | AnyObject::FluxFrame(_)
            | AnyObject::SpecFluxFrame(_)
            | AnyObject::CmpFrame(_)
            | AnyObject::KeyMap(_)
            | AnyObject::Table(_)
            | AnyObject::FitsTable(_)
            | AnyObject::NativeChannel(_)
            | AnyObject::MocChan(_)
            | AnyObject::FitsChan(_) => None,
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
            AnyObject::Moc(r) => Some(Box::new(r)),
            AnyObject::StcResourceProfile(r) => Some(Box::new(r)),
            AnyObject::StcSearchLocation(r) => Some(Box::new(r)),
            AnyObject::StcCatalogEntryLocation(r) => Some(Box::new(r)),
            AnyObject::StcObsDataLocation(r) => Some(Box::new(r)),
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
            | AnyObject::KeyMap(_)
            | AnyObject::Table(_)
            | AnyObject::FitsTable(_)
            | AnyObject::NativeChannel(_)
            | AnyObject::MocChan(_)
            | AnyObject::FitsChan(_) => None,
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
            AnyObject::Moc(r) => Some(r),
            AnyObject::StcResourceProfile(r) => Some(r),
            AnyObject::StcSearchLocation(r) => Some(r),
            AnyObject::StcCatalogEntryLocation(r) => Some(r),
            AnyObject::StcObsDataLocation(r) => Some(r),
            _ => None,
        }
    }

    /// The contained object as a `&mut dyn Region`, or `None` if the variant
    /// does not implement [`Region`].
    pub(crate) fn as_region_mut(&mut self) -> Option<&mut dyn Region> {
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
            AnyObject::Moc(r) => Some(r),
            AnyObject::StcResourceProfile(r) => Some(r),
            AnyObject::StcSearchLocation(r) => Some(r),
            AnyObject::StcCatalogEntryLocation(r) => Some(r),
            AnyObject::StcObsDataLocation(r) => Some(r),
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
            AnyObject::Moc(r) => Some(Box::new(r)),
            AnyObject::StcResourceProfile(r) => Some(Box::new(r)),
            AnyObject::StcSearchLocation(r) => Some(Box::new(r)),
            AnyObject::StcCatalogEntryLocation(r) => Some(Box::new(r)),
            AnyObject::StcObsDataLocation(r) => Some(Box::new(r)),
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
        "Moc" => Ok(AnyObject::Moc(Moc::from_node(node)?)),
        "StcResourceProfile" => Ok(AnyObject::StcResourceProfile(
            StcResourceProfile::from_node(node)?,
        )),
        "StcSearchLocation" => Ok(AnyObject::StcSearchLocation(StcSearchLocation::from_node(
            node,
        )?)),
        "StcCatalogEntryLocation" => Ok(AnyObject::StcCatalogEntryLocation(
            StcCatalogEntryLocation::from_node(node)?,
        )),
        "StcObsDataLocation" => Ok(AnyObject::StcObsDataLocation(
            StcObsDataLocation::from_node(node)?,
        )),
        "KeyMap" => Ok(AnyObject::KeyMap(Box::new(
            crate::keymap::KeyMap::from_node(node)?,
        ))),
        "Table" => Ok(AnyObject::Table(Box::new(crate::table::Table::from_node(
            node,
        )?))),
        "FitsTable" => Ok(AnyObject::FitsTable(Box::new(
            crate::fitstable::FitsTable::from_node(node)?,
        ))),
        "Channel" => Ok(AnyObject::NativeChannel(Box::new(
            crate::channel::NativeChannel::from_node(node)?,
        ))),
        "MocChan" => Ok(AnyObject::MocChan(Box::new(
            crate::channel::MocChan::from_node(node)?,
        ))),
        "FitsChan" => Ok(AnyObject::FitsChan(Box::new(
            crate::fitschan::FitsChan::from_node(node)?,
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
        AnyObject::Moc(r) => Serializable::to_node(r),
        AnyObject::StcResourceProfile(r) => Serializable::to_node(r),
        AnyObject::StcSearchLocation(r) => Serializable::to_node(r),
        AnyObject::StcCatalogEntryLocation(r) => Serializable::to_node(r),
        AnyObject::StcObsDataLocation(r) => Serializable::to_node(r),
        AnyObject::KeyMap(m) => Serializable::to_node(m.as_ref()),
        AnyObject::Table(t) => Serializable::to_node(t.as_ref()),
        AnyObject::FitsTable(t) => Serializable::to_node(t.as_ref()),
        AnyObject::NativeChannel(c) => Serializable::to_node(c.as_ref()),
        AnyObject::MocChan(c) => Serializable::to_node(c.as_ref()),
        AnyObject::FitsChan(c) => Serializable::to_node(c.as_ref()),
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
