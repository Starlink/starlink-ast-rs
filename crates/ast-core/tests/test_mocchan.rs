//! `MocChan`: writing Mocs as text and reading them back.
//!
//! C ancestor: `reference/starlink-ast/ast_tester/testmocchan.c`. A Moc of
//! a few cells is written in string and then JSON form, the lines are
//! checked and read back to an equal Moc, and the `MocFormat` a read leaves
//! is checked; then a Moc made from a small Circle makes the same round
//! trips. The checks added upstream are `zero_line_len_is_too_small`
//! (`checkZeroLineLen`, `7ef81c01`), `dump_names_the_format`
//! (`checkDumpFormat`, `7ef81c01`) and `negative_line_len_loads_as_zero`
//! (`checkLoadNegativeLineLen`, `228a0310`).
//!
//! Divergence from C: the lines go to an in-memory sink and are read back
//! through `MocChan::from_string`, rather than through source and sink
//! functions over a static array. The C sink drops blank lines; none is
//! written.

use crate::common::native_dump;
use ast_core::error::ErrorCode;
use ast_core::frames::{SkyFrame, SkySystem};
use ast_core::mappings::Mapping;
use ast_core::regions::{CircleBuilder, CmpOperator, Moc};
use ast_core::{AnyObject, Channel, MocChan, MocFormat, from_native_string};

/// Write `moc` through a MocChan with the given format, returning the lines.
fn write(moc: &Moc, format: MocFormat) -> String {
    let mut ch = MocChan::with_string_sink();
    ch.set_moc_format(format);
    assert_eq!(ch.write(&AnyObject::Moc(moc.clone())).unwrap(), 1);
    ch.into_string()
}

/// Read a Moc from `text`, returning it and the MocFormat left behind.
fn read(text: &str) -> (Moc, MocFormat) {
    let mut ch = MocChan::from_string(text);
    let Some(AnyObject::Moc(moc)) = ch.read().unwrap() else {
        panic!("expected a Moc");
    };
    (moc, ch.moc_format())
}

#[test]
fn cells_round_trip() {
    let mut moc = Moc::new();
    moc.set_max_order(9).unwrap();
    for (order, npix) in [(7, 1000), (7, 1001), (7, 1002), (7, 997), (6, 500)] {
        moc.add_cell(CmpOperator::Or, order, npix).unwrap();
    }

    // Errors 1-3: the default format is strings, and is not set.
    let ch = MocChan::new();
    assert!(!ch.test_moc_format());
    let text = write(&moc, ch.moc_format());
    assert_eq!(text, "6/500 7/997,1000-1002 9/\n");

    // Errors 4-7.
    let (back, format) = read(&text);
    assert!(back.equal(&moc));
    assert_eq!(format, MocFormat::String);

    // Errors 8-9.
    let text = write(&moc, MocFormat::Json);
    assert_eq!(text, "{\"6\":[500],\"7\":[997,1000,1001,1002],\"9\":[]}\n");

    // Errors 10-12.
    let (back, format) = read(&text);
    assert!(back.equal(&moc));
    assert_eq!(format, MocFormat::Json);
}

#[test]
fn circle_round_trip() {
    let mut moc = Moc::new();
    moc.set_max_order(15).unwrap();
    moc.set_min_order(12);
    let circle = CircleBuilder::new(SkyFrame::new(SkySystem::Icrs), vec![1.0, 1.0])
        .unwrap()
        .radius(0.01)
        .build()
        .unwrap();
    moc.add_region(CmpOperator::Or, &circle).unwrap();

    // Errors 13-18.
    for format in [MocFormat::Unknown, MocFormat::Json] {
        let (back, _) = read(&write(&moc, format));
        assert!(back.equal(&moc), "{format:?}");
    }
}

#[test]
fn zero_line_len_is_too_small() {
    let mut moc = Moc::new();
    moc.add_cell(CmpOperator::Or, 3, 5).unwrap();
    let mut ch = MocChan::with_string_sink();
    ch.set_moc_line_len(-5);
    assert_eq!(ch.moc_line_len(), 0);
    let err = ch.write(&AnyObject::Moc(moc)).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Smbuf);
}

