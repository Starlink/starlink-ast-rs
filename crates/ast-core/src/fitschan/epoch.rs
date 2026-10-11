//! The date of observation a foreign read gives its Frames: C's
//! `ChooseEpoch`, `TimeSysToAst` and `TDBConv` (fitschan.c:6185, 34887,
//! 33894).

use super::fits_store::FitsStore;
use super::warning::{Warning, Warnings};
use crate::error::AstResult;
use crate::frames::{Frame, FrameRef, TimeFrame, TimeScale, TimeSystem};

/// C's `TimeSysToAst`: the timescale a TIMESYS value names, UTC when there
/// is none. `UT`, `ET` and a value AST does not support raise `badval`
/// warnings.
pub(crate) fn time_sys_to_ast(timesys: Option<&str>, warnings: &mut Warnings) -> TimeScale {
    let Some(timesys) = timesys else {
        return TimeScale::Utc;
    };
    match timesys {
        "UTC" => TimeScale::Utc,
        "UT" => {
            warnings.push(Warning::new(
                "badval",
                "The original FITS header contained a value of UT for keyword TIMESYS \
                 which is being interpreted as UTC.",
            ));
            TimeScale::Utc
        }
        "TAI" | "IAT" => TimeScale::Tai,
        "ET" => {
            warnings.push(Warning::new(
                "badval",
                "The original FITS header contained a value of ET for keyword TIMESYS. \
                 TT will be used instead.",
            ));
            TimeScale::Tt
        }
        "TT" | "TDT" => TimeScale::Tt,
        "TDB" => TimeScale::Tdb,
        "TCG" => TimeScale::Tcg,
        "TCB" => TimeScale::Tcb,
        other => {
            warnings.push(Warning::new(
                "badval",
                format!(
                    "The original FITS header contained a value of {other} for keyword \
                     TIMESYS. AST does not support this timescale so UTC will be used \
                     instead."
                ),
            ));
            TimeScale::Utc
        }
    }
}

/// C's `TDBConv`: convert the MJD `mjd` from `timescale` to TDB, or with
/// `from_tdb` from TDB to `timescale`, using the Mapping `astConvert` finds
/// from an MJD TimeFrame in TDB to one in `timescale`.
pub(crate) fn tdb_conv(mjd: f64, timescale: TimeScale, from_tdb: bool) -> AstResult<f64> {
    if timescale == TimeScale::Tdb {
        return Ok(mjd);
    }
    let mut tdbframe = TimeFrame::new(TimeSystem::Mjd);
    tdbframe.set_timescale(TimeScale::Tdb)?;
    let mut timeframe = TimeFrame::new(TimeSystem::Mjd);
    timeframe.set_timescale(timescale)?;
    let fs = Frame::convert_to(&tdbframe, &timeframe, &[])?
        .expect("MJD TimeFrames in any two timescales convert");
    let map = fs.mapping(FrameRef::Base, FrameRef::Current)?;
    let pts = crate::coords::PointSet::from_rows([[mjd]]);
    Ok(map.transform(&pts, from_tdb)?.point(0)[0])
}

/// C's `ChooseEpoch`: the TDB MJD of the observation, from MJD-AVG, then
/// MJD-OBS (to which a read translates DATE-OBS when there is no MJD-OBS),
/// in the timescale TIMESYS names. The timescale is found, and any warning
/// about it raised, whether or not there is a date.
pub(crate) fn choose_epoch(store: &FitsStore, warnings: &mut Warnings) -> AstResult<Option<f64>> {
    let timescale = time_sys_to_ast(store.timesys.as_deref(), warnings);
    match store.mjdobs {
        Some(mjd) => tdb_conv(mjd, timescale, false).map(Some),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tdb_conv_round_trips() {
        for scale in [
            TimeScale::Utc,
            TimeScale::Tai,
            TimeScale::Tt,
            TimeScale::Tcb,
        ] {
            let tdb = tdb_conv(51544.0, scale, false).unwrap();
            assert_ne!(tdb, 51544.0);
            let back = tdb_conv(tdb, scale, true).unwrap();
            assert!((back - 51544.0).abs() < 1e-11, "{scale:?}");
        }
        assert_eq!(tdb_conv(51544.0, TimeScale::Tdb, false).unwrap(), 51544.0);
    }

    #[test]
    fn time_sys_to_ast_warns_as_c_does() {
        let mut w = Vec::new();
        assert_eq!(time_sys_to_ast(None, &mut w), TimeScale::Utc);
        assert_eq!(time_sys_to_ast(Some("IAT"), &mut w), TimeScale::Tai);
        assert_eq!(time_sys_to_ast(Some("TDT"), &mut w), TimeScale::Tt);
        assert!(w.is_empty());
        assert_eq!(time_sys_to_ast(Some("UT"), &mut w), TimeScale::Utc);
        assert_eq!(time_sys_to_ast(Some("ET"), &mut w), TimeScale::Tt);
        assert_eq!(time_sys_to_ast(Some("utc"), &mut w), TimeScale::Utc);
        assert_eq!(w.len(), 3);
        assert!(w.iter().all(|w| w.condition == "badval"));
    }
}
