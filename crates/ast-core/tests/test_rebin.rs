//! `mappings::rebin` on known data.
//!
//! C ancestor: `reference/starlink-ast/ast_tester/testrebin.c`, tests 1 to
//! 9, each run for the `D`, `F` and `I` types and the `SINC`, `NEAREST`,
//! `LINEAR`, `SINCSINC`, `SINCCOS` and `SINCGAUSS` spreading schemes.
//!
//! Divergence from C: C's three-phase callback over void pointers becomes
//! the [`RebinTest`] trait, generic over the pixel type through
//! [`TestType`], and each case allocates fresh zeroed arrays rather than
//! reusing one allocation per type; every input element is written before
//! each call in both, so the inputs are the same.

use ast_core::constants::BAD_VALUE;
use ast_core::mappings::{
    Mapping, RebinFlags, RebinInput, RebinOptions, RebinOutput, ShiftMap, Spread, UnitMap, rebin,
};
use ast_core::pixel::RebinValue;

/// The spreading schemes every test is run with, in C's order.
const SPREADS: [Spread; 6] = [
    Spread::Sinc,
    Spread::Nearest,
    Spread::Linear,
    Spread::SincSinc,
    Spread::SincCos,
    Spread::SincGauss,
];

/// C's `equald`: a relative tolerance of about 1e-9.
fn equald(a: f64, b: f64) -> bool {
    if a != 0.0 && b != 0.0 {
        (a - b).abs() <= 1.0e9 * (a + b).abs() * f64::EPSILON
    } else {
        (a + b).abs() <= 1.0e-11
    }
}

/// C's `equalf`: a relative tolerance of 50 `FLT_EPSILON`.
fn equalf(a: f32, b: f32) -> bool {
    if a != 0.0 && b != 0.0 {
        (a - b).abs() <= 50.0 * (a + b).abs() * f32::EPSILON
    } else {
        (a + b).abs() <= 1.0e-11
    }
}

/// The per-type parts of C's test harness.
trait TestType: RebinValue {
    /// C's `type_name`.
    const NAME: &'static str;
    /// The largest value of the type, as a double.
    fn maxval() -> f64;
    /// The bad value the test passes to `rebin` and checks outputs against.
    fn bad() -> Self;
    /// C's `equal_gen`: approximate for a floating type, exact otherwise.
    fn equal(a: Self, b: Self) -> bool;
}

impl TestType for f64 {
    const NAME: &'static str = "DOUBLE";
    fn maxval() -> f64 {
        f64::MAX
    }
    fn bad() -> Self {
        BAD_VALUE
    }
    fn equal(a: Self, b: Self) -> bool {
        equald(a, b)
    }
}

impl TestType for f32 {
    const NAME: &'static str = "FLOAT";
    fn maxval() -> f64 {
        f64::from(f32::MAX)
    }
    fn bad() -> Self {
        BAD_VALUE as f32
    }
    fn equal(a: Self, b: Self) -> bool {
        equalf(a, b)
    }
}

impl TestType for i32 {
    const NAME: &'static str = "INT";
    fn maxval() -> f64 {
        f64::from(i32::MAX)
    }
    fn bad() -> Self {
        i32::MIN
    }
    fn equal(a: Self, b: Self) -> bool {
        a == b
    }
}

/// C's `get_kfac`: a data scale of `limit`, reduced for a type too small
/// to hold twenty times it.
fn kfac<T: TestType>(limit: f64) -> f64 {
    (T::maxval() / 20.0).min(limit)
}

/// C's `is_bad`.
fn is_bad<T: TestType>(v: T) -> bool {
    v == T::bad()
}

/// The grids, mapping and options of one case (C's phase 0).
struct Setup {
    lbnd_in: Vec<i64>,
    ubnd_in: Vec<i64>,
    lbnd_out: Vec<i64>,
    ubnd_out: Vec<i64>,
    lbnd: Vec<i64>,
    ubnd: Vec<i64>,
    map: Box<dyn Mapping>,
    params: [f64; 2],
    tol: f64,
}

