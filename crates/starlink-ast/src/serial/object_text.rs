// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! `show`, `to_native_string` and `TryFrom<AnyObject>` for every class.
//!
//! Both text forms render the class's own node, so neither copies the object
//! into an [`AnyObject`].

// `show` is `astShow`, which writes an Object through a Channel with no
// attributes set to standard output; `to_native_string` is `astToString`,
// which writes it through a Channel set to `Comment=0,Full=-1,Indent=0`,
// ending each line with a newline (object.c `Show`, `astToString_`).

use std::io::Write;

use super::native_write::{RenderOptions, render_native_with_options};
use super::{AnyObject, Serializable};
use crate::channel::ChannelConfig;
use crate::error::{AstError, AstResult, ErrorCode};

/// The text a Native Channel configured by `config` writes for `obj`: each
/// line the renderer produces, followed by a newline, as the Channel's sink
/// receives them.
fn channel_text<T: Serializable + ?Sized>(obj: &T, config: &ChannelConfig) -> AstResult<String> {
    let text = render_native_with_options(&obj.to_node()?, &RenderOptions::from_config(config))?;
    let mut out = String::with_capacity(text.len() + 1);
    for line in text.lines() {
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

/// The text `show` prints for `obj`.
fn show_text<T: Serializable + ?Sized>(obj: &T) -> AstResult<String> {
    channel_text(obj, &ChannelConfig::default())
}

/// The text `to_native_string` returns for `obj`.
fn compact_text<T: Serializable + ?Sized>(obj: &T) -> AstResult<String> {
    let mut config = ChannelConfig::default();
    config.set_comment(false);
    config.set_full(-1);
    config.set_indent(0);
    channel_text(obj, &config)
}

/// Print `text` to standard output, ignoring write errors.
// C's default Channel sink, `printf`, ignores them too.
fn print_text(text: &str) {
    let _ = std::io::stdout().lock().write_all(text.as_bytes());
}

/// The error for an object of the wrong class.
// The message is the one C's `astMAKE_CHECK` reports.
fn wrong_class(wanted: &str, got: &AnyObject) -> AstError {
    AstError::new(
        ErrorCode::Objin,
        format!(
            "Pointer to {wanted} required, but pointer to {} given.",
            got.class_name()
        ),
    )
}

macro_rules! object_text {
    ($( $variant:ident : $ty:ty = $class:literal $(, $boxed:ident)? ;)*) => {
        $(
            impl $ty {
                /// Write this object's Native description to standard output,
                /// as an aid to debugging.
                ///
                /// The text is what a [`NativeChannel`](crate::channel::NativeChannel)
                /// with the default [`ChannelConfig`](crate::channel::ChannelConfig) writes
                /// (Comment true, Full 0, Indent 3): it includes comments,
                /// writes as comments the default values that help a reader,
                /// and indents nested objects.
                /// Errors writing to standard output are ignored.
                ///
                /// # Errors
                ///
                /// Passes on the error of writing this object to a Channel
                /// ([`Channel::write`](crate::channel::Channel::write)).
                #[doc(alias = "astShow")]
                pub fn show(&self) -> AstResult<()> {
                    print_text(&show_text(self)?);
                    Ok(())
                }

                /// This object's minimal Native serialization.
                ///
                /// The text is what a [`NativeChannel`](crate::channel::NativeChannel)
                /// with Comment false, Full -1 and Indent 0 writes: no comments,
                /// no default values and one leading space on every line, each
                /// line ending in a newline.
                /// Read it back with [`from_native_string`](crate::from_native_string).
                ///
                /// # Errors
                ///
                /// Passes on the error of writing this object to a Channel
                /// ([`Channel::write`](crate::channel::Channel::write)).
                #[doc(alias = "astToString")]
                pub fn to_native_string(&self) -> AstResult<String> {
                    compact_text(self)
                }
            }

            impl TryFrom<AnyObject> for $ty {
                type Error = AstError;

                /// The object, if `obj` holds this class.
                ///
                /// # Errors
                ///
                /// - `ErrorCode::Objin` if `obj` holds another class; the
                ///   message names both classes.
                fn try_from(obj: AnyObject) -> AstResult<Self> {
                    match obj {
                        AnyObject::$variant(x) => Ok(object_text!(@unbox x $(, $boxed)?)),
                        other => Err(wrong_class($class, &other)),
                    }
                }
            }
        )*

        impl AnyObject {
            /// The AST class name of the object held, the Class attribute:
            /// `"SkyFrame"` for a [`SkyFrame`](crate::frames::SkyFrame),
            /// `"Frame"` for a [`BasicFrame`](crate::frames::BasicFrame),
            /// `"Box"` for a [`BoxRegion`](crate::regions::BoxRegion) and
            /// `"Channel"` for a [`NativeChannel`](crate::channel::NativeChannel).
            ///
            /// It is the name a Native dump of the object carries on its
            /// `Begin` line.
            #[doc(alias = "Class")]
            pub fn class_name(&self) -> &'static str {
                match self {
                    $(AnyObject::$variant(x) => Serializable::class_name(object_text!(@ref x $(, $boxed)?)),)*
                }
            }

            /// The class name the [`TryFrom`] impl for this variant's type
            /// reports as wanted, checked against [`class_name`](Self::class_name)
            /// by the tests below.
            #[cfg(test)]
            fn wanted_class_name(&self) -> &'static str {
                match self {
                    $(AnyObject::$variant(_) => $class,)*
                }
            }

            /// The variant names, in declaration order.
            #[cfg(test)]
            const VARIANTS: &'static [&'static str] = &[$(stringify!($variant)),*];

            /// This object's variant name.
            #[cfg(test)]
            fn variant_name(&self) -> &'static str {
                match self {
                    $(AnyObject::$variant(_) => stringify!($variant),)*
                }
            }

            /// Write the object's Native description to standard output, as
            /// an aid to debugging.
            ///
            /// The text is what a [`NativeChannel`](crate::channel::NativeChannel)
            /// with the default [`ChannelConfig`](crate::channel::ChannelConfig) writes
            /// (Comment true, Full 0, Indent 3).
            /// Errors writing to standard output are ignored.
            ///
            /// # Errors
            ///
            /// Passes on the error of writing the object to a Channel
            /// ([`Channel::write`](crate::channel::Channel::write)).
            #[doc(alias = "astShow")]
            pub fn show(&self) -> AstResult<()> {
                print_text(&self.show_text()?);
                Ok(())
            }

            /// The text [`show`](Self::show) prints.
            pub(crate) fn show_text(&self) -> AstResult<String> {
                match self {
                    $(AnyObject::$variant(x) => show_text(object_text!(@ref x $(, $boxed)?)),)*
                }
            }

            /// The object's minimal Native serialization: the text a
            /// [`NativeChannel`](crate::channel::NativeChannel) with Comment
            /// false, Full -1 and Indent 0 writes.
            /// Read it back with [`from_native_string`](crate::from_native_string).
            ///
            /// # Errors
            ///
            /// Passes on the error of writing the object to a Channel
            /// ([`Channel::write`](crate::channel::Channel::write)).
            #[doc(alias = "astToString")]
            pub fn to_native_string(&self) -> AstResult<String> {
                match self {
                    $(AnyObject::$variant(x) => compact_text(object_text!(@ref x $(, $boxed)?)),)*
                }
            }
        }
    };
    (@unbox $x:ident, boxed) => { *$x };
    (@unbox $x:ident) => { $x };
    (@ref $x:ident, boxed) => { &**$x };
    (@ref $x:ident) => { $x };
}

