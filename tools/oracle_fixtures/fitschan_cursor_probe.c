/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* fitschan_cursor_probe -- capture how a FitsChan's card functions use and
 * move the current card: astPutFits, astPutCards, astFindFits, astDelFits,
 * astEmptyFits, the astGetFits<X> and astSetFits<X> families, astRead and
 * astWrite, and the Card, CardName, CardType, CardComm, Ncard and Nkey
 * attributes.
 *
 * Each case is a script of operations, one string each, fields separated
 * by '|':
 *
 *   P|ow|card            astPutFits
 *   PC|c1~c2~...         astPutCards on c1, c2, ... each padded to 80
 *                        characters and concatenated, the last unpadded
 *   SC|n  CC  TC         set, clear and test Card
 *   F|inc|template       astFindFits
 *   D  E                 astDelFits, astEmptyFits
 *   GI|name GK|name GF|name GS|name GL|name GCN|name GCF|name GCI|name
 *                        astGetFits<X>, a name of "-" meaning NULL (the
 *                        current card)
 *   SI|ow|name|value|comment (also SK, SF, SS, SL, SCN)  astSetFits<X>
 *   SCF|ow|name|re|im|comment  SCI|ow|name|re|im|comment
 *   SU|ow|name|comment   astSetFitsU
 *   CM|ow|comment        astSetFitsCM
 *   ENC|encoding         set Encoding
 *   QE                   get Encoding (guessed unless set)
 *   CL|0 or 1            set Clean
 *   PW  RF               astPurgeWCS, astRetainFits
 *   TF|name              astTestFits, a name of "-" meaning NULL
 *   QCD                  get and test CDMatrix
 *   FD|n                 set FitsDigits
 *   AI|attr  AD|attr     get an int or double attribute, and test it
 *   ASI|attr|v  ASD|attr|v  set an int or double attribute
 *   AX|attr              clear an attribute
 *   RW                   astRead, then astWrite of what was read
 *   SCD|0 or 1  XCD      set and clear CDMatrix
 *   R                    astRead
 *   RN                   astRead, then the native dump of what was read
 *   RSW|warnings         astRead, set Warnings, then astWrite of what was read
 *   RCIW|n               astRead, clear the Ident of Frame n, then astWrite
 *   AC|attr  ASC|attr|v  get and test, or set, a string attribute
 *   WN                   the Channel's warnings (astWarnings)
 *   NEW|c1~c2~#~c3       (first operation only) create the FitsChan with a
 *                        source function giving c1, c2, then NULL at '#',
 *                        then c3 and NULL; and a sink function, whose
 *                        lines are printed as "|sink card"
 *   RDF  WRF             astReadFits, astWriteFits
 *   MKF|file|l1~l2       write the lines l1, l2 to a file in the temporary
 *                        directory
 *   SRF|file  SKF|file   set SourceFile or SinkFile to a file there
 *   CATF|file            print the file's lines as "|file line"
 *   CPN                  astCopy, then the copy's Ncard, then annul it
 *
 * After the last operation the FitsChan is annulled, and anything it
 * writes to the sink or to the SinkFile is printed.
 *   W                    astWrite of a 2-D ZoomMap of factor 2
 *   WF                   astWrite of a 2-D Frame
 *   WFT|title            astWrite of a 2-D Frame with that Title
 *   WC                   astWrite of a series CmpMap of 2-D ZoomMaps of
 *                        factor 2 and 3
 *   WCF                  astWrite of a CmpFrame of two 1-D Frames
 *
 * After each operation it prints the result and the state: Card, Ncard,
 * Nkey, CardType, CardName and CardComm, then every card astFindFits
 * returns from the start, and the error if one stops it, as Rust source for
 * crates/starlink-ast/tests/fitschan_cursor_oracle/cases.rs:
 *
 *   ./fitschan_cursor_probe > ../../crates/starlink-ast/tests/fitschan_cursor_oracle/cases.rs
 */
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include "ast.h"
#include "ast_err.h"

static char out[ 1 << 20 ];
static size_t nout;

static void add( const char *s ) {
   for( ; *s && nout < sizeof( out ) - 1; s++ ) out[ nout++ ] = *s;
   out[ nout ] = 0;
}

/* The source function's text, '~' between cards and '#' ending a batch. */
static const char *src_pos;
static char src_card[ 200 ];

static const char *source( void ) {
   size_t n;
   if( !src_pos || !*src_pos ) return NULL;
   if( *src_pos == '#' ) {
      src_pos++;
      if( *src_pos == '~' ) src_pos++;
      return NULL;
   }
   n = strcspn( src_pos, "~" );
   memcpy( src_card, src_pos, n );
   src_card[ n ] = 0;
   src_pos += n;
   if( *src_pos ) src_pos++;
   return src_card;
}

static void sink( const char *card ) {
   add( "|sink " );
   add( card );
   add( "\n" );
}

/* The path of `name` in the temporary directory. */
static const char *tmp_path( const char *name ) {
   static char path[ 1000 ];
   const char *dir = getenv( "TMPDIR" );
   snprintf( path, sizeof path, "%s/%s", dir && *dir ? dir : "/tmp", name );
   return path;
}

static char sink_name[ 400 ];

/* Print each line of the temporary file `name` as "|file line". */
static void cat_file( const char *name ) {
   char line[ 400 ];
   FILE *fd = fopen( tmp_path( name ), "r" );
   if( !fd ) {
      add( "|nofile\n" );
      return;
   }
   while( fgets( line, sizeof line, fd ) ) {
      line[ strcspn( line, "\n" ) ] = 0;
      add( "|file " );
      add( line );
      add( "\n" );
   }
   fclose( fd );
}

static void dump_sink( const char *line ) {
   add( "|" );
   add( line );
   add( "\n" );
}

static const char *code_name( int status ) {
   if( status == AST__BDFTS ) return "Bdfts";
   if( status == AST__FTCNV ) return "Ftcnv";
   if( status == AST__FUNDEF ) return "Fundef";
   if( status == AST__BDFMT ) return "Bdfmt";
   if( status == AST__ATTIN ) return "Attin";
   if( status == AST__RDERR ) return "Rderr";
   if( status == AST__WRERR ) return "Wrerr";
   return "Other";
}

static void state( AstFitsChan *fc ) {
   char buf[ 300 ], card[ 81 ];
   const char *name;
   int icard = astGetI( fc, "Card" );
   name = astGetC( fc, "CardName" );
   snprintf( buf, sizeof buf, "card=%d ncard=%d nkey=%d type=%d name=%s comm=[%s]\n",
             icard, astGetI( fc, "Ncard" ), astGetI( fc, "Nkey" ),
             astGetI( fc, "CardType" ), name ? name : "(null)",
             astGetC( fc, "CardComm" ) );
   add( buf );
   astClear( fc, "Card" );
   while( astFindFits( fc, "%f", card, 1 ) ) {
      add( "|" );
      add( card );
      add( "\n" );
   }
   if( !astOK ) {
      add( "|err " );
      add( code_name( astStatus ) );
      add( "\n" );
      astClearStatus;
   }
   astSetI( fc, "Card", icard );
}

/* Copy field `i` (0-based) of `op`, fields separated by '|', into `buf`. */
static const char *field( const char *op, int i, char *buf ) {
   const char *a = op;
   int n;
   while( i-- > 0 ) {
      a = strchr( a, '|' );
      if( !a ) { buf[ 0 ] = 0; return buf; }
      a++;
   }
   n = strcspn( a, "|" );
   memcpy( buf, a, n );
   buf[ n ] = 0;
   return buf;
}

