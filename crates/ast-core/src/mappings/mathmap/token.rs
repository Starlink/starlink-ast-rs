//! Lexer/tokenizer for MathMap expressions.
//!
//! The tokenizer processes a cleaned (lowercase, no whitespace) expression
//! string and produces a stream of tokens for the parser.
//!
//! # Design choice: hand-rolled vs `logos`
//!
//! The design spec called for using the `logos` crate (MIT/Apache-2.0,
//! derive-macro-based DFA lexer). A hand-rolled tokenizer was chosen
//! instead because the MathMap token grammar has properties that are
//! awkward to express in logos's regex-driven model:
//!
//! - **Angle-bracket system constants** (`<bad>`, `<pi>`, `<max_10_exp>`)
//!   require context-sensitive longest-match against a fixed set.
//! - **Dotted Fortran-style logical operators** (`.and.`, `.or.`, `.not.`,
//!   `.eqv.`, `.xor.`) overlap with the `.` in floating-point literals.
//! - **Function recognition** requires longest-match among ~50 names that
//!   share common prefixes (`sin`/`sinc`/`sind`/`sinh`, `a`/`abs`/`acos`/
//!   `acosd`/`acosh`/`acoth`/`acsch`/`asech`/`asin`/...).
//! - The input is pre-cleaned (lowercased, whitespace-stripped) so there
//!   is no benefit from logos's skip/whitespace handling.
//!
//! **If revisiting this decision**, `logos` would provide:
//! - Compile-time DFA generation (faster tokenization for large expressions)
//! - Declarative token definitions (easier to audit against the C spec)
//! - Automatic longest-match semantics
//!
//! The trade-off is manageable either way — the hand-rolled version is
//! ~200 lines and the MathMap expression language is frozen (no new tokens
//! will be added).

use crate::error::{AstError, AstResult, ErrorCode};

/// A token in a MathMap expression.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Token {
    // Literals
    Number(f64),
    Variable(usize), // 0-based index into variable list

    // System constants
    Bad,
    Dig,
    Epsilon,
    MantDig,
    Max,
    Max10Exp,
    MaxExp,
    Min,
    Min10Exp,
    MinExp,
    Radix,
    Rounds,
    ConstE,
    ConstPi,

    // Unary functions (1 arg)
    Abs,
    Acos,
    Acosd,
    Acosh,
    Acoth,
    Acsch,
    Asech,
    Asin,
    Asind,
    Asinh,
    Atan,
    Atand,
    Atanh,
    Ceil,
    Cos,
    Cosd,
    Cosh,
    Coth,
    Csch,
    Exp,
    Floor,
    Int,
    IsBad,
    Log,
    Log10,
    Nint,
    Poisson,
    Sech,
    Sin,
    Sinc,
    Sind,
    Sinh,
    Sqr,
    Sqrt,
    Tan,
    Tand,
    Tanh,

    // Binary functions (2 args)
    Atan2,
    Atan2d,
    Dim,
    Gauss,
    FMod,
    Pow,
    Rand,
    Sign,

    // Ternary function
    Qif,

    // Variable-arg functions
    FnMax,
    FnMin,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    DoubleStar,
    ShiftLeft,
    ShiftRight,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    EqEq,
    NotEq,
    Ampersand,
    Pipe,
    Caret,
    AndAnd,
    OrOr,
    CaretCaret,
    DotEqv,
    DotNeqv,
    DotXor,
    Bang,
    DotNot,

    // Delimiters
    LParen,
    RParen,
    Comma,

    // End of expression
    Eof,
}

