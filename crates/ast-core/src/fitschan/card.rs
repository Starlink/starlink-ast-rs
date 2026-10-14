//! FITS header card parsing and formatting.
//!
//! Implements the FITS standard 80-character header card format, supporting
//! all FITS value types: integer, float, string, logical, complex, comment,
//! undefined, and CONTINUE long strings.

use super::card_value::{scan_all, scan_double, scan_int};
use super::format::{VALUE_MAXWIDTH, VALUE_WIDTH, encode_float, encode_float_in};
use crate::error::{AstError, AstResult, ErrorCode};
use crate::frames::axis_format::{is_c_space, sprintf_double};

/// Length of a FITS header card in characters.
pub(super) const FITS_CARD_LEN: usize = 80;

/// Length of a FITS keyword name field (columns 1-8).
pub(super) const FITS_NAME_LEN: usize = 8;

/// A legal FITS keyword-name character: letter, digit, `-`, or `_`.
/// Mirrors C `isFits` (fitschan.c:1361).
fn is_fits_name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-' || c == b'_'
}

/// Validate a raw FITS keyword name (columns 1-8 of the card, original case),
/// returning the warning text C would emit if the name contains an illegal
/// character. Mirrors C `CheckFitsName` (fitschan.c:5939) for the cases that
/// produce a `Warn(this, "badkeyname", ...)` rather than an error.
///
/// Returns `None` for a legal name, a blank (all-whitespace) name (the
/// comment-card special case), or an over-length name (C reports that as an
/// error, not a warning — out of scope here). The returned string preserves
/// the original case of the name, exactly as C's message does (the name field
/// is copied verbatim by `SplitCard` before this check, never upper-cased).
///
/// An `=` before column 9 is an error in C, not a warning, so it is excluded.
/// Non-printing characters use the "non-printing character (ascii value N)"
/// wording; other illegal characters use the "illegal character ('c')" wording.
pub(crate) fn check_fits_name(raw_name: &str) -> Option<String> {
    // The name field is the first 8 columns; trim trailing whitespace as
    // SplitCard does (it nulls trailing spaces). A name that is entirely
    // whitespace is the comment-card special case — legal, no warning.
    let name = raw_name.get(..FITS_NAME_LEN).unwrap_or(raw_name);
    let trimmed = name.trim_end();
    if trimmed.is_empty() {
        return None;
    }

    for &c in trimmed.as_bytes() {
        if !is_fits_name_char(c) {
            // An `=` is reported as an error by C, not a warning.
            if c == b'=' {
                return None;
            }
            if c < b' ' {
                return Some(format!(
                    "The FITS keyword name ('{trimmed}') contains an illegal \
                     non-printing character (ascii value {c})."
                ));
            }
            if c > b' ' {
                return Some(format!(
                    "The FITS keyword name ('{trimmed}') contains an illegal \
                     character ('{}').",
                    c as char
                ));
            }
            // C's chain has no arm for a space (fitschan.c:6072-6095): it
            // breaks out of the loop without warning.
            return None;
        }
    }
    None
}

/// Column (0-based) where right-justified numeric values and logical T/F land.
/// In FITS standard terms this is column 30 (1-based), i.e. index 29.
const FITS_RL_COL: usize = 29;

/// The width of the imaginary part of a complex value, right-justified to
/// column 50: C's `FITSIMCOL - FITSRLCOL`.
const IMAG_WIDTH: usize = 20;

/// The column (1-based) in which a quoted string value ends at the
/// earliest, C's `FITSSTCOL`.
const FITS_ST_COL: usize = 20;

/// The column (1-based) in which a comment starts when it fits, C's
/// `FITSCOMCOL`.
pub(super) const FITS_COM_COL: usize = 32;

/// The number of decimal digits in `INT_MAX`, C's `int_dig` in `Split`
/// (fitschan.c:32995): an integer value with this many digits or more is
/// held as a 64-bit integer.
const INT_DIG: usize = 10;

/// A typed value from a FITS header card.
#[derive(Debug, Clone, PartialEq)]
pub enum FitsValue {
    /// Integer value that C holds as an `int` (`AST__INT`).
    Int(i64),
    /// Integer value that C holds as an `int64_t` (`AST__KINT`): one read
    /// from a card with too many digits or too large a value for an `int`,
    /// or stored by [`FitsChan::set_fits_k`](super::FitsChan::set_fits_k).
    KInt(i64),
    /// Floating-point value.
    Float(f64),
    /// Character string value (single-quoted in FITS).
    String(String),
    /// Logical value (T or F).
    Logical(bool),
    /// Complex floating-point (real, imaginary).
    ComplexFloat(f64, f64),
    /// Complex integer (real, imaginary).
    ComplexInt(i64, i64),
    /// Comment-only card (COMMENT, HISTORY, blank keyword).
    Comment(String),
    /// Undefined value (keyword present but value blank).
    Undef,
    /// CONTINUE long string value.
    Continue(String),
}

/// Break warning text into ~60-character lines on word boundaries, mirroring
/// the wrapping in C `Warn` (fitschan.c:35129-35223). Each returned line is the
/// value of one `ASTWARN` card. Caller frames these with leading and trailing
/// blank `ASTWARN` cards.
pub(crate) fn wrap_warning_text(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let b = bytes.len();
    let mut lines = Vec::new();
    let mut a = 0usize;
    while a < b {
        // Nominal last character index for this line (C: c = a + 60).
        let mut c = a + 60;
        if c >= b {
            c = b - 1;
        } else {
            // Move back to the first space so words are not split (C loop).
            while c > a && !bytes[c].is_ascii_whitespace() {
                c -= 1;
            }
        }
        let nc = c - a + 1;
        lines.push(text[a..a + nc].to_string());
        a = c + 1;
    }
    lines
}

/// Consumption state of a card during a read, mirroring the C library's
/// `USED` / `PROVISIONALLY_USED` card flags (fitschan.c:1554-1565).
///
/// A card consumed in building the object returned by `astRead` is treated as
/// if it no longer exists — it is not re-read, not found by public searches,
/// and not written out — but is retained in the list to position newly written
/// cards. Consumption is provisional during a read attempt and promoted to
/// definite on success (or cleared on failure).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Consumption {
    /// Not consumed.
    #[default]
    Free,
    /// Candidate for consumption by the in-progress read.
    Provisional,
    /// Consumed by a completed read; excluded from output.
    Used,
}

