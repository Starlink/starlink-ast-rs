//! Top-level orchestration for `astUnitMapper_`. Mirrors C
//! `astUnitMapper_` (`unit.c`:5238-5732) and its helper
//! `MakeMapping` (`unit.c`:3227-3371).
//!
//! The algorithm proceeds in five phases:
//!
//! 1. Identical-string fast path (`UnitMap`).
//! 2. Parse both expressions with `create_tree(_, basic=true)` so all
//!    derived units have been substituted with their basic-unit
//!    definitions.
//! 3. Two-pass fixed-unit walk: for each basic unit name, run
//!    `FixUnits` on both trees, simplify with `std=1`, invert the
//!    masked input, concatenate with the masked output, simplify
//!    again. All passes must produce equivalent ("test tree") results
//!    for the conversion to be possible. (If the forward direction
//!    fails, retry with the trees swapped, then invert the resulting
//!    test tree — mirrors the C two-pass loop.)
//! 4. Dispatch the final test tree to `MakeMapping`, which emits a
//!    `UnitMap`, `ZoomMap`, or `MathMap` depending on the tree shape.

use crate::mappings::{DynMapping, MathMap, UnitMap, ZoomMap};

use super::cmp::cmp_tree;
use super::concat::concat_tree;
use super::dim_anal::{fix_units, locate_units};
use super::invert::invert_tree;
use super::parser::{clean_exp, create_tree};
use super::simplify::simplify_tree;
use super::to_expr::make_exp;
use super::tree::{Oper, UnitNode};

use crate::constants::BAD_VALUE;

/// Convert a final simplified `UnitNode` tree into a Mapping. Mirrors
/// C `MakeMapping` (`unit.c`:3227).
fn make_mapping(tree: &UnitNode) -> Option<DynMapping> {
    // Pure LDVAR → identity.
    if tree.opcode == Oper::LdVar {
        return Some(Box::new(UnitMap::new(1)));
    }

    // OP_MULT(LdCon(k), LdVar) → UnitMap (k≈1) or ZoomMap.
    if tree.opcode == Oper::Mult
        && tree.arg[0].con != BAD_VALUE
        && tree.arg[1].opcode == Oper::LdVar
    {
        let k = tree.arg[0].con;
        if k == 1.0 {
            return Some(Box::new(UnitMap::new(1)));
        }
        return ZoomMap::new(1, k).ok().map(|z| Box::new(z) as DynMapping);
    }

    // Otherwise build a MathMap. Format the tree as the forward
    // expression "output_units = <expr>" with `input_units` as the
    // varying variable. Then invert the tree (using `output_units` as
    // the new src) and format that as the inverse "input_units = <expr>".
    let fwd_expr = make_exp(tree);
    let mut src = UnitNode::new(Oper::LdVar);
    src.name = Some("output_units".to_string());
    let inv_tree = invert_tree(tree, Box::new(src))?;
    let inv_expr = make_exp(&inv_tree);

    let fwd_eq = format!("output_units={}", fwd_expr);
    let inv_eq = format!("input_units={}", inv_expr);

    let fwd_slice: &[&str] = &[&fwd_eq];
    let inv_slice: &[&str] = &[&inv_eq];
    let mut mathmap = MathMap::new(1, 1, fwd_slice, inv_slice).ok()?;
    mathmap.set_simp_fi(true);
    mathmap.set_simp_if(true);
    Some(Box::new(mathmap))
}

