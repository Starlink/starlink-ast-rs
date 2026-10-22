//! KeyMaps: sets of values, each stored under a string key.
//!
//! See [`KeyMap`] for keys, the value types and the conversions between
//! them.

// Ported from reference/starlink-ast/src/keymap.c.
//
// Keys are stored in a std::collections::HashMap rather than C's chained
// bucket table. C's table arithmetic survives only in `table`, which
// reconstructs the `MapSz` card at dump time.

pub(crate) mod convert;
pub(crate) mod entry;
pub(crate) mod serial;
pub(crate) mod sort;
pub(crate) mod table;

use crate::error::{AstError, AstResult, ErrorCode};
use crate::keymap::convert::{
    RawValue, unwrap_byte, unwrap_double, unwrap_float, unwrap_int, unwrap_kint, unwrap_object,
    unwrap_sint, unwrap_string,
};
use crate::keymap::entry::{
    EntryValue, MapEntry, Scalar1D, is_empty_vector, promote_scalar_to_vector, set_element,
    single_element_vector, single_key_map, single_key_map_mut,
};
use crate::keymap::table::{MAX_ENTRIES_PER_TABLE_ENTRY, MIN_TABLE_SIZE};
use crate::serial::AnyObject;
use std::collections::HashMap;

pub use crate::keymap::entry::AstType;
pub use crate::keymap::sort::SortBy;

/// The `SizeGuess` value reported when the attribute is unset.
///
/// This is `MIN_TABLE_SIZE * MAX_ENTRIES_PER_TABLE_ENTRY`: the number of
/// entries the smallest table holds before C would double it.
const DEFAULT_SIZE_GUESS: usize = MIN_TABLE_SIZE * MAX_ENTRIES_PER_TABLE_ENTRY;

/// A set of values, each stored under a string key.
///
/// A KeyMap takes a key and gives back the value stored under it.
/// It is not a [`Mapping`](crate::mappings::Mapping), and the Mapping
/// methods do not apply to it.
///
/// # Keys
///
/// Keys are strings, and trailing spaces in them are ignored, so `"RA"`
/// and `"RA  "` name the same entry.
/// Keys are case sensitive unless [KeyCase](Self::key_case) is false, in
/// which case every key is converted to upper case before it is used.
///
/// # Values
///
/// Each value is a scalar or a one-dimensional vector of at least one
/// element, of one of these types:
///
/// | Type | Methods | [`AstType`] |
/// |---|---|---|
/// | `i32` | [`put_0i`](Self::put_0i), [`get_0i`](Self::get_0i), ... | `Int` |
/// | `i16` | [`put_0s`](Self::put_0s), [`get_0s`](Self::get_0s), ... | `SInt` |
/// | `i64` | [`put_0k`](Self::put_0k), [`get_0k`](Self::get_0k), ... | `KInt` |
/// | `u8` | [`put_0b`](Self::put_0b), [`get_0b`](Self::get_0b), ... | `Byte` |
/// | `f32` | [`put_0f`](Self::put_0f), [`get_0f`](Self::get_0f), ... | `Float` |
/// | `f64` | [`put_0d`](Self::put_0d), [`get_0d`](Self::get_0d), ... | `Double` |
/// | `String` | [`put_0c`](Self::put_0c), [`get_0c`](Self::get_0c), ... | `String` |
/// | [`AnyObject`] | [`put_0a`](Self::put_0a), [`get_0a`](Self::get_0a), ... | `Object` |
///
/// The `put_0` methods store a scalar, the `put_1` methods a vector, and
/// the `put_elem` methods one element of a vector.
/// The `get_0`, `get_1` and `get_elem` methods read a scalar, a vector and
/// one element.
/// Storing a value under a key replaces whatever was stored there before,
/// of whatever type.
/// A key can also be stored with no value, by [`put_u`](Self::put_u).
/// Each entry may carry a comment, which [`map_comment`](Self::map_comment)
/// returns.
///
/// # Reading values
///
/// A value can be read as any type, not only the one it was stored as:
///
/// - A floating-point value read as an integer type is rounded to the
///   nearest integer, and any value outside the range of an integer type
///   is clamped to that type's limits.
///   A NaN reads as 0.
/// - An `f64` read as an `f32` is rounded to the nearest `f32`, and one
///   beyond its range, [`BAD_VALUE`](crate::constants::BAD_VALUE) included,
///   reads as an infinity.
/// - A number read as a string is formatted as a decimal number, in
///   exponent notation such as `1e+20` for very large or small values:
///   an `f32` with up to 6 significant digits, and an `f64` with up to 17, or up to 14 when
///   that gives a string at least ten characters shorter.
///   [`BAD_VALUE`](crate::constants::BAD_VALUE) formats as `"<bad>"`.
/// - A string read as a number must hold nothing but the number and
///   surrounding white space.
///   A floating-point number may be written in hexadecimal, as in
///   `0x1.8p1`, and one read as an integer type is rounded.
///   `"<bad>"` reads as [`BAD_VALUE`](crate::constants::BAD_VALUE) when
///   read as an `f64`.
/// - An Object can be read only as an Object, and no other value can be
///   read as one.
///
/// A conversion that is not possible is an [`ErrorCode::Mpger`] error.
/// A vector read as a scalar gives its first element, and a scalar read
/// as a vector gives a vector of length 1.
/// A read of a key that is not in the KeyMap gives `Ok(None)`, or an
/// [`ErrorCode::Mpker`] error when [KeyError](Self::key_error) is true.
///
/// # Objects as values
///
/// Any AST Object can be stored, a KeyMap included.
/// The KeyMap owns what it stores, and a read returns a copy, so changing
/// the copy does not change the stored Object; store the changed copy to
/// replace it.
/// Setting [KeyError](Self::key_error) or [MapLocked](Self::map_locked)
/// also sets it in every KeyMap held in this one, and
/// [`map_copy`](Self::map_copy) merges a KeyMap held under a key into the
/// one held under the same key in the destination.
///
/// # Attributes
///
/// - [KeyCase](Self::key_case): whether keys are case sensitive.
/// - [KeyError](Self::key_error): whether reading a missing key is an
///   error.
/// - [MapLocked](Self::map_locked): whether new keys are refused.
/// - [SizeGuess](Self::size_guess): the expected number of entries.
/// - [SortBy](Self::sort_by): the order in which keys are returned.
///
/// # Examples
///
/// ```
/// use ast_core::KeyMap;
///
/// let mut km = KeyMap::new();
/// km.put_0i("NAXIS", 2, Some("number of axes"))?;
/// km.put_1d("CRVAL", &[10.5, -30.25], None)?;
///
/// assert_eq!(km.map_size(), 2);
/// assert_eq!(km.get_0d("NAXIS")?, Some(2.0));
/// assert_eq!(km.get_0c("NAXIS")?.as_deref(), Some("2"));
/// assert_eq!(km.get_1d("CRVAL")?, Some(vec![10.5, -30.25]));
/// assert_eq!(km.get_0d("CRVAL")?, Some(10.5));
/// assert_eq!(km.map_get_c("CRVAL")?.as_deref(), Some("(10.5,-30.25)"));
/// assert_eq!(km.get_0i("CRPIX")?, None);
/// # Ok::<(), ast_core::error::AstError>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct KeyMap {
    /// Keys are normalized on the way in, so the `String` key is directly
    /// comparable and `HashMap` equality agrees with `KeyCmp`.
    entries: HashMap<String, MapEntry>,
    /// The source of both age counters. Incremented twice when a new key
    /// is added, matching `AddTableEntry`, so the dumped `MemCnt` is
    /// twice the number of keys for a map built by fresh puts.
    member_count: u64,
    sizeguess: Option<usize>,
    key_case: Option<bool>,
    key_error: Option<bool>,
    map_locked: Option<bool>,
    sort_by: Option<SortBy>,
    /// The Object attribute Ident (object.c), which every AST Object has.
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The cursor [`KeyMap::map_iterate`] resumes from. An index into
    /// [`KeyMap::sorted_keys`] rather than C's saved hash-table/list
    /// position, which is what keeps a walk bounded even when `SortBy`
    /// is set to a mode whose C sorted list is circular.
    iter_index: usize,
}

impl KeyMap {
    /// Creates an empty KeyMap with every attribute unset.
    ///
    /// Entries are added with the `put` methods, such as
    /// [`put_0i`](Self::put_0i) and [`put_1i`](Self::put_1i), and attributes
    /// are set with their setters, such as [`set_key_case`](Self::set_key_case).
    /// [`with_size_guess`](Self::with_size_guess) creates one with
    /// [SizeGuess](Self::size_guess) set.
    #[doc(alias = "astKeyMap")]
    pub fn new() -> Self {
        KeyMap::default()
    }

    /// Creates an empty KeyMap with [SizeGuess](Self::size_guess) set to
    /// `sizeguess`, its storage sized to match.
    pub fn with_size_guess(sizeguess: usize) -> Self {
        KeyMap {
            entries: HashMap::with_capacity(sizeguess),
            sizeguess: Some(sizeguess),
            ..KeyMap::default()
        }
    }

    /// The number of entries in the KeyMap, those with no value included.
    #[doc(alias = "astMapSize")]
    pub fn map_size(&self) -> usize {
        self.entries.len()
    }

    /// The number of entries the KeyMap is expected to hold (the SizeGuess
    /// attribute).
    ///
    /// The KeyMap's storage is sized for this many entries when it is set.
    /// A larger value can make adding entries quicker, at the cost of
    /// memory, but has no other effect.
    /// It can be changed only while the KeyMap is empty, and is most
    /// conveniently given when the KeyMap is created, with
    /// [`with_size_guess`](Self::with_size_guess).
    ///
    /// Default: 160.
    #[doc(alias = "SizeGuess")]
    pub fn size_guess(&self) -> usize {
        self.sizeguess.unwrap_or(DEFAULT_SIZE_GUESS)
    }

