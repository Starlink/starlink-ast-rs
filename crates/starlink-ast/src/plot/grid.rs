// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Annotated coordinate grids: where the ticks and grid lines go, whether
//! the axes are labelled round the edges of the plotting area or inside
//! it, and the values the grid chooses for the attributes left unset.
// C: plot.c Grid (:18258), DrawGrid (:9416), AddCdt (:4422), EdgeLabels
// (:10897), swapEdges (:25770) and Labelat (:20613), and the GetUsed*
// functions (plot.c:1225) behind the string getter of the attributes with
// dynamic defaults.

use super::Plot;
use super::attrs::{Edge, Labelling};
use super::curve::CurveData;
use super::elements::PlotElement;
use super::ticks::TickInfo;
use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::grf::{Grf, GrfPrimitive};
use crate::mappings::Mapping;
use crate::math::sqrt;

/// The most breaks a grid line made of several sections may have.
// C: AST__MXBRK (plot.h).
const MXBRK: usize = 100;

/// The values a grid uses for the attributes whose defaults depend on
/// what is drawn: for each attribute, the value set, or else the one the
/// grid chose.
///
/// Per-axis values are indexed by the 0-based axis.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct UsedValues {
    /// The gap between major ticks of an axis with linear ticks.
    pub gap: [f64; 2],
    /// The ratio between major ticks of an axis with logarithmic ticks.
    pub log_gap: [f64; 2],
    /// The axis value at a major tick.
    pub centre: [f64; 2],
    /// The edge an axis's labels are on.
    pub edge: [Edge; 2],
    /// The value of the other axis along which an axis is labelled inside
    /// the plotting area, or `BAD_VALUE` when labelled round the edges.
    pub label_at: [f64; 2],
    /// Whether an axis's descriptive label includes its units.
    pub label_units: [bool; 2],
    /// Whether an axis's major ticks are spaced logarithmically.
    pub log_ticks: [bool; 2],
    /// Whether an axis's numerical labels are powers of ten.
    pub log_label: [bool; 2],
    /// The number of minor tick divisions between major ticks.
    pub min_tick: [i32; 2],
    /// Whether an axis's descriptive label is drawn.
    pub text_lab: [bool; 2],
    /// The length of an axis's major ticks.
    pub maj_tick_len: [f64; 2],
    /// Whether grid lines are drawn across the plotting area.
    pub grid: bool,
    /// Whether the axes are labelled round the edges or inside.
    pub labelling: Labelling,
    /// Whether a border is drawn.
    pub border: bool,
}

/// The values the last grid on a Plot used, held as C holds them: an
/// attribute's unset state until a grid records it.
// C: the Plot's ugap, uloggap, ucentre, uedge, ulblat, ulbunit, ulgtk,
// ulglb, umintk, utxtlb, umjtkln, ugrid, ulbling and uborder, which
// astInitPlot starts at the attributes' unset values (-1 or AST__BAD), and
// a grid sets with SetUsed*. A grid records Gap or LogGap, not both, so the
// other keeps its earlier value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UsedState {
    gap: [f64; 2],
    log_gap: [f64; 2],
    centre: [f64; 2],
    edge: [i32; 2],
    label_at: [f64; 2],
    label_units: [i32; 2],
    log_ticks: [i32; 2],
    log_label: [i32; 2],
    min_tick: [i32; 2],
    text_lab: [i32; 2],
    maj_tick_len: [f64; 2],
    grid: i32,
    labelling: i32,
    border: i32,
}

impl Default for UsedState {
    fn default() -> Self {
        Self {
            gap: [BAD_VALUE; 2],
            log_gap: [BAD_VALUE; 2],
            centre: [BAD_VALUE; 2],
            edge: [-1; 2],
            label_at: [BAD_VALUE; 2],
            label_units: [-1; 2],
            log_ticks: [-1; 2],
            log_label: [-1; 2],
            min_tick: [-1; 2],
            text_lab: [-1; 2],
            maj_tick_len: [BAD_VALUE; 2],
            grid: -1,
            labelling: -1,
            border: -1,
        }
    }
}

