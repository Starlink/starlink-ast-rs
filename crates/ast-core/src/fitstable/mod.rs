//! FitsTables: FITS binary tables, held as a Table of column data and a
//! FitsChan of header cards.
//!
//! See [`FitsTable`] for what a FitsTable holds and how its header is kept.

// Ported from reference/starlink-ast/src/fitstable.c.
//
// How the C class maps onto owned values:
//
// C's FitsTable is a subclass of Table that overrides AddColumn, to refuse
// columns a FITS binary table cannot hold, and adds methods that read and
// write the header and whole columns. Here the Table is held by value. The
// Table methods that only read are reached through FitsTable::as_table;
// those that change the Table are methods of FitsTable, so that a column
// cannot be added without the FitsTable's check.
//
// C's astGetTableHeader first rewrites the cards that describe the columns
// in the FitsTable's own header, where a dump sees them, so
// FitsTable::get_table_header takes &mut self. Creating a FitsTable from a
// header reads the caller's FitsChan in place, moving its current card, so
// FitsTable::from_header takes &mut FitsChan.
//
// C passes column data as untyped memory. Here it is a ColumnData holding a
// vector of the column's own data type, and sizes in bytes are those of the
// C types.

mod serial;

use crate::error::{AstError, AstResult, ErrorCode};
use crate::fitschan::FitsChan;
use crate::keymap::table::chr_len;
use crate::keymap::{AstType, KeyMap, SortBy};
use crate::serial::AnyObject;
use crate::table::{Table, cell_key};

/// The KeyMap key holding a column's null value in its definition
/// (fitstable.c `NULLKEY`).
const NULLKEY: &str = "Null";

/// The keywords whose values are set by the FITS standard or by the
/// contents of the table (fitstable.c `PurgeHeader`).
const FIXED: [&str; 14] = [
    "XTENSION", "BITPIX", "NAXIS", "NAXIS1", "NAXIS2", "PCOUNT", "GCOUNT", "TFIELDS", "TFORM%d",
    "TTYPE%d", "TNULL%d", "THEAP", "TDIM%d", "TUNIT%d",
];

/// The values of a whole column of a [`FitsTable`], row by row.
///
/// The values of the first row come first, and the elements of the array
/// in each cell are in Fortran order, the first axis varying fastest.
/// Each variant holds the values of a column of one data type.
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnData {
    /// The values of an [`AstType::Byte`] column.
    Byte(Vec<u8>),
    /// The values of an [`AstType::SInt`] column.
    SInt(Vec<i16>),
    /// The values of an [`AstType::Int`] column.
    Int(Vec<i32>),
    /// The values of an [`AstType::KInt`] column.
    KInt(Vec<i64>),
    /// The values of an [`AstType::Float`] column.
    Float(Vec<f32>),
    /// The values of an [`AstType::Double`] column.
    Double(Vec<f64>),
    /// The values of an [`AstType::String`] column, as fixed-length strings
    /// of `len` bytes with no terminating nulls.
    ///
    /// A string shorter than `len` is padded with nulls.
    String {
        /// The length of each string.
        len: usize,
        /// The bytes of each string. When storing, only the first `len`
        /// bytes of each are used, and a shorter one is padded with nulls.
        values: Vec<Vec<u8>>,
    },
}

impl ColumnData {
    /// The data type of the column the values belong to.
    pub fn data_type(&self) -> AstType {
        match self {
            ColumnData::Byte(_) => AstType::Byte,
            ColumnData::SInt(_) => AstType::SInt,
            ColumnData::Int(_) => AstType::Int,
            ColumnData::KInt(_) => AstType::KInt,
            ColumnData::Float(_) => AstType::Float,
            ColumnData::Double(_) => AstType::Double,
            ColumnData::String { .. } => AstType::String,
        }
    }

    /// The number of elements, counting each string as one.
    pub fn nelem(&self) -> usize {
        match self {
            ColumnData::Byte(v) => v.len(),
            ColumnData::SInt(v) => v.len(),
            ColumnData::Int(v) => v.len(),
            ColumnData::KInt(v) => v.len(),
            ColumnData::Float(v) => v.len(),
            ColumnData::Double(v) => v.len(),
            ColumnData::String { values, .. } => values.len(),
        }
    }

    /// The size in bytes of the C array holding the values.
    fn size(&self) -> usize {
        match self {
            ColumnData::String { len, values } => values.len() * len,
            _ => self.nelem() * elem_size(self.data_type(), 0),
        }
    }
}

/// The null value of an integer column, from [`FitsTable::column_null`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnNull {
    /// The null value set before the call, or else `set`, or else a default
    /// not in the column; see [`FitsTable::column_null`].
    pub value: i64,
    /// Whether `value` was set by an earlier call rather than being a
    /// default.
    pub was_set: bool,
    /// Whether the column holds a null value or an empty cell, when asked
    /// for.
    pub has_null: Option<bool>,
}

/// The size in bytes of one element of a column of type `ty`, as the C
/// type that holds it, with `clen` the length of a string.
fn elem_size(ty: AstType, clen: usize) -> usize {
    match ty {
        AstType::Int | AstType::Float => 4,
        AstType::KInt | AstType::Double => 8,
        AstType::SInt => 2,
        AstType::Byte => 1,
        AstType::String => clen,
        _ => 0,
    }
}

/// The integer at the start of `s` and the number of bytes it occupies,
/// as `sscanf( s, "%d%n", ... )` reads them, or `None` when `s` does not
/// start with an integer.
fn scan_int(s: &str) -> Option<(i64, usize)> {
    let bytes = s.as_bytes();
    let mut i = bytes
        .iter()
        .take_while(|b| b.is_ascii_whitespace() || **b == b'\x0b')
        .count();
    let start = i;
    if matches!(bytes.get(i), Some(b'+' | b'-')) {
        i += 1;
    }
    let ndigit = bytes[i..].iter().take_while(|b| b.is_ascii_digit()).count();
    if ndigit == 0 {
        return None;
    }
    i += ndigit;
    s[start..i].parse().ok().map(|v| (v, i))
}

