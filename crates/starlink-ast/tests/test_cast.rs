// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Casting a Frame into the class of another, and the conversions that
//! cast the template when it does not match.
//!
//! Ported from `reference/starlink-ast/ast_tester/testcast.c`.
//! Divergences from the C:
//!
//! - The Plot3D cases are left out, as Plot3D is not ported.
//! - C reads the class of the cast Frame from its Class attribute; here it is
//!   found with `downcast_ref`.

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::frames::FluxFrame;
use starlink_ast::frames::{
    BasicFrame, BasicFrameBuilder, CmpFrame, Frame, FrameSearch, FrameSet, SkyFrame, SpecFluxFrame,
    SpecFrame,
};
use starlink_ast::plot::Plot;
use starlink_ast::regions::{
    BoxRegionBuilder, Circle, CircleBuilder, StcObsDataLocation, StcObsDataLocationBuilder,
    StcSearchLocationBuilder,
};

pub(crate) fn frame(naxes: usize, domain: &str) -> BasicFrame {
    let mut f = BasicFrameBuilder::new(naxes).build().unwrap();
    if !domain.is_empty() {
        Frame::set_domain(&mut f, domain).unwrap();
    }
    f
}

/// A SpecFluxFrame whose SpecFrame has `domain`, or the default one.
pub(crate) fn spec_flux(domain: &str) -> SpecFluxFrame {
    let mut spec = SpecFrame::unset_system();
    if !domain.is_empty() {
        Frame::set_domain(&mut spec, domain).unwrap();
    }
    let flux = FluxFrame::with_spec_val(BAD_VALUE, Some(SpecFrame::unset_system()));
    SpecFluxFrame::new(spec, flux)
}

/// Check that converting `from` to `to` finds no solution and reports no
/// error.
pub(crate) fn check_no_conversion(what: &str, from: &dyn Frame, to: &dyn Frame) {
    let mut from = from.snapshot();
    let mut to = to.snapshot();
    match from.convert_to(to.as_mut(), &[]) {
        Ok(None) => {}
        Ok(Some(_)) => panic!("a conversion was found {what}"),
        Err(e) => panic!("converting {what} gave {e}"),
    }
}

fn unit_circle(frame: impl Frame + 'static) -> Circle {
    CircleBuilder::new(frame, vec![0.0, 0.0])
        .unwrap()
        .radius(1.0)
        .build()
        .unwrap()
}

/// A plain CmpFrame that has a Domain, and components that have Domains.
pub(crate) fn cmp_abc() -> CmpFrame {
    let mut cmp = CmpFrame::new(Box::new(frame(1, "A")), Box::new(frame(1, "B"))).unwrap();
    cmp.set_domain("C");
    cmp
}

#[test]
fn conversions() {
    // Two SpecFluxFrames whose SpecFrames have different Domains.
    let (sff1, sff2) = (spec_flux("FOO"), spec_flux(""));
    check_no_conversion(
        "between SpecFluxFrames with different Domains",
        &sff1,
        &sff2,
    );
    check_no_conversion(
        "between SpecFluxFrames with different Domains, reversed",
        &sff2,
        &sff1,
    );

    // A CmpFrame that holds a Region, and one that holds a Frame with
    // another Domain in its place.
    let cmp1 = CmpFrame::new(
        Box::new(unit_circle(frame(2, "A"))),
        Box::new(SpecFrame::unset_system()),
    )
    .unwrap();
    let cmp2 = CmpFrame::new(Box::new(frame(2, "B")), Box::new(SpecFrame::unset_system())).unwrap();
    check_no_conversion("from a CmpFrame that holds a Region", &cmp1, &cmp2);
    check_no_conversion("to a CmpFrame that holds a Region", &cmp2, &cmp1);

    // The same, for a Region in a sky coordinate system.
    let cmp1 = CmpFrame::new(
        Box::new(unit_circle(SkyFrame::unset_system())),
        Box::new(SpecFrame::unset_system()),
    )
    .unwrap();
    check_no_conversion("from a CmpFrame that holds a sky Region", &cmp1, &cmp2);
    check_no_conversion("to a CmpFrame that holds a sky Region", &cmp2, &cmp1);

    // A plain CmpFrame does not match a SpecFluxFrame.
    let sff2 = spec_flux("");
    check_no_conversion("from a CmpFrame to a SpecFluxFrame", &cmp_abc(), &sff2);
    check_no_conversion("from a SpecFluxFrame to a CmpFrame", &sff2, &cmp_abc());

    // Nor does a CmpFrame of three axes that holds a CmpFrame.
    let nested = CmpFrame::new(
        Box::new(
            CmpFrame::new(
                Box::new(SkyFrame::unset_system()),
                Box::new(SpecFrame::unset_system()),
            )
            .unwrap(),
        ),
        Box::new(frame(1, "B")),
    )
    .unwrap();
    check_no_conversion(
        "from a SpecFluxFrame to a CmpFrame that holds a CmpFrame",
        &sff2,
        &nested,
    );
    check_no_conversion(
        "from a CmpFrame that holds a CmpFrame to a SpecFluxFrame",
        &nested,
        &sff2,
    );
}

