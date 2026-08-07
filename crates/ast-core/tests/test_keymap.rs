//! Rust port of `reference/starlink-ast/ast_tester/testkeymap.c`.
//!
//! The C original drives the KeyMap through the protected `astMapPut*` /
//! `astMapGet*` API and checks each value with `astMapGet0C`. This port
//! uses the public Rust API only, so it doubles as a worked example.
//!
//! Divergences from the C original are noted at each site. The
//! `astMapPutP` / `astMapGetP` family has no Rust counterpart, because
//! `AST__POINTERTYPE` holds a raw C pointer.

use ast_core::keymap::{AstType, KeyMap, SortBy};
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
fn a_key_map_cannot_contain_itself_transitively() {
    // The direct check above catches a candidate that IS a copy of the
    // receiver, at check_circle's own same_entries test — it never
    // reaches the recursive walk over the candidate's nested KeyMaps.
    // This test puts the matching KeyMap two Object-entry levels below
    // the candidate, inside wrappers that do not themselves match the
    // receiver, so only the recursive branch (keymap.c:951's transitive
    // CheckCircle, walking the candidate's own contents) can find it.
    let mut victim = KeyMap::new();
    victim.put_0i("X", 1, None).unwrap();

    // A structural copy of the receiver, buried inside a chain of
    // wrappers whose own key sets differ from the receiver's.
    let mut level2 = KeyMap::new();
    level2
        .put_0a("MATCH", AnyObject::KeyMap(Box::new(victim.clone())), None)
        .unwrap();
    let mut candidate = KeyMap::new();
    candidate.put_0i("EXTRA", 99, None).unwrap();
    candidate
        .put_0a("NESTED", AnyObject::KeyMap(Box::new(level2)), None)
        .unwrap();

    let candidate_obj = AnyObject::KeyMap(Box::new(candidate));
    let err = victim
        .check_circle(&candidate_obj, "astMapPut0A")
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::Kycir);
    assert!(
        err.to_string()
            .contains("the first KeyMap contains the second KeyMap"),
        "{err}"
    );
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
