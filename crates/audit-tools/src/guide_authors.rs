// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Authorship of the user guide's chapters, from `git blame` on SUN/211.
//!
//! Each guide chapter is ported from a range of lines of `sun_master.tex` and
//! is credited to the people who wrote those lines. A person's share is the
//! fraction of the range's counted lines that blame attributes to them; the
//! counted lines are the C edition's (what `selectfc` keeps), excluding blank
//! lines and LaTeX comments.

/// A person credited for guide text, matched in blame output by surname.
struct Writer {
    surname: &'static str,
    identity: &'static str,
}

const WRITERS: [Writer; 2] = [
    Writer {
        surname: "Berry",
        identity: "David Berry <davidstuartberry@gmail.com>",
    },
    Writer {
        surname: "Warren-Smith",
        identity: "Rodney Warren-Smith <rfws@star.rl.ac.uk>",
    },
];

/// The share below which a writer is not credited.
pub const CREDIT_THRESHOLD: f64 = 0.10;

/// Each blame author's share of the counted lines whose final line number is
/// in `start..=end` (1-based), sorted by descending share.
///
/// `blame_porcelain` is the output of `git blame --line-porcelain` on the whole
/// file, since whether a line is in a Fortran-only block depends on the lines
/// before it.
pub fn shares(blame_porcelain: &str, start: usize, end: usize) -> Vec<(String, f64)> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    let mut total = 0usize;
    let mut line_no = 0usize;
    let mut author = String::new();
    let mut fortran = false;
    for line in blame_porcelain.lines() {
        if let Some(text) = line.strip_prefix('\t') {
            let marker = text.trim_end();
            if marker == "f+" || marker == "c+" {
                fortran = marker == "f+";
                continue;
            }
            if marker == "f-" || marker == "c-" {
                fortran = false;
                continue;
            }
            let counted = !fortran
                && (start..=end).contains(&line_no)
                && !text.trim().is_empty()
                && !text.trim_start().starts_with('%');
            if counted {
                total += 1;
                match counts.iter_mut().find(|(a, _)| *a == author) {
                    Some((_, n)) => *n += 1,
                    None => counts.push((author.clone(), 1)),
                }
            }
        } else if let Some(name) = line.strip_prefix("author ") {
            author = name.to_string();
        } else if let Some(n) = line.split(' ').nth(2) {
            // A header line: `<sha> <orig-line> <final-line> [<group-size>]`.
            if let Ok(n) = n.parse() {
                line_no = n;
            }
        }
    }
    let mut out: Vec<(String, f64)> = counts
        .into_iter()
        .map(|(a, n)| (a, n as f64 / total as f64))
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

/// The commit author and co-author for a chapter with these shares.
///
/// Shares of blame names with the same writer's surname are added together.
/// Writers at or above [`CREDIT_THRESHOLD`] are credited, the larger share as
/// author; anyone else contributes to the total but is never credited.
pub fn credit(shares: &[(String, f64)]) -> (Option<String>, Option<String>) {
    let mut credited: Vec<(&str, f64)> = WRITERS
        .iter()
        .map(|w| {
            let share = shares
                .iter()
                .filter(|(name, _)| name.contains(w.surname))
                .map(|(_, s)| s)
                .sum::<f64>();
            (w.identity, share)
        })
        .filter(|(_, share)| *share >= CREDIT_THRESHOLD)
        .collect();
    credited.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut names = credited.into_iter().map(|(id, _)| id.to_string());
    (names.next(), names.next())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
1111111111111111111111111111111111111111 1 1 1
author David Berry
\tA line by David.
1111111111111111111111111111111111111111 2 2 1
author David Berry
\tf+
2222222222222222222222222222222222222222 3 3 1
author David Berry
\tA Fortran line.
1111111111111111111111111111111111111111 4 4 1
author David Berry
\tf-
3333333333333333333333333333333333333333 5 5 1
author Rodney Warren-Smith
\t% a comment
3333333333333333333333333333333333333333 6 6 1
author Rodney Warren-Smith
\tA line by Rodney.
1111111111111111111111111111111111111111 7 7 1
author David Berry
\t
1111111111111111111111111111111111111111 8 8 1
author David Berry
\tAnother line by David.
";

    #[test]
    fn fortran_blocks_and_comments_are_not_counted() {
        let s = shares(SAMPLE, 1, 8);
        assert_eq!(
            s,
            vec![
                ("David Berry".to_string(), 2.0 / 3.0),
                ("Rodney Warren-Smith".to_string(), 1.0 / 3.0)
            ]
        );
    }

    #[test]
    fn only_lines_in_the_range_are_counted() {
        let s = shares(SAMPLE, 6, 6);
        assert_eq!(s, vec![("Rodney Warren-Smith".to_string(), 1.0)]);
    }

    #[test]
    fn under_ten_percent_is_not_credited() {
        let (a, c) = credit(&[
            ("Rodney Warren-Smith".into(), 0.91),
            ("David Berry".into(), 0.09),
        ]);
        assert_eq!(
            a.as_deref(),
            Some("Rodney Warren-Smith <rfws@star.rl.ac.uk>")
        );
        assert_eq!(c, None);
    }

    #[test]
    fn larger_share_is_author() {
        let (a, c) = credit(&[
            ("David Berry".into(), 0.6),
            ("Rodney Warren-Smith".into(), 0.3),
        ]);
        assert_eq!(
            a.as_deref(),
            Some("David Berry <davidstuartberry@gmail.com>")
        );
        assert_eq!(
            c.as_deref(),
            Some("Rodney Warren-Smith <rfws@star.rl.ac.uk>")
        );
    }

    #[test]
    fn other_authors_dilute_but_are_not_credited() {
        let (a, c) = credit(&[
            ("Tim Jenness".into(), 0.5),
            ("David Berry".into(), 0.45),
            ("Rodney Warren-Smith".into(), 0.05),
        ]);
        assert_eq!(
            a.as_deref(),
            Some("David Berry <davidstuartberry@gmail.com>")
        );
        assert_eq!(c, None);
    }

    #[test]
    fn one_writer_can_have_two_blame_names() {
        let (a, c) = credit(&[
            ("David S. Berry".into(), 0.3),
            ("Rodney Warren-Smith".into(), 0.4),
            ("David Berry".into(), 0.3),
        ]);
        assert_eq!(
            a.as_deref(),
            Some("David Berry <davidstuartberry@gmail.com>")
        );
        assert_eq!(
            c.as_deref(),
            Some("Rodney Warren-Smith <rfws@star.rl.ac.uk>")
        );
    }
}
