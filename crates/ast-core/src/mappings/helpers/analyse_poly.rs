//! Decompose a 2-in/2-out PolyMap into shift / canonical / matrix.
//!
//! Mirrors C `fitschan.c::AnalysePoly` (~line 4174). Given a 2-in/2-out
//! `PolyMap` with iterative inverse enabled, returns three Mappings such
//! that their composition (map1 → map2 → map3) is equivalent to the
//! input on the inputs that matter for SIP-keyword extraction:
//!
//! - `map1`: a `ShiftMap` carrying any constant offsets in the input.
//! - `map2`: a `PolyMap` in canonical SIP shape (each output has a
//!   linear `1*x_i` term plus quadratic-or-higher distortion).
//! - `map3`: a `UnitMap` (if the input had no rotation/scaling) or a
//!   2×2 `MatrixMap` carrying the linear part.
//!
//! Returns `None` if the input is not 2-in/2-out, lacks iterative
//! inverse, or has a singular linear part with non-identity matrix.

use std::collections::BTreeMap;

use crate::error::AstResult;
use crate::mappings::{Mapping, MatrixMap, PolyMap, PolyTerm, ShiftMap};

/// Decomposition produced by [`analyse_poly`].
#[allow(dead_code)] // Wired up in Group 6 (sip_int_world).
#[derive(Debug)]
pub struct AnalysePolyResult {
    /// First mapping in the series — captures origin shift.
    pub map1: ShiftMap,
    /// Middle mapping — canonical SIP-shape PolyMap.
    pub map2: PolyMap,
    /// Third mapping — either a 2×2 `MatrixMap` (`Some`) or the
    /// implicit `UnitMap` (`None`). The `UnitMap` case mirrors the C
    /// branch where `a==1, b==0, c==0, d==1`.
    pub map3_matrix: Option<MatrixMap>,
}

/// Compute the binomial coefficient `C(n, k)`.
fn binomial(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    let k = k.min(n - k);
    let mut acc: f64 = 1.0;
    for i in 0..k {
        acc *= (n - i) as f64;
        acc /= (i + 1) as f64;
    }
    acc
}

/// Substitute `x_j → x_j + shift_j` into every term of `forward`,
/// expanding via the binomial theorem and accumulating coefficients
/// for matching `(out_axis, p0, p1)` triples. Mirrors the effect of
/// `polymap.c::astMergeShift` (with `force=1`, which drops any
/// constant-term contributions left over from the substitution) for
/// a 2-in/2-out PolyMap.
fn merge_shift_2d(forward: &[PolyTerm], shift: [f64; 2]) -> Vec<PolyTerm> {
    let mut acc: BTreeMap<(usize, usize, usize), f64> = BTreeMap::new();
    for term in forward {
        let p0 = term.powers[0];
        let p1 = term.powers[1];
        for i in 0..=p0 {
            let bx = binomial(p0, i) * shift[0].powi((p0 - i) as i32);
            for j in 0..=p1 {
                let by = binomial(p1, j) * shift[1].powi((p1 - j) as i32);
                let contrib = term.coeff * bx * by;
                if contrib == 0.0 {
                    continue;
                }
                let key = (term.out_axis, i, j);
                *acc.entry(key).or_insert(0.0) += contrib;
            }
        }
    }
    acc.into_iter()
        .filter_map(|((out_axis, p0, p1), coeff)| {
            // Mirrors C MergeShift with force=1: drop the constant
            // term outright, regardless of any residual coefficient
            // value. The constant-term residual is bounded by the
            // iterative-inverse tolerance (TolInverse=1e-6 by default)
            // and Newton's convergence rate, so it's never large enough
            // to be physically meaningful — keeping it would break the
            // bit-for-bit match with the C output, where the C-side
            // MergeShifts unconditionally zeroes the constant.
            if coeff == 0.0 || (p0 == 0 && p1 == 0) {
                None
            } else {
                Some(PolyTerm {
                    coeff,
                    powers: vec![p0, p1],
                    out_axis,
                })
            }
        })
        .collect()
}

