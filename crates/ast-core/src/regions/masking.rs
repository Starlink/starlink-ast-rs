//! Generic masking of data arrays using Regions.
//!
//! The [`mask`] function iterates over pixel positions in an N-dimensional
//! grid, transforms them into the region's coordinate frame via a supplied
//! mapping, tests each position against the region predicate, and sets
//! masked pixels to a specified value.

use crate::coords::PointSet;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::mappings::Mapping;
use crate::regions::Region;

/// Apply a region as a mask to a data array.
///
/// Iterates over pixel positions in the grid defined by `lbnd..=ubnd`,
/// transforms each position through `mapping` into the region's frame,
/// tests against the region predicate, and sets masked pixels to `val`.
///
/// Returns the number of pixels that were masked.
///
/// # Arguments
///
/// - `region`: the Region defining the mask shape.
/// - `mapping`: transforms grid (pixel) coordinates into the region's
///   coordinate frame.
/// - `inside`: if `true`, mask pixels **inside** the region; if `false`,
///   mask pixels **outside** the region.
/// - `lbnd`: lower bounds of the grid (one per axis, inclusive).
/// - `ubnd`: upper bounds of the grid (one per axis, inclusive).
/// - `data`: flattened data array in row-major order (axis 1 varies
///   fastest, like C AST's convention). Length must equal the product
///   of the grid dimensions.
/// - `val`: value to assign to masked pixels.
///
/// # Errors
///
/// Returns an error if:
/// - `lbnd` and `ubnd` have different lengths.
/// - The number of grid axes does not match `mapping.nin()`.
/// - Any `ubnd[i] < lbnd[i]`.
/// - The data array length does not match the grid volume.
pub fn mask<T: Copy>(
    region: &dyn Region,
    mapping: &dyn Mapping,
    inside: bool,
    lbnd: &[i64],
    ubnd: &[i64],
    data: &mut [T],
    val: T,
) -> AstResult<usize> {
    let ndim = lbnd.len();

    // Validate inputs.
    if ubnd.len() != ndim {
        return Err(AstError::new(
            ErrorCode::Dimin,
            format!(
                "mask: lbnd has {} elements but ubnd has {}",
                ndim,
                ubnd.len()
            ),
        ));
    }

    if ndim != mapping.nin() {
        return Err(AstError::new(
            ErrorCode::Dimin,
            format!(
                "mask: grid has {} axes but mapping.nin() = {}",
                ndim,
                mapping.nin()
            ),
        ));
    }

    // Compute per-axis dimensions and total volume.
    let mut dims = Vec::with_capacity(ndim);
    for i in 0..ndim {
        if ubnd[i] < lbnd[i] {
            return Err(AstError::new(
                ErrorCode::Dimin,
                format!("mask: ubnd[{i}] ({}) < lbnd[{i}] ({})", ubnd[i], lbnd[i]),
            ));
        }
        dims.push((ubnd[i] - lbnd[i] + 1) as usize);
    }

    let total: usize = dims.iter().product();
    if data.len() != total {
        return Err(AstError::new(
            ErrorCode::Dimin,
            format!(
                "mask: data length {} does not match grid volume {}",
                data.len(),
                total
            ),
        ));
    }

    if total == 0 {
        return Ok(0);
    }

    // Row length is the extent along axis 0 (first axis varies fastest).
    let row_len = dims[0];

    // Number of rows is the product of remaining dimensions.
    let nrows: usize = if ndim > 1 {
        dims[1..].iter().product()
    } else {
        1
    };

    // Strides for converting a multi-index (on axes 1..) into a row number.
    // row_index = sum_j( (idx[j] - lbnd[j+1]) * stride[j] ) for j in 0..ndim-1
    let mut strides = Vec::with_capacity(ndim.saturating_sub(1));
    if ndim > 1 {
        let mut s = 1usize;
        for d in &dims[1..] {
            strides.push(s);
            s *= d;
        }
    }

    let mut masked_count = 0usize;

    // Iterate over rows. For each row, build pixel coordinates for all
    // points along axis 0, transform them, then test against the region.
    let mut row_indices = vec![0usize; ndim.saturating_sub(1)];

    for row_idx in 0..nrows {
        // Decode row_idx into per-axis indices for axes 1..ndim.
        if ndim > 1 {
            let mut rem = row_idx;
            for j in (0..ndim - 1).rev() {
                row_indices[j] = rem / strides[j];
                rem %= strides[j];
            }
        }

        // Build PointSet for this row: one point per pixel along axis 0.
        let mut pts = PointSet::zeros(row_len, ndim);
        {
            let mut view = pts.view_mut();
            for px in 0..row_len {
                // Axis 0 (varies fastest).
                view[[px, 0]] = (lbnd[0] + px as i64) as f64;
                // Remaining axes: fixed for this row.
                for j in 0..ndim - 1 {
                    view[[px, j + 1]] = (lbnd[j + 1] + row_indices[j] as i64) as f64;
                }
            }
        }

        // Transform pixel coordinates to region frame.
        let transformed = mapping.transform(&pts, true)?;

        // Test each transformed point against the region.
        let data_offset = row_idx * row_len;
        for px in 0..row_len {
            let point = transformed.point(px);
            let is_inside = region.point_in_region(point)?;

            let should_mask = if inside { is_inside } else { !is_inside };
            if should_mask {
                data[data_offset + px] = val;
                masked_count += 1;
            }
        }
    }

    Ok(masked_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::BasicFrameBuilder;
    use crate::mappings::UnitMap;
    use crate::regions::CircleBuilder;

    #[test]
    fn mask_inside_circle_unit_mapping() {
        // 5x5 grid with a circle of radius 2 centered at (3,3).
        // Grid coords: (1..=5, 1..=5). Centre at (3,3), radius 2.
        let frame = BasicFrameBuilder::new(2).build().unwrap();
        let circle = CircleBuilder::new(frame, vec![3.0, 3.0])
            .unwrap()
            .radius(2.0)
            .build()
            .unwrap();
        let mapping = UnitMap::new(2);

        let mut data = vec![0.0f64; 25];
        let count = mask(&circle, &mapping, true, &[1, 1], &[5, 5], &mut data, -1.0).unwrap();

        // Count pixels inside: distance from (3,3) <= 2
        // The pixels at distance <= 2 from center (3,3):
        // (3,3)=0, (2,3)=1, (4,3)=1, (3,2)=1, (3,4)=1,
        // (2,2)=sqrt(2), (4,2)=sqrt(2), (2,4)=sqrt(2), (4,4)=sqrt(2),
        // (1,3)=2, (5,3)=2, (3,1)=2, (3,5)=2
        // That's 13 pixels.
        assert_eq!(count, 13);

        // Verify a known inside pixel: (3,3) -> data index
        // Row-major with axis 0 fastest: pixel (x, y) at offset (y-1)*5 + (x-1)
        let idx_3_3 = (3 - 1) * 5 + (3 - 1); // = 12
        assert_eq!(data[idx_3_3], -1.0);

        // Verify a known outside pixel: (1,1) distance = sqrt(8) > 2
        let idx_1_1 = 0;
        assert_eq!(data[idx_1_1], 0.0);
    }

    #[test]
    fn mask_outside_circle() {
        let frame = BasicFrameBuilder::new(2).build().unwrap();
        let circle = CircleBuilder::new(frame, vec![3.0, 3.0])
            .unwrap()
            .radius(2.0)
            .build()
            .unwrap();
        let mapping = UnitMap::new(2);

        let mut data = vec![0.0f64; 25];
        let count = mask(&circle, &mapping, false, &[1, 1], &[5, 5], &mut data, -1.0).unwrap();

        // 25 total - 13 inside = 12 outside
        assert_eq!(count, 12);

        // Inside pixel should be untouched.
        let idx_3_3 = (3 - 1) * 5 + (3 - 1);
        assert_eq!(data[idx_3_3], 0.0);

        // Outside pixel should be masked.
        let idx_1_1 = 0;
        assert_eq!(data[idx_1_1], -1.0);
    }
}
