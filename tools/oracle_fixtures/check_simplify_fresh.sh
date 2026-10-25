#!/bin/sh
# Copyright 2026 Association of Universities for Research in Astronomy (AURA)
# SPDX-License-Identifier: LGPL-3.0-or-later

# Check every simplify reference against the pinned C oracle.
#
# Card-only drift means a reference predates a library change and must be
# regenerated; it exits non-zero. Numeric drift means the reference was
# captured on another platform and is reported but tolerated. Structural drift
# should never happen and exits non-zero.
#
#   ./tools/oracle_fixtures/check_simplify_fresh.sh
set -e
HERE=$(cd "$(dirname "$0")/../.." && pwd)
SRC=$HERE/reference/starlink-ast
[ -d "$SRC/src" ] || { echo "reference submodule not initialised"; exit 1; }

make -C "$HERE/tools/oracle_fixtures" gen_simp >/dev/null

exec python3 "$HERE/tools/oracle_fixtures/check_simplify_fresh.py" \
   "$HERE/crates/starlink-ast/tests/fixtures" \
   "$HERE/tools/oracle_fixtures/gen_simp" \
   "${TMPDIR:-/tmp}/ast-simplify-oracle"
