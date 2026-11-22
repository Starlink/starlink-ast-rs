// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Where a grid's major tick marks go on each axis, the labels they carry,
//! and the sections of the grid lines through them.
// C: plot.c GridLines (:18758), TickMarks (:27287), GetTicks (:16304),
// DefGap (:8988), GoodGrid (:16900), Typical (:28603), CountGood (:7448),
// FindMajTicks (:12346), FindMajTicks2 (:12617), Norm1 (:23002), Compared
// (:7238), CheckLabels (:6025), CheckLabels2 (:6167).
//
// C's casts of doubles to int truncate; the port casts at the same places.
// TickMarks changes the current Frame's Format and axis Digits directly,
// as C's astGetFrame pointer does, without FrameSet integrity, and puts
// them back before it returns.

use std::cmp::Ordering;

use super::Plot;
use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::FrameRef;
use crate::frames::find_frame::frame_class_name;
use crate::grf::{Grf, GrfCap};
use crate::mappings::DynMapping;
use crate::mappings::Mapping;
use crate::mappings::helpers::ast_equals;
use crate::math::{log10, powf, sqrt};

/// The fewest, most and best number of major ticks on an axis.
// C: MAJTICKS_MIN, MAJTICKS_MAX, MAJTICKS_OPT.
const MAJTICKS_MIN: f64 = 6.0;
const MAJTICKS_MAX: f64 = 14.0;
const MAJTICKS_OPT: usize = 10;

/// The tick marks of one axis of a grid.
// C: TickInfo.
#[derive(Debug, Clone, Default)]
pub(crate) struct TickInfo {
    /// The axis values of the major ticks.
    pub(crate) ticks: Vec<f64>,
    /// The axis values of the minor ticks, if given explicitly.
    pub(crate) minticks: Vec<f64>,
    /// The number of minor tick divisions between major ticks, or the
    /// number of explicit minor ticks.
    pub(crate) nminor: i32,
    /// The formatted label of each major tick.
    pub(crate) labels: Vec<String>,
    /// The start on the other axis of each section of the grid lines, and
    /// its length.
    pub(crate) start: Vec<f64>,
    pub(crate) length: Vec<f64>,
    /// The number of sections.
    pub(crate) nsect: usize,
    /// The Format the labels were made with.
    pub(crate) fmt: Option<String>,
    /// The gap between major ticks.
    pub(crate) gap: f64,
}

impl TickInfo {
    /// The number of major ticks.
    pub(crate) fn nmajor(&self) -> usize {
        self.ticks.len()
    }
}

/// What GetTicks finds on its first call and keeps for the second axis.
// C: GetTicksStatics.
struct TickStatics {
    /// The base to current Mapping.
    map: DynMapping,
    /// The physical values of a grid over the plotting area, sorted on
    /// each axis with bad values last.
    phys: [Vec<f64>; 2],
    defgaps: [f64; 2],
    typval: [f64; 2],
    width: [f64; 2],
    maxticks: usize,
    mintick: usize,
    ngood: [usize; 2],
    bad: bool,
}

/// The order C's Compared gives: increasing, bad values last.
fn compared(a: &f64, b: &f64) -> Ordering {
    if a == b {
        Ordering::Equal
    } else if *a == BAD_VALUE {
        Ordering::Greater
    } else if *b == BAD_VALUE {
        Ordering::Less
    } else if a > b {
        Ordering::Greater
    } else {
        Ordering::Less
    }
}

/// The number of values that are not bad.
// C: CountGood (plot.c:7448).
fn count_good(data: &[f64]) -> usize {
    data.iter().filter(|&&v| v != BAD_VALUE).count()
}

