//! Rebuild a `PolyMap` with each input axis pre-scaled.
//!
//! Mirrors C `fitschan.c::ScalePolyInputs` (~26899). Returns a new
//! `PolyMap` whose forward transformation reproduces the supplied
//! `polymap` after multiplying each input axis `j` by `scales[j]`.
//! For each forward term `c * x0^p0 * x1^p1 * ... * xN^pN`, the new
//! coefficient becomes `c * Π scales[j]^pj`.
//!
//! Inverse coefficients (when an explicit inverse is present) get
//! divided by `scales[out_axis - 1]`. Note: in the SIP usage path the
//! supplied PolyMap always uses `iter_inverse=1`, so the
//! inverse-coefficients branch is never exercised. The C reference
//! uses a stale pointer in this branch (logged in
//! `docs/issues/c-library-quirks.md`); the Rust port indexes
//! `scales` correctly per the apparent intent.

use crate::error::AstResult;
use crate::mappings::{Mapping, PolyMap, PolyTerm};

/// Rebuild `polymap` with input axes pre-scaled by `scales[j]`.
///
/// Returns `None` if any scale is zero (mirrors the C check at the
/// top of `ScalePolyInputs`). Returns an error if the rebuilt PolyMap
/// fails validation.
#[allow(dead_code)] // Wired up in Group 6 (sip_int_world).
pub fn scale_poly_inputs(polymap: &PolyMap, scales: &[f64]) -> AstResult<Option<PolyMap>> {
    let nin = polymap.nin();
    let nout = polymap.nout();
    if scales.len() != nin {
        return Ok(None);
    }
    for &s in scales {
        if s == 0.0 || !s.is_finite() {
            return Ok(None);
        }
    }

    // Forward terms: each coefficient gets multiplied by Π scales[j]^pj.
    let scaled_forward: Vec<PolyTerm> = polymap
        .forward_terms()
        .iter()
        .map(|t| {
            let mut coeff = t.coeff;
            for (j, &p) in t.powers.iter().enumerate() {
                coeff *= scales[j].powi(p as i32);
            }
            PolyTerm {
                coeff,
                powers: t.powers.clone(),
                out_axis: t.out_axis,
            }
        })
        .collect();

    // Inverse terms (if any): each coefficient gets divided by
    // scales[out_axis - 1] of the inverse, which is the original
    // forward input. (Mirrors C lines 27013-27028 with the apparent
    // intent — the C source itself uses a stale pointer in this
    // branch, so the on-paper algorithm is followed in Rust.)
    let scaled_inverse: Option<Vec<PolyTerm>> = polymap.inverse_terms().map(|terms| {
        terms
            .iter()
            .map(|t| {
                // The inverse polymap has nout inputs (matching original
                // forward outputs) and outputs the original forward
                // inputs. So out_axis here is the original forward input
                // axis index (1-based).
                let iin = (t.out_axis - 1) % nin;
                let mut coeff = t.coeff;
                coeff /= scales[iin];
                PolyTerm {
                    coeff,
                    powers: t.powers.clone(),
                    out_axis: t.out_axis,
                }
            })
            .collect()
    });

    // Build the new PolyMap. Mirrors C's "if Invert was set, build
    // uninverted then invert" handling.
    let was_inverted = polymap.is_inverted();
    let mut result = if was_inverted {
        // Construct as (nout → nin) using inverse-as-forward and
        // forward-as-inverse, then flip.
        let inv_as_fwd = scaled_inverse.unwrap_or_default();
        let fwd_as_inv = scaled_forward.clone();
        PolyMap::new(nout, nin, &inv_as_fwd, Some(&fwd_as_inv))?
    } else {
        PolyMap::new(nin, nout, &scaled_forward, scaled_inverse.as_deref())?
    };

    if was_inverted {
        result.invert();
    }

    // Copy iter_inverse / niter_inverse / tol_inverse attributes.
    // Each only when set, as fitschan.c does (:4640-4654, :27261-27275).
    if polymap.test_iter_inverse() {
        result.set_iter_inverse(polymap.iter_inverse())?;
    }
    if polymap.test_niter_inverse() {
        result.set_niter_inverse(polymap.niter_inverse());
    }
    if polymap.test_tol_inverse() {
        result.set_tol_inverse(polymap.tol_inverse());
    }

    Ok(Some(result))
}

#[cfg(test)]
// Captured C-oracle constants are emitted at full %.17g precision; some
// representations exceed f64's distinguishable range, but committing the
// verbatim oracle output preserves the audit trail.
#[allow(clippy::excessive_precision)]
mod tests {
    use super::*;

