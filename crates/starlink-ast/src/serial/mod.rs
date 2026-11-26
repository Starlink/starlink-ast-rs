// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Serialization: Node tree, Serializable trait, and Native format.

pub(crate) mod native_read;
pub(crate) mod native_stream;
pub(crate) mod native_write;
pub(crate) mod node;
mod object_text;

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
use crate::plot::Plot;
use crate::regions::data::{RegionData, check_fill_factor};
use crate::regions::{
    BoxRegion, Circle, CmpOperator, CmpRegion, CmpRegionBuilder, DynRegion, Ellipse, Interval, Moc,
    NullRegion, PointList, Polygon, Prism, Region, StcCatalogEntryLocation, StcObsDataLocation,
    StcResourceProfile, StcSearchLocation,
};

/// Internal trait for serializing AST objects to/from Node trees.
pub(crate) trait Serializable {
    /// The AST class name for Begin/End markers.
    fn class_name(&self) -> &'static str;

    /// Serialize this object to a Node tree.
    fn to_node(&self) -> AstResult<Node>;

    /// Reconstruct from a Node tree.
    fn from_node(node: &Node) -> AstResult<Self>
    where
        Self: Sized;
}

/// The most values a dump can name, C's `MXLOADEL`.
///
/// A name in a dump has at most eight characters (`MAX_NAME` in channel.c),
/// and the names that number a class's values, such as a MatrixMap's `M0`
/// to `M9999998`, start with at least one letter, so no dump can supply
/// more values than this. C's MatrixMap and PointSet loaders refuse a dump
/// claiming more; the port's loaders refuse any count read from a dump
/// that would make them size or read more, so that a few bytes of input
/// cannot make them allocate or loop without bound.
pub(crate) const MXLOADEL: usize = 9_999_999;

/// The error for a dump read by `loader` (such as `astLoadLutMap`) for an
/// Object of class `class` whose count `claim`, such as "The LutMap read
/// has 10000000 lookup table elements", needs more than [`MXLOADEL`]
/// values.
pub(crate) fn too_many_values(loader: &str, class: &str, claim: &str) -> AstError {
    AstError::new(
        ErrorCode::Lderr,
        format!("{loader}({class}): {claim}, but a dump cannot hold more than {MXLOADEL} values."),
    )
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
        origin: 0,
        // C reads Invert with its unset value as the default
        // (mapping.c:24130), so an `Invert = 0` card leaves it set to 0.
        invert_zero: node.get("Invert").is_some() && node.get_int("Invert", 0) == 0,
        frozen: false,
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
///
/// As in C, a negative Nin or Nout is 0, a missing Nout is Nin, and a number
/// over [`MXAXES`](crate::constants::MXAXES) records C's error for the load
/// and is taken, with the other, as 0, so that nothing is sized from it.
pub(crate) fn read_mapping_base(whole: &Node) -> MappingBaseAttrs {
    let node = &whole.section("Mapping");
    let nin = usize::try_from(node.get_int("Nin", 0)).unwrap_or(0);
    let nout = usize::try_from(node.get_int("Nout", nin as i64)).unwrap_or(0);
    let limit = crate::constants::MXAXES;
    let (nin, nout) = if nin > limit || nout > limit {
        let (code, which, n) = if nin > limit {
            (ErrorCode::Badni, "input", nin)
        } else {
            (ErrorCode::Badno, "output", nout)
        };
        node::record_load_error(AstError::new(
            code,
            format!(
                "astLoadMapping({}): Bad number of {which} coordinates ({n}) read from the \
                 dump - this number should not exceed {limit}.",
                whole.class
            ),
        ));
        (0, 0)
    } else {
        (nin, nout)
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

/// C's `astCheckLoadedCoords` (mapping.c): the numbers of inputs and outputs
/// `base` was read with, which describe the Mapping before any inversion,
/// must be `nin` and `nout` for an object of class `name`.
pub(crate) fn check_loaded_coords(
    name: &str,
    base: &MappingBaseAttrs,
    nin: usize,
    nout: usize,
) -> AstResult<()> {
    if base.nin == nin && base.nout == nout {
        return Ok(());
    }
    Err(AstError::new(
        ErrorCode::Lderr,
        format!(
            "astLoad({name}): Invalid numbers of inputs ({}) and outputs ({}) read for a \
             {name} - there should be {nin} and {nout}.",
            base.nin, base.nout
        ),
    ))
}

/// C's `astCheckLoadedMap` (mapping.c): the component Mapping an object of
/// class `name` reads as item `item`, which must be there and be a Mapping.
/// Gives it, with its numbers of inputs and outputs when used inverted if
/// `invert`.
pub(crate) fn check_loaded_map(
    node: Option<&Node>,
    invert: bool,
    name: &str,
    item: &str,
) -> AstResult<(DynMapping, usize, usize)> {
    let node =
        check_loaded(node, "Mapping", true, name, item)?.expect("a needed component is there");
    let map = mapping_from_node(node)?;
    let (nin, nout) = if map.is_inverted() == invert {
        (map.nin(), map.nout())
    } else {
        (map.nout(), map.nin())
    };
    Ok((map, nin, nout))
}

// ---------------------------------------------------------------------------
// Mapping dispatch
// ---------------------------------------------------------------------------

// The dispatches below load each class through one of these, so that the
// dispatch's own frame holds only a pointer. A `match` building every
// class's value in place would need room for all of them at once in an
// unoptimized build, and loading recurses through the dispatch once per
// level of nesting.

/// Load a `T` and box it as a Mapping.
#[inline(never)]
fn boxed_mapping<T: Serializable + Mapping + 'static>(node: &Node) -> AstResult<DynMapping> {
    Ok(Box::new(T::from_node(node)?))
}

/// Load a `T` and box it as a Frame.
#[inline(never)]
fn boxed_frame<T: Serializable + Frame + 'static>(node: &Node) -> AstResult<DynFrame> {
    Ok(Box::new(T::from_node(node)?))
}

/// Load a `T` and box it as a Region.
#[inline(never)]
fn boxed_region<T: Serializable + Region + 'static>(node: &Node) -> AstResult<DynRegion> {
    Ok(Box::new(T::from_node(node)?))
}

