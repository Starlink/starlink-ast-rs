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

use ast_core::mappings::{
    LutInterp, LutMap, Mapping, MathMap, PermMap, PolyMap, PolyTerm, SphMap, SplineMap, WcsMap,
    WcsProjection,
};

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
