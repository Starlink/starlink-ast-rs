/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* fits_plotter_probe -- run the steps of fplottest.f, the program behind the
 * AST FITS plotter CGI page, for each example of web/plotter/examples.txt
 * and a few cases that fail, and write its three outputs as fixtures for
 * crates/ast-plotter-wasm.
 *
 * For each case it writes <dir>/<case>.fits (the header rewritten as
 * FITS-WCS), <dir>/<case>.ast (the FrameSet as Native text) and
 * <dir>/<case>.svg (the annotated grid, through grf_log.c's SVG at 600 by
 * 600 pixels). An output that fails holds a first line ERROR and then the
 * messages the C library reported, one per line, or fplottest's own message
 * when it prints one in place of the output; the outputs are produced
 * in fplottest's order (FITS, AST, grid), each independently, as the page
 * shows them. A case whose header the probe makes rather than takes from
 * the examples also gets <dir>/<case>.head, its header, for the tests to
 * read.
 *
 * fplottest.f: https://starlink.eao.hawaii.edu/cgi-bin/ast/file?file=fplottest.f
 * (fetched 2026-11-22). Its PGPLOT window is PGWNAD( 0.0, 1.0, 0.0, 1.0 ),
 * whose world coordinates PGQWIN reports as 0 to 1 on each axis.
 *
 * The probe defines astPutErr itself to collect messages, so it does not
 * link libast_err. Build and run from reference/starlink-ast/build
 * (configured with -DCMAKE_C_FLAGS=-ffp-contract=off):
 *
 *   cc -ffp-contract=off -O3 -DHAVE_CONFIG_H -I. -Isrc -I../src -I.. \
 *      ../../../tools/oracle_fixtures/fits_plotter_probe.c ../src/grf_log.c \
 *      -L. -last -lm -Wl,-rpath,$PWD \
 *      -o ../../../tools/oracle_fixtures/fits_plotter_probe
 *   ../../../tools/oracle_fixtures/fits_plotter_probe \
 *      ../../../web/plotter/examples.txt \
 *      ../../../crates/ast-plotter-wasm/tests/fixtures
 */
#include <ctype.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"
#include "grf_log.h"

#define SVG_SIZE 600
#define MAXCASE 32

/* The messages reported since the last output was written. */
#define MAXMSG 32
static char msgs[ MAXMSG ][ 1024 ];
static int nmsg = 0;

void astPutErr_( int status_value, const char *message ) {
   (void) status_value;
   if( !strncmp( message, "AST: Error", 10 ) ) return;
   if( nmsg < MAXMSG ) {
      snprintf( msgs[ nmsg ], sizeof( msgs[ nmsg ] ), "%s", message );
      nmsg++;
   }
}

struct pcase {
   char name[ 64 ];
   char settings[ 1024 ];
   char fsettings[ 256 ];
   char version;
   char header[ 16384 ];
};

static struct pcase cases[ MAXCASE ];
static int ncase = 0;

/* Read the examples file: "== NAME", "-- settings: ...", "-- version: ...",
   then header lines up to the next "== " line. */
static void read_examples( const char *path ) {
   char line[ 1024 ];
   FILE *fp = fopen( path, "r" );
   struct pcase *c = NULL;
   if( !fp ) { fprintf( stderr, "cannot open %s\n", path ); exit( 1 ); }
   while( fgets( line, sizeof( line ), fp ) ) {
      size_t len = strlen( line );
      if( len && line[ len - 1 ] == '\n' ) line[ --len ] = '\0';
      if( !strncmp( line, "== ", 3 ) ) {
         int i;
         c = &cases[ ncase++ ];
         memset( c, 0, sizeof( *c ) );
         for( i = 0; line[ 3 + i ] && i < 63; i++ ) {
            c->name[ i ] = tolower( (unsigned char) line[ 3 + i ] );
         }
         c->version = ' ';
      } else if( !c ) {
         continue;
      } else if( !strncmp( line, "-- settings:", 12 ) ) {
         const char *v = line + 12;
         if( *v == ' ' ) v++;
         snprintf( c->settings, sizeof( c->settings ), "%s", v );
      } else if( !strncmp( line, "-- version:", 11 ) ) {
         const char *v = line + 11;
         while( *v == ' ' ) v++;
         c->version = *v ? *v : ' ';
      } else {
         strncat( c->header, line, sizeof( c->header ) - strlen( c->header ) - 2 );
         strcat( c->header, "\n" );
      }
   }
   fclose( fp );
}

