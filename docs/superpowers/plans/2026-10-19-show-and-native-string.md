# `show`, `to_native_string` and typed reads: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every AST class `show()` and `to_native_string()` methods matching C's `astShow` and `astToString` byte for byte, `TryFrom<AnyObject>` for typed reads, and remove the free `to_native_string` function.

**Architecture:** One macro in `crates/ast-core/src/serial/` expands over the `AnyObject` variant list to generate the two methods and the `TryFrom` impl for each class, plus the same methods on `AnyObject`. Both methods render the class's `Serializable::to_node` through the existing `native_write::render_native_with_options` with the option values C's Channel settings give, so no object is copied into an `AnyObject`. A C probe records `astShow` and `astToString` output for one object of each class as Rust source, and an oracle test compares exactly.

**Tech Stack:** Rust 2024 (`ast-core`), C probe linked against the reference `libast` built per `docs/issues/c-oracle-probe-recipe.md`.

**Spec:** `docs/superpowers/specs/2026-10-19-show-and-native-string-design.md`

## Global Constraints

- `show` matches C's `astShow`: a Channel with no attributes set (`Comment` true, `Full` 0, `Indent` 3), writing to standard output.
- `to_native_string` matches C's `astToString`: `"Comment=0,Full=-1,Indent=0"`, each line followed by `\n`.
- Both take `&self` and never clone the object into an `AnyObject`.
- A wrong class in `TryFrom` gives `ErrorCode::Objin` and the message `Pointer to <Wanted> required, but pointer to <Got> given.` where both names are AST class names (`Box`, not `BoxRegion`; `Frame`, not `BasicFrame`; `Channel`, not `NativeChannel`).
- No `#[doc(alias)]` for C names; no `Display` impl; `Debug` unchanged.
- `from_native_string` and `from_native_string_with` stay free functions with unchanged signatures.
- fmt and clippy clean with and without the `c-oracle` feature; comments describe the code as it is.
- Never stage `reference/starlink-ast`.

## Review Focus

1. An object with `ID` or `Ident` set: the compact form must still write them (`Full=-1` drops defaults, not set values) — pinned by the `zoommap_with_id` probe case in Task 1.
2. An `IntraMap`: `to_native_string` works without a registry, but reading it back needs one; the round-trip test reads through `tests/common::from_native_string`, which registers the test transforms — Task 2.
3. A boxed variant (`KeyMap`, `Table`, `FitsTable`, `NativeChannel`, `MocChan`, `FitsChan`): `TryFrom` must return the unboxed value — Task 2 test `try_from_unboxes_boxed_variants`.
4. Nested objects (CmpMap, FrameSet, Regions): with `Indent=0` every nested line starts at column 0 — covered by the byte-exact oracle cases `cmpmap`, `frameset` and `cmpregion` in Task 1.
5. `show` writing to a closed standard output: C's default sink is `printf`, which ignores write errors, so `show` ignores them too rather than panicking as `print!` would — Task 2 implements it with `std::io::Write` on a locked stdout and discards the `io::Result`.

---

### Task 1: C oracle for `astShow` and `astToString`

**Files:**
- Create: `tools/oracle_fixtures/show_tostring_probe.c`
- Create: `crates/ast-core/tests/show_tostring_oracle/cases.rs` (generated)

**Interfaces:**
- Produces: `crates/ast-core/tests/show_tostring_oracle/cases.rs` defining
  `pub const CASES: &[(&str, &str, &str, &str)]` — `(case name, AST class name, astShow text, astToString text)`.

