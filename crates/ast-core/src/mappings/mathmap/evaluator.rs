//! Stack-based virtual machine for evaluating compiled MathMap expressions.
// The VM uses indexed access to stack slots by design (stack[slot][point]),
// where `slot` is a separate computed index. Iterator-based rewrites would
// obscure the stack machine semantics.
#![allow(clippy::needless_range_loop)]

use crate::constants::BAD_VALUE;

use super::opcode::{CompiledExpr, Opcode};
use super::rng::MathMapRng;

/// The library-computed constants C's `EvaluateFunction` keeps in statics
/// after its first invocation (mathmap.c:1885-1919): ln 2, and a value that
/// can be squared without overflow. (C's pi, from `acos( -1.0 )`, is the
/// same double as `std::f64::consts::PI`, so it needs no computing here.)
#[derive(Clone, Copy)]
struct Constants {
    log2: f64,
    safe_sq: f64,
    rsafe_sq: f64,
}

static CONSTANTS: std::sync::LazyLock<Constants> = std::sync::LazyLock::new(|| {
    let safe_sq = 0.9 * crate::math::sqrt(f64::MAX);
    Constants {
        log2: crate::math::ln(2.0),
        safe_sq,
        rsafe_sq: 1.0 / safe_sq,
    }
});

const PI: f64 = std::f64::consts::PI;

