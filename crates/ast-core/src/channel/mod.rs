//! Channel I/O abstraction for reading and writing AST objects.
//!
//! A [`Channel`] connects a [`Source`] (text line supplier) and a [`Sink`]
//! (text line consumer) to the serialization machinery, allowing AST objects
//! to be read from and written to external storage.
//!
//! The built-in [`NativeChannel`] uses the AST Native text format, and
//! [`MocChan`] the IVOA MOC serializations.

pub(crate) mod base;
pub mod mocchan;
pub mod native;
pub mod source_sink;

use std::sync::Arc;

use crate::error::AstResult;
use crate::keymap::KeyMap;
use crate::mappings::IntraTransform;
use crate::serial::AnyObject;

pub use mocchan::{MocChan, MocFormat};
pub use native::NativeChannel;
pub use source_sink::{FileSink, FileSource, StringSink, StringSource};

/// The attributes every Channel has, which control how objects are read
/// and written.
///
/// Each attribute is either set or unset, and an unset one takes the
/// default its getter states.
/// [`Channel::config`] and [`Channel::config_mut`] give a Channel's
/// attributes:
///
/// - [Comment](Self::comment): whether written text includes comments.
/// - [Full](Self::full): how much detail written text includes.
/// - [Indent](Self::indent): the indentation of nested objects in written
///   text.
/// - [ReportLevel](Self::report_level): which conditions met while reading
///   are reported.
/// - [Skip](Self::skip): whether data between objects are passed over when
///   reading.
/// - [Strict](Self::strict): whether a reported condition is an error
///   rather than a warning.
// C: the Channel class's attributes (channel.c). A Channel's dump records
// only those that are set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelConfig {
    comment: Option<bool>,
    full: Option<i32>,
    skip: Option<bool>,
    strict: Option<bool>,
    indent: Option<i32>,
    report_level: Option<i32>,
    /// The class's default `Skip`: true for a FitsChan (fitschan.c
    /// `GetSkip`), false otherwise.
    default_skip: bool,
    /// The class's default `Full`: -1 for a FitsChan (fitschan.c
    /// `GetFull`), 0 otherwise.
    default_full: i32,
}

impl ChannelConfig {
    /// The attributes of a FitsChan, whose `Skip` defaults to true and
    /// `Full` to -1.
    pub(crate) fn fitschan() -> Self {
        Self {
            default_skip: true,
            default_full: -1,
            ..Self::default()
        }
    }

    /// The `Comment` attribute: whether text written includes comments that
    /// describe each item.
    ///
    /// Default: true.
    ///
    /// A [`NativeChannel`] ends each line it writes with a comment saying
    /// what the item is.
    /// A [`FitsChan`] writing in the `NATIVE` encoding surrounds the object
    /// with banners of `COMMENT` cards and gives each card a comment.
    /// A [`MocChan`] ignores it.
    ///
    /// [`FitsChan`]: crate::fitschan::FitsChan
    #[doc(alias = "Comment")]
    pub fn comment(&self) -> bool {
        self.comment.unwrap_or(true)
    }

    /// Set the [Comment](Self::comment) attribute.
    pub fn set_comment(&mut self, comment: bool) {
        self.comment = Some(comment);
    }

    /// Whether the [Comment](Self::comment) attribute is set.
    pub fn test_comment(&self) -> bool {
        self.comment.is_some()
    }

    /// Clear the [Comment](Self::comment) attribute.
    pub fn clear_comment(&mut self) {
        self.comment = None;
    }

    /// The `Full` attribute: how much detail text written includes, -1, 0
    /// or 1.
    ///
    /// Default: 0, or -1 for a [`FitsChan`].
    ///
    /// Values that are set are always written.
    /// With Full -1, no default values are written.
    /// With Full 0, the default values that are useful to a reader are
    /// written as comments; in a NativeChannel's text these are lines that
    /// begin with `#`.
    /// With Full 1, every default value is written as a comment, and the
    /// class marker of each class that wrote no items is written too.
    ///
    /// A [`NativeChannel`] and a [`FitsChan`] writing in the `NATIVE`
    /// encoding use it; a [`MocChan`] ignores it.
    ///
    /// [`FitsChan`]: crate::fitschan::FitsChan
    #[doc(alias = "Full")]
    pub fn full(&self) -> i32 {
        self.full.unwrap_or(self.default_full)
    }

