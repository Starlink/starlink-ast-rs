# Plan: `astResample<X>`, `astRebin<X>` and `astRebinSeq<X>`

The three image regridding entry points of `mapping.c`, about 12,000 lines of C.
No subclass overrides them; `region.c` `Mask<X>` calls `astResample<X>`.

## C structure

| Part | C | Lines |
|---|---|---|
| Entry, validation, simplify | `Resample<X>` | 14206 |
| Adaptive subdivision, linear fit | `ResampleAdaptively` | 14471 |
| Blocks of at most `mxpix` output pixels | `ResampleWithBlocking` | 15992 |
| Positions of one block, dispatch by scheme and type | `ResampleSection` | 14920 |
| Nearest neighbour | `InterpolateNearest<X>` | 4933 |
| Linear | `InterpolateLinear<X>` | 3991 |
| Separable kernels, user kernel (`AST__UKERN1`) | `InterpolateKernel1<X>` | 2841 |
| Kernels | `Sinc`, `SincCos`, `SincGauss`, `SincSinc`, `Somb`, `SombCos`, `Gauss` | 16473 on |
| Block average | `InterpolateBlockAverage<X>` | 5560 |
| Flux conservation | `ConserveFlux<X>` | 960 |
| Rebin entry; RebinSeq entry | `Rebin<X>`, `RebinSeq<X>` | 9922, 12260 |
| Rebin drivers | `RebinAdaptively`, `RebinWithBlocking`, `RebinSection` | 10249, 12746, 10712 |
| Spreading | `SpreadNearest<X>`, `SpreadLinear<X>`, `SpreadKernel1<X>` | 19646, 18804, 17574 |

## Per-type behaviour that must carry over

- Resample covers D, F, L, UL, K, UK, I, UI, S, US, B, UB; L and K are both
  `i64` on LP64 platforms, so the port has ten types. Rebin and RebinSeq cover
  D, F, I, B, UB.
- Each type sums in its own floating type (`Xfloattype`): F, S, US, B and UB
  in `float`, the others in `double`. The port uses an associated type, not
  `f64` throughout, or float-type results will differ.
- Integer outputs are rounded half away from zero by `(Xtype)(val ± 0.5)`.
  C's cast is undefined out of range. Resample range-checks before every
  such cast but flux conservation's, so `PixelValue::from_f64` saturates
  there. Rebin's spreading functions mirror the conversion clang compiles,
  which wraps for `B` and `UB` (`RebinValue::from_c`): a kernel's negative
  lobes then add wrapped contributions to unsigned pixels that sum back to
  the right total, where saturating would bias it.
- Negative variances are bad only for signed types (`CHECK_FOR_NEGATIVE_VARIANCE`).
- C writes each of the Usebad x Usevar x Nobad cases as its own loop so the
  compiler can drop the unused tests. The port keeps one loop per case only if
  a benchmark shows the branches cost something; the arithmetic order is what
  must match.

## Stages

Stages 1 to 4 are done: `mappings::resample` has every scheme of `astResample<X>`, and `mappings::rebin` ports `astRebin<X>` and `astRebinSeq<X>`.

1. **Resample framework, nearest and linear.** The public `Mapping::resample`
   generic over a pixel-type trait; the validation and errors of
   `Resample<X>`; `ResampleAdaptively`, `ResampleWithBlocking` and
   `ResampleSection` with their 1-D, 2-D and n-D linear-fit paths (as
   `tran_grid.rs` keeps TranGrid's); `AST__NEAREST` and `AST__LINEAR` with
   `AST__USEBAD`, `AST__NOBAD` and variances. Oracle probe over every type.
   Ports `testresample.c` tests 1 (nearest and linear), 2, 3 and 5-9.
2. **Kernels.** `InterpolateKernel1<X>` with the Sinc family, Somb, SombCos
   and a user kernel; `AST__BLOCKAVE`; `AST__UINTERP` (a user interpolator,
   a Rust closure in place of the C function pointer); `AST__CONSERVEFLUX`.
   Ports the rest of tests 1 and 4.
3. **Rebin.** `Rebin<X>`, the Rebin drivers and the three spreading
   functions, `AST__GAUSS` included. Ports `testrebin.c`.
4. **RebinSeq.** Weights carried between calls, `nused`, and the flags
   `AST__REBININIT`, `AST__REBINEND`, `AST__GENVAR`, `AST__VARWGT`,
   `AST__DISVAR`, `AST__NONORM` and `AST__PARWGT`. Ports `testrebinseq.c`.
5. **Follow-ups.** Decide whether Region `Mask<X>`, which reproduces C's
   nearest-neighbour `astResample` by its own route
   (`regions/masking.rs`), should call `resample`; and benchmark RebinSeq
   against C, as SMURF's map-maker calls it in its inner loop.

Each stage lands with a C probe compared bit for bit, negative controls, and
its `ast_tester` port, as the Moc stages did.
