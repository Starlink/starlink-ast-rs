# FitsChan: C-parity audit

Date: 2026-10-10.
Scope: the FitsChan class (`fitschan.c`) as a Channel and as a store of FITS header cards: its attributes, its public functions, the Channel methods it overrides, and its life as an Object.
The FITS encodings themselves (FITS-WCS, FITS-PC, FITS-IRAF, FITS-AIPS, FITS-AIPS++, FITS-CLASS, DSS, NATIVE) are out of scope except where an attribute controls them: their read and write paths are byte-compared against C by the 194 wcsconv cases (`tests/test_wcsconv.rs`) and catalogued in `docs/issues/fitschan-c-port-gaps.md` and `docs/issues/fset_to_store-port-gaps.md`.
`ast-capi` is out of scope: no string-attribute dispatch exists, so the audit asks for a typed accessor for each attribute rather than C's `GetAttrib` and friends.
The four Channel-level gaps PLAN.md §5 deferred to this audit are included (CH-1 to CH-4).

The audit was made against C on branch `u/timj/norm-unit` and Rust at `d8ce484e`.

## Method

1. Every attribute named in C's `GetAttrib` was listed with its default (`astMAKE_GET`) and the places C reads it, and checked for a Rust accessor and for what the port does without one.
2. Every public function in `fitschan.h` (outside the `astCLASS` protected blocks) was checked for a Rust counterpart.
3. Every `vtab` entry `astInitFitsChanVtab` assigns was classified: the FitsChan methods, the Channel methods it overrides (`Read`, `Write`, `GetSkip`, `GetFull`, the `Write*` seams, `GetNextData`, `SetSourceFile`), and the Object methods (`Dump`, `Copy`, `Delete`).
4. The behaviour of the card functions the port has was compared by reading and by paired C and Rust probes; that part is in [Card functions](#card-functions).

## Summary

The FITS encodings are in good shape; the gaps are in the surface around them, and in the behaviour of the card functions the port has.
The card cursor is the largest: C's put, set, find, get, read and delete all act on the current card, and the port's overwrite replaces the first card with the same keyword, `find_fits` wraps round and takes no templates, the getters never move the cursor, and a read leaves the cursor inside the header (CA-1 to CA-11).
The getters also do not convert between value types as C's do, and do not skip the cards a read consumed, so a write after a read can choose a different encoding from C's (CA-6, CA-9).
63 findings: 12 HIGH, 30 MED, 19 LOW, and 2 gated on FitsTable.
Of C's 25 attributes the port exposes 8, and only Encoding has all four of get, set, test and clear.
The port has no way to drive or inspect the card cursor (`Card`, `CardName`, `CardType`, `CardComm`), no `astTestFits`, `astPurgeWCS`, `astRetainFits` or `astShowFits`, no complex, continued-string or explicitly 64-bit card values, no source or sink, and a FitsChan is not an Object that can be dumped.
Each "behaves as the default" entry below was checked by reading the port's code path unless it says PROBED; SipOK was the one such entry that did not hold when probed.
Where an attribute is missing the port mostly behaves as C does at the attribute's default, so existing callers get C's results and cannot change the default.
The exception is SipOK (FA-9): the port ties it to the encoding where C does not, and writes headers for SIP-distorted FrameSets that C refuses or writes differently.

## Progress

| IDs | Status | Evidence |
|---|---|---|
| FA-1, FA-2, FA-4, CA-1, CA-2, CA-3, CA-4, CA-5, CA-8, CA-10, CA-11, CA-15, CA-16, CA-18, CA-23 | FIXED 2026-10-10: the current card is C's: `card`, `set_card`, `clear_card` (replacing `rewind`), `test_card`, `card_name`, `card_type`, `card_comm` and `nkey`; overwriting replaces the current card and keeps its comment when the new one is blank; `put_cards` replaces the cards; `find_fits` searches forward with `Match` templates and formats with FitsDigits; the getters find their card as `SearchCard` does; a read of an encoding other than NATIVE happens only from the first card and leaves the FitsChan at end-of-file; setter names are read as `Split` reads them; `set_fits_cm` stores a blank keyword; `del_fits` does nothing at end-of-file; a complex integer's imaginary part ends in column 51 | `tests/test_fitschan_cursor_oracle.rs`: 20 scripts of card operations, each step's result and the whole FitsChan after it, byte-exact against C from `tools/oracle_fixtures/fitschan_cursor_probe.c` |
| CA-6, CA-7, CA-25, FF-7, FF-8, FF-9, FF-10 | FIXED 2026-10-10: the getters convert between types as `CnvType` does (fixed upstream in 4d24a55c, see c-library-quirks.md) and report AST__FTCNV and AST__FUNDEF; they take `Option<&str>`, `None` being the current card; a string keeps its trailing spaces until `get_fits_s` strips those of a short one; `get_fits_k` / `set_fits_k`, `_cn`, `_cf` and `_ci` added, and integers parsed as C's AST__KINT are `FitsValue::KInt` | the `conversions` script: 25 cards of every type through all 8 getters; `typed_setters` and `current_card_values` |
| CA-9, CA-19, FA-6, FF-1, FF-2, FF-3 | FIXED 2026-10-10: the cards a read consumes are flagged and hidden as C's are, so the getters, `find_fits`, `ncard`, `nkey` and the encoding guess pass over them; the encoding guess is C's `GetEncoding`, `KeyFields` templates and all; a NATIVE read starts from the current card; Clean added, under which a failed foreign read removes the cards it looked at and no `ASTWARN` cards are written; `purge_wcs`, `retain_fits` and `test_fits` added; native keywords are numbered from a sequence kept by the FitsChan, as C's `keyseq`, skipping any keyword a card holds (fixed upstream in 2aefc460, see c-library-quirks.md) | the `guess_*`, `native_*`, `purge_wcs`, `retain_survives_read`, `failed_read_*`, `clean_suppresses_warnings` and `test_fits` scripts |
| CA-12, CA-13, CA-14, CA-17 | FIXED 2026-10-11: a card's value is typed as C's `Split` types it: any case-insensitive abbreviation of YES, NO, TRUE or FALSE is a logical; numbers are read as `sscanf` reads `%d`, `%PRId64` and `%lf`, so a hexadecimal float is accepted, a 64-bit integer out of range is clamped and a parenthesised complex value is rejected; a value that cannot be typed is stored as a commentary card holding the keyword, after a `badkeyvalue` ASTWARN block; `END` with a value is an ordinary keyword; an `=` in the keyword is AST__BDFTS. A string value too long for its card is AST__BDFTS when the card is formatted (`find_fits`, `to_header`), and a long comment starts left of column 32 as C's `FormatCard` places it | the `split_logicals`, `split_numbers`, `split_bad_value`, `put_cards_bad_value`, `name_checks`, `string_*`, `continue_69_fails` and `long_comment` scripts |
| CA-20 | FIXED 2026-10-11: CDMatrix is unset until set, and its default is C's `GetCDMatrix`: true when a `CD%1d_%1d` card that a read has not consumed is present; `test_cd_matrix` and `clear_cd_matrix` added, and a FITS-WCS write uses the value at the time of the write | the `cd_matrix_default` and `cd_matrix_after_read` scripts |
| FA-9 | FIXED 2026-10-10: SipOK added (get, set, test, clear) and passed to `make_int_world` for FITS-WCS only, as C does since upstream bdc6336c (see c-library-quirks.md); other encodings take the SipOK=0 linear approximation. The probe also showed FITS-WCS PC cards written with a blank comment where C writes "Transformation matrix element", PC elements dropped by a `1e-15` test where C uses `astEQUAL`, and `AP_ORDER` / `BP_ORDER` commented "SIP max order" where C writes "SIP inverse max order"; all three fixed. The wcsconv corpus could not see the comments, because its writes overwrite cards that already have them | `tests/test_fitschan_sipok_oracle.rs`: `sip.head` written with every encoding and SipOK unset, false and true, byte-exact against C from `tools/oracle_fixtures/fitschan_sipok_probe.c` |
| — | NEW, NOT FIXED: on `sip.head`, whose PC matrix has shear, C's FITS-AIPS, FITS-AIPS++ and FITS-CLASS writers refuse (`astWrite` returns 0) where the port's write a header, FITS-AIPS and FITS-CLASS dropping the rotation; for DSS, C returns 0 where the port raises an error. The linear approximation C and the port both write for SipOK=0 is 4.95 arcsec (about 4 pixels) out at the corner of the image yet passes the FitsTol test of 0.1 pixel, which is worth understanding in C | `EXCLUDED` in `tests/test_fitschan_sipok_oracle.rs`, gated by `excluded_encodings_still_differ` |
| FA-6 | NOT PORTED: a NATIVE read that fails with Clean set removes in C the cards `GetNextData` had read before the failure (fitschan.c:25960-25974); the port parses the whole `BEGAST` to `ENDAST` span before building the object, so it does not know how far C's reader would have got, and leaves every card | would be exposed by a malformed native block read with Clean set |

## Attributes

C default in brackets.
"Rust" lists the accessors that exist.

| ID | Attribute | C reads it in | Rust | Port without it | Severity |
|---|---|---|---|---|---|
| FA-1 | `Card` (cursor, 1-based; clear = rewind) | `astSetCard` / `astGetCard` / `astClearCard`, 21 uses | `rewind` only; `CardList::position` is private | Clients cannot read or move the cursor, so they cannot iterate cards, insert at a chosen place, or delete a chosen card, which C programs and pyast do routinely | HIGH |
| FA-2 | `CardName`, `CardType`, `CardComm` (read-only, current card) | `GetCardName`, `GetCardType`, `GetCardComm` | none | No way to inspect the current card other than `find_fits` text | HIGH |
| FA-3 | `Ncard` (read-only) | `GetNcard` | `ncard()` | C excludes cards flagged as used; whether `ncard` does is in the card-function section | — |
| FA-4 | `Nkey` (read-only, distinct keywords) | `GetNkey` | none | Missing | LOW |
| FA-5 | `Encoding` [guessed from the cards] | `GetEncoding` | get, set, test, clear | Complete | — |
| FA-6 | `Clean` [0] | `Read` (keep used cards on failure), `PurgeWCS`, `Warn` | none | The port's failed read behaves as Clean=0 (cards not removed); `Warn` with Clean=1 adds no ASTWARN cards in C, which the port cannot request | MED |
| FA-7 | `Iwc` [0] | `WcsFrame` (fitschan.c:38161): add an IWC Frame to a FrameSet read from FITS-WCS | none | Never adds the IWC Frame, as C's default; callers that want intermediate world coordinates cannot get them | MED |
| FA-8 | `PolyTan` [-1: use PV on TAN only when it is not SCAMP-style] | `GetUsedPolyTan` (fitschan.c:14026) | none; `spec_trans::get_used_poly_tan` hardcodes -1 | Behaves as the default; a header that needs PolyTan=0 or 1 cannot be read as intended | MED |
| FA-9 | `SipOK` [1] | `WcsFromStore` → `MakeIntWorld` (fitschan.c:2990), for every encoding | none; `fset_to_store/mod.rs:260` passes true only for FITS-WCS | **Wrong output, PROBED.** C passes SipOK (default 1) to `MakeIntWorld` whatever the encoding; the port passes false for every encoding but FITS-WCS. Reading `wcsconv/inputs/sip.head` and writing it: with FITS-IRAF and FITS-AIPS C writes nothing (`astWrite` returns 0) where the port writes a linear approximation; with FITS-PC C writes CRPIX 127.993456 and CTYPE `RA---TAN-SIP`, the port CRPIX 129.355915 and `RA---TAN`. SipOK=0 is also unavailable. C's FITS-PC header names SIP but carries no SIP coefficients, which looks like a C defect to take upstream before mirroring | HIGH |
| FA-10 | `SipReplace` [1] | `fitschan.c:29679` | set only | Missing get, test and clear | LOW |
| FA-11 | `DefB1950` [1 unless Encoding is FITS-WCS] | `fitschan.c:39510` | none public; `mod.rs:820-870` passes false for FITS-WCS and true for the other encodings | Behaves as the default (read); cannot be overridden | LOW |
| FA-12 | `CarLin` [0] | `fitschan.c:32265`, `:35936` | set, get | Missing test and clear | LOW |
| FA-13 | `CDMatrix` [0] | writers | set, get | Missing test and clear | LOW |
| FA-14 | `FitsDigits` [`AST__DBL_DIG`] | formatter | set, get | Missing test and clear | LOW |
| FA-15 | `FitsRounding` [10] | formatter | set, get | Missing test and clear; C clamps a negative value to 0 (`astMAX(0,value)`), whether the port does is in the card-function section | LOW |
| FA-16 | `FitsTol` [0.1] | `fset_to_store` | set, get | Missing test and clear; C clamps negative to 0 | LOW |
| FA-17 | `FitsAxisOrder` [`<auto>`] | `fitschan.c:5398`, `:11223` | none | Always `<auto>`; a caller cannot order the FITS axes to match an output Frame (fset_to_store gap doc) | MED |
| FA-18 | `AltAxes` [ALL] | `WcsFromStore` (fitschan.c:12257) | none | Always ALL; IDENT and NONE (which limit or suppress alternate descriptions) unavailable | MED |
| FA-19 | `IgnoreBadAlt` [0] | `WcsToStore` (fitschan.c:12057) | none | Behaves as 0 (read: an error building an alternate Frame propagates out of `decode_wcs_with_alts_inner`, wcs_read.rs:525-580, as C's `AddFrame` does with `ignorebad` 0); a header with one unreadable alternate description cannot be read for its primary one | MED |
| FA-20 | `Warnings` [`BadKeyName BadKeyValue Tnx Zpx BadPV BadCel BadMat BadCTYPE`] | `Warn` (fitschan.c:35302) | none; the default list is hardcoded at each warning site | Issues exactly the default conditions; the other nine (`BadAlt`, `BadLat`, `BadVal`, `Distortion`, `NoCTYPE`, `NoEquinox`, `NoLatPole`, `NoLonPole`, `NoRadesys`) can never be requested, and none can be silenced | MED |
| FA-21 | `AllWarnings` (read-only) | `GetAllWarnings` | none | Missing | LOW |
| FA-22 | `TabOK` [0], `ForceTab` [0] | the `-TAB` paths | none | `-TAB` is unported (FitsTable); the port behaves as TabOK=0 | gated on FitsTable |

## Public functions

| ID | C | Rust | Note | Severity |
|---|---|---|---|---|
| FF-1 | `astTestFits` | none | Whether a keyword exists and whether it has a value; `find_fits` gives existence only | HIGH |
| FF-2 | `astPurgeWCS` | none | Delete every WCS card (C reads every object with Clean=1 and discards them); used before writing new WCS into an existing header | HIGH |
| FF-3 | `astRetainFits` | none | Keep the current card when a read would consume it | MED |
| FF-4 | `astShowFits` | none; `to_header` returns the text | `to_header` appends END, which `astShowFits` does not; a convenience | LOW |
| FF-5 | `astReadFits` | none | Call the source function now rather than on first use; needs FC-1 | MED |
| FF-6 | `astWriteFits` | none | Write the cards to the sink and empty the FitsChan; needs FC-1 | MED |
| FF-7 | `astGetFitsCF`, `astSetFitsCF` | none | Complex floating values | MED |
| FF-8 | `astGetFitsCI`, `astSetFitsCI` | none | Complex integer values | MED |
| FF-9 | `astGetFitsCN`, `astSetFitsCN` | none | CONTINUE (long) strings; how `get_fits_s` treats CONTINUE cards is in the card-function section | MED |
| FF-10 | `astGetFitsK`, `astSetFitsK` | `get_fits_i` / `set_fits_i` take `i64` | The port merges I and K; C's I is 32-bit and reports overflow, whether the port does is in the card-function section | LOW |
| FF-11 | `astGetTables`, `astPutTable`, `astPutTables`, `astRemoveTables`, `astTableSource`, `astSetTableSource` | none | FitsTable is unported | gated on FitsTable |
| FF-12 | `astFitsChan(source, sink, options)` | `FitsChan::new`, `from_header` | No source or sink function, so no lazy read of cards and no write on deletion | see FC-1 |

The Rust functions with C counterparts are `put_fits`, `put_cards`, `find_fits`, `del_fits`, `empty_fits`, `ncard`, `rewind`, `get_fits_i`, `get_fits_f`, `get_fits_s`, `get_fits_l`, `set_fits_i`, `set_fits_f`, `set_fits_s`, `set_fits_l`, `set_fits_u` and `set_fits_cm`.

## Channel and Object behaviour

| ID | C | Rust | Finding | Severity |
|---|---|---|---|---|
| FC-1 | Source and sink functions, `SourceFile`, `SinkFile` (fitschan.c `ReadFromSource`, `SetSourceFile` override, `Delete` writes to the sink) | none | Cards come only from `from_header` / `put_fits` and leave only through `to_header`. C reads a source lazily on first use and writes the cards to the sink when the FitsChan is deleted or `astWriteFits` is called; `SourceFile` reads a text file of cards at once | MED |
| FC-2 | `GetSkip` override: Skip defaults to 1 | none | The port's Native read does not consult Skip at all. Native output matches C at the defaults (probed: a FrameSet written with Encoding=NATIVE gives C's 43 cards exactly) | LOW |
| FC-3 | `GetFull` override: Full defaults to -1 | none | As FC-2; output with Full or Comment set is unverified | LOW |
| CH-1 | FitsChan `Dump` writes its cards and attributes; a FitsChan can be read back from a Channel | not an `AnyObject` variant, and does not implement `Clone` | A FitsChan cannot be copied (C's `astCopy`), dumped, read back from a Channel, or held by a KeyMap | MED |
| CH-2 | Object `ID` attribute | absent everywhere | A dump never carries it, for any class | MED |
| CH-3 | `NativeChannel` honours `Skip` and `Strict`, and channel.c `Read` reads one object at a time | stored but not acted on; the whole source is read at once | Exposed by a source holding two objects, or data between them | MED |
| CH-4 | With `Full` above zero C writes unset attributes that are not helpful as comments | the native writer does not, for any class | Exposed by any dump with Full=1 | LOW |

## Card functions

The card functions the port has were compared with C by reading both and by paired probes: C programs linked against the reference build, and a scratch crate using only the public `ast_core` API, kept outside the repository.
The probes covered 31 card forms through `astPutFits`, cursor and overwrite sequences, `astPutCards`, `astFindFits` templates, 15 keywords through each of the four typed getters, the typed setters, 20 floating values at 9 FitsDigits and 2 FitsRounding settings, 9 headers for the encoding guess, and a FITS-WCS read followed by writes.
With no `Card` accessor in the port (FA-1), its cursor was observed by inserting a marker card with `put_fits(…, false)` after each operation.
Two HIGH findings (CA-1, CA-4) were re-checked by reading both sources after the probes.

| ID | Severity | C | Rust | Finding | Evidence |
|---|---|---|---|---|---|
| CA-1 | HIGH | fitschan.c `InsCard` (`if(overwrite) DeleteCard`), `PutFits` :25450 | card_list.rs:157 `CardList::put` | With `overwrite`, C replaces the *current* card whatever its keyword, and appends at EOF. The port searches the whole list for the first card with the same keyword and replaces that, inserting at the cursor if there is none. | PROBED: cards A, B, C with the cursor on B; `put_fits("X=9", false)`, `put_fits("Y=8", true)`, `put_fits("A=7", true)`. C gives A, X, Y, A=7; the port gives A=7, X, Y, B, C. |
| CA-2 | HIGH | `MAKE_FSET` fitschan.c:15980-16010, `SetFitsU` :16049 | mod.rs:335-416 `set_fits_*` | The same overwrite defect in every `set_fits_*`. C also keeps the replaced card's comment when the new comment is blank. | PROBED: `A=1 / orig`, rewind, `set_fits_i("A", 2, "", true)`: C `A = 2 / orig`, the port `A = 2`. Then `set_fits_i("Q", 3, "", true)` on card 1: C replaces A with `Q = 3 / orig`; the port inserts Q and keeps A. |
| CA-3 | HIGH | fitschan.c:25363 `PutCards` | mod.rs:192 `put_cards` | C empties the FitsChan, splits the input into 80-character cards with no delimiters, and rewinds. The port splits on newlines, appends to the existing cards, does not rewind, and skips cards it cannot parse. | PROBED: a FitsChan holding `ZZZ=0`, then two padded 80-character cards concatenated. C gives AAA, BBB with Card 1; the port ZZZ, AAA (BBB lost). |
| CA-4 | HIGH | fitschan.c:14627 `FindFits`, :14852 `FindKeyCard`, :22615 `Match` | mod.rs:226 `find_fits` | C searches forward from the current card only, matches `Match` templates (`%d`, `%1d`, `%f`, `%0c`, …), treats `%f` as the current card, ignores trailing spaces in the template, and leaves the cursor at EOF when nothing is found. The port compares the exact keyword, wraps round to the start, and restores the cursor on failure. | PROBED: cards A1, B, A2, CRVAL1. `A%d`, `CRVAL%1d`, `%f` and `"B   "` are found by C and not by the port. With the cursor on A2, `find("B")` fails in C (Card at EOF) and succeeds in the port. |
| CA-5 | MED | fitschan.c:14830 (`FormatCard` with FitsDigits) | mod.rs:233, :249 (`.format()`, 15 digits) | `find_fits` formats the card it returns with 15 digits rather than FitsDigits. | PROBED: `V = 266.41666666666703`: C returns `266.41666666666703`, the port `266.416666666667` (its `to_header` gives C's text). |
| CA-6 | HIGH | `MAKE_FGET` fitschan.c:15412, `SearchCard` :27294, `CnvValue` :7427 | mod.rs:311-327, card_list.rs:257-290 | C's getters convert between types: integer to string or logical, string `'1'` to integer, floating to string or logical, logical to integer, floating or string. The port returns `None` for every cross-type request but integer and floating. | PROBED: `GetFitsS(I=5)` C `"5"`, port `None`; `GetFitsI(S='1')` C 1, port `None`; `GetFitsL(I0=0)` C false, port `None`; `GetFitsF(SF='2.5')` C 2.5, port `None`. |
| CA-7 | MED | `MAKE_FGET` fitschan.c:15448-15452, `GetValue` | mod.rs:311-327 | C raises AST__FTCNV on an impossible conversion and AST__FUNDEF on an undefined value. The port returns `None` for both, as for an absent keyword. | PROBED: `GetFitsI(SN='abc')` C AST__FTCNV; `GetFits*(U =)` C AST__FUNDEF; the port `None` for both. |
| CA-8 | MED | `SearchCard` fitschan.c:27294, `FindKeyCard` | mod.rs:311 (`&self`), card_list.rs:178 `find` | C's getters move the cursor to the card found, or to EOF when the keyword is absent, and look at the card after the current one before searching from the start. The port's getters take `&self`, always return the first match, and never move the cursor. | PROBED (cursor): after `GetFitsI("DUP")` C's Card is the found card, after a failed get EOF; the port's cursor does not move. UNVERIFIED (value): with the cursor on the first of two DUP cards C should return the second; a C probe at that Card would settle it. |
| CA-9 | HIGH | `ignore_used` / `MoveCard`, `GetEncoding` fitschan.c:12559 | card_list.rs:178 `find`, guess_encoding.rs:27 | The getters and the encoding guess do not skip cards a read has consumed. After a FITS-WCS read C hides the WCS cards, so `GetFitsF(CRVAL1)` finds nothing and the guessed Encoding becomes NATIVE; the port still returns the value and FITS-WCS. A following write with Encoding unset therefore writes FITS-WCS in the port and a NATIVE block in C. | PROBED: an 18-card CD header, read, then `GetFitsF(CRVAL1)`: C not found, port `Some(10.0)`; Encoding C NATIVE, port FITS-WCS; a write gives C a 136-card native block and the port FITS-WCS cards. |
| CA-10 | HIGH | `Read` fitschan.c:25945 (`astSetCard(this, INT_MAX)`) | mod.rs:193-345 (`move_cursor_to_last_naxis`) | After a successful foreign-encoding read C leaves the cursor at EOF; the port leaves it on the last `NAXISn`, so a later `put_fits` or write lands inside the header. | PROBED: read the CD header, then `put_fits("MARK", false)`: the port puts MARK between NAXIS1 and NAXIS2; C's Card is EOF. |
| CA-11 | MED | `Read` fitschan.c:25923 (`if(!astTestCard(this))`) | mod.rs:193 | For encodings other than NATIVE, C reads only when the cursor is at the start and otherwise returns nothing with the cards unchanged; the port reads from any position. | PROBED: cursor on CRVAL1, then read: C returns NULL and keeps all 18 cards; the port returns an object and consumes the WCS cards. |
| CA-12 | MED | `Split` fitschan.c:32715 (`FullForm("YES NO TRUE FALSE", …)`) | card.rs:285 | C accepts any case-insensitive abbreviation of YES, NO, TRUE or FALSE as a logical value; the port only `T` and `F`. | PROBED: `LOGY = YES`, `LOGT = t`, `LOGN = no / x`: C T, T, F; the port AST__BDFTS. |
| CA-13 | MED | `Split` fitschan.c:32820-32842 | card.rs:365-372, mod.rs:177, :212, card_list.rs:803 | For a value it cannot type, C stores a blank commentary card with the keyword, after an ASTWARN block (`badkeyvalue`). `put_fits` returns AST__BDFTS and stores nothing; `put_cards` and `from_header` drop the line. | PROBED: `BAD = abc / bad`: C three ASTWARN cards and `BAD`; the port AST__BDFTS, or no cards through `put_cards`. |
| CA-14 | LOW | `Split` fitschan.c:32730-32790 | card.rs:300-312, :684 `try_parse_complex` | The port accepts a parenthesised complex value C rejects, and rejects a hexadecimal float (C 12.0 for `0x1.8p3`) and an integer above the 64-bit range (C clamps). | PROBED: `(1.0, 2.0)`, `0x1.8p3`, `123456789012345678901`. |
| CA-15 | MED | `MAKE_FSET` fitschan.c:15990 (`Split` of the name), `CheckFitsName` | mod.rs:335-416 | C truncates a setter's keyword to 8 characters, the rest becoming the comment, and checks it; the port writes the name unchanged, giving a malformed card. | PROBED: `set_fits_i("averylongname", 5, …)`: C `AVERYLON= 5 / gname`; the port `AVERYLONGNAME= 5`. |
| CA-16 | MED | `SetFitsCM` fitschan.c:16184 | mod.rs:418 `set_fits_cm` | C writes a commentary card with a blank keyword; the port writes `COMMENT`. | PROBED. |
| CA-17 | MED | `EncodeValue` (AST__BDFTS for a string too long) | card.rs:723 `format_string_value`, :416 | C reports a string value that cannot fit; the port writes a truncated card with no closing quote. | PROBED: a 70-character value. |
| CA-18 | MED | `DeleteCard`, `DelFits` fitschan.c:8694 | mod.rs:258 `del_fits` | At EOF `astDelFits` does nothing in C; the port returns AST__BDFTS. | PROBED. |
| CA-19 | MED | `GetEncoding` fitschan.c:12576-12660 | guess_encoding.rs:27-90, :143-150 | The encoding guess differs: the port's FITS-WCS test looks at axis 1 and the primary description only where C looks at any axis and version (`%d%0c`); it has a `C{m}VAL{i}`-style FITS-PC rule and a COMMENT `AST +++` NATIVE rule C lacks; it ignores C's `RADESYS` and `PV%d_%d` exclusions; it misses the AIPS spectral suffix `LSRK`. | PROBED, C / port: CD + RADECSYS + RADESYS: WCS / IRAF; CD + C1VAL1: IRAF / PC; CTYPE2 + CRVAL2: WCS / NATIVE; CRVAL1A: WCS / NATIVE; `VELOLSRK`: AIPS / WCS; PROJP1 + PV1_1: WCS / PC; C2PIX1: WCS / PC; `COMMENT AST +++`: WCS / NATIVE; CTYPE1 + CRPIX1: NATIVE / NATIVE. |
| CA-20 | MED | `GetCDMatrix` fitschan.c:12489 | mod.rs:139, :497, :537 | C's CDMatrix default is 1 when an unconsumed `CD%1d_%1d` card is present; the port's is always false. | PROBED: a `CD1_1` card with CDMatrix cleared: C 1, port false. |
| CA-21 | MED | FitsDigits fitschan.c:43280-43283, `EncodeValue` | mod.rs:471 (`clamp(1, 17)`) | C's FitsDigits is an unclamped integer: a negative value means at most that many digits, fewer if needed to fit 20 columns, and values above 17 print more digits. The port clamps to 1..=17. | PROBED: −5 gives C `1.2346E+8`, `0.33333`, `266.42`; 25 gives C 25 digits. |
| CA-22 | LOW | `EncodeValue` | format.rs:51 `format_significant_rounded` | At FitsDigits 1 or 0 the port prints 2 significant figures where C prints 1. | PROBED. |
| CA-23 | LOW | `EncodeValue` fitschan.c:10225 (width 21) | card.rs:490-506 (width 20) | The imaginary part of a complex integer ends one column early. | PROBED: `CI = 1 2`. |
| CA-24 | LOW | FitsTol fitschan.c:41978, FitsRounding :43327 | mod.rs:484, :527 | C clamps FitsTol and FitsRounding to at least 0; the port stores a negative value. | PROBED for FitsTol. |
| CA-25 | LOW | `GetFitsS` (trims strings of 8 characters or fewer when read) | card.rs:575-580 (trims when parsed) | The port strips trailing blanks from a short string when the card is stored, where C keeps them and strips them in `GetFitsS`; `GetFitsCN` would show the difference. | Partly PROBED: C `GetFitsCN(S8='ab  ')` gives `"ab  "`. |

The agent's remaining findings repeat the inventory: missing test and clear accessors (FA-10 to FA-16), DefB1950 (FA-11), and the missing functions (FF-1 to FF-10).
It also found that FitsChan does not implement `Clone`, C's `astCopy`; that belongs with CH-1.

Checked and matching:

- `put_fits` parsing and formatting, byte for byte: blank keyword, COMMENT, HISTORY, CONTINUE, `'O''Hara'`, a padded short string, a long string with trailing blanks, `T`, integers including `12345678901` and `+5`, the `1.5D3` exponent, complex floating `1.5 2.5`, complex integer, undefined values with and without a comment, no `=` in column 9, a lower-case keyword, `-0.0`, `END`, `/` inside a string, and a comment with no space after `/`.
- `put_fits(…, false)` inserts before the current card and leaves the cursor on the next.
- `find_fits` matches keywords case-insensitively, and `inc` moves past the card found.
- Floating formatting at FitsDigits 2 to 17 (17, 15, 8 and 5 probed) and FitsRounding 10 and 3, for 20 values covering exponent form, the `.0` suffix, 1e-300 and 1e17.
- String quoting, quote doubling, padding to 8 characters, a 68-character string filling the card, the comment at column 32 or after a long value, and truncation at column 80.
- Getters: floating to integer truncates (2.7 to 2, −2.5 to −2), integer to floating, short strings lose trailing blanks, `GetFitsS` does not join CONTINUE cards, and an absent keyword is not found.
- `ncard` excludes consumed cards.
- The cards left after a FITS-WCS read are the same, a FITS-WCS write puts its keywords where the original WCS cards were, and a successful write leaves the cursor at EOF.
- Defaults: Encoding NATIVE for an empty FitsChan, FitsDigits 17, FitsRounding 10, CarLin 0, SipReplace 1, FitsTol 0.1, CDMatrix 0 with no CD cards; Encoding's set, test and clear.

## Recommended order

1. **The card cursor, as C models it** (FA-1, FA-2, CA-1, CA-2, CA-4, CA-8, CA-10, CA-11, CA-3, CA-18). Put, set, find, get, read and delete all move or use the cursor, and the port gets most of them wrong; `Card`, `CardName`, `CardType` and `CardComm` come with it, and are what make the rest testable without a marker card.
2. **Consumed cards** (CA-9, FA-6, FF-2, FF-3). The getters and the encoding guess must skip what a read used, with Clean, `astPurgeWCS` and `astRetainFits` built on the same flags; CA-9 changes what a write after a read produces.
3. **Writing SIP-distorted FrameSets** (FA-9). Before porting C's SipOK handling, settle upstream whether C's FITS-PC header naming SIP without coefficients is a defect.
4. **Card values** (CA-6, CA-7, CA-12 to CA-17, CA-25, FF-1, FF-7 to FF-10). C's type conversions and errors, its value parsing and name handling, `astTestFits`, and the complex, CONTINUE and 64-bit accessors.
5. **The encoding guess and CDMatrix default** (CA-19, CA-20). Each changes which encoding or matrix form a write uses when the caller does not set it.
6. **Formatting** (CA-5, CA-21 to CA-23).
7. **The remaining attributes** (FA-4, FA-7, FA-8, FA-10 to FA-21, CA-24), each with get, set, test and clear.
8. **FitsChan as an Object** (CH-1 with `Clone`, FC-1 to FC-3, FF-4 to FF-6) and the Channel items (CH-2 to CH-4).
