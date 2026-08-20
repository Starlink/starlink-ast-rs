//! Rust port of `reference/starlink-ast/ast_tester/testkeymap.c`.
//!
//! The C original drives the KeyMap through the protected `astMapPut*` /
//! `astMapGet*` API and checks each value with `astMapGet0C`. This port
//! uses the public Rust API only, so it doubles as a worked example.
//!
//! Divergences from the C original are noted at each site. The
//! `astMapPutP` / `astMapGetP` family has no Rust counterpart, because
//! `AST__POINTERTYPE` holds a raw C pointer. The rest of the whole-file
//! divergences:
//!
//! - `astMapGet1C` and `astMapGetElemC` take a buffer width and pad each
//!   string out to it in a flat buffer. `get_1c` returns `Vec<String>` and
//!   `get_elem_c` returns `String`, both unpadded, so the C checks on the
//!   padding become checks on the values. The fixed stride inside
//!   `astMapGetC`'s *returned* string is a different thing and is
//!   preserved: `map_get_c` still yields `"(Hello, ,  Hello)"`.
//! - `astCopy` becomes `Clone`, the documented deep copy for this port.
//! - A NULL element of an Object vector is `Option::None`; `astMapPut1A`
//!   stores NULL rather than rejecting it (`keymap.c:5012`).
//! - `astMapPutElem*`'s negative index is not a distinct sentinel: C's
//!   `MAKE_MAPPUTELEM` (`keymap.c:8392`) computes
//!   `new = ( elem >= nel || elem < 0 )`, so *any* out-of-range index
//!   appends and `-1` is one spelling of it. `put_elem_*` takes a `usize`,
//!   so the C call sites that pass `-1` pass an index at or past the
//!   current length here.
//! - `astGetC( obj, "class" )` becomes a comparison of `AnyObject`
//!   variants, which is strictly stronger than comparing class-name
//!   strings.
//! - `put_0a` takes its value by value, so no caller-side alias survives
//!   the insert. C's `MapLocked` / `KeyError` inheritance blocks keep
//!   mutating and reading a nested KeyMap through the caller's own
//!   reference-counted handle, which is unrepresentable here; those blocks
//!   read the nested map back out as a `get_0a` clone instead.
//! - The C `stopit()` labels ("Error A6") are kept as comments rather than
//!   as message strings: the test name and the assertion line locate a
//!   Rust failure.

use ast_core::frames::{SkyFrame, SkySystem, SpecFrame, SpecSystem};
use ast_core::keymap::{AstType, KeyMap, SortBy};
use ast_core::{AnyObject, error::ErrorCode, from_native_string, to_native_string};

#[test]
fn scalar_put_and_get_round_trip_every_type() {
    let mut km = KeyMap::new();
    km.put_0i("INT", -42, None).unwrap();
    km.put_0s("SINT", -7, None).unwrap();
    km.put_0k("KINT", -10 * i64::from(i32::MAX), None).unwrap();
    km.put_0b("BYTE", 200, None).unwrap();
    km.put_0f("FLOAT", 1.5, None).unwrap();
    km.put_0d("DOUBLE", -0.25, None).unwrap();
    km.put_0c("STRING", "Hello", None).unwrap();

    assert_eq!(km.map_size(), 7);
    assert_eq!(km.get_0i("INT").unwrap(), Some(-42));
    assert_eq!(km.get_0s("SINT").unwrap(), Some(-7));
    assert_eq!(km.get_0k("KINT").unwrap(), Some(-10 * i64::from(i32::MAX)));
    assert_eq!(km.get_0b("BYTE").unwrap(), Some(200));
    assert_eq!(km.get_0f("FLOAT").unwrap(), Some(1.5));
    assert_eq!(km.get_0d("DOUBLE").unwrap(), Some(-0.25));
    assert_eq!(km.get_0c("STRING").unwrap().as_deref(), Some("Hello"));
}

#[test]
fn get_converts_between_types() {
    let mut km = KeyMap::new();
    km.put_0d("D", 2.5, None).unwrap();
    // ConvertValue rounds by adding a half and truncating.
    assert_eq!(km.get_0i("D").unwrap(), Some(3));
    assert_eq!(km.get_0c("D").unwrap().as_deref(), Some("2.5"));

    km.put_0c("S", "42", None).unwrap();
    assert_eq!(km.get_0i("S").unwrap(), Some(42));
    assert_eq!(km.get_0d("S").unwrap(), Some(42.0));

    km.put_0c("JUNK", "hello", None).unwrap();
    let err = km.get_0i("JUNK").unwrap_err();
    assert_eq!(err.code(), ErrorCode::Mpger);
    assert!(
        err.to_string().contains(
            "The value of KeyMap key \"JUNK\" cannot be read using the requested data type."
        ),
        "{err}"
    );
}

#[test]
fn a_missing_key_is_not_an_error_unless_key_error_is_set() {
    let mut km = KeyMap::new();
    assert_eq!(km.get_0i("NOPE").unwrap(), None);

    km.set_key_error(true);
    let err = km.get_0i("NOPE").unwrap_err();
    assert_eq!(err.code(), ErrorCode::Mpker);
    assert!(
        err.to_string()
            .contains("No value was found for NOPE in the supplied KeyMap."),
        "{err}"
    );
}

#[test]
fn keys_ignore_trailing_spaces_and_honour_key_case() {
    let mut km = KeyMap::new();
    km.put_0i("FRED", 1, None).unwrap();
    // KeyCmp ignores trailing spaces, so the padded key is the same key.
    assert_eq!(km.get_0i("FRED   ").unwrap(), Some(1));
    assert_eq!(km.map_size(), 1);
    // Case matters by default.
    assert_eq!(km.get_0i("fred").unwrap(), None);

    // testkeymap.c testcasesens: with KeyCase=0 the key is stored
    // upper-cased, so a differently-cased lookup succeeds — but a leading
    // space still makes it a different key.
    let mut ci = KeyMap::new();
    ci.set_key_case(false).unwrap();
    ci.put_0i("FREDS", 1, None).unwrap();
    assert_eq!(ci.get_0i("freds").unwrap(), Some(1));
    assert_eq!(ci.get_0i("fReDs").unwrap(), Some(1));
    assert_eq!(ci.get_0i(" fReDs").unwrap(), None);
}

#[test]
fn re_putting_a_key_replaces_the_value_and_keeps_the_size() {
    // The brief's version of this test also asserts `map_type("A")`, but
    // `map_type` is a Task 9 interface, not produced here; that check
    // belongs with Task 9's tests instead of being stubbed early.
    let mut km = KeyMap::new();
    km.put_0i("A", 1, Some("first comment")).unwrap();
    km.put_0c("A", "second", None).unwrap();
    assert_eq!(km.map_size(), 1);
    assert_eq!(km.get_0c("A").unwrap().as_deref(), Some("second"));
}

#[test]
fn map_locked_blocks_new_keys_but_not_replacements() {
    let mut km = KeyMap::new();
    km.put_0i("A", 1, None).unwrap();
    km.set_map_locked(true);

    // Replacing an existing key is still allowed.
    km.put_0i("A", 2, None).unwrap();
    assert_eq!(km.get_0i("A").unwrap(), Some(2));

    let err = km.put_0i("B", 1, None).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Badkey);
    assert!(
        err.to_string().contains(
            "astMapPut0I(KeyMap): Failed to add item \"B\" to a KeyMap: \
             \"B\" is not a known item."
        ),
        "{err}"
    );
    assert_eq!(km.map_size(), 1);
}

#[test]
fn vector_put_and_get_round_trip() {
    let mut km = KeyMap::new();
    km.put_1i("VI", &[1, 2, 3], None).unwrap();
    km.put_1d("VD", &[1.5, -2.5], Some("two doubles")).unwrap();
    km.put_1c("VC", &["Position", "Position", "Time", "Energy"], None)
        .unwrap();

    assert_eq!(km.get_1i("VI").unwrap(), Some(vec![1, 2, 3]));
    assert_eq!(km.get_1d("VD").unwrap(), Some(vec![1.5, -2.5]));
    assert_eq!(
        km.get_1c("VC").unwrap(),
        Some(vec![
            "Position".to_string(),
            "Position".to_string(),
            "Time".to_string(),
            "Energy".to_string()
        ])
    );

    assert_eq!(km.map_length("VI"), 3);
    assert_eq!(km.map_length("VC"), 4);
    // A scalar has length 1 through astMapLength, even though its nel is 0.
    km.put_0i("S", 1, None).unwrap();
    assert_eq!(km.map_length("S"), 1);
    // A missing key has length 0.
    assert_eq!(km.map_length("NOPE"), 0);
}

#[test]
fn map_length_honours_key_case() {
    // astMapLength (keymap.c:8088) folds the key through ConvertKey before
    // the table lookup, the same as every other MapXxx entry point. With
    // KeyCase=0 a lookup in a different case from the key's own must
    // still find the entry, not report length 0.
    let mut km = KeyMap::new();
    km.set_key_case(false).unwrap();
    km.put_1i("FREDS", &[1, 2, 3], None).unwrap();
    assert_eq!(km.map_length("fReDs"), 3);
}

#[test]
fn get_1_converts_each_element() {
    let mut km = KeyMap::new();
    km.put_1c("V", &["1", "2.6", "3"], None).unwrap();
    assert_eq!(km.get_1i("V").unwrap(), Some(vec![1, 3, 3]));

    // A single unconvertible element fails the whole read, naming the
    // element index. C's message uses a 1-based element number.
    km.put_1c("BAD", &["1", "nope"], None).unwrap();
    let err = km.get_1i("BAD").unwrap_err();
    assert_eq!(err.code(), ErrorCode::Mpger);
    assert!(
        err.to_string().contains("element 2 of KeyMap key \"BAD\""),
        "{err}"
    );
}

#[test]
fn get_1_reads_a_scalar_as_a_length_one_vector() {
    let mut km = KeyMap::new();
    km.put_0i("S", 7, None).unwrap();
    assert_eq!(km.get_1i("S").unwrap(), Some(vec![7]));
}

#[test]
fn get_0_on_a_vector_returns_the_first_element() {
    // MAKE_MAPGET0 (keymap.c:5415 onward) takes the entry's storage base
    // pointer as `raw` regardless of whether the entry is a scalar or a
    // vector, so a scalar get on a vector entry legitimately reads the
    // vector's first element rather than failing or reading the whole
    // vector.
    let mut km = KeyMap::new();
    km.put_1i("V", &[10, 20, 30], None).unwrap();
    assert_eq!(km.get_0i("V").unwrap(), Some(10));

    km.put_1c("C", &["first", "second"], None).unwrap();
    assert_eq!(km.get_0c("C").unwrap().as_deref(), Some("first"));
}

#[test]
fn put_elem_creates_and_appends() {
    let mut km = KeyMap::new();

    // Absent key: creates a length-1 vector.
    km.put_elem_i("A", 0, 10).unwrap();
    assert_eq!(km.map_length("A"), 1);
    assert_eq!(km.get_1i("A").unwrap(), Some(vec![10]));

    // In-range index: overwrites.
    km.put_elem_i("A", 0, 11).unwrap();
    assert_eq!(km.get_1i("A").unwrap(), Some(vec![11]));

    // Out-of-range index: appends at the end, whatever the index was.
    km.put_elem_i("A", 99, 12).unwrap();
    assert_eq!(km.get_1i("A").unwrap(), Some(vec![11, 12]));

    // A scalar is promoted to a vector.
    km.put_0i("B", 1, None).unwrap();
    km.put_elem_i("B", 1, 2).unwrap();
    assert_eq!(km.get_1i("B").unwrap(), Some(vec![1, 2]));
}

