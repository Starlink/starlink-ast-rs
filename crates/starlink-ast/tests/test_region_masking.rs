//! Masking with a sky Polygon whose great-circle edges are curved in the
//! pixel grid.
//!
//! Ancestor: `reference/starlink-ast/ast_tester/testregionmasking.c`
//! (`checkCurvature`). The ground truth classifies each pixel center by
//! transforming it to the sky and asking the Polygon directly; the masks are
//! compared with it. `mask` resamples through the Region, which simplifies
//! it when the grid has more than 1024 pixels, and with the default
//! `SimpVertices` that rebuilds the Polygon from its vertices in the grid,
//! with straight edges. For CAR the straight edges miss the curved boundary;
//! with `SimpVertices` false the simplification keeps the curved Polygon. For
//! TAN, great circles are straight lines, so both masks are exact.
//!
//! The pixel counts the C library reports are pinned as well as C's own
//! assertions, which only require the CAR default mask to differ.
//!
//! The CAR and TAN GRID-to-ICRS FrameSets are embedded in native form, as the
//! C test embeds them.

use starlink_ast::constants::BAD_VALUE;
use starlink_ast::coords::PointSet;
use starlink_ast::frames::{FrameRef, SkyFrame};
use starlink_ast::mappings::Mapping;
use starlink_ast::regions::{Polygon, PolygonBuilder, Region, mask};
use starlink_ast::{AnyObject, from_native_string};

const CAR_WCS: &str = r#" Begin FrameSet
 Nframe = 2
 Base = 1
 Currnt = 2
 Lnk2 = 1
 Frm1 =
 Begin Frame
 Title = "Pixel Coordinates"
 Naxes = 2
 Domain = "GRID"
 Ax1 =
 Begin Axis
 Label = "Pixel axis 1"
 End Axis
 Ax2 =
 Begin Axis
 Label = "Pixel axis 2"
 End Axis
 End Frame
 Frm2 =
 Begin SkyFrame
 Ident = " "
 IsA Object
 Naxes = 2
 System = "ICRS"
 Ax1 =
 Begin SkyAxis
 End SkyAxis
 Ax2 =
 Begin SkyAxis
 End SkyAxis
 IsA Frame
 Proj = "Cartesian"
 SRefIs = "Ignored"
 SRef1 = 3.1415926535897931
 SRef2 = 1.0471975511965976
 End SkyFrame
 Map2 =
 Begin CmpMap
 Nin = 2
 IsSimp = 1
 IsA Mapping
 MapA =
 Begin WinMap
 Nin = 2
 IsA Mapping
 Sft1 = 0.6108652381980153
 Scl1 = -0.0017453292519943296
 Sft2 = -0.43633231299858238
 Scl2 = 0.0017453292519943296
 End WinMap
 MapB =
 Begin CmpMap
 Nin = 2
 IsA Mapping
 InvA = 1
 MapA =
 Begin WcsMap
 Nin = 2
 Invert = 1
 IsA Mapping
 Type = "CAR"
 End WcsMap
 MapB =
 Begin CmpMap
 Nin = 2
 IsA Mapping
 InvA = 1
 MapA =
 Begin SphMap
 Nin = 3
 Nout = 2
 Invert = 1
 IsA Mapping
 UntRd = 1
 PlrLg = 0
 End SphMap
 MapB =
 Begin CmpMap
 Nin = 3
 Nout = 2
 IsA Mapping
 MapA =
 Begin MatrixMap
 Nin = 3
 IsA Mapping
 M0 = -0.50000000000000011
 M1 = 0
 M2 = 0.8660254037844386
 M3 = 0
 M4 = -1
 M5 = 0
 M6 = 0.8660254037844386
 M7 = 0
 M8 = 0.50000000000000011
 IM0 = -0.50000000000000011
 IM1 = 0
 IM2 = 0.8660254037844386
 IM3 = 0
 IM4 = -1
 IM5 = 0
 IM6 = 0.86602540378443849
 IM7 = 0
 IM8 = 0.50000000000000011
 Form = "Full"
 End MatrixMap
 MapB =
 Begin SphMap
 Nin = 3
 Nout = 2
 IsA Mapping
 UntRd = 1
 PlrLg = 3.1415926535897931
 End SphMap
 End CmpMap
 End CmpMap
 End CmpMap
 End CmpMap
 End FrameSet
"#;

