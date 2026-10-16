//! Polygon region: a closed area defined by ordered vertices.
//!
//! A [`Polygon`] represents a closed polygonal area within a 2-D
//! coordinate frame. It is defined by an ordered list of vertices.
//! Point membership is determined by the crossing-number (ray-casting)
//! algorithm: a ray cast from the test point in the +x direction, and
//! the number of polygon edge crossings determines inside/outside.
//! Odd crossings = inside, even = outside.
//!
//! The polygon is restricted to 2-dimensional frames (naxes must be 2).

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::{AxisKind, DynFrame, Frame, FrameRef, FrameSet, simplify_frame_via_mapping};
use crate::mappings::helpers::{is_bad, point_has_bad};
use crate::mappings::{DynMapping, Mapping, UnitMap, ViaSimplify};

use super::data::{RegionData, RegionGeometry};
use super::{DynRegion, Region};
use crate::constants::BAD_VALUE;
use crate::frames::{LineDef, SkyFrame};

/// The width of the polygon perpendicular to edge `i`, and the point half
/// way across.
///
/// Faithful port of C `Polywidth` (polygon.c:4047). Builds a line
/// perpendicular to edge `i` through its mid-point, extending towards the
/// polygon interior, and returns the distance to the nearest other edge
/// (`AST__BAD` if none is hit within its length) together with the position
/// half that distance along the perpendicular. When no width is found the
/// returned point is the far end of the perpendicular probe line (a
/// representative "inside" point for a clockwise/negated polygon).
fn polywidth(
    frm: &dyn Frame,
    edges: &[LineDef],
    i: usize,
    nv: usize,
) -> AstResult<(f64, [f64; 2])> {
    let mut result = BAD_VALUE;

    // Move to the mid point of edge `i`: the start of the perpendicular line.
    let l1 = 0.5 * edges[i].length;
    let start = frm.line_offset(&edges[i], l1, 0.0)?;

    // Move away at right angles. Start at 10*l1; for spherical Frames this may
    // wrap around, so shrink until the end is actually the requested distance
    // from the start.
    let mut l2 = 10.0 * l1;
    let mut end;
    loop {
        end = frm.line_offset(&edges[i], l1, l2)?;
        let d = frm.distance(&start, &end)?;
        if !is_bad(d) && (d - l2).abs() < 1.0e-6 * l2 {
            break;
        }
        l2 *= 0.5;
    }

    // Describe the perpendicular line, marked infinite so any crossing counts.
    let mut line = frm
        .line_def(&start, &end)?
        .ok_or_else(|| AstError::new(ErrorCode::Inter, "polywidth: bad perpendicular line"))?;
    line.infinite = true;

    // Nearest crossing with any other edge.
    for (j, edge) in edges.iter().enumerate().take(nv) {
        if j == i {
            continue;
        }
        let (crossed, _cross, d) = frm.line_crossing(&line, edge)?;
        if crossed && !is_bad(d) && (d < result || is_bad(result)) {
            result = d;
        }
    }

    let cen = if !is_bad(result) {
        let half = frm.offset(&start, &end, 0.5 * result)?;
        [half[0], half[1]]
    } else {
        end
    };

    Ok((result, cen))
}

/// The test `Polygon::convex` applies to each pixel value: a pixel is selected
/// when its value compares with the supplied value in this way (polygon.c
/// `ISVALID`, and the `AST__LT` ... `AST__NE` codes of polygon.h and
/// timeframe.h).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelOper {
    /// Pixel value less than the supplied value.
    Lt,
    /// Pixel value less than or equal to the supplied value.
    Le,
    /// Pixel value equal to the supplied value.
    Eq,
    /// Pixel value not equal to the supplied value.
    Ne,
    /// Pixel value greater than or equal to the supplied value.
    Ge,
    /// Pixel value greater than the supplied value.
    Gt,
}

impl PixelOper {
    fn selects<T: PartialOrd>(self, v: T, value: T) -> bool {
        match self {
            PixelOper::Lt => v < value,
            PixelOper::Le => v <= value,
            PixelOper::Eq => v == value,
            PixelOper::Ne => v != value,
            PixelOper::Ge => v >= value,
            PixelOper::Gt => v > value,
        }
    }
}

pub use crate::pixel::PixelValue;

/// A 2-D array, row by row with X varying fastest, with pixel indices `lbnd`
/// to `ubnd` inclusive, and the test that selects its pixels. `ConvexHull`
/// addresses pixels by 1-based GRID indices, `Outline` by pixel indices.
struct PixelSelection<'a, T> {
    value: T,
    oper: PixelOper,
    array: &'a [T],
    lbnd: [i64; 2],
    ubnd: [i64; 2],
    xdim: i64,
    ydim: i64,
}

impl<'a, T: PixelValue> PixelSelection<'a, T> {
    /// The selection, or [`ErrorCode::Dimin`] naming C function `fname` if
    /// the bounds are empty or `array` does not cover them; C reads past the
    /// array in those cases.
    fn new(
        value: T,
        oper: PixelOper,
        array: &'a [T],
        lbnd: [i64; 2],
        ubnd: [i64; 2],
        fname: &str,
    ) -> AstResult<Self> {
        let xdim = ubnd[0] - lbnd[0] + 1;
        let ydim = ubnd[1] - lbnd[1] + 1;
        if xdim < 1 || ydim < 1 || (array.len() as i64) < xdim * ydim {
            return Err(AstError::new(
                ErrorCode::Dimin,
                format!(
                    "{fname}{}: the array has {} elements, which does not cover \
                     pixel bounds ({}:{},{}:{}).",
                    T::CODE,
                    array.len(),
                    lbnd[0],
                    ubnd[0],
                    lbnd[1],
                    ubnd[1]
                ),
            ));
        }
        Ok(PixelSelection {
            value,
            oper,
            array,
            lbnd,
            ubnd,
            xdim,
            ydim,
        })
    }

    fn selected(&self, index: i64) -> bool {
        self.oper.selects(self.array[index as usize], self.value)
    }

    /// polygon.c `FindBoxEdge`: the lower (`low`) or upper edge, on GRID axis
    /// `axis` (0 or 1), of the bounding box of the selected pixels, and the
    /// highest and lowest GRID index on the other axis of the selected pixels
    /// on that edge, as `(val, valmax, valmin)`. `val` is zero when no pixel
    /// is selected.
    fn find_box_edge(&self, axis: usize, low: bool) -> (i64, i64, i64) {
        let (astep, bstep, alen, blen) = if axis == 1 {
            (self.xdim, 1, self.ydim, self.xdim)
        } else {
            (1, self.xdim, self.xdim, self.ydim)
        };
        let (a0, a1, inc) = if low { (1, alen, 1) } else { (alen, 1, -1) };
        let (b0, b1) = (1, blen);

        let (mut val, mut valmax, mut valmin) = (0, 0, 0);
        let mut a = a0;
        loop {
            let mut pc = (a - 1) * astep + (b0 - 1) * bstep;
            for b in b0..=b1 {
                if self.selected(pc) {
                    if valmin == 0 {
                        valmin = b;
                    }
                    valmax = b;
                }
                pc += bstep;
            }
            if valmax != 0 {
                val = a;
                break;
            }
            if a != a1 {
                a += inc;
            } else {
                break;
            }
        }
        (val, valmax, valmin)
    }

    /// polygon.c `PartHull`: the vertices of the convex hull of the selected
    /// pixels on the far side of the line from GRID pixel (xs,ys) to (xe,ye),
    /// as PIXEL coordinates. With `starpix` pixel centres are at half-integer
    /// PIXEL coordinates, otherwise at integers.
    fn part_hull(
        &self,
        (xs, ys): (i64, i64),
        (xe, ye): (i64, i64),
        starpix: bool,
        lbnd: [i64; 2],
    ) -> AstResult<Vec<[f64; 2]>> {
        if xs == xe && ys == ye {
            return Ok(vec![if starpix {
                [(xs + lbnd[0]) as f64 - 1.5, (ys + lbnd[1]) as f64 - 1.5]
            } else {
                [(xs + lbnd[0]) as f64 - 1.0, (ys + lbnd[1]) as f64 - 1.0]
            }]);
        }
        if xs == xe {
            return Err(AstError::new(
                ErrorCode::Inter,
                "astOutline(Polygon): Bounding box has zero width (internal AST \
                 programming error).",
            ));
        } else if ys == ye {
            return Err(AstError::new(
                ErrorCode::Inter,
                "astOutline(Polygon): Bounding box has zero height (internal AST \
                 programming error).",
            ));
        }

        let xdelta = (xe - xs) as f64 / (ye - ys) as f64;
        let (xl, xr) = if xe > xs { (xs, xe) } else { (xe, xs) };
        let yinc = if ye > ys { 1 } else { -1 };

        let mut hull: Vec<[f64; 2]> = Vec::new();
        let mut iy = ys;
        loop {
            // The GRID X where the line crosses this row; the cast truncates,
            // as C's cast to AstDim does.
            let xlim = (0.5 + xs as f64 + xdelta * (iy - ys) as f64) as i64;
            let (x0, x1) = if yinc < 0 { (xl, xlim) } else { (xlim, xr) };

            let row = (iy - 1) * self.xdim - 1;
            for ix in x0..=x1 {
                if self.selected(row + ix) {
                    let p = [ix as f64, iy as f64];
                    // Remove hull vertices until the new pixel is to the left
                    // of the line through the last two, then add it.
                    loop {
                        let n = hull.len();
                        if n < 2 {
                            hull.push(p);
                            break;
                        }
                        let dx1 = hull[n - 1][0] - hull[n - 2][0];
                        let dy1 = hull[n - 1][1] - hull[n - 2][1];
                        let dx2 = p[0] - hull[n - 2][0];
                        let dy2 = p[1] - hull[n - 2][1];
                        if dx1 * dy2 > dx2 * dy1 {
                            hull.push(p);
                            break;
                        }
                        hull.pop();
                    }
                }
            }

            if iy == ye {
                break;
            }
            iy += yinc;
        }

        // GRID to PIXEL.
        let xoff = if starpix {
            lbnd[0] as f64 - 1.5
        } else {
            lbnd[0] as f64 - 1.0
        };
        let yoff = if starpix {
            lbnd[1] as f64 - 1.5
        } else {
            lbnd[1] as f64 - 1.0
        };
        for v in &mut hull {
            v[0] += xoff;
            v[1] += yoff;
        }
        Ok(hull)
    }