#[test]
fn put_elem_comment_retention_differs_on_create_vs_promote() {
    // MAKE_MAPPUTELEM (keymap.c:8270-8287): the create-on-absent-key
    // branch calls astMapPut1<X> with a NULL comment, so an undefined
    // entry's comment is dropped once put_elem gives it its first value.
    // The promote-a-scalar branch instead calls astMapPut1<X> with
    // `mapentry->comment`, so a scalar's own comment survives becoming a
    // vector.
    let mut km = KeyMap::new();
    km.put_u("CREATED", Some("dropped on create")).unwrap();
    km.put_elem_i("CREATED", 0, 1).unwrap();
    assert_eq!(km.map_comment("CREATED"), None);

    km.put_0i("PROMOTED", 1, Some("kept on promote")).unwrap();
    km.put_elem_i("PROMOTED", 1, 2).unwrap();
    assert_eq!(
        km.map_comment("PROMOTED").as_deref(),
        Some("kept on promote")
    );
}

#[test]
fn put_elem_on_a_locked_map_reports_the_create_path_name() {
    // MAKE_MAPPUTELEM's create-on-absent-key branch (keymap.c:8273-8274)
    // literally calls astMapPut1<X>(this, key, 1, &value, NULL), and that
    // function's own MapLocked check (keymap.c:4972) hardcodes its own
    // name — astMapPut1I, not astMapPutElemI — into the AST__BADKEY
    // message. A locked map's diagnostic for a brand new key must name
    // the astMapPut1<X> call, whether reached via astMapPut1<X> directly
    // or via astMapPutElem<X>'s create path.
    let mut km = KeyMap::new();
    km.set_map_locked(true);

    let err = km.put_1i("NEW", &[1], None).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Badkey);
    assert!(
        err.to_string().contains(
            "astMapPut1I(KeyMap): Failed to add item \"NEW\" to a KeyMap: \
             \"NEW\" is not a known item."
        ),
        "{err}"
    );

    let err = km.put_elem_i("NEW", 0, 1).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Badkey);
    assert!(
        err.to_string().contains(
            "astMapPut1I(KeyMap): Failed to add item \"NEW\" to a KeyMap: \
             \"NEW\" is not a known item."
        ),
        "{err}"
    );
}

#[test]
fn put_elem_converts_or_reports_mpper() {
    let mut km = KeyMap::new();
    km.put_1i("V", &[1, 2], None).unwrap();
    // The value is converted to the entry's existing type.
    km.put_elem_d("V", 0, 2.6).unwrap();
    assert_eq!(km.get_1i("V").unwrap(), Some(vec![3, 2]));

    km.put_1a(
        "OBJ",
        vec![Some(AnyObject::KeyMap(Box::new(KeyMap::new())))],
        None,
    )
    .unwrap();
    let err = km.put_elem_i("OBJ", 0, 1).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Mpper);
    assert!(
        err.to_string().contains(
            "The supplied value cannot be converted to the data type of \
             KeyMap key \"OBJ\"."
        ),
        "{err}"
    );
}

#[test]
fn get_elem_rejects_an_out_of_range_index() {
    let mut km = KeyMap::new();
    km.put_1i("V", &[1, 2, 3], None).unwrap();
    assert_eq!(km.get_elem_i("V", 0).unwrap(), Some(1));
    assert_eq!(km.get_elem_i("V", 2).unwrap(), Some(3));

    // Unlike put, get does not tolerate an out-of-range index.
    let err = km.get_elem_i("V", 3).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Mpvin);
    assert!(
        err.to_string().contains(
            "Illegal zero-based vector index 3 supplied for KeyMap entry 'V' \
             - the vector has 3 elements."
        ),
        "{err}"
    );
}

#[test]
fn get_elem_conversion_failure_names_the_one_based_element() {
    // MAKE_MAPGETELEM's AST__MPGER message (keymap.c:6916-6919) uses
    // `elem + 1`, unlike its AST__MPVIN message a few lines above, which
    // uses the raw zero-based `elem`.
    let mut km = KeyMap::new();
    km.put_1c("V", &["1", "nope", "3"], None).unwrap();
    assert_eq!(km.get_elem_i("V", 0).unwrap(), Some(1));

    let err = km.get_elem_i("V", 1).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Mpger);
    assert!(
        err.to_string().contains(
            "astMapGetElemI(KeyMap): The value of element 2 of KeyMap key \
             \"V\" cannot be read using the requested data type."
        ),
        "{err}"
    );
}

#[test]
fn map_len_c_reports_the_longest_element() {
    // testkeymap.c asserts 4, 0 and 5 for these three cases.
    let mut km = KeyMap::new();
    km.put_0i("I", 1000, None).unwrap();
    assert_eq!(km.map_len_c("I").unwrap(), 4);

    // A missing key has no elements, so no length.
    assert_eq!(km.map_len_c("NOPE").unwrap(), 0);

    km.put_1c("V", &["Hello", "", "Hello"], None).unwrap();
    assert_eq!(km.map_len_c("V").unwrap(), 5);
}

#[test]
fn map_len_c_honours_key_case() {
    // astMapLenC (keymap.c:7886) folds the key through ConvertKey before
    // the table lookup, the same as astMapLength. With KeyCase=0 a
    // lookup in a different case from the key's own must still find the
    // entry, not silently report 0 as though the key were absent.
    let mut km = KeyMap::new();
    km.set_key_case(false).unwrap();
    km.put_0i("FREDS", 1000, None).unwrap();
    assert_eq!(km.map_len_c("fReDs").unwrap(), 4);
}

#[test]
fn map_get_c_joins_with_the_c_fixed_stride() {
    // testkeymap.c (around line 837): a vector whose elements are
    // "Hello", " " (the stored comment, not one of the elements — the
    // comment plays no part in the read) and "  Hello", giving mxlen 7.
    // C reads the elements into a buffer of mxlen+1 stride and walks that
    // stride, so a short element's slot is padded with the null bytes
    // left over from `astMalloc`'s buffer, which the fixed-stride walk
    // reads back as spaces only when the slot actually held some —
    // here the middle element (a single space) pads out to 7 characters,
    // giving "(Hello, ,  Hello)", 17 characters.
    let mut km = KeyMap::new();
    km.put_1c("V", &["Hello", " ", "  Hello"], None).unwrap();
    assert_eq!(km.map_len_c("V").unwrap(), 7);
    let got = km.map_get_c("V").unwrap().unwrap();
    assert_eq!(got, "(Hello, ,  Hello)");
    assert_eq!(got.len(), 17);

    // An empty element's slot is entirely null bytes, which the walk
    // reads back as an empty string rather than as padding: C's join
    // does not pad short elements out to mxlen at all, it just walks
    // each fixed-size slot as a null-terminated C string. Verified
    // against the repo's own built libast (see task-8-report.md).
    km.put_1c("W", &["Hello", "", "Hello"], None).unwrap();
    assert_eq!(km.map_len_c("W").unwrap(), 5);
    assert_eq!(km.map_get_c("W").unwrap().unwrap(), "(Hello,,Hello)");

    // A length-1 entry has no parentheses: C delegates to astMapGet0C.
    km.put_0c("S", "Hello", None).unwrap();
    assert_eq!(km.map_get_c("S").unwrap().as_deref(), Some("Hello"));

    // A missing key fails.
    assert_eq!(km.map_get_c("NOPE").unwrap(), None);
}

#[test]
fn put_u_creates_an_undefined_entry() {
    let mut km = KeyMap::new();
    km.put_u("U", Some("no value yet")).unwrap();

    // The key is there, but has no value.
    assert!(km.map_has_key("U").unwrap());
    assert!(!km.map_defined("U").unwrap());
    assert_eq!(km.map_type("U").unwrap(), AstType::Undef);
    assert_eq!(km.map_size(), 1);
    assert_eq!(km.map_comment("U").as_deref(), Some("no value yet"));

    // Reading it returns failure, not an error, even with KeyError set.
    km.set_key_error(true);
    assert_eq!(km.get_0i("U").unwrap(), None);

    // Putting a value over it makes it defined.
    km.put_0i("U", 3, None).unwrap();
    assert!(km.map_defined("U").unwrap());
    assert_eq!(km.get_0i("U").unwrap(), Some(3));
}

#[test]
fn map_type_and_has_key_on_a_missing_key() {
    let km = KeyMap::new();
    // Neither reports an error, and neither consults KeyError.
    assert_eq!(km.map_type("NOPE").unwrap(), AstType::BadType);
    assert!(!km.map_has_key("NOPE").unwrap());
    assert!(!km.map_defined("NOPE").unwrap());
}

#[test]
fn map_defined_on_a_missing_key_consults_key_error() {
    // Unlike map_type and map_has_key, astMapDefined's KeyError branch
    // fires for a genuinely absent key (SearchTableEntry returns NULL),
    // but not for a present-but-undefined one (keymap.c:7347-7434).
    let mut km = KeyMap::new();
    km.set_key_error(true);

    let err = km.map_defined("NOPE").unwrap_err();
    assert_eq!(err.code(), ErrorCode::Mpker);
    assert!(
        err.to_string()
            .contains("No value was found for NOPE in the supplied KeyMap."),
        "{err}"
    );

    km.put_u("U", None).unwrap();
    assert!(!km.map_defined("U").unwrap());
}

#[test]
fn map_defined_missing_key_error_uses_the_case_folded_key() {
    // astMapDefined's AST__MPKER message (keymap.c:7431-7433) quotes
    // ConvertKey's own `key` local, which is the case-folded string when
    // KeyCase=0 — not the caller's original, differently-cased argument.
    let mut km = KeyMap::new();
    km.set_key_case(false).unwrap();
    km.set_key_error(true);

    let err = km.map_defined("nope").unwrap_err();
    assert_eq!(err.code(), ErrorCode::Mpker);
    assert!(
        err.to_string()
            .contains("No value was found for NOPE in the supplied KeyMap."),
        "{err}"
    );
}

#[test]
fn map_remove_is_silent_on_a_missing_key() {
    let mut km = KeyMap::new();
    km.put_0i("A", 1, None).unwrap();
    km.map_remove("NOPE").unwrap();
    assert_eq!(km.map_size(), 1);
    km.map_remove("A").unwrap();
    assert_eq!(km.map_size(), 0);
    assert!(!km.map_has_key("A").unwrap());
}

#[test]
fn map_rename_moves_the_value_and_the_comment() {
    let mut km = KeyMap::new();
    km.put_0i("OLD", 42, Some("a comment")).unwrap();
    km.map_rename("OLD", "NEW").unwrap();

    assert!(!km.map_has_key("OLD").unwrap());
    assert_eq!(km.get_0i("NEW").unwrap(), Some(42));
    assert_eq!(km.map_comment("NEW").as_deref(), Some("a comment"));
    assert_eq!(km.map_size(), 1);

    // Renaming a key to itself does nothing at all.
    km.map_rename("NEW", "NEW").unwrap();
    assert_eq!(km.get_0i("NEW").unwrap(), Some(42));

    // Renaming a missing key does nothing, and is not an error.
    km.map_rename("GONE", "SOMETHING").unwrap();
    assert_eq!(km.map_size(), 1);

    // Renaming onto an occupied key overwrites it.
    km.put_0i("OTHER", 7, None).unwrap();
    km.map_rename("NEW", "OTHER").unwrap();
    assert_eq!(km.map_size(), 1);
    assert_eq!(km.get_0i("OTHER").unwrap(), Some(42));
}