/// A FITS binary table.
///
/// A FitsTable holds the data of the table in a [`Table`], one cell per
/// row of each column, and the FITS header of the table in a
/// [`FitsChan`].
/// Columns, cells, cell keys and parameters are as for a Table, and the
/// Table methods that change the table are methods of the FitsTable too;
/// [`as_table`](Self::as_table) gives the Table for those that only read
/// it, such as [`Table::nrow`] and [`Table::column_type`].
/// Like a Table, a FitsTable is not suited to very large tables.
///
/// # Columns
///
/// A column holds one of these data types, written in the header as the
/// data type code of its `TFORMn` keyword:
///
/// | [`AstType`] | `TFORMn` code | [`ColumnData`] |
/// |---|---|---|
/// | `Byte` | `B` | `Byte(Vec<u8>)` |
/// | `SInt` | `I` | `SInt(Vec<i16>)` |
/// | `Int` | `J` | `Int(Vec<i32>)` |
/// | `KInt` | `K` | `KInt(Vec<i64>)` |
/// | `Float` | `E` | `Float(Vec<f32>)` |
/// | `Double` | `D` | `Double(Vec<f64>)` |
/// | `String` | `A` | `String { len, values }` |
///
/// A FitsTable is made for the `-TAB` algorithm of FITS-WCS Paper III, and does not
/// cover every feature of a FITS binary table: it has no heap
/// data following the main table, and no columns of complex, logical or
/// bit values, of variable-length arrays, or with scaling or a display
/// format.
///
/// [`get_column_data`](Self::get_column_data) and
/// [`put_column_data`](Self::put_column_data) read and store a whole
/// column at once.
/// An integer column may have a null value, which marks an element as
/// null; see [`column_null`](Self::column_null).
///
/// # The header
///
/// The header holds the cards that describe the table.
/// The cards whose values are set by the FITS standard or by the columns
/// and rows of the table are `XTENSION`, `BITPIX`, `NAXIS`, `NAXIS1`,
/// `NAXIS2`, `PCOUNT`, `GCOUNT`, `TFIELDS` and `THEAP`, and the `TFORMn`,
/// `TTYPEn`, `TUNITn`, `TNULLn` and `TDIMn` of each column.
/// They are removed from a header stored with
/// [`put_table_header`](Self::put_table_header), and
/// [`get_table_header`](Self::get_table_header) writes them, but for
/// `THEAP`, from the table as it is.
/// [`from_header`](Self::from_header) creates a FitsTable with the columns
/// a header describes.
///
/// # Attributes
///
/// A FitsTable has the attributes of a Table, read through
/// [`as_table`](Self::as_table).
///
/// # Examples
///
/// ```
/// use ast_core::{AstType, ColumnData, FitsTable};
///
/// let mut table = FitsTable::new().unwrap();
/// table.add_column("Flux", AstType::Double, &[], "Jy")?;
/// table.add_column("Flag", AstType::Int, &[], "")?;
/// table.put_column_data("FLUX", &ColumnData::Double(vec![1.5, 2.5, 3.5]))?;
/// table.put_0i("FLAG(2)", 1, None)?;
/// assert_eq!(table.as_table().nrow(), 3);
///
/// // The empty cells of an integer column read as its null value.
/// let null = table.column_null("FLAG", None, false)?.value as i32;
/// let flags = table.get_column_data("FLAG", f32::NAN, f64::NAN, None)?;
/// assert_eq!(flags, ColumnData::Int(vec![null, 1, null]));
///
/// let mut header = table.get_table_header()?;
/// assert_eq!(header.get_fits_s(Some("XTENSION"))?.as_deref(), Some("BINTABLE"));
/// assert_eq!(header.get_fits_i(Some("NAXIS2"))?, Some(3));
/// assert_eq!(header.get_fits_s(Some("TFORM1"))?.as_deref(), Some("1D"));
/// assert_eq!(header.get_fits_s(Some("TUNIT1"))?.as_deref(), Some("Jy"));
/// # Ok::<(), ast_core::error::AstError>(())
/// ```
#[derive(Debug, Clone)]
pub struct FitsTable {
    /// The column data and parameters.
    table: Table,
    /// The FITS header cards.
    header: FitsChan,
}

impl FitsTable {
    /// Creates an empty FitsTable, with no columns and an empty header.
    ///
    /// [`from_header`](Self::from_header) creates one with the columns a
    /// FITS header describes.
    ///
    /// # Errors
    ///
    /// The errors of [`FitsChan::new`], which makes the header.
    // astFitsTable with a NULL header; its header is made by astFitsChan.
    #[doc(alias = "astFitsTable")]
    pub fn new() -> AstResult<Self> {
        let mut table = Table::new();
        table.set_class("FitsTable");
        Ok(FitsTable {
            table,
            header: FitsChan::new()?,
        })
    }

    /// Creates a FitsTable with an empty column for each column of the FITS
    /// binary table `header` describes, and a header copied from `header`.
    ///
    /// Each column has the index in the FitsTable that it has in `header`,
    /// and takes its name from `TTYPEn` (`FCOLUMNn` when there is none),
    /// its data type and length from `TFORMn`, its shape from `TDIMn`, its
    /// unit from `TUNITn` and its null value from `TNULLn`.
    /// A header with no `TFIELDS` keyword describes no columns.
    /// The cards the table determines are left out of the copy, as by
    /// [`put_table_header`](Self::put_table_header).
    ///
    /// `header` is searched in place, so its current card is moved.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Nofts`] if `header` has no `TFORMn` keyword for a
    ///   column.
    /// - [`ErrorCode::Bdfts`] if a `TFORMn` has a negative repeat count or a
    ///   data type code other than those in [`FitsTable`]'s table, if a
    ///   `TDIMn` does not give as many elements as its `TFORMn`, or if a
    ///   column is scaled, with `TSCALn` other than 1 or `TZEROn` other than
    ///   0.
    /// - The errors of [`add_column`](Self::add_column) for a column it
    ///   cannot add, such as [`ErrorCode::Oldcol`] for two columns with the
    ///   same name and different properties.
    /// - The errors of [`column_null`](Self::column_null) for a `TNULLn` it
    ///   cannot store.
    /// - The errors of [`FitsChan::get_fits_s`] and the other
    ///   `get_fits` methods for a keyword whose value cannot be read as the
    ///   type it needs.
    // astFitsTable with a header.
    #[doc(alias = "astFitsTable")]
    pub fn from_header(header: &mut FitsChan) -> AstResult<Self> {
        let mut new = Self::new()?;
        new.generate_columns(header)?;
        new.put_table_header(header)?;
        Ok(new)
    }

