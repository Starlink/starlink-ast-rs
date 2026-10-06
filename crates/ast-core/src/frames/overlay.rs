//! The Frame-class part of C's `astOverlay` (frame.c:8513), which every
//! class's Overlay starts from, and the Axis objects it overlays.
//!
//! C's Overlay is a method of the template: `astOverlay( template,
//! template_axes, result )` dispatches on the template's class, which
//! transfers the attributes it defines, each through the result's own
//! setter, and only onto a result of its own class. The port follows that
//! shape with [`Frame::overlay`], called on the template.

use super::Frame;
use super::axis::FrameAxis;
use crate::error::AstResult;

/// A copy of one Axis object of a template, as C's `astGetAxis` returns it
/// to `astAxisOverlay`.
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct AxisOverlaySource(pub(crate) FrameAxis);

impl AxisOverlaySource {
    /// Overlay this template Axis onto `result` (axis.c `AxisOverlay`, and
    /// skyaxis.c `AxisOverlay` when both are SkyAxis objects).
    pub(crate) fn apply_to(&self, result: &mut FrameAxis) {
        result.overlay_from(&self.0);
    }
}

/// C's Frame `Overlay` (frame.c:8513): transfer each Frame attribute set on
/// `template`, then ActiveUnit, then System and AlignSystem where valid for
/// the result, then each template axis's attributes onto the result axis
/// it corresponds to.
pub(crate) fn frame_overlay<T: Frame + ?Sized>(
    template: &T,
    template_axes: &[i32],
    result: &mut dyn Frame,
) -> AstResult<()> {
    if template.test_dtai() {
        result.set_dtai(template.dtai())?;
    }
    if template.test_dut1() {
        result.set_dut1(template.dut1())?;
    }
    if template.test_digits() {
        result.set_digits(template.digits())?;
    }
    if template.test_domain() {
        result.set_domain(template.domain())?;
    }
    if template.test_epoch() {
        result.set_epoch_mjd(template.epoch_mjd())?;
    }
    if template.test_title() {
        result.set_title(&template.title())?;
    }
    if template.test_obs_lat() {
        result.set_obs_lat(template.obs_lat())?;
    }
    if template.test_obs_lon() {
        result.set_obs_lon(template.obs_lon())?;
    }
    if template.test_obs_alt() {
        result.set_obs_alt(template.obs_alt())?;
    }
    result.set_active_unit(template.get_active_unit());
    result.overlay_system_from(template.as_any())?;

    let template_naxes = template.naxes();
    for result_axis in 0..result.naxes() {
        let template_axis = if template_axes.is_empty() {
            result_axis as i32
        } else {
            template_axes.get(result_axis).copied().unwrap_or(-1)
        };
        if template_axis < 0 || template_axis as usize >= template_naxes {
            continue;
        }
        if let Some(source) = template.axis_overlay_source(template_axis as usize + 1) {
            result.apply_axis_overlay(result_axis + 1, &source);
        }
    }
    Ok(())
}
