//! Native-format serialisation for `FitsTable`.
//!
//! Ported from `Dump` and `astLoadFitsTable` in
//! `reference/starlink-ast/src/fitstable.c`.

use super::FitsTable;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::node::Node;
use crate::serial::{AnyObject, Serializable, any_object_from_node, any_object_to_node};
use crate::table::Table;

impl Serializable for FitsTable {
    fn class_name(&self) -> &'static str {
        "FitsTable"
    }

    fn to_node(&self) -> AstResult<Node> {
        // The Table part holds the column data and definitions.
        let mut node = self.table.to_node()?;
        node.class = "FitsTable".to_string();
        node.class_comment = Some("FITS binary table".to_string());
        node.push_isa(
            "Table",
            Some("Two-dimensional table of data values".to_string()),
        );

        node.push_object(
            "Header",
            any_object_to_node(&AnyObject::FitsChan(Box::new(self.header.clone())))?,
            Some("FITS headers".to_string()),
        );
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let mut table = Table::from_node(node)?;
        table.set_class("FitsTable");

        // C's Dump always writes the header, and a FitsTable read without
        // it would fail when first used.
        let missing = || {
            AstError::new(
                ErrorCode::Badin,
                "astLoadFitsTable(FitsTable): No Header FitsChan found whilst reading a \
                 FitsTable.",
            )
        };
        let part = node.section("FitsTable");
        let object = part.get_object("Header").ok_or_else(missing)?;
        let header = match any_object_from_node(object)? {
            AnyObject::FitsChan(fc) => *fc,
            _ => return Err(missing()),
        };
        Ok(FitsTable { table, header })
    }
}