/// An edge from C's integer code, taken modulo 4 as C does.
fn edge_of(code: i32) -> Edge {
    match code.rem_euclid(4) {
        0 => Edge::Left,
        1 => Edge::Top,
        2 => Edge::Right,
        _ => Edge::Bottom,
    }
}

/// C's integer code of an edge.
pub(crate) fn edge_code(edge: Edge) -> i32 {
    match edge {
        Edge::Left => 0,
        Edge::Top => 1,
        Edge::Right => 2,
        Edge::Bottom => 3,
    }
}

/// One numerical label to draw.
// C: LabelList, without the fields PlotLabels adds.
#[derive(Debug, Clone)]
pub(crate) struct EdgeLabel {
    pub(crate) index: f64,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) text: String,
    pub(crate) just: [u8; 2],
    pub(crate) upx: f32,
    pub(crate) upy: f32,
    pub(crate) val: f64,
}

/// Join the breaks of a later section of a grid line to the earlier ones.
// C: AddCdt (plot.c:4422).
fn add_cdt(cdt1: &mut CurveData, cdt2: &CurveData, method: &str) -> AstResult<()> {
    if cdt1.xbrk.len() + cdt2.xbrk.len() > MXBRK {
        return Err(AstError::new(
            ErrorCode::Cvbrk,
            format!("{method}(Plot): Number of breaks in curve exceeds {MXBRK}."),
        ));
    }
    cdt1.xbrk.extend_from_slice(&cdt2.xbrk);
    cdt1.ybrk.extend_from_slice(&cdt2.ybrk);
    cdt1.vxbrk.extend_from_slice(&cdt2.vxbrk);
    cdt1.vybrk.extend_from_slice(&cdt2.vybrk);
    cdt1.length += cdt2.length;
    if !cdt2.out {
        cdt1.out = false;
    }
    Ok(())
}

impl Plot {
    /// Trace (and draw if `drawgrid`) the grid line through each major tick
    /// of each axis, returning where each breaks.
    // C: DrawGrid (plot.c:9416).
    fn draw_grid_lines(
        &mut self,
        grf: &mut dyn Grf,
        grid: &[TickInfo; 2],
        drawgrid: bool,
    ) -> AstResult<[Vec<CurveData>; 2]> {
        let mut cdata: [Vec<CurveData>; 2] = [Vec::new(), Vec::new()];
        for i in 0..2 {
            let info = &grid[i];
            let mut total_length = 0.0f64;
            for &tick in &info.ticks {
                let mut start = [0.0; 2];
                start[i] = tick;
                start[1 - i] = info.start[0];
                let mut cdt =
                    self.ax_plot(grf, 1 - i, &start, info.length[0], drawgrid, "astGrid")?;
                for k in 1..info.nsect {
                    start[1 - i] = info.start[k];
                    let tcdt =
                        self.ax_plot(grf, 1 - i, &start, info.length[k], drawgrid, "astGrid")?;
                    add_cdt(&mut cdt, &tcdt, "astGrid")?;
                }
                total_length += f64::from(cdt.length);
                cdata[i].push(cdt);
            }
            if total_length == 0.0 {
                return Err(AstError::new(
                    ErrorCode::Inter,
                    format!(
                        "astGrid(Plot): No grid curves can be drawn for axis {}.",
                        i + 1
                    ),
                ));
            }
        }
        Ok(cdata)
    }

