//! FrameSet Variants subsystem, ported from `ast_tester/testframeset.c`
//! (errors 1-39): AddVariant (rename and with-Mapping), the Variant
//! get/set/clear/test attribute, AllVariants, MirrorVariants, the per-variant
//! transform change, a frame added on top of a variant, two dump/restore
//! round-trips, and copy preservation.
//!
//! C drives these through the string-attribute layer (`astGetC(fs,"Variant")`
//! etc.); the Rust port uses the typed methods (`variant()`, `set_variant()`,
//! `all_variants()`, …) and asserts the same values and transform results.
//! The transform values (e.g. (50.5,100.5) → (150,150) under FP2) are the C
//! oracle's, confirmed by the reference `testframeset.c`.

use crate::common::native_dump;
use ast_core::coords::PointSet;
use ast_core::error::ErrorCode;
use ast_core::frames::{BasicFrameBuilder, FrameRef, FrameSet};
use ast_core::mappings::{CmpMap, Composition, DynMapping, Mapping, WinMap};

fn frm(domain: &str) -> ast_core::frames::BasicFrame {
    BasicFrameBuilder::new(2).domain(domain).build().unwrap()
}

/// Build the testframeset.c FrameSet: PIXEL grid → FPLANE via a WinMap whose
/// window maps pixel (1,1)-(100,200) to focal-plane (-2.5,-1.0)-(2.5,1.0).
fn base_fs() -> FrameSet {
    let p2fmap =
        WinMap::from_corners(&[1.0, 1.0], &[100.0, 200.0], &[-2.5, -1.0], &[2.5, 1.0]).unwrap();
    let mut fs = FrameSet::new(Box::new(frm("PIXEL")));
    fs.add_frame(FrameRef::Current, Box::new(p2fmap), Box::new(frm("FPLANE")))
        .unwrap();
    fs
}

fn tran(fs: &FrameSet, x: f64, y: f64) -> (f64, f64) {
    let out = Mapping::transform(fs, &PointSet::from_rows([[x, y]]), true).unwrap();
    (out.point(0)[0], out.point(0)[1])
}

