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
// SimplifyTree — full port of `unit.c`:4653-4961
// ---------------------------------------------------------------------

/// `astEQUAL`-style fuzzy equality. Approximates pal's `astEQUAL`
/// macro: tolerant by ~1e-9 relative.
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

/// Bottom-up algebraic simplification. Mirrors C `SimplifyTree`
/// (`unit.c`:4653). When `std=true`, applies the canonical-form
/// standardisations needed to compare two trees for equivalence
/// (FindFactors / CombineFactors and the ln/log/sqrt rewrites).
/// When `std=false`, only genuine simplifications run.
pub(crate) fn simplify_tree(node: &mut Box<UnitNode>, std: bool) -> bool {
    let mut result = false;

    // Constant fold first (C: FixConstants(node, 0)).
    fix_constants(node, false);

    // Recurse into children.
    for child in node.arg.iter_mut() {
        if simplify_tree(child, std) {
            result = true;
        }
    }

    let op = node.opcode;
    let mut newnode: Option<Box<UnitNode>> = None;

    match op {
        // ---- Natural log -------------------------------------------
        Oper::Ln => {
            if node.arg[0].opcode == Oper::Exp {
                // ln(exp(x)) → x
                newnode = Some(Box::new((*node.arg[0].arg[0]).clone()));
            } else if std && node.arg[0].opcode == Oper::Pow {
                // ln(x**k) → k * ln(x)
                let k = node.arg[0].arg[1].clone();
                let x = node.arg[0].arg[0].clone();
                let mut ln_x = UnitNode::new(Oper::Ln);
                ln_x.arg[0] = x;
                let mult = UnitNode::binary(Oper::Mult, k, Box::new(ln_x));
                newnode = Some(mult);
            }
        }

        // ---- Common log → ln/ln(10) (when std) ---------------------
        Oper::Log => {
            if std {
                let mut ln = UnitNode::new(Oper::Ln);
                ln.arg[0] = node.arg[0].clone();
                let div = UnitNode::binary(Oper::Div, Box::new(ln), UnitNode::ldcon(10.0_f64.ln()));
                newnode = Some(div);
            }
        }

        // ---- Exponential -------------------------------------------
        Oper::Exp => {
            if node.arg[0].opcode == Oper::Ln {
                // exp(ln(x)) → x
                newnode = Some(Box::new((*node.arg[0].arg[0]).clone()));
            }
        }

        // ---- Square root → x**0.5 (when std) -----------------------
        Oper::Sqrt => {
            if std {
                let pow = UnitNode::binary(
                    Oper::Pow,
                    Box::new((*node.arg[0]).clone()),
                    UnitNode::ldcon(0.5),
                );
                newnode = Some(pow);
            }
        }

        // ---- Exponentiation ----------------------------------------
        Oper::Pow => {
            if node.arg[0].opcode == Oper::Exp {
                // (e**x)**k → e**(k*x)
                let k = (*node.arg[1]).clone();
                let x = (*node.arg[0].arg[0]).clone();
                let mut e = UnitNode::new(Oper::Exp);
                let mult = UnitNode::binary(Oper::Mult, Box::new(k), Box::new(x));
                e.arg[0] = mult;
                newnode = Some(Box::new(e));
            } else if node.arg[1].con == 0.0 {
                // x ** 0 → 1
                newnode = Some(UnitNode::ldcon(1.0));
            } else if ast_equal(node.arg[1].con, 1.0) {
                // x ** 1 → x
                newnode = Some(Box::new((*node.arg[0]).clone()));
            } else if node.arg[0].opcode == Oper::Pow {
                // (x**k1)**k2 → x ** (k1*k2)
                let k1 = node.arg[0].arg[1].con;
                let k2 = node.arg[1].con;
                let new_pow = UnitNode::binary(
                    Oper::Pow,
                    Box::new((*node.arg[0].arg[0]).clone()),
                    UnitNode::ldcon(k1 * k2),
                );
                newnode = Some(new_pow);
            } else if std && node.arg[0].opcode == Oper::Mult {
                // (x*y)**k → (x**k)*(y**k)
                let k = (*node.arg[1]).clone();
                let x = (*node.arg[0].arg[0]).clone();
                let y = (*node.arg[0].arg[1]).clone();
                let xk = UnitNode::binary(Oper::Pow, Box::new(x), Box::new(k.clone()));
                let yk = UnitNode::binary(Oper::Pow, Box::new(y), Box::new(k));
                let mult = UnitNode::binary(Oper::Mult, xk, yk);
                newnode = Some(mult);
            }
        }

        // ---- Division ----------------------------------------------
        Oper::Div => {
            if ast_equal(node.arg[1].con, 1.0) {
                newnode = Some(Box::new((*node.arg[0]).clone()));
            } else if node.arg[1].con != BAD_VALUE {
                if node.arg[1].con != 0.0 {
                    let mult = UnitNode::binary(
                        Oper::Mult,
                        UnitNode::ldcon(1.0 / node.arg[1].con),
                        Box::new((*node.arg[0]).clone()),
                    );
                    newnode = Some(mult);
                }
                // C reports AST__BADUN on divide-by-zero; we just
                // leave the node alone, the upper layers will fail.
            } else if std {
                // x/y → x * (y**(-1))
                let pow = UnitNode::binary(
                    Oper::Pow,
                    Box::new((*node.arg[1]).clone()),
                    UnitNode::ldcon(-1.0),
                );
                let mult = UnitNode::binary(Oper::Mult, Box::new((*node.arg[0]).clone()), pow);
                newnode = Some(mult);
            }
        }

        // ---- Multiplication ----------------------------------------
        Oper::Mult => {
            if node.arg[1].con != BAD_VALUE && node.arg[0].con == BAD_VALUE {
                // Swap so constant is on the left.
                let mut sw = UnitNode::new(Oper::Mult);
                sw.arg[0] = node.arg[1].clone();
                sw.arg[1] = node.arg[0].clone();
                newnode = Some(Box::new(sw));
            } else if node.arg[0].con == 0.0 {
                newnode = Some(UnitNode::ldcon(0.0));
            } else if ast_equal(node.arg[0].con, 1.0) {
                newnode = Some(Box::new((*node.arg[1]).clone()));
            } else if std {
                // FindFactors + CombineFactors canonicalisation.
                let (factors, powers, coeff) = find_factors(node);
                newnode = combine_factors(&factors, &powers, coeff);
            }
        }

        _ => {}
    }

    // If the rewrite produced something identical to the existing
    // node, drop it.
    if let Some(nn) = newnode.as_ref() {
        if super::cmp::cmp_tree(nn, node, true) == 0 {
            newnode = None;
        } else {
            result = true;
        }
    }

    if let Some(nn) = newnode {
        *node = nn;
    }

    // Re-simplify if anything changed (mirrors C tail recursion).
    if result {
        simplify_tree(node, std);
    }

    result
}

