//! The reference C library as a second engine, loaded at runtime from the
//! shared library `tools/perf/build_c_engine.sh` builds.
//!
//! The library is opened with libloading's default flags, which on Unix
//! include `RTLD_LOCAL`, so its copies of PAL and ERFA stay private and cannot
//! clash with the ones compiled into `ast-core`.

use std::collections::BTreeMap;
use std::ffi::{CString, c_char, c_int, c_void};
use std::path::{Path, PathBuf};

use libloading::Library;

/// Which build of the C library to load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// The timing build.
    Bench,
    /// The build that counts calls into libm, PAL, ERFA and wcslib.
    Count,
}

impl Variant {
    fn name(self) -> &'static str {
        match self {
            Variant::Bench => "bench",
            Variant::Count => "count",
        }
    }

    /// Where `tools/perf/build_c_engine.sh` puts this variant, unless
    /// `AST_BENCH_C_LIB` names another file.
    pub fn default_path(self) -> PathBuf {
        if let Some(p) = std::env::var_os("AST_BENCH_C_LIB") {
            return PathBuf::from(p);
        }
        let ext = if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/perf/c")
            .join(self.name())
            .join(format!("libast_bench_engine.{ext}"))
    }
}

type LoadFn = unsafe extern "C" fn(*const c_char, *const c_char, *mut c_char, c_int) -> *mut c_void;
type DimFn = unsafe extern "C" fn(*mut c_void) -> c_int;
type TranFn =
    unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, *const f64, c_int, *mut f64) -> c_int;
type FreeFn = unsafe extern "C" fn(*mut c_void);
type ResetFn = unsafe extern "C" fn() -> c_int;
type DumpFn = unsafe extern "C" fn(*mut c_char, c_int) -> c_int;

/// A loaded C engine.
pub struct CEngine {
    path: PathBuf,
    load: LoadFn,
    nin: DimFn,
    nout: DimFn,
    tran: TranFn,
    free: FreeFn,
    reset: ResetFn,
    dump: DumpFn,
    // Keeps the code the function pointers above point into mapped.
    _lib: Library,
}

impl CEngine {
    /// Load the given variant from its default path.
    pub fn open(v: Variant) -> Result<CEngine, String> {
        CEngine::open_path(&v.default_path(), v)
    }

    /// Load a C engine from `path`; `v` names the build command in errors.
    pub fn open_path(path: &Path, v: Variant) -> Result<CEngine, String> {
        let fail = |e: &dyn std::fmt::Display| {
            format!(
                "cannot load the C engine {}: {e}\nbuild it with: tools/perf/build_c_engine.sh {}",
                path.display(),
                v.name()
            )
        };
        // SAFETY: loading runs the library's initialisers; the library is the
        // reference AST build, whose initialisers only set up static tables.
        let lib = unsafe { Library::new(path) }.map_err(|e| fail(&e))?;
        // SAFETY: each symbol is declared in crates/ast-bench/c/bench_engine.h
        // with exactly the signature of the type it is read as.
        unsafe {
            let load = *lib.get::<LoadFn>(b"abe_load").map_err(|e| fail(&e))?;
            let nin = *lib.get::<DimFn>(b"abe_nin").map_err(|e| fail(&e))?;
            let nout = *lib.get::<DimFn>(b"abe_nout").map_err(|e| fail(&e))?;
            let tran = *lib.get::<TranFn>(b"abe_tran").map_err(|e| fail(&e))?;
            let free = *lib.get::<FreeFn>(b"abe_free").map_err(|e| fail(&e))?;
            let reset = *lib
                .get::<ResetFn>(b"abe_counts_reset")
                .map_err(|e| fail(&e))?;
            let dump = *lib
                .get::<DumpFn>(b"abe_counts_dump")
                .map_err(|e| fail(&e))?;
            Ok(CEngine {
                path: path.to_path_buf(),
                load,
                nin,
                nout,
                tran,
                free,
                reset,
                dump,
                _lib: lib,
            })
        }
    }

    /// The `build-info.txt` written beside the library, or a note that it is
    /// missing.
    pub fn build_info(&self) -> String {
        let p = self.path.with_file_name("build-info.txt");
        std::fs::read_to_string(&p)
            .map(|s| s.trim().replace('\n', "; "))
            .unwrap_or_else(|_| format!("no {}", p.display()))
    }

    /// Load the fixture `relpath` under `root` as C's oracle generator does.
    pub fn load(&self, root: &Path, relpath: &str) -> Result<CMapping<'_>, String> {
        let root_c = CString::new(root.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        let rel_c = CString::new(relpath).map_err(|e| e.to_string())?;
        let mut err = vec![0 as c_char; 512];
        // SAFETY: both strings are nul-terminated and outlive the call; `err`
        // has the length passed.
        let map = unsafe {
            (self.load)(
                root_c.as_ptr(),
                rel_c.as_ptr(),
                err.as_mut_ptr(),
                err.len() as c_int,
            )
        };
        if map.is_null() {
            // SAFETY: abe_load nul-terminates `err` on failure.
            let msg = unsafe { std::ffi::CStr::from_ptr(err.as_ptr()) };
            return Err(msg.to_string_lossy().into_owned());
        }
        Ok(CMapping { engine: self, map })
    }