/// The input and output arrays of one case after rebinning.
struct Data<'a, T> {
    inp: &'a [T],
    in_var: &'a [T],
    out: &'a [T],
    out_var: &'a [T],
}

/// One of C's test functions, split into its three phases.
trait RebinTest {
    /// The limit passed to [`kfac`].
    const KLIMIT: f64;
    /// Phase 0: the grids, mapping and options for `spread`.
    fn setup(&self, spread: Spread) -> Setup;
    /// Phase 1: fill the input data and variance.
    fn fill<T: TestType>(&self, s: &Setup, kfac: f64, inp: &mut [T], in_var: &mut [T]);
    /// Phase 2: validate the output data and variance.
    fn check<T: TestType>(
        &self,
        s: &Setup,
        spread: Spread,
        kfac: f64,
        d: &Data<'_, T>,
    ) -> Result<(), String>;
}

fn nel(lbnd: &[i64], ubnd: &[i64]) -> usize {
    lbnd.iter()
        .zip(ubnd)
        .map(|(l, u)| (u - l + 1) as usize)
        .product()
}

/// C's `tester` for one type: run `t` with every spreading scheme, with
/// `AST__USEBAD + AST__USEVAR`, `maxpix` 100 and `wlim` 0.
fn run_type<T: TestType, R: RebinTest>(t: &R) {
    for spread in SPREADS {
        let s = t.setup(spread);
        let kfac = kfac::<T>(R::KLIMIT);
        let nel_in = nel(&s.lbnd_in, &s.ubnd_in);
        let nel_out = nel(&s.lbnd_out, &s.ubnd_out);
        let mut inp = vec![T::ZERO; nel_in];
        let mut in_var = vec![T::ZERO; nel_in];
        t.fill(&s, kfac, &mut inp, &mut in_var);
        let mut out = vec![T::ZERO; nel_out];
        let mut out_var = vec![T::ZERO; nel_out];
        rebin(
            s.map.as_ref(),
            &RebinInput {
                lbnd: &s.lbnd_in,
                ubnd: &s.ubnd_in,
                region_lbnd: &s.lbnd,
                region_ubnd: &s.ubnd,
                data: &inp,
                var: Some(&in_var),
            },
            &mut RebinOutput {
                lbnd: &s.lbnd_out,
                ubnd: &s.ubnd_out,
                data: &mut out,
                var: Some(&mut out_var),
            },
            &RebinOptions {
                spread,
                params: &s.params,
                flags: RebinFlags {
                    use_bad: true,
                    use_var: true,
                },
                tol: s.tol,
                maxpix: 100,
                wlim: 0.0,
            },
            T::bad(),
        )
        .unwrap_or_else(|e| {
            panic!(
                "rebin failed for spread {spread:?}, type {}: {e:?}",
                T::NAME
            )
        });
        let d = Data {
            inp: &inp,
            in_var: &in_var,
            out: &out,
            out_var: &out_var,
        };
        if let Err(msg) = t.check(&s, spread, kfac, &d) {
            panic!("{msg} (spread {spread:?}, type {})", T::NAME);
        }
    }
}

/// C's `tester`: run `t` for the `D`, `F` and `I` types.
fn tester<R: RebinTest>(t: &R) {
    run_type::<f64, R>(t);
    run_type::<f32, R>(t);
    run_type::<i32, R>(t);
}

/// The sum of the good values of `out`.
fn good_sum<T: TestType>(out: &[T]) -> f64 {
    out.iter()
        .filter(|v| !is_bad(**v))
        .fold(0.0, |acc, v| acc + v.to_f64())
}

/// Whether `sum` matches `kt`: to `equald` for a floating type, and to
/// within `itol` for an integer type.
fn sum_matches<T: TestType>(sum: f64, kt: f64, itol: f64) -> bool {
    if T::FLOATING {
        equald(sum, kt)
    } else {
        (sum - kt).abs() < itol
    }
}

