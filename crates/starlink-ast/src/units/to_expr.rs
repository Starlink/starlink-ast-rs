//! Format a `UnitNode` tree as an algebraic expression. Mirrors C
//! `MakeExp` (`unit.c`:2860) in two of its three output styles, selected
//! in C by the `mathmap` argument:
//!
//! - `mathmap = 1`, [`make_exp`]: an expression that feeds directly into
//!   `MathMap::new`, so the syntax must match what the MathMap parser
//!   accepts: `+`, `-`, `*`, `/`, `**`, `log10(...)` (base 10),
//!   `log(...)` (natural), `sqrt(...)`, `exp(...)`, parens, numeric
//!   literals, identifier variables.
//! - `mathmap = 2`, [`make_units_exp`]: a units string, as returned by
//!   `astUnitNormaliser`. Each unit keeps its multiplier prefix, `log(`
//!   is base 10 and `ln(` natural, and a constant multiplying or dividing
//!   a unit is folded into the unit's prefix where one is closer
//!   (`ModifyPrefix`, `unit.c`:4262).
//!
//! The axis-label style (`mathmap = 0`) is in [`super::label`].

use super::complicate::complicate_tree;
use super::simplify::{fix_constants, simplify_tree};
use super::table::{Multiplier, iter_multipliers};
use super::tree::{Oper, UnitNode};
use crate::mappings::helpers::ast_equal;

/// C `MakeExp`'s output style, its `mathmap` argument.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExpStyle {
    /// `mathmap = 1`: an expression for the MathMap parser.
    MathMap,
    /// `mathmap = 2`: a units string.
    Units,
}

/// AST's printf format width for constants (`AST__DBL_DIG`). The C
/// library uses 17 in C11 mode (the build mode our oracle tests use).
const DBL_DIG: usize = 17;

/// Format a constant the way C `MakeExp` does: `"%.*g"` with
/// `AST__DBL_DIG` precision.
pub(crate) fn format_const(v: f64) -> String {
    // Rust's `{:.*e}` is exponent notation; `%g` chooses between fixed
    // and scientific based on magnitude. We approximate with
    // `format!("{:.*e}", DBL_DIG-1, v)` for very small/large, and
    // direct decimal formatting otherwise — but the easiest faithful
    // emulation is to delegate to a small helper.
    format_g(v, DBL_DIG)
}

