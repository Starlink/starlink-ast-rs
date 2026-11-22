# The browser FITS plotter

A single HTML page that does what the AST library's [FITS plotter](https://starlink.eao.hawaii.edu/cgi-bin/ast/fplot.pl) page does, without a server: paste a FITS header, and it shows the header's coordinate grid, the header rewritten as FITS-WCS, and the FrameSet as AST Native text.
The work is `crates/ast-plotter-wasm`, which follows `fplottest.f`, the Fortran program behind that page, on the Rust port of AST compiled to WebAssembly.

## Building

```shell
tools/build_plotter.sh --smoke   # every example and a date axis drawn in Node through the page's
                                 # worker code, and a new instance after a trap
tools/build_plotter.sh           # writes target/plotter/fits-plotter.html
```

The build needs rustup with the `wasm32-unknown-unknown` target (`rustup target add wasm32-unknown-unknown`), `wasm-bindgen-cli` of the version `crates/ast-plotter-wasm/Cargo.toml` pins (the script names the install command if it is missing), `python3`, and `node` for `--smoke`.
It uses rustup's `cargo` even when another Rust is first on `PATH`.

The page is one file holding the HTML, the CSS, the JavaScript, the examples and the WebAssembly, so it opens from the file system by double-clicking it.

## Files

- `index.html`: the page, with markers the build replaces with the other files.
- `plotter.css`: its look, in light and dark mode.
- `worker.js`: the drawing Worker's code, which the smoke test also runs.
- `plotter.js`: filling the inputs from an example, redrawing 400 ms after the last edit, and showing each output or the error that replaced it.
  It draws in a Worker (`worker.js`), so the page stays responsive: an edit stops a draw still running, and a draw that takes longer than 20 s is stopped and reported (a very small `Tol` or a very large `MinTick` can make a grid take minutes, in the C library too).
- `examples.txt`: the eight examples of the CGI page, read by the build, by `crates/ast-plotter-wasm`'s tests and by `tools/oracle_fixtures/fits_plotter_probe.c`, which runs `fplottest.f`'s steps on the C library to make those tests' fixtures.

## How it is checked

`crates/ast-plotter-wasm`'s tests compare its three outputs with the C library's, byte for byte, for the eight examples and six other cases, except seven outputs that show port defects, named in `PLOTTER_EXCLUDED` with their reasons; the page draws with two `SvgGrf` settings the comparison leaves off (text drawn over a cleared box, and an equal-aspect square with text sized to it).
The page itself is checked by eye.