- [ ] **Step 1: Write the probe.**
  It builds one object per case, captures `astShow` by redirecting standard output to a temporary file, captures `astToString`, and prints Rust source.
  Cases come from two sources:
  1. Fixture files read with `astRead` from a `source` callback (as `object_id_probe.c` reads): the 41 paths below, relative to `crates/ast-core/tests/fixtures/`.
  2. Objects built in C for the 15 classes that have no top-level fixture.

  ```c
  /* show_tostring_probe -- capture astShow and astToString for one object
   * of each class the port's AnyObject holds.
   *
   * Prints Rust source for crates/ast-core/tests/show_tostring_oracle/cases.rs:
   *
   *   ./show_tostring_probe > ../../crates/ast-core/tests/show_tostring_oracle/cases.rs
   */
  #include <stdio.h>
  #include <stdlib.h>
  #include <string.h>
  #include <unistd.h>
  #include "ast.h"

  #define FIX "../../crates/ast-core/tests/fixtures/"

  static FILE *src_file;
  static char src_line[ 100000 ];
  static const char *source( void ) {
     return fgets( src_line, sizeof( src_line ), src_file ) ? strtok( src_line, "\n" ) ? src_line : "" : NULL;
  }

  static AstObject *read_fixture( const char *rel ) {
     char path[ 1000 ];
     AstChannel *ch;
     AstObject *obj;
     snprintf( path, sizeof( path ), FIX "%s", rel );
     src_file = fopen( path, "r" );
     if( !src_file ) { fprintf( stderr, "cannot open %s\n", path ); exit( 1 ); }
     ch = astChannel( source, NULL, " " );
     obj = astRead( ch );
     ch = astAnnul( ch );
     fclose( src_file );
     return obj;
  }

  /* astShow writes to stdout; send it to a file and read it back. */
  static char *capture_show( AstObject *obj ) {
     static char buf[ 4000000 ];
     size_t n;
     int saved = dup( 1 );
     FILE *tmp = tmpfile();
     fflush( stdout );
     dup2( fileno( tmp ), 1 );
     astShow( obj );
     fflush( stdout );
     dup2( saved, 1 );
     close( saved );
     rewind( tmp );
     n = fread( buf, 1, sizeof( buf ) - 1, tmp );
     buf[ n ] = 0;
     fclose( tmp );
     return buf;
  }

  static void emit( const char *name, AstObject *obj ) {
     char *show = capture_show( obj );
     char *str = astToString( obj );
     printf( "    (\n        \"%s\",\n        \"%s\",\n        r####\"%s\"####,\n        r####\"%s\"####,\n    ),\n",
             name, astGetC( obj, "Class" ), show, str );
     str = astFree( str );
  }
  ```

  `main` prints the header, then one `emit` per case, then `];`:

  ```c
  int main( void ) {
     int status = 0;
     static const char *fixtures[][ 2 ] = {
        { "unitmap", "simplify/lsst1.simp" },
        { "zoommap", "serialisation/zoommap.ast" },
        { "shiftmap", "simplify/win_to_shift.simp" },
        { "matrixmap", "simplify/win_to_matrix.simp" },
        { "permmap", "serialisation/permmap.ast" },
        { "winmap", "serialisation/winmap.ast" },
        { "lutmap", "simplify/neg_lut_constant.map" },
        { "polymap", "simplify/cap_winmap_04.map" },
        { "chebymap", "simplify/neg_chebymap_standalone.map" },
        { "cmpmap", "simplify/brad.map" },
        { "wcsmap", "simplify/wcsmap_bad_to_unit.map" },
        { "slamap", "simplify/sla_ha_cancel.map" },
        { "specmap", "simplify/spec_lsr_cancel.map" },
        { "timemap", "simplify/cap_timemap_01.simp" },
        { "pcdmap", "simplify/pcd_zero_to_unit.map" },
        { "grismmap", "simplify/grism_zoom_merge.simp" },
        { "unitnormmap", "simplify/cap_unitnormmap_01.simp" },
        { "tranmap", "simplify/neg_tranmap_oneway.map" },
        { "ratemap", "simplify/rate_inner_reduce_lone.map" },
        { "mathmap", "simplify/neg_math_lone.map" },
        { "dssmap", "serialisation/c_dssmap.ast" },
        { "normmap", "simplify/cap_permmap_01.simp" },
        { "selectormap", "simplify/selectormap_region_simplify.map" },
        { "switchmap", "simplify/switchmap_inv_fwdonly.map" },
        { "frame", "serialisation/basicframe.ast" },
        { "skyframe", "serialisation/skyframe.ast" },
        { "specframe", "serialisation/specframe.ast" },
        { "dsbspecframe", "serialisation/jan-dsb.ast" },
        { "timeframe", "serialisation/timeframe.ast" },
        { "cmpframe", "serialisation/cmpframe.ast" },
        { "frameset", "wcsconv/inputs/dss.ast" },
        { "circle", "simplify/defunc_nullregion_2d.map" },
        { "box", "simplify/cap_slamap_02.map" },
        { "ellipse", "simplify/cefit_circle_to_ellipse.map" },
        { "polygon", "simplify/cap_poly_unc_frame.map" },
        { "interval", "simplify/cap_interval_01.simp" },
        { "nullregion", "simplify/cap_slamap_07.simp" },
        { "cmpregion", "simplify/cap_slamap_07.map" },
        { "pointlist", "simplify/cap_zoommap_04.map" },
        { "prism", "simplify/cap_slamap_02.simp" },
        { "stcresourceprofile", "simplify/cap_timemap_02.map" },
     };
     size_t i;
     AstObject *obj;

     astWatch( &status );
     printf( "// Generated by tools/oracle_fixtures/show_tostring_probe.c; do not edit.\n" );
     printf( "// (case, AST class, astShow output, astToString output)\n" );
     printf( "pub const CASES: &[(&str, &str, &str, &str)] = &[\n" );
     for( i = 0; i < sizeof( fixtures ) / sizeof( fixtures[ 0 ] ); i++ ) {
        obj = read_fixture( fixtures[ i ][ 1 ] );
        emit( fixtures[ i ][ 0 ], obj );
        obj = astAnnul( obj );
     }

     /* Set attributes must survive Full=-1 (Review Focus 1). */
     obj = (AstObject *) astZoomMap( 2, 4.0, "ID=zoom_id,Ident=zoom ident" );
     emit( "zoommap_with_id", obj );
     obj = astAnnul( obj );

     build_constructed();   /* see Step 2 */
     printf( "];\n" );
     return status != 0;
  }
  ```

  Before running, verify each fixture's top-level object is the class its case name says: the probe must `exit( 1 )` if `astGetC( obj, "Class" )` does not match the expected class, so add the expected class as a third column of `fixtures` and check it in the loop.