/// A typical value of the good values of `value` between `lolim` and
/// `hilim`, the mode of a histogram refined recursively, and, with
/// `width`, the width of the range the values cover.
// C: Typical (plot.c:28603).
fn typical(value: &[f64], lolim: f64, hilim: f64, width: Option<&mut f64>) -> f64 {
    let mut result = BAD_VALUE;
    let mut minval = f64::MAX;
    let mut maxval = -f64::MAX;
    let mut ngood = 0usize;
    let in_range = |v: f64| v != BAD_VALUE && v >= lolim && v <= hilim;
    for &v in value {
        if in_range(v) {
            if v < minval {
                minval = v;
            }
            if v > maxval {
                maxval = v;
            }
            if ngood == 0 {
                result = v;
            }
            ngood += 1;
        }
    }
    let mut wout = 0.0;
    if maxval != -f64::MAX {
        wout = maxval - minval;
    }
    if ngood > 2 {
        let nbin = ngood.div_ceil(2);
        let delta = (maxval - minval) / nbin as f64;
        if delta > 0.0 {
            let mut hist = vec![0i32; nbin];
            let mut mean = 0.0;
            let mut nc = 0usize;
            for &v in value {
                if in_range(v) {
                    mean += v;
                    let mut ibin = ((v - minval) / delta) as usize;
                    if ibin == nbin {
                        ibin -= 1;
                    }
                    hist[ibin] += 1;
                    nc += 1;
                }
            }
            mean /= ngood as f64;
            let modify = nbin > 4 && f64::from(hist[0] + hist[nbin - 1]) < 0.98 * ngood as f64;
            let mut nonemp = 0usize;
            let mut maxcnt: i32 = 0;
            let mut ibin = 0usize;
            let w0 = (nbin / 2) as f64;
            for (i, &h) in hist.iter().enumerate() {
                let mut cnt = f64::from(h);
                if cnt != 0.0 {
                    nonemp += 1;
                }
                if modify {
                    let w1 = i as f64 * w0;
                    let w2 = (nbin - 1 - i) as f64 * w0;
                    let w = if w1 < w2 { w1 } else { w2 };
                    if w < 1.0 {
                        cnt *= w;
                    }
                }
                // C keeps the largest count in an int; its two tests set the
                // same values, kept apart as C has them.
                #[allow(clippy::if_same_then_else)]
                if cnt > f64::from(maxcnt) {
                    maxcnt = cnt as i32;
                    ibin = i;
                } else if cnt == f64::from(maxcnt)
                    && (minval + (i as f64 - 0.5) * delta - mean).abs()
                        < (minval + (ibin as f64 - 0.5) * delta - mean).abs()
                {
                    maxcnt = cnt as i32;
                    ibin = i;
                }
            }
            wout = nonemp as f64 * delta;
            if (maxcnt as usize) < nc && ibin as f64 * delta > 1000.0 * f64::EPSILON * maxval.abs()
            {
                let lo = minval + ibin as f64 * delta;
                result = typical(value, lo, lo + delta, None);
            }
        }
    }
    if let Some(w) = width {
        *w = wout;
    }
    result
}

/// Tick values at multiples of `gap` from `centre` that cover the sorted
/// axis values `data`, with `nfill` extra ticks either side of each gap in
/// their coverage.
// C: FindMajTicks2 (plot.c:12617).
fn find_maj_ticks2(nfill: i32, gap: f64, centre: f64, data: &[f64]) -> Vec<f64> {
    let mut ticks = Vec::new();
    let k = ((data[0] - centre) / gap).floor() as i32;
    for i in 0..nfill {
        ticks.push(gap * f64::from(k - nfill + i) + centre);
    }
    ticks.push(gap * f64::from(k) + centre);
    let mut klast = k;
    for &d in &data[1..] {
        let k = ((d - centre) / gap).floor() as i32;
        for j in 0..nfill + 1 {
            if k - klast > nfill + 2 - j {
                ticks.push(gap * f64::from(klast + nfill + 1 - j) + centre);
            }
            if k - klast > nfill - j {
                ticks.push(gap * f64::from(k - nfill + j) + centre);
            }
        }
        klast = k;
    }
    for i in 0..nfill {
        ticks.push(gap * f64::from(klast + i + 1) + centre);
    }
    ticks
}

impl Plot {
    /// The physical values on a grid covering the part of the plotting
    /// area with valid coordinates, and the fraction of the plotting area
    /// it covers.
    // C: GoodGrid (plot.c:16900), whose grid omits the corners outside a
    // disk; its cell height is measured between two points of one row, so
    // the area is never widened in y.
    fn good_grid(&self) -> AstResult<(PointSet, f64)> {
        let mut dim: usize = 16;
        let mut ngood = 0;
        let (mut xmin, mut xmax, mut ymin, mut ymax) = (0.0, 0.0, 0.0, 0.0);
        let mut grid = None;
        while ngood < 4 {
            dim *= 2;
            if dim >= 512 {
                return Err(AstError::new(
                    ErrorCode::Vsmal,
                    "astGrid(Plot): The area of the plot containing usable coordinates on \
                     both axes is too small.",
                ));
            }
            let (graph, phys) = self.make_grid(
                None, true, dim, self.xlo, self.xhi, self.ylo, self.yhi, false,
            )?;
            ngood = 0;
            xmin = f64::MAX;
            xmax = -f64::MAX;
            ymin = f64::MAX;
            ymax = -f64::MAX;
            for j in 0..graph.npoint() {
                if phys.axis(0)[j] != BAD_VALUE && phys.axis(1)[j] != BAD_VALUE {
                    let (px, py) = (graph.axis(0)[j], graph.axis(1)[j]);
                    if px < xmin {
                        xmin = px;
                    }
                    if px > xmax {
                        xmax = px;
                    }
                    if py < ymin {
                        ymin = py;
                    }
                    if py > ymax {
                        ymax = py;
                    }
                    ngood += 1;
                }
            }
            grid = Some((graph, phys));
        }
        let (graph, mut phys) = grid.expect("the loop runs at least once");
        let frac = ngood as f64 / graph.npoint() as f64;
        let dx = graph.axis(0)[1] - graph.axis(0)[0];
        let dy = graph.axis(1)[1] - graph.axis(1)[0];
        xmax += dx;
        xmin -= dx;
        ymax += dy;
        ymin -= dy;
        if (xmax - xmin) < 0.9 * (self.xhi - self.xlo)
            || (ymax - ymin) < 0.9 * (self.yhi - self.ylo)
        {
            let a = (xmax - xmin) / (self.xhi - self.xlo);
            let b = (ymax - ymin) / (self.yhi - self.ylo);
            let mut d = (dim as f64 * if a > b { a } else { b }) as usize;
            if d < 32 {
                d = 32;
            }
            phys = self
                .make_grid(None, true, d, xmin, xmax, ymin, ymax, false)?
                .1;
        }
        Ok((phys, frac))
    }

