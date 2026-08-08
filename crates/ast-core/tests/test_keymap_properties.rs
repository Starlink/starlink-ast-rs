//! Property tests for `KeyMap`.
//!
//! The two invariants that make the `HashMap`-backed storage safe: a dump is
//! independent of insertion order, and a dump/load cycle is a fixed point.

use ast_core::constants::BAD_VALUE;
use ast_core::keymap::KeyMap;
use ast_core::{AnyObject, AstType, SortBy, from_native_string, to_native_string};

/// A deterministic xorshift generator. Fixed seeds keep a CI failure
/// reproducible; `HashMap`'s own iteration order varies per process, which is
/// exactly what these tests are here to defeat.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // A zero state is a fixed point for xorshift, so avoid it.
        Rng(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}

/// One instruction for building a KeyMap. Ordering these differently is the
/// whole point, so every variant must be order-insensitive in its effect.
#[derive(Clone, Debug)]
enum Put {
    Int(String, i32),
    Short(String, i16),
    Long(String, i64),
    Byte(String, u8),
    Double(String, f64),
    Float(String, f32),
    Str(String, String),
    IntVec(String, Vec<i32>),
    StrVec(String, Vec<String>),
    DoubleVec(String, Vec<f64>),
    Undef(String),
    Nested(String, Vec<Put>),
}

/// Generate a random put list with unique keys. Keys are drawn to exercise
/// KeyCmp's digit handling and its case folding: mixed case, embedded
/// numbers, and lengths either side of a common prefix.
fn gen_puts(rng: &mut Rng, count: usize, depth: usize) -> Vec<Put> {
    const STEMS: &[&str] = &[
        "Alpha", "beta", "GAMMA", "delta9", "Delta10", "Delta2", "eps", "Epsilon", "z", "Zeta_1",
        "item", "Item", "a1b", "a10b", "a2b",
    ];
    let mut puts = Vec::with_capacity(count);
    for i in 0..count {
        // Suffix the stem with the index so keys stay unique regardless of
        // which stems were drawn.
        let key = format!("{}{}", STEMS[rng.below(STEMS.len())], i);
        let n = 1 + rng.below(4);
        let variant = rng.below(if depth > 0 { 12 } else { 11 });
        puts.push(match variant {
            0 => Put::Int(key, rng.next_u64() as i32),
            1 => Put::Short(key, rng.next_u64() as i16),
            2 => Put::Long(key, rng.next_u64() as i64),
            3 => Put::Byte(key, rng.next_u64() as u8),
            // Values that stress the 17-significant-digit formatting, plus
            // BAD_VALUE, which is dumped as no card at all.
            4 => Put::Double(key, pick_double(rng)),
            5 => Put::Float(key, (rng.next_u64() as i32 as f32) / 7.0),
            6 => Put::Str(key, gen_string(rng)),
            7 => {
                let values = (0..n).map(|_| rng.next_u64() as i32).collect();
                Put::IntVec(key, values)
            }
            8 => {
                let values = (0..n).map(|_| gen_string(rng)).collect();
                Put::StrVec(key, values)
            }
            9 => {
                let values = (0..n).map(|_| pick_double(rng)).collect();
                Put::DoubleVec(key, values)
            }
            10 => Put::Undef(key),
            _ => {
                let inner_count = 1 + rng.below(3);
                Put::Nested(key, gen_puts(rng, inner_count, depth - 1))
            }
        });
    }
    puts
}

fn pick_double(rng: &mut Rng) -> f64 {
    const SPECIALS: &[f64] = &[
        BAD_VALUE,
        0.0,
        -0.0,
        1.0,
        -1.0,
        1e-300,
        1e300,
        std::f64::consts::PI,
    ];
    if rng.below(3) == 0 {
        SPECIALS[rng.below(SPECIALS.len())]
    } else {
        (rng.next_u64() as i64 as f64) / 1024.0
    }
}

/// Strings including the empty string, which MapGetC's fixed-stride
/// concatenation treats specially, and one with embedded spaces.
fn gen_string(rng: &mut Rng) -> String {
    const CHOICES: &[&str] = &["", "a", "Hello", "with space", "0123456789", "]"];
    CHOICES[rng.below(CHOICES.len())].to_string()
}

fn build(puts: &[Put]) -> KeyMap {
    let mut km = KeyMap::new();
    apply(&mut km, puts);
    km
}

