/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* grf_log_probe -- drive C's grf_log.c (the logging and SVG graphics module)
 * through a fixed sequence of calls covering every function of grf.h and
 * every attribute, and capture what each call returns, the call log and the
 * SVG it writes.
 *
 * Prints Rust source for crates/starlink-ast/tests/grf_log_oracle/cases.rs:
 *
 *   ./grf_log_probe > ../../crates/starlink-ast/tests/grf_log_oracle/cases.rs
 *
 * Build from reference/starlink-ast/build (configured with
 * -DCMAKE_C_FLAGS=-ffp-contract=off), compiling grf_log.c in. grf_log.c needs
 * the generated headers in build/src and HAVE_CONFIG_H, and its error path
 * calls astPutErr, which the ast_err library supplies and must be linked
 * whole:
 *
 *   cc -ffp-contract=off -O3 -DHAVE_CONFIG_H -I. -Isrc -I../src -I.. \
 *      ../../../tools/oracle_fixtures/grf_log_probe.c ../src/grf_log.c \
 *      -L. -last -Wl,-force_load,libast_err.a -lm -Wl,-rpath,$PWD \
 *      -o ../../../tools/oracle_fixtures/grf_log_probe
 *
 * On Linux, link the error library with
 * -Wl,--whole-archive -last_err -Wl,--no-whole-archive in place of
 * -Wl,-force_load,libast_err.a.
 */
#include <float.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"
#include "grf.h"
#include "grf_log.h"

enum kind { SCALES, QCH, TXEXT, TEXT, LINE, MARK, ATTR, CAP, FLUSH, BBUF, EBUF };

struct call {
   enum kind kind;
   const char *text;     /* TXEXT, TEXT */
   float x, y;           /* TXEXT, TEXT */
   const char *just;     /* TXEXT, TEXT */
   float upx, upy;       /* TXEXT, TEXT; or, with angle set, computed */
   double angle;         /* TEXT: when not 999, the up-vector is made from it */
   int n;                /* LINE, MARK */
   float px[ 5 ], py[ 5 ];
   int ival;             /* MARK type, ATTR attribute, CAP capability */
   double value;         /* ATTR value (AST__BAD queries) */
   int ival2;            /* ATTR primitive, CAP value */
};

#define NOANG 999.0
#define TX( t, x, y, j, ux, uy ) { TXEXT, t, x, y, j, ux, uy, NOANG, 0, {0}, {0}, 0, 0.0, 0 }
#define TA( t, x, y, j, a ) { TEXT, t, x, y, j, 0.0f, 1.0f, a, 0, {0}, {0}, 0, 0.0, 0 }
#define TU( t, x, y, j, ux, uy ) { TEXT, t, x, y, j, ux, uy, NOANG, 0, {0}, {0}, 0, 0.0, 0 }
#define AT( a, v, p ) { ATTR, NULL, 0, 0, NULL, 0, 0, NOANG, 0, {0}, {0}, a, v, p }
#define SIMPLE( k ) { k, NULL, 0, 0, NULL, 0, 0, NOANG, 0, {0}, {0}, 0, 0.0, 0 }

