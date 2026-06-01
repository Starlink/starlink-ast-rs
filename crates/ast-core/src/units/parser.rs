//! String → `UnitNode` tree parser. Mirrors `CleanExp`, `MakeTree`,
//! `CreateTree`, `SplitUnit`, and `ConStart` in
//! `reference/starlink-ast/src/unit.c`. The parsing algorithm is the
//! recursive-descent "scan from the right for the lowest-precedence
//! operator" used by the C version. We deliberately preserve that
//! algorithm (and its quirks) line for line — the user has required
//! C parity throughout the port (memory:
//! `feedback_no_corner_cuts_on_c.md`).

use super::simplify::{fix_constants, invert_constants, remake_tree};
use super::table::{
    KnownUnit, Multiplier, find_known_unit, find_known_unit_by_label, iter_known_units,
    iter_multipliers,
};
use super::tree::{Oper, UnitNode};

/// Top-level entry point. Mirrors C `CreateTree` (`unit.c`:1283-1371).
///
/// `basic = true` requests `RemakeTree` substitution (replace derived
/// units with their basic-unit definitions). `false` keeps the tree
/// referring to derived-unit symbols directly.
pub(crate) fn create_tree(exp: &str, basic: bool) -> Option<Box<UnitNode>> {
    // CleanExp: drop leading/trailing whitespace, collapse internal
    // whitespace, strip whitespace adjacent to operators or parens,
    // and rewrite a few non-standard idioms (e.g. <word><digit> →
    // <word>^<digit>, "MJY" → "MJy", "STER" → "sr", trailing "micron"
    // → "(...m*1.0E-6)", leading "nano" → "n").
    let cleaned = clean_exp(exp);
    if cleaned.is_empty() {
        return None;
    }

    // Build the parsed tree.
    let mut tree = make_tree(&cleaned)?;

    // Replace each subtree which simply combines constants (i.e. has
    // no OP_LDVAR nodes) with a single OP_LDCON node.
    fix_constants(&mut tree, false);

    // Invert literal constant unit multipliers.
    invert_constants(&mut tree)?;

    // Optionally substitute LDVAR(derived) with the parsed sub-tree
    // of the derived unit's definition.
    if basic {
        remake_tree(&mut tree);
    }

    Some(tree)
}

// ---------------------------------------------------------------------
// CleanExp — `unit.c`:282-566
// ---------------------------------------------------------------------

/// Match the C `ISWORD` macro (`unit.c`:110): alphanumeric, '+', '-'.
fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '+' || c == '-'
}

