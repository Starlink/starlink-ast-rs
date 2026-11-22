#!/usr/bin/env bash
# Copyright 2026 Association of Universities for Research in Astronomy (AURA)
# SPDX-License-Identifier: LGPL-3.0-or-later
#
# Builds the browser FITS plotter, target/plotter/fits-plotter.html: one file
# holding the page (web/plotter/), the wasm-bindgen glue, the examples and
# the WebAssembly, so it opens from file:// with nothing else.
#
#   tools/build_plotter.sh            build the page
#   tools/build_plotter.sh --smoke    build for Node with the smoke-test
#                                     feature and run tools/plotter_smoke.mjs
#
# Needs rustup with the wasm32-unknown-unknown target, wasm-bindgen-cli of the
# version crates/ast-plotter-wasm pins, python3, and node for --smoke.

set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

smoke=0
case "${1:-}" in
  --smoke) smoke=1 ;;
  "") ;;
  *) echo "usage: $0 [--smoke]" >&2; exit 2 ;;
esac

# Rustup's cargo, because a Homebrew rustc earlier on PATH has no wasm32
# standard library; cargo install puts wasm-bindgen in CARGO_HOME.
if ! command -v rustup >/dev/null; then
  echo "rustup is needed, with: rustup target add wasm32-unknown-unknown" >&2
  exit 1
fi
PATH="$(dirname "$(rustup which cargo)"):$PATH:${CARGO_HOME:-$HOME/.cargo}/bin"
export PATH
# A conda environment's compiler settings break the link of build scripts.
unset CC CFLAGS LDFLAGS

want=$(sed -n 's/^wasm-bindgen = "=\(.*\)"$/\1/p' crates/ast-plotter-wasm/Cargo.toml)
have=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)
if [ "$have" != "$want" ]; then
  echo "wasm-bindgen-cli $want is needed (found ${have:-none}):" >&2
  echo "  cargo install wasm-bindgen-cli --version $want --locked" >&2
  exit 1
fi

out=target/plotter
mkdir -p "$out"
# Link-time optimisation takes about a quarter off the module. It is set here
# rather than in the workspace's release profile, with a target directory of
# its own, so that other release builds keep their settings and artefacts.
export CARGO_TARGET_DIR="$out/cargo"
export CARGO_PROFILE_RELEASE_LTO=fat
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
wasm="$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/ast_plotter.wasm"

features=()
if [ "$smoke" = 1 ]; then
  features=(--features smoke-test)
fi
cargo build -p ast-plotter-wasm --target wasm32-unknown-unknown --release \
  ${features[@]+"${features[@]}"}

# The examples as a JSON array of {name, settings, version, header}, read from
# examples.txt as the C probe and the Rust tests read it.
python3 - web/plotter/examples.txt "$out/examples.json" <<'EOF'
import json
import sys

examples = []
with open(sys.argv[1], encoding="utf-8") as f:
    for line in f.read().splitlines():
        if line.startswith("== "):
            examples.append(
                {"name": line[3:].strip(), "settings": "", "version": " ", "header": ""}
            )
        elif not examples:
            continue
        elif line.startswith("-- settings:"):
            value = line[len("-- settings:"):]
            examples[-1]["settings"] = value[1:] if value.startswith(" ") else value
        elif line.startswith("-- version:"):
            value = line[len("-- version:"):].lstrip()
            examples[-1]["version"] = value[0] if value else " "
        else:
            examples[-1]["header"] += line + "\n"
with open(sys.argv[2], "w", encoding="utf-8") as f:
    json.dump(examples, f, ensure_ascii=False)
EOF

if [ "$smoke" = 1 ]; then
  rm -rf "$out/node"
  wasm-bindgen --target nodejs --no-typescript --out-dir "$out/node" "$wasm"
  node tools/plotter_smoke.mjs "$out/node" "$out/examples.json"
  exit 0
fi

rm -rf "$out/glue"
wasm-bindgen --target no-modules --no-typescript --out-dir "$out/glue" "$wasm"

# The page with each marker replaced by its file. Text that would close the
# script element it sits in is refused rather than escaped, since only the
# JSON can be escaped without changing what it means.
python3 - "$out" <<'EOF'
import base64
import json
import pathlib
import sys

out = pathlib.Path(sys.argv[1])
web = pathlib.Path("web/plotter")


def script_safe(name, text):
    if "</script" in text.lower():
        sys.exit(f"build_plotter: {name} contains </script")
    return text


examples = json.loads((out / "examples.json").read_text(encoding="utf-8"))
parts = {
    "/*@PLOTTER_CSS@*/": (web / "plotter.css").read_text(encoding="utf-8"),
    "/*@GLUE@*/": script_safe(
        "the wasm-bindgen glue", (out / "glue/ast_plotter.js").read_text(encoding="utf-8")
    ),
    "/*@EXAMPLES@*/": json.dumps(examples, ensure_ascii=False).replace("</", "<\\/"),
    "/*@WASM@*/": base64.b64encode((out / "glue/ast_plotter_bg.wasm").read_bytes()).decode(),
    "/*@PLOTTER_JS@*/": script_safe("plotter.js", (web / "plotter.js").read_text(encoding="utf-8")),
}
page = (web / "index.html").read_text(encoding="utf-8")
for marker, text in parts.items():
    if page.count(marker) != 1:
        sys.exit(f"build_plotter: index.html must hold {marker} once")
    page = page.replace(marker, text)
(out / "fits-plotter.html").write_text(page, encoding="utf-8")
print(f"wrote {out / 'fits-plotter.html'} ({len(page.encode()) // 1024} KiB)")
EOF
