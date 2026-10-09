//! Integration tests: FitsChan: FITS headers, WCS encodings and the wcsconverter regression.
//!
//! Each module is one test file in the parent directory, built into this one
//! test program so that a change to ast-core relinks a few programs rather than
//! one per file.

#[path = "../common/mod.rs"]
mod common;

#[path = "../test_alt_wcs_read.rs"]
mod test_alt_wcs_read;
#[path = "../test_alt_wcs_round_trip.rs"]
mod test_alt_wcs_round_trip;
#[path = "../test_fitschan.rs"]
mod test_fitschan;
#[path = "../test_fitschan_astwarn.rs"]
mod test_fitschan_astwarn;
#[path = "../test_fitschan_class.rs"]
mod test_fitschan_class;
#[path = "../test_fitschan_encodings.rs"]
mod test_fitschan_encodings;
#[path = "../test_fitschan_native_keyword.rs"]
mod test_fitschan_native_keyword;
#[path = "../test_fitschan_read.rs"]
mod test_fitschan_read;
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