    /// Zero the counting build's counts.
    pub fn counts_reset(&self) -> Result<(), String> {
        // SAFETY: no arguments; returns -1 in the bench build.
        match unsafe { (self.reset)() } {
            0 => Ok(()),
            _ => Err(NOT_COUNTING.to_string()),
        }
    }

    /// The counting build's counts since the last reset, by C symbol name.
    pub fn counts(&self) -> Result<BTreeMap<String, u64>, String> {
        let mut buf = vec![0 as c_char; 1 << 16];
        // SAFETY: `buf` has the length passed; the C side nul-terminates.
        let n = unsafe { (self.dump)(buf.as_mut_ptr(), buf.len() as c_int) };
        if n < 0 {
            return Err(NOT_COUNTING.to_string());
        }
        // SAFETY: as above.
        let text = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_string_lossy();
        text.lines()
            .map(|l| {
                let (k, v) = l
                    .split_once('\t')
                    .ok_or_else(|| format!("bad counts line {l:?}"))?;
                Ok((k.to_string(), v.parse::<u64>().map_err(|e| e.to_string())?))
            })
            .collect()
    }
}

const NOT_COUNTING: &str =
    "this is the bench build; call counts need tools/perf/build_c_engine.sh count";

/// A Mapping held by the C engine; freed on drop.
pub struct CMapping<'a> {
    engine: &'a CEngine,
    map: *mut c_void,
}

impl CMapping<'_> {
    /// Number of input coordinates.
    pub fn nin(&self) -> usize {
        // SAFETY: `map` is a live handle from abe_load.
        unsafe { (self.engine.nin)(self.map) as usize }
    }

    /// Number of output coordinates.
    pub fn nout(&self) -> usize {
        // SAFETY: as for `nin`.
        unsafe { (self.engine.nout)(self.map) as usize }
    }

    /// Transform `npoint` points held one axis after another, C's layout.
    pub fn tran(
        &self,
        forward: bool,
        npoint: usize,
        ncoord_in: usize,
        input: &[f64],
        ncoord_out: usize,
        out: &mut [f64],
    ) -> Result<(), String> {
        if input.len() != npoint * ncoord_in || out.len() != npoint * ncoord_out {
            return Err("C tran: buffer sizes do not match npoint and ncoord".into());
        }
        // SAFETY: the buffers have the sizes checked above, and `map` is live.
        let rc = unsafe {
            (self.engine.tran)(
                self.map,
                c_int::from(forward),
                npoint as c_int,
                ncoord_in as c_int,
                input.as_ptr(),
                ncoord_out as c_int,
                out.as_mut_ptr(),
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err("C transform raised an AST error".into())
        }
    }
}

impl Drop for CMapping<'_> {
    fn drop(&mut self) {
        // SAFETY: `map` came from abe_load and is freed exactly once.
        unsafe { (self.engine.free)(self.map) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::{CORE_FIXTURES, parse_oracle};

    #[test]
    fn missing_library_names_the_build_command() {
        let e = CEngine::open_path(Path::new("/nonexistent/libx.dylib"), Variant::Bench)
            .err()
            .unwrap();
        assert!(e.contains("tools/perf/build_c_engine.sh bench"), "{e}");
    }

    #[test]
    fn c_reproduces_its_own_oracle_rows() {
        let eng = CEngine::open(Variant::Bench).unwrap();
        let text =
            std::fs::read_to_string(format!("{CORE_FIXTURES}/oracle/simplify_fixtures.oracle"))
                .unwrap();
        let s = parse_oracle(&text)
            .unwrap()
            .into_iter()
            .find(|s| s.relpath == "simplify/cap_slamap_01.simp" && s.forward)
            .unwrap();
        let m = eng.load(Path::new(CORE_FIXTURES), &s.relpath).unwrap();
        assert_eq!((m.nin(), m.nout()), (s.nin, s.nout));
        let n = s.npoint();
        let mut out = vec![0.0; s.nout * n];
        m.tran(s.forward, n, s.nin, &s.inputs.concat(), s.nout, &mut out)
            .unwrap();
        // The oracle was captured on another platform, so allow the golden
        // tolerance of crates/ast-core/tests/test_transform_oracle.rs.
        for (got, want) in out.iter().zip(s.outputs.concat()) {
            assert!(
                (got - want).abs() <= 1e-7 + 1e-7 * want.abs(),
                "{got} vs {want}"
            );
        }
    }

    #[test]
    fn bench_build_has_no_counts() {
        let eng = CEngine::open(Variant::Bench).unwrap();
        assert!(eng.counts_reset().unwrap_err().contains("count"));
    }
}
