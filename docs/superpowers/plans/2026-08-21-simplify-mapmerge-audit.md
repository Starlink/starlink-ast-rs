# Simplify / `MapMerge` Gap Closure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close every known simplify / `MapMerge` gap in the Rust port, against a reference corpus proven sound, with a per-class audit a test enforces so the gaps cannot silently reopen.

**Architecture:** Three workstreams in strict order. W3 regenerates the reference corpora that the pinned C no longer reproduces, so every later measurement is scored against a sound oracle. W1 replaces the prose audit with per-class checklists of C's own `MapMerge` / `Simplify` steps, enforced by a test that fails on a missing checklist or an unverdicted row. W2 fixes the six defect causes and the four `IsSimp` under-tagging cases as outputs of that audit.

**Tech Stack:** Rust 2024 (`ast-core`), C99 (`reference/starlink-ast` at pin `f6762005`, built with cmake), Python 3 and POSIX sh for corpus tooling.

**Spec:** [`docs/superpowers/specs/2026-08-21-simplify-mapmerge-audit-design.md`](../specs/2026-08-21-simplify-mapmerge-audit-design.md)

## Global Constraints

- Never modify a `.simp` or `.native` reference by hand. Every regenerated reference comes from the pinned C oracle.
- The oracle is `reference/starlink-ast` at pin `f6762005`, built with cmake. Never reach into it from `ast-core`'s `build.rs`.
- When replaying a `wcsconv` manifest row through `wcsconverter`, **always pass the row's `attrs` field**. Omitting it manufactures structural differences that do not exist.
- `cargo fmt` is enforced; `cargo clippy --workspace -- -D warnings` is a CI gate. Every task ends green on both.
- Prose in Markdown is one sentence per line. American English.
- Comments describe the code as it is today. No references to plans, history, or previous mistakes.
- No heuristic, tolerance, or iteration count that has no analogue in the C source. If the C rule is not identified, stop and read C — do not infer it from a fixture.
- Never loosen a negative-control fixture to make a positive case pass.
- Every relaxation is per case, named in source, justified in a comment, and counted in `PLAN.md` §6 and `crates/ast-core/tests/test_verification_ledger.rs`.
- Commit with `git commit --fixup` when amending earlier work in this plan.

## Reference data (used by several tasks)

**The 17 card-only stale `.native` cases** — all upstream-tracked:
`serpens`, `zpx`, `degen-wcsaxes`, `fits-pc-alt`, `cube-reorder`, `fits-pc-unit`, `multi-wcs-spec`, `zero-cdelt`, `alt-spectral`, `crota-3d`, `axes-swap`, `cd-cross`, `class-noproj`, `class-arc`, `class-sin`, `skyspec3d`, `vopt-f2w`.

**The 6 mixed cases** — all upstream-tracked:
`scp`, `tnx`, `tnx-cheb`, `ssysobs`, `skyspec3d-class`, `wave-f2w`.

**The 9 platform-only cases** — not stale, do not regenerate:
`timj`, `multi-wcs`, `skyref`, `iraf-longpole`, `skyref-alt`, `tidy-offsets`, `tidy-pole`, `offset-sref`, `azel`.

**The audit universe** — 24 modules in `crates/ast-core/src/mappings/`:
`chebymap`, `cmpmap`, `dssmap`, `grismmap`, `lutmap`, `matrix`, `normmap`, `pcdmap`, `perm`, `polymap`, `ratemap`, `selectormap`, `shift`, `slamap`, `specmap`, `sphmap`, `splinemap`, `switchmap`, `timemap`, `tranmap`, `unit`, `unitnormmap`, `wcsmap`, `winmap`, `zoom`.
Plus 6 in `crates/ast-core/src/regions/`: `mod` (covering `Region::Simplify` and `GetDefUnc`), `circle`, `box_region`, `prism`, `interval`, `nullregion`, `pointlist`.

Helper modules excluded from the universe: `mod`, `simplify`, `helpers`, `matrix_merge`, `winmap_merge`, `linear_approx`, `map_box`, `polymap_simd`, `rate`.

---

## Task 1: Corpus freshness classifier

Builds the tool every later corpus task depends on. It classifies each `native` manifest row against the pinned C as reproduced, card-only, numeric, or structural.

**Files:**
- Create: `tools/oracle_fixtures/check_corpus_fresh.py`
- Create: `tools/oracle_fixtures/check_corpus_fresh.sh`
- Modify: `tools/oracle_fixtures/README.md`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `check_corpus_fresh.sh <oracle-build-dir>` exits 0 when no card-only or structural drift exists; exits 1 otherwise. Prints one line per drifting case as `<name> <verdict> <kwdiffs> <valdiffs>` where verdict is `CARD-ONLY`, `NUMERIC`, or `STRUCTURAL`. The Python module exposes `classify(fixtures_dir, wcsconverter, workdir) -> dict[str, tuple[str, int, int]]`.

- [ ] **Step 1: Write the classifier**

Create `tools/oracle_fixtures/check_corpus_fresh.py`:

```python
"""Classify every native wcsconv reference against the pinned C oracle.

A reference can differ from the oracle for two unrelated reasons, and they need
opposite responses. An `IsSimp` card difference means the reference predates a
library change and must be regenerated. A last-bits value difference means the
reference was captured on a different platform and must be left alone. This
tells them apart so neither gets mistaken for the other.
"""

import os
import pathlib
import subprocess
import sys

VERDICT_OK = "OK"
VERDICT_CARD = "CARD-ONLY"
VERDICT_NUMERIC = "NUMERIC"
VERDICT_STRUCTURAL = "STRUCTURAL"


def _rows(manifest):
    """Yield (name, input, expected, encoding, attrs) for each manifest row."""
    for line in manifest.read_text().splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        fields = [f.strip() for f in line.split("|")]
        if len(fields) < 4:
            continue
        attrs = fields[4] if len(fields) > 4 else ""
        yield fields[0], fields[1], fields[2], fields[3], attrs


def _keywords(text):
    """FITS keyword sequence, ignoring values and every IsSimp card."""
    out = []
    for line in text.splitlines():
        key = line[:8].strip()
        if not key or key.startswith("ISSIMP"):
            continue
        out.append(key)
    return out


def _without_issimp(text):
    return [l for l in text.splitlines() if not l[:8].strip().startswith("ISSIMP")]


def classify(fixtures_dir, wcsconverter, workdir):
    """Return {case_name: (verdict, keyword_diffs, value_diffs)}."""
    fixtures_dir = pathlib.Path(fixtures_dir)
    workdir = pathlib.Path(workdir)
    workdir.mkdir(parents=True, exist_ok=True)
    out_path = workdir / "corpus_check.out"
    results = {}

    for name, inp, exp, encoding, attrs in _rows(fixtures_dir / "wcsconv_tests.txt"):
        if encoding != "native":
            continue
        if out_path.exists():
            out_path.unlink()
        cmd = [str(wcsconverter), str(fixtures_dir / "inputs" / inp), "native", str(out_path)]
        if attrs:
            cmd.append(attrs)
        subprocess.run(cmd, capture_output=True)
        if not out_path.exists():
            results[name] = (VERDICT_STRUCTURAL, -1, -1)
            continue

        got = out_path.read_text()
        want = (fixtures_dir / "expected" / exp).read_text()
        if got == want:
            results[name] = (VERDICT_OK, 0, 0)
            continue

        got_kw, want_kw = _keywords(got), _keywords(want)
        got_lines, want_lines = _without_issimp(got), _without_issimp(want)
        value_diffs = sum(1 for a, b in zip(got_lines, want_lines) if a != b)
        value_diffs += abs(len(got_lines) - len(want_lines))

        if got_kw != want_kw:
            keyword_diffs = sum(1 for a, b in zip(got_kw, want_kw) if a != b)
            keyword_diffs += abs(len(got_kw) - len(want_kw))
            results[name] = (VERDICT_STRUCTURAL, keyword_diffs, value_diffs)
        elif value_diffs:
            results[name] = (VERDICT_NUMERIC, 0, value_diffs)
        else:
            results[name] = (VERDICT_CARD, 0, 0)

    return results


def main(argv):
    if len(argv) != 4:
        print("usage: check_corpus_fresh.py <fixtures_dir> <wcsconverter> <workdir>",
              file=sys.stderr)
        return 2
    results = classify(argv[1], argv[2], argv[3])
    bad = 0
    for name in sorted(results):
        verdict, kw, val = results[name]
        if verdict == VERDICT_OK:
            continue
        print(f"{name} {verdict} {kw} {val}")
        if verdict in (VERDICT_CARD, VERDICT_STRUCTURAL):
            bad += 1
    counts = {}
    for verdict, _, _ in results.values():
        counts[verdict] = counts.get(verdict, 0) + 1
    print(f"reproduced={counts.get(VERDICT_OK, 0)} "
          f"card-only={counts.get(VERDICT_CARD, 0)} "
          f"numeric={counts.get(VERDICT_NUMERIC, 0)} "
          f"structural={counts.get(VERDICT_STRUCTURAL, 0)}", file=sys.stderr)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
```

