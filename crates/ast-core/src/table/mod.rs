//! A Table: a two-dimensional table of values, held as a KeyMap.
//!
//! Ported from `reference/starlink-ast/src/table.c`.
//!
//! Each cell is a KeyMap entry whose key is the column name followed by the
//! one-based row index in parentheses (`"FLUX(3)"`). The column definitions
//! (name, data type, shape and unit of each column) are held in a second
//! KeyMap, and the names of the global parameters in a third. A parameter's
//! value is a KeyMap entry whose key is the parameter name.
//!
//! # How the C class maps onto owned values
//!
//! C's Table is a subclass of KeyMap that overrides every `MapPut0`,
//! `MapPut1`, `MapPutElem`, `MapGet0`, `MapGet1` and `MapGetElem` method,
//! `MapPutU`, `SetKeyCase` and `ClearKeyCase`, and inherits the rest. Here
//! the KeyMap is held by value: the overridden methods are methods of
//! [`Table`] that check the key and then use the KeyMap's own, the
//! inherited methods that only read are reached through
//! [`Table::as_keymap`], and those that change the KeyMap are forwarded.
//!
//! C's `astColumnProps` and `astParameterProps` return a new pointer to the
//! Table's own KeyMaps, so a caller can change the Table's column
//! definitions through it; [`Table::column_props`] and
//! [`Table::parameter_props`] return copies.
//!
//! C's `astGetColumnLength` stores the length it computes in the column's
//! definition, where it is seen by a dump. The column definitions are held
//! in a `RefCell` so that [`Table::column_length`], which only reads the
//! Table, can store it as C does.
//!
//! A column may not hold `AST__POINTERTYPE` values, which have no Rust
//! counterpart.

mod serial;

use crate::error::{AstError, AstResult, ErrorCode};
use crate::keymap::{AstType, KeyMap, SortBy};
use crate::serial::AnyObject;
use std::cell::RefCell;

/// The longest column or parameter name, `AST__MXCOLNAMLEN` (table.h).
pub const MXCOLNAMLEN: usize = 100;

/// The keys of a column definition (table.c).
const NAME: &str = "Name";
const TYPE: &str = "Type";
const SHAPE: &str = "Shape";
const UNIT: &str = "Unit";
const LENGTH: &str = "Length";

/// A two-dimensional table of values.
#[derive(Debug, Clone)]
pub struct Table {
    /// The cells and the parameter values.
    map: KeyMap,
    /// One KeyMap per column, keyed by column name.
    columns: RefCell<KeyMap>,
    /// One entry per parameter, keyed by parameter name.
    parameters: KeyMap,
    /// The number of rows.
    nrow: usize,
    /// The class named in error messages, which C takes from the object
    /// (`astGetClass`): "Table", or the class of a Table embedded in a
    /// subclass.
    class: &'static str,
}

impl Default for Table {
    fn default() -> Self {
        Table::new()
    }
}

/// A KeyMap for column or parameter definitions: keys are not case
/// sensitive, and are sorted oldest first (`astInitTable`).
fn definitions() -> KeyMap {
    let mut km = KeyMap::new();
    km.set_key_case(false)
        .expect("KeyCase can be set on an empty KeyMap");
    km.set_sort_by(SortBy::AgeDown);
    km
}

/// The name C's `TypeString` gives a column data type.
fn type_string(ty: AstType) -> &'static str {
    match ty {
        AstType::Int => "int",
        AstType::Byte => "byte",
        AstType::Double => "double",
        AstType::String => "string",
        AstType::Object => "Object",
        AstType::Float => "float",
        AstType::SInt => "short int",
        AstType::KInt => "int64_t",
        AstType::Undef => "undefined",
        AstType::BadType => "bad type",
    }
}

use crate::keymap::table::chr_len;

/// The key of the cell in row `irow` of column `column` (table.c and
/// fitstable.c `MakeKey`).
pub(crate) fn cell_key(column: &str, irow: usize) -> String {
    format!("{}({irow})", &column[..chr_len(column)])
}

