//! Rust port of `reference/starlink-ast/ast_tester/testkeymap.c`.
//!
//! The C original drives the KeyMap through the protected `astMapPut*` /
//! `astMapGet*` API and checks each value with `astMapGet0C`. This port
//! uses the public Rust API only, so it doubles as a worked example.
//!
//! Divergences from the C original are noted at each site. The
//! `astMapPutP` / `astMapGetP` family has no Rust counterpart, because
//! `AST__POINTERTYPE` holds a raw C pointer.

use ast_core::keymap::KeyMap;
use ast_core::{AnyObject, error::ErrorCode};

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
fn a_key_map_cannot_contain_itself() {
    // testkeymap.c checks AST__KYCIR for both the direct and the
    // transitive case.
    let mut inner = KeyMap::new();
    inner.put_0i("X", 1, None).unwrap();
    let mut outer = KeyMap::new();
    outer
        .put_0a("INNER", AnyObject::KeyMap(Box::new(inner)), None)
        .unwrap();

    let self_copy = AnyObject::KeyMap(Box::new(outer.clone()));
    let err = outer.check_circle(&self_copy, "astMapPut0A").unwrap_err();
    assert_eq!(err.code(), ErrorCode::Kycir);
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

    // Comment retention/dropping on create-vs-promote is checked once
    // `map_comment` and `put_u` land in Task 9.
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
