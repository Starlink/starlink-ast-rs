//! FitsTable tests, ported from
//! `reference/starlink-ast/ast_tester/testfitstable.c`.
//!
//! Divergences from the C test:
//!
//! - C passes column data as untyped memory sized in bytes; here it is a
//!   [`ColumnData`] of the column's own type. C's `astColumnSize` is
//!   still checked, as the number of bytes C needs.
//! - C reads a cell into a buffer of the column's type; here a missing
//!   cell is `None`.

use starlink_ast::error::ErrorCode;
use starlink_ast::{AstType, ColumnData, FitsChan, FitsTable};

/// The header of the FitsTable once its columns are added.
const HEADER1: [&str; 18] = [
    "XTENSION= 'BINTABLE'",
    "BITPIX  =                    8",
    "NAXIS   =                    2",
    "NAXIS1  =                   14",
    "NAXIS2  =                    0",
    "PCOUNT  =                    0",
    "GCOUNT  =                    1",
    "TFIELDS =                    3",
    "TFORM1  = '6B      '",
    "TTYPE1  = 'BYTECOL '",
    "TUNIT1  = 'ADU     '",
    "TDIM1   = '(2,3)   '",
    "TFORM2  = '1K      '",
    "TTYPE2  = 'KINTCOL '",
    "TUNIT2  = 'm       '",
    "TFORM3  = '0A      '",
    "TTYPE3  = 'STRINGCOL'",
    "TDIM3   = '(0,3)   '",
];

/// The header of the FitsTable once its rows are filled. Cards 4, 18 and
/// 20 depend on the length of the longest string, and are checked
/// separately.
const HEADER2: [&str; 20] = [
    "XTENSION= 'BINTABLE'",
    "BITPIX  =                    8",
    "NAXIS   =                    2",
    "NAXIS1  =                   44",
    "NAXIS2  =                    3",
    "PCOUNT  =                    0",
    "GCOUNT  =                    1",
    "TFIELDS =                    3",
    "TFORM1  = '6B      '",
    "TTYPE1  = 'BYTECOL '",
    "TUNIT1  = 'ADU     '",
    "TNULL1  =                  254",
    "TDIM1   = '(2,3)   '",
    "TFORM2  = '1K      '",
    "TTYPE2  = 'KINTCOL '",
    "TUNIT2  = 'm       '",
    "TNULL2  =  9223372036854775807",
    "TFORM3  = '30A     '",
    "TTYPE3  = 'STRINGCOL'",
    "TDIM3   = '(10,3)  '",
];

/// Every card of `header`, from the first.
fn header_cards(header: &mut FitsChan) -> Vec<String> {
    header.clear_card().unwrap();
    let mut cards = Vec::new();
    while let Some(card) = header.find_fits("%f", true).unwrap() {
        cards.push(card);
    }
    cards
}

/// A FitsChan holding `cards`.
fn header_of(cards: &[&str]) -> FitsChan {
    let mut fc = FitsChan::new().unwrap();
    for card in cards {
        fc.put_fits(card, false).unwrap();
    }
    fc
}