/// The comment to attach to `key`'s entry, derived from the key text alone
/// so it stays fixed across a reshuffle — the whole point of these tests is
/// that content cannot depend on insertion order, and a comment tied to the
/// generation index or to `Rng` state would violate that for a reason that
/// has nothing to do with the property under test.
///
/// One case in four omits the comment, exercising the path where C's
/// `Dump` writes no `Com%d` card at all. The other three include a space,
/// and one embeds a literal double quote — the entry's comment round-trips
/// as the *value* of a `Com%d` string card, so it goes through the same
/// quoting as any other string value and a quote character inside it must
/// survive doubling on write and undoubling on read.
fn comment_for(key: &str) -> Option<String> {
    let sum: u32 = key.bytes().map(u32::from).sum();
    match sum % 4 {
        0 => None,
        1 => Some(format!("note for {key}")),
        2 => Some(format!("{key} needs a \"quoted\" aside")),
        _ => Some(format!("{key}: trailing detail")),
    }
}

fn apply(km: &mut KeyMap, puts: &[Put]) {
    for put in puts {
        let comment = key_of(put).and_then(|k| comment_for(&k));
        let comment = comment.as_deref();
        match put {
            Put::Int(k, v) => km.put_0i(k, *v, comment).unwrap(),
            Put::Short(k, v) => km.put_0s(k, *v, comment).unwrap(),
            Put::Long(k, v) => km.put_0k(k, *v, comment).unwrap(),
            Put::Byte(k, v) => km.put_0b(k, *v, comment).unwrap(),
            Put::Double(k, v) => km.put_0d(k, *v, comment).unwrap(),
            Put::Float(k, v) => km.put_0f(k, *v, comment).unwrap(),
            Put::Str(k, v) => km.put_0c(k, v, comment).unwrap(),
            Put::IntVec(k, v) => km.put_1i(k, v, comment).unwrap(),
            Put::StrVec(k, v) => {
                let refs: Vec<&str> = v.iter().map(String::as_str).collect();
                km.put_1c(k, &refs, comment).unwrap()
            }
            Put::DoubleVec(k, v) => km.put_1d(k, v, comment).unwrap(),
            Put::Undef(k) => km.put_u(k, comment).unwrap(),
            Put::Nested(k, inner) => {
                let obj = AnyObject::KeyMap(Box::new(build(inner)));
                km.put_0a(k, obj, comment).unwrap()
            }
        }
    }
}

fn dump(km: &KeyMap) -> String {
    to_native_string(&AnyObject::KeyMap(Box::new(km.clone()))).unwrap()
}

fn load(text: &str) -> KeyMap {
    match from_native_string(text).unwrap() {
        AnyObject::KeyMap(km) => *km,
        other => panic!("expected a KeyMap, got {other:?}"),
    }
}

/// Twelve fixed seeds. Enough shapes to catch a missing sort, few enough to
/// stay fast, and reproducible when one fails.
const SEEDS: &[u64] = &[1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233];

#[test]
fn the_dump_is_independent_of_insertion_order() {
    for &seed in SEEDS {
        let mut rng = Rng::new(seed);
        let count = 4 + rng.below(20);
        let puts = gen_puts(&mut rng, count, 2);
        let km = build(&puts);
        assert_eq!(
            km.map_size(),
            puts.len(),
            "seed {seed}: generator produced a duplicate key"
        );

        let reference = dump(&km);

        // Five reshuffles per seed. Every key is unique, so reordering the
        // puts cannot change the resulting content — only the member numbers,
        // which is why MemCnt is the total and not a per-entry card.
        for round in 0..5 {
            let mut shuffled = puts.clone();
            rng.shuffle(&mut shuffled);
            assert_eq!(
                dump(&build(&shuffled)),
                reference,
                "seed {seed} round {round}: dump depends on insertion order"
            );
        }
    }
}

