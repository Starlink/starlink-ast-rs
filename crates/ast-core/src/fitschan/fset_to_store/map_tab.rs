//! Describing WCS axes with the `-TAB` algorithm of FITS-WCS paper III:
//! whether the pixel to WCS Mapping of an axis can be expressed as a
//! look-up table, and the FitsTable columns that hold it.
//!
//! Ported from `IsMapTab1D`, `IsMapTab2D`, `AdaptLut` and
//! `ChangePermSplit` in `reference/starlink-ast/src/fitschan.c`.

use crate::coords::PointSet;
use crate::error::AstResult;
use crate::fitstable::FitsTable;
use crate::frames::Frame;
use crate::keymap::AstType;
use crate::mappings::simplify::flatten;
use crate::mappings::{
    CmpMap, Composition, DynMapping, LutMap, Mapping, PermMap, TranMap, UnitMap,
};

/// The name of the FITS binary table extension a write puts its look-up
/// tables in (fitschan.c `AST_TABEXTNAME`).
pub(crate) const TABEXTNAME: &str = "WCS-TAB";

/// What the axis writers need for the `-TAB` algorithm.
#[derive(Clone, Copy)]
pub(crate) struct TabWrite<'a> {
    /// TabOK: `-TAB` may be used when positive, and is the table's EXTVER.
    pub tab_ok: i32,
    /// ForceTab: use `-TAB` even where another algorithm would do.
    pub force_tab: bool,
    /// The table every `-TAB` axis of the write shares (C's
    /// `store->tables`).
    pub tables: &'a std::cell::RefCell<Option<FitsTable>>,
}

/// Run `f` on the write's table, as C's axis writers do: a table already
/// stored is changed in place, whatever `f` returns, and otherwise `f` is
/// given none, and any table it makes is stored only when it returns a
/// value.
pub(crate) fn with_table<T>(
    tab: TabWrite<'_>,
    f: impl FnOnce(&mut Option<FitsTable>) -> AstResult<Option<T>>,
) -> AstResult<Option<T>> {
    let mut stored = tab.tables.borrow_mut();
    if stored.is_some() {
        return f(&mut stored);
    }
    let mut table = None;
    let result = f(&mut table)?;
    if result.is_some() {
        *stored = table;
    }
    Ok(result)
}

/// A WCS axis described by a look-up table, from [`is_map_tab_1d`].
pub(crate) struct TabAxis {
    /// The Mapping from the WCS axis to the grid axis that feeds it.
    pub map: DynMapping,
    /// The one-based index of the table column holding the coordinate
    /// array.
    pub icolmain: usize,
    /// The one-based index of the column holding the index vector, when
    /// the grid coordinates of the table entries are not 1, 2, 3, ...
    pub icolindex: Option<usize>,
    /// The LutMap interpolation method: 0 linear, 1 nearest neighbour.
    pub interp: i32,
}

/// The values of the one-input, one-output `map` at `xs`, forward or
/// inverse (C's `astTran1`).
fn tran1(map: &dyn Mapping, xs: &[f64], forward: bool) -> AstResult<Vec<f64>> {
    let pts = PointSet::from_rows(xs.iter().map(|&x| [x]));
    let out = map.transform(&pts, forward)?;
    Ok((0..xs.len()).map(|i| out.point(i)[0]).collect())
}

