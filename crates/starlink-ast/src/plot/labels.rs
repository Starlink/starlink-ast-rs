// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A grid's numerical labels, its axis labels and its title.
// C: plot.c Labels (:20871), GVec (:19181), PlotLabels (:23471), Overlap
// (:23268), SplitValue (:13068), FindDPTZ (:12766), BoxCheck (:5315),
// Inside (:19951), Cross (:7509), Compare_LL (:7318) and TextLabels
// (:26541).

use std::cmp::Ordering;

use super::Plot;
use super::attrs::TextGapType;
use super::elements::PlotElement;
use super::grid::EdgeLabel;
use super::text::{chr_len, has_escapes};
use super::ticks::TickInfo;
use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::Frame;
use crate::grf::{Grf, GrfPrimitive};
use crate::math::sqrt;

/// The boxes of the labels drawn so far, against which a new label is
/// tested for overlap; each is four x then four y corner coordinates.
// C: the float array Overlap keeps, whose first element is the count.
pub(crate) type LabelBoxes = Vec<[f32; 8]>;

/// One numerical label being placed.
// C: LabelList, with its priority, saved_prio and atext.
struct Label {
    index: f64,
    text: String,
    x: f64,
    y: f64,
    just: [u8; 2],
    upx: f64,
    upy: f64,
    priority: i32,
    saved_prio: i32,
    /// The byte offset in `text` of its abbreviated form.
    atext: usize,
}

impl Label {
    fn atext(&self) -> &str {
        &self.text[self.atext..]
    }
}

/// Whether the point `(x, y)` is inside the convex polygon of the `n`
/// corners `(cx[i], cy[i])`.
// C: Inside (plot.c:19951).
fn inside(n: usize, cx: &[f32], cy: &[f32], x: f32, y: f32) -> bool {
    let side = |i: usize, j: usize| (cx[j] - cx[i]) * (y - cy[i]) > (x - cx[i]) * (cy[j] - cy[i]);
    let sgn = side(0, 1);
    for i in 1..n {
        let j = if i + 1 >= n { i + 1 - n } else { i + 1 };
        if side(i, j) != sgn {
            return false;
        }
    }
    true
}

/// Whether the segment AB crosses the segment CD.
// C: Cross (plot.c:7509).
#[allow(clippy::too_many_arguments)]
fn cross(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32, dx: f32, dy: f32) -> bool {
    let (m1, m2) = (dx - cx, dy - cy);
    let denom = (bx - ax) * m2 - (by - ay) * m1;
    let num = (ay - cy) * m1 - (ax - cx) * m2;
    if denom == 0.0 {
        return false;
    }
    let t1 = num / denom;
    if !(0.0..=1.0).contains(&t1) {
        return false;
    }
    let (m1, m2) = (bx - ax, by - ay);
    let denom = (dx - cx) * m2 - (dy - cy) * m1;
    let num = (cy - ay) * m1 - (cx - ax) * m2;
    if denom == 0.0 {
        return false;
    }
    let t2 = num / denom;
    (0.0..=1.0).contains(&t2)
}

/// Whether two boxes overlap. Each box is put into order round its edge
/// first, as C does, in place.
// C: BoxCheck (plot.c:5315).
fn box_check(b: &mut [f32; 8], c: &mut [f32; 8]) -> bool {
    let corners = |p: &[f32; 8]| ([p[0], p[1], p[2], p[3]], [p[4], p[5], p[6], p[7]]);
    let (bx, by) = corners(b);
    let (cx, cy) = corners(c);
    for j in 0..4 {
        if inside(4, &cx, &cy, bx[j], by[j]) {
            return true;
        }
    }
    for j in 0..4 {
        if inside(4, &bx, &by, cx[j], cy[j]) {
            return true;
        }
    }
    let order = |p: &mut [f32; 8]| {
        if cross(p[0], p[4], p[1], p[5], p[2], p[6], p[3], p[7]) {
            p.swap(2, 1);
            p.swap(6, 5);
        } else if cross(p[1], p[5], p[2], p[6], p[3], p[7], p[0], p[4]) {
            p.swap(2, 3);
            p.swap(6, 7);
        }
    };
    order(b);
    order(c);
    for j in 0..4 {
        let jp = (j + 1) % 4;
        for i in 0..4 {
            let ip = (i + 1) % 4;
            if cross(
                b[j],
                b[j + 4],
                b[jp],
                b[jp + 4],
                c[i],
                c[i + 4],
                c[ip],
                c[ip + 4],
            ) {
                return true;
            }
        }
    }
    false
}

