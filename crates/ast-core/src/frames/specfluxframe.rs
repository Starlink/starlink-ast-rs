//! A compound spectrum/flux frame (SpecFluxFrame).

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{DynFrame, FluxFrame, Frame, SpecFrame};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, UnitMap};
use crate::serial::{Node, Serializable};

/// A compound 2-axis frame combining a SpecFrame (axis 1) and a FluxFrame (axis 2).
///
/// A `SpecFluxFrame` is used to describe a spectrum plot where the first axis
/// represents spectral position and the second axis represents flux. It is
/// essentially a [`CmpFrame`](crate::frames::CmpFrame) with known component
/// types, enabling type-safe access to the spectral and flux components.
///
/// In the C library, `SpecFluxFrame` inherits from `CmpFrame`. In this Rust
/// port, it stores the two components directly with their concrete types.
///
/// # Examples
///
/// ```
/// use ast_core::frames::{Frame, FluxFrame, FluxSystem, SpecFluxFrame, SpecFrame, SpecSystem};
///
/// let sf = SpecFrame::new(SpecSystem::Freq);
/// let ff = FluxFrame::new(FluxSystem::FluxDen);
/// let sff = SpecFluxFrame::new(sf, ff);
/// assert_eq!(sff.naxes(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct SpecFluxFrame {
    /// The Frame-class attributes.
    attrs: super::frame_attrs::FrameAttrs,
    /// The default Domain, derived from the components' (cmpframe.c
    /// `GetDomain`), dropped whenever a component is reachable mutably.
    domain_default: std::cell::OnceCell<String>,
    spec_frame: SpecFrame,
    flux_frame: FluxFrame,
    /// External-axis swap flag. `false` (default) leaves axis 0 as
    /// the SpecFrame and axis 1 as the FluxFrame. `true` swaps them.
    /// Mirrors C `astGetPerm` on a SpecFluxFrame whose component
    /// order has been permuted via `astPermAxes`.
    swap: bool,
    // Match-control attributes (used by `astFindFrame` / `astMatch`).
}

impl SpecFluxFrame {
    /// `"<flux label> versus <spec label>"`, capitalised (specfluxframe.c
    /// `GetTitle`), for the `frame_attrs_title` macro.
    fn frame_default_title(&self) -> AstResult<String> {
        let t = format!(
            "{} versus {}",
            Frame::label(&self.flux_frame, 1)?,
            Frame::label(&self.spec_frame, 1)?
        );
        let mut c = t.chars();
        Ok(match c.next() {
            Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
            None => t,
        })
    }

    /// The Frame class's default, 1 (frame.c:13572).
    fn frame_default_permute(&self) -> bool {
        true
    }

    /// Create a new `SpecFluxFrame` from a SpecFrame and a FluxFrame.
    ///
    /// The SpecFrame forms axis 1, and the FluxFrame forms axis 2.
    pub fn new(spec_frame: SpecFrame, flux_frame: FluxFrame) -> Self {
        Self {
            attrs: super::frame_attrs::FrameAttrs::default(),
            domain_default: std::cell::OnceCell::new(),
            spec_frame,
            flux_frame,
            swap: false,
        }
    }

    /// This SpecFluxFrame as the CmpFrame it is in C, for the methods it
    /// inherits from CmpFrame.
    pub(crate) fn as_cmp_frame(&self) -> super::CmpFrame {
        super::CmpFrame::with_parts(
            Box::new(self.spec_frame.clone()),
            Box::new(self.flux_frame.clone()),
            self.perm(),
            self.attrs.clone(),
        )
    }

    /// The SpecFrame and FluxFrame components.
    pub(crate) fn components(&self) -> (&SpecFrame, &FluxFrame) {
        (&self.spec_frame, &self.flux_frame)
    }

    /// Mutable access to the SpecFrame and FluxFrame components.
    pub(crate) fn components_mut(&mut self) -> (&mut SpecFrame, &mut FluxFrame) {
        (&mut self.spec_frame, &mut self.flux_frame)
    }

    /// The axis permutation as a CmpFrame holds it: the internal axis (0 for
    /// the SpecFrame, 1 for the FluxFrame) of each external axis.
    pub(crate) fn perm(&self) -> Vec<usize> {
        if self.swap { vec![1, 0] } else { vec![0, 1] }
    }

    /// Whether the external axis order is (Flux, Spec) instead of
    /// the default (Spec, Flux). Mirrors a non-identity perm on a
    /// 2-axis CmpFrame.
    pub fn axes_swapped(&self) -> bool {
        self.swap
    }

    /// Return a reference to the embedded SpecFrame (axis 1).
    pub fn spec_frame(&self) -> &SpecFrame {
        &self.spec_frame
    }

