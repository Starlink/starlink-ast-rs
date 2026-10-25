/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* Write the ast-bench fixtures: one plain Mapping per class, as C's own
   Native dump, and bench.oracle recording C's transform of points sampled
   over each fixture's own domain, in gen_transform_oracle's format.

     gen_bench_fixtures <output directory>

   Built by tools/perf/build_c_engine.sh (bench variant). gen_transform_oracle
   samples every Native dump over [-1000, 1000], which suits few of these (a
   TimeMap takes MJDs, a LutMap its table range, a WcsMap small angles), so
   each fixture here carries its own sampling domain. */
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"
#include "transform_oracle.h"

static FILE *sink_fp;
static void sink( const char *line ) { fprintf( sink_fp, "%s\n", line ); }

typedef struct {
   const char *name;
   AstMapping *(* build)( void );
   double lo, hi;
} Fixture;

static AstMapping *zoommap( void ) { return (AstMapping *) astZoomMap( 2, 2.5, " " ); }
static AstMapping *shiftmap( void ) {
   double s[ 2 ] = { 0.25, -0.5 };
   return (AstMapping *) astShiftMap( 2, s, " " );
}
static AstMapping *matrixmap_full( void ) {
   double m[ 9 ] = { 0.866, -0.5, 0.0, 0.5, 0.866, 0.0, 0.1, 0.05, 0.995 };
   return (AstMapping *) astMatrixMap( 3, 3, 0, m, " " );
}
static AstMapping *matrixmap_diag( void ) {
   double m[ 3 ] = { 1.5, 0.75, 1.25 };
   return (AstMapping *) astMatrixMap( 3, 3, 1, m, " " );
}
static AstMapping *winmap( void ) {
   double ina[ 2 ] = { 0.0, 0.0 }, inb[ 2 ] = { 1.0, 1.0 };
   double outa[ 2 ] = { 0.5, -0.25 }, outb[ 2 ] = { 2.0, 0.5 };
   return (AstMapping *) astWinMap( 2, ina, inb, outa, outb, " " );
}
static AstMapping *permmap( void ) {
   int perm[ 2 ] = { 2, 1 };
   return (AstMapping *) astPermMap( 2, perm, 2, perm, NULL, " " );
}
static AstMapping *unitmap( void ) { return (AstMapping *) astUnitMap( 2, " " ); }
static AstMapping *polymap( void ) {
   /* Two outputs, each dominantly linear with corrections up to degree 5;
      columns are coefficient, output, power of x, power of y. */
   double c[] = {
      1.0, 1, 1, 0,   0.05, 1, 0, 1,   0.02, 1, 2, 0,   0.01, 1, 1, 1,
      0.005, 1, 0, 2,   0.003, 1, 3, 0,   0.001, 1, 2, 1,   0.001, 1, 1, 2,
      0.0001, 1, 5, 0,
      1.0, 2, 0, 1,   0.05, 2, 1, 0,   0.02, 2, 0, 2,   0.01, 2, 1, 1,
      0.005, 2, 2, 0,   0.003, 2, 0, 3,   0.001, 2, 1, 2,   0.001, 2, 2, 1,
      0.0001, 2, 0, 5 };
   return (AstMapping *) astPolyMap( 2, 2, 18, c, 0, NULL,
                                     "IterInverse=1, NiterInverse=10, TolInverse=1e-7" );
}
static AstMapping *chebymap( void ) {
   double c[] = {
      1.0, 1, 1, 0,   1.0, 2, 0, 1,   0.05, 1, 0, 1,   0.05, 2, 1, 0,
      0.02, 1, 2, 0,   0.02, 2, 0, 2,   0.01, 1, 1, 1,   0.01, 2, 1, 1,
      0.005, 1, 3, 0,   0.005, 2, 0, 3,   0.003, 1, 0, 3,   0.003, 2, 3, 0 };
   double lbnd[ 2 ] = { -2.0, -2.0 }, ubnd[ 2 ] = { 2.0, 2.0 };
   return (AstMapping *) astChebyMap( 2, 2, 12, c, 0, NULL, lbnd, ubnd,
                                      NULL, NULL, " " );
}
static AstMapping *lutmap( void ) {
   double lut[ 100 ];
   for( int i = 0; i < 100; i++ ) lut[ i ] = i + 0.1*pow( (double) i, 1.3 );
   return (AstMapping *) astLutMap( 100, lut, 0.0, 1.0, "LutInterp=0" );
}
static AstMapping *wcsmap_tan( void ) { return (AstMapping *) astWcsMap( 2, AST__TAN, 1, 2, " " ); }
static AstMapping *wcsmap_sin( void ) { return (AstMapping *) astWcsMap( 2, AST__SIN, 1, 2, " " ); }
static AstMapping *wcsmap_zpn( void ) {
   return (AstMapping *) astWcsMap( 2, AST__ZPN, 1, 2, "PV2_1=1, PV2_3=0.05" );
}
static AstMapping *sphmap( void ) { return (AstMapping *) astSphMap( " " ); }
static AstMapping *pcdmap( void ) {
   double c[ 2 ] = { 0.0, 0.0 };
   return (AstMapping *) astPcdMap( 1.0e-3, c, " " );
}
static AstMapping *grismmap( void ) {
   return (AstMapping *) astGrismMap( "GrismNR=1.5, GrismNRP=0, GrismWaveR=5e-7, "
                                      "GrismAlpha=0, GrismG=1e6, GrismM=1, "
                                      "GrismEps=0, GrismTheta=0" );
}
static AstMapping *unitnormmap( void ) {
   double c[ 2 ] = { 0.5, -0.5 };
   return (AstMapping *) astUnitNormMap( 2, c, " " );
}
static AstMapping *normmap( void ) {
   AstFrame *f = astFrame( 2, " " );
   AstMapping *m = (AstMapping *) astNormMap( f, " " );
   f = astAnnul( f );
   return m;
}
static AstMapping *tranmap( void ) {
   double s[ 2 ] = { 0.5, 0.5 };
   AstMapping *z = (AstMapping *) astZoomMap( 2, 2.0, " " );
   AstMapping *sh = (AstMapping *) astShiftMap( 2, s, " " );
   AstMapping *m = (AstMapping *) astTranMap( z, sh, " " );
   z = astAnnul( z );
   sh = astAnnul( sh );
   return m;
}
static AstMapping *ratemap( void ) {
   double c[] = { 1.0, 1, 1, 0,   0.1, 1, 0, 1,   0.05, 1, 2, 0,
                  0.1, 2, 1, 0,   1.0, 2, 0, 1,   0.05, 2, 0, 2 };
   AstMapping *p = (AstMapping *) astPolyMap( 2, 2, 6, c, 0, NULL, " " );
   AstMapping *m = (AstMapping *) astRateMap( p, 1, 1, " " );
   p = astAnnul( p );
   return m;
}
static AstMapping *slamap( void ) {
   AstSlaMap *m = astSlaMap( 0, " " );
   astSlaAdd( m, "EQGAL", 0, NULL );
   return (AstMapping *) m;
}
static AstMapping *timemap( void ) {
   double a[ 2 ] = { 0.0, AST__BAD };
   AstTimeMap *m = astTimeMap( 0, " " );
   astTimeAdd( m, "TAITOUTC", 2, a );
   return (AstMapping *) m;
}
static AstMapping *specmap( void ) {
   AstSpecMap *m = astSpecMap( 1, 0, " " );
   astSpecAdd( m, "WNTOFR", 0, NULL );
   return (AstMapping *) m;
}
static AstMapping *splinemap( void ) {
   /* A clamped cubic (order 4) spline with 5x5 coefficients on [0, 1]. */
   double t[ 9 ] = { 0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0 };
   double cu[ 25 ], cv[ 25 ];
   for( int i = 0; i < 5; i++ ) {
      for( int j = 0; j < 5; j++ ) {
         cu[ j*5 + i ] = i/4.0 + 0.05*j;
         cv[ j*5 + i ] = j/4.0 + 0.05*i;
      }
   }
   return (AstMapping *) astSplineMap( 4, 4, 5, 5, t, t, cu, cv, " " );
}
static AstMapping *mathmap( void ) {
   const char *fwd[ 2 ] = { "u = x + y", "v = x - y" };
   const char *inv[ 2 ] = { "x = 0.5 * (u + v)", "y = 0.5 * (u - v)" };
   return (AstMapping *) astMathMap( 2, 2, 2, fwd, 2, inv, " " );
}
static AstMapping *cmpmap_series( void ) {
   AstMapping *a = zoommap(), *b = shiftmap();
   AstMapping *m = (AstMapping *) astCmpMap( a, b, 1, " " );
   a = astAnnul( a );
   b = astAnnul( b );
   return m;
}
static AstMapping *cmpmap_parallel( void ) {
   double s[ 1 ] = { 0.25 };
   AstMapping *a = (AstMapping *) astZoomMap( 1, 2.5, " " );
   AstMapping *b = (AstMapping *) astShiftMap( 1, s, " " );
   AstMapping *m = (AstMapping *) astCmpMap( a, b, 0, " " );
   a = astAnnul( a );
   b = astAnnul( b );
   return m;
}

