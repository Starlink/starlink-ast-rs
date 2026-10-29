// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later
//
// Derived from PAL: Copyright Science and Technology Facilities Council, Rutherford Appleton Laboratory, Tim Jenness and Patrick T. Wallace; GNU Lesser General Public License version 3 or later.
//
// Ported from PAL palDmat (pal/palDmat.c), PAL v0.9.10-9-g75017db as vendored at reference/starlink-ast 3d3f255f.

// The index loops mirror C's pointer walks over rows and columns.
#![allow(clippy::needless_range_loop)]

/// Inverts the `n` by `n` matrix `a` (row-major) in place and solves `a x = y`
/// for `y`, by Gauss-Jordan elimination with partial pivoting.
///
/// Returns whether the matrix is singular, and the determinant, which is zero
/// when it is. A singular matrix leaves `a` and `y` partially reduced, as
/// far as the elimination got.
pub(crate) fn dmat(n: usize, a: &mut [f64], y: &mut [f64]) -> (bool, f64) {
    const SFA: f64 = 1e-20;
    assert!(a.len() >= n * n && y.len() >= n);

    let mut iw = vec![0_usize; n];
    let mut singular = false;
    let mut d = 1.0_f64;
    for k in 0..n {
        let ko = k * n;
        let mut amx = a[ko + k].abs();
        let mut imx = k;
        for i in k + 1..n {
            let t = a[i * n + k].abs();
            if t > amx {
                amx = t;
                imx = i;
            }
        }
        if amx < SFA {
            singular = true;
        } else {
            if imx != k {
                for j in 0..n {
                    a.swap(ko + j, imx * n + j);
                }
                y.swap(k, imx);
                d = -d;
            }
            iw[k] = imx;
            d *= a[ko + k];
            if d.abs() < SFA {
                singular = true;
            } else {
                a[ko + k] = 1.0 / a[ko + k];
                for j in 0..n {
                    if j != k {
                        a[ko + j] *= a[ko + k];
                    }
                }
                let yk = y[k] * a[ko + k];
                y[k] = yk;
                for i in 0..n {
                    if i != k {
                        let io = i * n;
                        for j in 0..n {
                            if j != k {
                                a[io + j] -= a[io + k] * a[ko + j];
                            }
                        }
                        y[i] -= a[io + k] * yk;
                    }
                }
                for i in 0..n {
                    if i != k {
                        a[i * n + k] *= -a[ko + k];
                    }
                }
            }
        }
    }
    if singular {
        return (true, 0.0);
    }
    for k in (0..n).rev() {
        let ki = iw[k];
        if k != ki {
            for i in 0..n {
                a.swap(i * n + k, i * n + ki);
            }
        }
    }
    (false, d)
}
