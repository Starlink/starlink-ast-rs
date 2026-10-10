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
 *   R                    astRead
 *   W                    astWrite of a 2-D ZoomMap of factor 2
 *
 * After each operation it prints the result and the state: Card, Ncard,
 * Nkey, CardType, CardName and CardComm, then every card astFindFits
 * returns from the start, as Rust source for
 * crates/ast-core/tests/fitschan_cursor_oracle/cases.rs:
 *
 *   ./fitschan_cursor_probe > ../../crates/ast-core/tests/fitschan_cursor_oracle/cases.rs
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

static const char *code_name( int status ) {
   if( status == AST__BDFTS ) return "Bdfts";
   if( status == AST__FTCNV ) return "Ftcnv";
   if( status == AST__FUNDEF ) return "Fundef";
   if( status == AST__BDFMT ) return "Bdfmt";
   if( status == AST__ATTIN ) return "Attin";
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
   char f0[ 400 ], f1[ 400 ], f2[ 400 ], f3[ 400 ], f4[ 400 ], buf[ 500 ];
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
   } else if( !strcmp( f0, "R" ) ) {
      AstObject *obj = astRead( fc );
      snprintf( buf, sizeof buf, "= %s\n", obj ? astGetC( obj, "Class" ) : "null" );
      if( obj ) obj = astAnnul( obj );
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
      putchar( *s );
   }
   putchar( '"' );
}

static void emit( const char *name, const char **ops, int *status ) {
   AstFitsChan *fc = astFitsChan( NULL, NULL, " " );
   int i;
   nout = 0;
   out[ 0 ] = 0;
   for( i = 0; ops[ i ]; i++ ) run( fc, ops[ i ], status );
   fc = astAnnul( fc );
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
   CASE( "current_card_values",
         "P|0|A       = 1", "P|0|B       = 'x'", "SC|1", "GI|-", "GS|-",
         "SC|2", "GS|-", "GI|-", "SC|3", "GI|-" );
   CASE( "typed_setters",
         "SK|0|KV|5|small", "SK|0|KB|12345678901|", "SCN|0|CN|long text|c",
         "SCF|0|CFV|1.5|-2.5|cf", "SCI|0|CIV|3|-4|ci", "CC", "GK|KV",
         "GCN|CN", "GCF|CFV", "GCI|CIV" );

   printf( "];\n" );
   return 0;
}