/// Fill `inp` and `in_var` with `k * kfac` and `k` for the 1-based element
/// number `k`.
fn fill_ramp<T: TestType>(kfac: f64, inp: &mut [T], in_var: &mut [T]) {
    for (k, (v, var)) in inp.iter_mut().zip(in_var.iter_mut()).enumerate() {
        let k1 = (k + 1) as f64;
        *v = T::from_f64(k1 * kfac);
        *var = T::from_f64(k1);
    }
}

/// Fill the 1-d `inp` and `in_var` with `i * kfac` and `i` for pixel `i`.
fn fill_pixel_ramp<T: TestType>(s: &Setup, kfac: f64, inp: &mut [T], in_var: &mut [T]) {
    for i in s.lbnd_in[0]..=s.ubnd_in[0] {
        let idx = (i - s.lbnd_in[0]) as usize;
        inp[idx] = T::from_f64(i as f64 * kfac);
        in_var[idx] = T::from_f64(i as f64);
    }
}

/// Check that output element `k` and its variance are zero; `label` is the
/// element named in a failure.
fn check_zero<T: TestType>(
    test: &str,
    label: i64,
    d: &Data<'_, T>,
    k: usize,
) -> Result<(), String> {
    let v = d.out[k].to_f64();
    if !equald(v, 0.0) {
        return Err(format!("{test} {label}: {v} != 0"));
    }
    let vv = d.out_var[k].to_f64();
    if !equald(vv, 0.0) {
        return Err(format!("{test} {label}: variance {vv} != 0"));
    }
    Ok(())
}

/// Check that output element `k` and its variance equal input element
/// `kin` and its variance; `label` is the element named in a failure.
fn check_copy<T: TestType>(
    test: &str,
    label: i64,
    d: &Data<'_, T>,
    k: usize,
    kin: usize,
) -> Result<(), String> {
    if !T::equal(d.out[k], d.inp[kin]) {
        return Err(format!(
            "{test} {label}: data {} != {}",
            d.out[k].to_f64(),
            d.inp[kin].to_f64()
        ));
    }
    if !T::equal(d.out_var[k], d.in_var[kin]) {
        return Err(format!(
            "{test} {label}: variance {} != {}",
            d.out_var[k].to_f64(),
            d.in_var[kin].to_f64()
        ));
    }
    Ok(())
}

/// Check that the only nonzero good output value is a single `kt`;
/// `test` is named in a failure, which reports 1-based element numbers
/// offset by `label0`.
fn check_single_spike<T: TestType>(
    test: &str,
    out: &[T],
    kt: f64,
    label0: i64,
) -> Result<(), String> {
    let mut nz = 0;
    for (k, &o) in out.iter().enumerate() {
        let v = o.to_f64();
        if v != 0.0 && !is_bad(o) {
            let label = k as i64 + label0;
            if nz == 0 {
                nz += 1;
                if v != kt {
                    return Err(format!("{test} {label}: {v} {kt}"));
                }
            } else {
                return Err(format!("{test} {label}: {v}"));
            }
        }
    }
    Ok(())
}

/// TEST 1: 1-d UnitMap.
struct Test1;

impl RebinTest for Test1 {
    const KLIMIT: f64 = 1000.0;
    fn setup(&self, spread: Spread) -> Setup {
        // C keeps a branch for AST__GAUSS although the schemes it runs do
        // not include it.
        let params = if spread == Spread::Gauss {
            [2.0, 2.0]
        } else {
            [2.0, 0.5]
        };
        Setup {
            lbnd_in: vec![10],
            ubnd_in: vec![19],
            lbnd_out: vec![12],
            ubnd_out: vec![20],
            lbnd: vec![11],
            ubnd: vec![17],
            map: Box::new(UnitMap::new(1)),
            params,
            tol: 0.1,
        }
    }
    fn fill<T: TestType>(&self, s: &Setup, kfac: f64, inp: &mut [T], in_var: &mut [T]) {
        fill_pixel_ramp(s, kfac, inp, in_var);
    }
    fn check<T: TestType>(
        &self,
        s: &Setup,
        spread: Spread,
        _kfac: f64,
        d: &Data<'_, T>,
    ) -> Result<(), String> {
        let (lo, ub, uo) = (s.lbnd_out[0], s.ubnd[0], s.ubnd_out[0]);
        for i in lo..=ub {
            let ignore = spread == Spread::Gauss && (i <= lo + 1 || i >= ub - 1);
            if !ignore {
                let oidx = (i - lo) as usize;
                let iidx = (i - s.lbnd_in[0]) as usize;
                check_copy("TEST1", i, d, oidx, iidx)?;
            }
        }
        for i in ub + 1..=uo {
            check_zero("TEST1", i, d, (i - lo) as usize)?;
        }
        Ok(())
    }
}

