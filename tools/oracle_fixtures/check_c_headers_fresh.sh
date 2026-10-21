#!/bin/bash
# Fail if docs/audits/c-docs/c-headers.tsv differs from the headers in the
# pinned reference source, including a header whose text changed (its digest
# column). Run after every move of reference/starlink-ast.
set -euo pipefail
cd "$(dirname "$0")/../.."
fresh=$(mktemp)
trap 'rm -f "$fresh"' EXIT
cargo run -q -p audit-tools --bin c-docs -- headers reference/starlink-ast/src > "$fresh"
if ! diff -u docs/audits/c-docs/c-headers.tsv "$fresh"; then
    echo "c-headers.tsv is stale: regenerate it, add ledger rows for new headers and re-review the rows whose digest changed" >&2
    exit 1
fi
