//! Native-format serialization for `Table`.
//!
//! Ported from `Dump` and `astLoadTable` in
//! `reference/starlink-ast/src/table.c`.

use super::Table;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::keymap::KeyMap;
use crate::serial::node::Node;
use crate::serial::{AnyObject, Serializable, any_object_from_node, any_object_to_node};
use std::cell::RefCell;

impl Serializable for Table {
    fn class_name(&self) -> &'static str {
        "Table"
    }

    fn to_node(&self) -> AstResult<Node> {
        // The KeyMap part holds the cells and parameter values.
        let mut node = self.map.to_node()?;
        node.class = "Table".to_string();
        node.class_comment = Some("Two-dimensional table of data values".to_string());
        node.push_isa("KeyMap", Some("Map of key/value pairs".to_string()));

        node.push_int(
            "Nrow",
            self.nrow as i64,
            Some("Number of rows in table".to_string()),
        );
        node.push_object(
            "Columns",
            any_object_to_node(&AnyObject::KeyMap(Box::new(self.columns.borrow().clone())))?,
            Some("KeyMap holding column definitions".to_string()),
        );
        node.push_object(
            "Params",
            any_object_to_node(&AnyObject::KeyMap(Box::new(self.parameters.clone())))?,
            Some("KeyMap holding parameter definitions".to_string()),
        );
        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self> {
        let map = KeyMap::from_node(node)?;
        let part = node.section("Table");
        let nrow = part.get_int("Nrow", 0).max(0) as usize;
        let columns = read_keymap(&part, "Columns")?;
        let parameters = read_keymap(&part, "Params")?;
        Ok(Table {
            map,
            columns: RefCell::new(columns),
            parameters,
            nrow,
            class: "Table",
        })
    }
}

/// The KeyMap written as item `name`. C's `Dump` always writes it, and a
/// Table read without it would fail when first used.
fn read_keymap(part: &Node, name: &str) -> AstResult<KeyMap> {
    let missing = || {
        AstError::new(
            ErrorCode::Badin,
            format!("astLoadTable(Table): No {name} KeyMap found whilst reading a Table."),
        )
    };
    let object = part.get_object(name).ok_or_else(missing)?;
    match any_object_from_node(object)? {
        AnyObject::KeyMap(km) => Ok(*km),
        _ => Err(missing()),
    }
}
