//! Serialisation: Node tree, Serializable trait, and Native format.

pub(crate) mod native_read;
pub(crate) mod native_write;
pub(crate) mod node;

pub(crate) use node::{Node, NodeValue};

use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{
    BasicFrame, CmpFrame, DSBSpecFrame, DynFrame, FluxFrame, Frame, FrameSet, SkyFrame,
    SpecFluxFrame, SpecFrame, TimeFrame,
};
use crate::mappings::{
    ChebyMap, CmpMap, DssMap, DynMapping, GrismMap, LutMap, Mapping, MathMap, MatrixMap, NormMap,
    PcdMap, PermMap, PolyMap, RateMap, ShiftMap, SlaMap, SpecMap, SphMap, SplineMap, TimeMap,
    TranMap, UnitMap, UnitNormMap, WcsMap, WinMap, ZoomMap,
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

/// Write common Mapping attributes (Nin, Nout, IsSimp, Invert, IsA Mapping).
pub(crate) fn write_mapping_base(node: &mut Node, mapping: &dyn Mapping) {
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
    if mapping.do_not_simplify() {
        node.push_int("IsSimp", 1, Some("Mapping has been simplified".to_string()));
    }
    if mapping.is_inverted() {
        node.push_int("Invert", 1, Some("Mapping inverted".to_string()));
    }
    if !mapping.has_forward() {
        node.push_int(
            "Fwd",
            0,
            Some("Forward transformation not defined".to_string()),
        );
    }
    if !mapping.has_inverse() {
        node.push_int(
            "Inv",
            0,
            Some("Inverse transformation not defined".to_string()),
        );
    }
    node.push_isa(
        "Mapping",
        Some("Mapping between coordinate systems".to_string()),
    );
}

/// Common Mapping attributes read from a serialized node.
#[derive(Debug, Clone)]
pub(crate) struct MappingBaseAttrs {
    pub nin: usize,
    pub nout: usize,
    pub inverted: bool,
    pub do_not_simplify: bool,
    pub has_forward: bool,
    pub has_inverse: bool,
}

/// Read common Mapping attributes from a node.
pub(crate) fn read_mapping_base(node: &Node) -> MappingBaseAttrs {
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
        do_not_simplify: node.get_int("IsSimp", 0) != 0,
        has_forward: node.get_int("Fwd", 1) != 0,
        has_inverse: node.get_int("Inv", 1) != 0,
    }
}

// ---------------------------------------------------------------------------
// Mapping dispatch
// ---------------------------------------------------------------------------