/// Tokenize a cleaned expression string.
///
/// `expr` must be lowercase with no whitespace.
/// `variables` is the list of defined variable names (lowercase, no whitespace).
pub(crate) fn tokenize(expr: &str, variables: &[&str]) -> AstResult<Vec<Token>> {
    let bytes = expr.as_bytes();
    let mut tokens = Vec::new();
    let mut pos = 0;

    while pos < bytes.len() {
        let ch = bytes[pos];

        // Try system constants first (they start with '<')
        if ch == b'<' {
            if let Some((tok, end)) = try_system_constant(expr, pos) {
                tokens.push(tok);
                pos = end;
                continue;
            }
        }

        // Try functions (alphabetic start, followed by '(')
        if ch.is_ascii_alphabetic() || ch == b'.' {
            // Try dotted operators first (.and., .or., .not., etc.)
            if ch == b'.' {
                if let Some((tok, end)) = try_dot_operator(expr, pos) {
                    tokens.push(tok);
                    pos = end;
                    continue;
                }
            }

            if ch.is_ascii_alphabetic() {
                // Try function name (ends with '(')
                if let Some((tok, end)) = try_function(expr, pos) {
                    tokens.push(tok);
                    pos = end;
                    continue;
                }

                // Try variable name
                if let Some((tok, end)) = try_variable(expr, pos, variables) {
                    tokens.push(tok);
                    pos = end;
                    continue;
                }

                return Err(AstError::new(
                    ErrorCode::Udvof,
                    format!(
                        "undefined variable or function in expression \"{}\" at position {}",
                        expr, pos
                    ),
                ));
            }
        }

        // Try numeric constant
        if ch.is_ascii_digit() || ch == b'.' {
            let (val, end) = parse_number(expr, pos)?;
            tokens.push(Token::Number(val));
            pos = end;
            continue;
        }

        // Operators and punctuation
        match ch {
            b'(' => {
                tokens.push(Token::LParen);
                pos += 1;
            }
            b')' => {
                tokens.push(Token::RParen);
                pos += 1;
            }
            b',' => {
                tokens.push(Token::Comma);
                pos += 1;
            }
            b'+' => {
                tokens.push(Token::Plus);
                pos += 1;
            }
            b'-' => {
                tokens.push(Token::Minus);
                pos += 1;
            }
            b'*' => {
                if pos + 1 < bytes.len() && bytes[pos + 1] == b'*' {
                    tokens.push(Token::DoubleStar);
                    pos += 2;
                } else {
                    tokens.push(Token::Star);
                    pos += 1;
                }
            }
            b'/' => {
                tokens.push(Token::Slash);
                pos += 1;
            }
            b'<' => {
                if pos + 1 < bytes.len() && bytes[pos + 1] == b'<' {
                    tokens.push(Token::ShiftLeft);
                    pos += 2;
                } else if pos + 1 < bytes.len() && bytes[pos + 1] == b'=' {
                    tokens.push(Token::LessEq);
                    pos += 2;
                } else {
                    tokens.push(Token::Less);
                    pos += 1;
                }
            }
            b'>' => {
                if pos + 1 < bytes.len() && bytes[pos + 1] == b'>' {
                    tokens.push(Token::ShiftRight);
                    pos += 2;
                } else if pos + 1 < bytes.len() && bytes[pos + 1] == b'=' {
                    tokens.push(Token::GreaterEq);
                    pos += 2;
                } else {
                    tokens.push(Token::Greater);
                    pos += 1;
                }
            }
            b'=' => {
                if pos + 1 < bytes.len() && bytes[pos + 1] == b'=' {
                    tokens.push(Token::EqEq);
                    pos += 2;
                } else {
                    return Err(AstError::new(
                        ErrorCode::Miopr,
                        format!("unexpected '=' in expression \"{}\"", expr),
                    ));
                }
            }
            b'!' => {
                if pos + 1 < bytes.len() && bytes[pos + 1] == b'=' {
                    tokens.push(Token::NotEq);
                    pos += 2;
                } else {
                    tokens.push(Token::Bang);
                    pos += 1;
                }
            }
            b'&' => {
                if pos + 1 < bytes.len() && bytes[pos + 1] == b'&' {
                    tokens.push(Token::AndAnd);
                    pos += 2;
                } else {
                    tokens.push(Token::Ampersand);
                    pos += 1;
                }
            }
            b'|' => {
                if pos + 1 < bytes.len() && bytes[pos + 1] == b'|' {
                    tokens.push(Token::OrOr);
                    pos += 2;
                } else {
                    tokens.push(Token::Pipe);
                    pos += 1;
                }
            }
            b'^' => {
                if pos + 1 < bytes.len() && bytes[pos + 1] == b'^' {
                    tokens.push(Token::CaretCaret);
                    pos += 2;
                } else {
                    tokens.push(Token::Caret);
                    pos += 1;
                }
            }
            _ => {
                return Err(AstError::new(
                    ErrorCode::Miopr,
                    format!(
                        "unexpected character '{}' in expression \"{}\"",
                        ch as char, expr
                    ),
                ));
            }
        }
    }

    tokens.push(Token::Eof);
    Ok(tokens)
}

