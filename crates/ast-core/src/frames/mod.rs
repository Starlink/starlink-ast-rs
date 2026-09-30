//! The Frame trait and associated types.

pub(crate) mod axis;
pub(crate) mod axis_format;
mod basic;
mod cmpframe;
pub(crate) mod date_time;
mod dsbspecframe;
pub(crate) mod find_frame;
mod fluxframe;
pub(crate) mod frame_attrs;
mod frameset;
pub(crate) mod sky_convert;
pub(crate) mod skyaxis;
mod skyframe;
mod specfluxframe;
pub(crate) mod specframe;
mod timeframe;
pub use basic::{BasicFrame, BasicFrameBuilder, FrameSystem};
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
use crate::mappings::{AsAny, DynMapping, Mapping, PermMap};
use crate::serial::AnyObject;

/// An owned, dynamically-dispatched Frame.
pub type DynFrame = Box<dyn Frame>;

/// The Axis object attached to one axis of a [`Frame`].
///
/// C's Frame holds an array of `Axis` objects, one per axis, and reaches
/// per-axis behaviour by fetching one with `astGetAxis` and calling an
/// `astAxis*` method on it (`frame.c:1466` for `astAxDistance`, and likewise
/// for `astAxOffset` and `astAxNorm`). The Axis class matters: a `SkyAxis`
/// wraps values into a cyclic range where a plain `Axis` does not.
///
/// Crucially the axes of one Frame need not share a class. `CmpFrame` routes
/// each axis to its owning component (`cmpframe.c:3474`), and `SubFrame`
/// re-uses the target's Axis objects on the result (`frame.c:11023`), so a sky
/// axis picked out of a `SkyFrame` stays a sky axis even when it ends up
/// alongside a plain axis on a `CmpFrame`. This enum is that per-axis fact.
///
/// Where C hands out a counted reference to the Axis object, this carries the
/// attributes that the `astAxis*` methods read, already resolved to their
/// effective values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisKind {
    /// A plain Cartesian axis, with no constraint on its values. The
    /// behaviour of C's base `Axis` class (`axis.c`).
    Plain,
    /// A celestial axis, whose values wrap into a cyclic range. The
    /// behaviour of C's `SkyAxis` class (`skyaxis.c`).
    Sky {
        /// True for a latitude axis, which is always wrapped into
        /// `[-PI,PI]`. Mirrors the `IsLatitude` Axis attribute.
        is_latitude: bool,
        /// True when the axis range is centred on zero (`[-PI,PI]`) rather
        /// than on PI (`[0,2*PI]`). Mirrors the `CentreZero` Axis attribute,
        /// whose default is `IsLatitude` (`skyaxis.c:4620`).
        centre_zero: bool,
    },
}

impl AxisKind {
    /// Signed increment from `v1` to `v2` along this axis.
    ///
    /// Mirrors `astAxisDistance`: `axis.c:353` for a plain axis (`v2 - v1`),
    /// `skyaxis.c:412` for a sky axis (the same difference wrapped into
    /// +PI to -PI).
    pub fn distance(&self, v1: f64, v2: f64) -> f64 {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        if is_bad(v1) || is_bad(v2) {
            return BAD_VALUE;
        }
        match self {
            Self::Plain => v2 - v1,
            Self::Sky { .. } => crate::pal::drange(v2 - v1),
        }
    }

    /// The value reached by adding `dist` to `v1` along this axis.
    ///
    /// Mirrors `astAxisOffset`: `axis.c:1165` for a plain axis (`v1 + dist`),
    /// `skyaxis.c:1300` for a sky axis (the same sum, then normalised).
    pub fn offset(&self, v1: f64, dist: f64) -> f64 {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        if is_bad(v1) || is_bad(dist) {
            return BAD_VALUE;
        }
        let mut result = v1 + dist;
        self.norm(&mut result);
        result
    }