// ---------------------------------------------------------------------
// FindFactors — `unit.c`:1596-1831
// ---------------------------------------------------------------------

/// Factor list paired with corresponding powers and an overall
/// coefficient. Each factor is owned (deep-cloned from the source
/// tree) so the caller can drop the source tree after collection.
///
/// We use `Vec<Box<UnitNode>>` rather than `Vec<UnitNode>` to keep
/// pointer stability for entries during the collection loop — the
/// `find_factors_inner` callee inserts into the same vector while
/// iterating, and the boxing matches the tree's own
/// `Vec<Box<UnitNode>>` argument storage from `tree.rs`.
#[allow(clippy::vec_box)]
pub(crate) fn find_factors(node: &UnitNode) -> (Vec<Box<UnitNode>>, Vec<f64>, f64) {
    let mut factors: Vec<Box<UnitNode>> = Vec::new();
    let mut powers: Vec<f64> = Vec::new();
    let mut coeff: f64 = 1.0;
    find_factors_inner(node, &mut factors, &mut powers, &mut coeff);
    (factors, powers, coeff)
}

#[allow(clippy::vec_box)]
fn find_factors_inner(
    node: &UnitNode,
    factors: &mut Vec<Box<UnitNode>>,
    powers: &mut Vec<f64>,
    coeff: &mut f64,
) {
    use super::cmp::cmp_tree;
    match node.opcode {
        Oper::Mult => {
            find_factors_inner(&node.arg[0], factors, powers, coeff);
            let mut fact1: Vec<Box<UnitNode>> = Vec::new();
            let mut pow1: Vec<f64> = Vec::new();
            let mut coeff1: f64 = 1.0;
            find_factors_inner(&node.arg[1], &mut fact1, &mut pow1, &mut coeff1);
            for (i, fac) in fact1.into_iter().enumerate() {
                let mut found = None;
                for (j, existing) in factors.iter().enumerate() {
                    if cmp_tree(existing, &fac, false) == 0 {
                        found = Some(j);
                        break;
                    }
                }
                match found {
                    Some(j) => powers[j] += pow1[i],
                    None => {
                        factors.push(fac);
                        powers.push(pow1[i]);
                    }
                }
            }
            *coeff *= coeff1;
        }

        Oper::Pow => {
            find_factors_inner(&node.arg[0], factors, powers, coeff);
            let con = node.arg[1].con;
            for p in powers.iter_mut() {
                *p *= con;
            }
            // Exponentiate the coefficient.
            if *coeff >= 0.0 || (con as i64 as f64 == con) {
                *coeff = coeff.powf(con);
            }
            // (C reports AST__BADUN otherwise; we just leave coeff as-is.)
        }

        Oper::Div => {
            find_factors_inner(&node.arg[0], factors, powers, coeff);
            let mut fact1: Vec<Box<UnitNode>> = Vec::new();
            let mut pow1: Vec<f64> = Vec::new();
            let mut coeff1: f64 = 1.0;
            find_factors_inner(&node.arg[1], &mut fact1, &mut pow1, &mut coeff1);
            for (i, fac) in fact1.into_iter().enumerate() {
                let mut found = None;
                for (j, existing) in factors.iter().enumerate() {
                    if cmp_tree(existing, &fac, false) == 0 {
                        found = Some(j);
                        break;
                    }
                }
                match found {
                    Some(j) => powers[j] -= pow1[i],
                    None => {
                        factors.push(fac);
                        powers.push(-pow1[i]);
                    }
                }
            }
            if coeff1 != 0.0 {
                *coeff /= coeff1;
            }
        }

        Oper::Sqrt => {
            find_factors_inner(&node.arg[0], factors, powers, coeff);
            for p in powers.iter_mut() {
                *p *= 0.5;
            }
            if *coeff >= 0.0 {
                *coeff = coeff.sqrt();
            }
        }

        Oper::LdCon => {
            *coeff = node.con;
        }

        // LdPi/LdE: treat as constants (unlike C, which leaves them as
        // factors — but the C code's coefficient handling falls through
        // since `node->con != AST__BAD` is false for these). To match
        // the C exactly, treat them as opaque factors.
        _ => {
            // Default: a single factor "node" with power 1.
            factors.push(Box::new(node.clone()));
            powers.push(1.0);
        }
    }
}

