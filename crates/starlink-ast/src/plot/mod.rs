// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Plots: FrameSets that draw annotated coordinate grids and other graphics.
//!
//! A [`Plot`] joins a graphics coordinate system, in which a graphics
//! backend ([`Grf`](crate::grf::Grf)) draws, to the coordinate systems of a
//! Frame or FrameSet. Its base Frame is a 2-D Frame with the Domain
//! `GRAPHICS`, mapped onto a box in the base Frame of the Frame or FrameSet
//! it was built from, and its current Frame is that FrameSet's current
//! Frame, so a Plot transforms graphics coordinates to physical ones as any
//! FrameSet does.
//!
//! A Plot is a FrameSet in every respect: it can be used wherever a Frame or
//! Mapping can, Frames can be added to and removed from it, and it is saved
//! and read with a FrameSet's description followed by its own attributes.
//! Two things differ: transforming points from graphics coordinates gives
//! values normalised in the current Frame, and a Plot can carry a clipping
//! region, outside which transformed points are bad.
// C: plot.c. Classification of astInitPlotVtab (CLAUDE.md, "Before porting a
// class"):
//
// - Architecture, ported later at C's granularity as Plot's drawing seam
//   (stages 4 and 6 of 2026-10-30-plot-design.md): Border, BoundingBox,
//   Clip, Curve, GenCurve, Grid, GridLine, Mark, PolyCurve, RegionOutline,
//   SetTickValues, Text, and the helpers Plot3D reaches through the vtab:
//   CopyPlotDefaults, Mirror, GetDrawnTicks, CvBrk, BBuf, EBuf.
// - Architecture, ported here: the Mapping Transform override (Trans:
//   normalisation then clipping) and the FrameSet RemoveFrame override
//   (keeping the clipping Frame's index).
// - C-ABI only (spec non-goal): GrfSet, GrfPush, GrfPop, GetGrfContext,
//   GrfWrapper, the Grf attribute, ManageLock.
// - Ceremony: the per-attribute Get/Set/Test/Clear functions and the string
//   Get/Set/Test/ClearAttrib dispatch, which become typed accessors.

mod attrs;
mod axes;
mod border;
mod curve;
mod draw;
mod elements;
mod forward;
mod grid;
mod labels;
mod sample;
mod serial;
mod text;
mod ticks;

pub use attrs::{Edge, Labelling, TextGapType};
pub use draw::Draw;
pub use elements::PlotElement;
pub use grid::UsedValues;

use crate::constants::BAD_VALUE;
use crate::coords::{PointSet, Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{BasicFrameBuilder, DynFrame, Frame, FrameRef, FrameSet, pick_axes};
use crate::mappings::{DynMapping, Mapping, PermMap, ViaSimplify, WinMap};

/// A clipping region: points whose position in a chosen Frame of the Plot
/// falls outside bounds on each of its axes are bad.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ClipRegion {
    /// The 1-based index of the clipping Frame. A loaded Plot may hold any
    /// index, which a transform then reports as invalid.
    pub(crate) frame: i32,
    /// The lower bound on each axis of the clipping Frame.
    pub(crate) lbnd: Vec<f64>,
    /// The upper bound on each axis of the clipping Frame.
    pub(crate) ubnd: Vec<f64>,
}

