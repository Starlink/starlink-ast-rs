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
 *   SCD|0 or 1  XCD      set and clear CDMatrix
 *   R                    astRead
 *   W                    astWrite of a 2-D ZoomMap of factor 2
 *
 * After each operation it prints the result and the state: Card, Ncard,
 * Nkey, CardType, CardName and CardComm, then every card astFindFits
 * returns from the start, and the error if one stops it, as Rust source for
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

   printf( "];\n" );
   return 0;
}
