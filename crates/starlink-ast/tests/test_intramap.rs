//! IntraMap, the Mapping that applies a transformation supplied by the
//! caller (intramap.c).
//!
//! There is no `ast_tester` program for IntraMap; the simplify fixtures cover
//! its MapMerge, and `intramap_oracle/cases.rs`, from
//! `tools/oracle_fixtures/intramap_probe.c`, holds C's dumps and
//! transformations for a doubling function and one without a forward
//! transformation, registered as here.

use crate::common::native_dump;
// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "intramap_oracle/cases.rs"]
mod cases;

use std::sync::Arc;

use starlink_ast::channel::{Channel, NativeChannel};
use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::{Points, PointsMut};
use starlink_ast::error::{AstError, AstResult, ErrorCode};
use starlink_ast::mappings::{
    CmpMap, Composition, IntraFlags, IntraMap, IntraRegistry, IntraTransform, Mapping, ZoomMap,
};
use starlink_ast::{AnyObject, from_native_string, from_native_string_with};

/// A doubling transformation, as the probe registers it: forward y = 2x,
/// inverse x = y/2, bad staying bad.
struct Double {
    name: &'static str,
    flags: IntraFlags,
    purpose: &'static str,
}

impl IntraTransform for Double {
    fn name(&self) -> &str {
        self.name
    }
    fn nin(&self) -> Option<usize> {
        Some(1)
    }
    fn nout(&self) -> Option<usize> {
        Some(1)
    }
    fn flags(&self) -> IntraFlags {
        self.flags
    }
    fn purpose(&self) -> &str {
        self.purpose
    }
    fn author(&self) -> &str {
        "AST test suite"
    }
    fn contact(&self) -> &str {
        "starlink-ast"
    }
    fn transform(
        &self,
        _map: &IntraMap,
        input: &Points<'_>,
        forward: bool,
        out: &mut PointsMut<'_>,
    ) -> AstResult<()> {
        for ip in 0..input.npoint() {
            let x = input.point(ip)[0];
            out.point_mut(ip)[0] = if x == BAD_VALUE {
                BAD_VALUE
            } else if forward {
                2.0 * x
            } else {
                0.5 * x
            };
        }
        Ok(())
    }
}

fn double() -> Arc<dyn IntraTransform> {
    Arc::new(Double {
        name: "double",
        flags: IntraFlags {
            simp_fi: true,
            simp_if: true,
            ..IntraFlags::default()
        },
        purpose: "Double the value",
    })
}

fn nofwd() -> Arc<dyn IntraTransform> {
    Arc::new(Double {
        name: "nofwd",
        flags: IntraFlags {
            no_fwd: true,
            ..IntraFlags::default()
        },
        purpose: "No forward transformation",
    })
}

fn dump_of(map: &IntraMap) -> String {
    native_dump(&AnyObject::IntraMap(map.clone())).unwrap()
}

/// The dumps of the probe's IntraMaps, byte for byte against C's: the
/// IntraFlag, an inverted IntraMap, and the `Fwd = 0` C writes for a
/// function without a forward transformation.
#[test]
fn dumps_match_the_c_library() {
    let mut dbl = IntraMap::new(double(), 1, 1).unwrap();
    dbl.set_intra_flag("v1");
    let want = |name: &str| cases::DUMPS.iter().find(|c| c.0 == name).unwrap().1;
    assert_eq!(dump_of(&dbl), want("double_with_flag"));
    dbl.invert();
    assert_eq!(dump_of(&dbl), want("double_inverted"));
    assert_eq!(
        dump_of(&IntraMap::new(nofwd(), 1, 1).unwrap()),
        want("nofwd")
    );
}

/// The same dumps written with Full = 1, which writes every unset value, as
/// C writes them less its unset `RefCnt` and `Nobj`, counts the port does
/// not have. C needs intramap.c as fixed upstream at 4fe377c3 to write the
/// unset IntraFlag.
#[test]
fn full_dumps_match_the_c_library() {
    let full = |map: &IntraMap| {
        let mut ch = NativeChannel::with_string_sink();
        ch.config_mut().set_full(1);
        ch.write(&AnyObject::IntraMap(map.clone())).unwrap();
        ch.into_string()
    };
    let want = |name: &str| cases::FULL_DUMPS.iter().find(|c| c.0 == name).unwrap().1;
    let mut dbl = IntraMap::new(double(), 1, 1).unwrap();
    dbl.set_intra_flag("v1");
    assert_eq!(full(&dbl), want("double_with_flag"));
    dbl.invert();
    assert_eq!(full(&dbl), want("double_inverted"));
    assert_eq!(full(&IntraMap::new(nofwd(), 1, 1).unwrap()), want("nofwd"));
}

/// The transformation is applied in the direction the Invert flag makes of
/// the one asked for, with the probe's values.
#[test]
fn transforms_match_the_c_library() {
    let dbl = IntraMap::new(double(), 1, 1).unwrap();
    assert_eq!(dbl.tran1(&cases::INPUT, true).unwrap(), cases::FORWARD);
    assert_eq!(dbl.tran1(&cases::INPUT, false).unwrap(), cases::INVERSE);
    let mut inverted = dbl.clone();
    inverted.invert();
    assert_eq!(
        inverted.tran1(&cases::INPUT, false).unwrap(),
        cases::FORWARD
    );
}

/// NOFWD and NOINV decide which directions are defined, relative to the
/// Invert flag.
#[test]
fn flags_decide_the_defined_directions() {
    let mut m = IntraMap::new(nofwd(), 1, 1).unwrap();
    assert!(!m.has_forward() && m.has_inverse());
    assert_eq!(m.tran1(&[1.0], true).unwrap_err().code(), ErrorCode::Trnnd);
    m.invert();
    assert!(m.has_forward() && !m.has_inverse());
}

