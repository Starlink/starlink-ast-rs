// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The border round the regions of valid coordinates.
// C: plot.c Border (:5062), Boundary (:4870) and TraceBorder (:27696).
// A current Frame that is a Region is traced by DrawRegion (:9583), in
// curve.rs, where its class can trace its boundary.

use super::Plot;
use super::elements::PlotElement;
use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::FrameRef;
use crate::grf::{Grf, GrfPrimitive};
use crate::mappings::Mapping;
use crate::math::{powf, sqrt};
use crate::regions::{any_as_region, remove_regions};

impl Plot {
    // C: Border (plot.c:5062).
    pub(crate) fn border_impl(&mut self, grf: &mut dyn Grf) -> AstResult<bool> {
        self.reset_boxp();
        let naxes = Mapping::nin(self);
        if naxes != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "astBorder(Plot): Number of axes ({naxes}) in the base Frame of the supplied \
                     Plot is invalid - this number should be 2."
                ),
            ));
        }
        match self.fset_2d_current()? {
            Some(mut plot) => {
                self.lend_draw_state(&mut plot);
                let result = plot.border_2d(grf);
                self.return_draw_state(&mut plot);
                result
            }
            None => self.border_2d(grf),
        }
    }

    /// The rest of `border_impl`, on a Plot with a current Frame of no more
    /// than two axes.
    fn border_2d(&mut self, grf: &mut dyn Grf) -> AstResult<bool> {
        let naxes = Mapping::nout(self);
        if naxes != 2 {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "astBorder(Plot): Number of axes ({naxes}) in the current Frame of the \
                     supplied Plot is invalid - this number should be 2."
                ),
            ));
        }
        self.reset_grf();
        self.grf_attrs(grf, PlotElement::Border, true, GrfPrimitive::Line)?;
        let (xlo, xhi, ylo, yhi) = (self.xlo, self.xhi, self.ylo, self.yhi);
        self.line_plot(grf, xlo, ylo, xhi, ylo, true, "astBorder")?;
        self.line_plot(grf, xhi, ylo, xhi, yhi, true, "astBorder")?;
        self.line_plot(grf, xhi, yhi, xlo, yhi, true, "astBorder")?;
        self.line_plot(grf, xlo, yhi, xlo, ylo, true, "astBorder")?;
        let inval = self.boundary(grf)?;
        self.fpoly(grf)?;
        self.grf_attrs(grf, PlotElement::Border, false, GrfPrimitive::Line)?;
        Ok(inval)
    }

    /// Draw the boundary between valid and invalid physical coordinates
    /// through the plotting area, returning whether any invalid ones were
    /// found.
    // C: Boundary (plot.c:4870). C disables astRate while tracing, for
    // speed; the port's Mappings have no such switch.
    fn boundary(&mut self, grf: &mut dyn Grf) -> AstResult<bool> {
        if self.draw_region(grf, "astBorder")? {
            return Ok(false);
        }
        let mut tol = 0.25 * self.tol();
        // C: `(int) 4*pow( tol, power ) + 10`, which casts only the 4.
        let mut dim = (4.0 * powf(tol, -0.666666666) + 10.0) as i32;
        dim = dim.clamp(3, 400);
        // C measures both from the x extent.
        let dx = (self.xhi - self.xlo).abs();
        let dy = (self.xhi - self.xlo).abs();
        tol *= if dx > dy { dx } else { dy };

        // For a Region, a grid as fine over the graphics bounds of the
        // Region as the usual grid is over the plotting area.
        let (lbnd, ubnd) = if let Some(region) = any_as_region(self.frameset.current().as_any()) {
            let mut map = self.frameset.mapping(FrameRef::Base, FrameRef::Current)?;
            map.invert();
            let rmap = remove_regions(map.as_ref());
            let breg = region.reg_map_region(rmap.as_ref(), self.frameset.base())?;
            let (lbnd, ubnd) = breg.get_region_bounds()?;
            let rat = ((ubnd[0] - lbnd[0]) * (ubnd[1] - lbnd[1]))
                / ((self.xhi - self.xlo) * (self.yhi - self.ylo));
            dim = ((sqrt(rat) * f64::from(dim)) as i32).clamp(3, 2000);
            ([lbnd[0], lbnd[1]], [ubnd[0], ubnd[1]])
        } else {
            ([self.xlo, self.ylo], [self.xhi, self.yhi])
        };
        let mut edges = [false; 4];
        self.trace_border(
            lbnd[0],
            ubnd[0],
            lbnd[1],
            ubnd[1],
            dim as usize,
            tol,
            &mut edges,
        )
    }

    /// Draw the boundary between valid and invalid physical coordinates in
    /// the graphics box `xlo..xhi`, `ylo..yhi`, sampled on a `dim` by `dim`
    /// grid, cells larger than `tol` being traced again more finely.
    /// `edges` returns which edges of the box (left, top, right, bottom)
    /// the boundary leaves through.
    // C: TraceBorder (plot.c:27696).
    #[allow(clippy::too_many_arguments)]
    fn trace_border(
        &mut self,
        xlo: f64,
        xhi: f64,
        ylo: f64,
        yhi: f64,
        dim: usize,
        tol: f64,
        edges: &mut [bool; 4],
    ) -> AstResult<bool> {
        *edges = [false; 4];
        let mut result = false;
        let (_, phys) = self.make_grid(None, false, dim, xlo, xhi, ylo, yhi, false)?;
        let (px, py) = (phys.axis(0), phys.axis(1));
        let bad = |k: usize| px[k] == BAD_VALUE || py[k] == BAD_VALUE;
        let ncell = dim - 1;
        let totcells = ncell * ncell;
        let dx = (xhi - xlo) / ncell as f64;
        let dy = (yhi - ylo) / ncell as f64;
        let mut recurse = dx > tol || dy > tol;
        let mut bndry = vec![false; if recurse { totcells } else { 0 }];
        let mut drawn = vec![false; if recurse { totcells } else { 0 }];
        let mut cylo = ylo;
        let mut cyhi = ylo + dy;
        let lastcell = ncell - 1;
        let mut icell = 0;
        for irow in 0..ncell {
            // The grid point at the bottom left of the row's first cell,
            // and the one above it.
            let row = irow * dim;
            let mut bad1 = bad(row);
            let mut bad2 = bad(row + dim);
            for icol in 0..ncell {
                let bad3 = bad(row + icol + 1);
                let bad4 = bad(row + dim + icol + 1);
                if bad1 || bad2 || bad3 || bad4 {
                    result = true;
                }
                if bad2 != bad1 || bad3 != bad1 || bad4 != bad1 {
                    if recurse {
                        bndry[icell] = true;
                    } else {
                        let cxlo = xlo + icol as f64 * dx;
                        let cxhi = cxlo + dx;
                        let xc = (0.5 * (cxlo + cxhi)) as f32;
                        let yc = (0.5 * (cylo + cyhi)) as f32;
                        if bad1 == bad4 && bad2 == bad3 {
                            self.bpoly(cxlo as f32, yc);
                            self.apoly(cxhi as f32, yc);
                            self.bpoly(xc, cylo as f32);
                            self.apoly(xc, cyhi as f32);
                        } else if bad1 != bad2 {
                            if icol == 0 {
                                self.bpoly(cxlo as f32, yc);
                            }
                            if bad2 != bad4 {
                                self.apoly(xc, cyhi as f32);
                            } else if bad3 != bad4 {
                                self.apoly(cxhi as f32, yc);
                            } else {
                                self.apoly(xc, cylo as f32);
                            }
                        } else if bad4 != bad2 {
                            if bad3 != bad4 {
                                self.bpoly(xc, cyhi as f32);
                                self.apoly(cxhi as f32, yc);
                            } else {
                                self.bpoly(xc, cyhi as f32);
                                self.apoly(xc, cylo as f32);
                            }
                        } else {
                            self.bpoly(xc, cylo as f32);
                            self.apoly(cxhi as f32, yc);
                        }
                        if icol == 0 {
                            if bad1 != bad2 {
                                edges[0] = true;
                            }
                        } else if icol == lastcell && bad3 != bad4 {
                            edges[2] = true;
                        }
                        if irow == 0 {
                            if bad1 != bad3 {
                                edges[3] = true;
                            }
                        } else if irow == lastcell && bad2 != bad4 {
                            edges[1] = true;
                        }
                    }
                }
                bad1 = bad3;
                bad2 = bad4;
                icell += 1;
            }
            cylo = cyhi;
            cyhi = cylo + dy;
        }
        let mut sedges = [false; 4];
        while recurse {
            recurse = false;
            cylo = ylo;
            cyhi = ylo + dy;
            icell = 0;
            for irow in 0..ncell {
                for icol in 0..ncell {
                    if bndry[icell] && !drawn[icell] {
                        let cxlo = xlo + icol as f64 * dx;
                        let cxhi = cxlo + dx;
                        if self.trace_border(cxlo, cxhi, cylo, cyhi, 3, tol, &mut sedges)? {
                            result = true;
                        }
                        if sedges[0] {
                            if icol == 0 {
                                edges[0] = true;
                            } else if !bndry[icell - 1] {
                                bndry[icell - 1] = true;
                                recurse = true;
                            }
                        }
                        if sedges[1] {
                            if irow == lastcell {
                                edges[1] = true;
                            } else {
                                bndry[icell + ncell] = true;
                            }
                        }
                        if sedges[2] {
                            if icol == lastcell {
                                edges[2] = true;
                            } else {
                                bndry[icell + 1] = true;
                            }
                        }
                        if sedges[3] {
                            if irow == 0 {
                                edges[3] = true;
                            } else if !bndry[icell - ncell] {
                                bndry[icell - ncell] = true;
                                recurse = true;
                            }
                        }
                        drawn[icell] = true;
                    }
                    icell += 1;
                }
                cylo += dy;
                cyhi = cylo + dy;
            }
        }
        Ok(result)
    }

    /// A copy of the Plot whose current Frame has no more than two axes,
    /// or `None` if its current Frame already has no more than two.
    // C: Fset2D( this, AST__CURRENT ), which returns the Plot itself in the
    // second case.
    pub(crate) fn fset_2d_current(&self) -> AstResult<Option<Plot>> {
        if self.frameset.current().naxes() <= 2 {
            return Ok(None);
        }
        let fs = super::fset_2d(self.frameset.clone(), false)?;
        Ok(Some(self.with_frameset(fs)))
    }
}
