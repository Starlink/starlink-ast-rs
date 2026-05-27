/*
 * Captures reference output values for AnalysePoly tests.
 *
 * AnalysePoly is `static` in fitschan.c, so we can't call it directly.
 * Instead this file copies the body of fitschan.c::AnalysePoly verbatim
 * (renamed `analyse_poly_oracle`) and exposes it for fixture capture.
 * `astMergeShift_` is a protected (not public) symbol of AstPolyMap;
 * we declare its prototype manually since the public ast.h omits it.
 *
 * For each fixture we emit:
 *   - shift[2]      : ShiftMap displacement (None if AnalysePoly fails)
 *   - matrix[4]     : MatrixMap row-major (a,b,c,d), or [1,0,0,1] for
 *                     the UnitMap path
 *   - coeffs        : map2's PolyMap coefficient list (coeff, out_axis,
 *                     p0, p1) groups
 *
 * Usage:
 *   make analyse_poly
 *   ./analyse_poly > /tmp/analyse_poly.rs
 *
 * See docs/superpower/plans/2026-05-25-makeintworld-port.md, Task 2.2.
 */

#include "ast.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Declarations for symbols not exposed in the public ast.h. The
 * `_` suffix is the AST internal-symbol convention; the macro form
 * (without status) lives in protected polymap.h.
 *
 * `astMergeShift_` operates on raw internal pointers, not the public
 * handle/ID values returned by the public constructors. We bridge with
 * `astMakePointer_` and `astMakeId_` (also internal). */
extern AstPolyMap *astMergeShift_(AstPolyMap *this_, AstShiftMap *shift,
                                  int before, int force, int *status);
extern AstObject *astMakePointer_(AstObject *id, int *status);
extern AstObject *astMakeId_(AstObject *ptr, int *status);

#define AS_PTR(x) ((void *)astMakePointer_((AstObject *)(x), status))
#define AS_ID(x) ((void *)astMakeId_((AstObject *)(x), status))

/* Format a double so it always parses as a Rust f64 literal. */
static void emit_double(double v) {
    char buf[64];
    snprintf(buf, sizeof(buf), "%.17g", v);
    if (strchr(buf, '.') == NULL && strchr(buf, 'e') == NULL
        && strchr(buf, 'n') == NULL && strchr(buf, 'i') == NULL) {
        printf("        %s.0,\n", buf);
    } else {
        printf("        %s,\n", buf);
    }
}

/*
 * Verbatim copy of fitschan.c::AnalysePoly (~line 4174). External-symbol
 * references (astMergeShift_) declared above; everything else is in
 * the public ast.h.
 *
 * Behavioural deltas vs. the upstream copy:
 *   - returns map3 form via a separate flag (so caller can tell
 *     UnitMap vs. MatrixMap); the C code returns it as an AstMapping*
 *     handle, which we keep, plus an additional structural tag.
 *   - signature: takes a status pointer explicitly.
 */
