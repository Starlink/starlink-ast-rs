//! Mapping that applies a transformation supplied by the caller.
//!
//! An `IntraMap` wraps a transformation the caller provides as an
//! [`IntraTransform`] (intramap.c). The transformation has a name, by which a
//! dump refers to it, and flags saying whether it has forward and inverse
//! directions and whether a forward and inverse pair simplifies away.
//!
//! # Registration
//!
//! C registers transformation functions with `astIntraReg` in one
//! process-wide table, and an IntraMap read from a dump finds its function
//! there by name. A transformation cannot be written to a dump, so a reader
//! still has to find it by name; here the table belongs to the Channel that
//! reads: [`IntraRegistry`] holds the transformations a Channel knows, added
//! by [`Channel::register_intra`](crate::channel::Channel::register_intra).
//! While a Channel reads, it lends its registry to the loaders through a
//! thread-local slot that only that read can see ([`with_registry`]), since
//! the loaders of the Mappings an IntraMap can sit inside see only their own
//! part of the dump. An IntraMap made in code holds its transformation
//! directly and needs no registry. Reading from a string,
//! [`from_native_string_with`](crate::from_native_string_with) takes a
//! registry in place of a Channel.
//!
//! The transformation is shared, as `Arc<dyn IntraTransform>`, because one
//! transformation serves every IntraMap that names it and the registries
//! that hold it.

use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::unit::UnitMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// The properties of an [`IntraTransform`] that C gives `astIntraReg` as its
/// `flags` argument.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IntraFlags {
    /// There is no forward transformation (C's `AST__NOFWD`).
    pub no_fwd: bool,
    /// There is no inverse transformation (C's `AST__NOINV`).
    pub no_inv: bool,
    /// The forward transformation followed by the inverse is the identity,
    /// so such a pair simplifies away (C's `AST__SIMPFI`).
    pub simp_fi: bool,
    /// The inverse transformation followed by the forward is the identity
    /// (C's `AST__SIMPIF`).
    pub simp_if: bool,
}

/// A transformation an [`IntraMap`] applies, supplied by the caller: what C
/// registers with `astIntraReg`.
pub trait IntraTransform: Send + Sync {
    /// The name a dump refers to the transformation by. White space in it is
    /// ignored, as C's `astIntraReg` removes it, and a name that is all white
    /// space is refused.
    fn name(&self) -> &str;

    /// The number of input coordinates the transformation takes, or `None`
    /// for any number (C's `AST__ANY`).
    fn nin(&self) -> Option<usize>;

    /// The number of output coordinates it gives, or `None` for any number.
    fn nout(&self) -> Option<usize>;

    /// Its flags.
    fn flags(&self) -> IntraFlags;

    /// A one-line description of what it does.
    fn purpose(&self) -> &str;

    /// Its author.
    fn author(&self) -> &str;

    /// How to contact the author.
    fn contact(&self) -> &str;

    /// Transform the points in `input` with the forward (`forward`) or
    /// inverse transformation into `out`, which has the IntraMap's number of
    /// coordinates in that direction. `map` is the IntraMap applying it, so
    /// its [`IntraMap::intra_flag`] can select a variant.
    fn transform(
        &self,
        map: &IntraMap,
        input: &PointSet,
        forward: bool,
        out: &mut PointSet,
    ) -> AstResult<()>;
}

/// The name of a transformation as C's `CleanName` (intramap.c) makes it,
/// with every white-space character removed; a name with nothing else is
/// AST__ITFNI.
fn clean_name(name: &str, caller: &str) -> AstResult<String> {
    let clean: String = name
        .chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'))
        .collect();
    if clean.is_empty() {
        return Err(AstError::new(
            ErrorCode::Itfni,
            format!("{caller}: Invalid blank transformation function name given."),
        ));
    }
    Ok(clean)
}

/// The transformations a Channel can give to the IntraMaps it reads, by
/// name.
#[derive(Clone, Default)]
pub struct IntraRegistry {
    entries: Vec<(String, Arc<dyn IntraTransform>)>,
}

