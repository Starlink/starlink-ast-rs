# Native Serialization Round-Trip Gaps — Simplify-Fixture Corpus

> **HISTORICAL DOCUMENT (resolved).** The simplify-fixture corpus now passes
> 719/719 byte-exact (was 348/389 at the time of writing). Retained as a
> record; no outstanding work.

**Status (2026-05-19):** 348 / 389 fixtures pass byte-exact round-trip
(89%). 41 fixtures fail. Test now hard-fails on these — see
[commit 5cc131c](../../#5cc131c).

## Why this matters

CLAUDE.md commits the port to byte-exact native serialization with
the C AST library. The simplify-fixture corpus
(`crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/`,
~239 distinct fixtures × {`.map`, `.simp`} = ~478 files, ~389 of which
are mapping fixtures the test exercises) is the authoritative ground
truth for that contract. Until these round-trip cleanly, the per-class
bench oracle is blocked — see [Prerequisite](#prerequisite-for-the-per-class-bench-oracle).

## Test infrastructure

- **Authoritative**:
  `test_serialisation::native_byte_exact_round_trip_simplify_fixtures`.
  Reads every `.map`/`.simp`, parses, re-serializes, diffs byte-for-byte.
  Now **hard-fails** with the full list.
- **Supplementary**: `test_native_roundtrip_all`. Rust-side
  double-round-trip with deliberately non-default attribute values.
  Catches "silently drop non-default attribute" regressions the corpus
  wouldn't see (e.g. UnitNormMap centre, PolyMap iter-inverse settings).
- **Per-class bench oracle** (downstream consumer):
  `examples/bench_per_class.rs` + `tests/bench_per_class_c.c`. Generates
  a `.ast` file per mapping class, hands it to the C library via
  `astFromString`, benches Rust vs C side by side. This was the
  workstream that surfaced these gaps — when Rust's serialised output
  diverged from what C expected, the C side either parsed it wrong
  or crashed.

## Failures grouped by root cause

### A. Comment-string drift (cosmetic, but byte-exact must match)

The argument-comment strings emitted alongside conversion-step
arguments don't match the C library's text. Many of these are in
TimeMap / SlaMap / SpecMap conversion chains, which encode argument
semantics in comments rather than separate attributes.

| Class | Symptom | Fixture example |
|---|---|---|
| TimeMap | `Time1a` comment: "Zero point for input" / "UTC offset" / "Input MJD offset" vs C's "MJD offset" | `neg_time_2arg_mismatch.map:6`, `time_2arg_same_cancel.map:6`, `time_2arg_swapped_cancel.map:6`, `time_noop_eliminate.map:6` |
| TimeMap | `Time1e` comment: "DTAI" vs C's "Distance from earth spin axis" | `neg_time_5arg_mismatch.map:10`, `time_5arg_cancel.map:10` |
| SlaMap | `Sla1` direction-string reversed: "J2000.0 FK5 to ICRS" vs C's "ICRS to J2000.0 FK5" | `sla_icrs_cancel.map:5` |
| SlaMap | `Sla1a` comment for Besselian epoch / TDB | `sla_fk45_cancel.map:6`, `sla_helioecl_cancel.map:6` |
| SlaMap | `Sla2b` "required apparent place" vs C's "apparent place" | `neg_sla_2arg_mismatch.map:10`, `sla_geocentric_cancel.map:10` |
| MathMap | `SimpFI` boolean comment polarity wrong | `neg_math_no_simpfi.map:11` |
| WcsMap | `Type = " "` comment: "Unknown projection" vs C's "&lt;null&gt; projection" | `wcsmap_bad_to_unit.map:4` |
| Box region | `Begin Box` class comment: "Box region" vs C's "Axis intervals" | `box_parallel_merge.map:6`, `box_parallel_merge.simp:1` |
| Interval region | Same class-comment issue when interval simplifies to Box | `interval_parallel_merge.map:7`, `interval_parallel_merge.simp:1` |

**Fix shape**: per-class table mapping argument index → C comment string.
Mostly mechanical, no behaviour change. Where comments differ on
direction (e.g. SlaMap `Sla1` "X to Y" vs "Y to X"), the comment must
reflect the actual conversion direction stored in the term.

### B. Default-value attribute handling

Rust elides defaults differently from C. C emits some defaults
uncommented; Rust either omits them entirely or comments them out
with `#`. Both interpretations are valid serializations of the same
state, but byte-exact requires we match C's choice per attribute.

| Class | Symptom | Fixture example |
|---|---|---|
| SphMap | Default `PlrLg = 0` emitted as `#   PlrLg = 0` (commented) — C emits uncommented | `sph_inverse_cancel.map:11`, `neg_sph_polarlong_mismatch.map:11`, `sph_matrix_sandwich.map:10` |
| GrismMap | Default `GrmNR = 1` / `GrmNRP = 0` commented in Rust, uncommented in C | `neg_grism_different_attrs.map:9`, `grism_zoom_inv_merge.simp:7` |
| ZoomMap | `Zoom = 1` (default factor) omitted entirely in Rust; C emits it | `zoom_parallel_all_unit.map:14`, `pcd_swap_zoom_simplifies.map:16`, `pcd_zoom_swap_cancel.map:21` |
| GrismMap | First post-`IsA Mapping` attribute differs — Rust drops `GrmNR = 1.5` line | `grism_inverse_cancel.map:7`, `grism_zoom_merge.map:6` |

**Fix shape**: each attribute's "is this a default that I should emit"
predicate needs to match the C library's behaviour exactly. The C library
distinguishes "default value, emit uncommented" from "default value, emit
commented" from "default value, omit" per attribute. Rust currently makes
a different choice on a handful of attributes.

### C. Attribute-name / -form representation

Rust and C use different attribute *names* for the same state.

| Class | Symptom | Fixture example |
|---|---|---|
| MatrixMap | Rust emits per-element `M0 = 1` lines; C emits a single `Form = "Unit"` and skips the elements | `matrix_unit_to_unit.map:4` |
| LutMap | Rust emits `#   Incr = 1` (commented); C emits `#   Start = 0` (different default attribute name in the same slot) | `lut_inverse_cancel.map:10`, `lut_linear_to_win.map:5` |
| MatrixMap | Differing matrix values: `M1 = 3` vs C's `M1 = 0` — actually a *behaviour* bug, not cosmetic — element packing differs | `matrix_full_to_diagonal.map:5` |

**Fix shape**: probably the most invasive category. MatrixMap's
`Form` attribute encodes whether the matrix is `"Full"`, `"Diagonal"`,
or `"Unit"` and is mutually exclusive with emitting the matrix
elements. LutMap's `Start` vs `Incr` similarly encodes the table's
parameterisation differently. Will need to read the C library's
`PolyMap.c` / `LutMap.c` / `MatrixMap.c` to confirm the canonical
form choice.

### D. Attribute ordering

Rust emits attributes in a different order than C, breaking byte-exact
even when the *set* of attributes is correct.

| Class | Symptom | Fixture example |
|---|---|---|
| RateMap | `Inv = 0` emitted before `IsA Mapping`; C has `IsA Mapping` first | `ratemap_inverse_cancel.map:8` |
| RateMap | Inner mapping direction inversion: Rust emits `Fwd = 0`, C emits `Inv = 0` | `neg_ratemap_different_inner.map:25` |

**Fix shape**: Each Mapping's `to_node` orders its `push_*` calls. The
C library uses a specific order (typically: class-mandatory header,
then class-specific attributes, then `IsA Mapping` separator, then
inherited Mapping attributes). Rust needs to match per class.

### E. `IsSimp` flag in `.simp` files

The "this object is simplified" flag is dropped on some `.simp`
outputs.

| Class | Symptom | Fixture example |
|---|---|---|
| GrismMap | Missing `IsSimp = 1` line in simplified output | `grism_zoom_merge.simp:2` |

**Fix shape**: `Mapping::set_do_not_simplify` is the in-memory flag.
Its serialization (`IsSimp = 1`) should be emitted for any object
the user has called `simplify()` on. The corpus's `.simp` files all
have it; Rust appears to drop it for GrismMap specifically.

### F. SkyFrame default Title

`NormMap` round-trips don't preserve the inner frame's default title.
Rust outputs `"FK5 equatorial coordinates; mean equinox J2000.0"` where
C has `"ICRS coordinates"`. Affects every NormMap fixture that ships a
default SkyFrame inside.

| Class | Symptom | Fixture example |
|---|---|---|
| NormMap | Inner SkyFrame Title default | `normmap_inverse_cancel.map:12`, `normmap_inverse_cancel_upper.map:11`, `normmap_duplicate_elim.{map,simp}:10/7`, `neg_normmap_parallel.map:11`, `neg_normmap_nonnorm_neighbour.map:10`, `neg_normmap_different_frames.map:32` |

**Fix shape**: probably the SkyFrame `Title` derivation logic differs
between Rust and C when System is unset — Rust defaults to FK5 with
J2000 equinox, C defaults to ICRS. This may be a Frame-side bug rather
than a serializer-side bug.

## Per-class failure tally

| Class | Failures | Categories |
|---|---:|---|
| NormMap | 7 | F |
| TimeMap | 6 | A |
| SlaMap | 5 | A |
| GrismMap | 5 | A, B, E |
| SphMap | 3 | B |
| RateMap | 2 | D |
| PcdMap | 2 | B (via inner ZoomMap) |
| MatrixMap | 2 | C |
| LutMap | 2 | C |
| Interval region | 2 | A |
| Box region | 2 | A |
| ZoomMap | 1 | B |
| WcsMap | 1 | A |
| MathMap | 1 | A |

## Prerequisite for the per-class bench oracle

The `examples/bench_per_class.rs` harness (introduced
[commit 4337459](../../#4337459) and extended in
[commit 8366faf](../../#8366faf)) currently covers 20 mapping
variants out of 26+ public mapping classes. The four omitted classes:

- **ChebyMap**: Rust serialization missing bounds / inverse-poly
  section — C's `astFromString` crashes.
- **UnitNormMap**: `centre` attribute elided when default (zero); C
  has no default and crashes.
- **RateMap**: `ax1`/`ax2` axis selectors not serialised at all; C
  can't reconstruct which Jacobian element to compute.
- **MathMap**: not in the `AnyObject` enum, so `to_native_string`
  can't reach it.

These four (plus the 41 fixture corpus failures above) are the
serialization punch list that blocks "every mapping benched against
C oracle". Once they round-trip, the bench scaffold can grow variants
for them and produce a complete per-class report.

## Suggested fix order for next session

1. **Category A (cosmetic comments)** — 22 failures, mechanical fixes.
   Look up the C library's comment table in `slamap.c`, `timemap.c`,
   `specmap.c`, `box.c`, `mathmap.c`, `wcsmap.c` and copy the strings
   verbatim. No behaviour change.
2. **Category B (default-emission)** — 9 failures. Per-attribute audit
   of "when is the default emitted commented vs uncommented vs
   omitted". This is the path of least resistance after (1).
3. **Category D (attribute ordering)** — 2 failures, both RateMap.
   Reorder `to_node` push calls.
4. **Category E (IsSimp)** — 1 failure. Check `GrismMap::to_node` for
   why it doesn't honour `do_not_simplify`.
5. **Category F (NormMap Title)** — 7 failures but probably one root
   fix in SkyFrame's default-Title derivation.
6. **Category C (Form / Start representation)** — 4 failures. Most
   invasive — touches MatrixMap and LutMap structurally.
7. **Bench-blockers** (ChebyMap, UnitNormMap, RateMap, MathMap
   listed above) — once the corpus passes, expand fixtures or write
   targeted tests to cover their serialization, then wire them into
   `bench_per_class`.

## Resumption checklist

When you come back to this:

- [ ] `cargo test -p ast-core --release --test test_serialisation native_byte_exact_round_trip_simplify_fixtures` — confirm baseline is still 41 failures (or recapture the punch list if other unrelated changes shifted it).
- [ ] Pick a category from the list above, work through it. The eprintln list above the assert names every failing fixture + the first differing line, so the punch list is concrete.
- [ ] After fixes: the test will print a smaller failure count or pass entirely. Each commit should reduce the count visibly.
- [ ] Last thing: re-run `bench_per_class` and update its omitted-variants list as ChebyMap / UnitNormMap / RateMap / MathMap become serialisable.

## Out of scope for this report

- C-library byte-exact for **frame** and **frameset** classes. Tracked
  in [`native-roundtrip-gaps.md`](native-roundtrip-gaps.md) which
  records 14 known frame-side failures.
- C-library byte-exact for **regions**. Two failures appear here
  (Box / Interval class comments) but the corpus test isn't a
  systematic regions audit.

## Related commits

- [`4337459`](../../#4337459): per-class bench scaffold (discovered
  the gaps)
- [`8366faf`](../../#8366faf): bench extended to 20 variants
- [`5cc131c`](../../#5cc131c): simplify-fixture test made hard-fail
- [`064b4d6`](../../#064b4d6): supplementary `test_native_roundtrip_all`