#[test]
fn map_rename_respects_map_locked() {
    let mut km = KeyMap::new();
    km.put_0i("A", 1, None).unwrap();
    km.put_0i("B", 2, None).unwrap();
    km.set_map_locked(true);

    // Renaming onto an existing key is allowed: no new key appears.
    km.map_rename("A", "B").unwrap();
    assert_eq!(km.get_0i("B").unwrap(), Some(1));

    // Renaming onto a new key is not.
    let err = km.map_rename("B", "C").unwrap_err();
    assert_eq!(err.code(), ErrorCode::Badkey);
    assert!(
        err.to_string().contains(
            "Failed to rename item \"B\" in a KeyMap to \"C\": \"C\" is not a \
             known item."
        ),
        "{err}"
    );

    // A failed rename leaves the KeyMap unchanged: unlike C, which loses
    // the entry, Rust puts it back.
    assert!(km.map_has_key("B").unwrap());
    assert_eq!(km.get_0i("B").unwrap(), Some(1));
    assert_eq!(km.map_size(), 1);
}

#[test]
fn map_rename_locked_error_uses_the_case_folded_keys() {
    // astMapRename's AST__BADKEY message reports the keys after ConvertKey's
    // case folding, not the caller's original strings — confirmed against a
    // C oracle build: astMapRename(km, "a", "new key  ") with KeyCase=0
    // reports "A" and "NEW KEY  " (folded, but not trimmed of trailing
    // spaces), not "a" and "new key  ".
    let mut km = KeyMap::new();
    km.set_key_case(false).unwrap();
    km.put_0i("a", 1, None).unwrap();
    km.set_map_locked(true);

    let err = km.map_rename("a", "new key  ").unwrap_err();
    assert_eq!(err.code(), ErrorCode::Badkey);
    assert!(
        err.to_string().contains(
            "Failed to rename item \"A\" in a KeyMap to \"NEW KEY  \": \"NEW KEY  \" \
             is not a known item."
        ),
        "{err}"
    );

    // The failed rename left the KeyMap unchanged.
    assert!(km.map_has_key("a").unwrap());
    assert_eq!(km.get_0i("a").unwrap(), Some(1));
}

#[test]
fn map_key_walks_the_entries_in_order() {
    let mut km = KeyMap::new();
    // Insert deliberately out of alphabetical order.
    km.put_0i("Size", 1, None).unwrap();
    km.put_0i("Error", 2, None).unwrap();
    km.put_0i("Name", 3, None).unwrap();
    km.put_0i("Resolution", 4, None).unwrap();

    assert_eq!(km.map_size(), 4);
    let keys: Vec<String> = (0..km.map_size()).map(|i| km.map_key(i).unwrap()).collect();
    // KeyCmp order, not insertion order and not bucket order. KeyCase
    // defaults to case-sensitive, so the keys keep the case they were
    // put with.
    assert_eq!(keys, vec!["Error", "Name", "Resolution", "Size"]);

    // An out-of-range index is an error, not a None.
    let err = km.map_key(4).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Mpind);
    assert!(
        err.to_string()
            .contains("Cannot find element 4 (zero-based) of the KeyMap."),
        "{err}"
    );
}

#[test]
fn map_key_on_an_empty_keymap_is_out_of_range() {
    // GetKey (keymap.c:3430) finds no entry at index 0 in an empty
    // KeyMap and falls straight through to the AST__MPIND error, the
    // same branch an out-of-range index on a populated KeyMap takes.
    let km = KeyMap::new();
    let err = km.map_key(0).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Mpind);
    assert!(
        err.to_string()
            .contains("Cannot find element 0 (zero-based) of the KeyMap."),
        "{err}"
    );
}

#[test]
fn map_iterate_on_an_empty_keymap_returns_none() {
    // astMapIterate (keymap.c:8599) has nothing to walk, so even the
    // first call with reset=true returns immediately with no key.
    let mut km = KeyMap::new();
    assert_eq!(km.map_iterate(true), None);
    assert_eq!(km.map_iterate(false), None);
}

#[test]
fn map_iterate_visits_every_key_once_and_then_stops() {
    let mut km = KeyMap::new();
    for k in ["Size", "Error", "Name"] {
        km.put_0c(k, "x", None).unwrap();
    }

    let mut seen = Vec::new();
    let mut key = km.map_iterate(true);
    while let Some(k) = key {
        seen.push(k);
        // A runaway iteration would hang the test, so bound it.
        assert!(seen.len() <= 3, "iteration did not terminate: {seen:?}");
        key = km.map_iterate(false);
    }
    // KeyCase defaults to case-sensitive, so the keys keep the case they
    // were put with.
    assert_eq!(seen, vec!["Error", "Name", "Size"]);

    // Once exhausted, the cursor has reset, so a fresh walk works.
    assert_eq!(km.map_iterate(false).as_deref(), Some("Error"));
}

#[test]
fn map_iterate_terminates_with_sort_by_set() {
    // C's astMapIterate loops forever here; see docs/issues/c-library-quirks.md.
    let mut km = KeyMap::new();
    for k in ["A", "B", "C"] {
        km.put_0i(k, 1, None).unwrap();
    }
    km.set_sort_by(SortBy::KeyUp);

    let mut count = 0;
    let mut key = km.map_iterate(true);
    while key.is_some() {
        count += 1;
        assert!(count <= 3, "iteration did not terminate");
        key = km.map_iterate(false);
    }
    assert_eq!(count, 3);
}

#[test]
fn sort_by_reorders_the_read_out() {
    let mut km = KeyMap::new();
    // Insert oldest-first, non-alphabetically.
    km.put_0i("Zulu", 1, None).unwrap();
    km.put_0i("Alpha", 2, None).unwrap();
    km.put_0i("Mike", 3, None).unwrap();
    // Re-put the oldest key: its value age advances, its key age does not.
    km.put_0i("Zulu", 9, None).unwrap();

    let read = |km: &KeyMap| -> Vec<String> {
        (0..km.map_size()).map(|i| km.map_key(i).unwrap()).collect()
    };

    // KeyCase defaults to true (astMAKE_GET(KeyMap,KeyCase,int,1,...),
    // keymap.c:10158), so keys are stored case-preserved, not upper-cased.
    km.set_sort_by(SortBy::KeyUp);
    assert_eq!(read(&km), vec!["Alpha", "Mike", "Zulu"]);

    km.set_sort_by(SortBy::KeyDown);
    assert_eq!(read(&km), vec!["Zulu", "Mike", "Alpha"]);

    // AgeUp is newest-first, and Zulu's re-put made it newest by value.
    km.set_sort_by(SortBy::AgeUp);
    assert_eq!(read(&km), vec!["Zulu", "Mike", "Alpha"]);

    km.set_sort_by(SortBy::AgeDown);
    assert_eq!(read(&km), vec!["Alpha", "Mike", "Zulu"]);

    // KeyAgeUp is blind to the re-put, so Zulu is still the oldest key and
    // therefore sorts last.
    km.set_sort_by(SortBy::KeyAgeUp);
    assert_eq!(read(&km), vec!["Mike", "Alpha", "Zulu"]);

    km.set_sort_by(SortBy::KeyAgeDown);
    assert_eq!(read(&km), vec!["Zulu", "Alpha", "Mike"]);

    // Clearing returns to the alphabetical default.
    km.clear_sort_by();
    assert_eq!(read(&km), vec!["Alpha", "Mike", "Zulu"]);
}

#[test]
fn map_copy_adds_and_replaces() {
    let mut src = KeyMap::new();
    src.put_0i("NEW", 1, Some("from source")).unwrap();
    src.put_0i("BOTH", 2, None).unwrap();

    let mut dest = KeyMap::new();
    dest.put_0c("BOTH", "old", None).unwrap();
    dest.put_0i("ONLYDEST", 3, None).unwrap();

    dest.map_copy(&src).unwrap();

    assert_eq!(dest.map_size(), 3);
    assert_eq!(dest.get_0i("NEW").unwrap(), Some(1));
    assert_eq!(dest.map_comment("NEW").as_deref(), Some("from source"));
    // The replaced entry takes the source's type too, not just its value.
    assert_eq!(dest.map_type("BOTH").unwrap(), AstType::Int);
    assert_eq!(dest.get_0i("BOTH").unwrap(), Some(2));
    assert_eq!(dest.get_0i("ONLYDEST").unwrap(), Some(3));
}

#[test]
fn map_copy_keeps_the_destination_key_age_for_a_replaced_key() {
    let mut dest = KeyMap::new();
    dest.put_0i("FIRST", 1, None).unwrap();
    dest.put_0i("SECOND", 2, None).unwrap();

    let mut src = KeyMap::new();
    src.put_0i("FIRST", 9, None).unwrap();

    dest.map_copy(&src).unwrap();

    // FIRST is still the older key, so KeyAgeDown puts it first.
    dest.set_sort_by(SortBy::KeyAgeDown);
    let keys: Vec<String> = (0..dest.map_size())
        .map(|i| dest.map_key(i).unwrap())
        .collect();
    assert_eq!(keys, vec!["FIRST", "SECOND"]);
}

#[test]
fn map_copy_refuses_a_new_key_in_a_locked_destination() {
    let mut src = KeyMap::new();
    src.put_0i("EXISTS", 1, None).unwrap();
    src.put_0i("FRESH", 2, None).unwrap();

    let mut dest = KeyMap::new();
    dest.put_0i("EXISTS", 0, None).unwrap();
    dest.set_map_locked(true);

    let err = dest.map_copy(&src).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Badkey);
    assert!(
        err.to_string()
            .contains("Failed to copy item \"FRESH\": \"FRESH\" is not a known item."),
        "{err}"
    );
}

#[test]
fn map_copy_merges_nested_key_maps_instead_of_replacing_them() {
    let mut inner_src = KeyMap::new();
    inner_src.put_0i("FROMSRC", 1, None).unwrap();
    let mut src = KeyMap::new();
    src.put_0a("NESTED", AnyObject::KeyMap(Box::new(inner_src)), None)
        .unwrap();

    let mut inner_dest = KeyMap::new();
    inner_dest.put_0i("FROMDEST", 2, None).unwrap();
    let mut dest = KeyMap::new();
    dest.put_0a("NESTED", AnyObject::KeyMap(Box::new(inner_dest)), None)
        .unwrap();

    dest.map_copy(&src).unwrap();

    // The nested KeyMap was merged, so it holds both keys.
    let Some(AnyObject::KeyMap(nested)) = dest.get_0a("NESTED").unwrap().unwrap() else {
        panic!("expected a nested KeyMap");
    };
    assert_eq!(nested.map_size(), 2);
    assert_eq!(nested.get_0i("FROMSRC").unwrap(), Some(1));
    assert_eq!(nested.get_0i("FROMDEST").unwrap(), Some(2));
}