    /// Set the [Full](Self::full) attribute.
    /// Any positive value is stored as 1 and any negative one as -1.
    pub fn set_full(&mut self, full: i32) {
        self.full = Some(full.signum());
    }

    /// Whether the [Full](Self::full) attribute is set.
    pub fn test_full(&self) -> bool {
        self.full.is_some()
    }

    /// Clear the [Full](Self::full) attribute.
    pub fn clear_full(&mut self) {
        self.full = None;
    }

    /// The `Skip` attribute: whether a read passes over data between
    /// objects.
    ///
    /// Default: false, or true for a [`FitsChan`].
    ///
    /// When it is false, a [`NativeChannel`]'s source must hold nothing
    /// but object descriptions, comments and blank lines, and a read that
    /// meets anything else fails with [`ErrorCode::Badin`].
    /// When it is true, anything before the next object's `Begin` line is
    /// passed over.
    ///
    /// A FitsChan reading in the `NATIVE` encoding passes over the cards
    /// before the object's description when it is true; when it is false,
    /// any of those cards other than a commentary card is an error.
    /// A [`MocChan`] ignores it.
    ///
    /// [`FitsChan`]: crate::fitschan::FitsChan
    /// [`ErrorCode::Badin`]: crate::error::ErrorCode::Badin
    #[doc(alias = "Skip")]
    pub fn skip(&self) -> bool {
        self.skip.unwrap_or(self.default_skip)
    }

    /// Set the [Skip](Self::skip) attribute.
    pub fn set_skip(&mut self, skip: bool) {
        self.skip = Some(skip);
    }

    /// Whether the [Skip](Self::skip) attribute is set.
    pub fn test_skip(&self) -> bool {
        self.skip.is_some()
    }

    /// Clear the [Skip](Self::skip) attribute.
    pub fn clear_skip(&mut self) {
        self.skip = None;
    }

    /// The `Strict` attribute: whether a condition a read reports is an
    /// error rather than a warning.
    ///
    /// Default: false.
    ///
    /// When it is false, a read goes on after a condition the
    /// [ReportLevel](Self::report_level) selects, and the condition is
    /// added to the warnings that [`Channel::warnings`] returns.
    /// When it is true, such a condition makes a [`NativeChannel`]'s read
    /// fail with [`ErrorCode::Badin`].
    /// A NativeChannel reports an item it does not recognize in an object's
    /// description, such as an attribute added by a later version of AST or
    /// a misspelled one in a hand-edited description: by default the item
    /// is ignored, which leaves that attribute at its default value, and
    /// with Strict set the error names it.
    ///
    /// In a [`FitsChan`], a condition its Warnings attribute names, met by
    /// any method that reads or stores cards, is with Strict set the error
    /// [`ErrorCode::Badin`] at the point it is met: the method stops there,
    /// and no `ASTWARN` cards or warnings are stored for it.
    /// A FitsChan with Clean set, or a ReportLevel below 1, reports no
    /// conditions this way.
    /// A [`MocChan`] reports no conditions.
    ///
    /// [`FitsChan`]: crate::fitschan::FitsChan
    /// [`ErrorCode::Badin`]: crate::error::ErrorCode::Badin
    #[doc(alias = "Strict")]
    pub fn strict(&self) -> bool {
        self.strict.unwrap_or(false)
    }

    /// Set the [Strict](Self::strict) attribute.
    pub fn set_strict(&mut self, strict: bool) {
        self.strict = Some(strict);
    }

    /// Whether the [Strict](Self::strict) attribute is set.
    pub fn test_strict(&self) -> bool {
        self.strict.is_some()
    }

    /// Clear the [Strict](Self::strict) attribute.
    pub fn clear_strict(&mut self) {
        self.strict = None;
    }

