// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Tree inversion. Mirrors C `InvertTree` (`unit.c`:2516-2685).
//!
//! Given a forward tree (assumed to have been simplified with
//! `simplify_tree(std=true)` so it contains no `Log`, `Sqrt`, or `Div`
//! nodes), construct an inverted tree where the head corresponds to
//! the `LdVar` leaf of the forward tree, and `src` is the new leaf.

use crate::constants::BAD_VALUE;

use super::tree::{Oper, UnitNode};

/// Invert a unit-conversion tree. Mirrors C `InvertTree`
/// (`unit.c`:2516).
///
/// The supplied forward tree must have exactly one `LdVar` node. The
/// returned tree has its head where the forward `LdVar` was, with
/// `src` plugged in as the (new) varying input. Returns `None` when
/// the supplied tree is uninvertible (e.g. an `LdCon`-only tree, a
/// `Mult` whose two arguments both vary, etc.).
pub(crate) fn invert_tree(fwdnode: &UnitNode, src: Box<UnitNode>) -> Option<Box<UnitNode>> {
    invert_tree_inner(fwdnode, src)
}

fn invert_tree_inner(fwdnode: &UnitNode, src: Box<UnitNode>) -> Option<Box<UnitNode>> {
    let fop = fwdnode.opcode;
    let newnode: Box<UnitNode>;
    let nextnode: &UnitNode;

    match fop {
        // Inverse of "exp(x)" is "ln(x)".
        Oper::Exp => {
            let mut n = UnitNode::new(Oper::Ln);
            n.arg[0] = src;
            newnode = Box::new(n);
            nextnode = &fwdnode.arg[0];
        }

        // Inverse of "ln(x)" is "exp(x)".
        Oper::Ln => {
            let mut n = UnitNode::new(Oper::Exp);
            n.arg[0] = src;
            newnode = Box::new(n);
            nextnode = &fwdnode.arg[0];
        }

        // Inverse of "x**k" is "x**(1/k)".
        Oper::Pow => {
            let k = fwdnode.arg[1].con;
            if k == BAD_VALUE || k == 0.0 {
                return None;
            }
            let mut n = UnitNode::new(Oper::Pow);
            n.arg[0] = src;
            n.arg[1] = UnitNode::ldcon(1.0 / k);
            newnode = Box::new(n);
            nextnode = &fwdnode.arg[0];
        }

        // Inverse of "k*x" is "(1/k)*x". Only invertible when exactly
        // one operand is constant.
        Oper::Mult => {
            let c0 = fwdnode.arg[0].con;
            let c1 = fwdnode.arg[1].con;
            let varg = if c0 != BAD_VALUE && c1 == BAD_VALUE {
                1usize
            } else if c0 == BAD_VALUE && c1 != BAD_VALUE {
                0usize
            } else {
                return None;
            };
            let k = fwdnode.arg[1 - varg].con;
            if k == 0.0 {
                return None;
            }
            let mut n = UnitNode::new(Oper::Mult);
            n.arg[0] = UnitNode::ldcon(1.0 / k);
            n.arg[1] = src;
            newnode = Box::new(n);
            nextnode = &fwdnode.arg[varg];
        }

        // LDVAR — nothing left to invert. Return `src` directly.
        Oper::LdVar => {
            return Some(src);
        }

        // Anything else (LdCon, LdPi, LdE, Log, Sqrt, Div, Null) — not
        // invertible (after std simplify Log/Sqrt/Div are gone, and the
        // others are leaves that can't carry a varying input).
        _ => return None,
    }

    // Recurse to invert the remainder of the chain.
    invert_tree_inner(nextnode, newnode)
}
