//! Port of `fitschan.c::LogAxis` (~20508).
//!
//! Tests whether a specified output of `map` is logarithmically
//! related to the corresponding pixel input — i.e. matches the
//! `S = Sr · exp(w/Sr)` form of FITS-WCS paper III's `-LOG`
//! algorithm. Returns the WCS→IWC mapping `w = Sr · log(S/Sr)`
//! for the chosen axis (identity on the rest) when the test passes,
//! or `None` otherwise.
//!
//! Caller-side use (in `other_axes`): if `is_map_linear` says the
//! axis is linear, build a ShiftMap; else if `log_axis` returns
//! `Some(map)`, append `-LOG` to the CTYPE and use that mapping;
//! else fall through to `-TAB` or the always-linear fallback.

#![allow(dead_code)]

use crate::error::AstResult;
use crate::mappings::{CmpMap, Composition, DynMapping, Mapping, MathMap, UnitMap, ZoomMap};

use super::is_map_linear::is_map_linear;

/// Test whether output `iax` of `map` is logarithmic in its
/// corresponding pixel input over the box `[lbnd_p, ubnd_p]`.
/// Returns the WCS→IWC mapping `w = Sr · log(S/Sr)` (with identity
/// on the other `nwcs - 1` axes) when the test passes; `None`
/// otherwise.
///
/// `crval` is the reference value `Sr`; the function returns `None`
/// if it is zero (matches C:20588).
///
/// Mirrors `fitschan.c::LogAxis`.
pub(crate) fn log_axis(
    map: &dyn Mapping,
    iax: usize,
    nwcs: usize,
    lbnd_p: &[f64],
    ubnd_p: &[f64],
    crval: f64,
) -> AstResult<Option<DynMapping>> {
    if !crval.is_finite() || crval == 0.0 {
        return Ok(None);
    }
    if iax >= nwcs {
        return Ok(None);
    }

    // Step 1: build a 1-D MathMap that does logs=log(s) /
    // s=exp(logs). Mirrors C:20594-20597.
    let log_1d = MathMap::new(1, 1, &["logs=log(s)"], &["s=exp(logs)"])?;

    // Step 2: embed it at axis `iax` of an `nwcs`-axis Mapping.
    // Mirrors C:20598 (AddUnitMaps).
    let log_padded: DynMapping = embed_axis(Box::new(log_1d.clone()), iax, nwcs)?;

    // Step 3: compose `map` (pixel→WCS) with `log_padded`
    // (WCS→log-of-WCS); the result goes pixel→log(WCS) on axis iax,
    // identity elsewhere. If this composition is linear in pixel at
    // output iax, then the original pixel→WCS mapping is logarithmic
    // there. Mirrors C:20599-20603.
    let composition: DynMapping = Box::new(CmpMap::new(
        map.clone_box(),
        log_padded,
        Composition::Series,
    )?);

    if !is_map_linear(composition.as_ref(), lbnd_p, ubnd_p, iax)? {
        return Ok(None);
    }

    // Step 4: build the WCS→IWC mapping. The 1-D form is:
    //   w = Sr · log(S/Sr)
    //     = ZoomMap(Sr) · MathMap(log) · ZoomMap(1/Sr)
    // Embed at axis `iax`. Mirrors C:20605-20615.
    let zoom_in = ZoomMap::new(1, 1.0 / crval)?;
    let zoom_out = ZoomMap::new(1, crval)?;
    let chain_1d: DynMapping = Box::new(CmpMap::new(
        Box::new(CmpMap::new(
            Box::new(zoom_in),
            Box::new(log_1d),
            Composition::Series,
        )?),
        Box::new(zoom_out),
        Composition::Series,
    )?);
    Ok(Some(embed_axis(chain_1d, iax, nwcs)?))
}