/// TEST 2: 2-d UnitMap.
struct Test2;

impl RebinTest for Test2 {
    const KLIMIT: f64 = 1000.0;
    fn setup(&self, _spread: Spread) -> Setup {
        Setup {
            lbnd_in: vec![-1, 3],
            ubnd_in: vec![2, 6],
            lbnd_out: vec![0, 2],
            ubnd_out: vec![3, 5],
            lbnd: vec![-1, 3],
            ubnd: vec![1, 6],
            map: Box::new(UnitMap::new(2)),
            params: [2.0, 2.0],
            tol: 0.0,
        }
    }
    fn fill<T: TestType>(&self, _s: &Setup, kfac: f64, inp: &mut [T], in_var: &mut [T]) {
        fill_ramp(kfac, inp, in_var);
    }
    fn check<T: TestType>(
        &self,
        _s: &Setup,
        _spread: Spread,
        _kfac: f64,
        d: &Data<'_, T>,
    ) -> Result<(), String> {
        for k in 0..d.out.len() {
            let k1 = k as i64 + 1;
            if k1 <= 4 || k1 % 4 == 0 || k1 % 4 == 3 {
                check_zero("TEST2", k1, d, k)?;
            } else {
                check_copy("TEST2", k1, d, k, k - 3)?;
            }
        }
        Ok(())
    }
}

/// TEST 3: 3-d UnitMap.
struct Test3;

impl RebinTest for Test3 {
    const KLIMIT: f64 = 1000.0;
    fn setup(&self, _spread: Spread) -> Setup {
        Setup {
            lbnd_in: vec![-1, 3, -1],
            ubnd_in: vec![2, 6, 1],
            lbnd_out: vec![0, 2, 0],
            ubnd_out: vec![3, 5, 2],
            lbnd: vec![-1, 3, -1],
            ubnd: vec![1, 6, 1],
            map: Box::new(UnitMap::new(3)),
            params: [2.0, 2.0],
            tol: 0.0,
        }
    }
    fn fill<T: TestType>(&self, _s: &Setup, kfac: f64, inp: &mut [T], in_var: &mut [T]) {
        fill_ramp(kfac, inp, in_var);
    }
    fn check<T: TestType>(
        &self,
        s: &Setup,
        _spread: Spread,
        _kfac: f64,
        d: &Data<'_, T>,
    ) -> Result<(), String> {
        let nxy = nel(&s.lbnd_out[..2], &s.ubnd_out[..2]);
        for k in 0..d.out.len() {
            let k1 = k as i64 + 1;
            let k2 = (k1 - 1) % 16 + 1;
            let l = s.lbnd_out[2] + (k / nxy) as i64;
            if k2 <= 4 || k2 % 4 == 0 || k2 % 4 == 3 || l == 2 {
                check_zero("TEST3", k1, d, k)?;
            } else {
                check_copy("TEST3", k1, d, k, k + 13)?;
            }
        }
        Ok(())
    }
}

/// TEST 4: 1-d ShiftMap by 3.
struct Test4;

