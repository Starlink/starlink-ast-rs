# Build oracle fixture generators against reference libast.
#
# libast.so bundles erfa, but pal and cminpack are separate shared
# libraries, and its yaml serialisation code pulls in libyaml at link
# time (the conda env's libyaml-0.so.2 satisfies it at run time).
# RPATH points back at the build directory so the resulting binaries
# find the shared libraries at run time without LD_LIBRARY_PATH
# gymnastics.

REF := ../../reference/starlink-ast
INCS := -I$(REF)/src -I$(REF)/build
# libast calls Grf entry points it does not define, so the logging stubs
# must be linked whole rather than only the members something references.
# GNU ld and Apple's ld64 spell that differently. On macOS libast.dylib
# records its own libyaml dependency, which need not be on the linker's
# search path, so -lyaml is only passed elsewhere.
ifeq ($(shell uname -s),Darwin)
GRF_LOG := -Wl,-force_load,$(REF)/build/libast_grf_log.a
YAML :=
else
GRF_LOG := -Wl,--whole-archive -last_grf_log -Wl,--no-whole-archive
YAML := -lyaml
endif
LIBS := -L$(REF)/build $(GRF_LOG) \
        -last -last_pal -last_cminpack -last_err -last_grf3d $(YAML) -lm
RPATH := -Wl,-rpath,$(abspath $(REF)/build)
CFLAGS := -O0 -g -Wall

PROGS := attr_test_clear_probe region_interval_probe region_box_probe region_mask_probe region_disc_probe region_equal_probe region_frame_probe region_attrib_probe region_overlap_probe intramap_probe xphmap_probe dss_merge_probe spline_inverse_probe poly_iter_inverse_probe map_split_probe equal_probe rate_probe lutmap_forward_probe linear_approx analyse_poly scale_poly_inputs orth_vector_set \
         sip_int_world keymap_hash gen_simp unitnorm_probe \
         frame_in_map_slot_probe skyframe_format_probe \
         frame_attrs_probe frame_class_attrs_probe \
         frame_axis_attrs_probe skyframe_attrs_probe \
         picked_skyaxis_probe class_attrs_probe sky_geometry_probe \
         match_axes_probe sky_convert_probe integrity_probe \
         spec_convert_probe unit_mapper_probe time_convert_probe \
         min_max_axes_probe time_map_probe overlay_probe \
         flux_unit_system_probe system_change_probe new_unit_probe \
         sky_norm_probe specflux_cmp_probe \
         frame_load_probe sky_interleave_probe dsb_topo_probe \
         axis_format_probe axis_unformat_probe time_epoch_probe \
         specflux_primary_probe cmp_match_end_probe \
         time_scale_set_probe flux_badun_probe spec_origin_sor_probe use_defs_probe \
         sky_use_defs_probe spec_use_defs_probe \
         region_object_probe section_load_probe sky_last_probe ident_probe frameset_current_probe sky_plain_axis_probe box_axin_probe norm_points_probe frame_api_probe region_grid_probe report_probe allow_simplify_probe lutmap_inverse_probe mapping_load_probe specmap_3d_probe tran_definedness_probe poly_bad_probe mapping_bad_probe remove_regions_probe wcsmap_pv_probe selectormap_dump_probe sky_pole_probe

all: $(PROGS)

%: %.c
	$(CC) $(CFLAGS) $(INCS) -o $@ $< $(LIBS) $(RPATH)

# gen_simp reads the whole simplify corpus, which includes IntraMap fixtures.
# An IntraMap names its transformation function in the dump and refuses to load
# until that function is registered, so link the upstream helper that registers
# the corpus's four functions rather than keeping a second copy of the table.
gen_simp: gen_simp.c $(REF)/ast_tester/transform_oracle_util.c
	$(CC) $(CFLAGS) $(INCS) -I$(REF)/ast_tester -o $@ $^ $(LIBS) $(RPATH)

clean:
	rm -rf $(PROGS) $(addsuffix .dSYM,$(PROGS))

.PHONY: all clean
