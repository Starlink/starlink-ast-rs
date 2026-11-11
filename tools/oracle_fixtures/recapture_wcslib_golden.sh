#!/bin/sh
# Copyright 2026 Association of Universities for Research in Astronomy (AURA)
# SPDX-License-Identifier: LGPL-3.0-or-later

# Recapture the wcslib golden values from the reference C.
#
# crates/starlink-ast/tests/wcslib_golden/projections.tsv holds the results of
# the reference library's wcslib projections (wcslib/proj.c, tpn.c and
# wcstrig.c) at inputs the file itself lists. This builds those sources with
# recapture_wcslib_golden.c and recomputes every result, keeping the inputs.
#
#   ./tools/oracle_fixtures/recapture_wcslib_golden.sh
#        Rewrite the golden file in place.
#   ./tools/oracle_fixtures/recapture_wcslib_golden.sh --check
#        Capture to a temporary file and exit non-zero, with a summary of the
#        differences, if the golden file is not that capture.
#   ./tools/oracle_fixtures/recapture_wcslib_golden.sh --output <path>
#        Write the capture to <path> and leave the golden file alone.
#   ./tools/oracle_fixtures/recapture_wcslib_golden.sh --compare
#        Capture to a temporary file and run the golden test against it, in a
#        debug and in a release build: do the native projections give, bit for
#        bit, what the C gives on this platform?
#
# --golden <path> names the file to read, and to check or rewrite, in place of
# the committed one. AST_REFERENCE names the reference checkout, by default
# reference/starlink-ast, and CC the C compiler, by default cc.
#
# The results are the capture platform's: the projections call its libm, whose
# last bit differs between platforms, and the golden values are those of macOS
# on aarch64. On any other platform only --output and --compare are allowed,
# and the header of a capture names the platform it is from.
#
# The reference checkout is never modified: its sources are copied to a
# scratch directory and built there.
set -e

usage() {
   echo "usage: $0 [--check | --compare | --output <path>] [--golden <path>]" >&2
   exit 2
}

HERE=$(cd "$(dirname "$0")/../.." && pwd)
TOOL=$HERE/tools/oracle_fixtures/recapture_wcslib_golden.c
GOLDEN=$HERE/crates/starlink-ast/tests/wcslib_golden/projections.tsv
SRC=${AST_REFERENCE:-$HERE/reference/starlink-ast}
CHECK=
COMPARE=
OUTPUT=
while [ $# -gt 0 ]; do
   case $1 in
      --check) CHECK=1 ;;
      --compare) COMPARE=1 ;;
      --output) [ $# -ge 2 ] || usage; OUTPUT=$2; shift ;;
      --golden) [ $# -ge 2 ] || usage; GOLDEN=$2; shift ;;
      *) usage ;;
   esac
   shift
done
[ -z "$CHECK$COMPARE" ] || [ -z "$OUTPUT" ] || usage
[ -z "$CHECK" ] || [ -z "$COMPARE" ] || usage

[ -f "$SRC/wcslib/proj.c" ] || { echo "reference submodule not initialised: $SRC" >&2; exit 1; }
[ -f "$GOLDEN" ] || { echo "no golden file: $GOLDEN" >&2; exit 1; }

# The platform as a capture's header names it: the C library too where it is
# glibc, whose results have changed between versions.
PLATFORM=$(uname -sm)
if [ "$PLATFORM" = "Darwin arm64" ]; then
   PLATFORM="macOS aarch64"
elif LIBC=$(getconf GNU_LIBC_VERSION 2>/dev/null); then
   PLATFORM="$PLATFORM with $LIBC"
fi
if [ "$PLATFORM" != "macOS aarch64" ] && [ -z "$OUTPUT$COMPARE" ]; then
   echo "this platform is $PLATFORM: the golden values are those of macOS aarch64," >&2
   echo "so they are neither rewritten nor checked here; --output <path> captures this" >&2
   echo "platform's values to a file of its own, and --compare checks the native" >&2
   echo "projections against them" >&2
   exit 1
fi

# The commit the header of the capture names. It describes the sources only if
# they are as committed, and GIT_OPTIONAL_LOCKS keeps git from refreshing the
# checkout's index while it compares them.
PIN=$(cd "$SRC" && git rev-parse --short=8 HEAD)
( cd "$SRC" && GIT_OPTIONAL_LOCKS=0 git diff --quiet HEAD -- wcslib ) || {
   echo "$SRC/wcslib has uncommitted changes: commit $PIN would not describe the capture" >&2
   exit 1
}

