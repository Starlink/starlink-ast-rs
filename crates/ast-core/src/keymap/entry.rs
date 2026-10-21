//! Entry storage: the AST type codes, the scalar-or-vector payload, and
//! the per-entry bookkeeping C keeps in `struct AstMapEntry`.

use crate::keymap::KeyMap;
use crate::keymap::convert::RawValue;
use crate::serial::AnyObject;

/// The type of the value stored in a KeyMap entry, as
/// [`KeyMap::map_type`] gives it.
///
/// # See also
///
/// The C constants `AST__BADTYPE`, `AST__INTTYPE`, `AST__DOUBLETYPE`,
/// `AST__STRINGTYPE`, `AST__OBJECTTYPE`, `AST__FLOATTYPE`, `AST__SINTTYPE`,
/// `AST__UNDEFTYPE`, `AST__BYTETYPE` and `AST__KINTTYPE`, one per variant
/// in that order, whose values [`code`](Self::code) gives.
/// `AST__POINTERTYPE`, for a generic pointer, has no variant.
// The discriminants are C's AST__*TYPE constants from ast.h.
// AST__POINTERTYPE (6) holds a raw C pointer and is not representable in the
// Rust API, so its code is reserved but unhandled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AstType {
    /// No type: what [`KeyMap::map_type`] gives for a key that is not in
    /// the KeyMap.
    /// No entry has this type, so it is not in [`AstType::ALL`] and
    /// [`AstType::from_code`] does not return it.
    BadType = 0,
    /// `i32`.
    Int = 1,
    /// `f64`.
    Double = 2,
    /// A string.
    String = 3,
    /// An AST Object ([`AnyObject`]).
    Object = 4,
    /// `f32`.
    Float = 5,
    /// `i16`.
    SInt = 7,
    /// No value: the key is in the KeyMap, stored by [`KeyMap::put_u`].
    Undef = 8,
    /// `u8`.
    Byte = 9,
    /// `i64`.
    KInt = 10,
}

impl AstType {
    /// Every type an entry can have, in ascending code order.
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

    /// The integer code of this type, which a dumped KeyMap records for each
    /// entry.
    pub fn code(self) -> i64 {
        self as i64
    }

    /// The type with integer code `code` (see [`code`](Self::code)), or
    /// `None` for 0 ([`BadType`](Self::BadType)), for 6 (a generic pointer,
    /// which has no variant), and for any code not in [`AstType::ALL`].
    pub fn from_code(code: i64) -> Option<AstType> {
        AstType::ALL.into_iter().find(|t| t.code() == code)
    }

