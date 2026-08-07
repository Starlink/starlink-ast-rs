//! The parts of C's chained bucket table that a `HashMap`-backed KeyMap
//! still needs: the hash function, the trailing-space-insensitive key
//! comparison, key normalisation, and the arithmetic that reconstructs
//! the `MapSz` card at dump time.

use crate::error::{AstError, AstResult, ErrorCode};
use std::cmp::Ordering;

/// The smallest hash table C ever allocates.
pub(crate) const MIN_TABLE_SIZE: usize = 16;

/// C doubles the table when a single bucket holds more than this many
/// entries.
pub(crate) const MAX_ENTRIES_PER_TABLE_ENTRY: usize = 10;

/// `AST__MXKEYLEN`: the key buffer length used when `KeyCase` is 0.
/// The longest usable key is one character shorter, because C
/// null-terminates within the buffer.
pub(crate) const MXKEYLEN: usize = 200;

/// The used length of a string, ignoring trailing white space and
/// non-printing characters. Mirrors `astChrLen`.
pub(crate) fn chr_len(s: &str) -> usize {
    s.trim_end_matches(|c: char| !c.is_ascii_graphic() || c.is_whitespace())
        .len()
}

/// The bucket index for a key.
///
/// djb2 (`hash * 33 + c`) seeded at 5381, accumulated in an unsigned
/// 64-bit register and masked down to the table size. Spaces are ignored
/// so that trailing spaces used to pad F77 character variables will be
/// ignored.
pub(crate) fn hash_fun(key: &str, bitmask: u64) -> usize {
    let mut hash: u64 = 5381;
    for c in key.bytes() {
        if c != b' ' {
            hash = hash
                .wrapping_shl(5)
                .wrapping_add(hash)
                .wrapping_add(c as u64);
        }
    }
    (hash & bitmask) as usize
}

/// Compare two keys, ignoring trailing spaces. Mirrors `KeyCmp`, whose
/// `1` / `0` / `-1` become `Greater` / `Equal` / `Less`.
pub(crate) fn key_cmp(key1: &str, key2: &str) -> Ordering {
    let (b1, b2) = (key1.as_bytes(), key2.as_bytes());

    // Advance to the first differing byte, or to the end of the shorter
    // string.
    let mut i = 0;
    while i < b1.len() && i < b2.len() && b1[i] == b2[i] {
        i += 1;
    }

    match (i < b1.len(), i < b2.len()) {
        // Both still have characters, and they differ.
        (true, true) => b1[i].cmp(&b2[i]),
        // key2 ended: key1 matches if the rest of it is only spaces.
        (true, false) => {
            if b1[i..].iter().all(|&c| c == b' ') {
                Ordering::Equal
            } else {
                Ordering::Greater
            }
        }
        // key1 ended: mirror image.
        (false, true) => {
            if b2[i..].iter().all(|&c| c == b' ') {
                Ordering::Equal
            } else {
                Ordering::Less
            }
        }
        (false, false) => Ordering::Equal,
    }
}

/// Normalise a supplied key: when `KeyCase` is 0 the key is
/// upper-cased, and an over-long key is an error. Mirrors `ConvertKey`.
///
/// `method` is the reporting name of the calling public function, which
/// C uses verbatim in the error message.
pub(crate) fn convert_key(key: &str, key_case: bool, method: &str) -> AstResult<String> {
    if key_case {
        return Ok(key.to_string());
    }
    if chr_len(key) >= MXKEYLEN {
        return Err(AstError::new(
            ErrorCode::Bigkey,
            format!(
                "{method}(KeyMap): Supplied key '{key}' is too long (keys must be \
                 no more than {} characters long).",
                MXKEYLEN - 1
            ),
        ));
    }
    Ok(key.to_ascii_uppercase())
}

/// The table size `NewTable` allocates for a requested size: clamped up
/// to `MIN_TABLE_SIZE`, otherwise rounded up to a power of two.
pub(crate) fn new_table_size(size: usize) -> usize {
    if size <= MIN_TABLE_SIZE {
        MIN_TABLE_SIZE
    } else {
        size.next_power_of_two()
    }
}

