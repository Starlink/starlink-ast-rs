// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

#![no_main]

use libfuzzer_sys::fuzz_target;

#[allow(dead_code)]
#[path = "../../crates/starlink-ast/tests/fuzz_entry/mod.rs"]
mod fuzz_entry;

fuzz_target!(|data: &[u8]| fuzz_entry::mocchan_read(data));