/// The column name and row index of a cell key: a name of at least one
/// character other than `(`, then `(`, an optionally signed integer that
/// may follow white space, `)`, and optional white space, as table.c
/// `ParseKey` reads it with `"%*[^(]%n(%d) %n"`. The name keeps any spaces
/// before the `(`.
fn split_cell_key(key: &str) -> Option<(&str, i64)> {
    let open = key.find('(')?;
    if open == 0 {
        return None;
    }
    let rest = &key[open + 1..];
    let digits = rest.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let sign_len = usize::from(digits.starts_with(['+', '-']));
    let ndigit = digits[sign_len..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    if ndigit == 0 {
        return None;
    }
    let number = &digits[..sign_len + ndigit];
    let after = digits[sign_len + ndigit..].strip_prefix(')')?;
    if !after
        .trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c'])
        .is_empty()
    {
        return None;
    }
    // C's %d overflows silently; a row index that does not fit is treated
    // as invalid.
    let irow = number.parse::<i64>().ok()?;
    Some((&key[..open], irow))
}

impl Table {
    /// An empty Table, with no columns, parameters or rows.
    pub fn new() -> Self {
        let mut map = KeyMap::new();
        map.set_key_case(false)
            .expect("KeyCase can be set on an empty KeyMap");
        Table {
            map,
            columns: RefCell::new(definitions()),
            parameters: definitions(),
            nrow: 0,
            class: "Table",
        }
    }

    /// Name `class` in error messages, for a Table embedded in a subclass.
    pub(crate) fn set_class(&mut self, class: &'static str) {
        self.class = class;
    }

    /// The KeyMap holding the cells and parameter values, for the KeyMap
    /// methods a Table inherits unchanged that only read it.
    pub fn as_keymap(&self) -> &KeyMap {
        &self.map
    }

    // -----------------------------------------------------------------
    // Columns
    // -----------------------------------------------------------------

    /// Add a column. Mirrors `astAddColumn`.
    ///
    /// `dims` gives the shape of the array of values in each cell, and is
    /// empty for a column of scalars. Adding a column that already exists
    /// with the same type, unit and shape does nothing; with a different
    /// one it is an `AST__OLDCOL` error.
    pub fn add_column(
        &mut self,
        name: &str,
        ty: AstType,
        dims: &[usize],
        unit: &str,
    ) -> AstResult<()> {
        let class = self.class;
        let namlen = chr_len(name);
        if namlen == 0 {
            return Err(AstError::new(
                ErrorCode::Badkey,
                format!("astAddColumn({class}): Illegal blank column name supplied."),
            ));
        }
        if namlen > MXCOLNAMLEN {
            return Err(AstError::new(
                ErrorCode::Badkey,
                format!(
                    "astAddColumn({class}): Column name '{name}' is too long (must be \
                     no more than {MXCOLNAMLEN} characters)."
                ),
            ));
        }
        if ty == AstType::BadType {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!("astAddColumn({class}): Bad data type supplied for new column {name}."),
            ));
        }
        if let Some(idim) = dims.iter().position(|&d| d < 1) {
            return Err(AstError::new(
                ErrorCode::Dimin,
                format!(
                    "astAddColumn({class}): Length of axis {} ({}) for new column {name} \
                     is invalid.",
                    idim + 1,
                    dims[idim]
                ),
            ));
        }

        let mut cols = self.columns.borrow_mut();
        if let Some(AnyObject::KeyMap(col)) = cols.object_mut(name) {
            let oldtype = col.get_0i(TYPE)?.unwrap_or(0);
            if i64::from(oldtype) != ty.code() {
                return Err(AstError::new(
                    ErrorCode::Oldcol,
                    format!(
                        "astAddColumn({class}): A column called {name} already exists in \
                         the table with a different data type ({}).",
                        AstType::from_code(i64::from(oldtype)).map_or("bad type", type_string)
                    ),
                ));
            }
            let oldunit = col.get_0c(UNIT)?.unwrap_or_default();
            if oldunit != unit {
                return Err(AstError::new(
                    ErrorCode::Oldcol,
                    format!(
                        "astAddColumn({class}): A column called {name} already exists in \
                         the table with a different unit string ('{oldunit}')."
                    ),
                ));
            }
            let oldndim = col.map_length(SHAPE);
            if oldndim != dims.len() {
                return Err(AstError::new(
                    ErrorCode::Oldcol,
                    format!(
                        "astAddColumn({class}): A column called {name} already exists in \
                         the table with a different number of axes ({oldndim})."
                    ),
                ));
            }
            if !dims.is_empty() {
                let olddims = col.get_1i(SHAPE)?.unwrap_or_default();
                if olddims.iter().zip(dims).any(|(&o, &d)| o as usize != d) {
                    return Err(AstError::new(
                        ErrorCode::Oldcol,
                        format!(
                            "astAddColumn({class}): A column called {name} already exists \
                             in the table with a different shape."
                        ),
                    ));
                }
            }
            return Ok(());
        }

        let mut col = KeyMap::new();
        col.put_0c(NAME, name, None)?;
        col.put_0i(TYPE, ty.code() as i32, None)?;
        if !dims.is_empty() {
            let shape: Vec<i32> = dims.iter().map(|&d| d as i32).collect();
            col.put_1i(SHAPE, &shape, None)?;
        }
        col.put_0c(UNIT, unit, None)?;
        cols.put_0a(name, AnyObject::KeyMap(Box::new(col)), None)
    }

    /// Remove a column and every value in it. Removing a column that does
    /// not exist does nothing. Mirrors `astRemoveColumn`.
    pub fn remove_column(&mut self, name: &str) -> AstResult<()> {
        let class = self.class;
        if chr_len(name) == 0 {
            return Err(AstError::new(
                ErrorCode::Badkey,
                format!("astRemoveColumn({class}): Illegal blank column name supplied."),
            ));
        }
        let mut cols = self.columns.borrow_mut();
        if cols.map_has_key(name)? {
            cols.map_remove(name)?;
            for irow in 1..=self.nrow {
                self.map.map_remove(&cell_key(name, irow))?;
            }
        }
        Ok(())
    }

    /// The number of columns. Mirrors the `Ncolumn` attribute.
    pub fn ncolumn(&self) -> usize {
        self.columns.borrow().map_size()
    }

    /// The name of column `index` (one-based, in the order the columns were
    /// added), in upper case. Mirrors `astColumnName`.
    pub fn column_name(&self, index: usize) -> AstResult<String> {
        let class = self.class;
        let cols = self.columns.borrow();
        if index < 1 || index > cols.map_size() {
            return Err(AstError::new(
                ErrorCode::Mpind,
                format!(
                    "astColumnName({class}): Cannot find column {index} (zero-based) of \
                     the Table - invalid index."
                ),
            ));
        }
        cols.map_key(index - 1)
    }

    /// Whether the Table has a column called `column`. Mirrors
    /// `astHasColumn`.
    pub fn has_column(&self, column: &str) -> AstResult<bool> {
        self.columns.borrow().map_has_key(column)
    }

    /// A copy of the column definitions: one KeyMap per column, keyed by
    /// column name, holding the keys `Name`, `Type`, `Unit` and, for a
    /// column of arrays, `Shape`. Mirrors `astColumnProps`, which returns
    /// the Table's own KeyMap rather than a copy.
    pub fn column_props(&self) -> KeyMap {
        self.columns.borrow().clone()
    }

    /// Apply `f` to the definition of `column`, or fail with `AST__BADCOL`
    /// naming `method` when there is no such column.
    fn with_column<T>(
        &self,
        column: &str,
        method: &str,
        f: impl FnOnce(&mut KeyMap) -> AstResult<T>,
    ) -> AstResult<T> {
        let class = self.class;
        let mut cols = self.columns.borrow_mut();
        match cols.object_mut(column) {
            Some(AnyObject::KeyMap(col)) => f(col),
            _ => Err(AstError::new(
                ErrorCode::Badcol,
                format!("{method}({class}): No column named '{column}' exists in the table."),
            )),
        }
    }

    /// Apply `f` to the definition of `column`, the Table's own KeyMap as
    /// C's `astColumnProps` gives it, or return `None` when there is no
    /// such column.
    pub(crate) fn with_column_def<T>(
        &self,
        column: &str,
        f: impl FnOnce(&mut KeyMap) -> AstResult<T>,
    ) -> AstResult<Option<T>> {
        let mut cols = self.columns.borrow_mut();
        match cols.object_mut(column) {
            Some(AnyObject::KeyMap(col)) => f(col).map(Some),
            _ => Ok(None),
        }
    }

    /// The data type of the values in `column`. Mirrors the `ColumnType`
    /// attribute.
    pub fn column_type(&self, column: &str) -> AstResult<AstType> {
        self.with_column(column, "astGetColumnType", |col| {
            let code = col.get_0i(TYPE)?.unwrap_or(0);
            Ok(AstType::from_code(i64::from(code)).unwrap_or(AstType::BadType))
        })
    }

    /// The unit string of `column`. Mirrors the `ColumnUnit` attribute.
    pub fn column_unit(&self, column: &str) -> AstResult<String> {
        self.with_column(column, "astGetColumnUnit", |col| {
            Ok(col.get_0c(UNIT)?.unwrap_or_default())
        })
    }

    /// The number of axes of the array in each cell of `column`, zero for
    /// a column of scalars. Mirrors the `ColumnNdim` attribute.
    pub fn column_ndim(&self, column: &str) -> AstResult<usize> {
        self.with_column(column, "astGetColumnNdim", |col| Ok(col.map_length(SHAPE)))
    }

    /// The number of values in each cell of `column`, the product of its
    /// shape. Mirrors the `ColumnLength` attribute, and like C stores the
    /// value in the column's definition the first time it is asked for.
    pub fn column_length(&self, column: &str) -> AstResult<usize> {
        self.with_column(column, "astGetColumnLength", |col| {
            if let Some(len) = col.get_0i(LENGTH)? {
                return Ok(len as usize);
            }
            let dims = col.get_1i(SHAPE)?.unwrap_or_default();
            let len: i32 = dims.iter().product();
            col.put_0i(LENGTH, len, None)?;
            Ok(len as usize)
        })
    }

    /// The shape of the array in each cell of `column`, empty for a column
    /// of scalars or a column that does not exist. Mirrors
    /// `astColumnShape`, which also pads the shape with ones to the length
    /// of the caller's array.
    pub fn column_shape(&self, column: &str) -> AstResult<Vec<usize>> {
        let mut cols = self.columns.borrow_mut();
        match cols.object_mut(column) {
            Some(AnyObject::KeyMap(col)) => Ok(col
                .get_1i(SHAPE)?
                .unwrap_or_default()
                .into_iter()
                .map(|d| d as usize)
                .collect()),
            _ => Ok(Vec::new()),
        }
    }

    /// The length of the longest string a value in `column` is formatted
    /// as. Mirrors the `ColumnLenC` attribute.
    pub fn column_len_c(&self, column: &str) -> AstResult<usize> {
        let class = self.class;
        if !self.columns.borrow().map_has_key(column)? {
            return Err(AstError::new(
                ErrorCode::Badcol,
                format!(
                    "astGetColumnLenC({class}): No column named '{column}' exists in the table."
                ),
            ));
        }
        let mut result = 0;
        for irow in 1..=self.nrow {
            result = result.max(self.map.map_len_c(&cell_key(column, irow))?);
        }
        Ok(result)
    }

    // -----------------------------------------------------------------
    // Parameters
    // -----------------------------------------------------------------

    /// Add a global parameter. Adding one that exists does nothing.
    /// Mirrors `astAddParameter`.
    pub fn add_parameter(&mut self, name: &str) -> AstResult<()> {
        let class = self.class;
        let namlen = chr_len(name);
        if namlen == 0 {
            return Err(AstError::new(
                ErrorCode::Badkey,
                format!("astAddParameter({class}): Illegal blank parameter name supplied."),
            ));
        }
        if namlen > MXCOLNAMLEN {
            return Err(AstError::new(
                ErrorCode::Badkey,
                format!(
                    "astAddParameter({class}): Parameter name '{name}' is too long (must \
                     be no more than {MXCOLNAMLEN} characters)."
                ),
            ));
        }
        if !self.parameters.map_has_key(name)? {
            // The value is arbitrary and unused.
            self.parameters.put_0i(name, 1, None)?;
        }
        Ok(())
    }

    /// Remove a global parameter and its value. Removing a parameter that
    /// does not exist does nothing. Mirrors `astRemoveParameter`.
    pub fn remove_parameter(&mut self, name: &str) -> AstResult<()> {
        let class = self.class;
        if chr_len(name) == 0 {
            return Err(AstError::new(
                ErrorCode::Badkey,
                format!("astRemoveParameter({class}): Illegal blank parameter name supplied."),
            ));
        }
        if self.parameters.map_has_key(name)? {
            self.parameters.map_remove(name)?;
            self.map.map_remove(name)?;
        }
        Ok(())
    }

    /// The number of global parameters. Mirrors the `Nparameter`
    /// attribute.
    pub fn nparameter(&self) -> usize {
        self.parameters.map_size()
    }

    /// The name of parameter `index` (one-based, in the order the
    /// parameters were added), in upper case. Mirrors `astParameterName`.
    pub fn parameter_name(&self, index: usize) -> AstResult<String> {
        let class = self.class;
        if index < 1 || index > self.parameters.map_size() {
            return Err(AstError::new(
                ErrorCode::Mpind,
                format!(
                    "astParameterName({class}): Cannot find parameter {index} (zero-based) \
                     of the Table - invalid index."
                ),
            ));
        }
        self.parameters.map_key(index - 1)
    }

    /// Whether the Table has a global parameter called `parameter`.
    /// Mirrors `astHasParameter`.
    pub fn has_parameter(&self, parameter: &str) -> AstResult<bool> {
        self.parameters.map_has_key(parameter)
    }

    /// A copy of the parameter definitions, one entry per parameter keyed
    /// by its name. Mirrors `astParameterProps`, which returns the Table's
    /// own KeyMap rather than a copy.
    pub fn parameter_props(&self) -> KeyMap {
        self.parameters.clone()
    }

    // -----------------------------------------------------------------
    // Rows
    // -----------------------------------------------------------------

    /// The number of rows: the highest row index any value has been
    /// stored in, less the rows removed from the end. Mirrors the `Nrow`
    /// attribute.
    pub fn nrow(&self) -> usize {
        self.nrow
    }

    /// Remove every value in row `index` (one-based). The row is left
    /// empty, and the number of rows changes only when it is the last.
    /// An index outside the Table does nothing. Mirrors `astRemoveRow`.
    pub fn remove_row(&mut self, index: usize) -> AstResult<()> {
        if index > 0 && index <= self.nrow {
            let names: Vec<String> = {
                let cols = self.columns.borrow();
                (0..cols.map_size())
                    .map(|icol| cols.map_key(icol))
                    .collect::<AstResult<_>>()?
            };
            for col in names {
                self.map.map_remove(&format!("{col}({index})"))?;
            }
            if index == self.nrow {
                self.nrow = index - 1;
            }
        }
        Ok(())
    }

    /// Remove every empty row, moving the rows below up. Mirrors
    /// `astPurgeRows`.
    pub fn purge_rows(&mut self) -> AstResult<()> {
        let class = self.class;
        let nrow = self.nrow;

        // The number of values stored in each row.
        let mut nval = vec![0usize; nrow];
        let mut reset = true;
        while let Some(key) = self.map.map_iterate(reset) {
            reset = false;
            // The value of a global parameter is not in any row. C reported
            // it as an internal error until fixed upstream (f32573b1).
            if self.has_parameter(&key)? {
                continue;
            }
            let iold = key
                .find('(')
                .and_then(|op| leading_int(&key[op + 1..]))
                .filter(|&i| i <= nrow as i64);
            match iold {
                Some(i) => nval[(i - 1) as usize] += 1,
                None => {
                    return Err(AstError::new(
                        ErrorCode::Inter,
                        format!(
                            "astPurgeRows({class}): Illegal key '{key}' found in a Table \
                             (internal programming error)."
                        ),
                    ));
                }
            }
        }

        let ncol = self.ncolumn();
        let mut inew = nrow;
        for icol in 1..=ncol {
            let col = self.column_name(icol)?;
            inew = 0;
            let mut iold = 0;
            while iold < nrow {
                if nval[iold] > 0 {
                    let old = iold;
                    inew += 1;
                    if old + 1 != inew {
                        self.map.map_rename(
                            &format!("{col}({})", old + 1),
                            &format!("{col}({inew})"),
                        )?;
                    }
                }
                iold += 1;
            }
            if iold == inew {
                break;
            }
        }
        self.nrow = inew;
        Ok(())
    }

    // -----------------------------------------------------------------
    // Cell and parameter values
    // -----------------------------------------------------------------

    /// The column name and row index of `key`, a cell of a column of the
    /// Table, or `None`. With `report`, a key that is not such a cell is
    /// an `AST__BADKEY` error. Mirrors table.c `ParseKey`.
    fn parse_key(
        &self,
        key: &str,
        report: bool,
        method: &str,
    ) -> AstResult<Option<(String, usize)>> {
        let class = self.class;
        let bad = |what: String| -> AstResult<Option<(String, usize)>> {
            if report {
                Err(AstError::new(
                    ErrorCode::Badkey,
                    format!(
                        "{method}({class}): Failed to store a value for cell \"{key}\": {what}"
                    ),
                ))
            } else {
                Ok(None)
            }
        };
        let Some((name, irow)) = split_cell_key(key) else {
            return bad("the cell name is invalid.".to_string());
        };
        if name.len() > MXCOLNAMLEN {
            return bad("column name is too long.".to_string());
        }
        if irow < 1 {
            return bad(format!("row index {irow} is invalid."));
        }
        let colname = name.to_ascii_uppercase();
        if !self.columns.borrow().map_has_key(&colname)? {
            return bad(format!(
                "the table does not contain a column called '{colname}'."
            ));
        }
        Ok(Some((colname, irow as usize)))
    }

    /// Check a put of one value of type `ty` to `key`, and return the row
    /// it is in, or `None` for a parameter. Mirrors the checks of table.c
    /// `MapPut0<X>`.
    fn check_put0(&self, key: &str, ty: AstType, method: &str) -> AstResult<Option<usize>> {
        let class = self.class;
        if self.has_parameter(key)? {
            return Ok(None);
        }
        let (col, irow) = self
            .parse_key(key, true, method)?
            .expect("a reported parse gives a cell");
        let coltype = self.column_type(&col)?;
        if coltype != ty {
            return Err(AstError::new(
                ErrorCode::Badtyp,
                format!(
                    "{method}({class}): Failed to store a {} value for cell \"{key}\": \
                     column {col} holds {} values.",
                    type_string(ty),
                    type_string(coltype)
                ),
            ));
        }
        if self.column_ndim(&col)? > 0 {
            return Err(AstError::new(
                ErrorCode::Badtyp,
                format!(
                    "{method}({class}): Failed to store a scalar value for cell \"{key}\": \
                     column {col} holds vector  values."
                ),
            ));
        }
        Ok(Some(irow))
    }

    /// Check a put of `size` values of type `ty` to `key`, and return the
    /// row it is in, or `None` for a parameter. Mirrors the checks of
    /// table.c `MapPut1<X>`.
    fn check_put1(
        &self,
        key: &str,
        ty: AstType,
        size: usize,
        method: &str,
    ) -> AstResult<Option<usize>> {
        let class = self.class;
        if self.has_parameter(key)? {
            return Ok(None);
        }
        let (col, irow) = self
            .parse_key(key, true, method)?
            .expect("a reported parse gives a cell");
        let coltype = self.column_type(&col)?;
        if coltype != ty {
            return Err(AstError::new(
                ErrorCode::Badtyp,
                format!(
                    "{method}({class}): Failed to store {} values for cell \"{key}\": \
                     column {col} holds {} values.",
                    type_string(ty),
                    type_string(coltype)
                ),
            ));
        }
        let len = self.column_length(&col)?;
        if len != size {
            return Err(AstError::new(
                ErrorCode::Badtyp,
                format!(
                    "{method}({class}): Failed to store a vector value for cell \"{key}\": \
                     column {col} needs {len} values per cell but {size} were supplied."
                ),
            ));
        }
        Ok(Some(irow))
    }

    /// Check a put of element `elem` of type `ty` to `key`, and return the
    /// row it is in, or `None` for a parameter. Mirrors the checks of
    /// table.c `MapPutElem<X>`.
    fn check_put_elem(
        &self,
        key: &str,
        ty: AstType,
        elem: usize,
        method: &str,
    ) -> AstResult<Option<usize>> {
        let class = self.class;
        if self.has_parameter(key)? {
            return Ok(None);
        }
        let (col, irow) = self
            .parse_key(key, true, method)?
            .expect("a reported parse gives a cell");
        let coltype = self.column_type(&col)?;
        if coltype != ty {
            return Err(AstError::new(
                ErrorCode::Badtyp,
                format!(
                    "{method}({class}): Failed to store a {} value in cell \"{key}\": \
                     column {col} holds {} values.",
                    type_string(ty),
                    type_string(coltype)
                ),
            ));
        }
        let len = self.column_length(&col)?;
        if len <= elem {
            return Err(AstError::new(
                ErrorCode::Badtyp,
                format!(
                    "{method}({class}): Failed to store a value for element {elem} \
                     (zero-based) of cell \"{key}\": column {col} has only {len} values \
                     per cell."
                ),
            ));
        }
        Ok(Some(irow))
    }

    /// Whether the cell `key` holds no value: absent, or stored with
    /// [`Table::put_u`]. KeyMap's `MapPutElem` stores an element of such
    /// a key as a new one-element vector through `astMapPut1`, which for a
    /// Table is the Table's own and so checks the column length (keymap.c
    /// `MapPutElem<X>`).
    fn cell_is_empty(&self, key: &str) -> AstResult<bool> {
        Ok(matches!(
            self.map.map_type(key)?,
            AstType::BadType | AstType::Undef
        ))
    }

    /// Note that a value has been stored in row `irow`.
    fn extend_rows(&mut self, irow: Option<usize>) {
        if let Some(irow) = irow {
            self.nrow = self.nrow.max(irow);
        }
    }

    /// Whether a get from `key` should go to the KeyMap: a parameter, or a
    /// cell within the Table. Mirrors table.c `MapGet0<X>`, which reports a
    /// key that is neither only when `KeyError` is set.
    fn gettable(&self, key: &str, method: &str) -> AstResult<bool> {
        if self.has_parameter(key)? {
            return Ok(true);
        }
        Ok(match self.parse_key(key, self.map.key_error(), method)? {
            Some((_, irow)) => irow <= self.nrow,
            None => false,
        })
    }

    /// Store an undefined value. Mirrors `astMapPutU`.
    pub fn put_u(&mut self, key: &str, comment: Option<&str>) -> AstResult<()> {
        let irow = if self.has_parameter(key)? {
            None
        } else {
            let (_, irow) = self
                .parse_key(key, true, "astMapPutU")?
                .expect("a reported parse gives a cell");
            Some(irow)
        };
        self.extend_rows(irow);
        self.map.put_u(key, comment)
    }
}

