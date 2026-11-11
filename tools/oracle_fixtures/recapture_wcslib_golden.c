/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* recapture_wcslib_golden -- recompute every output of
 * crates/starlink-ast/tests/wcslib_golden/projections.tsv from the wcslib
 * C of the reference library (wcslib/proj.c, tpn.c and wcstrig.c).
 *
 *   ./recapture_wcslib_golden <golden file> <reference commit> > <new file>
 *
 * The golden file carries its own test vectors, so it is the input as well
 * as the output: the FI and RI lines (the shared inputs), the S lines (the
 * parameter sets) and the inputs of the X lines are kept as they are, and
 * the A, E, F and R lines and the results of the X lines are computed
 * afresh, whatever the file held for them. Comment lines and empty lines
 * are copied, except that the commit in the comment that names
 * "reference/starlink-ast at <commit>" becomes the commit given.
 *
 * The file's header describes each line; src/wcslib/golden_tests.rs, which
 * reads the file, fixes what each means, and this program calls the C as
 * that test calls the Rust:
 *
 * - a parameter set is a zeroed AstPrjPrm whose parameter arrays are filled
 *   as InitPrjPrm (src/wcsmap.c) fills a WcsMap's, and nothing else is set
 *   up in advance, so the first projection call runs the set-up;
 * - the set-up has failed if the projection's set function says so, and E
 *   then gives the status of a forward call and of a reverse call at
 *   (0, 0), each the first call on its own such struct;
 * - otherwise one struct serves every F, R and X line of the set.
 *
 * A line that is not of a known type, that names an unknown projection,
 * that is malformed where it holds inputs, or that is out of place, stops
 * the program with the line's number. Build it with
 * recapture_wcslib_golden.sh, which supplies the reference sources.
 */
#define _POSIX_C_SOURCE 200809L

#include <float.h>
#include <inttypes.h>
#include <math.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "proj.h"

/* The number of elements in each projection parameter array. */
#define SLOTS ( WCSLIB_MXPAR + 1 )

/* The number of parameters of each axis that an A line lists. */
#define A_PARAMS 7

/* The value AST stores for a parameter that has not been supplied. */
#define AST__BAD ( -DBL_MAX )

typedef int ( *SetFn )( struct AstPrjPrm * );
typedef int ( *PrjFn )( const double, const double, struct AstPrjPrm *, double *, double * );

/* A projection: its code and its wcslib functions. */
typedef struct {
   const char *code;
   SetFn set;
   PrjFn fwd;
   PrjFn rev;
} Projection;

#define PROJECTION( c ) { #c, ast##c##set, ast##c##fwd, ast##c##rev }

/* Every projection with wcslib functions of its own. NCP and GLS have none:
   a WcsMap turns them into SIN and SFL. */
static const Projection projections[] = {
   PROJECTION( AZP ), PROJECTION( SZP ), PROJECTION( TAN ), PROJECTION( STG ),
   PROJECTION( SIN ), PROJECTION( ARC ), PROJECTION( ZPN ), PROJECTION( ZEA ),
   PROJECTION( AIR ), PROJECTION( CYP ), PROJECTION( CEA ), PROJECTION( CAR ),
   PROJECTION( MER ), PROJECTION( SFL ), PROJECTION( PAR ), PROJECTION( MOL ),
   PROJECTION( AIT ), PROJECTION( COP ), PROJECTION( COE ), PROJECTION( COD ),
   PROJECTION( COO ), PROJECTION( BON ), PROJECTION( PCO ), PROJECTION( TSC ),
   PROJECTION( CSC ), PROJECTION( QSC ), PROJECTION( HPX ), PROJECTION( XPH ),
   PROJECTION( TPN )
};

/* A parameter set, as its S line gives it. */
typedef struct {
   const Projection *proj;
   size_t stride;
   int tpn_tan;
/* The supplied parameters: index 0 for the latitude axis, 1 for the
   longitude axis. */
   int supplied[ 2 ][ SLOTS ];
   double value[ 2 ][ SLOTS ];
} Set;

