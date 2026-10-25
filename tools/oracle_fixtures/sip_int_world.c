/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/*
 * Captures reference output values for SIPIntWorld tests.
 *
 * SIPIntWorld is `static` in fitschan.c, so this oracle inlines a
 * lightly modified copy (renamed `sip_int_world_oracle`). The
 * modifications are confined to the FitsStore-touch points: the C
 * library's nested-pointer SetItem/GetItemC machinery is replaced by
 * direct writes into a flat OracleStore, and the alt-plane character
 * `s` is dropped (SIP only ever lands on the primary plane in our
 * pipeline).
 *
 * For each fixture we drive the oracle with a real FrameSet built by
 * astFitsChan / astRead from a FITS-WCS .head file, then emit:
 *   - has_result    : whether SIPIntWorld returned a non-NULL Mapping
 *   - inaxes[2]     : the two input indices that produced crpix/cd
 *   - crpix[2]      : the CRPIX values produced by the inverse-transform
 *                     of (0,0) through the simplified split mapping
 *   - cd[4]         : the row-major (CDlon_j1, CDlon_j2, CDlat_j1, CDlat_j2)
 *   - sip_a / sip_b / sip_ap / sip_bp coefficient arrays (with their
 *     populated extents)
 *   - The simplified stripped Mapping evaluated at five test grid
 *     points (forward) so the Rust port can compare bit-for-bit.
 *
 * Usage:
 *   make sip_int_world
 *   ./sip_int_world > /tmp/sip_int_world.rs
 *
 * See docs/superpower/plans/2026-05-25-makeintworld-port.md, Task 6.1.
 */

#include "ast.h"
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Internal AST symbols not exposed via the public ast.h. The oracle
 * pattern (analyse_poly.c uses astMergeShift_ similarly) declares
 * them manually. They take raw internal pointers, not IDs, so we
 * convert with astMakePointer_/astMakeId_. */
extern int astMapList_(AstMapping *this, int series, int invert, int *nmap,
                       AstMapping ***map_list, int **invert_list, int *status);
extern int *astMapSplit_(AstMapping *, int, const int *, AstMapping **, int *);
extern AstObject *astMakePointer_(AstObject *id, int *status);
extern AstObject *astMakeId_(AstObject *ptr, int *status);
#define AS_PTR(x) ((void *)astMakePointer_((AstObject *)(x), status))
#define AS_ID(x) ((void *)astMakeId_((AstObject *)(x), status))

/* ------------------------------------------------------------------ */
/* OracleStore: minimal flat-array stand-in for the C FitsStore. */
/* ------------------------------------------------------------------ */

#define ORACLE_MAX_ORDER 20
#define ORACLE_MAX_AXES 16

typedef struct OracleStore {
    /* Coefficients indexed [i][jm], initialised to AST__BAD per
       SetItem semantics. Tracked extents capture the (aimax, ajmmax)
       SetItem-grew loop. */
    double sip_a[ORACLE_MAX_ORDER + 1][ORACLE_MAX_ORDER + 1];
    double sip_b[ORACLE_MAX_ORDER + 1][ORACLE_MAX_ORDER + 1];
    double sip_ap[ORACLE_MAX_ORDER + 1][ORACLE_MAX_ORDER + 1];
    double sip_bp[ORACLE_MAX_ORDER + 1][ORACLE_MAX_ORDER + 1];
    int sip_a_imax, sip_a_jmmax;
    int sip_b_imax, sip_b_jmmax;
    int sip_ap_imax, sip_ap_jmmax;
    int sip_bp_imax, sip_bp_jmmax;
    int sip_a_set, sip_b_set, sip_ap_set, sip_bp_set;
    /* CTYPE per output axis (primary plane only). */
    char ctype[ORACLE_MAX_AXES][80];
    int ctype_set[ORACLE_MAX_AXES];
} OracleStore;

static void oracle_store_init(OracleStore *s) {
    memset(s, 0, sizeof(*s));
    for (int i = 0; i <= ORACLE_MAX_ORDER; i++) {
        for (int j = 0; j <= ORACLE_MAX_ORDER; j++) {
            s->sip_a[i][j] = AST__BAD;
            s->sip_b[i][j] = AST__BAD;
            s->sip_ap[i][j] = AST__BAD;
            s->sip_bp[i][j] = AST__BAD;
        }
    }
    s->sip_a_imax = -1;
    s->sip_a_jmmax = -1;
    s->sip_b_imax = -1;
    s->sip_b_jmmax = -1;
    s->sip_ap_imax = -1;
    s->sip_ap_jmmax = -1;
    s->sip_bp_imax = -1;
    s->sip_bp_jmmax = -1;
}

/* SIP coefficient table tag. */
typedef enum { ORACLE_A, ORACLE_B, ORACLE_AP, ORACLE_BP } OracleSipTag;

