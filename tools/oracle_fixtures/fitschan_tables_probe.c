/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* fitschan_tables_probe -- capture how a FitsChan (fitschan.c) holds
 * FitsTables: astPutTable, astPutTables, astGetTables, astRemoveTables,
 * and the tables in a copy, an emptied FitsChan and the native dump.
 *
 * Each case is a script of operations applied to a new, empty FitsChan.
 * Each operation is a string of '|'-separated fields:
 *
 *   PT|ext|col               astPutTable of a FitsTable with one column,
 *                            named col, under the name ext
 *   PTS|entries              astPutTables of a KeyMap whose ','-separated
 *                            entries are key=col (a FitsTable as for PT),
 *                            key=#n (the integer n), key=*col (a vector of
 *                            two such FitsTables) or key=? (a KeyMap)
 *   GT                       astGetTables: "null", or the number of tables
 *                            and each key with the name of its table's
 *                            first column, in the order of the keys (the
 *                            port reads out a KeyMap with SortBy=None in
 *                            key order, where C uses its hash table)
 *   RT|key                   astRemoveTables
 *   PF|card                  astPutFits
 *   EF                       astEmptyFits
 *   PC|cards                 astPutCards
 *   CP                       replace the FitsChan by a copy of itself
 *   RW                       write the FitsChan to a Channel and read it
 *                            back
 *   D                        write the FitsChan to a Channel
 *
 * After each operation: "= " and its result, or "! " and the error status,
 * which is then cleared; then any dump lines, each prefixed by "|".
 *
 * Prints Rust source for crates/starlink-ast/tests/fitschan_tables_oracle/cases.rs:
 *
 *   ./fitschan_tables_probe > ../../crates/starlink-ast/tests/fitschan_tables_oracle/cases.rs
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"

static char lines[ 4000 ][ 200 ];
static int nline, iline;
static char out[ 400000 ];

static void sink( const char *line ) {
   strncpy( lines[ nline ], line, 199 );
   lines[ nline++ ][ 199 ] = 0;
}

static const char *source( void ) {
   return iline < nline ? lines[ iline++ ] : NULL;
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

/* Split a list separated by "sep" into strings. */
static int list( char *s, char sep, char *v[ 40 ] ) {
   int n = 0;
   if( !*s ) return 0;
   v[ n++ ] = s;
   while( n < 40 && ( s = strchr( s, sep ) ) ) {
      *s++ = 0;
      v[ n++ ] = s;
   }
   return n;
}

/* A FitsTable with one integer column named "col", holding 1 in row 1. */
static AstFitsTable *table( const char *col ) {
   AstFitsTable *ft;
   char key[ 200 ];
   ft = astFitsTable( NULL, " " );
   astAddColumn( ft, col, AST__INTTYPE, 0, NULL, " " );
   snprintf( key, sizeof( key ), "%s(1)", col );
   astMapPut0I( ft, key, 1, NULL );
   return ft;
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

static AstFitsChan *run( AstFitsChan *fc, const char *op ) {
   char buf[ 2000 ], *f[ 6 ], *v[ 40 ];
   const char *code, *key;
   int n, nv, i;
   AstFitsTable *ft;
   AstFitsChan *new;
   AstKeyMap *km;
   AstObject *obj;

   strcpy( buf, op );
   n = split( buf, f );
   code = f[ 0 ];
   key = n > 1 ? f[ 1 ] : "";

   if( !strcmp( code, "PT" ) ) {
      ft = table( f[ 2 ] );
      astPutTable( fc, ft, key );
      ft = astAnnul( ft );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "PTS" ) ) {
      km = astKeyMap( " " );
      nv = list( f[ 1 ], ',', v );
      for( i = 0; i < nv; i++ ) {
         char *val = strchr( v[ i ], '=' );
         *val++ = 0;
         if( val[ 0 ] == '#' ) {
            astMapPut0I( km, v[ i ], atoi( val + 1 ), NULL );
         } else if( val[ 0 ] == '*' ) {
            AstObject *pair[ 2 ];
            pair[ 0 ] = (AstObject *) table( val + 1 );
            pair[ 1 ] = (AstObject *) table( val + 1 );
            astMapPut1A( km, v[ i ], 2, pair, NULL );
            pair[ 0 ] = astAnnul( pair[ 0 ] );
            pair[ 1 ] = astAnnul( pair[ 1 ] );
         } else if( val[ 0 ] == '?' ) {
            obj = (AstObject *) astKeyMap( " " );
            astMapPut0A( km, v[ i ], obj, NULL );
            obj = astAnnul( obj );
         } else {
            ft = table( val );
            astMapPut0A( km, v[ i ], ft, NULL );
            ft = astAnnul( ft );
         }
      }
      astPutTables( fc, km );
      km = astAnnul( km );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "GT" ) ) {
      km = astGetTables( fc );
      if( km ) astSet( km, "SortBy=KeyUp" );
      if( !km ) {
         add( "= null%s\n", "" );
      } else {
         addi( "= %lld", astMapSize( km ) );
         for( i = 0; i < astMapSize( km ); i++ ) {
            const char *k = astMapKey( km, i );
            add( " %s:", k );
            if( astMapGet0A( km, k, &obj ) ) {
               if( astIsAFitsTable( obj ) ) {
                  add( "%s", astColumnName( (AstFitsTable *) obj, 1 ) );
               } else {
                  add( "%s", astGetC( obj, "Class" ) );
               }
               obj = astAnnul( obj );
            }
         }
         add( "%s\n", "" );
         km = astAnnul( km );
      }
   } else if( !strcmp( code, "RT" ) ) {
      astRemoveTables( fc, key );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "PF" ) ) {
      astPutFits( fc, key, 0 );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "EF" ) ) {
      astEmptyFits( fc );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "PC" ) ) {
      astPutCards( fc, key );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "CP" ) ) {
      new = astCopy( fc );
      fc = astAnnul( fc );
      fc = new;
      add( "=%s\n", "" );
   } else if( !strcmp( code, "RW" ) ) {
      AstChannel *ch;
      nline = 0;
      ch = astChannel( source, sink, " " );
      astWrite( ch, fc );
      iline = 0;
      obj = astRead( ch );
      ch = astAnnul( ch );
      add( "= %s\n", obj ? astGetC( obj, "Class" ) : "null" );
      if( obj && astIsAFitsChan( obj ) ) {
         fc = astAnnul( fc );
         fc = (AstFitsChan *) obj;
      } else if( obj ) {
         obj = astAnnul( obj );
      }
   } else if( !strcmp( code, "D" ) ) {
      add( "=%s\n", "" );
      dump( (AstObject *) fc );
   }
   return fc;
}

