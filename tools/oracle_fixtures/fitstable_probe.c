/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* fitstable_probe -- capture how a FitsTable (fitstable.c) behaves: adding
 * columns, creating a FitsTable from a FITS header, column null values,
 * column sizes, getting and putting whole columns, getting and putting the
 * header, copying, and the native dump.
 *
 * Each case is a script of operations applied to a new, empty FitsTable.
 * Each operation is a string of '|'-separated fields:
 *
 *   NEW|cards                astFitsTable from a FitsChan holding the
 *                            ';'-separated cards; the FitsTable replaces the
 *                            current one if it is created. The result is
 *                            the FitsChan's Card attribute afterwards,
 *                            also after an error.
 *   NEWH                     astFitsTable from astGetTableHeader
 *   AC|name|type|dims|unit   astAddColumn; type is a letter (I D F C A S B
 *                            K U) or a number; dims is a ','-separated list
 *                            or empty
 *   RC|name  RR|index  PR    astRemoveColumn, astRemoveRow, astPurgeRows
 *   MR|key                   astMapRemove
 *   P0x|key|value            astMapPut0<x> (x: I D F C S B K)
 *   P1x|key|v,v,...          astMapPut1<x> (x: I D F C S B K)
 *   PALL|key                 astMapPut1B of the 256 values 0 to 255
 *   G1K|key                  astMapGet1K
 *   CNULL|col|set|newval     astColumnNullK, showing the null value, wasset
 *                            and hasnull
 *   CNULLN|col|set|newval    astColumnNullK with no hasnull
 *   CSZ|col                  astColumnSize
 *   GCD|col|fnull|dnull|mx   astGetColumnData into an array of mx bytes
 *                            ("all" for the column size); shows nelem and
 *                            the values, strings with nulls as "\0"
 *   PCD|col|clen|v,v,...     astPutColumnData of the values, of the
 *                            column's type, strings padded with nulls to
 *                            clen
 *   GTH                      astGetTableHeader, showing its Card attribute
 *                            and its cards
 *   PTH|cards                astPutTableHeader of the ';'-separated cards
 *   GA|attr                  astGetC of an attribute
 *   CS|name|mxdim            astColumnShape
 *   CP                       replace the FitsTable by a copy of itself
 *   RW                       write the FitsTable to a Channel and read it
 *                            back
 *   D                        write the FitsTable to a Channel
 *
 * After each operation: "= " and its result, or "! " and the error status,
 * which is then cleared; then any dump lines, each prefixed by "|".
 *
 * Prints Rust source for crates/starlink-ast/tests/fitstable_oracle/cases.rs:
 *
 *   ./fitstable_probe > ../../crates/starlink-ast/tests/fitstable_oracle/cases.rs
 */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"

static char lines[ 2000 ][ 200 ];
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

/* A floating value with "prec" significant digits, or "nan", "inf" or
   "-inf". */
static void addg( double v, int prec ) {
   char buf[ 100 ];
   if( isnan( v ) ) {
      strcpy( buf, " nan" );
   } else if( isinf( v ) ) {
      strcpy( buf, v > 0 ? " inf" : " -inf" );
   } else {
      snprintf( buf, sizeof( buf ), " %.*g", prec, v );
   }
   strcat( out, buf );
}

