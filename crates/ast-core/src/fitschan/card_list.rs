//! Ordered collection of FITS header cards with position tracking.

use std::cell::RefCell;

use super::card::{Consumption, FitsCard, FitsValue, bad_value_warning, check_fits_name};
use super::fits_match::{match_keyword, match_keyword_checked};
use crate::error::AstResult;

/// An ordered collection of [`FitsCard`]s with position-based iteration.
///
/// CardList maintains a current position pointer that advances during
/// search operations, mimicking the C library's "current card" semantics.
#[derive(Debug, Clone)]
pub struct CardList {
    cards: Vec<FitsCard>,
    position: usize,
    /// Keyword names read with `mark=1` during the current read pass. Filled
    /// by the `*_marked` getters and `record_consumed`, drained by
    /// `take_consumed_names`. Interior mutability lets the `&self` readers
    /// record consumption without taking `&mut self`. Mirrors the C
    /// `MarkCard` side effect of `GetValue(..., mark=1, ...)`.
    consumed_names: RefCell<Vec<String>>,
    /// Whether cards added from now on should be stamped as "new" so that
    /// [`fix_new`](CardList::fix_new) can strip them again. Mirrors C's
    /// `mark_new` thread-global (fitschan.c:40625).
    mark_new: bool,
}

/// Which of C's two "new card" flags a [`fix_new`](CardList::fix_new) call
/// acts on (fitschan.c:1583-1584).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewScope {
    /// C's `NEW1`: cards added anywhere in the current `astWrite` call.
    Write,
    /// C's `NEW2`: cards added during the current per-version pass.
    Version,
}

impl CardList {
    /// Create an empty card list.
    pub fn new() -> Self {
        Self {
            cards: Vec::new(),
            position: 0,
            consumed_names: RefCell::new(Vec::new()),
            mark_new: false,
        }
    }

    /// Return the number of cards in the list.
    pub fn len(&self) -> usize {
        self.cards.len()
    }

    /// Return true if the list contains no cards.
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// A list of copies of the cards in `range`, leaving out consumed ones.
    pub(crate) fn sub_list(&self, range: std::ops::RangeInclusive<usize>) -> CardList {
        let mut list = CardList::new();
        for card in &self.cards[range] {
            if !card.is_consumed() {
                list.cards.push(card.clone());
            }
        }
        list
    }

    /// Append a card at the end of the list.
    pub fn push(&mut self, card: FitsCard) {
        self.cards.push(card);
    }

    /// Insert a block of cards in front of the current card, preserving
    /// their order, and leave that card current. Used by the NATIVE writer,
    /// whose cards C stores one at a time with `InsCard`.
    pub(crate) fn insert_at_current(&mut self, block: Vec<FitsCard>) {
        let n = block.len();
        let at = self.position.min(self.cards.len());
        self.cards.splice(at..at, block);
        self.position = at + n;
    }

    /// Insert a block of `ASTWARN` comment cards holding `text` in front of the
    /// current card, mirroring the card sequence C `Warn` (fitschan.c:35055)
    /// writes: a leading blank `ASTWARN`, one card per wrapped ~60-character
    /// line of the text, then a trailing blank `ASTWARN`. Each card is stored
    /// as a string-valued `ASTWARN` keyword so the header writer renders
    /// `ASTWARN = '...'`.
    ///
    /// C writes every card with `astSetFitsS( ..., overwrite = 0 )`, which
    /// `InsCard` (fitschan.c:15926) inserts before the current card and leaves
    /// that card current — so the position advances past the block and
    /// consecutive warnings chain in front of the same card.
    ///
    /// The block is *not* added if an identical block of adjacent `ASTWARN`
    /// cards already exists (fitschan.c:35060-35118).
    pub(crate) fn push_astwarn(&mut self, text: &str) {
        let lines = super::card::wrap_warning_text(text);
        if lines.is_empty() || self.astwarn_block_exists(&lines) {
            return;
        }

        let astwarn = |s: &str| FitsCard {
            keyword: "ASTWARN".to_string(),
            value: FitsValue::String(s.to_string()),
            comment: None,
            used: false,
            flags: crate::fitschan::card::CardFlags::default(),
        };

        let mut block = Vec::with_capacity(lines.len() + 2);
        block.push(astwarn(" "));
        for line in &lines {
            block.push(astwarn(line));
        }
        block.push(astwarn(" "));

        let n = block.len();
        let at = self.position.min(self.cards.len());
        self.cards.splice(at..at, block);
        // The card that was current stays current; it has moved down by `n`.
        self.position = at + n;
    }

    /// True when the list already holds `lines` as a run of adjacent `ASTWARN`
    /// cards. Mirrors C `Warn`'s dedup probe (fitschan.c:35060-35118): search
    /// the whole list for the first line, then require each later line on the
    /// next card. A partial match is not a match. Leaves the current position
    /// unchanged, as C does by reinstating the saved card index
    /// (fitschan.c:35120).
    fn astwarn_block_exists(&self, lines: &[String]) -> bool {
        let is_astwarn_holding = |card: &FitsCard, want: &str| {
            card.keyword.eq_ignore_ascii_case("ASTWARN")
                && matches!(&card.value, FitsValue::String(s) if s == want)
        };
        let first = &lines[0];
        for start in 0..self.cards.len() {
            if !is_astwarn_holding(&self.cards[start], first) {
                continue;
            }
            let all_present = lines[1..].iter().enumerate().all(|(k, line)| {
                self.cards
                    .get(start + 1 + k)
                    .is_some_and(|c| is_astwarn_holding(c, line))
            });
            if all_present {
                return true;
            }
        }
        false
    }

    /// Insert or replace a card, then advance the position past it.
    ///
    /// If `overwrite` is true and a card with the same keyword already exists,
    /// the first matching card is replaced. Otherwise, the card is inserted in
    /// front of the current card, which for a position at end-of-file means it
    /// is appended.
    ///
    /// In both cases the position is left identifying the card *after* the one
    /// stored, so a run of stores appends in call order. That is C's contract
    /// for `astPutFits`/`astSetFits<X>`: "In either case, the Card attribute is
    /// then incremented by one so that it subsequently identifies the card
    /// following the one stored" (fitschan.c:25501).
    pub fn put(&mut self, card: FitsCard, overwrite: bool) {
        if overwrite {
            // Search for existing card with same keyword.
            let kw_upper = card.keyword.to_uppercase();
            if let Some(idx) = self
                .cards
                .iter()
                .position(|c| c.keyword.eq_ignore_ascii_case(&kw_upper))
            {
                self.cards[idx] = card;
                self.position = idx + 1;
                return;
            }
        }
        // Insert in front of the current card.
        let pos = self.position.min(self.cards.len());
        self.cards.insert(pos, card);
        self.position = pos + 1;
    }

    /// Find the first card matching the given keyword (case-insensitive).
    pub fn find(&self, keyword: &str) -> Option<&FitsCard> {
        self.cards
            .iter()
            .find(|c| c.keyword.eq_ignore_ascii_case(keyword))
    }

    /// Mark the first card matching the given keyword as `used`, so it
    /// survives a subsequent [`sweep_unused_wcs`] pass.
    ///
    /// Returns `true` when a card was found and marked, `false` when no
    /// such card existed. This is the Rust equivalent of the C library's
    /// "card was already in the input header — leave it alone" behaviour
    /// for keywords that an encoder doesn't itself emit.
    pub(crate) fn mark_used(&mut self, keyword: &str) -> bool {
        if let Some(card) = self
            .cards
            .iter_mut()
            .find(|c| c.keyword.eq_ignore_ascii_case(keyword))
        {
            card.used = true;
            true
        } else {
            false
        }
    }

    /// Search forward from the current position for a card matching keyword.
    ///
    /// If found, advances position past the result and returns a reference.
    /// If not found, position is unchanged.
    pub fn find_next(&mut self, keyword: &str) -> Option<&FitsCard> {
        let start = self.position;
        for i in start..self.cards.len() {
            if self.cards[i].keyword.eq_ignore_ascii_case(keyword) {
                self.position = i + 1;
                return Some(&self.cards[i]);
            }
        }
        None
    }

