//! Compilation test: verifies trait signatures are coherent and Region: Frame + Mapping works.

use ast_core::coords::PointSet;
use ast_core::error::{AstError, AstResult, ErrorCode};
use ast_core::frames::{DynFrame, Frame};
use ast_core::mappings::{DynMapping, Mapping};
use ast_core::regions::{DynRegion, Overlap, Region};

/// A dummy type that implements all three traits, proving they're coherent.
struct DummyRegion;

impl Mapping for DummyRegion {
    fn nin(&self) -> usize { 2 }
    fn nout(&self) -> usize { 2 }
    fn is_inverted(&self) -> bool { false }
    fn invert(&mut self) {}
    fn transform(&self, _pts: &PointSet, _forward: bool) -> AstResult<PointSet> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn transform_into(&self, _pts: &PointSet, _forward: bool, _out: &mut PointSet) -> AstResult<()> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn simplified(&self) -> AstResult<DynMapping> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn rate(&self, _at: &[f64], _ax1: usize, _ax2: usize) -> AstResult<f64> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
}

impl Frame for DummyRegion {
    fn naxes(&self) -> usize { 2 }
    fn format(&self, _axis: usize, _value: f64) -> AstResult<String> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn unformat(&self, _axis: usize, _text: &str) -> AstResult<(f64, usize)> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn distance(&self, _a: &[f64], _b: &[f64]) -> AstResult<f64> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn angle(&self, _a: &[f64], _b: &[f64], _c: &[f64]) -> AstResult<f64> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn offset(&self, _from: &[f64], _to: &[f64], _dist: f64) -> AstResult<Vec<f64>> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn norm(&self, _coords: &mut [f64]) -> AstResult<()> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn snapshot(&self) -> DynFrame {
        panic!("not implemented")
    }
    fn map_from(&self, _previous: &dyn Frame) -> AstResult<DynMapping> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
}

impl Region for DummyRegion {
    fn point_in_region(&self, _point: &[f64]) -> AstResult<bool> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn overlap(&self, _other: &dyn Region) -> AstResult<Overlap> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn negated(&self) -> DynRegion {
        panic!("not implemented")
    }
}

#[test]
fn dummy_region_implements_all_traits() {
    let r = DummyRegion;
    assert_eq!(r.nin(), 2);
    assert_eq!(r.naxes(), 2);
    assert!(r.point_in_region(&[0.0, 0.0]).is_err());
}

#[test]
fn region_is_object_safe() {
    let _: Box<dyn Region> = Box::new(DummyRegion);
    let _: Box<dyn Frame> = Box::new(DummyRegion);
    let _: Box<dyn Mapping> = Box::new(DummyRegion);
}

#[test]
fn dyn_types_are_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Box<dyn Mapping>>();
    assert_send::<Box<dyn Frame>>();
    assert_send::<Box<dyn Region>>();
}
