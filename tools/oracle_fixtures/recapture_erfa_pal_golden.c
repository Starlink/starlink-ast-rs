/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/*
 * Recompute the outputs of a PAL/ERFA golden file from the C library.
 *
 *    recapture_erfa_pal_golden <roots.tsv> <pin>  > <new roots.tsv>
 *
 * The golden file (crates/starlink-ast/tests/erfa_pal_golden/roots.tsv) holds
 * one call per line, "ROOT in... = out...", every double as the 16 hex digits
 * of its IEEE 754 bit pattern. This program keeps each line's root and inputs,
 * calls the C routine the root names on those inputs, and writes the line
 * again with the outputs C gives. An output NaN is written as "nan".
 *
 * A "#" line is copied, except that the commit after "reference/starlink-ast
 * at " in the one header line that names it is replaced by <pin>.
 *
 * The layout of each root's inputs and outputs is the one run() reads in
 * crates/starlink-ast/src/pal/golden_tests.rs: the doubles of the arguments in
 * order, an integer as its value, a matrix by rows, a status as its C value.
 * A root this program does not know, a malformed token or a wrong number of
 * inputs is an error naming the line; nothing is copied unchecked.
 *
 * Build it with recapture_erfa_pal_golden.sh, which compiles the reference
 * checkout's palwrap.c beside it. pal.h and erfa.h are the reference's
 * top-level headers, so every palXxx and eraXxx below is the astPalXxx or
 * astEraXxx symbol AST itself calls.
 */

#include <limits.h>
#include <math.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "erfa.h"
#include "pal.h"

/* The longest line, and the most doubles on either side of "=". */
#define MAX_LINE 65536
#define MAX_VALUES 1024

/* The largest matrix order a dmat line may hold. */
#define DMAT_MAX_ORDER 16

/* The header text the pinned commit follows. */
#define PIN_PREFIX "reference/starlink-ast at "

/* The digits of a bit pattern and of a commit hash, in value order. */
#define HEX_DIGITS "0123456789abcdef"

static const char *file_name;
static long line_number;

/* Report an error at the current line and stop. */
static void die( const char *fmt, ... ) {
   va_list args;
   fprintf( stderr, "%s:%ld: ", file_name, line_number );
   va_start( args, fmt );
   vfprintf( stderr, fmt, args );
   va_end( args );
   fputc( '\n', stderr );
   exit( 1 );
}

/* An integer argument: the file holds it as a double with that value. */
static int to_int( double x, const char *what ) {
   if( !( x >= (double) INT_MIN && x <= (double) INT_MAX ) || x != floor( x ) ) {
      die( "%s is not an int: %.17g", what, x );
   }
   return (int) x;
}

/* A 3x3 matrix from nine doubles by rows, and back. */
static void get_m3( const double *x, double m[3][3] ) {
   memcpy( m, x, 9 * sizeof( double ) );
}

static void put_m3( double m[3][3], double *out ) {
   memcpy( out, m, 9 * sizeof( double ) );
}

static void get_v3( const double *x, double v[3] ) {
   memcpy( v, x, 3 * sizeof( double ) );
}

/* The orders deuler is called with; a line holds an index into this list,
   which is EULER_ORDERS in golden_tests.rs. */
static const char *const EULER_ORDERS[] = {
   "", "x", "zyx", "ZXZ", "xyzx", "abc", "z1y", "XYZ", "123", "321", "zy", "Zy3",
   "y?z", "zzz", "z\0y", "zyxz"
};
#define N_EULER_ORDERS ( sizeof( EULER_ORDERS ) / sizeof( EULER_ORDERS[0] ) )

/* Each root: x holds its nx inputs, out receives its outputs, and the number
   of outputs is returned. */

static size_t root_epj( const double *x, size_t nx, double *out ) {
   (void) nx;
   out[0] = palEpj( x[0] );
   return 1;
}

static size_t root_epj2d( const double *x, size_t nx, double *out ) {
   (void) nx;
   out[0] = palEpj2d( x[0] );
   return 1;
}

static size_t root_epb( const double *x, size_t nx, double *out ) {
   (void) nx;
   out[0] = palEpb( x[0] );
   return 1;
}

static size_t root_epb2d( const double *x, size_t nx, double *out ) {
   (void) nx;
   out[0] = palEpb2d( x[0] );
   return 1;
}