static void run( AstFitsChan *fc, const char *op, int *status ) {
   char f0[ 400 ], f1[ 400 ], f2[ 400 ], f3[ 400 ], f4[ 400 ], buf[ 4000 ];
   char card[ 81 ];
   field( op, 0, f0 );
   field( op, 1, f1 );
   field( op, 2, f2 );
   field( op, 3, f3 );
   field( op, 4, f4 );
   add( "> " ); add( op ); add( "\n" );
   buf[ 0 ] = 0;
   if( !strcmp( f0, "P" ) ) {
      astPutFits( fc, f2, atoi( f1 ) );
   } else if( !strcmp( f0, "PC" ) ) {
      char cards[ 4000 ];
      const char *a = op + 3;
      size_t n = 0;
      cards[ 0 ] = 0;
      while( 1 ) {
         size_t len = strcspn( a, "~" );
         memcpy( cards + n, a, len );
         n += len;
         if( !a[ len ] ) break;
         while( n % 80 ) cards[ n++ ] = ' ';
         a += len + 1;
      }
      cards[ n ] = 0;
      astPutCards( fc, cards );
   } else if( !strcmp( f0, "SC" ) ) {
      astSetI( fc, "Card", atoi( f1 ) );
   } else if( !strcmp( f0, "CC" ) ) {
      astClear( fc, "Card" );
   } else if( !strcmp( f0, "TC" ) ) {
      snprintf( buf, sizeof buf, "= %d\n", astTest( fc, "Card" ) );
   } else if( !strcmp( f0, "F" ) ) {
      if( astFindFits( fc, f2, card, atoi( f1 ) ) ) {
         snprintf( buf, sizeof buf, "= found [%s]\n", card );
      } else {
         snprintf( buf, sizeof buf, "= nf\n" );
      }
   } else if( !strcmp( f0, "D" ) ) {
      astDelFits( fc );
   } else if( !strcmp( f0, "E" ) ) {
      astEmptyFits( fc );
   } else if( f0[ 0 ] == 'G' ) {
      const char *name = strcmp( f1, "-" ) ? f1 : NULL;
      int iv, ci[ 2 ], ok = 0;
      int64_t kv;
      double dv, cf[ 2 ];
      char *sv;
      if( !strcmp( f0, "GI" ) ) {
         if( ( ok = astGetFitsI( fc, name, &iv ) ) ) snprintf( buf, sizeof buf, "= %d\n", iv );
      } else if( !strcmp( f0, "GK" ) ) {
         if( ( ok = astGetFitsK( fc, name, &kv ) ) ) snprintf( buf, sizeof buf, "= %lld\n", (long long) kv );
      } else if( !strcmp( f0, "GF" ) ) {
         if( ( ok = astGetFitsF( fc, name, &dv ) ) ) snprintf( buf, sizeof buf, "= %.17g\n", dv );
      } else if( !strcmp( f0, "GS" ) ) {
         if( ( ok = astGetFitsS( fc, name, &sv ) ) ) snprintf( buf, sizeof buf, "= [%s]\n", sv );
      } else if( !strcmp( f0, "GCN" ) ) {
         if( ( ok = astGetFitsCN( fc, name, &sv ) ) ) snprintf( buf, sizeof buf, "= [%s]\n", sv );
      } else if( !strcmp( f0, "GL" ) ) {
         if( ( ok = astGetFitsL( fc, name, &iv ) ) ) snprintf( buf, sizeof buf, "= %d\n", iv ? 1 : 0 );
      } else if( !strcmp( f0, "GCF" ) ) {
         if( ( ok = astGetFitsCF( fc, name, cf ) ) ) snprintf( buf, sizeof buf, "= %.17g %.17g\n", cf[ 0 ], cf[ 1 ] );
      } else if( !strcmp( f0, "GCI" ) ) {
         if( ( ok = astGetFitsCI( fc, name, ci ) ) ) snprintf( buf, sizeof buf, "= %d %d\n", ci[ 0 ], ci[ 1 ] );
      } else {
         printf( "unknown op %s\n", op );
         exit( 1 );
      }
      if( !ok ) snprintf( buf, sizeof buf, "= nf\n" );
   } else if( !strcmp( f0, "SI" ) ) {
      astSetFitsI( fc, f2, atoi( f3 ), f4, atoi( f1 ) );
   } else if( !strcmp( f0, "SK" ) ) {
      astSetFitsK( fc, f2, (int64_t) atoll( f3 ), f4, atoi( f1 ) );
   } else if( !strcmp( f0, "SCN" ) ) {
      astSetFitsCN( fc, f2, f3, f4, atoi( f1 ) );
   } else if( !strcmp( f0, "SCF" ) ) {
      char f5[ 400 ];
      double v[ 2 ];
      v[ 0 ] = atof( f3 );
      v[ 1 ] = atof( f4 );
      astSetFitsCF( fc, f2, v, field( op, 5, f5 ), atoi( f1 ) );
   } else if( !strcmp( f0, "SCI" ) ) {
      char f5[ 400 ];
      int v[ 2 ];
      v[ 0 ] = atoi( f3 );
      v[ 1 ] = atoi( f4 );
      astSetFitsCI( fc, f2, v, field( op, 5, f5 ), atoi( f1 ) );
   } else if( !strcmp( f0, "SF" ) ) {
      astSetFitsF( fc, f2, atof( f3 ), f4, atoi( f1 ) );
   } else if( !strcmp( f0, "SS" ) ) {
      astSetFitsS( fc, f2, f3, f4, atoi( f1 ) );
   } else if( !strcmp( f0, "SL" ) ) {
      astSetFitsL( fc, f2, atoi( f3 ), f4, atoi( f1 ) );
   } else if( !strcmp( f0, "SU" ) ) {
      astSetFitsU( fc, f2, f3, atoi( f1 ) );
   } else if( !strcmp( f0, "CM" ) ) {
      astSetFitsCM( fc, f2, atoi( f1 ) );
   } else if( !strcmp( f0, "ENC" ) ) {
      astSetC( fc, "Encoding", f1 );
   } else if( !strcmp( f0, "CL" ) ) {
      astSetI( fc, "Clean", atoi( f1 ) );
   } else if( !strcmp( f0, "PW" ) ) {
      astPurgeWCS( fc );
   } else if( !strcmp( f0, "RF" ) ) {
      astRetainFits( fc );
   } else if( !strcmp( f0, "TF" ) ) {
      int there;
      int has = astTestFits( fc, strcmp( f1, "-" ) ? f1 : NULL, &there );
      snprintf( buf, sizeof buf, "= %s\n", !there ? "nf" : has ? "1" : "0" );
   } else if( !strcmp( f0, "AI" ) ) {
      snprintf( buf, sizeof buf, "= %d %d\n", astGetI( fc, f1 ), astTest( fc, f1 ) );
   } else if( !strcmp( f0, "AD" ) ) {
      snprintf( buf, sizeof buf, "= %.17g %d\n", astGetD( fc, f1 ), astTest( fc, f1 ) );
   } else if( !strcmp( f0, "AC" ) ) {
      snprintf( buf, sizeof buf, "= [%s] %d\n", astGetC( fc, f1 ),
                strcmp( f1, "AllWarnings" ) ? astTest( fc, f1 ) : 0 );
   } else if( !strcmp( f0, "ASC" ) ) {
      astSetC( fc, f1, f2 );
   } else if( !strcmp( f0, "WN" ) ) {
      AstKeyMap *km = astWarnings( fc );
      int i, n = km ? astMapSize( km ) : 0;
      snprintf( buf, sizeof buf, "= %d\n", n );
      for( i = 0; i < n; i++ ) {
         char key[ 20 ];
         const char *text;
         sprintf( key, "Warning_%d", i + 1 );
         if( astMapGet0C( km, key, &text ) ) {
            strcat( buf, "|" );
            strncat( buf, text, sizeof buf - strlen( buf ) - 3 );
            strcat( buf, "\n" );
         }
      }
      if( km ) km = astAnnul( km );
   } else if( !strcmp( f0, "ASI" ) ) {
      astSetI( fc, f1, atoi( f2 ) );
   } else if( !strcmp( f0, "ASD" ) ) {
      astSetD( fc, f1, atof( f2 ) );
   } else if( !strcmp( f0, "AX" ) ) {
      astClear( fc, f1 );
   } else if( !strcmp( f0, "RN" ) ) {
      AstObject *obj = astRead( fc );
      snprintf( buf, sizeof buf, "= %s\n", obj ? astGetC( obj, "Class" ) : "null" );
      add( buf );
      buf[ 0 ] = 0;
      if( obj ) {
         AstChannel *ch = astChannel( NULL, dump_sink, " " );
         astWrite( ch, obj );
         ch = astAnnul( ch );
         obj = astAnnul( obj );
      }
   } else if( !strcmp( f0, "RCIW" ) ) {
      AstObject *obj = astRead( fc );
      if( obj ) {
         AstFrame *frm = astGetFrame( (AstFrameSet *) obj, atoi( f1 ) );
         astClear( frm, "Ident" );
         frm = astAnnul( frm );
      }
      snprintf( buf, sizeof buf, "= %s %d\n", obj ? astGetC( obj, "Class" ) : "null",
                obj ? astWrite( fc, obj ) : 0 );
      if( obj ) obj = astAnnul( obj );
   } else if( !strcmp( f0, "RSW" ) ) {
      AstObject *obj = astRead( fc );
      astSetC( fc, "Warnings", f1 );
      snprintf( buf, sizeof buf, "= %s %d\n", obj ? astGetC( obj, "Class" ) : "null",
                obj ? astWrite( fc, obj ) : 0 );
      if( obj ) obj = astAnnul( obj );
   } else if( !strcmp( f0, "RW" ) ) {
      AstObject *obj = astRead( fc );
      snprintf( buf, sizeof buf, "= %s %d\n", obj ? astGetC( obj, "Class" ) : "null",
                obj ? astWrite( fc, obj ) : 0 );
      if( obj ) obj = astAnnul( obj );
   } else if( !strcmp( f0, "FD" ) ) {
      astSetI( fc, "FitsDigits", atoi( f1 ) );
   } else if( !strcmp( f0, "QCD" ) ) {
      snprintf( buf, sizeof buf, "= %d %d\n", astGetI( fc, "CDMatrix" ),
                astTest( fc, "CDMatrix" ) );
   } else if( !strcmp( f0, "SCD" ) ) {
      astSetI( fc, "CDMatrix", atoi( f1 ) );
   } else if( !strcmp( f0, "XCD" ) ) {
      astClear( fc, "CDMatrix" );
   } else if( !strcmp( f0, "QE" ) ) {
      snprintf( buf, sizeof buf, "= %s\n", astGetC( fc, "Encoding" ) );
   } else if( !strcmp( f0, "RDF" ) ) {
      astReadFits( fc );
   } else if( !strcmp( f0, "WRF" ) ) {
      astWriteFits( fc );
   } else if( !strcmp( f0, "MKF" ) ) {
      FILE *fd = fopen( tmp_path( f1 ), "w" );
      const char *a = op + 5 + strlen( f1 );
      for( ; *a; a++ ) fputc( *a == '~' ? '\n' : *a, fd );
      fclose( fd );
   } else if( !strcmp( f0, "SRF" ) ) {
      astSetC( fc, "SourceFile", tmp_path( f1 ) );
   } else if( !strcmp( f0, "SKF" ) ) {
      astSetC( fc, "SinkFile", tmp_path( f1 ) );
      strcpy( sink_name, f1 );
   } else if( !strcmp( f0, "CATF" ) ) {
      cat_file( f1 );
   } else if( !strcmp( f0, "CPN" ) ) {
      AstFitsChan *copy = astCopy( fc );
      snprintf( buf, sizeof buf, "= %d\n", astGetI( copy, "Ncard" ) );
      copy = astAnnul( copy );
   } else if( !strcmp( f0, "R" ) ) {
      AstObject *obj = astRead( fc );
      snprintf( buf, sizeof buf, "= %s\n", obj ? astGetC( obj, "Class" ) : "null" );
      if( obj ) obj = astAnnul( obj );
   } else if( !strcmp( f0, "WF" ) ) {
      AstFrame *fr = astFrame( 2, " " );
      snprintf( buf, sizeof buf, "= %d\n", astWrite( fc, fr ) );
      fr = astAnnul( fr );
   } else if( !strcmp( f0, "WFT" ) ) {
      AstFrame *fr = astFrame( 2, " " );
      astSetC( fr, "Title", f1 );
      snprintf( buf, sizeof buf, "= %d\n", astWrite( fc, fr ) );
      fr = astAnnul( fr );
   } else if( !strcmp( f0, "WC" ) ) {
      AstZoomMap *z1 = astZoomMap( 2, 2.0, " " );
      AstZoomMap *z2 = astZoomMap( 2, 3.0, " " );
      AstCmpMap *cm = astCmpMap( z1, z2, 1, " " );
      snprintf( buf, sizeof buf, "= %d\n", astWrite( fc, cm ) );
      cm = astAnnul( cm );
      z1 = astAnnul( z1 );
      z2 = astAnnul( z2 );
   } else if( !strcmp( f0, "WCF" ) ) {
      AstFrame *f1 = astFrame( 1, " " );
      AstFrame *f2 = astFrame( 1, " " );
      AstCmpFrame *cf = astCmpFrame( f1, f2, " " );
      snprintf( buf, sizeof buf, "= %d\n", astWrite( fc, cf ) );
      cf = astAnnul( cf );
      f1 = astAnnul( f1 );
      f2 = astAnnul( f2 );
   } else if( !strcmp( f0, "W" ) ) {
      AstZoomMap *zm = astZoomMap( 2, 2.0, " " );
      snprintf( buf, sizeof buf, "= %d\n", astWrite( fc, zm ) );
      zm = astAnnul( zm );
   } else {
      printf( "unknown op %s\n", op );
      exit( 1 );
   }
   if( !astOK ) {
      snprintf( buf, sizeof buf, "= err %s\n", code_name( astStatus ) );
      astClearStatus;
   }
   add( buf );
   state( fc );
}

static void rust_str( const char *s ) {
   putchar( '"' );
   for( ; *s; s++ ) {
      if( *s == '"' || *s == '\\' ) putchar( '\\' );
      if( *s == '\n' ) { printf( "\\n" ); continue; }
      if( *s == '\r' ) { printf( "\\r" ); continue; }
      putchar( *s );
   }
   putchar( '"' );
}

static void emit( const char *name, const char **ops, int *status ) {
   AstFitsChan *fc;
   int i = 0;
   nout = 0;
   out[ 0 ] = 0;
   sink_name[ 0 ] = 0;
   if( !strncmp( ops[ 0 ], "NEW|", 4 ) ) {
      src_pos = ops[ 0 ] + 4;
      fc = astFitsChan( source, sink, " " );
      i = 1;
   } else {
      fc = astFitsChan( NULL, NULL, " " );
   }
   for( ; ops[ i ]; i++ ) run( fc, ops[ i ], status );
   add( "> annul\n" );
   fc = astAnnul( fc );
   if( !astOK ) {
      add( "= err " );
      add( code_name( astStatus ) );
      add( "\n" );
      astClearStatus;
   }
   if( sink_name[ 0 ] ) {
      cat_file( sink_name );
      remove( tmp_path( sink_name ) );
   }
   printf( "    (\n        \"%s\",\n        &[\n", name );
   for( i = 0; ops[ i ]; i++ ) {
      printf( "            " );
      rust_str( ops[ i ] );
      printf( ",\n" );
   }
   printf( "        ],\n        " );
   rust_str( out );
   printf( ",\n    ),\n" );
}

#define CASE(name, ...) { static const char *ops[] = { __VA_ARGS__, NULL }; emit( name, ops, &status ); }

