//! Pratt parser for MathMap expressions.
//!
//! Converts a token stream into a compiled expression (opcodes + constants).

use crate::error::{AstError, AstResult, ErrorCode};

use super::opcode::{CompiledExpr, Opcode};
use super::token::Token;

/// Compile a tokenized expression into opcodes and constants.
///
/// This implements operator-precedence parsing (a variant of Pratt parsing)
/// following the same precedence as the C library's symbol table.
pub(crate) fn compile(tokens: &[Token]) -> AstResult<CompiledExpr> {
    let mut compiler = Compiler {
        tokens,
        pos: 0,
        opcodes: Vec::new(),
        constants: Vec::new(),
        stack_size: 0,
        max_stack: 0,
    };
    compiler.parse_expr(0)?;

    // Should be at EOF
    if compiler.pos < compiler.tokens.len() && compiler.tokens[compiler.pos] != Token::Eof {
        return Err(AstError::new(
            ErrorCode::Miopr,
            "unexpected tokens after expression".to_string(),
        ));
    }

    Ok(CompiledExpr {
        opcodes: compiler.opcodes,
        constants: compiler.constants,
        stack_size: compiler.max_stack,
    })
}

struct Compiler<'a> {
    tokens: &'a [Token],
    pos: usize,
    opcodes: Vec<Opcode>,
    constants: Vec<f64>,
    stack_size: i32,
    max_stack: usize,
}

impl<'a> Compiler<'a> {
    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> &Token {
        let tok = self.tokens.get(self.pos).unwrap_or(&Token::Eof);
        self.pos += 1;
        tok
    }

    fn push_stack(&mut self, n: i32) {
        self.stack_size += n;
        if self.stack_size > self.max_stack as i32 {
            self.max_stack = self.stack_size as usize;
        }
    }

    /// Parse an expression with minimum binding power `min_bp`.
    fn parse_expr(&mut self, min_bp: u8) -> AstResult<()> {
        // Parse prefix / atom
        self.parse_prefix()?;

        // Parse infix operators
        loop {
            let tok = self.peek().clone();
            let (lbp, op) = match infix_binding(&tok) {
                Some(v) => v,
                None => break,
            };
            if lbp < min_bp {
                break;
            }
            self.advance();
            let rbp = right_bp(&tok);
            self.parse_expr(rbp)?;
            self.push_stack(-1); // binary op consumes 2, produces 1
            self.opcodes.push(op);
        }

        Ok(())
    }

