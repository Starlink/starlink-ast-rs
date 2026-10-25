# Copyright 2026 Association of Universities for Research in Astronomy (AURA)
# SPDX-License-Identifier: LGPL-3.0-or-later

"""Take IsSimp cards from a freshly generated reference, values from the old one.

Some references are stale in their IsSimp cards and, separately, were captured
on a platform whose last bits differ from this one. The two never share a line,
so the correct reference is the committed file with only its IsSimp cards
replaced. This asserts that separation rather than assuming it.
"""

import pathlib
import re
import sys

_NUMERIC = re.compile(r"^[+-]?(\d+\.\d*|\.\d+|\d+)([eEdD][+-]?\d+)?$")


def _is_issimp(line):
    return line[:8].strip().startswith("ISSIMP")


def _without_issimp(text):
    return [l for l in text.splitlines() if not _is_issimp(l)]


def _issimp(text):
    return [l for l in text.splitlines() if _is_issimp(l)]


def _card(line):
    """Split a FITS card into keyword, value text and comment."""
    keyword = line[:8].strip()
    rest = line[9:] if line[8:9] == "=" else ""
    value, sep, comment = rest.partition(" /")
    return keyword, value.strip(), comment.strip() if sep else ""


def _is_float_literal(text):
    """True for a numeric value written with a decimal point or an exponent.

    Native format writes an integer without either, so this separates the values
    platform arithmetic can move from the ones that encode structure.
    """
    return bool(_NUMERIC.match(text)) and ("." in text or "e" in text.lower()
                                           or "d" in text.lower())


def assert_separable(old_text, new_text):
    """Check that the two files differ only in IsSimp cards and float values.

    Splicing cards from one file into another reconstructs what the new library
    would have emitted on the old platform only while that holds. A differing
    keyword, comment, card count, or integer value means something other than
    platform arithmetic moved, and the file needs a genuine capture instead.
    """
    old, new = _without_issimp(old_text), _without_issimp(new_text)
    if len(old) != len(new):
        raise ValueError(f"card counts differ outside IsSimp: {len(old)} vs {len(new)}")
    for a, b in zip(old, new):
        if a == b:
            continue
        old_kw, old_val, old_com = _card(a)
        new_kw, new_val, new_com = _card(b)
        if old_kw != new_kw or old_com != new_com:
            raise ValueError(f"{old_kw or a!r}: differs outside its value; not separable")
        if not (_is_float_literal(old_val) and _is_float_literal(new_val)):
            raise ValueError(f"{old_kw}: {old_val!r} -> {new_val!r} is not a float "
                             "value; not separable")


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
    """Every assertion the reconstruction rests on. Raises ValueError on any.

    The two conditions on the merged file say that the splice took the right
    lines from each side. `assert_separable` says the splice was legitimate in
    the first place, which the merged file alone cannot show: `merge` satisfies
    the other two by construction whatever it was fed.
    """
    assert_separable(old_text, new_text)
    if _without_issimp(merged_text) != _without_issimp(old_text):
        raise ValueError("merged file differs from the reference outside IsSimp cards")
    if _issimp(merged_text) != _issimp(new_text):
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
