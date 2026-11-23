// Copyright 2026 Association of Universities for Research in Astronomy (AURA)
// SPDX-License-Identifier: LGPL-3.0-or-later

//! A PolyMap offers an iterative inverse only when it can evaluate one.
//!
//! Ancestor: `ast_tester/testpolymapinv.c`. The iterative inverse applies
//! the forward transformation repeatedly, so it needs a forward
//! transformation and equal numbers of inputs and outputs.
//!
//! Divergences from the C test:
//! - C asks for an iterative inverse through the constructor's options
//!   string (`"IterInverse=1"`); the Rust constructor takes no options, so
//!   that case calls `set_iter_inverse` on the new PolyMap.
//! - The errors' messages are checked as well as their codes, against the C
//!   library at the pinned commit.

use starlink_ast::error::ErrorCode;
use starlink_ast::mappings::{Mapping, PolyMap, PolyTerm};

/// The polynomial 1 + 2x.
fn line() -> Vec<PolyTerm> {
    vec![
        PolyTerm {
            coeff: 1.0,
            powers: vec![0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 2.0,
            powers: vec![1],
            out_axis: 1,
        },
    ]
}

const NO_FORWARD: &str = "astSetIterInverse(PolyMap): Cannot use an iterative inverse because \
                          the PolyMap was created without a forward transformation.";

fn assert_refused(pm: &mut PolyMap, message: &str) {
    let err = pm.set_iter_inverse(true).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Attin);
    assert_eq!(err.message(), message);
    assert!(!pm.iter_inverse());
}

#[test]
fn empty_polymap_has_no_iterative_inverse() {
    let mut pm = PolyMap::new(1, 1, &[], None).unwrap();
    assert!(!pm.has_forward());
    assert!(!pm.iter_inverse());
    assert!(!pm.has_inverse());
    assert_eq!(
        pm.tran1(&[0.5], false).unwrap_err().code(),
        ErrorCode::Trnnd
    );

    pm.invert();
    assert!(!pm.has_forward());
    assert_eq!(pm.tran1(&[0.5], true).unwrap_err().code(), ErrorCode::Trnnd);
    assert_refused(&mut pm, NO_FORWARD);
}

#[test]
fn inverse_only_polymap_uses_its_inverse_coefficients() {
    let mut pm = PolyMap::new(1, 1, &[], Some(&line())).unwrap();
    assert!(!pm.has_forward());
    assert!(!pm.iter_inverse());
    assert!(pm.has_inverse());
    assert!((pm.tran1(&[0.5], false).unwrap()[0] - 2.0).abs() <= 1.0e-9);

    assert_refused(&mut pm, NO_FORWARD);
    assert!((pm.tran1(&[0.5], false).unwrap()[0] - 2.0).abs() <= 1.0e-9);
}

#[test]
fn new_inverse_only_polymap_refuses_an_iterative_inverse() {
    // C: astPolyMap( 1, 1, 0, NULL, 2, coeff, "IterInverse=1" ).
    let mut pm = PolyMap::new(1, 1, &[], Some(&line())).unwrap();
    assert_refused(&mut pm, NO_FORWARD);
}

#[test]
fn forward_only_polymap_has_an_iterative_inverse() {
    let pm = PolyMap::new(1, 1, &line(), None).unwrap();
    assert!(pm.iter_inverse());
    assert!(pm.has_inverse());
    assert!((pm.tran1(&[0.5], true).unwrap()[0] - 2.0).abs() <= 1.0e-9);
    assert!((pm.tran1(&[2.0], false).unwrap()[0] - 0.5).abs() <= 1.0e-9);
}

#[test]
fn polymap_with_unequal_axes_has_no_iterative_inverse() {
    let terms = [
        PolyTerm {
            coeff: 2.0,
            powers: vec![1, 0],
            out_axis: 1,
        },
        PolyTerm {
            coeff: 3.0,
            powers: vec![0, 1],
            out_axis: 1,
        },
    ];
    let mut pm = PolyMap::new(2, 1, &terms, None).unwrap();
    assert!(!pm.iter_inverse());
    assert!(!pm.has_inverse());
    assert_refused(
        &mut pm,
        "astSetIterInverse(PolyMap): Cannot use an iterative inverse because the PolyMap \
         has unequal numbers of inputs and outputs.",
    );
    pm.set_iter_inverse(false).unwrap();
}

#[test]
fn loaded_empty_polymap_has_no_inverse() {
    let obj =
        starlink_ast::from_native_string(" Begin PolyMap\n Nin = 1\n IsA Mapping\n End PolyMap\n")
            .unwrap()
            .into_mapping()
            .unwrap();
    assert!(!obj.has_inverse());
}

#[test]
fn dump_cannot_ask_for_an_iterative_inverse_without_a_forward() {
    let err = starlink_ast::from_native_string(
        " Begin PolyMap\n Nin = 1\n IsA Mapping\n IterInv = 1\n End PolyMap\n",
    )
    .unwrap_err();
    assert_eq!(err.code(), ErrorCode::Attin);
}
