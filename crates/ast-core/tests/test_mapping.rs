//! Mapping-level tests for the record that a Mapping has been simplified.
//!
//! Ancestor: `reference/starlink-ast/ast_tester/testmapping.c` — the
//! `testequaldumpstable`, `testsimplevsinvert`, `testsimplevsinvertdump` and
//! `testmutationclearssimple` functions added with the C fix at `e40e71b4`, plus
//! `testprotectedcmpequal` and `testprotectedcmpequalstable`, added with the
//! `CmpMap::Equal` recursion fix at `0f797ae7` / `16ed5d37`.
//!
//! Divergences from the C original:
//!
//! * C sets `Invert` to the value it already holds to show that a no-op set
//!   leaves the record alone. This API has no set-to-a-value form — `invert()`
//!   toggles — so the no-op is expressed as inverting and inverting back, which
//!   is the case the C fix exists to protect.
//! * `testselfsimplifyafterset` is not ported. It sets `ZoomMap`'s `Zoom` and
//!   `PcdMap`'s `Disco` after simplifying, and neither class has a `&mut self`
//!   setter here: their attributes are fixed at construction, so the stale
//!   record it guards against cannot be constructed. `PolyMap` can, and
//!   `mutating_an_attribute_discards_the_record` covers it.
//! * The rest of `testmapping.c` (PolyMap / LinearApprox, converted from
//!   `testmapping.f`) is covered by `test_polymap.rs` and `test_polytran.rs`.

use ast_core::mappings::{CmpMap, Composition, DynMapping, Mapping, PolyMap, PolyTerm, ZoomMap};
use ast_core::{AnyObject, to_native_string};

/// `x -> x^2`, `y -> y^2`: non-linear, so it stops the tree collapsing.
fn square_map() -> PolyMap {
    PolyMap::new(
        2,
        2,
        &[
            PolyTerm {
                coeff: 1.0,
                powers: vec![2, 0],
                out_axis: 1,
            },
            PolyTerm {
                coeff: 1.0,
                powers: vec![0, 2],
                out_axis: 2,
            },
        ],
        None,
    )
    .unwrap()
}

/// A simplified two-component parallel CmpMap whose first component is itself
/// the result of a simplification, so both it and the CmpMap are recorded as
/// simplified.
fn simplified_tree() -> DynMapping {
    let series: DynMapping = Box::new(
        CmpMap::new(
            Box::new(ZoomMap::new(2, 2.0).unwrap()),
            Box::new(ZoomMap::new(2, 3.0).unwrap()),
            Composition::Series,
        )
        .unwrap(),
    );
    let inner = series.simplify().unwrap();
    let parallel: DynMapping =
        Box::new(CmpMap::new(inner, Box::new(square_map()), Composition::Parallel).unwrap());
    parallel.simplify().unwrap()
}

fn dump(m: &DynMapping) -> String {
    let obj = if let Some(c) = m.as_any().downcast_ref::<CmpMap>() {
        AnyObject::CmpMap(c.clone())
    } else if let Some(z) = m.as_any().downcast_ref::<ZoomMap>() {
        AnyObject::ZoomMap(z.clone())
    } else {
        panic!("no AnyObject wrapper for this test's Mapping")
    };
    to_native_string(&obj).unwrap()
}

/// Two ZoomMaps in series, simplified: a single ZoomMap that is the result of
/// a simplification and has no components of its own to carry a tag.
fn simplified_zoom() -> DynMapping {
    let series: DynMapping = Box::new(
        CmpMap::new(
            Box::new(ZoomMap::new(1, 2.0).unwrap()),
            Box::new(ZoomMap::new(1, 3.0).unwrap()),
            Composition::Series,
        )
        .unwrap(),
    );
    series.simplify().unwrap()
}

fn count_tags(text: &str) -> usize {
    text.lines()
        .filter(|l| l.trim_start().starts_with("IsSimp ="))
        .count()
}