    /// Whether the axes can be labelled round the edges of the plotting
    /// area: whether enough grid lines cross the edges chosen (or `force`).
    /// With `ink` the labels are drawn.
    // C: EdgeLabels (plot.c:10897).
    pub(crate) fn edge_labels(
        &mut self,
        grf: &mut dyn Grf,
        ink: bool,
        grid: &[TickInfo; 2],
        cdata: &[Vec<CurveData>; 2],
        force: bool,
    ) -> AstResult<bool> {
        let mindim = {
            let (a, b) = (self.xhi - self.xlo, self.yhi - self.ylo);
            if a < b { a } else { b }
        };
        let tol = 0.005 * mindim;
        let mut llist: [Vec<EdgeLabel>; 2] = [Vec::new(), Vec::new()];
        let mut medge = [0usize; 2];
        for axis in 0..2 {
            let a1 = axis + 1;
            let upright = if self.test_label_up(a1)? {
                self.label_up(a1)?
            } else {
                true
            };
            let txtgap = self.num_lab_gap(a1)? * mindim;
            let edge = edge_code(self.edge(a1)?);
            let (just, edgeval, mut xref, mut yref, edgeax): ([u8; 2], f64, f32, f32, usize);
            xref = 0.0;
            yref = 0.0;
            let pick = |pos: &'static [u8; 2], neg: &'static [u8; 2]| {
                if txtgap > 0.0 {
                    *pos
                } else if txtgap < 0.0 {
                    *neg
                } else {
                    *b"CC"
                }
            };
            match edge {
                0 => {
                    just = if !upright { *b"BC" } else { pick(b"CR", b"CL") };
                    if !self.xrev {
                        edgeval = self.xlo;
                        xref = (edgeval - txtgap) as f32;
                    } else {
                        edgeval = self.xhi;
                        xref = (edgeval + txtgap) as f32;
                    }
                    edgeax = 1;
                }
                1 => {
                    just = pick(b"BC", b"TC");
                    if !self.yrev {
                        edgeval = self.yhi;
                        yref = (edgeval + txtgap) as f32;
                    } else {
                        edgeval = self.ylo;
                        yref = (edgeval - txtgap) as f32;
                    }
                    edgeax = 0;
                }
                2 => {
                    just = if !upright { *b"BC" } else { pick(b"CL", b"CR") };
                    if !self.xrev {
                        edgeval = self.xhi;
                        xref = (edgeval + txtgap) as f32;
                    } else {
                        edgeval = self.xlo;
                        xref = (edgeval - txtgap) as f32;
                    }
                    edgeax = 1;
                }
                _ => {
                    just = pick(b"TC", b"BC");
                    if !self.yrev {
                        edgeval = self.ylo;
                        yref = (edgeval - txtgap) as f32;
                    } else {
                        edgeval = self.yhi;
                        yref = (edgeval + txtgap) as f32;
                    }
                    edgeax = 0;
                }
            }
            let info = &grid[axis];
            let mut maxlab = 0;
            for (tick, cdt) in cdata[axis].iter().enumerate().take(info.nmajor()) {
                let mut ok = false;
                for brk in 0..cdt.xbrk.len() {
                    let (xb, yb) = (cdt.xbrk[brk], cdt.ybrk[brk]);
                    let (vxb, vyb) = (cdt.vxbrk[brk], cdt.vybrk[brk]);
                    let near = (edgeax == 0
                        && (f64::from(yb) - edgeval).abs() < tol
                        && f64::from(vyb).abs() > 0.09)
                        || (edgeax == 1
                            && (f64::from(xb) - edgeval).abs() < tol
                            && f64::from(vxb).abs() > 0.09);
                    let Some(text) = info.labels.get(tick) else {
                        continue;
                    };
                    if near {
                        let (index, x, y) = if edgeax == 0 {
                            (f64::from(xb), f64::from(xb), f64::from(yref))
                        } else {
                            (f64::from(yb), f64::from(xref), f64::from(yb))
                        };
                        let (upx, upy) = if edge == 1 || edge == 3 || upright {
                            (0.0, 1.0)
                        } else if edge == 0 {
                            (-1.0, 0.0)
                        } else {
                            (1.0, 0.0)
                        };
                        let val = info.ticks[tick];
                        let labfound = llist[axis]
                            .iter()
                            .any(|l| (val - l.val).abs() < 0.2 * info.gap);
                        llist[axis].push(EdgeLabel {
                            index,
                            x,
                            y,
                            text: text.clone(),
                            just,
                            upx,
                            upy,
                            val,
                        });
                        if !labfound {
                            ok = true;
                        }
                    }
                }
                if ok {
                    maxlab += 1;
                }
            }
            medge[axis] = maxlab;
        }
        let need = |lt: bool| if lt { 1 } else { 2 };
        let edgelabs =
            (medge[0] > need(self.log_ticks(1)?) && medge[1] > need(self.log_ticks(2)?)) || force;
        if edgelabs && ink {
            let esc = self.escape();
            let mut boxes = super::labels::LabelBoxes::new();
            let numlab_id = [PlotElement::NumLab1, PlotElement::NumLab2];
            for (axis, labels) in llist.into_iter().enumerate() {
                self.grf_attrs(grf, numlab_id[axis], true, GrfPrimitive::Text)?;
                let fmt = match &grid[axis].fmt {
                    Some(f) => f.clone(),
                    None => self.frameset.current().format_str(axis + 1)?,
                };
                self.plot_labels(grf, esc, axis, labels, &fmt, &mut boxes)?;
                self.grf_attrs(grf, numlab_id[axis], false, GrfPrimitive::Text)?;
            }
        }
        Ok(edgelabs)
    }