/* A case made from an example with other inputs. */
static void derive( const char *from, const char *name, const char *settings,
                    const char *fsettings, char version ) {
   int i;
   for( i = 0; i < ncase; i++ ) {
      if( !strcmp( cases[ i ].name, from ) ) {
         struct pcase *c = &cases[ ncase++ ];
         *c = cases[ i ];
         snprintf( c->name, sizeof( c->name ), "%s", name );
         if( settings ) snprintf( c->settings, sizeof( c->settings ), "%s", settings );
         if( fsettings ) snprintf( c->fsettings, sizeof( c->fsettings ), "%s", fsettings );
         c->version = version;
         return;
      }
   }
   fprintf( stderr, "no example %s\n", from );
   exit( 1 );
}

/* Replace the header line of keyword "key" with "line". */
static void set_card( struct pcase *c, const char *key, const char *line ) {
   char out[ sizeof( c->header ) ] = "";
   const char *p = c->header;
   while( *p ) {
      size_t n = strcspn( p, "\n" );
      size_t klen = strlen( key );
      if( n >= klen && !strncmp( p, key, klen ) && ( p[ klen ] == ' ' || p[ klen ] == '=' ) ) {
         strcat( out, line );
      } else {
         strncat( out, p, n );
      }
      strcat( out, "\n" );
      p += n;
      if( *p == '\n' ) p++;
   }
   strcpy( c->header, out );
}

static FILE *open_out( const char *dir, const char *name, const char *ext );

/* Write a case's header to <dir>/<case>.head. */
static void write_head( const char *dir, const struct pcase *c ) {
   FILE *fp = open_out( dir, c->name, "head" );
   fputs( c->header, fp );
   fclose( fp );
}

static FILE *open_out( const char *dir, const char *name, const char *ext ) {
   char path[ 1024 ];
   FILE *fp;
   snprintf( path, sizeof( path ), "%s/%s.%s", dir, name, ext );
   fp = fopen( path, "w" );
   if( !fp ) { fprintf( stderr, "cannot write %s\n", path ); exit( 1 ); }
   return fp;
}

/* Write an output: the text in "tmp" if all is well, else ERROR and the
   messages, clearing the error. */
static void finish_out( const char *dir, const char *name, const char *ext,
                        FILE *tmp ) {
   FILE *fp = open_out( dir, name, ext );
   if( astOK ) {
      int ch;
      rewind( tmp );
      while( ( ch = fgetc( tmp ) ) != EOF ) fputc( ch, fp );
   } else {
      int i;
      fprintf( fp, "ERROR\n" );
      for( i = 0; i < nmsg; i++ ) fprintf( fp, "%s\n", msgs[ i ] );
      astClearStatus;
   }
   nmsg = 0;
   fclose( fp );
   fclose( tmp );
}

/* Write the same error, or the same message, to all three outputs. */
static void all_out( const char *dir, const char *name, const char *text ) {
   static const char *exts[ 3 ] = { "fits", "ast", "svg" };
   int i, j, ok = astOK;
   for( i = 0; i < 3; i++ ) {
      FILE *fp = open_out( dir, name, exts[ i ] );
      fprintf( fp, "ERROR\n" );
      if( ok ) {
         fprintf( fp, "%s\n", text );
      } else {
         for( j = 0; j < nmsg; j++ ) fprintf( fp, "%s\n", msgs[ j ] );
      }
      fclose( fp );
   }
   if( !ok ) astClearStatus;
   nmsg = 0;
}

/* The Channel sink for the AST output. */
static FILE *ast_out = NULL;
static void sink( const char *line ) {
   fprintf( ast_out, "%s\n", line );
}

/* NAXISn as fplottest reads it: the card's value field read list-directed
   as an integer, 100 if the keyword is absent. */
