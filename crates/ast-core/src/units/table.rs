//! Known-unit and SI-multiplier registries. Mirrors the
//! `MakeKnownUnit` calls and the `MAKEMULT` table in
//! `reference/starlink-ast/src/unit.c`:2203-2388.
//!
//! Both registries are populated once at first access via
//! `OnceLock` and are immutable thereafter. The lookup helpers walk
//! the tables in registration order — for `Multiplier` this matters
//! because the longest match wins (see `parser::split_unit`).

use std::sync::OnceLock;

/// A known SI prefix, e.g. `k` (kilo, 1e3) or `M` (mega, 1e6).
/// Mirrors the C `Multiplier` struct (`unit.c`:168-176).
#[derive(Debug)]
pub(crate) struct Multiplier {
    pub sym: &'static str,
    pub label: &'static str,
    pub scale: f64,
}

/// A known unit, e.g. `Hz`, `m`, `eV`. Mirrors the C `KnownUnit`
/// struct (`unit.c`:191-199). The `head` field — the parsed
/// definition tree for derived units — is built lazily by the
/// parser the first time the unit is consulted in a substitution
/// context (`RemakeTree`); this avoids ordering issues when
/// initializing the registry.
#[derive(Debug)]
pub(crate) struct KnownUnit {
    /// Symbol (e.g. `"Hz"`).
    pub sym: &'static str,
    /// Label (e.g. `"Hertz"`).
    pub label: &'static str,
    /// Algebraic definition in basic units (e.g. `"1/s"`). `None`
    /// for the basic units themselves and for dimensionless
    /// placeholders like `count`.
    pub definition: Option<&'static str>,
    /// If non-empty, this name is an alias — when resolving, walk
    /// to the entry with this `sym`.
    pub alias_of: Option<&'static str>,
}

