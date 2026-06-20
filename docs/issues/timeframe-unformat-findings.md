# TimeFrame Besselian `astUnformat` report — findings

This documents the outcome of investigating the reported discrepancy in
`astUnformat` of a Besselian-epoch string (e.g. `"B2001.5"`) on a
`system=jepoch, timescale=utc, timeorigin=2005` TimeFrame.

## Short version

**This is not a bug in `astUnformat`.** On a freshly constructed TimeFrame,
`astUnformat("B2001.5")` returns the correct value — it matches both AST's own
`astConvert` and astropy to full double precision, and `astFormat`/`astUnformat`
round-trips. The value reported as "wrong" only appears when the TimeFrame's
`TimeOrigin` has been silently shifted by an earlier attribute change, which is
the actual (and intended) behaviour worth understanding.

## What was reported

Unformatting `"B2001.5"` into a `system=jepoch, timescale=utc, timeorigin=2005`
TimeFrame was observed to return

```
-3.50131054408916
```

whereas the expected value (confirmed by astropy and by AST's own `astConvert`)
is

```
-3.5013115581077727
```

The difference, ~1.0e-6 Julian years ≈ 32 s, is the size of the TAI-UTC
leap-second offset at that epoch, which made it look as though the leap-second
correction was being dropped on the Besselian-epoch path.

## What we found

The value depends on the **state of the TimeFrame object**, specifically its
`TimeOrigin`, not on any defect in the unformat path.

Measured directly against the reference C library (CMake build):

| TimeFrame used                                  | `TimeOrigin` | `astUnformat("B2001.5")` |
| ----------------------------------------------- | ------------ | ------------------------ |
| Pristine, `timescale=utc, timeorigin=2005`      | 2005.0       | **-3.501311558107773**   |
| After a prior `astSet(timescale=...)` sequence  | 2004.99999899| -3.501310544089165       |

- The **pristine** frame's result, `-3.501311558107773`, matches:
  - AST's own `astConvert` from an `mjd/tt` frame to the
    `jepoch/utc/origin=2005` frame, and
  - astropy:
    `Time(52091.8966962715, format="mjd", scale="tt").utc.jyear - 2005.0`
    `== -3.5013115581077727`.
- It also round-trips: `astFormat` of the unformatted value reproduces the
  Besselian instant.

The reported value, `-3.501310544089165`, is exactly what you get from a frame
whose `TimeOrigin` is `2004.99999899` rather than `2005.0` — and the difference
between those two origins is precisely the ~32 s leap-second offset. So the
"missing leap second" is not missing from the conversion; it has moved into the
frame's stored origin.

## Why the origin shifts (and why that is correct)

`TimeOrigin` records an **absolute instant**, and its numeric value is expressed
in the frame's current system and timescale. Changing the timescale of a JEPOCH
frame therefore legitimately changes the numeric `TimeOrigin`, because the
Julian-epoch value of one fixed instant differs between, say, TT and UTC by the
leap-second offset.

This re-expression is done by `OriginScale` in `timeframe.c`, whose documented
purpose is "Convert the TimeOrigin in a TimeFrame to a new timescale … in order
to keep things consistent." So a sequence such as

```c
AstTimeFrame *tf = astTimeFrame("system=jepoch, timeorigin=2005.0");
/* ... other astSet calls ... */
astSet(tf, "timescale=utc");        /* TimeOrigin is now re-expressed in UTC */
```

leaves `tf` with a `TimeOrigin` that prints as `2004.99999899`, not `2005.0`.
`astUnformat` on that frame then correctly returns the value relative to *that*
origin. Both numbers are self-consistent; they simply correspond to two
different origins.

## How to reproduce the correct value

Construct the timescale on the frame from the outset (or set `timescale`
*before* `timeorigin`, so the origin is interpreted in the intended scale), and
compare against `astConvert`:

```c
/* Correct, pristine frame */
AstTimeFrame *bf = astTimeFrame("system=jepoch, timescale=utc, timeorigin=2005");
double v;
astUnformat(bf, 1, "B2001.5", &v);
/* v == -3.501311558107773  (matches astConvert and astropy) */

/* Oracle: convert the same instant (MJD 52091.8966962715 in the TT scale) */
AstTimeFrame *src = astTimeFrame("system=mjd, timescale=tt");
AstFrameSet  *fs  = astConvert(src, bf, " ");
double mjd_tt = 52091.8966962715, oracle;
astTran1(fs, 1, &mjd_tt, 1, &oracle);
/* oracle == -3.501311558107773  */
```

The pitfall to avoid when checking this: do not reuse a TimeFrame that earlier
test steps have mutated (especially via `astSet(timescale=...)`), because its
`TimeOrigin` will no longer be the value you originally supplied. Reproduce with
a pristine object.

## Conclusion / action

- No change has been made to `astUnformat` or the Besselian-epoch path; it is
  behaving correctly.
- If reporters are seeing the offset value, the cause is a TimeFrame whose
  `TimeOrigin` was re-expressed by a prior timescale change. Building the frame
  with the intended `timescale` in place gives the astropy/`astConvert` answer.
- The only thing arguably worth a documentation note upstream is making the
  `TimeOrigin` re-expression behaviour (via `OriginScale`) more discoverable,
  since it is easy to mistake for a conversion error. The numerics themselves
  are correct.