    /// The sorted physical values on a grid over the plotting area, a
    /// first guess at the gap between major ticks on each axis, and the
    /// defaults for LogTicks and LogLabel, which it sets.
    // C: DefGap (plot.c:8988).
    #[allow(clippy::type_complexity)]
    fn def_gap(&mut self) -> AstResult<([Vec<f64>; 2], [f64; 2], [usize; 2], f64, bool)> {
        let (phys, frac) = self.good_grid()?;
        let psize = phys.npoint();
        let mut gaps = [1.0, 1.0];
        let mut ngood = [0, 0];
        let mut inval = false;
        let mut sorted = [phys.axis(0).to_vec(), phys.axis(1).to_vec()];
        for j in 0..2 {
            sorted[j].sort_by(compared);
            ngood[j] = count_good(&sorted[j]);
            if ngood[j] < psize {
                inval = true;
            }
            if ngood[j] < MAJTICKS_OPT {
                return Err(too_small(j));
            }
            let minv = sorted[j][0];
            let maxv = sorted[j][ngood[j] - 1];
            let logticks = if self.test_log_ticks(j + 1)? {
                self.log_ticks(j + 1)?
            } else {
                let mut lt = false;
                if self.log_plot(j + 1)? && minv * maxv > 0.0 {
                    let dran = maxv / minv;
                    if dran >= 90.0 || dran <= 1.0 / 90.0 {
                        lt = true;
                    }
                }
                self.set_log_ticks(j + 1, lt)?;
                lt
            };
            if !self.test_log_label(j + 1)? {
                self.set_log_label(j + 1, logticks)?;
            }
            if !logticks {
                let dk = (ngood[j] as f64 / MAJTICKS_OPT as f64) as usize;
                let mut qgap = Vec::with_capacity(MAJTICKS_OPT);
                let mut k = dk;
                while k < ngood[j] && qgap.len() < MAJTICKS_OPT {
                    qgap.push(sorted[j][k] - sorted[j][k - dk]);
                    k += dk;
                }
                qgap.sort_by(compared);
                gaps[j] = qgap[qgap.len() / 2];
                if gaps[j] <= 0.0 {
                    gaps[j] = (sorted[j][ngood[j] - 1] - sorted[j][0]) / MAJTICKS_OPT as f64;
                    if gaps[j] <= 0.0 {
                        return Err(too_small(j));
                    }
                }
            } else if maxv * minv <= 0.0 {
                return Err(AstError::new(
                    ErrorCode::Zerax,
                    format!(
                        "astGrid(Plot): The range of coordinate values covered by axis {} \
                         includes the origin and so logarithmic ticks cannot be produced.",
                        j + 1
                    ),
                ));
            } else if maxv == minv {
                return Err(too_small(j));
            } else {
                let mut n = (0.5 + MAJTICKS_OPT as f64 * sqrt(frac)) as i32;
                if n < 5 {
                    n = 5;
                }
                gaps[j] = powf(maxv / minv, 1.0 / (f64::from(n) - 1.0));
            }
        }
        Ok((sorted, gaps, ngood, frac, inval))
    }

    /// The tick statics GetTicks finds on its first call.
    fn tick_statics(&mut self) -> AstResult<TickStatics> {
        let map = self.frameset.mapping(FrameRef::Base, FrameRef::Current)?;
        let (phys, defgaps, ngood, frac, bad) = self.def_gap()?;
        let mut maxticks = (0.5 + MAJTICKS_MAX * sqrt(frac)) as usize;
        let mut mintick = (0.5 + MAJTICKS_MIN * sqrt(frac)) as usize;
        if mintick < 3 {
            mintick = 3;
        }
        if maxticks < 8 {
            maxticks = 8;
        }
        if maxticks < mintick {
            maxticks = mintick;
        }
        let mut typval = [0.0; 2];
        let mut width = [0.0; 2];
        for i in 0..2 {
            typval[i] = typical(
                &phys[i][..ngood[i]],
                -f64::MAX,
                f64::MAX,
                Some(&mut width[i]),
            );
        }
        Ok(TickStatics {
            map,
            phys,
            defgaps,
            typval,
            width,
            maxticks,
            mintick,
            ngood,
            bad,
        })
    }

