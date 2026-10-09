//! The Moc class core against the C library (moc.c): Mocs built cell by cell
//! with `add_cell` (OR, XOR and AND, at several orders), one whose MaxOrder
//! needs 8 byte values, and an empty one; for each, its dump and that dump
//! read back, MocLength, MocType, MocArea and normalised cells, the
//! membership of a grid of sky positions (plain, negated, and with System set
//! to Galactic through the Moc), its boundary mesh and bounding box;
//! `test_cell`; the overlap of a Moc with a Circle each way round; the mask
//! `reg_pins_mask` gives for boundary points supplied out of Dec order; and
//! the errors for an invalid cell and for setting MaxOrder twice.
//!
//! The values come from `tools/oracle_fixtures/region_moc_probe.c`, run
//! against the library with moc.c's `RegBaseBox`, `RegPins`, `Comp_corner`
//! and `Transform` fixed upstream (2b631e08, c95aa801).

// Generated; formatted by its generator rather than rustfmt. Its values are
// C's to 17 significant digits, which is more than some of them need, and
// some are pi/2 to the last bit.
#[rustfmt::skip]
#[allow(clippy::excessive_precision, clippy::approx_constant)]
#[path = "region_moc_oracle/cases.rs"]
mod cases;

use ast_core::coords::PointSet;
use ast_core::error::ErrorCode;
use ast_core::frames::{SkyFrame, SkySystem};
use ast_core::mappings::Mapping;
use ast_core::regions::{CircleBuilder, CmpOperator, DynRegion, Moc, Overlap, Region};
use ast_core::{AnyObject, from_native_string, to_native_string};

fn low() -> Moc {
    let mut moc = Moc::new();
    for (cmode, order, npix) in [
        (CmpOperator::Or, 2, 37),
        (CmpOperator::Or, 1, 3),
        (CmpOperator::Or, 2, 100),
        (CmpOperator::Xor, 1, 25),
        (CmpOperator::Xor, 2, 101),
        (CmpOperator::And, 2, 13),
        (CmpOperator::Or, 0, 7),
        (CmpOperator::And, 2, 30),
    ] {
        moc.add_cell(cmode, order, npix).unwrap();
    }
    moc
}

fn built(name: &str) -> Moc {
    match name {
        "low" => low(),
        "high" => {
            let mut moc = Moc::new();
            moc.add_cell(CmpOperator::Or, 15, 1234567890).unwrap();
            moc.add_cell(CmpOperator::Or, 10, 5).unwrap();
            moc.add_cell(CmpOperator::Or, 3, 700).unwrap();
            moc
        }
        "galactic" => {
            let mut moc = low().copy_region();
            let moc_ref = moc
                .as_any_mut()
                .downcast_mut::<Moc>()
                .expect("a copy of a Moc is a Moc");
            moc_ref
                .with_integrity(|f| {
                    f.as_any_mut()
                        .downcast_mut::<SkyFrame>()
                        .expect("a Moc's Frame is a SkyFrame")
                        .set_system(SkySystem::Galactic);
                    Ok(())
                })
                .unwrap();
            moc_ref.clone()
        }
        "empty" => Moc::new(),
        other => panic!("no case {other}"),
    }
}

fn dump(region: &dyn Region) -> String {
    to_native_string(&AnyObject::from_mapping(region as &dyn Mapping).unwrap()).unwrap()
}

fn grid() -> PointSet {
    PointSet::from_rows(cases::GRID.iter().map(|p| p.as_slice()))
}

fn assert_points(got: &PointSet, want: &[[f64; 2]], what: &str) {
    assert_eq!(got.npoint(), want.len(), "{what}: count");
    for (i, w) in want.iter().enumerate() {
        assert_eq!(got.point(i), w.as_slice(), "{what}: point {i}");
    }
}

