//! Native-format serialisation for `KeyMap`.
//!
//! Ported from `Dump`, `DumpEntry` and `astLoadKeyMap` in
//! `reference/starlink-ast/src/keymap.c`.

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::keymap::entry::{AstType, EntryValue, MapEntry, Scalar1D};
use crate::keymap::sort::SortBy;
use crate::keymap::{KeyMap, table};
use crate::serial::node::Node;
use crate::serial::{Serializable, any_object_from_node, any_object_to_node};

impl Serializable for KeyMap {
    fn class_name(&self) -> &'static str {
        "KeyMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new("KeyMap");
        node.class_comment = Some("Map of key/value pairs".to_string());

        // The five attributes, each omitted entirely when unset.
        push_int_attr(
            &mut node,
            "SzGss",
            self.size_guess() as i64,
            self.test_size_guess(),
            "Guess at KeyMap size",
        );
        push_string_attr(
            &mut node,
            "SortBy",
            self.sort_by().as_str(),
            self.test_sort_by(),
            "Sorting scheme for keys",
        );
        push_int_attr(
            &mut node,
            "KyCas",
            self.key_case() as i64,
            self.test_key_case(),
            "Are keys case sensitive?",
        );
        push_int_attr(
            &mut node,
            "KyErr",
            self.key_error() as i64,
            self.test_key_error(),
            "Report non-existant keys?",
        );
        push_int_attr(
            &mut node,
            "MpLck",
            self.map_locked() as i64,
            self.test_map_locked(),
            "Prevent addition of new entries?",
        );

        // Both unconditional in C.
        node.push_int(
            "MapSz",
            self.map_sz_for_dump(),
            Some("Size of hash table".to_string()),
        );
        node.push_int(
            "MemCnt",
            self.member_count as i64,
            Some("Total member count".to_string()),
        );

        for (n, key) in self.dump_keys().into_iter().enumerate() {
            let entry = &self.entries[&key];
            dump_entry(&mut node, n + 1, &key, entry)?;
        }
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let mut km = KeyMap::new();

        // C reads each attribute before any entry, and applies it through
        // its own setter. SizeGuess and KeyCase refuse on a non-empty map,
        // which is why the read has to happen first.
        if let Some(v) = node.get_int_any_if_set("SzGss") {
            // A negative card would wrap to an enormous `usize` and blow
            // up the `HashMap::reserve` inside `set_size_guess`. C's own
            // `SetSizeGuess`/`NewTable` (`keymap.c:9463`, `keymap.c:8720`)
            // clamps any size at or below `MIN_TABLE_SIZE` -- which
            // includes zero, negative, and any small positive quotient --
            // down to the same minimum table without erroring, so
            // treating a negative guess as 0 here has the same "smallest
            // table" effect C's own clamp would produce.
            km.set_size_guess(v.max(0) as usize)?;
        }
        if let Some(v) = node.get_string_if_set("SortBy") {
            km.set_sort_by(SortBy::parse(&v, "astRead")?);
        }
        if let Some(v) = node.get_int_any_if_set("KyCas") {
            km.set_key_case(v != 0)?;
        }
        if let Some(v) = node.get_int_any_if_set("KyErr") {
            km.set_key_error(v != 0);
        }
        if let Some(v) = node.get_int_any_if_set("MpLck") {
            km.set_map_locked(v != 0);
        }

        // MapSz is C's bucket-count history and has no HashMap counterpart,
        // so it is read and discarded. The written value is recomputed from
        // the contents at dump time.

        // Presize from the number of Key%d cards present. C has no presize
        // step at all -- this is purely a Rust-side allocation optimisation
        // -- so there is no C behaviour to mirror here, only the
        // requirement that it never panics. A node key can contain
        // non-ASCII characters (the native reader's key-boundary predicate
        // is `char::is_alphanumeric`, which is Unicode-aware), so this must
        // not byte-slice the key by a fixed offset: `str::get` returns
        // `None` for an out-of-range or non-boundary index instead of
        // panicking, unlike `Index`.
        let n_entries = node
            .items
            .iter()
            .filter(|i| {
                i.key
                    .get(..3)
                    .is_some_and(|p| p.eq_ignore_ascii_case("key"))
                    && i.key.get(3..).is_some_and(|rest| {
                        !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
                    })
            })
            .count();
        km.entries.reserve(n_entries);

