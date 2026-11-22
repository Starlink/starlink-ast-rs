/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* plot_native_probe -- capture Plots' Native dumps (plot.c Dump and
 * astLoadPlot) and how a clipped Plot transforms (plot.c Trans).
 *
 * DUMP_CASES are Plots the port can build with its public API: each Task 1
 * construction, and Plots with attributes set by an astSet string. Each
 * records the dump a default Channel (Comment true, Full 0) writes.
 *
 * LOAD_CASES are Plots built with calls the port does not yet have: a
 * clipping region set by astClip, tick values set by the protected
 * astSetTickValues, and a Plot after astGrid has drawn it. The port reads
 * each dump, so these record the dump, optionally the dump after a Frame is
 * removed, and where three graphics points go forward through the Plot after
 * any removal, and where they go backwards through the Plot inverted, which
 * makes the clipping Frame the base Frame for clipped Plots of the current
 * Frame.
 *
 * Prints Rust source for crates/starlink-ast/tests/plot_native_oracle/cases.rs:
 *
 *   ./plot_native_probe > ../../crates/starlink-ast/tests/plot_native_oracle/cases.rs
 *
 * Build and run from reference/starlink-ast/build (configured with
 * -DCMAKE_C_FLAGS=-ffp-contract=off). It links grf_log.c, which astGrid draws
 * through, in place of the stub graphics library for the current interface;
 * the car1 Plot reads ../ast_tester/fixtures/wcsconv/inputs/car1.head:
 *
 *   cc -ffp-contract=off -O3 -I. -I../src ../../../tools/oracle_fixtures/plot_native_probe.c \
 *      -L. -last -Wl,-force_load,libast_err.a \
 *      -Wl,-force_load,libast_grf_log.a -Wl,-force_load,libast_grf3d.a \
 *      -last_grf_3.2 -last_grf_2.0 \
 *      -lm -Wl,-rpath,$PWD -o ../../../tools/oracle_fixtures/plot_native_probe
 *
 * The older stub libraries are linked, not forced in: grf_2.0 defines some
 * of grf_log's functions too. On Linux, link the forced ones with
 * -Wl,--whole-archive -last_err -last_grf_log -last_grf3d -Wl,--no-whole-archive.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"
#include "grf_log.h"

/* The protected astSetTickValues (plot.h), called with a real pointer. */
extern void astSetTickValues_( void *, int, int, double *, int, double *, int * );

static char *text;
static size_t ntext;

static void sink( const char *line ) {
   size_t n = strlen( line );
   text = realloc( text, ntext + n + 2 );
   memcpy( text + ntext, line, n );
   ntext += n;
   text[ ntext++ ] = '\n';
   text[ ntext ] = 0;
}

/* The dump a default Channel writes, in a buffer the caller frees. */
static char *dump( AstObject *obj ) {
   AstChannel *ch = astChannel( NULL, sink, " " );
   text = NULL;
   ntext = 0;
   astWrite( ch, obj );
   ch = astAnnul( ch );
   return text;
}

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

