//! The floating-point library functions `ast-core` calls, routed through one
//! place so the `call-counts` feature can count them by their C names. The
//! bare `f64` methods are banned elsewhere in this crate by `clippy.toml`.
//!
//! Each function is the `f64` method it wraps, so results are bit-identical.
#![allow(clippy::disallowed_methods)]
// One wrapper per method `clippy.toml` bans, whether or not the library
// calls it today, so a new call site always has its counted form.
#![allow(dead_code)]

use crate::call_counts::hit;

#[inline(always)]
pub(crate) fn sin(x: f64) -> f64 {
    hit("sin");
    x.sin()
}

#[inline(always)]
pub(crate) fn cos(x: f64) -> f64 {
    hit("cos");
    x.cos()
}

#[inline(always)]
pub(crate) fn tan(x: f64) -> f64 {
    hit("tan");
    x.tan()
}

#[inline(always)]
pub(crate) fn asin(x: f64) -> f64 {
    hit("asin");
    x.asin()
}

#[inline(always)]
pub(crate) fn acos(x: f64) -> f64 {
    hit("acos");
    x.acos()
}

#[inline(always)]
pub(crate) fn atan(x: f64) -> f64 {
    hit("atan");
    x.atan()
}

#[inline(always)]
pub(crate) fn atan2(y: f64, x: f64) -> f64 {
    hit("atan2");
    y.atan2(x)
}

#[inline(always)]
pub(crate) fn exp(x: f64) -> f64 {
    hit("exp");
    x.exp()
}

#[inline(always)]
pub(crate) fn ln(x: f64) -> f64 {
    hit("log");
    x.ln()
}

#[inline(always)]
pub(crate) fn log10(x: f64) -> f64 {
    hit("log10");
    x.log10()
}

#[inline(always)]
pub(crate) fn powf(x: f64, y: f64) -> f64 {
    hit("pow");
    x.powf(y)
}

#[inline(always)]
pub(crate) fn powi(x: f64, n: i32) -> f64 {
    hit("powi");
    x.powi(n)
}

#[inline(always)]
pub(crate) fn sqrt(x: f64) -> f64 {
    hit("sqrt");
    x.sqrt()
}

/// The square root of `x`, counted as a call only when `counted`: for a
/// branchless kernel that takes a root C skips and then discards it.
#[inline(always)]
pub(crate) fn sqrt_counted_if(x: f64, counted: bool) -> f64 {
    if counted {
        hit("sqrt");
    }
    x.sqrt()
}

#[inline(always)]
pub(crate) fn hypot(x: f64, y: f64) -> f64 {
    hit("hypot");
    x.hypot(y)
}

#[inline(always)]
pub(crate) fn sinh(x: f64) -> f64 {
    hit("sinh");
    x.sinh()
}

#[inline(always)]
pub(crate) fn cosh(x: f64) -> f64 {
    hit("cosh");
    x.cosh()
}

#[inline(always)]
pub(crate) fn tanh(x: f64) -> f64 {
    hit("tanh");
    x.tanh()
}

/// Both `sin` and `cos`, counted as one call of each as C makes them.
#[inline(always)]
pub(crate) fn sin_cos(x: f64) -> (f64, f64) {
    hit("sin");
    hit("cos");
    x.sin_cos()
}

#[cfg(all(test, feature = "call-counts"))]
mod tests {
    #[test]
    fn counts_by_c_name() {
        crate::call_counts::reset();
        let _ = super::sin(1.0) + super::ln(2.0) + super::powf(2.0, 0.5);
        let _ = super::sin_cos(0.5);
        let c = crate::call_counts::snapshot();
        assert_eq!(c.get("sin"), Some(&2));
        assert_eq!(c.get("cos"), Some(&1));
        assert_eq!(c.get("log"), Some(&1));
        assert_eq!(c.get("pow"), Some(&1));
    }
}
