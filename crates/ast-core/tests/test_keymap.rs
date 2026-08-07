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
