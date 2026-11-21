// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Attributes by name: setting, getting, testing and clearing an object's
//! attributes from strings such as `"System=ICRS, Label(1)=First axis"`.
//!
//! The everyday API of this crate is typed: each attribute has its own
//! getter, setter, `test_` and `clear_` methods.
//! A program that takes attribute settings as text, from a configuration
//! file or a user, needs the names and the syntax of the AST documentation's
//! attribute reference instead, and this module gives them: the `Attributes`
//! trait, compiled with the `attribute-strings` feature, which accepts the
//! strings the C library accepts, and gives the same values and the same
//! errors.
//!
//! It covers `Plot`, `FrameSet`, the Frame classes `BasicFrame`, `SkyFrame`,
//! `SpecFrame`, `DSBSpecFrame`, `TimeFrame` and `CmpFrame`, and `FitsChan`.
//! Used on any other class it fails with an error saying that the class is
//! not yet supported.
// The rest of the module is private to the crate; a class's own names are in
// the file named after it.
//
// How C's attribute functions map onto this module. In C each class has four
// virtual functions, SetAttrib, GetAttrib, TestAttrib and ClearAttrib; a
// class's function handles the names the class owns and passes any other to
// the function of its parent class, which it saved when its virtual function
// table was built. Here a class's function is a free function in this
// module, `<class>_set` and so on, generic over the object, and it ends by
// calling its parent's function by name. The virtual dispatch from an object
// of unknown class (a FrameSet's current Frame, a CmpFrame's component) is
// the hidden `attr_set`, `attr_get`, `attr_test` and `attr_clear` methods of
// the `Frame` trait, which each class implements by calling its own function.
// C's `astSet` and `astClear` are themselves virtual (`VSet`, `Clear`), which
// FrameSet overrides to restore its integrity once per call: those are the
// hidden `attr_vset` and `attr_clear_list`.
//
// C reports an error as a series of messages under one status. The first is
// the `AstError`'s message and the others are its context, in order.

pub(crate) mod axis;
pub(crate) mod frame;
pub(crate) mod mapping;
pub(crate) mod object;
#[cfg(feature = "attribute-strings")]
mod public;
pub(crate) mod skyframe;
pub(crate) mod specframe;

#[cfg(feature = "attribute-strings")]
pub use public::Attributes;

use crate::error::{AstError, ErrorCode};
use crate::frames::axis_format::{c_strtod, sprintf_double};
use crate::serial::native_write::format_double_g_signed;

/// C's `isspace` in the "C" locale.
pub(crate) fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

// ---------------------------------------------------------------------------
// Splitting and normalising: astSet, astSetC, astGet, astTest and astClear
// ---------------------------------------------------------------------------

/// Mark the commas that separate assignments, by turning each into a
/// newline: every comma outside double quotes, but only when the text holds
/// more than one `=`.
// C: object.c VSet, "eq1 && strchr( eq1 + 1, '=' )".
fn mark_separators(buf: &mut [u8]) {
    let Some(eq1) = buf.iter().position(|&b| b == b'=') else {
        return;
    };
    if !buf[eq1 + 1..].contains(&b'=') {
        return;
    }
    let mut quoted = false;
    for b in buf {
        if !quoted {
            if *b == b',' {
                *b = b'\n';
            } else if *b == b'"' {
                quoted = true;
            }
        } else if *b == b'"' {
            quoted = false;
        }
    }
}

/// One assignment as C's `VSet` passes it to `astSetAttrib`: the name in
/// lower case without white space, the value unchanged but for delimiting
/// double quotes, and any `\r` restored to the comma it stands for.
fn assignment(assign: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(assign.len() + 1);
    let mut lo = true;
    let mut tq: i32 = -1;
    let mut quoted = false;
    for &c in assign {
        if c == b'=' {
            lo = false;
        }
        if !lo || !is_space(c) {
            if c == b'\r' {
                out.push(b',');
            } else if lo {
                out.push(c.to_ascii_lowercase());
            } else {
                out.push(c);
                if tq > 0 && !is_space(c) {
                    if c == b'"' {
                        quoted = true;
                        out.pop();
                    }
                    tq = 0;
                }
            }
        }
        if c == b'=' && tq == -1 {
            tq = 1;
        }
    }
    if quoted {
        // C steps back over trailing white space, then over one closing
        // quote.
        while out.last().is_some_and(|&b| is_space(b)) {
            out.pop();
        }
        if out.last() == Some(&b'"') {
            out.pop();
        }
    }
    out
}

