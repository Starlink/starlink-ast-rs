//! Numeric rate-of-change computation via central finite differences.

use crate::constants::BAD_VALUE;
use crate::coords::PointSet;
use crate::error::AstResult;

use super::helpers::is_bad;
use super::Mapping;

pub fn numeric_rate(
    mapping: &dyn Mapping,
    at: &[f64],
    ax1: usize,
    ax2: usize,
) -> AstResult<f64> {
    let ax2_idx = ax2 - 1;
    let h = {
        let base = at[ax2_idx].abs();
        if base > 1e-10 { base * 1e-6 } else { 1e-10 }
    };

    let mut at_plus = at.to_vec();
    let mut at_minus = at.to_vec();
    at_plus[ax2_idx] += h;
    at_minus[ax2_idx] -= h;

    let pts_plus = PointSet::from_rows([at_plus.as_slice()]);
    let pts_minus = PointSet::from_rows([at_minus.as_slice()]);

    let out_plus = mapping.transform(&pts_plus, true)?;
    let out_minus = mapping.transform(&pts_minus, true)?;

    let val_plus = out_plus.point(0)[ax1 - 1];
    let val_minus = out_minus.point(0)[ax1 - 1];

    if is_bad(val_plus) || is_bad(val_minus) {
        Ok(BAD_VALUE)
    } else {
        Ok((val_plus - val_minus) / (2.0 * h))
    }
}
