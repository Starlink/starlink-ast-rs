// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The vendored wcslib functions the Rust port is checked against, bit for
//! bit.

unsafe extern "C" {
    pub(super) fn astCosd(angle: f64) -> f64;
    pub(super) fn astSind(angle: f64) -> f64;
    pub(super) fn astTand(angle: f64) -> f64;
    pub(super) fn astACosd(v: f64) -> f64;
    pub(super) fn astASind(v: f64) -> f64;
    pub(super) fn astATand(v: f64) -> f64;
    pub(super) fn astATan2d(y: f64, x: f64) -> f64;
}

use super::WcsProjection;
use super::ffi::PrjParams;
use super::prj::{Prj, Status};
use crate::test_inputs::{SplitMix64, specials};

// Rust (via LLVM) does not specify the sign or payload of a NaN produced by
// arithmetic, and here folds `(-r) * c` to `-(r * c)`, so a NaN's sign can
// differ from C's while every non-NaN result matches bit for bit.
// AST tests only `is_nan`, never a NaN's sign.
fn same_bits_or_both_nan(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
}

/// Random points per direction and per parameter set.
const RANDOM_POINTS: usize = 20_000;

/// `v` and the values one ULP below and above it (those of zero are the
/// smallest subnormals).
fn with_ulps(v: f64) -> [f64; 3] {
    let up = if v >= 0.0 {
        f64::from_bits(v.to_bits() + 1)
    } else {
        f64::from_bits(v.to_bits() - 1)
    };
    let down = if v > 0.0 {
        f64::from_bits(v.to_bits() - 1)
    } else if v == 0.0 {
        -f64::from_bits(1)
    } else {
        f64::from_bits(v.to_bits() + 1)
    };
    [down, v, up]
}

/// The (phi, theta) points the forward projection is checked at, in degrees.
fn forward_points() -> Vec<(f64, f64)> {
    let mut rng = SplitMix64::new(0x5eed_f0d0);
    let mut out = Vec::new();
    for _ in 0..RANDOM_POINTS {
        out.push((
            (rng.next_f64() * 2.0 - 1.0) * 180.0,
            (rng.next_f64() * 2.0 - 1.0) * 90.0,
        ));
    }
    // Close to either pole, where the small angle formulae apply.
    for i in 0..RANDOM_POINTS / 4 {
        let off = crate::math::powf(10.0, -(rng.next_f64() * 9.0 + 1.0));
        let theta = if i % 2 == 0 { 90.0 - off } else { -90.0 + off };
        out.push(((rng.next_f64() * 2.0 - 1.0) * 180.0, theta));
    }
    // Every multiple of 45 degrees in both coordinates.
    for i in -8..=8 {
        for j in -4..=4 {
            out.push((45.0 * f64::from(i), 45.0 * f64::from(j)));
        }
    }
    // The face centres, face edges and facet apexes of the quad-cube and
    // HEALPix projections, and the latitude where HEALPix's equatorial zone
    // ends, each one ULP either side.
    let mut angles = Vec::new();
    for i in -24..=24 {
        angles.extend(with_ulps(7.5 * f64::from(i)));
    }
    angles.extend(with_ulps(
        crate::math::asin(2.0 / 3.0) * 180.0 / std::f64::consts::PI,
    ));
    angles.extend(with_ulps(
        -crate::math::asin(2.0 / 3.0) * 180.0 / std::f64::consts::PI,
    ));
    for &p in &angles {
        for &t in &angles {
            if p.abs() <= 360.0 && t.abs() <= 90.0 + 1e-9 {
                out.push((p, t));
            }
        }
    }
    // The specials in both coordinates.
    let sp = specials();
    for &p in &sp {
        for &t in &sp {
            out.push((p, t));
        }
    }
    out
}

