/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* time_map_probe -- capture single TimeMap conversion steps (timemap.c
 * AddArgs and Transform).
 *
 * Each case is a TimeMap holding one step added with astTimeAdd, applied
 * to two points in the forward and the inverse direction with astTran1.
 * No Mapping is simplified, so the values exercise the step arithmetic and
 * the arguments AddArgs derives, and nothing else.
 *
 * Every step is run twice: once with an MJD offset of 55000.5 and points
 * that are small offsets from it, as TimeFrame conversions use them, and
 * once with an MJD offset of zero and absolute points, as TimeFrame uses
 * them to re-express a TimeOrigin in a new TimeScale or System. The
 * second grid is where rounding of a small correction added to a large
 * value shows.
 *
 * Prints Rust source for crates/starlink-ast/tests/time_map_oracle/cases.rs:
 *
 *   ./time_map_probe > ../../crates/starlink-ast/tests/time_map_oracle/cases.rs
 */
#include <stdio.h>
#include <string.h>
#include "ast.h"

/* Observer: west longitude and geodetic latitude in radians, altitude in
   metres. */
#define WLON -2.7135
#define LAT 0.3459
#define ALT 2000.0

/* One step: its code, the number of arguments and the arguments with the
   MJD (or system) offsets set for the offset grid. The absolute grid sets
   every offset to zero; "noff" says how many leading arguments are
   offsets. */
typedef struct {
   const char *label;
   const char *code;
   int narg;
   int noff;
   double args[ 5 ];
   double off_pts[ 2 ];
   double abs_pts[ 2 ];
} Step;

