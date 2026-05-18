/*
 * C oracle for bench_per_class.rs.
 *
 * Reads the manifest written by the Rust example
 * (default /tmp/ast_bench_per_class/manifest.tsv). Each line specifies a
 * `(variant, ast_file, direction, npoint)` 4-tuple. We:
 *   1. Read the .ast file content,
 *   2. Build the Mapping via astFromString,
 *   3. Generate the same deterministic input point set as the Rust side,
 *   4. Run a warmup astTran2, then time `iters` runs,
 *   5. Print one TSV row matching the Rust side's format so the two
 *      outputs can be joined / paste-d together.
 *
 * Build (from the starlink-astrs root, after CMake build of reference):
 *   cc -O2 -o /tmp/bench_per_class_c crates/ast-core/tests/bench_per_class_c.c \
 *      -I reference/starlink-ast/build-ast -I reference/starlink-ast \
 *      -I /opt/homebrew/include \
 *      -L reference/starlink-ast/build-ast -L /opt/homebrew/lib \
 *      -last -lyaml -lm -lpthread
 *
 * Run:
 *   DYLD_LIBRARY_PATH=reference/starlink-ast/build-ast /tmp/bench_per_class_c \
 *      /tmp/ast_bench_per_class/manifest.tsv
 *
 * The mapping is taken as-is (no astSimplify) so we bench the same shape
 * Rust does.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "ast.h"

/* Grf stubs — libast pulls graphics symbols at runtime even though we
 * never plot. */
int astGCap(int cap, int v){return 0;}
int astGScales(float *a,float *b){return 0;}
int astGLine(int n,const float *x,const float *y){return 0;}
int astGMark(int n,const float *x,const float *y,int t){return 0;}
int astGText(const char *t,float x,float y,const char *j,float ux,float uy){return 0;}
int astGTxExt(const char *t,float x,float y,const char *j,float ux,float uy,float *xb,float *yb){return 0;}
int astGAttr(int a,double v,double *ov,int p){return 0;}
int astGQch(float *chv,float *chh){return 0;}
int astGFlush(void){return 0;}

static double now_sec(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec + ts.tv_nsec / 1e9;
}

/* Slurp a whole file into a malloc'd buffer (NUL-terminated). */
static char *read_file(const char *path) {
    FILE *f = fopen(path, "r");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    long len = ftell(f);
    fseek(f, 0, SEEK_SET);
    char *buf = malloc((size_t)len + 1);
    if (!buf) { fclose(f); return NULL; }
    if (fread(buf, 1, (size_t)len, f) != (size_t)len) {
        free(buf); fclose(f); return NULL;
    }
    buf[len] = '\0';
    fclose(f);
    return buf;
}

/* Match the Rust side's `make_points`. Generate `npoint` rows, each `nin`
 * long, by linearly scanning t in [0, 1] and offsetting each axis by
 * axis/(2*nin). Frac the result so it stays in [0, 1] per axis.
 *
 * Returns an array of `nin` axis-separated arrays (column-major), each of
 * length `npoint`. The caller frees with free_columns().
 */
static double **make_points(int npoint, int nin, double lo, double hi) {
    double **cols = malloc(sizeof(double *) * (size_t)nin);
    for (int c = 0; c < nin; c++) {
        cols[c] = malloc(sizeof(double) * (size_t)npoint);
    }
    double span = hi - lo;
    for (int i = 0; i < npoint; i++) {
        double t = (npoint > 1) ? (double)i / (double)(npoint - 1) : 0.5;
        for (int axis = 0; axis < nin; axis++) {
            double offset = (double)axis / (2.0 * (double)nin);
            double u = t + offset;
            u = u - (long)u;       /* fract() */
            if (u < 0.0) u += 1.0; /* defensive — t+offset is non-negative here */
            cols[axis][i] = lo + span * u;
        }
    }
    return cols;
}

static void free_columns(double **cols, int nin) {
    for (int c = 0; c < nin; c++) free(cols[c]);
    free(cols);
}

static int iterations_for(int npoint) {
    int n = 500000 / (npoint > 0 ? npoint : 1);
    return n < 3 ? 3 : n;
}

