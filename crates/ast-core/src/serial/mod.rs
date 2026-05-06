//! Serialisation: Node tree, Serializable trait, and Native format.

pub(crate) mod native_read;
pub(crate) mod native_write;
pub(crate) mod node;

pub(crate) use node::{Node, NodeValue};

use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{BasicFrame, CmpFrame, DynFrame, Frame, FrameSet, SkyFrame};
use crate::mappings::{
    ChebyMap, CmpMap, DynMapping, LutMap, Mapping, MatrixMap, PermMap, PolyMap, ShiftMap, SlaMap,
    SpecMap, SphMap, TimeMap, UnitMap, WcsMap, WinMap, ZoomMap,
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

/// Write common Mapping attributes (Nin, Nout, Invert, IsA Mapping).
pub(crate) fn write_mapping_base(node: &mut Node, mapping: &dyn Mapping) {
    let nin = mapping.nin();
    let nout = mapping.nout();
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
    if mapping.is_inverted() {
        node.push_int("Invert", 1, Some("Mapping inverted".to_string()));
    }
    node.push_isa(
        "Mapping",
        Some("Mapping between coordinate systems".to_string()),
    );
}

/// Read common Mapping attributes from a node.
///
/// Returns `(nin, nout, inverted)`.
pub(crate) fn read_mapping_base(node: &Node) -> (usize, usize, bool) {
    let nin = node.get_int("Nin", 0) as usize;
    let nout_raw = node.get_int("Nout", 0);
    let nout = if nout_raw <= 0 {
        nin
    } else {
        nout_raw as usize
    };
    let inverted = node.get_int("Invert", 0) != 0;
    (nin, nout, inverted)
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
        "SphMap" => Ok(Box::new(SphMap::from_node(node)?)),
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
        if let Some(m) = self.as_any().downcast_ref::<SphMap>() {
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
    /// A [`SphMap`] (Cartesian to spherical coordinate mapping).
    SphMap(SphMap),
    /// A [`BasicFrame`] (generic coordinate frame).
    BasicFrame(BasicFrame),
    /// A [`SkyFrame`] (celestial coordinate frame).
    SkyFrame(SkyFrame),
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
            AnyObject::SphMap(m) => Some(m),
            AnyObject::FrameSet(f) => Some(f),
            AnyObject::BasicFrame(_) | AnyObject::SkyFrame(_) | AnyObject::CmpFrame(_) => None,
        }
    }

    /// Return a reference to the contained object as a `&dyn Frame`,
    /// or `None` if the variant does not implement [`Frame`].
    pub fn as_frame(&self) -> Option<&dyn Frame> {
        match self {
            AnyObject::BasicFrame(f) => Some(f),
            AnyObject::SkyFrame(f) => Some(f),
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
            | AnyObject::SphMap(_) => None,
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
            AnyObject::SphMap(m) => Some(Box::new(m)),
            AnyObject::FrameSet(f) => Some(Box::new(f)),
            AnyObject::BasicFrame(_) | AnyObject::SkyFrame(_) | AnyObject::CmpFrame(_) => None,
        }
    }

    /// Consume `self` and return a [`DynFrame`] (`Box<dyn Frame>`),
    /// or `None` if the variant does not implement [`Frame`].
    pub fn into_frame(self) -> Option<DynFrame> {
        match self {
            AnyObject::BasicFrame(f) => Some(Box::new(f)),
            AnyObject::SkyFrame(f) => Some(Box::new(f)),
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
            | AnyObject::SphMap(_) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// AnyObject dispatch: Node ↔ AnyObject
// ---------------------------------------------------------------------------

/// Reconstruct an [`AnyObject`] from a [`Node`], dispatching by class name.
fn any_object_from_node(node: &Node) -> AstResult<AnyObject> {
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
        "SphMap" => Ok(AnyObject::SphMap(SphMap::from_node(node)?)),
        "Frame" => Ok(AnyObject::BasicFrame(BasicFrame::from_node(node)?)),
        "SkyFrame" => Ok(AnyObject::SkyFrame(SkyFrame::from_node(node)?)),
        "CmpFrame" => Ok(AnyObject::CmpFrame(CmpFrame::from_node(node)?)),
        "FrameSet" => Ok(AnyObject::FrameSet(FrameSet::from_node(node)?)),
        other => Err(AstError::new(
            ErrorCode::Lderr,
            format!("unknown AST class: {other}"),
        )),
    }
}

// ---------------------------------------------------------------------------
// Public API: to_native_string / from_native_string
// ---------------------------------------------------------------------------

/// Serialise any AST object to Native text format.
///
/// The returned string is in the AST Native serialisation format, compatible
/// with the C library's `astWrite` / `astRead` channel functions.
pub fn to_native_string(obj: &AnyObject) -> AstResult<String> {
    let node = match obj {
        AnyObject::UnitMap(m) => Serializable::to_node(m)?,
        AnyObject::ZoomMap(m) => Serializable::to_node(m)?,
        AnyObject::ShiftMap(m) => Serializable::to_node(m)?,
        AnyObject::MatrixMap(m) => Serializable::to_node(m)?,
        AnyObject::PermMap(m) => Serializable::to_node(m)?,
        AnyObject::WinMap(m) => Serializable::to_node(m)?,
        AnyObject::LutMap(m) => Serializable::to_node(m)?,
        AnyObject::PolyMap(m) => Serializable::to_node(m)?,
        AnyObject::ChebyMap(m) => Serializable::to_node(m)?,
        AnyObject::CmpMap(m) => Serializable::to_node(m)?,
        AnyObject::WcsMap(m) => Serializable::to_node(m)?,
        AnyObject::SlaMap(m) => Serializable::to_node(m)?,
        AnyObject::SpecMap(m) => Serializable::to_node(m)?,
        AnyObject::TimeMap(m) => Serializable::to_node(m)?,
        AnyObject::SphMap(m) => Serializable::to_node(m)?,
        AnyObject::BasicFrame(f) => Serializable::to_node(f)?,
        AnyObject::SkyFrame(f) => Serializable::to_node(f)?,
        AnyObject::CmpFrame(f) => Serializable::to_node(f)?,
        AnyObject::FrameSet(f) => Serializable::to_node(f)?,
    };
    native_write::render_native(&node)
}

/// Deserialise an AST object from Native text format.
///
/// Parses the AST Native serialisation format and returns a type-erased
/// [`AnyObject`]. Use [`AnyObject::as_mapping`], [`AnyObject::as_frame`],
/// [`AnyObject::into_mapping`], or [`AnyObject::into_frame`] to extract
/// the concrete trait object.
pub fn from_native_string(s: &str) -> AstResult<AnyObject> {
    let node = native_read::parse_native(s)?;
    any_object_from_node(&node)
}