#[test]
fn map_copy_replaces_a_nested_key_map_when_the_other_side_is_not_one() {
    // A non-Object destination entry is replaced, not merged, even though
    // the source is a KeyMap.
    let mut inner = KeyMap::new();
    inner.put_0i("A", 1, None).unwrap();
    let mut src = KeyMap::new();
    src.put_0a("K", AnyObject::KeyMap(Box::new(inner)), None)
        .unwrap();

    let mut dest = KeyMap::new();
    dest.put_0i("K", 7, None).unwrap();
    dest.map_copy(&src).unwrap();

    assert_eq!(dest.map_type("K").unwrap(), AstType::Object);
}

#[test]
fn map_copy_entry_copies_one_key_and_honours_merge() {
    let mut src = KeyMap::new();
    src.put_0i("WANTED", 1, None).unwrap();
    src.put_0i("UNWANTED", 2, None).unwrap();

    let mut dest = KeyMap::new();
    dest.map_copy_entry("WANTED", &src, false).unwrap();
    assert_eq!(dest.map_size(), 1);
    assert_eq!(dest.get_0i("WANTED").unwrap(), Some(1));

    // A key absent from the source is silently ignored.
    dest.map_copy_entry("MISSING", &src, false).unwrap();
    assert_eq!(dest.map_size(), 1);

    // With merge false, a nested KeyMap is replaced rather than merged.
    let mut inner_src = KeyMap::new();
    inner_src.put_0i("FROMSRC", 1, None).unwrap();
    let mut src2 = KeyMap::new();
    src2.put_0a("N", AnyObject::KeyMap(Box::new(inner_src)), None)
        .unwrap();

    let mut inner_dest = KeyMap::new();
    inner_dest.put_0i("FROMDEST", 2, None).unwrap();
    let mut dest2 = KeyMap::new();
    dest2
        .put_0a("N", AnyObject::KeyMap(Box::new(inner_dest)), None)
        .unwrap();

    dest2.map_copy_entry("N", &src2, false).unwrap();
    let Some(AnyObject::KeyMap(nested)) = dest2.get_0a("N").unwrap().unwrap() else {
        panic!("expected a nested KeyMap");
    };
    assert_eq!(nested.map_size(), 1);
    assert!(nested.map_has_key("FROMSRC").unwrap());
    assert!(!nested.map_has_key("FROMDEST").unwrap());
}

#[test]
fn map_copy_merges_through_a_length_one_vector() {
    // C's merge test accepts nel == 0 or nel == 1, so a length-1 vector of
    // KeyMaps merges through its element 0.
    let mut inner_src = KeyMap::new();
    inner_src.put_0i("FROMSRC", 1, None).unwrap();
    let mut src = KeyMap::new();
    src.put_1a(
        "N",
        vec![Some(AnyObject::KeyMap(Box::new(inner_src)))],
        None,
    )
    .unwrap();

    let mut inner_dest = KeyMap::new();
    inner_dest.put_0i("FROMDEST", 2, None).unwrap();
    let mut dest = KeyMap::new();
    dest.put_1a(
        "N",
        vec![Some(AnyObject::KeyMap(Box::new(inner_dest)))],
        None,
    )
    .unwrap();

    dest.map_copy(&src).unwrap();

    let nested = dest.get_1a("N").unwrap().unwrap();
    assert_eq!(nested.len(), 1);
    let Some(AnyObject::KeyMap(nested)) = &nested[0] else {
        panic!("expected a nested KeyMap");
    };
    assert_eq!(nested.map_size(), 2);
}

#[test]
fn map_copy_entry_locked_destination_error_keeps_trailing_spaces() {
    // C's astMapCopyEntry binds `key` once to ConvertKey's untrimmed
    // output and reuses that same binding for the AST__BADKEY message
    // (keymap.c:4349-4372); only the table lookups use a trimmed form.
    // The stored key is always trimmed, so pass an untrimmed key in the
    // call to exercise the divergence between lookup key and message key.
    let mut src = KeyMap::new();
    src.put_0i("FRESH", 1, None).unwrap();

    let mut dest = KeyMap::new();
    dest.set_map_locked(true);

    let err = dest.map_copy_entry("FRESH   ", &src, false).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Badkey);
    assert!(
        err.to_string()
            .contains("Failed to copy item \"FRESH   \": \"FRESH   \" is not a known item."),
        "{err}"
    );
}

// ---------------------------------------------------------------------------
// The blocks below follow reference/starlink-ast/ast_tester/testkeymap.c in
// its own order: its two static helper functions first, then `main`.
// ---------------------------------------------------------------------------

/// Serialise, read back, and compare entry by entry. Ported from
/// `checkdump` (`testkeymap.c:64`), which compares sizes, then per key the
/// type and either the Object class name or the string rendering.
fn check_dump(km: &KeyMap) {
    let text = to_native_string(&AnyObject::KeyMap(Box::new(km.clone()))).unwrap();
    let AnyObject::KeyMap(result) = from_native_string(&text).unwrap() else {
        panic!("round trip did not give a KeyMap");
    };

    // checkDump 1.
    assert_eq!(result.map_size(), km.map_size());
    for i in 0..result.map_size() {
        let key = result.map_key(i).unwrap();
        let ty = result.map_type(&key).unwrap();
        // checkDump 4.
        assert_eq!(ty, km.map_type(&key).unwrap(), "key {key}");
        if ty == AstType::Object {
            // checkDump 5 / 6 / 7. C compares astGetC(obj, "class"); the
            // AnyObject variant is the Rust equivalent, and comparing it is
            // strictly stronger than comparing class-name strings.
            let a = result.get_0a(&key).unwrap().expect("entry is present");
            let b = km.get_0a(&key).unwrap().expect("entry is present");
            assert_eq!(
                a.as_ref().map(std::mem::discriminant),
                b.as_ref().map(std::mem::discriminant),
                "key {key}"
            );
        } else {
            // checkDump 8 / 9 / 10.
            let a = result.get_0c(&key).unwrap().expect("entry is present");
            let b = km.get_0c(&key).unwrap().expect("entry is present");
            assert_eq!(a, b, "key {key}");
        }
    }
}

#[test]
fn case_insensitive_keys() {
    // testcasesens (testkeymap.c:278).
    let mut map = KeyMap::new();
    map.set_key_case(false).unwrap();
    map.put_0i("Freds", 1999, Some("com 1")).unwrap();

    // Error case 1.
    assert!(map.map_has_key("fReDs").unwrap());
    // Error case 2: the stored key is upper-cased.
    assert_eq!(map.map_key(0).unwrap(), "FREDS");
    // Error case 3 / 4 / 4b: an int reads as the string "1999".
    let sval = map.get_0c("freds").unwrap().expect("Freds is present");
    assert_eq!(sval, "1999");
    assert_eq!(sval.len(), 4);

    // Error case 5. Setting KeyCase to the value it already reports is
    // allowed however full the KeyMap is (keymap.c:9279 only walks the
    // table when the value would change), which is why C does the astSetL
    // first; clearing it on a non-empty map would change the reported
    // value, so it is AST__NOWRT.
    map.set_key_case(false).unwrap();
    let err = map.clear_key_case().unwrap_err();
    assert_eq!(err.code(), ErrorCode::Nowrt);

    // Error case 6 / 7.
    assert_eq!(map.map_size(), 1);
    map.map_remove("freDs").unwrap();
    assert_eq!(map.map_size(), 0);

    // Error case 8: now that the map is empty the clear succeeds, and with
    // KeyCase back at its default a leading space is significant.
    map.clear_key_case().unwrap();
    map.put_0i("Freds", 1999, Some("com 1")).unwrap();
    assert!(!map.map_has_key(" fReDs").unwrap());
}

/// The five keys `testsorting` (`testkeymap.c:133`) puts, in put order.
const SORT_KEYS: [&str; 5] = ["ABC", "zzzzzzzzzzz", "this_is_a_key", "HE-HE", "A"];

/// The keys read back in `map_key` order.
fn read_keys(km: &KeyMap) -> Vec<String> {
    (0..km.map_size()).map(|i| km.map_key(i).unwrap()).collect()
}

/// A KeyMap holding `SORT_KEYS` put in order with values 1..=5, under the
/// given `SortBy`.
fn sort_fixture(sort_by: SortBy) -> KeyMap {
    let mut km = KeyMap::new();
    km.set_sort_by(sort_by);
    for (i, key) in SORT_KEYS.iter().enumerate() {
        km.put_0i(key, i as i32 + 1, Some(" ")).unwrap();
    }
    km
}

#[test]
fn sort_age_down_and_up() {
    // testsorting, first block (testkeymap.c:145).
    let mut km = sort_fixture(SortBy::AgeDown);

    // Error Sort 1: AgeDown gives put order, even for entries added to an
    // already-sorted KeyMap.
    assert_eq!(read_keys(&km), SORT_KEYS);

    // Error Sort 2: AgeUp reverses it, sorting entries already present.
    km.set_sort_by(SortBy::AgeUp);
    let mut reversed = SORT_KEYS;
    reversed.reverse();
    assert_eq!(read_keys(&km), reversed);

    // Error Sort 2b / 2c: re-putting an existing key makes it the newest by
    // value, so under AgeDown it moves to the end.
    km.put_0i(SORT_KEYS[0], 10, Some(" ")).unwrap();
    km.set_sort_by(SortBy::AgeDown);
    assert_eq!(
        read_keys(&km),
        [
            SORT_KEYS[1],
            SORT_KEYS[2],
            SORT_KEYS[3],
            SORT_KEYS[4],
            SORT_KEYS[0]
        ]
    );
}

#[test]
fn sort_key_age() {
    // testsorting, second block (testkeymap.c:199).
    let mut km = sort_fixture(SortBy::KeyAgeDown);

    // Error Sort 0.
    assert_eq!(read_keys(&km), SORT_KEYS);

    // Error Sort -1.
    km.set_sort_by(SortBy::KeyAgeUp);
    let mut reversed = SORT_KEYS;
    reversed.reverse();
    assert_eq!(read_keys(&km), reversed);

    // Error Sort -2: re-putting an existing key does NOT change its key
    // age, which is why the keymember counter is separate from member.
    km.put_0i(SORT_KEYS[0], 10, Some(" ")).unwrap();
    km.set_sort_by(SortBy::KeyAgeDown);
    assert_eq!(read_keys(&km), SORT_KEYS);
}

#[test]
fn sort_key_alphabetical() {
    // testsorting, third block (testkeymap.c:246).
    const SKEYS: [&str; 5] = ["A", "ABC", "HE-HE", "this_is_a_key", "zzzzzzzzzzz"];

    let mut km = sort_fixture(SortBy::KeyUp);

    // Error Sort 3. Note "HE-HE" before "this_is_a_key": KeyCmp folds
    // case, so this is not a plain byte ordering.
    assert_eq!(read_keys(&km), SKEYS);

    // Error Sort 4.
    km.set_sort_by(SortBy::KeyDown);
    let mut reversed = SKEYS;
    reversed.reverse();
    assert_eq!(read_keys(&km), reversed);
}

/// The seven scalar entries `main` puts at `testkeymap.c:374-380`, one per
/// scalar type. C's `astSkyFrame( " " )` leaves System at its ICRS default.
fn seven_key_map() -> KeyMap {
    let mut map = KeyMap::new();
    map.put_0s("Freds", 1999, Some("com 1")).unwrap();
    map.put_0i("Fredi", 1999, Some("com 1")).unwrap();
    map.put_0k("Fredk", 1999, Some("com 1")).unwrap();
    map.put_0d("Fredd", 1999.9, Some("com2 ")).unwrap();
    map.put_0f("Fredr", 1999.9, Some("com2 ")).unwrap();
    map.put_0c("Fredc", "Hello", Some(" ")).unwrap();
    map.put_0a(
        "Freda",
        AnyObject::SkyFrame(SkyFrame::new(SkySystem::Icrs)),
        Some(" "),
    )
    .unwrap();
    map
}

