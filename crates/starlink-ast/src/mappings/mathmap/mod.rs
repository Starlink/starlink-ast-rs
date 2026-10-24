//! MathMap: transform coordinates using mathematical expressions.
//!
//! See [`MathMap`].

mod evaluator;
pub(crate) mod opcode;
mod parser;
pub(crate) mod rng;
mod token;

use std::cell::UnsafeCell;

use crate::constants::BAD_VALUE;
use crate::coords::{PointSet, Points, PointsMut};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::serial::{Node, Serializable, read_mapping_base, write_mapping_base};

use super::rate::numeric_rate;
use super::unit::UnitMap;
use super::{DynMapping, Mapping};

use crate::mappings::SimplifiedRecord;
use opcode::CompiledExpr;
use rng::MathMapRng;

/// Transform coordinates using mathematical expressions.
///
/// A MathMap is a Mapping which allows you to specify a set of forward and/or
/// inverse transformation functions using arithmetic operations and
/// mathematical functions similar to those available in C.
/// The MathMap interprets these functions at run time, whenever its forward or
/// inverse transformation is required.
/// Because the functions are not compiled in the normal sense (unlike an
/// [`IntraMap`](super::IntraMap)), they may be used to describe coordinate
/// transformations in a transportable manner.
/// A MathMap therefore provides a flexible way of defining new types of
/// Mapping whose descriptions may be stored as part of a dataset and
/// interpreted by other programs.
///
/// [`seed`](MathMap::seed) controls the random number functions, and
/// [`simp_fi`](MathMap::simp_fi) and [`simp_if`](MathMap::simp_if) allow a
/// MathMap in series with its own inverse to simplify away.
/// Otherwise, compound Mappings which involve MathMaps are not simplified,
/// because it cannot be known how different MathMaps will interact.
///
/// # Defining transformation functions
///
/// A MathMap's transformation functions are supplied as a set of expressions.
/// Normally you supply the same number of expressions for the forward
/// transformation as there are output coordinates.
/// For instance, with two outputs you might use:
///
/// - `r = sqrt( x * x + y * y )`
/// - `theta = atan2( y, x )`
///
/// which defines a transformation from Cartesian to polar coordinates.
/// The variables on the left of each expression (`r` and `theta`) name the
/// output variables, and those on the right (`x` and `y`) are references to
/// input variables.
///
/// To complement this, you supply expressions for the inverse transformation,
/// normally as many as there are input coordinates:
///
/// - `x = r * cos( theta )`
/// - `y = r * sin( theta )`
///
/// Here the input variables (`x` and `y`) are named on the left of each
/// expression, and the output variables (`r` and `theta`) are referenced on
/// the right.
///
/// You cannot normally refer to a variable on the right of an expression
/// unless it is named on the left of an expression in the complementary set
/// of functions, so both sets must use the same consistent set of variable
/// names.
/// To leave one of the transformations undefined, supply dummy expressions
/// which simply name each of the output (or input) variables: `x` and `y`
/// for the inverse transformation above name the input variables without
/// defining an inverse transformation.
///
/// # Calculating intermediate values
///
/// Additional expressions calculate intermediate values, which later
/// expressions may use.
/// These five forward expressions describe 2-dimensional pincushion
/// distortion:
///
/// - `r = sqrt( xin * xin + yin * yin )`
/// - `rout = r * ( 1 + 0.1 * r * r )`
/// - `theta = atan2( yin, xin )`
/// - `xout = rout * cos( theta )`
/// - `yout = rout * sin( theta )`
///
/// The last `nout` expressions give the output variables; you may define as
/// many intermediate variables before them as you choose, and refer to each
/// in any later expression.
/// The intermediate variables are private to their transformation: the
/// inverse transformation may refer only to the output variables (`xout` and
/// `yout`), though it may define its own private intermediate variables.
///
/// # Expression syntax
///
/// The expressions closely follow the syntax of C, with some extensions for
/// compatibility with Fortran.
/// They may contain references to variables and literal constants, together
/// with arithmetic, boolean, relational and bitwise operators, and function
/// invocations.
/// Parentheses may be used to override the normal order of evaluation.
/// There is no built-in limit to the length of expressions, and they are
/// insensitive to case and to additional white space.
///
/// - **Variables.**
///   Variable names must begin with an alphabetic character and may contain
///   only alphabetic characters, digits and the underscore character `_`.
/// - **Literal constants**, such as `0`, `1`, `0.007` or `2.505e-16`.
///   The decimal point and exponent are optional, and `D` may also be used as
///   the exponent character.
///   A unary minus may be used as a prefix.
/// - **Arithmetic** is all double-precision floating point.
///
/// ## Missing data
///
/// Unless indicated otherwise, if any argument of a function or operator is
/// [`BAD_VALUE`] (missing data), the result is also [`BAD_VALUE`], so such
/// values propagate automatically through a MathMap's transformations.
/// [`BAD_VALUE`] is written `<bad>` in expressions.
/// A bad result is also produced by any numerical error (such as division by
/// zero or overflow), or by an invalid argument to a function or operator.
///
/// ## Arithmetic operators
///
/// `x1 + x2`, `x1 - x2`, `x1 * x2`, `x1 / x2`, `x1 ** x2` (power), unary `+`
/// (no effect) and unary `-` (negation).
///
/// ## Boolean operators
///
/// Zero is false and non-zero is true; [`BAD_VALUE`] means "unknown", so a
/// boolean operator returns 0, 1 or bad.
/// Where appropriate, tri-state logic is used: `a || b` is 1 when `a` is
/// non-zero, even if `b` is bad.
///
/// - `x1 && x2` (or `.and.`): 1 if both are non-zero, tri-state.
/// - `x1 || x2` (or `.or.`): 1 if either is non-zero, tri-state.
/// - `x1 ^^ x2`: 1 if exactly one is non-zero.
///   `.neqv.` and `.xor.` compute the same, but bind as loosely as `.eqv.`.
/// - `x1 .eqv. x2`: 1 if both have the same truth value.
/// - `! x` (or `.not.`): 1 if `x` is zero.
///
/// ## Relational operators
///
/// `x1 == x2` (`.eq.`), `x1 != x2` (`.ne.`), `x1 > x2` (`.gt.`),
/// `x1 >= x2` (`.ge.`), `x1 < x2` (`.lt.`) and `x1 <= x2` (`.le.`) return 0
/// or 1, or bad if either argument is bad.
/// They cannot usefully compare a value with `<bad>`, since the result is
/// always bad; use `isbad()` instead.
///
/// ## Bitwise operators
///
/// The operands are floating-point values regarded as fixed-point binary
/// numbers, with negative numbers in two's complement, so that integer values
/// give the results of the pure integer case; bits of the fractional part are
/// operated on as well.
///
/// - `x1 >> x2`: shift `x1` right by the integer part of `x2` (left if
///   negative), dividing by the corresponding power of 2.
/// - `x1 << x2`: shift `x1` left by the integer part of `x2` (right if
///   negative), multiplying by the corresponding power of 2.
/// - `x1 & x2`, `x1 | x2`, `x1 ^ x2`: bitwise AND, OR and exclusive OR.
///
/// There is no bit inversion operator, because inverting the bits of a
/// two's-complement fixed-point number is the same as negating it; to invert
/// only the bits left of the binary point, use `x ^ -1`.
///
/// ## Functions
///
/// | Function | Value |
/// |---|---|
/// | `abs(x)`, `fabs(x)` | absolute value |
/// | `acos(x)`, `asin(x)`, `atan(x)` | inverse cosine, sine and tangent, in radians |
/// | `acosd(x)`, `asind(x)`, `atand(x)` | the same, in degrees |
/// | `atan2(x1, x2)`, `atan2d(x1, x2)` | inverse tangent of `x1/x2`, in radians or degrees |
/// | `acosh(x)`, `asinh(x)`, `atanh(x)`, `acoth(x)`, `acsch(x)`, `asech(x)` | inverse hyperbolic functions |
/// | `aint(x)`, `int(x)` | integer part (round towards zero) |
/// | `ceil(x)` | smallest integer not less than `x` |
/// | `floor(x)` | largest integer not greater than `x` |
/// | `nint(x)` | nearest integer |
/// | `cos(x)`, `sin(x)`, `tan(x)` | cosine, sine and tangent of `x` in radians |
/// | `cosd(x)`, `sind(x)`, `tand(x)` | the same, of `x` in degrees |
/// | `cosh(x)`, `sinh(x)`, `tanh(x)`, `coth(x)`, `csch(x)`, `sech(x)` | hyperbolic functions |
/// | `dim(x1, x2)` | `x1 - x2` if `x1` is greater than `x2`, otherwise 0 |
/// | `exp(x)` | exponential |
/// | `fmod(x1, x2)`, `mod(x1, x2)` | remainder of `x1` divided by `x2` |
/// | `gauss(x1, x2)` | random sample from a Gaussian distribution with mean `x1` and standard deviation `x2` |
/// | `isbad(x)` | 1 if `x` is bad, otherwise 0 |
/// | `log(x)`, `log10(x)` | natural and base-10 logarithm |
/// | `max(x1, x2, ...)`, `min(x1, x2, ...)` | maximum and minimum of two or more values |
/// | `poisson(x)` | random integer-valued sample from a Poisson distribution with mean `x` |
/// | `pow(x1, x2)` | `x1` raised to the power `x2` |
/// | `qif(x1, x2, x3)` | `x2` if `x1` is true, otherwise `x3` |
/// | `rand(x1, x2)` | random sample from a uniform distribution from `x1` to `x2` inclusive |
/// | `sign(x1, x2)` | absolute value of `x1` with the sign of `x2` |
/// | `sinc(x)` | `sin(x)/x` |
/// | `sqr(x)` | `x*x` |
/// | `sqrt(x)` | square root |
///
/// ## Symbolic constants
///
/// The enclosing `<>` brackets are part of the name.
///
/// | Constant | Value |
/// |---|---|
/// | `<bad>` | [`BAD_VALUE`], flagging missing data |
/// | `<dig>` | number of decimal digits of precision in an `f64` |
/// | `<e>` | base of natural logarithms |
/// | `<epsilon>` | smallest positive number such that `1.0 + <epsilon>` differs from 1 |
/// | `<mant_dig>` | number of base-`<radix>` digits in the mantissa of an `f64` |
/// | `<max>` | largest representable `f64` |
/// | `<max_10_exp>` | largest integer such that 10 raised to it is representable |
/// | `<max_exp>` | largest integer such that `<radix>` raised to it minus 1 is representable |
/// | `<min>` | smallest positive normalized `f64` |
/// | `<min_10_exp>` | smallest negative integer such that 10 raised to it is a normalized `f64` |
/// | `<min_exp>` | smallest negative integer such that `<radix>` raised to it minus 1 is a normalized `f64` |
/// | `<pi>` | ratio of a circle's circumference to its diameter |
/// | `<radix>` | radix of the mantissa of an `f64` |
/// | `<rounds>` | rounding mode after addition: -1 indeterminate, 0 toward zero, 1 to nearest, 2 toward plus infinity, 3 toward minus infinity |
///
/// ## Precedence and associativity
///
/// From highest to lowest precedence:
///
/// 1. constants and variables;
/// 2. function arguments and parenthesized expressions;
/// 3. function invocations;
/// 4. `**`;
/// 5. unary `+ - ! .not.`;
/// 6. `* /`;
/// 7. `+ -`;
/// 8. `<< >>`;
/// 9. `< .lt. <= .le. > .gt. >= .ge.`;
/// 10. `== .eq. != .ne.`;
/// 11. `&`;
/// 12. `^`;
/// 13. `|`;
/// 14. `&& .and.`;
/// 15. `^^`;
/// 16. `|| .or.`;
/// 17. `.eqv. .neqv. .xor.`.
///
/// So `**` binds more tightly than a unary operator on its left: `-x**2` is
/// `-(x**2)`.
/// A unary operator on the right of `**` applies to its operand: `x**-2` is
/// `x**(-2)`.
///
/// All operators associate from left to right, except unary `+`, unary `-`,
/// `!`, `.not.` and `**`, which associate from right to left.
pub struct MathMap {
    nin: usize,
    nout: usize,
    /// The cleaned forward expressions (lowercase, no whitespace).
    fwd_exprs: Vec<String>,
    /// The cleaned inverse expressions.
    inv_exprs: Vec<String>,
    /// Compiled forward expressions.
    fwd_compiled: Vec<CompiledExpr>,
    /// Compiled inverse expressions.
    inv_compiled: Vec<CompiledExpr>,
    /// Random number generator context.
    /// UnsafeCell because transform takes &self but needs to mutate the RNG.
    /// This is safe because MathMap is Send but not Sync.
    rng: UnsafeCell<MathMapRng>,
    /// Forward-inverse pairs simplify?
    simp_fi: Option<bool>,
    /// Inverse-forward pairs simplify?
    simp_if: Option<bool>,
    inverted: bool,
    simplified: SimplifiedRecord,
    ident: Option<String>,
    /// The Object-class ID attribute, which a clone does not inherit.
    id: crate::object_id::ObjectId,
    /// The flags C keeps on the base Mapping class (mapping.c).
    mapping_flags: crate::mappings::MappingFlags,
    /// Whether the forward transform is defined.
    has_forward: bool,
    /// Whether the inverse transform is defined.
    has_inverse: bool,
}

