// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Every projection checked, bit for bit, against values captured from the
//! wcslib C it was ported from.
//!
//! `tests/wcslib_golden/projections.tsv` lists, for each projection and
//! for the parameter sets that matter to it, the status and the output
//! bits of forward and reverse calls at points that include the poles, the
//! multiples of 45 degrees, the edges of the quad-cube and HEALPix facets,
//! and points outside each projection's domain; its header says what each
//! line holds.
//! The published archive does not carry it, so the test is ignored there.
//! Every result is compared before the test fails, and the failure names the
//! first few that differ (projection, parameter set and input) and counts the
//! rest, with each projection's largest difference in ULP.
//! Two NaN outputs compare equal whatever their sign and payload, since the
//! sign of an arithmetic NaN is not specified and AST tests only `is_nan`;
//! every other output is compared by its bits.

use std::collections::BTreeMap;

use super::WcsProjection;
use super::prj::{Prj, Status};

fn bits(tok: &str) -> f64 {
    if tok == "nan" {
        f64::NAN
    } else {
        f64::from_bits(u64::from_str_radix(tok, 16).expect("hex bits"))
    }
}

fn same_bits_or_both_nan(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
}

/// How many doubles apart `a` and `b` are; `u64::MAX` when they differ in
/// sign or one is not finite.
fn ulps_apart(a: f64, b: f64) -> u64 {
    if same_bits_or_both_nan(a, b) {
        0
    } else if a.is_finite() && b.is_finite() && a.is_sign_negative() == b.is_sign_negative() {
        a.to_bits().abs_diff(b.to_bits())
    } else {
        u64::MAX
    }
}

/// The results that differ from the recorded ones: the first few in full,
/// the rest only counted, so that one run shows how far another platform's
/// libm moves them.
#[derive(Default)]
struct Differences {
    shown: Vec<String>,
    count: usize,
}

impl Differences {
    const SHOWN: usize = 20;

    fn add(&mut self, what: String) {
        self.count += 1;
        if self.shown.len() < Self::SHOWN {
            self.shown.push(what);
        }
    }
}

/// A recorded result: the status and, when it is 0, the two outputs.
struct Expected {
    status: i32,
    out: Option<(f64, f64)>,
}

fn expected(toks: &[&str]) -> Expected {
    let status: i32 = toks[0].parse().expect("status");
    let out = (status == 0).then(|| (bits(toks[1]), bits(toks[2])));
    Expected { status, out }
}

/// The comparison counts for one projection.
#[derive(Default)]
struct Tally {
    sets: usize,
    forward: usize,
    forward_ok: usize,
    reverse: usize,
    reverse_ok: usize,
    status_differ: usize,
    output_differ: usize,
    max_ulps: u64,
    max_abs: f64,
    /// The result furthest from the recorded one, relative to the larger
    /// of 1 and the recorded value, with that relative difference.
    worst: Option<(f64, String)>,
}

impl Tally {
    /// Records a projected pair against the recorded one.
    fn compare(
        &mut self,
        diffs: &mut Differences,
        what: String,
        got: (f64, f64),
        want: (f64, f64),
    ) {
        let ulps = ulps_apart(got.0, want.0).max(ulps_apart(got.1, want.1));
        if ulps > 0 {
            self.output_differ += 1;
            self.max_ulps = self.max_ulps.max(ulps);
            // An output near zero, where a projection's terms cancel, can be
            // many ULP of itself from the recorded one while within an ULP of
            // the projection's scale, so the absolute difference is counted
            // too.
            let abs = (got.0 - want.0).abs().max((got.1 - want.1).abs());
            if abs.is_finite() {
                self.max_abs = self.max_abs.max(abs);
            }
            let scaled = |g: f64, w: f64| (g - w).abs() / w.abs().max(1.0);
            // A NaN against a number is as far as results get.
            let rel = match scaled(got.0, want.0).max(scaled(got.1, want.1)) {
                r if r.is_nan() => f64::INFINITY,
                r => r,
            };
            let line = format!(
                "{what}: {:e} {:e}, expected {:e} {:e}",
                got.0, got.1, want.0, want.1
            );
            if self.worst.as_ref().is_none_or(|(r, _)| rel > *r) {
                self.worst = Some((rel, line.clone()));
            }
            diffs.add(line);
        }
    }