/// The integer at the start of `s`, after any white space, as `sscanf`'s
/// `%d` reads it.
fn leading_int(s: &str) -> Option<i64> {
    let s = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let sign_len = usize::from(s.starts_with(['+', '-']));
    let ndigit = s[sign_len..].bytes().take_while(u8::is_ascii_digit).count();
    if ndigit == 0 {
        return None;
    }
    s[..sign_len + ndigit].parse().ok()
}

/// The typed puts and gets of one data type, each checking the key as
/// table.c does before using the KeyMap's own.
macro_rules! typed_access {
    ($ty:expr, $tname:literal,
     $put0:ident, $get0:ident, $put1:ident, $get1:ident, $put_elem:ident, $get_elem:ident,
     $val:ty, $vals:ty, $out0:ty, $out1:ty, $elem_val:ty, $out_elem:ty, |$v:ident| $one:expr) => {
        impl Table {
            #[doc = concat!("Store a ", $tname, " value in a cell or parameter. Mirrors `astMapPut0`.")]
            pub fn $put0(&mut self, key: &str, value: $val, comment: Option<&str>) -> AstResult<()> {
                let irow = self.check_put0(key, $ty, "astMapPut0")?;
                self.extend_rows(irow);
                self.map.$put0(key, value, comment)
            }

            #[doc = concat!("The value of a cell or parameter as ", $tname, ", or `None` if it has none. Mirrors `astMapGet0`.")]
            pub fn $get0(&self, key: &str) -> AstResult<Option<$out0>> {
                if self.gettable(key, "astMapGet0")? {
                    self.map.$get0(key)
                } else {
                    Ok(None)
                }
            }

            #[doc = concat!("Store an array of ", $tname, " values in a cell or parameter. Mirrors `astMapPut1`.")]
            pub fn $put1(&mut self, key: &str, values: $vals, comment: Option<&str>) -> AstResult<()> {
                let irow = self.check_put1(key, $ty, values.len(), "astMapPut1")?;
                self.extend_rows(irow);
                self.map.$put1(key, values, comment)
            }

            #[doc = concat!("The values of a cell or parameter as ", $tname, ", or `None` if it has none. Mirrors `astMapGet1`.")]
            pub fn $get1(&self, key: &str) -> AstResult<Option<$out1>> {
                if self.gettable(key, "astMapGet1")? {
                    self.map.$get1(key)
                } else {
                    Ok(None)
                }
            }

            #[doc = concat!("Store one ", $tname, " element (zero-based) of a cell or parameter. Mirrors `astMapPutElem`.")]
            pub fn $put_elem(&mut self, key: &str, elem: usize, value: $elem_val) -> AstResult<()> {
                let irow = self.check_put_elem(key, $ty, elem, "astMapPutElem")?;
                if irow.is_some() && self.cell_is_empty(key)? {
                    let $v = value;
                    return self.$put1(key, $one, None);
                }
                self.extend_rows(irow);
                self.map.$put_elem(key, elem, value)
            }

            #[doc = concat!("One element (zero-based) of a cell or parameter as ", $tname, ", or `None` if it has none. Mirrors `astMapGetElem`.")]
            pub fn $get_elem(&self, key: &str, elem: usize) -> AstResult<Option<$out_elem>> {
                if self.gettable(key, "astMapGetElem")? {
                    self.map.$get_elem(key, elem)
                } else {
                    Ok(None)
                }
            }
        }
    };
}