/// Per-card read-tracking flags, mirroring the C library's card flag bitmask
/// (fitschan.c:1554-1580). A FitsChan's dump holds them as that bitmask
/// ([`c_bits`](Self::c_bits)).
///
/// `consumption` and `protected` are orthogonal: a `Used` card may also be
/// `protected`. A protected card is exempt from being marked consumed (the
/// foundation for the public `astRetainFits` operation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct CardFlags {
    /// Consumption state for the current/last read.
    pub consumption: Consumption,
    /// Set via `astRetainFits`; exempts the card from consumption marking.
    pub protected: bool,
    /// Set on a card added during the current write pass, mirroring C's
    /// `NEW1` flag (fitschan.c:1652). Cleared, or used to delete the
    /// card, once the whole write is known to have succeeded or failed.
    pub new_write: bool,
    /// Set on a card added during the current per-version pass, mirroring
    /// C's `NEW2` flag (fitschan.c:1653). Cleared, or used to delete the
    /// card, at the end of each axis-description version.
    pub new_version: bool,
}

impl CardFlags {
    /// The flags as C's bitmask: `USED` 1, `PROVISIONALLY_USED` 2, `NEW1` 4,
    /// `NEW2` 8 and `PROTECTED` 16 (fitschan.c:1682-1708).
    pub(crate) fn c_bits(self) -> i64 {
        let consumption = match self.consumption {
            Consumption::Free => 0,
            Consumption::Used => 1,
            Consumption::Provisional => 2,
        };
        consumption
            | if self.new_write { 4 } else { 0 }
            | if self.new_version { 8 } else { 0 }
            | if self.protected { 16 } else { 0 }
    }

    /// The flags C's bitmask `bits` holds. A card both used and
    /// provisionally used is used.
    pub(crate) fn from_c_bits(bits: i64) -> Self {
        Self {
            consumption: if bits & 1 != 0 {
                Consumption::Used
            } else if bits & 2 != 0 {
                Consumption::Provisional
            } else {
                Consumption::Free
            },
            new_write: bits & 4 != 0,
            new_version: bits & 8 != 0,
            protected: bits & 16 != 0,
        }
    }
}

/// The data type of a FITS card, as C's `CardType` gives it (the `AST__*`
/// type codes of fitschan.h).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardType {
    /// No current card (`AST__NOTYPE`).
    NoType,
    /// A commentary card (`AST__COMMENT`).
    Comment,
    /// An integer held as an `int` (`AST__INT`).
    Int,
    /// A floating value (`AST__FLOAT`).
    Float,
    /// A string (`AST__STRING`).
    String,
    /// A complex floating value (`AST__COMPLEXF`).
    ComplexF,
    /// A complex integer value (`AST__COMPLEXI`).
    ComplexI,
    /// A logical value (`AST__LOGICAL`).
    Logical,
    /// A CONTINUE card (`AST__CONTINUE`).
    Continue,
    /// A keyword with no value (`AST__UNDEF`).
    Undef,
    /// An integer held as an `int64_t` (`AST__KINT`).
    KInt,
}

impl CardType {
    /// C's integer code for the type.
    pub fn code(self) -> i32 {
        match self {
            CardType::NoType => -1,
            CardType::Comment => 0,
            CardType::Int => 1,
            CardType::Float => 2,
            CardType::String => 3,
            CardType::ComplexF => 4,
            CardType::ComplexI => 5,
            CardType::Logical => 6,
            CardType::Continue => 7,
            CardType::Undef => 8,
            CardType::KInt => 9,
        }
    }

    /// The name a FitsChan's dump gives the type (fitschan.c `type_names`),
    /// or `None` for [`CardType::NoType`].
    pub(crate) fn dump_name(self) -> Option<&'static str> {
        TYPE_NAMES.get(usize::try_from(self.code()).ok()?).copied()
    }

    /// The type a dump names, ignoring case, as C's `FindString` matches it.
    pub(crate) fn from_dump_name(name: &str) -> Option<Self> {
        let code = TYPE_NAMES
            .iter()
            .position(|n| n.eq_ignore_ascii_case(name))?;
        Some(match code {
            0 => CardType::Comment,
            1 => CardType::Int,
            2 => CardType::Float,
            3 => CardType::String,
            4 => CardType::ComplexF,
            5 => CardType::ComplexI,
            6 => CardType::Logical,
            7 => CardType::Continue,
            8 => CardType::Undef,
            _ => CardType::KInt,
        })
    }
}

/// The names of the card types, indexed by C's type code (fitschan.c
/// `type_names`).
const TYPE_NAMES: [&str; 10] = [
    "comment",
    "integer",
    "floating point",
    "string",
    "complex floating point",
    "complex integer",
    "logical",
    "continuation string",
    "undef",
    "64 bit integer",
];

/// A single FITS header card (keyword + value + comment).
#[derive(Debug, Clone)]
pub struct FitsCard {
    /// Keyword name (max 8 chars, uppercase, trimmed).
    pub keyword: String,
    /// Typed value.
    pub value: FitsValue,
    /// Comment string (text after / separator).
    pub comment: Option<String>,
    /// Read-consumption / protection flags (see [`CardFlags`]).
    pub(crate) flags: CardFlags,
}

impl FitsCard {
    /// The card's data type (C's `CardType`).
    pub fn card_type(&self) -> CardType {
        match self.value {
            FitsValue::Int(_) => CardType::Int,
            FitsValue::KInt(_) => CardType::KInt,
            FitsValue::Float(_) => CardType::Float,
            FitsValue::String(_) => CardType::String,
            FitsValue::Logical(_) => CardType::Logical,
            FitsValue::ComplexFloat(..) => CardType::ComplexF,
            FitsValue::ComplexInt(..) => CardType::ComplexI,
            FitsValue::Comment(_) => CardType::Comment,
            FitsValue::Undef => CardType::Undef,
            FitsValue::Continue(_) => CardType::Continue,
        }
    }

