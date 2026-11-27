// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Cutting text at the byte positions the C library cuts it at.
//!
//! The C library counts columns and lengths in bytes. The port keeps those
//! counts, and where one falls inside a character of a `str` the cut moves
//! back to the start of that character, so no character is split.

/// The largest index no greater than `i` that starts a character of `s`, or
/// the length of `s` if `i` is beyond it.
pub(crate) fn floor_char_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::floor_char_boundary;

    #[test]
    fn a_cut_inside_a_character_moves_to_its_start() {
        let s = "ab\u{e9}c";
        assert_eq!(floor_char_boundary(s, 2), 2);
        assert_eq!(floor_char_boundary(s, 3), 2);
        assert_eq!(floor_char_boundary(s, 4), 4);
        assert_eq!(floor_char_boundary(s, 99), 5);
    }
}