    /// Look `keyword` up from the start of the list and return its
    /// floating-point value, leaving the found card current.
    ///
    /// Mirrors C `astGetFitsF`, which finds its card via `SearchCard` — "if the
    /// requested card is found, it becomes the current card" (fitschan.c:15358)
    /// — and, unlike `GetValue`, does *not* reinstate the original card index.
    /// The cursor it leaves behind is what decides where `Warn` inserts its
    /// `ASTWARN` block, so callers that must not disturb it use [`get_float`].
    pub(crate) fn get_float_at_cursor(&mut self, keyword: &str) -> Option<f64> {
        let idx = self
            .cards
            .iter()
            .position(|c| c.keyword.eq_ignore_ascii_case(keyword))?;
        let value = match &self.cards[idx].value {
            FitsValue::Float(v) => *v,
            FitsValue::Int(v) | FitsValue::KInt(v) => *v as f64,
            _ => return None,
        };
        self.position = idx;
        Some(value)
    }

    /// Like [`Self::find_next`], but skips cards consumed by a read (mirrors C
    /// `CARDUSED` / `ignore_used` in the public card-search path). Used by the
    /// public `find_fits`; internal callers use [`Self::find_next`] so they still
    /// see consumed cards (e.g. for write-position bookkeeping).
    pub fn find_next_visible(&mut self, keyword: &str) -> Option<&FitsCard> {
        let start = self.position;
        for i in start..self.cards.len() {
            if !self.cards[i].is_consumed() && self.cards[i].keyword.eq_ignore_ascii_case(keyword) {
                self.position = i + 1;
                return Some(&self.cards[i]);
            }
        }
        None
    }

    /// Get the integer value of the first card matching keyword.
    pub fn get_int(&self, keyword: &str) -> Option<i64> {
        self.find(keyword).and_then(|c| match &c.value {
            FitsValue::Int(v) | FitsValue::KInt(v) => Some(*v),
            FitsValue::Float(v) => Some(*v as i64),
            _ => None,
        })
    }

    /// Get the floating-point value of the first card matching keyword.
    pub fn get_float(&self, keyword: &str) -> Option<f64> {
        self.find(keyword).and_then(|c| match &c.value {
            FitsValue::Float(v) => Some(*v),
            FitsValue::Int(v) | FitsValue::KInt(v) => Some(*v as f64),
            _ => None,
        })
    }

    /// Get the string value of the first card matching keyword.
    pub fn get_string(&self, keyword: &str) -> Option<String> {
        self.find(keyword).and_then(|c| match &c.value {
            FitsValue::String(s) => Some(s.clone()),
            _ => None,
        })
    }

    /// Get the logical value of the first card matching keyword.
    pub fn get_logical(&self, keyword: &str) -> Option<bool> {
        self.find(keyword).and_then(|c| match &c.value {
            FitsValue::Logical(v) => Some(*v),
            _ => None,
        })
    }

    // -----------------------------------------------------------------------
    // Read-consumption tracking (C "USED card" mechanism)
    //
    // The `*_marked` getters mirror C `GetValue(..., mark=1, ...)`: a value
    // read this way records its keyword so the originating card can later be
    // flagged consumed (excluded from output). The plain getters above are
    // the `mark=0` analogues — used for observation-detail keywords that must
    // survive into the output even though they were read.
    // -----------------------------------------------------------------------

    /// Record a keyword as consumed by the current read pass.
    pub(crate) fn record_consumed(&self, keyword: &str) {
        self.consumed_names.borrow_mut().push(keyword.to_string());
    }

    /// `get_float`, recording the keyword as consumed when a card is found.
    pub(crate) fn get_float_marked(&self, keyword: &str) -> Option<f64> {
        let v = self.get_float(keyword);
        if v.is_some() {
            self.record_consumed(keyword);
        }
        v
    }

    /// `get_string`, recording the keyword as consumed when a card is found.
    pub(crate) fn get_string_marked(&self, keyword: &str) -> Option<String> {
        let v = self.get_string(keyword);
        if v.is_some() {
            self.record_consumed(keyword);
        }
        v
    }

    /// Apply the unconditional trailing-space strip that C's FITS-WCS
    /// ingestion path (`WcsFcRead`) applies to every recognised string
    /// keyword immediately before storing it into the `FitsStore`
    /// (fitschan.c:36906):
    ///
    /// ```c
    /// if( CnvValue( fc, AST__STRING, undef, &cval, method, status ) ) {
    ///    cval[ astChrLen( cval ) ] = 0;  /* Exclude trailing spaces */
    ///    SetItemC( (char *****) item, i, jm, s, cval, status );
    /// ```
    ///
    /// This has no length gate, unlike the generic-accessor `<= 8` char
    /// rule (`card.rs::parse_string_value`, mirroring `MAKE_FGET`/
    /// `astGetFits<X>`, fitschan.c ~15407-15420): C keeps the two rules
    /// distinct, so a long padded value read through `astGetFitsS` keeps
    /// its padding, while the same value read into the `FitsStore` by
    /// `WcsFcRead` does not. This applies regardless of the keyword's
    /// `mark` flag — `mark` only gates whether the source card is later
    /// flagged consumed, not whether the stored value gets stripped
    /// (e.g. TIMESYS is `mark=0` at fitschan.c:36816-36823 but still
    /// goes through this same strip).
    ///
    /// Callers apply this at WCS-store call sites only
    /// (`wcs_keywords.rs`, `enc_aips.rs`, `enc_class.rs`) — never at
    /// plain `get_string`/`get_string_marked` sites used for other
    /// purposes, such as `SpecTrans`'s own CTYPE condition checks
    /// (which mirror C's `GetValue2`, which does no stripping at all).
    pub(crate) fn strip_wcs_store_string(value: Option<String>) -> Option<String> {
        // Mirrors C `ChrLen` (fitschan.c:6294-6353), which scans back
        // from the end and stops at the first character that is both
        // printable and not whitespace (`isprint(c) && !isspace(c)`) —
        // equivalent to the first ASCII graphic character (space is
        // printable but is whitespace, so it does not stop the scan).
        value.map(|s| {
            s.trim_end_matches(|c: char| !c.is_ascii_graphic())
                .to_string()
        })
    }

    /// `find`, recording the keyword as consumed when a card is found.
    #[allow(dead_code)]
    pub(crate) fn find_marked(&self, keyword: &str) -> Option<&FitsCard> {
        let found = self.find(keyword);
        if found.is_some() {
            self.record_consumed(keyword);
        }
        found
    }

    /// Drain and return the keyword names recorded as consumed this pass.
    pub(crate) fn take_consumed_names(&self) -> Vec<String> {
        std::mem::take(&mut self.consumed_names.borrow_mut())
    }

    /// Mark the first non-`Used`, non-protected card matching `keyword` as
    /// `Provisional`, mirroring C `MarkCard` (fitschan.c:22987). A no-op when
    /// no matching card exists — so SpecTrans-synthesised keywords (which have
    /// no card in this list) are silently ignored, while genuine originals are
    /// marked.
    pub(crate) fn mark_card(&mut self, keyword: &str) {
        if let Some(card) = self.cards.iter_mut().find(|c| {
            c.keyword.eq_ignore_ascii_case(keyword)
                && c.flags.consumption != Consumption::Used
                && !c.flags.protected
        }) {
            card.flags.consumption = Consumption::Provisional;
        }
    }

