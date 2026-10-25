// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Tree comparison. Mirrors C `CmpTree` (`unit.c`:568-672).
//!
//! Returns 0 when two trees are equivalent. Non-zero values are
//! ordering signs (+1: tree1 > tree2, -1: tree1 < tree2). When
//! `exact=false`, OP_MULT nodes match even if their arguments are
//! swapped (used by `FindFactors` to coalesce `a*b` with `b*a`).

use crate::constants::BAD_VALUE;

use super::tree::{Oper, UnitNode};

/// Fuzzy equality matching the C `astEQUAL` macro: identical doubles
/// or relative difference ≤ 1e-9.
fn ast_equal(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    let scale = a.abs().max(b.abs());
    (a - b).abs() <= 1e-9 * scale
}

/// Mirrors C `CmpTree` (`unit.c`:568). Returns 0 when equivalent.
pub(crate) fn cmp_tree(tree1: &UnitNode, tree2: &UnitNode, exact: bool) -> i32 {
    let op = tree1.opcode;
    if op != tree2.opcode {
        // Compare opcodes as integers (C casts the enum to int).
        return if (op as i32) > (tree2.opcode as i32) {
            1
        } else {
            -1
        };
    }

    // Both LdVar: compare the variable names.
    if op == Oper::LdVar {
        let n1 = tree1.name.as_deref().unwrap_or("");
        let n2 = tree2.name.as_deref().unwrap_or("");
        return match n1.cmp(n2) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        };
    }

    // Both LdCon (con != BAD_VALUE in C; we only enter this branch for
    // LdCon opcode with a real constant value).
    if op == Oper::LdCon && tree1.con != BAD_VALUE {
        if ast_equal(tree1.con, tree2.con) {
            return 0;
        }
        return if tree1.con > tree2.con { 1 } else { -1 };
    }

    // Otherwise, compare the arguments in order.
    let mut result: i32 = 0;
    for i in 0..tree1.arg.len() {
        result = cmp_tree(&tree1.arg[i], &tree2.arg[i], exact);
        if result != 0 {
            break;
        }
    }

    // For OP_MULT with `exact=false`, retry with swapped arguments.
    if result != 0 && op == Oper::Mult && !exact {
        let mut swapped: i32 = 0;
        for i in 0..tree1.arg.len() {
            let other = if i == 0 { 1 } else { 0 };
            swapped = cmp_tree(&tree1.arg[i], &tree2.arg[other], false);
            if swapped != 0 {
                break;
            }
        }
        if swapped == 0 {
            return 0;
        }
    }

    result
}