// Safety: MathMap is Send but not Sync (the RNG is mutated during transform).
// This matches the design constraint: "Send, not Sync".
unsafe impl Send for MathMap {}

impl std::fmt::Debug for MathMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MathMap")
            .field("nin", &self.nin)
            .field("nout", &self.nout)
            .field("fwd_exprs", &self.fwd_exprs)
            .field("inv_exprs", &self.inv_exprs)
            .field("inverted", &self.inverted)
            .finish_non_exhaustive()
    }
}

impl Clone for MathMap {
    fn clone(&self) -> Self {
        // Safety: we have exclusive access during clone (single-threaded or Send)
        let rng = unsafe { (*self.rng.get()).clone() };
        Self {
            nin: self.nin,
            nout: self.nout,
            fwd_exprs: self.fwd_exprs.clone(),
            inv_exprs: self.inv_exprs.clone(),
            fwd_compiled: self.fwd_compiled.clone(),
            inv_compiled: self.inv_compiled.clone(),
            rng: UnsafeCell::new(rng),
            simp_fi: self.simp_fi,
            simp_if: self.simp_if,
            inverted: self.inverted,
            simplified: self.simplified,
            ident: self.ident.clone(),
            id: Default::default(),
            mapping_flags: self.mapping_flags.clone(),
            has_forward: self.has_forward,
            has_inverse: self.has_inverse,
        }
    }
}