    /// Mark *every* non-`Used`, non-protected card matching `keyword` as
    /// `Provisional`, mirroring C `MarkAllCards` (fitschan.c:23105).
    ///
    /// This is the consumption rule for the `GetValue2` path that `SpecTrans`
    /// and `ClassTrans` read through. The value a translation uses is still
    /// the first match, but a header that concatenates several HDUs' worth of
    /// WCS cards under one keyword must not keep its later copies: leaving
    /// them behind produces a partial, self-inconsistent WCS description.
    pub(crate) fn mark_all_cards(&mut self, keyword: &str) {
        for card in self.cards.iter_mut() {
            if card.keyword.eq_ignore_ascii_case(keyword)
                && card.flags.consumption != Consumption::Used
                && !card.flags.protected
            {
                card.flags.consumption = Consumption::Provisional;
            }
        }
    }

    /// Mark the card at `index` as provisionally consumed, mirroring C
    /// `MarkCard` (fitschan.c:23012): a card carrying the `PROTECTED` flag is
    /// exempt, and a card already `Used` is left alone.
    pub(crate) fn mark_at(&mut self, index: usize) {
        if let Some(card) = self.cards.get_mut(index)
            && !card.flags.protected
            && card.flags.consumption != Consumption::Used
        {
            card.flags.consumption = Consumption::Provisional;
        }
    }

    /// Validate and mark the AST banner comment block adjacent to the card at
    /// `index`, mirroring C `ComBlock` (fitschan.c:7878). `incr` is `-1` to
    /// scan backwards from the card before `index` (delimiter character `+`)
    /// and `1` to scan forwards from the card after it (delimiter `-`).
    ///
    /// A block is complete when every card in it is a COMMENT whose text
    /// starts and ends with `AST`, terminated by a card whose interior is
    /// nothing but the delimiter character and spaces. Nothing is marked
    /// unless the whole block validates.
    pub(crate) fn mark_com_block(&mut self, index: usize, incr: i32) -> bool {
        let del = if incr == 1 { '-' } else { '+' };
        let mut found = Vec::new();
        let mut i = index as isize;
        loop {
            i += incr as isize;
            if i < 0 || i as usize >= self.cards.len() {
                return false;
            }
            let FitsValue::Comment(text) = &self.cards[i as usize].value else {
                return false;
            };
            let t = text.trim_end();
            if t.len() < 6 || !t.starts_with("AST") || !t.ends_with("AST") {
                return false;
            }
            found.push(i as usize);
            if t[3..t.len() - 3].chars().all(|c| c == del || c == ' ') {
                break;
            }
        }
        for idx in found {
            self.mark_at(idx);
        }
        true
    }

    /// Resolve provisional consumption after a read completes, mirroring C
    /// `FixUsed` (fitschan.c:16392). On success (`promote`) provisional cards
    /// become `Used`; otherwise they revert to `Free`. When `remove` is set
    /// (NATIVE-encoding read), consumed cards are physically deleted instead.
    pub(crate) fn fix_used(&mut self, promote: bool, remove: bool) {
        if remove {
            // A removed current card passes currency to the next card kept,
            // as C's `DeleteCard` does.
            let before = self.cards[..self.position.min(self.cards.len())]
                .iter()
                .filter(|c| c.flags.consumption != Consumption::Provisional)
                .count();
            self.cards
                .retain(|c| c.flags.consumption != Consumption::Provisional);
            self.position = before;
        } else {
            let target = if promote {
                Consumption::Used
            } else {
                Consumption::Free
            };
            for c in &mut self.cards {
                if c.flags.consumption == Consumption::Provisional {
                    c.flags.consumption = target;
                }
            }
        }
        self.position = self.position.min(self.cards.len());
    }

    /// Mark the first card matching `keyword` as protected, exempting it from
    /// consumption marking. Foundation for the public `astRetainFits`.
    /// Returns `true` when a card was found.
    #[allow(dead_code)]
    pub(crate) fn set_protected(&mut self, keyword: &str) -> bool {
        if let Some(card) = self
            .cards
            .iter_mut()
            .find(|c| c.keyword.eq_ignore_ascii_case(keyword))
        {
            card.flags.protected = true;
            true
        } else {
            false
        }
    }

    /// Record `keyword` as consumed, then delete the first card matching it.
    ///
    /// Used by SpecTrans when it translates a non-standard original keyword
    /// into a canonical form: the original is removed from the working `trans`
    /// list and recorded so it is later marked consumed on the source FitsChan
    /// (and thus excluded from output), mirroring C `SpecTrans`'s `MarkCard`
    /// on each translated original.
    pub(crate) fn delete_consumed(&mut self, keyword: &str) -> bool {
        self.record_consumed(keyword);
        self.delete(keyword)
    }

    /// Delete the first card matching keyword. Returns true if a card was deleted.
    pub fn delete(&mut self, keyword: &str) -> bool {
        if let Some(idx) = self
            .cards
            .iter()
            .position(|c| c.keyword.eq_ignore_ascii_case(keyword))
        {
            self.cards.remove(idx);
            // Adjust position if needed.
            if self.position > idx && self.position > 0 {
                self.position -= 1;
            }
            true
        } else {
            false
        }
    }

    /// Delete the card at the current position.
    ///
    /// If the position is past the end, this is a no-op.
    pub fn delete_at_position(&mut self) {
        if self.position < self.cards.len() {
            self.cards.remove(self.position);
        }
    }

    /// Reset the position to the beginning of the list.
    pub fn rewind(&mut self) {
        self.position = 0;
    }

    /// Set the current position, clamped to end-of-file.
    ///
    /// Passing `usize::MAX` is the idiom for "end of file", mirroring C's
    /// `astSetCard( this, INT_MAX )`.
    pub fn set_position(&mut self, pos: usize) {
        self.position = pos.min(self.cards.len());
    }

    /// Return the current position.
    pub fn position(&self) -> usize {
        self.position
    }

    /// Begin marking every subsequently added card as "new", mirroring C's
    /// `mark_new = 1` at the head of `Write` (fitschan.c:40625). While set,
    /// [`put_or_append`] stamps both the `NEW1` and `NEW2` equivalents onto
    /// each card it inserts, so [`fix_new`] can later strip them.
    pub(crate) fn begin_marking_new(&mut self) {
        self.mark_new = true;
    }

    /// Stop marking added cards as "new", mirroring C's `mark_new = 0` at
    /// the tail of `Write` (fitschan.c:40754).
    pub(crate) fn end_marking_new(&mut self) {
        self.mark_new = false;
    }

    /// Resolve the cards flagged "new" by the current write pass, mirroring
    /// C `FixNew` (fitschan.c:16376).
    ///
    /// When `remove` is set the flagged cards are deleted outright, which is
    /// how C discards the partial description a failed version emitted;
    /// otherwise the flag alone is cleared and the cards are kept. The scope
    /// selects which of C's two flags to act on: [`NewScope::Write`] is
    /// `NEW1` (the whole `astWrite` call) and [`NewScope::Version`] is `NEW2`
    /// (one pass of the per-version loop).
    ///
    /// As in C, when any card is removed the position is left at
    /// end-of-file; when none is, the original position is retained.
    pub(crate) fn fix_new(&mut self, scope: NewScope, remove: bool) {
        let get = |f: &crate::fitschan::card::CardFlags| match scope {
            NewScope::Write => f.new_write,
            NewScope::Version => f.new_version,
        };
        if remove {
            let before = self.cards.len();
            self.cards.retain(|c| !get(&c.flags));
            if self.cards.len() != before {
                self.position = self.cards.len();
                return;
            }
        } else {
            for c in &mut self.cards {
                match scope {
                    NewScope::Write => c.flags.new_write = false,
                    NewScope::Version => c.flags.new_version = false,
                }
            }
        }
        self.position = self.position.min(self.cards.len());
    }

    // -----------------------------------------------------------------------
    // The current card, as C's FitsChan models it
    //
    // C keeps a pointer to the current card, or NULL at end-of-file, and its
    // public functions skip cards a read has consumed (`CARDUSED` with the
    // default `ignore_used` of 1). Here the current card is `position`, an
    // index into `cards`, which still holds the consumed cards; `len()` is
    // end-of-file.
    // -----------------------------------------------------------------------

