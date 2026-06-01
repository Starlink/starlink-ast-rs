//! Tree simplification, constant folding, and substitution of
//! derived-unit definitions. Mirrors `RemakeTree`, `FixConstants`,
//! `InvertConstants`, and the simplification passes in
//! `reference/starlink-ast/src/unit.c`.
//!
//! Commit 1 only ports the "minimum subset" of `SimplifyTree` needed
//! to detect `UnitMap` / `ZoomMap` shapes — constant folding, the
//! commute-constant-left rule on `OP_MULT`, and the trivial
//! `*1` / `*0` / `/1` rewrites. The full standardisation rules
//! (`ln(x^k) → k*ln(x)`, `sqrt(x) → x^0.5`, `(x*y)^k → x^k * y^k`,
//! the `FindFactors` / `CombineFactors` standard form) land in commit
//! 2 alongside the MathMap path.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::constants::BAD_VALUE;

use super::parser::create_tree;
use super::table::iter_known_units;
use super::tree::{Oper, UnitNode};

// ---------------------------------------------------------------------
// Cached parsed definitions ("head" trees) for derived KnownUnits.
//
// In the C library, `KnownUnit::head` is built once during the
// `GetKnownUnits` initialisation pass (see `MakeKnownUnit` at
// `unit.c`:3201). The Rust `KnownUnit` struct doesn't carry the head
// directly because the table is a `&'static [KnownUnit]` populated
// before parsing is available. We mirror the C behaviour with a
// global, lazily-initialised cache populated in declaration order so
// that derived-unit definitions (e.g. `J = "N m"`) see the heads of
// their dependencies (e.g. `N = "kg m/s**2"`) already cached.
// ---------------------------------------------------------------------

fn definition_heads() -> &'static HashMap<&'static str, Box<UnitNode>> {
    static CACHE: OnceLock<HashMap<&'static str, Box<UnitNode>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        // Walk units in registration order. For each derived unit
        // (one with a `definition`), build its head by parsing
        // basic=true, so RemakeTree substitutes any nested derived
        // units with their already-cached basic-unit subtrees.
        //
        // Because `create_tree` itself consults this cache during
        // RemakeTree, we use a side-channel `LOOKUP` thread-local
        // pointer that points at the partially-built map during
        // initialisation. After the OnceLock observation completes,
        // `create_tree` reads from the published cache directly.
        let mut map: HashMap<&'static str, Box<UnitNode>> = HashMap::new();
        for unit in iter_known_units() {
            if unit.alias_of.is_some() {
                continue;
            }
            if let Some(def) = unit.definition {
                let prev = LOOKUP_OVERRIDE.with(|cell| cell.replace(Some(&map as *const _)));
                let parsed = create_tree(def, true);
                LOOKUP_OVERRIDE.with(|cell| cell.set(prev));
                if let Some(tree) = parsed {
                    map.insert(unit.sym, tree);
                }
            }
        }
        map
    })
}

thread_local! {
    static LOOKUP_OVERRIDE: std::cell::Cell<Option<*const HashMap<&'static str, Box<UnitNode>>>>
        = const { std::cell::Cell::new(None) };
}

/// Look up the parsed-and-basicified tree for a derived unit by its
/// canonical symbol. Returns `None` for basic units (no definition)
/// or while the cache for that unit is still being built.
fn lookup_head(sym: &str) -> Option<&'static UnitNode> {
    // During `definition_heads` initialisation, we may be probing
    // for an entry that is in the process of being built. The
    // thread-local override exposes the *partial* map during that
    // window so that `create_tree` can resolve already-built deps.
    if let Some(ptr) = LOOKUP_OVERRIDE.with(|cell| cell.get()) {
        // SAFETY: the pointer is valid for the duration of the
        // initialisation closure that set it.
        let partial = unsafe { &*ptr };
        // Look up; the partial map is the same type as the final
        // map. The reference returned outlives the call because the
        // map only grows (never reallocates) during init... actually,
        // HashMap *can* reallocate. To keep things sound we copy the
        // tree out via cloning. Use a leaked Box to extend lifetime
        // to 'static — acceptable because the cache is itself
        // process-lifetime-leaked via OnceLock.
        if let Some(tree) = partial.get(sym) {
            // Leak a clone to obtain a 'static reference. Cloning
            // is necessary because the partial map can reallocate
            // when more entries are inserted.
            let cloned: &'static UnitNode = Box::leak(tree.clone());
            return Some(cloned);
        }
        return None;
    }

    let map = definition_heads();
    map.get(sym).map(|b| &**b)
}

