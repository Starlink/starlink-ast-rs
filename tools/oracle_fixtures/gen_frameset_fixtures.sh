#!/usr/bin/env bash
#
# Regenerate the FrameSet native-text (.ast) round-trip fixtures from the
# reference AST library (the oracle). For every distinct WCS object the
# wcsconv test harness parses (one per input file in the manifest), dump the
# object as native text and write it to the fixture corpus. This is a
# comprehensive serialization regression suite: every object the harness
# exercises must round-trip byte-exact through the Rust serializer.
#
# Each object is dumped in native AST text — the exact serialization path
# under test — and read two ways by input extension:
#   *.ast            -> native AST text, read+re-dumped through a Channel
#                       (`ast2ast`); these pre-existing native inputs (e.g.
#                       degen1, lsst, timj) round-trip the serializer on
#                       objects FITS headers cannot produce.
#   everything else  -> FITS header read through a FitsChan (`head2ast`)
#                       (.head, .fits-dss, .fits-class, .fits-pc).
# Read-affecting attributes from the manifest (carlin / sipreplace / SipOK)
# are passed through; output-only attrs (cdmatrix / FitsDigits / fitsrounding)
# do not change the parsed object and are inert here.
#
# Fixtures are keyed by the full input filename with `.` -> `-` (e.g.
# `axes-swap-head.ast`, `dss-ast.ast`, `dss-fits-dss.ast`) so the two `dss.*`
# inputs do not collide.
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

# Build both probes against the reference library.
make -C "$here" head2ast ast2ast >/dev/null

head2ast="$here/head2ast"
ast2ast="$here/ast2ast"
inputs="$repo/crates/ast-core/tests/fixtures/wcsconv/inputs"
manifest="$repo/crates/ast-core/tests/fixtures/wcsconv/wcsconv_tests.txt"
outdir="$repo/crates/ast-core/tests/fixtures/native/reference/frameset_fixtures"

# Regenerate from scratch so removed/renamed cases don't linger.
rm -rf "$outdir"
mkdir -p "$outdir"

count=0
# Walk the wcsconv manifest: pipe-delimited `name | input | reference |
# encoding | attrs | skip`. Emit one fixture per distinct input file (the
# parsed object is independent of the output encoding); the first manifest
# row seen for an input supplies its read-affecting attributes.
declare -A seen
while IFS='|' read -r name input reference encoding attrs skip; do
    name="$(echo "$name" | xargs)"
    input="$(echo "$input" | xargs)"
    attrs="$(echo "${attrs:-}" | xargs || true)"
    case "$name" in '#'*|'') continue;; esac
    [ -n "$input" ] || continue
    [ -f "$inputs/$input" ] || continue
    [ -z "${seen[$input]:-}" ] || continue
    seen[$input]=1

    # Read native AST text (.ast) through a Channel; FITS through a FitsChan.
    case "$input" in
        *.ast) out="$("$ast2ast" "$inputs/$input" 2>/dev/null || true)" ;;
        *)     out="$("$head2ast" "$inputs/$input" "${attrs:- }" 2>/dev/null || true)" ;;
    esac
    if [ -z "$out" ]; then
        echo "warning: empty dump for $input (skipped)" >&2
        continue
    fi

    # Fixture name = input filename with `.` -> `-`.
    fixture="${input//./-}.ast"
    printf '%s\n' "$out" > "$outdir/$fixture"
    count=$((count + 1))
    echo "wrote $fixture"
done < "$manifest"

echo "Generated $count FrameSet .ast fixtures in $outdir"