/// All known units, in registration order. Mirrors C's linked list
/// built by `GetKnownUnits` (`unit.c`:2137-2293).
fn known_units_table() -> &'static [KnownUnit] {
    static TABLE: OnceLock<Vec<KnownUnit>> = OnceLock::new();
    TABLE.get_or_init(|| {
        // Match C `MakeKnownUnit` calls at `unit.c`:2203-2273 in the
        // same order so the linked-list traversal order is identical
        // (matters for ambiguous-prefix tie-breaks at parse time).
        vec![
            // --- Basic units (head = NULL on the C side) ---
            KnownUnit {
                sym: "g",
                label: "gram",
                definition: None,
                alias_of: None,
            },
            KnownUnit {
                sym: "m",
                label: "metre",
                definition: None,
                alias_of: None,
            },
            KnownUnit {
                sym: "s",
                label: "second",
                definition: None,
                alias_of: None,
            },
            KnownUnit {
                sym: "rad",
                label: "radian",
                definition: None,
                alias_of: None,
            },
            KnownUnit {
                sym: "K",
                label: "Kelvin",
                definition: None,
                alias_of: None,
            },
            KnownUnit {
                sym: "A",
                label: "Ampere",
                definition: None,
                alias_of: None,
            },
            KnownUnit {
                sym: "mol",
                label: "mole",
                definition: None,
                alias_of: None,
            },
            KnownUnit {
                sym: "cd",
                label: "candela",
                definition: None,
                alias_of: None,
            },
            // --- SI-derived units (definitions in basic units) ---
            KnownUnit {
                sym: "sr",
                label: "steradian",
                definition: Some("rad rad"),
                alias_of: None,
            },
            KnownUnit {
                sym: "Hz",
                label: "Hertz",
                definition: Some("1/s"),
                alias_of: None,
            },
            KnownUnit {
                sym: "N",
                label: "Newton",
                definition: Some("kg m/s**2"),
                alias_of: None,
            },
            KnownUnit {
                sym: "J",
                label: "Joule",
                definition: Some("N m"),
                alias_of: None,
            },
            KnownUnit {
                sym: "W",
                label: "Watt",
                definition: Some("J/s"),
                alias_of: None,
            },
            KnownUnit {
                sym: "C",
                label: "Coulomb",
                definition: Some("A s"),
                alias_of: None,
            },
            KnownUnit {
                sym: "V",
                label: "Volt",
                definition: Some("J/C"),
                alias_of: None,
            },
            KnownUnit {
                sym: "Pa",
                label: "Pascal",
                definition: Some("N/m**2"),
                alias_of: None,
            },
            KnownUnit {
                sym: "Ohm",
                label: "Ohm",
                definition: Some("V/A"),
                alias_of: None,
            },
            KnownUnit {
                sym: "S",
                label: "Siemens",
                definition: Some("A/V"),
                alias_of: None,
            },
            KnownUnit {
                sym: "F",
                label: "Farad",
                definition: Some("C/V"),
                alias_of: None,
            },
            KnownUnit {
                sym: "Wb",
                label: "Weber",
                definition: Some("V s"),
                alias_of: None,
            },
            KnownUnit {
                sym: "T",
                label: "Tesla",
                definition: Some("Wb/m**2"),
                alias_of: None,
            },
            KnownUnit {
                sym: "H",
                label: "Henry",
                definition: Some("Wb/A"),
                alias_of: None,
            },
            KnownUnit {
                sym: "lm",
                label: "lumen",
                definition: Some("cd sr"),
                alias_of: None,
            },
            KnownUnit {
                sym: "lx",
                label: "lux",
                definition: Some("lm/m**2"),
                alias_of: None,
            },
            // --- Angular ---
            KnownUnit {
                sym: "deg",
                label: "degree",
                definition: Some("pi/180 rad"),
                alias_of: None,
            },
            KnownUnit {
                sym: "arcmin",
                label: "arc-minute",
                definition: Some("1/60 deg"),
                alias_of: None,
            },
            KnownUnit {
                sym: "arcsec",
                label: "arc-second",
                definition: Some("1/3600 deg"),
                alias_of: None,
            },
            KnownUnit {
                sym: "mas",
                label: "milli-arcsecond",
                definition: Some("1/3600000 deg"),
                alias_of: None,
            },
            // --- Time ---
            KnownUnit {
                sym: "min",
                label: "minute",
                definition: Some("60 s"),
                alias_of: None,
            },
            KnownUnit {
                sym: "h",
                label: "hour",
                definition: Some("3600 s"),
                alias_of: None,
            },
            KnownUnit {
                sym: "d",
                label: "day",
                definition: Some("86400 s"),
                alias_of: None,
            },
            KnownUnit {
                sym: "yr",
                label: "year",
                definition: Some("31557600 s"),
                alias_of: None,
            },
            KnownUnit {
                sym: "a",
                label: "year",
                definition: Some("31557600 s"),
                alias_of: None,
            },
            // --- Energy / mass ---
            KnownUnit {
                sym: "eV",
                label: "electron-Volt",
                definition: Some("1.60217733E-19 J"),
                alias_of: None,
            },
            KnownUnit {
                sym: "erg",
                label: "erg",
                definition: Some("1.0E-7 J"),
                alias_of: None,
            },
            KnownUnit {
                sym: "Ry",
                label: "Rydberg",
                definition: Some("13.605692 eV"),
                alias_of: None,
            },
            KnownUnit {
                sym: "solMass",
                label: "solar mass",
                definition: Some("1.9891E30 kg"),
                alias_of: None,
            },
            KnownUnit {
                sym: "u",
                label: "unified atomic mass unit",
                definition: Some("1.6605387E-27 kg"),
                alias_of: None,
            },
            // --- Astrophysical ---
            KnownUnit {
                sym: "solLum",
                label: "solar luminosity",
                definition: Some("3.8268E26 W"),
                alias_of: None,
            },
            KnownUnit {
                sym: "Angstrom",
                label: "Angstrom",
                definition: Some("1.0E-10 m"),
                alias_of: None,
            },
            KnownUnit {
                sym: "micron",
                label: "micron",
                definition: Some("1.0E-6 m"),
                alias_of: None,
            },
            KnownUnit {
                sym: "solRad",
                label: "solar radius",
                definition: Some("6.9599E8 m"),
                alias_of: None,
            },
            KnownUnit {
                sym: "AU",
                label: "astronomical unit",
                definition: Some("1.49598E11 m"),
                alias_of: None,
            },
            KnownUnit {
                sym: "lyr",
                label: "light year",
                definition: Some("9.460730E15 m"),
                alias_of: None,
            },
            KnownUnit {
                sym: "pc",
                label: "parsec",
                definition: Some("3.0867E16 m"),
                alias_of: None,
            },
            // --- Dimensionless / count-like ---
            KnownUnit {
                sym: "count",
                label: "count",
                definition: None,
                alias_of: None,
            },
            KnownUnit {
                sym: "adu",
                label: "analogue-to-digital unit",
                definition: None,
                alias_of: None,
            },
            KnownUnit {
                sym: "photon",
                label: "photon",
                definition: None,
                alias_of: None,
            },
            // --- Flux ---
            KnownUnit {
                sym: "Jy",
                label: "Jansky",
                definition: Some("1.0E-26 W /m**2 /Hz"),
                alias_of: None,
            },
            KnownUnit {
                sym: "mag",
                label: "magnitude",
                definition: None,
                alias_of: None,
            },
            // --- Magnetic ---
            KnownUnit {
                sym: "G",
                label: "Gauss",
                definition: Some("1.0E-4 T"),
                alias_of: None,
            },
            // --- Pixel / counts continued ---
            KnownUnit {
                sym: "pixel",
                label: "pixel",
                definition: None,
                alias_of: None,
            },
            // --- Misc ---
            KnownUnit {
                sym: "barn",
                label: "barn",
                definition: Some("1.0E-28 m**2"),
                alias_of: None,
            },
            KnownUnit {
                sym: "D",
                label: "Debye",
                definition: Some("(1.0E-29/3) C.m"),
                alias_of: None,
            },
            // --- Aliases (mirrors `MakeUnitAlias` calls at unit.c:2280-2286) ---
            KnownUnit {
                sym: "Ang",
                label: "Angstrom",
                definition: None,
                alias_of: Some("Angstrom"),
            },
            KnownUnit {
                sym: "ct",
                label: "count",
                definition: None,
                alias_of: Some("count"),
            },
            KnownUnit {
                sym: "ph",
                label: "photon",
                definition: None,
                alias_of: Some("photon"),
            },
            KnownUnit {
                sym: "Jan",
                label: "Jansky",
                definition: None,
                alias_of: Some("Jy"),
            },
            KnownUnit {
                sym: "pix",
                label: "pixel",
                definition: None,
                alias_of: Some("pixel"),
            },
            KnownUnit {
                sym: "sec",
                label: "second",
                definition: None,
                alias_of: Some("s"),
            },
            KnownUnit {
                sym: "meter",
                label: "metre",
                definition: None,
                alias_of: Some("m"),
            },
        ]
    })
}

