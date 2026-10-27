/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* Decompose the fixed cost of a one-point astTranN call in the reference C
   library.

     prof_tran <fixture root> <fixture> <mode>|all [seconds]

   Built by tools/perf/build_c_engine.sh into target/perf/c/<variant>/prof_tran
   for the bench variant and the nothreads variant, the same build with
   AST_WITH_PTHREADS=OFF, so the two show what thread safety costs.
   <fixture> is a Native dump relative to <fixture root>, as ast-bench names
   its cases (crates/ast-bench/fixtures and unitmap_2d.map, say).
   Each mode repeats one operation on the loaded Mapping for <seconds>
   (default 2) and prints its mean cost in nanoseconds:

     tran       astTranN forward at one point, the public call ast-bench times
     pointset   what TranN wraps the caller's arrays in: astPointSet,
                astSetPoints and astDelete, twice
     malloc     TranN's two astMalloc/astFree pairs for its pointer arrays
     transform  the protected astTransform on PointSets built once, the part
                of a TranN call that is the Mapping's arithmetic
     getnin     astGetI( map, "Nin" ): one public call, handle lookup included
     getnin_p   the protected astGetNin on the Object pointer
     loop       an empty loop, the clock floor the other modes include

   `all` runs every mode in turn.  The input point is 0.3, 0.4, 0.5, ... on
   the Mapping's axes; a fixture whose domain excludes it (a LutMap, a
   TimeMap) transforms it to AST__BAD, which the tran line reports.
   The protected entry points are called on the Object pointer that
   astMakePointer returns, as docs/issues/c-oracle-probe-recipe.md describes.
   To see where the cycles go inside one mode, run it for longer and sample
   it; crates/ast-bench/README.md has the commands. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "ast.h"
#include "transform_oracle.h"

/* Protected entry points.  ast.h declares astSetPoints_; the rest are
   declared here, with the status pointer every protected call takes. */
AstPointSet *astPointSet_( AstDim npoint, int ncoord, const char *options,
                           int *status, ... );
void *astDelete_( void *object, int *status );
void *astTransform_( void *mapping, AstPointSet *in, int forward,
                     AstPointSet *out, int *status );
int astGetNin_( void *mapping, int *status );

#define MAXAXES 32
#define BATCH 1000

typedef enum { TRAN, POINTSET, MALLOC, TRANSFORM, GETNIN, GETNIN_P, LOOP, NMODES } Mode;

static const char *const mode_names[ NMODES ] = {
   "tran", "pointset", "malloc", "transform", "getnin", "getnin_p", "loop"
};

/* The loaded Mapping and the one-point buffers every mode works on. */
typedef struct {
   AstMapping *map;      /* the public handle */
   void *raw;            /* the Object pointer behind it */
   int nin, nout;
   double in[ MAXAXES ], out[ MAXAXES ];
   double *ipp[ MAXAXES ], *opp[ MAXAXES ];   /* one pointer per axis */
   AstPointSet *pin, *pout;                    /* built once, for transform */
   int status;
} Ctx;

/* Sinks that keep the compiler from discarding a result. */
static volatile double sink;
static volatile int isink;

static double now( void ) {
   struct timespec ts;
   clock_gettime( CLOCK_MONOTONIC, &ts );
   return ts.tv_sec + 1e-9 * ts.tv_nsec;
}

/* Repeat `body` in batches until `seconds` have passed, leaving the batch
   count in `iters` and the start and end times in `t0` and `t`. */
#define TIMED( body ) \
   do { \
      iters = 0; \
      t0 = now(); \
      do { \
         for ( int k = 0; k < BATCH; k++ ) { body; } \
         iters += BATCH; \
         t = now(); \
      } while ( t - t0 < seconds ); \
   } while ( 0 )