/// C's `AdaptLut`: append to `xtab` and `vtab` samples of the 1-D `map`
/// over (`x0`, `x1`], `npos` interior samples at a time, subdividing an
/// interval wherever the Mapping departs from the straight line between its
/// ends by more than `eps` in input.
#[allow(clippy::too_many_arguments)]
fn adapt_lut(
    map: &dyn Mapping,
    npos: usize,
    eps: f64,
    mut x0: f64,
    x1: f64,
    mut v0: f64,
    v1: f64,
    xtab: &mut Vec<f64>,
    vtab: &mut Vec<f64>,
) -> AstResult<()> {
    let dx = (x1 - x0) / (npos + 1) as f64;
    let xx0 = x0 + dx;
    let xx: Vec<f64> = (0..npos).map(|ipos| xx0 + ipos as f64 * dx).collect();
    let vv = tran1(map, &xx, true)?;

    let subdivide = if v0 == v1 {
        vv.iter().any(|&v| v != v0)
    } else {
        let rg = (x1 - x0) / (v1 - v0);
        vv.iter().zip(&xx).any(|(&v, &x)| {
            v == crate::constants::BAD_VALUE || (rg * (v - v0) - (x - x0)).abs() > eps
        })
    };

    if subdivide {
        for ipos in 0..npos {
            adapt_lut(map, npos, eps, x0, xx[ipos], v0, vv[ipos], xtab, vtab)?;
            x0 = xx[ipos];
            v0 = vv[ipos];
        }
        adapt_lut(map, npos, eps, x0, x1, v0, v1, xtab, vtab)?;
    } else {
        xtab.extend_from_slice(&xx);
        vtab.extend_from_slice(&vv);
        xtab.push(x1);
        vtab.push(v1);
    }
    Ok(())
}

/// C's `ChangePermSplit`: set `PermSplit` on every PermMap in `map`, looking
/// inside the Mappings C's `astDecompose` breaks into components.
fn change_perm_split(map: &mut dyn Mapping) {
    if let Some(pm) = map.as_any_mut().downcast_mut::<PermMap>() {
        pm.set_perm_split(true);
    } else if let Some(cmp) = map.as_any_mut().downcast_mut::<CmpMap>() {
        change_perm_split(cmp.first_mut().as_mut());
        change_perm_split(cmp.second_mut().as_mut());
    } else if let Some(tm) = map.as_any_mut().downcast_mut::<TranMap>() {
        let (map1, map2) = tm.components_mut();
        change_perm_split(map1.as_mut());
        change_perm_split(map2.as_mut());
    }
}

/// `map` with its inverse used as its forward transformation.
fn inverted(map: &dyn Mapping) -> DynMapping {
    let mut inv = map.clone_box();
    inv.invert();
    inv
}

/// The grid input of the pixel to WCS Mapping `map` that alone feeds output
/// `iax`, and the Mapping from that output to that input: the split of the
/// inverse Mapping, failing that the split of each input in turn, and
/// failing that the split of the inverse with every PermMap splitting its
/// inverse transformation (fitschan.c `IsMapTab1D`).
fn split_output(map: &dyn Mapping, iax: usize) -> AstResult<Option<(Vec<usize>, DynMapping)>> {
    if let Some(split) = inverted(map).map_split_zero_based(&[iax])? {
        return Ok(Some(split));
    }

    for iin in 0..map.nin() {
        let Some((outs, ret)) = map.map_split_zero_based(&[iin])? else {
            continue;
        };
        let nout = ret.nout();
        let Some(iout) = outs[..nout].iter().position(|&o| o == iax) else {
            continue;
        };
        // A PermMap that selects output `iout` of `ret`, so that the
        // whole, inverted, maps the WCS axis to grid input `iin`. C's
        // internal arrays are 0-based with -1 for no input; the
        // constructor here is 1-based, where -1 names a constant there
        // is none of, which gives a bad value as C's does.
        let mut inperm = vec![-1i64; nout];
        inperm[iout] = 1;
        let outperm = [iout as i64 + 1];
        let pm = PermMap::new(&inperm, &outperm, &[])?;
        let mut cm: DynMapping = Box::new(CmpMap::new(ret, Box::new(pm), Composition::Series)?);
        cm.invert();
        return Ok(Some((vec![iin], cm)));
    }

    let mut tmap = inverted(map);
    change_perm_split(tmap.as_mut());
    tmap.map_split_zero_based(&[iax])
}

