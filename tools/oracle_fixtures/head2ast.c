/* head2ast — generate a native-text (.ast) FrameSet fixture from a FITS
 * header, using the reference AST library as the oracle.
 *
 * Reads a FITS `.head` file into a FitsChan (applying any attribute
 * string given as argv[2], e.g. "cdmatrix=1,sipreplace=0,FitsDigits=8"),
 * `astRead`s the WCS object, and dumps it via an `astChannel` text sink
 * with the default verbosity (Comment=1, Full=0) — exactly the form the
 * Rust `to_native_string` produces. The dump is written to stdout.
 *
 * This mirrors the AST branch of `ast_tester/wcsconverter.c`: it applies
 * the attribute string before the read, so SipReplace and friends affect
 * the parsed object identically.
 *
 * Usage:  head2ast <input.head> [attrs] > output.ast
 *
 * The generated `.ast` files are committed under
 * crates/ast-core/tests/fixtures/native/reference/frameset_fixtures/ and
 * drive the `native_byte_exact_round_trip_frameset_fixtures` test.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"

/* astChannel sink: accumulate each emitted line (newline-terminated) into
 * a growable buffer that we print once at the end. */
static char *sink_buf;
static size_t sink_len, sink_cap;

static void sink(const char *line) {
    size_t n = strlen(line);
    while (sink_len + n + 2 > sink_cap) {
        sink_cap = sink_cap ? sink_cap * 2 : 8192;
        sink_buf = realloc(sink_buf, sink_cap);
    }
    memcpy(sink_buf + sink_len, line, n);
    sink_len += n;
    sink_buf[sink_len++] = '\n';
    sink_buf[sink_len] = 0;
}

int main(int argc, char **argv) {
    int status = 0;
    astWatch(&status);

    if (argc < 2) {
        fprintf(stderr, "usage: head2ast <input.head> [attrs]\n");
        return 2;
    }
    const char *in_file = argv[1];
    const char *attrs = (argc >= 3) ? argv[2] : " ";

    FILE *fp = fopen(in_file, "r");
    if (!fp) {
        fprintf(stderr, "head2ast: cannot open %s\n", in_file);
        return 1;
    }

    AstFitsChan *fc = astFitsChan(NULL, NULL, "%s", attrs);
    char line[300], buf[81];
    while (fgets(line, sizeof line, fp)) {
        size_t n = strlen(line);
        while (n > 0 && (line[n - 1] == '\n' || line[n - 1] == '\r')) {
            line[--n] = 0;
        }
        memset(buf, ' ', 80);
        buf[80] = 0;
        memcpy(buf, line, n < 80 ? n : 80);
        astPutFits(fc, buf, 0);
    }
    fclose(fp);

    astClear(fc, "Card");
    AstObject *obj = astRead(fc);
    if (!obj) {
        fprintf(stderr, "head2ast: no WCS could be read from %s\n", in_file);
        return 1;
    }

    /* Dump as native text with default verbosity (matches to_native_string). */
    AstChannel *ch = astChannel(NULL, sink, "Comment=1");
    astWrite(ch, obj);
    if (sink_buf) {
        fputs(sink_buf, stdout);
    }

    return astOK ? 0 : 1;
}