        let mut n = 0;
        loop {
            n += 1;
            // The loop stops at the first missing key, so a gap in the
            // numbering truncates the load.
            let Some(key) = node.get_string_if_set(&format!("Key{n}")) else {
                break;
            };
            let comment = node.get_string_if_set(&format!("Com{n}"));
            load_entry(&mut km, node, n, &key, comment.as_deref())?;
        }

        // The final total, which C reads after the entries so it overrides
        // whatever the puts accumulated.
        km.member_count = node.get_int("MemCnt", 0) as u64;
        Ok(km)
    }
}

impl KeyMap {
    /// The keys in dump order, which is always `key_cmp` order.
    ///
    /// `Dump` walks the hash table directly and never consults `SortBy`,
    /// so the dump order is independent of the read-out order
    /// [`KeyMap::sorted_keys`] produces.
    pub(crate) fn dump_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.entries.keys().cloned().collect();
        keys.sort_by(|a, b| table::key_cmp(a, b));
        keys
    }

    /// The `MapSz` value to write, replayed from the contents in dump
    /// order. `Dump` writes C's `this->mapsize` directly, which is a
    /// record of collision history that a `HashMap` does not keep; this
    /// reconstructs the value a freshly built KeyMap holding exactly
    /// these entries would carry.
    pub(crate) fn map_sz_for_dump(&self) -> i64 {
        table::map_sz(&self.dump_keys(), Some(self.size_guess())) as i64
    }
}

/// Write one entry's cards, matching `DumpEntry` (`keymap.c:2700`).
fn dump_entry(node: &mut Node, n: usize, key: &str, entry: &MapEntry) -> AstResult<()> {
    node.push_string(&format!("Key{n}"), key, Some("Item name".to_string()));

    // C tests both the pointer and the first character, so an empty
    // comment is not written.
    if let Some(comment) = &entry.comment {
        if !comment.is_empty() {
            node.push_string(
                &format!("Com{n}"),
                comment,
                Some("Item comment".to_string()),
            );
        }
    }

    let ast_type = entry.value.ast_type();
    node.push_int(
        &format!("Typ{n}"),
        ast_type.code(),
        Some(ast_type.type_comment().to_string()),
    );

    let nel = entry.value.nel();
    if nel > 0 {
        node.push_int(
            &format!("Nel{n}"),
            nel as i64,
            Some("Vector length".to_string()),
        );
    }

    // The comment goes on the first element actually written, and an empty
    // string on the rest — but only for a vector: a scalar always uses
    // "Item value" regardless of whether it is written.
    let mut com: Option<&str> = Some("Item values");
    let scalar_name = format!("Val{n}");
    for i in 0..entry.value.len().max(1) {
        let name = if nel == 0 {
            scalar_name.clone()
        } else {
            format!("V{n}_{}", i + 1)
        };
        let comment = if nel == 0 {
            Some("Item value".to_string())
        } else {
            Some(com.unwrap_or("").to_string())
        };
        if push_element(node, &name, comment, &entry.value, i)? && nel > 0 {
            com = None;
        }
    }
    Ok(())
}

