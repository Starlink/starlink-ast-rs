//! Integration tests: Mapping classes, their transforms, simplification and merging.
//!
//! Each module is one test file in the parent directory, built into this one
//! test program so that a change to ast-core relinks a few programs rather than
//! one per file.

#[path = "../common/mod.rs"]
mod common;

#[path = "../test_allow_simplify.rs"]
mod test_allow_simplify;
#[path = "../test_chebymap.rs"]
mod test_chebymap;
#[path = "../test_cmpmap.rs"]
mod test_cmpmap;
#[path = "../test_dss_merge_oracle.rs"]
mod test_dss_merge_oracle;
#[path = "../test_equal.rs"]
mod test_equal;
#[path = "../test_grismmap.rs"]
mod test_grismmap;
#[path = "../test_intramap.rs"]
mod test_intramap;
#[path = "../test_is_linear.rs"]
mod test_is_linear;
#[path = "../test_lutmap.rs"]
mod test_lutmap;
#[path = "../test_lutmap_forward_oracle.rs"]
mod test_lutmap_forward_oracle;
#[path = "../test_lutmap_inverse.rs"]
mod test_lutmap_inverse;
#[path = "../test_map_split.rs"]
mod test_map_split;
#[path = "../test_map_split_oracle.rs"]
mod test_map_split_oracle;
#[path = "../test_mapping.rs"]
mod test_mapping;
#[path = "../test_mapping_bad.rs"]
mod test_mapping_bad;
#[path = "../test_mathmap.rs"]
mod test_mathmap;
#[path = "../test_matrixmap.rs"]
mod test_matrixmap;
#[path = "../test_nan_input_parity.rs"]
mod test_nan_input_parity;
#[path = "../test_nan_outputs.rs"]
mod test_nan_outputs;
#[path = "../test_normmap.rs"]
mod test_normmap;
#[path = "../test_numeric_rate.rs"]
mod test_numeric_rate;
#[path = "../test_pcdmap.rs"]
mod test_pcdmap;
#[path = "../test_permmap.rs"]
mod test_permmap;
#[path = "../test_poly_bad.rs"]
mod test_poly_bad;
#[path = "../test_poly_iter_inverse_oracle.rs"]
mod test_poly_iter_inverse_oracle;
#[path = "../test_polymap.rs"]
mod test_polymap;
#[path = "../test_polymap_simd.rs"]
mod test_polymap_simd;
#[path = "../test_polytran.rs"]
mod test_polytran;
#[path = "../test_rate_oracle.rs"]
mod test_rate_oracle;
#[path = "../test_ratemap.rs"]
mod test_ratemap;
#[path = "../test_remove_regions_oracle.rs"]
mod test_remove_regions_oracle;
#[path = "../test_resample.rs"]
mod test_resample;
#[path = "../test_resample_oracle.rs"]
mod test_resample_oracle;
#[path = "../test_selectormap.rs"]
mod test_selectormap;
#[path = "../test_shiftmap.rs"]
mod test_shiftmap;
#[path = "../test_simplify_seams.rs"]
mod test_simplify_seams;
#[path = "../test_skyref_offset_mapping.rs"]
mod test_skyref_offset_mapping;
#[path = "../test_slamap.rs"]
mod test_slamap;
#[path = "../test_specmap.rs"]
mod test_specmap;
#[path = "../test_specmap_3d.rs"]
mod test_specmap_3d;
#[path = "../test_sphmap.rs"]
mod test_sphmap;
#[path = "../test_splinemap.rs"]
mod test_splinemap;
#[path = "../test_splinemap_c_api.rs"]
mod test_splinemap_c_api;
#[path = "../test_switchmap.rs"]
mod test_switchmap;
#[path = "../test_time_map_oracle.rs"]
mod test_time_map_oracle;
#[path = "../test_timemap.rs"]
mod test_timemap;
#[path = "../test_tran1_tran2.rs"]
mod test_tran1_tran2;
#[path = "../test_tran_definedness.rs"]
mod test_tran_definedness;
#[path = "../test_tran_definedness_oracle.rs"]
mod test_tran_definedness_oracle;
#[path = "../test_tran_grid_oracle.rs"]
mod test_tran_grid_oracle;
#[path = "../test_trangrid.rs"]
mod test_trangrid;
#[path = "../test_tranmap.rs"]
mod test_tranmap;
#[path = "../test_transform_oracle.rs"]
mod test_transform_oracle;
#[path = "../test_transform_validation.rs"]
mod test_transform_validation;
#[path = "../test_unitmap.rs"]
mod test_unitmap;
#[path = "../test_unitnormmap.rs"]
mod test_unitnormmap;
#[path = "../test_wcsmap.rs"]
mod test_wcsmap;
#[path = "../test_wcsmap_pv.rs"]
mod test_wcsmap_pv;
#[path = "../test_winmap.rs"]
mod test_winmap;
#[path = "../test_xphmap.rs"]
mod test_xphmap;
#[path = "../test_zoommap.rs"]
mod test_zoommap;