    // Captured from tools/oracle_fixtures/scale_poly_inputs.c via
    // `make scale_poly_inputs > captured.rs`. Regenerate when the
    // upstream reference C library is rebuilt.

    /// Fixture: SCALE_POLY_INPUTS_SIP_UNIFORM
    /// scales = [2.0, 2.0]
    pub const SCALE_POLY_INPUTS_SIP_UNIFORM: Option<&[(f64, i32, i32, i32)]> = Some(&[
        (2.0, 1, 1, 0),
        (0.0040000000000000001, 1, 1, 1),
        (2.0, 2, 0, 1),
        (0.0080000000000000002, 2, 2, 0),
    ]);

    /// Fixture: SCALE_POLY_INPUTS_SIP_NONUNIFORM
    /// scales = [3.0, 5.0]
    pub const SCALE_POLY_INPUTS_SIP_NONUNIFORM: Option<&[(f64, i32, i32, i32)]> = Some(&[
        (3.0, 1, 1, 0),
        (0.014999999999999999, 1, 1, 1),
        (5.0, 2, 0, 1),
        (0.018000000000000002, 2, 2, 0),
    ]);

    /// Fixture: SCALE_POLY_INPUTS_3D_MIXED
    /// scales = [2.0, 3.0, 5.0]
    pub const SCALE_POLY_INPUTS_3D_MIXED: Option<&[(f64, i32, i32, i32)]> =
        Some(&[(4.0, 1, 1, 0), (135.0, 1, 0, 2), (210.0, 1, 1, 1)]);

    /// Fixture: SCALE_POLY_INPUTS_ZERO_SCALE — oracle returned NULL
    pub const SCALE_POLY_INPUTS_ZERO_SCALE: Option<&[(f64, i32, i32, i32)]> = None;

    fn term(coeff: f64, out_axis: usize, powers: &[usize]) -> PolyTerm {
        PolyTerm {
            coeff,
            powers: powers.to_vec(),
            out_axis,
        }
    }

    fn assert_terms_match(actual: &[PolyTerm], expected: &[(f64, i32, i32, i32)], label: &str) {
        // Compare as multisets keyed by (out_axis, powers).
        let mut actual_sorted: Vec<(usize, Vec<usize>, f64)> = actual
            .iter()
            .map(|t| (t.out_axis, t.powers.clone(), t.coeff))
            .collect();
        actual_sorted.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let mut expected_sorted: Vec<(usize, Vec<usize>, f64)> = expected
            .iter()
            .map(|&(coeff, out, p0, p1)| (out as usize, vec![p0 as usize, p1 as usize], coeff))
            .collect();
        expected_sorted.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

        assert_eq!(
            actual_sorted.len(),
            expected_sorted.len(),
            "{}: term count {} != {}",
            label,
            actual_sorted.len(),
            expected_sorted.len()
        );
        for (i, (a, e)) in actual_sorted.iter().zip(expected_sorted.iter()).enumerate() {
            assert_eq!(a.0, e.0, "{}[{}]: out_axis", label, i);
            assert_eq!(a.1, e.1, "{}[{}]: powers", label, i);
            assert!(
                (a.2 - e.2).abs() < 1e-12,
                "{}[{}]: coeff actual={} expected={}",
                label,
                i,
                a.2,
                e.2
            );
        }
    }

    #[test]
    fn sip_uniform_scaling() {
        // y0 = x0 + 0.001*x0*x1
        // y1 = x1 + 0.002*x0^2
        let pm = PolyMap::new(
            2,
            2,
            &[
                term(1.0, 1, &[1, 0]),
                term(0.001, 1, &[1, 1]),
                term(1.0, 2, &[0, 1]),
                term(0.002, 2, &[2, 0]),
            ],
            None,
        )
        .expect("polymap");
        let result = scale_poly_inputs(&pm, &[2.0, 2.0])
            .expect("ok")
            .expect("some");
        let expected = SCALE_POLY_INPUTS_SIP_UNIFORM.expect("oracle some");
        assert_terms_match(result.forward_terms(), expected, "sip_uniform");
    }