static const Step steps[] = {
   { "MJDTOMJD", "MJDTOMJD", 2, 2, { 55000.5, 54000.25 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "MJDTOJD", "MJDTOJD", 2, 2, { 55000.5, 2455000.0 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "JDTOMJD", "JDTOMJD", 2, 2, { 2455000.0, 55000.5 }, { 0.25, 1.75 }, { 2455000.75, 2458000.25 } },
   { "MJDTOBEP", "MJDTOBEP", 2, 2, { 55000.5, 2009.5 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "BEPTOMJD", "BEPTOMJD", 2, 2, { 2009.5, 55000.5 }, { 0.25, 1.75 }, { 2009.5, 2017.75 } },
   { "MJDTOJEP", "MJDTOJEP", 2, 2, { 55000.5, 2009.25 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "JEPTOMJD", "JEPTOMJD", 2, 2, { 2009.25, 55000.5 }, { 0.25, 1.75 }, { 2009.25, 2017.75 } },
   { "TAITOUTC", "TAITOUTC", 2, 1, { 55000.5, AST__BAD }, { 0.25, 1.75 }, { 55000.5, 41000.25 } },
   { "TAITOUTC:dtai", "TAITOUTC", 2, 1, { 55000.5, 35.0 }, { 0.25, 1.75 }, { 55000.5, 41000.25 } },
   { "UTCTOTAI", "UTCTOTAI", 2, 1, { 55000.5, AST__BAD }, { 0.25, 1.75 }, { 55000.5, 41000.25 } },
   { "UTCTOTAI:dtai", "UTCTOTAI", 2, 1, { 55000.5, 35.0 }, { 0.25, 1.75 }, { 55000.5, 41000.25 } },
   { "TAITOTT", "TAITOTT", 1, 1, { 55000.5 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "TTTOTAI", "TTTOTAI", 1, 1, { 55000.5 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "TTTOTDB", "TTTOTDB", 5, 1, { 55000.5, WLON, LAT, ALT, AST__BAD }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "TTTOTDB:dtai", "TTTOTDB", 5, 1, { 55000.5, WLON, LAT, ALT, 35.0 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "TDBTOTT", "TDBTOTT", 5, 1, { 55000.5, WLON, LAT, ALT, AST__BAD }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "TDBTOTT:dtai", "TDBTOTT", 5, 1, { 55000.5, WLON, LAT, ALT, 35.0 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "TTTOTCG", "TTTOTCG", 1, 1, { 55000.5 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "TCGTOTT", "TCGTOTT", 1, 1, { 55000.5 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "TDBTOTCB", "TDBTOTCB", 1, 1, { 55000.5 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "TCBTOTDB", "TCBTOTDB", 1, 1, { 55000.5 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "UTTOGMST", "UTTOGMST", 1, 1, { 55000.5 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "GMSTTOUT", "GMSTTOUT", 1, 1, { 55000.5 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "GMSTTOLMST", "GMSTTOLMST", 3, 1, { 55000.5, WLON, LAT }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "LMSTTOGMST", "LMSTTOGMST", 3, 1, { 55000.5, WLON, LAT }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "LASTTOLMST", "LASTTOLMST", 3, 1, { 55000.5, WLON, LAT }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "LMSTTOLAST", "LMSTTOLAST", 3, 1, { 55000.5, WLON, LAT }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "UTTOUTC", "UTTOUTC", 1, 0, { 0.25 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "UTCTOUT", "UTCTOUT", 1, 0, { 0.25 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "LTTOUTC", "LTTOUTC", 1, 0, { -10.0 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
   { "UTCTOLT", "UTCTOLT", 1, 0, { -10.0 }, { 0.25, 1.75 }, { 55000.5, 58000.25 } },
};
#define NSTEP ( (int) ( sizeof( steps ) / sizeof( steps[ 0 ] ) ) )

static void flit( double value ) {
   char buf[ 40 ];
   if( value == AST__BAD ) { fputs( "BAD", stdout ); return; }
   snprintf( buf, sizeof( buf ), "%.17g", value );
   fputs( buf, stdout );
   if( !strpbrk( buf, ".en" ) ) fputs( ".0", stdout );
}

static void flist( const double *v, int n ) {
   int i;
   printf( "[" );
   for( i = 0; i < n; i++ ) { if( i ) printf( ", " ); flit( v[ i ] ); }
   printf( "]" );
}

static void emit( const char *label, const Step *s, const double *args,
                  const double in[ 2 ] ) {
   double fwd[ 2 ], inv[ 2 ];
   AstTimeMap *map = astTimeMap( 0, "" );
   astTimeAdd( map, s->code, s->narg, args );
   astTran1( map, 2, in, 1, fwd );
   astTran1( map, 2, in, 0, inv );
   map = astAnnul( map );
   if( !astOK ) {
      fprintf( stderr, "time_map_probe: %s failed\n", label );
      astClearStatus;
      return;
   }
   printf( "    (\"%s\", \"%s\", &", label, s->code );
   flist( args, s->narg );
   printf( ", " );
   flist( in, 2 );
   printf( ", " );
   flist( fwd, 2 );
   printf( ", " );
   flist( inv, 2 );
   printf( "),\n" );
}

int main( void ) {
   int status = 0, i, j;
   char label[ 80 ];
   double args[ 5 ];
   astWatch( &status );

   printf( "// Generated by tools/oracle_fixtures/time_map_probe.c against\n"
           "// the C library at u/timj/norm-unit 5a2cac2a, past the submodule pin.\n"
           "// Do not edit.\n\n"
           "// Each value is printed with 17 digits so that it reads back exactly.\n"
           "#![allow(clippy::excessive_precision, clippy::approx_constant)]\n\n"
           "#[allow(unused_imports)]\n"
           "use starlink_ast::constants::BAD_VALUE as BAD;\n\n"
           "/// `(case, step code, astTimeAdd arguments, input points, forward\n"
           "/// result, inverse result)`. The case is `off:<label>` when the\n"
           "/// points are offsets from the MJD offset in the arguments, and\n"
           "/// `abs:<label>` when the offsets are zero and the points absolute.\n"
           "pub type StepCase = (\n"
           "    &'static str,\n"
           "    &'static str,\n"
           "    &'static [f64],\n"
           "    [f64; 2],\n"
           "    [f64; 2],\n"
           "    [f64; 2],\n"
           ");\n\n"
           "pub const CASES: &[StepCase] = &[\n" );

   for( i = 0; i < NSTEP; i++ ) {
      const Step *s = steps + i;
      snprintf( label, sizeof( label ), "off:%s", s->label );
      emit( label, s, s->args, s->off_pts );

      for( j = 0; j < s->narg; j++ ) args[ j ] = ( j < s->noff ) ? 0.0 : s->args[ j ];
      snprintf( label, sizeof( label ), "abs:%s", s->label );
      emit( label, s, args, s->abs_pts );
   }

   printf( "];\n" );
   return 0;
}
