//! Peak memory of bulk transforms, counted by a global allocator per thread.
//!
//! A series CmpMap holds the result of its first component while the second
//! runs, so a chain of nested series would hold one copy of the points per
//! level; cmpmap.c `Transform` passes the points through 8192 at a time to
//! bound that, and the port does the same. Building a PointSet from rows
//! must not need more than the PointSet itself either. Both matter for
//! callers transforming many millions of points, such as
//! `Moc::add_pixel_mask` on a large image.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use starlink_ast::coords::PointSet;
use starlink_ast::mappings::{CmpMap, Composition, DynMapping, ZoomMap};

/// The system allocator, counting the bytes each thread has allocated and
/// not freed, and the most it has had at once.
struct Counting;

thread_local! {
    static CURRENT: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

fn record(delta: isize) {
    let _ = CURRENT.try_with(|c| {
        let now = c.get() + delta;
        c.set(now);
        let _ = PEAK.try_with(|p| p.set(p.get().max(now)));
    });
}

// SAFETY: every call is forwarded unchanged to the system allocator; the
// counters are thread-local cells with no destructors.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size() as isize);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        record(-(layout.size() as isize));
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record(new_size as isize - layout.size() as isize);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The most bytes `f` has allocated at once beyond what was allocated when
/// it started, on this thread.
fn peak_during<R>(f: impl FnOnce() -> R) -> (R, usize) {
    let base = CURRENT.with(Cell::get);
    PEAK.with(|p| p.set(base));
    let r = f();
    (r, (PEAK.with(Cell::get) - base) as usize)
}

const NPOINT: usize = 200_000;

fn points() -> PointSet {
    let mut pts = PointSet::zeros(NPOINT, 2);
    for i in 0..NPOINT {
        pts.point_mut(i).copy_from_slice(&[i as f64, -(i as f64)]);
    }
    pts
}

/// Eight ZoomMaps in series, nested as `z, (z, (z, ...))` or the other way.
fn chain(right: bool) -> DynMapping {
    let zoom = || -> DynMapping { Box::new(ZoomMap::new(2, 2.0).unwrap()) };
    let mut map = zoom();
    for _ in 1..8 {
        map = Box::new(if right {
            CmpMap::new(zoom(), map, Composition::Series).unwrap()
        } else {
            CmpMap::new(map, zoom(), Composition::Series).unwrap()
        });
    }
    map
}

/// The bytes of a 2-D PointSet of `NPOINT` points.
const OUTPUT: usize = NPOINT * 2 * 8;

/// What a chain of eight series may hold beyond its result: a batch of 8192
/// 2-D points in and out of each level, whatever the number of points. Full
/// size intermediates would be seven times `OUTPUT`.
const BATCHES: usize = 2 << 20;

#[test]
fn nested_series_holds_no_full_size_intermediates() {
    let pts = points();
    for right in [true, false] {
        let map = chain(right);
        for forward in [true, false] {
            let (out, peak) = peak_during(|| map.transform(&pts, forward).unwrap());
            assert_eq!(out.point(1)[0], if forward { 256.0 } else { 1.0 / 256.0 });
            assert!(
                peak < OUTPUT + BATCHES,
                "nested {} forward={forward}: peak {peak} bytes for a {OUTPUT}-byte result",
                if right { "right" } else { "left" }
            );
        }
    }
}

#[test]
fn nested_series_transform_into_holds_no_full_size_intermediates() {
    let pts = points();
    let map = chain(true);
    let mut out = PointSet::zeros(NPOINT, 2);
    let ((), peak) = peak_during(|| {
        map.transform_into(pts.as_points(), true, out.as_points_mut())
            .unwrap()
    });
    assert_eq!(out.point(1)[0], 256.0);
    assert!(peak < BATCHES, "peak {peak} bytes");
}

#[test]
fn from_rows_needs_only_the_point_set() {
    let rows: Vec<[f64; 2]> = (0..NPOINT).map(|i| [i as f64, 0.0]).collect();
    let (pts, peak) = peak_during(|| PointSet::from_rows(rows.iter().map(|r| r.as_slice())));
    assert_eq!(pts.npoint(), NPOINT);
    assert!(
        peak <= OUTPUT,
        "peak {peak} bytes for a {OUTPUT}-byte PointSet"
    );
}

#[test]
fn nested_series_transform_into_nan_holds_no_full_size_intermediates() {
    let pts = points();
    let map = chain(true);
    let mut out = PointSet::zeros(NPOINT, 2);
    let ((), peak) = peak_during(|| {
        map.transform_into_nan(pts.as_points(), true, out.as_points_mut())
            .unwrap()
    });
    assert_eq!(out.point(1)[0], 256.0);
    assert!(peak < BATCHES, "peak {peak} bytes");
}

/// Splitting a writable view into two, as a parallel CmpMap does on every
/// call, allocates nothing for the axis counts held inline.
#[test]
fn splitting_a_view_allocates_nothing() {
    let mut buf = vec![0.0; 4 * 16];
    let (_, peak) = peak_during(|| {
        let out = starlink_ast::coords::PointsMut::from_strided(&mut buf, 4, 16, 16).unwrap();
        let (left, right) = out.split_axes_at(1);
        (left.ncoord(), right.ncoord())
    });
    assert_eq!(peak, 0);
}