typed_access!(
    AstType::Int,
    "int",
    put_0i,
    get_0i,
    put_1i,
    get_1i,
    put_elem_i,
    get_elem_i,
    i32,
    &[i32],
    i32,
    Vec<i32>,
    i32,
    i32,
    |v| &[v]
);
typed_access!(
    AstType::SInt,
    "short int",
    put_0s,
    get_0s,
    put_1s,
    get_1s,
    put_elem_s,
    get_elem_s,
    i16,
    &[i16],
    i16,
    Vec<i16>,
    i16,
    i16,
    |v| &[v]
);
typed_access!(
    AstType::KInt,
    "64-bit int",
    put_0k,
    get_0k,
    put_1k,
    get_1k,
    put_elem_k,
    get_elem_k,
    i64,
    &[i64],
    i64,
    Vec<i64>,
    i64,
    i64,
    |v| &[v]
);
typed_access!(
    AstType::Byte,
    "byte",
    put_0b,
    get_0b,
    put_1b,
    get_1b,
    put_elem_b,
    get_elem_b,
    u8,
    &[u8],
    u8,
    Vec<u8>,
    u8,
    u8,
    |v| &[v]
);
typed_access!(
    AstType::Float,
    "float",
    put_0f,
    get_0f,
    put_1f,
    get_1f,
    put_elem_f,
    get_elem_f,
    f32,
    &[f32],
    f32,
    Vec<f32>,
    f32,
    f32,
    |v| &[v]
);
typed_access!(
    AstType::Double,
    "double",
    put_0d,
    get_0d,
    put_1d,
    get_1d,
    put_elem_d,
    get_elem_d,
    f64,
    &[f64],
    f64,
    Vec<f64>,
    f64,
    f64,
    |v| &[v]
);
typed_access!(
    AstType::String,
    "string",
    put_0c,
    get_0c,
    put_1c,
    get_1c,
    put_elem_c,
    get_elem_c,
    &str,
    &[&str],
    String,
    Vec<String>,
    &str,
    String,
    |v| &[v]
);
typed_access!(
    AstType::Object,
    "Object",
    put_0a,
    get_0a,
    put_1a,
    get_1a,
    put_elem_a,
    get_elem_a,
    AnyObject,
    Vec<Option<AnyObject>>,
    Option<AnyObject>,
    Vec<Option<AnyObject>>,
    Option<AnyObject>,
    Option<AnyObject>,
    |v| vec![v]
);