impl IntraRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a transformation, as C's `astIntraReg` does. A name already held
    /// is refused with AST__MRITF unless it is the same transformation, and a
    /// blank name with AST__ITFNI.
    pub fn register(&mut self, transform: Arc<dyn IntraTransform>) -> AstResult<()> {
        let name = clean_name(transform.name(), "astIntraReg")?;
        if let Some((_, held)) = self.entries.iter().find(|(n, _)| *n == name) {
            if Arc::ptr_eq(held, &transform) {
                return Ok(());
            }
            return Err(AstError::new(
                ErrorCode::Mritf,
                format!(
                    "astIntraReg: Invalid attempt to register the transformation function \
                     name \"{name}\" multiple times."
                ),
            ));
        }
        self.entries.push((name, transform));
        Ok(())
    }

    /// The transformation registered under `name`, compared exactly as C's
    /// loader compares the name a dump holds.
    pub fn get(&self, name: &str) -> Option<Arc<dyn IntraTransform>> {
        self.entries
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, t)| Arc::clone(t))
    }
}

impl fmt::Debug for IntraRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.entries.iter().map(|(n, _)| n))
            .finish()
    }
}

thread_local! {
    /// The registry of the Channel reading on this thread, while it reads.
    static READING: RefCell<Option<IntraRegistry>> = const { RefCell::new(None) };
}

/// Run `f` with `registry` as the one IntraMap loaders on this thread find
/// transformations in, as a Channel does while it reads. The previous
/// registry, if any, is restored afterwards, even if `f` panics.
pub(crate) fn with_registry<T>(registry: &IntraRegistry, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<IntraRegistry>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            READING.with(|r| *r.borrow_mut() = previous);
        }
    }
    let previous = READING.with(|r| r.borrow_mut().replace(registry.clone()));
    let _restore = Restore(previous);
    f()
}

/// A Mapping that applies a transformation supplied by the caller.
#[derive(Clone)]
pub struct IntraMap {
    /// The transformation, shared with the registries and other IntraMaps
    /// that hold it.
    transform: Arc<dyn IntraTransform>,
    /// Its name as C's `CleanName` makes it.
    name: String,
    nin: usize,
    nout: usize,
    /// C's `IntraFlag` attribute, `None` while unset.
    intra_flag: Option<String>,
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
}

impl IntraMap {
    /// An IntraMap applying `transform` with `nin` inputs and `nout` outputs,
    /// as C's `astIntraMap` makes one: a number that the transformation fixes
    /// and that differs is AST__BADNI or AST__BADNO.
    pub fn new(transform: Arc<dyn IntraTransform>, nin: usize, nout: usize) -> AstResult<Self> {
        let name = clean_name(transform.name(), "astIntraMap")?;
        if let Some(n) = transform.nin()
            && n != nin
        {
            return Err(AstError::new(
                ErrorCode::Badni,
                format!(
                    "astInitIntraMap(IntraMap): Number of input coordinates ({nin}) does not \
                     match the number used by the \"{name}\" transformation function ({n})."
                ),
            ));
        }
        if let Some(n) = transform.nout()
            && n != nout
        {
            return Err(AstError::new(
                ErrorCode::Badno,
                format!(
                    "astInitIntraMap(IntraMap): Number of output coordinates ({nout}) does \
                     not match the number used by the \"{name}\" transformation function \
                     ({n})."
                ),
            ));
        }
        Ok(Self {
            transform,
            name,
            nin,
            nout,
            intra_flag: None,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        })
    }

    /// The name of the transformation, with its white space removed.
    pub fn transform_name(&self) -> &str {
        &self.name
    }

    /// The transformation.
    pub fn transformation(&self) -> &Arc<dyn IntraTransform> {
        &self.transform
    }

    /// The IntraFlag attribute: a string the transformation can read to
    /// select a variant, and which IntraMaps must share to be equal or to
    /// simplify together. `None` while unset.
    pub fn intra_flag(&self) -> Option<&str> {
        self.intra_flag.as_deref()
    }