    /// Put tick values into the primary domain of the Mapping by
    /// transforming them, with the other axis at `refval`, to graphics
    /// coordinates and back, trying other values of the other axis for
    /// those that fail.
    // C: Norm1 (plot.c:23002). C's `refval - 0.1*( itry + 1 )*width` is
    // written here as refval plus a negated step, which is the same value.
    fn norm1(
        map: &dyn Mapping,
        axis: usize,
        vals: &mut [f64],
        refval: f64,
        width: f64,
    ) -> AstResult<()> {
        let nv = vals.len();
        let mut p1 = PointSet::zeros(nv, 2);
        p1.axis_mut(axis).copy_from_slice(vals);
        p1.axis_mut(1 - axis).fill(refval);
        let p2 = map.transform(&p1, false)?;
        p1 = map.transform(&p2, true)?;
        let mut flags = vec![false; nv];
        let mut nbad = 0;
        for i in 0..nv {
            let a = p1.axis(axis)[i];
            if a != BAD_VALUE {
                vals[i] = a;
                p1.axis_mut(axis)[i] = BAD_VALUE;
                flags[i] = true;
            } else if vals[i] != BAD_VALUE {
                nbad += 1;
                p1.axis_mut(axis)[i] = vals[i];
            } else {
                flags[i] = true;
            }
        }
        for itry in 0..10 {
            for sign in [1.0, -1.0] {
                if nbad == 0 {
                    continue;
                }
                p1.axis_mut(1 - axis)
                    .fill(refval + sign * 0.1 * f64::from(itry + 1) * width);
                let p2 = map.transform(&p1, false)?;
                p1 = map.transform(&p2, true)?;
                nbad = 0;
                for i in 0..nv {
                    let a = p1.axis(axis)[i];
                    if a != BAD_VALUE {
                        vals[i] = a;
                        flags[i] = true;
                        p1.axis_mut(axis)[i] = BAD_VALUE;
                    } else if !flags[i] {
                        nbad += 1;
                        p1.axis_mut(axis)[i] = vals[i];
                    }
                }
            }
        }
        Ok(())
    }

    /// The major tick values on the 0-based `axis` for a gap `gap`, placed
    /// at multiples of the gap from `cen` (found, and returned through
    /// `cen`, if bad), covering the sorted axis values `data`.
    // C: FindMajTicks (plot.c:12346).
    #[allow(clippy::too_many_arguments)]
    fn find_maj_ticks(
        &self,
        map: &dyn Mapping,
        axis: usize,
        refval: f64,
        width: f64,
        gap: f64,
        cen: &mut f64,
        data: &[f64],
    ) -> AstResult<Vec<f64>> {
        let frame = self.frameset.current();
        let centre = if *cen != BAD_VALUE {
            *cen
        } else {
            let c = frame.centre(axis + 1, data[0], gap)?;
            *cen = c;
            c
        };
        let mut nfill = 0;
        let mut ticks = find_maj_ticks2(nfill, gap, centre, data);
        let mut nticks = ticks.len() as i64;
        let mut linc = -100000_i64;
        let mut nsame = 0;
        let mut use_nfill = 0;
        while nfill < 100 {
            nfill += 1;
            ticks = find_maj_ticks2(nfill, gap, centre, data);
            let ntnew = ticks.len() as i64;
            let inc = ntnew - nticks;
            if inc == linc {
                nsame += 1;
            } else {
                nsame = 0;
                use_nfill = nfill;
            }
            if nsame == 3 {
                ticks = find_maj_ticks2(use_nfill, gap, centre, data);
                break;
            }
            linc = inc;
            nticks = ntnew;
        }
        let mut bot = frame.bottom(axis + 1)?;
        let mut top = frame.top(axis + 1)?;
        if bot > top {
            std::mem::swap(&mut bot, &mut top);
        }
        let delta = 0.05 * gap;
        for r in ticks.iter_mut() {
            if *r != BAD_VALUE {
                if (*r - bot).abs() < delta {
                    *r = bot + delta;
                } else if (*r - top).abs() < delta {
                    *r = top - delta;
                } else if *r < bot || *r > top {
                    *r = BAD_VALUE;
                }
            }
        }
        Self::norm1(map, axis, &mut ticks, refval, width)?;
        for r in ticks.iter_mut() {
            if *r != BAD_VALUE {
                let f = (0.5 + (*r - centre) / gap).floor();
                *r = f * gap + centre;
                if r.abs() < 1.0e-10 * gap {
                    *r = 0.0;
                }
            }
        }
        ticks.sort_by(compared);
        // Drop duplicates and bad values; C keeps the first value even if
        // every value is bad.
        let mut w = 0;
        for r in 1..ticks.len() {
            if ticks[r] != BAD_VALUE && !ast_equals(ticks[r] - centre, ticks[w] - centre, 1.0e8) {
                w += 1;
                ticks[w] = ticks[r];
            }
        }
        ticks.truncate(w + 1);
        Ok(ticks)
    }

