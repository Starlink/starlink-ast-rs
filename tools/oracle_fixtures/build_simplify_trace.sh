#!/bin/sh
# Build a copy of the reference C library that traces the simplify machinery,
# plus a gen_simp driver, so the port's route through a Mapping can be diffed
# against C's.
#
#   ./tools/oracle_fixtures/build_simplify_trace.sh /tmp/ast-trace
#   AST_TRACE=1 /tmp/ast-trace/gen_simp in.map out.simp 2> trace.txt
#
# Three dispatchers in mapping.c cover every class, so no per-class patching is
# needed: astSimplify_ (whether the IsSimple bit short-circuited it),
# astMapMerge_ (which class was nominated, at which index, in which mode) and
# astMapList_ (what was decomposed).
#
# The submodule is never modified: everything happens in the copy.
set -e
DEST=${1:?usage: $0 <build-dir>}
HERE=$(cd "$(dirname "$0")/../.." && pwd)
SRC=$HERE/reference/starlink-ast
[ -d "$SRC/src" ] || { echo "reference submodule not initialised"; exit 1; }

rm -rf "$DEST"; mkdir -p "$DEST"
( cd "$SRC" && git ls-files ) > "$DEST/.files"
( cd "$SRC" && tar cf - -T "$DEST/.files" ) | ( cd "$DEST" && tar xf - )

python3 - "$DEST" <<'PY'
import pathlib, sys, re
dest = pathlib.Path(sys.argv[1])
c = dest/"src/mapping.c"
t = c.read_text()

helper = r'''/* ---- INSTRUMENTATION (not upstream): trace the simplify machinery ------ */
static int ast__trace_depth = 0;
static int ast__trace_on( void ){ return getenv( "AST_TRACE" ) != NULL; }
static void ast__trace( const char *fmt, ... ){
   va_list args;
   int i;
   if( !ast__trace_on() ) return;
   for( i = 0; i < ast__trace_depth; i++ ) fputs( "  ", stderr );
   va_start( args, fmt );
   vfprintf( stderr, fmt, args );
   va_end( args );
   fputc( '\n', stderr );
}
/* ----------------------------------------------------------------------- */

'''
anchor = "int astMapList_( AstMapping *this, int series, int invert, int *nmap,"
assert anchor in t
t = t.replace(anchor, helper + anchor, 1)

# astMapList_: log the decomposition request.
i = t.index(anchor)
j = t.index("{", i)
body = t.index("\n", j) + 1
t = (t[:body]
     + '   ast__trace( "MAPLIST %s series=%d invert=%d nmap_in=%d",\n'
       '               astGetClass( this ), series, invert, *nmap );\n'
     + t[body:])

# astMapMerge_: log the nomination, with the class actually nominated.
a2 = "int astMapMerge_( AstMapping *this, int where, int series, int *nmap,"
i = t.index(a2)
j = t.index("int result = -1;", i)
end = t.index("\n", j) + 1
t = (t[:end]
     + '   const char *ast__cls = astGetClass( this );\n'
       '   ast__trace( "MAPMERGE %s where=%d series=%d nmap=%d issimp=%d inv=%d",\n'
       '               ast__cls, where, series, *nmap,\n'
       '               astGetIsSimple( this ), astGetInvert( this ) );\n'
     + t[end:])

# astMapMerge_: also log what the nomination returned, so a productive
# nomination can be told from one that declined.
i = t.index(a2)
j = t.index("   return result;", i)
t = (t[:j]
     + '   ast__trace( "MAPMERGE-RESULT %s result=%d", ast__cls, result );\n'
     + t[j:])

# astSimplify_: log entry, and say whether the IsSimple bit short-circuited it.
a3 = "AstMapping *astSimplify_( AstMapping *this, int *status ) {"
i = t.index(a3)
guard = "   if( !astGetIsSimple( this ) && !astDoNotSimplify( this ) ) {"
j = t.index(guard, i)
t = (t[:j]
     + '   ast__trace( "SIMPLIFY-ENTER %s issimp=%d dns=%d inv=%d nin=%d nout=%d",\n'
       '               astGetClass( this ), astGetIsSimple( this ),\n'
       '               astDoNotSimplify( this ), astGetInvert( this ),\n'
       '               astGetNin( this ), astGetNout( this ) );\n'
       '   if( astGetIsSimple( this ) || astDoNotSimplify( this ) ) {\n'
       '      ast__trace( "SIMPLIFY-WALLED %s", astGetClass( this ) );\n'
       '   }\n'
       '   ast__trace_depth++;\n'
     + t[j:])
# ...and decrement on the way out.
ret = t.index("   return result;", t.index(a3))
t = t[:ret] + '   ast__trace_depth--;\n' + t[ret:]

if "#include <stdarg.h>" not in t:
    t = t.replace('#include "mapping.h"', '#include <stdarg.h>\n#include "mapping.h"', 1)
c.write_text(t)
print("instrumented", dest)
PY

cmake -S "$DEST" -B "$DEST/build" > "$DEST/cmake.log" 2>&1
# libast references the Grf and error-handler entry points without defining
# them; the satellite archives supply the no-op implementations, exactly as
# ast_tester/CMakeLists.txt's TEST_LIBS does for every C test program.
SATELLITES="ast ast_err ast_grf_2.0 ast_grf_3.2 ast_grf_5.6 ast_grf3d"
for t in $SATELLITES; do
   cmake --build "$DEST/build" --target "$t" -j8 >> "$DEST/build.log" 2>&1 \
      || { tail -25 "$DEST/build.log"; exit 1; }
done

cp "$(dirname "$0")/gen_simp.c" "$DEST/gen_simp.c"

YAML=$(ls -d /opt/homebrew/opt/libyaml/lib /usr/local/opt/libyaml/lib 2>/dev/null | head -1)
GRF="-last_err -last_grf_2.0 -last_grf_3.2 -last_grf_5.6 -last_grf3d"
# gen_simp registers the corpus's IntraMap functions through the upstream
# helper, as the Makefile's gen_simp rule links it.
cc "$DEST/gen_simp.c" "$DEST/ast_tester/transform_oracle_util.c" -o "$DEST/gen_simp" \
   -I "$DEST/build" -I "$DEST/ast_tester" -L "$DEST/build" \
   ${YAML:+-L "$YAML"} -Wl,-rpath,"$DEST/build" ${YAML:+-Wl,-rpath,"$YAML"} \
   -last $GRF -lyaml -lm
echo "built $DEST/gen_simp against the tracing libast"
