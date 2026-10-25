// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! `Region::show_mesh_text` against the C library's `astShowMesh` (region.c
//! `ShowMesh`): the table of current-Frame boundary mesh points, with and
//! without a title, unformatted and formatted by the axis Format, for a
//! Cartesian Circle, a sky Box straddling RA 0, a Polygon whose axes have
//! their own Format, a Box whose values take `%g`'s exponent form, and a
//! Circle whose base->current Mapping is a ZoomMap.
//!
//! Each Region is read from C's dump, and the table is what `astShowMesh`
//! wrote to standard output. The values come from
//! `tools/oracle_fixtures/region_showmesh_probe.c`.

// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_showmesh_oracle/cases.rs"]
mod cases;

use starlink_ast::from_native_string;

#[test]
fn show_mesh_text_matches_c() {
    for &(name, dump, format, ttl, want) in cases::CASES {
        let region = from_native_string(dump).unwrap().into_region().unwrap();
        let got = region.show_mesh_text(format, ttl).unwrap();
        assert_eq!(got, want, "{name}");
    }
}