static int analyse_poly_oracle(AstPolyMap *polymap, AstMapping **map1,
                               AstMapping **map2, AstMapping **map3,
                               int *map3_is_unit, int *status) {
    AstMapping *tmap;
    AstPolyMap *pm1;
    double *coeffs_f;
    double *coeffs_f_new;
    double *pv;
    double *pw;
    double a, b, c, d;
    double det;
    double matrix[4];
    double shift[2];
    double xo, yo;
    int a_ico, b_ico, c_ico, d_ico;
    int ico;
    int inverted;
    int ncoeff_f;
    int ncoeff_f_new;
    int nhigh;
    int ret;

    ret = 0;
    *map1 = NULL;
    *map2 = NULL;
    *map3 = NULL;
    *map3_is_unit = 0;

    if (!astOK) return ret;

    if (!astGetI(polymap, "IterInverse")) return ret;

    if (astGetI(polymap, "Nin") != 2 || astGetI(polymap, "Nout") != 2) {
        return ret;
    }

    if (astGetI(polymap, "Invert")) {
        astClear(polymap, "Invert");
        inverted = 1;
    } else {
        inverted = 0;
    }

    xo = 0.0;
    yo = 0.0;
    astTran2(polymap, 1, &xo, &yo, 0, shift, shift + 1);

    shift[0] *= -1.0;
    shift[1] *= -1.0;
    *map1 = (AstMapping *)astShiftMap(2, shift, " ");

    astInvert(*map1);
    {
        AstPolyMap *pm_ptr = AS_PTR(polymap);
        AstShiftMap *sm_ptr = AS_PTR(*map1);
        AstPolyMap *pm1_ptr = astMergeShift_(pm_ptr, sm_ptr, 1, 1, status);
        pm1 = AS_ID(pm1_ptr);
    }
    astInvert(*map1);

    astPolyCoeffs(pm1, 1, 0, NULL, &ncoeff_f);
    coeffs_f = (double *)malloc((size_t)(ncoeff_f * 4) * sizeof(*coeffs_f));
    if (coeffs_f) {
        astPolyCoeffs(pm1, 1, ncoeff_f * 4, coeffs_f, &ncoeff_f);

        a = 0.0; a_ico = -1;
        b = 0.0; b_ico = -1;
        c = 0.0; c_ico = -1;
        d = 0.0; d_ico = -1;
        nhigh = 0;

        pv = coeffs_f;
        for (ico = 0; ico < ncoeff_f; ico++, pv += 4) {
            if (pv[1] == 1) {
                if (pv[2] == 1 && pv[3] == 0) {
                    a = pv[0]; a_ico = ico;
                } else if (pv[2] == 0 && pv[3] == 1) {
                    b = pv[0]; b_ico = ico;
                } else {
                    nhigh++;
                }
            } else {
                if (pv[2] == 1 && pv[3] == 0) {
                    c = pv[0]; c_ico = ico;
                } else if (pv[2] == 0 && pv[3] == 1) {
                    d = pv[0]; d_ico = ico;
                } else {
                    nhigh++;
                }
            }
        }

        det = a * d - b * c;

        if (a == 1.0 && b == 0.0 && c == 0.0 && d == 1.0) {
            *map3 = (AstMapping *)astUnitMap(2, " ");
            *map3_is_unit = 1;
            *map2 = astClone(pm1);
        } else if (det != 0.0) {
            matrix[0] = a; matrix[1] = b; matrix[2] = c; matrix[3] = d;
            *map3 = (AstMapping *)astMatrixMap(2, 2, 0, matrix, " ");

            ncoeff_f_new = 2 * nhigh + 2;
            coeffs_f_new =
                (double *)malloc((size_t)(ncoeff_f_new * 4) * sizeof(*coeffs_f_new));
            if (astOK) {
                pw = coeffs_f_new;

                pw[0] = 1.0; pw[1] = 1; pw[2] = 1; pw[3] = 0;
                pw += 4;

                pv = coeffs_f;
                for (ico = 0; ico < ncoeff_f; ico++, pv += 4) {
                    pw[1] = 1;
                    if (pv[0] != 0.0 && ico != a_ico && ico != b_ico
                        && ico != c_ico && ico != d_ico) {
                        if (pv[1] == 1) {
                            pw[0] = d * pv[0] / det;
                        } else {
                            pw[0] = -b * pv[0] / det;
                        }
                        pw[2] = pv[2];
                        pw[3] = pv[3];
                        pw += 4;
                    }
                }

                pw[0] = 1.0; pw[1] = 2; pw[2] = 0; pw[3] = 1;
                pw += 4;

                pv = coeffs_f;
                for (ico = 0; ico < ncoeff_f; ico++, pv += 4) {
                    pw[1] = 2;
                    if (pv[0] != 0.0 && ico != a_ico && ico != b_ico
                        && ico != c_ico && ico != d_ico) {
                        if (pv[1] == 1) {
                            pw[0] = -c * pv[0] / det;
                        } else {
                            pw[0] = a * pv[0] / det;
                        }
                        pw[2] = pv[2];
                        pw[3] = pv[3];
                        pw += 4;
                    }
                }
            }

            tmap = (AstMapping *)astPolyMap(2, 2, ncoeff_f_new, coeffs_f_new,
                                            0, NULL, " ");
            *map2 = astSimplify(tmap);
            tmap = astAnnul(tmap);

            free(coeffs_f_new);
        }

        free(coeffs_f);
    }
    pm1 = astAnnul(pm1);

    if (inverted) astSet(polymap, "Invert=1");

    if (astOK && *map1 && *map2 && *map3) {
        ret = 1;
    } else {
        if (*map1) *map1 = astAnnul(*map1);
        if (*map2) *map2 = astAnnul(*map2);
        if (*map3) *map3 = astAnnul(*map3);
    }

    return ret;
}

