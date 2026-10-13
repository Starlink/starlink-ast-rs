//! Reading axes described by the `-TAB` algorithm of FITS-WCS paper III,
//! whose world coordinates are looked up in a FITS binary table.
//!
//! Ported from `TabMapping`, `MakeColumnMap` and `GetNamedTable` in
//! `reference/starlink-ast/src/fitschan.c`.
//!
//! C's table source function is passed the FitsChan, and stores the table
//! it reads with `astPutTable`. Here a [`TableSource`] returns the table,
//! which the FitsChan stores under the extension name asked for, as a
//! source that follows C's documentation does.

use super::fits_store::FitsStore;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::fitstable::FitsTable;
use crate::keymap::KeyMap;
use crate::keymap::table::chr_len;
use crate::mappings::{
    CmpMap, Composition, DynMapping, LutInterp, LutMap, Mapping, PermMap, UnitMap,
};
use crate::serial::AnyObject;

/// The class and method C names in the errors of a read.
const METHOD: &str = "astRead";
const CLASS: &str = "FitsChan";

/// A source of the FITS binary tables a `-TAB` header refers to (C's
/// `astTableSource`).
///
/// [`read_table`](TableSource::read_table) is called with the extension
/// name (`EXTNAME`), version (`EXTVER`) and level (`EXTLEVEL`) of a table
/// the read needs, and returns the table, or `None` when it cannot be read.
/// Any closure of that signature is a `TableSource`.
pub trait TableSource: Send {
    /// The table in the named extension, or `None` if it cannot be read.
    fn read_table(
        &mut self,
        extname: &str,
        extver: i32,
        extlevel: i32,
    ) -> AstResult<Option<FitsTable>>;
}

impl<F> TableSource for F
where
    F: FnMut(&str, i32, i32) -> AstResult<Option<FitsTable>> + Send,
{
    fn read_table(
        &mut self,
        extname: &str,
        extver: i32,
        extlevel: i32,
    ) -> AstResult<Option<FitsTable>> {
        self(extname, extver, extlevel)
    }
}

/// What a read of `-TAB` axes uses from the FitsChan: its TabOK attribute,
/// the tables it holds, and any table source.
pub(crate) struct TabReader<'a> {
    /// The TabOK attribute; `-TAB` axes are read only when it is positive.
    pub tab_ok: i32,
    /// The FitsChan's tables (C's `tables`).
    pub tables: &'a mut Option<KeyMap>,
    /// The FitsChan's table source, if any.
    pub source: Option<&'a mut Box<dyn TableSource>>,
}

impl TabReader<'_> {
    /// C's `GetNamedTable`: ask any table source for the table, storing what
    /// it returns under `extname`, then return the table stored under
    /// `extname`. A missing table is AST__NOTAB.
    fn named_table(&mut self, extname: &str, extver: i32, extlevel: i32) -> AstResult<FitsTable> {
        if let Some(source) = self.source.as_mut()
            && let Some(table) = source.read_table(extname, extver, extlevel)?
        {
            self.tables.get_or_insert_with(KeyMap::new).put_0a(
                extname,
                AnyObject::FitsTable(Box::new(table)),
                None,
            )?;
        }
        if let Some(tables) = self.tables.as_ref()
            && let Some(Some(AnyObject::FitsTable(table))) = tables.get_0a(extname)?
        {
            return Ok(*table);
        }
        Err(AstError::new(
            ErrorCode::Notab,
            format!(
                "{METHOD}({CLASS}): Failed to read FITS binary table from extension \
                 '{extname}' (extver={extver}, extlevel={extlevel})."
            ),
        ))
    }
}

/// The result of [`tab_mapping`].
pub(crate) struct TabLookup {
    /// The Mapping from the axes the CRVAL values describe (C's psi) to the
    /// world axes, with one input and output per FITS-WCS axis.
    pub map: DynMapping,
    /// Whether each FITS-WCS axis is a `-TAB` axis.
    pub axes: Vec<bool>,
}

/// Whether `ctype` names the `-TAB` algorithm, as C tests it: longer than
/// four characters with `-TAB` at the fifth.
fn is_tab(ctype: &str) -> bool {
    ctype.len() > 4 && ctype.as_bytes()[4..].starts_with(b"-TAB")
}

