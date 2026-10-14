# testfitschan.c checks without a Rust counterpart

Every helper function in `reference/starlink-ast/ast_tester/testfitschan.c` has Rust coverage.
The checks inline in `main()` are mostly covered by the FitsChan oracle tests, above all `test_fitschan_cursor_oracle.rs`.
This file lists the ones that are not, by the C error codes, as found on 2026-10-13.
Each entry says what C checks; porting one means adding that check to a Rust test, through an oracle case where the value is not obvious from the C source.

## Covered since 2026-10-14

The checks listed here as not covered on 2026-10-13 are in `test_fitschan_c_inline.rs`: 7777-7778, 12-13, 14-19, 115-117 and 133-135, 313, 630-633, 650-655 and 660-665, 804-805, and 836.
Porting 662-663 exposed a writer gap, now fixed: the FITS-WCS writer tested an unsimplified pixel-to-X Mapping when choosing a `-X2P` algorithm, so an axis linear in AWAV and written as FREQ was refused where C writes `FREQ-A2F`.
C sets the trial System on the FrameSet, whose `RemapFrame` and `TidyNodes` simplify the joined Mapping (`frameset.c:11214`).

## Partly covered

- **876, 880, 882**: Encoding set and read back for FITS-WCS, FITS-AIPS++ and FITS-CLASS (C 1916-1928), and `test_encoding` / `test_clean` true after a set (C 1947-1953).
- **4-11**: a read failing on `CRPIX2 = 'fred'` (a conversion error; the Rust cases fail on an undefined value), and RetainFits on CTYPE1 surviving a failed read with Clean=1 while CTYPE2 is removed (C 2182-2254).
- **510-534**: FrameSet System changed after a read and then written, for FK4-NO-E, Supergalactic and Ecliptic in FITS-WCS, FITS-PC, FITS-IRAF and FITS-AIPS++, with C's round trip at pixel offsets 50, 50.5, 51, 55 and 45 (C 157-191, 2724-2779).
- **603-605**: linear FITS-WCS writes giving `VOPT`, `ZOPT` and `BETA` (C 2789-2793).
- **680-684**: a SpecFrame with `s = 1.4204e9*exp(p*1e-4)` writes `FREQ-LOG` and reads back to 1e-6 relative at p=512 (C 3100-3133); only the plain-Frame `-LOG` write is covered.
- **650-659** (DATE-OBS block): a 2-D FK5 header writes OBSGEO-X/Y/Z of -5464586.59, -2492996.56 and 2150654.38 to within 1 m (C 3284-3299).
- **740-747**: `get_fits_l` of the strings `T`, `Y`, `F`, `N`, `.FALSE.` and `0` (C 3530-3569).
- **808-809**: PutFits with overwrite of KINT, COMPLEXI, COMPLEXF, LOGICAL, UNDEF, CONTINUE and COMMENT cards, and KTEST overwritten with 8888888888 and read back (C 3835-3889).
- **810-829**: TIMESYS `UTC`, `IAT`, `TDT` and `TCG` read (C 4142, 4171).

## No Rust equivalent

- **994-998**: Test of the read-only Ncard, Nkey, CardType and AllWarnings, and `astGetC(Class)` (C 2059-2076).
  These go through C's string attribute path, which belongs to `ast-capi`; they are tier-3 checks.
