#!/bin/sh
# Copyright 2026 Association of Universities for Research in Astronomy (AURA)
# SPDX-License-Identifier: LGPL-3.0-or-later

# Recapture the PAL and ERFA golden values from the reference C.
#
# crates/starlink-ast/tests/erfa_pal_golden/roots.tsv records, one call per
# line, the inputs of a PAL or ERFA routine and the outputs C gave for them.
# This builds the reference checkout's PAL and ERFA, calls every routine again
# on the file's own inputs, and writes the file with the outputs it got and
# with the reference commit in its header. Nothing else in the file changes.
#
#   ./tools/oracle_fixtures/recapture_erfa_pal_golden.sh [--check | --output <path>] [--golden <path>]
#
#   (no option)       rewrite the golden file in place
#   --check           write nothing; exit non-zero, with a summary of the lines
#                     that differ, if the golden file is not what C gives
#   --output <path>   leave the golden file alone and write the capture to <path>
#   --golden <path>   read, and check or rewrite, <path> in place of the committed file
#
# AST_REFERENCE names a reference checkout other than reference/starlink-ast.
# The checkout is never written: the objects go to a temporary directory.
#
# The values depend on the platform's libm, and the committed ones are macOS
# aarch64's. On any other platform only --output is accepted.
set -e
HERE=$(cd "$(dirname "$0")/../.." && pwd)
TOOLS=$HERE/tools/oracle_fixtures
SRC=${AST_REFERENCE:-$HERE/reference/starlink-ast}
GOLDEN=$HERE/crates/starlink-ast/tests/erfa_pal_golden/roots.tsv
CAPTURE_PLATFORM="Darwin arm64"

usage() {
   echo "usage: $0 [--check | --output <path>] [--golden <path>]" >&2
   exit 2
}

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
[ -f "$GOLDEN" ] || { echo "no golden file $GOLDEN" >&2; exit 1; }

PLATFORM="$(uname -s) $(uname -m)"
if [ "$PLATFORM" != "$CAPTURE_PLATFORM" ] && [ -z "$OUTPUT" ]; then
   echo "the golden values are those of $CAPTURE_PLATFORM and this is $PLATFORM:" >&2
   echo "only --output <path> is accepted here" >&2
   exit 1
fi

[ -f "$SRC/palwrap.c" ] || { echo "reference submodule not initialised: $SRC" >&2; exit 1; }

# The commit the header will name. Inside a directory that is not the top of
# its own checkout git answers for the enclosing repository, and with local
# changes to the sources the commit would not be the one they came from.
if ! PREFIX=$(git -C "$SRC" rev-parse --show-prefix 2>/dev/null) || [ -n "$PREFIX" ]; then
   echo "$SRC is not the top of a git checkout" >&2
   exit 1
fi
if ! GIT_OPTIONAL_LOCKS=0 git -C "$SRC" diff --quiet HEAD -- \
      pal erfa palwrap.c pal.h erfa.h pal2ast.h erfa2ast.h; then
   echo "$SRC has uncommitted changes to its PAL or ERFA sources" >&2
   exit 1
fi
PIN=$(git -C "$SRC" rev-parse --short=8 HEAD)

BUILD=$(mktemp -d)
trap 'rm -rf "$BUILD"' EXIT

# palwrap.c is the single translation unit AST builds PAL and ERFA from, under
# their astPal and astEra names. The flags are the ones the golden file's
# header records: both change results. The capture program is compiled
# separately, so that no routine is inlined into its caller and specialised
# for it, which a library build never does.
CC=${CC:-cc}
"$CC" -O3 -ffp-contract=off -w -I "$SRC" -I "$SRC/pal" -I "$SRC/erfa" \
   -c "$SRC/palwrap.c" -o "$BUILD/palwrap.o"
"$CC" -O3 -ffp-contract=off -Wall -Wextra -I "$SRC" \
   -c "$TOOLS/recapture_erfa_pal_golden.c" -o "$BUILD/recapture.o"
"$CC" "$BUILD/recapture.o" "$BUILD/palwrap.o" -lm -o "$BUILD/recapture"

FRESH=$BUILD/roots.tsv
"$BUILD/recapture" "$GOLDEN" "$PIN" > "$FRESH"

# The lines of $FRESH that are not the golden file's: how many, where the
# first few are, and the count for each root. The two files have the same
# lines in the same order, differing only after a line's "=" or in the header.
summarize() {
   awk '
      FNR == NR { golden[FNR] = $0; next }
      golden[FNR] != $0 {
         root = ( $0 ~ /^#/ ) ? "(header)" : $1
         if( !( root in count ) ) roots[++nroots] = root
         count[root]++
         if( ++n <= 5 ) first = first " " FNR
      }
      END {
         printf "%d of %d lines differ, at line%s%s\n", n, FNR, first, ( n > 5 ) ? " ..." : ""
         for( i = 1; i <= nroots; i++ ) printf "   %-8s %d\n", roots[i], count[roots[i]]
      }' "$GOLDEN" "$FRESH"
}

if [ -n "$OUTPUT" ]; then
   cp "$FRESH" "$OUTPUT"
   echo "wrote $OUTPUT: captured on $PLATFORM from reference/starlink-ast at $PIN"
   cmp -s "$GOLDEN" "$FRESH" || summarize
elif cmp -s "$GOLDEN" "$FRESH"; then
   echo "$GOLDEN is what reference/starlink-ast at $PIN gives"
elif [ -n "$CHECK" ]; then
   echo "$GOLDEN is not what reference/starlink-ast at $PIN gives:"
   summarize
   echo "run without --check to rewrite it, or with --output <path> to compare the capture"
   exit 1
else
   summarize
   cp "$FRESH" "$GOLDEN"
   echo "rewrote $GOLDEN from reference/starlink-ast at $PIN"
fi