/// C's `IsMapTab1D`: whether output `iax` of the pixel to WCS Mapping `map`
/// can be described by a look-up table, and if so the table columns for
/// FITS-WCS axis `iwcs` (0-based), added to `table` (made if need be).
///
/// The output must be fed by a single grid axis. When the Mapping from
/// that axis contains a LutMap, its table is used, transformed by what
/// follows it and indexed by what precedes it; otherwise the Mapping is
/// sampled adaptively across the `dim` pixels of the grid axis, when that
/// is known. Coordinates are multiplied by `scale`, and the column has
/// unit `unit`, or the unit of the WCS axis.
#[allow(clippy::too_many_arguments)]
pub(crate) fn is_map_tab_1d(
    map: &dyn Mapping,
    scale: f64,
    unit: Option<&str>,
    wcsfrm: &dyn Frame,
    dim: &[Option<f64>],
    iax: usize,
    iwcs: usize,
    table: &mut Option<FitsTable>,
) -> AstResult<Option<TabAxis>> {
    let unit = match unit {
        Some(u) => u.to_string(),
        None => wcsfrm.unit(iax + 1)?,
    };

    let Some((ins, ret)) = split_output(map, iax)? else {
        return Ok(None);
    };
    if ret.nout() != 1 {
        return Ok(None);
    }

    // The Mapping from the grid axis to the WCS axis.
    let mut ret = inverted(ret.as_ref());
    let colname = format!("COORDS{}", iwcs + 1);
    let cellname = format!("{colname}(1)");
    let indexname = format!("INDEX{}", iwcs + 1);

    // Its series components, each with the direction it is used in.
    let list = flatten(ret.as_ref(), false, true)?;
    let ilut = list
        .iter()
        .position(|(m, inv, _)| !inv && m.as_any().is::<LutMap>());

    let (icolmain, icolindex, interp) = if let Some(ilut) = ilut {
        // The Mappings before and after the LutMap, each combined in
        // series behind a UnitMap.
        let combine = |range: &[(DynMapping, bool, bool)]| -> AstResult<DynMapping> {
            let mut combined: DynMapping = Box::new(UnitMap::new(1));
            for (m, inv, _) in range {
                let m = if *inv {
                    inverted(m.as_ref())
                } else {
                    m.clone_box()
                };
                combined = Box::new(CmpMap::new(combined, m, Composition::Series)?);
            }
            Ok(combined)
        };
        let premap = combine(&list[..ilut])?;
        let postmap = combine(&list[ilut + 1..])?;

        let lutmap = list[ilut]
            .0
            .as_any()
            .downcast_ref::<LutMap>()
            .expect("found as a LutMap");
        let lut = lutmap.lut().to_vec();
        let (start, inc, nlut) = (lutmap.start(), lutmap.inc(), lut.len());
        let interp = match lutmap.interp() {
            crate::mappings::LutInterp::Linear => 0,
            crate::mappings::LutInterp::Nearest => 1,
        };

        if table.is_none() {
            *table = Some(FitsTable::new()?);
        }
        let table = table.as_mut().expect("just made");
        table.add_column(&colname, AstType::Double, &[1, nlut], &unit)?;
        let icolmain = table.as_table().ncolumn();

        // The WCS values of the table entries, in FITS units.
        let coords: Vec<f64> = tran1(postmap.as_ref(), &lut, true)?
            .into_iter()
            .map(|v| v * scale)
            .collect();
        table.put_1d(&cellname, &coords, None)?;

        // The grid coordinates of the table entries, which need an index
        // vector unless they are 1, 2, 3, ...
        let centres: Vec<f64> = (0..nlut).map(|i| start + i as f64 * inc).collect();
        let grid = tran1(premap.as_ref(), &centres, false)?;
        let icolindex = add_index(table, &indexname, &grid)?;
        (icolmain, icolindex, interp)
    } else if let Some(d) = dim.get(ins[0]).copied().flatten() {
        // Sample the Mapping across the pixels of the grid axis, more
        // densely where it changes fastest, to within 0.25 pixel.
        let x = [1.0, d];
        let v = tran1(ret.as_ref(), &x, true)?;
        let mut work1 = vec![x[0]];
        let mut work2 = vec![v[0]];
        adapt_lut(
            ret.as_ref(),
            3,
            0.25,
            x[0],
            x[1],
            v[0],
            v[1],
            &mut work1,
            &mut work2,
        )?;
        let nlut = work1.len();

        if table.is_none() {
            *table = Some(FitsTable::new()?);
        }
        let table = table.as_mut().expect("just made");
        table.add_column(&colname, AstType::Double, &[1, nlut], &unit)?;
        let icolmain = table.as_table().ncolumn();
        let coords: Vec<f64> = work2.iter().map(|v| v * scale).collect();
        table.put_1d(&cellname, &coords, None)?;
        let icolindex = add_index(table, &indexname, &work1)?;
        (icolmain, icolindex, 0)
    } else {
        return Ok(None);
    };

    // The Mapping from the WCS axis to the grid axis.
    ret.invert();
    Ok(Some(TabAxis {
        map: ret,
        icolmain,
        icolindex,
        interp,
    }))
}