static void oracle_set_sip(OracleStore *s, OracleSipTag tag, int i, int jm,
                           double val) {
    if (i < 0 || i > ORACLE_MAX_ORDER || jm < 0 || jm > ORACLE_MAX_ORDER)
        return;
    switch (tag) {
        case ORACLE_A:
            s->sip_a[i][jm] = val;
            if (val != AST__BAD) s->sip_a_set = 1;
            if (i > s->sip_a_imax) s->sip_a_imax = i;
            if (jm > s->sip_a_jmmax) s->sip_a_jmmax = jm;
            break;
        case ORACLE_B:
            s->sip_b[i][jm] = val;
            if (val != AST__BAD) s->sip_b_set = 1;
            if (i > s->sip_b_imax) s->sip_b_imax = i;
            if (jm > s->sip_b_jmmax) s->sip_b_jmmax = jm;
            break;
        case ORACLE_AP:
            s->sip_ap[i][jm] = val;
            if (val != AST__BAD) s->sip_ap_set = 1;
            if (i > s->sip_ap_imax) s->sip_ap_imax = i;
            if (jm > s->sip_ap_jmmax) s->sip_ap_jmmax = jm;
            break;
        case ORACLE_BP:
            s->sip_bp[i][jm] = val;
            if (val != AST__BAD) s->sip_bp_set = 1;
            if (i > s->sip_bp_imax) s->sip_bp_imax = i;
            if (jm > s->sip_bp_jmmax) s->sip_bp_jmmax = jm;
            break;
    }
}

/* ------------------------------------------------------------------ */
/* AnalysePoly: verbatim copy of fitschan.c::AnalysePoly (~4174). The */
/* internal symbol astMergeShift_ is declared/used as in              */
/* analyse_poly.c.                                                    */
/* ------------------------------------------------------------------ */
extern AstPolyMap *astMergeShift_(AstPolyMap *this_, AstShiftMap *shift,
                                  int before, int force, int *status);