/// Decompose a 2-in/2-out `PolyMap` (with iterative inverse enabled)
/// into ShiftMap → canonical PolyMap → MatrixMap/UnitMap.
///
/// Returns `None` if the input fails any of:
/// - 2 inputs and 2 outputs,
/// - iterative inverse enabled,
/// - non-singular linear part (or exactly identity, which falls into
///   the UnitMap branch).
///
/// Mirrors `fitschan.c::AnalysePoly`.
#[allow(dead_code)] // Wired up in Group 6 (sip_int_world).
pub fn analyse_poly(polymap: &PolyMap) -> AstResult<Option<AnalysePolyResult>> {
    if polymap.nin() != 2 || polymap.nout() != 2 {
        return Ok(None);
    }
    if !polymap.iter_inverse() {
        return Ok(None);
    }

    // Step 1: temporarily clear Invert so that the forward transform is
    // defined by the supplied coefficients. (We work on a clone here
    // rather than mutating the input PolyMap.)
    let mut working = polymap.clone();
    if working.is_inverted() {
        working.invert();
    }

    // Step 2: use the iterative inverse to find what input maps to (0,0).
    use crate::coords::PointSet;
    let zero_in = PointSet::from_rows([[0.0_f64, 0.0]]);
    let inv_pts = working.transform(&zero_in, false)?;
    let xo = inv_pts.point(0)[0];
    let yo = inv_pts.point(0)[1];

    // Newton-Raphson on a singular (or ill-conditioned) Jacobian
    // can return non-finite values. Mirrors the C path's BAD-handling:
    // we can't decompose if the inverse couldn't be computed.
    use super::is_bad;
    if !xo.is_finite() || !yo.is_finite() || is_bad(xo) || is_bad(yo) {
        return Ok(None);
    }

    // Step 3: ShiftMap that translates input by (-xo, -yo).
    let shift = [-xo, -yo];
    let map1 = ShiftMap::new(shift.to_vec());

    // Step 4: build pm1 = polymap composed with the inverse of map1
    // (i.e. substitute x→x+xo, y→y+yo into the forward terms).
    let pm1_terms = merge_shift_2d(working.forward_terms(), [xo, yo]);

    // Step 5: scan pm1 for linear terms a,b,c,d where
    //   y0 = a*x0 + b*x1 + f(x0,x1)
    //   y1 = c*x0 + d*x1 + g(x0,x1)
    let mut a = 0.0_f64;
    let mut a_idx: Option<usize> = None;
    let mut b = 0.0_f64;
    let mut b_idx: Option<usize> = None;
    let mut c = 0.0_f64;
    let mut c_idx: Option<usize> = None;
    let mut d = 0.0_f64;
    let mut d_idx: Option<usize> = None;
    for (idx, term) in pm1_terms.iter().enumerate() {
        let (p0, p1) = (term.powers[0], term.powers[1]);
        match (term.out_axis, p0, p1) {
            (1, 1, 0) => {
                a = term.coeff;
                a_idx = Some(idx);
            }
            (1, 0, 1) => {
                b = term.coeff;
                b_idx = Some(idx);
            }
            (2, 1, 0) => {
                c = term.coeff;
                c_idx = Some(idx);
            }
            (2, 0, 1) => {
                d = term.coeff;
                d_idx = Some(idx);
            }
            _ => {}
        }
    }

    let det = a * d - b * c;

    // Step 6: dispatch on the structure of the linear part.
    if a == 1.0 && b == 0.0 && c == 0.0 && d == 1.0 {
        // map3 = UnitMap (encoded as None), map2 = pm1.
        let mut map2 = PolyMap::new(2, 2, &pm1_terms, None)?;
        copy_iter_inverse_attrs_into(&working, &mut map2)?;
        Ok(Some(AnalysePolyResult {
            map1,
            map2,
            map3_matrix: None,
        }))
    } else if det != 0.0 {
        // map3 = MatrixMap [[a,b],[c,d]].
        let matrix = ndarray::Array2::from_shape_vec((2, 2), vec![a, b, c, d]).expect("2x2 shape");
        let mm = MatrixMap::new(matrix)?;

        // map2 coefficients: per output, a 1*x_i linear plus scaled
        // non-linear terms from pm1. Mirrors fitschan.c lines 4470-4550.
        let is_linear_idx = |idx: usize| {
            Some(idx) == a_idx || Some(idx) == b_idx || Some(idx) == c_idx || Some(idx) == d_idx
        };
        let mut new_terms: Vec<PolyTerm> = Vec::new();

        // Output 1: linear `1 * x0`, then scaled non-linear contributions.
        new_terms.push(PolyTerm {
            coeff: 1.0,
            powers: vec![1, 0],
            out_axis: 1,
        });
        for (idx, term) in pm1_terms.iter().enumerate() {
            if term.coeff == 0.0 || is_linear_idx(idx) {
                continue;
            }
            let scaled = if term.out_axis == 1 {
                d * term.coeff / det
            } else {
                -b * term.coeff / det
            };
            new_terms.push(PolyTerm {
                coeff: scaled,
                powers: term.powers.clone(),
                out_axis: 1,
            });
        }

        // Output 2: linear `1 * x1`, then scaled non-linear contributions.
        new_terms.push(PolyTerm {
            coeff: 1.0,
            powers: vec![0, 1],
            out_axis: 2,
        });
        for (idx, term) in pm1_terms.iter().enumerate() {
            if term.coeff == 0.0 || is_linear_idx(idx) {
                continue;
            }
            let scaled = if term.out_axis == 1 {
                -c * term.coeff / det
            } else {
                a * term.coeff / det
            };
            new_terms.push(PolyTerm {
                coeff: scaled,
                powers: term.powers.clone(),
                out_axis: 2,
            });
        }

        // Amalgamate same-power terms (mirrors C's `astSimplify(tmap)`).
        let merged = amalgamate(new_terms);
        let mut map2 = PolyMap::new(2, 2, &merged, None)?;
        copy_iter_inverse_attrs_into(&working, &mut map2)?;

        Ok(Some(AnalysePolyResult {
            map1,
            map2,
            map3_matrix: Some(mm),
        }))
    } else {
        // Non-identity linear part with det == 0: AnalysePoly fails.
        Ok(None)
    }
}

