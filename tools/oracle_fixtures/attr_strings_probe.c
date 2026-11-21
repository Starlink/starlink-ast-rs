/* Copyright 2026 Association of Universities for Research in Astronomy (AURA) */
/* SPDX-License-Identifier: LGPL-3.0-or-later */

/* attr_strings_probe -- apply attribute strings to objects with astSet,
 * astSetC, astGetC, astTest and astClear, and capture what the C library
 * does with each: the value or flag returned, or the error status and every
 * message reported, and the object's Native dump afterwards.
 *
 * Each case names an object the probe builds with the C constructors (or
 * reads from a FITS header), written out as Native text so that the Rust
 * test starts from the same object, and a list of operations applied to it
 * in order. A setting is applied as astSet( obj, "%s", text ), so "%" in it
 * is not a format specifier.
 *
 * The probe defines astPutErr itself to collect the messages, so it does
 * not link libast_err. The first message C reports for an error raised
 * under a public function names this file and a line ("AST: Error at line
 * ..."); it says nothing about the library and is dropped.
 *
 * Prints Rust source for crates/starlink-ast/tests/attr_strings_oracle/cases.rs.
 * Build and run from reference/starlink-ast/build (configured with
 * -DCMAKE_C_FLAGS=-ffp-contract=off), compiling grf_log.c in for the Plot
 * cases:
 *
 *   cc -ffp-contract=off -O3 -DHAVE_CONFIG_H -I. -Isrc -I../src -I.. \
 *      ../../../tools/oracle_fixtures/attr_strings_probe.c ../src/grf_log.c \
 *      -L. -last -lm -Wl,-rpath,$PWD \
 *      -o ../../../tools/oracle_fixtures/attr_strings_probe
 *   ../../../tools/oracle_fixtures/attr_strings_probe \
 *      > ../../../crates/starlink-ast/tests/attr_strings_oracle/cases.rs
 */
#include "plot_draw_probe.h"
#include "ast_err.h"

/* The messages reported since the last operation. */
#define MAXMSG 16
static char msgs[ MAXMSG ][ 1024 ];
static int nmsg = 0;

void astPutErr_( int status_value, const char *message ) {
   (void) status_value;
   if( !strncmp( message, "AST: Error", 10 ) ) return;
   if( nmsg < MAXMSG ) {
      snprintf( msgs[ nmsg ], sizeof( msgs[ nmsg ] ), "%s", message );
      nmsg++;
   }
}

enum { OP_END, OP_SET, OP_SETC, OP_GET, OP_TEST, OP_CLEAR };

struct op { int kind; const char *a; const char *b; };

#define SET( s ) { OP_SET, s, NULL }
#define SETC( n, v ) { OP_SETC, n, v }
#define GET( n ) { OP_GET, n, NULL }
#define TEST( n ) { OP_TEST, n, NULL }
#define CLEAR( n ) { OP_CLEAR, n, NULL }
/* The value and set state of an attribute. */
#define SEE( n ) GET( n ), TEST( n )
/* Set an attribute, look at it, clear it and look again. */
#define CYCLE( n, setting ) SET( setting ), SEE( n ), CLEAR( n ), SEE( n )

#define MAXOP 96
struct acase {
   const char *name;
   const char *object;
   struct op ops[ MAXOP ];
};