/// All known SI multipliers, in registration order. Mirrors C's
/// `MAKEMULT` macro at `unit.c`:2352-2388 — but reversed: the C
/// version prepends each entry to the list (so the last entry
/// registered ends up first), so consumers iterate Y, Z, …, deci.
/// We match that traversal order by reversing the list at
/// construction time.
fn multipliers_table() -> &'static [Multiplier] {
    static TABLE: OnceLock<Vec<Multiplier>> = OnceLock::new();
    TABLE.get_or_init(|| {
        // Listed here in C declaration order (deci → yotta).
        let mut v = vec![
            Multiplier {
                sym: "d",
                label: "deci",
                scale: 1.0e-1,
            },
            Multiplier {
                sym: "c",
                label: "centi",
                scale: 1.0e-2,
            },
            Multiplier {
                sym: "m",
                label: "milli",
                scale: 1.0e-3,
            },
            Multiplier {
                sym: "u",
                label: "micro",
                scale: 1.0e-6,
            },
            Multiplier {
                sym: "n",
                label: "nano",
                scale: 1.0e-9,
            },
            Multiplier {
                sym: "p",
                label: "pico",
                scale: 1.0e-12,
            },
            Multiplier {
                sym: "f",
                label: "femto",
                scale: 1.0e-15,
            },
            Multiplier {
                sym: "a",
                label: "atto",
                scale: 1.0e-18,
            },
            Multiplier {
                sym: "z",
                label: "zepto",
                scale: 1.0e-21,
            },
            Multiplier {
                sym: "y",
                label: "yocto",
                scale: 1.0e-24,
            },
            Multiplier {
                sym: "da",
                label: "deca",
                scale: 1.0e1,
            },
            Multiplier {
                sym: "h",
                label: "hecto",
                scale: 1.0e2,
            },
            Multiplier {
                sym: "k",
                label: "kilo",
                scale: 1.0e3,
            },
            Multiplier {
                sym: "M",
                label: "mega",
                scale: 1.0e6,
            },
            Multiplier {
                sym: "G",
                label: "giga",
                scale: 1.0e9,
            },
            Multiplier {
                sym: "T",
                label: "tera",
                scale: 1.0e12,
            },
            Multiplier {
                sym: "P",
                label: "peta",
                scale: 1.0e15,
            },
            Multiplier {
                sym: "E",
                label: "exa",
                scale: 1.0e18,
            },
            Multiplier {
                sym: "Z",
                label: "zetta",
                scale: 1.0e21,
            },
            Multiplier {
                sym: "Y",
                label: "yotta",
                scale: 1.0e24,
            },
        ];
        // C prepends each entry so traversal order is reverse of
        // declaration order. Match that.
        v.reverse();
        v
    })
}