    /// The keyword as C's `CardName` gives it: an 8-space name for a card
    /// with a blank keyword.
    pub(crate) fn c_name(&self) -> &str {
        if self.keyword.is_empty() {
            "        "
        } else {
            &self.keyword
        }
    }

    /// The comment as C's `CardComm` gives it: for a commentary card the
    /// text from column 9, leading and trailing blanks included, as C's
    /// `NewCard` keeps them; otherwise the comment after the value. `None`
    /// when there is none.
    pub(crate) fn c_comment(&self) -> Option<&str> {
        match &self.value {
            FitsValue::Comment(text) => (!text.trim().is_empty()).then_some(text.as_str()),
            _ => self.comment.as_deref().filter(|c| !c.is_empty()),
        }
    }

    /// Whether this card has been consumed by a completed read and so must be
    /// excluded from output (mirrors the C `USED` flag / `CARDUSED` macro).
    pub(crate) fn is_consumed(&self) -> bool {
        self.flags.consumption == Consumption::Used
    }

    /// Parse an 80-character FITS header card.
    ///
    /// If the input is shorter than 80 characters it is right-padded with spaces.
    /// If longer, it is truncated to 80 characters. Non-printing trailing
    /// characters (e.g. newlines) are stripped before padding. A value whose
    /// type cannot be determined gives a commentary card holding just the
    /// keyword, as the C library stores it. An `=` within the keyword name
    /// is AST__BDFTS.
    pub fn parse(card: &str) -> AstResult<Self> {
        Self::split(card).map(|(card, _)| card)
    }

    /// C's `Split`: the card [`parse`](Self::parse) gives, and whether its
    /// value was one whose type could not be determined, which C reports as
    /// a `badkeyvalue` warning.
    pub(crate) fn split(card: &str) -> AstResult<(Self, bool)> {
        // Prepare the 80-character card buffer.
        let (card, nc) = prepare_card(card);
        let bytes = card.as_bytes();

        // Extract keyword (columns 1-8, i.e. indices 0..8).
        let raw_name = &card[..FITS_NAME_LEN];
        let keyword = raw_name.trim_end().to_uppercase();
        let blank_name = keyword.is_empty();

        // C's `CheckFitsName` stops at the first character that is not a
        // legal keyword character, and only an `=` is an error.
        if raw_name.trim_end().bytes().find(|&c| !is_fits_name_char(c)) == Some(b'=') {
            return Err(AstError::new(
                ErrorCode::Bdfts,
                "astPutFits(FitsChan): An equals sign ('=') was found before column 9 \
                 within a FITS keyword name or header card.",
            ));
        }

        // Check for CONTINUE card: keyword is CONTINUE, col 9 is space, col 11 is quote.
        let is_continue = keyword == "CONTINUE"
            && nc > FITS_NAME_LEN + 3
            && bytes[FITS_NAME_LEN] == b' '
            && bytes[FITS_NAME_LEN + 2] == b'\'';

        // Commentary cards: COMMENT, HISTORY, blank keyword, or no '=' in col 9.
        let is_commentary = !is_continue
            && (blank_name
                || keyword == "COMMENT"
                || keyword == "HISTORY"
                || bytes[FITS_NAME_LEN] != b'=');

        if is_commentary {
            // C copies the characters supplied after column 8, not the
            // padding.
            let text = if nc > FITS_NAME_LEN {
                &card[FITS_NAME_LEN..nc]
            } else {
                ""
            };
            let keyword_out = if blank_name { String::new() } else { keyword };
            let card = FitsCard {
                keyword: keyword_out,
                value: FitsValue::Comment(text.to_string()),
                comment: None,
                flags: CardFlags::default(),
            };
            return Ok((card, false));
        }

        // Value cards: col 9 is '=' (or CONTINUE card).
        // Value area starts at col 11 (index 10) for '=' cards, or col 11 for CONTINUE.
        let value_start = if is_continue {
            FITS_NAME_LEN + 2 // skip "CONTINUE " then quote is at +2
        } else {
            FITS_NAME_LEN + 1 // skip "=" — but skip the space in col 10 during parsing
        };

        let value_area = &card[value_start..];

        // Find first non-blank character in value area.
        let first_nonblank = value_area.find(|c: char| !c.is_ascii_whitespace());

        // If the value area starts with a single quote, it is a string value.
        if let Some(pos) = first_nonblank {
            let first_char = value_area.as_bytes()[pos];
            if first_char == b'\'' {
                // The string, and any comment after it, end at character
                // `nc`, not at the padding.
                let value_area = &card[value_start..nc.max(value_start + pos + 1)];
                return parse_string_value(&keyword, value_area, pos, is_continue)
                    .map(|card| (card, false));
            }
        }

        // Other values end at the first '/', which starts the comment.
        let (val_str, comment_str) = split_value_comment(value_area);
        let value = match split_value(val_str) {
            Some(value) => value,
            // C stores a value it cannot type as a commentary card holding
            // just the keyword, and reports it as `badkeyvalue`.
            None => {
                return Ok((
                    FitsCard {
                        keyword,
                        value: FitsValue::Comment(String::new()),
                        comment: None,
                        flags: CardFlags::default(),
                    },
                    true,
                ));
            }
        };
        Ok((
            FitsCard {
                keyword,
                value,
                comment: trim_comment(comment_str),
                flags: CardFlags::default(),
            },
            false,
        ))
    }

    /// Format the card back to an 80-character FITS header card string.
    /// Format this card as an 80-character FITS header string using a
    /// specific number of significant digits for float values, with the
    /// formatter's default sequence-rounding protection (`fitsrnd = 10`).
    pub fn format_with_digits(&self, digits: i32) -> AstResult<String> {
        self.format_internal(digits, 10)
    }

    /// Format this card with the given digit count and FitsRounding
    /// protection. `fitsrnd` is the number of leading significant digits
    /// the formatter protects from sequence-rounding (mirrors the C
    /// library's `FitsRounding` attribute).
    pub fn format_with_digits_and_rounding(
        &self,
        digits: i32,
        fitsrnd: usize,
    ) -> AstResult<String> {
        self.format_internal(digits, fitsrnd)
    }