static size_t root_dcs2c( const double *x, size_t nx, double *out ) {
   (void) nx;
   palDcs2c( x[0], x[1], out );
   return 3;
}

/* The port's dcc2s makes each negative zero of the vector positive before
   it calls the routine, so the file records C's routine on that vector. */
static size_t root_dcc2s( const double *x, size_t nx, double *out ) {
   double v[3];
   int i;
   (void) nx;
   for( i = 0; i < 3; i++ ) v[i] = ( x[i] == 0.0 ) ? 0.0 : x[i];
   palDcc2s( v, &out[0], &out[1] );
   return 2;
}

static size_t root_dmxv( const double *x, size_t nx, double *out ) {
   double m[3][3], v[3];
   (void) nx;
   get_m3( x, m );
   get_v3( x + 9, v );
   palDmxv( m, v, out );
   return 3;
}

static size_t root_dimxv( const double *x, size_t nx, double *out ) {
   double m[3][3], v[3];
   (void) nx;
   get_m3( x, m );
   get_v3( x + 9, v );
   palDimxv( m, v, out );
   return 3;
}

static size_t root_dmxm( const double *x, size_t nx, double *out ) {
   double a[3][3], b[3][3], c[3][3];
   (void) nx;
   get_m3( x, a );
   get_m3( x + 9, b );
   palDmxm( a, b, c );
   put_m3( c, out );
   return 9;
}

static size_t root_dvxv( const double *x, size_t nx, double *out ) {
   double a[3], b[3];
   (void) nx;
   get_v3( x, a );
   get_v3( x + 3, b );
   palDvxv( a, b, out );
   return 3;
}

static size_t root_dvdv( const double *x, size_t nx, double *out ) {
   double a[3], b[3];
   (void) nx;
   get_v3( x, a );
   get_v3( x + 3, b );
   out[0] = palDvdv( a, b );
   return 1;
}

static size_t root_dvn( const double *x, size_t nx, double *out ) {
   double v[3];
   (void) nx;
   get_v3( x, v );
   palDvn( v, out, &out[3] );
   return 4;
}

/* dmat: n a(n*n) y(n) = ok det a' y', ok being 1 for a non-singular matrix
   and a', y' what palDmat leaves in the matrix and vector. */
static size_t root_dmat( const double *x, size_t nx, double *out ) {
   int iw[DMAT_MAX_ORDER];
   double d, *a, *y;
   size_t nn;
   int n, jf;
   if( nx < 1 ) die( "dmat has no matrix order" );
   n = to_int( x[0], "dmat's matrix order" );
   if( n < 1 || n > DMAT_MAX_ORDER ) {
      die( "dmat's matrix order %d is outside 1 to %d", n, DMAT_MAX_ORDER );
   }
   nn = (size_t) n * (size_t) n;
   if( nx != 1 + nn + (size_t) n ) {
      die( "%zu inputs given to dmat of order %d, which takes %zu", nx, n, 1 + nn + (size_t) n );
   }
   a = out + 2;
   y = a + nn;
   memcpy( a, x + 1, nn * sizeof( double ) );
   memcpy( y, x + 1 + nn, (size_t) n * sizeof( double ) );
   palDmat( n, a, y, &d, &jf, iw );
   out[0] = ( jf == 0 ) ? 1.0 : 0.0;
   out[1] = d;
   return 2 + nn + (size_t) n;
}

static size_t root_galeq( const double *x, size_t nx, double *out ) {
   (void) nx;
   palGaleq( x[0], x[1], &out[0], &out[1] );
   return 2;
}

static size_t root_eqgal( const double *x, size_t nx, double *out ) {
   (void) nx;
   palEqgal( x[0], x[1], &out[0], &out[1] );
   return 2;
}

static size_t root_galsup( const double *x, size_t nx, double *out ) {
   (void) nx;
   palGalsup( x[0], x[1], &out[0], &out[1] );
   return 2;
}

static size_t root_supgal( const double *x, size_t nx, double *out ) {
   (void) nx;
   palSupgal( x[0], x[1], &out[0], &out[1] );
   return 2;
}

