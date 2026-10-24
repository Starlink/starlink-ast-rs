//! The pixel types the array functions accept.
//!
//! Each array function (resampling, rebinning, outlining, ...) is one generic function bounded by [`PixelValue`].
//! The trait's items say how a pixel type is summed, rounded and converted, and [`PixelValue::CODE`] names the type by a one- or two-letter code.
//! The accepted types are `f64`, `f32`, `i64`, `u64`, `i32`, `u32`, `i16`, `u16`, `i8` and `u8`.

// C generates each array function once per type from a macro (astOutlineF,
// astResampleD, ...); the items of PixelValue carry what that macro's
// arguments carry. C's `L` (`long`) and `K` (`INT_BIG`) are both 64-bit on the
// platforms the port supports, so both are `i64`, as `UL` and `UK` are both
// `u64`.

use std::ops::{Add, AddAssign, Div, Mul};

mod private {
    pub trait Sealed {}
    pub trait SealedSum {}
}

/// The floating type a pixel type is summed in while interpolating: `f32` or `f64`.
// The `Xfloattype` argument of the interpolation macros in mapping.c.
pub trait SumValue:
    Copy
    + PartialOrd
    + Add<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + AddAssign
    + private::SealedSum
{
    /// Zero.
    const ZERO: Self;
    /// One.
    const ONE: Self;
    /// One half, added to or subtracted from a value to round it.
    const HALF: Self;
    /// The double `v` converted to this type.
    // C's `(Xfloattype) v`.
    fn from_f64(v: f64) -> Self;
}

impl private::SealedSum for f32 {}
impl private::SealedSum for f64 {}

impl SumValue for f32 {
    const ZERO: Self = 0.0;
    const ONE: Self = 1.0;
    const HALF: Self = 0.5;
    fn from_f64(v: f64) -> Self {
        v as f32
    }
}

impl SumValue for f64 {
    const ZERO: Self = 0.0;
    const ONE: Self = 1.0;
    const HALF: Self = 0.5;
    fn from_f64(v: f64) -> Self {
        v
    }
}

/// A pixel type the array functions accept.
pub trait PixelValue: PartialOrd + Copy + Send + Sync + std::fmt::Debug + private::Sealed {
    /// The type's one- or two-letter code: `"D"` for `f64`, `"F"` for `f32`, `"L"` for `i64`, `"UL"` for `u64`, `"I"` for `i32`, `"UI"` for `u32`, `"S"` for `i16`, `"US"` for `u16`, `"B"` for `i8` and `"UB"` for `u8`.
    // The code C appends to the function name for this type, as in astOutlineF.
    const CODE: &'static str;
    /// Whether the type is floating point.
    // C's `Xfloating`.
    const FLOATING: bool;
    /// Whether the type is signed, so that a negative variance can occur and is bad.
    // C's `Xsigned`.
    const SIGNED: bool;
    /// The type values of this type are summed in: `f32` for `f32` and the integer types of 16 bits or fewer, `f64` otherwise.
    // C's `Xfloattype`.
    type Sum: SumValue;
    /// Zero, which a variance is tested against.
    const ZERO: Self;
    /// The sums at or below which an interpolated value overflows this type when rounded; zero, and unused, for a floating type.
    // C's `LO_<X>`.
    const LO_LIM: Self::Sum;
    /// The sums at or above which an interpolated value overflows this type when rounded; zero, and unused, for a floating type.
    // C's `HI_<X>`.
    const HI_LIM: Self::Sum;
    /// The value as a double.
    fn to_f64(self) -> f64;
    /// The value as its sum type.
    fn to_sum(self) -> Self::Sum;
    /// A sum converted back to this type as an interpolated value.
    ///
    /// A floating type keeps the value unchanged.
    /// An integer type rounds it half away from zero, by adding or subtracting a half and truncating, and saturates a value out of range.
    // C assigns an interpolated value this way; its cast is undefined for a
    // value out of range, where Rust's `as` saturates.
    fn from_sum(v: Self::Sum) -> Self;
    /// The double `v` in this type: truncated toward zero, and saturated when out of range, for an integer type.
    // C's `(Xtype) v`, whose cast is undefined for a value out of range.
    fn from_f64(v: f64) -> Self;
}

macro_rules! pixel_value {
    ($($t:ty => $code:literal, $floating:literal, $signed:literal, $sum:ty, $lo:expr, $hi:expr);* $(;)?) => {$(
        impl private::Sealed for $t {}
        impl PixelValue for $t {
            const CODE: &'static str = $code;
            const FLOATING: bool = $floating;
            const SIGNED: bool = $signed;
            type Sum = $sum;
            const ZERO: Self = 0 as $t;
            const LO_LIM: $sum = $lo;
            const HI_LIM: $sum = $hi;
            fn to_f64(self) -> f64 {
                self as f64
            }
            fn to_sum(self) -> $sum {
                self as $sum
            }
            fn from_sum(v: $sum) -> Self {
                if $floating {
                    v as $t
                } else {
                    let half = <$sum as SumValue>::HALF;
                    (v + if v >= <$sum as SumValue>::ZERO { half } else { -half }) as $t
                }
            }
            fn from_f64(v: f64) -> Self {
                v as $t
            }
        }
    )*};
}