static const struct acase cases[] = {

/* Object attributes, as a plain Frame inherits them. */
   { "frame2_object", "frame2", {
      SEE( "ID" ), CYCLE( "ID", "ID=my id" ),
      SEE( "Ident" ), CYCLE( "Ident", "Ident=An ident" ),
      SEE( "UseDefs" ), CYCLE( "UseDefs", "UseDefs=0" ),
      SET( "UseDefs=5" ), SEE( "UseDefs" ),
      SET( "usedefs= -1 " ), GET( "UseDefs" ),
      SET( "UseDefs=x" ), SET( "UseDefs=1.5" ), SET( "UseDefs=1\t" ),
      SET( "UseDefs=" ), GET( "UseDefs" ),
      SEE( "Class" ), SET( "Class=Fred" ), CLEAR( "Class" ),
      SET( "Ident=x%^50+y%+z %% w%s70+" ), GET( "Ident" ),
      SET( "Ident=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa%+bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" ),
      GET( "Ident" ),
      SET( "Ident=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" ),
      GET( "Ident" ),
      SETC( "Ident", "a, b" ), GET( "Ident" ),
      SETC( " Ident ", "\"q\"" ), GET( "Ident" ),
      SETC( "Ident", "a,b=c" ), GET( "Ident" ),
      SETC( "Ident", "one\ntwo" ), GET( "Ident" ),
      TEST( " I dent " ), GET( " i D e n t " ),
      SET( "ID=a" ), SET( "Ident=b" ), CLEAR( " ID , Ident,UseDefs" ),
      SEE( "ID" ), SEE( "Ident" ), SEE( "UseDefs" ),
   } },

/* Mapping attributes, as a plain Frame inherits them. */
   { "frame2_mapping", "frame2", {
      SEE( "Nin" ), SEE( "Nout" ), SEE( "IsLinear" ), SEE( "IsSimple" ),
      SEE( "TranForward" ), SEE( "TranInverse" ), SEE( "Invert" ),
      SEE( "Report" ),
      SET( "Nin=3" ), SET( "Nout=" ), SET( "islinear=0" ), SET( "IsSimple=1" ),
      SET( "TranForward=1" ), SET( "traninverse=0" ),
      CLEAR( "Nin" ), CLEAR( "nout" ), CLEAR( "IsLinear" ), CLEAR( "IsSimple" ),
      CLEAR( "TranForward" ), CLEAR( "TranInverse" ),
   } },

/* Names no class has, and blank ones. */
   { "frame2_unknown", "frame2", {
      SET( "Fred=1" ), GET( "Fred" ), TEST( "Fred" ), CLEAR( "Fred" ),
      GET( " " ), TEST( " " ), CLEAR( " " ), CLEAR( " , ," ), CLEAR( "" ),
      SET( " " ), SET( "" ), SET( "Fred" ), SET( "=1" ), SET( "ID" ),
      GET( "ID=3" ), GET( "" ), TEST( "" ),
      SET( "ID=a" ), SET( "Ident=b" ), CLEAR( "ID,Fred,Ident" ),
      SEE( "ID" ), SEE( "Ident" ),
   } },

/* How astSet splits a string into assignments. */
   { "frame2_splitting", "frame2", {
      SET( "Ident=a,b" ), GET( "Ident" ),
      SET( "Ident=\"a,b\",ID=1" ), GET( "Ident" ), GET( "ID" ),
      CLEAR( "ID" ), SET( "Ident=a,b,ID=1" ), GET( "Ident" ), GET( "ID" ),
      SET( "Ident=" ), SEE( "Ident" ),
      SET( " I dent = Two words " ), GET( "Ident" ),
      SET( "Ident= \"quoted\" " ), GET( "Ident" ),
      SET( "Ident=\"quoted\"  " ), GET( "Ident" ),
      SET( "Ident=\"unclosed" ), GET( "Ident" ),
      SET( "Ident=in\"side\"" ), GET( "Ident" ),
      SET( "ID=x,UseDefs=0,Ident=y" ), GET( "ID" ), GET( "UseDefs" ),
      GET( "Ident" ),
      SET( "ID = p , ,  Ident = q " ), GET( "ID" ), GET( "Ident" ),
      SET( "ID=a=b" ), GET( "ID" ),
      SET( "ID=a=b,c" ), GET( "ID" ),
      SET( "ID=one\nIdent=two" ), GET( "ID" ), GET( "Ident" ),
      SET( "ID=r\rs" ), GET( "ID" ),
   } },

/* Attributes of an Object that the Rust library has no value for, and the
   Mapping flags of a plain Frame, which it does not store. */
   { "frame2_no_rust_value", "frame2", {
      GET( "Nobject" ), GET( "ObjSize" ), GET( "RefCount" ),
      TEST( "Nobject" ), TEST( "ObjSize" ), TEST( "RefCount" ),
      SET( "Nobject=1" ), SET( "ObjSize=1" ), SET( "RefCount=1" ),
      CLEAR( "Nobject" ), CLEAR( "ObjSize" ), CLEAR( "RefCount" ),
      SET( "Invert=1" ), CLEAR( "Invert" ), SET( "Report=1" ),
      CLEAR( "Report" ),
   } },

/* The same names on a Frame of one axis. */
   { "frame1_object", "frame1", {
      GET( "Class" ), GET( "Nin" ), GET( "Nout" ), SEE( "Invert" ),
      CYCLE( "ID", "ID=x" ), CYCLE( "Ident", "Ident=i" ),
      CYCLE( "UseDefs", "UseDefs=0" ), SET( "Nin=1" ), CLEAR( "Class" ),
   } },
};