/// C's `isspace` in the "C" locale.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

impl Plot {
    /// A label as it is to be drawn: split into two lines about its middle
    /// space when it is long, or when an earlier label was split, unless it
    /// holds escape sequences or they are not interpreted.
    // C: SplitValue (plot.c:13068).
    fn split_value(&self, value: &str, axis: usize, split: &mut bool) -> String {
        if !self.escape() || has_escapes(value) {
            return value.to_string();
        }
        let v = value.as_bytes();
        let l = v.len();
        let mut idmin = 2 * l as i64;
        let mut imin: Option<usize> = None;
        for (i, &c) in v.iter().enumerate() {
            if is_space(c) {
                let id = (i as i64 - (l / 2) as i64).abs();
                if id < idmin {
                    idmin = id;
                    imin = Some(i);
                }
            }
        }
        if !(*split || (l > 9 && imin.is_some())) {
            return value.to_string();
        }
        *split = true;
        let mut d = String::new();
        match imin {
            None => {
                if axis == 0 {
                    d.push_str(&" ".repeat(l));
                    d.push_str("%v170+");
                }
                d.push_str(value);
            }
            Some(imin) => {
                let span = |r: std::ops::Range<usize>| {
                    let mut s: Option<(usize, usize)> = None;
                    for i in r {
                        if !is_space(v[i]) {
                            s = Some((s.map_or(i, |p| p.0), i));
                        }
                    }
                    s
                };
                let (bs, be) = span(0..imin).map_or((-1, -1), |(a, b)| (a as i64, b as i64));
                let (as_, ae) = span(imin + 1..l).map_or((-1, -1), |(a, b)| (a as i64, b as i64));
                let nbef = be - bs + 1;
                let naft = ae - as_ + 1;
                let (nlong, nshort) = if nbef > naft {
                    (nbef, naft)
                } else {
                    (naft, nbef)
                };
                let rsp = (0.5 * (nlong - nshort + 1) as f64) as f32;
                if nbef < naft {
                    d.push_str(&" ".repeat(rsp as usize));
                }
                if bs >= 0 {
                    d.push_str(&value[bs as usize..=be as usize]);
                }
                d.push_str("%v100+");
                d.push_str(&format!(
                    "%<{}+",
                    (60.0 * f64::from(nlong as f32 - rsp)) as i32
                ));
                if as_ >= 0 {
                    d.push_str(&value[as_ as usize..=ae as usize]);
                }
            }
        }
        d
    }

    /// The number of decimal places in the last field of a label, the
    /// number of trailing zeros across its fields, and whether it has a
    /// non-zero digit.
    // C: FindDPTZ (plot.c:12766). C searches for the decimal point past the
    // end of the last field, and tests it with ff - dot, which is never
    // positive; the port does the same.
    fn find_dptz(&self, axis: usize, fmt: &str, text: &str) -> AstResult<(i32, i32, bool)> {
        let frame = self.frameset.current();
        let split = frame.fields(axis + 1, fmt, text, 10)?;
        let t = text.as_bytes();
        let nf = split.fields.len();
        let (mut ndp, mut ntz, mut result) = (0i32, 0i32, false);
        if nf == 0 {
            return Ok((ndp, ntz, result));
        }
        let last = &split.fields[nf - 1];
        let (ff, fnc) = (last.start, last.len() as i64);
        let mut dot = t[ff..].iter().position(|&c| c == b'.').map(|p| ff + p);
        if let Some(d) = dot
            && ff as i64 - d as i64 >= fnc
        {
            dot = None;
        }
        if let Some(d) = dot {
            ndp = t[d + 1..].iter().take_while(|c| c.is_ascii_digit()).count() as i32;
            let mxnd = (fnc - (d - ff) as i64 - 1) as i32;
            if ndp > mxnd {
                ndp = mxnd;
            }
        }
        for f in split.fields.iter().rev() {
            let mut l = t[f.start..]
                .iter()
                .take_while(|c| b"-+0123456789.".contains(c))
                .count();
            if l > f.len() {
                l = f.len();
            }
            let mut stopped = false;
            for j in (0..l).rev() {
                let c = t[f.start + j];
                if c == b'0' {
                    ntz += 1;
                } else if c.is_ascii_digit() {
                    result = true;
                    stopped = true;
                    break;
                }
            }
            if stopped {
                break;
            }
        }
        Ok((ndp, ntz, result))
    }