    /// Format this card as an 80-character FITS header string.
    pub fn format(&self) -> AstResult<String> {
        self.format_internal(15, 10)
    }

    /// C's `FormatCard`. A string value that cannot fit in the card is
    /// AST__BDFTS, as `EncodeValue` reports it.
    fn format_internal(&self, digits: i32, fitsrnd: usize) -> AstResult<String> {
        let mut buf = String::with_capacity(FITS_CARD_LEN);

        // Write keyword, left-justified, padded to 8 chars.
        let kw = &self.keyword;
        buf.push_str(kw);
        for _ in kw.len()..FITS_NAME_LEN {
            buf.push(' ');
        }

        match &self.value {
            FitsValue::Comment(text) => {
                // Commentary cards: keyword + text, padded to 80.
                let remaining = FITS_CARD_LEN - buf.len();
                let text_bytes = text.as_bytes();
                let to_copy = text_bytes.len().min(remaining);
                buf.push_str(&text[..to_copy]);
            }
            FitsValue::Continue(s) => {
                // CONTINUE card: no '=' sign, space then quoted string.
                buf.push(' '); // col 9
                buf.push(' '); // col 10
                format_string_value(&mut buf, s, &self.keyword)?;
                append_comment(&mut buf, self.comment.as_deref());
            }
            FitsValue::Undef => {
                // Value indicator with blank value area.
                buf.push_str("= ");
                append_comment(&mut buf, self.comment.as_deref());
            }
            FitsValue::Int(v) | FitsValue::KInt(v) => {
                buf.push_str("= ");
                // Right-justify integer to column 30 (index 29).
                // Columns used for value: indices 10..30, i.e. 20 chars.
                let formatted = format!("{v}");
                let width = FITS_RL_COL + 1 - (FITS_NAME_LEN + 2); // 20
                if formatted.len() < width {
                    for _ in 0..(width - formatted.len()) {
                        buf.push(' ');
                    }
                }
                buf.push_str(&formatted);
                append_comment(&mut buf, self.comment.as_deref());
            }
            FitsValue::Float(v) => {
                buf.push_str("= ");
                let text = encode_float(*v, digits, fitsrnd).ok_or_else(|| {
                    AstError::new(
                        ErrorCode::Bdfts,
                        format!(
                            "Cannot encode floating point value {} into a FITS header \
                             card for keyword '{}'.",
                            sprintf_double("%g", *v),
                            self.keyword
                        ),
                    )
                })?;
                buf.push_str(&text);
                append_comment(&mut buf, self.comment.as_deref());
            }
            FitsValue::Logical(v) => {
                buf.push_str("= ");
                // T or F at column 30 (index 29). Value area starts at index 10.
                // So we need 19 spaces then T/F.
                let width = FITS_RL_COL + 1 - (FITS_NAME_LEN + 2); // 20
                for _ in 0..(width - 1) {
                    buf.push(' ');
                }
                buf.push(if *v { 'T' } else { 'F' });
                append_comment(&mut buf, self.comment.as_deref());
            }
            FitsValue::String(s) => {
                buf.push_str("= ");
                format_string_value(&mut buf, s, &self.keyword)?;
                append_comment(&mut buf, self.comment.as_deref());
            }
            FitsValue::ComplexFloat(r, i) => {
                buf.push_str("= ");
                // The real part is right-justified to column 30 and the
                // imaginary part to column 50.
                let cannot = |part: &str| {
                    AstError::new(
                        ErrorCode::Bdfts,
                        format!(
                            "Cannot encode {part} part of a complex floating point value \
                             [{},{}] into a FITS header card for keyword '{}'.",
                            sprintf_double("%g", *r),
                            sprintf_double("%g", *i),
                            self.keyword
                        ),
                    )
                };
                let room = FITS_CARD_LEN - (FITS_NAME_LEN + 3);
                let real = encode_float_in(*r, digits, VALUE_WIDTH, VALUE_MAXWIDTH, fitsrnd)
                    .filter(|t| t.len() <= room)
                    .ok_or_else(|| cannot("real"))?;
                let imag = encode_float_in(
                    *i,
                    digits,
                    IMAG_WIDTH,
                    VALUE_MAXWIDTH - 1 - real.len(),
                    fitsrnd,
                )
                .ok_or_else(|| cannot("imaginary"))?;
                buf.push_str(&real);
                buf.push_str(&imag);
                append_comment(&mut buf, self.comment.as_deref());
            }
            FitsValue::ComplexInt(r, i) => {
                buf.push_str("= ");
                let r_fmt = format!("{r}");
                let i_fmt = format!("{i}");
                let r_width = FITS_RL_COL + 1 - (FITS_NAME_LEN + 2); // 20
                if r_fmt.len() < r_width {
                    for _ in 0..(r_width - r_fmt.len()) {
                        buf.push(' ');
                    }
                }
                buf.push_str(&r_fmt);
                // C's `%*d` with width FITSIMCOL - FITSRLCOL + 1 (fitschan.c
                // `EncodeValue`).
                let i_width = 21;
                if i_fmt.len() < i_width {
                    for _ in 0..(i_width - i_fmt.len()) {
                        buf.push(' ');
                    }
                }
                buf.push_str(&i_fmt);
                append_comment(&mut buf, self.comment.as_deref());
            }
        }

        // Pad to 80 characters.
        while buf.len() < FITS_CARD_LEN {
            buf.push(' ');
        }
        // Truncate if over 80.
        buf.truncate(FITS_CARD_LEN);
        Ok(buf)
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Prepare the card: strip trailing non-printing chars, pad/truncate to 80.
/// The card padded with spaces to 80 characters, and C's `nc`, the number of
/// characters `Split` reads: at most 80, less any non-printing characters
/// that end them.
fn prepare_card(input: &str) -> (String, usize) {
    let truncated: String = input.chars().take(FITS_CARD_LEN).collect();
    let trimmed = truncated.trim_end_matches(|c: char| !c.is_ascii_graphic() && c != ' ');
    let nc = trimmed.len();
    let mut card = trimmed.to_string();
    while card.len() < FITS_CARD_LEN {
        card.push(' ');
    }
    (card, nc)
}

/// Parse a string value (starts with quote) from the value area.
fn parse_string_value(
    keyword: &str,
    value_area: &str,
    quote_pos: usize,
    is_continue: bool,
) -> AstResult<FitsCard> {
    let bytes = value_area.as_bytes();
    let mut result = String::new();
    let mut i = quote_pos + 1; // skip opening quote
    let mut last_was_quote = false;

    while i < bytes.len() {
        let ch = bytes[i] as char;
        if ch == '\'' {
            if last_was_quote {
                // Two adjacent quotes → one literal quote.
                result.push('\'');
                last_was_quote = false;
            } else {
                last_was_quote = true;
            }
        } else {
            if last_was_quote {
                // Single quote ended the string. 'i' points to first char after closing quote.
                break;
            }
            result.push(ch);
        }
        i += 1;
    }

    // If we ended the loop with last_was_quote true and didn't break,
    // that means the closing quote was at the end of the area.

    // The value is kept as written, trailing spaces included: C's `Split`
    // stores it so, and only `astGetFitsS` (`MAKE_FGET`) strips the
    // trailing spaces of a value of 8 characters or fewer.

    // Everything after the closing quote is potential comment area.
    // Find the '/' comment separator in the remainder.
    let remainder = if i < bytes.len() {
        &value_area[i..]
    } else {
        ""
    };
    let comment = find_comment_in_remainder(remainder);

    let value = if is_continue {
        FitsValue::Continue(result)
    } else {
        FitsValue::String(result)
    };

    Ok(FitsCard {
        keyword: keyword.to_string(),
        value,
        comment,
        flags: crate::fitschan::card::CardFlags::default(),
    })
}

/// Split the value area into value string and comment string at the first '/'.
fn split_value_comment(value_area: &str) -> (&str, Option<&str>) {
    if let Some(slash_pos) = value_area.find('/') {
        (&value_area[..slash_pos], Some(&value_area[slash_pos + 1..]))
    } else {
        (value_area, None)
    }
}

/// Find comment after the closing quote of a string value.
fn find_comment_in_remainder(remainder: &str) -> Option<String> {
    if let Some(slash_pos) = remainder.find('/') {
        let comment = &remainder[slash_pos + 1..];
        let trimmed = comment.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    } else {
        None
    }
}

/// Trim and return comment text, or None if blank.
fn trim_comment(comment: Option<&str>) -> Option<String> {
    comment.and_then(|c| {
        let trimmed = c.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// C's `Split` on the value string of a card that is not a string: the
/// value, or `None` when its type cannot be determined.
///
/// Any case-insensitive abbreviation of YES, NO, TRUE or FALSE is a logical.
/// A value with no `.`, `E` or `D` (either case) is read as two integers,
/// then as one, as `sscanf` reads `%d` and `%PRId64`; any other as two
/// floating point values, then one, as `%lf` reads them, and failing that
/// the same again with each `D` replaced by `e`. Each reading must use
/// every character but trailing blanks.
fn split_value(v: &str) -> Option<FitsValue> {
    let v0 = v.trim_start_matches(|c: char| is_c_space(c as u8));
    match full_form(&["YES", "NO", "TRUE", "FALSE"], v0) {
        Some(0 | 2) => return Some(FitsValue::Logical(true)),
        Some(1 | 3) => return Some(FitsValue::Logical(false)),
        _ => {}
    }
    if chr_len(v) == 0 {
        return Some(FitsValue::Undef);
    }
    if !v.contains(['.', 'E', 'e', 'D', 'd']) {
        if let Some(ri) = scan_all(v, 2, scan_int) {
            // `%d` narrows each part to an `int`.
            return Some(FitsValue::ComplexInt(
                i64::from(ri[0] as i32),
                i64::from(ri[1] as i32),
            ));
        }
        let k = scan_all(v, 1, scan_int)?[0];
        // C keeps an `int` only when the value fits one and has fewer
        // digits than `INT_MAX`.
        let ndig = v.bytes().filter(u8::is_ascii_digit).count();
        return Some(if i32::try_from(k).is_ok() && ndig < INT_DIG {
            FitsValue::Int(k)
        } else {
            FitsValue::KInt(k)
        });
    }
    let floats = |v: &str| {
        if let Some(ri) = scan_all(v, 2, scan_double) {
            return Some(FitsValue::ComplexFloat(ri[0], ri[1]));
        }
        scan_all(v, 1, scan_double).map(|f| FitsValue::Float(f[0]))
    };
    floats(v).or_else(|| floats(&v.replace(['d', 'D'], "e")))
}

/// C's `FullForm` with abbreviations allowed: the index of the option in
/// `list` of which `test`, ignoring trailing blanks and case, is an
/// abbreviation, or `None` if there is no such option or more than one.
fn full_form(list: &[&str], test: &str) -> Option<usize> {
    let t = &test.as_bytes()[..chr_len(test)];
    let mut found = list.iter().enumerate().filter(|(_, option)| {
        let o = option.as_bytes();
        t.len() <= o.len() && t.eq_ignore_ascii_case(&o[..t.len()])
    });
    let (i, _) = found.next()?;
    found.next().is_none().then_some(i)
}

/// C's `ChrLen`: the length of `s` without trailing blanks and
/// non-printing characters.
pub(crate) fn chr_len(s: &str) -> usize {
    let b = s.as_bytes();
    b.iter()
        .rposition(|&c| c.is_ascii_graphic())
        .map_or(0, |i| i + 1)
}

/// The text of the `badkeyvalue` warning C's `Split` gives for `card`,
/// which holds at most 80 characters of it.
pub(crate) fn bad_value_warning(card: &str) -> String {
    let card = card.split('\0').next().unwrap_or_default();
    let end = card
        .char_indices()
        .nth(FITS_CARD_LEN)
        .map_or(card.len(), |(i, _)| i);
    format!("The keyword value is illegal in '{}'", &card[..end])
}

/// C's `EncodeValue` for a string value starting in column 11: the string
/// in quotes, each quote doubled, padded to end in column 20 at least. The
/// characters that will not fit in the card are dropped if they are blanks,
/// and are otherwise AST__BDFTS.
fn format_string_value(buf: &mut String, s: &str, keyword: &str) -> AstResult<()> {
    let room = FITS_CARD_LEN - (FITS_NAME_LEN + 3);
    buf.push('\'');
    let mut len = 1;
    let mut rest = "";
    for (i, ch) in s.char_indices() {
        let quote = ch == '\'';
        if len + ch.len_utf8() + usize::from(quote) > room {
            rest = &s[i..];
            break;
        }
        buf.push(ch);
        len += ch.len_utf8();
        if quote {
            buf.push('\'');
            len += 1;
        }
    }
    while len < FITS_ST_COL - (FITS_NAME_LEN + 3) {
        buf.push(' ');
        len += 1;
    }
    buf.push('\'');
    if rest.bytes().any(|c| !is_c_space(c)) {
        return Err(AstError::new(
            ErrorCode::Bdfts,
            format!("Cannot encode string '{s}' into a FITS header card for keyword '{keyword}'."),
        ));
    }
    Ok(())
}

/// C's `FormatCard` for the comment after a value: "/ " and the comment,
/// starting in column 32 if it fits there, otherwise far enough left to end
/// in column 80 though never closer than one blank after the value, and
/// truncated at column 80.
fn append_comment(buf: &mut String, comment: Option<&str>) {
    let Some(com) = comment.filter(|c| !c.is_empty()) else {
        return;
    };
    // `comstart` is C's 1-based column, from its own expression.
    let comstart = (FITS_CARD_LEN + 3)
        .saturating_sub(chr_len(com))
        .min(FITS_COM_COL)
        .max(buf.len() + 2);
    while buf.len() < comstart - 1 {
        buf.push(' ');
    }
    for ch in "/ ".chars().chain(com.chars()) {
        if buf.len() >= FITS_CARD_LEN {
            break;
        }
        buf.push(ch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to assert parsing of a card with keyword, value, and comment.
    fn assert_parse(
        card_str: &str,
        expected_kw: &str,
        expected_val: FitsValue,
        expected_com: &str,
    ) {
        let card = FitsCard::parse(card_str).unwrap();
        assert_eq!(card.keyword, expected_kw, "keyword mismatch");
        assert_eq!(card.value, expected_val, "value mismatch for {expected_kw}");
        assert_eq!(
            card.comment,
            Some(expected_com.to_string()),
            "comment mismatch for {expected_kw}"
        );
    }

    #[test]
    fn parse_integer() {
        assert_parse(
            "NAXIS   =                    2 / Number of axes",
            "NAXIS",
            FitsValue::Int(2),
            "Number of axes",
        );
    }

    #[test]
    fn parse_negative_integer() {
        assert_parse(
            "NEGVAL  =                  -42 / Negative value",
            "NEGVAL",
            FitsValue::Int(-42),
            "Negative value",
        );
    }

    #[test]
    fn parse_large_integer() {
        let card = "BIGINT  =       9999999999999 / Large int";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "BIGINT");
        assert_eq!(parsed.value, FitsValue::KInt(9_999_999_999_999));
    }

    #[test]
    fn parse_float() {
        assert_parse(
            "CRVAL1  =     266.416666666667 / Reference value",
            "CRVAL1",
            FitsValue::Float(266.416666666667),
            "Reference value",
        );
    }

    #[test]
    fn parse_scientific_notation() {
        let card = "SMALLF  =            1.5E-10 / Tiny float";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "SMALLF");
        assert_eq!(parsed.value, FitsValue::Float(1.5e-10));
        assert_eq!(parsed.comment, Some("Tiny float".to_string()));
    }

    #[test]
    fn parse_d_exponent() {
        let card = "DEXP    =            1.5D-10 / D exponent";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "DEXP");
        assert_eq!(parsed.value, FitsValue::Float(1.5e-10));
        assert_eq!(parsed.comment, Some("D exponent".to_string()));
    }

    #[test]
    fn parse_string() {
        assert_parse(
            "CTYPE1  = 'RA---TAN'           / Axis type",
            "CTYPE1",
            FitsValue::String("RA---TAN".into()),
            "Axis type",
        );
    }

    #[test]
    fn parse_string_with_escaped_quotes() {
        let card = "COMMENT1= 'It''s a test'       / Escaped quote";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "COMMENT1");
        assert_eq!(parsed.value, FitsValue::String("It's a test".into()));
        assert_eq!(parsed.comment, Some("Escaped quote".to_string()));
    }

    #[test]
    fn parse_string_value_keeps_padding_on_long_values() {
        // C strips trailing spaces only when the value is 8 characters or
        // fewer (fitschan.c:15439-15448). An 18-character padded value
        // keeps its padding.
        let card = FitsCard::parse(
            "RA      = '07:21:44.00       ' / RA of observation (hr)                        ",
        )
        .unwrap();
        assert_eq!(
            card.value,
            FitsValue::String("07:21:44.00       ".to_string())
        );
    }

    #[test]
    fn parse_string_value_keeps_padding_on_short_values() {
        // C's Split stores the value as written; only astGetFitsS strips
        // the trailing spaces of a short value.
        let card = FitsCard::parse(
            "CTYPE1  = 'RA--    '           / Axis type                                     ",
        )
        .unwrap();
        assert_eq!(card.value, FitsValue::String("RA--    ".to_string()));
    }

    #[test]
    fn parse_long_string() {
        // A string that fills most of the card.
        let card =
            "LONGSTR = 'This is a long string value that fills up most of the card'          ";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "LONGSTR");
        assert_eq!(
            parsed.value,
            FitsValue::String("This is a long string value that fills up most of the card".into())
        );
    }

    #[test]
    fn parse_logical_true() {
        assert_parse(
            "SIMPLE  =                    T / Fits standard",
            "SIMPLE",
            FitsValue::Logical(true),
            "Fits standard",
        );
    }

    #[test]
    fn parse_logical_false() {
        let card = "EXTEND  =                    F / No extensions";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "EXTEND");
        assert_eq!(parsed.value, FitsValue::Logical(false));
    }

    #[test]
    fn parse_comment_card() {
        let card = "COMMENT This is a test comment";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "COMMENT");
        if let FitsValue::Comment(text) = &parsed.value {
            assert!(text.contains("This is a test comment"));
        } else {
            panic!("Expected Comment value");
        }
    }

