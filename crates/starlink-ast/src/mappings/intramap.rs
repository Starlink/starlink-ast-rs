// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Mapping that applies a transformation supplied by the caller.
//!
//! An [`IntraMap`] wraps a transformation the caller provides as an
//! [`IntraTransform`].
//! The transformation has a name, by which a dump refers to it, and flags
//! saying whether it has forward and inverse directions and whether a forward
//! and inverse pair simplifies away.
//!
//! # Registration
//!
//! A transformation cannot be written to a dump, so a reader has to find it
//! by name.
//! The names a reader knows belong to the Channel that reads:
//! [`IntraRegistry`] holds the transformations a Channel knows, added by
//! [`Channel::register_intra`](crate::channel::Channel::register_intra).
//! While a Channel reads, it lends its registry to the loaders through a
//! thread-local slot that only that read can see, since the loaders of the
//! Mappings an IntraMap can sit inside see only their own part of the dump.
//! An IntraMap made in code holds its transformation directly and needs no
//! registry.
//! Reading from a string,
//! [`from_native_string_with`](crate::from_native_string_with) takes a
//! registry in place of a Channel.
//!
//! The transformation is shared, as `Arc<dyn IntraTransform>`, because one
//! transformation serves every IntraMap that names it and the registries
//! that hold it.

// C registers transformation functions with astIntraReg in one process-wide
// table (intramap.c), and an IntraMap read from a dump finds its function
// there by name; the port keeps that table per Channel. The thread-local slot
// is `with_registry`.

use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;

use crate::coords::{Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::unit::UnitMap;
use super::{DynMapping, Mapping};
use crate::mappings::SimplifiedRecord;

/// Flags describing an [`IntraTransform`], which may affect the behavior of
/// any [`IntraMap`] that uses it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IntraFlags {
    /// The transformation does not implement a forward transformation.
    ///
    /// An IntraMap which uses it has no forward transformation
    /// ([`has_forward`](Mapping::has_forward) is false), and the
    /// transformation is never called with `forward` true.
    pub no_fwd: bool,
    /// The transformation does not implement an inverse transformation.
    ///
    /// An IntraMap which uses it has no inverse transformation
    /// ([`has_inverse`](Mapping::has_inverse) is false), and the
    /// transformation is never called with `forward` false.
    pub no_inv: bool,
    /// Applying the forward transformation followed immediately by the
    /// matching inverse always restores the original coordinates.
    ///
    /// Simplification may then replace such a pair of IntraMaps in series by
    /// a [`UnitMap`].
    /// Both transformations need not actually be implemented.
    pub simp_fi: bool,
    /// Applying the inverse transformation followed immediately by the
    /// matching forward transformation always restores the original
    /// coordinates.
    ///
    /// Simplification may then replace such a pair of IntraMaps in series by
    /// a [`UnitMap`].
    pub simp_if: bool,
}

/// A privately-defined coordinate transformation, supplied by the caller,
/// that an [`IntraMap`] applies.
///
/// The purpose, author and contact details are written to a dump with any
/// IntraMap that uses the transformation, so that a user who receives the
/// dump without the transformation can find out how to obtain it.
pub trait IntraTransform: Send + Sync {
    /// The name a dump refers to the transformation by.
    ///
    /// White space in it is removed, and a name that is all white space is
    /// refused.
    fn name(&self) -> &str;

    /// The number of input coordinates the transformation takes, or `None`
    /// if it accepts any number.
    // `None` is C's AST__ANY.
    fn nin(&self) -> Option<usize>;