    /// The FitsTable's own header, which a FITS-WCS write adds EXTVER to as
    /// C's `astWrite` does.
    pub(crate) fn header_mut(&mut self) -> &mut FitsChan {
        &mut self.header
    }

    /// The Table holding the column data and the parameters, for the Table
    /// methods that only read it.
    pub fn as_table(&self) -> &Table {
        &self.table
    }

    /// Adds a column, with no values in it, as [`Table::add_column`].
    ///
    /// A FitsTable cannot hold [`AstType::Object`] or [`AstType::Undef`]
    /// values, so a column of either type is refused.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Naxin`] if `ty` is [`AstType::Object`] or
    ///   [`AstType::Undef`].
    /// - The errors of [`Table::add_column`].
    // fitstable.c AddColumn.
    #[doc(alias = "astAddColumn")]
    pub fn add_column(
        &mut self,
        name: &str,
        ty: AstType,
        dims: &[usize],
        unit: &str,
    ) -> AstResult<()> {
        let text = match ty {
            AstType::Object => Some("Object pointer"),
            AstType::Undef => Some("undefined type"),
            _ => None,
        };
        if let Some(text) = text {
            return Err(AstError::new(
                ErrorCode::Naxin,
                format!(
                    "astAddColumn(FitsTable): Bad data type ({text}) supplied for new column \
                     {name}. The FitsTable class does not support {text} columns."
                ),
            ));
        }
        self.table.add_column(name, ty, dims, unit)
    }

    /// Add a column for each column defined in `header` (fitstable.c
    /// `GenerateColumns`).
    fn generate_columns(&mut self, header: &mut FitsChan) -> AstResult<()> {
        let bdfts = |msg: String| AstError::new(ErrorCode::Bdfts, msg);
        let ncol = header.get_fits_i(Some("TFIELDS"))?.unwrap_or(0);
        for icol in 1..=ncol.max(0) {
            let keyword = format!("TFORM{icol}");
            let Some(tform) = header.get_fits_s(Some(&keyword))? else {
                return Err(AstError::new(
                    ErrorCode::Nofts,
                    format!(
                        "astFitsTable: Supplied FITS binary table header does not contain \
                         the required keyword '{keyword}'."
                    ),
                ));
            };

            // The repeat count and data type code.
            let (repeat, nc) = scan_int(&tform).unwrap_or((1, 0));
            if repeat < 0 {
                return Err(bdfts(format!(
                    "astFitsTable: Keyword '{keyword}' in supplied FITS binary table header \
                     has unsupported value '{tform}'."
                )));
            }
            let ty = match tform.as_bytes().get(nc) {
                Some(b'B') => AstType::Byte,
                Some(b'I') => AstType::SInt,
                Some(b'J') => AstType::Int,
                Some(b'K') => AstType::KInt,
                Some(b'D') => AstType::Double,
                Some(b'E') => AstType::Float,
                Some(b'A') => AstType::String,
                _ => {
                    return Err(bdfts(format!(
                        "astFitsTable: Keyword '{keyword}' in supplied FITS binary table \
                         header has unsupported value '{tform}'."
                    )));
                }
            };

            let name = header
                .get_fits_s(Some(&format!("TTYPE{icol}")))?
                .unwrap_or_else(|| format!("FCOLUMN{icol}"));
            let unit = header
                .get_fits_s(Some(&format!("TUNIT{icol}")))?
                .unwrap_or_default();

            // The shape is given by TDIMn, "(i,j,...)", in which a string
            // column's first value is the length of each string. Without
            // it, a column of numbers holds vectors of the repeat count, or
            // scalars, and a string column holds strings of that length.
            let (nel, dims) = if let Some(tdim) = header.get_fits_s(Some(&format!("TDIM{icol}")))? {
                let ndim = 1 + tdim.bytes().filter(|&b| b == b',').count();
                let mut dims = Vec::with_capacity(ndim);
                let mut nel = 1i64;
                let mut p = tdim.strip_prefix('(').unwrap_or(&tdim);
                while dims.len() < ndim {
                    let Some((dim, nc)) = scan_int(p) else {
                        break;
                    };
                    nel *= dim;
                    dims.push(dim);
                    p = &p[nc..];
                    p = p.strip_prefix(',').unwrap_or(p);
                }
                if ty == AstType::String && !dims.is_empty() {
                    dims.remove(0);
                }
                (nel, dims)
            } else if ty == AstType::String || repeat == 1 {
                (repeat, Vec::new())
            } else {
                (repeat, vec![repeat])
            };

            if repeat != nel {
                let tform = header
                    .get_fits_s(Some(&format!("TFORM{icol}")))?
                    .unwrap_or_default();
                let tdim = header
                    .get_fits_s(Some(&format!("TDIM{icol}")))?
                    .unwrap_or_else(|| " ".to_string());
                return Err(bdfts(format!(
                    "astFitsTable: Supplied FITS binary table header contains inconsistent \
                     TFORM ({tform}) and TDIM ({tdim}) keywords for field {icol}."
                )));
            }

            for (keyword, unscaled) in [("TSCAL", 1.0), ("TZERO", 0.0)] {
                if header
                    .get_fits_f(Some(&format!("{keyword}{icol}")))?
                    .is_some_and(|v| v != unscaled)
                {
                    return Err(bdfts(
                        "astFitsTable: Supplied FITS binary table header contains scaled \
                         columns which are not supported by AST."
                            .to_string(),
                    ));
                }
            }

            // A length below one is reported by AddColumn. A negative
            // length is passed as zero, which the error message then
            // shows in its place.
            let dims: Vec<usize> = dims.into_iter().map(|d| d.max(0) as usize).collect();
            self.add_column(&name, ty, &dims, &unit)?;

            if let Some(null) = header.get_fits_k(Some(&format!("TNULL{icol}")))? {
                self.column_null(&name, Some(null), false)?;
            }
        }
        Ok(())
    }