    /// Set IntraFlag, discarding both simplified records, as C's
    /// `SetIntraFlag` does whatever the value.
    pub fn set_intra_flag(&mut self, flag: &str) {
        self.simplified.clear();
        self.intra_flag = Some(flag.to_string());
    }

    /// Whether IntraFlag is set (intramap.c `TestIntraFlag`).
    pub fn test_intra_flag(&self) -> bool {
        self.intra_flag.is_some()
    }

    /// Unset IntraFlag, discarding both simplified records (intramap.c
    /// `ClearIntraFlag`).
    pub fn clear_intra_flag(&mut self) {
        self.simplified.clear();
        self.intra_flag = None;
    }
}

impl fmt::Debug for IntraMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IntraMap")
            .field("transform", &self.name)
            .field("nin", &self.nin)
            .field("nout", &self.nout)
            .field("intra_flag", &self.intra_flag)
            .field("inverted", &self.inverted)
            .finish()
    }
}

impl Mapping for IntraMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    fn nin(&self) -> usize {
        if self.inverted { self.nout } else { self.nin }
    }

    fn nout(&self) -> usize {
        if self.inverted { self.nin } else { self.nout }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        let ncoord = if forward { self.nout() } else { self.nin() };
        let mut out = PointSet::zeros(pts.npoint(), ncoord);
        self.transform_into(pts, forward, &mut out)?;
        Ok(out)
    }

    /// intramap.c `Transform`: the transformation applied in the direction
    /// the Invert flag makes of `forward`, an error from it reported as one
    /// signalled by the named function.
    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        crate::mappings::validate_transform(self, pts, forward, Some(out))?;
        self.transform
            .transform(self, pts, forward != self.inverted, out)
            .map_err(|e| {
                AstError::new(
                    e.code(),
                    format!(
                        "astTransform(IntraMap): Error signalled by \"{}\" transformation \
                         function: {}",
                        self.name,
                        e.message()
                    ),
                )
            })
    }

    /// C's IntraMap inherits the base `Rate`, a numeric estimate.
    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        super::rate::numeric_rate(self, at, ax1, ax2)
    }

    /// The transformation's NOFWD and NOINV flags, which C gives
    /// `astInitMapping` (intramap.c `astInitIntraMap`).
    fn has_forward(&self) -> bool {
        let flags = self.transform.flags();
        if self.inverted {
            !flags.no_inv
        } else {
            !flags.no_fwd
        }
    }

    fn has_inverse(&self) -> bool {
        let flags = self.transform.flags();
        if self.inverted {
            !flags.no_fwd
        } else {
            !flags.no_inv
        }
    }

    /// C passes the flags to `astInitMapping`, so its dump writes them.
    fn dump_tran_flags(&self) -> (bool, bool) {
        let flags = self.transform.flags();
        (!flags.no_fwd, !flags.no_inv)
    }

    /// intramap.c `MapMerge`: in series, this IntraMap and the next Mapping,
    /// if that is an IntraMap of the same transformation with the same
    /// IntraFlag, become a UnitMap when together they keep the number of
    /// coordinates and the transformation's flags say that direction pair
    /// is the identity: forward then inverse with SIMPFI, inverse then
    /// forward with SIMPIF.
    fn map_merge(
        &self,
        ctx: &mut super::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        if !series || where_ + 1 >= ctx.len() {
            return None;
        }
        let (map1, invert1, _) = ctx.entry(where_);
        let (map2, invert2, _) = ctx.entry(where_ + 1);
        let first = map1.as_any().downcast_ref::<IntraMap>()?;
        let second = map2.as_any().downcast_ref::<IntraMap>()?;
        if first.name != second.name
            || first.intra_flag.as_deref().unwrap_or("")
                != second.intra_flag.as_deref().unwrap_or("")
        {
            return None;
        }
        // The numbers of coordinates the first takes and the second gives in
        // the orientations the merge list gives them.
        let nin1 = if *invert1 { first.nout } else { first.nin };
        let nout2 = if *invert2 { second.nin } else { second.nout };
        if nin1 != nout2 {
            return None;
        }
        let flags = first.transform.flags();
        let simpler = match (*invert1, *invert2) {
            (false, true) => flags.simp_fi,
            (true, false) => flags.simp_if,
            _ => false,
        };
        if !simpler {
            return None;
        }
        ctx.replace_pair_with(where_, Box::new(UnitMap::new(nin1)), false);
        Some(where_)
    }

    /// intramap.c `Equal`: the same numbers of coordinates, the same Invert
    /// flag and transformation, and an IntraFlag set on both and equal, so
    /// two IntraMaps without one are not equal.
    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(o) = other.as_any().downcast_ref::<IntraMap>() else {
            return false;
        };
        self.nin() == o.nin()
            && self.nout() == o.nout()
            && self.inverted == o.inverted
            && self.name == o.name
            && matches!((&self.intra_flag, &o.intra_flag), (Some(a), Some(b)) if a == b)
    }

    fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    fn set_ident(&mut self, ident: &str) {
        self.ident = Some(ident.to_string());
    }

    fn clear_ident(&mut self) {
        self.ident = None;
    }

    fn test_ident(&self) -> bool {
        self.ident.is_some()
    }

    fn id(&self) -> &str {
        self.id.get()
    }
    fn set_id(&mut self, id: &str) {
        self.id.set(id);
    }
    fn clear_id(&mut self) {
        self.id.clear();
    }
    fn test_id(&self) -> bool {
        self.id.test()
    }

    fn has_been_simplified(&self) -> bool {
        self.simplified.get(self.inverted)
    }

    fn set_has_been_simplified(&mut self, flag: bool) {
        self.simplified.set(self.inverted, flag);
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

impl Serializable for IntraMap {
    fn class_name(&self) -> &'static str {
        "IntraMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new(self.class_name());
        node.class_comment = Some("Map points using a private transformation function".to_string());
        write_mapping_base(&mut node, self);
        node.push_string(
            "Fname",
            &self.name,
            Some("Name of transformation function".to_string()),
        );
        node.write_string(
            "Iflag",
            self.intra_flag.is_some(),
            false,
            self.intra_flag.as_deref().unwrap_or(""),
            "IntraMap identification string",
        );
        node.push_string(
            "Purp",
            self.transform.purpose(),
            Some("Purpose of function".to_string()),
        );
        node.push_string(
            "Auth",
            self.transform.author(),
            Some("Author's name".to_string()),
        );
        node.push_string(
            "Cntact",
            self.transform.contact(),
            Some("Contact address".to_string()),
        );
        Ok(node)
    }

    /// intramap.c `astLoadIntraMap`: the transformation is the one the
    /// reading Channel's registry holds under the dump's `Fname`, and
    /// AST__URITF when it holds none.
    fn from_node(node: &Node) -> AstResult<Self> {
        let whole = node;
        let node = &whole.section("IntraMap");
        let base = read_mapping_base(whole);
        let fname = node.get_string_if_set("Fname").unwrap_or_default();
        let text = |key: &str| node.get_string_if_set(key).unwrap_or_default();
        let transform = READING
            .with(|r| r.borrow().as_ref().and_then(|reg| reg.get(&fname)))
            .ok_or_else(|| {
                AstError::new(
                    ErrorCode::Uritf,
                    format!(
                        "astLoadIntraMap(IntraMap): An IntraMap was read which uses an unknown \
                         transformation function. This is a private extension to the AST \
                         library: to handle it, you must obtain the source code from its \
                         author. You can then register it with AST in your software and \
                         re-read the IntraMap. Function name: \"{fname}\". Purpose: \"{}\". \
                         Author: \"{}\". Contact address: \"{}\".",
                        text("Purp"),
                        text("Auth"),
                        text("Cntact")
                    ),
                )
            })?;
        let mut m = IntraMap {
            transform,
            name: fname,
            nin: base.nin,
            nout: base.nout,
            intra_flag: node.get_string_if_set("Iflag"),
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
        };
        if base.inverted {
            m.invert();
        }
        m.simplified = SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified);
        Ok(m)
    }
}
