// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The curve tracer: curves in physical coordinates drawn as polylines in
//! graphics coordinates, subdivided where they bend or break, and clipped
//! to the plotting area.
// C: plot.c Crv (:7607), CrvLine (:8318), AxPlot (:4575), Map1 (:21725),
// the polyline buffer Apoly (:4517), Bpoly (:5458), Opoly (:23201) and Fpoly
// (:13273), PurgeCdata (:24397) and GridLine (:18636).

use std::cmp::Ordering;

use super::Plot;
use super::draw::{fmax, fmin};
use super::elements::PlotElement;
use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::FrameRef;
use crate::frames::{AxisKind, Frame};
use crate::grf::{Grf, GrfPrimitive};
use crate::mappings::helpers::ast_equals;
use crate::mappings::{DynMapping, Mapping};
use crate::math::{log10, powf, sqrt};
use crate::regions::{DynRegion, any_as_region, remove_regions};

/// The number of segments the tracer divides a curve into at each level.
// C: CRV_NSEG.
const CRV_NSEG: usize = 14;
/// The number of points bounding those segments.
// C: CRV_NPNT.
const CRV_NPNT: usize = CRV_NSEG + 1;
/// The deepest the tracer subdivides.
// C: CRV_MXENT.
const CRV_MXENT: i32 = 10;
/// The most breaks a curve may have.
// C: AST__PLOT_CRV_MXBRK (plot.h).
pub(crate) const CRV_MXBRK: usize = 1000;

/// Where a drawn curve breaks: the graphics position of each end of each
/// drawn section, and the unit vector there pointing into the section.
// C: AstPlotCurveData (plot.h), with nbrk the length of the vectors.
#[derive(Debug, Clone, Default)]
pub(crate) struct CurveData {
    /// Whether no part of the curve was inside the plotting area.
    pub(crate) out: bool,
    pub(crate) xbrk: Vec<f32>,
    pub(crate) ybrk: Vec<f32>,
    pub(crate) vxbrk: Vec<f32>,
    pub(crate) vybrk: Vec<f32>,
    /// The drawn length of the curve in graphics coordinates.
    pub(crate) length: f32,
}

impl CurveData {
    fn push(&mut self, x: f32, y: f32, vx: f32, vy: f32) {
        self.xbrk.push(x);
        self.ybrk.push(y);
        self.vxbrk.push(vx);
        self.vybrk.push(vy);
    }

    fn clear_breaks(&mut self) {
        self.xbrk.clear();
        self.ybrk.clear();
        self.vxbrk.clear();
        self.vybrk.clear();
    }

    /// Remove the pairs of breaks that bound a section of zero length.
    // C: PurgeCdata (plot.c:24397).
    fn purge(&mut self) {
        let mut brk = 0;
        while brk < self.xbrk.len() {
            if self.xbrk[brk] == self.xbrk[brk + 1] && self.ybrk[brk] == self.ybrk[brk + 1] {
                for v in [
                    &mut self.xbrk,
                    &mut self.ybrk,
                    &mut self.vxbrk,
                    &mut self.vybrk,
                ] {
                    v.drain(brk..brk + 2);
                }
            } else {
                brk += 2;
            }
        }
    }
}

/// A curve, parametrised by the distance along it from 0 to 1: how to find
/// the graphics position at each distance.
// C: the Crv_map function pointer and the globals each mapping function
// reads (Map1_*).
pub(crate) enum CurveMap {
    /// A curve along one axis of the current Frame, the others held
    /// constant.
    // C: Map1, with Map1_ncoord the length of `origin`.
    Axis {
        /// The physical position at distance 0.
        origin: Vec<f64>,
        /// The change in the axis value from distance 0 to 1.
        length: f64,
        /// The 0-based axis that varies.
        axis: usize,
        /// Whether the axis value varies logarithmically.
        log: bool,
        /// Whether positions that normalisation changes are bad.
        norm: bool,
    },
    /// A straight line in graphics coordinates, from `(x0, y0)` at distance
    /// 0 to `(x0 + dx, y0 + dy)` at 1, bad where the physical position is.
    // C: Map2, with Map2_x0, Map2_y0, Map2_deltax and Map2_deltay.
    Line { x0: f64, y0: f64, dx: f64, dy: f64 },
    /// The boundary of a Region in the current Frame, as the Region traces
    /// it, mapped into graphics coordinates by `map`, the Plot's base to
    /// current Mapping with any Regions in it removed.
    // C: Map5, with Map5_region and Map5_map.
    Region { region: DynRegion, map: DynMapping },
}