    /// Return a mutable reference to the embedded SpecFrame (axis 1).
    pub fn spec_frame_mut(&mut self) -> &mut SpecFrame {
        self.domain_default = std::cell::OnceCell::new();
        &mut self.spec_frame
    }

    /// Return a reference to the embedded FluxFrame (axis 2).
    pub fn flux_frame(&self) -> &FluxFrame {
        &self.flux_frame
    }

    /// Return a mutable reference to the embedded FluxFrame (axis 2).
    pub fn flux_frame_mut(&mut self) -> &mut FluxFrame {
        self.domain_default = std::cell::OnceCell::new();
        &mut self.flux_frame
    }

    /// Route a 1-based compound axis to the appropriate child frame
    /// and its local 1-based axis index.
    ///
    /// Axis 1 maps to the SpecFrame; axis 2 maps to the FluxFrame.
    fn route_axis(&self, axis: usize) -> (&dyn Frame, usize) {
        if axis == 1 {
            (&self.spec_frame, 1)
        } else {
            (&self.flux_frame, 1)
        }
    }

    /// The component holding a 1-based axis, and the axis within it.
    fn axis_component(&self, axis: usize) -> AstResult<(&dyn Frame, usize)> {
        self.validate_axis(axis)?;
        Ok(self.route_axis(axis))
    }

    /// The component holding a 1-based axis, to change, and the axis
    /// within it.
    fn axis_component_mut(&mut self, axis: usize) -> AstResult<(&mut dyn Frame, usize)> {
        self.validate_axis(axis)?;
        Ok(if axis == 1 {
            (&mut self.spec_frame, 1)
        } else {
            (&mut self.flux_frame, 1)
        })
    }

    /// Validate that `axis` is 1 or 2.
    fn validate_axis(&self, axis: usize) -> AstResult<()> {
        if !(1..=2).contains(&axis) {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("SpecFluxFrame has 2 axes, got axis={axis}"),
            ));
        }
        Ok(())
    }

    /// A standardised copy of the embedded FluxFrame: System set explicitly
    /// (to the possibly-unit-derived `system()`) and Unit cleared so it
    /// represents the default units for that System. Mirrors C `GetFluxFrame`
    /// with `std=1` (specfluxframe.c:240).
    fn std_flux_frame(&self) -> AstResult<FluxFrame> {
        let mut ff = self.flux_frame.clone();
        ff.set_system(self.flux_frame.system()?)?;
        ff.clear_axis_unit();
        Ok(ff)
    }

    /// A standardised copy of the embedded SpecFrame: System and Unit set to
    /// the spectral system and unit used by the FluxFrame's flux density
    /// system (wavelength/Angstrom or frequency/Hz). Mirrors C `GetSpecFrame`
    /// with `std=1` (specfluxframe.c:307).
    fn std_spec_frame(&self) -> AstResult<SpecFrame> {
        let system = self.flux_frame.system()?;
        let mut sf = self.spec_frame.clone();
        sf.set_system(system.density_spec_system())?;
        sf.store_unit(system.density_unit());
        Ok(sf)
    }
}

// ---------------------------------------------------------------------------
// Frame trait implementation
// ---------------------------------------------------------------------------

impl Frame for SpecFluxFrame {
    fn axis_perm(&self) -> Vec<usize> {
        if self.swap { vec![1, 0] } else { vec![0, 1] }
    }

    // A SpecFluxFrame is a CmpFrame in C (specfluxframe.c), so its
    // Frame-class attributes follow CmpFrame's rules.
    frame_attrs_title!();
    frame_attrs_digits!();
    frame_attrs_ident!();

    /// An unset UseDefs is the SpecFrame component's own, as CmpFrame's
    /// `GetUseDefs` reads its first component (cmpframe.c).
    fn use_defs(&self) -> bool {
        self.attrs
            .use_defs
            .unwrap_or_else(|| self.spec_frame.stored_use_defs().unwrap_or(true))
    }
    frame_attrs_match!(min = 0, max = 1_000_000);

    fn set_frame_flags(&mut self, flags: u32) {
        // A SpecFluxFrame is a CmpFrame in C, whose `SetFrameFlags` gives
        // the components the same flags (cmpframe.c).
        self.attrs.flags = flags;
        self.spec_frame.set_frame_flags(flags);
        self.flux_frame.set_frame_flags(flags);
    }
    cmp_frame_shared_attr!(concrete spec_frame, flux_frame; epoch, epoch_mjd, set_epoch_mjd, test_epoch, clear_epoch);
    cmp_frame_shared_attr!(concrete spec_frame, flux_frame; obs_lat, obs_lat, set_obs_lat, test_obs_lat, clear_obs_lat);
    cmp_frame_shared_attr!(concrete spec_frame, flux_frame; obs_lon, obs_lon, set_obs_lon, test_obs_lon, clear_obs_lon);
    cmp_frame_shared_attr!(concrete spec_frame, flux_frame; obs_alt, obs_alt, set_obs_alt, test_obs_alt, clear_obs_alt);
    cmp_frame_shared_attr!(concrete spec_frame, flux_frame; dtai, dtai, set_dtai, test_dtai, clear_dtai);
    cmp_frame_shared_attr!(concrete spec_frame, flux_frame; dut1, dut1, set_dut1, test_dut1, clear_dut1);
    cmp_frame_axis_attrs!();

