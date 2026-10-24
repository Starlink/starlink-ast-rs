//! The read-only `IsLinear` attribute is what C's `GetIsLinear` overrides
//! report: UnitMap, ZoomMap, ShiftMap, WinMap and MatrixMap are linear, a
//! CmpMap is when both its components are, a FrameSet when its
//! base-to-current Mapping is, and a Region, being a Frame, is linear; every
//! other Mapping, a PermMap and a linear PolyMap among them, takes the base
//! class's answer and is not. The expectations are C's, from `astGetI(map,
//! "IsLinear")` on the same Mappings.

use starlink_ast::frames::{BasicFrameBuilder, FrameRef, FrameSet};
use starlink_ast::mappings::{
    CmpMap, Composition, Mapping, MatrixMap, PermMap, PolyMap, PolyTerm, ShiftMap, SphMap, UnitMap,
    WinMap, ZoomMap,
};
use starlink_ast::regions::CircleBuilder;

#[test]
fn is_linear_matches_the_c_library() {
    let frame = || BasicFrameBuilder::new(2).build().unwrap();
    let mut fs = FrameSet::new(Box::new(frame()));
    fs.add_frame(
        FrameRef::Base,
        Box::new(ZoomMap::new(2, 3.0).unwrap()),
        Box::new(frame()),
    )
    .unwrap();
    let perm = || PermMap::new(&[2, 1], &[2, 1], &[]).unwrap();
    let cases: Vec<(&str, Box<dyn Mapping>, bool)> = vec![
        ("unitmap", Box::new(UnitMap::new(2)), true),
        ("zoommap", Box::new(ZoomMap::new(2, 2.0).unwrap()), true),
        ("shiftmap", Box::new(ShiftMap::new(vec![1.0, 2.0])), true),
        (
            "winmap",
            Box::new(WinMap::new(&[1.0, 2.0], &[2.0, 3.0]).unwrap()),
            true,
        ),
        (
            "matrixmap",
            Box::new(MatrixMap::new(ndarray::array![[1.0, 2.0], [3.0, 4.0]]).unwrap()),
            true,
        ),
        ("permmap", Box::new(perm()), false),
        (
            "polymap_linear",
            Box::new(
                PolyMap::new(
                    1,
                    1,
                    &[PolyTerm {
                        coeff: 1.0,
                        powers: vec![1],
                        out_axis: 1,
                    }],
                    None,
                )
                .unwrap(),
            ),
            false,
        ),
        (
            "cmpmap_linear",
            Box::new(
                CmpMap::new(
                    Box::new(ZoomMap::new(2, 2.0).unwrap()),
                    Box::new(ShiftMap::new(vec![1.0, 2.0])),
                    Composition::Series,
                )
                .unwrap(),
            ),
            true,
        ),
        (
            "cmpmap_with_permmap",
            Box::new(
                CmpMap::new(
                    Box::new(ZoomMap::new(2, 2.0).unwrap()),
                    Box::new(perm()),
                    Composition::Series,
                )
                .unwrap(),
            ),
            false,
        ),
        ("sphmap", Box::new(SphMap::new()), false),
        (
            "circle",
            Box::new(
                CircleBuilder::new(frame(), vec![0.0, 0.0])
                    .unwrap()
                    .radius(1.0)
                    .build()
                    .unwrap(),
            ),
            true,
        ),
        ("frameset_zoom", Box::new(fs), true),
    ];
    for (name, map, want) in cases {
        assert_eq!(map.is_linear(), want, "{name}");
    }
}
