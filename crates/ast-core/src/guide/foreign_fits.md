# Using Foreign FITS Encodings

We saw in the previous section ([Storing AST Objects in FITS Headers](crate::guide::fitschans)) how to store and retrieve any kind of AST Object in a FITS header by using a FitsChan.
To achieve this, we set the FitsChan's Encoding attribute to NATIVE.
However, the Objects we wrote could then only be read back by other programs that use AST.

In practice, we will also encounter FITS headers containing WCS information written by other software systems.
We will probably also need to write FITS headers in a format that can be understood by these systems.
Indeed, this interchange of data is one of the main reasons for the existence of FITS, so in this section we will examine how to accommodate these requirements.

## The Foreign FITS Encodings

As mentioned previously ([The Native FITS Encoding](crate::guide::fitschans#the-native-fits-encoding)), there are a number of conventions currently in use for storing WCS information in FITS headers, which we call *encodings*.
Here, we are concerned with those encodings defined by software systems other than AST, which we term *foreign encodings*.

AST supports several foreign encodings, which may be selected by setting the Encoding attribute of a FitsChan to one of the following [`Encoding`](crate::Encoding) values:

- [`Encoding::Dss`](crate::Encoding::Dss):
  This encoding stores WCS information using the convention developed at the Space Telescope Science Institute for the Digitized Sky Survey (DSS) astrometric plate calibrations.
  DSS images which use this convention are widely available and it is understood by a number of important and well-established astronomy applications.

  However, the calibration model used (based on a polynomial fit) is not easily applicable to other types of data and creating the polynomial coefficients needed to calibrate your own images can prove difficult.
  For this reason, the DSS encoding is probably best viewed as a "read-only" format.
  It is possible, however, to read in WCS information using this encoding and then to write it back out again, so long as only minor changes have been made.
- [`Encoding::FitsWcs`](crate::Encoding::FitsWcs):
  This encoding is very important because it is based on a FITS standard which addresses the problem of celestial coordinate systems in a proper manner, by considerably extending the original FITS standard.

  The conventions used are described in a series of papers by E.W. Greisen, M. Calabretta, *et al.*, often referred to as the "FITS-WCS papers".
  They are described at <http://fits.gsfc.nasa.gov/fits_wcs.html>.
  This encoding should be understood by any FITS-WCS compliant software.
  For details of the coverage of these conventions provided by the FitsChan class, see *FITS-WCS Coverage*<!-- link: crate::guide::fits_wcs -->.
- [`Encoding::FitsIraf`](crate::Encoding::FitsIraf):
  This encoding is based on the conventions described in the document "World Coordinate Systems Representations Within the FITS Format" by R.J. Hanisch and D.G. Wells, 1988.
  It is employed by the IRAF data analysis facility, so its use will facilitate data exchange with IRAF.
  This encoding is in effect a sub-set of the current FITS-WCS encoding.
- [`Encoding::FitsPc`](crate::Encoding::FitsPc):
  This encoding is based on a previous version of the proposed FITS WCS standard which used `PCjjjiii` and `CDELTj` keywords to describe axis rotation and scaling.
  It is included only to allow data created with that previous version to be read.
  It should not, in general, be used to create new data sets.
- [`Encoding::FitsAips`](crate::Encoding::FitsAips):
  This encoding is based on the conventions described in the document "Non-linear Coordinate Systems in AIPS" by Eric W. Greisen (revised 9th September, 1994).
  It is currently employed by the AIPS data analysis facility, so its use will facilitate data exchange with AIPS.
  This encoding uses `CROTAi` and `CDELTi` keywords to describe axis rotation and scaling.
- [`Encoding::FitsAipsPP`](crate::Encoding::FitsAipsPP):
  Encodes coordinate system information in FITS header cards using the conventions used by the AIPS++ project.
  This is an extension of FITS-AIPS which includes some of the features of FITS-PC and FITS-IRAF.
- [`Encoding::FitsClass`](crate::Encoding::FitsClass):
  Encodes coordinate system information in FITS header cards using the conventions used by the CLASS project.

For more detail about the above encodings, see the description of [`Encoding`](crate::Encoding).

## Limitations of Foreign Encodings

The foreign encodings available for storing WCS information in FITS headers have a number of limitations when compared with the native encoding of AST Objects ([Storing AST Objects in FITS Headers](crate::guide::fitschans)).
The main ones are:

1. Only one class of AST Object, the FrameSet, may be represented using a foreign FITS encoding.
   This should not come as a surprise, because the purpose of storing WCS information in FITS headers is to attach coordinate systems to an associated array of data.
   Since the FrameSet is the AST Object designed for the same purpose ([The Base and Current Frames](crate::guide::framesets#the-base-and-current-frames)), there is a natural correspondence.

   The way in which a FrameSet is translated to and from the foreign encoding also follows from this correspondence.
   The FrameSet's base Frame identifies the data grid coordinates of the associated FITS data.
   These are the same as FITS pixel coordinates, in which the first pixel (in 2 dimensions) has coordinates (1,1) at its center.
   Similarly, the current Frame of the FrameSet identifies the FITS world coordinate system associated with the data.
2. You may store a representation of only a single FrameSet in any individual set of FITS header cards (*i.e.* in a single FitsChan) at one time.
   If you attempt to store more than one, you may over-write the previous one or generate an invalid representation of your WCS information.

   This is mainly a consequence of the use of fixed FITS keywords by foreign encodings and the fact that you cannot, in general, have multiple FITS cards with the same keyword.
3. In general, it will not be possible to store every possible FrameSet that you might construct.
   Depending on the encoding, only certain FrameSets that conform to particular restrictions can be represented and, even then, some of their information may be lost.
   See the description of [`Encoding`](crate::Encoding) for more details of these limitations.

It should be understood that using foreign encodings to read and write information held in AST Objects is essentially a process of converting the data format.
As such, it potentially suffers from the same problems faced by all such processes, *i.e.* differences between the AST data model and that of the foreign encoding may cause some information to be lost.
Because the AST model is extremely flexible, however, any data loss can largely be eliminated when reading.
Instead, this effect manifests itself in the form of the above encoding-dependent restrictions on the kind of AST Objects which may be written.

One of the aims of the AST library, of course, is to insulate you from the details of these foreign encodings and the restrictions they impose.
We will see shortly, therefore, how AST provides a mechanism for determining whether your WCS information satisfies the necessary conditions and allows you to make an automatic choice of which encoding to use.

## Identifying Foreign Encodings on Input

Let us now examine the practicalities of extracting WCS information from a set of FITS header cards which have been written by some other software system.
We will pretend that our program does not know which encoding has been used for the WCS information and must discover this for itself.
In order to have a concrete example, however, we will use the following set of cards.
These use the FITS-AIPS encoding and contain a typical mix of other FITS cards which are irrelevant to the WCS information in which we are interested:

```text
SIMPLE  =                    T / Written by IDL:  30-Jul-1997 05:35:42.00
BITPIX  =                  -32 / Bits per pixel.
NAXIS   =                    2 / Number of dimensions
NAXIS1  =                  300 / Length of x axis.
NAXIS2  =                  300 / Length of y axis.
CTYPE1  = 'GLON-ZEA'           / X-axis type
CTYPE2  = 'GLAT-ZEA'           / Y-axis type
CRVAL1  =           -149.56866 / Reference pixel value
CRVAL2  =           -19.758201 / Reference pixel value
CRPIX1  =              150.500 / Reference pixel
CRPIX2  =              150.500 / Reference pixel
CDELT1  =             -1.20000 / Degrees/pixel
CDELT2  =              1.20000 / Degrees/pixel
CROTA1  =              0.00000 / Rotation in degrees.
SURVEY  = 'COBE DIRBE'
BUNITS  = 'MJy/sr  '           /
ORIGIN  = 'CDAC    '           / Cosmology Data Analysis Center
TELESCOP= 'COBE    '           / COsmic Background Explorer satellite
INSTRUME= 'DIRBE   '           / COBE instrument [DIRBE, DMR, FIRAS]
PIXRESOL=                    9 / Quad tree pixel resolution [6, 9]
DATE    = '27/09/94'           / FITS file creation date (dd/mm/yy)
DATE-MAP= '16/09/94'           / Date of original file creation (dd/mm/yy)
COMMENT     COBE specific keywords
DATE-BEG= '08/12/89'           / date of initial data represented (dd/mm/yy)
DATE-END= '25/09/90'           / date of final data represented   (dd/mm/yy)
```

The first step is to create a FitsChan and insert these cards into it.
If `cards` is a list of strings holding the header cards, this could be done as follows:

```rust
use ast_core::FitsChan;

# const HEADER: &str = "\
# SIMPLE  =                    T / Written by IDL:  30-Jul-1997 05:35:42.00
# BITPIX  =                  -32 / Bits per pixel.
# NAXIS   =                    2 / Number of dimensions
# NAXIS1  =                  300 / Length of x axis.
# NAXIS2  =                  300 / Length of y axis.
# CTYPE1  = 'GLON-ZEA'           / X-axis type
# CTYPE2  = 'GLAT-ZEA'           / Y-axis type
# CRVAL1  =           -149.56866 / Reference pixel value
# CRVAL2  =           -19.758201 / Reference pixel value
# CRPIX1  =              150.500 / Reference pixel
# CRPIX2  =              150.500 / Reference pixel
# CDELT1  =             -1.20000 / Degrees/pixel
# CDELT2  =              1.20000 / Degrees/pixel
# CROTA1  =              0.00000 / Rotation in degrees.
# SURVEY  = 'COBE DIRBE'
# BUNITS  = 'MJy/sr  '           /
# ORIGIN  = 'CDAC    '           / Cosmology Data Analysis Center
# TELESCOP= 'COBE    '           / COsmic Background Explorer satellite
# INSTRUME= 'DIRBE   '           / COBE instrument [DIRBE, DMR, FIRAS]
# PIXRESOL=                    9 / Quad tree pixel resolution [6, 9]
# DATE    = '27/09/94'           / FITS file creation date (dd/mm/yy)
# DATE-MAP= '16/09/94'           / Date of original file creation (dd/mm/yy)
# COMMENT     COBE specific keywords
# DATE-BEG= '08/12/89'           / date of initial data represented (dd/mm/yy)
# DATE-END= '25/09/90'           / date of final data represented   (dd/mm/yy)";
# let cards: Vec<&str> = HEADER.lines().collect();
let mut fitschan = FitsChan::new()?;
for card in cards {
    fitschan.put_fits(card, false)?;
}
# Ok::<(), ast_core::error::AstError>(())
```

Note that we have not set the Encoding attribute of the FitsChan as we did in [Creating a FitsChan](crate::guide::fitschans#creating-a-fitschan) when we wanted to use the native encoding.
This is because we are pretending not to know which encoding to use and want AST to determine this for us.
By leaving the Encoding attribute un-set, its default value will adjust to whichever encoding AST considers to be most appropriate, according to the FITS header cards present.
For details of how this choice is made, see the description of [`encoding`](crate::FitsChan::encoding).

This approach has the obvious advantages of making our program simpler and more flexible and of freeing us from having to know about the different encodings available.
As a bonus, it also means that the program will be able to read any new encodings that AST may support in future, without needing to be changed.

At this point, we could inquire the default value of the Encoding attribute, which indicates which encoding AST intends to use, as follows:

```rust
# use ast_core::FitsChan;
# const HEADER: &str = "\
# SIMPLE  =                    T / Written by IDL:  30-Jul-1997 05:35:42.00
# BITPIX  =                  -32 / Bits per pixel.
# NAXIS   =                    2 / Number of dimensions
# NAXIS1  =                  300 / Length of x axis.
# NAXIS2  =                  300 / Length of y axis.
# CTYPE1  = 'GLON-ZEA'           / X-axis type
# CTYPE2  = 'GLAT-ZEA'           / Y-axis type
# CRVAL1  =           -149.56866 / Reference pixel value
# CRVAL2  =           -19.758201 / Reference pixel value
# CRPIX1  =              150.500 / Reference pixel
# CRPIX2  =              150.500 / Reference pixel
# CDELT1  =             -1.20000 / Degrees/pixel
# CDELT2  =              1.20000 / Degrees/pixel
# CROTA1  =              0.00000 / Rotation in degrees.
# SURVEY  = 'COBE DIRBE'
# BUNITS  = 'MJy/sr  '           /
# ORIGIN  = 'CDAC    '           / Cosmology Data Analysis Center
# TELESCOP= 'COBE    '           / COsmic Background Explorer satellite
# INSTRUME= 'DIRBE   '           / COBE instrument [DIRBE, DMR, FIRAS]
# PIXRESOL=                    9 / Quad tree pixel resolution [6, 9]
# DATE    = '27/09/94'           / FITS file creation date (dd/mm/yy)
# DATE-MAP= '16/09/94'           / Date of original file creation (dd/mm/yy)
# COMMENT     COBE specific keywords
# DATE-BEG= '08/12/89'           / date of initial data represented (dd/mm/yy)
# DATE-END= '25/09/90'           / date of final data represented   (dd/mm/yy)";
# let mut fitschan = FitsChan::new()?;
# for card in HEADER.lines() { fitschan.put_fits(card, false)?; }
let encode = fitschan.encoding()?;
# assert_eq!(encode, ast_core::Encoding::FitsAips);
# Ok::<(), ast_core::error::AstError>(())
```

The result of this inquiry would be [`Encoding::FitsAips`](crate::Encoding::FitsAips).
Note that we could also have set the FitsChan's Encoding attribute explicitly, with [`set_encoding`](crate::FitsChan::set_encoding).

If we tried to read information using this encoding ([Reading Foreign WCS Information from a FITS Header](#reading-foreign-wcs-information-from-a-fits-header)), but failed, we could then change the encoding and try again.
This would allow our program to take control of how the optimum choice of encoding is arrived at.
However, it would also involve using explicit knowledge of the encodings available and this is best avoided if possible.

## Reading Foreign WCS Information from a FITS Header

Having stored a set of FITS header cards in a FitsChan and determined how the WCS information is encoded ([Identifying Foreign Encodings on Input](#identifying-foreign-encodings-on-input)), the next step is to read an AST Object from the FitsChan using [`read`](crate::Channel::read).
We must also remember to rewind the FitsChan first, if necessary, such as by clearing its Card attribute, which defaults to 1:

```rust
use ast_core::Channel;
# use ast_core::FitsChan;
# const HEADER: &str = "\
# SIMPLE  =                    T / Written by IDL:  30-Jul-1997 05:35:42.00
# BITPIX  =                  -32 / Bits per pixel.
# NAXIS   =                    2 / Number of dimensions
# NAXIS1  =                  300 / Length of x axis.
# NAXIS2  =                  300 / Length of y axis.
# CTYPE1  = 'GLON-ZEA'           / X-axis type
# CTYPE2  = 'GLAT-ZEA'           / Y-axis type
# CRVAL1  =           -149.56866 / Reference pixel value
# CRVAL2  =           -19.758201 / Reference pixel value
# CRPIX1  =              150.500 / Reference pixel
# CRPIX2  =              150.500 / Reference pixel
# CDELT1  =             -1.20000 / Degrees/pixel
# CDELT2  =              1.20000 / Degrees/pixel
# CROTA1  =              0.00000 / Rotation in degrees.
# SURVEY  = 'COBE DIRBE'
# BUNITS  = 'MJy/sr  '           /
# ORIGIN  = 'CDAC    '           / Cosmology Data Analysis Center
# TELESCOP= 'COBE    '           / COsmic Background Explorer satellite
# INSTRUME= 'DIRBE   '           / COBE instrument [DIRBE, DMR, FIRAS]
# PIXRESOL=                    9 / Quad tree pixel resolution [6, 9]
# DATE    = '27/09/94'           / FITS file creation date (dd/mm/yy)
# DATE-MAP= '16/09/94'           / Date of original file creation (dd/mm/yy)
# COMMENT     COBE specific keywords
# DATE-BEG= '08/12/89'           / date of initial data represented (dd/mm/yy)
# DATE-END= '25/09/90'           / date of final data represented   (dd/mm/yy)";
# let mut fitschan = FitsChan::new()?;
# for card in HEADER.lines() { fitschan.put_fits(card, false)?; }

fitschan.clear_card()?;
let wcsinfo = fitschan.read()?;
# assert!(wcsinfo.is_some());
# Ok::<(), ast_core::error::AstError>(())
```

If `read` returns an Object, then it has been read successfully.
Otherwise, there was either no information to read or the choice of FITS encoding ([Identifying Foreign Encodings on Input](#identifying-foreign-encodings-on-input)) was inappropriate.

At this point you might like to indulge in a little data validation along the lines described in [Validating Input](crate::guide::channels#validating-input), for example:

```rust
use ast_core::frames::FrameSet;
# use ast_core::{Channel, FitsChan};
# const HEADER: &str = "\
# SIMPLE  =                    T / Written by IDL:  30-Jul-1997 05:35:42.00
# BITPIX  =                  -32 / Bits per pixel.
# NAXIS   =                    2 / Number of dimensions
# NAXIS1  =                  300 / Length of x axis.
# NAXIS2  =                  300 / Length of y axis.
# CTYPE1  = 'GLON-ZEA'           / X-axis type
# CTYPE2  = 'GLAT-ZEA'           / Y-axis type
# CRVAL1  =           -149.56866 / Reference pixel value
# CRVAL2  =           -19.758201 / Reference pixel value
# CRPIX1  =              150.500 / Reference pixel
# CRPIX2  =              150.500 / Reference pixel
# CDELT1  =             -1.20000 / Degrees/pixel
# CDELT2  =              1.20000 / Degrees/pixel
# CROTA1  =              0.00000 / Rotation in degrees.
# SURVEY  = 'COBE DIRBE'
# BUNITS  = 'MJy/sr  '           /
# ORIGIN  = 'CDAC    '           / Cosmology Data Analysis Center
# TELESCOP= 'COBE    '           / COsmic Background Explorer satellite
# INSTRUME= 'DIRBE   '           / COBE instrument [DIRBE, DMR, FIRAS]
# PIXRESOL=                    9 / Quad tree pixel resolution [6, 9]
# DATE    = '27/09/94'           / FITS file creation date (dd/mm/yy)
# DATE-MAP= '16/09/94'           / Date of original file creation (dd/mm/yy)
# COMMENT     COBE specific keywords
# DATE-BEG= '08/12/89'           / date of initial data represented (dd/mm/yy)
# DATE-END= '25/09/90'           / date of final data represented   (dd/mm/yy)";
# let mut fitschan = FitsChan::new()?;
# for card in HEADER.lines() { fitschan.put_fits(card, false)?; }
# fitschan.clear_card()?;
# let wcsinfo = fitschan.read()?.unwrap();

match FrameSet::try_from(wcsinfo) {
    Ok(frameset) => {
        // <the Object is a FrameSet, so use it>
    }
    Err(_) => {
        // <something unexpected was read>
#       unreachable!();
    }
}
# Ok::<(), ast_core::error::AstError>(())
```

If a foreign encoding has definitely been used, then the Object will automatically be a FrameSet ([Limitations of Foreign Encodings](#limitations-of-foreign-encodings)), so this stage can be omitted.
However, if the native encoding ([The Native FITS Encoding](crate::guide::fitschans#the-native-fits-encoding)) might have been employed, which is a possibility if you accept the FitsChan's default Encoding value, then any class of Object might have been read and a quick check would be worthwhile.

If you used `show` ([Displaying Objects](crate::guide::objects#displaying-objects)) to examine the FrameSet which results from reading our example FITS header ([Identifying Foreign Encodings on Input](#identifying-foreign-encodings-on-input)), you would find that its base Frame describes the image's pixel coordinate system and that its current Frame is a SkyFrame representing galactic coordinates.
These two Frames are inter-related by a Mapping (actually a CmpMap) which incorporates the effects of various rotations, scalings and a "zenithal equal area" sky projection, so that each pixel of the FITS image is mapped on to a corresponding sky position in galactic coordinates.

```rust
use ast_core::frames::{FrameRef, FrameSet, SkyFrame, SkySystem};
# use ast_core::{Channel, FitsChan};
# const HEADER: &str = "\
# SIMPLE  =                    T / Written by IDL:  30-Jul-1997 05:35:42.00
# BITPIX  =                  -32 / Bits per pixel.
# NAXIS   =                    2 / Number of dimensions
# NAXIS1  =                  300 / Length of x axis.
# NAXIS2  =                  300 / Length of y axis.
# CTYPE1  = 'GLON-ZEA'           / X-axis type
# CTYPE2  = 'GLAT-ZEA'           / Y-axis type
# CRVAL1  =           -149.56866 / Reference pixel value
# CRVAL2  =           -19.758201 / Reference pixel value
# CRPIX1  =              150.500 / Reference pixel
# CRPIX2  =              150.500 / Reference pixel
# CDELT1  =             -1.20000 / Degrees/pixel
# CDELT2  =              1.20000 / Degrees/pixel
# CROTA1  =              0.00000 / Rotation in degrees.
# SURVEY  = 'COBE DIRBE'
# BUNITS  = 'MJy/sr  '           /
# ORIGIN  = 'CDAC    '           / Cosmology Data Analysis Center
# TELESCOP= 'COBE    '           / COsmic Background Explorer satellite
# INSTRUME= 'DIRBE   '           / COBE instrument [DIRBE, DMR, FIRAS]
# PIXRESOL=                    9 / Quad tree pixel resolution [6, 9]
# DATE    = '27/09/94'           / FITS file creation date (dd/mm/yy)
# DATE-MAP= '16/09/94'           / Date of original file creation (dd/mm/yy)
# COMMENT     COBE specific keywords
# DATE-BEG= '08/12/89'           / date of initial data represented (dd/mm/yy)
# DATE-END= '25/09/90'           / date of final data represented   (dd/mm/yy)";
# let mut fitschan = FitsChan::new()?;
# for card in HEADER.lines() { fitschan.put_fits(card, false)?; }
# fitschan.clear_card()?;
# let frameset = FrameSet::try_from(fitschan.read()?.unwrap())?;

let current = frameset.frame(FrameRef::Current)?;
let skyframe = current.as_any().downcast_ref::<SkyFrame>().expect("a SkyFrame");
assert_eq!(skyframe.system(), SkySystem::Galactic);
# Ok::<(), ast_core::error::AstError>(())
```

Because this FrameSet may be used both as a Mapping ([Using a FrameSet as a Mapping](crate::guide::framesets#using-a-frameset-as-a-mapping)) and as a Frame ([Using a FrameSet as a Frame](crate::guide::framesets#using-a-frameset-as-a-frame)), it may be employed directly to perform many useful operations without any need to decompose it into its component parts.
These include:

- Transforming data grid (FITS pixel) coordinates into galactic coordinates and *vice versa* ([Using a FrameSet as a Mapping](crate::guide::framesets#using-a-frameset-as-a-mapping)).
- Formatting coordinate values (either pixel or galactic coordinates) ready for display to a user ([Formatting Axis Values](crate::guide::frames#formatting-axis-values) and [Normalizing Frame Coordinates](crate::guide::frames#normalizing-frame-coordinates)).
- Inquiring about axis labels (or other axis information — [Frame Attributes](crate::guide::frames#frame-attributes)) which might be used, for example, to label columns of coordinates in a table ([Frame Axis Attributes](crate::guide::frames#frame-axis-attributes)).
- Aligning the image with another image from which a similar FrameSet has been obtained ([Example — Registering Two Images](crate::guide::frameset_ops#example--registering-two-images)).
- Generating a new FrameSet which reflects any geometrical processing you perform on the associated image data ([Example — Binning an Image](crate::guide::frameset_ops#example--binning-an-image)).
  This new FrameSet could then be written out as FITS headers to describe the modified image ([Writing Foreign WCS Information to a FITS Header](#writing-foreign-wcs-information-to-a-fits-header)).

If the FrameSet contains other Frames (apart from the base and current Frames), then you would also have access to information about other coordinate systems associated with the image.

## Removing WCS Information from FITS Headers — the Destructive Read

It is instructive at this point to examine the contents of a FitsChan after we have read a FrameSet from it ([Reading Foreign WCS Information from a FITS Header](#reading-foreign-wcs-information-from-a-fits-header)).
The following would rewind our FitsChan and display its contents:

```rust
# use ast_core::{Channel, FitsChan};
# const HEADER: &str = "\
# SIMPLE  =                    T / Written by IDL:  30-Jul-1997 05:35:42.00
# BITPIX  =                  -32 / Bits per pixel.
# NAXIS   =                    2 / Number of dimensions
# NAXIS1  =                  300 / Length of x axis.
# NAXIS2  =                  300 / Length of y axis.
# CTYPE1  = 'GLON-ZEA'           / X-axis type
# CTYPE2  = 'GLAT-ZEA'           / Y-axis type
# CRVAL1  =           -149.56866 / Reference pixel value
# CRVAL2  =           -19.758201 / Reference pixel value
# CRPIX1  =              150.500 / Reference pixel
# CRPIX2  =              150.500 / Reference pixel
# CDELT1  =             -1.20000 / Degrees/pixel
# CDELT2  =              1.20000 / Degrees/pixel
# CROTA1  =              0.00000 / Rotation in degrees.
# SURVEY  = 'COBE DIRBE'
# BUNITS  = 'MJy/sr  '           /
# ORIGIN  = 'CDAC    '           / Cosmology Data Analysis Center
# TELESCOP= 'COBE    '           / COsmic Background Explorer satellite
# INSTRUME= 'DIRBE   '           / COBE instrument [DIRBE, DMR, FIRAS]
# PIXRESOL=                    9 / Quad tree pixel resolution [6, 9]
# DATE    = '27/09/94'           / FITS file creation date (dd/mm/yy)
# DATE-MAP= '16/09/94'           / Date of original file creation (dd/mm/yy)
# COMMENT     COBE specific keywords
# DATE-BEG= '08/12/89'           / date of initial data represented (dd/mm/yy)
# DATE-END= '25/09/90'           / date of final data represented   (dd/mm/yy)";
# let mut fitschan = FitsChan::new()?;
# for card in HEADER.lines() { fitschan.put_fits(card, false)?; }
# fitschan.clear_card()?;
# fitschan.read()?;
fitschan.clear_card()?;
while let Some(card) = fitschan.find_fits("%f", true)? {
    println!("{card}");
}
# fitschan.clear_card()?;
# assert!(fitschan.find_fits("CTYPE1", false)?.is_none());
# assert_eq!(fitschan.ncard()?, 16);
# Ok::<(), ast_core::error::AstError>(())
```

The output, if we started with the example FITS header in [Identifying Foreign Encodings on Input](#identifying-foreign-encodings-on-input), might look like this:

```text
SIMPLE  =                    T / Written by IDL:  30-Jul-1997 05:35:42.00
BITPIX  =                  -32 / Bits per pixel.
NAXIS   =                    2 / Number of dimensions
NAXIS1  =                  300 / Length of x axis.
NAXIS2  =                  300 / Length of y axis.
SURVEY  = 'COBE DIRBE'
BUNITS  = 'MJy/sr  '
ORIGIN  = 'CDAC    '           / Cosmology Data Analysis Center
TELESCOP= 'COBE    '           / COsmic Background Explorer satellite
INSTRUME= 'DIRBE   '           / COBE instrument [DIRBE, DMR, FIRAS]
PIXRESOL=                    9 / Quad tree pixel resolution [6, 9]
DATE    = '27/09/94'           / FITS file creation date (dd/mm/yy)
DATE-MAP= '16/09/94'           / Date of original file creation (dd/mm/yy)
COMMENT     COBE specific keywords
DATE-BEG= '08/12/89'           / date of initial data represented (dd/mm/yy)
DATE-END= '25/09/90'           / date of final data represented   (dd/mm/yy)
```

Comparing this with the original, you can see that all the FITS cards that represent WCS information have been removed.
They have effectively been "sucked out" of the FitsChan by the destructive read that `read` performs and converted into an equivalent FrameSet.
AST remembers where they were stored, however, so that if we later write WCS information back into the FitsChan ([Writing Foreign WCS Information to a FITS Header](#writing-foreign-wcs-information-to-a-fits-header)) they will, as far as possible, go back into their original locations.
This helps to preserve the overall layout of the FITS header.

You can now see why `read` performs destructive reads.
It is a mechanism for removing WCS information from a FITS header while insulating you, as a programmer, from the details of the encoding being used.
It means you can ensure that all relevant header cards have been removed, giving you a clean slate, without having to know which FITS keywords any particular encoding uses.

Clearing this WCS information out of a FITS header is particularly important when considering how to write new WCS information back after processing ([Writing Foreign WCS Information to a FITS Header](#writing-foreign-wcs-information-to-a-fits-header)).
If any relevant FITS cards are left over from the input dataset and find their way into the new processed header, they could interfere with the new information being written.
(This can happen if a particular keyword is present in the input header but is not used in the output header — whether particular keywords are used can depend on the WCS information being stored.
In such a case, the original value would not be over-written by a new output value, so would remain erroneously present.)
The destructive read mechanism ensures that this doesn't happen.

## Propagating WCS Information through Data Processing Steps

One of the purposes of AST is to make it feasible to propagate WCS information through successive stages of data processing, so that it remains consistent with the associated image data.
As far as possible, this should happen regardless of the FITS encoding used to store the original WCS information.

If the data processing being performed does not change the relationship between image pixel and world coordinates (whatever these may be), then propagation of the WCS information is straightforward.
You can simply copy the FITS header from input to output.

If this relationship changes, however, then the WCS information must be processed alongside the image data and a new FITS header generated to represent it.
In this case, the sequence of operations within your program would probably be as follows:

1. Read the image data and associated FITS header from the input dataset, putting the header cards into a FitsChan ([Identifying Foreign Encodings on Input](#identifying-foreign-encodings-on-input)).
2. Read an AST Object, a FrameSet, from the FitsChan (typically using a foreign FITS encoding — [Reading Foreign WCS Information from a FITS Header](#reading-foreign-wcs-information-from-a-fits-header)).
3. Process the image data and modify the FrameSet accordingly (*e.g.* [Example — Binning an Image](crate::guide::frameset_ops#example--binning-an-image)).
4. Write the FrameSet back into the FitsChan ([Writing Foreign WCS Information to a FITS Header](#writing-foreign-wcs-information-to-a-fits-header)).
5. Perform any other modification of FITS header cards your program may require.
6. Write the FitsChan contents (*i.e.* processed header cards) and image data to the output dataset.

In stage (2), the original WCS information will be removed from the FitsChan by a destructive read.
Later, in stage (4), new WCS information is written to replace it.
This is the process which we consider next ([Writing Foreign WCS Information to a FITS Header](#writing-foreign-wcs-information-to-a-fits-header)).

## Writing Foreign WCS Information to a FITS Header

Before we can write processed WCS information held in a FrameSet back into a FitsChan in preparation for output, we must select the FITS encoding to use.
Unfortunately, we cannot simply depend on the default value of the Encoding attribute, as we did when reading the input information ([Identifying Foreign Encodings on Input](#identifying-foreign-encodings-on-input)), because the destructive action of reading the WCS data ([Removing WCS Information from FITS Headers — the Destructive Read](#removing-wcs-information-from-fits-headers--the-destructive-read)) will have altered the FitsChan's contents.
This, in turn, will have changed the choice of default encoding, probably causing it to revert to NATIVE.

We will return to the question of the optimum choice of encoding below.
For now, let's assume that we want to use the same encoding for output as we used for input.
Since we inquired what that was before we read the input WCS data from the FitsChan ([Identifying Foreign Encodings on Input](#identifying-foreign-encodings-on-input)), we can now set that value explicitly.
We can also set the FitsChan's Card attribute back to 1 at the same time (because the write will fail if the FitsChan is not rewound).
`write` can then be used to write the output WCS information into the FitsChan:

```rust
use ast_core::Channel;
# use ast_core::FitsChan;
# const HEADER: &str = "\
# SIMPLE  =                    T / Written by IDL:  30-Jul-1997 05:35:42.00
# BITPIX  =                  -32 / Bits per pixel.
# NAXIS   =                    2 / Number of dimensions
# NAXIS1  =                  300 / Length of x axis.
# NAXIS2  =                  300 / Length of y axis.
# CTYPE1  = 'GLON-ZEA'           / X-axis type
# CTYPE2  = 'GLAT-ZEA'           / Y-axis type
# CRVAL1  =           -149.56866 / Reference pixel value
# CRVAL2  =           -19.758201 / Reference pixel value
# CRPIX1  =              150.500 / Reference pixel
# CRPIX2  =              150.500 / Reference pixel
# CDELT1  =             -1.20000 / Degrees/pixel
# CDELT2  =              1.20000 / Degrees/pixel
# CROTA1  =              0.00000 / Rotation in degrees.
# SURVEY  = 'COBE DIRBE'
# BUNITS  = 'MJy/sr  '           /
# ORIGIN  = 'CDAC    '           / Cosmology Data Analysis Center
# TELESCOP= 'COBE    '           / COsmic Background Explorer satellite
# INSTRUME= 'DIRBE   '           / COBE instrument [DIRBE, DMR, FIRAS]
# PIXRESOL=                    9 / Quad tree pixel resolution [6, 9]
# DATE    = '27/09/94'           / FITS file creation date (dd/mm/yy)
# DATE-MAP= '16/09/94'           / Date of original file creation (dd/mm/yy)
# COMMENT     COBE specific keywords
# DATE-BEG= '08/12/89'           / date of initial data represented (dd/mm/yy)
# DATE-END= '25/09/90'           / date of final data represented   (dd/mm/yy)";
# let mut fitschan = FitsChan::new()?;
# for card in HEADER.lines() { fitschan.put_fits(card, false)?; }
# let encode = fitschan.encoding()?;
# fitschan.clear_card()?;
# let wcsinfo = fitschan.read()?.unwrap();
# assert_eq!(fitschan.encoding()?, ast_core::Encoding::Native);

fitschan.set_card(1)?;
fitschan.set_encoding(encode);
let nobj = fitschan.write(&wcsinfo)?;
# assert_eq!(nobj, 1);
# Ok::<(), ast_core::error::AstError>(())
```

The value returned by `write` (assigned to `nobj`) indicates how many Objects were written.
This will either be 1 or zero.
A value of zero is used to indicate that the information could not be encoded in the form you requested.
If this happens, nothing will have been written.

If your choice of encoding proves inadequate, the probable reason is that the changes you have made to the FrameSet have caused it to depart from the data model which the encoding assumes.
AST knows about the data model used by each encoding and will attempt to simplify the FrameSet you provide so as to fit into that model, thus relieving you of the need to understand the details and limitations of each encoding yourself.
(Storing values in the FitsChan for FITS headers NAXIS1, NAXIS2, *etc.* (the grid dimensions in pixels), before invoking `write` can sometimes help to produce a successful write.)
When this attempt fails, however, you must consider what alternative encoding to use.

Ideally, you would probably want to try a sequence of alternative encodings, using an approach such as the following:

```rust
use ast_core::{Channel, Encoding};
# use ast_core::FitsChan;
# const HEADER: &str = "\
# SIMPLE  =                    T / Written by IDL:  30-Jul-1997 05:35:42.00
# BITPIX  =                  -32 / Bits per pixel.
# NAXIS   =                    2 / Number of dimensions
# NAXIS1  =                  300 / Length of x axis.
# NAXIS2  =                  300 / Length of y axis.
# CTYPE1  = 'GLON-ZEA'           / X-axis type
# CTYPE2  = 'GLAT-ZEA'           / Y-axis type
# CRVAL1  =           -149.56866 / Reference pixel value
# CRVAL2  =           -19.758201 / Reference pixel value
# CRPIX1  =              150.500 / Reference pixel
# CRPIX2  =              150.500 / Reference pixel
# CDELT1  =             -1.20000 / Degrees/pixel
# CDELT2  =              1.20000 / Degrees/pixel
# CROTA1  =              0.00000 / Rotation in degrees.
# SURVEY  = 'COBE DIRBE'
# BUNITS  = 'MJy/sr  '           /
# ORIGIN  = 'CDAC    '           / Cosmology Data Analysis Center
# TELESCOP= 'COBE    '           / COsmic Background Explorer satellite
# INSTRUME= 'DIRBE   '           / COBE instrument [DIRBE, DMR, FIRAS]
# PIXRESOL=                    9 / Quad tree pixel resolution [6, 9]
# DATE    = '27/09/94'           / FITS file creation date (dd/mm/yy)
# DATE-MAP= '16/09/94'           / Date of original file creation (dd/mm/yy)
# COMMENT     COBE specific keywords
# DATE-BEG= '08/12/89'           / date of initial data represented (dd/mm/yy)
# DATE-END= '25/09/90'           / date of final data represented   (dd/mm/yy)";
# let mut fitschan = FitsChan::new()?;
# for card in HEADER.lines() { fitschan.put_fits(card, false)?; }
# let encode = fitschan.encoding()?;
# fitschan.clear_card()?;
# let wcsinfo = fitschan.read()?.unwrap();

// 1.
fitschan.set_card(1)?;
fitschan.set_encoding(Encoding::FitsWcs);
if fitschan.write(&wcsinfo)? == 0 {
    // 2.
    fitschan.set_encoding(encode);
    if fitschan.write(&wcsinfo)? == 0 {
        // 3.
        fitschan.set_encoding(Encoding::Native);
        fitschan.write(&wcsinfo)?;
    }
}
# fitschan.clear_card()?;
# assert!(fitschan.find_fits("CTYPE1", false)?.is_some());
# Ok::<(), ast_core::error::AstError>(())
```

That is:

1. Start by trying the FITS-WCS encoding, on the grounds that FITS should provide a universal interchange standard in which all WCS information should be expressed if possible.
2. If that fails, then try the original encoding used for the input WCS information, on the grounds that you are at least not making the information any harder for others to read than it originally was.
3. If that also fails, then you are probably trying to store fairly complex information for which you need the native encoding.
   Only other AST programs will then be able to read this information, but these are probably the only programs that will be able to do anything sensible with it anyway.

An alternative approach might be to encode the WCS information in several ways, since this gives the maximum chance that other software will be able to read it.
This approach is only possible if there is no significant conflict between the FITS keywords used by the different encodings (in practice, this means you should avoid mixing FITS-IRAF, FITS-WCS, FITS-AIPS, FITS-AIPS++ and FITS-PC encodings since they share many keywords).
Adopting this approach would simply require multiple calls to `write`, rewinding the FitsChan and changing its Encoding value before each one.

Unfortunately, however, there is a drawback to duplicating WCS information in the FITS header in this way, because any program which modifies one version of this information and simply copies the remainder of the header will risk producing two inconsistent sets of information.
This could obviously be confusing to subsequent software.
Whether you consider this a worthwhile risk probably depends on the use to which you expect your data to be put.