    /// The number of output coordinates it gives, or `None` if it can give any
    /// number.
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
    /// coordinates in that direction.
    ///
    /// Both hold one slice per axis.
    /// `map` is the IntraMap applying it, so its [`IntraMap::intra_flag`] can
    /// select a variant.
    /// An error returned here ends the operation that called the IntraMap and
    /// is passed on to its caller; `ErrorCode::Itfer` is available for the
    /// purpose, but any code may be used.
    fn transform(
        &self,
        map: &IntraMap,
        input: &Points<'_>,
        forward: bool,
        out: &mut PointsMut<'_>,
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
///
/// A dump of an IntraMap does not include its transformation, only the
/// transformation's name.
/// A program can therefore read it only if the transformation has been
/// registered under the same name with the Channel that reads it
/// ([`Channel::register_intra`](crate::channel::Channel::register_intra)),
/// or in the registry passed to
/// [`from_native_string_with`](crate::from_native_string_with); otherwise the
/// read fails.
#[derive(Clone, Default)]
pub struct IntraRegistry {
    entries: Vec<(String, Arc<dyn IntraTransform>)>,
}

impl IntraRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a transformation under its
    /// [`name`](IntraTransform::name), so that IntraMaps read with this
    /// registry can use it.
    ///
    /// White space in the name is removed; the name is case-sensitive.
    /// The same transformation may be registered more than once, which lets
    /// registration be done independently at several places in a program,
    /// but a different transformation cannot take over a name already held.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Itfni` if the name is blank.
    /// - `ErrorCode::Mritf` if the name is already held by a different
    ///   transformation (not the same `Arc`).
    #[doc(alias = "astIntraReg")]
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

    /// The transformation registered under `name`, or `None` when none is.
    ///
    /// The name is compared exactly, so it must match the name [`register`](Self::register) stored, which has every white-space character removed.
    // This is the comparison C's IntraMap loader makes with the name a dump holds.
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

/// Map points using a private transformation function.
///
/// An IntraMap is a specialized form of Mapping which encapsulates a
/// privately-defined coordinate transformation, an [`IntraTransform`], so that
/// it may be used like any other Mapping.
/// This allows you to create Mappings that perform any conceivable coordinate
/// transformation.
///
/// An IntraMap is intended for use within a single program or a private suite
/// of software, where all programs have access to the same transformations.
/// IntraMaps should not normally be stored in datasets which may be exported
/// for processing by other software, since that software will not have the
/// transformation available, and reading the dataset will fail there; see
/// [`IntraRegistry`].
///
/// [`intra_flag`](IntraMap::intra_flag) distinguishes one IntraMap from
/// others that use the same transformation.
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
    /// Create an IntraMap applying `transform` with `nin` inputs and `nout`
    /// outputs.
    ///
    /// `nin` and `nout` must be compatible with the numbers of coordinates the
    /// transformation accepts and produces
    /// ([`IntraTransform::nin`] and [`IntraTransform::nout`]).
    /// The transformation does not need to be registered.
    ///
    /// ```
    /// use std::sync::Arc;
    /// use starlink_ast::coords::{Points, PointsMut};
    /// use starlink_ast::error::AstResult;
    /// use starlink_ast::mappings::{IntraFlags, IntraMap, IntraTransform, Mapping};
    ///
    /// struct Double;
    /// impl IntraTransform for Double {
    ///     fn name(&self) -> &str { "Double" }
    ///     fn nin(&self) -> Option<usize> { None }
    ///     fn nout(&self) -> Option<usize> { None }
    ///     fn flags(&self) -> IntraFlags { IntraFlags::default() }
    ///     fn purpose(&self) -> &str { "Multiply by two" }
    ///     fn author(&self) -> &str { "A. Author" }
    ///     fn contact(&self) -> &str { "author@example.org" }
    ///     fn transform(&self, _map: &IntraMap, input: &Points<'_>, forward: bool,
    ///                  out: &mut PointsMut<'_>) -> AstResult<()> {
    ///         let f = if forward { 2.0 } else { 0.5 };
    ///         for j in 0..input.ncoord() {
    ///             for (o, i) in out.axis_mut(j).iter_mut().zip(input.axis(j)) {
    ///                 *o = i * f;
    ///             }
    ///         }
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let map = IntraMap::new(Arc::new(Double), 1, 1).unwrap();
    /// assert_eq!(map.tran1(&[3.0], true).unwrap(), vec![6.0]);
    /// assert_eq!(map.tran1(&[3.0], false).unwrap(), vec![1.5]);
    /// ```
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Itfni` if the transformation's name is blank.
    /// - `ErrorCode::Badni` if the transformation fixes its number of inputs
    ///   and `nin` differs.
    /// - `ErrorCode::Badno` if the transformation fixes its number of outputs
    ///   and `nout` differs.
    #[doc(alias = "astIntraMap")]
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

    /// A string which distinguishes this IntraMap from other IntraMaps.
    ///
    /// The transformation may enquire this value and adapt the transformation
    /// it provides to the particular IntraMap involved.
    /// Numerical values encoded as a string can be passed this way, though it
    /// is not suited to large amounts of data.
    ///
    /// A pair of IntraMaps whose transformations could cancel is simplified to
    /// a [`UnitMap`] only if their values are the same (compared
    /// case-sensitively, with an unset value reading as an empty string).
    ///
    /// Default: `None` (unset).
    ///
    /// [`set_intra_flag`](IntraMap::set_intra_flag),
    /// [`test_intra_flag`](IntraMap::test_intra_flag) and
    /// [`clear_intra_flag`](IntraMap::clear_intra_flag) set, test and clear
    /// it.
    #[doc(alias = "IntraFlag")]
    pub fn intra_flag(&self) -> Option<&str> {
        self.intra_flag.as_deref()
    }

    /// Set [`intra_flag`](IntraMap::intra_flag).
    ///
    /// This clears the record that the IntraMap has been simplified, whatever
    /// the value.
    pub fn set_intra_flag(&mut self, flag: &str) {
        // As C's SetIntraFlag, whatever the value.
        self.simplified.clear();
        self.intra_flag = Some(flag.to_string());
    }

    /// Whether [`intra_flag`](IntraMap::intra_flag) is set.
    pub fn test_intra_flag(&self) -> bool {
        self.intra_flag.is_some()
    }

    /// Clear [`intra_flag`](IntraMap::intra_flag).
    ///
    /// This also clears the record that the IntraMap has been simplified.
    pub fn clear_intra_flag(&mut self) {
        // intramap.c ClearIntraFlag.
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

    // intramap.c `Transform`: the transformation applied in the direction
    // the Invert flag makes of `forward`, an error from it reported as one
    // signaled by the named function.
    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        self.transform
            .transform(self, &pts, forward != self.inverted, &mut out)
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

    /// A numerical estimate of the rate, as [`Mapping::rate`] describes for any
    /// Mapping without an analytic one.
    // C's IntraMap inherits the base Rate.
    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        super::rate::numeric_rate(self, at, ax1, ax2)
    }

    // The transformation's NOFWD and NOINV flags, which C gives
    // `astInitMapping` (intramap.c `astInitIntraMap`).
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

    // C passes the flags to `astInitMapping`, so its dump writes them.
    fn dump_tran_flags(&self) -> (bool, bool) {
        let flags = self.transform.flags();
        (!flags.no_fwd, !flags.no_inv)
    }

    // intramap.c `MapMerge`: in series, this IntraMap and the next Mapping,
    // if that is an IntraMap of the same transformation with the same
    // IntraFlag, become a UnitMap when together they keep the number of
    // coordinates and the transformation's flags say that direction pair
    // is the identity: forward then inverse with SIMPFI, inverse then
    // forward with SIMPIF.
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

    // intramap.c `Equal`: the same numbers of coordinates, the same Invert
    // flag and transformation, and an IntraFlag set on both and equal, so
    // two IntraMaps without one are not equal.
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