impl MathMap {
    /// Create a MathMap with `nin` inputs and `nout` outputs from the
    /// expressions defining its forward (`fwd`) and inverse (`inv`)
    /// transformations.
    ///
    /// `fwd` must hold at least `nout` expressions and `inv` at least `nin`;
    /// any extra expressions define intermediate variables.
    /// The [`MathMap`] type documentation describes the expressions.
    ///
    /// ```
    /// use starlink_ast::mappings::{Mapping, MathMap};
    ///
    /// let map = MathMap::new(
    ///     2,
    ///     2,
    ///     &["r = sqrt( x * x + y * y )", "theta = atan2( y, x )"],
    ///     &["x = r * cos( theta )", "y = r * sin( theta )"],
    /// )
    /// .unwrap();
    /// let (r, theta) = map.tran2(&[3.0], &[4.0], true).unwrap();
    /// assert_eq!(r[0], 5.0);
    /// assert!((theta[0] - 4.0_f64.atan2(3.0)).abs() < 1e-15);
    /// let (x, y) = map.tran2(&r, &theta, false).unwrap();
    /// assert!((x[0] - 3.0).abs() < 1e-14 && (y[0] - 4.0).abs() < 1e-14);
    /// ```
    ///
    /// # Errors
    ///
    /// - `ErrorCode::Badni` if `nin` is zero.
    /// - `ErrorCode::Badno` if `nout` is zero.
    /// - `ErrorCode::Inntf` if `fwd` has fewer than `nout` expressions or
    ///   `inv` fewer than `nin`.
    /// - `ErrorCode::Misvn` if a function has no left-hand side, `ErrorCode::Varin` if a left-hand side (or a function with no `=`) is not a variable name, or `ErrorCode::Duvar` if a direction's inputs and functions name a variable twice.
    /// - `ErrorCode::Norhs` if a function's `=` has nothing after it, or if some but not all of a direction's functions have an `=`.
    /// - Any of the expression errors, such as `ErrorCode::Miopr` (a missing operator), `ErrorCode::Miopa` (a missing operand), `ErrorCode::Mrpar` (a missing right parenthesis) or `ErrorCode::Udvof` (an undefined variable or function), if an expression cannot be parsed.
    #[doc(alias = "astMathMap")]
    pub fn new(nin: usize, nout: usize, fwd: &[&str], inv: &[&str]) -> AstResult<Self> {
        if nin == 0 {
            return Err(AstError::new(
                ErrorCode::Badni,
                "MathMap nin must be at least 1".to_string(),
            ));
        }
        if nout == 0 {
            return Err(AstError::new(
                ErrorCode::Badno,
                "MathMap nout must be at least 1".to_string(),
            ));
        }
        // mathmap.c astInitMathMap: AST__INNTF for too few functions.
        if fwd.len() < nout {
            return Err(AstError::new(
                ErrorCode::Inntf,
                format!(
                    "MathMap needs at least {} forward expressions, got {}",
                    nout,
                    fwd.len()
                ),
            ));
        }
        if inv.len() < nin {
            return Err(AstError::new(
                ErrorCode::Inntf,
                format!(
                    "MathMap needs at least {} inverse expressions, got {}",
                    nin,
                    inv.len()
                ),
            ));
        }

        // Clean expressions (lowercase, no whitespace)
        let fwd_clean: Vec<String> = fwd.iter().map(|s| clean_expr(s)).collect();
        let inv_clean: Vec<String> = inv.iter().map(|s| clean_expr(s)).collect();

        // Compile both directions (cross-referencing variable names)
        let compiled = compile_mapping(&fwd_clean, &inv_clean, nin, nout)?;
        let fwd_compiled = compiled.fwd;
        let inv_compiled = compiled.inv;
        let has_forward = compiled.has_fwd;
        let has_inverse = compiled.has_inv;

        Ok(Self {
            nin,
            nout,
            fwd_exprs: fwd_clean,
            inv_exprs: inv_clean,
            fwd_compiled,
            inv_compiled,
            rng: UnsafeCell::new(MathMapRng::with_default_seed()),
            simp_fi: None,
            simp_if: None,
            inverted: false,
            simplified: SimplifiedRecord::default(),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
            has_forward,
            has_inverse,
        })
    }

