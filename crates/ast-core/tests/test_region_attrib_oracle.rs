//! Frame attributes set and cleared through a Region, against the C
//! library (region.c `SetAttrib` and `ClearAttrib`).
//!
//! An Adaptive Region is remapped into its changed current Frame; one that
//! is not has the change made to its current and base Frames alike, on the
//! base-Frame axis the current-Frame axis is connected to, and to its
//! uncertainty (`RegSetAttrib`, `RegClearAttrib`), and a CmpRegion or Prism
//! makes it to its components too. Each case is compared with C's dump, byte
//! for byte, and with whether C finds (0.5, 0.5) inside.
//!
//! The dumps come from `tools/oracle_fixtures/region_attrib_probe.c`, which
//! makes the same changes. C lower-cased the value of a setting made to a
//! Region that is not Adaptive; fixed upstream (bb287a61), which the probe
//! needs.

use crate::common::native_dump;
// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_attrib_oracle/cases.rs"]
mod cases;

use ast_core::AnyObject;
use ast_core::frames::{BasicFrameBuilder, Frame};
use ast_core::mappings::Mapping;
use ast_core::regions::{
    BoxRegionBuilder, CmpOperator, CmpRegionBuilder, DynRegion, IntervalBuilder, PrismBuilder,
    Region,
};

/// The unit square in a Frame with axes in meters and ActiveUnit set.
fn metre_box() -> DynRegion {
    let mut frame = BasicFrameBuilder::new(2).build().unwrap();
    frame.set_unit(1, "m").unwrap();
    frame.set_unit(2, "m").unwrap();
    frame.set_active_unit(true);
    Box::new(
        BoxRegionBuilder::from_bounds(frame, &[0.0, 0.0], &[1.0, 1.0])
            .unwrap()
            .build()
            .unwrap(),
    )
}

fn unit_box(lbnd: [f64; 2], ubnd: [f64; 2], unc: Option<DynRegion>) -> DynRegion {
    let frame = BasicFrameBuilder::new(2).build().unwrap();
    let mut builder = BoxRegionBuilder::from_bounds(frame, &lbnd, &ubnd).unwrap();
    if let Some(unc) = unc {
        builder = builder.uncertainty(unc);
    }
    Box::new(builder.build().unwrap())
}

/// The unit Box [0, 1] x [0, 2] with its current-Frame axes swapped.
fn permuted_box() -> DynRegion {
    let mut reg = unit_box([0.0, 0.0], [1.0, 2.0], None);
    reg.perm_axes(&[2, 1]).unwrap();
    reg
}

fn region(name: &str) -> DynRegion {
    match name {
        "unit_km_adaptive" => {
            let mut reg = metre_box();
            reg.set_adaptive(true);
            reg.set_unit(1, "km").unwrap();
            reg
        }
        "unit_km_not_adaptive" => {
            let mut reg = metre_box();
            reg.set_adaptive(false);
            reg.set_unit(1, "km").unwrap();
            reg
        }
        "unit_cleared_not_adaptive" => {
            let mut reg = metre_box();
            reg.set_adaptive(false);
            reg.set_unit(1, "km").unwrap();
            reg.clear_unit(1).unwrap();
            reg
        }
        "permuted_box" => permuted_box(),
        "label_after_permaxes" => {
            let mut reg = permuted_box();
            reg.set_adaptive(false);
            reg.set_label(1, "Swapped").unwrap();
            reg
        }
        "label_and_title_with_uncertainty" => {
            let unc = unit_box([0.0, 0.0], [0.01, 0.01], None);
            let mut reg = unit_box([0.0, 0.0], [1.0, 1.0], Some(unc));
            reg.set_adaptive(false);
            reg.set_label(2, "Second").unwrap();
            reg.set_title("Titled").unwrap();
            reg
        }
        "label_cmpregion" => {
            let mut reg: DynRegion = Box::new(
                CmpRegionBuilder::from_dyn(
                    unit_box([0.0, 0.0], [1.0, 1.0], None),
                    unit_box([0.5, 0.5], [2.0, 2.0], None),
                    CmpOperator::Or,
                )
                .unwrap()
                .build()
                .unwrap(),
            );
            reg.set_adaptive(false);
            reg.set_label(1, "Joined").unwrap();
            reg
        }
        "labels_prism" => {
            let interval = IntervalBuilder::new(
                BasicFrameBuilder::new(1).build().unwrap(),
                vec![0.0],
                vec![1.0],
            )
            .unwrap()
            .build()
            .unwrap();
            let mut reg: DynRegion = Box::new(
                PrismBuilder::from_dyn(unit_box([0.0, 0.0], [1.0, 1.0], None), Box::new(interval))
                    .unwrap()
                    .build()
                    .unwrap(),
            );
            reg.set_adaptive(false);
            reg.set_label(3, "Height").unwrap();
            reg.set_symbol(1, "X").unwrap();
            reg
        }
        other => panic!("no Rust construction for case {other}"),
    }
}

/// Whether (0.5, 0.5) is inside a 2-D Region: 1, 0, or -1 when not 2-D.
fn inside(reg: &dyn Region) -> i32 {
    if reg.naxes() != 2 {
        return -1;
    }
    let (x, _) = reg.tran2(&[0.5], &[0.5], true).unwrap();
    i32::from(x[0] != ast_core::constants::BAD_VALUE)
}

#[test]
fn settings_match_the_c_library() {
    for &(name, want_inside, want_dump) in cases::CASES {
        let reg = region(name);
        assert_eq!(inside(reg.as_ref()), want_inside, "{name}: (0.5, 0.5)");
        let dump =
            native_dump(&AnyObject::from_mapping(reg.as_ref() as &dyn Mapping).unwrap()).unwrap();
        assert_eq!(dump, want_dump, "{name}: dump");
    }
}
