//! Inverse of `SimplifyTree`'s standardisation passes — used by
//! `MakeExp` to make the formatted expression more natural for human
//! readers (e.g. turning `(A**-1)*B` back into `B/A`). Mirrors C
//! `ComplicateTree` (`unit.c`:827-1020).

use crate::constants::BAD_VALUE;

use super::cmp::cmp_tree;
use super::simplify::simplify_tree;
use super::tree::{Oper, UnitNode};

/// Helper: `astEQUAL`-style fuzzy equality. Mirrors `astEQUAL` from
/// `pal.h` (≈ relative-1e-9 tolerance for non-zero, exact for zero).
fn ast_equal(a: f64, b: f64) -> bool {
    if a == BAD_VALUE || b == BAD_VALUE {
        return false;
    }
    if a == b {
        return true;
    }
    let scale = a.abs().max(b.abs());
    (a - b).abs() <= 1e-9 * scale
}

/// "Complicate" a tree by undoing `SimplifyTree`'s canonical-form
/// rewrites that would look unnatural to a human reader. Mirrors
/// `ComplicateTree` (`unit.c`:827).
pub(crate) fn complicate_tree(node: &mut Box<UnitNode>) -> bool {
    let mut result = false;

    // Recurse into children first.
    for child in node.arg.iter_mut() {
        if complicate_tree(child) {
            result = true;
        }
    }

    let op = node.opcode;
    let mut newnode: Option<Box<UnitNode>> = None;

    if op == Oper::Mult {
        let a0 = &node.arg[0];
        let a1 = &node.arg[1];

        // k * ln(x): rearrange to ln(x**k). If k is an integer multiple
        // of "0.1/ln(10)" convert ln to log (base 10). If k==1 drop the
        // POW.
        if a0.con != BAD_VALUE && a1.opcode == Oper::Ln {
            let con0 = a0.con;
            let fk = 10.0 * con0 * crate::math::ln(10.0_f64);
            let k = fk.round() as i64;
            let (head_op, eff_con) = if ast_equal(fk, k as f64) {
                (Oper::Log, k as f64 / 10.0)
            } else {
                (Oper::Ln, con0)
            };
            let mut head = UnitNode::new(head_op);
            let inner = a1.arg[0].clone();
            if !ast_equal(eff_con, 1.0) {
                let pow = UnitNode::binary(Oper::Pow, inner, UnitNode::ldcon(eff_con));
                head.arg[0] = pow;
            } else {
                head.arg[0] = inner;
            }
            newnode = Some(Box::new(head));

        // (A**-1) * B → B / A
        } else if a0.opcode == Oper::Pow && ast_equal(a0.arg[1].con, -1.0) {
            let mut div = UnitNode::new(Oper::Div);
            div.arg[0] = a1.clone();
            div.arg[1] = a0.arg[0].clone();
            newnode = Some(Box::new(div));

        // B * (A**-1) → B / A
        } else if a1.opcode == Oper::Pow && ast_equal(a1.arg[1].con, -1.0) {
            let mut div = UnitNode::new(Oper::Div);
            div.arg[0] = a0.clone();
            div.arg[1] = a1.arg[0].clone();
            newnode = Some(Box::new(div));

        // (x**k)*(y**k) → (x*y)**k
        } else if a0.opcode == Oper::Pow
            && a1.opcode == Oper::Pow
            && ast_equal(a0.arg[1].con, a1.arg[1].con)
        {
            let mut head = UnitNode::new(Oper::Pow);
            let mult = UnitNode::binary(Oper::Mult, a0.arg[0].clone(), a1.arg[0].clone());
            head.arg[0] = mult;
            head.arg[1] = a0.arg[1].clone();
            newnode = Some(Box::new(head));

        // c * sqrt(x) → sqrt((c**2) * x), if c > 0
        } else {
            let kcon = a0.con;
            if kcon != BAD_VALUE && kcon > 0.0 && a1.opcode == Oper::Sqrt {
                let inner_mult =
                    UnitNode::binary(Oper::Mult, UnitNode::ldcon(kcon * kcon), a1.arg[0].clone());
                let mut head = UnitNode::new(Oper::Sqrt);
                head.arg[0] = inner_mult;
                newnode = Some(Box::new(head));
            }
        }
    } else if op == Oper::Pow {
        // x**0.5 → sqrt(x)
        if ast_equal(node.arg[1].con, 0.5) {
            let mut head = UnitNode::new(Oper::Sqrt);
            head.arg[0] = node.arg[0].clone();
            newnode = Some(Box::new(head));
        }
    }

    // If the rewrite produced something identical to the existing
    // node, drop it.
    if let Some(nn) = newnode.as_ref() {
        if cmp_tree(nn, node, true) == 0 {
            newnode = None;
        }
    }

    if let Some(nn) = newnode {
        *node = nn;
        result = true;
        // Re-simplify (without standardisation) and re-complicate.
        simplify_tree(node, false);
        complicate_tree(node);
    }

    result
}
