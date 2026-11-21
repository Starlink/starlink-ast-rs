#!/usr/bin/env python3
# Copyright 2026 Association of Universities for Research in Astronomy (AURA)
# SPDX-License-Identifier: LGPL-3.0-or-later
"""Generate crates/starlink-ast/src/plot/forward.rs.

A Plot is a FrameSet in the C library, so every Frame and Mapping method of
a Plot does what the FrameSet's does, except the few that C's Plot class
overrides and those that copy or compare the object itself. This script
reads the `Frame` and `Mapping` trait definitions and writes a forwarding
method to the Plot's FrameSet for every trait method not in OWN_FRAME or
OWN_MAPPING, which `plot/mod.rs` implements by hand, or in INHERIT_MAPPING,
which keep the trait's default body.

Rerun it whenever either trait gains, loses or changes a method:

    python3 tools/gen_plot_forward.py

`plot_forwards_the_frameset_methods_c_inherits` in tests/test_plot.rs fails
when the generated file is out of date.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CRATE = ROOT / "crates" / "starlink-ast" / "src"

# Methods a Plot implements itself, with the body each gets; plot/mod.rs
# holds the helpers they call.
OWN_FRAME = {
    "snapshot": "self.plot_snapshot()",
    "equal": "self.frame_equal(other)",
    "frame_without_regions": "self.plot_frame_without_regions()",
    "simplify_frame": "self.plot_simplify_frame()",
    "as_frameset": "Some(&self.frameset)",
    "as_frameset_mut": "Some(&mut self.frameset)",
    "attr_class": '"Plot"',
    "attr_vset": "crate::attributes::frameset::frameset_vset(self, assignments, text)",
    "attr_clear_list": "crate::attributes::frameset::frameset_clear_list(self, names)",
    "attr_set": "crate::attributes::plot::plot_set(self, setting)",
    "attr_get": "crate::attributes::plot::plot_get(self, name)",
    "attr_test": "crate::attributes::plot::plot_test(self, name)",
    "attr_clear": "crate::attributes::plot::plot_clear(self, name)",
}
OWN_MAPPING = {
    "transform": "self.plot_transform(pts, forward)",
    "transform_into": "self.plot_transform_into(pts, forward, out)",
    "clone_box": "Box::new(self.clone())",
    "equal": "self.mapping_equal(other)",
    "simplified": "self.plot_simplified(gate)",
    "mapping_without_regions": "self.plot_mapping_without_regions()",
    "as_frameset": "Some(&self.frameset)",
}

# Methods a Plot inherits from the Mapping class rather than from FrameSet,
# which does not override them in C (frameset.c's vtab overrides only Rate,
# MapSplit, Simplify and Transform among Mapping's methods). Their default
# bodies transform through `self`, so on a Plot they reach the Plot's own
# transform, which normalises and clips, as mapping.c's versions reach
# Plot's Transform through astTransform.
INHERIT_MAPPING = {
    "tran1",
    "tran2",
    "transform_into_nan",
    "linear_approx",
    "quad_approx",
    "tran_grid",
    "map_box",
}


def trait_body(path, header):
    text = path.read_text()
    start = text.index(header)
    brace = text.index("{", start)
    depth = 0
    for i in range(brace, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return text[brace + 1 : i]
    raise SystemExit(f"unterminated trait {header}")


def split_top(s, sep=","):
    """Split at top-level separators, outside (), [], <> and {}."""
    out, depth, cur = [], 0, ""
    i = 0
    while i < len(s):
        c = s[i]
        if c in "([{<":
            depth += 1
        elif c in ")]}>" and not (c == ">" and i > 0 and s[i - 1] == "-"):
            depth -= 1
        if c == sep and depth == 0:
            out.append(cur)
            cur = ""
        else:
            cur += c
        i += 1
    if cur.strip():
        out.append(cur)
    return out


def methods(body):
    """Yield (name, generics, params, ret, where) for each `fn` at the
    trait's top level."""
    body = re.sub(r"//[^\n]*", "", body)
    i = 0
    depth = 0
    while i < len(body):
        c = body[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
        if depth == 0 and body.startswith("fn ", i) and (i == 0 or body[i - 1] in " \n"):
            m = re.match(r"fn\s+([a-z_0-9]+)\s*(<[^>]*>)?\s*\(", body[i:])
            name, generics = m.group(1), m.group(2) or ""
            j = i + m.end()
            pd = 1
            k = j
            while pd:
                if body[k] == "(":
                    pd += 1
                elif body[k] == ")":
                    pd -= 1
                k += 1
            params = body[j : k - 1]
            # The return type and where clause run to the first `;` or `{`
            # outside brackets: `[f64; 2]` holds a `;`.
            rest_end = k
            bd = 0
            while bd or body[rest_end] not in ";{":
                if body[rest_end] in "([<" and body[rest_end - 1 : rest_end + 1] != "->":
                    bd += 1
                elif body[rest_end] in ")]>" and body[rest_end - 1 : rest_end + 1] != "->":
                    bd -= 1
                rest_end += 1
            tail = body[k:rest_end]
            where = ""
            if "where" in tail:
                tail, where = tail.split("where", 1)
                where = "where " + where.strip()
            ret = tail.strip()
            yield name, generics, " ".join(params.split()), " ".join(ret.split()), " ".join(where.split())
            i = rest_end
            continue
        i += 1


def forward(trait, name, generics, params, ret, where, body=None):
    parts = [p.strip() for p in split_top(params) if p.strip()]
    receiver, args = parts[0], parts[1:]
    names = [a.split(":", 1)[0].strip().removeprefix("mut ") for a in args]
    target = "&mut self.frameset" if "mut" in receiver else "&self.frameset"
    if body is not None:
        # The trait may name an argument with a leading underscore.
        call = body
        parts = [receiver] + [
            a.strip().lstrip("_") if a.strip().startswith("_") else a.strip() for a in args
        ]
    else:
        call = f"{trait}::{name}({', '.join([target] + names)})"
        # A forwarded argument is only passed on, so a `mut` binding the
        # trait's default body needed is dropped.
        parts = [receiver] + [a.strip().removeprefix("mut ") for a in args]
    sig = f"fn {name}{generics}({', '.join(parts)})"
    if ret:
        sig += f" {ret}"
    if where:
        sig += f" {where}"
    return f"    {sig} {{\n        {call}\n    }}\n"


def generate():
    frame = trait_body(CRATE / "frames" / "mod.rs", "pub trait Frame: ")
    mapping = trait_body(CRATE / "mappings" / "mod.rs", "pub trait Mapping: ")
    out = [
        "// Copyright 2026 Association of Universities for Research in Astronomy (AURA)\n",
        "// SPDX-License-Identifier: LGPL-3.0-or-later\n",
        "\n",
        "// Generated by tools/gen_plot_forward.py from the Frame and Mapping\n",
        "// traits; do not edit. Every method a Plot does not implement itself\n",
        "// does what its FrameSet's does, as a C Plot inherits it.\n",
        "\n",
        "#![allow(unused_imports)]\n",
        "\n",
        "use super::Plot;\n",
        "use crate::coords::{PointSet, Points, PointsMut};\n",
        "use crate::error::AstResult;\n",
        "use crate::frames::*;\n",
        "use crate::mappings::*;\n",
        "\n",
        "impl Frame for Plot {\n",
    ]
    for name, generics, params, ret, where in methods(frame):
        if "Self: Sized" in where:
            continue
        out.append(forward("Frame", name, generics, params, ret, where, OWN_FRAME.get(name)))
    out.append("}\n\nimpl Mapping for Plot {\n")
    for name, generics, params, ret, where in methods(mapping):
        if "Self: Sized" in where or name in INHERIT_MAPPING:
            continue
        out.append(forward("Mapping", name, generics, params, ret, where, OWN_MAPPING.get(name)))
    out.append("}\n")
    return "".join(out)


def formatted(text):
    """The text as rustfmt formats it, as `cargo fmt` would leave it."""
    return subprocess.run(
        ["rustfmt", "--edition", "2024", "--emit", "stdout"],
        input=text,
        capture_output=True,
        text=True,
        check=True,
    ).stdout


def main():
    path = CRATE / "plot" / "forward.rs"
    text = formatted(generate())
    if "--check" in sys.argv:
        sys.exit(0 if path.read_text() == text else 1)
    path.write_text(text)


if __name__ == "__main__":
    main()