static const Fixture FIXTURES[] = {
   { "zoommap_2d.map", zoommap, -1.0, 1.0 },
   { "shiftmap_2d.map", shiftmap, -1.0, 1.0 },
   { "matrixmap_3x3_full.map", matrixmap_full, -1.0, 1.0 },
   { "matrixmap_3x3_diag.map", matrixmap_diag, -1.0, 1.0 },
   { "winmap_2d.map", winmap, -1.0, 1.0 },
   { "permmap_2d_swap.map", permmap, -1.0, 1.0 },
   { "unitmap_2d.map", unitmap, -1.0, 1.0 },
   { "polymap_2d_deg5.map", polymap, -0.5, 0.5 },
   { "chebymap_2d_deg3.map", chebymap, -1.0, 1.0 },
   { "lutmap_100.map", lutmap, 0.0, 99.0 },
   { "wcsmap_tan.map", wcsmap_tan, -0.05, 0.05 },
   { "wcsmap_sin.map", wcsmap_sin, -0.05, 0.05 },
   { "wcsmap_zpn.map", wcsmap_zpn, -0.05, 0.05 },
   { "sphmap.map", sphmap, -1.0, 1.0 },
   { "pcdmap.map", pcdmap, -0.1, 0.1 },
   { "grismmap.map", grismmap, 4.0e-7, 6.0e-7 },
   { "unitnormmap_2d.map", unitnormmap, -1.0, 1.0 },
   { "normmap_2d.map", normmap, -1.0, 1.0 },
   { "tranmap_zoom_shift.map", tranmap, -1.0, 1.0 },
   { "ratemap_polymap.map", ratemap, -0.5, 0.5 },
   { "slamap_eqgal.map", slamap, 0.0, 1.5 },
   { "timemap_taiutc.map", timemap, 50000.0, 50100.0 },
   { "specmap_wn_to_fr.map", specmap, 1.0, 1000.0 },
   { "splinemap_5x5_cubic.map", splinemap, 0.1, 0.9 },
   { "mathmap_2d_linear.map", mathmap, -1.0, 1.0 },
   { "cmpmap_series.map", cmpmap_series, -1.0, 1.0 },
   { "cmpmap_parallel.map", cmpmap_parallel, -1.0, 1.0 },
};