/// Evaluate a compiled expression for `npoint` points.
///
/// `inputs` is indexed as `inputs[coord][point]`.
/// `output` is filled with the result for each point.
pub(crate) fn evaluate(
    expr: &CompiledExpr,
    inputs: &[&[f64]],
    npoint: usize,
    output: &mut [f64],
    rng: &mut MathMapRng,
) {
    let stack_size = expr.stack_size;
    if stack_size == 0 || npoint == 0 {
        return;
    }

    // Allocate stack workspace
    let mut work = vec![0.0_f64; npoint * (stack_size.saturating_sub(1))];

    // Stack pointers: stack[0] -> output, stack[i>0] -> work slices
    let mut stack: Vec<&mut [f64]> = Vec::with_capacity(stack_size);

    // We need to use unsafe to create multiple mutable slices from work.
    // This is safe because each stack slot refers to a disjoint region.
    unsafe {
        let work_ptr = work.as_mut_ptr();
        // Placeholder - we'll rebuild this per evaluation
        stack.push(std::slice::from_raw_parts_mut(output.as_mut_ptr(), npoint));
        for i in 1..stack_size {
            stack.push(std::slice::from_raw_parts_mut(
                work_ptr.add((i - 1) * npoint),
                npoint,
            ));
        }
    }

    let d2r = PI / 180.0;
    let r2d = 180.0 / PI;
    let Constants {
        log2,
        safe_sq,
        rsafe_sq,
    } = *CONSTANTS;

    let mut tos: i32 = -1;
    let mut icon: usize = 0;

    for opcode in &expr.opcodes {
        match opcode {
            Opcode::Null => {}

            // Load constant
            Opcode::LdCon => {
                let value = expr.constants[icon];
                icon += 1;
                tos += 1;
                let slot = tos as usize;
                for point in 0..npoint {
                    stack[slot][point] = value;
                }
            }

            // Load variable
            Opcode::LdVar => {
                let ivar = expr.constants[icon] as usize;
                icon += 1;
                tos += 1;
                let slot = tos as usize;
                for point in 0..npoint {
                    stack[slot][point] = inputs[ivar][point];
                }
            }

            // System constants
            Opcode::LdBad => {
                tos += 1;
                let slot = tos as usize;
                for point in 0..npoint {
                    stack[slot][point] = BAD_VALUE;
                }
            }
            Opcode::LdDig => {
                tos += 1;
                let slot = tos as usize;
                let val = 15.0; // DBL_DIG equivalent for f64
                for point in 0..npoint {
                    stack[slot][point] = val;
                }
            }
            Opcode::LdEps => {
                tos += 1;
                let slot = tos as usize;
                for point in 0..npoint {
                    stack[slot][point] = f64::EPSILON;
                }
            }
            Opcode::LdMax => {
                tos += 1;
                let slot = tos as usize;
                for point in 0..npoint {
                    stack[slot][point] = f64::MAX;
                }
            }
            Opcode::LdMax10E => {
                tos += 1;
                let slot = tos as usize;
                for point in 0..npoint {
                    stack[slot][point] = 308.0; // DBL_MAX_10_EXP
                }
            }
            Opcode::LdMaxE => {
                tos += 1;
                let slot = tos as usize;
                for point in 0..npoint {
                    stack[slot][point] = 1024.0; // DBL_MAX_EXP
                }
            }
            Opcode::LdMDig => {
                tos += 1;
                let slot = tos as usize;
                for point in 0..npoint {
                    stack[slot][point] = 53.0; // DBL_MANT_DIG
                }
            }
            Opcode::LdMin => {
                tos += 1;
                let slot = tos as usize;
                for point in 0..npoint {
                    stack[slot][point] = f64::MIN_POSITIVE;
                }
            }
            Opcode::LdMin10E => {
                tos += 1;
                let slot = tos as usize;
                for point in 0..npoint {
                    stack[slot][point] = -307.0; // DBL_MIN_10_EXP
                }
            }
            Opcode::LdMinE => {
                tos += 1;
                let slot = tos as usize;
                for point in 0..npoint {
                    stack[slot][point] = -1021.0; // DBL_MIN_EXP
                }
            }
            Opcode::LdRad => {
                tos += 1;
                let slot = tos as usize;
                for point in 0..npoint {
                    stack[slot][point] = 2.0; // FLT_RADIX
                }
            }
            Opcode::LdRnd => {
                tos += 1;
                let slot = tos as usize;
                for point in 0..npoint {
                    stack[slot][point] = 1.0; // FLT_ROUNDS (round to nearest)
                }
            }
            Opcode::LdE => {
                tos += 1;
                let slot = tos as usize;
                let val = std::f64::consts::E;
                for point in 0..npoint {
                    stack[slot][point] = val;
                }
            }
            Opcode::LdPi => {
                tos += 1;
                let slot = tos as usize;
                let val = PI;
                for point in 0..npoint {
                    stack[slot][point] = val;
                }
            }

            // Unary functions (1 argument)
            Opcode::Abs => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = x.abs();
                    }
                }
            }
            Opcode::Acos => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = if x.abs() <= 1.0 {
                            crate::math::acos(x)
                        } else {
                            BAD_VALUE
                        };
                    }
                }
            }
            Opcode::Acosd => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = if x.abs() <= 1.0 {
                            crate::math::acos(x) * r2d
                        } else {
                            BAD_VALUE
                        };
                    }
                }
            }
            Opcode::Acosh => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = if x < 1.0 {
                            BAD_VALUE
                        } else if x > safe_sq {
                            crate::math::ln(x) + log2
                        } else {
                            crate::math::ln(x + crate::math::sqrt(x * x - 1.0))
                        };
                    }
                }
            }
            Opcode::Acoth => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = if x.abs() <= 1.0 {
                            BAD_VALUE
                        } else {
                            0.5 * crate::math::ln((x + 1.0) / (x - 1.0))
                        };
                    }
                }
            }
            Opcode::Acsch => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        if x == 0.0 {
                            stack[slot][point] = BAD_VALUE;
                        } else {
                            let sign = if x >= 0.0 { 1.0 } else { -1.0 };
                            let ax = x.abs();
                            stack[slot][point] = if ax < rsafe_sq {
                                sign * (log2 - crate::math::ln(ax))
                            } else {
                                let r = 1.0 / ax;
                                sign * crate::math::ln(r + crate::math::sqrt(r * r + 1.0))
                            };
                        }
                    }
                }
            }
            Opcode::Asech => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = if x <= 0.0 || x > 1.0 {
                            BAD_VALUE
                        } else if x < rsafe_sq {
                            log2 - crate::math::ln(x)
                        } else {
                            let r = 1.0 / x;
                            crate::math::ln(r + crate::math::sqrt(r * r - 1.0))
                        };
                    }
                }
            }
            Opcode::Asin => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = if x.abs() <= 1.0 {
                            crate::math::asin(x)
                        } else {
                            BAD_VALUE
                        };
                    }
                }
            }
            Opcode::Asind => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = if x.abs() <= 1.0 {
                            crate::math::asin(x) * r2d
                        } else {
                            BAD_VALUE
                        };
                    }
                }
            }
            Opcode::Asinh => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        let sign = if x >= 0.0 { 1.0 } else { -1.0 };
                        let ax = x.abs();
                        stack[slot][point] = if ax > safe_sq {
                            sign * (crate::math::ln(ax) + log2)
                        } else {
                            sign * crate::math::ln(ax + crate::math::sqrt(ax * ax + 1.0))
                        };
                    }
                }
            }
            Opcode::Atan => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = crate::math::atan(x);
                    }
                }
            }
            Opcode::Atand => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = crate::math::atan(x) * r2d;
                    }
                }
            }
            Opcode::Atanh => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = if x.abs() >= 1.0 {
                            BAD_VALUE
                        } else {
                            0.5 * crate::math::ln((1.0 + x) / (1.0 - x))
                        };
                    }
                }
            }
            Opcode::Ceil => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = x.ceil();
                    }
                }
            }
            Opcode::Cos => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = crate::math::cos(x);
                    }
                }
            }
            Opcode::Cosd => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = crate::math::cos(x * d2r);
                    }
                }
            }
            Opcode::Cosh => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        let r = crate::math::cosh(x);
                        stack[slot][point] = if r.is_infinite() { BAD_VALUE } else { r };
                    }
                }
            }
            Opcode::Coth => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        let t = crate::math::tanh(x);
                        stack[slot][point] = safe_div(1.0, t);
                    }
                }
            }
            Opcode::Csch => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        let s = crate::math::sinh(x);
                        stack[slot][point] = if s.is_infinite() {
                            0.0
                        } else {
                            safe_div(1.0, s)
                        };
                    }
                }
            }
            Opcode::Exp => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        let r = crate::math::exp(x);
                        stack[slot][point] = if r.is_infinite() { BAD_VALUE } else { r };
                    }
                }
            }
            Opcode::Floor => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = x.floor();
                    }
                }
            }
            Opcode::Int => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = x.trunc();
                    }
                }
            }
            Opcode::IsBad => {
                // IsBad does NOT propagate bad - it tests for it
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    stack[slot][point] = if x == BAD_VALUE { 1.0 } else { 0.0 };
                }
            }
            Opcode::Log => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = if x > 0.0 {
                            crate::math::ln(x)
                        } else {
                            BAD_VALUE
                        };
                    }
                }
            }
            Opcode::Log10 => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = if x > 0.0 {
                            crate::math::log10(x)
                        } else {
                            BAD_VALUE
                        };
                    }
                }
            }
            Opcode::Nint => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = if x >= 0.0 {
                            (x + 0.5).floor()
                        } else {
                            (x - 0.5).ceil()
                        };
                    }
                }
            }
            Opcode::Poiss => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = rng.poisson(x);
                    }
                }
            }
            Opcode::Sech => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        let c = crate::math::cosh(x);
                        stack[slot][point] = if c.is_infinite() { 0.0 } else { 1.0 / c };
                    }
                }
            }
            Opcode::Sin => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = crate::math::sin(x);
                    }
                }
            }
            Opcode::Sinc => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = if x == 0.0 {
                            1.0
                        } else {
                            crate::math::sin(x) / x
                        };
                    }
                }
            }
            Opcode::Sind => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = crate::math::sin(x * d2r);
                    }
                }
            }
            Opcode::Sinh => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        let r = crate::math::sinh(x);
                        stack[slot][point] = if r.is_infinite() { BAD_VALUE } else { r };
                    }
                }
            }
            Opcode::Sqr => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = safe_mul(x, x);
                    }
                }
            }
            Opcode::Sqrt => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = if x >= 0.0 {
                            crate::math::sqrt(x)
                        } else {
                            BAD_VALUE
                        };
                    }
                }
            }
            Opcode::Tan => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        let r = crate::math::tan(x);
                        stack[slot][point] = if r.is_infinite() { BAD_VALUE } else { r };
                    }
                }
            }
            Opcode::Tand => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = crate::math::tan(x * d2r);
                    }
                }
            }
            Opcode::Tanh => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = crate::math::tanh(x);
                    }
                }
            }

            // Unary operators
            Opcode::Neg => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = -x;
                    }
                }
            }
            Opcode::Not => {
                let slot = tos as usize;
                for point in 0..npoint {
                    let x = stack[slot][point];
                    if x != BAD_VALUE {
                        stack[slot][point] = if x == 0.0 { 1.0 } else { 0.0 };
                    }
                }
            }

            // Binary arithmetic operators
            Opcode::Add => {
                let slot2 = tos as usize;
                tos -= 1;
                let slot1 = tos as usize;
                for point in 0..npoint {
                    let x1 = stack[slot1][point];
                    if x1 != BAD_VALUE {
                        let x2 = stack[slot2][point];
                        if x2 != BAD_VALUE {
                            stack[slot1][point] = safe_add(x1, x2);
                        } else {
                            stack[slot1][point] = BAD_VALUE;
                        }
                    }
                }
            }
            Opcode::Sub => {
                let slot2 = tos as usize;
                tos -= 1;
                let slot1 = tos as usize;
                for point in 0..npoint {
                    let x1 = stack[slot1][point];
                    if x1 != BAD_VALUE {
                        let x2 = stack[slot2][point];
                        if x2 != BAD_VALUE {
                            stack[slot1][point] = safe_sub(x1, x2);
                        } else {
                            stack[slot1][point] = BAD_VALUE;
                        }
                    }
                }
            }
            Opcode::Mul => {
                let slot2 = tos as usize;
                tos -= 1;
                let slot1 = tos as usize;
                for point in 0..npoint {
                    let x1 = stack[slot1][point];
                    if x1 != BAD_VALUE {
                        let x2 = stack[slot2][point];
                        if x2 != BAD_VALUE {
                            stack[slot1][point] = safe_mul(x1, x2);
                        } else {
                            stack[slot1][point] = BAD_VALUE;
                        }
                    }
                }
            }
            Opcode::Div => {
                let slot2 = tos as usize;
                tos -= 1;
                let slot1 = tos as usize;
                for point in 0..npoint {
                    let x1 = stack[slot1][point];
                    if x1 != BAD_VALUE {
                        let x2 = stack[slot2][point];
                        if x2 != BAD_VALUE {
                            stack[slot1][point] = safe_div(x1, x2);
                        } else {
                            stack[slot1][point] = BAD_VALUE;
                        }
                    }
                }
            }

            // Power
            Opcode::Pow => {
                let slot2 = tos as usize;
                tos -= 1;
                let slot1 = tos as usize;
                for point in 0..npoint {
                    let x1 = stack[slot1][point];
                    if x1 != BAD_VALUE {
                        let x2 = stack[slot2][point];
                        if x2 != BAD_VALUE {
                            let r = crate::math::powf(x1, x2);
                            stack[slot1][point] = if r.is_infinite() || r.is_nan() {
                                BAD_VALUE
                            } else {
                                r
                            };
                        } else {
                            stack[slot1][point] = BAD_VALUE;
                        }
                    }
                }
            }

            // Bit-shift operators
            Opcode::ShftL => {
                let slot2 = tos as usize;
                tos -= 1;
                let slot1 = tos as usize;
                for point in 0..npoint {
                    let x1 = stack[slot1][point];
                    if x1 != BAD_VALUE {
                        let x2 = stack[slot2][point];
                        if x2 != BAD_VALUE {
                            stack[slot1][point] = shift_bits(x1, x2);
                        } else {
                            stack[slot1][point] = BAD_VALUE;
                        }
                    }
                }
            }
            Opcode::ShftR => {
                let slot2 = tos as usize;
                tos -= 1;
                let slot1 = tos as usize;
                for point in 0..npoint {
                    let x1 = stack[slot1][point];
                    if x1 != BAD_VALUE {
                        let x2 = stack[slot2][point];
                        if x2 != BAD_VALUE {
                            stack[slot1][point] = shift_bits(x1, -x2);
                        } else {
                            stack[slot1][point] = BAD_VALUE;
                        }
                    }
                }
            }

            // Relational operators
            Opcode::Eq => binary_op_bad(&mut stack, &mut tos, npoint, |x1, x2| {
                if x1 == x2 { 1.0 } else { 0.0 }
            }),
            Opcode::Ne => binary_op_bad(&mut stack, &mut tos, npoint, |x1, x2| {
                if x1 != x2 { 1.0 } else { 0.0 }
            }),
            Opcode::Lt => binary_op_bad(&mut stack, &mut tos, npoint, |x1, x2| {
                if x1 < x2 { 1.0 } else { 0.0 }
            }),
            Opcode::Le => binary_op_bad(&mut stack, &mut tos, npoint, |x1, x2| {
                if x1 <= x2 { 1.0 } else { 0.0 }
            }),
            Opcode::Gt => binary_op_bad(&mut stack, &mut tos, npoint, |x1, x2| {
                if x1 > x2 { 1.0 } else { 0.0 }
            }),
            Opcode::Ge => binary_op_bad(&mut stack, &mut tos, npoint, |x1, x2| {
                if x1 >= x2 { 1.0 } else { 0.0 }
            }),

            // Bit-wise operators
            Opcode::BitAnd => {
                let slot2 = tos as usize;
                tos -= 1;
                let slot1 = tos as usize;
                for point in 0..npoint {
                    let x1 = stack[slot1][point];
                    if x1 != BAD_VALUE {
                        let x2 = stack[slot2][point];
                        if x2 != BAD_VALUE {
                            stack[slot1][point] = bitwise_op(x1, x2, |a, b| a & b);
                        } else {
                            stack[slot1][point] = BAD_VALUE;
                        }
                    }
                }
            }
            Opcode::BitOr => {
                let slot2 = tos as usize;
                tos -= 1;
                let slot1 = tos as usize;
                for point in 0..npoint {
                    let x1 = stack[slot1][point];
                    if x1 != BAD_VALUE {
                        let x2 = stack[slot2][point];
                        if x2 != BAD_VALUE {
                            stack[slot1][point] = bitwise_op(x1, x2, |a, b| a | b);
                        } else {
                            stack[slot1][point] = BAD_VALUE;
                        }
                    }
                }
            }
            Opcode::BitXor => {
                let slot2 = tos as usize;
                tos -= 1;
                let slot1 = tos as usize;
                for point in 0..npoint {
                    let x1 = stack[slot1][point];
                    if x1 != BAD_VALUE {
                        let x2 = stack[slot2][point];
                        if x2 != BAD_VALUE {
                            stack[slot1][point] = bitwise_op(x1, x2, |a, b| a ^ b);
                        } else {
                            stack[slot1][point] = BAD_VALUE;
                        }
                    }
                }
            }

            // Boolean operators (tri-state)
            Opcode::And => {
                let slot2 = tos as usize;
                tos -= 1;
                let slot1 = tos as usize;
                for point in 0..npoint {
                    let x1 = stack[slot1][point];
                    let x2 = stack[slot2][point];
                    stack[slot1][point] = tristate_and(x1, x2);
                }
            }
            Opcode::Or => {
                let slot2 = tos as usize;
                tos -= 1;
                let slot1 = tos as usize;
                for point in 0..npoint {
                    let x1 = stack[slot1][point];
                    let x2 = stack[slot2][point];
                    stack[slot1][point] = tristate_or(x1, x2);
                }
            }
            Opcode::Xor => binary_op_bad(&mut stack, &mut tos, npoint, |x1, x2| {
                if (x1 != 0.0) != (x2 != 0.0) { 1.0 } else { 0.0 }
            }),
            Opcode::Eqv => binary_op_bad(&mut stack, &mut tos, npoint, |x1, x2| {
                if (x1 != 0.0) == (x2 != 0.0) { 1.0 } else { 0.0 }
            }),

            // Two-argument functions
            Opcode::Atan2 => binary_op_bad(&mut stack, &mut tos, npoint, |x1, x2| {
                crate::math::atan2(x1, x2)
            }),
            Opcode::Atan2d => binary_op_bad(&mut stack, &mut tos, npoint, |x1, x2| {
                crate::math::atan2(x1, x2) * r2d
            }),
            Opcode::Dim => binary_op_bad(&mut stack, &mut tos, npoint, |x1, x2| {
                if x1 > x2 { x1 - x2 } else { 0.0 }
            }),
            Opcode::Mod => binary_op_bad(&mut stack, &mut tos, npoint, |x1, x2| {
                if x2 != 0.0 { x1 % x2 } else { BAD_VALUE }
            }),
            Opcode::Sign => binary_op_bad(&mut stack, &mut tos, npoint, |x1, x2| {
                if (x1 >= 0.0) == (x2 >= 0.0) { x1 } else { -x1 }
            }),

            // Gauss(mean, sd)
            Opcode::Gauss => {
                let slot2 = tos as usize;
                tos -= 1;
                let slot1 = tos as usize;
                for point in 0..npoint {
                    let x1 = stack[slot1][point];
                    if x1 != BAD_VALUE {
                        let x2 = stack[slot2][point];
                        if x2 != BAD_VALUE {
                            // Generate Gaussian sample
                            let result = loop {
                                let g = rng.gauss();
                                let sd = x2.abs();
                                let r = safe_mul(g, sd);
                                if r != BAD_VALUE {
                                    let r = safe_add(r, x1);
                                    if r != BAD_VALUE {
                                        break r;
                                    }
                                }
                            };
                            stack[slot1][point] = result;
                        } else {
                            stack[slot1][point] = BAD_VALUE;
                        }
                    }
                }
            }

            // Rand(lo, hi)
            Opcode::Rand => {
                let slot2 = tos as usize;
                tos -= 1;
                let slot1 = tos as usize;
                for point in 0..npoint {
                    let x1 = stack[slot1][point];
                    if x1 != BAD_VALUE {
                        let x2 = stack[slot2][point];
                        if x2 != BAD_VALUE {
                            let ran = rng.rand();
                            stack[slot1][point] = x1 * ran + x2 * (1.0 - ran);
                        } else {
                            stack[slot1][point] = BAD_VALUE;
                        }
                    }
                }
            }

            // Ternary: qif(cond, then, else)
            Opcode::Qif => {
                let slot3 = tos as usize;
                tos -= 1;
                let slot2 = tos as usize;
                tos -= 1;
                let slot1 = tos as usize;
                for point in 0..npoint {
                    let cond = stack[slot1][point];
                    let then_val = stack[slot2][point];
                    let else_val = stack[slot3][point];
                    // Note: cond==BAD propagates to BAD per the C impl
                    // but actually the C impl: *y = (x1) ? (x2) : (x3)
                    // which means BAD (large negative) is truthy
                    stack[slot1][point] = if cond != 0.0 { then_val } else { else_val };
                }
            }

            // Variable-argument functions
            Opcode::Max => {
                let narg = expr.constants[icon] as usize;
                icon += 1;
                for _ in 0..(narg - 1) {
                    let slot2 = tos as usize;
                    tos -= 1;
                    let slot1 = tos as usize;
                    for point in 0..npoint {
                        let x1 = stack[slot1][point];
                        if x1 != BAD_VALUE {
                            let x2 = stack[slot2][point];
                            if x2 != BAD_VALUE {
                                stack[slot1][point] = if x1 >= x2 { x1 } else { x2 };
                            } else {
                                stack[slot1][point] = BAD_VALUE;
                            }
                        }
                    }
                }
            }
            Opcode::Min => {
                let narg = expr.constants[icon] as usize;
                icon += 1;
                for _ in 0..(narg - 1) {
                    let slot2 = tos as usize;
                    tos -= 1;
                    let slot1 = tos as usize;
                    for point in 0..npoint {
                        let x1 = stack[slot1][point];
                        if x1 != BAD_VALUE {
                            let x2 = stack[slot2][point];
                            if x2 != BAD_VALUE {
                                stack[slot1][point] = if x1 <= x2 { x1 } else { x2 };
                            } else {
                                stack[slot1][point] = BAD_VALUE;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A standard binary op that propagates BAD from either argument.
fn binary_op_bad(
    stack: &mut [&mut [f64]],
    tos: &mut i32,
    npoint: usize,
    op: impl Fn(f64, f64) -> f64,
) {
    let slot2 = *tos as usize;
    *tos -= 1;
    let slot1 = *tos as usize;
    // We need split borrows; use raw pointer approach
    let ptr = stack.as_mut_ptr();
    unsafe {
        let s1 = &mut *(*ptr.add(slot1));
        let s2 = &*(*ptr.add(slot2));
        for point in 0..npoint {
            let x1 = s1[point];
            if x1 != BAD_VALUE {
                let x2 = s2[point];
                if x2 != BAD_VALUE {
                    s1[point] = op(x1, x2);
                } else {
                    s1[point] = BAD_VALUE;
                }
            }
        }
    }
}

// Safe arithmetic functions matching C library behaviour.

fn safe_add(x1: f64, x2: f64) -> f64 {
    if x1 >= 0.0 {
        if x2 <= 0.0 || (f64::MAX - x1) >= x2 {
            x1 + x2
        } else {
            BAD_VALUE
        }
    } else if x2 >= 0.0 || (f64::MAX + x1) >= -x2 {
        x1 + x2
    } else {
        BAD_VALUE
    }
}

fn safe_sub(x1: f64, x2: f64) -> f64 {
    if x1 >= 0.0 {
        if x2 >= 0.0 || (f64::MAX - x1) >= -x2 {
            x1 - x2
        } else {
            BAD_VALUE
        }
    } else if x2 <= 0.0 || (f64::MAX + x1) >= x2 {
        x1 - x2
    } else {
        BAD_VALUE
    }
}

fn safe_mul(x1: f64, x2: f64) -> f64 {
    let abs1 = x1.abs();
    let abs2 = x2.abs();
    if abs1 <= 1.0 || abs2 <= 1.0 || (f64::MAX / abs1) >= abs2 {
        x1 * x2
    } else {
        BAD_VALUE
    }
}

fn safe_div(x1: f64, x2: f64) -> f64 {
    if x2 != 0.0 {
        let abs2 = x2.abs();
        if abs2 >= 1.0 || (f64::MAX * abs2) >= x1.abs() {
            x1 / x2
        } else {
            BAD_VALUE
        }
    } else {
        BAD_VALUE
    }
}

fn shift_bits(x1: f64, x2: f64) -> f64 {
    let (frac, expon) = frexp(x1);
    let new_exp = x2.trunc() + expon as f64;
    if new_exp < -(i32::MAX as f64) {
        0.0
    } else if new_exp > i32::MAX as f64 {
        if frac == 0.0 { 0.0 } else { BAD_VALUE }
    } else {
        let r = ldexp(frac, new_exp as i32);
        if r.is_infinite() { BAD_VALUE } else { r }
    }
}

fn tristate_and(x1: f64, x2: f64) -> f64 {
    if x1 == BAD_VALUE {
        if x2 != 0.0 { BAD_VALUE } else { 0.0 }
    } else if x2 == BAD_VALUE {
        if x1 != 0.0 { BAD_VALUE } else { 0.0 }
    } else if x1 != 0.0 && x2 != 0.0 {
        1.0
    } else {
        0.0
    }
}

fn tristate_or(x1: f64, x2: f64) -> f64 {
    if x1 == BAD_VALUE {
        if x2 == 0.0 || x2 == BAD_VALUE {
            BAD_VALUE
        } else {
            1.0
        }
    } else if x2 == BAD_VALUE {
        if x1 == 0.0 { BAD_VALUE } else { 1.0 }
    } else if x1 != 0.0 || x2 != 0.0 {
        1.0
    } else {
        0.0
    }
}

/// Bitwise operation on f64 values using i64 reinterpretation.
fn bitwise_op(x1: f64, x2: f64, op: impl Fn(i64, i64) -> i64) -> f64 {
    let b1 = x1.to_bits() as i64;
    let b2 = x2.to_bits() as i64;
    let result = op(b1, b2);
    f64::from_bits(result as u64)
}

fn frexp(x: f64) -> (f64, i32) {
    if x == 0.0 {
        return (0.0, 0);
    }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7FF) as i32 - 1022;
    let frac_bits = (bits & 0x800F_FFFF_FFFF_FFFF) | (1022_u64 << 52);
    (f64::from_bits(frac_bits), exp)
}

fn ldexp(frac: f64, exp: i32) -> f64 {
    frac * crate::math::powi(2.0_f64, exp)
}