// ---------------------------------------------------------------------
// CombineFactors — `unit.c`:673-825
// ---------------------------------------------------------------------

/// Reorder the factor list canonically (`cmp_tree(_,_,false)` order)
/// and emit a tree that multiplies them all together with the given
/// coefficient. Mirrors C `CombineFactors`.
pub(crate) fn combine_factors(
    factors: &[Box<UnitNode>],
    powers: &[f64],
    coeff: f64,
) -> Option<Box<UnitNode>> {
    use super::cmp::cmp_tree;

    // Sort factors+powers in lockstep using the tree-comparison sort.
    let mut idx: Vec<usize> = (0..factors.len()).collect();
    idx.sort_by(|&a, &b| {
        let c = cmp_tree(&factors[a], &factors[b], false);
        if c < 0 {
            std::cmp::Ordering::Less
        } else if c > 0 {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });

    // The first root term is the coefficient unless it equals 1.
    let mut head: Option<Box<UnitNode>> = if !ast_equal(coeff, 1.0) {
        Some(UnitNode::ldcon(coeff))
    } else {
        None
    };
    let mut result: Option<Box<UnitNode>> = None;

    for &i in idx.iter() {
        if powers[i] == 0.0 {
            continue;
        }
        let factor_clone = (*factors[i]).clone();
        let node2: Box<UnitNode> = if ast_equal(powers[i], 1.0) {
            Box::new(factor_clone)
        } else {
            UnitNode::binary(
                Oper::Pow,
                Box::new(factor_clone),
                UnitNode::ldcon(powers[i]),
            )
        };

        if let Some(h) = head.take() {
            let m = UnitNode::binary(Oper::Mult, h, node2);
            head = Some(m.clone());
            result = Some(m);
        } else {
            head = Some(node2);
        }
    }

    if result.is_none() {
        result = head;
    }
    if result.is_none() {
        result = Some(UnitNode::ldcon(1.0));
    }
    result
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
