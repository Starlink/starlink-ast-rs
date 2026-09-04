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

use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::error::ErrorCode;
use ast_core::mappings::{
    GrismMap, LutInterp, LutMap, Mapping, MathMap, PcdMap, PermMap, PolyMap, PolyTerm, ShiftMap,
    SphMap, SplineMap, WcsMap, WcsProjection, ZoomMap,
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

/// C's GrismMap setters go through a class-local `MAKE_SET`
/// (`grismmap.c:236-262`) rather than `astMAKE_SET1`, but the body opens with
/// the same `astClearIsSimple`, and C applies it to all eight attributes even
/// though its own `MapMerge` reads only three of them (`grismmap.c:577-579`).
/// That is the err-towards-discarding latitude `mapping.h:449-460` grants, so
/// the port covers all eight too.
#[test]
fn grism_nr_discards_the_record() {
    discards(GrismMap::default_map(), "GrismMap::set_grism_nr", |m| {
        m.set_grism_nr(1.5)
    });
}

#[test]
fn grism_nrp_discards_the_record() {
    discards(GrismMap::default_map(), "GrismMap::set_grism_nrp", |m| {
        m.set_grism_nrp(1e-5)
    });
}

#[test]
fn grism_wave_r_discards_the_record() {
    discards(GrismMap::default_map(), "GrismMap::set_grism_wave_r", |m| {
        m.set_grism_wave_r(6000.0)
    });
}

#[test]
fn grism_alpha_discards_the_record() {
    discards(GrismMap::default_map(), "GrismMap::set_grism_alpha", |m| {
        m.set_grism_alpha(0.1)
    });
}

#[test]
fn grism_g_discards_the_record() {
    discards(GrismMap::default_map(), "GrismMap::set_grism_g", |m| {
        m.set_grism_g(1000.0)
    });
}

#[test]
fn grism_m_discards_the_record() {
    discards(GrismMap::default_map(), "GrismMap::set_grism_m", |m| {
        m.set_grism_m(1)
    });
}

#[test]
fn grism_eps_discards_the_record() {
    discards(GrismMap::default_map(), "GrismMap::set_grism_eps", |m| {
        m.set_grism_eps(0.01)
    });
}

#[test]
fn grism_theta_discards_the_record() {
    discards(GrismMap::default_map(), "GrismMap::set_grism_theta", |m| {
        m.set_grism_theta(0.02)
    });
}

/// The setters recompute the derived constants, as C's `MAKE_SET` does by
/// calling `UpdateConstants` after the assignment (`grismmap.c:256-259`).
/// Without that call the transformation would keep using constants derived from
/// the old attribute values, which is not visible through the simplified
/// records, so it gets its own assertion.
#[test]
fn grism_setters_recompute_the_constants() {
    let mut set = GrismMap::default_map();
    set.set_grism_g(1000.0);
    set.set_grism_m(1);
    set.set_grism_wave_r(6000.0);

    let constructed = GrismMap::new(1.0, 0.0, 6000.0, 0.0, 1000.0, 1, 0.0, 0.0);

    let pts = PointSet::from_rows(vec![vec![1.0], vec![2.0], vec![3.0]]);
    let via_setters = set.transform(&pts, true).unwrap();
    let via_constructor = constructed.transform(&pts, true).unwrap();
    for i in 0..pts.npoint() {
        assert_eq!(
            via_setters.point(i),
            via_constructor.point(i),
            "point {i}: the setters left stale derived constants"
        );
    }
}

/// C stores a GrismMap attribute raw and reads it back through a getter that
/// substitutes the default for the `AST__BAD` unset sentinel, so
/// `astSetGrismNR( map, AST__BAD )` restores the default and makes
/// `astTestGrismNR` false (`grismmap.c:1733-1737`). The port stores what the
/// getter would report and carries the sentinel test as the flag the dump reads,
/// so the sentinel has to be recognised in the setter. The dump is where the
/// test half is observable: an untested attribute is written commented out.
#[test]
fn grism_sentinel_restores_the_default_and_untests_the_attribute() {
    let mut m = GrismMap::new(1.5, 0.0, 6000.0, 0.0, 1000.0, 1, 0.0, 0.0);
    assert!(
        to_native_string(&AnyObject::GrismMap(m.clone()))
            .unwrap()
            .contains("GrmNR = 1.5"),
        "a constructed GrismMap should write GrmNR as a set card"
    );

    m.set_grism_nr(BAD_VALUE);
    assert_eq!(m.grism_nr(), 1.0, "the sentinel should restore the default");

    let dump = to_native_string(&AnyObject::GrismMap(m)).unwrap();
    assert!(
        dump.contains("#   GrmNR = 1 "),
        "the sentinel should leave GrmNR untested, so the card is commented out; got:\n{dump}"
    );
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
/// flag to have survived.
///
/// C's loader re-applies a validated attribute through the private setter
/// (`zoommap.c:2231-2238` states the convention), and that setter used to
/// discard the record `astLoadMapping` had just installed from the `IsSimp`
/// card (`mapping.c:24137`), so a dumped card could not be read back. Upstream
/// `07d0cd2d` guards each clear on the value actually differing, and the
/// re-applied value is the one just constructed with, so the record now
/// survives. `m` must carry an explicitly set attribute, or there is no card
/// for the loader to route and the case proves nothing.
fn load_preserves(mut m: impl Mapping, what: &str) {
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
        loaded.has_been_simplified(),
        "{what}: a no-op set inside the loader discarded the simplified record"
    );
}

/// `Seed` carries a separate set flag, so setting an unset `Seed` to the value it
/// already holds is still a change.
///
/// Ancestor: the case added to `testissimp.c` with upstream `1adbc353`. A new
/// MathMap has an unpredictable default seed, so the integer in the field does
/// not move when a caller sets that same integer explicitly -- but the attribute
/// goes from unset to set, which is what makes it a change. C's guard therefore
/// tests `!seed_set` before comparing values (`mathmap.c:5630`), and a guard that
/// compared values alone would keep a stale record here.
#[test]
fn setting_an_unset_seed_to_its_own_default_discards_the_record() {
    let mut m = math_map();
    assert!(
        !m.test_seed(),
        "a new MathMap must not report Seed as explicitly set, or the case \
         proves nothing"
    );
    let default_seed = m.seed();

    m.set_has_been_simplified(true);
    m.set_seed(default_seed);

    assert!(
        m.test_seed(),
        "setting the default Seed did not make it explicit"
    );
    assert!(
        !m.has_been_simplified(),
        "setting an unset Seed to its own default kept the simplified record"
    );
}

/// And setting an explicit `Seed` to the value it already holds is not a change.
///
/// The other half of the guard: once `Seed` is set, a set to the same value is
/// the no-op the loader relies on, which is what `load_preserves` covers for the
/// serialised case.
#[test]
fn re_setting_an_explicit_seed_to_the_same_value_keeps_the_record() {
    let mut m = math_map();
    m.set_seed(4242);
    m.set_has_been_simplified(true);
    m.set_seed(4242);
    assert!(
        m.has_been_simplified(),
        "re-setting Seed to the value it already held discarded the record"
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
fn load_preserves_the_record_for_every_clear_on_set_class() {
    let mut zoom = ZoomMap::new(2, 3.0).unwrap();
    zoom.set_has_been_simplified(true);
    load_preserves(zoom, "ZoomMap::Zoom");

    let mut math = math_map();
    math.set_simp_fi(true);
    math.set_simp_if(true);
    load_preserves(math, "MathMap::SimpFI");

    let mut lut = lut_map();
    lut.set_interp(LutInterp::Nearest);
    load_preserves(lut, "LutMap::LutInterp");

    let mut lut_eps = lut_map();
    lut_eps.set_lut_epsilon(1e-10);
    load_preserves(lut_eps, "LutMap::LutEpsilon");

    let mut poly = poly_map();
    poly.set_iter_inverse(true);
    poly.set_niter_inverse(7);
    poly.set_tol_inverse(1e-8);
    load_preserves(poly, "PolyMap::IterInverse");

    let mut spline = spline_map();
    spline.set_inv_niter(9);
    spline.set_inv_tol(1e-7);
    spline.set_out_unit(true);
    load_preserves(spline, "SplineMap::InvNiter");

    let mut sph = SphMap::new();
    sph.set_unit_radius(true);
    sph.set_polar_long(0.5);
    load_preserves(sph, "SphMap::UnitRadius");

    let mut wcs = wcs_map();
    wcs.set_fits_proj(true);
    wcs.set_lon_check(true);
    load_preserves(wcs, "WcsMap::FITSProj");

    let mut perm = perm_map();
    perm.set_perm_split(true);
    load_preserves(perm, "PermMap::PermSplit");

    let mut pcd = PcdMap::new(0.01, [1.0, 2.0]);
    pcd.set_disc(0.02);
    load_preserves(pcd, "PcdMap::Disco");

    let mut pcd_cen = PcdMap::new(0.01, [1.0, 2.0]);
    pcd_cen.set_pcd_cen(1, 3.0).unwrap();
    load_preserves(pcd_cen, "PcdMap::PcdCen");

    let mut grism = GrismMap::new(1.5, 0.0, 6000.0, 0.0, 1000.0, 1, 0.0, 0.0);
    grism.set_grism_g(1200.0);
    load_preserves(grism, "GrismMap::GrismG");
}