/* A projection struct with the storage of its parameter arrays. */
typedef struct {
   struct AstPrjPrm prj;
   double p[ SLOTS ];
   double p2[ SLOTS ];
} Prj;

/* A list of input pairs. */
typedef struct {
   double ( *v )[ 2 ];
   size_t n;
   size_t cap;
} Pairs;

/* Where the lines of the file after the shared inputs stand within a
   parameter set. */
typedef enum {
   SHARED,       /* Before the first S line */
   WANT_A,       /* After an S line */
   WANT_RESULT,  /* After an A line */
   IN_F,         /* Within the F lines */
   IN_R,         /* Within the R lines */
   IN_X,         /* After the last R line */
   AFTER_E       /* After an E line */
} State;

static const char *path;
static size_t lineno;

/* The allocated size of a projection parameter array, which astZPNset
   reads through the astSizeOf macro to find the highest coefficient. The
   reference library asks its memory manager; here every such array is the
   p or p2 of a Prj. */
size_t astSizeOf_( const void *ptr, int *status );
size_t astSizeOf_( const void *ptr, int *status ) {
   (void) ptr;
   (void) status;
   return SLOTS*sizeof( double );
}

/* Report an error at the current line and stop. */
static _Noreturn void fail( const char *fmt, ... ) {
   va_list args;
   fprintf( stderr, "%s:%zu: ", path, lineno );
   va_start( args, fmt );
   vfprintf( stderr, fmt, args );
   va_end( args );
   fputc( '\n', stderr );
   exit( 1 );
}

/* Split "line" in place at each space, storing up to "max" tokens, and
   return the number of tokens found. */
static size_t split( char *line, char **tok, size_t max ) {
   size_t n = 0;
   char *start = line;
   char *c;
   for( c = line; ; c++ ) {
      if( *c == ' ' || *c == '\0' ) {
         int last = ( *c == '\0' );
         *c = '\0';
         if( n < max ) tok[ n ] = start;
         n++;
         start = c + 1;
         if( last ) break;
      }
   }
   return n;
}

/* The double a token stands for: 16 lower-case hex digits are its bit
   pattern and "nan" is a quiet NaN. Returns 0 for any other token. */
static int parse_bits( const char *tok, double *out ) {
   uint64_t bits = 0;
   size_t i;
   if( !strcmp( tok, "nan" ) ) {
      bits = UINT64_C( 0x7ff8000000000000 );
   } else {
      if( strlen( tok ) != 16 ) return 0;
      for( i = 0; i < 16; i++ ) {
         const char *digit = strchr( "0123456789abcdef", tok[ i ] );
         if( !digit ) return 0;
         bits = ( bits << 4 ) | (uint64_t) ( digit - "0123456789abcdef" );
      }
   }
   memcpy( out, &bits, sizeof( bits ) );
   return 1;
}

/* A decimal integer of digits only, no greater than "max". Returns 0 for
   any other token. */
static int parse_count( const char *tok, size_t max, size_t *out ) {
   size_t v = 0;
   size_t digit;
   const char *c;
   if( !*tok ) return 0;
   for( c = tok; *c; c++ ) {
      if( *c < '0' || *c > '9' ) return 0;
      digit = (size_t) ( *c - '0' );
      if( digit > max || v > ( max - digit )/10 ) return 0;
      v = v*10 + digit;
   }
   *out = v;
   return 1;
}

/* Write a double as a space and the token that stands for it. */
static void put_bits( double v ) {
   uint64_t bits;
   if( isnan( v ) ) {
      printf( " nan" );
   } else {
      memcpy( &bits, &v, sizeof( bits ) );
      printf( " %016" PRIx64, bits );
   }
}

/* Add a pair to a list. */
static void push( Pairs *pairs, double a, double b ) {
   if( pairs->n == pairs->cap ) {
      pairs->cap = pairs->cap ? 2*pairs->cap : 64;
      pairs->v = realloc( pairs->v, pairs->cap*sizeof( *pairs->v ) );
      if( !pairs->v ) fail( "out of memory" );
   }
   pairs->v[ pairs->n ][ 0 ] = a;
   pairs->v[ pairs->n ][ 1 ] = b;
   pairs->n++;
}