    /// Whether a label would overlap a label already in `boxes`, adding
    /// its box to them (and to the grid's bounding box) if not.
    // C: Overlap (plot.c:23268) with mode -1; its other modes, which count
    // and truncate the boxes, are `boxes.len()` and `boxes.truncate`.
    #[allow(clippy::too_many_arguments)]
    fn overlap(
        &mut self,
        grf: &mut dyn Grf,
        esc: bool,
        text: &str,
        x: f32,
        y: f32,
        just: [u8; 2],
        upx: f32,
        upy: f32,
        boxes: &mut LabelBoxes,
    ) -> AstResult<bool> {
        let b = self.render_text(grf, false, esc, text, x, y, just, upx, upy)?;
        let mut new = [
            b.xbn[0], b.xbn[1], b.xbn[2], b.xbn[3], b.ybn[0], b.ybn[1], b.ybn[2], b.ybn[3],
        ];
        for old in boxes.iter_mut() {
            if box_check(&mut new, old) {
                return Ok(true);
            }
        }
        // C stores the box as BoxCheck left it, its corners possibly put in
        // order round its edge.
        boxes.push(new);
        let d = &mut self.draw;
        for i in 0..4 {
            d.box_lbnd[0] = super::draw::fmin(b.xbn[i], d.box_lbnd[0]);
            d.box_ubnd[0] = super::draw::fmax(b.xbn[i], d.box_ubnd[0]);
            d.box_lbnd[1] = super::draw::fmin(b.ybn[i], d.box_lbnd[1]);
            d.box_ubnd[1] = super::draw::fmax(b.ybn[i], d.box_ubnd[1]);
        }
        Ok(false)
    }

