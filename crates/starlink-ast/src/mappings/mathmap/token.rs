// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

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
//! - **Dotted Fortran-style logical operators** (`.and.`, `.or.`, `.not.`, `.eqv.`, `.xor.`) overlap with the `.` in floating-point literals; which reading applies depends on whether an operator or an operand is expected.
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

/// The symbols that may follow an operand: they read as operators from the left.
const OPERATORS: &[(&str, Token)] = &[
    (")", Token::RParen),
    ("**", Token::DoubleStar),
    ("*", Token::Star),
    ("/", Token::Slash),
    ("+", Token::Plus),
    ("-", Token::Minus),
    ("<<", Token::ShiftLeft),
    (">>", Token::ShiftRight),
    ("<", Token::Less),
    (".lt.", Token::Less),
    ("<=", Token::LessEq),
    (".le.", Token::LessEq),
    (">", Token::Greater),
    (".gt.", Token::Greater),
    (">=", Token::GreaterEq),
    (".ge.", Token::GreaterEq),
    ("==", Token::EqEq),
    (".eq.", Token::EqEq),
    ("!=", Token::NotEq),
    (".ne.", Token::NotEq),
    ("&", Token::Ampersand),
    ("^", Token::Caret),
    ("|", Token::Pipe),
    ("&&", Token::AndAnd),
    (".and.", Token::AndAnd),
    ("^^", Token::CaretCaret),
    ("||", Token::OrOr),
    (".or.", Token::OrOr),
    (".eqv.", Token::DotEqv),
    (".neqv.", Token::DotNeqv),
    (".xor.", Token::DotXor),
    (",", Token::Comma),
];

/// The symbols that may start an operand: constants, function calls (the name with its opening parenthesis), an opening parenthesis and the prefix operators.
const OPERANDS: &[(&str, Token)] = &[
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
    ("ceil(", Token::Ceil),
    ("cos(", Token::Cos),
    ("cosd(", Token::Cosd),
    ("cosh(", Token::Cosh),
    ("coth(", Token::Coth),
    ("csch(", Token::Csch),
    ("exp(", Token::Exp),
    ("fabs(", Token::Abs),
    ("floor(", Token::Floor),
    ("int(", Token::Int),
    ("isbad(", Token::IsBad),
    ("log(", Token::Log),
    ("log10(", Token::Log10),
    ("nint(", Token::Nint),
    ("poisson(", Token::Poisson),
    ("sech(", Token::Sech),
    ("sin(", Token::Sin),
    ("sinc(", Token::Sinc),
    ("sind(", Token::Sind),
    ("sinh(", Token::Sinh),
    ("sqr(", Token::Sqr),
    ("sqrt(", Token::Sqrt),
    ("tan(", Token::Tan),
    ("tand(", Token::Tand),
    ("tanh(", Token::Tanh),
    ("atan2(", Token::Atan2),
    ("atan2d(", Token::Atan2d),
    ("dim(", Token::Dim),
    ("fmod(", Token::FMod),
    ("gauss(", Token::Gauss),
    ("mod(", Token::FMod),
    ("pow(", Token::Pow),
    ("rand(", Token::Rand),
    ("sign(", Token::Sign),
    ("qif(", Token::Qif),
    ("max(", Token::FnMax),
    ("min(", Token::FnMin),
    ("(", Token::LParen),
    ("+", Token::Plus),
    ("-", Token::Minus),
    ("!", Token::Bang),
    (".not.", Token::DotNot),
];

/// The longest symbol in `symbols` that starts `rest`, with its length.
fn longest_match(rest: &str, symbols: &[(&str, Token)]) -> Option<(Token, usize)> {
    let mut best: Option<(&Token, usize)> = None;
    for (text, tok) in symbols {
        if rest.starts_with(text) && best.is_none_or(|(_, len)| text.len() > len) {
            best = Some((tok, text.len()));
        }
    }
    best.map(|(tok, len)| (tok.clone(), len))
}

/// Whether a symbol leaves an operand behind it, so that the next symbol must be an operator: true for a constant and a closing parenthesis.
fn ends_operand(tok: &Token) -> bool {
    !matches!(
        tok,
        Token::Plus
            | Token::Minus
            | Token::Star
            | Token::Slash
            | Token::DoubleStar
            | Token::ShiftLeft
            | Token::ShiftRight
            | Token::Less
            | Token::LessEq
            | Token::Greater
            | Token::GreaterEq
            | Token::EqEq
            | Token::NotEq
            | Token::Ampersand
            | Token::Pipe
            | Token::Caret
            | Token::AndAnd
            | Token::OrOr
            | Token::CaretCaret
            | Token::DotEqv
            | Token::DotNeqv
            | Token::DotXor
            | Token::Bang
            | Token::DotNot
            | Token::LParen
            | Token::Comma
    ) && !is_function(tok)
}

