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
//! * `testselfsimplifyafterset` sets the attribute on the simplified result,
//!   which is a fresh owned Mapping here rather than a shared pointer.
//! * The rest of `testmapping.c` (PolyMap / LinearApprox, converted from
//!   `testmapping.f`) is covered by `test_polymap.rs` and `test_polytran.rs`.

use crate::common::native_dump;
use ast_core::AnyObject;
use ast_core::mappings::{
    CmpMap, Composition, DynMapping, Mapping, PcdMap, PolyMap, PolyTerm, UnitMap, ZoomMap,
};

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
    native_dump(&obj).unwrap()
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

/// `testselfsimplifyafterset`: a simplified Mapping whose attribute is then set
/// to a value that makes it a unit transformation simplifies again, to a
/// UnitMap, because the setter discarded the simplified record.
#[test]
fn a_simplified_mapping_set_to_unit_simplifies_again() {
    // Two ZoomMaps in series simplify to one simplified ZoomMap.
    let input: DynMapping = Box::new(
        CmpMap::new(
            Box::new(ZoomMap::new(2, 2.0).unwrap()),
            Box::new(ZoomMap::new(2, 3.0).unwrap()),
            Composition::Series,
        )
        .unwrap(),
    );
    let mut zoom = input.simplify().unwrap();
    zoom.as_mut()
        .as_any_mut()
        .downcast_mut::<ZoomMap>()
        .expect("two ZoomMaps simplify to a ZoomMap")
        .set_zoom(1.0)
        .unwrap();
    let resimp = zoom.simplify().unwrap();
    assert!(
        resimp.as_ref().as_any().is::<UnitMap>(),
        "Error selfsimp-1: a simplified ZoomMap set to a unit Zoom did not \
         simplify to a UnitMap"
    );

    // A PcdMap in series with a UnitMap simplifies back to the PcdMap.
    let input: DynMapping = Box::new(
        CmpMap::new(
            Box::new(PcdMap::new(0.01, [0.0, 0.0])),
            Box::new(UnitMap::new(2)),
            Composition::Series,
        )
        .unwrap(),
    );
    let mut pcd = input.simplify().unwrap();
    pcd.as_mut()
        .as_any_mut()
        .downcast_mut::<PcdMap>()
        .expect("a PcdMap and a UnitMap simplify to the PcdMap")
        .set_disc(0.0);
    let resimp = pcd.simplify().unwrap();
    assert!(
        resimp.as_ref().as_any().is::<UnitMap>(),
        "Error selfsimp-2: a simplified PcdMap set to a zero Disco did not \
         simplify to a UnitMap"
    );
}

/// testmapping.c Errors 0-4: `linear_approx` of the PolyMap out1 = 1 + 2x,
/// out2 = 1 + 3y over [-1,1]² is exact; with out2's y coefficient bad, out2's
/// row of the fit is bad and out1's is unaffected; and adding a 3y² term
/// makes the fit fail. C's comment puts that term on out2, but its
/// coefficient row (3.0, 1, 0, 2) puts it on out1, which is what C tests.
#[test]
fn linear_approx_of_a_polymap() {
    use ast_core::constants::BAD_VALUE;
    let term = |coeff: f64, out_axis: usize, px: usize, py: usize| PolyTerm {
        coeff,
        powers: vec![px, py],
        out_axis,
    };
    let terms = |c3: f64| {
        vec![
            term(1.0, 1, 0, 0),
            term(2.0, 1, 1, 0),
            term(1.0, 2, 0, 0),
            term(c3, 2, 0, 1),
        ]
    };
    let (lbnd, ubnd) = ([-1.0, -1.0], [1.0, 1.0]);

    let pm = PolyMap::new(2, 2, &terms(3.0), None).unwrap();
    let fit = pm
        .linear_approx(&lbnd, &ubnd, 0.001)
        .unwrap()
        .expect("Error 1");
    assert_eq!(fit.shift, [1.0, 1.0], "Error 0");
    assert_eq!(fit.matrix, [2.0, 0.0, 0.0, 3.0], "Error 0");

    let pm = PolyMap::new(2, 2, &terms(BAD_VALUE), None).unwrap();
    let fit = pm
        .linear_approx(&lbnd, &ubnd, 0.001)
        .unwrap()
        .expect("Error 3");
    assert_eq!(fit.shift, [1.0, BAD_VALUE], "Error 2");
    assert_eq!(fit.matrix, [2.0, 0.0, BAD_VALUE, BAD_VALUE], "Error 2");

    let mut quadratic = terms(BAD_VALUE);
    quadratic.push(term(3.0, 1, 0, 2));
    let pm = PolyMap::new(2, 2, &quadratic, None).unwrap();
    assert!(
        pm.linear_approx(&lbnd, &ubnd, 0.001).unwrap().is_none(),
        "Error 4"
    );
}