/// Check that casting `this` into the class of `obj` gives nothing.
fn check_no_cast(what: &str, this: &dyn Frame, obj: &dyn Frame) {
    let cast = this
        .cast(obj)
        .unwrap_or_else(|e| panic!("casting {what} gave {e}"));
    assert!(cast.is_none(), "casting {what} gave a Frame");
}

/// Check that casting `this` into the class of `obj` gives a `T`.
pub(crate) fn check_cast<T: 'static>(what: &str, this: &dyn Frame, obj: &dyn Frame) {
    let cast = this
        .cast(obj)
        .unwrap_or_else(|e| panic!("casting {what} gave {e}"))
        .unwrap_or_else(|| panic!("casting {what} gave nothing"));
    assert!(
        cast.as_ref().as_any().downcast_ref::<T>().is_some(),
        "casting {what} gave another class"
    );
}

#[test]
fn casts() {
    let frame_a = frame(2, "A");
    let circle = unit_circle(frame_a.clone());
    let bx = BoxRegionBuilder::new(frame_a.clone(), vec![0.0, 0.0], vec![1.0, 1.0])
        .unwrap()
        .build()
        .unwrap();
    let search = StcSearchLocationBuilder::new(circle.clone())
        .build()
        .unwrap();
    let obsdata = StcObsDataLocationBuilder::new(circle.clone())
        .build()
        .unwrap();
    let fset = FrameSet::new(Box::new(frame_a.clone()));
    let plot = Plot::new(&frame_a, [0.0, 0.0, 1.0, 1.0], [0.0, 0.0, 1.0, 1.0]).unwrap();

    // A Frame cannot be cast into a class that it is not an instance of.
    check_no_cast(
        "an StcSearchLocation into an StcObsDataLocation",
        &search,
        &obsdata,
    );
    check_no_cast(
        "an StcObsDataLocation into an StcSearchLocation",
        &obsdata,
        &search,
    );
    check_no_cast("a Circle into a Box", &circle, &bx);
    check_no_cast("a Circle into an StcSearchLocation", &circle, &search);
    check_no_cast(
        "a CmpFrame into a SpecFluxFrame",
        &cmp_abc(),
        &spec_flux(""),
    );
    check_no_cast("a FrameSet into a Plot", &fset, &plot);
    check_no_cast(
        "a Circle into a SkyFrame",
        &circle,
        &SkyFrame::unset_system(),
    );

    // It can be cast into its own class and into the classes that its own
    // is derived from. A Region or FrameSet is cast into a class of Frame
    // that is not a Region or FrameSet by casting its current Frame.
    let sky_circle = unit_circle(SkyFrame::unset_system());
    check_cast::<StcObsDataLocation>(
        "an StcObsDataLocation into its own class",
        &obsdata,
        &obsdata.clone(),
    );
    check_cast::<Circle>("a Circle into its own class", &circle, &sky_circle);
    check_cast::<CmpFrame>(
        "a SpecFluxFrame into a CmpFrame",
        &spec_flux(""),
        &cmp_abc(),
    );
    check_cast::<FrameSet>("a Plot into a FrameSet", &plot, &fset);
    check_cast::<BasicFrame>("a Circle into a Frame", &circle, &frame_a);
    check_cast::<SkyFrame>(
        "a sky Circle into a SkyFrame",
        &sky_circle,
        &SkyFrame::unset_system(),
    );
    check_cast::<BasicFrame>("a CmpFrame into a Frame", &cmp_abc(), &frame_a);
    check_cast::<BasicFrame>("a FrameSet into a Frame", &fset, &frame_a);
}
