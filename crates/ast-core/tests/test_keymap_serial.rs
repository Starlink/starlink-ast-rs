//! KeyMap serialisation: hand-built round trips, the one real C-generated
//! KeyMap in the fixture corpus, and the `MapSz` C oracle.
//!
//! `MapSz` is excluded from the byte-exact comparison and asserted
//! separately against the C probe's committed output; see
//! `common::is_map_sz_card` and `tests/fixtures/oracle/keymap_mapsz.txt`.

use crate::common;
use crate::common::native_dump;

use ast_core::keymap::{KeyMap, SortBy};
use ast_core::{AnyObject, from_native_string};
use std::collections::BTreeMap;

/// One card of a Native dump, with indentation removed.
///
/// `commented` records whether the card was written as an unset attribute
/// (`#   Title = …`), which the writer uses to mean "this is the default".
#[derive(Debug, PartialEq, Eq, Clone)]
struct Card {
    commented: bool,
    content: String,
}

fn normalise(line: &str) -> Card {
    let s = line.trim_start();
    if let Some(rest) = s.strip_prefix('#') {
        Card {
            commented: true,
            content: rest.trim_start().to_string(),
        }
    } else {
        Card {
            commented: false,
            content: s.to_string(),
        }
    }
}

/// The column at which a card's content starts, treating a commented-out
/// card's `#` as occupying a column of its own.
///
/// `native_write.rs`'s `write_object_with_options` (`native_write.rs:205-214`)
/// gives a set card `base_indent + indent + 1` leading spaces, and an unset
/// card `"#"` followed by `base_indent + indent` spaces. Counting the `#`
/// itself as one column makes both forms land on the same total: at the
/// default `indent = 3` a top-level (`base_indent = 0`) set card starts its
/// content at column 4, and so does `"#" + 3 spaces"`. This is confirmed
/// against the real C fixture below, not just against the writer's own
/// arithmetic: in `cap_timemap_02.map`, line 473 (`Ident = …`, set) and line
/// 475 (`#   Title = …`, commented) sit at the same nesting level inside the
/// `CmpFrame`/`Object` block, and both have their content starting at raw
/// character index 22 — the `#` on line 475 occupies index 0, exactly like
/// the first space of line 473's indent.
fn column(line: &str) -> usize {
    if let Some(rest) = line.strip_prefix('#') {
        1 + (rest.len() - rest.trim_start().len())
    } else {
        line.len() - line.trim_start().len()
    }
}

#[test]
fn column_agrees_for_a_set_and_an_unset_card_at_the_same_nesting_level() {
    // Lines 473 and 475 of the fixture, reproduced verbatim: a set `Ident`
    // card and a commented `Title` card one level inside a nested CmpFrame,
    // both indented for base_indent = 19 (indent = 3, so content at 19+3=22).
    let set_line = "                      Ident = \"ICRS-TT-CXO\" \t# comment";
    let unset_line = "#                     Title = \"x\" \t# comment";
    assert_eq!(column(set_line), 22);
    assert_eq!(column(unset_line), 22);
    assert_eq!(column(set_line), column(unset_line));
}

/// Replace the trailing digits of a card name with `#`, so `Key3` and `Key1`
/// compare equal and `V3_2` compares equal to `V1_2`.
///
/// Entry cards are `Key<n>`, `Com<n>`, `Typ<n>`, `Nel<n>`, `Val<n>` and
/// `V<n>_<i>`; only the entry index `<n>` is renumbered, never the element
/// index `<i>`.
fn renumber(content: &str) -> String {
    let Some((name, rest)) = content.split_once(" =") else {
        return content.to_string();
    };
    let renamed = if let Some((head, tail)) = name.split_once('_') {
        // V<n>_<i>: renumber the entry index, keep the element index.
        format!(
            "{}#_{tail}",
            head.trim_end_matches(|c: char| c.is_ascii_digit())
        )
    } else {
        let stem = name.trim_end_matches(|c: char| c.is_ascii_digit());
        if stem.len() == name.len() {
            name.to_string()
        } else {
            format!("{stem}#")
        }
    };
    format!("{renamed} ={rest}")
}

