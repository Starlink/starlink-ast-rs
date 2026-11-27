// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Native-format serialization for `Table`.
//!
//! Ported from `Dump` and `astLoadTable` in
//! `reference/starlink-ast/src/table.c`.

use super::Table;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::keymap::KeyMap;
use crate::serial::node::Node;
use crate::serial::{
    AnyObject, MXLOADEL, Serializable, any_object_from_node, any_object_to_node, too_many_values,
};
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
        let class = &node.class;
        let nrow = part.get_int("Nrow", 0).max(0) as usize;
        // A row's cells are named by their row number (see `MXLOADEL`).
        if nrow > MXLOADEL {
            return Err(too_many_values(
                "astLoadTable",
                class,
                &format!("The Table read has {nrow} rows"),
            ));
        }
        let columns = read_keymap(&part, class, "Columns", "KeyMap of column definitions")?;
        // A column name makes the key of each of its cells, so it is no
        // longer than a column added to a Table may have (table.c
        // `astLoadTable`).
        for icol in 0..columns.map_size() {
            let name = columns.map_key(icol)?;
            if name.trim_end_matches(' ').chars().count() > super::MXCOLNAMLEN {
                return Err(AstError::new(
                    ErrorCode::Lderr,
                    format!(
                        "astLoad({class}): The name of column {} read for a {class} is too long \
                         (it must have no more than {} characters).",
                        icol + 1,
                        super::MXCOLNAMLEN
                    ),
                ));
            }
        }
        let parameters = read_keymap(&part, class, "Params", "KeyMap of parameter definitions")?;
        Ok(Table {
            map,
            columns: RefCell::new(columns),
            parameters,
            nrow,
            class: "Table",
        })
    }
}

/// The KeyMap written as item `key` of a Table of class `class`, which
/// must be a KeyMap if it is there (table.c `astLoadTable`, with
/// astCheckLoaded); an empty one when it is not.
fn read_keymap(part: &Node, class: &str, key: &str, item: &str) -> AstResult<KeyMap> {
    let Some(object) =
        crate::serial::check_loaded(part.get_object(key), "KeyMap", false, class, item)?
    else {
        return Ok(KeyMap::new());
    };
    match any_object_from_node(object)? {
        AnyObject::KeyMap(km) => Ok(*km),
        AnyObject::Table(t) => Ok(t.as_keymap().clone()),
        other => Err(AstError::new(
            ErrorCode::Lderr,
            format!(
                "astLoad({class}): The {item} read for a {class} is a {}, which is not a KeyMap.",
                other.class_name()
            ),
        )),
    }
}