impl RebinTest for Test4 {
    const KLIMIT: f64 = 1000.0;
    fn setup(&self, _spread: Spread) -> Setup {
        Setup {
            lbnd_in: vec![10],
            ubnd_in: vec![19],
            lbnd_out: vec![12],
            ubnd_out: vec![20],
            lbnd: vec![11],
            ubnd: vec![17],
            map: Box::new(ShiftMap::new(vec![3.0])),
            params: [2.0, 2.0],
            tol: 0.1,
        }
    }
    fn fill<T: TestType>(&self, s: &Setup, kfac: f64, inp: &mut [T], in_var: &mut [T]) {
        fill_pixel_ramp(s, kfac, inp, in_var);
    }
    fn check<T: TestType>(
        &self,
        s: &Setup,
        _spread: Spread,
        _kfac: f64,
        d: &Data<'_, T>,
    ) -> Result<(), String> {
        let lo = s.lbnd_out[0];
        for i in lo..=s.lbnd[0] + 2 {
            check_zero("TEST4", i, d, (i - lo) as usize)?;
        }
        for i in s.lbnd[0] + 3..=s.ubnd_out[0] {
            let oidx = (i - lo) as usize;
            let iidx = (i - 3 - s.lbnd_in[0]) as usize;
            check_copy("TEST4", i, d, oidx, iidx)?;
        }
        Ok(())
    }
}

/// TEST 5: 2-d ShiftMap by (3, -1).
struct Test5;

impl RebinTest for Test5 {
    const KLIMIT: f64 = 1000.0;
    fn setup(&self, _spread: Spread) -> Setup {
        Setup {
            lbnd_in: vec![-1, 3],
            ubnd_in: vec![2, 6],
            lbnd_out: vec![0, 2],
            ubnd_out: vec![3, 5],
            lbnd: vec![-1, 3],
            ubnd: vec![1, 6],
            map: Box::new(ShiftMap::new(vec![3.0, -1.0])),
            params: [2.0, 2.0],
            tol: 0.0,
        }
    }
    fn fill<T: TestType>(&self, _s: &Setup, kfac: f64, inp: &mut [T], in_var: &mut [T]) {
        fill_ramp(kfac, inp, in_var);
    }
    fn check<T: TestType>(
        &self,
        _s: &Setup,
        _spread: Spread,
        _kfac: f64,
        d: &Data<'_, T>,
    ) -> Result<(), String> {
        for k in 0..d.out.len() {
            let k1 = k as i64 + 1;
            if (k1 - 1) % 4 < 2 {
                check_zero("TEST5", k1, d, k)?;
            } else {
                check_copy("TEST5", k1, d, k, k - 2)?;
            }
        }
        Ok(())
    }
}

/// TEST 6: 3-d ShiftMap by (3, -1, 1).
struct Test6;

impl RebinTest for Test6 {
    const KLIMIT: f64 = 1000.0;
    fn setup(&self, _spread: Spread) -> Setup {
        Setup {
            lbnd_in: vec![-1, 3, -1],
            ubnd_in: vec![2, 6, 1],
            lbnd_out: vec![0, 2, 0],
            ubnd_out: vec![3, 5, 2],
            lbnd: vec![-1, 3, -1],
            ubnd: vec![1, 6, 1],
            map: Box::new(ShiftMap::new(vec![3.0, -1.0, 1.0])),
            params: [2.0, 2.0],
            tol: 0.0,
        }
    }
    fn fill<T: TestType>(&self, _s: &Setup, kfac: f64, inp: &mut [T], in_var: &mut [T]) {
        fill_ramp(kfac, inp, in_var);
    }
    fn check<T: TestType>(
        &self,
        _s: &Setup,
        _spread: Spread,
        _kfac: f64,
        d: &Data<'_, T>,
    ) -> Result<(), String> {
        for k in 0..d.out.len() {
            let k1 = k as i64 + 1;
            let k2 = (k1 - 1) % 16 + 1;
            if (k2 - 1) % 4 < 2 {
                check_zero("TEST6", k1, d, k)?;
            } else {
                check_copy("TEST6", k1, d, k, k - 2)?;
            }
        }
        Ok(())
    }
}

/// TEST 7: 1-d ShiftMap by 1.5 of a single value.
struct Test7;

