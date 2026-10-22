#!/bin/bash
# Fail if docs/audits/c-docs/c-headers.tsv differs from the headers in the
# pinned reference source, including a header whose text changed (its digest
# column). Run after every move of reference/starlink-ast.
#
# The source is read from the commit the superproject pins, not from whatever
# the submodule has checked out, so a stale or advanced checkout cannot change
# the answer.
set -euo pipefail
cd "$(dirname "$0")/../.."
pin=$(git rev-parse HEAD:reference/starlink-ast)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
if ! git -C reference/starlink-ast archive "$pin" src | tar -x -C "$work"; then
    echo "pinned commit $pin is not in reference/starlink-ast: run git submodule update --init reference/starlink-ast" >&2
    exit 1
fi
fresh="$work/c-headers.tsv"
cargo run -q -p audit-tools --bin c-docs -- headers "$work/src" > "$fresh"
if ! diff -u docs/audits/c-docs/c-headers.tsv "$fresh"; then
    echo "c-headers.tsv is stale against pin $pin: regenerate it, add ledger rows for new headers and re-review the rows whose digest changed" >&2
    exit 1
fi
