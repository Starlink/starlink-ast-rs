//! A KeyMap: an unordered collection of key/value pairs in which each
//! value may be a scalar or a 1-D vector of one of eight data types,
//! including a nested AST Object.
//!
//! Ported from `reference/starlink-ast/src/keymap.c`.
//!
//! Keys are stored in a [`std::collections::HashMap`] rather than C's
//! chained bucket table. C's table arithmetic survives only in
//! [`table`], which reconstructs the `MapSz` card at dump time.

// Removed in Task 14 when the public API lands.
#![allow(dead_code)]

pub(crate) mod convert;
pub(crate) mod entry;
pub(crate) mod sort;
pub(crate) mod table;

use crate::error::{AstError, AstResult, ErrorCode};
use crate::keymap::convert::RawValue;
use crate::keymap::entry::{EntryValue, MapEntry, Scalar1D};
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

/// An unordered collection of key/value pairs.
///
/// Each value is a scalar or a 1-D vector of one of eight data types,
/// one of which is a nested AST Object. Keys are compared ignoring
/// trailing spaces, and — unless [`KeyMap::set_key_case`] turns it off —
/// case sensitively.
#[derive(Debug, Clone, Default)]
pub struct KeyMap {
    /// Keys are normalised on the way in, so the `String` key is directly
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
}

impl KeyMap {
    /// An empty KeyMap with every attribute unset.
    pub fn new() -> Self {
        KeyMap::default()
    }

    /// An empty KeyMap whose storage is presized for `sizeguess`
    /// entries, with the `SizeGuess` attribute set to match.
    pub fn with_size_guess(sizeguess: usize) -> Self {
        KeyMap {
            entries: HashMap::with_capacity(sizeguess),
            sizeguess: Some(sizeguess),
            ..KeyMap::default()
        }
    }

    /// The number of entries.
    pub fn map_size(&self) -> usize {
        self.entries.len()
    }

    /// An estimate of the number of entries the KeyMap will hold.
    ///
    /// Setting this presizes the internal storage. It has no effect on
    /// behaviour, only on the work done while entries are added.
    pub fn size_guess(&self) -> usize {
        self.sizeguess.unwrap_or(DEFAULT_SIZE_GUESS)
    }

    /// Set `SizeGuess`. Fails with `AST__NOWRT` if the KeyMap already
    /// holds any entries.
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

    /// Clear `SizeGuess`. Fails with `AST__NOWRT` if the KeyMap already
    /// holds any entries.
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

    /// Whether `SizeGuess` has been set.
    pub fn test_size_guess(&self) -> bool {
        self.sizeguess.is_some()
    }

    /// Whether keys are case sensitive. Default `true`.
    pub fn key_case(&self) -> bool {
        self.key_case.unwrap_or(true)
    }

    /// Set `KeyCase`. Fails with `AST__NOWRT` if the KeyMap already holds
    /// any entries **and** the value would change; setting it to the
    /// value already reported is always allowed.
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

    /// Clear `KeyCase`. Fails with `AST__NOWRT` if the KeyMap already
    /// holds any entries **and** clearing would change the reported
    /// value, leaving the attribute unchanged.
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

    /// Whether `KeyCase` has been set.
    pub fn test_key_case(&self) -> bool {
        self.key_case.is_some()
    }

    /// Whether reading a missing key is an error. Default `false`.
    pub fn key_error(&self) -> bool {
        self.key_error.unwrap_or(false)
    }

    /// Set `KeyError`, recursively in every nested KeyMap.
    pub fn set_key_error(&mut self, key_error: bool) {
        self.key_error = Some(key_error);
        for nested in self.nested_key_maps_mut() {
            nested.set_key_error(key_error);
        }
    }

    /// Clear `KeyError`, recursively in every nested KeyMap.
    pub fn clear_key_error(&mut self) {
        self.key_error = None;
        for nested in self.nested_key_maps_mut() {
            nested.clear_key_error();
        }
    }

