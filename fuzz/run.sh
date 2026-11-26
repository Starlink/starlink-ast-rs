#!/bin/sh
# Copyright 2026 Association of Universities for Research in Astronomy (AURA)
# SPDX-License-Identifier: LGPL-3.0-or-later
#
# Run every fuzz target at once for SECONDS seconds (default 3600) with JOBS
# worker processes each (default 4), recording each crash, timeout and
# out-of-memory in artifacts/<target>/ without stopping. Needs a nightly
# toolchain and cargo-fuzz; run seed.sh first.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
cd "$here"
seconds=${SECONDS_TO_RUN:-3600}
jobs=${JOBS:-4}
nightly=$(dirname "$(rustup which --toolchain nightly rustc)")
export PATH="$nightly:$HOME/.cargo/bin:$PATH" RUSTUP_TOOLCHAIN=nightly
cargo fuzz build --debug-assertions
mkdir -p logs
for spec in native_read:native fitschan_read_write:fits mocchan_read:moc; do
    target=${spec%%:*}
    dict=${spec##*:}
    mkdir -p "artifacts/$target"
    cargo fuzz run --debug-assertions "$target" -- \
        -dict="dict/$dict.dict" -timeout=10 -rss_limit_mb=2048 -malloc_limit_mb=2048 \
        -max_total_time="$seconds" -fork="$jobs" \
        -ignore_crashes=1 -ignore_timeouts=1 -ignore_ooms=1 \
        > "logs/$target.log" 2>&1 &
done
wait
for target in native_read fitschan_read_write mocchan_read; do
    echo "$target: $(ls "artifacts/$target" | wc -l | tr -d ' ') artifacts"
done