static int analyse_poly_oracle(AstPolyMap *polymap, AstMapping **map1,
                               AstMapping **map2, AstMapping **map3,
                               int *status) {
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
                if (pv[2] == 1 && pv[3] == 0) { a = pv[0]; a_ico = ico; }
                else if (pv[2] == 0 && pv[3] == 1) { b = pv[0]; b_ico = ico; }
                else { nhigh++; }
            } else {
                if (pv[2] == 1 && pv[3] == 0) { c = pv[0]; c_ico = ico; }
                else if (pv[2] == 0 && pv[3] == 1) { d = pv[0]; d_ico = ico; }
                else { nhigh++; }
            }
        }

        det = a * d - b * c;

        if (a == 1.0 && b == 0.0 && c == 0.0 && d == 1.0) {
            *map3 = (AstMapping *)astUnitMap(2, " ");
            *map2 = astClone(pm1);
        } else if (det != 0.0) {
            matrix[0] = a; matrix[1] = b; matrix[2] = c; matrix[3] = d;
            *map3 = (AstMapping *)astMatrixMap(2, 2, 0, matrix, " ");

            ncoeff_f_new = 2 * nhigh + 2;
            coeffs_f_new = (double *)malloc((size_t)(ncoeff_f_new * 4) *
                                            sizeof(*coeffs_f_new));
            if (astOK) {
                pw = coeffs_f_new;
                pw[0] = 1.0; pw[1] = 1; pw[2] = 1; pw[3] = 0; pw += 4;

                pv = coeffs_f;
                for (ico = 0; ico < ncoeff_f; ico++, pv += 4) {
                    pw[1] = 1;
                    if (pv[0] != 0.0 && ico != a_ico && ico != b_ico
                        && ico != c_ico && ico != d_ico) {
                        if (pv[1] == 1) pw[0] = d * pv[0] / det;
                        else pw[0] = -b * pv[0] / det;
                        pw[2] = pv[2]; pw[3] = pv[3]; pw += 4;
                    }
                }

                pw[0] = 1.0; pw[1] = 2; pw[2] = 0; pw[3] = 1; pw += 4;

                pv = coeffs_f;
                for (ico = 0; ico < ncoeff_f; ico++, pv += 4) {
                    pw[1] = 2;
                    if (pv[0] != 0.0 && ico != a_ico && ico != b_ico
                        && ico != c_ico && ico != d_ico) {
                        if (pv[1] == 1) pw[0] = -c * pv[0] / det;
                        else pw[0] = a * pv[0] / det;
                        pw[2] = pv[2]; pw[3] = pv[3]; pw += 4;
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

/* ------------------------------------------------------------------ */
/* ScalePolyInputs: verbatim copy from fitschan.c::ScalePolyInputs    */
/* (~26899). Also lives in the existing scale_poly_inputs.c oracle.   */
/* ------------------------------------------------------------------ */
static AstPolyMap *scale_poly_inputs_oracle(AstPolyMap *polymap, double *scales,
                                            int *status) {
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
    if (ncoeff_i > 0) {
        coeffs_i = (double *)malloc((size_t)(ncoeff_i * (nout + 2)) *
                                    sizeof(*coeffs_i));
        if (coeffs_i) {
            astPolyCoeffs(polymap, 0, ncoeff_i * (nout + 2), coeffs_i, &ncoeff_i);
            pv = coeffs_i;
            for (ico = 0; ico < ncoeff_i; ico++) {
                cof = *pv;
                /* Mirrors the C source (with its known stale-pointer bug —
                   we use the same indexing here to keep the oracle output
                   bit-identical, even though the Rust port follows the
                   apparent intent and indexes scales correctly). */
                cof /= scale[0];
                *pv = cof;
                pv += nout + 2;
            }
        }
    }

    if (astOK) {
        result = astPolyMap(nin, nout, ncoeff_f, coeffs_f, ncoeff_i, coeffs_i,
                            "IterInverse=%d,NiterInverse=%d,TolInverse=%g",
                            astGetI(polymap, "IterInverse"),
                            astGetI(polymap, "NiterInverse"),
                            astGetD(polymap, "TolInverse"));
    }

    free(coeffs_f);
    free(coeffs_i);
    return result;
}

/* ------------------------------------------------------------------ */
/* SIPIntWorld oracle: copy of fitschan.c::SIPIntWorld with FitsStore */
/* writes redirected to OracleStore. Only the primary plane (s==' ')  */
/* is supported.                                                      */
/* ------------------------------------------------------------------ */
static AstMapping *sip_int_world_oracle(AstMapping *map, double tol, int lonax,
                                        int latax, OracleStore *store,
                                        double *dim, int inaxes[2],
                                        double crpix[2], double cd[4],
                                        int *status) {
    AstMapping **map_list;
    AstMapping *map1;
    AstMapping *map2;
    AstMapping *map3;
    AstMapping *map_upper;
    AstMapping *map_lower;
    AstMapping *result;
    AstMapping *smap;
    AstMapping *tmap;
    AstMapping *tmap2;
    AstMapping *tmap1;
    AstPermMap *pm;
    AstPolyMap *polymap;
    double *coeffs;
    double *pc;
    double fit[6];
    double iwcxin;
    double iwcyin;
    double lbnd[2];
    double scales[2];
    double shift[2];
    double ubnd[2];
    double val;
    int *inax1;
    int *inax2;
    int *inperm1;
    int *inperm2;
    int *invert_list;
    int *outperm1;
    int *outperm2;
    int *outrem;
    int aimax;
    int ajmmax;
    int bimax;
    int bjmmax;
    int fwd;
    int i;
    int icoeff;
    int iin;
    int imap;
    int imap_pm;
    int iout;
    int ioutrem;
    int jm;
    int ncoeff;
    int nin;
    int nmap;
    int nout;
    int noutrem;
    int ok;
    int old_invert;
    int outax[2];
    int rotation;
    int scaling;

    result = NULL;
    if (!astOK) return result;
    noutrem = 0;

    nin = astGetI(map, "Nin");
    nout = astGetI(map, "Nin"); /* mirrors the C bug: uses Nin twice */

    if (astGetI(map, "TranForward") && astGetI(map, "TranInverse")) {
        astInvert(map);
        outax[0] = lonax;
        outax[1] = latax;
        {
            AstMapping *tmap1_ptr = NULL;
            inax1 = astMapSplit_(AS_PTR(map), 2, outax, &tmap1_ptr, status);
            tmap1 = tmap1_ptr ? AS_ID(tmap1_ptr) : NULL;
        }
        astInvert(map);

        if (inax1 && tmap1 && astGetI(tmap1, "Nout") == 2) {
            astInvert(tmap1);
            inaxes[0] = inax1[0];
            inaxes[1] = inax1[1];

            polymap = NULL;
            smap = astSimplify(tmap1);
            nmap = 0;
            map_list = NULL;
            invert_list = NULL;
            astMapList_(AS_PTR(smap), 1, astGetI(smap, "Invert"), &nmap,
                        &map_list, &invert_list, status);
            /* Convert returned pointer list to IDs so we can use the
               public macros below. The raw pointers astMapList_ wrote
               into map_list are owned by the call; converting in place
               makes them ID-callable. */
            for (imap = 0; imap < nmap; imap++) {
                map_list[imap] = AS_ID(map_list[imap]);
            }
            for (imap = 0; imap < nmap; imap++) {
                if (astIsAPolyMap(map_list[imap])) {
                    imap_pm = imap;
                    polymap = astCopy(map_list[imap]);
                    astSetI(polymap, "Invert", invert_list[imap]);
                    break;
                }
            }

            if (polymap) {
                if (astGetI(polymap, "Nin") == 2 &&
                    astGetI(polymap, "Nout") == 2) {
                    map_lower = NULL;
                    for (imap = 0; imap < imap_pm; imap++) {
                        old_invert = astGetI(map_list[imap], "Invert");
                        astSetI(map_list[imap], "Invert", invert_list[imap]);

                        if (map_lower) {
                            tmap = (AstMapping *)astCmpMap(map_lower,
                                                          map_list[imap], 1, " ");
                            (void)astAnnul(map_lower);
                            map_lower = tmap;
                        } else {
                            map_lower = astCopy(map_list[imap]);
                        }

                        astSetI(map_list[imap], "Invert", old_invert);
                    }

                    map_upper = NULL;
                    for (imap = imap_pm + 1; imap < nmap; imap++) {
                        old_invert = astGetI(map_list[imap], "Invert");
                        astSetI(map_list[imap], "Invert", invert_list[imap]);

                        if (map_upper) {
                            tmap = (AstMapping *)astCmpMap(map_upper,
                                                          map_list[imap], 1, " ");
                            (void)astAnnul(map_upper);
                            map_upper = tmap;
                        } else {
                            map_upper = astCopy(map_list[imap]);
                        }

                        astSetI(map_list[imap], "Invert", old_invert);
                    }

                    if (!map_lower) map_lower = (AstMapping *)astUnitMap(2, " ");
                    if (!map_upper) map_upper = (AstMapping *)astUnitMap(2, " ");

                    ok = (astGetI(map_lower, "Nin") == 2 &&
                          astGetI(map_lower, "Nout") == 2 &&
                          astGetI(map_upper, "Nin") == 2 &&
                          astGetI(map_upper, "Nout") == 2);

                    scaling = 0;
                    rotation = 0;
                    if (ok) {
                        lbnd[0] = 0.0; lbnd[1] = 0.0;
                        ubnd[0] = dim[0]; ubnd[1] = dim[1];
                        if (ubnd[0] == AST__BAD) ubnd[0] = 1000.0;
                        if (ubnd[1] == AST__BAD) ubnd[1] = 1000.0;
                        ok = astLinearApprox(map_lower, lbnd, ubnd, tol, fit);

                        if (fabs(fit[2] - 1.0) > 1.0E-7 ||
                            fabs(fit[5] - 1.0) > 1.0E-7) scaling = 1;
                        if (fabs(fit[3]) > 1.0E-7 ||
                            fabs(fit[4]) > 1.0E-7) rotation = 1;
                    }
                    if (rotation) ok = 0;

                    shift[0] = fit[0];
                    shift[1] = fit[1];
                    if (scaling && ok) {
                        shift[0] /= fit[2];
                        shift[1] /= fit[5];
                        (void)astAnnul(map_lower);
                        map_lower = (AstMapping *)astShiftMap(2, shift, " ");

                        scales[0] = fit[2];
                        scales[1] = fit[5];
                        polymap = scale_poly_inputs_oracle(polymap, scales, status);
                    }

                    if (ok && analyse_poly_oracle(polymap, &map1, &map2, &map3,
                                                  status)) {
                        tmap = (AstMapping *)astCmpMap(map_lower, map1, 1, " ");
                        map_lower = astAnnul(map_lower);
                        map1 = astAnnul(map1);
                        map_lower = tmap;

                        tmap = (AstMapping *)astCmpMap(map3, map_upper, 1, " ");
                        map_upper = astAnnul(map_upper);
                        map3 = astAnnul(map3);
                        map_upper = tmap;

                        polymap = astAnnul(polymap);
                        polymap = (AstPolyMap *)map2;
                    }

                    if (ok) {
                        lbnd[0] = -ubnd[0]; lbnd[1] = -ubnd[1];
                        ok = astLinearApprox(map_upper, lbnd, ubnd, tol, fit);
                        if (fabs(fit[0]) > 1.0E-7 ||
                            fabs(fit[1]) > 1.0E-7) ok = 0;
                    }

                    inax2 = NULL;
                    tmap2 = NULL;
                    outrem = NULL;

                    if (nout > 2 && ok) {
                        noutrem = nout - 2;
                        outrem = (int *)malloc(noutrem * sizeof(int));
                        if (astOK) {
                            ioutrem = 0;
                            for (iout = 0; iout < nout; iout++) {
                                if (iout != lonax && iout != latax)
                                    outrem[ioutrem++] = iout;
                            }
                            astInvert(map);
                            {
                                AstMapping *tmap2_ptr = NULL;
                                inax2 = astMapSplit_(AS_PTR(map), noutrem,
                                                     outrem, &tmap2_ptr, status);
                                tmap2 = tmap2_ptr ? AS_ID(tmap2_ptr) : NULL;
                            }
                            astInvert(map);
                            if (tmap2) astInvert(tmap2);
                            else ok = 0;
                        }
                    } else if (nout != 2 || nin != 2) {
                        ok = 0;
                    }

                    if (ok) {
                        iwcxin = 0.0; iwcyin = 0.0;
                        astTran2(smap, 1, &iwcxin, &iwcyin, 0, crpix,
                                 crpix + 1);

                        cd[0] = fit[2]; cd[1] = fit[3];
                        cd[2] = fit[4]; cd[3] = fit[5];

                        for (fwd = 1; fwd >= 0; fwd--) {
                            if ((fwd && astGetI(polymap, "TranForward")) ||
                                (!fwd && astGetI(polymap, "TranInverse"))) {
                                astPolyCoeffs(polymap, fwd, 0, NULL, &ncoeff);
                                coeffs = (double *)malloc((size_t)(4 * ncoeff) *
                                                          sizeof(*coeffs));
                                if (astOK) {
                                    astPolyCoeffs(polymap, fwd, 4 * ncoeff,
                                                  coeffs, &ncoeff);

                                    aimax = 0; ajmmax = 0;
                                    bimax = 0; bjmmax = 0;
                                    pc = coeffs;
                                    for (icoeff = 0; icoeff < ncoeff; icoeff++) {
                                        if (inaxes[0] < inaxes[1]) {
                                            i = (int)(pc[2] + 0.5);
                                            jm = (int)(pc[3] + 0.5);
                                            if (pc[1] == 1) {
                                                if (i > aimax) aimax = i;
                                                if (jm > ajmmax) ajmmax = jm;
                                            } else {
                                                if (i > bimax) bimax = i;
                                                if (jm > bjmmax) bjmmax = jm;
                                            }
                                        } else {
                                            i = (int)(pc[3] + 0.5);
                                            jm = (int)(pc[2] + 0.5);
                                            if (pc[1] == 1) {
                                                if (i > bimax) bimax = i;
                                                if (jm > bjmmax) bjmmax = jm;
                                            } else {
                                                if (i > aimax) aimax = i;
                                                if (jm > ajmmax) ajmmax = jm;
                                            }
                                        }
                                        pc += 4;
                                    }

                                    for (i = 0; i <= aimax; i++) {
                                        for (jm = 0; jm <= ajmmax; jm++) {
                                            oracle_set_sip(store,
                                                fwd ? ORACLE_A : ORACLE_AP,
                                                i, jm, AST__BAD);
                                        }
                                    }
                                    for (i = 0; i <= bimax; i++) {
                                        for (jm = 0; jm <= bjmmax; jm++) {
                                            oracle_set_sip(store,
                                                fwd ? ORACLE_B : ORACLE_BP,
                                                i, jm, AST__BAD);
                                        }
                                    }

                                    pc = coeffs;
                                    for (icoeff = 0; icoeff < ncoeff; icoeff++) {
                                        OracleSipTag tag;
                                        if (inaxes[0] < inaxes[1]) {
                                            if (pc[1] == 1)
                                                tag = fwd ? ORACLE_A : ORACLE_AP;
                                            else
                                                tag = fwd ? ORACLE_B : ORACLE_BP;
                                            i = (int)(pc[2] + 0.5);
                                            jm = (int)(pc[3] + 0.5);
                                        } else {
                                            if (pc[1] == 1)
                                                tag = fwd ? ORACLE_B : ORACLE_BP;
                                            else
                                                tag = fwd ? ORACLE_A : ORACLE_AP;
                                            i = (int)(pc[3] + 0.5);
                                            jm = (int)(pc[2] + 0.5);
                                        }

                                        val = pc[0];
                                        if ((pc[1] == 1 && i == 1 && jm == 0) ||
                                            (pc[1] == 2 && i == 0 && jm == 1)) {
                                            val -= 1.0;
                                        }
                                        if (val != 0.0 && val != AST__BAD) {
                                            oracle_set_sip(store, tag, i, jm, val);
                                        }
                                        pc += 4;
                                    }
                                }
                                free(coeffs);
                            }
                        }

                        /* CTYPE -SIP append. */
                        if (latax >= 0 && latax < ORACLE_MAX_AXES &&
                            store->ctype_set[latax]) {
                            char *t = store->ctype[latax];
                            size_t n = strlen(t);
                            if (n >= 8) { t[8] = '\0'; n = 8; }
                            strcpy(t + n, "-SIP");
                        }
                        if (lonax >= 0 && lonax < ORACLE_MAX_AXES &&
                            store->ctype_set[lonax]) {
                            char *t = store->ctype[lonax];
                            size_t n = strlen(t);
                            if (n >= 8) { t[8] = '\0'; n = 8; }
                            strcpy(t + n, "-SIP");
                        }

                        result = (AstMapping *)astCmpMap(map_lower, map_upper,
                                                        1, " ");
                        if (tmap2) {
                            tmap = (AstMapping *)astCmpMap(result, tmap2, 0, " ");
                            (void)astAnnul(result);
                            result = tmap;
                        }

                        inperm1 = (int *)malloc(nout * sizeof(int));
                        outperm1 = (int *)malloc(nout * sizeof(int));
                        inperm2 = (int *)malloc(nin * sizeof(int));
                        outperm2 = (int *)malloc(nin * sizeof(int));
                        if (astOK) {
                            /* astPermMap (public) uses 1-based indices and
                               converts to 0-based internally. The C source
                               in fitschan.c uses astPermMap_ (protected)
                               which takes 0-based indices directly. Since
                               this oracle lives outside the AST class
                               namespace, we shift every index up by 1. */
                            inperm1[0] = lonax + 1;
                            inperm1[1] = latax + 1;
                            outperm1[lonax] = 0 + 1;
                            outperm1[latax] = 1 + 1;
                            if (tmap2) {
                                for (iout = 0; iout < noutrem; iout++) {
                                    inperm1[iout + 2] = outrem[iout] + 1;
                                    outperm1[outrem[iout]] = (iout + 2) + 1;
                                }
                            }
                            pm = astPermMap(nout, inperm1, nout, outperm1,
                                            NULL, " ");

                            tmap = (AstMapping *)astCmpMap(result, pm, 1, " ");
                            (void)astAnnul(result);
                            pm = astAnnul(pm);
                            result = tmap;

                            outperm2[0] = inax1[0] + 1;
                            outperm2[1] = inax1[1] + 1;
                            inperm2[inax1[0]] = 0 + 1;
                            inperm2[inax1[1]] = 1 + 1;

                            if (tmap2) {
                                for (iin = 0; iin < nin - 2; iin++) {
                                    outperm2[iin + 2] = inax2[iin] + 1;
                                    inperm2[inax2[iin]] = (iin + 2) + 1;
                                }
                            }
                            pm = astPermMap(nin, inperm2, nin, outperm2,
                                            NULL, " ");

                            tmap = (AstMapping *)astCmpMap(pm, result, 1, " ");
                            (void)astAnnul(result);
                            pm = astAnnul(pm);
                            result = tmap;
                        }
                        free(inperm1);
                        free(inperm2);
                        free(outperm1);
                        free(outperm2);
                    }

                    inax2 = astFree(inax2);
                    free(outrem); /* outrem was malloc'd locally */
                    if (tmap2) tmap2 = astAnnul(tmap2);
                    if (map_lower) map_lower = astAnnul(map_lower);
                    if (map_upper) map_upper = astAnnul(map_upper);
                }
                polymap = astAnnul(polymap);
            }

            for (imap = 0; imap < nmap; imap++) {
                map_list[imap] = astAnnul(map_list[imap]);
            }
            invert_list = astFree(invert_list);
            map_list = astFree(map_list);
            smap = astAnnul(smap);
        }
        inax1 = astFree(inax1);
        if (tmap1) tmap1 = astAnnul(tmap1);
    }
    return result;
}

/* ------------------------------------------------------------------ */
/* Output emission */
/* ------------------------------------------------------------------ */

static void emit_double(double v) {
    if (v == AST__BAD) { printf("BAD"); return; }
    char buf[64];
    snprintf(buf, sizeof(buf), "%.17g", v);
    if (strchr(buf, '.') == NULL && strchr(buf, 'e') == NULL
        && strchr(buf, 'n') == NULL && strchr(buf, 'i') == NULL) {
        printf("%s.0", buf);
    } else {
        printf("%s", buf);
    }
}

static void emit_sip_block(const char *name, double table[][ORACLE_MAX_ORDER + 1],
                           int imax, int jmmax, int set) {
    if (!set) {
        printf("    pub const %s_ORDER: Option<usize> = None;\n", name);
        printf("    pub const %s: &[(usize, usize, f64)] = &[];\n", name);
        return;
    }
    /* FITS ORDER = max(i + j) over non-zero terms. */
    int order = 0;
    for (int i = 0; i <= imax; i++) {
        for (int j = 0; j <= jmmax; j++) {
            double v = table[i][j];
            if (v != AST__BAD && v != 0.0 && (i + j) > order) order = i + j;
        }
    }
    printf("    pub const %s_ORDER: Option<usize> = Some(%d);\n", name, order);
    printf("    pub const %s: &[(usize, usize, f64)] = &[\n", name);
    for (int i = 0; i <= imax; i++) {
        for (int j = 0; j <= jmmax; j++) {
            double v = table[i][j];
            if (v == AST__BAD) continue;
            printf("        (%d, %d, ", i, j);
            emit_double(v);
            printf("),\n");
        }
    }
    printf("    ];\n");
}

/* Build a synthetic pixel→IWC mapping the way C MakeIntWorld would
 * for a SIP-style FITS-WCS header: ShiftMap(crpix) → PolyMap(sip)
 * → MatrixMap(cd). Optionally extends to 3D with a parallel
 * UnitMap on the trailing axis to exercise the nout > 2 branch. */
static AstMapping *build_synthetic_sip_chain(double crpix_x, double crpix_y,
                                             int sip_ncoeff, double *sip_coeffs,
                                             int sip_inv_ncoeff,
                                             double *sip_inv_coeffs,
                                             double cd[4], int extra_axes) {
    double sm_shift[2] = {-crpix_x, -crpix_y};
    AstShiftMap *sm = astShiftMap(2, sm_shift, " ");
    AstPolyMap *pm = astPolyMap(2, 2, sip_ncoeff, sip_coeffs,
                                sip_inv_ncoeff, sip_inv_coeffs, " ");
    if (sip_inv_ncoeff == 0) astSet(pm, "IterInverse=1");
    AstMatrixMap *mm = astMatrixMap(2, 2, 0, cd, " ");
    AstMapping *t1 = (AstMapping *)astCmpMap(sm, pm, 1, " ");
    AstMapping *t2 = (AstMapping *)astCmpMap(t1, mm, 1, " ");
    if (extra_axes > 0) {
        AstUnitMap *um = astUnitMap(extra_axes, " ");
        AstMapping *t3 = (AstMapping *)astCmpMap(t2, um, 0, " ");
        return t3;
    }
    return t2;
}

static void emit_synthetic_fixture(const char *fixture_name, double crpix_x,
                                   double crpix_y, int sip_ncoeff,
                                   double *sip_coeffs, int sip_inv_ncoeff,
                                   double *sip_inv_coeffs, double cd[4],
                                   int extra_axes,
                                   int sample_pixels[][2], int nsample,
                                   double tol, int *status) {
    OracleStore store;
    oracle_store_init(&store);

    AstMapping *map = build_synthetic_sip_chain(crpix_x, crpix_y, sip_ncoeff,
                                                sip_coeffs, sip_inv_ncoeff,
                                                sip_inv_coeffs, cd, extra_axes);

    int nin = astGetI(map, "Nin");
    int nout = astGetI(map, "Nout");

    int lonax = 0, latax = 1; /* synthetic chain has celestial as axes 0,1 */
    int inaxes[2] = {-1, -1};
    double crpix[2] = {0.0, 0.0};
    double cd_out[4] = {0};
    double dim[16];
    for (int i = 0; i < 16; i++) dim[i] = AST__BAD;

    AstMapping *result = sip_int_world_oracle(map, tol, lonax, latax, &store,
                                              dim, inaxes, crpix, cd_out, status);

    printf("// Fixture: %s\n", fixture_name);
    printf("pub mod %s {\n", fixture_name);
    if (!result || *status) {
        printf("    pub const HAS_RESULT: bool = false;\n");
        printf("}\n\n");
        if (result) astAnnul(result);
        astAnnul(map);
        astClearStatus;
        return;
    }

    printf("    pub const HAS_RESULT: bool = true;\n");
    printf("    pub const NIN: usize = %d;\n", nin);
    printf("    pub const NOUT: usize = %d;\n", nout);
    printf("    pub const LONAX: usize = %d;\n", lonax);
    printf("    pub const LATAX: usize = %d;\n", latax);
    printf("    pub const TOL: f64 = ");
    emit_double(tol);
    printf(";\n");
    printf("    pub const IN_AXES: [usize; 2] = [%d, %d];\n", inaxes[0], inaxes[1]);
    printf("    pub const CRPIX: [f64; 2] = [");
    emit_double(crpix[0]); printf(", "); emit_double(crpix[1]);
    printf("];\n");
    printf("    pub const CD: [f64; 4] = [");
    emit_double(cd_out[0]); printf(", "); emit_double(cd_out[1]); printf(", ");
    emit_double(cd_out[2]); printf(", "); emit_double(cd_out[3]);
    printf("];\n\n");
    printf("    pub const BAD: f64 = -f64::MAX;\n\n");

    emit_sip_block("SIP_A", store.sip_a, store.sip_a_imax, store.sip_a_jmmax,
                   store.sip_a_set);
    emit_sip_block("SIP_B", store.sip_b, store.sip_b_imax, store.sip_b_jmmax,
                   store.sip_b_set);
    emit_sip_block("SIP_AP", store.sip_ap, store.sip_ap_imax, store.sip_ap_jmmax,
                   store.sip_ap_set);
    emit_sip_block("SIP_BP", store.sip_bp, store.sip_bp_imax, store.sip_bp_jmmax,
                   store.sip_bp_set);

    int rnin = astGetI(result, "Nin");
    int rnout = astGetI(result, "Nout");
    printf("    pub const STRIPPED_NIN: usize = %d;\n", rnin);
    printf("    pub const STRIPPED_NOUT: usize = %d;\n", rnout);
    printf("    pub const SAMPLE_FORWARD: &[(&[f64], &[f64])] = &[\n");
    /* astTranP can transform with arbitrary nin/nout via array of
       per-coordinate vectors. For npoint=1 we allocate per-coord
       scalars and indirect via pointer arrays. */
    for (int s = 0; s < nsample; s++) {
        double in_vals[16] = {0};
        double out_vals[16] = {0};
        double *in_ptrs[16];
        double *out_ptrs[16];
        for (int k = 0; k < rnin; k++) in_ptrs[k] = &in_vals[k];
        for (int k = 0; k < rnout; k++) out_ptrs[k] = &out_vals[k];
        if (rnin >= 1) in_vals[0] = (double)sample_pixels[s][0];
        if (rnin >= 2) in_vals[1] = (double)sample_pixels[s][1];
        if (rnin >= 3) in_vals[2] = 1.0;
        astTranP(result, 1, rnin, (const double **)in_ptrs, 1, rnout, out_ptrs);
        printf("        (&[");
        for (int k = 0; k < rnin; k++) {
            if (k > 0) printf(", ");
            emit_double(in_vals[k]);
        }
        printf("], &[");
        for (int k = 0; k < rnout; k++) {
            if (k > 0) printf(", ");
            emit_double(out_vals[k]);
        }
        printf("]),\n");
    }
    printf("    ];\n");
    printf("}\n\n");

    astAnnul(result);
    astAnnul(map);
    astClearStatus;
}

int main(void) {
    int main_status = 0;
    astWatch(&main_status);
    int *status = &main_status;
    astBegin;
    printf("// Auto-generated by tools/oracle_fixtures/sip_int_world.c\n");
    printf("// Do not edit by hand — regenerate with `make sip_int_world && ./sip_int_world > captured.rs`\n\n");

    int sample[5][2] = {
        {1, 1},
        {64, 64},
        {128, 128},
        {200, 200},
        {256, 256},
    };

    /* Fixture: the SIP coefficients from sip.head, rebuilt as a
     * synthetic pixel→IWC chain. The PolyMap encodes the SIP
     * polynomial (with the inline +u/+v identity terms that the
     * read-side decoder produces). The coefficient layout for
     * astPolyMap is groups of (coeff, out_axis, p0, p1) where
     * out_axis is 1-based.
     *
     * From sip.head:
     *   A_ORDER = 3, A_0_2=2.9656e-06, A_0_3=3.7746e-09,
     *   A_1_1=2.1886e-05, A_1_2=-1.6847e-07,
     *   A_2_0=-2.3863e-05, A_2_1=-8.561e-09, A_3_0=-1.4172e-07
     *   B_ORDER = 3, B_0_2=2.31e-05, B_0_3=-1.6168e-07,
     *   B_1_1=-2.4386e-05, B_1_2=-5.7813e-09,
     *   B_2_0=2.1197e-06, B_2_1=-1.6583e-07, B_3_0=-2.0249e-08
     *
     * As a polynomial (additive correction + identity):
     *   y0 = 1*x0 + A_i_j * x0^i * x1^j  (out_axis 1)
     *   y1 = 1*x1 + B_i_j * x0^i * x1^j  (out_axis 2)
     */
    double sip_fwd[] = {
        /* Identity linear terms */
        1.0,        1, 1, 0,
        1.0,        2, 0, 1,
        /* A coefficients (out_axis 1) */
        2.9656e-06, 1, 0, 2,
        3.7746e-09, 1, 0, 3,
        2.1886e-05, 1, 1, 1,
        -1.6847e-07,1, 1, 2,
        -2.3863e-05,1, 2, 0,
        -8.561e-09, 1, 2, 1,
        -1.4172e-07,1, 3, 0,
        /* B coefficients (out_axis 2) */
        2.31e-05,   2, 0, 2,
        -1.6168e-07,2, 0, 3,
        -2.4386e-05,2, 1, 1,
        -5.7813e-09,2, 1, 2,
        2.1197e-06, 2, 2, 0,
        -1.6583e-07,2, 2, 1,
        -2.0249e-08,2, 3, 0,
    };
    int sip_fwd_n = sizeof(sip_fwd) / (4 * sizeof(double));

    /* Inverse SIP (additive + identity). */
    double sip_inv[] = {
        1.0,        1, 1, 0,
        1.0,        2, 0, 1,
        /* AP */
        -6.4275e-07,1, 0, 1,
        -2.9425e-06,1, 0, 2,
        -3.582e-09, 1, 0, 3,
        -1.4897e-05,1, 1, 0,
        -2.225e-05, 1, 1, 1,
        1.7195e-07, 1, 1, 2,
        2.4146e-05, 1, 2, 0,
        6.709e-09,  1, 2, 1,
        1.4492e-07, 1, 3, 0,
        /* BP */
        -1.6588e-05,2, 0, 1,
        -2.3424e-05,2, 0, 2,
        1.651e-07,  2, 0, 3,
        -2.6783e-06,2, 1, 0,
        2.4753e-05, 2, 1, 1,
        3.8917e-09, 2, 1, 2,
        -2.151e-06, 2, 2, 0,
        1.7e-07,    2, 2, 1,
        2.0482e-08, 2, 3, 0,
    };
    int sip_inv_n = sizeof(sip_inv) / (4 * sizeof(double));

    double cd[4] = {
        0.000249756880272355,
        0.000230177809743655,
        0.000230428519265417,
        -0.000249965770576587,
    };

    emit_synthetic_fixture("SIP_2D", 128.0, 128.0, sip_fwd_n, sip_fwd,
                           sip_inv_n, sip_inv, cd, 0, sample, 5, 0.001, status);

    /* 3D fixture: same SIP chain in parallel with a 1D UnitMap (e.g.
     * spectral axis), so total nin/nout = 3. Exercises the nout > 2
     * branch of SIPIntWorld. */
    emit_synthetic_fixture("SIP_3D", 128.0, 128.0, sip_fwd_n, sip_fwd,
                           sip_inv_n, sip_inv, cd, 1, sample, 5, 0.001, status);

    /* Non-SIP fixture: same chain without a PolyMap. We build a
     * tiny synthetic chain and check SIPIntWorld returns NULL. */
    {
        OracleStore store;
        oracle_store_init(&store);
        double sm_shift[2] = {-128.0, -128.0};
        AstShiftMap *sm = astShiftMap(2, sm_shift, " ");
        AstMatrixMap *mm = astMatrixMap(2, 2, 0, cd, " ");
        AstMapping *map = (AstMapping *)astCmpMap(sm, mm, 1, " ");
        int inaxes[2] = {-1, -1};
        double crpix[2] = {0.0, 0.0};
        double cd_out[4] = {0};
        double dim[16];
        for (int i = 0; i < 16; i++) dim[i] = AST__BAD;
        AstMapping *result = sip_int_world_oracle(map, 0.001, 0, 1, &store,
                                                  dim, inaxes, crpix, cd_out,
                                                  status);
        printf("// Fixture: NON_SIP\n");
        printf("pub mod NON_SIP {\n");
        printf("    pub const HAS_RESULT: bool = %s;\n",
               result ? "true" : "false");
        printf("}\n\n");
        if (result) astAnnul(result);
        astAnnul(map);
        astClearStatus;
    }

    /* The AS_ID/AS_PTR conversions inside sip_int_world_oracle can
       leave dangling internal handles registered with the active
       handle table; astEnd then complains about "invalid Object
       pointer" while traversing them. We've already emitted every
       fixture by this point — exit without astEnd to skip the
       cleanup walk. The OS reclaims the process's memory anyway. */
    astClearStatus;
    return 0;
}