/// The assignments in a buffer whose separators are newlines, each
/// normalised and blank ones dropped.
fn assignments(buf: &[u8]) -> Vec<String> {
    let mut list = Vec::new();
    for assign in buf.split(|&b| b == b'\n') {
        let mut out = assignment(assign);
        if out.is_empty() {
            continue;
        }
        // "Title=" would not match an attribute name without a value.
        if out.last() == Some(&b'=') {
            out.push(b' ');
        }
        list.push(String::from_utf8_lossy(&out).into_owned());
    }
    list
}

/// The assignments of a settings string, as C's `astSet` passes them one by
/// one to `astSetAttrib`: each `name=value` with the name in lower case and
/// without white space.
///
/// C takes the string as a `printf` format; this is the string
/// `astSet( obj, "%s", settings )` applies, with no format specifiers
/// expanded.
// C: object.c VSet.
pub(crate) fn split_settings(settings: &str) -> Vec<String> {
    let mut buf = settings.as_bytes().to_vec();
    mark_separators(&mut buf);
    assignments(&buf)
}

/// The assignments C's `astSetC( obj, name, value )` makes: usually one,
/// since the value's commas do not separate assignments.
// C: object.c astSetC_ replaces each comma of the value with "\r" and calls
// astSet with the format "<name>=%*s"; VSet looks for separating commas in
// that format, before the value is put into it, and turns each "\r" back
// into a comma as it normalises an assignment.
#[cfg_attr(not(feature = "attribute-strings"), allow(dead_code))]
pub(crate) fn split_setting_c(name: &str, value: &str) -> (Vec<String>, String) {
    let name = &name[..chr_len(name)];
    let mut buf = format!("{name}=").into_bytes();
    mark_separators(&mut buf);
    buf.extend(value.bytes().map(|b| if b == b',' { b'\r' } else { b }));
    let text = String::from_utf8_lossy(&buf).replace('\n', ",");
    (assignments(&buf), text)
}

/// The length of a string without its trailing white space.
// C: memory.c astChrLen.
pub(crate) fn chr_len(text: &str) -> usize {
    let b = text.as_bytes();
    let mut len = b.len();
    while len > 0 && is_space(b[len - 1]) {
        len -= 1;
    }
    len
}