/* Run forward-then-inverse on a 2-axis mapping, returning ns/pt for the
 * inverse step (the forward output is generated but not timed). Used to
 * mirror the Rust side's round-trip pattern.
 *
 * `nin_eff` / `nout_eff` are the *effective* in/out (forward direction).
 * The inverse expects nout_eff inputs and produces nin_eff outputs.
 */
static double time_inverse(AstMapping *map, double *xin, double *yin, int npoint) {
    int iters = iterations_for(npoint);
    double *xout = malloc(sizeof(double) * (size_t)npoint);
    double *yout = malloc(sizeof(double) * (size_t)npoint);

    /* warmup forward */
    astTran2(map, npoint, xin, yin, 1, xout, yout);

    /* warmup inverse */
    double *xinv = malloc(sizeof(double) * (size_t)npoint);
    double *yinv = malloc(sizeof(double) * (size_t)npoint);
    astTran2(map, npoint, xout, yout, 0, xinv, yinv);

    double t0 = now_sec();
    for (int k = 0; k < iters; k++) {
        astTran2(map, npoint, xout, yout, 0, xinv, yinv);
    }
    double elapsed = now_sec() - t0;

    free(xout); free(yout); free(xinv); free(yinv);
    return elapsed * 1e9 / ((double)iters * (double)npoint);
}

static double time_forward(AstMapping *map, double *xin, double *yin, int npoint) {
    int iters = iterations_for(npoint);
    double *xout = malloc(sizeof(double) * (size_t)npoint);
    double *yout = malloc(sizeof(double) * (size_t)npoint);

    /* warmup */
    astTran2(map, npoint, xin, yin, 1, xout, yout);

    double t0 = now_sec();
    for (int k = 0; k < iters; k++) {
        astTran2(map, npoint, xin, yin, 1, xout, yout);
    }
    double elapsed = now_sec() - t0;

    free(xout); free(yout);
    return elapsed * 1e9 / ((double)iters * (double)npoint);
}

/* Map variant name -> input domain. Must match the Rust side's
 * `Variant.input_range`. Returns 1 on match, 0 if unknown. */