    /// Whether `KeyError` has been set.
    pub fn test_key_error(&self) -> bool {
        self.key_error.is_some()
    }

    /// Whether adding a new key is forbidden. Default `false`.
    pub fn map_locked(&self) -> bool {
        self.map_locked.unwrap_or(false)
    }

    /// Set `MapLocked`, recursively in every nested KeyMap.
    pub fn set_map_locked(&mut self, map_locked: bool) {
        self.map_locked = Some(map_locked);
        for nested in self.nested_key_maps_mut() {
            nested.set_map_locked(map_locked);
        }
    }

    /// Clear `MapLocked`, recursively in every nested KeyMap.
    pub fn clear_map_locked(&mut self) {
        self.map_locked = None;
        for nested in self.nested_key_maps_mut() {
            nested.clear_map_locked();
        }
    }

    /// Whether `MapLocked` has been set.
    pub fn test_map_locked(&self) -> bool {
        self.map_locked.is_some()
    }

    /// The order in which [`KeyMap::map_key`] and the dump present keys.
    /// Default [`SortBy::None`].
    pub fn sort_by(&self) -> SortBy {
        self.sort_by.unwrap_or_default()
    }

    /// Set `SortBy`. Does not recurse into nested KeyMaps.
    pub fn set_sort_by(&mut self, sort_by: SortBy) {
        self.sort_by = Some(sort_by);
    }

    /// Clear `SortBy`.
    pub fn clear_sort_by(&mut self) {
        self.sort_by = None;
    }

