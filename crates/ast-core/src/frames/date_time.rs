//! Frame-level date/time parsing helpers.
//!
//! [`read_date_time`] ports `astReadDateTime` (frame.c:8892): it parses a
//! date/time string in any of the formats AST accepts and returns the
//! equivalent Modified Julian Date. All calendar arithmetic is delegated to
//! the vendored PAL via [`crate::pal`]; only the format dispatch is ported.

use crate::pal;

/// Convert a decimal-year epoch (e.g. an `EQUINOX = 1950.0` FITS
/// value) to Modified Julian Date using the IAU 1984 convention.
///
/// Per `astReadDateTime` (frame.c:9031): values strictly less than
/// 1984.0 are interpreted as Besselian (`palEpb2d`); values >= 1984.0
/// are interpreted as Julian (`palEpj2d`).
pub(crate) fn year_to_mjd_iau1984(year: f64) -> f64 {
    if year < 1984.0 {
        pal::epb2d(year)
    } else {
        pal::epj2d(year)
    }
}

/// Parse a date/time string and return the Modified Julian Date.
///
/// Mirrors `astReadDateTime` (frame.c:8892). The recognised formats, tried
/// in priority order, are:
///
/// 1. Besselian epoch, e.g. `"B1950.0"` → `palEpb2d`.
/// 2. Julian epoch, e.g. `"J2000.0"` → `palEpj2d`.
/// 3. Bare decimal years, e.g. `"1976.2"` (IAU 1984 rule: `< 1984.0`
///    Besselian, else Julian).
/// 4. Modified Julian Date, e.g. `"MJD 54321.0"`.
/// 5. Julian Date, e.g. `"JD 2454321.5"` → `jd - 2400000.5`.
/// 6. Gregorian calendar date and (optionally) time, e.g.
///    `"1996-Oct-2 12:13:56.985"`, with numeric or three-letter months,
///    `-`/`/`/`:`/space field separators, an optional `T` date/time
///    delimiter (ISO 8601), and a day fraction expressed either as decimal
///    days or as hours/minutes/seconds.
///
/// Returns `None` (C returns `AST__BAD`) unless the *entire* string is
/// consumed by one format. The date/time is interpreted as a calendar value
/// not tied to any time system (86400 seconds per day, no leap seconds).
// Wired into TimeFrame::unformat in a later step of the iso-date port.
#[allow(dead_code)]
pub(crate) fn read_date_time(value: &str) -> Option<f64> {
    let b = value.as_bytes();
    let len = b.len();

    // Besselian epoch: " %*1[Bb] %lf %n".
    if let Some(epoch) = scan_prefixed_value(b, b"Bb") {
        return Some(pal::epb2d(epoch));
    }

    // Julian epoch: " %*1[Jj] %lf %n".
    if let Some(epoch) = scan_prefixed_value(b, b"Jj") {
        return Some(pal::epj2d(epoch));
    }

    // Decimal years: " %lf %n".
    {
        let mut i = 0;
        skip_ws(b, &mut i);
        if let Some(epoch) = read_f64(b, &mut i) {
            skip_ws(b, &mut i);
            if i >= len {
                return Some(year_to_mjd_iau1984(epoch));
            }
        }
    }

    // Modified Julian Date: " %*1[Mm] %*1[Jj] %*1[Dd] %lf %n".
    if let Some(mjd) = scan_tagged_value(b, &[b"Mm", b"Jj", b"Dd"]) {
        return Some(mjd);
    }

    // Julian Date: " %*1[Jj] %*1[Dd] %lf %n".
    if let Some(jd) = scan_tagged_value(b, &[b"Jj", b"Dd"]) {
        return Some(jd - 2_400_000.5);
    }

    // Gregorian calendar date (and optional time).
    read_gregorian(value)
}

/// Match `" %*1[<set>] %lf %n"` fully against `b`: skip leading whitespace,
/// match exactly one character from `set`, skip whitespace, read a float,
/// skip trailing whitespace, and require the whole buffer be consumed.
fn scan_prefixed_value(b: &[u8], set: &[u8]) -> Option<f64> {
    let mut i = 0;
    skip_ws(b, &mut i);
    if i >= b.len() || !set.contains(&b[i]) {
        return None;
    }
    i += 1;
    skip_ws(b, &mut i);
    let v = read_f64(b, &mut i)?;
    skip_ws(b, &mut i);
    if i >= b.len() { Some(v) } else { None }
}