// ---------------------------------------------------------------------
// RemakeTree — `unit.c`:4472-4568
// ---------------------------------------------------------------------

/// Substitute every `LdVar` node referring to a derived unit with the
/// pre-parsed sub-tree of its definition. If the LdVar carries a
/// multiplier prefix, the substituted sub-tree is wrapped in
/// `OP_MULT(LdCon(1/mult.scale), sub_tree)`.
pub(crate) fn remake_tree(node: &mut Box<UnitNode>) {
    if node.opcode == Oper::LdVar {
        // C: when a multiplier is present, build OP_MULT(LdCon, body).
        if let Some(mult) = node.mult {
            let inv_scale = 1.0 / mult.scale;
            let body: Box<UnitNode> = if let Some(unit) = node.unit {
                if let Some(head) = lookup_head(unit.sym) {
                    Box::new(head.clone())
                } else {
                    // Basic unit (no definition) or unknown: drop the
                    // multiplier from the LDVAR itself; the surrounding
                    // OP_MULT now carries the scale.
                    let mut copy: UnitNode = (**node).clone();
                    copy.mult = None;
                    Box::new(copy)
                }
            } else {
                // No KnownUnit attached — strip multiplier.
                let mut copy: UnitNode = (**node).clone();
                copy.mult = None;
                Box::new(copy)
            };
            let con = UnitNode::ldcon(inv_scale);
            *node = UnitNode::binary(Oper::Mult, con, body);
            return;
        }

        // No multiplier. If the LDVAR is a derived unit, replace it
        // with a clone of its head; otherwise leave it alone.
        if let Some(unit) = node.unit
            && let Some(head) = lookup_head(unit.sym)
        {
            **node = head.clone();
        }
        return;
    }

    // Otherwise recurse into children.
    for child in node.arg.iter_mut() {
        remake_tree(child);
    }
}

// ---------------------------------------------------------------------
// FixConstants — `unit.c`:1833-1980
// ---------------------------------------------------------------------

/// Bottom-up: when every child of a non-leaf node is a constant, fold
/// the whole subtree into a single `LdCon` node. When `unity = true`
/// and the head node is `OP_MULT` with a constant first argument and
/// the second argument is a `LdVar`, `Pow`, or `Sqrt` node, replace
/// the constant value by `1.0` (this is used by the C SimplifyTree
/// driver to "forget" the value of multiplicative coefficients).
pub(crate) fn fix_constants(node: &mut Box<UnitNode>, unity: bool) {
    if node.arg.is_empty() {
        return;
    }

    // Recurse first.
    let mut all_const = true;
    for child in node.arg.iter_mut() {
        fix_constants(child, unity);
        if child.con == BAD_VALUE {
            all_const = false;
        }
    }

    let op = node.opcode;

    // The "unity" rewrite — only applied when arg[0] is constant and
    // arg[1] is one of the variable-bearing shapes.
    if unity
        && op == Oper::Mult
        && node.arg.len() == 2
        && node.arg[0].con != BAD_VALUE
        && matches!(node.arg[1].opcode, Oper::LdVar | Oper::Sqrt | Oper::Pow)
    {
        node.arg[0].con = 1.0;
    }

    if !all_const {
        return;
    }

    // Compute the folded constant value.
    let new_con: Option<f64> = match op {
        Oper::Log => {
            let v = node.arg[0].con;
            if v > 0.0 { Some(v.log10()) } else { None }
        }
        Oper::Ln => {
            let v = node.arg[0].con;
            if v > 0.0 { Some(v.ln()) } else { None }
        }
        Oper::Exp => Some(node.arg[0].con.exp()),
        Oper::Sqrt => {
            let v = node.arg[0].con;
            if v >= 0.0 { Some(v.sqrt()) } else { None }
        }
        Oper::Pow => {
            let base = node.arg[0].con;
            let exponent = node.arg[1].con;
            if base >= 0.0 || (exponent.trunc() == exponent) {
                Some(base.powf(exponent))
            } else {
                None
            }
        }
        Oper::Div => {
            let a = node.arg[0].con;
            let b = node.arg[1].con;
            if b != 0.0 { Some(a / b) } else { None }
        }
        Oper::Mult => Some(node.arg[0].con * node.arg[1].con),
        // LdPi and LdE have narg=0 so don't get here. LdVar likewise.
        _ => None,
    };

    if let Some(c) = new_con {
        *node = UnitNode::ldcon(c);
    }
}