static const struct call calls[] = {
   SIMPLE( SCALES ),
   SIMPLE( QCH ),
   AT( GRF__SIZE, 2.0, GRF__TEXT ),
   SIMPLE( QCH ),
   AT( GRF__SIZE, 0.0, GRF__TEXT ),
   SIMPLE( QCH ),
   AT( GRF__SIZE, -1.0, GRF__TEXT ),
   SIMPLE( QCH ),
   AT( GRF__SIZE, 1.0, GRF__TEXT ),
   /* Every justification, then up-vectors, then the special texts. */
   TX( "Label", 0.3f, 0.4f, "TL", 0.0f, 1.0f ),
   TX( "Label", 0.3f, 0.4f, "TC", 0.0f, 1.0f ),
   TX( "Label", 0.3f, 0.4f, "TR", 0.0f, 1.0f ),
   TX( "Label", 0.3f, 0.4f, "CL", 0.0f, 1.0f ),
   TX( "Label", 0.3f, 0.4f, "CC", 0.0f, 1.0f ),
   TX( "Label", 0.3f, 0.4f, "CR", 0.0f, 1.0f ),
   TX( "Label", 0.3f, 0.4f, "BL", 0.0f, 1.0f ),
   TX( "Label", 0.3f, 0.4f, "BC", 0.0f, 1.0f ),
   TX( "Label", 0.3f, 0.4f, "BR", 0.0f, 1.0f ),
   TX( "Label", 0.3f, 0.4f, "ML", 0.0f, 1.0f ),
   TX( "Label", 0.3f, 0.4f, "MC", 0.0f, 1.0f ),
   TX( "Label", 0.3f, 0.4f, "MR", 0.0f, 1.0f ),
   TX( "12h", 0.7f, 0.1f, "BL", 1.0f, 0.0f ),
   TX( "12h", 0.7f, 0.1f, "CR", 0.6f, 0.8f ),
   TX( "12h", 0.7f, 0.1f, "TC", -1.0f, 0.0f ),
   TX( "12h", 0.7f, 0.1f, "CC", 0.0f, 0.0f ),   /* zero up-vector: error */
   TX( "", 0.7f, 0.1f, "CC", 0.0f, 1.0f ),
   TX( "\xc2\xb0", 0.5f, 0.5f, "CC", 0.0f, 1.0f ),
   AT( GRF__SIZE, 1.7, GRF__TEXT ),
   TX( "-30\xc2\xb0", 0.123456f, 0.987654f, "BR", 0.25f, 0.75f ),
   AT( GRF__SIZE, 1.0, GRF__TEXT ),
   /* Text: each code, then rotation angles about the rounding rule. */
   TU( "Top left", 0.1f, 0.9f, "TL", 0.0f, 1.0f ),
   TU( "Centre", 0.5f, 0.5f, "CC", 0.0f, 1.0f ),
   TU( "Bottom right", 0.9f, 0.1f, "BR", 0.0f, 1.0f ),
   TU( "Baseline", 0.5f, 0.2f, "MC", 0.0f, 1.0f ),
   TA( "a0", 0.5f, 0.5f, "CC", 0.0 ),
   TA( "a-0.005", 0.5f, 0.5f, "CC", -0.005 ),
   TA( "a-2.5", 0.5f, 0.5f, "CC", -2.5 ),
   TA( "a-7.5", 0.5f, 0.5f, "CC", -7.5 ),
   TA( "a-10", 0.5f, 0.5f, "CC", -10.0 ),
   TA( "a-10.5", 0.5f, 0.5f, "CC", -10.5 ),
   TA( "a-12", 0.5f, 0.5f, "CC", -12.0 ),
   TA( "a92.3", 0.5f, 0.5f, "CC", 92.3 ),
   TA( "a180", 0.5f, 0.5f, "CC", 180.0 ),
   TA( "a3", 0.5f, 0.5f, "CC", 3.0 ),
   TA( "a94", 0.5f, 0.5f, "CC", 94.0 ),
   TA( "a-94", 0.5f, 0.5f, "CC", -94.0 ),
   TU( "", 0.5f, 0.5f, "CC", 0.0f, 1.0f ),
   TU( "\xc2\xb0", 0.25f, 0.75f, "BL", 0.0f, 1.0f ),
   /* Lines: none, one point, two, a closed polygon, an open polyline. */
   { LINE, NULL, 0, 0, NULL, 0, 0, NOANG, 0, {0}, {0}, 0, 0.0, 0 },
   { LINE, NULL, 0, 0, NULL, 0, 0, NOANG, 1, {0.1f}, {0.2f}, 0, 0.0, 0 },
   { LINE, NULL, 0, 0, NULL, 0, 0, NOANG, 2, {0.1f, 0.9f}, {0.2f, 0.8f}, 0, 0.0, 0 },
   { LINE, NULL, 0, 0, NULL, 0, 0, NOANG, 4, {0.1f, 0.9f, 0.5f, 0.1f},
     {0.1f, 0.1f, 0.9f, 0.1f}, 0, 0.0, 0 },
   { LINE, NULL, 0, 0, NULL, 0, 0, NOANG, 5, {0.0f, 0.25f, 0.5f, 0.75f, 1.0f},
     {0.0f, 0.5f, 0.25f, 0.75f, 1.0f}, 0, 0.0, 0 },
   { LINE, NULL, 0, 0, NULL, 0, 0, NOANG, 2, {0.0013889f, 0.9986111f},
     {0.0009259f, 0.4990741f}, 0, 0.0, 0 },
   /* Marks. */
   { MARK, NULL, 0, 0, NULL, 0, 0, NOANG, 0, {0}, {0}, 3, 0.0, 0 },
   { MARK, NULL, 0, 0, NULL, 0, 0, NOANG, 3, {0.2f, 0.4f, 0.6f},
     {0.3f, 0.5f, 0.7f}, 2, 0.0, 0 },
   /* Attributes: set and query each, on each primitive. */
   AT( GRF__STYLE, 2.0, GRF__LINE ),
   AT( GRF__STYLE, AST__BAD, GRF__LINE ),
   AT( GRF__WIDTH, 3.4, GRF__LINE ),
   AT( GRF__WIDTH, AST__BAD, GRF__TEXT ),
   AT( GRF__FONT, 2.0, GRF__TEXT ),
   AT( GRF__FONT, AST__BAD, GRF__TEXT ),
   AT( GRF__COLOUR, 3.0, GRF__LINE ),
   AT( GRF__COLOUR, 15.0, GRF__TEXT ),
   AT( GRF__COLOUR, 2.6, GRF__MARK ),
   AT( GRF__COLOUR, AST__BAD, GRF__MARK ),
   AT( GRF__SIZE, 1.25, GRF__MARK ),
   { LINE, NULL, 0, 0, NULL, 0, 0, NOANG, 2, {0.3f, 0.7f}, {0.3f, 0.3f}, 0, 0.0, 0 },
   TU( "Coloured", 0.5f, 0.6f, "CC", 0.0f, 1.0f ),
   { MARK, NULL, 0, 0, NULL, 0, 0, NOANG, 1, {0.5f}, {0.5f}, 1, 0.0, 0 },
   /* Out-of-range values, as SVG draws them. */
   AT( GRF__COLOUR, 16.0, GRF__LINE ),
   AT( GRF__WIDTH, 0.5, GRF__LINE ),
   { LINE, NULL, 0, 0, NULL, 0, 0, NOANG, 2, {0.3f, 0.7f}, {0.4f, 0.4f}, 0, 0.0, 0 },
   AT( GRF__COLOUR, -1.0, GRF__TEXT ),
   AT( GRF__SIZE, 0.0, GRF__TEXT ),
   TU( "Default", 0.5f, 0.7f, "CC", 0.0f, 1.0f ),
   AT( GRF__SIZE, 2.3, GRF__TEXT ),
   TU( "Large", 0.5f, 0.8f, "CC", 0.0f, 1.0f ),
   /* Colour 0, and half-way values, which round away from zero. */
   AT( GRF__COLOUR, 0.0, GRF__LINE ),
   AT( GRF__WIDTH, 2.5, GRF__LINE ),
   { LINE, NULL, 0, 0, NULL, 0, 0, NOANG, 2, {0.3f, 0.7f}, {0.5f, 0.5f}, 0, 0.0, 0 },
   AT( GRF__COLOUR, 2.5, GRF__MARK ),
   { MARK, NULL, 0, 0, NULL, 0, 0, NOANG, 1, {0.25f}, {0.25f}, 4, 0.0, 0 },
   AT( GRF__SIZE, 1.25, GRF__TEXT ),
   /* Text holding characters that XML and the log quote, written raw. */
   TU( "a<b & \"c\"", 0.5f, 0.9f, "CC", 0.0f, 1.0f ),
   TX( "a<b & \"c\"", 0.5f, 0.9f, "CC", 0.0f, 1.0f ),
   /* Capabilities, buffering, flushing. */
   { CAP, NULL, 0, 0, NULL, 0, 0, NOANG, 0, {0}, {0}, GRF__ESC, 0.0, 1 },
   { CAP, NULL, 0, 0, NULL, 0, 0, NOANG, 0, {0}, {0}, GRF__MJUST, 0.0, 0 },
   { CAP, NULL, 0, 0, NULL, 0, 0, NOANG, 0, {0}, {0}, GRF__SCALES, 0.0, 0 },
   SIMPLE( BBUF ),
   SIMPLE( EBUF ),
   SIMPLE( FLUSH ),
};
#define NCALL ( (int)( sizeof( calls ) / sizeof( calls[ 0 ] ) ) )

