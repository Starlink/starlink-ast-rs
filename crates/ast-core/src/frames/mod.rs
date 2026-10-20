//! The Frame trait and associated types.

pub(crate) mod axis;
#[macro_use]
pub(crate) mod frame_attrs;
pub(crate) mod axis_format;
mod basic;
mod cmpframe;
pub(crate) mod date_time;
mod dsbspecframe;
pub(crate) mod find_frame;
mod fluxframe;
mod frameset;
pub(crate) mod overlay;
pub(crate) mod sky_convert;
mod sky_last;
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

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::{AsAny, DynMapping, Mapping};
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

/// The frame flag C sets while a FrameSet's integrity is being restored
/// (frame.h `AST__INTFLAG`).
pub(crate) const INTFLAG: u32 = 1;

/// A view of a Frame as `&dyn Frame`, which a default method of [`Frame`]
/// cannot form from its own `&self`. Implemented for every Frame.
#[doc(hidden)]
pub trait AsDynFrame {
    /// `self` as `&dyn Frame`.
    fn as_dyn_frame(&self) -> &dyn Frame;
}

impl<T: Frame> AsDynFrame for T {
    fn as_dyn_frame(&self) -> &dyn Frame {
        self
    }
}

/// A coordinate frame defining the meaning of coordinate values.
///
/// All concrete frame classes implement this trait. Frames are `Send`
/// but not `Sync`.
///
/// Each axis of a Frame carries attributes of its own, each taking a 1-based
/// axis index: [`label`](Frame::label), [`symbol`](Frame::symbol),
/// [`unit`](Frame::unit), [`format_str`](Frame::format_str),
/// [`axis_digits`](Frame::axis_digits), [`direction`](Frame::direction),
/// [`top`](Frame::top) and [`bottom`](Frame::bottom), together with the
/// [`internal_unit`](Frame::internal_unit) and
/// [`norm_unit`](Frame::norm_unit) derived from the Unit.
/// They are used chiefly in textual output, such as formatting a coordinate
/// value with [`format`](Frame::format).
// The crate-private Axis class is documented here, through these per-axis
// attributes, which is why its C documentation ledger row is merged into
// Frame.
pub trait Frame: AsAny + AsDynFrame + Send + fmt::Debug {
    /// The number of axes in this Frame: the Naxes attribute.
    ///
    /// This is the number of dimensions of the coordinate space the Frame
    /// describes, fixed when the Frame is created.
    ///
    /// - FrameSet: the Naxes of its current Frame.
    /// - CmpFrame: the sum of the Naxes of its two component Frames.
    #[doc(alias = "Naxes")]
    fn naxes(&self) -> usize;