/// The state the tracer keeps while drawing one curve.
// C: the Crv_* globals.
pub(crate) struct Crv {
    /// The largest ratio between adjacent segment lengths still drawn
    /// straight.
    scerr: f64,
    /// The unit vector along the last segment drawn, bad after a break.
    ux0: f64,
    uy0: f64,
    /// The largest gap between drawn sections that is not a break.
    tol: f64,
    /// The squared distance below which a curve is short enough to draw
    /// straight.
    limit: f64,
    /// Whether to draw, or only to trace.
    ink: bool,
    /// The plotting area.
    xlo: f64,
    xhi: f64,
    ylo: f64,
    yhi: f64,
    /// Whether to clip to the plotting area.
    clip: bool,
    /// Whether nothing has been drawn yet.
    out: bool,
    /// The end of the last section drawn, and the unit vector along it.
    xl: f64,
    yl: f64,
    vxl: f64,
    vyl: f64,
    /// The drawn length so far.
    len: f32,
    /// The breaks found so far, of which `nbrk` are counted.
    brk: CurveData,
    nbrk: usize,
    /// The depth of recursion.
    nent: i32,
    /// The public method drawing, for error messages.
    method: &'static str,
}

impl Crv {
    /// The tracer state for a curve on `plot`: segments shorter than
    /// `limit_tol` are drawn straight, gaps wider than `gap_tol` are
    /// breaks.
    // C: the Crv_* globals as AxPlot, LinePlot and DrawRegion set them.
    pub(crate) fn new(
        plot: &Plot,
        limit_tol: f64,
        gap_tol: f64,
        ink: bool,
        method: &'static str,
    ) -> AstResult<Self> {
        Ok(Crv {
            scerr: if plot.log_plot(1)? || plot.log_plot(2)? {
                100.0
            } else {
                1.5
            },
            ux0: BAD_VALUE,
            uy0: BAD_VALUE,
            tol: gap_tol,
            limit: 0.5 * limit_tol * limit_tol,
            ink,
            xlo: plot.xlo,
            xhi: plot.xhi,
            ylo: plot.ylo,
            yhi: plot.yhi,
            clip: plot.clip() & 1 != 0,
            out: true,
            xl: 0.0,
            yl: 0.0,
            vxl: 0.0,
            vyl: 0.0,
            len: 0.0,
            brk: CurveData::default(),
            nbrk: 0,
            nent: 0,
            method,
        })
    }
}

/// What `crv`'s recursive calls share.
// C: the CrvStatics structure, of which only these fields outlive the call
// that sets them: limit2 is set on the outermost entry, and all_bad, set
// by each call, is what a caller passes to its next subdivision.
struct CrvShared {
    limit2: f64,
    all_bad: bool,
}

/// C's `astMAX` of two doubles.
fn dmax(a: f64, b: f64) -> f64 {
    if a > b { a } else { b }
}

/// C's `astMIN` of two doubles.
fn dmin(a: f64, b: f64) -> f64 {
    if a < b { a } else { b }
}