/// Embed `map` (1-axis-in / 1-axis-out) at index `iax` of an
/// `nax`-axis Mapping by parallel-composing UnitMaps before/after.
/// Mirrors `fitschan.c::AddUnitMaps`.
fn embed_axis(map: DynMapping, iax: usize, nax: usize) -> AstResult<DynMapping> {
    let nmap = map.nin();
    let mut ret = map;
    if iax > 0 {
        ret = Box::new(CmpMap::new(
            Box::new(UnitMap::new(iax)),
            ret,
            Composition::Parallel,
        )?);
    }
    if iax + nmap < nax {
        ret = Box::new(CmpMap::new(
            ret,
            Box::new(UnitMap::new(nax - iax - nmap)),
            Composition::Parallel,
        )?);
    }
    Ok(ret)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::PointSet;
    use crate::mappings::{ShiftMap, UnitMap};

    /// Build a 1-D mapping pixel→S of the form `S = Sr·exp(slope·p)`
    /// using a MathMap. The expected log_axis result on this mapping
    /// is `w = Sr·slope·p` (a ZoomMap, after simplification).
    fn build_log_chain_2d(sr: f64, slope: f64) -> Box<dyn Mapping> {
        // 2-D: identity on axis 0, exp on axis 1.
        // forward: s0 = x1, s1 = sr * exp(slope * x2)
        // inverse: x1 = s0, x2 = log(s1/sr)/slope
        let fwd_x_axis = "s0=x1".to_string();
        let fwd_y_axis = format!("s1={sr}*exp({slope}*x2)");
        let inv_x_axis = "x1=s0".to_string();
        let inv_y_axis = format!("x2=log(s1/{sr})/{slope}");
        let mm = MathMap::new(
            2,
            2,
            &[&fwd_x_axis, &fwd_y_axis],
            &[&inv_x_axis, &inv_y_axis],
        )
        .expect("mathmap");
        Box::new(mm)
    }

    #[test]
    fn linear_axis_is_not_log() {
        // A pure linear mapping shouldn't be detected as log.
        let m = ShiftMap::new(vec![10.0, 20.0]);
        let r = log_axis(&m, 0, 2, &[1.0, 1.0], &[100.0, 100.0], 5.0).unwrap();
        assert!(r.is_none(), "linear ShiftMap should not be log");
    }

    #[test]
    fn zero_crval_returns_none() {
        let m = UnitMap::new(2);
        let r = log_axis(&m, 0, 2, &[1.0, 1.0], &[100.0, 100.0], 0.0).unwrap();
        assert!(r.is_none());
    }

    #[test]
    fn out_of_range_axis_returns_none() {
        let m = UnitMap::new(2);
        let r = log_axis(&m, 5, 2, &[1.0, 1.0], &[100.0, 100.0], 5.0).unwrap();
        assert!(r.is_none());
    }

    #[test]
    fn detects_log_axis_and_returns_inverse_mapping() {
        // Construct pixel → WCS where axis 1 is logarithmic with
        // Sr=2.0, slope=0.1: S = 2 * exp(0.1 * p).
        let sr = 2.0;
        let slope = 0.1;
        let m = build_log_chain_2d(sr, slope);
        let r = log_axis(m.as_ref(), 1, 2, &[1.0, 1.0], &[10.0, 10.0], sr).unwrap();
        let mapping = r.expect("expected log_axis to detect");
        assert_eq!(mapping.nin(), 2);
        assert_eq!(mapping.nout(), 2);

        // Verify the mapping does S → Sr*log(S/Sr) on axis 1, identity
        // on axis 0. At pixel p=10 on the original chain:
        //   S = sr * exp(slope * 10) = 2 * exp(1) ≈ 5.4366
        // Forward through the WCS→IWC mapping (built by log_axis):
        //   w = sr * log(S/sr) = sr * slope * p = 2 * 0.1 * 10 = 2.0
        let s_val = sr * (slope * 10.0_f64).exp();
        let pts = PointSet::from_rows([[3.0_f64, s_val]]);
        let out = mapping.transform(&pts, true).unwrap();
        let p = out.point(0);
        assert!((p[0] - 3.0).abs() < 1e-9, "axis 0 identity, got {}", p[0]);
        // Within tolerance for the log fit.
        assert!(
            (p[1] - 2.0).abs() < 1e-6,
            "axis 1 log: expected 2.0, got {}",
            p[1]
        );
    }
}