static int type_code( const char *t ) {
   switch( t[ 0 ] ) {
      case 'I': return AST__INTTYPE;
      case 'D': return AST__DOUBLETYPE;
      case 'F': return AST__FLOATTYPE;
      case 'C': return AST__STRINGTYPE;
      case 'A': return AST__OBJECTTYPE;
      case 'S': return AST__SINTTYPE;
      case 'B': return AST__BYTETYPE;
      case 'K': return AST__KINTTYPE;
      case 'U': return AST__UNDEFTYPE;
   }
   return atoi( t );
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

/* A FitsChan holding the ';'-separated cards in "s". */
static AstFitsChan *cards( char *s ) {
   AstFitsChan *fc;
   char *v[ 40 ];
   int i, n;
   fc = astFitsChan( NULL, NULL, " " );
   n = list( s, ';', v );
   for( i = 0; i < n; i++ ) astPutFits( fc, v[ i ], 0 );
   return fc;
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

/* Show the values in a column data array of "nelem" elements. */
static int col_attr( AstFitsTable *t, const char *attr, const char *col ) {
   char name[ 200 ];
   snprintf( name, sizeof( name ), "%s(%s)", attr, col );
   return astGetI( t, name );
}

static void show_data( AstFitsTable *t, const char *col, void *data, int nelem ) {
   int type, i, j, clen;
   type = col_attr( t, "ColumnType", col );
   for( i = 0; i < nelem; i++ ) {
      switch( type ) {
         case AST__BYTETYPE: addi( " %lld", ( (unsigned char *) data )[ i ] ); break;
         case AST__SINTTYPE: addi( " %lld", ( (short *) data )[ i ] ); break;
         case AST__INTTYPE: addi( " %lld", ( (int *) data )[ i ] ); break;
         case AST__KINTTYPE: addi( " %lld", (long long) ( (int64_t *) data )[ i ] ); break;
         case AST__FLOATTYPE: addg( ( (float *) data )[ i ], 9 ); break;
         case AST__DOUBLETYPE: addg( ( (double *) data )[ i ], 17 ); break;
         case AST__STRINGTYPE:
            clen = col_attr( t, "ColumnLenC", col );
            add( " '%s", "" );
            for( j = 0; j < clen; j++ ) {
               char c = ( (char *) data )[ i*clen + j ];
               if( c ) {
                  char s[ 2 ] = { c, 0 };
                  add( "%s", s );
               } else {
                  add( "%s", "\\0" );
               }
            }
            add( "'%s", "" );
            break;
      }
   }
}

static AstFitsTable *run( AstFitsTable *t, const char *op ) {
   char buf[ 1000 ], *f[ 6 ], *v[ 40 ];
   const char *code, *key, *cval;
   int n, nv, i, dims[ 10 ], ndim, wasset, hasnull, nelem, type, st;
   double dvals[ 40 ];
   float fvals[ 40 ];
   int ivals[ 40 ];
   short svals[ 40 ];
   unsigned char bvals[ 256 ];
   int64_t kvals[ 256 ];
   int64_t null;
   size_t size;
   char *data;
   AstFitsChan *fc;
   AstFitsTable *new;
   AstObject *obj;

   strcpy( buf, op );
   n = split( buf, f );
   code = f[ 0 ];
   key = n > 1 ? f[ 1 ] : "";

   if( !strcmp( code, "NEW" ) ) {
      fc = cards( f[ 1 ] );
      new = astFitsTable( fc, " " );
      st = astStatus;
      astClearStatus;
      addi( "= card %lld\n", astGetI( fc, "Card" ) );
      astSetStatus( st );
      fc = astAnnul( fc );
      if( new ) {
         t = astAnnul( t );
         t = new;
      }
   } else if( !strcmp( code, "NEWH" ) ) {
      fc = astGetTableHeader( t );
      new = astFitsTable( fc, " " );
      add( "=%s\n", "" );
      fc = astAnnul( fc );
      if( new ) {
         t = astAnnul( t );
         t = new;
      }
   } else if( !strcmp( code, "AC" ) ) {
      ndim = list( f[ 3 ], ',', v );
      for( i = 0; i < ndim; i++ ) dims[ i ] = atoi( v[ i ] );
      astAddColumn( t, key, type_code( f[ 2 ] ), ndim, dims, f[ 4 ] );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "RC" ) ) {
      astRemoveColumn( t, key );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "RR" ) ) {
      astRemoveRow( t, atoi( key ) );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "PR" ) ) {
      astPurgeRows( t );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "MR" ) ) {
      astMapRemove( t, key );
      add( "=%s\n", "" );
   } else if( !strncmp( code, "P0", 2 ) ) {
      switch( code[ 2 ] ) {
         case 'I': astMapPut0I( t, key, atoi( f[ 2 ] ), NULL ); break;
         case 'D': astMapPut0D( t, key, atof( f[ 2 ] ), NULL ); break;
         case 'F': astMapPut0F( t, key, (float) atof( f[ 2 ] ), NULL ); break;
         case 'C': astMapPut0C( t, key, f[ 2 ], NULL ); break;
         case 'S': astMapPut0S( t, key, (short) atoi( f[ 2 ] ), NULL ); break;
         case 'B': astMapPut0B( t, key, (unsigned char) atoi( f[ 2 ] ), NULL ); break;
         case 'K': astMapPut0K( t, key, (int64_t) atoll( f[ 2 ] ), NULL ); break;
      }
      add( "=%s\n", "" );
   } else if( !strcmp( code, "PALL" ) ) {
      for( i = 0; i < 256; i++ ) bvals[ i ] = (unsigned char) i;
      astMapPut1B( t, key, 256, bvals, NULL );
      add( "=%s\n", "" );
   } else if( !strncmp( code, "P1", 2 ) ) {
      nv = list( f[ 2 ], ',', v );
      switch( code[ 2 ] ) {
         case 'I':
            for( i = 0; i < nv; i++ ) ivals[ i ] = atoi( v[ i ] );
            astMapPut1I( t, key, nv, ivals, NULL );
            break;
         case 'D':
            for( i = 0; i < nv; i++ ) dvals[ i ] = atof( v[ i ] );
            astMapPut1D( t, key, nv, dvals, NULL );
            break;
         case 'F':
            for( i = 0; i < nv; i++ ) fvals[ i ] = (float) atof( v[ i ] );
            astMapPut1F( t, key, nv, fvals, NULL );
            break;
         case 'C':
            astMapPut1C( t, key, nv, (const char **) v, NULL );
            break;
         case 'S':
            for( i = 0; i < nv; i++ ) svals[ i ] = (short) atoi( v[ i ] );
            astMapPut1S( t, key, nv, svals, NULL );
            break;
         case 'B':
            for( i = 0; i < nv; i++ ) bvals[ i ] = (unsigned char) atoi( v[ i ] );
            astMapPut1B( t, key, nv, bvals, NULL );
            break;
         case 'K':
            for( i = 0; i < nv; i++ ) kvals[ i ] = atoll( v[ i ] );
            astMapPut1K( t, key, nv, kvals, NULL );
            break;
      }
      add( "=%s\n", "" );
   } else if( !strcmp( code, "G1K" ) ) {
      if( astMapGet1K( t, key, 256, &nv, kvals ) ) {
         add( "=%s", "" );
         for( i = 0; i < nv; i++ ) addi( " %lld", (long long) kvals[ i ] );
         add( "%s\n", "" );
      } else {
         add( "= none%s\n", "" );
      }
   } else if( !strcmp( code, "CNULL" ) || !strcmp( code, "CNULLN" ) ) {
      hasnull = -1;
      null = astColumnNullK( t, key, atoi( f[ 2 ] ), (int64_t) atoll( f[ 3 ] ),
                             &wasset, strcmp( code, "CNULL" ) ? NULL : &hasnull );
      addi( "= %lld", (long long) null );
      addi( " %lld", wasset );
      addi( " %lld\n", hasnull );
   } else if( !strcmp( code, "CSZ" ) ) {
      addi( "= %lld\n", (long long) astColumnSize( t, key ) );
   } else if( !strcmp( code, "GCD" ) ) {
      size = astColumnSize( t, key );
      if( strcmp( f[ 4 ], "all" ) ) size = atoi( f[ 4 ] );
      data = calloc( size + 1, 1 );
      nelem = -1;
      astGetColumnData( t, key, (float) atof( f[ 2 ] ), atof( f[ 3 ] ), size, data,
                        &nelem );
      addi( "= %lld", nelem );
      if( astOK ) show_data( t, key, data, nelem );
      add( "%s\n", "" );
      free( data );
   } else if( !strcmp( code, "PCD" ) ) {
      int clen = atoi( f[ 2 ] );
      nv = list( f[ 3 ], ',', v );
      type = col_attr( t, "ColumnType", key );
      data = calloc( nv*( clen + 8 ) + 1, 1 );
      size = 0;
      for( i = 0; i < nv; i++ ) {
         switch( type ) {
            case AST__BYTETYPE: ( (unsigned char *) data )[ i ] = atoi( v[ i ] ); size += 1; break;
            case AST__SINTTYPE: ( (short *) data )[ i ] = atoi( v[ i ] ); size += 2; break;
            case AST__INTTYPE: ( (int *) data )[ i ] = atoi( v[ i ] ); size += 4; break;
            case AST__KINTTYPE: ( (int64_t *) data )[ i ] = atoll( v[ i ] ); size += 8; break;
            case AST__FLOATTYPE: ( (float *) data )[ i ] = atof( v[ i ] ); size += 4; break;
            case AST__DOUBLETYPE: ( (double *) data )[ i ] = atof( v[ i ] ); size += 8; break;
            case AST__STRINGTYPE:
               strncpy( data + i*clen, v[ i ], clen );
               size += clen;
               break;
         }
      }
      if( astOK ) astPutColumnData( t, key, clen, size, data );
      add( "=%s\n", "" );
      free( data );
   } else if( !strcmp( code, "GTH" ) ) {
      char card[ 81 ];
      fc = astGetTableHeader( t );
      if( fc ) {
         addi( "= card %lld\n", astGetI( fc, "Card" ) );
         while( astFindFits( fc, "%f", card, 1 ) ) add( "|%s\n", card );
         fc = astAnnul( fc );
      } else {
         add( "=%s\n", "" );
      }
   } else if( !strcmp( code, "PTH" ) ) {
      fc = cards( f[ 1 ] );
      astPutTableHeader( t, fc );
      fc = astAnnul( fc );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "GA" ) ) {
      cval = astGetC( t, key );
      add( "= '%s'\n", cval ? cval : "(null)" );
   } else if( !strcmp( code, "CS" ) ) {
      int mx = atoi( f[ 2 ] );
      ndim = -1;
      astColumnShape( t, key, mx, &ndim, dims );
      addi( "= %lld", ndim );
      for( i = 0; i < mx; i++ ) addi( " %lld", dims[ i ] );
      add( "%s\n", "" );
   } else if( !strcmp( code, "CP" ) ) {
      new = astCopy( t );
      t = astAnnul( t );
      t = new;
      add( "=%s\n", "" );
   } else if( !strcmp( code, "RW" ) ) {
      AstChannel *ch;
      nline = 0;
      ch = astChannel( source, sink, " " );
      astWrite( ch, t );
      iline = 0;
      obj = astRead( ch );
      ch = astAnnul( ch );
      add( "= %s\n", obj ? astGetC( obj, "Class" ) : "null" );
      if( obj && astIsAFitsTable( obj ) ) {
         t = astAnnul( t );
         t = (AstFitsTable *) obj;
      } else if( obj ) {
         obj = astAnnul( obj );
      }
   } else if( !strcmp( code, "D" ) ) {
      add( "=%s\n", "" );
      dump( (AstObject *) t );
   }
   return t;
}

