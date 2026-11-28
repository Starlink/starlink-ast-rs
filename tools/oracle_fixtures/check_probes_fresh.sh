#!/usr/bin/env bash
# Copyright 2026 Association of Universities for Research in Astronomy (AURA)
# SPDX-License-Identifier: LGPL-3.0-or-later
#
# Is each generated C-probe oracle still what the pinned C library prints?
#
# Builds every probe listed in probes.tsv (or only those named) against the
# reference library, runs it as its header documents, and compares what it
# prints with the committed crates/starlink-ast/tests/*_oracle/cases.rs.
# Prints "<probe> SAME", "<probe> DIFFERS <n> lines" or "<probe> FAILED <why>"
# for each, and exits 1 if any differs or fails.  A probe whose drift
# probes.tsv records as known prints "<probe> KNOWN <reason>" instead of
# DIFFERS, and "<probe> FIXED" (a failure) once it matches again, so that the
# known entry is removed.
#
#   check_probes_fresh.sh [--write | --capture <dir>] [probe...]
#
# A probe counts as SAME if its output, or that output after rustfmt, is
# the committed file, since cargo fmt reformats the generated files it is
# not told to skip.  --write replaces each output with what the probe
# printed instead of comparing; run cargo fmt after it.  --capture writes what each probe prints to <dir>/<probe>
# instead; capturing at two pins and diffing the directories shows what
# moving the pin changed, apart from any drift the committed files already
# had (a hand edit, or a probe that prints the time).
#
# AST_REFERENCE names a reference checkout other than reference/starlink-ast,
# already built with cmake into its build/ directory; it is how the script
# is checked against an older pin.

set -u

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
ref=$(cd "${AST_REFERENCE:-$root/reference/starlink-ast}" && pwd)
fixtures="$root/crates/starlink-ast/tests/fixtures"

# A conda environment's compiler settings break these links.
unset CC CFLAGS CPPFLAGS LDFLAGS CXX

usage() {
    echo "usage: check_probes_fresh.sh [--write | --capture <dir>] [probe...]" >&2
    exit 2
}

write=0
capture=
if [ "${1:-}" = "--write" ]; then
    write=1
    shift
elif [ "${1:-}" = "--capture" ]; then
    [ $# -ge 2 ] && [ -n "$2" ] || usage
    capture=$2
    mkdir -p "$capture"
    shift 2
fi
case ${1:-} in -*) usage ;; esac

if [ ! -f "$ref/build/libast.a" ] && [ ! -f "$ref/build/libast.dylib" ] && [ ! -f "$ref/build/libast.so" ]; then
    echo "check_probes_fresh: no built library in $ref/build" >&2
    exit 2
fi

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

# The error library defines astPutErr, which nothing in the probe calls
# directly, so the linker must be told to load all of it.
case $(uname -s) in
Darwin) errlib=(-Wl,-force_load,libast_err.a) ;;
*) errlib=(-Wl,--whole-archive libast_err.a -Wl,--no-whole-archive) ;;
esac

# The compiler lines the probes' headers document, run in $ref/build.
build_probe() {
    local probe=$1 recipe=$2
    case $recipe in
    make)
        make -s -B -C "$here" REF="$ref" "$probe" ;;
    fast)
        (cd "$ref/build" && cc -ffp-contract=off -O3 -I. -I../src "$here/$probe.c" \
            -L. -last "${errlib[@]}" -lm -Wl,-rpath,"$PWD" \
            -o "$here/$probe") ;;
    grflog)
        (cd "$ref/build" && cc -ffp-contract=off -O3 -DHAVE_CONFIG_H -I. -Isrc -I../src -I.. \
            "$here/$probe.c" ../src/grf_log.c \
            -L. -last "${errlib[@]}" -lm -Wl,-rpath,"$PWD" \
            -o "$here/$probe") ;;
    grflog-ownerr)
        # The probe defines its own astPutErr, so the error library is not loaded.
        (cd "$ref/build" && cc -ffp-contract=off -O3 -DHAVE_CONFIG_H -I. -Isrc -I../src -I.. \
            "$here/$probe.c" ../src/grf_log.c \
            -L. -last -lm -Wl,-rpath,"$PWD" -o "$here/$probe") ;;
    plain)
        (cd "$ref/build" && cc "$here/$probe.c" -I. -L. -last -lm -Wl,-rpath,"$PWD" \
            -o "$here/$probe") ;;
    ctests)
        # The probe includes each C test in turn; build_<probe>.sh builds and
        # runs them, so the program run is a copy of that script.
        cp "$here/build_$probe.sh" "$here/$probe" && chmod +x "$here/$probe" ;;
    *)
        echo "unknown build recipe $recipe" >&2
        return 1 ;;
    esac
}