fn try_system_constant(expr: &str, pos: usize) -> Option<(Token, usize)> {
    let rest = &expr[pos..];
    let constants: &[(&str, Token)] = &[
        ("<bad>", Token::Bad),
        ("<dig>", Token::Dig),
        ("<epsilon>", Token::Epsilon),
        ("<mant_dig>", Token::MantDig),
        ("<max>", Token::Max),
        ("<max_10_exp>", Token::Max10Exp),
        ("<max_exp>", Token::MaxExp),
        ("<min>", Token::Min),
        ("<min_10_exp>", Token::Min10Exp),
        ("<min_exp>", Token::MinExp),
        ("<radix>", Token::Radix),
        ("<rounds>", Token::Rounds),
        ("<e>", Token::ConstE),
        ("<pi>", Token::ConstPi),
    ];
    // Match longest first
    let mut best: Option<(Token, usize)> = None;
    for (text, tok) in constants {
        if rest.starts_with(text) && best.as_ref().is_none_or(|(_, len)| text.len() > *len) {
            best = Some((tok.clone(), text.len()));
        }
    }
    best.map(|(tok, len)| (tok, pos + len))
}

fn try_dot_operator(expr: &str, pos: usize) -> Option<(Token, usize)> {
    let rest = &expr[pos..];
    let ops: &[(&str, Token)] = &[
        (".and.", Token::AndAnd),
        (".or.", Token::OrOr),
        (".not.", Token::DotNot),
        (".eqv.", Token::DotEqv),
        (".neqv.", Token::DotNeqv),
        (".xor.", Token::DotXor),
        (".lt.", Token::Less),
        (".le.", Token::LessEq),
        (".gt.", Token::Greater),
        (".ge.", Token::GreaterEq),
        (".eq.", Token::EqEq),
        (".ne.", Token::NotEq),
    ];
    let mut best: Option<(Token, usize)> = None;
    for (text, tok) in ops {
        if rest.starts_with(text) && best.as_ref().is_none_or(|(_, len)| text.len() > *len) {
            best = Some((tok.clone(), text.len()));
        }
    }
    best.map(|(tok, len)| (tok, pos + len))
}