- [ ] **Step 2: Build the classes with no top-level fixture in `build_constructed()`.**
  Each case is one `emit` of an object built as stated; copy the construction from the named existing probe where one is given, with that probe's fixture paths made relative to `FIX`.

  | Case | Class | Construction |
  |---|---|---|
  | `sphmap` | SphMap | `astSphMap( "UnitRadius=1" )` |
  | `xphmap` | XphMap | as `xphmap_probe.c` obtains its XphMap (XphMap has no public constructor) |
  | `intramap` | IntraMap | register `"double"` exactly as `intramap_probe.c:118` does, then `astIntraMap( "double", 1, 1, " " )` |
  | `splinemap` | SplineMap | as `spline_inverse_probe.c:57-77`, reading `programs/testsplinemap/2dspline_c.dat` |
  | `fluxframe` | FluxFrame | `astFluxFrame( 1.0E9, astSpecFrame( "System=FREQ,Unit=Hz" ), "Unit=Jy" )` |
  | `moc` | Moc | as `mocchan_probe.c:45` makes its Moc, adding the same cells |
  | `stcsearchlocation`, `stccatalogentrylocation`, `stcobsdatalocation` | Stc classes | each wraps `astCircle( astSkyFrame( " " ), 1, (double[]){ 0.1, 0.2 }, (double[]){ 0.01 }, NULL, " " )` with `ncoords` 0 and `coords` NULL |
  | `keymap` | KeyMap | `astKeyMap( " " )`, then `astMapPut0D( km, "Pi", 3.14159, "a double" )` and `astMapPut0C( km, "Name", "value", NULL )` |
  | `table` | Table | as `table_probe.c:386` builds its first table |
  | `fitstable` | FitsTable | as `fitstable_probe.c:216` builds its first table |
  | `channel` | Channel | `astChannel( NULL, NULL, "Comment=0,Indent=2" )` |
  | `mocchan` | MocChan | `astMocChan( NULL, NULL, "MocFormat=JSON" )` |
  | `fitschan` | FitsChan | `astFitsChan( NULL, NULL, " " )` with two cards added by `astPutFits( fc, "NAXIS   = 2", 0 )` and `astPutFits( fc, "CTYPE1  = 'RA---TAN'", 0 )` |

  `build_constructed` annuls each object after its `emit`.