    /// The Domain, set or else the components' joined with a hyphen, or
    /// "CMP" when both are blank (cmpframe.c `GetDomain`).
    fn domain(&self) -> &str {
        if let Some(d) = self.attrs.domain.as_deref() {
            return d;
        }
        self.domain_default.get_or_init(|| {
            let dom1 = Frame::domain(&self.spec_frame);
            let dom2 = Frame::domain(&self.flux_frame);
            if dom1.is_empty() && dom2.is_empty() {
                "CMP".to_string()
            } else {
                format!("{dom1}-{dom2}")
            }
        })
    }
    fn set_domain(&mut self, domain: &str) -> AstResult<()> {
        self.attrs.domain = Some(super::frame_attrs::clean_domain(domain));
        Ok(())
    }
    fn test_domain(&self) -> bool {
        self.attrs.domain.is_some()
    }
    fn clear_domain(&mut self) -> AstResult<()> {
        self.attrs.domain = None;
        Ok(())
    }

    /// CmpFrame's rule (cmpframe.c:8369): the Frame class's value when set,
    /// else true when either component has active units.
    fn get_active_unit(&self) -> bool {
        match self.attrs.active_unit {
            Some(v) => v,
            None => {
                Frame::get_active_unit(&self.spec_frame) || Frame::get_active_unit(&self.flux_frame)
            }
        }
    }
    fn set_active_unit(&mut self, value: bool) {
        self.attrs.active_unit = Some(value);
        Frame::set_active_unit(&mut self.spec_frame, value);
        Frame::set_active_unit(&mut self.flux_frame, value);
    }
    fn test_active_unit(&self) -> bool {
        self.attrs.active_unit.is_some()
    }

    fn naxes(&self) -> usize {
        2
    }