static int naxis( AstFitsChan *fc, const char *key ) {
   char card[ 81 ];
   int value = 100;
   astClear( fc, "Card" );
   if( astFindFits( fc, key, card, 1 ) ) {
      if( sscanf( card + 10, "%d", &value ) != 1 ) value = 100;
   }
   return value;
}

static void run( const struct pcase *c, const char *dir ) {
   AstFitsChan *fc;
   AstFrameSet *fs;
   AstPlot *pl;
   double pbox[ 4 ];
   float gbox[ 4 ], range, delta, asp;
   char card[ 81 ];
   const char *p;
   FILE *tmp;

   astBegin;
   fc = astFitsChan( NULL, NULL, "%s", c->fsettings );
   for( p = c->header; *p && astOK; ) {
      size_t n = strcspn( p, "\n" );
      char line[ 1024 ];
      if( n >= sizeof( line ) ) n = sizeof( line ) - 1;
      memcpy( line, p, n );
      line[ n ] = '\0';
      astPutFits( fc, line, 0 );
      p += n;
      if( *p == '\n' ) p++;
   }
   pbox[ 0 ] = 0.5;
   pbox[ 1 ] = 0.5;
   pbox[ 2 ] = (double) naxis( fc, "NAXIS1" ) + 0.5;
   pbox[ 3 ] = (double) naxis( fc, "NAXIS2" ) + 0.5;
   astClear( fc, "Card" );
   fs = astRead( fc );
   if( !astOK || !fs ) {
      all_out( dir, c->name, "!!! No object read from file!!!" );
      astEnd;
      return;
   }

   /* FITS: rewind, Encoding FITS-WCS, write, rewind, every card. */
   tmp = tmpfile();
   astClear( fc, "Card" );
   astSetC( fc, "Encoding", "FITS-WCS" );
   if( astOK && astWrite( fc, fs ) == 0 ) {
      if( astOK ) fprintf( tmp, "ERROR\nFailed to create new FITS headers\n" );
   } else if( astOK ) {
      astClear( fc, "Card" );
      while( astFindFits( fc, "%f", card, 1 ) ) fprintf( tmp, "%s\n", card );
   }
   finish_out( dir, c->name, "fits", tmp );

   /* AST: the FrameSet through a Channel. */
   ast_out = tmpfile();
   {
      AstChannel *ch = astChannel( NULL, sink, " " );
      if( astWrite( ch, fs ) == 0 && astOK ) {
         fprintf( ast_out, "ERROR\nFailed to write FrameSet to standard output\n" );
      }
   }
   finish_out( dir, c->name, "ast", ast_out );
   ast_out = NULL;

   /* Grid: the version character, then the plot. */
   tmp = tmpfile();
   if( c->version == '$' ) {
      astSetI( fs, "Current", astGetI( fs, "Base" ) );
   } else if( c->version != ' ' ) {
      int i, found = 0, nframe = astGetI( fs, "Nframe" );
      for( i = 1; i <= nframe && !found; i++ ) {
         AstFrame *frm = astGetFrame( fs, i );
         const char *ident = astGetC( frm, "Ident" );
         if( ident && ident[ 0 ] == c->version && ident[ 1 ] == '\0' ) {
            astSetI( fs, "Current", i );
            found = 1;
         }
         frm = astAnnul( frm );
      }
      if( !found ) {
         FILE *fp = open_out( dir, c->name, "svg" );
         fprintf( fp, "ERROR\nThe supplied FITS headers do not define a coordinate "
                  "system with the requested version character '%c'.\n", c->version );
         fclose( fp );
         fclose( tmp );
         astEnd;
         return;
      }
   }

   gbox[ 0 ] = 0.0f;
   gbox[ 1 ] = 0.0f;
   gbox[ 2 ] = 1.0f;
   gbox[ 3 ] = 1.0f;
   range = gbox[ 2 ] - gbox[ 0 ];
   gbox[ 0 ] = gbox[ 0 ] + 0.05f * range;
   gbox[ 2 ] = gbox[ 2 ] - 0.05f * range;
   range = gbox[ 3 ] - gbox[ 1 ];
   gbox[ 1 ] = gbox[ 1 ] + 0.05f * range;
   gbox[ 3 ] = gbox[ 3 ] - 0.05f * range;
   asp = fabsf( (float)( pbox[ 3 ] - pbox[ 1 ] ) / (float)( pbox[ 2 ] - pbox[ 0 ] ) );
   if( asp < 0.2f || asp > 5.0f ) asp = 1.0f;
   if( asp > 1.0f ) {
      delta = ( asp - 1.0f ) * ( gbox[ 2 ] - gbox[ 0 ] ) / ( 2.0f * asp );
      gbox[ 2 ] = gbox[ 2 ] - delta;
      gbox[ 0 ] = gbox[ 0 ] + delta;
   } else {
      delta = ( 1.0f - asp ) * ( gbox[ 3 ] - gbox[ 1 ] ) / 2.0f;
      gbox[ 3 ] = gbox[ 3 ] - delta;
      gbox[ 1 ] = gbox[ 1 ] + delta;
   }

   astGrfLogInit( NULL );
   astGrfLogSetSvg( tmp, SVG_SIZE, SVG_SIZE );
   pl = astPlot( fs, gbox, pbox, "%s", c->settings );
   if( astOK ) astGrid( pl );
   astGrfLogClose();
   finish_out( dir, c->name, "svg", tmp );
   astEnd;
}