/* fk45z and fk54z are ERFA's routines, which AST's SlaMap calls. */
static size_t root_fk45z( const double *x, size_t nx, double *out ) {
   (void) nx;
   eraFk45z( x[0], x[1], x[2], &out[0], &out[1] );
   return 2;
}

static size_t root_fk54z( const double *x, size_t nx, double *out ) {
   (void) nx;
   eraFk54z( x[0], x[1], x[2], &out[0], &out[1], &out[2], &out[3] );
   return 4;
}

static size_t root_fk5hz( const double *x, size_t nx, double *out ) {
   (void) nx;
   palFk5hz( x[0], x[1], x[2], &out[0], &out[1] );
   return 2;
}

static size_t root_hfk5z( const double *x, size_t nx, double *out ) {
   (void) nx;
   palHfk5z( x[0], x[1], x[2], &out[0], &out[1], &out[2], &out[3] );
   return 4;
}

static size_t root_prec( const double *x, size_t nx, double *out ) {
   double m[3][3];
   (void) nx;
   palPrec( x[0], x[1], m );
   put_m3( m, out );
   return 9;
}

static size_t root_prebn( const double *x, size_t nx, double *out ) {
   double m[3][3];
   (void) nx;
   palPrebn( x[0], x[1], m );
   put_m3( m, out );
   return 9;
}

static size_t root_ecmat( const double *x, size_t nx, double *out ) {
   double m[3][3];
   (void) nx;
   palEcmat( x[0], m );
   put_m3( m, out );
   return 9;
}

/* deuler: the index of the order in EULER_ORDERS, then phi theta psi. */
static size_t root_deuler( const double *x, size_t nx, double *out ) {
   double m[3][3];
   int order;
   (void) nx;
   order = to_int( x[0], "deuler's order index" );
   if( order < 0 || (size_t) order >= N_EULER_ORDERS ) {
      die( "deuler's order index %d is outside 0 to %zu", order, N_EULER_ORDERS - 1 );
   }
   palDeuler( EULER_ORDERS[order], x[1], x[2], x[3], m );
   put_m3( m, out );
   return 9;
}

static size_t root_addet( const double *x, size_t nx, double *out ) {
   (void) nx;
   palAddet( x[0], x[1], x[2], &out[0], &out[1] );
   return 2;
}

static size_t root_subet( const double *x, size_t nx, double *out ) {
   (void) nx;
   palSubet( x[0], x[1], x[2], &out[0], &out[1] );
   return 2;
}

static size_t root_mappa( const double *x, size_t nx, double *out ) {
   (void) nx;
   palMappa( x[0], x[1], out );
   return 21;
}

static size_t root_ampqk( const double *x, size_t nx, double *out ) {
   double amprms[21];
   (void) nx;
   memcpy( amprms, x + 2, sizeof( amprms ) );
   palAmpqk( x[0], x[1], amprms, &out[0], &out[1] );
   return 2;
}

static size_t root_mapqkz( const double *x, size_t nx, double *out ) {
   double amprms[21];
   (void) nx;
   memcpy( amprms, x + 2, sizeof( amprms ) );
   palMapqkz( x[0], x[1], amprms, &out[0], &out[1] );
   return 2;
}

static size_t root_drange( const double *x, size_t nx, double *out ) {
   (void) nx;
   out[0] = palDrange( x[0] );
   return 1;
}

static size_t root_dranrm( const double *x, size_t nx, double *out ) {
   (void) nx;
   out[0] = palDranrm( x[0] );
   return 1;
}

static size_t root_dsep( const double *x, size_t nx, double *out ) {
   (void) nx;
   out[0] = palDsep( x[0], x[1], x[2], x[3] );
   return 1;
}

static size_t root_dbear( const double *x, size_t nx, double *out ) {
   (void) nx;
   out[0] = palDbear( x[0], x[1], x[2], x[3] );
   return 1;
}

/* evp: = dvb dpb dvh dph, three doubles each. */
static size_t root_evp( const double *x, size_t nx, double *out ) {
   (void) nx;
   palEvp( x[0], x[1], out, out + 3, out + 6, out + 9 );
   return 12;
}

static size_t root_geoc( const double *x, size_t nx, double *out ) {
   (void) nx;
   palGeoc( x[0], x[1], &out[0], &out[1] );
   return 2;
}