    /// Whether applying the MathMap's forward transformation followed
    /// immediately by the matching inverse always restores the original
    /// coordinates.
    ///
    /// If true, simplification may replace such a sequence by a
    /// [`UnitMap`](super::UnitMap).
    /// For it to happen, the two MathMaps must be in series and identical,
    /// with textually identical transformation functions; functional
    /// equivalence is not sufficient.
    /// Both MathMaps must consent: if either has `simp_fi` false, nothing is
    /// simplified.
    /// It does not apply to an inverse transformation followed by a forward
    /// one, which [`simp_if`](MathMap::simp_if) controls.
    ///
    /// "Forward" and "inverse" refer to the transformations defined when the
    /// MathMap was created; if the MathMap is inverted, the roles of
    /// `simp_fi` and `simp_if` are interchanged.
    ///
    /// Default: false.
    ///
    /// [`set_simp_fi`](MathMap::set_simp_fi),
    /// [`test_simp_fi`](MathMap::test_simp_fi) and
    /// [`clear_simp_fi`](MathMap::clear_simp_fi) set, test and clear it.
    #[doc(alias = "SimpFI")]
    pub fn simp_fi(&self) -> bool {
        self.simp_fi.unwrap_or(false)
    }

    /// Set [`simp_fi`](MathMap::simp_fi).
    ///
    /// A value different from the one held clears the record that the
    /// MathMap has been simplified.
    pub fn set_simp_fi(&mut self, val: bool) {
        // `SimpFI` is read by MathMap's own MapMerge, which cancels a
        // forward-then-inverse pair only when both MathMaps permit it
        // (mathmap.c:4079-4082), so a record left in place could decline the
        // cancellation the new value allows (mathmap.c:5698-5705,
        // `astClearIsSimple` at mapping.h:449-472).
        if Some(val) != self.simp_fi {
            self.simplified.clear();
        }
        self.simp_fi = Some(val);
    }

    /// Whether applying the MathMap's inverse transformation followed
    /// immediately by the matching forward transformation always restores the
    /// original coordinates.
    ///
    /// It is the counterpart of [`simp_fi`](MathMap::simp_fi), whose
    /// conditions apply in the same way, for an inverse transformation
    /// followed by a forward one.
    ///
    /// Default: false.
    ///
    /// [`set_simp_if`](MathMap::set_simp_if),
    /// [`test_simp_if`](MathMap::test_simp_if) and
    /// [`clear_simp_if`](MathMap::clear_simp_if) set, test and clear it.
    #[doc(alias = "SimpIF")]
    pub fn simp_if(&self) -> bool {
        self.simp_if.unwrap_or(false)
    }

    /// Set [`simp_if`](MathMap::simp_if).
    ///
    /// A value different from the one held clears the record that the
    /// MathMap has been simplified.
    pub fn set_simp_if(&mut self, val: bool) {
        // `SimpIF` is read by the same MapMerge test (mathmap.c:5771-5778).
        if Some(val) != self.simp_if {
            self.simplified.clear();
        }
        self.simp_if = Some(val);
    }

    /// Whether [`simp_fi`](MathMap::simp_fi) is set, to either value.
    pub fn test_simp_fi(&self) -> bool {
        self.simp_fi.is_some()
    }

    /// Clear [`simp_fi`](MathMap::simp_fi), so that it reads as the default
    /// of false.
    ///
    /// This also clears the record that the MathMap has been simplified.
    pub fn clear_simp_fi(&mut self) {
        // mathmap.c ClearSimpFI.
        self.simplified.clear();
        self.simp_fi = None;
    }

    /// Whether [`simp_if`](MathMap::simp_if) is set, to either value.
    pub fn test_simp_if(&self) -> bool {
        self.simp_if.is_some()
    }

    /// Clear [`simp_if`](MathMap::simp_if), so that it reads as the default
    /// of false.
    ///
    /// This also clears the record that the MathMap has been simplified.
    pub fn clear_simp_if(&mut self) {
        // mathmap.c ClearSimpIF.
        self.simplified.clear();
        self.simp_if = None;
    }

    /// The seed that determines the sequence of random numbers produced by
    /// the random number functions in the MathMap's expressions.
    ///
    /// It may take any integer value.
    /// Set it to a value of your choosing to make the random number functions
    /// repeatable.
    /// You may also read an unpredictable default and use it to reproduce the
    /// resulting sequence, from the same MathMap or another.
    ///
    /// Default: an unpredictable value chosen when the MathMap is created, so
    /// that each MathMap uses a different set of random numbers.
    ///
    /// [`set_seed`](MathMap::set_seed), [`test_seed`](MathMap::test_seed) and
    /// [`clear_seed`](MathMap::clear_seed) set, test and clear it.
    #[doc(alias = "Seed")]
    pub fn seed(&self) -> i32 {
        // Safety: single-threaded access
        unsafe { (*self.rng.get()).seed }
    }

