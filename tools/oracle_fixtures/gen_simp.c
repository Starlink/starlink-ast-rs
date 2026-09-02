/*
*  Name:
*     gen_simp

*  Purpose:
*     Read a native-format Object, simplify it, and write the result.

*  Description:
*     Reads one native-encoded Object from the named file, calls astSimplify on
*     it, and writes the result to the named output file.
*
*     With -m, a FrameSet is replaced by its base->current Mapping before being
*     simplified. Most of the simplify corpus's FrameSet fixtures were captured
*     that way -- their reference is a Mapping, not a FrameSet -- while a
*     fixture whose reference is itself a FrameSet was captured without it.
*
*     The output Channel's Comment and Indent attributes come from the two
*     optional trailing arguments, so a reference captured with either turned
*     off can be reproduced exactly. Their defaults are the Channel's own
*     (channel.c:5501, channel.c:5630).

*  Usage:
*     gen_simp [-m] <in> <out> [comment] [indent]
*/

#include "ast.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static FILE *src;
static char linebuf[4096];

static const char *srcfn( void ) {
   if( fgets( linebuf, sizeof linebuf, src ) ) {
      size_t n = strlen( linebuf );
      while( n && ( linebuf[ n - 1 ] == '\n' || linebuf[ n - 1 ] == '\r' ) ) {
         linebuf[ --n ] = 0;
      }
      return linebuf;
   }
   return NULL;
}

int main( int argc, char **argv ) {
   int comment = 1;
   int indent = 3;
   int getmapping = 0;
   int iarg = 1;

   if( argc > 1 && !strcmp( argv[ 1 ], "-m" ) ) {
      getmapping = 1;
      iarg = 2;
   }
   if( argc - iarg < 2 ) {
      fprintf( stderr, "usage: gen_simp [-m] <in> <out> [comment] [indent]\n" );
      return 2;
   }
   const char *infile = argv[ iarg ];
   const char *outfile = argv[ iarg + 1 ];
   if( argc > iarg + 2 ) comment = atoi( argv[ iarg + 2 ] );
   if( argc > iarg + 3 ) indent = atoi( argv[ iarg + 3 ] );

   astBegin;

   src = fopen( infile, "r" );
   if( !src ) {
      fprintf( stderr, "gen_simp: cannot open %s\n", infile );
      return 1;
   }
   AstChannel *in = astChannel( srcfn, NULL, "" );
   AstObject *obj = astRead( in );
   fclose( src );
   if( !obj ) {
      fprintf( stderr, "gen_simp: read failed for %s\n", infile );
      return 1;
   }

   AstMapping *map;
   if( getmapping && astIsAFrameSet( obj ) ) {
      map = astGetMapping( (AstFrameSet *) obj, AST__BASE, AST__CURRENT );
   } else {
      map = (AstMapping *) astClone( obj );
   }

   AstMapping *simp = astSimplify( map );

   AstChannel *out = astChannel( NULL, NULL, "SinkFile=%s,Comment=%d,Indent=%d",
                                 outfile, comment, indent );
   if( astWrite( out, simp ) != 1 ) {
      fprintf( stderr, "gen_simp: write failed for %s\n", outfile );
      return 1;
   }

   astEnd;
   return astOK ? 0 : 1;
}