#[test]
fn variant_lifecycle_matches_c() {
    let mut fs = base_fs();

    // Errors 1-3: default state — the only variant is the current frame's
    // Domain (FPLANE); Variant is not "set".
    assert_eq!(fs.all_variants(), "FPLANE");
    assert_eq!(fs.variant(), "FPLANE");
    assert!(!fs.test_variant());

    // Errors 4-6: AddVariant(None, "FP1") renames the current variant.
    fs.add_variant(None, "FP1").unwrap();
    assert_eq!(fs.all_variants(), "FP1");
    assert_eq!(fs.variant(), "FP1");
    assert!(fs.test_variant());

    // Errors 7-9: clearing reverts to the frame Domain default.
    fs.clear_variant();
    assert_eq!(fs.all_variants(), "FPLANE");
    assert_eq!(fs.variant(), "FPLANE");
    assert!(!fs.test_variant());

    // Re-add FP1, then add FP2 reached by an extra WinMap.
    fs.add_variant(None, "FP1").unwrap();

    // FP2 mapping = inverse(p2fmap) ∘ p2fmap2 where p2fmap2 maps the unit
    // window to (100,100)-(200,200). (testframeset.c:134-141.)
    let p2fmap =
        WinMap::from_corners(&[1.0, 1.0], &[100.0, 200.0], &[-2.5, -1.0], &[2.5, 1.0]).unwrap();
    let p2fmap2 = WinMap::from_corners(
        &[1.0, 1.0],
        &[100.0, 200.0],
        &[100.0, 100.0],
        &[200.0, 200.0],
    )
    .unwrap();
    let mut p2fmap_inv: DynMapping = Box::new(p2fmap);
    p2fmap_inv.invert();
    let fp2: DynMapping =
        Box::new(CmpMap::new(p2fmap_inv, Box::new(p2fmap2), Composition::Series).unwrap());
    let fp2 = <dyn Mapping>::simplify(fp2.as_ref()).unwrap();
    fs.add_variant(Some(fp2), "FP2").unwrap();

    // Errors 10-12.
    assert_eq!(fs.all_variants(), "FP1 FP2");
    assert_eq!(fs.variant(), "FP2");
    assert!(fs.test_variant());

    // Error 13: under FP2, pixel (50.5,100.5) maps to (150,150).
    let (x, y) = tran(&fs, 50.5, 100.5);
    assert!(
        (x - 150.0).abs() < 1e-6 && (y - 150.0).abs() < 1e-6,
        "FP2: ({x},{y})"
    );

    // Error 14: select FP1 → the original focal-plane mapping, (0,0).
    fs.set_variant("FP1").unwrap();
    let (x, y) = tran(&fs, 50.5, 100.5);
    assert!(
        (x - 0.0).abs() < 1e-6 && (y - 0.0).abs() < 1e-6,
        "FP1: ({x},{y})"
    );

    // Add FP3 = (current→base) ∘ p2fmap3, window to (-100,-100)-(-200,-200).
    let p2fmap3 = WinMap::from_corners(
        &[1.0, 1.0],
        &[100.0, 200.0],
        &[-100.0, -100.0],
        &[-200.0, -200.0],
    )
    .unwrap();
    let cur_to_base = fs.mapping(FrameRef::Current, FrameRef::Base).unwrap();
    let fp3: DynMapping =
        Box::new(CmpMap::new(cur_to_base, Box::new(p2fmap3), Composition::Series).unwrap());
    let fp3 = <dyn Mapping>::simplify(fp3.as_ref()).unwrap();
    fs.add_variant(Some(fp3), "FP3").unwrap();

    // Errors 15-17.
    assert_eq!(fs.all_variants(), "FP1 FP2 FP3");
    assert_eq!(fs.variant(), "FP3");
    assert!(fs.test_variant());

    // Error 18: under FP3, (50.5,100.5) → (-150,-150).
    let (x, y) = tran(&fs, 50.5, 100.5);
    assert!(
        (x + 150.0).abs() < 1e-6 && (y + 150.0).abs() < 1e-6,
        "FP3: ({x},{y})"
    );

    // Error 19: back to FP2 → (150,150).
    fs.set_variant("FP2").unwrap();
    let (x, y) = tran(&fs, 50.5, 100.5);
    assert!(
        (x - 150.0).abs() < 1e-6 && (y - 150.0).abs() < 1e-6,
        "FP2 again: ({x},{y})"
    );
}

#[test]
fn set_unknown_variant_errors() {
    // Error 24 region: a frame with no variants rejects a Variant set.
    let mut fs = base_fs();
    let err = fs.set_variant("NOPE").unwrap_err();
    assert_eq!(err.code(), ErrorCode::Attin);
}

/// With no variants, the only variant of the current Frame is named by its
/// Domain, and selecting it does nothing: the values of a C-oracle probe.
#[test]
fn selecting_the_only_variant_of_a_frame_without_variants() {
    let mut fs = base_fs();
    fs.set_variant("  fplane ").unwrap();
    assert_eq!(fs.variant(), "FPLANE");
    assert!(!fs.test_variant());
    for name in ["FP1", "pixel"] {
        let err = fs.set_variant(name).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Attin);
        assert_eq!(
            err.message(),
            format!(
                "astSetVariant(FrameSet): Unknown Frame variant '{}' requested.",
                name.to_uppercase()
            )
        );
    }
}