/* Apply one operation, recording it and its outcome. */
static AstFitsChan *apply( AstFitsChan *fc, const char *op ) {
   add( "> %s\n", op );
   fc = run( fc, op );
   if( !astOK ) {
      addi( "! %lld\n", astStatus );
      astClearStatus;
   }
   return fc;
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
   fc = astFitsChan( NULL, NULL, " " );
   for( i = 0; ops[ i ]; i++ ) fc = apply( fc, ops[ i ] );
   fc = astAnnul( fc );
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

int main( void ) {
   int status = 0;
   astWatch( &status );

   printf( "// Generated by tools/oracle_fixtures/fitschan_tables_probe.c. Do not edit.\n\n" );
   printf( "/// `(name, operations, transcript)`: see the probe for the operations\n"
           "/// and the transcript format.\n" );
   printf( "pub type Case = (&'static str, &'static [&'static str], &'static str);\n\n" );
   printf( "pub const CASES: &[Case] = &[\n" );

   CASE( "none", "GT", "RT|", "RT|A", "GT", "D" );
   CASE( "put_table", "PT|WCS-TAB|COORDS", "GT", "PT|INDEX|IDX", "GT",
         "PT|WCS-TAB|NEWCOORDS", "GT", "D" );
   CASE( "put_tables", "PTS|A=COLA,B=COLB,N=#3,V=*COLV,K=?", "GT",
         "PTS|A=COLA2,C=COLC", "GT", "D" );
   CASE( "put_tables_empty", "PTS|N=#1", "GT", "D" );
   CASE( "remove_list", "PT|A|CA", "PT|B|CB", "PT|C|CC", "PT|D|CD", "RT|B,D", "GT",
         "RT|nothing", "RT|A, C", "GT", "RT|C,A", "GT", "D" );
   CASE( "remove_all", "PT|A|CA", "PT|B|CB", "PT|C|CC", "PT|D|CD", "PT|E|CE", "RT|",
         "GT", "RT|  ", "GT", "D" );
   CASE( "remove_escaped", "PT|A,B|CAB", "PT|A|CA", "PT|B|CB", "RT|A\\,B", "GT" );
   CASE( "key_case", "PT|wcs|C1", "PT|WCS|C2", "GT", "RT|Wcs", "GT", "RT|wcs", "GT" );
   CASE( "copy", "PT|A|CA", "PF|NAXIS   = 2", "CP", "GT", "D" );
   CASE( "read_back", "PT|A|CA", "PT|B|CB", "PF|NAXIS   = 2", "RW", "GT", "D" );
   CASE( "empty_fits", "PT|A|CA", "PF|NAXIS   = 2", "EF", "GT", "D", "PT|B|CB", "GT" );
   CASE( "put_cards", "PT|A|CA", "PC|NAXIS   = 2", "GT", "D" );
   CASE( "emptied_by_remove", "PT|A|CA", "RT|A", "GT", "D", "RW", "D" );
   printf( "];\n" );
   return 0;
}
