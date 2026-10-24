//! Tests for CmpMap (compound mapping) and composition methods.
//!
//! Ancestor: behavior derived from the C AST CmpMap documentation and
//! reference implementation.

use crate::common::native_dump;
use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;
use starlink_ast::mappings::{CmpMap, Composition, Mapping, ShiftMap, UnitMap, ZoomMap};

// ------------------------------------------------------------------ //
// Series composition                                                   //
// ------------------------------------------------------------------ //

/// zoom(2) then shift(10): y = 2*x + 10.
#[test]
fn cmpmap_series_zoom_then_shift() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([10.0]));
    let cm = CmpMap::new(z, s, Composition::Series).unwrap();
    assert_eq!(cm.nin(), 1);
    assert_eq!(cm.nout(), 1);
    let pts = PointSet::from_rows([[5.0]]);
    let out = cm.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 20.0).abs() < 1e-12);
}

/// Series inverse: undo shift then undo zoom → (y - 10) / 2.
#[test]
fn cmpmap_series_inverse() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([10.0]));
    let cm = CmpMap::new(z, s, Composition::Series).unwrap();
    // Forward: 5 → 20.  Inverse of 20 → 5.
    let pts = PointSet::from_rows([[20.0]]);
    let out = cm.transform(&pts, false).unwrap();
    assert!((out.point(0)[0] - 5.0).abs() < 1e-12);
}

// ------------------------------------------------------------------ //
// Parallel composition                                                 //
// ------------------------------------------------------------------ //

/// zoom(3) alongside shift(100): first coord scaled by 3, second shifted by 100.
#[test]
fn cmpmap_parallel_zoom_alongside_shift() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 3.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([100.0]));
    let cm = CmpMap::new(z, s, Composition::Parallel).unwrap();
    assert_eq!(cm.nin(), 2);
    assert_eq!(cm.nout(), 2);
    let pts = PointSet::from_rows([[2.0, 5.0]]);
    let out = cm.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 6.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 105.0).abs() < 1e-12);
}

// ------------------------------------------------------------------ //
// Error cases                                                          //
// ------------------------------------------------------------------ //

/// Series dimension mismatch must return an error.
#[test]
fn cmpmap_series_dimension_mismatch_error() {
    let a: Box<dyn Mapping> = Box::new(ZoomMap::new(2, 2.0).unwrap()); // nout=2
    let b: Box<dyn Mapping> = Box::new(ZoomMap::new(3, 3.0).unwrap()); // nin=3
    assert!(CmpMap::new(a, b, Composition::Series).is_err());
}

// ------------------------------------------------------------------ //
// BAD_VALUE propagation                                                //
// ------------------------------------------------------------------ //

/// BAD_VALUE propagates through a series CmpMap.
#[test]
fn cmpmap_series_bad_value_propagation() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([10.0]));
    let cm = CmpMap::new(z, s, Composition::Series).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE]]);
    let out = cm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
}

/// BAD_VALUE propagates through a parallel CmpMap.
#[test]
fn cmpmap_parallel_bad_value_propagation() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([10.0]));
    let cm = CmpMap::new(z, s, Composition::Parallel).unwrap();
    let pts = PointSet::from_rows([[BAD_VALUE, 5.0]]);
    let out = cm.transform(&pts, true).unwrap();
    assert_eq!(out.point(0)[0], BAD_VALUE);
}

// ------------------------------------------------------------------ //
// simplified()                                                         //
// ------------------------------------------------------------------ //

/// Two UnitMaps in series simplify to a single UnitMap.
#[test]
fn cmpmap_simplified_two_unitmaps_series() {
    let a: Box<dyn Mapping> = Box::new(UnitMap::new(2));
    let b: Box<dyn Mapping> = Box::new(UnitMap::new(2));
    let cm = CmpMap::new(a, b, Composition::Series).unwrap();
    let simp = <dyn Mapping>::simplify(&cm).unwrap();
    let pts = PointSet::from_rows([[3.0, 7.0]]);
    let out = simp.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[3.0, 7.0]);
}

/// Two UnitMaps in parallel simplify to a single wider UnitMap.
#[test]
fn cmpmap_simplified_two_unitmaps_parallel() {
    let a: Box<dyn Mapping> = Box::new(UnitMap::new(1));
    let b: Box<dyn Mapping> = Box::new(UnitMap::new(2));
    let cm = CmpMap::new(a, b, Composition::Parallel).unwrap();
    let simp = <dyn Mapping>::simplify(&cm).unwrap();
    assert_eq!(simp.nin(), 3);
    let pts = PointSet::from_rows([[1.0, 2.0, 3.0]]);
    let out = simp.transform(&pts, true).unwrap();
    assert_eq!(out.point(0), &[1.0, 2.0, 3.0]);
}

// ------------------------------------------------------------------ //
// Composition methods on Box<dyn Mapping>                             //
// ------------------------------------------------------------------ //

