# Copyright 2026 Association of Universities for Research in Astronomy (AURA)
# SPDX-License-Identifier: LGPL-3.0-or-later

"""Classify every simplify reference against a fresh capture from the pinned C.

Card-only drift means the reference predates a library change and must be
regenerated. Numeric drift means it was captured on another platform: it is
reported and tolerated, because regenerating it here would replace a
cross-platform reference with this machine's libm. Structural drift -- a
different nesting or a different class -- should never happen.

A negative fixture names its own input as its reference. Its fresh capture
carries none of the ``IsSimp`` cards a simplify result gains, so those lines are
dropped from both sides for those rows and for those rows only --
``test_simplify_regression.rs`` compares them the same way and gates the card
counts separately.

A row whose reference is a Mapping but whose input is a FrameSet was captured
from the FrameSet's base->current Mapping, which is what ``gen_simp -m`` does and
what ``test_simplify_regression.rs`` does on the port side.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

CARD = re.compile(r"^\s*([A-Za-z][A-Za-z0-9_]*)\s*=")
STRUCT = re.compile(r"^\s*(Begin|End|IsA)\s+(\S+)")

Row = tuple[str, str, str]


def parse_manifest(path: Path) -> list[Row]:
    """Return (name, input, reference) for every manifest row.

    Paths are relative to the fixture root, as the upstream harness reads them.
    """
    rows: list[Row] = []
    for raw in path.read_text().splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        fields = [f.strip() for f in line.split("|")]
        if len(fields) != 4:
            raise SystemExit(f"malformed manifest row: {raw!r}")
        name, inp, ref, _skip = fields
        rows.append((name, inp, ref))
    return rows


def top_class(text: str) -> str:
    """Return the class name on the first ``Begin`` line."""
    for line in text.splitlines():
        found = STRUCT.match(line)
        if found and found.group(1) == "Begin":
            return found.group(2)
    return ""


def shape(text: str) -> list[str]:
    """Return the structural skeleton: one token per Begin/End/IsA and card name."""
    out: list[str] = []
    for line in text.splitlines():
        found = STRUCT.match(line)
        if found:
            out.append(f"{found.group(1)} {found.group(2)}")
            continue
        found = CARD.match(line)
        if found:
            out.append(found.group(1))
    return out


def strip_is_simp(text: str) -> str:
    """Return ``text`` with every ``IsSimp`` card line removed."""
    kept = [ln for ln in text.splitlines() if not ln.strip().startswith("IsSimp =")]
    return "\n".join(kept) + "\n"


def classify(ref: str, fresh: str, negative: bool) -> str:
    """Return 'fresh', 'numeric', 'card' or 'structural' for one comparison."""
    if negative:
        # The reference is the input, which `astSimplify` was never run on, so
        # every `IsSimp` card the fresh capture carries is a stamp the
        # simplifier put on a Mapping it handed back -- the outermost one and
        # any component returned unchanged. `test_simplify_regression.rs` drops
        # those lines from both sides for exactly these rows and gates the card
        # counts separately; freshness is asked the same way.
        ref, fresh = strip_is_simp(ref), strip_is_simp(fresh)
    if ref == fresh:
        return "fresh"
    ref_shape, fresh_shape = shape(ref), shape(fresh)
    if ref_shape != fresh_shape:
        prefixes = ("Begin", "End", "IsA")
        classes_ref = [t for t in ref_shape if t.startswith(prefixes)]
        classes_fresh = [t for t in fresh_shape if t.startswith(prefixes)]
        return "structural" if classes_ref != classes_fresh else "card"
    return "numeric"


def main(argv: list[str]) -> int:
    """Compare every manifest row's reference against a fresh capture."""
    if len(argv) != 4:
        print("usage: check_simplify_fresh.py <corpus-dir> <gen_simp> <work-dir>")
        return 2
    corpus, driver, work = Path(argv[1]), Path(argv[2]), Path(argv[3])
    work.mkdir(parents=True, exist_ok=True)

    counts = {"fresh": 0, "numeric": 0, "card": 0, "structural": 0}
    for name, inp, ref in parse_manifest(corpus / "simplify/cases.txt"):
        reference = (corpus / ref).read_text()
        out = work / f"{name}.freshC"
        command = [str(driver)]
        if top_class(reference) != "FrameSet":
            command.append("-m")
        command += [str(corpus / inp), str(out), "1", "3"]
        result = subprocess.run(command, capture_output=True, text=True, check=False)
        if result.returncode != 0:
            print(f"CAPTURE-FAILED {name}: {result.stderr.strip()}")
            counts["structural"] += 1
            continue
        verdict = classify(reference, out.read_text(), negative=(inp == ref))
        counts[verdict] += 1
        if verdict != "fresh":
            print(f"{verdict.upper():>10} {name}")

    print(
        f"\n{counts['fresh']} fresh, {counts['numeric']} numerically drifted, "
        f"{counts['card']} card-drifted, {counts['structural']} structurally drifted"
    )
    if counts["card"] or counts["structural"]:
        print(
            "\nCard-drifted references predate a library change: regenerate them "
            "with gen_simp. Numeric drift is left alone -- regenerating it would "
            "bake this machine's libm into a cross-platform reference."
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