#[test]
fn scalar_puts_and_types() {
    // main (testkeymap.c:372-431).
    let map = seven_key_map();

    // Error -12.
    assert!(map.map_defined("Freda").unwrap());

    // Error -11 / -10 / -9: an Object has no string rendering, so its
    // MapLenC is 0, not an error.
    assert_eq!(map.map_len_c("Fredi").unwrap(), 4);
    assert_eq!(map.map_len_c("Freda").unwrap(), 0);
    assert_eq!(map.map_len_c("Fredc").unwrap(), 5);

    // Error -8: KeyCase defaults to case-sensitive, so "freda" is absent.
    assert_eq!(map.map_type("freda").unwrap(), AstType::BadType);
    // Error -7 / -6 / -5 / -5b / -4.
    assert_eq!(map.map_type("Freda").unwrap(), AstType::Object);
    assert_eq!(map.map_type("Fredc").unwrap(), AstType::String);
    assert_eq!(map.map_type("Fredd").unwrap(), AstType::Double);
    assert_eq!(map.map_type("Fredr").unwrap(), AstType::Float);
    assert_eq!(map.map_type("Fredi").unwrap(), AstType::Int);

    // Error -3 / -2.
    assert!(!map.map_has_key("fredi").unwrap());
    assert!(map.map_has_key("Fredi").unwrap());
}

#[test]
fn clone_preserves_every_entry() {
    // main (testkeymap.c:433-532). C's astCopy is Rust's Clone.
    let map = seven_key_map();
    let mut map2 = map.clone();

    // Error 0.
    assert_eq!(map2.map_size(), 7);

    // Error badkey / nokey: every put key is present exactly once, and
    // nothing else is.
    let mut keys = read_keys(&map2);
    keys.sort();
    assert_eq!(
        keys,
        [
            "Freda", "Fredc", "Fredd", "Fredi", "Fredk", "Fredr", "Freds"
        ]
    );

    // Error -1: a scalar has length 1 through astMapLength.
    assert_eq!(map2.map_length("Fredi"), 1);

    // Error 1 / 2, 1k / 2k, 1 / 2B, 3 / 4, 3b / 4b.
    assert_eq!(map2.get_0i("Fredi").unwrap(), Some(1999));
    assert_eq!(map2.get_0k("Fredk").unwrap(), Some(1999));
    assert_eq!(map2.get_0s("Freds").unwrap(), Some(1999));
    assert_eq!(map2.get_0d("Fredd").unwrap(), Some(1999.9));
    assert_eq!(map2.get_0f("Fredr").unwrap(), Some(1999.9));

    // Error 5 / 6 / 7.
    let cval = map2.get_0c("Fredc").unwrap().expect("Fredc is present");
    assert_eq!(cval.len(), 5);
    assert_eq!(cval, "Hello");

    // Error 5b / 6b / 7b: astMapGetC on a length-1 entry delegates to
    // astMapGet0C, so there are no parentheses.
    let cval = map2.map_get_c("Fredc").unwrap().expect("Fredc is present");
    assert_eq!(cval.len(), 5);
    assert_eq!(cval, "Hello");

    // Error 8 / 9.
    let aval = map2.get_0a("Freda").unwrap().expect("Freda is present");
    assert!(matches!(aval, Some(AnyObject::SkyFrame(_))));

    // Clone is the deep copy, so the two KeyMaps are independent. C reads
    // only the copy here, but for entries holding an AnyObject the property
    // is not free. The stored object is only reachable through get_0a,
    // which itself clones, so replacing the whole entry is the strongest
    // mutation the public API allows.
    map2.put_0i("Fredi", -7, Some("com 1")).unwrap();
    map2.put_0a(
        "Freda",
        AnyObject::SpecFrame(SpecFrame::new(SpecSystem::Wavelen)),
        Some(" "),
    )
    .unwrap();
    map2.map_remove("Fredc").unwrap();

    assert_eq!(map.map_size(), 7);
    assert_eq!(map.get_0i("Fredi").unwrap(), Some(1999));
    assert_eq!(map.get_0c("Fredc").unwrap().as_deref(), Some("Hello"));
    let aval = map.get_0a("Freda").unwrap().expect("Freda is present");
    assert!(matches!(aval, Some(AnyObject::SkyFrame(_))));
}

#[test]
fn scalar_cross_type_reads() {
    // main (testkeymap.c:534-590).
    let map2 = seven_key_map();

    // Error 10 / 11, 10b / 11b: an int reads as double and as float.
    assert_eq!(map2.get_0d("Fredi").unwrap(), Some(1999.0));
    assert_eq!(map2.get_0f("Fredi").unwrap(), Some(1999.0));

    // Error 12 / 13a / 13.
    let cval = map2.get_0c("Fredi").unwrap().expect("Fredi is present");
    assert_eq!(cval.len(), 4);
    assert_eq!(cval, "1999");

    // Error 14 / 15, 14k / 15k, 14b / 15b: ConvertValue rounds 1999.9 up
    // by adding a half and truncating, in all three integer widths.
    assert_eq!(map2.get_0i("Fredd").unwrap(), Some(2000));
    assert_eq!(map2.get_0k("Fredd").unwrap(), Some(2000));
    assert_eq!(map2.get_0s("Fredd").unwrap(), Some(2000));

    // Error 16 / 17a / 17.
    let cval = map2.get_0c("Fredd").unwrap().expect("Fredd is present");
    assert_eq!(cval.len(), 6);
    assert_eq!(cval, "1999.9");
}

#[test]
fn a_scalar_reads_as_a_length_one_vector() {
    // main (testkeymap.c:592-636). C pre-fills a two-element buffer with a
    // sentinel and checks that element 1 is untouched, which is a
    // statement about C's caller-supplied buffer; get_1* returns a Vec
    // sized to the entry's own length, so the same fact is the length.
    let map2 = seven_key_map();

    // Error 18 / 19 / 20 / 21.
    assert_eq!(map2.get_1i("Fredi").unwrap(), Some(vec![1999]));
    // Error 22 / 23 / 24 / 25.
    assert_eq!(map2.get_1d("Fredd").unwrap(), Some(vec![1999.9]));

    // Error 26 / 27 / 28 / 29.
    let avec = map2.get_1a("Freda").unwrap().expect("Freda is present");
    assert_eq!(avec.len(), 1);
    assert!(matches!(avec[0], Some(AnyObject::SkyFrame(_))));
}

/// [`seven_key_map`] with five of its scalars replaced by vectors, as
/// `main` does at `testkeymap.c:638-677`. `Fredk` and `Fredr` stay scalar.
/// C's `astSpecFrame( " " )` leaves System at its WAVELEN default
/// (`GetSystem`, `specframe.c:2014`).
fn vector_key_map() -> KeyMap {
    let mut map = seven_key_map();
    map.put_1i("Fredi", &[1999, 0], Some("com 1")).unwrap();
    // Error 29b.
    assert_eq!(map.map_length("Fredi"), 2);
    map.put_1s("Freds", &[1999, 0], Some("com 1")).unwrap();
    // Error 29c.
    assert_eq!(map.map_length("Freds"), 2);
    map.put_1d("Fredd", &[1999.9, -0.01], Some("com2")).unwrap();
    map.put_1c("Fredc", &["Hello", " ", "  Hello"], Some(" "))
        .unwrap();
    map.put_1a(
        "Freda",
        vec![
            Some(AnyObject::SkyFrame(SkyFrame::new(SkySystem::Icrs))),
            None,
            Some(AnyObject::SpecFrame(SpecFrame::new(SpecSystem::Wavelen))),
            None,
        ],
        Some(" "),
    )
    .unwrap();
    map
}

#[test]
fn vector_puts_replace_scalars() {
    // main (testkeymap.c:638-763). A scalar read of a vector entry gives
    // the entry's first element, because C's MAKE_MAPGET0 takes the
    // storage base pointer as `raw` either way.
    let map2 = vector_key_map().clone();

    // Error A1 / A2, A1k / A2k, A3 / A4.
    assert_eq!(map2.get_0i("Fredi").unwrap(), Some(1999));
    assert_eq!(map2.get_0k("Fredk").unwrap(), Some(1999));
    assert_eq!(map2.get_0d("Fredd").unwrap(), Some(1999.9));

    // Error A5 / A6 / A7: MapLenC reports the longest stored element,
    // "  Hello" at 7, while MapGet0C still gives element 0.
    let cval = map2.get_0c("Fredc").unwrap().expect("Fredc is present");
    assert_eq!(map2.map_len_c("Fredc").unwrap(), 7);
    assert_eq!(cval, "Hello");

    // Error A8 / A9.
    let aval = map2.get_0a("Freda").unwrap().expect("Freda is present");
    assert!(matches!(aval, Some(AnyObject::SkyFrame(_))));

    // Error A10 / A11, A12 / A13a / A13.
    assert_eq!(map2.get_0d("Fredi").unwrap(), Some(1999.0));
    let cval = map2.get_0c("Fredi").unwrap().expect("Fredi is present");
    assert_eq!(cval.len(), 4);
    assert_eq!(cval, "1999");

    // Error A14 / A15, A14k / A15k.
    assert_eq!(map2.get_0i("Fredd").unwrap(), Some(2000));
    assert_eq!(map2.get_0k("Fredd").unwrap(), Some(2000));

    // Error A16 / A17a / A17.
    let cval = map2.get_0c("Fredd").unwrap().expect("Fredd is present");
    assert_eq!(cval.len(), 6);
    assert_eq!(cval, "1999.9");
}

#[test]
fn vector_reads() {
    // main (testkeymap.c:765-835).
    let map2 = vector_key_map().clone();

    // Error B1 / B2a / B2b / B2c.
    assert_eq!(map2.get_1i("Fredi").unwrap(), Some(vec![1999, 0]));
    // Error B3 / B4a / B4b / B4c.
    assert_eq!(map2.get_1d("Fredd").unwrap(), Some(vec![1999.9, -0.01]));

    // Error B5 / B6a / B6b / B6c / B6d / B6e: elements 1 and 3 were put as
    // NULL and read back as NULL (astMapPut1A's `value[i] ? astClone(...)
    // : NULL`, keymap.c:5012).
    let avec = map2.get_1a("Freda").unwrap().expect("Freda is present");
    assert_eq!(avec.len(), 4);
    assert!(matches!(avec[0], Some(AnyObject::SkyFrame(_))));
    assert!(avec[1].is_none());
    assert!(matches!(avec[2], Some(AnyObject::SpecFrame(_))));
    assert!(avec[3].is_none());

    // Error B7 / B8a / B8b / B8c / B8d. C reads into a flat buffer of
    // 11-byte slots and checks the first bytes of slots 0, 1 and 2; the
    // Rust read is unpadded, so the same facts are the values.
    assert_eq!(
        map2.get_1c("Fredc").unwrap(),
        Some(vec![
            "Hello".to_string(),
            " ".to_string(),
            "  Hello".to_string()
        ])
    );
}

