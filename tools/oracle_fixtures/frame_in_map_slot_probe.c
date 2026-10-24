/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* Write the two dumps that bound what a Frame may stand in for.
 *
 * frame_in_map_slot.ast is a FrameSet whose Map<n> slot holds a bare CmpFrame.
 * C accepts a Frame wherever a Mapping is expected, because Frame::Transform
 * (frame.c:11739) transforms through astUnitMap( astGetNaxes( this ) ). The
 * program transforms through the FrameSet to show the construct is functional
 * rather than merely accepted.
 *
 * region_in_cmpframe.ast is a CmpFrame one component of which is a Box. That
 * nesting is meaningful in both libraries and needs no Frame-as-Mapping rule,
 * so it is captured alongside as the control the refusal must not catch.
 *
 * Build and run from tools/oracle_fixtures: make frame_in_map_slot_probe
 */

#include <stdio.h>
#include "ast.h"

int main(void) {
    astBegin;

    /* A 2-D pixel Frame as the base. */
    AstFrame *pix = astFrame(2, "Domain=GRID,Title=pixel grid");
    AstFrameSet *fs = astFrameSet(pix, " ");

    /* An ordinary Mapping step, so the dump has a Map1 that reads normally. */
    AstFrame *mid = astFrame(2, "Domain=MID");
    astAddFrame(fs, AST__BASE, astZoomMap(2, 2.0, " "), mid);

    /* The construct under question: a bare CmpFrame used as the Mapping. Its
       two 1-D components make it 2-in, 2-out, so it is a valid step here. */
    AstCmpFrame *cf = astCmpFrame(astFrame(1, "Domain=A"), astFrame(1, "Domain=B"), " ");
    AstFrame *top = astFrame(2, "Domain=TOP");
    astAddFrame(fs, 2, (AstMapping *) cf, top);

    /* C transforms through it: the Frame acts as a UnitMap. */
    double xin[2] = {1.0, 2.0}, yin[2] = {3.0, 4.0};
    double xout[2], yout[2];
    astTran2(fs, 2, xin, yin, 1, xout, yout);
    printf("nframe=%d  (%g,%g) -> (%g,%g)\n", astGetI(fs, "Nframe"),
           xin[0], yin[0], xout[0], yout[0]);

    AstChannel *ch = astChannel(NULL, NULL, "SinkFile=frame_in_map_slot.ast,Comment=1");
    astWrite(ch, fs);

    /* The control: a CmpFrame whose first component is a Region. A Region is a
       Frame in both libraries, so this needs no Frame-as-Mapping rule. */
    double lbnd[1] = {0.0}, ubnd[1] = {1.0};
    AstFrame *bf = astFrame(1, "Domain=A");
    AstBox *box = astBox(bf, 1, lbnd, ubnd, NULL, " ");
    AstCmpFrame *rcf = astCmpFrame((AstFrame *) box, astFrame(1, "Domain=B"), " ");
    printf("region-in-cmpframe naxes=%d\n", astGetI(rcf, "Naxes"));

    AstChannel *rch = astChannel(NULL, NULL, "SinkFile=region_in_cmpframe.ast,Comment=1");
    astWrite(rch, rcf);

    if (!astOK) {
        fprintf(stderr, "AST reported an error, status %d\n", astStatus);
        astEnd;
        return 1;
    }
    astEnd;
    return 0;
}
