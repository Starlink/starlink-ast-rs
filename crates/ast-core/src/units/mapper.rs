//! Top-level orchestration for `astUnitMapper_`. Mirrors the linear
//! branches of `unit.c`:5238-5732. The non-linear (MathMap) branch
//! lands in commit 2; this commit returns `None` for any conversion
//! that requires a non-linear transformation.
//!
//! ## Why this commit doesn't use `DimAnal`
//!
//! The C `astUnitMapper_` (see `unit.c`:5495-5694) does NOT call
//! `DimAnal`. Instead, it parses both trees with basic=true, applies
//! `FixUnits` per basic unit, runs the *full* `SimplifyTree(std=1)`
//! pass (FindFactors / CombineFactors and friends), inverts the
//! input tree, and concatenates with the output. The end result is
//! a tree whose head is `LdCon(scale)` for linear conversions and
//! `MakeMapping` translates that to a `ZoomMap`.
//!
//! `DimAnal` (`unit.c`:1373) has a quirk: `LdPi` returns `scale = 1/π`
//! and `LdE` returns `1/e`. That convention is fine for the `min →
//! 60 s` style cases (where the constant `60` was inverted to `1/60`
//! by `InvertConstants` and re-inverted by DimAnal's `1/con` rule),
//! but it breaks for any unit defined in terms of `pi` (e.g.
//! `deg = pi/180 rad`) because `LdPi` never went through
//! `InvertConstants`. The C library uses DimAnal only via
//! `astUnitAnalyser_`, which has no internal callers.
//!
//! For the linear-only subset that commit 1 covers, we implement a
//! direct tree analyser, [`analyse_tree`], that walks a parsed
//! `basic=true` unit tree and produces:
//!
//! - a per-unit-name power map, and
//! - the scaling factor that converts `1.0` of the supplied unit
//!   into the basic-unit form.
//!
//! Two unit strings are dimensionally compatible when their power
//! maps are equal, and the linear conversion factor is then
//! `in.scale / out.scale`. Commit 2 will switch this to the full
//! C pipeline so non-linear cases (`log(Hz) → Hz` etc.) produce a
//! `MathMap`.

use std::collections::HashMap;

use crate::constants::BAD_VALUE;
use crate::mappings::{DynMapping, UnitMap, ZoomMap};

use super::parser::{clean_exp, create_tree};
use super::tree::{Oper, UnitNode};

