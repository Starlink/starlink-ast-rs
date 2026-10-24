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