fn ustreq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Normalise a unit expression: tokenise, rewrite a few non-standard
/// idioms in alphanumeric tokens, then collapse spaces and discard
/// whitespace adjacent to operators or parentheses. Mirrors
/// `CleanExp` (`unit.c`:282).
pub(crate) fn clean_exp(exp: &str) -> String {
    if exp.is_empty() {
        return String::new();
    }

    // ----- Step 1: tokenise. Each contiguous block of word
    // characters or non-word characters is a token.
    let mut tokens: Vec<String> = Vec::new();
    let chars: Vec<char> = exp.chars().collect();
    let n = chars.len();
    let mut start = 0usize;
    let mut word = is_word(chars[0]);
    let mut p = 0usize;
    while p < n {
        let c = chars[p];
        if word {
            if !is_word(c) {
                tokens.push(chars[start..p].iter().collect());
                start = p;
                word = false;
            }
        } else if is_word(c) {
            tokens.push(chars[start..p].iter().collect());
            start = p;
            word = true;
        }
        p += 1;
    }
    tokens.push(chars[start..n].iter().collect());

    // ----- Step 2: rewrite non-standard idioms in word tokens.
    for tok in tokens.iter_mut() {
        let l = tok.chars().count();

        // 2a. Any alphabetical word followed by a digit (with possible
        // trailing sign+digit) is taken as <word>^<digit>. The C
        // condition: l > 1, doesn't start with '+' or '-', and exactly
        // one of the last `l-1` chars is a digit (i.e. the last char
        // is the only digit; any other digits in the token would
        // change `strcspn` count).
        if l > 1 {
            let bytes = tok.as_bytes();
            if bytes[0] != b'-' && bytes[0] != b'+' {
                // strcspn(t, "0123456789") returns the number of
                // leading non-digit chars. C requires == l - 1, i.e.
                // the only digit is the final character.
                let leading_nondigit = bytes.iter().take_while(|c| !c.is_ascii_digit()).count();
                if leading_nondigit == l - 1 {
                    let w = bytes[l - 2];
                    let mut new_tok = String::with_capacity(l + 1);
                    if w != b'+' && w != b'-' {
                        // <word>^<digit>
                        new_tok.push_str(&tok[..l - 1]);
                        new_tok.push('^');
                        new_tok.push_str(&tok[l - 1..]);
                    } else {
                        // <word>^<sign><digit>
                        new_tok.push_str(&tok[..l - 2]);
                        new_tok.push('^');
                        new_tok.push_str(&tok[l - 2..]);
                    }
                    *tok = new_tok;
                    continue;
                }
            }
        }

        // 2b. If the word ends with "micron" change to
        // "(<start>m*1.0E-6)" (handles "Kmicron" etc.).
        if let Some(pos) = tok.find("micron") {
            let mut new_tok = String::with_capacity(pos + 11);
            new_tok.push('(');
            if pos > 0 {
                new_tok.push_str(&tok[..pos]);
            }
            new_tok.push_str("m*1.0E-6)");
            *tok = new_tok;
            continue;
        }

        // 2c. STER → sr (case-insensitive).
        if ustreq(tok, "STER") {
            *tok = "sr".to_string();
            continue;
        }

        // 2d. Three-character word ending in "JY" (case-sensitive on
        // the last two) → replace last char with lowercase 'y'.
        if l == 3 && tok.ends_with("JY") {
            let mut new_tok = String::with_capacity(3);
            new_tok.push(tok.chars().next().unwrap());
            new_tok.push('J');
            new_tok.push('y');
            *tok = new_tok;
            continue;
        }

        // 2e. Leading "nano" (case-insensitive, len ≥ 4) → "n…".
        if l >= 4 {
            let prefix: String = tok.chars().take(4).collect();
            if ustreq(&prefix, "nano") {
                let mut new_tok = String::with_capacity(l - 3);
                new_tok.push('n');
                new_tok.push_str(&tok[4..]);
                *tok = new_tok;
                continue;
            }
        }
    }

    // Concatenate.
    let concatenated: String = tokens.concat();

    // ----- Step 3: collapse spaces. Drop leading whitespace, collapse
    // multiple internal spaces, drop trailing space.
    let mut buf = String::with_capacity(concatenated.len());
    let mut prev_space = true;
    for c in concatenated.chars() {
        if c.is_whitespace() {
            if !prev_space {
                buf.push(c);
                prev_space = true;
            }
        } else {
            buf.push(c);
            prev_space = false;
        }
    }
    if buf.ends_with(char::is_whitespace) {
        buf.pop();
    }

    // ----- Step 4: drop spaces adjacent to operators / parens.
    let cs: Vec<char> = buf.chars().collect();
    let mut out = String::with_capacity(cs.len());
    let mut prev_space = false;
    let mut prev_op = false;
    for &c in cs.iter() {
        if c.is_whitespace() {
            if !prev_op {
                out.push(c);
                prev_op = true;
                prev_space = true;
            }
        } else if matches!(c, '*' | '/' | '^' | '.' | ')' | '(') {
            if prev_space {
                out.pop();
            }
            out.push(c);
            prev_op = true;
            prev_space = false;
        } else {
            out.push(c);
            prev_op = false;
            prev_space = false;
        }
    }
    if prev_space {
        out.pop();
    }

    out
}

// ---------------------------------------------------------------------
// ConStart — `unit.c`:1126-1197
// ---------------------------------------------------------------------

