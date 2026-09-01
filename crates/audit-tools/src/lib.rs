//! Source citations for the `MapMerge` / `Simplify` audit checklists.
//!
//! A checklist row cites a source line, and a line number rots as the file
//! around it changes. Recording the enclosing item's name beside the number
//! makes the rot detectable: the number and the name have to still agree.
//! This crate holds the one derivation of that name, so the tool that writes
//! the citations and the test that checks them cannot disagree about it.

/// A parsed `file:line[-line] [(fn name)]` citation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    /// File name as written, without a directory.
    pub file: String,
    /// First line of the cited range, 1-based.
    pub start: usize,
    /// Last line of the cited range, when the citation names one.
    pub end: Option<usize>,
    /// The item name the citation claims, when it carries one.
    pub item: Option<String>,
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Parse the first citation in `text`.
pub fn parse_citation(text: &str) -> Option<Citation> {
    for (colon, _) in text.match_indices(':') {
        let stem: String = text[..colon]
            .chars()
            .rev()
            .take_while(|c| is_ident(*c) || *c == '.')
            .collect();
        let file: String = stem.chars().rev().collect();
        if !(file.ends_with(".rs") || file.ends_with(".c") || file.ends_with(".h")) {
            continue;
        }
        let after = &text[colon + 1..];
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        let Ok(start) = digits.parse::<usize>() else {
            continue;
        };
        let mut rest = &after[digits.len()..];
        let mut end = None;
        if let Some(tail) = rest.strip_prefix('-') {
            let d: String = tail.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(n) = d.parse::<usize>() {
                end = Some(n);
                rest = &tail[d.len()..];
            }
        }
        let item = rest
            .trim_start()
            .strip_prefix("(fn ")
            .and_then(|t| t.split_once(')'))
            .map(|(name, _)| name.trim().to_string());
        return Some(Citation {
            file,
            start,
            end,
            item,
        });
    }
    None
}

/// Strip line comments and string / char literal bodies so their braces do not
/// count as block structure.
fn code_only(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let bytes: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            '/' if bytes.get(i + 1) == Some(&'/') => break,
            '"' => {
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        '\\' => i += 2,
                        '"' => break,
                        _ => i += 1,
                    }
                }
                i += 1;
            }
            '\'' if bytes.get(i + 1) == Some(&'{') || bytes.get(i + 1) == Some(&'}') => i += 3,
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn rust_fn_name(code: &str) -> Option<String> {
    let at = code
        .find("fn ")
        .filter(|at| *at == 0 || !code[..*at].chars().next_back().is_some_and(is_ident))?;
    let name: String = code[at + 3..]
        .chars()
        .take_while(|c| is_ident(*c))
        .collect();
    (!name.is_empty()).then_some(name)
}

/// The name of the `fn` whose body contains `line` (1-based).
pub fn enclosing_rust_item(src: &str, line: usize) -> Option<String> {
    let mut stack: Vec<(String, usize)> = Vec::new();
    let mut depth = 0usize;
    let mut pending: Option<String> = None;
    for (i, text) in src.lines().enumerate() {
        let code = code_only(text);
        if let Some(name) = rust_fn_name(&code) {
            pending = Some(name);
        }
        for c in code.chars() {
            match c {
                '{' => {
                    depth += 1;
                    if let Some(name) = pending.take() {
                        stack.push((name, depth));
                    }
                }
                '}' => {
                    if stack.last().is_some_and(|(_, d)| *d == depth) {
                        stack.pop();
                    }
                    depth = depth.saturating_sub(1);
                }
                // A signature with no body, as in a trait declaration.
                ';' => pending = None,
                _ => {}
            }
        }
        if i + 1 == line {
            return stack.last().map(|(name, _)| name.clone());
        }
    }
    None
}

/// The name of the C function whose body contains `line` (1-based).
pub fn enclosing_c_function(src: &str, line: usize) -> Option<String> {
    c_functions(src)
        .into_iter()
        .find(|(_, start, end)| *start <= line && line <= *end)
        .map(|(name, _, _)| name)
}