/// Match `" %*1[s0] %*1[s1] ... %lf %n"`: a sequence of single-character tags
/// (each from its own set, whitespace-separated) followed by a float that
/// consumes the rest of the buffer.
fn scan_tagged_value(b: &[u8], tags: &[&[u8]]) -> Option<f64> {
    let mut i = 0;
    for tag in tags {
        skip_ws(b, &mut i);
        if i >= b.len() || !tag.contains(&b[i]) {
            return None;
        }
        i += 1;
    }
    skip_ws(b, &mut i);
    let v = read_f64(b, &mut i)?;
    skip_ws(b, &mut i);
    if i >= b.len() { Some(v) } else { None }
}

/// Port of the Gregorian-calendar branch of `astReadDateTime`
/// (frame.c:9051-9321).
fn read_gregorian(value: &str) -> Option<f64> {
    let owned;
    let mut b: &[u8] = value.as_bytes();

    // Identify the year and month.
    let (year, cmonth, mut month, ym_nc) = scan_year_month(b)?;

    // Advance past the matched "year sep month sep".
    let mut pos = ym_nc;

    // ISO 8601 uses a "T" delimiter between date and time. If one is present
    // in the remaining text, replace it with a space before parsing.
    if let Some(rel) = b[pos..].iter().position(|&c| c == b'T') {
        let mut v = value.as_bytes().to_vec();
        v[pos + rel] = b' ';
        owned = v;
        b = &owned;
    }

    let rest = &b[pos..];
    let l = rest.len();

    // Day and fraction of day. First test that the remainder has the form of
    // a bare (decimal) day number; if so, read it.
    let mut day: f64 = 0.0;
    let mut hour: i32 = 0;
    let mut minute: i32 = 0;
    let mut sec: f64 = 0.0;

    if form_is_number(rest, 0, l) {
        let mut i = 0;
        skip_ws(rest, &mut i);
        day = read_f64(rest, &mut i)?;
        skip_ws(rest, &mut i);
        if i != l {
            return None;
        }
    } else {
        // Day plus hours:minutes(:seconds). Try with a trailing seconds field
        // first: " %2d%*1[ ] %2d %1[:/-] %2d %1[:/-]%n".
        let mut matched = false;
        if let Some((iday, h, s3, m, s4, nc)) = scan_day_hms_head(rest) {
            if s3 == s4 {
                day = iday as f64;
                hour = h;
                minute = m;
                matched = true;
                pos = nc; // offset within `rest` past "day hh:mm:"
            }
        }

        if matched {
            // Identify the seconds field in the tail.
            let tail = &rest[pos..];
            let tl = tail.len();
            if !form_is_number(tail, 0, tl) {
                return None;
            }
            let mut i = 0;
            skip_ws(tail, &mut i);
            sec = read_f64(tail, &mut i)?;
            skip_ws(tail, &mut i);
            if i != tl {
                return None;
            }
        } else {
            // No seconds field: " %2d%*1[ ] %2d %1[:/-] %2d %n" fully matched.
            let (iday, h, _s3, m, nc) = scan_day_hm(rest)?;
            if nc != l {
                return None;
            }
            day = iday as f64;
            hour = h;
            minute = m;
        }
    }

    // Convert a three-letter month name to a number if necessary.
    if let Some(name) = cmonth {
        const TABLE: &str = "jan feb mar apr may jun jul aug sep oct nov dec";
        let lc = name.to_ascii_lowercase();
        let idx = TABLE.find(&lc)?;
        month = 1 + (idx / 4) as i32;
    }

    // Convert years, months and integral days to an MJD.
    let iday = day as i32;
    let (mut mjd, stat) = pal::cldj(year, month, iday);
    if stat != 0 {
        return None;
    }

    // Add any fractional part of the day.
    mjd += day - iday as f64;

    // Convert hours, minutes and seconds to a fraction of a day.
    let (hms, stat) = pal::dtf2d(hour, minute, sec);
    if stat != 0 {
        return None;
    }
    mjd += hms;

    Some(mjd)
}