    /// Wrap `value` into this axis's canonical range.
    ///
    /// Mirrors `astAxisNorm`: `axis.c:1070` for a plain axis (no constraints,
    /// so no action), `skyaxis.c:1057` for a sky axis.
    pub fn norm(&self, value: &mut f64) {
        if let Self::Sky { centre_zero, .. } = self {
            if !crate::mappings::helpers::is_bad(*value) {
                *value = if *centre_zero {
                    crate::pal::drange(*value)
                } else {
                    crate::pal::dranrm(*value)
                };
            }
        }
    }

    /// Normalise `values` in the manner selected by `oper`.
    ///
    /// `oper` 0 normalises each value exactly as [`norm`](Self::norm) does.
    /// `oper` 1 chooses, for a sky longitude axis, whichever of `[0,2*PI]` and
    /// `[-PI,PI]` leaves the list with the smaller range; a latitude axis is
    /// always wrapped into `[-PI,PI]`.
    ///
    /// Mirrors `astAxisNormValues`: `axis.c:1115` for a plain axis (no action),
    /// `skyaxis.c:1118` for a sky axis.
    pub fn norm_values(&self, oper: i32, values: &mut [f64]) {
        let Self::Sky {
            is_latitude,
            centre_zero,
        } = *self
        else {
            return;
        };

        // C `VAL_OK` (skyaxis.c:1169): bad values, and values outside -1E6 to
        // +1E6 radians, are returned unchanged, which keeps DBL_MAX-style
        // sentinels intact.
        fn val_ok(v: f64) -> bool {
            !crate::mappings::helpers::is_bad(v) && v.abs() < 1.0e6
        }

        // Oper 0 mirrors AxisNorm, so it follows CentreZero.
        if oper == 0 {
            for v in values.iter_mut() {
                if val_ok(*v) {
                    *v = if centre_zero {
                        crate::pal::drange(*v)
                    } else {
                        crate::pal::dranrm(*v)
                    };
                }
            }
            return;
        }

        // Latitude axes always normalise into [-PI,+PI].
        if is_latitude {
            for v in values.iter_mut() {
                if val_ok(*v) {
                    *v = crate::pal::drange(*v);
                }
            }
            return;
        }

        // Longitude axes: bring every value into [-PI,2*PI] and find the
        // extremes within each of [-PI,0], [0,PI] and [PI,2PI], so the range
        // each candidate normalisation would produce can be compared.
        let two_pi = 2.0 * std::f64::consts::PI;
        let (mut mx1, mut mn1) = (-f64::MAX, f64::MAX);
        let (mut mx2, mut mn2) = (-f64::MAX, f64::MAX);
        let (mut mx3, mut mn3) = (-f64::MAX, f64::MAX);

        for v in values.iter_mut() {
            if !val_ok(*v) {
                continue;
            }
            while *v > two_pi {
                *v -= two_pi;
            }
            while *v < -std::f64::consts::FRAC_PI_2 {
                *v += two_pi;
            }
            if *v > std::f64::consts::PI {
                mx3 = mx3.max(*v);
                mn3 = mn3.min(*v);
            } else if *v > 0.0 {
                mx2 = mx2.max(*v);
                mn2 = mn2.min(*v);
            } else {
                mx1 = mx1.max(*v);
                mn1 = mn1.min(*v);
            }
        }

        // The range that normalising into [0,2.PI] would give.
        let (hi, lo) = if mx1 != -f64::MAX {
            (
                mx2.max((mx1 + two_pi).max(mx3)),
                mn2.min((mn1 + two_pi).min(mn3)),
            )
        } else {
            (mx2.max(mx3), mn2.min(mn3))
        };
        let range1 = hi - lo;

        // The range that normalising into [-PI,PI] would give. C tests
        // `mn3 != -DBL_MAX` (skyaxis.c:1266) where `mn3` starts at `+DBL_MAX`,
        // so the test always passes; it is kept because both branches agree —
        // the unset extremes saturate the max/min either way.
        let (hi, lo) = if mn3 != -f64::MAX {
            (
                mx2.max((mx3 - two_pi).max(mx1)),
                mn2.min((mn3 - two_pi).min(mn1)),
            )
        } else {
            (mx2.max(mx1), mn2.min(mn1))
        };
        let range2 = hi - lo;

        // Whichever candidate gives the smaller range wins; a tie keeps
        // [0,2PI], as C's `<` comparison does.
        if range2 < range1 {
            for v in values.iter_mut() {
                if val_ok(*v) {
                    *v = crate::pal::drange(*v);
                }
            }
        } else {
            for v in values.iter_mut() {
                if val_ok(*v) {
                    *v = crate::pal::dranrm(*v);
                }
            }
        }
    }
}

