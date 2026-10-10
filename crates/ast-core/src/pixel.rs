//! The pixel types of the array functions: one per C type suffix.
//!
//! C generates each array function once per type from a macro (`astOutlineF`,
//! `astResampleD`, ...). The port has one generic function bounded by
//! [`PixelValue`], whose items carry what the macro arguments carry in C.
//! C's `L` (`long`) and `K` (`INT_BIG`) are both 64-bit on the platforms the
//! port supports, so both are `i64`, as `UL` and `UK` are both `u64`.

use std::ops::{Add, AddAssign, Div, Mul};

mod private {
    pub trait Sealed {}
    pub trait SealedSum {}
}

/// The floating type C sums a pixel type in (the `Xfloattype` argument of
/// mapping.c's interpolation macros): `f32` or `f64`.
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
    /// One half, which C adds to or subtracts from a value it rounds.
    const HALF: Self;
    /// The double `v` in this type, as C's `(Xfloattype) v`.
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

/// A pixel type the array functions accept: one of the types C generates
/// them for.
pub trait PixelValue: PartialOrd + Copy + Send + Sync + std::fmt::Debug + private::Sealed {
    /// The code C appends to the function name for this type, as in
    /// `astOutlineF`.
    const CODE: &'static str;
    /// Whether the type is floating point (C's `Xfloating`).
    const FLOATING: bool;
    /// Whether the type is signed (C's `Xsigned`), so that a negative
    /// variance can occur and is bad.
    const SIGNED: bool;
    /// The type C sums values of this type in (C's `Xfloattype`): `f32` for
    /// `float` and the types of 16 bits or fewer, `f64` otherwise.
    type Sum: SumValue;
    /// Zero, which a variance is tested against.
    const ZERO: Self;
    /// The sums at or below which an interpolated value overflows this type
    /// when rounded (C's `LO_<X>`); zero, and unused, for a floating type.
    const LO_LIM: Self::Sum;
    /// The sums at or above which an interpolated value overflows this type
    /// when rounded (C's `HI_<X>`); zero, and unused, for a floating type.
    const HI_LIM: Self::Sum;
    /// The value as a double, as C's `(double) value`.
    fn to_f64(self) -> f64;
    /// The value as its sum type, as C's `(Xfloattype) value`.
    fn to_sum(self) -> Self::Sum;
    /// A sum converted back to this type as C assigns an interpolated value:
    /// unchanged for a floating type, and for an integer type rounded half
    /// away from zero by adding or subtracting a half and truncating. C's
    /// cast is undefined for a value out of range; Rust's saturates.
    fn from_sum(v: Self::Sum) -> Self;
    /// The double `v` in this type, as C's `(Xtype) v`: truncated toward
    /// zero for an integer type. C's cast is undefined for a value out of
    /// range; Rust's saturates.
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