/// The `MapSz` value to dump for a map holding `keys`.
///
/// C's `mapsize` is a running record of the collision pressure the map
/// has experienced, so it depends on the order keys were added and on
/// keys that have since been removed. This reconstructs the value a
/// freshly built C KeyMap holding exactly these keys would carry:
/// `NewTable` from `SizeGuess`, then `AddTableEntry`'s doubling rule
/// replayed over the keys in dump order.
///
/// `astMapKey`-order keys must be passed, so the result is a function of
/// the map's contents alone.
pub(crate) fn map_sz(keys: &[String], sizeguess: Option<usize>) -> usize {
    let mut mapsize = match sizeguess {
        Some(g) => new_table_size(g / MAX_ENTRIES_PER_TABLE_ENTRY),
        None => MIN_TABLE_SIZE,
    };

    let mut counts = vec![0usize; mapsize];
    let mut placed = 0usize;
    while placed < keys.len() {
        let itab = hash_fun(&keys[placed], (mapsize - 1) as u64);
        counts[itab] += 1;
        placed += 1;

        // DoubleTableSize fires when one bucket passes the limit. It
        // doubles once and redistributes, without re-checking the new
        // distribution, so a single insert never doubles twice.
        if counts[itab] > MAX_ENTRIES_PER_TABLE_ENTRY {
            mapsize *= 2;
            counts = vec![0usize; mapsize];
            for key in &keys[..placed] {
                counts[hash_fun(key, (mapsize - 1) as u64)] += 1;
            }
        }
    }

    mapsize
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn hash_fun_is_djb2_skipping_spaces() {
        // reference/starlink-ast/src/keymap.c HashFun: hash starts at 5381
        // and every non-space character contributes hash*33 + c. Spaces are
        // skipped so that trailing spaces padding an F77 CHARACTER variable
        // do not change the bucket.
        fn djb2(s: &str) -> u64 {
            let mut h: u64 = 5381;
            for b in s.bytes() {
                if b != b' ' {
                    h = h.wrapping_shl(5).wrapping_add(h).wrapping_add(b as u64);
                }
            }
            h
        }
        let mask = (MIN_TABLE_SIZE - 1) as u64;
        for key in ["A", "ABC", "HE-HE", "this_is_a_key", "zzzzzzzzzzz", "Size"] {
            assert_eq!(hash_fun(key, mask), (djb2(key) & mask) as usize, "{key}");
        }

        // Spaces anywhere are skipped, not just trailing ones.
        assert_eq!(hash_fun("AB", mask), hash_fun("A B  ", mask));
        assert_eq!(hash_fun("", mask), (5381u64 & mask) as usize);
    }

    #[test]
    fn key_cmp_ignores_trailing_spaces() {
        // C returns 0 for equal, +1 / -1 otherwise, and only trailing
        // spaces on the *shorter* side are skipped.
        assert_eq!(key_cmp("FRED", "FRED"), Ordering::Equal);
        assert_eq!(key_cmp("FRED", "FRED   "), Ordering::Equal);
        assert_eq!(key_cmp("FRED   ", "FRED"), Ordering::Equal);
        assert_eq!(key_cmp("FRED", "FREDA"), Ordering::Less);
        assert_eq!(key_cmp("FREDA", "FRED"), Ordering::Greater);
        assert_eq!(key_cmp("FRED", "FRED A"), Ordering::Less);
        assert_eq!(key_cmp("ABC", "ABD"), Ordering::Less);
        // Leading and interior spaces are significant: ' ' (0x20) sorts
        // below every printing character.
        assert_eq!(key_cmp(" FRED", "FRED"), Ordering::Less);
    }

    #[test]
    fn convert_key_upper_cases_only_when_case_insensitive() {
        assert_eq!(convert_key("fReDs", true, "astMapPut0I").unwrap(), "fReDs");
        assert_eq!(convert_key("fReDs", false, "astMapPut0I").unwrap(), "FREDS");
        // KeyCase=1 skips the length check entirely, matching C.
        let long = "x".repeat(500);
        assert_eq!(convert_key(&long, true, "astMapPut0I").unwrap(), long);
    }

    #[test]
    fn convert_key_rejects_over_long_keys_when_case_insensitive() {
        // astChrLen strips trailing whitespace and non-printing characters
        // before the length test, so a padded 200-character key passes.
        let exact = "x".repeat(MXKEYLEN - 1);
        assert!(convert_key(&exact, false, "astMapPut0I").is_ok());
        let padded = format!("{exact}          ");
        assert!(convert_key(&padded, false, "astMapPut0I").is_ok());

        let too_long = "x".repeat(MXKEYLEN);
        let err = convert_key(&too_long, false, "astMapPut0I").unwrap_err();
        assert_eq!(err.code(), ErrorCode::Bigkey);
        assert!(
            err.to_string()
                .contains("keys must be no more than 199 characters long"),
            "{err}"
        );
    }

    #[test]
    fn chr_len_strips_trailing_whitespace_and_non_printing() {
        assert_eq!(chr_len("FRED"), 4);
        assert_eq!(chr_len("FRED   "), 4);
        assert_eq!(chr_len("FRED\t\n"), 4);
        assert_eq!(chr_len("FRED\0\0"), 4);
        assert_eq!(chr_len("   "), 0);
        assert_eq!(chr_len(""), 0);
        // Interior whitespace is not touched.
        assert_eq!(chr_len("F R "), 3);
    }

    #[test]
    fn new_table_size_rounds_up_to_a_power_of_two() {
        // keymap.c NewTable: sizes at or below MIN_TABLE_SIZE clamp to it,
        // anything larger rounds up to the next power of two.
        assert_eq!(new_table_size(0), 16);
        assert_eq!(new_table_size(1), 16);
        assert_eq!(new_table_size(16), 16);
        assert_eq!(new_table_size(17), 32);
        assert_eq!(new_table_size(32), 32);
        assert_eq!(new_table_size(33), 64);
        assert_eq!(new_table_size(1000), 1024);
        assert_eq!(new_table_size(1024), 1024);
        assert_eq!(new_table_size(200_000), 262_144);
    }

    #[test]
    fn map_sz_is_min_table_size_for_small_maps() {
        // Four keys in a 16-bucket table cannot put 11 in one bucket.
        let keys: Vec<String> = ["Size", "Name", "Resolution", "Error"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(map_sz(&keys, None), 16);
    }

    #[test]
    fn map_sz_honours_size_guess_like_set_size_guess() {
        // SetSizeGuess divides by MAX_ENTRIES_PER_TABLE_ENTRY and hands the
        // quotient to NewTable, which clamps at MIN_TABLE_SIZE. So any
        // guess below 170 leaves the table at 16.
        assert_eq!(map_sz(&[], Some(100)), 16);
        assert_eq!(map_sz(&[], Some(160)), 16);
        assert_eq!(map_sz(&[], Some(170)), 32);
        assert_eq!(map_sz(&[], Some(5000)), 512);
    }

    #[test]
    fn map_sz_doubles_when_a_bucket_overflows() {
        // AddTableEntry doubles the table as soon as a *single* bucket
        // holds more than MAX_ENTRIES_PER_TABLE_ENTRY entries, so a map
        // large enough to guarantee that must report more than 16.
        let keys: Vec<String> = (0..500).map(|i| format!("KEY_{i}")).collect();
        let sz = map_sz(&keys, None);
        assert!(sz > MIN_TABLE_SIZE, "{sz}");
        assert!(sz.is_power_of_two(), "{sz}");
        // Insertion order must not matter: the dump order is what is
        // replayed, and the dump order is deterministic.
        let mut reversed = keys.clone();
        reversed.reverse();
        assert_eq!(map_sz(&reversed, None), sz);
    }

    /// The first `n` keys of the form `K<i>` that land in bucket 5 of a
    /// 16-bucket table.
    ///
    /// A spread key set exercises the doubling rule only incidentally. A set
    /// that all lands in one bucket exercises it on the eleventh insert, and
    /// keeps exercising it as the doubling redistributes the set across the
    /// wider mask. The `keymap_hash` oracle probe builds the same set from
    /// the same rule, so the two sides can be compared without transporting
    /// a key list between them.
    fn colliding_keys(n: usize) -> Vec<String> {
        let mut out = Vec::with_capacity(n);
        let mut i = 0u64;
        while out.len() < n {
            let key = format!("K{i}");
            if hash_fun(&key, 15) == 5 {
                out.push(key);
            }
            i += 1;
        }
        out
    }

    #[test]
    fn map_sz_replays_the_doubling_rule_for_a_colliding_key_set() {
        // Ten in one bucket is at the limit, so no doubling: the rule is
        // `nentry > MAX_ENTRIES_PER_TABLE_ENTRY`, not `>=`.
        assert_eq!(map_sz(&colliding_keys(10), None), 16);

        // The eleventh forces a doubling, and the size then grows
        // monotonically with the set. The exact value at each n is a
        // property of djb2 that this file cannot derive without restating
        // the algorithm, so it is pinned against the C oracle in
        // tests/fixtures/keymap/mapsz_oracle.txt rather than asserted here.
        assert!(map_sz(&colliding_keys(11), None) > 16);
        let mut prev = 16;
        for n in [11usize, 40, 300] {
            let sz = map_sz(&colliding_keys(n), None);
            assert!(sz.is_power_of_two(), "n={n} gave {sz}");
            assert!(sz >= prev, "n={n} gave {sz}, down from {prev}");
            prev = sz;
        }

        // A colliding set mixed with a spread one: the collisions drive the
        // doubling, so the mixture cannot be smaller than the collisions
        // alone.
        let mut mixed = colliding_keys(40);
        mixed.extend((0..40).map(|i| format!("SPREAD_{i}")));
        assert!(map_sz(&mixed, None) >= map_sz(&colliding_keys(40), None));
    }
}