/// The KeyMap methods a Table inherits unchanged that change the KeyMap.
impl Table {
    /// Remove an entry. The number of rows does not change. Mirrors
    /// `astMapRemove`, which a Table inherits.
    pub fn map_remove(&mut self, key: &str) -> AstResult<()> {
        self.map.map_remove(key)
    }

    /// Rename an entry. Mirrors `astMapRename`, which a Table inherits and
    /// which does not check that the new key names a cell.
    pub fn map_rename(&mut self, old_key: &str, new_key: &str) -> AstResult<()> {
        self.map.map_rename(old_key, new_key)
    }

    /// The next key in read-out order. Mirrors `astMapIterate`.
    pub fn map_iterate(&mut self, reset: bool) -> Option<String> {
        self.map.map_iterate(reset)
    }

    /// Set KeyError. Mirrors the KeyMap attribute.
    pub fn set_key_error(&mut self, key_error: bool) {
        self.map.set_key_error(key_error);
    }

    /// Clear KeyError. Mirrors the KeyMap attribute.
    pub fn clear_key_error(&mut self) {
        self.map.clear_key_error();
    }

    /// Set MapLocked. Mirrors the KeyMap attribute.
    pub fn set_map_locked(&mut self, map_locked: bool) {
        self.map.set_map_locked(map_locked);
    }