/* Emit a fixture's analyse_poly output as a Rust constant block. */
static void emit_fixture(const char *name, AstPolyMap *pm) {
    int status = 0;
    AstMapping *map1 = NULL;
    AstMapping *map2 = NULL;
    AstMapping *map3 = NULL;
    int map3_is_unit = 0;
    int ok = analyse_poly_oracle(pm, &map1, &map2, &map3, &map3_is_unit, &status);

    printf("    /// Fixture: %s\n", name);
    printf("    pub const %s: Option<AnalysePolyFixture> = ", name);
    if (!ok) {
        printf("None;\n\n");
        return;
    }

    /* Extract the shift values from map1 (a ShiftMap). astTran2 of (0,0)
     * forward by map1 produces the shift. */
    double sx = 0.0, sy = 0.0;
    {
        double in_x = 0.0, in_y = 0.0;
        astTran2(map1, 1, &in_x, &in_y, 1, &sx, &sy);
    }

    /* Extract the matrix from map3. For UnitMap we emit identity; for
     * MatrixMap we transform the 4 corners (0,0), (1,0), (0,1) — the
     * Cartesian-basis images give us [a,c,b,d] which we shuffle to
     * row-major [a,b,c,d]. */
    double mat[4] = {1.0, 0.0, 0.0, 1.0};
    if (!map3_is_unit) {
        double in_x[3] = {0.0, 1.0, 0.0};
        double in_y[3] = {0.0, 0.0, 1.0};
        double out_x[3], out_y[3];
        astTran2(map3, 3, in_x, in_y, 1, out_x, out_y);
        mat[0] = out_x[1]; /* a = matrix · e0 → out0 component */
        mat[1] = out_x[2]; /* b */
        mat[2] = out_y[1]; /* c */
        mat[3] = out_y[2]; /* d */
    }

    /* Extract map2's coefficients. */
    int ncoeff_f = 0;
    astPolyCoeffs((AstPolyMap *)map2, 1, 0, NULL, &ncoeff_f);
    double *cf = (double *)malloc((size_t)(ncoeff_f * 4) * sizeof(double));
    astPolyCoeffs((AstPolyMap *)map2, 1, ncoeff_f * 4, cf, &ncoeff_f);

    printf("Some(AnalysePolyFixture {\n");
    printf("        shift: [\n");
    emit_double(sx);
    emit_double(sy);
    printf("        ],\n");
    printf("        matrix: [\n");
    for (int i = 0; i < 4; ++i) emit_double(mat[i]);
    printf("        ],\n");
    printf("        map3_is_unit: %s,\n", map3_is_unit ? "true" : "false");
    printf("        coeffs: &[\n");
    for (int i = 0; i < ncoeff_f; ++i) {
        printf("            (");
        char buf[64];
        snprintf(buf, sizeof(buf), "%.17g", cf[i * 4 + 0]);
        if (strchr(buf, '.') == NULL && strchr(buf, 'e') == NULL) {
            printf("%s.0", buf);
        } else {
            printf("%s", buf);
        }
        printf(", %d, %d, %d),\n",
               (int)cf[i * 4 + 1], (int)cf[i * 4 + 2], (int)cf[i * 4 + 3]);
    }
    printf("        ],\n");
    printf("    });\n\n");

    free(cf);
}

