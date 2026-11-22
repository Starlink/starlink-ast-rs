// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A grid's tick marks and the curves through them.
// C: plot.c DrawTicks (:10148), Ticker (:27103), EdgeCrossings (:11369),
// SaveTick (:24817) and DrawAxis (:9231).

use super::Plot;
use super::elements::PlotElement;
use super::ticks::TickInfo;
use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::AstResult;
use crate::frames::FrameRef;
use crate::grf::{Grf, GrfPrimitive};
use crate::math::sqrt;

/// The number of samples along an edge at which ticks are looked for.
// C: EDGETICKS_DIM.
const EDGETICKS_DIM: usize = 100;

/// The positions of the tick marks a grid drew, per axis, kept for a 3-D
/// Plot to read.
// C: the Plot's majtickgx, majtickgy, majtickcount, mintickgx, mintickgy
// and mintickcount.
#[derive(Debug, Clone, Default)]
pub(crate) struct DrawnTicks {
    pub(crate) major: [Vec<(f64, f64)>; 3],
    pub(crate) minor: [Vec<(f64, f64)>; 3],
}

/// The samples along one edge EdgeCrossings searches, kept between calls
/// for the same edge and axis.
// C: EdgeCrossingsStatics.
struct EdgeSamples {
    edge: usize,
    axis: usize,
    edgeax: usize,
    edgeval: f64,
    /// Graphics coordinates of each sample.
    graph: [Vec<f64>; 2],
    /// Physical coordinates of each sample.
    phys: [Vec<f64>; 2],
    /// The unit vector in graphics coordinates along the other physical
    /// axis at each sample.
    vec: [Vec<f64>; 2],
    /// Three times the RMS step in the axis value, or `None` when every
    /// sample is bad.
    limit: Option<f64>,
}

/// One tick position on an edge: graphics position and unit vector.
type Crossing = [f64; 4];

impl Plot {
    /// Record a tick drawn at `(gx, gy)` on the 0-based `axis`, or forget
    /// all recorded ticks if `axis` is `None`.
    // C: SaveTick (plot.c:24817).
    fn save_tick(&mut self, axis: Option<usize>, gx: f64, gy: f64, major: bool) {
        match axis {
            None => self.draw.ticks = DrawnTicks::default(),
            Some(a) => {
                let list = if major {
                    &mut self.draw.ticks.major[a]
                } else {
                    &mut self.draw.ticks.minor[a]
                };
                list.push((gx, gy));
            }
        }
    }