    /// Format a coordinate value for a Frame axis.
    ///
    /// Returns the text form of `value` on the 1-based `axis`.
    /// The formatting is set by the Frame's attributes, in particular the
    /// [`format_str`](Frame::format_str) of the axis; when none is set, a
    /// default based on [`axis_digits`](Frame::axis_digits) is used.
    /// `BAD_VALUE` formats as `"<bad>"`.
    /// [`unformat`](Frame::unformat) reads a formatted value back.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not between 1 and
    ///   [`naxes`](Frame::naxes).
    /// - `ErrorCode::Fmter` when the axis's Format cannot format the value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::constants::BAD_VALUE;
    /// use ast_core::frames::{BasicFrameBuilder, Frame};
    ///
    /// let frame = BasicFrameBuilder::new(2).build().unwrap();
    /// assert_eq!(frame.format(1, 1.5).unwrap(), "1.5");
    /// assert_eq!(frame.format(2, BAD_VALUE).unwrap(), "<bad>");
    /// ```
    #[doc(alias = "astFormat")]
    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        // The default is C's base `astFormat` → `AxisFormat` (axis.c): the
        // `<bad>` token for the bad value, otherwise the effective per-axis
        // Format string (set value, else the default `"%1.<Digits>G"` with
        // the per-axis Digits) rendered by `axis_format::format_value`. Leaf
        // Frame classes that do not override Format in C (SpecFrame,
        // FluxFrame, DSBSpecFrame) inherit this; CmpFrame, TimeFrame and
        // SkyFrame override.
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
        // The Format, set or default, and the Digits it defaults from
        // (frame.c:4260, axis.c:1819).
        let digits = self.axis_digits(axis)?;
        let fmt = self.format_str(axis)?;
        crate::frames::axis_format::format_value(&fmt, digits, value)
    }

    /// Read a formatted coordinate value for a Frame axis.
    ///
    /// Reads the value at the start of `text` for the 1-based `axis` and
    /// returns `(value, nread)`, where `nread` is the number of bytes read,
    /// including any white space before or after the value.
    /// Reading stops at the first character that cannot be part of the
    /// value, so `text` may carry more after it.
    /// This is the inverse of [`format`](Frame::format), and is meant for
    /// decoding free-format user input; where input is ambiguous it is read
    /// with reference to the axis's [`format_str`](Frame::format_str).
    ///
    /// When `text` holds no suitably formatted value the result is
    /// `(BAD_VALUE, 0)`, without error.
    /// A typing error part way through can end the read early and still give
    /// a plausible value (`"1.5r6"` reads as 1.5), so check that `nread`
    /// covers the text you expected to be read.
    /// The string `"<bad>"`, in any case and with embedded white space,
    /// reads as `BAD_VALUE`.
    ///
    /// # Input formats
    ///
    /// A plain Frame axis reads an optional sign, then one or more digits
    /// possibly holding a decimal point, then an optional exponent (`E` or
    /// `e` and a possibly signed integer): `99`, `1.25`, `-1.6`, `1E8`,
    /// `-.99e-17`.
    ///
    /// A FrameSet reads as its current Frame does.
    ///
    /// A SkyFrame axis reads angles and times, and returns radians:
    ///
    /// - An optional sign, then one to three fields giving degrees,
    ///   arc-minutes and arc-seconds, or hours, minutes and seconds
    ///   (`-12 42 03`).
    /// - Each field is one or more digits, leading zeros allowed.
    ///   At most one field may hold a decimal point, and it is then the last
    ///   field (`124.707`, `-13 33.8`).
    /// - The first field may take any value; later fields must be less than
    ///   60 (`720 45 31` is valid, `11 45 61` is not).
    /// - Fields are separated by white space or by `:`, used consistently,
    ///   with extra white space allowed around fields and separators
    ///   (`- 2: 04 : 7.1`).
    /// - A field identifier may replace a separator or follow the last
    ///   field: `d` (degrees), `h` (hours), `m` (minutes of arc or time),
    ///   `s` (seconds of arc or time), `'` (minutes of arc) and `"` (seconds
    ///   of arc), in either case, in order of decreasing significance
    ///   (`-11D 3' 14.4"`, `22h14m11.2s`).
    /// - `d`, `'` or `"` make the value an angle and `h` makes it a time
    ///   (24 hours to 360 degrees); the two kinds may not be mixed
    ///   (`10h14'3"` is invalid).
    ///   With neither, the value is an angle if the axis's Format gives an
    ///   angle and a time otherwise, so values from `format` read back
    ///   correctly.
    /// - Omitted fields are zero.
    ///   The remaining ones are identified by field identifiers or by extra
    ///   colons (`-05m13s` is `-:05:13`); an unidentified field is taken to
    ///   be adjacent to its neighbors (`14:25.4s` is minutes and seconds,
    ///   `14::25.4s` degrees and seconds).
    /// - When the fields cannot be identified uniquely (`01:02`), the first
    ///   is taken to be the most significant field the axis's Format would
    ///   produce, normally degrees or hours; if that is inconsistent, the
    ///   last is taken to be the least significant field it would produce.
    ///
    /// Examples, with alternatives depending on whether the Format gives an
    /// angle or a time: `-14d 13m 22.2s`; `+ 12:34:56.7`; `001 : 02 : 03.4`;
    /// `22h 30` (22h 30m 00s); `136::10"` (136d 00' 10"); `-14M 27S`;
    /// `-:14:`; `-::4.1`; `.9"`; `d12m` (0d 12' 00"); `H 12:22.3s`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not between 1 and
    ///   [`naxes`](Frame::naxes).
    /// - `ErrorCode::Unfer` when a SkyFrame value has the right form but a
    ///   field that is out of range, such as a minutes field of 60 or more.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::{BasicFrameBuilder, Frame};
    ///
    /// let frame = BasicFrameBuilder::new(1).build().unwrap();
    /// assert_eq!(frame.unformat(1, " -1.6e2 rest").unwrap(), (-160.0, 8));
    /// assert_eq!(frame.unformat(1, "abc").unwrap().1, 0);
    /// ```
    #[doc(alias = "astUnformat")]
    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        // The default is C's base `astUnformat` (frame.c:11819): validate the
        // axis, then apply the `astAxisUnformat` contract (axis.c:1284) — the
        // longest `"%lf %n"` numeric prefix (counting trailing whitespace),
        // the `<bad>` token, else `(BAD_VALUE, 0)` with no error. Leaf Frame
        // classes that do not override Unformat in C (SpecFrame, FluxFrame,
        // DSBSpecFrame, SpecFluxFrame) inherit this; CmpFrame, TimeFrame and
        // SkyFrame override.
        if axis < 1 || axis > self.naxes() {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!("axis index {axis} is out of range 1..={}", self.naxes()),
            ));
        }
        Ok(crate::frames::basic::axis_unformat(text))
    }

    /// Calculate the distance between two points in this Frame.
    ///
    /// `a` and `b` each hold one coordinate per axis.
    /// The distance is measured along the geodesic curve joining the two
    /// points: in a plain Frame, the Cartesian distance along the straight
    /// line between them; in a SkyFrame, the arc along the great circle
    /// through the two sky positions.
    ///
    /// The result is `BAD_VALUE` if any input coordinate is `BAD_VALUE`.
    ///
    /// # Panics
    ///
    /// If any point holds fewer than [`naxes`](Frame::naxes) coordinates.
    ///
    /// # Errors
    ///
    /// No Frame class raises an error of its own; a CmpFrame, FrameSet or
    /// Region passes on any error from `distance` on the Frames it holds.
    #[doc(alias = "astDistance")]
    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64>;

    /// Calculate the angle subtended by two points at a third point.
    ///
    /// Returns the angle at point `b` from the line joining `a` and `b` to
    /// the line joining `c` and `b`, in radians.
    /// Each point holds one coordinate per axis, and the lines are the
    /// geodesic curves of the Frame, great circles in a SkyFrame.
    ///
    /// In a 2-dimensional Frame the angle is in the range -pi to +pi, with
    /// positive rotation in the sense from the positive direction of axis 2
    /// to the positive direction of axis 1.
    /// With more axes it is always positive, between zero and pi.
    ///
    /// The result is `BAD_VALUE` if any input coordinate is `BAD_VALUE`, or
    /// if `a` and `b`, or `b` and `c`, coincide.
    ///
    /// # Panics
    ///
    /// If any point holds fewer than [`naxes`](Frame::naxes) coordinates.
    ///
    /// # Errors
    ///
    /// No Frame class raises an error of its own; a CmpFrame, FrameSet or
    /// Region passes on any error from `angle` on the Frames it holds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::{BasicFrameBuilder, Frame};
    ///
    /// let frame = BasicFrameBuilder::new(2).build().unwrap();
    /// // From the +axis-2 direction to the +axis-1 direction is +pi/2.
    /// let angle = frame.angle(&[0.0, 1.0], &[0.0, 0.0], &[1.0, 0.0]).unwrap();
    /// assert!((angle - std::f64::consts::FRAC_PI_2).abs() < 1e-15);
    /// ```
    #[doc(alias = "astAngle")]
    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64>;

    /// Calculate an offset along a geodesic curve.
    ///
    /// Returns the point `dist` along the geodesic curve from `from` towards
    /// `to`: along the straight line through them in a plain Frame, along
    /// the great circle through them in a SkyFrame.
    /// A negative `dist` goes in the opposite direction, and the curve is
    /// extended as needed, so the point need not lie between the two given.
    /// The geodesic is the path of shortest distance, as
    /// [`distance`](Frame::distance) measures it.
    ///
    /// Every coordinate of the result is `BAD_VALUE` if any input
    /// coordinate is `BAD_VALUE`, or if the two points coincide (or
    /// otherwise fail to fix a unique geodesic) and `dist` is not zero.
    ///
    /// # Panics
    ///
    /// If any point holds fewer than [`naxes`](Frame::naxes) coordinates.
    ///
    /// # Errors
    ///
    /// No Frame class raises an error of its own; a CmpFrame, FrameSet or
    /// Region passes on any error from `offset` on the Frames it holds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::{BasicFrameBuilder, Frame};
    ///
    /// let frame = BasicFrameBuilder::new(2).build().unwrap();
    /// let p = frame.offset(&[0.0, 0.0], &[3.0, 4.0], 10.0).unwrap();
    /// assert_eq!(p, vec![6.0, 8.0]);
    /// ```
    #[doc(alias = "astOffset")]
    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>>;

    /// Normalize a set of Frame coordinates representing one point.
    ///
    /// `coords` holds one value per axis.
    /// Values that lie outside the range expected for display are replaced
    /// in place by acceptable (normalized) ones; others are left unchanged.
    /// A Frame whose coordinates are unconstrained never changes them, while
    /// one whose axes are cyclic, such as a SkyFrame, wraps them into a
    /// standard range such as zero to 2*pi.
    /// Normalizing and then formatting with [`format`](Frame::format) puts
    /// any coordinates into a form suitable for display.
    /// A [`NormMap`](crate::mappings::NormMap) applies a Frame's `norm` as a
    /// Mapping.
    ///
    /// # Panics
    ///
    /// If `coords` holds fewer than [`naxes`](Frame::naxes) coordinates.
    ///
    /// # Errors
    ///
    /// No Frame class raises an error of its own; a CmpFrame, FrameSet or
    /// Region passes on any error from `norm` on the Frames it holds.
    #[doc(alias = "astNorm")]
    fn norm(&self, coords: &mut [f64]) -> AstResult<()>;

    /// Deep-clone this frame for integrity delta computation.
    ///
    /// Used by FrameSet and Region integrity: a snapshot is taken before
    /// mutation, then `map_from` computes the correction mapping.
    fn snapshot(&self) -> DynFrame;

    /// This Frame with every Region it holds replaced by the Region's Frame,
    /// or `None` when it holds none: C's `RemoveRegions` method applied to a
    /// Frame (region.c, cmpframe.c, frameset.c).
    #[doc(hidden)]
    fn frame_without_regions(&self) -> Option<DynFrame> {
        None
    }

    /// Compute the mapping from a previous state of this frame to the current state.
    ///
    /// After mutating a frame attribute, this returns the transformation that
    /// maps coordinates in the old frame to coordinates in the new frame.
    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping>;

    /// Determine how to convert between two coordinate systems.
    ///
    /// Compares this Frame, the source coordinate system (the one in which
    /// coordinates are already available), with `to`, the destination, and
    /// returns a [`FrameSet`] describing the conversion if one is possible,
    /// or `None` if it is not.
    /// The FrameSet holds two Frames: its base Frame describes the source
    /// and its current Frame the destination, and the Mapping between them
    /// performs the conversion.
    /// Used as a Mapping, the FrameSet converts source coordinates to
    /// destination coordinates (and back with its inverse); used as a Frame,
    /// its attributes describe the destination.
    /// Alignment takes place in the coordinate system given by the
    /// AlignSystem attribute of `to`.
    ///
    /// Either Frame may be a FrameSet, whose current Frame then describes
    /// its coordinate system.
    /// This is how, for instance, two images calibrated by attaching a
    /// FrameSet to each are aligned: the conversion finds a celestial
    /// coordinate system both have in common, and converts between their
    /// pixel coordinates through it.
    /// To convert between the base Frames of two FrameSets (usually their
    /// pixel coordinates), invert both with
    /// [`invert`](crate::mappings::Mapping::invert) first.
    ///
    /// # Arguments
    ///
    /// - `domains`: a priority order for the intermediate coordinate system.
    ///   The conversion is first sought through an intermediate system whose
    ///   Domain matches the first entry, then the second, and so on.
    ///   An empty entry, or an empty slice, permits any Domain.
    ///   Matching ignores case and white space.
    ///   With two plain Frames there is usually only one possible
    ///   intermediate system, so an empty slice is normal.
    ///
    /// # Per class
    ///
    /// - Frame: alignment occurs in the coordinate system given by
    ///   AlignSystem.
    /// - SpecFrame: alignment occurs in the spectral system and standard of
    ///   rest given by AlignSystem and AlignStdOfRest.
    /// - DSBSpecFrame: when AlignSideBand is true, alignment occurs in the
    ///   upper sideband, in the system and standard of rest given by
    ///   AlignSystem and AlignStdOfRest; when it is false, the two are
    ///   aligned as plain SpecFrames and the SideBand is ignored.
    /// - TimeFrame: alignment occurs in the time system and time scale
    ///   given by AlignSystem and AlignTimeScale.
    /// - FrameSet: every Frame of each FrameSet is considered as a way of
    ///   reaching an intermediate coordinate system.
    ///   Each entry of `domains` is taken in turn.
    ///   Within it, each FrameSet's Frames are tried with its base Frame
    ///   first, then the others in ascending index order, then its current
    ///   Frame last; the port uses this order for an inverted FrameSet too
    ///   (whose base and current Frames have swapped), where C takes the
    ///   other Frames in descending order.
    ///   Every source Frame is tried against the first destination
    ///   Frame, then against the second, and so on.
    ///   Among the paths found, one with both transformations defined is
    ///   preferred, then one with only the forward transformation.
    ///   To force a particular Frame to be used, name its Domain in
    ///   `domains` or make it the base Frame.
    ///
    /// # Errors
    ///
    /// Passes on the error of the class-specific matching of the Frames
    /// involved ([`match_template`](Frame::match_template) and
    /// [`sub_frame`](Frame::sub_frame)).
    /// Units that cannot be converted into each other are not an error: that
    /// pair of Frames does not match.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::{BasicFrameBuilder, Frame};
    /// use ast_core::mappings::Mapping;
    ///
    /// let a = BasicFrameBuilder::new(2).build().unwrap();
    /// let b = BasicFrameBuilder::new(2).build().unwrap();
    /// let cvt = a.convert_to(&b, &[]).unwrap().expect("plain Frames align");
    /// assert_eq!(cvt.nin(), 2);
    ///
    /// let c = BasicFrameBuilder::new(3).build().unwrap();
    /// assert!(a.convert_to(&c, &[]).unwrap().is_none());
    /// ```
    ///
    /// # See also
    ///
    /// `astConvert`.
    #[doc(alias = "astConvert")]
    fn convert_to(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>> {
        // C `astConvert` (frame.c:2965) swaps its first two arguments and
        // defers to the generic `astConvertX` driver (frame.c:3028).
        //
        // The body joins `domains` into the comma-separated list C expects
        // (an empty slice becomes a single blank field that permits any
        // domain). This is frame.c `Convert`, which dispatches `astConvertX`
        // on the class of `to`: when `to` is a FrameSet, the FrameSet
        // `ConvertX` (frameset.c:3204) searches every Frame of it; otherwise
        // the generic driver runs. FrameSet overrides this method, as
        // frameset.c overrides `Convert`, so that a FrameSet `self` reaches
        // its `ConvertX` too. All other class-specific alignment lives in
        // `match_template` / `sub_frame`, exactly as in C.
        //
        // C's `ConvertX` also sets the Base attribute of a FrameSet `self` or
        // `to` to the Frame used (frameset.c:3515); the port takes both by
        // shared reference and leaves them unchanged.
        let domainlist = if domains.is_empty() {
            " ".to_string()
        } else {
            domains.join(",")
        };
        if let Some(to_fs) = to.as_any().downcast_ref::<FrameSet>() {
            return frameset::frameset_convert_x(to_fs, self.as_dyn_frame(), &domainlist);
        }
        // The Frame itself, not a copy, takes part, as in C: a SkyFrame
        // keeps the LAST it computes for the conversion. A Region `to` is
        // replaced by its Frame, as region.c's `ConvertX` replaces it.
        find_frame::convert_frames_or_regions(self.as_dyn_frame(), to, &domainlist)
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

    // -----------------------------------------------------------------
    // Frame-class attributes (frame.c). Every Frame class implements the
    // full get / set / test / clear set, as C's Frame virtual functions
    // reach every class; a getter returns the value set or the class's
    // default, as `astGet` does. FrameSet and Region forward each to the
    // Frame they present, applying FrameSet integrity on the setters.
    // -----------------------------------------------------------------

    /// The Frame title: the Title attribute, set or default.
    ///
    /// A string used as a title in, for instance, graphical output, to
    /// describe the coordinate system the Frame represents, such as
    /// `"Detector Coordinates"` or `"Galactic Coordinates"`.
    /// It is meant for human readers, not software.
    /// When it is not set, a default suited to the class of Frame is
    /// supplied.
    ///
    /// Default: `"<n>-d coordinate system"`, where `<n>` is
    /// [`naxes`](Frame::naxes).
    ///
    /// - CmpFrame: the default is `"<n>-d compound coordinate system"`.
    /// - FrameSet: the Title of its current Frame.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Badun` when the default depends on a System that cannot
    ///   be determined: a FluxFrame whose Unit fits no flux system.
    #[doc(alias = "Title")]
    fn title(&self) -> AstResult<String>;
    /// Set the Title; see [`title`](Frame::title).
    fn set_title(&mut self, title: &str) -> AstResult<()>;
    /// Whether the Title is set; see [`title`](Frame::title).
    fn test_title(&self) -> bool;
    /// Clear the Title; see [`title`](Frame::title).
    fn clear_title(&mut self) -> AstResult<()>;

    /// The coordinate system domain: the Domain attribute, set or default.
    ///
    /// Identifies the physical domain of the coordinate system the Frame
    /// describes.
    /// It also controls matching when the Frame is the template of
    /// [`find_frame_template`](Frame::find_frame_template): with Domain set,
    /// the template matches only targets with the same Domain; unset, the
    /// target's Domain is ignored.
    /// Domain values are stored with white space removed and letters in
    /// upper case.
    ///
    /// Default: an empty string.
    ///
    /// - SkyFrame: see [`SkyFrame::domain`].
    /// - SpecFrame: see [`SpecFrame`].
    /// - DSBSpecFrame: see [`DSBSpecFrame::domain`].
    /// - FluxFrame: `"FLUX"`.
    /// - SpecFluxFrame: `"SPECTRUM-FLUX"`.
    /// - TimeFrame: `"TIME"`.
    /// - CmpFrame: `"<dom1>-<dom2>"`, from the Domains of its two component
    ///   Frames, or `"CMP"` when both are blank.
    /// - FrameSet: the Domain of its current Frame.
    #[doc(alias = "Domain")]
    fn domain(&self) -> &str;
    /// Set the Domain, removing white space and upper-casing letters; see
    /// [`domain`](Frame::domain).
    fn set_domain(&mut self, domain: &str) -> AstResult<()>;
    /// Whether the Domain is set; see [`domain`](Frame::domain).
    fn test_domain(&self) -> bool;
    /// Clear the Domain; see [`domain`](Frame::domain).
    fn clear_domain(&mut self) -> AstResult<()>;

    /// The epoch of observation: the Epoch attribute, set or default, as a
    /// Modified Julian Date.
    ///
    /// Qualifies the coordinate systems a Frame describes by giving the
    /// moment at which the coordinates are known to be correct, often the
    /// date of observation.
    /// It matters where coordinate systems move with respect to each other
    /// over time.
    /// Strictly the value is in the TDB timescale, but for some purposes,
    /// such as converting sky positions between types of equatorial system,
    /// the timescale is not significant and UTC may be used.
    ///
    /// Default: J2000.0 (Julian).
    ///
    /// - Frame: the plain Frame makes no use of the Epoch, as it does not
    ///   distinguish between Cartesian coordinate systems.
    /// - CmpFrame: the default is the Epoch of the first component Frame if
    ///   set there, else that of the second if set there, else the first
    ///   component's default; setting or clearing the Epoch of a CmpFrame
    ///   sets or clears it in both components.
    /// - FrameSet: the Epoch of its current Frame.
    /// - SkyFrame: see [`SkyFrame::epoch_mjd`].
    /// - TimeFrame: a time axis is not characterized by one Epoch, so the
    ///   TimeFrame makes no use of it, though user code may use it for a
    ///   "typical" time.
    ///   The default is the TDB equivalent of the TimeOrigin if set, else
    ///   J2000.0.
    #[doc(alias = "Epoch")]
    fn epoch_mjd(&self) -> f64;
    /// Set the Epoch, as a TDB Modified Julian Date; see
    /// [`epoch_mjd`](Frame::epoch_mjd).
    fn set_epoch_mjd(&mut self, mjd: f64) -> AstResult<()>;
    /// Whether the Epoch is set; see [`epoch_mjd`](Frame::epoch_mjd).
    fn test_epoch(&self) -> bool;
    /// Clear the Epoch; see [`epoch_mjd`](Frame::epoch_mjd).
    fn clear_epoch(&mut self) -> AstResult<()>;

    /// The number of digits of precision for the Frame as a whole: the
    /// Digits attribute, set or default.
    ///
    /// How many digits of precision are wanted by default when a coordinate
    /// value is formatted for an axis with [`format`](Frame::format).
    /// A value set for an individual axis with
    /// [`set_axis_digits`](Frame::set_axis_digits) over-rides this one for
    /// that axis.
    /// Digits only chooses the default [`format_str`](Frame::format_str):
    /// a Format set on an axis over-rides it, except that a Format giving
    /// its precision as `.*` takes the number of decimal places from Digits.
    ///
    /// Default: 7.
    /// A value below 1 is stored as 1.
    ///
    /// - FrameSet: the Digits of its current Frame.
    /// - TimeFrame: ignored when a value is formatted as a date and time.
    #[doc(alias = "Digits")]
    fn digits(&self) -> i32;
    /// Set the Frame's Digits, stored as at least 1; see
    /// [`digits`](Frame::digits).
    fn set_digits(&mut self, value: i32) -> AstResult<()>;
    /// Whether the Frame's Digits is set; see [`digits`](Frame::digits).
    fn test_digits(&self) -> bool;
    /// Clear the Frame's Digits; see [`digits`](Frame::digits).
    fn clear_digits(&mut self) -> AstResult<()>;

    /// The geodetic latitude of the observer, in radians: the ObsLat
    /// attribute, set or default.
    ///
    /// Relative to the IAU 1976 reference ellipsoid.
    /// The plain Frame makes no use of it, but specialized classes such as
    /// SpecFrame, SkyFrame and TimeFrame do.
    ///
    /// Default: 0.
    ///
    /// - SpecFrame: see [`SpecFrame`].
    /// - TimeFrame: with ObsLon, it is used when converting between some
    ///   time scales (TDB, TCB, LMST, LAST).
    #[doc(alias = "ObsLat")]
    fn obs_lat(&self) -> f64;
    /// Set ObsLat, in radians; see [`obs_lat`](Frame::obs_lat).
    fn set_obs_lat(&mut self, rad: f64) -> AstResult<()>;
    /// Whether ObsLat is set; see [`obs_lat`](Frame::obs_lat).
    fn test_obs_lat(&self) -> bool;
    /// Clear ObsLat; see [`obs_lat`](Frame::obs_lat).
    fn clear_obs_lat(&mut self) -> AstResult<()>;

    /// The geodetic longitude of the observer, in radians: the ObsLon
    /// attribute, set or default.
    ///
    /// Measured positive eastwards; geodetic and geocentric longitude are
    /// the same.
    /// The plain Frame makes no use of it, but specialized classes such as
    /// SpecFrame, SkyFrame and TimeFrame do.
    ///
    /// Default: 0.
    ///
    /// - SpecFrame: see [`SpecFrame`].
    /// - TimeFrame: with ObsLat, it is used when converting between some
    ///   time scales (TDB, TCB, LMST, LAST).
    #[doc(alias = "ObsLon")]
    fn obs_lon(&self) -> f64;
    /// Set ObsLon, in radians; see [`obs_lon`](Frame::obs_lon).
    fn set_obs_lon(&mut self, rad: f64) -> AstResult<()>;
    /// Whether ObsLon is set; see [`obs_lon`](Frame::obs_lon).
    fn test_obs_lon(&self) -> bool;
    /// Clear ObsLon; see [`obs_lon`](Frame::obs_lon).
    fn clear_obs_lon(&mut self) -> AstResult<()>;

    /// The geodetic altitude of the observer, in meters: the ObsAlt
    /// attribute, set or default.
    ///
    /// Relative to the IAU 1976 reference ellipsoid.
    /// The plain Frame makes no use of it, but specialized classes such as
    /// SpecFrame, SkyFrame and TimeFrame do.
    ///
    /// Default: 0.
    ///
    /// - SpecFrame: see [`SpecFrame`].
    /// - TimeFrame: with ObsLon and ObsLat, it is used when converting
    ///   between some time scales (TDB, TCB, LMST, LAST).
    #[doc(alias = "ObsAlt")]
    fn obs_alt(&self) -> f64;
    /// Set ObsAlt, in meters; see [`obs_alt`](Frame::obs_alt).
    fn set_obs_alt(&mut self, metres: f64) -> AstResult<()>;
    /// Whether ObsAlt is set; see [`obs_alt`](Frame::obs_alt).
    fn test_obs_alt(&self) -> bool;
    /// Clear ObsAlt; see [`obs_alt`](Frame::obs_alt).
    fn clear_obs_alt(&mut self) -> AstResult<()>;

    /// The TAI-UTC correction, in seconds: the Dtai attribute, set or
    /// default.
    ///
    /// The difference between TAI and UTC (the number of leap seconds) at
    /// the moment of the Frame's [`epoch_mjd`](Frame::epoch_mjd).
    /// When it is not set, the number of leap seconds comes from AST's
    /// internal table, which is kept up to date by hand, so a value is
    /// needed only when the table predates leap seconds that occurred
    /// before the Epoch.
    ///
    /// Default: `BAD_VALUE`, meaning the table is used.
    #[doc(alias = "Dtai")]
    fn dtai(&self) -> f64;
    /// Set Dtai, in seconds; see [`dtai`](Frame::dtai).
    fn set_dtai(&mut self, seconds: f64) -> AstResult<()>;
    /// Whether Dtai is set; see [`dtai`](Frame::dtai).
    fn test_dtai(&self) -> bool;
    /// Clear Dtai; see [`dtai`](Frame::dtai).
    fn clear_dtai(&mut self) -> AstResult<()>;

    /// The UT1-UTC correction, in seconds: the Dut1 attribute, set or
    /// default.
    ///
    /// Used when calculating the Local Apparent Sidereal Time for a
    /// SkyFrame's Epoch, which converting positions to or from the AzEl
    /// system needs.
    /// It should be the difference between the UT1 and UTC timescales at the
    /// moment of the Epoch.
    /// The value depends on unpredictable changes in the Earth's rotation;
    /// values are published by the International Earth Rotation and
    /// Reference Systems Service (IERS).
    /// Leap seconds currently keep it below 1 second, so leaving it unset
    /// usually causes no great error, but it could grow if leap seconds were
    /// abandoned.
    ///
    /// Default: 0.
    #[doc(alias = "Dut1")]
    fn dut1(&self) -> f64;
    /// Set Dut1, in seconds; see [`dut1`](Frame::dut1).
    fn set_dut1(&mut self, seconds: f64) -> AstResult<()>;
    /// Whether Dut1 is set; see [`dut1`](Frame::dut1).
    fn test_dut1(&self) -> bool;
    /// Clear Dut1; see [`dut1`](Frame::dut1).
    fn clear_dut1(&mut self) -> AstResult<()>;

    /// The coordinate system used to describe positions within the domain:
    /// the System attribute, as its name, or `None` for a class with no
    /// System.
    ///
    /// Positions within one physical domain can often be described in
    /// several coordinate systems: a SkyFrame can use galactic or equatorial
    /// coordinates, a SpecFrame frequency, wavelength or velocity.
    /// The System identifies the one a Frame uses, from a set each class
    /// defines; names are case-insensitive, and where a System has
    /// alternative names the first is the one reported.
    /// Each class sets its System with a typed setter, such as
    /// [`BasicFrame::set_system`] and [`SkyFrame::set_system`].
    ///
    /// - Frame: always `"Cartesian"`, and cannot be altered.
    /// - CmpFrame: always `"Compound"`, and cannot be altered.
    /// - FrameSet: the System of its current Frame.
    /// - SkyFrame: see [`SkyFrame::system`].
    /// - SpecFrame: see [`SpecFrame::system`].
    /// - TimeFrame (default `"MJD"`), with the default Unit in parentheses:
    ///   `"MJD"`, Modified Julian Date (d); `"JD"`, Julian Date (d);
    ///   `"JEPOCH"`, Julian epoch (yr); `"BEPOCH"`, Besselian epoch (yr).
    ///   Other units are allowed for all but BEPOCH, converted by simple
    ///   scaling (1 yr = 365.25 d, 1 d = 24 h, 1 h = 60 min, 1 min = 60 s);
    ///   Besselian epochs use tropical years of 365.2422 days, so selecting
    ///   BEPOCH sets the Unit to `"yr"` and a later change of Unit is an
    ///   error.
    ///   A TimeFrame always uses active units, so changing its Unit re-maps
    ///   it within its FrameSet.
    /// - FluxFrame, with the default Unit in parentheses: `"FLXDN"`, flux
    ///   per unit frequency (W/m^2/Hz); `"FLXDNW"`, flux per unit wavelength
    ///   (W/m^2/Angstrom); `"SFCBR"`, surface brightness in frequency units
    ///   (W/m^2/Hz/arcmin**2); `"SFCBRW"`, surface brightness in wavelength
    ///   units (W/m^2/Angstrom/arcmin**2).
    ///   With a Unit set and no System, the System follows from the Unit,
    ///   and reading it is an error if the Unit suits none of them; with
    ///   neither set, the System is FLXDN and the Unit W/m^2/Hz.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Badun` for a FluxFrame whose System follows from a Unit
    ///   that fits no flux system.
    #[doc(alias = "System")]
    fn system_string(&self) -> AstResult<Option<&str>> {
        Ok(None)
    }

    /// The Object-class Ident attribute, blank unless set.
    fn ident(&self) -> &str;
    /// Set Ident. Mirrors `astSetIdent` (`object.c:5225`).
    fn set_ident(&mut self, ident: &str) -> AstResult<()>;
    /// Clear Ident. Mirrors `astClearIdent` (`object.c:5217`).
    fn clear_ident(&mut self) -> AstResult<()>;
    /// Whether Ident is set. Mirrors `astTestIdent` (`object.c:5229`).
    fn test_ident(&self) -> bool;

    /// The Object-class ID attribute, blank unless set. Unlike Ident, a
    /// clone does not inherit it, as C's `astCopy` does not copy it.
    fn id(&self) -> &str;
    /// Set ID. Mirrors `astSetID`.
    fn set_id(&mut self, id: &str) -> AstResult<()>;
    /// Clear ID. Mirrors `astClearID`.
    fn clear_id(&mut self) -> AstResult<()>;
    /// Whether ID is set. Mirrors `astTestID`.
    fn test_id(&self) -> bool;

    /// The Object-class UseDefs attribute as stored on this object, `None`
    /// when unset. C keeps it on every Object, but only Frame classes read
    /// it, so the port keeps it on Frames.
    fn stored_use_defs(&self) -> Option<bool>;
    /// Store UseDefs, `None` to clear it.
    fn store_use_defs(&mut self, value: Option<bool>);
    /// UseDefs, set or default: whether default values may be used for
    /// attributes that have not been set (`object.c:5304`). A FrameSet,
    /// Region or CmpFrame takes an unset value from a Frame it contains.
    fn use_defs(&self) -> bool {
        self.stored_use_defs().unwrap_or(true)
    }
    /// Set UseDefs. Mirrors `astSetUseDefs`.
    fn set_use_defs(&mut self, value: bool) -> AstResult<()> {
        self.store_use_defs(Some(value));
        Ok(())
    }
    /// Clear UseDefs. Mirrors `astClearUseDefs`.
    fn clear_use_defs(&mut self) -> AstResult<()> {
        self.store_use_defs(None);
        Ok(())
    }
    /// Whether UseDefs is set. Mirrors `astTestUseDefs`.
    fn test_use_defs(&self) -> bool {
        self.stored_use_defs().is_some()
    }

    // Match-control attributes, used by `astFindFrame` / `astMatch`.

    /// The minimum number of Frame axes to match: the MinAxes attribute,
    /// set or default.
    ///
    /// Controls how the Frame behaves as the template of
    /// [`find_frame_template`](Frame::find_frame_template): the fewest axes
    /// a target may have to match.
    /// Normally it equals [`naxes`](Frame::naxes), so a template matches
    /// only Frames with as many axes as itself; other values let matching
    /// find Frames with given numbers of axes.
    /// Setting MinAxes raises a set [`max_axes`](Frame::max_axes) that is
    /// smaller, and the default is lowered to stay within a set MaxAxes.
    /// When template and target have different numbers of axes,
    /// [`match_end`](Frame::match_end) decides which axes match.
    ///
    /// Default: [`naxes`](Frame::naxes).
    ///
    /// - CmpFrame: the default is 0, which with the MaxAxes default of
    ///   1000000 lets a CmpFrame template match any target made of a subset
    ///   of its axes; set both to the number of axes to match only Frames of
    ///   that size.
    /// - FrameSet: the MinAxes of its current Frame.
    #[doc(alias = "MinAxes")]
    fn min_axes(&self) -> usize;
    /// Set MinAxes; see [`min_axes`](Frame::min_axes).
    fn set_min_axes(&mut self, n: usize) -> AstResult<()>;
    /// Whether MinAxes is set; see [`min_axes`](Frame::min_axes).
    fn test_min_axes(&self) -> bool;
    /// Clear MinAxes; see [`min_axes`](Frame::min_axes).
    fn clear_min_axes(&mut self) -> AstResult<()>;

    /// The maximum number of Frame axes to match: the MaxAxes attribute,
    /// set or default.
    ///
    /// Controls how the Frame behaves as the template of
    /// [`find_frame_template`](Frame::find_frame_template): the most axes a
    /// target may have to match.
    /// Normally it equals [`naxes`](Frame::naxes), so a template matches
    /// only Frames with as many axes as itself; other values let matching
    /// find Frames with given numbers of axes.
    /// Setting MaxAxes lowers a set [`min_axes`](Frame::min_axes) that is
    /// larger, and the default is raised to stay within a set MinAxes.
    /// When template and target have different numbers of axes,
    /// [`match_end`](Frame::match_end) decides which axes match.
    ///
    /// Default: [`naxes`](Frame::naxes).
    ///
    /// - CmpFrame: the default is 1000000, which with the MinAxes default of
    ///   0 lets a CmpFrame template match any target made of a subset of its
    ///   axes; set both to the number of axes to match only Frames of that
    ///   size.
    /// - FrameSet: the MaxAxes of its current Frame.
    #[doc(alias = "MaxAxes")]
    fn max_axes(&self) -> usize;
    /// Set MaxAxes; see [`max_axes`](Frame::max_axes).
    fn set_max_axes(&mut self, n: usize) -> AstResult<()>;
    /// Whether MaxAxes is set; see [`max_axes`](Frame::max_axes).
    fn test_max_axes(&self) -> bool;
    /// Clear MaxAxes; see [`max_axes`](Frame::max_axes).
    fn clear_max_axes(&mut self) -> AstResult<()>;

    /// Match trailing axes? The MatchEnd attribute, set or default.
    ///
    /// Controls how the Frame behaves as the template of
    /// [`find_frame_template`](Frame::find_frame_template) when template and
    /// target have different numbers of axes.
    /// When false, the first axes of each are matched and any trailing axes
    /// of either are disregarded; when true, the last axes are matched and
    /// any unmatched leading axes are disregarded.
    ///
    /// Default: false.
    ///
    /// - FrameSet: the MatchEnd of its current Frame.
    #[doc(alias = "MatchEnd")]
    fn match_end(&self) -> bool;
    /// Set MatchEnd; see [`match_end`](Frame::match_end).
    fn set_match_end(&mut self, v: bool) -> AstResult<()>;
    /// Whether MatchEnd is set; see [`match_end`](Frame::match_end).
    fn test_match_end(&self) -> bool;
    /// Clear MatchEnd; see [`match_end`](Frame::match_end).
    fn clear_match_end(&mut self) -> AstResult<()>;

    /// Preserve axes? The PreserveAxes attribute, set or default.
    ///
    /// Controls how the Frame behaves as the template of
    /// [`find_frame_template`](Frame::find_frame_template): which axes appear,
    /// and in what order, in the Frame found.
    /// When false, that Frame has the number and order of axes of the
    /// template; when true, it keeps those of the target.
    ///
    /// Default: false, so the Frame found resembles the template.
    ///
    /// - FrameSet: the PreserveAxes of its current Frame.
    #[doc(alias = "PreserveAxes")]
    fn preserve_axes(&self) -> bool;
    /// Set PreserveAxes; see [`preserve_axes`](Frame::preserve_axes).
    fn set_preserve_axes(&mut self, v: bool) -> AstResult<()>;
    /// Whether PreserveAxes is set; see
    /// [`preserve_axes`](Frame::preserve_axes).
    fn test_preserve_axes(&self) -> bool;
    /// Clear PreserveAxes; see [`preserve_axes`](Frame::preserve_axes).
    fn clear_preserve_axes(&mut self) -> AstResult<()>;

    /// Permute axis order? The Permute attribute, set or default.
    ///
    /// Controls how the Frame behaves as the template of
    /// [`find_frame_template`](Frame::find_frame_template): whether the axes
    /// of the target may be permuted to obtain a match.
    /// When false, the template matches a target only without changing the
    /// order of its axes; when true, it permutes the target's axes as
    /// needed.
    ///
    /// Default: true.
    ///
    /// - Frame: the plain Frame behaves as if Permute were true whatever its
    ///   value, since its axes cannot be told apart and match any other
    ///   Frame in any order.
    /// - FrameSet: the Permute of its current Frame.
    #[doc(alias = "Permute")]
    fn permute(&self) -> bool;
    /// Set Permute; see [`permute`](Frame::permute).
    fn set_permute(&mut self, v: bool) -> AstResult<()>;
    /// Whether Permute is set; see [`permute`](Frame::permute).
    fn test_permute(&self) -> bool;
    /// Clear Permute; see [`permute`](Frame::permute).
    fn clear_permute(&mut self) -> AstResult<()>;

    /// Determine how the Unit attribute will be used: the ActiveUnit flag.
    ///
    /// See [`set_active_unit`](Frame::set_active_unit) for what the flag
    /// does.
    /// A Frame holding a sky axis always reports false, whatever was set.
    #[doc(alias = "astGetActiveUnit")]
    fn get_active_unit(&self) -> bool;
    /// Specify how the Unit attribute should be used: set the ActiveUnit
    /// flag.
    ///
    /// The flag controls how the Frame behaves when it is matched against
    /// another by [`find_frame_template`](Frame::find_frame_template) or
    /// [`convert_to`](Frame::convert_to).
    /// When it is false in either Frame, the Mapping ignores any difference
    /// in the [`unit`](Frame::unit) of corresponding template and target
    /// axes, and the units are purely descriptive.
    /// This is the default for a plain Frame, and the behavior of every Frame
    /// before AST 2.0.
    ///
    /// When it is true in both, the Mapping takes account of differences in
    /// units wherever possible: target and template axes with units of
    /// `"km"` and `"m"` are connected by a [`ZoomMap`](crate::mappings::ZoomMap)
    /// with a factor of 1000.
    /// If no Mapping can be found between the units, the two Frames do not
    /// match.
    /// Unit strings are then expected to follow the syntax of FITS-WCS Paper
    /// I, "Representations of world coordinates in FITS" (Greisen &
    /// Calabretta): its named unit symbols, functions, operators and
    /// multiplier prefixes.
    /// Symbols the paper does not list are allowed, but convert only to
    /// units built from the same unknown symbols (`"flops"` converts to
    /// `"Mflops"`).
    /// Common non-standard spellings are accepted too, such as a trailing
    /// `s` on Angstrom, a lower-case `a` in `"angstrom"`, `"micron"` for
    /// `"um"` and `"sec"` for `"s"`.
    ///
    /// With the flag true, setting a new Unit on an axis may also change its
    /// Label and Symbol (Unit `"Hz"` with Label `"frequency"` becomes Label
    /// `"log( frequency )"` when the Unit changes to `"log(Hz)"`), and a
    /// change of Unit clears its Format.
    /// Changing the Unit of the current Frame of a FrameSet then re-maps the
    /// Frame within the FrameSet to take account of the new units.
    ///
    /// The flag is not an attribute: it cannot be cleared.
    ///
    /// # Per class
    ///
    /// - SkyFrame: see [`SkyFrame::set_active_unit`].
    /// - SpecFrame: see [`SpecFrame::set_active_unit`].
    /// - TimeFrame and FluxFrame: always true; the value given is ignored.
    /// - CmpFrame: unless set, true if either component Frame uses active
    ///   units; the value given is also set on both component Frames.
    /// - FrameSet and Region: the flag of the Frame they present.
    #[doc(alias = "astSetActiveUnit")]
    fn set_active_unit(&mut self, value: bool);
    /// Whether the ActiveUnit flag has been set.
    fn test_active_unit(&self) -> bool;

    // Per-axis attributes. Each takes a 1-based external axis and reports
    // `AST__AXIIN` for one out of range. C reaches them through
    // `astGetAxis`, lending the Frame's Digits to an Axis that has none
    // (frame.c `MAKE_GET`); a class over-rides the getters for which C's
    // does.

    /// The label of an axis: the `Label(axis)` attribute of the 1-based
    /// `axis`, set or default.
    ///
    /// A label attached to the axis when it is represented in, for
    /// instance, graphical output.
    /// It is meant for human readers, not software.
    /// When it is not set, a suitable default is supplied.
    ///
    /// Default: `"Axis <n>"`, where `<n>` is 1, 2, ... for successive axes
    /// (counted before any axis permutation).
    ///
    /// - SkyFrame: see [`SkyFrame::label`].
    /// - SpecFrame: see [`SpecFrame::label`].
    /// - DSBSpecFrame: see [`DSBSpecFrame::label`].
    /// - TimeFrame: a default suited to the time system.
    /// - FrameSet: the Label of the axis of its current Frame.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not between 1 and
    ///   [`naxes`](Frame::naxes).
    #[doc(alias = "Label")]
    fn label(&self, axis: usize) -> AstResult<String>;
    /// Set `Label(axis)`; see [`label`](Frame::label).
    fn set_label(&mut self, axis: usize, label: &str) -> AstResult<()>;
    /// Whether `Label(axis)` is set; see [`label`](Frame::label).
    fn test_label(&self, axis: usize) -> AstResult<bool>;
    /// Clear `Label(axis)`; see [`label`](Frame::label).
    fn clear_label(&mut self, axis: usize) -> AstResult<()>;

    /// The symbol of an axis: the `Symbol(axis)` attribute of the 1-based
    /// `axis`, set or default.
    ///
    /// A short-form symbol for coordinate values on the axis, for use where
    /// a full description would be out of place, such as in algebraic
    /// expressions: `"RA"` and `"Dec"`, for instance.
    /// When it is not set, a suitable default is supplied.
    ///
    /// Default: `"<Domain><n>"`, where `<n>` is 1, 2, ... for successive
    /// axes (counted before any axis permutation) and `<Domain>` is the
    /// Frame's [`domain`](Frame::domain), or `"x"` if no Domain is set; the
    /// Domain is truncated if needed so that the whole is at most 50
    /// characters, and white space is replaced by underscores.
    ///
    /// - SkyFrame: see [`SkyFrame::symbol`].
    /// - SpecFrame: see [`SpecFrame::symbol`].
    /// - TimeFrame: a default suited to the time system.
    /// - FrameSet: the Symbol of the axis of its current Frame.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not between 1 and
    ///   [`naxes`](Frame::naxes).
    #[doc(alias = "Symbol")]
    fn symbol(&self, axis: usize) -> AstResult<String>;
    /// Set `Symbol(axis)`; see [`symbol`](Frame::symbol).
    fn set_symbol(&mut self, axis: usize, symbol: &str) -> AstResult<()>;
    /// Whether `Symbol(axis)` is set; see [`symbol`](Frame::symbol).
    fn test_symbol(&self, axis: usize) -> AstResult<bool>;
    /// Clear `Symbol(axis)`; see [`symbol`](Frame::symbol).
    fn clear_symbol(&mut self, axis: usize) -> AstResult<()>;

    /// The physical units of formatted values on an axis: the `Unit(axis)`
    /// attribute of the 1-based `axis`, set or default.
    ///
    /// A textual description of the units of coordinate values on the axis
    /// once formatted into a string with [`format`](Frame::format).
    /// These may differ from the units of the floating point values
    /// application code handles (a SkyFrame always uses radians for those);
    /// [`internal_unit`](Frame::internal_unit) gives the latter.
    /// [`set_active_unit`](Frame::set_active_unit) controls how the units
    /// are used.
    /// Trailing spaces are removed when the Unit is set.
    ///
    /// Default: an empty string.
    ///
    /// - SkyFrame: see [`SkyFrame::unit`].
    /// - SpecFrame: see [`SpecFrame::unit`].
    /// - TimeFrame: a default suited to the System; a Unit that does not
    ///   suit it, such as `"km"`, is an error, and a Besselian epoch takes
    ///   only `"yr"` (`ErrorCode::Attin` from `set_unit`).
    /// - FrameSet: the Unit of the axis of its current Frame.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not between 1 and
    ///   [`naxes`](Frame::naxes).
    #[doc(alias = "Unit")]
    fn unit(&self, axis: usize) -> AstResult<String>;
    /// Set `Unit(axis)`; see [`unit`](Frame::unit).
    /// TimeFrame and FluxFrame also record it against their System, as
    /// [`SpecFrame::set_unit`] does.
    fn set_unit(&mut self, axis: usize, unit: &str) -> AstResult<()>;
    /// Whether `Unit(axis)` is set; see [`unit`](Frame::unit).
    fn test_unit(&self, axis: usize) -> AstResult<bool>;
    /// Clear `Unit(axis)`; see [`unit`](Frame::unit).
    fn clear_unit(&mut self, axis: usize) -> AstResult<()>;

    /// The format specification for values on an axis: the `Format(axis)`
    /// attribute of the 1-based `axis`, set or default.
    ///
    /// The format [`format`](Frame::format) uses to turn coordinate values on
    /// the axis into text.
    /// When it is not set, a default is chosen from the axis's
    /// [`axis_digits`](Frame::axis_digits) so that it shows that many
    /// digits of precision.
    ///
    /// Default: `"%1.<Digits>G"`.
    ///
    /// - Frame: a C `printf` format specification, such as `"%1.7G"`, for
    ///   formatting one value as a double precision number.
    /// - FrameSet: the Format of the axis of its current Frame, whose class
    ///   also decides the syntax.
    /// - SkyFrame: a syntax and default for sexagesimal values suited to the
    ///   celestial coordinate system; see [`SkyFrame::format_str`].
    /// - TimeFrame: the Frame syntax, extended to format values as Gregorian
    ///   calendar dates and times, described below.
    ///
    /// # TimeFrame formats
    ///
    /// A TimeFrame Format is either a Frame (`printf`) format, the default,
    /// or the `iso` syntax:
    ///
    /// - A `printf` format such as `"%1.7G"` formats the value as it is: an
    ///   offset from the TimeOrigin in units of the Unit.
    /// - A Format starting with `iso` converts the value to an absolute MJD,
    ///   adding the TimeOrigin, and formats it as a Gregorian date
    ///   `yyyy-mm-dd`.
    ///   A precision after it (`iso.2`) appends the time of day with that
    ///   many decimal places in the seconds (`iso.0` gives `hh:mm:ss`,
    ///   `iso.2` gives `hh:mm:ss.ss`); without one, the date is rounded to
    ///   the nearest day.
    ///   Date and time are separated by a space, or by `T` if the Format
    ///   ends with `T`.
    ///   Digits is ignored for this syntax.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not between 1 and
    ///   [`naxes`](Frame::naxes).
    #[doc(alias = "Format")]
    fn format_str(&self, axis: usize) -> AstResult<String>;
    /// Set `Format(axis)`; see [`format_str`](Frame::format_str).
    fn set_format_str(&mut self, axis: usize, format: &str) -> AstResult<()>;
    /// Whether `Format(axis)` is set; see [`format_str`](Frame::format_str).
    fn test_format_str(&self, axis: usize) -> AstResult<bool>;
    /// Clear `Format(axis)`; see [`format_str`](Frame::format_str).
    fn clear_format_str(&mut self, axis: usize) -> AstResult<()>;

    /// The number of digits of precision for an axis: the `Digits(axis)`
    /// attribute of the 1-based `axis`, set, or else the Frame's
    /// [`digits`](Frame::digits).
    ///
    /// A value set for an axis over-rides the Frame's Digits for that axis;
    /// [`digits`](Frame::digits) describes the attribute.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not between 1 and
    ///   [`naxes`](Frame::naxes).
    #[doc(alias = "Digits")]
    fn axis_digits(&self, axis: usize) -> AstResult<i32>;
    /// Set `Digits(axis)`; see [`axis_digits`](Frame::axis_digits).
    fn set_axis_digits(&mut self, axis: usize, digits: i32) -> AstResult<()>;
    /// Whether `Digits(axis)` is set; see
    /// [`axis_digits`](Frame::axis_digits).
    fn test_axis_digits(&self, axis: usize) -> AstResult<bool>;
    /// Clear `Digits(axis)`; see [`axis_digits`](Frame::axis_digits).
    fn clear_axis_digits(&mut self, axis: usize) -> AstResult<()>;

    /// Display an axis in the conventional direction? The
    /// `Direction(axis)` attribute of the 1-based `axis`, set or default.
    ///
    /// A hint for how the axis should be displayed in, for instance,
    /// graphical output: true for the conventional sense (increasing left
    /// to right for an abscissa, bottom to top for an ordinate), false for
    /// reversed, as is often done for astronomical magnitude or right
    /// ascension.
    /// It does not affect the behavior of the library; applications are
    /// free to ignore it.
    ///
    /// Default: true.
    ///
    /// - SkyFrame: see [`SkyFrame::direction`].
    /// - FrameSet: the Direction of the axis of its current Frame.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not between 1 and
    ///   [`naxes`](Frame::naxes).
    #[doc(alias = "Direction")]
    fn direction(&self, axis: usize) -> AstResult<bool>;
    /// Set `Direction(axis)`; see [`direction`](Frame::direction).
    fn set_direction(&mut self, axis: usize, direction: bool) -> AstResult<()>;
    /// Whether `Direction(axis)` is set; see [`direction`](Frame::direction).
    fn test_direction(&self, axis: usize) -> AstResult<bool>;
    /// Clear `Direction(axis)`; see [`direction`](Frame::direction).
    fn clear_direction(&mut self, axis: usize) -> AstResult<()>;

    /// The highest axis value to display: the `Top(axis)` attribute of the
    /// 1-based `axis`, set or default.
    ///
    /// The highest value to be displayed on the axis, by a grid plot for
    /// instance.
    ///
    /// Default: `f64::MAX`, so all values are displayed.
    ///
    /// - SkyFrame: see [`SkyFrame::top`].
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not between 1 and
    ///   [`naxes`](Frame::naxes).
    #[doc(alias = "Top")]
    fn top(&self, axis: usize) -> AstResult<f64>;
    /// Set `Top(axis)`; see [`top`](Frame::top).
    fn set_top(&mut self, axis: usize, top: f64) -> AstResult<()>;
    /// Whether `Top(axis)` is set; see [`top`](Frame::top).
    fn test_top(&self, axis: usize) -> AstResult<bool>;
    /// Clear `Top(axis)`; see [`top`](Frame::top).
    fn clear_top(&mut self, axis: usize) -> AstResult<()>;

    /// The lowest axis value to display: the `Bottom(axis)` attribute of the
    /// 1-based `axis`, set or default.
    ///
    /// The lowest value to be displayed on the axis, by a grid plot for
    /// instance.
    ///
    /// Default: `-f64::MAX`, so all values are displayed.
    ///
    /// - SkyFrame: see [`SkyFrame::bottom`].
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not between 1 and
    ///   [`naxes`](Frame::naxes).
    #[doc(alias = "Bottom")]
    fn bottom(&self, axis: usize) -> AstResult<f64>;
    /// Set `Bottom(axis)`; see [`bottom`](Frame::bottom).
    fn set_bottom(&mut self, axis: usize, bottom: f64) -> AstResult<()>;
    /// Whether `Bottom(axis)` is set; see [`bottom`](Frame::bottom).
    fn test_bottom(&self, axis: usize) -> AstResult<bool>;
    /// Clear `Bottom(axis)`; see [`bottom`](Frame::bottom).
    fn clear_bottom(&mut self, axis: usize) -> AstResult<()>;

    /// The Top that the Axis object of a 1-based axis reports itself: C's
    /// `astGetAxisTop( astGetAxis( this, axis ) )`, which is how a
    /// CmpFrame reads the Top of a component's axis (frame.c `MAKE_GET`,
    /// through `astGetAxis`), bypassing any over-ride of `astGetTop` by the
    /// component's class. The same as [`top`](Self::top) unless the class
    /// over-rides `astGetTop`, as SkyFrame does.
    #[doc(hidden)]
    fn axis_object_top(&self, axis: usize) -> AstResult<f64> {
        self.top(axis)
    }

    /// C's protected frame flags (frame.c `GetFrameFlags`): transient
    /// markers that tell a class's SubFrame why a conversion is being made,
    /// such as [`INTFLAG`] during FrameSet integrity restoration.
    #[doc(hidden)]
    fn frame_flags(&self) -> u32 {
        0
    }

    /// Set the frame flags (frame.c `SetFrameFlags`). CmpFrame passes them
    /// on to its components (cmpframe.c `SetFrameFlags`).
    #[doc(hidden)]
    fn set_frame_flags(&mut self, _flags: u32) {}

    /// The Bottom that the Axis object of a 1-based axis reports itself, as
    /// [`axis_object_top`](Self::axis_object_top) does for Top.
    #[doc(hidden)]
    fn axis_object_bottom(&self, axis: usize) -> AstResult<f64> {
        self.bottom(axis)
    }

    /// The physical units of unformatted values on an axis: the read-only
    /// `InternalUnit(axis)` attribute of the 1-based `axis`.
    ///
    /// A textual description of the units of the floating point values on
    /// the axis, as application code handles them.
    /// Usually it equals [`unit`](Frame::unit), since formatted and
    /// unformatted values normally share units; the main exception is a
    /// SkyFrame, whose values are in radians (`"rad"`) whatever its Unit.
    /// An `axis` out of range gives an empty string.
    #[doc(alias = "InternalUnit")]
    fn internal_unit(&self, axis: usize) -> String {
        // C `astGetInternalUnit` (frame.c). Frames whose internal and
        // external units agree trigger no remapping in `MakeFitsFrameSet`.
        self.unit(axis).unwrap_or_default()
    }

    /// The normalized physical units of formatted values on an axis: the
    /// read-only `NormUnit(axis)` attribute of the 1-based `axis`.
    ///
    /// The [`unit`](Frame::unit) of the axis in an equivalent but possibly
    /// simpler form: a Unit of `"s*(m/s)"` gives `"m"`.
    /// When no simplification can be made, as for a blank Unit or a
    /// description of axis values such as `"ddd:mm:ss"`, it equals the Unit.
    /// An `axis` out of range gives an empty string.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::{BasicFrameBuilder, Frame};
    ///
    /// let frame = BasicFrameBuilder::new(1).unit(1, "s*(m/s)").build().unwrap();
    /// assert_eq!(frame.norm_unit(1), "m");
    /// ```
    #[doc(alias = "NormUnit")]
    fn norm_unit(&self, axis: usize) -> String {
        // C `astGetNormUnit` (frame.c:13149) normalizes the Unit the Frame
        // reports rather than the one stored in the Axis, because SpecFrame,
        // TimeFrame, FluxFrame and SkyFrame each report Units of their own.
        // C takes the Axis method instead when the Axis Unit is set, which
        // normalizes the same string. `SkyFrame` overrides this, as it does
        // in C. C reports `AST__FMTER` for a result longer than its
        // 127-character buffer; the port has no such limit.
        crate::units::norm_unit(&self.unit(axis).unwrap_or_default())
    }

    /// Permute the order of this Frame's axes.
    ///
    /// `perm` has one element per axis and lists the axes in their new
    /// order, using the original 1-based axis numbers: after the call, axis
    /// `i` is what was axis `perm[i - 1]`, so `&[2, 1]` swaps two axes.
    /// Only genuine permutations are accepted, each axis appearing once.
    /// Successive permutations are cumulative.
    ///
    /// The default implementation only validates `perm`, which suits a
    /// 1-axis Frame, whose only permutation is `[1]`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Prmin` when `perm` does not have one element per axis,
    ///   holds an axis number out of range, or names an axis twice.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::{BasicFrameBuilder, Frame};
    ///
    /// let mut frame = BasicFrameBuilder::new(2).label(1, "X").label(2, "Y").build().unwrap();
    /// frame.perm_axes(&[2, 1]).unwrap();
    /// assert_eq!(frame.label(1).unwrap(), "Y");
    /// assert!(frame.perm_axes(&[1, 1]).is_err());
    /// ```
    #[doc(alias = "astPermAxes")]
    fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        validate_permutation(perm, self.naxes())?;
        Ok(())
    }

    /// The axis permutation, `perm[external] = internal`, 0-based: C's
    /// protected `astGetPerm`, which a FrameSet and a Region forward to
    /// their current Frame, and which the Frame part of a dump uses to write
    /// its values in internal axis order (frame.c `Dump`).
    #[doc(hidden)]
    fn axis_perm(&self) -> Vec<usize> {
        (0..self.naxes()).collect()
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

    /// Find a coordinate system with specified characteristics.
    ///
    /// Uses `template`, an instance of the kind of Frame wanted, to search
    /// this Frame, the target, for a coordinate system with a given set of
    /// characteristics, and returns a [`FrameSet`] describing the system
    /// found and how to convert to and from it, or `None` if there is none.
    /// It answers questions about coordinate information of unknown form,
    /// such as whether there is a wavelength scale, a 2-dimensional
    /// coordinate system or a celestial coordinate system, and reconciles a
    /// preference for a coordinate system with what is available.
    /// [`FrameSet::find_frame`] is the same search with the domain list as
    /// a slice.
    ///
    /// The systems searched are the target's own and every "virtual"
    /// system reachable from it by a built-in conversion, such as the
    /// celestial coordinate conversions; for a FrameSet target, the union
    /// of those of all its Frames.
    /// An attribute set in the template must have the same value in the
    /// system found; one left unset takes its value from one of the target's
    /// actual (not virtual) systems.
    /// The result is a hybrid Frame with the attributes set in the template
    /// and the rest from the target, the best compromise between what was
    /// asked for and what is available.
    ///
    /// The returned FrameSet holds two Frames: its base Frame is (the base
    /// Frame of) the target, and its current Frame the system found, related
    /// by the Mapping from target coordinates to the new system.
    /// Used as a Mapping it converts between them; used as a Frame its
    /// attributes describe the system found.
    /// The Mapping aligns in the coordinate system given by the template's
    /// AlignSystem attribute.
    ///
    /// # Arguments
    ///
    /// - `domainlist`: a comma-separated priority list of Domains.
    ///   A system whose Domain is the first entry is sought first, then the
    ///   second, and so on; an empty field (two adjacent commas, or an
    ///   empty string) accepts any Domain.
    ///   Matching ignores case and white space.
    ///   Setting the template's Domain and then giving a list that holds
    ///   neither that Domain nor an empty field finds nothing.
    ///
    /// # Using templates
    ///
    /// A Frame is found if the template matches it and its Domain is in
    /// `domainlist` (or the list has an empty field).
    /// The template matches according to these rules:
    ///
    /// - A template matches a Frame of its own class or of a more
    ///   specialized class: a SkyFrame template matches any SkyFrame but not
    ///   a plain Frame, and a plain Frame template matches any class of
    ///   Frame.
    /// - A template of any class can match a CmpFrame that contains a Frame
    ///   of its class, provided its [`min_axes`](Frame::min_axes) is no more
    ///   and its [`max_axes`](Frame::max_axes) no less than the number of
    ///   axes of the CmpFrame.
    /// - A CmpFrame template takes its MinAxes and MaxAxes from its
    ///   components, so set those on the component templates before
    ///   combining them.
    /// - A template with one of its main attributes set matches only Frames
    ///   with the same value, or that a built-in conversion can give that
    ///   value; descriptive attributes such as Title, Label and Symbol do
    ///   not restrict the match.
    ///   Setting the template's [`domain`](Frame::domain) restricts the
    ///   search to Frames of that Domain, as `domainlist` does.
    /// - A template normally matches only a Frame with as many axes as
    ///   itself; MinAxes, MaxAxes and [`match_end`](Frame::match_end) change
    ///   that, and [`permute`](Frame::permute) and
    ///   [`preserve_axes`](Frame::preserve_axes) control whether permuted
    ///   axes match and whether their order is kept in the result.
    ///
    /// Some templates:
    ///
    /// - A 3-axis [`BasicFrame`] with nothing set finds any 3-dimensional
    ///   coordinate system.
    /// - A [`SkyFrame`] finds a celestial coordinate system of any kind; with
    ///   MaxAxes set to 100 it also finds one inside a CmpFrame of up to 100
    ///   axes, the returned Mapping including a [`PermMap`](crate::mappings::PermMap) that selects
    ///   its axes.
    ///   With System set to FK5 it finds an FK5 system, taking the Equinox
    ///   from the target.
    ///   With Permute false it finds one only if its axes are in the
    ///   conventional (longitude, latitude) order.
    /// - A 2-axis BasicFrame with `domainlist` `"SKY,PIXEL,"` seeks a SKY
    ///   system, then a PIXEL one, then any 2-dimensional one.
    /// - A 1-axis BasicFrame with Domain WAVELENGTH, or with nothing set and
    ///   `domainlist` `"WAVELENGTH"`, finds a 1-dimensional wavelength
    ///   system.
    /// - A 1-axis BasicFrame with MaxAxes 3 finds a system of 1, 2 or 3
    ///   dimensions and returns a 1-dimensional Frame related to it by a
    ///   Mapping that extracts its first axis; with PreserveAxes set as well,
    ///   the Frame returned has the axes of the system found.
    ///
    /// # Per class
    ///
    /// - FrameSet: each field of `domainlist` is taken in turn, and within
    ///   it the template is matched against the target's Frames in the
    ///   order current Frame, base Frame, then the others in the order they
    ///   were added; the first match whose Frame has a Domain in that field
    ///   is accepted.
    ///
    /// # Errors
    ///
    /// Passes on the error of the class-specific matching of the template
    /// against the target ([`match_template`](Frame::match_template) and
    /// [`sub_frame`](Frame::sub_frame)).
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::{BasicFrameBuilder, Frame};
    ///
    /// let target = BasicFrameBuilder::new(2).domain("PIXEL").build().unwrap();
    /// let template = BasicFrameBuilder::new(2).build().unwrap();
    /// let found = target
    ///     .find_frame_template(&template, "sky,pixel,")
    ///     .unwrap()
    ///     .expect("a 2-dimensional system");
    /// assert_eq!(found.current().domain(), "PIXEL");
    /// assert!(target.find_frame_template(&template, "SKY").unwrap().is_none());
    /// ```
    ///
    /// # See also
    ///
    /// `astFindFrame`.
    #[doc(alias = "astFindFrame")]
    fn find_frame_template(
        &self,
        template: &dyn Frame,
        domainlist: &str,
    ) -> AstResult<Option<find_frame::FindFrameResult>> {
        find_frame::default_find_frame(self.as_dyn_frame(), template, domainlist)
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

    /// Overlay the attributes set on this Frame, the template, onto
    /// `result`. `template_axes` is parallel to `result`'s axes, giving the
    /// (zero-based) template axis from which each result axis takes its
    /// axis attributes, or `-1` for none; an empty slice means the same
    /// index. This is C's `astOverlay` (frame.c:8513), a method of the
    /// template: each class transfers the attributes it defines, and only
    /// onto a result of its own class.
    ///
    /// The default is the Frame-class part of C's Overlay alone.
    fn overlay(&self, template_axes: &[i32], result: &mut dyn Frame) -> AstResult<()> {
        overlay::frame_overlay(self, template_axes, result)
    }

    /// A copy of the Axis object on 1-based `axis`, for `AxisOverlay`
    /// (C `astGetAxis` followed by `astAxisOverlay`). `None` for a class
    /// whose axes the port does not hold as Axis objects.
    #[doc(hidden)]
    fn axis_overlay_source(&self, _axis: usize) -> Option<overlay::AxisOverlaySource> {
        None
    }

    /// Overlay `source` onto the Axis object on 1-based `axis`, as C's
    /// `astAxisOverlay` does (axis.c:1226, skyaxis.c:1370).
    #[doc(hidden)]
    fn apply_axis_overlay(&mut self, _axis: usize, _source: &overlay::AxisOverlaySource) {}

    /// Set this Frame's System and AlignSystem from `template`'s, where set
    /// and valid for this class: the System part of C's Frame `Overlay`,
    /// which skips a System `astValidateSystem` rejects (frame.c:8580).
    #[doc(hidden)]
    fn overlay_system_from(&mut self, _template: &dyn std::any::Any) -> AstResult<()> {
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

    /// Calculate an offset along a geodesic curve in a 2-dimensional Frame.
    ///
    /// Returns `(point2, direction)`: `point2` is the point `offset` along
    /// the geodesic curve that leaves `point1` at `angle`, and `direction` is
    /// the direction of the curve at `point2`.
    /// In a plain Frame the curve is a straight line; in a SkyFrame it is a
    /// great circle.
    /// The geodesic is the path of shortest distance, as
    /// [`distance`](Frame::distance) measures it.
    ///
    /// Both angles are in radians, measured from the positive direction of
    /// axis 2, with positive rotation from the positive direction of axis 2
    /// to the positive direction of axis 1.
    /// A positive `offset` goes in the direction of `angle`, a negative one
    /// in the opposite direction.
    ///
    /// Every output is `BAD_VALUE` if any input is `BAD_VALUE`.
    ///
    /// # Panics
    ///
    /// If any point holds fewer than 2 coordinates.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Naxin` when the Frame is not 2-dimensional.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::{BasicFrameBuilder, Frame};
    ///
    /// let frame = BasicFrameBuilder::new(2).build().unwrap();
    /// let (p, dir) = frame.offset2(&[1.0, 1.0], 0.0, 2.0).unwrap();
    /// assert_eq!((p, dir), (vec![1.0, 3.0], 0.0));
    /// ```
    #[doc(alias = "astOffset2")]
    fn offset2(&self, point1: &[f64], angle: f64, offset: f64) -> AstResult<(Vec<f64>, f64)> {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        if self.naxes() != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                "astOffset2 can only be used with 2-dimensional Frames",
            ));
        }
        assert_points_len("offset2", 2, &[point1]);
        if is_bad(point1[0]) || is_bad(point1[1]) || is_bad(angle) || is_bad(offset) {
            return Ok((vec![BAD_VALUE, BAD_VALUE], BAD_VALUE));
        }
        let p2 = vec![
            point1[0] + crate::math::sin(angle) * offset,
            point1[1] + crate::math::cos(angle) * offset,
        ];
        Ok((p2, angle))
    }

    /// Find the point of intersection between two geodesic curves.
    ///
    /// Each curve is given by two points on it, `a1` and `a2` for the
    /// first, `b1` and `b2` for the second, and the result is the
    /// coordinates where they cross.
    /// In a plain Frame the curves are straight lines; in a SkyFrame they
    /// are great circles, which in general cross at two diametrically
    /// opposite points, and the one closer to `a1` is returned.
    /// The geodesic is the path of shortest distance, as
    /// [`distance`](Frame::distance) measures it.
    ///
    /// Every coordinate of the result is `BAD_VALUE` if any input
    /// coordinate is `BAD_VALUE`, if the two points of either curve
    /// coincide, or if the curves do not cross.
    ///
    /// # Panics
    ///
    /// If any point holds fewer than 2 coordinates.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Naxin` when the Frame is not 2-dimensional.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::{BasicFrameBuilder, Frame};
    ///
    /// let frame = BasicFrameBuilder::new(2).build().unwrap();
    /// let cross = frame
    ///     .intersect(&[0.0, 0.0], &[2.0, 2.0], &[0.0, 2.0], &[2.0, 0.0])
    ///     .unwrap();
    /// assert_eq!(cross, vec![1.0, 1.0]);
    /// ```
    #[doc(alias = "astIntersect")]
    fn intersect(&self, a1: &[f64], a2: &[f64], b1: &[f64], b2: &[f64]) -> AstResult<Vec<f64>> {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        if self.naxes() != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                "astIntersect can only be used with 2-dimensional Frames",
            ));
        }
        assert_points_len("intersect", 2, &[a1, a2, b1, b2]);
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

    /// Normalize a collection of points.
    ///
    /// `points` holds one coordinate per axis for each point, and the
    /// normalized points are returned.
    /// There are two ways to normalize:
    ///
    /// - With `contig` false, each point is normalized independently, as
    ///   [`norm`](Frame::norm) does, which puts the values into the range
    ///   expected for display.
    /// - With `contig` true, the values are put into whichever range avoids
    ///   discontinuities within the collection, which suits points that may
    ///   span a discontinuity of the coordinate system.
    ///   Points in a SkyFrame that span longitude zero are put into the
    ///   longitude range -pi to +pi rather than 0 to 2*pi.
    ///
    /// A Frame whose coordinates are unconstrained never changes them.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Ncpin` when `points` does not have one coordinate per
    ///   axis.
    #[doc(alias = "astNormPoints")]
    fn norm_points(&self, points: &PointSet, contig: bool) -> AstResult<PointSet> {
        // C's public `astNormPoints` validates its arguments and calls
        // `astNormalPoints` (frame.c), the per-class method this is: each
        // point is normalized as `norm` does, and when `contig` is set a
        // class whose axes wrap may also choose among equivalent values so
        // that the points form one contiguous group (a SkyFrame keeps the
        // longitudes on one side of the widest gap between them).
        let _ = contig;
        norm_each_point(self.as_dyn_frame(), points)
    }

    /// A grid of about `size` points covering the box from `lbnd` to
    /// `ubnd`, one bound per axis. Mirrors C `astFrameGrid` (frame.c), a
    /// per-class method: a Frame spaces the points evenly along each axis,
    /// giving more ticks to the longer axes among those that share a Unit;
    /// a SkyFrame lays them on parallels, roughly equal-area; a CmpFrame
    /// takes the cross product of its components' grids. Fails with
    /// `AST__ATTIN` for a `size` below 1 or a bound pair that is bad.
    fn frame_grid(&self, size: i32, lbnd: &[f64], ubnd: &[f64]) -> AstResult<PointSet> {
        frame_grid_default(self.as_dyn_frame(), size, lbnd, ubnd)
    }

    /// Whether `val` lies between `lo` and `hi` on a 1-based `axis`,
    /// including the limits when `closed`. Mirrors C `astAxIn` (frame.c),
    /// which asks the Axis object: a plain Axis compares the values
    /// (axis.c `AxisIn`), and a SkyAxis first moves `val` by whole turns
    /// into the range (skyaxis.c `AxisIn`).
    fn ax_in(&self, axis: usize, lo: f64, hi: f64, val: f64, closed: bool) -> bool {
        match self.get_axis(axis) {
            AxisKind::Sky { .. } => skyaxis::axis_in(lo, hi, val, closed),
            AxisKind::Plain => axis::axis_in(lo, hi, val, closed),
        }
    }

    /// Find the distance between two axis values.
    ///
    /// Returns the signed increment from `v1` to `v2` on the 1-based
    /// `axis`.
    /// On a plain axis this is the difference `v2 - v1`; on a sky axis it
    /// is the difference wrapped into the range -pi to +pi.
    /// The result is `BAD_VALUE` if either value is `BAD_VALUE`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not between 1 and
    ///   [`naxes`](Frame::naxes).
    #[doc(alias = "astAxDistance")]
    fn ax_distance(&self, axis: usize, v1: f64, v2: f64) -> AstResult<f64> {
        // C `astAxDistance` (frame.c:1466) fetches the Axis with `astGetAxis`
        // and calls `astAxisDistance` on it.
        self.validate_frame_axis(axis, "astAxDistance")?;
        Ok(self.get_axis(axis).distance(v1, v2))
    }

    /// Add an increment onto a supplied axis value.
    ///
    /// Returns the value formed by adding the signed increment `dist` to
    /// `v1` on the 1-based `axis`.
    /// On a plain axis this is the sum `v1 + dist`; on a sky axis the sum
    /// is then wrapped as [`ax_norm`](Frame::ax_norm) with operation 0 wraps
    /// it.
    /// The result is `BAD_VALUE` if either value is `BAD_VALUE`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not between 1 and
    ///   [`naxes`](Frame::naxes).
    #[doc(alias = "astAxOffset")]
    fn ax_offset(&self, axis: usize, v1: f64, dist: f64) -> AstResult<f64> {
        // C `astAxOffset` (frame.c:1744) fetches the Axis with `astGetAxis`
        // and calls `astAxisOffset` on it.
        self.validate_frame_axis(axis, "astAxOffset")?;
        Ok(self.get_axis(axis).offset(v1, dist))
    }

    /// Return the angle from an axis to a line through two points.
    ///
    /// Returns the angle, as seen from point `a`, from the positive
    /// direction of the 1-based `axis` to the geodesic curve joining `a` to
    /// `b`, in radians.
    /// The geodesic is the path of shortest distance, as
    /// [`distance`](Frame::distance) measures it.
    ///
    /// In a 2-dimensional Frame the angle is in the range -pi to +pi, with
    /// positive rotation in the sense from the positive direction of axis 2
    /// to the positive direction of axis 1.
    /// With more axes it is always positive, between zero and pi.
    ///
    /// The result is `BAD_VALUE` if any input coordinate is `BAD_VALUE`, or
    /// if the angle is undefined.
    ///
    /// # Panics
    ///
    /// If any point holds fewer than [`naxes`](Frame::naxes) coordinates.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not between 1 and
    ///   [`naxes`](Frame::naxes).
    /// - Passes on the error of [`angle`](Frame::angle).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::f64::consts::{FRAC_PI_2, PI};
    /// use ast_core::frames::{BasicFrameBuilder, Frame};
    ///
    /// let frame = BasicFrameBuilder::new(2).build().unwrap();
    /// // A line along +axis 2 is a quarter turn from +axis 1.
    /// let angle = frame.ax_angle(&[0.0, 0.0], &[0.0, 1.0], 1).unwrap();
    /// assert!((angle + FRAC_PI_2).abs() < 1e-12);
    /// // A line along -axis 1 is a half turn from +axis 1.
    /// let angle = frame.ax_angle(&[0.0, 0.0], &[-1.0, 0.0], 1).unwrap();
    /// assert!((angle.abs() - PI).abs() < 1e-12);
    /// ```
    #[doc(alias = "astAxAngle")]
    fn ax_angle(&self, a: &[f64], b: &[f64], axis: usize) -> AstResult<f64> {
        // C `astAxAngle` (frame.c:1324) offsets `a` slightly in the +axis
        // direction and calls `astAngle`.
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        // frame.c:1442 validates the axis first.
        self.validate_frame_axis(axis, "astAxAngle")?;
        assert_points_len("ax_angle", self.naxes(), &[a, b]);
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

    /// Normalize an array of axis values.
    ///
    /// Modifies `values`, all on the 1-based `axis`, so that they are
    /// normalized in the manner `oper` selects:
    ///
    /// - 0: as [`norm`](Frame::norm) normalizes a value on this axis.
    /// - 1: chosen automatically so that the values have the smallest
    ///   range.
    ///
    /// Values on a plain axis are returned unchanged.
    /// On a sky axis, which has a discontinuity at zero longitude, operation
    /// 0 puts a longitude into the range 0 to 2*pi, and operation 1 puts the
    /// longitudes into 0 to 2*pi or -pi to +pi, whichever gives the smaller
    /// range; a latitude axis always takes the range -pi to +pi.
    /// A SkyFrame's own axes are both treated as longitude axes, as C's are;
    /// a latitude axis picked out of a SkyFrame into another Frame is
    /// treated as a latitude axis.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Axiin` when `axis` is not between 1 and
    ///   [`naxes`](Frame::naxes).
    /// - `ErrorCode::Oprin` when `oper` is not 0 or 1.
    #[doc(alias = "astAxNorm")]
    fn ax_norm(&self, axis: usize, oper: i32, values: &mut [f64]) -> AstResult<()> {
        // C `astAxNorm` (frame.c:1560) fetches the Axis with `astGetAxis`,
        // validates `oper`, and calls `astAxisNormValues` on it.
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

    /// Resolve a vector into two orthogonal components.
    ///
    /// The vector from `point1` to `point2` is the basis vector, and the
    /// vector from `point1` to `point3` is resolved into components parallel
    /// and perpendicular to it.
    /// Returns `(point4, d1, d2)`:
    ///
    /// - `point4`: the point of closest approach of the basis vector to
    ///   `point3`.
    /// - `d1`: the distance from `point1` to `point4`, the length of the
    ///   parallel component, positive in the sense of movement from
    ///   `point1` to `point2`.
    /// - `d2`: the distance from `point4` to `point3`, the length of the
    ///   perpendicular component, which is never negative.
    ///
    /// Each vector is the path of shortest distance between its points, as
    /// [`distance`](Frame::distance) measures it.
    /// Every output is `BAD_VALUE` if any input coordinate is `BAD_VALUE`,
    /// or if the outputs are undefined, as for a basis vector of zero
    /// length.
    ///
    /// # Panics
    ///
    /// If any point holds fewer than [`naxes`](Frame::naxes) coordinates.
    ///
    /// # Errors
    ///
    /// No Frame class raises an error of its own; a CmpFrame, FrameSet or
    /// Region passes on any error from `resolve` on the Frames it holds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::{BasicFrameBuilder, Frame};
    ///
    /// let frame = BasicFrameBuilder::new(2).build().unwrap();
    /// let (p4, d1, d2) = frame.resolve(&[0.0, 0.0], &[1.0, 0.0], &[3.0, 4.0]).unwrap();
    /// assert_eq!((p4, d1, d2), (vec![3.0, 0.0], 3.0, 4.0));
    /// ```
    #[doc(alias = "astResolve")]
    fn resolve(
        &self,
        point1: &[f64],
        point2: &[f64],
        point3: &[f64],
    ) -> AstResult<(Vec<f64>, f64, f64)> {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        let naxes = self.naxes();
        assert_points_len("resolve", naxes, &[point1, point2, point3]);
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
                let bv = crate::math::sqrt(bv);
                d1 = dp / bv;
                for i in 0..naxes {
                    point4[i] = point1[i] + (d1 / bv) * (point2[i] - point1[i]);
                }
                let mut s = 0.0;
                for i in 0..naxes {
                    let c = point3[i] - point4[i];
                    s += c * c;
                }
                d2 = crate::math::sqrt(s);
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
    ///
    /// # Panics
    ///
    /// If `point1` or `point2` holds fewer than [`naxes`](Frame::naxes)
    /// coordinates.
    fn resolve_points(
        &self,
        point1: &[f64],
        point2: &[f64],
        input: &crate::coords::PointSet,
    ) -> AstResult<crate::coords::PointSet> {
        use crate::constants::BAD_VALUE;
        use crate::mappings::helpers::is_bad;
        let nax = self.naxes();
        assert_points_len("resolve_points", nax, &[point1, point2]);
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
            let bv = crate::math::sqrt(bv);
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
                    crate::math::sqrt(s)
                } else {
                    let x1 = c * basisv[0];
                    let y1 = c * basisv[1];
                    let x2 = row[0] - (point1[0] + x1);
                    let y2 = row[1] - (point1[1] + y1);
                    let mut m = crate::math::sqrt(x2 * x2 + y2 * y2);
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

    /// Find any corresponding axes in two Frames.
    ///
    /// Returns one element for each axis of `frm2`: the 1-based index of
    /// the corresponding axis of this Frame, or 0 when it has none.
    ///
    /// Axes correspond when a Mapping can be found between them by
    /// [`find_frame_template`](Frame::find_frame_template) or
    /// [`convert_to`](Frame::convert_to), so they need not be identical:
    /// the axes of two SkyFrames correspond even when they describe
    /// different celestial coordinate systems.
    ///
    /// # Errors
    ///
    /// Passes on the error of the class-specific matching of the Frames'
    /// axes ([`match_template`](Frame::match_template)).
    ///
    /// # Examples
    ///
    /// ```
    /// use ast_core::frames::{BasicFrameBuilder, CmpFrame, Frame, SkyFrame};
    ///
    /// let sky = SkyFrame::unset_system();
    /// let spec = BasicFrameBuilder::new(1).build().unwrap();
    /// let cmp = CmpFrame::new(Box::new(spec), Box::new(sky.clone()));
    /// assert_eq!(cmp.match_axes(&sky).unwrap(), vec![2, 3]);
    /// ```
    #[doc(alias = "astMatchAxes")]
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
        probe.set_preserve_axes(true)?;
        probe.set_max_axes(10000)?;
        probe.set_min_axes(1)?;

        for (ifirst, slot) in axes.iter_mut().enumerate() {
            let (pfrm, pax) = self.primary_frame(ifirst);
            if let Some(m) = find_frame::match_with_subclass(probe.as_ref(), pfrm, true)? {
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
/// A copy of a Frame of a specialised class cast into a plain Frame, as C's
/// `astCast` gives it for a class with no `Cast` override: `astCastCopy`
/// (object.c) keeps the whole Frame-class part of the object, so every
/// Frame-class attribute in `attrs` carries over, set or unset as it was.
pub(crate) fn cast_to_basic_frame(
    frame: &dyn Frame,
    attrs: &frame_attrs::FrameAttrs,
) -> AstResult<BasicFrame> {
    let mut bf = reduce_to_basic_frame(frame)?;
    bf.set_frame_attrs(attrs.clone());
    Ok(bf)
}

pub(crate) fn reduce_to_basic_frame(frame: &dyn Frame) -> AstResult<BasicFrame> {
    let n = frame.naxes();
    let mut bf = BasicFrameBuilder::new(n).build()?;
    // Frame-level Domain: copied by astOverlay (cmpframe.c:1341).
    if frame.test_domain() {
        bf.set_domain(frame.domain().to_string());
    }
    // Per-axis Axis metadata: C copies each source Axis into the basic
    // Frame with astSetAxis (cmpframe.c:1335-1338), so the Label, Unit and
    // Symbol the source reports, and the Format, Digits and Direction set
    // on it, carry through.
    for axis in 1..=n {
        bf.set_label(axis, &frame.label(axis)?)?;
        bf.set_unit(axis, &frame.unit(axis)?)?;
        bf.set_symbol(axis, &frame.symbol(axis)?)?;
        if frame.test_format_str(axis)? {
            bf.set_format_str(axis, &frame.format_str(axis)?)?;
        }
        // Digits/Direction default on a plain Frame to 7 and 1; only a
        // non-default value carries information worth transferring (the
        // BasicFrame already defaults to these). Setting unconditionally
        // would spuriously flag the attribute as explicitly set.
        let digits = frame.axis_digits(axis)?;
        if digits != 7 {
            bf.set_axis_digits(axis, digits)?;
        }
        let direction = frame.direction(axis)?;
        if !direction {
            bf.set_direction(axis, direction)?;
        }
    }
    Ok(bf)
}

/// frame.c `ReportPoints`: one line per point, `(in, ...) --> (out, ...)`,
/// the input values formatted by `frame_in` and the output values by
/// `frame_out`, each with [`Frame::format`].
pub(crate) fn frame_report_lines(
    frame_in: &dyn Frame,
    frame_out: &dyn Frame,
    inp: &PointSet,
    out: &PointSet,
) -> String {
    let fmt =
        |frame: &dyn Frame, coord: usize, v: f64| frame.format(coord + 1, v).unwrap_or_default();
    crate::mappings::report::report_lines(
        inp,
        out,
        |c, v| fmt(frame_in, c, v),
        |c, v| fmt(frame_out, c, v),
    )
}

/// frame.c `FrameGrid`: evenly spaced points along each axis, the number of
/// ticks on an axis scaled by its length over the geometric mean length of
/// the axes that share its Unit, the first axis varying fastest.
pub(crate) fn frame_grid_default(
    frame: &dyn Frame,
    size: i32,
    lbnd: &[f64],
    ubnd: &[f64],
) -> AstResult<PointSet> {
    use crate::constants::BAD_VALUE;
    let naxes = frame.naxes();
    let mut nsame = vec![1i32; naxes];
    let mut gmean = vec![0.0f64; naxes];
    let mut bad = false;
    for iax in 0..naxes {
        if ubnd[iax] == BAD_VALUE && lbnd[iax] == BAD_VALUE {
            bad = true;
            break;
        }
        gmean[iax] = ubnd[iax] - lbnd[iax];
        let unit = frame.unit(iax + 1)?;
        for jax in 0..naxes {
            if jax != iax && unit == frame.unit(jax + 1)? {
                nsame[iax] += 1;
                gmean[iax] *= ubnd[jax] - lbnd[jax];
            }
        }
    }
    if bad || size < 1 {
        return Err(AstError::new(
            ErrorCode::Attin,
            if bad {
                "astFrameGrid: One of more of the supplied bounds is AST__BAD \
                 (programming error)."
                    .to_string()
            } else {
                format!(
                    "astFrameGrid: The supplied grid size ({size}) is invalid (programming error)."
                )
            },
        ));
    }
    // C assigns the nominal tick count and each axis's count to `int`,
    // truncating.
    let ntick0 = (crate::math::powf(size as f64, 1.0 / naxes as f64) as i32).max(2);
    for iax in 0..naxes {
        gmean[iax] = crate::math::powf(gmean[iax].abs(), 1.0 / nsame[iax] as f64);
    }
    let ntick: Vec<i32> = (0..naxes)
        .map(|iax| ((ntick0 as f64 * (ubnd[iax] - lbnd[iax]) / gmean[iax]) as i32).max(2))
        .collect();
    let np: i32 = ntick.iter().product();
    let step: Vec<f64> = (0..naxes)
        .map(|iax| (ubnd[iax] - lbnd[iax]) / (ntick[iax] - 1) as f64)
        .collect();
    let mut out = PointSet::zeros(np as usize, naxes);
    let mut pi = vec![0i32; naxes];
    for ipp in 0..np as usize {
        for (iax, v) in out.point_mut(ipp).iter_mut().enumerate() {
            *v = lbnd[iax] + pi[iax] as f64 * step[iax];
        }
        for iax in 0..naxes {
            pi[iax] += 1;
            if pi[iax] > ntick[iax] - 1 {
                pi[iax] = 0;
            } else {
                break;
            }
        }
    }
    Ok(out)
}

/// frame.c `NormalPoints`: check that `points` has one value per axis of
/// `frame`, then normalise each point with [`Frame::norm`].
pub(crate) fn norm_each_point(frame: &dyn Frame, points: &PointSet) -> AstResult<PointSet> {
    let naxes = frame.naxes();
    if points.ncoord() != naxes {
        return Err(AstError::new(
            ErrorCode::Ncpin,
            format!(
                "astNormalPoints: Bad number of coordinate values ({}) in input PointSet; \
                 the Frame has {naxes} axes.",
                points.ncoord()
            ),
        ));
    }
    let mut out = points.clone();
    for i in 0..out.npoint() {
        frame.norm(&mut out.point_mut(i))?;
    }
    Ok(out)
}

/// Create a new Frame by picking axes from an existing one.
///
/// Returns `(new_frame, map)`.
/// The new Frame's axes are copied from `frame`, along with its other
/// attributes such as its Title.
/// `axes` lists the 1-based numbers of the axes of `frame` to copy, in the
/// order wanted in the new Frame; any number of them may be picked, in any
/// order, but each only once.
/// An entry of 0, or greater than the number of axes of `frame`, adds an
/// axis with default attributes.
///
/// `map` converts coordinates from `frame` into the new Frame in its forward
/// direction, and back in its inverse: a [`PermMap`](crate::mappings::PermMap),
/// or a [`UnitMap`](crate::mappings::UnitMap) when the axes are picked in
/// their original order.
/// A default axis takes the value `BAD_VALUE` in the forward direction.
///
/// The new Frame is an independent copy: changing it does not change
/// `frame`.
/// Its class depends on the axes picked: picking both axes of a
/// [`SkyFrame`] gives a SkyFrame, but a single axis cannot be a valid
/// SkyFrame and gives a plain Frame instead (whose axis keeps behaving as a
/// sky axis).
/// Picking from a [`FrameSet`] is the same as picking from its current
/// Frame, and the result is not a FrameSet.
/// For a Region, [`pick_axes_region`](crate::regions::pick_axes_region)
/// tries to keep the bounds on the axes picked.
///
/// # Errors
///
/// - `ErrorCode::Selin` when an axis is picked more than once.
///
/// # Examples
///
/// ```
/// use ast_core::frames::{BasicFrameBuilder, Frame, pick_axes};
/// use ast_core::mappings::Mapping;
///
/// let frame = BasicFrameBuilder::new(3).label(3, "Z").build().unwrap();
/// let (picked, map) = pick_axes(&frame, &[3, 1]).unwrap();
/// assert_eq!(picked.naxes(), 2);
/// assert_eq!(picked.label(1).unwrap(), "Z");
/// assert_eq!((map.nin(), map.nout()), (3, 2));
/// assert!(pick_axes(&frame, &[1, 1]).is_err());
/// // Axis 5 does not exist, so the second axis of the result is a default one.
/// let (picked, _) = pick_axes(&frame, &[1, 5]).unwrap();
/// assert_eq!(picked.naxes(), 2);
/// ```
#[doc(alias = "astPickAxes")]
pub fn pick_axes(frame: &dyn Frame, axes: &[usize]) -> AstResult<(DynFrame, DynMapping)> {
    // C's `astPickAxes` (frame.c:8799). An axis number of 0 or beyond the
    // Frame's axes gives a new axis with default attributes. The new Frame
    // and Mapping are what the class's SubFrame produces with no template,
    // so picking every axis of a SkyFrame gives a SkyFrame, a single sky
    // axis keeps its SkyAxis, and an identity selection gives a UnitMap.
    // The public interface is 1-based; C's internal one, and SubFrame's,
    // 0-based (frame.c:16220).
    let zero_based: Vec<i32> = axes.iter().map(|&ax| ax as i32 - 1).collect();
    validate_axis_selection(frame, &zero_based, "astPickAxes")?;
    let (map, new_frame) = frame
        .sub_frame(None, zero_based.len(), &zero_based, &[])?
        .ok_or_else(|| AstError::new(ErrorCode::Inter, "astPickAxes: SubFrame found no result"))?;
    Ok((new_frame, map))
}

/// Reject a selection of 0-based `axes` that selects any axis of `frame`
/// more than once (frame.c `ValidateAxisSelection`). Out-of-range entries
/// are allowed: they select no axis.
pub(crate) fn validate_axis_selection(
    frame: &dyn Frame,
    axes: &[i32],
    method: &str,
) -> AstResult<()> {
    let mut count = vec![0usize; frame.naxes()];
    for &axis in axes {
        if let Some(c) = usize::try_from(axis).ok().and_then(|a| count.get_mut(a)) {
            *c += 1;
        }
    }
    if count.iter().any(|&c| c > 1) {
        return Err(AstError::new(
            ErrorCode::Selin,
            format!("{method}: Invalid axis selection - each axis may be selected only once."),
        ));
    }
    Ok(())
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

/// Panic unless every point in `points` holds at least `naxes` coordinates.
///
/// A point given as a slice shorter than the Frame's axes is a caller error,
/// which C cannot detect; this makes it a panic that names the shortfall
/// rather than a computation over the coordinates given.
#[track_caller]
pub(crate) fn assert_points_len(method: &str, naxes: usize, points: &[&[f64]]) {
    for p in points {
        assert!(
            p.len() >= naxes,
            "{method}: a point has {} coordinates but the Frame has {naxes} axes",
            p.len()
        );
    }
}

/// Cartesian distance between two points, the Frame-class algorithm
/// (frame.c Distance): AST__BAD if any coordinate is bad, otherwise the
/// square root of the summed squared displacements.
pub(crate) fn cartesian_distance(a: &[f64], b: &[f64]) -> f64 {
    use crate::constants::BAD_VALUE;
    use crate::mappings::helpers::is_bad;

    let mut dist = 0.0;
    for (&p1, &p2) in a.iter().zip(b) {
        if is_bad(p1) || is_bad(p2) {
            return BAD_VALUE;
        }
        let delta = p1 - p2;
        dist += delta * delta;
    }
    crate::math::sqrt(dist)
}

/// The point `dist` along the straight line from `from` towards `to`, the
/// Frame-class algorithm (frame.c Offset): every coordinate is AST__BAD if
/// any input coordinate is bad, or if the points coincide and `dist` is not
/// zero. C applies no check to a bad `dist`; here it gives a bad point too.
pub(crate) fn cartesian_offset(from: &[f64], to: &[f64], dist: f64) -> Vec<f64> {
    use crate::constants::BAD_VALUE;
    use crate::mappings::helpers::is_bad;

    let total = cartesian_distance(from, to);
    if is_bad(dist) || is_bad(total) || (total == 0.0 && dist != 0.0) {
        return vec![BAD_VALUE; from.len()];
    }
    let fract = if total == 0.0 { 0.0 } else { dist / total };
    from.iter()
        .zip(to)
        .map(|(&p1, &p2)| p1 + fract * (p2 - p1))
        .collect()
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
        let anga = crate::math::atan2(ab[0], ab[1]);
        let angc = crate::math::atan2(cb[0], cb[1]);
        crate::pal::drange(angc - anga)
    } else {
        let sp: f64 = (0..naxes).map(|i| ab[i] * cb[i]).sum();
        crate::math::acos((sp / crate::math::sqrt(sla * slc)).clamp(-1.0, 1.0))
    }
}