/// Write element `i` of `value` as a card, returning whether anything was
/// written. A `BAD_VALUE` double and a null Object write nothing,
/// matching `DumpEntry`.
fn push_element(
    node: &mut Node,
    name: &str,
    comment: Option<String>,
    value: &EntryValue,
    i: usize,
) -> AstResult<bool> {
    match value {
        // Short and byte go through astWriteInt after a cast, so both
        // appear as plain integers; so does int itself. KInt uses
        // astWriteInt64, which the Node layer does not distinguish from
        // astWriteInt — both store an i64.
        EntryValue::Int(v) => node.push_int(name, i64::from(v.as_slice()[i]), comment),
        EntryValue::SInt(v) => node.push_int(name, i64::from(v.as_slice()[i]), comment),
        EntryValue::Byte(v) => node.push_int(name, i64::from(v.as_slice()[i]), comment),
        EntryValue::KInt(v) => node.push_int(name, v.as_slice()[i], comment),
        EntryValue::Double(v) => {
            let d = v.as_slice()[i];
            if d == BAD_VALUE {
                return Ok(false);
            }
            node.push_double(name, d, comment);
        }
        // A float is widened and written as a double, with no BAD check.
        EntryValue::Float(v) => node.push_double(name, f64::from(v.as_slice()[i]), comment),
        EntryValue::String(v) => node.push_string(name, &v.as_slice()[i], comment),
        EntryValue::Object(v) => {
            // A null Object element writes no card, exactly as a BAD
            // double does.
            let Some(obj) = v.as_slice()[i].as_deref() else {
                return Ok(false);
            };
            node.push_object(name, any_object_to_node(obj)?, comment);
        }
        // An undefined entry has nothing after its Typ card.
        EntryValue::Undef { .. } => return Ok(false),
    }
    Ok(true)
}

