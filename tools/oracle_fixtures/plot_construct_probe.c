/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* plot_construct_probe -- build Plots as astPlot builds them (plot.c
 * astInitPlot_, Fset2D) from assorted Frames and FrameSets and graphics
 * boxes, and capture the resulting FrameSet structure, the graphics bounds
 * and reversal flags, the graphics Frame's Directions, and points
 * transformed through the Plot (plot.c Transform, which normalises forward
 * results in the current Frame).
 *
 * Prints Rust source for crates/starlink-ast/tests/plot_construct_oracle/cases.rs:
 *
 *   ./plot_construct_probe > ../../crates/starlink-ast/tests/plot_construct_oracle/cases.rs
 *
 * Build from reference/starlink-ast/build (configured with
 * -DCMAKE_C_FLAGS=-ffp-contract=off). The car1 case reads
 * ast_tester/fixtures/wcsconv/inputs/car1.head relative to the build
 * directory, so run it from there:
 *
 *   cc -ffp-contract=off -O3 -I. ../../../tools/oracle_fixtures/plot_construct_probe.c \
 *      -L. -last -Wl,-force_load,libast_err.a -lm -Wl,-rpath,$PWD \
 *      -o ../../../tools/oracle_fixtures/plot_construct_probe
 *   ../../../tools/oracle_fixtures/plot_construct_probe > ...
 *
 * On Linux, link the error library with
 * -Wl,--whole-archive -last_err -Wl,--no-whole-archive.
 */
#include <float.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"

static void flit( double value ) {
   char buf[ 40 ];
   if( value == AST__BAD ) { fputs( "BAD", stdout ); return; }
   snprintf( buf, sizeof( buf ), "%.17g", value );
   fputs( buf, stdout );
   if( !strpbrk( buf, ".en" ) ) fputs( ".0", stdout );
}

static void rstr( const char *s ) {
   putchar( '"' );
   for( ; *s; s++ ) {
      if( *s == '"' || *s == '\\' ) putchar( '\\' );
      putchar( *s );
   }
   putchar( '"' );
}

/* A FrameSet read from a FITS header file. */
static AstFrameSet *read_head( const char *path ) {
   char line[ 200 ];
   FILE *fp = fopen( path, "r" );
   AstFitsChan *fc = astFitsChan( NULL, NULL, " " );
   AstFrameSet *fs;
   if( !fp ) { fprintf( stderr, "cannot open %s\n", path ); exit( 1 ); }
   while( fgets( line, sizeof( line ), fp ) ) {
      line[ strcspn( line, "\n" ) ] = 0;
      astPutFits( fc, line, 0 );
   }
   fclose( fp );
   astClear( fc, "Card" );
   fs = astRead( fc );
   fc = astAnnul( fc );
   return fs;
}

/* The Frame (or FrameSet) a case id names, or NULL for the default. */
static AstObject *make_frame( const char *id ) {
   if( !strcmp( id, "null" ) ) return NULL;
   if( !strcmp( id, "basic2" ) ) return (AstObject *) astFrame( 2, " " );
   if( !strcmp( id, "basic2_perm" ) ) {
      int perm[ 2 ] = { 2, 1 };
      AstFrame *f = astFrame( 2, "Domain=PIX" );
      astPermAxes( f, perm );
      return (AstObject *) f;
   }
   if( !strcmp( id, "basic3" ) ) return (AstObject *) astFrame( 3, "Domain=CUBE" );
   if( !strcmp( id, "sky" ) ) return (AstObject *) astSkyFrame( " " );
   if( !strcmp( id, "sky_spec" ) ) {
      AstFrame *s = (AstFrame *) astSkyFrame( " " );
      AstFrame *p = (AstFrame *) astSpecFrame( " " );
      AstObject *c = (AstObject *) astCmpFrame( s, p, " " );
      s = astAnnul( s );
      p = astAnnul( p );
      return c;
   }
   if( !strcmp( id, "car1" ) ) {
      return (AstObject *) read_head( "../ast_tester/fixtures/wcsconv/inputs/car1.head" );
   }
   if( !strcmp( id, "plot_of_sky" ) ) {
      float g[ 4 ] = { 0.1f, 0.1f, 0.9f, 0.9f };
      double b[ 4 ] = { 0.0, 0.0, 1.0, 1.0 };
      AstFrame *s = (AstFrame *) astSkyFrame( " " );
      AstObject *p = (AstObject *) astPlot( s, g, b, " " );
      s = astAnnul( s );
      return p;
   }
   fprintf( stderr, "unknown frame id %s\n", id );
   exit( 1 );
}

