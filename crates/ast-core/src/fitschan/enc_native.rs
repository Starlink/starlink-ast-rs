//! Keyword-card NATIVE encoding decoder.
//!
//! Translates `BEGAST_xx` / `ENDAST_xx` / `ISA_xx` / `<NAME>_xx` FITS keyword cards
//! into the line-oriented text format consumed by
//! [`crate::serial::native_read::parse_native`].
//!
//! Mirrors C library `fitschan.c::GetNextData` (line 17703) and
//! `fitschan.c::CreateKeyword` (line 8244).

use super::card::{FITS_CARD_LEN, FITS_NAME_LEN};
use super::card_list::CardList;
use crate::error::{AstError, AstResult, ErrorCode};
use crate::fitschan::card::{FitsCard, FitsValue};

/// Characters used in the 2-character sequence suffix on every NATIVE
/// keyword. Matches `SEQ_CHARS` in `fitschan.c` (line 1377).
pub(crate) const SEQ_CHARS: &str = "_ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// Returns true if `c` is a valid sequence-suffix character.
pub(crate) fn is_seq_char(c: char) -> bool {
    SEQ_CHARS.contains(c)
}

/// Returns true if `s` is exactly two SEQ_CHARS characters.
pub(crate) fn is_two_char_seq_suffix(s: &str) -> bool {
    s.len() == 2 && s.chars().all(is_seq_char)
}

/// Classification of a keyword card under the NATIVE keyword-card encoding.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum KeywordKind {
    /// `BEGAST<xx>` — opens an object block; value is the class name.
    Begin,
    /// `ENDAST<xx>` — closes an object block; value is the class name.
    End,
    /// `ISA<xx>` — inheritance marker; value is the parent class name.
    IsA,
    /// `<NAME>_<xx>` — data attribute; the inner string is the lowercased
    /// base name (without the 2-char suffix) ready to emit as a native key.
    Data(String),
    /// Any other keyword (SIMPLE, NAXIS, COMMENT, CONTINUE, ...) — not part
    /// of NATIVE keyword-card encoding.
    Other,
}

/// Classify a FITS keyword as a NATIVE encoding marker, data card, or other.
pub(crate) fn classify(keyword: &str) -> KeywordKind {
    let len = keyword.len();
    if len < 3 {
        return KeywordKind::Other;
    }
    // The trailing 2 characters must both be SEQ_CHARS to be a NATIVE card.
    let (head, suffix) = keyword.split_at(len - 2);
    if !is_two_char_seq_suffix(suffix) {
        return KeywordKind::Other;
    }
    match head {
        "BEGAST" => KeywordKind::Begin,
        "ENDAST" => KeywordKind::End,
        "ISA" => KeywordKind::IsA,
        _ => {
            if head.is_empty() {
                KeywordKind::Other
            } else {
                // Convert FITS-uppercase head (e.g. "NFRAME") to the
                // capitalize-first form used in native text (e.g. "Nframe"),
                // so that keys match the names written by astWriteInt /
                // astWriteString (e.g. `astWriteInt(this, "Nframe", ...)`).
                let mut chars = head.chars();
                let name = match chars.next() {
                    Some(c) => {
                        let upper: String = c.to_ascii_uppercase().to_string();
                        upper + &chars.as_str().to_ascii_lowercase()
                    }
                    None => String::new(),
                };
                KeywordKind::Data(name)
            }
        }
    }
}

/// Index range of the first complete top-level object in a keyword-card
/// NATIVE header at or after card `start`: from its `BEGAST<xx>` card to the
/// `ENDAST<xx>` card that closes it, inclusive, skipping cards a read
/// consumed. `None` when there is no `BEGAST` or the object is unterminated.
///
/// This is the span C `GetNextData` walks with `skip` 0 from the current
/// card, and therefore the span it marks as used (fitschan.c:17859). Cards
/// before the `BEGAST` are walked with `skip` 1 and left unmarked.
pub(crate) fn native_span(cards: &CardList, start: usize) -> Option<(usize, usize)> {
    let mut first = None;
    let mut depth = 0usize;
    for (i, card) in cards.iter().enumerate().skip(start) {
        if card.is_consumed() {
            continue;
        }
        match classify(&card.keyword) {
            KeywordKind::Begin => {
                if first.is_none() {
                    first = Some(i);
                }
                depth += 1;
            }
            KeywordKind::End => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    return first.map(|f| (f, i));
                }
            }
            _ => {}
        }
    }
    None
}

/// Result of formatting a card value for native-text emission.
#[derive(Debug)]
pub(crate) enum FormattedValue {
    /// The card has a non-empty scalar value; emit `<key> = <text>`.
    Scalar(String),
    /// The card has an empty string value; emit `<key> =` and expect a
    /// child object to follow on subsequent cards (a `BEGAST<xx>` card).
    ObjectMarker,
}

/// Format a `FitsValue` for emission as native-text scalar.
///
/// Returns `None` if the value type is not representable in NATIVE encoding
/// (Comment, Logical, Undefined) — such cards should be ignored by the
/// keyword-card decoder.
pub(crate) fn format_value(value: &FitsValue) -> Option<FormattedValue> {
    match value {
        FitsValue::String(s) => {
            let trimmed = s.trim_end();
            if trimmed.is_empty() {
                Some(FormattedValue::ObjectMarker)
            } else {
                // Reverse the FITS-level pre-quoting (fitschan.c:18010) before
                // applying the channel-text quoting. Embedded double-quotes
                // are escaped by doubling (`""`), matching the convention
                // un-escaped by `parse_native::parse_scalar_value`. Backslash
                // is NOT a metacharacter in this format — it passes through
                // unchanged.
                let unquoted = un_pre_quote(trimmed);
                let escaped = unquoted.replace('"', "\"\"");
                Some(FormattedValue::Scalar(format!("\"{escaped}\"")))
            }
        }
        FitsValue::Int(n) | FitsValue::KInt(n) => Some(FormattedValue::Scalar(n.to_string())),
        FitsValue::Float(f) => Some(FormattedValue::Scalar(format_native_float(*f))),
        // Logical, Comment, Undefined, Continue, ComplexFloat, ComplexInt are not native scalars.
        _ => None,
    }
}

