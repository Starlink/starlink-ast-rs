//! Dimensional analysis. Mirrors `DimAnal`, `LocateUnits`, `FixUnits`
//! in `reference/starlink-ast/src/unit.c`.
//!
//! `DimAnal` walks a parsed unit tree and accumulates a per-quantity
//! power vector plus an overall scale factor. The "quantities" are
//! the ten basic units the C library tracks: g, m, s, rad, K, count,
//! adu, photon, mag, pixel — see `quant_units` at `unit.c`:210 and
//! the iq++ markers at `unit.c`:2204-2269. Note that A, mol, and cd
//! are NOT tracked as DimAnal quantities in the C library (despite
//! being basic SI units), and we mirror that for parity.

use crate::constants::BAD_VALUE;

use super::table::KnownUnit;
use super::tree::{Oper, UnitNode};

/// Number of basic-unit "quantities" tracked. Mirrors `NQUANT`
/// (`unit.c`:142).
pub(crate) const NQUANT: usize = 10;

/// Indices into the 10-entry power vector. Order matches the C
/// `quant_units` array (`unit.c`:210, populated at `unit.c`:2204-2269):
/// `[g, m, s, rad, K, count, adu, photon, mag, pixel]`.
#[allow(dead_code)]
pub(crate) mod quant_idx {
    pub const G: usize = 0;
    pub const M: usize = 1;
    pub const S: usize = 2;
    pub const RAD: usize = 3;
    pub const K: usize = 4;
    pub const COUNT: usize = 5;
    pub const ADU: usize = 6;
    pub const PHOTON: usize = 7;
    pub const MAG: usize = 8;
    pub const PIXEL: usize = 9;
}

/// Map a basic-unit symbol to its `quant_idx`. Returns `None` for
/// units that aren't one of the 10 DimAnal quantities (A, mol, cd,
/// any derived unit, or any user-defined unit).
fn quant_index(unit: &KnownUnit) -> Option<usize> {
    match unit.sym {
        "g" => Some(quant_idx::G),
        "m" => Some(quant_idx::M),
        "s" => Some(quant_idx::S),
        "rad" => Some(quant_idx::RAD),
        "K" => Some(quant_idx::K),
        "count" => Some(quant_idx::COUNT),
        "adu" => Some(quant_idx::ADU),
        "photon" => Some(quant_idx::PHOTON),
        "mag" => Some(quant_idx::MAG),
        "pixel" => Some(quant_idx::PIXEL),
        _ => None,
    }
}

/// Dimensional analysis: extract per-quantity power exponents and a
/// scale factor from a tree. Returns `None` if the tree contains a
/// transcendental operator (Log/Ln/Exp) — these aren't dimensionally
/// analysable in the C sense.
///
/// Mirrors C `DimAnal` (`unit.c`:1373-1521).
pub(crate) fn dim_anal(node: &UnitNode) -> Option<([f64; NQUANT], f64)> {
    let mut powers = [0.0f64; NQUANT];
    let scale = match node.opcode {
        // Constant: dimensionless. C uses `*scale = 1.0/node->con` —
        // unit trees are post-InvertConstants so this re-inverts.
        Oper::LdCon => 1.0 / node.con,

        // Variable: must match a basic-unit quant slot.
        Oper::LdVar => {
            let unit = node.unit?;
            let idx = quant_index(unit)?;
            powers[idx] = 1.0;
            let mut s = if let Some(m) = node.mult {
                1.0 / m.scale
            } else {
                1.0
            };
            // Special-case g → kg internally.
            if unit.sym == "g" {
                s *= 0.001;
            }
            s
        }

        // Transcendentals fail dimensional analysis.
        Oper::Log | Oper::Ln | Oper::Exp => return None,

        Oper::Sqrt => {
            let (mut p, s) = dim_anal(&node.arg[0])?;
            for x in p.iter_mut() {
                *x *= 0.5;
            }
            powers = p;
            if s < 0.0 {
                return None;
            }
            s.sqrt()
        }

        Oper::Pow => {
            let (mut p, s) = dim_anal(&node.arg[0])?;
            // The C version reads `node->arg[1]->con` directly; if
            // the second argument is variable (con == BAD_VALUE) the
            // power is bogus. InvertConstants already rejects that
            // upstream, but guard defensively.
            let exponent = node.arg[1].con;
            if exponent == BAD_VALUE {
                return None;
            }
            for x in p.iter_mut() {
                *x *= exponent;
            }
            powers = p;
            s.powf(exponent)
        }

        Oper::Div => {
            let (p0, s0) = dim_anal(&node.arg[0])?;
            let (p1, s1) = dim_anal(&node.arg[1])?;
            for i in 0..NQUANT {
                powers[i] = p0[i] - p1[i];
            }
            if s1 == 0.0 {
                return None;
            }
            s0 / s1
        }

        Oper::Mult => {
            let (p0, s0) = dim_anal(&node.arg[0])?;
            let (p1, s1) = dim_anal(&node.arg[1])?;
            for i in 0..NQUANT {
                powers[i] = p0[i] + p1[i];
            }
            s0 * s1
        }

        Oper::LdPi => 1.0 / std::f64::consts::PI,
        Oper::LdE => 1.0 / std::f64::consts::E,

        Oper::Null => return None,
    };

    Some((powers, scale))
}

