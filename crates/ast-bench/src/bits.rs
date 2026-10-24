//! A hash of the output bits of every corpus transform, so a change that
//! must not alter results can be checked against a base commit exactly.
//!
//! Hashes depend on the platform's libm, so they are compared with a base
//! commit built on the same machine, never with a committed file.

use std::collections::BTreeMap;

use starlink_ast::coords::PointSet;

use crate::cases::Case;
use crate::load::{Loaded, load_fixture};
use crate::oracle::Section;

/// Points per case: every oracle row, repeated to more than one 8192-point
/// series batch and ending part-way through the second.
pub const BITS_N: usize = 10_000;
const _: () = assert!(BITS_N > 8192 && BITS_N % 8192 != 0);

/// FNV-1a over the bits of every output value, point by point.
fn hash(out: &PointSet) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |x: u64| {
        for b in x.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    eat(out.npoint() as u64);
    eat(out.ncoord() as u64);
    for i in 0..out.npoint() {
        for &v in out.point(i).iter() {
            eat(v.to_bits());
        }
    }
    format!("{h:016x}")
}

/// Every row of `s`, repeated to [`BITS_N`] points.
fn rows(s: &Section) -> PointSet {
    let n = s.npoint();
    let idx: Vec<usize> = (0..BITS_N).map(|i| i % n).collect();
    crate::points::rows_to_pointset(s, &idx)
}

/// The hashes of `case` through `transform` and through
/// `transform_nan_protocol`; a case that cannot load or transform gives the
/// reason in place of a hash, so a change in failures is a difference too.
pub fn case_bits(case: &Case) -> (String, String) {
    let s = &case.section;
    let obj = match load_fixture(&case.root, &s.relpath) {
        Ok(Loaded::Mapping(obj)) => obj,
        Ok(Loaded::Untransformable(why)) => {
            return (format!("untransformable: {why}"), String::new());
        }
        Err(e) => return (format!("load error: {e}"), String::new()),
    };
    let map = obj.as_mapping().expect("loaded as a Mapping");
    let pts = rows(s);
    let plain = match map.transform(&pts, s.forward) {
        Ok(out) => hash(&out),
        Err(e) => format!("error {:?}", e.code()),
    };
    let nan = match map.transform_nan_protocol(&pts, s.forward) {
        Ok(out) => hash(&out),
        Err(e) => format!("error {:?}", e.code()),
    };
    (plain, nan)
}

/// `key\tplain\tnan` for every case of every oracle, in a fixed order. The
/// key numbers repeated sections so each is distinct.
pub fn all_bits() -> Result<String, String> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut out = String::new();
    for case in crate::sweep::all_cases()? {
        let s = &case.section;
        let dir = if s.forward { "forward" } else { "inverse" };
        let base = format!("{} {dir}", s.relpath);
        let k = seen.entry(base.clone()).or_default();
        let key = format!("{base} #{k}");
        *k += 1;
        let (plain, nan) = case_bits(&case);
        out += &format!("{key}\t{plain}\t{nan}\n");
    }
    Ok(out)
}

/// The lines of `bits` output by key.
fn parse(text: &str) -> Result<BTreeMap<String, String>, String> {
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            l.split_once('\t')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| format!("bad bits line {l:?}"))
        })
        .collect()
}

/// Every case whose hashes differ between `base` and `cur`, or that is in
/// only one of them.
pub fn diff_bits(base: &str, cur: &str) -> Result<Vec<String>, String> {
    let (b, c) = (parse(base)?, parse(cur)?);
    let mut out = Vec::new();
    for (k, bv) in &b {
        match c.get(k) {
            None => out.push(format!("only in the base: {k}")),
            Some(cv) if cv != bv => out.push(format!("{k}: {bv} -> {cv}")),
            Some(_) => {}
        }
    }
    for k in c.keys().filter(|k| !b.contains_key(*k)) {
        out.push(format!("only in the current build: {k}"));
    }
    Ok(out)
}

/// Every case whose output differs between the two paths: `transform` and
/// the NaN protocol must give the same bits. A case that does not load, or
/// has nothing to transform, is not compared.
pub fn path_mismatches(text: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for l in text.lines().filter(|l| !l.is_empty()) {
        let mut f = l.split('\t');
        let (Some(key), Some(plain), Some(nan)) = (f.next(), f.next(), f.next()) else {
            return Err(format!("bad bits line {l:?}"));
        };
        let compared = !(plain.starts_with("untransformable") || plain.starts_with("load error"));
        if compared && plain != nan {
            out.push(format!("{key}: transform {plain}, NaN protocol {nan}"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_bits_have_no_difference() {
        let a = "x.map forward #0\t00ff\t00fe\n";
        assert!(diff_bits(a, a).unwrap().is_empty());
    }

    #[test]
    fn a_changed_hash_is_a_difference() {
        let base = "x.map forward #0\t00ff\t00fe\n";
        let cur = "x.map forward #0\t00ff\t00fd\n";
        let d = diff_bits(base, cur).unwrap();
        assert_eq!(d.len(), 1);
        assert!(d[0].contains("x.map forward #0"), "{d:?}");
    }

    #[test]
    fn a_case_in_only_one_side_is_a_difference() {
        let base = "x.map forward #0\t00ff\t00fe\n";
        let cur = "";
        assert_eq!(diff_bits(base, cur).unwrap().len(), 1);
    }

    #[test]
    fn a_bench_case_hashes_the_same_twice_and_crosses_a_batch() {
        let cases = crate::cases::load_set("bench").unwrap();
        let c = cases
            .iter()
            .find(|c| c.section.relpath == "cmpmap_series.map")
            .unwrap();
        assert_eq!(case_bits(c), case_bits(c));
        assert!(case_bits(c).0.len() == 16, "{:?}", case_bits(c));
    }

    #[test]
    fn the_two_paths_must_agree() {
        let text = "a.map forward #0\t00ff\t00ff\n\
                    b.map forward #0\t00ff\t00fe\n\
                    c.ast forward #0\tuntransformable: bare Frame\t\n";
        let m = path_mismatches(text).unwrap();
        assert_eq!(m.len(), 1, "{m:?}");
        assert!(m[0].starts_with("b.map forward #0"), "{m:?}");
    }
}