/// A KeyMap dump split into its attribute cards and its per-entry blocks.
#[derive(Debug, PartialEq, Eq)]
struct Split {
    /// The cards before the first `Key<n>`, in order.
    header: Vec<Card>,
    /// Entry blocks keyed by the entry's key name, each renumbered.
    ///
    /// A `BTreeMap` rather than insertion order: C dumps entries in
    /// hash-bucket order and the port dumps in `KeyCmp` order
    /// (`serial.rs`'s `dump_keys`), so the two sides never agree on entry
    /// order and the comparison must be order-blind. Keying by name (rather
    /// than, say, sorting both sides' block lists) also gives a precise
    /// "entry X differs" failure message instead of a positional diff.
    entries: BTreeMap<String, Vec<Card>>,
}

/// Split the `Begin KeyMap` … `End KeyMap` block that `lines` consists of.
///
/// Only cards at the block's own attribute column start a new entry; a
/// `Key<n>` card belonging to a nested KeyMap stays inside the enclosing
/// entry's block, where it belongs.
fn split_keymap(text: &str) -> Split {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let begin = lines
        .iter()
        .position(|l| l.trim_start().starts_with("Begin KeyMap"))
        .expect("no Begin KeyMap");
    let base = column(lines[begin]);
    let own = base + 3;

    let mut header = Vec::new();
    let mut entries: BTreeMap<String, Vec<Card>> = BTreeMap::new();
    let mut current: Option<String> = None;

    for line in &lines[begin + 1..] {
        if line.trim_start().starts_with("End KeyMap") && column(line) == base {
            break;
        }
        let card = normalise(line);
        let starts_entry = column(line) == own
            && !card.commented
            && card.content.starts_with("Key")
            && card
                .content
                .split_once(" =")
                .is_some_and(|(n, _)| n[3..].chars().all(|c| c.is_ascii_digit()) && n.len() > 3);

        if starts_entry {
            let key = card
                .content
                .split_once(" =")
                .unwrap()
                .1
                .split('\t')
                .next()
                .unwrap()
                .trim()
                .trim_matches('"')
                .to_string();
            current = Some(key.clone());
            entries.entry(key).or_default();
        }

        let target = match &current {
            Some(k) => entries.get_mut(k).unwrap(),
            None => &mut header,
        };
        target.push(Card {
            commented: card.commented,
            content: renumber(&card.content),
        });
    }

    Split { header, entries }
}

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/simplify/cap_timemap_02.map"
);

/// Lift the one `Begin KeyMap` … `End KeyMap` block out of a Native dump.
///
/// The block is a self-contained document, so the KeyMap is read on its own,
/// apart from the `StcResourceProfile` that holds it.
fn extract_keymap_block(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let begin = lines
        .iter()
        .position(|l| l.trim_start().starts_with("Begin KeyMap"))
        .expect("fixture has no KeyMap");
    let base = column(lines[begin]);
    let end = lines[begin..]
        .iter()
        .position(|l| l.trim_start().starts_with("End KeyMap") && column(l) == base)
        .expect("unterminated KeyMap")
        + begin;
    lines[begin..=end].join("\n")
}

#[test]
fn the_c_fixture_keymap_round_trips() {
    let text = std::fs::read_to_string(FIXTURE).unwrap();
    let block = extract_keymap_block(&text);

    let AnyObject::KeyMap(km) = from_native_string(&block).unwrap() else {
        panic!("the block did not load as a KeyMap");
    };

    // Four keys, all present, with the types the fixture records.
    assert_eq!(km.map_size(), 4);
    for key in ["Size", "Name", "Resolution", "Error"] {
        assert!(km.map_has_key(key).unwrap(), "missing {key}");
    }
    assert_eq!(km.map_length("Name").unwrap(), 4);

    let out = native_dump(&AnyObject::KeyMap(km)).unwrap();

    let want = split_keymap(&block);
    let got = split_keymap(&out);

    // MapSz is excluded from the card comparison and asserted separately.
    let strip = |cards: Vec<Card>| -> Vec<Card> {
        cards
            .into_iter()
            .filter(|c| !common::is_map_sz_card(&c.content))
            .collect()
    };
    assert_eq!(strip(got.header), strip(want.header), "header cards differ");
    assert_eq!(
        got.entries.keys().collect::<Vec<_>>(),
        want.entries.keys().collect::<Vec<_>>(),
        "entry key sets differ"
    );
    for (key, want_cards) in want.entries {
        let got_cards = got.entries.get(&key).unwrap().clone();
        assert_eq!(got_cards, want_cards, "entry {key} differs");
    }
}