/// Match the year/month head of a Gregorian date. Returns
/// `(year, month_name, month_number, chars_consumed)`. Exactly one of the
/// month forms is used: a numeric month leaves `month_name` `None`, a
/// three-letter month leaves `month_number` 0 (resolved by the caller).
///
/// Mirrors the four alternative scans at frame.c:9076-9097, including the
/// requirement that the two field separators be identical.
fn scan_year_month(b: &[u8]) -> Option<(i32, Option<String>, i32, usize)> {
    // " %d %1[:/-] %2d %1[:/-]%n" — numeric month, ":/-" separators
    // (whitespace permitted around both separators).
    if let Some((y, s1, mon, s2, nc)) = scan_ym_numeric(b, b":/-", true) {
        if s1 == s2 {
            return Some((y, None, mon, nc));
        }
    }
    // " %d%1[ ] %2d%1[ ]%n" — numeric month, the separator IS a single space
    // (matched immediately, with no extra whitespace skipped before it).
    if let Some((y, s1, mon, s2, nc)) = scan_ym_numeric(b, b" ", false) {
        if s1 == s2 {
            return Some((y, None, mon, nc));
        }
    }
    // " %d %1[:/-] %3[A-Za-z] %1[:/-]%n" — named month, ":/-" separators.
    if let Some((y, s1, name, s2, nc)) = scan_ym_named(b, b":/-", true) {
        if s1 == s2 {
            return Some((y, Some(name), 0, nc));
        }
    }
    // " %d%1[ ] %3[A-Za-z]%1[ ]%n" — named month, space separator.
    if let Some((y, s1, name, s2, nc)) = scan_ym_named(b, b" ", false) {
        if s1 == s2 {
            return Some((y, Some(name), 0, nc));
        }
    }
    None
}

/// `" %d <sep> %2d <sep>%n"` with a numeric month. When `ws_around_sep` is
/// true the `:/-` separators are matched after skipping whitespace (the
/// `" %1[..]"` directives); when false the separator is a single space
/// matched immediately (the `"%1[ ]"` directive). A leading-space directive
/// always precedes the `%2d` month, so whitespace before the month digits is
/// permitted in both forms. Returns `(year, sep1, month, sep2, chars)`.
fn scan_ym_numeric(
    b: &[u8],
    seps: &[u8],
    ws_around_sep: bool,
) -> Option<(i32, u8, i32, u8, usize)> {
    let mut i = 0;
    skip_ws(b, &mut i);
    let year = read_int(b, &mut i, usize::MAX)?;
    if ws_around_sep {
        skip_ws(b, &mut i);
    }
    let sep1 = match_one(b, &mut i, seps)?;
    skip_ws(b, &mut i);
    let month = read_int(b, &mut i, 2)?;
    if ws_around_sep {
        skip_ws(b, &mut i);
    }
    let sep2 = match_one(b, &mut i, seps)?;
    Some((year as i32, sep1, month as i32, sep2, i))
}

/// `" %d <sep> %3[A-Za-z] <sep>%n"` with a three-letter named month.
fn scan_ym_named(
    b: &[u8],
    seps: &[u8],
    ws_around_sep: bool,
) -> Option<(i32, u8, String, u8, usize)> {
    let mut i = 0;
    skip_ws(b, &mut i);
    let year = read_int(b, &mut i, usize::MAX)?;
    if ws_around_sep {
        skip_ws(b, &mut i);
    }
    let sep1 = match_one(b, &mut i, seps)?;
    skip_ws(b, &mut i);
    let name = read_alpha(b, &mut i, 3)?;
    if ws_around_sep {
        skip_ws(b, &mut i);
    }
    let sep2 = match_one(b, &mut i, seps)?;
    Some((year as i32, sep1, name, sep2, i))
}

/// `" %2d%*1[ ] %2d %1[:/-] %2d %1[:/-]%n"` — day, hour, minute and the two
/// time separators. Returns `(day, hour, sep3, minute, sep4, chars_consumed)`
/// where consumption stops after the minute/second separator.
fn scan_day_hms_head(b: &[u8]) -> Option<(i32, i32, u8, i32, u8, usize)> {
    // The two accepted separator alphabets: ":/-" (with leading-ws skip) and
    // a plain space. The day/hour are always whitespace-separated by a single
    // " " (`%*1[ ]`); minute/second separators come from the chosen alphabet.
    for (mid_seps, allow_ws_before_sep) in [(&b":/-"[..], true), (&b" "[..], false)] {
        let mut i = 0;
        skip_ws(b, &mut i);
        let Some(iday) = read_int(b, &mut i, 2) else {
            continue;
        };
        // "%*1[ ]" — exactly one space between day and hour.
        if i >= b.len() || b[i] != b' ' {
            continue;
        }
        i += 1;
        skip_ws(b, &mut i);
        let Some(hour) = read_int(b, &mut i, 2) else {
            continue;
        };
        if allow_ws_before_sep {
            skip_ws(b, &mut i);
        }
        let Some(s3) = match_one(b, &mut i, mid_seps) else {
            continue;
        };
        skip_ws(b, &mut i);
        let Some(minute) = read_int(b, &mut i, 2) else {
            continue;
        };
        if allow_ws_before_sep {
            skip_ws(b, &mut i);
        }
        let Some(s4) = match_one(b, &mut i, mid_seps) else {
            continue;
        };
        return Some((iday as i32, hour as i32, s3, minute as i32, s4, i));
    }
    None
}

