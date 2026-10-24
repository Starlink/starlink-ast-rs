/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* fitschan_tab_read_probe -- capture how a FitsChan (fitschan.c) reads
 * FITS-WCS headers that describe axes with the "-TAB" algorithm of
 * FITS-WCS paper III: the look-up tables the FrameSet uses, the errors,
 * the tables left in the FitsChan and the cards left after the read.
 *
 * Each case is a script of operations applied to a new, empty FitsChan.
 * Each operation is a string of '|'-separated fields:
 *
 *   C|card                   astPutFits
 *   S|setting                astSet
 *   TC|ext|col|dims|v,v,...  add a double precision column, of the given
 *                            ','-separated shape and holding the values in
 *                            row 1, to the FitsTable named ext, which is
 *                            created if need be; the tables are held by
 *                            the probe until PT or SRC
 *   PT                       astPutTable of each table held
 *   SRC                      register a table source function that stores
 *                            the named table, if held, with astPutTable,
 *                            and records each call
 *   R                        astRead: the class of the Object read, or
 *                            "null", then its dump
 *   GT                       astGetTables: "null", or the number of tables
 *                            and their keys in key order
 *   CARDS                    the cards left, from the first
 *
 * After each operation: "= " and its result, or "! " and the error status,
 * which is then cleared; then any dump lines, each prefixed by "|".
 *
 * Prints Rust source for crates/starlink-ast/tests/fitschan_tab_read_oracle/cases.rs:
 *
 *   ./fitschan_tab_read_probe > ../../crates/starlink-ast/tests/fitschan_tab_read_oracle/cases.rs
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"

static char lines[ 4000 ][ 200 ];
static int nline;
static char out[ 800000 ];

/* The tables held by the probe. */
static AstFitsTable *held[ 10 ];
static char heldname[ 10 ][ 80 ];
static int nheld;

static void sink( const char *line ) {
   strncpy( lines[ nline ], line, 199 );
   lines[ nline++ ][ 199 ] = 0;
}

static void add( const char *fmt, const char *s ) {
   char buf[ 2000 ];
   snprintf( buf, sizeof( buf ), fmt, s );
   strcat( out, buf );
}

static void addi( const char *fmt, long long v ) {
   char buf[ 100 ];
   snprintf( buf, sizeof( buf ), fmt, v );
   strcat( out, buf );
}

/* Split "s" at '|' into at most 6 fields. */
static int split( char *s, char *f[ 6 ] ) {
   int n = 0;
   f[ n++ ] = s;
   while( n < 6 && ( s = strchr( s, '|' ) ) ) {
      *s++ = 0;
      f[ n++ ] = s;
   }
   return n;
}

/* Split a ','-separated list into strings. */
static int list( char *s, char *v[ 100 ] ) {
   int n = 0;
   if( !*s ) return 0;
   v[ n++ ] = s;
   while( n < 100 && ( s = strchr( s, ',' ) ) ) {
      *s++ = 0;
      v[ n++ ] = s;
   }
   return n;
}

static void dump( AstObject *obj ) {
   AstChannel *ch;
   int i;
   nline = 0;
   ch = astChannel( NULL, sink, " " );
   astWrite( ch, obj );
   ch = astAnnul( ch );
   for( i = 0; i < nline; i++ ) add( "|%s\n", lines[ i ] );
}

/* The table source function: store the named table, if held. */
static void tabsource( AstFitsChan *fc, const char *extname, int extver,
                       int extlevel, int *status ) {
   int i;
   char buf[ 200 ];
   snprintf( buf, sizeof( buf ), "~ source %s %d %d\n", extname, extver, extlevel );
   strcat( out, buf );
   for( i = 0; i < nheld; i++ ) {
      if( !strcmp( heldname[ i ], extname ) ) astPutTable( fc, held[ i ], extname );
   }
}