/// A FrameSet whose base Frame is a graphics coordinate system, through
/// which annotated coordinate grids and other graphics are drawn.
///
/// See the [module documentation](self).
///
/// # Examples
///
/// ```
/// use starlink_ast::frames::{Frame, SkyFrame};
/// use starlink_ast::mappings::Mapping;
/// use starlink_ast::plot::Plot;
///
/// // A sky coordinate system drawn in the graphics box 0.1 to 0.9, with
/// // the corners of the box at (0, -0.5) and (1, 0.5) in the sky.
/// let sky = SkyFrame::unset_system();
/// let plot = Plot::new(&sky, [0.1, 0.1, 0.9, 0.9], [0.0, -0.5, 1.0, 0.5]).unwrap();
/// assert_eq!(plot.frameset().nframe(), 2);
/// let (ra, dec) = plot.tran2(&[0.5], &[0.5], true).unwrap();
/// assert!((ra[0] - 0.5).abs() < 1e-7 && dec[0].abs() < 1e-7);
/// ```
#[derive(Debug, Clone)]
pub struct Plot {
    frameset: FrameSet,
    /// The graphics box, lower and upper bounds on x then y.
    xlo: f64,
    ylo: f64,
    xhi: f64,
    yhi: f64,
    /// Whether the graphics box was given with each axis reversed.
    xrev: bool,
    yrev: bool,
    /// The plot box in the base Frame of the supplied FrameSet: x1, y1,
    /// x2, y2.
    bbox: [f64; 4],
    clip: Option<ClipRegion>,
    /// Major and minor tick values given for each axis, to be drawn in
    /// place of those a grid would choose.
    // C: majtickval and mintickval, set by the protected astSetTickValues,
    // which Plot3D calls.
    tick_values: [TickValues; 3],
    /// Plot's own attributes.
    pub(crate) attrs: attrs::PlotAttrs,
    /// What C keeps in globals while drawing.
    pub(crate) draw: draw::DrawState,
    /// The values the last grid used for attributes left unset.
    pub(crate) used: grid::UsedState,
}

/// Tick values given for one axis of a Plot.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TickValues {
    pub(crate) major: Vec<f64>,
    pub(crate) minor: Vec<f64>,
}

/// The FrameSet `fs` with a 2-D base Frame (or current Frame, if not
/// `base`): unchanged if that Frame is 2-D or smaller, else with a new
/// Frame of its first two axes, joined to it by a PermMap that gives the
/// other axes the value 0, as the new base (or current) Frame.
// C: Fset2D (plot.c:13798).
fn fset_2d(mut fs: FrameSet, base: bool) -> AstResult<FrameSet> {
    let which = if base {
        FrameRef::Base
    } else {
        FrameRef::Current
    };
    let frm = fs.frame(which)?;
    let nax = frm.naxes();
    if nax <= 2 {
        return Ok(fs);
    }
    let (newfrm, _) = pick_axes(frm, &[1, 2])?;
    let mut inperm = vec![1_i64, 2];
    inperm.extend(std::iter::repeat_n(-1, nax - 2));
    let map = PermMap::new(&inperm, &[1, 2], &[0.0])?;
    let ic = fs.current_index();
    fs.add_frame(which, Box::new(map), newfrm)?;
    if base {
        fs.set_base(FrameRef::Current)?;
        fs.set_current(FrameRef::Index(ic))?;
    }
    Ok(fs)
}

fn bad_box(text: &str) -> AstError {
    AstError::new(ErrorCode::Badbx, format!("astInitPlot(Plot): {text}"))
}

/// Whether a value is bad as C's `astISBAD` tests it: the bad value or not
/// finite.
fn is_bad(v: f64) -> bool {
    v == BAD_VALUE || !v.is_finite()
}