    /// polygon.c `ConvexHull`: the convex hull of the selected pixels, as the
    /// hulls of the four corners of their bounding box, bottom right, top
    /// right, top left and bottom left, or `None` when no pixel is selected.
    fn convex_hull(&self, starpix: bool) -> AstResult<Option<Vec<[f64; 2]>>> {
        let lbnd = self.lbnd;
        let (ylo, yloxmax, yloxmin) = self.find_box_edge(1, true);
        if ylo <= 0 {
            return Ok(None);
        }
        let (yhi, yhixmax, yhixmin) = self.find_box_edge(1, false);
        let (xlo, xloymax, xloymin) = self.find_box_edge(0, true);
        let (xhi, xhiymax, xhiymin) = self.find_box_edge(0, false);

        let mut hull = self.part_hull((yloxmax, ylo), (xhi, xhiymin), starpix, lbnd)?;
        hull.extend(self.part_hull((xhi, xhiymax), (yhixmax, yhi), starpix, lbnd)?);
        hull.extend(self.part_hull((yhixmin, yhi), (xlo, xloymax), starpix, lbnd)?);
        hull.extend(self.part_hull((xlo, xloymin), (yloxmin, ylo), starpix, lbnd)?);
        Ok(Some(hull))
    }
}

/// The perturbation, in pixels, that `TraceEdge` moves each outline vertex
/// towards the inside of the outline, off the pixel corner (polygon.c
/// `DELTA`).
const TRACE_DELTA: f64 = 0.01;

impl<T: PixelValue> PixelSelection<'_, T> {
    /// polygon.c `FindInsidePoint`: the pixel indices and vector index of the
    /// central pixel if it is selected, or else of the first selected pixel
    /// met by a spiral search out from it, each ring searched anti-clockwise
    /// from the middle of its right edge. The search stops at the largest
    /// ring that fits in the array.
    fn find_inside_point(&self) -> AstResult<(i64, i64, i64)> {
        let (lbnd, ubnd, nx) = (self.lbnd, self.ubnd, self.xdim);
        let mut inx = (ubnd[0] + lbnd[0]) / 2;
        let mut iny = (ubnd[1] + lbnd[1]) / 2;
        let mut iv = (inx - lbnd[0]) + nx * (iny - lbnd[1]);
        if self.selected(iv) {
            return Ok((inx, iny, iv));
        }

        let cy = iny;
        let nskin = (ubnd[0] - inx)
            .min(inx - lbnd[0])
            .min(ubnd[1] - iny)
            .min(iny - lbnd[1]);
        let (mut xlo, mut xhi, mut ylo, mut yhi) = (inx, inx, iny, iny);
        for _ in 0..nskin {
            xhi += 1;
            xlo -= 1;
            yhi += 1;
            ylo -= 1;
            iv += 1;
            inx += 1;

            // Up the right edge from the middle.
            iny = cy;
            while iny <= yhi {
                if self.selected(iv) {
                    return Ok((inx, iny, iv));
                }
                iv += nx;
                iny += 1;
            }
            iv -= nx;
            iny -= 1;

            // Left along the top edge.
            inx = xhi;
            while inx >= xlo {
                if self.selected(iv) {
                    return Ok((inx, iny, iv));
                }
                iv -= 1;
                inx -= 1;
            }
            iv += 1;
            inx += 1;

            // Down the left edge.
            iny = yhi;
            while iny >= ylo {
                if self.selected(iv) {
                    return Ok((inx, iny, iv));
                }
                iv -= nx;
                iny -= 1;
            }
            iv += nx;
            iny += 1;

            // Right along the bottom edge.
            inx = xlo;
            while inx <= xhi {
                if self.selected(iv) {
                    return Ok((inx, iny, iv));
                }
                iv += 1;
                inx += 1;
            }
            iv -= 1;
            inx -= 1;

            // Up the right edge to just below the middle.
            iny = ylo;
            while iny < cy {
                if self.selected(iv) {
                    return Ok((inx, iny, iv));
                }
                iv += nx;
                iny += 1;
            }
        }

        let text = match self.oper {
            PixelOper::Lt => "less than",
            PixelOper::Le => "less than or equal to",
            PixelOper::Eq => "equal to",
            PixelOper::Ne => "not equal to",
            PixelOper::Ge => "greater than or equal to",
            PixelOper::Gt => "greater than",
        };
        Err(AstError::new(
            ErrorCode::Nonin,
            format!(
                "astOutline{}: Could not find a pixel value {text} {} in the supplied array.",
                T::CODE,
                crate::frames::axis_format::format_value("%g", 6, self.value.to_f64())?
            ),
        ))
    }

    /// polygon.c `TraceEdge`: the outline, as PIXEL coordinates, of the
    /// selected pixels round pixel (ix0,iy0), whose right neighbour is not
    /// selected, traced anti-clockwise from corner to pixel corner, each
    /// vertex moved [`TRACE_DELTA`] towards the inside. With `full` every
    /// corner is a vertex, otherwise only those where the outline turns.
    /// `None` when the outline turns more often right than left, so encloses
    /// a hole rather than the selected pixels.
    fn trace_edge(
        &self,
        ix0: i64,
        iy0: i64,
        starpix: bool,
        full: bool,
    ) -> Option<(Vec<f64>, Vec<f64>)> {
        let (lbnd, ubnd, nx) = (self.lbnd, self.ubnd, self.xdim);
        let (mut xvert, mut yvert) = (Vec::new(), Vec::new());

        // The corners of a pixel are 0 bottom left, 1 top left, 2 top right
        // and 3 bottom right. The trace starts up the right edge of
        // (ix0,iy0), arriving at its top right corner.
        let (mut ix, mut iy) = (ix0, iy0);
        let mut at = 2;
        let mut pa = (ix - lbnd[0]) + nx * (iy - lbnd[1]);
        let mut nright = 0i64;
        loop {
            let (xx, yy, pc, dx, dy);
            match at {
                0 => {
                    (xx, yy) = if starpix {
                        (ix as f64 - 1.0, iy as f64 - 1.0)
                    } else {
                        (ix as f64 - 0.5, iy as f64 - 0.5)
                    };
                    let pb = pa - nx - 1;
                    pc = pb + 1;
                    if iy > lbnd[1] && ix > lbnd[0] && self.selected(pb) {
                        nright += 1;
                        pa = pb;
                        at = 1;
                        ix -= 1;
                        iy -= 1;
                        (dx, dy) = (TRACE_DELTA, -TRACE_DELTA);
                    } else if iy > lbnd[1] && self.selected(pc) {
                        pa = pc;
                        at = 0;
                        iy -= 1;
                        (dx, dy) = (TRACE_DELTA, 0.0);
                    } else {
                        nright -= 1;
                        at = 3;
                        (dx, dy) = (TRACE_DELTA, TRACE_DELTA);
                    }
                }
                1 => {
                    (xx, yy) = if starpix {
                        (ix as f64 - 1.0, iy as f64)
                    } else {
                        (ix as f64 - 0.5, iy as f64 + 0.5)
                    };
                    let pb = pa + nx - 1;
                    pc = pa - 1;
                    if iy < ubnd[1] && ix > lbnd[0] && self.selected(pb) {
                        nright += 1;
                        pa = pb;
                        at = 2;
                        ix -= 1;
                        iy += 1;
                        (dx, dy) = (-TRACE_DELTA, -TRACE_DELTA);
                    } else if ix > lbnd[0] && self.selected(pc) {
                        pa = pc;
                        at = 1;
                        ix -= 1;
                        (dx, dy) = (0.0, -TRACE_DELTA);
                    } else {
                        nright -= 1;
                        at = 0;
                        (dx, dy) = (TRACE_DELTA, -TRACE_DELTA);
                    }
                }
                2 => {
                    (xx, yy) = if starpix {
                        (ix as f64, iy as f64)
                    } else {
                        (ix as f64 + 0.5, iy as f64 + 0.5)
                    };
                    let pb = pa + nx + 1;
                    pc = pa + nx;
                    if iy < ubnd[1] && ix < ubnd[0] && self.selected(pb) {
                        nright += 1;
                        pa = pb;
                        at = 3;
                        ix += 1;
                        iy += 1;
                        (dx, dy) = (-TRACE_DELTA, TRACE_DELTA);
                    } else if iy < ubnd[1] && self.selected(pc) {
                        pa = pc;
                        at = 2;
                        iy += 1;
                        (dx, dy) = (-TRACE_DELTA, 0.0);
                    } else {
                        nright -= 1;
                        at = 1;
                        (dx, dy) = (-TRACE_DELTA, -TRACE_DELTA);
                    }
                }
                _ => {
                    (xx, yy) = if starpix {
                        (ix as f64, iy as f64 - 1.0)
                    } else {
                        (ix as f64 + 0.5, iy as f64 - 0.5)
                    };
                    let pb = pa - (nx - 1);
                    pc = pa + 1;
                    if iy > lbnd[1] && ix < ubnd[0] && self.selected(pb) {
                        nright += 1;
                        pa = pb;
                        at = 0;
                        ix += 1;
                        iy -= 1;
                        (dx, dy) = (TRACE_DELTA, TRACE_DELTA);
                    } else if ix < ubnd[0] && self.selected(pc) {
                        pa = pc;
                        at = 3;
                        ix += 1;
                        (dx, dy) = (0.0, TRACE_DELTA);
                    } else {
                        nright -= 1;
                        at = 2;
                        (dx, dy) = (-TRACE_DELTA, TRACE_DELTA);
                    }
                }
            }
            // A corner where the outline bends, or every corner when the
            // outline is to be smoothed.
            if full || pa != pc {
                xvert.push(xx + dx);
                yvert.push(yy + dy);
            }
            if ix == ix0 && iy == iy0 && at == 2 {
                break;
            }
        }
        (nright <= 0).then_some((xvert, yvert))
    }
}

