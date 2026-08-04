//! Simplify one native-format Mapping fixture and print the result.
//!
//! Usage: `cargo run -p ast-core --example simp_one -- <path-to-.map>`
//!
//! A FrameSet input is reduced to its base→current Mapping first, matching what
//! the simplify regression harness feeds to `simplify`.

use ast_core::mappings::{
    CmpMap, GrismMap, Mapping, MatrixMap, NormMap, PcdMap, PermMap, RateMap, ShiftMap, SlaMap,
    SpecMap, SphMap, TimeMap, TranMap, UnitMap, UnitNormMap, WcsMap, WinMap, ZoomMap,
};
use ast_core::{AnyObject, from_native_string, to_native_string};

/// Wrap a Mapping back into an `AnyObject` so it can be serialised.
fn as_any_object(m: &dyn Mapping) -> Option<AnyObject> {
    macro_rules! try_class {
        ($($class:ident),+ $(,)?) => {
            $(
                if let Some(x) = m.as_any().downcast_ref::<$class>() {
                    return Some(AnyObject::$class(x.clone()));
                }
            )+
        };
    }
    try_class!(
        UnitMap,
        ZoomMap,
        ShiftMap,
        MatrixMap,
        PermMap,
        WinMap,
        CmpMap,
        WcsMap,
        SlaMap,
        SpecMap,
        TimeMap,
        PcdMap,
        GrismMap,
        SphMap,
        UnitNormMap,
        TranMap,
        RateMap,
        NormMap,
    );
    None
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: simp_one <file.map>");
    let text = std::fs::read_to_string(&path).expect("read fixture");
    let obj = from_native_string(&text).expect("parse fixture");
    let mapping = match obj {
        AnyObject::FrameSet(ref fs) => fs
            .mapping(
                ast_core::frames::FrameRef::Base,
                ast_core::frames::FrameRef::Current,
            )
            .expect("frameset mapping"),
        other => other.into_mapping().expect("mapping"),
    };
    let simplified = mapping.simplify().expect("simplify");
    match as_any_object(simplified.as_ref()) {
        Some(obj) => println!("{}", to_native_string(&obj).expect("serialize")),
        None => println!("(result class is not one this example can serialize)"),
    }
}
