// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The comparison the C-oracle tests make between the port and C values
//! captured on another platform (`common::within_libm_rounding`).

use starlink_ast::constants::BAD_VALUE;

use crate::common::{LIBM_ATOL, LIBM_RTOL, within_libm_rounding};

#[test]
fn accepts_libm_rounding_and_rejects_more() {
    // The largest differences the Linux CI run showed.
    assert!(within_libm_rounding(0.0077360 + 2.78e-16, 0.0077360));
    assert!(within_libm_rounding(2.7127172622538644, 2.712717262253863));
    // Ten times the tolerance is a real difference.
    let x = 1.25;
    assert!(!within_libm_rounding(
        x + 10.0 * (LIBM_ATOL + LIBM_RTOL * x),
        x
    ));
    assert!(!within_libm_rounding(1.0e-12, 0.0));
}

#[test]
fn bad_and_nan_match_only_themselves() {
    assert!(within_libm_rounding(BAD_VALUE, BAD_VALUE));
    assert!(!within_libm_rounding(BAD_VALUE, 1.0));
    assert!(!within_libm_rounding(1.0, BAD_VALUE));
    assert!(!within_libm_rounding(f64::NAN, f64::NAN));
}