/// Whether a token is a function name, which carries its opening parenthesis.
fn is_function(tok: &Token) -> bool {
    matches!(
        tok,
        Token::Abs
            | Token::Acos
            | Token::Acosd
            | Token::Acosh
            | Token::Acoth
            | Token::Acsch
            | Token::Asech
            | Token::Asin
            | Token::Asind
            | Token::Asinh
            | Token::Atan
            | Token::Atand
            | Token::Atanh
            | Token::Ceil
            | Token::Cos
            | Token::Cosd
            | Token::Cosh
            | Token::Coth
            | Token::Csch
            | Token::Exp
            | Token::Floor
            | Token::Int
            | Token::IsBad
            | Token::Log
            | Token::Log10
            | Token::Nint
            | Token::Poisson
            | Token::Sech
            | Token::Sin
            | Token::Sinc
            | Token::Sind
            | Token::Sinh
            | Token::Sqr
            | Token::Sqrt
            | Token::Tan
            | Token::Tand
            | Token::Tanh
            | Token::Atan2
            | Token::Atan2d
            | Token::Dim
            | Token::Gauss
            | Token::FMod
            | Token::Pow
            | Token::Rand
            | Token::Sign
            | Token::Qif
            | Token::FnMax
            | Token::FnMin
    )
}

/// Tokenize a cleaned expression string.
///
/// `expr` must be lowercase with no whitespace.
/// `variables` is the list of defined variable names (lowercase, no whitespace).
///
/// Each symbol is read either as an operator or as an operand, according to the symbol before it, as C's `CompileExpression` does: after an operand only an operator may follow, and text that reads as no operator there is a missing operator (`ErrorCode::Miopr`) rather than the start of a name.
/// Where an operand is expected, text that matches no operand symbol is read as a numeric constant, then as a variable name, and otherwise is a missing operand (`ErrorCode::Miopa`).
pub(crate) fn tokenize(expr: &str, variables: &[&str]) -> AstResult<Vec<Token>> {
    let bytes = expr.as_bytes();
    let mut tokens = Vec::new();
    let mut pos = 0;
    let mut operator_next = false;
    let mut levels: Vec<ParenLevel> = Vec::new();

    while pos < bytes.len() {
        let rest = &expr[pos..];
        if operator_next {
            let Some((tok, len)) = longest_match(rest, OPERATORS) else {
                return Err(AstError::new(
                    ErrorCode::Miopr,
                    format!(
                        "missing or invalid operator in the expression \"{}\"",
                        through_char_at(expr, pos)
                    ),
                ));
            };
            validate_symbol(&tok, &mut levels, &expr[..pos + len])?;
            operator_next = ends_operand(&tok);
            tokens.push(tok);
            pos += len;
        } else if let Some((tok, len)) = longest_match(rest, OPERANDS) {
            validate_symbol(&tok, &mut levels, &expr[..pos + len])?;
            operator_next = ends_operand(&tok);
            tokens.push(tok);
            pos += len;
        } else if bytes[pos].is_ascii_digit() || bytes[pos] == b'.' {
            let (val, end) = parse_number(expr, pos)?;
            tokens.push(Token::Number(val));
            operator_next = true;
            pos = end;
        } else if let Some(end) = parse_name(expr, pos) {
            let name = &expr[pos..end];
            let Some(idx) = variables.iter().position(|&var| var == name) else {
                return Err(AstError::new(
                    ErrorCode::Udvof,
                    format!(
                        "undefined variable or function in the expression \"{}\"",
                        &expr[..end]
                    ),
                ));
            };
            tokens.push(Token::Variable(idx));
            operator_next = true;
            pos = end;
        } else {
            return Err(AstError::new(
                ErrorCode::Miopa,
                format!(
                    "missing or invalid operand in the expression \"{}\"",
                    through_char_at(expr, pos)
                ),
            ));
        }
    }

    if !operator_next {
        return Err(AstError::new(
            ErrorCode::Miopa,
            format!("missing or invalid operand in the expression \"{expr}\""),
        ));
    }
    if !levels.is_empty() {
        return Err(AstError::new(
            ErrorCode::Mrpar,
            format!("missing right parenthesis in the expression \"{expr}\""),
        ));
    }

    tokens.push(Token::Eof);
    Ok(tokens)
}

/// An open level of parentheses: the number of arguments the symbol that opened it takes, and the number of arguments read so far.
///
/// `nargs` is 0 for a bare `(`, the fixed count for a function, or minus the minimum count for a function taking a variable number of arguments.
/// `argcount` is 0 at a bare `(` and stays there, and counts the arguments of a function call.
struct ParenLevel {
    nargs: i32,
    argcount: i32,
}