impl Plot {
    /// A Plot of `frame`, which may be any Frame, a FrameSet, a Region or
    /// another Plot.
    ///
    /// `graphbox` is the plotting area in graphics coordinates, as `[x1,
    /// y1, x2, y2]`, the corners of a box. `basebox` gives the coordinates
    /// in the base Frame of `frame` (the Frame itself, for a Frame) that
    /// those two corners have, so that the graphics box maps linearly onto
    /// that box. A graphics box given right to left or top to bottom, as on
    /// a screen whose y axis runs down, reverses the corresponding graphics
    /// axis.
    ///
    /// A base Frame with more than two axes is reduced to its first two,
    /// the others being taken as zero. A Plot built from a Plot keeps that
    /// Plot's Frames, the old graphics Frame among them, and clears the
    /// Domain of Frames whose Domain was `GRAPHICS`.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Naxin` when the base Frame has fewer than two axes.
    /// - `ErrorCode::Badbx` when the graphics box has zero width or height,
    ///   or a corner of either box is bad or not finite.
    #[doc(alias = "astPlot")]
    // C: astInitPlot_ (plot.c:30676).
    pub fn new(frame: &dyn Frame, graphbox: [f32; 4], basebox: [f64; 4]) -> AstResult<Self> {
        let (fset, mess) = if let Some(plot) = frame.as_any().downcast_ref::<Plot>() {
            let mut fs = FrameSet::new(Box::new(plot.frameset.clone()));
            // C walks astGetFrame( fset, i ) for i from 0, which is the
            // base Frame, to one less than the number of Frames, so the
            // last Frame is never examined (c-library-quirks.md).
            let base = fs.base_index();
            let indices = std::iter::once(base).chain(1..fs.nframe());
            for i in indices {
                let f = fs.frame_mut(FrameRef::Index(i))?;
                if f.domain() == "GRAPHICS" {
                    f.clear_domain()?;
                }
            }
            (fs, "base Frame of the supplied Plot")
        } else if let Some(fs) = frame.as_frameset() {
            (
                fset_2d(fs.clone(), true)?,
                "base Frame of the supplied FrameSet",
            )
        } else {
            (
                fset_2d(FrameSet::new(frame.snapshot()), true)?,
                "supplied Frame",
            )
        };
        Self::from_frameset(fset, mess, graphbox, basebox)
    }

    /// A Plot of a default 2-D Frame, as [`new`](Plot::new) with no Frame.
    ///
    /// # Errors
    ///
    /// As for [`new`](Plot::new).
    // C: astInitPlot_ with a NULL Frame.
    pub fn default_frame(graphbox: [f32; 4], basebox: [f64; 4]) -> AstResult<Self> {
        let frame = BasicFrameBuilder::new(2).build()?;
        Self::from_frameset(
            FrameSet::new(Box::new(frame)),
            "default 2-d Frame",
            graphbox,
            basebox,
        )
    }

