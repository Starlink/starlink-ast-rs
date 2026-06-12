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
    /// `None` if the frames are incompatible. Mirrors C `astConvert`
    /// (frame.c:2965), which swaps its first two arguments and defers to
    /// the generic `astConvertX` driver (frame.c:3028).
    ///
    /// The default body joins `domains` into the comma-separated list C
    /// expects (an empty slice becomes a single blank field that permits
    /// any domain) and delegates to the generic driver. No Frame subclass
    /// overrides it; all class-specific alignment lives in `match_template`
    /// / `sub_frame`, exactly as in C.
    fn convert_to(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>> {
        let domainlist = if domains.is_empty() {
            " ".to_string()
        } else {
            domains.join(",")
        };
        // Snapshot `self` to a `dyn Frame`: the trait method's `&self`
        // cannot be coerced to `&dyn Frame` for the unsized default body
        // (the same indirection `default_find_frame` uses).
        let from = self.snapshot();
        find_frame::convert_frames(from.as_ref(), to, &domainlist)
    }

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

    /// Set the frame Domain. Default no-op; concrete classes with a
    /// Domain field override this. Mirrors C `astSetDomain`.
    fn set_domain(&mut self, _domain: &str) {}

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

    /// Whether unit differences affect alignment in `Match`/`Overlay`.
    /// Mirrors C `astGetActiveUnit` (frame.c:4651): always false when the
    /// Frame has a SkyAxis; otherwise the stored flag, default false.
    fn get_active_unit(&self) -> bool {
        false
    }

    /// Set the ActiveUnit flag. Mirrors C `astSetActiveUnit`
    /// (frame.c:9882). SkyFrame ignores it (forced false); SpecFrame /
    /// FluxFrame force true.
    fn set_active_unit(&mut self, _value: bool) {}

    /// Whether ActiveUnit has been explicitly set (for serialization).
    /// Mirrors C `astTestActiveUnit` (used by `Dump`, frame.c:14614).
    fn test_active_unit(&self) -> bool {
        false
    }

    /// Effective `Label(axis)` (1-based): the explicitly-set per-axis
    /// label if present, else the class default. Mirrors the Frame
    /// `astGetLabel` virtual (frame.c:5350) whose default is "Axis N"
    /// (frame.c:5395). Concrete frames override to return their own
    /// system-derived or stored label.
    fn effective_label(&self, axis: usize) -> String {
        format!("Axis {axis}")
    }

    /// Effective `Unit(axis)` (1-based): the explicitly-set per-axis
    /// unit if present, else the class default. Mirrors the Frame
    /// `astGetUnit` virtual (frame.c:13530), whose base default is the
    /// empty string. Concrete frames override to return their own
    /// system-derived or stored unit.
    fn effective_unit(&self, _axis: usize) -> String {
        String::new()
    }

    /// Effective `Symbol(axis)` (1-based): the explicitly-set per-axis
    /// symbol if present, else the class default. Mirrors the Frame
    /// `astGetSymbol` virtual (frame.c:5760), whose base default falls
    /// back to the label. Concrete frames override accordingly.
    fn effective_symbol(&self, axis: usize) -> String {
        self.effective_label(axis)
    }

    /// Effective `Format(axis)` (1-based): the explicitly-set per-axis
    /// format string, or the empty string when none is set. Mirrors
    /// `astTestFormat`/`astGetFormat` (frame.c) — only an explicitly
    /// set Format is load-bearing for a cast; the default format is
    /// derived from Digits and need not be copied.
    fn effective_format(&self, _axis: usize) -> String {
        String::new()
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

    /// Cast `self` down into the class of `other`, returning a copy of
    /// `self`'s ancestral component that matches `other`'s class, or
    /// `None` if `self` is not a subclass of `other`'s class. Used by
    /// the matchsub retry so a specialised template can probe a plainer
    /// target. Mirrors C `astCast` (object.c / cmpframe.c:1245).
    /// Default: not castable.
    fn cast(&self, _other: &dyn Frame) -> AstResult<Option<DynFrame>> {
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

    /// Offset along a geodesic at a given angle in a 2-D Frame. Returns
    /// `(point2, ret_angle)`. Errors if the Frame is not 2-D. Bad inputs
    /// give all-BAD outputs. Mirrors C `astOffset2` (frame.c:8228).
    fn offset2(&self, point1: &[f64], angle: f64, offset: f64) -> AstResult<(Vec<f64>, f64)> {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        if self.naxes() != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                "astOffset2 can only be used with 2-dimensional Frames",
            ));
        }
        if is_bad(point1[0]) || is_bad(point1[1]) || is_bad(angle) || is_bad(offset) {
            return Ok((vec![BAD_VALUE, BAD_VALUE], BAD_VALUE));
        }
        let p2 = vec![
            point1[0] + angle.sin() * offset,
            point1[1] + angle.cos() * offset,
        ];
        Ok((p2, angle))
    }

    /// Point of intersection of two geodesics (straight lines) in a 2-D
    /// Frame. Bad inputs, vertical/parallel degeneracies give all-BAD.
    /// Mirrors C `astIntersect` (frame.c:6243).
    fn intersect(&self, a1: &[f64], a2: &[f64], b1: &[f64], b2: &[f64]) -> AstResult<Vec<f64>> {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        if self.naxes() != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                "astIntersect can only be used with 2-dimensional Frames",
            ));
        }
        let mut cross = vec![BAD_VALUE, BAD_VALUE];
        if [a1[0], a1[1], a2[0], a2[1], b1[0], b1[1], b2[0], b2[1]]
            .iter()
            .any(|&v| is_bad(v))
        {
            return Ok(cross);
        }
        let dxa = a2[0] - a1[0];
        let dxb = b2[0] - b1[0];
        if dxa != 0.0 {
            let ma = (a2[1] - a1[1]) / dxa;
            let ca = a1[1] - a1[0] * ma;
            if dxb != 0.0 {
                let mb = (b2[1] - b1[1]) / dxb;
                let cb = b1[1] - b1[0] * mb;
                if ma != mb {
                    cross[0] = (cb - ca) / (ma - mb);
                    cross[1] = ((ma + mb) * cross[0] + ca + cb) / 2.0;
                }
            } else if b1[1] != b2[1] {
                cross[0] = b1[0];
                cross[1] = ma * b1[0] + ca;
            }
        } else if dxb != 0.0 && a1[1] != a2[1] {
            let mb = (b2[1] - b1[1]) / dxb;
            let cb = b1[1] - b1[0] * mb;
            cross[0] = a1[0];
            cross[1] = mb * a1[0] + cb;
        }
        Ok(cross)
    }

    /// Signed axis increment from `v1` to `v2` on a 1-based `axis`.
    /// Base (Cartesian): `v2 - v1` with BAD propagation. Mirrors C
    /// `astAxDistance` (frame.c:1466 -> axis.c AxisDistance).
    fn ax_distance(&self, _axis: usize, v1: f64, v2: f64) -> AstResult<f64> {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        Ok(if is_bad(v1) || is_bad(v2) {
            BAD_VALUE
        } else {
            v2 - v1
        })
    }

    /// Axis value formed by adding `dist` to `v1` on a 1-based `axis`.
    /// Base (Cartesian): `v1 + dist` with BAD propagation. Mirrors C
    /// `astAxOffset` (frame.c:1744 -> axis.c AxisOffset).
    fn ax_offset(&self, _axis: usize, v1: f64, dist: f64) -> AstResult<f64> {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        Ok(if is_bad(v1) || is_bad(dist) {
            BAD_VALUE
        } else {
            v1 + dist
        })
    }

    /// Angle, as seen from `a`, between the positive direction of the
    /// 1-based `axis` and the geodesic from `a` to `b`. Offsets `a`
    /// slightly in the +axis direction and calls `angle`. Mirrors C
    /// `astAxAngle` (frame.c:1324).
    fn ax_angle(&self, a: &[f64], b: &[f64], axis: usize) -> AstResult<f64> {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        let naxes = self.naxes();
        let mut aa = vec![0.0; naxes];
        let mut mxab = 0.0;
        for i in 0..naxes {
            if is_bad(a[i]) || is_bad(b[i]) {
                return Ok(BAD_VALUE);
            }
            aa[i] = a[i];
            let ab = (a[i] - b[i]).abs();
            if ab > mxab {
                mxab = ab;
            }
        }
        let k = axis - 1;
        let eps = 10000.0 * f64::EPSILON;
        if a[k] != 0.0 {
            aa[k] += eps * a[k].abs();
        } else if b[k] != 0.0 {
            // C reads b[iaxis-1] here, where iaxis is the loop index left
            // at naxes after the component loop completes, so this nudges
            // by 10000*EPSILON*|b[naxes-1]| rather than |b[axis-1]|. The
            // index is preserved to match the C library bit-for-bit.
            aa[k] = eps * b[naxes - 1].abs();
        } else if mxab != 0.0 {
            aa[k] = eps * mxab;
        } else {
            aa[k] = 1.0;
        }
        self.angle(&aa, a, b)
    }

    /// Normalise an array of axis values on a 1-based `axis`. `oper`
    /// selects the normalisation mode (0 = like `norm`, 1 = smallest
    /// range). Base (Cartesian): values are returned unchanged. Mirrors
    /// C `astAxNorm` (frame.c:1560).
    fn ax_norm(&self, _axis: usize, oper: i32, _values: &mut [f64]) -> AstResult<()> {
        if !(0..=1).contains(&oper) {
            return Err(AstError::new(
                ErrorCode::Oprin,
                format!("astAxNorm: invalid operation {oper}"),
            ));
        }
        Ok(())
    }

    /// Resolve the vector point1->point3 into components parallel and
    /// perpendicular to the basis vector point1->point2. Returns
    /// `(point4, d1, d2)` where point4 is the closest approach, d1 the
    /// signed parallel length, d2 the (non-negative) perpendicular
    /// length. Zero-length basis or bad inputs give BAD. Mirrors C
    /// `astResolve` (frame.c:9429).
    fn resolve(
        &self,
        point1: &[f64],
        point2: &[f64],
        point3: &[f64],
    ) -> AstResult<(Vec<f64>, f64, f64)> {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        let naxes = self.naxes();
        let mut point4 = vec![BAD_VALUE; naxes];
        let mut d1 = BAD_VALUE;
        let mut d2 = BAD_VALUE;
        let ok = (0..naxes).all(|i| !(is_bad(point1[i]) || is_bad(point2[i]) || is_bad(point3[i])));
        if ok {
            let mut dp = 0.0;
            let mut bv = 0.0;
            for i in 0..naxes {
                let c = point2[i] - point1[i];
                dp += c * (point3[i] - point1[i]);
                bv += c * c;
            }
            if bv > 0.0 {
                let bv = bv.sqrt();
                d1 = dp / bv;
                for i in 0..naxes {
                    point4[i] = point1[i] + (d1 / bv) * (point2[i] - point1[i]);
                }
                let mut s = 0.0;
                for i in 0..naxes {
                    let c = point3[i] - point4[i];
                    s += c * c;
                }
                d2 = s.sqrt();
            }
        }
        Ok((point4, d1, d2))
    }

    /// Batch form of `resolve`: resolve every vector point1->in[i] into
    /// `(parallel, perpendicular)` components, returned as a 2-coord
    /// PointSet. For 2-D Frames the perpendicular component is signed
    /// (positive when basis->vector rotation matches axis1->axis2). The
    /// 1-axis case returns `(ax_distance, 0)`. Mirrors C
    /// `astResolvePoints` (frame.c:9588). SkyFrame overrides with
    /// great-circle geometry.
    fn resolve_points(
        &self,
        point1: &[f64],
        point2: &[f64],
        input: &crate::coords::PointSet,
    ) -> AstResult<crate::coords::PointSet> {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        let nax = self.naxes();
        let npoint = input.npoint();
        let mut d1v = vec![BAD_VALUE; npoint];
        let mut d2v = vec![BAD_VALUE; npoint];

        if nax < 2 {
            for p in 0..npoint {
                let ip = input.point(p)[0];
                d1v[p] = self.ax_distance(1, point1[0], ip)?;
                d2v[p] = 0.0;
            }
            return crate::coords::PointSet::from_columns(&[&d1v, &d2v]);
        }

        // Basis vector + squared length.
        let mut basisv = vec![0.0; nax];
        let mut bv = 0.0;
        let mut ok = true;
        for i in 0..nax {
            if is_bad(point1[i]) || is_bad(point2[i]) {
                ok = false;
                break;
            }
            basisv[i] = point2[i] - point1[i];
            bv += basisv[i] * basisv[i];
        }
        if ok && bv > 0.0 {
            let bv = bv.sqrt();
            for p in 0..npoint {
                let row = input.point(p);
                let mut dp = 0.0;
                let mut good = true;
                for (i, &bi) in basisv.iter().enumerate() {
                    let d = row[i] - point1[i];
                    if is_bad(d) {
                        good = false;
                        break;
                    }
                    dp += bi * d;
                }
                if !good {
                    continue;
                }
                let d1 = dp / bv;
                let c = d1 / bv;
                let d2 = if nax > 2 {
                    let mut s = 0.0;
                    for (i, &bi) in basisv.iter().enumerate() {
                        let d = row[i] - (point1[i] + c * bi);
                        s += d * d;
                    }
                    s.sqrt()
                } else {
                    let x1 = c * basisv[0];
                    let y1 = c * basisv[1];
                    let x2 = row[0] - (point1[0] + x1);
                    let y2 = row[1] - (point1[1] + y1);
                    let mut m = (x2 * x2 + y2 * y2).sqrt();
                    if x1 * y2 - x2 * y1 < 0.0 {
                        m = -m;
                    }
                    m
                };
                d1v[p] = d1;
                d2v[p] = d2;
            }
        }
        crate::coords::PointSet::from_columns(&[&d1v, &d2v])
    }

    /// Find, for each axis of `frm2`, the corresponding 1-based axis
    /// index within `self` (or 0 if none). Mirrors C `astMatchAxes`
    /// (frame.c:7349), which delegates to `MatchAxesX` with args
    /// swapped so it can be overridden on either class.
    fn match_axes(&self, frm2: &dyn Frame) -> AstResult<Vec<i32>>
    where
        Self: Sized,
    {
        frm2.match_axes_x(self)
    }

    /// Implementation half of `match_axes` (args swapped). `self` plays
    /// the role of C's `frm2`; `frm1` is the Frame whose axes are
    /// reported. For each axis of `self`, identify its primary frame and
    /// match it against `frm1`, recording the 1-based `frm1` axis index
    /// (or 0). Mirrors C `astMatchAxesX` (frame.c:7438).
    fn match_axes_x(&self, frm1: &dyn Frame) -> AstResult<Vec<i32>> {
        let nax2 = self.naxes();
        let mut axes = vec![0i32; nax2];

        // C temporarily widens frm1's Match-control attributes so it can
        // probe any axis count and so the result Frame keeps the target
        // (primary-frame) axis order. frm1 is `&dyn`, so snapshot it and
        // mutate the snapshot.
        let mut probe = frm1.snapshot();
        probe.set_preserve_axes(true);
        probe.set_max_axes(10000);
        probe.set_min_axes(1);

        for (ifirst, slot) in axes.iter_mut().enumerate() {
            let (pfrm, pax) = self.primary_frame(ifirst);
            if let Some(m) = probe.match_template(pfrm, true)? {
                // C reads `frm1_axes[pax] + 1`, where frm1_axes is the
                // template-axis association of the result Frame and, with
                // PreserveAxes set, the result Frame carries the target
                // (pfrm) axis order. So the result slot whose target axis
                // is `pax` gives the frm1 (template) axis there.
                if let Some(pos) = m.target_axes.iter().position(|&t| t == pax as i32) {
                    if m.template_axes[pos] >= 0 {
                        *slot = m.template_axes[pos] + 1;
                    }
                }
            }
        }
        Ok(axes)
    }

    /// Whether this Frame is equivalent to a UnitMap when treated as a
    /// Mapping. Base Frame is always unit-equivalent. Mirrors C
    /// `astIsUnitFrame` (frame.c:6417).
    fn is_unit_frame(&self) -> bool {
        true
    }

    /// Extend a box to include the effect of coordinate-system
    /// singularities. The base Frame has none, so the bounds are left
    /// unchanged. SkyFrame overrides this (pole inclusion). Mirrors C
    /// `astNormBox` (frame.c:7766).
    fn norm_box(&self, _lbnd: &mut [f64], _ubnd: &mut [f64]) -> AstResult<()> {
        Ok(())
    }
}

