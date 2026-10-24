//! Tree concatenation. Mirrors C `ConcatTree` (`unit.c`:1022-1124) and
//! the helper `ReplaceNode` (`unit.c`:4570-4651).
//!
//! `concat_tree(tree1, tree2)` returns a new tree obtained by feeding
//! the output of `tree1` (the head node) into the (single) `LdVar`
//! input of `tree2`.

use super::dim_anal::locate_units;
use super::tree::{Oper, UnitNode};

/// Replace, within `target`, the first occurrence of an `LdVar` whose
/// name equals `old_name` with a clone of `new_node`. Mirrors C
/// `ReplaceNode` (`unit.c`:4570). Returns `true` if a replacement
/// happened.
///
/// We match by `name` rather than by pointer-identity (which the C
/// version uses) because we don't carry stable pointers across the
/// `Box` clones in `LocateUnits`. Since `concat_tree` only invokes
/// `LocateUnits` to confirm there is exactly one `LdVar` and then
/// uses *that* name to drive the substitution, name-based matching
/// is equivalent to the C pointer-equality check.
fn replace_ldvar_in_place(target: &mut UnitNode, old_name: &str, new_node: &UnitNode) -> bool {
    for child in target.arg.iter_mut() {
        if child.opcode == Oper::LdVar && child.name.as_deref() == Some(old_name) {
            **child = new_node.clone();
            return true;
        }
        if replace_ldvar_in_place(child, old_name, new_node) {
            return true;
        }
    }
    false
}

/// Concatenate two trees. The output of `tree1` (head node) is
/// substituted into the (single) `LdVar` input of `tree2`. Mirrors
/// C `ConcatTree` (`unit.c`:1022).
///
/// - If `tree2` has no `LdVar` nodes, returns `None`.
/// - If `tree2` has more than one `LdVar`, returns `None` (the C
///   library reports an `AST__INTER` error here; we propagate failure).
/// - If `tree2` is itself a single `LdVar`, returns a clone of `tree1`.
pub(crate) fn concat_tree(tree1: &UnitNode, tree2: &UnitNode) -> Option<Box<UnitNode>> {
    // Find the unique LdVar name in tree2.
    let mut names: Vec<String> = Vec::new();
    locate_units(tree2, &mut names);
    if names.is_empty() {
        return None;
    }
    if names.len() > 1 {
        return None;
    }
    let target_name = &names[0];

    // If tree2 itself is the target LdVar, just clone tree1.
    if tree2.opcode == Oper::LdVar && tree2.name.as_deref() == Some(target_name.as_str()) {
        return Some(Box::new(tree1.clone()));
    }

    let mut result = Box::new(tree2.clone());
    if !replace_ldvar_in_place(&mut result, target_name, tree1) {
        return None;
    }
    Some(result)
}