WORK=$(mktemp -d "${TMPDIR:-/tmp}/recapture-wcslib-golden.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
trap 'exit 1' HUP INT TERM

# proj.c includes "../src/memory.h" for astSizeOf, and the reference library's
# memory.h needs the configured library. The copy of proj.c finds this one in
# its place, and the tool defines astSizeOf_.
mkdir "$WORK/wcslib" "$WORK/src"
for f in proj.c proj.h tpn.c wcsmath.h wcstrig.c wcstrig.h; do
   cp "$SRC/wcslib/$f" "$WORK/wcslib/$f"
done
cat > "$WORK/src/memory.h" <<'EOF'
#include <stddef.h>
size_t astSizeOf_( const void *, int * );
#define astSizeOf(ptr) astSizeOf_( ptr, NULL )
EOF

# A fixed optimization level and no floating-point contraction, so that the C
# computes what its source says: the flags the golden file's header records.
CC=${CC:-cc}
FLAGS="-O3 -ffp-contract=off"
for f in proj tpn wcstrig; do
#  shellcheck disable=SC2086
   $CC $FLAGS -w -c "$WORK/wcslib/$f.c" -o "$WORK/$f.o"
done
# shellcheck disable=SC2086
$CC $FLAGS -Wall -Wextra -I "$WORK/wcslib" -c "$TOOL" -o "$WORK/tool.o"
$CC "$WORK/proj.o" "$WORK/tpn.o" "$WORK/wcstrig.o" "$WORK/tool.o" -o "$WORK/recapture" -lm

# The header line that names the commit also names the platform the values are
# from.
NEW=$WORK/projections.tsv
"$WORK/recapture" "$GOLDEN" "$PIN" > "$WORK/capture"
sed "s/^\\(# Captured from .* built .* on \\)[^,]*,\$/\\1$PLATFORM,/" "$WORK/capture" > "$NEW"
[ "$(grep -c "^# Captured from .* built .* on $PLATFORM,\$" "$NEW")" = 1 ] || {
   echo "$GOLDEN has no one header line \"# Captured from ... built ... on <platform>,\"" >&2
   exit 1
}

# Say how far the golden file is from the capture, with the first differences.
summarize() {
   diff "$GOLDEN" "$NEW" > "$WORK/diff" || true
   echo "$(grep -c '^<' "$WORK/diff") of the $(wc -l < "$GOLDEN" | tr -d ' ') lines of $GOLDEN"
   echo "differ from a capture on $PLATFORM with the reference at $PIN; the first:"
   head -n 12 "$WORK/diff"
}

if [ -n "$OUTPUT" ]; then
   cp "$NEW" "$OUTPUT"
   echo "captured on $PLATFORM with the reference at $PIN: wrote $OUTPUT"
elif [ -n "$COMPARE" ]; then
   echo "captured on $PLATFORM with the reference at $PIN"
   TEST=wcslib::golden_tests::projections_match_the_captured_wcslib_values
   for profile in debug release; do
      flag=
      [ "$profile" = debug ] || flag=--release
      echo "building and running the golden test ($profile)"
#     One test must have run and passed: a filter that matches nothing passes too.
#     shellcheck disable=SC2086
      if ! ( cd "$HERE" && AST_WCSLIB_GOLDEN=$NEW cargo test $flag -p starlink-ast --lib "$TEST" -- \
               --exact --include-ignored ) > "$WORK/test.out" 2>&1 ||
            ! grep -q '^test result: ok\. 1 passed' "$WORK/test.out"; then
         cat "$WORK/test.out" >&2
         echo "$profile: the native projections do not give the capture" >&2
         exit 1
      fi
      echo "$profile: the native projections give the capture bit for bit"
   done
elif cmp -s "$GOLDEN" "$NEW"; then
   echo "$GOLDEN is the capture on $PLATFORM with the reference at $PIN"
elif [ -n "$CHECK" ]; then
   summarize >&2
   exit 1
else
   summarize
   cat "$NEW" > "$GOLDEN"
   echo "rewrote $GOLDEN"
fi