static size_t root_dav2m( const double *x, size_t nx, double *out ) {
   double v[3], m[3][3];
   (void) nx;
   get_v3( x, v );
   palDav2m( v, m );
   put_m3( m, out );
   return 9;
}

static size_t root_dat( const double *x, size_t nx, double *out ) {
   (void) nx;
   out[0] = palDat( x[0] );
   return 1;
}

static size_t root_gmst( const double *x, size_t nx, double *out ) {
   (void) nx;
   out[0] = palGmst( x[0] );
   return 1;
}

static size_t root_eqeqx( const double *x, size_t nx, double *out ) {
   (void) nx;
   out[0] = palEqeqx( x[0] );
   return 1;
}

/* gd2gc and gc2gd are ERFA's routines on the WGS84 ellipsoid: = 1 and the
   outputs, or = 0 when C fails. */
static size_t root_gd2gc( const double *x, size_t nx, double *out ) {
   (void) nx;
   if( eraGd2gc( 1, x[0], x[1], x[2], out + 1 ) != 0 ) {
      out[0] = 0.0;
      return 1;
   }
   out[0] = 1.0;
   return 4;
}

static size_t root_gc2gd( const double *x, size_t nx, double *out ) {
   double xyz[3];
   (void) nx;
   get_v3( x, xyz );
   if( eraGc2gd( 1, xyz, &out[1], &out[2], &out[3] ) != 0 ) {
      out[0] = 0.0;
      return 1;
   }
   out[0] = 1.0;
   return 4;
}

static size_t root_pvobs( const double *x, size_t nx, double *out ) {
   (void) nx;
   palPvobs( x[0], x[1], x[2], out );
   return 6;
}

static size_t root_rvgalc( const double *x, size_t nx, double *out ) {
   (void) nx;
   out[0] = palRvgalc( x[0], x[1] );
   return 1;
}

static size_t root_rvlg( const double *x, size_t nx, double *out ) {
   (void) nx;
   out[0] = palRvlg( x[0], x[1] );
   return 1;
}

static size_t root_rvlsrd( const double *x, size_t nx, double *out ) {
   (void) nx;
   out[0] = palRvlsrd( x[0], x[1] );
   return 1;
}

static size_t root_rvlsrk( const double *x, size_t nx, double *out ) {
   (void) nx;
   out[0] = palRvlsrk( x[0], x[1] );
   return 1;
}

/* djcl: = status [year month day fraction]; C writes the outputs only for
   status 0. */
static size_t root_djcl( const double *x, size_t nx, double *out ) {
   int iy, im, id, j;
   double fd;
   (void) nx;
   palDjcl( x[0], &iy, &im, &id, &fd, &j );
   out[0] = (double) j;
   if( j != 0 ) return 1;
   out[1] = (double) iy;
   out[2] = (double) im;
   out[3] = (double) id;
   out[4] = fd;
   return 5;
}

/* cldj: = status [mjd]; C writes the MJD for status 0 and -3 (a day that is
   not a day of the month). */
static size_t root_cldj( const double *x, size_t nx, double *out ) {
   double djm;
   int iy, im, id, j;
   (void) nx;
   iy = to_int( x[0], "cldj's year" );
   im = to_int( x[1], "cldj's month" );
   id = to_int( x[2], "cldj's day" );
   palCldj( iy, im, id, &djm, &j );
   out[0] = (double) j;
   if( j != 0 && j != -3 ) return 1;
   out[1] = djm;
   return 2;
}

/* dd2tf: = sign (the character's code) hours minutes seconds fraction. */
static size_t root_dd2tf( const double *x, size_t nx, double *out ) {
   int ihmsf[4], ndp, i;
   char sign;
   (void) nx;
   ndp = to_int( x[0], "dd2tf's number of decimal places" );
   palDd2tf( ndp, x[1], &sign, ihmsf );
   out[0] = (double) (unsigned char) sign;
   for( i = 0; i < 4; i++ ) out[1 + i] = (double) ihmsf[i];
   return 5;
}

/* dtf2d: = status (0, or 1, 2, 3 for the field out of range) days. */
static size_t root_dtf2d( const double *x, size_t nx, double *out ) {
   int ihour, imin, j;
   (void) nx;
   ihour = to_int( x[0], "dtf2d's hour" );
   imin = to_int( x[1], "dtf2d's minute" );
   palDtf2d( ihour, imin, x[2], &out[1], &j );
   out[0] = (double) j;
   return 2;
}

