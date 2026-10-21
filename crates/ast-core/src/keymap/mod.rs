//! A KeyMap: an unordered collection of key/value pairs in which each
//! value may be a scalar or a 1-D vector of one of eight data types,
//! including a nested AST Object.
//!
//! Ported from `reference/starlink-ast/src/keymap.c`.
//!
//! Keys are stored in a [`std::collections::HashMap`] rather than C's
//! chained bucket table. C's table arithmetic survives only in
//! `table`, which reconstructs the `MapSz` card at dump time.

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

    /// The order in which [`KeyMap::map_key`] and `map_iterate` present
    /// keys. Default [`SortBy::None`].
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

    /// The Ident attribute, a string identifying the KeyMap, or `""` when
    /// unset. Mirrors `astGetIdent` (object.c).
    pub fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    /// Set Ident. A blank Ident is set, as in C.
    pub fn set_ident(&mut self, ident: &str) {
        self.ident = Some(ident.to_string());
    }

    /// Clear Ident.
    pub fn clear_ident(&mut self) {
        self.ident = None;
    }

    /// The Object-class `ID` attribute, blank unless set. Unlike `Ident`, a
    /// clone does not inherit it, as C's `astCopy` does not copy it.
    pub fn id(&self) -> &str {
        self.id.get()
    }

    /// Set `ID`.
    pub fn set_id(&mut self, id: &str) {
        self.id.set(id);
    }

    /// Whether `ID` is set.
    pub fn test_id(&self) -> bool {
        self.id.test()
    }

    /// Clear `ID`.
    pub fn clear_id(&mut self) {
        self.id.clear();
    }

    /// Whether Ident has been set.
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

    /// Store an Object, replacing any existing entry at `key`. Mirrors
    /// `astMapPut0A`.
    ///
    /// C's `astMapPut0A` runs a `CheckCircle` guard here that fails with
    /// `AST__KYCIR` when the stored Object would make the KeyMap contain
    /// itself. `put_0a` moves `value` in while `self` is already borrowed
    /// mutably, so a caller cannot pass this same KeyMap to its own
    /// `put_0a` — the guard's case is unconstructible in an owned-value
    /// API and has no Rust counterpart. See `docs/issues/c-library-quirks.md`.
    pub fn put_0a(&mut self, key: &str, value: AnyObject, comment: Option<&str>) -> AstResult<()> {
        self.put_scalar(
            key,
            EntryValue::Object(Scalar1D::Scalar(Some(Box::new(value)))),
            comment,
            "astMapPut0A",
        )
    }

    /// Add `key` with no associated value. Mirrors `astMapPutU`
    /// (`keymap.c:5147`).
    ///
    /// The entry is present for [`KeyMap::map_has_key`] but not for
    /// [`KeyMap::map_defined`], and every getter reports failure for it
    /// without consulting `KeyError` — the entry was found, it simply has
    /// no value, which is a different outcome from the key being absent.
    /// Subject to `MapLocked` exactly as a put is, and reuses the
    /// previous `keymember` when it replaces an existing entry.
    pub fn put_u(&mut self, key: &str, comment: Option<&str>) -> AstResult<()> {
        let key = table::convert_key(key, self.key_case(), "astMapPutU")?;
        self.insert_entry(key, EntryValue::Undef { nel: 0 }, comment, "astMapPutU")
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
    /// normalised, trimmed, and absent from `self.entries`. Used by
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

    /// Copy every entry from `source` into this KeyMap. Mirrors
    /// `astMapCopy` (`keymap.c:4112`).
    ///
    /// An entry whose key already exists here is replaced, keeping this
    /// KeyMap's record of when the key was first added. The exception is a
    /// nested scalar KeyMap present on both sides, which is merged
    /// recursively rather than replaced. A key that does not already
    /// exist is an error when `MapLocked` is set.
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

    /// Copy the single entry `key` from `source` into this KeyMap.
    /// Mirrors `astMapCopyEntry` (`keymap.c:4264`).
    ///
    /// `merge` selects whether a nested scalar KeyMap present on both
    /// sides is merged recursively or replaced outright. A key absent
    /// from `source` is silently ignored (`keymap.c:4356`, `if(
    /// in_entry )`).
    ///
    /// Each KeyMap's own `KeyCase` normalises the key it is searched with,
    /// and the entry is stored under the destination's form: `KeyCase` can
    /// only be changed while a KeyMap is empty, so every key a KeyMap holds
    /// should be in that KeyMap's own casing (C `ConvertKey( that, ... )` and
    /// `ConvertKey( this, ... )`, keymap.c:4436-4441).
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

    /// Normalise `key` (trailing spaces only — the key has already been
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

    /// Store `values`, replacing any existing entry at `key`. Mirrors
    /// `astMapPut1I`. Fails with `AST__BADKEY` when `MapLocked` is set and
    /// `key` is new.
    pub fn put_1i(&mut self, key: &str, values: &[i32], comment: Option<&str>) -> AstResult<()> {
        self.put_vector(
            key,
            EntryValue::Int(Scalar1D::Vector(values.to_vec())),
            comment,
            "astMapPut1I",
        )
    }

    /// Store `values`, replacing any existing entry at `key`. Mirrors
    /// `astMapPut1S`. Fails with `AST__BADKEY` when `MapLocked` is set and
    /// `key` is new.
    pub fn put_1s(&mut self, key: &str, values: &[i16], comment: Option<&str>) -> AstResult<()> {
        self.put_vector(
            key,
            EntryValue::SInt(Scalar1D::Vector(values.to_vec())),
            comment,
            "astMapPut1S",
        )
    }

    /// Store `values`, replacing any existing entry at `key`. Mirrors
    /// `astMapPut1K`. Fails with `AST__BADKEY` when `MapLocked` is set and
    /// `key` is new.
    pub fn put_1k(&mut self, key: &str, values: &[i64], comment: Option<&str>) -> AstResult<()> {
        self.put_vector(
            key,
            EntryValue::KInt(Scalar1D::Vector(values.to_vec())),
            comment,
            "astMapPut1K",
        )
    }

    /// Store `values`, replacing any existing entry at `key`. Mirrors
    /// `astMapPut1B`. Fails with `AST__BADKEY` when `MapLocked` is set and
    /// `key` is new.
    pub fn put_1b(&mut self, key: &str, values: &[u8], comment: Option<&str>) -> AstResult<()> {
        self.put_vector(
            key,
            EntryValue::Byte(Scalar1D::Vector(values.to_vec())),
            comment,
            "astMapPut1B",
        )
    }

    /// Store `values`, replacing any existing entry at `key`. Mirrors
    /// `astMapPut1F`. Fails with `AST__BADKEY` when `MapLocked` is set and
    /// `key` is new.
    pub fn put_1f(&mut self, key: &str, values: &[f32], comment: Option<&str>) -> AstResult<()> {
        self.put_vector(
            key,
            EntryValue::Float(Scalar1D::Vector(values.to_vec())),
            comment,
            "astMapPut1F",
        )
    }

    /// Store `values`, replacing any existing entry at `key`. Mirrors
    /// `astMapPut1D`. Fails with `AST__BADKEY` when `MapLocked` is set and
    /// `key` is new.
    pub fn put_1d(&mut self, key: &str, values: &[f64], comment: Option<&str>) -> AstResult<()> {
        self.put_vector(
            key,
            EntryValue::Double(Scalar1D::Vector(values.to_vec())),
            comment,
            "astMapPut1D",
        )
    }

    /// Store `values`, replacing any existing entry at `key`. Mirrors
    /// `astMapPut1C`. Fails with `AST__BADKEY` when `MapLocked` is set and
    /// `key` is new.
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

    /// Store `values`, replacing any existing entry at `key`. Mirrors
    /// `astMapPut1A`.
    ///
    /// See [`KeyMap::put_0a`] for why C's `CHECK_A`/`CheckCircle` guard
    /// (`keymap.c:4989`) has no Rust counterpart here.
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

    /// Normalise `key` and hand the vector value to [`KeyMap::insert_entry`].
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

    /// Read `key` as a `Vec<i32>`. Mirrors `astMapGet1I`. See
    /// [`KeyMap::get_0i`] for the meaning of `Ok(None)` versus the two
    /// error codes; a scalar entry reads as a length-1 vector.
    pub fn get_1i(&self, key: &str) -> AstResult<Option<Vec<i32>>> {
        Ok(self
            .get_vector(key, AstType::Int, "astMapGet1I")?
            .map(|v| v.into_iter().map(unwrap_int).collect()))
    }

    /// Read `key` as a `Vec<i16>`. Mirrors `astMapGet1S`. See
    /// [`KeyMap::get_1i`] for the meaning of `Ok(None)`.
    pub fn get_1s(&self, key: &str) -> AstResult<Option<Vec<i16>>> {
        Ok(self
            .get_vector(key, AstType::SInt, "astMapGet1S")?
            .map(|v| v.into_iter().map(unwrap_sint).collect()))
    }

    /// Read `key` as a `Vec<i64>`. Mirrors `astMapGet1K`. See
    /// [`KeyMap::get_1i`] for the meaning of `Ok(None)`.
    pub fn get_1k(&self, key: &str) -> AstResult<Option<Vec<i64>>> {
        Ok(self
            .get_vector(key, AstType::KInt, "astMapGet1K")?
            .map(|v| v.into_iter().map(unwrap_kint).collect()))
    }

    /// Read `key` as a `Vec<u8>`. Mirrors `astMapGet1B`. See
    /// [`KeyMap::get_1i`] for the meaning of `Ok(None)`.
    pub fn get_1b(&self, key: &str) -> AstResult<Option<Vec<u8>>> {
        Ok(self
            .get_vector(key, AstType::Byte, "astMapGet1B")?
            .map(|v| v.into_iter().map(unwrap_byte).collect()))
    }

    /// Read `key` as a `Vec<f32>`. Mirrors `astMapGet1F`. See
    /// [`KeyMap::get_1i`] for the meaning of `Ok(None)`.
    pub fn get_1f(&self, key: &str) -> AstResult<Option<Vec<f32>>> {
        Ok(self
            .get_vector(key, AstType::Float, "astMapGet1F")?
            .map(|v| v.into_iter().map(unwrap_float).collect()))
    }

    /// Read `key` as a `Vec<f64>`. Mirrors `astMapGet1D`. See
    /// [`KeyMap::get_1i`] for the meaning of `Ok(None)`.
    pub fn get_1d(&self, key: &str) -> AstResult<Option<Vec<f64>>> {
        Ok(self
            .get_vector(key, AstType::Double, "astMapGet1D")?
            .map(|v| v.into_iter().map(unwrap_double).collect()))
    }

    /// Read `key` as a `Vec<String>`. Mirrors `astMapGet1C`. See
    /// [`KeyMap::get_1i`] for the meaning of `Ok(None)`.
    ///
    /// C's `astMapGet1C` truncates each string to a caller-supplied
    /// buffer length; the Rust vector is sized to the entry instead, so
    /// there is no analogous truncation here. The truncating behaviour
    /// is observable only through [`KeyMap::map_get_c`], which reproduces
    /// C's fixed-stride buffer explicitly.
    pub fn get_1c(&self, key: &str) -> AstResult<Option<Vec<String>>> {
        Ok(self
            .get_vector(key, AstType::String, "astMapGet1C")?
            .map(|v| v.into_iter().map(unwrap_string).collect()))
    }

    /// Read `key` as a `Vec<Option<AnyObject>>`. Mirrors `astMapGet1A`.
    /// See [`KeyMap::get_1i`] for the meaning of `Ok(None)`.
    ///
    /// A `None` element is C's stored NULL Object pointer
    /// (`astMapPut1A`'s `value[i] ? astClone(value[i]) : NULL`,
    /// `keymap.c:5012`), not a conversion failure.
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

    /// Store `value` at zero-based element `elem` of `key`. Mirrors
    /// `astMapPutElemI`.
    pub fn put_elem_i(&mut self, key: &str, elem: usize, value: i32) -> AstResult<()> {
        self.put_elem(
            key,
            elem,
            RawValue::Int(value),
            "astMapPutElemI",
            "astMapPut1I",
        )
    }

    /// Store `value` at zero-based element `elem` of `key`. Mirrors
    /// `astMapPutElemS`.
    pub fn put_elem_s(&mut self, key: &str, elem: usize, value: i16) -> AstResult<()> {
        self.put_elem(
            key,
            elem,
            RawValue::SInt(value),
            "astMapPutElemS",
            "astMapPut1S",
        )
    }

    /// Store `value` at zero-based element `elem` of `key`. Mirrors
    /// `astMapPutElemK`.
    pub fn put_elem_k(&mut self, key: &str, elem: usize, value: i64) -> AstResult<()> {
        self.put_elem(
            key,
            elem,
            RawValue::KInt(value),
            "astMapPutElemK",
            "astMapPut1K",
        )
    }

    /// Store `value` at zero-based element `elem` of `key`. Mirrors
    /// `astMapPutElemB`.
    pub fn put_elem_b(&mut self, key: &str, elem: usize, value: u8) -> AstResult<()> {
        self.put_elem(
            key,
            elem,
            RawValue::Byte(value),
            "astMapPutElemB",
            "astMapPut1B",
        )
    }

    /// Store `value` at zero-based element `elem` of `key`. Mirrors
    /// `astMapPutElemF`.
    pub fn put_elem_f(&mut self, key: &str, elem: usize, value: f32) -> AstResult<()> {
        self.put_elem(
            key,
            elem,
            RawValue::Float(value),
            "astMapPutElemF",
            "astMapPut1F",
        )
    }

    /// Store `value` at zero-based element `elem` of `key`. Mirrors
    /// `astMapPutElemD`.
    pub fn put_elem_d(&mut self, key: &str, elem: usize, value: f64) -> AstResult<()> {
        self.put_elem(
            key,
            elem,
            RawValue::Double(value),
            "astMapPutElemD",
            "astMapPut1D",
        )
    }

    /// Store `value` at zero-based element `elem` of `key`. Mirrors
    /// `astMapPutElemC`.
    pub fn put_elem_c(&mut self, key: &str, elem: usize, value: &str) -> AstResult<()> {
        self.put_elem(
            key,
            elem,
            RawValue::String(value.to_string()),
            "astMapPutElemC",
            "astMapPut1C",
        )
    }

    /// Store `value` at zero-based element `elem` of `key`. Mirrors
    /// `astMapPutElemA`.
    ///
    /// See [`KeyMap::put_0a`] for why C's `CHECK_A`/`CheckCircle` guard
    /// (`keymap.c:8461`) has no Rust counterpart here.
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

    /// Read zero-based element `elem` of `key` as an `i32`. Mirrors
    /// `astMapGetElemI`.
    pub fn get_elem_i(&self, key: &str, elem: usize) -> AstResult<Option<i32>> {
        Ok(self
            .get_elem(key, elem, AstType::Int, "astMapGetElemI")?
            .map(unwrap_int))
    }

    /// Read zero-based element `elem` of `key` as an `i16`. Mirrors
    /// `astMapGetElemS`.
    pub fn get_elem_s(&self, key: &str, elem: usize) -> AstResult<Option<i16>> {
        Ok(self
            .get_elem(key, elem, AstType::SInt, "astMapGetElemS")?
            .map(unwrap_sint))
    }

    /// Read zero-based element `elem` of `key` as an `i64`. Mirrors
    /// `astMapGetElemK`.
    pub fn get_elem_k(&self, key: &str, elem: usize) -> AstResult<Option<i64>> {
        Ok(self
            .get_elem(key, elem, AstType::KInt, "astMapGetElemK")?
            .map(unwrap_kint))
    }

    /// Read zero-based element `elem` of `key` as a `u8`. Mirrors
    /// `astMapGetElemB`.
    pub fn get_elem_b(&self, key: &str, elem: usize) -> AstResult<Option<u8>> {
        Ok(self
            .get_elem(key, elem, AstType::Byte, "astMapGetElemB")?
            .map(unwrap_byte))
    }

    /// Read zero-based element `elem` of `key` as an `f32`. Mirrors
    /// `astMapGetElemF`.
    pub fn get_elem_f(&self, key: &str, elem: usize) -> AstResult<Option<f32>> {
        Ok(self
            .get_elem(key, elem, AstType::Float, "astMapGetElemF")?
            .map(unwrap_float))
    }

    /// Read zero-based element `elem` of `key` as an `f64`. Mirrors
    /// `astMapGetElemD`.
    pub fn get_elem_d(&self, key: &str, elem: usize) -> AstResult<Option<f64>> {
        Ok(self
            .get_elem(key, elem, AstType::Double, "astMapGetElemD")?
            .map(unwrap_double))
    }

    /// Read zero-based element `elem` of `key` as a `String`. Mirrors
    /// `astMapGetElemC`.
    pub fn get_elem_c(&self, key: &str, elem: usize) -> AstResult<Option<String>> {
        Ok(self
            .get_elem(key, elem, AstType::String, "astMapGetElemC")?
            .map(unwrap_string))
    }

    /// Read zero-based element `elem` of `key` as an Object. Mirrors
    /// `astMapGetElemA`. See [`KeyMap::get_1a`] for what a `None` element
    /// means.
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
            return Err(AstError::new(
                ErrorCode::Mpvin,
                format!(
                    "{method}(KeyMap): Illegal zero-based vector index {elem} \
                     supplied for KeyMap entry '{nkey}' - the vector has {n} elements."
                ),
            ));
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

    /// The number of values held at `key`: 0 if the key is absent, 1 for
    /// a scalar, the vector length otherwise. Mirrors `astMapLength`,
    /// which has no `KeyError` branch.
    pub fn map_length(&self, key: &str) -> usize {
        let Ok(nkey) = table::convert_key(key, self.key_case(), "astMapLength") else {
            return 0;
        };
        match self.lookup(&nkey) {
            Some(entry) => entry.value.len().max(1),
            None => 0,
        }
    }

    /// The length of the longest value at `key` when formatted as a
    /// string, excluding the terminating null C's own `ConvertValue`
    /// counts. Mirrors `astMapLenC` (`keymap.c:7808`).
    ///
    /// Returns 0 (not an error) for a missing key or one whose elements
    /// cannot be formatted as strings, matching C's own "no error, zero
    /// result" contract.
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

    /// The value at `key` formatted as a single string. Mirrors
    /// `astMapGetC` (`keymap.c:6466`).
    ///
    /// A length-0 entry (the key is absent, or present but undefined)
    /// gives `Ok(None)`. A length-1 entry delegates to [`KeyMap::get_0c`]
    /// with no parentheses. A vector is joined with a bare comma
    /// (`astAppendString( ..., "," )`, `keymap.c:6614`), wrapped in
    /// parentheses.
    ///
    /// C fills a buffer of stride `mxlen + 1` per element — `mxlen` being
    /// [`KeyMap::map_len_c`]'s longest formatted length — via `strncpy`
    /// truncated to `mxlen` bytes plus an explicit null, then walks that
    /// buffer reading each slot as a null-terminated string. Because
    /// `mxlen` is by construction at least as long as every element's own
    /// formatted length, the `strncpy` never actually truncates: each
    /// slot holds exactly the element's own string followed by null
    /// padding, which reads back byte-for-byte as that same string. So
    /// joining [`KeyMap::get_1c`]'s untruncated strings directly, with no
    /// space-padding of short elements, reproduces C's buffer walk
    /// exactly — an empty element contributes nothing between its commas,
    /// not `mxlen` spaces.
    pub fn map_get_c(&self, key: &str) -> AstResult<Option<String>> {
        // C's astMapGetC asks astMapLength for the entry's length first, so a
        // key too long for a case-insensitive KeyMap is reported under that
        // name. `map_length` cannot report it, so the key is checked here.
        table::convert_key(key, self.key_case(), "astMapLength")?;
        let nval = self.map_length(key);
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

    /// The data type of the value stored at `key`, or `AstType::BadType`
    /// if `key` is not present. Mirrors `astMapType` (`keymap.c:8496`),
    /// which reports no error for a missing key whatever `KeyError` is
    /// set to.
    pub fn map_type(&self, key: &str) -> AstResult<AstType> {
        let nkey = table::convert_key(key, self.key_case(), "astMapType")?;
        Ok(match self.lookup(&nkey) {
            Some(entry) => entry.value.ast_type(),
            None => AstType::BadType,
        })
    }

    /// Whether `key` is present, whether or not it has a value. Mirrors
    /// `astMapHasKey` (`keymap.c:7448`), which never consults `KeyError`.
    pub fn map_has_key(&self, key: &str) -> AstResult<bool> {
        let nkey = table::convert_key(key, self.key_case(), "astMapHasKey")?;
        Ok(self.lookup(&nkey).is_some())
    }

    /// Whether `key` is present *and* has a value. Mirrors `astMapDefined`
    /// (`keymap.c:7347`).
    ///
    /// Unlike [`KeyMap::map_has_key`], a genuinely absent key does consult
    /// `KeyError`: C's `MapDefined` only skips the `KeyError` branch when
    /// `SearchTableEntry` finds the entry, which happens for a
    /// present-but-undefined key too (`mapentry->type == AST__UNDEFTYPE`
    /// gives `result = 0` without going near the `else if` that reports
    /// `AST__MPKER`). A key that is not in the table at all falls into
    /// that `else if`, exactly as `KeyMap::get_scalar` does for a
    /// missing key.
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

    /// Remove `key` if present. Removing an absent key is not an error.
    /// Mirrors `astMapRemove` (`keymap.c:7545`): `RemoveTableEntry`
    /// returns NULL for a missing key and `FreeMapEntry(NULL)` is a
    /// no-op.
    pub fn map_remove(&mut self, key: &str) -> AstResult<()> {
        let nkey = table::convert_key(key, self.key_case(), "astMapRemove")?;
        self.entries.remove(nkey.trim_end_matches(' '));
        Ok(())
    }

    /// The comment stored with `key`, or `None` if the key is absent or
    /// has no comment.
    ///
    /// This has no C counterpart: C exposes an entry's comment only
    /// through the serialised form. Rust adds it because `to_node` needs
    /// it and because a caller who can set a comment should be able to
    /// read it back.
    pub fn map_comment(&self, key: &str) -> Option<String> {
        let nkey = table::convert_key(key, self.key_case(), "map_comment").ok()?;
        self.lookup(&nkey).and_then(|e| e.comment.clone())
    }

    /// Move the entry at `old_key` to `new_key`, replacing any entry
    /// already stored there. Mirrors `astMapRename` (`keymap.c:7608`).
    ///
    /// A no-op if the two normalised keys are equal (C's whole body is
    /// guarded on `strcmp(oldkey, newkey)`), and silent if `old_key` is
    /// absent (C's `RemoveTableEntry` returns NULL and the rest of the
    /// body is skipped). Otherwise the destination's `keymember` is
    /// inherited if it was already occupied, and a fresh one is
    /// allocated if it was not — `AddTableEntry`'s usual rule.
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

        // C reports AST__BADKEY here and leaves the KeyMap with the old
        // key gone and the new key never written, losing the entry (see
        // docs/issues/c-library-quirks.md). Rust puts the entry back
        // first, so a failed rename leaves the KeyMap unchanged.
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

    /// The key at `index` in read-out order. Mirrors `astMapKey`
    /// (`keymap.c:4432`, via `GetKey` at `keymap.c:3430`), including its
    /// error for an out-of-range index.
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

    /// The next key in read-out order, or `None` once every key has been
    /// returned. Pass `reset` to restart from the first key. Mirrors
    /// `astMapIterate` (`keymap.c:8599`).
    ///
    /// The cursor is a plain index into `KeyMap::sorted_keys` rather
    /// than C's saved hash-table/sorted-list position, so unlike C the
    /// walk always terminates, even with `SortBy` set to a mode whose
    /// sorted list is circular (see `docs/issues/c-library-quirks.md`).
    /// As in C, once the walk is exhausted the cursor resets, so the next
    /// call without `reset` starts again from the first key.
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