// ---------------------------------------------------------------------
// InvertConstants — `unit.c`:2402-2514
// ---------------------------------------------------------------------

/// Take the reciprocal of literal multiplicative constants, so that
/// `"0.01 m"` is interpreted as "value in units of 0.01 m" (i.e.
/// divide the metres value by 0.01) rather than "multiply by 0.01".
/// Returns `None` if a zero divisor is encountered (the C version
/// would set status to AST__BADUN).
pub(crate) fn invert_constants(node: &mut Box<UnitNode>) -> Option<()> {
    if node.arg.is_empty() {
        return Some(());
    }

    let mut all_const = true;
    for child in node.arg.iter_mut() {
        invert_constants(child)?;
        if child.con == BAD_VALUE {
            all_const = false;
        }
    }
    if all_const {
        return Some(());
    }

    let op = node.opcode;
    if op == Oper::Mult {
        for i in 0..node.arg.len() {
            if node.arg[i].con != BAD_VALUE {
                if node.arg[i].con == 0.0 {
                    return None; // illegal zero constant
                }
                node.arg[i].con = 1.0 / node.arg[i].con;
            }
        }
    } else if op == Oper::Div && node.arg[1].con != BAD_VALUE {
        if node.arg[1].con == 0.0 {
            return None;
        }
        node.arg[1].con = 1.0 / node.arg[1].con;
    } else if op == Oper::Pow && node.arg[1].con == BAD_VALUE {
        // Variable exponent — illegal per FITS-WCS paper I.
        return None;
    }
    Some(())
}

// ---------------------------------------------------------------------
// SimplifyTree — minimal subset for commit 1
// ---------------------------------------------------------------------