    /// The `Indent` attribute: the number of spaces by which written text
    /// indents each level of nested objects.
    ///
    /// Default: 3.
    ///
    /// With Indent 0, or a negative Indent, nested objects are not
    /// indented, though every line still starts with one character, a
    /// space or the `#` of a default value.
    /// A [`NativeChannel`] uses it; a [`FitsChan`] and a [`MocChan`] ignore
    /// it.
    ///
    /// [`FitsChan`]: crate::fitschan::FitsChan
    #[doc(alias = "Indent")]
    pub fn indent(&self) -> i32 {
        self.indent.unwrap_or(3)
    }

    /// Set the [Indent](Self::indent) attribute.
    /// Any value is stored, a negative one included.
    pub fn set_indent(&mut self, indent: i32) {
        self.indent = Some(indent);
    }

    /// Whether the [Indent](Self::indent) attribute is set.
    pub fn test_indent(&self) -> bool {
        self.indent.is_some()
    }

    /// Clear the [Indent](Self::indent) attribute.
    pub fn clear_indent(&mut self) {
        self.indent = None;
    }

    /// The `ReportLevel` attribute: which conditions met while reading or
    /// writing an object are reported.
    ///
    /// Default: 1.
    ///
    /// The conditions it selects are reported as warnings or as errors, as
    /// [Strict](Self::strict) decides.
    /// It takes these values:
    ///
    /// - 0: report no conditions.
    /// - 1: report the conditions that change significant information, such
    ///   as an item in an object's description that a [`NativeChannel`] does
    ///   not recognize, which may have been added by a later version of AST.
    /// - 2: report those, and significant default values, such as the time
    ///   scale assumed when an object read names none.
    /// - 3: report those, and any other conditions of interest that have no
    ///   significant effect, such as the use of a synonym.
    ///
    /// The NativeChannel and the [`FitsChan`] report every condition they
    /// meet at level 1, so 1, 2 and 3 select the same conditions; a
    /// [`MocChan`] reports none.
    /// A FitsChan's conditions are those its
    /// [Warnings](crate::fitschan::FitsChan::warning_conditions) attribute
    /// selects; with ReportLevel 0 their `ASTWARN` cards are still stored.
    ///
    /// Other conditions stop a read altogether, and are always errors.
    ///
    /// [`FitsChan`]: crate::fitschan::FitsChan
    #[doc(alias = "ReportLevel")]
    pub fn report_level(&self) -> i32 {
        self.report_level.unwrap_or(1)
    }

    /// Set the [ReportLevel](Self::report_level) attribute.
    /// Any value is stored.
    pub fn set_report_level(&mut self, level: i32) {
        self.report_level = Some(level);
    }

    /// Whether the [ReportLevel](Self::report_level) attribute is set.
    pub fn test_report_level(&self) -> bool {
        self.report_level.is_some()
    }

    /// Clear the [ReportLevel](Self::report_level) attribute.
    pub fn clear_report_level(&mut self) {
        self.report_level = None;
    }
}

/// Provides the lines of text a Channel reads.
///
/// A Channel asks its source for one line at a time, and an implementation
/// connects it to wherever the text is held, such as a file, a database or
/// a network connection; [`StringSource`] and [`FileSource`] are ready-made
/// ones.
/// Any data the source needs, such as an open file, belongs in the type
/// that implements it.
///
/// A Channel and its clones share one source, so it must be `Send` for the
/// Channel to be.
pub trait Source: Send {
    /// Return the next line of text, without its line terminator, or `None`
    /// when there are no more lines.
    fn read_line(&mut self) -> Option<String>;
}

/// Receives the lines of text a Channel writes.
///
/// A Channel passes its sink one line at a time, and an implementation
/// delivers each to wherever the text is to be kept; [`StringSink`] and
/// [`FileSink`] are ready-made ones.
/// Any data the sink needs belongs in the type that implements it.
///
/// A Channel and its clones share one sink, so it must be `Send` for the
/// Channel to be.
pub trait Sink: Send {
    /// Accept one line of output text, which has no line terminator.
    fn write_line(&mut self, line: &str);
}

