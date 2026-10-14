# testfitschan.c checks without a Rust counterpart

Every helper function in `reference/starlink-ast/ast_tester/testfitschan.c` has Rust coverage.
The checks inline in `main()` are mostly covered by the FitsChan oracle tests, above all `test_fitschan_cursor_oracle.rs`.
This file listed the ones that were not, by the C error codes, as found on 2026-10-13, and records where they went.

## Covered since 2026-10-14

Every inline check except 994-998 now has a Rust counterpart.

The checks listed here as not covered on 2026-10-13 are in `test_fitschan_c_inline.rs`: 7777-7778, 12-13, 14-19, 115-117 and 133-135, 313, 630-633, 650-655 and 660-665, 804-805, and 836.
Porting 662-663 exposed a writer gap, now fixed: the FITS-WCS writer tested an unsimplified pixel-to-X Mapping when choosing a `-X2P` algorithm, so an axis linear in AWAV and written as FREQ was refused where C writes `FREQ-A2F`.
C sets the trial System on the FrameSet, whose `RemapFrame` and `TidyNodes` simplify the joined Mapping (`frameset.c:11214`).

The checks listed as partly covered on 2026-10-13 are also there: 876, 880 and 882, 4-11, 500-534 (with 600-609, the spectral systems written beside them), 680-684, the DATE-OBS block's 650-659, 740-747, 808-809, and 810-821.

## No Rust equivalent

- **994-998**: Test of the read-only Ncard, Nkey, CardType and AllWarnings, and `astGetC(Class)` (C 2059-2076).
  These go through C's string attribute path, which belongs to `ast-capi`; they are tier-3 checks.