    #[test]
    fn parse_history_card() {
        let card = "HISTORY Created by AST library";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "HISTORY");
        assert!(matches!(parsed.value, FitsValue::Comment(_)));
    }

    #[test]
    fn parse_blank_keyword() {
        let card = "        This is a blank keyword comment card";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "");
        assert!(matches!(parsed.value, FitsValue::Comment(_)));
    }

    #[test]
    fn parse_end_card() {
        let card = "END";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "END");
        assert!(matches!(parsed.value, FitsValue::Comment(_)));
    }

    #[test]
    fn parse_undefined_value() {
        let card = "UNDEF   =                      / Undefined value";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "UNDEF");
        assert_eq!(parsed.value, FitsValue::Undef);
        assert_eq!(parsed.comment, Some("Undefined value".to_string()));
    }

    #[test]
    fn parse_continue_card() {
        let card = "CONTINUE  'continued value'    / More text";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "CONTINUE");
        assert_eq!(parsed.value, FitsValue::Continue("continued value".into()));
        assert_eq!(parsed.comment, Some("More text".to_string()));
    }

    #[test]
    fn parse_short_card() {
        // Card shorter than 80 characters — should be padded.
        let card = "NAXIS   =                    3";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "NAXIS");
        assert_eq!(parsed.value, FitsValue::Int(3));
    }

    #[test]
    fn parse_complex_float() {
        let card = "CMPLXF  =                  1.5                 2.5 / Complex float";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "CMPLXF");
        assert_eq!(parsed.value, FitsValue::ComplexFloat(1.5, 2.5));
    }

    #[test]
    fn parse_complex_int() {
        let card = "CMPLXI  =                    3                    4 / Complex int";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "CMPLXI");
        assert_eq!(parsed.value, FitsValue::ComplexInt(3, 4));
    }

    #[test]
    fn parenthesised_complex_is_an_illegal_value() {
        let (card, bad) = FitsCard::split("CMPLXF  = (1.5, 2.5)          / Complex float").unwrap();
        assert!(bad);
        assert_eq!(card.keyword, "CMPLXF");
        assert_eq!(card.value, FitsValue::Comment(String::new()));
        assert_eq!(card.comment, None);
    }

    #[test]
    fn format_integer() {
        let card = FitsCard {
            keyword: "NAXIS".to_string(),
            value: FitsValue::Int(2),
            comment: Some("Number of axes".to_string()),
            flags: crate::fitschan::card::CardFlags::default(),
        };
        let formatted = card.format().unwrap();
        assert_eq!(formatted.len(), 80);
        assert!(formatted.starts_with("NAXIS   = "));
        assert!(formatted.contains("2"));
        assert!(formatted.contains("/ Number of axes"));
    }

    #[test]
    fn format_string() {
        let card = FitsCard {
            keyword: "CTYPE1".to_string(),
            value: FitsValue::String("RA---TAN".into()),
            comment: Some("Axis type".to_string()),
            flags: crate::fitschan::card::CardFlags::default(),
        };
        let formatted = card.format().unwrap();
        assert_eq!(formatted.len(), 80);
        assert!(formatted.contains("'RA---TAN'"));
        assert!(formatted.contains("/ Axis type"));
    }

    #[test]
    fn format_logical() {
        let card = FitsCard {
            keyword: "SIMPLE".to_string(),
            value: FitsValue::Logical(true),
            comment: Some("Fits standard".to_string()),
            flags: crate::fitschan::card::CardFlags::default(),
        };
        let formatted = card.format().unwrap();
        assert_eq!(formatted.len(), 80);
        // T should be at column 30 (index 29).
        assert_eq!(formatted.as_bytes()[29], b'T');
    }

    #[test]
    fn format_float() {
        let card = FitsCard {
            keyword: "CRVAL1".to_string(),
            value: FitsValue::Float(266.416666666667),
            comment: Some("Reference value".to_string()),
            flags: crate::fitschan::card::CardFlags::default(),
        };
        let formatted = card.format().unwrap();
        assert_eq!(formatted.len(), 80);
        assert!(formatted.contains("/ Reference value"));
    }

    /// A value too wide for the 20-column field has no padding to absorb the
    /// exponent-zero strip, so C closes the gap (fitschan.c:9979-9987) and the
    /// value still starts in column 11 like every other value in the header.
    /// Reference card taken from `tidy-pole.native`.
    #[test]
    fn format_float_overwide_exponent_stays_left_of_the_field() {
        let card = FitsCard {
            keyword: "SCL1_A".to_string(),
            value: FitsValue::Float(-1.7453292519943296E-4),
            comment: Some("Scale factor for axis 1".to_string()),
            flags: crate::fitschan::card::CardFlags::default(),
        };
        let formatted = card.format_with_digits(17).unwrap();
        assert_eq!(
            formatted.trim_end(),
            "SCL1_A  = -1.7453292519943296E-4 / Scale factor for axis 1"
        );
    }

    /// A value that fits the 20-column field is right-justified to column 30,
    /// which the exponent-zero strip does not disturb.
    #[test]
    fn format_float_narrow_exponent_stays_right_justified() {
        let card = FitsCard {
            keyword: "SFT1_C".to_string(),
            value: FitsValue::Float(-7.67E-9),
            comment: None,
            flags: crate::fitschan::card::CardFlags::default(),
        };
        let formatted = card.format_with_digits(17).unwrap();
        assert_eq!(formatted.trim_end(), "SFT1_C  =             -7.67E-9");
        assert_eq!(formatted.trim_end().len(), 30);
    }

    #[test]
    fn format_undefined() {
        let card = FitsCard {
            keyword: "UNDEF".to_string(),
            value: FitsValue::Undef,
            comment: Some("No value".to_string()),
            flags: crate::fitschan::card::CardFlags::default(),
        };
        let formatted = card.format().unwrap();
        assert_eq!(formatted.len(), 80);
        assert!(formatted.starts_with("UNDEF   = "));
        assert!(formatted.contains("/ No value"));
    }

    #[test]
    fn format_comment_card() {
        let card = FitsCard {
            keyword: "COMMENT".to_string(),
            value: FitsValue::Comment(" This is a test".to_string()),
            comment: None,
            flags: crate::fitschan::card::CardFlags::default(),
        };
        let formatted = card.format().unwrap();
        assert_eq!(formatted.len(), 80);
        assert!(formatted.starts_with("COMMENT  This is a test"));
    }

    #[test]
    fn roundtrip_integer() {
        let input = "CRPIX1  =                128.0 / Reference pixel";
        let card = FitsCard::parse(input).unwrap();
        let formatted = card.format().unwrap();
        assert_eq!(formatted.len(), 80);
        let reparsed = FitsCard::parse(&formatted).unwrap();
        assert_eq!(card.keyword, reparsed.keyword);
        // Value should be equivalent.
        match (&card.value, &reparsed.value) {
            (FitsValue::Float(a), FitsValue::Float(b)) => {
                assert!((a - b).abs() < 1e-10);
            }
            _ => panic!("Expected Float values"),
        }
    }

    #[test]
    fn roundtrip_string_with_quotes() {
        let card = FitsCard {
            keyword: "TEST".to_string(),
            value: FitsValue::String("It's OK".into()),
            comment: Some("Quoted".to_string()),
            flags: crate::fitschan::card::CardFlags::default(),
        };
        let formatted = card.format().unwrap();
        assert_eq!(formatted.len(), 80);
        let reparsed = FitsCard::parse(&formatted).unwrap();
        assert_eq!(reparsed.value, FitsValue::String("It's OK".into()));
    }

    #[test]
    fn roundtrip_logical() {
        let card = FitsCard {
            keyword: "SIMPLE".to_string(),
            value: FitsValue::Logical(true),
            comment: Some("Standard".to_string()),
            flags: crate::fitschan::card::CardFlags::default(),
        };
        let formatted = card.format().unwrap();
        let reparsed = FitsCard::parse(&formatted).unwrap();
        assert_eq!(reparsed.value, FitsValue::Logical(true));
    }

    #[test]
    fn roundtrip_complex_float() {
        let card = FitsCard {
            keyword: "CMPLX".to_string(),
            value: FitsValue::ComplexFloat(1.5, -2.5),
            comment: None,
            flags: crate::fitschan::card::CardFlags::default(),
        };
        let formatted = card.format().unwrap();
        assert_eq!(formatted.len(), 80);
        // The parenthesized format is not used for formatting (only parsing).
        // The formatted output uses two space-separated floats which the parser
        // should recognize as complex float.
    }

    #[test]
    fn format_continue_card() {
        let card = FitsCard {
            keyword: "CONTINUE".to_string(),
            value: FitsValue::Continue("long string value".into()),
            comment: Some("continuation".to_string()),
            flags: crate::fitschan::card::CardFlags::default(),
        };
        let formatted = card.format().unwrap();
        assert_eq!(formatted.len(), 80);
        assert!(formatted.starts_with("CONTINUE"));
        assert!(formatted.contains("'long string value"));
    }

    #[test]
    fn parse_no_comment() {
        let card = "NAXIS   =                    4";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "NAXIS");
        assert_eq!(parsed.value, FitsValue::Int(4));
        assert_eq!(parsed.comment, None);
    }

    #[test]
    fn parse_string_with_slash_inside() {
        // The '/' inside a quoted string should NOT be treated as comment separator.
        let card = "PATH    = '/usr/local/bin'     / The path";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "PATH");
        assert_eq!(parsed.value, FitsValue::String("/usr/local/bin".into()));
        assert_eq!(parsed.comment, Some("The path".to_string()));
    }

    #[test]
    fn parse_zero_float() {
        let card = "ZEROF   =                  0.0 / Zero float";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "ZEROF");
        assert_eq!(parsed.value, FitsValue::Float(0.0));
    }

    #[test]
    fn parse_negative_float() {
        let card = "NEGF    =    -1.23456789012345 / Negative float";
        let parsed = FitsCard::parse(card).unwrap();
        assert_eq!(parsed.keyword, "NEGF");
        if let FitsValue::Float(v) = parsed.value {
            assert!((v - (-1.23456789012345)).abs() < 1e-14);
        } else {
            panic!("Expected Float");
        }
    }

    #[test]
    fn check_fits_name_legal() {
        assert_eq!(check_fits_name("CRVAL1  "), None);
        assert_eq!(check_fits_name("MJD-OBS "), None);
        assert_eq!(check_fits_name("WCSAXES "), None);
        assert_eq!(check_fits_name("OBS_LON "), None);
        // All-blank name is the comment-card special case — legal.
        assert_eq!(check_fits_name("        "), None);
        // An '=' before column 9 is a C error, not a warning.
        assert_eq!(check_fits_name("A=B     "), None);
    }

    #[test]
    fn check_fits_name_space_is_not_reported() {
        // C CheckFitsName (fitschan.c:6072-6095) branches on `== '='`,
        // `< ' '` and `> ' '`. A space matches none of them, so no warning
        // is issued for a name field that contains one.
        assert_eq!(check_fits_name("FITS hea"), None);
        assert_eq!(check_fits_name("A B     "), None);
    }

    #[test]
    fn check_fits_name_illegal_char_preserves_case() {
        // Mirrors the jan.fits-class "sys." card. The name field is the
        // first 8 columns; the warning text keeps the original case.
        let text = check_fits_name("sys.").expect("'.' is illegal");
        assert_eq!(
            text,
            "The FITS keyword name ('sys.') contains an illegal character ('.')."
        );
    }

    #[test]
    fn wrap_warning_text_breaks_on_word_boundary() {
        let text = "The FITS keyword name ('sys.') contains an illegal character ('.').";
        let lines = wrap_warning_text(text);
        assert_eq!(
            lines,
            vec![
                "The FITS keyword name ('sys.') contains an illegal character ".to_string(),
                "('.').".to_string(),
            ]
        );
    }
}
