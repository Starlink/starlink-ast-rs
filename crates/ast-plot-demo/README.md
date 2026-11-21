# ast-plot-demo

A window that draws the coordinate grid of a Starlink AST Plot through [egui](https://github.com/emilk/egui).

Pick one of the grid test cases from the "Case" menu.
Each is a FITS header, a plot box and some Plot attributes, the same cases `starlink-ast`'s grid tests draw.
Edit the attributes in the field beside it, in the C library's `astSet` syntax (`Grid=1,Labelling=interior,Colour(axes)=3`), and press Enter to redraw; "Reset" restores the case's own attributes.
The grid is redrawn to fill the window when it is resized, and follows the light or dark theme.

```shell
cargo run -p ast-plot-demo --release
```

The drawing goes through `EguiGrf`, a `starlink_ast::grf::Grf` that turns a Plot's lines, markers and text into egui shapes and answers its text-size questions from egui's own font layout.

The crate is not a default member of the workspace, so a plain `cargo build` does not build egui.
Building it needs no system packages; on Linux it needs an X11 or Wayland desktop to run.
It is not published.
