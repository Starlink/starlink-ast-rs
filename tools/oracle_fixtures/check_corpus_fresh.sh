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
#  Build the project's own wcsconverter target rather than hand-linking it.
#  libast does not resolve the Grf entry points on its own -- the project supplies
#  them from its grf stub libraries -- so a hand-rolled `cc ... -last` link fails,
#  and would need updating again whenever that link line changes.
   cmake --build "$DEST/build" --target wcsconverter -j8 > "$DEST/build.log" 2>&1
   cp "$DEST/build/ast_tester/wcsconverter" "$DEST/wcsconverter"
fi

exec python3 "$HERE/tools/oracle_fixtures/check_corpus_fresh.py" \
   "$HERE/crates/starlink-ast/tests/fixtures" "$DEST/wcsconverter" "$DEST/work"
