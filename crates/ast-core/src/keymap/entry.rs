//! Entry storage: the AST type codes, the scalar-or-vector payload, and
//! the per-entry bookkeeping C keeps in `struct AstMapEntry`.

use crate::serial::AnyObject;

/// The AST data type of a KeyMap entry.
///
/// The discriminants are C's `AST__*TYPE` constants from `ast.h`.
/// `AST__BADTYPE` (0) is C's "no type" marker and has no variant;
/// `AST__POINTERTYPE` (6) holds a raw C pointer and is not representable
/// in the Rust API, so its code is reserved but unhandled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AstType {
    /// `AST__INTTYPE`: `int`.
    Int = 1,
    /// `AST__DOUBLETYPE`: `double`.
    Double = 2,
    /// `AST__STRINGTYPE`: null-terminated string.
    String = 3,
    /// `AST__OBJECTTYPE`: a nested AST Object.
    Object = 4,
    /// `AST__FLOATTYPE`: `float`.
    Float = 5,
    /// `AST__SINTTYPE`: `short int`.
    SInt = 7,
    /// `AST__UNDEFTYPE`: the key exists but has no value.
    Undef = 8,
    /// `AST__BYTETYPE`: `unsigned char`.
    Byte = 9,
    /// `AST__KINTTYPE`: 64-bit integer.
    KInt = 10,
}

impl AstType {
    /// Every representable type, in ascending code order.
    pub const ALL: [AstType; 9] = [
        AstType::Int,
        AstType::Double,
        AstType::String,
        AstType::Object,
        AstType::Float,
        AstType::SInt,
        AstType::Undef,
        AstType::Byte,
        AstType::KInt,
    ];

    /// The C `AST__*TYPE` code for this type.
    pub fn code(self) -> i64 {
        self as i64
    }

    /// The type for a C `AST__*TYPE` code, or `None` when the code is
    /// `AST__BADTYPE`, `AST__POINTERTYPE`, or out of range.
    pub fn from_code(code: i64) -> Option<AstType> {
        AstType::ALL.into_iter().find(|t| t.code() == code)
    }

    /// The comment written alongside the `Typ%d` card, matching
    /// `DumpEntry` in `keymap.c`.
    pub fn type_comment(self) -> &'static str {
        match self {
            AstType::String => "Item data type (string)",
            AstType::Object => "Item data type (AST Object)",
            AstType::Int => "Item data type (int)",
            AstType::SInt => "Item data type (short int)",
            AstType::KInt => "Item data type (64 bit int)",
            AstType::Byte => "Item data type (unsigned byte)",
            AstType::Double => "Item data type (double)",
            AstType::Float => "Item data type (float)",
            AstType::Undef => "Item data type (undefined)",
        }
    }
}

/// A payload that is either a single value or a 1-D vector.
///
/// C distinguishes these with `Entry.nel`: 0 for a scalar, the element
/// count for a vector. The distinction is visible in the dump (`Val%d`
/// versus `Nel%d` plus `V%d_%d`) and in `astMapLength`, so it is
/// preserved rather than normalised to a length-1 vector.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Scalar1D<T> {
    /// `nel == 0`.
    Scalar(T),
    /// `nel > 0`.
    Vector(Vec<T>),
}

impl<T> Scalar1D<T> {
    /// The number of values held: 1 for a scalar, the vector length
    /// otherwise. This is what `astMapGet1<X>` fills.
    pub(crate) fn len(&self) -> usize {
        match self {
            Scalar1D::Scalar(_) => 1,
            Scalar1D::Vector(v) => v.len(),
        }
    }

    /// C's `Entry.nel`: 0 for a scalar, the vector length otherwise.
    /// This is what `astMapLength` returns and what the `Nel%d` card
    /// records.
    pub(crate) fn nel(&self) -> usize {
        match self {
            Scalar1D::Scalar(_) => 0,
            Scalar1D::Vector(v) => v.len(),
        }
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        match self {
            Scalar1D::Scalar(v) => std::slice::from_ref(v),
            Scalar1D::Vector(v) => v.as_slice(),
        }
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [T] {
        match self {
            Scalar1D::Scalar(v) => std::slice::from_mut(v),
            Scalar1D::Vector(v) => v.as_mut_slice(),
        }
    }
}

/// The typed value held by a KeyMap entry.
#[derive(Debug, Clone)]
pub(crate) enum EntryValue {
    Int(Scalar1D<i32>),
    Double(Scalar1D<f64>),
    String(Scalar1D<String>),
    /// `None` is C's NULL Object element, which `MapPut1A` stores and
    /// `DumpEntry` omits.
    Object(Scalar1D<Option<Box<AnyObject>>>),
    Float(Scalar1D<f32>),
    SInt(Scalar1D<i16>),
    Byte(Scalar1D<u8>),
    KInt(Scalar1D<i64>),
    /// `AST__UNDEFTYPE`: the key exists but holds no value. C tracks
    /// this with `Entry.defined == 0` and still honours `nel`.
    Undef {
        nel: usize,
    },
}

impl EntryValue {
    pub(crate) fn ast_type(&self) -> AstType {
        match self {
            EntryValue::Int(_) => AstType::Int,
            EntryValue::Double(_) => AstType::Double,
            EntryValue::String(_) => AstType::String,
            EntryValue::Object(_) => AstType::Object,
            EntryValue::Float(_) => AstType::Float,
            EntryValue::SInt(_) => AstType::SInt,
            EntryValue::Byte(_) => AstType::Byte,
            EntryValue::KInt(_) => AstType::KInt,
            EntryValue::Undef { .. } => AstType::Undef,
        }
    }