// testkeymap.c:837-847 (Error BB1 / BB2 / BB3) reads the same "Fredc"
// vector back through astMapGetC and asserts the 17-character
// "(Hello, ,  Hello)"; `map_get_c_joins_with_the_c_fixed_stride` above
// already covers exactly that, including the length.

#[test]
fn element_reads() {
    // main (testkeymap.c:849-877).
    let map2 = vector_key_map().clone();

    // Error B1z / B2bz.
    assert_eq!(map2.get_elem_i("Fredi", 0).unwrap(), Some(1999));
    // Error B3z / B4cz.
    assert_eq!(map2.get_elem_d("Fredd", 1).unwrap(), Some(-0.01));
    // Error B5z / B6dz.
    let aval = map2.get_elem_a("Freda", 2).unwrap().expect("element 2");
    assert!(matches!(aval, Some(AnyObject::SpecFrame(_))));
    // Error B7z / B8dz.
    assert_eq!(
        map2.get_elem_c("Fredc", 2).unwrap().as_deref(),
        Some("  Hello")
    );
}

#[test]
fn remove_then_read_fails() {
    // main (testkeymap.c:879-883).
    let mut map2 = vector_key_map().clone();

    // Removing an absent key is silent.
    map2.map_remove("Bert").unwrap();
    map2.map_remove("Fredc").unwrap();
    // Error C1: the read now reports "no such key", not an error.
    assert_eq!(map2.get_1c("Fredc").unwrap(), None);
}

#[test]
fn check_dump_round_trip() {
    // main (testkeymap.c:885), on the KeyMap left by remove_then_read_fails.
    let mut map2 = vector_key_map().clone();
    map2.map_remove("Bert").unwrap();
    map2.map_remove("Fredc").unwrap();
    check_dump(&map2);
}

#[test]
fn five_hundred_entries() {
    // main (testkeymap.c:890-931). 200 int keys, 299 int64 keys and one
    // int64 key holding a value no int could.
    let mut map = KeyMap::new();
    for i in 1..=200 {
        map.put_0i(&format!("Fred{i}"), i, Some(" ")).unwrap();
    }
    for i in 201..=499 {
        map.put_0k(&format!("Fred{i}"), i64::from(i), Some(" "))
            .unwrap();
    }
    map.put_0k("Fred500", i64::from(i32::MAX) * 100, Some(" "))
        .unwrap();

    // Error d1.
    assert_eq!(map.map_size(), 500);
    // Error d2 / d3.
    assert_eq!(map.map_type("Fred123").unwrap(), AstType::Int);
    assert_eq!(map.map_type("Fred234").unwrap(), AstType::KInt);
    // Error d4 / d5.
    assert_eq!(map.get_0c("Fred489").unwrap().as_deref(), Some("489"));
    // Error d6 / d7.
    assert_eq!(
        map.get_0k("Fred500").unwrap().map(|k| k / 100),
        Some(i64::from(i32::MAX))
    );

    // checkDump 2: the largest round trip in the file.
    check_dump(&map);
}

#[test]
fn put_elem_grows_the_vector() {
    // main (testkeymap.c:933-986).
    let mut map = KeyMap::new();
    map.put_1i("Fredi", &[1, 2], Some("com 1")).unwrap();

    // Error GETELEM_1 / GETELEM_2: an in-range index overwrites.
    map.put_elem_i("Fredi", 0, -1).unwrap();
    assert_eq!(map.get_elem_i("Fredi", 0).unwrap(), Some(-1));

    // Error GETELEM_3 / GETELEM_4: index 9 is past the length 2, so the
    // value lands at index 2.
    map.put_elem_i("Fredi", 9, -2).unwrap();
    assert_eq!(map.get_elem_i("Fredi", 2).unwrap(), Some(-2));

    // Error GETELEM_5 / GETELEM_6. C spells this index -1, which
    // MAKE_MAPPUTELEM's `new = ( elem >= nel || elem < 0 )`
    // (keymap.c:8392) treats identically to any index at or past the
    // length; put_elem_i takes a usize, so pass the length itself.
    let n = map.map_length("Fredi");
    map.put_elem_i("Fredi", n, -3).unwrap();
    assert_eq!(map.get_elem_i("Fredi", 3).unwrap(), Some(-3));

    // Error GETELEM_7.
    assert_eq!(map.map_length("Fredi"), 4);

    // Error GETELEM_8: put_elem_a on an absent key creates a length-1
    // vector, whatever the index.
    let mut map2 = KeyMap::new();
    map2.put_elem_a("A A", 0, Some(AnyObject::KeyMap(Box::new(map))))
        .unwrap();
    assert_eq!(map2.map_length("A A"), 1);

    // Error GETELEM_9 / GETELEM_10 / GETELEM_11: the nested KeyMap still
    // holds the four-element vector.
    let aval = map2.get_elem_a("A A", 0).unwrap().expect("A A is present");
    let Some(AnyObject::KeyMap(nested)) = aval else {
        panic!("expected a nested KeyMap");
    };
    assert_eq!(nested.get_elem_i("Fredi", 3).unwrap(), Some(-3));

    // testkeymap.c:988-1000 then puts map2 into itself and expects
    // AST__KYCIR. put_elem_a moves its value in while `self` is already
    // borrowed mutably, so a caller cannot pass a KeyMap to its own
    // put_elem_a: C's CheckCircle guard (keymap.c:8461) has no Rust
    // counterpart. See docs/issues/c-library-quirks.md.
}

#[test]
fn map_locked_rejects_new_keys() {
    // main (testkeymap.c:1002-1041). C keeps writing to the nested KeyMap
    // through its own km2 handle, which AST reference-counts so that the
    // handle and the stored Object are the same object. put_0a takes its
    // value by value, so no such alias survives the insert; the nested map
    // is read back out as a get_0a clone and the inherited flag checked
    // and exercised on that.
    let mut map = KeyMap::new();
    map.put_0c(" B", "Hello", Some(" ")).unwrap();

    // Error GETELEM_12B: a new key in a locked map is AST__BADKEY.
    map.set_map_locked(true);
    let err = map.put_0c(" BZZ", "Bye Bye", Some(" ")).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Badkey);
    map.clear_map_locked();

    // Unlocked, the same put succeeds.
    map.put_0c(" BZZ", "Bye Bye", Some(" ")).unwrap();

    let mut km2 = KeyMap::new();
    km2.put_0c(" BZZ", "Bye Bye", Some(" ")).unwrap();
    map.put_0a(" BZY", AnyObject::KeyMap(Box::new(km2)), Some(" "))
        .unwrap();

    // Error GETELEM_12C: MapLocked set on the outer map is inherited by
    // every nested KeyMap, where it blocks a new key but not a replacement.
    map.set_map_locked(true);
    map.put_0c(" BZZ", "You Bye", Some(" ")).unwrap();

    let mut nested = nested_key_map(&map, " BZY");
    assert!(nested.map_locked());
    nested.put_0c(" BZZ", "You Bye", Some(" ")).unwrap();
    let err = nested.put_0c(" BZA", "No Bye", Some(" ")).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Badkey);

    // Clearing on the outer map clears the nested one too.
    map.clear_map_locked();
    let mut nested = nested_key_map(&map, " BZY");
    assert!(!nested.map_locked());
    nested.put_0c(" BZA", "No Bye", Some(" ")).unwrap();
}

#[test]
fn key_error_is_inherited_by_nested_maps() {
    // main (testkeymap.c:1043-1068), restructured the same way as
    // map_locked_rejects_new_keys and for the same reason.
    let mut map = KeyMap::new();
    map.put_0a(
        " BZY",
        AnyObject::KeyMap(Box::new(KeyMap::new())),
        Some(" "),
    )
    .unwrap();

    // Error GETELEM_12D.
    assert!(!nested_key_map(&map, " BZY").key_error());

    // Error GETELEM_12E.
    map.set_key_error(true);
    let nested = nested_key_map(&map, " BZY");
    assert!(nested.key_error());

    // Error GETELEM_12F: with KeyError inherited, a missing key in the
    // nested map is AST__MPKER rather than a silent failure.
    let err = nested.get_0c("FRED").unwrap_err();
    assert_eq!(err.code(), ErrorCode::Mpker);

    map.clear_key_error();
    let nested = nested_key_map(&map, " BZY");
    assert!(!nested.key_error());
    assert_eq!(nested.get_0c("FRED").unwrap(), None);
}

/// The nested KeyMap stored at `key`, as a clone.
fn nested_key_map(km: &KeyMap, key: &str) -> KeyMap {
    let Some(Some(AnyObject::KeyMap(nested))) = km.get_0a(key).unwrap() else {
        panic!("expected a nested KeyMap at {key}");
    };
    *nested
}

#[test]
fn put_elem_c_appends() {
    // main (testkeymap.c:1070-1081). KeyCmp ignores trailing spaces, so
    // " B " and " B" are the same key — but the leading space is
    // significant in both.
    let mut map = KeyMap::new();
    map.put_0c(" B", "Hello", Some(" ")).unwrap();

    map.put_elem_c(" B ", 2, "YES YES").unwrap();

    // Error GETELEM_13: the scalar was promoted to a vector, and index 2
    // being past its length appended at index 1.
    assert_eq!(map.map_length(" B"), 2);
    // Error GETELEM_14 / GETELEM_15.
    assert_eq!(
        map.get_elem_c(" B ", 1).unwrap().as_deref(),
        Some("YES YES")
    );
}

/// `map1`, holding the three "a" keys `main` puts into it at
/// `testkeymap.c:1091` / `:1163` / `:1219`. The first block's third key is
/// an int64; the other two blocks make it an int.
fn map_copy_source_a(a3_is_int64: bool) -> KeyMap {
    let mut map1 = KeyMap::new();
    map1.put_0i("a1", 1, Some(" ")).unwrap();
    map1.put_0i("a2", 2, Some(" ")).unwrap();
    if a3_is_int64 {
        map1.put_0k("a3", 3, Some(" ")).unwrap();
    } else {
        map1.put_0i("a3", 3, Some(" ")).unwrap();
    }
    map1
}

/// `map`, the mapcopy destination: two scalars plus a nested `map1`.
fn map_copy_dest(a3_is_int64: bool) -> KeyMap {
    let mut map = KeyMap::new();
    map.put_0c("aa1", "Yes", Some(" ")).unwrap();
    map.put_0i("aa2", 2, Some(" ")).unwrap();
    map.put_0a(
        "aa3",
        AnyObject::KeyMap(Box::new(map_copy_source_a(a3_is_int64))),
        Some(" "),
    )
    .unwrap();
    map
}

/// `map2`, the nested KeyMap the mapcopy source holds.
fn map_copy_nested_b() -> KeyMap {
    let mut map2 = KeyMap::new();
    map2.put_0i("b1", 10, Some(" ")).unwrap();
    map2.put_0i("b2", 20, Some(" ")).unwrap();
    map2.put_0i("b3", 30, Some(" ")).unwrap();
    map2
}