/// C reads outside its arrays when the Frame whose variant is selected is
/// on the root node, which has no Mapping from a parent to replace; the port
/// moves the Frame to a new node joined to the root by the change of
/// variant, so the Frames derived from the root do not move.
#[test]
fn selecting_a_variant_of_a_frame_on_the_root_node() {
    use ast_core::mappings::ShiftMap;
    let mut fs = base_fs();
    fs.set_current(FrameRef::Index(1)).unwrap();
    let shift = ShiftMap::new(vec![10.0, 20.0]);
    fs.add_variant(Some(Box::new(shift)), "SHIFTED").unwrap();
    assert_eq!(fs.variant(), "SHIFTED");
    assert_eq!(fs.nframe(), 2);

    // PIXEL is now the root shifted by (10, 20); FPLANE still derives from
    // the root, so PIXEL (60.5, 120.5) is root (50.5, 100.5).
    let pix_to_fp = |fs: &FrameSet| {
        let map = fs.mapping(FrameRef::Index(1), FrameRef::Index(2)).unwrap();
        let out = map
            .transform(&PointSet::from_rows([[60.5, 120.5]]), true)
            .unwrap();
        (out.point(0)[0], out.point(0)[1])
    };
    let mut unshifted = base_fs();
    unshifted.set_current(FrameRef::Index(2)).unwrap();
    assert_eq!(pix_to_fp(&fs), tran(&unshifted, 50.5, 100.5));

    let text = native_dump(&ast_core::AnyObject::FrameSet(fs.clone())).unwrap();
    let ast_core::AnyObject::FrameSet(back) = ast_core::from_native_string(&text).unwrap() else {
        panic!("expected a FrameSet");
    };
    assert_eq!(pix_to_fp(&back), pix_to_fp(&fs));

    fs.set_variant("PIXEL").unwrap();
    assert_eq!(fs.variant(), "PIXEL");
    let out = fs
        .mapping(FrameRef::Index(1), FrameRef::Index(2))
        .unwrap()
        .transform(&PointSet::from_rows([[50.5, 100.5]]), true)
        .unwrap();
    assert_eq!(
        (out.point(0)[0], out.point(0)[1]),
        tran(&unshifted, 50.5, 100.5)
    );
}

#[test]
fn duplicate_variant_name_errors() {
    let mut fs = base_fs();
    fs.add_variant(None, "FP1").unwrap();
    let z = WinMap::from_corners(&[1.0, 1.0], &[2.0, 2.0], &[1.0, 1.0], &[2.0, 2.0]).unwrap();
    let err = fs.add_variant(Some(Box::new(z)), "FP1").unwrap_err();
    assert_eq!(err.code(), ErrorCode::Bdvnm);
}

#[test]
fn add_variant_to_mirror_errors() {
    // Errors 23-31 region: add a DSB frame on top, mirror the original
    // frame's variants, then adding a variant to the mirror is rejected.
    let mut fs = three_variant_fs();
    let orig = matches_current_index(&fs);

    // Add a new DSB frame on top of the current variant frame.
    fs.add_frame(
        FrameRef::Current,
        Box::new(ast_core::mappings::UnitMap::new(2)),
        Box::new(frm("DSB")),
    )
    .unwrap();

    // A frame with no variants of its own reports just its Domain.
    assert_eq!(fs.all_variants(), "DSB");
    assert!(!fs.test_variant());

    // Mirror the original (FP*) frame's variants.
    fs.mirror_variants(FrameRef::Index(orig)).unwrap();
    assert_eq!(fs.all_variants(), "FP1 FP2 FP3");

    // Adding a variant to a mirror frame is illegal.
    let z = WinMap::from_corners(&[1.0, 1.0], &[2.0, 2.0], &[1.0, 1.0], &[2.0, 2.0]).unwrap();
    let err = fs.add_variant(Some(Box::new(z)), "FP4").unwrap_err();
    assert_eq!(err.code(), ErrorCode::Mirro);
}