    /// Whether card `i` is skipped by the public card functions.
    fn hidden(&self, i: usize) -> bool {
        self.cards[i].is_consumed()
    }

    /// The current card, or `None` at end-of-file.
    pub(crate) fn current(&self) -> Option<&FitsCard> {
        self.cards.get(self.position)
    }

    /// C's `MoveCard`: move `mv` cards towards end-of-file (or, if negative,
    /// towards the first card), counting only cards not consumed. Returns the
    /// number of cards moved.
    pub(crate) fn move_card(&mut self, mv: i64) -> i64 {
        if self.cards.is_empty() || mv == 0 {
            return 0;
        }
        let n = self.cards.len();
        let mut card = self.position;
        let mut moved = 0;
        if mv > 0 {
            while moved < mv && card < n {
                card += 1;
                if card < n && !self.hidden(card) {
                    moved += 1;
                }
            }
        } else if card != 0 {
            // From end-of-file the first step back reaches the last card.
            while moved < -mv {
                let mut card0 = card;
                loop {
                    card0 -= 1;
                    if card0 == 0 || !self.hidden(card0) {
                        break;
                    }
                }
                if self.hidden(card0) {
                    break;
                }
                card = card0;
                moved += 1;
                if card == 0 {
                    break;
                }
            }
        }
        self.position = card;
        moved
    }

    /// C's `ClearCard`: make the first card not consumed the current card.
    pub(crate) fn clear_card(&mut self) {
        if self.cards.is_empty() {
            return;
        }
        self.position = 0;
        if self.hidden(0) {
            self.move_card(1);
        }
    }

    /// C's `GetCard`: the 1-based index of the current card among the cards
    /// not consumed, one more than their number at end-of-file, and 0 for an
    /// empty list.
    pub(crate) fn get_card(&mut self) -> usize {
        if self.cards.is_empty() {
            return 0;
        }
        let card0 = self.position;
        self.clear_card();
        let mut index = 1;
        while self.position != card0 && self.position < self.cards.len() {
            index += 1;
            self.move_card(1);
        }
        index
    }

    /// C's `SetCard`: rewind, then move forward `icard - 1` cards.
    pub(crate) fn set_card(&mut self, icard: i64) {
        self.clear_card();
        self.move_card(icard.saturating_sub(1));
    }

    /// C's `TestCard`: whether the current card is not the first.
    pub(crate) fn test_card(&mut self) -> bool {
        let card = self.get_card();
        self.clear_card();
        if self.get_card() == card {
            false
        } else {
            self.set_card(card as i64);
            true
        }
    }

    /// C's `GetNcard`: the number of cards not consumed.
    pub(crate) fn ncard(&self) -> usize {
        self.cards.iter().filter(|c| !c.is_consumed()).count()
    }

    /// C's `GetNkey`: the number of different keywords among the cards not
    /// consumed.
    pub(crate) fn nkey(&self) -> usize {
        let names: std::collections::HashSet<&str> = self
            .cards
            .iter()
            .filter(|c| !c.is_consumed())
            .map(|c| c.c_name())
            .collect();
        names.len()
    }

    /// C's `KeyFields`: the number of cards not consumed whose keyword
    /// matches `template` (see [`match_keyword`]), and the lowest and highest
    /// value of each of the first `maxfld` integer (`%d`) fields among them.
    pub(crate) fn key_fields(&self, template: &str, maxfld: usize) -> (usize, Vec<i32>, Vec<i32>) {
        let mut lo = vec![i32::MAX; maxfld];
        let mut hi = vec![-i32::MAX; maxfld];
        let mut n = 0;
        for card in self.cards.iter().filter(|c| !c.is_consumed()) {
            if let Some(fields) = match_keyword(card.c_name(), template, maxfld) {
                n += 1;
                for (i, f) in fields.into_iter().enumerate() {
                    lo[i] = lo[i].min(f);
                    hi[i] = hi[i].max(f);
                }
            }
        }
        (n, lo, hi)
    }

    /// The first card not consumed with keyword `keyword`, ignoring case.
    pub(crate) fn first_visible(&self, keyword: &str) -> Option<&FitsCard> {
        self.cards
            .iter()
            .find(|c| !c.is_consumed() && c.c_name().eq_ignore_ascii_case(keyword))
    }

    /// C's `RetainFits`: protect the current card from being consumed.
    pub(crate) fn protect_current(&mut self) {
        if let Some(card) = self.cards.get_mut(self.position) {
            card.flags.protected = true;
        }
    }

    /// The second part of C's `PurgeWCS`: give every card not consumed
    /// whose keyword `FindWcs` recognises the flags `USED` alone, so it is
    /// hidden and no longer protected.
    pub(crate) fn consume_wcs_cards(&mut self) {
        for card in &mut self.cards {
            if !card.is_consumed() && is_findwcs_keyword(&card.keyword) {
                card.flags = crate::fitschan::card::CardFlags {
                    consumption: Consumption::Used,
                    ..Default::default()
                };
            }
        }
    }

    /// C's `DeleteCard`: delete the current card, making the next card not
    /// consumed the current one. Does nothing at end-of-file.
    pub(crate) fn delete_card(&mut self) {
        let card = self.position;
        if card >= self.cards.len() {
            return;
        }
        self.move_card(1);
        self.cards.remove(card);
        // The cards after the deleted one move down one place.
        if self.position > card {
            self.position -= 1;
        }
        if self.cards.is_empty() {
            self.position = 0;
        }
    }

    /// C's `InsCard`: with `overwrite`, delete the current card; then insert
    /// `card` in front of the current card (appending at end-of-file), which
    /// stays current. The card is flagged new when the list is marking new
    /// cards (C's `mark_new`).
    pub(crate) fn ins_card(&mut self, mut card: FitsCard, overwrite: bool) {
        if overwrite {
            self.delete_card();
        }
        if self.mark_new {
            card.flags.new_write = true;
            card.flags.new_version = true;
        }
        let pos = self.position.min(self.cards.len());
        self.cards.insert(pos, card);
        self.position = pos + 1;
    }

    /// C's `FindKeyCard`: search forward from the current card for one whose
    /// keyword matches the template `name` (see [`match_keyword`]), leaving
    /// it current, or end-of-file if there is none.
    pub(crate) fn find_key_card(&mut self, name: &str, method: &str) -> AstResult<bool> {
        while let Some(card) = self.current() {
            let matched = match_keyword_checked(card.c_name(), name, method);
            if let Ok(true) = matched {
                return Ok(true);
            }
            // C's loop moves on from a card whose test failed, and only
            // then stops for the error.
            self.move_card(1);
            matched?;
        }
        Ok(false)
    }

    /// C's `SearchCard`: if the card after the current one has keyword
    /// `name`, make it current; otherwise search from the first card with
    /// [`Self::find_key_card`].
    pub(crate) fn search_card(&mut self, name: &str, method: &str) -> AstResult<bool> {
        self.move_card(1);
        if let Some(card) = self.current() {
            // C's `Ustrncmp( CardName, name, FITSNAMLEN )`.
            let a = card
                .c_name()
                .bytes()
                .take(8)
                .map(|c| c.to_ascii_uppercase());
            let b = name.bytes().take(8).map(|c| c.to_ascii_uppercase());
            if a.eq(b) {
                return Ok(true);
            }
        }
        self.clear_card();
        self.find_key_card(name, method)
    }

    /// Return true if the position is at or past the end of the list.
    pub fn at_eof(&self) -> bool {
        self.position >= self.cards.len()
    }

    /// Set an integer keyword value. Overwrites if keyword exists, appends if not.
    pub(crate) fn set_int(&mut self, keyword: &str, value: i64, comment: &str) {
        let card = FitsCard {
            keyword: keyword.to_uppercase(),
            value: FitsValue::Int(value),
            comment: if comment.is_empty() {
                None
            } else {
                Some(comment.to_string())
            },
            used: false,
            flags: crate::fitschan::card::CardFlags::default(),
        };
        self.put_or_append(card);
    }