- [ ] **Step 2: Write the wrapper that builds the oracle**

Create `tools/oracle_fixtures/check_corpus_fresh.sh`:

```sh
#!/bin/sh
# Check every native wcsconv reference against the pinned C oracle.
#
# Card-only drift means a reference predates a library change and must be
# regenerated; it exits non-zero. Numeric drift means the reference was
# captured on another platform and is reported but tolerated. Structural drift
# should never happen and exits non-zero.
#
#   ./tools/oracle_fixtures/check_corpus_fresh.sh [build-dir]
set -e
HERE=$(cd "$(dirname "$0")/../.." && pwd)
DEST=${1:-${TMPDIR:-/tmp}/ast-corpus-oracle}
SRC=$HERE/reference/starlink-ast
[ -d "$SRC/src" ] || { echo "reference submodule not initialised"; exit 1; }

if [ ! -x "$DEST/wcsconverter" ]; then
   rm -rf "$DEST"; mkdir -p "$DEST"
   ( cd "$SRC" && git ls-files ) > "$DEST/.files"
   ( cd "$SRC" && tar cf - -T "$DEST/.files" ) | ( cd "$DEST" && tar xf - )
   cmake -S "$DEST" -B "$DEST/build" > "$DEST/cmake.log" 2>&1
   cmake --build "$DEST/build" --target ast -j8 > "$DEST/build.log" 2>&1
   YAML=$(ls -d /opt/homebrew/opt/libyaml/lib /usr/local/opt/libyaml/lib 2>/dev/null | head -1)
   cc "$DEST/ast_tester/wcsconverter.c" -o "$DEST/wcsconverter" \
      -I "$DEST/build" -L "$DEST/build" ${YAML:+-L "$YAML"} \
      -Wl,-rpath,"$DEST/build" ${YAML:+-Wl,-rpath,"$YAML"} -last -lyaml -lm
fi

exec python3 "$HERE/tools/oracle_fixtures/check_corpus_fresh.py" \
   "$HERE/crates/ast-core/tests/fixtures/wcsconv" "$DEST/wcsconverter" "$DEST/work"
```

- [ ] **Step 3: Run it and verify it reproduces the known classification**

Run: `chmod +x tools/oracle_fixtures/check_corpus_fresh.sh && ./tools/oracle_fixtures/check_corpus_fresh.sh`

Expected: exit status 1, and the summary line on stderr reads `reproduced=75 card-only=17 numeric=15 structural=0`.
If `structural` is not 0, stop: either the classifier is dropping the `attrs` field or something genuinely changed upstream. Do not proceed.

- [ ] **Step 4: Document it**

Append to `tools/oracle_fixtures/README.md`:

```markdown
## `check_corpus_fresh.sh` — is the native corpus still what C produces?

Moving the `reference/starlink-ast` pin can invalidate committed references
without anything in this repo changing, and upstream will not notice because
their suite compares with `astEqual` rather than byte for byte.

```shell
./tools/oracle_fixtures/check_corpus_fresh.sh
```

Each drifting case prints as `<name> <verdict> <keyword-diffs> <value-diffs>`:

- `CARD-ONLY` — differs only in `IsSimp` cards. The reference predates a
  library change and must be regenerated. Exits non-zero.
- `NUMERIC` — identical keywords, last-bits value differences. The reference
  was captured on another platform. Reported, tolerated.
- `STRUCTURAL` — the keyword sequence itself differs. Should never happen from
  an `IsSimp` or KeyMap change; investigate before regenerating anything.

Run this whenever the submodule pin moves. It belongs in the upstream-sync
protocol in `CLAUDE.md`.
```

- [ ] **Step 5: Commit**

```bash
git add tools/oracle_fixtures/check_corpus_fresh.py tools/oracle_fixtures/check_corpus_fresh.sh tools/oracle_fixtures/README.md
git commit -m "tools: classify native reference drift as card-only, numeric or structural

Moving the submodule pin invalidated two reference corpora without anything in
this repo changing. This tells staleness apart from platform arithmetic so
neither is mistaken for the other."
```

---

## Task 2: Regenerate the 17 card-only references

**Files:**
- Modify: `crates/ast-core/tests/fixtures/wcsconv/expected/{serpens,zpx,degen-wcsaxes,fits-pc-alt,cube-reorder,fits-pc-unit,multi-wcs-spec,zero-cdelt,alt-spectral,crota-3d,axes-swap,cd-cross,class-noproj,class-arc,class-sin,skyspec3d,vopt-f2w}.native`
- Modify: the same 17 filenames under `reference/starlink-ast/ast_tester/`

**Interfaces:**
- Consumes: `check_corpus_fresh.sh` from Task 1.
- Produces: 17 regenerated references. After this task `check_corpus_fresh.sh` reports `card-only=6` (only the mixed cases remain).

- [ ] **Step 1: Regenerate into both trees**

```bash
D=${TMPDIR:-/tmp}/ast-corpus-oracle
F=crates/ast-core/tests/fixtures/wcsconv
U=reference/starlink-ast/ast_tester
for b in serpens zpx degen-wcsaxes fits-pc-alt cube-reorder fits-pc-unit \
         multi-wcs-spec zero-cdelt alt-spectral crota-3d axes-swap cd-cross \
         class-noproj class-arc class-sin skyspec3d vopt-f2w; do
  attrs=$(awk -F'|' -v n="${b}_native" '{gsub(/ /,"",$1)} $1==n {gsub(/^ +| +$/,"",$5); print $5}' $F/wcsconv_tests.txt)
  if [ -n "$attrs" ]; then $D/wcsconverter $F/inputs/$b.head native $F/expected/$b.native "$attrs"
  else $D/wcsconverter $F/inputs/$b.head native $F/expected/$b.native; fi
  cp $F/expected/$b.native $U/$b.native
  echo "regenerated $b"
done
```

- [ ] **Step 2: Verify only `ISSIMP` lines changed**

```bash
git diff --stat crates/ast-core/tests/fixtures/wcsconv/expected/
git diff -U0 crates/ast-core/tests/fixtures/wcsconv/expected/ | grep '^[+-]' | grep -v '^[+-][+-]' | grep -vc ISSIMP
```

Expected: the second command prints `0`.
If it prints anything else, a file in this set is not card-only. Stop and re-run Task 1's classifier — the set is wrong.

- [ ] **Step 3: Run the classifier again**

Run: `./tools/oracle_fixtures/check_corpus_fresh.sh`

Expected: summary reads `reproduced=92 card-only=6 numeric=9 structural=0`. Exit status is still 1, because the 6 mixed cases remain.

- [ ] **Step 4: Run the wcsconv suite**

Run: `cargo test -p ast-core --test test_wcsconv`

Expected: some cases now fail, because `ISSIMP_PLACEMENT_LICENSED` rows are pinned against the references just replaced. Record which. Task 4 deletes them; do not adjust them here.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core/tests/fixtures/wcsconv/expected reference/starlink-ast
git commit -m "test(wcsconv): regenerate the 17 card-only native references

The pinned C no longer reproduced them: upstream's per-orientation
astSetIsSimple change moved IsSimp placement, and these references predate it.
Differences are IsSimp cards only, so the capture platform does not matter.
Written into the submodule tree as well, for upstream."
```

---

## Task 3: The 6 mixed references, card hunks only

Their `IsSimp` staleness is real; their value differences are this platform's arithmetic. Keep the first, discard the second.

**Files:**
- Modify: `crates/ast-core/tests/fixtures/wcsconv/expected/{scp,tnx,tnx-cheb,ssysobs,skyspec3d-class,wave-f2w}.native`
- Modify: the same 6 under `reference/starlink-ast/ast_tester/`
- Create: `tools/oracle_fixtures/merge_card_hunks.py`

**Interfaces:**
- Consumes: `check_corpus_fresh.sh` from Task 1.
- Produces: 6 merged references. After this task `check_corpus_fresh.sh` reports `card-only=0 structural=0`.

- [ ] **Step 1: Write the merge tool**

Create `tools/oracle_fixtures/merge_card_hunks.py`:

```python
"""Take IsSimp cards from a freshly generated reference, values from the old one.

Some references are stale in their IsSimp cards and, separately, were captured
on a platform whose last bits differ from this one. The two never share a line,
so the correct reference is the committed file with only its IsSimp cards
replaced. This asserts that separation rather than assuming it.
"""

import pathlib
import sys


def _is_issimp(line):
    return line[:8].strip().startswith("ISSIMP")