- [ ] **Step 3: Build and run the probe.**

  ```sh
  cd /Users/timj/work/lsst/starlink-astrs/tools/oracle_fixtures
  R=$PWD/../../reference/starlink-ast/build-ast
  cc show_tostring_probe.c grf_stubs.c -o $SCRATCH/show_tostring_probe \
    -I "$R" -L "$R" -Wl,-rpath,"$R" \
    -last -last_pal -last_cminpack -lyaml -lm
  mkdir -p ../../crates/ast-core/tests/show_tostring_oracle
  LD_LIBRARY_PATH= $SCRATCH/show_tostring_probe > ../../crates/ast-core/tests/show_tostring_oracle/cases.rs
  ```

  Expected: exit 0; `cases.rs` holds 57 entries (41 fixtures, `zoommap_with_id`, 15 constructed); `rg -c '^    \($' cases.rs` prints `57`.
  Check that no captured text contains `"####`, which would end a raw string early.
  The binary goes to the scratchpad, not `tools/oracle_fixtures/`.

- [ ] **Step 4: Check the class column covers every `AnyObject` variant.**

  ```sh
  rg -o '^        "[A-Za-z]+",$' crates/ast-core/tests/show_tostring_oracle/cases.rs | sort -u | wc -l
  ```

  Expected: 56, one per `AnyObject` variant (the AST class names: `Frame` for `BasicFrame`, `Box` for `BoxRegion`, `Channel` for `NativeChannel`).

- [ ] **Step 5: Commit.**

  ```sh
  git add tools/oracle_fixtures/show_tostring_probe.c crates/ast-core/tests/show_tostring_oracle/cases.rs
  git commit -m "Record astShow and astToString output for every class"
  ```

---

### Task 2: `show`, `to_native_string` and `TryFrom<AnyObject>`

**Files:**
- Create: `crates/ast-core/src/serial/object_text.rs`
- Modify: `crates/ast-core/src/serial/mod.rs` (declare the module; add `AnyObject::class_name`)
- Modify: `crates/ast-core/src/channel/native.rs:143-153` (build `RenderOptions` with the shared helper)
- Create: `crates/ast-core/tests/test_show_tostring_oracle.rs`
- Modify: `crates/ast-core/tests/serial/main.rs` (add the module)

**Interfaces:**
- Consumes: `CASES` from Task 1.
- Produces, on every class in `AnyObject` and on `AnyObject`:
  - `pub fn show(&self) -> AstResult<()>`
  - `pub fn to_native_string(&self) -> AstResult<String>`
  - `impl TryFrom<AnyObject> for <Class> { type Error = AstError; }`
- Produces `pub fn AnyObject::class_name(&self) -> &'static str` (the AST class name, e.g. `"Box"`).