/// `equal` is documented as a comparison, so it must not change how either
/// operand serialises. The CmpMap implementation decomposes both operands and
/// lines them up by orientation, which used to discard the record that a
/// component had been simplified.
#[test]
fn comparing_two_mappings_leaves_the_dump_unchanged() {
    let t = simplified_tree();
    let u = simplified_tree();

    let before = dump(&t);
    assert!(t.equal(u.as_ref()), "the two trees must compare equal");
    let after = dump(&t);

    // Both the CmpMap and its ZoomMap component are results of a
    // simplification, so the dump must tag both. Without this the stability
    // check below would pass on a dump that tags nothing.
    assert_eq!(count_tags(&before), 2, "dump:\n{before}");
    assert_eq!(before, after, "comparing the Mapping changed its dump");
}

/// Whether a Mapping needs re-simplifying depends on the orientation it is in
/// and on nothing else. A simplified Mapping that is inverted needs looking at
/// again, since the inverse may simplify differently, but restoring the
/// original orientation restores the original Mapping and hence the record.
#[test]
fn the_record_follows_the_orientation() {
    let mut s = simplified_zoom();
    assert!(s.has_been_simplified(), "simplify must record its result");

    s.invert();
    assert!(
        !s.has_been_simplified(),
        "the inverted orientation has not been simplified"
    );

    s.invert();
    assert!(
        s.has_been_simplified(),
        "restoring the orientation restores the record"
    );
}

/// As above, but reading the dump rather than the record, since the two have
/// to agree.
#[test]
fn the_dump_tag_follows_the_orientation() {
    // A single ZoomMap, so the only tag in the dump is its own: a compound
    // result would keep its components' tags across the inversion, since
    // inverting the parent does not change how a child is stored.
    let mut s = simplified_zoom();

    let forward = dump(&s);
    s.invert();
    let inverted = dump(&s);
    s.invert();
    let back = dump(&s);

    assert_eq!(count_tags(&forward), 1, "dump:\n{forward}");
    assert_eq!(
        count_tags(&inverted),
        0,
        "an inverted Mapping must not claim it needs no re-simplifying:\n{inverted}"
    );
    assert_eq!(
        count_tags(&back),
        1,
        "the tag was lost over invert and back"
    );
    assert_eq!(forward, back, "the dump changed over invert and back");
}