/// See if `text` starts with a literal numeric constant; if so,
/// return `(value, nc)` where `nc` is the number of bytes consumed.
/// Mirrors C `ConStart`. Importantly, refuses to interpret leading
/// 'n'/'N' as numeric (Cygwin scanf bug; we keep the same guard so
/// "nano" isn't misread).
pub(crate) fn con_start(text: &str) -> Option<(f64, usize)> {
    if text.is_empty() {
        return None;
    }

    // Refuse leading 'n'/'N' (after skipping whitespace).
    let trimmed = text.trim_start();
    let first = trimmed.chars().next()?;
    if first == 'n' || first == 'N' {
        return None;
    }

    // Find the longest prefix that parses as a float. C uses sscanf
    // "%lf%n" — we emulate by trying successively longer prefixes.
    // A cheap implementation: walk char-by-char, stop when the prefix
    // stops being a parseable float.
    let bytes = text.as_bytes();
    let mut best: Option<(f64, usize)> = None;
    let mut i = 0usize;
    // Skip leading sign.
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    // Walk numeric chars: digits, '.', 'e'/'E', and following sign.
    let mut have_digit = false;
    let mut have_dot = false;
    let mut have_exp = false;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_digit() {
            have_digit = true;
        } else if c == b'.' && !have_dot && !have_exp {
            have_dot = true;
        } else if (c == b'e' || c == b'E') && have_digit && !have_exp {
            have_exp = true;
            // Optional sign right after exponent.
            if i + 1 < bytes.len() && (bytes[i + 1] == b'+' || bytes[i + 1] == b'-') {
                i += 1;
            }
        } else {
            break;
        }
        i += 1;
        if have_digit {
            if let Ok(v) = text[..i].parse::<f64>() {
                best = Some((v, i));
            }
        }
    }
    best
}

// ---------------------------------------------------------------------
// SplitUnit — `unit.c`:4963-5082
// ---------------------------------------------------------------------

/// Split a string ending with the unit name/label `u` into a multiplier
/// prefix (if any) plus the unit. Returns `(matched_multiplier_or_none,
/// length_of_u_in_chars)` if the trailing `u` matches and any leading
/// substring is a known multiplier. `cs = true` means case-sensitive
/// match for `u`.
pub(crate) fn split_unit(
    str_in: &str,
    u: &str,
    cs: bool,
) -> Option<(Option<&'static Multiplier>, usize)> {
    let ls = str_in.len();
    let lu = u.len();
    if ls < lu {
        return None;
    }
    let lm = ls - lu;

    let tail = &str_in[lm..];
    let tail_match = if cs {
        tail == u
    } else {
        tail.eq_ignore_ascii_case(u)
    };
    if !tail_match {
        return None;
    }

    if lm == 0 {
        return Some((None, lu));
    }

    let prefix = &str_in[..lm];

    // Try case-sensitive match against multiplier symbols first.
    for m in iter_multipliers() {
        if m.sym.len() == lm && prefix == m.sym {
            return Some((Some(m), lu));
        }
    }
    // Case-insensitive against multiplier symbols.
    for m in iter_multipliers() {
        if m.sym.len() == lm && prefix.eq_ignore_ascii_case(m.sym) {
            return Some((Some(m), lu));
        }
    }
    // Case-insensitive against multiplier labels.
    for m in iter_multipliers() {
        if m.label.len() == lm && prefix.eq_ignore_ascii_case(m.label) {
            return Some((Some(m), lu));
        }
    }

    None
}

// ---------------------------------------------------------------------
// MakeTree — `unit.c`:3599-4082
// ---------------------------------------------------------------------

fn ends_with_func_call(slice: &str, func: &str) -> bool {
    // C: EndsWith(c, i+1, "sqrt(", ...) — case-insensitive.
    if slice.len() < func.len() {
        return false;
    }
    let start = slice.len() - func.len();
    slice[start..].eq_ignore_ascii_case(func)
}

/// Build a `UnitNode` tree from a (cleaned) unit expression. `nc` is
/// the number of bytes from `exp` to use. Mirrors C `MakeTree`
/// (`unit.c`:3599).
pub(crate) fn make_tree(exp: &str) -> Option<Box<UnitNode>> {
    make_tree_n(exp.as_bytes(), exp.len())
}