- [ ] **Step 1: Write the failing oracle test** in `crates/ast-core/tests/test_show_tostring_oracle.rs`.

  ```rust
  //! `show` and `to_native_string` against C's `astShow` and `astToString`.
  //!
  //! `show_tostring_oracle/cases.rs` is generated by
  //! `tools/oracle_fixtures/show_tostring_probe.c`: one object of each class,
  //! with the text C writes for each. Each case is read back from C's `astShow`
  //! text and must write both texts exactly.

  use ast_core::AnyObject;
  use ast_core::mappings::ZoomMap;

  use crate::common::{assert_c_text, from_native_string};

  #[path = "show_tostring_oracle/cases.rs"]
  mod cases;
  use cases::CASES;

  #[test]
  fn to_native_string_matches_ast_to_string() {
      for (name, class, show, compact) in CASES {
          let obj = from_native_string(show).unwrap_or_else(|e| panic!("{name}: {e}"));
          assert_eq!(obj.class_name(), *class, "{name}");
          assert_c_text(&obj.to_native_string().unwrap(), compact, name);
      }
  }

  #[test]
  fn show_text_matches_ast_show() {
      for (name, _, show, _) in CASES {
          let obj = from_native_string(show).unwrap();
          assert_c_text(&obj.show_text().unwrap(), show, name);
      }
  }

  #[test]
  fn compact_text_reads_back_to_the_same_object() {
      for (name, _, _, compact) in CASES {
          let obj = from_native_string(compact).unwrap_or_else(|e| panic!("{name}: {e}"));
          assert_eq!(obj.to_native_string().unwrap(), *compact, "{name}");
      }
  }

  #[test]
  fn every_anyobject_class_has_a_case() {
      let mut classes: Vec<&str> = CASES.iter().map(|c| c.1).collect();
      classes.sort_unstable();
      classes.dedup();
      assert_eq!(classes.len(), AnyObject::CLASS_NAMES.len());
      for c in AnyObject::CLASS_NAMES {
          assert!(classes.contains(c), "no oracle case for {c}");
      }
  }

  #[test]
  fn a_concrete_class_writes_what_its_anyobject_writes() {
      let map = ZoomMap::new(2, 4.0).unwrap();
      let any = AnyObject::ZoomMap(map.clone());
      assert_eq!(map.to_native_string().unwrap(), any.to_native_string().unwrap());
  }

  #[test]
  fn try_from_returns_the_class_and_rejects_another() {
      let any = AnyObject::ZoomMap(ZoomMap::new(2, 4.0).unwrap());
      let map = ZoomMap::try_from(any.clone()).unwrap();
      assert_eq!(map.zoom(), 4.0);
      let err = ast_core::mappings::PermMap::try_from(any).unwrap_err();
      assert_eq!(err.code(), ast_core::error::ErrorCode::Objin);
      assert_eq!(
          err.message(),
          "Pointer to PermMap required, but pointer to ZoomMap given."
      );
  }

  #[test]
  fn try_from_unboxes_boxed_variants() {
      let km = ast_core::KeyMap::new();
      let back = ast_core::KeyMap::try_from(AnyObject::KeyMap(Box::new(km))).unwrap();
      assert_eq!(back.size(), 0);
  }
  ```

  `show_text` is a `#[doc(hidden)] pub` method on `AnyObject` returning the string `show` prints, so the test does not capture standard output; `CLASS_NAMES` is a `pub const &[&str]` on `AnyObject` generated by the same macro.
  Use the accessor names `ZoomMap::zoom`, `AstError::message`, `KeyMap::size` as they exist; if one is named differently, use the existing name (check with `rg -n 'pub fn (zoom|message|size)\b' crates/ast-core/src`).

  Register the module in `crates/ast-core/tests/serial/main.rs` next to `test_object_id_oracle`:

  ```rust
  #[path = "../test_show_tostring_oracle.rs"]
  mod test_show_tostring_oracle;
  ```

- [ ] **Step 2: Run it to verify it fails.**

  Run: `cargo test -p ast-core --test serial test_show_tostring_oracle::`
  Expected: compile errors — no method `to_native_string` on `AnyObject`, no `class_name`, `show_text`, `CLASS_NAMES`, no `TryFrom`.

- [ ] **Step 3: Share the option mapping with `NativeChannel`.**
  In `crates/ast-core/src/serial/native_write.rs`, add:

  ```rust
  impl RenderOptions {
      /// The options a Native Channel with `config` writes with. C's
      /// indentation loops run no times for a negative Indent, so it writes
      /// what an Indent of 0 would.
      pub(crate) fn from_config(config: &crate::channel::ChannelConfig) -> Self {
          RenderOptions {
              comment: config.comment(),
              full: config.full() as i8,
              indent: config.indent().max(0) as usize,
          }
      }
  }
  ```

  and replace the struct literal in `NativeChannel::write` (`channel/native.rs:146-152`) with `RenderOptions::from_config(&self.base.config)`.