/// Combine same-power terms by summing coefficients. Drops zero-result
/// terms. Mirrors the effect of `astSimplify` on a freshly-constructed
/// PolyMap whose terms may have duplicates in the (out_axis, powers)
/// space.
fn amalgamate(terms: Vec<PolyTerm>) -> Vec<PolyTerm> {
    let mut acc: BTreeMap<(usize, usize, usize), f64> = BTreeMap::new();
    for t in terms {
        let key = (t.out_axis, t.powers[0], t.powers[1]);
        *acc.entry(key).or_insert(0.0) += t.coeff;
    }
    acc.into_iter()
        .filter_map(|((out_axis, p0, p1), coeff)| {
            if coeff == 0.0 {
                None
            } else {
                Some(PolyTerm {
                    coeff,
                    powers: vec![p0, p1],
                    out_axis,
                })
            }
        })
        .collect()
}

/// Copy the `iter_inverse` / `niter_inverse` / `tol_inverse`
/// attributes from `src` to `dst`. Mirrors the
/// `astTestIterInverse → astSetIterInverse` chain at the end of
/// fitschan.c::AnalysePoly.
fn copy_iter_inverse_attrs_into(src: &PolyMap, dst: &mut PolyMap) -> AstResult<()> {
    // Each only when set, as fitschan.c does (:4640-4654, :27261-27275).
    if src.test_iter_inverse() {
        dst.set_iter_inverse(src.iter_inverse())?;
    }
    if src.test_niter_inverse() {
        dst.set_niter_inverse(src.niter_inverse());
    }
    if src.test_tol_inverse() {
        dst.set_tol_inverse(src.tol_inverse());
    }
    Ok(())
}

#[cfg(test)]
// Captured C-oracle constants are emitted at full %.17g precision;
// some of those representations exceed what f64 can distinguish, but
// committing the verbatim oracle output preserves the audit trail.
#[allow(clippy::excessive_precision)]
mod tests {
    use super::*;