    /// Parse a prefix expression (atom, unary op, function call, etc.)
    fn parse_prefix(&mut self) -> AstResult<()> {
        let tok = self.advance().clone();
        match tok {
            // Numeric constant
            Token::Number(val) => {
                self.push_stack(1);
                self.opcodes.push(Opcode::LdCon);
                self.constants.push(val);
            }

            // Variable
            Token::Variable(idx) => {
                self.push_stack(1);
                self.opcodes.push(Opcode::LdVar);
                self.constants.push(idx as f64);
            }

            // System constants
            Token::Bad => {
                self.push_stack(1);
                self.opcodes.push(Opcode::LdBad);
            }
            Token::Dig => {
                self.push_stack(1);
                self.opcodes.push(Opcode::LdDig);
            }
            Token::Epsilon => {
                self.push_stack(1);
                self.opcodes.push(Opcode::LdEps);
            }
            Token::MantDig => {
                self.push_stack(1);
                self.opcodes.push(Opcode::LdMDig);
            }
            Token::Max => {
                self.push_stack(1);
                self.opcodes.push(Opcode::LdMax);
            }
            Token::Max10Exp => {
                self.push_stack(1);
                self.opcodes.push(Opcode::LdMax10E);
            }
            Token::MaxExp => {
                self.push_stack(1);
                self.opcodes.push(Opcode::LdMaxE);
            }
            Token::Min => {
                self.push_stack(1);
                self.opcodes.push(Opcode::LdMin);
            }
            Token::Min10Exp => {
                self.push_stack(1);
                self.opcodes.push(Opcode::LdMin10E);
            }
            Token::MinExp => {
                self.push_stack(1);
                self.opcodes.push(Opcode::LdMinE);
            }
            Token::Radix => {
                self.push_stack(1);
                self.opcodes.push(Opcode::LdRad);
            }
            Token::Rounds => {
                self.push_stack(1);
                self.opcodes.push(Opcode::LdRnd);
            }
            Token::ConstE => {
                self.push_stack(1);
                self.opcodes.push(Opcode::LdE);
            }
            Token::ConstPi => {
                self.push_stack(1);
                self.opcodes.push(Opcode::LdPi);
            }

            // Unary operators
            Token::Plus => {
                // Unary plus: no-op, just parse the operand
                self.parse_expr(17)?;
            }
            Token::Minus => {
                // Unary minus
                self.parse_expr(17)?;
                self.opcodes.push(Opcode::Neg);
            }
            Token::Bang | Token::DotNot => {
                self.parse_expr(17)?;
                self.opcodes.push(Opcode::Not);
            }

            // Parenthesized expression
            Token::LParen => {
                self.parse_expr(0)?;
                self.expect_token(&Token::RParen)?;
            }

            // One-argument functions (the '(' was consumed by tokenizer)
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
            | Token::Tanh => {
                let op = unary_fn_opcode(&tok);
                self.parse_expr(0)?;
                self.expect_token(&Token::RParen)?;
                self.opcodes.push(op);
            }

            // Two-argument functions
            Token::Atan2
            | Token::Atan2d
            | Token::Dim
            | Token::Gauss
            | Token::FMod
            | Token::Pow
            | Token::Rand
            | Token::Sign => {
                let op = binary_fn_opcode(&tok);
                self.parse_expr(0)?;
                self.expect_token(&Token::Comma)?;
                self.parse_expr(0)?;
                self.expect_token(&Token::RParen)?;
                self.push_stack(-1);
                self.opcodes.push(op);
            }

            // Ternary function: qif(cond, then, else)
            Token::Qif => {
                self.parse_expr(0)?;
                self.expect_token(&Token::Comma)?;
                self.parse_expr(0)?;
                self.expect_token(&Token::Comma)?;
                self.parse_expr(0)?;
                self.expect_token(&Token::RParen)?;
                self.push_stack(-2);
                self.opcodes.push(Opcode::Qif);
            }

            // Variable-argument functions: max(...), min(...)
            Token::FnMax | Token::FnMin => {
                let op = if tok == Token::FnMax {
                    Opcode::Max
                } else {
                    Opcode::Min
                };
                let nargs = self.parse_vararg_list()?;
                if nargs < 2 {
                    return Err(AstError::new(
                        ErrorCode::Miopa,
                        format!(
                            "{} requires at least 2 arguments",
                            if op == Opcode::Max { "max" } else { "min" }
                        ),
                    ));
                }
                // Variable-arg: net stack change is -(nargs-1)
                self.push_stack(-(nargs as i32 - 1));
                self.opcodes.push(op);
                self.constants.push(nargs as f64);
            }

            Token::Eof => {
                return Err(AstError::new(
                    ErrorCode::Miopa,
                    "unexpected end of expression".to_string(),
                ));
            }

            _ => {
                return Err(AstError::new(
                    ErrorCode::Miopa,
                    format!("unexpected token in expression: {:?}", tok),
                ));
            }
        }
        Ok(())
    }

    fn parse_vararg_list(&mut self) -> AstResult<usize> {
        let mut count = 0;
        // First argument
        self.parse_expr(0)?;
        count += 1;
        // Subsequent arguments separated by commas
        while self.peek() == &Token::Comma {
            self.advance();
            self.parse_expr(0)?;
            count += 1;
        }
        self.expect_token(&Token::RParen)?;
        Ok(count)
    }

    fn expect_token(&mut self, expected: &Token) -> AstResult<()> {
        let tok = self.peek().clone();
        if std::mem::discriminant(&tok) == std::mem::discriminant(expected) {
            self.advance();
            Ok(())
        } else {
            Err(AstError::new(
                ErrorCode::Mrpar,
                format!("expected {:?}, got {:?}", expected, tok),
            ))
        }
    }
}

