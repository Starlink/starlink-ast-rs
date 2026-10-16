//! Compilation test: verifies trait signatures are coherent and Region: Frame + Mapping works.

use ast_core::coords::{PointSet, Points, PointsMut};
use ast_core::error::{AstError, AstResult, ErrorCode};
use ast_core::frames::{BasicFrame, BasicFrameBuilder, DynFrame, Frame};
use ast_core::mappings::{DynMapping, Mapping, ViaSimplify};
use ast_core::regions::{DynRegion, Overlap, Region};

/// A dummy type that implements all three traits, proving they're coherent.
/// Its Frame-class attributes are those of an inner plain Frame.
#[derive(Debug)]
struct DummyRegion {
    frame: BasicFrame,
    flags: ast_core::mappings::MappingFlags,
}

impl DummyRegion {
    fn new() -> Self {
        DummyRegion {
            flags: ast_core::mappings::MappingFlags::default(),
            frame: BasicFrameBuilder::new(2).build().unwrap(),
        }
    }
}

/// Forward Frame-class attributes to `self.frame` through the trait.
macro_rules! forward {
    ($($get:ident: $ty:ty, $set:ident, $test:ident, $clear:ident;)*) => {$(
        fn $get(&self) -> $ty {
            Frame::$get(&self.frame)
        }
        fn $set(&mut self, v: $ty) -> AstResult<()> {
            Frame::$set(&mut self.frame, v)
        }
        fn $test(&self) -> bool {
            Frame::$test(&self.frame)
        }
        fn $clear(&mut self) -> AstResult<()> {
            Frame::$clear(&mut self.frame)
        }
    )*};
}

macro_rules! forward_axis {
    ($($get:ident: $ty:ty, $set:ident($arg:ty), $test:ident, $clear:ident;)*) => {$(
        fn $get(&self, axis: usize) -> AstResult<$ty> {
            Frame::$get(&self.frame, axis)
        }
        fn $set(&mut self, axis: usize, v: $arg) -> AstResult<()> {
            Frame::$set(&mut self.frame, axis, v)
        }
        fn $test(&self, axis: usize) -> AstResult<bool> {
            Frame::$test(&self.frame, axis)
        }
        fn $clear(&mut self, axis: usize) -> AstResult<()> {
            Frame::$clear(&mut self.frame, axis)
        }
    )*};
}

