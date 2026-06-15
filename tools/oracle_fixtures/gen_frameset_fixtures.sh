#!/usr/bin/env bash
#
# Regenerate the FrameSet native-text (.ast) round-trip fixtures from the
# reference AST library (the oracle). For every wcsconv `native` test case
# whose FITS header parses to a WCS object containing a CmpFrame, dump the
# object as native text via `head2ast` and write it to the fixture corpus.
#
# The resulting `.ast` files drive
# `native_byte_exact_round_trip_frameset_fixtures` in
# crates/ast-core/tests/test_serialisation.rs — a serializer-only test that
# is independent of the FitsChan read/encode path.
#
# Usage (from repo root or anywhere):
#   tools/oracle_fixtures/gen_frameset_fixtures.sh
#
# Requires the reference library to be built with CMake (never autotools):
#   cmake --build reference/starlink-ast/build --target ast
#
# Fixtures are committed; this script only needs to be re-run when the
# oracle output changes (e.g. after a reference rebuild) or to add cases.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"

ref_build="$repo/reference/starlink-ast/build"
if [ ! -f "$ref_build/libast.so" ]; then
    echo "error: reference libast not built. Run:" >&2
    echo "  cmake --build reference/starlink-ast/build --target ast" >&2
    exit 1
fi

# Build head2ast against the reference library.
make -C "$here" head2ast >/dev/null

probe="$here/head2ast"
inputs="$repo/crates/ast-core/tests/fixtures/wcsconv/inputs"
manifest="$repo/crates/ast-core/tests/fixtures/wcsconv/wcsconv_tests.txt"
outdir="$repo/crates/ast-core/tests/fixtures/native/reference/frameset_fixtures"
mkdir -p "$outdir"

count=0
# Walk the wcsconv manifest: pipe-delimited `name | input | reference |
# encoding | attrs | skip`. Take `native` rows whose input is a `.head`
# that parses to a CmpFrame-containing object.
while IFS='|' read -r name input reference encoding attrs skip; do
    name="$(echo "$name" | xargs)"
    input="$(echo "$input" | xargs)"
    encoding="$(echo "$encoding" | xargs || true)"
    attrs="$(echo "${attrs:-}" | xargs || true)"
    case "$name" in '#'*|'') continue;; esac
    [ "$encoding" = "native" ] || continue
    case "$input" in *.head) ;; *) continue;; esac
    [ -f "$inputs/$input" ] || continue

    out="$("$probe" "$inputs/$input" "${attrs:- }" 2>/dev/null || true)"
    echo "$out" | grep -q "Begin CmpFrame" || continue

    # Fixture name = input header stem.
    stem="${input%.head}"
    printf '%s\n' "$out" > "$outdir/$stem.ast"
    count=$((count + 1))
    echo "wrote $stem.ast"
done < "$manifest"

echo "Generated $count FrameSet .ast fixtures in $outdir"