/// The `MapSz` value in a Native document, as an integer.
///
/// `MapSz` is excluded from the card-by-card comparison, so it is read out
/// explicitly wherever it has to be checked.
fn map_sz_card_of(text: &str) -> i64 {
    text.lines()
        .find(|l| common::is_map_sz_card(l))
        .unwrap_or_else(|| panic!("no MapSz card in:\n{text}"))
        .split_once('=')
        .unwrap()
        .1
        .split('\t')
        .next()
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

/// The `MapSz` a freshly dumped `KeyMap` carries.
fn dumped_map_sz(km: &KeyMap) -> i64 {
    let text = native_dump(&AnyObject::KeyMap(Box::new(km.clone()))).unwrap();
    map_sz_card_of(&text)
}

#[test]
fn the_c_fixture_map_sz_matches() {
    // C wrote MapSz = 16 for these four keys. Located by scanning for the
    // `Begin KeyMap` marker, not by a hardcoded line number: the block sits
    // wherever the enclosing StcResourceProfile happens to place it.
    let text = std::fs::read_to_string(FIXTURE).unwrap();
    let block = extract_keymap_block(&text);
    assert_eq!(map_sz_card_of(&block), 16);

    let AnyObject::KeyMap(km) = from_native_string(&block).unwrap() else {
        panic!("the block did not load as a KeyMap");
    };
    assert_eq!(dumped_map_sz(&km), 16);
}

/// One row of the C oracle probe's output.
struct OracleRow {
    label: String,
    sizeguess: i32,
    nkey: usize,
    map_sz: i64,
}

/// Parse `mapsz_oracle.txt`. The format is the probe's `printf`:
/// `<label> sizeguess=<n> nkey=<n> MapSz=<n>`, label left-padded to 28.
fn oracle_rows() -> Vec<OracleRow> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/oracle/keymap_mapsz.txt"
    );
    let text = std::fs::read_to_string(path).unwrap();
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let field = |name: &str| -> i64 {
            let at = line
                .find(name)
                .unwrap_or_else(|| panic!("no {name} in oracle row {line:?}"));
            line[at + name.len()..]
                .split_whitespace()
                .next()
                .unwrap()
                .parse()
                .unwrap()
        };
        let label_end = line.find("sizeguess=").unwrap();
        rows.push(OracleRow {
            label: line[..label_end].trim().to_string(),
            sizeguess: field("sizeguess=") as i32,
            nkey: field("nkey=") as usize,
            map_sz: field("MapSz="),
        });
    }
    assert!(!rows.is_empty(), "oracle fixture is empty");
    rows
}

/// The first `n` keys of the form `K<i>` that hash to bucket 5 of a
/// 16-bucket table, by the same rule the C probe and the production
/// `keymap::table` module use.
///
/// This is a *third* independent restatement of djb2 (the first is
/// production `hash_fun` in `crates/ast-core/src/keymap/table.rs:41-50`; the
/// second is `bucket_of` in `tools/oracle_fixtures/keymap_hash.c:23-29`).
/// The C oracle's `bucket_of` widens each byte through `(unsigned char) *p`
/// — unsigned — while production `hash_fun` widens through `c as i8 as i64
/// as u64` — signed, matching C's `HashFun`, whose `key` pointer is `const
/// char *` and is dereferenced through a plain (signed) `int`. The two
/// widenings only agree for ASCII bytes (all below 0x80), which is all this
/// helper ever generates (`K0`, `K1`, …), so the divergence never surfaces
/// here — but it would for a key containing a byte at or above 0x80.
fn colliding_keys(n: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(n);
    let mut i = 0u64;
    while out.len() < n {
        let key = format!("K{i}");
        let mut hash: u64 = 5381;
        for b in key.bytes() {
            if b != b' ' {
                hash = hash
                    .wrapping_shl(5)
                    .wrapping_add(hash)
                    .wrapping_add(b as u64);
            }
        }
        if (hash & 15) == 5 {
            out.push(key);
        }
        i += 1;
    }
    out
}