    /// Set [`seed`](MathMap::seed), restarting the random number sequence.
    ///
    /// Setting a value, or setting the default value explicitly, clears the
    /// record that the MathMap has been simplified.
    pub fn set_seed(&mut self, seed: i32) {
        // `Seed` changes the sequence the random-number functions return, hence
        // the transformation itself (mathmap.c:5629-5635).
        //
        // The guard has two terms because `Seed` carries a separate set flag.
        // An unset MathMap still *has* a seed -- the unpredictable default -- so
        // setting that same integer explicitly leaves the field alone while
        // changing the attribute's state from unset to set, which is a real
        // change and must discard the records. Comparing values alone would miss
        // it (upstream `1adbc353`).
        let rng = self.rng.get_mut();
        if !rng.seed_set || seed != rng.seed {
            self.simplified.clear();
        }
        self.rng.get_mut().set_seed(seed);
    }

    /// Whether [`seed`](MathMap::seed) is set.
    pub fn test_seed(&self) -> bool {
        // Safety: single-threaded access
        unsafe { (*self.rng.get()).seed_set }
    }

    /// Clear [`seed`](MathMap::seed), which gives it a new unpredictable
    /// default value.
    ///
    /// This also clears the record that the MathMap has been simplified.
    pub fn clear_seed(&mut self) {
        // As for `set_seed` (mathmap.c:5616).
        self.simplified.clear();
        self.rng.get_mut().clear_seed();
    }
}

impl Mapping for MathMap {
    fn mapping_flags(&self) -> &crate::mappings::MappingFlags {
        &self.mapping_flags
    }

    fn mapping_flags_mut(&mut self) -> &mut crate::mappings::MappingFlags {
        &mut self.mapping_flags
    }

    fn nin(&self) -> usize {
        if self.inverted { self.nout } else { self.nin }
    }

    fn nout(&self) -> usize {
        if self.inverted { self.nin } else { self.nout }
    }

    fn is_inverted(&self) -> bool {
        self.inverted
    }

    fn invert_orientation(&mut self) {
        self.inverted = !self.inverted;
    }

    fn has_forward(&self) -> bool {
        if self.inverted {
            self.has_inverse
        } else {
            self.has_forward
        }
    }

    fn has_inverse(&self) -> bool {
        if self.inverted {
            self.has_forward
        } else {
            self.has_inverse
        }
    }

    fn dump_tran_flags(&self) -> (bool, bool) {
        // C passes the function-list availability to astInitMapping
        // (mathmap.c).
        (self.has_forward, self.has_inverse)
    }

    fn transform(&self, pts: &PointSet, forward: bool) -> AstResult<PointSet> {
        crate::mappings::validate_transform(self, pts, forward, None)?;
        let effective_forward = forward != self.inverted;
        let ncoord_out = if effective_forward {
            self.nout
        } else {
            self.nin
        };
        let mut out = PointSet::zeros(pts.npoint(), ncoord_out);
        self.transform_into(pts.as_points(), forward, out.as_points_mut())?;
        Ok(out)
    }

    /// NaN-as-bad fast path. MathMap evaluates user-supplied expressions
    /// via a compiled token list. Arithmetic operators propagate NaN
    /// cleanly per IEEE 754, but conditional operators (`>`, `<`,
    /// `==`, `?:`, `qif`) follow C-like ordered semantics that return
    /// false for NaN comparisons and can therefore silently take the
    /// "wrong" branch. Auditing every operator is out of scope; use the
    /// delegate-and-post-process pattern so the protocol-canonical
    /// sentinel reaches downstream stages without re-implementing the
    /// evaluator.
    fn transform_into_nan(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        let npoint = pts.npoint();
        self.transform_into(pts, forward, out.reborrow())?;
        crate::mappings::bad_to_nan(self, forward, npoint, &mut out);
        Ok(())
    }

    fn transform_into(
        &self,
        pts: Points<'_>,
        forward: bool,
        mut out: PointsMut<'_>,
    ) -> AstResult<()> {
        crate::mappings::validate_transform_into(self, &pts, forward, Some(&out))?;
        let effective_forward = forward != self.inverted;
        let npoint = pts.npoint();
        let ncoord_in = if effective_forward {
            self.nin
        } else {
            self.nout
        };
        let ncoord_out = if effective_forward {
            self.nout
        } else {
            self.nin
        };

        let exprs = if effective_forward {
            &self.fwd_compiled
        } else {
            &self.inv_compiled
        };

        if exprs.is_empty() {
            // Undefined transformation: fill with BAD
            for axis in out.axes_iter_mut().take(ncoord_out) {
                axis[..npoint].fill(BAD_VALUE);
            }
            return Ok(());
        }

        let nfun = exprs.len();

        // Collect input data as column vectors. A NaN input is a bad value
        // (CLAUDE.md), which the evaluator tests for as C's does.
        let input_data: Vec<Vec<f64>> = (0..ncoord_in)
            .map(|c| {
                pts.axis(c)[..npoint]
                    .iter()
                    .map(|&v| if v.is_nan() { BAD_VALUE } else { v })
                    .collect()
            })
            .collect();
        let input_refs: Vec<&[f64]> = input_data.iter().map(Vec::as_slice).collect();

        // Allocate intermediate workspace
        let n_intermediate = nfun.saturating_sub(ncoord_out);
        let mut intermediates: Vec<Vec<f64>> =
            (0..n_intermediate).map(|_| vec![0.0; npoint]).collect();

        // Output workspace
        let mut outputs: Vec<Vec<f64>> = (0..ncoord_out).map(|_| vec![0.0; npoint]).collect();

        // Safety: MathMap is Send but not Sync. We have exclusive logical
        // access to the RNG during a transform call.
        let rng_ptr = self.rng.get();

        // Evaluate each function.
        // We use raw pointers to work around borrow checker limitations:
        // we need immutable refs to previously computed slots while writing
        // to the current slot. This is safe because we only write to slot
        // `ifun` and read from slots `0..ifun`.
        for ifun in 0..nfun {
            let mut data_ptrs: Vec<&[f64]> = input_refs.clone();

            // Add previously computed intermediates (read-only)
            for slot in intermediates.iter().take(n_intermediate.min(ifun)) {
                // Safety: we only read from slots < ifun which are complete
                unsafe {
                    let ptr = slot.as_ptr();
                    data_ptrs.push(std::slice::from_raw_parts(ptr, npoint));
                }
            }

            // Add previously computed outputs (read-only)
            let prev_outputs = ifun.saturating_sub(n_intermediate);
            for slot in outputs.iter().take(prev_outputs) {
                unsafe {
                    let ptr = slot.as_ptr();
                    data_ptrs.push(std::slice::from_raw_parts(ptr, npoint));
                }
            }

            // Determine where this function's output goes
            let output_slice: &mut [f64] = if ifun < n_intermediate {
                &mut intermediates[ifun]
            } else {
                &mut outputs[ifun - n_intermediate]
            };

            // Safety: we have exclusive access to the RNG through the mapping
            unsafe {
                evaluator::evaluate(
                    &exprs[ifun],
                    &data_ptrs,
                    npoint,
                    output_slice,
                    &mut *rng_ptr,
                );
            }
        }

        // Copy outputs to the PointSet
        for (coord, values) in outputs.iter().enumerate().take(ncoord_out) {
            for (val, &v) in out.axis_mut(coord).iter_mut().zip(values).take(npoint) {
                *val = if v.is_nan() { BAD_VALUE } else { v };
            }
        }

        Ok(())
    }