- [ ] **Step 4: Write `crates/ast-core/src/serial/object_text.rs`.**

  ```rust
  //! `show`, `to_native_string` and `TryFrom<AnyObject>` for every class.
  //!
  //! C's `astShow` writes an Object through a Channel with no attributes set,
  //! to standard output; `astToString` writes it through a Channel set to
  //! `Comment=0,Full=-1,Indent=0`, ending each line with a newline
  //! (`object.c` `Show`, `astToString_`). Both render the class's own node, so
  //! neither copies the object into an [`AnyObject`].

  use std::io::Write;

  use super::native_write::{RenderOptions, render_native_with_options};
  use super::{AnyObject, Serializable};
  use crate::channel::ChannelConfig;
  use crate::error::{AstError, AstResult, ErrorCode};

  /// The text C's `astShow` writes for `obj`.
  pub(crate) fn show_text<T: Serializable + ?Sized>(obj: &T) -> AstResult<String> {
      let options = RenderOptions::from_config(&ChannelConfig::default());
      render_native_with_options(&obj.to_node()?, &options)
  }

  /// The text C's `astToString` returns for `obj`.
  pub(crate) fn compact_text<T: Serializable + ?Sized>(obj: &T) -> AstResult<String> {
      let mut config = ChannelConfig::default();
      config.set_comment(false);
      config.set_full(-1);
      config.set_indent(0);
      render_native_with_options(&obj.to_node()?, &RenderOptions::from_config(&config))
  }

  /// Print `text` to standard output. Write errors are ignored, as C's
  /// default Channel sink, `printf`, ignores them.
  pub(crate) fn print_text(text: &str) {
      let _ = std::io::stdout().lock().write_all(text.as_bytes());
  }

  /// C's `astMAKE_CHECK` error for an Object of the wrong class.
  pub(crate) fn wrong_class(wanted: &str, got: &AnyObject) -> AstError {
      AstError::new(
          ErrorCode::Objin,
          format!(
              "Pointer to {wanted} required, but pointer to {} given.",
              got.class_name()
          ),
      )
  }
  ```

  Confirm the line endings: `render_native_with_options` must return text whose every line ends in `\n`, which is what `NativeChannel`'s string sink produced; if it returns text without a final newline, append one in both functions so that `show_text` equals what the free `to_native_string` returned.
  Use `AstError::new`'s actual signature (`rg -n 'pub fn new' crates/ast-core/src/error.rs`).

- [ ] **Step 5: Generate the methods with one macro** at the end of `object_text.rs`.
  The variant list is `AnyObject`'s (`serial/mod.rs:1865`); the macro takes `(Variant, Type, "AstClass", boxed-or-not)` rows.

  ```rust
  macro_rules! object_text {
      ($( $variant:ident : $ty:ty = $class:literal $(, $boxed:ident)? ;)*) => {
          $(
              impl $ty {
                  /// Write this object's Native description to standard output,
                  /// as C's `astShow` does: comments included, default values
                  /// left out, nested objects indented.
                  pub fn show(&self) -> AstResult<()> {
                      print_text(&show_text(self)?);
                      Ok(())
                  }

                  /// This object's minimal Native serialisation, as C's
                  /// `astToString` returns it: no comments, no default values,
                  /// no indentation, each line ending in a newline. Read it back
                  /// with [`from_native_string`](crate::from_native_string).
                  pub fn to_native_string(&self) -> AstResult<String> {
                      compact_text(self)
                  }
              }

              impl TryFrom<AnyObject> for $ty {
                  type Error = AstError;

                  /// The object, if `obj` holds this class; otherwise C's
                  /// AST__OBJIN error naming both classes.
                  fn try_from(obj: AnyObject) -> AstResult<Self> {
                      match obj {
                          AnyObject::$variant(x) => Ok(object_text!(@unbox x $(, $boxed)?)),
                          other => Err(wrong_class($class, &other)),
                      }
                  }
              }
          )*

          impl AnyObject {
              /// The AST class name of every variant, in declaration order.
              pub const CLASS_NAMES: &'static [&'static str] = &[$($class),*];

              /// The AST class name of the object held, as C's `astGetClass`
              /// returns it (`"Box"` for a [`BoxRegion`](crate::regions::BoxRegion)).
              pub fn class_name(&self) -> &'static str {
                  match self { $(AnyObject::$variant(_) => $class,)* }
              }

              /// Write the object's Native description to standard output,
              /// as C's `astShow` does.
              pub fn show(&self) -> AstResult<()> {
                  print_text(&self.show_text()?);
                  Ok(())
              }

              /// The text [`show`](Self::show) prints.
              #[doc(hidden)]
              pub fn show_text(&self) -> AstResult<String> {
                  match self { $(AnyObject::$variant(x) => show_text(object_text!(@ref x $(, $boxed)?)),)* }
              }

              /// The object's minimal Native serialisation, as C's
              /// `astToString` returns it.
              pub fn to_native_string(&self) -> AstResult<String> {
                  match self { $(AnyObject::$variant(x) => compact_text(object_text!(@ref x $(, $boxed)?)),)* }
              }
          }
      };
      (@unbox $x:ident, boxed) => { *$x };
      (@unbox $x:ident) => { $x };
      (@ref $x:ident, boxed) => { &**$x };
      (@ref $x:ident) => { $x };
  }
  ```

  Matching on `&self` binds `x` as `&T` for a plain variant and `&Box<T>` for a boxed one; the `@ref` arm turns both into `&T`.

  Invoke it with all 56 rows, for example:

  ```rust
  object_text! {
      UnitMap: crate::mappings::UnitMap = "UnitMap";
      ZoomMap: crate::mappings::ZoomMap = "ZoomMap";
      // ... one row per AnyObject variant, in the enum's order ...
      BasicFrame: crate::frames::BasicFrame = "Frame";
      BoxRegion: crate::regions::BoxRegion = "Box";
      KeyMap: crate::keymap::KeyMap = "KeyMap", boxed;
      Table: crate::table::Table = "Table", boxed;
      FitsTable: crate::fitstable::FitsTable = "FitsTable", boxed;
      NativeChannel: crate::channel::NativeChannel = "Channel", boxed;
      MocChan: crate::channel::MocChan = "MocChan", boxed;
      FitsChan: crate::fitschan::FitsChan = "FitsChan", boxed;
  }
  ```

  Take each class string from that type's `Serializable::class_name`, not from memory (`rg -n 'fn class_name' -A2 crates/ast-core/src`).
  Declare the module in `serial/mod.rs`: `mod object_text;`.