    fn from_frameset(
        mut fset: FrameSet,
        mess: &str,
        graphbox: [f32; 4],
        basebox: [f64; 4],
    ) -> AstResult<Self> {
        let naxes = fset.base().naxes();
        if naxes != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "astInitPlot(Plot): Number of axes ({naxes}) in the {mess} is invalid - \
                     this number should be 2."
                ),
            ));
        }
        let g = graphbox;
        if g[2] == g[0] || g[3] == g[1] {
            return Err(bad_box(
                "The plotting area has zero size in the graphics world coordinate system.",
            ));
        }
        if g.iter().any(|&v| is_bad(f64::from(v))) {
            return Err(bad_box(
                "The plotting area has undefined limits in the graphics world coordinate system.",
            ));
        }
        if is_bad(basebox[2]) || is_bad(basebox[0]) {
            return Err(bad_box(
                "The limits of the horizontal axis of the Plot are undefined or bad.",
            ));
        } else if is_bad(basebox[3]) || is_bad(basebox[1]) {
            return Err(bad_box(
                "The limits of the vertical axis of the Plot are undefined or bad.",
            ));
        }

        let mut graphics = BasicFrameBuilder::new(2).build()?;
        graphics.set_domain("GRAPHICS");
        graphics.set_title("Graphical Coordinates");
        let gbox = g.map(f64::from);
        let (xlo, xhi, xrev) = if g[0] <= g[2] {
            (gbox[0], gbox[2], false)
        } else {
            graphics.set_direction(1, false)?;
            (gbox[2], gbox[0], true)
        };
        let (ylo, yhi, yrev) = if g[1] <= g[3] {
            (gbox[1], gbox[3], false)
        } else {
            graphics.set_direction(2, false)?;
            (gbox[3], gbox[1], true)
        };

        let map = WinMap::from_corners(&gbox[0..2], &gbox[2..4], &basebox[0..2], &basebox[2..4])?;
        let ci = fset.current_index();
        fset.set_current(FrameRef::Index(fset.base_index()))?;
        let mut frameset = FrameSet::new(Box::new(graphics));
        frameset.add_frameset(FrameRef::Index(1), Box::new(map), fset)?;
        frameset.set_current(FrameRef::Index(ci + 1))?;

        Ok(Self {
            frameset,
            xlo,
            ylo,
            xhi,
            yhi,
            xrev,
            yrev,
            bbox: basebox,
            clip: None,
            tick_values: Default::default(),
            draw: Default::default(),
            used: Default::default(),
            attrs: attrs::PlotAttrs::default(),
        })
    }

    /// The FrameSet this Plot is.
    pub fn frameset(&self) -> &FrameSet {
        &self.frameset
    }

    /// The FrameSet this Plot is, mutably.
    ///
    /// Changes made through it bypass what the Plot does itself: removing
    /// a Frame this way does not keep the clipping region's Frame, which
    /// [`remove_frame`](Plot::remove_frame) does.
    pub fn frameset_mut(&mut self) -> &mut FrameSet {
        &mut self.frameset
    }

    /// The graphics box, as `[xlo, ylo, xhi, yhi]` with each lower bound
    /// below its upper bound, whichever way round the box was given.
    pub fn graphics_box(&self) -> [f64; 4] {
        [self.xlo, self.ylo, self.xhi, self.yhi]
    }

    /// Whether the graphics box was given with its x and y axes reversed.
    pub fn reversed(&self) -> [bool; 2] {
        [self.xrev, self.yrev]
    }

    /// The box in the base Frame of the Frame or FrameSet the Plot was
    /// built from that the graphics box maps onto, as given: `[x1, y1, x2,
    /// y2]`.
    pub fn plot_box(&self) -> [f64; 4] {
        self.bbox
    }

    /// Add a Frame, as [`FrameSet::add_frame`] does.
    ///
    /// # Errors
    ///
    /// As for [`FrameSet::add_frame`].
    // C's Plot inherits AddFrame from FrameSet; a new Frame goes at the end,
    // so the clipping region's Frame index still holds.
    pub fn add_frame(
        &mut self,
        anchor: FrameRef,
        map: DynMapping,
        new_frame: DynFrame,
    ) -> AstResult<()> {
        self.frameset.add_frame(anchor, map, new_frame)
    }

    /// Select the base Frame, as [`FrameSet::set_base`] does.
    ///
    /// # Errors
    ///
    /// As for [`FrameSet::set_base`].
    pub fn set_base(&mut self, which: FrameRef) -> AstResult<()> {
        self.frameset.set_base(which)
    }

    /// Select the current Frame, as [`FrameSet::set_current`] does.
    ///
    /// # Errors
    ///
    /// As for [`FrameSet::set_current`].
    pub fn set_current(&mut self, which: FrameRef) -> AstResult<()> {
        self.frameset.set_current(which)
    }

    /// Remove a Frame, as [`FrameSet::remove_frame`] does.
    ///
    /// If the Plot has a clipping region, removing its Frame removes the
    /// clipping region, and removing a Frame before it keeps it on the same
    /// Frame.
    ///
    /// # Errors
    ///
    /// As for [`FrameSet::remove_frame`].
    #[doc(alias = "astRemoveFrame")]
    // C: RemoveFrame (plot.c:24637).
    pub fn remove_frame(&mut self, which: FrameRef) -> AstResult<()> {
        let ifrm = match which {
            FrameRef::Index(i) => i,
            FrameRef::Base => self.frameset.base_index(),
            FrameRef::Current => self.frameset.current_index(),
            _ => self.frameset.current_index(),
        };
        self.frameset.remove_frame(which)?;
        let ifrm = ifrm as i32;
        if let Some(clip) = &mut self.clip {
            if ifrm < clip.frame {
                clip.frame -= 1;
            } else if ifrm == clip.frame {
                self.clip = None;
            }
        }
        Ok(())
    }

    /// This Plot with `frameset` in place of its own.
    fn with_frameset(&self, frameset: FrameSet) -> Plot {
        Plot {
            frameset,
            ..self.clone()
        }
    }

    /// Transform points through the Plot: through its FrameSet, then, going
    /// forward, normalising them in the current Frame, then setting points
    /// outside the clipping region bad.
    // C: Trans (plot.c:28181), with `norm` set, as Transform calls it.
    fn trans(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        self.trans_with(None, pts, forward, true)
    }

    /// Transform points through the Plot's FrameSet, normalising forward
    /// results in `frm` (the current Frame if `None`) when `norm` is set,
    /// then setting points outside the clipping region bad.
    // C: Trans (plot.c:28181). C's `mapping` argument is always the Plot's
    // base to current Mapping where the port calls it.
    pub(crate) fn trans_with(
        &self,
        frm: Option<&dyn Frame>,
        pts: &PointSet,
        forward: bool,
        norm: bool,
    ) -> AstResult<PointSet> {
        let mut result = Mapping::transform(&self.frameset, pts, forward)?;
        let npoint = result.npoint();
        if forward && norm {
            let cfr = frm.unwrap_or_else(|| self.frameset.current());
            for i in 0..npoint {
                let mut work = result.point(i).to_vec();
                cfr.norm(&mut work)?;
                result.set_point(i, &work);
            }
        }
        let Some(clip) = &self.clip else {
            return Ok(result);
        };
        // C: astValidateFrameIndex, which takes AST__BASE (0) and
        // AST__CURRENT (-1) as the Frames they name now.
        let nframe = self.frameset.nframe();
        let clip_frame = match clip.frame {
            0 => self.frameset.base_index() as i32,
            -1 => self.frameset.current_index() as i32,
            f => f,
        };
        if clip_frame < 1 || clip_frame as usize > nframe {
            return Err(AstError::new(
                ErrorCode::Frmin,
                format!(
                    "astTransform(Plot): Invalid Frame index ({}) given. This value should be \
                     in the range 1 to {nframe} (or AST__CURRENT or AST__BASE).",
                    clip.frame
                ),
            ));
        }
        let clip_frame = clip_frame as usize;
        let fr = self.frameset.frame(FrameRef::Index(clip_frame))?;
        let naxes = fr.naxes();
        if naxes != clip.lbnd.len() {
            return Err(AstError::new(
                ErrorCode::Clpax,
                format!(
                    "astTransform(Plot): The supplied Plot specifies clipping in {} dimensions, \
                     but the clipping Frame ('{}') has {naxes} axes.",
                    clip.lbnd.len(),
                    fr.title()?
                ),
            ));
        }
        // The positions in the clipping Frame. C uses the input or result
        // PointSet itself when the clipping Frame is the base or current
        // Frame, and so normalises the caller's input in place; the port
        // normalises a copy of the input (c-library-quirks.md). Going
        // backwards with the base Frame clipping, it normalises the result
        // itself, which the caller receives normalised, as from C.
        let (base, current) = (self.frameset.base_index(), self.frameset.current_index());
        let mut clip_norm = true;
        let mut clip_pts = if clip_frame == base {
            if forward {
                pts.clone()
            } else {
                for i in 0..npoint {
                    let mut work = result.point(i).to_vec();
                    fr.norm(&mut work)?;
                    result.set_point(i, &work);
                }
                clip_norm = false;
                result.clone()
            }
        } else if clip_frame == current {
            if forward {
                if norm {
                    clip_norm = false;
                }
                result.clone()
            } else {
                pts.clone()
            }
        } else {
            let from = if forward {
                FrameRef::Current
            } else {
                FrameRef::Base
            };
            let map = self.frameset.mapping(from, FrameRef::Index(clip_frame))?;
            map.transform(&result, true)?
        };
        if clip_norm {
            for i in 0..npoint {
                let mut work = clip_pts.point(i).to_vec();
                fr.norm(&mut work)?;
                clip_pts.set_point(i, &work);
            }
        }
        let clip_or = self.clip_op();
        // C sets the first `naxes` output coordinates bad, which are all of
        // them only when the clipping Frame has as many axes as the current
        // Frame (c-library-quirks.md).
        let nbad = naxes.min(result.ncoord());
        for j in 0..npoint {
            let mut clipit = clip_or;
            for i in 0..naxes {
                let axval = clip_pts.axis(i)[j];
                if axval == BAD_VALUE {
                    clipit = true;
                    break;
                }
                let (lbnd, ubnd) = (clip.lbnd[i], clip.ubnd[i]);
                let axin = if lbnd <= ubnd {
                    axval >= lbnd && axval <= ubnd
                } else {
                    axval < ubnd || axval > lbnd
                };
                if axin && clip_or {
                    clipit = false;
                    break;
                } else if !axin && !clip_or {
                    clipit = true;
                    break;
                }
            }
            if clipit {
                for i in 0..nbad {
                    result.axis_mut(i)[j] = BAD_VALUE;
                }
            }
        }
        Ok(result)
    }

    /// The Frame methods the Plot implements itself, which the generated
    /// forwarding in `forward.rs` leaves out.
    fn own_snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }
}