/* Apply one operation, recording it and its outcome. */
static AstFitsTable *apply( AstFitsTable *t, const char *op ) {
   add( "> %s\n", op );
   t = run( t, op );
   if( !astOK ) {
      addi( "! %lld\n", astStatus );
      astClearStatus;
   }
   return t;
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
   AstFitsTable *t;
   int i;
   out[ 0 ] = 0;
   t = astFitsTable( NULL, " " );
   for( i = 0; ops[ i ]; i++ ) t = apply( t, ops[ i ] );
   t = astAnnul( t );
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

/* The header of the FitsTable built by testfitstable.c. */
#define HEADER1 "XTENSION= 'BINTABLE';BITPIX  =                    8;" \
   "NAXIS   =                    2;NAXIS1  =                   14;" \
   "NAXIS2  =                    0;PCOUNT  =                    0;" \
   "GCOUNT  =                    1;TFIELDS =                    3;" \
   "TFORM1  = '6B      ';TTYPE1  = 'BYTECOL ';TUNIT1  = 'ADU     ';" \
   "TDIM1   = '(2,3)   ';TFORM2  = '1K      ';TTYPE2  = 'KINTCOL ';" \
   "TUNIT2  = 'm       ';TFORM3  = '0A      ';TTYPE3  = 'STRINGCOL';" \
   "TDIM3   = '(0,3)   '"

/* A one-column header with the column cards given. */
#define ONECOL(cols) "XTENSION= 'BINTABLE';TFIELDS =                    1;" cols

int main( void ) {
   int status = 0;
   astWatch( &status );

   printf( "// Generated by tools/oracle_fixtures/fitstable_probe.c. Do not edit.\n\n" );
   printf( "/// `(name, operations, transcript)`: see the probe for the operations\n"
           "/// and the transcript format.\n" );
   printf( "pub type Case = (&'static str, &'static [&'static str], &'static str);\n\n" );
   printf( "pub const CASES: &[Case] = &[\n" );

   CASE( "empty", "GA|NRow", "GA|NColumn", "D", "GTH", "D" );
   CASE( "add_column_types", "AC|JUNK|A||m", "AC|U|U||", "AC|P|6||", "AC|X|99||",
         "AC|OK|D||", "GA|NColumn", "D" );
   CASE( "testfitstable_header", "AC|BYTECOL|B|2,3|ADU", "AC|KINTCOL|K||m",
         "AC|STRINGCOL|C|3| ", "GTH", "D" );
   CASE( "from_header", "NEW|" HEADER1, "GA|NColumn", "GA|ColumnLength(bytecol)",
         "GA|ColumnNdim(bytecol)", "GA|ColumnType(bytecol)", "GA|ColumnUnit(bytecol)",
         "GA|ColumnLength(kintcol)", "GA|ColumnNdim(kintcol)", "GA|ColumnType(kintcol)",
         "GA|ColumnUnit(kintcol)", "GA|ColumnLength(StringCol)",
         "GA|ColumnNdim(StringCol)", "GA|ColumnType(StringCol)", "CS|StringCol|2", "D",
         "GTH" );
   CASE( "from_header_extra_cards", "NEW|EXTNAME = 'WCS-TAB ';" HEADER1 ";EXTVER  = 2;"
         "TNULL1  = 254", "D", "GTH" );
   CASE( "header_columns",
         "NEW|" ONECOL( "TFORM1  = 'J       ';TTYPE1  = 'SCALAR  '" ), "D",
         "NEW|" ONECOL( "TFORM1  = '10A     ';TTYPE1  = 'NAME    '" ), "GA|ColumnNdim(NAME)",
         "GA|ColumnType(NAME)", "D",
         "NEW|" ONECOL( "TFORM1  = '6J      ';TTYPE1  = 'ARR     ';TDIM1   = '2,3     '" ),
         "GA|ColumnNdim(ARR)", "GA|ColumnLength(ARR)", "D",
         "NEW|" ONECOL( "TFORM1  = '3E      '" ), "D",
         "NEW|" ONECOL( "TFORM1  = '1E      ';TSCAL1  = 1.0;TZERO1  = 0.0" ), "D",
         "NEW|" ONECOL( "TFORM1  = '20A     ';TTYPE1  = 'S       ';TDIM1   = '(10)    '" ),
         "GA|ColumnNdim(S)", "D",
         "NEW|" ONECOL( "TFORM1  = '1I      ';TTYPE1  = 'N       ';TNULL1  = -5" ),
         "CNULL|N|0|0", "D" );
   CASE( "header_errors",
         "NEW|" ONECOL( "TTYPE1  = 'NOFORM  '" ),
         "NEW|" ONECOL( "TFORM1  = '1X      '" ),
         "NEW|" ONECOL( "TFORM1  = '-1J     '" ),
         "NEW|" ONECOL( "TFORM1  = ''" ),
         "NEW|" ONECOL( "TFORM1  = '5J      ';TDIM1   = '(2,3)   '" ),
         "NEW|" ONECOL( "TFORM1  = '1E      ';TSCAL1  = 2.0" ),
         "NEW|" ONECOL( "TFORM1  = '1E      ';TZERO1  = 5.0" ),
         "NEW|" ONECOL( "TFORM1  = '1E      ';TSCAL1  = 'x'" ),
         "NEW|" ONECOL( "TFORM1  = '2J      ';TDIM1   = '(2,0)   '" ),
         "NEW|XTENSION= 'BINTABLE';TFIELDS =                    2;TFORM1  = '1J      ';"
         "TTYPE1  = 'A       ';TFORM2  = '1J      ';TTYPE2  = 'A       '",
         "NEW|" ONECOL( "TFORM1  = '1B      ';TNULL1  = 300" ),
         "GA|NColumn" );
   CASE( "data", "NEW|" HEADER1, "P1B|BYTECOL(1)|0,1,128,127,129,255",
         "P1B|BYTECOL(2)|0,1,0,1,0,1", "P0K|KINTCOL(2)|10", "P0K|KINTCOL(3)|-10",
         "P1C|STRINGCOL(1)|hello, ,goodbye", "P1C|STRINGCOL(3)| , , ", "GA|NRow",
         "CSZ|stringcol", "GA|ColumnLenC(StringCol)", "GCD|StringCol|0|0|all",
         "CSZ|bytecol", "GCD|BYTECOL|0|0|all", "CNULL|BYTECOL|0|0", "CSZ|kintcol",
         "GCD|KINTCOL|0|0|all", "CNULL|KINTCOL|0|0", "GTH", "D" );
   CASE( "column_null", "AC|I|I||", "AC|B|B|2|", "AC|F|F||", "P0I|I(1)|5",
         "P0I|I(2)|7", "CNULL|I|0|0", "CNULLN|I|0|0", "CNULL|I|1|7", "CNULL|I|0|0",
         "CNULL|I|1|5", "CNULL|I|0|0", "CNULL|B|1|300", "CNULL|B|1|-1", "CNULL|B|0|0",
         "P1B|B(3)|0,255", "CNULL|B|0|0", "P1B|B(1)|1,254", "CNULL|B|0|0",
         "CNULL|F|1|3", "CNULL|nothing|0|0", "D" );
   CASE( "column_null_arrays", "AC|A|I|2,3|", "P1I|A(1)|1,2,3,4,5,9",
         "P1I|A(2)|1,2,3,4,5,6", "CNULL|A|1|9", "CNULL|A|0|0", "CNULLN|A|0|0" );
   CASE( "column_null_exhausted", "AC|ALL|B|256|", "PALL|ALL(1)", "CNULL|ALL|0|0",
         "CNULLN|ALL|0|0", "PALL|ALL(3)", "CNULL|ALL|0|0", "CNULLN|ALL|0|0", "GTH" );
   CASE( "column_null_kint", "AC|K|K||", "P0K|K(1)|9223372036854775807",
         "P0K|K(2)|-9223372036854775808", "CNULL|K|0|0", "P0K|K(4)|9223372036854775806",
         "P0K|K(5)|-9223372036854775807", "CNULL|K|0|0", "GTH" );
   CASE( "column_size", "AC|I|I|2|", "AC|S|S||", "AC|K|K||", "AC|F|F||", "AC|D|D|3|",
         "AC|B|B||", "AC|C|C|2|", "CSZ|I", "P0S|S(3)|1", "CSZ|I", "CSZ|S", "CSZ|K",
         "CSZ|F", "CSZ|D", "CSZ|B", "CSZ|C", "P1C|C(1)|abc,de", "CSZ|C", "CSZ|nothing" );
   CASE( "get_column_data_floats", "AC|D|D|2|", "AC|F|F||",
         "P1D|D(1)|1.5,nan", "P1D|D(3)|inf,-2.25", "P0F|F(2)|0.1", "P0F|F(3)|inf",
         "GCD|D|0|-1|all", "GCD|D|0|nan|all", "GCD|F|-1|0|all", "GCD|F|nan|0|all",
         "GCD|D|0|7|20", "GCD|D|0|7|16", "GCD|D|0|7|0", "GCD|F|-1|0|11" );
   CASE( "get_column_data_ints", "AC|I|I||", "AC|S|S|2|", "AC|B|B||", "AC|K|K||",
         "P0I|I(1)|3", "P0I|I(3)|-3", "P1S|S(2)|1,-1", "P0B|B(1)|255", "P0K|K(3)|-7",
         "GCD|I|0|0|all", "GCD|S|0|0|all", "GCD|B|0|0|all", "GCD|K|0|0|all",
         "CNULL|I|1|99", "GCD|I|0|0|all", "CNULL|S|1|-32768", "GCD|S|0|0|all" );
   CASE( "get_column_data_strings", "AC|C|C|2|", "P1C|C(1)|abc,d", "P1C|C(3)|,xy",
         "GCD|C|0|0|all", "GCD|C|0|0|7", "AC|E|C||", "P0C|E(2)|", "GCD|E|0|0|all",
         "CSZ|E" );
   CASE( "put_column_data", "AC|I|I|2|", "AC|D|D||", "AC|F|F||", "AC|C|C|2|",
         "AC|K|K||", "AC|S|S||", "AC|B|B||",
         "PCD|I|0|1,2,3,4,5,6", "GA|NRow", "PCD|D|0|1.5,nan,2.5", "PCD|F|0|nan,0.5",
         "PCD|C|4|ab,c,,defg", "PCD|K|0|-1,1", "PCD|S|0|-2", "PCD|B|0|200,201,202,203",
         "GA|NRow", "D", "PCD|I|0|7,8", "PCD|D|0|", "GA|NRow", "D",
         "PCD|I|0|1,2,3", "PCD|C|0|", "PCD|nothing|0|1", "D" );
   CASE( "put_column_data_strings", "AC|C|C||", "PCD|C|5|hello,hi  ,  x", "G1K|C(1)",
         "GA|ColumnLenC(C)", "GCD|C|0|0|all", "D" );
   CASE( "header_nulls", "AC|I|I||", "AC|J|I||", "AC|K|I||", "AC|D|D||",
         "P0I|I(1)|1", "P0I|I(3)|2", "P0I|J(1)|1", "P0I|J(2)|1", "P0I|J(3)|1",
         "P0I|K(1)|0", "P0I|K(2)|0", "P0I|K(3)|0", "CNULL|K|1|-1", "GTH", "D" );
   CASE( "header_strings", "AC|S|C||", "AC|V|C|2,2|", "AC|N|C|2|", "P0C|S(1)|abc",
         "P1C|V(1)|a,bb,ccc,d", "GTH", "NEWH", "GA|ColumnNdim(S)", "GA|ColumnNdim(V)",
         "GA|ColumnLength(V)", "GA|ColumnNdim(N)", "D" );
   CASE( "put_table_header", "AC|X|D|2|deg",
         "PTH|SIMPLE  =                    T;EXTNAME = 'TABLE   ';NAXIS1  = 99;"
         "TFORM1  = '9J      ';TFORM2  = '1E      ';TTYPE7  = 'Q       ';THEAP   = 5;"
         "COMMENT a comment;TDIM1   = '(3,3)   ';TUNIT1  = 'm       ';"
         "TNULL3  = 1;HISTORY history",
         "D", "GTH", "D" );
   CASE( "round_trip", "NEW|" HEADER1, "P1B|BYTECOL(2)|1,2,3,4,5,6",
         "P1C|STRINGCOL(1)|a,bb,ccc", "CNULL|KINTCOL|1|-1", "GTH", "NEWH", "D", "GTH" );
   CASE( "copy_and_read", "NEW|" HEADER1, "P1B|BYTECOL(1)|0,1,2,3,4,5",
         "P0K|KINTCOL(2)|10", "CNULL|BYTECOL|1|9", "CP", "D", "RW", "GA|NRow",
         "CNULL|BYTECOL|0|0", "D", "GTH", "RW", "D" );
   CASE( "purge", "NEW|" HEADER1, "P1B|BYTECOL(1)|0,1,128,127,129,255",
         "P1B|BYTECOL(2)|0,1,0,1,0,1", "P0K|KINTCOL(2)|10", "P0K|KINTCOL(3)|-10",
         "P1C|STRINGCOL(1)|hello, ,goodbye", "P1C|STRINGCOL(3)| , , ",
         "MR|BYTECOL(3)", "MR|KINTCOL(3)", "MR|STRINGCOL(3)", "GTH",
         "CNULL|BYTECOL|0|0", "PR", "GA|NRow", "GTH", "CNULL|BYTECOL|1|11",
         "CNULL|BYTECOL|0|0", "D" );
   printf( "];\n" );
   return 0;
}
