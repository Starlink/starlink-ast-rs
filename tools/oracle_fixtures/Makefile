# Build oracle fixture generators against reference libast.
#
# libast.so bundles pal and erfa, but its yaml serialisation code pulls
# in libyaml at link time (the conda env's libyaml-0.so.2 satisfies it
# at run time). RPATH points back at the build directory so the
# resulting binaries find the shared library at run time without
# LD_LIBRARY_PATH gymnastics.

REF := ../../reference/starlink-ast
INCS := -I$(REF)/src -I$(REF)/build
LIBS := -L$(REF)/build -Wl,--whole-archive -last_grf_log -Wl,--no-whole-archive \
        -last -last_err -last_grf3d -lyaml -lm
RPATH := -Wl,-rpath,$(abspath $(REF)/build)
CFLAGS := -O0 -g -Wall

PROGS := linear_approx analyse_poly scale_poly_inputs find_basis_vectors \
         fit_line make_int_world sip_int_world head2ast ast2ast keymap_hash \
         gen_simp unitnorm_probe frame_in_map_slot_probe

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
	rm -f $(PROGS)

.PHONY: all clean