    /// Whether the Edge attributes should be swapped to put the textual
    /// labels on suitable edges when the axes are labelled inside.
    // C: swapEdges (plot.c:25770).
    fn swap_edges(&self, grid: &[TickInfo; 2], cdata: &[Vec<CurveData>; 2]) -> AstResult<bool> {
        let (mut xmax, mut xmin, mut ymax, mut ymin) =
            (-1.0e10f32, 1.0e10f32, -1.0e10f32, 1.0e10f32);
        for cdt in cdata[0].iter().take(grid[0].nmajor()) {
            if let (Some(&x), Some(&y)) = (cdt.xbrk.first(), cdt.ybrk.first()) {
                xmax = super::draw::fmax(xmax, x);
                xmin = super::draw::fmin(xmin, x);
                ymax = super::draw::fmax(ymax, y);
                ymin = super::draw::fmin(ymin, y);
            }
        }
        let oldedge = edge_code(self.edge(1)?);
        Ok(if xmax - xmin > ymax - ymin {
            oldedge == 0 || oldedge == 2
        } else {
            oldedge == 1 || oldedge == 3
        })
    }

    /// The other-axis values along which to label each axis inside the
    /// plotting area, and whether doing so labels more ticks than labelling
    /// round the edges would.
    // C: Labelat (plot.c:20613). C reads the second tick of the other axis
    // whether or not it has one; with one tick the port takes the gap to
    // be zero.
    fn labelat(
        &self,
        grid: &[TickInfo; 2],
        cdata: &[Vec<CurveData>; 2],
        labelat: &mut [f64; 2],
    ) -> AstResult<bool> {
        let mut result = true;
        let margin = 0.05 * (self.xhi - self.xlo);
        let (xlo, xhi) = (self.xlo + margin, self.xhi - margin);
        let margin = 0.05 * (self.yhi - self.ylo);
        let (ylo, yhi) = (self.ylo + margin, self.yhi - margin);
        for axis in 0..2 {
            let info = &grid[1 - axis];
            let n = grid[axis].nmajor();
            let lim = if info.ticks.len() > 1 {
                1.0e-6 * (info.ticks[1] - info.ticks[0]).abs()
            } else {
                0.0
            };
            let mut maxlen = -1.0f64;
            let mut minlen = f64::MAX;
            let mut zerolen = 0.0;
            labelat[axis] = BAD_VALUE;
            for (tick, &value) in info.ticks.iter().enumerate() {
                let mut phys = PointSet::zeros(n, 2);
                phys.axis_mut(axis).copy_from_slice(&grid[axis].ticks);
                phys.axis_mut(1 - axis).fill(value);
                let graph = self.trans_with(None, &phys, false, false)?;
                let mut nin = 0;
                for i in 0..n {
                    let (x, y) = (graph.axis(0)[i], graph.axis(1)[i]);
                    if x != BAD_VALUE && x > xlo && x < xhi && y != BAD_VALUE && y > ylo && y < yhi
                    {
                        nin += 1;
                    }
                }
                // C: sqrt( (float) nin )*cdt->length.
                let efflen = sqrt(f64::from(nin as f32)) * f64::from(cdata[1 - axis][tick].length);
                if efflen > maxlen {
                    maxlen = efflen;
                    labelat[axis] = value;
                }
                if efflen < minlen && efflen > 0.0 {
                    minlen = efflen;
                }
                if value.abs() <= lim {
                    zerolen = efflen;
                }
            }
            if zerolen > 0.4 * maxlen {
                labelat[axis] = 0.0;
            }
            if (maxlen - minlen) / (maxlen + minlen) < 1.0e-5 {
                result = false;
            }
            if self.test_label_at(axis + 1)? {
                labelat[axis] = self.label_at(axis + 1)?;
            }
        }
        Ok(result)
    }