impl Mapping for DummyRegion {
    fn mapping_flags(&self) -> &ast_core::mappings::MappingFlags {
        &self.flags
    }
    fn id(&self) -> &str {
        Frame::id(&self.frame)
    }
    fn set_id(&mut self, id: &str) {
        Frame::set_id(&mut self.frame, id).unwrap();
    }
    fn clear_id(&mut self) {
        Frame::clear_id(&mut self.frame).unwrap();
    }
    fn test_id(&self) -> bool {
        Frame::test_id(&self.frame)
    }
    fn mapping_flags_mut(&mut self) -> &mut ast_core::mappings::MappingFlags {
        &mut self.flags
    }
    fn nin(&self) -> usize {
        2
    }
    fn nout(&self) -> usize {
        2
    }
    fn is_inverted(&self) -> bool {
        false
    }
    fn invert_orientation(&mut self) {}
    fn transform(&self, _pts: &PointSet, _forward: bool) -> AstResult<PointSet> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn transform_into(
        &self,
        _pts: Points<'_>,
        _forward: bool,
        _out: PointsMut<'_>,
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
    forward! {
        epoch_mjd: f64, set_epoch_mjd, test_epoch, clear_epoch;
        digits: i32, set_digits, test_digits, clear_digits;
        obs_lat: f64, set_obs_lat, test_obs_lat, clear_obs_lat;
        obs_lon: f64, set_obs_lon, test_obs_lon, clear_obs_lon;
        obs_alt: f64, set_obs_alt, test_obs_alt, clear_obs_alt;
        dtai: f64, set_dtai, test_dtai, clear_dtai;
        dut1: f64, set_dut1, test_dut1, clear_dut1;
        min_axes: usize, set_min_axes, test_min_axes, clear_min_axes;
        max_axes: usize, set_max_axes, test_max_axes, clear_max_axes;
        match_end: bool, set_match_end, test_match_end, clear_match_end;
        preserve_axes: bool, set_preserve_axes, test_preserve_axes, clear_preserve_axes;
        permute: bool, set_permute, test_permute, clear_permute;
    }
    forward_axis! {
        label: String, set_label(&str), test_label, clear_label;
        symbol: String, set_symbol(&str), test_symbol, clear_symbol;
        unit: String, set_unit(&str), test_unit, clear_unit;
        format_str: String, set_format_str(&str), test_format_str, clear_format_str;
        axis_digits: i32, set_axis_digits(i32), test_axis_digits, clear_axis_digits;
        direction: bool, set_direction(bool), test_direction, clear_direction;
        top: f64, set_top(f64), test_top, clear_top;
        bottom: f64, set_bottom(f64), test_bottom, clear_bottom;
    }
    fn title(&self) -> AstResult<String> {
        Frame::title(&self.frame)
    }
    fn set_title(&mut self, title: &str) -> AstResult<()> {
        Frame::set_title(&mut self.frame, title)
    }
    fn test_title(&self) -> bool {
        Frame::test_title(&self.frame)
    }
    fn clear_title(&mut self) -> AstResult<()> {
        Frame::clear_title(&mut self.frame)
    }
    fn domain(&self) -> &str {
        Frame::domain(&self.frame)
    }
    fn set_domain(&mut self, domain: &str) -> AstResult<()> {
        Frame::set_domain(&mut self.frame, domain)
    }
    fn test_domain(&self) -> bool {
        Frame::test_domain(&self.frame)
    }
    fn clear_domain(&mut self) -> AstResult<()> {
        Frame::clear_domain(&mut self.frame)
    }
    fn ident(&self) -> &str {
        Frame::ident(&self.frame)
    }
    fn set_ident(&mut self, ident: &str) -> AstResult<()> {
        Frame::set_ident(&mut self.frame, ident)
    }
    fn clear_ident(&mut self) -> AstResult<()> {
        Frame::clear_ident(&mut self.frame)
    }
    fn test_ident(&self) -> bool {
        Frame::test_ident(&self.frame)
    }
    fn id(&self) -> &str {
        Frame::id(&self.frame)
    }
    fn set_id(&mut self, id: &str) -> AstResult<()> {
        Frame::set_id(&mut self.frame, id)
    }
    fn clear_id(&mut self) -> AstResult<()> {
        Frame::clear_id(&mut self.frame)
    }
    fn test_id(&self) -> bool {
        Frame::test_id(&self.frame)
    }
    fn stored_use_defs(&self) -> Option<bool> {
        self.frame.stored_use_defs()
    }
    fn store_use_defs(&mut self, value: Option<bool>) {
        self.frame.store_use_defs(value);
    }
    fn get_active_unit(&self) -> bool {
        Frame::get_active_unit(&self.frame)
    }
    fn set_active_unit(&mut self, value: bool) {
        Frame::set_active_unit(&mut self.frame, value)
    }
    fn test_active_unit(&self) -> bool {
        Frame::test_active_unit(&self.frame)
    }
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
    fn test_negated(&self) -> bool {
        false
    }
    fn set_negated_raw(&mut self, _negated: bool, _set: bool) {}
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
    fn set_fill_factor(&mut self, _factor: f64) -> AstResult<()> {
        Ok(())
    }
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
    fn is_bounded(&self) -> AstResult<bool> {
        Ok(true)
    }
    fn is_adaptive(&self) -> bool {
        true
    }
    fn test_adaptive(&self) -> bool {
        false
    }
    fn set_adaptive_raw(&mut self, _adaptive: bool, _set: bool) {}
    fn has_dummy_fs(&self) -> bool {
        false
    }
    fn set_region_fs(&mut self, _frame: &dyn Frame) {}
    fn get_region_bounds(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn region_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn get_region_disc(&self) -> AstResult<(Vec<f64>, f64)> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn get_region_mesh(&self) -> AstResult<PointSet> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn get_region_grid(&self) -> AstResult<PointSet> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
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
    fn reg_frame_attrib<'a>(
        &mut self,
        _setting: &ast_core::regions::FrameSetting<'a>,
    ) -> AstResult<ast_core::regions::FrameSetting<'a>> {
        Err(AstError::new(ErrorCode::Nodef, "not implemented"))
    }
    fn store_uncertainty(&mut self, _unc: Option<DynRegion>) {}
    fn test_region_fs(&self) -> bool {
        false
    }
    fn set_region_fs_raw(&mut self, _value: bool, _set: bool) {}
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
    let r = DummyRegion::new();
    assert_eq!(r.nin(), 2);
    assert_eq!(r.naxes(), 2);
    assert!(r.point_in_region(&[0.0, 0.0]).is_err());
}

#[test]
fn region_is_object_safe() {
    let _: Box<dyn Region> = Box::new(DummyRegion::new());
    let _: Box<dyn Frame> = Box::new(DummyRegion::new());
    let _: Box<dyn Mapping> = Box::new(DummyRegion::new());
}

#[test]
fn dyn_types_are_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Box<dyn Mapping>>();
    assert_send::<Box<dyn Frame>>();
    assert_send::<Box<dyn Region>>();
}
