//! Compilation test: verifies trait signatures are coherent and Region: Frame + Mapping works.

use ast_core::coords::PointSet;
use ast_core::error::{AstError, AstResult, ErrorCode};
use ast_core::frames::{DynFrame, Frame};
use ast_core::mappings::{DynMapping, Mapping, ViaSimplify};
use ast_core::regions::{DynRegion, Overlap, Region};

/// A dummy type that implements all three traits, proving they're coherent.
#[derive(Debug)]
struct DummyRegion;

impl Mapping for DummyRegion {
    fn nin(&self) -> usize {
        2
    }
    fn nout(&self) -> usize {
        2
    }
    fn is_inverted(&self) -> bool {
        false
    }
    fn invert(&mut self) {}
    fn transform(&self, _pts: &PointSet, _forward: bool) -> AstResult<PointSet> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn transform_into(
        &self,
        _pts: &PointSet,
        _forward: bool,
        _out: &mut PointSet,
    ) -> AstResult<()> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn simplified(&self, _gate: ViaSimplify) -> AstResult<(DynMapping, bool)> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn rate(&self, _at: &[f64], _ax1: usize, _ax2: usize) -> AstResult<f64> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn equal(&self, _other: &dyn Mapping) -> bool {
        false
    }
    fn clone_box(&self) -> DynMapping {
        panic!("not implemented")
    }
}

impl Frame for DummyRegion {
    fn naxes(&self) -> usize {
        2
    }
    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        (self, axis)
    }
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
    fn convert_to(
        &self,
        _to: &dyn Frame,
        _domains: &[&str],
    ) -> AstResult<Option<ast_core::frames::FrameSet>> {
        Ok(None)
    }
    fn equal(&self, _other: &dyn Frame) -> bool {
        false
    }
}

impl Region for DummyRegion {
    fn point_in_region(&self, _point: &[f64]) -> AstResult<bool> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn overlap(&self, _other: &dyn Region) -> AstResult<Overlap> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn negated_copy(&self) -> DynRegion {
        panic!("not implemented")
    }
    fn negate(&mut self) {}
    fn is_negated(&self) -> bool {
        false
    }
    fn is_closed(&self) -> bool {
        true
    }
    fn set_closed(&mut self, _closed: bool) {}
    fn test_closed(&self) -> bool {
        false
    }
    fn set_closed_raw(&mut self, _closed: bool, _set: bool) {}
    fn fill_factor(&self) -> f64 {
        1.0
    }
    fn set_fill_factor(&mut self, _factor: f64) {}
    fn test_fill_factor(&self) -> bool {
        false
    }
    fn clear_fill_factor(&mut self) {}
    fn mesh_size(&self) -> usize {
        200
    }
    fn set_mesh_size(&mut self, _size: usize) {}
    fn test_mesh_size(&self) -> bool {
        false
    }
    fn clear_mesh_size(&mut self) {}
    fn is_bounded(&self) -> bool {
        true
    }
    fn is_adaptive(&self) -> bool {
        true
    }
    fn set_adaptive(&mut self, _adaptive: bool) {}
    fn has_dummy_fs(&self) -> bool {
        false
    }
    fn set_region_fs(&mut self, _frame: &dyn Frame) {}
    fn get_region_bounds(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn get_region_disc(&self) -> AstResult<(Vec<f64>, f64)> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn get_region_mesh(&self) -> AstResult<PointSet> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn get_region_grid(&self, _npoints: usize) -> AstResult<Vec<Vec<f64>>> {
        Ok(Vec::new())
    }
    fn get_region_points(&self) -> AstResult<PointSet> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn get_region_frame(&self) -> DynFrame {
        panic!("not implemented")
    }
    fn get_region_frameset(&self) -> ast_core::frames::FrameSet {
        panic!("not implemented")
    }
    fn region_frameset_mut(&mut self) -> &mut ast_core::frames::FrameSet {
        panic!("not implemented")
    }
    fn reg_perm_axes(&mut self, _perm: &[usize]) -> AstResult<()> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn store_uncertainty(&mut self, _unc: Option<DynRegion>) {}
    fn set_region_fs_flag(&mut self, _value: bool) {}
    fn region_fs_flag(&self) -> bool {
        false
    }
    fn get_uncertainty(&self) -> Option<DynRegion> {
        None
    }
    fn map_region(&self, _mapping: &dyn Mapping, _frame: &dyn Frame) -> AstResult<DynRegion> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn clone_region(&self) -> DynRegion {
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