    /// The major tick values and gap for the 0-based `axis`, the number of
    /// minor divisions, and the explicit minor ticks if any, with `cen` the
    /// tick centre (bad to choose one, returned).
    // C: GetTicks (plot.c:16304), which reports through *inval and *refval
    // the statics' bad flag and typical value, read here from `statics`.
    fn get_ticks(
        &mut self,
        axis: usize,
        cen: &mut f64,
        format_set: bool,
        statics: &TickStatics,
    ) -> AstResult<(f64, Vec<f64>, Vec<f64>, i32)> {
        let refval = statics.typval[1 - axis];
        let tv = &self.tick_values[axis];
        if !tv.major.is_empty() {
            let ticks = tv.major.clone();
            let minticks = tv.minor.clone();
            let nminor = minticks.len() as i32;
            let used_gap = if ticks.len() > 1 && ticks[0] != BAD_VALUE && ticks[1] != BAD_VALUE {
                (ticks[1] - ticks[0]).abs()
            } else {
                BAD_VALUE
            };
            return Ok((used_gap, ticks, minticks, nminor));
        }
        let logticks = self.log_ticks(axis + 1)?;
        let gap = if logticks {
            self.log_gap(axis + 1)?
        } else {
            self.tick_gap(axis + 1)?
        };
        let data = &statics.phys[axis][..statics.ngood[axis]];
        let maxv = data[data.len() - 1];
        let minv = data[0];
        let mut nminor = 0;
        let mut used_gap = 0.0;
        let mut ticks = Vec::new();
        if logticks {
            let dran = if minv != 0.0 { maxv / minv } else { 0.0 };
            if dran < 10.0 && dran > 0.1 {
                return Err(AstError::new(
                    ErrorCode::Vsmal,
                    format!(
                        "astGrid(Plot): Cannot produce logarithmically spaced major tick marks \
                         on axis {} since the dynamic range of the axis is too small.",
                        axis + 1
                    ),
                ));
            }
            let findcen = *cen == BAD_VALUE || !format_set;
            let (mut ilo, mut ihi, mut used_cen) = (0i32, 0i32, 0.0);
            // The index range of ticks covering the axis for a gap.
            let range = |used_cen: f64, log_used_gap: f64| -> (i32, i32) {
                let lo = log10(minv / used_cen) / log_used_gap;
                let hi = log10(maxv / used_cen) / log_used_gap;
                if log_used_gap > 0.0 {
                    ((lo).floor() as i32, hi.ceil() as i32)
                } else {
                    (lo.ceil() as i32, (hi).floor() as i32)
                }
            };
            let centre = |used_gap: f64, log_used_gap: f64| -> f64 {
                let mut c = powf(
                    used_gap,
                    f64::from((0.5 * log10(maxv * minv) / log_used_gap) as i32),
                );
                if maxv < 0.0 {
                    c = -c;
                }
                c
            };
            if gap == BAD_VALUE {
                let mut test_gap = statics.defgaps[axis];
                let mut nmajor = 0i32;
                for _ in 0..10 {
                    let mut log_used_gap = f64::from(log10(test_gap).round() as i32);
                    if log_used_gap == 0.0 {
                        log_used_gap = if test_gap > 1.0 { 1.0 } else { -1.0 };
                    }
                    nminor = 9;
                    used_gap = powf(10.0, log_used_gap);
                    used_cen = if findcen {
                        centre(used_gap, log_used_gap)
                    } else {
                        *cen
                    };
                    (ilo, ihi) = range(used_cen, log_used_gap);
                    nmajor = ihi - ilo + 1;
                    if nmajor <= 0 {
                        test_gap = sqrt(test_gap);
                    } else if (nmajor as usize) < statics.mintick {
                        test_gap = powf(test_gap, f64::from(nmajor) / statics.mintick as f64);
                    } else if nmajor as usize > statics.maxticks {
                        test_gap = powf(test_gap, f64::from(nmajor) / statics.maxticks as f64);
                    } else {
                        break;
                    }
                }
                let _ = nmajor;
                ilo -= 1;
                ihi += 1;
            } else if gap == 0.0 {
                return Err(AstError::new(
                    ErrorCode::Attin,
                    format!(
                        "astGrid(Plot): Invalid value zero given for attribute LogGap({}).",
                        axis + 1
                    ),
                ));
            } else if gap < 0.0 {
                return Err(AstError::new(
                    ErrorCode::Attin,
                    format!(
                        "astGrid(Plot): Invalid negative value {} given for attribute \
                         LogGap({}).",
                        crate::frames::axis_format::sprintf_double("%f", gap),
                        axis + 1
                    ),
                ));
            } else {
                used_gap = gap;
                if maxv.abs() < minv.abs() {
                    if gap > 1.0 {
                        used_gap = 1.0 / gap;
                    }
                } else if gap < 1.0 {
                    used_gap = 1.0 / gap;
                }
                let mut log_used_gap = f64::from(log10(used_gap).round() as i32);
                if log_used_gap == 0.0 {
                    log_used_gap = if gap > 1.0 { 1.0 } else { -1.0 };
                }
                used_gap = powf(10.0, log_used_gap);
                nminor = 9;
                used_cen = if findcen {
                    centre(used_gap, log_used_gap)
                } else {
                    *cen
                };
                (ilo, ihi) = range(used_cen, log_used_gap);
                if ihi - ilo + 1 < 2 {
                    return Err(AstError::new(
                        ErrorCode::Attin,
                        format!(
                            "astGrid(Plot): Unusable value {} given for attribute LogGap({}).",
                            crate::frames::axis_format::sprintf_double("%f", gap),
                            axis + 1
                        ),
                    ));
                }
            }
            for i in ilo..=ihi {
                ticks.push(used_cen * powf(used_gap, f64::from(i)));
            }
            *cen = used_cen;
        } else {
            let cen0 = if format_set { *cen } else { BAD_VALUE };
            let frame = self.frameset.current();
            let width = statics.width[1 - axis];
            let map = statics.map.as_ref();
            if gap == BAD_VALUE {
                let mut old_used_gap = BAD_VALUE;
                let mut test_gap = statics.defgaps[axis];
                let (mut too_large, mut too_small) = (false, false);
                let mut nochange = 0;
                let mut i = 0;
                while i < 10 {
                    // C undoes the count of a pass that did not change the
                    // gap, but still tests its ticks.
                    let mut counted = true;
                    let (new_used_gap, nm) = frame.gap(axis + 1, test_gap)?;
                    if let Some(nm) = nm {
                        nminor = nm;
                    }
                    if new_used_gap != used_gap {
                        nochange = 0;
                        old_used_gap = used_gap;
                        used_gap = new_used_gap;
                        *cen = cen0;
                        ticks =
                            self.find_maj_ticks(map, axis, refval, width, used_gap, cen, data)?;
                    } else if nochange < 25 {
                        nochange += 1;
                        counted = false;
                    } else {
                        return Err(AstError::new(
                            ErrorCode::Vsmal,
                            format!(
                                "astGrid(Plot): Cannot produce enough major tick marks on axis \
                                 {} using the current axis format (\"{}\").",
                                axis + 1,
                                frame.format_str(axis + 1)?
                            ),
                        ));
                    }
                    let nmajor = ticks.len();
                    if nmajor == 0 {
                        test_gap *= 0.5;
                        too_large = true;
                        too_small = false;
                    } else if nmajor < statics.mintick {
                        if too_small {
                            break;
                        }
                        test_gap *= nmajor as f64 / statics.mintick as f64;
                        too_large = true;
                        too_small = false;
                    } else if nmajor > statics.maxticks {
                        if too_large {
                            used_gap = old_used_gap;
                            *cen = cen0;
                            ticks =
                                self.find_maj_ticks(map, axis, refval, width, used_gap, cen, data)?;
                            break;
                        }
                        test_gap *= nmajor as f64 / statics.maxticks as f64;
                        too_small = true;
                        too_large = false;
                    } else {
                        break;
                    }
                    if counted {
                        i += 1;
                    }
                }
            } else {
                let (g, nm) = frame.gap(axis + 1, gap)?;
                used_gap = g;
                if let Some(nm) = nm {
                    nminor = nm;
                }
                *cen = cen0;
                ticks = self.find_maj_ticks(map, axis, refval, width, used_gap, cen, data)?;
            }
        }
        if ticks.is_empty() {
            return Err(AstError::new(
                ErrorCode::Grfer,
                "astGrid(Plot): Cannot find any usable tick mark values. ",
            ));
        }
        Ok((used_gap, ticks, Vec::new(), nminor))
    }