fn try_function(expr: &str, pos: usize) -> Option<(Token, usize)> {
    let rest = &expr[pos..];
    // Functions end with '(' — match longest
    let functions: &[(&str, Token)] = &[
        ("abs(", Token::Abs),
        ("acos(", Token::Acos),
        ("acosd(", Token::Acosd),
        ("acosh(", Token::Acosh),
        ("acoth(", Token::Acoth),
        ("acsch(", Token::Acsch),
        ("aint(", Token::Int),
        ("asech(", Token::Asech),
        ("asin(", Token::Asin),
        ("asind(", Token::Asind),
        ("asinh(", Token::Asinh),
        ("atan(", Token::Atan),
        ("atand(", Token::Atand),
        ("atanh(", Token::Atanh),
        ("atan2(", Token::Atan2),
        ("atan2d(", Token::Atan2d),
        ("ceil(", Token::Ceil),
        ("cos(", Token::Cos),
        ("cosd(", Token::Cosd),
        ("cosh(", Token::Cosh),
        ("coth(", Token::Coth),
        ("csch(", Token::Csch),
        ("dim(", Token::Dim),
        ("exp(", Token::Exp),
        ("fabs(", Token::Abs),
        ("floor(", Token::Floor),
        ("fmod(", Token::FMod),
        ("gauss(", Token::Gauss),
        ("int(", Token::Int),
        ("isbad(", Token::IsBad),
        ("log(", Token::Log),
        ("log10(", Token::Log10),
        ("max(", Token::FnMax),
        ("min(", Token::FnMin),
        ("mod(", Token::FMod),
        ("nint(", Token::Nint),
        ("poisson(", Token::Poisson),
        ("pow(", Token::Pow),
        ("qif(", Token::Qif),
        ("rand(", Token::Rand),
        ("sech(", Token::Sech),
        ("sign(", Token::Sign),
        ("sin(", Token::Sin),
        ("sinc(", Token::Sinc),
        ("sind(", Token::Sind),
        ("sinh(", Token::Sinh),
        ("sqr(", Token::Sqr),
        ("sqrt(", Token::Sqrt),
        ("tan(", Token::Tan),
        ("tand(", Token::Tand),
        ("tanh(", Token::Tanh),
    ];
    let mut best: Option<(Token, usize)> = None;
    for (text, tok) in functions {
        if rest.starts_with(text) && best.as_ref().is_none_or(|(_, len)| text.len() > *len) {
            best = Some((tok.clone(), text.len()));
        }
    }
    // For functions, we consume the name including the '(' —
    // but we need the parser to see it as a function call followed by paren
    best.map(|(tok, len)| (tok, pos + len))
}

fn try_variable(expr: &str, pos: usize, variables: &[&str]) -> Option<(Token, usize)> {
    // Parse a name: starts with alpha, continues with alnum or underscore
    let bytes = expr.as_bytes();
    if pos >= bytes.len() || !bytes[pos].is_ascii_alphabetic() {
        return None;
    }
    let mut end = pos + 1;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    let name = &expr[pos..end];

    // Look up in variable list
    for (idx, &var) in variables.iter().enumerate() {
        if var == name {
            return Some((Token::Variable(idx), end));
        }
    }
    None
}

fn parse_number(expr: &str, pos: usize) -> AstResult<(f64, usize)> {
    let bytes = expr.as_bytes();
    let mut end = pos;
    let mut has_digit = false;
    let mut has_dot = false;
    let mut in_exponent = false;
    let mut exp_sign = false;
    let mut exp_digit = false;

    while end < bytes.len() {
        let ch = bytes[end];
        if ch.is_ascii_digit() {
            has_digit = true;
            if in_exponent {
                exp_digit = true;
            }
            end += 1;
        } else if ch == b'.' && !has_dot && !in_exponent {
            has_dot = true;
            end += 1;
        } else if (ch == b'e' || ch == b'd') && !in_exponent && has_digit {
            in_exponent = true;
            end += 1;
        } else if (ch == b'+' || ch == b'-') && in_exponent && !exp_sign && !exp_digit {
            exp_sign = true;
            end += 1;
        } else {
            break;
        }
    }

    if !has_digit || (in_exponent && !exp_digit) {
        return Err(AstError::new(
            ErrorCode::Conin,
            format!("invalid constant syntax in expression \"{}\"", &expr[..end]),
        ));
    }

    // Replace 'd' with 'e' for Rust parsing
    let num_str: String = expr[pos..end]
        .chars()
        .map(|c| if c == 'd' { 'e' } else { c })
        .collect();

    let val: f64 = num_str.parse().map_err(|_| {
        AstError::new(
            ErrorCode::Conin,
            format!("invalid constant in expression: \"{}\"", num_str),
        )
    })?;

    Ok((val, end))
}