    /// Inherited from CmpFrame (cmpframe.c `PrimaryFrame`): each axis
    /// belongs to the one-axis SpecFrame or FluxFrame component.
    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        if self.perm()[axis] == 0 {
            self.spec_frame.primary_frame(0)
        } else {
            self.flux_frame.primary_frame(0)
        }
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        self.validate_axis(axis)?;
        let (child, local_axis) = self.route_axis(axis);
        child.format(local_axis, value)
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        // SpecFluxFrame inherits from CmpFrame in C and does not override
        // Unformat, so it uses CmpFrame's per-axis routing (cmpframe.c:10041):
        // each external axis is unformatted by its owning component frame.
        self.validate_axis(axis)?;
        let (child, local_axis) = self.route_axis(axis);
        child.unformat(local_axis, text)
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        if point_has_bad(a) || point_has_bad(b) {
            return Ok(BAD_VALUE);
        }
        // Euclidean distance in the 2-D compound space.
        let d0 = a[0] - b[0];
        let d1 = a[1] - b[1];
        Ok(crate::math::sqrt(d0 * d0 + d1 * d1))
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        if point_has_bad(a) || point_has_bad(b) || point_has_bad(c) {
            return Ok(BAD_VALUE);
        }
        let ba0 = a[0] - b[0];
        let ba1 = a[1] - b[1];
        let bc0 = c[0] - b[0];
        let bc1 = c[1] - b[1];
        let dot = ba0 * bc0 + ba1 * bc1;
        let mag_ba = crate::math::sqrt(ba0 * ba0 + ba1 * ba1);
        let mag_bc = crate::math::sqrt(bc0 * bc0 + bc1 * bc1);
        let denom = mag_ba * mag_bc;
        if denom == 0.0 {
            return Ok(0.0);
        }
        let cos_angle = (dot / denom).clamp(-1.0, 1.0);
        Ok(crate::math::acos(cos_angle))
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        // C inherits CmpFrame's Offset (cmpframe.c Offset): the offset is
        // split between the two one-axis components in proportion to their
        // distances, so the path passes through the second point, and each
        // component applies the Frame-class offset to its share.
        if point_has_bad(from) || point_has_bad(to) || is_bad(dist) {
            return Ok(vec![BAD_VALUE; 2]);
        }
        let d1 = super::cartesian_distance(&from[..1], &to[..1]);
        let d2 = super::cartesian_distance(&from[1..2], &to[1..2]);
        let total = crate::math::sqrt(d1 * d1 + d2 * d2);
        let (off1, off2) = if total == 0.0 {
            if dist != 0.0 {
                return Ok(vec![BAD_VALUE; 2]);
            }
            (0.0, 0.0)
        } else {
            (dist * d1 / total, dist * d2 / total)
        };
        let r1 = super::cartesian_offset(&from[..1], &to[..1], off1)[0];
        let r2 = super::cartesian_offset(&from[1..2], &to[1..2], off2)[0];
        if is_bad(r1) || is_bad(r2) {
            return Ok(vec![BAD_VALUE; 2]);
        }
        Ok(vec![r1, r2])
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        // Delegate axis 1 to SpecFrame, axis 2 to FluxFrame.
        self.spec_frame.norm(&mut coords[..1])?;
        self.flux_frame.norm(&mut coords[1..2])?;
        Ok(())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        super::find_frame::integrity_mapping(previous, self)
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        // C Frame equality is conversion-based (frame.c:3549); SpecFluxFrame
        // (a CmpFrame subclass) does not override it. Route through the
        // shared helper so the component spectral/flux conversions decide
        // equality.
        super::find_frame::frames_equal(self, other)
    }

    /// Inherited from CmpFrame, whose System is always "Compound"
    /// (cmpframe.c `GetSystem`).
    fn system_string(&self) -> AstResult<Option<&str>> {
        Ok(Some("Compound"))
    }

    fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        super::validate_permutation(perm, 2)?;
        // Two-element permutation: identity is [1,2]; swap is [2,1].
        if perm[0] == 2 && perm[1] == 1 {
            self.swap = !self.swap;
        }
        Ok(())
    }

    // -----------------------------------------------------------------
    // Match-control attributes (mirror C `astGet/SetMinAxes` etc.).
    // -----------------------------------------------------------------

    // -----------------------------------------------------------------
    // Match / SubFrame / Overlay (specfluxframe.c:1216 / 1428).
    // -----------------------------------------------------------------

    fn match_template(
        &self,
        target: &dyn Frame,
        match_sub: bool,
    ) -> AstResult<Option<super::find_frame::MatchResult>> {
        // specfluxframe.c:1330: a target that is not a SpecFluxFrame is
        // matched by the parent CmpFrame Match.
        let Some(target_sff) = target.as_any().downcast_ref::<SpecFluxFrame>() else {
            return self.as_cmp_frame().match_template(target, match_sub);
        };

        // Axis-swap detection (specfluxframe.c:1363-1375).
        let swap1 = self.swap;
        let swap2 = target_sff.swap;
        let swap = swap1 != swap2;
        if swap && !self.permute() {
            return Ok(None);
        }

        // Build axis association arrays (specfluxframe.c:1383-1401).
        let (template_axes_vec, target_axes_vec) = if self.preserve_axes() {
            (
                vec![if swap { 1 } else { 0 }, if swap { 0 } else { 1 }],
                vec![0i32, 1],
            )
        } else {
            (
                vec![0i32, 1],
                vec![if swap { 1 } else { 0 }, if swap { 0 } else { 1 }],
            )
        };

        // Defer to target's SubFrame (specfluxframe.c:1408).
        let Some((map, result)) =
            target.sub_frame(Some(self), 2, &target_axes_vec, &template_axes_vec)?
        else {
            return Ok(None);
        };

        Ok(Some(super::find_frame::MatchResult {
            template_axes: template_axes_vec,
            target_axes: target_axes_vec,
            map,
            result,
        }))
    }

    fn sub_frame(
        &self,
        template: Option<&dyn Frame>,
        result_naxes: usize,
        target_axes: &[i32],
        template_axes: &[i32],
    ) -> AstResult<Option<(DynMapping, DynFrame)>> {
        specfluxframe_sub_frame(self, template, result_naxes, target_axes, template_axes)
    }

    fn overlay(&self, template_axes: &[i32], result: &mut dyn Frame) -> AstResult<()> {
        // A SpecFluxFrame is a CmpFrame in C and uses its Overlay.
        super::find_frame::compound_overlay(
            &self.spec_frame,
            &self.flux_frame,
            &self.perm(),
            template_axes,
            result,
        )
    }

    fn axis_overlay_source(&self, axis: usize) -> Option<super::overlay::AxisOverlaySource> {
        match *self.perm().get(axis.checked_sub(1)?)? {
            0 => self.spec_frame.axis_overlay_source(1),
            _ => self.flux_frame.axis_overlay_source(1),
        }
    }

    fn apply_axis_overlay(&mut self, axis: usize, source: &super::overlay::AxisOverlaySource) {
        match axis
            .checked_sub(1)
            .and_then(|a| self.perm().get(a).copied())
        {
            Some(0) => self.spec_frame.apply_axis_overlay(1, source),
            Some(_) => self.flux_frame.apply_axis_overlay(1, source),
            None => {}
        }
    }

    fn pick_axes_snapshot(&self, target_axes: &[i32]) -> AstResult<DynFrame> {
        // Full pick of both axes in canonical order: identity.
        if target_axes.len() == 2 && target_axes[0] == 0 && target_axes[1] == 1 {
            return Ok(Box::new(self.clone()));
        }
        // Reverse pick: swap axes.
        if target_axes.len() == 2 && target_axes[0] == 1 && target_axes[1] == 0 {
            let mut snap = self.clone();
            snap.swap = !snap.swap;
            return Ok(Box::new(snap));
        }
        // Any other selection collapses to a BasicFrame snapshot.
        let bf = super::basic::BasicFrameBuilder::new(target_axes.len()).build()?;
        Ok(Box::new(bf))
    }
}

