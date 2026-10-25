// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Local Apparent Sidereal Time for SkyFrame conversions, with C's table
//! of LAST values already computed.
//!
//! `calc_last` ports skyframe.c `CalcLAST`. Every LAST it computes exactly
//! goes into a table, one per observer position, DUT1 and DTAI
//! (`SetCachedLAST`), and a later request at an epoch within 0.4 day of two
//! tabulated epochs is answered by linear interpolation between them
//! (`GetCachedLAST`); C puts the error at under 0.1 arcsec. A value served
//! from the table is brought into 0 to 2π, while an exact calculation can
//! be negative, since C truncates the LAST day with `(int)`.
//!
//! C keeps the table in the SkyFrame class's virtual function table, shared
//! by every SkyFrame in the process behind a read-write lock. Here it is
//! thread-local, so the conversion path takes no lock: SkyFrames are `Send`
//! but not `Sync`, and each thread sees the LAST values that thread
//! computed. Within one thread the results follow C's, including their
//! dependence on what was converted before.

use std::cell::RefCell;
use std::f64::consts::PI;

use crate::constants::BAD_VALUE;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::FrameRef;

/// The LAST values computed for one observer position, DUT1 and DTAI
/// (skyframe.c `AstSkyLastTable`). `epoch` increases monotonically, and
/// each `last` is continuous with its neighbors rather than wrapped.
struct LastTable {
    obslat: f64,
    obslon: f64,
    obsalt: f64,
    dut1: f64,
    dtai: f64,
    epoch: Vec<f64>,
    last: Vec<f64>,
}

impl LastTable {
    /// Whether the table is for the given observer, allowing C's
    /// tolerances (skyframe.c `GetCachedLAST`, `SetCachedLAST`).
    fn is_for(&self, obslon: f64, obslat: f64, obsalt: f64, dut1: f64, dtai: f64) -> bool {
        (self.obslat - obslat).abs() < 2.0e-7
            && (self.obslon - obslon).abs() < 2.0e-7
            && (self.obsalt - obsalt).abs() < 1.0
            && (self.dut1 - dut1).abs() < 1.0e-5
            && equal(self.dtai, dtai, 1.0e-5)
    }
}

thread_local! {
    static LAST_TABLES: RefCell<Vec<LastTable>> = const { RefCell::new(Vec::new()) };
}

/// skyframe.c `EQUAL`: two bad values are equal, a bad and a good value
/// are not, and two good values are equal within `tol`.
pub(crate) fn equal(a: f64, b: f64, tol: f64) -> bool {
    if a == BAD_VALUE {
        b == BAD_VALUE
    } else {
        b != BAD_VALUE && (a - b).abs() <= tol
    }
}

/// The LAST, in radians, at TDB MJD `epoch` for the given observer
/// (skyframe.c `CalcLAST`). `dtai` is `BAD_VALUE` when unset.
pub(crate) fn calc_last(
    epoch: f64,
    obslon: f64,
    obslat: f64,
    obsalt: f64,
    dut1: f64,
    dtai: f64,
) -> AstResult<f64> {
    if let Some(last) = cached_last(epoch, obslon, obslat, obsalt, dut1, dtai) {
        return Ok(last);
    }
    let last = exact_last(epoch, obslon, obslat, obsalt, dut1, dtai)?;
    cache_last(last, epoch, obslon, obslat, obsalt, dut1, dtai);
    Ok(last)
}