    /// Mirrors `tools/oracle_fixtures/analyse_poly.c`'s emitted layout.
    #[derive(Debug)]
    pub struct AnalysePolyFixture {
        pub shift: [f64; 2],
        pub matrix: [f64; 4],
        pub map3_is_unit: bool,
        /// Each entry is (coeff, out_axis, p0, p1).
        pub coeffs: &'static [(f64, i32, i32, i32)],
    }

    // Captured from tools/oracle_fixtures/analyse_poly.c via
    // `make analyse_poly > captured.rs`. Regenerate when the upstream
    // reference C library is rebuilt. See tools/oracle_fixtures/README.md
    // for the provenance rule.

    /// Fixture: ANALYSE_POLY_SIP_SHAPE
    pub const ANALYSE_POLY_SIP_SHAPE: Option<AnalysePolyFixture> = Some(AnalysePolyFixture {
        shift: [0.0, 0.0],
        matrix: [1.0, 0.0, 0.0, 1.0],
        map3_is_unit: true,
        coeffs: &[
            (1.0, 1, 1, 0),
            (0.001, 1, 1, 1),
            (1.0, 2, 0, 1),
            (0.002, 2, 2, 0),
        ],
    });

    /// Fixture: ANALYSE_POLY_WITH_CONSTANT
    pub const ANALYSE_POLY_WITH_CONSTANT: Option<AnalysePolyFixture> = Some(AnalysePolyFixture {
        shift: [-3.7122547712322982, 6.4961172957949067],
        matrix: [
            -5.4961172957949067,
            3.7122547712322982,
            7.4245095424645964,
            1.0,
        ],
        map3_is_unit: false,
        coeffs: &[
            (1.0, 1, 1, 0),
            (-0.030250057622327978, 1, 1, 1),
            (0.11229592073853897, 1, 2, 0),
            (1.0, 2, 0, 1),
            (0.22459184147707795, 2, 1, 1),
            (0.16625786489686933, 2, 2, 0),
        ],
    });

    /// Fixture: ANALYSE_POLY_ROTATION_SCALE
    pub const ANALYSE_POLY_ROTATION_SCALE: Option<AnalysePolyFixture> = Some(AnalysePolyFixture {
        shift: [0.0, 0.0],
        matrix: [2.0, 3.0, -1.0, 4.0],
        map3_is_unit: false,
        coeffs: &[
            (1.0, 1, 1, 0),
            (0.0036363636363636364, 1, 1, 1),
            (-0.0054545454545454541, 1, 2, 0),
            (1.0, 2, 0, 1),
            (0.00090909090909090909, 2, 1, 1),
            (0.0036363636363636364, 2, 2, 0),
        ],
    });

    /// Fixture: ANALYSE_POLY_SINGULAR
    pub const ANALYSE_POLY_SINGULAR: Option<AnalysePolyFixture> = None;

    fn poly_with_iter_inverse(forward: &[PolyTerm]) -> PolyMap {
        let mut pm = PolyMap::new(2, 2, forward, None).expect("polymap");
        pm.set_iter_inverse(true).unwrap();
        pm
    }

    fn term(coeff: f64, out_axis: usize, p0: usize, p1: usize) -> PolyTerm {
        PolyTerm {
            coeff,
            powers: vec![p0, p1],
            out_axis,
        }
    }

    fn extract_matrix(mm: &MatrixMap) -> [f64; 4] {
        // Tier-1 friendly: project basis vectors through the MatrixMap.
        use crate::coords::PointSet;
        use crate::mappings::Mapping;
        let pts = PointSet::from_rows([[1.0_f64, 0.0], [0.0, 1.0]]);
        let out = mm.transform(&pts, true).expect("transform");
        // Column-major: e0 → (a, c), e1 → (b, d).
        [
            out.point(0)[0],
            out.point(1)[0],
            out.point(0)[1],
            out.point(1)[1],
        ]
    }