    /// Gets, and with `set` also sets, the null value of an integer column.
    ///
    /// An element of an integer column is null when it equals the column's
    /// null value, or when it is in a cell in which no value has been
    /// stored.
    /// The null value is written to the header as the column's `TNULLn`
    /// keyword, and is the value [`get_column_data`](Self::get_column_data)
    /// gives for the elements of an empty cell.
    /// Only an integer column ([`AstType::Byte`], [`AstType::SInt`],
    /// [`AstType::Int`] or [`AstType::KInt`]) has a null value; for any
    /// other column this does nothing, and returns a value of 0 that was
    /// not set and, if asked for, no nulls.
    ///
    /// With `set`, the null value becomes `set`, replacing any set before.
    /// The value returned, [`ColumnNull::value`], is the null value in use
    /// before the call: the one set by an earlier call if there was one,
    /// otherwise `set` if given, otherwise a default that does not occur in
    /// the column.
    /// When every value the column's type can hold occurs in the column, the
    /// default is 0 if the column has no empty cells, and an error if it
    /// has; a header written then has no `TNULLn` keyword for the column.
    /// [`ColumnNull::was_set`] says whether the value returned was set by an
    /// earlier call.
    ///
    /// With `check_nulls`, [`ColumnNull::has_null`] says whether the column
    /// holds any null element: an element equal to the null value in use
    /// after the call, or an element of an empty cell.
    /// Without it, `has_null` is `None` and the column is not searched.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Badcol`] if there is no column called `column`.
    /// - [`ErrorCode::Badnull`] if `set` is outside the range of the
    ///   column's type, when the null value is unchanged.
    /// - [`ErrorCode::Badnull`] if a default is needed, every value the
    ///   column's type can hold occurs in the column, and the column has an
    ///   empty cell.
    #[doc(alias = "astColumnNull")]
    #[doc(alias = "astColumnNullK")]
    pub fn column_null(
        &mut self,
        column: &str,
        set: Option<i64>,
        check_nulls: bool,
    ) -> AstResult<ColumnNull> {
        self.null_value(column, set, check_nulls)
    }

    /// [`FitsTable::column_null`], which changes only the column's
    /// definition, through the Table's shared access to it (fitstable.c
    /// `ColumnNullK`).
    fn null_value(
        &self,
        column: &str,
        set: Option<i64>,
        check_nulls: bool,
    ) -> AstResult<ColumnNull> {
        let mut result = ColumnNull {
            value: 0,
            was_set: false,
            has_null: check_nulls.then_some(false),
        };
        let (imin, imax) = match self.table.column_type(column)? {
            AstType::Byte => (0, i64::from(u8::MAX)),
            AstType::SInt => (i64::from(i16::MIN), i64::from(i16::MAX)),
            AstType::Int => (i64::from(i32::MIN), i64::from(i32::MAX)),
            AstType::KInt => (i64::MIN, i64::MAX),
            _ => (0, 0),
        };
        if imax <= imin {
            return Ok(result);
        }

        // The null value already set, and the one in use on exit.
        let Some(old) = self.table.with_column_def(column, |col| {
            let old = col.get_0k(NULLKEY)?;
            if let Some(null) = set
                && (imin..=imax).contains(&null)
            {
                col.put_0k(NULLKEY, null, None)?;
            }
            Ok(old)
        })?
        else {
            return Ok(result);
        };
        if let Some(null) = set
            && !(imin..=imax).contains(&null)
        {
            return Err(AstError::new(
                ErrorCode::Badnull,
                format!(
                    "astColumnNull(FitsTable): Supplied null value ({null}) is outside the \
                     range of integers that can be stored in column '{column}'."
                ),
            ));
        }
        result.was_set = old.is_some();
        result.value = old.unwrap_or(0);
        let (null, gotresult) = match set {
            Some(newval) => {
                if old.is_none() {
                    result.value = newval;
                }
                (newval, true)
            }
            None => (result.value, result.was_set),
        };
        if gotresult && !check_nulls {
            return Ok(result);
        }

        // Look for the null value, or for a default: the highest or the
        // lowest allowed value, moving both inwards and starting again
        // whenever both are found.
        let nel = self.table.column_length(column)?;
        let mut nullfound = false;
        let (mut nullhi, mut nulllo) = (imax, imin);
        let (mut foundhi, mut foundlo) = (false, false);
        let nrow = self.table.nrow();
        let mut irow = 1;
        while irow <= nrow {
            let mut restart = false;
            match self.table.get_1k(&cell_key(column, irow))? {
                Some(cell) => {
                    let cell = &cell[..nel.min(cell.len())];
                    if gotresult {
                        if !nullfound && cell.contains(&null) {
                            nullfound = true;
                        }
                    } else if nulllo <= nullhi {
                        for &value in cell {
                            if value == nulllo {
                                foundlo = true;
                            } else if value == nullhi {
                                foundhi = true;
                            }
                            // When every allowed value has been tried, both
                            // flags are left set so that no default is
                            // chosen.
                            if foundlo && foundhi {
                                nullhi -= 1;
                                nulllo += 1;
                                if nulllo <= nullhi {
                                    restart = true;
                                    foundlo = false;
                                    foundhi = false;
                                }
                            }
                        }
                    }
                }
                None => nullfound = true,
            }
            if gotresult && nullfound {
                break;
            }
            irow = if restart { 1 } else { irow + 1 };
        }

        if check_nulls {
            result.has_null = Some(nullfound);
        }
        if !gotresult {
            if !foundhi {
                result.value = nullhi;
            } else if !foundlo {
                result.value = nulllo;
            } else if nullfound {
                return Err(AstError::new(
                    ErrorCode::Badnull,
                    format!(
                        "astColumnNull(FitsTable): Cannot find an unused value to use as the \
                         null value in column '{column}'."
                    ),
                ));
            }
        }
        Ok(result)
    }