#[test]
fn map_copy_merges_nested_key_maps() {
    // main (testkeymap.c:1085-1150). The source's nested KeyMap sits under
    // "bb3", a key the destination does not have, so it is added whole;
    // MapCopy's merge exception needs the same key on both sides.
    let mut map = map_copy_dest(true);

    let mut map3 = KeyMap::new();
    map3.put_0c("bb1", "No", Some(" ")).unwrap();
    map3.put_0i("aa2", 20, Some(" ")).unwrap();
    map3.put_0a(
        "bb3",
        AnyObject::KeyMap(Box::new(map_copy_nested_b())),
        Some(" "),
    )
    .unwrap();

    map.map_copy(&map3).unwrap();

    // Error MAPCOPY_0: three destination keys plus the two new source keys.
    assert_eq!(map.map_size(), 5);

    // Error MAPCOPY_1 / MAPCOPY_2: a key only the destination had survives.
    assert_eq!(map.get_0c("aa1").unwrap().as_deref(), Some("Yes"));
    // Error MAPCOPY_3 / MAPCOPY_4: a key both had takes the source's value.
    assert_eq!(map.get_0i("aa2").unwrap(), Some(20));
    // Error MAPCOPY_5 / MAPCOPY_6.
    let aval = map.get_0a("aa3").unwrap().expect("aa3 is present");
    assert!(matches!(aval, Some(AnyObject::KeyMap(_))));
    // MAPCOPY_7..12 are dead code in the Fortran ancestor, kept dead in C
    // (testkeymap.c:1134-1135), so there is nothing to port.

    // Error MAPCOPY_13 / MAPCOPY_14.
    assert_eq!(map.get_0c("bb1").unwrap().as_deref(), Some("No"));
    // Error MAPCOPY_15 / MAPCOPY_16.
    let aval = map.get_0a("bb3").unwrap().expect("bb3 is present");
    assert!(matches!(aval, Some(AnyObject::KeyMap(_))));
    // MAPCOPY_17..22 dead for the same reason as MAPCOPY_7..12.
}

#[test]
fn map_copy_replaces_a_nested_key_map() {
    // main (testkeymap.c:1152-1206). Every source key is already in the
    // destination, so nothing is added. The scalar entries are replaced,
    // but the nested KeyMaps under the shared "aa3" key hit MapCopy's one
    // exception (MapCopy opens at keymap.c:4112; the pair of
    // astIsAKeyMap tests that select the merge is at :4235) and are
    // merged instead.
    let mut map = map_copy_dest(false);

    let mut map3 = KeyMap::new();
    map3.put_0i("aa1", 0, Some(" ")).unwrap();
    map3.put_0i("aa2", 20, Some(" ")).unwrap();
    map3.put_0a(
        "aa3",
        AnyObject::KeyMap(Box::new(map_copy_nested_b())),
        Some(" "),
    )
    .unwrap();

    map.map_copy(&map3).unwrap();

    // Error MAPCOPY_23: every source key was already present.
    assert_eq!(map.map_size(), 3);

    // Error MAPCOPY_24 / MAPCOPY_25: the string entry was replaced by an
    // int one, so a type change as well as a value change.
    assert_eq!(map.get_0i("aa1").unwrap(), Some(0));
    // Error MAPCOPY_26 / MAPCOPY_27.
    assert_eq!(map.get_0i("aa2").unwrap(), Some(20));
    // Error MAPCOPY_28 / MAPCOPY_29.
    let aval = map.get_0a("aa3").unwrap().expect("aa3 is present");
    let Some(AnyObject::KeyMap(nested)) = aval else {
        panic!("expected a nested KeyMap");
    };
    // MAPCOPY_30..41 are dead code in the ancestor, like MAPCOPY_7..12.
    // The merge is what distinguishes this block from the one above, so
    // check it directly: the nested map holds both sides' keys.
    assert_eq!(nested.map_size(), 6);
    assert_eq!(nested.get_0i("a1").unwrap(), Some(1));
    assert_eq!(nested.get_0i("b3").unwrap(), Some(30));
}

#[test]
fn map_copy_into_a_locked_map_fails() {
    // main (testkeymap.c:1208-1247). The source and destination hold the
    // same three top-level keys, so what fails is the recursive merge of
    // the nested "aa3" KeyMaps: SetMapLocked propagates into nested
    // KeyMaps (keymap.c:9423), and the source's nested b1/b2/b3 are keys
    // the destination's nested a1/a2/a3 does not have.
    let mut map = map_copy_dest(false);

    let mut map3 = KeyMap::new();
    map3.put_0i("aa1", 0, Some(" ")).unwrap();
    map3.put_0i("aa2", 20, Some(" ")).unwrap();
    map3.put_0a(
        "aa3",
        AnyObject::KeyMap(Box::new(map_copy_nested_b())),
        Some(" "),
    )
    .unwrap();

    map.set_map_locked(true);

    // Error MAPCOPY_42.
    let err = map.map_copy(&map3).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Badkey);
}

#[test]
fn undefined_entries() {
    // main (testkeymap.c:1249-1283).
    let mut map = KeyMap::new();
    map.put_u("GG", Some("A comment")).unwrap();

    // Error UNDEF_0.
    assert!(!map.map_defined("GG").unwrap());
    // Error UNDEF_1 / UNDEF_1B / UNDEF_2 / UNDEF_3 / UNDEF_4: every getter
    // reports failure, not an error — the entry is present, it has no value.
    assert_eq!(map.get_0i("GG").unwrap(), None);
    assert_eq!(map.get_0s("GG").unwrap(), None);
    assert_eq!(map.get_0c("GG").unwrap(), None);
    assert!(map.get_0a("GG").unwrap().is_none());
    assert_eq!(map.get_1i("GG").unwrap(), None);
    // Error UNDEF_5: KeyCase defaults to case-sensitive, so "gg" is a
    // missing key rather than the undefined one; with KeyError clear that
    // is also a plain failure rather than an error.
    assert_eq!(map.get_elem_c("gg", 0).unwrap(), None);
    // Error UNDEF_6.
    assert!(map.map_has_key("GG").unwrap());

    // Error UNDEF_7 / UNDEF_8.
    assert_eq!(map.map_type("GG").unwrap(), AstType::Undef);
    assert_eq!(map.map_size(), 1);

    // Error UNDEF_9 / UNDEF_10: a put over the undefined entry defines it.
    map.put_0i("GG", 0, Some(" ")).unwrap();
    assert_eq!(map.get_0i("GG").unwrap(), Some(0));
}

#[test]
fn get_elem_on_an_undefined_entry_returns_none_not_error() {
    // astMapGetElem<X> (keymap.c:6773) sets result = 1 as soon as the key
    // is found and never clears it for AST__UNDEFTYPE: the "} else if(
    // raw )" guard (keymap.c:6910) just skips the conversion, so C reports
    // success without writing to its own output parameter. There is no
    // value for an Option to carry in that case, so every in-range index
    // reads back as Ok(None) here instead, matching get_0i and get_1i.
    let mut map = KeyMap::new();
    map.put_u("GG", Some("A comment")).unwrap();

    assert_eq!(map.get_elem_i("GG", 0).unwrap(), None);
    assert_eq!(map.get_elem_c("GG", 0).unwrap(), None);
    assert!(map.get_elem_a("GG", 0).unwrap().is_none());

    // The out-of-range check (keymap.c:6901) runs before the raw-pointer
    // guard, so it still fires for an undefined entry.
    let err = map.get_elem_i("GG", 1).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Mpvin);
}

#[test]
fn rename_round_trip() {
    // main (testkeymap.c:1285-1302).
    let mut map = KeyMap::new();
    map.put_0i("GG", 0, Some(" ")).unwrap();

    map.map_rename("GG", "GGNEW").unwrap();
    // Error RENAME_1 / RENAME_2 / RENAME_3.
    assert!(!map.map_has_key("GG").unwrap());
    assert_eq!(map.get_0i("GGNEW").unwrap(), Some(0));

    map.map_rename("GGNEW", "GG").unwrap();
    // Error RENAME_4 / RENAME_5 / RENAME_6.
    assert!(!map.map_has_key("GGNEW").unwrap());
    assert_eq!(map.get_0i("GG").unwrap(), Some(0));
}

#[test]
fn map_copy_entry() {
    // main (testkeymap.c:1304-1350), with merge false.
    let mut map = map_copy_dest(false);

    let mut map3 = KeyMap::new();
    map3.put_0c("bb1", "No", Some(" ")).unwrap();
    map3.put_0i("aa2", 20, Some(" ")).unwrap();
    map3.put_0a(
        "aa3",
        AnyObject::KeyMap(Box::new(map_copy_nested_b())),
        Some(" "),
    )
    .unwrap();

    map.map_copy_entry("bb1", &map3, false).unwrap();
    // Error MAPCOPYENTRY_1: one key added, and only that one.
    assert_eq!(map.map_size(), 4);
    // Error MAPCOPYENTRY_2 / MAPCOPYENTRY_3.
    assert_eq!(map.get_0c("bb1").unwrap().as_deref(), Some("No"));

    map.map_copy_entry("aa3", &map3, false).unwrap();
    // Error MAPCOPYENTRY_4.
    let aval = map.get_0a("aa3").unwrap().expect("aa3 is present");
    let Some(AnyObject::KeyMap(nested)) = aval else {
        panic!("expected a nested KeyMap");
    };
    // Error MAPCOPYENTRY_5 / MAPCOPYENTRY_6: the source's nested map is
    // there in full.
    assert_eq!(nested.get_0i("b3").unwrap(), Some(30));
    // Error MAPCOPYENTRY_7: with merge false the destination's own nested
    // map was replaced, not merged, so its keys are gone.
    assert_eq!(nested.get_0i("a1").unwrap(), None);
}

#[test]
fn put_elem_d_appends_five_times() {
    // main (testkeymap.c:1352-1362). C passes -1 each time; any index at
    // or past the length appends (keymap.c:8392), and put_elem_d takes a
    // usize, so pass the length. The first call creates the entry.
    let mut map = KeyMap::new();
    for value in [1.0, 2.0, 3.0, 4.0, 15.0] {
        let n = map.map_length("HHH");
        map.put_elem_d("HHH", n, value).unwrap();
    }
    // Error MAPPUTELEM_7.
    assert_eq!(map.map_length("HHH"), 5);
    assert_eq!(
        map.get_1d("HHH").unwrap(),
        Some(vec![1.0, 2.0, 3.0, 4.0, 15.0])
    );
}

#[test]
fn int64_edge_values() {
    // main (testkeymap.c:1364-1407). The two seed values are outside the
    // int range, so a KInt entry is the only one that can hold them.
    let mut map = KeyMap::new();
    map.put_0d("asa", 0.0, Some(" ")).unwrap();
    map.put_0f("asar", 1.0, Some(" ")).unwrap();
    map.put_0i("dfdfd", 1, Some(" ")).unwrap();

    let kvec = [-i64::from(i32::MAX), 10 * i64::from(i32::MIN)];
    map.put_1k("AAA", &kvec, Some("com 1")).unwrap();
    // C appends with index -1; put_elem_* takes a usize, so pass the
    // length. Both appends convert to the entry's own KInt type.
    let n = map.map_length("AAA");
    map.put_elem_i("AAA", n, 1).unwrap();
    let n = map.map_length("AAA");
    map.put_elem_k("AAA", n, 1).unwrap();
    // Error I8_1.
    assert_eq!(map.map_length("AAA"), 4);

    map.put_0c("bb1", "No", Some(" ")).unwrap();

    // Error I8_2 / I8_3.
    assert_eq!(
        map.get_elem_k("AAA", 1).unwrap(),
        Some(10 * i64::from(i32::MIN))
    );

    // Error I8_4 / I8_5 / I8_6.
    assert_eq!(
        map.get_1k("AAA").unwrap(),
        Some(vec![-i64::from(i32::MAX), 10 * i64::from(i32::MIN), 1, 1])
    );
}