/// intramap.c astInitIntraMap: numbers of coordinates the function fixes
/// must match (AST__BADNI, AST__BADNO); astIntraReg's CleanName refuses a
/// blank name (AST__ITFNI).
#[test]
fn construction_checks_as_c_does() {
    assert_eq!(
        IntraMap::new(double(), 2, 1).unwrap_err().code(),
        ErrorCode::Badni
    );
    assert_eq!(
        IntraMap::new(double(), 1, 2).unwrap_err().code(),
        ErrorCode::Badno
    );
    let blank: Arc<dyn IntraTransform> = Arc::new(Double {
        name: " \t ",
        flags: IntraFlags::default(),
        purpose: "",
    });
    assert_eq!(
        IntraMap::new(blank, 1, 1).unwrap_err().code(),
        ErrorCode::Itfni
    );
}

/// intramap.c IntraReg: a name already registered is refused (AST__MRITF)
/// unless it is the same transformation, and white space in a name is
/// ignored.
#[test]
fn registry_refuses_a_second_transformation_of_one_name() {
    let mut reg = IntraRegistry::new();
    let t = double();
    reg.register(Arc::clone(&t)).unwrap();
    reg.register(Arc::clone(&t)).unwrap();
    assert_eq!(reg.register(double()).unwrap_err().code(), ErrorCode::Mritf);
    let spaced: Arc<dyn IntraTransform> = Arc::new(Double {
        name: " dou ble ",
        flags: IntraFlags::default(),
        purpose: "",
    });
    assert_eq!(reg.register(spaced).unwrap_err().code(), ErrorCode::Mritf);
    assert!(reg.get("double").is_some());
}

/// An IntraMap reads back through a Channel that knows its transformation,
/// and without one is AST__URITF, as C's loader reports an unregistered
/// function.
#[test]
fn reading_needs_the_transformation_registered() {
    let mut dbl = IntraMap::new(double(), 1, 1).unwrap();
    dbl.set_intra_flag("v1");
    let text = dump_of(&dbl);
    assert_eq!(
        from_native_string(&text).unwrap_err().code(),
        ErrorCode::Uritf
    );

    let mut ch = NativeChannel::from_string(&text);
    ch.register_intra(double()).unwrap();
    let Some(AnyObject::IntraMap(back)) = ch.read().unwrap() else {
        panic!("not an IntraMap");
    };
    assert!(back.equal(&dbl));
    assert_eq!(dump_of(&back), text);
}

/// `from_native_string_with` gives an IntraMap nested inside another
/// Mapping its transformation, and a registry without it is AST__URITF.
#[test]
fn from_native_string_with_reads_a_nested_intramap() {
    let cmp = CmpMap::new(
        Box::new(ZoomMap::new(1, 3.0).unwrap()),
        Box::new(IntraMap::new(double(), 1, 1).unwrap()),
        Composition::Series,
    )
    .unwrap();
    let text = native_dump(&AnyObject::CmpMap(cmp.clone())).unwrap();

    let mut reg = IntraRegistry::new();
    reg.register(nofwd()).unwrap();
    assert_eq!(
        from_native_string_with(&text, &reg).unwrap_err().code(),
        ErrorCode::Uritf
    );

    reg.register(double()).unwrap();
    let back = from_native_string_with(&text, &reg)
        .unwrap()
        .into_mapping()
        .unwrap();
    assert_eq!(back.tran1(&[1.5], true).unwrap(), [9.0]);
    assert_eq!(
        native_dump(&AnyObject::from_mapping(back.as_ref()).unwrap()).unwrap(),
        text
    );
}

/// intramap.c Equal: the same transformation and an IntraFlag set on both
/// and equal, so two IntraMaps without one are not equal.
#[test]
fn equality_needs_an_intra_flag_on_both() {
    let a = IntraMap::new(double(), 1, 1).unwrap();
    let mut b = a.clone();
    assert!(!a.equal(&b));
    let mut a = a;
    a.set_intra_flag("x");
    assert!(!a.equal(&b));
    b.set_intra_flag("x");
    assert!(a.equal(&b));
    b.invert();
    assert!(!a.equal(&b));
}

/// IntraFlag get/set/test/clear (intramap.c).
#[test]
fn intra_flag_set_test_clear() {
    let mut m = IntraMap::new(double(), 1, 1).unwrap();
    assert!(!m.test_intra_flag() && m.intra_flag().is_none());
    m.set_intra_flag("abc");
    assert!(m.test_intra_flag());
    assert_eq!(m.intra_flag(), Some("abc"));
    m.clear_intra_flag();
    assert!(!m.test_intra_flag());
}

/// An error from the transformation is reported as one signaled by the
/// named function, with the transformation's own error code.
#[test]
fn transformation_errors_name_the_function() {
    struct Failing;
    impl IntraTransform for Failing {
        fn name(&self) -> &str {
            "failing"
        }
        fn nin(&self) -> Option<usize> {
            None
        }
        fn nout(&self) -> Option<usize> {
            None
        }
        fn flags(&self) -> IntraFlags {
            IntraFlags::default()
        }
        fn purpose(&self) -> &str {
            ""
        }
        fn author(&self) -> &str {
            ""
        }
        fn contact(&self) -> &str {
            ""
        }
        fn transform(
            &self,
            _: &IntraMap,
            _: &Points<'_>,
            _: bool,
            _: &mut PointsMut<'_>,
        ) -> AstResult<()> {
            Err(AstError::new(ErrorCode::Inter, "it failed"))
        }
    }
    let m = IntraMap::new(Arc::new(Failing), 2, 2).unwrap();
    let err = m.tran2(&[1.0], &[2.0], true).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Inter);
    assert!(err.message().contains("\"failing\""), "{}", err.message());
}
