// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Enumerating the steps of a C seam body from the source.
//!
//! AST's house style precedes every step inside a function body with a comment
//! block starting at column 0. Deriving the steps from those comments keeps the
//! audit's universe of steps out of any hand-maintained list: a step that is
//! present in C and absent from a checklist is then a detectable omission
//! rather than an invisible one.

use crate::c_functions;

/// The line and text of each column-0 comment block inside `function`'s body.
///
/// A block's text is joined onto one line with runs of whitespace collapsed, so
/// a comment that wraps in the source compares as what it says.
pub fn c_step_comments(src: &str, function: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = src.lines().collect();
    let Some((_, start, end)) = c_functions(src)
        .into_iter()
        .find(|(name, _, _)| name == function)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut i = start; // Skip the declarator line itself.
    while i + 1 < end {
        let text = lines[i];
        if !text.starts_with("/*") {
            i += 1;
            continue;
        }
        let first = i;
        let mut body = String::new();
        loop {
            body.push(' ');
            body.push_str(lines[i]);
            if lines[i].contains("*/") || i + 1 >= end {
                break;
            }
            i += 1;
        }
        let cleaned = body
            .trim()
            .trim_start_matches("/*")
            .trim_end_matches("*/")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        // AST opens a function body with its own prologue: a `*+`-marked block,
        // or one whose first field is `* Name:`. It documents the function
        // rather than naming a step in it.
        if !cleaned.starts_with("*+") && !cleaned.starts_with("* Name:") {
            out.push((first + 1, cleaned));
        }
        i += 1;
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;

    const C: &str = "\
/* Not inside anything. */
static int MapMerge( AstMapping *this, int *status ) {

/* Local Variables: */
   int result;

/* Check the global error status. */
   if ( !astOK ) return 0;

/* Combine the two Mappings, if
   they can be combined. */
   result = 1;

   return result;
}

/* Also not inside anything. */
";

    #[test]
    fn enumerates_the_step_comments_of_one_definition() {
        let steps = c_step_comments(C, "MapMerge");
        assert_eq!(
            steps,
            vec![
                (4, "Local Variables:".to_string()),
                (7, "Check the global error status.".to_string()),
                (
                    10,
                    "Combine the two Mappings, if they can be combined.".to_string()
                ),
            ]
        );
    }

    const C_WITH_PROLOGUE: &str = "\
static int MapMerge( AstMapping *this, int *status ) {
/*
*  Name:
*     MapMerge
*  Purpose:
*     Simplify a sequence of Mappings.
*/

/* Check the global error status. */
   if ( !astOK ) return 0;
   return 1;
}
";

    #[test]
    fn the_functions_own_prologue_is_not_a_step() {
        assert_eq!(
            c_step_comments(C_WITH_PROLOGUE, "MapMerge"),
            vec![(9, "Check the global error status.".to_string())]
        );
    }

    #[test]
    fn an_unknown_function_has_no_steps() {
        assert!(c_step_comments(C, "Simplify").is_empty());
    }
}