def merge(old_text, new_text):
    """Old file's lines, with the new file's IsSimp cards spliced in.

    Both files are FITS card sequences that agree on every non-IsSimp keyword,
    so walking them together and taking IsSimp runs from the new side
    reconstructs what the new library would emit on the old platform.
    """
    old_lines = old_text.splitlines(keepends=True)
    new_lines = new_text.splitlines(keepends=True)
    out, i, j = [], 0, 0
    while i < len(old_lines) or j < len(new_lines):
        old_at_card = i < len(old_lines) and _is_issimp(old_lines[i])
        new_at_card = j < len(new_lines) and _is_issimp(new_lines[j])
        if new_at_card:
            out.append(new_lines[j]); j += 1
            if old_at_card:
                i += 1
            continue
        if old_at_card:
            i += 1
            continue
        if i < len(old_lines) and j < len(new_lines):
            out.append(old_lines[i]); i += 1; j += 1
            continue
        raise ValueError("files diverge outside IsSimp cards; not separable")
    return "".join(out)


def check(old_text, new_text, merged_text):
    """Both assertions from the spec. Raises ValueError if either fails."""
    strip = lambda t: [l for l in t.splitlines() if not _is_issimp(l)]
    if strip(merged_text) != strip(old_text):
        raise ValueError("merged file differs from the reference outside IsSimp cards")
    cards = lambda t: [l for l in t.splitlines() if _is_issimp(l)]
    if cards(merged_text) != cards(new_text):
        raise ValueError("merged file's IsSimp cards do not match the fresh output")


def main(argv):
    if len(argv) != 4:
        print("usage: merge_card_hunks.py <committed> <fresh> <out>", file=sys.stderr)
        return 2
    old = pathlib.Path(argv[1]).read_text()
    new = pathlib.Path(argv[2]).read_text()
    merged = merge(old, new)
    check(old, new, merged)
    pathlib.Path(argv[3]).write_text(merged)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
```

- [ ] **Step 2: Write a failing test for the merge logic**

Create `tools/oracle_fixtures/test_merge_card_hunks.py`:

```python
import merge_card_hunks as m

OLD = ("NIN_A   =                    2 / Number of input coordinates\n"
       "SFT2_A  =      -223.8380193875 / Shift for axis 2\n"
       "END\n")
NEW = ("NIN_A   =                    2 / Number of input coordinates\n"
       "ISSIMP_B=                    1 / Mapping has been simplified\n"
       "SFT2_A  =  -223.83801938750003 / Shift for axis 2\n"
       "END\n")


def test_takes_cards_from_new_and_values_from_old():
    merged = m.merge(OLD, NEW)
    assert "ISSIMP_B" in merged
    assert "-223.8380193875 " in merged
    assert "-223.83801938750003" not in merged
    m.check(OLD, NEW, merged)


def test_check_rejects_a_non_separable_pair():
    bad = NEW.replace("NIN_A   =                    2", "NIN_A   =                    3")
    try:
        m.check(OLD, bad, m.merge(OLD, bad))
    except ValueError:
        return
    raise AssertionError("expected ValueError for a non-separable pair")
```

- [ ] **Step 3: Run it to verify it fails**

Run: `cd tools/oracle_fixtures && python3 -m pytest test_merge_card_hunks.py -v`
Expected: FAIL — `merge_card_hunks` does not exist yet if Step 1 was skipped, otherwise both tests should already pass. If they pass, confirm Step 1's file matches exactly and move on.

- [ ] **Step 4: Run it to verify it passes**

Run: `cd tools/oracle_fixtures && python3 -m pytest test_merge_card_hunks.py -v`
Expected: PASS, 2 passed.

- [ ] **Step 5: Apply to the six references**

```bash
D=${TMPDIR:-/tmp}/ast-corpus-oracle
F=crates/ast-core/tests/fixtures/wcsconv
U=reference/starlink-ast/ast_tester
for b in scp tnx tnx-cheb ssysobs skyspec3d-class wave-f2w; do
  attrs=$(awk -F'|' -v n="${b}_native" '{gsub(/ /,"",$1)} $1==n {gsub(/^ +| +$/,"",$5); print $5}' $F/wcsconv_tests.txt)
  if [ -n "$attrs" ]; then $D/wcsconverter $F/inputs/$b.head native $D/fresh.native "$attrs"
  else $D/wcsconverter $F/inputs/$b.head native $D/fresh.native; fi
  python3 tools/oracle_fixtures/merge_card_hunks.py $F/expected/$b.native $D/fresh.native $D/merged.native
  mv $D/merged.native $F/expected/$b.native
  cp $F/expected/$b.native $U/$b.native
  echo "merged $b"
done
```

If `merge_card_hunks.py` raises for any file, that file is not separable. Stop and record it — it needs a genuine Linux capture and the spec's W3.2 assumption was wrong for it.

- [ ] **Step 6: Verify only `ISSIMP` lines changed**

```bash
git diff -U0 crates/ast-core/tests/fixtures/wcsconv/expected/ | grep '^[+-]' | grep -v '^[+-][+-]' | grep -vc ISSIMP
```

Expected: `0`.

- [ ] **Step 7: Confirm the corpus is clean**

Run: `./tools/oracle_fixtures/check_corpus_fresh.sh`

Expected: summary reads `reproduced=92 card-only=0 numeric=15 structural=0`, exit status 0.
The 15 numeric cases are the 9 platform-only ones plus the 6 just merged, which now differ from the oracle only in this platform's last bits — that is the intended end state.

- [ ] **Step 8: Commit**

```bash
git add crates/ast-core/tests/fixtures/wcsconv/expected reference/starlink-ast tools/oracle_fixtures
git commit -m "test(wcsconv): refresh the six mixed native references, cards only

These six are stale in their IsSimp cards and, separately, were captured on a
platform whose last bits differ from this one. The two never share a line, so
the reference is the committed file with only its IsSimp cards replaced.
merge_card_hunks.py asserts that separation rather than assuming it."
```

---

## Task 4: Retire the stale licences and document the platform captures

**Files:**
- Modify: `crates/ast-core/tests/test_wcsconv.rs:105-130` (the `ISSIMP_PLACEMENT_LICENSED` table)
- Modify: `crates/ast-core/tests/fixtures/wcsconv/wcsconv_tests.txt`
- Modify: `crates/ast-core/tests/test_verification_ledger.rs`
- Modify: `PLAN.md`
- Modify: `CLAUDE.md`

**Interfaces:**
- Consumes: the regenerated corpus from Tasks 2 and 3.
- Produces: an `ISSIMP_PLACEMENT_LICENSED` table containing only genuine FitsChan reader-order cases, and a ledger count matching it.

- [ ] **Step 1: Find which licence rows are now unnecessary**

Run: `cargo test -p ast-core --test test_wcsconv 2>&1 | grep -E "IsSimp card count"`

Every row whose reported `(got, want)` are now equal no longer needs a licence.
Delete those rows outright. Do not adjust their numbers — a licence pinned against a reference we just replaced has no meaning.

- [ ] **Step 2: Delete the retired rows and re-justify the survivors**

Edit the table so each surviving row carries a comment naming the C behaviour that causes it. Replace the block comment above `ISSIMP_PLACEMENT_LICENSED` with:

```rust
/// Cases where the port's `IsSimp` placement differs from C's in the FitsChan
/// read path, with the card count pinned on each side.
///
/// C's reader reduces Mappings as it assembles them, so its cards record the
/// order that reader worked in. This port simplifies once, at the end. The
/// resulting Mapping is the same; only which sub-Mappings were momentarily a
/// whole simplification result differs. That is an internal detail of C's
/// reader, not a port gap, and is the one place the design permits a residue.
///
/// Every row must name a C behaviour. A row may never cite a stale reference:
/// the corpus is checked against the pinned oracle by
/// `tools/oracle_fixtures/check_corpus_fresh.sh`.
```

- [ ] **Step 3: Document the nine platform-only cases in the manifest**

Add above the affected rows in `crates/ast-core/tests/fixtures/wcsconv/wcsconv_tests.txt`:

```
# timj, multi-wcs, skyref, iraf-longpole, skyref-alt, tidy-offsets, tidy-pole,
# offset-sref and azel were captured on Linux. Replaying them through the
# pinned C on macOS reproduces every keyword and every IsSimp card, and differs
# only in the last one or two digits of trig and matrix values -- for example
# 0.086291786736833131 against ...173. That is platform arithmetic, not
# staleness, so these references are correct and must not be regenerated.
# tools/oracle_fixtures/check_corpus_fresh.sh reports them as NUMERIC and
# tolerates them.
```

- [ ] **Step 4: Update the ledger and PLAN.md**

Set the `ISSIMP_PLACEMENT_LICENSED` entry count in `crates/ast-core/tests/test_verification_ledger.rs` to the number of surviving rows, and rewrite its reason string to match Step 2's comment.
Update the `wcsconv` row of `PLAN.md` §6 with the new count.
Replace `PLAN.md` §9.3's disposition column with what actually happened.

- [ ] **Step 5: Add the freshness check to the sync protocol**

In `CLAUDE.md`, under "Upstream sync protocol", add:

```markdown
  After moving the pin, run `tools/oracle_fixtures/check_corpus_fresh.sh`.
  Card-only drift means references predate the new pin and must be regenerated
  from the oracle; numeric drift means they were captured on another platform
  and must be left alone. Regenerate in the same commit range as the pin move.