int main( void ) {
   int status = 0;
   astWatch( &status );
   printf( "// Generated by tools/oracle_fixtures/fitschan_cursor_probe.c. Do not edit.\n\n"
           "/// `(case, the operations, what C prints after each)`.\n"
           "pub const CASES: &[(&str, &[&str], &str)] = &[\n" );

   CASE( "insert_and_overwrite",
         "P|0|A       = 1", "P|0|B       = 2", "P|0|C       = 3", "CC", "SC|2",
         "P|0|X       = 9", "P|1|Y       = 8 / cb", "P|1|A       = 7 / cc",
         "P|1|Z       = 6" );
   CASE( "overwrite_keeps_comment",
         "P|0|A       = 1 / orig", "CC", "SI|1|A|2|", "CC", "SI|1|Q|3|",
         "CC", "SF|1|Q|2.5|new", "CC", "SU|1|U|", "CC", "SL|1|L|1|",
         "CC", "SS|1|S|text|" );
   CASE( "set_at_eof_appends",
         "SI|1|A|1|one", "SI|1|B|2|", "SI|0|C|3|three", "SC|1", "SI|0|D|4|" );
   CASE( "put_cards_replaces",
         "P|0|ZZZ     = 0",
         "PC|AAA     = 1~BBB     = 2~CCC     = 'x'" );
   CASE( "put_cards_short_last",
         "PC|AAA     = 1~BBB     = 2" );
   CASE( "find_templates",
         "P|0|A1      = 1", "P|0|B       = 2", "P|0|A2      = 3",
         "P|0|CRVAL1  = 4", "CC", "F|0|A%d", "F|1|A%d", "F|0|A%d", "CC",
         "F|0|CRVAL%1d", "CC", "F|0|%f", "F|0|%0f", "F|0|B   ", "SC|3",
         "F|0|B", "F|0|%f", "CC", "F|1|%2d", "CC", "F|0|%1c%d" );
   CASE( "find_inc_steps",
         "P|0|A       = 1", "P|0|B       = 2", "P|0|A       = 3", "CC",
         "F|1|A", "F|1|A", "F|1|A" );
   CASE( "delete",
         "P|0|A       = 1", "P|0|B       = 2", "P|0|C       = 3", "SC|2", "D",
         "D", "D", "D", "SC|1", "D" );
   CASE( "set_card_range",
         "P|0|A       = 1", "P|0|B       = 2", "SC|0", "SC|-3", "SC|5",
         "SC|2", "TC", "CC", "TC" );
   CASE( "getters_move_cursor",
         "P|0|DUP     = 1", "P|0|X       = 2", "P|0|DUP     = 3",
         "P|0|Y       = 4.5", "CC", "GI|DUP", "GI|DUP", "GI|DUP", "GI|NONE",
         "SC|2", "GI|DUP", "SC|1", "GF|Y", "GF|DUP", "CC", "GI|DU%1d" );
   CASE( "card_queries",
         "P|0|        blank keyword text", "P|0|COMMENT   a comment",
         "P|0|HISTORY history text", "P|0|S       = 'abc' / string comment",
         "P|0|I       = 1", "P|0|K       = 0000000005",
         "P|0|K2      = 12345678901", "P|0|F       = 1.5", "P|0|L       = T",
         "P|0|U       =", "P|0|CF      = 1.5 2.5", "P|0|CI      = 1 2",
         "P|0|CONTINUE  'more'", "P|0|NOEQUALS  text", "SC|1", "SC|2",
         "SC|3", "SC|4", "SC|5", "SC|6", "SC|7", "SC|8", "SC|9", "SC|10",
         "SC|11", "SC|12", "SC|13", "SC|14", "SC|15" );
   CASE( "comment_setter",
         "P|0|A       = 1", "CM|0|a comment card", "CM|0|", "CC",
         "CM|1|replaces A" );
   CASE( "setter_names",
         "SI|0|averylongname|5|", "SI|0|lower|6|given", "SI|0|AB / x|7|" );
   CASE( "empty",
         "P|0|A       = 1", "P|0|B       = 2", "E", "P|0|C       = 3" );
   CASE( "read_at_start_only",
         "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200",
         "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10",
         "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'",
         "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001",
         "P|0|CD2_2   = 0.001", "P|0|END", "SC|4", "R", "CC", "R",
         "P|0|MARK    = 1", "R" );
   CASE( "read_failure_rewinds",
         "P|0|NAXIS   = 2", "P|0|CTYPE1  = 'RA---TAN'", "SC|2", "ENC|FITS-WCS",
         "R" );
   CASE( "write_inserts_before_current",
         "P|0|A       = 1", "P|0|B       = 2", "P|0|C       = 3", "SC|2", "W",
         "P|0|MARK    = 1" );

   {
      static const char *types[] = { "GI", "GK", "GF", "GS", "GCN", "GL", "GCF", "GCI" };
      static const char *setup[] = {
         "P|0|I       = 7", "P|0|IN      = -3", "P|0|K       = 0000000005",
         "P|0|KB      = 12345678901", "P|0|F       = 2.7", "P|0|FN      = -2.5",
         "P|0|FZ      = -0.0", "P|0|FR      = 0.1", "P|0|S       = '1'",
         "P|0|SF      = '2.5'", "P|0|SN      = 'abc'", "P|0|SY      = 'yes'",
         "P|0|SFT     = ' .true.'", "P|0|S2      = '1.5 2.5'",
         "P|0|SI2     = '3 4'", "P|0|SP      = 'ab  '", "P|0|L       = T",
         "P|0|LF      = F", "P|0|U       =", "P|0|CF      = 1.5 2.5",
         "P|0|CI      = 1 2", "P|0|CONTINUE  'more'", "P|0|COMMENT   text",
         "P|0|BIG     = '99999999999'", "P|0|SBAD    = '<bad>'", NULL };
      static const char *names[] = { "I", "IN", "K", "KB", "F", "FN", "FZ",
         "FR", "S", "SF", "SN", "SY", "SFT", "S2", "SI2", "SP", "L", "LF", "U",
         "CF", "CI", "CONTINUE", "COMMENT", "BIG", "SBAD", "NONE", NULL };
      static char opbuf[ 300 ][ 40 ];
      static const char *ops[ 300 ];
      int n = 0, i, t;
      for( i = 0; setup[ i ]; i++ ) ops[ n++ ] = setup[ i ];
      for( i = 0; names[ i ]; i++ ) {
         for( t = 0; t < 8; t++ ) {
            snprintf( opbuf[ n - 25 ], sizeof opbuf[ 0 ], "%s|%s", types[ t ], names[ i ] );
            ops[ n ] = opbuf[ n - 25 ];
            n++;
         }
      }
      ops[ n ] = NULL;
      emit( "conversions", ops, &status );
   }
   CASE( "guess_cd_radecsys_radesys",
         "P|0|CTYPE1  = 'RA---TAN'", "P|0|CRVAL1  = 1", "P|0|CD1_1   = 1",
         "P|0|RADECSYS= 'FK5'", "P|0|RADESYS = 'FK5'", "QE" );
   CASE( "guess_cd_cmval",
         "P|0|CTYPE1  = 'RA---TAN'", "P|0|CD1_1   = 1", "P|0|C1VAL1  = 1", "QE" );
   CASE( "guess_axis2_only",
         "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CRVAL2  = 2", "QE" );
   CASE( "guess_alternate_only",
         "P|0|CRVAL1A = 1", "QE" );
   CASE( "guess_aips_lsrk",
         "P|0|CTYPE1  = 'VELOLSRK'", "P|0|CRVAL1  = 1", "QE" );
   CASE( "guess_projp_and_pv",
         "P|0|CTYPE1  = 'RA---TAN'", "P|0|CRVAL1  = 1", "P|0|PROJP1  = 1",
         "P|0|PV1_1   = 1", "QE" );
   CASE( "guess_cmpix_only",
         "P|0|CTYPE1  = 'RA---TAN'", "P|0|CRVAL1  = 1", "P|0|C2PIX1  = 1", "QE" );
   CASE( "guess_comment_markers",
         "P|0|COMMENT AST +++ AST", "P|0|CTYPE1  = 'RA---TAN'", "QE" );
   CASE( "guess_ctype_crpix_only",
         "P|0|CTYPE1  = 'RA---TAN'", "P|0|CRPIX1  = 1", "QE" );
   CASE( "guess_after_read",
         "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200",
         "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10",
         "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'",
         "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001",
         "P|0|CD2_2   = 0.001", "CC", "QE", "R", "QE", "GF|CRVAL1", "SC|3",
         "W" );
   CASE( "native_two_objects",
         "ENC|NATIVE", "P|0|A       = 1", "W", "P|0|B       = 2", "W",
         "P|0|C       = 3", "CC", "R", "R", "R" );
   CASE( "native_read_from_middle",
         "ENC|NATIVE", "W", "P|0|B       = 2", "W", "SC|20", "R", "CC", "R",
         "R" );
   CASE( "purge_wcs",
         "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|OBJECT  = 'm31'", "P|0|CDELT3  = 2", "P|0|PC3_3   = 1",
         "SC|3", "PW", "P|0|AFTER   = 1" );
   CASE( "retain_survives_read",
         "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "SC|6", "RF", "CC", "R", "CC", "GF|CRVAL1" );
   CASE( "failed_read_without_clean",
         "P|0|NAXIS   = 2", "P|0|CRPIX1  = 50", "P|0|CTYPE1  = 'RA---TAN'",
         "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CRVAL1  =", "P|0|CRVAL2  = 20",
         "ENC|FITS-WCS", "CC", "R" );
   CASE( "failed_read_with_clean",
         "P|0|NAXIS   = 2", "P|0|CRPIX1  = 50", "P|0|CTYPE1  = 'RA---TAN'",
         "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CRVAL1  =", "P|0|CRVAL2  = 20",
         "ENC|FITS-WCS", "CL|1", "CC", "R" );
   CASE( "clean_suppresses_warnings",
         "P|0|BAD%NAME= 1", "CL|1", "P|0|BAD%TWO = 2", "CL|0", "P|0|BAD%3   = 3" );
   CASE( "native_write_after_duplicate_delete",
         "ENC|NATIVE", "P|0|ZOOM_A  = 1", "P|0|ZOOM_A  = 2", "CC", "D", "SC|2", "W" );
   CASE( "test_fits",
         "P|0|A       = 1", "P|0|U       =", "P|0|COMMENT   text", "TF|A", "TF|U",
         "TF|COMMENT", "TF|NONE", "SC|2", "TF|-", "SC|9", "TF|-" );
   CASE( "current_card_values",
         "P|0|A       = 1", "P|0|B       = 'x'", "SC|1", "GI|-", "GS|-",
         "SC|2", "GS|-", "GI|-", "SC|3", "GI|-" );
   CASE( "typed_setters",
         "SK|0|KV|5|small", "SK|0|KB|12345678901|", "SCN|0|CN|long text|c",
         "SCF|0|CFV|1.5|-2.5|cf", "SCI|0|CIV|3|-4|ci", "CC", "GK|KV",
         "GCN|CN", "GCF|CFV", "GCI|CIV" );

   CASE( "split_logicals",
         "P|0|LY      = YES", "P|0|LT      = t", "P|0|LN      = no / x",
         "P|0|LTR     = TrUe", "P|0|LFA     = fa", "P|0|LF      = F",
         "P|0|LYY     = YESS", "P|0|LTE     = T E", "CC", "GL|LY", "GS|LN",
         "GL|LYY" );
   CASE( "split_numbers",
         "P|0|PI      = +5", "P|0|DE      = 1.5D3", "P|0|DL      = -2d-1",
         "P|0|HX      = 0x1.8p3", "P|0|BI      = 123456789012345678901",
         "P|0|BN      = -123456789012345678901", "P|0|CP      = (1.0, 2.0)",
         "P|0|CX      = 1 2 3", "P|0|IN      = inf", "P|0|NE      = 1.5e",
         "P|0|END     = 5", "P|0|lower   = 6 / lc", "CC", "GK|BI", "GK|BN",
         "GF|HX", "GI|LOWER", "F|0|lower" );
   CASE( "split_bad_value",
         "P|0|A       = 1", "P|0|BAD     = abc / bad", "P|0|C       = 3",
         "SC|2", "P|1|BAD2    = 1 2 3 / x", "CL|1", "P|0|BAD3    = xyz" );
   CASE( "put_cards_bad_value",
         "PC|A       = 1~BAD     = abc / bad~C       = 3" );
   CASE( "cd_matrix_default",
         "QCD", "P|0|CD1_2   = 1", "QCD", "CC", "D", "QCD",
         "P|0|CD1_1A  = 1", "QCD", "P|0|CD12_1  = 1", "QCD",
         "P|0|CD2_2   = 1", "SC|2", "QCD", "SCD|0",
         "QCD", "XCD", "QCD", "E", "SCD|1", "QCD" );
   CASE( "cd_matrix_after_read",
         "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200",
         "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10",
         "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'",
         "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001",
         "P|0|CD2_2   = 0.001", "QCD", "CC", "R", "QCD" );
   CASE( "fits_digits_minus5",
         "FD|-5", "SF|0|V0|123456789.0|", "SF|0|V1|0.3333333333333333|", "SF|0|V2|266.41666666666663|", "SF|0|V3|-0.0005|", "SF|0|V4|1e-300|", "SF|0|V5|-1.5e300|", "SF|0|V6|0.0|", "SF|0|V7|-0.0|", "SF|0|V8|12345678901234567890.0|", "SF|0|V9|-123456789012345678.0|", "SF|0|V10|99999.99999999|", "SF|0|V11|1.0|", "SF|0|V12|-7.0|", "SCF|0|CF|1.5|-0.0005|", "SCF|0|CG|-123456789012345678.0|2.0|" );
   CASE( "fits_digits_0",
         "FD|0", "SF|0|V0|123456789.0|", "SF|0|V1|0.3333333333333333|", "SF|0|V2|266.41666666666663|", "SF|0|V3|-0.0005|", "SF|0|V4|1e-300|", "SF|0|V5|-1.5e300|", "SF|0|V6|0.0|", "SF|0|V7|-0.0|", "SF|0|V8|12345678901234567890.0|", "SF|0|V9|-123456789012345678.0|", "SF|0|V10|99999.99999999|", "SF|0|V11|1.0|", "SF|0|V12|-7.0|", "SCF|0|CF|1.5|-0.0005|", "SCF|0|CG|-123456789012345678.0|2.0|" );
   CASE( "fits_digits_1",
         "FD|1", "SF|0|V0|123456789.0|", "SF|0|V1|0.3333333333333333|", "SF|0|V2|266.41666666666663|", "SF|0|V3|-0.0005|", "SF|0|V4|1e-300|", "SF|0|V5|-1.5e300|", "SF|0|V6|0.0|", "SF|0|V7|-0.0|", "SF|0|V8|12345678901234567890.0|", "SF|0|V9|-123456789012345678.0|", "SF|0|V10|99999.99999999|", "SF|0|V11|1.0|", "SF|0|V12|-7.0|", "SCF|0|CF|1.5|-0.0005|", "SCF|0|CG|-123456789012345678.0|2.0|" );
   CASE( "fits_digits_15",
         "FD|15", "SF|0|V0|123456789.0|", "SF|0|V1|0.3333333333333333|", "SF|0|V2|266.41666666666663|", "SF|0|V3|-0.0005|", "SF|0|V4|1e-300|", "SF|0|V5|-1.5e300|", "SF|0|V6|0.0|", "SF|0|V7|-0.0|", "SF|0|V8|12345678901234567890.0|", "SF|0|V9|-123456789012345678.0|", "SF|0|V10|99999.99999999|", "SF|0|V11|1.0|", "SF|0|V12|-7.0|", "SCF|0|CF|1.5|-0.0005|", "SCF|0|CG|-123456789012345678.0|2.0|" );
   CASE( "fits_digits_18",
         "FD|18", "SF|0|V0|123456789.0|", "SF|0|V1|0.3333333333333333|", "SF|0|V2|266.41666666666663|", "SF|0|V3|-0.0005|", "SF|0|V4|1e-300|", "SF|0|V5|-1.5e300|", "SF|0|V6|0.0|", "SF|0|V7|-0.0|", "SF|0|V8|12345678901234567890.0|", "SF|0|V9|-123456789012345678.0|", "SF|0|V10|99999.99999999|", "SF|0|V11|1.0|", "SF|0|V12|-7.0|", "SCF|0|CF|1.5|-0.0005|", "SCF|0|CG|-123456789012345678.0|2.0|" );
   CASE( "fits_digits_25",
         "FD|25", "SF|0|V0|123456789.0|", "SF|0|V1|0.3333333333333333|", "SF|0|V2|266.41666666666663|", "SF|0|V3|-0.0005|", "SF|0|V4|1e-300|", "SF|0|V5|-1.5e300|", "SF|0|V6|0.0|", "SF|0|V7|-0.0|", "SF|0|V8|12345678901234567890.0|", "SF|0|V9|-123456789012345678.0|", "SF|0|V10|99999.99999999|", "SF|0|V11|1.0|", "SF|0|V12|-7.0|", "SCF|0|CF|1.5|-0.0005|", "SCF|0|CG|-123456789012345678.0|2.0|" );
   CASE( "fits_digits_minus17",
         "FD|-17", "SF|0|W0|0.3333333333333333|", "SF|0|W1|266.41666666666663|", "SF|0|W2|-1.2345678901234567e-100|", "SF|0|W3|-123456789012345678.0|", "SF|0|W4|-0.00012345678901234567|", "SF|0|W5|1.2345678901234567e300|", "SCF|0|CW|-1.2345678901234567e-100|-0.00012345678901234567|" );
   CASE( "attribute_accessors",
         "AI|SipReplace", "ASI|SipReplace|0", "AI|SipReplace", "AX|SipReplace", "AI|SipReplace",
         "AI|CarLin", "ASI|CarLin|1", "AI|CarLin", "AX|CarLin", "AI|CarLin",
         "AI|FitsRounding", "ASI|FitsRounding|-3", "AI|FitsRounding", "AX|FitsRounding", "AI|FitsRounding",
         "AD|FitsTol", "ASD|FitsTol|-1", "AD|FitsTol", "ASD|FitsTol|0.5", "AD|FitsTol", "AX|FitsTol", "AD|FitsTol",
         "AI|FitsDigits", "ASI|FitsDigits|17", "AI|FitsDigits", "ASI|FitsDigits|-4", "AI|FitsDigits", "AX|FitsDigits", "AI|FitsDigits",
         "AI|DefB1950", "ENC|FITS-WCS", "AI|DefB1950", "ASI|DefB1950|1", "AI|DefB1950", "AX|DefB1950", "AI|DefB1950" );
   CASE( "fits_rounding_zero",
         "ASI|FitsRounding|0", "SF|0|R|1.2999999|", "AX|FitsRounding", "SF|0|S|1.2999999|" );
   CASE( "def_b1950_default_wcs",
         "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "def_b1950_set_wcs",
         "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "ENC|FITS-WCS", "ASI|DefB1950|1", "CC", "RW" );
   CASE( "def_b1950_cleared_pc",
         "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "ENC|FITS-PC", "ASI|DefB1950|0", "CC", "RW" );
   CASE( "poly_tan_accessors",
         "AI|PolyTan", "ASI|PolyTan|0", "AI|PolyTan", "ASI|PolyTan|3", "AI|PolyTan", "AX|PolyTan", "AI|PolyTan" );
   CASE( "poly_tan_auto_lat", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|PV2_0   = 0.0", "P|0|PV2_1   = 1.0", "P|0|PV2_2   = 0.01", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "poly_tan_off_lat", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|PV2_0   = 0.0", "P|0|PV2_1   = 1.0", "P|0|PV2_2   = 0.01", "ENC|FITS-WCS", "ASI|PolyTan|0", "CC", "RW" );
   CASE( "poly_tan_auto_lon", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|PV1_0   = 0.0", "P|0|PV1_1   = 1.0", "P|0|PV1_2   = 0.01", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "poly_tan_on_lon", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|PV1_0   = 0.0", "P|0|PV1_1   = 1.0", "P|0|PV1_2   = 0.01", "ENC|FITS-WCS", "ASI|PolyTan|1", "CC", "RW" );
   CASE( "poly_tan_zero_terms", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|PV2_1   = 0.0", "P|0|PV2_2   = 0.0", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "poly_tan_auto_both", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|PV1_0   = 0.0", "P|0|PV1_1   = 1.0", "P|0|PV1_2   = 0.01", "P|0|PV2_0   = 0.0", "P|0|PV2_1   = 1.0", "P|0|PV2_2   = 0.01", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "poly_tan_off_both", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|PV1_0   = 0.0", "P|0|PV1_1   = 1.0", "P|0|PV1_2   = 0.01", "P|0|PV2_0   = 0.0", "P|0|PV2_1   = 1.0", "P|0|PV2_2   = 0.01", "ENC|FITS-WCS", "ASI|PolyTan|0", "CC", "RW" );
   CASE( "ignore_bad_alt_accessors",
         "AI|IgnoreBadAlt", "ASI|IgnoreBadAlt|1", "AI|IgnoreBadAlt", "AX|IgnoreBadAlt", "AI|IgnoreBadAlt" );
   CASE( "bad_alt_fails", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1A = 'RA---TAN'", "P|0|CTYPE2A = 'GLAT-TAN'", "P|0|CRVAL1A = 1", "P|0|CRVAL2A = 2", "P|0|CRPIX1A = 1", "P|0|CRPIX2A = 1", "P|0|CTYPE1B = 'GLON-TAN'", "P|0|CTYPE2B = 'GLAT-TAN'", "P|0|CRVAL1B = 1", "P|0|CRVAL2B = 2", "P|0|CRPIX1B = 1", "P|0|CRPIX2B = 1", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "bad_alt_ignored", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1A = 'RA---TAN'", "P|0|CTYPE2A = 'GLAT-TAN'", "P|0|CRVAL1A = 1", "P|0|CRVAL2A = 2", "P|0|CRPIX1A = 1", "P|0|CRPIX2A = 1", "P|0|CTYPE1B = 'GLON-TAN'", "P|0|CTYPE2B = 'GLAT-TAN'", "P|0|CRVAL1B = 1", "P|0|CRVAL2B = 2", "P|0|CRPIX1B = 1", "P|0|CRPIX2B = 1", "ENC|FITS-WCS", "ASI|IgnoreBadAlt|1", "CC", "RW" );
   CASE( "celestial_unknown_projection", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---XYZ'", "P|0|CTYPE2  = 'DEC--XYZ'", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "celestial_two_longitudes", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'GLON-TAN'", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "celestial_mixed_projections", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--SIN'", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "celestial_lone_latitude", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'FREQ'", "P|0|CTYPE2  = 'DEC--TAN'", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "celestial_lowercase", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'ra---tan'", "P|0|CTYPE2  = 'dec--tan'", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "celestial_bare_names", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA'", "P|0|CTYPE2  = 'DEC'", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "celestial_lowercase_projection", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---tan'", "P|0|CTYPE2  = 'DEC--tan'", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "warnings_accessors", "AC|Warnings", "AC|AllWarnings", "ASC|Warnings| BadVal\tnoCTYPE ", "AC|Warnings", "ASC|Warnings|BadVal bogus", "AC|Warnings", "ASC|Warnings| ", "AC|Warnings", "AX|Warnings", "AC|Warnings" );
   CASE( "warnings_bad_name_default", "P|0|BAD%NAME= 1", "P|0|GOOD    = 'x", "WN" );
   CASE( "warnings_bad_name_silenced", "ASC|Warnings|BadKeyValue", "P|0|BAD%NAME= 1", "P|0|GOOD    = 'x", "WN" );
   CASE( "warnings_bad_value_only", "ASC|Warnings|badkeyvalue", "P|0|BAD%NAME= 1", "P|0|GOOD    = 'x", "WN" );
   CASE( "warnings_lowercase_names", "P|0|bad%x   some text", "P|0|bad%y   = 1", "P|0|bad%z   = 'x' / c", "WN" );
   CASE( "warnings_setter_names", "SI|0|bad.a|1|c", "SU|0|bad.b|c", "GF|bad.a", "TF|bad.b", "WN", "CM|0|x", "P|1|comment more", "P|0|comment again" );
   CASE( "getter_bad_template", "P|0|A       = 1", "P|0|B       = 2", "P|0|C       = 3", "CC", "GF|b%a", "SC|2", "GF|c%a", "SC|4", "GF|x%a", "SC|1", "GF|a%", "SC|1", "GF|b%1", "SC|1", "GF|c%1d" );
   CASE( "sky_equinox_no_radesys", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|EQUINOX = 1950.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_equinox_no_radesys_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|EQUINOX = 1950.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_equinox_1984_no_radesys", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|EQUINOX = 1984.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_equinox_1984_no_radesys_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|EQUINOX = 1984.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_nothing_wcs", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_nothing_wcs_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_nothing_pc", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "ENC|FITS-PC", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_nothing_pc_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "ENC|FITS-PC", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_fk5_no_equinox", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'FK5'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_fk5_no_equinox_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'FK5'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_fk4_padded", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'FK4 X'", "P|0|MJD-OBS = 50000.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_fk4_padded_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'FK4 X'", "P|0|MJD-OBS = 50000.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_icrs_equinox", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|EQUINOX = 2000.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_icrs_equinox_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|EQUINOX = 2000.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_radesys_unknown", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'FK6'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_radesys_unknown_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'FK6'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_radesys_lower", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'fk5'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_radesys_lower_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'fk5'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_ecliptic_default", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'ELON-TAN'", "P|0|CTYPE2  = 'ELAT-TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "ENC|FITS-WCS", "ASI|DefB1950|1", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_ecliptic_default_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'ELON-TAN'", "P|0|CTYPE2  = 'ELAT-TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "ENC|FITS-WCS", "ASI|DefB1950|1", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_ecliptic_equinox", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'ELON-TAN'", "P|0|CTYPE2  = 'ELAT-TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|EQUINOX = 1950.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_ecliptic_equinox_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'ELON-TAN'", "P|0|CTYPE2  = 'ELAT-TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|EQUINOX = 1950.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_ecliptic_j2010", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'ELON-TAN'", "P|0|CTYPE2  = 'ELAT-TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|EQUINOX = 2010.0", "P|0|MJD-OBS = 55000.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_ecliptic_j2010_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'ELON-TAN'", "P|0|CTYPE2  = 'ELAT-TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|EQUINOX = 2010.0", "P|0|MJD-OBS = 55000.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_helio_equinox", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'HLON-TAN'", "P|0|CTYPE2  = 'HLAT-TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|EQUINOX = 1950.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_helio_equinox_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'HLON-TAN'", "P|0|CTYPE2  = 'HLAT-TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|EQUINOX = 1950.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_galactic_def_b1950", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'GLON-TAN'", "P|0|CTYPE2  = 'GLAT-TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "ENC|FITS-WCS", "ASI|DefB1950|1", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_galactic_def_b1950_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'GLON-TAN'", "P|0|CTYPE2  = 'GLAT-TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "ENC|FITS-WCS", "ASI|DefB1950|1", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_unknown_system", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'XLON-TAN'", "P|0|CTYPE2  = 'XLAT-TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_unknown_system_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'XLON-TAN'", "P|0|CTYPE2  = 'XLAT-TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_offset_system", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'OFLN-TAN'", "P|0|CTYPE2  = 'OFLT-TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_offset_system_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'OFLN-TAN'", "P|0|CTYPE2  = 'OFLT-TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_timesys_ut_uc", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|MJD-OBS = 55000.0", "P|0|TIMESYS = 'UT'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_timesys_ut_uc_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|MJD-OBS = 55000.0", "P|0|TIMESYS = 'UT'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_timesys_et_uc", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|MJD-OBS = 55000.0", "P|0|TIMESYS = 'ET'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_timesys_et_uc_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|MJD-OBS = 55000.0", "P|0|TIMESYS = 'ET'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_timesys_xyz_uc", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|MJD-OBS = 55000.0", "P|0|TIMESYS = 'XYZ'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_timesys_xyz_uc_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|MJD-OBS = 55000.0", "P|0|TIMESYS = 'XYZ'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_timesys_tcb_uc", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|MJD-OBS = 55000.0", "P|0|TIMESYS = 'TCB'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_timesys_tcb_uc_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|MJD-OBS = 55000.0", "P|0|TIMESYS = 'TCB'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_timesys_tcb_padded_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|MJD-OBS = 55000.0", "P|0|TIMESYS = 'TCB     '", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_timesys_tcb_padded_rw_wn", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|MJD-OBS = 55000.0", "P|0|TIMESYS = 'TCB     '", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW", "WN" );
   CASE( "sky_timesys_utc_lc", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|MJD-OBS = 55000.0", "P|0|TIMESYS = 'utc'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_timesys_utc_lc_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|MJD-OBS = 55000.0", "P|0|TIMESYS = 'utc'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_timesys_no_date", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|TIMESYS = 'ET'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_timesys_no_date_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|TIMESYS = 'ET'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_mjd_avg", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|MJD-OBS = 55000.0", "P|0|MJD-AVG = 55000.5", "P|0|DATE-AVG= '2020-01-01'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_mjd_avg_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|MJD-OBS = 55000.0", "P|0|MJD-AVG = 55000.5", "P|0|DATE-AVG= '2020-01-01'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_date_avg_only", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|DATE-AVG= '2020-01-01'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_date_avg_only_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|DATE-AVG= '2020-01-01'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_bad_latitude", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 95", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_bad_latitude_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 95", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_bad_latitude_car", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = -100", "P|0|CTYPE1  = 'RA---CAR'", "P|0|CTYPE2  = 'DEC--CAR'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_bad_latitude_car_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = -100", "P|0|CTYPE1  = 'RA---CAR'", "P|0|CTYPE2  = 'DEC--CAR'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_car_no_latpole", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---CAR'", "P|0|CTYPE2  = 'DEC--CAR'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|LONPOLE = 0.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_car_no_latpole_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---CAR'", "P|0|CTYPE2  = 'DEC--CAR'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|LONPOLE = 0.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_car_latpole", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---CAR'", "P|0|CTYPE2  = 'DEC--CAR'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|LONPOLE = 0.0", "P|0|LATPOLE = -70.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_car_latpole_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---CAR'", "P|0|CTYPE2  = 'DEC--CAR'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|LONPOLE = 0.0", "P|0|LATPOLE = -70.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "sky_sfl_defaults", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 30", "P|0|CTYPE1  = 'RA---SFL'", "P|0|CTYPE2  = 'DEC--SFL'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sky_sfl_defaults_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 30", "P|0|CTYPE1  = 'RA---SFL'", "P|0|CTYPE2  = 'DEC--SFL'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "dist_sip_axis3", "P|0|NAXIS   = 3", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|NAXIS3  = 10", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRPIX3  = 1", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CRVAL3  = 1.0E9", "P|0|CDELT1  = -0.001", "P|0|CDELT2  = 0.001", "P|0|CDELT3  = 1.0E6", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CTYPE3  = 'FREQ-SIP'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "dist_zpx_three", "P|0|NAXIS   = 3", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|NAXIS3  = 10", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRPIX3  = 1", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CRVAL3  = 1.0E9", "P|0|CDELT1  = -0.001", "P|0|CDELT2  = 0.001", "P|0|CDELT3  = 1.0E6", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---ZPX'", "P|0|CTYPE2  = 'DEC--ZPX'", "P|0|CTYPE3  = 'FREQ-ZPX'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "dist_unknown", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN-XYZ'", "P|0|CTYPE2  = 'DEC--TAN-XYZ'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "dist_short_code", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN-X'", "P|0|CTYPE2  = 'DEC--TAN-X'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "dist_padded_code", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN- X'", "P|0|CTYPE2  = 'DEC--TAN- X'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "noctype_axis3", "P|0|NAXIS   = 3", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|NAXIS3  = 10", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRPIX3  = 1", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CRVAL3  = 1.0E9", "P|0|CDELT1  = -0.001", "P|0|CDELT2  = 0.001", "P|0|CDELT3  = 1.0E6", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "noctype_all", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "badval_crval", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CRVAL1  = 'abc'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "badval_equinox_j", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|EQUINOX = 'J2000'", "ENC|FITS-PC", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "badval_equinox_x", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|EQUINOX = 'X2000'", "ENC|FITS-PC", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "badval_two_cards", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CDELT1  = 'a'", "P|0|LONPOLE = 'b'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "badval_then_undef", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CDELT1  = 'a'", "P|0|LONPOLE =", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "class_no_velocity", "P|0|NAXIS   = 3", "P|0|CTYPE1  = 'FREQ    '", "P|0|CRVAL1  = 0.0", "P|0|CRPIX1  = 512.5", "P|0|CDELT1  = 39062.5", "P|0|CTYPE2  = 'RA---TAN'", "P|0|CRVAL2  = 40.738", "P|0|CRPIX2  = 0.5", "P|0|CDELT2  = -0.01", "P|0|CTYPE3  = 'DEC--TAN'", "P|0|CRVAL3  = 62.933", "P|0|CRPIX3  = 0.5", "P|0|CDELT3  = 0.01", "P|0|RADESYS = 'FK5'", "P|0|EQUINOX = 2000.0", "P|0|SPECSYS = 'SOURCE'", "P|0|RESTFREQ= 1.102013543E11", "P|0|DELTAV  = -106.265872717", "P|0|MJD-OBS = 52413.59", "ENC|FITS-CLASS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "class_no_velocity_card5", "P|0|NAXIS   = 3", "P|0|CTYPE1  = 'FREQ    '", "P|0|CRVAL1  = 0.0", "P|0|CRPIX1  = 512.5", "P|0|CDELT1  = 39062.5", "P|0|CTYPE2  = 'RA---TAN'", "P|0|CRVAL2  = 40.738", "P|0|CRPIX2  = 0.5", "P|0|CDELT2  = -0.01", "P|0|CTYPE3  = 'DEC--TAN'", "P|0|CRVAL3  = 62.933", "P|0|CRPIX3  = 0.5", "P|0|CDELT3  = 0.01", "P|0|RADESYS = 'FK5'", "P|0|EQUINOX = 2000.0", "P|0|SPECSYS = 'SOURCE'", "P|0|RESTFREQ= 1.102013543E11", "P|0|DELTAV  = -106.265872717", "P|0|MJD-OBS = 52413.59", "ENC|FITS-CLASS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "SC|5", "R", "WN" );
   CASE( "class_velo_lsr", "P|0|NAXIS   = 3", "P|0|CTYPE1  = 'FREQ    '", "P|0|CRVAL1  = 0.0", "P|0|CRPIX1  = 512.5", "P|0|CDELT1  = 39062.5", "P|0|CTYPE2  = 'RA---TAN'", "P|0|CRVAL2  = 40.738", "P|0|CRPIX2  = 0.5", "P|0|CDELT2  = -0.01", "P|0|CTYPE3  = 'DEC--TAN'", "P|0|CRVAL3  = 62.933", "P|0|CRPIX3  = 0.5", "P|0|CDELT3  = 0.01", "P|0|RADESYS = 'FK5'", "P|0|EQUINOX = 2000.0", "P|0|SPECSYS = 'SOURCE'", "P|0|RESTFREQ= 1.102013543E11", "P|0|DELTAV  = -106.265872717", "P|0|MJD-OBS = 52413.59", "P|0|VELO-LSR= 2500.0", "ENC|FITS-CLASS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "sao_ok", "P|0|NAXIS   =                    2", "P|0|NAXIS1  =                 2048", "P|0|NAXIS2  =                 2048", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CRVAL1  =              180.000", "P|0|CRVAL2  =               45.000", "P|0|CRPIX1  =             1024.500", "P|0|CRPIX2  =             1024.500", "P|0|CDELT1  =           -1.111E-04", "P|0|CDELT2  =            1.111E-04", "P|0|EQUINOX =               2000.0", "P|0|RADESYS = 'FK5     '", "P|0|CO1_1   =            0.000E+00", "P|0|CO1_2   =            1.000E+00", "P|0|CO1_3   =            0.000E+00", "P|0|CO1_4   =            2.500E-06", "P|0|CO1_5   =           -1.200E-06", "P|0|CO1_6   =            5.000E-07", "P|0|CO1_7   =            1.000E-09", "P|0|CO1_8   =            0.000E+00", "P|0|CO1_9   =            0.000E+00", "P|0|CO1_10  =            0.000E+00", "P|0|CO2_1   =            0.000E+00", "P|0|CO2_2   =            0.000E+00", "P|0|CO2_3   =            1.000E+00", "P|0|CO2_4   =           -1.300E-06", "P|0|CO2_5   =            2.400E-06", "P|0|CO2_6   =            4.500E-07", "P|0|CO2_7   =            0.000E+00", "P|0|CO2_8   =            0.000E+00", "P|0|CO2_9   =            0.000E+00", "P|0|CO2_10  =            1.500E-09", "ENC|FITS-WCS", "CC", "R", "WN" );
   CASE( "sao_ok_rw", "P|0|NAXIS   =                    2", "P|0|NAXIS1  =                 2048", "P|0|NAXIS2  =                 2048", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CRVAL1  =              180.000", "P|0|CRVAL2  =               45.000", "P|0|CRPIX1  =             1024.500", "P|0|CRPIX2  =             1024.500", "P|0|CDELT1  =           -1.111E-04", "P|0|CDELT2  =            1.111E-04", "P|0|EQUINOX =               2000.0", "P|0|RADESYS = 'FK5     '", "P|0|CO1_1   =            0.000E+00", "P|0|CO1_2   =            1.000E+00", "P|0|CO1_3   =            0.000E+00", "P|0|CO1_4   =            2.500E-06", "P|0|CO1_5   =           -1.200E-06", "P|0|CO1_6   =            5.000E-07", "P|0|CO1_7   =            1.000E-09", "P|0|CO1_8   =            0.000E+00", "P|0|CO1_9   =            0.000E+00", "P|0|CO1_10  =            0.000E+00", "P|0|CO2_1   =            0.000E+00", "P|0|CO2_2   =            0.000E+00", "P|0|CO2_3   =            1.000E+00", "P|0|CO2_4   =           -1.300E-06", "P|0|CO2_5   =            2.400E-06", "P|0|CO2_6   =            4.500E-07", "P|0|CO2_7   =            0.000E+00", "P|0|CO2_8   =            0.000E+00", "P|0|CO2_9   =            0.000E+00", "P|0|CO2_10  =            1.500E-09", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "sao_zero_axis2", "P|0|NAXIS   =                    2", "P|0|NAXIS1  =                 2048", "P|0|NAXIS2  =                 2048", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CRVAL1  =              180.000", "P|0|CRVAL2  =               45.000", "P|0|CRPIX1  =             1024.500", "P|0|CRPIX2  =             1024.500", "P|0|CDELT1  =           -1.111E-04", "P|0|CDELT2  =            1.111E-04", "P|0|EQUINOX =               2000.0", "P|0|RADESYS = 'FK5     '", "P|0|CO1_1   =            0.000E+00", "P|0|CO1_2   =            1.000E+00", "P|0|CO1_3   =            0.000E+00", "P|0|CO1_4   =            2.500E-06", "P|0|CO1_5   =           -1.200E-06", "P|0|CO1_6   =            5.000E-07", "P|0|CO1_7   =            1.000E-09", "P|0|CO1_8   =            0.000E+00", "P|0|CO1_9   =            0.000E+00", "P|0|CO1_10  =            0.000E+00", "P|0|CO2_1   = 0.0", "P|0|CO2_2   = 0.0", "P|0|CO2_3   = 0.0", "P|0|CO2_4   = 0.0", "ENC|FITS-WCS", "CC", "R", "WN" );
   CASE( "sao_zero_axis2_rw", "P|0|NAXIS   =                    2", "P|0|NAXIS1  =                 2048", "P|0|NAXIS2  =                 2048", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CRVAL1  =              180.000", "P|0|CRVAL2  =               45.000", "P|0|CRPIX1  =             1024.500", "P|0|CRPIX2  =             1024.500", "P|0|CDELT1  =           -1.111E-04", "P|0|CDELT2  =            1.111E-04", "P|0|EQUINOX =               2000.0", "P|0|RADESYS = 'FK5     '", "P|0|CO1_1   =            0.000E+00", "P|0|CO1_2   =            1.000E+00", "P|0|CO1_3   =            0.000E+00", "P|0|CO1_4   =            2.500E-06", "P|0|CO1_5   =           -1.200E-06", "P|0|CO1_6   =            5.000E-07", "P|0|CO1_7   =            1.000E-09", "P|0|CO1_8   =            0.000E+00", "P|0|CO1_9   =            0.000E+00", "P|0|CO1_10  =            0.000E+00", "P|0|CO2_1   = 0.0", "P|0|CO2_2   = 0.0", "P|0|CO2_3   = 0.0", "P|0|CO2_4   = 0.0", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "sao_three_ctypes", "P|0|NAXIS   =                    2", "P|0|NAXIS1  =                 2048", "P|0|NAXIS2  =                 2048", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CRVAL1  =              180.000", "P|0|CRVAL2  =               45.000", "P|0|CRPIX1  =             1024.500", "P|0|CRPIX2  =             1024.500", "P|0|CDELT1  =           -1.111E-04", "P|0|CDELT2  =            1.111E-04", "P|0|EQUINOX =               2000.0", "P|0|RADESYS = 'FK5     '", "P|0|CO1_1   =            0.000E+00", "P|0|CO1_2   =            1.000E+00", "P|0|CO1_3   =            0.000E+00", "P|0|CO1_4   =            2.500E-06", "P|0|CO1_5   =           -1.200E-06", "P|0|CO1_6   =            5.000E-07", "P|0|CO1_7   =            1.000E-09", "P|0|CO1_8   =            0.000E+00", "P|0|CO1_9   =            0.000E+00", "P|0|CO1_10  =            0.000E+00", "P|0|CO2_1   =            0.000E+00", "P|0|CO2_2   =            0.000E+00", "P|0|CO2_3   =            1.000E+00", "P|0|CO2_4   =           -1.300E-06", "P|0|CO2_5   =            2.400E-06", "P|0|CO2_6   =            4.500E-07", "P|0|CO2_7   =            0.000E+00", "P|0|CO2_8   =            0.000E+00", "P|0|CO2_9   =            0.000E+00", "P|0|CO2_10  =            1.500E-09", "P|0|NAXIS3  = 5", "P|0|CTYPE3  = 'FREQ'", "P|0|CRVAL3  = 1.0E9", "P|0|CRPIX3  = 1", "P|0|CDELT3  = 1.0E6", "ENC|FITS-WCS", "CC", "R", "WN" );
   CASE( "sao_three_ctypes_rw", "P|0|NAXIS   =                    2", "P|0|NAXIS1  =                 2048", "P|0|NAXIS2  =                 2048", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CRVAL1  =              180.000", "P|0|CRVAL2  =               45.000", "P|0|CRPIX1  =             1024.500", "P|0|CRPIX2  =             1024.500", "P|0|CDELT1  =           -1.111E-04", "P|0|CDELT2  =            1.111E-04", "P|0|EQUINOX =               2000.0", "P|0|RADESYS = 'FK5     '", "P|0|CO1_1   =            0.000E+00", "P|0|CO1_2   =            1.000E+00", "P|0|CO1_3   =            0.000E+00", "P|0|CO1_4   =            2.500E-06", "P|0|CO1_5   =           -1.200E-06", "P|0|CO1_6   =            5.000E-07", "P|0|CO1_7   =            1.000E-09", "P|0|CO1_8   =            0.000E+00", "P|0|CO1_9   =            0.000E+00", "P|0|CO1_10  =            0.000E+00", "P|0|CO2_1   =            0.000E+00", "P|0|CO2_2   =            0.000E+00", "P|0|CO2_3   =            1.000E+00", "P|0|CO2_4   =           -1.300E-06", "P|0|CO2_5   =            2.400E-06", "P|0|CO2_6   =            4.500E-07", "P|0|CO2_7   =            0.000E+00", "P|0|CO2_8   =            0.000E+00", "P|0|CO2_9   =            0.000E+00", "P|0|CO2_10  =            1.500E-09", "P|0|NAXIS3  = 5", "P|0|CTYPE3  = 'FREQ'", "P|0|CRVAL3  = 1.0E9", "P|0|CRPIX3  = 1", "P|0|CDELT3  = 1.0E6", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "sao_sin", "P|0|NAXIS   =                    2", "P|0|NAXIS1  =                 2048", "P|0|NAXIS2  =                 2048", "P|0|CTYPE1  = 'RA---SIN'", "P|0|CTYPE2  = 'DEC--SIN'", "P|0|CRVAL1  =              180.000", "P|0|CRVAL2  =               45.000", "P|0|CRPIX1  =             1024.500", "P|0|CRPIX2  =             1024.500", "P|0|CDELT1  =           -1.111E-04", "P|0|CDELT2  =            1.111E-04", "P|0|EQUINOX =               2000.0", "P|0|RADESYS = 'FK5     '", "P|0|CO1_1   =            0.000E+00", "P|0|CO1_2   =            1.000E+00", "P|0|CO1_3   =            0.000E+00", "P|0|CO1_4   =            2.500E-06", "P|0|CO1_5   =           -1.200E-06", "P|0|CO1_6   =            5.000E-07", "P|0|CO1_7   =            1.000E-09", "P|0|CO1_8   =            0.000E+00", "P|0|CO1_9   =            0.000E+00", "P|0|CO1_10  =            0.000E+00", "P|0|CO2_1   =            0.000E+00", "P|0|CO2_2   =            0.000E+00", "P|0|CO2_3   =            1.000E+00", "P|0|CO2_4   =           -1.300E-06", "P|0|CO2_5   =            2.400E-06", "P|0|CO2_6   =            4.500E-07", "P|0|CO2_7   =            0.000E+00", "P|0|CO2_8   =            0.000E+00", "P|0|CO2_9   =            0.000E+00", "P|0|CO2_10  =            1.500E-09", "ENC|FITS-WCS", "CC", "R", "WN" );
   CASE( "sao_sin_rw", "P|0|NAXIS   =                    2", "P|0|NAXIS1  =                 2048", "P|0|NAXIS2  =                 2048", "P|0|CTYPE1  = 'RA---SIN'", "P|0|CTYPE2  = 'DEC--SIN'", "P|0|CRVAL1  =              180.000", "P|0|CRVAL2  =               45.000", "P|0|CRPIX1  =             1024.500", "P|0|CRPIX2  =             1024.500", "P|0|CDELT1  =           -1.111E-04", "P|0|CDELT2  =            1.111E-04", "P|0|EQUINOX =               2000.0", "P|0|RADESYS = 'FK5     '", "P|0|CO1_1   =            0.000E+00", "P|0|CO1_2   =            1.000E+00", "P|0|CO1_3   =            0.000E+00", "P|0|CO1_4   =            2.500E-06", "P|0|CO1_5   =           -1.200E-06", "P|0|CO1_6   =            5.000E-07", "P|0|CO1_7   =            1.000E-09", "P|0|CO1_8   =            0.000E+00", "P|0|CO1_9   =            0.000E+00", "P|0|CO1_10  =            0.000E+00", "P|0|CO2_1   =            0.000E+00", "P|0|CO2_2   =            0.000E+00", "P|0|CO2_3   =            1.000E+00", "P|0|CO2_4   =           -1.300E-06", "P|0|CO2_5   =            2.400E-06", "P|0|CO2_6   =            4.500E-07", "P|0|CO2_7   =            0.000E+00", "P|0|CO2_8   =            0.000E+00", "P|0|CO2_9   =            0.000E+00", "P|0|CO2_10  =            1.500E-09", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "sao_not_sao", "P|0|NAXIS   =                    2", "P|0|NAXIS1  =                 2048", "P|0|NAXIS2  =                 2048", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CRVAL1  =              180.000", "P|0|CRVAL2  =               45.000", "P|0|CRPIX1  =             1024.500", "P|0|CRPIX2  =             1024.500", "P|0|CDELT1  =           -1.111E-04", "P|0|CDELT2  =            1.111E-04", "P|0|EQUINOX =               2000.0", "P|0|RADESYS = 'FK5     '", "P|0|CO1_1   =            0.000E+00", "P|0|CO1_2   =            1.000E+00", "P|0|CO1_4   =            2.500E-06", "P|0|CO1_5   =           -1.200E-06", "P|0|CO1_6   =            5.000E-07", "P|0|CO1_7   =            1.000E-09", "P|0|CO1_8   =            0.000E+00", "P|0|CO1_9   =            0.000E+00", "P|0|CO1_10  =            0.000E+00", "ENC|FITS-WCS", "CC", "R", "WN" );
   CASE( "sao_not_sao_rw", "P|0|NAXIS   =                    2", "P|0|NAXIS1  =                 2048", "P|0|NAXIS2  =                 2048", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CRVAL1  =              180.000", "P|0|CRVAL2  =               45.000", "P|0|CRPIX1  =             1024.500", "P|0|CRPIX2  =             1024.500", "P|0|CDELT1  =           -1.111E-04", "P|0|CDELT2  =            1.111E-04", "P|0|EQUINOX =               2000.0", "P|0|RADESYS = 'FK5     '", "P|0|CO1_1   =            0.000E+00", "P|0|CO1_2   =            1.000E+00", "P|0|CO1_4   =            2.500E-06", "P|0|CO1_5   =           -1.200E-06", "P|0|CO1_6   =            5.000E-07", "P|0|CO1_7   =            1.000E-09", "P|0|CO1_8   =            0.000E+00", "P|0|CO1_9   =            0.000E+00", "P|0|CO1_10  =            0.000E+00", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "badval_complex", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|LONPOLE = 1.0 2.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "badval_complex_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|LONPOLE = 1.0 2.0", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "badval_string_number", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|LONPOLE = '170.0'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "badval_string_number_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|LONPOLE = '170.0'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "badval_logical", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|LONPOLE = T", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "badval_logical_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|LONPOLE = T", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "badval_commentary", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|LONPOLE  is not given", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "R", "WN" );
   CASE( "badval_commentary_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|LONPOLE  is not given", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW" );
   CASE( "write_timesys_ut", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "P|0|TIMESYS = 'UT'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW", "WN" );
   CASE( "write_timesys_xyz_alt", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "P|0|TIMESYS = 'XYZ'", "P|0|CTYPE1A = 'GLON-TAN'", "P|0|CTYPE2A = 'GLAT-TAN'", "P|0|CRVAL1A = 1", "P|0|CRVAL2A = 2", "P|0|CRPIX1A = 1", "P|0|CRPIX2A = 1", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW", "WN" );
   CASE( "write_timesys_tai_alt", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "P|0|TIMESYS = 'TAI'", "P|0|CTYPE1A = 'GLON-TAN'", "P|0|CTYPE2A = 'GLAT-TAN'", "P|0|CRVAL1A = 1", "P|0|CRVAL2A = 2", "P|0|CRPIX1A = 1", "P|0|CRPIX2A = 1", "ENC|FITS-WCS", "ASC|Warnings|BadAlt BadCel BadCTYPE BadKeyName BadKeyValue BadLat BadMat BadPV BadVal Distortion NoCTYPE NoEquinox NoLatPole NoLonPole NoMJD-OBS NoRADESYS Tnx Zpx", "CC", "RW", "WN" );
   CASE( "write_warning_placement", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "P|0|TIMESYS = 'UT'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt", "SC|3", "RSW|BadVal", "WN" );
   CASE( "write_warning_placement_start", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "P|0|TIMESYS = 'UT'", "ENC|FITS-WCS", "ASC|Warnings|BadAlt", "CC", "RSW|BadVal", "WN" );
   CASE( "alt_axes_all", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "P|0|CTYPE1A = 'GLON-TAN'", "P|0|CTYPE2A = 'GLAT-TAN'", "P|0|CRVAL1A = 1", "P|0|CRVAL2A = 2", "P|0|CRPIX1A = 1", "P|0|CRPIX2A = 1", "P|0|CD1_1A  = -0.001", "P|0|CD2_2A  = 0.001", "P|0|CTYPE1C = 'ELON-TAN'", "P|0|CTYPE2C = 'ELAT-TAN'", "P|0|CRVAL1C = 3", "P|0|CRVAL2C = 4", "P|0|CRPIX1C = 1", "P|0|CRPIX2C = 1", "P|0|CD1_1C  = -0.001", "P|0|CD2_2C  = 0.001", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "alt_axes_none", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "P|0|CTYPE1A = 'GLON-TAN'", "P|0|CTYPE2A = 'GLAT-TAN'", "P|0|CRVAL1A = 1", "P|0|CRVAL2A = 2", "P|0|CRPIX1A = 1", "P|0|CRPIX2A = 1", "P|0|CD1_1A  = -0.001", "P|0|CD2_2A  = 0.001", "P|0|CTYPE1C = 'ELON-TAN'", "P|0|CTYPE2C = 'ELAT-TAN'", "P|0|CRVAL1C = 3", "P|0|CRVAL2C = 4", "P|0|CRPIX1C = 1", "P|0|CRPIX2C = 1", "P|0|CD1_1C  = -0.001", "P|0|CD2_2C  = 0.001", "ENC|FITS-WCS", "ASC|AltAxes|NONE", "CC", "RW" );
   CASE( "alt_axes_ident", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "P|0|CTYPE1A = 'GLON-TAN'", "P|0|CTYPE2A = 'GLAT-TAN'", "P|0|CRVAL1A = 1", "P|0|CRVAL2A = 2", "P|0|CRPIX1A = 1", "P|0|CRPIX2A = 1", "P|0|CD1_1A  = -0.001", "P|0|CD2_2A  = 0.001", "P|0|CTYPE1C = 'ELON-TAN'", "P|0|CTYPE2C = 'ELAT-TAN'", "P|0|CRVAL1C = 3", "P|0|CRVAL2C = 4", "P|0|CRPIX1C = 1", "P|0|CRPIX2C = 1", "P|0|CD1_1C  = -0.001", "P|0|CD2_2C  = 0.001", "ENC|FITS-WCS", "ASC|AltAxes|ident", "CC", "RW" );
   CASE( "alt_axes_all_noident", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "P|0|CTYPE1A = 'GLON-TAN'", "P|0|CTYPE2A = 'GLAT-TAN'", "P|0|CRVAL1A = 1", "P|0|CRVAL2A = 2", "P|0|CRPIX1A = 1", "P|0|CRPIX2A = 1", "P|0|CD1_1A  = -0.001", "P|0|CD2_2A  = 0.001", "P|0|CTYPE1C = 'ELON-TAN'", "P|0|CTYPE2C = 'ELAT-TAN'", "P|0|CRVAL1C = 3", "P|0|CRVAL2C = 4", "P|0|CRPIX1C = 1", "P|0|CRPIX2C = 1", "P|0|CD1_1C  = -0.001", "P|0|CD2_2C  = 0.001", "ENC|FITS-WCS", "CC", "RCIW|4" );
   CASE( "alt_axes_ident_noident", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "P|0|CTYPE1A = 'GLON-TAN'", "P|0|CTYPE2A = 'GLAT-TAN'", "P|0|CRVAL1A = 1", "P|0|CRVAL2A = 2", "P|0|CRPIX1A = 1", "P|0|CRPIX2A = 1", "P|0|CD1_1A  = -0.001", "P|0|CD2_2A  = 0.001", "P|0|CTYPE1C = 'ELON-TAN'", "P|0|CTYPE2C = 'ELAT-TAN'", "P|0|CRVAL1C = 3", "P|0|CRVAL2C = 4", "P|0|CRPIX1C = 1", "P|0|CRPIX2C = 1", "P|0|CD1_1C  = -0.001", "P|0|CD2_2C  = 0.001", "ENC|FITS-WCS", "ASC|AltAxes|IDENT", "CC", "RCIW|4" );
   CASE( "alt_axes_accessors", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "P|0|CTYPE1A = 'GLON-TAN'", "P|0|CTYPE2A = 'GLAT-TAN'", "P|0|CRVAL1A = 1", "P|0|CRVAL2A = 2", "P|0|CRPIX1A = 1", "P|0|CRPIX2A = 1", "P|0|CD1_1A  = -0.001", "P|0|CD2_2A  = 0.001", "P|0|CTYPE1C = 'ELON-TAN'", "P|0|CTYPE2C = 'ELAT-TAN'", "P|0|CRVAL1C = 3", "P|0|CRVAL2C = 4", "P|0|CRPIX1C = 1", "P|0|CRPIX2C = 1", "P|0|CD1_1C  = -0.001", "P|0|CD2_2C  = 0.001", "ENC|FITS-WCS", "AC|AltAxes", "ASC|AltAxes|Ident", "AC|AltAxes", "AX|AltAxes", "AC|AltAxes" );
   CASE( "alt_axes_offset", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 0", "P|0|CRVAL2  = 0", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'OFLN-TAN'", "P|0|CTYPE2  = 'OFLT-TAN'", "P|0|SREF1   = 0.5", "P|0|SREF2   = 0.3", "P|0|SREFIS  = 'ORIGIN'", "P|0|MJD-OBS = 55000.0", "P|0|CTYPE1B = 'GLON-TAN'", "P|0|CTYPE2B = 'GLAT-TAN'", "P|0|CRVAL1B = 1", "P|0|CRVAL2B = 2", "P|0|CRPIX1B = 1", "P|0|CRPIX2B = 1", "P|0|CD1_1B  = -0.001", "P|0|CD2_2B  = 0.001", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "alt_axes_offset_none", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 0", "P|0|CRVAL2  = 0", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'OFLN-TAN'", "P|0|CTYPE2  = 'OFLT-TAN'", "P|0|SREF1   = 0.5", "P|0|SREF2   = 0.3", "P|0|SREFIS  = 'ORIGIN'", "P|0|MJD-OBS = 55000.0", "P|0|CTYPE1B = 'GLON-TAN'", "P|0|CTYPE2B = 'GLAT-TAN'", "P|0|CRVAL1B = 1", "P|0|CRVAL2B = 2", "P|0|CRPIX1B = 1", "P|0|CRPIX2B = 1", "P|0|CD1_1B  = -0.001", "P|0|CD2_2B  = 0.001", "ENC|FITS-WCS", "ASC|AltAxes|NONE", "CC", "RW" );
   CASE( "alt_axes_alt_offset", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "P|0|CTYPE1B = 'OFLN-TAN'", "P|0|CTYPE2B = 'OFLT-TAN'", "P|0|CRVAL1B = 0", "P|0|CRVAL2B = 0", "P|0|CRPIX1B = 1", "P|0|CRPIX2B = 1", "P|0|CD1_1B  = -0.001", "P|0|CD2_2B  = 0.001", "P|0|RADESYSB= 'ICRS'", "P|0|SREF1B  = 0.5", "P|0|SREF2B  = 0.3", "P|0|SREFISB = 'POLE'", "P|0|CTYPE1D = 'GLON-TAN'", "P|0|CTYPE2D = 'GLAT-TAN'", "P|0|CRVAL1D = 1", "P|0|CRVAL2D = 2", "P|0|CRPIX1D = 1", "P|0|CRPIX2D = 1", "P|0|CD1_1D  = -0.001", "P|0|CD2_2D  = 0.001", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "iwc_off", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD1_2   = 0.0002", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "ENC|FITS-WCS", "CC", "RN" );
   CASE( "iwc_on", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD1_2   = 0.0002", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "ENC|FITS-WCS", "ASI|Iwc|1", "CC", "RN" );
   CASE( "iwc_on_alt", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD1_2   = 0.0002", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "P|0|CTYPE1A = 'GLON-TAN'", "P|0|CTYPE2A = 'GLAT-TAN'", "P|0|CRVAL1A = 1", "P|0|CRVAL2A = 2", "P|0|CRPIX1A = 1", "P|0|CRPIX2A = 1", "P|0|CD1_1A  = -0.001", "P|0|CD2_2A  = 0.001", "ENC|FITS-WCS", "ASI|Iwc|1", "CC", "RN" );
   CASE( "iwc_on_linear", "P|0|NAXIS   = 1", "P|0|NAXIS1  = 100", "P|0|CRPIX1  = 50", "P|0|CRVAL1  = 10", "P|0|CDELT1  = 2.0", "P|0|CTYPE1  = 'TIME'", "ENC|FITS-WCS", "ASI|Iwc|1", "CC", "RN" );
   CASE( "iwc_accessors", "AI|Iwc", "ASI|Iwc|5", "AI|Iwc", "AX|Iwc", "AI|Iwc" );
   CASE( "iwc_on_rw", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD1_2   = 0.0002", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "P|0|CTYPE1A = 'GLON-TAN'", "P|0|CTYPE2A = 'GLAT-TAN'", "P|0|CRVAL1A = 1", "P|0|CRVAL2A = 2", "P|0|CRPIX1A = 1", "P|0|CRPIX2A = 1", "P|0|CD1_1A  = -0.001", "P|0|CD2_2A  = 0.001", "ENC|FITS-WCS", "ASI|Iwc|1", "CC", "RW" );
   CASE( "iwc_on_spectral", "P|0|NAXIS   = 3", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|NAXIS3  = 10", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRPIX3  = 1", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CRVAL3  = 1.0E9", "P|0|CDELT1  = -0.001", "P|0|CDELT2  = 0.001", "P|0|CDELT3  = 1.0E6", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CTYPE3  = 'FREQ'", "P|0|SPECSYS = 'BARYCENT'", "P|0|MJD-OBS = 55000.0", "ENC|FITS-WCS", "ASI|Iwc|1", "CC", "RN" );
   CASE( "iwc_off_spectral", "P|0|NAXIS   = 3", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|NAXIS3  = 10", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRPIX3  = 1", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CRVAL3  = 1.0E9", "P|0|CDELT1  = -0.001", "P|0|CDELT2  = 0.001", "P|0|CDELT3  = 1.0E6", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CTYPE3  = 'FREQ'", "P|0|SPECSYS = 'BARYCENT'", "P|0|MJD-OBS = 55000.0", "ENC|FITS-WCS", "CC", "RN" );
   CASE( "iwc_on_spectral_cunit", "P|0|NAXIS   = 3", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|NAXIS3  = 10", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRPIX3  = 1", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CRVAL3  = 1.0", "P|0|CDELT1  = -0.001", "P|0|CDELT2  = 0.001", "P|0|CDELT3  = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CTYPE3  = 'FREQ'", "P|0|SPECSYS = 'BARYCENT'", "P|0|MJD-OBS = 55000.0", "P|0|CUNIT3  = 'GHz'", "ENC|FITS-WCS", "ASI|Iwc|1", "CC", "RN" );
   CASE( "iwc_on_cunit", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 10", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 1", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 3", "P|0|CDELT1  = 2.0", "P|0|CDELT2  = 0.5", "P|0|CTYPE1  = 'TIME'", "P|0|CUNIT1  = 's'", "P|0|CTYPE2  = 'DIST'", "P|0|CUNIT2  = 'm'", "ENC|FITS-WCS", "ASI|Iwc|1", "CC", "RN" );
   CASE( "fao_copy", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD1_2   = 0.0002", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "ENC|FITS-WCS", "ASC|FitsAxisOrder|<copy>", "CC", "RW" );
   CASE( "fao_dec_ra", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD1_2   = 0.0002", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "ENC|FITS-WCS", "ASC|FitsAxisOrder|Dec RA", "CC", "RW" );
   CASE( "fao_lower", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD1_2   = 0.0002", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "ENC|FITS-WCS", "ASC|FitsAxisOrder|dec ra", "CC", "RW" );
   CASE( "fao_missing", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD1_2   = 0.0002", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "ENC|FITS-WCS", "ASC|FitsAxisOrder|RA", "CC", "RW" );
   CASE( "fao_twice", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD1_2   = 0.0002", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "ENC|FITS-WCS", "ASC|FitsAxisOrder|RA RA Dec", "CC", "RW" );
   CASE( "fao_extra", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD1_2   = 0.0002", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "ENC|FITS-WCS", "ASC|FitsAxisOrder|X RA Y Dec", "CC", "RW" );
   CASE( "fao_swap_auto", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 20", "P|0|CRVAL2  = 10", "P|0|CD1_2   = -0.001", "P|0|CD2_1   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'DEC--TAN'", "P|0|CTYPE2  = 'RA---TAN'", "P|0|MJD-OBS = 55000.0", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "fao_swap_copy", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 20", "P|0|CRVAL2  = 10", "P|0|CD1_2   = -0.001", "P|0|CD2_1   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'DEC--TAN'", "P|0|CTYPE2  = 'RA---TAN'", "P|0|MJD-OBS = 55000.0", "ENC|FITS-WCS", "ASC|FitsAxisOrder|<copy>", "CC", "RW" );
   CASE( "fao_lonpole_auto", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD1_2   = 0.0002", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "P|0|LONPOLE = 90.0", "ENC|FITS-WCS", "CC", "RW" );
   CASE( "fao_lonpole_copy", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD1_2   = 0.0002", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "P|0|LONPOLE = 90.0", "ENC|FITS-WCS", "ASC|FitsAxisOrder|<copy>", "CC", "RW" );
   CASE( "fao_spec_order", "P|0|NAXIS   = 3", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|NAXIS3  = 10", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRPIX3  = 1", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CRVAL3  = 1.0E9", "P|0|CDELT1  = -0.001", "P|0|CDELT2  = 0.001", "P|0|CDELT3  = 1.0E6", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|CTYPE3  = 'FREQ'", "P|0|SPECSYS = 'BARYCENT'", "P|0|MJD-OBS = 55000.0", "ENC|FITS-WCS", "ASC|FitsAxisOrder|Freq Dec RA", "CC", "RW" );
   CASE( "fao_auto_spaced", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CD1_1   = -0.001", "P|0|CD1_2   = 0.0002", "P|0|CD2_2   = 0.001", "P|0|RADESYS = 'ICRS'", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|MJD-OBS = 55000.0", "ENC|FITS-WCS", "ASC|FitsAxisOrder|<AUTO> ", "CC", "RW" );
   CASE( "fao_accessors", "AC|FitsAxisOrder", "ASC|FitsAxisOrder|RA Dec", "AC|FitsAxisOrder", "AX|FitsAxisOrder", "AC|FitsAxisOrder" );
   CASE( "warnings_tab_separated", "ASC|Warnings|BadKeyName\tBadKeyValue", "P|0|BAD%NAME= 1", "WN" );
   CASE( "warnings_bad_alt", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|RADESYS = 'ICRS'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|CTYPE1A = 'RA---TAN'", "P|0|CTYPE2A = 'GLAT-TAN'", "P|0|CRVAL1A = 1", "P|0|CRVAL2A = 2", "P|0|CRPIX1A = 1", "P|0|CRPIX2A = 1", "ENC|FITS-WCS", "ASI|IgnoreBadAlt|1", "ASC|Warnings|BadAlt", "CC", "R", "WN" );
   CASE( "warnings_bad_alt_default", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|RADESYS = 'ICRS'", "P|0|CD1_1   = -0.001", "P|0|CD2_2   = 0.001", "P|0|CTYPE1A = 'RA---TAN'", "P|0|CTYPE2A = 'GLAT-TAN'", "P|0|CRVAL1A = 1", "P|0|CRVAL2A = 2", "P|0|CRPIX1A = 1", "P|0|CRPIX2A = 1", "ENC|FITS-WCS", "ASI|IgnoreBadAlt|1", "CC", "R", "WN" );
   CASE( "warnings_bad_mat_default", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|RADESYS = 'ICRS'", "P|0|CD1_1   = 0.0", "P|0|CD2_2   = 0.001", "ENC|FITS-WCS", "CC", "R", "WN" );
   CASE( "warnings_bad_mat_silenced", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|RADESYS = 'ICRS'", "P|0|CD1_1   = 0.0", "P|0|CD2_2   = 0.001", "ENC|FITS-WCS", "ASC|Warnings|BadCel", "CC", "R", "WN" );
   CASE( "warnings_clean", "P|0|NAXIS   = 2", "P|0|NAXIS1  = 100", "P|0|NAXIS2  = 200", "P|0|CRPIX1  = 50", "P|0|CRPIX2  = 100", "P|0|CRVAL1  = 10", "P|0|CRVAL2  = 20", "P|0|CTYPE1  = 'RA---TAN'", "P|0|CTYPE2  = 'DEC--TAN'", "P|0|RADESYS = 'ICRS'", "P|0|CD1_1   = 0.0", "P|0|CD2_2   = 0.001", "ENC|FITS-WCS", "CL|1", "CC", "R", "WN" );
   CASE( "name_checks",
         "P|0|A       = 1", "P|0|A=1     = 5", "P|0|A=B", "P|0|AB CD   = 6",
         "P|0|Z       = 2" );
   CASE( "string_68_fits", "SS|0|S68|aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa|", "P|0|AFTER   = 1" );
   CASE( "string_69_fails", "SS|0|S69|aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa|", "P|0|AFTER   = 1", "CC",
         "GS|S69", "F|0|S69", "F|0|AFTER" );
   CASE( "string_quotes_fit", "SS|0|SQ|aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa''aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa|", "P|0|AFTER   = 1" );
   CASE( "string_quotes_fail", "SS|0|SQ|aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa''aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa|", "P|0|AFTER   = 1" );
   CASE( "string_trailing_blanks", "SS|0|SB|aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa   |", "P|0|AFTER   = 1" );
   CASE( "continue_69_fails", "SCN|0|CN69|aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa|", "P|0|AFTER   = 1" );
   CASE( "long_comment", "SS|0|SC|short|cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc", "SI|0|IC|1|cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
         "P|0|AFTER   = 1" );

   CASE( "source_read_on_first_use",
         "NEW|A       = 1~B       = 2", "P|0|C       = 3" );
   CASE( "source_read_fits_batches",
         "NEW|A       = 1~B       = 2~#~C       = 3~D       = 4~#~E       = 5",
         "CC", "RDF", "F|1|%f", "RDF", "RDF", "RDF" );
   CASE( "source_bad_card",
         "NEW|A       = 1~B       = 'x~C       = 3", "CC", "RDF" );
   CASE( "write_fits_to_sink",
         "NEW|A       = 1~B       = 2", "WRF", "P|0|Z       = 9" );
   CASE( "write_fits_without_sink", "P|0|A       = 1", "WRF" );
   CASE( "write_fits_attributes",
         "ASC|Warnings|BadKeyName", "ASC|FitsAxisOrder|<copy>",
         "P|0|A       = 1", "WRF", "AC|Warnings", "AC|FitsAxisOrder" );
   CASE( "empty_fits_attributes",
         "ASC|Warnings|BadKeyName", "ASC|FitsAxisOrder|<copy>",
         "P|0|A       = 1", "E", "AC|Warnings", "AC|FitsAxisOrder" );
   CASE( "put_cards_attributes",
         "ASC|Warnings|BadKeyName", "PC|AAA     = 1", "AC|Warnings" );
   CASE( "sink_skips_used_cards",
         "NEW|A       = 1", "W", "CC", "R" );
   CASE( "sink_skips_purged_cards",
         "NEW|CRPIX1  = 1~A       = 1", "PW" );
   CASE( "source_file",
         "P|0|A       = 1", "CC",
         "MKF|fcp_src.txt|B       = 2  ~C       = 3\r~~HISTORY hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh~D       = 4",
         "SRF|fcp_src.txt" );
   CASE( "source_file_missing", "P|0|A       = 1", "SRF|fcp_none.txt" );
   CASE( "sink_file", "P|0|A       = 1", "SKF|fcp_sink.txt" );
   CASE( "sink_file_replaces_sink",
         "NEW|X       = 1", "SKF|fcp_sink2.txt" );
   CASE( "sink_file_write_fits",
         "P|0|A       = 1", "SKF|fcp_sink3.txt", "WRF", "CATF|fcp_sink3.txt",
         "P|0|B       = 2" );
   CASE( "sink_file_unopenable",
         "P|0|A       = 1", "SKF|fcp_nodir/x.txt", "WRF", "AX|SinkFile" );
   CASE( "copy_has_no_source_or_sink",
         "NEW|A       = 1~B       = 2", "CPN", "SKF|fcp_sink4.txt", "CPN" );
   CASE( "skip_and_full_defaults", "AI|Skip", "AI|Full" );
   CASE( "native_full_minus1", "WF" );
   CASE( "native_full_0", "ASI|Full|0", "WF", "CC", "R" );
   CASE( "native_full_1", "ASI|Full|1", "WF" );
   CASE( "native_no_comments", "ASI|Comment|0", "WF" );
   CASE( "native_skip_0", "P|0|A       = 1", "W", "CC", "ASI|Skip|0", "R" );
   CASE( "native_skip_0_from_begin", "W", "CC", "ASI|Skip|0", "R" );
   CASE( "native_skip_0_data_item", "P|0|NAXIS1  = 2", "W", "CC", "ASI|Skip|0", "R" );
   CASE( "native_skip_0_clean", "P|0|A       = 1", "W", "CC", "ASI|Skip|0", "CL|1", "R" );
   CASE( "native_skip_0_clean_eof", "CM|0|hello", "SU|0|U|", "CC", "ASI|Skip|0", "CL|1", "R" );
   CASE( "native_skip_0_clean_comments", "CM|0|hello", "CC", "ASI|Skip|0", "CL|1", "R" );
   CASE( "native_unknown_item", "P|0|BEGAST_A= 'ZoomMap '", "P|0|NIN_A   = 1", "P|0|ISA_A   = 'Mapping '",
         "P|0|FOO_A   = 3", "P|0|ZOOM_A  = 2", "P|0|ENDAST_A= 'ZoomMap '", "P|0|MARK    = 1",
         "ENC|NATIVE", "CC", "R", "WN" );
   CASE( "native_unknown_item_strict", "P|0|BEGAST_A= 'ZoomMap '", "P|0|NIN_A   = 1", "P|0|ISA_A   = 'Mapping '",
         "P|0|FOO_A   = 3", "P|0|ZOOM_A  = 2", "P|0|ENDAST_A= 'ZoomMap '", "P|0|MARK    = 1",
         "ENC|NATIVE", "ASI|Strict|1", "CC", "R", "WN" );
   CASE( "native_bad_value", "P|0|BEGAST_A= 'ZoomMap '", "P|0|NIN_A   = 1", "P|0|ISA_A   = 'Mapping '",
         "P|0|ZOOM_A  = 'x'", "P|0|ENDAST_A= 'ZoomMap '", "P|0|MARK    = 1",
         "ENC|NATIVE", "CC", "R" );
   CASE( "native_bad_value_clean", "P|0|BEGAST_A= 'ZoomMap '", "P|0|NIN_A   = 1", "P|0|ISA_A   = 'Mapping '",
         "P|0|ZOOM_A  = 'x'", "P|0|ENDAST_A= 'ZoomMap '", "P|0|MARK    = 1",
         "ENC|NATIVE", "CL|1", "CC", "R" );
   CASE( "native_full_0_nested", "ASI|Full|0", "WC", "CC", "R" );
   CASE( "native_full_0_cmpframe", "ASI|Full|0", "WCF" );
   CASE( "native_full_minus1_nested", "WC" );
   CASE( "native_long_title", "WFT|tttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttt", "CC", "R" );
   CASE( "native_long_title_no_comments", "ASI|Comment|0", "WFT|tttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttttt", "CC", "R" );

   printf( "];\n" );
   return 0;
}