/// Add the index vector `grid` to `table` as column `name`, unless it is a
/// unit index (1, 2, 3, ... to within 1.0E-6), returning the column's
/// one-based index.
fn add_index(table: &mut FitsTable, name: &str, grid: &[f64]) -> AstResult<Option<usize>> {
    let unit_index = grid
        .iter()
        .enumerate()
        .all(|(i, &g)| (g - i as f64 - 1.0).abs() <= 1.0E-6);
    if unit_index {
        return Ok(None);
    }
    table.add_column(name, AstType::Double, &[grid.len()], " ")?;
    let icol = table.as_table().ncolumn();
    table.put_1d(&format!("{name}(1)"), grid, None)?;
    Ok(Some(icol))
}

/// Two WCS axes described by look-up tables, from [`is_map_tab_2d`].
pub(crate) struct TabAxes {
    /// The Mapping from the two WCS axes to the grid axes that feed them.
    pub map: DynMapping,
    /// The first axis's table columns and interpolation method.
    pub first: TabAxis,
    /// The second axis's table columns and interpolation method.
    pub second: TabAxis,
}

/// C's `IsMapTab2D`: whether outputs `iax1` and `iax2` of `map` can be
/// described by look-up tables, as columns for FITS-WCS axes `iwcs1` and
/// `iwcs2`. Only outputs that are separable, each fed by a single grid axis
/// and so described by a 1-D table, are supported; for a pair that is not,
/// C leaves a 2-D coordinate array to be done.
#[allow(clippy::too_many_arguments)]
pub(crate) fn is_map_tab_2d(
    map: &dyn Mapping,
    scale: f64,
    unit: Option<&str>,
    wcsfrm: &dyn Frame,
    dim: &[Option<f64>],
    iax1: usize,
    iax2: usize,
    iwcs1: usize,
    iwcs2: usize,
    table: &mut Option<FitsTable>,
) -> AstResult<Option<TabAxes>> {
    let ret1 = is_map_tab_1d(map, scale, unit, wcsfrm, dim, iax1, iwcs1, table)?;
    let ret2 = is_map_tab_1d(map, scale, unit, wcsfrm, dim, iax2, iwcs2, table)?;

    match (ret1, ret2) {
        (Some(first), Some(second)) => {
            // The Mapping from the two WCS axes to the grid axes, split
            // from the whole if possible, adding a second grid axis when
            // both are fed by the same one.
            let mut ret: Option<DynMapping> = None;
            if let Some((_, split)) = inverted(map).map_split_zero_based(&[iax1, iax2])? {
                match split.nout() {
                    1 => {
                        let pm = PermMap::new(&[1], &[1, 1], &[])?;
                        ret = Some(Box::new(CmpMap::new(
                            split,
                            Box::new(pm),
                            Composition::Series,
                        )?));
                    }
                    2 => ret = Some(split),
                    _ => {}
                }
            }
            // Otherwise the two 1-D Mappings side by side. They may not be
            // independent, which is why the split is tried first.
            let map = match ret {
                Some(m) => m,
                None => Box::new(CmpMap::new(
                    first.map.clone_box(),
                    second.map.clone_box(),
                    Composition::Parallel,
                )?),
            };
            Ok(Some(TabAxes { map, first, second }))
        }
        // Only one output is separable: its columns are removed again.
        (Some(only), None) | (None, Some(only)) => {
            remove_columns(table, &only)?;
            Ok(None)
        }
        (None, None) => Ok(None),
    }
}