/* The constructions of plot_construct_probe.c, in its order. */
struct construct_case { const char *id; float g[ 4 ]; double b[ 4 ]; };
static const struct construct_case constructs[] = {
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
#define NCONSTRUCT ( sizeof( constructs ) / sizeof( constructs[ 0 ] ) )

static AstObject *construct_frame( const char *id ) {
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

/* The Plots of plot_attrs_probe.c's make_plot that these cases use. */
static AstPlot *attrs_plot( const char *id ) {
   float g[ 4 ] = { 0.1f, 0.1f, 0.9f, 0.9f };
   double b[ 4 ] = { 0.0, 0.0, 100.0, 50.0 };
   AstObject *f;
   AstPlot *p;
   if( !strcmp( id, "basic2" ) ) {
      f = (AstObject *) astFrame( 2, " " );
   } else if( !strcmp( id, "basic2_pos" ) ) {
      f = (AstObject *) astFrame( 2, " " );
      b[ 0 ] = 1.0; b[ 1 ] = 2.0;
   } else if( !strcmp( id, "sky" ) ) {
      f = (AstObject *) astSkyFrame( " " );
      b[ 0 ] = 0.0; b[ 1 ] = -0.5; b[ 2 ] = 1.0; b[ 3 ] = 0.5;
   } else if( !strcmp( id, "car1" ) ) {
      f = (AstObject *) read_head( "../ast_tester/fixtures/wcsconv/inputs/car1.head" );
      b[ 0 ] = 0.0; b[ 1 ] = 0.0; b[ 2 ] = 2962.0; b[ 3 ] = 562.0;
   } else {
      fprintf( stderr, "unknown plot id %s\n", id );
      exit( 1 );
   }
   p = astPlot( f, g, b, " " );
   f = astAnnul( f );
   return p;
}

struct set_case { const char *plot; const char *set; };
static const struct set_case set_cases[] = {
   { "basic2", "Tol=0.05,Grid=1,TickAll=0,ForceExterior=1,Invisible=1,Border=0,"
               "ClipOp=1,Clip=2,DrawTitle=0,Escape=0,TitleGap=0.1,"
               "Labelling=interior,TextGapType=plot" },
   { "basic2", "DrawAxes(1)=0,Abbrev(2)=0,LabelAt(1)=10,Centre(2)=3,"
               "NumLabGap(1)=0.02,TextLabGap(2)=0.03,LabelUp(1)=1,LogTicks(2)=1,"
               "LogLabel(1)=1,NumLab(2)=0,MinTick(1)=3,TextLab(1)=0,"
               "LabelUnits(2)=1,MajTickLen(1)=0.02,MinTickLen(2)=0.01,"
               "Edge(1)=top,Edge(2)=right" },
   /* Dump writes Gap and LogGap with the set flag left by the Centre loop,
      that of the last axis's Centre. */
   { "basic2", "Gap(1)=10,LogGap(2)=100" },
   { "basic2", "Centre(1)=3,Gap(1)=10,LogGap(2)=100" },
   { "basic2", "Centre(2)=3,Gap(1)=10,LogGap(2)=100" },
   { "basic2", "Colour(Axes)=3,Width(Grid1)=2.5,Size(Strings)=0.8,"
               "Style(Ticks2)=4,Font(Title)=3,Colour(Axis3)=7,Width(Ticks3)=3" },
   { "basic2_pos", "LogPlot(1)=1" },
   { "basic2_pos", "LogPlot(1)=1,LogPlot(2)=1" },
   { "sky", "LabelUnits(1)=1,Edge(2)=top" },
   { "car1", "Grid=1,Colour(Grid)=2" },
};
#define NSET ( sizeof( set_cases ) / sizeof( set_cases[ 0 ] ) )

static const double gpoints[ 3 ][ 2 ] = { { 0.15, 0.2 }, { 0.5, 0.5 }, { 0.85, 0.8 } };

/* Print a LOAD_CASES entry for a Plot, removing Frame "remove" (if not
   zero) after the first dump. */
static void load_case( const char *what, AstPlot *plot, int remove ) {
   double in[ 2 ][ 3 ], out[ 3 ][ 3 ], back[ 3 ][ 3 ];
   char *before = dump( (AstObject *) plot );
   char *after = NULL;
   int i, k, nout, nback;
   if( remove ) {
      astRemoveFrame( plot, remove );
      after = dump( (AstObject *) plot );
   }
   for( i = 0; i < 3; i++ ) {
      in[ 0 ][ i ] = gpoints[ i ][ 0 ];
      in[ 1 ][ i ] = gpoints[ i ][ 1 ];
   }
   nout = astGetI( plot, "Nout" );
   astTranN( plot, 3, 2, 3, (const double *) in, 1, nout, 3, (double *) out );
   astInvert( plot );
   nback = astGetI( plot, "Nin" );
   astTranN( plot, 3, 2, 3, (const double *) in, 0, nback, 3, (double *) back );
   astInvert( plot );
   if( !astOK ) { fprintf( stderr, "%s failed\n", what ); exit( 1 ); }
   printf( "    LoadCase {\n        what: " );
   rstr( what );
   printf( ",\n        dump: " );
   rstr( before );
   printf( ",\n        remove: %d,\n        removed_dump: ", remove );
   rstr( after ? after : "" );
   printf( ",\n        forward: &[" );
   for( i = 0; i < 3; i++ ) {
      printf( "&[" );
      for( k = 0; k < nout; k++ ) {
         flit( out[ k ][ i ] );
         if( k < nout - 1 ) printf( ", " );
      }
      printf( "]%s", i < 2 ? ", " : "" );
   }
   printf( "],\n        inverted_backward: &[" );
   for( i = 0; i < 3; i++ ) {
      printf( "&[" );
      for( k = 0; k < nback; k++ ) {
         flit( back[ k ][ i ] );
         if( k < nback - 1 ) printf( ", " );
      }
      printf( "]%s", i < 2 ? ", " : "" );
   }
   printf( "],\n    },\n" );
   free( before );
   free( after );
}

int main( void ) {
   size_t i;

   printf( "// Generated by tools/oracle_fixtures/plot_native_probe.c.\n"
           "// Do not edit.\n\n"
           "#![allow(clippy::excessive_precision, clippy::approx_constant)]\n\n"
           "#[allow(unused_imports)]\n"
           "use starlink_ast::constants::BAD_VALUE as BAD;\n\n"
           "/// A Plot the port builds, and the dump C writes of it.\n"
           "pub struct DumpCase {\n"
           "    /// `construct:N` for the Nth construction of `plot_construct_oracle`,\n"
           "    /// otherwise a Plot of the attributes test.\n"
           "    pub plot: &'static str,\n"
           "    /// The astSet string applied to it.\n"
           "    pub set: &'static str,\n"
           "    pub dump: &'static str,\n"
           "}\n\n"
           "/// A Plot the port reads from C's dump of it.\n"
           "pub struct LoadCase {\n"
           "    pub what: &'static str,\n"
           "    pub dump: &'static str,\n"
           "    /// A Frame C then removed, or 0, and the dump after.\n"
           "    pub remove: usize,\n"
           "    pub removed_dump: &'static str,\n"
           "    /// Where (0.15, 0.2), (0.5, 0.5) and (0.85, 0.8) go forward, after\n"
           "    /// any removal.\n"
           "    pub forward: &'static [&'static [f64]],\n"
           "    /// Where the same points go backwards through the Plot inverted,\n"
           "    /// after any removal.\n"
           "    pub inverted_backward: &'static [&'static [f64]],\n"
           "}\n\n" );

   printf( "pub const DUMP_CASES: &[DumpCase] = &[\n" );
   for( i = 0; i < NCONSTRUCT; i++ ) {
      const struct construct_case *c = constructs + i;
      AstObject *obj = construct_frame( c->id );
      AstPlot *plot = astPlot( obj, c->g, c->b, " " );
      char *d = dump( (AstObject *) plot );
      if( obj ) obj = astAnnul( obj );
      if( !astOK ) { fprintf( stderr, "construct %zu failed\n", i ); exit( 1 ); }
      printf( "    DumpCase {\n        plot: \"construct:%zu\",\n        set: \"\",\n"
              "        dump: ", i );
      rstr( d );
      printf( ",\n    },\n" );
      free( d );
      plot = astAnnul( plot );
   }
   for( i = 0; i < NSET; i++ ) {
      AstPlot *plot = attrs_plot( set_cases[ i ].plot );
      char *d;
      astSet( plot, "%s", set_cases[ i ].set );
      d = dump( (AstObject *) plot );
      if( !astOK ) { fprintf( stderr, "set case %zu failed\n", i ); exit( 1 ); }
      printf( "    DumpCase {\n        plot: " );
      rstr( set_cases[ i ].plot );
      printf( ",\n        set: " );
      rstr( set_cases[ i ].set );
      printf( ",\n        dump: " );
      rstr( d );
      printf( ",\n    },\n" );
      free( d );
      plot = astAnnul( plot );
   }
   printf( "];\n\n" );

   printf( "pub const LOAD_CASES: &[LoadCase] = &[\n" );
   {
      double lb[ 2 ] = { 0.1, -0.2 }, ub[ 2 ] = { 0.6, 0.3 };
      AstPlot *plot = attrs_plot( "sky" );
      astClip( plot, AST__CURRENT, lb, ub );
      load_case( "sky clipped in the current Frame", plot, 0 );
      plot = astAnnul( plot );

      plot = attrs_plot( "sky" );
      astSet( plot, "ClipOp=1" );
      astClip( plot, AST__CURRENT, lb, ub );
      load_case( "sky clipped in the current Frame with ClipOp", plot, 0 );
      plot = astAnnul( plot );
   }
   {
      double lb[ 2 ] = { 0.2, 0.2 }, ub[ 2 ] = { 0.7, 0.6 };
      AstPlot *plot = attrs_plot( "basic2" );
      astClip( plot, AST__BASE, lb, ub );
      load_case( "basic2 clipped in the base Frame", plot, 0 );
      plot = astAnnul( plot );
   }
   {
      double lb[ 2 ] = { 3.0, -0.5 }, ub[ 2 ] = { 3.3, 0.0 };
      AstPlot *plot = attrs_plot( "car1" );
      astClip( plot, AST__CURRENT, lb, ub );
      load_case( "car1 clipped in the current Frame", plot, 0 );
      plot = astAnnul( plot );

      plot = attrs_plot( "car1" );
      astClip( plot, AST__CURRENT, lb, ub );
      load_case( "car1 clipped, an earlier Frame removed", plot, 2 );
      plot = astAnnul( plot );

      {
         /* Holds the second point only once its RA is normalised. */
         double lb2[ 2 ] = { 6.0, 1.0 }, ub2[ 2 ] = { 6.3, 1.4 };
         plot = attrs_plot( "car1" );
         astClip( plot, AST__CURRENT, lb2, ub2 );
         load_case( "car1 clipped about RA 6.16", plot, 0 );
      }
      plot = astAnnul( plot );

      plot = attrs_plot( "car1" );
      astClip( plot, 2, lb, ub );
      load_case( "car1 clipped, the clipping Frame removed", plot, 2 );
      plot = astAnnul( plot );
   }
   {
      double maj[ 3 ] = { 30.0, 10.0, 20.0 }, min[ 2 ] = { 25.0, 15.0 };
      double maj2[ 1 ] = { 5.0 };
      AstPlot *plot = attrs_plot( "basic2" );
      astSetTickValues_( astMakePointer( plot ), 0, 3, maj, 2, min, astGetStatusPtr );
      astSetTickValues_( astMakePointer( plot ), 1, 1, maj2, 0, NULL, astGetStatusPtr );
      load_case( "basic2 with tick values", plot, 0 );
      plot = astAnnul( plot );
   }
   {
      FILE *log = fopen( "/dev/null", "w" );
      AstPlot *plot = attrs_plot( "car1" );
      char *before = dump( (AstObject *) plot );
      char *after;
      astGrfLogInit( log );
      astGrid( plot );
      astGrfLogClose();
      fclose( log );
      after = dump( (AstObject *) plot );
      if( !astOK ) { fprintf( stderr, "grid failed\n" ); exit( 1 ); }
      /* Dump writes nothing astGrid records; fail loudly if that changes. */
      if( strcmp( before, after ) ) {
         fprintf( stderr, "astGrid changed the dump\n" );
         exit( 1 );
      }
      load_case( "car1 after astGrid", plot, 0 );
      free( before );
      free( after );
      plot = astAnnul( plot );
   }
   printf( "];\n" );
   return 0;
}