/// A FITS keyword name as C's `sprintf( name, "PS%d_%d%c", ... )` makes it,
/// a space for the primary axis description included.
fn ps_name(iaxis: usize, m: usize, s: char) -> String {
    format!("PS{}_{m}{s}", iaxis + 1)
}

/// The required string parameter `m` of axis `iaxis`, or AST__NOFTS.
fn required_ps(store: &FitsStore, iaxis: usize, m: usize, s: char) -> AstResult<String> {
    store.ps[iaxis][m].clone().ok_or_else(|| {
        AstError::new(
            ErrorCode::Nofts,
            format!(
                "{METHOD}({CLASS}): No value can be found for {}.",
                ps_name(iaxis, m, s)
            ),
        )
    })
}

/// C's `TabMapping`: the Mapping that performs the look-ups of every `-TAB`
/// axis of axis description `s`, or `None` when TabOK is not positive or no
/// axis uses `-TAB`.
///
/// Each `-TAB` axis has the `-TAB` removed from its CTYPE in `store`, so
/// that the rest of the read treats it as linear. The tables used are
/// removed from the FitsChan once the Mapping is made.
pub(crate) fn tab_mapping(
    store: &mut FitsStore,
    s: char,
    reader: &mut TabReader<'_>,
) -> AstResult<Option<TabLookup>> {
    if reader.tab_ok <= 0 {
        return Ok(None);
    }
    let wcsaxes = store.naxis;
    let mut used_tables: Vec<String> = Vec::new();
    let mut tabaxis = vec![false; wcsaxes];

    // The FITS-WCS axis of each input of `tmap0`, and its inverse.
    let mut permout: Vec<i64> = vec![-1; wcsaxes];
    let mut permin: Vec<i64> = vec![-1; wcsaxes];
    let mut nperm = 0usize;
    let mut tmap0: Option<DynMapping> = None;
    let mut found = false;

    for iaxis in 0..wcsaxes {
        // An axis that shares a coordinate array with an earlier one has
        // been handled with it.
        if permin[iaxis] != -1 {
            continue;
        }
        let ctype = store.ctype[iaxis].clone();
        let tmap2: DynMapping = match ctype.as_deref().filter(|c| is_tab(c)) {
            Some(ctype) => {
                found = true;
                let extname = required_ps(store, iaxis, 0, s)?;
                let extver = store.pv[iaxis]
                    .get(1)
                    .copied()
                    .flatten()
                    .map_or(1, |v| v as i32);
                let extlevel = store.pv[iaxis]
                    .get(2)
                    .copied()
                    .flatten()
                    .map_or(1, |v| v as i32);
                let table = reader.named_table(&extname, extver, extlevel)?;
                if !used_tables.contains(&extname) {
                    used_tables.push(extname.clone());
                }

                let coordscol = required_ps(store, iaxis, 1, s)?;
                let t = table.as_table();
                if !t.has_column(&coordscol)? {
                    return Err(badtab(format!(
                        "Unable to find the coordinate array for FITS-WCS axis {} (type \
                         {ctype}): column '{coordscol}' cannot be found in table '{extname}'.",
                        iaxis + 1
                    )));
                }
                let mdim = t.column_ndim(&coordscol)?;
                if mdim == 1 {
                    return Err(badtab(format!(
                        "Unable to use the coordinate array for FITS-WCS axis {} (type \
                         {ctype}): column '{coordscol}' in table '{extname}' has one axis but \
                         at least two are required.",
                        iaxis + 1
                    )));
                }

                // The FITS-WCS axis of each dimension of the coordinate
                // array after the first, which enumerates the coordinates.
                let mut marray: Vec<Option<usize>> = vec![None; mdim];
                for (maxis, slot) in marray.iter_mut().enumerate().skip(1) {
                    for iiaxis in 0..wcsaxes {
                        if store.ps[iiaxis][0].as_deref() != Some(extname.as_str())
                            || store.ps[iiaxis][1].as_deref() != Some(coordscol.as_str())
                        {
                            continue;
                        }
                        let ival = store.pv[iiaxis]
                            .get(3)
                            .copied()
                            .flatten()
                            .map_or(1, |v| v.round() as i64);
                        if ival != maxis as i64 {
                            continue;
                        }
                        if slot.is_none() {
                            *slot = Some(iiaxis);
                        } else {
                            return Err(badtab(format!(
                                "Unable to use the coordinate array for FITS-WCS axis {} \
                                 (type {ctype}): more than one intermediate WCS axis is \
                                 mapped onto  dimension {maxis} of the coordinate array in \
                                 column '{coordscol}' of table '{extname}'.",
                                iaxis + 1
                            )));
                        }
                    }
                }
                let mut maxes = Vec::with_capacity(mdim.saturating_sub(1));
                for (maxis, slot) in marray.iter().enumerate().skip(1) {
                    match slot {
                        Some(a) => maxes.push(*a),
                        None => {
                            return Err(badtab(format!(
                                "Unable to use the coordinate array for FITS-WCS axis {} \
                                 (type {ctype}): no intermediate WCS axis is mapped onto  \
                                 dimension {maxis} of the coordinate array in column  \
                                 '{coordscol}' of table '{extname}'.",
                                iaxis + 1
                            )));
                        }
                    }
                }

                // The index vector of each dimension, as an inverted LutMap,
                // or a UnitMap for a dimension that has none.
                let mut indexmap: Option<DynMapping> = None;
                let mut unit = true;
                for &axis in &maxes {
                    let tmap1: DynMapping = match store.ps[axis][2].as_deref() {
                        Some(indexcol) => {
                            let mut map = make_column_map(&table, indexcol, true, 0)?;
                            map.invert();
                            unit = false;
                            map
                        }
                        None => Box::new(UnitMap::new(1)),
                    };
                    indexmap = Some(match indexmap {
                        Some(prev) => Box::new(CmpMap::new(prev, tmap1, Composition::Parallel)?),
                        None => tmap1,
                    });
                }

                // An extension to the published algorithm: PVi_4 (translated
                // from QVi_4) is zero for linear and otherwise nearest
                // neighbour interpolation.
                let interp = store.pv[iaxis]
                    .get(4)
                    .copied()
                    .flatten()
                    .map_or(0, |v| v.round() as i64);
                let tmap1 = make_column_map(&table, &coordscol, false, interp)?;
                let tmap2: DynMapping = match (unit, indexmap) {
                    (false, Some(indexmap)) => {
                        Box::new(CmpMap::new(indexmap, tmap1, Composition::Series)?)
                    }
                    _ => tmap1,
                };

                for &axis in &maxes {
                    permout[nperm] = axis as i64;
                    permin[axis] = nperm as i64;
                    nperm += 1;
                }

                // The axis is linear from now on: what remains maps pixel
                // coordinates to the psi values the look-up starts from.
                let name = format!("{}{}", &ctype[..4], &ctype[8..]);
                store.ctype[iaxis] = Some(name);
                tabaxis[iaxis] = true;
                tmap2
            }
            None => {
                permout[nperm] = iaxis as i64;
                permin[iaxis] = nperm as i64;
                nperm += 1;
                Box::new(UnitMap::new(1))
            }
        };
        tmap0 = Some(match tmap0 {
            Some(prev) => Box::new(CmpMap::new(prev, tmap2, Composition::Parallel)?),
            None => tmap2,
        });
    }

    let mut result = None;
    if found && let Some(tmap0) = tmap0 {
        for (iaxis, &pin) in permin.iter().enumerate() {
            if pin < 0 || pin as usize >= wcsaxes || permout[pin as usize] != iaxis as i64 {
                return Err(AstError::new(
                    ErrorCode::Inter,
                    format!(
                        "{METHOD}({CLASS}): Invalid permutation arrays in function \
                         TabMapping (internal AST progranmming error)."
                    ),
                ));
            }
        }

        // Sandwich the look-ups between PermMaps so that the inputs and
        // outputs are in FITS-WCS axis order.
        let inperm: Vec<i64> = permin.iter().map(|p| p + 1).collect();
        let outperm: Vec<i64> = permout.iter().map(|p| p + 1).collect();
        let pm = PermMap::new(&inperm, &outperm, &[])?;
        let mut pm_inv = pm.clone();
        pm_inv.invert();
        let tmap1 = CmpMap::new(Box::new(pm), tmap0, Composition::Series)?;
        let tmap2: DynMapping = Box::new(CmpMap::new(
            Box::new(tmap1),
            Box::new(pm_inv),
            Composition::Series,
        )?);
        result = Some(TabLookup {
            map: tmap2.simplify()?,
            axes: tabaxis,
        });
    }

    // The tables have been used, so are removed from the FitsChan.
    if let Some(tables) = reader.tables.as_mut() {
        for name in &used_tables {
            tables.map_remove(name)?;
        }
    }
    Ok(result)
}

