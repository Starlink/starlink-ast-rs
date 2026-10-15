//! Integration tests: FitsChan: FITS headers, WCS encodings and the wcsconverter regression.
//!
//! Each module is one test file in the parent directory, built into this one
//! test program so that a change to ast-core relinks a few programs rather than
//! one per file.

// Test code computes expected values directly; the counted `math` wrappers
// are for the library (`crates/ast-core/clippy.toml`).
#![allow(clippy::disallowed_methods)]

#[path = "../common/mod.rs"]
mod common;

#[path = "../test_alt_wcs_read.rs"]
mod test_alt_wcs_read;
#[path = "../test_alt_wcs_round_trip.rs"]
mod test_alt_wcs_round_trip;
#[path = "../test_fitschan.rs"]
mod test_fitschan;
#[path = "../test_fitschan_alt_epoch_oracle.rs"]
mod test_fitschan_alt_epoch_oracle;
#[path = "../test_fitschan_alt_naxes_oracle.rs"]
mod test_fitschan_alt_naxes_oracle;
#[path = "../test_fitschan_astwarn.rs"]
mod test_fitschan_astwarn;
#[path = "../test_fitschan_axis_count_oracle.rs"]
mod test_fitschan_axis_count_oracle;
#[path = "../test_fitschan_c_inline.rs"]
mod test_fitschan_c_inline;
#[path = "../test_fitschan_class.rs"]
mod test_fitschan_class;
#[path = "../test_fitschan_coupled_oracle.rs"]
mod test_fitschan_coupled_oracle;
#[path = "../test_fitschan_cursor_oracle.rs"]
mod test_fitschan_cursor_oracle;
#[path = "../test_fitschan_encodings.rs"]
mod test_fitschan_encodings;
#[path = "../test_fitschan_encodings_oracle.rs"]
mod test_fitschan_encodings_oracle;
#[path = "../test_fitschan_errors.rs"]
mod test_fitschan_errors;
#[path = "../test_fitschan_labels_oracle.rs"]
mod test_fitschan_labels_oracle;
#[path = "../test_fitschan_linear_oracle.rs"]
mod test_fitschan_linear_oracle;
#[path = "../test_fitschan_native_keyword.rs"]
mod test_fitschan_native_keyword;
#[path = "../test_fitschan_read.rs"]
mod test_fitschan_read;
#[path = "../test_fitschan_read_chain_oracle.rs"]
mod test_fitschan_read_chain_oracle;
#[path = "../test_fitschan_sipok_oracle.rs"]
mod test_fitschan_sipok_oracle;
#[path = "../test_fitschan_spec_unit_oracle.rs"]
mod test_fitschan_spec_unit_oracle;
#[path = "../test_fitschan_splitmap_oracle.rs"]
mod test_fitschan_splitmap_oracle;
#[path = "../test_fitschan_tab.rs"]
mod test_fitschan_tab;
#[path = "../test_fitschan_tab_read_oracle.rs"]
mod test_fitschan_tab_read_oracle;
#[path = "../test_fitschan_tab_write_oracle.rs"]
mod test_fitschan_tab_write_oracle;
#[path = "../test_fitschan_tables_oracle.rs"]
mod test_fitschan_tables_oracle;
#[path = "../test_fitschan_time.rs"]
mod test_fitschan_time;
#[path = "../test_fitschan_wcs.rs"]
mod test_fitschan_wcs;
#[path = "../test_fitschan_wcs_write.rs"]
mod test_fitschan_wcs_write;
#[path = "../test_lsst_oracle.rs"]
mod test_lsst_oracle;
#[path = "../test_wcsconv.rs"]
mod test_wcsconv;