// ---------------------------------------------------------------------------
// MakeSFMapping and its component-mapping helpers (specfluxframe.c)
//
// All PermMap index arrays are translated from C's 0-based axis numbering to
// the 1-based convention of the Rust `PermMap`.
// ---------------------------------------------------------------------------

use crate::mappings::{CmpMap, Composition, Mapping, MathMap, PermMap, RateMap, TranMap};

/// The forward half of the flux-scaling TranMap (specfluxframe.c::MakeMapF,
/// 754). A 2-in/2-out Mapping whose forward transform takes standardised
/// `(v1, x1)` to standardised `(v2, x2)`, dividing the flux value by the
/// absolute rate of change of `x2` with respect to `x1`.
fn make_map_f(x1: &SpecFrame, x2: &SpecFrame) -> AstResult<DynMapping> {
    // m: x1 -> x2 spectral mapping.
    let m = x1.make_spec_mapping(x2)?;
    // rate: d(x2)/d(x1) as a 1-in/1-out mapping (C astRateMap(m,0,0)).
    let rate = RateMap::new(m.clone_box(), 1, 1)?;
    // div: 2-in/1-out, out = in0 / abs(in1) (inverse undefined, matching C).
    let div = MathMap::new(2, 1, &["out=in0/abs(in1)"], &["in0", "in1"])?;
    let unit = UnitMap::new(1);
    // perm: 2-in/3-out; C inperm=[0,2], outperm=[0,1,1] -> 1-based [1,3]/[1,2,2].
    let perm = PermMap::new(&[1, 3], &[1, 2, 2], &[])?;

    // cmap1 = unit ∥ rate  : (v1,x1) -> (v1, dx2/dx1)
    let cmap1 = CmpMap::new(Box::new(unit), Box::new(rate), Composition::Parallel)?;
    // cmap2 = cmap1 ∘ div  : (v1,x1) -> v2
    let cmap2 = CmpMap::new(Box::new(cmap1), Box::new(div), Composition::Series)?;
    // cmap3 = cmap2 ∥ m    : (v1,x1,x1) -> (v2,x2)
    let cmap3 = CmpMap::new(Box::new(cmap2), m, Composition::Parallel)?;
    // ret = perm ∘ cmap3   : (v1,x1) -> (v2,x2)
    let ret = CmpMap::new(Box::new(perm), Box::new(cmap3), Composition::Series)?;
    Ok(Box::new(ret))
}

/// The inverse half of the flux-scaling TranMap (specfluxframe.c::MakeMapI,
/// 895). Built as a forward `(v2,x2) -> (v1,x1)` Mapping which is then
/// inverted, so that its *inverse* transform runs `(v2,x2) -> (v1,x1)` — i.e.
/// its forward direction matches `make_map_f`.
fn make_map_i(x1: &SpecFrame, x2: &SpecFrame) -> AstResult<DynMapping> {
    // m: x1 -> x2 spectral mapping; RateMap captures the original invert flag.
    let m = x1.make_spec_mapping(x2)?;
    let rate = RateMap::new(m.clone_box(), 1, 1)?;
    // Invert m so its forward goes x2 -> x1.
    let mut m_inv = m;
    m_inv.invert();
    // mult: 2-in/1-out, out = in0 * abs(in1) (inverse undefined, matching C).
    let mult = MathMap::new(2, 1, &["out=in0*abs(in1)"], &["in0", "in1"])?;
    let unit = UnitMap::new(1);
    let perm = PermMap::new(&[1, 3], &[1, 2, 2], &[])?;

    // cmap1 = unit ∥ rate  : (v2,x1) -> (v2, dx2/dx1)
    let cmap1 = CmpMap::new(
        Box::new(unit.clone()),
        Box::new(rate),
        Composition::Parallel,
    )?;
    // cmap2 = cmap1 ∘ mult : (v2,x1) -> v1
    let cmap2 = CmpMap::new(Box::new(cmap1), Box::new(mult), Composition::Series)?;
    // cmap3 = cmap2 ∥ unit : (v2,x1,x1) -> (v1,x1)
    let cmap3 = CmpMap::new(
        Box::new(cmap2),
        Box::new(unit.clone()),
        Composition::Parallel,
    )?;
    // cmap4 = perm ∘ cmap3 : (v2,x1) -> (v1,x1)
    let cmap4 = CmpMap::new(Box::new(perm), Box::new(cmap3), Composition::Series)?;
    // cmap5 = unit ∥ m_inv : (v2,x2) -> (v2,x1)
    let cmap5 = CmpMap::new(Box::new(unit), m_inv, Composition::Parallel)?;
    // ret = cmap5 ∘ cmap4  : (v2,x2) -> (v1,x1), then invert.
    let mut ret = CmpMap::new(Box::new(cmap5), Box::new(cmap4), Composition::Series)?;
    ret.invert();
    Ok(Box::new(ret))
}

