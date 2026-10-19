//! The comparator the simplify regression and the wcsconv harness use for
//! dumps: byte-equal except for numbers within a tolerance, so that what it
//! lets through is only last-bit numeric drift.

use crate::common::{native_strings_astequal, native_strings_equivalent};

fn eq(a: &str, b: &str) -> bool {
    native_strings_equivalent(a, b, 4)
}

#[test]
fn identical_dumps_match() {
    assert!(eq(
        " Begin ZoomMap\n    Zoom = 2\n",
        " Begin ZoomMap\n    Zoom = 2\n"
    ));
}

#[test]
fn a_number_a_few_ulp_away_matches() {
    assert!(eq("    Zoom = 0.30000000000000004\n", "    Zoom = 0.3\n"));
    assert!(native_strings_astequal(
        "    Zoom = 1.0000000000001\n",
        "    Zoom = 1\n"
    ));
}

#[test]
fn whitespace_must_match() {
    assert!(!eq(
        "    Zoom = 2 \t# Zoom factor\n",
        "    Zoom = 2 \t#  Zoom factor\n"
    ));
    assert!(!eq("    Zoom =  2\n", "    Zoom = 2\n"));
    assert!(!eq(" Begin ZoomMap\n", " Begin ZoomMap \n"));
    assert!(
        !eq("\n Begin ZoomMap\n", " Begin ZoomMap\n"),
        "leading text is not trimmed"
    );
    assert!(
        !eq(" Begin ZoomMap\n", " Begin ZoomMap"),
        "the final newline counts"
    );
}

#[test]
fn the_same_value_written_another_way_is_a_difference() {
    assert!(!eq("    Zoom = 1.0\n", "    Zoom = 1\n"));
    assert!(!eq("    Zoom = 1e3\n", "    Zoom = 1000\n"));
    assert!(!native_strings_astequal(
        "    Zoom = 1.0\n",
        "    Zoom = 1\n"
    ));
}

#[test]
fn padding_may_realign_a_differing_number_in_its_field_only() {
    // A right-justified FITS value that gained a digit keeps its width.
    assert!(eq(
        "M0_B    = -0.29008032260091554 / Forward matrix value\n",
        "M0_B    =  -0.2900803226009156 / Forward matrix value\n",
    ));
    // The same padding change with the number's width unchanged is not.
    assert!(!eq(
        "M0_B    =  -0.29008032260091554 / Forward matrix value\n",
        "M0_B    = -0.2900803226009156 / Forward matrix value\n",
    ));
    // A free-format card whose number gained a character keeps its 80
    // columns by losing a trailing space.
    assert!(native_strings_astequal(
        "IM1_A   = 1.3877787807814457E-17 / Inverse matrix value  \n",
        "IM1_A   = -1.3877787807814457E-17 / Inverse matrix value \n",
    ));
    // Nor is padding before a number that matches exactly.
    assert!(!eq("M0_B    =  -0.5 / x\n", "M0_B    = -0.5 / x\n"));
}