/// polygon.c `SmoothPoly`: replace each vertex by `strength` times the mean
/// of the vertices in a box of `2*boxsize+1` vertices centred on it, plus
/// `1-strength` times itself. The box wraps round the ends of the polygon,
/// and is narrowed to leave room for two boxes round it.
fn smooth_poly(x: &mut [f64], y: &mut [f64], boxsize: i32, strength: f64) {
    let nv = x.len();
    let (ox, oy) = (x.to_vec(), y.to_vec());
    let b = 1.0 - strength;

    let mut half_width = nv as i64 / 4 - 1;
    if i64::from(boxsize) < half_width {
        half_width = i64::from(boxsize);
    }
    if half_width < 1 {
        half_width = 1;
    }
    let half_width = half_width as usize;
    let a = strength / (2 * half_width + 1) as f64;

    // The sums over the box centred on the first vertex, which takes
    // vertices from the end of the polygon.
    let (mut px, mut qx) = (1, nv);
    let (mut sx, mut sy) = (ox[0], oy[0]);
    for _ in 0..half_width {
        qx -= 1;
        sx += ox[px] + ox[qx];
        sy += oy[px] + oy[qx];
        px += 1;
    }

    // Each step forms the new vertex, then moves the box on one vertex: the
    // first half box drops vertices from the end of the polygon, the middle
    // drops them from the start, and the last half box adds them from the
    // start.
    let mut i = 0;
    let mut step = |i: usize, px: usize, qx: usize, sx: &mut f64, sy: &mut f64| {
        x[i] = a * *sx + b * ox[i];
        y[i] = a * *sy + b * oy[i];
        *sx += ox[px] - ox[qx];
        *sy += oy[px] - oy[qx];
    };
    while i < half_width {
        step(i, px, qx, &mut sx, &mut sy);
        px += 1;
        qx += 1;
        i += 1;
    }
    qx = 0;
    while i < nv - half_width - 1 {
        step(i, px, qx, &mut sx, &mut sy);
        px += 1;
        qx += 1;
        i += 1;
    }
    px = 0;
    while i < nv {
        step(i, px, qx, &mut sx, &mut sy);
        px += 1;
        qx += 1;
        i += 1;
    }
}

/// One edge of a downsized polygon (polygon.c `Segment`): the indices of the
/// original vertices at its ends, `i1` in the first cycle and `i2` possibly
/// in the second (so `i2 > i1`), and the largest distance of an original
/// vertex between them from the edge, at vertex `imax`.
#[derive(Debug, Clone, Copy)]
struct Segment {
    i1: usize,
    i2: usize,
    error: f64,
    imax: usize,
}

impl Segment {
    /// polygon.c `NewSegment` given no Segment to reuse. C leaves `error`
    /// and `imax` uninitialised; they are set by `find_max` before use, but
    /// for an edge whose original vertices all lie more than one unit to its
    /// left, a signed `find_max` leaves `imax` as it was, and this sets it to
    /// the start.
    fn new(i1: usize, i2: usize, nv: usize) -> Self {
        let mut seg = Segment {
            i1,
            i2,
            error: 0.0,
            imax: i1,
        };
        seg.reset(i1, i2, nv);
        seg.imax = seg.i1;
        seg
    }

    /// polygon.c `NewSegment` given a Segment to reuse: set the ends, keeping
    /// `error` and `imax`.
    fn reset(&mut self, i1: usize, mut i2: usize, nv: usize) {
        if i2 < i1 {
            i2 += nv;
        }
        if i1 < nv {
            self.i1 = i1;
            self.i2 = i2;
        } else {
            self.i1 = i1 - nv;
            self.i2 = i2 - nv;
        }
    }

    /// polygon.c `FindMax`: the original vertex between the ends furthest
    /// from the line joining them, to the right of it, or either side when
    /// `abs`. In a plain Frame this is plane geometry; otherwise the Frame's
    /// `resolve_points` gives the distances.
    fn find_max(&mut self, frm: &dyn Frame, x: &[f64], y: &[f64], abs: bool) -> AstResult<()> {
        let nv = x.len();
        let i1 = self.i1;
        let mut i2 = self.i2;
        let n = i2 as i64 - i1 as i64 - 1;
        let mut i2b = i2;
        if i2 >= nv {
            i2 -= nv;
            i2b = nv;
        }

        if n < 1 {
            self.error = 0.0;
            self.imax = i1;
        } else if frm.as_any().is::<crate::frames::BasicFrame>() {
            let (ax, ay) = (x[i1], y[i1]);
            let bax = x[i2] - ax;
            let bay = y[i2] - ay;
            let ba = crate::math::sqrt(bax * bax + bay * bay);
            self.error = -1.0;
            let mut test = |i: usize, imax: usize| {
                let cax = x[i] - ax;
                let cay = y[i] - ay;
                let mut error = (bay * cax - cay * bax) / ba;
                if abs {
                    error = error.abs();
                }
                if error > self.error {
                    self.error = error;
                    self.imax = imax;
                }
            };
            for i in i1 + 1..i2b {
                test(i, i);
            }
            if i2b != i2 {
                for i in 0..i2 {
                    test(i, i + i2b);
                }
            }
        } else {
            let mut between: Vec<[f64; 2]> = (i1 + 1..i2b).map(|i| [x[i], y[i]]).collect();
            if i2b != i2 {
                between.extend((0..i2).map(|i| [x[i], y[i]]));
            }
            let resolved = frm.resolve_points(
                &[x[i1], y[i1]],
                &[x[i2], y[i2]],
                &PointSet::from_rows(&between),
            )?;
            self.error = -1.0;
            for i in 1..=n as usize {
                let mut error = resolved.point(i - 1)[1];
                if abs {
                    error = error.abs();
                }
                if error > self.error {
                    self.error = error;
                    self.imax = i + i1;
                }
            }
        }
        Ok(())
    }
}

/// polygon.c `AddToChain`: insert Segment `seg` into `chain`, which is
/// ordered by decreasing error, after every Segment whose error it does not
/// exceed.
fn add_to_chain(chain: &mut Vec<usize>, segs: &[Segment], seg: usize) {
    let error = segs[seg].error;
    let at = if chain.is_empty() || error > segs[chain[0]].error {
        0
    } else {
        (1..chain.len())
            .find(|&k| error > segs[chain[k]].error)
            .unwrap_or(chain.len())
    };
    chain.insert(at, seg);
}

/// polygon.c `DownsizePoly`: the vertices of a polygon, given as `x` and `y`
/// in Frame `frm`, reduced to a subset that keeps every original vertex
/// within `maxerr` of the result, or that has `maxvert` vertices, whichever
/// comes first. `maxerr` at or below zero means no error limit, and
/// `maxvert` below 3 no vertex limit. The vertices are added one at a time
/// to a triangle, each time the one furthest from the edge that departs
/// furthest from the original.
fn downsize_poly(
    x: &[f64],
    y: &[f64],
    maxerr: f64,
    maxvert: usize,
    frm: &dyn Frame,
) -> AstResult<Vec<Vec<f64>>> {
    let nv = x.len();
    let copy = || (0..nv).map(|i| vec![x[i], y[i]]).collect();
    if maxerr <= 0.0 && (maxvert < 3 || maxvert >= nv) {
        return Ok(copy());
    }

    // Three widely spaced vertices: the extremes on the axis of greater
    // range, and the vertex furthest from the line between them.
    let (mut x1, mut x2, mut y1, mut y2) = (f64::MAX, -f64::MAX, f64::MAX, -f64::MAX);
    let (mut i1x, mut i1y) = (0, 0);
    let (mut i2x, mut i2y) = (nv / 2, nv / 2);
    for i in 0..nv {
        if y[i] < y1 {
            y1 = y[i];
            i1y = i;
        } else if y[i] > y2 {
            y2 = y[i];
            i2y = i;
        }
        if x[i] < x1 {
            x1 = x[i];
            i1x = i;
        } else if x[i] > x2 {
            x2 = x[i];
            i2x = i;
        }
    }
    let (i1, mut i2) = if y2 - y1 > x2 - x1 {
        (i1y, i2y)
    } else {
        (i1x, i2x)
    };

    let mut seg1 = Segment::new(i1, i2, nv);
    seg1.find_max(frm, x, y, false)?;
    let mut seg2 = Segment::new(i2, i1, nv);
    seg2.find_max(frm, x, y, false)?;

    // Order the three anti-clockwise.
    let mut i3;
    if seg2.error > seg1.error {
        i3 = seg2.imax;
        seg1.reset(i1, i3, nv);
        seg1.find_max(frm, x, y, false)?;
        i2 = seg1.imax;
    } else {
        i2 = seg1.imax;
        seg1.reset(i2, i1, nv);
        seg1.find_max(frm, x, y, false)?;
        i3 = seg1.imax;
    }
    if i2 >= nv {
        i2 -= nv;
    }
    if i3 >= nv {
        i3 -= nv;
    }

    seg1.reset(i1, i2, nv);
    seg2.reset(i2, i3, nv);
    let mut seg3 = Segment::new(i3, i1, nv);
    let mut newpoly = vec![i1, i2, i3];

    // The edges, held in `segs` and listed in `chain` by decreasing error.
    seg3.find_max(frm, x, y, true)?;
    seg1.find_max(frm, x, y, true)?;
    seg2.find_max(frm, x, y, true)?;
    let mut segs = vec![seg3, seg1, seg2];
    let mut chain = vec![0];
    add_to_chain(&mut chain, &segs, 1);
    add_to_chain(&mut chain, &segs, 2);

    let maxvert = if maxvert < 3 { nv } else { maxvert };

    // Split the edge of largest error at its furthest vertex until the
    // error is small enough or there are enough vertices.
    while segs[chain[0]].error > maxerr && newpoly.len() < maxvert {
        let head = chain.remove(0);
        let imax = segs[head].imax;
        newpoly.push(if imax >= nv { imax - nv } else { imax });

        let mut split = Segment::new(imax, segs[head].i2, nv);
        segs[head].i2 = imax;
        split.find_max(frm, x, y, true)?;
        segs.push(split);
        add_to_chain(&mut chain, &segs, segs.len() - 1);
        segs[head].find_max(frm, x, y, true)?;
        add_to_chain(&mut chain, &segs, head);
    }

    if newpoly.len() == nv {
        return Ok(copy());
    }
    newpoly.sort_unstable();
    Ok(newpoly.iter().map(|&i| vec![x[i], y[i]]).collect())
}