    /// Set a float keyword value. Overwrites if keyword exists, appends if not.
    pub(crate) fn set_float(&mut self, keyword: &str, value: f64, comment: &str) {
        let card = FitsCard {
            keyword: keyword.to_uppercase(),
            value: FitsValue::Float(value),
            comment: if comment.is_empty() {
                None
            } else {
                Some(comment.to_string())
            },
            used: false,
            flags: crate::fitschan::card::CardFlags::default(),
        };
        self.put_or_append(card);
    }

    /// Set a string keyword value. Overwrites if keyword exists, appends if not.
    pub(crate) fn set_string(&mut self, keyword: &str, value: &str, comment: &str) {
        let card = FitsCard {
            keyword: keyword.to_uppercase(),
            value: FitsValue::String(value.to_string()),
            comment: if comment.is_empty() {
                None
            } else {
                Some(comment.to_string())
            },
            used: false,
            flags: crate::fitschan::card::CardFlags::default(),
        };
        self.put_or_append(card);
    }

    /// Reset the `used` flag on every card to false.
    ///
    /// Used by the FitsChan write path to begin tracking which cards the
    /// encoder touches in this write pass. After the encoder finishes, any
    /// WCS card still flagged unused is a leftover that should be removed
    /// (see [`sweep_unused_wcs`]).
    pub(crate) fn mark_all_unused(&mut self) {
        for card in &mut self.cards {
            card.used = false;
        }
        // Initialise the insertion-point cursor to one past the
        // last existing WCS card in the chan. New cards added by
        // `put_or_append` will land there, which mirrors the C
        // library's effect of inserting freshly-synthesised
        // keywords (MJD-OBS, DATE-OBS, WCSAXES, ...) within the WCS
        // block rather than appending them to the end of the
        // header. Falls back to end-of-list when no WCS card is
        // present in the input.
        //
        // The match set here mirrors C's `FindWcs` (fitschan.c
        // :10710) — narrower than the sweep set (`is_wcs_keyword`)
        // because some non-canonical keywords (RESTFREQ, EPOCH,
        // ...) are processed by SpecTrans into canonical forms but
        // are not part of the FindWcs match set, so the C library
        // anchors the cursor on the canonical block and inserts
        // new cards before any non-canonical trailing keyword.
        self.position = self
            .cards
            .iter()
            .rposition(|c| is_findwcs_keyword(&c.keyword))
            .map(|i| i + 1)
            .unwrap_or(self.cards.len());
    }

    /// Delete every stale WCS-keyword card left over from the input header.
    ///
    /// Used after a FitsChan write pass. A card is stale when all three hold:
    /// it names a WCS keyword, the read consumed it (so it described the old
    /// WCS), and the encoder did not re-write it during this pass (marked
    /// `used = true` via [`put_or_append`]). Switching to a CD-matrix encoding
    /// leaves the input's CDELT/CROTA cards in exactly that state.
    ///
    /// A card the read never consumed is not stale — nothing claimed it, and C
    /// leaves it in the header (`FixUsed`, fitschan.c:16431, removes only the
    /// cards the read marked). That is what keeps the observation-detail
    /// keywords `WcsFcRead` reads with `mark = 0` — MJD-OBS, MJD-AVG, TIMESYS,
    /// DUT1, OBSGEO-* — in the output. Non-WCS cards (SIMPLE, NAXIS, COMMENT,
    /// etc.) are always preserved.
    pub(crate) fn sweep_unused_wcs(&mut self) {
        self.cards
            .retain(|card| card.used || !card.is_consumed() || !is_wcs_keyword(&card.keyword));
        self.position = self.position.min(self.cards.len());
    }

    /// Overwrite existing card or append at end (preserves write order).
    ///
    /// In both branches the resulting card is marked `used = true` so that
    /// [`sweep_unused_wcs`] will keep it. This is how the FitsChan write
    /// path tracks which cards the encoder touched during this pass.
    ///
    /// When overwriting, the existing comment is preserved only if the
    /// data value is unchanged. This matches the C library's SetFits: when
    /// a value is re-written identically, the input header's comment
    /// survives; when the value changes, the encoder's new comment is used.
    fn put_or_append(&mut self, mut card: FitsCard) {
        card.used = true;
        if self.mark_new {
            card.flags.new_write = true;
            card.flags.new_version = true;
        }
        let kw_upper = card.keyword.to_uppercase();
        if let Some(idx) = self
            .cards
            .iter()
            .position(|c| c.keyword.eq_ignore_ascii_case(&kw_upper))
        {
            if self.cards[idx].comment.is_some()
                && values_similar(&self.cards[idx].value, &card.value)
            {
                card.comment = self.cards[idx].comment.clone();
            }
            self.cards[idx] = card;
            // Track the highest position we've touched so future new
            // cards can be inserted just past it.
            if idx >= self.position {
                self.position = idx + 1;
            }
        } else {
            // For a brand-new keyword, insert at `position`: the spot
            // just after the last touched card. Mirrors the effect of
            // C's SetValue inserting at the current-card pointer
            // (fitschan.c:28129) — freshly-synthesised keywords
            // (MJD-OBS, DATE-OBS, WCSAXES) end up clustered with the
            // WCS region they belong to rather than appended at the
            // end of the header. When `position` outruns `cards.len()`
            // (e.g. new-card-only writes), this falls back to push().
            let pos = self.position.min(self.cards.len());
            self.cards.insert(pos, card);
            self.position = pos + 1;
        }
    }

    /// Return an iterator over all cards.
    pub fn iter(&self) -> impl Iterator<Item = &FitsCard> {
        self.cards.iter()
    }

    /// Return a mutating iterator over all cards, for passes that visit every
    /// card in turn (see [`super::wcs_mark`]).
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut FitsCard> {
        self.cards.iter_mut()
    }

    /// Parse multi-line FITS header text into a CardList.
    ///
    /// Each line is parsed as a FITS card. Lines that fail to parse are
    /// skipped with a warning (not an error), following the C library's
    /// permissive behaviour.
    pub fn from_header(text: &str) -> AstResult<Self> {
        Ok(Self::from_header_with_warnings(text)?.0)
    }

    /// [`from_header`](Self::from_header), also returning the text of each
    /// `ASTWARN` block stored, in order, as C's `Warn` adds it to the
    /// Channel's warnings.
    pub(crate) fn from_header_with_warnings(text: &str) -> AstResult<(Self, Vec<String>)> {
        let mut list = Self::new();
        let mut warnings = Vec::new();
        for line in text.lines() {
            // Skip completely empty lines.
            if line.trim().is_empty() {
                continue;
            }
            match FitsCard::split(line) {
                Ok((card, bad_value)) => {
                    // Validate the keyword name (original case, columns 1-8)
                    // and insert ASTWARN cards before the offending card if it
                    // contains an illegal character. Mirrors C `CheckFitsName`
                    // /`Warn`, which run inside `astPutFits`/`SplitCard`
                    // (fitschan.c:32259) so the warning cards precede the bad
                    // card in the FitsChan. Warnings are enabled by default
                    // (the Warnings attribute includes "BadKeyName", Clean is
                    // off).
                    let name_warning = check_fits_name(line);
                    if let Some(text) = &name_warning {
                        // `push_astwarn` inserts in front of the current card,
                        // as C's `InsCard` does. Here the cards are being
                        // appended, so the cursor belongs at the end of the
                        // list — where C's is while `astPutFits` appends.
                        list.position = list.cards.len();
                        list.push_astwarn(text);
                        warnings.push(text.clone());
                    }
                    if bad_value {
                        let text = bad_value_warning(line);
                        list.position = list.cards.len();
                        list.push_astwarn(&text);
                        warnings.push(text);
                    }
                    // The setter `PutFits` hands the card to splits its
                    // keyword name again and warns again of an illegal name;
                    // that block matches the first, so adds no cards.
                    if let Some(text) = name_warning {
                        warnings.push(text);
                    }
                    list.push(card);
                }
                Err(_) => {
                    // Permissive: skip unparseable lines.
                    continue;
                }
            }
        }
        Ok((list, warnings))
    }
}