// ===========================================================================
// Tests added upstream alongside the KeyMap defect fixes in the `f6762005`
// sync. Each is named for the C function it comes from.
// ===========================================================================

/// `testdumpstable` — a dump must be byte stable, both across a write / read /
/// write cycle and against the order the entries were added.
///
/// C's keys are chosen to collide in the default-sized hash table, since only
/// a chained set can expose an add-order dependency. This port keys off a
/// `HashMap` and sorts at dump time, so no key set can expose one — the case
/// is kept because it is the property that matters, not because the mechanism
/// is shared.
#[test]
fn dump_is_stable_against_insertion_order() {
    let keys = ["DELTA", "EPSILON", "ETA", "ALPHA", "BETA", "GAMMA"];
    let order = [4usize, 2, 5, 0, 3, 1];

    let mut km1 = KeyMap::new();
    for (i, k) in keys.iter().enumerate() {
        km1.put_0i(k, i as i32 + 1, Some(" ")).unwrap();
    }
    let dump1 = to_native_string(&AnyObject::KeyMap(Box::new(km1))).unwrap();

    // Error dumpstable-2.
    let reload = from_native_string(&dump1).unwrap();
    let dump2 = to_native_string(&reload).unwrap();
    assert_eq!(dump1, dump2, "dump changed on a write, read, write cycle");

    // Error dumpstable-3.
    let mut km2 = KeyMap::new();
    for &i in &order {
        km2.put_0i(keys[i], i as i32 + 1, Some(" ")).unwrap();
    }
    let dump3 = to_native_string(&AnyObject::KeyMap(Box::new(km2))).unwrap();
    assert_eq!(
        dump1, dump3,
        "dump depends on the order the entries were added"
    );
}

/// `testiterate` — iteration must terminate on a KeyMap with `SortBy` set.
/// C's sorted list is circular, so a walk that does not detect the wrap runs
/// forever. The bound below makes a regression a failure rather than a hang.
#[test]
fn iteration_terminates_with_sortby_set() {
    let mut km = KeyMap::new();
    km.set_sort_by(SortBy::KeyUp);
    km.put_0i("AAA", 1, Some(" ")).unwrap();
    km.put_0i("BBB", 2, Some(" ")).unwrap();
    km.put_0i("CCC", 3, Some(" ")).unwrap();

    let mut seen = Vec::new();
    let mut key = km.map_iterate(true);
    while let Some(k) = key {
        seen.push(k);
        if seen.len() >= 100 {
            break;
        }
        key = km.map_iterate(false);
    }
    assert_eq!(seen, ["AAA", "BBB", "CCC"], "iteration did not terminate");
}

/// `testrenamelocked` — a rename refused because the KeyMap is locked must
/// leave it unchanged, not consume the entry.
#[test]
fn a_refused_rename_keeps_the_entry() {
    let mut km = KeyMap::new();
    km.put_0i("KEEP", 42, Some(" ")).unwrap();
    km.set_map_locked(true);

    // Error rename-locked-noerr: NEWKEY is not already a known key.
    assert!(km.map_rename("KEEP", "NEWKEY").is_err());

    // Errors rename-locked-lost / -unreadable / -value / -newkey.
    assert!(km.map_has_key("KEEP").unwrap());
    assert_eq!(km.get_0i("KEEP").unwrap(), Some(42));
    assert!(!km.map_has_key("NEWKEY").unwrap());
}

/// `testconvrange` — converting an out-of-range double to a narrow integer
/// must saturate at the type's limit, and a NaN must give a defined zero,
/// rather than performing an undefined cast.
#[test]
fn numeric_conversion_saturates_and_defines_nan() {
    let mut km = KeyMap::new();
    km.put_0d("BIG", 5_000_000_100.0, Some(" ")).unwrap();
    km.put_0d("NEG", -5_000_000_100.0, Some(" ")).unwrap();
    km.put_0d("NAN", f64::NAN, Some(" ")).unwrap();

    // Errors range-int-max / range-int-min / range-short-max / range-byte-max.
    assert_eq!(km.get_0i("BIG").unwrap(), Some(i32::MAX));
    assert_eq!(km.get_0i("NEG").unwrap(), Some(i32::MIN));
    assert_eq!(km.get_0s("BIG").unwrap(), Some(i16::MAX));
    assert_eq!(km.get_0b("BIG").unwrap(), Some(u8::MAX));

    // Error range-byte-min: a negative value saturates at zero for an
    // unsigned type rather than becoming a large positive one.
    assert_eq!(km.get_0b("NEG").unwrap(), Some(0));

    // Errors range-nan-int / -short / -int64 / -byte.
    assert_eq!(km.get_0i("NAN").unwrap(), Some(0));
    assert_eq!(km.get_0s("NAN").unwrap(), Some(0));
    assert_eq!(km.get_0k("NAN").unwrap(), Some(0));
    assert_eq!(km.get_0b("NAN").unwrap(), Some(0));
}

/// `testconvstring` — a numeric string too large for the target type must
/// saturate rather than wrap, and a string that is not a number at all is
/// still refused.
#[test]
fn numeric_string_conversion_saturates() {
    let mut km = KeyMap::new();
    km.put_0c("OVER", "9999999999", Some(" ")).unwrap();
    km.put_0c("HUGE", "99999999999999999999999999999999999999", Some(" "))
        .unwrap();
    km.put_0c("OK", "42", Some(" ")).unwrap();
    km.put_0c("OKSPACE", " 42 ", Some(" ")).unwrap();
    km.put_0c("BLANK", "   ", Some(" ")).unwrap();
    km.put_0c("TRAIL", "12abc", Some(" ")).unwrap();

    // Errors convstr-ok / convstr-okspace.
    assert_eq!(km.get_0i("OK").unwrap(), Some(42));
    assert_eq!(km.get_0i("OKSPACE").unwrap(), Some(42));

    // Errors convstr-blank / convstr-trail: not a number, so refused.
    assert!(km.get_0i("BLANK").is_err());
    assert!(km.get_0i("TRAIL").is_err());

    // Errors convstr-over / convstr-huge: magnitude overflow saturates.
    assert_eq!(km.get_0i("OVER").unwrap(), Some(i32::MAX));
    assert_eq!(km.get_0i("HUGE").unwrap(), Some(i32::MAX));
}

/// `testemptyvector` — a zero-length vector has no representation in an entry,
/// where a length of zero means "scalar". The put must be refused rather than
/// creating an entry that cannot be dumped.
#[test]
fn a_zero_length_vector_is_refused() {
    let mut km = KeyMap::new();

    // Errors emptyvec-int-noerr / -str-noerr / -obj-noerr.
    assert!(km.put_1i("EMPTYI", &[], Some(" ")).is_err());
    assert!(km.put_1c("EMPTYC", &[], Some(" ")).is_err());
    assert!(km.put_1a("EMPTYA", Vec::new(), Some(" ")).is_err());

    // Errors emptyvec-int-present / -str-present / -obj-present.
    assert!(!km.map_has_key("EMPTYI").unwrap());
    assert!(!km.map_has_key("EMPTYC").unwrap());
    assert!(!km.map_has_key("EMPTYA").unwrap());

    // Error emptyvec-dump: the KeyMap must still dump.
    km.put_0i("REAL", 7, Some(" ")).unwrap();
    to_native_string(&AnyObject::KeyMap(Box::new(km))).unwrap();
}

/// `testundefelem` — the element accessors must report failure for an
/// undefined entry, as the scalar and vector accessors already do, and an
/// out-of-range index must still be reported as a bad index.
#[test]
fn element_accessors_report_an_undefined_entry() {
    let mut km = KeyMap::new();
    km.put_u("UNDEF", Some(" ")).unwrap();

    // Error undef-get0: the scalar accessor already reports it.
    assert_eq!(km.get_0i("UNDEF").unwrap(), None);

    // Errors undef-getelem-i / -c / -a.
    assert_eq!(km.get_elem_i("UNDEF", 0).unwrap(), None);
    assert_eq!(km.get_elem_c("UNDEF", 0).unwrap(), None);
    assert!(km.get_elem_a("UNDEF", 0).unwrap().is_none());

    // Error undef-getelem-i-oob: an out-of-range index on an undefined entry
    // is a bad index, not silently folded into the "no value" case.
    let err = km.get_elem_i("UNDEF", 5).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Mpvin);
}

/// `testlockedroundtrip` — a locked, non-empty KeyMap must survive a dump and
/// reload. The lock has to be applied after the entries are read, or every put
/// during the load is refused.
#[test]
fn a_locked_keymap_survives_a_round_trip() {
    let mut km = KeyMap::new();
    km.put_0i("AAA", 1, Some(" ")).unwrap();
    km.put_0i("BBB", 2, Some(" ")).unwrap();

    let mut nested = KeyMap::new();
    nested.put_0i("INNER", 3, Some(" ")).unwrap();
    km.put_0a("NEST", AnyObject::KeyMap(Box::new(nested)), Some(" "))
        .unwrap();

    km.set_map_locked(true);
    let dump = to_native_string(&AnyObject::KeyMap(Box::new(km))).unwrap();

    // Error locked-reload.
    let reloaded = from_native_string(&dump).unwrap();
    let AnyObject::KeyMap(km2) = reloaded else {
        panic!("reload did not produce a KeyMap");
    };

    // Errors locked-reload-aaa / -flag.
    assert_eq!(km2.get_0i("AAA").unwrap(), Some(1));
    assert!(km2.map_locked(), "MapLocked was lost over the round trip");

    // Error locked-reload-nestflag: the flag must reach nested KeyMaps too.
    let nested = km2.get_0a("NEST").unwrap().flatten();
    let Some(AnyObject::KeyMap(inner)) = nested else {
        panic!("the nested KeyMap did not survive the round trip");
    };
    assert!(
        inner.map_locked(),
        "MapLocked did not reach the nested KeyMap"
    );
}

/// `testcopyentrycase` — a copy must express the copied key in the
/// *destination's* `KeyCase`, both when searching the destination and when
/// storing into it. `KeyCase` can only be changed while a KeyMap is empty, so
/// every key a KeyMap holds should be in that KeyMap's own casing.
#[test]
fn a_copied_key_takes_the_destinations_key_case() {
    let mut src = KeyMap::new();
    src.set_key_case(true).unwrap();
    let mut dst = KeyMap::new();
    dst.set_key_case(false).unwrap();

    src.put_0i("MixedKey", 11, Some(" ")).unwrap();
    // The destination already holds the key under its own folded form.
    dst.put_0i("MIXEDKEY", 99, Some(" ")).unwrap();

    dst.map_copy_entry("MixedKey", &src, false).unwrap();

    // Error copyentry-dup: no second, differently cased copy of one key.
    assert_eq!(dst.map_size(), 1);
    // The copied value is retrievable under the destination's own casing.
    assert_eq!(dst.get_0i("MIXEDKEY").unwrap(), Some(11));
    // And the key as stored is the destination's folded form.
    assert_eq!(dst.map_key(0).unwrap(), "MIXEDKEY");

    // The bulk copy path shares the same helper and has to agree.
    let mut bulk = KeyMap::new();
    bulk.set_key_case(false).unwrap();
    bulk.map_copy(&src).unwrap();
    assert_eq!(bulk.map_key(0).unwrap(), "MIXEDKEY");
    assert_eq!(bulk.get_0i("MIXEDKEY").unwrap(), Some(11));
}