/// Read one entry's cards and put it into `km`, matching `astLoadKeyMap`
/// (`keymap.c:10844`).
fn load_entry(
    km: &mut KeyMap,
    node: &Node,
    n: usize,
    key: &str,
    comment: Option<&str>,
) -> AstResult<()> {
    // `astReadInt` (channel.c:3092) returns its default -- AST__BADTYPE, 0
    // -- both when the card is absent and when it is a commented-out
    // default, so `get_int_any_if_set` (which treats Unset like absent) is
    // the right lookup here: a real C dump's `# Typ%d = ...` comment line
    // is never re-read on load.
    //
    // C's own check is value-based, not presence-based
    // (`if( type == AST__BADTYPE && astOK )`, keymap.c:11054): an
    // explicit `Typ%d = 0` card produces exactly the same "no data type
    // code found" error as a missing one, never "unknown data type code
    // (0)".
    let code = node
        .get_int_any_if_set(&format!("Typ{n}"))
        .unwrap_or(AstType::BadType.code());
    if code == AstType::BadType.code() {
        return Err(AstError::new(
            ErrorCode::Bdfts,
            "astLoadKeyMap(KeyMap): No data type code found whilst reading a \
             KeyMap."
                .to_string(),
        ));
    }
    let ast_type = AstType::from_code(code).ok_or_else(|| {
        AstError::new(
            ErrorCode::Bdfts,
            format!(
                "astLoadKeyMap(KeyMap): Unknown data type code ({code}) \
                 encountered whilst reading a KeyMap."
            ),
        )
    })?;

    // C's own read loop (`for( index = 0; astOK && index < nel; index++
    // )`, `keymap.c:11087` onward) with a negative `nel` never executes --
    // `index < nel` is false from the start -- so C ends up with an empty
    // allocation and no elements read, but does not error: confirmed
    // against the reference library, which loads such a dump with
    // `astMapLength` reporting the raw negative value back afterward
    // (see the c-library-quirks entry this fix logs). Clamping to 0 here
    // reproduces the same "no elements read" outcome without the
    // `usize` wraparound that would otherwise blow up the unbounded
    // `.collect()` below.
    let nel = node.get_int(&format!("Nel{n}"), 0).max(0) as usize;
    let names: Vec<String> = if nel == 0 {
        vec![format!("Val{n}")]
    } else {
        (1..=nel).map(|i| format!("V{n}_{i}")).collect()
    };

    // Loading goes through the public put path, so MapLocked and KeyCase
    // apply to the loaded keys exactly as they do to a caller's puts.
    match ast_type {
        AstType::Int => {
            let v: Vec<i32> = names.iter().map(|nm| node.get_int(nm, 0) as i32).collect();
            if nel == 0 {
                km.put_0i(key, v[0], comment)?;
            } else {
                km.put_1i(key, &v, comment)?;
            }
        }
        AstType::SInt => {
            let v: Vec<i16> = names.iter().map(|nm| node.get_int(nm, 0) as i16).collect();
            if nel == 0 {
                km.put_0s(key, v[0], comment)?;
            } else {
                km.put_1s(key, &v, comment)?;
            }
        }
        AstType::KInt => {
            let v: Vec<i64> = names.iter().map(|nm| node.get_int(nm, 0)).collect();
            if nel == 0 {
                km.put_0k(key, v[0], comment)?;
            } else {
                km.put_1k(key, &v, comment)?;
            }
        }
        AstType::Byte => {
            let v: Vec<u8> = names.iter().map(|nm| node.get_int(nm, 0) as u8).collect();
            if nel == 0 {
                km.put_0b(key, v[0], comment)?;
            } else {
                km.put_1b(key, &v, comment)?;
            }
        }
        AstType::Double => {
            let v: Vec<f64> = names
                .iter()
                .map(|nm| node.get_double(nm, BAD_VALUE))
                .collect();
            if nel == 0 {
                km.put_0d(key, v[0], comment)?;
            } else {
                km.put_1d(key, &v, comment)?;
            }
        }
        AstType::Float => {
            let v: Vec<f32> = names
                .iter()
                .map(|nm| node.get_double(nm, 0.0) as f32)
                .collect();
            if nel == 0 {
                km.put_0f(key, v[0], comment)?;
            } else {
                km.put_1f(key, &v, comment)?;
            }
        }
        AstType::String => {
            let v: Vec<String> = names.iter().map(|nm| node.get_string(nm, "")).collect();
            if nel == 0 {
                km.put_0c(key, &v[0], comment)?;
            } else {
                let refs: Vec<&str> = v.iter().map(String::as_str).collect();
                km.put_1c(key, &refs, comment)?;
            }
        }
        AstType::Object => {
            let v: Vec<Option<crate::serial::AnyObject>> = names
                .iter()
                .map(|nm| node.get_object(nm))
                .map(|n| n.map(any_object_from_node).transpose())
                .collect::<AstResult<Vec<_>>>()?;
            if nel == 0 {
                let obj = v.into_iter().next().flatten();
                if let Some(o) = &obj {
                    km.check_circle(o, "astMapPut0A")?;
                }
                km.put_scalar(
                    key,
                    EntryValue::Object(Scalar1D::Scalar(obj.map(Box::new))),
                    comment,
                    "astMapPut0A",
                )?;
            } else {
                km.put_1a(key, v, comment)?;
            }
        }
        AstType::Undef => km.put_u(key, comment)?,
        AstType::BadType => unreachable!("from_code rejects 0"),
    }
    Ok(())
}

/// Write an integer attribute, omitted entirely when `set` is false.
///
/// C passes `helpful = 0` for all five KeyMap attribute cards
/// (`SzGss`/`KyCas`/`KyErr`/`MpLck`/`SortBy`), so `Use` (`channel.c:4141`)
/// returns false for an unset one at the default `Full`: the card is
/// absent, not written as `# Key = Value`. See `write_region_base`
/// (`crates/ast-core/src/serial/mod.rs`) for the same pattern.
fn push_int_attr(node: &mut Node, key: &str, value: i64, set: bool, comment: &str) {
    if set {
        node.push_int(key, value, Some(comment.to_string()));
    }
}