    fn map_merge(
        &self,
        ctx: &mut crate::mappings::simplify::MergeCtx,
        where_: usize,
        series: bool,
    ) -> Option<usize> {
        // A MathMap has no replace-with-simpler step. C's `MapMerge` merges it
        // only with the Mapping that follows it in series, only when that one is
        // also a MathMap, and only when both permit it via SimpFI / SimpIF
        // (`mathmap.c:4047-4083`).
        crate::mappings::simplify::default_map_merge(ctx, where_, series)
    }

    fn rate(&self, at: &[f64], ax1: usize, ax2: usize) -> AstResult<f64> {
        crate::mappings::validate_rate(self, at, ax1, ax2)?;
        numeric_rate(self, at, ax1, ax2)
    }

    fn equal(&self, other: &dyn Mapping) -> bool {
        let Some(other) = other.as_any().downcast_ref::<MathMap>() else {
            return false;
        };
        if self.nin != other.nin || self.nout != other.nout {
            return false;
        }
        if self.inverted != other.inverted {
            return false;
        }
        self.fwd_exprs == other.fwd_exprs && self.inv_exprs == other.inv_exprs
    }

    fn try_merge(
        &self,
        other: &dyn Mapping,
        series: bool,
        self_inv: bool,
        other_inv: bool,
    ) -> Option<DynMapping> {
        if !series {
            return None;
        }
        let other = other.as_any().downcast_ref::<MathMap>()?;

        // Check SimpFI / SimpIF attributes
        let self_simplifies = if self_inv {
            self.simp_if()
        } else {
            self.simp_fi()
        };
        let other_simplifies = if other_inv {
            other.simp_fi()
        } else {
            other.simp_if()
        };

        if !self_simplifies || !other_simplifies {
            return None;
        }

        // Get effective dimensions
        let nin1 = if self_inv { self.nout } else { self.nin };
        let nout2 = if other_inv { other.nin } else { other.nout };
        if nin1 != nout2 {
            return None;
        }

        // Compare effective forward of self with effective inverse of other
        let fwd1: &[String] = if !self_inv {
            &self.fwd_exprs
        } else {
            &self.inv_exprs
        };
        let inv2: &[String] = if !other_inv {
            &other.inv_exprs
        } else {
            &other.fwd_exprs
        };
        if fwd1.len() != inv2.len() {
            return None;
        }
        if fwd1 != inv2 {
            return None;
        }

        // Compare effective inverse of self with effective forward of other
        let inv1: &[String] = if !self_inv {
            &self.inv_exprs
        } else {
            &self.fwd_exprs
        };
        let fwd2: &[String] = if !other_inv {
            &other.fwd_exprs
        } else {
            &other.inv_exprs
        };
        if inv1.len() != fwd2.len() {
            return None;
        }
        if inv1 != fwd2 {
            return None;
        }

        // They cancel — return a UnitMap
        Some(Box::new(UnitMap::new(nin1)))
    }

    fn ident(&self) -> &str {
        self.ident.as_deref().unwrap_or("")
    }

    fn set_ident(&mut self, ident: &str) {
        self.ident = Some(ident.to_string());
    }

    fn clear_ident(&mut self) {
        self.ident = None;
    }

    fn test_ident(&self) -> bool {
        self.ident.is_some()
    }

    fn id(&self) -> &str {
        self.id.get()
    }
    fn set_id(&mut self, id: &str) {
        self.id.set(id);
    }
    fn clear_id(&mut self) {
        self.id.clear();
    }
    fn test_id(&self) -> bool {
        self.id.test()
    }

    fn has_been_simplified(&self) -> bool {
        self.simplified.get(self.inverted)
    }

    fn set_has_been_simplified(&mut self, flag: bool) {
        self.simplified.set(self.inverted, flag);
    }

    fn clone_box(&self) -> DynMapping {
        Box::new(self.clone())
    }
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

impl Serializable for MathMap {
    fn class_name(&self) -> &'static str {
        "MathMap"
    }