/// An AST__BADTAB error from the read.
fn badtab(text: String) -> AstError {
    AstError::new(ErrorCode::Badtab, format!("{METHOD}({CLASS}): {text}"))
}

/// C's `MakeColumnMap`: a LutMap from the array in row 1 of column `col` of
/// `table`, either an index vector (`isindex`) or a coordinate array, using
/// interpolation `interp` (zero for linear). An unusable column is an error,
/// with C's context message added.
fn make_column_map(
    table: &FitsTable,
    col: &str,
    isindex: bool,
    interp: i64,
) -> AstResult<DynMapping> {
    make_column_map_inner(table, col, isindex, interp).map_err(|e| {
        AstError::new(
            e.code(),
            format!(
                "{e}\n{METHOD}({CLASS}): Cannot read a look-up table for a tabular WCS axis \
                 from column '{col}' of a FITS binary table."
            ),
        )
    })
}

fn make_column_map_inner(
    table: &FitsTable,
    col: &str,
    isindex: bool,
    interp: i64,
) -> AstResult<DynMapping> {
    let t = table.as_table();
    let ndim = t.column_ndim(col)?;
    // FITS-WCS paper III puts the table in row 1, the only row.
    let key = format!("{}(1)", &col[..chr_len(col)]);
    let no_value = || {
        badtab(format!(
            "Row 1 of the binary table contains no value for column '{col}'."
        ))
    };
    let lut_interp = if interp == 0 {
        LutInterp::Linear
    } else {
        LutInterp::Nearest
    };

    let lut = if isindex {
        // FITS-WCS paper II requires an index vector to be one-dimensional.
        if ndim != 1 {
            return Err(badtab(format!(
                "Column '{col}' has {ndim} dimensions but it holds an index vector and \
                 should therefore be 1-dimensional."
            )));
        }
        let nel = t.column_length(col)?;
        let mut lut = t.get_1d(&key)?.ok_or_else(no_value)?;
        lut.truncate(nel);
        lut
    } else {
        // The first dimension of a coordinate array enumerates the
        // coordinates, so has length ndim - 1. C reads it from an array of
        // `ndim` elements, which `astColumnShape` pads with ones.
        let dims = t.column_shape(col)?;
        let first = dims.first().copied().unwrap_or(1);
        if first as i64 != ndim as i64 - 1 {
            return Err(badtab(format!(
                "The first dimension of the coordinate array has length {first} (should be \
                 {} since the array has {ndim} dimensions).",
                ndim as i64 - 1
            )));
        }
        // Only a one-dimensional look-up, held in an array whose first
        // dimension spans a single element, is supported.
        if ndim > 2 {
            return Err(AstError::new(
                ErrorCode::Inter,
                format!(
                    "{METHOD}({CLASS}): AST can currently only handle 1-dimensional \
                     coordinate look-up tables (the supplied table has {} dimensions).",
                    ndim - 1
                ),
            ));
        }
        let mut lut = t.get_1d(&key)?.ok_or_else(no_value)?;
        lut.truncate(dims[1]);
        lut
    };

    // FITS-WCS paper III (sec 6.1.2) puts the first entry at grid
    // coordinate 1.
    let mut map = LutMap::new(&lut, 1.0, 1.0)?;
    map.set_interp(lut_interp);
    Ok(Box::new(map))
}