/// An attribute name as C's `astGet` and `astTest` pass it on: without white
/// space and in lower case.
// C: object.c Get, astTest_.
pub(crate) fn normalise_name(name: &str) -> String {
    name.chars()
        .filter(|c| !(c.is_ascii() && is_space(*c as u8)))
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The names in a comma-separated list, as C's `astClear` passes them one by
/// one to `astClearAttrib`: each normalised, and blank ones dropped.
// C: object.c Clear.
pub(crate) fn split_names(names: &str) -> Vec<String> {
    names
        .split(',')
        .map(normalise_name)
        .filter(|n| !n.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// sscanf
// ---------------------------------------------------------------------------

/// One field of a [`sscanf`] result: a conversion of the format that is not
/// suppressed with `*`, in the format's order.
#[derive(Debug, Clone, PartialEq)]
enum Field {
    Int(i32),
    Double(f64),
    Text(String),
    /// `%n`: the number of bytes read when the scan reached it.
    Count(usize),
}

/// What C's `sscanf` gives for a format: the fields it would store through
/// its pointer arguments and its return value.
/// A field the scan did not reach is zero or empty, as it is in C for the
/// callers here, which zero a `%n` count before each scan and read the other
/// fields only after a match.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Scan {
    /// The number of fields converted and stored, or -1 if the input ran
    /// out before any conversion.
    pub(crate) ret: i32,
    fields: Vec<Field>,
}

impl Scan {
    /// The `%d` field stored through argument `i` (counted from zero).
    pub(crate) fn int(&self, i: usize) -> i32 {
        match self.fields.get(i) {
            Some(Field::Int(v)) => *v,
            _ => 0,
        }
    }

    /// The `%lg` field stored through argument `i`.
    pub(crate) fn double(&self, i: usize) -> f64 {
        match self.fields.get(i) {
            Some(Field::Double(v)) => *v,
            _ => 0.0,
        }
    }

    /// The `%s` or `%[` field stored through argument `i`.
    #[allow(dead_code)]
    pub(crate) fn text(&self, i: usize) -> &str {
        match self.fields.get(i) {
            Some(Field::Text(v)) => v,
            _ => "",
        }
    }

    /// The `%n` count stored through argument `i`.
    pub(crate) fn count(&self, i: usize) -> usize {
        match self.fields.get(i) {
            Some(Field::Count(v)) => *v,
            _ => 0,
        }
    }

    /// The count of the format's last `%n`, which the C callers name `nc`.
    fn nc(&self) -> usize {
        match self.fields.last() {
            Some(Field::Count(v)) => *v,
            _ => 0,
        }
    }
}

/// C's `sscanf( input, fmt, ... )` for the conversions the attribute
/// functions use: `%d`, `%lg`, `%s`, `%[...]` and `%n`, with `*` and a field
/// width.
// C: the attribute functions call astSscanf, which is sscanf itself unless
// AST was configured for a system whose sscanf mishandles a space before
// "%n" (memory.h); this is the standard function.
pub(crate) fn sscanf(input: &str, fmt: &str) -> Scan {
    let s = input.as_bytes();
    let f = fmt.as_bytes();
    let (mut i, mut k) = (0usize, 0usize);
    let mut fields = Vec::new();
    let mut assigned = 0;
    let mut converted = false;
    // Why the scan stopped, once it has: `true` for the input running out,
    // `false` for input that does not match.
    let mut stopped: Option<bool> = None;
    while k < f.len() {
        let c = f[k];
        if is_space(c) {
            k += 1;
            if stopped.is_none() {
                while i < s.len() && is_space(s[i]) {
                    i += 1;
                }
            }
            continue;
        }
        if c != b'%' {
            k += 1;
            if stopped.is_none() {
                if i >= s.len() {
                    stopped = Some(true);
                } else if s[i] != c {
                    stopped = Some(false);
                } else {
                    i += 1;
                }
            }
            continue;
        }
        k += 1;
        let suppress = f.get(k) == Some(&b'*');
        if suppress {
            k += 1;
        }
        let mut width = 0usize;
        while let Some(d) = f.get(k).filter(|d| d.is_ascii_digit()) {
            width = width * 10 + usize::from(d - b'0');
            k += 1;
        }
        let width = if width == 0 { usize::MAX } else { width };
        while matches!(f.get(k), Some(b'l' | b'h' | b'L')) {
            k += 1;
        }
        let Some(&conv) = f.get(k) else { break };
        k += 1;
        let mut set: &[u8] = &[];
        let mut negate = false;
        if conv == b'[' {
            if f.get(k) == Some(&b'^') {
                negate = true;
                k += 1;
            }
            let start = k;
            // A "]" straight after the opening is a member of the set.
            if f.get(k) == Some(&b']') {
                k += 1;
            }
            while k < f.len() && f[k] != b']' {
                k += 1;
            }
            set = &f[start..k];
            k = (k + 1).min(f.len());
        }
        if conv == b'n' {
            if !suppress {
                fields.push(Field::Count(if stopped.is_none() { i } else { 0 }));
            }
            continue;
        }
        let mut value = match conv {
            b'd' => Field::Int(0),
            b'e' | b'f' | b'g' => Field::Double(0.0),
            _ => Field::Text(String::new()),
        };
        let mut matched = false;
        if stopped.is_none() {
            // Every conversion but "%[" skips leading white space.
            if conv != b'[' {
                while i < s.len() && is_space(s[i]) {
                    i += 1;
                }
            }
            if i >= s.len() {
                stopped = Some(true);
            } else {
                let limit = s.len().min(i.saturating_add(width));
                let used = match conv {
                    b'd' => scan_int(&s[i..limit]).map(|(v, n)| {
                        value = Field::Int(v);
                        n
                    }),
                    b'e' | b'f' | b'g' => c_strtod(&s[i..limit]).map(|(v, n)| {
                        value = Field::Double(v);
                        n
                    }),
                    b's' | b'[' => {
                        let n = s[i..limit]
                            .iter()
                            .take_while(|&&b| {
                                if conv == b's' {
                                    !is_space(b)
                                } else {
                                    set.contains(&b) != negate
                                }
                            })
                            .count();
                        (n > 0).then(|| {
                            value = Field::Text(String::from_utf8_lossy(&s[i..i + n]).into_owned());
                            n
                        })
                    }
                    _ => None,
                };
                match used {
                    Some(n) => {
                        i += n;
                        matched = true;
                        converted = true;
                    }
                    None => stopped = Some(false),
                }
            }
        }
        if !suppress {
            fields.push(value);
            if matched {
                assigned += 1;
            }
        }
    }
    let ret = if stopped == Some(true) && !converted {
        -1
    } else {
        assigned
    };
    Scan { ret, fields }
}

/// A `%d` conversion at the start of `s`: an optional sign and decimal
/// digits, and the number of bytes they take. A value beyond the 64-bit
/// range saturates, as the C library's `strtol` does, and is then narrowed
/// to an `int` as C's assignment narrows it.
fn scan_int(s: &[u8]) -> Option<(i32, usize)> {
    let mut i = 0;
    let neg = match s.first() {
        Some(b'-') => {
            i = 1;
            true
        }
        Some(b'+') => {
            i = 1;
            false
        }
        _ => false,
    };
    let start = i;
    let mut v: i128 = 0;
    while let Some(d) = s.get(i).filter(|d| d.is_ascii_digit()) {
        v = (v * 10 + i128::from(d - b'0')).min(i128::from(u64::MAX));
        i += 1;
    }
    if i == start {
        return None;
    }
    let v = if neg { -v } else { v };
    let v = v.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64;
    Some((v as i32, i))
}

/// The scan of a setting or name with `fmt`, if it stored `nfield` fields
/// and read the whole string: C's
/// `nc = 0, ( nfield == astSscanf( setting, fmt, ..., &nc ) ) && ( nc >= len )`,
/// where `nc` is the count of the format's last `%n`.
pub(crate) fn scan_whole(setting: &str, fmt: &str, nfield: i32) -> Option<Scan> {
    let scan = sscanf(setting, fmt);
    (scan.ret == nfield && scan.nc() >= setting.len()).then_some(scan)
}

/// The value of `"<name>= %d %n"`: an integer setting.
pub(crate) fn int_value(setting: &str, name: &str) -> Option<i32> {
    named(setting, name)?;
    scan_whole(setting, &format!("{name}= %d %n"), 1).map(|s| s.int(0))
}

/// The value of `"<name>= %lg %n"`: a floating point setting.
pub(crate) fn double_value(setting: &str, name: &str) -> Option<f64> {
    named(setting, name)?;
    scan_whole(setting, &format!("{name}= %lg %n"), 1).map(|s| s.double(0))
}

/// The value of `"<name>=%n%*[^\n]%n"`: a string setting, everything after
/// the `=`, which is at least one character.
pub(crate) fn string_value<'a>(setting: &'a str, name: &str) -> Option<&'a str> {
    named(setting, name)?;
    let scan = scan_whole(setting, &format!("{name}=%n%*[^\n]%n"), 0)?;
    setting.get(scan.count(0)..)
}