    fn to_node(&self) -> AstResult<Node> {
        let mut node = Node::new("MathMap");
        node.class_comment = Some("Transformation using mathematical functions".to_string());
        write_mapping_base(&mut node, self);

        // mathmap.c `Dump`: Nfwd and Ninv are set when they differ from the
        // number of outputs and inputs.
        let nfwd = self.fwd_exprs.len();
        node.write_int(
            "Nfwd",
            nfwd != self.nout,
            false,
            nfwd as i64,
            "Number of forward transformation functions",
        );
        for (i, expr) in self.fwd_exprs.iter().enumerate() {
            let key = format!("Fwd{}", i + 1);
            node.push_string(&key, expr, Some(format!("Forward function {}", i + 1)));
        }
        let ninv = self.inv_exprs.len();
        node.write_int(
            "Ninv",
            ninv != self.nin,
            false,
            ninv as i64,
            "Number of inverse transformation functions",
        );
        for (i, expr) in self.inv_exprs.iter().enumerate() {
            let key = format!("Inv{}", i + 1);
            node.push_string(&key, expr, Some(format!("Inverse function {}", i + 1)));
        }
        let v = self.simp_fi.unwrap_or(false);
        node.write_int(
            "SimpFI",
            self.simp_fi.is_some(),
            false,
            i64::from(v),
            if v {
                "Forward-inverse pairs may simplify"
            } else {
                "Forward-inverse pairs do not simplify"
            },
        );
        let v = self.simp_if.unwrap_or(false);
        node.write_int(
            "SimpIF",
            self.simp_if.is_some(),
            false,
            i64::from(v),
            if v {
                "Inverse-forward pairs may simplify"
            } else {
                "Inverse-forward pairs do not simplify"
            },
        );

        // The seed flag, then the seed: the one set, or the default chosen
        // when the MathMap was made.
        // Safety: single-threaded access for serialization
        let rng = unsafe { &*self.rng.get() };
        let set = rng.seed_set;
        node.write_int(
            "Seeded",
            set,
            false,
            i64::from(set),
            if set {
                "Explicit random number seed set"
            } else {
                "No random number seed set"
            },
        );
        node.write_int(
            "Seed",
            set,
            false,
            i64::from(rng.seed),
            if set {
                "Random number seed value"
            } else {
                "Default random number seed used"
            },
        );

        Ok(node)
    }