static double **alloc_cols( int n, int np ) {
   double **c = malloc( sizeof( double * )*(size_t) n );
   for( int i = 0; i < n; i++ ) c[ i ] = malloc( sizeof( double )*(size_t) np );
   return c;
}
static void free_cols( double **c, int n ) {
   if( !c ) return;
   for( int i = 0; i < n; i++ ) free( c[ i ] );
   free( c );
}

/* As gen_transform_oracle.c's transform_write: transform, then write the
   section in the same column format. Aborts on an AST error. */
static void transform_write( FILE *fp, const char *name, AstMapping *map,
                             int forward, int in_n, int out_n, int np,
                             double **in, double **out ) {
   enum { COLW = 24 };
   char buf[ 64 ];
   astTranP( map, np, in_n, (const double **) in, forward, out_n, out );
   if( !astOK ) {
      fprintf( stderr, "%s dir=%s: transform raised an error\n", name,
               forward ? "forward" : "inverse" );
      exit( 1 );
   }
   /* Sections are separated by a blank line, and the file ends with the
      last row. */
   static int first = 1;
   if( !first ) fprintf( fp, "\n" );
   first = 0;
   fprintf( fp, "[%s  nin=%d nout=%d dir=%s]\n", name, in_n, out_n,
            forward ? "forward" : "inverse" );
   for( int p = 0; p < np; p++ ) {
      for( int a = 0; a < in_n; a++ ) {
         oracle_format_double( buf, sizeof buf, in[ a ][ p ] );
         fprintf( fp, "%s%*s", a ? " " : "  ", COLW, buf );
      }
      fprintf( fp, "  " );
      for( int a = 0; a < out_n; a++ ) {
         oracle_format_double( buf, sizeof buf, out[ a ][ p ] );
         fprintf( fp, "%s%*s", a ? " " : "", COLW, buf );
      }
      fprintf( fp, "\n" );
   }
}

