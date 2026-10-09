//! The public `map_region` of an Interval with a missing limit, against the
//! C library's `astMapRegion`, which simplifies the mapped Interval and so
//! re-defines it in the new Frame (interval.c `Simplify`): through a ZoomMap,
//! a PermMap swapping the axes, and a WinMap reversing an axis that is
//! excluded rather than included.
//!
//! The values come from `tools/oracle_fixtures/region_interval_remap_probe.c`.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_interval_remap_oracle/cases.rs"]
mod cases;

use ast_core::frames::{BasicFrameBuilder, Frame};
use ast_core::mappings::Mapping;
use ast_core::{AnyObject, from_native_string, to_native_string};

#[test]
fn mapped_interval_is_redefined_in_the_new_frame() {
    for &(name, region, mapping, want) in cases::CASES {
        let region = from_native_string(region).unwrap().into_region().unwrap();
        let mapping = from_native_string(mapping).unwrap().into_mapping().unwrap();
        let mut frame = BasicFrameBuilder::new(2).build().unwrap();
        Frame::set_domain(&mut frame, "NEW").unwrap();
        let mapped = region.map_region(mapping.as_ref(), &frame).unwrap();
        let got =
            to_native_string(&AnyObject::from_mapping(mapped.as_ref() as &dyn Mapping).unwrap())
                .unwrap();
        assert_eq!(got, want, "{name}");
    }
}