/// Format a float for NATIVE text emission. Mirrors the C library's choice
/// of `%.17g` (full double precision) so a NATIVE round-trip is value-exact.
fn format_native_float(x: f64) -> String {
    if x.is_nan() {
        // Native format does not have a NaN literal; the AST library uses
        // the BAD_VALUE sentinel for missing data and never serialises NaN.
        // Emit a placeholder that downstream parsing will reject loudly.
        return "<bad>".to_string();
    }
    let s = format!("{x:.17e}");
    // Compress to a more compact form when round-tripping is preserved.
    // Match parse_native's tolerance for both `1.5` and `1.5e0` styles.
    s
}

/// Translate a card list with keyword-card NATIVE encoding into the
/// line-oriented text format consumed by [`crate::serial::native_read::parse_native`].
///
/// The card list is walked in order. Cards that are not part of NATIVE
/// keyword-card encoding (SIMPLE, NAXIS, COMMENT, etc.) are silently
/// skipped. Data cards whose value type is not representable as a NATIVE
/// scalar (Logical, Comment, Continue, Undef, complex) are also silently
/// skipped — this matches the C library's value-type gate in
/// `fitschan.c::GetNextData` (lines 17887, 17918, 17943, 17971-17978) where
/// `BEGAST`/`ENDAST`/`ISA` require `AST__STRING` and data cards require
/// `AST__STRING | AST__INT | AST__KINT | AST__FLOAT`.
///
/// Returns an error only if a `BEGAST`/`ENDAST`/`ISA` marker has a
/// non-string value (an outright invalid header).
pub(crate) fn cards_to_native_text(cards: &CardList) -> AstResult<String> {
    let mut out = String::new();
    let card_vec: Vec<&FitsCard> = cards.iter().collect();
    let mut i = 0;
    while i < card_vec.len() {
        let card = card_vec[i];
        let kind = classify(&card.keyword);
        match kind {
            KeywordKind::Begin => {
                let class = expect_class_name(&card.value, &card.keyword)?;
                out.push_str(" Begin ");
                out.push_str(&class);
                out.push('\n');
            }
            KeywordKind::End => {
                let class = expect_class_name(&card.value, &card.keyword)?;
                out.push_str(" End ");
                out.push_str(&class);
                out.push('\n');
            }
            KeywordKind::IsA => {
                let class = expect_class_name(&card.value, &card.keyword)?;
                out.push_str("    IsA ");
                out.push_str(&class);
                out.push('\n');
            }
            KeywordKind::Data(name) => {
                let (formatted, advance) =
                    format_value_with_continues(&card.value, &card_vec[i + 1..])?;
                match formatted {
                    Some(FormattedValue::Scalar(text)) => {
                        out.push_str("    ");
                        out.push_str(&name);
                        out.push_str(" = ");
                        out.push_str(&text);
                        out.push('\n');
                    }
                    Some(FormattedValue::ObjectMarker) => {
                        out.push_str("    ");
                        out.push_str(&name);
                        out.push_str(" =\n");
                    }
                    None => {
                        // Non-scalar Data card; skip silently. C library gates
                        // Data emission on AST__STRING|AST__INT|AST__KINT|AST__FLOAT
                        // (fitschan.c::GetNextData lines 17971-17978).
                    }
                }
                i += advance;
            }
            KeywordKind::Other => {}
        }
        i += 1;
    }
    Ok(out)
}

fn expect_class_name(value: &FitsValue, keyword: &str) -> AstResult<String> {
    match value {
        FitsValue::String(s) => Ok(un_pre_quote(s.trim_end()).to_string()),
        _ => Err(AstError::new(
            ErrorCode::Rderr,
            format!("NATIVE keyword '{keyword}' must have string value, got {value:?}"),
        )),
    }
}

