/* ast2ast — read a native-text (.ast) object via a source Channel and
 * re-dump it via a sink Channel (Comment=1, Full=0). This exercises the
 * C oracle's own native round-trip, isolating from FITS. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ast.h"

static char *in_buf; static size_t in_len, in_pos;
static char *out_buf; static size_t out_len, out_cap;

static const char *source(void) {
    if (in_pos >= in_len) return NULL;
    size_t start = in_pos;
    while (in_pos < in_len && in_buf[in_pos] != '\n') in_pos++;
    size_t n = in_pos - start;
    static char line[4096];
    memcpy(line, in_buf + start, n); line[n] = 0;
    if (in_pos < in_len) in_pos++; /* skip newline */
    return line;
}
static void sink(const char *line) {
    size_t n = strlen(line);
    while (out_len + n + 2 > out_cap) { out_cap = out_cap ? out_cap*2 : 8192; out_buf = realloc(out_buf, out_cap); }
    memcpy(out_buf + out_len, line, n); out_len += n; out_buf[out_len++] = '\n'; out_buf[out_len]=0;
}

int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: ast2ast <in.ast>\n"); return 2; }
    FILE *f = fopen(argv[1], "r"); if (!f) { perror("fopen"); return 1; }
    fseek(f, 0, SEEK_END); long sz = ftell(f); fseek(f, 0, SEEK_SET);
    in_buf = malloc(sz + 1); fread(in_buf, 1, sz, f); in_buf[sz] = 0; in_len = sz; fclose(f);

    astBegin;
    AstChannel *chin = astChannel(source, NULL, "");
    AstObject *obj = astRead(chin);
    if (!obj) { fprintf(stderr, "astRead failed\n"); return 1; }
    AstChannel *chout = astChannel(NULL, sink, "Comment=1,Full=0");
    astWrite(chout, obj);
    astEnd;
    fputs(out_buf, stdout);
    return 0;
}