/// `then()` on Box<dyn Mapping> constructs a series CmpMap.
#[test]
fn dyn_mapping_then_method() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 2.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([5.0]));
    let cm = z.then(s).unwrap();
    let pts = PointSet::from_rows([[3.0]]);
    let out = cm.transform(&pts, true).unwrap();
    // 3.0 * 2 + 5 = 11
    assert!((out.point(0)[0] - 11.0).abs() < 1e-12);
}

/// `alongside()` on Box<dyn Mapping> constructs a parallel CmpMap.
#[test]
fn dyn_mapping_alongside_method() {
    let z: Box<dyn Mapping> = Box::new(ZoomMap::new(1, 4.0).unwrap());
    let s: Box<dyn Mapping> = Box::new(ShiftMap::new([100.0]));
    let cm = z.alongside(s).unwrap();
    assert_eq!(cm.nin(), 2);
    assert_eq!(cm.nout(), 2);
    let pts = PointSet::from_rows([[2.0, 50.0]]);
    let out = cm.transform(&pts, true).unwrap();
    assert!((out.point(0)[0] - 8.0).abs() < 1e-12);
    assert!((out.point(0)[1] - 150.0).abs() < 1e-12);
}

/// Series pair (M, M^-1) of MathMaps with SimpFI/SimpIF set cancels to
/// UnitMap via MathMap::try_merge. Mirrors C mathmap.c MapMerge.
#[test]
fn series_inverse_pair_of_mathmaps_cancels_to_unitmap() {
    use starlink_ast::mappings::{DynMapping, MathMap};

    let mut m_fwd = MathMap::new(1, 1, &["y=2*x"], &["x=0.5*y"]).unwrap();
    m_fwd.set_simp_fi(true);
    m_fwd.set_simp_if(true);
    let mut m_inv = m_fwd.clone();
    m_inv.invert();

    let cmp = CmpMap::new(
        Box::new(m_fwd) as DynMapping,
        Box::new(m_inv) as DynMapping,
        Composition::Series,
    )
    .unwrap();

    let simp = <dyn Mapping>::simplify(&cmp).unwrap();
    assert!(
        simp.as_any().is::<UnitMap>(),
        "expected UnitMap after simplify (got nin={}, nout={})",
        simp.nin(),
        simp.nout(),
    );
}

/// A simplified parallel CmpMap that contains an inverted CmpMap child
/// must serialize without redundant `Invert` flag on the inner CmpMap
/// AND without `InvA` on the outer parallel — the inversion should be
/// pushed into the inner's `inv_a`/`inv_b` (with children swapped for
/// series). Mirrors the cmpmap_series_in_parallel_inverted fixture and
/// the C cmpmap.c MapMerge canonicalization. Numerically identical
/// before and after canonicalization.
#[test]
fn parallel_with_inverted_cmpmap_child_canonicalises_for_serialisation() {
    use starlink_ast::AnyObject;
    use starlink_ast::mappings::{DynMapping, MathMap};
    fn make_inner_series() -> CmpMap {
        let mut a = MathMap::new(1, 1, &["y=2*x"], &["x=0.5*y"]).unwrap();
        a.set_simp_fi(true);
        a.set_simp_if(true);
        let b = a.clone();
        CmpMap::new(
            Box::new(a) as DynMapping,
            Box::new(b) as DynMapping,
            Composition::Series,
        )
        .unwrap()
    }

    // outer parallel CmpMap with first child applied in inverse direction.
    let mut first = make_inner_series();
    first.invert();
    let second = make_inner_series();
    let outer = CmpMap::new(
        Box::new(first) as DynMapping,
        Box::new(second) as DynMapping,
        Composition::Parallel,
    )
    .unwrap();
    let outer_dyn: DynMapping = Box::new(outer);

    let simp = outer_dyn.simplify().unwrap();
    let cmp = simp.as_any().downcast_ref::<CmpMap>().unwrap().clone();
    let s = native_dump(&AnyObject::CmpMap(cmp)).unwrap();
    // Outer should NOT carry InvA. Inner-A SHOULD carry InvA=1, InvB=1.
    // We check by counting Begin CmpMap occurrences and the position of
    // any InvA = 1 line.
    let begin_indents: Vec<usize> = s
        .lines()
        .filter(|l| l.contains("Begin CmpMap"))
        .map(|l| l.len() - l.trim_start().len())
        .collect();
    let outer_indent = begin_indents[0];
    let outer_inva = s.lines().any(|l| {
        let ind = l.len() - l.trim_start().len();
        ind == outer_indent + 4 && l.trim_start().starts_with("InvA = 1")
    });
    assert!(
        !outer_inva,
        "outer parallel must not have InvA after canonicalisation:\n{s}"
    );
    let inner_inva = s.lines().filter(|l| l.contains("InvA = 1")).count();
    let inner_invb = s.lines().filter(|l| l.contains("InvB = 1")).count();
    assert!(
        inner_inva >= 1 && inner_invb >= 1,
        "inner CmpMap must carry InvA=1 and InvB=1:\n{s}"
    );
    // No Invert line on any CmpMap in the canonicalized output.
    let invert_lines: Vec<&str> = s.lines().filter(|l| l.contains("Invert = 1")).collect();
    assert!(
        invert_lines.is_empty(),
        "no inner CmpMap should carry Invert=1 after canonicalisation:\n{s}"
    );
}

