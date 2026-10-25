/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/*
 * Captures reference output values for ScalePolyInputs tests.
 *
 * ScalePolyInputs is `static` in fitschan.c, so this oracle inlines a
 * verbatim copy (renamed `scale_poly_inputs_oracle`) and uses public
 * AST API throughout.
 *
 * For each fixture we emit the captured PolyMap's forward coefficient
 * list as `(coeff, out_axis, p0, p1, ...)` tuples — this is the data
 * a caller needs to compare bit-for-bit against the Rust port.
 *
 * Usage:
 *   make scale_poly_inputs
 *   ./scale_poly_inputs > /tmp/scale_poly_inputs.rs
 *
 * See docs/superpower/plans/2026-05-25-makeintworld-port.md, Task 2.3.
 */

#include "ast.h"
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void emit_double_inline(double v) {
    char buf[64];
    snprintf(buf, sizeof(buf), "%.17g", v);
    if (strchr(buf, '.') == NULL && strchr(buf, 'e') == NULL
        && strchr(buf, 'n') == NULL && strchr(buf, 'i') == NULL) {
        printf("%s.0", buf);
    } else {
        printf("%s", buf);
    }
}

/* Verbatim copy of fitschan.c::ScalePolyInputs (~26899). The `scale`
 * pointer is intentionally left in its post-forward-loop state when
 * processing the inverse coefficients, mirroring the C source — for
 * SIP usage this branch is never taken (iter_inverse=1, ncoeff_i=0).
 * Returns a fresh PolyMap; caller is responsible for annul. */
static AstPolyMap *scale_poly_inputs_oracle(AstPolyMap *polymap, double *scales) {
    AstPolyMap *result = NULL;
    double *coeffs_f = NULL;
    double *coeffs_i = NULL;
    double *pv;
    double *scale;
    double cof;
    int ico, iin;
    int ncoeff_f, ncoeff_i;
    int nin, nout;

    if (!astOK || !polymap) return NULL;

    nin = astGetI(polymap, "Nin");
    nout = astGetI(polymap, "Nout");

    for (iin = 0; iin < nin; iin++) {
        if (scales[iin] == 0.0 || scales[iin] == AST__BAD) {
            return NULL;
        }
    }

    astPolyCoeffs(polymap, 1, 0, NULL, &ncoeff_f);
    coeffs_f = (double *)malloc((size_t)(ncoeff_f * (nin + 2)) * sizeof(*coeffs_f));
    if (coeffs_f) {
        astPolyCoeffs(polymap, 1, ncoeff_f * (nin + 2), coeffs_f, &ncoeff_f);

        pv = coeffs_f;
        for (ico = 0; ico < ncoeff_f; ico++) {
            cof = *pv;
            pv += 2;
            scale = scales;
            for (iin = 0; iin < nin; iin++, scale++, pv++) {
                cof *= pow(*scale, *pv);
            }
            pv[-(2 + nin)] = cof;
        }
    }

    astPolyCoeffs(polymap, 0, 0, NULL, &ncoeff_i);
    coeffs_i = (double *)malloc((size_t)(ncoeff_i * (nout + 2)) * sizeof(*coeffs_i));
    if (coeffs_i) {
        astPolyCoeffs(polymap, 0, ncoeff_i * (nout + 2), coeffs_i, &ncoeff_i);

        pv = coeffs_i;
        for (ico = 0; ico < ncoeff_i; ico++, pv += 2 + nout) {
            iin = (int)pv[1] - 1;
            /* NB: `scale` here is the stale post-forward-loop pointer (see
             * docs/issues/c-library-quirks.md). Untouched in SIP usage. */
            pv[0] *= 1.0 / scale[iin];
        }
    }

    if (astOK) {
        if (astGetI(polymap, "Invert")) {
            result = astPolyMap(nout, nin, ncoeff_i, coeffs_i, ncoeff_f, coeffs_f, " ");
            astInvert(result);
        } else {
            result = astPolyMap(nin, nout, ncoeff_f, coeffs_f, ncoeff_i, coeffs_i, " ");
        }

        if (astTest(polymap, "IterInverse")) {
            astSetI(result, "IterInverse", astGetI(polymap, "IterInverse"));
        }
        if (astTest(polymap, "NiterInverse")) {
            astSetI(result, "NiterInverse", astGetI(polymap, "NiterInverse"));
        }
        if (astTest(polymap, "TolInverse")) {
            astSetD(result, "TolInverse", astGetD(polymap, "TolInverse"));
        }
    }

    free(coeffs_i);
    free(coeffs_f);

    return result;
}