    /// The comment a dumped KeyMap writes beside an entry's type.
    ///
    /// # Panics
    ///
    /// If `self` is [`BadType`](Self::BadType), which no entry has.
    // The comment on the Typ%d card, as DumpEntry in keymap.c writes it.
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
            AstType::BadType => unreachable!("no KeyMap entry is ever stored as BadType"),
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

/// Wrap a single value as a length-1 vector `EntryValue`, matching the
/// `astMapPut1<X>( this, key, 1, &value, ... )` call `MAKE_MAPPUTELEM`
/// makes to create an absent or undefined key (`keymap.c:8271`).
///
/// Panics if `raw` is `RawValue::Undef`: `astMapPutElem<X>` is only ever
/// generated for the eight typed `X`, never for undefined values, so a
/// caller reaching this with `Undef` is a programming error rather than
/// a case C's macro family has to handle.
pub(crate) fn single_element_vector(raw: &RawValue) -> EntryValue {
    match raw {
        RawValue::Int(v) => EntryValue::Int(Scalar1D::Vector(vec![*v])),
        RawValue::SInt(v) => EntryValue::SInt(Scalar1D::Vector(vec![*v])),
        RawValue::KInt(v) => EntryValue::KInt(Scalar1D::Vector(vec![*v])),
        RawValue::Byte(v) => EntryValue::Byte(Scalar1D::Vector(vec![*v])),
        RawValue::Float(v) => EntryValue::Float(Scalar1D::Vector(vec![*v])),
        RawValue::Double(v) => EntryValue::Double(Scalar1D::Vector(vec![*v])),
        RawValue::String(v) => EntryValue::String(Scalar1D::Vector(vec![v.clone()])),
        RawValue::Object(v) => EntryValue::Object(Scalar1D::Vector(vec![v.clone()])),
        RawValue::Undef => unreachable!("astMapPutElem<X> never puts an Undef value"),
    }
}

/// Turn a scalar `EntryValue` into a length-1 vector in place, keeping
/// its value. Mirrors the promotion `MAKE_MAPPUTELEM` performs on a
/// scalar entry before extending it (`keymap.c:8286` onward): a new
/// `Entry1<X>` is created with the scalar's own value and comment, which
/// is the caller's job — this only reshapes the payload.
///
/// Panics on an `Undef` entry: the caller only promotes a *defined*
/// scalar (`EntryValue::nel() == 0` guarded by `EntryValue::defined()`),
/// matching C's `mapentry->type == AST__UNDEFTYPE` branch being handled
/// separately by creating a fresh entry instead of promoting.
pub(crate) fn promote_scalar_to_vector(value: &mut EntryValue) {
    fn promote<T>(s: &mut Scalar1D<T>) {
        if let Scalar1D::Scalar(_) = s {
            let Scalar1D::Scalar(v) = std::mem::replace(s, Scalar1D::Vector(Vec::new())) else {
                unreachable!()
            };
            *s = Scalar1D::Vector(vec![v]);
        }
    }
    match value {
        EntryValue::Int(s) => promote(s),
        EntryValue::SInt(s) => promote(s),
        EntryValue::KInt(s) => promote(s),
        EntryValue::Byte(s) => promote(s),
        EntryValue::Float(s) => promote(s),
        EntryValue::Double(s) => promote(s),
        EntryValue::String(s) => promote(s),
        EntryValue::Object(s) => promote(s),
        EntryValue::Undef { .. } => unreachable!("an Undef entry is never promoted"),
    }
}

/// Write `raw` at zero-based `index` in `value`'s vector, writing in
/// place when `index` is already in range and appending when
/// `index == len`.
///
/// `raw` must already have been converted to `value`'s own type (by
/// [`crate::keymap::convert::convert_value`]) and `index` must already
/// be clamped to `0..=len` by the caller — `MAKE_MAPPUTELEM` only ever
/// reaches this point after computing exactly such a target
/// (`keymap.c:8397` onward, the `elem*raw_size` write). An index beyond
/// `len` is a programming error, not a user error, so this panics rather
/// than silently discarding the value.
pub(crate) fn set_element(value: &mut EntryValue, index: usize, raw: RawValue) {
    fn write<T>(s: &mut Scalar1D<T>, index: usize, v: T) {
        let Scalar1D::Vector(vec) = s else {
            unreachable!("set_element is only called on a vector entry")
        };
        match index.cmp(&vec.len()) {
            std::cmp::Ordering::Less => vec[index] = v,
            std::cmp::Ordering::Equal => vec.push(v),
            std::cmp::Ordering::Greater => {
                unreachable!("index {index} is out of range for length {}", vec.len())
            }
        }
    }
    match (value, raw) {
        (EntryValue::Int(s), RawValue::Int(v)) => write(s, index, v),
        (EntryValue::SInt(s), RawValue::SInt(v)) => write(s, index, v),
        (EntryValue::KInt(s), RawValue::KInt(v)) => write(s, index, v),
        (EntryValue::Byte(s), RawValue::Byte(v)) => write(s, index, v),
        (EntryValue::Float(s), RawValue::Float(v)) => write(s, index, v),
        (EntryValue::Double(s), RawValue::Double(v)) => write(s, index, v),
        (EntryValue::String(s), RawValue::String(v)) => write(s, index, v),
        (EntryValue::Object(s), RawValue::Object(v)) => write(s, index, v),
        (value, raw) => unreachable!(
            "set_element type mismatch: entry is {:?}, converted value is {:?}",
            value.ast_type(),
            raw.ast_type()
        ),
    }
}

/// Whether `value` is a zero-length vector.
///
/// A KeyMap entry has no representation for one: a length of zero means the
/// entry is a scalar (`InitMapEntry`, keymap.c:3900), so C rejects the put
/// with `AST__NELIN` rather than creating an entry that cannot be read back
/// or dumped (keymap.c:5044, upstream b7135abc / 9e763e45). Before that fix
/// the entry was created and its value read out of an all-zero bit pattern,
/// which for a String entry handed `astWriteString` a NULL and crashed.
pub(crate) fn is_empty_vector(value: &EntryValue) -> bool {
    fn empty<T>(s: &Scalar1D<T>) -> bool {
        matches!(s, Scalar1D::Vector(v) if v.is_empty())
    }
    match value {
        EntryValue::Int(s) => empty(s),
        EntryValue::SInt(s) => empty(s),
        EntryValue::KInt(s) => empty(s),
        EntryValue::Byte(s) => empty(s),
        EntryValue::Float(s) => empty(s),
        EntryValue::Double(s) => empty(s),
        EntryValue::String(s) => empty(s),
        EntryValue::Object(s) => empty(s),
        EntryValue::Undef { .. } => false,
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

/// The KeyMap held by a scalar or length-1-vector Object entry, if it is
/// one. Mirrors the merge test in `MapCopy` (`keymap.c:4217`), which
/// accepts `nel == 0` or `nel == 1`.
pub(crate) fn single_key_map(value: &EntryValue) -> Option<&KeyMap> {
    let obj = match value {
        EntryValue::Object(Scalar1D::Scalar(obj)) => obj.as_deref()?,
        EntryValue::Object(Scalar1D::Vector(v)) if v.len() == 1 => v[0].as_deref()?,
        _ => return None,
    };
    match obj {
        AnyObject::KeyMap(km) => Some(km),
        _ => None,
    }
}

/// The mutable counterpart of [`single_key_map`].
pub(crate) fn single_key_map_mut(value: &mut EntryValue) -> Option<&mut KeyMap> {
    let obj = match value {
        EntryValue::Object(Scalar1D::Scalar(obj)) => obj.as_deref_mut()?,
        EntryValue::Object(Scalar1D::Vector(v)) if v.len() == 1 => v[0].as_deref_mut()?,
        _ => return None,
    };
    match obj {
        AnyObject::KeyMap(km) => Some(km),
        _ => None,
    }
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