/* How many of "n" shared inputs a set uses: every stride-th from the
   first. */
static size_t used( size_t n, size_t stride ) {
   return n ? ( n - 1 )/stride + 1 : 0;
}

/* Read the fields of an S line after its type: the projection code, the
   stride, the TPNTan flag and the parameters. */
static void parse_set( char **tok, Set *set ) {
   size_t i;
   char *pv;

   memset( set, 0, sizeof( *set ) );
   for( i = 0; i < sizeof( projections )/sizeof( *projections ); i++ ) {
      if( !strcmp( tok[ 0 ], projections[ i ].code ) ) set->proj = projections + i;
   }
   if( !set->proj ) fail( "unknown projection code \"%s\"", tok[ 0 ] );

   if( !parse_count( tok[ 1 ], SIZE_MAX, &set->stride ) || set->stride < 1 ) {
      fail( "the stride \"%s\" is not a positive integer", tok[ 1 ] );
   }

   if( !strcmp( tok[ 2 ], "1" ) ) {
      set->tpn_tan = 1;
   } else if( strcmp( tok[ 2 ], "0" ) ) {
      fail( "the TPNTan flag \"%s\" is neither 0 nor 1", tok[ 2 ] );
   }

/* The parameters are "-" for none, or axis:m:value items separated by
   commas, with axis 2 the latitude axis and axis 1 the longitude axis. */
   pv = tok[ 3 ];
   if( strcmp( pv, "-" ) ) {
      while( pv ) {
         char *next = strchr( pv, ',' );
         char *m_tok, *v_tok;
         int axis;
         size_t m;
         if( next ) *next++ = '\0';
         m_tok = strchr( pv, ':' );
         v_tok = m_tok ? strchr( m_tok + 1, ':' ) : NULL;
         if( !v_tok ) fail( "the parameter \"%s\" is not axis:m:value", pv );
         *m_tok++ = '\0';
         *v_tok++ = '\0';
         if( !strcmp( pv, "2" ) ) {
            axis = 0;
         } else if( !strcmp( pv, "1" ) ) {
            axis = 1;
         } else {
            fail( "the parameter axis \"%s\" is neither 1 nor 2", pv );
         }
         if( !parse_count( m_tok, WCSLIB_MXPAR, &m ) ) {
            fail( "the parameter index \"%s\" is not an integer from 0 to %d",
                  m_tok, WCSLIB_MXPAR );
         }
         if( !parse_bits( v_tok, &set->value[ axis ][ m ] ) ) {
            fail( "the parameter value \"%s\" is neither 16 hex digits nor nan", v_tok );
         }
         set->supplied[ axis ][ m ] = 1;
         pv = next;
      }
   }
}

/* Make "prj" a projection struct for a parameter set on which no
   projection function has been called: zeroed but for its parameter
   arrays, which hold the supplied parameters and the defaults of the rest,
   as InitPrjPrm (src/wcsmap.c) leaves those of a WcsMap. */