#[test]
fn testfitstable() {
    // An empty FitsTable has the default header.
    let mut table = FitsTable::new().unwrap();
    let mut header = table.get_table_header().unwrap();
    assert_eq!(header.ncard().unwrap(), 8, "error 1");
    assert_eq!(
        header.get_fits_i(Some("NAXIS")).unwrap(),
        Some(2),
        "error 2-3"
    );
    assert_eq!(
        header.get_fits_i(Some("NAXIS1")).unwrap(),
        Some(0),
        "error 4-5"
    );
    assert_eq!(
        header.get_fits_i(Some("NAXIS2")).unwrap(),
        Some(0),
        "error 6-7"
    );

    // A FitsTable cannot hold Objects.
    assert_eq!(
        table
            .add_column("JUNK", AstType::Object, &[], "m")
            .unwrap_err()
            .code(),
        ErrorCode::Naxin,
        "error 8"
    );

    table
        .add_column("BYTECOL", AstType::Byte, &[2, 3], "ADU")
        .unwrap();
    table
        .add_column("KINTCOL", AstType::KInt, &[], "m")
        .unwrap();
    table
        .add_column("STRINGCOL", AstType::String, &[3], " ")
        .unwrap();

    let mut header = table.get_table_header().unwrap();
    let cards = header_cards(&mut header);
    assert_eq!(cards.len(), HEADER1.len(), "error 9/11");
    for (card, want) in cards.iter().zip(HEADER1) {
        assert!(card.starts_with(want), "error 10: {card} is not {want}");
    }

    // A FitsTable created from that header has the same columns.
    let mut table2 = FitsTable::from_header(&mut header).unwrap();
    let t2 = table2.as_table();
    assert_eq!(t2.ncolumn(), 3, "error 11a");
    assert_eq!(t2.column_length("bytecol").unwrap(), 6, "error 11b");
    assert_eq!(t2.column_ndim("bytecol").unwrap(), 2, "error 11c");
    assert_eq!(
        t2.column_type("bytecol").unwrap(),
        AstType::Byte,
        "error 11d"
    );
    assert_eq!(t2.column_unit("bytecol").unwrap(), "ADU", "error 11e");
    assert_eq!(t2.column_length("kintcol").unwrap(), 1, "error 11f");
    assert_eq!(t2.column_ndim("kintcol").unwrap(), 0, "error 11g");
    assert_eq!(
        t2.column_type("kintcol").unwrap(),
        AstType::KInt,
        "error 11h"
    );
    assert_eq!(t2.column_unit("kintcol").unwrap(), "m", "error 11i");
    assert_eq!(t2.column_length("StringCol").unwrap(), 3, "error 11j");
    assert_eq!(t2.column_ndim("StringCol").unwrap(), 1, "error 11k");
    assert_eq!(
        t2.column_type("StringCol").unwrap(),
        AstType::String,
        "error 11l"
    );

    // Fill the rows.
    table
        .put_1b("BYTECOL(1)", &[0, 1, 128, 127, 129, 255], Some(" "))
        .unwrap();
    table
        .put_1b("BYTECOL(2)", &[0, 1, 0, 1, 0, 1], Some(" "))
        .unwrap();
    table.put_0k("KINTCOL(2)", 10, Some(" ")).unwrap();
    table.put_0k("KINTCOL(3)", -10, Some(" ")).unwrap();
    table
        .put_1c("STRINGCOL(1)", &["hello", " ", "goodbye"], Some(" "))
        .unwrap();
    table
        .put_1c("STRINGCOL(3)", &[" ", " ", " "], Some(" "))
        .unwrap();
    assert_eq!(table.as_table().nrow(), 3, "error 12");
    assert_eq!(table.as_table().ncolumn(), 3, "error 13");

    // The string column survives a copy into the other FitsTable.
    let clen = table.as_table().column_len_c("StringCol").unwrap();
    assert_eq!(clen, 7, "ColumnLenC(StringCol)");
    assert_eq!(table.column_size("stringcol").unwrap(), 3 * 3 * clen);
    let strings = table.get_column_data("StringCol", 0.0, 0.0, None).unwrap();
    table2.put_column_data("StringCol", &strings).unwrap();
    let strings2 = table2.get_column_data("StringCol", 0.0, 0.0, None).unwrap();
    assert_eq!(strings2.nelem(), strings.nelem(), "error 13d");

    // The byte column, with null values in the missing row.
    assert_eq!(table.column_size("bytecol").unwrap(), 18, "error 13e");
    let bytes = table.get_column_data("BYTECOL", 0.0, 0.0, None).unwrap();
    assert_eq!(bytes.nelem(), 18, "error 13f");
    let null = table.column_null("BYTECOL", None, true).unwrap().value;
    let ColumnData::Byte(vals) = &bytes else {
        panic!("BYTECOL does not hold bytes");
    };
    assert_eq!(
        &vals[..12],
        &[0, 1, 128, 127, 129, 255, 0, 1, 0, 1, 0, 1],
        "error checkbytes 1"
    );
    assert!(
        vals[12..].iter().all(|&v| i64::from(v) == null),
        "error checkbytes 2"
    );
    table2.put_column_data("byteCol", &bytes).unwrap();
    table2.column_null("BYTECOL", Some(null), true).unwrap();
    let bytes2 = table2.get_column_data("BYTECOL", 0.0, 0.0, None).unwrap();
    assert_eq!(bytes2.nelem(), 18, "error 13g");

    // The 64-bit integer column.
    assert_eq!(table.column_size("kintcol").unwrap(), 24, "error 13h");
    let kints = table.get_column_data("KINTCOL", 0.0, 0.0, None).unwrap();
    assert_eq!(kints.nelem(), 3, "error 13i");
    let null = table.column_null("KINTCOL", None, true).unwrap().value;
    assert_eq!(
        kints,
        ColumnData::KInt(vec![null, 10, -10]),
        "error checkints"
    );
    table2.put_column_data("KINTCol", &kints).unwrap();
    let kints2 = table2.get_column_data("KINTCOL", 0.0, 0.0, None).unwrap();
    assert_eq!(kints2.nelem(), 3, "error 13j");

    // A float column, with the given null value or NaN for the missing
    // row.
    table
        .add_column("REALCOL", AstType::Float, &[], " ")
        .unwrap();
    table2
        .add_column("REALCOL", AstType::Float, &[], " ")
        .unwrap();
    table.put_0f("REALCOL(1)", -10.0, Some(" ")).unwrap();
    table.put_0f("REALCOL(3)", 10.0, Some(" ")).unwrap();
    assert_eq!(table.column_size("realcol").unwrap(), 12, "error 13k");
    let reals = table.get_column_data("REALCOL", -1.0, 0.0, None).unwrap();
    assert_eq!(
        reals,
        ColumnData::Float(vec![-10.0, -1.0, 10.0]),
        "error 13l"
    );
    table2.put_column_data("realCol", &reals).unwrap();
    table2.map_remove("REALCOL(2)").unwrap();
    let ColumnData::Float(reals2) = table2
        .get_column_data("REALCOL", f32::NAN, 0.0, None)
        .unwrap()
    else {
        panic!("REALCOL does not hold floats");
    };
    assert_eq!(reals2.len(), 3, "error 13m");
    assert_eq!(reals2[0], -10.0, "error checkreals2 1");
    assert!(reals2[1].is_nan(), "error checkreals2 2b");
    assert_eq!(reals2[2], 10.0, "error checkreals2 3");

    table.remove_column("REALCOL").unwrap();
    table.map_remove("BYTECOL(3)").unwrap();
    table.map_remove("KINTCOL(3)").unwrap();
    table.map_remove("STRINGCOL(3)").unwrap();
    assert_eq!(table.as_table().nrow(), 3, "error 14");
    assert_eq!(table.as_table().ncolumn(), 3, "error 15");

    // The header now describes the rows and their null values.
    let slen = table.as_table().column_len_c("StringCol").unwrap();
    let mut header = table.get_table_header().unwrap();
    let cards = header_cards(&mut header);
    assert_eq!(cards.len(), HEADER2.len(), "error 16/18");
    for (icard, (card, want)) in cards.iter().zip(HEADER2).enumerate() {
        let want = match icard + 1 {
            4 => format!("NAXIS1  = {:20}", 6 + 8 + 3 * slen),
            18 => format!("TFORM3  = '{}A     '", 3 * slen),
            20 => format!("TDIM3   = '({slen},3)   '"),
            _ => want.to_string(),
        };
        assert!(card.starts_with(&want), "error 17: {card} is not {want}");
    }

    let null = table.column_null("BYTECOL", None, true).unwrap();
    assert_eq!(null.value, 254, "error 19");
    assert!(!null.was_set, "error 20");
    assert_eq!(null.has_null, Some(true), "error 21");

    table.purge_rows().unwrap();
    assert_eq!(table.as_table().nrow(), 2, "error 22");
    assert_eq!(table.as_table().ncolumn(), 3, "error 23");
    let mut header = table.get_table_header().unwrap();
    assert_eq!(header.get_fits_i(Some("TNULL1")).unwrap(), None, "error 24");

    let null = table.column_null("BYTECOL", Some(11), true).unwrap();
    assert_eq!(null.value, 11, "error 25");
    assert!(!null.was_set, "error 26");
    assert_eq!(null.has_null, Some(false), "error 27");
    let null = table.column_null("BYTECOL", None, true).unwrap();
    assert_eq!(null.value, 11, "error 28");
    assert!(null.was_set, "error 29");
    assert_eq!(null.has_null, Some(false), "error 30");

    let _copy = table.clone();

    // A null value stored in any element of a cell is found.
    let mut table2 = FitsTable::new().unwrap();
    table2.add_column("SCALAR", AstType::Int, &[], " ").unwrap();
    table2.put_0i("SCALAR(1)", 5, Some(" ")).unwrap();
    table2.put_0i("SCALAR(2)", 7, Some(" ")).unwrap();
    let null = table2.column_null("SCALAR", Some(7), true).unwrap();
    assert_eq!(null.has_null, Some(true), "error 31");
    table2
        .add_column("ARRAY", AstType::Int, &[2, 3], " ")
        .unwrap();
    table2
        .put_1i("ARRAY(1)", &[1, 2, 3, 4, 5, 9], Some(" "))
        .unwrap();
    table2
        .put_1i("ARRAY(2)", &[1, 2, 3, 4, 5, 9], Some(" "))
        .unwrap();
    let null = table2.column_null("ARRAY", Some(9), true).unwrap();
    assert_eq!(null.has_null, Some(true), "error 32");

    // The default null value avoids the values in cells that follow an
    // empty cell.
    let mut table2 = FitsTable::new().unwrap();
    table2.add_column("GAP", AstType::Byte, &[2], " ").unwrap();
    table2.put_1b("GAP(2)", &[0, 255], Some(" ")).unwrap();
    let null = table2.column_null("GAP", None, true).unwrap();
    assert_eq!(null.value, 254, "error 32a");
    assert_eq!(null.has_null, Some(true), "error 32b");

    // When every value allowed by the column type is in use, the default
    // null value is zero if there are no empty cells, and an error
    // otherwise.
    let all: Vec<u8> = (0..=255).collect();
    let mut table2 = FitsTable::new().unwrap();
    table2
        .add_column("ALL", AstType::Byte, &[256], " ")
        .unwrap();
    table2.put_1b("ALL(1)", &all, Some(" ")).unwrap();
    let null = table2.column_null("ALL", None, true).unwrap();
    assert_eq!(null.value, 0, "error 33");
    assert_eq!(null.has_null, Some(false), "error 34");
    table2.put_1b("ALL(3)", &all, Some(" ")).unwrap();
    assert_eq!(
        table2.column_null("ALL", None, true).unwrap_err().code(),
        ErrorCode::Badnull,
        "error 35"
    );

    // Column definitions read from a FITS header: a unit scale is
    // accepted, a string column with no TDIM holds scalar strings, and
    // TDIM may omit the parentheses.
    let mut header = header_of(&[
        "XTENSION= 'BINTABLE'",
        "TFIELDS =                    3",
        "TFORM1  = '1J      '",
        "TTYPE1  = 'SCALED  '",
        "TSCAL1  =                  1.0",
        "TFORM2  = '10A     '",
        "TTYPE2  = 'NAME    '",
        "TFORM3  = '6J      '",
        "TTYPE3  = 'ARRAY   '",
        "TDIM3   = '2,3     '",
    ]);
    let table2 = FitsTable::from_header(&mut header).expect("error 36");
    let t2 = table2.as_table();
    assert_eq!(t2.column_ndim("NAME").unwrap(), 0, "error 37");
    assert_eq!(t2.column_type("NAME").unwrap(), AstType::String, "error 38");
    assert_eq!(t2.column_ndim("ARRAY").unwrap(), 2, "error 39");
    assert_eq!(t2.column_length("ARRAY").unwrap(), 6, "error 40");

    // A non-zero offset is rejected.
    header.set_fits_f("TZERO1", 5.0, "", false).unwrap();
    assert_eq!(
        FitsTable::from_header(&mut header).unwrap_err().code(),
        ErrorCode::Bdfts,
        "error 41"
    );

    // A missing TFORM keyword is reported.
    let mut header = header_of(&[
        "XTENSION= 'BINTABLE'",
        "TFIELDS =                    1",
        "TTYPE1  = 'NOFORM  '",
    ]);
    assert_eq!(
        FitsTable::from_header(&mut header).unwrap_err().code(),
        ErrorCode::Nofts,
        "error 42"
    );
}
