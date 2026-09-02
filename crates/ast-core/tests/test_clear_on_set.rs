//! The clear-on-set convention: a setter for an attribute C covers discards
//! both simplified records.
//!
//! `mapping.h:449-460` states the rule and the reason. A record says that this
//! Mapping has already been through `astSimplify` in the orientation it is now
//! in, and `Mapping::simplify` returns such a Mapping unchanged. Nothing else
//! reads the records — a Mapping is offered to its neighbours' `MapMerge`
//! whatever it holds — and a record can only ever suppress a simplification,
//! never make one wrong, so C errs towards discarding.
//!
//! No fixture can express this. The simplify corpus dumps, simplifies and dumps
//! again; there is no "set an attribute after simplifying" step in the format,
//! and adding one would change every consumer. So the gate is a case per setter
//! here, with the C behaviour confirmed by the probe recorded in
//! `docs/issues/c-oracle-probe-recipe.md` ("Worked probe — the clear-on-set
//! convention").
//!
//! Each case records the flag on the Mapping, asserts it is there, calls the
//! setter and asserts it is gone. Recording it directly rather than through
//! `simplify()` keeps the case about the setter: several of these classes
//! simplify to a different class, which would leave nothing of the right type
//! to call the setter on. `record_survives_invert_and_back` is the control that
//! makes the assertions falsifiable — it pins an operation C does *not* cover,
//! so the flag has to be able to survive.

use ast_core::error::ErrorCode;
use ast_core::mappings::{
    LutInterp, LutMap, Mapping, MathMap, PcdMap, PermMap, PolyMap, PolyTerm, ShiftMap, SphMap,
    SplineMap, WcsMap, WcsProjection, ZoomMap,
};
use ast_core::{AnyObject, from_native_string, to_native_string};

/// Record the flag on `m`, run `mutate`, and require the flag to be gone.
fn discards<M: Mapping>(mut m: M, what: &str, mutate: impl FnOnce(&mut M)) {
    m.set_has_been_simplified(true);
    assert!(
        m.has_been_simplified(),
        "{what}: the class does not record the simplified flag at all"
    );
    mutate(&mut m);
    assert!(
        !m.has_been_simplified(),
        "{what}: the simplified record survived the set"
    );
}

fn lut_map() -> LutMap {
    LutMap::new(&[0.0, 1.0, 4.0, 9.0], 0.0, 1.0).unwrap()
}

fn math_map() -> MathMap {
    MathMap::new(1, 1, &["y=2*x"], &["x=0.5*y"]).unwrap()
}

fn poly_map() -> PolyMap {
    PolyMap::new(
        1,
        1,
        &[PolyTerm {
            coeff: 2.0,
            powers: vec![1],
            out_axis: 1,
        }],
        None,
    )
    .unwrap()
}

/// The order-1 constant spline from `test_splinemap.rs`: the shape does not
/// matter here, only that the attributes exist.
fn spline_map() -> SplineMap {
    SplineMap::new(
        1,
        1,
        1,
        1,
        vec![0.0, 1.0],
        vec![0.0, 1.0],
        vec![1.0],
        vec![2.0],
    )
}

fn perm_map() -> PermMap {
    PermMap::new(&[1, 2], &[1, 2], &[]).unwrap()
}

fn wcs_map() -> WcsMap {
    WcsMap::new(2, WcsProjection::Tan, 1, 2).unwrap()
}

#[test]
fn lut_interp_discards_the_record() {
    discards(lut_map(), "LutMap::set_interp", |m| {
        m.set_interp(LutInterp::Nearest)
    });
}

#[test]
fn math_simp_fi_discards_the_record() {
    discards(math_map(), "MathMap::set_simp_fi", |m| m.set_simp_fi(true));
}

#[test]
fn math_simp_if_discards_the_record() {
    discards(math_map(), "MathMap::set_simp_if", |m| m.set_simp_if(true));
}

#[test]
fn math_seed_discards_the_record() {
    discards(math_map(), "MathMap::set_seed", |m| m.set_seed(1234));
}