    /// The size of the column's element type in bytes, or an internal
    /// error naming `method` for a type a FitsTable cannot hold.
    fn column_elem_size(&self, column: &str, ty: AstType, method: &str) -> AstResult<usize> {
        match ty {
            AstType::String => self.table.column_len_c(column),
            AstType::Int
            | AstType::KInt
            | AstType::Double
            | AstType::Float
            | AstType::SInt
            | AstType::Byte => Ok(elem_size(ty, 0)),
            _ => Err(AstError::new(
                ErrorCode::Inter,
                format!(
                    "{method}(FitsTable): Unsupported column type {} (internal AST \
                     programming error).",
                    ty.code()
                ),
            )),
        }
    }

    /// The number of bytes needed to hold every value in `column`.
    ///
    /// Each element counts as 1 byte in an [`AstType::Byte`] column, 2 in
    /// an [`AstType::SInt`] column, 4 in an [`AstType::Int`] or
    /// [`AstType::Float`] column, 8 in an [`AstType::KInt`] or
    /// [`AstType::Double`] column, and the length of the longest string
    /// ([`Table::column_len_c`]) in an [`AstType::String`] column, as in the
    /// FITS binary table and in the [`ColumnData`] that
    /// [`get_column_data`](Self::get_column_data) gives.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Badcol`] if there is no column called `column`.
    /// - [`ErrorCode::Inter`] if the column holds a type a FitsTable cannot
    ///   hold.
    #[doc(alias = "astColumnSize")]
    pub fn column_size(&self, column: &str) -> AstResult<usize> {
        let ty = self.table.column_type(column)?;
        let nb = self.column_elem_size(column, ty, "astColumnSize")?;
        Ok(nb * self.table.column_length(column)? * self.table.nrow())
    }

    /// The values in `column`, row by row, from the first row, for at most
    /// `max_rows` rows.
    ///
    /// No type conversion is done: the values have the column's own type,
    /// in the [`ColumnData`] variant for it.
    /// The elements of an array in each cell are in Fortran order.
    /// Strings have the length of the column's longest string
    /// ([`Table::column_len_c`]), a shorter one being padded with nulls.
    ///
    /// The elements of an empty cell are the column's null value
    /// ([`column_null`](Self::column_null)) in an integer column, all nulls
    /// in a string column, `fnull` in an [`AstType::Float`] column and
    /// `dnull` in an [`AstType::Double`] column.
    /// An `fnull` or `dnull` that is neither infinite nor NaN also replaces
    /// any infinite or NaN value stored in the column.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Badcol`] if there is no column called `column`.
    /// - [`ErrorCode::Inter`] if the column holds a type a FitsTable cannot
    ///   hold.
    /// - [`ErrorCode::Badnull`] if the integer column has an empty cell and
    ///   every value its type can hold occurs in it, so that it has no null
    ///   value, as for [`column_null`](Self::column_null).
    // The C documentation says strings are returned null terminated; C's
    // GetColumnData copies them without the terminator, as here, and
    // replaces non-finite values as here although the documentation does
    // not say so (docs/issues/c-library-quirks.md). C limits the rows by the
    // size of the caller's array, for which max_rows stands.
    #[doc(alias = "astGetColumnData")]
    pub fn get_column_data(
        &self,
        column: &str,
        fnull: f32,
        dnull: f64,
        max_rows: Option<usize>,
    ) -> AstResult<ColumnData> {
        let ty = self.table.column_type(column)?;
        let nb = self.column_elem_size(column, ty, "astGetColumnData")?;
        let nel = self.table.column_length(column)?;
        let mut nrow = self.table.nrow();
        if let Some(max_rows) = max_rows {
            nrow = nrow.min(max_rows);
        }

        // The null value of an integer column, found when first needed.
        let mut int_null = None;
        let mut int_null = |table: &Self| -> AstResult<i64> {
            if int_null.is_none() {
                int_null = Some(table.null_value(column, None, false)?.value);
            }
            Ok(int_null.expect("set above"))
        };

        // One cell's values, each fitted to the cell length as C's fixed
        // buffer does, or `None` for an empty cell.
        fn fit<T: Clone>(v: Option<Vec<T>>, nel: usize, pad: T) -> Option<Vec<T>> {
            v.map(|mut v| {
                v.resize(nel, pad);
                v
            })
        }

        macro_rules! collect {
            ($variant:ident, $get:ident, $t:ty, $null:expr, $clean:expr) => {{
                let mut out: Vec<$t> = Vec::with_capacity(nel * nrow);
                for irow in 1..=nrow {
                    let key = cell_key(column, irow);
                    match fit(self.table.$get(&key)?, nel, <$t>::default()) {
                        Some(mut cell) => {
                            $clean(&mut cell);
                            out.extend(cell);
                        }
                        None => {
                            let null: $t = $null;
                            out.extend(std::iter::repeat_n(null, nel));
                        }
                    }
                }
                ColumnData::$variant(out)
            }};
        }

        Ok(match ty {
            // Integer nulls are converted to the column type as C assigns
            // them, keeping the low-order bits.
            AstType::Int => collect!(Int, get_1i, i32, int_null(self)? as i32, |_: &mut Vec<
                i32,
            >| {}),
            AstType::KInt => collect!(KInt, get_1k, i64, int_null(self)?, |_: &mut Vec<i64>| {}),
            AstType::SInt => collect!(SInt, get_1s, i16, int_null(self)? as i16, |_: &mut Vec<
                i16,
            >| {}),
            AstType::Byte => collect!(Byte, get_1b, u8, int_null(self)? as u8, |_: &mut Vec<
                u8,
            >| {}),
            AstType::Double => collect!(Double, get_1d, f64, dnull, |cell: &mut Vec<f64>| {
                if dnull.is_finite() {
                    cell.iter_mut()
                        .filter(|v| !v.is_finite())
                        .for_each(|v| *v = dnull);
                }
            }),
            AstType::Float => collect!(Float, get_1f, f32, fnull, |cell: &mut Vec<f32>| {
                if fnull.is_finite() {
                    cell.iter_mut()
                        .filter(|v| !v.is_finite())
                        .for_each(|v| *v = fnull);
                }
            }),
            AstType::String => {
                // Each string truncated or padded with nulls to `nb` bytes,
                // as astMapGet1C's strncpy leaves it.
                let mut values = Vec::with_capacity(nel * nrow);
                for irow in 1..=nrow {
                    let cell = self.table.get_1c(&cell_key(column, irow))?;
                    for i in 0..nel {
                        let s = cell
                            .as_ref()
                            .and_then(|c| c.get(i))
                            .map_or(&[][..], |s| s.as_bytes());
                        let mut value = s[..s.len().min(nb)].to_vec();
                        value.resize(nb, 0);
                        values.push(value);
                    }
                }
                ColumnData::String { len: nb, values }
            }
            _ => unreachable!("column_elem_size rejects other types"),
        })
    }