/// The Frame `astConvex` and `astOutline` define their Polygon in.
fn pixel_frame() -> AstResult<crate::frames::BasicFrame> {
    crate::frames::BasicFrameBuilder::new(2)
        .domain("PIXEL")
        .unit(1, "pixel")
        .unit(2, "pixel")
        .title("Pixel coordinates")
        .build()
}

/// A Polygon in `frame` with vertices `points` as given. C builds a Polygon
/// round three junk vertices and then replaces its PointSet, so the vertex
/// order is not checked by `EnsureInside`.
fn pixel_polygon(frame: crate::frames::BasicFrame, points: Vec<Vec<f64>>) -> Polygon {
    Polygon {
        data: RegionData::new(FrameSet::new(Box::new(frame)), points),
        simp_vertices: None,
    }
}

/// A polygonal region within a 2-D coordinate frame.
///
/// Defined by an ordered list of N vertices (N >= 3). The polygon is
/// implicitly closed (the last vertex connects back to the first).
/// Point membership uses the crossing-number (ray-casting) algorithm.
#[derive(Debug, Clone)]
pub struct Polygon {
    /// Shared region data (frameset, defining points, flags).
    pub(crate) data: RegionData,
    /// `SimpVertices`: whether simplification may re-define the Polygon by
    /// transforming its vertices without checking that the edges stayed
    /// straight. `None` is unset, and reads back as the default `true`
    /// (polygon.c:6510-6514).
    pub(crate) simp_vertices: Option<bool>,
}

/// Builder for constructing a [`Polygon`].
pub struct PolygonBuilder {
    frame: DynFrame,
    vertices: Vec<Vec<f64>>,
    uncertainty: Option<DynRegion>,
}

impl PolygonBuilder {
    /// Create a builder for a Polygon in the given frame.
    ///
    /// # Parameters
    ///
    /// - `frame`: A 2-D frame (naxes must be 2).
    /// - `vertices`: Ordered list of vertex positions. Each vertex is a
    ///   `Vec<f64>` of length 2.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Badin`] if the frame does not have exactly 2 axes, as
    ///   astInitPolygon reports it, or (from [`PolygonBuilder::build`]) a
    ///   vertex is BAD.
    /// - [`ErrorCode::Nptin`] if no vertices are supplied, as astPointSet
    ///   reports it. One or two vertices are accepted, as C accepts them.
    /// - [`ErrorCode::Naxin`] if a vertex does not have 2 coordinates.
    pub fn new(frame: impl Frame + 'static, vertices: Vec<Vec<f64>>) -> AstResult<Self> {
        Self::from_dyn(Box::new(frame), vertices)
    }

    /// Create a builder for a Polygon from an owned [`DynFrame`].
    ///
    /// Equivalent to [`PolygonBuilder::new`] but accepts a boxed trait object,
    /// for use when the frame is only available as a `DynFrame` (e.g. a
    /// current-Frame snapshot during simplification).
    pub(crate) fn from_dyn(frame: DynFrame, vertices: Vec<Vec<f64>>) -> AstResult<Self> {
        let naxes = frame.naxes();
        if naxes != 2 {
            return Err(AstError::new(
                ErrorCode::Badin,
                format!(
                    "astInitPolygon(Polygon): The supplied {} has {naxes} axes - polygons must \
                     have exactly 2 axes.",
                    crate::frames::find_frame::frame_class_name(frame.as_ref())
                ),
            ));
        }
        if vertices.is_empty() {
            return Err(AstError::new(
                ErrorCode::Nptin,
                "astInitPointSet(PointSet): Number of points (0) is not valid.",
            ));
        }
        for (i, v) in vertices.iter().enumerate() {
            if v.len() != naxes {
                return Err(AstError::new(
                    ErrorCode::Naxin,
                    format!(
                        "Polygon vertex {} has {} coordinates but frame has {} axes",
                        i,
                        v.len(),
                        naxes
                    ),
                ));
            }
        }
        Ok(PolygonBuilder {
            frame,
            vertices,
            uncertainty: None,
        })
    }

    /// Set an uncertainty region.
    pub fn uncertainty(mut self, unc: DynRegion) -> Self {
        self.uncertainty = Some(unc);
        self
    }

    /// Build the Polygon.
    pub fn build(self) -> AstResult<Polygon> {
        let frameset = FrameSet::new(self.frame);

        // A vertex may not be bad (polygon.c:7131). C checks every X value
        // before any Y value, so reports the first vertex with a bad X.
        for axis in 0..2 {
            if let Some(j) = self.vertices.iter().position(|v| v[axis] == BAD_VALUE) {
                return Err(AstError::new(
                    ErrorCode::Badin,
                    format!(
                        "astInitPolygon(Polygon): One or more bad axis values supplied for \
                         the vertex number {}.",
                        j + 1
                    ),
                ));
            }
        }

        // Defining points are the vertices themselves.
        let points = self.vertices;
        let data = RegionData::new(frameset, points);
        let mut polygon = Polygon {
            data,
            simp_vertices: None,
        };

        // astSetUnc, as C's constructor calls it (region.c:13372). C reaches it
        // from astInitRegion, which astInitPolygon calls before EnsureInside, so
        // the uncertainty is re-centred on the first vertex as supplied -- before
        // any reversal below can change which vertex that is.
        if let Some(unc) = self.uncertainty {
            polygon.set_uncertainty(unc)?;
        }

        // Mirror C astInitPolygon (polygon.c:7160): reverse the vertex order if
        // necessary so that the unnegated Polygon represents the bounded
        // interior, matching the parent Region convention.
        polygon.ensure_inside()?;
        Ok(polygon)
    }
}

// ============================================================================
// Attributes
// ============================================================================

impl Polygon {
    /// `SimpVertices`: may simplification re-define the Polygon by
    /// transforming its vertices?
    ///
    /// When this is false, [`Mapping::simplified`] additionally checks that the
    /// transformation left the edges straight, and abandons the replacement if
    /// it did not. The default is true (polygon.c:6511).
    pub fn simp_vertices(&self) -> bool {
        self.simp_vertices.unwrap_or(true)
    }

    /// Set `SimpVertices`.
    pub fn set_simp_vertices(&mut self, value: bool) {
        self.simp_vertices = Some(value);
    }

    /// Has `SimpVertices` been set?
    pub fn test_simp_vertices(&self) -> bool {
        self.simp_vertices.is_some()
    }

    /// Clear `SimpVertices`, restoring the default.
    pub fn clear_simp_vertices(&mut self) {
        self.simp_vertices = None;
    }
}

// ============================================================================
// Integrity
// ============================================================================

impl Polygon {
    /// Mutate the current frame with region integrity enforcement.
    ///
    /// When adaptive (the default), the FrameSet's base-to-current mapping is
    /// updated to account for the change, and defining points remain valid
    /// in the base frame. When non-adaptive, the change is applied to both
    /// frames without remapping.
    ///
    /// `f` makes the change to each Frame it is made to, so it may be called
    /// more than once.
    pub fn with_integrity<F>(&mut self, f: F) -> AstResult<()>
    where
        F: Fn(&mut dyn Frame) -> AstResult<()>,
    {
        super::data::with_region_integrity(self, f)
    }

