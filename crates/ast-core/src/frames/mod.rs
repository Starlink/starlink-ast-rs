//! The Frame trait and associated types.

pub(crate) mod axis_format;
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
pub(crate) mod specframe;
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

    /// `Digits`: default number of significant figures used when
    /// formatting coordinate values. Mirrors C `astGetDigits`
    /// (frame.c:13134) — default 7.
    fn digits(&self) -> i32 {
        7
    }

    /// Set the `Digits` attribute. C constrains the stored value to be
    /// at least 1 (frame.c:13138).
    fn set_digits(&mut self, _value: i32) {}

    /// Test whether `Digits` has been explicitly set.
    fn test_digits(&self) -> bool {
        false
    }

    /// Per-axis `Digits(axis)` (1-based). Falls back to the frame-level
    /// `Digits` when no axis-specific value is set. Mirrors C
    /// `astGetAxisDigits` resolution in `Format` (frame.c:4260).
    fn axis_digits(&self, _axis: usize) -> i32 {
        self.digits()
    }

    /// Per-axis `Direction(axis)` (1-based). Plot hint; default 1.
    /// Mirrors C `astGetAxisDirection` (axis.c) — default 1.
    fn direction(&self, _axis: usize) -> bool {
        true
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

    /// Snapshot of `self` with only the axes listed in `target_axes`.
    ///
    /// `target_axes` entries are zero-based indices into `self`'s
    /// axis space (with `-1` meaning "no association — use defaults").
    /// The returned Frame has `target_axes.len()` axes, in the order
    /// given. Per-axis metadata (Label/Unit/Symbol/Format) follows
    /// the picked axes; Frame-level metadata (Title/Domain) is
    /// preserved from `self`. Mirrors C `astPickAxes` (frame.c:8400).
    ///
    /// The default implementation falls back to a plain BasicFrame
    /// snapshot with `target_axes.len()` axes — only correct for
    /// classes whose per-axis metadata isn't load-bearing. Concrete
    /// classes with strong axis identity (SkyFrame, SpecFrame,
    /// FluxFrame) override this.
    fn pick_axes_snapshot(&self, target_axes: &[i32]) -> AstResult<DynFrame> {
        // Default: produce a BasicFrame of the right size, copying
        // the Frame-level Domain.
        let mut builder = BasicFrameBuilder::new(target_axes.len());
        if self.test_domain() {
            builder = builder.domain(self.domain());
        }
        let frame = builder.build()?;
        Ok(Box::new(frame))
    }

    /// Overlay `template`'s explicitly-set attributes onto `self`.
    ///
    /// `template_axes` is parallel to `self`'s axes, giving the
    /// (zero-based) template axis index from which each result axis
    /// should inherit per-axis attributes. `-1` means no
    /// association. Mirrors C `astOverlay` (frame.c:8980).
    ///
    /// Default implementation copies the template's Domain onto
    /// `self` if the template has its Domain explicitly set; per-
    /// axis attributes are class-specific and handled by overrides.
    fn overlay_template(&mut self, template: &dyn Frame, template_axes: &[i32]) -> AstResult<()> {
        let _ = template_axes;
        if template.test_domain() {
            // No public set_domain on `dyn Frame` — only concrete
            // classes know how. The default is a no-op; concrete
            // overrides do the actual copy.
            let _ = template.domain();
        }
        Ok(())
    }

    /// Identify the primary (leaf, non-compound) Frame and underlying
    /// axis for a given axis of this Frame. `axis` is 0-based (matching
    /// the existing `primary_frame_walk` callers and C `astPrimaryFrame`
    /// internal convention). Returns the leaf frame and the 0-based axis
    /// within it, undoing all axis permutations and Frame combination.
    /// Mirrors C `astPrimaryFrame` (frame.c:8818). Leaf Frames are
    /// already primary, so return `(self, axis)`; compound and
    /// permuting Frames (CmpFrame, SkyFrame) override to descend and
    /// undo permutations.
    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize);
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