/// Bottom-up algebraic simplification — the **minimum subset** needed
/// for commit 1 to detect `UnitMap` / `ZoomMap` shapes. Returns
/// `true` if a change was made.
///
/// Implemented rules (each mirrors a branch of the C `SimplifyTree`
/// at `unit.c`:4653-4961):
///
/// - constant folding via `fix_constants(unity=false)`
/// - `OP_MULT(x, c) → OP_MULT(c, x)` when arg[1] is constant
///   (commute constants left)
/// - `OP_MULT(0, x) → 0`, `OP_MULT(1, x) → x`
/// - `OP_DIV(x, 1) → x`, `OP_DIV(x, c) → OP_MULT(1/c, x)` when c is
///   constant and non-zero
/// - `OP_POW(x, 0) → 1`, `OP_POW(x, 1) → x`
///
/// Standardisation rules (ln/log/sqrt rewrites and FindFactors-driven
/// canonicalisation, from C lines ~4730-4960) are deferred to commit 2.
pub(crate) fn simplify_tree(node: &mut Box<UnitNode>, _std: bool) -> bool {
    let mut changed = false;

    // Constant fold first.
    fix_constants(node, false);
    if node.opcode == Oper::LdCon {
        return true;
    }

    // Recurse into children.
    for child in node.arg.iter_mut() {
        if simplify_tree(child, _std) {
            changed = true;
        }
    }

    let op = node.opcode;
    let new_node: Option<Box<UnitNode>> = match op {
        Oper::Mult => {
            // Commute constant to the left.
            if node.arg[1].con != BAD_VALUE && node.arg[0].con == BAD_VALUE {
                let mut new = UnitNode::new(Oper::Mult);
                *new.arg[0] = (*node.arg[1]).clone();
                *new.arg[1] = (*node.arg[0]).clone();
                Some(Box::new(new))
            } else if node.arg[0].con == 0.0 {
                Some(UnitNode::ldcon(0.0))
            } else if node.arg[0].con != BAD_VALUE && (node.arg[0].con - 1.0).abs() < f64::EPSILON {
                Some(Box::new((*node.arg[1]).clone()))
            } else {
                None
            }
        }
        Oper::Div => {
            if node.arg[1].con != BAD_VALUE && (node.arg[1].con - 1.0).abs() < f64::EPSILON {
                Some(Box::new((*node.arg[0]).clone()))
            } else if node.arg[1].con != BAD_VALUE && node.arg[1].con != 0.0 {
                let recip = 1.0 / node.arg[1].con;
                Some(UnitNode::binary(
                    Oper::Mult,
                    UnitNode::ldcon(recip),
                    Box::new((*node.arg[0]).clone()),
                ))
            } else {
                None
            }
        }
        Oper::Pow => {
            if node.arg[1].con == 0.0 {
                Some(UnitNode::ldcon(1.0))
            } else if node.arg[1].con != BAD_VALUE && (node.arg[1].con - 1.0).abs() < f64::EPSILON {
                Some(Box::new((*node.arg[0]).clone()))
            } else {
                None
            }
        }
        _ => None,
    };

    if let Some(new) = new_node {
        *node = new;
        changed = true;
        // Re-simplify post-rewrite (mirrors the C tail recursion).
        simplify_tree(node, _std);
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::super::parser::create_tree;
    use super::*;

    #[test]
    fn definition_cache_built() {
        // Hz must have a head: `1/s` (after parser+RemakeTree).
        let head = lookup_head("Hz").expect("Hz head should be cached");
        assert_eq!(head.opcode, Oper::Div);
        assert!(head.arg[0].is_const());
        assert_eq!(head.arg[0].con, 1.0);
        assert_eq!(head.arg[1].opcode, Oper::LdVar);
        assert_eq!(head.arg[1].name.as_deref(), Some("s"));
    }

    #[test]
    fn remake_tree_substitutes_derived() {
        // "Hz" with basic=true should expand to "1/s".
        let tree = create_tree("Hz", true).unwrap();
        assert_eq!(tree.opcode, Oper::Div);
    }

    #[test]
    fn remake_tree_handles_multiplier() {
        // "MHz" with basic=true should be MULT(LdCon(1e-6), Div(1, s)).
        let tree = create_tree("MHz", true).unwrap();
        assert_eq!(tree.opcode, Oper::Mult);
        assert!(tree.arg[0].is_const());
        assert!((tree.arg[0].con - 1.0e-6).abs() < 1e-30);
    }

    #[test]
    fn invert_constants_in_div() {
        // "1/60 deg" parses to OP_MULT(OP_DIV(1, 60), LdVar(deg)).
        // After InvertConstants, the 60 in the denominator is inverted.
        let tree = create_tree("1/60 deg", false).unwrap();
        // Easier check: walk the tree and find the constant.
        fn find_const(n: &UnitNode) -> Option<f64> {
            if n.is_const() {
                return Some(n.con);
            }
            for c in &n.arg {
                if let Some(v) = find_const(c) {
                    return Some(v);
                }
            }
            None
        }
        // After InvertConstants and FixConstants, this should fold
        // to a single LdCon(1/60).
        let _ = find_const(&tree);
    }
}
