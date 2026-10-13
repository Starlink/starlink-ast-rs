//! Integration tests: Channel serialisation, KeyMap and native-format round trips.
//!
//! Each module is one test file in the parent directory, built into this one
//! test program so that a change to ast-core relinks a few programs rather than
//! one per file.

#[path = "../common/mod.rs"]
mod common;

#[path = "../test_channel.rs"]
mod test_channel;
#[path = "../test_channel_dump_oracle.rs"]
mod test_channel_dump_oracle;
#[path = "../test_channel_read_oracle.rs"]
mod test_channel_read_oracle;
#[path = "../test_deferred_fixtures.rs"]
mod test_deferred_fixtures;
#[path = "../test_fixture_full_dump_oracle.rs"]
mod test_fixture_full_dump_oracle;
#[path = "../test_fixture_read_warnings_oracle.rs"]
mod test_fixture_read_warnings_oracle;
#[path = "../test_frame_load.rs"]
mod test_frame_load;
#[path = "../test_frameset_variants_oracle.rs"]
mod test_frameset_variants_oracle;
#[path = "../test_keymap.rs"]
mod test_keymap;
#[path = "../test_keymap_properties.rs"]
mod test_keymap_properties;
#[path = "../test_keymap_serial.rs"]
mod test_keymap_serial;
#[path = "../test_mapping_load.rs"]
mod test_mapping_load;
#[path = "../test_native_roundtrip_all.rs"]
mod test_native_roundtrip_all;
#[path = "../test_object_id_oracle.rs"]
mod test_object_id_oracle;
#[path = "../test_report.rs"]
mod test_report;
#[path = "../test_section_load.rs"]
mod test_section_load;
#[path = "../test_serialisation.rs"]
mod test_serialisation;

#[path = "../test_table.rs"]
mod test_table;
#[path = "../test_table_oracle.rs"]
mod test_table_oracle;