/// The second Mapping of MakeSFMapping (specfluxframe.c::MakeMap2, 516): a
/// 3-in/3-out parallel CmpMap converting `(v1, x1)` from the SpecFluxFrame's
/// user units into the default units of the flux system's density frame, and
/// passing the third axis (a copy of `v1`) unchanged.
fn make_map2(sff: &SpecFluxFrame) -> AstResult<Option<DynMapping>> {
    // ax1_map: supplied FluxFrame -> default-units FluxFrame.
    let f1 = sff.flux_frame.clone();
    let f2 = sff.std_flux_frame()?;
    let Some(ax1_map) = f1.make_flux_mapping(&f2)? else {
        return Ok(None);
    };
    // ax2_map: supplied SpecFrame -> density-system SpecFrame.
    let s1 = sff.spec_frame.clone();
    let s2 = sff.std_spec_frame()?;
    let ax2_map = s1.make_spec_mapping(&s2)?;
    // ax3_map: UnitMap on the copied flux axis.
    let ax3_map = UnitMap::new(1);

    let tmap = CmpMap::new(ax1_map, ax2_map, Composition::Parallel)?;
    let ret = CmpMap::new(Box::new(tmap), Box::new(ax3_map), Composition::Parallel)?;
    Ok(Some(Box::new(ret)))
}

/// The third Mapping of MakeSFMapping (specfluxframe.c::MakeMap3, 631): a
/// 3-in/3-out parallel CmpMap. The first two axes are a `TranMap` between
/// standardised `(v1,x1)` and `(v2,x2)` (the flux scaling); the third axis is
/// the unmodified `x1 -> x2` spectral mapping.
fn make_map3(target: &SpecFluxFrame, result: &SpecFluxFrame) -> AstResult<Option<DynMapping>> {
    // Standardised SpecFrames for the flux-scaling TranMap.
    let sf1 = target.std_spec_frame()?;
    let sf2 = result.std_spec_frame()?;
    let fmap = make_map_f(&sf1, &sf2)?;
    let imap = make_map_i(&sf1, &sf2)?;
    let mapa = TranMap::new(fmap, imap)?;

    // Third axis: the unmodified supplied x1 -> x2 spectral mapping.
    let mapb = target.spec_frame.make_spec_mapping(&result.spec_frame)?;

    let ret = CmpMap::new(Box::new(mapa), mapb, Composition::Parallel)?;
    Ok(Some(Box::new(ret)))
}

/// Build the 2-in/2-out conversion Mapping between two SpecFluxFrames,
/// mirroring C `MakeSFMapping` (specfluxframe.c:1060). The Mapping's inputs
/// and outputs are `(x, v)` — the spectral position on axis 0 and the flux
/// value on axis 1, in each frame's own systems and units. Returns `None`
/// when no conversion can be formed.
fn make_sf_mapping(
    target: &SpecFluxFrame,
    result: &SpecFluxFrame,
) -> AstResult<Option<DynMapping>> {
    // Identity short-circuit: when the spectral and flux components are each
    // equal (same system/unit/attributes), the conversion is an identity, so
    // emit a UnitMap directly.
    //
    // This is a DELIBERATE divergence from the reference C library. C's
    // SpecFluxFrame conversion always carries the flux-scaling RateMap, which
    // `astSimplify` cannot reduce even when target == result, so the
    // conversion never collapses to a UnitMap — and the conversion-based
    // `astEqual` therefore reports two separately-constructed *identical*
    // SpecFluxFrames as NOT equal (astEqual(a,b) == 0, while the component
    // FluxFrame/SpecFrame each correctly report equal). That looks like a C
    // bug (a frame should equal its own copy); we treat genuinely-identical
    // SpecFluxFrames as equal here. Logged in docs/issues/c-library-quirks.md;
    // revisit if upstream confirms the C behaviour is intentional.
    if Frame::equal(&target.spec_frame, &result.spec_frame)
        && Frame::equal(&target.flux_frame, &result.flux_frame)
    {
        return Ok(Some(Box::new(UnitMap::new(2))));
    }

    // map1: PermMap copying v1 to output 0 and x1 to outputs 1 and 2.
    // C inperm=[2,0], outperm=[1,0,0]; 1-based -> inperm=[3,1], outperm=[2,1,1].
    let map1 = PermMap::new(&[3, 1], &[2, 1, 1], &[])?;

    let Some(map2) = make_map2(target)? else {
        return Ok(None);
    };
    let Some(map3) = make_map3(target, result)? else {
        return Ok(None);
    };
    let Some(mut map4) = make_map2(result)? else {
        return Ok(None);
    };
    map4.invert();
    // map5 = inverse of map1.
    let mut map5 = PermMap::new(&[3, 1], &[2, 1, 1], &[])?;
    map5.invert();

    let tmap1 = CmpMap::new(Box::new(map1), map2, Composition::Series)?;
    let tmap2 = CmpMap::new(Box::new(tmap1), map3, Composition::Series)?;
    let tmap3 = CmpMap::new(Box::new(tmap2), map4, Composition::Series)?;
    let tmap4 = CmpMap::new(Box::new(tmap3), Box::new(map5), Composition::Series)?;

    // Simplify the result, mirroring C astSimplify.
    Ok(Some(<dyn Mapping>::simplify(&tmap4)?))
}

