#!/usr/bin/env bash
# Copyright 2026 Association of Universities for Research in Astronomy (AURA)
# SPDX-License-Identifier: LGPL-3.0-or-later

# Run the Rust and the starlink-pyast FITS WCS readers on the same HDU and
# diff what they print.
#
# Usage: compare.sh [--dump] <file.fits> [<hdu>]
#
# PYTHON names the interpreter that has starlink-pyast (default python3).
# Exits 0 when the two outputs are identical.

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$here/../..
python=${PYTHON:-python3}

rust=$(mktemp)
pyast=$(mktemp)
trap 'rm -f "$rust" "$pyast"' EXIT

cargo run -q --manifest-path "$root/Cargo.toml" -p starlink-ast --example fits_wcs -- "$@" > "$rust"
"$python" "$here/fits_wcs.py" "$@" > "$pyast"

if diff -u -L rust -L pyast "$rust" "$pyast"; then
    echo "identical ($(wc -l < "$rust" | tr -d ' ') lines)"
fi
