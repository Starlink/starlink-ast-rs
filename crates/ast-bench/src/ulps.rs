// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The output values of every corpus transform at its oracle rows, and how
//! far they moved from a base commit's, in units in the last place.
//!
//! [`crate::bits`] says whether any output changed; this says by how much,
//! so a change that is allowed to move results can be inspected case by
//! case and axis by axis. Like the hashes, the values depend on the
//! platform's libm, so they are only compared with a base commit built on
//! the same machine.

use std::collections::BTreeMap;

use crate::bits::keyed_cases;
use crate::cases::Case;
use crate::load::{Loaded, load_fixture};
use crate::roman::ulps;

/// How far one output axis of one case moved from the base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UlpDiff {
    /// The case's key, as `ast-bench bits` prints it.
    pub key: String,
    /// The output axis, 1-based.
    pub axis: usize,
    /// Values on this axis whose bits differ.
    pub differ: usize,
    /// Values on this axis.
    pub values: usize,
    /// The largest difference in units in the last place; `u64::MAX` when a
    /// sign changed or either value is NaN, which includes a value becoming
    /// or ceasing to be `AST__BAD`.
    pub max_ulps: u64,
}

/// `ncoord\tbits bits …` for the outputs of `case` at each of its oracle
/// rows, point by point, or the reason it has none.
fn case_values(case: &Case) -> String {
    let s = &case.section;
    let obj = match load_fixture(&case.root, &s.relpath) {
        Ok(Loaded::Mapping(obj)) => obj,
        Ok(Loaded::Untransformable(why)) => return format!("untransformable: {why}"),
        Err(e) => return format!("load error: {e}"),
    };
    let map = obj.as_mapping().expect("loaded as a Mapping");
    let idx: Vec<usize> = (0..s.npoint()).collect();
    let pts = crate::points::rows_to_pointset(s, &idx);
    match map.transform(&pts, s.forward) {
        Ok(out) => {
            let mut line = format!("{}\t", out.ncoord());
            let bits: Vec<String> = (0..out.npoint())
                .flat_map(|i| {
                    out.point(i)
                        .iter()
                        .map(|v| format!("{:016x}", v.to_bits()))
                        .collect::<Vec<_>>()
                })
                .collect();
            line += &bits.join(" ");
            line
        }
        Err(e) => format!("error {:?}", e.code()),
    }
}

/// `key\t` and the case's values for every case of every oracle, in the order
/// and with the keys of `ast-bench bits`.
pub fn all_values() -> Result<String, String> {
    let mut out = String::new();
    for (key, case) in keyed_cases()? {
        out += &format!("{key}\t{}\n", case_values(&case));
    }
    Ok(out)
}

/// One line of [`all_values`] output: the values with their coordinate
/// count, or the reason there are none.
enum Values {
    Bits { ncoord: usize, bits: Vec<u64> },
    Failure(String),
}

fn parse(text: &str) -> Result<BTreeMap<String, Values>, String> {
    let mut out = BTreeMap::new();
    for l in text.lines().filter(|l| !l.is_empty()) {
        let (key, rest) = l
            .split_once('\t')
            .ok_or_else(|| format!("bad values line {l:?}"))?;
        let values = match rest.split_once('\t') {
            Some((n, vals)) if n.parse::<usize>().is_ok() => Values::Bits {
                ncoord: n.parse().unwrap(),
                bits: vals
                    .split(' ')
                    .filter(|v| !v.is_empty())
                    .map(|v| u64::from_str_radix(v, 16).map_err(|e| format!("{key}: {v:?}: {e}")))
                    .collect::<Result<_, _>>()?,
            },
            _ => Values::Failure(rest.to_string()),
        };
        out.insert(key.to_string(), values);
    }
    Ok(out)
}

/// Every (case, axis) whose values differ between `base` and `cur`, the
/// largest difference first. A case in only one of them, or whose failure
/// or shape differs, is an error, since its values cannot be compared.
pub fn diff_ulps(base: &str, cur: &str) -> Result<Vec<UlpDiff>, String> {
    let (b, c) = (parse(base)?, parse(cur)?);
    if let Some(k) = b.keys().find(|k| !c.contains_key(*k)) {
        return Err(format!("only in the base: {k}"));
    }
    if let Some(k) = c.keys().find(|k| !b.contains_key(*k)) {
        return Err(format!("only in the current build: {k}"));
    }
    let mut out = Vec::new();
    for (key, bv) in &b {
        match (bv, &c[key]) {
            (Values::Failure(x), Values::Failure(y)) if x == y => {}
            (
                Values::Bits {
                    ncoord: nb,
                    bits: xb,
                },
                Values::Bits {
                    ncoord: nc,
                    bits: xc,
                },
            ) if nb == nc && xb.len() == xc.len() => {
                let mut axes: Vec<UlpDiff> = (1..=*nb)
                    .map(|axis| UlpDiff {
                        key: key.clone(),
                        axis,
                        differ: 0,
                        values: 0,
                        max_ulps: 0,
                    })
                    .collect();
                for (i, (&x, &y)) in xb.iter().zip(xc).enumerate() {
                    let a = &mut axes[i % nb];
                    a.values += 1;
                    let d = ulps(f64::from_bits(x), f64::from_bits(y));
                    if d != 0 {
                        a.differ += 1;
                        a.max_ulps = a.max_ulps.max(d);
                    }
                }
                out.extend(axes.into_iter().filter(|a| a.differ > 0));
            }
            _ => {
                return Err(format!(
                    "{key}: the base and current build differ in what they produce"
                ));
            }
        }
    }
    out.sort_by(|x, y| y.max_ulps.cmp(&x.max_ulps).then_with(|| x.key.cmp(&y.key)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_has_no_diffs() {
        let t = "a\t2\t3ff0000000000000 4000000000000000\n";
        assert!(diff_ulps(t, t).unwrap().is_empty());
    }

    #[test]
    fn one_ulp_on_axis_two_is_reported() {
        let base = "a\t2\t3ff0000000000000 4000000000000000 3ff0000000000000 4000000000000000\n";
        let cur = "a\t2\t3ff0000000000000 4000000000000001 3ff0000000000000 4000000000000000\n";
        let d = diff_ulps(base, cur).unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(
            (d[0].axis, d[0].differ, d[0].values, d[0].max_ulps),
            (2, 1, 2, 1)
        );
    }

    #[test]
    fn a_key_on_one_side_only_is_an_error() {
        assert!(diff_ulps("a\t1\t3ff0000000000000\n", "b\t1\t3ff0000000000000\n").is_err());
    }

    #[test]
    fn a_changed_failure_text_is_an_error() {
        assert!(diff_ulps("a\terror AST__BADIN\n", "a\t1\t3ff0000000000000\n").is_err());
    }
}
