// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Copyright (C) 2013-2016, NumFOCUS Foundation.
// All rights reserved.
//
// This library is derived, with permission, from the International
// Astronomical Union's "Standards of Fundamental Astronomy" library,
// available from http://www.iausofa.org.
//
// The ERFA version is intended to retain identical functionality to
// the SOFA library, but made distinct through different function and
// file names, as set out in the SOFA license conditions.  The SOFA
// original has a role as a reference standard for the IAU and IERS,
// and consequently redistribution is permitted only in its unaltered
// state.  The ERFA version is not subject to this restriction and
// therefore can be included in distributions which do not support the
// concept of "read only" software.
//
// Although the intent is to replicate the SOFA API (other than
// replacement of prefix names) and results (with the exception of
// bugs;  any that are discovered will be fixed), SOFA is not
// responsible for any errors found in this version of the library.
//
// If you wish to acknowledge the SOFA heritage, please acknowledge
// that you are using a library derived from SOFA, rather than SOFA
// itself.
//
//
// TERMS AND CONDITIONS
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
//
// 1 Redistributions of source code must retain the above copyright
//   notice, this list of conditions and the following disclaimer.
//
// 2 Redistributions in binary form must reproduce the above copyright
//   notice, this list of conditions and the following disclaimer in
//   the documentation and/or other materials provided with the
//   distribution.
//
// 3 Neither the name of the Standards Of Fundamental Astronomy Board,
//   the International Astronomical Union nor the names of its
//   contributors may be used to endorse or promote products derived
//   from this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
// FOR A PARTICULAR PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE
// COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
// INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
// BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
// LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
// LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
// ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
// POSSIBILITY OF SUCH DAMAGE.
//
// Ported from ERFA eraCal2jd (erfa/cal2jd.c), ERFA 2.0.1 as vendored at reference/starlink-ast 3d3f255f.

use super::ERFA_DJM0;

/// Why [`cal2jd`] gave no result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cal2jdError {
    /// Year earlier than -4799 (ERFA status -1).
    BadYear,
    /// Month outside 1 to 12 (ERFA status -2).
    BadMonth,
}

/// A warning that comes with [`cal2jd`]'s result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cal2jdWarning {
    /// The day is not a day of the month (ERFA status -3). The result is the
    /// date the arithmetic gives, as for a valid day.
    BadDay,
}

/// Gregorian calendar date to a two-part Julian date `(2400000.5, MJD)`.
pub(crate) fn cal2jd(
    iy: i32,
    im: i32,
    id: i32,
) -> Result<((f64, f64), Option<Cal2jdWarning>), Cal2jdError> {
    // Earliest year allowed (4800BC).
    const IYMIN: i32 = -4799;

    // Month lengths in days.
    const MTAB: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

    // Validate year and month.
    if iy < IYMIN {
        return Err(Cal2jdError::BadYear);
    }
    if !(1..=12).contains(&im) {
        return Err(Cal2jdError::BadMonth);
    }

    // If February in a leap year, 1, otherwise 0.
    let ly = i32::from(im == 2 && iy % 4 == 0 && (iy % 100 != 0 || iy % 400 == 0));

    // Validate day, taking into account leap years.
    let warning = if id < 1 || id > MTAB[(im - 1) as usize] + ly {
        Some(Cal2jdWarning::BadDay)
    } else {
        None
    };

    // Return result. `iy` is at least IYMIN and `im` is 1 to 12, so none of
    // the int arithmetic overflows.
    let my = (im - 14) / 12;
    let iypmy = i64::from(iy + my);
    let djm = ((1461 * (iypmy + 4800)) / 4 + (367 * i64::from(im - 2 - 12 * my)) / 12
        - (3 * ((iypmy + 4900) / 100)) / 4
        + i64::from(id)
        - 2432076) as f64;
    Ok(((ERFA_DJM0, djm), warning))
}
