//! Library-wide constants.

/// The AST "bad value" sentinel, equivalent to C's `AST__BAD`.
///
/// Used to mark undefined or out-of-bounds coordinate values in outputs.
/// Inputs to transform functions also accept `f64::NAN`; outputs always
/// use this value.
pub const BAD_VALUE: f64 = -f64::MAX;

/// The C library's `PACKAGE_STRING`, its name and version, as the build of
/// the AST release this port tracks defines it (`version.number` in that
/// release). It names the generator in a Moc's FITS header (`MOCTOOL`).
pub const PACKAGE_STRING: &str = "ast 9.5.0";