/// The record is advisory: reporting that a Mapping may need re-simplifying is
/// always safe, whereas reporting that it does not can suppress a
/// simplification that is available. So anything that changes what a Mapping
/// does must discard it — including a change made through one of the Mapping's
/// own attributes, and a transformation replaced by `poly_tran`.
#[test]
fn mutating_an_attribute_discards_the_record() {
    // out1 = x + 0.1*x^2, out2 = y + 0.1*y^2. Monotonic over the bounds below,
    // so the inverse can be fitted, and not the identity, so it does not
    // simplify away to a UnitMap.
    let terms = vec![
        PolyTerm {
            coeff: 1.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 0.1,
            powers: vec![2, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 1.0,
            powers: vec![0, 1],
            out_axis: 2,
        },
        PolyTerm {
            coeff: 0.1,
            powers: vec![0, 2],
            out_axis: 2,
        },
    ];
    let build = || -> DynMapping { Box::new(PolyMap::new(2, 2, &terms, None).unwrap()) };

    // Setting an attribute.
    let mut set = build().simplify().unwrap();
    assert!(set.has_been_simplified());
    set.as_any_mut()
        .downcast_mut::<PolyMap>()
        .unwrap()
        .set_iter_inverse(false)
        .unwrap();
    assert!(
        !set.has_been_simplified(),
        "setting an attribute left the record in place"
    );

    // Clearing one back to its default changes the Mapping just as much.
    let mut input = PolyMap::new(2, 2, &terms, None).unwrap();
    input.set_iter_inverse(false).unwrap();
    let mut cleared: DynMapping = Box::new(input);
    cleared = cleared.simplify().unwrap();
    assert!(cleared.has_been_simplified());
    cleared
        .as_any_mut()
        .downcast_mut::<PolyMap>()
        .unwrap()
        .clear_iter_inverse();
    assert!(
        !cleared.has_been_simplified(),
        "clearing an attribute left the record in place"
    );

    // `poly_tran` returns a copy with one transformation replaced, so the copy
    // must not inherit the record.
    let fitted = build().simplify().unwrap();
    assert!(fitted.has_been_simplified());
    let refitted = fitted
        .as_any()
        .downcast_ref::<PolyMap>()
        .unwrap()
        .poly_tran(false, 1.0e-5, 1.0e-3, 8, &[-1.0, -1.0], &[1.0, 1.0])
        .unwrap()
        .expect("the inverse of a monotonic quadratic must be fittable");
    assert!(
        !refitted.has_been_simplified(),
        "poly_tran returned a Mapping still claiming to be simplified"
    );
}

/// Two protected CmpMaps that describe the same transformation compare equal.
///
/// Ancestor: `testprotectedcmpequal` (added upstream with `0f797ae7`). `Ident`
/// makes `astDoNotSimplify` true, so `astMapList` refuses to decompose either
/// operand and returns a list holding a clone of the CmpMap itself
/// (`cmpmap.c:1154`). Comparing those lists element by element calls `astEqual`
/// on the same pair again, which recursed until the stack was exhausted before
/// upstream `16ed5d37`. The port decomposes the components instead, so this both
/// terminates and gives the right answer.
///
/// `b` is the reversed pair with each component inverted and the CmpMap itself
/// inverted, which is the same effective sequence and the same component senses
/// as `a`.
#[test]
fn protected_cmp_maps_describing_the_same_mapping_are_equal() {
    let a = {
        let a1 = ZoomMap::new(1, 2.0).unwrap();
        let a2 = ZoomMap::new(1, 3.0).unwrap();
        let mut a = CmpMap::new(Box::new(a1), Box::new(a2), Composition::Series).unwrap();
        a.set_ident("keep");
        a
    };
    let b = {
        let mut b1 = ZoomMap::new(1, 3.0).unwrap();
        b1.invert();
        let mut b2 = ZoomMap::new(1, 2.0).unwrap();
        b2.invert();
        let mut b = CmpMap::new(Box::new(b1), Box::new(b2), Composition::Series).unwrap();
        b.set_ident("keep");
        b.invert();
        b
    };
    assert!(
        a.do_not_simplify() && b.do_not_simplify(),
        "Ident must make both protected, or the case does not reach the branch \
         it exists for"
    );
    assert!(
        (&a as &dyn Mapping).equal(&b as &dyn Mapping),
        "two protected CmpMaps describing the same transformation compared unequal"
    );

    // The control. Without it the assertion above would pass for an `equal`
    // that returned true for any protected pair, which is exactly what a
    // recursion guard written the wrong way would produce.
    let c = {
        let c1 = ZoomMap::new(1, 2.0).unwrap();
        let c2 = ZoomMap::new(1, 5.0).unwrap();
        let mut c = CmpMap::new(Box::new(c1), Box::new(c2), Composition::Series).unwrap();
        c.set_ident("keep");
        c
    };
    assert!(
        !(&a as &dyn Mapping).equal(&c as &dyn Mapping),
        "two protected CmpMaps with different zoom factors compared equal"
    );
}

/// Comparing protected CmpMaps that share a component leaves that component's
/// live `Invert` alone.
///
/// Ancestor: `testprotectedcmpequalstable`. C has to save and restore the
/// component's `Invert` around the comparison, because it re-instates the value
/// the component had when the CmpMap was built. The port holds components by
/// value, so a CmpMap cannot observe or disturb anything a caller does to the
/// Mapping it was built from; this pins that, and pins the equality the C test
/// asserts alongside it.
#[test]
fn comparing_protected_cmp_maps_leaves_a_shared_component_alone() {
    let mut map1 = ZoomMap::new(1, 2.0).unwrap();
    let map2 = ZoomMap::new(1, 3.0).unwrap();
    let build = |m1: &ZoomMap, m2: &ZoomMap| {
        let mut c = CmpMap::new(
            (m1 as &dyn Mapping).clone_box(),
            (m2 as &dyn Mapping).clone_box(),
            Composition::Series,
        )
        .unwrap();
        c.set_ident("keep");
        c
    };
    let a = build(&map1, &map2);
    let b = build(&map1, &map2);

    // The CmpMaps hold the components as they were; a caller stays free to
    // change the Mapping it built them from.
    map1.invert();
    let before = map1.is_inverted();
    assert!(before, "invert() did not take");

    assert!(
        (&a as &dyn Mapping).equal(&b as &dyn Mapping),
        "two protected CmpMaps built from the same components compared unequal"
    );
    assert_eq!(
        map1.is_inverted(),
        before,
        "comparing the CmpMaps changed the component's Invert"
    );
}