/// [`Frame::simplify_frame`] for a class that is both a Frame and a Mapping.
///
/// C has one `Simplify` slot, so `astSimplify` on a Region or a FrameSet runs
/// that class's own Mapping-level reduction and the caller then asks whether
/// what came back is still a Frame (`astIsAFrame( map1 )`,
/// `cmpframe.c:9024`). A Region always simplifies to a Region, but a
/// FrameSet's reduction can hand back a plain Mapping, which is the case that
/// test exists for; here it is the `into_frame` conversion that drops it.
///
/// "No simplification took place" is
/// [`simplify_reporting_change`](crate::mappings::Mapping)'s answer rather than
/// a fresh comparison: C's identity test on the returned pointer asks for the
/// same fact the engine already reports.
pub(crate) fn simplify_frame_via_mapping(m: &dyn Mapping) -> AstResult<Option<DynFrame>> {
    let (simplified, changed) = m.simplify_reporting_change()?;
    if !changed {
        return Ok(None);
    }
    // Every class that implements `Frame` has an `AnyObject` variant, and
    // `any_object_from_mapping_covers_as_mapping` keeps that true, so a `None`
    // here is a Mapping-valued result rather than a missing case.
    Ok(AnyObject::from_mapping(simplified.as_ref()).and_then(|o| o.into_frame()))
}

/// A cached description of a line segment within a 2-D [`Frame`].
///
/// Mirrors C `AstLineDef` (frame.h:646). Created by [`Frame::line_def`] and
/// consumed by [`Frame::line_crossing`] / [`Frame::line_offset`], it caches
/// the intermediate values (length, unit direction, unit normal) that those
/// methods reuse. For the base (planar) Frame the geometry is flat; SkyFrame
/// overrides these methods with great-circle geometry (and stores extra
/// spherical state, which is not represented here).
#[derive(Debug, Clone)]
pub struct LineDef {
    /// Length of the line segment (geodesic distance start→end). For a planar
    /// Frame this is the Cartesian distance; for a SkyFrame it is the
    /// great-circle arc length in radians.
    pub length: f64,
    /// When true, the start and end points are disregarded and the line is
    /// treated as extending from `start` for an infinite distance.
    pub infinite: bool,
    /// Frame axis values at the line start.
    pub start: [f64; 2],
    /// Frame axis values at the line end.
    pub end: [f64; 2],
    /// Unit vector pointing from start to end.
    pub dir: [f64; 2],
    /// Unit vector perpendicular to the line (dir rotated +90°).
    pub q: [f64; 2],
    /// Great-circle geometry for a line defined within a SkyFrame. `None` for
    /// planar Frames. Mirrors C `SkyLineDef` (skyframe.c:814), which is a
    /// distinct struct from the planar `AstLineDef`; here the spherical state
    /// is carried alongside the planar fields and the SkyFrame line methods
    /// use it in place of the planar geometry.
    pub sky: Option<SkyLineDef>,
}