#[test]
fn mocs_match_c() {
    for case in cases::CASES {
        let (name, want_dump, _, length, moc_type, area, cells, plain, negated, mesh, bounds) =
            *case;
        let moc = built(name);
        assert_eq!(dump(&moc), want_dump, "{name}: dump");
        assert_eq!(
            moc.moc_length().unwrap() as i32,
            length,
            "{name}: MocLength"
        );
        assert_eq!(moc.moc_type().unwrap(), moc_type, "{name}: MocType");
        assert_eq!(moc.moc_area(), area, "{name}: MocArea");
        let got_cells: Vec<(i32, i64)> = (0..length).map(|i| moc.get_cell(i).unwrap()).collect();
        assert_eq!(got_cells, cells, "{name}: cells");

        assert_points(
            &moc.transform(&grid(), true).unwrap(),
            plain,
            &format!("{name}: grid"),
        );
        let neg = moc.negated_copy();
        assert_points(
            &neg.transform(&grid(), true).unwrap(),
            negated,
            &format!("{name}: negated grid"),
        );
        assert_points(&moc.region_mesh().unwrap(), mesh, &format!("{name}: mesh"));
        let (lbnd, ubnd) = moc.get_region_bounds().unwrap();
        assert_eq!(
            [lbnd[0], lbnd[1], ubnd[0], ubnd[1]],
            bounds,
            "{name}: bounds"
        );
    }
}

#[test]
fn c_dumps_read_back_as_c_reads_them() {
    for case in cases::CASES {
        let (name, first, want, ..) = *case;
        let region = from_native_string(first).unwrap().into_region().unwrap();
        assert_eq!(dump(region.as_ref()), want, "{name}");
    }
}

#[test]
fn test_cell_matches_c() {
    let moc = low();
    for &(order, npix, parent, want) in cases::TEST_CELLS {
        assert_eq!(
            moc.test_cell(order, npix, parent).unwrap(),
            want,
            "({order}, {npix}, {parent})"
        );
    }
}

fn code(overlap: Overlap) -> i32 {
    match overlap {
        Overlap::Incomparable => 0,
        Overlap::None => 1,
        Overlap::FirstInside => 2,
        Overlap::SecondInside => 3,
        Overlap::Partial => 4,
        Overlap::Same => 5,
        Overlap::Negation => 6,
    }
}

#[test]
fn overlap_with_a_circle_matches_c() {
    let moc = low();
    let circle: DynRegion = Box::new(
        CircleBuilder::new(SkyFrame::new(SkySystem::Icrs), vec![1.2, 0.3])
            .unwrap()
            .radius(0.6)
            .build()
            .unwrap(),
    );
    assert_eq!(
        code(moc.overlap(circle.as_ref()).unwrap()),
        cases::OVERLAP_MOC_CIRCLE
    );
    assert_eq!(
        code(circle.overlap(&moc).unwrap()),
        cases::OVERLAP_CIRCLE_MOC
    );
}

#[test]
fn reg_pins_flags_each_point_in_place() {
    let moc = low();
    let mesh = moc.region_mesh().unwrap();
    let n = mesh.npoint();
    let mut rows: Vec<Vec<f64>> = (0..n).rev().map(|i| mesh.point(i).to_vec()).collect();
    rows.push(vec![3.0, -1.4]);
    let pins = PointSet::from_rows(rows.iter().map(|r| r.as_slice()));
    let (result, mask) = moc.reg_pins_mask(&pins, None).unwrap();
    assert_eq!(result, cases::PINS_RESULT);
    assert_eq!(mask, cases::PINS_MASK);
}

#[test]
fn errors_match_c() {
    let mut moc = low();
    let err = moc.add_cell(CmpOperator::Or, 3, 5).unwrap_err();
    assert_eq!(err.code() as i32, cases::CELL_ORDER);
    let err = moc.set_max_order(3).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Nowrt);
    assert_eq!(err.code() as i32, cases::MAX_ORDER_SET);
}