/// Reverse `pre_quote`: strip one outer pair of double quotes when the first
/// and last characters are both `"`. Mirrors C `UnPreQuote`
/// (fitschan.c:34680-34751). Interior double quotes are untouched — the ends
/// of the string are known from the FITS quoting, so only the first and last
/// characters are significant.
fn un_pre_quote(value: &str) -> &str {
    let b = value.as_bytes();
    if b.len() >= 2 && b[0] == b'"' && b[b.len() - 1] == b'"' {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

/// Format a card value, consuming any trailing CONTINUE cards if the value
/// is a string ending in `&`. Returns the formatted value plus the number
/// of additional CONTINUE cards consumed.
///
/// Mirrors the C library logic at `fitschan.c:17979-18024`.
fn format_value_with_continues(
    value: &FitsValue,
    rest: &[&FitsCard],
) -> AstResult<(Option<FormattedValue>, usize)> {
    let FitsValue::String(s) = value else {
        return Ok((format_value(value), 0));
    };
    let trimmed = s.trim_end();
    if trimmed.is_empty() {
        return Ok((Some(FormattedValue::ObjectMarker), 0));
    }
    if !trimmed.ends_with('&') {
        return Ok((format_value(value), 0));
    }
    // C stores the first card of a continued value raw (fitschan.c:18011) and
    // overwrites its trailing ampersand with the next segment; only the
    // CONTINUE segments are un-quoted (fitschan.c:18032).
    let mut joined = trimmed.strip_suffix('&').unwrap_or(trimmed).to_string();
    let mut consumed = 0usize;
    for next in rest {
        if next.keyword != "CONTINUE" {
            break;
        }
        consumed += 1;
        // CONTINUE cards parse with value = FitsValue::Continue(text).
        let seg = match &next.value {
            FitsValue::Continue(s) => s,
            FitsValue::String(s) => s,
            _ => {
                return Err(AstError::new(
                    ErrorCode::Rderr,
                    "CONTINUE card without string value",
                ));
            }
        };
        let seg_trimmed = seg.trim_end();
        // The ampersand test comes first, on the raw value, before
        // un-quoting — exactly C's ordering at fitschan.c:18029-18032.
        let next_continues = seg_trimmed.ends_with('&');
        let seg_text = seg_trimmed.strip_suffix('&').unwrap_or(seg_trimmed);
        joined.push_str(un_pre_quote(seg_text));
        if !next_continues {
            break;
        }
    }
    let escaped = joined.replace('"', "\"\"");
    Ok((
        Some(FormattedValue::Scalar(format!("\"{escaped}\""))),
        consumed,
    ))
}

// ===========================================================================
// Keyword-card NATIVE encoder (Node tree -> BEGAST/ENDAST/ISA/<NAME>_xx cards)
// ===========================================================================
//
// This is the inverse of the decoder above: it walks a serialised [`Node`]
// tree and emits the keyword-card NATIVE representation used by the C library
// as the default storage form (BEGAST_xx, <NAME>_xx, ISA_xx, ENDAST_xx).
//
// Mirrors C `fitschan.c`'s Channel-method overrides:
//   Write       (40206) — top-level banners + recursion
//   WriteBegin  (40432) — BEGAST keyword
//   WriteInt    (40837) / WriteDouble (40518) — scalar value cards
//   WriteString (41253) — string value cards, with CONTINUE splitting
//   WriteObject (41104) — object marker keyword + recursion
//   WriteIsA    (40945) — ISA keyword (gated on items_written / Full)
//   WriteEnd    (40642) — ENDAST keyword
//   CreateKeyword (8209) — unique <BASE><xx> sequence suffixing
//
// The values themselves are stored as typed [`FitsValue`]s; the existing card
// formatter (`FitsCard::format_with_digits_and_rounding`) reproduces C's
// `EncodeFloat`/`EncodeValue` column layout when the header is rendered.

use crate::serial::node::{Node, NodeValue};

/// Maximum characters in a FITS keyword (C `FITSNAMLEN`).
const FITS_NAM_LEN: usize = 8;

/// Width available for the value/quoting area of a NATIVE string card,
/// matching `AST__FITSCHAN_FITSCARDLEN - FITSNAMLEN - 6` in C `WriteString`.
const STRING_VALUE_WIDTH: usize = 80 - FITS_NAM_LEN - 6;

/// Assigns unique sequence-suffixed FITS keywords, mirroring C `CreateKeyword`
/// (fitschan.c:8209). Each distinct uppercased base name keeps its own
/// monotonic sequence counter, which the FitsChan keeps from one write to
/// the next (C's `keyseq`); the counter is encoded into two `SEQ_CHARS`
/// characters appended to the (truncated) base, and a keyword some card
/// already has is passed over.
struct KeywordSequencer<'a> {
    /// Per-base sequence counter (keyed by the uppercased, truncated base).
    seq: &'a mut std::collections::HashMap<String, usize>,
    /// Whether a card already has a keyword (C's `HasCard`).
    taken: &'a dyn Fn(&str) -> bool,
    /// The keywords this write has created so far.
    created: std::collections::HashSet<String>,
}

impl KeywordSequencer<'_> {
    /// Create the next unique keyword based on `name`, mirroring
    /// `CreateKeyword`. The base is uppercased and truncated to
    /// `FITSNAMLEN - 2` characters to leave room for the 2-char suffix; the
    /// sequence counter for that base is incremented and encoded.
    fn create(&mut self, name: &str) -> String {
        let seq_chars: Vec<char> = SEQ_CHARS.chars().collect();
        let nseq = seq_chars.len();

        // Uppercase and truncate the base to FITSNAMLEN-2 characters.
        let base: String = name
            .chars()
            .take(FITS_NAM_LEN - 2)
            .map(|c| c.to_ascii_uppercase())
            .collect();

        // Look up the last sequence number used for this base; C starts a new
        // base at 0 and pre-increments, so the first emitted suffix is seq=1.
        // Sequence numbers whose keyword a card already has are passed over,
        // and the last is re-used once the two characters run out.
        let entry = self.seq.entry(base.clone()).or_insert(0);
        let limit_seq = nseq * nseq - 1;
        loop {
            let limit = *entry >= limit_seq;
            if !limit {
                *entry += 1;
            }
            let seq = *entry;
            // Encode the sequence number into two SEQ_CHARS characters.
            let mut keyword = base.clone();
            keyword.push(seq_chars[seq / nseq]);
            keyword.push(seq_chars[seq % nseq]);
            if limit || !((self.taken)(&keyword) || self.created.contains(&keyword)) {
                self.created.insert(keyword.clone());
                return keyword;
            }
        }
    }
}