/// Remove the columns of `axis` from `table`.
fn remove_columns(table: &mut Option<FitsTable>, axis: &TabAxis) -> AstResult<()> {
    let table = table.as_mut().expect("a column was added to the table");
    let main = table.as_table().column_name(axis.icolmain)?;
    let index = axis
        .icolindex
        .map(|i| table.as_table().column_name(i))
        .transpose()?;
    if let Some(index) = index {
        table.remove_column(&index)?;
    }
    table.remove_column(&main)?;
    Ok(())
}

/// Store a 1-D `-TAB` description of WCS axis `iax` (FITS axis `fits_i`) in
/// `store` and `table`, as C's `OtherAxes` and `SpectralAxes` do, and
/// return the WCS to IWC Mapping, in which IWC are the grid coordinates of
/// the look-up: the axis's Mapping beside UnitMaps for the other `nwcs`
/// axes, after a PermMap into FITS axis order `wperm`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn store_tab_axis(
    tab_ok: i32,
    axis: TabAxis,
    table: &mut FitsTable,
    store: &mut super::super::fits_store::FitsStore,
    iax: usize,
    fits_i: usize,
    nwcs: usize,
    wperm: &[usize],
) -> AstResult<DynMapping> {
    store.ps[fits_i][0] = Some(TABEXTNAME.to_string());
    store.pv[fits_i][1] = Some(f64::from(tab_ok));
    set_extver(table, tab_ok)?;
    store.ps[fits_i][1] = Some(table.as_table().column_name(axis.icolmain)?);
    if let Some(icol) = axis.icolindex {
        store.ps[fits_i][2] = Some(table.as_table().column_name(icol)?);
    }
    store.pv[fits_i][4] = Some(f64::from(axis.interp));

    let tmap1 = super::other_axes::add_unit_maps(axis.map, iax, nwcs)?;
    Ok(Box::new(CmpMap::new(
        Box::new(fits_order_perm(wperm)?),
        tmap1,
        Composition::Series,
    )?))
}

/// The PermMap from WCS axis order to FITS axis order `wperm`.
pub(crate) fn fits_order_perm(wperm: &[usize]) -> AstResult<PermMap> {
    let n = wperm.len();
    let mut outperm = vec![0i64; n];
    let inperm: Vec<i64> = wperm.iter().map(|&w| w as i64 + 1).collect();
    for (i, &w) in wperm.iter().enumerate() {
        outperm[w] = i as i64 + 1;
    }
    PermMap::new(&inperm, &outperm, &[])
}

/// A `TabWrite` with TabOK and ForceTab unset, for unit tests.
#[cfg(test)]
pub(crate) fn no_tab() -> TabWrite<'static> {
    TabWrite {
        tab_ok: 0,
        force_tab: false,
        tables: Box::leak(Box::new(std::cell::RefCell::new(None))),
    }
}

/// Store the table version `extver` as EXTVER in the header of `table`.
/// C marks every card made during a write as new, in whichever FitsChan it
/// is made, so the card carries C's NEW flags, which the write never clears
/// from the table's header.
pub(crate) fn set_extver(table: &mut FitsTable, extver: i32) -> AstResult<()> {
    let header = table.header_mut();
    header.cards.begin_marking_new();
    let result = header.set_fits_i("EXTVER", extver, "Table version number", false);
    header.cards.end_marking_new();
    result
}
