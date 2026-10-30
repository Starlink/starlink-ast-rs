// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Every PAL routine the crate uses, checked bit for bit against values
//! captured from the C it was ported from.
//!
//! `tests/erfa_pal_golden/roots.tsv` lists, for each routine in this
//! module's interface, 180 to 900 calls: the inputs and the output bits.
//! The calls include the epochs 1000 to 3000, the poles and the latitudes just
//! short of them, the instants of every leap second and the doubles next to
//! them, singular matrices, and the special values (`BAD_VALUE`, infinities,
//! NaN, subnormals) wherever C is defined on them; its header says what each
//! line holds.
//! The published archive does not carry it, so the test is ignored there.
//! Every result is compared before the test fails, and the failure names the
//! first few that differ and counts the rest, with each routine's largest
//! difference in ULP.
//! Two NaN outputs compare equal whatever their sign and payload, since the
//! sign of an arithmetic NaN is not specified and AST tests only `is_nan`;
//! every other output is compared by its bits.
//!
//! Inputs on which C is undefined (a NaN or out-of-range `double` to integer
//! cast, an `abs(INT_MIN)`) are not in the file; the port's value for them is
//! pinned by `undefined_inputs_give_the_documented_values`.

use std::collections::BTreeMap;

use super::*;

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

/// The orders `deuler` is called with; a line holds an index into this list.
const EULER_ORDERS: [&str; 16] = [
    "", "x", "zyx", "ZXZ", "xyzx", "abc", "z1y", "XYZ", "123", "321", "zy", "Zy3", "y?z", "zzz",
    "z\0y", "zyxz",
];

fn m3(x: &[f64]) -> [[f64; 3]; 3] {
    [[x[0], x[1], x[2]], [x[3], x[4], x[5]], [x[6], x[7], x[8]]]
}

fn flat(m: [[f64; 3]; 3]) -> Vec<f64> {
    m.iter().flatten().copied().collect()
}

fn v3(x: &[f64]) -> [f64; 3] {
    [x[0], x[1], x[2]]
}

fn amprms(x: &[f64]) -> [f64; 21] {
    let mut a = [0.0; 21];
    a.copy_from_slice(&x[..21]);
    a
}

fn pair(p: (f64, f64)) -> Vec<f64> {
    vec![p.0, p.1]
}