/// Return (left binding power, opcode) for infix operators.
fn infix_binding(tok: &Token) -> Option<(u8, Opcode)> {
    match tok {
        Token::DoubleStar => Some((15, Opcode::Pow)),
        Token::Star => Some((14, Opcode::Mul)),
        Token::Slash => Some((14, Opcode::Div)),
        Token::Plus => Some((13, Opcode::Add)),
        Token::Minus => Some((13, Opcode::Sub)),
        Token::ShiftLeft => Some((12, Opcode::ShftL)),
        Token::ShiftRight => Some((12, Opcode::ShftR)),
        Token::Less => Some((11, Opcode::Lt)),
        Token::LessEq => Some((11, Opcode::Le)),
        Token::Greater => Some((11, Opcode::Gt)),
        Token::GreaterEq => Some((11, Opcode::Ge)),
        Token::EqEq => Some((10, Opcode::Eq)),
        Token::NotEq => Some((10, Opcode::Ne)),
        Token::Ampersand => Some((9, Opcode::BitAnd)),
        Token::Caret => Some((8, Opcode::BitXor)),
        Token::Pipe => Some((7, Opcode::BitOr)),
        Token::AndAnd => Some((6, Opcode::And)),
        Token::CaretCaret => Some((5, Opcode::Xor)),
        Token::OrOr => Some((4, Opcode::Or)),
        Token::DotEqv => Some((3, Opcode::Eqv)),
        // C computes `.neqv.` and `.xor.` as `^^` does but gives them the
        // priority of `.eqv.` (mathmap.c, the `symbol` array).
        Token::DotXor | Token::DotNeqv => Some((3, Opcode::Xor)),
        _ => None,
    }
}

/// Right binding power for infix operators.
/// ** is right-associative, everything else is left-associative.
fn right_bp(tok: &Token) -> u8 {
    match tok {
        Token::DoubleStar => 15, // Right-associative: same as left bp
        _ => {
            if let Some((lbp, _)) = infix_binding(tok) {
                lbp + 1
            } else {
                0
            }
        }
    }
}

fn unary_fn_opcode(tok: &Token) -> Opcode {
    match tok {
        Token::Abs => Opcode::Abs,
        Token::Acos => Opcode::Acos,
        Token::Acosd => Opcode::Acosd,
        Token::Acosh => Opcode::Acosh,
        Token::Acoth => Opcode::Acoth,
        Token::Acsch => Opcode::Acsch,
        Token::Asech => Opcode::Asech,
        Token::Asin => Opcode::Asin,
        Token::Asind => Opcode::Asind,
        Token::Asinh => Opcode::Asinh,
        Token::Atan => Opcode::Atan,
        Token::Atand => Opcode::Atand,
        Token::Atanh => Opcode::Atanh,
        Token::Ceil => Opcode::Ceil,
        Token::Cos => Opcode::Cos,
        Token::Cosd => Opcode::Cosd,
        Token::Cosh => Opcode::Cosh,
        Token::Coth => Opcode::Coth,
        Token::Csch => Opcode::Csch,
        Token::Exp => Opcode::Exp,
        Token::Floor => Opcode::Floor,
        Token::Int => Opcode::Int,
        Token::IsBad => Opcode::IsBad,
        Token::Log => Opcode::Log,
        Token::Log10 => Opcode::Log10,
        Token::Nint => Opcode::Nint,
        Token::Poisson => Opcode::Poiss,
        Token::Sech => Opcode::Sech,
        Token::Sin => Opcode::Sin,
        Token::Sinc => Opcode::Sinc,
        Token::Sind => Opcode::Sind,
        Token::Sinh => Opcode::Sinh,
        Token::Sqr => Opcode::Sqr,
        Token::Sqrt => Opcode::Sqrt,
        Token::Tan => Opcode::Tan,
        Token::Tand => Opcode::Tand,
        Token::Tanh => Opcode::Tanh,
        _ => Opcode::Null,
    }
}

fn binary_fn_opcode(tok: &Token) -> Opcode {
    match tok {
        Token::Atan2 => Opcode::Atan2,
        Token::Atan2d => Opcode::Atan2d,
        Token::Dim => Opcode::Dim,
        Token::Gauss => Opcode::Gauss,
        Token::FMod => Opcode::Mod,
        Token::Pow => Opcode::Pow,
        Token::Rand => Opcode::Rand,
        Token::Sign => Opcode::Sign,
        _ => Opcode::Null,
    }
}
