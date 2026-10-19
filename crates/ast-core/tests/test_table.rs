//! Table tests, ported from `reference/starlink-ast/ast_tester/testtable.c`.
//!
//! Divergences from the C test:
//!
//! - C stores a Table in one of its own cells to check the circularity
//!   error (`AST__KYCIR`). A Rust value cannot hold itself, so that check
//!   has no counterpart.
//! - `checkDump` compares the class, `Nrow` and every entry of the Table
//!   read back from its native dump, as C does.
//! - C's `astColumnShape` pads the shape with ones to the length of the
//!   caller's array; `column_shape` returns the shape alone.

use crate::common::native_dump;
use ast_core::error::ErrorCode;
use ast_core::{AnyObject, AstType, Table, from_native_string};

/// Write `table` to its native dump and read it back, checking that the
/// result is a Table with the same number of rows and the same entries.
fn check_dump(table: &Table) {
    let text = native_dump(&AnyObject::Table(Box::new(table.clone()))).unwrap();
    let AnyObject::Table(result) = from_native_string(&text).unwrap() else {
        panic!("object read from the dump is not a Table");
    };
    assert_eq!(result.nrow(), table.nrow(), "checkDump 0");
    let (old, new) = (table.as_keymap(), result.as_keymap());
    assert_eq!(new.map_size(), old.map_size(), "checkDump 1");
    for i in 0..new.map_size() {
        let key = new.map_key(i).unwrap();
        let ty = new.map_type(&key).unwrap();
        assert_eq!(old.map_type(&key).unwrap(), ty, "checkDump 4");
        if ty == AstType::Object {
            assert!(new.get_0a(&key).unwrap().is_some(), "checkDump 5");
            assert!(old.get_0a(&key).unwrap().is_some(), "checkDump 6");
        } else {
            let a = new.get_0c(&key).unwrap().expect("checkDump 8");
            let b = old.get_0c(&key).unwrap().expect("checkDump 9");
            assert_eq!(a, b, "checkDump 10");
        }
    }
}

/// testtable.c `checkpurge`.
fn check_purge() {
    let mut table = Table::new();
    table.add_column("Fred", AstType::Int, &[], " ").unwrap();
    table.add_column("Tom", AstType::Int, &[], " ").unwrap();
    table.put_0i("Fred(2)", 123, Some(" ")).unwrap();
    table.put_0i("Fred(4)", 456, Some(" ")).unwrap();
    table.put_0i("Tom(1)", -123, Some(" ")).unwrap();
    table.put_0i("Tom(2)", -456, Some(" ")).unwrap();
    table.put_0i("Tom(6)", 0, Some(" ")).unwrap();

    // A parameter value is in no row, and must not stop the purge.
    table.add_parameter("Colour").unwrap();
    table.put_0c("Colour", "Red", Some(" ")).unwrap();
    assert_eq!(table.nrow(), 6, "purge-1");

    table.map_remove("Tom(6)").unwrap();
    assert_eq!(table.nrow(), 6, "purge-2");

    table.purge_rows().expect("purge-13");
    assert_eq!(table.nrow(), 3, "purge-3");
    assert_eq!(table.get_0i("Tom(1)").unwrap(), Some(-123), "purge-4");
    assert_eq!(table.get_0i("Tom(2)").unwrap(), Some(-456), "purge-6");
    assert_eq!(table.get_0i("Fred(1)").unwrap(), None, "purge-8");
    assert_eq!(table.get_0i("Fred(2)").unwrap(), Some(123), "purge-9");
    assert_eq!(table.get_0i("Fred(3)").unwrap(), Some(456), "purge-11");
    assert_eq!(
        table.get_0c("Colour").unwrap().as_deref(),
        Some("Red"),
        "purge-14"
    );
}