/* The object a case starts from. */
static AstObject *make( const char *id ) {
   if( !strcmp( id, "frame2" ) ) return (AstObject *) astFrame( 2, " " );
   if( !strcmp( id, "frame1" ) ) return (AstObject *) astFrame( 1, " " );
   fprintf( stderr, "unknown object %s\n", id );
   exit( 1 );
}

/* The Rust name of an error status. */
static const char *code_name( int status ) {
   switch( status ) {
      case AST__BADAT: return "Badat";
      case AST__NOWRT: return "Nowrt";
      case AST__ATTIN: return "Attin";
      case AST__AXIIN: return "Axiin";
      case AST__OPT: return "Opt";
      default:
         fprintf( stderr, "status %d has no name in code_name\n", status );
         exit( 1 );
   }
}

/* A Rust string literal, with control characters escaped. */
static void rs( const char *s ) {
   putchar( '"' );
   for( ; *s; s++ ) {
      unsigned char c = (unsigned char) *s;
      if( c == '"' || c == '\\' ) {
         putchar( '\\' );
         putchar( c );
      } else if( c == '\n' ) {
         fputs( "\\n", stdout );
      } else if( c == '\r' ) {
         fputs( "\\r", stdout );
      } else if( c == '\t' ) {
         fputs( "\\t", stdout );
      } else if( c < 0x20 || c == 0x7f ) {
         printf( "\\x%02x", c );
      } else {
         putchar( c );
      }
   }
   putchar( '"' );
}

static char dumpbuf[ 1 << 20 ];
static void sink( const char *line ) {
   strncat( dumpbuf, line, sizeof( dumpbuf ) - strlen( dumpbuf ) - 2 );
   strcat( dumpbuf, "\n" );
}

/* Print the Native dump of an object as a Rust raw string. */
static void dump( AstObject *obj ) {
   AstChannel *ch = astChannel( NULL, sink, " " );
   dumpbuf[ 0 ] = '\0';
   astWrite( ch, obj );
   ch = astAnnul( ch );
   printf( "r###\"%s\"###", dumpbuf );
}

/* Print the outcome of the operation just applied: the error if there was
   one, and otherwise what "ok" prints. */
static int failed( void ) {
   int i;
   if( astOK ) {
      if( nmsg ) {
         fprintf( stderr, "messages without an error status\n" );
         exit( 1 );
      }
      return 0;
   }
   printf( "Out::Err(E::%s, &[", code_name( astStatus ) );
   for( i = 0; i < nmsg; i++ ) {
      if( i ) printf( ", " );
      rs( msgs[ i ] );
   }
   printf( "])" );
   astClearStatus;
   nmsg = 0;
   return 1;
}

