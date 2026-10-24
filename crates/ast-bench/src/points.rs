//! Choose oracle rows for a timing run of N points.

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;

use crate::oracle::Section;

/// The largest N that times only fully non-BAD rows.
pub const SMALL_N_MAX: usize = 100;

/// The point counts every case is timed at.
pub const NPOINTS: [usize; 5] = [1, 10, 100, 100_000, 1_000_000];

/// Row indices for an N-point run, repeating the candidate rows in order.
///
/// Up to `SMALL_N_MAX` points, only rows that are non-BAD in every input and
/// output are candidates: a short run made only of BAD points would time the
/// early exit, not the transform. Above it, every row is a candidate.
pub fn select_rows(s: &Section, n: usize) -> Result<Vec<usize>, String> {
    let all = 0..s.npoint();
    let candidates: Vec<usize> = if n <= SMALL_N_MAX {
        all.filter(|&r| {
            s.inputs
                .iter()
                .chain(&s.outputs)
                .all(|col| col[r] != BAD_VALUE)
        })
        .collect()
    } else {
        all.collect()
    };
    if candidates.is_empty() {
        return Err(format!(
            "{} dir={}: no non-BAD row to time at N={n}",
            s.relpath,
            if s.forward { "forward" } else { "inverse" }
        ));
    }
    Ok((0..n).map(|i| candidates[i % candidates.len()]).collect())
}

/// The selected rows' inputs as a `PointSet`, one array per axis.
pub fn rows_to_pointset(s: &Section, rows: &[usize]) -> PointSet {
    PointSet::from_rows(
        rows.iter()
            .map(|&r| s.inputs.iter().map(|c| c[r]).collect::<Vec<f64>>()),
    )
}

/// The selected rows' inputs one axis after another, C's `astTranN` layout.
pub fn rows_to_columns(s: &Section, rows: &[usize]) -> Vec<f64> {
    s.inputs
        .iter()
        .flat_map(|c| rows.iter().map(move |&r| c[r]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::Section;
    use starlink_ast::constants::BAD_VALUE as B;

    fn sec(inputs: Vec<Vec<f64>>, outputs: Vec<Vec<f64>>) -> Section {
        Section {
            relpath: "t.map".into(),
            nin: inputs.len(),
            nout: outputs.len(),
            forward: true,
            inputs,
            outputs,
        }
    }

    #[test]
    fn small_n_uses_only_rows_good_on_both_sides() {
        // Row 0 bad input, row 1 bad output, row 2 good.
        let s = sec(vec![vec![B, 1.0, 2.0]], vec![vec![0.0, B, 4.0]]);
        assert_eq!(select_rows(&s, 1).unwrap(), vec![2]);
        assert_eq!(select_rows(&s, 3).unwrap(), vec![2, 2, 2]);
    }

    #[test]
    fn small_n_with_no_good_row_is_an_error() {
        let s = sec(vec![vec![B, 1.0]], vec![vec![0.0, B]]);
        let e = select_rows(&s, 10).unwrap_err();
        assert!(e.contains("t.map") && e.contains("non-BAD"), "{e}");
    }

    #[test]
    fn large_n_uses_every_row_in_order_bad_included() {
        let s = sec(vec![vec![B, 1.0, 2.0]], vec![vec![0.0, B, 4.0]]);
        assert_eq!(
            select_rows(&s, 100_000).unwrap()[..7],
            [0, 1, 2, 0, 1, 2, 0]
        );
    }

    #[test]
    fn fewer_rows_than_n_repeat_in_order() {
        let col: Vec<f64> = (0..24).map(f64::from).collect();
        let s = sec(vec![col.clone()], vec![col]);
        assert_eq!(select_rows(&s, 10).unwrap(), (0..10).collect::<Vec<_>>());
        let r = select_rows(&s, 100).unwrap();
        assert_eq!((r[23], r[24], r[99]), (23, 0, 3));
    }

    #[test]
    fn column_layout_matches_c() {
        let s = sec(vec![vec![1.0, 2.0], vec![10.0, 20.0]], vec![vec![0.0, 0.0]]);
        assert_eq!(rows_to_columns(&s, &[1, 0]), vec![2.0, 1.0, 20.0, 10.0]);
        let ps = rows_to_pointset(&s, &[1, 0]);
        assert_eq!(ps.point(0), &[2.0, 20.0]);
    }
}