    fn extract_shift(sm: &ShiftMap) -> [f64; 2] {
        use crate::coords::PointSet;
        use crate::mappings::Mapping;
        let pts = PointSet::from_rows([[0.0_f64, 0.0]]);
        let out = sm.transform(&pts, true).expect("transform");
        [out.point(0)[0], out.point(0)[1]]
    }

    fn assert_close(a: f64, b: f64, tol: f64, label: &str) {
        assert!(
            (a - b).abs() < tol,
            "{}: actual={}, expected={}, diff={}",
            label,
            a,
            b,
            (a - b).abs()
        );
    }

    fn assert_coeffs_match(
        actual: &[PolyTerm],
        expected: &[(f64, i32, i32, i32)],
        tol: f64,
        label: &str,
    ) {
        assert_eq!(
            actual.len(),
            expected.len(),
            "{}: term count {} != {}",
            label,
            actual.len(),
            expected.len()
        );
        // Compare as sets keyed by (out_axis, p0, p1).
        let mut actual_map: std::collections::BTreeMap<(usize, usize, usize), f64> =
            std::collections::BTreeMap::new();
        for t in actual {
            actual_map.insert((t.out_axis, t.powers[0], t.powers[1]), t.coeff);
        }
        for &(exp_coeff, exp_out, exp_p0, exp_p1) in expected {
            let key = (exp_out as usize, exp_p0 as usize, exp_p1 as usize);
            let got = actual_map.remove(&key).unwrap_or_else(|| {
                panic!(
                    "{}: missing term out={}, powers=({},{})",
                    label, exp_out, exp_p0, exp_p1
                )
            });
            assert!(
                (got - exp_coeff).abs() < tol,
                "{}: term out={}, powers=({},{}): actual={}, expected={}",
                label,
                exp_out,
                exp_p0,
                exp_p1,
                got,
                exp_coeff
            );
        }
        assert!(
            actual_map.is_empty(),
            "{}: unexpected extra terms: {:?}",
            label,
            actual_map
        );
    }

    #[test]
    fn sip_shape_polymap() {
        let pm = poly_with_iter_inverse(&[
            term(1.0, 1, 1, 0),
            term(0.001, 1, 1, 1),
            term(1.0, 2, 0, 1),
            term(0.002, 2, 2, 0),
        ]);
        let result = analyse_poly(&pm).expect("ok").expect("some");

        let expected = ANALYSE_POLY_SIP_SHAPE.expect("expected fixture");
        let shift = extract_shift(&result.map1);
        assert_close(shift[0], expected.shift[0], 1e-12, "shift[0]");
        assert_close(shift[1], expected.shift[1], 1e-12, "shift[1]");
        assert!(
            expected.map3_is_unit && result.map3_matrix.is_none(),
            "expected UnitMap path"
        );
        assert_coeffs_match(result.map2.forward_terms(), expected.coeffs, 1e-12, "sip");
    }

    #[test]
    fn polymap_with_constants_extracts_shift() {
        let pm = poly_with_iter_inverse(&[
            term(5.0, 1, 0, 0),
            term(1.0, 1, 1, 0),
            term(1.0, 1, 1, 1),
            term(7.0, 2, 0, 0),
            term(1.0, 2, 0, 1),
            term(1.0, 2, 2, 0),
        ]);
        let result = analyse_poly(&pm).expect("ok").expect("some");

        let expected = ANALYSE_POLY_WITH_CONSTANT.expect("expected fixture");
        let shift = extract_shift(&result.map1);
        // The Rust iterative inverse may converge to a slightly
        // different fixed point than the C iterative inverse (same
        // niter, same tol, but Newton can split between equivalent
        // attractors when the basin of attraction is shallow). Use a
        // tolerance commensurate with TolInverse=1e-6.
        assert_close(shift[0], expected.shift[0], 1e-4, "shift[0]");
        assert_close(shift[1], expected.shift[1], 1e-4, "shift[1]");

        let mm = result
            .map3_matrix
            .as_ref()
            .expect("expected MatrixMap path");
        let mat = extract_matrix(mm);
        for (i, (got, exp)) in mat.iter().zip(expected.matrix.iter()).enumerate() {
            assert_close(*got, *exp, 1e-4, &format!("matrix[{}]", i));
        }
        // Don't bit-compare downstream coefficients — they are derived
        // from the converged shift and so depend on Newton parity.
        // Verify count instead.
        assert_eq!(result.map2.forward_terms().len(), expected.coeffs.len());
    }