/// `printf`-equivalent of `%.*g` for `f64`, as C99 7.19.6.1 defines it:
/// with `P` significant digits and `X` the exponent the `%e` form would
/// have after rounding to `P` digits, the fixed form with `P - 1 - X`
/// decimals is used when `P > X >= -4`, otherwise the `%e` form with
/// `P - 1` decimals; trailing zeros are then removed, and the exponent has
/// a sign and at least two digits.
fn format_g(v: f64, precision: usize) -> String {
    if v == 0.0 {
        return if v.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    let p = precision.max(1);
    let sci = format!("{:.*e}", p - 1, v);
    let e_idx = sci.find('e').expect("Rust's {:e} has an exponent");
    let x: i32 = sci[e_idx + 1..].parse().expect("integer exponent");
    if (p as i32) > x && x >= -4 {
        let dec = (p as i32 - 1 - x) as usize;
        strip_trailing_zeros_in_mantissa(&format!("{:.*}", dec, v))
    } else {
        let mantissa = strip_trailing_zeros_in_mantissa(&sci[..e_idx]);
        let sign = if x < 0 { '-' } else { '+' };
        format!("{mantissa}e{sign}{:02}", x.abs())
    }
}

fn strip_trailing_zeros_in_mantissa(m: &str) -> String {
    if !m.contains('.') {
        return m.to_string();
    }
    let trimmed = m.trim_end_matches('0');
    let trimmed = trimmed.trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Top-level entry: format the tree as a MathMap-compatible algebraic
/// expression. Mirrors C `MakeExp(tree, 1, 1, status)`.
pub(crate) fn make_exp(tree: &UnitNode) -> String {
    make_exp_styled(tree, ExpStyle::MathMap)
}

/// Format the tree as a units string. Mirrors C `MakeExp(tree, 2, 1,
/// status)`, the call `astUnitNormaliser` makes.
pub(crate) fn make_units_exp(tree: &UnitNode) -> String {
    make_exp_styled(tree, ExpStyle::Units)
}

fn make_exp_styled(tree: &UnitNode, style: ExpStyle) -> String {
    // C: copy + ComplicateTree to make the expression more natural.
    // C repeats this at every level of its recursion; `complicate_tree`
    // already recurses and re-complicates each subtree it rewrites, so
    // once at the top leaves nothing for the lower levels to change.
    let mut newtree = Box::new(tree.clone());
    complicate_tree(&mut newtree);
    // Neither style takes the axis-label "FixConstants(unity=true)" pass.
    make_exp_inner(&newtree, style)
}

/// Recursive worker, formatting an already-complicated tree. C's `top`
/// argument only affects the axis-label style, which is not ported.
fn make_exp_inner(node: &UnitNode, style: ExpStyle) -> String {
    // Constant.
    if node.con != crate::constants::BAD_VALUE && node.opcode == Oper::LdCon {
        return format_const(node.con);
    }

    match node.opcode {
        // LdPi / LdE — these don't have a `con` set by the parser, so
        // we render their numeric values. (C MakeExp doesn't expose a
        // dedicated branch for these; it relies on the SimplifyTree
        // pass to fold them via FixConstants. We do the same in the
        // top-level `make_exp` via `complicate_tree`'s embedded
        // SimplifyTree calls.)
        Oper::LdPi => format_const(std::f64::consts::PI),
        Oper::LdE => format_const(std::f64::consts::E),

        // A units string keeps the multiplier prefix; a MathMap
        // expression refers to the variable by its bare name.
        Oper::LdVar => {
            let name = node.name.as_deref().unwrap_or_default();
            match (style, node.mult) {
                (ExpStyle::Units, Some(mult)) => format!("{}{}", mult.sym, name),
                _ => name.to_string(),
            }
        }

        // OP_LOG is base 10: `log10(...)` for MathMap, `log(...)` in a
        // units string.
        Oper::Log => {
            let a = make_exp_inner(&node.arg[0], style);
            match style {
                ExpStyle::MathMap => format!("log10({})", a),
                ExpStyle::Units => format!("log({})", a),
            }
        }

        // OP_LN is natural: MathMap's `log(...)`, a units string's
        // `ln(...)`.
        Oper::Ln => {
            let a = make_exp_inner(&node.arg[0], style);
            match style {
                ExpStyle::MathMap => format!("log({})", a),
                ExpStyle::Units => format!("ln({})", a),
            }
        }

        Oper::Exp => {
            let a = make_exp_inner(&node.arg[0], style);
            format!("exp({})", a)
        }

        Oper::Sqrt => {
            let a = make_exp_inner(&node.arg[0], style);
            format!("sqrt({})", a)
        }

        Oper::Pow => {
            let arg0 = make_exp_inner(&node.arg[0], style);
            let arg1 = make_exp_inner(&node.arg[1], style);
            // C: parens around arg0 if it has narg==2, OR if it's an
            // OP_LDVAR and mathmap is zero. mathmap=1 path drops the
            // second condition.
            let par = node.arg[0].arg.len() == 2;
            if par {
                format!("({}){}{}", arg0, "**", arg1)
            } else {
                format!("{}{}{}", arg0, "**", arg1)
            }
        }

        Oper::Div | Oper::Mult if style == ExpStyle::Units => match modify_prefix(node) {
            // C formats the modified node with a recursive `MakeExp`, which
            // complicates its copy first.
            Some(mut sunit) => {
                complicate_tree(&mut sunit);
                make_exp_inner(&sunit, style)
            }
            None => format_binary(node, style),
        },

        Oper::Div | Oper::Mult => format_binary(node, style),

        // LdCon when con==BAD_VALUE shouldn't happen in practice.
        Oper::LdCon | Oper::Null => "0".to_string(),
    }
}

/// Format a `Div` or `Mult` node from its two formatted arguments, adding
/// the parentheses C's `MakeExp` adds.
fn format_binary(node: &UnitNode, style: ExpStyle) -> String {
    match node.opcode {
        Oper::Div => {
            let arg0 = make_exp_inner(&node.arg[0], style);
            let arg1 = make_exp_inner(&node.arg[1], style);
            // C: parens around arg1 if it's MULT and contains a '*'.
            let par = node.arg[1].opcode == Oper::Mult && arg1.contains('*');
            if par {
                format!("{}/({})", arg0, arg1)
            } else {
                format!("{}/{}", arg0, arg1)
            }
        }

        Oper::Mult => {
            let arg0 = make_exp_inner(&node.arg[0], style);
            let arg1 = make_exp_inner(&node.arg[1], style);
            // C: parens around arg0 if it's DIV containing '/' or POW
            // containing '**'.
            let par = (node.arg[0].opcode == Oper::Div && arg0.contains('/'))
                || (node.arg[0].opcode == Oper::Pow && arg0.contains("**"));
            if par {
                format!("({})*{}", arg0, arg1)
            } else {
                format!("{}*{}", arg0, arg1)
            }
        }

        _ => unreachable!("format_binary is only called for Div and Mult"),
    }
}

/// Whether two multiplier prefixes are the same table entry, the pointer
/// comparison C makes.
fn same_multiplier(a: Option<&'static Multiplier>, b: Option<&'static Multiplier>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => std::ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// Fold the constant in a `constant*unit`, `unit/constant` or
/// `constant/unit` node into the unit's multiplier prefix, choosing the
/// prefix closest to the combined scale. Returns the modified node, or
/// `None` when the node has no such form or nothing would change. Mirrors
/// C `ModifyPrefix` (`unit.c`:4262).
///
/// `make_exp_inner` formats the returned node by calling itself on it, so
/// the recursion ends only because an unchanged node gives `None`. For
/// `constant/unit` the stored constant is the numerator, not the combined
/// scale, so whether it has changed is only judged once it is recomputed.
fn modify_prefix(old: &UnitNode) -> Option<Box<UnitNode>> {
    if old.opcode != Oper::Div && old.opcode != Oper::Mult {
        return None;
    }

    let find = |op: Oper| (0..2).find(|&i| old.arg[i].opcode == op);
    let ivar = find(Oper::LdVar)?;
    let icon = find(Oper::LdCon)?;

    let mut newtree = Box::new(old.clone());

    // The constant by which the unit, without its prefix, is multiplied.
    // A unit in the denominator of a DIV makes the node its reciprocal.
    let cmult = newtree.arg[ivar].mult.map_or(1.0, |m| m.scale);
    let mut con = newtree.arg[icon].con;
    let recip = if newtree.opcode == Oper::Mult {
        con *= cmult;
        false
    } else {
        con = cmult / con;
        ivar == 1
    };

    // Find the known multiplier closest to the constant; `None` if unity
    // is closest.
    let mut rmin = if con > 1.0 { con } else { 1.0 / con };
    let mut mmult = None;
    for mult in iter_multipliers() {
        let r = if con > mult.scale {
            con / mult.scale
        } else {
            mult.scale / con
        };
        if r < rmin {
            mmult = Some(mult);
            rmin = r;
        }
    }
    if let Some(m) = mmult {
        con /= m.scale;
    }

    let mut changed = false;
    if !same_multiplier(newtree.arg[ivar].mult, mmult) {
        newtree.arg[ivar].mult = mmult;
        changed = true;
    }
    if newtree.arg[icon].con != con {
        newtree.arg[icon].con = con;
        // The numerator of a reciprocal is judged below.
        if !recip {
            changed = true;
        }
    }

    if !recip {
        if newtree.opcode != Oper::Mult {
            newtree.opcode = Oper::Mult;
            changed = true;
        }
        // Tolerance from C.
        if (con - 1.0).abs() < 1.0e-6 {
            return Some(newtree.arg[ivar].clone());
        }
    } else {
        // The numerator of a reciprocal is the reciprocal of the scale.
        newtree.arg[icon].con = 1.0 / newtree.arg[icon].con;
        if !ast_equal(newtree.arg[icon].con, old.arg[0].con) {
            changed = true;
        }
    }

    changed.then_some(newtree)
}

// ---------------------------------------------------------------------
// Internal: re-export the simplify-driven re-fix used by the C
// `MakeExp` for axis labels. We don't actually use the
// `mathmap=0` branch — that's for axis label production — so the
// helpers below remain unused. Suppress dead-code warnings via the
// crate-level `allow(dead_code)`.
// ---------------------------------------------------------------------
#[allow(dead_code)]
fn relabel_pass(node: &mut Box<UnitNode>) {
    fix_constants(node, true);
    while simplify_tree(node, false) {
        fix_constants(node, true);
    }
}