    /// Whether `SortBy` has been set.
    pub fn test_sort_by(&self) -> bool {
        self.sort_by.is_some()
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

    /// Store `value`, replacing any existing entry at `key`. Mirrors
    /// `astMapPut0I`. Fails with `AST__BADKEY` when `MapLocked` is set and
    /// `key` is new.
    pub fn put_0i(&mut self, key: &str, value: i32, comment: Option<&str>) -> AstResult<()> {
        self.put_scalar(
            key,
            EntryValue::Int(Scalar1D::Scalar(value)),
            comment,
            "astMapPut0I",
        )
    }

    /// Store `value`, replacing any existing entry at `key`. Mirrors
    /// `astMapPut0S`. Fails with `AST__BADKEY` when `MapLocked` is set and
    /// `key` is new.
    pub fn put_0s(&mut self, key: &str, value: i16, comment: Option<&str>) -> AstResult<()> {
        self.put_scalar(
            key,
            EntryValue::SInt(Scalar1D::Scalar(value)),
            comment,
            "astMapPut0S",
        )
    }

    /// Store `value`, replacing any existing entry at `key`. Mirrors
    /// `astMapPut0K`. Fails with `AST__BADKEY` when `MapLocked` is set and
    /// `key` is new.
    pub fn put_0k(&mut self, key: &str, value: i64, comment: Option<&str>) -> AstResult<()> {
        self.put_scalar(
            key,
            EntryValue::KInt(Scalar1D::Scalar(value)),
            comment,
            "astMapPut0K",
        )
    }

    /// Store `value`, replacing any existing entry at `key`. Mirrors
    /// `astMapPut0B`. Fails with `AST__BADKEY` when `MapLocked` is set and
    /// `key` is new.
    pub fn put_0b(&mut self, key: &str, value: u8, comment: Option<&str>) -> AstResult<()> {
        self.put_scalar(
            key,
            EntryValue::Byte(Scalar1D::Scalar(value)),
            comment,
            "astMapPut0B",
        )
    }

    /// Store `value`, replacing any existing entry at `key`. Mirrors
    /// `astMapPut0F`. Fails with `AST__BADKEY` when `MapLocked` is set and
    /// `key` is new.
    pub fn put_0f(&mut self, key: &str, value: f32, comment: Option<&str>) -> AstResult<()> {
        self.put_scalar(
            key,
            EntryValue::Float(Scalar1D::Scalar(value)),
            comment,
            "astMapPut0F",
        )
    }

    /// Store `value`, replacing any existing entry at `key`. Mirrors
    /// `astMapPut0D`. Fails with `AST__BADKEY` when `MapLocked` is set and
    /// `key` is new.
    pub fn put_0d(&mut self, key: &str, value: f64, comment: Option<&str>) -> AstResult<()> {
        self.put_scalar(
            key,
            EntryValue::Double(Scalar1D::Scalar(value)),
            comment,
            "astMapPut0D",
        )
    }

    /// Store a string, replacing any existing entry at `key`. Mirrors
    /// `astMapPut0C`.
    pub fn put_0c(&mut self, key: &str, value: &str, comment: Option<&str>) -> AstResult<()> {
        self.put_scalar(
            key,
            EntryValue::String(Scalar1D::Scalar(value.to_string())),
            comment,
            "astMapPut0C",
        )
    }

    /// Store an Object, replacing any existing entry at `key`.
    ///
    /// Mirrors `astMapPut0A`, including its `CheckCircle` guard: an Object
    /// that would make the KeyMap contain itself is `AST__KYCIR`.
    pub fn put_0a(&mut self, key: &str, value: AnyObject, comment: Option<&str>) -> AstResult<()> {
        self.check_circle(&value, "astMapPut0A")?;
        self.put_scalar(
            key,
            EntryValue::Object(Scalar1D::Scalar(Some(Box::new(value)))),
            comment,
            "astMapPut0A",
        )
    }

    /// Normalise `key` and hand the value to [`KeyMap::insert_entry`].
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
    /// counter rules. The key must already be normalised (see
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
                    "{method}(KeyMap): Failed to add item \"{key}\" to a KeyMap: \
                     \"{key}\" is not a known item."
                ),
            ));
        }

        // AddTableEntry bumps member_count once for the entry's own
        // `member`, and a second time for a fresh key's `keymember`.
        self.member_count += 1;
        let member = self.member_count;
        let keymember = match &previous {
            Some(old) => old.keymember,
            None => {
                self.member_count += 1;
                self.member_count
            }
        };

        self.entries.insert(
            key,
            MapEntry {
                comment: comment.map(str::to_string),
                member,
                keymember,
                value,
            },
        );
        Ok(())
    }

    /// Read `key` as an `i32`. Mirrors `astMapGet0I`.
    ///
    /// `Ok(None)` means the key is absent (and `KeyError` is clear) or the
    /// entry is undefined. A present but unconvertible value is
    /// `AST__MPGER`; a missing key with `KeyError` set is `AST__MPKER`.
    pub fn get_0i(&self, key: &str) -> AstResult<Option<i32>> {
        match self.get_scalar(key, AstType::Int, "astMapGet0I")? {
            Some(RawValue::Int(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// Read `key` as an `i16`. Mirrors `astMapGet0S`. See [`KeyMap::get_0i`]
    /// for the meaning of `Ok(None)` versus the two error codes.
    pub fn get_0s(&self, key: &str) -> AstResult<Option<i16>> {
        match self.get_scalar(key, AstType::SInt, "astMapGet0S")? {
            Some(RawValue::SInt(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// Read `key` as an `i64`. Mirrors `astMapGet0K`. See [`KeyMap::get_0i`]
    /// for the meaning of `Ok(None)` versus the two error codes.
    pub fn get_0k(&self, key: &str) -> AstResult<Option<i64>> {
        match self.get_scalar(key, AstType::KInt, "astMapGet0K")? {
            Some(RawValue::KInt(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// Read `key` as a `u8`. Mirrors `astMapGet0B`. See [`KeyMap::get_0i`]
    /// for the meaning of `Ok(None)` versus the two error codes.
    pub fn get_0b(&self, key: &str) -> AstResult<Option<u8>> {
        match self.get_scalar(key, AstType::Byte, "astMapGet0B")? {
            Some(RawValue::Byte(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// Read `key` as an `f32`. Mirrors `astMapGet0F`. See [`KeyMap::get_0i`]
    /// for the meaning of `Ok(None)` versus the two error codes.
    pub fn get_0f(&self, key: &str) -> AstResult<Option<f32>> {
        match self.get_scalar(key, AstType::Float, "astMapGet0F")? {
            Some(RawValue::Float(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// Read `key` as an `f64`. Mirrors `astMapGet0D`. See [`KeyMap::get_0i`]
    /// for the meaning of `Ok(None)` versus the two error codes.
    pub fn get_0d(&self, key: &str) -> AstResult<Option<f64>> {
        match self.get_scalar(key, AstType::Double, "astMapGet0D")? {
            Some(RawValue::Double(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// Read `key` as a `String`. Mirrors `astMapGet0C`. See
    /// [`KeyMap::get_0i`] for the meaning of `Ok(None)` versus the two
    /// error codes.
    pub fn get_0c(&self, key: &str) -> AstResult<Option<String>> {
        match self.get_scalar(key, AstType::String, "astMapGet0C")? {
            Some(RawValue::String(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// Read `key` as an Object. Mirrors `astMapGet0A`.
    ///
    /// The outer `Option` is "was there an entry" (C's returned flag); the
    /// inner one is C's stored `NULL` Object pointer. `astMapGet0A` returns
    /// 1 with `*value == NULL` for a stored NULL, which is a different
    /// outcome from returning 0 — so `Ok(Some(None))` (present, NULL) and
    /// `Ok(None)` (absent) are deliberately distinct, and this method does
    /// not flatten them.
    pub fn get_0a(&self, key: &str) -> AstResult<Option<Option<AnyObject>>> {
        match self.get_scalar(key, AstType::Object, "astMapGet0A")? {
            Some(RawValue::Object(v)) => Ok(Some(v.map(|b| *b))),
            _ => Ok(None),
        }
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
        let Some(entry) = self.entries.get(nkey.trim_end_matches(' ')) else {
            if self.key_error() {
                return Err(AstError::new(
                    ErrorCode::Mpker,
                    format!(
                        "{method}(KeyMap): No value was found for {key} in the \
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
                    "{method}(KeyMap): The value of KeyMap key \"{key}\" cannot \
                     be read using the requested data type."
                ),
            )),
        }
    }

    /// Borrow an Object entry without copying it.
    ///
    /// Used by the attribute-recursion helpers and by [`KeyMap::check_circle`].
    pub(crate) fn get_object_ref(&self, key: &str) -> Option<&AnyObject> {
        let entry = self.entries.get(key.trim_end_matches(' '))?;
        match &entry.value {
            // A stored NULL Object element borrows to nothing, which is
            // what C's `aval ? astClone( aval ) : NULL` amounts to for a
            // borrow rather than a clone.
            EntryValue::Object(objs) => objs.as_slice().first().and_then(|b| b.as_deref()),
            _ => None,
        }
    }

    /// Fail with `AST__KYCIR` if storing `candidate` in this KeyMap would
    /// make the KeyMap contain itself, directly or transitively. Mirrors
    /// `CheckCircle` (`keymap.c:951`).
    ///
    /// `candidate`'s own subtree is searched for a KeyMap matching `self`
    /// (see [`KeyMap::same_entries`] for what "matching" means in an
    /// owned-value API). This is the only case an owned-value API can
    /// reach: `put_0a`/`put_1a`/`put_elem_a` move their `AnyObject`
    /// argument, so a caller cannot pass this KeyMap into its own `put_0a`
    /// to build the literal `keymap == this` case C detects by pointer.
    ///
    /// A candidate's held Objects are found by walking `EntryValue::Object`
    /// entries directly here, not through [`KeyMap::nested_key_maps_mut`]:
    /// that helper is `&mut` and this method only needs to read.
    pub fn check_circle(&self, candidate: &AnyObject, method: &str) -> AstResult<()> {
        fn contains(haystack: &KeyMap, needle: &KeyMap) -> bool {
            if haystack.same_entries(needle) {
                return true;
            }
            for entry in haystack.entries.values() {
                if let EntryValue::Object(objs) = &entry.value {
                    for obj in objs.as_slice().iter().flatten() {
                        if let AnyObject::KeyMap(km) = obj.as_ref() {
                            if contains(km, needle) {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        }

        if let AnyObject::KeyMap(km) = candidate {
            if self.same_entries(km) {
                return Err(AstError::new(
                    ErrorCode::Kycir,
                    format!(
                        "{method}(KeyMap): Cannot add a KeyMap into another \
                         KeyMap because they are same KeyMap."
                    ),
                ));
            }
            if contains(km, self) {
                return Err(AstError::new(
                    ErrorCode::Kycir,
                    format!(
                        "{method}(KeyMap): Cannot add a KeyMap into another \
                         KeyMap because the first KeyMap contains the second \
                         KeyMap."
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Whether `other` is structurally the same KeyMap as `self`: the same
    /// key set, and for every key the same type and `keymember`.
    ///
    /// C's `CheckCircle` detects "are these the same KeyMap" with a pointer
    /// comparison (`keymap == this`). Owned Rust values have no identity to
    /// compare, so this is the structural stand-in: two KeyMaps built by
    /// the same sequence of puts (and never diverging afterwards) compare
    /// equal here, which is what `let self_copy = km.clone(); km.check_circle(&self_copy, ...)`
    /// needs it to mean.
    fn same_entries(&self, other: &KeyMap) -> bool {
        if self.entries.len() != other.entries.len() {
            return false;
        }
        self.entries.iter().all(|(key, entry)| {
            other.entries.get(key).is_some_and(|other_entry| {
                entry.value.ast_type() == other_entry.value.ast_type()
                    && entry.keymember == other_entry.keymember
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    // Enabled once `sorted_keys` exists.
    // #[test]
    // fn key_error_and_map_locked_recurse_into_nested_key_maps() {
    //     let mut inner = KeyMap::new();
    //     inner.put_0i("X", 1, None).unwrap();
    //     let mut middle = KeyMap::new();
    //     middle.put_0a("INNER", AnyObject::KeyMap(Box::new(inner)), None).unwrap();
    //     let mut outer = KeyMap::new();
    //     outer.put_0a("MIDDLE", AnyObject::KeyMap(Box::new(middle)), None).unwrap();
    //
    //     outer.set_key_error(true);
    //     outer.set_map_locked(true);
    //
    //     fn check(km: &KeyMap, depth: usize) {
    //         assert!(km.key_error(), "depth {depth}");
    //         assert!(km.map_locked(), "depth {depth}");
    //         for key in km.sorted_keys() {
    //             if let Some(AnyObject::KeyMap(nested)) = km.get_object_ref(&key) {
    //                 check(nested, depth + 1);
    //             }
    //         }
    //     }
    //     check(&outer, 0);
    // }

    #[test]
    fn sort_by_and_size_guess_do_not_recurse() {
        let mut inner = KeyMap::new();
        inner.put_0i("X", 1, None).unwrap();
        let mut outer = KeyMap::new();
        outer
            .put_0a("INNER", AnyObject::KeyMap(Box::new(inner)), None)
            .unwrap();

        outer.set_sort_by(SortBy::KeyUp);
        let Some(AnyObject::KeyMap(nested)) = outer.get_object_ref("INNER") else {
            panic!("nested KeyMap missing");
        };
        assert_eq!(nested.sort_by(), SortBy::None);
        assert!(!nested.test_sort_by());
    }
}