    /// Clear MapLocked. Mirrors the KeyMap attribute.
    pub fn clear_map_locked(&mut self) {
        self.map.clear_map_locked();
    }

    /// Set SortBy. Mirrors the KeyMap attribute.
    pub fn set_sort_by(&mut self, sort_by: SortBy) {
        self.map.set_sort_by(sort_by);
    }

    /// Clear SortBy. Mirrors the KeyMap attribute.
    pub fn clear_sort_by(&mut self) {
        self.map.clear_sort_by();
    }

    /// Set SizeGuess. Mirrors the KeyMap attribute.
    pub fn set_size_guess(&mut self, sizeguess: usize) -> AstResult<()> {
        self.map.set_size_guess(sizeguess)
    }

    /// Clear SizeGuess. Mirrors the KeyMap attribute.
    pub fn clear_size_guess(&mut self) -> AstResult<()> {
        self.map.clear_size_guess()
    }

    /// Does nothing: a Table's keys are never case sensitive. Mirrors
    /// table.c `SetKeyCase`.
    pub fn set_key_case(&mut self, _key_case: bool) -> AstResult<()> {
        Ok(())
    }

    /// Does nothing: a Table's keys are never case sensitive. Mirrors
    /// table.c `ClearKeyCase`.
    pub fn clear_key_case(&mut self) -> AstResult<()> {
        Ok(())
    }