static int input_range_for(const char *name, double *lo, double *hi) {
    if (!strcmp(name, "polymap_2d_deg5"))      { *lo = -0.5;     *hi = 0.5;     return 1; }
    if (!strcmp(name, "wcsmap_tan"))           { *lo = -0.05;    *hi = 0.05;    return 1; }
    if (!strcmp(name, "wcsmap_sin"))           { *lo = -0.05;    *hi = 0.05;    return 1; }
    if (!strcmp(name, "wcsmap_zpn"))           { *lo = -0.05;    *hi = 0.05;    return 1; }
    if (!strcmp(name, "lutmap_100entries"))    { *lo = 0.0;      *hi = 99.0;    return 1; }
    if (!strcmp(name, "pcdmap_typical"))       { *lo = -0.1;     *hi = 0.1;     return 1; }
    if (!strcmp(name, "grismmap_typical"))     { *lo = -1e-3;    *hi = 1e-3;    return 1; }
    if (!strcmp(name, "slamap_eqgal"))         { *lo = 0.0;      *hi = 1.5;     return 1; }
    if (!strcmp(name, "timemap_taiutc"))       { *lo = 50000.0;  *hi = 50100.0; return 1; }
    if (!strcmp(name, "specmap_wn_to_fr"))     { *lo = 1.0;      *hi = 1000.0;  return 1; }
    if (!strcmp(name, "dssmap_typical"))       { *lo = 5000.0;   *hi = 15000.0; return 1; }
    if (!strcmp(name, "splinemap_5x5_cubic")) { *lo = 0.1;      *hi = 0.9;     return 1; }
    if (!strcmp(name, "ratemap_polymap"))      { *lo = -0.5;     *hi = 0.5;     return 1; }
    /* Default for the linear / SphMap / etc. variants. */
    *lo = -1.0; *hi = 1.0;
    return 1;
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: %s <manifest.tsv>\n", argv[0]);
        return 2;
    }
    FILE *mf = fopen(argv[1], "r");
    if (!mf) { perror(argv[1]); return 2; }

    /* Match the Rust header. */
    printf("%-28s\t%-7s\t%7s\t%12s\n", "variant", "dir", "npoint", "c_ns/pt");

    char line[1024];
    while (fgets(line, sizeof(line), mf)) {
        if (line[0] == '#' || line[0] == '\n') continue;
        char variant[128], ast_path[512], direction[16];
        int npoint = 0;
        if (sscanf(line, "%127[^\t]\t%511[^\t]\t%15[^\t]\t%d",
                   variant, ast_path, direction, &npoint) != 4) {
            fprintf(stderr, "skipping malformed line: %s", line);
            continue;
        }

        /* Each variant gets its own object-lifetime context. Without
         * astBegin/astEnd the handle stack accumulates internal objects
         * (e.g. PolyMap LinearGuess caches, WcsMap projection state) and
         * eventually crashes after a few dozen variants. astEnd at the
         * bottom releases everything from this iteration. */
        astBegin;
        char *text = read_file(ast_path);
        if (!text) { fprintf(stderr, "cannot read %s\n", ast_path); astEnd; continue; }
        AstObject *obj = astFromString(text);
        free(text);
        if (!obj) { fprintf(stderr, "astFromString failed for %s\n", ast_path); astEnd; continue; }
        AstMapping *map = (AstMapping *)obj;
        int nin = astGetI(map, "Nin");

        double lo, hi;
        input_range_for(variant, &lo, &hi);
        double **cols = make_points(npoint, nin, lo, hi);

        /* Dispatch on dimensionality: astTran1 / astTran2 are convenience
         * wrappers around astTranN. The current Rust variants span 1D
         * (LutMap), 2D (most), and 3D-in/2D-out (SphMap forward only). */
        double ns_pt = 0.0;
        if (nin == 1) {
            /* astTran1: 1D forward/inverse. */
            int iters = iterations_for(npoint);
            double *xout = malloc(sizeof(double) * (size_t)npoint);
            int fwd = !strcmp(direction, "forward") ? 1 : 0;
            /* Inverse arm: feed inverse with forward's output, matching
             * the Rust round-trip. */
            double *target = cols[0];
            double *xfwd = NULL;
            if (!fwd) {
                xfwd = malloc(sizeof(double) * (size_t)npoint);
                astTran1(map, npoint, cols[0], 1, xfwd);
                target = xfwd;
            }
            /* warmup */
            astTran1(map, npoint, target, fwd, xout);
            double t0 = now_sec();
            for (int k = 0; k < iters; k++) {
                astTran1(map, npoint, target, fwd, xout);
            }
            double elapsed = now_sec() - t0;
            ns_pt = elapsed * 1e9 / ((double)iters * (double)npoint);
            free(xout);
            if (xfwd) free(xfwd);
        } else if (nin == 2) {
            if (!strcmp(direction, "forward")) {
                ns_pt = time_forward(map, cols[0], cols[1], npoint);
            } else {
                ns_pt = time_inverse(map, cols[0], cols[1], npoint);
            }
        } else if (nin == 3) {
            /* SphMap: 3 in -> 2 out. Use astTranN to handle 3 inputs. */
            int nout = astGetI(map, "Nout");
            int iters = iterations_for(npoint);
            double *in_flat = malloc(sizeof(double) * (size_t)(npoint * nin));
            double *out_flat = malloc(sizeof(double) * (size_t)(npoint * nout));
            for (int axis = 0; axis < nin; axis++) {
                for (int i = 0; i < npoint; i++) {
                    in_flat[axis * npoint + i] = cols[axis][i];
                }
            }
            /* warmup */
            astTranN(map, npoint, nin, npoint, in_flat, 1, nout, npoint, out_flat);
            double t0 = now_sec();
            for (int k = 0; k < iters; k++) {
                astTranN(map, npoint, nin, npoint, in_flat, 1, nout, npoint, out_flat);
            }
            double elapsed = now_sec() - t0;
            ns_pt = elapsed * 1e9 / ((double)iters * (double)npoint);
            free(in_flat); free(out_flat);
        } else {
            fprintf(stderr, "unsupported nin=%d for variant %s\n", nin, variant);
        }

        printf("%-28s\t%-7s\t%7d\t%12.1f\n", variant, direction, npoint, ns_pt);
        fflush(stdout); /* keep output progressive in case of a later crash */

        free_columns(cols, nin);
        /* astEnd cleans up the AST objects from this iteration; no need
         * to astAnnul(obj) explicitly. */
        astEnd;
    }

    fclose(mf);
    return 0;
}
