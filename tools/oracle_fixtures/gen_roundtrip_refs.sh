#!/bin/bash
# Capture the C oracle's own native round-trip of every simplify fixture.
#
# A `.map` / `.simp` reference is C's dump of a *simplified* object. Reading it
# back is not the identity: a loader that validates an attribute re-applies it
# through the private setter, which for a clear-on-set class discards the
# simplified record `astLoadMapping` installed from the `IsSimp` card (see
# `docs/issues/c-library-quirks.md`). So the reference for a round-trip test is
# C's dump of the *loaded* object, which for those fixtures differs.
#
# This writes `<fixture>.rt` for each fixture where it differs, and removes a
# stale one where it no longer does. A difference in anything but an `IsSimp`
# line is refused rather than captured: it means either a real serialisation
# divergence or a fixture holding more objects than `ast2ast` reads, and both
# need reading before they become a reference.
#
# Requires the submodule and a built oracle:
#   git submodule update --init reference/starlink-ast
#   (build libast with cmake, then) make -C tools/oracle_fixtures ast2ast
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
corpus=$here/../../crates/ast-core/tests/fixtures/native/reference/simplify_fixtures
probe=$here/ast2ast
[[ -x $probe ]] || { echo "build $probe first: make -C tools/oracle_fixtures ast2ast" >&2; exit 2; }

tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
written=0 removed=0 refused=0

for fixture in "$corpus"/*.map "$corpus"/*.simp; do
    [[ -e $fixture ]] || continue
    name=$(basename "$fixture")
    if ! "$probe" "$fixture" > "$tmp/out" 2> "$tmp/err"; then
        echo "REFUSED $name: astRead failed: $(head -1 "$tmp/err")" >&2
        refused=$((refused + 1))
        continue
    fi
    # Trailing blank lines are not part of either dump's content.
    if [[ "$(sed -e :a -e '/^\n*$/{$d;N;};/\n$/ba' "$fixture")" \
       == "$(sed -e :a -e '/^\n*$/{$d;N;};/\n$/ba' "$tmp/out")" ]]; then
        if [[ -e $fixture.rt ]]; then
            rm "$fixture.rt"
            echo "removed $name.rt (C now round-trips it unchanged)"
            removed=$((removed + 1))
        fi
        continue
    fi
    if diff "$fixture" "$tmp/out" | grep -E '^[<>]' | grep -qv 'IsSimp'; then
        echo "REFUSED $name: C's round-trip differs in something other than an \
IsSimp card -- read the diff before capturing it" >&2
        refused=$((refused + 1))
        continue
    fi
    cp "$tmp/out" "$fixture.rt"
    written=$((written + 1))
done

echo "round-trip references: $written written, $removed removed, $refused refused"
[[ $refused -eq 0 ]]