    /// The samples along `edge` for finding where `axis` takes a value.
    // C: the initialisation half of EdgeCrossings (plot.c:11369).
    fn edge_samples(&self, edge: usize, axis: usize, gap: &[f64; 2]) -> AstResult<EdgeSamples> {
        let logticks = self.log_ticks(2 - axis)?;
        let (edgeax, edgeval, edgehi, edgelo) = match edge {
            0 => (
                0,
                if self.xrev { self.xhi } else { self.xlo },
                self.yhi,
                self.ylo,
            ),
            2 => (
                0,
                if self.xrev { self.xlo } else { self.xhi },
                self.yhi,
                self.ylo,
            ),
            3 => (
                1,
                if self.yrev { self.yhi } else { self.ylo },
                self.xhi,
                self.xlo,
            ),
            _ => (
                1,
                if self.yrev { self.ylo } else { self.yhi },
                self.xhi,
                self.xlo,
            ),
        };
        let frame = self.frameset.current();
        let mut g1 = PointSet::zeros(EDGETICKS_DIM, 2);
        let dd = (edgehi - edgelo) / (EDGETICKS_DIM - 1) as f64;
        let mut value = edgelo;
        for i in 0..EDGETICKS_DIM {
            g1.axis_mut(edgeax)[i] = edgeval;
            g1.axis_mut(1 - edgeax)[i] = value;
            value += dd;
        }
        let p2 = self.trans_with(Some(frame), &g1, true, false)?;
        let mut limit = f64::MAX;
        let mut nsum = 0;
        for _ in 0..3 {
            let mut pq1 = BAD_VALUE;
            let mut sum = 0.0;
            nsum = 0;
            for &q in p2.axis(axis) {
                if q != BAD_VALUE && pq1 != BAD_VALUE {
                    let diff = q - pq1;
                    if diff.abs() < limit {
                        sum += diff * diff;
                        nsum += 1;
                    }
                }
                pq1 = q;
            }
            if nsum == 0 {
                break;
            }
            limit = 3.0 * sqrt(sum / nsum as f64);
        }
        let mut s = EdgeSamples {
            edge,
            axis,
            edgeax,
            edgeval,
            graph: [g1.axis(0).to_vec(), g1.axis(1).to_vec()],
            phys: [p2.axis(0).to_vec(), p2.axis(1).to_vec()],
            vec: [Vec::new(), Vec::new()],
            limit: None,
        };
        if nsum == 0 {
            return Ok(s);
        }
        let mut p3 = PointSet::zeros(EDGETICKS_DIM, 2);
        let mut offset = 0.2 * gap[1 - axis];
        for i in 0..EDGETICKS_DIM {
            let (a, b) = (p2.axis(axis)[i], p2.axis(1 - axis)[i]);
            p3.axis_mut(1 - axis)[i] = if a != BAD_VALUE && b != BAD_VALUE {
                if logticks {
                    offset = 0.2 * b * (gap[1 - axis] - 1.0);
                }
                b + offset
            } else {
                BAD_VALUE
            };
            p3.axis_mut(axis)[i] = a;
        }
        let p4 = self.trans_with(None, &p3, false, false)?;
        let mut v = [p4.axis(0).to_vec(), p4.axis(1).to_vec()];
        for i in 0..EDGETICKS_DIM {
            let (x, y) = (g1.axis(0)[i], g1.axis(1)[i]);
            if x != BAD_VALUE && y != BAD_VALUE && v[0][i] != BAD_VALUE && v[1][i] != BAD_VALUE {
                let dx = v[0][i] - x;
                let dy = v[1][i] - y;
                let dl2 = dx * dx + dy * dy;
                if dl2 > 0.0 {
                    let dl = sqrt(dl2);
                    v[0][i] = dx / dl;
                    v[1][i] = dy / dl;
                } else {
                    v[0][i] = BAD_VALUE;
                    v[1][i] = BAD_VALUE;
                }
            } else {
                v[0][i] = BAD_VALUE;
                v[1][i] = BAD_VALUE;
            }
        }
        // Discontinuities: steps of more than nine times the RMS step.
        limit *= 3.0;
        let q1a = p2.axis(axis);
        let q2a = p2.axis(1 - axis);
        let (p1a, p2a) = (g1.axis(0), g1.axis(1));
        let (v1a, v2a) = (&v[0], &v[1]);
        let mut out_g = [Vec::new(), Vec::new()];
        let mut out_q = [Vec::new(), Vec::new()];
        let mut out_v = [Vec::new(), Vec::new()];
        let mut pq1 = BAD_VALUE;
        for i in 0..EDGETICKS_DIM {
            if q1a[i] != BAD_VALUE && pq1 != BAD_VALUE && (q1a[i] - pq1).abs() > limit {
                // C copies the current sample's graphics position but the
                // previous sample's vector and other axis value here.
                out_g[0].push(p1a[i]);
                out_g[1].push(p2a[i]);
                out_v[0].push(v1a[i - 1]);
                out_v[1].push(v2a[i - 1]);
                out_q[1].push(q2a[i - 1]);
                out_q[0].push(if i > 1 && q1a[i - 2] != BAD_VALUE {
                    2.0 * pq1 - q1a[i - 2]
                } else {
                    pq1
                });
                for list in [&mut out_g, &mut out_v, &mut out_q] {
                    list[0].push(BAD_VALUE);
                    list[1].push(BAD_VALUE);
                }
                out_g[0].push(p1a[i - 1]);
                out_g[1].push(p2a[i - 1]);
                out_v[0].push(v1a[i]);
                out_v[1].push(v2a[i]);
                out_q[1].push(q2a[i]);
                out_q[0].push(if i < EDGETICKS_DIM - 1 && q1a[i + 1] != BAD_VALUE {
                    2.0 * q1a[i] - q1a[i + 1]
                } else {
                    pq1
                });
            }
            pq1 = q1a[i];
            out_g[0].push(p1a[i]);
            out_g[1].push(p2a[i]);
            out_q[0].push(q1a[i]);
            out_q[1].push(q2a[i]);
            out_v[0].push(v1a[i]);
            out_v[1].push(v2a[i]);
        }
        // `out_q` holds the searched axis first; store it by axis.
        let [qa, qb] = out_q;
        s.phys = if axis == 0 { [qa, qb] } else { [qb, qa] };
        s.graph = out_g;
        s.vec = out_v;
        s.limit = Some(limit);
        Ok(s)
    }

