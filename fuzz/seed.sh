#!/bin/sh
# Copyright 2026 Association of Universities for Research in Astronomy (AURA)
# SPDX-License-Identifier: LGPL-3.0-or-later
#
# Fill corpus/<target>/ with the test fixtures each target starts from:
# the Native dumps, the FITS headers, and a few MOCs in each form.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
fix="$here/../crates/starlink-ast/tests/fixtures"
corpus="$here/corpus"
mkdir -p "$corpus/native_read" "$corpus/fitschan_read_write" "$corpus/mocchan_read"

# A Native dump has a Begin line; file names are made unique by their path.
find "$fix" \( -name '*.ast' -o -name '*.native' \) -type f | while read -r f; do
    if grep -q '^ *Begin ' "$f"; then
        cp "$f" "$corpus/native_read/$(echo "${f#"$fix"/}" | tr / _)"
    fi
done
find "$fix" \( -name '*.head' -o -name '*.fits-*' \) -type f | while read -r f; do
    cp "$f" "$corpus/fitschan_read_write/$(echo "${f#"$fix"/}" | tr / _)"
done

i=0
for moc in '1/3 2/37,101-103' '6/500 7/997,1000-1002 9/' '0/0-11' '29/0' '3/1 2/7' \
    '{"6":[500],"7":[997,1000,1001,1002],"9":[]}' '{"1":[3],"2":[37,101,102,103]}' '{}' ''; do
    printf '%s\n' "$moc" > "$corpus/mocchan_read/seed$i"
    i=$((i + 1))
done
for t in native_read fitschan_read_write mocchan_read; do
    echo "$t: $(ls "$corpus/$t" | wc -l | tr -d ' ') seeds"
done
