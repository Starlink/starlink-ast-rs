// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Regular grids of graphics positions and the physical positions they map
//! to, which the border tracer and tick placement sample a Plot with.
// C: plot.c GraphGrid (:17130) and MakeGrid (:21477).

use super::Plot;
use crate::coords::PointSet;
use crate::error::AstResult;
use crate::frames::Frame;

/// A `dim` by `dim` grid of graphics positions covering `xlo..xhi` and
/// `ylo..yhi`, row by row, leaving out the corners outside a disk when
/// `disk` is set.
// C: GraphGrid (plot.c:17130).
fn graph_grid(dim: usize, disk: bool, xlo: f64, xhi: f64, ylo: f64, yhi: f64) -> PointSet {
    let dx = (xhi - xlo) / (dim - 1) as f64;
    let dy = (yhi - ylo) / (dim - 1) as f64;
    let cen = 0.5 * (dim - 1) as f64;
    let r2 = 1.9 * cen * cen;
    let mut px = Vec::with_capacity(dim * dim);
    let mut py = Vec::with_capacity(dim * dim);
    for j in 0..dim {
        let mut dy2 = j as f64 - cen;
        dy2 *= dy2;
        let y = ylo + j as f64 * dy;
        for i in 0..dim {
            let ok = if disk {
                let mut dx2 = i as f64 - cen;
                dx2 *= dx2;
                dx2 + dy2 <= r2
            } else {
                true
            };
            if ok {
                px.push(xlo + i as f64 * dx);
                py.push(y);
            }
        }
    }
    PointSet::from_columns(&[&px, &py]).expect("two equal columns")
}

impl Plot {
    /// A grid of graphics positions (see `graph_grid`) and the physical
    /// positions they map to, normalised in `frm` (the current Frame if
    /// `None`) when `norm` is set.
    // C: MakeGrid (plot.c:21477), with `map` always the Plot's base to
    // current Mapping and `nphy` its number of outputs.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn make_grid(
        &self,
        frm: Option<&dyn Frame>,
        disk: bool,
        dim: usize,
        xlo: f64,
        xhi: f64,
        ylo: f64,
        yhi: f64,
        norm: bool,
    ) -> AstResult<(PointSet, PointSet)> {
        let graph = graph_grid(dim, disk, xlo, xhi, ylo, yhi);
        let phys = self.trans_with(frm, &graph, true, norm)?;
        Ok((graph, phys))
    }
}