/// The (x, y) points the reverse projection is checked at, apart from the
/// forward outputs.
fn reverse_points() -> Vec<(f64, f64)> {
    let mut rng = SplitMix64::new(0x5eed_5e5e);
    let mut out = Vec::new();
    for i in 0..RANDOM_POINTS {
        // Several magnitudes, so the small angle branches are reached.
        let scale = [400.0, 4.0, 1e-2, 1e-5][i % 4];
        out.push((
            (rng.next_f64() * 2.0 - 1.0) * scale,
            (rng.next_f64() * 2.0 - 1.0) * scale,
        ));
    }
    for i in -4..=4 {
        for j in -4..=4 {
            out.push((100.0 * f64::from(i), 100.0 * f64::from(j)));
        }
    }
    // The edges and centres of the faces and facets, each one ULP either
    // side.
    let mut coords = Vec::new();
    for i in -32..=32 {
        coords.extend(with_ulps(11.25 * f64::from(i)));
    }
    for &x in &coords {
        for &y in &coords {
            out.push((x, y));
        }
    }
    let sp = specials();
    for &x in &sp {
        for &y in &sp {
            out.push((x, y));
        }
    }
    out
}

/// Reverse points at the edges and apexes of the facets of a projection
/// set up with the given parameters, each one ULP either side.
///
/// The lengths are the scale-free units the facets are built from: HEALPix's
/// facet width 180/H, its polar-zone limit (K+1)/2 widths, the end of its
/// equatorial zone, and XPH's facet width 45*sqrt(2); multiples of each and
/// of half of each are taken in both coordinates.
fn facet_points(prj: &Prj) -> Vec<(f64, f64)> {
    let h = prj.get_param(0, 1).unwrap_or(0.0);
    let k = prj.get_param(0, 2).unwrap_or(0.0);
    let mut units = vec![45.0 * crate::math::sqrt(2.0)];
    if h != 0.0 && h.is_finite() {
        units.push(180.0 / h);
        if k.is_finite() {
            units.push((k + 1.0) / 2.0 * (180.0 / h));
            units.push(90.0 * (k - 1.0) / h);
            units.push(90.0 * k / h);
        }
    }
    let mut out = Vec::new();
    for unit in units {
        for half in [1.0, 0.5] {
            let mut coords = Vec::new();
            for i in -8..=8 {
                coords.extend(with_ulps(unit * half * f64::from(i)));
            }
            for &x in &coords {
                for &y in &coords {
                    out.push((x, y));
                }
            }
        }
    }
    out
}

/// The parameter arrays of a `WcsMap` on the celestial axes 1 (longitude)
/// and 2 (latitude) with the given `(axis, m, value)` parameters set.
fn params_of(pv: &[(usize, usize, f64)]) -> Vec<Vec<Option<f64>>> {
    let mut params = vec![Vec::new(), Vec::new()];
    for &(axis, m, value) in pv {
        let p: &mut Vec<Option<f64>> = &mut params[if axis == 2 { 0 } else { 1 }];
        if m >= p.len() {
            p.resize(m + 1, None);
        }
        p[m] = Some(value);
    }
    params
}

/// Check that the native projection `proj` gives the status and the bits of
/// the vendored C, for each parameter set in `pv_sets` (a list of
/// `(axis, m, value)` with axis 1 the longitude and 2 the latitude).
///
/// One C struct serves every call for a parameter set, with forward and
/// reverse calls interleaved, as `WcsMap` uses it: C sets itself up on its
/// first call and keeps the state.
/// Outputs are compared only where the status is `Ok`.
/// A parameter set whose set-up fails is checked at the first forward call
/// and at the first reverse call, each on a fresh C struct.
/// C leaves its `flag` set after a failed set-up, so its later calls run the
/// projection on half-initialized constants, while the native `Prj` keeps
/// returning the set-up status; those calls are not compared.
pub(crate) fn check_projection(proj: WcsProjection, pv_sets: &[Vec<(usize, usize, f64)>]) {
    check_projection_tpn(proj, pv_sets, false);
}