    /// The labels of the tick values `ticks` on the 0-based `axis`,
    /// formatted after normalising with the other axis at `refval`, or
    /// `None` if two adjacent labels are the same (unless `force`).
    // C: CheckLabels (plot.c:6025).
    fn check_labels(
        &self,
        axis: usize,
        ticks: &[f64],
        force: bool,
        refval: f64,
    ) -> AstResult<Option<Vec<String>>> {
        let frame = self.frameset.current();
        let mut labels: Vec<String> = Vec::with_capacity(ticks.len());
        for &t in ticks {
            let mut val = [0.0; 2];
            val[axis] = t;
            val[1 - axis] = refval;
            frame.norm(&mut val)?;
            let label = frame.format(axis + 1, val[axis])?;
            if !force && labels.last() == Some(&label) {
                return Ok(None);
            }
            labels.push(label);
        }
        Ok(Some(labels))
    }

    /// The labels of `ticks` formatted now, or `None` if any is longer
    /// than the same label in `old`.
    // C: CheckLabels2 (plot.c:6167).
    fn check_labels2(
        &self,
        axis: usize,
        ticks: &[f64],
        old: &[String],
        refval: f64,
    ) -> AstResult<Option<Vec<String>>> {
        let frame = self.frameset.current();
        let mut labels = Vec::with_capacity(ticks.len());
        for (i, &t) in ticks.iter().enumerate() {
            let mut val = [0.0; 2];
            val[axis] = t;
            val[1 - axis] = refval;
            frame.norm(&mut val)?;
            let label = frame.format(axis + 1, val[axis])?;
            if label.len() > old[i].len() {
                return Ok(None);
            }
            labels.push(label);
        }
        Ok(Some(labels))
    }