fixture_list() {
    # The C locale keeps the table order the same on every host.
    (cd "$fixtures" && find . -type f \( -name '*.map' -o -name '*.simp' -o -name '*.ast' \
        -o -name '*.native' \) | LC_ALL=C sort)
}

status=0
seen=" "
while IFS=$'\t' read -r probe recipe run_dir env stdin output known; do
    case $probe in ''|'#'*) continue ;; esac
    if [ $# -gt 0 ]; then
        wanted=0
        for name in "$@"; do [ "$name" = "$probe" ] && wanted=1; done
        [ $wanted = 1 ] || continue
    fi
    seen="$seen$probe "

    if ! build_probe "$probe" "$recipe" >"$scratch/build.log" 2>&1; then
        echo "$probe FAILED build"
        sed 's/^/    /' "$scratch/build.log" | head -5
        status=1
        continue
    fi

    case $run_dir in
    tools) dir=$here ;;
    build) dir=$ref/build ;;
    fixtures) dir=$fixtures ;;
    *) echo "$probe FAILED unknown run_dir $run_dir"; status=1; continue ;;
    esac

    envs=()
    [ "$env" != "-" ] && envs=("$env")

    got="$scratch/$probe.out"
    if [ "$stdin" = "fixtures-list" ]; then
        fixture_list >"$scratch/stdin"
    else
        : >"$scratch/stdin"
    fi
    if ! (cd "$dir" && env "${envs[@]+"${envs[@]}"}" "$here/$probe" <"$scratch/stdin" >"$got" 2>/dev/null); then
        echo "$probe FAILED run"
        status=1
        continue
    fi

    if [ -n "$capture" ]; then
        cp "$got" "$capture/$probe"
        echo "$probe CAPTURED"
    elif [ $write = 1 ]; then
        cp "$got" "$root/$output"
        echo "$probe WROTE $output"
    elif cmp -s "$got" "$root/$output" || {
        # cargo fmt reformats the generated files it is not told to skip.
        cp "$got" "$scratch/fmt.rs" && rustfmt --edition 2024 "$scratch/fmt.rs" 2>/dev/null \
            && cmp -s "$scratch/fmt.rs" "$root/$output"
    }; then
        if [ "$known" != "-" ]; then
            echo "$probe FIXED: remove its known entry in probes.tsv ($known)"
            status=1
        else
            echo "$probe SAME"
        fi
    elif [ "$known" != "-" ]; then
        echo "$probe KNOWN $known"
    else
        # The committed file may be the raw output or its rustfmt form, so
        # the count is against whichever is nearer.
        n=$(diff "$root/$output" "$got" | grep -c '^[<>]')
        if rustfmt --edition 2024 "$scratch/fmt.rs" 2>/dev/null; then
            m=$(diff "$root/$output" "$scratch/fmt.rs" | grep -c '^[<>]')
            [ "$m" -lt "$n" ] && n=$m
        fi
        echo "$probe DIFFERS $n lines"
        status=1
    fi
done <"$here/probes.tsv"

for name in "$@"; do
    case $seen in
    *" $name "*) ;;
    *) echo "$name FAILED not in probes.tsv"; status=1 ;;
    esac
done

exit $status