/// Build the KeyMap a given oracle label describes.
///
/// Returns `None` for a label this test does not know how to reproduce, so
/// that adding a row to the probe cannot silently do nothing here — the
/// caller counts what it skipped and fails if the count grows.
fn key_map_for(row: &OracleRow) -> Option<KeyMap> {
    let mut km = KeyMap::new();
    if row.sizeguess > 0 {
        // The probe sets SizeGuess before its put loop (`keymap_hash.c:75`),
        // which matters: C's SetSizeGuess (`keymap.c:9463`) refuses to run
        // on a non-empty map, reporting AST__NOWRT at `keymap.c:9512-9514`,
        // so the order has to match here too.
        km.set_size_guess(row.sizeguess as usize).unwrap();
    }
    let keys: Vec<String> = match row.label.as_str() {
        "empty"
        | "four keys"
        | "100 keys"
        | "500 keys"
        | "1000 keys"
        | "guess 100"
        | "guess 160"
        | "guess 170"
        | "guess 5000"
        | "guess 5000, 500 keys" => (0..row.nkey).map(|i| format!("KEY_{i}")).collect(),
        "500 keys, k%d" => (0..row.nkey).map(|i| format!("k{i}")).collect(),
        "500 keys, ZZ%d_TAIL" => (0..row.nkey).map(|i| format!("ZZ{i}_TAIL")).collect(),
        "cap_timemap_02 keys" => ["Size", "Name", "Resolution", "Error"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        "10 colliding" | "11 colliding" | "40 colliding" | "300 colliding" => {
            colliding_keys(row.nkey)
        }
        "40 colliding + 40 spread" => {
            let mut v = colliding_keys(40);
            v.extend((0..40).map(|i| format!("SPREAD_{i}")));
            v
        }
        _ => return None,
    };
    for (i, key) in keys.iter().enumerate() {
        km.put_0i(key, i as i32, None).unwrap();
    }
    Some(km)
}

#[test]
fn map_sz_matches_c_oracle() {
    // Every label the probe emits today. If the probe gains a row, this
    // number goes up and key_map_for gains an arm; it must never be raised
    // to paper over a label key_map_for does not handle.
    const EXPECTED_ROWS: usize = 18;

    let rows = oracle_rows();
    assert_eq!(
        rows.len(),
        EXPECTED_ROWS,
        "the oracle fixture has {} rows, not {EXPECTED_ROWS}; add the new \
         label to key_map_for and update this constant",
        rows.len()
    );

    for row in &rows {
        let km = key_map_for(row).unwrap_or_else(|| {
            panic!(
                "oracle row {:?} has no arm in key_map_for; add one rather \
                 than skipping the row",
                row.label
            )
        });
        assert_eq!(
            dumped_map_sz(&km),
            row.map_sz,
            "MapSz for {:?} (sizeguess={}, nkey={}): C says {}",
            row.label,
            row.sizeguess,
            row.nkey,
            row.map_sz
        );
    }
}

/// Serialise, load, serialise again, and require the two texts to be equal.
///
/// A dump that is not a fixed point means `to_node` and `from_node` disagree
/// about some default, comment, or ordering.
fn assert_double_round_trip(km: &KeyMap, name: &str) -> KeyMap {
    let text1 = native_dump(&AnyObject::KeyMap(Box::new(km.clone()))).unwrap();
    let AnyObject::KeyMap(loaded) =
        from_native_string(&text1).unwrap_or_else(|e| panic!("{name}: load failed: {e}\n{text1}"))
    else {
        panic!("{name}: did not load as a KeyMap");
    };
    let text2 = native_dump(&AnyObject::KeyMap(loaded.clone())).unwrap();
    assert_eq!(text1, text2, "{name}: dump is not a fixed point");
    *loaded
}

#[test]
fn every_scalar_type_round_trips() {
    let mut km = KeyMap::new();
    km.put_0i("an_int", -7, Some("int comment")).unwrap();
    km.put_0d("a_double", 1.5, None).unwrap();
    km.put_0f("a_float", 0.25, None).unwrap();
    km.put_0c("a_string", "text with spaces", None).unwrap();
    km.put_0s("a_short", -3, None).unwrap();
    km.put_0b("a_byte", 200, None).unwrap();
    km.put_0k("an_int64", 9_000_000_000, None).unwrap();
    km.put_u("undefined", None).unwrap();
    km.put_0a(
        "an_object",
        AnyObject::UnitMap(ast_core::mappings::UnitMap::new(2)),
        None,
    )
    .unwrap();

    let back = assert_double_round_trip(&km, "scalars");

    assert_eq!(back.map_size(), km.map_size());
    assert_eq!(back.get_0i("an_int").unwrap(), Some(-7));
    assert_eq!(back.get_0d("a_double").unwrap(), Some(1.5));
    assert_eq!(back.get_0f("a_float").unwrap(), Some(0.25));
    assert_eq!(
        back.get_0c("a_string").unwrap().as_deref(),
        Some("text with spaces")
    );
    assert_eq!(back.get_0s("a_short").unwrap(), Some(-3));
    assert_eq!(back.get_0b("a_byte").unwrap(), Some(200));
    assert_eq!(back.get_0k("an_int64").unwrap(), Some(9_000_000_000));
    assert!(back.map_has_key("undefined").unwrap());
    assert!(!back.map_defined("undefined").unwrap());
    assert!(back.get_0a("an_object").unwrap().is_some());
}

#[test]
fn every_vector_type_round_trips() {
    let mut km = KeyMap::new();
    km.put_1i("ints", &[1, 2, 3], None).unwrap();
    km.put_1d("doubles", &[1.5, 2.5], Some("two doubles"))
        .unwrap();
    km.put_1f("floats", &[0.5, 0.25, 0.125], None).unwrap();
    km.put_1c("strings", &["a", "", "c c"], None).unwrap();
    km.put_1s("shorts", &[-1, 0, 1], None).unwrap();
    km.put_1b("bytes", &[0, 255], None).unwrap();
    km.put_1k("int64s", &[-9_000_000_000, 9_000_000_000], None)
        .unwrap();

    let back = assert_double_round_trip(&km, "vectors");

    assert_eq!(
        back.get_1i("ints").unwrap().as_deref(),
        Some(&[1, 2, 3][..])
    );
    assert_eq!(
        back.get_1c("strings").unwrap().unwrap(),
        vec!["a".to_string(), String::new(), "c c".to_string()]
    );
    assert_eq!(back.map_length("floats").unwrap(), 3);
    assert_eq!(back.map_length("bytes").unwrap(), 2);
}

#[test]
fn a_bad_double_round_trips_as_bad() {
    // DumpEntry writes no card at all for an AST__BAD double (keymap.c
    // rotates the comment inside the `if`), and LoadKeyMap's default for a
    // missing Val card is AST__BAD, so the value survives by round-tripping
    // through absence.
    let mut km = KeyMap::new();
    km.put_0d("bad", ast_core::constants::BAD_VALUE, None)
        .unwrap();
    km.put_1d(
        "part_bad",
        &[1.0, ast_core::constants::BAD_VALUE, 3.0],
        None,
    )
    .unwrap();

    let text = native_dump(&AnyObject::KeyMap(Box::new(km.clone()))).unwrap();
    // "bad" sorts before "part_bad" under key_cmp ('b' 0x62 < 'p' 0x70), so
    // the bad scalar is dumped as entry 1 and this checks Val1 specifically,
    // not "some Val card is missing".
    assert!(!text.contains("Val1 ="), "a BAD scalar writes no Val card");

    let back = assert_double_round_trip(&km, "bad double");
    assert_eq!(
        back.get_0d("bad").unwrap(),
        Some(ast_core::constants::BAD_VALUE)
    );
    assert_eq!(
        back.get_1d("part_bad").unwrap().as_deref(),
        Some(&[1.0, ast_core::constants::BAD_VALUE, 3.0][..])
    );
}

#[test]
fn a_null_object_entry_round_trips() {
    // MapPut1A stores NULL rather than rejecting it
    // (`reference/starlink-ast/src/keymap.c:5012`:
    // `MAKE_MAPPUT1(A,...,(value[i]?astClone(value[i]):NULL))`), and
    // DumpEntry writes no card for a null element.
    let mut km = KeyMap::new();
    km.put_1a(
        "objects",
        vec![
            Some(AnyObject::UnitMap(ast_core::mappings::UnitMap::new(1))),
            None,
        ],
        None,
    )
    .unwrap();

    let back = assert_double_round_trip(&km, "null object element");
    let got = back.get_1a("objects").unwrap().unwrap();
    assert_eq!(got.len(), 2);
    assert!(got[0].is_some());
    assert!(got[1].is_none());
}

#[test]
fn a_non_first_object_vector_element_has_no_trailing_space() {
    // `DumpEntry` (`keymap.c:2796-2801`) gives every element after the
    // first an empty comment, and `WriteObject` (`channel.c:5106-5112`)
    // appends `" ="` with no trailing space, only adding `" \t# "` when
    // `astGetComment(this) && *comment` is true. With an empty comment
    // that condition is false, so the card for a non-first element of an
    // Object vector must end exactly at `=`, never `= ` — this is what
    // `native_write.rs`'s Object arm got wrong before it routed through
    // the same non-empty-comment guard as every other card kind.
    let mut km = KeyMap::new();
    km.put_1a(
        "objects",
        vec![
            Some(AnyObject::UnitMap(ast_core::mappings::UnitMap::new(1))),
            Some(AnyObject::UnitMap(ast_core::mappings::UnitMap::new(2))),
        ],
        None,
    )
    .unwrap();

    let text = native_dump(&AnyObject::KeyMap(Box::new(km.clone()))).unwrap();
    // `normalise()` only trims leading whitespace, so check the raw line.
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with("V1_2 ="))
        .unwrap_or_else(|| panic!("no V1_2 card found in:\n{text}"));
    assert!(
        line.ends_with('='),
        "V1_2 card must end at '=' with no trailing space or comment, got {line:?}"
    );

    let back = assert_double_round_trip(&km, "object vector, no trailing space");
    let got = back.get_1a("objects").unwrap().unwrap();
    assert_eq!(got.len(), 2);
    assert!(got[0].is_some());
    assert!(got[1].is_some());
}

