// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Opcode enum and compiled expression representation.

/// Operation codes for the MathMap stack VM.
///
/// These match the C library's `Oper` enum exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Opcode {
    // User-supplied constants and variables
    LdCon,
    LdVar,

    // System constants
    LdBad,
    LdDig,
    LdEps,
    LdMax,
    LdMax10E,
    LdMaxE,
    LdMDig,
    LdMin,
    LdMin10E,
    LdMinE,
    LdRad,
    LdRnd,

    // Mathematical constants
    LdE,
    LdPi,

    // Functions with one argument
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
    Poiss,
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

    // Functions with two arguments
    Atan2,
    Atan2d,
    Dim,
    Gauss,
    Mod,
    Pow,
    Rand,
    Sign,

    // Functions with three arguments
    Qif,

    // Variable-argument functions
    Max,
    Min,

    // Unary arithmetic
    Neg,

    // Unary boolean
    Not,

    // Binary arithmetic
    Add,
    Div,
    Mul,
    Sub,

    // Bit-shift
    ShftL,
    ShftR,

    // Relational
    Eq,
    Ge,
    Gt,
    Le,
    Lt,
    Ne,

    // Bit-wise
    BitAnd,
    BitOr,
    BitXor,

    // Binary boolean
    And,
    Eqv,
    Or,
    Xor,

    // Null (no-op)
    Null,
}

/// A compiled expression: opcodes + associated constants.
#[derive(Debug, Clone)]
pub(crate) struct CompiledExpr {
    /// The sequence of opcodes to execute.
    pub opcodes: Vec<Opcode>,
    /// The constants referenced by LdCon/LdVar/Max/Min opcodes.
    pub constants: Vec<f64>,
    /// Stack size needed to evaluate this expression.
    pub stack_size: usize,
}