/// `" %2d%*1[ ] %2d <sep> %2d %n"` — day, hour, minute without a trailing
/// seconds separator. Returns `(day, hour, sep3, minute, chars_consumed)`.
fn scan_day_hm(b: &[u8]) -> Option<(i32, i32, u8, i32, usize)> {
    for (mid_seps, allow_ws_before_sep) in [(&b":/-"[..], true), (&b" "[..], false)] {
        let mut i = 0;
        skip_ws(b, &mut i);
        let Some(iday) = read_int(b, &mut i, 2) else {
            continue;
        };
        if i >= b.len() || b[i] != b' ' {
            continue;
        }
        i += 1;
        skip_ws(b, &mut i);
        let Some(hour) = read_int(b, &mut i, 2) else {
            continue;
        };
        if allow_ws_before_sep {
            skip_ws(b, &mut i);
        }
        let Some(s3) = match_one(b, &mut i, mid_seps) else {
            continue;
        };
        skip_ws(b, &mut i);
        let Some(minute) = read_int(b, &mut i, 2) else {
            continue;
        };
        skip_ws(b, &mut i);
        return Some((iday as i32, hour as i32, s3, minute as i32, i));
    }
    None
}

/// Mirror the three form-check scans used before reading a day or seconds
/// field (frame.c:9126-9139): `" %*2[0-9].%*[0-9] %n"`, `" %*2[0-9]. %n"`,
/// `" %*2[0-9] %n"`. Returns true if the slice `b[start..end]` consists of
/// whitespace, then 1-2 digits, an optional `.` with optional trailing
/// digits, then whitespace — i.e. an unsigned number with at most two
/// integer digits and no exponent or sign. The `end` bound is the C `nc==l`
/// requirement.
fn form_is_number(b: &[u8], start: usize, end: usize) -> bool {
    let mut i = start;
    skip_ws(b, &mut i);
    // 1 or 2 integer digits (the C "%*2[0-9]").
    let mut ndig = 0;
    while i < end && ndig < 2 && b[i].is_ascii_digit() {
        i += 1;
        ndig += 1;
    }
    if ndig == 0 {
        return false;
    }
    // Optional "." then optional digits.
    if i < end && b[i] == b'.' {
        i += 1;
        while i < end && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    // Trailing whitespace, then must reach `end`.
    while i < end && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i == end
}

// ---------------------------------------------------------------------------
// Low-level scan primitives (each mirrors a C scanf directive)
// ---------------------------------------------------------------------------

fn skip_ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && b[*i].is_ascii_whitespace() {
        *i += 1;
    }
}

/// Match exactly one byte from `set`, advancing on success.
fn match_one(b: &[u8], i: &mut usize, set: &[u8]) -> Option<u8> {
    if *i < b.len() && set.contains(&b[*i]) {
        let c = b[*i];
        *i += 1;
        Some(c)
    } else {
        None
    }
}

/// Read a signed integer of at most `maxw` digits (C `%d` / `%2d`).
fn read_int(b: &[u8], i: &mut usize, maxw: usize) -> Option<i64> {
    let start = *i;
    let mut j = *i;
    if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
        j += 1;
    }
    let digit_start = j;
    let mut ndig = 0;
    while j < b.len() && ndig < maxw && b[j].is_ascii_digit() {
        j += 1;
        ndig += 1;
    }
    if j == digit_start {
        *i = start;
        return None;
    }
    let s = std::str::from_utf8(&b[start..j]).ok()?;
    let v = s.parse::<i64>().ok()?;
    *i = j;
    Some(v)
}

/// Read up to `maxw` ASCII alphabetic characters (C `%3[A-Za-z]`).
fn read_alpha(b: &[u8], i: &mut usize, maxw: usize) -> Option<String> {
    let start = *i;
    let mut j = *i;
    let mut n = 0;
    while j < b.len() && n < maxw && b[j].is_ascii_alphabetic() {
        j += 1;
        n += 1;
    }
    if j == start {
        return None;
    }
    let s = String::from_utf8(b[start..j].to_vec()).ok()?;
    *i = j;
    Some(s)
}