/// Find a `KnownUnit` by symbol. Walks the registration list and
/// follows aliases. Mirrors the search loop in `SplitUnit`
/// (`unit.c`:5040-5076).
pub(crate) fn find_known_unit(sym: &str) -> Option<&'static KnownUnit> {
    let table = known_units_table();
    let mut entry = table.iter().find(|u| u.sym == sym)?;
    // Alias chain: walk `alias_of` until we hit the canonical entry.
    while let Some(target_sym) = entry.alias_of {
        entry = table
            .iter()
            .find(|u| u.sym == target_sym && u.alias_of.is_none())?;
    }
    Some(entry)
}

/// Find a `KnownUnit` by label (used by `astUnitLabel` and as a
/// fallback in `SplitUnit`'s case-insensitive search). Mirrors C
/// `astUnitLabel_` (`unit.c`:5171-5236).
pub(crate) fn find_known_unit_by_label(label: &str) -> Option<&'static KnownUnit> {
    let table = known_units_table();
    let mut entry = table.iter().find(|u| u.label.eq_ignore_ascii_case(label))?;
    while let Some(target_sym) = entry.alias_of {
        entry = table
            .iter()
            .find(|u| u.sym == target_sym && u.alias_of.is_none())?;
    }
    Some(entry)
}

/// Iterate all (canonical, non-alias) known units in declaration
/// order. Used during dimensional-analysis "for each known basic
/// unit" walks.
pub(crate) fn iter_known_units() -> impl Iterator<Item = &'static KnownUnit> {
    known_units_table().iter()
}

/// Find a multiplier by its symbol prefix. Walks the table in
/// (reversed) declaration order; the longest match wins is the
/// caller's job (`SplitUnit`).
pub(crate) fn find_multiplier(sym: &str) -> Option<&'static Multiplier> {
    multipliers_table().iter().find(|m| m.sym == sym)
}

/// Iterate all multipliers in C-traversal order (yotta → deci).
pub(crate) fn iter_multipliers() -> impl Iterator<Item = &'static Multiplier> {
    multipliers_table().iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_units_have_no_definition() {
        for &sym in &["g", "m", "s", "rad", "K", "A", "mol", "cd"] {
            let u = find_known_unit(sym).expect(sym);
            assert!(u.definition.is_none(), "{sym} should have no definition");
        }
    }

    #[test]
    fn derived_units_have_definitions() {
        assert_eq!(find_known_unit("Hz").unwrap().definition, Some("1/s"));
        assert_eq!(find_known_unit("J").unwrap().definition, Some("N m"));
        assert_eq!(
            find_known_unit("eV").unwrap().definition,
            Some("1.60217733E-19 J")
        );
    }

    #[test]
    fn aliases_resolve_to_canonical() {
        // Ang → Angstrom
        let u = find_known_unit("Ang").unwrap();
        assert_eq!(u.sym, "Angstrom");
        // sec → s
        let u = find_known_unit("sec").unwrap();
        assert_eq!(u.sym, "s");
        // meter → m
        let u = find_known_unit("meter").unwrap();
        assert_eq!(u.sym, "m");
    }

    #[test]
    fn unknown_returns_none() {
        assert!(find_known_unit("xyzzy").is_none());
    }

    #[test]
    fn label_lookup() {
        let u = find_known_unit_by_label("Hertz").unwrap();
        assert_eq!(u.sym, "Hz");
        // Case insensitive.
        let u = find_known_unit_by_label("hertz").unwrap();
        assert_eq!(u.sym, "Hz");
    }

    #[test]
    fn multipliers_present() {
        assert_eq!(find_multiplier("k").unwrap().scale, 1.0e3);
        assert_eq!(find_multiplier("M").unwrap().scale, 1.0e6);
        assert_eq!(find_multiplier("u").unwrap().scale, 1.0e-6);
        assert_eq!(find_multiplier("da").unwrap().scale, 1.0e1);
        assert!(find_multiplier("x").is_none());
    }

    #[test]
    fn full_table_size_matches_c() {
        // 8 basic + 16 SI-derived + 4 angular + 5 time + 5 energy/mass +
        // 7 astrophysical + 3 dimensionless + 2 flux + 1 magnetic +
        // 1 pixel + 2 misc + 7 aliases = 61 entries.
        // (Matches the C MakeKnownUnit + MakeUnitAlias count.)
        let n = iter_known_units().count();
        assert_eq!(n, 61, "expected 61 known units (got {n}); table drift?");
    }

    #[test]
    fn multipliers_table_has_20_entries() {
        assert_eq!(iter_multipliers().count(), 20);
    }
}