object_text! {
    UnitMap: super::UnitMap = "UnitMap";
    ZoomMap: super::ZoomMap = "ZoomMap";
    ShiftMap: super::ShiftMap = "ShiftMap";
    MatrixMap: super::MatrixMap = "MatrixMap";
    PermMap: super::PermMap = "PermMap";
    WinMap: super::WinMap = "WinMap";
    LutMap: super::LutMap = "LutMap";
    PolyMap: super::PolyMap = "PolyMap";
    ChebyMap: super::ChebyMap = "ChebyMap";
    CmpMap: super::CmpMap = "CmpMap";
    WcsMap: super::WcsMap = "WcsMap";
    SlaMap: super::SlaMap = "SlaMap";
    SpecMap: super::SpecMap = "SpecMap";
    TimeMap: super::TimeMap = "TimeMap";
    PcdMap: super::PcdMap = "PcdMap";
    GrismMap: super::GrismMap = "GrismMap";
    SphMap: super::SphMap = "SphMap";
    UnitNormMap: super::UnitNormMap = "UnitNormMap";
    XphMap: super::XphMap = "XphMap";
    IntraMap: super::IntraMap = "IntraMap";
    TranMap: super::TranMap = "TranMap";
    RateMap: super::RateMap = "RateMap";
    MathMap: super::MathMap = "MathMap";
    SplineMap: super::SplineMap = "SplineMap";
    DssMap: super::DssMap = "DssMap";
    NormMap: super::NormMap = "NormMap";
    SelectorMap: super::SelectorMap = "SelectorMap";
    SwitchMap: super::SwitchMap = "SwitchMap";
    BasicFrame: super::BasicFrame = "Frame";
    SkyFrame: super::SkyFrame = "SkyFrame";
    SpecFrame: super::SpecFrame = "SpecFrame";
    DSBSpecFrame: super::DSBSpecFrame = "DSBSpecFrame";
    TimeFrame: super::TimeFrame = "TimeFrame";
    FluxFrame: super::FluxFrame = "FluxFrame";
    SpecFluxFrame: super::SpecFluxFrame = "SpecFluxFrame", boxed;
    CmpFrame: super::CmpFrame = "CmpFrame";
    FrameSet: super::FrameSet = "FrameSet";
    Circle: super::Circle = "Circle";
    BoxRegion: super::BoxRegion = "Box";
    Ellipse: super::Ellipse = "Ellipse";
    Polygon: super::Polygon = "Polygon";
    Interval: super::Interval = "Interval";
    NullRegion: super::NullRegion = "NullRegion";
    CmpRegion: super::CmpRegion = "CmpRegion";
    PointList: super::PointList = "PointList";
    Prism: super::Prism = "Prism";
    Moc: super::Moc = "Moc";
    StcResourceProfile: super::StcResourceProfile = "StcResourceProfile";
    StcSearchLocation: super::StcSearchLocation = "StcSearchLocation";
    StcCatalogEntryLocation: super::StcCatalogEntryLocation = "StcCatalogEntryLocation";
    StcObsDataLocation: super::StcObsDataLocation = "StcObsDataLocation";
    KeyMap: crate::keymap::KeyMap = "KeyMap", boxed;
    Table: crate::table::Table = "Table", boxed;
    FitsTable: crate::fitstable::FitsTable = "FitsTable", boxed;
    NativeChannel: crate::channel::NativeChannel = "Channel", boxed;
    MocChan: crate::channel::MocChan = "MocChan", boxed;
    FitsChan: crate::fitschan::FitsChan = "FitsChan", boxed;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use super::AnyObject;
    use crate::serial::any_object_to_node;

    include!("../../tests/show_tostring_oracle/cases.rs");

    #[test]
    fn show_text_matches_ast_show() {
        for (name, _, show, _) in CASES {
            let obj = crate::from_native_string(show).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(obj.show_text().unwrap(), *show, "{name}");
        }
    }

    #[test]
    fn show_text_is_what_a_default_channel_writes() {
        use crate::channel::{Channel, NativeChannel};
        for (name, _, show, _) in CASES {
            let obj = crate::from_native_string(show).unwrap();
            let mut ch = NativeChannel::with_string_sink();
            ch.write(&obj).unwrap();
            assert_eq!(obj.show_text().unwrap(), ch.into_string(), "{name}");
        }
    }

    /// Every file under `tests/fixtures` whose first line that is neither
    /// blank nor a comment is a `Begin` line: the Native dumps.
    fn native_dumps(dir: &Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                native_dumps(&path, out);
            } else if let Ok(text) = std::fs::read_to_string(&path)
                && text
                    .lines()
                    .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
                    .is_some_and(|l| l.trim_start().starts_with("Begin "))
            {
                out.push(path);
            }
        }
    }

    /// The three places a class name lives agree for `obj`: the name the
    /// `TryFrom` error reports as wanted, `class_name`, and the name its
    /// dump's `Begin` line carries.
    fn check_names(obj: &AnyObject, what: &str) {
        let written = any_object_to_node(obj).unwrap().class;
        assert_eq!(obj.class_name(), written, "{what}: class_name");
        assert_eq!(
            obj.wanted_class_name(),
            written,
            "{what}: TryFrom's wanted name"
        );
    }

    #[test]
    fn every_class_name_matches_the_class_its_dump_names() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let mut files = Vec::new();
        native_dumps(&root, &mut files);
        // The transformations the corpus's IntraMap fixtures name.
        let mut registry = crate::mappings::IntraRegistry::new();
        for name in [
            "simplifyidentity",
            "simplifyidentity2",
            "nosimpfi",
            "nosimpif",
        ] {
            registry
                .register(std::sync::Arc::new(Identity(name)))
                .unwrap();
        }
        let mut seen = BTreeSet::new();
        let mut unread = Vec::new();
        for path in &files {
            let text = std::fs::read_to_string(path).unwrap();
            match crate::from_native_string_with(&text, &registry) {
                Ok(obj) => {
                    check_names(&obj, &path.display().to_string());
                    seen.insert(obj.variant_name());
                }
                Err(_) => unread.push(path.strip_prefix(&root).unwrap().display().to_string()),
            }
        }
        // A Frame in a Mapping slot is refused by decision
        // (docs/issues/frame-in-a-mapping-slot.md); any other dump must read.
        assert_eq!(unread, ["serialisation/frame_in_map_slot.ast"]);
        for (what, obj) in objects_without_a_fixture() {
            check_names(&obj, what);
            seen.insert(obj.variant_name());
        }
        let missing: Vec<_> = AnyObject::VARIANTS
            .iter()
            .filter(|v| !seen.contains(*v))
            .collect();
        assert!(missing.is_empty(), "no object checked for {missing:?}");
    }

    /// An identity transformation under `name`: the IntraMap built below,
    /// and the transformations the corpus's IntraMap fixtures name.
    struct Identity(&'static str);

    impl crate::mappings::IntraTransform for Identity {
        fn name(&self) -> &str {
            self.0
        }
        fn nin(&self) -> Option<usize> {
            Some(1)
        }
        fn nout(&self) -> Option<usize> {
            Some(1)
        }
        fn flags(&self) -> crate::mappings::IntraFlags {
            crate::mappings::IntraFlags::default()
        }
        fn purpose(&self) -> &str {
            "Copy the value"
        }
        fn author(&self) -> &str {
            "test"
        }
        fn contact(&self) -> &str {
            "test"
        }
        fn transform(
            &self,
            _map: &crate::mappings::IntraMap,
            input: &crate::coords::Points<'_>,
            _forward: bool,
            out: &mut crate::coords::PointsMut<'_>,
        ) -> crate::error::AstResult<()> {
            out.axis_mut(0).copy_from_slice(input.axis(0));
            Ok(())
        }
    }

    /// One object of each class that is never the top-level object of a
    /// Native dump under `tests/fixtures`.
    fn objects_without_a_fixture() -> Vec<(&'static str, AnyObject)> {
        use crate::frames::{FluxFrame, SkyFrame, SkySystem};
        use crate::mappings::{IntraMap, SphMap, SplineMap, XphMap, XphProjection};
        use crate::regions::{
            CircleBuilder, Moc, StcCatalogEntryLocationBuilder, StcObsDataLocationBuilder,
            StcSearchLocationBuilder,
        };
        let circle = || {
            CircleBuilder::new(SkyFrame::new(SkySystem::Icrs), vec![1.0, 0.5])
                .unwrap()
                .radius(0.01)
                .build()
                .unwrap()
        };
        let spline = SplineMap::new(
            2,
            2,
            2,
            2,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 1.0, 0.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        vec![
            ("SphMap", AnyObject::SphMap(SphMap::new())),
            (
                "XphMap",
                AnyObject::XphMap(XphMap::new(3, XphProjection::Hpx0)),
            ),
            (
                "IntraMap",
                AnyObject::IntraMap(
                    IntraMap::new(std::sync::Arc::new(Identity("identity")), 1, 1).unwrap(),
                ),
            ),
            ("SplineMap", AnyObject::SplineMap(spline)),
            (
                "FluxFrame",
                AnyObject::FluxFrame(FluxFrame::with_spec_val(1.0e9, None)),
            ),
            ("Moc", AnyObject::Moc(Moc::new())),
            (
                "StcSearchLocation",
                AnyObject::StcSearchLocation(
                    StcSearchLocationBuilder::new(circle()).build().unwrap(),
                ),
            ),
            (
                "StcCatalogEntryLocation",
                AnyObject::StcCatalogEntryLocation(
                    StcCatalogEntryLocationBuilder::new(circle())
                        .build()
                        .unwrap(),
                ),
            ),
            (
                "StcObsDataLocation",
                AnyObject::StcObsDataLocation(
                    StcObsDataLocationBuilder::new(circle()).build().unwrap(),
                ),
            ),
            ("Table", AnyObject::Table(Box::default())),
            (
                "FitsTable",
                AnyObject::FitsTable(Box::new(crate::fitstable::FitsTable::new().unwrap())),
            ),
            ("NativeChannel", AnyObject::NativeChannel(Box::default())),
            ("MocChan", AnyObject::MocChan(Box::default())),
        ]
    }
}
