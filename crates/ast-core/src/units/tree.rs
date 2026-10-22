//! `UnitNode` tree representation. Mirrors the `UnitNode` struct +
//! `Oper` enum in `reference/starlink-ast/src/unit.c` (~150-188).

use crate::constants::BAD_VALUE;

use super::table::{KnownUnit, Multiplier};

/// Operation performed by a `UnitNode`. Mirrors C's `Oper` enum
/// (`unit.c`:152-165).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Oper {
    /// Load constant.
    LdCon,
    /// Load variable (a unit reference).
    LdVar,
    /// Common logarithm (base 10).
    Log,
    /// Natural logarithm.
    Ln,
    /// Natural exponential.
    Exp,
    /// Square root.
    Sqrt,
    /// Exponentiation: `arg[0] ** arg[1]`.
    Pow,
    /// Division: `arg[0] / arg[1]`.
    Div,
    /// Multiplication: `arg[0] * arg[1]`.
    Mult,
    /// Load constant PI.
    LdPi,
    /// Load constant E.
    LdE,
    /// Sentinel "no-op" (used during parsing).
    Null,
}

impl Oper {
    /// Number of arguments consumed by this operation. Mirrors the
    /// `narg` initialization in `NewNode` (`unit.c`:3743-3818).
    pub(crate) fn narg(self) -> usize {
        match self {
            Oper::LdCon | Oper::LdVar | Oper::LdPi | Oper::LdE | Oper::Null => 0,
            Oper::Log | Oper::Ln | Oper::Exp | Oper::Sqrt => 1,
            Oper::Pow | Oper::Div | Oper::Mult => 2,
        }
    }

    /// Human-readable opcode name (used in diagnostics; mirrors the C
    /// `OpName` helper at `unit.c`:3786).
    pub(crate) fn name(self) -> &'static str {
        match self {
            Oper::LdCon => "OP_LDCON",
            Oper::LdVar => "OP_LDVAR",
            Oper::Log => "OP_LOG",
            Oper::Ln => "OP_LN",
            Oper::Exp => "OP_EXP",
            Oper::Sqrt => "OP_SQRT",
            Oper::Pow => "OP_POW",
            Oper::Div => "OP_DIV",
            Oper::Mult => "OP_MULT",
            Oper::LdPi => "OP_LDPI",
            Oper::LdE => "OP_LDE",
            Oper::Null => "OP_NULL",
        }
    }
}

/// A node in the unit-expression tree. Mirrors the `UnitNode` struct
/// in `unit.c`:179-188.
///
/// The C version stores arguments as a `UnitNode**` array sized by
/// `narg`. We mirror that with a `Vec<Box<UnitNode>>` rather than
/// a tagged enum-per-arity, because the C simplification passes
/// repeatedly mutate `arg[i]` in place to swap subtrees — a flat
/// `Vec<Box<...>>` lets us do the same with `arg.swap`/`arg[i] =
/// new_node` without re-tagging the parent.
#[derive(Debug, Clone)]
pub(crate) struct UnitNode {
    pub opcode: Oper,
    // Boxing each child individually mirrors the C `UnitNode**` array
    // and lets `arg[i] = new_subtree` swap subtrees without disturbing
    // sibling addresses. Suppress clippy's `vec_box` here because the
    // boxing is load-bearing for the simplification passes that
    // mutate args in place.
    #[allow(clippy::vec_box)]
    pub arg: Vec<Box<UnitNode>>,
    /// Constant value when `opcode == LdCon`. Other opcodes store
    /// `BAD_VALUE` here as a sentinel — matches the C convention so
    /// the helper predicates `is_const()` etc. line up with the
    /// `con != AST__BAD` checks on the C side.
    pub con: f64,
    /// Pointer to the `KnownUnit` table entry for `LdVar` nodes.
    /// Stored as `'static` because the table is initialized once and
    /// never freed.
    pub unit: Option<&'static KnownUnit>,
    /// Multiplier prefix for `LdVar` nodes (e.g. the `M` in `MHz`).
    pub mult: Option<&'static Multiplier>,
    /// User-supplied unit name (no multiplier prefix). For `LdVar`
    /// nodes only — set even when `unit` is `None` (i.e. unknown
    /// units pass through with their literal name).
    pub name: Option<String>,
}

impl UnitNode {
    /// Make a new node. Mirrors `NewNode` (`unit.c`:3743-3818). The
    /// arg slots are pre-allocated to `opcode.narg()` entries with
    /// dummy `Null` placeholders that the caller is expected to
    /// overwrite.
    pub(crate) fn new(opcode: Oper) -> Self {
        let narg = opcode.narg();
        let mut arg = Vec::with_capacity(narg);
        for _ in 0..narg {
            arg.push(Box::new(UnitNode {
                opcode: Oper::Null,
                arg: Vec::new(),
                con: BAD_VALUE,
                unit: None,
                mult: None,
                name: None,
            }));
        }
        UnitNode {
            opcode,
            arg,
            con: BAD_VALUE,
            unit: None,
            mult: None,
            name: None,
        }
    }

    /// Make a constant-value node.
    pub(crate) fn ldcon(value: f64) -> Box<UnitNode> {
        let mut n = UnitNode::new(Oper::LdCon);
        n.con = value;
        Box::new(n)
    }

    /// Make a variable (unit-reference) node.
    pub(crate) fn ldvar(
        unit: Option<&'static KnownUnit>,
        mult: Option<&'static Multiplier>,
        name: impl Into<String>,
    ) -> Box<UnitNode> {
        let mut n = UnitNode::new(Oper::LdVar);
        n.unit = unit;
        n.mult = mult;
        n.name = Some(name.into());
        Box::new(n)
    }

    /// Make a binary node `a op b`. Args are placed at `arg[0]` and
    /// `arg[1]`.
    pub(crate) fn binary(opcode: Oper, a: Box<UnitNode>, b: Box<UnitNode>) -> Box<UnitNode> {
        debug_assert_eq!(opcode.narg(), 2);
        let mut n = UnitNode::new(opcode);
        n.arg[0] = a;
        n.arg[1] = b;
        Box::new(n)
    }

    /// Make a unary node `op(a)`. Arg is placed at `arg[0]`.
    pub(crate) fn unary(opcode: Oper, a: Box<UnitNode>) -> Box<UnitNode> {
        debug_assert_eq!(opcode.narg(), 1);
        let mut n = UnitNode::new(opcode);
        n.arg[0] = a;
        Box::new(n)
    }

    /// True if this node is a literal constant.
    pub(crate) fn is_const(&self) -> bool {
        self.opcode == Oper::LdCon && self.con != BAD_VALUE
    }
}
