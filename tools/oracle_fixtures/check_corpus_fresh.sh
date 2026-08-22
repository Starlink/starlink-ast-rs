#!/bin/sh
# Check every native wcsconv reference against the pinned C oracle.
#
# Card-only drift means a reference predates a library change and must be
# regenerated; it exits non-zero. Numeric drift means the reference was
# captured on another platform and is reported but tolerated. Structural drift
# should never happen and exits non-zero.
#
#   ./tools/oracle_fixtures/check_corpus_fresh.sh [build-dir]
set -e
HERE=$(cd "$(dirname "$0")/../.." && pwd)
DEST=${1:-${TMPDIR:-/tmp}/ast-corpus-oracle}
SRC=$HERE/reference/starlink-ast
[ -d "$SRC/src" ] || { echo "reference submodule not initialised"; exit 1; }

if [ ! -x "$DEST/wcsconverter" ]; then
   rm -rf "$DEST"; mkdir -p "$DEST"
   ( cd "$SRC" && git ls-files ) > "$DEST/.files"
   ( cd "$SRC" && tar cf - -T "$DEST/.files" ) | ( cd "$DEST" && tar xf - )
   cmake -S "$DEST" -B "$DEST/build" > "$DEST/cmake.log" 2>&1
   cmake --build "$DEST/build" --target ast -j8 > "$DEST/build.log" 2>&1
   YAML=$(ls -d /opt/homebrew/opt/libyaml/lib /usr/local/opt/libyaml/lib 2>/dev/null | head -1)
   cc "$DEST/ast_tester/wcsconverter.c" -o "$DEST/wcsconverter" \
      -I "$DEST/build" -L "$DEST/build" ${YAML:+-L "$YAML"} \
      -Wl,-rpath,"$DEST/build" ${YAML:+-Wl,-rpath,"$YAML"} -last -lyaml -lm
fi

exec python3 "$HERE/tools/oracle_fixtures/check_corpus_fresh.py" \
   "$HERE/crates/ast-core/tests/fixtures/wcsconv" "$DEST/wcsconverter" "$DEST/work"