/// Runs the port's routine `root` on `x`, in the layout the file records:
/// the doubles of the inputs in argument order (an integer as its value, a
/// matrix by rows) and the outputs in the same way.
/// A status is its C value, so a routine that fails gives only the status
/// where C leaves its outputs unwritten.
fn run(root: &str, x: &[f64]) -> Vec<f64> {
    match root {
        "epj" => vec![epj(x[0])],
        "epj2d" => vec![epj2d(x[0])],
        "epb" => vec![epb(x[0])],
        "epb2d" => vec![epb2d(x[0])],
        "dcs2c" => dcs2c(x[0], x[1]).to_vec(),
        "dcc2s" => pair(dcc2s(&v3(x))),
        "dmxv" => dmxv(&m3(x), &v3(&x[9..])).to_vec(),
        "dimxv" => dimxv(&m3(x), &v3(&x[9..])).to_vec(),
        "dmxm" => flat(dmxm(&m3(x), &m3(&x[9..]))),
        "dvxv" => dvxv(&v3(x), &v3(&x[3..])).to_vec(),
        "dvdv" => vec![dvdv(&v3(x), &v3(&x[3..]))],
        "dvn" => {
            let (uv, vm) = dvn(&v3(x));
            vec![uv[0], uv[1], uv[2], vm]
        }
        "dmat" => {
            let n = x[0] as usize;
            let mut a = x[1..1 + n * n].to_vec();
            let mut y = x[1 + n * n..1 + n * n + n].to_vec();
            let (ok, d) = dmat_det(n, &mut a, &mut y);
            let mut out = vec![f64::from(u8::from(ok)), d];
            out.extend(a);
            out.extend(y);
            out
        }
        "galeq" => pair(galeq(x[0], x[1])),
        "eqgal" => pair(eqgal(x[0], x[1])),
        "galsup" => pair(galsup(x[0], x[1])),
        "supgal" => pair(supgal(x[0], x[1])),
        "fk45z" => pair(fk45z(x[0], x[1], x[2])),
        "fk54z" => {
            let r = fk54z(x[0], x[1], x[2]);
            vec![r.0, r.1, r.2, r.3]
        }
        "fk5hz" => pair(fk5hz(x[0], x[1], x[2])),
        "hfk5z" => {
            let r = hfk5z(x[0], x[1], x[2]);
            vec![r.0, r.1, r.2, r.3]
        }
        "prec" => flat(prec(x[0], x[1])),
        "prebn" => flat(prebn(x[0], x[1])),
        "ecmat" => flat(ecmat(x[0])),
        "deuler" => flat(deuler(EULER_ORDERS[x[0] as usize], x[1], x[2], x[3])),
        "addet" => pair(addet(x[0], x[1], x[2])),
        "subet" => pair(subet(x[0], x[1], x[2])),
        "mappa" => mappa(x[0], x[1]).to_vec(),
        "ampqk" => pair(ampqk(x[0], x[1], &amprms(&x[2..]))),
        "mapqkz" => pair(mapqkz(x[0], x[1], &amprms(&x[2..]))),
        "drange" => vec![drange(x[0])],
        "dranrm" => vec![dranrm(x[0])],
        "dsep" => vec![dsep(x[0], x[1], x[2], x[3])],
        "dbear" => vec![dbear(x[0], x[1], x[2], x[3])],
        "evp" => {
            let (a, b, c, d) = evp(x[0], x[1]);
            [a, b, c, d].concat()
        }
        "geoc" => pair(geoc(x[0], x[1])),
        "dav2m" => flat(dav2m(&v3(x))),
        "dat" => vec![dat(x[0])],
        "gmst" => vec![gmst(x[0])],
        "eqeqx" => vec![eqeqx(x[0])],
        "gd2gc" => match gd2gc(x[0], x[1], x[2]) {
            Some(v) => vec![1.0, v[0], v[1], v[2]],
            None => vec![0.0],
        },
        "gc2gd" => match gc2gd(v3(x)) {
            Some((l, p, h)) => vec![1.0, l, p, h],
            None => vec![0.0],
        },
        "pvobs" => pvobs(x[0], x[1], x[2]).to_vec(),
        "rvgalc" => vec![rvgalc(x[0], x[1])],
        "rvlg" => vec![rvlg(x[0], x[1])],
        "rvlsrd" => vec![rvlsrd(x[0], x[1])],
        "rvlsrk" => vec![rvlsrk(x[0], x[1])],
        "djcl" => match djcl(x[0]) {
            Ok((y, m, d, f)) => vec![0.0, f64::from(y), f64::from(m), f64::from(d), f],
            Err(_) => vec![-1.0],
        },
        "cldj" => {
            use crate::erfa::{Cal2jdError, Cal2jdWarning};
            match cldj(x[0] as i32, x[1] as i32, x[2] as i32) {
                Err(Cal2jdError::BadYear) => vec![-1.0],
                Err(Cal2jdError::BadMonth) => vec![-2.0],
                Ok((m, None)) => vec![0.0, m],
                Ok((m, Some(Cal2jdWarning::BadDay))) => vec![-3.0, m],
            }
        }
        "dd2tf" => {
            let (s, f) = dd2tf(x[0] as i32, x[1]);
            let mut out = vec![f64::from(s as u8)];
            out.extend(f.map(f64::from));
            out
        }
        "dtf2d" => {
            use crate::erfa::Tf2dWarning;
            let (d, w) = dtf2d(x[0] as i32, x[1] as i32, x[2]);
            let status = match w {
                None => 0.0,
                Some(Tf2dWarning::Hour) => 1.0,
                Some(Tf2dWarning::Minute) => 2.0,
                Some(Tf2dWarning::Second) => 3.0,
            };
            vec![status, d]
        }
        _ => panic!("unknown root {root}"),
    }
}

/// The results that differ from the recorded ones: the first few in full,
/// the rest only counted, so that one run shows how far another platform
/// moves them.
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

/// The comparison counts for one root.
#[derive(Default)]
struct Tally {
    calls: usize,
    differ: usize,
    max_ulps: u64,
}