#[test]
fn dump_names_the_format() {
    for (format, name) in [(MocFormat::String, "STRING"), (MocFormat::Json, "JSON")] {
        let mut ch = MocChan::new();
        ch.set_moc_format(format);
        let text = native_dump(&AnyObject::MocChan(Box::new(ch))).unwrap();
        assert!(text.contains(&format!("MocEnc = \"{name}\"")), "{text}");
        let AnyObject::MocChan(back) = from_native_string(&text).unwrap() else {
            panic!("expected a MocChan");
        };
        assert_eq!(back.moc_format(), format);
    }
}

#[test]
fn negative_line_len_loads_as_zero() {
    let AnyObject::MocChan(ch) =
        from_native_string(" Begin MocChan\n    MocLLn = -3\n End MocChan").unwrap()
    else {
        panic!("expected a MocChan");
    };
    assert_eq!(ch.moc_line_len(), 0);
}

/// A MocEnc read from a dump matches as C's `astChrMatch` matches it, so
/// white space after the name is ignored.
#[test]
fn dump_format_ignores_trailing_white_space() {
    let mut ch = MocChan::new();
    ch.set_moc_format(MocFormat::Json);
    let text = native_dump(&AnyObject::MocChan(Box::new(ch))).unwrap();
    let text = text.replace("MocEnc = \"JSON\"", "MocEnc = \"json  \"");
    let AnyObject::MocChan(back) = from_native_string(&text).unwrap() else {
        panic!("expected a MocChan");
    };
    assert_eq!(back.moc_format(), MocFormat::Json);
}

// ===========================================================================
// SourceFile, SinkFile and standard input and output
// ===========================================================================

/// A path in the temporary directory that no other test uses.
fn scratch_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("ast_mocchan_{}_{name}", std::process::id()))
}

/// SourceFile is read in place of the source, clearing it restores the
/// source, and setting it again reads the file from its start.
// C: channel.c `SetSourceFile`, `ClearSourceFile` and `ReadData`, which
// MocChan inherits.
#[test]
fn source_file_replaces_source_and_clear_restores_it() {
    let path = scratch_path("source.moc");
    std::fs::write(&path, "1/3 2/37,101-103\n").unwrap();
    let name = path.to_str().unwrap();
    let from_file = read("1/3 2/37,101-103").0;

    let mut ch = MocChan::from_string("0/1");
    assert!(!ch.test_source_file());
    ch.set_source_file(name);
    assert!(ch.test_source_file());
    assert_eq!(ch.source_file(), Some(name));
    let AnyObject::Moc(moc) = ch.read().unwrap().unwrap() else {
        panic!("not a Moc");
    };
    assert_eq!(
        write(&moc, MocFormat::String),
        write(&from_file, MocFormat::String)
    );

    ch.clear_source_file();
    assert!(!ch.test_source_file());
    let AnyObject::Moc(moc) = ch.read().unwrap().unwrap() else {
        panic!("not a Moc");
    };
    assert_eq!(
        write(&moc, MocFormat::String),
        write(&read("0/1").0, MocFormat::String)
    );

    // The file is opened again, from its start.
    ch.set_source_file(name);
    let AnyObject::Moc(moc) = ch.read().unwrap().unwrap() else {
        panic!("not a Moc");
    };
    assert_eq!(
        write(&moc, MocFormat::String),
        write(&from_file, MocFormat::String)
    );
    let _ = std::fs::remove_file(&path);
}

/// SinkFile is written in place of the sink, clearing it restores the sink,
/// and setting it again creates the file anew.
#[test]
fn sink_file_replaces_sink_and_clear_restores_it() {
    let first = scratch_path("sink1.moc");
    let second = scratch_path("sink2.moc");
    let a = AnyObject::Moc(read("1/3").0);
    let b = AnyObject::Moc(read("2/37").0);

    let mut ch = MocChan::with_string_sink();
    ch.set_sink_file(first.to_str().unwrap());
    assert!(ch.test_sink_file());
    assert_eq!(ch.sink_file(), first.to_str());
    ch.write(&a).unwrap();
    ch.set_sink_file(second.to_str().unwrap());
    ch.write(&b).unwrap();
    ch.clear_sink_file();
    assert!(!ch.test_sink_file());
    ch.write(&a).unwrap();
    // The first name again replaces the file's contents.
    ch.set_sink_file(first.to_str().unwrap());
    ch.write(&b).unwrap();
    ch.clear_sink_file();

    let a_moc = read("1/3").0;
    assert_eq!(
        std::fs::read_to_string(&first).unwrap(),
        write(&read("2/37").0, MocFormat::String)
    );
    assert_eq!(
        std::fs::read_to_string(&second).unwrap(),
        write(&read("2/37").0, MocFormat::String)
    );
    assert_eq!(ch.into_string(), write(&a_moc, MocFormat::String));
    let _ = std::fs::remove_file(&first);
    let _ = std::fs::remove_file(&second);
}

