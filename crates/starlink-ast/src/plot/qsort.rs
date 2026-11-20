// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The C library's `qsort`, for orderings that C leaves to it.
//!
//! `qsort` is not stable: elements that compare equal end in an order that
//! depends on the implementation. Where the C library sorts with ties and
//! then uses the order, the port sorts as the C library on the oracle
//! platform (macOS, whose `qsort` is FreeBSD's) does, so that it draws
//! what that C library draws.
// C: plot.c Fpoly, which sorts polylines by the x coordinate of their ends
// with qsort, and then takes the first match a binary search finds among
// equal values. The algorithm is Apple Libc's stdlib/FreeBSD/qsort.c
// (_qsort, _isort, med3) and heapsort.c: Bentley and McIlroy's "Engineering
// a Sort Function" with insertion sort for 7 elements or fewer, a bounded
// insertion sort when a partition needs no swaps, and heapsort past a
// recursion depth of twice log2 of the length. Checked against the system
// qsort on 3000 arrays of random keys with many ties.

use std::cmp::Ordering;

/// The median of three elements, as FreeBSD's `med3` chooses it.
fn med3<T>(
    a: &[T],
    i: usize,
    j: usize,
    k: usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) -> usize {
    if cmp(&a[i], &a[j]) == Ordering::Less {
        if cmp(&a[j], &a[k]) == Ordering::Less {
            j
        } else if cmp(&a[i], &a[k]) == Ordering::Less {
            k
        } else {
            i
        }
    } else if cmp(&a[j], &a[k]) == Ordering::Greater {
        j
    } else if cmp(&a[i], &a[k]) == Ordering::Less {
        i
    } else {
        k
    }
}

/// Swap the `n` elements starting at `i` with those starting at `j`.
fn vecswap<T>(a: &mut [T], i: usize, j: usize, n: usize) {
    for k in 0..n {
        a.swap(i + k, j + k);
    }
}

/// Insertion sort, giving up (and returning false) after more than
/// `limit` swaps when `limit` is not zero.
// C: _isort.
fn isort<T>(a: &mut [T], cmp: &mut impl FnMut(&T, &T) -> Ordering, limit: usize) -> bool {
    let mut count = 0;
    for m in 1..a.len() {
        let mut l = m;
        while l > 0 && cmp(&a[l - 1], &a[l]) == Ordering::Greater {
            a.swap(l, l - 1);
            l -= 1;
            count += 1;
            if limit != 0 && count > limit {
                return false;
            }
        }
    }
    true
}

/// Sort `a` as the C library's `qsort` on macOS sorts it with the
/// comparison `cmp`, ties included.
pub(crate) fn qsort<T: Copy>(a: &mut [T], cmp: &mut impl FnMut(&T, &T) -> Ordering) {
    // C: DEPTH(n), twice the index of n's highest set bit.
    let depth = 2 * (usize::BITS - 1 - a.len().max(1).leading_zeros()) as i32;
    sort(a, cmp, depth);
}

