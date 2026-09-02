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
         gen_simp

all: $(PROGS)

%: %.c
	$(CC) $(CFLAGS) $(INCS) -o $@ $< $(LIBS) $(RPATH)

clean:
	rm -f $(PROGS)

.PHONY: all clean
