// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Integration tests: Frame, CmpFrame and FrameSet, and Frame attributes and axes.
//!
//! Each module is one test file in the parent directory, built into this one
//! test program so that a change to starlink-ast relinks a few programs rather than
//! one per file.

// Test code computes expected values directly; the counted `math` wrappers
// are for the library (`crates/starlink-ast/clippy.toml`).
#![allow(clippy::disallowed_methods)]

#[path = "../common/mod.rs"]
mod common;

#[path = "../test_active_unit.rs"]
mod test_active_unit;
#[path = "../test_attr_test_clear_oracle.rs"]
mod test_attr_test_clear_oracle;
#[path = "../test_axis_format.rs"]
mod test_axis_format;
#[path = "../test_axis_unformat.rs"]
mod test_axis_unformat;
#[path = "../test_basicframe.rs"]
mod test_basicframe;
#[path = "../test_class_attrs.rs"]
mod test_class_attrs;
#[path = "../test_clear_on_set.rs"]
mod test_clear_on_set;
#[path = "../test_cmp_match_end.rs"]
mod test_cmp_match_end;
#[path = "../test_cmpframe.rs"]
mod test_cmpframe;
#[path = "../test_convert.rs"]
mod test_convert;
#[path = "../test_convert_frameset_oracle.rs"]
mod test_convert_frameset_oracle;
#[path = "../test_equal_oracle.rs"]
mod test_equal_oracle;
#[path = "../test_find_frame.rs"]
mod test_find_frame;
#[path = "../test_frame.rs"]
mod test_frame;
#[path = "../test_frame_api.rs"]
mod test_frame_api;
#[path = "../test_frame_attrs.rs"]
mod test_frame_attrs;
#[path = "../test_frame_axis_attrs.rs"]
mod test_frame_axis_attrs;
#[path = "../test_frame_class_attrs.rs"]
mod test_frame_class_attrs;
#[path = "../test_frame_equality.rs"]
mod test_frame_equality;
#[path = "../test_frame_format.rs"]
mod test_frame_format;
#[path = "../test_frame_geometry.rs"]
mod test_frame_geometry;
#[path = "../test_frame_in_map_slot.rs"]
mod test_frame_in_map_slot;
#[path = "../test_frameset.rs"]
mod test_frameset;
#[path = "../test_frameset_current.rs"]
mod test_frameset_current;
#[path = "../test_frameset_integrity.rs"]
mod test_frameset_integrity;
#[path = "../test_frameset_repair.rs"]
mod test_frameset_repair;
#[path = "../test_frameset_variants.rs"]
mod test_frameset_variants;
#[path = "../test_ident.rs"]
mod test_ident;
#[path = "../test_match_axes.rs"]
mod test_match_axes;
#[path = "../test_min_max_axes.rs"]
mod test_min_max_axes;
#[path = "../test_new_unit.rs"]
mod test_new_unit;
#[path = "../test_norm_points.rs"]
mod test_norm_points;
#[path = "../test_overlay.rs"]
mod test_overlay;
#[path = "../test_picked_skyaxis.rs"]
mod test_picked_skyaxis;
#[path = "../test_system_change.rs"]
mod test_system_change;
#[path = "../test_unit_mapper.rs"]
mod test_unit_mapper;
#[path = "../test_use_defs.rs"]
mod test_use_defs;