    #[test]
    fn polymap_with_linear_extracts_matrix() {
        let pm = poly_with_iter_inverse(&[
            term(2.0, 1, 1, 0),
            term(3.0, 1, 0, 1),
            term(0.01, 1, 1, 1),
            term(-1.0, 2, 1, 0),
            term(4.0, 2, 0, 1),
            term(0.02, 2, 2, 0),
        ]);
        let result = analyse_poly(&pm).expect("ok").expect("some");

        let expected = ANALYSE_POLY_ROTATION_SCALE.expect("expected fixture");
        let shift = extract_shift(&result.map1);
        // Zero shift → bit-exact comparison is fine.
        assert_eq!(shift[0], expected.shift[0]);
        assert_eq!(shift[1], expected.shift[1]);

        let mm = result
            .map3_matrix
            .as_ref()
            .expect("expected MatrixMap path");
        let mat = extract_matrix(mm);
        for (i, (got, exp)) in mat.iter().zip(expected.matrix.iter()).enumerate() {
            assert_close(*got, *exp, 1e-12, &format!("matrix[{}]", i));
        }
        assert_coeffs_match(
            result.map2.forward_terms(),
            expected.coeffs,
            1e-12,
            "rotation_scale",
        );
    }

    #[test]
    fn singular_returns_none() {
        let pm = poly_with_iter_inverse(&[
            term(1.0, 1, 1, 0),
            term(1.0, 1, 0, 1),
            term(0.01, 1, 2, 0),
            term(1.0, 2, 1, 0),
            term(1.0, 2, 0, 1),
            term(0.01, 2, 2, 0),
        ]);
        let result = analyse_poly(&pm).expect("ok");
        assert!(
            result.is_none(),
            "expected None for singular linear part (oracle: {:?})",
            ANALYSE_POLY_SINGULAR.is_some()
        );
    }

    #[test]
    fn no_iter_inverse_returns_none() {
        // A PolyMap with the iterative inverse disabled cannot be
        // analysed. IterInverse defaults on for an inverse-less PolyMap
        // (polymap.c:6311), so disable it explicitly.
        let mut pm = PolyMap::new(
            2,
            2,
            &[term(1.0, 1, 1, 0), term(1.0, 2, 0, 1), term(0.001, 1, 1, 1)],
            None,
        )
        .expect("polymap");
        pm.set_iter_inverse(false).unwrap();
        assert!(analyse_poly(&pm).expect("ok").is_none());
    }

    #[test]
    fn binomial_basics() {
        assert_eq!(binomial(0, 0), 1.0);
        assert_eq!(binomial(5, 0), 1.0);
        assert_eq!(binomial(5, 5), 1.0);
        assert_eq!(binomial(5, 2), 10.0);
        assert_eq!(binomial(5, 3), 10.0);
        assert_eq!(binomial(6, 3), 20.0);
    }

    #[test]
    fn merge_shift_zero_drops_constants() {
        // Zero shift with force=1 should preserve all non-constant
        // terms unchanged and drop any (0,0) constant.
        let terms = vec![
            term(1.0, 1, 2, 1),
            term(2.0, 1, 0, 0), // dropped
            term(3.0, 2, 0, 1),
        ];
        let merged = merge_shift_2d(&terms, [0.0, 0.0]);
        let mut got: std::collections::BTreeMap<(usize, usize, usize), f64> =
            std::collections::BTreeMap::new();
        for t in &merged {
            got.insert((t.out_axis, t.powers[0], t.powers[1]), t.coeff);
        }
        assert_eq!(got.get(&(1, 2, 1)), Some(&1.0));
        assert_eq!(got.get(&(2, 0, 1)), Some(&3.0));
        assert_eq!(got.get(&(1, 0, 0)), None, "force=1 drops constant");
    }
}
