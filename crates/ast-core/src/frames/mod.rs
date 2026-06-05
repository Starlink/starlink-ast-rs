//! The Frame trait and associated types.

mod basic;
mod cmpframe;
pub(crate) mod date_time;
mod dsbspecframe;
pub(crate) mod find_frame;
mod fluxframe;
mod frameset;
pub(crate) mod sky_convert;
pub(crate) mod skyaxis;
mod skyframe;
mod specfluxframe;
mod specframe;
mod timeframe;
pub use basic::{BasicFrame, BasicFrameBuilder};
pub use cmpframe::CmpFrame;
pub use dsbspecframe::{DSBSpecFrame, SideBand};
pub use fluxframe::{FluxFrame, FluxSystem};
pub use frameset::{FrameRef, FrameSet};
pub use sky_convert::{SkyRefIs, SkySystem};
pub use skyframe::SkyFrame;
pub use specfluxframe::SpecFluxFrame;
pub use specframe::{SpecFrame, SpecSystem, StdOfRest};
pub use timeframe::{TimeFrame, TimeScale, TimeSystem};

use std::fmt;

use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{AsAny, DynMapping, PermMap};

/// An owned, dynamically-dispatched Frame.
pub type DynFrame = Box<dyn Frame>;

/// A coordinate frame defining the meaning of coordinate values.
///
/// All concrete frame classes implement this trait. Frames are `Send`
/// but not `Sync`.
pub trait Frame: AsAny + Send + fmt::Debug {
    /// Number of axes in this frame.
    fn naxes(&self) -> usize;

    /// Format a coordinate value for display.
    ///
    /// `axis` is 1-based.
    fn format(&self, axis: usize, value: f64) -> AstResult<String>;