/// Build the SpecFluxFrame conversion in *external* axis order, bracketing
/// the internal `(spec, flux)` [`make_sf_mapping`] with axis-swap PermMaps for
/// any frame whose external axes are `(flux, spec)`.
fn build_sf_conversion(
    target: &SpecFluxFrame,
    result: &SpecFluxFrame,
) -> AstResult<Option<DynMapping>> {
    let Some(core) = make_sf_mapping(target, result)? else {
        return Ok(None);
    };

    // Fast path: no axis swap on either side — the core mapping is already in
    // external axis order.
    if !target.swap && !result.swap {
        return Ok(Some(core));
    }

    // A 2-axis swap PermMap (its own inverse).
    let swap = || PermMap::new(&[2, 1], &[2, 1], &[]);

    let mut map = core;
    if target.swap {
        // External target is (flux, spec); swap to (spec, flux) before core.
        map = Box::new(CmpMap::new(Box::new(swap()?), map, Composition::Series)?);
    }
    if result.swap {
        // Core yields (spec, flux); swap to external (flux, spec).
        map = Box::new(CmpMap::new(map, Box::new(swap()?), Composition::Series)?);
    }
    Ok(Some(map.simplify()?))
}

// ---------------------------------------------------------------------------
// SubFrame helper
// ---------------------------------------------------------------------------

/// Mirrors C `SubFrame` for SpecFluxFrame (specfluxframe.c:1428).
fn specfluxframe_sub_frame(
    target: &SpecFluxFrame,
    template: Option<&dyn Frame>,
    result_naxes: usize,
    target_axes: &[i32],
    template_axes: &[i32],
) -> AstResult<Option<(DynMapping, DynFrame)>> {
    // Result-is-a-SpecFluxFrame branch (specfluxframe.c:1554+): only
    // when template is itself a SpecFluxFrame and we're picking 2
    // axes from a 2-axis target.
    let template_is_sff = template
        .as_ref()
        .map(|t| t.as_any().downcast_ref::<SpecFluxFrame>().is_some())
        .unwrap_or(false);

    if template_is_sff && result_naxes == 2 {
        // Form result from a copy of the target.
        let mut result = target.clone();
        // Apply the target_axes permutation (specfluxframe.c:1562).
        if target_axes.len() == 2 && target_axes[0] == 1 && target_axes[1] == 0 {
            result.swap = !result.swap;
        }

        // Overlay the template attributes (specfluxframe.c:1565).
        if let Some(t) = template {
            let mut result_dyn: DynFrame = Box::new(result);
            t.overlay(template_axes, result_dyn.as_mut())?;
            // Recover the result SpecFluxFrame after overlay.
            let result = result_dyn
                .as_any()
                .downcast_ref::<SpecFluxFrame>()
                .expect("overlay preserves type")
                .clone();

            // Build the full SpecFluxFrame conversion (MakeSFMapping),
            // bracketed by axis-swap PermMaps so the returned Mapping is in
            // each frame's external axis order.
            match build_sf_conversion(target, &result)? {
                Some(map) => return Ok(Some((map, Box::new(result)))),
                None => return Ok(None),
            }
        }

        // No template — identity Mapping over the (possibly
        // permuted) result.
        let map: DynMapping = Box::new(UnitMap::new(2));
        return Ok(Some((map, Box::new(result))));
    }

    // specfluxframe.c:1554: any other selection is made by the parent
    // CmpFrame SubFrame, so a single axis keeps its SpecFrame or
    // FluxFrame class.
    target
        .as_cmp_frame()
        .sub_frame(template, result_naxes, target_axes, template_axes)
}