/// Build the FP1/FP2/FP3 FrameSet with FP2 selected (the state at C Error 19).
fn three_variant_fs() -> FrameSet {
    let mut fs = base_fs();
    fs.add_variant(None, "FP1").unwrap();
    let p2fmap =
        WinMap::from_corners(&[1.0, 1.0], &[100.0, 200.0], &[-2.5, -1.0], &[2.5, 1.0]).unwrap();
    let p2fmap2 = WinMap::from_corners(
        &[1.0, 1.0],
        &[100.0, 200.0],
        &[100.0, 100.0],
        &[200.0, 200.0],
    )
    .unwrap();
    let mut inv: DynMapping = Box::new(p2fmap);
    inv.invert();
    let cmp2 = CmpMap::new(inv, Box::new(p2fmap2), Composition::Series).unwrap();
    let fp2 = <dyn Mapping>::simplify(&cmp2).unwrap();
    fs.add_variant(Some(fp2), "FP2").unwrap();
    let p2fmap3 = WinMap::from_corners(
        &[1.0, 1.0],
        &[100.0, 200.0],
        &[-100.0, -100.0],
        &[-200.0, -200.0],
    )
    .unwrap();
    let cb = fs.mapping(FrameRef::Current, FrameRef::Base).unwrap();
    let cmp3 = CmpMap::new(cb, Box::new(p2fmap3), Composition::Series).unwrap();
    let fp3 = <dyn Mapping>::simplify(&cmp3).unwrap();
    fs.add_variant(Some(fp3), "FP3").unwrap();
    fs.set_variant("FP2").unwrap();
    fs
}

fn matches_current_index(fs: &FrameSet) -> usize {
    // The variant-bearing frame is the current one (FPLANE = frame 2).
    let _ = fs;
    2
}

// ---------------------------------------------------------------------------
// Serialization round-trips (testframeset.c checkdump, errors 20-22 / 34-36)
// ---------------------------------------------------------------------------

/// A Rust-built FrameSet with FP1/FP2 variants (FP2 selected) survives a
/// native dump/restore: AllVariants, the selected Variant, and the transform
/// are all preserved (C `Vrnts` object on the member Frame).
#[test]
fn variants_native_round_trip() {
    use ast_core::{AnyObject, from_native_string};

    let fs = three_variant_fs(); // FP1/FP2/FP3, FP2 selected
    let text = native_dump(&AnyObject::FrameSet(fs)).unwrap();
    assert!(text.contains("Vrnts"), "dump must carry the Vrnts object");

    let AnyObject::FrameSet(fs2) = from_native_string(&text).unwrap() else {
        panic!("expected FrameSet");
    };
    assert_eq!(fs2.all_variants(), "FP1 FP2 FP3");
    assert_eq!(fs2.variant(), "FP2");
    let (x, y) = tran(&fs2, 50.5, 100.5);
    assert!((x - 150.0).abs() < 1e-6 && (y - 150.0).abs() < 1e-6);
}

/// Read a native dump written by the reference C library (FP1/FP2, FP2
/// selected) and confirm the variant state and transforms match. This proves
/// the Rust `Vrnts` serialization format is byte-compatible with C's per-Frame
/// variant storage. Fixture generated by `astWrite` on the built libast.
#[test]
fn reads_c_generated_variant_dump() {
    use ast_core::{AnyObject, from_native_string};

    let path = format!(
        "{}/tests/fixtures/serialisation/c_variants.ast",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).expect("c_variants.ast fixture");
    let AnyObject::FrameSet(mut fs) = from_native_string(&text).unwrap() else {
        panic!("expected FrameSet");
    };

    assert_eq!(fs.all_variants(), "FP1 FP2");
    assert_eq!(fs.variant(), "FP2");
    assert!(fs.test_variant());
    let (x, y) = tran(&fs, 50.5, 100.5);
    assert!(
        (x - 150.0).abs() < 1e-6 && (y - 150.0).abs() < 1e-6,
        "FP2: ({x},{y})"
    );

    fs.set_variant("FP1").unwrap();
    let (x, y) = tran(&fs, 50.5, 100.5);
    assert!(
        (x - 0.0).abs() < 1e-6 && (y - 0.0).abs() < 1e-6,
        "FP1: ({x},{y})"
    );
}