impl RebinTest for Test7 {
    const KLIMIT: f64 = 1000.0;
    fn setup(&self, _spread: Spread) -> Setup {
        Setup {
            lbnd_in: vec![10],
            ubnd_in: vec![19],
            lbnd_out: vec![12],
            ubnd_out: vec![20],
            lbnd: vec![11],
            ubnd: vec![17],
            map: Box::new(ShiftMap::new(vec![1.5])),
            params: [2.0, 2.0],
            tol: 0.1,
        }
    }
    fn fill<T: TestType>(&self, s: &Setup, kfac: f64, inp: &mut [T], in_var: &mut [T]) {
        inp.fill(T::from_f64(0.0));
        in_var.fill(T::from_f64(kfac));
        inp[(14 - s.lbnd_in[0]) as usize] = T::from_f64(kfac);
    }
    fn check<T: TestType>(
        &self,
        s: &Setup,
        spread: Spread,
        kfac: f64,
        d: &Data<'_, T>,
    ) -> Result<(), String> {
        let sum = good_sum(d.out);
        let kt = kfac;
        if !sum_matches::<T>(sum, kt, 3.0) {
            return Err(format!("TEST7 Data sum is {sum} should be {kt}"));
        }
        let lo = s.lbnd_out[0];
        if spread == Spread::Nearest {
            check_single_spike("TEST7", d.out, kt, lo)
        } else {
            // Input pixel 14 is centered at 13.5 and lands at 15.0, the
            // boundary of output pixels 15 and 16, so the output is
            // symmetric about it.
            for i in 0..=3 {
                let idx1 = (15 - i - lo) as usize;
                let idx2 = (16 + i - lo) as usize;
                if !T::equal(d.out[idx1], d.out[idx2]) {
                    return Err(format!(
                        "TEST7 {} ({}) != {} ({})",
                        15 - i,
                        d.out[idx1].to_f64(),
                        16 + i,
                        d.out[idx2].to_f64()
                    ));
                }
            }
            Ok(())
        }
    }
}

/// TEST 8: 2-d ShiftMap by (0.5, -0.5) of a single value.
struct Test8;

impl RebinTest for Test8 {
    const KLIMIT: f64 = 10000.0;
    fn setup(&self, _spread: Spread) -> Setup {
        Setup {
            lbnd_in: vec![-2, 0],
            ubnd_in: vec![3, 5],
            lbnd_out: vec![-2, 0],
            ubnd_out: vec![3, 5],
            lbnd: vec![-2, 0],
            ubnd: vec![3, 5],
            map: Box::new(ShiftMap::new(vec![0.5, -0.5])),
            params: [2.0, 2.0],
            tol: 0.0,
        }
    }
    fn fill<T: TestType>(&self, _s: &Setup, kfac: f64, inp: &mut [T], in_var: &mut [T]) {
        inp.fill(T::from_f64(0.0));
        in_var.fill(T::from_f64(kfac));
        inp[20] = T::from_f64(kfac);
    }
    fn check<T: TestType>(
        &self,
        s: &Setup,
        spread: Spread,
        kfac: f64,
        d: &Data<'_, T>,
    ) -> Result<(), String> {
        let sum = good_sum(d.out);
        let kt = kfac;
        if !sum_matches::<T>(sum, kt, 5.0) {
            return Err(format!("TEST8 Data sum is {sum} should be {kt}"));
        }
        if spread == Spread::Nearest {
            return check_single_spike("TEST8", d.out, kt, 1);
        }
        // Input element 21 is pixel (0, 3), centered at (-0.5, 2.5), which
        // lands at (0, 2), a corner of output pixels (0..1, 2..3), so the
        // output is point-symmetric about it.
        let (lb, ub) = (&s.lbnd_out, &s.ubnd_out);
        let nx = (ub[0] - lb[0] + 1) as usize;
        let mut k = 0;
        for j in lb[1]..=ub[1] {
            for i in lb[0]..=ub[0] {
                let ii = 1 - i;
                let jj = 5 - j;
                if (lb[0]..=ub[0]).contains(&ii) && (lb[1]..=ub[1]).contains(&jj) {
                    let kk = (jj - lb[1]) as usize * nx + (ii - lb[0]) as usize;
                    if !T::equal(d.out[kk], d.out[k]) {
                        return Err(format!(
                            "TEST8 {} ({}) != {} ({})",
                            kk + 1,
                            d.out[kk].to_f64(),
                            k + 1,
                            d.out[k].to_f64()
                        ));
                    }
                }
                k += 1;
            }
        }
        Ok(())
    }
}