/// Build a centred banner comment string, mirroring C `MakeBanner`
/// (fitschan.c:20776). The text is the concatenation of `prefix`, `middle`,
/// and `suffix`, centred in the 72-column comment field (`FITSCARDLEN -
/// FITSNAMLEN`) and flanked by the `AST` token at each end.
pub(crate) fn make_banner(prefix: &str, middle: &str, suffix: &str) -> String {
    const TOKEN: &str = "AST";
    let mxlen = 80 - FITS_NAM_LEN; // 72
    let ltok = TOKEN.len();
    let text_len = prefix.chars().count() + middle.chars().count() + suffix.chars().count();

    // Column (1-based) where the text starts, centring it with the token plus
    // padding on each side. Mirrors C's arithmetic exactly.
    let start_calc = ltok as isize
        + 2
        + (mxlen as isize - ltok as isize - 1 - text_len as isize - 1 - ltok as isize) / 2;
    let min_start = ltok as isize + 2;
    let start = start_calc.max(min_start) as usize;

    let mut out: Vec<char> = Vec::with_capacity(mxlen);

    // Leading token.
    for c in TOKEN.chars() {
        if out.len() >= mxlen {
            break;
        }
        out.push(c);
    }
    // Pad to the start column (start - 1 characters total before text).
    while out.len() < start - 1 {
        out.push(' ');
    }
    // Prefix, middle, suffix, each truncated so it never reaches the trailing
    // token area (mxlen - ltok - 1).
    let text_limit = mxlen - ltok - 1;
    for s in [prefix, middle, suffix] {
        for c in s.chars() {
            if out.len() >= text_limit {
                break;
            }
            out.push(c);
        }
    }
    // Pad the end of the text up to (mxlen - ltok).
    while out.len() < mxlen - ltok {
        out.push(' ');
    }
    // Trailing token.
    for c in TOKEN.chars() {
        if out.len() >= mxlen {
            break;
        }
        out.push(c);
    }

    out.into_iter().collect()
}

/// Render a [`Node`] tree as keyword-card NATIVE cards, appending them to
/// `cards`. The cards reproduce the C library's default NATIVE storage form;
/// the leading/trailing comment banners are added by the caller.
///
/// `with_comments` controls whether per-card comment text is emitted (C
/// `Comment` attribute, default true). Unset (`# default`) items are dropped,
/// matching C `Use()` at the default `Full = 0`.
///
/// `keyseq` holds the last sequence number used for each keyword base, kept
/// by the FitsChan from one write to the next, and `taken` says whether a
/// card in the FitsChan already has a keyword.
pub(crate) fn write_keyword_native(
    cards: &mut Vec<FitsCard>,
    node: &Node,
    with_comments: bool,
    keyseq: &mut std::collections::HashMap<String, usize>,
    taken: &dyn Fn(&str) -> bool,
) {
    let mut seq = KeywordSequencer {
        seq: keyseq,
        taken,
        created: std::collections::HashSet::new(),
    };
    write_object(cards, node, &mut seq, with_comments);
}

/// Emit one object as a BEGAST … ENDAST block (recursing for nested objects).
/// Returns nothing; cards are pushed onto `cards`.
fn write_object(
    cards: &mut Vec<FitsCard>,
    node: &Node,
    seq: &mut KeywordSequencer<'_>,
    with_comments: bool,
) {
    // BEGAST keyword, value = class name, comment = the class comment.
    let begast = seq.create("BEGAST");
    push_string_card(
        cards,
        &begast,
        &node.class,
        node.class_comment.as_deref(),
        with_comments,
    );

    // `items_written` gates ISA emission, mirroring C WriteIsA: an ISA marker
    // is written only if at least one item has been written since the last
    // Begin/IsA (at the default Full = 0).
    let mut items_written = 0usize;

    for item in &node.items {
        // IsA marker.
        if item.key == "IsA" {
            if items_written == 0 {
                // C suppresses the ISA item when nothing has been written
                // since the previous Begin/IsA.
                continue;
            }
            if let NodeValue::String(ref class) = item.value {
                let isa = seq.create("ISA");
                push_string_card(cards, &isa, class, item.comment.as_deref(), with_comments);
            }
            items_written = 0;
            continue;
        }

        match &item.value {
            // Unset (commented default): dropped at the default Full = 0,
            // matching C Use() (helpful && full > -1 is false for full = 0
            // unless the value is helpful; the wcsconv fixtures show none).
            NodeValue::Unset(_) => continue,

            // Nested object: emit a blank-string marker keyword, then recurse.
            NodeValue::Object(child) => {
                let key = seq.create(&item.key);
                push_string_card(cards, &key, "", item.comment.as_deref(), with_comments);
                write_object(cards, child, seq, with_comments);
                items_written += 1;
            }

            // Integer scalar.
            NodeValue::Int(v) => {
                let key = seq.create(&item.key);
                push_card(
                    cards,
                    key,
                    FitsValue::Int(*v),
                    item.comment.as_deref(),
                    with_comments,
                );
                items_written += 1;
            }

            // Double scalar.
            NodeValue::Double(v) => {
                let key = seq.create(&item.key);
                push_card(
                    cards,
                    key,
                    FitsValue::Float(*v),
                    item.comment.as_deref(),
                    with_comments,
                );
                items_written += 1;
            }

            // String scalar, with CONTINUE splitting for long values.
            NodeValue::String(s) => {
                let key = seq.create(&item.key);
                write_string_value(cards, &key, s, item.comment.as_deref(), with_comments);
                items_written += 1;
            }
        }
    }

    // ENDAST keyword.
    let endast = seq.create("ENDAST");
    push_string_card(
        cards,
        &endast,
        &node.class,
        if with_comments {
            Some("End of object definition")
        } else {
            None
        },
        with_comments,
    );
}

