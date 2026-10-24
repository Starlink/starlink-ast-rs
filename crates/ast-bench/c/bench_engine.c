/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* The reference C library as an ast-bench engine.  See bench_engine.h. */
#include <stdio.h>
#include <string.h>
#include "ast.h"
#include "transform_oracle.h"
#include "bench_engine.h"

static int registered = 0;

void *abe_load( const char *root, const char *relpath, char *err, int errlen ) {
   AstMapping *map;
   int status_value = 0;
   if( !registered ) {
      oracle_register_intramaps( &status_value );
      registered = 1;
   }
   astBegin;
   map = oracle_load_mapping( root, relpath, NULL, NULL );
   if( map && astOK ) astExempt( map );
   astEnd;
   if( !astOK || !map ) {
      snprintf( err, (size_t) errlen, "C could not load %s", relpath );
      astClearStatus;
      return NULL;
   }
   return map;
}

int abe_nin( void *map ) { return astGetI( (AstMapping *) map, "Nin" ); }
int abe_nout( void *map ) { return astGetI( (AstMapping *) map, "Nout" ); }

int abe_tran( void *map, int forward, int npoint, int ncoord_in,
              const double *in, int ncoord_out, double *out ) {
   astTranN( (AstMapping *) map, npoint, ncoord_in, npoint, in, forward,
             ncoord_out, npoint, out );
   if( !astOK ) {
      astClearStatus;
      return 1;
   }
   return 0;
}

void abe_free( void *map ) { astAnnul( (AstMapping *) map ); }

#ifdef AST_BENCH_COUNTS
void ast_counts_reset( void );
int ast_counts_dump( char *buf, int max );
int abe_counts_reset( void ) { ast_counts_reset(); return 0; }
int abe_counts_dump( char *buf, int max ) { return ast_counts_dump( buf, max ); }
#else
int abe_counts_reset( void ) { return -1; }
int abe_counts_dump( char *buf, int max ) { (void) buf; (void) max; return -1; }
#endif