    /// Set the Object attribute Ident.
    pub fn set_ident(&mut self, ident: &str) {
        self.map.set_ident(ident);
    }

    /// Clear the Object attribute Ident.
    pub fn clear_ident(&mut self) {
        self.map.clear_ident();
    }

    /// Set the Object attribute ID.
    pub fn set_id(&mut self, id: &str) {
        self.map.set_id(id);
    }

    /// Clear the Object attribute ID.
    pub fn clear_id(&mut self) {
        self.map.clear_id();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_keys_parse_as_c_scans_them() {
        assert_eq!(split_cell_key("Fred(2)"), Some(("Fred", 2)));
        assert_eq!(split_cell_key("Fred (2)"), Some(("Fred ", 2)));
        assert_eq!(split_cell_key("Fred( 3)"), Some(("Fred", 3)));
        assert_eq!(split_cell_key("Fred(3) "), Some(("Fred", 3)));
        assert_eq!(split_cell_key("Fred(-1)"), Some(("Fred", -1)));
        assert_eq!(split_cell_key("Fred( 3 )"), None);
        assert_eq!(split_cell_key("Fred(2x)"), None);
        assert_eq!(split_cell_key("Fred()"), None);
        assert_eq!(split_cell_key("(2)"), None);
        assert_eq!(split_cell_key("Fred(2)(3)"), None);
        assert_eq!(split_cell_key("Fred"), None);
    }
}
