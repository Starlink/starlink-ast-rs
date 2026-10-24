//! Mappings refuse the arguments C's constructors and `Add` methods refuse,
//! with C's error codes.

use starlink_ast::error::{AstResult, ErrorCode};
use starlink_ast::mappings::{
    DynMapping, Mapping, MathMap, PolyMap, PolyTerm, RateMap, SlaCvtType, SlaMap, SpecCvtType,
    SpecMap, SplineMap, SwitchMap, TimeCvtType, TimeMap, TranMap, UnitMap, UnitNormMap, WcsMap,
    WcsProjection, ZoomMap,
};

fn code<T>(r: AstResult<T>) -> ErrorCode {
    match r {
        Ok(_) => panic!("expected an error"),
        Err(e) => e.code(),
    }
}

/// A Mapping with an inverse but no forward transformation.
fn inverse_only() -> DynMapping {
    let mut rate = RateMap::new(Box::new(ZoomMap::new(1, 2.0).unwrap()), 1, 1).unwrap();
    rate.invert();
    assert!(!rate.has_forward() && rate.has_inverse());
    Box::new(rate)
}

/// A Mapping with a forward but no inverse transformation.
fn forward_only() -> DynMapping {
    let rate = RateMap::new(Box::new(ZoomMap::new(1, 2.0).unwrap()), 1, 1).unwrap();
    assert!(rate.has_forward() && !rate.has_inverse());
    Box::new(rate)
}

/// tranmap.c astInitTranMap: the first Mapping supplies the forward
/// transformation and the second the inverse.
#[test]
fn a_tranmap_needs_its_two_directions() {
    let unit = || -> DynMapping { Box::new(UnitMap::new(1)) };
    assert_eq!(code(TranMap::new(inverse_only(), unit())), ErrorCode::Intrd);
    assert_eq!(code(TranMap::new(unit(), forward_only())), ErrorCode::Intrd);
    assert!(TranMap::new(forward_only(), inverse_only()).is_ok());
}

/// switchmap.c astInitSwitchMap: the forward selector needs a forward
/// transformation and the inverse selector an inverse one.
#[test]
fn a_switchmap_selector_needs_its_direction() {
    let routes = || -> Vec<DynMapping> { vec![Box::new(ZoomMap::new(1, 2.0).unwrap())] };
    assert_eq!(
        code(SwitchMap::new(Some(inverse_only()), None, routes())),
        ErrorCode::Intrd
    );
    assert_eq!(
        code(SwitchMap::new(None, Some(forward_only()), routes())),
        ErrorCode::Intrd
    );
}

/// slamap.c, specmap.c and timemap.c: each conversion takes exactly its
/// number of arguments, or it is AST__TIMIN.
#[test]
fn a_conversion_takes_exactly_its_arguments() {
    let mut sla = SlaMap::new();
    assert_eq!(code(sla.add(SlaCvtType::Prec, &[1950.0])), ErrorCode::Timin);
    assert_eq!(
        code(sla.add(SlaCvtType::Prec, &[1950.0, 2000.0, 1.0])),
        ErrorCode::Timin
    );
    assert!(sla.add(SlaCvtType::Prec, &[1950.0, 2000.0]).is_ok());

    let mut spec = SpecMap::new();
    assert_eq!(code(spec.add(SpecCvtType::FrToVl, &[])), ErrorCode::Timin);
    assert!(spec.add(SpecCvtType::FrToVl, &[1.0e11]).is_ok());

    let mut time = TimeMap::new();
    assert_eq!(
        code(time.add(TimeCvtType::MjdToMjd, &[0.0])),
        ErrorCode::Timin
    );
    assert!(time.add(TimeCvtType::MjdToMjd, &[0.0, 1.0]).is_ok());
    assert_eq!(sla.len() + spec.len() + time.len(), 3);
}