/// C compares CmpMaps by setting each component's Invert flag to the one the
/// CmpMap applies and asking the component's own Equal (cmpmap.c Equal), so
/// an inverted UnitMap equals a UnitMap and an inverted ZoomMap(2) equals
/// ZoomMap(0.5); ZoomMap(2) does not. The expectations are C's.
#[test]
fn cmpmap_equality_compares_components_as_applied() {
    let series =
        |a: Box<dyn Mapping>, b: Box<dyn Mapping>| CmpMap::new(a, b, Composition::Series).unwrap();
    let inverted = |mut m: Box<dyn Mapping>| {
        m.invert();
        m
    };
    let zoom = |f: f64| -> Box<dyn Mapping> { Box::new(ZoomMap::new(1, f).unwrap()) };
    let shift = || -> Box<dyn Mapping> { Box::new(ShiftMap::new(vec![1.0])) };

    let a = series(inverted(Box::new(UnitMap::new(1))), zoom(2.0));
    let b = series(Box::new(UnitMap::new(1)), zoom(2.0));
    assert!(a.equal(&b));

    let a = series(inverted(zoom(2.0)), shift());
    let b = series(zoom(0.5), shift());
    assert!(a.equal(&b));

    let a = series(zoom(2.0), shift());
    let b = series(zoom(0.5), shift());
    assert!(!a.equal(&b));
}

/// A FrameSet given as a component is replaced by its base-to-current
/// Mapping, as C's astInitCmpMap does. The expected dump is C's for the same
/// construction.
#[test]
fn a_frameset_component_becomes_its_mapping() {
    use starlink_ast::AnyObject;
    use starlink_ast::frames::{BasicFrameBuilder, FrameRef, FrameSet};

    let frame = || Box::new(BasicFrameBuilder::new(2).build().unwrap());
    let mut fs = FrameSet::new(frame());
    fs.add_frame(
        FrameRef::Base,
        Box::new(ZoomMap::new(2, 3.0).unwrap()),
        frame(),
    )
    .unwrap();
    let cmp = CmpMap::new(Box::new(fs), Box::new(UnitMap::new(2)), Composition::Series).unwrap();
    let want = [
        " Begin CmpMap \t# Compound Mapping",
        "    Nin = 2 \t# Number of input coordinates",
        " IsA Mapping \t# Mapping between coordinate systems",
        "    MapA = \t# First component Mapping",
        "       Begin ZoomMap \t# Zoom about the origin",
        "          Nin = 2 \t# Number of input coordinates",
        "       IsA Mapping \t# Mapping between coordinate systems",
        "          Zoom = 3 \t# Zoom factor",
        "       End ZoomMap",
        "    MapB = \t# Second component Mapping",
        "       Begin UnitMap \t# Unit (null) Mapping",
        "          Nin = 2 \t# Number of input coordinates",
        "       IsA Mapping \t# Mapping between coordinate systems",
        "       End UnitMap",
        " End CmpMap",
    ]
    .join("\n")
        + "\n";
    assert_eq!(native_dump(&AnyObject::CmpMap(cmp)).unwrap(), want);
}

/// `decompose` gives the components and the Invert values to apply them
/// with, as C's `astDecompose` does (C probe): an inverted series CmpMap
/// swaps the components and their flags, an inverted parallel CmpMap keeps
/// the order and negates the flags.
#[test]
fn decompose_gives_components_and_invert_values_like_c() {
    let zoom = || -> Box<dyn Mapping> { Box::new(ZoomMap::new(1, 2.0).unwrap()) };
    let shift = || -> Box<dyn Mapping> {
        let mut s = ShiftMap::new(vec![1.0]);
        s.invert();
        Box::new(s)
    };
    let is_zoom = |m: &dyn Mapping| m.as_any().is::<ZoomMap>();
    let is_shift = |m: &dyn Mapping| m.as_any().is::<ShiftMap>();

    let mut ser = CmpMap::new(zoom(), shift(), Composition::Series).unwrap();
    let (d, kind) = ser.decompose();
    assert_eq!(kind, Composition::Series);
    assert!(is_zoom(d.map1) && is_shift(d.map2));
    assert_eq!((d.invert1, d.invert2), (false, true));
    ser.invert();
    let (d, kind) = ser.decompose();
    assert_eq!(kind, Composition::Series);
    assert!(is_shift(d.map1) && is_zoom(d.map2));
    assert_eq!((d.invert1, d.invert2), (false, true));

    let mut par = CmpMap::new(zoom(), shift(), Composition::Parallel).unwrap();
    let (d, kind) = par.decompose();
    assert_eq!(kind, Composition::Parallel);
    assert!(is_zoom(d.map1) && is_shift(d.map2));
    assert_eq!((d.invert1, d.invert2), (false, true));
    par.invert();
    let (d, kind) = par.decompose();
    assert_eq!(kind, Composition::Parallel);
    assert!(is_zoom(d.map1) && is_shift(d.map2));
    assert_eq!((d.invert1, d.invert2), (true, false));
}