// C: _qsort.
fn sort<T: Copy>(mut a: &mut [T], cmp: &mut impl FnMut(&T, &T) -> Ordering, mut depth_limit: i32) {
    loop {
        let n = a.len();
        let limit_hit = depth_limit <= 0;
        depth_limit -= 1;
        if limit_hit {
            heapsort(a, cmp);
            return;
        }
        if n <= 7 {
            isort(a, cmp, 0);
            return;
        }
        let mut pl = 0;
        let mut pm = n / 2;
        let mut pn = n - 1;
        if n > 40 {
            let d = n / 8;
            pl = med3(a, pl, pl + d, pl + 2 * d, cmp);
            pm = med3(a, pm - d, pm, pm + d, cmp);
            pn = med3(a, pn - 2 * d, pn - d, pn, cmp);
        }
        pm = med3(a, pl, pm, pn, cmp);
        a.swap(0, pm);
        let mut swapped = false;
        let (mut pa, mut pb) = (1usize, 1usize);
        // pc and pd may step below pb, so they are kept one above C's
        // pointers to stay unsigned.
        let (mut pc1, mut pd1) = (n, n);
        loop {
            while pb < pc1 {
                let r = cmp(&a[pb], &a[0]);
                if r == Ordering::Greater {
                    break;
                }
                if r == Ordering::Equal {
                    swapped = true;
                    a.swap(pa, pb);
                    pa += 1;
                }
                pb += 1;
            }
            while pb < pc1 {
                let r = cmp(&a[pc1 - 1], &a[0]);
                if r == Ordering::Less {
                    break;
                }
                if r == Ordering::Equal {
                    swapped = true;
                    a.swap(pc1 - 1, pd1 - 1);
                    pd1 -= 1;
                }
                pc1 -= 1;
            }
            if pb >= pc1 {
                break;
            }
            a.swap(pb, pc1 - 1);
            swapped = true;
            pb += 1;
            pc1 -= 1;
        }
        let (pc, pd) = (pc1 as isize - 1, pd1 as isize - 1);
        let d1 = pa.min(pb - pa);
        vecswap(a, 0, pb - d1, d1);
        let d1 = ((pd - pc) as usize).min(n - pd as usize - 1);
        vecswap(a, pb, n - d1, d1);
        if !swapped && isort(a, cmp, 1 + n / 4) {
            return;
        }
        let d1 = pb - pa;
        let d2 = (pd - pc) as usize;
        if d1 <= d2 {
            if d1 > 1 {
                sort(&mut a[..d1], cmp, depth_limit);
            }
            if d2 > 1 {
                a = &mut a[n - d2..];
                continue;
            }
        } else {
            if d2 > 1 {
                sort(&mut a[n - d2..], cmp, depth_limit);
            }
            if d1 > 1 {
                a = &mut a[..d1];
                continue;
            }
        }
        return;
    }
}

/// Heapsort, which `qsort` falls back to past its recursion limit.
// C: FreeBSD's heapsort.c, its CREATE and SELECT macros, on a heap indexed
// from 1.
fn heapsort<T: Copy>(a: &mut [T], cmp: &mut impl FnMut(&T, &T) -> Ordering) {
    let mut nmemb = a.len();
    if nmemb <= 1 {
        return;
    }
    // Element i of the heap, counting from 1.
    let at = |i: usize| i - 1;
    for l in (1..=nmemb / 2).rev() {
        let mut par_i = l;
        while par_i * 2 <= nmemb {
            let mut child_i = par_i * 2;
            if child_i < nmemb && cmp(&a[at(child_i)], &a[at(child_i + 1)]) == Ordering::Less {
                child_i += 1;
            }
            if cmp(&a[at(child_i)], &a[at(par_i)]) != Ordering::Greater {
                break;
            }
            a.swap(at(par_i), at(child_i));
            par_i = child_i;
        }
    }
    while nmemb > 1 {
        let k = a[at(nmemb)];
        a[at(nmemb)] = a[at(1)];
        nmemb -= 1;
        let mut par_i = 1;
        while par_i * 2 <= nmemb {
            let mut child_i = par_i * 2;
            if child_i < nmemb && cmp(&a[at(child_i)], &a[at(child_i + 1)]) == Ordering::Less {
                child_i += 1;
            }
            a[at(par_i)] = a[at(child_i)];
            par_i = child_i;
        }
        loop {
            let child_i = par_i;
            par_i = child_i / 2;
            if child_i == 1 || cmp(&k, &a[at(par_i)]) == Ordering::Less {
                a[at(child_i)] = k;
                break;
            }
            a[at(child_i)] = a[at(par_i)];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_as_a_total_order_would() {
        let mut v: Vec<i32> = (0..200).map(|i| (i * 7919) % 101).collect();
        qsort(&mut v, &mut |a: &i32, b: &i32| a.cmp(b));
        assert!(v.windows(2).all(|w| w[0] <= w[1]));
    }
}