#[test]
fn every_attribute_round_trips() {
    // MapLocked is exercised on an empty map: astLoadKeyMap applies MpLck
    // (`reference/starlink-ast/src/keymap.c:11011`) before loading any
    // entry, so reloading a dump of a locked *non-empty* KeyMap always
    // fails with AST__BADKEY — confirmed by the unit test
    // `reloading_a_locked_non_empty_keymap_fails_like_c`
    // (`crates/ast-core/src/keymap/serial.rs`). Locking after the entries
    // are in, as the C ancestor's test order might suggest, would make this
    // round trip fail by construction, not by a bug.
    let mut km = KeyMap::new();
    km.set_size_guess(320).unwrap();
    km.set_key_case(false).unwrap();
    km.set_key_error(true);
    km.set_sort_by(SortBy::KeyDown);
    km.set_map_locked(true);

    let back = assert_double_round_trip(&km, "attributes");

    assert_eq!(back.size_guess(), 320);
    assert!(!back.key_case());
    assert!(back.key_error());
    assert_eq!(back.sort_by(), SortBy::KeyDown);
    assert!(back.map_locked());
}

#[test]
fn a_nested_key_map_round_trips() {
    let mut inner = KeyMap::new();
    inner.put_0i("b3", 3, None).unwrap();
    inner.put_1c("b4", &["x", "y"], None).unwrap();

    let mut km = KeyMap::new();
    km.put_0i("a1", 1, None).unwrap();
    km.put_0a(
        "nested",
        AnyObject::KeyMap(Box::new(inner)),
        Some("a KeyMap"),
    )
    .unwrap();

    let back = assert_double_round_trip(&km, "nested");

    let Some(Some(AnyObject::KeyMap(got))) = back.get_0a("nested").unwrap() else {
        panic!("nested entry did not load as a KeyMap");
    };
    assert_eq!(got.get_0i("b3").unwrap(), Some(3));
    assert_eq!(got.map_length("b4").unwrap(), 2);
}

#[test]
fn a_dump_is_independent_of_insertion_order() {
    // The same property the crate's property tests check over generated
    // inputs; this is the fixed case, so a regression names itself. Each
    // key's value is fixed to its position in `keys`, not to the loop index,
    // so every rotation inserts the same (key, value) pairs and only the
    // insertion *order* changes.
    let keys = ["Size", "Name", "Resolution", "Error"];
    let mut first: Option<String> = None;
    for rotate in 0..keys.len() {
        let mut km = KeyMap::new();
        for i in 0..keys.len() {
            let idx = (i + rotate) % keys.len();
            km.put_0i(keys[idx], idx as i32, None).unwrap();
        }
        let text = native_dump(&AnyObject::KeyMap(Box::new(km))).unwrap();
        // MemCnt counts puts, not keys, so it is order-independent here only
        // because every rotation does the same number of fresh puts.
        match &first {
            None => first = Some(text),
            Some(f) => assert_eq!(&text, f, "rotation {rotate} dumped differently"),
        }
    }
}