/// Write a string attribute, splitting into CONTINUE cards when the value is
/// too long to fit one card. Mirrors C `WriteString` (fitschan.c:41253).
fn write_string_value(
    cards: &mut Vec<FitsCard>,
    keyword: &str,
    value: &str,
    comment: Option<&str>,
    with_comments: bool,
) {
    // Fast path: short strings need no splitting.
    let chars: Vec<char> = value.chars().collect();
    let mut start = 0usize;
    let mut first = true;

    while start < chars.len() || first {
        // Available columns for this sub-string. The first card reserves room
        // for the comment (length + " / "); single quotes cost two columns.
        let mut nc: isize = STRING_VALUE_WIDTH as isize;
        if first && with_comments {
            if let Some(c) = comment {
                if !c.is_empty() {
                    nc -= chr_len(c) as isize + 3;
                }
            }
        }

        // Copy characters until the card is full.
        let mut buf = String::new();
        while start < chars.len() && nc > 0 {
            let cc = chars[start];
            start += 1;
            buf.push(cc);
            nc -= if cc == '\'' { 2 } else { 1 };
        }

        // If a trailing single quote overflowed, push it to the next card.
        if nc < 0 {
            start -= 1;
            buf.pop();
        }

        // If the value is not exhausted, append an ampersand (making room by
        // pushing the last character into the next sub-string).
        if start < chars.len() {
            start -= 1;
            buf.pop();
            buf.push('&');
        }

        let quoted = pre_quote(&buf);

        if first {
            push_string_card_raw(cards, keyword.to_string(), quoted, comment, with_comments);
        } else {
            // CONTINUE card: stored as a Continue value (no '=' sign).
            cards.push(FitsCard {
                keyword: "CONTINUE".to_string(),
                value: FitsValue::Continue(quoted),
                comment: None,
                used: false,
                flags: crate::fitschan::card::CardFlags::default(),
            });
        }
        first = false;

        if start >= chars.len() {
            break;
        }
    }
}

/// Length of a string ignoring trailing white space (C `ChrLen`).
fn chr_len(s: &str) -> usize {
    s.trim_end().chars().count()
}

/// Pre-quote a value with surrounding double quotes when it has trailing
/// white space or double quotes at either end, truncating it to what a FITS
/// card can hold. Mirrors C `PreQuote` (fitschan.c:24989-25111).
///
/// Embedded single quotes are not doubled here — the card formatter doubles
/// them when rendering the card — but they are *counted* against the length
/// limit, because that doubling is what makes them cost two columns.
/// Interior double quotes are left alone: FITS delimits the value, so only
/// the first and last characters matter when un-quoting.
fn pre_quote(value: &str) -> String {
    // C: AST__FITSCHAN_FITSCARDLEN - FITSNAMLEN - 4 == 80 - 8 - 4.
    const MAX: usize = FITS_CARD_LEN - FITS_NAME_LEN - 4;

    let chars: Vec<char> = value.chars().collect();
    let starts_dquote = chars.first() == Some(&'"');

    // A zero-length value keeps the initial 2, so it is emitted as `""`.
    let mut dquotes = 2;
    let mut nc = 0;
    let mut sq = 0;

    for (i, &c) in chars.iter().enumerate() {
        if c == '\'' {
            sq += 1;
        }
        let dq = if c.is_whitespace() || (starts_dquote && c == '"') {
            2
        } else {
            0
        };
        if nc + 1 + dq + sq > MAX {
            break;
        }
        nc = i + 1;
        dquotes = dq;
    }

    let body: String = chars[..nc].iter().collect();
    if dquotes != 0 {
        format!("\"{body}\"")
    } else {
        body
    }
}

/// Push a string-valued card whose value is pre-quoted/escaped by the card
/// formatter. `value` is the raw (un-doubled) string.
fn push_string_card(
    cards: &mut Vec<FitsCard>,
    keyword: &str,
    value: &str,
    comment: Option<&str>,
    with_comments: bool,
) {
    push_string_card_raw(
        cards,
        keyword.to_string(),
        value.to_string(),
        comment,
        with_comments,
    );
}

fn push_string_card_raw(
    cards: &mut Vec<FitsCard>,
    keyword: String,
    value: String,
    comment: Option<&str>,
    with_comments: bool,
) {
    cards.push(FitsCard {
        keyword,
        value: FitsValue::String(value),
        comment: if with_comments {
            comment.map(str::to_string)
        } else {
            None
        },
        used: false,
        flags: crate::fitschan::card::CardFlags::default(),
    });
}