/* Helper: build a 2-in/2-out PolyMap with iter_inverse=1 from a
 * coefficient array. Coefficients use the C astPolyMap layout:
 * groups of (coeff, out_axis, p0, p1). */
static AstPolyMap *build_pm(int ncoeff, double *coeffs) {
    AstPolyMap *pm = astPolyMap(2, 2, ncoeff, coeffs, 0, NULL, " ");
    astSet(pm, "IterInverse=1");
    return pm;
}

int main(void) {
    astBegin;

    printf("// Auto-generated by tools/oracle_fixtures/analyse_poly.c.\n");
    printf("// Do not edit by hand — regenerate with `make analyse_poly`.\n\n");

    /* Fixture 1: SIP-shape — identity linear, only quadratic distortion.
     *   y0 = 1*x0 + 0.001*x0*x1
     *   y1 = 1*x1 + 0.002*x0*x0
     * No constant terms, identity 2x2 linear matrix → expect:
     *   shift=(0,0), map3=Unit, map2 = original. */
    {
        double cf[] = {
            1.0, 1, 1, 0,
            0.001, 1, 1, 1,
            1.0, 2, 0, 1,
            0.002, 2, 2, 0,
        };
        AstPolyMap *pm = build_pm(4, cf);
        emit_fixture("ANALYSE_POLY_SIP_SHAPE", pm);
    }

    /* Fixture 2: PolyMap with constant terms.
     *   y0 = 5 + x0 + x0*x1
     *   y1 = 7 + x1 + x0*x0
     * After analyse, map1 should shift origin away from (5, 7) image.
     * The constant terms get folded into shift; matrix stays identity. */
    {
        double cf[] = {
            5.0, 1, 0, 0,
            1.0, 1, 1, 0,
            1.0, 1, 1, 1,
            7.0, 2, 0, 0,
            1.0, 2, 0, 1,
            1.0, 2, 2, 0,
        };
        AstPolyMap *pm = build_pm(6, cf);
        emit_fixture("ANALYSE_POLY_WITH_CONSTANT", pm);
    }

    /* Fixture 3: PolyMap with rotation+scaling.
     *   y0 = 2*x0 + 3*x1 + 0.01*x0*x1
     *   y1 = -1*x0 + 4*x1 + 0.02*x0*x0
     * Linear matrix [[2,3],[-1,4]], det=11. Should produce a
     * non-identity MatrixMap. */
    {
        double cf[] = {
            2.0, 1, 1, 0,
            3.0, 1, 0, 1,
            0.01, 1, 1, 1,
            -1.0, 2, 1, 0,
            4.0, 2, 0, 1,
            0.02, 2, 2, 0,
        };
        AstPolyMap *pm = build_pm(6, cf);
        emit_fixture("ANALYSE_POLY_ROTATION_SCALE", pm);
    }

    /* Fixture 4: Singular linear part (det == 0 and not identity).
     *   y0 = 1*x0 + 1*x1 + 0.01*x0*x0
     *   y1 = 1*x0 + 1*x1 + 0.01*x0*x0
     * Linear matrix [[1,1],[1,1]], det=0. AnalysePoly should
     * return 0 (None in Rust). */
    {
        double cf[] = {
            1.0, 1, 1, 0,
            1.0, 1, 0, 1,
            0.01, 1, 2, 0,
            1.0, 2, 1, 0,
            1.0, 2, 0, 1,
            0.01, 2, 2, 0,
        };
        AstPolyMap *pm = build_pm(6, cf);
        emit_fixture("ANALYSE_POLY_SINGULAR", pm);
    }

    astEnd;
    return 0;
}