/// Run the per-unit fixed-mask loop. Returns the final test tree if
/// every basic unit produced an equivalent result, or `None` if any
/// pair failed.
fn build_test_tree(in_tree: &UnitNode, out_tree: &UnitNode) -> Option<Box<UnitNode>> {
    // Collect all distinct LdVar names in either tree.
    let mut names: Vec<String> = Vec::new();
    locate_units(in_tree, &mut names);
    locate_units(out_tree, &mut names);

    let mut testtree: Option<Box<UnitNode>> = None;

    for name in names.iter() {
        // Mask all units other than `name` to 1, then simplify(std=1).
        let mut intemp = fix_units(in_tree, name);
        let mut outtemp = fix_units(out_tree, name);
        simplify_tree(&mut intemp, true);
        simplify_tree(&mut outtemp, true);

        let in_const = intemp.con != BAD_VALUE;
        let out_const = outtemp.con != BAD_VALUE;

        if in_const && out_const {
            // Both are constants — this unit cancels in both
            // expressions; skip it.
            continue;
        }

        if in_const || out_const {
            // Only one tree depends on this unit — incompatible.
            return None;
        }

        // Build inverted intemp using "input_units" as the new leaf,
        // then concat with outtemp, then simplify.
        let mut src = UnitNode::new(Oper::LdVar);
        src.name = Some("input_units".to_string());
        let inv = invert_tree(&intemp, Box::new(src))?;
        let mut totaltree = concat_tree(&inv, &outtemp)?;
        simplify_tree(&mut totaltree, true);

        match testtree.as_ref() {
            None => testtree = Some(totaltree),
            Some(prev) => {
                if cmp_tree(&totaltree, prev, false) != 0 {
                    return None;
                }
            }
        }
    }

    testtree
}

/// A `Mapping` between two unit strings, and the label `in_lab` rewritten
/// for the new units, as C's `astUnitMapper` gives them when it is passed
/// an input label (unit.c:5349): the label unchanged when the unit strings
/// are identical or both blank, and otherwise the label tree joined to the
/// units tree and printed as a label. The label is `None` where C's is
/// NULL. `None` overall when no Mapping can be found, or when the label
/// cannot be parsed, which C reports as an error.
pub(crate) fn unit_mapper_label(
    in_unit: &str,
    out_unit: &str,
    in_lab: &str,
) -> Option<(DynMapping, Option<String>)> {
    if in_unit == out_unit {
        return Some((Box::new(UnitMap::new(1)), Some(in_lab.to_string())));
    }
    let in_tree = create_tree(in_unit, true);
    let out_tree = create_tree(out_unit, true);
    let (in_tree, out_tree) = match (in_tree, out_tree) {
        (None, None) => {
            return Some((Box::new(UnitMap::new(1)), Some(in_lab.to_string())));
        }
        (Some(i), Some(o)) => (i, o),
        _ => return None,
    };
    let (testtree, swapped) = match build_test_tree(&in_tree, &out_tree) {
        Some(t) => (t, false),
        None => (build_test_tree(&out_tree, &in_tree)?, true),
    };
    let final_tree = if swapped {
        let mut src = UnitNode::new(Oper::LdVar);
        src.name = Some("input_units".to_string());
        invert_tree(&testtree, Box::new(src))?
    } else {
        testtree
    };
    let map = make_mapping(&final_tree)?;
    let labtree = super::label::make_label_tree(in_lab)?;
    let mut total = concat_tree(&labtree, &final_tree)?;
    simplify_tree(&mut total, true);
    Some((map, super::label::make_label_exp(&total)))
}

/// Build a `Mapping` between two unit strings. Returns `None` if
/// the units are dimensionally incompatible, unparseable, or
/// uninvertible.
///
/// Mirrors C `astUnitMapper_` (`unit.c`:5238).
pub(crate) fn unit_mapper(in_unit: &str, out_unit: &str) -> Option<DynMapping> {
    // Strict identical-string fast path — `unit.c`:5468.
    if in_unit == out_unit {
        return Some(Box::new(UnitMap::new(1)));
    }

    // Equivalent normalised strings — also a UnitMap.
    let cleaned_in = clean_exp(in_unit);
    let cleaned_out = clean_exp(out_unit);
    if cleaned_in == cleaned_out {
        return Some(Box::new(UnitMap::new(1)));
    }

    // Parse both with basic-unit substitution.
    let in_tree = create_tree(in_unit, true)?;
    let out_tree = create_tree(out_unit, true)?;

    // Two-pass loop: try forward direction first; if that fails, swap
    // and invert. Mirrors `unit.c`:5533-5688.
    let (testtree, swapped) = match build_test_tree(&in_tree, &out_tree) {
        Some(t) => (t, false),
        None => {
            let t = build_test_tree(&out_tree, &in_tree)?;
            (t, true)
        }
    };

    // If we swapped, invert the test tree before constructing the Mapping.
    let final_tree = if swapped {
        let mut src = UnitNode::new(Oper::LdVar);
        src.name = Some("input_units".to_string());
        invert_tree(&testtree, Box::new(src))?
    } else {
        testtree
    };

    make_mapping(&final_tree)
}