impl Default for CardList {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// WCS keyword identification
// ---------------------------------------------------------------------------

/// Check if a keyword is a WCS-related keyword that should be removed
/// when writing new WCS data.
///
/// Based on the C library's `FindWcs` function (fitschan.c line 10710).
/// Returns true for all keywords that form part of a WCS encoding.
fn is_wcs_keyword(keyword: &str) -> bool {
    let kw = keyword.to_uppercase();

    // Fixed WCS keywords (exact match).
    if matches!(
        kw.as_str(),
        "WCSAXES"
            | "WCSNAME"
            | "LONPOLE"
            | "LONGPOLE"
            | "LATPOLE"
            | "EQUINOX"
            | "EPOCH"
            | "RADESYS"
            | "RADECSYS"
            | "MJD-OBS"
            | "DATE-OBS"
            | "TIMESYS"
            | "SPECSYS"
            | "SSYSSRC"
            | "SSYSOBS"
            | "VELOSYS"
            | "ZSOURCE"
            | "RESTFRQ"
            | "RESTFREQ"
            | "RESTWAV"
            | "MJD-AVG"
            | "IMAGFREQ"
            | "DELTAV"
            | "VLSR"
            | "VELO-LSR"
            | "VELO-OBS"
            | "VELO-HEL"
            | "VELO-EAR"
    ) {
        return true;
    }

    // Per-axis WCS keywords (CRPIXn, CRVALn, etc.).
    if kw.len() >= 6 {
        let prefix5 = &kw[..5];
        if matches!(
            prefix5,
            "CRPIX" | "CRVAL" | "CDELT" | "CTYPE" | "CUNIT" | "CNAME"
        ) {
            let rest = &kw[5..];
            if rest
                .chars()
                .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
            {
                return true;
            }
        }
        if kw.starts_with("CROTA") {
            return true;
        }
    }

    // CD matrix: CDi_j or CD00i00j format.
    if kw.starts_with("CD") && kw.len() >= 4 {
        let rest = &kw[2..];
        if rest.contains('_') && rest.chars().all(|c| c.is_ascii_digit() || c == '_') {
            return true;
        }
        if rest.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }

    // PC matrix: PCi_j or PC00i00j format.
    if kw.starts_with("PC") && kw.len() >= 4 {
        let rest = &kw[2..];
        if rest.contains('_') && rest.chars().all(|c| c.is_ascii_digit() || c == '_') {
            return true;
        }
        if rest.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }

    // PV and PS projection parameters: PVi_m, PSi_m.
    if (kw.starts_with("PV") || kw.starts_with("PS")) && kw.len() >= 4 {
        let rest = &kw[2..];
        if rest.contains('_') && rest.chars().all(|c| c.is_ascii_digit() || c == '_') {
            return true;
        }
    }

    // PROJP parameters (old-style).
    if kw.starts_with("PROJP") {
        return true;
    }

    // WCSAXES with version suffix (WCSAXESA, etc.)
    if kw.starts_with("WCSAXES") && kw.len() > 7 {
        return true;
    }

    // DSS plate solution keywords.
    if matches!(
        kw.as_str(),
        "PLTRAH"
            | "PLTRAM"
            | "PLTRAS"
            | "PLTDECD"
            | "PLTDECM"
            | "PLTDECS"
            | "PLTDECSN"
            | "PLTSCALE"
            | "XPIXELSZ"
            | "YPIXELSZ"
            | "CNPIX1"
            | "CNPIX2"
            | "PPO1"
            | "PPO2"
            | "PPO3"
            | "PPO4"
            | "PPO5"
            | "PPO6"
    ) {
        return true;
    }
    if kw.starts_with("AMDX") || kw.starts_with("AMDY") {
        return true;
    }

    // OBSGEO- keywords (observatory position).
    if kw.starts_with("OBSGEO-") {
        return true;
    }

    // Alternate version keywords: CnVAL, CnPIX, CnELT, CnYPE, CnNIT.
    if kw.len() >= 5 && kw.starts_with('C') && kw.as_bytes()[1].is_ascii_digit() {
        let suffix = &kw[2..];
        if suffix.starts_with("VAL")
            || suffix.starts_with("PIX")
            || suffix.starts_with("ELT")
            || suffix.starts_with("YPE")
            || suffix.starts_with("NIT")
        {
            return true;
        }
    }

    // AST-extension SkyFrame offset keywords: SREFIS[s], SREF{n}[s],
    // SREFP{n}[s]. These are read into the FitsStore by the write-time
    // store reader (mirroring C `WcsFcRead` (fitschan.c:36650-36695)
    // which reads them with `mark=1`, i.e. for removal) and re-emitted
    // by the writer from `store.skyref` / `store.skyrefis` only when the
    // current Frame is described via `SkySys`. When the offset Frame is
    // routed through `OtherAxes` (Domain != "SKY"), nothing re-emits
    // them, so an input SREFIS / SREF / SREFP card must be swept.
    //
    // Match `SREFIS` and `SREFP`/`SREF` followed by a digit (axis
    // index), each with an optional trailing version letter. This
    // deliberately does NOT match the non-AST `SREFRA` / `SREFDEC`
    // input keywords (their character after `SREF` is a letter, not a
    // digit), which the writer passes through unchanged.
    if let Some(rest) = kw.strip_prefix("SREFIS") {
        if rest.is_empty() || (rest.len() == 1 && rest.as_bytes()[0].is_ascii_uppercase()) {
            return true;
        }
    }
    if let Some(rest) = kw.strip_prefix("SREFP").or_else(|| kw.strip_prefix("SREF")) {
        let mut it = rest.chars();
        if let Some(c0) = it.next()
            && c0.is_ascii_digit()
        {
            // Remaining axis digits then an optional version letter.
            let tail = &rest[c0.len_utf8()..];
            let ok = tail.chars().enumerate().all(|(i, c)| {
                c.is_ascii_digit() || (i + 1 == tail.chars().count() && c.is_ascii_uppercase())
            });
            if ok {
                return true;
            }
        }
    }

    false
}

/// Cursor-anchor predicate mirroring C's `FindWcs` match list
/// (fitschan.c:10810-10866). Used only by [`CardList::mark_all_unused`]
/// to position the new-card insertion cursor: C inserts freshly
/// synthesised WCS cards in front of "the card just past the last
/// FindWcs-recognised keyword in the input", which is **not** the
/// same set as the sweep set ([`is_wcs_keyword`]). For example
/// RESTFREQ — the AIPS misspelling — is processed as part of the
/// SpecTrans rename pipeline and so is in the sweep set, but it's
/// not in C's FindWcs list, so C inserts new cards *before* a
/// trailing RESTFREQ card rather than after it.
fn is_findwcs_keyword(keyword: &str) -> bool {
    let kw = keyword.to_uppercase();

    // Exact-match keywords from FindWcs (with `%0c` alt-letter
    // suffix expansions handled by the optional-trailing-letter
    // logic below).
    let bare = if !kw.is_empty() && kw.as_bytes()[kw.len() - 1].is_ascii_uppercase() {
        // Optional alt-letter suffix: try stripping one letter.
        let stripped = &kw[..kw.len() - 1];
        match stripped {
            "LONPOLE" | "LATPOLE" | "EQUINOX" | "RADESYS" | "RESTFRQ" | "WCSNAME" | "SPECSYS"
            | "SSYSSRC" | "ZSOURCE" | "VELOSYS" | "MJD_AVG" => return true,
            _ => &kw[..],
        }
    } else {
        &kw[..]
    };
    if matches!(
        bare,
        "LONGPOLE"
            | "LONPOLE"
            | "LATPOLE"
            | "EPOCH"
            | "EQUINOX"
            | "MJD-OBS"
            | "DATE-OBS"
            | "TIMESYS"
            | "RADECSYS"
            | "RADESYS"
            | "WCSNAME"
            | "SPECSYS"
            | "SSYSSRC"
            | "ZSOURCE"
            | "VELOSYS"
            | "RESTFRQ"
            | "MJD_AVG"
            | "OBSGEO-X"
            | "OBSGEO-Y"
            | "OBSGEO-Z"
            | "CNPIX1"
            | "CNPIX2"
            | "XPIXELSZ"
            | "YPIXELSZ"
            | "PLTRAH"
            | "PLTRAM"
            | "PLTRAS"
            | "PLTDECD"
            | "PLTDECM"
            | "PLTDECS"
            | "PLTDECSN"
            | "PLTSCALE"
            | "PPO1"
            | "PPO2"
            | "PPO4"
            | "PPO5"
    ) {
        return true;
    }

    // Per-axis WCS keywords (CRPIXn, CRVALn, etc.) and PC/CD/PV/PS
    // matrices. Same patterns as `is_wcs_keyword`, but excludes
    // the AIPS-specific aliases (RESTFREQ, etc.) and DSS-only
    // keywords already enumerated above.
    if kw.len() >= 6 {
        let prefix5 = &kw[..5];
        if matches!(prefix5, "CRPIX" | "CRVAL" | "CDELT" | "CTYPE" | "CUNIT") {
            let rest = &kw[5..];
            if rest
                .chars()
                .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
            {
                return true;
            }
        }
        if kw.starts_with("CROTA") {
            return true;
        }
    }
    if kw.starts_with("CD") && kw.len() >= 4 {
        let rest = &kw[2..];
        if rest.contains('_') && rest.chars().all(|c| c.is_ascii_digit() || c == '_') {
            return true;
        }
        if rest.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    if kw.starts_with("PC") && kw.len() >= 4 {
        let rest = &kw[2..];
        if rest.contains('_') && rest.chars().all(|c| c.is_ascii_digit() || c == '_') {
            return true;
        }
        if rest.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    if (kw.starts_with("PV") || kw.starts_with("PS")) && kw.len() >= 4 {
        let rest = &kw[2..];
        if rest.contains('_') && rest.chars().all(|c| c.is_ascii_digit() || c == '_') {
            return true;
        }
    }
    if kw.starts_with("PROJP") {
        return true;
    }
    if kw.starts_with("AMDX") || kw.starts_with("AMDY") {
        return true;
    }
    if kw.starts_with("PPO") && kw.len() >= 4 {
        let rest = &kw[3..];
        if rest.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    if kw.len() >= 5 && kw.starts_with('C') && kw.as_bytes()[1].is_ascii_digit() {
        let suffix = &kw[2..];
        if suffix.starts_with("VAL")
            || suffix.starts_with("PIX")
            || suffix.starts_with("ELT")
            || suffix.starts_with("YPE")
            || suffix.starts_with("NIT")
        {
            return true;
        }
    }

    false
}

/// Compare two FitsValues for "similarity" (C library semantics).
///
/// For strings: case-insensitive comparison ignoring trailing spaces.
/// For floats: C `astEQUAL` tolerance — see [`floats_similar`].
/// For integers/logicals: exact equality.
fn values_similar(a: &FitsValue, b: &FitsValue) -> bool {
    match (a, b) {
        (FitsValue::String(sa), FitsValue::String(sb)) => {
            sa.trim_end().eq_ignore_ascii_case(sb.trim_end())
        }
        (FitsValue::Float(fa), FitsValue::Float(fb)) => floats_similar(*fa, *fb),
        (FitsValue::Int(ia), FitsValue::Int(ib)) | (FitsValue::KInt(ia), FitsValue::KInt(ib)) => {
            ia == ib
        }
        (FitsValue::Logical(la), FitsValue::Logical(lb)) => la == lb,
        (FitsValue::ComplexFloat(ra, ia), FitsValue::ComplexFloat(rb, ib)) => {
            floats_similar(*ra, *rb) && floats_similar(*ia, *ib)
        }
        (FitsValue::ComplexInt(ra, ia), FitsValue::ComplexInt(rb, ib)) => ra == rb && ia == ib,
        _ => false,
    }
}

/// Check if two floats are "the same value" for comment-preservation.
///
/// Mirrors C's `EQUAL` macro (object.h ~244, fitschan.c ~1389):
///   `fabs(a - b) <= 1.0E5 * max((|a| + |b|) * DBL_EPSILON, DBL_MIN)`
///
/// This is what C's `SetFits` (fitschan.c:15648) compares against
/// when deciding whether to preserve an existing card's comment on
/// overwrite. The 1e5 multiplier on machine epsilon gives roughly
/// `1.1e-11 * (|a| + |b|)` of absolute tolerance, generous enough
/// to absorb the kind of FP residue a `pixel → IWC → spectral`
/// re-derivation can accumulate (~5e-9 relative on a 1.4204e9 Hz
/// CRVAL, for instance).
fn floats_similar(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    let scale = (a.abs() + b.abs()) * f64::EPSILON;
    let tol = 1.0e5 * scale.max(f64::MIN_POSITIVE);
    (a - b).abs() <= tol
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_header() -> &'static str {
        "SIMPLE  =                    T / Fits standard\n\
         BITPIX  =                  -32 / Bits per pixel\n\
         NAXIS   =                    2 / Number of axes\n\
         NAXIS1  =                  100 / Length of axis 1\n\
         NAXIS2  =                  200 / Length of axis 2\n\
         CRVAL1  =     266.416666666667 / Reference value\n\
         CTYPE1  = 'RA---TAN'           / Axis type\n\
         END"
    }

    #[test]
    fn from_header_parses_cards() {
        let list = CardList::from_header(sample_header()).unwrap();
        // END is stored as an ordinary commentary card, so 8 cards total.
        assert_eq!(list.len(), 8);
        assert!(!list.is_empty());
    }

    #[test]
    fn find_locates_keyword_case_insensitive() {
        let list = CardList::from_header(sample_header()).unwrap();
        let card = list.find("naxis").unwrap();
        assert_eq!(card.keyword, "NAXIS");
        assert_eq!(card.value, FitsValue::Int(2));

        let card = list.find("CTYPE1").unwrap();
        assert_eq!(card.value, FitsValue::String("RA---TAN".into()));
    }

    #[test]
    fn find_returns_none_for_missing() {
        let list = CardList::from_header(sample_header()).unwrap();
        assert!(list.find("MISSING").is_none());
    }

    #[test]
    fn put_with_overwrite_replaces() {
        let mut list = CardList::from_header(sample_header()).unwrap();
        let new_card = FitsCard {
            keyword: "NAXIS".to_string(),
            value: FitsValue::Int(3),
            comment: Some("Changed".to_string()),
            used: false,
            flags: crate::fitschan::card::CardFlags::default(),
        };
        list.put(new_card, true);
        let card = list.find("NAXIS").unwrap();
        assert_eq!(card.value, FitsValue::Int(3));
        assert_eq!(card.comment, Some("Changed".to_string()));
    }

    #[test]
    fn put_without_overwrite_inserts() {
        let mut list = CardList::from_header(sample_header()).unwrap();
        let original_len = list.len();
        list.set_position(2);
        let new_card = FitsCard {
            keyword: "NEWKEY".to_string(),
            value: FitsValue::Int(99),
            comment: None,
            used: false,
            flags: crate::fitschan::card::CardFlags::default(),
        };
        list.put(new_card, false);
        assert_eq!(list.len(), original_len + 1);
        // Inserted at position 2.
        assert_eq!(list.cards[2].keyword, "NEWKEY");
    }

    #[test]
    fn delete_removes_card() {
        let mut list = CardList::from_header(sample_header()).unwrap();
        let original_len = list.len();
        assert!(list.delete("NAXIS1"));
        assert_eq!(list.len(), original_len - 1);
        assert!(list.find("NAXIS1").is_none());
    }

    #[test]
    fn delete_returns_false_for_missing() {
        let mut list = CardList::from_header(sample_header()).unwrap();
        assert!(!list.delete("MISSING"));
    }

    #[test]
    fn get_int_extracts_value() {
        let list = CardList::from_header(sample_header()).unwrap();
        assert_eq!(list.get_int("NAXIS"), Some(2));
        assert_eq!(list.get_int("NAXIS1"), Some(100));
        assert_eq!(list.get_int("MISSING"), None);
    }

    #[test]
    fn get_float_extracts_value() {
        let list = CardList::from_header(sample_header()).unwrap();
        let crval = list.get_float("CRVAL1").unwrap();
        assert!((crval - 266.416666666667).abs() < 1e-10);
        // Integer card should also work via conversion.
        assert_eq!(list.get_float("NAXIS"), Some(2.0));
    }

    #[test]
    fn get_string_extracts_value() {
        let list = CardList::from_header(sample_header()).unwrap();
        assert_eq!(list.get_string("CTYPE1"), Some("RA---TAN".to_string()));
        assert_eq!(list.get_string("NAXIS"), None); // not a string
    }

    #[test]
    fn get_logical_extracts_value() {
        let list = CardList::from_header(sample_header()).unwrap();
        assert_eq!(list.get_logical("SIMPLE"), Some(true));
        assert_eq!(list.get_logical("NAXIS"), None); // not a logical
    }

    #[test]
    fn find_next_advances_position() {
        let mut list = CardList::from_header(sample_header()).unwrap();
        list.rewind();
        // Find NAXIS (should be at index 2).
        let card = list.find_next("NAXIS").unwrap();
        assert_eq!(card.value, FitsValue::Int(2));
        // Position should be past NAXIS (index 3).
        assert_eq!(list.position(), 3);
        // find_next again from current position should not find plain NAXIS.
        assert!(list.find_next("NAXIS").is_none());
    }

    #[test]
    fn rewind_resets_position() {
        let mut list = CardList::from_header(sample_header()).unwrap();
        list.set_position(5);
        assert_eq!(list.position(), 5);
        list.rewind();
        assert_eq!(list.position(), 0);
    }

    #[test]
    fn at_eof() {
        let mut list = CardList::from_header(sample_header()).unwrap();
        assert!(!list.at_eof());
        list.set_position(list.len());
        assert!(list.at_eof());
    }

    #[test]
    fn delete_at_position() {
        let mut list = CardList::from_header(sample_header()).unwrap();
        let original_len = list.len();
        // Position 0 is SIMPLE.
        list.set_position(0);
        list.delete_at_position();
        assert_eq!(list.len(), original_len - 1);
        // First card should now be BITPIX.
        assert_eq!(list.cards[0].keyword, "BITPIX");
    }

    #[test]
    fn iter_yields_all_cards() {
        let list = CardList::from_header(sample_header()).unwrap();
        let keywords: Vec<&str> = list.iter().map(|c| c.keyword.as_str()).collect();
        assert_eq!(
            keywords,
            vec![
                "SIMPLE", "BITPIX", "NAXIS", "NAXIS1", "NAXIS2", "CRVAL1", "CTYPE1", "END"
            ]
        );
    }

    #[test]
    fn from_header_skips_empty_lines() {
        let text = "NAXIS   =                    2 / Number of axes\n\n\nEND\n";
        let list = CardList::from_header(text).unwrap();
        assert_eq!(list.len(), 2);
    }

    // -----------------------------------------------------------------
    // push_astwarn (C `Warn`, fitschan.c:34990-35167)
    // -----------------------------------------------------------------

    fn int_card(keyword: &str, value: i64) -> FitsCard {
        FitsCard {
            keyword: keyword.to_string(),
            value: FitsValue::Int(value),
            comment: None,
            used: false,
            flags: crate::fitschan::card::CardFlags::default(),
        }
    }

    fn keywords_of(list: &CardList) -> Vec<String> {
        list.iter().map(|c| c.keyword.clone()).collect()
    }

    fn astwarn_values(list: &CardList) -> Vec<String> {
        list.iter()
            .filter(|c| c.keyword == "ASTWARN")
            .map(|c| match &c.value {
                FitsValue::String(s) => s.clone(),
                other => panic!("ASTWARN card holds {other:?}"),
            })
            .collect()
    }

    /// C `Warn` writes each card with `astSetFitsS( ..., overwrite = 0 )`,
    /// which `InsCard` inserts in front of the current card, leaving that card
    /// current. So the block lands before the card at the current position,
    /// and the position follows its card down the list.
    #[test]
    fn push_astwarn_inserts_before_the_current_card() {
        let mut list = CardList::new();
        list.push(int_card("NAXIS1", 512));
        list.push(int_card("NAXIS2", 512));
        list.set_position(1); // current card = NAXIS2

        list.push_astwarn("short warning");

        assert_eq!(
            keywords_of(&list),
            vec!["NAXIS1", "ASTWARN", "ASTWARN", "ASTWARN", "NAXIS2"]
        );
        // The current card is still NAXIS2, now at index 4.
        assert_eq!(list.position(), 4);
    }

    /// Consecutive warnings chain into a growing run of blocks in front of the
    /// same card, because each insertion carries the position with it.
    #[test]
    fn consecutive_push_astwarn_calls_chain_before_the_same_card() {
        let mut list = CardList::new();
        list.push(int_card("NAXIS1", 512));
        list.push(int_card("NAXIS2", 512));
        list.set_position(1);

        list.push_astwarn("first warning");
        list.push_astwarn("second warning");

        assert_eq!(
            astwarn_values(&list),
            vec![" ", "first warning", " ", " ", "second warning", " "]
        );
        assert_eq!(keywords_of(&list).last().unwrap(), "NAXIS2");
        assert_eq!(list.position(), 7);
    }

    /// C dedups on the whole block: the first line is searched for anywhere in
    /// the list, then every later line must be on the *next* card
    /// (fitschan.c:35088-35118). An identical block is not repeated.
    #[test]
    fn push_astwarn_skips_an_identical_existing_block() {
        let text = "The FITS keyword name ('sys.') contains an illegal character ('.').";
        let mut list = CardList::new();
        list.push(int_card("NAXIS1", 512));
        list.set_position(0);
        list.push_astwarn(text);
        let after_first = astwarn_values(&list);

        list.push_astwarn(text);

        assert_eq!(astwarn_values(&list), after_first);
    }

    /// Two blocks whose first lines differ but whose *second* lines are
    /// identical must both be stored: the adjacency requirement means matching
    /// only the shared trailing line is not a match.
    #[test]
    fn push_astwarn_stores_blocks_that_share_a_trailing_line() {
        let first = "Projection parameter PV2_1 found, but is not used by -TAN projections.";
        let second = "Projection parameter PV2_2 found, but is not used by -TAN projections.";
        let mut list = CardList::new();
        list.push(int_card("NAXIS2", 512));
        list.set_position(0);

        list.push_astwarn(first);
        list.push_astwarn(second);

        let values = astwarn_values(&list);
        assert_eq!(
            values.len(),
            8,
            "two 4-card blocks expected, got {values:?}"
        );
        assert!(
            values[1].contains("PV2_1") && values[5].contains("PV2_2"),
            "both blocks must be stored, got {values:?}"
        );
        assert_eq!(values[2], values[6], "shared trailing line");
    }

    /// The dedup probe rewinds to search but must leave the position where it
    /// found it, mirroring `astSetCard( this, icard )` at fitschan.c:35120.
    #[test]
    fn push_astwarn_dedup_probe_does_not_move_the_position() {
        let text = "a repeated warning";
        let mut list = CardList::new();
        list.push(int_card("NAXIS1", 512));
        list.push(int_card("NAXIS2", 512));
        list.set_position(0);
        list.push_astwarn(text);
        let pos = list.position();

        // Second call dedups and returns without writing.
        list.push_astwarn(text);

        assert_eq!(list.position(), pos);
    }
}