    /// The tick marks and labels of the 0-based `axis`, with `cen` the
    /// tick centre (bad to choose one) and `gap` the gap, both returned.
    // C: TickMarks (plot.c:27287).
    fn tick_marks(
        &mut self,
        grf: &mut dyn Grf,
        axis: usize,
        cen: &mut f64,
        gap: &mut f64,
        statics: &TickStatics,
    ) -> AstResult<TickInfo> {
        const MAXFLD: usize = 10;
        let a1 = axis + 1;
        let fmtset = self.frameset.current().test_format_str(a1)?;
        let (used_gap, ticks, minticks, mut nminor) = self.get_ticks(axis, cen, fmtset, statics)?;
        let refval = statics.typval[1 - axis];
        let mut old_format = None;
        let mut reset_fmt = false;
        if self.log_label(a1)? && self.escape() && self.g_cap(grf, GrfCap::Scales, 1) {
            if fmtset {
                old_format = Some(self.frameset.current().format_str(a1)?);
            }
            let fr = self.frameset.frame_mut(FrameRef::Current)?;
            fr.set_format_str(a1, "%&g")?;
            let text = fr.format(a1, 1.0e4)?;
            let split = fr.fields(a1, "%&g", &text, MAXFLD)?;
            let ok = split.fields.len() == 2
                && split.fields[0].len() == 2
                && text[split.fields[0].clone()].starts_with("10");
            if !ok {
                match old_format.take() {
                    Some(f) => fr.set_format_str(a1, &f)?,
                    None => fr.clear_format_str(a1)?,
                }
            } else {
                reset_fmt = true;
            }
        }
        let mut digset = false;
        if fmtset {
            let fmt = self.frameset.current().format_str(a1)?;
            digset = true;
            let b = fmt.as_bytes();
            let mut a = 0;
            while let Some(off) = b[a..].iter().position(|&c| c == b'.') {
                a += off + 1;
                if b.get(a) == Some(&b'*') {
                    digset = false;
                    break;
                }
            }
        }
        let mut labels = Vec::new();
        let mut used_fmt = None;
        let mut ok = false;
        let frame_digits_set = {
            let fr = self.frameset.current();
            fr.test_axis_digits(a1)? || fr.test_digits()
        };
        if digset || frame_digits_set {
            if let Some(l) = self.check_labels(axis, &ticks, true, refval)? {
                labels = l;
                ok = true;
            }
            used_fmt = Some(self.frameset.current().format_str(a1)?);
        } else {
            let first = self
                .check_labels(axis, &ticks, true, refval)?
                .expect("forced labels are always made");
            let mut bot_digits = 1;
            let mut oldlabels = first;
            let start = self.frameset.current().digits() - 1;
            let mut digits = start;
            while digits > 0 {
                self.frameset
                    .frame_mut(FrameRef::Current)?
                    .set_axis_digits(a1, digits)?;
                match self.check_labels2(axis, &ticks, &oldlabels, refval)? {
                    None => {
                        bot_digits = digits + 1;
                        break;
                    }
                    Some(newlabels) => oldlabels = newlabels,
                }
                digits -= 1;
            }
            for digits in bot_digits..1000 {
                self.frameset
                    .frame_mut(FrameRef::Current)?
                    .set_axis_digits(a1, digits)?;
                if let Some(l) = self.check_labels(axis, &ticks, false, refval)? {
                    labels = l;
                    ok = true;
                    used_fmt = Some(self.frameset.current().format_str(a1)?);
                    break;
                }
            }
            self.frameset
                .frame_mut(FrameRef::Current)?
                .clear_axis_digits(a1)?;
        }
        if reset_fmt {
            let fr = self.frameset.frame_mut(FrameRef::Current)?;
            match old_format {
                Some(f) => fr.set_format_str(a1, &f)?,
                None => fr.clear_format_str(a1)?,
            }
        }
        if !ok {
            let fr = self.frameset.current();
            let class = frame_class_name(fr);
            return Err(AstError::new(
                ErrorCode::Plfmt,
                if fmtset {
                    format!(
                        "astGrid(Plot): No numerical labels can be produced for axis {a1} using \
                         the supplied {class} format string '{}'.",
                        fr.format_str(a1)?
                    )
                } else {
                    format!(
                        "astGrid(Plot): No suitable format can be found to produce numerical \
                         labels for axis {a1} of a {class}."
                    )
                },
            ));
        }
        *gap = used_gap;
        if self.test_min_tick(a1)? {
            nminor = self.min_tick(a1)?;
        }
        Ok(TickInfo {
            ticks,
            minticks,
            nminor,
            labels,
            start: Vec::new(),
            length: Vec::new(),
            nsect: 0,
            fmt: used_fmt,
            gap: used_gap,
        })
    }