struct construct_case { const char *id; float g[ 4 ]; double b[ 4 ]; };
static const struct construct_case cases[] = {
   { "null", { 0.1f, 0.1f, 0.9f, 0.9f }, { 0.0, 0.0, 100.0, 50.0 } },
   { "basic2", { 0.1f, 0.1f, 0.9f, 0.9f }, { 0.0, 0.0, 100.0, 50.0 } },
   { "basic2", { 0.9f, 0.1f, 0.1f, 0.9f }, { 0.0, 0.0, 100.0, 50.0 } },
   { "basic2", { 0.1f, 0.9f, 0.9f, 0.1f }, { 0.0, 0.0, 100.0, 50.0 } },
   { "basic2", { 0.0f, 0.0f, 720.0f, 540.0f }, { -5.0, 2.5, 5.0, 7.5 } },
   { "basic2_perm", { 0.1f, 0.1f, 0.9f, 0.9f }, { 1.0, 1.0, 3.0, 2.0 } },
   { "basic3", { 0.1f, 0.1f, 0.9f, 0.9f }, { 0.0, 0.0, 10.0, 10.0 } },
   { "sky", { 0.1f, 0.1f, 0.9f, 0.9f }, { 0.0, -0.5, 1.0, 0.5 } },
   { "sky_spec", { 0.1f, 0.1f, 0.9f, 0.9f }, { 0.0, -0.5, 1.0, 0.5 } },
   { "car1", { 0.1f, 0.1f, 0.9f, 0.9f }, { 0.0, 0.0, 2962.0, 562.0 } },
   { "plot_of_sky", { 0.2f, 0.2f, 0.8f, 0.8f }, { 0.0, 0.0, 1.0, 1.0 } },
};
#define NCASE ( (int)( sizeof( cases ) / sizeof( cases[ 0 ] ) ) )

/* The value after "<key> = " in a dump, or BAD. */
static double dump_value( const char *dump, const char *key ) {
   char pat[ 40 ];
   const char *p;
   snprintf( pat, sizeof( pat ), " %s = ", key );
   p = strstr( dump, pat );
   return p ? atof( p + strlen( pat ) ) : AST__BAD;
}

