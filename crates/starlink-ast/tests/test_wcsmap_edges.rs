// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! WcsMap projections at face centres, poles, longitude edges, non-default
//! projection parameters and native coordinates a little past a limit:
//! the points where the wcslib code AST carries has differed from upstream.
//!
//! C ancestor: `reference/starlink-ast/ast_tester/testwcsmap.c`. Every case
//! is that file's, with its wanted values, which are upstream wcslib 8.10's
//! output for the same projection and parameters unless a comment says
//! otherwise. As there, cases are written in degrees and a WcsMap works in
//! radians on both sides.
//!
//! Divergence from the C: a failing case panics with its label, where the C
//! records the failure and carries on to the next case.

// The wanted values are written as the C test writes them, with 17
// significant digits, so they can be checked against it digit for digit.
#![allow(clippy::excessive_precision)]

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;
use starlink_ast::mappings::{Mapping, WcsMap, WcsProjection};

/// A WcsMap of the projection with longitude axis 1 and latitude axis 2, and
/// the given `PV2_m` parameters.
fn wcsmap(proj: WcsProjection, pv: &[(usize, f64)]) -> WcsMap {
    let mut map = WcsMap::new(2, proj, 1, 2).unwrap();
    for &(m, value) in pv {
        map.set_pv(2, m, value).unwrap();
    }
    map
}

/// Transform one point, forward when `forward` is true.
fn tran(map: &WcsMap, forward: bool, a: f64, b: f64) -> (f64, f64) {
    let out = map
        .transform(&PointSet::from_rows([[a, b]]), forward)
        .unwrap();
    (out.point(0)[0], out.point(0)[1])
}

/// Compare one output (radians, or `BAD_VALUE`) with a wanted value
/// (degrees, or `BAD_VALUE` to require a bad output). A zero tolerance asks
/// for the converted value to equal the wanted one.
fn check_value(label: &str, which: &str, got: f64, want: f64, tol: f64) {
    if want == BAD_VALUE {
        assert!(
            got == BAD_VALUE,
            "{label}: {which} is {:.17e}, wanted bad",
            got.to_degrees()
        );
        return;
    }
    assert!(
        got != BAD_VALUE,
        "{label}: {which} is bad, wanted {want:.17e}"
    );
    let deg = got.to_degrees();
    let bad = if tol == 0.0 {
        deg != want
    } else {
        (deg - want).abs() > tol
    };
    assert!(
        !bad,
        "{label}: {which} is {deg:.17e}, wanted {want:.17e} (tolerance {tol:e})"
    );
}

/// Native spherical (phi, theta) to projection plane (x, y), with separate
/// tolerances on x and y.
#[allow(clippy::too_many_arguments)]
fn check_fwd_xy(
    label: &str,
    proj: WcsProjection,
    pv: &[(usize, f64)],
    phi: f64,
    theta: f64,
    x_want: f64,
    x_tol: f64,
    y_want: f64,
    y_tol: f64,
) {
    let map = wcsmap(proj, pv);
    let (x, y) = tran(&map, true, phi.to_radians(), theta.to_radians());
    check_value(label, "x", x, x_want, x_tol);
    check_value(label, "y", y, y_want, y_tol);
}

/// Native spherical (phi, theta) to projection plane (x, y).
#[allow(clippy::too_many_arguments)]
fn check_fwd(
    label: &str,
    proj: WcsProjection,
    pv: &[(usize, f64)],
    phi: f64,
    theta: f64,
    x_want: f64,
    y_want: f64,
    tol: f64,
) {
    check_fwd_xy(label, proj, pv, phi, theta, x_want, tol, y_want, tol);
}

/// Projection plane (x, y) to native spherical (phi, theta).
#[allow(clippy::too_many_arguments)]
fn check_rev(
    label: &str,
    proj: WcsProjection,
    pv: &[(usize, f64)],
    x: f64,
    y: f64,
    phi_want: f64,
    theta_want: f64,
    tol: f64,
) {
    let map = wcsmap(proj, pv);
    let (phi, theta) = tran(&map, false, x.to_radians(), y.to_radians());
    check_value(label, "phi", phi, phi_want, tol);
    check_value(label, "theta", theta, theta_want, tol);
}