/* The roots, in the order of ROOTS in golden_tests.rs. nin is the number of
   inputs a line of the root holds; dmat's depends on its first input. */
#define VARIABLE ( (size_t) -1 )

static const struct root {
   const char *name;
   size_t nin;
   size_t ( *fn )( const double *x, size_t nx, double *out );
} ROOTS[] = {
   { "epj", 1, root_epj },
   { "epj2d", 1, root_epj2d },
   { "epb", 1, root_epb },
   { "epb2d", 1, root_epb2d },
   { "dcs2c", 2, root_dcs2c },
   { "dcc2s", 3, root_dcc2s },
   { "dmxv", 12, root_dmxv },
   { "dimxv", 12, root_dimxv },
   { "dmxm", 18, root_dmxm },
   { "dvxv", 6, root_dvxv },
   { "dvdv", 6, root_dvdv },
   { "dvn", 3, root_dvn },
   { "dmat", VARIABLE, root_dmat },
   { "galeq", 2, root_galeq },
   { "eqgal", 2, root_eqgal },
   { "galsup", 2, root_galsup },
   { "supgal", 2, root_supgal },
   { "fk45z", 3, root_fk45z },
   { "fk54z", 3, root_fk54z },
   { "fk5hz", 3, root_fk5hz },
   { "hfk5z", 3, root_hfk5z },
   { "prec", 2, root_prec },
   { "prebn", 2, root_prebn },
   { "ecmat", 1, root_ecmat },
   { "deuler", 4, root_deuler },
   { "addet", 3, root_addet },
   { "subet", 3, root_subet },
   { "mappa", 2, root_mappa },
   { "ampqk", 23, root_ampqk },
   { "mapqkz", 23, root_mapqkz },
   { "drange", 1, root_drange },
   { "dranrm", 1, root_dranrm },
   { "dsep", 4, root_dsep },
   { "dbear", 4, root_dbear },
   { "evp", 2, root_evp },
   { "geoc", 2, root_geoc },
   { "dav2m", 3, root_dav2m },
   { "dat", 1, root_dat },
   { "gmst", 1, root_gmst },
   { "eqeqx", 1, root_eqeqx },
   { "gd2gc", 3, root_gd2gc },
   { "gc2gd", 3, root_gc2gd },
   { "pvobs", 3, root_pvobs },
   { "rvgalc", 2, root_rvgalc },
   { "rvlg", 2, root_rvlg },
   { "rvlsrd", 2, root_rvlsrd },
   { "rvlsrk", 2, root_rvlsrk },
   { "djcl", 1, root_djcl },
   { "cldj", 3, root_cldj },
   { "dd2tf", 2, root_dd2tf },
   { "dtf2d", 3, root_dtf2d },
};
#define N_ROOTS ( sizeof( ROOTS ) / sizeof( ROOTS[0] ) )

static const struct root *find_root( const char *name ) {
   size_t i;
   for( i = 0; i < N_ROOTS; i++ ) {
      if( strcmp( ROOTS[i].name, name ) == 0 ) return &ROOTS[i];
   }
   return NULL;
}

/* The double whose bit pattern is the 16 lower-case hex digits of tok. */
static double parse_bits( const char *tok ) {
   uint64_t bits = 0;
   double value;
   size_t i;
   if( strlen( tok ) != 16 || strspn( tok, HEX_DIGITS ) != 16 ) {
      die( "\"%s\" is not 16 hex digits", tok );
   }
   for( i = 0; i < 16; i++ ) {
      bits = ( bits << 4 ) | (uint64_t) ( strchr( HEX_DIGITS, tok[i] ) - HEX_DIGITS );
   }
   memcpy( &value, &bits, sizeof( value ) );
   return value;
}

/* Write one output as the file records it. */
static void print_value( double value ) {
   uint64_t bits;
   if( isnan( value ) ) {
      fputs( " nan", stdout );
   } else {
      memcpy( &bits, &value, sizeof( bits ) );
      printf( " %016llx", (unsigned long long) bits );
   }
}

/* Copy a "#" line, replacing the commit after PIN_PREFIX by pin. Returns
   whether the line named a commit. */
