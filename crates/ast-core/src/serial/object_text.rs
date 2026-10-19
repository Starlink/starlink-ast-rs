//! `show`, `to_native_string` and `TryFrom<AnyObject>` for every class.
//!
//! C's `astShow` writes an Object through a Channel with no attributes set,
//! to standard output; `astToString` writes it through a Channel set to
//! `Comment=0,Full=-1,Indent=0`, ending each line with a newline
//! (`object.c` `Show`, `astToString_`). Both render the class's own node, so
//! neither copies the object into an [`AnyObject`].

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

/// The text C's `astShow` writes for `obj`.
fn show_text<T: Serializable + ?Sized>(obj: &T) -> AstResult<String> {
    channel_text(obj, &ChannelConfig::default())
}

/// The text C's `astToString` returns for `obj`.
fn compact_text<T: Serializable + ?Sized>(obj: &T) -> AstResult<String> {
    let mut config = ChannelConfig::default();
    config.set_comment(false);
    config.set_full(-1);
    config.set_indent(0);
    channel_text(obj, &config)
}

/// Print `text` to standard output. Write errors are ignored, as C's
/// default Channel sink, `printf`, ignores them.
fn print_text(text: &str) {
    let _ = std::io::stdout().lock().write_all(text.as_bytes());
}

/// C's `astMAKE_CHECK` error for an Object of the wrong class.
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
                /// as C's `astShow` does: the text a Channel with no attributes
                /// set writes (`Comment` true, `Full` 0, `Indent` 3), so
                /// comments are included, the default values that help a
                /// reader are written as comments, and nested objects are
                /// indented. Errors writing to standard output are ignored, as
                /// C's `printf` sink ignores them.
                pub fn show(&self) -> AstResult<()> {
                    print_text(&show_text(self)?);
                    Ok(())
                }

                /// This object's minimal Native serialisation, as C's
                /// `astToString` returns it: the text a Channel set to
                /// `Comment=0,Full=-1,Indent=0` writes, so no comments, no
                /// default values and one leading space on every line, each
                /// line ending in a newline. Read it back with
                /// [`from_native_string`](crate::from_native_string).
                pub fn to_native_string(&self) -> AstResult<String> {
                    compact_text(self)
                }
            }

            impl TryFrom<AnyObject> for $ty {
                type Error = AstError;

                /// The object, if `obj` holds this class; otherwise C's
                /// AST__OBJIN error naming both classes.
                fn try_from(obj: AnyObject) -> AstResult<Self> {
                    match obj {
                        AnyObject::$variant(x) => Ok(object_text!(@unbox x $(, $boxed)?)),
                        other => Err(wrong_class($class, &other)),
                    }
                }
            }
        )*

        impl AnyObject {
            /// The AST class name of the object held, as C's `astGetClass`
            /// returns it (`"Box"` for a
            /// [`BoxRegion`](crate::regions::BoxRegion)).
            pub fn class_name(&self) -> &'static str {
                match self {
                    $(AnyObject::$variant(_) => $class,)*
                }
            }

            /// Write the object's Native description to standard output, as
            /// C's `astShow` does: the text a Channel with no attributes set
            /// writes (`Comment` true, `Full` 0, `Indent` 3). Errors writing
            /// to standard output are ignored, as C's `printf` sink ignores
            /// them.
            pub fn show(&self) -> AstResult<()> {
                print_text(&self.show_text()?);
                Ok(())
            }

            /// The text [`show`](Self::show) prints.
            #[doc(hidden)]
            pub fn show_text(&self) -> AstResult<String> {
                match self {
                    $(AnyObject::$variant(x) => show_text(object_text!(@ref x $(, $boxed)?)),)*
                }
            }

            /// The object's minimal Native serialisation, as C's
            /// `astToString` returns it: the text a Channel set to
            /// `Comment=0,Full=-1,Indent=0` writes. Read it back with
            /// [`from_native_string`](crate::from_native_string).
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