    /// Parse a formatted coordinate string back to a numeric value.
    ///
    /// `axis` is 1-based. Returns `(value, characters_consumed)`.
    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)>;

    /// Geodesic distance between two points in this frame.
    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64>;

    /// Angle at point `b` in the triangle a-b-c.
    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64>;

    /// Move along the geodesic from `from` toward `to` by distance `dist`.
    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>>;

    /// Normalise coordinates to their canonical range.
    fn norm(&self, coords: &mut [f64]) -> AstResult<()>;

    /// Deep-clone this frame for integrity delta computation.
    ///
    /// Used by FrameSet and Region integrity: a snapshot is taken before
    /// mutation, then `map_from` computes the correction mapping.
    fn snapshot(&self) -> DynFrame;

    /// Compute the mapping from a previous state of this frame to the current state.
    ///
    /// After mutating a frame attribute, this returns the transformation that
    /// maps coordinates in the old frame to coordinates in the new frame.
    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping>;

    /// Find a mapping to convert coordinates from this frame to `to`.
    ///
    /// `domains` is a priority-ordered list of domain names to try when
    /// searching for an alignment path. Returns a [`FrameSet`] connecting
    /// `self` (base) to `to` (current) if conversion is possible, or
    /// `None` if the frames are incompatible.
    fn convert(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>>;

    /// Observer geodetic latitude in radians.
    fn obs_lat(&self) -> Option<f64> {
        None
    }

    /// Observer geodetic longitude in radians.
    fn obs_lon(&self) -> Option<f64> {
        None
    }

    /// Observer altitude in metres.
    fn obs_alt(&self) -> Option<f64> {
        None
    }

    /// TAI-UTC in seconds.
    fn dtai(&self) -> Option<f64> {
        None
    }

    /// UT1-UTC in seconds.
    fn dut1(&self) -> Option<f64> {
        None
    }

    /// Coordinate system name (e.g., "FK5", "GALACTIC"), if applicable.
    fn system_string(&self) -> Option<&str> {
        None
    }

    /// Frame domain, e.g. "GRID", "PIXEL", "SKY", "IWC". Empty string if unset.
    fn domain(&self) -> &str {
        ""
    }

    /// Whether the domain attribute has been explicitly set (non-default).
    /// Mirrors C's `astTestDomain`. The default implementation returns
    /// `false`; concrete frames that distinguish "explicit" from
    /// "default" override this to read their backing `Option<String>`
    /// state.
    fn test_domain(&self) -> bool {
        false
    }

    /// Frame identification token (single uppercase letter recommended for
    /// FITS-WCS alt-plane assignment). Empty string if unset.
    fn ident(&self) -> &str {
        ""
    }

    // -----------------------------------------------------------------
    // Match-control attributes (used by `astFindFrame` / `astMatch`).
    // Mirrors the C virtual getters / setters / testers — see
    // frame.c (default impls), skyframe.c / specframe.c (overrides).
    // -----------------------------------------------------------------

    /// Minimum acceptable target axis count when this Frame is used as
    /// a template. Mirrors C `astGetMinAxes` (frame.c). Default: the
    /// number of axes in self.
    fn min_axes(&self) -> usize {
        self.naxes()
    }

    /// Set the minimum acceptable target axis count.
    fn set_min_axes(&mut self, _n: usize) {}

    /// Test whether `MinAxes` has been explicitly set.
    fn test_min_axes(&self) -> bool {
        false
    }

    /// Maximum acceptable target axis count when this Frame is used as
    /// a template. Mirrors C `astGetMaxAxes`. Default: the number of
    /// axes in self.
    fn max_axes(&self) -> usize {
        self.naxes()
    }

    /// Set the maximum acceptable target axis count.
    fn set_max_axes(&mut self, _n: usize) {}

    /// Test whether `MaxAxes` has been explicitly set.
    fn test_max_axes(&self) -> bool {
        false
    }

    /// `MatchEnd`: when the template has fewer axes than the target,
    /// match the *final* template axes onto the *final* target axes
    /// (preserving end-alignment) rather than the default initial-axis
    /// alignment. Mirrors C `astGetMatchEnd`. Default: `false` (`0`).
    fn match_end(&self) -> bool {
        false
    }

    /// Set the `MatchEnd` attribute.
    fn set_match_end(&mut self, _v: bool) {}

    /// Test whether `MatchEnd` has been explicitly set.
    fn test_match_end(&self) -> bool {
        false
    }

    /// `PreserveAxes`: when set, `Match` returns a result Frame whose
    /// axis count equals the target's (rather than the template's).
    /// Mirrors C `astGetPreserveAxes`. Default: `false` (`0`).
    fn preserve_axes(&self) -> bool {
        false
    }

    /// Set the `PreserveAxes` attribute.
    fn set_preserve_axes(&mut self, _v: bool) {}

    /// Test whether `PreserveAxes` has been explicitly set.
    fn test_preserve_axes(&self) -> bool {
        false
    }

    /// `Permute`: whether the template is willing to match a target
    /// whose axes are in a permuted order. Mirrors C `astGetPermute`.
    /// Default: `true` (`1`) for plain Frame; `false` (`0`) for
    /// SkyFrame and SpecFrame (overridden in those classes).
    fn permute(&self) -> bool {
        true
    }

    /// Set the `Permute` attribute.
    fn set_permute(&mut self, _v: bool) {}

    /// Test whether `Permute` has been explicitly set.
    fn test_permute(&self) -> bool {
        false
    }

    /// Permute the axes of this frame.
    ///
    /// `perm` is a 1-based permutation array of length `naxes`. After the
    /// call, external axis `i` corresponds to what was previously axis
    /// `perm[i-1]`. For example, `perm = &[2, 1]` swaps two axes.
    ///
    /// The default implementation validates the permutation and returns Ok
    /// for 1-axis frames (where the only valid permutation is `[1]`).
    fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        validate_permutation(perm, self.naxes())?;
        Ok(())
    }

    /// Test whether this frame is equal to another.
    ///
    /// Uses AST's standard floating-point tolerance for numeric comparisons.
    fn equal(&self, other: &dyn Frame) -> bool;

    // -----------------------------------------------------------------
    // astFindFrame / astMatch / astSubFrame trampoline.
    // See `find_frame.rs` for the algorithm; per-class overrides
    // implement the celestial / spectral / compound matching kernels.
    // -----------------------------------------------------------------

    /// Match `target` against this template.
    ///
    /// Returns `Some(MatchResult)` if `target` can be reached from
    /// this template's coordinate system, `None` otherwise. Mirrors C
    /// `astMatch` (frame.c:7073).
    ///
    /// `match_sub`: when `true`, allow the target to be a more
    /// specialised class than the template (the target is cast down).
    /// `false` requires the target to be the same class or more
    /// specialised. Mirrors the `matchsub` parameter handled by C's
    /// `astMatch_` global wrapper.
    ///
    /// Default implementation returns `Ok(None)` — the trampoline is
    /// only meaningful once a per-class override lands. Concrete
    /// classes override with their own matching kernels.
    fn match_template(
        &self,
        target: &dyn Frame,
        match_sub: bool,
    ) -> AstResult<Option<find_frame::MatchResult>> {
        let _ = (target, match_sub);
        Ok(None)
    }

    /// Select axes from `self` (the target), overlaying `template`'s
    /// explicitly-set attributes on the result.
    ///
    /// `result_naxes` is the number of axes in the final result Frame
    /// (equal to `target_axes.len()` in canonical use). `target_axes`
    /// holds zero-based axis indices into `self` (or `-1` for "no
    /// association"); `template_axes` is the parallel array of zero-
    /// based template indices.
    ///
    /// Returns `Some((map, result))` where `map` transforms `self`
    /// (target) coordinates into `result` coordinates. Mirrors C
    /// `astSubFrame` (frame.c:10838).
    ///
    /// Default implementation returns `Ok(None)`.
    fn sub_frame(
        &self,
        template: Option<&dyn Frame>,
        result_naxes: usize,
        target_axes: &[i32],
        template_axes: &[i32],
    ) -> AstResult<Option<(DynMapping, DynFrame)>> {
        let _ = (template, result_naxes, target_axes, template_axes);
        Ok(None)
    }

    /// Top-level FindFrame — search self (or, in FrameSet / CmpFrame
    /// overrides, self's components) for a Frame the supplied
    /// template matches.
    ///
    /// `domainlist` is a comma-separated, case-insensitive priority
    /// list. An empty field permits any domain. Mirrors C
    /// `astFindFrame` (frame.c:3735).
    ///
    /// Default implementation: try `template.match_template(self, …)`,
    /// gate on domain match, and wrap the result as a 2-frame
    /// FrameSet.
    fn find_frame_template(
        &self,
        template: &dyn Frame,
        domainlist: &str,
    ) -> AstResult<Option<find_frame::FindFrameResult>> {
        find_frame::default_find_frame(self.snapshot(), template, domainlist)
    }
}

