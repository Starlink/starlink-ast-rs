/*
 * C oracle benchmark for comparison with bench_lsst.rs.
 *
 * Build (from the starlink-astrs root, after cmake build of reference):
 *   cc -O2 -o /tmp/bench_lsst_c crates/ast-core/tests/bench_lsst_c.c \
 *      -I reference/starlink-ast -I reference/starlink-ast/build \
 *      -L reference/starlink-ast/build -last -lyaml -lm -lpthread
 *
 * Run:
 *   LD_LIBRARY_PATH=reference/starlink-ast/build /tmp/bench_lsst_c
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "ast.h"

/* Grf stubs — we don't use plotting but the shared lib references these */
int astGCap( int cap, int value ) { return 0; }
int astGScales( float *alpha, float *beta ) { return 0; }
int astGLine( int n, const float *x, const float *y ) { return 0; }
int astGMark( int n, const float *x, const float *y, int type ) { return 0; }
int astGText( const char *text, float x, float y, const char *just, float upx, float upy ) { return 0; }
int astGTxExt( const char *text, float x, float y, const char *just, float upx, float upy, float *xb, float *yb ) { return 0; }
int astGAttr( int attr, double value, double *old_value, int prim ) { return 0; }
int astGQch( float *chv, float *chh ) { return 0; }
int astGFlush( void ) { return 0; }

static double get_time_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000.0 + ts.tv_nsec / 1e6;
}

int main(void) {
    const char *path = "reference/starlink-ast/ast_tester/lsst.ast";
    FILE *f = fopen(path, "r");
    if (!f) {
        path = "crates/ast-core/tests/fixtures/native/reference/lsst.ast";
        f = fopen(path, "r");
    }
    if (!f) { fprintf(stderr, "Cannot find lsst.ast\n"); return 1; }

    fseek(f, 0, SEEK_END);
    long len = ftell(f);
    fseek(f, 0, SEEK_SET);
    char *text = malloc(len + 1);
    fread(text, 1, len, f);
    text[len] = '\0';
    fclose(f);

    AstObject *obj = astFromString(text);
    free(text);
    if (!obj) { fprintf(stderr, "Failed to parse lsst.ast\n"); return 1; }

    AstMapping *map = (AstMapping *)obj;
    int nin = astGetI(map, "Nin");
    int nout = astGetI(map, "Nout");
    printf("\n=== LSST FrameSet Transform Benchmark (C) ===\n\n");
    printf("Loaded: nin=%d, nout=%d\n\n", nin, nout);

    AstMapping *simp = (AstMapping *)astSimplify(map);
    if (!simp) { fprintf(stderr, "astSimplify failed\n"); return 1; }
    printf("Simplified: nin=%d, nout=%d (class=%s)\n\n",
           astGetI(simp, "Nin"), astGetI(simp, "Nout"), astGetC(simp, "Class"));

    int sizes[] = {1, 10000, 100000};
    int nsizes = 3;

    struct { AstMapping *m; const char *label; } variants[2] = {
        { map,  "Original mapping"   },
        { simp, "Simplified mapping" },
    };

    for (int v = 0; v < 2; v++) {
        AstMapping *m = variants[v].m;
        printf("--- %s ---\n", variants[v].label);

        for (int s = 0; s < nsizes; s++) {
            int n = sizes[s];
            printf("N = %d:\n", n);

            double *xin = malloc(n * sizeof(double));
            double *yin = malloc(n * sizeof(double));
            double *xout = malloc(n * sizeof(double));
            double *yout = malloc(n * sizeof(double));

            for (int i = 0; i < n; i++) {
                xin[i] = 100.0 + i * 0.01;
                yin[i] = 200.0 + i * 0.005;
            }

            int iters = (n >= 100000) ? 3 : (n >= 10000) ? 10 : 100;

            /* Warm up */
            astTran2(m, n, xin, yin, 1, xout, yout);

            /* Forward */
            double t0 = get_time_ms();
            for (int i = 0; i < iters; i++) {
                astTran2(m, n, xin, yin, 1, xout, yout);
            }
            double fwd_ms = (get_time_ms() - t0) / iters;
            double fwd_ns_per_pt = fwd_ms * 1e6 / n;
            printf("  forward %7d pts: %10.3f ms total, %8.1f ns/pt  (%d iters)\n",
                   n, fwd_ms, fwd_ns_per_pt, iters);

            /* Inverse */
            astTran2(m, n, xout, yout, 0, xin, yin);

            t0 = get_time_ms();
            for (int i = 0; i < iters; i++) {
                astTran2(m, n, xout, yout, 0, xin, yin);
            }
            double inv_ms = (get_time_ms() - t0) / iters;
            double inv_ns_per_pt = inv_ms * 1e6 / n;
            printf("  inverse %7d pts: %10.3f ms total, %8.1f ns/pt  (%d iters)\n",
                   n, inv_ms, inv_ns_per_pt, iters);

            printf("\n");
            free(xin); free(yin); free(xout); free(yout);
        }
    }

    return 0;
}