/// [`check_projection`] with TPN's `TPNTan` switch given: whether the
/// transformation includes the TAN projection as well as the polynomial.
pub(crate) fn check_projection_tpn(
    proj: WcsProjection,
    pv_sets: &[Vec<(usize, usize, f64)>],
    tpn_tan: bool,
) {
    let fwd_points = forward_points();
    let rev_points = reverse_points();
    let mut compared = (0usize, 0usize);
    let mut facet_total = 0usize;
    for (set_index, pv) in pv_sets.iter().enumerate() {
        let params = params_of(pv);
        let label = |dir: &str, a: f64, b: f64| {
            format!("{} set {set_index} {pv:?} {dir}({a:e}, {b:e})", proj.code())
        };
        let prj = Prj::new(proj, &params, tpn_tan).unwrap();
        let new_c = || {
            let mut c = PrjParams::new(proj).unwrap();
            c.init_prj_prm(&params, tpn_tan);
            c
        };

        // The parameters, including the defaults, are the same.
        let c = new_c();
        for axis in 0..2 {
            for m in 0..=100 {
                assert_eq!(
                    prj.get_param(axis, m).map(f64::to_bits),
                    c.get_param(axis, m).map(f64::to_bits),
                    "{} set {set_index} {pv:?} parameter {axis}/{m}",
                    proj.code(),
                );
            }
        }

        if prj.setup_status() != Status::Ok {
            let (cf, ..) = new_c().forward_raw(0.0, 0.0);
            let (cr, ..) = new_c().reverse_raw(0.0, 0.0);
            assert_eq!(
                (cf, cr),
                (prj.setup_status() as i32, prj.setup_status() as i32),
                "{} set {set_index} {pv:?}: set-up status",
                proj.code()
            );
            let (status, ..) = prj.fwd_raw(0.0, 0.0);
            assert_eq!(status, prj.setup_status());
            let (status, ..) = prj.rev_raw(0.0, 0.0);
            assert_eq!(status, prj.setup_status());
            continue;
        }

        let mut c = new_c();
        // The reverse points: the generic ones, the facet edges and apexes
        // of this set, then its forward outputs (the round trips).
        let mut revs = rev_points.clone();
        let before = revs.len();
        revs.extend(facet_points(&prj));
        facet_total += revs.len() - before;
        for &(phi, theta) in &fwd_points {
            if let (Status::Ok, x, y) = prj.fwd_raw(phi, theta) {
                revs.push((x, y));
            }
        }
        for i in 0..fwd_points.len().max(revs.len()) {
            if i < fwd_points.len() {
                let (phi, theta) = fwd_points[i];
                let (rs, rx, ry) = prj.fwd_raw(phi, theta);
                let (cs, cx, cy) = c.forward_raw(phi, theta);
                assert_eq!(rs as i32, cs, "{}", label("fwd status", phi, theta));
                if rs == Status::Ok {
                    assert!(
                        same_bits_or_both_nan(rx, cx) && same_bits_or_both_nan(ry, cy),
                        "{}: {rx:e} {ry:e} vs {cx:e} {cy:e}",
                        label("fwd", phi, theta)
                    );
                    compared.0 += 1;
                }
            }
            if i < revs.len() {
                let (x, y) = revs[i];
                let (rs, rp, rt) = prj.rev_raw(x, y);
                let (cs, cp, ct) = c.reverse_raw(x, y);
                assert_eq!(rs as i32, cs, "{}", label("rev status", x, y));
                if rs == Status::Ok {
                    assert!(
                        same_bits_or_both_nan(rp, cp) && same_bits_or_both_nan(rt, ct),
                        "{}: {rp:e} {rt:e} vs {cp:e} {ct:e}",
                        label("rev", x, y)
                    );
                    compared.1 += 1;
                }
            }
        }
    }
    eprintln!(
        "{}: {} parameter sets, {} forward and {} reverse outputs compared, {} facet points",
        proj.code(),
        pv_sets.len(),
        compared.0,
        compared.1,
        facet_total
    );
}