/// splinemap.c astInitSplineMap: bad orders, too few coefficients and
/// knots that decrease are AST__ATTIN.
#[test]
fn a_splinemap_checks_its_arguments() {
    let knots = || vec![0.0, 0.0, 1.0, 1.0];
    let coeffs = || vec![0.0; 4];
    assert!(SplineMap::new(2, 2, 2, 2, knots(), knots(), coeffs(), coeffs()).is_ok());
    assert_eq!(
        code(SplineMap::new(
            0,
            2,
            2,
            2,
            knots(),
            knots(),
            coeffs(),
            coeffs()
        )),
        ErrorCode::Attin
    );
    assert_eq!(
        code(SplineMap::new(
            2,
            2,
            1,
            2,
            vec![0.0, 0.0, 1.0],
            knots(),
            vec![0.0; 2],
            vec![0.0; 2]
        )),
        ErrorCode::Attin
    );
    assert_eq!(
        code(SplineMap::new(
            2,
            2,
            2,
            2,
            vec![0.0, 1.0, 0.5, 1.0],
            knots(),
            coeffs(),
            coeffs()
        )),
        ErrorCode::Attin
    );
}

/// unitnormmap.c astInitUnitNormMap: the center needs at least one axis.
#[test]
fn a_unitnormmap_needs_a_centre() {
    assert_eq!(code(UnitNormMap::new(Vec::<f64>::new())), ErrorCode::Badsm);
}

/// selectormap.c astInitSelectorMap: every Region must refer to the same
/// coordinate Frame as the first (AST__BADNI).
#[test]
fn a_selectormap_needs_regions_in_one_frame() {
    use starlink_ast::frames::{BasicFrameBuilder, SkyFrame, SkySystem};
    use starlink_ast::mappings::SelectorMapBuilder;
    use starlink_ast::regions::{CircleBuilder, DynRegion};

    let plain = || -> DynRegion {
        Box::new(
            CircleBuilder::new(BasicFrameBuilder::new(2).build().unwrap(), vec![0.0, 0.0])
                .unwrap()
                .radius(1.0)
                .build()
                .unwrap(),
        )
    };
    let sky: DynRegion = Box::new(
        CircleBuilder::new(SkyFrame::new(SkySystem::Icrs), vec![0.0, 0.0])
            .unwrap()
            .radius(0.1)
            .build()
            .unwrap(),
    );
    assert!(SelectorMapBuilder::new(vec![plain(), plain()]).is_ok());
    assert_eq!(
        code(SelectorMapBuilder::new(vec![plain(), sky])),
        ErrorCode::Badni
    );
}

/// polymap.c StoreArrays: a term for an output that does not exist is
/// AST__BADCI.
#[test]
fn a_polymap_term_needs_a_real_output() {
    let term = |out_axis| PolyTerm {
        coeff: 1.0,
        powers: vec![1],
        out_axis,
    };
    for out_axis in [0, 2] {
        assert_eq!(
            code(PolyMap::new(1, 1, &[term(out_axis)], None)),
            ErrorCode::Badci
        );
        assert_eq!(
            code(PolyMap::new(1, 1, &[term(1)], Some(&[term(out_axis)]))),
            ErrorCode::Badci
        );
    }
}

/// wcsmap.c SetPV: an axis out of range, or one that is not celestial, is
/// AST__AXIIN.
#[test]
fn a_wcsmap_pv_needs_a_celestial_axis() {
    let mut wcs = WcsMap::new(3, WcsProjection::Tan, 1, 2).unwrap();
    for axis in [0, 3, 4] {
        assert_eq!(
            code(wcs.set_pv(axis, 1, 0.0)),
            ErrorCode::Axiin,
            "axis {axis}"
        );
    }
}

/// mathmap.c astInitMathMap: too few functions in either direction is
/// AST__INNTF.
#[test]
fn a_mathmap_needs_a_function_per_output() {
    assert_eq!(
        code(MathMap::new(1, 2, &["y=x"], &["x=y"])),
        ErrorCode::Inntf
    );
    assert_eq!(
        code(MathMap::new(2, 1, &["y=x1"], &["x1=y"])),
        ErrorCode::Inntf
    );
}