const TAN_WCS: &str = r#" Begin FrameSet
 Nframe = 2
 Base = 1
 Currnt = 2
 Lnk2 = 1
 Frm1 =
 Begin Frame
 Title = "Pixel Coordinates"
 Naxes = 2
 Domain = "GRID"
 Ax1 =
 Begin Axis
 Label = "Pixel axis 1"
 End Axis
 Ax2 =
 Begin Axis
 Label = "Pixel axis 2"
 End Axis
 End Frame
 Frm2 =
 Begin SkyFrame
 Ident = " "
 IsA Object
 Naxes = 2
 System = "ICRS"
 Ax1 =
 Begin SkyAxis
 End SkyAxis
 Ax2 =
 Begin SkyAxis
 End SkyAxis
 IsA Frame
 Proj = "gnomonic"
 SRefIs = "Ignored"
 SRef1 = 3.1415926535897931
 SRef2 = 1.0471975511965976
 End SkyFrame
 Map2 =
 Begin CmpMap
 Nin = 2
 IsSimp = 1
 IsA Mapping
 MapA =
 Begin WinMap
 Nin = 2
 IsA Mapping
 Sft1 = 0.6108652381980153
 Scl1 = -0.0017453292519943296
 Sft2 = -0.43633231299858238
 Scl2 = 0.0017453292519943296
 End WinMap
 MapB =
 Begin CmpMap
 Nin = 2
 IsA Mapping
 InvA = 1
 MapA =
 Begin WcsMap
 Nin = 2
 Invert = 1
 IsA Mapping
 Type = "TAN"
 End WcsMap
 MapB =
 Begin CmpMap
 Nin = 2
 IsA Mapping
 InvA = 1
 MapA =
 Begin SphMap
 Nin = 3
 Nout = 2
 Invert = 1
 IsA Mapping
 UntRd = 1
 PlrLg = 0
 End SphMap
 MapB =
 Begin CmpMap
 Nin = 3
 Nout = 2
 IsA Mapping
 MapA =
 Begin MatrixMap
 Nin = 3
 IsA Mapping
 M0 = -0.8660254037844386
 M1 = 0
 M2 = -0.5
 M3 = 0
 M4 = -1
 M5 = 0
 M6 = -0.5
 M7 = 0
 M8 = 0.8660254037844386
 IM0 = -0.8660254037844386
 IM1 = 0
 IM2 = -0.5
 IM3 = 0
 IM4 = -1
 IM5 = 0
 IM6 = -0.5
 IM7 = 0
 IM8 = 0.8660254037844386
 Form = "Full"
 End MatrixMap
 MapB =
 Begin SphMap
 Nin = 3
 Nout = 2
 IsA Mapping
 UntRd = 1
 PlrLg = 3.1415926535897931
 End SphMap
 End CmpMap
 End CmpMap
 End CmpMap
 End CmpMap
 End FrameSet
"#;

/// What the C library reports for one projection: the pixels inside by the
/// ground truth, and the pixels where the default mask and the
/// `SimpVertices`-false mask disagree with it.
struct Expected {
    inside: usize,
    default_wrong: usize,
    sv0_wrong: usize,
}

fn check_curvature(wcs: &str, want: Expected) {
    let AnyObject::FrameSet(fs) = from_native_string(wcs).unwrap() else {
        panic!("embedded WCS did not read back as a FrameSet");
    };
    let sky = fs
        .current()
        .as_any()
        .downcast_ref::<SkyFrame>()
        .expect("the current Frame is a SkyFrame")
        .clone();
    let grid = fs.base();
    let sky2grid = fs.mapping(FrameRef::Current, FrameRef::Base).unwrap();
    let grid2sky = fs.mapping(FrameRef::Base, FrameRef::Current).unwrap();

    // A band at high declination, where the great circle joining two vertices
    // at equal declination bows away from the parallel.
    let d2r = std::f64::consts::PI / 180.0;
    let vertices = [(160.0, 50.0), (200.0, 50.0), (200.0, 70.0), (160.0, 70.0)]
        .iter()
        .map(|&(lon, lat)| vec![lon * d2r, lat * d2r])
        .collect();
    let mut poly = PolygonBuilder::new(sky, vertices).unwrap().build().unwrap();

    // Negate if the vertex order selected the complement of the band.
    let centre = PointSet::from_rows([[180.0 * d2r, 60.0 * d2r]]);
    if poly.transform(&centre, true).unwrap().point(0)[0] == BAD_VALUE {
        poly.negate();
    }

    // A pixel box around the curved Region, padded by three pixels.
    let mut polc = poly.clone();
    polc.set_simp_vertices(false);
    let (lo, hi) = polc
        .map_region(sky2grid.as_ref(), grid)
        .unwrap()
        .get_region_bounds()
        .unwrap();
    let lbnd = [lo[0].floor() as i64 - 3, lo[1].floor() as i64 - 3];
    let ubnd = [hi[0].ceil() as i64 + 3, hi[1].ceil() as i64 + 3];

    // Ground truth: each pixel center taken to the sky and tested.
    let centres: Vec<[f64; 2]> = (lbnd[1]..=ubnd[1])
        .flat_map(|iy| (lbnd[0]..=ubnd[0]).map(move |ix| [ix as f64, iy as f64]))
        .collect();
    let npix = centres.len();
    let on_sky = grid2sky
        .transform(&PointSet::from_rows(centres), true)
        .unwrap();
    let tested = poly.transform(&on_sky, true).unwrap();
    let truth: Vec<bool> = (0..npix).map(|i| tested.point(i)[0] != BAD_VALUE).collect();

    let masked = |region: &Polygon| {
        let mut data = vec![0_i32; npix];
        mask(
            region,
            Some(sky2grid.as_ref()),
            true,
            &lbnd,
            &ubnd,
            &mut data,
            1,
        )
        .unwrap();
        data
    };
    let wrong = |m: &[i32]| (0..npix).filter(|&i| (m[i] != 0) != truth[i]).count();

    let mdef = masked(&poly);
    let msv0 = masked(&polc);
    assert_eq!(
        truth.iter().filter(|&&b| b).count(),
        want.inside,
        "pixels inside"
    );
    assert_eq!(wrong(&mdef), want.default_wrong, "default mask");
    assert_eq!(wrong(&msv0), want.sv0_wrong, "SimpVertices=0 mask");
}

/// CAR: the default mask linearizes the curved edges; the
/// `SimpVertices`-false mask matches the ground truth.
#[test]
fn car_default_mask_linearises_curved_edges() {
    check_curvature(
        CAR_WCS,
        Expected {
            inside: 38036,
            default_wrong: 467,
            sv0_wrong: 0,
        },
    );
}

/// TAN: great circles are straight in the grid, so both masks are exact.
#[test]
fn tan_masks_match_the_ground_truth() {
    check_curvature(
        TAN_WCS,
        Expected {
            inside: 39080,
            default_wrong: 0,
            sv0_wrong: 0,
        },
    );
}