/// Every function definition in `src`, as `(name, first line, last line)`.
///
/// AST's house style puts a definition's declarator at column 0 and its closing
/// brace alone at column 0, so a definition is delimited without parsing C.
pub fn c_functions(src: &str) -> Vec<(String, usize, usize)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        match c_definition_name(&lines, i) {
            Some(name) => {
                let mut j = i + 1;
                while j < lines.len() && lines[j] != "}" {
                    j += 1;
                }
                out.push((name, i + 1, j + 1));
                i = j + 1;
            }
            None => i += 1,
        }
    }
    out
}

/// The name of the function defined at `lines[i]`, if a definition starts there.
///
/// What separates a definition from the prototype of the same name earlier in
/// the file is where the declarator ends: a definition reaches `{` with its
/// parameter list balanced, a prototype reaches `;`.
fn c_definition_name(lines: &[&str], i: usize) -> Option<String> {
    if !lines[i].chars().next().is_some_and(is_ident) {
        return None;
    }
    let open = lines[i].find('(')?;
    let mut depth = 0i32;
    let mut opened = false;
    'scan: for text in lines.iter().skip(i).take(9) {
        for c in text.chars() {
            match c {
                '(' => {
                    depth += 1;
                    opened = true;
                }
                ')' => depth -= 1,
                '{' if opened && depth == 0 => break 'scan,
                ';' if opened && depth == 0 => return None,
                _ => {}
            }
        }
    }
    let ident: String = lines[i][..open]
        .trim_end()
        .chars()
        .rev()
        .take_while(|c| is_ident(*c))
        .collect();
    (!ident.is_empty()).then(|| ident.chars().rev().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Line numbers are 1-based and the assertions below depend on them:
    // 4 `fn outer`, 7 `42`, 13 `fn other`, 14 its body, 18 `fn free`, 19 its body.
    const RUST: &str = "\
struct A;

impl A {
    fn outer(&self) -> u32 {
        let s = \"} not a brace\";
        if true {
            42
        } else {
            0
        }
    }

    fn other(&self) {
        let _ = 1;
    }
}

fn free() -> u32 {
    1
}
";

    #[test]
    fn finds_the_enclosing_fn_through_nested_braces_and_strings() {
        // Inside `outer`'s `if`.
        assert_eq!(enclosing_rust_item(RUST, 7).as_deref(), Some("outer"));
        // A `}` inside a string literal must not close a block.
        assert_eq!(enclosing_rust_item(RUST, 5).as_deref(), Some("outer"));
        assert_eq!(enclosing_rust_item(RUST, 14).as_deref(), Some("other"));
        assert_eq!(enclosing_rust_item(RUST, 19).as_deref(), Some("free"));
    }

    #[test]
    fn a_line_outside_every_fn_has_no_enclosing_item() {
        assert_eq!(enclosing_rust_item(RUST, 1), None);
        assert_eq!(enclosing_rust_item(RUST, 3), None);
    }

    const C: &str = "\
/* Prototype. */
static int MapMerge( AstMapping *, int, int * );

/* Body. */
static int MapMerge( AstMapping *this, int where,
                     int *status ) {

/* Local Variables: */
   int result;

/* Check the global error status. */
   if ( !astOK ) return 0;

   return result;
}

static void Other( void ) {
   return;
}
";

    #[test]
    fn finds_the_c_definition_not_the_prototype() {
        assert_eq!(enclosing_c_function(C, 12).as_deref(), Some("MapMerge"));
        assert_eq!(enclosing_c_function(C, 2), None);
        assert_eq!(enclosing_c_function(C, 18).as_deref(), Some("Other"));
    }

    #[test]
    fn parses_all_three_citation_forms() {
        let c = parse_citation("polygon.rs:1158").unwrap();
        assert_eq!(
            (c.file.as_str(), c.start, c.end, c.item),
            ("polygon.rs", 1158, None, None)
        );
        let c = parse_citation("polygon.rs:1158-1262").unwrap();
        assert_eq!((c.start, c.end), (1158, Some(1262)));
        let c = parse_citation("polygon.rs:1158 (fn simplified)").unwrap();
        assert_eq!(c.item.as_deref(), Some("simplified"));
        assert!(parse_citation("prose with no citation").is_none());
    }
}
