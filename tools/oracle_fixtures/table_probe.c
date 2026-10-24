/* table_probe -- capture how a Table (table.c) behaves: adding, removing
 * and querying columns and parameters, putting and getting cell values,
 * removing and purging rows, its read-only attributes, and its native dump.
 *
 * Each case is a script of operations applied to a new Table. Each
 * operation is a string of '|'-separated fields:
 *
 *   AC|name|type|dims|unit   astAddColumn; type is a letter (I D F C A S B
 *                            K U) or a number; dims is a ','-separated list
 *                            or empty
 *   AP|name                  astAddParameter
 *   RC|name  RP|name         astRemoveColumn, astRemoveParameter
 *   RR|index                 astRemoveRow
 *   PR                       astPurgeRows
 *   MR|key                   astMapRemove
 *   MN|old|new               astMapRename
 *   P0x|key|value[|comment]  astMapPut0<x> (x: I D F C S B K, or A to put
 *                            a new empty KeyMap)
 *   P1x|key|v,v,...          astMapPut1<x> (x: I D C B K)
 *   PEx|key|elem|value       astMapPutElem<x> (x: I D C)
 *   PU|key                   astMapPutU
 *   G0x|key                  astMapGet0<x> (x: I D C A)
 *   G1x|key                  astMapGet1<x> (x: I D C)
 *   GEx|key|elem             astMapGetElem<x> (x: I C)
 *   GA|attr                  astGetC of an attribute
 *   SA|setting  CA|attr      astSet, astClear
 *   TA|attr                  astTest
 *   CN|index  PN|index       astColumnName, astParameterName
 *   CS|name|mxdim            astColumnShape
 *   HC|name  HP|name         astHasColumn, astHasParameter
 *   MS  MK|index  MT|key     astMapSize, astMapKey, astMapType
 *   CP                       replace the Table by a copy of itself
 *   RW                       write the Table to a Channel and read it back
 *   D                        write the Table to a Channel
 *
 * After each operation: "= " and its result, or "! " and the error status,
 * which is then cleared; then any dump lines, each prefixed by "|".
 *
 * Prints Rust source for crates/starlink-ast/tests/table_oracle/cases.rs:
 *
 *   ./table_probe > ../../crates/starlink-ast/tests/table_oracle/cases.rs
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"

static char lines[ 2000 ][ 200 ];
static int nline, iline;
static char out[ 200000 ];

static void sink( const char *line ) {
   strncpy( lines[ nline ], line, 199 );
   lines[ nline++ ][ 199 ] = 0;
}

static const char *source( void ) {
   return iline < nline ? lines[ iline++ ] : NULL;
}

static void add( const char *fmt, const char *s ) {
   char buf[ 400 ];
   snprintf( buf, sizeof( buf ), fmt, s );
   strcat( out, buf );
}

static void addi( const char *fmt, long v ) {
   char buf[ 100 ];
   snprintf( buf, sizeof( buf ), fmt, v );
   strcat( out, buf );
}

static void addd( double v ) {
   char buf[ 100 ];
   snprintf( buf, sizeof( buf ), "%.17g", v );
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

/* Split a ','-separated list into strings. */
static int list( char *s, char *v[ 20 ] ) {
   int n = 0;
   if( !*s ) return 0;
   v[ n++ ] = s;
   while( n < 20 && ( s = strchr( s, ',' ) ) ) {
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

static AstTable *run( AstTable *t, const char *op ) {
   char buf[ 400 ], *f[ 6 ], *v[ 20 ];
   const char *code, *key, *cval;
   char cbuf[ 20 * 41 ];
   int n, nv, i, ival, ivals[ 20 ], nval, dims[ 10 ], ndim;
   double dval, dvals[ 20 ];
   unsigned char bvals[ 20 ];
   int64_t kvals[ 20 ];
   AstObject *obj;
   AstTable *copy;

   strcpy( buf, op );
   n = split( buf, f );
   code = f[ 0 ];
   key = n > 1 ? f[ 1 ] : "";

   if( !strcmp( code, "AC" ) ) {
      ndim = list( f[ 3 ], v );
      for( i = 0; i < ndim; i++ ) dims[ i ] = atoi( v[ i ] );
      astAddColumn( t, key, type_code( f[ 2 ] ), ndim, dims, f[ 4 ] );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "AP" ) ) {
      astAddParameter( t, key );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "RC" ) ) {
      astRemoveColumn( t, key );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "RP" ) ) {
      astRemoveParameter( t, key );
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
   } else if( !strcmp( code, "MN" ) ) {
      astMapRename( t, key, f[ 2 ] );
      add( "=%s\n", "" );
   } else if( !strncmp( code, "P0", 2 ) ) {
      const char *com = n > 3 ? f[ 3 ] : NULL;
      switch( code[ 2 ] ) {
         case 'I': astMapPut0I( t, key, atoi( f[ 2 ] ), com ); break;
         case 'D': astMapPut0D( t, key, atof( f[ 2 ] ), com ); break;
         case 'F': astMapPut0F( t, key, (float) atof( f[ 2 ] ), com ); break;
         case 'C': astMapPut0C( t, key, f[ 2 ], com ); break;
         case 'S': astMapPut0S( t, key, (short) atoi( f[ 2 ] ), com ); break;
         case 'B': astMapPut0B( t, key, (unsigned char) atoi( f[ 2 ] ), com ); break;
         case 'K': astMapPut0K( t, key, (int64_t) atoll( f[ 2 ] ), com ); break;
         case 'A':
            obj = (AstObject *) astKeyMap( " " );
            astMapPut0A( t, key, obj, com );
            obj = astAnnul( obj );
            break;
      }
      add( "=%s\n", "" );
   } else if( !strncmp( code, "P1", 2 ) ) {
      nv = list( f[ 2 ], v );
      switch( code[ 2 ] ) {
         case 'I':
            for( i = 0; i < nv; i++ ) ivals[ i ] = atoi( v[ i ] );
            astMapPut1I( t, key, nv, ivals, NULL );
            break;
         case 'D':
            for( i = 0; i < nv; i++ ) dvals[ i ] = atof( v[ i ] );
            astMapPut1D( t, key, nv, dvals, NULL );
            break;
         case 'C':
            astMapPut1C( t, key, nv, (const char **) v, NULL );
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
   } else if( !strncmp( code, "PE", 2 ) ) {
      i = atoi( f[ 2 ] );
      switch( code[ 2 ] ) {
         case 'I': astMapPutElemI( t, key, i, atoi( f[ 3 ] ) ); break;
         case 'D': astMapPutElemD( t, key, i, atof( f[ 3 ] ) ); break;
         case 'C': astMapPutElemC( t, key, i, f[ 3 ] ); break;
      }
      add( "=%s\n", "" );
   } else if( !strcmp( code, "PU" ) ) {
      astMapPutU( t, key, NULL );
      add( "=%s\n", "" );
   } else if( !strncmp( code, "G0", 2 ) ) {
      switch( code[ 2 ] ) {
         case 'I':
            ival = 0;
            if( astMapGet0I( t, key, &ival ) ) addi( "= %ld\n", ival );
            else add( "= none%s\n", "" );
            break;
         case 'D':
            if( astMapGet0D( t, key, &dval ) ) {
               add( "= %s", "" );
               addd( dval );
               add( "%s\n", "" );
            } else {
               add( "= none%s\n", "" );
            }
            break;
         case 'C':
            if( astMapGet0C( t, key, &cval ) ) add( "= '%s'\n", cval );
            else add( "= none%s\n", "" );
            break;
         case 'A':
            if( astMapGet0A( t, key, &obj ) ) {
               add( "= %s\n", obj ? astGetC( obj, "Class" ) : "null" );
               if( obj ) obj = astAnnul( obj );
            } else {
               add( "= none%s\n", "" );
            }
            break;
      }
   } else if( !strncmp( code, "G1", 2 ) ) {
      nval = 0;
      switch( code[ 2 ] ) {
         case 'I':
            if( astMapGet1I( t, key, 20, &nval, ivals ) ) {
               add( "=%s", "" );
               for( i = 0; i < nval; i++ ) addi( " %ld", ivals[ i ] );
               add( "%s\n", "" );
            } else {
               add( "= none%s\n", "" );
            }
            break;
         case 'D':
            if( astMapGet1D( t, key, 20, &nval, dvals ) ) {
               add( "=%s", "" );
               for( i = 0; i < nval; i++ ) {
                  add( " %s", "" );
                  addd( dvals[ i ] );
               }
               add( "%s\n", "" );
            } else {
               add( "= none%s\n", "" );
            }
            break;
         case 'C':
            if( astMapGet1C( t, key, 41, 20, &nval, cbuf ) ) {
               add( "=%s", "" );
               for( i = 0; i < nval; i++ ) add( " '%s'", cbuf + 41*i );
               add( "%s\n", "" );
            } else {
               add( "= none%s\n", "" );
            }
            break;
      }
   } else if( !strncmp( code, "GE", 2 ) ) {
      i = atoi( f[ 2 ] );
      switch( code[ 2 ] ) {
         case 'I':
            if( astMapGetElemI( t, key, i, &ival ) ) addi( "= %ld\n", ival );
            else add( "= none%s\n", "" );
            break;
         case 'C':
            if( astMapGetElemC( t, key, 41, i, cbuf ) ) add( "= '%s'\n", cbuf );
            else add( "= none%s\n", "" );
            break;
      }
   } else if( !strcmp( code, "GA" ) ) {
      cval = astGetC( t, key );
      add( "= '%s'\n", cval ? cval : "(null)" );
   } else if( !strcmp( code, "SA" ) ) {
      astSet( t, "%s", key );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "CA" ) ) {
      astClear( t, key );
      add( "=%s\n", "" );
   } else if( !strcmp( code, "TA" ) ) {
      addi( "= %ld\n", astTest( t, key ) );
   } else if( !strcmp( code, "CN" ) ) {
      cval = astColumnName( t, atoi( key ) );
      add( "= '%s'\n", cval ? cval : "(null)" );
   } else if( !strcmp( code, "PN" ) ) {
      cval = astParameterName( t, atoi( key ) );
      add( "= '%s'\n", cval ? cval : "(null)" );
   } else if( !strcmp( code, "CS" ) ) {
      int mx = atoi( f[ 2 ] );
      ndim = -1;
      astColumnShape( t, key, mx, &ndim, dims );
      addi( "= %ld", ndim );
      for( i = 0; i < mx; i++ ) addi( " %ld", dims[ i ] );
      add( "%s\n", "" );
   } else if( !strcmp( code, "HC" ) ) {
      addi( "= %ld\n", astHasColumn( t, key ) );
   } else if( !strcmp( code, "HP" ) ) {
      addi( "= %ld\n", astHasParameter( t, key ) );
   } else if( !strcmp( code, "MS" ) ) {
      addi( "= %ld\n", astMapSize( t ) );
   } else if( !strcmp( code, "MK" ) ) {
      cval = astMapKey( t, atoi( key ) );
      add( "= '%s'\n", cval ? cval : "(null)" );
   } else if( !strcmp( code, "MT" ) ) {
      addi( "= %ld\n", astMapType( t, key ) );
   } else if( !strcmp( code, "CP" ) ) {
      copy = astCopy( t );
      t = astAnnul( t );
      t = copy;
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
      if( obj && astIsATable( obj ) ) {
         t = astAnnul( t );
         t = (AstTable *) obj;
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
static AstTable *apply( AstTable *t, const char *op ) {
   add( "> %s\n", op );
   t = run( t, op );
   if( !astOK ) {
      addi( "! %ld\n", astStatus );
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
   AstTable *t;
   int i;
   out[ 0 ] = 0;
   t = astTable( " " );
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

int main( void ) {
   int status = 0;
   astWatch( &status );

   printf( "// Generated by tools/oracle_fixtures/table_probe.c. Do not edit.\n\n" );
   printf( "/// `(name, operations, transcript)`: see the probe for the operations\n"
           "/// and the transcript format.\n" );
   printf( "pub type Case = (&'static str, &'static [&'static str], &'static str);\n\n" );
   printf( "pub const CASES: &[Case] = &[\n" );

   CASE( "empty", "GA|NRow", "GA|NColumn", "GA|NParameter", "MS", "D" );
   CASE( "no_column", "P0I|Fred|123|com 1", "P0I|Fred(2)|123|com 1", "G0I|Fred(2)",
         "D" );
   CASE( "add_column", "AC|Fred|F|5,2| ", "GA|NColumn", "CN|1", "GA|ColumnType(Fred)",
         "CS|Fred|2", "CS|Fred|4", "GA|ColumnUnit(Fred)", "GA|ColumnNdim(Fred)",
         "GA|ColumnLength(fred)", "HC|FRED", "HC|fred", "HC|dodo", "D" );
   CASE( "add_column_errors", "AC||I||", "AC| |I||",
         "AC|ABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFGHIJKLMNOPQRSTUVWXYZ|I||",
         "AC|A|99||", "AC|A|0||", "AC|B|I|3,0|", "AC|C|U||", "D" );
   CASE( "add_column_again", "AC|Fred|F|5,2|W", "AC|Fred|F|5,2|W", "AC|fred|I|5,2|W",
         "AC|Fred|F|5,2|pW", "AC|Fred|F|5|W", "AC|Fred|F|5,3|W", "AC|Fred|F||W",
         "GA|NColumn", "D" );
   CASE( "scalar_puts", "AC|Fred|F||pW", "P0I|Fred(2)|123|com 1", "P0F|Fred(1)|-123|com 1",
         "P0F|Fred(2)|123|com 2", "G0D|Fred(2)", "G0I|Fred(2)", "G0C|Fred(2)",
         "G0C|Fred(3)", "G0C|fred(1)", "GA|NRow", "GA|ColumnLenC(Fred)", "MS",
         "MK|0", "MK|1", "D" );
   CASE( "vector_column_scalar_put", "AC|Fred|F|5,2| ", "P0F|Fred(2)|123|com 1",
         "P1I|Fred(1)|1,2", "GA|NRow", "D" );
   CASE( "vector_puts", "AC|Dick|C|2,2| ", "P1C|Dick(4)|One,two,three,FouR",
         "G1C|Dick(4)", "PEC|Dick(4)|3|OHOHOHOH", "GEC|Dick(4)|3", "GA|ColumnLenC(Dick)",
         "P1C|Dick(2)|a,b,c", "PEC|Dick(1)|4|x", "PEC|Dick(1)|1|y", "G1C|Dick(1)",
         "PEI|Dick(1)|0|3", "GA|NRow", "D" );
   CASE( "bytes_and_kint", "AC|HeHe|B|1,2| ", "P1B|HeHe(2)|127,255", "AC|GoGo|B|| ",
         "P0B|GoGo(2)|246", "AC|FredK|K|2| ", "P1K|FredK(1)|1,-1", "G1I|FredK(1)",
         "G1I|HeHe(2)", "D" );
   CASE( "object_column", "AC|Dick|A|| W/m**2", "P0A|Dick(1)", "G0A|Dick(1)",
         "P0I|Dick(2)|1", "D" );
   CASE( "remove_column", "AC|Fred|F||pW", "AC|Dick|A||W/m**2", "P0F|Fred(1)|-123",
         "P0F|Fred(2)|123", "P0A|Dick(3)", "CN|1", "CN|2", "CN|3", "CN|0", "RC|Dick",
         "GA|NRow", "GA|NColumn", "RC|", "RC|nothing", "MS", "D" );
   CASE( "remove_row", "AC|Fred|F||", "P0F|Fred(1)|-123", "P0F|Fred(2)|123", "RR|3",
         "GA|NRow", "RR|1", "GA|NRow", "RR|2", "GA|NRow", "RR|0", "G0C|Fred(2)", "D" );
   CASE( "parameters", "AP|COLOUR", "HP|colour", "P0C|COLOUR|Red", "G0C|colour",
         "AP|Colour", "GA|NParameter", "PN|1", "PN|2", "AP|", "AP|size", "P1I|size|1,2",
         "G1I|size", "PU|undef", "D", "RP|colour", "HP|COLOUR", "G0C|COLOUR", "RP|",
         "D" );
   CASE( "purge", "AC|Fred|I||", "AC|Tom|I||", "P0I|Fred(2)|123", "P0I|Fred(4)|456",
         "P0I|Tom(1)|-123", "P0I|Tom(2)|-456", "P0I|Tom(6)|0", "GA|NRow", "MR|Tom(6)",
         "GA|NRow", "PR", "GA|NRow", "G0I|Tom(1)", "G0I|Tom(2)", "G0I|Fred(1)",
         "G0I|Fred(2)", "G0I|Fred(3)", "D" );
   CASE( "purge_nothing", "AC|Fred|I||", "P0I|Fred(1)|1", "P0I|Fred(2)|2", "PR",
         "GA|NRow", "D" );
   CASE( "purge_with_parameter", "AC|Fred|I||", "AP|P", "P0I|P|5", "P0I|Fred(2)|2",
         "PR", "GA|NRow", "D" );
   CASE( "keys", "AC|Fred|I||", "P0I|Fred (2)|1", "P0I|Fred( 3 )|2", "P0I|Fred(3) |3",
         "P0I|Fred(-1)|4", "P0I|Fred(0)|4", "P0I|Fred(2x)|4", "P0I|Fred()|4",
         "P0I|(2)|4", "P0I|Fred(2)(3)|4", "P0I|Freddy(2)|4", "MS", "MK|0", "MK|1",
         "MK|2", "D" );
   CASE( "key_error", "AC|Fred|I||", "SA|KeyError=1", "G0I|Fred(2)", "G0I|nothing",
         "G0I|Tom(1)", "P0I|Fred(1)|1", "G0I|Fred(1)", "G0I|Fred(2)", "D" );
   CASE( "key_case", "SA|KeyCase=1", "GA|KeyCase", "TA|KeyCase", "CA|KeyCase",
         "TA|KeyCase", "AC|Fred|I||", "P0I|fred(1)|1", "MK|0", "D" );
   CASE( "no_such_column", "AC|Fred|I||", "GA|ColumnType(nothing)", "GA|ColumnUnit(nothing)",
         "GA|ColumnLength(nothing)", "GA|ColumnNdim(nothing)", "GA|ColumnLenC(nothing)" );
   CASE( "map_remove_rename", "AC|Fred|I||", "P0I|Fred(1)|1", "P0I|Fred(3)|3",
         "MR|Fred(3)", "GA|NRow", "MN|Fred(1)|Fred(2)", "G0I|Fred(2)", "MN|Fred(2)|Tom",
         "MS", "MK|0", "D" );
   CASE( "length_cache", "AC|Fred|I|3|", "D", "GA|ColumnLength(Fred)", "D" );
   CASE( "length_cache_put", "AC|Fred|I|3|", "P1I|Fred(1)|1,2,3", "D" );
   CASE( "copy_and_read", "AC|Fred|D|2|m", "AP|P", "P0C|P|x", "P1D|Fred(1)|1.5,2.5",
         "P1D|Fred(3)|-1,1e300", "CP", "G1D|Fred(3)", "D", "RW", "GA|NRow",
         "GA|ColumnLength(Fred)", "G1D|Fred(1)", "D" );
   printf( "];\n" );
   return 0;
}
