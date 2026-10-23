# How To…

For those of you with a plane to catch, this section provides some instant templates and recipes for performing the most commonly-required operations using AST, but without going into detail.
The examples given (sort of) follow on from each other, so you should be able to construct a variety of programs by piecing them together.
Note that some of them appear longer than they actually are, because we have included plenty of comments and a few options that you probably won't need.

If any of this material has you completely baffled, then you may want to read the introduction to AST programming concepts in [An AST Object Primer](crate::guide::objects) first.
Otherwise, references to more detailed reading are given after each example, just in case they don't quite do what you want.

To add AST to a program, see [Using AST from Cargo](crate::guide::cargo).

## … Structure an AST Program

An AST program normally has the following structure:

```rust,no_run
// no_run: shows the shape of a program, not a complete one.
use ast_core::error::AstResult;

fn main() -> AstResult<()> {
    // <statements which use AST, propagating errors with `?`>
    Ok(())
}
```

Every operation that can fail returns an [`AstResult`](crate::error::AstResult), so a function that uses AST usually returns one too and passes errors on with `?`.
Objects are freed when they go out of scope, so there is nothing to tidy up after you have finished using AST.
For more details of errors, see [Error Detection](crate::guide::objects#error-detection).

## … Read a WCS Calibration from a Dataset

Precisely how you extract world coordinate system (WCS) information from a dataset obviously depends on what type of dataset it is.
Usually, however, you should be able to obtain a set of FITS header cards which contain the WCS information (and probably much more besides).
Suppose that `header` is a string containing a complete set of concatenated FITS header cards, each of 80 characters (such as produced by the CFITSIO function `fits_hdr2str`).
Then proceed as follows:

```rust
use ast_core::channel::Channel;
use ast_core::frames::FrameSet;
use ast_core::FitsChan;

# let header: String = [
#     "CTYPE1  = 'RA---TAN'", "CTYPE2  = 'DEC--TAN'",
#     "CRPIX1  = 100.5", "CRPIX2  = 98.4",
#     "CRVAL1  = 165.04167", "CRVAL2  = -23.433889",
#     "CD1_1   = -0.0003333333", "CD2_2   = 0.0003333333",
# ].iter().map(|c| format!("{c:80}")).collect();
// Create a FitsChan and fill it with FITS header cards.
let mut fitschan = FitsChan::new()?;
fitschan.put_cards(&header)?;

// Read WCS information from the FitsChan.
let wcsinfo = fitschan.read()?;
# let wcsinfo = FrameSet::try_from(wcsinfo.unwrap())?;
# assert_eq!(wcsinfo.nframe(), 2);
# Ok::<(), ast_core::error::AstError>(())
```

The result should be a FrameSet, `wcsinfo`, which contains the WCS information.
It can now be used to perform many useful tasks, some of which are illustrated in the following recipes.

Some datasets which do not easily yield FITS header cards may require a different approach, possibly involving use of a [`Channel`](crate::Channel) (*Saving and Restoring Objects*<!-- link: crate::guide::channels -->) rather than a FitsChan.
The whole process can probably be encapsulated in a similar way for most data systems, whether they use FITS header cards or not.

For more details about reading WCS information from datasets, see *Identifying Foreign Encodings on Input*<!-- link: crate::guide::foreign_fits#identifying-foreign-encodings-on-input --> and *Reading Foreign WCS Information from a FITS Header*<!-- link: crate::guide::foreign_fits#reading-foreign-wcs-information-from-a-fits-header -->.
For a more general description of FitsChans and their use with FITS header cards, see *Storing AST Objects in FITS Headers*<!-- link: crate::guide::fitschans --> and *Using Foreign FITS Encodings*<!-- link: crate::guide::foreign_fits -->.
For more details about FrameSets, see *Coordinate System Networks*<!-- link: crate::guide::framesets --> and *Higher Level Operations on FrameSets*<!-- link: crate::guide::frameset_ops -->.

## … Validate WCS Information

Once you have read WCS information from a dataset, as in [… Read a WCS Calibration from a Dataset](#-read-a-wcs-calibration-from-a-dataset), you may wish to check that you have been successful.
The following will detect and classify the things that might possibly go wrong:

```rust
use ast_core::channel::Channel;
use ast_core::frames::FrameSet;
use ast_core::FitsChan;

# let header = format!("{:80}", "COMMENT no WCS here");
# let mut fitschan = FitsChan::new()?;
# fitschan.put_cards(&header)?;
match fitschan.read() {
    Err(error) => {
        // <an error occurred; `error` describes it>
#       panic!("{error}");
    }
    Ok(None) => {
        // <there was no WCS information present>
    }
    Ok(Some(object)) => match FrameSet::try_from(object) {
        Err(_) => {
            // <something unexpected was read (i.e. not a FrameSet)>
#           unreachable!();
        }
        Ok(wcsinfo) => {
            // <WCS information was read OK>
#           unreachable!("{}", wcsinfo.nframe());
        }
    },
}
# Ok::<(), ast_core::error::AstError>(())
```

For more information about detecting errors in AST functions, see [Error Detection](crate::guide::objects#error-detection).
For details of how to validate input data read by AST, see *Validating Input*<!-- link: crate::guide::channels#validating-input --> and *Reading Foreign WCS Information from a FITS Header*<!-- link: crate::guide::foreign_fits#reading-foreign-wcs-information-from-a-fits-header -->.

## … Display AST Data

You can display the data stored in any AST Object in textual form as follows:

```rust
# use ast_core::frames::{FrameSet, SkyFrame, SkySystem};
# let wcsinfo = FrameSet::new(Box::new(SkyFrame::new(SkySystem::Fk5)));
wcsinfo.show()?;
# Ok::<(), ast_core::error::AstError>(())
```

Here, we have used the FrameSet which we read earlier ([… Read a WCS Calibration from a Dataset](#-read-a-wcs-calibration-from-a-dataset)).
The result is written to the program's standard output stream.
This can be very useful during debugging.

For more details about using `show`, see [Displaying Objects](crate::guide::objects#displaying-objects).
For information about interpreting the output, also see *The Textual Output Format*<!-- link: crate::guide::channels#the-textual-output-format -->.

## … Convert Between Pixel and World Coordinates

You may use a FrameSet, such as we read in [… Read a WCS Calibration from a Dataset](#-read-a-wcs-calibration-from-a-dataset), to transform a set of points between the pixel coordinates of an image and the associated world coordinates.
If you are working in two dimensions, proceed as follows:

```rust
# use ast_core::channel::Channel;
# use ast_core::frames::FrameSet;
# use ast_core::FitsChan;
use ast_core::mappings::Mapping;

# let header: String = [
#     "CTYPE1  = 'RA---TAN'", "CTYPE2  = 'DEC--TAN'",
#     "CRPIX1  = 100.5", "CRPIX2  = 98.4",
#     "CRVAL1  = 165.04167", "CRVAL2  = -23.433889",
#     "CD1_1   = -0.0003333333", "CD2_2   = 0.0003333333",
# ].iter().map(|c| format!("{c:80}")).collect();
# let mut fitschan = FitsChan::new()?;
# fitschan.put_cards(&header)?;
# let wcsinfo = FrameSet::try_from(fitschan.read()?.unwrap())?;
let xpixel = [100.5, 200.0];
let ypixel = [98.4, 300.0];
let (xworld, yworld) = wcsinfo.tran2(&xpixel, &ypixel, true)?;
# assert!((xworld[0] - 165.04167_f64.to_radians()).abs() < 1e-12);
# assert!((yworld[0] - (-23.433889_f64).to_radians()).abs() < 1e-12);
# Ok::<(), ast_core::error::AstError>(())
```

Here, `xpixel` and `ypixel` hold the pixel coordinates, and `xworld` and `yworld` receive the returned world coordinates.
By pixel coordinates, we mean a coordinate system in which the first pixel in the image is centred on (1,1) and each pixel is a unit square.
Note that the world coordinates will not necessarily be celestial coordinates, but if they are, then they will be in radians.
To transform in the opposite direction, give the world coordinates as input and change the third argument of [`tran2`](crate::mappings::Mapping::tran2) to `false`.

To transform points in one dimension, use [`tran1`](crate::mappings::Mapping::tran1).
In any other number of dimensions (or if the number of dimensions is initially unknown), use [`transform`](crate::mappings::Mapping::transform).

For more information about transforming coordinates, see [Transforming Coordinates](crate::guide::objects#transforming-coordinates) and *Using a FrameSet as a Mapping*<!-- link: crate::guide::framesets#using-a-frameset-as-a-mapping -->.
For details of how to handle missing coordinates, see [Handling Missing (Bad) Coordinate Values](crate::guide::mappings#handling-missing-bad-coordinate-values).

## … Test if a WCS is a Celestial Coordinate System

The world coordinate system (WCS) currently associated with an image may often be a celestial coordinate system, but this need not necessarily be the case.
For instance, instead of right ascension and declination, an image might have a WCS with axes representing wavelength and slit position, or maybe just plain old pixels.

If you have obtained a WCS calibration for an image, as in [… Read a WCS Calibration from a Dataset](#-read-a-wcs-calibration-from-a-dataset), in the form of a FrameSet `wcsinfo`, then you may determine if the current coordinate system is a celestial one or not, as follows:

```rust
use ast_core::frames::{FrameRef, FrameSet, SkyFrame, SkySystem};

# let wcsinfo = FrameSet::new(Box::new(SkyFrame::new(SkySystem::Fk5)));
// Determine if the current Frame is a SkyFrame.
let issky = wcsinfo.frame(FrameRef::Current)?.as_any().is::<SkyFrame>();
# assert!(issky);
# Ok::<(), ast_core::error::AstError>(())
```

This will set `issky` to `true` if the WCS is a celestial coordinate system, and to `false` otherwise.

## … Test if a WCS is a Spectral Coordinate System

Testing for a spectral coordinate system is basically the same as testing for a celestial coordinate system (see the previous section).
The difference is that you test for a [`SpecFrame`](crate::frames::SpecFrame) in place of a SkyFrame.
The test asks for one exact type, so a dual-sideband spectral coordinate system, which is a [`DSBSpecFrame`](crate::frames::DSBSpecFrame), has to be tested for as well:

```rust
use ast_core::frames::{DSBSpecFrame, FrameRef, FrameSet, SpecSystem};
use ast_core::frames::SpecFrame;

# let wcsinfo = FrameSet::new(Box::new(DSBSpecFrame::new(SpecSystem::Freq)));
let current = wcsinfo.frame(FrameRef::Current)?.as_any();
let isspec = current.is::<SpecFrame>() || current.is::<DSBSpecFrame>();
# assert!(isspec);
# Ok::<(), ast_core::error::AstError>(())
```

## … Format Coordinates for Display

Once you have converted pixel coordinates into world coordinates ([… Convert Between Pixel and World Coordinates](#-convert-between-pixel-and-world-coordinates)), you may want to format them as text before displaying them.
Typically, this would convert from (say) radians into something more comprehensible.
Using the FrameSet `wcsinfo` obtained in [… Read a WCS Calibration from a Dataset](#-read-a-wcs-calibration-from-a-dataset) and a pair of world coordinates `xw` and `yw`, you could proceed as follows:

```rust
use ast_core::frames::Frame;
# use ast_core::channel::Channel;
# use ast_core::frames::FrameSet;
# use ast_core::mappings::Mapping;
# use ast_core::FitsChan;

# let header: String = [
#     "CTYPE1  = 'RA---TAN'", "CTYPE2  = 'DEC--TAN'",
#     "CRPIX1  = 100.5", "CRPIX2  = 98.4",
#     "CRVAL1  = 165.04167", "CRVAL2  = -23.433889",
#     "CD1_1   = -0.0003333333", "CD2_2   = 0.0003333333",
# ].iter().map(|c| format!("{c:80}")).collect();
# let mut fitschan = FitsChan::new()?;
# fitschan.put_cards(&header)?;
# let wcsinfo = FrameSet::try_from(fitschan.read()?.unwrap())?;
# let (x, y) = wcsinfo.tran2(&[100.5], &[98.4], true)?;
# let (xw, yw) = (x[0], y[0]);
let xtext = wcsinfo.format(1, xw)?;
let ytext = wcsinfo.format(2, yw)?;

println!("Position = {xtext}, {ytext}");
# assert_eq!((xtext.as_str(), ytext.as_str()), ("11:00:10.0", "-23:26:02"));
# Ok::<(), ast_core::error::AstError>(())
```

Here, the first argument to [`format`](crate::frames::Frame::format) is the axis number.

With celestial coordinates, this will usually result in sexagesimal notation, such as "12:34:56.7".
However, the same method may be applied to any type of coordinates and appropriate formatting will be employed.

For more information about formatting coordinate values and how to control the style of formatting used, see *Formatting Axis Values*<!-- link: crate::guide::frames#formatting-axis-values --> and *Formatting Celestial Coordinates*<!-- link: crate::guide::skyframes#formatting-celestial-coordinates -->.
If necessary, also see *Normalising Frame Coordinates*<!-- link: crate::guide::frames#normalizing-frame-coordinates --> for details of how to "normalize" a set of coordinates so that they lie within the standard range (*e.g.* 0 to 24 hours for right ascension and ±90° for declination).

## … Display Coordinates as they are Transformed

In addition to formatting coordinates as part of a program's output, you may also want to examine coordinate values while debugging your program.
To save time, you can "eavesdrop" on the coordinate values being processed every time they are transformed.
For example, when using the FrameSet `wcsinfo` obtained in [… Read a WCS Calibration from a Dataset](#-read-a-wcs-calibration-from-a-dataset) to transform coordinates ([… Convert Between Pixel and World Coordinates](#-convert-between-pixel-and-world-coordinates)), you could inspect the coordinate values as follows:

```rust
# use ast_core::channel::Channel;
# use ast_core::frames::FrameSet;
# use ast_core::FitsChan;
use ast_core::mappings::Mapping;

# let header: String = [
#     "CTYPE1  = 'RA---TAN'", "CTYPE2  = 'DEC--TAN'",
#     "CRPIX1  = 100.5", "CRPIX2  = 98.4",
#     "CRVAL1  = 165.04167", "CRVAL2  = -23.433889",
#     "CD1_1   = -0.0003333333", "CD2_2   = 0.0003333333",
# ].iter().map(|c| format!("{c:80}")).collect();
# let mut fitschan = FitsChan::new()?;
# fitschan.put_cards(&header)?;
# let mut wcsinfo = FrameSet::try_from(fitschan.read()?.unwrap())?;
# let (xpixel, ypixel) = ([100.5], [98.4]);
wcsinfo.set_report(true);
let (xworld, yworld) = wcsinfo.tran2(&xpixel, &ypixel, true)?;
# let _ = (xworld, yworld);
# Ok::<(), ast_core::error::AstError>(())
```

By setting the FrameSet's [Report](crate::mappings::Mapping::report) attribute to `true`, coordinate transformations are automatically displayed on the program's standard output stream, appropriately formatted, for example:

```text
(42.1087, 20.2717) --> (2:06:03.0, 34:22:39)
(43.0197, 21.1705) --> (2:08:20.6, 35:31:24)
(43.9295, 22.0716) --> (2:10:38.1, 36:40:09)
(44.8382, 22.9753) --> (2:12:55.6, 37:48:55)
(45.7459, 23.8814) --> (2:15:13.1, 38:57:40)
(46.6528, 24.7901) --> (2:17:30.6, 40:06:25)
(47.5589, 25.7013) --> (2:19:48.1, 41:15:11)
(48.4644, 26.6149) --> (2:22:05.6, 42:23:56)
(49.3695, 27.5311) --> (2:24:23.1, 43:32:41)
(50.2742, 28.4499) --> (2:26:40.6, 44:41:27)
```

The report can be sent somewhere other than standard output with [`set_report_sink`](crate::mappings::report::set_report_sink).
For further details of how to set and enquire attribute values, see [Setting Attribute Values](crate::guide::objects#setting-attribute-values) and [Getting Attribute Values](crate::guide::objects#getting-attribute-values).

## … Read Coordinates Entered by a User

In addition to writing out coordinate values generated by your program ([… Format Coordinates for Display](#-format-coordinates-for-display)), you may also need to accept coordinates entered by a user, or perhaps read from a file.
In this case, you will probably want to allow "free-format" input, so that the user has some flexibility in the format that can be used.
You will probably also want to detect any typing errors.

Let's assume that you want to read a number of lines of text, each containing the world coordinates of a single point, and to split each line into individual numerical coordinate values.
Using the FrameSet `wcsinfo` obtained earlier ([… Read a WCS Calibration from a Dataset](#-read-a-wcs-calibration-from-a-dataset)), you could proceed as follows:

```rust
use ast_core::frames::Frame;
# use ast_core::frames::{FrameSet, SkyFrame, SkySystem};
# let wcsinfo = FrameSet::new(Box::new(SkyFrame::new(SkySystem::Fk5)));
# let input = "11:00:10, -23:26:02\n12 34.5 +1 2 3\n1:2:3 x\n";
# let (mut good, mut bad) = (0, 0);

// Obtain the number of coordinate axes (if not already known).
let naxes = wcsinfo.naxes();

// Loop to read each line of input text, in this case from a string (your
// program will probably read text in some other way).
for line in input.lines() {
    let mut coord = vec![0.0; naxes];
    let mut t = line;
    let mut n = 0;

    // Attempt to read a coordinate for each axis.
    for iaxis in 1..=naxes {
        (coord[iaxis - 1], n) = wcsinfo.unformat(iaxis, t)?;

        // If nothing was read and this is not the first axis or the
        // end-of-string, try stepping over a separator and reading again.
        if n == 0 && iaxis > 1 && !t.is_empty() {
            t = &t[1..];
            (coord[iaxis - 1], n) = wcsinfo.unformat(iaxis, t)?;
        }

        // Quit if nothing was read, otherwise move on to the next coordinate.
        if n == 0 {
            break;
        }
        t = &t[n..];
    }

    if !t.is_empty() || n == 0 {
        // <error in input data at the start of `t`: handle it here>
#       bad += 1;
        break;
    } else {
        // <coordinates were read OK>
#       good += 1;
    }
}
# assert_eq!((good, bad), (2, 1));
# Ok::<(), ast_core::error::AstError>(())
```

An error detected by AST is returned by [`unformat`](crate::frames::Frame::unformat) and passed on by `?`.

This algorithm has the advantage of accepting free-format input in whatever style is appropriate for the world coordinates in use (under the control of the FrameSet you provide).
For example, wavelength values might be read as floating point numbers (*e.g.* "1.047" or "4787"), whereas celestial positions could be given in sexagesimal format (*e.g.* "12:34:56" or "12 34.5") and would be converted into radians.
Individual coordinate values may be separated by white space and/or any non-ambiguous separator character, such as a comma.

For more information on reading coordinate values using `unformat`, see *Reading Formatted Axis Values*<!-- link: crate::guide::frames#reading-formatted-axis-values -->.
For details of how sexagesimal formats are handled, and the forms of input that may be used for celestial coordinates, see *Reading Formatted Celestial Coordinates*<!-- link: crate::guide::skyframes#reading-formatted-celestial-coordinates -->.

## … Create a New WCS Calibration

This section describes how to add a WCS calibration to a data set which you are creating from scratch, rather than modifying an existing data set.

In most common cases, the simplest way to create a new WCS calibration from scratch is probably to create a set of strings describing the required calibration in terms of the keywords used by the FITS WCS standard, and then convert these strings into an AST FrameSet describing the calibration.
This FrameSet can then be used for many other purposes, or simply stored in the data set.

The full FITS-WCS standard is quite involved, currently running to four separate papers, but the basic kernel is quite simple, involving the following keywords (all of which end with an integer axis index, indicated below by `<i>`):

- `CRPIX<i>`: hold the pixel coordinates at a reference point.
- `CRVAL<i>`: hold the corresponding WCS coordinates at the reference point.
- `CTYPE<i>`: name the quantity represented by the WCS axes, together with the projection algorithm used to convert the scaled and rotated pixel coordinates to WCS coordinates.
- `CD<i>_<j>`: a set of keywords which specify the elements of a matrix.
  This matrix scales pixel offsets from the reference point into the offsets required as input by the projection algorithm specified by the CTYPE keywords.
  This matrix specifies the scale and rotation of the image.
  If there is no rotation the off-diagonal elements of the matrix (*e.g.* `CD1_2` and `CD2_1`) can be omitted.

As an example consider the common case of a simple 2D image of the sky in which north is parallel to the second pixel axis and east parallel to the (negative) first pixel axis.
The image scale is 1.2 arc-seconds per pixel on both axes, and the image is presumed to have been obtained with a tangent plane projection.
Furthermore, it is known that pixel coordinates (100.5,98.4) correspond to an RA of 11:00:10 and a Dec. of -23:26:02.
A suitable set of FITS-WCS header cards could be:

```text
CTYPE1  = 'RA---TAN'       / Axis 1 represents RA with a tan projection
CTYPE2  = 'DEC--TAN'       / Axis 2 represents Dec with a tan projection
CRPIX1  = 100.5            / Pixel coordinates of reference point
CRPIX2  = 98.4             / Pixel coordinates of reference point
CRVAL1  = 165.04167        / Degrees equivalent of "11:00:10" hours
CRVAL2  = -23.433889       / Decimal equivalent of "-23:26:02" degrees
CD1_1   = -0.0003333333    / Decimal degrees equivalent of -1.2 arc-seconds
CD2_2   = 0.0003333333     / Decimal degrees equivalent of 1.2 arc-seconds
```

Notes:

- A FITS header card begins with the keyword name starting at column 1, has an equals sign in column 9, and the keyword value in columns 11 to 80.
- String values must be enclosed in single quotes.
- Celestial longitude and latitude must both be specified in decimal degrees.
- The `CD1_1` value is negative to indicate that RA increases as the first pixel axis decreases.
- The (RA,Dec) coordinates will be taken as ICRS coordinates.
  For FK5 you should add:

  ```text
  RADESYS = 'FK5'
  EQUINOX = 2005.6
  ```

  The EQUINOX value defaults to J2000.0 if omitted.
  FK4 can also be used in place of FK5, in which case EQUINOX defaults to B1950.0.

Once you have created these FITS-WCS header card strings, you should store them in a FitsChan and then read the corresponding FrameSet from the FitsChan.
How to do this is described in [… Read a WCS Calibration from a Dataset](#-read-a-wcs-calibration-from-a-dataset).

Having created the WCS calibration, you may want to store it in a data file.
How to do this is described in [… Write a Modified WCS Calibration to a Dataset](#-write-a-modified-wcs-calibration-to-a-dataset).
If you are writing the WCS calibration to a FITS file you obviously have the choice of storing the FITS-WCS cards directly.

If the required WCS calibration cannot be described as a set of FITS-WCS headers, then a different approach is necessary.
In this case, you should first create a Frame describing pixel coordinates, and store this Frame in a new FrameSet.
You should then create a new Frame describing the world coordinate system.
This Frame may be a specific kind of Frame such as a [`SkyFrame`](crate::frames::SkyFrame) for celestial coordinates, a [`SpecFrame`](crate::frames::SpecFrame) for spectral coordinates, a [`TimeFrame`](crate::frames::TimeFrame) for time coordinates, or a [`CmpFrame`](crate::frames::CmpFrame) for a combination of different coordinates.
You also need to create a suitable Mapping which transforms pixel coordinates into world coordinates.
AST provides many different types of Mappings, all of which can be combined together in arbitrary fashions to create more complicated Mappings.
The WCS Frame should then be added into the FrameSet, using the Mapping to connect the WCS Frame with the pixel Frame.

## … Modify a WCS Calibration

The usual reason for wishing to modify the WCS calibration associated with a dataset is that the data have been geometrically transformed in some way (here, we will assume a 2-dimensional image dataset).
This causes the image features (stars, galaxies, *etc.*) to move with respect to the grid of pixels which they occupy, so that any coordinate systems previously associated with the image become invalid.

To correct for this, it is necessary to set up a Mapping which expresses the positions of image features in the new data grid in terms of their positions in the old grid.
In both cases, the grid coordinates we use will have the first pixel centred at (1,1) with each pixel being a unit square.

AST allows you to correct for any type of geometrical transformation in this way, so long as a suitable Mapping to describe it can be constructed.
For purposes of illustration, we will assume here that the new image coordinates `xnew` and `ynew` can be expressed in terms of the old coordinates `xold` and `yold` as follows:

```text
xnew = xold * m[0] + yold * m[1] + z[0]
ynew = xold * m[2] + yold * m[3] + z[1]
```

where `m` is a 2×2 transformation matrix and `z` represents a shift of origin.
This is therefore a general linear coordinate transformation which can represent displacement, rotation, magnification and shear.

In AST, it can be represented by concatenating two Mappings.
The first is a [`MatrixMap`](crate::mappings::MatrixMap), which implements the matrix multiplication.
The second is a [`WinMap`](crate::mappings::WinMap), which linearly transforms one coordinate window on to another, but will be used here simply to implement the shift of origin (alternatively, a [`ShiftMap`](crate::mappings::ShiftMap) could have been used in place of a WinMap).
These Mappings may be constructed and concatenated as follows:

```rust
use ast_core::mappings::{Mapping, MatrixMap, WinMap};
use ndarray::array;

# let m = [0.0, 1.0, -1.0, 0.0];
# let z = [10.0, 20.0];
// The MatrixMap may be constructed directly from the matrix "m".
let matrixmap = MatrixMap::new(array![[m[0], m[1]], [m[2], m[3]]])?;

// For the WinMap, we set up the coordinates of the corners of a unit
// square (window) and then the same square shifted by the required
// amount.
let ina = [0.0, 0.0];
let inb = [1.0, 1.0];
let outa = [z[0], z[1]];
let outb = [1.0 + z[0], 1.0 + z[1]];

// The WinMap will then implement this shift.
let winmap = WinMap::from_corners(&ina, &inb, &outa, &outb)?;

// Join the two Mappings together, so that they are applied one after
// the other.
let newmap = Box::new(matrixmap).then(Box::new(winmap))?;
# let (x, y) = newmap.tran2(&[1.0], &[2.0], true)?;
# assert_eq!((x[0], y[0]), (12.0, 19.0));
# Ok::<(), ast_core::error::AstError>(())
```

You might, of course, create any other form of Mapping depending on the type of geometrical transformation involved.
For an overview of the Mappings provided by AST, see [Mappings Available](crate::guide::concepts#mappings-available), and for a description of the capabilities of each kind of Mapping, see its own documentation.
For an overview of how individual Mappings may be combined, see [Compound Mappings](crate::guide::concepts#compound-mappings) ([Compound Mappings](crate::guide::cmpmaps) gives more details).

Assuming you have obtained a WCS calibration for your original image in the form of a FrameSet, `wcsinfo1` ([… Read a WCS Calibration from a Dataset](#-read-a-wcs-calibration-from-a-dataset)), the Mapping created above may be used to produce a calibration for the new image as follows:

```rust
use ast_core::frames::{FrameRef, FrameSet, SkyFrame, SkySystem};
# use ast_core::mappings::{Mapping, ShiftMap};
# let wcsinfo1 = FrameSet::new(Box::new(SkyFrame::new(SkySystem::Fk5)));
# let newmap = ShiftMap::new(vec![1.0, 2.0]);

// If necessary, make a copy of the WCS calibration, since we are about
// to alter it.
let mut wcsinfo2 = wcsinfo1.clone();

// Re-map the base Frame so that it refers to the new data grid instead
// of the old one.
wcsinfo2.remap_frame(FrameRef::Base, Box::new(newmap))?;
# Ok::<(), ast_core::error::AstError>(())
```

This will produce a new FrameSet, `wcsinfo2`, in which all the coordinate systems associated with your original image are modified so that they are correctly registered with the new image instead.

For more information about re-mapping the Frames within a FrameSet, see *Re-Defining a FrameSet Coordinate System*<!-- link: crate::guide::frameset_ops#re-defining-a-frameset-coordinate-system -->.
Also see *Example — Binning an Image*<!-- link: crate::guide::frameset_ops#example--binning-an-image --> for a similar example to the above, applicable to the case of reducing the size of an image by binning.

## … Write a Modified WCS Calibration to a Dataset

If you have modified the WCS calibration associated with a dataset, such as in the example above ([… Modify a WCS Calibration](#-modify-a-wcs-calibration)), then you will need to write the modified version out along with any new data.

In the same way as when reading a WCS calibration ([… Read a WCS Calibration from a Dataset](#-read-a-wcs-calibration-from-a-dataset)), how you do this will depend on your data system, but we will assume that you wish to generate a set of FITS header cards that can be stored with the data.
You should usually make preparations for doing this when you first read the WCS calibration from your input dataset by modifying the example given in [… Read a WCS Calibration from a Dataset](#-read-a-wcs-calibration-from-a-dataset) as follows:

```rust
use ast_core::channel::Channel;
use ast_core::frames::FrameSet;
use ast_core::FitsChan;

# let cards = [
#     "CTYPE1  = 'RA---TAN'", "CTYPE2  = 'DEC--TAN'",
#     "CRPIX1  = 100.5", "CRPIX2  = 98.4",
#     "CRVAL1  = 165.04167", "CRVAL2  = -23.433889",
#     "CD1_1   = -0.0003333333", "CD2_2   = 0.0003333333",
# ];
// Create an input FitsChan and fill it with FITS header cards. Note, if
// you have all the header cards in a single string, use `put_cards` in
// place of `put_fits`.
let mut fitschan1 = FitsChan::new()?;
for card in cards {
    fitschan1.put_fits(card, false)?;
}

// Note which encoding has been used for the WCS information.
let encode = fitschan1.encoding()?;

// Rewind the input FitsChan and read the WCS information from it.
fitschan1.clear_card()?;
let wcsinfo1 = fitschan1.read()?;
# assert_eq!(encode, ast_core::Encoding::FitsWcs);
# let _ = FrameSet::try_from(wcsinfo1.unwrap())?;
# Ok::<(), ast_core::error::AstError>(())
```

Note how we have added an enquiry to determine how the WCS information is encoded in the input FITS cards, storing the result in the `encode` variable.
This must be done **before** actually reading the WCS calibration.

Once you have produced a modified WCS calibration for the output dataset (*e.g.* [… Modify a WCS Calibration](#-modify-a-wcs-calibration)), in the form of a FrameSet `wcsinfo2`, you can produce a new FitsChan containing the output FITS header cards as follows:

```rust
use ast_core::channel::Channel;
use ast_core::{AnyObject, Encoding};
# use ast_core::frames::FrameSet;
# use ast_core::FitsChan;
# let cards = [
#     "CTYPE1  = 'RA---TAN'", "CTYPE2  = 'DEC--TAN'",
#     "CRPIX1  = 100.5", "CRPIX2  = 98.4",
#     "CRVAL1  = 165.04167", "CRVAL2  = -23.433889",
#     "CD1_1   = -0.0003333333", "CD2_2   = 0.0003333333",
# ];
# let mut fitschan1 = FitsChan::new()?;
# for card in cards { fitschan1.put_fits(card, false)?; }
# let encode = fitschan1.encoding()?;
# fitschan1.clear_card()?;
# let wcsinfo2 = fitschan1.read()?.unwrap();

// Make a copy of the input FitsChan, AFTER the WCS information has been
// read from it. This will propagate all the input FITS header cards,
// apart from those describing the input WCS calibration.
let mut fitschan2 = fitschan1.clone();

// If necessary, make modifications to the cards in "fitschan2" (e.g. you
// might need to change NAXIS1, NAXIS2, etc., to account for a change in
// image size). You probably only need to do this if your data system does
// not provide these facilities itself.
// <details not shown - see below>

// Alternatively, if your data system handles the propagation of FITS
// header cards to the output dataset for you, then simply create an
// empty FitsChan to contain the output WCS information alone.
// let mut fitschan2 = FitsChan::new()?;

// Rewind the new FitsChan (if necessary) and attempt to write the output
// WCS information to it using the same encoding method as the input
// dataset.
fitschan2.set_card(1)?;
fitschan2.set_encoding(encode);
if fitschan2.write(&wcsinfo2)? == 0 {
    // If this didn't work (the WCS FrameSet has become too complex), then
    // use the native AST encoding instead.
    fitschan2.set_encoding(Encoding::Native);
    fitschan2.write(&wcsinfo2)?;
}
# fitschan2.clear_card()?;
# assert!(fitschan2.find_fits("CRPIX1", false)?.is_some());
# Ok::<(), ast_core::error::AstError>(())
```

For details of how to modify the contents of the output FitsChan in other ways, such as by adding, over-writing or deleting header cards, see *Addressing Cards in a FitsChan*<!-- link: crate::guide::fitschans#addressing-cards-in-a-fitschan -->, *Adding Concatenated Cards to a FitsChan*<!-- link: crate::guide::fitschans#adding-concatenated-cards-to-a-fitschan -->, *Adding Individual Cards to a FitsChan*<!-- link: crate::guide::fitschans#adding-individual-cards-to-a-fitschan --> and *Finding and Changing Cards in a FitsChan*<!-- link: crate::guide::fitschans#finding-and-changing-cards-in-a-fitschan -->.

Once you have assembled the output FITS cards, you may retrieve them from the FitsChan that contains them as follows:

```rust
# use ast_core::FitsChan;
# let mut fitschan2 = FitsChan::new()?;
# fitschan2.put_fits("CRPIX1  = 100.5", false)?;
fitschan2.clear_card()?;
while let Some(card) = fitschan2.find_fits("%f", true)? {
    println!("{card}");
}
# Ok::<(), ast_core::error::AstError>(())
```

Here, we have simply written each card to the standard output stream, but you would obviously replace this with code to store the cards in your output dataset.

For data systems that do not use FITS header cards, a different approach may be needed, possibly involving use of a Channel (*Saving and Restoring Objects*<!-- link: crate::guide::channels -->) rather than a FitsChan.
The whole process can probably be encapsulated in a similar way for most data systems, whether they use FITS header cards or not.

For an overview of how to propagate WCS information through data processing steps, see *Propagating WCS Information through Data Processing Steps*<!-- link: crate::guide::foreign_fits#propagating-wcs-information-through-data-processing-steps -->.
For more information about writing WCS information to FitsChans, see *Writing Native Objects to a FitsChan*<!-- link: crate::guide::fitschans#writing-native-objects-to-a-fitschan --> and *Writing Foreign WCS Information to a FITS Header*<!-- link: crate::guide::foreign_fits#writing-foreign-wcs-information-to-a-fits-header -->.
For information about the options for encoding WCS information in FITS header cards, see *The Native FITS Encoding*<!-- link: crate::guide::fitschans#the-native-fits-encoding -->, *The Foreign FITS Encodings*<!-- link: crate::guide::foreign_fits#the-foreign-fits-encodings -->, and the [`Encoding`](crate::Encoding) type.
For a complete understanding of FitsChans and their use with FITS header cards, you should read *Storing AST Objects in FITS Headers*<!-- link: crate::guide::fitschans --> and *Using Foreign FITS Encodings*<!-- link: crate::guide::foreign_fits -->.