static int copy_comment( const char *line, const char *pin ) {
   const char *at = strstr( line, PIN_PREFIX );
   const char *rest;
   if( !at ) {
      puts( line );
      return 0;
   }
   rest = at + strlen( PIN_PREFIX );
   if( strspn( rest, HEX_DIGITS ) == 0 ) die( "no commit after \"%s\"", PIN_PREFIX );
   fwrite( line, 1, (size_t) ( rest - line ), stdout );
   fputs( pin, stdout );
   puts( rest + strspn( rest, HEX_DIGITS ) );
   return 1;
}

/* Recompute one data line, "ROOT in... = out...", its tokens separated by
   single spaces. The line is split in place. */
static void recompute( char *line ) {
   static char *toks[2 * MAX_VALUES + 2];
   static double in[MAX_VALUES], out[MAX_VALUES];
   const struct root *root;
   size_t ntok = 0, eq = 0, nin, nout, i;
   char *tok, *next;

   for( tok = line; tok; tok = next ) {
      next = strchr( tok, ' ' );
      if( next ) *next++ = '\0';
      if( tok[0] == '\0' ) die( "empty token: tokens are separated by one space" );
      if( ntok == sizeof( toks ) / sizeof( toks[0] ) ) die( "too many tokens" );
      if( strcmp( tok, "=" ) == 0 ) {
         if( eq ) die( "more than one \"=\"" );
         eq = ntok;
      }
      toks[ntok++] = tok;
   }
   root = find_root( toks[0] );
   if( !root ) die( "unknown root \"%s\"", toks[0] );
   if( !eq ) die( "no \"=\" separator" );
   if( eq + 1 == ntok ) die( "no outputs after \"=\"" );

   nin = eq - 1;
   if( nin > MAX_VALUES ) die( "more than %d inputs", MAX_VALUES );
   if( root->nin != VARIABLE && nin != root->nin ) {
      die( "%zu inputs given to %s, which takes %zu", nin, root->name, root->nin );
   }
   for( i = 0; i < nin; i++ ) in[i] = parse_bits( toks[1 + i] );

   /* The recorded outputs are replaced, but must be well formed. */
   for( i = eq + 1; i < ntok; i++ ) {
      if( strcmp( toks[i], "nan" ) != 0 ) (void) parse_bits( toks[i] );
   }

   nout = root->fn( in, nin, out );
   for( i = 0; i < eq; i++ ) printf( i ? " %s" : "%s", toks[i] );
   fputs( " =", stdout );
   for( i = 0; i < nout; i++ ) print_value( out[i] );
   putchar( '\n' );
}

int main( int argc, char **argv ) {
   static char line[MAX_LINE];
   const char *pin;
   FILE *fp;
   int pins = 0;

   if( argc != 3 ) {
      fprintf( stderr, "usage: %s <roots.tsv> <pin>\n", argv[0] );
      return 2;
   }
   file_name = argv[1];
   pin = argv[2];
   if( pin[0] == '\0' || pin[strspn( pin, HEX_DIGITS )] != '\0' ) {
      fprintf( stderr, "%s: pin \"%s\" is not a commit hash\n", argv[0], pin );
      return 2;
   }
   fp = fopen( file_name, "r" );
   if( !fp ) {
      perror( file_name );
      return 1;
   }

   while( fgets( line, sizeof( line ), fp ) ) {
      size_t len = strlen( line );
      line_number++;
      if( len > 0 && line[len - 1] == '\n' ) {
         line[--len] = '\0';
      } else if( !feof( fp ) ) {
         die( "line longer than %d bytes", MAX_LINE - 2 );
      }
      if( line[0] == '#' ) {
         pins += copy_comment( line, pin );
      } else if( line[0] == '\0' ) {
         putchar( '\n' );
      } else {
         recompute( line );
      }
   }
   if( ferror( fp ) ) {
      perror( file_name );
      return 1;
   }
   fclose( fp );
   if( pins != 1 ) {
      fprintf( stderr, "%s: %d header lines name the commit after \"%s\", expected 1\n",
               file_name, pins, PIN_PREFIX );
      return 1;
   }
   if( fflush( stdout ) != 0 || ferror( stdout ) ) {
      perror( "stdout" );
      return 1;
   }
   return 0;
}