/// The exact LAST, converting the Epoch from TDB to LAST through two
/// TimeFrames whose origin is the integer part of the Epoch, for accuracy.
fn exact_last(
    epoch: f64,
    obslon: f64,
    obslat: f64,
    obsalt: f64,
    dut1: f64,
    dtai: f64,
) -> AstResult<f64> {
    use crate::frames::{TimeFrame, TimeScale, TimeSystem};

    // C's `(int) epoch`, which truncates towards zero.
    let origin = epoch.trunc();
    let frame = |scale: TimeScale| -> AstResult<TimeFrame> {
        let mut f = TimeFrame::new(TimeSystem::Mjd);
        f.set_timescale(scale)?;
        f.set_time_origin(origin);
        f.set_obs_lon(obslon);
        f.set_obs_lat(obslat);
        f.set_obs_alt(obsalt);
        f.set_dut1(dut1);
        if dtai != BAD_VALUE {
            f.set_dtai(dtai);
        }
        Ok(f)
    };
    let tdb = frame(TimeScale::Tdb)?;
    let last_frame = frame(TimeScale::Last)?;
    let fs = crate::frames::find_frame::convert(&tdb, &last_frame, &[])?.ok_or_else(|| {
        AstError::new(ErrorCode::Inter, "CalcLAST: no conversion from TDB to LAST")
    })?;
    let mapping = fs.mapping(FrameRef::Base, FrameRef::Current)?;
    let pts = crate::coords::PointSet::from_columns(&[&[epoch - origin][..]])?;
    let out = mapping.transform(&pts, true)?;
    let day = out.point(0)[0];
    // The fraction of a day, from C's `(int)` truncation, so it may be
    // negative.
    Ok((day - day.trunc()) * 2.0 * PI)
}

/// The LAST at `epoch` from the table for the given observer, if the
/// table can give it (skyframe.c `GetCachedLAST`).
fn cached_last(
    epoch: f64,
    obslon: f64,
    obslat: f64,
    obsalt: f64,
    dut1: f64,
    dtai: f64,
) -> Option<f64> {
    let close = 0.001 / 86400.0;
    let result = LAST_TABLES.with_borrow(|tables| {
        let table = tables
            .iter()
            .find(|t| t.is_for(obslon, obslat, obsalt, dut1, dtai))?;
        let (ep, lp) = (&table.epoch, &table.last);
        // The earliest entry at or after `epoch`, or the last entry if
        // there is none.
        let (mut ilo, mut ihi) = (0, ep.len() - 1);
        while ihi > ilo {
            let itest = (ilo + ihi) / 2;
            if ep[itest] >= epoch {
                ihi = itest;
            } else {
                ilo = itest + 1;
            }
        }
        let dep = ep[ilo] - epoch;
        if ilo == 0 || dep <= 0.0 {
            (dep.abs() < close).then_some(lp[ilo])
        } else if ep[ilo] - ep[ilo - 1] < 0.4 {
            Some(
                lp[ilo - 1]
                    + (epoch - ep[ilo - 1]) * (lp[ilo] - lp[ilo - 1]) / (ep[ilo] - ep[ilo - 1]),
            )
        } else {
            (dep.abs() < close).then_some(lp[ilo])
        }
    });
    result.map(|mut last| {
        while last > 2.0 * PI {
            last -= 2.0 * PI;
        }
        while last < 0.0 {
            last += 2.0 * PI;
        }
        last
    })
}

/// Add an exactly computed LAST to the table for the given observer,
/// creating the table if need be (skyframe.c `SetCachedLAST`).
fn cache_last(last: f64, epoch: f64, obslon: f64, obslat: f64, obsalt: f64, dut1: f64, dtai: f64) {
    LAST_TABLES.with_borrow_mut(|tables| {
        let Some(table) = tables
            .iter_mut()
            .find(|t| t.is_for(obslon, obslat, obsalt, dut1, dtai))
        else {
            tables.push(LastTable {
                obslat,
                obslon,
                obsalt,
                dut1,
                dtai,
                epoch: vec![epoch],
                last: vec![last],
            });
            return;
        };
        // The new entry goes after every entry with an epoch no later than
        // its own. It is made continuous with the entry before it, or with
        // the entry after it when it is the earliest, so that interpolation
        // between neighbors is meaningful.
        let at = table
            .epoch
            .iter()
            .rposition(|&e| e <= epoch)
            .map_or(0, |i| i + 1);
        let reference = if at > 0 {
            table.last[at - 1]
        } else {
            table.last[0]
        };
        let last = if last > reference + PI {
            last - 2.0 * PI
        } else if last < reference - PI {
            last + 2.0 * PI
        } else {
            last
        };
        table.epoch.insert(at, epoch);
        table.last.insert(at, last);
    });
}