    fn from_node(node: &Node) -> AstResult<Self>
    where
        Self: Sized,
    {
        // Each class's items come from its own part of the description, as
        // C's loaders read them through astReadClassData (channel.c).
        let whole = node;
        let node = &whole.section("MathMap");
        let base = read_mapping_base(whole);
        let nin = base.nin;
        let nout = base.nout;

        // Read Nfwd / Ninv with defaults
        let nfwd = {
            let v = node.get_int("Nfwd", 0);
            if v > 0 { v as usize } else { nout }
        };
        let ninv = {
            let v = node.get_int("Ninv", 0);
            if v > 0 { v as usize } else { nin }
        };

        // Read forward expressions
        let mut fwd_exprs = Vec::with_capacity(nfwd);
        for i in 0..nfwd {
            let key = format!("Fwd{}", i + 1);
            let expr = node.get_string(&key, "");
            fwd_exprs.push(expr.to_string());
        }

        // Read inverse expressions
        let mut inv_exprs = Vec::with_capacity(ninv);
        for i in 0..ninv {
            let key = format!("Inv{}", i + 1);
            let expr = node.get_string(&key, "");
            inv_exprs.push(expr.to_string());
        }

        // Compile both directions (cross-referencing variable names)
        let compiled = compile_mapping(&fwd_exprs, &inv_exprs, nin, nout)?;
        let fwd_compiled = compiled.fwd;
        let inv_compiled = compiled.inv;
        let has_forward = compiled.has_fwd;
        let has_inverse = compiled.has_inv;

        // Read the raw attribute values against C's unset defaults. C assigns
        // each straight into the structure component and only then re-applies it
        // through the setter, so the set is a no-op and the guarded clear does
        // not fire (mathmap.c:7397, 7403, 7416). The fields are therefore seeded
        // in the struct literal below rather than left unset, which is what lets
        // a dumped `IsSimp` card survive being read back.
        let simp_fi_raw = node.get_int("SimpFI", -1);
        let simp_if_raw = node.get_int("SimpIF", -1);
        let seeded = node.get_int("Seeded", 0) != 0;
        let seed = node.get_int("Seed", 0) as i32;

        let mut m = Self {
            nin,
            nout,
            fwd_exprs,
            inv_exprs,
            fwd_compiled,
            inv_compiled,
            rng: UnsafeCell::new(MathMapRng::with_default_seed()),
            simp_fi: (simp_fi_raw >= 0).then_some(simp_fi_raw != 0),
            simp_if: (simp_if_raw >= 0).then_some(simp_if_raw != 0),
            inverted: base.inverted,
            simplified: SimplifiedRecord::for_orientation(base.inverted, base.has_been_simplified),
            ident: None,
            id: Default::default(),
            mapping_flags: crate::mappings::MappingFlags::default(),
            has_forward,
            has_inverse,
        };
        if simp_fi_raw >= 0 {
            m.set_simp_fi(simp_fi_raw != 0);
        }
        if simp_if_raw >= 0 {
            m.set_simp_if(simp_if_raw != 0);
        }
        if seeded {
            m.set_seed(seed);
        }
        Ok(m)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Clean an expression: lowercase, remove whitespace.
fn clean_expr(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The transformation direction and 1-based position, as the caller numbers them, of entry `index` in a list of functions whose names are extracted.
/// The list starts with the `nprimary` functions of the other direction that name the primary variables, which follow `nextra` intermediate functions there, and continues with every function of the direction being compiled.
fn function_label(index: usize, nprimary: usize, nextra: usize, forward: bool) -> String {
    if index < nprimary {
        let dir = if forward { "inverse" } else { "forward" };
        format!("{dir} transformation function {}", index + 1 + nextra)
    } else {
        let dir = if forward { "forward" } else { "inverse" };
        format!("{dir} transformation function {}", index + 1 - nprimary)
    }
}

/// Extract the variable names from the left-hand sides of `funs`.
///
/// Each function's left-hand side is the text before its first `=`, or the whole function if it has none.
/// It must be a name (a letter followed by letters, digits and underscores), and no two may be equal.
/// `funs` holds the functions of the other direction that name the primary variables (`nin` inverse functions when compiling forward, `nout` forward functions when compiling the inverse), followed by every function of the direction being compiled.
/// The counts are used to number the functions in messages.
fn extract_variables(
    funs: &[&str],
    nin: usize,
    nout: usize,
    nfwd: usize,
    ninv: usize,
    forward: bool,
) -> AstResult<Vec<String>> {
    let nprimary = if forward { nin } else { nout };
    let nextra = if forward { ninv - nin } else { nfwd - nout };
    let mut names: Vec<String> = Vec::with_capacity(funs.len());
    for (ifun, fun) in funs.iter().enumerate() {
        let lhs = fun.split('=').next().unwrap_or("");
        let error = if lhs.is_empty() {
            let msg = if fun.contains('=') {
                format!("no left hand side in expression: \"{fun}\"")
            } else {
                "transformation function contains no variable name".to_string()
            };
            Some(AstError::new(ErrorCode::Misvn, msg))
        } else if token::parse_name(lhs, 0) != Some(lhs.len()) {
            Some(AstError::new(
                ErrorCode::Varin,
                format!("variable name is invalid: \"{lhs}\""),
            ))
        } else {
            None
        };
        if let Some(e) = error {
            return Err(e.with_context(format!(
                "Error in {}.",
                function_label(ifun, nprimary, nextra, forward)
            )));
        }
        names.push(lhs.to_string());
    }
    for i1 in 1..names.len() {
        if let Some(i2) = names[..i1].iter().position(|n| *n == names[i1]) {
            let label = |i: usize| {
                function_label(i, nprimary, nextra, forward).replace(" transformation", "")
            };
            return Err(AstError::new(
                ErrorCode::Duvar,
                format!("duplicate definition of variable name: \"{}\"", names[i1]),
            )
            .with_context(format!("Conflict between {} and {}.", label(i1), label(i2))));
        }
    }
    Ok(names)
}

/// Extract the right-hand sides of `funs`, the text after each first `=`.
///
/// Returns `None` when no function has an `=`, so that the transformation is undefined.
/// A function whose `=` has nothing after it, or a mixture of functions with and without an `=`, has a missing right-hand side.
fn extract_expressions(funs: &[String], forward: bool) -> AstResult<Option<Vec<String>>> {
    let dir = if forward { "forward" } else { "inverse" };
    let mut exprs = Vec::with_capacity(funs.len());
    let mut first_undefined = None;
    for (ifun, fun) in funs.iter().enumerate() {
        match fun.split_once('=') {
            Some((_, "")) => {
                return Err(AstError::new(
                    ErrorCode::Norhs,
                    format!("missing right hand side in expression: \"{fun}\""),
                )
                .with_context(format!(
                    "Error in {dir} transformation function {}.",
                    ifun + 1
                )));
            }
            Some((_, rhs)) => exprs.push(rhs.to_string()),
            None => {
                first_undefined.get_or_insert(ifun);
            }
        }
    }
    match first_undefined {
        None => Ok(Some(exprs)),
        Some(_) if exprs.is_empty() => Ok(None),
        Some(iud) => Err(AstError::new(
            ErrorCode::Norhs,
            format!("missing right hand side in function: \"{}\"", funs[iud]),
        )
        .with_context(format!(
            "Error in {dir} transformation function {}.",
            iud + 1
        ))),
    }
}

/// Compile the functions of one direction.
///
/// `names` are the variable names `extract_variables` returned for that direction: the `nprimary` input names, then one per function.
/// Each expression may use the input names and the names defined before it.
/// Returns `None` when the direction is undefined.
fn compile_direction(
    funs: &[String],
    names: &[String],
    nprimary: usize,
    forward: bool,
) -> AstResult<Option<Vec<CompiledExpr>>> {
    let Some(exprs) = extract_expressions(funs, forward)? else {
        return Ok(None);
    };
    let dir = if forward { "forward" } else { "inverse" };
    let mut compiled = Vec::with_capacity(exprs.len());
    for (ifun, rhs) in exprs.iter().enumerate() {
        let vars: Vec<&str> = names[..nprimary + ifun]
            .iter()
            .map(String::as_str)
            .collect();
        let expr = token::tokenize(rhs, &vars)
            .and_then(|tokens| parser::compile(&tokens))
            .map_err(|e| {
                e.with_context(format!(
                    "Error in {dir} transformation function {}.",
                    ifun + 1
                ))
            })?;
        compiled.push(expr);
    }
    Ok(Some(compiled))
}

/// Result of compiling both directions of a MathMap.
struct CompiledMapping {
    fwd: Vec<CompiledExpr>,
    inv: Vec<CompiledExpr>,
    has_fwd: bool,
    has_inv: bool,
}

/// Compile the forward and inverse functions of a MathMap.
///
/// Each direction's input variables are named by the left-hand sides of the last functions of the other direction: the last `nin` inverse functions name the inputs, and the last `nout` forward functions name the outputs.
/// The forward direction is compiled first, its names validated, then its expressions; then the inverse direction.
fn compile_mapping(
    fwd_exprs: &[String],
    inv_exprs: &[String],
    nin: usize,
    nout: usize,
) -> AstResult<CompiledMapping> {
    let (nfwd, ninv) = (fwd_exprs.len(), inv_exprs.len());
    if nfwd < nout || ninv < nin {
        return Err(AstError::new(
            ErrorCode::Inntf,
            format!(
                "MathMap needs at least {nout} forward and {nin} inverse functions, got {nfwd} and {ninv}"
            ),
        ));
    }

    let fwd_funs: Vec<&str> = inv_exprs[ninv - nin..]
        .iter()
        .chain(fwd_exprs)
        .map(String::as_str)
        .collect();
    let fwd_names = extract_variables(&fwd_funs, nin, nout, nfwd, ninv, true)?;
    let fwd = compile_direction(fwd_exprs, &fwd_names, nin, true)?;

    let inv_funs: Vec<&str> = fwd_exprs[nfwd - nout..]
        .iter()
        .chain(inv_exprs)
        .map(String::as_str)
        .collect();
    let inv_names = extract_variables(&inv_funs, nin, nout, nfwd, ninv, false)?;
    let inv = compile_direction(inv_exprs, &inv_names, nout, false)?;

    Ok(CompiledMapping {
        has_fwd: fwd.is_some(),
        has_inv: inv.is_some(),
        fwd: fwd.unwrap_or_default(),
        inv: inv.unwrap_or_default(),
    })
}