// The Frame and Mapping methods that differ from a FrameSet's. The rest are
// generated into forward.rs.
impl Plot {
    pub(crate) fn frame_equal(&self, other: &dyn Frame) -> bool {
        other
            .as_any()
            .downcast_ref::<Plot>()
            .is_some_and(|o| Frame::equal(&self.frameset, &o.frameset))
    }

    pub(crate) fn mapping_equal(&self, other: &dyn Mapping) -> bool {
        other
            .as_any()
            .downcast_ref::<Plot>()
            .is_some_and(|o| Mapping::equal(&self.frameset, &o.frameset))
    }

    /// A FrameSet result from the inner FrameSet as a Plot, as C's FrameSet
    /// methods, which copy `this`, give a Plot.
    fn rewrap_frame(&self, frame: DynFrame) -> DynFrame {
        match frame.as_any().downcast_ref::<FrameSet>() {
            Some(fs) => Box::new(self.with_frameset(fs.clone())),
            None => frame,
        }
    }

    fn rewrap_mapping(&self, map: DynMapping) -> DynMapping {
        match map.as_any().downcast_ref::<FrameSet>() {
            Some(fs) => Box::new(self.with_frameset(fs.clone())),
            None => map,
        }
    }

    pub(crate) fn plot_snapshot(&self) -> DynFrame {
        self.own_snapshot()
    }

