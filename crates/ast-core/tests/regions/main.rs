//! Integration tests: Region classes, their membership, meshes, overlap and masking.
//!
//! Each module is one test file in the parent directory, built into this one
//! test program so that a change to ast-core relinks a few programs rather than
//! one per file.

#[path = "../test_box_axin.rs"]
mod test_box_axin;
#[path = "../test_box_region.rs"]
mod test_box_region;
#[path = "../test_circle.rs"]
mod test_circle;
#[path = "../test_cmpregion.rs"]
mod test_cmpregion;
#[path = "../test_ellipse.rs"]
mod test_ellipse;
#[path = "../test_interval.rs"]
mod test_interval;
#[path = "../test_masking.rs"]
mod test_masking;
#[path = "../test_nullregion.rs"]
mod test_nullregion;
#[path = "../test_overlap.rs"]
mod test_overlap;
#[path = "../test_pointlist.rs"]
mod test_pointlist;
#[path = "../test_polygon.rs"]
mod test_polygon;
#[path = "../test_prism.rs"]
mod test_prism;
#[path = "../test_region_attr_test_oracle.rs"]
mod test_region_attr_test_oracle;
#[path = "../test_region_attrib_oracle.rs"]
mod test_region_attrib_oracle;
#[path = "../test_region_bounded_oracle.rs"]
mod test_region_bounded_oracle;
#[path = "../test_region_box_oracle.rs"]
mod test_region_box_oracle;
#[path = "../test_region_cmplist_oracle.rs"]
mod test_region_cmplist_oracle;
#[path = "../test_region_cmpregion_oracle.rs"]
mod test_region_cmpregion_oracle;
#[path = "../test_region_compound_attr_oracle.rs"]
mod test_region_compound_attr_oracle;
#[path = "../test_region_convex_oracle.rs"]
mod test_region_convex_oracle;
#[path = "../test_region_disc_oracle.rs"]
mod test_region_disc_oracle;
#[path = "../test_region_ellipse_oracle.rs"]
mod test_region_ellipse_oracle;
#[path = "../test_region_ellipsepars_oracle.rs"]
mod test_region_ellipsepars_oracle;
#[path = "../test_region_equal_oracle.rs"]
mod test_region_equal_oracle;
#[path = "../test_region_errors_oracle.rs"]
mod test_region_errors_oracle;
#[path = "../test_region_frame_oracle.rs"]
mod test_region_frame_oracle;
#[path = "../test_region_grid.rs"]
mod test_region_grid;
#[path = "../test_region_interval_oracle.rs"]
mod test_region_interval_oracle;
#[path = "../test_region_mapregion_oracle.rs"]
mod test_region_mapregion_oracle;
#[path = "../test_region_mask_oracle.rs"]
mod test_region_mask_oracle;
#[path = "../test_region_nulltran_oracle.rs"]
mod test_region_nulltran_oracle;
#[path = "../test_region_object.rs"]
mod test_region_object;
#[path = "../test_region_outline_oracle.rs"]
mod test_region_outline_oracle;
#[path = "../test_region_overlap_oracle.rs"]
mod test_region_overlap_oracle;
#[path = "../test_region_polygon_oracle.rs"]
mod test_region_polygon_oracle;
#[path = "../test_region_showmesh_oracle.rs"]
mod test_region_showmesh_oracle;
#[path = "../test_regions.rs"]
mod test_regions;