int main( void ) {
   int status = 0;
   size_t c;
   astWatch( &status );

   printf( "// Generated by tools/oracle_fixtures/attr_strings_probe.c against\n"
           "// the C library at the submodule pin. Do not edit.\n\n"
           "use starlink_ast::error::ErrorCode as E;\n\n"
           "/// One operation on a case's object.\n"
           "#[derive(Debug, Clone, Copy, PartialEq)]\n"
           "pub enum Op {\n"
           "    /// `astSet( obj, \"%%s\", settings )`.\n"
           "    Set(&'static str),\n"
           "    /// `astSetC( obj, name, value )`.\n"
           "    SetC(&'static str, &'static str),\n"
           "    /// `astGetC( obj, name )`.\n"
           "    Get(&'static str),\n"
           "    /// `astTest( obj, name )`.\n"
           "    Test(&'static str),\n"
           "    /// `astClear( obj, names )`.\n"
           "    Clear(&'static str),\n"
           "}\n\n"
           "/// What the C library did with an operation: an error is its\n"
           "/// status and every message reported, in order.\n"
           "#[derive(Debug, Clone, PartialEq)]\n"
           "pub enum Out {\n"
           "    Done,\n"
           "    Value(&'static str),\n"
           "    Flag(bool),\n"
           "    Err(E, &'static [&'static str]),\n"
           "}\n\n"
           "/// A case: the object as Native text (for a FitsChan, its header\n"
           "/// cards), the operations with C's outcomes, and the Native dump\n"
           "/// afterwards (empty for a FitsChan).\n"
           "pub struct Case {\n"
           "    pub name: &'static str,\n"
           "    pub fitschan: bool,\n"
           "    pub object: &'static str,\n"
           "    pub ops: &'static [(Op, Out)],\n"
           "    pub dump: &'static str,\n"
           "}\n\n"
           "pub const CASES: &[Case] = &[\n" );

   for( c = 0; c < sizeof( cases ) / sizeof( cases[ 0 ] ); c++ ) {
      const struct acase *ac = &cases[ c ];
      const struct op *op;
      AstObject *obj = make( ac->object );
      if( !astOK ) {
         fprintf( stderr, "case %s: cannot make the object\n", ac->name );
         return 1;
      }
      printf( "    Case {\n        name: " );
      rs( ac->name );
      printf( ",\n        fitschan: false,\n        object: " );
      dump( obj );
      printf( ",\n        ops: &[\n" );
      for( op = ac->ops; op->kind != OP_END; op++ ) {
         const char *value;
         int flag;
         printf( "            (" );
         switch( op->kind ) {
            case OP_SET:
               printf( "Op::Set(" ); rs( op->a ); printf( "), " );
               astSet( obj, "%s", op->a );
               if( !failed() ) printf( "Out::Done" );
               break;
            case OP_SETC:
               printf( "Op::SetC(" ); rs( op->a ); printf( ", " );
               rs( op->b ); printf( "), " );
               astSetC( obj, op->a, op->b );
               if( !failed() ) printf( "Out::Done" );
               break;
            case OP_GET:
               printf( "Op::Get(" ); rs( op->a ); printf( "), " );
               value = astGetC( obj, op->a );
               if( !failed() ) {
                  printf( "Out::Value(" ); rs( value ); printf( ")" );
               }
               break;
            case OP_TEST:
               printf( "Op::Test(" ); rs( op->a ); printf( "), " );
               flag = astTest( obj, op->a );
               if( !failed() ) printf( "Out::Flag(%s)", flag ? "true" : "false" );
               break;
            default:
               printf( "Op::Clear(" ); rs( op->a ); printf( "), " );
               astClear( obj, op->a );
               if( !failed() ) printf( "Out::Done" );
               break;
         }
         printf( "),\n" );
      }
      printf( "        ],\n        dump: " );
      dump( obj );
      printf( ",\n    },\n" );
      obj = astAnnul( obj );
   }
   printf( "];\n" );
   return astOK ? 0 : 1;
}