static void fresh( const Set *set, Prj *prj ) {
   const char *code = set->proj->code;
   int undefaultable;
   int i;

   memset( prj, 0, sizeof( *prj ) );
   prj->prj.p = prj->p;
   prj->prj.p2 = prj->p2;

/* Make the set-up compute its constants afresh. */
   prj->prj.flag = 0;
   prj->prj.r0 = 0;

/* TPN's "n" says whether the transformation includes the TAN projection or
   only the polynomial correction. */
   if( !strcmp( code, "TPN" ) ) prj->prj.n = set->tpn_tan;

/* Start with every parameter unset, then copy those supplied. */
   for( i = 0; i < SLOTS; i++ ) {
      prj->p[ i ] = set->supplied[ 0 ][ i ] ? set->value[ 0 ][ i ] : AST__BAD;
      prj->p2[ i ] = set->supplied[ 1 ][ i ] ? set->value[ 1 ][ i ] : AST__BAD;
   }

/* Defaults for the parameters that do not default to zero. */
   if( !strcmp( code, "SZP" ) ) {
      if( prj->p[ 3 ] == AST__BAD ) prj->p[ 3 ] = 90.0;

   } else if( !strcmp( code, "AIR" ) ) {
      if( prj->p[ 1 ] == AST__BAD ) prj->p[ 1 ] = 90.0;

   } else if( !strcmp( code, "CYP" ) ) {
      if( prj->p[ 1 ] == AST__BAD ) prj->p[ 1 ] = 1.0;
      if( prj->p[ 2 ] == AST__BAD ) prj->p[ 2 ] = 1.0;

   } else if( !strcmp( code, "CEA" ) ) {
      if( prj->p[ 1 ] == AST__BAD ) prj->p[ 1 ] = 1.0;

   } else if( !strcmp( code, "TPN" ) ) {
      if( prj->p[ 1 ] == AST__BAD ) prj->p[ 1 ] = 1.0;
      if( prj->p2[ 1 ] == AST__BAD ) prj->p2[ 1 ] = 1.0;

   } else if( !strcmp( code, "HPX" ) ) {
      if( prj->p[ 1 ] == AST__BAD ) prj->p[ 1 ] = 4.0;
      if( prj->p[ 2 ] == AST__BAD ) prj->p[ 2 ] = 3.0;
   }

/* Zero for the rest, except parameter 1 of Bonne and the conics, which has
   no default and stays bad. */
   undefaultable = !strcmp( code, "BON" ) || !strcmp( code, "COP" ) ||
                   !strcmp( code, "COE" ) || !strcmp( code, "COD" ) ||
                   !strcmp( code, "COO" );
   for( i = 0; i < SLOTS; i++ ) {
      if( i == 1 && undefaultable ) continue;
      if( prj->p[ i ] == AST__BAD ) prj->p[ i ] = 0.0;
      if( prj->p2[ i ] == AST__BAD ) prj->p2[ i ] = 0.0;
   }
}

/* Write the A line of a set from a struct that has been set up. */
static void put_a( const Prj *prj ) {
   int i;
   printf( "A" );
   for( i = 0; i < A_PARAMS; i++ ) put_bits( prj->p[ i ] );
   for( i = 0; i < A_PARAMS; i++ ) put_bits( prj->p2[ i ] );
   printf( "\n" );
}

/* Write the E line of a set whose set-up fails. */
static void put_e( const Set *set ) {
   Prj prj;
   double a = 0.0, b = 0.0;
   int fwd, rev;
   fresh( set, &prj );
   fwd = set->proj->fwd( 0.0, 0.0, &prj.prj, &a, &b );
   fresh( set, &prj );
   rev = set->proj->rev( 0.0, 0.0, &prj.prj, &a, &b );
   printf( "E %d %d\n", fwd, rev );
}

/* Write a status and, when it is 0, the two outputs, to end a line. */
static void put_result( int status, double a, double b ) {
   printf( " %d", status );
   if( status == 0 ) {
      put_bits( a );
      put_bits( b );
   }
   printf( "\n" );
}

/* Write the F line of a set for a forward input. */
static void put_f( const Set *set, Prj *prj, const double in[ 2 ] ) {
   double x = 0.0, y = 0.0;
   int status = set->proj->fwd( in[ 0 ], in[ 1 ], &prj->prj, &x, &y );
   printf( "F" );
   put_result( status, x, y );
}

/* Write the result of a reverse call to end an R or an X line. */
static void put_rev( const Set *set, Prj *prj, const double in[ 2 ] ) {
   double phi = 0.0, theta = 0.0;
   int status = set->proj->rev( in[ 0 ], in[ 1 ], &prj->prj, &phi, &theta );
   put_result( status, phi, theta );
}