/// testmapping.c Errors 5-8: PermMap equality compares the values of the
/// constants an axis is fed from, not their indices, and ignores constants
/// no axis references.
#[test]
fn perm_map_equality_compares_referenced_constant_values() {
    use ast_core::mappings::PermMap;
    let pm = |perm: i64, con: &[f64]| PermMap::new(&[perm], &[perm], con).unwrap();
    assert!(!pm(-1, &[5.0]).equal(&pm(-1, &[7.0])), "Error 5");
    assert!(pm(-1, &[5.0]).equal(&pm(-1, &[5.0])), "Error 6");
    assert!(pm(-1, &[5.0]).equal(&pm(-2, &[9.0, 5.0])), "Error 7");
    assert!(pm(-1, &[5.0, 7.0]).equal(&pm(-1, &[5.0, 9.0])), "Error 8");
}

/// C's `astSimplify` clears an AST__TRNND or AST__NODEF raised while reducing
/// a Mapping and returns the Mapping unsimplified.
/// A PermMap whose outputs are both constants, followed by a parallel CmpMap,
/// is swapped with it by transforming the constants through the CmpMap's
/// components, and the first component here has no forward transformation.
/// The expected dump is what the C library writes for the same Mappings: the
/// outer CmpMap carries no `IsSimp`, while the parallel CmpMap, simplified
/// before the merge failed, does.
#[test]
fn simplify_clears_an_undefined_transformation_error() {
    use ast_core::mappings::{MathMap, PermMap};
    let pm = PermMap::new(&[1, 2], &[-1, -2], &[3.0, 4.0]).unwrap();
    let mm = MathMap::new(1, 1, &["y"], &["x=y"]).unwrap();
    assert!(!mm.has_forward());
    let par = CmpMap::new(
        Box::new(mm),
        Box::new(UnitMap::new(1)),
        Composition::Parallel,
    )
    .unwrap();
    let ser = CmpMap::new(Box::new(pm), Box::new(par), Composition::Series).unwrap();
    let simplified = <dyn Mapping>::simplify(&ser).unwrap();
    let got = native_dump(&AnyObject::from_mapping(simplified.as_ref()).unwrap()).unwrap();
    let want = " Begin CmpMap 	# Compound Mapping
    Nin = 2 	# Number of input coordinates
    Fwd = 0 	# Forward transformation not defined
 IsA Mapping 	# Mapping between coordinate systems
    MapA = 	# First component Mapping
       Begin PermMap 	# Coordinate permutation
          Nin = 2 	# Number of input coordinates
       IsA Mapping 	# Mapping between coordinate systems
          Out1 = -1 	# Output coordinate 1 = constant no. 1
          Out2 = -2 	# Output coordinate 2 = constant no. 2
          In1 = 1 	# Input coordinate 1 = output coordinate 1
          In2 = 2 	# Input coordinate 2 = output coordinate 2
          Nconst = 2 	# Number of constants
          Con1 = 3 	# Constant number 1
          Con2 = 4 	# Constant number 2
       End PermMap
    MapB = 	# Second component Mapping
       Begin CmpMap 	# Compound Mapping
          Nin = 2 	# Number of input coordinates
          IsSimp = 1 	# Mapping has been simplified
          Fwd = 0 	# Forward transformation not defined
       IsA Mapping 	# Mapping between coordinate systems
          Series = 0 	# Component Mappings applied in parallel
          MapA = 	# First component Mapping
             Begin MathMap 	# Transformation using mathematical functions
                Nin = 1 	# Number of input coordinates
                Fwd = 0 	# Forward transformation not defined
             IsA Mapping 	# Mapping between coordinate systems
                Fwd1 = \"y\" 	# Forward function 1
                Inv1 = \"x=y\" 	# Inverse function 1
             End MathMap
          MapB = 	# Second component Mapping
             Begin UnitMap 	# Unit (null) Mapping
                Nin = 1 	# Number of input coordinates
             IsA Mapping 	# Mapping between coordinate systems
             End UnitMap
       End CmpMap
 End CmpMap
";
    assert_eq!(got, want);
}