#[test]
fn the_read_out_order_is_independent_of_insertion_order() {
    // map_key / map_iterate must sort too, not just to_node.
    for &seed in SEEDS {
        let mut rng = Rng::new(seed);
        let count = 4 + rng.below(20);
        let puts = gen_puts(&mut rng, count, 0);
        let km = build(&puts);
        assert_eq!(
            km.map_size(),
            puts.len(),
            "seed {seed}: generator produced a duplicate key"
        );
        let reference: Vec<String> = (0..km.map_size()).map(|i| km.map_key(i).unwrap()).collect();

        for round in 0..5 {
            let mut shuffled = puts.clone();
            rng.shuffle(&mut shuffled);
            let mut other = build(&shuffled);
            let keys: Vec<String> = (0..other.map_size())
                .map(|i| other.map_key(i).unwrap())
                .collect();
            assert_eq!(
                keys, reference,
                "seed {seed} round {round}: map_key order depends on insertion order"
            );

            // map_iterate must agree with map_key.
            let mut iterated = Vec::new();
            let mut reset = true;
            while let Some(key) = other.map_iterate(reset) {
                iterated.push(key);
                reset = false;
            }
            assert_eq!(iterated, reference, "seed {seed}: map_iterate disagrees");
        }
    }
}

#[test]
fn a_dump_load_cycle_is_a_fixed_point() {
    for &seed in SEEDS {
        let mut rng = Rng::new(seed);
        let count = 4 + rng.below(20);
        let puts = gen_puts(&mut rng, count, 2);
        let km = build(&puts);
        assert_eq!(
            km.map_size(),
            puts.len(),
            "seed {seed}: generator produced a duplicate key"
        );

        let once = dump(&km);
        let twice = dump(&load(&once));
        assert_eq!(twice, once, "seed {seed}: dump/load is not idempotent");

        // And stable under a third cycle, which catches an asymmetry that
        // happens to cancel on the first.
        let thrice = dump(&load(&twice));
        assert_eq!(thrice, once, "seed {seed}: third cycle diverged");
    }
}

/// Recursively dump a nested Object so that two Object entries can be
/// compared by content — `AnyObject` has no equality relation.
fn dump_object(obj: &Option<AnyObject>) -> Option<String> {
    obj.as_ref().map(|o| to_native_string(o).unwrap())
}

#[test]
fn a_dump_load_cycle_preserves_every_value() {
    for &seed in SEEDS {
        let mut rng = Rng::new(seed);
        let count = 4 + rng.below(20);
        let puts = gen_puts(&mut rng, count, 1);
        let km = build(&puts);
        assert_eq!(
            km.map_size(),
            puts.len(),
            "seed {seed}: generator produced a duplicate key"
        );
        let back = load(&dump(&km));

        assert_eq!(back.map_size(), km.map_size(), "seed {seed}: size");
        for i in 0..km.map_size() {
            let key = km.map_key(i).unwrap();
            let ty = km.map_type(&key).unwrap();
            assert_eq!(
                back.map_type(&key).unwrap(),
                ty,
                "seed {seed}: type of {key}"
            );
            assert_eq!(
                back.map_length(&key),
                km.map_length(&key),
                "seed {seed}: length of {key}"
            );
            assert_eq!(
                back.map_defined(&key).unwrap(),
                km.map_defined(&key).unwrap(),
                "seed {seed}: definedness of {key}"
            );
            assert_eq!(
                back.map_comment(&key),
                km.map_comment(&key),
                "seed {seed}: comment of {key}"
            );
            match ty {
                AstType::Int
                | AstType::SInt
                | AstType::KInt
                | AstType::Byte
                | AstType::Double
                | AstType::Float => {
                    assert_eq!(
                        back.get_1d(&key).unwrap(),
                        km.get_1d(&key).unwrap(),
                        "seed {seed}: numeric value of {key}"
                    );
                }
                AstType::String => {
                    assert_eq!(
                        back.get_1c(&key).unwrap(),
                        km.get_1c(&key).unwrap(),
                        "seed {seed}: text value of {key}"
                    );
                }
                AstType::Object => {
                    let original = km.get_0a(&key).unwrap();
                    let round_tripped = back.get_0a(&key).unwrap();
                    assert_eq!(
                        round_tripped.is_some(),
                        original.is_some(),
                        "seed {seed}: object presence of {key}"
                    );
                    if let (Some(original), Some(round_tripped)) = (original, round_tripped) {
                        assert_eq!(
                            dump_object(&round_tripped),
                            dump_object(&original),
                            "seed {seed}: nested object value of {key}"
                        );
                    }
                }
                AstType::Undef => {
                    // get_1c would return Ok(None) here regardless of any
                    // asymmetry, so assert on definedness directly instead
                    // of falling through to a comparison that cannot fail.
                    assert!(
                        !km.map_defined(&key).unwrap(),
                        "seed {seed}: {key} should be undefined"
                    );
                    assert!(
                        !back.map_defined(&key).unwrap(),
                        "seed {seed}: round-tripped {key} should be undefined"
                    );
                }
                AstType::BadType => {
                    unreachable!("no entry is ever stored with BadType");
                }
            }
        }
    }
}

