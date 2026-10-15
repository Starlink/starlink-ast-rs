//! Per-thread counts of calls into libm, PAL, ERFA and wcslib, keyed by the C
//! function name, so the port's calls can be compared with C's.
//!
//! Without the `call-counts` feature `hit` is an empty inline function and
//! nothing is counted.

#[cfg(feature = "call-counts")]
mod imp {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    thread_local! {
        static COUNTS: RefCell<BTreeMap<&'static str, u64>> = const { RefCell::new(BTreeMap::new()) };
    }

    pub(crate) fn hit(name: &'static str) {
        COUNTS.with(|c| *c.borrow_mut().entry(name).or_default() += 1);
    }

    /// Zero this thread's counts.
    pub fn reset() {
        COUNTS.with(|c| c.borrow_mut().clear());
    }

    /// This thread's counts since the last `reset`, by C function name.
    pub fn snapshot() -> BTreeMap<String, u64> {
        COUNTS.with(|c| {
            c.borrow()
                .iter()
                .map(|(k, v)| ((*k).to_string(), *v))
                .collect()
        })
    }
}

#[cfg(feature = "call-counts")]
pub(crate) use imp::hit;
#[cfg(feature = "call-counts")]
pub use imp::{reset, snapshot};

#[cfg(not(feature = "call-counts"))]
#[inline(always)]
pub(crate) fn hit(_name: &'static str) {}

/// Call a PAL, ERFA or wcslib function, counting it under its symbol name for the
/// `call-counts` feature.
macro_rules! counted_call {
    ($f:ident ( $($arg:expr),* $(,)? )) => {{
        crate::call_counts::hit(stringify!($f));
        // SAFETY: the arguments are the ones the calling wrapper documents
        // for this C function: valid pointers to buffers of the sizes it
        // writes, and plain values otherwise.
        unsafe { $f($($arg),*) }
    }};
}
pub(crate) use counted_call as counted;