- [ ] **Step 6: Run the tests to verify they pass.**

  Run: `cargo test -p ast-core --test serial test_show_tostring_oracle::`
  Expected: 7 passed.
  A byte difference in a case is a port defect to fix in that class's `to_node` or in the renderer, not in the expected text; diagnose it with the simplify-style diff of the two texts before changing code.

- [ ] **Step 7: Run the whole `ast-core` suite, fmt and clippy.**

  Run: `cargo test -p ast-core && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo clippy --workspace --all-targets --features ast-bench/c-oracle -- -D warnings`
  Expected: all pass.

- [ ] **Step 8: Commit.**

  ```sh
  git add crates/ast-core/src/serial crates/ast-core/src/channel/native.rs \
    crates/ast-core/tests/test_show_tostring_oracle.rs crates/ast-core/tests/serial/main.rs
  git commit -m "Add show, to_native_string and TryFrom<AnyObject> to every class"
  ```

---

### Task 3: Remove the free `to_native_string`

**Files:**
- Modify: `crates/ast-core/src/serial/mod.rs:2629-2644` (delete the function and its section comment's mention)
- Modify: `crates/ast-core/src/lib.rs:30` (drop it from the re-export)
- Modify: `crates/ast-core/tests/common/mod.rs` (add `native_dump`)
- Modify: the 100 test files, `crates/ast-core/src/mappings/dssmap.rs` (inline tests), `crates/ast-core/examples/simp_one.rs`, `crates/ast-core/examples/wcsconv_debug.rs`

**Interfaces:**
- Consumes: `AnyObject::to_native_string` from Task 2.
- Produces: `crate::common::native_dump(obj: &AnyObject) -> AstResult<String>`, the default-Channel dump the free function wrote.

- [ ] **Step 1: Add the helper** to `crates/ast-core/tests/common/mod.rs`:

  ```rust
  /// The Native dump a Channel with no attributes set writes for `obj`:
  /// comments included and nested objects indented, as the reference
  /// fixtures were written.
  pub fn native_dump(obj: &ast_core::AnyObject) -> ast_core::error::AstResult<String> {
      use ast_core::{Channel, NativeChannel};
      let mut ch = NativeChannel::with_string_sink();
      ch.write(obj)?;
      Ok(ch.into_string())
  }
  ```

- [ ] **Step 2: Remove the free function and its re-export**, then confirm the build breaks where it is used.

  Run: `cargo test -p ast-core --no-run 2>&1 | rg -c 'to_native_string'`
  Expected: a non-zero count of unresolved-import and unresolved-name errors.

- [ ] **Step 3: Rewrite the call sites with a script** in the scratchpad (not committed):

  ```python
  import pathlib, re
  root = pathlib.Path("crates/ast-core/tests")
  for p in root.glob("*.rs"):
      s = p.read_text()
      if "to_native_string" not in s:
          continue
      t = s.replace("ast_core::to_native_string(", "native_dump(")
      def fix_use(m):
          items = [i.strip() for i in m.group(1).split(",") if i.strip() and i.strip() != "to_native_string"]
          return "use ast_core::{" + ", ".join(items) + "};" if len(items) != 1 else f"use ast_core::{items[0]};"
      t = re.sub(r"use ast_core::\{([^}]*\bto_native_string\b[^}]*)\};", fix_use, t)
      t = t.replace("use ast_core::to_native_string;\n", "")
      t = re.sub(r"(?<![.\w])to_native_string\(", "native_dump(", t)
      if "native_dump(" in t and "use crate::common::native_dump;" not in t:
          # After the file's leading //! block, or at the top if it has none.
          m = re.match(r"(?://!.*\n)*\n?", t)
          t = t[: m.end()] + "use crate::common::native_dump;\n" + t[m.end():]
      p.write_text(t)
  ```

  Method calls (`.to_native_string(`) are left alone by the negative look-behind.
  Then fix by hand what remains:
  - `src/mappings/dssmap.rs` inline tests: call `obj.to_native_string()` only where the test compares two dumps of its own; where it compares against a commented fixture, build a `NativeChannel` with a string sink as `native_dump` does.
  - `examples/simp_one.rs` and `examples/wcsconv_debug.rs`: their output is diffed against C's commented dumps (`tools/oracle_fixtures/README.md`), so they write through `NativeChannel::with_string_sink()` as `native_dump` does.

- [ ] **Step 4: Build, then run the full suite.**

  Run: `cargo test -p ast-core --no-run && cargo test -p ast-core`
  Expected: builds; every test passes with the same count as before Task 3 (the helper writes exactly what the free function wrote).
  Also: `rg -n '\bto_native_string\b' crates --type rust | rg -v '\.to_native_string\(|fn to_native_string'` prints nothing.

- [ ] **Step 5: fmt, clippy, and commit.**

  ```sh
  cargo fmt && cargo clippy --workspace --all-targets -- -D warnings
  git add crates/ast-core
  git commit -m "Remove the free to_native_string in favour of the method"
  ```

---

### Task 4: Record the change in the design documents

**Files:**
- Modify: `docs/superpowers/specs/2026-04-20-rust-port-design.md` (revision history)
- Modify: `CLAUDE.md` ("AST documentation" pointer)
- Modify: `PLAN.md` §6, the "C-oracle tests off macOS" row (file count)

- [ ] **Step 1: Add a revision entry** at the top of the spec's revision list, in its existing format, dated 2026-10-19:
  "`show()` and `to_native_string()` on every class and on `AnyObject`, matching C's `astShow` and `astToString`; the free `to_native_string(&AnyObject)` is removed (it wrote the `astShow` text under the `astToString` name); `TryFrom<AnyObject>` on every class reports a wrong class as `AST__OBJIN`. Design: `2026-10-19-show-and-native-string-design.md`."

- [ ] **Step 2: Correct `CLAUDE.md`.** Replace the two bullets under "AST documentation" with:

  ```markdown
  - **AST documentation**: one LaTeX source, `reference/starlink-ast/sun_master.tex`, is built by `builddocs` into two editions of the same guide and reference:
    - SUN/211 (C): https://www.starlink.ac.uk/docs/sun211.htx/sun211.html
    - SUN/210 (Fortran): https://www.starlink.ac.uk/docs/sun210.htx/sun210.html
  ```

- [ ] **Step 3: Update PLAN §6.** The new oracle test compares with `assert_c_text`, so the "C-oracle tests off macOS" row's file count rises by one; change "24 files" to "25 files" in that row (check the current number first with `rg -n 'C-oracle tests off macOS' PLAN.md`).

- [ ] **Step 4: Run the meta tests** (they check CLAUDE.md and PLAN.md citations).

  Run: `cargo test -p ast-core --test meta`
  Expected: pass.

- [ ] **Step 5: Commit.**

  ```sh
  git add docs/superpowers/specs/2026-04-20-rust-port-design.md CLAUDE.md PLAN.md
  git commit -m "Record show and to_native_string in the design and fix the SUN pointers"
  ```