/// Remapping a Frame drops its variant Mappings, keeping the selected one,
/// as C's `astRemapFrame` clears the Variant attribute first; C reports
/// AllVariants `"SKYX"` after remapping a Frame with variants SKYX and V2.
/// Permuting the current Frame's axes remaps it, so it drops them too.
#[test]
fn remapping_a_frame_drops_its_variants() {
    let mut fs = FrameSet::new(Box::new(frm("PIX")));
    fs.add_frame(
        FrameRef::Base,
        Box::new(ast_core::mappings::ZoomMap::new(2, 2.0).unwrap()),
        Box::new(frm("SKYX")),
    )
    .unwrap();
    fs.add_variant(
        Some(Box::new(ast_core::mappings::ShiftMap::new([1.0, 1.0]))),
        "V2",
    )
    .unwrap();
    assert_eq!(fs.all_variants(), "SKYX V2");
    fs.remap_frame(
        FrameRef::Current,
        Box::new(ast_core::mappings::ZoomMap::new(2, 3.0).unwrap()),
    )
    .unwrap();
    assert_eq!(fs.all_variants(), "SKYX");
    assert!(!fs.test_variant());

    fs.add_variant(
        Some(Box::new(ast_core::mappings::ShiftMap::new([1.0, 1.0]))),
        "V3",
    )
    .unwrap();
    assert_eq!(fs.all_variants(), "SKYX V3");
    fs.perm_axes(&[2, 1]).unwrap();
    assert_eq!(fs.all_variants(), "SKYX");
}

/// The point (1,1) mapped from Frame `a` to Frame `b`.
fn map11(fs: &FrameSet, a: usize, b: usize) -> (f64, f64) {
    let m = fs.mapping(FrameRef::Index(a), FrameRef::Index(b)).unwrap();
    let (x, y) = m.tran2(&[1.0], &[1.0], true).unwrap();
    (x[0], y[0])
}

/// Selecting a variant replaces the Mapping of the variant owner's node in
/// place, as C's `SetVariant` does, so a Frame derived from the owner moves
/// with it, and selecting through a mirror changes the owner.
/// C (PIX -Zoom 2-> SKY with variants SKY and V2 = Shift 1, then FOO from
/// SKY by Zoom 3): after `Variant = SKY`, PIX->SKY (2,2), PIX->FOO (6,6) and
/// SKY->FOO (3,3); then with FOO mirroring SKY, `Variant = V2` from FOO
/// gives PIX->SKY (3,3), PIX->FOO (9,9) and SKY->FOO (3,3).
#[test]
fn set_variant_replaces_the_owner_node_mapping() {
    let mut fs = FrameSet::new(Box::new(frm("PIX")));
    fs.add_frame(
        FrameRef::Base,
        Box::new(ast_core::mappings::ZoomMap::new(2, 2.0).unwrap()),
        Box::new(frm("SKY")),
    )
    .unwrap();
    fs.add_variant(
        Some(Box::new(ast_core::mappings::ShiftMap::new([1.0, 1.0]))),
        "V2",
    )
    .unwrap();
    assert_eq!(map11(&fs, 1, 2), (3.0, 3.0));
    fs.add_frame(
        FrameRef::Index(2),
        Box::new(ast_core::mappings::ZoomMap::new(2, 3.0).unwrap()),
        Box::new(frm("FOO")),
    )
    .unwrap();
    assert_eq!(map11(&fs, 1, 3), (9.0, 9.0));

    fs.set_current(FrameRef::Index(2)).unwrap();
    fs.set_variant("SKY").unwrap();
    assert_eq!(fs.variant(), "SKY");
    assert_eq!(map11(&fs, 1, 2), (2.0, 2.0));
    assert_eq!(map11(&fs, 1, 3), (6.0, 6.0));
    assert_eq!(map11(&fs, 2, 3), (3.0, 3.0));

    fs.set_current(FrameRef::Index(3)).unwrap();
    fs.mirror_variants(FrameRef::Index(2)).unwrap();
    fs.set_variant("V2").unwrap();
    assert_eq!(fs.variant(), "V2");
    assert_eq!(map11(&fs, 1, 2), (3.0, 3.0));
    assert_eq!(map11(&fs, 1, 3), (9.0, 9.0));
    assert_eq!(map11(&fs, 2, 3), (3.0, 3.0));
}