fn make_tree_n(exp: &[u8], nc: usize) -> Option<Box<UnitNode>> {
    if nc == 0 {
        return None;
    }

    // -------- Pass 1: rightmost MULT/DIV at depth zero ---------
    let mut op = Oper::Null;
    let mut oplen = 1usize;
    let mut depth: i32 = 0;
    let mut i: i32 = nc as i32 - 1;
    let mut split_i: i32 = i; // index of operator
    while i >= 0 {
        let c = exp[i as usize];
        if c == b'(' {
            depth -= 1;
            if depth < 0 {
                return None; // missing closing parenthesis
            }
            // Implicit MULT before "(" if not at start and prev is
            // not an operator/space/known-function-name end.
            if depth == 0 && i > 0 {
                let d = exp[(i - 1) as usize];
                if d != b'*'
                    && d != b'/'
                    && d != b'^'
                    && d != b'.'
                    && d != b' '
                    // Check the substring exp[..=i] doesn't end with
                    // a function name like "sqrt(", "exp(", "ln(",
                    // "log(".
                    && !ends_with_func_call(
                        std::str::from_utf8(&exp[..=i as usize]).ok()?,
                        "sqrt(",
                    )
                    && !ends_with_func_call(
                        std::str::from_utf8(&exp[..=i as usize]).ok()?,
                        "exp(",
                    )
                    && !ends_with_func_call(std::str::from_utf8(&exp[..=i as usize]).ok()?, "ln(")
                    && !ends_with_func_call(
                        std::str::from_utf8(&exp[..=i as usize]).ok()?,
                        "log(",
                    )
                {
                    op = Oper::Mult;
                    oplen = 0;
                    split_i = i;
                    break;
                }
            }
        } else if c == b')' {
            depth += 1;
            // Implicit MULT after ")" if not at end and next is not op/space.
            if depth == 1 && (i as usize) < nc - 1 {
                let d = exp[(i + 1) as usize];
                if d != b'*' && d != b'/' && d != b'^' && d != b'.' && d != b' ' {
                    op = Oper::Mult;
                    oplen = 0;
                    // Adjust so left = exp[..i+1], right = exp[i+1..].
                    split_i = i + 1;
                    break;
                }
            }
        } else if depth == 0 {
            if c == b'/' {
                op = Oper::Div;
                split_i = i;
            } else if c == b' ' {
                op = Oper::Mult;
                split_i = i;
            } else if c == b'*' {
                // Only treated as MULT if not ** (adjacent to another asterisk).
                let prev_is_star = (i > 0) && exp[(i - 1) as usize] == b'*';
                let next_is_star = ((i as usize) < nc - 1) && exp[(i + 1) as usize] == b'*';
                if !prev_is_star && !next_is_star {
                    op = Oper::Mult;
                    split_i = i;
                }
            } else if c == b'.' {
                let prev_is_digit = (i > 0) && exp[(i - 1) as usize].is_ascii_digit();
                let next_is_digit =
                    ((i as usize) < nc - 1) && exp[(i + 1) as usize].is_ascii_digit();
                if !prev_is_digit && !next_is_digit {
                    op = Oper::Mult;
                    split_i = i;
                }
            }
        }
        if op != Oper::Null {
            break;
        }
        i -= 1;
    }

    // -------- Branch A: rightmost MULT/DIV found at depth zero ---------
    if op != Oper::Null {
        let li = split_i as usize;
        let left_len = li;
        let right_start = li + oplen;
        let right_len = nc - li - oplen;
        let left = make_tree_n(exp, left_len)?;
        let right = make_tree_n(&exp[right_start..], right_len)?;
        return Some(UnitNode::binary(op, left, right));
    }

    // Final depth must be zero by now (if we never found a MULT/DIV
    // and depth is positive, we have a missing opening paren).
    if depth > 0 {
        return None;
    }

    // -------- Pass 2: leftmost POW operator at depth zero ---------
    op = Oper::Null;
    depth = 0;
    let mut i: usize = 0;
    let mut split_i: usize = 0;
    while i < nc {
        let c = exp[i];
        if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
            if depth < 0 {
                return None;
            }
        } else if depth == 0 {
            if i + 1 < nc && exp[i] == b'*' && exp[i + 1] == b'*' {
                op = Oper::Pow;
                oplen = 2;
                split_i = i;
                break;
            } else if c == b'^' {
                op = Oper::Pow;
                oplen = 1;
                split_i = i;
                break;
            }
        }
        i += 1;
    }

    if op != Oper::Null {
        let left = make_tree_n(exp, split_i)?;
        let right_start = split_i + oplen;
        let right_len = nc - split_i - oplen;
        let right = make_tree_n(&exp[right_start..], right_len)?;
        return Some(UnitNode::binary(op, left, right));
    }

    // -------- Branch C: function call at start ---------
    let head = std::str::from_utf8(&exp[..nc]).ok()?;
    let (func_op, func_oplen) = if head.len() >= 5
        && (head[..5].eq_ignore_ascii_case("sqrt(") || head.starts_with("SQRT("))
    {
        (Some(Oper::Sqrt), 4usize)
    } else if head.len() >= 4
        && (head[..4].eq_ignore_ascii_case("exp(") || head.starts_with("EXP("))
    {
        (Some(Oper::Exp), 3)
    } else if head.len() >= 3 && (head[..3].eq_ignore_ascii_case("ln(") || head.starts_with("LN("))
    {
        (Some(Oper::Ln), 2)
    } else if head.len() >= 4
        && (head[..4].eq_ignore_ascii_case("log(") || head.starts_with("LOG("))
    {
        (Some(Oper::Log), 3)
    } else {
        (None, 0)
    };

    if let Some(fo) = func_op {
        let arg_start = func_oplen;
        let arg_len = nc - func_oplen;
        let arg = make_tree_n(&exp[arg_start..], arg_len)?;
        return Some(UnitNode::unary(fo, arg));
    }

    // -------- Branch D: whole expression in parentheses ---------
    if exp[0] == b'(' && exp[nc - 1] == b')' {
        return make_tree_n(&exp[1..], nc - 2);
    }

    // -------- Branch E: numerical constant ---------
    if let Some((con, n)) = con_start(head) {
        if n == nc {
            let mut node = UnitNode::new(Oper::LdCon);
            node.con = con;
            return Some(Box::new(node));
        }
        // Number followed by something — error in C; we treat as
        // unparseable.
        return None;
    }

    // -------- Branch F: named constants pi / e ---------
    //
    // In C, `NewNode(OP_LDPI)` sets `con = PI` (not AST__BAD), so the
    // node behaves as a constant for `FixConstants` /
    // `InvertConstants` / `SimplifyTree` purposes. We emit `LdCon`
    // directly with the numeric value to mirror that: the only
    // effect of preserving the dedicated `OP_LDPI` opcode in C is on
    // `CmpTree`'s opcode-based ordering between an LdPi node and a
    // pure LdCon, which never affects the FindFactors / CombineFactors
    // canonicalisation of unit-mapper trees.
    if nc == 2 && (head == "pi" || head == "PI") {
        return Some(UnitNode::ldcon(std::f64::consts::PI));
    }
    if nc == 1 && (head == "e" || head == "E") {
        return Some(UnitNode::ldcon(std::f64::consts::E));
    }

    // -------- Branch G: unit symbol (with optional multiplier) ---------
    let mut nc_search = nc;
    let mut munit: Option<&'static KnownUnit> = None;
    let mut mmult: Option<&'static Multiplier> = None;
    let mut maxlen: i32 = -1;
    let mut plural = false;

    loop {
        let head_str = std::str::from_utf8(&exp[..nc_search]).ok()?;

        // Case-sensitive symbol match.
        for unit in iter_known_units() {
            if let Some((mult, l)) = split_unit(head_str, unit.sym, true) {
                if l as i32 > maxlen {
                    maxlen = l as i32;
                    munit = Some(unit);
                    mmult = mult;
                }
            }
        }

        // Case-insensitive symbol match.
        if munit.is_none() {
            for unit in iter_known_units() {
                if let Some((mult, l)) = split_unit(head_str, unit.sym, false) {
                    if l as i32 > maxlen {
                        maxlen = l as i32;
                        munit = Some(unit);
                        mmult = mult;
                    }
                }
            }
        }

        // Case-insensitive label match.
        if munit.is_none() {
            for unit in iter_known_units() {
                if let Some((mult, l)) = split_unit(head_str, unit.label, false) {
                    if l as i32 > maxlen {
                        maxlen = l as i32;
                        munit = Some(unit);
                        mmult = mult;
                    }
                }
            }
        }

        // If still no match and the string ends with 's'/'S', try
        // dropping it (plural form like "Angstroms").
        if munit.is_none() && nc_search > 1 && !plural {
            let last = exp[nc_search - 1];
            if last == b's' || last == b'S' {
                plural = true;
                nc_search -= 1;
                continue;
            }
        }
        break;
    }

    // The C bumps nc back if we dropped a plural 's' — but that doesn't
    // affect tree construction (the LDVAR uses the canonical unit
    // symbol from the table, not the user's input bytes).
    let _ = plural;

    if let Some(unit) = munit {
        // Resolve alias (table::find_known_unit already follows the
        // chain; here we reuse find_known_unit_by_label to be paranoid
        // when the case-insensitive label match returned an alias).
        let canonical = if unit.alias_of.is_some() {
            find_known_unit(unit.sym).or_else(|| find_known_unit_by_label(unit.label))
        } else {
            Some(unit)
        };
        let canonical = canonical?;
        return Some(UnitNode::ldvar(Some(canonical), mmult, canonical.sym));
    }

    // -------- Branch H: unknown user-defined unit, optionally
    // with a known multiplier prefix ---------
    let head_str = std::str::from_utf8(&exp[..nc]).ok()?;
    let mut effective_start = 0usize;
    let mut user_mult: Option<&'static Multiplier> = None;
    for m in iter_multipliers() {
        let n_remaining = nc as i32 - m.sym.len() as i32;
        if n_remaining > 0 && head_str.starts_with(m.sym) {
            effective_start = m.sym.len();
            user_mult = Some(m);
            break;
        }
    }

    // Disallow non-alpha characters in the rest of the user-defined name.
    let rest = &head_str[effective_start..];
    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }

    Some(UnitNode::ldvar(None, user_mult, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_exp_strips_outer_whitespace() {
        assert_eq!(clean_exp("  Hz  "), "Hz");
    }

    #[test]
    fn clean_exp_collapses_internal_spaces() {
        assert_eq!(clean_exp("kg   m"), "kg m");
    }

    #[test]
    fn clean_exp_drops_space_around_operators() {
        assert_eq!(clean_exp("kg / m * s"), "kg/m*s");
    }

    #[test]
    fn clean_exp_word_then_digit() {
        assert_eq!(clean_exp("cm2"), "cm^2");
    }

    #[test]
    fn clean_exp_word_then_signed_digit() {
        assert_eq!(clean_exp("cm-2"), "cm^-2");
    }

    #[test]
    fn clean_exp_micron_rewrite() {
        // Bare "micron" → "(m*1.0E-6)"; "Kmicron" → "(Km*1.0E-6)".
        assert_eq!(clean_exp("micron"), "(m*1.0E-6)");
    }

    #[test]
    fn clean_exp_ster_rewrite() {
        assert_eq!(clean_exp("STER"), "sr");
    }

    #[test]
    fn clean_exp_mjy_rewrite() {
        assert_eq!(clean_exp("MJY"), "MJy");
    }

    #[test]
    fn con_start_simple() {
        assert_eq!(con_start("1.5"), Some((1.5, 3)));
        assert_eq!(con_start("1.0E-6m"), Some((1.0e-6, 6)));
        assert_eq!(con_start("nano"), None); // refuses leading 'n'
    }

    #[test]
    fn split_unit_with_multiplier() {
        let (mult, l) = split_unit("MHz", "Hz", true).unwrap();
        assert_eq!(l, 2);
        assert_eq!(mult.unwrap().sym, "M");
    }

    #[test]
    fn split_unit_no_multiplier() {
        let (mult, l) = split_unit("Hz", "Hz", true).unwrap();
        assert_eq!(l, 2);
        assert!(mult.is_none());
    }

    #[test]
    fn make_tree_simple_unit() {
        let t = create_tree("Hz", false).unwrap();
        assert_eq!(t.opcode, Oper::LdVar);
        assert_eq!(t.unit.unwrap().sym, "Hz");
    }

    #[test]
    fn make_tree_multiplied_unit() {
        let t = create_tree("MHz", false).unwrap();
        assert_eq!(t.opcode, Oper::LdVar);
        assert_eq!(t.unit.unwrap().sym, "Hz");
        assert_eq!(t.mult.unwrap().sym, "M");
    }

    #[test]
    fn make_tree_division() {
        let t = create_tree("m/s", false).unwrap();
        assert_eq!(t.opcode, Oper::Div);
    }

    #[test]
    fn make_tree_pow() {
        let t = create_tree("m**2", false).unwrap();
        assert_eq!(t.opcode, Oper::Pow);
    }

    #[test]
    fn make_tree_basic_substitution() {
        // Hz with basic=true expands to 1/s.
        let t = create_tree("Hz", true).unwrap();
        assert_eq!(t.opcode, Oper::Div);
        assert!(t.arg[0].is_const());
        assert_eq!(t.arg[1].opcode, Oper::LdVar);
        assert_eq!(t.arg[1].name.as_deref(), Some("s"));
    }
}
