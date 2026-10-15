/* The C ABI ast-bench loads at runtime from the reference library
   (tools/perf/build_c_engine.sh builds it). */
#ifndef AST_BENCH_ENGINE_H
#define AST_BENCH_ENGINE_H

/* Load a fixture (path relative to root); returns NULL and fills err on
   failure.  A .head file is read through a FitsChan and anything else as
   Native text, exactly as ast_tester/transform_oracle_util.c's
   oracle_load_mapping does. */
void *abe_load( const char *root, const char *relpath, char *err, int errlen );
int abe_nin( void *map );
int abe_nout( void *map );

/* in holds ncoord_in columns of npoint values, out receives ncoord_out
   columns.  Returns 0 on success, nonzero (status cleared) on an AST error. */
int abe_tran( void *map, int forward, int npoint, int ncoord_in,
              const double *in, int ncoord_out, double *out );
void abe_free( void *map );

/* Counting build only; the bench build returns -1 from both. */
int abe_counts_reset( void );
/* Writes "name\tcount\n" lines, in name order, into buf (at most max bytes
   including the terminating nul); returns the bytes written. */
int abe_counts_dump( char *buf, int max );

#endif