pixel_value!(
    f64 => "D", true, true, f64, 0.0, 0.0;
    f32 => "F", true, true, f32, 0.0, 0.0;
    i64 => "L", false, true, f64, -0.5 + i64::MIN as f64, 0.5 + i64::MAX as f64;
    u64 => "UL", false, false, f64, -0.5, 0.5 + u64::MAX as f64;
    i32 => "I", false, true, f64, -0.5 + i32::MIN as f64, 0.5 + i32::MAX as f64;
    u32 => "UI", false, false, f64, -0.5, 0.5 + u32::MAX as f64;
    i16 => "S", false, true, f32, -0.5 + i16::MIN as f32, 0.5 + i16::MAX as f32;
    u16 => "US", false, false, f32, -0.5, 0.5 + u16::MAX as f32;
    i8 => "B", false, true, f32, -0.5 + i8::MIN as f32, 0.5 + i8::MAX as f32;
    u8 => "UB", false, false, f32, -0.5, 0.5 + u8::MAX as f32;
);

/// A pixel type the rebinning functions accept, with the arithmetic their spreading functions do in the type itself.
///
/// The rebinning functions accept `f64`, `f32`, `i32`, `i8` and `u8` only.
/// Integer addition in the type wraps on overflow, and conversion from a double saturates for `i32` and wraps for `i8` and `u8`.
// C's astRebin<X> and astRebinSeq<X> exist for D, F, I, B and UB only. C's
// integer arithmetic is undefined on overflow; the port wraps where C adds in
// the type and saturates where C converts a double, as Rust's casts do.
pub trait RebinValue: PixelValue {
    /// `v` rounded half away from zero, through `i32`, for an integer type, and `v` itself otherwise; a double in both cases.
    // C's `CONV( IntType, v )`.
    fn conv(v: f64) -> f64;
    /// `self + b` in this type, wrapping on overflow for an integer type.
    // C's `a += b` with both in this type.
    fn add(self, b: Self) -> Self;
    /// `self * self` in this type's arithmetic, as a double: `i8` and `u8` are widened to `i32` first, and `i32` wraps on overflow.
    // C's `c * c` after integer promotion.
    fn square(self) -> f64;
    /// `self - b * b` in this type, the product formed as in [`RebinValue::square`] and the result wrapped into the type.
    // C's `a -= b * b` with both in this type.
    fn sub_square(self, b: Self) -> Self;
    /// The double `v` assigned to this type.
    ///
    /// For `i8` and `u8` the value goes through a 32-bit integer, so a value out of range wraps; for `i32` it saturates.
    /// The spreading relies on the wrap: a kernel's negative lobes add wrapped values to an unsigned pixel that sum back into range.
    // This is how C's `(Xtype) v` compiles on the supported platforms (arm64
    // saturates for `int`); the C standard leaves an out-of-range value
    // undefined.
    fn from_c(v: f64) -> Self;
}

macro_rules! rebin_value {
    ($($t:ty => $int:literal, $add:expr, $sq:expr, $subsq:expr, $from:expr);* $(;)?) => {$(
        impl RebinValue for $t {
            fn conv(v: f64) -> f64 {
                if $int {
                    (v + if v > 0.0 { 0.5 } else { -0.5 }) as i32 as f64
                } else {
                    v
                }
            }
            fn add(self, b: Self) -> Self {
                let f: fn($t, $t) -> $t = $add;
                f(self, b)
            }
            fn square(self) -> f64 {
                let f: fn($t) -> f64 = $sq;
                f(self)
            }
            fn sub_square(self, b: Self) -> Self {
                let f: fn($t, $t) -> $t = $subsq;
                f(self, b)
            }
            fn from_c(v: f64) -> Self {
                let f: fn(f64) -> $t = $from;
                f(v)
            }
        }
    )*};
}

rebin_value!(
    f64 => false, |a, b| a + b, |c| c * c, |a, b| a - b * b, |v| v;
    f32 => false, |a, b| a + b, |c| f64::from(c * c), |a, b| a - b * b, |v| v as f32;
    i32 => true, |a, b| a.wrapping_add(b), |c| f64::from(c.wrapping_mul(c)),
        |a, b| a.wrapping_sub(b.wrapping_mul(b)), |v| v as i32;
    i8 => true, |a, b| a.wrapping_add(b), |c| f64::from(i32::from(c) * i32::from(c)),
        |a, b| (i32::from(a) - i32::from(b) * i32::from(b)) as i8, |v| v as i32 as i8;
    u8 => true, |a, b| a.wrapping_add(b), |c| f64::from(i32::from(c) * i32::from(c)),
        |a, b| (i32::from(a) - i32::from(b) * i32::from(b)) as u8, |v| v as i32 as u8;
);