// The values were captured with Apple's libm, and the routines call the
// platform libm for the trigonometric functions, whose last bit can differ
// elsewhere; run with `--ignored` on another platform to check whether it
// agrees there too.
#[test]
#[cfg_attr(
    all(
        not(ast_dist_tier = "cutdown"),
        not(all(target_os = "macos", target_arch = "aarch64"))
    ),
    ignore = "golden values captured with macOS aarch64 libm"
)]
#[cfg_attr(ast_dist_tier = "cutdown", ignore = "golden values not distributed")]
fn roots_match_the_captured_c_values() {
    let path = format!(
        "{}/tests/erfa_pal_golden/roots.tsv",
        env!("CARGO_MANIFEST_DIR")
    );
    let golden = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut tally: BTreeMap<String, Tally> = BTreeMap::new();
    let mut diffs = Differences::default();
    for line in golden
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let toks: Vec<&str> = line.split(' ').collect();
        let eq = toks.iter().position(|&t| t == "=").expect("separator");
        let root = toks[0];
        let input: Vec<f64> = toks[1..eq].iter().map(|t| bits(t)).collect();
        let want: Vec<f64> = toks[eq + 1..].iter().map(|t| bits(t)).collect();
        let got = run(root, &input);
        let t = tally.entry(root.to_string()).or_default();
        t.calls += 1;
        let ulps = if got.len() == want.len() {
            got.iter()
                .zip(&want)
                .map(|(&g, &w)| ulps_apart(g, w))
                .max()
                .unwrap_or(0)
        } else {
            u64::MAX
        };
        if ulps > 0 {
            t.differ += 1;
            t.max_ulps = t.max_ulps.max(ulps);
            diffs.add(format!(
                "{root}({}): {got:?}, expected {want:?}",
                input
                    .iter()
                    .map(|v| format!("{v:e}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    for (root, t) in &tally {
        eprintln!(
            "{root}: {} calls, {} differ, by at most {} ULP",
            t.calls, t.differ, t.max_ulps
        );
        assert!(t.calls > 0, "{root} has no golden values");
    }
    for root in ROOTS {
        assert!(tally.contains_key(*root), "{root} has no golden values");
    }
    assert!(
        diffs.count == 0,
        "{} results differ from the recorded ones; the first {}:\n{}",
        diffs.count,
        diffs.shown.len(),
        diffs.shown.join("\n")
    );
}

/// Every root the file must cover.
const ROOTS: &[&str] = &[
    "epj", "epj2d", "epb", "epb2d", "dcs2c", "dcc2s", "dmxv", "dimxv", "dmxm", "dvxv", "dvdv",
    "dvn", "dmat", "galeq", "eqgal", "galsup", "supgal", "fk45z", "fk54z", "fk5hz", "hfk5z",
    "prec", "prebn", "ecmat", "deuler", "addet", "subet", "mappa", "ampqk", "mapqkz", "drange",
    "dranrm", "dsep", "dbear", "evp", "geoc", "dav2m", "dat", "gmst", "eqeqx", "gd2gc", "gc2gd",
    "pvobs", "rvgalc", "rvlg", "rvlsrd", "rvlsrk", "djcl", "cldj", "dd2tf", "dtf2d",
];

/// The inputs C is undefined on, which the file leaves out: the port returns
/// without panicking and gives the value its documentation states.
#[test]
fn undefined_inputs_give_the_documented_values() {
    use crate::erfa::Tf2dWarning;
    use crate::test_inputs::specials;
    // `dat`: a date `jd2cal` rejects or a NaN reaches `eraDat` as nothing it
    // wrote, and gives 0.0.
    for d in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        BAD_VALUE,
        1e300,
        -1e300,
    ] {
        assert_eq!(dat(d).to_bits(), 0.0_f64.to_bits(), "{d:e}");
        let _ = djcl(d);
    }
    // `dd2tf` of a NaN, infinite or huge fraction, or with more decimal
    // places than an `int` holds: the sign follows the days.
    for ndp in -2..=10 {
        for d in specials() {
            if !(-9..=9).contains(&ndp) || !d.is_finite() || d.abs() >= 1e7 {
                let (s, _) = dd2tf(ndp, d);
                assert_eq!(s, if d >= 0.0 { '+' } else { '-' }, "{ndp} {d:e}");
            }
        }
    }
    // `dtf2d` of an hour or minute of `i32::MIN` warns for that field.
    for s in [0.0, 1.0, f64::NAN] {
        assert_eq!(dtf2d(i32::MIN, 0, s).1, Some(Tf2dWarning::Hour));
        assert_eq!(dtf2d(0, i32::MIN, s).1, Some(Tf2dWarning::Minute));
    }
}