/// The number of arguments the symbol `tok` opens a level of parentheses for, as `ParenLevel::nargs` describes, or `None` if it opens none.
fn opened_nargs(tok: &Token) -> Option<i32> {
    match tok {
        Token::LParen => Some(0),
        Token::Qif => Some(3),
        Token::Atan2
        | Token::Atan2d
        | Token::Dim
        | Token::Gauss
        | Token::FMod
        | Token::Pow
        | Token::Rand
        | Token::Sign => Some(2),
        Token::FnMax | Token::FnMin => Some(-2),
        _ if is_function(tok) => Some(1),
        _ => None,
    }
}

/// Check a symbol against the parentheses and function arguments read so far, as C's `ValidateSymbol` does, updating `levels`.
///
/// A comma must be inside a function call's parentheses (`ErrorCode::Comin`), a closing parenthesis must have an opening one (`ErrorCode::Mlpar`), and a function call must close with the number of arguments the function takes (`ErrorCode::Wrnfa`).
/// `quoted` is the expression through the symbol, which an error message quotes.
fn validate_symbol(tok: &Token, levels: &mut Vec<ParenLevel>, quoted: &str) -> AstResult<()> {
    if *tok == Token::Comma {
        match levels.last_mut() {
            Some(level) if level.argcount != 0 => level.argcount += 1,
            _ => {
                return Err(AstError::new(
                    ErrorCode::Comin,
                    format!("spurious comma encountered in the expression \"{quoted}\""),
                ));
            }
        }
    } else if let Some(nargs) = opened_nargs(tok) {
        levels.push(ParenLevel {
            nargs,
            argcount: i32::from(nargs != 0),
        });
    } else if *tok == Token::RParen {
        let Some(level) = levels.last() else {
            return Err(AstError::new(
                ErrorCode::Mlpar,
                format!("missing left parenthesis in the expression \"{quoted}\""),
            ));
        };
        if level.nargs > 0 && level.argcount != level.nargs {
            return Err(AstError::new(
                ErrorCode::Wrnfa,
                format!("wrong number of function arguments in the expression \"{quoted}\""),
            ));
        }
        if level.nargs < 0 && level.argcount < -level.nargs {
            return Err(AstError::new(
                ErrorCode::Wrnfa,
                format!("insufficient function arguments in the expression \"{quoted}\""),
            ));
        }
        levels.pop();
    }
    Ok(())
}

/// The expression up to and including the character at `pos`, which an error message quotes to show where reading stopped.
fn through_char_at(expr: &str, pos: usize) -> &str {
    let len = expr[pos..].chars().next().map_or(0, char::len_utf8);
    &expr[..pos + len]
}

/// The end of the name starting at `pos`: a letter followed by letters, digits and underscores.
/// `None` if `pos` does not start with a letter.
pub(crate) fn parse_name(expr: &str, pos: usize) -> Option<usize> {
    let bytes = expr.as_bytes();
    if pos >= bytes.len() || !bytes[pos].is_ascii_alphabetic() {
        return None;
    }
    let mut end = pos + 1;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    Some(end)
}

/// Read the numeric constant starting at `pos`, which is a digit or `.`, returning its value and the end of its text.
///
/// The constant extends while each character may continue it: digits, one `.` before any exponent, one `d` or `e` exponent marker after a digit, and one sign directly after the marker.
/// A `.`, exponent marker or sign that cannot continue it makes the constant invalid (`ErrorCode::Conin`), except that a sign after a digit ends it.
/// Every field must contain a digit.
fn parse_number(expr: &str, pos: usize) -> AstResult<(f64, usize)> {
    let bytes = expr.as_bytes();
    let mut numer = bytes[pos].is_ascii_digit();
    let mut dpoint = bytes[pos] == b'.';
    let mut expon = false;
    let mut sign = false;
    let mut valid = true;
    let mut end = pos + 1;

    while end < bytes.len() {
        let ch = bytes[end];
        let mut iscon = false;
        if ch.is_ascii_digit() {
            numer = true;
            iscon = true;
        } else if ch == b'.' {
            if !(dpoint || expon) {
                dpoint = true;
                iscon = true;
            } else {
                valid = false;
            }
        } else if ch == b'd' || ch == b'e' {
            if !expon && numer {
                expon = true;
                numer = false;
                iscon = true;
            } else {
                valid = false;
            }
        } else if ch == b'+' || ch == b'-' {
            if expon && !sign && !numer {
                sign = true;
                iscon = true;
            } else if !numer {
                valid = false;
            }
        }
        if iscon || !valid {
            end += 1;
        }
        if !iscon {
            break;
        }
    }

    let value = if valid && numer {
        let text: String = expr[pos..end]
            .chars()
            .map(|c| if c == 'd' { 'e' } else { c })
            .collect();
        text.parse::<f64>().ok()
    } else {
        None
    };
    value.map(|v| (v, end)).ok_or_else(|| {
        AstError::new(
            ErrorCode::Conin,
            format!(
                "invalid constant syntax in the expression \"{}\"",
                &expr[..end]
            ),
        )
    })
}