static void run( AstFitsChan *fc, const char *op ) {
   char buf[ 4000 ], *f[ 6 ], *v[ 100 ];
   const char *code;
   int nv, i, ndim, dims[ 10 ];
   double vals[ 100 ];
   AstFitsTable *ft;
   AstKeyMap *km;
   AstObject *obj;

   strcpy( buf, op );
   (void) split( buf, f );
   code = f[ 0 ];

   if( !strcmp( code, "C" ) ) {
      astPutFits( fc, f[ 1 ], 0 );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "S" ) ) {
      astSet( fc, "%s", f[ 1 ] );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "TC" ) ) {
      ft = NULL;
      for( i = 0; i < nheld; i++ ) {
         if( !strcmp( heldname[ i ], f[ 1 ] ) ) ft = held[ i ];
      }
      if( !ft ) {
         ft = astFitsTable( NULL, " " );
         held[ nheld ] = ft;
         strcpy( heldname[ nheld++ ], f[ 1 ] );
      }
      ndim = list( f[ 3 ], v );
      for( i = 0; i < ndim; i++ ) dims[ i ] = atoi( v[ i ] );
      astAddColumn( ft, f[ 2 ], AST__DOUBLETYPE, ndim, dims, " " );
      nv = list( f[ 4 ], v );
      for( i = 0; i < nv; i++ ) vals[ i ] = atof( v[ i ] );
      snprintf( buf, sizeof( buf ), "%s(1)", f[ 2 ] );
      if( nv == 1 && ndim == 0 ) {
         astMapPut0D( ft, buf, vals[ 0 ], NULL );
      } else if( nv > 0 ) {
         astMapPut1D( ft, buf, nv, vals, NULL );
      }
      add( "=%s\n", "" );
   } else if( !strcmp( code, "PT" ) ) {
      for( i = 0; i < nheld; i++ ) astPutTable( fc, held[ i ], heldname[ i ] );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "SRC" ) ) {
      astTableSource( fc, tabsource );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "R" ) ) {
      astClear( fc, "Card" );
      obj = astRead( fc );
      add( "= %s\n", obj ? astGetC( obj, "Class" ) : "null" );
      if( obj ) {
         dump( obj );
         obj = astAnnul( obj );
      }
   } else if( !strcmp( code, "GT" ) ) {
      km = astGetTables( fc );
      if( !km ) {
         add( "= null%s\n", "" );
      } else {
         astSet( km, "SortBy=KeyUp" );
         addi( "= %lld", astMapSize( km ) );
         for( i = 0; i < astMapSize( km ); i++ ) add( " %s", astMapKey( km, i ) );
         add( "%s\n", "" );
         km = astAnnul( km );
      }
   } else if( !strcmp( code, "CARDS" ) ) {
      char card[ 81 ];
      add( "=%s\n", "" );
      astClear( fc, "Card" );
      while( astFindFits( fc, "%f", card, 1 ) ) add( "|%s\n", card );
   }
}

static void str( const char *s ) {
   putchar( '"' );
   for( ; *s; s++ ) {
      if( *s == '"' || *s == '\\' ) putchar( '\\' );
      if( *s == '\n' ) {
         printf( "\\n" );
      } else {
         putchar( *s );
      }
   }
   putchar( '"' );
}

static void emit( const char *name, const char *ops[] ) {
   AstFitsChan *fc;
   int i;
   out[ 0 ] = 0;
   nheld = 0;
   fc = astFitsChan( NULL, NULL, " " );
   for( i = 0; ops[ i ]; i++ ) {
      add( "> %s\n", ops[ i ] );
      run( fc, ops[ i ] );
      if( !astOK ) {
         addi( "! %lld\n", astStatus );
         astClearStatus;
      }
   }
   fc = astAnnul( fc );
   for( i = 0; i < nheld; i++ ) held[ i ] = astAnnul( held[ i ] );
   printf( "    (\"%s\", &[", name );
   for( i = 0; ops[ i ]; i++ ) {
      if( i ) printf( ", " );
      str( ops[ i ] );
   }
   printf( "],\n     " );
   str( out );
   printf( "),\n" );
}

#define CASE(name, ...) { const char *ops[] = { __VA_ARGS__, NULL }; emit( name, ops ); }