    /// The graphics positions, and unit vectors along the other axis, where
    /// the 0-based `axis` takes the value `axval` on `edge`.
    // C: the search half of EdgeCrossings (plot.c:11369).
    fn edge_crossings(
        &self,
        edge: usize,
        axis: usize,
        axval: f64,
        gap: &[f64; 2],
        cache: &mut Option<EdgeSamples>,
    ) -> AstResult<Vec<Crossing>> {
        let edge = edge % 4;
        if !matches!(cache, Some(s) if s.edge == edge && s.axis == axis) {
            *cache = Some(self.edge_samples(edge, axis, gap)?);
        }
        let s = cache.as_ref().expect("set above");
        let mut cross = Vec::new();
        if s.limit.is_none() {
            return Ok(cross);
        }
        let p2 = &s.graph[1 - s.edgeax];
        let q1 = &s.phys[axis];
        let q2 = &s.phys[1 - axis];
        let (v1, v2) = (&s.vec[0], &s.vec[1]);
        let (mut pp2, mut pq1, mut pv1, mut pv2) = (0.0, BAD_VALUE, 0.0, 0.0);
        let mut plarger = false;
        for i in 0..q1.len() {
            if q1[i] != BAD_VALUE && q2[i] != BAD_VALUE {
                let larger = q1[i] > axval;
                if pq1 != BAD_VALUE && larger != plarger {
                    let f = (axval - pq1) / (q1[i] - pq1);
                    if f != -1.0 {
                        let z = pp2 + f * (p2[i] - pp2);
                        let (mut vx, mut vy) = (BAD_VALUE, BAD_VALUE);
                        if v1[i] != BAD_VALUE
                            && pv1 != BAD_VALUE
                            && v2[i] != BAD_VALUE
                            && pv2 != BAD_VALUE
                        {
                            vx = pv1 + f * (v1[i] - pv1);
                            vy = pv2 + f * (v2[i] - pv2);
                            let dl2 = vx * vx + vy * vy;
                            if dl2 > 0.0 {
                                let dl = sqrt(dl2);
                                vx /= dl;
                                vy /= dl;
                            } else {
                                vx = BAD_VALUE;
                                vy = BAD_VALUE;
                            }
                        }
                        cross.push(if s.edgeax == 1 {
                            [z, s.edgeval, vx, vy]
                        } else {
                            [s.edgeval, z, vx, vy]
                        });
                    }
                }
                plarger = larger;
            }
            pp2 = p2[i];
            pq1 = q1[i];
            pv1 = v1[i];
            pv2 = v2[i];
        }
        Ok(cross)
    }

    /// Draw a tick of length `tklen` at each place on `edge` where the
    /// 0-based `axis` takes `value`, pointing into the plotting area.
    // C: Ticker (plot.c:27103). C moves on to the next crossing only after
    // drawing one, so a crossing with a bad position or vector is looked at
    // again in its place; the port does the same.
    #[allow(clippy::too_many_arguments)]
    fn ticker(
        &mut self,
        edge: usize,
        axis: usize,
        value: f64,
        gap: &[f64; 2],
        tklen: f64,
        majtick: bool,
        save: bool,
        cache: &mut Option<EdgeSamples>,
    ) -> AstResult<()> {
        let cross = self.edge_crossings(edge, axis, value, gap, cache)?;
        let mut k = 0;
        for _ in 0..cross.len() {
            let [x, y, mut vx, mut vy] = cross[k];
            if vx == BAD_VALUE || vy == BAD_VALUE || x == BAD_VALUE || y == BAD_VALUE {
                continue;
            }
            if self.xrev {
                vx = -vx;
            }
            if self.yrev {
                vy = -vy;
            }
            if (edge == 0 && vx < 0.0)
                || (edge == 1 && vy > 0.0)
                || (edge == 2 && vx > 0.0)
                || (edge == 3 && vy < 0.0)
            {
                vx = -vx;
                vy = -vy;
            }
            if self.xrev {
                vx = -vx;
            }
            if self.yrev {
                vy = -vy;
            }
            if ((edge == 1 || edge == 3) && x < self.xhi && x > self.xlo)
                || ((edge == 0 || edge == 2) && y < self.yhi && y > self.ylo)
            {
                let mut xe = x + tklen * vx;
                let mut ye = y + tklen * vy;
                if edge == 1 || edge == 3 {
                    if xe > self.xhi {
                        ye = y + tklen * vy * (self.xhi - x) / (xe - x);
                        xe = self.xhi;
                    } else if xe < self.xlo {
                        ye = y + tklen * vy * (self.xlo - x) / (xe - x);
                        xe = self.xlo;
                    }
                } else if ye > self.yhi {
                    xe = x + tklen * vx * (self.yhi - y) / (ye - y);
                    ye = self.yhi;
                } else if ye < self.ylo {
                    xe = x + tklen * vx * (self.ylo - y) / (ye - y);
                    ye = self.ylo;
                }
                if save {
                    self.save_tick(Some(axis), x, y, majtick);
                }
                if x != xe || y != ye {
                    self.bpoly(x as f32, y as f32);
                    self.apoly(xe as f32, ye as f32);
                    self.opoly();
                }
            }
            k += 1;
        }
        Ok(())
    }

