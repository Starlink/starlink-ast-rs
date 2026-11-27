// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Library-wide constants.

/// The AST "bad value" sentinel.
///
/// Used to mark undefined or out-of-bounds coordinate values in outputs.
/// Inputs to transform functions also accept `f64::NAN`; outputs always
/// use this value.
///
/// # See also
///
/// - `AST__BAD`, the C constant with the same value.
pub const BAD_VALUE: f64 = -f64::MAX;

/// The name and version of the AST release this library implements, such as
/// `"ast 9.5.0"`.
///
/// It names the generator in a Moc's FITS header (`MOCTOOL`).
///
/// # See also
///
/// - `astVersion`, which gives the same version as one integer,
///   major × 1,000,000 + minor × 1,000 + release.
// The C build's `PACKAGE_STRING`, from `version.number` in that release.
#[doc(alias = "astVersion")]
pub const PACKAGE_STRING: &str = "ast 9.5.0";

/// The largest number of inputs or outputs a Mapping, or axes a Frame, may
/// have.
///
/// A constructor given more, or a dump that describes more, fails with
/// `ErrorCode::Badni`, `ErrorCode::Badno` or `ErrorCode::Naxin`.
///
/// # See also
///
/// - `AST__MXAXES`, the C constant with the same value.
pub const MXAXES: usize = 65536;

/// The deepest an Object may be nested inside others in a Native dump.
///
/// A dump that nests Objects deeper fails to read with `ErrorCode::Lderr`,
/// rather than overflowing the stack of the recursive reader.
/// The C library has no such limit.
pub const MAX_NATIVE_NESTING: usize = 130;

/// The deepest a MathMap expression may nest parentheses, function
/// arguments, unary operators and the right operands of `**`.
///
/// An expression nested deeper fails with `ErrorCode::Miopa`, rather than
/// overflowing the stack of the recursive parser.
/// The C library has no such limit, as its parser does not recurse.
pub const MAX_EXPRESSION_NESTING: usize = 100;

/// The deepest a units string may nest, counting each parenthesis, function
/// and operator that splits it.
///
/// A units string nested deeper fails with `ErrorCode::Badun` wherever it
/// is used, rather than overflowing the stack of the recursive parser.
/// The C library has no such limit.
pub const MAX_UNIT_NESTING: usize = 100;