/// A SourceFile that cannot be opened is an error at the first read, and a
/// SinkFile at the first write.
#[test]
fn source_and_sink_file_open_failures() {
    let missing = scratch_path("no_such_dir").join("x.moc");
    let mut ch = MocChan::from_string("1/3");
    ch.set_source_file(missing.to_str().unwrap());
    let e = ch.read().unwrap_err();
    assert_eq!(e.code(), ErrorCode::Rderr);
    assert!(
        e.message()
            .contains("astRead(MocChan): Failed to open input SourceFile"),
        "{e}"
    );

    let mut ch = MocChan::with_string_sink();
    ch.set_sink_file(missing.to_str().unwrap());
    let e = ch.write(&AnyObject::Moc(read("1/3").0)).unwrap_err();
    assert_eq!(e.code(), ErrorCode::Wrerr);
    assert!(
        e.message()
            .contains("astWrite(MocChan): Failed to open output SinkFile"),
        "{e}"
    );
}

/// A clone has neither attribute.
#[test]
fn clone_does_not_inherit_source_file_or_sink_file() {
    let mut ch = MocChan::new();
    ch.set_source_file("a.moc");
    ch.set_sink_file("b.moc");
    let copy = ch.clone();
    assert!(!copy.test_source_file());
    assert!(!copy.test_sink_file());
}

/// The variable that tells a copy of this test program which standard I/O
/// check it is the child of.
const STDIO_CHILD: &str = "AST_TEST_MOCCHAN_STDIO_CHILD";

/// Run the test `name` in a child of this test program with standard input
/// `input` and `mode` in [`STDIO_CHILD`], returning its standard output.
fn run_stdio_child(name: &str, mode: &str, input: &str) -> String {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--nocapture", "--test-threads=1"])
        .env(STDIO_CHILD, mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "{mode}: {stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

/// A MocChan with no source reads standard input.
// C: channel.c `ReadData` reads stdin when the Channel has neither a
// SourceFile nor a source function.
#[test]
fn mocchan_without_source_reads_standard_input() {
    let name = "test_mocchan::mocchan_without_source_reads_standard_input";
    match std::env::var(STDIO_CHILD).as_deref() {
        Ok("read") => {
            let mut ch = MocChan::new();
            let AnyObject::Moc(moc) = ch.read().unwrap().unwrap() else {
                panic!("not a Moc");
            };
            assert_eq!(
                write(&moc, MocFormat::String),
                write(&read("1/3 2/37,101-103").0, MocFormat::String)
            );
        }
        Ok(_) => {}
        Err(_) => {
            run_stdio_child(name, "read", "1/3 2/37,101-103\n");
        }
    }
}

/// A MocChan with no sink writes standard output.
// C: channel.c `WriteData` printfs to stdout when the Channel has neither a
// SinkFile nor a sink function.
#[test]
fn mocchan_without_sink_writes_standard_output() {
    let name = "test_mocchan::mocchan_without_sink_writes_standard_output";
    match std::env::var(STDIO_CHILD).as_deref() {
        Ok("write") => {
            let mut ch = MocChan::new();
            ch.write(&AnyObject::Moc(read("1/3 2/37").0)).unwrap();
        }
        Ok(_) => {}
        Err(_) => {
            let out = run_stdio_child(name, "write", "");
            assert!(
                out.contains(&write(&read("1/3 2/37").0, MocFormat::String)),
                "{out}"
            );
        }
    }
}