/* A float as a Rust literal that reads back exactly. */
static void f32lit( float v ) {
   char buf[ 40 ];
   snprintf( buf, sizeof( buf ), "%.9g", (double) v );
   fputs( buf, stdout );
   if( !strpbrk( buf, ".en" ) ) fputs( ".0", stdout );
}

static void f64lit( double v ) {
   char buf[ 40 ];
   snprintf( buf, sizeof( buf ), "%.17g", v );
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

/* A Rust slice literal. */
static void f32arr( int n, const float *v ) {
   int i;
   printf( "&[" );
   for( i = 0; i < n; i++ ) { if( i ) printf( ", " ); f32lit( v[ i ] ); }
   printf( "]" );
}

static const char *attr_name( int a ) {
   static const char *n[] = { "Style", "Width", "Size", "Font", "Colour" };
   return n[ a ];
}
static const char *prim_name( int p ) {
   static const char *n[] = { "Text", "Line", "Mark" };
   return n[ p ];
}
static const char *cap_name( int c ) {
   static const char *n[] = { "Escapes", "MJust", "Scales" };
   return n[ c ];
}

static void dump( FILE *fp ) {
   int c;
   rewind( fp );
   while( ( c = fgetc( fp ) ) != EOF ) putchar( c );
}

int main( void ) {
   int status = 0, i;
   FILE *logf = tmpfile(), *svgf = tmpfile();
   astWatch( &status );
   astGrfLogInit( logf );
   astGrfLogSetSvg( svgf, 720, 540 );

   printf( "// Generated by tools/oracle_fixtures/grf_log_probe.c.\n"
           "// Do not edit.\n\n"
           "// Each value is printed with enough digits to read back exactly.\n"
           "#![allow(clippy::excessive_precision, clippy::approx_constant)]\n\n"
           "/// One call of C's graphics interface, with its arguments; a\n"
           "/// justification is C's two-character code.\n"
           "#[derive(Debug)]\n"
           "pub enum Call {\n"
           "    Scales,\n"
           "    Qch,\n"
           "    TxExt(&'static str, f32, f32, &'static str, [f32; 2]),\n"
           "    Text(&'static str, f32, f32, &'static str, [f32; 2]),\n"
           "    Line(&'static [f32], &'static [f32]),\n"
           "    Mark(&'static [f32], &'static [f32], i32),\n"
           "    Attr(&'static str, Option<f64>, &'static str),\n"
           "    Cap(&'static str, i32),\n"
           "    BBuf,\n"
           "    EBuf,\n"
           "    Flush,\n"
           "}\n\n"
           "/// What a call returned.\n"
           "#[derive(Debug)]\n"
           "pub enum Expected {\n"
           "    Done,\n"
           "    Pair([f32; 2]),\n"
           "    Box(&'static [f32], &'static [f32]),\n"
           "    Previous(f64),\n"
           "    Cap(bool),\n"
           "    Error,\n"
           "}\n\n"
           "pub const CALLS: &[(Call, Expected)] = &[\n" );

   for( i = 0; i < NCALL; i++ ) {
      const struct call *c = calls + i;
      float a, b, xb[ 4 ], yb[ 4 ], upx = c->upx, upy = c->upy;
      double old;
      int r;
      printf( "    (" );
      switch( c->kind ) {
      case SCALES:
         r = astGScales( &a, &b );
         printf( "Call::Scales, Expected::Pair([" ); f32lit( a ); printf( ", " );
         f32lit( b ); printf( "])" );
         break;
      case QCH:
         r = astGQch( &a, &b );
         printf( "Call::Qch, Expected::Pair([" ); f32lit( a ); printf( ", " );
         f32lit( b ); printf( "])" );
         break;
      case TXEXT:
         r = astGTxExt( c->text, c->x, c->y, c->just, upx, upy, xb, yb );
         printf( "Call::TxExt(" ); rstr( c->text ); printf( ", " ); f32lit( c->x );
         printf( ", " ); f32lit( c->y ); printf( ", " ); rstr( c->just );
         printf( ", [" ); f32lit( upx ); printf( ", " ); f32lit( upy ); printf( "]), " );
         if( !astOK ) {
            astClearStatus;
            printf( "Expected::Error" );
         } else {
            printf( "Expected::Box(" ); f32arr( 4, xb ); printf( ", " );
            f32arr( 4, yb ); printf( ")" );
         }
         break;
      case TEXT:
         if( c->angle != NOANG ) {
            double rad = c->angle * acos( -1.0 ) / 180.0;
            upx = (float) -sin( rad );
            upy = (float) cos( rad );
         }
         r = astGText( c->text, c->x, c->y, c->just, upx, upy );
         printf( "Call::Text(" ); rstr( c->text ); printf( ", " ); f32lit( c->x );
         printf( ", " ); f32lit( c->y ); printf( ", " ); rstr( c->just );
         printf( ", [" ); f32lit( upx ); printf( ", " ); f32lit( upy );
         printf( "]), Expected::Done" );
         break;
      case LINE:
         r = astGLine( c->n, c->px, c->py );
         printf( "Call::Line(" ); f32arr( c->n, c->px ); printf( ", " );
         f32arr( c->n, c->py ); printf( "), Expected::Done" );
         break;
      case MARK:
         r = astGMark( c->n, c->px, c->py, c->ival );
         printf( "Call::Mark(" ); f32arr( c->n, c->px ); printf( ", " );
         f32arr( c->n, c->py ); printf( ", %d), Expected::Done", c->ival );
         break;
      case ATTR:
         r = astGAttr( c->ival, c->value, &old, c->ival2 );
         printf( "Call::Attr(\"%s\", ", attr_name( c->ival ) );
         if( c->value == AST__BAD ) printf( "None" );
         else { printf( "Some(" ); f64lit( c->value ); printf( ")" ); }
         printf( ", \"%s\"), Expected::Previous(", prim_name( c->ival2 ) );
         f64lit( old ); printf( ")" );
         break;
      case CAP:
         r = astGCap( c->ival, c->ival2 );
         printf( "Call::Cap(\"%s\", %d), Expected::Cap(%s)", cap_name( c->ival ),
                 c->ival2, r ? "true" : "false" );
         break;
      case BBUF:
         r = astGBBuf();
         printf( "Call::BBuf, Expected::Done" );
         break;
      case EBUF:
         r = astGEBuf();
         printf( "Call::EBuf, Expected::Done" );
         break;
      case FLUSH:
         r = astGFlush();
         printf( "Call::Flush, Expected::Done" );
         break;
      }
      (void) r;
      printf( "),\n" );
   }

   /* astGrfLogClose writes the SVG footer and forgets the files without
      closing them, so they can be read back here. */
   astGrfLogClose();
   printf( "];\n\n/// The call log.\npub const LOG: &str = r##\"" );
   dump( logf );
   printf( "\"##;\n\n/// The SVG.\npub const SVG: &str = r##\"" );
   dump( svgf );
   printf( "\"##;\n" );
   return astOK ? 0 : 1;
}