    /// Permute the axes of this region's current frame.
    pub fn perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.data.perm_axes(perm)
    }

    /// The convex hull of the pixels of a 2-D array whose values pass a test,
    /// as a Polygon in a PIXEL Frame (polygon.c `astConvex<X>` and
    /// `astConvex8<X>`).
    ///
    /// `array` holds the pixels row by row, X varying fastest, with pixel
    /// indices `lbnd` to `ubnd` inclusive on each axis. A pixel is selected
    /// when its value compares with `value` as `oper` says. With `starpix`,
    /// the Starlink convention, pixel centres are at half-integer PIXEL
    /// coordinates; otherwise, as in `astResample`, at integers.
    ///
    /// The vertices are the centres of selected pixels, one per corner of
    /// each of the four parts of the hull, so a vertex may repeat. The result
    /// is `None` when no pixel is selected, as C returns a null pointer
    /// without an error.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Dimin`] if `ubnd` is below `lbnd` on either axis or
    /// `array` is shorter than the pixel count, where C reads past the array.
    pub fn convex<T: PixelValue>(
        value: T,
        oper: PixelOper,
        array: &[T],
        lbnd: [i64; 2],
        ubnd: [i64; 2],
        starpix: bool,
    ) -> AstResult<Option<Polygon>> {
        let pixels = PixelSelection::new(value, oper, array, lbnd, ubnd, "astConvex")?;
        let Some(hull) = pixels.convex_hull(starpix)? else {
            return Ok(None);
        };
        let points = hull.iter().map(|v| v.to_vec()).collect();
        Ok(Some(pixel_polygon(pixel_frame()?, points)))
    }

    /// A Polygon with a subset of this one's vertices that approximates it
    /// to within `maxerr`, a distance in its base Frame, or that has
    /// `maxvert` vertices, whichever is reached first (polygon.c
    /// `astDownsize`). `maxerr` at or below zero means no error limit, and
    /// `maxvert` below 3 no vertex limit. The vertices kept are densest where
    /// the boundary bends most.
    ///
    /// # Errors
    ///
    /// Any error from the base Frame's `resolve_points`.
    pub fn downsize(&self, maxerr: f64, maxvert: usize) -> AstResult<Polygon> {
        let x: Vec<f64> = self.data.points.iter().map(|p| p[0]).collect();
        let y: Vec<f64> = self.data.points.iter().map(|p| p[1]).collect();
        let points = downsize_poly(&x, &y, maxerr, maxvert, self.data.frameset.base())?;
        let mut result = self.clone();
        result.data.points = points;
        result.data.invalidate_cache();
        Ok(result)
    }

    /// A Polygon outlining the region of a 2-D array whose pixels pass a
    /// test and that contains a given pixel, in a PIXEL Frame (polygon.c
    /// `astOutline<X>` and `astOutline8<X>`).
    ///
    /// `array` holds the pixels row by row, X varying fastest, with pixel
    /// indices `lbnd` to `ubnd` inclusive on each axis, and a pixel is
    /// selected when its value compares with `value` as `oper` says. The
    /// outline is of the selected region containing pixel `inside`, given by
    /// its pixel indices; if that pixel is outside the array or not selected,
    /// the selected pixel nearest the centre of the array found by a spiral
    /// search is used. The outline follows pixel edges, so holes in the
    /// region are not part of it. With `starpix` pixel centres are at
    /// half-integer PIXEL coordinates; otherwise, as in `astResample`, at
    /// integers.
    ///
    /// When `maxerr` is above zero or `maxvert` above 2, the full outline is
    /// smoothed and then reduced as [`Polygon::downsize`] reduces a Polygon.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Nonin`] if the spiral search finds no selected pixel, and
    /// [`ErrorCode::Dimin`] if `ubnd` is below `lbnd` on either axis or
    /// `array` is shorter than the pixel count, where C reads past the array.
    #[allow(clippy::too_many_arguments)]
    pub fn outline<T: PixelValue>(
        value: T,
        oper: PixelOper,
        array: &[T],
        lbnd: [i64; 2],
        ubnd: [i64; 2],
        maxerr: f64,
        maxvert: usize,
        inside: [i64; 2],
        starpix: bool,
    ) -> AstResult<Polygon> {
        let pixels = PixelSelection::new(value, oper, array, lbnd, ubnd, "astOutline")?;

        // Smoothing needs every corner of the outline, not only the bends.
        let smooth = maxerr > 0.0 || maxvert > 2;

        let [mut inx, mut iny] = inside;
        let mut iv = 0;
        let mut valid = inx >= lbnd[0] && inx <= ubnd[0] && iny >= lbnd[1] && iny <= ubnd[1];
        if valid {
            iv = (inx - lbnd[0]) + (iny - lbnd[1]) * pixels.xdim;
            valid = pixels.selected(iv);
        }
        if !valid {
            (inx, iny, iv) = pixels.find_inside_point()?;
        }

        // Move right from the inside pixel to the edge of its region. An
        // edge whose outline does not enclose the inside pixel is the edge
        // of a hole, which is crossed to look for the next edge.
        let mut candidate = None;
        let mut pv = iv;
        let mut ixv = inx;
        let mut stop_at_invalid = true;
        loop {
            ixv += 1;
            if ixv > ubnd[0] {
                break;
            }
            pv += 1;
            let valid = pixels.selected(pv);
            if stop_at_invalid {
                if !valid {
                    candidate = pixels.trace_edge(ixv - 1, iny, starpix, smooth);
                    if candidate.is_some() {
                        break;
                    }
                    stop_at_invalid = false;
                }
            } else if valid {
                stop_at_invalid = true;
            }
        }
        // Otherwise the region reaches the right edge of the array.
        let candidate = candidate.or_else(|| pixels.trace_edge(ixv - 1, iny, starpix, smooth));
        let Some((mut xv, mut yv)) = candidate else {
            // Not expected: the outline of a hole encloses it, so the search
            // leaves every hole before the edge of the array. C would pass a
            // null PointSet on.
            return Err(AstError::new(
                ErrorCode::Inter,
                format!(
                    "astOutline{}: No outline encloses pixel ({inx},{iny}) (internal AST \
                     programming error).",
                    T::CODE
                ),
            ));
        };

        if smooth {
            let mut boxsize = if maxerr > 0.0 {
                maxerr as i32
            } else {
                i32::MAX
            };
            let nv0 = xv.len() as i64;
            let tmp = if maxvert > 2 {
                nv0 / (2 * maxvert as i64)
            } else {
                i64::from(i32::MAX)
            };
            if tmp < i64::from(boxsize) {
                boxsize = tmp as i32;
            }
            // Room for at least ten full boxes round the outline.
            let tmp = nv0 / 20;
            if tmp < i64::from(boxsize) {
                boxsize = tmp as i32;
            }
            if boxsize == 0 {
                boxsize = 1;
            }
            smooth_poly(&mut xv, &mut yv, boxsize, 1.0);
        }

        let frame = pixel_frame()?;
        let points = downsize_poly(&xv, &yv, maxerr, maxvert, &frame)?;
        Ok(pixel_polygon(frame, points))
    }

    /// Un-negated base-Frame membership, with the effective `Closed` value
    /// supplied by the caller. `reg_base_inside` passes the Region's own
    /// value; `reg_base_box` passes the raw one, because the probe C hands to
    /// `astNormBox` is a copy with Negated cleared (polygon.c:4300-4302).
    fn base_inside(&self, pts: &PointSet, closed: bool) -> AstResult<Vec<bool>> {
        // Faithful port of C Polygon Transform (polygon.c:6235). The membership
        // test is winding-relative: a line is drawn from a cached interior
        // point to the test point, and the number of edges crossed determines
        // whether the two lie on the same side of the boundary. This mirrors C
        // exactly (rather than a winding-absolute ray-cast to infinity) so that
        // reversing the vertex order in `ensure_inside` is a true no-op on the
        // represented region, as it is in C.
        let npoint = pts.npoint();
        let bfrm = self.data.frameset.base();

        let (edges, in_point, _acw) = self.cache()?;
        let nv = edges.len();

        // Boundary position of a test point relative to the polygon.
        #[derive(PartialEq)]
        enum Pos {
            Unknown,
            In,
            Out,
            On,
        }

        let mut result = vec![false; npoint];

        for (idx, inside) in result.iter_mut().enumerate() {
            let point = pts.point(idx);

            // BAD input coordinates mean the point is outside.
            if point_has_bad(&point[..2]) {
                continue;
            }

            let p = [point[0], point[1]];

            // Define the line from the interior point to the test point.
            let a = bfrm
                .line_def(&in_point, &p)?
                .ok_or_else(|| AstError::new(ErrorCode::Inter, "reg_base_inside: bad test line"))?;

            // Count crossings of this line with the polygon edges.
            let mut ncross = 0i32;
            let mut pos = Pos::Unknown;

            for edge in edges.iter().take(nv) {
                if bfrm.line_contains(edge, &p)? {
                    pos = Pos::On;
                    break;
                } else if bfrm.line_crossing(edge, &a)?.0 {
                    ncross += 1;
                }
            }

            if pos == Pos::Unknown {
                pos = if ncross % 2 == 0 { Pos::In } else { Pos::Out };
            }

            *inside = match pos {
                Pos::In => true,
                Pos::Out => false,
                _ => closed,
            };
        }

        Ok(result)
    }

    /// Calculate the cached intermediate values used by the membership and
    /// bounded tests.
    ///
    /// Faithful port of C `Cache` (polygon.c:575). Builds an edge description
    /// for each edge (edge `i` runs from vertex `i-1` to vertex `i`, edge 0
    /// wrapping from the last vertex). For each non-degenerate edge it measures
    /// the polygon width perpendicular to that edge via [`polywidth`] and keeps
    /// the interior representative point for the widest edge. If any positive
    /// width is found the vertices are stored in anti-clockwise order
    /// (`acw = true`) and the retained point lies inside the polygon; if none
    /// is found the vertices are in clockwise order (`acw = false`) and the
    /// retained point lies outside (the probes ran off to the infinite exterior
    /// of a polygonal hole). Returns `(edges, inside_point, acw)`.
    fn cache(&self) -> AstResult<(Vec<LineDef>, [f64; 2], bool)> {
        // Kept until the Polygon changes, as C keeps it under `stale`.
        if let Some(super::data::ClassCache::Polygon { edges, inside, acw }) =
            &*self.data.class_cache.borrow()
        {
            return Ok((edges.clone(), *inside, *acw));
        }
        let (edges, inside, acw) = self.compute_cache()?;
        *self.data.class_cache.borrow_mut() = Some(super::data::ClassCache::Polygon {
            edges: edges.clone(),
            inside,
            acw,
        });
        Ok((edges, inside, acw))
    }

    fn compute_cache(&self) -> AstResult<(Vec<LineDef>, [f64; 2], bool)> {
        let bfrm = self.data.frameset.base();
        let ptr = &self.data.points;
        let nv = ptr.len();

        let mut edges: Vec<LineDef> = Vec::with_capacity(nv);
        let mut start = [ptr[nv - 1][0], ptr[nv - 1][1]];
        for vertex in ptr.iter() {
            let end = [vertex[0], vertex[1]];
            let edge = bfrm
                .line_def(&start, &end)?
                .ok_or_else(|| AstError::new(ErrorCode::Inter, "cache: bad polygon edge"))?;
            edges.push(edge);
            start = end;
        }

        // Find a point well inside the polygon: the mid-width point of the edge
        // with the greatest perpendicular width. `polcen` tracks the latest
        // probe centre so that, if no positive width is found (a clockwise
        // polygon), `inside` falls back to a point outside the polygonal hole.
        let mut maxwid = -1.0;
        let mut inside = [BAD_VALUE, BAD_VALUE];
        let mut polcen = [BAD_VALUE, BAD_VALUE];
        for i in 0..nv {
            if edges[i].length > 0.0 {
                let (polwid, cen) = polywidth(bfrm, &edges, i, nv)?;
                polcen = cen;
                if !is_bad(polwid) && polwid > maxwid {
                    maxwid = polwid;
                    inside = cen;
                }
            }
        }

        if maxwid < 0.0 {
            inside = polcen;
        }

        Ok((edges, inside, maxwid >= 0.0))
    }

    /// Whether the base-Frame vertices are stored in anti-clockwise order.
    fn cache_acw(&self) -> AstResult<bool> {
        Ok(self.cache()?.2)
    }

    /// The solid angle (steradians) subtended by a Polygon defined in a
    /// SkyFrame.
    ///
    /// Faithful port of C `SkyArea` (polygon.c:5469): sum the interior angle at
    /// each vertex (folded into `0..2*PI`) and subtract `(n-2)*PI`.
    fn sky_area(&self) -> AstResult<f64> {
        let bfrm = self.data.frameset.base();
        let ptr = &self.data.points;
        let nvert = ptr.len();
        let mut result = 0.0;
        for ivert in 0..nvert {
            let b = [ptr[ivert][0], ptr[ivert][1]];
            let a = if ivert < nvert - 1 {
                [ptr[ivert + 1][0], ptr[ivert + 1][1]]
            } else {
                [ptr[0][0], ptr[0][1]]
            };
            let c = if ivert > 0 {
                [ptr[ivert - 1][0], ptr[ivert - 1][1]]
            } else {
                [ptr[nvert - 1][0], ptr[nvert - 1][1]]
            };
            let mut angle = bfrm.angle(&a, &b, &c)?;
            if angle < 0.0 {
                angle += 2.0 * std::f64::consts::PI;
            }
            result += angle;
        }
        result -= (nvert as f64 - 2.0) * std::f64::consts::PI;
        Ok(result)
    }

    /// Whether the (possibly negated) Polygon is bounded.
    ///
    /// Faithful port of C `GetBounded` (polygon.c:2749): SkyFrame Polygons are
    /// always bounded (a finite region has a finite negation on the sphere).
    /// Otherwise the result depends on the vertex order (`acw`) and the
    /// negation flag: anti-clockwise ⇒ bounded iff not negated; clockwise ⇒
    /// bounded iff negated.
    fn bounded_impl(&self) -> AstResult<bool> {
        let is_sky = self.data.frameset.base().as_any().is::<SkyFrame>();
        if is_sky {
            return Ok(true);
        }
        let acw = self.cache_acw()?;
        let neg = self.data.negated;
        Ok(if acw { !neg } else { neg })
    }

    /// Ensure the unnegated Polygon represents the inside of the polygon.
    ///
    /// Faithful port of C `EnsureInside` (polygon.c:1768). Reversing the vertex
    /// order is equivalent to negating the Polygon, but the parent Region class
    /// assumes an unnegated closed region is bounded. So if the unnegated
    /// Polygon would be unbounded, reverse the vertex order and toggle the
    /// negation flag to compensate, leaving the represented region unchanged
    /// while restoring the bounded-when-unnegated convention.
    ///
    /// For a SkyFrame Polygon both lobes are bounded, so "bounded" is taken to
    /// mean the smaller lobe: area < 2π steradians.
    pub(crate) fn ensure_inside(&mut self) -> AstResult<()> {
        let is_sky = self.data.frameset.base().as_any().is::<SkyFrame>();
        let bounded = if is_sky {
            self.sky_area()? < 2.0 * std::f64::consts::PI
        } else {
            self.bounded_impl()?
        };

        let negated = self.data.negated;
        if (bounded && negated) || (!bounded && !negated) {
            self.data.points.reverse();
            self.data.negate();
            self.data.invalidate_cache();
        }
        Ok(())
    }
}