    /// The grid, drawn if `ink`; the values it uses are recorded.
    // C: Grid (plot.c:18258).
    pub(crate) fn grid_impl(&mut self, grf: &mut dyn Grf, ink: bool) -> AstResult<()> {
        let naxes = Mapping::nin(self);
        if naxes != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "astGrid(Plot): Number of axes ({naxes}) in the base Frame of the supplied \
                     Plot is invalid - this number should be 2."
                ),
            ));
        }
        let _escapes = crate::escapes::set_escapes(true);
        let logticksset = [self.test_log_ticks(1)?, self.test_log_ticks(2)?];
        let loglabelset = [self.test_log_label(1)?, self.test_log_label(2)?];
        self.reset_grf();
        // C sets and clears attributes of the Plot and its current Frame as
        // it goes, and on an error skips the clearing; the port puts them
        // back, and forgets any polylines not yet drawn.
        let (attrs, frameset) = (self.attrs.clone(), self.frameset.clone());
        let result = match self.fset_2d_current()? {
            Some(mut plot) => {
                self.lend_draw_state(&mut plot);
                let result = plot.grid_2d(grf, ink, logticksset, loglabelset);
                self.return_draw_state(&mut plot);
                self.used = plot.used.clone();
                result
            }
            None => self.grid_2d(grf, ink, logticksset, loglabelset),
        };
        if result.is_err() {
            self.attrs = attrs;
            self.frameset = frameset;
            self.draw.poly_x.clear();
            self.draw.poly_y.clear();
            self.draw.polys.clear();
            self.draw.grfattrs_nesting = 0;
        }
        result
    }

    /// The rest of `grid_impl`, on a Plot whose current Frame has no more
    /// than two axes.
    fn grid_2d(
        &mut self,
        grf: &mut dyn Grf,
        ink: bool,
        logticksset: [bool; 2],
        loglabelset: [bool; 2],
    ) -> AstResult<()> {
        let naxes = Mapping::nout(self);
        if naxes != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "astGrid(Plot): Number of axes ({naxes}) in the current Frame of the \
                     supplied Plot is invalid - this number should be 2."
                ),
            ));
        }
        let clip = if self.test_clip() {
            let c = self.clip();
            self.set_clip(1)?;
            Some(c)
        } else {
            None
        };
        self.draw.box_lbnd = [f32::MAX, f32::MAX];
        self.draw.box_ubnd = [f32::MIN_POSITIVE, f32::MIN_POSITIVE];
        let mut cen = [self.tick_centre(1)?, self.tick_centre(2)?];
        let mut gap = [0.0; 2];
        let (grid, inval) = self.grid_lines(grf, &mut cen, &mut gap)?;
        let drawgrid = if self.test_grid() { self.grid() } else { inval };
        let cdata = self.draw_grid_lines(grf, &grid, ink && drawgrid)?;
        let mut clredge = false;
        let labelling = self.labelling();
        let mut edgeticks;
        if labelling == Labelling::Interior {
            edgeticks = false;
        } else {
            edgeticks = self.edge_labels(grf, false, &grid, &cdata, false)?;
            if !edgeticks && !self.test_edge(1)? && !self.test_edge(2)? {
                let (e0, e1) = (self.edge(1)?, self.edge(2)?);
                self.set_edge(1, e1)?;
                self.set_edge(2, e0)?;
                edgeticks = self.edge_labels(grf, false, &grid, &cdata, false)?;
                if edgeticks {
                    clredge = true;
                } else {
                    self.clear_edge(1)?;
                    self.clear_edge(2)?;
                }
            }
        }
        if !edgeticks && self.force_exterior() {
            edgeticks = self.edge_labels(grf, false, &grid, &cdata, true)?;
        }
        if !edgeticks
            && !self.test_edge(1)?
            && !self.test_edge(2)?
            && self.swap_edges(&grid, &cdata)?
        {
            let (e0, e1) = (self.edge(1)?, self.edge(2)?);
            self.set_edge(1, e1)?;
            self.set_edge(2, e0)?;
            clredge = true;
        }
        let mut labelat = [BAD_VALUE; 2];
        if !edgeticks {
            let useint = self.labelat(&grid, &cdata, &mut labelat)?;
            if !useint && labelling == Labelling::Exterior {
                labelat = [BAD_VALUE; 2];
                edgeticks = self.edge_labels(grf, false, &grid, &cdata, true)?;
            }
        }
        let border = if self.test_border() {
            self.border()
        } else {
            edgeticks
        };
        let dounits = [self.label_units(1)?, self.label_units(2)?];
        if ink {
            self.draw_ticks(grf, &grid, drawgrid, &labelat, &gap)?;
            self.draw_axis(grf, &grid, &labelat, &gap)?;
            if border {
                super::Draw::draw_border(self, grf)?;
            }
            self.labels(grf, &grid, &cdata, &gap, &labelat)?;
            self.text_labels(grf, edgeticks, dounits)?;
        }
        self.fpoly(grf)?;
        for axis in 0..2 {
            let a1 = axis + 1;
            let lt = self.log_ticks(a1)?;
            self.used.log_ticks[axis] = i32::from(lt);
            self.used.log_label[axis] = i32::from(self.log_label(a1)?);
            if lt {
                self.used.log_gap[axis] = gap[axis];
            } else {
                self.used.gap[axis] = gap[axis];
            }
            self.used.centre[axis] = cen[axis];
            self.used.edge[axis] = edge_code(self.edge(a1)?);
            self.used.label_at[axis] = labelat[axis];
            self.used.label_units[axis] = i32::from(dounits[axis]);
            let info = &grid[axis];
            // C: SetUsedMinTick stores at least 1. With explicit ticks C
            // divides by one less than the number of major ticks, which for
            // a single tick gives 0 on arm64 (and traps on x86); the port
            // takes 0 there.
            let min_tick = if !info.minticks.is_empty() {
                let (nminor, nmajor) = (info.nminor, info.nmajor() as i32);
                (nminor + nmajor).checked_div(nmajor - 1).unwrap_or(0)
            } else {
                info.nminor
            };
            self.used.min_tick[axis] = min_tick.max(1);
            self.used.text_lab[axis] = i32::from(if self.test_text_lab(a1)? {
                self.text_lab(a1)?
            } else {
                edgeticks
            });
            self.used.maj_tick_len[axis] = if self.test_maj_tick_len(a1)? {
                self.maj_tick_len(a1)?
            } else if drawgrid {
                0.0
            } else {
                self.maj_tick_len(a1)?
            };
        }
        self.used.grid = i32::from(drawgrid);
        self.used.labelling = i32::from(!edgeticks);
        self.used.border = i32::from(border);
        if clredge {
            self.clear_edge(1)?;
            self.clear_edge(2)?;
        }
        for axis in 0..2 {
            if !logticksset[axis] {
                self.clear_log_ticks(axis + 1)?;
            }
            if !loglabelset[axis] {
                self.clear_log_label(axis + 1)?;
            }
        }
        if let Some(c) = clip {
            self.set_clip(c)?;
        }
        if !self.draw.boxp_freeze {
            self.draw.boxp_lbnd = self.draw.box_lbnd;
            self.draw.boxp_ubnd = self.draw.box_ubnd;
        }
        Ok(())
    }

    /// The values a grid uses for the attributes whose defaults depend on
    /// what it draws: each attribute's own value where it is set, else the
    /// value the grid chooses.
    ///
    /// The grid is worked out without drawing anything, through `grf`,
    /// which is asked only for capabilities, scales and text sizes.
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Naxin` when the base or current Frame does not have
    ///   two axes (a current Frame of more is reduced to its first two).
    /// - `ErrorCode::Vsmal`, `ErrorCode::Zerax`, `ErrorCode::Attin` or
    ///   `ErrorCode::Plfmt` when no usable ticks or labels can be found for
    ///   an axis.
    /// - Any error the backend reports.
    pub fn used_values(&mut self, grf: &mut dyn Grf) -> AstResult<UsedValues> {
        // C runs an invisible grid for each attribute that is unset, and
        // none if all are set; the grid gives the same values each time.
        let mut any_unset = !self.test_grid() || !self.test_labelling() || !self.test_border();
        for a in 1..=2 {
            any_unset = any_unset
                || !self.test_tick_gap(a)?
                || !self.test_log_gap(a)?
                || !self.test_tick_centre(a)?
                || !self.test_edge(a)?
                || !self.test_label_at(a)?
                || !self.test_label_units(a)?
                || !self.test_log_ticks(a)?
                || !self.test_log_label(a)?
                || !self.test_min_tick(a)?
                || !self.test_text_lab(a)?
                || !self.test_maj_tick_len(a)?;
        }
        if any_unset {
            self.begin_drawing();
            self.grid_impl(grf, false)?;
        }
        let u = self.used.clone();
        let pick_f = |set: bool, v: f64, used: f64| if set { v } else { used };
        let mut out = UsedValues {
            gap: [0.0; 2],
            log_gap: [0.0; 2],
            centre: [0.0; 2],
            edge: [Edge::Left; 2],
            label_at: [0.0; 2],
            label_units: [false; 2],
            log_ticks: [false; 2],
            log_label: [false; 2],
            min_tick: [0; 2],
            text_lab: [false; 2],
            maj_tick_len: [0.0; 2],
            grid: if self.test_grid() {
                self.grid()
            } else {
                u.grid != 0
            },
            labelling: if self.test_labelling() {
                self.labelling()
            } else if u.labelling != 0 {
                Labelling::Interior
            } else {
                Labelling::Exterior
            },
            border: if self.test_border() {
                self.border()
            } else {
                u.border != 0
            },
        };
        for i in 0..2 {
            let a = i + 1;
            out.gap[i] = pick_f(self.test_tick_gap(a)?, self.tick_gap(a)?, u.gap[i]);
            out.log_gap[i] = pick_f(self.test_log_gap(a)?, self.log_gap(a)?, u.log_gap[i]);
            out.centre[i] = pick_f(self.test_tick_centre(a)?, self.tick_centre(a)?, u.centre[i]);
            out.edge[i] = if self.test_edge(a)? {
                self.edge(a)?
            } else {
                edge_of(u.edge[i])
            };
            out.label_at[i] = pick_f(self.test_label_at(a)?, self.label_at(a)?, u.label_at[i]);
            out.label_units[i] = if self.test_label_units(a)? {
                self.label_units(a)?
            } else {
                u.label_units[i] != 0
            };
            out.log_ticks[i] = if self.test_log_ticks(a)? {
                self.log_ticks(a)?
            } else {
                u.log_ticks[i] != 0
            };
            out.log_label[i] = if self.test_log_label(a)? {
                self.log_label(a)?
            } else {
                u.log_label[i] != 0
            };
            out.min_tick[i] = if self.test_min_tick(a)? {
                self.min_tick(a)?
            } else {
                u.min_tick[i]
            };
            out.text_lab[i] = if self.test_text_lab(a)? {
                self.text_lab(a)?
            } else {
                u.text_lab[i] != 0
            };
            out.maj_tick_len[i] = pick_f(
                self.test_maj_tick_len(a)?,
                self.maj_tick_len(a)?,
                u.maj_tick_len[i],
            );
        }
        Ok(out)
    }
}