fn push_card(
    cards: &mut Vec<FitsCard>,
    keyword: String,
    value: FitsValue,
    comment: Option<&str>,
    with_comments: bool,
) {
    cards.push(FitsCard {
        keyword,
        value,
        comment: if with_comments {
            comment.map(str::to_string)
        } else {
            None
        },
        used: false,
        flags: crate::fitschan::card::CardFlags::default(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fitschan::FitsChan;

    fn cards_from(text: &str) -> CardList {
        let fc = FitsChan::from_header(text).expect("from_header");
        fc.cards_for_test().clone()
    }

    #[test]
    fn detects_keyword_native_when_begast_present() {
        let header = "\
BEGAST_A= 'FrameSet'           / Set of inter-related coordinate systems
NFRAME_A=                    2 / Number of Frames in FrameSet
ENDAST_A= 'FrameSet'           / End of object definition
END
";
        let cards = cards_from(header);
        assert!(native_span(&cards, 0).is_some());
    }

    #[test]
    fn rejects_pure_wcs_header() {
        let header = "\
SIMPLE  =                    T / file conforms to FITS standard
NAXIS   =                    2 / number of data axes
CTYPE1  = 'RA---TAN'           / coord type
CRPIX1  =                100.0 / reference pixel
CRVAL1  =                  0.0 / reference value
END
";
        let cards = cards_from(header);
        assert!(native_span(&cards, 0).is_none());
    }

    #[test]
    fn seq_chars_recognises_underscore_and_letters() {
        assert!(is_seq_char('_'));
        assert!(is_seq_char('A'));
        assert!(is_seq_char('Z'));
        assert!(!is_seq_char('a'));
        assert!(!is_seq_char('1'));
        assert!(!is_seq_char('0'));
    }

    #[test]
    fn two_char_seq_suffix_examples() {
        assert!(is_two_char_seq_suffix("_A"));
        assert!(is_two_char_seq_suffix("AB"));
        assert!(is_two_char_seq_suffix("__"));
        assert!(is_two_char_seq_suffix("ZZ"));
        assert!(!is_two_char_seq_suffix("A"));
        assert!(!is_two_char_seq_suffix("ABC"));
        assert!(!is_two_char_seq_suffix("a_"));
        assert!(!is_two_char_seq_suffix("0_"));
    }

    #[test]
    fn classify_recognises_begast() {
        assert_eq!(classify("BEGAST_A"), KeywordKind::Begin);
        assert_eq!(classify("BEGAST_Z"), KeywordKind::Begin);
        assert_eq!(classify("BEGASTAA"), KeywordKind::Begin);
    }

    #[test]
    fn classify_recognises_endast_and_isa() {
        assert_eq!(classify("ENDAST_A"), KeywordKind::End);
        assert_eq!(classify("ISA_A"), KeywordKind::IsA);
        assert_eq!(classify("ISAAB"), KeywordKind::IsA);
    }

    #[test]
    fn classify_recognises_data_card() {
        // Keys are converted to capitalize-first (e.g. NFRAME → Nframe) so
        // they match the mixed-case names written by astWriteInt etc. in
        // native text (e.g. `Nframe = 2`).
        assert_eq!(
            classify("NFRAME_A"),
            KeywordKind::Data("Nframe".to_string())
        );
        assert_eq!(
            classify("DOMAIN_B"),
            KeywordKind::Data("Domain".to_string())
        );
        assert_eq!(
            classify("FRAMEA_A"),
            KeywordKind::Data("Framea".to_string())
        );
    }

    #[test]
    fn classify_rejects_non_native_keywords() {
        // Syntactically invalid SEQ_CHARS suffix → always Other.
        assert_eq!(classify("CTYPE1"), KeywordKind::Other); // suffix is "E1", '1' not in SEQ_CHARS
        assert_eq!(classify(""), KeywordKind::Other);
        assert_eq!(classify("AB"), KeywordKind::Other);
        // Standard FITS keywords whose last two chars happen to pass the SEQ_CHARS
        // check are returned as Data — rejection for "wrong value type" is the
        // caller's responsibility, not the keyword-name classifier.
        // C library rejects SIMPLE/COMMENT/CONTINUE via AST card-type check
        // (AST__LOGICAL / AST__COMMENT), which is not visible here.
        assert_eq!(classify("SIMPLE"), KeywordKind::Data("Simp".to_string()));
        assert_eq!(classify("NAXIS"), KeywordKind::Data("Nax".to_string()));
        assert_eq!(classify("COMMENT"), KeywordKind::Data("Comme".to_string()));
        assert_eq!(
            classify("CONTINUE"),
            KeywordKind::Data("Contin".to_string())
        );
    }

    #[test]
    fn format_value_string_non_empty() {
        let v = FitsValue::String("FrameSet".to_string());
        match format_value(&v) {
            Some(FormattedValue::Scalar(s)) => assert_eq!(s, "\"FrameSet\""),
            other => panic!("expected Scalar, got {other:?}"),
        }
    }

    #[test]
    fn format_value_string_empty_is_object_marker() {
        let v = FitsValue::String("        ".to_string());
        assert!(matches!(
            format_value(&v),
            Some(FormattedValue::ObjectMarker)
        ));
    }

    #[test]
    fn format_value_int() {
        let v = FitsValue::Int(42);
        match format_value(&v) {
            Some(FormattedValue::Scalar(s)) => assert_eq!(s, "42"),
            other => panic!("expected Scalar, got {other:?}"),
        }
    }

    #[test]
    fn format_value_float_basic() {
        let v = FitsValue::Float(1.5);
        match format_value(&v) {
            Some(FormattedValue::Scalar(s)) => {
                // Just confirm it's parseable by Rust as 1.5.
                let parsed: f64 = s.parse().expect("parses");
                assert_eq!(parsed, 1.5);
            }
            other => panic!("expected Scalar, got {other:?}"),
        }
    }

    #[test]
    fn format_value_string_with_embedded_quotes_roundtrips() {
        // The encoder doubles literal `"` so the parser's `""` -> `"` un-escape
        // recovers the original value.
        let v = FitsValue::String(r#"say "hi""#.to_string());
        let Some(FormattedValue::Scalar(s)) = format_value(&v) else {
            panic!("expected Scalar");
        };
        // Strip outer quotes, run the parser's inverse transform.
        assert!(s.starts_with('"') && s.ends_with('"'));
        let inner = &s[1..s.len() - 1];
        let decoded = inner.replace("\"\"", "\"");
        assert_eq!(decoded, r#"say "hi""#);
    }

    #[test]
    fn format_value_string_with_backslash_passes_through() {
        // Backslash is not a metacharacter — it must round-trip as-is.
        let v = FitsValue::String(r"a\b".to_string());
        let Some(FormattedValue::Scalar(s)) = format_value(&v) else {
            panic!("expected Scalar");
        };
        assert_eq!(s, r#""a\b""#);
    }

    #[test]
    fn un_pre_quote_strips_one_outer_pair() {
        // C UnPreQuote (fitschan.c:34733-34745).
        assert_eq!(un_pre_quote("\",1)     \""), ",1)     ");
        assert_eq!(un_pre_quote("plain"), "plain");
        // Interior double quotes are left alone.
        assert_eq!(un_pre_quote("\"a\"b\""), "a\"b");
        // A single character is never a pair.
        assert_eq!(un_pre_quote("\""), "\"");
        assert_eq!(un_pre_quote(""), "");
    }

    #[test]
    fn continued_value_un_pre_quotes_each_continue_segment() {
        let cards = cards_from(concat!(
            "BEGAST_A= 'Frame   '\n",
            "TITLE_A = 'first pixel at (1&'\n",
            "CONTINUE  '\",1)     \"'\n",
            "ENDAST_A= 'Frame   '\n",
        ));
        let text = cards_to_native_text(&cards).expect("translates");
        assert!(
            text.contains("Title = \"first pixel at (1,1)     \""),
            "trailing spaces must survive un-quoting; got:\n{text}"
        );
    }

    #[test]
    fn cards_to_native_text_round_trips_a_minimal_frameset() {
        let header = "\
BEGAST_A= 'FrameSet'           / Set of inter-related coordinate systems
NFRAME_A=                    2 / Number of Frames in FrameSet
ENDAST_A= 'FrameSet'           / End of object definition
END
";
        let cards = cards_from(header);
        let text = cards_to_native_text(&cards).expect("translates");
        assert!(text.contains("Begin FrameSet"));
        assert!(text.contains("Nframe = 2"));
        assert!(text.contains("End FrameSet"));
    }

    #[test]
    fn cards_to_native_text_emits_object_marker_for_empty_string() {
        let header = "\
BEGAST_A= 'FrameSet'
FRAMEA_A= '        '           / First component Frame
BEGAST_B= 'Frame   '
ENDAST_A= 'Frame   '
ENDAST_B= 'FrameSet'
END
";
        let cards = cards_from(header);
        let text = cards_to_native_text(&cards).expect("translates");
        // The empty-string FRAMEA_A must produce a child-object marker.
        assert!(text.contains("Framea =\n"));
        // The next emitted line is `Begin Frame`.
        let pos_marker = text.find("Framea =\n").expect("marker present");
        let after = &text[pos_marker + "framea =\n".len()..];
        assert!(
            after.trim_start().starts_with("Begin Frame"),
            "expected child Begin after object marker, got: {after}"
        );
    }

    #[test]
    fn cards_to_native_text_joins_continue_cards() {
        let header = "\
BEGAST_A= 'Frame   '
TITLE_A = 'first part &'        / first part
CONTINUE  '   second part'      / continued
ENDAST_A= 'Frame   '
END
";
        let cards = cards_from(header);
        let text = cards_to_native_text(&cards).expect("translates");
        assert!(
            text.contains("Title = \"first part    second part\""),
            "expected joined string, got: {text}"
        );
    }

    #[test]
    fn cards_to_native_text_skips_non_scalar_data_cards() {
        // A card whose name happens to end in two SEQ_CHARS but whose value
        // is logical (e.g. SIMPLE = T in a header that also has BEGAST
        // somewhere) must be silently skipped, not error.
        let header = "\
SIMPLE  =                    T / not a NATIVE data card despite name suffix
BEGAST_A= 'FrameSet'
NFRAME_A=                    2
ENDAST_A= 'FrameSet'
END
";
        let cards = cards_from(header);
        let text = cards_to_native_text(&cards).expect("translates");
        // SIMPLE -> Data("Simp") classification, but value is Logical -> skip.
        assert!(!text.contains("Simp"));
        // FrameSet body still emitted.
        assert!(text.contains("Begin FrameSet"));
        assert!(text.contains("Nframe = 2"));
    }

    // -----------------------------------------------------------------
    // Keyword-card writer (write_keyword_native / CreateKeyword / banner)
    // -----------------------------------------------------------------

    #[test]
    fn create_keyword_sequences_per_base() {
        let mut keyseq = std::collections::HashMap::new();
        let mut seq = KeywordSequencer {
            seq: &mut keyseq,
            taken: &|_| false,
            created: std::collections::HashSet::new(),
        };
        // First use of each base starts at suffix `_A` (seq = 1).
        assert_eq!(seq.create("BEGAST"), "BEGAST_A");
        assert_eq!(seq.create("BEGAST"), "BEGAST_B");
        assert_eq!(seq.create("BEGAST"), "BEGAST_C");
        // A different base keeps its own counter.
        assert_eq!(seq.create("ISA"), "ISA_A");
        assert_eq!(seq.create("BEGAST"), "BEGAST_D");
        // Long names are truncated to FITSNAMLEN - 2 = 6 characters.
        assert_eq!(seq.create("MatrixMap"), "MATRIX_A");
    }

    #[test]
    fn create_keyword_wraps_after_first_block() {
        let mut keyseq = std::collections::HashMap::new();
        let mut seq = KeywordSequencer {
            seq: &mut keyseq,
            taken: &|_| false,
            created: std::collections::HashSet::new(),
        };
        // 26 second-characters after '_': seq 1..=26 map to _A.._Z, then the
        // high character advances (seq 27 -> "A_").
        let mut last = String::new();
        for _ in 0..27 {
            last = seq.create("X");
        }
        assert_eq!(last, "XA_");
    }

    #[test]
    fn make_banner_centres_text_with_ast_tokens() {
        let b = make_banner("Beginning of AST data for ", "FrameSet", " object");
        // 72 columns total, AST token at each end.
        assert_eq!(b.chars().count(), 72);
        assert!(b.starts_with("AST"));
        assert!(b.ends_with("AST"));
        assert!(b.contains("Beginning of AST data for FrameSet object"));
    }

    /// Write a small object tree to keyword cards and read it straight back
    /// through the decoder, confirming the structure round-trips.
    #[test]
    fn write_keyword_native_round_trips() {
        use crate::serial::node::Node;

        let mut inner = Node::new("UnitMap");
        inner.class_comment = Some("Unit (null) Mapping".to_string());
        inner.push_int("Nin", 2, Some("Number of input coordinates".to_string()));
        inner.push_isa(
            "Mapping",
            Some("Mapping between coordinate systems".to_string()),
        );

        let mut outer = Node::new("CmpMap");
        outer.class_comment = Some("Compound Mapping".to_string());
        outer.push_int("Nin", 2, Some("Number of input coordinates".to_string()));
        outer.push_double("Zoom", 5.5, Some("Zoom factor".to_string()));
        outer.push_isa(
            "Mapping",
            Some("Mapping between coordinate systems".to_string()),
        );
        outer.push_object("MapA", inner, Some("First component Mapping".to_string()));

        let mut block: Vec<FitsCard> = Vec::new();
        write_keyword_native(
            &mut block,
            &outer,
            true,
            &mut std::collections::HashMap::new(),
            &|_| false,
        );

        // The first card opens the outer object; the last closes it.
        assert_eq!(block.first().unwrap().keyword, "BEGAST_A");
        match &block.first().unwrap().value {
            FitsValue::String(s) => assert_eq!(s, "CmpMap"),
            other => panic!("expected string value, got {other:?}"),
        }
        assert!(block.last().unwrap().keyword.starts_with("ENDAST"));

        // Round-trip through the decoder: load the cards back into native text.
        let mut cards = CardList::new();
        for c in block {
            cards.push(c);
        }
        let text = cards_to_native_text(&cards).expect("decode");
        assert!(text.contains("Begin CmpMap"));
        assert!(text.contains("Nin = 2"));
        assert!(text.contains("Zoom = 5.5"));
        // The decoder capitalises-first the FITS-uppercase key (MAPA -> Mapa).
        assert!(text.contains("Mapa ="));
        assert!(text.contains("Begin UnitMap"));
        assert!(text.contains("IsA Mapping"));
        assert!(text.contains("End UnitMap"));
        assert!(text.contains("End CmpMap"));
    }

    #[test]
    fn pre_quote_empty_value_is_two_double_quotes() {
        // C PreQuote initialises dquotes = 2 and the loop never runs
        // (fitschan.c:25056-25064), so a zero-length value is emitted as "".
        assert_eq!(pre_quote(""), "\"\"");
    }

    #[test]
    fn pre_quote_truncates_at_the_fits_value_limit() {
        // The accommodation test is
        //   nc + 1 + dq + sq > AST__FITSCHAN_FITSCARDLEN - FITSNAMLEN - 4
        // (fitschan.c:25089), i.e. > 68 for the standard 80/8 card layout.
        let long = "x".repeat(80);
        assert_eq!(pre_quote(&long), "x".repeat(68));
        // Each single quote will be doubled by the card formatter, so it
        // costs two of the 68 columns.
        let quotes = "'".repeat(80);
        assert_eq!(pre_quote(&quotes), "'".repeat(34));
    }

    #[test]
    fn pre_quote_uses_the_last_accommodated_character() {
        // Trailing white space that is truncated away does not force quotes,
        // because dquotes tracks the last *accepted* character.
        let v = format!("{}   ", "y".repeat(68));
        assert_eq!(pre_quote(&v), "y".repeat(68));
        // Trailing white space that survives does force them.
        assert_eq!(pre_quote("z  "), "\"z  \"");
    }

    #[test]
    fn pre_quote_un_pre_quote_round_trip() {
        for v in ["", "z  ", "\"lead", "plain", "a\"b"] {
            let q = pre_quote(v);
            assert_eq!(un_pre_quote(&q), v, "round trip failed for {v:?}");
        }
    }

    /// A long string value is split across CONTINUE cards with a trailing
    /// ampersand on each non-final segment, mirroring C `WriteString`.
    #[test]
    fn write_string_value_splits_long_strings() {
        let mut block: Vec<FitsCard> = Vec::new();
        let long = "Pixel coordinates; first pixel at (-128.5,-127.5) plus padding";
        write_string_value(&mut block, "TITLE_A", long, Some("Title"), true);

        assert!(block.len() >= 2, "long value should split: {block:?}");
        // First card carries the keyword and ends with an ampersand.
        assert_eq!(block[0].keyword, "TITLE_A");
        match &block[0].value {
            FitsValue::String(s) => assert!(s.ends_with('&'), "first segment: {s}"),
            other => panic!("expected string, got {other:?}"),
        }
        // Subsequent cards are CONTINUE.
        assert_eq!(block[1].keyword, "CONTINUE");
    }
}