/// The value of `"<name>= %n%*s %n"`: a setting of one word, from its first
/// character to the end of the setting, so with any white space after it.
pub(crate) fn keyword_value<'a>(setting: &'a str, name: &str) -> Option<&'a str> {
    named(setting, name)?;
    let scan = scan_whole(setting, &format!("{name}= %n%*s %n"), 0)?;
    setting.get(scan.count(0)..)
}

/// The axis and value of `"<name>(%d)= %d %n"`: an integer setting of an
/// attribute with an axis index.
pub(crate) fn axis_int_value(setting: &str, name: &str) -> Option<(i32, i32)> {
    indexed(setting, name)?;
    scan_whole(setting, &format!("{name}(%d)= %d %n"), 2).map(|s| (s.int(0), s.int(1)))
}

/// The axis and value of `"<name>(%d)= %lg %n"`.
pub(crate) fn axis_double_value(setting: &str, name: &str) -> Option<(i32, f64)> {
    indexed(setting, name)?;
    scan_whole(setting, &format!("{name}(%d)= %lg %n"), 2).map(|s| (s.int(0), s.double(1)))
}

/// The axis and value of `"<name>(%d)=%n%*[^\n]%n"`.
pub(crate) fn axis_string_value<'a>(setting: &'a str, name: &str) -> Option<(i32, &'a str)> {
    indexed(setting, name)?;
    let scan = scan_whole(setting, &format!("{name}(%d)=%n%*[^\n]%n"), 1)?;
    Some((scan.int(0), setting.get(scan.count(1)..)?))
}