/* A one-axis spectral -TAB header. */
#define SPEC1 "C|NAXIS   = 1", "C|NAXIS1  = 5", "C|CTYPE1  = 'WAVE-TAB'", \
   "C|CUNIT1  = 'm'", "C|CRPIX1  = 1", "C|CRVAL1  = 1", "C|CDELT1  = 1", \
   "C|PS1_0   = 'WCS-TAB'", "C|PS1_1   = 'COORDS'"

/* A two-axis celestial -TAB header. */
#define CEL2 "C|NAXIS   = 2", "C|NAXIS1  = 4", "C|NAXIS2  = 3", \
   "C|CTYPE1  = 'RA---TAB'", "C|CTYPE2  = 'DEC--TAB'", "C|CRPIX1  = 1", \
   "C|CRPIX2  = 1", "C|CRVAL1  = 1", "C|CRVAL2  = 1", "C|CDELT1  = 1", \
   "C|CDELT2  = 1", "C|PS1_0   = 'WCS-TAB'", "C|PS1_1   = 'RA'", \
   "C|PS2_0   = 'WCS-TAB'", "C|PS2_1   = 'DEC'"

#define RA_TAB "TC|WCS-TAB|RA|1,4|10,10.5,11,11.5"
#define DEC_TAB "TC|WCS-TAB|DEC|1,3|-30,-29.5,-29"