static void emit_fixture(const char *name, AstPolyMap *pm, double scales[], int nscales) {
    AstPolyMap *result = scale_poly_inputs_oracle(pm, scales);
    if (!result) {
        printf("    /// Fixture: %s — oracle returned NULL\n", name);
        printf("    pub const %s: Option<&[(f64, i32, i32, i32)]> = None;\n\n", name);
        return;
    }

    int ncoeff;
    int nin = astGetI(result, "Nin");
    astPolyCoeffs(result, 1, 0, NULL, &ncoeff);
    double *cf = (double *)malloc((size_t)(ncoeff * (nin + 2)) * sizeof(double));
    astPolyCoeffs(result, 1, ncoeff * (nin + 2), cf, &ncoeff);

    printf("    /// Fixture: %s\n", name);
    printf("    /// scales = [");
    for (int i = 0; i < nscales; ++i) {
        if (i) printf(", ");
        emit_double_inline(scales[i]);
    }
    printf("]\n");
    printf("    pub const %s: Option<&[(f64, i32, i32, i32)]> = Some(&[\n", name);
    for (int i = 0; i < ncoeff; ++i) {
        double *r = cf + i * (nin + 2);
        printf("        (");
        emit_double_inline(r[0]);
        printf(", %d, %d, %d),\n", (int)r[1], (int)r[2], (int)r[3]);
    }
    printf("    ]);\n\n");

    free(cf);
}

int main(void) {
    astBegin;

    printf("// Auto-generated by tools/oracle_fixtures/scale_poly_inputs.c.\n");
    printf("// Do not edit by hand — regenerate with `make scale_poly_inputs`.\n\n");

    /* Fixture 1: SIP-shape PolyMap with uniform scaling.
     *   y0 = x0 + 0.001*x0*x1
     *   y1 = x1 + 0.002*x0^2
     * Scales = [2.0, 2.0]. Each term's coefficient gets multiplied by
     * scales[0]^p0 * scales[1]^p1. */
    {
        double cf[] = {
            1.0, 1, 1, 0,
            0.001, 1, 1, 1,
            1.0, 2, 0, 1,
            0.002, 2, 2, 0,
        };
        AstPolyMap *pm = astPolyMap(2, 2, 4, cf, 0, NULL, " ");
        astSet(pm, "IterInverse=1");
        double scales[] = {2.0, 2.0};
        emit_fixture("SCALE_POLY_INPUTS_SIP_UNIFORM", pm, scales, 2);
    }

    /* Fixture 2: same PolyMap, non-uniform scaling [3.0, 5.0]. */
    {
        double cf[] = {
            1.0, 1, 1, 0,
            0.001, 1, 1, 1,
            1.0, 2, 0, 1,
            0.002, 2, 2, 0,
        };
        AstPolyMap *pm = astPolyMap(2, 2, 4, cf, 0, NULL, " ");
        astSet(pm, "IterInverse=1");
        double scales[] = {3.0, 5.0};
        emit_fixture("SCALE_POLY_INPUTS_SIP_NONUNIFORM", pm, scales, 2);
    }

    /* Fixture 3: 3-input → 1-output PolyMap with mixed powers and scales.
     * No IterInverse (only valid for square nin==nout polymaps); we
     * still test the forward-coefficient scaling path. */
    {
        double cf[] = {
            2.0, 1, 1, 0, 0,
            3.0, 1, 0, 2, 1,
            7.0, 1, 1, 1, 1,
        };
        AstPolyMap *pm = astPolyMap(3, 1, 3, cf, 0, NULL, " ");
        double scales[] = {2.0, 3.0, 5.0};
        emit_fixture("SCALE_POLY_INPUTS_3D_MIXED", pm, scales, 3);
    }

    /* Fixture 4: zero scale → oracle returns NULL. */
    {
        double cf[] = {
            1.0, 1, 1, 0,
            1.0, 2, 0, 1,
        };
        AstPolyMap *pm = astPolyMap(2, 2, 2, cf, 0, NULL, " ");
        astSet(pm, "IterInverse=1");
        double scales[] = {0.0, 1.0};
        emit_fixture("SCALE_POLY_INPUTS_ZERO_SCALE", pm, scales, 2);
    }

    astEnd;
    return 0;
}