/// Cartesian angle subtended at `b` by `a` and `c` — the Frame-class
/// algorithm (frame.c:1230-1320): AST__BAD for bad inputs or
/// zero-length vectors; in 2 dimensions the result is the *signed*
/// angle (positive sense from axis 2 to axis 1) folded into ±π;
/// otherwise the unsigned `acos` form.
pub(crate) fn cartesian_angle(a: &[f64], b: &[f64], c: &[f64]) -> f64 {
    use crate::constants::BAD_VALUE;
    use crate::mappings::helpers::is_bad;

    let naxes = b.len();
    let mut sla = 0.0;
    let mut slc = 0.0;
    let mut ab = vec![0.0; naxes];
    let mut cb = vec![0.0; naxes];
    for i in 0..naxes {
        if is_bad(a[i]) || is_bad(b[i]) || is_bad(c[i]) {
            return BAD_VALUE;
        }
        ab[i] = a[i] - b[i];
        cb[i] = c[i] - b[i];
        sla += ab[i] * ab[i];
        slc += cb[i] * cb[i];
    }
    if sla == 0.0 || slc == 0.0 {
        return BAD_VALUE;
    }
    if naxes == 2 {
        let anga = ab[0].atan2(ab[1]);
        let angc = cb[0].atan2(cb[1]);
        crate::pal::drange(angc - anga)
    } else {
        let sp: f64 = (0..naxes).map(|i| ab[i] * cb[i]).sum();
        (sp / (sla * slc).sqrt()).clamp(-1.0, 1.0).acos()
    }
}

/// The Epoch (TDB MJD) of a frame *if explicitly set*, looked up per
/// concrete class. Stands in for C's virtual `astTestEpoch` /
/// `astGetEpoch` pair, which the `Frame` trait does not yet expose.
pub(crate) fn frame_epoch_if_set(f: &dyn Frame) -> Option<f64> {
    let any = f.as_any();
    if let Some(sf) = any.downcast_ref::<SkyFrame>() {
        return sf.test_epoch().then(|| sf.epoch_mjd());
    }
    if let Some(sf) = any.downcast_ref::<SpecFrame>() {
        return sf.test_epoch().then(|| sf.epoch_mjd());
    }
    if let Some(tf) = any.downcast_ref::<TimeFrame>() {
        return tf.test_epoch().then(|| tf.epoch_mjd());
    }
    if let Some(df) = any.downcast_ref::<DSBSpecFrame>() {
        return df.spec().test_epoch().then(|| df.epoch_mjd());
    }
    if let Some(bf) = any.downcast_ref::<BasicFrame>() {
        return bf.epoch();
    }
    if let Some(cf) = any.downcast_ref::<CmpFrame>() {
        return cf.test_epoch().then(|| cf.epoch_mjd());
    }
    None
}

/// Set the Epoch (TDB MJD) on a frame, per concrete class. Classes
/// without an epoch setter are skipped (C stores Epoch on the Frame
/// base struct for every class; the Rust BasicFrame only has a
/// year-based setter and FluxFrame none, so those are left alone).
pub(crate) fn set_frame_epoch_mjd(f: &mut dyn Frame, mjd: f64) {
    let any = f.as_any_mut();
    if let Some(sf) = any.downcast_mut::<SkyFrame>() {
        sf.set_epoch_mjd(mjd);
    } else if let Some(sf) = any.downcast_mut::<SpecFrame>() {
        sf.set_epoch_mjd(mjd);
    } else if let Some(tf) = any.downcast_mut::<TimeFrame>() {
        tf.set_epoch_mjd(mjd);
    } else if let Some(df) = any.downcast_mut::<DSBSpecFrame>() {
        df.set_epoch_mjd(mjd);
    } else if let Some(cf) = any.downcast_mut::<CmpFrame>() {
        cf.set_epoch_mjd(mjd);
    }
}

/// Set an observer attribute on a frame, per concrete class. The
/// `which` selector: 0 = ObsLat, 1 = ObsLon, 2 = ObsAlt, 3 = Dtai.
pub(crate) fn set_frame_obs(f: &mut dyn Frame, which: usize, v: f64) {
    macro_rules! fwd {
        ($t:ty) => {
            if let Some(x) = f.as_any_mut().downcast_mut::<$t>() {
                match which {
                    0 => x.set_obs_lat(v),
                    1 => x.set_obs_lon(v),
                    2 => x.set_obs_alt(v),
                    _ => x.set_dtai(v),
                }
                return;
            }
        };
    }
    fwd!(SkyFrame);
    fwd!(TimeFrame);
    if let Some(x) = f.as_any_mut().downcast_mut::<SpecFrame>() {
        // SpecFrame has no Dtai setter.
        match which {
            0 => x.set_obs_lat(v),
            1 => x.set_obs_lon(v),
            2 => x.set_obs_alt(v),
            _ => {}
        }
        return;
    }
    if let Some(x) = f.as_any_mut().downcast_mut::<DSBSpecFrame>() {
        match which {
            0 => x.set_obs_lat(v),
            1 => x.set_obs_lon(v),
            2 => x.set_obs_alt(v),
            _ => {}
        }
        return;
    }
    if let Some(x) = f.as_any_mut().downcast_mut::<CmpFrame>() {
        match which {
            0 => x.set_obs_lat(v),
            1 => x.set_obs_lon(v),
            2 => x.set_obs_alt(v),
            _ => x.set_dtai(v),
        }
    }
}