    /// Records a status against the recorded one.
    fn status(&mut self, diffs: &mut Differences, what: &str, got: Status, want: i32) {
        if got as i32 != want {
            self.status_differ += 1;
            let line = format!("{what}: status {}, expected {want}", got as i32);
            eprintln!("{line}");
            diffs.add(line);
        }
    }
}

// The values were captured with Apple's libm, and the projections call the
// platform libm, whose last bit can differ elsewhere; run with `--ignored` on
// another platform to check whether it agrees there too.
#[test]
#[cfg_attr(
    all(
        not(ast_dist_tier = "cutdown"),
        not(all(target_os = "macos", target_arch = "aarch64"))
    ),
    ignore = "golden values captured with macOS aarch64 libm"
)]
#[cfg_attr(ast_dist_tier = "cutdown", ignore = "golden values not distributed")]
fn projections_match_the_captured_wcslib_values() {
    let path = format!(
        "{}/tests/wcslib_golden/projections.tsv",
        env!("CARGO_MANIFEST_DIR")
    );
    let golden = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut fwd_inputs = Vec::new();
    let mut rev_inputs = Vec::new();
    let mut tally: BTreeMap<&'static str, Tally> = BTreeMap::new();
    let mut diffs = Differences::default();

    let mut lines = golden
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .peekable();
    while let Some(line) = lines.peek() {
        let t: Vec<&str> = line.split(' ').collect();
        match t[0] {
            "FI" => fwd_inputs.push((bits(t[1]), bits(t[2]))),
            "RI" => rev_inputs.push((bits(t[1]), bits(t[2]))),
            _ => break,
        }
        lines.next();
    }
    assert!(!fwd_inputs.is_empty() && !rev_inputs.is_empty());

    while let Some(line) = lines.next() {
        let t: Vec<&str> = line.split(' ').collect();
        assert_eq!(t[0], "S", "expected a set header, found {line}");
        let proj = WcsProjection::from_code(t[1]).expect("projection code");
        let stride: usize = t[2].parse().unwrap();
        let tpn_tan = t[3] == "1";
        let set = format!("{} set [{}]", proj.code(), t[4]);

        let mut params: Vec<Vec<Option<f64>>> = vec![Vec::new(), Vec::new()];
        if t[4] != "-" {
            for pv in t[4].split(',') {
                let f: Vec<&str> = pv.split(':').collect();
                let (axis, m): (usize, usize) = (f[0].parse().unwrap(), f[1].parse().unwrap());
                let p = &mut params[if axis == 2 { 0 } else { 1 }];
                if m >= p.len() {
                    p.resize(m + 1, None);
                }
                p[m] = Some(bits(f[2]));
            }
        }
        let prj = Prj::new(proj, &params, tpn_tan).unwrap();
        let tally = tally.entry(proj.code()).or_default();
        tally.sets += 1;

        let a: Vec<&str> = lines.next().expect("A line").split(' ').collect();
        assert_eq!(a[0], "A");
        for axis in 0..2 {
            for m in 0..=6 {
                let want = a[1 + axis * 7 + m];
                let got = prj.get_param(axis, m).unwrap();
                assert!(
                    want == "nan" && got.is_nan() || want == format!("{:016x}", got.to_bits()),
                    "{set}: parameter {axis}/{m} is {:016x}, expected {want}",
                    got.to_bits()
                );
            }
        }

        let next = lines.next().expect("results");
        let t: Vec<&str> = next.split(' ').collect();
        if t[0] == "E" {
            // A failed set-up gives its status at the first point that
            // reaches the projection.
            let (fwd, rev): (i32, i32) = (t[1].parse().unwrap(), t[2].parse().unwrap());
            assert_ne!(prj.fwd_raw(0.0, 0.0).0, Status::Ok, "{set}: set-up");
            assert_eq!(prj.fwd_raw(0.0, 0.0).0 as i32, fwd, "{set}: forward set-up");
            assert_eq!(prj.rev_raw(0.0, 0.0).0 as i32, rev, "{set}: reverse set-up");
            continue;
        }

        let used = |n: usize| (0..n).step_by(stride);
        let mut first = Some(next);
        let mut take = || first.take().or_else(|| lines.next()).expect("result line");

        for i in used(fwd_inputs.len()) {
            let (phi, theta) = fwd_inputs[i];
            let t: Vec<&str> = take().split(' ').collect();
            assert_eq!(t[0], "F", "{set}: layout");
            let want = expected(&t[1..]);
            let (status, x, y) = prj.fwd_raw(phi, theta);
            let what = format!("{set} forward({phi:e}, {theta:e})");
            tally.status(&mut diffs, &what, status, want.status);
            tally.forward += 1;
            if let Some(w) = want.out {
                tally.forward_ok += 1;
                if status == Status::Ok {
                    tally.compare(&mut diffs, what, (x, y), w);
                }
            }
        }
        for i in used(rev_inputs.len()) {
            let (x, y) = rev_inputs[i];
            let t: Vec<&str> = take().split(' ').collect();
            assert_eq!(t[0], "R", "{set}: layout");
            check_reverse(&prj, &set, x, y, &t[1..], tally, &mut diffs);
        }
        while let Some(l) = lines.peek() {
            if !l.starts_with("X ") {
                break;
            }
            let t: Vec<&str> = lines.next().unwrap().split(' ').collect();
            check_reverse(
                &prj,
                &set,
                bits(t[1]),
                bits(t[2]),
                &t[3..],
                tally,
                &mut diffs,
            );
        }
    }

    // Every projection but the deprecated aliases is covered, with points
    // that project and, for each direction, a recorded result to match.
    use WcsProjection::*;
    for proj in [
        Azp, Szp, Tan, Stg, Sin, Arc, Zpn, Zea, Air, Cyp, Cea, Car, Mer, Sfl, Par, Mol, Ait, Cop,
        Coe, Cod, Coo, Bon, Pco, Tsc, Csc, Qsc, Hpx, Xph, Tpn,
    ] {
        let t = tally
            .get(proj.code())
            .unwrap_or_else(|| panic!("{} has no golden values", proj.code()));
        assert!(
            t.forward_ok > 0 && t.reverse_ok > 0,
            "{} has no compared outputs",
            proj.code()
        );
        eprintln!(
            "{}: {} sets, {} forward ({} projected), {} reverse ({} projected); \
             {} statuses and {} outputs differ, by at most {} ULP and {:e} absolute",
            proj.code(),
            t.sets,
            t.forward,
            t.forward_ok,
            t.reverse,
            t.reverse_ok,
            t.status_differ,
            t.output_differ,
            t.max_ulps,
            t.max_abs
        );
        if let Some((rel, line)) = &t.worst {
            eprintln!("  furthest, {rel:e} relative: {line}");
        }
    }
    assert!(
        diffs.count == 0,
        "{} results differ from the recorded ones; the first {}:\n{}",
        diffs.count,
        diffs.shown.len(),
        diffs.shown.join("\n")
    );
}

fn check_reverse(
    prj: &Prj,
    set: &str,
    x: f64,
    y: f64,
    toks: &[&str],
    tally: &mut Tally,
    diffs: &mut Differences,
) {
    let want = expected(toks);
    let (status, phi, theta) = prj.rev_raw(x, y);
    let what = format!("{set} reverse({x:e}, {y:e})");
    tally.status(diffs, &what, status, want.status);
    tally.reverse += 1;
    if let Some(w) = want.out {
        tally.reverse_ok += 1;
        if status == Status::Ok {
            tally.compare(diffs, what, (phi, theta), w);
        }
    }
}