#[test]
fn clearing_math_seed_discards_the_record() {
    discards(math_map(), "MathMap::clear_seed", |m| m.clear_seed());
}

#[test]
fn poly_iter_inverse_discards_the_record() {
    discards(poly_map(), "PolyMap::set_iter_inverse", |m| {
        m.set_iter_inverse(true)
    });
}

#[test]
fn clearing_poly_iter_inverse_discards_the_record() {
    discards(poly_map(), "PolyMap::clear_iter_inverse", |m| {
        m.clear_iter_inverse()
    });
}

#[test]
fn poly_niter_inverse_discards_the_record() {
    discards(poly_map(), "PolyMap::set_niter_inverse", |m| {
        m.set_niter_inverse(20)
    });
}

#[test]
fn poly_tol_inverse_discards_the_record() {
    discards(poly_map(), "PolyMap::set_tol_inverse", |m| {
        m.set_tol_inverse(1.0e-8)
    });
}

#[test]
fn spline_inv_niter_discards_the_record() {
    discards(spline_map(), "SplineMap::set_inv_niter", |m| {
        m.set_inv_niter(20)
    });
}

#[test]
fn spline_inv_tol_discards_the_record() {
    discards(spline_map(), "SplineMap::set_inv_tol", |m| {
        m.set_inv_tol(1.0e-8)
    });
}

#[test]
fn spline_out_unit_discards_the_record() {
    discards(spline_map(), "SplineMap::set_out_unit", |m| {
        m.set_out_unit(true)
    });
}

#[test]
fn sph_unit_radius_discards_the_record() {
    discards(SphMap::new(), "SphMap::set_unit_radius", |m| {
        m.set_unit_radius(true)
    });
}

#[test]
fn sph_polar_long_discards_the_record() {
    discards(SphMap::new(), "SphMap::set_polar_long", |m| {
        m.set_polar_long(1.0)
    });
}

#[test]
fn wcs_lon_check_discards_the_record() {
    discards(wcs_map(), "WcsMap::set_lon_check", |m| {
        m.set_lon_check(false)
    });
}

#[test]
fn wcs_fits_proj_discards_the_record() {
    discards(wcs_map(), "WcsMap::set_fits_proj", |m| {
        m.set_fits_proj(false)
    });
}

#[test]
fn perm_split_discards_the_record() {
    discards(perm_map(), "PermMap::set_perm_split", |m| {
        m.set_perm_split(true)
    });
}

#[test]
fn clearing_perm_split_discards_the_record() {
    discards(perm_map(), "PermMap::clear_perm_split", |m| {
        m.clear_perm_split()
    });
}

#[test]
fn zoom_discards_the_record() {
    discards(ZoomMap::new(2, 3.0).unwrap(), "ZoomMap::set_zoom", |m| {
        m.set_zoom(4.0).unwrap();
    });
}

/// C leaves the Zoom value unchanged when a set of zero is rejected
/// (`zoommap.c:1706-1710` evaluates to `this->zoom` after the `astError`), but
/// `astClearIsSimple` is the first term of the same comma expression, so the
/// records go whether the assignment happens or not.
#[test]
fn rejected_zoom_still_discards_the_record() {
    let mut m = ZoomMap::new(2, 3.0).unwrap();
    m.set_has_been_simplified(true);
    let err = m
        .set_zoom(0.0)
        .expect_err("a zoom factor of zero must be rejected");
    assert_eq!(err.code(), ErrorCode::Zoomi);
    assert_eq!(m.zoom(), 3.0, "the rejected set changed the zoom factor");
    assert!(
        !m.has_been_simplified(),
        "the rejected set kept the simplified record"
    );
}

#[test]
fn lut_epsilon_discards_the_record() {
    discards(lut_map(), "LutMap::set_lut_epsilon", |m| {
        m.set_lut_epsilon(1e-7);
    });
}