/// Extract a subset of axes from a frame, returning a new frame and a mapping.
///
/// `axes` is a 1-based array of axis indices to pick from `frame`.
/// Returns `(new_frame, mapping)` where:
/// - `new_frame` has `axes.len()` axes, inheriting attributes from the
///   selected axes of the original frame.
/// - `mapping` transforms coordinates from the original frame (forward)
///   to the new frame. The inverse maps from the new frame back to the
///   original frame (unselected axes get [`BAD_VALUE`](crate::constants::BAD_VALUE)).
///
/// For a SkyFrame, picking both axes returns a clone; picking one axis
/// returns a BasicFrame (a single celestial axis is no longer a sky frame).
pub fn pick_axes(frame: &dyn Frame, axes: &[usize]) -> AstResult<(DynFrame, DynMapping)> {
    let naxes = frame.naxes();
    let nout = axes.len();

    if nout == 0 {
        return Err(AstError::new(
            ErrorCode::Naxin,
            "must pick at least one axis",
        ));
    }

    for &ax in axes {
        if ax < 1 || ax > naxes {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("axis index {} is out of range 1..={}", ax, naxes),
            ));
        }
    }

    // Build the new frame with the correct number of axes.
    let new_frame: DynFrame = Box::new(BasicFrameBuilder::new(nout).build()?);

    // Build a PermMap: forward selects the picked axes from the original.
    // outperm (forward): output[i] = input[axes[i]] (1-based).
    let outperm: Vec<i64> = axes.iter().map(|&ax| ax as i64).collect();
    // inperm (inverse): input[axes[i]-1] = output[i+1], others = BAD.
    let mut inperm = vec![0i64; naxes]; // 0 → BAD_VALUE
    for (i, &ax) in axes.iter().enumerate() {
        inperm[ax - 1] = (i + 1) as i64;
    }

    let mapping: DynMapping = Box::new(PermMap::new(&inperm, &outperm, &[])?);

    Ok((new_frame, mapping))
}

/// Validate that `perm` is a valid 1-based permutation of length `naxes`.
pub fn validate_permutation(perm: &[usize], naxes: usize) -> AstResult<()> {
    if perm.len() != naxes {
        return Err(AstError::new(
            ErrorCode::Prmin,
            format!(
                "permutation array has {} elements but frame has {} axes",
                perm.len(),
                naxes
            ),
        ));
    }
    let mut seen = vec![false; naxes];
    for (i, &p) in perm.iter().enumerate() {
        if p < 1 || p > naxes {
            return Err(AstError::new(
                ErrorCode::Prmin,
                format!(
                    "permutation element {} (at position {}) is out of range 1..={}",
                    p,
                    i + 1,
                    naxes
                ),
            ));
        }
        if seen[p - 1] {
            return Err(AstError::new(
                ErrorCode::Prmin,
                format!("axis {} appears more than once in permutation", p),
            ));
        }
        seen[p - 1] = true;
    }
    Ok(())
}