/// Reconstruct a concrete Mapping from a Node tree, dispatching by class name.
pub(crate) fn mapping_from_node(node: &Node) -> AstResult<DynMapping> {
    load_object(node, || {
        let mut result = mapping_from_node_inner(node)?;
        apply_mapping_object_items(result.as_mut(), node);
        Ok(result)
    })
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
        "UnitMap" => boxed_mapping::<UnitMap>(node),
        "ZoomMap" => boxed_mapping::<ZoomMap>(node),
        "ShiftMap" => boxed_mapping::<ShiftMap>(node),
        "MatrixMap" => boxed_mapping::<MatrixMap>(node),
        "PermMap" => boxed_mapping::<PermMap>(node),
        "WinMap" => boxed_mapping::<WinMap>(node),
        "LutMap" => boxed_mapping::<LutMap>(node),
        "PolyMap" => boxed_mapping::<PolyMap>(node),
        "ChebyMap" => boxed_mapping::<ChebyMap>(node),
        "CmpMap" => boxed_mapping::<CmpMap>(node),
        "WcsMap" => boxed_mapping::<WcsMap>(node),
        "SlaMap" => boxed_mapping::<SlaMap>(node),
        "SpecMap" => boxed_mapping::<SpecMap>(node),
        "TimeMap" => boxed_mapping::<TimeMap>(node),
        "PcdMap" => boxed_mapping::<PcdMap>(node),
        "SphMap" => boxed_mapping::<SphMap>(node),
        "GrismMap" => boxed_mapping::<GrismMap>(node),
        "UnitNormMap" => boxed_mapping::<UnitNormMap>(node),
        "XphMap" => boxed_mapping::<XphMap>(node),
        "IntraMap" => boxed_mapping::<IntraMap>(node),
        "TranMap" => boxed_mapping::<TranMap>(node),
        "RateMap" => boxed_mapping::<RateMap>(node),
        "SplineMap" => boxed_mapping::<SplineMap>(node),
        "DssMap" => boxed_mapping::<DssMap>(node),
        "NormMap" => boxed_mapping::<NormMap>(node),
        "MathMap" => boxed_mapping::<MathMap>(node),
        "SelectorMap" => boxed_mapping::<SelectorMap>(node),
        "SwitchMap" => boxed_mapping::<SwitchMap>(node),
        "FrameSet" => boxed_mapping::<FrameSet>(node),
        "Plot" => boxed_mapping::<Plot>(node),
        // Regions implement Mapping.
        "Circle" => boxed_mapping::<Circle>(node),
        "Box" => boxed_mapping::<BoxRegion>(node),
        "Ellipse" => boxed_mapping::<Ellipse>(node),
        "Polygon" => boxed_mapping::<Polygon>(node),
        "Interval" => boxed_mapping::<Interval>(node),
        "NullRegion" => boxed_mapping::<NullRegion>(node),
        "CmpRegion" => boxed_mapping::<CmpRegion>(node),
        "PointList" => boxed_mapping::<PointList>(node),
        "Prism" => boxed_mapping::<Prism>(node),
        "Moc" => boxed_mapping::<Moc>(node),
        "StcResourceProfile" => boxed_mapping::<StcResourceProfile>(node),
        "StcSearchLocation" => boxed_mapping::<StcSearchLocation>(node),
        "StcCatalogEntryLocation" => boxed_mapping::<StcCatalogEntryLocation>(node),
        "StcObsDataLocation" => boxed_mapping::<StcObsDataLocation>(node),
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
    /// Serialize this mapping to a Node tree.
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
        if let Some(f) = self.as_any().downcast_ref::<Plot>() {
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
    /// Serialize this frame to a Node tree.
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
        if let Some(f) = self.as_any().downcast_ref::<Plot>() {
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

/// The C class name of a Frame, as the `Begin` line of its dump gives it,
/// from the Frame as [`AsAny`](crate::mappings::AsAny) gives it.
/// A class the serialiser does not know is a plain "Frame".
#[cfg(feature = "attribute-strings")]
pub(crate) fn frame_class_name(frame: &dyn std::any::Any) -> &'static str {
    macro_rules! name_of {
        ($($class:ty),* $(,)?) => {
            $(
                if let Some(f) = frame.downcast_ref::<$class>() {
                    return Serializable::class_name(f);
                }
            )*
        };
    }
    name_of!(
        BasicFrame,
        CmpFrame,
        SkyFrame,
        DSBSpecFrame,
        SpecFrame,
        TimeFrame,
        FluxFrame,
        SpecFluxFrame,
        FrameSet,
        Plot,
        Circle,
        BoxRegion,
        Ellipse,
        Polygon,
        Interval,
        NullRegion,
        CmpRegion,
        PointList,
        Prism,
        Moc,
        StcResourceProfile,
        StcSearchLocation,
        StcCatalogEntryLocation,
        StcObsDataLocation,
    );
    "Frame"
}

/// Reconstruct a concrete Frame from a Node tree, dispatching by class name.
pub(crate) fn frame_from_node(node: &Node) -> AstResult<DynFrame> {
    load_object(node, || frame_from_node_inner(node))
}

fn frame_from_node_inner(node: &Node) -> AstResult<DynFrame> {
    match node.class.as_str() {
        "Frame" => boxed_frame::<BasicFrame>(node),
        "SkyFrame" => boxed_frame::<SkyFrame>(node),
        "DSBSpecFrame" => boxed_frame::<DSBSpecFrame>(node),
        "SpecFrame" => boxed_frame::<SpecFrame>(node),
        "TimeFrame" => boxed_frame::<TimeFrame>(node),
        "FluxFrame" => boxed_frame::<FluxFrame>(node),
        "SpecFluxFrame" => boxed_frame::<SpecFluxFrame>(node),
        "CmpFrame" => boxed_frame::<CmpFrame>(node),
        "FrameSet" => boxed_frame::<FrameSet>(node),
        "Plot" => boxed_frame::<Plot>(node),
        // Regions implement Frame.
        "Circle" => boxed_frame::<Circle>(node),
        "Box" => boxed_frame::<BoxRegion>(node),
        "Ellipse" => boxed_frame::<Ellipse>(node),
        "Polygon" => boxed_frame::<Polygon>(node),
        "Interval" => boxed_frame::<Interval>(node),
        "NullRegion" => boxed_frame::<NullRegion>(node),
        "CmpRegion" => boxed_frame::<CmpRegion>(node),
        "PointList" => boxed_frame::<PointList>(node),
        "Prism" => boxed_frame::<Prism>(node),
        "Moc" => boxed_frame::<Moc>(node),
        "StcResourceProfile" => boxed_frame::<StcResourceProfile>(node),
        "StcSearchLocation" => boxed_frame::<StcSearchLocation>(node),
        "StcCatalogEntryLocation" => boxed_frame::<StcCatalogEntryLocation>(node),
        "StcObsDataLocation" => boxed_frame::<StcObsDataLocation>(node),
        other => Err(AstError::new(
            ErrorCode::Lderr,
            format!("unknown or unsupported Frame class: {other}"),
        )),
    }
}

// ---------------------------------------------------------------------------
// PointSet serialization helpers
// ---------------------------------------------------------------------------

/// Write a PointSet as a serialized "PointSet" object node.
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

/// Read a PointSet from a serialized "PointSet" node.
///
/// Returns `Vec<Vec<f64>>` where `result[i]` is point `i` (length
/// `ncoord`). Iteration matches C `pointset.c::Dump` (point-outer,
/// coord-inner) — see `write_pointset_node` for the byte layout.
///
/// As pointset.c `astLoadPointSet` reads them, a number of points or
/// coordinates below 1 is 1, and a PointSet claiming more values than a dump
/// can name (`X1` to `X9999999`) is refused before anything is sized from it.
fn read_pointset_node(node: &Node) -> AstResult<Vec<Vec<f64>>> {
    const MXLOADVAL: i64 = 9_999_999;
    let npoint = node.get_int("Npoint", 1).max(1);
    let ncoord = node.get_int("Ncoord", 1).max(1);
    if ncoord > MXLOADVAL || npoint > MXLOADVAL / ncoord {
        return Err(AstError::new(
            ErrorCode::Lderr,
            format!(
                "astLoadPointSet({}): The PointSet read has {npoint} points with {ncoord} \
                 coordinates each, but a dump cannot hold more than {MXLOADVAL} coordinate \
                 values.",
                node.class
            ),
        ));
    }
    let (npoint, ncoord) = (npoint as usize, ncoord as usize);
    // A PointSet holding no data (pointset.c `Empty`).
    if node.get_int("Empty", 0) != 0 {
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

/// The two component Regions, `RegionA` and `RegionB`, of a CmpRegion or
/// Prism of class `class`, each of which must be there and be a Region
/// (`astCheckLoaded`).
fn component_regions(node: &Node, class: &str) -> AstResult<(DynRegion, DynRegion)> {
    let read = |key: &str, item: &str| -> AstResult<DynRegion> {
        let n = check_loaded(node.get_object(key), "Region", true, class, item)?
            .expect("a needed component is there");
        region_from_node(n)
    };
    let region1 = read("RegionA", "first component Region")?;
    let region2 = read("RegionB", "second component Region")?;
    Ok((region1, region2))
}

/// C's `astCheckLoadedPoints` (region.c): a Region of class `name` read
/// from a dump has `naxes` base Frame axes (any number if 0) and between
/// `minpt` and `maxpt` defining points (no upper limit if `None`).
fn check_loaded_points(
    data: &RegionData,
    name: &str,
    minpt: usize,
    maxpt: Option<usize>,
    naxes: usize,
) -> AstResult<()> {
    let nax = data.frameset.base().naxes();
    let np = data.points.len();
    let message = if naxes > 0 && nax != naxes {
        format!("The {name} read has {nax} axes, but a {name} must have {naxes}.")
    } else if data.points.is_empty() {
        format!("The points that define the {name} are missing from the {name} read.")
    } else if np < minpt || maxpt.is_some_and(|m| np > m) {
        format!(
            "The {name} read is defined by {np} points, which is not a number that a {name} can have."
        )
    } else {
        return Ok(());
    };
    Err(AstError::new(
        ErrorCode::Lderr,
        format!("astLoad({name}): {message}"),
    ))
}

/// C's check at the start of `astSetRegFS` (region.c): a Region of class
/// `class` defined by points of `ncoord` coordinates, if it has points,
/// cannot be given a Frame with another number of axes.
fn check_reg_fs(class: &str, ncoord: Option<usize>, frame: &dyn Frame) -> AstResult<()> {
    match ncoord {
        Some(n) if n != frame.naxes() => Err(AstError::new(
            ErrorCode::Regin,
            format!(
                "astSetRegFS({class}): The {class} is defined by points with {n} coordinate \
                 value(s), so it cannot be given a {} with {} axes.",
                crate::frames::class_name(frame),
                frame.naxes()
            ),
        )),
        _ => Ok(()),
    }
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
    let class = whole.class.as_str();
    // astLoadFrame reads the Region's own Frame part first. A Region then
    // has no FrameSet, so a System given there is not recognised (region.c).
    let frame_part = whole.section("Frame");
    crate::frames::frame_attrs::load_frame_axes(&frame_part, class)?;
    crate::frames::frame_attrs::read_system_item(&frame_part, "System", "System", |_| None::<()>)?;
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
    // A port-only limit: a mesh of more points than a dump can name (see
    // `MXLOADEL`) is refused rather than sized.
    if mesh_size > MXLOADEL {
        return Err(too_many_values(
            "astLoadRegion",
            class,
            &format!("The MeshSize read is {mesh_size}"),
        ));
    }
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

    // The defining points, which must be a PointSet, with the number of
    // coordinates each has; with none, the number of axes the Region spans
    // is its `RegAxes` value, for a Region such as a NullRegion (region.c
    // `astLoadRegion`). Their coordinate count sizes a dummy FrameSet when
    // neither `Frm` nor `FrmSet` is present.
    let (points, naxpt) = match node.get_object("Points") {
        Some(pts_node) if node_is_a(pts_node, "PointSet") => {
            let points = load_object(pts_node, || read_pointset_node(pts_node))?;
            let ncoord = usize::try_from(pts_node.get_int("Ncoord", 1).max(1)).unwrap_or(1);
            (points, ncoord)
        }
        Some(pts_node) => {
            any_object_from_node(pts_node)?;
            return Err(AstError::new(
                ErrorCode::Regin,
                format!(
                    "astLoadRegion({class}): Corrupt {class} specifies points using a {} \
                     (should be a PointSet).",
                    pts_node.class
                ),
            ));
        }
        None => (
            Vec::new(),
            usize::try_from(node.get_int("RegAxes", 0)).unwrap_or(0),
        ),
    };

    // The uncertainty Region, which must be a Region.
    let mut uncertainty = match check_loaded(
        node.get_object("Unc"),
        "Region",
        false,
        class,
        "uncertainty Region",
    )? {
        Some(unc_node) => Some(region_from_node(unc_node)?),
        None => None,
    };

    // The FrameSet or Frame. Detect which one was emitted by key
    // presence — the C library only writes `RegFS` when RegionFS was
    // explicitly set (rare), so most dumps have either `Frm` or `FrmSet`
    // and no `RegFS` line. A nested region (an uncertainty sub-region, or
    // a component of a Prism/CmpRegion/SelectorMap) is dumped with neither
    // — it inherits its container's frame. In that case C synthesizes a
    // dummy FrameSet with RegionFS=0 (`region.c:13593-13601`) that the
    // parent later replaces via `astSetRegFS`; we mirror that with the
    // `dummy_fs` flag.
    // C loader (region.c `Load`): `Frm` present → RegionFS = 1; otherwise
    // `FrmSet` present → read RegFS (default 1); neither → dummy FrameSet
    // with RegionFS = 0. `nax` is the number of base Frame axes. Each must
    // be of its class.
    let ncoord = (!points.is_empty()).then_some(naxpt);
    let frm = check_loaded(node.get_object("Frm"), "Frame", false, class, "Frame")?;
    let frmset = if frm.is_none() {
        check_loaded(
            node.get_object("FrmSet"),
            "FrameSet",
            false,
            class,
            "FrameSet",
        )?
    } else {
        None
    };
    let (frameset, dummy_fs, region_fs, nax) = if let Some(frm_node) = frm {
        // Only a single Frame stored — create a 2-frame FrameSet with
        // UnitMap connecting base to current (matches the C library and
        // keeps with_integrity working), as astSetRegFS does.
        let frame = frame_from_node(frm_node)?;
        check_reg_fs(class, ncoord, frame.as_ref())?;
        let naxes = frame.naxes();
        let current_snap = frame.snapshot();
        let mut fs = FrameSet::new(frame);
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(naxes)?), current_snap)
            .expect("adding current frame to region FrameSet");
        (fs, false, true, naxes)
    } else if let Some(fs_node) = frmset {
        let region_fs = node.get_int("RegFS", 1) != 0;
        let fs = FrameSet::from_node(fs_node)?;
        let nin = fs.base().naxes();
        (fs, false, region_fs, nin)
    } else {
        // Dummy FrameSet: a default BasicFrame of `naxpt` axes (>= 1),
        // marked dummy so the parent re-associates the real frame.
        // RegionFS = 0 (the Frame is inferred from the enclosing context).
        let nax = if naxpt > 0 { naxpt } else { 1 };
        let frame: DynFrame = Box::new(BasicFrameBuilder::new(nax).build()?);
        let current_snap = frame.snapshot();
        let mut fs = FrameSet::new(frame);
        fs.add_frame(FrameRef::Base, Box::new(UnitMap::new(nax)?), current_snap)
            .expect("adding current frame to dummy region FrameSet");
        (fs, true, false, nax)
    };

    // An uncertainty Region dumped frame-less (dummy FS) is given this
    // Region's base Frame, as C's `astSetRegFS` does for the Frame read, or
    // the loader for a FrameSet (region.c), with the same check.
    if !dummy_fs
        && let Some(unc) = uncertainty.as_mut()
        && unc.has_dummy_fs()
    {
        let base_frame = frameset.base().snapshot();
        let unc_ncoord = unc
            .base_points()
            .ok()
            .filter(|p| p.npoint() > 0)
            .map(|p| p.ncoord());
        check_reg_fs(
            crate::regions::class_name(unc.as_any()),
            unc_ncoord,
            base_frame.as_ref(),
        )?;
        unc.set_region_fs(base_frame.as_ref());
    }

    // The points must have one value per base Frame axis (region.c `Load`).
    if !points.is_empty() && naxpt != nax {
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

    // The uncertainty Region is defined in the base Frame, so has its axes.
    if let Some(unc) = &uncertainty
        && unc.naxes() != nax
    {
        return Err(AstError::new(
            ErrorCode::Lderr,
            format!(
                "astLoadRegion({class}): The uncertainty Region read for a {class} has {} axes, \
                 but the {class} has {nax}.",
                unc.naxes()
            ),
        ));
    }

    let data = RegionData {
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
        class_cache: RefCell::new(None),
        def_unc: RefCell::new(None),
    };

    Ok(data)
}

// ---------------------------------------------------------------------------
// Region dispatch
// ---------------------------------------------------------------------------

/// Extension trait enabling `to_node()` on `dyn Region`.
pub(crate) trait RegionToNode {
    /// Serialize this region to a Node tree.
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
    load_object(node, || region_from_node_inner(node))
}

fn region_from_node_inner(node: &Node) -> AstResult<DynRegion> {
    match node.class.as_str() {
        "Circle" => boxed_region::<Circle>(node),
        "Box" => boxed_region::<BoxRegion>(node),
        "Ellipse" => boxed_region::<Ellipse>(node),
        "Polygon" => boxed_region::<Polygon>(node),
        "Interval" => boxed_region::<Interval>(node),
        "NullRegion" => boxed_region::<NullRegion>(node),
        "CmpRegion" => boxed_region::<CmpRegion>(node),
        "PointList" => boxed_region::<PointList>(node),
        "Prism" => boxed_region::<Prism>(node),
        "Moc" => boxed_region::<Moc>(node),
        "StcResourceProfile" => boxed_region::<StcResourceProfile>(node),
        "StcSearchLocation" => boxed_region::<StcSearchLocation>(node),
        "StcCatalogEntryLocation" => boxed_region::<StcCatalogEntryLocation>(node),
        "StcObsDataLocation" => boxed_region::<StcObsDataLocation>(node),
        other => Err(AstError::new(
            ErrorCode::Lderr,
            format!("unknown Region class: {other}"),
        )),
    }
}

// ---------------------------------------------------------------------------
// Serializable implementations for Region classes
// ---------------------------------------------------------------------------

/// Build the standard Region serialization skeleton: class header, the
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
        check_loaded_points(&data, &node.class, 2, Some(2), 0)?;
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
        check_loaded_points(&data, &node.class, 2, Some(2), 0)?;
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
        check_loaded_points(&data, &node.class, 3, Some(3), 2)?;
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
        check_loaded_points(&data, &whole.class, 1, None, 2)?;
        // Every vertex is defined, as the constructor requires (polygon.c
        // `astLoadPolygon`), checked axis by axis.
        for axis in 0..2 {
            if let Some(vertex) = data
                .points
                .iter()
                .position(|p| p[axis] == crate::constants::BAD_VALUE)
            {
                return Err(AstError::new(
                    ErrorCode::Lderr,
                    format!(
                        "astLoad({0}): Vertex number {1} of the {0} read has an undefined value on \
                         axis {2}.",
                        whole.class,
                        vertex + 1,
                        axis + 1
                    ),
                ));
            }
        }
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
        check_loaded_points(&data, &node.class, 2, Some(2), 0)?;
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
        check_loaded_points(&data, &node.class, 1, None, 0)?;
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

        // The components, which must be Regions with the axes of the base
        // Frame both are defined in, and the operator, which must be AND, OR
        // or XOR (cmpregion.c `astLoadCmpRegion`).
        let class = &whole.class;
        let (mut region1, mut region2) = component_regions(node, class)?;
        let nax = data.frameset.base().naxes();
        if !(1..=3).contains(&oper_int) {
            return Err(AstError::new(
                ErrorCode::Lderr,
                format!(
                    "astLoadCmpRegion({class}): Illegal boolean operator value ({oper_int}) read \
                     for a {class}."
                ),
            ));
        } else if region1.naxes() != nax || region2.naxes() != nax {
            return Err(AstError::new(
                ErrorCode::Lderr,
                format!(
                    "astLoadCmpRegion({class}): The component Regions read for a {class} have {} \
                     and {} axes, but the {class} has {nax}.",
                    region1.naxes(),
                    region2.naxes()
                ),
            ));
        }

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

        // The encapsulated Region, which must be a Region in the Stc's base
        // Frame (stc.c `astLoadStc`).
        let class = &whole.class;
        let region_node = check_loaded(
            node.get_object("Region"),
            "Region",
            true,
            class,
            "encapsulated Region",
        )?
        .expect("a needed component is there");
        let mut region = region_from_node(region_node)?;
        let nax = data.frameset.base().naxes();
        if region.naxes() != nax {
            return Err(AstError::new(
                ErrorCode::Lderr,
                format!(
                    "astLoadStc({class}): The Region encapsulated by the {class} read has {} axes, \
                     but the {class} has {nax}.",
                    region.naxes()
                ),
            ));
        }
        let f1 = data.frameset.base().snapshot();
        if !data.dummy_fs && region.has_dummy_fs() {
            region.set_region_fs(f1.as_ref());
        }

        // The AstroCoords KeyMaps, read in turn, each of which must be a
        // KeyMap; the number claimed sizes nothing.
        let ncoord = node.get_int("Ncoord", 0).max(0);
        let mut coords = Vec::new();
        for ico in 1..=ncoord {
            let km_node = check_loaded(
                node.get_object(&format!("Coord{ico}")),
                "KeyMap",
                true,
                class,
                "AstroCoords KeyMap",
            )?
            .expect("a needed component is there");
            let mut keymap = load_object(km_node, || {
                <crate::keymap::KeyMap as Serializable>::from_node(km_node)
            })?;
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
        let join = |lo: i64, hi: i64| ((hi as u32 as u64) << 32 | (lo as u32 as u64)) as i64;
        let ill_formed = |detail: String| {
            AstError::new(ErrorCode::Lderr, "astLoadMoc(Moc): Ill-formed Moc.").with_context(detail)
        };
        // moc.c `astLoadMoc`: the number of ranges is not negative, a Moc
        // with ranges has a valid MaxOrder (C's unset value is -INT_MAX),
        // and the ranges are read in turn, the stated number sizing nothing;
        // a missing range reads as [0:0], which overlaps its predecessor.
        let nrange = node.get_int("NRange", 0);
        if nrange < 0 {
            return Err(ill_formed(format!("Invalid number of ranges ({nrange}).")));
        }
        let max_order = moc.test_max_order().then(|| moc.max_order());
        let valid_order = max_order.filter(|o| (0..=crate::regions::MOC_MAX_ORDER).contains(o));
        if nrange > 0 && valid_order.is_none() {
            return Err(ill_formed(format!(
                "Missing or invalid MaxOrder ({}) for a Moc containing {nrange} range(s).",
                max_order.unwrap_or(-i32::MAX)
            )));
        }
        let max_nest = valid_order.map_or(0, |o| 12 * (1_i64 << (2 * o)));
        let mut range = Vec::new();
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
            if lo < 0 {
                return Err(ill_formed(format!("Lower bound ({lo}) is negative.")));
            }
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

        // The components, which must be Regions whose axes together are
        // those of the Prism's base Frame (prism.c `astLoadPrism`).
        let class = &whole.class;
        let (mut region1, mut region2) = component_regions(node, class)?;
        let nax = data.frameset.base().naxes();
        if region1.naxes() + region2.naxes() != nax {
            return Err(AstError::new(
                ErrorCode::Lderr,
                format!(
                    "astLoadPrism({class}): The component Regions read for a {class} have {} and \
                     {} axes, but the {class} has {nax}.",
                    region1.naxes(),
                    region2.naxes()
                ),
            ));
        }

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
// AnyObject — type-erased AST object for serialization round-trips
// ---------------------------------------------------------------------------

/// Any AST object, with one variant per class.
///
/// Each AST class is its own Rust type, and there is no common base trait.
/// `AnyObject` is what reading a [`Channel`](crate::channel::Channel) or
/// [`from_native_string`] returns, since the class is not known in advance.
/// [`as_mapping`](AnyObject::as_mapping), [`as_frame`](AnyObject::as_frame)
/// and [`as_region`](AnyObject::as_region), and their `into_` forms, give the
/// object as a trait object without matching on variants, and each class's
/// `TryFrom<AnyObject>` gives the concrete type.
///
/// Every AST object has:
///
/// - a class name, [`class_name`](AnyObject::class_name) (the Class attribute);
/// - Ident and ID, two strings a user may set to label the object, which the
///   library stores and writes to a dump (see [`Mapping::ident`] and
///   [`Mapping::id`], or [`Frame::ident`] and [`Frame::id`]); a Mapping with
///   Ident set is also left intact by simplification (see
///   [`Mapping::do_not_simplify`]), and a clone keeps Ident but not ID;
/// - [`Clone`], which makes a deep copy, sharing nothing with the original,
///   so that changing one never changes the other (a Channel's clone is the
///   exception: see the variants below);
/// - [`show`](AnyObject::show) and
///   [`to_native_string`](AnyObject::to_native_string), which write its
///   Native description; the same two methods exist on each class.
///
/// Objects are owned values with no reference count: an object lives until
/// its owner drops it.
///
/// ```
/// use starlink_ast::mappings::ZoomMap;
///
/// let text = ZoomMap::new(1, 2.0).unwrap().to_native_string().unwrap();
/// let obj = starlink_ast::from_native_string(&text).unwrap();
/// assert_eq!(obj.class_name(), "ZoomMap");
/// assert!(obj.as_mapping().is_some());
/// assert!(obj.as_frame().is_none());
/// let zoom = ZoomMap::try_from(obj.clone()).unwrap();
/// assert_eq!(zoom.zoom(), 2.0);
/// ```
///
/// # See also
///
/// - `astCopy`: [`Clone`].
// Non-exhaustive so that each class the port adds is not a breaking change.
#[doc(alias = "Object")]
#[doc(alias = "astCopy")]
#[derive(Debug, Clone)]
#[non_exhaustive]
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
    /// A [`NormMap`] (coordinate normalization mapping).
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
    /// A [`Plot`] (FrameSet for drawing coordinate grids).
    Plot(Box<Plot>),
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
    /// A [`StcCatalogEntryLocation`] (STC catalog entry location).
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
    /// A [`crate::channel::NativeChannel`], the AST Channel class.
    /// Implements neither [`Mapping`] nor [`Frame`].
    /// It shares its source and sink with the
    /// channel it was cloned from; one read from a dump has neither.
    NativeChannel(Box<crate::channel::NativeChannel>),
    /// A [`crate::channel::MocChan`].
    /// Implements neither [`Mapping`] nor [`Frame`]; its source and sink are shared as a `NativeChannel`'s are.
    MocChan(Box<crate::channel::MocChan>),
    /// A [`crate::fitschan::FitsChan`].
    /// Implements neither [`Mapping`] nor [`Frame`].
    /// A clone has no source or sink, and nor has one read from a dump.
    FitsChan(Box<crate::fitschan::FitsChan>),
}

impl AnyObject {
    /// Wrap a `&dyn Mapping` in its matching variant — the inverse of
    /// [`as_mapping`](AnyObject::as_mapping).
    ///
    /// `None` means the concrete class has no variant here. Callers that
    /// serialize arbitrary Mappings must treat that as a failure rather than
    /// skip the object: a test that renders through this and ignores `None`
    /// stops checking whole classes without saying so.
    ///
    /// Every variant for which `as_mapping` yields `Some` appears below, and
    /// `any_object_from_mapping_covers_as_mapping` in `serial`'s tests asserts
    /// the two lists stay the same length.
    ///
    /// The wrapped object keeps the Mapping's [`report`](Mapping::report),
    /// which a clone clears, since wrapping it to write it is not a copy.
    pub fn from_mapping(m: &dyn Mapping) -> Option<Self> {
        fn wrapped<T: Mapping + Clone>(x: &T) -> T {
            let mut copy = x.clone();
            copy.set_report_attr(x.report_attr());
            copy
        }
        if let Some(x) = m.as_any().downcast_ref::<UnitMap>() {
            return Some(AnyObject::UnitMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<ZoomMap>() {
            return Some(AnyObject::ZoomMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<ShiftMap>() {
            return Some(AnyObject::ShiftMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<MatrixMap>() {
            return Some(AnyObject::MatrixMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<PermMap>() {
            return Some(AnyObject::PermMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<WinMap>() {
            return Some(AnyObject::WinMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<LutMap>() {
            return Some(AnyObject::LutMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<PolyMap>() {
            return Some(AnyObject::PolyMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<ChebyMap>() {
            return Some(AnyObject::ChebyMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<CmpMap>() {
            return Some(AnyObject::CmpMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<WcsMap>() {
            return Some(AnyObject::WcsMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<SlaMap>() {
            return Some(AnyObject::SlaMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<SpecMap>() {
            return Some(AnyObject::SpecMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<TimeMap>() {
            return Some(AnyObject::TimeMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<PcdMap>() {
            return Some(AnyObject::PcdMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<GrismMap>() {
            return Some(AnyObject::GrismMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<SphMap>() {
            return Some(AnyObject::SphMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<UnitNormMap>() {
            return Some(AnyObject::UnitNormMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<XphMap>() {
            return Some(AnyObject::XphMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<IntraMap>() {
            return Some(AnyObject::IntraMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<TranMap>() {
            return Some(AnyObject::TranMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<RateMap>() {
            return Some(AnyObject::RateMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<MathMap>() {
            return Some(AnyObject::MathMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<SplineMap>() {
            return Some(AnyObject::SplineMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<DssMap>() {
            return Some(AnyObject::DssMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<NormMap>() {
            return Some(AnyObject::NormMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<SelectorMap>() {
            return Some(AnyObject::SelectorMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<SwitchMap>() {
            return Some(AnyObject::SwitchMap(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<FrameSet>() {
            return Some(AnyObject::FrameSet(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<Plot>() {
            return Some(AnyObject::Plot(Box::new(wrapped(x))));
        }
        if let Some(x) = m.as_any().downcast_ref::<Circle>() {
            return Some(AnyObject::Circle(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<BoxRegion>() {
            return Some(AnyObject::BoxRegion(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<Ellipse>() {
            return Some(AnyObject::Ellipse(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<Polygon>() {
            return Some(AnyObject::Polygon(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<Interval>() {
            return Some(AnyObject::Interval(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<NullRegion>() {
            return Some(AnyObject::NullRegion(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<CmpRegion>() {
            return Some(AnyObject::CmpRegion(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<PointList>() {
            return Some(AnyObject::PointList(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<Prism>() {
            return Some(AnyObject::Prism(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<Moc>() {
            return Some(AnyObject::Moc(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<StcResourceProfile>() {
            return Some(AnyObject::StcResourceProfile(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<StcSearchLocation>() {
            return Some(AnyObject::StcSearchLocation(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<StcCatalogEntryLocation>() {
            return Some(AnyObject::StcCatalogEntryLocation(wrapped(x)));
        }
        if let Some(x) = m.as_any().downcast_ref::<StcObsDataLocation>() {
            return Some(AnyObject::StcObsDataLocation(wrapped(x)));
        }
        None
    }

    /// Return a reference to the contained object as a `&dyn Mapping`,
    /// or `None` if the variant does not implement [`Mapping`].
    ///
    /// Every Mapping class, FrameSet and every Region implement it; the other
    /// Frame classes, KeyMap, Table, FitsTable and the Channels do not.
    ///
    /// # See also
    ///
    /// - `astIsA<Class>`, one function per class `<Class>`, here `Mapping`; to
    ///   test for a single class, use its `TryFrom<AnyObject>` or match the
    ///   variant.
    #[doc(alias = "astIsA")]
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
            AnyObject::Plot(f) => Some(f.as_ref()),
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
    ///
    /// Every Frame class, FrameSet and every Region implement it.
    ///
    /// # See also
    ///
    /// - `astIsA<Class>`, one function per class `<Class>`, here `Frame`.
    #[doc(alias = "astIsA")]
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
            AnyObject::Plot(f) => Some(f.as_ref()),
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
            AnyObject::Plot(f) => Some(f),
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
            AnyObject::Plot(f) => Some(f),
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
    ///
    /// # See also
    ///
    /// - `astIsA<Class>`, one function per class `<Class>`, here `Region`.
    #[doc(alias = "astIsA")]
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
    boxed_any_object_from_node(node).map(|obj| *obj)
}

/// [`any_object_from_node`], boxed.
///
/// Boxed through `load_object`, whose frame would otherwise hold several
/// AnyObjects: the Objects in a KeyMap load through here, so KeyMaps nested
/// in KeyMaps recurse through it.
pub(crate) fn boxed_any_object_from_node(node: &Node) -> AstResult<Box<AnyObject>> {
    load_object(node, || any_object_from_node_inner(node).map(Box::new))
}

/// Load `node` with `load` as C loads an Object, giving C's error.
///
/// A typed read that fails records its error and lets the loader go on
/// (see [`node::record_read_error`]), where C's loader, the status set,
/// would do nothing more; so an error recorded while `load` ran is the one
/// reported, ahead of any the loader went on to raise. An Object read as an
/// item's value adds C's `Failed to read the "<item>" Object value.`, once,
/// for the innermost such Object.
pub(crate) fn load_object<T>(node: &Node, load: impl FnOnce() -> AstResult<T>) -> AstResult<T> {
    let outer = node::take_read_error();
    // object.c `astLoadObject`, which every loader runs first, reads the
    // Object's associated KeyMap and checks it is one.
    let object_part = node.section("Object");
    let keymap = check_loaded(
        object_part.get_object("KeyMap"),
        "KeyMap",
        false,
        &node.class,
        "associated KeyMap",
    );
    let result = match keymap.and_then(|km| km.map(any_object_from_node).transpose()) {
        Ok(_) => load(),
        Err(e) => Err(e),
    };
    let recorded = node::take_read_error();
    node::restore_read_error(outer);
    let result = match recorded {
        Some(e) => Err(e),
        None => result,
    };
    match (&node.item.0, result) {
        (Some(name), Err(e)) => Err(e.in_object_value(name)),
        (_, result) => result,
    }
}

/// Whether `node` describes an object of class `class` or of a class
/// derived from it.
pub(crate) fn node_is_a(node: &Node, class: &str) -> bool {
    native_stream::class_chain(&node.class).is_some_and(|chain| chain.contains(&class))
}

/// C's `astCheckLoaded` (object.c) for the component a loader reads as the
/// value of item `item` of an object of class `name`: `node` must be an
/// object of class `class` or a class derived from it, and must be there if
/// `needed`. Gives the component's description, for the caller to load, or
/// `None` when it is not there. A component of another class is read first,
/// as C reads it before checking it, so that its own error comes first.
pub(crate) fn check_loaded<'a>(
    node: Option<&'a Node>,
    class: &str,
    needed: bool,
    name: &str,
    item: &str,
) -> AstResult<Option<&'a Node>> {
    let Some(node) = node else {
        if needed {
            return Err(AstError::new(
                ErrorCode::Lderr,
                format!("astLoad({name}): The {item} is missing from the {name} read."),
            ));
        }
        return Ok(None);
    };
    if node_is_a(node, class) {
        return Ok(Some(node));
    }
    // The port loads an Axis or a PointSet only as part of the object
    // holding it.
    if !matches!(node.class.as_str(), "Axis" | "SkyAxis" | "PointSet") {
        any_object_from_node(node)?;
    }
    Err(AstError::new(
        ErrorCode::Lderr,
        format!(
            "astLoad({name}): The {item} read for a {name} is a {}, which is not a {class}.",
            node.class
        ),
    ))
}

/// Load a `T` and wrap it as an [`AnyObject`], in a frame of its own, as
/// [`boxed_mapping`] does for the Mapping dispatch.
#[inline(never)]
fn wrapped<T: Serializable>(node: &Node, wrap: fn(T) -> AnyObject) -> AstResult<AnyObject> {
    Ok(wrap(T::from_node(node)?))
}

/// [`wrapped`] for a Mapping, applying the items every Mapping shares.
#[inline(never)]
fn wrapped_mapping<T: Serializable + Mapping>(
    node: &Node,
    wrap: fn(T) -> AnyObject,
) -> AstResult<AnyObject> {
    Ok(wrap(load_mapping::<T>(node)?))
}

fn any_object_from_node_inner(node: &Node) -> AstResult<AnyObject> {
    match node.class.as_str() {
        "UnitMap" => wrapped_mapping::<UnitMap>(node, AnyObject::UnitMap),
        "ZoomMap" => wrapped_mapping::<ZoomMap>(node, AnyObject::ZoomMap),
        "ShiftMap" => wrapped_mapping::<ShiftMap>(node, AnyObject::ShiftMap),
        "MatrixMap" => wrapped_mapping::<MatrixMap>(node, AnyObject::MatrixMap),
        "PermMap" => wrapped_mapping::<PermMap>(node, AnyObject::PermMap),
        "WinMap" => wrapped_mapping::<WinMap>(node, AnyObject::WinMap),
        "LutMap" => wrapped_mapping::<LutMap>(node, AnyObject::LutMap),
        "PolyMap" => wrapped_mapping::<PolyMap>(node, AnyObject::PolyMap),
        "ChebyMap" => wrapped_mapping::<ChebyMap>(node, AnyObject::ChebyMap),
        "CmpMap" => wrapped_mapping::<CmpMap>(node, AnyObject::CmpMap),
        "WcsMap" => wrapped_mapping::<WcsMap>(node, AnyObject::WcsMap),
        "SlaMap" => wrapped_mapping::<SlaMap>(node, AnyObject::SlaMap),
        "SpecMap" => wrapped_mapping::<SpecMap>(node, AnyObject::SpecMap),
        "TimeMap" => wrapped_mapping::<TimeMap>(node, AnyObject::TimeMap),
        "PcdMap" => wrapped_mapping::<PcdMap>(node, AnyObject::PcdMap),
        "GrismMap" => wrapped_mapping::<GrismMap>(node, AnyObject::GrismMap),
        "SphMap" => wrapped_mapping::<SphMap>(node, AnyObject::SphMap),
        "UnitNormMap" => wrapped_mapping::<UnitNormMap>(node, AnyObject::UnitNormMap),
        "XphMap" => wrapped_mapping::<XphMap>(node, AnyObject::XphMap),
        "IntraMap" => wrapped_mapping::<IntraMap>(node, AnyObject::IntraMap),
        "TranMap" => wrapped_mapping::<TranMap>(node, AnyObject::TranMap),
        "RateMap" => wrapped_mapping::<RateMap>(node, AnyObject::RateMap),
        "MathMap" => wrapped_mapping::<MathMap>(node, AnyObject::MathMap),
        "SplineMap" => wrapped_mapping::<SplineMap>(node, AnyObject::SplineMap),
        "DssMap" => wrapped_mapping::<DssMap>(node, AnyObject::DssMap),
        "NormMap" => wrapped_mapping::<NormMap>(node, AnyObject::NormMap),
        "SelectorMap" => wrapped_mapping::<SelectorMap>(node, AnyObject::SelectorMap),
        "SwitchMap" => wrapped_mapping::<SwitchMap>(node, AnyObject::SwitchMap),
        "Frame" => wrapped::<BasicFrame>(node, AnyObject::BasicFrame),
        "SkyFrame" => wrapped::<SkyFrame>(node, AnyObject::SkyFrame),
        "SpecFrame" => wrapped::<SpecFrame>(node, AnyObject::SpecFrame),
        "DSBSpecFrame" => wrapped::<DSBSpecFrame>(node, AnyObject::DSBSpecFrame),
        "TimeFrame" => wrapped::<TimeFrame>(node, AnyObject::TimeFrame),
        "FluxFrame" => wrapped::<FluxFrame>(node, AnyObject::FluxFrame),
        "SpecFluxFrame" => {
            wrapped::<SpecFluxFrame>(node, |v| AnyObject::SpecFluxFrame(Box::new(v)))
        }
        "CmpFrame" => wrapped::<CmpFrame>(node, AnyObject::CmpFrame),
        "FrameSet" => wrapped::<FrameSet>(node, AnyObject::FrameSet),
        "Plot" => wrapped::<Plot>(node, |v| AnyObject::Plot(Box::new(v))),
        // Regions.
        "Circle" => wrapped::<Circle>(node, AnyObject::Circle),
        "Box" => wrapped::<BoxRegion>(node, AnyObject::BoxRegion),
        "Ellipse" => wrapped::<Ellipse>(node, AnyObject::Ellipse),
        "Polygon" => wrapped::<Polygon>(node, AnyObject::Polygon),
        "Interval" => wrapped::<Interval>(node, AnyObject::Interval),
        "NullRegion" => wrapped::<NullRegion>(node, AnyObject::NullRegion),
        "CmpRegion" => wrapped::<CmpRegion>(node, AnyObject::CmpRegion),
        "PointList" => wrapped::<PointList>(node, AnyObject::PointList),
        "Prism" => wrapped::<Prism>(node, AnyObject::Prism),
        "Moc" => wrapped::<Moc>(node, AnyObject::Moc),
        "StcResourceProfile" => wrapped::<StcResourceProfile>(node, AnyObject::StcResourceProfile),
        "StcSearchLocation" => wrapped::<StcSearchLocation>(node, AnyObject::StcSearchLocation),
        "StcCatalogEntryLocation" => {
            wrapped::<StcCatalogEntryLocation>(node, AnyObject::StcCatalogEntryLocation)
        }
        "StcObsDataLocation" => wrapped::<StcObsDataLocation>(node, AnyObject::StcObsDataLocation),
        "KeyMap" => wrapped::<crate::keymap::KeyMap>(node, |v| AnyObject::KeyMap(Box::new(v))),
        "Table" => wrapped::<crate::table::Table>(node, |v| AnyObject::Table(Box::new(v))),
        "FitsTable" => {
            wrapped::<crate::fitstable::FitsTable>(node, |v| AnyObject::FitsTable(Box::new(v)))
        }
        "Channel" => wrapped::<crate::channel::NativeChannel>(node, |v| {
            AnyObject::NativeChannel(Box::new(v))
        }),
        "MocChan" => wrapped::<crate::channel::MocChan>(node, |v| AnyObject::MocChan(Box::new(v))),
        "FitsChan" => {
            wrapped::<crate::fitschan::FitsChan>(node, |v| AnyObject::FitsChan(Box::new(v)))
        }
        // A PointSet, which the port does not load as an object of its own,
        // is still checked as C's loader checks it.
        "PointSet" => {
            read_pointset_node(node)?;
            Err(AstError::new(
                ErrorCode::Lderr,
                "unknown AST class: PointSet".to_string(),
            ))
        }
        other => Err(AstError::new(
            ErrorCode::Lderr,
            format!("{NO_LOADER}{other}"),
        )),
    }
}

/// The start of the error for a class with no loader of its own, such as
/// an Axis, which only a Frame's loader reads.
const NO_LOADER: &str = "unknown AST class: ";

/// Whether `e` is the error for loading a class with no loader of its own.
pub(crate) fn is_no_loader_error(e: &AstError) -> bool {
    e.code() == ErrorCode::Lderr && e.message().starts_with(NO_LOADER)
}

// ---------------------------------------------------------------------------
// AnyObject → Node helper (used by Channel)
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
        AnyObject::Plot(f) => Serializable::to_node(f.as_ref()),
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
// Public API: from_native_string
// ---------------------------------------------------------------------------

/// Re-create an AST object from its Native text, such as
/// [`to_native_string`](AnyObject::to_native_string) gives.
///
/// The text is read with a [`NativeChannel`](crate::channel::NativeChannel)
/// and the first object in it is returned as an [`AnyObject`].
/// Use [`AnyObject::as_mapping`], [`AnyObject::as_frame`],
/// [`AnyObject::into_mapping`], or [`AnyObject::into_frame`] to extract
/// the concrete trait object.
///
/// # Errors
///
/// - `ErrorCode::Rderr` if the text holds no object.
/// - `ErrorCode::Uritf` if the text holds an IntraMap, since no IntraMap
///   transformation is known here; read it with [`from_native_string_with`].
/// - Otherwise passes on the error of
///   [`Channel::read`](crate::channel::Channel::read).
#[doc(alias = "astFromString")]
pub fn from_native_string(s: &str) -> AstResult<AnyObject> {
    from_native_string_with(s, &IntraRegistry::new())
}

/// Deserialize an AST object from Native text format, giving any IntraMap in
/// it, however deeply nested, its transformation from `registry`.
///
/// This is [`from_native_string`] reading through a
/// [`NativeChannel`](crate::channel::NativeChannel) that has had each
/// transformation in `registry` registered with
/// [`Channel::register_intra`](crate::channel::Channel::register_intra).
///
/// # Errors
///
/// - `ErrorCode::Rderr` if the text holds no object.
/// - `ErrorCode::Uritf` if the text holds an IntraMap whose transformation is
///   not in `registry`.
/// - Otherwise passes on the error of
///   [`Channel::read`](crate::channel::Channel::read).
#[doc(alias = "astFromString")]
pub fn from_native_string_with(s: &str, registry: &IntraRegistry) -> AstResult<AnyObject> {
    use crate::channel::{Channel, NativeChannel};
    let mut ch = NativeChannel::from_string_with(s, registry.clone());
    ch.read()?
        .ok_or_else(|| AstError::new(ErrorCode::Rderr, "no object found in input"))
}