#[test]
fn pcd_disco_discards_the_record() {
    discards(PcdMap::new(1e-6, [0.0, 0.0]), "PcdMap::set_disc", |m| {
        m.set_disc(2e-6)
    });
}

/// The control. `Invert` is not one of the attributes C covers: inverting a
/// Mapping changes which record applies, and inverting it back restores the
/// original one, so the Mapping reports exactly what it reported before
/// (`mapping.h:424-437`). Without this case every assertion above would pass on
/// a port that simply never recorded anything.
#[test]
fn record_survives_invert_and_back() {
    let mut m = SphMap::new();
    m.set_has_been_simplified(true);
    m.invert();
    assert!(
        !m.has_been_simplified(),
        "the inverted orientation carries its own record, which is unset"
    );
    m.invert();
    assert!(
        m.has_been_simplified(),
        "inverting and inverting back lost the record"
    );
}

/// Record the flag on `m`, dump it to native, read it back, and require the
/// flag to be gone.
///
/// C's loader re-applies a validated attribute through the private setter
/// (`zoommap.c:2231-2238` states the convention), and for a class following
/// clear-on-set that setter discards the record `astLoadMapping` has just
/// installed from the `IsSimp` card (`mapping.c:24137`). `m` must therefore
/// carry an explicitly set attribute, or there is no card for the loader to
/// route and nothing to discard.
fn load_discards(mut m: impl Mapping, what: &str) {
    m.set_has_been_simplified(true);
    let obj = AnyObject::from_mapping(&m).expect("class is not in AnyObject");
    let text = to_native_string(&obj).expect("dump failed");
    assert!(
        text.contains("IsSimp"),
        "{what}: the dump carries no IsSimp card, so the case proves nothing"
    );
    let back = from_native_string(&text).expect("load failed");
    let loaded = back.as_mapping().expect("loaded object is not a Mapping");
    assert!(
        !loaded.has_been_simplified(),
        "{what}: the simplified record survived the load"
    );
}

/// The control: a class with no clear-on-set attribute routes nothing through a
/// setter, so its loader must honour the card.
#[test]
fn load_keeps_the_record_for_a_class_that_routes_nothing() {
    let mut m = ShiftMap::new([1.0, 2.0]);
    m.set_has_been_simplified(true);
    let text = to_native_string(&AnyObject::from_mapping(&m).unwrap()).unwrap();
    let back = from_native_string(&text).unwrap();
    assert!(
        back.as_mapping().unwrap().has_been_simplified(),
        "ShiftMap: the card was discarded by a class that clears nothing"
    );
}

#[test]
fn load_discards_the_record_for_every_clear_on_set_class() {
    let mut zoom = ZoomMap::new(2, 3.0).unwrap();
    zoom.set_has_been_simplified(true);
    load_discards(zoom, "ZoomMap::Zoom");

    let mut math = math_map();
    math.set_simp_fi(true);
    math.set_simp_if(true);
    load_discards(math, "MathMap::SimpFI");

    let mut lut = lut_map();
    lut.set_interp(LutInterp::Nearest);
    load_discards(lut, "LutMap::LutInterp");

    let mut poly = poly_map();
    poly.set_iter_inverse(true);
    poly.set_niter_inverse(7);
    poly.set_tol_inverse(1e-8);
    load_discards(poly, "PolyMap::IterInverse");

    let mut spline = spline_map();
    spline.set_inv_niter(9);
    spline.set_inv_tol(1e-7);
    spline.set_out_unit(true);
    load_discards(spline, "SplineMap::InvNiter");

    let mut sph = SphMap::new();
    sph.set_unit_radius(true);
    sph.set_polar_long(0.5);
    load_discards(sph, "SphMap::UnitRadius");

    let mut wcs = wcs_map();
    wcs.set_fits_proj(true);
    wcs.set_lon_check(true);
    load_discards(wcs, "WcsMap::FITSProj");

    let mut perm = perm_map();
    perm.set_perm_split(true);
    load_discards(perm, "PermMap::PermSplit");
}
