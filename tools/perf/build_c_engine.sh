#!/bin/sh
# Copyright 2026 Association of Universities for Research in Astronomy (AURA)
# SPDX-License-Identifier: LGPL-3.0-or-later

# Build the reference C library as a runtime-loadable engine for ast-bench,
# with the programs that go with it.
#
#   tools/perf/build_c_engine.sh bench       # timing build
#   tools/perf/build_c_engine.sh count       # counts calls into libm, PAL, ERFA and wcslib
#   tools/perf/build_c_engine.sh nothreads   # the timing build without pthreads
#
# Output: target/perf/c/<variant>/libast_bench_engine.{dylib,so} and
# build-info.txt for bench and count.  bench also writes gen_bench_fixtures,
# gen_roman_fixtures and prof_tran; nothreads writes prof_tran alone, since
# ast-bench times only the thread-safe library.
# The submodule is never modified: the build works on a copy.
set -e
USAGE="usage: $0 bench|count|nothreads"
VARIANT=${1:?$USAGE}
case "$VARIANT" in bench|count|nothreads) ;; *) echo "$USAGE"; exit 2 ;; esac
HERE=$(cd "$(dirname "$0")/../.." && pwd)
SRC=$HERE/reference/starlink-ast
DEST=$HERE/target/perf/c/$VARIANT
[ -d "$SRC/src" ] || { echo "reference submodule not initialised"; exit 1; }
# Rust never fuses a multiply and an add, so neither may C.
CFLAGS_PINNED="-O3 -ffp-contract=off"

rm -rf "$DEST"; mkdir -p "$DEST/src"
( cd "$SRC" && git ls-files ) > "$DEST/.files"
( cd "$SRC" && tar cf - -T "$DEST/.files" ) | ( cd "$DEST/src" && tar xf - )

PTHREADS=ON
if [ "$VARIANT" = nothreads ]; then PTHREADS=OFF; fi
EXTRA=""
if [ "$VARIANT" = count ]; then
   python3 "$HERE/tools/perf/gen_count_header.py" "$DEST/src"
   # Count calls made by AST's own sources only, never by the PAL, ERFA or
   # wcslib sources: those are the same code in both engines.
   for f in "$DEST"/src/src/*.c; do
      { printf '#include "ast_counts.h"\n'; cat "$f"; } > "$f.tmp" && mv "$f.tmp" "$f"
   done
   # Projection evaluations are reached through function pointers.
   sed -i.bak -e 's/prjdata->WcsFwd(/( ast_counts_hit( "wcs_fwd" ), prjdata->WcsFwd )(/g' \
              -e 's/prjdata->WcsRev(/( ast_counts_hit( "wcs_rev" ), prjdata->WcsRev )(/g' \
              "$DEST/src/src/wcsmap.c"
   EXTRA="-DAST_BENCH_COUNTS $DEST/src/ast_counts.c"
fi

cmake -S "$DEST/src" -B "$DEST/build" -DCMAKE_BUILD_TYPE=Release -DAST_BUILD_FORTRAN=OFF \
   -DAST_WITH_PTHREADS=$PTHREADS \
   -DCMAKE_C_FLAGS_RELEASE="$CFLAGS_PINNED" -DCMAKE_C_FLAGS="-I$DEST/src" \
   -DCMAKE_POSITION_INDEPENDENT_CODE=ON > "$DEST/cmake.log" 2>&1 \
   || { tail -25 "$DEST/cmake.log"; exit 1; }
LIBS="ast ast_pal ast_cminpack ast_err ast_grf_2.0 ast_grf_3.2 ast_grf_5.6 ast_grf3d"
for t in $LIBS; do
   cmake --build "$DEST/build" --target "$t-static" -j8 >> "$DEST/build.log" 2>&1 \
      || { tail -25 "$DEST/build.log"; exit 1; }
done

case "$(uname -s)" in
   Darwin) LIB=libast_bench_engine.dylib; SHARED="-dynamiclib" ;;
   *)      LIB=libast_bench_engine.so;    SHARED="-shared -Wl,-Bsymbolic" ;;
esac
YAML=$(ls -d /opt/homebrew/opt/libyaml/lib /usr/local/opt/libyaml/lib 2>/dev/null | head -1)
B=$DEST/build
ARCHIVES=""
for t in $LIBS; do ARCHIVES="$ARCHIVES $B/lib$t.a"; done
if [ "$VARIANT" != nothreads ]; then
   # shellcheck disable=SC2086
   cc $CFLAGS_PINNED $SHARED -fPIC -o "$DEST/$LIB" \
      -I "$B" -I "$DEST/src" -I "$DEST/src/ast_tester" -I "$HERE/crates/ast-bench/c" \
      "$HERE/crates/ast-bench/c/bench_engine.c" "$DEST/src/ast_tester/transform_oracle_util.c" \
      $EXTRA $ARCHIVES ${YAML:+-L "$YAML"} -lyaml -lm
fi

if [ "$VARIANT" != count ]; then
   # The harness that decomposes the fixed cost of a one-point astTranN.
   # shellcheck disable=SC2086
   cc $CFLAGS_PINNED -o "$DEST/prof_tran" \
      -I "$B" -I "$DEST/src" -I "$DEST/src/ast_tester" \
      "$HERE/tools/perf/prof_tran.c" "$DEST/src/ast_tester/transform_oracle_util.c" \
      $ARCHIVES ${YAML:+-L "$YAML"} -lyaml -lm
fi

if [ "$VARIANT" = bench ]; then
   # The program that writes ast-bench's fixtures and their oracle.
   # shellcheck disable=SC2086
   cc $CFLAGS_PINNED -o "$DEST/gen_bench_fixtures" \
      -I "$B" -I "$DEST/src" -I "$DEST/src/ast_tester" \
      "$HERE/tools/perf/gen_bench_fixtures.c" "$DEST/src/ast_tester/transform_oracle_util.c" \
      $ARCHIVES ${YAML:+-L "$YAML"} -lyaml -lm
   # The program that writes the Roman benchmark's fixtures.
   # shellcheck disable=SC2086
   cc $CFLAGS_PINNED -o "$DEST/gen_roman_fixtures" \
      -I "$B" -I "$DEST/src" "$HERE/tools/perf/gen_roman_fixtures.c" \
      $ARCHIVES ${YAML:+-L "$YAML"} -lyaml -lm
fi

{ echo "variant=$VARIANT"; echo "cc=$(cc --version | head -1)"; echo "cflags=$CFLAGS_PINNED";
  echo "pthreads=$PTHREADS"; echo "submodule=$(git -C "$SRC" rev-parse HEAD)"; } > "$DEST/build-info.txt"
if [ "$VARIANT" = nothreads ]; then echo "built $DEST/prof_tran"; else echo "built $DEST/$LIB"; fi
