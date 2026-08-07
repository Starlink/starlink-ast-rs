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
use crate::keymap::entry::MapEntry;
use crate::keymap::table::{MAX_ENTRIES_PER_TABLE_ENTRY, MIN_TABLE_SIZE};
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
    /// C walks every table entry, checks `type == AST__OBJECTTYPE`, and
    /// tests each held Object with `astIsAKeyMap`
    /// (`keymap.c:1250`-1268). `AnyObject` — the enum an Object entry's
    /// payload is stored as — does not yet carry a `KeyMap` variant, so
    /// no Object entry can presently hold a nested KeyMap and this
    /// always returns empty. `set_key_error` and `set_map_locked` call
    /// it unconditionally regardless, so recursion starts working the
    /// moment that variant exists, with no change needed here.
    fn nested_key_maps_mut(&mut self) -> Vec<&mut KeyMap> {
        Vec::new()
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

    // Enabled in Task 7 (put_0i lands there).
    // #[test]
    // fn size_guess_is_frozen_once_the_map_is_not_empty() {
    //     let mut km = KeyMap::new();
    //     km.put_0i("A", 1, None).unwrap();
    //
    //     let err = km.set_size_guess(1000).unwrap_err();
    //     assert_eq!(err.code(), ErrorCode::Nowrt);
    //     assert!(
    //         err.to_string().contains(
    //             "Illegal attempt to change the SizeGuess attribute of a non-empty KeyMap."
    //         ),
    //         "{err}"
    //     );
    //     // The failed set left the attribute alone.
    //     assert!(!km.test_size_guess());
    //
    //     let err = km.clear_size_guess().unwrap_err();
    //     assert_eq!(err.code(), ErrorCode::Nowrt);
    //     assert!(
    //         err.to_string().contains(
    //             "Illegal attempt to clear the SizeGuess attribute of a non-empty KeyMap."
    //         ),
    //         "{err}"
    //     );
    // }

    // Enabled in Task 7 (put_0i lands there).
    // #[test]
    // fn key_case_is_frozen_only_when_the_value_would_change() {
    //     let mut km = KeyMap::new();
    //     km.put_0i("A", 1, None).unwrap();
    //
    //     // Setting it to the value it already reports is a no-op, because
    //     // C's SetKeyCase guards the emptiness check on `newval != oldval`.
    //     assert!(km.key_case());
    //     km.set_key_case(true).unwrap();
    //     assert!(km.test_key_case());
    //
    //     let err = km.set_key_case(false).unwrap_err();
    //     assert_eq!(err.code(), ErrorCode::Nowrt);
    //     assert!(
    //         err.to_string().contains(
    //             "Illegal attempt to change the KeyCase attribute of a non-empty KeyMap."
    //         ),
    //         "{err}"
    //     );
    //     // Still true: the failed set restored the old value.
    //     assert!(km.key_case());
    //
    //     let err = km.clear_key_case().unwrap_err();
    //     assert_eq!(err.code(), ErrorCode::Nowrt);
    //     assert!(
    //         err.to_string().contains(
    //             "Illegal attempt to clear the KeyCase attribute of a non-empty KeyMap."
    //         ),
    //         "{err}"
    //     );
    //     assert!(km.test_key_case());
    // }

    // Enabled in Task 9 (sorted_keys, get_object_ref, and AnyObject::KeyMap land there).
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

    // Enabled in Task 9 (get_object_ref and AnyObject::KeyMap land there).
    // #[test]
    // fn sort_by_and_size_guess_do_not_recurse() {
    //     let mut inner = KeyMap::new();
    //     inner.put_0i("X", 1, None).unwrap();
    //     let mut outer = KeyMap::new();
    //     outer.put_0a("INNER", AnyObject::KeyMap(Box::new(inner)), None).unwrap();
    //
    //     outer.set_sort_by(SortBy::KeyUp);
    //     let Some(AnyObject::KeyMap(nested)) = outer.get_object_ref("INNER") else {
    //         panic!("nested KeyMap missing");
    //     };
    //     assert_eq!(nested.sort_by(), SortBy::None);
    //     assert!(!nested.test_sort_by());
    // }
}