// ============================================================================
// RegionGeometry implementation
// ============================================================================

impl RegionGeometry for Polygon {
    fn region_data(&self) -> &RegionData {
        &self.data
    }

    fn region_data_mut(&mut self) -> &mut RegionData {
        // Whatever the caller changes may change what `cache` derives.
        self.data.invalidate_cache();
        &mut self.data
    }

    fn reg_base_inside(&self, pts: &PointSet) -> AstResult<Vec<bool>> {
        // The caller applies negation, so this reports the un-negated
        // membership (C's `neg == 0` branch).
        self.base_inside(pts, self.data.closed_for_base_inside())
    }

    fn reg_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        if self.data.points.is_empty() {
            return Ok((vec![BAD_VALUE; 2], vec![BAD_VALUE; 2]));
        }

        let frm = self.data.frameset.base();
        let x0 = self.data.points[0][0];
        let y0 = self.data.points[0][1];

        // C expresses the box as axis offsets from the first vertex "in order
        // to avoid problems with boxes that cross RA=0 or RA=12h"
        // (polygon.c:4263-4297). Both ends start at zero, so the origin vertex
        // is always enclosed, and the `else if` means a vertex that would
        // extend both ends of an axis extends only the lower one.
        let mut lower = vec![0.0_f64; 2];
        let mut upper = vec![0.0_f64; 2];

        for v in &self.data.points {
            let dist = frm.ax_distance(1, x0, v[0])?;
            if dist < lower[0] {
                lower[0] = dist;
            } else if dist > upper[0] {
                upper[0] = dist;
            }

            let dist = frm.ax_distance(2, y0, v[1])?;
            if dist < lower[1] {
                lower[1] = dist;
            } else if dist > upper[1] {
                upper[1] = dist;
            }
        }

        lower[0] += x0;
        lower[1] += y0;
        upper[0] += x0;
        upper[1] += y0;

        // Extend the box to cover any coordinate-system singularity it
        // contains. C builds the membership probe by copying the Polygon,
        // giving it the base Frame and clearing Negated (polygon.c:4299-4305),
        // so the predicate is membership of the un-negated Polygon and the raw
        // Closed value is the effective one.
        let closed = self.data.closed;
        let inside = |point: &[f64]| -> AstResult<bool> {
            let pts = PointSet::from_rows(std::iter::once(point));
            Ok(self.base_inside(&pts, closed)?[0])
        };
        frm.norm_box(&mut lower, &mut upper, &inside)?;

        Ok((lower, upper))
    }

    /// polygon.c `RegBaseMesh`: MeshSize points shared between the edges by
    /// their lengths as the base Frame measures distance, `1 + (int)(len*mp)`
    /// to an edge; each edge starts at its normalised first vertex, and its
    /// other points are placed along it with the Frame's `offset`, so on the
    /// sky they follow the great circle. An edge of undefined length is left
    /// out, and a boundary of no length is AST__BADIN.
    fn reg_base_mesh(&self) -> AstResult<Vec<Vec<f64>>> {
        let frm = self.data.frameset.base();
        let vertices = &self.data.points;
        let nv = vertices.len();

        // The length of the edge ending at each vertex; the edge ending at
        // vertex 0 starts at the last vertex.
        let mut lens = vec![BAD_VALUE; nv];
        let mut total = 0.0;
        for iv in 0..nv {
            let start = &vertices[(iv + nv - 1) % nv];
            let d = frm.distance(start, &vertices[iv])?;
            if !is_bad(d) {
                total += d.abs();
            }
            lens[iv] = d;
        }
        if total <= 0.0 {
            return Err(AstError::new(
                ErrorCode::Badin,
                "astRegBaseMesh(Polygon): The boundary of the supplied Polygon has an \
                 undefined length.",
            ));
        }
        let mp = self.data.effective_mesh_size() as f64 / total;

        // The edges ending at vertices 1..nv, then the one ending at vertex 0.
        let mut mesh = Vec::new();
        for iv in (1..nv).chain(std::iter::once(0)) {
            if is_bad(lens[iv]) {
                continue;
            }
            let mut start = vertices[(iv + nv - 1) % nv].clone();
            frm.norm(&mut start)?;
            mesh.push(start.clone());
            let n = 1 + (lens[iv] * mp) as usize;
            if n > 1 {
                let delta = lens[iv] / n as f64;
                for ip in 1..n {
                    mesh.push(frm.offset(&start, &vertices[iv], delta * ip as f64)?);
                }
            }
        }
        Ok(mesh)
    }

    fn is_bounded_inner(&self) -> AstResult<bool> {
        // polygon.c GetBounded (polygon.c:2752): SkyFrame Polygons are always
        // bounded; otherwise it depends on vertex order and negation.
        self.bounded_impl()
    }

    fn remap_defining_points(&mut self, mapping: &dyn Mapping, forward: bool) -> AstResult<()> {
        // Transform the defining points (vertices) through the mapping.
        let naxes = 2;
        let pts = PointSet::from_rows(self.data.points.iter().map(|p| p.as_slice()));
        let transformed = mapping.transform(&pts, forward)?;

        let mut new_points = Vec::with_capacity(self.data.points.len());
        for i in 0..self.data.points.len() {
            let p = transformed.point(i);
            new_points.push(p[..naxes].to_vec());
        }
        self.data.points = new_points;
        self.data.invalidate_cache();
        Ok(())
    }
}

