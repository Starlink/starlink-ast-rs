//! `CmpRegion::reg_cmp_region_list` against the C library's protected
//! `astCmpRegionList` (cmpregion.c `CmpRegionList` and `XORCheck`): nested
//! ANDs and ORs, an OR containing an AND, an XOR, an OR containing an XOR,
//! and an OR of two ANDs equivalent to an XOR; and the dump of that last
//! CmpRegion, which cmpregion.c `Dump` writes as an XOR.
//!
//! The values come from `tools/oracle_fixtures/region_cmplist_probe.c`.

use crate::common::native_dump;
// Generated; formatted by its generator rather than rustfmt.
#[rustfmt::skip]
#[path = "region_cmplist_oracle/cases.rs"]
mod cases;

use starlink_ast::AnyObject;
use starlink_ast::frames::BasicFrameBuilder;
use starlink_ast::regions::{
    CircleBuilder, CmpOperator, CmpRegion, CmpRegionBuilder, DynRegion, Region,
};

fn circle(x: f64) -> DynRegion {
    Box::new(
        CircleBuilder::new(BasicFrameBuilder::new(2).build().unwrap(), vec![x, 0.0])
            .unwrap()
            .radius(1.0)
            .build()
            .unwrap(),
    )
}

fn cmp(a: DynRegion, b: DynRegion, op: CmpOperator) -> CmpRegion {
    CmpRegionBuilder::from_dyn(a, b, op)
        .unwrap()
        .build()
        .unwrap()
}

fn negated(r: DynRegion) -> DynRegion {
    r.negated_copy()
}

fn hand_xor() -> CmpRegion {
    let (b, c) = (circle(3.0), circle(6.0));
    cmp(
        Box::new(cmp(
            b.clone_region(),
            negated(c.clone_region()),
            CmpOperator::And,
        )),
        Box::new(cmp(negated(b), c, CmpOperator::And)),
        CmpOperator::Or,
    )
}

fn dump(r: &dyn Region) -> String {
    native_dump(&AnyObject::from_mapping(r).unwrap()).unwrap()
}

#[test]
fn region_lists_match_the_c_library() {
    let (a, b, c) = (|| circle(0.0), || circle(3.0), || circle(6.0));
    for &(name, oper, want) in cases::CASES {
        let cr = match name {
            "a_and_b_and_c" => cmp(
                a(),
                Box::new(cmp(b(), c(), CmpOperator::And)),
                CmpOperator::And,
            ),
            "a_or_b_or_c" => cmp(
                Box::new(cmp(a(), b(), CmpOperator::Or)),
                c(),
                CmpOperator::Or,
            ),
            "a_or_b_and_c" => cmp(
                a(),
                Box::new(cmp(b(), c(), CmpOperator::And)),
                CmpOperator::Or,
            ),
            "b_xor_c" => cmp(b(), c(), CmpOperator::Xor),
            "a_or_b_xor_c" => cmp(
                a(),
                Box::new(cmp(b(), c(), CmpOperator::Xor)),
                CmpOperator::Or,
            ),
            "hand_xor" => hand_xor(),
            other => panic!("no CmpRegion for case {other}"),
        };
        let (got_oper, list) = cr.reg_cmp_region_list();
        let code = match got_oper {
            CmpOperator::And => 1,
            CmpOperator::Or => 2,
            CmpOperator::Xor => 3,
        };
        assert_eq!(code, oper, "{name}: operator");
        let got: Vec<String> = list.iter().map(|r| dump(r.as_ref())).collect();
        assert_eq!(got, want, "{name}: Regions");
    }
}

#[test]
fn equivalent_xor_dumps_as_xor() {
    assert_eq!(dump(&hand_xor()), cases::HAND_XOR_DUMP);
}