int main( int argc, char **argv ) {
   char path[ 4096 ];
   int nsec = 0;
   if( argc != 2 ) {
      fprintf( stderr, "usage: %s <output directory>\n", argv[ 0 ] );
      return 2;
   }
   snprintf( path, sizeof path, "%s/bench.oracle", argv[ 1 ] );
   FILE *ofp = fopen( path, "w" );
   if( !ofp ) { perror( path ); return 1; }
   fprintf( ofp, "# bench oracle - regenerate with tools/perf/gen_bench_fixtures\n" );
   fprintf( ofp, "# tol: rtol=1e-07 atol=1e-07  equiv_rtol=1e-09 equiv_atol=1e-09  "
                 "rtrip_rtol=1e-06 rtrip_atol=0.05\n\n" );

   int np = oracle_sample_axis_count();
   for( size_t f = 0; f < sizeof FIXTURES/sizeof FIXTURES[ 0 ]; f++ ) {
      const Fixture *fx = FIXTURES + f;
      astBegin;
      AstMapping *map = fx->build();
      if( !astOK || !map ) {
         fprintf( stderr, "%s: construction failed\n", fx->name );
         return 1;
      }

      snprintf( path, sizeof path, "%s/%s", argv[ 1 ], fx->name );
      sink_fp = fopen( path, "w" );
      if( !sink_fp ) { perror( path ); return 1; }
      AstChannel *ch = astChannel( NULL, sink, " " );
      if( astWrite( ch, map ) != 1 || !astOK ) {
         fprintf( stderr, "%s: write failed\n", fx->name );
         return 1;
      }
      ch = astAnnul( ch );
      fclose( sink_fp );

      int nin = astGetI( map, "Nin" ), nout = astGetI( map, "Nout" );
      double *lo = malloc( sizeof( double )*(size_t) nin );
      double *hi = malloc( sizeof( double )*(size_t) nin );
      for( int a = 0; a < nin; a++ ) { lo[ a ] = fx->lo; hi[ a ] = fx->hi; }
      double **in = alloc_cols( nin, np ), **out = alloc_cols( nout, np );
      oracle_sample_points( nin, lo, hi, np, in );
      transform_write( ofp, fx->name, map, 1, nin, nout, np, in, out );
      nsec++;
      if( astGetI( map, "TranInverse" ) ) {
         double **back = alloc_cols( nin, np );
         transform_write( ofp, fx->name, map, 0, nout, nin, np, out, back );
         free_cols( back, nin );
         nsec++;
      }
      free_cols( in, nin );
      free_cols( out, nout );
      free( lo );
      free( hi );
      astEnd;
   }
   fclose( ofp );
   printf( "wrote %d sections\n", nsec );
   return 0;
}
