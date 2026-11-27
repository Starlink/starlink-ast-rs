/* Copyright 2026 Association of Universities for Research in Astronomy (AURA)
 * SPDX-License-Identifier: LGPL-3.0-or-later
 *
 * load_one -- read every Object from one file with the C library and print
 * what happened, for crates/ast-oracle-tests/src/bin/hardening_replay.rs to
 * compare with the port.
 *
 *   load_one native FILE   each line through a Channel, astRead until no more
 *   load_one fits FILE     each line put in a FitsChan with astPutFits, then
 *                          astRead with the default Encoding until no more
 *
 * Output, one item a line:
 *   ENCODING <name>      (fits) the Encoding the cards gave
 *   !! <status> <text>   each message the library reports, as astPutErr gets it
 *   PUTERR <status>      (fits) a card the FitsChan would not store
 *   OBJECT <class>       an Object read
 *   ERROR <status>       a read that failed, which ends the file
 *   END                  the end of the file
 * At most 16 Objects are read, as the fuzz targets read.
 *
 * The probe defines astPutErr itself to collect messages, so it does not
 * link libast_err. Build and run from reference/starlink-ast/build:
 *
 *   cc -O2 -I. ../../../tools/oracle_fixtures/load_one.c -L. -last -lm \
 *      -Wl,-rpath,$PWD -o ../../../tools/oracle_fixtures/load_one
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"

#define MAX_OBJECTS 16

void astPutErr_( int status, const char *message ) {
   static const char where[] = "AST: Error in routine ";
   if( strncmp( message, where, sizeof( where ) - 1 ) ) {
      printf( "!! %d %s\n", status, message );
   }
}

static FILE *in;

/* The next line of the input, without its newline, as a C string, which
   ends at any NUL in it. */
static const char *source( void ) {
   static char line[ 100000 ];
   size_t n;
   if( !fgets( line, sizeof line, in ) ) return NULL;
   n = strlen( line );
   while( n > 0 && ( line[ n - 1 ] == '\n' || line[ n - 1 ] == '\r' ) ) line[ --n ] = 0;
   return line;
}

/* Read up to MAX_OBJECTS Objects from "chan", printing each. */
static void read_all( AstChannel *chan ) {
   int status;
   int i;
   for( i = 0; i < MAX_OBJECTS; i++ ) {
      AstObject *obj = astRead( chan );
      if( !astOK ) {
         status = astStatus;
         astClearStatus;
         printf( "ERROR %d\n", status );
         return;
      }
      if( !obj ) break;
      printf( "OBJECT %s\n", astGetC( obj, "Class" ) );
      obj = astAnnul( obj );
   }
   printf( "END\n" );
}

int main( int argc, char **argv ) {
   int status = 0;
   const char *line;
   setbuf( stdout, NULL );
   if( argc != 3 || ( strcmp( argv[ 1 ], "native" ) && strcmp( argv[ 1 ], "fits" ) ) ) {
      fprintf( stderr, "usage: load_one native|fits FILE\n" );
      return 2;
   }
   in = fopen( argv[ 2 ], "rb" );
   if( !in ) {
      perror( argv[ 2 ] );
      return 2;
   }
   astWatch( &status );
   astBegin;
   if( !strcmp( argv[ 1 ], "native" ) ) {
      read_all( (AstChannel *) astChannel( source, NULL, " " ) );
   } else {
      AstFitsChan *fc = astFitsChan( NULL, NULL, " " );
      while( ( line = source() ) ) {
         if( !line[ 0 ] ) continue;
         astPutFits( fc, line, 0 );
         if( !astOK ) {
            status = astStatus;
            astClearStatus;
            printf( "PUTERR %d\n", status );
         }
      }
      astClear( fc, "Card" );
      printf( "ENCODING %s\n", astGetC( fc, "Encoding" ) );
      read_all( (AstChannel *) fc );
   }
   astEnd;
   return 0;
}