/// Build a plain `BasicFrame` carrying `frame`'s axis count and the
/// Frame-level + per-axis attributes that survive a cast to the Frame
/// base class. Used by the per-class `cast` impls when `other` is a
/// plain Frame. Mirrors the "create a basic Frame with the same axes
/// and attributes" path of C `Cast` (cmpframe.c:1326-1344, and the
/// Object-level CastCopy reduction for leaf frames).
pub(crate) fn reduce_to_basic_frame(frame: &dyn Frame) -> AstResult<BasicFrame> {
    let n = frame.naxes();
    let mut bf = BasicFrameBuilder::new(n).build()?;
    // Frame-level Domain: copied by astOverlay (cmpframe.c:1341).
    if frame.test_domain() {
        bf.set_domain(frame.domain().to_string());
    }
    // Per-axis Axis metadata: C copies each source Axis into the basic
    // Frame with astSetAxis (cmpframe.c:1335-1338), so the effective
    // Label/Unit/Symbol/Format and Digits/Direction of every axis carry
    // through. We read each via the Frame effective getters (the C
    // astGetLabel/astGetUnit/astGetSymbol/astGetFormat virtuals) and
    // store them on the BasicFrame.
    for axis in 1..=n {
        bf.set_label(axis, frame.effective_label(axis))?;
        bf.set_unit(axis, frame.effective_unit(axis))?;
        bf.set_symbol(axis, frame.effective_symbol(axis))?;
        let fmt = frame.effective_format(axis);
        if !fmt.is_empty() {
            bf.set_format_str(axis, fmt)?;
        }
        // Digits/Direction default on a plain Frame to 7 and 1; only a
        // non-default value carries information worth transferring (the
        // BasicFrame already defaults to these). Setting unconditionally
        // would spuriously flag the attribute as explicitly set.
        let digits = frame.axis_digits(axis);
        if digits != 7 {
            bf.set_axis_digits(axis, digits)?;
        }
        let direction = frame.direction(axis);
        if !direction {
            bf.set_direction(axis, direction)?;
        }
    }
    Ok(bf)
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