    /// Sets [SizeGuess](Self::size_guess).
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Nowrt`] if the KeyMap holds any entries.
    pub fn set_size_guess(&mut self, sizeguess: usize) -> AstResult<()> {
        if !self.entries.is_empty() {
            return Err(AstError::new(
                ErrorCode::Nowrt,
                "astSetAttrib(KeyMap): Illegal attempt to change the SizeGuess \
                 attribute of a non-empty KeyMap.",
            ));
        }
        self.sizeguess = Some(sizeguess);
        self.entries.reserve(sizeguess);
        Ok(())
    }

    /// Clears [SizeGuess](Self::size_guess).
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Nowrt`] if the KeyMap holds any entries.
    pub fn clear_size_guess(&mut self) -> AstResult<()> {
        if !self.entries.is_empty() {
            return Err(AstError::new(
                ErrorCode::Nowrt,
                "astClearAttrib(KeyMap): Illegal attempt to clear the SizeGuess \
                 attribute of a non-empty KeyMap.",
            ));
        }
        self.sizeguess = None;
        Ok(())
    }

    /// Whether [SizeGuess](Self::size_guess) is set.
    pub fn test_size_guess(&self) -> bool {
        self.sizeguess.is_some()
    }

    /// Whether keys are case sensitive (the KeyCase attribute).
    ///
    /// When true, keys are used as they are given.
    /// When false, every key given to any method is converted to upper
    /// case (its ASCII letters) before it is used, so `"Ra"` and `"RA"` name
    /// the same entry and the keys the KeyMap returns are in upper case.
    /// A key may then have no more than 200 characters, not counting
    /// trailing white space.
    ///
    /// It can be changed only while the KeyMap is empty.
    ///
    /// Default: true.
    #[doc(alias = "KeyCase")]
    pub fn key_case(&self) -> bool {
        self.key_case.unwrap_or(true)
    }

    /// Sets [KeyCase](Self::key_case).
    ///
    /// Setting the value [`key_case`](Self::key_case) already gives is
    /// always allowed.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Nowrt`] if the KeyMap holds any entries and
    ///   `key_case` differs from the current value; the attribute is then
    ///   unchanged.
    pub fn set_key_case(&mut self, key_case: bool) -> AstResult<()> {
        if key_case != self.key_case() && !self.entries.is_empty() {
            return Err(AstError::new(
                ErrorCode::Nowrt,
                "astSetAttrib(KeyMap): Illegal attempt to change the KeyCase \
                 attribute of a non-empty KeyMap.",
            ));
        }
        self.key_case = Some(key_case);
        Ok(())
    }

    /// Clears [KeyCase](Self::key_case).
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Nowrt`] if the KeyMap holds any entries and clearing
    ///   would change the value [`key_case`](Self::key_case) gives; the
    ///   attribute is then unchanged.
    pub fn clear_key_case(&mut self) -> AstResult<()> {
        let old = self.key_case();
        self.key_case = None;
        if self.key_case() != old && !self.entries.is_empty() {
            self.key_case = Some(old);
            return Err(AstError::new(
                ErrorCode::Nowrt,
                "astClearAttrib(KeyMap): Illegal attempt to clear the KeyCase \
                 attribute of a non-empty KeyMap.",
            ));
        }
        Ok(())
    }

    /// Whether [KeyCase](Self::key_case) is set.
    pub fn test_key_case(&self) -> bool {
        self.key_case.is_some()
    }

    /// Whether reading a key that is not in the KeyMap is an error (the
    /// KeyError attribute).
    ///
    /// When false, the `get_0`, `get_1` and `get_elem` methods give
    /// `Ok(None)` for a missing key, and [`map_defined`](Self::map_defined)
    /// gives `Ok(false)`.
    /// When true, they fail with [`ErrorCode::Mpker`] instead.
    /// A key stored with no value (see [`put_u`](Self::put_u)) is not
    /// missing, and reads as `Ok(None)` either way.
    /// [`map_get_c`](Self::map_get_c), [`map_has_key`](Self::map_has_key),
    /// [`map_length`](Self::map_length), [`map_len_c`](Self::map_len_c) and
    /// [`map_type`](Self::map_type) do not consult it.
    ///
    /// Setting or clearing it also sets or clears it in every KeyMap held
    /// in this one, at any depth, but not in a KeyMap stored afterwards.
    ///
    /// Default: false.
    #[doc(alias = "KeyError")]
    pub fn key_error(&self) -> bool {
        self.key_error.unwrap_or(false)
    }

    /// Sets [KeyError](Self::key_error), here and in every KeyMap held in
    /// this one.
    pub fn set_key_error(&mut self, key_error: bool) {
        self.key_error = Some(key_error);
        for nested in self.nested_key_maps_mut() {
            nested.set_key_error(key_error);
        }
    }

    /// Clears [KeyError](Self::key_error), here and in every KeyMap held in
    /// this one.
    pub fn clear_key_error(&mut self) {
        self.key_error = None;
        for nested in self.nested_key_maps_mut() {
            nested.clear_key_error();
        }
    }

    /// Whether [KeyError](Self::key_error) is set.
    pub fn test_key_error(&self) -> bool {
        self.key_error.is_some()
    }

    /// Whether new keys are refused (the MapLocked attribute).
    ///
    /// When true, storing a value under a key the KeyMap does not already
    /// hold fails with [`ErrorCode::Badkey`], whether by a `put` or
    /// `put_elem` method, [`map_copy`](Self::map_copy),
    /// [`map_copy_entry`](Self::map_copy_entry) or
    /// [`map_rename`](Self::map_rename).
    /// New values can still be stored under the keys already held, and
    /// entries can be removed.
    ///
    /// Setting or clearing it also sets or clears it in every KeyMap held
    /// in this one, at any depth, but not in a KeyMap stored afterwards.
    ///
    /// Default: false.
    #[doc(alias = "MapLocked")]
    pub fn map_locked(&self) -> bool {
        self.map_locked.unwrap_or(false)
    }

    /// Sets [MapLocked](Self::map_locked), here and in every KeyMap held in
    /// this one.
    pub fn set_map_locked(&mut self, map_locked: bool) {
        self.map_locked = Some(map_locked);
        for nested in self.nested_key_maps_mut() {
            nested.set_map_locked(map_locked);
        }
    }

    /// Clears [MapLocked](Self::map_locked), here and in every KeyMap held
    /// in this one.
    pub fn clear_map_locked(&mut self) {
        self.map_locked = None;
        for nested in self.nested_key_maps_mut() {
            nested.clear_map_locked();
        }
    }

    /// Whether [MapLocked](Self::map_locked) is set.
    pub fn test_map_locked(&self) -> bool {
        self.map_locked.is_some()
    }

    /// The order in which [`map_key`](Self::map_key) and
    /// [`map_iterate`](Self::map_iterate) return keys (the SortBy
    /// attribute).
    ///
    /// [`SortBy`] lists the orders.
    /// A new value applies to the entries already held as well as to those
    /// added later.
    /// Unlike KeyError and MapLocked, it is not passed on to the KeyMaps held
    /// in this one.
    ///
    /// Default: [`SortBy::None`].
    #[doc(alias = "SortBy")]
    pub fn sort_by(&self) -> SortBy {
        self.sort_by.unwrap_or_default()
    }

    /// Sets [SortBy](Self::sort_by).
    pub fn set_sort_by(&mut self, sort_by: SortBy) {
        self.sort_by = Some(sort_by);
    }

    /// Clears [SortBy](Self::sort_by).
    pub fn clear_sort_by(&mut self) {
        self.sort_by = None;
    }

    /// Whether [SortBy](Self::sort_by) is set.
    pub fn test_sort_by(&self) -> bool {
        self.sort_by.is_some()
    }

    /// The Ident attribute: a permanent string identifying this KeyMap.
    ///
    /// A clone keeps it, and it is written to and read back from a Native
    /// dump.
    /// Default: an empty string.
    // As astGetIdent (object.c).
    #[doc(alias = "Ident")]
    pub fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    /// Set [`ident`](Self::ident).
    ///
    /// A blank Ident counts as set.
    // As in C, a blank value counts as set.
    pub fn set_ident(&mut self, ident: &str) {
        self.ident = Some(ident.to_string());
    }

    /// Clear [`ident`](Self::ident).
    pub fn clear_ident(&mut self) {
        self.ident = None;
    }

    /// The ID attribute: a string identifying this KeyMap, which the library
    /// does not use.
    ///
    /// It is written to and read back from a Native dump, but unlike
    /// [`ident`](Self::ident) a clone does not keep it.
    /// Default: an empty string.
    // C's astCopy does not copy ID either.
    #[doc(alias = "ID")]
    pub fn id(&self) -> &str {
        self.id.get()
    }

    /// Set [`id`](Self::id).
    pub fn set_id(&mut self, id: &str) {
        self.id.set(id);
    }

    /// Whether [`id`](Self::id) is set.
    pub fn test_id(&self) -> bool {
        self.id.test()
    }

    /// Clear [`id`](Self::id).
    pub fn clear_id(&mut self) {
        self.id.clear();
    }

    /// Whether [`ident`](Self::ident) has been set.
    pub fn test_ident(&self) -> bool {
        self.ident.is_some()
    }
    /// Every KeyMap held directly by this one, scalar or vector element.
    ///
    /// Mirrors the walk C's `SetKeyError`/`SetMapLocked` and their `Clear`
    /// twins run over every table entry: check `type == AST__OBJECTTYPE`,
    /// then test each held Object with `astIsAKeyMap` (`keymap.c:1250`
    /// onward).
    pub(crate) fn nested_key_maps_mut(&mut self) -> Vec<&mut KeyMap> {
        self.entries
            .values_mut()
            .filter_map(|entry| match &mut entry.value {
                EntryValue::Object(objs) => Some(objs.as_mut_slice()),
                _ => None,
            })
            .flatten()
            .filter_map(|obj| match obj.as_mut() {
                Some(any) => match any.as_mut() {
                    AnyObject::KeyMap(km) => Some(km.as_mut()),
                    _ => None,
                },
                None => None,
            })
            .collect()
    }

    /// Every Object held directly by this KeyMap, scalar or vector element,
    /// to change where it is stored, as C changes an Object through the
    /// pointer `astMapGet0A` returns.
    pub(crate) fn objects_mut(&mut self) -> Vec<&mut AnyObject> {
        self.entries
            .values_mut()
            .filter_map(|entry| match &mut entry.value {
                EntryValue::Object(objs) => Some(objs.as_mut_slice()),
                _ => None,
            })
            .flatten()
            .filter_map(|obj| obj.as_deref_mut())
            .collect()
    }

    /// The Object stored as the first value of `key`, to change where it is
    /// stored, or `None` if `key` holds no Object.
    pub(crate) fn object_mut(&mut self, key: &str) -> Option<&mut AnyObject> {
        let nkey = table::convert_key(key, self.key_case(), "astMapGet0A").ok()?;
        let entry = self.entries.get_mut(nkey.trim_end_matches(' '))?;
        match &mut entry.value {
            EntryValue::Object(objs) => objs.as_mut_slice().first_mut()?.as_deref_mut(),
            _ => None,
        }
    }

    /// Stores the scalar `value` under `key`, with an optional comment.
    ///
    /// Any entry already stored under `key`, of whatever type, is replaced.
    /// Each value type has its own method:
    ///
    /// | Method | Value | Stored as |
    /// |---|---|---|
    /// | `put_0i` | `i32` | [`AstType::Int`] |
    /// | [`put_0s`](Self::put_0s) | `i16` | [`AstType::SInt`] |
    /// | [`put_0k`](Self::put_0k) | `i64` | [`AstType::KInt`] |
    /// | [`put_0b`](Self::put_0b) | `u8` | [`AstType::Byte`] |
    /// | [`put_0f`](Self::put_0f) | `f32` | [`AstType::Float`] |
    /// | [`put_0d`](Self::put_0d) | `f64` | [`AstType::Double`] |
    /// | [`put_0c`](Self::put_0c) | `&str` | [`AstType::String`] |
    /// | [`put_0a`](Self::put_0a) | [`AnyObject`] | [`AstType::Object`] |
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Badkey`] if `key` is not already in the KeyMap and
    ///   [MapLocked](Self::map_locked) is true; the KeyMap is then unchanged.
    /// - [`ErrorCode::Bigkey`] if [KeyCase](Self::key_case) is false and
    ///   `key` has more than 200 characters.
    ///
    /// # See also
    ///
    /// `astMapPut0<X>`, one function per data type `<X>`, where `<X>` is the last
    /// letter of the matching method in upper case (`I` for the `i` method).
    /// Its generic pointer type `P` has no counterpart.
    #[doc(alias = "astMapPut0")]
    pub fn put_0i(&mut self, key: &str, value: i32, comment: Option<&str>) -> AstResult<()> {
        self.put_scalar(
            key,
            EntryValue::Int(Scalar1D::Scalar(value)),
            comment,
            "astMapPut0I",
        )
    }

    /// Stores the scalar `value` under `key`, with an optional comment, as
    /// [`put_0i`](Self::put_0i) does for an `i32`.
    ///
    /// # Errors
    ///
    /// As for [`put_0i`](Self::put_0i).
    #[doc(alias = "astMapPut0")]
    pub fn put_0s(&mut self, key: &str, value: i16, comment: Option<&str>) -> AstResult<()> {
        self.put_scalar(
            key,
            EntryValue::SInt(Scalar1D::Scalar(value)),
            comment,
            "astMapPut0S",
        )
    }

    /// Stores the scalar `value` under `key`, with an optional comment, as
    /// [`put_0i`](Self::put_0i) does for an `i32`.
    ///
    /// # Errors
    ///
    /// As for [`put_0i`](Self::put_0i).
    #[doc(alias = "astMapPut0")]
    pub fn put_0k(&mut self, key: &str, value: i64, comment: Option<&str>) -> AstResult<()> {
        self.put_scalar(
            key,
            EntryValue::KInt(Scalar1D::Scalar(value)),
            comment,
            "astMapPut0K",
        )
    }

    /// Stores the scalar `value` under `key`, with an optional comment, as
    /// [`put_0i`](Self::put_0i) does for an `i32`.
    ///
    /// # Errors
    ///
    /// As for [`put_0i`](Self::put_0i).
    #[doc(alias = "astMapPut0")]
    pub fn put_0b(&mut self, key: &str, value: u8, comment: Option<&str>) -> AstResult<()> {
        self.put_scalar(
            key,
            EntryValue::Byte(Scalar1D::Scalar(value)),
            comment,
            "astMapPut0B",
        )
    }

    /// Stores the scalar `value` under `key`, with an optional comment, as
    /// [`put_0i`](Self::put_0i) does for an `i32`.
    ///
    /// # Errors
    ///
    /// As for [`put_0i`](Self::put_0i).
    #[doc(alias = "astMapPut0")]
    pub fn put_0f(&mut self, key: &str, value: f32, comment: Option<&str>) -> AstResult<()> {
        self.put_scalar(
            key,
            EntryValue::Float(Scalar1D::Scalar(value)),
            comment,
            "astMapPut0F",
        )
    }

    /// Stores the scalar `value` under `key`, with an optional comment, as
    /// [`put_0i`](Self::put_0i) does for an `i32`.
    ///
    /// # Errors
    ///
    /// As for [`put_0i`](Self::put_0i).
    #[doc(alias = "astMapPut0")]
    pub fn put_0d(&mut self, key: &str, value: f64, comment: Option<&str>) -> AstResult<()> {
        self.put_scalar(
            key,
            EntryValue::Double(Scalar1D::Scalar(value)),
            comment,
            "astMapPut0D",
        )
    }

    /// Stores the string `value` under `key`, with an optional comment, as
    /// [`put_0i`](Self::put_0i) does for an `i32`.
    ///
    /// # Errors
    ///
    /// As for [`put_0i`](Self::put_0i).
    #[doc(alias = "astMapPut0")]
    pub fn put_0c(&mut self, key: &str, value: &str, comment: Option<&str>) -> AstResult<()> {
        self.put_scalar(
            key,
            EntryValue::String(Scalar1D::Scalar(value.to_string())),
            comment,
            "astMapPut0C",
        )
    }

    /// Stores the Object `value` under `key`, with an optional comment, as
    /// [`put_0i`](Self::put_0i) does for an `i32`.
    ///
    /// The KeyMap takes ownership of `value`, and
    /// [`get_0a`](Self::get_0a) returns a copy of it.
    /// A KeyMap can hold another KeyMap, but never itself.
    ///
    /// # Errors
    ///
    /// As for [`put_0i`](Self::put_0i).
    // C's astMapPut0A runs a CheckCircle guard here that fails with
    // AST__KYCIR when the stored Object would make the KeyMap contain
    // itself. `put_0a` moves `value` in while `self` is already borrowed
    // mutably, so a caller cannot pass this same KeyMap to its own `put_0a`:
    // the guard's case is unconstructible in an owned-value API and has no
    // Rust counterpart. See docs/issues/c-library-quirks.md.
    #[doc(alias = "astMapPut0")]
    pub fn put_0a(&mut self, key: &str, value: AnyObject, comment: Option<&str>) -> AstResult<()> {
        self.put_scalar(
            key,
            EntryValue::Object(Scalar1D::Scalar(Some(Box::new(value)))),
            comment,
            "astMapPut0A",
        )
    }

    /// Stores `key` with no value, with an optional comment.
    ///
    /// Any value already stored under `key` is removed.
    /// The entry's type is [`AstType::Undef`]:
    /// [`map_has_key`](Self::map_has_key) reports it but
    /// [`map_defined`](Self::map_defined) does not, and the `get` methods
    /// give `Ok(None)` for it, whatever [KeyError](Self::key_error) is.
    /// Entries with no value can be added before setting
    /// [MapLocked](Self::map_locked), as places for values to be stored
    /// later.
    ///
    /// # Errors
    ///
    /// As for [`put_0i`](Self::put_0i).
    // keymap.c:5147. A replaced entry keeps its keymember, as for a put.
    #[doc(alias = "astMapPutU")]
    pub fn put_u(&mut self, key: &str, comment: Option<&str>) -> AstResult<()> {
        let key = table::convert_key(key, self.key_case(), "astMapPutU")?;
        self.insert_entry(key, EntryValue::Undef { nel: 0 }, comment, "astMapPutU")
    }

    /// Normalize `key` and hand the value to [`KeyMap::insert_entry`].
    fn put_scalar(
        &mut self,
        key: &str,
        value: EntryValue,
        comment: Option<&str>,
        method: &str,
    ) -> AstResult<()> {
        let key = table::convert_key(key, self.key_case(), method)?;
        self.insert_entry(key, value, comment, method)
    }

    /// Store an entry, applying C's key trimming, `MapLocked` check and age
    /// counter rules. The key must already be normalized (see
    /// [`table::convert_key`]).
    ///
    /// Mirrors the shared tail of every `MAKE_MAPPUT0` / `MAKE_MAPPUT1`
    /// expansion (`keymap.c:4690` onward) plus `AddTableEntry`
    /// (`keymap.c:622`): remove any existing entry at the key first,
    /// keeping its `keymember` so a re-put does not reset the key's age,
    /// then check `MapLocked`, then bump `member_count` — once for the
    /// entry's own `member`, and a second time for a fresh key's
    /// `keymember`.
    pub(crate) fn insert_entry(
        &mut self,
        key: String,
        value: EntryValue,
        comment: Option<&str>,
        method: &str,
    ) -> AstResult<()> {
        // C's AST__BADKEY message below uses the converted key exactly as
        // ConvertKey returned it — trailing spaces and all. Only the stored
        // key, and the table lookup, are trimmed: C terminates its copy of
        // the key string after storing it (keymap.c:4706), but the local
        // `key` variable used in the astError call above that point still
        // carries the untrimmed string.
        let untrimmed = key.clone();

        // The put macros strip trailing spaces from the stored key. Only
        // spaces: unlike astChrLen, other white space is significant.
        let key = key.trim_end_matches(' ').to_string();

        // C removes any existing entry first, retaining its keymember so
        // the key's age survives a re-put.
        let previous = self.entries.remove(&key);

        if previous.is_none() && self.map_locked() {
            return Err(AstError::new(
                ErrorCode::Badkey,
                format!(
                    "{method}(KeyMap): Failed to add item \"{untrimmed}\" to a KeyMap: \
                     \"{untrimmed}\" is not a known item."
                ),
            ));
        }

        let entry = MapEntry {
            comment: comment.map(str::to_string),
            member: 0,
            keymember: 0,
            value,
        };

        match previous {
            // A re-put of an existing key keeps its `keymember` and only
            // bumps `member_count` once, for the entry's own `member`.
            Some(old) => {
                self.member_count += 1;
                self.entries.insert(
                    key,
                    MapEntry {
                        member: self.member_count,
                        keymember: old.keymember,
                        ..entry
                    },
                );
            }
            None => self.insert_entry_raw(key, entry),
        }
        Ok(())
    }

    /// Store an already-built [`MapEntry`] under `key`, assigning it fresh
    /// `member` and `keymember` counters.
    ///
    /// Mirrors `AddTableEntry` (`keymap.c:622`) called with `keymember ==
    /// -1`: the case where the key is brand new to this KeyMap, so there
    /// is no previous `keymember` to inherit. `key` must already be
    /// normalized, trimmed, and absent from `self.entries`. Used by
    /// [`KeyMap::insert_entry`] for a fresh key, and by
    /// [`KeyMap::copy_one`] to install a cloned source entry without
    /// re-deriving its value.
    fn insert_entry_raw(&mut self, key: String, mut entry: MapEntry) {
        self.member_count += 1;
        entry.member = self.member_count;
        self.member_count += 1;
        entry.keymember = self.member_count;
        self.entries.insert(key, entry);
    }

    /// Copies every entry of `source` into this KeyMap.
    ///
    /// Each entry replaces any entry stored here under the same key, and the
    /// key keeps its place in the [`SortBy::KeyAgeUp`] and
    /// [`SortBy::KeyAgeDown`] orders.
    /// The exception is a key under which both KeyMaps hold a single
    /// KeyMap: the entries of the source's KeyMap are then copied into the
    /// one held here, by this method, rather than replacing it.
    /// Each key is stored as this KeyMap's [KeyCase](Self::key_case)
    /// converts it.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Badkey`] if [MapLocked](Self::map_locked) is true and
    ///   `source` holds a key this KeyMap does not; entries copied before
    ///   that key stay copied.
    /// - [`ErrorCode::Bigkey`] if [KeyCase](Self::key_case) is false and a
    ///   key of `source` has more than 200 characters.
    /// - As for this method, from a held KeyMap that is copied into.
    // keymap.c:4112.
    #[doc(alias = "astMapCopy")]
    pub fn map_copy(&mut self, source: &KeyMap) -> AstResult<()> {
        for key in source.sorted_keys() {
            // `sorted_keys` returns already-trimmed stored keys, so the
            // trimmed and message forms coincide here — unlike
            // `map_copy_entry`, which converts a caller-supplied key that
            // may still carry trailing spaces. The destination may fold
            // differently, so the key it is stored under is converted with
            // *this* KeyMap's KeyCase (C `ConvertKey( this, ... )`,
            // keymap.c:4200).
            let out_key = table::convert_key(&key, self.key_case(), "astMapCopy")?;
            let out_key = out_key.trim_end_matches(' ').to_string();
            self.copy_one(&key, &out_key, &out_key, source, true, "astMapCopy")?;
        }
        Ok(())
    }

    /// Copies the entry stored under `key` in `source` into this KeyMap.
    ///
    /// Nothing is done if `source` has no entry `key`.
    /// The entry replaces any entry stored here under the same key, as in
    /// [`map_copy`](Self::map_copy), except when both KeyMaps hold a single
    /// KeyMap under `key`.
    /// `merge` then decides: when true the entries of the source's KeyMap
    /// are copied into the one held here, by [`map_copy`](Self::map_copy),
    /// and when false the one held here is replaced.
    /// `key` is looked up in each KeyMap as that KeyMap's
    /// [KeyCase](Self::key_case) converts it, and stored as this KeyMap's
    /// converts it.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Badkey`] if `key` is not already in this KeyMap and
    ///   [MapLocked](Self::map_locked) is true.
    /// - [`ErrorCode::Bigkey`] if either KeyMap has
    ///   [KeyCase](Self::key_case) false and `key` has more than 200
    ///   characters.
    /// - As for [`map_copy`](Self::map_copy), when `merge` copies into a
    ///   held KeyMap.
    // keymap.c:4264. A key absent from `source` is ignored (keymap.c:4356,
    // `if( in_entry )`). Each KeyMap's own KeyCase normalizes the key it is
    // searched with, and the entry is stored under the destination's form:
    // KeyCase can only be changed while a KeyMap is empty, so every key a
    // KeyMap holds should be in that KeyMap's own casing (C
    // `ConvertKey( that, ... )` and `ConvertKey( this, ... )`,
    // keymap.c:4436-4441).
    #[doc(alias = "astMapCopyEntry")]
    pub fn map_copy_entry(&mut self, key: &str, source: &KeyMap, merge: bool) -> AstResult<()> {
        // C binds each converted key once — folded, but not trimmed — and
        // reuses it for both the table lookup and the AST__BADKEY message
        // (keymap.c:4436-4470). The lookups need the trimmed form to match
        // how keys are stored (see `table::convert_key`'s doc comment); the
        // message does not.
        let in_key = table::convert_key(key, source.key_case(), "astMapCopyEntry")?;
        let in_key = in_key.trim_end_matches(' ').to_string();
        let out_key = table::convert_key(key, self.key_case(), "astMapCopyEntry")?;
        let out_trimmed = out_key.trim_end_matches(' ').to_string();
        if !source.entries.contains_key(&in_key) {
            return Ok(());
        }
        self.copy_one(
            &in_key,
            &out_trimmed,
            &out_key,
            source,
            merge,
            "astMapCopyEntry",
        )
    }

    /// The body `map_copy` and `map_copy_entry` share.
    ///
    /// `in_key` searches the source and `out_key` the destination, each in its
    /// own KeyMap's `KeyCase`; the entry is stored under `out_key`.
    /// `message_key` is what an `AST__BADKEY` message quotes, which for
    /// `map_copy_entry` still carries the trailing spaces C's `ConvertKey`
    /// output has.
    fn copy_one(
        &mut self,
        in_key: &str,
        out_key: &str,
        message_key: &str,
        source: &KeyMap,
        merge: bool,
        method: &str,
    ) -> AstResult<()> {
        let in_entry = source.entries.get(in_key).expect("caller checked presence");

        let Some(out_entry) = self.entries.get_mut(out_key) else {
            if self.map_locked() {
                return Err(AstError::new(
                    ErrorCode::Badkey,
                    format!(
                        "{method}(KeyMap): Failed to copy item \"{message_key}\": \
                         \"{message_key}\" is not a known item."
                    ),
                ));
            }
            let copied = in_entry.clone();
            self.insert_entry_raw(out_key.to_string(), copied);
            return Ok(());
        };

        // Two nested scalar KeyMaps merge rather than replace. C accepts
        // nel 0 or 1 on both sides, so a length-1 vector merges through
        // element 0.
        if merge
            && let Some(inner_src) = single_key_map(&in_entry.value)
            && let Some(inner_dest) = single_key_map_mut(&mut out_entry.value)
        {
            return inner_dest.map_copy(inner_src);
        }

        // Otherwise replace the value while keeping the key's own age.
        let keymember = out_entry.keymember;
        let mut copied = in_entry.clone();
        self.member_count += 1;
        copied.member = self.member_count;
        copied.keymember = keymember;
        self.entries.insert(out_key.to_string(), copied);
        Ok(())
    }

    /// The value stored under `key`, as an `i32`.
    ///
    /// The value is converted to the type asked for as described under
    /// [Reading values](KeyMap#reading-values), and a vector gives its first
    /// element.
    /// The result is `Ok(None)` if `key` is not in the KeyMap and
    /// [KeyError](Self::key_error) is false, or if `key` has no value (see
    /// [`put_u`](Self::put_u)).
    ///
    /// Each type has its own method:
    ///
    /// | Method | Type returned |
    /// |---|---|
    /// | `get_0i` | `i32` |
    /// | [`get_0s`](Self::get_0s) | `i16` |
    /// | [`get_0k`](Self::get_0k) | `i64` |
    /// | [`get_0b`](Self::get_0b) | `u8` |
    /// | [`get_0f`](Self::get_0f) | `f32` |
    /// | [`get_0d`](Self::get_0d) | `f64` |
    /// | [`get_0c`](Self::get_0c) | `String` |
    /// | [`get_0a`](Self::get_0a) | `Option<AnyObject>` |
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Mpger`] if the value cannot be converted.
    /// - [`ErrorCode::Mpker`] if `key` is not in the KeyMap and
    ///   [KeyError](Self::key_error) is true.
    /// - [`ErrorCode::Bigkey`] if [KeyCase](Self::key_case) is false and
    ///   `key` has more than 200 characters.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::KeyMap;
    /// use ast_core::error::ErrorCode;
    ///
    /// let mut km = KeyMap::new();
    /// km.put_0d("X", 2.6, None)?;
    /// km.put_0c("NAME", "M31", None)?;
    /// assert_eq!(km.get_0i("X")?, Some(3));
    /// assert_eq!(km.get_0i("Y")?, None);
    /// assert_eq!(km.get_0i("NAME").unwrap_err().code(), ErrorCode::Mpger);
    ///
    /// km.set_key_error(true);
    /// assert_eq!(km.get_0i("Y").unwrap_err().code(), ErrorCode::Mpker);
    /// # Ok::<(), ast_core::error::AstError>(())
    /// ```
    ///
    /// # See also
    ///
    /// `astMapGet0<X>`, one function per data type `<X>`, where `<X>` is the last
    /// letter of the matching method in upper case (`I` for the `i` method).
    #[doc(alias = "astMapGet0")]
    pub fn get_0i(&self, key: &str) -> AstResult<Option<i32>> {
        match self.get_scalar(key, AstType::Int, "astMapGet0I")? {
            Some(RawValue::Int(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// The value stored under `key`, as an `i16`, read as
    /// [`get_0i`](Self::get_0i) reads it.
    ///
    /// # Errors
    ///
    /// As for [`get_0i`](Self::get_0i).
    #[doc(alias = "astMapGet0")]
    pub fn get_0s(&self, key: &str) -> AstResult<Option<i16>> {
        match self.get_scalar(key, AstType::SInt, "astMapGet0S")? {
            Some(RawValue::SInt(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// The value stored under `key`, as an `i64`, read as
    /// [`get_0i`](Self::get_0i) reads it.
    ///
    /// # Errors
    ///
    /// As for [`get_0i`](Self::get_0i).
    #[doc(alias = "astMapGet0")]
    pub fn get_0k(&self, key: &str) -> AstResult<Option<i64>> {
        match self.get_scalar(key, AstType::KInt, "astMapGet0K")? {
            Some(RawValue::KInt(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// The value stored under `key`, as a `u8`, read as
    /// [`get_0i`](Self::get_0i) reads it.
    ///
    /// # Errors
    ///
    /// As for [`get_0i`](Self::get_0i).
    #[doc(alias = "astMapGet0")]
    pub fn get_0b(&self, key: &str) -> AstResult<Option<u8>> {
        match self.get_scalar(key, AstType::Byte, "astMapGet0B")? {
            Some(RawValue::Byte(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// The value stored under `key`, as an `f32`, read as
    /// [`get_0i`](Self::get_0i) reads it.
    ///
    /// # Errors
    ///
    /// As for [`get_0i`](Self::get_0i).
    #[doc(alias = "astMapGet0")]
    pub fn get_0f(&self, key: &str) -> AstResult<Option<f32>> {
        match self.get_scalar(key, AstType::Float, "astMapGet0F")? {
            Some(RawValue::Float(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// The value stored under `key`, as an `f64`, read as
    /// [`get_0i`](Self::get_0i) reads it.
    ///
    /// # Errors
    ///
    /// As for [`get_0i`](Self::get_0i).
    #[doc(alias = "astMapGet0")]
    pub fn get_0d(&self, key: &str) -> AstResult<Option<f64>> {
        match self.get_scalar(key, AstType::Double, "astMapGet0D")? {
            Some(RawValue::Double(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// The value stored under `key`, as a string, read as
    /// [`get_0i`](Self::get_0i) reads it.
    ///
    /// # Errors
    ///
    /// As for [`get_0i`](Self::get_0i).
    #[doc(alias = "astMapGet0")]
    pub fn get_0c(&self, key: &str) -> AstResult<Option<String>> {
        match self.get_scalar(key, AstType::String, "astMapGet0C")? {
            Some(RawValue::String(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// A copy of the Object stored under `key`, read as
    /// [`get_0i`](Self::get_0i) reads it.
    ///
    /// The result is `Ok(Some(None))` when the first element of the value
    /// is an empty Object slot, given as `None` to
    /// [`put_1a`](Self::put_1a) or [`put_elem_a`](Self::put_elem_a); this
    /// differs from `Ok(None)`, which says there is no value.
    ///
    /// # Errors
    ///
    /// As for [`get_0i`](Self::get_0i).
    // The outer Option is C's returned flag and the inner one C's stored NULL
    // Object pointer: astMapGet0A returns 1 with `*value == NULL` for a
    // stored NULL, so the two are kept distinct.
    #[doc(alias = "astMapGet0")]
    pub fn get_0a(&self, key: &str) -> AstResult<Option<Option<AnyObject>>> {
        match self.get_scalar(key, AstType::Object, "astMapGet0A")? {
            Some(RawValue::Object(v)) => Ok(Some(v.map(|b| *b))),
            _ => Ok(None),
        }
    }

    /// Normalize `key` (trailing spaces only — the key has already been
    /// through [`table::convert_key`]'s case folding by the time it
    /// reaches here in every caller) and look it up.
    ///
    /// Every read shares this: it is the `SearchTableEntry` half of
    /// `HashFun` + `SearchTableEntry` that every `MAKE_MAPGET0`,
    /// `MAKE_MAPGET1`, `MAKE_MAPGETELEM`, `MapLength` and `MapLenC`
    /// expansion starts with, minus the hash-table indexing this crate's
    /// `HashMap` backing makes unnecessary.
    fn lookup(&self, key: &str) -> Option<&MapEntry> {
        self.entries.get(key.trim_end_matches(' '))
    }

    /// Read one value out of `key`, converting it to `want`.
    ///
    /// Mirrors the shared tail of every `MAKE_MAPGET0` expansion
    /// (`keymap.c:5415` onward): look the key up, take element 0 of
    /// whatever is stored there (a vector entry yields its first element,
    /// because C takes `raw` as the vector's base pointer), and run it
    /// through `ConvertValue`.
    fn get_scalar(&self, key: &str, want: AstType, method: &str) -> AstResult<Option<RawValue>> {
        let nkey = table::convert_key(key, self.key_case(), method)?;
        let Some(entry) = self.lookup(&nkey) else {
            if self.key_error() {
                return Err(AstError::new(
                    ErrorCode::Mpker,
                    format!(
                        "{method}(KeyMap): No value was found for {nkey} in the \
                         supplied KeyMap."
                    ),
                ));
            }
            return Ok(None);
        };

        // An undefined entry gives C a NULL raw pointer, which returns
        // failure without consulting KeyError: it is present, not missing.
        let Some(raw) = convert::element_at(&entry.value, 0) else {
            return Ok(None);
        };
        if matches!(raw, RawValue::Undef) && want != AstType::Undef {
            return Ok(None);
        }

        match convert::convert_value(&raw, want)? {
            Some(v) => Ok(Some(v)),
            None => Err(AstError::new(
                ErrorCode::Mpger,
                format!(
                    "{method}(KeyMap): The value of KeyMap key \"{nkey}\" cannot \
                     be read using the requested data type."
                ),
            )),
        }
    }

    /// Stores the vector `values` under `key`, with an optional comment.
    ///
    /// Any entry already stored under `key`, of whatever type, is replaced.
    /// The vector must have at least one element; a vector of one element
    /// is still a vector, of length 1.
    /// Each value type has its own method:
    ///
    /// | Method | Values | Stored as |
    /// |---|---|---|
    /// | `put_1i` | `&[i32]` | [`AstType::Int`] |
    /// | [`put_1s`](Self::put_1s) | `&[i16]` | [`AstType::SInt`] |
    /// | [`put_1k`](Self::put_1k) | `&[i64]` | [`AstType::KInt`] |
    /// | [`put_1b`](Self::put_1b) | `&[u8]` | [`AstType::Byte`] |
    /// | [`put_1f`](Self::put_1f) | `&[f32]` | [`AstType::Float`] |
    /// | [`put_1d`](Self::put_1d) | `&[f64]` | [`AstType::Double`] |
    /// | [`put_1c`](Self::put_1c) | `&[&str]` | [`AstType::String`] |
    /// | [`put_1a`](Self::put_1a) | `Vec<Option<AnyObject>>` | [`AstType::Object`] |
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Nelin`] if `values` is empty.
    /// - [`ErrorCode::Badkey`] if `key` is not already in the KeyMap and
    ///   [MapLocked](Self::map_locked) is true; the KeyMap is then unchanged.
    /// - [`ErrorCode::Bigkey`] if [KeyCase](Self::key_case) is false and
    ///   `key` has more than 200 characters.
    ///
    /// # See also
    ///
    /// `astMapPut1<X>`, one function per data type `<X>`, where `<X>` is the last
    /// letter of the matching method in upper case (`I` for the `i` method).
    /// Its generic pointer type `P` has no counterpart.
    #[doc(alias = "astMapPut1")]
    pub fn put_1i(&mut self, key: &str, values: &[i32], comment: Option<&str>) -> AstResult<()> {
        self.put_vector(
            key,
            EntryValue::Int(Scalar1D::Vector(values.to_vec())),
            comment,
            "astMapPut1I",
        )
    }

    /// Stores the vector `values` under `key`, with an optional comment, as
    /// [`put_1i`](Self::put_1i) does for `i32` values.
    ///
    /// # Errors
    ///
    /// As for [`put_1i`](Self::put_1i).
    #[doc(alias = "astMapPut1")]
    pub fn put_1s(&mut self, key: &str, values: &[i16], comment: Option<&str>) -> AstResult<()> {
        self.put_vector(
            key,
            EntryValue::SInt(Scalar1D::Vector(values.to_vec())),
            comment,
            "astMapPut1S",
        )
    }

    /// Stores the vector `values` under `key`, with an optional comment, as
    /// [`put_1i`](Self::put_1i) does for `i32` values.
    ///
    /// # Errors
    ///
    /// As for [`put_1i`](Self::put_1i).
    #[doc(alias = "astMapPut1")]
    pub fn put_1k(&mut self, key: &str, values: &[i64], comment: Option<&str>) -> AstResult<()> {
        self.put_vector(
            key,
            EntryValue::KInt(Scalar1D::Vector(values.to_vec())),
            comment,
            "astMapPut1K",
        )
    }

    /// Stores the vector `values` under `key`, with an optional comment, as
    /// [`put_1i`](Self::put_1i) does for `i32` values.
    ///
    /// # Errors
    ///
    /// As for [`put_1i`](Self::put_1i).
    #[doc(alias = "astMapPut1")]
    pub fn put_1b(&mut self, key: &str, values: &[u8], comment: Option<&str>) -> AstResult<()> {
        self.put_vector(
            key,
            EntryValue::Byte(Scalar1D::Vector(values.to_vec())),
            comment,
            "astMapPut1B",
        )
    }

    /// Stores the vector `values` under `key`, with an optional comment, as
    /// [`put_1i`](Self::put_1i) does for `i32` values.
    ///
    /// # Errors
    ///
    /// As for [`put_1i`](Self::put_1i).
    #[doc(alias = "astMapPut1")]
    pub fn put_1f(&mut self, key: &str, values: &[f32], comment: Option<&str>) -> AstResult<()> {
        self.put_vector(
            key,
            EntryValue::Float(Scalar1D::Vector(values.to_vec())),
            comment,
            "astMapPut1F",
        )
    }

    /// Stores the vector `values` under `key`, with an optional comment, as
    /// [`put_1i`](Self::put_1i) does for `i32` values.
    ///
    /// # Errors
    ///
    /// As for [`put_1i`](Self::put_1i).
    #[doc(alias = "astMapPut1")]
    pub fn put_1d(&mut self, key: &str, values: &[f64], comment: Option<&str>) -> AstResult<()> {
        self.put_vector(
            key,
            EntryValue::Double(Scalar1D::Vector(values.to_vec())),
            comment,
            "astMapPut1D",
        )
    }

    /// Stores the vector `values` under `key`, with an optional comment, as
    /// [`put_1i`](Self::put_1i) does for `i32` values.
    ///
    /// # Errors
    ///
    /// As for [`put_1i`](Self::put_1i).
    #[doc(alias = "astMapPut1")]
    pub fn put_1c(&mut self, key: &str, values: &[&str], comment: Option<&str>) -> AstResult<()> {
        self.put_vector(
            key,
            EntryValue::String(Scalar1D::Vector(
                values.iter().map(|s| s.to_string()).collect(),
            )),
            comment,
            "astMapPut1C",
        )
    }

    /// Stores the vector of Objects `values` under `key`, with an optional
    /// comment, as [`put_1i`](Self::put_1i) does for `i32` values.
    ///
    /// The KeyMap takes ownership of the Objects.
    /// A `None` element is an empty Object slot, which reads back as `None`.
    ///
    /// # Errors
    ///
    /// As for [`put_1i`](Self::put_1i).
    // See put_0a for why C's CHECK_A/CheckCircle guard (keymap.c:4989) has no
    // Rust counterpart here.
    #[doc(alias = "astMapPut1")]
    pub fn put_1a(
        &mut self,
        key: &str,
        values: Vec<Option<AnyObject>>,
        comment: Option<&str>,
    ) -> AstResult<()> {
        self.put_vector(
            key,
            EntryValue::Object(Scalar1D::Vector(
                values.into_iter().map(|v| v.map(Box::new)).collect(),
            )),
            comment,
            "astMapPut1A",
        )
    }

    /// Normalize `key` and hand the vector value to [`KeyMap::insert_entry`].
    ///
    /// A zero-length vector is refused — see [`entry::is_empty_vector`].
    fn put_vector(
        &mut self,
        key: &str,
        value: EntryValue,
        comment: Option<&str>,
        method: &str,
    ) -> AstResult<()> {
        if is_empty_vector(&value) {
            return Err(AstError::new(
                ErrorCode::Nelin,
                format!(
                    "{method}(KeyMap): Illegal vector length 0 supplied for \
                     KeyMap entry \"{key}\" - must be at least one."
                ),
            ));
        }
        let key = table::convert_key(key, self.key_case(), method)?;
        self.insert_entry(key, value, comment, method)
    }

    /// Every value stored under `key`, as `i32`s.
    ///
    /// Each element is converted to the type asked for as described under
    /// [Reading values](KeyMap#reading-values), and a scalar gives a vector
    /// of length 1.
    /// The result is `Ok(None)` if `key` is not in the KeyMap and
    /// [KeyError](Self::key_error) is false, or if `key` has no value (see
    /// [`put_u`](Self::put_u)).
    ///
    /// Each type has its own method:
    ///
    /// | Method | Type returned |
    /// |---|---|
    /// | `get_1i` | `Vec<i32>` |
    /// | [`get_1s`](Self::get_1s) | `Vec<i16>` |
    /// | [`get_1k`](Self::get_1k) | `Vec<i64>` |
    /// | [`get_1b`](Self::get_1b) | `Vec<u8>` |
    /// | [`get_1f`](Self::get_1f) | `Vec<f32>` |
    /// | [`get_1d`](Self::get_1d) | `Vec<f64>` |
    /// | [`get_1c`](Self::get_1c) | `Vec<String>` |
    /// | [`get_1a`](Self::get_1a) | `Vec<Option<AnyObject>>` |
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Mpger`] if an element cannot be converted.
    /// - [`ErrorCode::Mpker`] if `key` is not in the KeyMap and
    ///   [KeyError](Self::key_error) is true.
    /// - [`ErrorCode::Bigkey`] if [KeyCase](Self::key_case) is false and
    ///   `key` has more than 200 characters.
    ///
    /// # See also
    ///
    /// `astMapGet1<X>`, one function per data type `<X>`, where `<X>` is the last
    /// letter of the matching method in upper case (`I` for the `i` method).
    /// The vector returned has the length of the entry, so there is no
    /// buffer size to give.
    #[doc(alias = "astMapGet1")]
    pub fn get_1i(&self, key: &str) -> AstResult<Option<Vec<i32>>> {
        Ok(self
            .get_vector(key, AstType::Int, "astMapGet1I")?
            .map(|v| v.into_iter().map(unwrap_int).collect()))
    }

    /// Every value stored under `key`, as `i16`s, read as
    /// [`get_1i`](Self::get_1i) reads them.
    ///
    /// # Errors
    ///
    /// As for [`get_1i`](Self::get_1i).
    #[doc(alias = "astMapGet1")]
    pub fn get_1s(&self, key: &str) -> AstResult<Option<Vec<i16>>> {
        Ok(self
            .get_vector(key, AstType::SInt, "astMapGet1S")?
            .map(|v| v.into_iter().map(unwrap_sint).collect()))
    }

    /// Every value stored under `key`, as `i64`s, read as
    /// [`get_1i`](Self::get_1i) reads them.
    ///
    /// # Errors
    ///
    /// As for [`get_1i`](Self::get_1i).
    #[doc(alias = "astMapGet1")]
    pub fn get_1k(&self, key: &str) -> AstResult<Option<Vec<i64>>> {
        Ok(self
            .get_vector(key, AstType::KInt, "astMapGet1K")?
            .map(|v| v.into_iter().map(unwrap_kint).collect()))
    }

    /// Every value stored under `key`, as `u8`s, read as
    /// [`get_1i`](Self::get_1i) reads them.
    ///
    /// # Errors
    ///
    /// As for [`get_1i`](Self::get_1i).
    #[doc(alias = "astMapGet1")]
    pub fn get_1b(&self, key: &str) -> AstResult<Option<Vec<u8>>> {
        Ok(self
            .get_vector(key, AstType::Byte, "astMapGet1B")?
            .map(|v| v.into_iter().map(unwrap_byte).collect()))
    }

    /// Every value stored under `key`, as `f32`s, read as
    /// [`get_1i`](Self::get_1i) reads them.
    ///
    /// # Errors
    ///
    /// As for [`get_1i`](Self::get_1i).
    #[doc(alias = "astMapGet1")]
    pub fn get_1f(&self, key: &str) -> AstResult<Option<Vec<f32>>> {
        Ok(self
            .get_vector(key, AstType::Float, "astMapGet1F")?
            .map(|v| v.into_iter().map(unwrap_float).collect()))
    }

    /// Every value stored under `key`, as `f64`s, read as
    /// [`get_1i`](Self::get_1i) reads them.
    ///
    /// # Errors
    ///
    /// As for [`get_1i`](Self::get_1i).
    #[doc(alias = "astMapGet1")]
    pub fn get_1d(&self, key: &str) -> AstResult<Option<Vec<f64>>> {
        Ok(self
            .get_vector(key, AstType::Double, "astMapGet1D")?
            .map(|v| v.into_iter().map(unwrap_double).collect()))
    }

    /// Every value stored under `key`, as strings, read as
    /// [`get_1i`](Self::get_1i) reads them.
    ///
    /// Each string is returned whole.
    ///
    /// # Errors
    ///
    /// As for [`get_1i`](Self::get_1i).
    // C's astMapGet1C truncates each string to a caller-supplied buffer
    // length; the Rust vector is sized to the entry instead.
    #[doc(alias = "astMapGet1")]
    pub fn get_1c(&self, key: &str) -> AstResult<Option<Vec<String>>> {
        Ok(self
            .get_vector(key, AstType::String, "astMapGet1C")?
            .map(|v| v.into_iter().map(unwrap_string).collect()))
    }

    /// Copies of every Object stored under `key`, read as
    /// [`get_1i`](Self::get_1i) reads them.
    ///
    /// A `None` element is an empty Object slot (see
    /// [`put_1a`](Self::put_1a)).
    ///
    /// # Errors
    ///
    /// As for [`get_1i`](Self::get_1i).
    // A None element is C's stored NULL Object pointer (astMapPut1A's
    // `value[i] ? astClone(value[i]) : NULL`, keymap.c:5012).
    #[doc(alias = "astMapGet1")]
    pub fn get_1a(&self, key: &str) -> AstResult<Option<Vec<Option<AnyObject>>>> {
        Ok(self
            .get_vector(key, AstType::Object, "astMapGet1A")?
            .map(|v| v.into_iter().map(unwrap_object).collect()))
    }

    /// Read every element of `key`, converting each to `want`.
    ///
    /// Mirrors the shared tail of every `MAKE_MAPGET1` expansion
    /// (`keymap.c:5874` onward): look the key up, treat a scalar as a
    /// length-1 vector, and run every element through `ConvertValue`,
    /// naming the (1-based) failing element if any conversion fails.
    /// Unlike C's `mxval`-bounded buffer, the returned `Vec` is always
    /// sized to the entry's own length.
    fn get_vector(
        &self,
        key: &str,
        want: AstType,
        method: &str,
    ) -> AstResult<Option<Vec<RawValue>>> {
        let nkey = table::convert_key(key, self.key_case(), method)?;
        let Some(entry) = self.lookup(&nkey) else {
            if self.key_error() {
                return Err(AstError::new(
                    ErrorCode::Mpker,
                    format!(
                        "{method}(KeyMap): No value was found for {nkey} in the \
                         supplied KeyMap."
                    ),
                ));
            }
            return Ok(None);
        };
        if !entry.value.defined() {
            return Ok(None);
        }

        // C reads `nel` values, treating a scalar as one.
        let n = entry.value.len().max(1);
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let Some(raw) = convert::element_at(&entry.value, i) else {
                break;
            };
            match convert::convert_value(&raw, want)? {
                Some(v) => out.push(v),
                None => {
                    return Err(AstError::new(
                        ErrorCode::Mpger,
                        format!(
                            "{method}(KeyMap): The value of element {} of KeyMap key \
                             \"{nkey}\" cannot be read using the requested data type.",
                            i + 1
                        ),
                    ));
                }
            }
        }
        Ok(Some(out))
    }

    /// Stores `value` in element `elem` (zero-based) of the vector stored
    /// under `key`.
    ///
    /// - `value` is converted to the type of the entry, as described under
    ///   [Reading values](KeyMap#reading-values).
    /// - If `elem` is at or beyond the end of the vector, `value` is
    ///   appended to it, making it one element longer.
    /// - A scalar entry first becomes a vector of length 1, keeping its
    ///   comment.
    /// - If `key` is not in the KeyMap, or has no value (see
    ///   [`put_u`](Self::put_u)), a vector of length 1 holding `value` is
    ///   stored under it, with no comment and the type of `value`, whatever
    ///   `elem` is.
    ///
    /// Each value type has its own method:
    ///
    /// | Method | Value |
    /// |---|---|
    /// | `put_elem_i` | `i32` |
    /// | [`put_elem_s`](Self::put_elem_s) | `i16` |
    /// | [`put_elem_k`](Self::put_elem_k) | `i64` |
    /// | [`put_elem_b`](Self::put_elem_b) | `u8` |
    /// | [`put_elem_f`](Self::put_elem_f) | `f32` |
    /// | [`put_elem_d`](Self::put_elem_d) | `f64` |
    /// | [`put_elem_c`](Self::put_elem_c) | `&str` |
    /// | [`put_elem_a`](Self::put_elem_a) | `Option<AnyObject>` |
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Mpper`] if `value` cannot be converted to the type of
    ///   the entry.
    /// - [`ErrorCode::Badkey`] if `key` is not in the KeyMap and
    ///   [MapLocked](Self::map_locked) is true.
    /// - [`ErrorCode::Bigkey`] if [KeyCase](Self::key_case) is false and
    ///   `key` has more than 200 characters.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::KeyMap;
    ///
    /// let mut km = KeyMap::new();
    /// km.put_1d("V", &[1.0, 2.0], None)?;
    /// km.put_elem_i("V", 0, 5)?;
    /// km.put_elem_i("V", 10, 7)?;
    /// assert_eq!(km.get_1d("V")?, Some(vec![5.0, 2.0, 7.0]));
    /// # Ok::<(), ast_core::error::AstError>(())
    /// ```
    ///
    /// # See also
    ///
    /// `astMapPutElem<X>`, one function per data type `<X>`, where `<X>` is the last
    /// letter of the matching method in upper case (`I` for the `i` method).
    /// Its generic pointer type `P` has no counterpart.
    #[doc(alias = "astMapPutElem")]
    pub fn put_elem_i(&mut self, key: &str, elem: usize, value: i32) -> AstResult<()> {
        self.put_elem(
            key,
            elem,
            RawValue::Int(value),
            "astMapPutElemI",
            "astMapPut1I",
        )
    }

    /// Stores `value` in element `elem` (zero-based) of the vector stored
    /// under `key`, as [`put_elem_i`](Self::put_elem_i) does for an `i32`.
    ///
    /// # Errors
    ///
    /// As for [`put_elem_i`](Self::put_elem_i).
    #[doc(alias = "astMapPutElem")]
    pub fn put_elem_s(&mut self, key: &str, elem: usize, value: i16) -> AstResult<()> {
        self.put_elem(
            key,
            elem,
            RawValue::SInt(value),
            "astMapPutElemS",
            "astMapPut1S",
        )
    }

    /// Stores `value` in element `elem` (zero-based) of the vector stored
    /// under `key`, as [`put_elem_i`](Self::put_elem_i) does for an `i32`.
    ///
    /// # Errors
    ///
    /// As for [`put_elem_i`](Self::put_elem_i).
    #[doc(alias = "astMapPutElem")]
    pub fn put_elem_k(&mut self, key: &str, elem: usize, value: i64) -> AstResult<()> {
        self.put_elem(
            key,
            elem,
            RawValue::KInt(value),
            "astMapPutElemK",
            "astMapPut1K",
        )
    }

    /// Stores `value` in element `elem` (zero-based) of the vector stored
    /// under `key`, as [`put_elem_i`](Self::put_elem_i) does for an `i32`.
    ///
    /// # Errors
    ///
    /// As for [`put_elem_i`](Self::put_elem_i).
    #[doc(alias = "astMapPutElem")]
    pub fn put_elem_b(&mut self, key: &str, elem: usize, value: u8) -> AstResult<()> {
        self.put_elem(
            key,
            elem,
            RawValue::Byte(value),
            "astMapPutElemB",
            "astMapPut1B",
        )
    }

    /// Stores `value` in element `elem` (zero-based) of the vector stored
    /// under `key`, as [`put_elem_i`](Self::put_elem_i) does for an `i32`.
    ///
    /// # Errors
    ///
    /// As for [`put_elem_i`](Self::put_elem_i).
    #[doc(alias = "astMapPutElem")]
    pub fn put_elem_f(&mut self, key: &str, elem: usize, value: f32) -> AstResult<()> {
        self.put_elem(
            key,
            elem,
            RawValue::Float(value),
            "astMapPutElemF",
            "astMapPut1F",
        )
    }

    /// Stores `value` in element `elem` (zero-based) of the vector stored
    /// under `key`, as [`put_elem_i`](Self::put_elem_i) does for an `i32`.
    ///
    /// # Errors
    ///
    /// As for [`put_elem_i`](Self::put_elem_i).
    #[doc(alias = "astMapPutElem")]
    pub fn put_elem_d(&mut self, key: &str, elem: usize, value: f64) -> AstResult<()> {
        self.put_elem(
            key,
            elem,
            RawValue::Double(value),
            "astMapPutElemD",
            "astMapPut1D",
        )
    }

    /// Stores `value` in element `elem` (zero-based) of the vector stored
    /// under `key`, as [`put_elem_i`](Self::put_elem_i) does for an `i32`.
    ///
    /// # Errors
    ///
    /// As for [`put_elem_i`](Self::put_elem_i).
    #[doc(alias = "astMapPutElem")]
    pub fn put_elem_c(&mut self, key: &str, elem: usize, value: &str) -> AstResult<()> {
        self.put_elem(
            key,
            elem,
            RawValue::String(value.to_string()),
            "astMapPutElemC",
            "astMapPut1C",
        )
    }

    /// Stores the Object `value` in element `elem` (zero-based) of the
    /// vector stored under `key`, as [`put_elem_i`](Self::put_elem_i) does
    /// for an `i32`.
    ///
    /// The KeyMap takes ownership of `value`, and `None` stores an empty
    /// Object slot.
    ///
    /// # Errors
    ///
    /// As for [`put_elem_i`](Self::put_elem_i).
    // See put_0a for why C's CHECK_A/CheckCircle guard (keymap.c:8461) has no
    // Rust counterpart here.
    #[doc(alias = "astMapPutElem")]
    pub fn put_elem_a(
        &mut self,
        key: &str,
        elem: usize,
        value: Option<AnyObject>,
    ) -> AstResult<()> {
        self.put_elem(
            key,
            elem,
            RawValue::Object(value.map(Box::new)),
            "astMapPutElemA",
            "astMapPut1A",
        )
    }

    /// Store `value` at zero-based element `elem` of `key`, creating an
    /// absent or undefined key as a length-1 vector, promoting a scalar
    /// key to a length-1 vector, and appending when `elem` is out of
    /// range. Mirrors `MAKE_MAPPUTELEM` (`keymap.c:8237`).
    ///
    /// `method` names this call for `ConvertKey` and for every diagnostic
    /// raised once the key is known to exist (`AST__MPPER`); `create_method`
    /// names the `astMapPut1<X>` call C literally makes on the create path
    /// (`keymap.c:8274`), whose own `MapLocked` check
    /// (`keymap.c:4972`) hardcodes that name — not `astMapPutElem<X>` —
    /// into the `AST__BADKEY` message.
    fn put_elem(
        &mut self,
        key: &str,
        elem: usize,
        value: RawValue,
        method: &str,
        create_method: &str,
    ) -> AstResult<()> {
        // `converted` keeps ConvertKey's output untrimmed, which is what
        // every diagnostic below quotes (C's own `key` variable never gets
        // trimmed — only the stored copy does, keymap.c:4706). `nkey` is
        // the trimmed form used for the table itself.
        let converted = table::convert_key(key, self.key_case(), method)?;
        let nkey = converted.trim_end_matches(' ').to_string();

        // An absent or undefined entry is created as a length-1 vector,
        // with no comment: C calls astMapPut1<X> with a NULL comment
        // (keymap.c:8271).
        let needs_create = match self.entries.get(&nkey) {
            None => true,
            Some(e) => !e.value.defined(),
        };
        if needs_create {
            let vector = single_element_vector(&value);
            return self.insert_entry(converted, vector, None, create_method);
        }

        // A scalar is promoted to a length-1 vector, keeping its comment:
        // C re-calls astMapPut1<X> with `mapentry->comment`
        // (keymap.c:8286 onward). Promoting in place and leaving the
        // comment untouched has the same effect without re-running
        // insert_entry's key-trim/MapLocked machinery on an already-present
        // key. That re-put counts as a value put (AddTableEntry's `member`
        // bump), but the key is not new, so `keymember` is untouched. An
        // element of an existing vector is changed in place, which counts
        // as nothing.
        let entry = self.entries.get_mut(&nkey).expect("checked above");
        if entry.value.nel() == 0 {
            promote_scalar_to_vector(&mut entry.value);
            self.member_count += 1;
            entry.member = self.member_count;
        }

        let want = entry.value.ast_type();
        let Some(value_converted) = convert::convert_value(&value, want)? else {
            return Err(AstError::new(
                ErrorCode::Mpper,
                format!(
                    "{method}(KeyMap): The supplied value cannot be converted to \
                     the data type of KeyMap key \"{converted}\"."
                ),
            ));
        };

        // An index at or beyond the end appends, rather than failing
        // (keymap.c:8397: `new = ( elem >= nel || elem < 0 )`). Rust's
        // `usize` cannot go negative, so only the high side is reachable.
        let n = entry.value.len();
        let target = if elem >= n { n } else { elem };
        set_element(&mut entry.value, target, value_converted);
        Ok(())
    }

    /// Element `elem` (zero-based) of the value stored under `key`, as an
    /// `i32`.
    ///
    /// The element is converted to the type asked for as described under
    /// [Reading values](KeyMap#reading-values).
    /// A scalar counts as a vector of length 1, and so does a key with no
    /// value.
    /// The result is `Ok(None)` if `key` is not in the KeyMap and
    /// [KeyError](Self::key_error) is false, or if `key` has no value (see
    /// [`put_u`](Self::put_u)) and `elem` is 0.
    ///
    /// Each type has its own method:
    ///
    /// | Method | Type returned |
    /// |---|---|
    /// | `get_elem_i` | `i32` |
    /// | [`get_elem_s`](Self::get_elem_s) | `i16` |
    /// | [`get_elem_k`](Self::get_elem_k) | `i64` |
    /// | [`get_elem_b`](Self::get_elem_b) | `u8` |
    /// | [`get_elem_f`](Self::get_elem_f) | `f32` |
    /// | [`get_elem_d`](Self::get_elem_d) | `f64` |
    /// | [`get_elem_c`](Self::get_elem_c) | `String` |
    /// | [`get_elem_a`](Self::get_elem_a) | `Option<AnyObject>` |
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Mpvin`] if `key` is in the KeyMap and `elem` is not
    ///   less than its length.
    /// - [`ErrorCode::Mpger`] if the element cannot be converted.
    /// - [`ErrorCode::Mpker`] if `key` is not in the KeyMap and
    ///   [KeyError](Self::key_error) is true.
    /// - [`ErrorCode::Bigkey`] if [KeyCase](Self::key_case) is false and
    ///   `key` has more than 200 characters.
    ///
    /// # See also
    ///
    /// `astMapGetElem<X>`, one function per data type `<X>`, where `<X>` is the last
    /// letter of the matching method in upper case (`I` for the `i` method).
    #[doc(alias = "astMapGetElem")]
    pub fn get_elem_i(&self, key: &str, elem: usize) -> AstResult<Option<i32>> {
        Ok(self
            .get_elem(key, elem, AstType::Int, "astMapGetElemI")?
            .map(unwrap_int))
    }

    /// Element `elem` (zero-based) of the value stored under `key`, as an
    /// `i16`, read as [`get_elem_i`](Self::get_elem_i) reads it.
    ///
    /// # Errors
    ///
    /// As for [`get_elem_i`](Self::get_elem_i).
    #[doc(alias = "astMapGetElem")]
    pub fn get_elem_s(&self, key: &str, elem: usize) -> AstResult<Option<i16>> {
        Ok(self
            .get_elem(key, elem, AstType::SInt, "astMapGetElemS")?
            .map(unwrap_sint))
    }

    /// Element `elem` (zero-based) of the value stored under `key`, as an
    /// `i64`, read as [`get_elem_i`](Self::get_elem_i) reads it.
    ///
    /// # Errors
    ///
    /// As for [`get_elem_i`](Self::get_elem_i).
    #[doc(alias = "astMapGetElem")]
    pub fn get_elem_k(&self, key: &str, elem: usize) -> AstResult<Option<i64>> {
        Ok(self
            .get_elem(key, elem, AstType::KInt, "astMapGetElemK")?
            .map(unwrap_kint))
    }

    /// Element `elem` (zero-based) of the value stored under `key`, as a
    /// `u8`, read as [`get_elem_i`](Self::get_elem_i) reads it.
    ///
    /// # Errors
    ///
    /// As for [`get_elem_i`](Self::get_elem_i).
    #[doc(alias = "astMapGetElem")]
    pub fn get_elem_b(&self, key: &str, elem: usize) -> AstResult<Option<u8>> {
        Ok(self
            .get_elem(key, elem, AstType::Byte, "astMapGetElemB")?
            .map(unwrap_byte))
    }

    /// Element `elem` (zero-based) of the value stored under `key`, as an
    /// `f32`, read as [`get_elem_i`](Self::get_elem_i) reads it.
    ///
    /// # Errors
    ///
    /// As for [`get_elem_i`](Self::get_elem_i).
    #[doc(alias = "astMapGetElem")]
    pub fn get_elem_f(&self, key: &str, elem: usize) -> AstResult<Option<f32>> {
        Ok(self
            .get_elem(key, elem, AstType::Float, "astMapGetElemF")?
            .map(unwrap_float))
    }

    /// Element `elem` (zero-based) of the value stored under `key`, as an
    /// `f64`, read as [`get_elem_i`](Self::get_elem_i) reads it.
    ///
    /// # Errors
    ///
    /// As for [`get_elem_i`](Self::get_elem_i).
    #[doc(alias = "astMapGetElem")]
    pub fn get_elem_d(&self, key: &str, elem: usize) -> AstResult<Option<f64>> {
        Ok(self
            .get_elem(key, elem, AstType::Double, "astMapGetElemD")?
            .map(unwrap_double))
    }

    /// Element `elem` (zero-based) of the value stored under `key`, as a
    /// string, read as [`get_elem_i`](Self::get_elem_i) reads it.
    ///
    /// # Errors
    ///
    /// As for [`get_elem_i`](Self::get_elem_i).
    #[doc(alias = "astMapGetElem")]
    pub fn get_elem_c(&self, key: &str, elem: usize) -> AstResult<Option<String>> {
        Ok(self
            .get_elem(key, elem, AstType::String, "astMapGetElemC")?
            .map(unwrap_string))
    }

    /// A copy of the Object in element `elem` (zero-based) of the value
    /// stored under `key`, read as [`get_elem_i`](Self::get_elem_i) reads
    /// it.
    ///
    /// `Ok(Some(None))` is an empty Object slot (see
    /// [`put_1a`](Self::put_1a)).
    ///
    /// # Errors
    ///
    /// As for [`get_elem_i`](Self::get_elem_i).
    #[doc(alias = "astMapGetElem")]
    pub fn get_elem_a(&self, key: &str, elem: usize) -> AstResult<Option<Option<AnyObject>>> {
        Ok(self
            .get_elem(key, elem, AstType::Object, "astMapGetElemA")?
            .map(unwrap_object))
    }

    /// Read zero-based element `elem` of `key`, converting it to `want`.
    ///
    /// Mirrors the shared tail of every `MAKE_MAPGETELEM` expansion
    /// (`keymap.c:6773` onward): look the key up, treat a scalar as a
    /// length-1 vector, reject an out-of-range or (on the C side)
    /// negative `elem` with `AST__MPVIN`, then run the element through
    /// `ConvertValue`. Unlike put, an out-of-range index here is a user
    /// error, not something to append past.
    ///
    /// An in-range index of an undefined entry is `Ok(None)` rather than
    /// an error, for every index: C's range check runs first and still
    /// reports `AST__MPVIN` when it fails, but once `elem` is in range,
    /// the raw pointer for an `AST__UNDEFTYPE` entry is `NULL`, and the
    /// `} else if( raw )` guard (`keymap.c:6910`) skips `ConvertValue`
    /// entirely, leaving the macro's `result` at the `1` it was set to as
    /// soon as the key was found — success, with nothing written to the
    /// caller's output buffer. `Ok(None)` is the value-carrying equivalent
    /// of that: no conversion ran, so there is nothing to return.
    fn get_elem(
        &self,
        key: &str,
        elem: usize,
        want: AstType,
        method: &str,
    ) -> AstResult<Option<RawValue>> {
        let nkey = table::convert_key(key, self.key_case(), method)?;
        let Some(entry) = self.lookup(&nkey) else {
            if self.key_error() {
                return Err(AstError::new(
                    ErrorCode::Mpker,
                    format!(
                        "{method}(KeyMap): No value was found for {nkey} in the \
                         supplied KeyMap."
                    ),
                ));
            }
            return Ok(None);
        };

        let n = entry.value.len().max(1);
        if elem >= n {
            // The numeric methods share MAKE_MAPGETELEM's message, which
            // names the method with a literal `<X>`. MapGetElemC and the
            // public astMapGetElemAId_ have their own, giving the index
            // one-based and an upper limit of `nel + 1`, one more than the
            // vector length (c-library-quirks.md).
            let message = match want {
                AstType::String | AstType::Object => format!(
                    "{method}(KeyMap): Illegal vector index {} supplied for KeyMap \
                     entry '{nkey}' - should be in the range 1 to {}.",
                    elem + 1,
                    n + 1
                ),
                _ => format!(
                    "astMapGetElem<X>(KeyMap): Illegal zero-based vector index {elem} \
                     supplied for KeyMap entry '{nkey}' - the vector has {n} elements."
                ),
            };
            return Err(AstError::new(ErrorCode::Mpvin, message));
        }

        if !entry.value.defined() {
            return Ok(None);
        }

        let Some(raw) = convert::element_at(&entry.value, elem) else {
            return Ok(None);
        };
        match convert::convert_value(&raw, want)? {
            Some(v) => Ok(Some(v)),
            None => Err(AstError::new(
                ErrorCode::Mpger,
                format!(
                    "{method}(KeyMap): The value of element {} of KeyMap key \
                     \"{nkey}\" cannot be read using the requested data type.",
                    elem + 1
                ),
            )),
        }
    }

    /// The number of values stored under `key`.
    ///
    /// This is 1 for a scalar and for a key with no value, the length of a
    /// vector, and 0 for a key that is not in the KeyMap.
    /// [KeyError](Self::key_error) is not consulted.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Bigkey`] if [KeyCase](Self::key_case) is false and
    ///   `key` has more than 200 characters.
    #[doc(alias = "astMapLength")]
    pub fn map_length(&self, key: &str) -> AstResult<usize> {
        let nkey = table::convert_key(key, self.key_case(), "astMapLength")?;
        Ok(match self.lookup(&nkey) {
            Some(entry) => entry.value.len().max(1),
            None => 0,
        })
    }

    /// The length in bytes of the longest value stored under `key`, when
    /// formatted as a string as [`get_1c`](Self::get_1c) formats it.
    ///
    /// The result is 0, not an error, for a key that is not in the KeyMap,
    /// a key with no value, and a value that cannot be formatted as a
    /// string (an Object).
    /// [KeyError](Self::key_error) is not consulted.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Bigkey`] if [KeyCase](Self::key_case) is false and
    ///   `key` has more than 200 characters.
    // keymap.c:7808. C's ConvertValue counts the terminating null, which
    // astMapLenC then subtracts.
    #[doc(alias = "astMapLenC")]
    pub fn map_len_c(&self, key: &str) -> AstResult<usize> {
        let nkey = table::convert_key(key, self.key_case(), "astMapLenC")?;
        let Some(entry) = self.lookup(&nkey) else {
            return Ok(0);
        };
        if !entry.value.defined() {
            return Ok(0);
        }
        let mut result = 0;
        for i in 0..entry.value.len().max(1) {
            let Some(raw) = convert::element_at(&entry.value, i) else {
                break;
            };
            if let Some(RawValue::String(s)) = convert::convert_value(&raw, AstType::String)? {
                // ConvertValue's returned length includes the terminating
                // null; astMapLenC decrements before comparing.
                result = result.max(s.len());
            }
        }
        Ok(result)
    }

    /// The value stored under `key` as a single string.
    ///
    /// A scalar, or a vector of length 1, gives the string
    /// [`get_0c`](Self::get_0c) gives.
    /// A longer vector gives its elements as [`get_1c`](Self::get_1c)
    /// formats them, separated by commas and enclosed in parentheses, as in
    /// `(1,2,3)`.
    /// The result is `Ok(None)` for a key that is not in the KeyMap, whatever
    /// [KeyError](Self::key_error) is, and for a key with no value.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Mpger`] if the value cannot be formatted as a string
    ///   (an Object).
    /// - [`ErrorCode::Bigkey`] if [KeyCase](Self::key_case) is false and
    ///   `key` has more than 200 characters.
    // keymap.c:6466. A vector is joined with a bare comma
    // (`astAppendString( ..., "," )`, keymap.c:6614).
    //
    // C fills a buffer of stride `mxlen + 1` per element, `mxlen` being
    // map_len_c's longest formatted length, via `strncpy` truncated to
    // `mxlen` bytes plus an explicit null, then walks that buffer reading
    // each slot as a null-terminated string. Because `mxlen` is by
    // construction at least as long as every element's own formatted
    // length, the `strncpy` never truncates: each slot holds exactly the
    // element's own string followed by null padding. So joining get_1c's
    // strings directly, with no space-padding of short elements, reproduces
    // C's buffer walk exactly: an empty element contributes nothing between
    // its commas, not `mxlen` spaces.
    #[doc(alias = "astMapGetC")]
    pub fn map_get_c(&self, key: &str) -> AstResult<Option<String>> {
        // C's astMapGetC asks astMapLength for the entry's length first, so a
        // key too long for a case-insensitive KeyMap is reported under that
        // name.
        let nval = self.map_length(key)?;
        if nval == 0 {
            return Ok(None);
        }
        if nval == 1 {
            return self.get_0c(key);
        }
        let Some(parts) = self.get_1c(key)? else {
            return Ok(None);
        };
        let mut out = String::from("(");
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(part);
        }
        out.push(')');
        Ok(Some(out))
    }

    /// The type the value stored under `key` was stored as.
    ///
    /// This is [`AstType::Undef`] for a key with no value, and
    /// [`AstType::BadType`] for a key that is not in the KeyMap, whatever
    /// [KeyError](Self::key_error) is.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Bigkey`] if [KeyCase](Self::key_case) is false and
    ///   `key` has more than 200 characters.
    // keymap.c:8496.
    #[doc(alias = "astMapType")]
    pub fn map_type(&self, key: &str) -> AstResult<AstType> {
        let nkey = table::convert_key(key, self.key_case(), "astMapType")?;
        Ok(match self.lookup(&nkey) {
            Some(entry) => entry.value.ast_type(),
            None => AstType::BadType,
        })
    }

    /// Whether `key` is in the KeyMap, with or without a value.
    ///
    /// Unlike [`map_defined`](Self::map_defined), this is true for a key
    /// with no value (see [`put_u`](Self::put_u)).
    /// [KeyError](Self::key_error) is not consulted.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Bigkey`] if [KeyCase](Self::key_case) is false and
    ///   `key` has more than 200 characters.
    // keymap.c:7448.
    #[doc(alias = "astMapHasKey")]
    pub fn map_has_key(&self, key: &str) -> AstResult<bool> {
        let nkey = table::convert_key(key, self.key_case(), "astMapHasKey")?;
        Ok(self.lookup(&nkey).is_some())
    }

    /// Whether `key` is in the KeyMap with a value.
    ///
    /// This is false for a key with no value (see [`put_u`](Self::put_u)),
    /// for which [`map_has_key`](Self::map_has_key) is true.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Mpker`] if `key` is not in the KeyMap and
    ///   [KeyError](Self::key_error) is true.
    /// - [`ErrorCode::Bigkey`] if [KeyCase](Self::key_case) is false and
    ///   `key` has more than 200 characters.
    // keymap.c:7347. C's MapDefined skips the KeyError branch only when
    // SearchTableEntry finds the entry, which it does for a key with no
    // value too (`mapentry->type == AST__UNDEFTYPE` gives `result = 0`
    // without reaching the `else if` that reports AST__MPKER). A key that is
    // not in the table at all reaches that `else if`, as in get_scalar.
    #[doc(alias = "astMapDefined")]
    pub fn map_defined(&self, key: &str) -> AstResult<bool> {
        let nkey = table::convert_key(key, self.key_case(), "astMapDefined")?;
        match self.lookup(&nkey) {
            Some(entry) => Ok(entry.value.defined()),
            None => {
                if self.key_error() {
                    return Err(AstError::new(
                        ErrorCode::Mpker,
                        format!(
                            "astMapDefined(KeyMap): No value was found for {nkey} in \
                             the supplied KeyMap."
                        ),
                    ));
                }
                Ok(false)
            }
        }
    }

    /// Removes the entry stored under `key`.
    ///
    /// Nothing is done if `key` is not in the KeyMap.
    /// An entry can be removed even when [MapLocked](Self::map_locked) is
    /// true.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Bigkey`] if [KeyCase](Self::key_case) is false and
    ///   `key` has more than 200 characters.
    // keymap.c:7545: RemoveTableEntry returns NULL for a missing key and
    // FreeMapEntry(NULL) is a no-op.
    #[doc(alias = "astMapRemove")]
    pub fn map_remove(&mut self, key: &str) -> AstResult<()> {
        let nkey = table::convert_key(key, self.key_case(), "astMapRemove")?;
        self.entries.remove(nkey.trim_end_matches(' '));
        Ok(())
    }

    /// The comment stored with `key`, or `None` if `key` is not in the
    /// KeyMap, has no comment, or has more than 200 characters when
    /// [KeyCase](Self::key_case) is false.
    // No C counterpart: C exposes an entry's comment only through the
    // serialized form. Rust adds it because `to_node` needs it and because a
    // caller who can set a comment should be able to read it back.
    pub fn map_comment(&self, key: &str) -> Option<String> {
        let nkey = table::convert_key(key, self.key_case(), "map_comment").ok()?;
        self.lookup(&nkey).and_then(|e| e.comment.clone())
    }

    /// Moves the entry stored under `old_key` to `new_key`.
    ///
    /// Any entry already stored under `new_key` is replaced.
    /// Nothing is done if `old_key` is not in the KeyMap, or if the two keys
    /// name the same entry.
    /// For the [`SortBy`] orders, the moved value counts as newly stored,
    /// and `new_key` keeps the age it had, or is the newest key if it was
    /// not in the KeyMap.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Badkey`] if `old_key` is in the KeyMap, `new_key` is
    ///   not, and [MapLocked](Self::map_locked) is true; the KeyMap is then
    ///   unchanged.
    /// - [`ErrorCode::Bigkey`] if [KeyCase](Self::key_case) is false and
    ///   either key has more than 200 characters.
    // keymap.c:7608. A no-op if the two normalized keys are equal (C's whole
    // body is guarded on `strcmp(oldkey, newkey)`), and silent if `old_key`
    // is absent (C's RemoveTableEntry returns NULL and the rest of the body
    // is skipped). Otherwise the destination's `keymember` is inherited if it
    // was already occupied, and a fresh one is allocated if it was not, as
    // AddTableEntry does.
    #[doc(alias = "astMapRename")]
    pub fn map_rename(&mut self, old_key: &str, new_key: &str) -> AstResult<()> {
        // `convert_key` case-folds per KeyCase but does not trim trailing
        // spaces. C's error message below uses these converted-but-untrimmed
        // strings; the table lookups use the trimmed versions, matching
        // key_cmp's trailing-space-insensitive comparison.
        let old_converted = table::convert_key(old_key, self.key_case(), "astMapRename")?;
        let new_converted = table::convert_key(new_key, self.key_case(), "astMapRename")?;
        let old = old_converted.trim_end_matches(' ').to_string();
        let new = new_converted.trim_end_matches(' ').to_string();

        if old == new {
            return Ok(());
        }

        let Some(mut entry) = self.entries.remove(&old) else {
            return Ok(());
        };

        let previous = self.entries.remove(&new);
        let there = previous.is_some();

        // C reports AST__BADKEY here before removing anything (fixed
        // upstream in 89d0edfa; see docs/issues/c-library-quirks.md), so a
        // refused rename leaves the KeyMap unchanged. Rust puts the entry
        // back to the same effect.
        if !there && self.map_locked() {
            self.entries.insert(old, entry);
            return Err(AstError::new(
                ErrorCode::Badkey,
                format!(
                    "astMapRename(KeyMap): Failed to rename item \"{old_converted}\" in \
                     a KeyMap to \"{new_converted}\": \"{new_converted}\" is not a known \
                     item."
                ),
            ));
        }

        self.member_count += 1;
        entry.member = self.member_count;
        entry.keymember = match previous {
            Some(old_dest) => old_dest.keymember,
            None => {
                self.member_count += 1;
                self.member_count
            }
        };
        self.entries.insert(new, entry);
        Ok(())
    }

    /// The keys in read-out order, as set by `SortBy`.
    ///
    /// `SortBy::None` orders alphabetically. C leaves it unordered, reading
    /// out in hash-bucket order; see `docs/issues/c-library-quirks.md`.
    pub(crate) fn sorted_keys(&self) -> Vec<String> {
        let sort_by = self.sort_by();
        let mut keys: Vec<&String> = self.entries.keys().collect();
        keys.sort_by(|a, b| {
            sort::compare_entries(
                sort_by,
                (a.as_str(), &self.entries[*a]),
                (b.as_str(), &self.entries[*b]),
            )
        });
        keys.into_iter().cloned().collect()
    }

    /// The key at zero-based `index`, in the order set by
    /// [SortBy](Self::sort_by).
    ///
    /// With `index` running from 0 to one less than
    /// [`map_size`](Self::map_size), this gives every key once;
    /// [`map_iterate`](Self::map_iterate) does the same without an index.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Mpind`] if `index` is not less than
    ///   [`map_size`](Self::map_size).
    // keymap.c:4432, via GetKey at keymap.c:3430.
    #[doc(alias = "astMapKey")]
    pub fn map_key(&self, index: usize) -> AstResult<String> {
        let keys = self.sorted_keys();
        keys.get(index).cloned().ok_or_else(|| {
            AstError::new(
                ErrorCode::Mpind,
                format!(
                    "astMapKey(KeyMap): Cannot find element {index} (zero-based) \
                     of the KeyMap."
                ),
            )
        })
    }

    /// The next key, in the order set by [SortBy](Self::sort_by), or `None`
    /// once every key has been returned.
    ///
    /// When `reset` is true the walk starts again from the first key.
    /// After `None`, the next call starts again from the first key too.
    // astMapIterate (keymap.c:8599), a protected C function. The cursor is a
    // plain index into `sorted_keys` rather than C's saved hash-table or
    // sorted-list position, so the walk always terminates (C's did not with
    // SortBy set until upstream 83a772cc; see
    // docs/issues/c-library-quirks.md).
    pub fn map_iterate(&mut self, reset: bool) -> Option<String> {
        if reset {
            self.iter_index = 0;
        }
        let keys = self.sorted_keys();
        match keys.get(self.iter_index) {
            Some(key) => {
                self.iter_index += 1;
                Some(key.clone())
            }
            None => {
                self.iter_index = 0;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// keymap.c `MapPutElem<X>` changes an element of an existing vector in
    /// place, which does not count as a put; a scalar is first re-put as a
    /// vector through `astMapPut1<X>`, which counts once.
    #[test]
    fn put_elem_counts_only_a_promotion() {
        let mut km = KeyMap::new();
        km.put_1i("V", &[1, 2], None).unwrap();
        let before = km.member_count;
        km.put_elem_i("V", 0, 5).unwrap();
        km.put_elem_i("V", 2, 6).unwrap();
        assert_eq!(km.member_count, before);

        km.put_0i("S", 1, None).unwrap();
        let before = km.member_count;
        km.put_elem_i("S", 0, 5).unwrap();
        assert_eq!(km.member_count, before + 1);
        km.put_elem_i("S", 1, 6).unwrap();
        assert_eq!(km.member_count, before + 1);
    }

    #[test]
    fn attribute_defaults_match_the_c_get_macros() {
        let km = KeyMap::new();
        // keymap.c:10155, the astMAKE_GET block.
        assert!(km.key_case());
        assert!(!km.key_error());
        assert!(!km.map_locked());
        assert_eq!(km.sort_by(), SortBy::None);
        // MIN_TABLE_SIZE * MAX_ENTRIES_PER_TABLE_ENTRY. The C prose claims
        // 300; the code computes 160.
        assert_eq!(km.size_guess(), 160);

        assert!(!km.test_key_case());
        assert!(!km.test_key_error());
        assert!(!km.test_map_locked());
        assert!(!km.test_sort_by());
        assert!(!km.test_size_guess());
    }

    #[test]
    fn setting_and_clearing_tracks_the_test_flag() {
        let mut km = KeyMap::new();
        km.set_key_error(true);
        assert!(km.key_error());
        assert!(km.test_key_error());
        km.clear_key_error();
        assert!(!km.key_error());
        assert!(!km.test_key_error());

        km.set_size_guess(1000).unwrap();
        assert_eq!(km.size_guess(), 1000);
        assert!(km.test_size_guess());
        km.clear_size_guess().unwrap();
        assert_eq!(km.size_guess(), 160);
    }

    #[test]
    fn size_guess_is_frozen_once_the_map_is_not_empty() {
        let mut km = KeyMap::new();
        km.put_0i("A", 1, None).unwrap();

        let err = km.set_size_guess(1000).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Nowrt);
        assert!(
            err.to_string().contains(
                "Illegal attempt to change the SizeGuess attribute of a non-empty KeyMap."
            ),
            "{err}"
        );
        // The failed set left the attribute alone.
        assert!(!km.test_size_guess());

        let err = km.clear_size_guess().unwrap_err();
        assert_eq!(err.code(), ErrorCode::Nowrt);
        assert!(
            err.to_string().contains(
                "Illegal attempt to clear the SizeGuess attribute of a non-empty KeyMap."
            ),
            "{err}"
        );
    }

    #[test]
    fn key_case_is_frozen_only_when_the_value_would_change() {
        let mut km = KeyMap::new();
        km.put_0i("A", 1, None).unwrap();

        // Setting it to the value it already reports is a no-op, because
        // C's SetKeyCase guards the emptiness check on `newval != oldval`.
        assert!(km.key_case());
        km.set_key_case(true).unwrap();
        assert!(km.test_key_case());

        let err = km.set_key_case(false).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Nowrt);
        assert!(
            err.to_string()
                .contains("Illegal attempt to change the KeyCase attribute of a non-empty KeyMap."),
            "{err}"
        );
        // Still true: the failed set restored the old value.
        assert!(km.key_case());

        // Clearing is allowed here even though the map is non-empty: the
        // stored value (true) already matches the default, so ClearKeyCase's
        // own guard (`defval != oldval`) never triggers the emptiness check.
        km.clear_key_case().unwrap();
        assert!(!km.test_key_case());
        assert!(km.key_case());
    }

    #[test]
    fn key_error_and_map_locked_recurse_into_nested_key_maps() {
        let mut inner = KeyMap::new();
        inner.put_0i("X", 1, None).unwrap();
        let mut middle = KeyMap::new();
        middle
            .put_0a("INNER", AnyObject::KeyMap(Box::new(inner)), None)
            .unwrap();
        let mut outer = KeyMap::new();
        outer
            .put_0a("MIDDLE", AnyObject::KeyMap(Box::new(middle)), None)
            .unwrap();

        outer.set_key_error(true);
        outer.set_map_locked(true);

        fn check(km: &KeyMap, depth: usize) {
            assert!(km.key_error(), "depth {depth}");
            assert!(km.map_locked(), "depth {depth}");
            for key in km.sorted_keys() {
                if let Ok(Some(Some(AnyObject::KeyMap(nested)))) = km.get_0a(&key) {
                    check(&nested, depth + 1);
                }
            }
        }
        check(&outer, 0);
    }

    #[test]
    fn sort_by_and_size_guess_do_not_recurse() {
        let mut inner = KeyMap::new();
        inner.put_0i("X", 1, None).unwrap();
        let mut outer = KeyMap::new();
        outer
            .put_0a("INNER", AnyObject::KeyMap(Box::new(inner)), None)
            .unwrap();

        outer.set_sort_by(SortBy::KeyUp);
        let Some(Some(AnyObject::KeyMap(nested))) = outer.get_0a("INNER").unwrap() else {
            panic!("nested KeyMap missing");
        };
        assert_eq!(nested.sort_by(), SortBy::None);
        assert!(!nested.test_sort_by());
    }
}