    #[test]
    fn sip_nonuniform_scaling() {
        let pm = PolyMap::new(
            2,
            2,
            &[
                term(1.0, 1, &[1, 0]),
                term(0.001, 1, &[1, 1]),
                term(1.0, 2, &[0, 1]),
                term(0.002, 2, &[2, 0]),
            ],
            None,
        )
        .expect("polymap");
        let result = scale_poly_inputs(&pm, &[3.0, 5.0])
            .expect("ok")
            .expect("some");
        let expected = SCALE_POLY_INPUTS_SIP_NONUNIFORM.expect("oracle some");
        assert_terms_match(result.forward_terms(), expected, "sip_nonuniform");
    }

    #[test]
    fn three_d_mixed() {
        let pm = PolyMap::new(
            3,
            1,
            &[
                term(2.0, 1, &[1, 0, 0]),
                term(3.0, 1, &[0, 2, 1]),
                term(7.0, 1, &[1, 1, 1]),
            ],
            None,
        )
        .expect("polymap");
        let result = scale_poly_inputs(&pm, &[2.0, 3.0, 5.0])
            .expect("ok")
            .expect("some");
        // Expected fixture stores (coeff, out, p0, p1) tuples — extend
        // with p2 by inferring from input geometry. We compare values
        // directly against captured per-term outputs.
        let actual = result.forward_terms();
        // Sort actual terms by powers for stable comparison.
        let mut sorted: Vec<&PolyTerm> = actual.iter().collect();
        sorted.sort_by(|a, b| a.powers.cmp(&b.powers));
        // Captured fixture (powers prefix only): sort by (p0, p1).
        // Verify each captured tuple matches a corresponding sorted term.
        let expected = SCALE_POLY_INPUTS_3D_MIXED.expect("oracle some");
        assert_eq!(sorted.len(), expected.len());
        // For the 3D case the captured fixture only carries (p0, p1) in
        // its tuple shape (it's pinned by the oracle's emission code to
        // 4 fields). We cross-check by computing expected coefficients
        // analytically for each input term.
        let _ = expected;
        // Direct expected values (analytical):
        //   term (2, 1, [1,0,0]) * scales[0]^1 = 2 * 2 = 4
        //   term (3, 1, [0,2,1]) * scales[1]^2 * scales[2]^1 = 3*9*5 = 135
        //   term (7, 1, [1,1,1]) * 2*3*5 = 210
        let expected_terms = [
            (4.0_f64, vec![1usize, 0, 0]),
            (210.0_f64, vec![1usize, 1, 1]),
            (135.0_f64, vec![0usize, 2, 1]),
        ];
        for (exp_coeff, exp_powers) in &expected_terms {
            let found = actual
                .iter()
                .find(|t| t.powers == *exp_powers)
                .unwrap_or_else(|| panic!("no term with powers {:?}", exp_powers));
            assert!(
                (found.coeff - exp_coeff).abs() < 1e-12,
                "powers={:?}: actual={}, expected={}",
                exp_powers,
                found.coeff,
                exp_coeff
            );
        }
    }

    #[test]
    fn zero_scale_returns_none() {
        let pm = PolyMap::new(2, 2, &[term(1.0, 1, &[1, 0]), term(1.0, 2, &[0, 1])], None)
            .expect("polymap");
        assert_eq!(SCALE_POLY_INPUTS_ZERO_SCALE, None);
        let result = scale_poly_inputs(&pm, &[0.0, 1.0]).expect("ok");
        assert!(result.is_none(), "zero scale should refuse");
    }

    #[test]
    fn wrong_length_returns_none() {
        let pm = PolyMap::new(2, 2, &[term(1.0, 1, &[1, 0]), term(1.0, 2, &[0, 1])], None)
            .expect("polymap");
        // 1 scale for a 2-input PolyMap.
        assert!(scale_poly_inputs(&pm, &[2.0]).expect("ok").is_none());
    }

    #[test]
    fn iter_inverse_attrs_propagated() {
        let mut pm = PolyMap::new(
            2,
            2,
            &[
                term(1.0, 1, &[1, 0]),
                term(0.001, 1, &[1, 1]),
                term(1.0, 2, &[0, 1]),
                term(0.002, 2, &[2, 0]),
            ],
            None,
        )
        .expect("polymap");
        pm.set_iter_inverse(true).unwrap();
        pm.set_niter_inverse(7);
        pm.set_tol_inverse(1.0e-9);
        let result = scale_poly_inputs(&pm, &[2.0, 2.0])
            .expect("ok")
            .expect("some");
        assert!(result.iter_inverse());
        assert_eq!(result.niter_inverse(), 7);
        assert_eq!(result.tol_inverse(), 1.0e-9);
    }
}
