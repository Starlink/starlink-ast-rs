//! Reader-side tests for FITS-WCS alternate-plane support.

use ast_core::AnyObject;
use ast_core::channel::Channel;
use ast_core::fitschan::FitsChan;
use ast_core::frames::FrameRef;

const FITS_WCS_MULTI_HEAD: &str = "\
NAXIS   =                    2
NAXIS1  =                  300
NAXIS2  =                  300
WCSAXES =                    2
CTYPE1  = 'GLON-TAN'           / Galactic longitude
CTYPE2  = 'GLAT-TAN'           / Galactic latitude
CRVAL1  =              180.000
CRVAL2  =                0.000
CRPIX1  =              150.500
CRPIX2  =              150.500
CDELT1  =             -0.01000
CDELT2  =              0.01000
WCSAXESA=                    2
CTYPE1A = 'RA---TAN'
CTYPE2A = 'DEC--TAN'
CRVAL1A =              266.405
CRVAL2A =              -28.936
CRPIX1A =              150.500
CRPIX2A =              150.500
CDELT1A =             -0.01000
CDELT2A =              0.01000
EQUINOXA=               2000.0
RADESYSA= 'FK5     '
END
";

#[test]
fn fits_wcs_multi_reads_into_three_frame_frameset() {
    let mut fc = FitsChan::from_header(FITS_WCS_MULTI_HEAD).unwrap();
    let obj = fc.read().unwrap().unwrap();
    let fs = match obj {
        AnyObject::FrameSet(fs) => fs,
        _ => panic!("expected FrameSet"),
    };
    assert_eq!(fs.nframe(), 3, "expect base + primary + alt-A");

    // Frame 1 is base (GRID).
    let base = fs.frame(FrameRef::Index(1)).unwrap();
    assert_eq!(base.domain(), "GRID");
    // Frame 2 is primary (Galactic SKY); Ident is empty.
    let primary = fs.frame(FrameRef::Index(2)).unwrap();
    assert_eq!(primary.domain(), "SKY");
    assert_eq!(primary.ident(), "");
    // Frame 3 is alt-A (FK5 SKY); Ident="A".
    let alt = fs.frame(FrameRef::Index(3)).unwrap();
    assert_eq!(alt.domain(), "SKY");
    assert_eq!(alt.ident(), "A");
}
