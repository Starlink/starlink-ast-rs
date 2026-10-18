/* Write the Mappings of the Roman benchmark, as C's own Native dumps.

     gen_roman_fixtures <output directory>

   Built by tools/perf/build_c_engine.sh (bench variant). The Mappings and
   their constants are those of perf/bench_simd.c in Starlink/ast pull
   request 51 (https://github.com/Starlink/ast/pull/51), which times them
   over random inputs for the Roman Space Telescope's WCS; `ast-bench roman`
   loads these dumps into both engines and repeats that benchmark. */
#include <stdio.h>
#include <stdlib.h>
#include "ast.h"

static FILE *sink_fp;
static void sink( const char *line ) { fprintf( sink_fp, "%s\n", line ); }

/* Degree-5 2-D PolyMap (Roman WCS SIP-like distortion, 10 terms); columns
   are coefficient, output, power of x, power of y. */
static const double POLY5_COEFF_F[ 10*4 ] = {
    0.11034133, 1, 1, 0,
   -3.0e-8,     1, 2, 0,
    3.4168e-4,  1, 0, 1,
   -1.0e-8,     1, 0, 2,
    1.4e-7,     1, 1, 1,
    3.1436e-4,  2, 1, 0,
    7.0e-8,     2, 2, 0,
    0.10828278, 2, 0, 1,
    2.1e-7,     2, 0, 2,
   -2.0e-8,     2, 1, 1,
};

/* Degree-1 2-D PolyMap (Roman WCS linear transform, 4 terms). */
static const double POLY1_COEFF_F[ 4*4 ] = {
   -0.5,       1, 1, 0,
   -0.8660254, 1, 0, 1,
   -0.8660254, 2, 1, 0,
    0.5,       2, 0, 1,
};

static const double DIAG2X2[ 2 ] = { 2.0, 0.5 };
static const double ROT2X2[ 4 ] = { -0.5, -0.8660254, -0.8660254, 0.5 };

static AstMapping *sphmap( void ) { return (AstMapping *) astSphMap( "UnitRadius=1" ); }
static AstMapping *poly5( void ) {
   return (AstMapping *) astPolyMap( 2, 2, 10, POLY5_COEFF_F, 0, NULL, " " );
}
static AstMapping *poly1( void ) {
   return (AstMapping *) astPolyMap( 2, 2, 4, POLY1_COEFF_F, 0, NULL, " " );
}
static AstMapping *matdiag2( void ) {
   return (AstMapping *) astMatrixMap( 2, 2, 1, DIAG2X2, " " );
}
static AstMapping *matfull2( void ) {
   return (AstMapping *) astMatrixMap( 2, 2, 0, ROT2X2, " " );
}

static const struct { const char *name; AstMapping *(* build)( void ); } FIXTURES[] = {
   { "sphmap.map", sphmap },
   { "poly5.map", poly5 },
   { "poly1.map", poly1 },
   { "matdiag2.map", matdiag2 },
   { "matfull2.map", matfull2 },
};

int main( int argc, char **argv ) {
   char path[ 4096 ];
   if( argc != 2 ) {
      fprintf( stderr, "usage: %s <output directory>\n", argv[ 0 ] );
      return 2;
   }
   for( size_t f = 0; f < sizeof FIXTURES/sizeof FIXTURES[ 0 ]; f++ ) {
      astBegin;
      AstMapping *map = FIXTURES[ f ].build();
      if( !astOK || !map ) {
         fprintf( stderr, "%s: construction failed\n", FIXTURES[ f ].name );
         return 1;
      }
      snprintf( path, sizeof path, "%s/%s", argv[ 1 ], FIXTURES[ f ].name );
      sink_fp = fopen( path, "w" );
      if( !sink_fp ) { perror( path ); return 1; }
      AstChannel *ch = astChannel( NULL, sink, " " );
      if( astWrite( ch, map ) != 1 || !astOK ) {
         fprintf( stderr, "%s: write failed\n", FIXTURES[ f ].name );
         return 1;
      }
      fclose( sink_fp );
      astEnd;
   }
   printf( "wrote %zu fixtures\n", sizeof FIXTURES/sizeof FIXTURES[ 0 ] );
   return 0;
}