```

- [ ] **Step 6: Run everything**

Run: `cargo test -p ast-core && cargo clippy -p ast-core --all-targets -- -D warnings`
Expected: all pass, no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/ast-core/tests PLAN.md CLAUDE.md crates/ast-core/tests/fixtures/wcsconv/wcsconv_tests.txt
git commit -m "test(wcsconv): retire licences pinned against replaced references

Rows licensed against references that Tasks 2 and 3 regenerated are deleted
rather than renumbered. Survivors each name the C behaviour that causes them.
The nine Linux-captured references are documented as platform arithmetic, and
the corpus freshness check joins the upstream-sync protocol."
```

---

## Task 5: The audit checklist format and its enforcement

Builds the mechanism with one worked example. ShiftMap is the example because its C body is short and this session already traced it end to end.

**Files:**
- Create: `docs/audits/mapmerge/README.md`
- Create: `docs/audits/mapmerge/shift.md`
- Create: `crates/ast-core/tests/test_mapmerge_audit.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: the checklist format, and `test_mapmerge_audit.rs` with two tests — `every_audited_class_has_a_checklist` and `every_checklist_row_has_a_verdict`. Later tasks add one `docs/audits/mapmerge/<module>.md` per module named in the audit universe.

- [ ] **Step 1: Write the format guide**

Create `docs/audits/mapmerge/README.md`:

```markdown
# `MapMerge` / `Simplify` audit checklists

One file per class, named for its Rust module: `shift.md` audits
`crates/ast-core/src/mappings/shift.rs`.

Each row is one of C's own comment-delimited steps in that class's `MapMerge`
or `Simplify` body. AST's C names its steps in comments, so the granularity is
C's, not ours — typically 5 to 20 rows per class, never line by line.

## Required front matter

```
C source: reference/starlink-ast/src/<file>.c
Rust: crates/ast-core/src/<dir>/<module>.rs
Audited: YYYY-MM-DD against pin <sha>
```

## The table

| column | meaning |
|---|---|
| `#` | row number, from 1 |
| `C step` | C's own wording for the step |
| `C site` | `file.c:start-end` |
| `Rust site` | `path.rs:start-end`, or `--` when the verdict is `n/a` |
| `verdict` | exactly one of the three below |

## Verdicts

- `ported` — the Rust code performs this step.
- `n/a: <reason>` — the step cannot apply. A reason is required and must say
  why, not that it is unnecessary. "The Rust type cannot be inverted in place"
  is a reason; "not needed" is not.
- `gap: PLAN.md <section>` — not ported. The named `PLAN.md` section must exist
  and must say what would expose the gap, not merely what is missing.

`test_mapmerge_audit.rs` enforces all of this. A class in the audit universe
with no checklist fails, and so does a row whose verdict is outside the set.
```

- [ ] **Step 2: Write the worked example**

Create `docs/audits/mapmerge/shift.md`:

```markdown
# `MapMerge` audit — ShiftMap

C source: reference/starlink-ast/src/shiftmap.c
Rust: crates/ast-core/src/mappings/shift.rs
Audited: 2026-08-21 against pin f6762005

| # | C step | C site | Rust site | verdict |
|---|---|---|---|---|
| 1 | Build an equivalent forward WinMap from the shift terms, folding in the applied inversion | shiftmap.c:671-700 | shift.rs:185-204 | ported |
| 2 | Replace the nominated ShiftMap with that WinMap and set its invert flag to zero | shiftmap.c:701-710 | shift.rs:202-203 | ported |
| 3 | Delegate to WinMap's MapMerge, which carries the full swap and merge logic | shiftmap.c:711-715 | shift.rs:205 | ported |
| 4 | Revert to a ShiftMap when the WinMap merge changed nothing, or merely produced an equivalent lone ShiftMap | shiftmap.c:716-720 | shift.rs:209-219 | ported |
| 5 | Revert with astClone forward, or astCopy plus negated shifts when inverted -- both carry the object's flags | shiftmap.c:721-741 | shift.rs:220-234 | ported |
| 6 | Report a change only when the original was applied inverted | shiftmap.c:742-745 | shift.rs:235-236 | ported |
```

- [ ] **Step 3: Write the failing test**

Create `crates/ast-core/tests/test_mapmerge_audit.rs`:

```rust
//! Enforce that every class in the audit universe has a `MapMerge` /
//! `Simplify` checklist, and that no row in one is left without a verdict.
//!
//! A prose audit cannot say whether it is finished. This one can: a class with
//! no checklist fails, and so does a row whose verdict is outside the closed
//! set. See `docs/audits/mapmerge/README.md` for the format.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Modules under `src/mappings/` that are helpers, not Mapping classes.
const MAPPING_HELPERS: &[&str] = &[
    "mod",
    "simplify",
    "helpers",
    "matrix_merge",
    "winmap_merge",
    "linear_approx",
    "map_box",
    "polymap_simd",
    "rate",
];

/// Region modules in the audit universe. `mod` covers `Region::Simplify` and
/// `GetDefUnc`; the rest each override `Simplify`.
const REGION_MODULES: &[&str] = &[
    "mod",
    "circle",
    "box_region",
    "prism",
    "interval",
    "nullregion",
    "pointlist",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/ast-core has two ancestors")
        .to_path_buf()
}

fn audit_dir() -> PathBuf {
    repo_root().join("docs/audits/mapmerge")
}

/// Every module that must carry a checklist, as checklist file stems.
fn audit_universe() -> BTreeSet<String> {
    let mut universe = BTreeSet::new();
    let mappings = repo_root().join("crates/ast-core/src/mappings");
    for entry in std::fs::read_dir(&mappings).expect("mappings dir readable") {
        let path = entry.expect("readable entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).expect("utf-8 stem");
        if MAPPING_HELPERS.contains(&stem) {
            continue;
        }
        universe.insert(stem.to_string());
    }
    for name in REGION_MODULES {
        universe.insert(format!("region_{name}"));
    }
    universe
}

fn checklist_path(stem: &str) -> PathBuf {
    audit_dir().join(format!("{stem}.md"))
}

/// The verdict cell of every table row in a checklist.
fn verdicts(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| l.trim_start().starts_with('|'))
        .filter_map(|l| {
            let cells: Vec<&str> = l.trim().trim_matches('|').split('|').collect();
            if cells.len() < 5 {
                return None;
            }
            let first = cells[0].trim();
            // Skip the header row and the `|---|` separator.
            if first.parse::<u32>().is_err() {
                return None;
            }
            Some(cells[4].trim().to_string())
        })
        .collect()
}

fn verdict_is_valid(v: &str) -> bool {
    v == "ported"
        || (v.starts_with("n/a:") && v.len() > "n/a:".len() + 1)
        || (v.starts_with("gap: PLAN.md ") && v.len() > "gap: PLAN.md ".len() + 1)
}

#[test]
fn every_audited_class_has_a_checklist() {
    let missing: Vec<String> = audit_universe()
        .into_iter()
        .filter(|stem| !checklist_path(stem).is_file())
        .collect();
    assert!(
        missing.is_empty(),
        "these classes have no MapMerge/Simplify checklist under \
         docs/audits/mapmerge/. Write one per docs/audits/mapmerge/README.md, \
         or add the module to the helper list if it is not a class: {missing:?}"
    );
}