/// The axis of `"<name>(%d)%n"`: the name of an attribute with an axis
/// index, as a get, test or clear is given it.
pub(crate) fn axis_name(attrib: &str, name: &str) -> Option<i32> {
    indexed(attrib, name)?;
    scan_whole(attrib, &format!("{name}(%d)%n"), 1).map(|s| s.int(0))
}

/// A quick test that `text` starts with `name` and then `(`.
fn indexed(text: &str, name: &str) -> Option<()> {
    text.strip_prefix(name)?.starts_with('(').then_some(())
}

/// The value of `"<name>=%n%*s %n"` when the count is at least `min`: a
/// setting whose value starts with a word, from the character after the
/// `=`, so with any white space before the word and anything after it.
// C tests such a count against the length of "<name>=", not against the
// length of the setting, so text after the first word is part of the value.
pub(crate) fn word_value<'a>(setting: &'a str, name: &str, min: usize) -> Option<&'a str> {
    named(setting, name)?;
    let scan = sscanf(setting, &format!("{name}=%n%*s %n"));
    (scan.ret == 0 && scan.count(1) >= min).then(|| &setting[scan.count(0)..])
}

/// The value of `"<name>=%n%*s %n"` read to the end of the setting: one
/// word, from the character after the `=`, so with any white space around
/// it.
pub(crate) fn word_value_whole<'a>(setting: &'a str, name: &str) -> Option<&'a str> {
    named(setting, name)?;
    let scan = scan_whole(setting, &format!("{name}=%n%*s %n"), 0)?;
    setting.get(scan.count(0)..)
}

/// One value through a Mapping of one input and one output.
// C: astTran1 with one point.
pub(crate) fn tran1(
    map: &dyn crate::mappings::Mapping,
    value: f64,
) -> crate::error::AstResult<f64> {
    let points = crate::coords::PointSet::from_rows([[value]]);
    Ok(map.transform(&points, true)?.point(0)[0])
}

thread_local! {
    /// Whether a SpecFrame or TimeFrame is handing an axis attribute given
    /// without an index to the Frame class.
    static IN_SHORTCUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Held while a class of one axis hands a setting such as `top=5` to the
/// Frame class as `top(1)=5`; see `specframe::specframe_set`.
pub(crate) struct ShortcutGuard;

impl ShortcutGuard {
    /// Enter the hand-over: `None` if one is already under way in this
    /// thread, which means the Frame class has handed the setting back.
    pub(crate) fn enter() -> Option<Self> {
        (!IN_SHORTCUT.with(|flag| flag.replace(true))).then_some(ShortcutGuard)
    }
}

impl Drop for ShortcutGuard {
    fn drop(&mut self) {
        IN_SHORTCUT.with(|flag| flag.set(false));
    }
}

/// Whether the setting is of the read-only attribute `name`:
/// C's `MATCH( name )`, `"<name>=%*[^\n]%n"`.
pub(crate) fn is_read_only(setting: &str, name: &str) -> bool {
    string_value(setting, name).is_some()
}

/// A quick test that `setting` starts with `name` and then `=`, which every
/// format above needs before it is worth scanning.
fn named(setting: &str, name: &str) -> Option<()> {
    setting.strip_prefix(name)?.starts_with('=').then_some(())
}

/// C's `Ustrncmp( value, name, ChrLen( value ) ) == 0` in `fitschan.c`:
/// `value`, without trailing white space, is `name` or a leading part of
/// it, ignoring case.
pub(crate) fn abbreviates(value: &str, name: &str) -> bool {
    let v = &value[..chr_len(value)];
    v.len() <= name.len() && name.as_bytes()[..v.len()].eq_ignore_ascii_case(v.as_bytes())
}

// ---------------------------------------------------------------------------
// Formatting values
// ---------------------------------------------------------------------------

/// C's `sprintf( buff, "%d", value )`.
pub(crate) fn fmt_int(value: i32) -> String {
    value.to_string()
}

/// C's `sprintf( buff, "%.*g", AST__DBL_DIG, value )`, the form every
/// floating point attribute is returned in.
pub(crate) fn fmt_double(value: f64) -> String {
    format_double_g_signed(value, crate::serial::native_write::DBL_DIG)
}

/// A year as text with `digits` significant digits at most, without
/// trailing zeros but always with a digit after the decimal point:
/// "2000.0", "1950.5".
// C: frame.c astFmtDecimalYr_. It removes trailing zeros from the whole of
// the "%#.*g" text, so from the exponent of a year too large to print
// without one (c-library-quirks.md).
pub(crate) fn fmt_decimal_yr(year: f64, digits: usize) -> String {
    let digits = digits.min(crate::serial::native_write::DBL_DIG);
    let mut text = sprintf_double(&format!("%#.{digits}g"), year);
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.push('0');
    }
    text
}