/// Great-circle description of a line segment within a SkyFrame.
///
/// Mirrors C `SkyLineDef` (skyframe.c:814): positions are held as 3-D unit
/// vectors rather than (lon, lat) pairs. `start`/`end` are the segment
/// end-points; `dir` is the point 90° along the great circle from `start`
/// (so that `q`, `dir`, `start` form the spherical analogue of the planar
/// direction/normal); `q` is the pole of the great circle, oriented so the
/// pole lies to the left of the line travelling start→end.
#[derive(Debug, Clone)]
pub struct SkyLineDef {
    /// Unit vector at the line start.
    pub start: [f64; 3],
    /// Unit vector at the line end.
    pub end: [f64; 3],
    /// Unit vector 90° along the great circle from `start` (the "quarter
    /// point"), used to disambiguate forward vs reverse arc direction.
    pub dir: [f64; 3],
    /// Unit vector normal to the great-circle plane (its pole).
    pub q: [f64; 3],
}

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
    ///
    /// The default mirrors C base `astFormat` → `AxisFormat` (axis.c): the
    /// `<bad>` token for the bad value, otherwise the effective per-axis
    /// Format string (set value, else the default `"%1.<Digits>G"` with the
    /// per-axis Digits) rendered by `axis_format::format_value`. Leaf Frame
    /// classes that do not override Format in C (SpecFrame, FluxFrame,
    /// DSBSpecFrame) inherit this; CmpFrame, TimeFrame and SkyFrame override.
    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        if axis < 1 || axis > self.naxes() {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("axis index {axis} is out of range 1..={}", self.naxes()),
            ));
        }
        // C `AxisFormat` returns "<bad>" for the bad value (axis.c:704).
        if crate::mappings::helpers::is_bad(value) {
            return Ok("<bad>".to_string());
        }
        // Resolve the effective Format string: the per-axis Format if set,
        // else the default "%1.<Digits>G" with the per-axis Digits
        // (frame.c:4260 → GetDefaultFormat axis.c:1819).
        let digits = self.axis_digits(axis).max(1) as usize;
        let fmt = match self.effective_format(axis) {
            s if !s.is_empty() => s,
            _ => format!("%1.{digits}G"),
        };
        Ok(crate::frames::axis_format::format_value(
            &fmt, digits, value,
        ))
    }

    /// Parse a formatted coordinate string back to a numeric value.
    ///
    /// `axis` is 1-based. Returns `(value, characters_consumed)`.
    ///
    /// The default mirrors C base `astUnformat` (frame.c:11819): validate the
    /// axis, then apply the `astAxisUnformat` contract (axis.c:1284) — the
    /// longest `"%lf %n"` numeric prefix (counting trailing whitespace), the
    /// `<bad>` token, else `(BAD_VALUE, 0)` with no error. Leaf Frame classes
    /// that do not override Unformat in C (SpecFrame, FluxFrame, DSBSpecFrame,
    /// SpecFluxFrame) inherit this; CmpFrame, TimeFrame and SkyFrame override.
    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        if axis < 1 || axis > self.naxes() {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("axis index {axis} is out of range 1..={}", self.naxes()),
            ));
        }
        Ok(crate::frames::basic::axis_unformat(text))
    }

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

    /// C `astSimplify` applied to a Frame — the Mapping `Simplify` slot every
    /// Frame inherits (`frame.c` defines none of its own, so a plain Frame gets
    /// `mapping.c:16853`, which returns a clone).
    ///
    /// `None` means no simplification took place, which is what C signals by
    /// returning a clone of the input pointer: the caller's identity test
    /// (`smap != map->frame`, `normmap.c:626`) is a `None` check here.
    ///
    /// The default is `None`: a Frame with no internal structure has nothing to
    /// reduce. [`CmpFrame`] overrides it (`cmpframe.c:9018-9034`), as does every
    /// class that is both a Frame and a Mapping — each `Region` and `FrameSet`,
    /// through `simplify_frame_via_mapping`.
    fn simplify_frame(&self) -> AstResult<Option<DynFrame>> {
        Ok(None)
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

    /// Clear Domain, restoring the class default. Default no-op; concrete
    /// classes with a Domain field override this. Mirrors C `astClearDomain`,
    /// which reaches every class through the Frame virtual-function table.
    fn clear_domain(&mut self) {}

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

    /// Set the Ident token. Default no-op; concrete classes with an Ident
    /// field override this. Mirrors C `astSetIdent`, which is an
    /// Object-level attribute (`object.c:5225`) and so applies to every
    /// class.
    fn set_ident(&mut self, _ident: &str) {}

    /// Clear Ident, restoring the unset state. Mirrors C `astClearIdent`
    /// (`object.c:5217`).
    fn clear_ident(&mut self) {}

    /// Whether Ident has been explicitly set. Mirrors C `astTestIdent`
    /// (`object.c:5229`).
    fn test_ident(&self) -> bool {
        false
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

    /// Internal `Unit(axis)` (1-based): the units in which this Frame's
    /// Mapping actually computes, before any external-unit conversion.
    /// Mirrors C `astGetInternalUnit` (frame.c). The base default
    /// equals the external [`effective_unit`](Self::effective_unit), so
    /// frames whose internal and external units agree trigger no
    /// remapping in `MakeFitsFrameSet`. `SkyFrame` overrides this to
    /// `"rad"` (its axes are angles stored in radians but presented in
    /// degrees).
    fn internal_unit(&self, axis: usize) -> String {
        self.effective_unit(axis)
    }

    /// Read-only `NormUnit(axis)` (1-based): the Unit this Frame reports
    /// for the axis, simplified where possible (`"s*(m/s)"` gives `"m"`),
    /// and equal to it when no simplification can be performed, as for a
    /// blank Unit or a description of axis values such as `"ddd:mm:ss"`.
    ///
    /// Mirrors C `astGetNormUnit` (frame.c:13149), which normalises the
    /// Unit the Frame reports rather than the one stored in the Axis,
    /// because SpecFrame, TimeFrame, FluxFrame and SkyFrame each report
    /// Units of their own. C takes the Axis method instead when the Axis
    /// Unit is set, which normalises the same string. `SkyFrame`
    /// overrides this, as it does in C.
    ///
    /// C reports `AST__FMTER` for a result longer than its 127-character
    /// buffer; the port has no such limit.
    fn norm_unit(&self, axis: usize) -> String {
        crate::units::norm_unit(&self.effective_unit(axis))
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

    /// Test whether `Label(axis)` (1-based) has been explicitly set.
    /// Mirrors C `astTestLabel` (frame.c). Default `false`; concrete
    /// frames that track per-axis label state override this.
    fn test_axis_label(&self, _axis: usize) -> bool {
        false
    }

    /// Test whether `Unit(axis)` (1-based) has been explicitly set.
    /// Mirrors C `astTestUnit` (frame.c). Default `false`; concrete
    /// frames that track per-axis unit state override this.
    fn test_axis_unit(&self, _axis: usize) -> bool {
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

    /// Create a cached description of the line segment from `start` to `end`
    /// in this 2-D Frame. Returns `None` (without error) if any supplied axis
    /// value is BAD. Mirrors C `astLineDef` (frame.c:6716); SkyFrame overrides
    /// with great-circle geometry.
    fn line_def(&self, start: &[f64], end: &[f64]) -> AstResult<Option<LineDef>> {
        use crate::mappings::helpers::is_bad;
        if self.naxes() != 2 {
            return Err(AstError::new(
                ErrorCode::Inter,
                "line_def can only be used with 2-dimensional Frames",
            ));
        }
        if is_bad(start[0]) || is_bad(start[1]) || is_bad(end[0]) || is_bad(end[1]) {
            return Ok(None);
        }
        let length = self.distance(start, end)?;
        let (dir, q) = if length > 0.0 {
            let dx = (end[0] - start[0]) / length;
            let dy = (end[1] - start[1]) / length;
            ([dx, dy], [-dy, dx])
        } else {
            ([1.0, 0.0], [0.0, 1.0])
        };
        Ok(Some(LineDef {
            length,
            infinite: false,
            start: [start[0], start[1]],
            end: [end[0], end[1]],
            dir,
            q,
            sky: None,
        }))
    }

    /// Return a position formed by moving `par` along `line` from its start
    /// and then `prp` at right angles to it. Mirrors C `astLineOffset`
    /// (frame.c:6829); SkyFrame overrides with great-circle geometry.
    fn line_offset(&self, line: &LineDef, par: f64, prp: f64) -> AstResult<[f64; 2]> {
        if self.naxes() != 2 {
            return Err(AstError::new(
                ErrorCode::Inter,
                "line_offset can only be used with 2-dimensional Frames",
            ));
        }
        Ok([
            line.start[0] + par * line.dir[0] + prp * line.q[0],
            line.start[1] + par * line.dir[1] + prp * line.q[1],
        ])
    }

    /// Determine whether two line segments cross. Returns `(crossed, cross,
    /// dist)`: `crossed` is true only when the crossing lies within the
    /// `[start, end)` segment of every finite line (an infinite line imposes
    /// no upper bound); `cross` is the axis values at the crossing point (BAD
    /// when the lines are parallel); `dist` is the signed distance from the
    /// start of `l1` to the crossing. Mirrors C `astLineCrossing`
    /// (frame.c:6562); SkyFrame overrides with great-circle geometry.
    fn line_crossing(&self, l1: &LineDef, l2: &LineDef) -> AstResult<(bool, [f64; 2], f64)> {
        use crate::constants::BAD_VALUE;
        if self.naxes() != 2 {
            return Err(AstError::new(
                ErrorCode::Inter,
                "line_crossing can only be used with 2-dimensional Frames",
            ));
        }
        // Each line is "p = start + t.dir". Solve start1 + t1.d1 = start2 +
        // t2.d2 for t1 and t2.
        let den = l1.dir[0] * l2.dir[1] - l2.dir[0] * l1.dir[1];
        if den != 0.0 {
            let dx = l2.start[0] - l1.start[0];
            let dy = l2.start[1] - l1.start[1];
            let t1 = (l2.dir[1] * dx - l2.dir[0] * dy) / den;
            let t2 = (l1.dir[1] * dx - l1.dir[0] * dy) / den;

            // Store the crossing point, using the smaller t value to reduce
            // error.
            let cross = if t1.abs() < t2.abs() {
                [l1.start[0] + t1 * l1.dir[0], l1.start[1] + t1 * l1.dir[1]]
            } else {
                [l2.start[0] + t2 * l2.dir[0], l2.start[1] + t2 * l2.dir[1]]
            };

            let len1 = if l1.infinite { f64::MAX } else { l1.length };
            let len2 = if l2.infinite { f64::MAX } else { l2.length };
            let crossed = t1 >= 0.0 && t1 < len1 && t2 >= 0.0 && t2 < len2;
            Ok((crossed, cross, t1))
        } else {
            Ok((false, [BAD_VALUE, BAD_VALUE], BAD_VALUE))
        }
    }

    /// Determine whether `point` lies on the line `l`. The start point is
    /// considered part of the line, the end point is not: the nearest point on
    /// the line must be in `[start, end)` and the perpendicular offset must be
    /// within `1e-7` of the line length. Mirrors C `astLineContains`
    /// (frame.c:6466); SkyFrame overrides with great-circle geometry.
    fn line_contains(&self, l: &LineDef, point: &[f64]) -> AstResult<bool> {
        use crate::mappings::helpers::is_bad;
        if self.naxes() != 2 {
            return Err(AstError::new(
                ErrorCode::Inter,
                "line_contains can only be used with 2-dimensional Frames",
            ));
        }
        if is_bad(point[0]) || is_bad(point[1]) {
            return Ok(false);
        }
        let dx = point[0] - l.start[0];
        let dy = point[1] - l.start[1];
        let p = dx * l.dir[0] + dy * l.dir[1];
        if p >= 0.0 && p < l.length && (dx * l.q[0] + dy * l.q[1]).abs() <= 1.0e-7 * l.length {
            return Ok(true);
        }
        Ok(false)
    }

    /// The kind of Axis object attached to a 1-based `axis` of this Frame.
    ///
    /// Mirrors C `astGetAxis` (frame.c:5309), which returns the per-axis
    /// `Axis` object that the `astAx*` and formatting methods then dispatch
    /// to. A Frame's axes need not all be of the same class — a CmpFrame can
    /// hold a sky axis beside a plain one — so the per-axis rule cannot be
    /// derived from the Frame's own class.
    ///
    /// The base Frame holds plain `Axis` objects on every axis. Compound and
    /// permuting Frames (CmpFrame, SkyFrame, FrameSet, Region) override to
    /// route to the axis's true owner, exactly as C's `GetAxis` overrides do.
    fn get_axis(&self, _axis: usize) -> AxisKind {
        AxisKind::Plain
    }

    /// Signed axis increment from `v1` to `v2` on a 1-based `axis`.
    ///
    /// Dispatches to the Axis object on that axis. Mirrors C `astAxDistance`
    /// (frame.c:1466), which fetches the Axis with `astGetAxis` and calls
    /// `astAxisDistance` on it.
    fn ax_distance(&self, axis: usize, v1: f64, v2: f64) -> AstResult<f64> {
        self.validate_frame_axis(axis, "astAxDistance")?;
        Ok(self.get_axis(axis).distance(v1, v2))
    }

    /// Axis value formed by adding `dist` to `v1` on a 1-based `axis`.
    ///
    /// Dispatches to the Axis object on that axis. Mirrors C `astAxOffset`
    /// (frame.c:1744), which fetches the Axis with `astGetAxis` and calls
    /// `astAxisOffset` on it.
    fn ax_offset(&self, axis: usize, v1: f64, dist: f64) -> AstResult<f64> {
        self.validate_frame_axis(axis, "astAxOffset")?;
        Ok(self.get_axis(axis).offset(v1, dist))
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
            // Upstream da9ddb39 fixed the stale-index bug: the nudge is
            // scaled by |b[axis-1]|, not |b[naxes-1]|.
            aa[k] = eps * b[k].abs();
        } else if mxab != 0.0 {
            aa[k] = eps * mxab;
        } else {
            aa[k] = 1.0;
        }
        self.angle(&aa, a, b)
    }

    /// Normalise an array of axis values on a 1-based `axis`. `oper`
    /// selects the normalisation mode (0 = like `norm`, 1 = smallest range).
    ///
    /// Dispatches to the Axis object on that axis. Mirrors C `astAxNorm`
    /// (frame.c:1560), which fetches the Axis with `astGetAxis`, validates
    /// `oper`, and calls `astAxisNormValues` on it.
    fn ax_norm(&self, axis: usize, oper: i32, values: &mut [f64]) -> AstResult<()> {
        self.validate_frame_axis(axis, "astAxNorm")?;
        if !(0..=1).contains(&oper) {
            return Err(AstError::new(
                ErrorCode::Oprin,
                format!("astAxNorm: invalid operation {oper}"),
            ));
        }
        self.get_axis(axis).norm_values(oper, values);
        Ok(())
    }

    /// Validate a 1-based axis index against this Frame's axis count.
    ///
    /// Mirrors the `astValidateAxis` call that opens each `astAx*` driver.
    fn validate_frame_axis(&self, axis: usize, method: &str) -> AstResult<()> {
        let naxes = self.naxes();
        if axis < 1 || axis > naxes {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("{method}: axis index {axis} is out of range 1..={naxes}"),
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

    /// Batch form of `resolve`: resolve every vector `point1->in[i]` into
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
    ///
    /// `inside` is a membership predicate for the box: it returns `true`
    /// when a supplied point lies inside. It mirrors the C `reg` Mapping
    /// argument (which leaves interior points unchanged and sets exterior
    /// points bad); the SkyFrame override uses it to test the poles.
    fn norm_box(
        &self,
        _lbnd: &mut [f64],
        _ubnd: &mut [f64],
        _inside: &dyn Fn(&[f64]) -> AstResult<bool>,
    ) -> AstResult<()> {
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

    // Build the new frame by selecting the axes, as C's astPickAxes does via
    // astSubFrame (frame.c:8815). Going through the class's own snapshot keeps
    // each picked axis's identity: a sky axis picked out of a SkyFrame stays a
    // sky axis, and a CmpFrame routes each axis to its owning component.
    let zero_based: Vec<i32> = axes.iter().map(|&ax| (ax - 1) as i32).collect();
    let new_frame: DynFrame = frame.pick_axes_snapshot(&zero_based)?;

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
        return bf.test_epoch().then(|| bf.epoch_mjd());
    }
    if let Some(cf) = any.downcast_ref::<CmpFrame>() {
        return cf.test_epoch().then(|| cf.epoch_mjd());
    }
    None
}

/// The *effective* Epoch (TDB MJD) of a frame, mirroring C's
/// `astGetEpoch`: every concrete class returns a value even when the
/// attribute was never explicitly set (the class-specific default —
/// J2000 for most, B1950 for FK4 SkyFrames, and for CmpFrame whatever a
/// component supplies). Unlike [`frame_epoch_if_set`], this never
/// returns `None`; the fallback is the J2000 MJD used by the C base
/// `GetEpoch` (`frame.c:3210`).
pub(crate) fn frame_epoch_effective_mjd(f: &dyn Frame) -> f64 {
    let any = f.as_any();
    if let Some(sf) = any.downcast_ref::<SkyFrame>() {
        return sf.epoch_mjd();
    }
    if let Some(sf) = any.downcast_ref::<SpecFrame>() {
        return sf.epoch_mjd();
    }
    if let Some(tf) = any.downcast_ref::<TimeFrame>() {
        return tf.epoch_mjd();
    }
    if let Some(df) = any.downcast_ref::<DSBSpecFrame>() {
        return df.epoch_mjd();
    }
    if let Some(cf) = any.downcast_ref::<CmpFrame>() {
        return cf.epoch_mjd();
    }
    if let Some(bf) = any.downcast_ref::<BasicFrame>() {
        return bf.epoch_mjd();
    }
    crate::pal::epj2d(2000.0)
}

/// The effective Epoch of a frame as a decimal year, using the
/// Besselian calendar below J1984 and the Julian calendar at or above
/// it — exactly the branch `frame.c::Dump` takes (frame.c:14338-14343).
/// Returns `(year, is_besselian)` so the caller can pick the matching
/// comment string.
pub(crate) fn frame_epoch_effective_year(f: &dyn Frame) -> (f64, bool) {
    let mjd = frame_epoch_effective_mjd(f);
    if mjd < crate::pal::epj2d(1984.0) {
        (crate::pal::epb(mjd), true)
    } else {
        (crate::pal::epj(mjd), false)
    }
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
                    3 => x.set_dtai(v),
                    _ => x.set_dut1(v),
                }
                return;
            }
        };
    }
    fwd!(SkyFrame);
    fwd!(TimeFrame);
    if let Some(x) = f.as_any_mut().downcast_mut::<SpecFrame>() {
        match which {
            0 => x.set_obs_lat(v),
            1 => x.set_obs_lon(v),
            2 => x.set_obs_alt(v),
            3 => x.set_dtai(v),
            _ => x.set_dut1(v),
        }
        return;
    }
    if let Some(x) = f.as_any_mut().downcast_mut::<DSBSpecFrame>() {
        match which {
            0 => x.set_obs_lat(v),
            1 => x.set_obs_lon(v),
            2 => x.set_obs_alt(v),
            3 => x.set_dtai(v),
            _ => x.set_dut1(v),
        }
        return;
    }
    if let Some(x) = f.as_any_mut().downcast_mut::<CmpFrame>() {
        match which {
            0 => x.set_obs_lat(v),
            1 => x.set_obs_lon(v),
            2 => x.set_obs_alt(v),
            3 => x.set_dtai(v),
            _ => x.set_dut1(v),
        }
    }
}