    pub(crate) fn plot_frame_without_regions(&self) -> Option<DynFrame> {
        Frame::frame_without_regions(&self.frameset).map(|f| self.rewrap_frame(f))
    }

    pub(crate) fn plot_simplify_frame(&self) -> AstResult<Option<DynFrame>> {
        Ok(Frame::simplify_frame(&self.frameset)?.map(|f| self.rewrap_frame(f)))
    }

    pub(crate) fn plot_transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        self.trans(pts, forward)
    }

    pub(crate) fn plot_transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        let mut input = PointSet::zeros(pts.npoint(), pts.ncoord());
        for j in 0..pts.ncoord() {
            input.axis_mut(j).copy_from_slice(pts.axis(j));
        }
        let result = self.trans(&input, forward)?;
        out.copy_from(&result.as_points());
        Ok(())
    }

    pub(crate) fn plot_simplified(&self, gate: ViaSimplify) -> AstResult<(DynMapping, bool)> {
        let (map, changed) = Mapping::simplified(&self.frameset, gate)?;
        Ok((self.rewrap_mapping(map), changed))
    }

    pub(crate) fn plot_mapping_without_regions(&self) -> Option<DynMapping> {
        Mapping::mapping_without_regions(&self.frameset).map(|m| self.rewrap_mapping(m))
    }
}