/// Forward then reverse, passing the forward's radians straight back, and
/// check the reverse's (phi, theta).
#[allow(clippy::too_many_arguments)]
fn check_round_trip(
    label: &str,
    proj: WcsProjection,
    pv: &[(usize, f64)],
    phi: f64,
    theta: f64,
    phi_want: f64,
    theta_want: f64,
    tol: f64,
) {
    let map = wcsmap(proj, pv);
    let (x, y) = tran(&map, true, phi.to_radians(), theta.to_radians());
    assert!(
        x != BAD_VALUE && y != BAD_VALUE,
        "{label}: the forward projection was bad"
    );
    let (phi_out, theta_out) = tran(&map, false, x, y);
    check_value(label, "phi", phi_out, phi_want, tol);
    check_value(label, "theta", theta_out, theta_want, tol);
}

/// Forward then reverse at a pole. The longitude of a pole is arbitrary, so
/// only theta is compared; phi must not be bad.
fn check_pole_round_trip(
    label: &str,
    proj: WcsProjection,
    pv: &[(usize, f64)],
    phi: f64,
    theta: f64,
    tol: f64,
) {
    let map = wcsmap(proj, pv);
    let (x, y) = tran(&map, true, phi.to_radians(), theta.to_radians());
    assert!(
        x != BAD_VALUE && y != BAD_VALUE,
        "{label}: the forward projection was bad"
    );
    let (phi_out, theta_out) = tran(&map, false, x, y);
    assert!(phi_out != BAD_VALUE, "{label}: phi at the pole is bad");
    check_value(label, "theta", theta_out, theta, tol);
}

const BAD: f64 = BAD_VALUE;

/// QSC, face 4. Within about 0.008 degrees of the face centre the forward
/// projection takes a small-angle branch, which squared the longitude offset
/// where it should have taken it once.
#[test]
fn qsc_near_face_4_centre() {
    use WcsProjection::Qsc;
    check_fwd(
        "qsc face 4 centre",
        Qsc,
        &[],
        -90.005,
        0.0,
        269.99486913923511,
        0.0,
        1.0e-10,
    );
    check_rev(
        "qsc face 4 centre reverse",
        Qsc,
        &[],
        269.99486913923511,
        0.0,
        -90.005,
        0.0,
        1.0e-9,
    );
    check_fwd(
        "qsc face 4 near",
        Qsc,
        &[],
        -89.995,
        0.004,
        270.00573448754045,
        0.0047579971900078627,
        1.0e-10,
    );
    check_rev(
        "qsc face 4 near reverse",
        Qsc,
        &[],
        270.00573448754045,
        0.0047579971900078627,
        -89.995,
        0.004,
        1.0e-9,
    );
}

/// HPX polar zone with an odd H, the polar zone's extent for H other than
/// 4, and a point on a facet edge.
#[test]
fn hpx_polar_zone_and_facet_edges() {
    use WcsProjection::Hpx;
    let h3k3 = [(1, 3.0), (2, 3.0)];
    let h4k4 = [(1, 4.0), (2, 4.0)];
    // The facet centre was computed with floor(... + 1/2), an integer
    // division that added nothing.
    check_fwd(
        "hpx H=3 K=3 south pole",
        Hpx,
        &h3k3,
        -180.0,
        -90.0,
        -120.0,
        -120.0,
        1.0e-10,
    );
    check_fwd(
        "hpx H=4 K=4 south pole",
        Hpx,
        &h4k4,
        -131.9,
        -90.0,
        -90.0,
        -112.5,
        1.0e-10,
    );
    // The polar zone extends to (K+1)/2 facets of 180/H degrees, not to a
    // fixed 90 degrees, which is only right for H = 4.
    check_rev(
        "hpx H=3 K=3 polar zone",
        Hpx,
        &h3k3,
        -0.4,
        -119.5,
        -48.000000000000171,
        -89.610150847356522,
        1.0e-9,
    );
    // A point on a facet edge, where (x - x_c)/sigma reaches 180/H only up
    // to rounding.
    check_fwd(
        "hpx facet edge",
        Hpx,
        &[],
        90.0,
        -89.26,
        134.28818979161747,
        -89.288189791617469,
        1.0e-10,
    );
    check_rev(
        "hpx facet edge reverse",
        Hpx,
        &[],
        134.28818979161747,
        -89.288189791617469,
        90.0,
        -89.26,
        1.0e-9,
    );
}