/// Read a floating-point number (C `%lf`): optional sign, integer and/or
/// fractional digits, optional exponent. Advances `i` past the number.
fn read_f64(b: &[u8], i: &mut usize) -> Option<f64> {
    let start = *i;
    let mut j = *i;
    if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
        j += 1;
    }
    let mut digits = 0;
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
        digits += 1;
    }
    if j < b.len() && b[j] == b'.' {
        j += 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    // Optional exponent.
    if j < b.len() && (b[j] == b'e' || b[j] == b'E') {
        let mut k = j + 1;
        if k < b.len() && (b[k] == b'+' || b[k] == b'-') {
            k += 1;
        }
        let exp_start = k;
        while k < b.len() && b[k].is_ascii_digit() {
            k += 1;
        }
        if k > exp_start {
            j = k;
        }
    }
    let s = std::str::from_utf8(&b[start..j]).ok()?;
    let v = s.parse::<f64>().ok()?;
    *i = j;
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn besselian_round_trip() {
        // FK4 default: B1950.0 -> MJD -> Besselian year should round-trip.
        let mjd = year_to_mjd_iau1984(1950.0);
        assert!((pal::epb(mjd) - 1950.0).abs() < 1.0e-9);
    }

    #[test]
    fn julian_round_trip() {
        // FK5 default: J2000.0 -> MJD -> Julian year should round-trip.
        let mjd = year_to_mjd_iau1984(2000.0);
        assert!((pal::epj(mjd) - 2000.0).abs() < 1.0e-9);
    }

    #[test]
    fn threshold_is_inclusive_for_julian() {
        // Exactly 1984.0 is treated as Julian (per the C >= test).
        assert_eq!(year_to_mjd_iau1984(1984.0), pal::epj2d(1984.0));
        // Just below 1984 is Besselian.
        assert_eq!(year_to_mjd_iau1984(1983.999), pal::epb2d(1983.999));
    }

    // -- read_date_time, oracle values from the built reference libast --

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1.0e-9, "expected {b}, got {a}");
    }

    #[test]
    fn besselian_epoch() {
        approx(read_date_time("B1950.0").unwrap(), 33281.923459049998);
    }

    #[test]
    fn julian_epoch() {
        approx(read_date_time("J2000.0").unwrap(), 51544.5);
    }

    #[test]
    fn decimal_year_besselian_and_julian() {
        // < 1984 Besselian, >= 1984 Julian.
        approx(read_date_time("1976.2").unwrap(), pal::epb2d(1976.2));
        approx(read_date_time("1984.0").unwrap(), pal::epj2d(1984.0));
    }

    #[test]
    fn mjd_and_jd() {
        approx(read_date_time("MJD 54321.0").unwrap(), 54321.0);
        approx(read_date_time("JD 2454321.5").unwrap(), 54321.0);
    }

    #[test]
    fn gregorian_numeric_and_named() {
        approx(read_date_time("1996-10-2").unwrap(), 50358.0);
        approx(read_date_time("1996-Oct-2").unwrap(), 50358.0);
        approx(read_date_time("1996/10/2").unwrap(), 50358.0);
        approx(read_date_time("1996 10 2").unwrap(), 50358.0);
    }

    #[test]
    fn gregorian_decimal_day() {
        approx(
            read_date_time("1996-Oct-2.5001").unwrap(),
            50358.500099999997,
        );
    }

    #[test]
    fn gregorian_with_time() {
        approx(
            read_date_time("1996-Oct-2 12:14:30.52").unwrap(),
            50358.510075462960,
        );
        approx(
            read_date_time("2005-jun-1 12:30").unwrap(),
            53522.520833333336,
        );
        approx(read_date_time("1977-01-01 00:00:00").unwrap(), 43144.0);
        approx(
            read_date_time("2004-12-31 18:00:10.12").unwrap(),
            53370.750117129632,
        );
    }

    #[test]
    fn iso8601_t_delimiter() {
        approx(
            read_date_time("1996-10-2T12:14:30.52").unwrap(),
            50358.510075462960,
        );
    }

    #[test]
    fn trailing_text_rejected() {
        // The whole-string requirement: trailing non-date text fails.
        assert!(read_date_time("B2001.5 lunch time").is_none());
        assert!(read_date_time("2005-jun-1 12:30 lunch time").is_none());
    }

    #[test]
    fn trailing_whitespace_allowed() {
        // A trailing space is consumed by the scanf " %n" tail.
        approx(read_date_time("B2001.5 ").unwrap(), 52091.896696271506);
    }

    #[test]
    fn separator_mismatch_rejected() {
        // The two year/month separators must be identical.
        assert!(read_date_time("1996-10/2").is_none());
    }

    #[test]
    fn invalid_month_name_rejected() {
        assert!(read_date_time("1996-Xyz-2").is_none());
    }

    #[test]
    fn garbage_rejected() {
        assert!(read_date_time("not a date").is_none());
        assert!(read_date_time("").is_none());
    }
}