    /// Draw the major and minor tick marks of each axis, round the edges
    /// if either axis is labelled there, else along the curves at
    /// `labelat`.
    // C: DrawTicks (plot.c:10148).
    pub(crate) fn draw_ticks(
        &mut self,
        grf: &mut dyn Grf,
        grid: &[TickInfo; 2],
        drawgrid: bool,
        labelat: &[f64; 2],
        gap: &[f64; 2],
    ) -> AstResult<()> {
        let mindim = {
            let (a, b) = (self.xhi - self.xlo, self.yhi - self.ylo);
            if a < b { a } else { b }
        };
        self.save_tick(None, 0.0, 0.0, false);
        let ticks_id = [PlotElement::Ticks1, PlotElement::Ticks2];
        if labelat[0] == BAD_VALUE || labelat[1] == BAD_VALUE {
            let nedge = if self.tick_all() { 2 } else { 1 };
            let mut cache = None;
            for ed in 0..nedge {
                for axis in 0..2 {
                    let a1 = axis + 1;
                    self.grf_attrs(grf, ticks_id[axis], true, GrfPrimitive::Line)?;
                    let mjtklen = if self.test_maj_tick_len(a1)? || !drawgrid {
                        self.maj_tick_len(a1)? * mindim
                    } else {
                        0.0
                    };
                    let mntklen = self.min_tick_len(a1)? * mindim;
                    let edge = ((super::grid::edge_code(self.edge(a1)?) + ed * 2) % 4)
                        .unsigned_abs() as usize;
                    let info = &grid[axis];
                    let nminor = info.nminor;
                    let mut minlo = (1 - nminor) / 2;
                    let mut minhi = nminor / 2;
                    let logticks = self.log_ticks(a1)?;
                    let (mut delta1, mut delta2) = (0.0, 0.0);
                    if !logticks {
                        delta1 = gap[axis] / f64::from(nminor);
                        delta2 = delta1;
                    }
                    let nmajor = info.nmajor();
                    for tick in 0..nmajor {
                        let value = info.ticks[tick];
                        self.ticker(edge, axis, value, gap, mjtklen, true, ed == 0, &mut cache)?;
                        if info.minticks.is_empty() {
                            if logticks {
                                delta1 = value * (gap[axis] - 1.0) / f64::from(nminor);
                                delta2 = delta1 / gap[axis];
                            }
                            // C writes "} if( tick == 1 )" with no else, so
                            // the last test pairs with the second only.
                            if tick == 0 {
                                minlo = 1 - nminor;
                            }
                            if tick == 1 {
                                minlo = (1 - nminor) / 2;
                            } else if tick + 1 == nmajor {
                                minhi = nminor - 1;
                            }
                            let mut minval = value + f64::from(minlo) * delta2;
                            for i in minlo..=minhi {
                                if i != 0 {
                                    self.ticker(
                                        edge,
                                        axis,
                                        minval,
                                        gap,
                                        mntklen,
                                        false,
                                        ed == 0,
                                        &mut cache,
                                    )?;
                                }
                                minval += if i < 0 { delta2 } else { delta1 };
                            }
                        }
                    }
                    for &value in &info.minticks {
                        self.ticker(edge, axis, value, gap, mntklen, false, ed == 0, &mut cache)?;
                    }
                    self.grf_attrs(grf, ticks_id[axis], false, GrfPrimitive::Line)?;
                }
            }
            return Ok(());
        }
        let map = self.frameset.mapping(FrameRef::Base, FrameRef::Current)?;
        // C reads LogTicks here only where it makes the minor ticks itself;
        // with explicit minor ticks it uses the value left by an earlier
        // axis, or 0.
        let mut logticks = false;
        for axis in 0..2 {
            let a1 = axis + 1;
            self.grf_attrs(grf, ticks_id[axis], true, GrfPrimitive::Line)?;
            let mjtklen = if self.test_maj_tick_len(a1)? || !drawgrid {
                self.maj_tick_len(a1)? * mindim
            } else {
                0.0
            };
            let mntklen = self.min_tick_len(a1)? * mindim;
            let (mut mjsign, mut mnsign) = (1.0, 1.0);
            let mut dl2_limit = 0.0001 * mindim;
            dl2_limit *= dl2_limit;
            let info = &grid[axis];
            let nmajor = info.nmajor();
            let nminor = info.nminor;
            let mut a: Vec<f64> = Vec::new();
            let mut b: Vec<f64> = Vec::new();
            let mut majflags: Vec<bool> = Vec::new();
            let ntot = if !info.minticks.is_empty() {
                nmajor + nmajor + nminor as usize
            } else {
                nmajor as i64 as usize + ((nmajor as i64 + 1) * (i64::from(nminor) - 1)) as usize
            };
            if ntot > 0 {
                let mut push = |a: &mut Vec<f64>, b: &mut Vec<f64>, val, at, at2, major| {
                    a.push(val);
                    b.push(at);
                    a.push(val);
                    b.push(at2);
                    majflags.push(major);
                };
                if !info.minticks.is_empty() {
                    // C reads one value past each list here; the port stops
                    // at its end. This path is reached only with explicit
                    // ticks, which a 3-D Plot sets.
                    for (list, major) in [(&info.ticks, true), (&info.minticks, false)] {
                        for &val in list.iter() {
                            let lblat2 = if logticks {
                                labelat[axis] + 0.2 * (val * (gap[axis] - 1.0))
                            } else {
                                labelat[axis] + 0.2 * gap[1 - axis]
                            };
                            push(&mut a, &mut b, val, labelat[axis], lblat2, major);
                        }
                    }
                } else {
                    let lblat = labelat[axis];
                    let mut lblat2 = 0.0;
                    let mut delta1 = 0.0;
                    logticks = self.log_ticks(a1)?;
                    if !logticks {
                        lblat2 = labelat[axis] + 0.2 * gap[1 - axis];
                        delta1 = gap[axis] / f64::from(nminor);
                    }
                    let (minlo, minhi) = (1, nminor - 1);
                    let frame = self.frameset.current();
                    let axmin = frame.bottom(a1)?;
                    let axmax = frame.top(a1)?;
                    for tick in -1..nmajor as i64 {
                        let val = if tick == -1 {
                            if !logticks {
                                info.ticks[0] - gap[axis]
                            } else {
                                info.ticks[0] / gap[axis]
                            }
                        } else {
                            info.ticks[tick as usize]
                        };
                        if logticks {
                            let diff = val * (gap[axis] - 1.0);
                            lblat2 = labelat[axis] + 0.2 * diff;
                            delta1 = diff / f64::from(nminor);
                        }
                        if tick > -1 {
                            push(&mut a, &mut b, val, lblat, lblat2, true);
                        }
                        let mut minval = val + f64::from(minlo) * delta1;
                        for i in minlo..=minhi {
                            if i != 0 && minval >= axmin && minval <= axmax {
                                push(&mut a, &mut b, minval, lblat, lblat2, false);
                            }
                            minval += delta1;
                        }
                    }
                }
                let nel = a.len();
                let mut pset1 = PointSet::zeros(nel, 2);
                pset1.axis_mut(axis).copy_from_slice(&a);
                pset1.axis_mut(1 - axis).copy_from_slice(&b);
                let pset2 = map.transform(&pset1, false)?;
                let pset3 = self.trans_with(None, &pset1, false, false)?;
                let (x, y) = (pset2.axis(0), pset2.axis(1));
                let (xc, yc) = (pset3.axis(0), pset3.axis(1));
                let mut ux = BAD_VALUE;
                let mut uy = 0.0;
                let mut first = true;
                for tick in 0..nel / 2 {
                    let a0 = a[2 * tick];
                    let (x0, y0) = (x[2 * tick], y[2 * tick]);
                    let (mut x1, mut y1) = (x[2 * tick + 1], y[2 * tick + 1]);
                    let (x2, y2) = (xc[2 * tick], yc[2 * tick]);
                    if x0 != BAD_VALUE
                        && y0 != BAD_VALUE
                        && x1 != BAD_VALUE
                        && y1 != BAD_VALUE
                        && x2 != BAD_VALUE
                        && y2 != BAD_VALUE
                        && x0 <= self.xhi
                        && x0 >= self.xlo
                        && y0 <= self.yhi
                        && y0 >= self.ylo
                    {
                        let dx = x1 - x0;
                        let dy = y1 - y0;
                        let dl2 = dx * dx + dy * dy;
                        if dl2 > dl2_limit {
                            let dl = sqrt(dl2);
                            if first {
                                first = false;
                                let k = 2 * tick + 2;
                                if tick + 1 < nel / 2
                                    && x[k] != BAD_VALUE
                                    && y[k] != BAD_VALUE
                                    && a0 != BAD_VALUE
                                    && a[k] != BAD_VALUE
                                {
                                    let mut ex = x[k] - x0;
                                    let mut ey = y[k] - y0;
                                    if a[k] < a0 {
                                        ex = -ex;
                                        ey = -ey;
                                    }
                                    if ex * dy > ey * dx {
                                        mjsign = -1.0;
                                        mnsign = -1.0;
                                    }
                                }
                            }
                            ux = dx / dl;
                            uy = dy / dl;
                        }
                        if ux != BAD_VALUE {
                            let major = majflags[tick];
                            if major {
                                x1 = x0 + mjsign * mjtklen * ux;
                                y1 = y0 + mjsign * mjtklen * uy;
                            } else {
                                x1 = x0 + mnsign * mntklen * ux;
                                y1 = y0 + mnsign * mntklen * uy;
                            }
                            self.save_tick(Some(axis), x0, y0, major);
                            if x0 != x1 || y0 != y1 {
                                self.bpoly(x0 as f32, y0 as f32);
                                self.apoly(x1 as f32, y1 as f32);
                                self.opoly();
                            }
                        }
                    }
                }
            }
            self.grf_attrs(grf, ticks_id[axis], false, GrfPrimitive::Line)?;
        }
        Ok(())
    }