/// Reads AST objects from text and writes them as text.
///
/// Writing an object to a Channel makes a textual description of it, and
/// reading from a Channel makes a new object from such a description.
/// A Channel reads its text from a [`Source`] and writes it to a [`Sink`].
/// Each kind of Channel has its own form of text:
///
/// - [`NativeChannel`]: AST's native text format, for any object.
/// - [`FitsChan`](crate::fitschan::FitsChan): FITS header cards.
/// - [`MocChan`]: the string and JSON serializations of the IVOA MOC
///   recommendation, for a [`Moc`](crate::regions::Moc).
///
/// Every Channel has the attributes of [`ChannelConfig`], which
/// [`config`](Self::config) and [`config_mut`](Self::config_mut) give.
pub trait Channel {
    /// Read the next object from the Channel.
    ///
    /// The class of the object returned depends on the text read, so is not
    /// known in advance.
    /// `Ok(None)` means there are no more objects to read.
    ///
    /// - A [`NativeChannel`] reads the lines of one object from its source
    ///   and leaves the rest for the next read; with
    ///   [Skip](ChannelConfig::skip) set, other lines before the object are
    ///   passed over.
    /// - A [`FitsChan`] reads the object from its cards, and a successful
    ///   read removes the cards it used; [`FitsChan`] says which cards a
    ///   read uses and what one that fails leaves.
    /// - A [`MocChan`] reads all of the rest of its source as the text of
    ///   one [`Moc`](crate::regions::Moc), which it normalizes, and sets its
    ///   [MocFormat](MocChan::moc_format) to the serialization it found; it
    ///   never returns `Ok(None)`.
    ///
    /// The warnings of the previous read or write are discarded first, and
    /// [`warnings`](Self::warnings) gives those of this one.
    ///
    /// # Errors
    ///
    /// For a NativeChannel:
    ///
    /// - [`ErrorCode::Badin`] if a line cannot be interpreted while Skip is
    ///   not set, an object does not start with a `Begin` line, the classes
    ///   in a description are out of order, the `End` line that should
    ///   follow the last class's data is missing, or Strict is set and an
    ///   item is not recognized.
    /// - [`ErrorCode::Eochn`] if the text ends within a class's data.
    /// - [`ErrorCode::Lderr`] if an object's `End` line comes before the
    ///   data of all of its classes.
    /// - [`ErrorCode::Ocluk`] if an object's class is unknown.
    /// - Passes on the error of making the object from its description.
    ///
    /// For a MocChan, [`ErrorCode::Inmoc`] if the text is empty or is not a
    /// MOC.
    ///
    /// For a FitsChan, passes on the error of reading its source or of
    /// making the object from its cards in the chosen encoding.
    ///
    /// [`FitsChan`]: crate::fitschan::FitsChan
    /// [`ErrorCode::Badin`]: crate::error::ErrorCode::Badin
    /// [`ErrorCode::Eochn`]: crate::error::ErrorCode::Eochn
    /// [`ErrorCode::Lderr`]: crate::error::ErrorCode::Lderr
    /// [`ErrorCode::Ocluk`]: crate::error::ErrorCode::Ocluk
    /// [`ErrorCode::Inmoc`]: crate::error::ErrorCode::Inmoc
    #[doc(alias = "astRead")]
    fn read(&mut self) -> AstResult<Option<AnyObject>>;