/// The order C's Fpoly sorts x coordinates in.
// C: Fpoly_scmp, Fpoly_ecmp.
fn fcmp(a: f32, b: f32) -> Ordering {
    if a < b {
        Ordering::Less
    } else if a > b {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

/// C's `astEQUALS` with a tolerance of 1e8, on floats: the difference is
/// taken in float, the rest in double, as C's macro does with float
/// arguments.
fn feq(a: f32, b: f32) -> bool {
    let tol = (1.0e8 * ((f64::from(a).abs() + f64::from(b).abs()) * f64::EPSILON)).max(1.0e-12);
    f64::from(a - b).abs() <= tol
}

impl Plot {
    /// Whether the 1-based `axis` of `frame` is a sky axis.
    // C: IsASkyAxis (plot.c:20257).
    pub(crate) fn is_a_sky_axis(frame: &dyn Frame, axis: usize) -> bool {
        matches!(frame.get_axis(axis), AxisKind::Sky { .. })
    }

    /// Add a point to the polyline being built, and to the grid's bounding
    /// box.
    // C: Apoly (plot.c:4517).
    pub(crate) fn apoly(&mut self, x: f32, y: f32) {
        self.draw.poly_x.push(x);
        self.draw.poly_y.push(y);
        let d = &mut self.draw;
        d.box_lbnd[0] = fmin(x, d.box_lbnd[0]);
        d.box_ubnd[0] = fmax(x, d.box_ubnd[0]);
        d.box_lbnd[1] = fmin(y, d.box_lbnd[1]);
        d.box_ubnd[1] = fmax(y, d.box_ubnd[1]);
    }

    /// Start a new polyline at a point, unless the point is the end of the
    /// polyline being built, which then continues.
    // C: Bpoly (plot.c:5458).
    pub(crate) fn bpoly(&mut self, x: f32, y: f32) {
        let ignore = match (self.draw.poly_x.last(), self.draw.poly_y.last()) {
            (Some(&lx), Some(&ly)) => feq(lx, x) && feq(ly, y),
            _ => false,
        };
        if !ignore {
            self.opoly();
            self.apoly(x, y);
        }
    }

    /// Finish the polyline being built, keeping it to draw later.
    // C: Opoly (plot.c:23201).
    pub(crate) fn opoly(&mut self) {
        if !self.draw.poly_x.is_empty() {
            let x = std::mem::take(&mut self.draw.poly_x);
            let y = std::mem::take(&mut self.draw.poly_y);
            self.draw.polys.push((x, y));
        }
    }

    /// Draw every polyline kept, joining those whose ends meet.
    // C: Fpoly (plot.c:13273). C marks polyline 0 drawn where it means the
    // first polyline of each joined line (c-library-quirks.md), so a
    // polyline that starts a joined line, other than the first, is found
    // again by the search for one ending where the line has reached, and
    // is drawn a second time, reversed, on the end of itself. C sorts with
    // qsort, whose order of equal values depends on the platform; the port
    // sorts stably.
    pub(crate) fn fpoly(&mut self, grf: &mut dyn Grf) -> AstResult<()> {
        self.opoly();
        let polys = std::mem::take(&mut self.draw.polys);
        let npoly = polys.len();
        if npoly == 1 {
            return self.g_line(grf, &polys[0].0, &polys[0].1);
        }
        if npoly == 0 {
            return Ok(());
        }
        let first = |p: usize| (polys[p].0[0], polys[p].1[0]);
        let last = |p: usize| {
            let n = polys[p].0.len() - 1;
            (polys[p].0[n], polys[p].1[n])
        };
        let mut skey: Vec<usize> = (0..npoly).collect();
        skey.sort_by(|a, b| fcmp(first(*a).0, first(*b).0));
        let mut ekey: Vec<usize> = (0..npoly).collect();
        ekey.sort_by(|a, b| fcmp(last(*a).0, last(*b).0));
        let mut drawn = vec![false; npoly];
        while let Some(ipoly) = (0..npoly).find(|&p| !drawn[p]) {
            // Entries are polyline indices, negated for a polyline joined
            // reversed; polyline 0 reversed is 0, as in C.
            let mut polylist: Vec<isize> = vec![ipoly as isize];
            drawn[0] = true;
            let (mut xt, mut yt) = last(ipoly);
            let mut polylen = polys[ipoly].0.len();
            loop {
                let mut found = None;
                for ipass in 0..2 {
                    let (key, end) = if ipass == 0 {
                        (&skey, &first as &dyn Fn(usize) -> (f32, f32))
                    } else {
                        (&ekey, &last as &dyn Fn(usize) -> (f32, f32))
                    };
                    let mut jpoly: Option<usize>;
                    let (mut ilo, mut ihi) = (0usize, npoly - 1);
                    let mut imid;
                    loop {
                        imid = (ilo + ihi) / 2;
                        jpoly = Some(key[imid]);
                        let xmid = end(key[imid]).0;
                        if feq(xmid, xt) {
                            break;
                        } else if xmid > xt {
                            if ihi == imid {
                                jpoly = Some(key[ilo]);
                                if !feq(end(key[ilo]).0, xt) {
                                    jpoly = None;
                                }
                                break;
                            }
                            ihi = imid;
                        } else {
                            if ilo == imid {
                                jpoly = Some(key[ihi]);
                                if !feq(end(key[ihi]).0, xt) {
                                    jpoly = None;
                                }
                                break;
                            }
                            ilo = imid;
                        }
                    }
                    let Some(j) = jpoly else {
                        continue;
                    };
                    if feq(end(j).1, yt) && !drawn[j] {
                        found = Some((ipass, j));
                        break;
                    }
                    let matches = |k: usize| {
                        let (xm, ym) = end(key[k]);
                        if feq(xm, xt) {
                            Some(feq(ym, yt) && !drawn[key[k]])
                        } else {
                            None
                        }
                    };
                    let mut hit = None;
                    for k in (0..imid).rev() {
                        match matches(k) {
                            Some(true) => {
                                hit = Some(key[k]);
                                break;
                            }
                            Some(false) => {}
                            None => break,
                        }
                    }
                    if hit.is_none() {
                        #[allow(clippy::needless_range_loop)]
                        for k in imid + 1..npoly {
                            match matches(k) {
                                Some(true) => {
                                    hit = Some(key[k]);
                                    break;
                                }
                                Some(false) => {}
                                None => break,
                            }
                        }
                    }
                    if let Some(j) = hit {
                        found = Some((ipass, j));
                        break;
                    }
                }
                let Some((ipass, jpoly)) = found else {
                    break;
                };
                let ipoint = polys[jpoly].0.len() - 1;
                if ipass == 0 {
                    polylist.push(jpoly as isize);
                    (xt, yt) = last(jpoly);
                } else {
                    polylist.push(-(jpoly as isize));
                    (xt, yt) = first(jpoly);
                }
                polylen += ipoint;
                drawn[jpoly] = true;
            }
            if polylist.len() == 1 {
                let j = polylist[0] as usize;
                self.g_line(grf, &polys[j].0, &polys[j].1)?;
            } else {
                let mut xnew = Vec::with_capacity(polylen);
                let mut ynew = Vec::with_capacity(polylen);
                for (k, &entry) in polylist.iter().enumerate() {
                    if entry >= 0 {
                        let (px, py) = &polys[entry as usize];
                        let skip = usize::from(k != 0);
                        xnew.extend_from_slice(&px[skip..]);
                        ynew.extend_from_slice(&py[skip..]);
                    } else {
                        let (px, py) = &polys[(-entry) as usize];
                        let np = if k == 0 { px.len() } else { px.len() - 1 };
                        xnew.extend(px[..np].iter().rev());
                        ynew.extend(py[..np].iter().rev());
                    }
                }
                self.g_line(grf, &xnew, &ynew)?;
            }
        }
        Ok(())
    }

    /// The graphics positions at distances `d` along a curve.
    // C: Map1 (plot.c:21725).
    fn curve_map(&self, map: &CurveMap, d: &[f64], x: &mut [f64], y: &mut [f64]) -> AstResult<()> {
        match map {
            CurveMap::Axis {
                origin,
                length,
                axis,
                log,
                norm,
            } => {
                let n = d.len();
                let ncoord = origin.len();
                let (axorig, axscale, neg) = if *log {
                    let neg = origin[*axis] < 0.0;
                    let axorig = log10(origin[*axis].abs());
                    (axorig, log10((origin[*axis] + length).abs()) - axorig, neg)
                } else {
                    (origin[*axis], *length, false)
                };
                let mut phys = PointSet::zeros(n, ncoord);
                for (j, &v) in origin.iter().enumerate() {
                    phys.axis_mut(j).fill(v);
                }
                for (p, &di) in phys.axis_mut(*axis).iter_mut().zip(d) {
                    *p = axorig + axscale * di;
                    if *log {
                        *p = if neg { -powf(10.0, *p) } else { powf(10.0, *p) };
                    }
                }
                let graph = self.trans(&phys, false)?;
                x.copy_from_slice(graph.axis(0));
                y.copy_from_slice(graph.axis(1));
                if *norm {
                    let frame = self.frameset.current();
                    for i in 0..n {
                        let orig = phys.point(i).to_vec();
                        let mut work = orig.clone();
                        frame.norm(&mut work)?;
                        if work
                            .iter()
                            .zip(&orig)
                            .any(|(&w, &o)| !ast_equals(w, o, 1.0e8))
                        {
                            x[i] = BAD_VALUE;
                            y[i] = BAD_VALUE;
                        }
                    }
                }
                Ok(())
            }
            // C: Map2 (plot.c:21975).
            CurveMap::Line {
                x0,
                y0,
                dx: deltax,
                dy: deltay,
            } => {
                for (i, &di) in d.iter().enumerate() {
                    x[i] = x0 + deltax * di;
                    y[i] = y0 + deltay * di;
                }
                let graph = PointSet::from_columns(&[x, y])?;
                let phys = self.trans_with(None, None, &graph, true, false)?;
                for j in 0..phys.ncoord() {
                    for (i, &p) in phys.axis(j).iter().enumerate() {
                        if p == BAD_VALUE {
                            x[i] = BAD_VALUE;
                            y[i] = BAD_VALUE;
                        }
                    }
                }
                Ok(())
            }
            // C: Map5 (plot.c:22536). A part of a split Region that cannot
            // be traced, which C reads from an array nothing filled, is bad.
            CurveMap::Region { region, map } => {
                let phys = region.reg_trace(d)?.unwrap_or_else(|| {
                    let mut bad = PointSet::zeros(d.len(), region.naxes());
                    for j in 0..bad.ncoord() {
                        bad.axis_mut(j).fill(BAD_VALUE);
                    }
                    bad
                });
                let graph = self.trans_with(Some(map.as_ref()), None, &phys, false, true)?;
                x.copy_from_slice(graph.axis(0));
                y.copy_from_slice(graph.axis(1));
                Ok(())
            }
        }
    }

    /// Draw the curve through the graphics positions `(x[i], y[i])` at the
    /// distances `d[i]` along it, drawing each segment straight where the
    /// curve is smooth enough there, and subdividing the others.
    // C: Crv (plot.c:7607). The CRV_TRACE and CRV_MARK debugging blocks are
    // not ported.
    #[allow(clippy::too_many_arguments)]
    fn crv(
        &mut self,
        map: &CurveMap,
        crv: &mut Crv,
        shared: &mut CrvShared,
        d: &[f64],
        x: &[f64],
        y: &[f64],
        skipbad: bool,
        bounds: Option<&[f64; 4]>,
    ) -> AstResult<()> {
        if crv.nent == 0 {
            shared.limit2 = 20.0 * crv.limit / (CRV_NSEG * CRV_NSEG) as f64;
        }
        crv.nent += 1;

        let (mut last_ok, mut last_x, mut last_y);
        if x[0] != BAD_VALUE && y[0] != BAD_VALUE {
            last_ok = true;
            last_x = x[0];
            last_y = y[0];
            shared.all_bad =
                (x[0] < crv.xlo || x[0] > crv.xhi || y[0] < crv.ylo || y[0] > crv.yhi) && crv.clip;
        } else {
            last_ok = false;
            last_x = 0.0;
            last_y = 0.0;
            shared.all_bad = true;
        }
        let mut bbox = [f64::MAX, -f64::MAX, f64::MAX, -f64::MAX];
        let mut dx = [0.0; CRV_NSEG];
        let mut dy = [0.0; CRV_NSEG];
        let mut dl2 = [0.0; CRV_NSEG];
        let (mut nlong, mut nshort) = (0, 0);
        let mut totlen = 0.0;
        for i in 0..CRV_NSEG {
            let (px, py) = (x[i + 1], y[i + 1]);
            if px != BAD_VALUE && py != BAD_VALUE {
                if px < bbox[0] {
                    bbox[0] = px;
                }
                if px > bbox[1] {
                    bbox[1] = px;
                }
                if py < bbox[2] {
                    bbox[2] = py;
                }
                if py > bbox[3] {
                    bbox[3] = py;
                }
                if !crv.clip || (px >= crv.xlo && px <= crv.xhi && py >= crv.ylo && py <= crv.yhi) {
                    shared.all_bad = false;
                }
                if last_ok {
                    let t1 = px - last_x;
                    let t2 = py - last_y;
                    let t3 = t1 * t1 + t2 * t2;
                    dx[i] = t1;
                    dy[i] = t2;
                    dl2[i] = t3;
                    if totlen != BAD_VALUE {
                        totlen += sqrt(t3);
                    }
                    if t3 > shared.limit2 {
                        nlong += 1;
                    } else {
                        nshort += 1;
                    }
                } else {
                    dx[i] = BAD_VALUE;
                    dy[i] = BAD_VALUE;
                    dl2[i] = BAD_VALUE;
                    totlen = BAD_VALUE;
                }
                last_ok = true;
                last_x = px;
                last_y = py;
            } else {
                dx[i] = BAD_VALUE;
                dy[i] = BAD_VALUE;
                dl2[i] = BAD_VALUE;
                totlen = BAD_VALUE;
                last_ok = false;
            }
        }

        // Which segments can be drawn as single straight lines.
        let mut vxl = crv.ux0;
        let mut vyl = crv.uy0;
        let mut dll = BAD_VALUE;
        if totlen != BAD_VALUE {
            totlen /= CRV_NSEG as f64;
            totlen *= totlen;
        }
        let mut seg_ok = [false; CRV_NSEG];
        for i in 0..CRV_NSEG {
            if dl2[i] != BAD_VALUE && dl2[i] > 0.0 {
                let dl = sqrt(dl2[i]);
                let vx = dx[i] / dl;
                let vy = dy[i] / dl;
                if totlen != BAD_VALUE && totlen < crv.limit {
                    seg_ok[i] = true;
                } else if vxl != BAD_VALUE {
                    let cosang = vxl * vx + vyl * vy;
                    // C's two tests set the same flags, kept apart as C has them.
                    #[allow(clippy::if_same_then_else)]
                    if cosang < 0.8 || dl2[i] * (1.0 - cosang * cosang) > crv.limit {
                        seg_ok[i] = false;
                        if i > 0 {
                            seg_ok[i - 1] = false;
                        }
                    } else if dll != BAD_VALUE && (dl < dll / crv.scerr || dl > dll * crv.scerr) {
                        seg_ok[i] = false;
                        if i > 0 {
                            seg_ok[i - 1] = false;
                        }
                    } else {
                        seg_ok[i] = true;
                    }
                } else {
                    seg_ok[i] = true;
                }
                vxl = vx;
                vyl = vy;
                dll = dl;
            } else {
                seg_ok[i] = false;
                vxl = BAD_VALUE;
                vyl = BAD_VALUE;
                dll = BAD_VALUE;
            }
        }
        // No isolated segment is drawn straight.
        if !seg_ok[1] {
            seg_ok[0] = false;
        }
        for i in 1..CRV_NSEG - 1 {
            if !seg_ok[i - 1] && !seg_ok[i + 1] {
                seg_ok[i] = false;
            }
        }
        if !seg_ok[CRV_NSEG - 2] {
            seg_ok[CRV_NSEG - 1] = false;
        }

        // Draw.
        let delta = (d[CRV_NSEG] - d[0]) / (CRV_NSEG * CRV_NSEG) as f64;
        let mut subdivide = crv.nent < CRV_MXENT && (!shared.all_bad || !skipbad) && nlong > nshort;
        if let Some(b) = bounds
            && bbox[0] != f64::MAX
            && bbox[1] - bbox[0] > 0.9 * (b[1] - b[0])
            && bbox[3] - bbox[2] > 0.9 * (b[3] - b[2])
        {
            subdivide = false;
        }
        let mut dd = Vec::new();
        let mut xx = Vec::new();
        let mut yy = Vec::new();
        if subdivide {
            for i in 0..CRV_NSEG {
                if !seg_ok[i] {
                    let mut d0 = d[i];
                    for _ in 0..=CRV_NSEG {
                        dd.push(d0);
                        d0 += delta;
                    }
                }
            }
            if !dd.is_empty() {
                xx = vec![0.0; dd.len()];
                yy = vec![0.0; dd.len()];
                self.curve_map(map, &dd, &mut xx, &mut yy)?;
            }
        }
        let mut k = 0;
        for i in 0..CRV_NSEG {
            if seg_ok[i] {
                self.crv_line(crv, x[i], y[i], x[i + 1], y[i + 1])?;
                let dl = sqrt(dl2[i]);
                crv.ux0 = dx[i] / dl;
                crv.uy0 = dy[i] / dl;
            } else if subdivide {
                let skip = shared.all_bad;
                let range = k..k + CRV_NPNT;
                self.crv(
                    map,
                    crv,
                    shared,
                    &dd[range.clone()],
                    &xx[range.clone()],
                    &yy[range],
                    skip,
                    Some(&bbox),
                )?;
                k += CRV_NPNT;
            } else {
                crv.ux0 = BAD_VALUE;
                crv.uy0 = BAD_VALUE;
            }
        }
        crv.nent -= 1;
        Ok(())
    }

    /// Draw the part of the straight line from `(xa, ya)` to `(xb, yb)`
    /// inside the plotting area, recording breaks where the curve starts
    /// again after a gap.
    // C: CrvLine (plot.c:8318).
    fn crv_line(&mut self, crv: &mut Crv, xa: f64, ya: f64, xb: f64, yb: f64) -> AstResult<()> {
        let mut dx = xb - xa;
        let mut dy = yb - ya;
        if dx == 0.0 && dy == 0.0 {
            return Ok(());
        }
        let (alpha, beta) = (f64::from(self.draw.alpha), f64::from(self.draw.beta));
        let (xam, yam, xbm, ybm, mut dl);
        if crv.clip
            && (xa < crv.xlo
                || xa > crv.xhi
                || xb < crv.xlo
                || xb > crv.xhi
                || ya < crv.ylo
                || ya > crv.yhi
                || yb < crv.ylo
                || yb > crv.yhi)
        {
            let (a1, a2) = if dx != 0.0 {
                let a1 = (xb - crv.xlo) / dx;
                let a2 = (xb - crv.xhi) / dx;
                if a1 < a2 { (a2, a1) } else { (a1, a2) }
            } else if (xa > crv.xlo || ast_equals(xa, crv.xlo, 1.0e8))
                && (xa < crv.xhi || ast_equals(xa, crv.xhi, 1.0e8))
            {
                (f64::MAX, -f64::MAX)
            } else {
                (0.0, 0.0)
            };
            let (a3, a4) = if dy != 0.0 {
                let a3 = (yb - crv.ylo) / dy;
                let a4 = (yb - crv.yhi) / dy;
                if a3 < a4 { (a4, a3) } else { (a3, a4) }
            } else if (ya > crv.ylo || ast_equals(ya, crv.ylo, 1.0e8))
                && (ya < crv.yhi || ast_equals(ya, crv.yhi, 1.0e8))
            {
                (f64::MAX, -f64::MAX)
            } else {
                (0.0, 0.0)
            };
            let aamin = dmin(1.0, dmax(0.0, dmax(a2, a4)));
            let aamax = dmax(0.0, dmin(1.0, dmin(a1, a3)));
            if aamax > aamin {
                xam = xb - aamax * dx;
                yam = yb - aamax * dy;
                xbm = xb - aamin * dx;
                ybm = yb - aamin * dy;
                dx *= alpha;
                dy *= beta;
                dl = sqrt(dx * dx + dy * dy);
                dx /= dl;
                dy /= dl;
                dl *= dmax(0.0, aamax - aamin);
            } else {
                return Ok(());
            }
        } else {
            xam = xa;
            yam = ya;
            xbm = xb;
            ybm = yb;
            dx *= alpha;
            dy *= beta;
            dl = sqrt(dx * dx + dy * dy);
            dx /= dl;
            dy /= dl;
        }
        if crv.out {
            crv.nbrk = 1;
            crv.brk.push(xam as f32, yam as f32, dx as f32, dy as f32);
            crv.out = false;
            crv.len = dl as f32;
            if crv.ink {
                self.bpoly(xam as f32, yam as f32);
            }
        } else {
            crv.len += dl as f32;
            if (xam - crv.xl).abs() > crv.tol || (yam - crv.yl).abs() > crv.tol {
                crv.nbrk += 2;
                if crv.nbrk > CRV_MXBRK {
                    return Err(AstError::new(
                        ErrorCode::Cvbrk,
                        format!(
                            "{}(Plot): Number of breaks in plotted curve exceeds {CRV_MXBRK}.",
                            crv.method
                        ),
                    ));
                }
                crv.brk.push(
                    crv.xl as f32,
                    crv.yl as f32,
                    -crv.vxl as f32,
                    -crv.vyl as f32,
                );
                crv.brk.push(xam as f32, yam as f32, dx as f32, dy as f32);
                if crv.ink {
                    self.bpoly(xam as f32, yam as f32);
                }
            }
        }
        if crv.ink {
            self.apoly(xbm as f32, ybm as f32);
        }
        crv.xl = xbm;
        crv.yl = ybm;
        crv.vxl = dx;
        crv.vyl = dy;
        Ok(())
    }

    /// Trace, and draw if `ink`, the curve along the 0-based `axis` of the
    /// current Frame from `start` for `length`, in the style of the other
    /// axis's grid lines, returning where it breaks.
    // C: AxPlot (plot.c:4575).
    pub(crate) fn ax_plot(
        &mut self,
        grf: &mut dyn Grf,
        axis: usize,
        start: &[f64],
        length: f64,
        ink: bool,
        method: &'static str,
    ) -> AstResult<CurveData> {
        let naxes = Mapping::nout(self);
        if start[..naxes].contains(&BAD_VALUE) || length == BAD_VALUE {
            return Ok(CurveData {
                out: true,
                ..CurveData::default()
            });
        }
        let gridid = if axis == 0 {
            PlotElement::Grid2
        } else {
            PlotElement::Grid1
        };
        self.grf_attrs(grf, gridid, true, GrfPrimitive::Line)?;
        self.g_scales(grf)?;
        let frame = self.frameset.current();
        let norm = !Self::is_a_sky_axis(frame, 1) && !Self::is_a_sky_axis(frame, 2);
        let map = CurveMap::Axis {
            origin: start[..naxes].to_vec(),
            length,
            axis,
            log: self.log_ticks(axis + 1)?,
            norm,
        };
        let tol = self.tol() * dmax(self.xhi - self.xlo, self.yhi - self.ylo);
        let mut crv = Crv::new(self, tol, tol, ink, method)?;
        let cdata = self.trace(&map, &mut crv)?;
        self.grf_attrs(grf, gridid, false, GrfPrimitive::Line)?;
        Ok(cdata)
    }
    /// Trace a curve with the tracer state `crv` from distance 0 to 1, and
    /// end the polyline.
    // C: Crv over CRV_NPNT points and Opoly, which DrawRegion (plot.c:9583)
    // does for each part of a Region without the rest of `trace`.
    fn trace_curve(&mut self, map: &CurveMap, crv: &mut Crv) -> AstResult<()> {
        let d: Vec<f64> = (0..CRV_NPNT).map(|i| i as f64 / CRV_NSEG as f64).collect();
        let mut x = vec![0.0; CRV_NPNT];
        let mut y = vec![0.0; CRV_NPNT];
        self.curve_map(map, &d, &mut x, &mut y)?;
        let mut shared = CrvShared {
            limit2: 0.0,
            all_bad: false,
        };
        self.crv(map, crv, &mut shared, &d, &x, &y, false, None)?;
        self.opoly();
        Ok(())
    }

    /// Trace the outline of the current Frame if it is a Region whose class
    /// can trace its boundary, each part of it that does not overlap the
    /// others in turn, returning whether it was traced.
    // C: DrawRegion (plot.c:9583). One tracer state serves every part, as
    // C sets the Crv globals once before its loop.
    pub(crate) fn draw_region(
        &mut self,
        grf: &mut dyn Grf,
        method: &'static str,
    ) -> AstResult<bool> {
        let Some(region) = any_as_region(self.frameset.current().as_any()) else {
            return Ok(false);
        };
        if region.reg_trace(&[])?.is_none() {
            return Ok(false);
        }
        let region = region.copy_region();
        let map = remove_regions(
            self.frameset
                .mapping(FrameRef::Base, FrameRef::Current)?
                .as_ref(),
        );
        // A tenth of the usual tolerance, since a Region's edges, a
        // Polygon's especially, can be crinkly.
        let tol = 0.1 * self.tol() * dmax(self.xhi - self.xlo, self.yhi - self.ylo);
        self.g_scales(grf)?;
        let mut crv = Crv::new(self, tol, 2.0 * tol, true, method)?;
        for part in region.reg_split()? {
            let curve = CurveMap::Region {
                region: part,
                map: map.clone(),
            };
            self.trace_curve(&curve, &mut crv)?;
        }
        Ok(true)
    }

    /// Trace a curve with the tracer state `crv`, from distance 0 to 1,
    /// returning where it breaks.
    // C: the part AxPlot (plot.c:4575), LinePlot (:21288) and DrawRegion
    // (:9583) share: Crv over CRV_NPNT points, Opoly, the final break and
    // PurgeCdata.
    pub(crate) fn trace(&mut self, map: &CurveMap, crv: &mut Crv) -> AstResult<CurveData> {
        self.trace_curve(map, crv)?;
        if crv.out {
            crv.nbrk = 0;
            crv.len = 0.0;
            crv.brk.clear_breaks();
        } else {
            crv.nbrk += 1;
            if crv.nbrk > CRV_MXBRK {
                return Err(AstError::new(
                    ErrorCode::Cvbrk,
                    format!(
                        "{}(Plot): Number of breaks in curve exceeds {CRV_MXBRK}.",
                        crv.method
                    ),
                ));
            }
            crv.brk.push(
                crv.xl as f32,
                crv.yl as f32,
                -crv.vxl as f32,
                -crv.vyl as f32,
            );
        }
        let mut cdata = std::mem::take(&mut crv.brk);
        cdata.length = crv.len;
        cdata.out = crv.out;
        cdata.purge();
        Ok(cdata)
    }

    /// Trace, and draw if `ink`, the straight line in graphics coordinates
    /// from `(xa, ya)` to `(xb, yb)` where its physical positions are good,
    /// returning where it breaks.
    // C: LinePlot (plot.c:21288).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn line_plot(
        &mut self,
        grf: &mut dyn Grf,
        xa: f64,
        ya: f64,
        xb: f64,
        yb: f64,
        ink: bool,
        method: &'static str,
    ) -> AstResult<CurveData> {
        if xa == BAD_VALUE || ya == BAD_VALUE || xb == BAD_VALUE || yb == BAD_VALUE {
            return Ok(CurveData::default());
        }
        let tol = self.tol() * dmax(self.xhi - self.xlo, self.yhi - self.ylo);
        self.g_scales(grf)?;
        let mut crv = Crv::new(self, tol, tol, ink, method)?;
        let map = CurveMap::Line {
            x0: xa,
            y0: ya,
            dx: xb - xa,
            dy: yb - ya,
        };
        self.trace(&map, &mut crv)
    }

    // C: GridLine (plot.c:18636).
    pub(crate) fn grid_line_impl(
        &mut self,
        grf: &mut dyn Grf,
        axis: usize,
        start: &[f64],
        length: f64,
    ) -> AstResult<()> {
        let naxes = Mapping::nin(self);
        if naxes != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "astGridLine(Plot): Number of axes ({naxes}) in the base Frame of the \
                     supplied Plot is invalid - this number should be 2."
                ),
            ));
        }
        self.reset_boxp();
        let nout = Mapping::nout(self);
        if axis < 1 || axis > nout {
            return Err(AstError::new(
                ErrorCode::Axiin,
                format!(
                    "astGridLine(Plot): Axis index ({axis}) invalid - it should be in the range \
                     1 to {nout}."
                ),
            ));
        }
        if start.len() < nout {
            return Err(AstError::new(
                ErrorCode::Ncpin,
                format!(
                    "astGridLine(Plot): {} values given for the start of the curve, but the \
                     current Frame has {nout} axes.",
                    start.len()
                ),
            ));
        }
        self.reset_grf();
        self.draw.curve_data = self.ax_plot(grf, axis - 1, start, length, true, "astGridLine")?;
        self.fpoly(grf)
    }
}