int main( void ) {
   int status = 0;
   astWatch( &status );

   printf( "// Generated by tools/oracle_fixtures/fitschan_tab_read_probe.c. Do not edit.\n\n" );
   printf( "/// `(name, operations, transcript)`: see the probe for the operations\n"
           "/// and the transcript format.\n" );
   printf( "pub type Case = (&'static str, &'static [&'static str], &'static str);\n\n" );
   printf( "pub const CASES: &[Case] = &[\n" );

   CASE( "spectral", SPEC1, "TC|WCS-TAB|COORDS|1,5|1e-6,2e-6,4e-6,5e-6,7e-6",
         "S|TabOK=1", "PT", "R", "GT", "CARDS" );
   CASE( "spectral_tabok_zero", SPEC1, "TC|WCS-TAB|COORDS|1,5|1e-6,2e-6,4e-6,5e-6,7e-6",
         "PT", "R", "GT", "CARDS" );
   CASE( "spectral_index", SPEC1, "C|PS1_2   = 'INDEX'",
         "TC|WCS-TAB|COORDS|1,5|1e-6,2e-6,4e-6,5e-6,7e-6",
         "TC|WCS-TAB|INDEX|5|1,2,4,8,16", "S|TabOK=1", "PT", "R", "GT" );
   CASE( "spectral_nearest", SPEC1, "C|PV1_4   = 1",
         "TC|WCS-TAB|COORDS|1,5|1e-6,2e-6,4e-6,5e-6,7e-6", "S|TabOK=1", "PT", "R" );
   CASE( "spectral_qv_nearest", SPEC1, "C|QV1_4   = 1",
         "TC|WCS-TAB|COORDS|1,5|1e-6,2e-6,4e-6,5e-6,7e-6", "S|TabOK=1", "PT", "R",
         "CARDS" );
   CASE( "spectral_extver", SPEC1, "C|PV1_1   = 3", "C|PV1_2   = 2",
         "TC|WCS-TAB|COORDS|1,5|1e-6,2e-6,4e-6,5e-6,7e-6", "S|TabOK=1", "SRC", "R",
         "GT" );
   CASE( "no_table", SPEC1, "S|TabOK=1", "R", "GT" );
   CASE( "source_without_table", SPEC1, "S|TabOK=1", "SRC", "R" );
   CASE( "no_column", SPEC1, "TC|WCS-TAB|OTHER|1,5|1,2,3,4,5", "S|TabOK=1", "PT", "R",
         "GT" );
   CASE( "one_dimensional_coords", SPEC1, "TC|WCS-TAB|COORDS|5|1,2,3,4,5", "S|TabOK=1",
         "PT", "R" );
   CASE( "bad_first_dimension", SPEC1, "TC|WCS-TAB|COORDS|2,5|1,2,3,4,5,6,7,8,9,10",
         "S|TabOK=1", "PT", "R" );
   CASE( "empty_column", SPEC1, "TC|WCS-TAB|COORDS|1,5|", "S|TabOK=1", "PT", "R" );
   CASE( "index_not_vector", SPEC1, "C|PS1_2   = 'INDEX'",
         "TC|WCS-TAB|COORDS|1,5|1e-6,2e-6,4e-6,5e-6,7e-6",
         "TC|WCS-TAB|INDEX|1,5|1,2,4,8,16", "S|TabOK=1", "PT", "R" );
   CASE( "celestial", CEL2, RA_TAB, DEC_TAB, "S|TabOK=1", "PT", "R", "GT", "CARDS" );
   CASE( "celestial_iwc", CEL2, RA_TAB, DEC_TAB, "S|TabOK=1", "S|Iwc=1", "PT", "R" );
   CASE( "spectral_iwc", SPEC1, "TC|WCS-TAB|COORDS|1,5|1e-6,2e-6,4e-6,5e-6,7e-6",
         "S|TabOK=1", "S|Iwc=1", "PT", "R" );
   CASE( "celestial_axref", CEL2, "C|AXREF1 = 10.25", "C|AXREF2 = -29.75", RA_TAB,
         DEC_TAB, "S|TabOK=1", "PT", "R" );
   CASE( "celestial_one_axis", "C|NAXIS   = 2", "C|NAXIS1  = 4", "C|NAXIS2  = 3",
         "C|CTYPE1  = 'RA---TAB'", "C|CTYPE2  = 'DEC'", "C|CRPIX1  = 1",
         "C|CRPIX2  = 1", "C|CRVAL1  = 1", "C|CRVAL2  = -30", "C|CDELT1  = 1",
         "C|CDELT2  = 1", "C|PS1_0   = 'WCS-TAB'", "C|PS1_1   = 'RA'", RA_TAB,
         "S|TabOK=1", "PT", "R" );
   CASE( "celestial_lone_axis", "C|NAXIS   = 2", "C|NAXIS1  = 4", "C|NAXIS2  = 3",
         "C|CTYPE1  = 'RA---TAB'", "C|CTYPE2  = 'FREQ'", "C|CRPIX1  = 1",
         "C|CRPIX2  = 1", "C|CRVAL1  = 1", "C|CRVAL2  = 1e9", "C|CDELT1  = 1",
         "C|CDELT2  = 1e6", "C|CUNIT2  = 'Hz'", "C|PS1_0   = 'WCS-TAB'",
         "C|PS1_1   = 'RA'", RA_TAB, "S|TabOK=1", "PT", "R" );
   CASE( "plain_one_other_axis", "C|NAXIS   = 2", "C|NAXIS1  = 4", "C|NAXIS2  = 3",
         "C|CTYPE1  = 'PIXEL'", "C|CTYPE2  = 'FREQ'", "C|CRPIX1  = 1",
         "C|CRPIX2  = 1", "C|CRVAL1  = 1", "C|CRVAL2  = 1e9", "C|CDELT1  = 1",
         "C|CDELT2  = 1e6", "C|CUNIT2  = 'Hz'", "R" );
   CASE( "plain_two_other_axes", "C|NAXIS   = 3", "C|NAXIS1  = 4", "C|NAXIS2  = 3", "C|NAXIS3  = 3",
         "C|CTYPE1  = 'PIXEL'", "C|CTYPE2  = 'FREQ'", "C|CTYPE3  = 'PIXEL'", "C|CRPIX1  = 1",
         "C|CRPIX2  = 1", "C|CRVAL1  = 1", "C|CRVAL2  = 1e9", "C|CDELT1  = 1",
         "C|CDELT2  = 1e6", "C|CUNIT2  = 'Hz'", "R" );
   CASE( "mixed", "C|NAXIS   = 3", "C|NAXIS1  = 10", "C|NAXIS2  = 10", "C|NAXIS3  = 5",
         "C|CTYPE1  = 'RA---TAN'", "C|CTYPE2  = 'DEC--TAN'", "C|CTYPE3  = 'FREQ-TAB'",
         "C|CRPIX1  = 5", "C|CRPIX2  = 5", "C|CRPIX3  = 1", "C|CRVAL1  = 10",
         "C|CRVAL2  = -30", "C|CRVAL3  = 1", "C|CDELT1  = -0.001", "C|CDELT2  = 0.001",
         "C|CDELT3  = 1", "C|CUNIT3  = 'Hz'", "C|PS3_0   = 'FREQS'",
         "C|PS3_1   = 'F'", "TC|FREQS|F|1,5|1e9,1.1e9,1.3e9,1.4e9,1.6e9", "S|TabOK=1",
         "PT", "R", "GT", "CARDS" );
   CASE( "alternate", SPEC1, "C|CTYPE1A = 'FREQ-TAB'", "C|CUNIT1A = 'Hz'",
         "C|CRPIX1A = 1", "C|CRVAL1A = 1", "C|CDELT1A = 1", "C|PS1_0A  = 'ALT-TAB'",
         "C|PS1_1A  = 'F'", "TC|WCS-TAB|COORDS|1,5|1e-6,2e-6,4e-6,5e-6,7e-6",
         "TC|ALT-TAB|F|1,5|1e9,2e9,3e9,4e9,5e9", "S|TabOK=1", "PT", "R", "GT" );
   CASE( "shared_table", "C|NAXIS   = 2", "C|NAXIS1  = 5", "C|NAXIS2  = 5",
         "C|CTYPE1  = 'WAVE-TAB'", "C|CTYPE2  = 'FREQ-TAB'", "C|CUNIT1  = 'm'",
         "C|CUNIT2  = 'Hz'", "C|CRPIX1  = 1", "C|CRPIX2  = 1", "C|CRVAL1  = 1",
         "C|CRVAL2  = 1", "C|CDELT1  = 1", "C|CDELT2  = 1", "C|PS1_0   = 'T'",
         "C|PS1_1   = 'W'", "C|PS2_0   = 'T'", "C|PS2_1   = 'F'",
         "TC|T|W|1,5|1e-6,2e-6,4e-6,5e-6,7e-6", "TC|T|F|1,5|1e9,2e9,3e9,4e9,5e9",
         "S|TabOK=1", "PT", "R", "GT" );
   CASE( "two_dimensional_coords", "C|NAXIS   = 2", "C|NAXIS1  = 2", "C|NAXIS2  = 2",
         "C|CTYPE1  = 'WAVE-TAB'", "C|CTYPE2  = 'FREQ-TAB'", "C|CRPIX1  = 1",
         "C|CRPIX2  = 1", "C|CRVAL1  = 1", "C|CRVAL2  = 1", "C|CDELT1  = 1",
         "C|CDELT2  = 1", "C|PS1_0   = 'T'", "C|PS1_1   = 'C'", "C|PS2_0   = 'T'",
         "C|PS2_1   = 'C'", "C|PV2_3   = 2", "TC|T|C|2,2,2|1,2,3,4,5,6,7,8",
         "S|TabOK=1", "PT", "R" );
   CASE( "two_axes_one_dimension", "C|NAXIS   = 2", "C|NAXIS1  = 2", "C|NAXIS2  = 2",
         "C|CTYPE1  = 'WAVE-TAB'", "C|CTYPE2  = 'FREQ-TAB'", "C|CRPIX1  = 1",
         "C|CRPIX2  = 1", "C|CRVAL1  = 1", "C|CRVAL2  = 1", "C|CDELT1  = 1",
         "C|CDELT2  = 1", "C|PS1_0   = 'T'", "C|PS1_1   = 'C'", "C|PS2_0   = 'T'",
         "C|PS2_1   = 'C'", "TC|T|C|1,2|1,2", "S|TabOK=1", "PT", "R" );
   printf( "];\n" );
   return 0;
}
