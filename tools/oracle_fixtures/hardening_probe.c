/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* hardening_probe -- record what the C library reports for every dump the
 * C hardening tests read.
 *
 * The C test named by TEST_SOURCE (a file in ast_tester/) is included whole,
 * with its main renamed, and its every astFromString call goes through
 * probe_from_string, which records the dump, the status left after the call
 * and every message passed to astPutErr during it, but for the first
 * ("AST: Error in routine ...", which names this file). Messages reported
 * after the call belong to the test's later operations and are not kept.
 *
 * Writes the Case entries of one test as Rust source to file descriptor 3;
 * build_hardening_probe.sh builds it once per test and prints
 * crates/starlink-ast/tests/hardening_oracle/cases.rs.
 *
 *   cc -DTEST_NAME='"testframeperm"' -DTEST_SOURCE='"testframeperm.c"' \
 *      hardening_probe.c -I$B -I$T -I$T/star_compat -L$B -last -lm
 *   (cd $T && ./a.out 3>cases.part)
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"

static int recording = 0;
static int nmsg = 0;
static char *msgs[ 64 ];
static int ordinal = 0;

/* The tests print to stdout and stderr, so the cases go to descriptor 3. */
static FILE *out;

void astPutErr_( int status_value, const char *message ) {
   (void) status_value;
   if( !recording ) return;
   if( !strncmp( message, "AST: Error in routine", 21 ) ) return;
   if( nmsg < 64 ) msgs[ nmsg++ ] = strdup( message );
}

/* Print "s" as a Rust string literal. */
static void str( const char *s ) {
   fputc( '"', out );
   for( ; *s; s++ ) {
      unsigned char c = (unsigned char) *s;
      if( c == '\n' ) {
         fputs( "\\n", out );
      } else if( c == '\t' ) {
         fputs( "\\t", out );
      } else if( c == '"' || c == '\\' ) {
         fputc( '\\', out );
         fputc( c, out );
      } else if( c < 0x20 || c >= 0x7f ) {
         fprintf( out, "\\u{%x}", c );
      } else {
         fputc( c, out );
      }
   }
   fputc( '"', out );
}

static AstObject *probe_from_string( const char *dump ) {
   AstObject *obj;
   int i, code;
   nmsg = 0;
   recording = 1;
   obj = astFromString( dump );
   code = astStatus;
   recording = 0;
   fprintf( out, "    Case {\n        test: \"%s\",\n        ordinal: %d,\n        dump: ",
           TEST_NAME, ordinal++ );
   str( dump );
   fprintf( out, ",\n        code: %d,\n        messages: &[", obj && code == 0 ? 0 : code );
   for( i = 0; i < nmsg; i++ ) {
      fprintf( out, "%s", i ? ", " : "" );
      str( msgs[ i ] );
      free( msgs[ i ] );
   }
   fprintf( out, "],\n    },\n" );
   return obj;
}

#undef astFromString
#define astFromString probe_from_string
#define main hardened_test_main
#include TEST_SOURCE
#undef main

int main( void ) {
   int result;
   out = fdopen( 3, "w" );
   if( !out ) return 2;
   result = hardened_test_main();
   fclose( out );
   return result;
}