/// The text of a value without its graphical escape sequences, which is
/// what a get returns unless escapes are turned on.
// C: plot.c astStripEscapes_. Its buffer holds 50 characters, so a value
// that has escape sequences is also cut to that length (c-library-quirks.md).
#[cfg_attr(not(feature = "attribute-strings"), allow(dead_code))]
pub(crate) fn strip_escapes(text: &str) -> String {
    const BUFF_LEN: usize = 50;
    if crate::escapes::escapes() || !crate::plot::text::has_escapes(text) {
        return text.to_string();
    }
    let t = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(BUFF_LEN);
    let mut a = 0;
    while a < t.len() && out.len() < BUFF_LEN {
        // The scan works on bytes, and every boundary it reports follows an
        // ASCII character or ends the text.
        let rest = std::str::from_utf8(&t[a..]).unwrap_or("");
        if rest.is_empty() {
            break;
        }
        let (found, _, _, nc) = crate::plot::text::find_escape(rest);
        if !found {
            let n = nc.min(BUFF_LEN - out.len());
            out.extend_from_slice(&t[a..a + n]);
        }
        a += nc.max(1);
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A setting that names no attribute of the class.
pub(crate) fn unknown_set(setting: &str, class: &str) -> AstError {
    AstError::new(
        ErrorCode::Badat,
        format!("astSet: The attribute setting \"{setting}\" is invalid for a {class}."),
    )
}

/// A get of a name that is no attribute of the class.
pub(crate) fn unknown_get(name: &str, class: &str) -> AstError {
    AstError::new(
        ErrorCode::Badat,
        format!("astGet: The {class} given does not have an attribute called \"{name}\"."),
    )
}

/// A test of a name that is no attribute of the class.
pub(crate) fn unknown_test(name: &str, class: &str) -> AstError {
    AstError::new(
        ErrorCode::Badat,
        format!("astTest: The attribute name \"{name}\" is invalid for a {class}."),
    )
}

/// A clear of a name that is no attribute of the class.
pub(crate) fn unknown_clear(name: &str, class: &str) -> AstError {
    AstError::new(
        ErrorCode::Badat,
        format!("astClear: The attribute name \"{name}\" is invalid for a {class}."),
    )
}

/// A get of a name that is blank.
#[cfg_attr(not(feature = "attribute-strings"), allow(dead_code))]
pub(crate) fn blank_get(class: &str) -> AstError {
    AstError::new(
        ErrorCode::Badat,
        format!("astGet({class}): A blank attribute name was given."),
    )
}

/// A setting of a read-only attribute.
pub(crate) fn read_only_set(setting: &str, class: &str) -> AstError {
    AstError::new(
        ErrorCode::Nowrt,
        format!("astSet: The setting \"{setting}\" is invalid for a {class}."),
    )
    .with_context("This is a read-only attribute.")
}

/// A clear of a read-only attribute.
pub(crate) fn read_only_clear(name: &str, class: &str) -> AstError {
    AstError::new(
        ErrorCode::Nowrt,
        format!("astClear: Invalid attempt to clear the \"{name}\" value for a {class}."),
    )
    .with_context("This is a read-only attribute.")
}

/// An axis index outside the Frame's axes, as C's `astValidateAxis` reports
/// it for the function `method`.
pub(crate) fn axis_invalid(method: &str, class: &str, axis: i32, naxes: usize) -> AstError {
    AstError::new(
        ErrorCode::Axiin,
        format!(
            "{method}({class}): Axis index ({axis}) invalid - it should be in the range 1 to \
             {naxes}."
        ),
    )
}

/// An operation on an object of a class whose attributes cannot yet be
/// reached by name. The C library has no such error.
pub(crate) fn not_supported(method: &str, class: &str) -> AstError {
    AstError::new(
        ErrorCode::Badat,
        format!("{method}: Attribute strings are not yet supported for a {class}."),
    )
}

/// A set or clear of an attribute the C library has and this one does not
/// store for the class. The C library has no such error.
pub(crate) fn not_stored(method: &str, attr: &str, class: &str) -> AstError {
    AstError::new(
        ErrorCode::Badat,
        format!(
            "{method}: The {attr} attribute of a {class} cannot yet be changed through an \
             attribute string."
        ),
    )
}

/// A get of an attribute the C library has and this one has no value for.
/// The C library has no such error.
pub(crate) fn no_value(attr: &str, class: &str) -> AstError {
    AstError::new(
        ErrorCode::Badat,
        format!("astGet: The {attr} attribute of a {class} has no value in this library."),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_normalised_values_are_not() {
        assert_eq!(
            split_settings(" Label ( 1 ) = Right ascension"),
            ["label(1)= Right ascension"]
        );
        assert_eq!(normalise_name(" Colour ( Axes ) "), "colour(axes)");
    }

    #[test]
    fn commas_split_only_between_assignments() {
        assert_eq!(split_settings("Title=a,b"), ["title=a,b"]);
        assert_eq!(
            split_settings("Title=\"a,b\",Grid=1"),
            ["title=a,b", "grid=1"]
        );
        assert_eq!(
            split_settings("Title=a,b,Grid=1"),
            ["title=a", "b", "grid=1"]
        );
        assert_eq!(split_settings("Title="), ["title= "]);
        assert_eq!(split_names("Title, ,Grid"), ["title", "grid"]);
    }

    #[test]
    fn decimal_years_are_formatted_as_c_formats_them() {
        assert_eq!(fmt_decimal_yr(2000.0, 17), "2000.0");
        assert_eq!(fmt_decimal_yr(1950.5, 17), "1950.5");
    }

    #[test]
    fn set_c_keeps_the_commas_of_a_value() {
        let (list, text) = split_setting_c(" Title ", "a, b=c");
        assert_eq!(list, ["title=a, b=c"]);
        assert_eq!(text, " Title=a\r b=c");
    }

    // The outcomes are those of the C library's sscanf.
    #[test]
    fn sscanf_matches_as_the_c_library_does() {
        let s = sscanf("digits(2)= 7 ", "digits(%d)= %d %n");
        assert_eq!((s.ret, s.int(0), s.int(1), s.count(2)), (2, 2, 7, 13));
        // White space before "%n" takes a tab too.
        assert_eq!(int_value("usedefs=1\t", "usedefs"), Some(1));
        assert_eq!(int_value("usedefs=1.5", "usedefs"), None);
        assert_eq!(int_value("usedefs= ", "usedefs"), None);
        assert_eq!(double_value("tol= 1e-3 ", "tol"), Some(1e-3));
        assert_eq!(string_value("id= a b ", "id"), Some(" a b "));
        assert_eq!(string_value("id=", "id"), None);
        assert_eq!(keyword_value("system=  FK5 ", "system"), Some("FK5 "));
        assert_eq!(keyword_value("system=FK5 x", "system"), None);
        assert_eq!(keyword_value("system= ", "system"), None);
        // The input running out before a conversion is EOF, not zero.
        assert_eq!(sscanf("", "id=%n%*[^\n]%n").ret, -1);
        assert_eq!(sscanf("x", "id=%n%*[^\n]%n").ret, 0);
        let s = sscanf("fred(3)=x y", "%*[^()=]%n(%d)%n=%*[^\n]%n");
        assert_eq!(
            (s.ret, s.count(0), s.int(1), s.count(2), s.count(3)),
            (1, 4, 3, 7, 11)
        );
        let s = sscanf("style(axis1)= 2", "style(%20[^()])= %d %n");
        assert_eq!((s.ret, s.text(0), s.int(1)), (2, "axis1", 2));
    }

    #[test]
    fn escapes_are_stripped_and_the_text_cut_as_c_cuts_it() {
        assert_eq!(strip_escapes("plain text"), "plain text");
        assert_eq!(strip_escapes("x%^50+y%+z %% w%s70+"), "xyz  w");
        let long = format!("{}%+{}", "a".repeat(30), "b".repeat(30));
        assert_eq!(
            strip_escapes(&long),
            format!("{}{}", "a".repeat(30), "b".repeat(20))
        );
    }
}
