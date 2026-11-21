// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! The page's example headers, read from `web/plotter/examples.txt`.

/// One example: a FITS header and the Plot settings that go with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    /// The name shown on the page's button, such as `AITOFF`.
    pub name: String,
    /// The Plot settings.
    pub settings: String,
    /// The version character: `' '` for the primary axes.
    pub version: char,
    /// The header, one card per line, each line ending in a newline.
    pub header: String,
}

/// The examples in `text`, in order.
///
/// An example starts at a line `== NAME`, followed by `-- settings: ...` and
/// `-- version: ...` lines (a blank version is `' '`) and then the header's
/// lines. Lines before the first `== ` line, such as comments, are ignored.
pub fn parse_examples(text: &str) -> Vec<Example> {
    let mut examples: Vec<Example> = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("== ") {
            examples.push(Example {
                name: name.trim().to_string(),
                settings: String::new(),
                version: ' ',
                header: String::new(),
            });
        } else if let Some(example) = examples.last_mut() {
            if let Some(settings) = line.strip_prefix("-- settings:") {
                example.settings = settings.strip_prefix(' ').unwrap_or(settings).to_string();
            } else if let Some(version) = line.strip_prefix("-- version:") {
                example.version = version.trim_start().chars().next().unwrap_or(' ');
            } else {
                example.header.push_str(line);
                example.header.push('\n');
            }
        }
    }
    examples
}