int main( int argc, char **argv ) {
   int status = 0, i;
   if( argc != 3 ) {
      fprintf( stderr, "usage: fits_plotter_probe <examples.txt> <fixtures dir>\n" );
      return 1;
   }
   astWatch( &status );
   read_examples( argv[ 1 ] );

   /* Cases beyond the examples: alternate axes and pixel coordinates, and
      inputs that fail. */
   derive( "solar", "solar_version_a", NULL, NULL, 'A' );
   derive( "solar", "solar_version_pixel", NULL, NULL, '$' );
   derive( "aitoff", "bad_version", NULL, NULL, 'Z' );
   derive( "aitoff", "bad_plot_setting", "Fred=1", NULL, ' ' );
   derive( "aitoff", "bad_fitschan_setting", NULL, "Encoding=nonsense", ' ' );
   {
      struct pcase *c = &cases[ ncase++ ];
      memset( c, 0, sizeof( *c ) );
      snprintf( c->name, sizeof( c->name ), "no_wcs" );
      snprintf( c->header, sizeof( c->header ), "SIMPLE  =                    T\n" );
      c->version = ' ';
   }

   /* Cases whose header is made here, each written to <dir>/<case>.head,
      which the Rust tests read as the case's header. AITOFF with no pixels:
      the plot box has no area, so its aspect ratio is NaN. */
   derive( "aitoff", "zero_naxis", NULL, NULL, ' ' );
   set_card( &cases[ ncase - 1 ], "NAXIS1", "NAXIS1  =                    0" );
   set_card( &cases[ ncase - 1 ], "NAXIS2", "NAXIS2  =                    0" );
   write_head( argv[ 2 ], &cases[ ncase - 1 ] );

   /* A header holding a ZoomMap in the Native encoding: what is read is a
      Mapping, not a coordinate system. */
   {
      struct pcase *c = &cases[ ncase++ ];
      AstFitsChan *fc;
      AstZoomMap *zm;
      char card[ 81 ];
      memset( c, 0, sizeof( *c ) );
      snprintf( c->name, sizeof( c->name ), "native_zoommap" );
      c->version = ' ';
      astBegin;
      zm = astZoomMap( 2, 3.0, " " );
      fc = astFitsChan( NULL, NULL, "Encoding=NATIVE" );
      astWrite( fc, zm );
      astClear( fc, "Card" );
      while( astFindFits( fc, "%f", card, 1 ) ) {
         strcat( c->header, card );
         strcat( c->header, "\n" );
      }
      astEnd;
      write_head( argv[ 2 ], c );
   }

   for( i = 0; i < ncase; i++ ) run( &cases[ i ], argv[ 2 ] );
   return 0;
}