/// Walk a `basic=true` unit tree and accumulate
/// `(unit_name → power)` together with a multiplicative scale
/// factor. Returns `None` if the tree contains a `Log`, `Ln`, or
/// `Exp` operation (commit 2 territory) or any other shape we can't
/// linearly evaluate.
///
/// The scale convention is "value in basic-unit-form of `1.0` of
/// the supplied unit". So for `MHz` we get
/// `({"s": -1.0}, 1e6)` — i.e. 1 MHz = 1e6 of the basic Hz form
/// (s^-1).
fn analyse_tree(node: &UnitNode) -> Option<(HashMap<String, f64>, f64)> {
    let mut powers: HashMap<String, f64> = HashMap::new();
    let scale = match node.opcode {
        // C `DimAnal` uses `*scale = 1.0/node->con`. The trees are
        // post-`InvertConstants` so this re-inverts, returning the
        // user-intended numeric value.
        Oper::LdCon => {
            if node.con == BAD_VALUE || node.con == 0.0 {
                return None;
            }
            1.0 / node.con
        }

        // For `LdPi` and `LdE` we use the actual numeric constants.
        // C `DimAnal` uses `1/π` / `1/e` here (matching its `LdCon`
        // convention) but those nodes never went through
        // `InvertConstants` so the C path through `astUnitMapper_`
        // wouldn't have inverted them either. The SimplifyTree path
        // in the C mapper compensates via `FindFactors` /
        // `CombineFactors`; we just use the natural constants.
        Oper::LdPi => std::f64::consts::PI,
        Oper::LdE => std::f64::consts::E,

        Oper::LdVar => {
            // The unit's name (no multiplier prefix) is the dimension
            // key. Multiplier scales contribute to the numeric scale
            // factor. As in `DimAnal` (`unit.c`:1456-1457), the
            // multiplier on a basic-unit LDVAR contributes
            // `1/mult.scale`, and the special case `g → kg` adds an
            // extra `0.001` factor.
            let name = node.name.as_deref()?;
            *powers.entry(name.to_string()).or_insert(0.0) += 1.0;
            let mut s = if let Some(m) = node.mult {
                1.0 / m.scale
            } else {
                1.0
            };
            if name == "g" {
                s *= 0.001;
            }
            s
        }

        // Transcendentals — defer to commit 2 (MathMap path).
        Oper::Log | Oper::Ln | Oper::Exp => return None,

        Oper::Sqrt => {
            let (mut p, s) = analyse_tree(&node.arg[0])?;
            for v in p.values_mut() {
                *v *= 0.5;
            }
            powers = p;
            if s < 0.0 {
                return None;
            }
            s.sqrt()
        }

        Oper::Pow => {
            let exponent = node.arg[1].con;
            if exponent == BAD_VALUE {
                return None;
            }
            let (mut p, s) = analyse_tree(&node.arg[0])?;
            for v in p.values_mut() {
                *v *= exponent;
            }
            powers = p;
            s.powf(exponent)
        }

        Oper::Mult => {
            let (p0, s0) = analyse_tree(&node.arg[0])?;
            let (p1, s1) = analyse_tree(&node.arg[1])?;
            for (k, v) in p0 {
                *powers.entry(k).or_insert(0.0) += v;
            }
            for (k, v) in p1 {
                *powers.entry(k).or_insert(0.0) += v;
            }
            s0 * s1
        }

        Oper::Div => {
            let (p0, s0) = analyse_tree(&node.arg[0])?;
            let (p1, s1) = analyse_tree(&node.arg[1])?;
            for (k, v) in p0 {
                *powers.entry(k).or_insert(0.0) += v;
            }
            for (k, v) in p1 {
                *powers.entry(k).or_insert(0.0) -= v;
            }
            if s1 == 0.0 {
                return None;
            }
            s0 / s1
        }

        Oper::Null => return None,
    };

    // Discard zero-power entries so `m * s / s` and `m` compare equal.
    powers.retain(|_, v| v.abs() > 1.0e-12);

    Some((powers, scale))
}

fn powers_compatible(a: &HashMap<String, f64>, b: &HashMap<String, f64>) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (k, v) in a {
        match b.get(k) {
            Some(w) if (v - w).abs() < 1.0e-12 => {}
            _ => return false,
        }
    }
    true
}

/// Build a `Mapping` between two unit strings. Returns `None` if
/// the units are dimensionally incompatible, unparseable, or
/// (in this commit) require a non-linear transformation.
///
/// Mirrors C `astUnitMapper_` (`unit.c`:5238).
pub(crate) fn unit_mapper(in_unit: &str, out_unit: &str) -> Option<DynMapping> {
    // Strict identical-string fast path — `unit.c`:5468.
    if in_unit == out_unit {
        return Some(Box::new(UnitMap::new(1)));
    }

    // Equivalent normalised strings — also a UnitMap.
    let cleaned_in = clean_exp(in_unit);
    let cleaned_out = clean_exp(out_unit);
    if cleaned_in == cleaned_out {
        return Some(Box::new(UnitMap::new(1)));
    }

    // Parse both. If either fails, no mapping.
    let in_tree = create_tree(in_unit, true)?;
    let out_tree = create_tree(out_unit, true)?;

    // Analyse both for power vectors and scale.
    let (in_powers, in_scale) = analyse_tree(&in_tree)?;
    let (out_powers, out_scale) = analyse_tree(&out_tree)?;

    // Powers must match exactly for the conversion to be a single
    // linear scaling.
    if !powers_compatible(&in_powers, &out_powers) {
        return None;
    }

    if out_scale == 0.0 {
        return None;
    }
    let factor = in_scale / out_scale;

    if (factor - 1.0).abs() < 1.0e-15 {
        Some(Box::new(UnitMap::new(1)))
    } else {
        ZoomMap::new(1, factor)
            .ok()
            .map(|z| Box::new(z) as DynMapping)
    }
}