    /// Stores `data` in `column`, one cell per row from the first row, and
    /// removes the values in the rows that follow, leaving their cells
    /// empty.
    ///
    /// No type conversion is done: `data` must be the [`ColumnData`]
    /// variant for the column's type, and must hold a whole number of
    /// cells, each of [`Table::column_length`] elements in Fortran order.
    /// Each string is the first `len` bytes of a fixed-length string, up to
    /// the first null; `len` need not be the length of the strings already
    /// in the column.
    /// A cell of an [`AstType::Float`] or [`AstType::Double`] column whose
    /// elements are all infinite or NaN is not stored, and keeps any value
    /// it held.
    /// Storing more rows than the table has adds rows.
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Badcol`] if there is no column called `column`.
    /// - [`ErrorCode::Badtyp`] if `data` is not the variant for the
    ///   column's type.
    /// - [`ErrorCode::Badsiz`] if `data` does not hold a whole number of
    ///   cells; the message gives the sizes in bytes, counted as for
    ///   [`column_size`](Self::column_size) with strings of `len` bytes.
    /// - [`ErrorCode::Inter`] if the column holds a type a FitsTable cannot
    ///   hold.
    /// - The errors of the `put_1` methods, such as [`Table::put_1i`], which
    ///   store each cell.
    // The skipping of non-finite cells follows fitstable.c PutColumnData; its
    // prologue does not mention it (c-library-quirks.md, "The
    // `astPutColumnData` documentation does not say a cell of non-finite
    // values keeps its old value").
    // C's astPutColumnData takes the data as untyped memory and so cannot
    // check its type.
    #[doc(alias = "astPutColumnData")]
    pub fn put_column_data(&mut self, column: &str, data: &ColumnData) -> AstResult<()> {
        let ty = self.table.column_type(column)?;
        let nb = match (ty, data) {
            (AstType::String, ColumnData::String { len, .. }) => *len,
            (AstType::String, _) => 0,
            _ => self.column_elem_size(column, ty, "astPutColumnData")?,
        };
        if data.data_type() != ty {
            return Err(AstError::new(
                ErrorCode::Badtyp,
                format!(
                    "astPutColumnData(FitsTable): The supplied data have type {:?} but \
                     column '{column}' has type {ty:?}.",
                    data.data_type()
                ),
            ));
        }
        let nel = self.table.column_length(column)?;
        let nbv = nb * nel;
        let size = data.size();
        let nrow = size.checked_div(nbv).unwrap_or(0);
        if nrow * nbv != size {
            return Err(AstError::new(
                ErrorCode::Badsiz,
                format!(
                    "astPutColumnData(FitsTable): The supplied array size ({size} bytes) is \
                     not an exact multiple of the size of one column value ({nbv} bytes)."
                ),
            ));
        }

        for irow in 1..=nrow {
            let key = cell_key(column, irow);
            let cell = (irow - 1) * nel..irow * nel;
            match data {
                ColumnData::Int(v) => self.table.put_1i(&key, &v[cell], None)?,
                ColumnData::KInt(v) => self.table.put_1k(&key, &v[cell], None)?,
                ColumnData::SInt(v) => self.table.put_1s(&key, &v[cell], None)?,
                ColumnData::Byte(v) => self.table.put_1b(&key, &v[cell], None)?,
                ColumnData::Double(v) => {
                    if v[cell.clone()].iter().any(|x| x.is_finite()) {
                        self.table.put_1d(&key, &v[cell], None)?;
                    }
                }
                ColumnData::Float(v) => {
                    if v[cell.clone()].iter().any(|x| x.is_finite()) {
                        self.table.put_1f(&key, &v[cell], None)?;
                    }
                }
                ColumnData::String { values, .. } => {
                    let strings: Vec<String> = values[cell]
                        .iter()
                        .map(|s| {
                            let s = &s[..s.len().min(nb)];
                            let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
                            String::from_utf8_lossy(&s[..end]).into_owned()
                        })
                        .collect();
                    let strings: Vec<&str> = strings.iter().map(String::as_str).collect();
                    self.table.put_1c(&key, &strings, None)?;
                }
            }
        }

        for irow in nrow + 1..=self.table.nrow() {
            self.table.map_remove(&cell_key(column, irow))?;
        }
        Ok(())
    }

    /// A copy of the FITS header, with the cards that describe the table
    /// written from the table as it is, and its first card current.
    ///
    /// The cards written are `XTENSION`, `BITPIX`, `NAXIS`, `NAXIS1`,
    /// `NAXIS2`, `PCOUNT`, `GCOUNT` and `TFIELDS`, and for each column
    /// `TFORMn`, `TTYPEn`, `TUNITn` (when the unit is not blank), `TNULLn`
    /// (for an integer column whose null value has been set or which holds
    /// nulls; see [`column_null`](Self::column_null)) and `TDIMn` (for a
    /// column of arrays).
    /// The FitsTable's own header is updated too.
    /// Changing the copy does not change the FitsTable; store a changed
    /// header with [`put_table_header`](Self::put_table_header).
    ///
    /// # Errors
    ///
    /// - [`ErrorCode::Badnull`] if an integer column has an empty cell and
    ///   no value its type can hold is unused, as for
    ///   [`column_null`](Self::column_null).
    /// - [`ErrorCode::Inter`] if a column holds a type a FitsTable cannot
    ///   hold.
    #[doc(alias = "astGetTableHeader")]
    pub fn get_table_header(&mut self) -> AstResult<FitsChan> {
        self.update_header("astGetTableHeader")?;
        self.header.clear_card()?;
        Ok(self.header.clone())
    }

    /// Replaces the FITS header with a copy of `header`, less the cards set
    /// by the FITS standard or by the columns and rows of the table.
    ///
    /// The cards removed are `XTENSION`, `BITPIX`, `NAXIS`, `NAXIS1`,
    /// `NAXIS2`, `PCOUNT`, `GCOUNT`, `TFIELDS` and `THEAP`, and every
    /// `TFORMn`, `TTYPEn`, `TNULLn`, `TDIMn` and `TUNITn`;
    /// [`get_table_header`](Self::get_table_header) writes them from the
    /// table.
    /// The copy keeps the attributes of `header`.
    ///
    /// # Errors
    ///
    /// Any error of [`FitsChan::find_fits`] or [`FitsChan::del_fits`], with
    /// which the cards are removed.
    // The C documentation's list of the cards removed leaves out TUNIT%d,
    // which PurgeHeader removes (docs/issues/c-library-quirks.md).
    #[doc(alias = "astPutTableHeader")]
    pub fn put_table_header(&mut self, header: &FitsChan) -> AstResult<()> {
        self.header = header.clone();
        self.purge_header()
    }

    /// Remove the cards set by the FITS standard or by the contents of the
    /// table (fitstable.c `PurgeHeader`).
    fn purge_header(&mut self) -> AstResult<()> {
        for keyword in FIXED {
            self.header.clear_card()?;
            while self.header.find_fits(keyword, false)?.is_some() {
                self.header.del_fits()?;
            }
        }
        Ok(())
    }

    /// Write the cards set by the FITS standard or by the contents of the
    /// table, naming `method` in errors (fitstable.c `UpdateHeader`).
    fn update_header(&mut self, method: &str) -> AstResult<()> {
        self.purge_header()?;

        let header = &mut self.header;
        header.clear_card()?;
        header.set_fits_s("XTENSION", "BINTABLE", "", false)?;
        header.set_fits_i("BITPIX", 8, "", false)?;
        header.set_fits_i("NAXIS", 2, "", false)?;
        header.set_fits_i("PCOUNT", 0, "", false)?;
        header.set_fits_i("GCOUNT", 1, "", false)?;
        let ncol = self.table.ncolumn();
        header.set_fits_i("TFIELDS", ncol as i32, "", false)?;

        let mut rowsize = 0;
        for icol in 1..=ncol {
            let name = self.table.column_name(icol)?;
            let mut nel = self.table.column_length(&name)?;
            let ty = self.table.column_type(&name)?;
            let unit = self.table.column_unit(&name)?;
            let dims = self.table.column_shape(&name)?;

            let mut slen = 0;
            let code = match ty {
                AstType::Byte => 'B',
                AstType::SInt => 'I',
                AstType::Int => 'J',
                AstType::KInt => 'K',
                AstType::Double => 'D',
                AstType::Float => 'E',
                AstType::String => {
                    slen = self.table.column_len_c(&name)?;
                    nel *= slen;
                    'A'
                }
                _ => {
                    return Err(AstError::new(
                        ErrorCode::Inter,
                        format!(
                            "{method}(FitsTable): Illegal type {} for column '{name}' in a \
                             FitsTable (internal AST programming error).",
                            ty.code()
                        ),
                    ));
                }
            };
            rowsize += nel * elem_size(ty, 1);

            let header = &mut self.header;
            header.set_fits_s(&format!("TFORM{icol}"), &format!("{nel}{code}"), "", false)?;
            header.set_fits_s(&format!("TTYPE{icol}"), &name, "", false)?;
            if chr_len(&unit) > 0 {
                header.set_fits_s(&format!("TUNIT{icol}"), &unit, "", false)?;
            }

            // A null value is written only when it has been set, or when
            // the column holds nulls.
            if matches!(
                ty,
                AstType::Byte | AstType::SInt | AstType::Int | AstType::KInt
            ) {
                let null = self.null_value(&name, None, true)?;
                if null.was_set || null.has_null == Some(true) {
                    self.header
                        .set_fits_k(&format!("TNULL{icol}"), null.value, "", false)?;
                }
            }

            // For strings, the first dimension is the length of each
            // string.
            if !dims.is_empty() {
                let mut tdim: Vec<String> = Vec::with_capacity(dims.len() + 1);
                if ty == AstType::String {
                    tdim.push(slen.to_string());
                }
                tdim.extend(dims.iter().map(usize::to_string));
                self.header.set_fits_s(
                    &format!("TDIM{icol}"),
                    &format!("({})", tdim.join(",")),
                    "",
                    false,
                )?;
            }
        }

        // The NAXISn cards follow NAXIS, from the fourth card.
        let nrow = self.table.nrow();
        let header = &mut self.header;
        header.set_card(4)?;
        header.set_fits_i("NAXIS1", rowsize as i32, "", false)?;
        header.set_fits_i("NAXIS2", nrow as i32, "", false)?;
        Ok(())
    }
}

// The Table methods that change the Table, each the Table's own.
macro_rules! forward_puts {
    ($(($put0:ident, $put1:ident, $put_elem:ident, $val:ty, $vals:ty, $elem_val:ty)),* $(,)?) => {
        impl FitsTable {
            $(
                #[doc = concat!("As [`Table::", stringify!($put0), "`].")]
                pub fn $put0(&mut self, key: &str, value: $val, comment: Option<&str>) -> AstResult<()> {
                    self.table.$put0(key, value, comment)
                }

                #[doc = concat!("As [`Table::", stringify!($put1), "`].")]
                pub fn $put1(&mut self, key: &str, values: $vals, comment: Option<&str>) -> AstResult<()> {
                    self.table.$put1(key, values, comment)
                }

                #[doc = concat!("As [`Table::", stringify!($put_elem), "`].")]
                pub fn $put_elem(&mut self, key: &str, elem: usize, value: $elem_val) -> AstResult<()> {
                    self.table.$put_elem(key, elem, value)
                }
            )*
        }
    };
}

forward_puts!(
    (put_0i, put_1i, put_elem_i, i32, &[i32], i32),
    (put_0s, put_1s, put_elem_s, i16, &[i16], i16),
    (put_0k, put_1k, put_elem_k, i64, &[i64], i64),
    (put_0b, put_1b, put_elem_b, u8, &[u8], u8),
    (put_0f, put_1f, put_elem_f, f32, &[f32], f32),
    (put_0d, put_1d, put_elem_d, f64, &[f64], f64),
    (put_0c, put_1c, put_elem_c, &str, &[&str], &str),
    (
        put_0a,
        put_1a,
        put_elem_a,
        AnyObject,
        Vec<Option<AnyObject>>,
        Option<AnyObject>
    ),
);

impl FitsTable {
    /// As [`Table::remove_column`].
    pub fn remove_column(&mut self, name: &str) -> AstResult<()> {
        self.table.remove_column(name)
    }

    /// As [`Table::add_parameter`].
    pub fn add_parameter(&mut self, name: &str) -> AstResult<()> {
        self.table.add_parameter(name)
    }

    /// As [`Table::remove_parameter`].
    pub fn remove_parameter(&mut self, name: &str) -> AstResult<()> {
        self.table.remove_parameter(name)
    }

    /// As [`Table::remove_row`].
    pub fn remove_row(&mut self, index: usize) -> AstResult<()> {
        self.table.remove_row(index)
    }

    /// As [`Table::purge_rows`].
    pub fn purge_rows(&mut self) -> AstResult<()> {
        self.table.purge_rows()
    }

    /// As [`Table::put_u`].
    pub fn put_u(&mut self, key: &str, comment: Option<&str>) -> AstResult<()> {
        self.table.put_u(key, comment)
    }

    /// As [`Table::map_remove`].
    pub fn map_remove(&mut self, key: &str) -> AstResult<()> {
        self.table.map_remove(key)
    }

    /// As [`Table::map_rename`].
    pub fn map_rename(&mut self, old_key: &str, new_key: &str) -> AstResult<()> {
        self.table.map_rename(old_key, new_key)
    }

    /// As [`Table::map_iterate`].
    pub fn map_iterate(&mut self, reset: bool) -> Option<String> {
        self.table.map_iterate(reset)
    }

    /// As [`Table::set_key_error`].
    pub fn set_key_error(&mut self, key_error: bool) {
        self.table.set_key_error(key_error);
    }

    /// As [`Table::clear_key_error`].
    pub fn clear_key_error(&mut self) {
        self.table.clear_key_error();
    }

    /// As [`Table::set_map_locked`].
    pub fn set_map_locked(&mut self, map_locked: bool) {
        self.table.set_map_locked(map_locked);
    }

    /// As [`Table::clear_map_locked`].
    pub fn clear_map_locked(&mut self) {
        self.table.clear_map_locked();
    }

    /// As [`Table::set_sort_by`].
    pub fn set_sort_by(&mut self, sort_by: SortBy) {
        self.table.set_sort_by(sort_by);
    }

    /// As [`Table::clear_sort_by`].
    pub fn clear_sort_by(&mut self) {
        self.table.clear_sort_by();
    }

    /// As [`Table::set_size_guess`].
    pub fn set_size_guess(&mut self, sizeguess: usize) -> AstResult<()> {
        self.table.set_size_guess(sizeguess)
    }

    /// As [`Table::clear_size_guess`].
    pub fn clear_size_guess(&mut self) -> AstResult<()> {
        self.table.clear_size_guess()
    }

    /// As [`Table::set_key_case`].
    pub fn set_key_case(&mut self, key_case: bool) -> AstResult<()> {
        self.table.set_key_case(key_case)
    }

    /// As [`Table::clear_key_case`].
    pub fn clear_key_case(&mut self) -> AstResult<()> {
        self.table.clear_key_case()
    }

    /// As [`Table::set_ident`].
    pub fn set_ident(&mut self, ident: &str) {
        self.table.set_ident(ident);
    }

    /// As [`Table::clear_ident`].
    pub fn clear_ident(&mut self) {
        self.table.clear_ident();
    }

    /// As [`Table::set_id`].
    pub fn set_id(&mut self, id: &str) {
        self.table.set_id(id);
    }

    /// As [`Table::clear_id`].
    pub fn clear_id(&mut self) {
        self.table.clear_id();
    }

    /// A copy of the column definitions, as [`Table::column_props`].
    ///
    /// The definition of an integer column whose null value has been set
    /// (see [`column_null`](Self::column_null)) also holds it, under
    /// `Null`.
    pub fn column_props(&self) -> KeyMap {
        self.table.column_props()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_scan_as_sscanf_reads_them() {
        assert_eq!(scan_int("6B"), Some((6, 1)));
        assert_eq!(scan_int(" 12J"), Some((12, 3)));
        assert_eq!(scan_int("-3,4"), Some((-3, 2)));
        assert_eq!(scan_int("A"), None);
        assert_eq!(scan_int(""), None);
        assert_eq!(scan_int("+"), None);
    }
}
