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
#
# --golden <path> names the file to read, and to check or rewrite, in place of
# the committed one. AST_REFERENCE names the reference checkout, by default
# reference/starlink-ast, and CC the C compiler, by default cc.
#
# The results are the capture platform's: the projections call its libm, whose
# last bit differs between platforms, and the golden values are those of macOS
# on aarch64. On any other platform only --output is allowed.
#
# The reference checkout is never modified: its sources are copied to a
# scratch directory and built there.
set -e

usage() {
   echo "usage: $0 [--check | --output <path>] [--golden <path>]" >&2
   exit 2
}

HERE=$(cd "$(dirname "$0")/../.." && pwd)
TOOL=$HERE/tools/oracle_fixtures/recapture_wcslib_golden.c
GOLDEN=$HERE/crates/starlink-ast/tests/wcslib_golden/projections.tsv
SRC=${AST_REFERENCE:-$HERE/reference/starlink-ast}
CHECK=
OUTPUT=
while [ $# -gt 0 ]; do
   case $1 in
      --check) CHECK=1 ;;
      --output) [ $# -ge 2 ] || usage; OUTPUT=$2; shift ;;
      --golden) [ $# -ge 2 ] || usage; GOLDEN=$2; shift ;;
      *) usage ;;
   esac
   shift
done
[ -z "$CHECK" ] || [ -z "$OUTPUT" ] || usage

[ -f "$SRC/wcslib/proj.c" ] || { echo "reference submodule not initialised: $SRC" >&2; exit 1; }
[ -f "$GOLDEN" ] || { echo "no golden file: $GOLDEN" >&2; exit 1; }

PLATFORM=$(uname -sm)
if [ "$PLATFORM" != "Darwin arm64" ] && [ -z "$OUTPUT" ]; then
   echo "this platform is $PLATFORM: the golden values are those of macOS on aarch64" >&2
   echo "(Darwin arm64), so they are neither rewritten nor checked here; --output <path>" >&2
   echo "captures this platform's values to a file of its own" >&2
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

NEW=$WORK/projections.tsv
"$WORK/recapture" "$GOLDEN" "$PIN" > "$NEW"

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