    /// Write an object to the Channel, after any objects written to it
    /// before.
    ///
    /// Returns the number of objects written: 1 normally, and 0 when the
    /// Channel's form of text cannot describe the object.
    /// That is an ordinary answer rather than an error: a FrameSet whose
    /// Mapping has no adequate linear approximation cannot be written as a
    /// FITS-WCS header, for instance.
    ///
    /// - A [`NativeChannel`] can describe any object, and passes the text to
    ///   its sink.
    /// - A [`FitsChan`] adds the cards that describe the object to those it
    ///   holds; they reach the sink only when the FitsChan is dropped or
    ///   [`write_fits`](crate::fitschan::FitsChan::write_fits) is called.
    ///   Setting the `NAXIS1`, `NAXIS2`, ... keywords first can help a write
    ///   in an encoding other than `NATIVE` succeed.
    /// - A [`MocChan`] writes only a [`Moc`](crate::regions::Moc), in the
    ///   serialization its [MocFormat](MocChan::moc_format) names, in lines
    ///   of at most [MocLineLen](MocChan::moc_line_len) characters.
    ///
    /// The warnings of the previous read or write are discarded first, and
    /// [`warnings`](Self::warnings) gives those of this one.
    ///
    /// # Errors
    ///
    /// - For a MocChan, [`ErrorCode::Smbuf`] if MocLineLen is too short to
    ///   hold a value, and the other errors of
    ///   [`Moc::get_moc_text`](crate::regions::Moc::get_moc_text).
    /// - Passes on the error of describing the object or, for a FitsChan,
    ///   of reading its source.
    ///
    /// [`FitsChan`]: crate::fitschan::FitsChan
    /// [`ErrorCode::Smbuf`]: crate::error::ErrorCode::Smbuf
    // C's `astWrite` likewise returns a count rather than reporting an error
    // for an object its encoding cannot describe.
    #[doc(alias = "astWrite")]
    fn write(&mut self, obj: &AnyObject) -> AstResult<usize>;

    /// Make `transform` available to the IntraMaps this Channel reads, which
    /// find their transformation by name.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Mritf`] if another transformation is already registered
    /// here under the same name.
    ///
    /// # See also
    ///
    /// `astIntraReg`, which registers a transformation for the whole
    /// process; here each Channel holds its own registrations.
    ///
    /// [`ErrorCode::Mritf`]: crate::error::ErrorCode::Mritf
    fn register_intra(&mut self, transform: Arc<dyn IntraTransform>) -> AstResult<()>;

    /// The texts of the warnings from the most recent read or write, in the
    /// order they were issued.
    ///
    /// These are the warnings [`warnings`](Self::warnings) returns as a
    /// [`KeyMap`].
    fn warning_texts(&self) -> &[String];

    /// The warnings from the most recent read or write, as a [`KeyMap`], or
    /// `None` if there were none.
    ///
    /// A warning reports a condition that does not stop the read or write
    /// but may leave the object read or written different from the original
    /// in some respect; a more significant difference is an error instead.
    /// The [ReportLevel](ChannelConfig::report_level) attribute selects the
    /// conditions that are reported, and the [Strict](ChannelConfig::strict)
    /// attribute makes them errors rather than warnings.
    ///
    /// - A [`NativeChannel`] warns of an item it does not recognize in an
    ///   object's description, which it ignores.
    /// - A [`FitsChan`] warns of the conditions its
    ///   [Warnings](crate::fitschan::FitsChan::warning_conditions) attribute
    ///   names.
    /// - A [`MocChan`] issues no warnings.
    ///
    /// The keys are `Warning_1`, `Warning_2`, ... in the order the warnings
    /// were issued, and each value is the text of one warning.
    /// A new KeyMap is made at each call.
    ///
    /// [`FitsChan`]: crate::fitschan::FitsChan
    #[doc(alias = "astWarnings")]
    fn warnings(&self) -> Option<KeyMap> {
        let texts = self.warning_texts();
        if texts.is_empty() {
            return None;
        }
        let mut map = KeyMap::with_size_guess(texts.len());
        for (i, text) in texts.iter().enumerate() {
            // C passes a single-space comment (`astMapPut0C(result, key,
            // (this->warnings)[i], " ")`), which its dump layer then treats as
            // absent; pass None so the Rust dump matches. C's `astWarnings`
            // also makes a new KeyMap at each call from the Channel's list of
            // texts.
            map.put_0c(&format!("Warning_{}", i + 1), text, None)
                .expect("Warning_<n> keys are valid and distinct");
        }
        Some(map)
    }

    /// The Channel's attributes.
    fn config(&self) -> &ChannelConfig;

    /// The Channel's attributes, to change.
    fn config_mut(&mut self) -> &mut ChannelConfig;
}