#[test]
fn every_sort_mode_gives_a_total_order_independent_of_insertion() {
    // AgeUp / AgeDown / KeyAgeUp / KeyAgeDown depend on the member numbers,
    // which *do* depend on insertion order, so those four are checked for
    // being a stable permutation of the key set rather than for equality.
    const MODES: &[SortBy] = &[
        SortBy::None,
        SortBy::AgeUp,
        SortBy::AgeDown,
        SortBy::KeyUp,
        SortBy::KeyDown,
        SortBy::KeyAgeUp,
        SortBy::KeyAgeDown,
    ];
    for &seed in SEEDS {
        let mut rng = Rng::new(seed);
        let count = 4 + rng.below(12);
        let puts = gen_puts(&mut rng, count, 0);
        for &mode in MODES {
            let mut km = build(&puts);
            assert_eq!(
                km.map_size(),
                puts.len(),
                "seed {seed}: generator produced a duplicate key"
            );
            km.set_sort_by(mode);
            let keys: Vec<String> = (0..km.map_size()).map(|i| km.map_key(i).unwrap()).collect();

            // Every key exactly once, whatever the mode.
            let mut sorted = keys.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(
                sorted.len(),
                km.map_size(),
                "seed {seed} mode {mode:?}: keys are not a permutation"
            );

            // Reading twice gives the same order.
            let again: Vec<String> = (0..km.map_size()).map(|i| km.map_key(i).unwrap()).collect();
            assert_eq!(again, keys, "seed {seed} mode {mode:?}: order is unstable");

            // The two key-ordered modes are exact reverses of each other.
            if mode == SortBy::KeyDown {
                let mut up = build(&puts);
                up.set_sort_by(SortBy::KeyUp);
                let up_keys: Vec<String> =
                    (0..up.map_size()).map(|i| up.map_key(i).unwrap()).collect();
                let mut reversed = keys.clone();
                reversed.reverse();
                assert_eq!(
                    up_keys, reversed,
                    "seed {seed}: KeyDown is not the reverse of KeyUp"
                );
            }
        }
    }
}

#[test]
fn map_copy_is_order_independent() {
    for &seed in SEEDS {
        let mut rng = Rng::new(seed);
        let a_count = 3 + rng.below(8);
        let a_puts = gen_puts(&mut rng, a_count, 1);
        let b_count = 3 + rng.below(8);
        let mut b_puts = gen_puts(&mut rng, b_count, 1);
        // Force an overlap so the replace path is exercised, on every seed
        // (not only the ones where the first key of b happens to be an Int).
        if let (Some(first), Some(target)) = (a_puts.first().cloned(), b_puts.first_mut()) {
            if let Some(src_key) = key_of(&first) {
                *key_of_mut(target) = src_key;
            }
        }

        let mut dest = build(&a_puts);
        dest.map_copy(&build(&b_puts)).unwrap();
        let reference = dump(&dest);

        for round in 0..3 {
            let mut a = a_puts.clone();
            let mut b = b_puts.clone();
            rng.shuffle(&mut a);
            rng.shuffle(&mut b);
            let mut dest = build(&a);
            dest.map_copy(&build(&b)).unwrap();
            assert_eq!(
                dump(&dest),
                reference,
                "seed {seed} round {round}: map_copy depends on insertion order"
            );
        }
    }
}

fn key_of(put: &Put) -> Option<String> {
    Some(match put {
        Put::Int(k, _)
        | Put::Short(k, _)
        | Put::Long(k, _)
        | Put::Byte(k, _)
        | Put::Double(k, _)
        | Put::Float(k, _)
        | Put::Str(k, _)
        | Put::IntVec(k, _)
        | Put::StrVec(k, _)
        | Put::DoubleVec(k, _)
        | Put::Undef(k)
        | Put::Nested(k, _) => k.clone(),
    })
}

