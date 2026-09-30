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

PROGS := linear_approx analyse_poly scale_poly_inputs orth_vector_set \
         sip_int_world keymap_hash gen_simp unitnorm_probe \
         frame_in_map_slot_probe skyframe_format_probe

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