    /// C's `Entry.nel`.
    pub(crate) fn nel(&self) -> usize {
        match self {
            EntryValue::Int(v) => v.nel(),
            EntryValue::Double(v) => v.nel(),
            EntryValue::String(v) => v.nel(),
            EntryValue::Object(v) => v.nel(),
            EntryValue::Float(v) => v.nel(),
            EntryValue::SInt(v) => v.nel(),
            EntryValue::Byte(v) => v.nel(),
            EntryValue::KInt(v) => v.nel(),
            EntryValue::Undef { nel } => *nel,
        }
    }

    /// The number of values held, treating a scalar as length 1.
    pub(crate) fn len(&self) -> usize {
        match self {
            EntryValue::Undef { nel } => *nel,
            other => match other {
                EntryValue::Int(v) => v.len(),
                EntryValue::Double(v) => v.len(),
                EntryValue::String(v) => v.len(),
                EntryValue::Object(v) => v.len(),
                EntryValue::Float(v) => v.len(),
                EntryValue::SInt(v) => v.len(),
                EntryValue::Byte(v) => v.len(),
                EntryValue::KInt(v) => v.len(),
                EntryValue::Undef { .. } => unreachable!(),
            },
        }
    }

    /// C's `Entry.defined`: false only for `AST__UNDEFTYPE`.
    pub(crate) fn defined(&self) -> bool {
        !matches!(self, EntryValue::Undef { .. })
    }
}

/// One key's worth of state.
///
/// `member` is bumped on every put of a value; `keymember` is retained
/// when an existing key is re-put, so the two age counters diverge and
/// `SortBy=AgeUp` and `SortBy=KeyAgeUp` order differently.
#[derive(Debug, Clone)]
pub(crate) struct MapEntry {
    pub(crate) comment: Option<String>,
    pub(crate) member: u64,
    pub(crate) keymember: u64,
    pub(crate) value: EntryValue,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_codes_match_c() {
        // reference/starlink-ast/src/ast.h: AST__BADTYPE 0 .. AST__KINTTYPE 10.
        assert_eq!(AstType::Int.code(), 1);
        assert_eq!(AstType::Double.code(), 2);
        assert_eq!(AstType::String.code(), 3);
        assert_eq!(AstType::Object.code(), 4);
        assert_eq!(AstType::Float.code(), 5);
        assert_eq!(AstType::SInt.code(), 7);
        assert_eq!(AstType::Undef.code(), 8);
        assert_eq!(AstType::Byte.code(), 9);
        assert_eq!(AstType::KInt.code(), 10);

        // 0 is AST__BADTYPE and 6 is AST__POINTERTYPE: neither is
        // representable, so both must round-trip as None.
        assert_eq!(AstType::from_code(0), None);
        assert_eq!(AstType::from_code(6), None);
        assert_eq!(AstType::from_code(11), None);
        for t in AstType::ALL {
            assert_eq!(AstType::from_code(t.code()), Some(t));
        }
    }

    #[test]
    fn type_comments_match_dumpentry() {
        // reference/starlink-ast/src/keymap.c DumpEntry, the Typ%d comments.
        assert_eq!(AstType::String.type_comment(), "Item data type (string)");
        assert_eq!(
            AstType::Object.type_comment(),
            "Item data type (AST Object)"
        );
        assert_eq!(AstType::Int.type_comment(), "Item data type (int)");
        assert_eq!(AstType::SInt.type_comment(), "Item data type (short int)");
        assert_eq!(AstType::KInt.type_comment(), "Item data type (64 bit int)");
        assert_eq!(
            AstType::Byte.type_comment(),
            "Item data type (unsigned byte)"
        );
        assert_eq!(AstType::Double.type_comment(), "Item data type (double)");
        assert_eq!(AstType::Float.type_comment(), "Item data type (float)");
        assert_eq!(AstType::Undef.type_comment(), "Item data type (undefined)");
    }

    #[test]
    fn scalar_and_vector_nel_differ_like_c() {
        // C: nel == 0 marks a scalar; nel > 0 is a vector length. Both
        // hold `len()` values, but only the vector dumps a Nel%d card.
        let s = EntryValue::Int(Scalar1D::Scalar(7));
        assert_eq!(s.nel(), 0);
        assert_eq!(s.len(), 1);
        assert!(s.defined());

        let v = EntryValue::Int(Scalar1D::Vector(vec![1, 2, 3]));
        assert_eq!(v.nel(), 3);
        assert_eq!(v.len(), 3);

        // A zero-length vector is still a vector in C (nel is 0 only for
        // scalars), but astMapPut1 with size 0 cannot arise from the Rust
        // API, so len() and nel() agree.
        let u = EntryValue::Undef { nel: 0 };
        assert_eq!(u.ast_type(), AstType::Undef);
        assert!(!u.defined());
    }
}