// ---------------------------------------------------------------------------
// Serialisation
// ---------------------------------------------------------------------------

impl Serializable for SpecFluxFrame {
    fn class_name(&self) -> &'static str {
        "SpecFluxFrame"
    }

    /// A SpecFluxFrame has no Dump of its own: C writes the CmpFrame it is,
    /// under its own class name, and then its `IsA CmpFrame` marker.
    fn to_node(&self) -> AstResult<Node> {
        let mut node = Serializable::to_node(&self.as_cmp_frame())?;
        node.class = self.class_name().to_string();
        node.class_comment =
            Some("Compound spectral/flux coordinate system description".to_string());
        node.push_isa(
            "CmpFrame",
            Some("Compound coordinate system description".to_string()),
        );
        Ok(node)
    }

    /// Loaded as the CmpFrame it is in C, whose components must be a
    /// SpecFrame and a FluxFrame.
    fn from_node(node: &Node) -> AstResult<Self> {
        let cmp = <super::CmpFrame as Serializable>::from_node(node)?;
        let (frame_a, frame_b, perm, attrs) = cmp.into_parts();
        let spec_frame = frame_a
            .as_any()
            .downcast_ref::<SpecFrame>()
            .ok_or_else(|| {
                AstError::new(ErrorCode::Lderr, "SpecFluxFrame: FrameA is not a SpecFrame")
            })?
            .clone();
        let flux_frame = frame_b
            .as_any()
            .downcast_ref::<FluxFrame>()
            .ok_or_else(|| {
                AstError::new(ErrorCode::Lderr, "SpecFluxFrame: FrameB is not a FluxFrame")
            })?
            .clone();
        let mut sff = SpecFluxFrame::new(spec_frame, flux_frame);
        sff.swap = perm == [1, 0];
        sff.attrs = attrs;
        Ok(sff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::{FluxSystem, SpecSystem};

    fn sff() -> SpecFluxFrame {
        SpecFluxFrame::new(
            SpecFrame::new(SpecSystem::Freq),
            FluxFrame::new(FluxSystem::FluxDen),
        )
    }

    #[test]
    fn match_template_specflux_self() {
        let template = sff();
        let target = sff();
        let m = template
            .match_template(&target, false)
            .unwrap()
            .expect("self-match");
        assert_eq!(m.template_axes.len(), 2);
        assert_eq!(m.target_axes.len(), 2);
        assert_eq!(m.result.naxes(), 2);
    }

    #[test]
    fn match_template_specflux_against_basicframe_fails() {
        let template = sff();
        let target = crate::frames::BasicFrameBuilder::new(2).build().unwrap();
        let result = template.match_template(&target, false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn match_template_specflux_swap_blocked_when_permute_false() {
        let mut template = sff();
        template.set_permute(false).unwrap();
        let mut target = sff();
        // Swap the target axes to force a permutation.
        target.perm_axes(&[2, 1]).unwrap();
        let result = template.match_template(&target, false).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn match_template_specflux_swap_allowed_when_permute_true() {
        let mut template = sff();
        template.set_permute(true).unwrap();
        let mut target = sff();
        target.perm_axes(&[2, 1]).unwrap();
        let m = template
            .match_template(&target, false)
            .unwrap()
            .expect("permute=true allows swap");
        assert_eq!(m.template_axes.len(), 2);
    }

    #[test]
    fn sub_frame_specflux_to_specflux_identity() {
        let target = sff();
        let template = sff();
        let (map, result) = target
            .sub_frame(Some(&template), 2, &[0, 1], &[0, 1])
            .unwrap()
            .expect("identity sub_frame");
        assert_eq!(map.nin(), 2);
        assert_eq!(map.nout(), 2);
        let r = result.as_any().downcast_ref::<SpecFluxFrame>().unwrap();
        assert!(!r.axes_swapped());
    }

    #[test]
    fn sub_frame_specflux_axis_swap() {
        let target = sff();
        let template = sff();
        let (_map, result) = target
            .sub_frame(Some(&template), 2, &[1, 0], &[0, 1])
            .unwrap()
            .expect("swap sub_frame");
        let r = result.as_any().downcast_ref::<SpecFluxFrame>().unwrap();
        assert!(r.axes_swapped());
    }

    #[test]
    fn pick_axes_snapshot_full_returns_specflux() {
        let target = sff();
        let snap = target.pick_axes_snapshot(&[0, 1]).unwrap();
        assert!(snap.as_any().downcast_ref::<SpecFluxFrame>().is_some());
    }

    #[test]
    fn pick_axes_snapshot_partial_returns_basicframe() {
        let target = sff();
        let snap = target.pick_axes_snapshot(&[0]).unwrap();
        assert_eq!(snap.naxes(), 1);
        assert!(snap.as_any().downcast_ref::<SpecFluxFrame>().is_none());
    }
}
