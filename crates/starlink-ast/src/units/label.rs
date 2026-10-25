// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Axis labels rewritten for a change of units, as C's `astUnitMapper`
//! produces them when given an input label (unit.c:5349).
//!
//! A label such as `"Frequency"` is parsed into a tree whose single
//! variable is the basic label (`MakeLabelTree`), joined to the tree that
//! maps the default units to the new ones, simplified, and printed in the
//! axis-label style of `MakeExp` (`mathmap = 0`), which drops simple scale
//! factors: a scaled wavelength is still a wavelength, but the logarithm of
//! one is `"log( wavelength )"`.

use crate::constants::BAD_VALUE;

use super::complicate::complicate_tree;
use super::parser::con_start;
use super::simplify::{fix_constants, simplify_tree};
use super::tree::{Oper, UnitNode};

/// The tree of a label, with the basic label as its one variable. Mirrors
/// C `MakeLabelTree` (unit.c:3474): `**` and `^` at the outermost level of
/// parentheses split it, then a leading `sqrt(`, `exp(`, `ln(` or `log(`,
/// then enclosing parentheses, then a numerical constant; anything else is
/// the basic label, without its surrounding white space. `None` when the
/// label cannot be parsed, which C reports as an error.
pub(crate) fn make_label_tree(lab: &str) -> Option<Box<UnitNode>> {
    let exp = lab.trim_matches(|c: char| c.is_ascii_whitespace());
    if exp.is_empty() {
        return None;
    }
    let bytes = exp.as_bytes();

    // A binary operator at depth zero.
    let mut depth = 0i32;
    let mut split = None;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            c if depth == 0 && !c.is_ascii_whitespace() => {
                if exp[i..].starts_with("**") {
                    split = Some((i, 2));
                    break;
                } else if c == b'^' {
                    split = Some((i, 1));
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    if let Some((at, oplen)) = split {
        let mut node = UnitNode::new(Oper::Pow);
        node.arg[0] = make_label_tree(&exp[..at])?;
        node.arg[1] = make_label_tree(&exp[at + oplen..])?;
        return Some(Box::new(node));
    }

    // A function of one argument.
    for (name, op) in [
        ("sqrt(", Oper::Sqrt),
        ("SQRT(", Oper::Sqrt),
        ("exp(", Oper::Exp),
        ("EXP(", Oper::Exp),
        ("ln(", Oper::Ln),
        ("LN(", Oper::Ln),
        ("log(", Oper::Log),
        ("LOG(", Oper::Log),
    ] {
        if exp.starts_with(name) {
            let mut node = UnitNode::new(op);
            node.arg[0] = make_label_tree(&exp[name.len() - 1..])?;
            return Some(Box::new(node));
        }
    }

    // Enclosing parentheses.
    if bytes[0] == b'(' && bytes[bytes.len() - 1] == b')' {
        return make_label_tree(&exp[1..exp.len() - 1]);
    }

    // A numerical constant, which must be the whole string.
    if let Some((con, n)) = con_start(exp) {
        if n != exp.len() {
            return None;
        }
        let mut node = UnitNode::new(Oper::LdCon);
        node.con = con;
        return Some(Box::new(node));
    }

    let mut node = UnitNode::new(Oper::LdVar);
    node.name = Some(exp.to_string());
    Some(Box::new(node))
}

/// A tree printed as an axis label: C `MakeExp( tree, 0, 1 )` (unit.c:2860).
/// `None` where C returns no string, as for a product of constants.
pub(crate) fn make_label_exp(tree: &UnitNode) -> Option<String> {
    label_exp(tree, true)
}

/// C `MakeExp` with `mathmap = 0`. As in C, every level copies and
/// complicates its tree and folds its constants afresh before formatting.
fn label_exp(tree: &UnitNode, top: bool) -> Option<String> {
    let mut node = Box::new(tree.clone());
    complicate_tree(&mut node);
    fix_constants(&mut node, true);
    while simplify_tree(&mut node, false) {
        fix_constants(&mut node, true);
    }

    if node.con != BAD_VALUE {
        return Some(super::to_expr::format_const(node.con));
    }
    Some(match node.opcode {
        Oper::LdVar => {
            let name = node.name.as_deref().unwrap_or_default();
            if top {
                name.to_string()
            } else {
                format!(" {name} ")
            }
        }
        Oper::Log => format!("log({})", label_exp(&node.arg[0], false)?),
        Oper::Ln => format!("ln({})", label_exp(&node.arg[0], false)?),
        Oper::Exp => format!("exp({})", label_exp(&node.arg[0], false)?),
        Oper::Sqrt => format!("sqrt({})", label_exp(&node.arg[0], false)?),
        Oper::Pow => {
            let a0 = label_exp(&node.arg[0], false)?;
            let a1 = label_exp(&node.arg[1], false)?;
            if node.arg[0].arg.len() == 2 || node.arg[0].opcode == Oper::LdVar {
                format!("({a0})**{a1}")
            } else {
                format!("{a0}**{a1}")
            }
        }
        Oper::Div => {
            let a0 = label_exp(&node.arg[0], false)?;
            let a1 = label_exp(&node.arg[1], false)?;
            if node.arg[1].opcode == Oper::Mult && a1.contains('*') {
                format!("{a0}/({a1})")
            } else {
                format!("{a0}/{a1}")
            }
        }
        Oper::Mult => {
            let mut a0 = label_exp(&node.arg[0], false);
            let mut a1 = label_exp(&node.arg[1], false);
            // A top-level label carries no constant factors.
            if top {
                if node.arg[0].con != BAD_VALUE {
                    a0 = None;
                }
                if node.arg[1].con != BAD_VALUE {
                    a1 = None;
                }
            }
            match (a0, a1) {
                (Some(a0), Some(a1)) => {
                    let par = (node.arg[0].opcode == Oper::Div && a0.contains('/'))
                        || (node.arg[0].opcode == Oper::Pow && a0.contains("**"));
                    if par {
                        format!("({a0})*{a1}")
                    } else {
                        format!("{a0}*{a1}")
                    }
                }
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => return None,
            }
        }
        Oper::LdCon | Oper::LdPi | Oper::LdE | Oper::Null => return None,
    })
}