    /// Draw the numerical labels of the 0-based `axis`, abbreviated and
    /// thinned so that none overlap.
    // C: PlotLabels (plot.c:23471).
    pub(crate) fn plot_labels(
        &mut self,
        grf: &mut dyn Grf,
        esc: bool,
        axis: usize,
        list: Vec<EdgeLabel>,
        fmt: &str,
        boxes: &mut LabelBoxes,
    ) -> AstResult<()> {
        if list.is_empty() || !self.num_lab(axis + 1)? {
            return Ok(());
        }
        let nlab = list.len();
        let nbox = boxes.len();
        let mut list: Vec<Label> = list
            .into_iter()
            .map(|l| Label {
                index: l.index,
                text: l.text,
                x: l.x,
                y: l.y,
                just: l.just,
                upx: f64::from(l.upx),
                upy: f64::from(l.upy),
                priority: 0,
                saved_prio: 0,
                atext: 0,
            })
            .collect();
        // C sorts the labels with qsort, whose order of labels at the same
        // index depends on the platform; the port sorts stably.
        list.sort_by(|a, b| {
            if a.index < b.index {
                Ordering::Less
            } else if a.index > b.index {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        });
        let tolx = (0.02 * (self.xhi - self.xlo).abs()) as f32;
        let toly = (0.02 * (self.yhi - self.ylo).abs()) as f32;
        for i in 1..nlab {
            for j in 0..i {
                if list[i].text == list[j].text
                    && (list[i].x - list[j].x).abs() < f64::from(tolx)
                    && (list[i].y - list[j].y).abs() < f64::from(toly)
                {
                    list[i].priority = -1;
                    list[i].saved_prio = -1;
                    break;
                }
            }
        }
        let mut mxdp = 0;
        for l in &list {
            let (dp, _, _) = self.find_dptz(axis, fmt, &l.text)?;
            if dp > mxdp {
                mxdp = dp;
            }
        }
        let mut split = false;
        let lab0 = (nlab / 2) as i64;
        let mut nzmax = -1;
        let mut root: Option<usize> = None;
        let mut rootoff = 0i64;
        let mut root_found = false;
        for i in 0..nlab {
            if list[i].priority > -1 {
                let (dp, nz0, found) = self.find_dptz(axis, fmt, &list[i].text)?;
                let nz = nz0 + mxdp - dp;
                list[i].priority = nz;
                list[i].saved_prio = nz;
                if nz > nzmax {
                    nzmax = nz;
                }
                let better = match root {
                    None => true,
                    Some(r) => {
                        nz > list[r].priority
                            || (nz == list[r].priority && (i as i64 - lab0).abs() < rootoff)
                    }
                } || !found;
                if better && !root_found {
                    let ok = axis == 0 || {
                        let text = self.split_value(&list[i].text, axis, &mut split);
                        let l = &list[i];
                        let (x, y, just, ux, uy) =
                            (l.x as f32, l.y as f32, l.just, l.upx as f32, l.upy as f32);
                        !self.overlap(grf, esc, &text, x, y, just, ux, uy, boxes)?
                    };
                    if ok {
                        root = Some(i);
                        rootoff = (i as i64 - lab0).abs();
                        if !found {
                            root_found = true;
                        }
                    }
                    boxes.truncate(nbox);
                }
            }
            list[i].atext = 0;
        }
        // C: when every label overlaps one on an earlier axis, C takes the
        // first label of non-negative priority working out from the middle,
        // and if none is found keeps the last it looked at; it reports an
        // error only when it looked at none (two labels or fewer).
        let root = match root {
            Some(r) => r,
            None => {
                let mut r = None;
                for off in 0..(nlab - 1) / 2 {
                    let a = nlab / 2 + off;
                    r = Some(a);
                    if list[a].priority >= 0 {
                        break;
                    }
                    let b = nlab / 2 - off;
                    r = Some(b);
                    if list[b].priority >= 0 {
                        break;
                    }
                }
                match r {
                    Some(r) => r,
                    None => {
                        return Err(AstError::new(
                            ErrorCode::Plfmt,
                            format!(
                                "astGrid(Plot): Cannot produce labels for axis {}.",
                                axis + 1
                            ),
                        ));
                    }
                }
            }
        };
        list[root].priority = nzmax + 1;
        list[root].saved_prio = nzmax + 1;
        let abb = self.abbreviate(axis + 1)?;
        let mut jgap = 1usize;
        let mut olap = 1usize;
        let mut odd = false;
        while olap > 0 && !odd {
            let mut prio = nzmax + 1;
            while prio > 0 {
                prio -= 1;
                for i in 0..nlab {
                    if list[i].priority != prio {
                        continue;
                    }
                    let prios: Vec<i32> = list.iter().map(|l| l.priority).collect();
                    let llhi = (i + 1..nlab)
                        .find(|&j| prios[j] > prio)
                        .or_else(|| (i + 1..nlab).find(|&j| prios[j] == prio));
                    let lllo = (0..i)
                        .rev()
                        .find(|&j| prios[j] > prio)
                        .or_else(|| (0..i).rev().find(|&j| prios[j] == prio));
                    let frame = self.frameset.current();
                    let text = &list[i].text;
                    let mut at = if !abb {
                        0
                    } else {
                        match (lllo, llhi) {
                            (None, Some(h)) => {
                                frame.abbrev(axis + 1, fmt, Some(&list[h].text), text)?
                            }
                            (Some(lo), None) => {
                                frame.abbrev(axis + 1, fmt, Some(&list[lo].text), text)?
                            }
                            (None, None) => frame.abbrev(axis + 1, fmt, None, text)?,
                            (Some(lo), Some(h)) => {
                                let tlo =
                                    frame.abbrev(axis + 1, fmt, Some(&list[lo].text), text)?;
                                let thi = frame.abbrev(axis + 1, fmt, Some(&list[h].text), text)?;
                                let (lolen, hilen) = (text.len() - tlo, text.len() - thi);
                                if lolen > 0 && lolen < hilen { tlo } else { thi }
                            }
                        }
                    };
                    if at >= text.len() {
                        at = frame.abbrev(axis + 1, fmt, None, text)?;
                    }
                    list[i].atext = at;
                }
            }
            let mut nleft = 1;
            let l = &list[root];
            let (t, x, y, just, ux, uy) = (
                self.split_value(l.atext(), axis, &mut split),
                l.x as f32,
                l.y as f32,
                l.just,
                l.upx as f32,
                l.upy as f32,
            );
            olap = usize::from(self.overlap(grf, esc, &t, x, y, just, ux, uy, boxes)?);
            for dir in [1i64, -1] {
                let mut latext = list[root].atext().to_string();
                let mut i = root as i64 + dir;
                while i >= 0 && (i as usize) < nlab {
                    let l = &list[i as usize];
                    if l.priority >= 0 {
                        if l.atext() != latext {
                            let (t, x, y, just, ux, uy) = (
                                self.split_value(l.atext(), axis, &mut split),
                                l.x as f32,
                                l.y as f32,
                                l.just,
                                l.upx as f32,
                                l.upy as f32,
                            );
                            if self.overlap(grf, esc, &t, x, y, just, ux, uy, boxes)? {
                                olap += 1;
                            } else {
                                nleft += 1;
                            }
                        }
                        latext = list[i as usize].atext().to_string();
                    }
                    i += dir;
                }
            }
            if axis == 1 && olap == 1 {
                olap = 0;
            }
            if olap > 0 && (jgap == 3 || nleft < 3 || self.log_plot(axis + 1)?) {
                jgap = 0;
                odd = true;
            } else {
                odd = false;
            }
            if olap > 0 {
                jgap += 1;
                let mut nexti = root + jgap;
                #[allow(clippy::needless_range_loop)]
                for i in root + 1..nlab {
                    if i == nexti {
                        nexti += jgap;
                        list[i].priority = list[i].saved_prio;
                    } else {
                        list[i].priority = -1;
                    }
                }
                let mut nexti = root as i64 - jgap as i64;
                for i in (0..root).rev() {
                    if i as i64 == nexti {
                        nexti -= jgap as i64;
                        list[i].priority = list[i].saved_prio;
                    } else {
                        list[i].priority = -1;
                    }
                }
                // C resets all but the last label's abbreviation.
                for l in list.iter_mut().take(nlab - 1) {
                    l.atext = 0;
                }
            }
            boxes.truncate(nbox);
        }
        #[allow(clippy::needless_range_loop)]
        for i in 0..nlab {
            if list[i].priority < 0 {
                continue;
            }
            let l = &list[i];
            let (t, x, y, just, ux, uy) = (
                self.split_value(l.atext(), axis, &mut split),
                l.x as f32,
                l.y as f32,
                l.just,
                l.upx as f32,
                l.upy as f32,
            );
            if !self.overlap(grf, esc, &t, x, y, just, ux, uy, boxes)? {
                let t = self.split_value(list[i].atext(), axis, &mut split);
                self.render_text_opt(grf, true, esc, &t, x, y, just, ux, uy)?;
            }
        }
        Ok(())
    }

    /// The graphics position of a physical position and the unit tangent
    /// in graphics coordinates to the 0-based `axis` there, found from
    /// points `delta` either side; the tangent `(dx, dy)` is updated only
    /// when one can be found.
    // C: GVec (plot.c:19181).
    fn gvec(
        &self,
        phy: &[f64; 2],
        axis: usize,
        delta: f64,
        g: &mut (f64, f64),
        d: &mut (f64, f64),
    ) -> AstResult<()> {
        let mut p1 = PointSet::zeros(3, 2);
        #[allow(clippy::needless_range_loop)]
        for i in 0..2 {
            p1.axis_mut(i).fill(phy[i]);
        }
        if phy[axis] != BAD_VALUE {
            p1.axis_mut(axis)[0] = phy[axis] - delta;
            p1.axis_mut(axis)[2] = phy[axis] + delta;
        }
        let p2 = self.trans_with(None, None, &p1, false, false)?;
        let (x, y) = (p2.axis(0), p2.axis(1));
        *g = (x[1], y[1]);
        let (gx, gy) = *g;
        if gx == BAD_VALUE || gy == BAD_VALUE {
            return Ok(());
        }
        let unit = |dx: f64, dy: f64| -> (f64, f64, f64) {
            let dd = dx * dx + dy * dy;
            if dd > 0.0 {
                let dd = sqrt(dd);
                (dx / dd, dy / dd, dd)
            } else {
                (dx, dy, BAD_VALUE)
            }
        };
        let (dx1, dy1, dd1) = if x[2] != BAD_VALUE && y[2] != BAD_VALUE {
            unit(x[2] - gx, y[2] - gy)
        } else {
            (0.0, 0.0, BAD_VALUE)
        };
        let (dx2, dy2, dd2) = if x[0] != BAD_VALUE && y[0] != BAD_VALUE {
            unit(gx - x[0], gy - y[0])
        } else {
            (0.0, 0.0, BAD_VALUE)
        };
        if dd1 == BAD_VALUE && dd2 == BAD_VALUE {
            return Ok(());
        }
        *d = if dd1 == BAD_VALUE {
            (dx2, dy2)
        } else if dd2 == BAD_VALUE || dd2 > 100.0 * dd1 {
            (dx1, dy1)
        } else if dd1 > 100.0 * dd2 {
            (dx2, dy2)
        } else if d.0 != BAD_VALUE {
            // C tests *dx twice where it means *dx and *dy.
            if (dx1 * d.0 + dy1 * d.1) / dd1 > (dx2 * d.0 + dy2 * d.1) / dd2 {
                (dx1, dy1)
            } else {
                (dx2, dy2)
            }
        } else if dd1 < dd2 {
            (dx1, dy1)
        } else {
            (dx2, dy2)
        };
        Ok(())
    }

    /// Draw the numerical labels: round the edges, or along the curves at
    /// `labelat`.
    // C: Labels (plot.c:20871).
    pub(crate) fn labels(
        &mut self,
        grf: &mut dyn Grf,
        grid: &[TickInfo; 2],
        cdata: &[Vec<super::curve::CurveData>; 2],
        gap: &[f64; 2],
        labelat: &[f64; 2],
    ) -> AstResult<()> {
        let esc = self.escape();
        if labelat[0] == BAD_VALUE || labelat[1] == BAD_VALUE {
            self.edge_labels(grf, true, grid, cdata, true)?;
            return Ok(());
        }
        let (alpha, beta) = self.g_scales(grf)?;
        let (a, b) = (
            (f64::from(alpha) * (self.xhi - self.xlo)).abs(),
            (f64::from(beta) * (self.yhi - self.ylo)).abs(),
        );
        let mindim = if a < b { a } else { b };
        let sin45 = 1.0 / sqrt(2.0);
        let mut boxes = LabelBoxes::new();
        let numlab_id = [PlotElement::NumLab1, PlotElement::NumLab2];
        for axis in 0..2 {
            let a1 = axis + 1;
            let logticks = self.log_ticks(a1)?;
            self.grf_attrs(grf, numlab_id[axis], true, GrfPrimitive::Text)?;
            let info = &grid[axis];
            let mut labellist = Vec::new();
            let upfree = !self.label_up(a1)?;
            let mut d = (BAD_VALUE, BAD_VALUE);
            let mut g;
            let txtgap = self.num_lab_gap(a1)? * mindim;
            let mut used: Vec<f64> = Vec::new();
            let nmajor = info.nmajor() as i64;
            let t0 = nmajor / 2;
            let mut tick = t0 - 1;
            let mut tinc = 1;
            loop {
                tick += tinc;
                if tick < 0 {
                    break;
                }
                if tick == nmajor {
                    tick = t0 - 1;
                    tinc = -1;
                    // C re-tests the loop condition only on the next pass.
                    if tick < 0 {
                        break;
                    }
                }
                let t = tick as usize;
                let mut val = [0.0; 2];
                val[axis] = info.ticks[t];
                val[1 - axis] = labelat[axis];
                let diff = if logticks {
                    info.ticks[t] * (gap[axis] - 1.0)
                } else {
                    gap[axis]
                };
                if used.iter().any(|&u| (val[axis] - u).abs() < 1.0e-3 * diff) {
                    continue;
                }
                used.push(val[axis]);
                g = (BAD_VALUE, BAD_VALUE);
                self.gvec(&val, axis, 0.01 * diff, &mut g, &mut d)?;
                let (mut gx, mut gy) = g;
                let (mut dx, mut dy) = d;
                if dx == BAD_VALUE || dy == BAD_VALUE || gx == BAD_VALUE || gy == BAD_VALUE {
                    continue;
                }
                dx *= f64::from(alpha);
                dy *= f64::from(beta);
                let (rx, ry) = (-dy, dx);
                let rlen = sqrt(rx * rx + ry * ry);
                let offx = rx * txtgap / rlen;
                let offy = ry * txtgap / rlen;
                gx += offx / f64::from(alpha);
                gy += offy / f64::from(beta);
                let just;
                if upfree {
                    let (upx, upy);
                    if dx < -0.01 * f64::from(alpha).abs() {
                        upx = -rx;
                        upy = -ry;
                        just = if txtgap < 0.0 { *b"BC" } else { *b"TC" };
                    } else {
                        upx = rx;
                        upy = ry;
                        just = if txtgap < 0.0 { *b"TC" } else { *b"BC" };
                    }
                    let just = if txtgap == 0.0 { *b"CC" } else { just };
                    self.push_label(&mut labellist, info, t, gx, gy, just, upx, upy, val[axis]);
                } else {
                    let (upx, upy) = (0.0, 1.0);
                    let lim = txtgap.abs() * sin45;
                    let mut j = [
                        if offy > lim {
                            b'B'
                        } else if offy < -lim {
                            b'T'
                        } else {
                            b'C'
                        },
                        if offx < -lim {
                            b'R'
                        } else if offx > lim {
                            b'L'
                        } else {
                            b'C'
                        },
                    ];
                    if txtgap == 0.0 {
                        j = *b"CC";
                    }
                    just = j;
                    self.push_label(&mut labellist, info, t, gx, gy, just, upx, upy, val[axis]);
                }
            }
            if !labellist.is_empty() {
                let fmt = match &info.fmt {
                    Some(f) => f.clone(),
                    None => self.frameset.current().format_str(a1)?,
                };
                self.plot_labels(grf, esc, axis, labellist, &fmt, &mut boxes)?;
            }
            self.grf_attrs(grf, numlab_id[axis], false, GrfPrimitive::Text)?;
        }
        Ok(())
    }

    /// Add an interior label to the list if its position is in the
    /// plotting area.
    #[allow(clippy::too_many_arguments)]
    fn push_label(
        &self,
        list: &mut Vec<EdgeLabel>,
        info: &TickInfo,
        tick: usize,
        gx: f64,
        gy: f64,
        just: [u8; 2],
        upx: f64,
        upy: f64,
        val: f64,
    ) {
        let Some(text) = info.labels.get(tick) else {
            return;
        };
        if gx >= self.xlo && gx <= self.xhi && gy >= self.ylo && gy <= self.yhi {
            list.push(EdgeLabel {
                index: tick as f64,
                x: gx,
                y: gy,
                text: text.clone(),
                just,
                upx: upx as f32,
                upy: upy as f32,
                val,
            });
        }
    }

    /// Draw the axes' descriptive labels and the title.
    // C: TextLabels (plot.c:26541).
    pub(crate) fn text_labels(
        &mut self,
        grf: &mut dyn Grf,
        edgeticks: bool,
        dounits: [bool; 2],
    ) -> AstResult<()> {
        let xrange = self.xhi - self.xlo;
        let yrange = self.yhi - self.ylo;
        let mindim = if xrange < yrange { xrange } else { yrange };
        let bounds = |p: &Plot| -> [f32; 4] {
            if p.text_gap_type() == TextGapType::Plot {
                [p.xlo as f32, p.xhi as f32, p.ylo as f32, p.yhi as f32]
            } else if p.draw.box_lbnd[0] != f32::MAX {
                [
                    p.draw.box_lbnd[0],
                    p.draw.box_ubnd[0],
                    p.draw.box_lbnd[1],
                    p.draw.box_ubnd[1],
                ]
            } else {
                [
                    (p.xlo + 0.2 * xrange) as f32,
                    (p.xhi - 0.2 * xrange) as f32,
                    (p.ylo + 0.2 * yrange) as f32,
                    (p.yhi - 0.2 * yrange) as f32,
                ]
            }
        };
        // C: astMIN and astMAX of a float and a double, which are doubles.
        let mid = |lo: f32, hi: f32, plo: f64, phi: f64| -> f32 {
            let a = if f64::from(hi) < phi {
                f64::from(hi)
            } else {
                phi
            };
            let b = if f64::from(lo) > plo {
                f64::from(lo)
            } else {
                plo
            };
            (0.5 * (a + b)) as f32
        };
        let [xlo, xhi, ylo, yhi] = bounds(self);
        let esc = self.escape();
        let textlab_id = [PlotElement::TextLab1, PlotElement::TextLab2];
        for axis in 0..2 {
            let a1 = axis + 1;
            self.grf_attrs(grf, textlab_id[axis], true, GrfPrimitive::Text)?;
            let draw = if self.test_text_lab(a1)? {
                self.text_lab(a1)?
            } else {
                edgeticks
            };
            if draw {
                let label = self.label(a1)?;
                let mut text = label[..chr_len(&label)].to_string();
                if dounits[axis] {
                    let units = self.unit(a1)?;
                    if !units.is_empty() {
                        text.push_str(" (");
                        text.push_str(&units[..chr_len(&units)]);
                        text.push(')');
                    }
                }
                let txtgap = (mindim * self.text_lab_gap(a1)?) as f32;
                let edge = super::grid::edge_code(self.edge(a1)?);
                let (upx, upy, just, xref, yref) = match edge {
                    0 => (
                        -1.0,
                        0.0,
                        *b"MC",
                        if self.xrev {
                            xhi + txtgap
                        } else {
                            xlo - txtgap
                        },
                        mid(ylo, yhi, self.ylo, self.yhi),
                    ),
                    1 => (
                        0.0,
                        1.0,
                        *b"MC",
                        mid(xlo, xhi, self.xlo, self.xhi),
                        if self.yrev {
                            ylo - txtgap
                        } else {
                            yhi + txtgap
                        },
                    ),
                    2 => (
                        1.0,
                        0.0,
                        *b"MC",
                        if self.xrev {
                            xlo - txtgap
                        } else {
                            xhi + txtgap
                        },
                        mid(ylo, yhi, self.ylo, self.yhi),
                    ),
                    _ => (
                        0.0,
                        1.0,
                        *b"TC",
                        mid(xlo, xhi, self.xlo, self.xhi),
                        if self.yrev {
                            yhi + txtgap
                        } else {
                            ylo - txtgap
                        },
                    ),
                };
                self.render_text_opt(grf, true, esc, &text, xref, yref, just, upx, upy)?;
            }
            self.grf_attrs(grf, textlab_id[axis], false, GrfPrimitive::Text)?;
        }
        if self.draw_title() {
            let title = self.title()?;
            let text = title[..chr_len(&title)].to_string();
            self.grf_attrs(grf, PlotElement::Title, true, GrfPrimitive::Text)?;
            let [xlo, xhi, ylo, yhi] = bounds(self);
            let xref = mid(xlo, xhi, self.xlo, self.xhi);
            let gap = (mindim * self.title_gap()) as f32;
            let yref = if self.yrev { ylo - gap } else { yhi + gap };
            self.render_text_opt(grf, true, esc, &text, xref, yref, *b"MC", 0.0, 1.0)?;
            self.grf_attrs(grf, PlotElement::Title, false, GrfPrimitive::Text)?;
        }
        let d = &mut self.draw;
        if d.box_lbnd[0] != f32::MAX {
            d.box_lbnd[0] = super::draw::fmin(d.box_lbnd[0], d.boxp_lbnd[0]);
            d.box_ubnd[0] = super::draw::fmax(d.box_ubnd[0], d.boxp_ubnd[0]);
            d.box_lbnd[1] = super::draw::fmin(d.box_lbnd[1], d.boxp_lbnd[1]);
            d.box_ubnd[1] = super::draw::fmax(d.box_ubnd[1], d.boxp_ubnd[1]);
        } else {
            d.box_lbnd = d.boxp_lbnd;
            d.box_ubnd = d.boxp_ubnd;
        }
        Ok(())
    }
}