    /// Draw the curve through the ticks of each axis labelled inside the
    /// plotting area.
    // C: DrawAxis (plot.c:9231).
    pub(crate) fn draw_axis(
        &mut self,
        grf: &mut dyn Grf,
        grid: &[TickInfo; 2],
        labelat: &[f64; 2],
        gap: &[f64; 2],
    ) -> AstResult<()> {
        let axis_id = [PlotElement::Axis1, PlotElement::Axis2];
        for axis in 0..2 {
            let a1 = axis + 1;
            self.grf_attrs(grf, axis_id[axis], true, GrfPrimitive::Line)?;
            if self.draw_axes(a1)? && labelat[axis] != BAD_VALUE {
                let frame = self.frameset.current();
                let mut bot = frame.bottom(a1)?;
                let mut top = frame.top(a1)?;
                if bot > top {
                    std::mem::swap(&mut bot, &mut top);
                }
                let logticks = self.log_ticks(a1)?;
                let mut diff = if logticks { 0.0 } else { gap[axis] };
                let mut start = [0.0; 2];
                for (tick, &value) in grid[axis].ticks.iter().enumerate() {
                    if logticks {
                        diff = value * (gap[axis] - 1.0);
                    }
                    start[axis] = value;
                    start[1 - axis] = labelat[axis];
                    if tick == 0 {
                        let mut udiff = value - bot;
                        if udiff > diff {
                            udiff = diff;
                        }
                        if udiff > 0.0 {
                            self.ax_plot(grf, axis, &start, -udiff, true, "astGrid")?;
                        }
                    }
                    let udiff = if value + diff > top {
                        top - value
                    } else {
                        diff
                    };
                    if udiff > 0.0 {
                        self.ax_plot(grf, axis, &start, udiff, true, "astGrid")?;
                    }
                }
                let mut udiff = top - start[axis];
                if udiff > diff {
                    udiff = diff;
                }
                if udiff > 0.0 {
                    self.ax_plot(grf, axis, &start, udiff, true, "astGrid")?;
                }
            }
            self.grf_attrs(grf, axis_id[axis], false, GrfPrimitive::Line)?;
        }
        Ok(())
    }
}
