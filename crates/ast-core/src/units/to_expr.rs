//! Format a `UnitNode` tree as an algebraic expression. Mirrors C
//! `MakeExp` with `mathmap=1` (`unit.c`:2769-2865).
//!
//! The generated expressions feed directly into `MathMap::new`, so the
//! syntax must match what the MathMap parser accepts: `+`, `-`, `*`,
//! `/`, `**`/`pow(,)`, `log(...)` (base 10), `log10(...)` (also base 10
//! — accepted alias), `ln(...)` (natural), `sqrt(...)`, `exp(...)`,
//! parens, numeric literals, identifier variables.
//!
//! Important: in `MakeExp(mathmap=1)`, the C library emits `log10(` for
//! `OP_LOG` and `log(` for `OP_LN`. The Rust MathMap accepts both
//! `log(` (natural) and `log10(` (base-10), matching the C convention.

use super::complicate::complicate_tree;
use super::simplify::{fix_constants, simplify_tree};
use super::tree::{Oper, UnitNode};

/// AST's printf format width for constants (`AST__DBL_DIG`). The C
/// library uses 17 in C11 mode (the build mode our oracle tests use).
const DBL_DIG: usize = 17;

/// Format a constant the way C `MakeExp` does: `"%.*g"` with
/// `AST__DBL_DIG` precision.
fn format_const(v: f64) -> String {
    // Rust's `{:.*e}` is exponent notation; `%g` chooses between fixed
    // and scientific based on magnitude. We approximate with
    // `format!("{:.*e}", DBL_DIG-1, v)` for very small/large, and
    // direct decimal formatting otherwise — but the easiest faithful
    // emulation is to delegate to a small helper.
    format_g(v, DBL_DIG)
}

/// `printf`-equivalent of `%.*g` for `f64`. Selects between `%e` and
/// `%f` based on magnitude (mirroring C's behaviour).
fn format_g(v: f64, precision: usize) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let abs = v.abs();
    let mag = abs.log10().floor() as i32;
    // C's %g: if exponent < -4 or >= precision, use %e style; else %f.
    if mag < -4 || mag >= precision as i32 {
        // Scientific. Strip trailing zeros after decimal point in
        // mantissa, like C's %g does.
        let s = format!("{:.*e}", precision - 1, v);
        strip_trailing_zeros_g(&s)
    } else {
        // Fixed. Precision = significant digits, so digits-after-decimal
        // = precision - 1 - mag.
        let dec = (precision as i32 - 1 - mag).max(0) as usize;
        let s = format!("{:.*}", dec, v);
        strip_trailing_zeros_g(&s)
    }
}

/// Strip trailing zeros from the mantissa of a `%g`-style string,
/// preserving structure: e.g. `"1.50000e6"` → `"1.5e6"`,
/// `"1.500000"` → `"1.5"`.
fn strip_trailing_zeros_g(s: &str) -> String {
    // Split off any exponent.
    if let Some(e_idx) = s.find(['e', 'E']) {
        let (mantissa, exp) = s.split_at(e_idx);
        let m = strip_trailing_zeros_in_mantissa(mantissa);
        format!("{}{}", m, exp)
    } else {
        strip_trailing_zeros_in_mantissa(s)
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
    // C: copy + ComplicateTree to make the expression more natural.
    let mut newtree = Box::new(tree.clone());
    complicate_tree(&mut newtree);
    // mathmap branch skips the "FixConstants(unity=true)" pass.
    make_exp_inner(&newtree, true)
}

/// Recursive worker. `top` corresponds to the C `top` argument and
/// affects how OP_LDVAR is rendered when producing axis labels — for
/// `mathmap=1` it's irrelevant, but we keep it for parity.
fn make_exp_inner(node: &UnitNode, _top: bool) -> String {
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

        Oper::LdVar => node.name.clone().unwrap_or_default(),

        // OP_LOG → log10(...) under mathmap=1.
        Oper::Log => {
            let a = make_exp_inner(&node.arg[0], false);
            format!("log10({})", a)
        }

        // OP_LN → log(...) under mathmap=1 (MathMap's `log()` is
        // natural log).
        Oper::Ln => {
            let a = make_exp_inner(&node.arg[0], false);
            format!("log({})", a)
        }

        Oper::Exp => {
            let a = make_exp_inner(&node.arg[0], false);
            format!("exp({})", a)
        }

        Oper::Sqrt => {
            let a = make_exp_inner(&node.arg[0], false);
            format!("sqrt({})", a)
        }

        Oper::Pow => {
            let arg0 = make_exp_inner(&node.arg[0], false);
            let arg1 = make_exp_inner(&node.arg[1], false);
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

        Oper::Div => {
            let arg0 = make_exp_inner(&node.arg[0], false);
            let arg1 = make_exp_inner(&node.arg[1], false);
            // C: parens around arg1 if it's MULT and contains a '*'.
            let par = node.arg[1].opcode == Oper::Mult && arg1.contains('*');
            if par {
                format!("{}/({})", arg0, arg1)
            } else {
                format!("{}/{}", arg0, arg1)
            }
        }

        Oper::Mult => {
            let arg0 = make_exp_inner(&node.arg[0], false);
            let arg1 = make_exp_inner(&node.arg[1], false);
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

        // LdCon when con==BAD_VALUE shouldn't happen in practice.
        Oper::LdCon | Oper::Null => "0".to_string(),
    }
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
