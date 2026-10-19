# Future work: a vectorized Rust rewrite of the ERFA, PAL and wcslib code the transforms use

Status: future ticket, on its own branch, after the Mapping, Frame, Region and FitsChan parity work is merged.
Nothing here is implemented; this records the investigation of 2026-10-18 so the ticket can start from it.
It extends `PLAN.md` §7's "wcslib Rust port (step 6a)" row to ERFA and PAL, and to bulk interfaces.

## Why

`ast-bench roman` (`crates/ast-bench/README.md`, record `docs/perf/2026-10-18-macos-aarch64-roman.txt`) puts SphMap level with C at large N: 18 million points per second forward and 51 inverse on both engines.
They are level because both run the same C for every point: the port calls `astPalDcc2s` and `astPalDcs2c` through FFI (`crates/ast-core/src/pal.rs`), which are ERFA's `eraC2s` (two `atan2` and a `sqrt`) and `eraS2c` (two `sin` and two `cos`).
Almost all of the time is in the platform libm, one value at a time.
[Starlink/ast pull request 51](https://github.com/Starlink/ast/pull/51) reports about 8x for SphMap from vector math functions (glibc's `libmvec` under `-ffast-math`), on x86-64 with AVX2.
The same holds for every projection in WcsMap and for SlaMap's and SkyFrame's PAL and ERFA calls: per-point calls into C, mostly into libm.

## What the investigation found

### Rust's `f64::atan2` is the platform's libm

`f64::atan2`, `sin` and `cos` in Rust's standard library call the platform's C math library (Apple's libm on macOS, glibc on Linux), the same functions C AST calls.
That is why a Rust port of ERFA code that keeps C's operations and order gives C's bits on the same machine, and why x86 and arm results differ today.
The `libm` crate (a pure-Rust port of musl) would give the same bits on every platform, but it is scalar and branchy and does not vectorize.

### A branchless `atan2` vectorizes and stays within 1 ULP of libm

A pure-Rust `atan2` (fdlibm's algorithm as in musl's `atan.c` and `atan2.c`), with each branch replaced by a select and inlined into the caller's loop, compiles to NEON vector instructions on aarch64 with no `unsafe` and no platform intrinsics: the function takes one value, and the compiler vectorizes the loop over the arrays.
On 1 048 576 of the Roman benchmark's sky points (macOS aarch64, M1 Ultra):

| | ns per call | Difference from libm |
|---|---:|---|
| libm `atan2` | 25.5 | |
| branchless `atan2` | 5.6 | at most 1 ULP over 5.2 million inputs (the sky points plus random pairs over 2^±40); 83% bit-identical |

It covers finite, non-zero `y` and `x`; zeros, infinities and NaN go to libm in a fix-up pass, as pull request 51's kernels do.
For SphMap forward (two `atan2` and a `sqrt` per point) that projects to about 15 ns per point, against 55 now and in C.
The code is in the appendix.

### A branchless `sin`/`cos` did not vectorize yet

The same treatment of `sin` and `cos` (fdlibm's three-part reduction by pi/2 from musl's `__rem_pio2.c`, done unconditionally, and the `__sin.c` and `__cos.c` kernels, with the quadrant picked by selects) is within 2 ULP of libm, the 2-ULP cases being results just below a power of two where the ULP halves.
It took 7.6 ns for a `sin` and `cos` pair against libm's 9.0, and the compiled loop holds no vector instructions: the conversion of the quadrant to an integer and the four-way select stop the vectorizer.
Picking the quadrant with floating-point arithmetic instead is the first thing to try.
Apple's libm already computes a `sin` and `cos` pair cheaply, so SphMap's inverse has less to gain than its forward.

## Proposed shape

Two steps, each gated on its own:

1. Port the ERFA, PAL and wcslib functions the transform paths call to Rust, with bulk interfaces (slices in, slices out, as a Mapping kernel holds its axes) over per-value bodies, keeping C's operations in C's order and calling `f64`'s libm functions.
   The output bits stay C's, so `ast-bench bits` and the transform corpus gate it unchanged, and `vendor/erfa`, `vendor/pal` and `vendor/wcslib` can then be deleted (design spec step 6a).
   wcslib's projection functions (`tanx2s` and the rest) already take strided arrays; what needs changing there is the per-element branching inside them.
2. Replace the libm calls inside those bodies with vectorizable ones like the `atan2` above.
   This moves results by up to 1 or 2 ULP, so it needs the decision below.

## Decisions for that ticket

- On by default, or opt-in (pull request 51 adds a per-Mapping `UseSIMD` attribute and keeps the scalar path)?
  On by default needs `ast-bench bits` and the C comparisons to accept a stated bound on these functions, and one re-baseline.
- The call-count parity tests (`ast-oracle-tests`) count PAL and ERFA calls by C symbol name; they need a new basis once those calls are no longer to C.
- Whether to take the `libm` crate's bits as the reference for the scalar path, giving the same results on every platform, rather than the platform's libm.
- The interface changes the bulk functions bring to callers outside the transform paths (SkyFrame, SpecFrame and TimeFrame conversions call PAL and ERFA one value at a time).

## Appendix: the branchless `atan2`

The constants are fdlibm's, from musl's `src/math/atan.c` and `atan2.c`.

```rust
const ATANHI: [f64; 4] = [
    4.63647609000806093515e-01, 7.85398163397448278999e-01,
    9.82793723247329054082e-01, 1.57079632679489655800e+00,
];
const ATANLO: [f64; 4] = [
    2.26987774529616870924e-17, 3.06161699786838301793e-17,
    1.39033110312309984516e-17, 6.12323399573676603587e-17,
];
const AT: [f64; 11] = [
    3.33333333333329318027e-01, -1.99999999998764832476e-01,
    1.42857142725034663711e-01, -1.11111104054623557880e-01,
    9.09088713343650656196e-02, -7.69187620504482999495e-02,
    6.66107313738753120669e-02, -5.83357013379057348645e-02,
    4.97687799461593236017e-02, -3.65315727442169155270e-02,
    1.62858201153657823623e-02,
];
const PI_LO: f64 = 1.2246467991473531772e-16;

/// atan(a) for a >= 0: fdlibm's reduction, chosen by selects.
#[inline(always)]
fn atan_pos(a: f64) -> f64 {
    let (c0, c1, c2, c3) = (a < 0.4375, a < 0.6875, a < 1.1875, a < 2.4375);
    let (num, den) = if c0 {
        (a, 1.0)
    } else if c1 {
        (2.0 * a - 1.0, 2.0 + a)
    } else if c2 {
        (a - 1.0, a + 1.0)
    } else if c3 {
        (a - 1.5, 1.0 + 1.5 * a)
    } else {
        (-1.0, a)
    };
    let x = num / den;
    let hi = if c1 { ATANHI[0] } else if c2 { ATANHI[1] } else if c3 { ATANHI[2] } else { ATANHI[3] };
    let lo = if c1 { ATANLO[0] } else if c2 { ATANLO[1] } else if c3 { ATANLO[2] } else { ATANLO[3] };
    let z = x * x;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));
    let reduced = hi - ((x * (s1 + s2) - lo) - x);
    let small = x - x * (s1 + s2);
    if c0 { small } else { reduced }
}

/// atan2 for finite, non-zero y and x; anything else is left to libm.
#[inline(always)]
fn atan2_v(y: f64, x: f64) -> f64 {
    let z = atan_pos((y / x).abs());
    let r = if x > 0.0 { z } else { std::f64::consts::PI - (z - PI_LO) };
    if y < 0.0 { -r } else { r }
}
```