/// Mutable counterpart of [`key_of`], covering every variant so the forced
/// overlap in [`map_copy_is_order_independent`] fires regardless of which
/// variant the generator drew for `target`.
fn key_of_mut(put: &mut Put) -> &mut String {
    match put {
        Put::Int(k, _)
        | Put::Short(k, _)
        | Put::Long(k, _)
        | Put::Byte(k, _)
        | Put::Double(k, _)
        | Put::Float(k, _)
        | Put::Str(k, _)
        | Put::IntVec(k, _)
        | Put::StrVec(k, _)
        | Put::DoubleVec(k, _)
        | Put::Undef(k)
        | Put::Nested(k, _) => k,
    }
}

/// A re-put changes `MemCnt` differently from a fresh put: `insert_entry`
/// bumps `member_count` once for a key already present, versus twice for a
/// key that is new. Every test above builds from a set of distinct keys, so
/// every put in it is a fresh insert and this distinction never fires: the
/// count the load path recomputes by replaying puts happens to equal the
/// count the dump carried regardless of whether the carried total is used.
/// This test applies a second, deterministic pass of puts that reuse keys
/// already in the map, so at least one put in the overall sequence is a
/// genuine re-put, and checks that the dump/load cycle is still a fixed
/// point and still preserves every value and comment.
#[test]
fn a_dump_load_cycle_is_a_fixed_point_after_a_reput() {
    for &seed in SEEDS {
        let mut rng = Rng::new(seed);
        let count = 4 + rng.below(12);
        let puts = gen_puts(&mut rng, count, 1);
        let mut km = build(&puts);
        assert_eq!(
            km.map_size(),
            puts.len(),
            "seed {seed}: generator produced a duplicate key"
        );

        // A second, fixed-order batch that retargets each generated put
        // onto a key already in the map, so every one of these is a
        // re-put rather than an insert. Not shuffled: the point is a
        // single deterministic order of re-puts, not order-independence.
        let reput_count = 1 + rng.below(6);
        let mut reputs = gen_puts(&mut rng, reput_count, 0);
        for (i, put) in reputs.iter_mut().enumerate() {
            *key_of_mut(put) = key_of(&puts[i % puts.len()]).unwrap();
        }
        apply(&mut km, &reputs);
        assert_eq!(
            km.map_size(),
            puts.len(),
            "seed {seed}: a re-put changed the key count"
        );

        let once = dump(&km);
        let back = load(&once);
        let twice = dump(&back);
        assert_eq!(
            twice, once,
            "seed {seed}: dump/load is not a fixed point after a re-put"
        );

        for i in 0..km.map_size() {
            let key = km.map_key(i).unwrap();
            let ty = km.map_type(&key).unwrap();
            assert_eq!(
                back.map_type(&key).unwrap(),
                ty,
                "seed {seed}: type of {key}"
            );
            assert_eq!(
                back.map_comment(&key),
                km.map_comment(&key),
                "seed {seed}: comment of {key}"
            );
            match ty {
                AstType::Int
                | AstType::SInt
                | AstType::KInt
                | AstType::Byte
                | AstType::Double
                | AstType::Float => {
                    assert_eq!(
                        back.get_1d(&key).unwrap(),
                        km.get_1d(&key).unwrap(),
                        "seed {seed}: numeric value of {key}"
                    );
                }
                AstType::String => {
                    assert_eq!(
                        back.get_1c(&key).unwrap(),
                        km.get_1c(&key).unwrap(),
                        "seed {seed}: text value of {key}"
                    );
                }
                AstType::Object => {
                    let original = km.get_0a(&key).unwrap();
                    let round_tripped = back.get_0a(&key).unwrap();
                    assert_eq!(
                        round_tripped.is_some(),
                        original.is_some(),
                        "seed {seed}: object presence of {key}"
                    );
                    if let (Some(original), Some(round_tripped)) = (original, round_tripped) {
                        assert_eq!(
                            dump_object(&round_tripped),
                            dump_object(&original),
                            "seed {seed}: nested object value of {key}"
                        );
                    }
                }
                AstType::Undef => {
                    assert!(
                        !km.map_defined(&key).unwrap(),
                        "seed {seed}: {key} should be undefined"
                    );
                    assert!(
                        !back.map_defined(&key).unwrap(),
                        "seed {seed}: round-tripped {key} should be undefined"
                    );
                }
                AstType::BadType => {
                    unreachable!("no entry is ever stored with BadType");
                }
            }
        }
    }
}