    /// The major ticks on each axis of a grid, their labels, and the
    /// sections of the grid lines through them, with `cen` and `gap` the
    /// tick centres and gaps (bad to choose them, returned), and whether
    /// any part of the plotting area has invalid coordinates.
    // C: GridLines (plot.c:18758). Where either axis has logarithmic ticks
    // C sets the section count of axis 1 for both axes, leaving that of
    // axis 2 at zero; the port does the same.
    pub(crate) fn grid_lines(
        &mut self,
        grf: &mut dyn Grf,
        cen: &mut [f64; 2],
        gap: &mut [f64; 2],
    ) -> AstResult<([TickInfo; 2], bool)> {
        let statics = self.tick_statics()?;
        let mut info0 = self.tick_marks(grf, 0, &mut cen[0], &mut gap[0], &statics)?;
        let lt0 = self.log_ticks(1)?;
        let mut info1 = self.tick_marks(grf, 1, &mut cen[1], &mut gap[1], &statics)?;
        let lt1 = self.log_ticks(2)?;
        let logticks = [lt0, lt1];
        let frame = self.frameset.current();
        for j in 0..2 {
            let mut bot = frame.bottom(2 - j)?;
            let mut top = frame.top(2 - j)?;
            if bot > top {
                std::mem::swap(&mut bot, &mut top);
            }
            let other = if j == 0 { &info1 } else { &info0 };
            let ticks = other.ticks.clone();
            let nticks = ticks.len();
            let g = gap[1 - j];
            let clamp = |v: f64| {
                let m = if bot > v { bot } else { v };
                if top < m { top } else { m }
            };
            let (mut starts, mut lengths) = (Vec::new(), Vec::new());
            let nsect = if logticks[0] || logticks[1] {
                let (s, end) = if logticks[1 - j] {
                    (clamp(ticks[0] / g), clamp(ticks[nticks - 1] * g))
                } else {
                    (clamp(ticks[0] - g), clamp(ticks[nticks - 1] + g))
                };
                starts.push(s);
                lengths.push(end - s);
                None
            } else {
                let mut i = 0;
                while i < nticks {
                    let mut start = ticks[i];
                    i += 1;
                    while i < nticks && (ticks[i] - ticks[i - 1]) < 1.5 * g {
                        i += 1;
                    }
                    let mut length = ticks[i - 1] - start;
                    start -= g;
                    length += 2.0 * g;
                    let end = start + length;
                    start = clamp(start);
                    starts.push(start);
                    lengths.push(clamp(end) - start);
                }
                Some(starts.len())
            };
            let target = if j == 0 { &mut info0 } else { &mut info1 };
            target.start = starts;
            target.length = lengths;
            match nsect {
                Some(n) => target.nsect = n,
                None => info0.nsect = 1,
            }
        }
        Ok(([info0, info1], statics.bad))
    }
}

/// C's error for an axis whose values cover too small a range.
fn too_small(j: usize) -> AstError {
    AstError::new(
        ErrorCode::Vsmal,
        format!(
            "astGrid(Plot): The range of coordinate values covered by axis {} is too small to \
             plot.",
            j + 1
        ),
    )
}