/// Write a string attribute, omitted entirely when `set` is false. See
/// [`push_int_attr`].
fn push_string_attr(node: &mut Node, key: &str, value: &str, set: bool, comment: &str) {
    if set {
        node.push_string(key, value, Some(comment.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serial::AnyObject;
    use crate::serial::native_write::render_native;
    use crate::serial::node::NodeValue;

    #[test]
    fn dump_card_order_matches_c() {
        let mut km = KeyMap::new();
        km.put_0i("Size", 1, None).unwrap();
        km.put_1c("Name", &["Position", "Time"], None).unwrap();

        let node = km.to_node().unwrap();
        assert_eq!(node.class, "KeyMap");
        assert_eq!(
            node.class_comment.as_deref(),
            Some("Map of key/value pairs")
        );

        let keys: Vec<&str> = node.items.iter().map(|i| i.key.as_str()).collect();
        // The five attribute cards are all unset here, so `Use` (C's
        // channel.c:4141, `helpful = 0` for every one of them) omits them
        // entirely rather than writing a commented-out default -- see
        // `push_int_attr`/`push_string_attr`. MapSz and MemCnt are
        // unconditional and come first, then the entries in KeyCmp order.
        // NAME sorts before SIZE, so the vector entry is entry 1.
        assert_eq!(
            keys,
            vec![
                "MapSz", "MemCnt", "Key1", "Typ1", "Nel1", "V1_1", "V1_2", "Key2", "Typ2", "Val2",
            ]
        );

        // Only the first element of a vector carries the comment, and the
        // rest carry an empty one rather than none.
        let comment = |key: &str| -> Option<String> {
            node.items
                .iter()
                .find(|i| i.key == key)
                .and_then(|i| i.comment.clone())
        };
        assert_eq!(comment("Key1").as_deref(), Some("Item name"));
        assert_eq!(comment("Typ1").as_deref(), Some("Item data type (string)"));
        assert_eq!(comment("Nel1").as_deref(), Some("Vector length"));
        assert_eq!(comment("V1_1").as_deref(), Some("Item values"));
        assert_eq!(comment("V1_2").as_deref(), Some(""));
        assert_eq!(comment("Val2").as_deref(), Some("Item value"));
        assert_eq!(comment("MapSz").as_deref(), Some("Size of hash table"));
        assert_eq!(comment("MemCnt").as_deref(), Some("Total member count"));
    }

    #[test]
    fn unset_attributes_are_omitted_and_set_ones_are_not() {
        // C passes helpful=0 for all five, so `Use` (channel.c:4141) omits
        // an unset one entirely at the default Full -- there is no
        // commented-out `# Key = Value` line to find, unlike a Region
        // attribute dumped through astWriteInt/astWriteDouble with a
        // non-zero default.
        let mut km = KeyMap::new();
        km.put_0i("A", 1, None).unwrap();

        let node = km.to_node().unwrap();
        for key in ["SzGss", "SortBy", "KyCas", "KyErr", "MpLck"] {
            assert!(node.get_raw(key).is_none(), "{key} should be absent");
        }
        // MapSz and MemCnt are unconditional.
        assert!(matches!(node.get_raw("MapSz"), Some(NodeValue::Int(_))));
        assert!(matches!(node.get_raw("MemCnt"), Some(NodeValue::Int(_))));

        km.set_key_error(true);
        km.set_sort_by(SortBy::KeyUp);
        let node = km.to_node().unwrap();
        assert_eq!(node.get_int("KyErr", -1), 1);
        assert_eq!(node.get_string("SortBy", ""), "KeyUp");
    }

    #[test]
    fn a_comment_is_written_only_when_non_empty() {
        let mut km = KeyMap::new();
        km.put_0i("A", 1, Some("has one")).unwrap();
        km.put_0i("B", 2, Some("")).unwrap();
        km.put_0i("C", 3, None).unwrap();

        let node = km.to_node().unwrap();
        assert_eq!(node.get_string("Com1", ""), "has one");
        // C tests `*(entry->comment)`, so an empty comment is not written.
        assert!(node.get_raw("Com2").is_none());
        assert!(node.get_raw("Com3").is_none());
    }

    #[test]
    fn a_bad_double_is_omitted_and_reloads_as_bad() {
        let mut km = KeyMap::new();
        km.put_0d("S", BAD_VALUE, None).unwrap();
        km.put_1d("V", &[BAD_VALUE, 1.5], None).unwrap();

        let node = km.to_node().unwrap();
        // No Val card at all for the scalar.
        assert!(node.get_raw("Val1").is_none());
        // The bad element of the vector is skipped, so the comment lands on
        // the first element actually written.
        assert!(node.get_raw("V2_1").is_none());
        let v2_2 = node.items.iter().find(|i| i.key == "V2_2").unwrap();
        assert_eq!(v2_2.comment.as_deref(), Some("Item values"));

        let back = KeyMap::from_node(&node).unwrap();
        assert_eq!(back.get_0d("S").unwrap(), Some(BAD_VALUE));
        assert_eq!(back.get_1d("V").unwrap(), Some(vec![BAD_VALUE, 1.5]));
    }

    #[test]
    fn putting_a_zero_length_vector_degrades_to_a_scalar() {
        // C has no representation for nel == 0 other than "scalar"
        // (keymap.h:116); DumpEntry's own handling of a genuine
        // zero-length put is undefined behaviour (and, for String,
        // crashes) -- see the "DumpEntry reads an Entry1X as an Entry0X
        // for a zero-length vector" entry in
        // docs/issues/c-library-quirks.md. `KeyMap::put_vector` degrades
        // the value to a scalar before it is ever stored, so `to_node`
        // never sees a zero-length vector and cannot panic on one.
        let mut km = KeyMap::new();
        km.put_1i("I", &[], None).unwrap();
        km.put_1d("D", &[], None).unwrap();
        km.put_1c("C", &[], None).unwrap();

        assert_eq!(km.map_type("I").unwrap(), AstType::Int);
        assert_eq!(km.map_length("I"), 1);
        assert_eq!(km.get_0i("I").unwrap(), Some(0));
        assert_eq!(km.get_0d("D").unwrap(), Some(0.0));
        assert_eq!(km.get_0c("C").unwrap().as_deref(), Some(""));

        // Dumps as a scalar Val%d card, exactly as C's own degraded
        // zero-length put does for every numeric type (confirmed against
        // the reference library): no Nel%d card, and a Val%d rather than
        // a V%d_1.
        let node = km.to_node().unwrap();
        let n = node
            .items
            .iter()
            .find(|item| item.key.starts_with("Key") && node.get_string(&item.key, "") == "I")
            .map(|item| item.key.trim_start_matches("Key").to_string())
            .expect("Key%d card for \"I\" not found");
        assert!(node.get_raw(&format!("Nel{n}")).is_none());
        assert_eq!(node.get_int(&format!("Val{n}"), -1), 0);

        let back = KeyMap::from_node(&node).unwrap();
        assert_eq!(back.get_0i("I").unwrap(), Some(0));
    }

    #[test]
    fn a_non_ascii_key_does_not_panic_the_presize_filter() {
        // The presize filter in `from_node` used to byte-slice a node
        // key by a fixed offset (`&key[..3]`), which panics on a
        // non-ASCII key whose byte 3 falls inside a multi-byte
        // character rather than on a UTF-8 boundary. The native
        // reader's key-boundary predicate (`char::is_alphanumeric`) is
        // Unicode-aware, so such a key is reachable from a real dump,
        // not just a hand-built Node.
        let mut node = Node::new("KeyMap");
        node.push_string("Kaéx1", "v", None);

        // Must not panic; the malformed key is simply not recognised as
        // a Key%d card; the fixture itself is not a legal KeyMap dump
        // (no Key1/Typ1/Val1), so from_node still errors, just not by
        // panicking.
        let _ = KeyMap::from_node(&node);
    }

    #[test]
    fn round_trip_preserves_every_type() {
        let mut inner = KeyMap::new();
        inner.put_0c("NESTED", "yes", None).unwrap();

        let mut km = KeyMap::new();
        // KeyCase must be set before any entry is added: C's SetKeyCase
        // (keymap.c:9234) reports AST__NOWRT if it would change the value
        // on a non-empty KeyMap.
        km.set_key_case(false).unwrap();
        km.put_0i("I", -7, Some("an int")).unwrap();
        km.put_0s("S", -3, None).unwrap();
        km.put_0k("K", i64::MIN, None).unwrap();
        km.put_0b("B", 200, None).unwrap();
        km.put_0d("D", 1.234_567_890_123_456_7e-5, None).unwrap();
        km.put_0f("F", 1.5, None).unwrap();
        km.put_0c("C", "text", None).unwrap();
        km.put_0a("A", AnyObject::KeyMap(Box::new(inner)), None)
            .unwrap();
        km.put_u("U", Some("undefined")).unwrap();
        km.put_1i("VI", &[1, 2, 3], None).unwrap();
        // MapLocked is exercised separately, in
        // `reloading_a_locked_non_empty_keymap_fails_like_c`: setting it
        // here would make every subsequent load fail (see that test).

        let node = km.to_node().unwrap();
        let back = KeyMap::from_node(&node).unwrap();

        assert_eq!(back.map_size(), km.map_size());
        for key in ["I", "S", "K", "B", "D", "F", "C", "A", "U", "VI"] {
            assert_eq!(
                back.map_type(key).unwrap(),
                km.map_type(key).unwrap(),
                "type of {key}"
            );
        }
        assert_eq!(back.get_0i("I").unwrap(), Some(-7));
        assert_eq!(back.map_comment("I").as_deref(), Some("an int"));
        assert_eq!(back.get_0k("K").unwrap(), Some(i64::MIN));
        assert_eq!(back.get_0b("B").unwrap(), Some(200));
        assert_eq!(back.get_0d("D").unwrap(), km.get_0d("D").unwrap());
        assert_eq!(back.get_0f("F").unwrap(), Some(1.5));
        assert_eq!(back.get_1i("VI").unwrap(), Some(vec![1, 2, 3]));
        assert!(!back.map_defined("U").unwrap());
        assert!(back.map_has_key("U").unwrap());
        assert_eq!(back.map_comment("U").as_deref(), Some("undefined"));
        assert!(!back.key_case());

        let Some(AnyObject::KeyMap(nested)) = back.get_0a("A").unwrap().unwrap() else {
            panic!("expected a nested KeyMap");
        };
        assert_eq!(nested.get_0c("NESTED").unwrap().as_deref(), Some("yes"));
    }

    #[test]
    fn reloading_a_locked_non_empty_keymap_fails_like_c() {
        // astLoadKeyMap applies MpLck (keymap.c:11011) before loading any
        // entry, so a dump of a locked, non-empty KeyMap can never be
        // reloaded through the public put path — confirmed against the
        // reference library itself, not just the source: astRead on such a
        // dump also fails with AST__BADKEY in C.
        let mut km = KeyMap::new();
        km.put_0i("A", 1, None).unwrap();
        km.set_map_locked(true);

        let node = km.to_node().unwrap();
        let err = KeyMap::from_node(&node).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Badkey);
        // The message names the entry-loading put (astMapPut0I, from
        // load_entry's AstType::Int branch), not astMapPut0I via any
        // other path, and quotes the key exactly as insert_entry's own
        // AST__BADKEY message does (keymap.c:4370-ish).
        assert_eq!(
            err.message(),
            "astMapPut0I(KeyMap): Failed to add item \"A\" to a KeyMap: \
             \"A\" is not a known item."
        );
    }

    #[test]
    fn the_dump_is_idempotent_and_insertion_order_independent() {
        // The two properties Task 15 tests exhaustively; a smoke check here
        // so serial.rs fails first if the ordering regresses.
        let build = |order: &[&str]| -> KeyMap {
            let mut km = KeyMap::new();
            for (i, key) in order.iter().enumerate() {
                km.put_0i(key, i as i32, None).unwrap();
            }
            km
        };
        let a = build(&["Size", "Error", "Name", "Resolution"]);
        let b = build(&["Resolution", "Name", "Error", "Size"]);

        let keys = |km: &KeyMap| -> Vec<String> {
            km.to_node()
                .unwrap()
                .items
                .iter()
                .filter(|i| i.key.starts_with("Key"))
                .map(|i| match &i.value {
                    NodeValue::String(s) => s.clone(),
                    other => panic!("expected a string key, got {other:?}"),
                })
                .collect()
        };
        // KeyCase defaults to case-sensitive, so the stored keys keep their
        // original case; key_cmp orders them case-insensitively.
        assert_eq!(keys(&a), vec!["Error", "Name", "Resolution", "Size"]);
        assert_eq!(keys(&a), keys(&b));

        // Reloading and re-dumping gives the same cards.
        let once = render_native(&a.to_node().unwrap()).unwrap();
        let twice = render_native(
            &KeyMap::from_node(&a.to_node().unwrap())
                .unwrap()
                .to_node()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn an_unknown_type_code_is_an_error() {
        let mut node = Node::new("KeyMap");
        node.push_int("MapSz", 16, None);
        node.push_int("MemCnt", 2, None);
        node.push_string("Key1", "A", None);
        node.push_int("Typ1", 6, None);
        node.push_string("Val1", "0x1234", None);

        let err = KeyMap::from_node(&node).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Bdfts);
        assert!(
            err.to_string()
                .contains("Unknown data type code (6) encountered whilst reading a KeyMap."),
            "{err}"
        );
    }

    #[test]
    fn a_missing_type_code_is_a_different_error() {
        let mut node = Node::new("KeyMap");
        node.push_int("MapSz", 16, None);
        node.push_int("MemCnt", 2, None);
        node.push_string("Key1", "A", None);

        let err = KeyMap::from_node(&node).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Bdfts);
        assert!(
            err.to_string()
                .contains("No data type code found whilst reading a KeyMap."),
            "{err}"
        );
    }

    #[test]
    fn the_entry_loop_stops_at_the_first_gap() {
        let mut node = Node::new("KeyMap");
        node.push_int("MapSz", 16, None);
        node.push_int("MemCnt", 6, None);
        node.push_string("Key1", "A", None);
        node.push_int("Typ1", 1, None);
        node.push_int("Val1", 1, None);
        // Key2 is missing, so Key3 is never reached.
        node.push_string("Key3", "C", None);
        node.push_int("Typ3", 1, None);
        node.push_int("Val3", 3, None);

        let km = KeyMap::from_node(&node).unwrap();
        assert_eq!(km.map_size(), 1);
        assert!(km.map_has_key("A").unwrap());
    }

    #[test]
    fn map_sz_is_written_from_the_contents() {
        // Task 3's map_sz, driven by the dump-order key sequence.
        let mut km = KeyMap::new();
        for i in 0..4 {
            km.put_0i(&format!("K{i}"), i, None).unwrap();
        }
        let node = km.to_node().unwrap();
        assert_eq!(node.get_int("MapSz", -1), km.map_sz_for_dump());
        assert_eq!(node.get_int("MapSz", -1), 16);

        // MapSz on the way in is discarded: it is C's bucket history and has
        // no HashMap counterpart.
        let mut node = node;
        node.replace_or_push("MapSz", NodeValue::Int(4096));
        let back = KeyMap::from_node(&node).unwrap();
        assert_eq!(back.to_node().unwrap().get_int("MapSz", -1), 16);
    }

    #[test]
    fn mem_cnt_round_trips() {
        let mut km = KeyMap::new();
        for i in 0..4 {
            km.put_0i(&format!("K{i}"), i, None).unwrap();
        }
        // Four fresh keys give eight, because each new key advances the
        // counter twice. cap_timemap_02.map records MemCnt = 8 for its four.
        let node = km.to_node().unwrap();
        assert_eq!(node.get_int("MemCnt", -1), 8);
        let back = KeyMap::from_node(&node).unwrap();
        assert_eq!(back.to_node().unwrap().get_int("MemCnt", -1), 8);
    }
}