/// Replace every `LdVar` reference whose `name` differs from
/// `target_name` with an `LdCon(1.0)` node. Mirrors C `FixUnits`
/// (`unit.c`:1982-2070).
pub(crate) fn fix_units(node: &UnitNode, target_name: &str) -> Box<UnitNode> {
    if node.opcode == Oper::LdVar {
        let name = node.name.as_deref().unwrap_or("");
        if name != target_name {
            return UnitNode::ldcon(1.0);
        }
        return Box::new(node.clone());
    }
    let mut copy = node.clone();
    for (i, child) in node.arg.iter().enumerate() {
        copy.arg[i] = fix_units(child, target_name);
    }
    Box::new(copy)
}

/// Walk the tree and collect the names of every distinct `LdVar`.
/// Mirrors C `LocateUnits` (`unit.c`:2687-2767), but instead of
/// returning pointers into the tree we return owned name strings —
/// equality there is by `strcmp(name)` which we mirror via
/// `String == String`.
pub(crate) fn locate_units(node: &UnitNode, out: &mut Vec<String>) {
    if node.opcode == Oper::LdVar {
        if let Some(name) = node.name.as_deref()
            && !out.iter().any(|n| n == name)
        {
            out.push(name.to_string());
        }
        return;
    }
    for child in &node.arg {
        locate_units(child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::super::parser::create_tree;
    use super::super::table::find_known_unit;
    use super::*;

    #[test]
    fn dim_anal_basic() {
        // "m" → powers = [0,1,0,...], scale = 1.
        let t = create_tree("m", true).unwrap();
        let (p, s) = dim_anal(&t).unwrap();
        assert_eq!(p[quant_idx::M], 1.0);
        assert_eq!(s, 1.0);
    }

    #[test]
    fn dim_anal_mhz() {
        let t = create_tree("MHz", true).unwrap();
        let (p, s) = dim_anal(&t).unwrap();
        assert_eq!(p[quant_idx::S], -1.0);
        assert!((s - 1.0e6).abs() < 1.0); // 1 MHz = 1e6 Hz
    }

    #[test]
    fn dim_anal_log_returns_none() {
        let t = create_tree("log(Hz)", true).unwrap();
        assert!(dim_anal(&t).is_none());
    }

    #[test]
    fn locate_units_dedups() {
        let t = create_tree("m/s", true).unwrap();
        let mut units = Vec::new();
        locate_units(&t, &mut units);
        assert!(units.iter().any(|n| n == "m"));
        assert!(units.iter().any(|n| n == "s"));
    }

    #[test]
    fn quant_index_lookup_works() {
        let g = find_known_unit("g").unwrap();
        assert_eq!(quant_index(g), Some(quant_idx::G));
        let a = find_known_unit("A").unwrap();
        // A is NOT a DimAnal quantity in the C version.
        assert_eq!(quant_index(a), None);
    }
}
