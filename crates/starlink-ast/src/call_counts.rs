// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Per-thread counts of calls into libm, PAL, ERFA and wcslib, keyed by the name of the called function.
//!
//! The counts show how many calls into each numerical library a computation makes, so two builds can be compared call for call.
//!
//! Without the `call-counts` feature nothing is counted, and the counting hook compiles to nothing.
//!
//! To find where a counted call comes from, set `AST_COUNT_BACKTRACE` to its name as the counts report it (for example `pow`): the first such call on each thread after a `reset` prints a backtrace to stderr.
//! Set `AST_COUNT_TRACE_ALL` to a name instead to print, for every such call after a `reset`, one line naming the `starlink_ast` functions on the stack, which `sort | uniq -c` turns into a count per call path.

// The names are the symbols the C library calls (the vendored PAL and ERFA
// routines carry their `astPal` / `astEra` prefixes), so the counts line up
// with those from an instrumented build of the C library.

#[cfg(feature = "call-counts")]
mod imp {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    thread_local! {
        static COUNTS: RefCell<BTreeMap<&'static str, u64>> = const { RefCell::new(BTreeMap::new()) };
        /// Whether the traced name has been called since the last `reset`;
        /// true until the first `reset`, so calls made before counting
        /// starts are not traced.
        static TRACED: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    }

    /// The name `AST_COUNT_BACKTRACE` asks to trace, read once.
    static TRACE: std::sync::LazyLock<Option<String>> =
        std::sync::LazyLock::new(|| std::env::var("AST_COUNT_BACKTRACE").ok());

    /// The name `AST_COUNT_TRACE_ALL` asks to trace at every call, read once.
    static TRACE_ALL: std::sync::LazyLock<Option<String>> =
        std::sync::LazyLock::new(|| std::env::var("AST_COUNT_TRACE_ALL").ok());

    /// The `starlink_ast` functions on the current stack, innermost first,
    /// leaving out this module and `math`.
    fn call_path() -> String {
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        bt.lines()
            .filter_map(|l| {
                let l = l.trim();
                let f = l.split_once(": ")?.1;
                (f.contains("starlink_ast::")
                    && !f.contains("call_counts")
                    && !f.contains("::math::"))
                .then(|| f.replace("starlink_ast::", ""))
            })
            .collect::<Vec<_>>()
            .join(" < ")
    }

    pub(crate) fn hit(name: &'static str) {
        COUNTS.with(|c| *c.borrow_mut().entry(name).or_default() += 1);
        if TRACE_ALL.as_deref() == Some(name) && !TRACED.with(|t| t.get()) {
            eprintln!("{name}: {}", call_path());
        }
        if TRACE.as_deref() == Some(name) && !TRACED.with(|t| t.replace(true)) {
            eprintln!(
                "first call to {name}:\n{}",
                std::backtrace::Backtrace::force_capture()
            );
        }
    }

    /// `n` calls to `name` at once, as a slice entry point makes one per
    /// value; traced as one call.
    pub(crate) fn hit_n(name: &'static str, n: u64) {
        if n > 0 {
            hit(name);
            COUNTS.with(|c| *c.borrow_mut().entry(name).or_default() += n - 1);
        }
    }

    /// Zero this thread's counts.
    pub fn reset() {
        COUNTS.with(|c| c.borrow_mut().clear());
        TRACED.with(|t| t.set(false));
    }

    /// This thread's counts since the last `reset`, by the name of the called function.
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
pub(crate) use imp::{hit, hit_n};
#[cfg(feature = "call-counts")]
pub use imp::{reset, snapshot};

#[cfg(not(feature = "call-counts"))]
#[inline(always)]
pub(crate) fn hit(_name: &'static str) {}

#[cfg(not(feature = "call-counts"))]
#[inline(always)]
pub(crate) fn hit_n(_name: &'static str, _n: u64) {}

/// Call a PAL, ERFA or wcslib function, counting it under its symbol name for the
/// `call-counts` feature.
// No wrapper calls into C through this macro at present.
#[allow(unused_macros)]
macro_rules! counted_call {
    ($f:ident ( $($arg:expr),* $(,)? )) => {{
        crate::call_counts::hit(stringify!($f));
        // SAFETY: the arguments are the ones the calling wrapper documents
        // for this C function: valid pointers to buffers of the sizes it
        // writes, and plain values otherwise.
        unsafe { $f($($arg),*) }
    }};
}
#[allow(unused_imports)]
pub(crate) use counted_call as counted;

#[cfg(all(test, feature = "call-counts"))]
mod tests {
    use super::*;

    #[test]
    fn hit_n_counts_n_calls() {
        reset();
        hit_n("test_hit_n", 5);
        hit_n("test_hit_n", 0);
        hit("test_hit_n");
        assert_eq!(snapshot().get("test_hit_n"), Some(&6));
    }
}
