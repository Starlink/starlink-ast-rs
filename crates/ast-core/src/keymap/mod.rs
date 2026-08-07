//! A KeyMap: an unordered collection of key/value pairs in which each
//! value may be a scalar or a 1-D vector of one of eight data types,
//! including a nested AST Object.
//!
//! Ported from `reference/starlink-ast/src/keymap.c`.
//!
//! Keys are stored in a [`std::collections::HashMap`] rather than C's
//! chained bucket table. C's table arithmetic survives only in
//! [`table`], which reconstructs the `MapSz` card at dump time.

// Removed in Task 14 when the public API lands.
#![allow(dead_code)]

pub(crate) mod entry;
pub(crate) mod table;