/* Copy a comment line, putting "pin" in place of the commit in "at
   <commit>" after the reference library's path. Returns whether the line
   named the commit. */
static int put_comment( const char *line, const char *pin ) {
   static const char marker[] = "reference/starlink-ast at ";
   const char *at = strstr( line, marker );
   const char *commit;
   size_t len;
   if( at ) {
      commit = at + strlen( marker );
      len = strspn( commit, "0123456789abcdef" );
      if( len >= 7 ) {
         printf( "%.*s%s%s\n", (int) ( commit - line ), line, pin, commit + len );
         return 1;
      }
   }
   printf( "%s\n", line );
   return 0;
}

int main( int argc, char **argv ) {
   Pairs fwd_inputs = { NULL, 0, 0 };
   Pairs rev_inputs = { NULL, 0, 0 };
   State state = SHARED;
   Set set;
   Prj prj;
   FILE *in;
   char *line = NULL;
   char *text = NULL;
   size_t cap = 0;
   size_t text_cap = 0;
   ssize_t len;
   size_t nf = 0, nr = 0;       /* Shared inputs the current set uses */
   size_t k = 0;                /* F or R lines of the current set read */
   size_t set_line = 0;         /* The line of the current S line */
   size_t nsets = 0;
   int failed = 0;              /* Whether the current set's set-up fails */
   int pins = 0;
   const char *pin;

   if( argc != 3 ) {
      fprintf( stderr, "usage: %s <golden file> <reference commit>\n", argv[ 0 ] );
      return 1;
   }
   path = argv[ 1 ];
   pin = argv[ 2 ];
   if( strlen( pin ) < 7 || pin[ strspn( pin, "0123456789abcdef" ) ] ) {
      fprintf( stderr, "the reference commit \"%s\" is not 7 or more hex digits\n", pin );
      return 1;
   }
   in = fopen( path, "r" );
   if( !in ) {
      perror( path );
      return 1;
   }

   while( ( len = getline( &line, &cap, in ) ) > 0 ) {
      char *tok[ 6 ];
      size_t ntok;
      const char *type;
      double a, b;

      lineno++;
      if( strlen( line ) != (size_t) len ) fail( "the line holds a NUL character" );
      if( line[ len - 1 ] != '\n' ) fail( "the line does not end with a newline" );
      line[ len - 1 ] = '\0';

/* Comments and empty lines can stand anywhere. */
      if( line[ 0 ] == '#' ) {
         if( put_comment( line, pin ) && ++pins > 1 ) {
            fail( "a second comment names the reference commit" );
         }
         continue;
      } else if( line[ 0 ] == '\0' ) {
         printf( "\n" );
         continue;
      }

/* Keep the text of the line, which splitting it into tokens breaks up. */
      if( text_cap < (size_t) len ) {
         text_cap = (size_t) len;
         text = realloc( text, text_cap );
         if( !text ) fail( "out of memory" );
      }
      memcpy( text, line, (size_t) len );
      ntok = split( line, tok, sizeof( tok )/sizeof( *tok ) );
      type = tok[ 0 ];

/* A shared input. */
      if( !strcmp( type, "FI" ) || !strcmp( type, "RI" ) ) {
         if( state != SHARED ) fail( "a shared input follows a parameter set" );
         if( ntok != 3 || !parse_bits( tok[ 1 ], &a ) || !parse_bits( tok[ 2 ], &b ) ) {
            fail( "expected \"%s\" and two values, each 16 hex digits or nan", type );
         }
         push( type[ 0 ] == 'F' ? &fwd_inputs : &rev_inputs, a, b );
         printf( "%s\n", text );

/* A parameter set. */
      } else if( !strcmp( type, "S" ) ) {
         if( state == SHARED ) {
            if( !fwd_inputs.n ) fail( "no FI line precedes the first parameter set" );
            if( !rev_inputs.n ) fail( "no RI line precedes the first parameter set" );
         } else if( state != IN_X && state != AFTER_E ) {
            fail( "the parameter set of line %zu is incomplete", set_line );
         }
         if( ntok != 5 ) fail( "expected \"S\", a code, a stride, a flag and parameters" );
         parse_set( tok + 1, &set );
         set_line = lineno;
         nsets++;
         nf = used( fwd_inputs.n, set.stride );
         nr = used( rev_inputs.n, set.stride );
         printf( "%s\n", text );
         state = WANT_A;

/* The parameters of the set. Whether its set-up fails is what the
   projection's set function returns on a struct of its own, which then
   holds the parameters as the set-up leaves them; the struct the set's
   results come from starts afresh. */
      } else if( !strcmp( type, "A" ) ) {
         if( state != WANT_A ) fail( "an A line that does not follow an S line" );
         fresh( &set, &prj );
         failed = ( set.proj->set( &prj.prj ) != 0 );
         put_a( &prj );
         fresh( &set, &prj );
         state = WANT_RESULT;

/* The file has the set-up failing. If it now succeeds, the set's forward
   and reverse results at the shared inputs take the line's place. */
      } else if( !strcmp( type, "E" ) ) {
         if( state != WANT_RESULT ) fail( "an E line that does not follow an A line" );
         if( failed ) {
            put_e( &set );
         } else {
            fprintf( stderr, "%s:%zu: the set-up of this set now succeeds: F and R "
                     "lines replace its E line\n", path, set_line );
            for( k = 0; k < nf; k++ ) put_f( &set, &prj, fwd_inputs.v[ k*set.stride ] );
            for( k = 0; k < nr; k++ ) {
               printf( "R" );
               put_rev( &set, &prj, rev_inputs.v[ k*set.stride ] );
            }
         }
         state = AFTER_E;

/* The file has the set-up succeeding. If it now fails, an E line takes the
   place of the set's first result and the rest are dropped. */
      } else if( !strcmp( type, "F" ) ) {
         if( state == WANT_RESULT ) {
            state = IN_F;
            k = 0;
            if( failed ) {
               fprintf( stderr, "%s:%zu: the set-up of this set now fails: an E line "
                        "replaces its F, R and X lines\n", path, set_line );
               put_e( &set );
            }
         }
         if( state != IN_F ) fail( "an F line out of place" );
         if( !failed ) put_f( &set, &prj, fwd_inputs.v[ k*set.stride ] );
         if( ++k == nf ) {
            state = IN_R;
            k = 0;
         }

      } else if( !strcmp( type, "R" ) ) {
         if( state != IN_R ) fail( "an R line out of place" );
         if( !failed ) {
            printf( "R" );
            put_rev( &set, &prj, rev_inputs.v[ k*set.stride ] );
         }
         if( ++k == nr ) state = IN_X;

/* A reverse point of the set's own: its inputs stay as they are written. */
      } else if( !strcmp( type, "X" ) ) {
         double xy[ 2 ];
         if( state != IN_X ) fail( "an X line out of place" );
         if( ntok < 3 || !parse_bits( tok[ 1 ], xy ) || !parse_bits( tok[ 2 ], xy + 1 ) ) {
            fail( "expected \"X\" and two inputs, each 16 hex digits or nan" );
         }
         if( !failed ) {
            printf( "X %s %s", tok[ 1 ], tok[ 2 ] );
            put_rev( &set, &prj, xy );
         }

      } else {
         fail( "unknown line type \"%s\"", type );
      }
   }

   if( ferror( in ) ) {
      perror( path );
      return 1;
   }
   if( !nsets ) fail( "the file holds no parameter set" );
   if( state != IN_X && state != AFTER_E ) {
      fail( "the file ends within the parameter set of line %zu", set_line );
   }
   if( !pins ) fail( "no comment names the reference commit (\"reference/starlink-ast at ...\")" );

   fclose( in );
   free( line );
   free( text );
   free( fwd_inputs.v );
   free( rev_inputs.v );
   if( fflush( stdout ) || ferror( stdout ) ) {
      perror( "stdout" );
      return 1;
   }
   return 0;
}