/// Reconstruct a concrete Mapping from a Node tree, dispatching by class name.
pub(crate) fn mapping_from_node(node: &Node) -> AstResult<DynMapping> {
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
        "TranMap" => Ok(Box::new(TranMap::from_node(node)?)),
        "RateMap" => Ok(Box::new(RateMap::from_node(node)?)),
        "SplineMap" => Ok(Box::new(SplineMap::from_node(node)?)),
        "DssMap" => Ok(Box::new(DssMap::from_node(node)?)),
        "NormMap" => Ok(Box::new(NormMap::from_node(node)?)),
        "MathMap" => Ok(Box::new(MathMap::from_node(node)?)),
        "FrameSet" => Ok(Box::new(FrameSet::from_node(node)?)),
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
        if let Some(f) = self.as_any().downcast_ref::<FrameSet>() {
            return Serializable::to_node(f);
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
        other => Err(AstError::new(
            ErrorCode::Lderr,
            format!("unknown or unsupported Frame class: {other}"),
        )),
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
#[derive(Debug)]
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
    /// A [`TranMap`] (compound transformation mapping).
    TranMap(TranMap),
    /// A [`RateMap`] (differential mapping — Jacobian element).
    RateMap(RateMap),
    /// A [`SplineMap`] (bivariate B-spline mapping).
    SplineMap(SplineMap),
    /// A [`DssMap`] (DSS plate fit mapping).
    DssMap(DssMap),
    /// A [`NormMap`] (coordinate normalisation mapping).
    NormMap(NormMap),
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
    SpecFluxFrame(SpecFluxFrame),
    /// A [`CmpFrame`] (compound frame).
    CmpFrame(CmpFrame),
    /// A [`FrameSet`] (frame + inter-frame mappings).
    FrameSet(FrameSet),
}

impl AnyObject {
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
            AnyObject::TranMap(m) => Some(m),
            AnyObject::RateMap(m) => Some(m),
            AnyObject::SplineMap(m) => Some(m),
            AnyObject::DssMap(m) => Some(m),
            AnyObject::NormMap(m) => Some(m),
            AnyObject::FrameSet(f) => Some(f),
            AnyObject::BasicFrame(_)
            | AnyObject::SkyFrame(_)
            | AnyObject::SpecFrame(_)
            | AnyObject::DSBSpecFrame(_)
            | AnyObject::TimeFrame(_)
            | AnyObject::FluxFrame(_)
            | AnyObject::SpecFluxFrame(_)
            | AnyObject::CmpFrame(_) => None,
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
            AnyObject::SpecFluxFrame(f) => Some(f),
            AnyObject::CmpFrame(f) => Some(f),
            AnyObject::FrameSet(f) => Some(f),
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
            | AnyObject::TranMap(_)
            | AnyObject::RateMap(_)
            | AnyObject::SplineMap(_)
            | AnyObject::DssMap(_)
            | AnyObject::NormMap(_) => None,
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
            AnyObject::TranMap(m) => Some(Box::new(m)),
            AnyObject::RateMap(m) => Some(Box::new(m)),
            AnyObject::SplineMap(m) => Some(Box::new(m)),
            AnyObject::DssMap(m) => Some(Box::new(m)),
            AnyObject::NormMap(m) => Some(Box::new(m)),
            AnyObject::FrameSet(f) => Some(Box::new(f)),
            AnyObject::BasicFrame(_)
            | AnyObject::SkyFrame(_)
            | AnyObject::SpecFrame(_)
            | AnyObject::DSBSpecFrame(_)
            | AnyObject::TimeFrame(_)
            | AnyObject::FluxFrame(_)
            | AnyObject::SpecFluxFrame(_)
            | AnyObject::CmpFrame(_) => None,
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
            AnyObject::SpecFluxFrame(f) => Some(Box::new(f)),
            AnyObject::CmpFrame(f) => Some(Box::new(f)),
            AnyObject::FrameSet(f) => Some(Box::new(f)),
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
            | AnyObject::TranMap(_)
            | AnyObject::RateMap(_)
            | AnyObject::SplineMap(_)
            | AnyObject::DssMap(_)
            | AnyObject::NormMap(_) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// AnyObject dispatch: Node ↔ AnyObject
// ---------------------------------------------------------------------------

/// Reconstruct an [`AnyObject`] from a [`Node`], dispatching by class name.
pub(crate) fn any_object_from_node(node: &Node) -> AstResult<AnyObject> {
    match node.class.as_str() {
        "UnitMap" => Ok(AnyObject::UnitMap(UnitMap::from_node(node)?)),
        "ZoomMap" => Ok(AnyObject::ZoomMap(ZoomMap::from_node(node)?)),
        "ShiftMap" => Ok(AnyObject::ShiftMap(ShiftMap::from_node(node)?)),
        "MatrixMap" => Ok(AnyObject::MatrixMap(MatrixMap::from_node(node)?)),
        "PermMap" => Ok(AnyObject::PermMap(PermMap::from_node(node)?)),
        "WinMap" => Ok(AnyObject::WinMap(WinMap::from_node(node)?)),
        "LutMap" => Ok(AnyObject::LutMap(LutMap::from_node(node)?)),
        "PolyMap" => Ok(AnyObject::PolyMap(PolyMap::from_node(node)?)),
        "ChebyMap" => Ok(AnyObject::ChebyMap(ChebyMap::from_node(node)?)),
        "CmpMap" => Ok(AnyObject::CmpMap(CmpMap::from_node(node)?)),
        "WcsMap" => Ok(AnyObject::WcsMap(WcsMap::from_node(node)?)),
        "SlaMap" => Ok(AnyObject::SlaMap(SlaMap::from_node(node)?)),
        "SpecMap" => Ok(AnyObject::SpecMap(SpecMap::from_node(node)?)),
        "TimeMap" => Ok(AnyObject::TimeMap(TimeMap::from_node(node)?)),
        "PcdMap" => Ok(AnyObject::PcdMap(PcdMap::from_node(node)?)),
        "GrismMap" => Ok(AnyObject::GrismMap(GrismMap::from_node(node)?)),
        "SphMap" => Ok(AnyObject::SphMap(SphMap::from_node(node)?)),
        "UnitNormMap" => Ok(AnyObject::UnitNormMap(UnitNormMap::from_node(node)?)),
        "TranMap" => Ok(AnyObject::TranMap(TranMap::from_node(node)?)),
        "RateMap" => Ok(AnyObject::RateMap(RateMap::from_node(node)?)),
        "SplineMap" => Ok(AnyObject::SplineMap(SplineMap::from_node(node)?)),
        "DssMap" => Ok(AnyObject::DssMap(DssMap::from_node(node)?)),
        "NormMap" => Ok(AnyObject::NormMap(NormMap::from_node(node)?)),
        "Frame" => Ok(AnyObject::BasicFrame(BasicFrame::from_node(node)?)),
        "SkyFrame" => Ok(AnyObject::SkyFrame(SkyFrame::from_node(node)?)),
        "SpecFrame" => Ok(AnyObject::SpecFrame(SpecFrame::from_node(node)?)),
        "DSBSpecFrame" => Ok(AnyObject::DSBSpecFrame(DSBSpecFrame::from_node(node)?)),
        "TimeFrame" => Ok(AnyObject::TimeFrame(TimeFrame::from_node(node)?)),
        "FluxFrame" => Ok(AnyObject::FluxFrame(FluxFrame::from_node(node)?)),
        "SpecFluxFrame" => Ok(AnyObject::SpecFluxFrame(SpecFluxFrame::from_node(node)?)),
        "CmpFrame" => Ok(AnyObject::CmpFrame(CmpFrame::from_node(node)?)),
        "FrameSet" => Ok(AnyObject::FrameSet(FrameSet::from_node(node)?)),
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
        AnyObject::TranMap(m) => Serializable::to_node(m),
        AnyObject::RateMap(m) => Serializable::to_node(m),
        AnyObject::SplineMap(m) => Serializable::to_node(m),
        AnyObject::DssMap(m) => Serializable::to_node(m),
        AnyObject::NormMap(m) => Serializable::to_node(m),
        AnyObject::BasicFrame(f) => Serializable::to_node(f),
        AnyObject::SkyFrame(f) => Serializable::to_node(f),
        AnyObject::SpecFrame(f) => Serializable::to_node(f),
        AnyObject::DSBSpecFrame(f) => Serializable::to_node(f),
        AnyObject::TimeFrame(f) => Serializable::to_node(f),
        AnyObject::FluxFrame(f) => Serializable::to_node(f),
        AnyObject::SpecFluxFrame(f) => Serializable::to_node(f),
        AnyObject::CmpFrame(f) => Serializable::to_node(f),
        AnyObject::FrameSet(f) => Serializable::to_node(f),
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
/// This delegates to [`NativeChannel`](crate::channel::NativeChannel).
pub fn from_native_string(s: &str) -> AstResult<AnyObject> {
    use crate::channel::{Channel, NativeChannel};
    let mut ch = NativeChannel::from_string(s);
    ch.read()?
        .ok_or_else(|| AstError::new(ErrorCode::Rderr, "no object found in input"))
}