#[test]
fn testtable() {
    let mut table = Table::new();

    // No column called Fred yet.
    assert_eq!(
        table.put_0i("Fred", 123, Some("com 1")).unwrap_err().code(),
        ErrorCode::Badkey,
        "1"
    );
    assert_eq!(
        table
            .put_0i("Fred(2)", 123, Some("com 1"))
            .unwrap_err()
            .code(),
        ErrorCode::Badkey,
        "2"
    );

    table
        .add_column("Fred", AstType::Float, &[5, 2], " ")
        .unwrap();
    assert_eq!(table.ncolumn(), 1, "2b");
    assert_eq!(table.column_name(1).unwrap(), "FRED", "2c");
    assert_eq!(table.column_type("Fred").unwrap(), AstType::Float, "2d");
    assert_eq!(table.column_shape("Fred").unwrap(), vec![5, 2], "2e-2g");
    assert_eq!(table.column_unit("Fred").unwrap(), " ", "2h");

    // Wrong type, and a scalar for a column of arrays.
    assert_eq!(
        table
            .put_0i("Fred(2)", 123, Some("com 1"))
            .unwrap_err()
            .code(),
        ErrorCode::Badtyp,
        "3"
    );
    assert_eq!(
        table
            .put_0f("Fred(2)", 123.0, Some("com 1"))
            .unwrap_err()
            .code(),
        ErrorCode::Badtyp,
        "4"
    );

    // A column of the same name with a different shape and unit.
    assert_eq!(
        table
            .add_column("Fred", AstType::Float, &[], "pW")
            .unwrap_err()
            .code(),
        ErrorCode::Oldcol,
        "5"
    );

    table.remove_column("Fred").unwrap();
    assert_eq!(table.ncolumn(), 0, "5b");

    table.add_column("Fred", AstType::Float, &[], "pW").unwrap();
    table.put_0f("Fred(1)", -123.0, Some("com 1")).unwrap();
    table.put_0f("Fred(2)", 123.0, Some("com 2")).unwrap();
    assert_eq!(table.get_0f("Fred(2)").unwrap(), Some(123.0), "6-7");
    assert_eq!(table.column_unit("Fred").unwrap(), "pW", "7b");

    table
        .add_column("Dick", AstType::Object, &[], "W/m**2")
        .unwrap();
    assert_eq!(table.ncolumn(), 2, "8b");
    assert_eq!(table.column_name(1).unwrap(), "FRED", "8c");
    assert_eq!(table.column_name(2).unwrap(), "DICK", "8d");

    table.remove_column("Dick").unwrap();
    assert_eq!(table.nrow(), 2, "9");
    assert_eq!(table.get_0f("Fred(3)").unwrap(), None, "10");
    assert_eq!(table.get_0i("Fred(2)").unwrap(), Some(123), "11-12");
    assert_eq!(
        table.get_0c("Fred(2)").unwrap().as_deref(),
        Some("123"),
        "13-15"
    );

    table.remove_row(3).unwrap();
    assert_eq!(table.nrow(), 2, "16");
    table.remove_row(2).unwrap();
    assert_eq!(table.nrow(), 1, "17");
    assert_eq!(table.get_0f("Fred(2)").unwrap(), None, "18");

    table.add_parameter("COLOUR").unwrap();
    assert!(table.has_parameter("COLOUR").unwrap(), "18_1");
    table.put_0c("COLOUR", "Red", Some(" ")).unwrap();
    assert_eq!(
        table.get_0c("COLOUR").unwrap().as_deref(),
        Some("Red"),
        "18_3-18_5"
    );
    table.remove_parameter("COLOUR").unwrap();
    assert!(!table.has_parameter("COLOUR").unwrap(), "18_2");

    check_dump(&table);

    let table2 = table.clone();
    assert_eq!(
        table2.get_0c("Fred(1)").unwrap().as_deref(),
        Some("-123"),
        "19-21"
    );

    table
        .add_column("Dick", AstType::String, &[2, 2], " ")
        .unwrap();
    table
        .put_1c("Dick(4)", &["One", "two", "three", "FouR"], Some("jjjj"))
        .unwrap();
    assert_eq!(
        table.get_1c("Dick(4)").unwrap(),
        Some(vec![
            "One".to_string(),
            "two".into(),
            "three".into(),
            "FouR".into()
        ]),
        "22-27"
    );
    table.put_elem_c("Dick(4)", 3, "OHOHOHOH").unwrap();
    assert_eq!(
        table.get_elem_c("Dick(4)", 3).unwrap().as_deref(),
        Some("OHOHOHOH"),
        "28-29"
    );
    assert_eq!(table.column_len_c("Dick").unwrap(), 8, "29b");

    table
        .add_column("HeHe", AstType::Byte, &[1, 2], " ")
        .unwrap();
    table.put_1b("HeHe(2)", &[127, 255], Some("jjjj")).unwrap();
    assert_eq!(
        table.get_1b("HeHe(2)").unwrap(),
        Some(vec![127, 255]),
        "30-33"
    );

    table.add_column("GoGo", AstType::Byte, &[], " ").unwrap();
    table.put_0b("GoGo(2)", (-10i8) as u8, Some(" ")).unwrap();
    assert_eq!(
        table.get_0b("GoGo(2)").unwrap().map(|b| b as i8),
        Some(-10),
        "33-34"
    );

    check_purge();

    assert!(table.has_column("GoGo").unwrap(), "35");
    assert!(!table.has_column("dodo").unwrap(), "36");
    table.remove_column("GoGo").unwrap();
    assert!(!table.has_column("GoGo").unwrap(), "37");

    table.add_column("FredK", AstType::KInt, &[2], " ").unwrap();
    table.put_1k("FredK(1)", &[1, -1], Some(" ")).unwrap();
    assert_eq!(
        table.get_1k("FredK(1)").unwrap(),
        Some(vec![1, -1]),
        "38-41"
    );
}