// ============================================================================
// Region trait
// ============================================================================

impl Region for Polygon {
    fn def_unc_cache(&self) -> Option<&std::cell::RefCell<Option<DynRegion>>> {
        Some(&self.data.def_unc)
    }

    fn explicit_unc(&self) -> Option<&DynRegion> {
        self.data.uncertainty.as_ref()
    }

    fn point_in_region(&self, point: &[f64]) -> AstResult<bool> {
        // Check for BAD coordinates in the input.
        if point_has_bad(point) {
            return Ok(false);
        }

        // Transform point from current frame to base frame.
        let naxes = self.data.frameset.base().naxes();
        let pts = PointSet::from_rows(std::iter::once(point));
        let base_pts = self.data.frameset.transform(&pts, false)?;
        let base_point = base_pts.point(0);

        // Check the transformed point for BAD values.
        if base_point[..naxes].iter().any(|&v| is_bad(v)) {
            return Ok(false);
        }

        // Test raw geometry.
        let inside = self.reg_base_inside_point(&base_point[..naxes])?;

        // Apply negation.
        if self.data.negated {
            Ok(!inside)
        } else {
            Ok(inside)
        }
    }

    fn reg_pins_mask(
        &self,
        pts: &PointSet,
        unc: Option<&dyn Region>,
    ) -> AstResult<(bool, Vec<bool>)> {
        // Faithful port of polygon.c::RegPins (4562). The margin width `wid` is
        // half the total geodesic length of the diagonals of the two
        // uncertainty bounding boxes: `wid = 0.5*(l1 + l2)`. Each supplied
        // point is tested against every polygon edge by resolving it into
        // components parallel and perpendicular to the edge; a point counts as
        // on the boundary iff, for at least one edge, its parallel component is
        // in `(-wid, edge_len+wid)` and its perpendicular component is in
        // `(-wid, wid)`. A point on no edge fails the whole test.
        let bfrm = self.data.frameset.base();
        if pts.ncoord() != 2 {
            return Err(AstError::new(
                ErrorCode::Inter,
                format!(
                    "reg_pins(Polygon): PointSet has {} coordinates per point, expected 2",
                    pts.ncoord()
                ),
            ));
        }

        // `tunc` is this Polygon's own uncertainty in the base Frame; `l1` is
        // the geodesic length of its bounding-box diagonal. Re-centring is
        // omitted (bound width is centre-invariant; the reachable default
        // uncertainty is already centred — tracked as the RCENT / R-UNC gaps).
        let tunc = self.get_unc_frm(FrameRef::Base)?;
        let (lbnd_tunc, ubnd_tunc) = tunc.get_region_bounds()?;
        let l1 = bfrm.distance(&lbnd_tunc, &ubnd_tunc)?;

        let l2 = if let Some(unc) = unc {
            let (lbnd_unc, ubnd_unc) = unc.get_region_bounds()?;
            bfrm.distance(&lbnd_unc, &ubnd_unc)?
        } else {
            0.0
        };

        let wid = 0.5 * (l1 + l2);

        let vertices = &self.data.points;
        let nv = vertices.len();
        let np = pts.npoint();

        // `on_edge[ip]` becomes true once point `ip` is found to lie within the
        // margin of some edge (mirrors C setting the point BAD in pset1).
        let mut on_edge = vec![false; np];

        // Edge `i` runs from vertex `i-1` to vertex `i`; edge 0 runs from the
        // last vertex to vertex 0.
        let mut start = vertices[nv - 1].clone();
        for end in vertices.iter() {
            let edge_len = bfrm.distance(&start, end)?;
            let resolved = bfrm.resolve_points(&start, end, pts)?;
            let parmax = edge_len + wid;
            for (ip, on) in on_edge.iter_mut().enumerate() {
                let comp = resolved.point(ip);
                let par = comp[0];
                let prp = comp[1];
                if !is_bad(par)
                    && !is_bad(prp)
                    && par > -wid
                    && par < parmax
                    && prp > -wid
                    && prp < wid
                {
                    *on = true;
                }
            }
            start = end.clone();
        }

        // On the boundary iff every supplied point lies on some edge. The
        // per-point `on_edge` flags are the boundary mask.
        let result = on_edge.iter().all(|&b| b);
        Ok((result, on_edge))
    }

    fn negated_copy(&self) -> DynRegion {
        let mut copy = self.clone();
        copy.data.negate();
        Box::new(copy)
    }

    fn negate(&mut self) {
        self.data.negate();
    }

    fn is_negated(&self) -> bool {
        self.data.negated
    }

    fn test_negated(&self) -> bool {
        self.data.negated_set
    }

    fn set_negated_raw(&mut self, negated: bool, set: bool) {
        self.data.negated = negated;
        self.data.negated_set = set;
        self.data.invalidate_cache();
    }

    fn is_closed(&self) -> bool {
        self.data.closed
    }

    fn set_closed(&mut self, closed: bool) {
        self.data.closed = closed;
        self.data.closed_set = true;
        self.data.invalidate_cache();
    }

    fn test_closed(&self) -> bool {
        self.data.closed_set
    }

    fn set_closed_raw(&mut self, closed: bool, set: bool) {
        self.data.closed = closed;
        self.data.closed_set = set;
        self.data.invalidate_cache();
    }

    fn fill_factor(&self) -> f64 {
        self.data.effective_fill_factor()
    }

    fn set_fill_factor(&mut self, factor: f64) -> AstResult<()> {
        self.data.set_fill_factor(factor)
    }

    fn test_fill_factor(&self) -> bool {
        self.data.fill_factor_set
    }

    fn clear_fill_factor(&mut self) {
        self.data.clear_fill_factor();
    }

    fn mesh_size(&self) -> usize {
        self.data.effective_mesh_size()
    }

    fn set_mesh_size(&mut self, size: usize) {
        self.data.set_mesh_size(size);
    }

    fn test_mesh_size(&self) -> bool {
        self.data.mesh_size_set
    }

    fn clear_mesh_size(&mut self) {
        self.data.clear_mesh_size();
    }

    fn is_bounded(&self) -> AstResult<bool> {
        self.is_bounded_inner()
    }

    fn is_adaptive(&self) -> bool {
        self.data.adaptive
    }

    fn test_adaptive(&self) -> bool {
        self.data.adaptive_set
    }

    fn set_adaptive_raw(&mut self, adaptive: bool, set: bool) {
        self.data.adaptive = adaptive;
        self.data.adaptive_set = set;
    }

    fn has_dummy_fs(&self) -> bool {
        self.data.dummy_fs
    }

    fn set_region_fs(&mut self, frame: &dyn Frame) {
        self.data.set_region_fs(frame);
    }

    fn get_region_bounds(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        super::region_bounds_current(self)
    }

    fn region_base_box(&self) -> AstResult<(Vec<f64>, Vec<f64>)> {
        self.reg_base_box()
    }

    fn get_region_mesh(&self) -> AstResult<PointSet> {
        // Check for a cached mesh first.
        {
            let cached = self.data.cached_base_mesh.borrow();
            if let Some(mesh) = cached.as_ref() {
                return Ok(PointSet::from_rows(mesh.iter().map(|p| p.as_slice())));
            }
        }

        // Compute the mesh.
        let mesh = self.reg_base_mesh()?;
        let result = PointSet::from_rows(mesh.iter().map(|p| p.as_slice()));

        // Cache it.
        *self.data.cached_base_mesh.borrow_mut() = Some(mesh);

        Ok(result)
    }

    fn get_region_grid(&self) -> AstResult<PointSet> {
        crate::regions::data::region_base_grid(self)
    }

    fn get_region_points(&self) -> AstResult<PointSet> {
        Ok(PointSet::from_rows(
            self.data.points.iter().map(|p| p.as_slice()),
        ))
    }

    fn get_region_frame(&self) -> DynFrame {
        self.data.frameset.current().snapshot()
    }

    fn get_region_frameset(&self) -> FrameSet {
        self.data.frameset.clone()
    }

    fn region_frameset_mut(&mut self) -> &mut FrameSet {
        self.data.invalidate_cache();
        &mut self.data.frameset
    }

    fn store_uncertainty(&mut self, unc: Option<DynRegion>) {
        self.data.uncertainty = unc;
        self.data.invalidate_cache();
    }

    fn test_region_fs(&self) -> bool {
        self.data.region_fs_set
    }

    fn set_region_fs_raw(&mut self, value: bool, set: bool) {
        self.data.region_fs = value;
        self.data.region_fs_set = set;
    }

    fn region_fs_flag(&self) -> bool {
        self.data.region_fs
    }

    fn get_uncertainty(&self) -> Option<DynRegion> {
        self.data.uncertainty.clone()
    }

    fn reg_perm_axes(&mut self, perm: &[usize]) -> AstResult<()> {
        self.perm_axes(perm)
    }

    fn reg_frame_attrib<'a>(
        &mut self,
        setting: &super::FrameSetting<'a>,
    ) -> AstResult<super::FrameSetting<'a>> {
        super::data::region_frame_attrib(self, setting)
    }

    fn clone_region(&self) -> DynRegion {
        Box::new(self.clone())
    }
}

// ============================================================================
// Frame trait — delegate to encapsulated FrameSet's current frame
// ============================================================================

impl Frame for Polygon {
    region_frame_attrs!();