#[test]
fn every_checklist_row_has_a_verdict() {
    let mut bad = Vec::new();
    for stem in audit_universe() {
        let path = checklist_path(&stem);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue; // reported by the other test
        };
        let rows = verdicts(&text);
        if rows.is_empty() {
            bad.push(format!("{stem}: checklist has no table rows"));
            continue;
        }
        for (i, v) in rows.iter().enumerate() {
            if !verdict_is_valid(v) {
                bad.push(format!("{stem} row {}: invalid verdict {v:?}", i + 1));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "a verdict must be `ported`, `n/a: <reason>`, or \
         `gap: PLAN.md <section>`: {bad:#?}"
    );
}
```

- [ ] **Step 4: Run it to verify it fails**

Run: `cargo test -p ast-core --test test_mapmerge_audit`
Expected: `every_audited_class_has_a_checklist` FAILS, listing every module except `shift` — roughly 30 names.
This is the correct starting state: the test is measuring the real gap.

- [ ] **Step 5: Commit the mechanism**

```bash
git add docs/audits/mapmerge crates/ast-core/tests/test_mapmerge_audit.rs
git commit -m "test: enforce per-class MapMerge audit checklists

A prose audit cannot say whether it is finished, which is how
simplifier-mapmerge-rebuild-plan.md came to claim completion its own body
contradicts. This one is a gate: a class with no checklist fails, and so does a
row without a verdict. ShiftMap is the worked example; the rest follow."
```

Note: this commit deliberately leaves `test_mapmerge_audit` failing. Tasks 6 and 7 close it. If a green tree between tasks is required, add `#[ignore]` with the reason `"checklists land in Tasks 6-7"` here and remove it in Task 7's final step.

---

## Task 6: Checklists for the four known suspects

These four are already diagnosed, so their checklists will contain the plan's first `gap` rows.

**Files:**
- Create: `docs/audits/mapmerge/{ratemap,chebymap,cmpmap,zoom}.md`
- Modify: `PLAN.md`

**Interfaces:**
- Consumes: the format and test from Task 5.
- Produces: four checklists, and `PLAN.md` sections named by their `gap` verdicts.

- [ ] **Step 1: Audit RateMap**

Read `reference/starlink-ast/src/ratemap.c`'s `MapMerge` body in full. It is 206 lines beginning at the step described at `ratemap.c:1094-1107`.
Create `docs/audits/mapmerge/ratemap.md` with one row per C step. Every step the Rust class does not perform gets `gap: PLAN.md §10.1`, because `crates/ast-core/src/mappings/ratemap.rs` has no `map_merge` at all.

- [ ] **Step 2: Record the RateMap gap in PLAN.md**

Add to `PLAN.md`:

```markdown
### 10.1 RateMap has no `map_merge`

C's `ratemap.c` `MapMerge` is a 206-line body. The Rust class implements none
of it; a comment cites only its opening steps at `ratemap.c:1094-1107`.

What would expose it: a simplify fixture placing a RateMap next to a Mapping it
can merge with, in series and in parallel. None exists in the corpus today,
which is why the gap has been invisible.
```

- [ ] **Step 3: Audit ChebyMap**

In C, ChebyMap is a PolyMap subclass and inherits PolyMap's `MapMerge`; `chebymap.c` defines none.
Confirm whether `crates/ast-core/src/mappings/chebymap.rs` reaches PolyMap's `map_merge` or silently falls back to the trait default.
Create `docs/audits/mapmerge/chebymap.md`. If it inherits correctly, every row is `n/a: inherited from PolyMap, see polymap.md`. If it does not, the rows are `gap: PLAN.md §10.2` and PLAN.md gains a section saying what would expose it.

- [ ] **Step 4: Audit CmpMap**

Create `docs/audits/mapmerge/cmpmap.md` from `reference/starlink-ast/src/cmpmap.c`'s `MapMerge` and `Simplify`.
Pay particular attention to the self-simplify head at `cmpmap.c:1437`, which the instrumented C shows is where the four under-tagging cases get their cards.
Rows the port does not perform get `gap: PLAN.md §10.3`.

- [ ] **Step 5: Record the CmpMap gap**

Add to `PLAN.md`:

```markdown
### 10.3 CmpMap under-tags four fixtures

`cap_cmpmap_01`, `cap_cmpmap_03`, `cap_cmpmap_04` and `cap_specmap_02` each
emit fewer `IsSimp` cards than C. The instrumented C shows the surviving
sub-Mapping is stamped inside `CmpMap::MapMerge`'s self-simplify head
(`cmpmap.c:1437`), reached from `Simplify`'s nominate loop.

What would expose it: already exposed, by the four fixtures named above and
pinned in `ISSIMP_DISAGREEMENTS`.

Six approaches to `cap_cmpmap_03` were measured and reverted; they are recorded
in `test_simplify_regression.rs` and must not be retried without a new reason.
```

- [ ] **Step 6: Audit ZoomMap and the clear-on-set convention**

Create `docs/audits/mapmerge/zoom.md`.
Include a row for `astSetZoom` / `astClearZoom` discarding both simplified records (`zoommap.c:1698,1704`), per the rule at `mapping.h:449-457`.
`crates/ast-core/src/mappings/zoom.rs` has no `set_zoom`, so that row is `n/a: the Rust class has no Zoom setter; the factor is fixed at construction`.

- [ ] **Step 7: Record the clear-on-set gap**

Add to `PLAN.md`:

```markdown
### 10.4 The clear-on-set convention is unported

Upstream's rule at `mapping.h:449-457`: an attribute setter whose value feeds
the first part of that class's own `astMapMerge` must call `astClearIsSimple`,
discarding both orientation records. C applies it in eleven classes -- ZoomMap
`Zoom`; LutMap `LutInterp`, `LutEpsilon`; MathMap `Seed`, `SimpFI`, `SimpIF`;
PolyMap `IterInverse`, `NiterInverse`, `TolInverse`; SplineMap `InvNiter`,
`InvTol`, `OutUnit`; SphMap `PolarLong`, `UnitRadius`; WcsMap `FITSProj`,
`LonCheck`; PcdMap `Disco`; PermMap `PermSplit`; IntraMap `IntraFlag`; and
GrismMap in macro form at `grismmap.c:165,255`.

The port implements it nowhere. Most of the gap is latent because several of
these attributes have no Rust setter yet.

What would expose it: a fixture that sets one of these attributes on an
already-simplified Mapping and re-simplifies it. None exists. Each class's
checklist carries the row, so adding a setter without the clear is caught by
review rather than by luck.
```

- [ ] **Step 8: Run the audit test**

Run: `cargo test -p ast-core --test test_mapmerge_audit`
Expected: `every_checklist_row_has_a_verdict` PASSES. `every_audited_class_has_a_checklist` still FAILS, now listing roughly 26 remaining modules.

- [ ] **Step 9: Commit**

```bash
git add docs/audits/mapmerge PLAN.md
git commit -m "docs: audit RateMap, ChebyMap, CmpMap and ZoomMap against C

The four already-diagnosed suspects. RateMap has none of C's 206-line MapMerge
body; the clear-on-set convention added upstream is unported in eleven classes;
CmpMap's self-simplify head is where the four under-tagging fixtures get their
cards. Each gap names what would expose it."
```

---

## Task 7: Checklists for the remaining classes

Bulk work, but mechanical. Each class is independent, so this task may be split across several commits — one per group of classes — provided the test is green at the end.

**Files:**
- Create: `docs/audits/mapmerge/<module>.md` for every remaining module in the audit universe
- Modify: `PLAN.md` for any new `gap` rows
- Modify: `crates/ast-core/tests/test_mapmerge_audit.rs` (remove `#[ignore]` if Task 5 added it)

**Interfaces:**
- Consumes: the format and test from Task 5.
- Produces: a complete audit universe. `test_mapmerge_audit` passes both tests.

- [ ] **Step 1: Audit the remaining Mapping classes**

For each of `dssmap`, `grismmap`, `lutmap`, `matrix`, `normmap`, `pcdmap`, `perm`, `polymap`, `selectormap`, `slamap`, `specmap`, `sphmap`, `splinemap`, `switchmap`, `timemap`, `tranmap`, `unit`, `unitnormmap`, `wcsmap`, `winmap`:

1. Open the C file's `MapMerge` (and `Simplify`, where the class has one).
2. List its comment-delimited steps.
3. Locate each step's Rust counterpart.
4. Write `docs/audits/mapmerge/<module>.md` in the Task 5 format.

Two classes have known findings to confirm rather than rediscover: `matrix`'s final fallback is gated on a mergeable neighbour (`matrix_merge.rs`), and `wcsmap` gained C's first step (`AST__WCSBAD` to `UnitMap`) only recently.

- [ ] **Step 2: Audit the Region modules**

For each of `region_mod`, `region_circle`, `region_box_region`, `region_prism`, `region_interval`, `region_nullregion`, `region_pointlist`:

`region_mod.md` covers `region.c`'s `Simplify` (from `region.c:11125`) and `GetDefUnc` (`region.c:3488`), against `crates/ast-core/src/regions/mod.rs`.
The others cover their class's `Simplify` override.

`region_circle.md` will contain the cause-B gap. Give it `gap: PLAN.md §10.5` and add:

```markdown
### 10.5 Region simplify tags a Mapping C leaves bare

Simplifying a Circle whose FrameSet holds a ZoomMap emits an `IsSimp` card on
that ZoomMap; the pinned C emits none. The instrumented C shows the object it
dumps is a freshly constructed ZoomMap with `flags = 0`, not the flagged one
its loader produced. C's Mapping copy constructor clears only `report`
(`mapping.c:23608-23648`), so a copy would have preserved the flag: the object
C writes was constructed, not copied. Which constructor builds it is not yet
identified.

The same defect from the other side: `simplify` does not stamp a returned
Region at all, which `neg_box_asymmetric_2d` shows.

What would expose it: already exposed, by `cefit_circle_conformal`,
`cefit_circle_nonconformal_keep` and `neg_box_asymmetric_2d`, and by CI run
32511602164 on both macos-latest and ubuntu-latest.
```

- [ ] **Step 3: Remove the ignore if Task 5 added one**

Delete any `#[ignore]` attribute added to `test_mapmerge_audit.rs` in Task 5.

- [ ] **Step 4: Run the audit test**

Run: `cargo test -p ast-core --test test_mapmerge_audit`
Expected: PASS, 2 passed.

- [ ] **Step 5: Run everything**

Run: `cargo test -p ast-core && cargo clippy -p ast-core --all-targets -- -D warnings`
Expected: all pass, no warnings.

- [ ] **Step 6: Commit**

```bash
git add docs/audits/mapmerge PLAN.md crates/ast-core/tests/test_mapmerge_audit.rs
git commit -m "docs: complete the MapMerge/Simplify audit universe

Every Mapping class and the Region simplify path now carry a step-by-step
checklist against C, and the audit test gates it. Each gap names what would
expose it rather than only what is missing."
```

---

## Task 8: Cause A — SwitchMap under-tags its selector sub-Mappings

**Files:**
- Modify: `crates/ast-core/src/mappings/switchmap.rs`
- Modify: `crates/ast-core/tests/test_simplify_regression.rs` (remove 4 rows from `SERIAL_UNGATED`, 4 from `ISSIMP_DISAGREEMENTS`)
- Modify: `docs/audits/mapmerge/switchmap.md`
- Test: `crates/ast-core/tests/test_simplify_regression.rs`

**Interfaces:**
- Consumes: `docs/audits/mapmerge/switchmap.md` from Task 7.
- Produces: `switchmap_invert_normalize`, `switchmap_internal_simplify`, `switchmap_fwdonly_simplify` and `switchmap_inv_invonly` byte-exact against their `.simp` references.

- [ ] **Step 1: Read C's rule**

Read `reference/starlink-ast/src/switchmap.c`'s `Simplify` and `MapMerge`, and find where the FSMap and ISMap sub-Mappings acquire their flag.
C emits 3 or 4 cards where the port emits 1, so the port is missing whatever stamps them.
Write the finding into `docs/audits/mapmerge/switchmap.md` before changing code. Do not infer the rule from the fixture.

- [ ] **Step 2: Run the failing fixtures to see the current diff**

Run: `SIMPLIFY_DIFF=1 cargo test -p ast-core --test test_simplify_regression 2>&1 | sed -n '/=== switchmap_invert_normalize/,/^===/p'`

Expected: the reference has `IsSimp = 1` on a UnitMap where the port has none.

- [ ] **Step 3: Implement C's step**

Apply the step identified in Step 1 to `crates/ast-core/src/mappings/switchmap.rs`, with a comment citing the C file and line range.

- [ ] **Step 4: Verify the four fixtures pass**

Run: `cargo test -p ast-core --test test_simplify_regression`
Expected: the four `switchmap_*` names no longer appear in the serialization-failure list.
The test still fails on the `SERIAL_UNGATED` and `ISSIMP_DISAGREEMENTS` set assertions, because those rows are now stale — Step 5 removes them.

- [ ] **Step 5: Delete the retired rows**

Remove these four from `SERIAL_UNGATED`:

```rust
("switchmap_invert_normalize", "IsSimp missing on selector sub-Mappings"),
("switchmap_internal_simplify", "IsSimp missing on selector sub-Mappings"),
("switchmap_fwdonly_simplify", "IsSimp missing on selector sub-Mappings"),
("switchmap_inv_invonly", "IsSimp missing on selector sub-Mappings"),
```

And these four from `ISSIMP_DISAGREEMENTS`:

```rust
"switchmap_fwdonly_simplify (1 vs C's 3)",
"switchmap_internal_simplify (1 vs C's 4)",
"switchmap_inv_invonly (1 vs C's 3)",
"switchmap_invert_normalize (1 vs C's 4)",
```

- [ ] **Step 6: Update the checklist verdict**

Change the affected row in `docs/audits/mapmerge/switchmap.md` from `gap: ...` to `ported`, with the Rust site filled in.

- [ ] **Step 7: Run everything**

Run: `cargo test -p ast-core && cargo clippy -p ast-core --all-targets -- -D warnings`
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add crates/ast-core/src/mappings/switchmap.rs crates/ast-core/tests/test_simplify_regression.rs docs/audits/mapmerge/switchmap.md
git commit -m "switchmap: stamp the selector sub-Mappings as C does

C flags the FSMap and ISMap sub-Mappings during simplification; the port left
them bare, so four fixtures emitted one IsSimp card where C emits three or
four. Their SERIAL_UNGATED and ISSIMP_DISAGREEMENTS rows are retired."
```

---

## Task 9: Cause B — Region simplify tags a Mapping C leaves bare

This is the CI failure. It is also the one cause whose C rule is not yet identified, so Step 1 is investigation and may take longer than the others.

**Files:**
- Modify: `crates/ast-core/src/regions/mod.rs` and/or `crates/ast-core/src/regions/circle.rs`
- Modify: `crates/ast-core/tests/test_regions.rs:3874-3915` (restore byte-exact comparison)
- Modify: `crates/ast-core/tests/test_simplify_regression.rs`
- Modify: `docs/audits/mapmerge/region_mod.md`, `docs/audits/mapmerge/region_circle.md`
- Modify: `PLAN.md` §10.5

**Interfaces:**
- Consumes: `docs/audits/mapmerge/region_circle.md` from Task 7.
- Produces: `cefit_circle_conformal`, `cefit_circle_nonconformal_keep` and `neg_box_asymmetric_2d` byte-exact; `test_regions.rs` no longer needs its local `strip_is_simp`.

- [ ] **Step 1: Identify the constructor**

Build the instrumented oracle and confirm the starting facts:

```bash
./tools/oracle_fixtures/build_issimp_instrumented.sh ${TMPDIR:-/tmp}/ast-instr
AST_LOG_ISSIMPLE=1 ${TMPDIR:-/tmp}/ast-instr/gen_simp \
  crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/cefit_circle_conformal.map \
  ${TMPDIR:-/tmp}/cc.out 2>&1 >/dev/null | grep -E "STAMP|DUMPTAG"
```

Expected: several `STAMP` lines and no `DUMPTAG` line, meaning nothing carrying the flag survives to the dump.

Then read the C source to find which constructor builds the ZoomMap that `region.c`'s `Simplify` path finally dumps. Start from `region.c:11125` (`Simplify`) and follow into `frameset.c`'s `Simplify`. The answer is a constructor call, because C's Mapping copy constructor preserves `flags` (`mapping.c:23608-23648`).

Record the finding in `docs/audits/mapmerge/region_mod.md` before writing any Rust.
If the rule cannot be identified from the C source, stop and report. Do not infer it from the fixture and do not add a heuristic.

- [ ] **Step 2: Note the pointer-identity subtlety**

C decides whether the FrameSet changed with `simpler = ( fs != new->frameset )` — a pointer comparison. `astSimplify_` returns `astClone( this )`, the same pointer, when the Mapping is already flagged simple (`mapping.c` ~24801-24805), so "already simple" reads as "unchanged".
The Rust port emulates this with the structural `Mapping::equal` in `crates/ast-core/src/regions/mod.rs:900-906`. Check that this emulation is faithful for the case in hand; it is a plausible home for the defect.

- [ ] **Step 3: Implement C's rule**

Apply the finding, with a comment citing the C file and line range.

- [ ] **Step 4: Restore the byte-exact region tests**

In `crates/ast-core/tests/test_regions.rs`, revert both `cefit` assertions to compare without stripping:

```rust
    assert_eq!(
        simpstab_simplified_native("cefit_circle_conformal.map"),
        simpstab_fixture("cefit_circle_conformal.simp")
    );
```

```rust
    assert_eq!(
        simpstab_simplified_native("cefit_circle_nonconformal_keep.map"),
        simpstab_fixture("cefit_circle_nonconformal_keep.simp")
    );
```

Delete the now-unused local `strip_is_simp` helper and the comment blocks above each assertion.

- [ ] **Step 5: Delete the retired rows**

From `SERIAL_UNGATED`:

```rust
("cefit_circle_conformal", "IsSimp on the FrameSet's ZoomMap; C emits none"),
("cefit_circle_nonconformal_keep", "IsSimp on the FrameSet's MathMap; C emits none"),
```

From `ISSIMP_DISAGREEMENTS`:

```rust
"cefit_circle_conformal (1 vs C's 0)",
"cefit_circle_nonconformal_keep (1 vs C's 0)",
```

And empty `NO_ISSIMP_CARD_UNGATED`:

```rust
const NO_ISSIMP_CARD_UNGATED: &[&str] = &[];
```

- [ ] **Step 6: Update the checklists and PLAN.md**

Change the affected rows in `docs/audits/mapmerge/region_mod.md` and `region_circle.md` to `ported`.
Delete `PLAN.md` §10.5.

- [ ] **Step 7: Run everything**

Run: `cargo test -p ast-core && cargo clippy -p ast-core --all-targets -- -D warnings`
Expected: all pass. `test_regions` reports 111 passed, 0 failed.

- [ ] **Step 8: Commit**

```bash
git add crates/ast-core/src/regions crates/ast-core/tests PLAN.md docs/audits/mapmerge
git commit -m "regions: match C's IsSimp placement through Region simplify

Simplifying a Region tagged a Mapping inside its encapsulated FrameSet that C
leaves bare, and never tagged the returned Region. Both cefit fixtures compare
byte-exactly again, so test_regions no longer strips the card, and
neg_box_asymmetric_2d's untagged-result licence is retired.

Fixes the CI failure in run 32511602164 on both macos-latest and
ubuntu-latest."
```

---

## Task 10: Cause C — the uncertainty does not collapse Prism to Box

**Files:**
- Modify: `crates/ast-core/src/regions/mod.rs` (the uncertainty simplify path) and/or `crates/ast-core/src/regions/prism.rs`
- Modify: `crates/ast-core/tests/test_simplify_regression.rs`
- Modify: `docs/audits/mapmerge/region_mod.md`, `docs/audits/mapmerge/region_prism.md`

**Interfaces:**
- Consumes: `docs/audits/mapmerge/region_mod.md` from Task 7.
- Produces: `box_unc_fillfactor_merge` and `nullregion_unc_fillfactor_merge` byte-exact.

- [ ] **Step 1: Confirm what differs**

Run: `SIMPLIFY_DIFF=1 cargo test -p ast-core --test test_simplify_regression 2>&1 | sed -n '/=== box_unc_fillfactor_merge/,/^===/p'`

Expected: `got "Begin Prism"` against `want "Begin Box"`, inside the `Unc =` block.

Note that `equal()` passes both fixtures and is right to: C's `region.c::Equal` compares class name, `points`, base Frame, current Frame, the base-to-current Mapping, `Negated` and `Closed`, and deliberately not the uncertainty. The byte comparison is the only gate that sees this.

- [ ] **Step 2: Read C's uncertainty simplification**

Read `region.c`'s `Simplify` from the uncertainty branch at `region.c:11145` onward, and `GetDefUnc` at `region.c:3488`.
Find where C collapses an extruded uncertainty to a Box. Record it in `docs/audits/mapmerge/region_mod.md`.

- [ ] **Step 3: Implement C's step**

Apply it with a comment citing the C file and line range.

- [ ] **Step 4: Verify**

Run: `cargo test -p ast-core --test test_simplify_regression`
Expected: neither fixture appears in the serialization-failure list.

- [ ] **Step 5: Delete the retired rows**

From `SERIAL_UNGATED`:

```rust
("box_unc_fillfactor_merge", "emits Prism where C emits Box"),
("nullregion_unc_fillfactor_merge", "emits Prism where C emits Box"),
```

- [ ] **Step 6: Update the checklists**

Change the affected rows to `ported`.

- [ ] **Step 7: Run everything**

Run: `cargo test -p ast-core && cargo clippy -p ast-core --all-targets -- -D warnings`
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add crates/ast-core/src/regions crates/ast-core/tests docs/audits/mapmerge
git commit -m "regions: collapse an extruded uncertainty to a Box as C does

Two fixtures emitted a Prism uncertainty where C emits a Box. astEqual cannot
see this -- C's Region Equal deliberately ignores the uncertainty -- so the
byte comparison is the only gate that catches it."
```

---

## Task 11: Cause D — `cap_zoommap_04` keeps a FrameSet where C has a PointSet

**Files:**
- Modify: `crates/ast-core/src/regions/mod.rs` or the relevant Region class
- Modify: `crates/ast-core/tests/test_simplify_regression.rs`
- Modify: the relevant `docs/audits/mapmerge/region_*.md`

**Interfaces:**
- Consumes: the Region checklists from Task 7.
- Produces: `cap_zoommap_04` byte-exact.

- [ ] **Step 1: Confirm what differs**

Run: `SIMPLIFY_DIFF=1 cargo test -p ast-core --test test_simplify_regression 2>&1 | sed -n '/=== cap_zoommap_04/,/^===/p'`

Expected: `got "FrmSet ="` and `Begin FrameSet` where the reference has `Points =` and `Begin PointSet`.
The port is keeping an encapsulated FrameSet that C has collapsed.

- [ ] **Step 2: Read C's rule**

This is the `region.c` `Simplify` branch that strips the FrameSet when the base-to-current Mapping is a UnitMap. `crates/ast-core/src/regions/mod.rs:224-232` is the port's counterpart — read both and find the divergence.
Record it in the checklist.

- [ ] **Step 3: Implement C's step**

Apply it with a comment citing the C file and line range.

- [ ] **Step 4: Verify**

Run: `cargo test -p ast-core --test test_simplify_regression`
Expected: `cap_zoommap_04` no longer appears in the serialization-failure list.

- [ ] **Step 5: Delete the retired rows**

From `SERIAL_UNGATED`:

```rust
("cap_zoommap_04", "emits FrmSet/FrameSet where C emits Points/PointSet"),
```

From `ISSIMP_DISAGREEMENTS`:

```rust
"cap_zoommap_04 (3 vs C's 0)",
```

- [ ] **Step 6: Update the checklist and run everything**

Change the affected row to `ported`.
Run: `cargo test -p ast-core && cargo clippy -p ast-core --all-targets -- -D warnings`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add crates/ast-core/src/regions crates/ast-core/tests docs/audits/mapmerge
git commit -m "regions: strip the encapsulated FrameSet as C does

cap_zoommap_04 kept a FrmSet where C collapses to bare Points."
```

---

## Task 12: Cause E — `Closed` is written when C omits it

**Files:**
- Modify: `crates/ast-core/src/regions/mod.rs` (the Region serialiser)
- Modify: `crates/ast-core/tests/test_simplify_regression.rs`
- Test: `crates/ast-core/tests/test_serialisation.rs`

**Interfaces:**
- Consumes: nothing beyond Task 7's checklists.
- Produces: `cap_interval_01` byte-exact.

- [ ] **Step 1: Confirm what differs**

Run: `SIMPLIFY_DIFF=1 cargo test -p ast-core --test test_simplify_regression 2>&1 | sed -n '/=== cap_interval_01/,/^===/p'`

Expected: the port emits `Closed = 1 # Boundary is inside` where the reference has no such card.

- [ ] **Step 2: Read C's dump rule**

In `reference/starlink-ast/src/region.c`, find the `astWriteInt( channel, "Closed", ... )` call and read its `set` and `helpful` arguments.
C writes an attribute card only when `set` is true, and writes an unset-but-`helpful` value as a comment. With `helpful` zero, an unset value produces no card at all — the same mechanism that removed `KyCas` from `cap_timemap_02`.

- [ ] **Step 3: Write the failing test**

Add to `crates/ast-core/tests/test_serialisation.rs`:

```rust
#[test]
fn unset_closed_writes_no_card() {
    // C writes Closed only when the attribute is set (region.c's Dump passes
    // helpful = 0), so a Region that never had Closed assigned must produce no
    // card at all -- not a commented default.
    use ast_core::regions::IntervalBuilder;
    use ast_core::frames::BasicFrame;

    let frame = BasicFrame::new(1).expect("1-d frame");
    let interval = IntervalBuilder::new(frame, &[0.0], &[1.0])
        .build()
        .expect("interval builds");
    let text = ast_core::to_native_string(&ast_core::serial::AnyObject::Interval(interval))
        .expect("serialises");
    assert!(
        !text.lines().any(|l| l.trim_start().starts_with("Closed =")),
        "unset Closed must emit no card, got:\n{text}"
    );
}
```

- [ ] **Step 4: Run it to verify it fails**

Run: `cargo test -p ast-core --test test_serialisation unset_closed_writes_no_card`
Expected: FAIL, with the dump showing `Closed = 1`.

- [ ] **Step 5: Implement the fix**

Make the Region serialiser track whether `Closed` was explicitly set and omit the card when it was not, mirroring C's `set` argument.
Comment it with the C file and line range.

- [ ] **Step 6: Run it to verify it passes**

Run: `cargo test -p ast-core --test test_serialisation unset_closed_writes_no_card`
Expected: PASS.

- [ ] **Step 7: Delete the retired row and run everything**

From `SERIAL_UNGATED`:

```rust
("cap_interval_01", "emits Closed = 1 where C omits it"),
```

Run: `cargo test -p ast-core && cargo clippy -p ast-core --all-targets -- -D warnings`
Expected: all pass.
If other fixtures change, they were relying on the spurious card; check each against the pinned C before adjusting anything.

- [ ] **Step 8: Commit**

```bash
git add crates/ast-core/src crates/ast-core/tests
git commit -m "regions: omit the Closed card when the attribute is unset

C's Dump passes helpful = 0 for Closed, so an unset value produces no card at
all. The port wrote one unconditionally."
```

---

## Task 13: Cause F — decide whether `cap_pointlist_01` is a defect

Unlike the others this may correctly end in a documented relaxation. Decide with the oracle, not by loosening a tolerance.

**Files:**
- Modify: `crates/ast-core/src/regions/pointlist.rs` **or** `crates/ast-core/tests/fixtures/native/reference/simplify_tests.txt`
- Modify: `crates/ast-core/tests/test_simplify_regression.rs`
- Modify: `PLAN.md` §6 and `crates/ast-core/tests/test_verification_ledger.rs` if a relaxation is added

**Interfaces:**
- Consumes: the instrumented oracle from `tools/oracle_fixtures/`.
- Produces: either a fixed arithmetic path, or one named relaxation counted in the ledger.

- [ ] **Step 1: Quantify the divergence**

Run: `SIMPLIFY_DIFF=1 cargo test -p ast-core --test test_simplify_regression 2>&1 | sed -n '/=== cap_pointlist_01/,/^===/p'`

Expected: `X5` differs at roughly 3e-15 relative and `X6` at roughly 1.2e-10 relative.
`X6`'s error is five orders of magnitude larger than `X5`'s. That asymmetry argues for a real arithmetic difference rather than platform noise, because platform noise would affect both alike.

- [ ] **Step 2: Find the C computation**

Identify which C routine produces these coordinates — the PointList mesh path in `pointlist.c`, reached from the Region simplify path.
Compare its order of operations with the Rust implementation. A different association order in a subtraction is the usual cause of an error this size.

- [ ] **Step 3a: If the port's arithmetic differs from C's, fix it**

Match C's construction order, not merely its result. Comment with the C file and line range.
Then delete the row from `SERIAL_UNGATED`:

```rust
("cap_pointlist_01", "two mesh coordinates diverge past 4 ulp"),
```

- [ ] **Step 3b: If the arithmetic matches and the difference is platform**

Set the fixture's `skip_string_compare` field in `simplify_tests.txt` to `yes`, with a comment above the row naming the two coordinates, their measured relative errors, and the C routine that computes them.
Delete the `SERIAL_UNGATED` row, add the case to `PLAN.md` §6, and raise the `skip_string_compare` count in `test_verification_ledger.rs` by one.

Do not widen a global tolerance. Do not take this branch without having read the C routine in Step 2.

- [ ] **Step 4: Run everything**

Run: `cargo test -p ast-core && cargo clippy -p ast-core --all-targets -- -D warnings`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/ast-core PLAN.md
git commit -m "pointlist: reconcile the mesh coordinates with C"
```

Use this message instead if Step 3b was taken:

```bash
git commit -m "test(serial): record cap_pointlist_01's platform-dependent mesh coordinates

The arithmetic matches C's construction order; the last bits do not survive
this platform. Named per case and counted in the ledger."
```

---

## Task 14: The four CmpMap under-tagging cases

Last, because Tasks 8 to 13 may change what these fixtures produce.

**Files:**
- Modify: `crates/ast-core/src/mappings/cmpmap.rs` and/or `crates/ast-core/src/mappings/simplify.rs`
- Modify: `crates/ast-core/tests/test_simplify_regression.rs`
- Modify: `docs/audits/mapmerge/cmpmap.md`
- Modify: `PLAN.md` §10.3

**Interfaces:**
- Consumes: `docs/audits/mapmerge/cmpmap.md` from Task 6.
- Produces: `ISSIMP_DISAGREEMENTS` and `SERIAL_UNGATED` both empty.

- [ ] **Step 1: Re-measure**

Run: `cargo test -p ast-core --test test_simplify_regression 2>&1 | grep -E "left:|right:"`

Expected: `ISSIMP_DISAGREEMENTS` now holds only `rigby`, `cap_cmpmap_01`, `cap_cmpmap_03`, `cap_cmpmap_04` and `cap_specmap_02`. If any has changed count, re-derive before proceeding.

- [ ] **Step 2: Trace C for one case**

```bash
${TMPDIR:-/tmp}/ast-instr/gen_simp \
  crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/cap_cmpmap_03.map \
  ${TMPDIR:-/tmp}/c3.simp
AST_LOG_ISSIMPLE=1 ${TMPDIR:-/tmp}/ast-instr/gen_simp \
  crates/ast-core/tests/fixtures/native/reference/simplify_fixtures/cap_cmpmap_03.map \
  ${TMPDIR:-/tmp}/c3.simp 2>&1 >/dev/null | grep -E "STAMP|DUMPTAG"
```

The surviving card comes from `CmpMap::MapMerge`'s self-simplify head at `cmpmap.c:1437`, reached from `Simplify`'s nominate loop.
Work out from the checklist which step of that head the port omits.

Six approaches were already measured and reverted: tagging the reduced leaf in `flatten`; carrying the record through ShiftMap's merge-revert; tagging the reconstructed parallel; tagging every `simplify_mapping` result; tagging at `ShiftMap::new`; and deferring leaf reduction entirely. They are recorded in `test_simplify_regression.rs`. Do not retry any of them without a reason the trace supplies.

- [ ] **Step 3: Implement C's step**

Apply it with a comment citing the C file and line range.

- [ ] **Step 4: Verify all four, and check the negative controls**

Run: `cargo test -p ast-core --test test_simplify_regression`
Expected: all four leave `ISSIMP_DISAGREEMENTS`.

If a negative-control fixture breaks, stop. That means the rule is not C's rule — it is stop signal 2, and the fix is wrong however many positive cases it passes.

- [ ] **Step 5: Empty the lists**

```rust
const ISSIMP_DISAGREEMENTS: &[&str] = &[];
```

```rust
const SERIAL_UNGATED: &[(&str, &str)] = &[];
```

`rigby` is expected to reach zero with the others; if it does not, it keeps its row and `PLAN.md` §6 records why, with its C cause named.

- [ ] **Step 6: Update the checklist, PLAN.md and the ledger**

Change the affected `cmpmap.md` rows to `ported` and delete `PLAN.md` §10.3.
Set the `test_simplify_regression.rs::ISSIMP_PLACEMENT_LICENSED` count in `test_verification_ledger.rs` to 0 and update `PLAN.md` §6.

- [ ] **Step 7: Run everything**

Run: `cargo test -p ast-core && cargo clippy -p ast-core --all-targets -- -D warnings && cargo fmt --check`
Expected: all pass.

- [ ] **Step 8: Confirm the definition of done**

Check each item from the spec:

```bash
grep -A3 "const SERIAL_UNGATED" crates/ast-core/tests/test_simplify_regression.rs
grep -A3 "const ISSIMP_DISAGREEMENTS" crates/ast-core/tests/test_simplify_regression.rs
cargo test -p ast-core --test test_mapmerge_audit
cargo test -p ast-core --test test_verification_ledger
./tools/oracle_fixtures/check_corpus_fresh.sh
```

Expected: both constants empty, both tests pass, and the corpus check exits 0.

- [ ] **Step 9: Commit**

```bash
git add crates/ast-core PLAN.md docs/audits/mapmerge
git commit -m "cmpmap: stamp the surviving sub-Mapping as C's self-simplify head does

The last four IsSimp under-tagging fixtures. SERIAL_UNGATED and
ISSIMP_DISAGREEMENTS are now empty, and the simplify path carries no IsSimp
licence at all."
```

---

## Self-review

**Spec coverage.** W3.1 is Task 2; W3.2 is Task 3; W3.3 is Task 4 Step 3; W3.4 is Task 1. W1's form is Task 5, its first targets are Task 6, its completion is Task 7. W2's causes A to F are Tasks 8 to 13, and the four under-tagging cases are Task 14. Every definition-of-done item is verified in Task 14 Step 8.

**Known gap, deliberate.** Task 9 Step 1 is investigation whose answer is not yet known, and the plan says to stop and report rather than infer a rule. That is the spec's instruction, not an omission.

**Type consistency.** `SERIAL_UNGATED` is `&[(&str, &str)]` and `ISSIMP_DISAGREEMENTS` is `&[&str]` throughout, matching the current source. The `classify` return type in Task 1 matches its use in the wrapper. Checklist file stems match `audit_universe()`: mapping modules by bare stem, region modules prefixed `region_`.
