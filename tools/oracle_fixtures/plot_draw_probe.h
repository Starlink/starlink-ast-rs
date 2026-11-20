/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* Shared by the Plot drawing probes (plot_curve_probe.c and the others that
 * include this file): building Plots from FITS headers as
 * ast_tester/testgrid.c builds them, capturing what C's grf_log.c writes
 * while a Plot draws, and printing it as Rust source.
 *
 * The probes link src/grf_log.c in, as tools/oracle_fixtures/grf_log_probe.c
 * does, and read headers relative to reference/starlink-ast/build, so run
 * them from there.
 */
#ifndef PLOT_DRAW_PROBE_H
#define PLOT_DRAW_PROBE_H

#include <float.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"
#include "grf_log.h"

#define HEADS "../ast_tester/fixtures/wcsconv/inputs/"

/* A Rust string literal. */
static void rstr( const char *s ) {
   putchar( '"' );
   for( ; *s; s++ ) {
      if( *s == '"' || *s == '\\' ) putchar( '\\' );
      putchar( *s );
   }
   putchar( '"' );
}

/* A Rust f64 or f32 literal that reads back exactly. */
static void flit( double value ) {
   char buf[ 40 ];
   snprintf( buf, sizeof( buf ), "%.17g", value );
   fputs( buf, stdout );
   if( !strpbrk( buf, ".en" ) ) fputs( ".0", stdout );
}

/* A FrameSet read from a FITS header file, through a FitsChan with the
   attributes "fattr". */
static AstFrameSet *read_head( const char *path, const char *fattr ) {
   char line[ 256 ];
   FILE *fp = fopen( path, "r" );
   AstFitsChan *fc = astFitsChan( NULL, NULL, "%s", fattr );
   AstFrameSet *fs;
   if( !fp ) { fprintf( stderr, "cannot open %s\n", path ); exit( 1 ); }
   while( fgets( line, sizeof( line ), fp ) ) {
      size_t len = strlen( line );
      while( len > 0 && ( line[ len - 1 ] == '\n' || line[ len - 1 ] == '\r' ) ) {
         line[ --len ] = 0;
      }
      astPutFits( fc, line, 0 );
   }
   fclose( fp );
   astClear( fc, "Card" );
   fs = astRead( fc );
   fc = astAnnul( fc );
   return fs;
}

/* The plot box testgrid.c uses for a header when none is given: the pixel
   grid from NAXIS1 and NAXIS2, each 100 if absent. */
static void naxis_box( const char *path, double pbox[ 4 ] ) {
   char line[ 256 ];
   int naxis1 = 100, naxis2 = 100;
   FILE *fp = fopen( path, "r" );
   if( !fp ) { fprintf( stderr, "cannot open %s\n", path ); exit( 1 ); }
   while( fgets( line, sizeof( line ), fp ) ) {
      if( !strncmp( line, "NAXIS1  =", 9 ) ) sscanf( line + 10, "%d", &naxis1 );
      if( !strncmp( line, "NAXIS2  =", 9 ) ) sscanf( line + 10, "%d", &naxis2 );
   }
   fclose( fp );
   pbox[ 0 ] = 0.5;
   pbox[ 1 ] = 0.5;
   pbox[ 2 ] = (double) naxis1 + 0.5;
   pbox[ 3 ] = (double) naxis2 + 0.5;
}

/* The graphics box testgrid.c uses for a plot box: 0.1 to 0.9, shrunk on
   one axis to keep the plot box's aspect ratio. */
static void testgrid_gbox( const double pbox[ 4 ], float gbox[ 4 ] ) {
   float delta, asp;
   gbox[ 0 ] = 0.1f;
   gbox[ 1 ] = 0.1f;
   gbox[ 2 ] = 0.9f;
   gbox[ 3 ] = 0.9f;
   asp = (float)( ( pbox[ 3 ] - pbox[ 1 ] ) / ( pbox[ 2 ] - pbox[ 0 ] ) );
   if( asp < 0.05f || asp > 20.0f ) asp = 1.0f;
   if( asp > 1.0f ) {
      delta = 0.5f * ( ( gbox[ 2 ] - gbox[ 0 ] ) - ( gbox[ 3 ] - gbox[ 1 ] ) / asp );
      gbox[ 2 ] -= delta;
      gbox[ 0 ] += delta;
   } else {
      delta = 0.5f * ( ( gbox[ 3 ] - gbox[ 1 ] ) - asp * ( gbox[ 2 ] - gbox[ 0 ] ) );
      gbox[ 3 ] -= delta;
      gbox[ 1 ] += delta;
   }
}

/* A Plot of the header "head" (a file name under HEADS) as testgrid.c
   builds it: "box" is the plot box, or NULL for the NAXIS box; "attr" is
   applied with astSet after the Plot's own "title=A FITS test". */
static AstPlot *testgrid_plot( const char *head, const char *fattr,
                               const double *box, const char *attr ) {
   char path[ 512 ];
   double pbox[ 4 ];
   float gbox[ 4 ];
   AstFrameSet *fs;
   AstPlot *pl;
   snprintf( path, sizeof( path ), "%s%s", HEADS, head );
   fs = read_head( path, fattr );
   if( box ) {
      memcpy( pbox, box, sizeof( pbox ) );
   } else {
      naxis_box( path, pbox );
   }
   testgrid_gbox( pbox, gbox );
   pl = astPlot( fs, gbox, pbox, "title=A FITS test" );
   astSet( pl, "%s", attr );
   fs = astAnnul( fs );
   return pl;
}

/* Capture of one case's log and SVG. */
static FILE *capture_log, *capture_svg;

static void capture_start( void ) {
   capture_log = tmpfile();
   capture_svg = tmpfile();
   astGrfLogInit( capture_log );
   astGrfLogSetSvg( capture_svg, 720, 540 );
}

/* A captured file as a Rust raw string. */
static void raw_file( FILE *fp ) {
   int c;
   fputs( "r##\"", stdout );
   rewind( fp );
   while( ( c = fgetc( fp ) ) != EOF ) putchar( c );
   fputs( "\"##", stdout );
}

/* Ends the capture and prints "log: ..., svg: ...," fields. */
static void capture_end( void ) {
   astGrfLogClose();
   fputs( "        log: ", stdout );
   raw_file( capture_log );
   fputs( ",\n        svg: ", stdout );
   raw_file( capture_svg );
   fputs( ",\n", stdout );
   fclose( capture_log );
   fclose( capture_svg );
}

#endif
