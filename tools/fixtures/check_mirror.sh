#!/bin/bash
# Copyright 2026 Association of Universities for Research in Astronomy (AURA)
# SPDX-License-Identifier: LGPL-3.0-or-later

# The Rust fixture tree must be a byte-identical copy of the upstream
# ast_tester/fixtures/ tree at the current submodule pin.
#
# A recursive diff catches both directions at once: an upstream fixture that was
# never copied here, and a local fixture that was never upstreamed.  That is the
# whole invariant, so this is the whole gate.
#
# It needs the submodule, so it runs in the nightly job that initialises it,
# alongside tiers 3 and 4, and hard-fails there.  Skipping it when the submodule
# is absent loses nothing a tier-1 test proves: the fixtures are committed here
# and every harness reads the committed copies.  What this measures is drift from
# upstream, not test coverage.
set -uo pipefail

here=$(cd "$(dirname "$0")/../.." && pwd)
ours=$here/crates/starlink-ast/tests/fixtures
theirs=$here/reference/starlink-ast/ast_tester/fixtures

if [[ ! -d $theirs ]]; then
    echo "reference/starlink-ast not initialised; run:" >&2
    echo "  git submodule update --init reference/starlink-ast" >&2
    exit 2
fi

if diff -r "$theirs" "$ours"; then
    pin=$(git -C "$here/reference/starlink-ast" rev-parse --short HEAD)
    echo "fixture tree matches upstream at $pin"
    exit 0
fi

echo >&2
echo "fixture tree differs from upstream." >&2
echo "  'Only in $theirs' names an upstream fixture that was not copied here." >&2
echo "  'Only in $ours' names a local fixture that was never upstreamed." >&2
exit 1