int main( void ) {
   int status = 0, i, j;
   double gx[ 5 ] = { 0.1, 0.5, 0.9, 0.3, 0.75 };
   double gy[ 5 ] = { 0.1, 0.5, 0.9, 0.7, 0.2 };
   astWatch( &status );

   printf( "// Generated by tools/oracle_fixtures/plot_construct_probe.c.\n"
           "// Do not edit.\n\n"
           "#![allow(clippy::excessive_precision, clippy::approx_constant)]\n\n"
           "use starlink_ast::constants::BAD_VALUE as BAD;\n\n"
           "/// A Plot built from a Frame id with a graphics box and a plot box.\n"
           "pub struct Construct {\n"
           "    pub id: &'static str,\n"
           "    pub graphbox: [f32; 4],\n"
           "    pub basebox: [f64; 4],\n"
           "    pub nframe: usize,\n"
           "    pub base: usize,\n"
           "    pub current: usize,\n"
           "    pub domains: &'static [&'static str],\n"
           "    /// Xlo, Ylo, Xhi, Yhi.\n"
           "    pub bounds: [f64; 4],\n"
           "    pub reversed: [bool; 2],\n"
           "    /// Direction(1), Direction(2) of the graphics Frame.\n"
           "    pub directions: [bool; 2],\n"
           "    /// Graphics points, as (x, y) pairs.\n"
           "    pub graphics: [[f64; 2]; 5],\n"
           "    /// Those points transformed forward, one row per point.\n"
           "    pub forward: &'static [&'static [f64]],\n"
           "    /// The forward points transformed back.\n"
           "    pub inverse: [[f64; 2]; 5],\n"
           "}\n\n"
           "pub const CONSTRUCT_CASES: &[Construct] = &[\n" );

   for( i = 0; i < NCASE; i++ ) {
      const struct construct_case *c = cases + i;
      AstObject *obj = make_frame( c->id );
      AstPlot *plot = astPlot( obj, c->g, c->b, " " );
      AstFrame *gf;
      char *dump;
      int nf, nout;
      double *in, *out, *back;
      if( !astOK ) { fprintf( stderr, "case %d failed\n", i ); return 1; }
      nf = astGetI( plot, "Nframe" );
      dump = astToString( plot );
      gf = astGetFrame( plot, 1 );
      printf( "    Construct {\n        id: " ); rstr( c->id );
      printf( ",\n        graphbox: [" );
      for( j = 0; j < 4; j++ ) { if( j ) printf( ", " ); flit( (double) c->g[ j ] ); }
      printf( "],\n        basebox: [" );
      for( j = 0; j < 4; j++ ) { if( j ) printf( ", " ); flit( c->b[ j ] ); }
      printf( "],\n        nframe: %d,\n        base: %d,\n        current: %d,\n"
              "        domains: &[", nf, astGetI( plot, "Base" ),
              astGetI( plot, "Current" ) );
      for( j = 1; j <= nf; j++ ) {
         AstFrame *f = astGetFrame( plot, j );
         if( j > 1 ) printf( ", " );
         rstr( astGetC( f, "Domain" ) );
         f = astAnnul( f );
      }
      printf( "],\n        bounds: [" ); flit( dump_value( dump, "Xlo" ) );
      printf( ", " ); flit( dump_value( dump, "Ylo" ) ); printf( ", " );
      flit( dump_value( dump, "Xhi" ) ); printf( ", " ); flit( dump_value( dump, "Yhi" ) );
      printf( "],\n        reversed: [%s, %s],\n",
              dump_value( dump, "Xrev" ) == 1.0 ? "true" : "false",
              dump_value( dump, "Yrev" ) == 1.0 ? "true" : "false" );
      printf( "        directions: [%s, %s],\n",
              astGetI( gf, "Direction(1)" ) ? "true" : "false",
              astGetI( gf, "Direction(2)" ) ? "true" : "false" );

      /* Transform the graphics points forward through the Plot and back. */
      nout = astGetI( plot, "Nout" );
      in = malloc( sizeof( double )*10 );
      out = malloc( sizeof( double )*5*nout );
      back = malloc( sizeof( double )*10 );
      for( j = 0; j < 5; j++ ) { in[ j ] = gx[ j ]; in[ 5 + j ] = gy[ j ]; }
      astTranN( plot, 5, 2, 5, in, 1, nout, 5, out );
      astTranN( plot, 5, nout, 5, out, 0, 2, 5, back );
      printf( "        graphics: [" );
      for( j = 0; j < 5; j++ ) {
         printf( "%s[", j ? ", " : "" ); flit( gx[ j ] ); printf( ", " ); flit( gy[ j ] );
         printf( "]" );
      }
      printf( "],\n        forward: &[" );
      for( j = 0; j < 5; j++ ) {
         int k;
         printf( "%s&[", j ? ", " : "" );
         for( k = 0; k < nout; k++ ) { if( k ) printf( ", " ); flit( out[ k*5 + j ] ); }
         printf( "]" );
      }
      printf( "],\n        inverse: [" );
      for( j = 0; j < 5; j++ ) {
         printf( "%s[", j ? ", " : "" ); flit( back[ j ] ); printf( ", " );
         flit( back[ 5 + j ] ); printf( "]" );
      }
      printf( "],\n    },\n" );
      free( in ); free( out ); free( back );
      dump = astFree( dump );
      gf = astAnnul( gf );
      plot = astAnnul( plot );
      if( obj ) obj = astAnnul( obj );
   }
   printf( "];\n\n/// Constructions C rejects, with the graphics and plot boxes.\n"
           "pub const BAD_BOX_CASES: &[([f32; 4], [f64; 4])] = &[\n" );
   {
      static const float g[][ 4 ] = { { 0.1f, 0.1f, 0.1f, 0.9f },
                                      { 0.1f, 0.5f, 0.9f, 0.5f },
                                      { 0.1f, 0.1f, 0.9f, 0.9f },
                                      { 0.1f, 0.1f, 0.9f, 0.9f } };
      static const double b[][ 4 ] = { { 0.0, 0.0, 1.0, 1.0 },
                                       { 0.0, 0.0, 1.0, 1.0 },
                                       { AST__BAD, 0.0, 1.0, 1.0 },
                                       { 0.0, 0.0, 1.0, AST__BAD } };
      AstFrame *f = astFrame( 2, " " );
      for( i = 0; i < 4; i++ ) {
         AstPlot *plot = astPlot( f, g[ i ], b[ i ], " " );
         if( astOK || status != AST__BADBX ) {
            fprintf( stderr, "bad-box case %d not rejected with AST__BADBX\n", i );
            return 1;
         }
         astClearStatus;
         if( plot ) plot = astAnnul( plot );
         printf( "    ([" );
         for( j = 0; j < 4; j++ ) { if( j ) printf( ", " ); flit( (double) g[ i ][ j ] ); }
         printf( "], [" );
         for( j = 0; j < 4; j++ ) { if( j ) printf( ", " ); flit( b[ i ][ j ] ); }
         printf( "]),\n" );
      }
      f = astAnnul( f );
   }
   printf( "];\n" );
   return astOK ? 0 : 1;
}
