//! Library-wide constants.

/// The AST "bad value" sentinel, equivalent to C's `AST__BAD`.
///
/// Used to mark undefined or out-of-bounds coordinate values in outputs.
/// Inputs to transform functions also accept `f64::NAN`; outputs always
/// use this value.
pub const BAD_VALUE: f64 = -f64::MAX;
