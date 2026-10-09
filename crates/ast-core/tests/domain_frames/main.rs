//! Integration tests: SkyFrame, SpecFrame, TimeFrame, FluxFrame, DSBSpecFrame and SpecFluxFrame.
//!
//! Each module is one test file in the parent directory, built into this one
//! test program so that a change to ast-core relinks a few programs rather than
//! one per file.

#[path = "../test_dsb_topo.rs"]
mod test_dsb_topo;
#[path = "../test_dsbspecframe.rs"]
mod test_dsbspecframe;
#[path = "../test_dsbspecframe_read.rs"]
mod test_dsbspecframe_read;
#[path = "../test_flux.rs"]
mod test_flux;
#[path = "../test_flux_badun.rs"]
mod test_flux_badun;
#[path = "../test_fluxframe.rs"]
mod test_fluxframe;
#[path = "../test_sky_convert.rs"]
mod test_sky_convert;
#[path = "../test_sky_geometry.rs"]
mod test_sky_geometry;
#[path = "../test_sky_interleave.rs"]
mod test_sky_interleave;
#[path = "../test_sky_last.rs"]
mod test_sky_last;
#[path = "../test_sky_norm.rs"]
mod test_sky_norm;
#[path = "../test_sky_plain_axis.rs"]
mod test_sky_plain_axis;
#[path = "../test_sky_pole_oracle.rs"]
mod test_sky_pole_oracle;
#[path = "../test_sky_use_defs.rs"]
mod test_sky_use_defs;
#[path = "../test_skyframe.rs"]
mod test_skyframe;
#[path = "../test_skyframe_attrs.rs"]
mod test_skyframe_attrs;
#[path = "../test_skyframe_format.rs"]
mod test_skyframe_format;
#[path = "../test_spec_convert.rs"]
mod test_spec_convert;
#[path = "../test_spec_origin_sor.rs"]
mod test_spec_origin_sor;
#[path = "../test_spec_use_defs.rs"]
mod test_spec_use_defs;
#[path = "../test_specflux.rs"]
mod test_specflux;
#[path = "../test_specflux_cmp.rs"]
mod test_specflux_cmp;
#[path = "../test_specflux_primary.rs"]
mod test_specflux_primary;
#[path = "../test_specframe.rs"]
mod test_specframe;
#[path = "../test_time_convert.rs"]
mod test_time_convert;
#[path = "../test_time_epoch.rs"]
mod test_time_epoch;
#[path = "../test_time_scale_set.rs"]
mod test_time_scale_set;
#[path = "../test_timeframe.rs"]
mod test_timeframe;
