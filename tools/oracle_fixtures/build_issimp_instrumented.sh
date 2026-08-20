#!/bin/sh
# Build a copy of the reference C library instrumented to log every
# astSetIsSimple, and a driver that simplifies a native .map file.
#
# The `IsSimp` card records which sub-Mappings were, at some instant, the whole
# result of a simplification. When the port and C disagree about one, the only
# way to find out *where* C stamps it is to watch C do it: reasoning about the
# call graph has been wrong more often than right here.
#
#   ./tools/oracle_fixtures/build_issimp_instrumented.sh /tmp/ast-instr
#   AST_LOG_ISSIMPLE=1 /tmp/ast-instr/gen_simp in.map out.simp 2> stamps.txt
#
# Each stamp prints the object address, class, shape, and a resolved backtrace,
# so `MapMerge + 216` inside `Simplify` identifies cmpmap.c's self-simplify head
# without guessing. Objects carrying the card at dump time print as DUMPTAG,
# which is how a stamp is matched to the node it ends up on.
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
import pathlib, sys
dest = pathlib.Path(sys.argv[1])

h = dest/"src/mapping.h"
lines = h.read_text().splitlines(keepends=True)
i = next(k for k, l in enumerate(lines) if l.startswith("#define astSetIsSimple(this)"))
assert "((void)(this&&" in lines[i + 1], "astSetIsSimple macro has changed shape"
lines[i] = ("void astLogSetIsSimple_( void *, const char *, int );\n"
            "#define astSetIsSimple(this) "
            "astLogSetIsSimple_((void *)(this),__FILE__,__LINE__)\n")
lines[i + 1] = ""
h.write_text("".join(lines))

c = dest/"src/mapping.c"
t = c.read_text()
anchor = "AstMapping *astSimplify_( AstMapping *this, int *status ) {"
helper = r'''/* ---- INSTRUMENTATION (not upstream): log every astSetIsSimple -------- */
#include <execinfo.h>
void astLogSetIsSimple_( void *obj, const char *file, int line ){
   int *status = astGetStatusPtr;
   int i, n;
   void *bt[ 40 ];
   char **sym;
   if( !obj ) return;
   ((AstMapping*)obj)->flags |= astIsSimpleFlag(obj);
   if( getenv( "AST_LOG_ISSIMPLE" ) ){
      n = backtrace( bt, 40 );
      sym = backtrace_symbols( bt, n );
      fprintf( stderr, "STAMP obj=%p class=%s nin=%d nout=%d inv=%d at %s:%d\n",
               obj, astGetClass( obj ), astGetNin( obj ), astGetNout( obj ),
               astGetInvert( obj ), file, line );
      for( i = 1; i < n && i < 14; i++ ) fprintf( stderr, "    %s\n", sym[ i ] );
      free( sym );
   }
}
/* --------------------------------------------------------------------- */

'''
assert anchor in t
c.write_text(t.replace(anchor, helper + anchor, 1))

lines = c.read_text().splitlines(keepends=True)
i = next(k for k, l in enumerate(lines) if 'astWriteInt( channel, "IsSimp"' in l)
lines[i - 1] = ('   if( getenv( "AST_LOG_ISSIMPLE" ) && astGetIsSimple( this ) ) '
                'fprintf( stderr, "DUMPTAG obj=%p class=%s nin=%d\\n", '
                '(void*)this, astGetClass(this), astGetNin(this) );\n' + lines[i - 1])
c.write_text("".join(lines))
print("instrumented", dest)
PY

cmake -S "$DEST" -B "$DEST/build" > "$DEST/cmake.log" 2>&1
cmake --build "$DEST/build" --target ast -j8 > "$DEST/build.log" 2>&1

cat > "$DEST/gen_simp.c" <<'EOF'
/* Read a native .map, astSimplify it, and write the result with the Channel
   settings ast_tester/gen_simplify_fixtures.c uses. */
#include "ast.h"
#include <stdio.h>
#include <string.h>
static FILE *src;
static char linebuf[4096];
static const char *srcfn(void) {
   if (fgets(linebuf, sizeof linebuf, src)) {
      size_t n = strlen(linebuf);
      while (n && (linebuf[n-1] == '\n' || linebuf[n-1] == '\r')) linebuf[--n] = 0;
      return linebuf;
   }
   return NULL;
}
int main(int argc, char **argv) {
   astBegin;
   if (argc < 3) { fprintf(stderr, "usage: gen_simp in.map out.simp\n"); return 2; }
   src = fopen(argv[1], "r");
   if (!src) { fprintf(stderr, "cannot open %s\n", argv[1]); return 1; }
   AstChannel *in = astChannel(srcfn, NULL, "");
   AstMapping *map = (AstMapping *) astRead(in);
   fclose(src);
   if (!map) { fprintf(stderr, "read failed\n"); return 1; }
   AstMapping *simp = astSimplify(map);
   AstChannel *out = astChannel(NULL, NULL, "SinkFile=%s", argv[2]);
   if (astWrite(out, simp) != 1) { fprintf(stderr, "write failed\n"); return 1; }
   astEnd;
   return 0;
}
EOF

YAML=$(ls -d /opt/homebrew/opt/libyaml/lib /usr/local/opt/libyaml/lib 2>/dev/null | head -1)
cc "$DEST/gen_simp.c" -o "$DEST/gen_simp" -I "$DEST/build" -L "$DEST/build" \
   ${YAML:+-L "$YAML"} -Wl,-rpath,"$DEST/build" ${YAML:+-Wl,-rpath,"$YAML"} \
   -last -lyaml -lm
echo "built $DEST/gen_simp against the instrumented libast"