    fn naxes(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    /// Mirrors C `Region::MatchAxesX` (region.c:6413): the axes of the
    /// current Frame of the Region's FrameSet are matched.
    fn match_axes_x(&self, frm1: &dyn Frame) -> AstResult<Vec<i32>> {
        self.data.frameset.current().match_axes_x(frm1)
    }

    /// Mirrors C `Region::Match` (region.c:6214): the current Frame of the
    /// Region's FrameSet is matched, through `astMatch` and so with its
    /// cast retry.
    fn match_template(
        &self,
        target: &dyn Frame,
        match_sub: bool,
    ) -> AstResult<Option<crate::frames::find_frame::MatchResult>> {
        crate::frames::find_frame::match_with_subclass(
            self.data.frameset.current(),
            target,
            match_sub,
        )
    }

    fn primary_frame(&self, axis: usize) -> (&dyn Frame, usize) {
        self.data.frameset.current().primary_frame(axis)
    }

    fn format(&self, axis: usize, value: f64) -> AstResult<String> {
        self.data.frameset.current().format(axis, value)
    }

    fn unformat(&self, axis: usize, text: &str) -> AstResult<(f64, usize)> {
        self.data.frameset.current().unformat(axis, text)
    }

    fn distance(&self, a: &[f64], b: &[f64]) -> AstResult<f64> {
        self.data.frameset.current().distance(a, b)
    }

    fn angle(&self, a: &[f64], b: &[f64], c: &[f64]) -> AstResult<f64> {
        self.data.frameset.current().angle(a, b, c)
    }

    fn offset(&self, from: &[f64], to: &[f64], dist: f64) -> AstResult<Vec<f64>> {
        self.data.frameset.current().offset(from, to, dist)
    }

    fn norm(&self, coords: &mut [f64]) -> AstResult<()> {
        self.data.frameset.current().norm(coords)
    }

    /// A Region's axes are its encapsulated current Frame's axes. Mirrors
    /// C `Region::AxDistance` / `AxOffset` / `AxNorm` (region.c:1457, 1610,
    /// 1536), which all forward to that Frame.
    fn get_axis(&self, axis: usize) -> AxisKind {
        self.data.frameset.current().get_axis(axis)
    }

    /// region.c `RemoveRegions`: a Region becomes its Frame.
    fn frame_without_regions(&self) -> Option<DynFrame> {
        Some(self.get_region_frame())
    }

    fn snapshot(&self) -> DynFrame {
        Box::new(self.clone())
    }

    /// C has one `Simplify` slot, so a Region simplifies through its Mapping
    /// side and the caller keeps the result because it is still a Frame
    /// (`astIsAFrame`, `cmpframe.c:9024`).
    fn simplify_frame(&self) -> AstResult<Option<DynFrame>> {
        simplify_frame_via_mapping(self)
    }

    fn map_from(&self, previous: &dyn Frame) -> AstResult<DynMapping> {
        self.data.frameset.current().map_from(previous)
    }

    fn convert_to(&self, to: &dyn Frame, domains: &[&str]) -> AstResult<Option<FrameSet>> {
        self.data.frameset.current().convert_to(to, domains)
    }

    fn equal(&self, other: &dyn Frame) -> bool {
        if let Some(other_poly) = other.as_any().downcast_ref::<Polygon>() {
            self.data
                .frameset
                .current()
                .equal(other_poly.data.frameset.current())
        } else {
            false
        }
    }
}

// ============================================================================
// Mapping trait — delegate to region_transform
// ============================================================================

impl Mapping for Polygon {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.data.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.data.mapping_flags
    }

    /// A Region is a Frame, which C's frame.c `GetIsLinear` reports as
    /// linear.
    fn is_linear(&self) -> bool {
        true
    }

    /// region.c `RemoveRegions`: a Region becomes its Frame, which as a
    /// Mapping is a UnitMap with its number of axes.
    fn mapping_without_regions(&self) -> Option<crate::mappings::DynMapping> {
        Some(Box::new(crate::mappings::UnitMap::new(self.nin())))
    }

    /// region.c `ReportPoints`: the current Frame formats the values.
    fn report_points(&self, _forward: bool, inp: &PointSet, out: &PointSet) {
        let frame = self.data.frameset.current();
        crate::mappings::report::write_report(&crate::frames::frame_report_lines(
            frame, frame, inp, out,
        ));
    }

    region_mapping_ident!();

    // C's `Frame` overrides `astDoNotSimplify` to zero (frame.c:3511-3554): a
    // set `Ident` is a user label on a coordinate system, not a request to keep
    // a Mapping intact, so a named Frame is still simplified.
    fn do_not_simplify(&self) -> bool {
        false
    }

    fn nin(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    fn nout(&self) -> usize {
        self.data.frameset.current().naxes()
    }

    fn is_inverted(&self) -> bool {
        false
    }

    fn invert_orientation(&mut self) {
        // A Region's forward and inverse transforms are identical (membership
        // is direction-independent), so inverting has no effect on behaviour.
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        super::data::region_transform(self, pts)
    }

    fn transform_into(&self, pts: &PointSet, forward: bool, out: &mut PointSet) -> AstResult<()> {
        crate::mappings::validate_transform(self, pts, forward, Some(out))?;
        let result = self.transform(pts, forward)?;
        let npoint = result.npoint();
        let ncoord = result.ncoord();
        for i in 0..npoint {
            let src = result.point(i);
            let mut dst = out.point_mut(i);
            dst[..ncoord].copy_from_slice(&src);
        }
        Ok(())
    }

    fn simplified(&self, _gate: ViaSimplify) -> AstResult<(DynMapping, bool)> {
        // Invoke the parent Region simplify first (polygon.c:5365); it
        // simplifies the encapsulated FrameSet and any explicit uncertainty.
        // All further work is done on the returned copy.
        let parent = self.parent_region_simplify()?;
        let new_owned;
        let this: &Polygon = match &parent {
            Some(p) => {
                new_owned = p
                    .as_ref()
                    .as_any()
                    .downcast_ref::<Polygon>()
                    .expect("parent_region_simplify preserves the Region class")
                    .clone();
                &new_owned
            }
            None => self,
        };
        let simpler = parent.is_some();

        // C's tail at polygon.c:5455: with no simplification of any kind the
        // supplied Region is returned as it stands; otherwise the parent's copy
        // takes the supplied Region's attributes and is returned.
        let keep_or_overlay = |new: &Polygon, simpler: bool| -> AstResult<(DynMapping, bool)> {
            if !simpler {
                return Ok((self.clone_box(), false));
            }
            let mut result = new.clone_region();
            result.reg_overlay(self, true)?;
            Ok((result.clone_box(), true))
        };

        let map = this
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;
        // Re-express the Polygon in its current Frame only when the base->
        // current Mapping is not a UnitMap and the current Frame is 2-D
        // (polygon.c:5379).
        if map.as_any().is::<UnitMap>() || map.nout() != 2 {
            return keep_or_overlay(this, simpler);
        }

        let current_frame = this.data.frameset.current().snapshot();
        // The uncertainty is taken in the *current* Frame (polygon.c:5384),
        // unconditionally: a Polygon with none gets its default here, and the
        // reg_overlay below is what puts the "none" back.
        let unc = this.get_unc_frm(FrameRef::Current)?;

        // C transforms the *supplied* Region's vertices through the *supplied*
        // Region's base->current Mapping (astRegTransform( this, this->points,
        // ... ), polygon.c:5387).
        let self_map = self
            .data
            .frameset
            .mapping(FrameRef::Base, FrameRef::Current)?;
        let pts = PointSet::from_rows(self.data.points.iter().map(|p| p.as_slice()));
        let transformed = self_map.transform(&pts, true)?;
        let naxes = current_frame.naxes();
        let mut new_points: Vec<Vec<f64>> = Vec::with_capacity(self.data.points.len());
        for i in 0..self.data.points.len() {
            let p = transformed.point(i);
            // C clears `ok` if any transformed vertex value is bad, on either
            // axis, and keeps the original Polygon in that case
            // (polygon.c:5402-5411).
            if is_bad(p[0]) || is_bad(p[1]) {
                return keep_or_overlay(this, simpler);
            }
            new_points.push(p[..naxes].to_vec());
        }

        // Assemble the replacement the way C's constructor does
        // (astInitPolygon, polygon.c:7120-7165): defining points first, then
        // the uncertainty through SetUnc, and only then the winding-order
        // correction — SetUnc re-centres the uncertainty on the first defining
        // point, so it must run before any reversal of the vertex order.
        let mut newpol = Polygon {
            data: RegionData::new(FrameSet::new(current_frame), new_points),
            simp_vertices: None,
        };
        newpol.set_uncertainty(unc)?;
        newpol.ensure_inside()?;

        // With SimpVertices zero, check that the transformation did not bend
        // the edges significantly: mesh the Polygon as it stands and require
        // every mesh point to lie on the replacement's boundary
        // (polygon.c:5412-5427).
        // C reads the attribute off the *supplied* Polygon (`this` at
        // polygon.c:5415), which is `self` here.
        if !self.simp_vertices() {
            let base_mesh = this.reg_base_mesh()?;
            let base_mesh_ps = PointSet::from_rows(base_mesh.iter().map(|p| p.as_slice()));
            let mesh = map.transform(&base_mesh_ps, true)?;
            if !newpol.reg_pins(&mesh, None)? {
                return keep_or_overlay(this, simpler);
            }
        }

        // A new Polygon was built from the remapped vertices, which is where C
        // sets `simpler = 1` unconditionally (polygon.c:5436).
        let mut result: DynRegion = Box::new(newpol);
        result.reg_overlay(self, true)?;
        Ok((result.clone_box(), true))
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        crate::mappings::rate::numeric_rate(self, at, ax1, ax2)
    }

    /// region.c `Equal`, which this class does not extend.
    fn equal(&self, other: &dyn Mapping) -> bool {
        super::data::region_equal(self, other).is_some()
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}