/* Time one mode and return its mean cost per call in nanoseconds. */
static double time_mode( Mode mode, Ctx *c, double seconds ) {
   long iters = 0;
   double t0 = 0.0, t = 0.0;
   int *status = &c->status;
   switch ( mode ) {
   case TRAN:
      TIMED( astTranN( c->map, 1, c->nin, 1, c->in, 1, c->nout, 1, c->out );
             sink += c->out[ 0 ] );
      break;
   case POINTSET:
      TIMED( AstPointSet *a = astPointSet_( 1, c->nin, "", status );
             AstPointSet *b = astPointSet_( 1, c->nout, "", status );
             astSetPoints_( a, c->ipp, status );
             astSetPoints_( b, c->opp, status );
             a = astDelete_( a, status );
             b = astDelete_( b, status ) );
      break;
   case MALLOC:
      TIMED( void *a = astMalloc( sizeof( double * ) * c->nin );
             void *b = astMalloc( sizeof( double * ) * c->nout );
             a = astFree( a );
             b = astFree( b ) );
      break;
   case TRANSFORM:
      TIMED( (void) astTransform_( c->raw, c->pin, 1, c->pout, status );
             sink += c->out[ 0 ] );
      break;
   case GETNIN:
      TIMED( isink += astGetI( c->map, "Nin" ) );
      break;
   case GETNIN_P:
      TIMED( isink += astGetNin_( c->raw, status ) );
      break;
   case LOOP:
      TIMED( sink += c->in[ 0 ] );
      break;
   case NMODES:
      break;
   }
   return 1e9 * ( t - t0 ) / iters;
}

int main( int argc, char **argv ) {
   if ( argc < 4 || argc > 5 ) {
      fprintf( stderr, "usage: prof_tran <fixture root> <fixture> <mode>|all [seconds]\n"
                       "modes:" );
      for ( int m = 0; m < NMODES; m++ ) fprintf( stderr, " %s", mode_names[ m ] );
      fprintf( stderr, "\n" );
      return 2;
   }
   double seconds = argc > 4 ? atof( argv[ 4 ] ) : 2.0;
   if ( !( seconds > 0.0 ) ) {
      fprintf( stderr, "prof_tran: seconds must be positive\n" );
      return 2;
   }
   int first = 0, last = NMODES - 1;
   if ( strcmp( argv[ 3 ], "all" ) ) {
      for ( first = 0; first < NMODES; first++ ) {
         if ( !strcmp( argv[ 3 ], mode_names[ first ] ) ) break;
      }
      if ( first == NMODES ) {
         fprintf( stderr, "prof_tran: no mode named %s\n", argv[ 3 ] );
         return 2;
      }
      last = first;
   }

   Ctx c;
   memset( &c, 0, sizeof( c ) );
   oracle_register_intramaps( &c.status );
   c.map = oracle_load_mapping( argv[ 1 ], argv[ 2 ], NULL, NULL );
   if ( !c.map || !astOK ) {
      fprintf( stderr, "prof_tran: cannot load %s under %s\n", argv[ 2 ], argv[ 1 ] );
      return 1;
   }
   c.raw = astMakePointer( c.map );
   c.nin = astGetI( c.map, "Nin" );
   c.nout = astGetI( c.map, "Nout" );
   if ( c.nin > MAXAXES || c.nout > MAXAXES ) {
      fprintf( stderr, "prof_tran: more than %d axes\n", MAXAXES );
      return 1;
   }
   for ( int i = 0; i < c.nin; i++ ) {
      c.in[ i ] = 0.3 + 0.1 * i;
      c.ipp[ i ] = c.in + i;
   }
   for ( int i = 0; i < c.nout; i++ ) c.opp[ i ] = c.out + i;
   c.pin = astPointSet_( 1, c.nin, "", &c.status );
   c.pout = astPointSet_( 1, c.nout, "", &c.status );
   astSetPoints_( c.pin, c.ipp, &c.status );
   astSetPoints_( c.pout, c.opp, &c.status );

   /* One transform first, so that the tran line can report the output. */
   astTranN( c.map, 1, c.nin, 1, c.in, 1, c.nout, 1, c.out );
   double out0 = c.out[ 0 ];
   if ( !astOK || c.status ) {
      fprintf( stderr, "prof_tran: the transform failed\n" );
      return 1;
   }

   for ( int m = first; m <= last; m++ ) {
      double ns = time_mode( (Mode) m, &c, seconds );
      if ( !astOK || c.status ) {
         fprintf( stderr, "prof_tran: AST error in mode %s\n", mode_names[ m ] );
         return 1;
      }
      printf( "%-24s %-10s %8.1f ns per call", argv[ 2 ], mode_names[ m ], ns );
      if ( m == TRAN ) {
         if ( out0 == AST__BAD ) printf( "   (output is AST__BAD)" );
         else printf( "   (out[0] = %g)", out0 );
      }
      printf( "\n" );
   }
   return 0;
}