/// TEST 9: 3-d ShiftMap of a single value.
struct Test9;

impl RebinTest for Test9 {
    const KLIMIT: f64 = 10000.0;
    fn setup(&self, spread: Spread) -> Setup {
        let shifts = if spread == Spread::Nearest {
            vec![1.7, 2.1, -1.2]
        } else {
            vec![0.5, 0.0, -0.5]
        };
        Setup {
            lbnd_in: vec![0; 3],
            ubnd_in: vec![6; 3],
            lbnd_out: vec![0; 3],
            ubnd_out: vec![6; 3],
            lbnd: vec![0; 3],
            ubnd: vec![6; 3],
            map: Box::new(ShiftMap::new(shifts)),
            params: [2.0, 2.0],
            tol: 0.0,
        }
    }
    fn fill<T: TestType>(&self, _s: &Setup, kfac: f64, inp: &mut [T], in_var: &mut [T]) {
        for (k, (v, var)) in inp.iter_mut().zip(in_var.iter_mut()).enumerate() {
            *v = T::from_f64(0.0);
            *var = T::from_f64((k + 1) as f64);
        }
        inp[171] = T::from_f64(kfac);
    }
    fn check<T: TestType>(
        &self,
        s: &Setup,
        spread: Spread,
        kfac: f64,
        d: &Data<'_, T>,
    ) -> Result<(), String> {
        let sum = good_sum(d.out);
        let kt = kfac;
        if !sum_matches::<T>(sum, kt, 5.0) {
            return Err(format!("TEST9 Data sum is {sum} should be {kt}"));
        }
        if spread == Spread::Nearest {
            // Input element 172 is pixel (3, 3, 3), centered at
            // (2.5, 2.5, 2.5), which lands at (4.2, 4.6, 1.3), inside output
            // element 139, pixel (5, 5, 2).
            for (k, o) in d.out.iter().enumerate() {
                let v = o.to_f64();
                if k == 138 {
                    if !equald(v, kt) {
                        return Err(format!("TEST9 El. 139 is {v} should be {kt}"));
                    }
                } else if !equald(v, 0.0) {
                    return Err(format!("TEST9 El. {} is {v} should be zero", k + 1));
                }
            }
            return Ok(());
        }
        let (lb, ub) = (&s.lbnd_out, &s.ubnd_out);
        let mut g = [0.0; 3];
        let mut w = 0.0;
        let mut k = 0;
        for l in lb[2]..=ub[2] {
            for j in lb[1]..=ub[1] {
                for i in lb[0]..=ub[0] {
                    let v = d.out[k].to_f64();
                    g[0] += i as f64 * v;
                    g[1] += j as f64 * v;
                    g[2] += l as f64 * v;
                    w += v;
                    k += 1;
                }
            }
        }
        for (axis, (gi, want)) in ["X", "Y", "Z"].iter().zip(g.iter().zip([3.5, 3.0, 2.5])) {
            let mean = gi / w;
            if !equald(mean, want) {
                return Err(format!("TEST9 Mean {axis} is {mean} should be {want}"));
            }
        }
        Ok(())
    }
}

#[test]
fn test1_unitmap_1d() {
    tester(&Test1);
}

#[test]
fn test2_unitmap_2d() {
    tester(&Test2);
}

#[test]
fn test3_unitmap_3d() {
    tester(&Test3);
}

#[test]
fn test4_shiftmap_1d_integer_shift() {
    tester(&Test4);
}

#[test]
fn test5_shiftmap_2d_integer_shift() {
    tester(&Test5);
}

#[test]
fn test6_shiftmap_3d_integer_shift() {
    tester(&Test6);
}

#[test]
fn test7_shiftmap_1d_single_value() {
    tester(&Test7);
}

#[test]
fn test8_shiftmap_2d_single_value() {
    tester(&Test8);
}

#[test]
fn test9_shiftmap_3d_single_value() {
    tester(&Test9);
}
