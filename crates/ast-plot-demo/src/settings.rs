// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! Plot attributes set from a string in the C library's `astSet` syntax:
//! `Name=value` settings separated by commas, as in
//! `Grid=1,Labelling=interior,Colour(axes)=3`, with the C library's names,
//! abbreviations and errors. A Frame or FrameSet attribute reaches the
//! Plot's current Frame, as it does in C.

use starlink_ast::attributes::Attributes;
use starlink_ast::error::AstResult;
use starlink_ast::plot::Plot;

/// Apply the settings in `settings` to `plot`, in order, stopping at the
/// first that fails.
///
/// # Errors
///
/// The C library's error for the first setting that fails.
pub fn apply_settings(plot: &mut Plot, settings: &str) -> AstResult<()> {
    plot.set_attributes(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cases::{build_plot, cases, graphics_box, plot_box};
    use starlink_ast::error::ErrorCode;
    use starlink_ast::plot::Plot;

    fn plot() -> Plot {
        let case = &cases()[0];
        build_plot(case, "", graphics_box(800.0, 600.0, plot_box(case))).unwrap()
    }

    #[test]
    fn settings_use_the_c_library_rules() {
        // C's Plot accepts the unique abbreviation "int" and reports a bad
        // one with its own message; Frame attributes reach the current
        // Frame.
        let mut plot = plot();
        apply_settings(&mut plot, "Labelling=int,System=ICRS,Label(1)=First axis").unwrap();
        assert_eq!(plot.get_attribute("Label(1)").unwrap(), "First axis");
        let err = apply_settings(&mut plot, "Labelling=sideways").unwrap_err();
        assert_eq!(err.code(), ErrorCode::Opt);
    }
}