/// HPX at the pole: the forward projection puts it at the apex of the facet,
/// and the reverse must accept it rather than reject sigma = 0.
#[test]
fn hpx_pole_at_facet_apex() {
    use WcsProjection::Hpx;
    for (phi, x) in [(-180.0, -135.0), (-45.0, -45.0), (0.0, 45.0), (90.0, 135.0)] {
        let label = format!("hpx pole phi={phi}");
        check_fwd(&label, Hpx, &[], phi, -90.0, x, -90.0, 1.0e-10);
        check_rev(
            &format!("{label} reverse"),
            Hpx,
            &[],
            x,
            -90.0,
            x,
            -90.0,
            1.0e-10,
        );
    }
}

/// HPX pole through a WcsMap, which converts x and y to radians and back:
/// for these H and K that leaves y a few ULP from the pole's, so sigma is
/// about 1e-16 rather than 0, or y is just beyond the polar zone.
#[test]
fn hpx_pole_round_trip_non_default_h_k() {
    use WcsProjection::Hpx;
    // (label, PV2_m parameters, phi, theta).
    type PoleCase = (&'static str, [(usize, f64); 2], f64, f64);
    let cases: [PoleCase; 4] = [
        ("hpx H=3 K=3 north pole", [(1, 3.0), (2, 3.0)], 60.0, 90.0),
        (
            "hpx H=3 K=1 south pole",
            [(1, 3.0), (2, 1.0)],
            -120.0,
            -90.0,
        ),
        ("hpx H=3 K=6 north pole", [(1, 3.0), (2, 6.0)], 0.0, 90.0),
        ("hpx H=6 K=1 north pole", [(1, 6.0), (2, 1.0)], 30.0, 90.0),
    ];
    for (label, pv, phi, theta) in cases {
        check_pole_round_trip(label, Hpx, &pv, phi, theta, 1.0e-10);
    }
}

/// HPX polar zone at native longitude +180: the facet arithmetic puts it
/// beyond x = 180, and wcslib folds it back to 360 - x so that it lands on
/// the facet it belongs to.
#[test]
fn hpx_phi_plus_180() {
    use WcsProjection::Hpx;
    check_fwd(
        "hpx phi=+180 north pole",
        Hpx,
        &[],
        180.0,
        90.0,
        135.0,
        90.0,
        1.0e-10,
    );
    check_fwd(
        "hpx phi=+180 north",
        Hpx,
        &[],
        180.0,
        60.0,
        163.52885682970026,
        61.471143170299726,
        1.0e-10,
    );
    check_fwd(
        "hpx phi=+180 south",
        Hpx,
        &[],
        180.0,
        -60.0,
        163.52885682970026,
        -61.471143170299726,
        1.0e-10,
    );
    check_fwd(
        "hpx phi=+180 south pole",
        Hpx,
        &[],
        180.0,
        -90.0,
        135.0,
        -90.0,
        1.0e-10,
    );
    check_fwd(
        "hpx H=3 K=3 phi=+180",
        Hpx,
        &[(1, 3.0), (2, 3.0)],
        180.0,
        60.0,
        158.03847577293368,
        81.961524227066306,
        1.0e-10,
    );
    check_fwd(
        "hpx H=4 K=4 phi=+180",
        Hpx,
        &[(1, 4.0), (2, 4.0)],
        180.0,
        -60.0,
        180.0,
        -79.55771365940052,
        1.0e-10,
    );
}

/// ZPN with a degree-2 polynomial: beyond the turning point of r(theta) the
/// projection folds back on itself, and the point is on the far side.
/// wcslib reports it as bad at every degree.
#[test]
fn zpn_degree_2_far_side() {
    check_fwd(
        "zpn degree 2 far side",
        WcsProjection::Zpn,
        &[(1, 1.27118), (2, -0.210643)],
        5.4085552,
        -89.2063216,
        BAD,
        BAD,
        0.0,
    );
}

/// PCO within 1e-4 degrees of the equator: cot(theta) is huge and
/// 1 - cos(phi*sin(theta)) cancels, so y must come from a series instead.
#[test]
fn pco_near_equator() {
    use WcsProjection::Pco;
    check_fwd(
        "pco near equator",
        Pco,
        &[],
        90.0,
        1.0e-9,
        90.0,
        2.2337005501361701e-09,
        1.0e-15,
    );
    check_fwd(
        "pco near equator, far from the meridian",
        Pco,
        &[],
        -170.0,
        -5.0e-5,
        -169.99999999993526,
        -0.0002700860857957293,
        1.0e-12,
    );
}

/// COP at the poles. The pole on the same side of the equator as the
/// standard parallels is the apex of the cone, r = 0, and is returned
/// exactly; the other pole is on the far side of the projection and is bad.
#[test]
fn cop_at_the_poles() {
    use WcsProjection::Cop;
    let north = [(1, 60.0), (2, 0.0)];
    let south = [(1, -45.0), (2, 5.0)];
    check_fwd(
        "cop opposite pole, north parallels",
        Cop,
        &north,
        -180.0,
        -90.0,
        BAD,
        BAD,
        0.0,
    );
    check_fwd(
        "cop opposite pole, south parallels",
        Cop,
        &south,
        0.0,
        90.0,
        BAD,
        BAD,
        0.0,
    );
    // At the apex r is exactly zero, so x is exactly zero on any platform;
    // y = Y0 depends on libm and is given a margin of a few hundred ULP.
    check_fwd_xy(
        "cop apex, north parallels",
        Cop,
        &north,
        37.0,
        90.0,
        0.0,
        0.0,
        33.079733725307534,
        1.0e-12,
    );
    check_rev(
        "cop apex, north parallels, reverse",
        Cop,
        &north,
        0.0,
        33.079733725307534,
        0.0,
        90.0,
        1.0e-12,
    );
    check_fwd_xy(
        "cop apex, south parallels",
        Cop,
        &south,
        30.0,
        -90.0,
        0.0,
        0.0,
        -57.077751773966277,
        1.0e-12,
    );
}

/// MER close to the pole, where y = ln(tan(45 + theta/2)) magnifies any
/// error in theta: wcslib's D2R must be (PI/180.0), so that theta*D2R is
/// theta*(pi/180) rather than (theta*pi)/180. Without the parentheses y is
/// 1.5e-6 larger; the tolerance allows for libm differences between
/// platforms.
#[test]
fn mer_near_pole() {
    check_fwd(
        "mer near pole",
        WcsProjection::Mer,
        &[],
        0.0,
        89.999999,
        0.0,
        1063.2311737965763,
        1.0e-9,
    );
}

/// Native coordinates a little past a limit. wcslib sets a longitude or
/// latitude within a per-projection tolerance of a limit to the limit, and
/// rejects one beyond it; the tolerance is 1e-13 degrees for ARC, 1e-12 for
/// PAR and 1e-11 for MOL. Wanted values are wcslib's prjx2s results with
/// bounds checking on.
#[test]
fn native_coordinates_past_a_limit() {
    use WcsProjection::{Arc, Mol, Par};
    // ARC beyond the antipode: r = 180 + 6e-14 gives a latitude 4 ULP below
    // -90, which is the pole; r = 180 + 2e-13 is beyond the tolerance.
    check_rev(
        "arc 4 ulp past the antipode",
        Arc,
        &[],
        -180.00000000000006,
        0.0,
        -90.0,
        -90.0,
        0.0,
    );
    check_rev(
        "arc 2e-13 past the antipode",
        Arc,
        &[],
        -180.0000000000002,
        0.0,
        BAD,
        BAD,
        0.0,
    );
    // PAR and MOL at phi = -180: the reverse of the forward lands about
    // 9e-14 degrees below -180, three ULP, through rounding alone.
    check_round_trip(
        "par round trip at -180",
        Par,
        &[],
        -180.0,
        -58.68,
        -180.0,
        -58.68,
        1.0e-9,
    );
    check_round_trip(
        "mol round trip at -180",
        Mol,
        &[],
        -180.0,
        -59.76,
        -180.0,
        -59.76,
        1.0e-9,
    );
    check_rev(
        "par 5.5e-12 below -180",
        Par,
        &[],
        -99.29743646562997,
        -60.262885419193012,
        BAD,
        BAD,
        0.0,
    );
}
