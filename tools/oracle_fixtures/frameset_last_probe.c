/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* frameset_last_probe -- AZEL conversions through a FrameSet member.
 * astConvert passes the FrameSet's own member Frames to the per-pair
 * conversion (frameset.c ConvertX), so an AZEL SkyFrame inside a FrameSet
 * keeps the LAST it computes and extrapolates it for Epoch changes under
 * 0.4 day (skyframe.c GetLAST), as a SkyFrame converted directly does.
 * The Epoch of the member is changed through the pointer astGetFrame
 * returns, which is the member itself.
 *
 * Prints one line per step: the step name and the three points
 * transformed, each with 17 digits. The values are pinned in
 * crates/starlink-ast/tests/test_sky_last.rs.
 */
#include <stdio.h>
#include "ast.h"

extern void astSetEpoch_( void *, double, int * );
extern void astSetObsLat_( void *, double, int * );
extern void astSetObsLon_( void *, double, int * );

static int status = 0;
static const double xin[ 3 ] = { 1.0, 4.2, 5.9 };
static const double yin[ 3 ] = { 0.5, -0.9, 1.3 };

static void set_epoch( AstFrame *f, double mjd ) {
   astSetEpoch_( astMakePointer( f ), mjd, &status );
}

static AstSkyFrame *sky( const char *system, double epoch ) {
   AstSkyFrame *f = astSkyFrame( " " );
   astSetC( f, "System", system );
   set_epoch( (AstFrame *) f, epoch );
   astSetObsLat_( astMakePointer( f ), 0.3459, &status );
   astSetObsLon_( astMakePointer( f ), -2.7135, &status );
   return f;
}

static void step( const char *name, AstFrame *from, AstFrame *to ) {
   double xout[ 3 ], yout[ 3 ];
   AstFrameSet *cvt = astConvert( from, to, "" );
   int i;
   astTran2( cvt, 3, xin, yin, 1, xout, yout );
   cvt = astAnnul( cvt );
   printf( "%s", name );
   for( i = 0; i < 3; i++ ) printf( " %.17g %.17g", xout[ i ], yout[ i ] );
   printf( "\n" );
}

int main( void ) {
   const double e0 = 55300.25;
   const double de[ 4 ] = { 0.0, 0.1, 0.2, 0.35 };
   const char *names[ 4 ] = { "e0", "plus_0.1", "plus_0.2", "plus_0.35" };
   AstFrame *member;
   AstFrameSet *fs;
   AstSkyFrame *icrs;
   int i;

   astWatch( &status );
   fs = astFrameSet( sky( "AZEL", e0 ), " " );
   icrs = sky( "ICRS", e0 );
   member = astGetFrame( fs, AST__CURRENT );
   for( i = 0; i < 4; i++ ) {
      set_epoch( member, e0 + de[ i ] );
      set_epoch( (AstFrame *) icrs, e0 + de[ i ] );
      step( names[ i ], (AstFrame *) fs, (AstFrame *) icrs );
   }
   step( "reverse", (AstFrame *) icrs, (AstFrame *) fs );
   return 0;
}
