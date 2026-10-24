# FITS-WCS Coverage

This section gives details of the FitsChan class implementation of the conventions described in the FITS-WCS papers available at <http://fits.gsfc.nasa.gov/fits_wcs.html>.
These conventions are used only if the Encoding attribute of the FitsChan has the value [`Encoding::FitsWcs`](crate::Encoding::FitsWcs) (whether set explicitly or defaulted).
It should always be possible for a FrameSet to be read (using [`read`](crate::Channel::read)) from a FitsChan containing a header which conforms to these conventions.
However, only those FrameSets which are compatible with the FITS-WCS model can be *written* to a FitsChan using [`write`](crate::Channel::write).
For instance, if the current Frame of a FrameSet is re-mapped using, say, an arbitrary MathMap then the FrameSet will no longer be compatible with the FITS-WCS model, and so will not be written out successfully to a FitsChan.

The following sub-sections describe the details of the implementation of each of the first four FITS-WCS papers.
Here, the term "pixel axes" is used to refer to the FITS pixel coordinates (i.e. the center of the first image pixel has a value 1.0 on each pixel axis); the term "IWC axes" is used to refer to the axes of the Intermediate World Coordinate system; and the term "WCS axes" is used to refer to the axes of the final physical coordinate system described by the CTYPE*i* keywords.

## Paper I - General Linear Coordinates

When reading a FrameSet from a FitsChan, these conventions are used if the CTYPE*i* keyword values within the FitsChan do not conform to the conventions described in later papers, in which case the axes are assumed to be linear.
When writing a FrameSet to a FitsChan, these conventions are used for axes which are described by a simple Frame (*i.e.* not a SkyFrame, SpecFrame, *etc.*).

The table below describes the use made by AST of each keyword defined by FITS-WCS paper I.

**Use of FITS-WCS Paper I keywords**

| Keyword | Read | Write |
|---|---|---|
| WCSAXES*a* | Ignored. | Set to the number of axes in the WCS Frame - only written if different to NAXIS. |
| CRVAL*ia* | Used to create the pixel to WCS Mapping. | Always written (see "Choice of Reference Point" below). |
| CRPIX*ja* | Used to create the pixel to WCS Mapping. | Always written (see "Choice of Reference Point" below). |
| CDELT*ia* | Used to create the pixel to WCS Mapping. | Only written if the CDMatrix attribute of the FitsChan is set to `false`. |
| CROTA*i* | Used to create the pixel to WCS Mapping. | Only written in FITS-AIPS and FITS-AIPS++ encodings. |
| CTYPE*ia* | Used to choose the class and attributes of the WCS Frame, and to create the pixel to WCS Mapping (note, "STOKES" and "COMPLEX" axes are treated as unknown linear axes). | Always written (see "Use and Choice of CTYPE keywords" below). |
| CUNIT*ia* | Used to set the Unit attributes of the WCS Frame. | Only written if the Unit attribute of the WCS Frame has been set explicitly. If so, the Unit value for each axis is used as the CUNIT value. |
| PC*i_ja* | Used to create the pixel to WCS Mapping. | Only written if the CDMatrix attribute of the FitsChan is set to `false`. |
| CD*i_ja* | Used to create the pixel to WCS Mapping. | Only written if the CDMatrix attribute of the FitsChan is set to `true`. |
| PV*i_ma* | Ignored for linear axes. | Not written if the axes are linear. |
| PS*i_ma* | Ignored. | Not used. |
| WCSNAME*a* | Used to set the Domain attribute of the WCS Frame. | Only written if the Domain attribute of the WCS Frame has been set explicitly. If so, the Domain value is used as the WCSNAME value. |
| CRDER*ia* | Ignored. | Not used. |
| CSYER*ia* | Ignored. | Not used. |

The CDMatrix attribute is [`cd_matrix`](crate::FitsChan::cd_matrix).

### Requirements for a Successful Write Operation

When writing a FrameSet in which the WCS Frame is a simple Frame to a FitsChan, success depends on the Mapping from pixel coordinates (the base Frame in the FrameSet) to the WCS Frame being linear.
The write operation will fail if this is not the case.

### Use and Choice of CTYPE*i* keywords

When reading a FrameSet from a FitsChan the CTYPE*i* values in the FitsChan are used to set the Symbol attributes of the corresponding WCS Frame.
The Label attributes of the WCS Frame are set from the CNAME*i* keywords, if present in the header.
Otherwise they are set from the CTYPE*i* comments strings in the header, so long as each axis has a unique non-blank comment.
Otherwise, the Label attributes are set to the CTYPE*i* values.
The above procedure is over-ridden if the axis types conform to the conventions described in paper II or III, as described below.

When writing a FrameSet to a FitsChan, each CTYPE*i* value is set to the value of the Symbol attribute of the corresponding axis in the Frame being written.
If a value has been set explicitly for the axis Label attribute, it is used as the axis comment (except that any existing comments in the FitsChan take precedence if the keyword value has not changed).
The above procedure is over-ridden if the Frame is a SkyFrame or a SpecFrame, in which case the CTYPE*i* value is derived from the System attribute of the Frame and the nature of the pixel to WCS Mapping according to the conventions of papers II and III, as described below.

### Choice of Reference Point

When writing a FrameSet to a FitsChan, the pixel coordinates of the reference point for linear axes (i.e. the CRPIX*j* values) are chosen as follows:

- If the FrameSet is being written to a FitsChan which previously contained a set of axis descriptions with the same identifying letter, then the previous CRVAL*j* values are converted into the coordinate system of the Frame being written (if possible).
  These values are then transformed into the pixel Frame, and the closest integer pixel values are used as the CRPIX keywords.
- If the above step could not be performed for any reason, the central pixel is used as the reference point.
  This requires the image dimensions to be present in the FitsChan in the form of a set of NAXIS*j* keyword values.
- If both the above two steps failed for any axis, then the pixel reference position is set to a value of 1.0 on the pixel axis.

The pixel to WCS Mapping is then used to find the corresponding CRVAL*j* values.

Again, the above procedure is over-ridden if the Frame is a SkyFrame or a SpecFrame, in which case the conventions of papers II and III are used as described below.

### Choice of Axis Ordering

When reading a FrameSet from a FitsChan, WCS axis *i* in the current Frame of the resulting FrameSet corresponds to axis *i* in the FITS header.

When writing a FrameSet to a FitsChan, the axis ordering for the FITS header is chosen to make the CD*i_j* or PC*i_j* matrix predominately diagonal.
This means that the axis numbering in the FITS header will not necessarily be the same as that in the AST Frame.

### Alternate Axis Descriptions

When reading a FrameSet from a FitsChan which contains alternate axis descriptions, each complete set of axis descriptions results in a single Frame being added to the final FrameSet, connected via an appropriate Mapping to the base pixel Frame.
The Ident attribute of the Frame is set to hold the single alphabetical character which is used to identify the set of axis descriptions within the FITS header (a single space is used for the primary axis descriptions).

When writing a FrameSet to a FitsChan, it is assumed that the base Frame represents pixel coordinates, and the current Frame represents the primary axis descriptions.
If there are any other Frames present in the FrameSet, an attempt is made to create a complete set of "alternate" keywords describing each additional Frame.
The first character in the Ident attribute of the Frame is used as the single character descriptor to be appended to the keyword, with the proviso that a given character can only be used once.
If a second Frame is found with an Ident attribute which has already been used, its Ident attribute is ignored and the next free character is used instead.
Note, failure to write a set of alternate axis descriptions does not result in failure of the entire write operation: the primary axis descriptions are still written, together with any other alternate axis descriptions which can be produced successfully.

## Paper II - Celestial Coordinates

These conventions are used when reading a FrameSet from a FitsChan containing appropriate CTYPE*i* values, and when writing a FrameSet in which the WCS Frame is a SkyFrame.

The table below describes the use made by AST of each keyword whose meaning is defined or extended by FITS-WCS paper II.

**Use of FITS-WCS Paper II keywords**

| Keyword | Read | Write |
|---|---|---|
| CTYPE*ia* | All coordinate systems and projection types listed in paper II are supported (note, "CUBEFACE" axes are treated as unknown linear axes). In addition, "-HPX" (HEALPix) and "-XPH" (polar HEALPix) are supported. | Determined by the System attribute of the SkyFrame and the projection of the WcsMap within the FrameSet. |
| CUNIT*ia* | Ignored (assumed to be 'degrees'). | Not written. |
| PV*i_ma* | Used to create the pixel to WCS Mapping (values are stored as attributes of a WcsMap within this Mapping). | Values are obtained from the WcsMap in the pixel to WCS Mapping. |
| LONPOLE*a* | Used to create the pixel to WCS Mapping. Also stored as a PV*i_m* attribute for the longitude axis of the WcsMap. | Only written if not equal to the default value defined in paper II (see "Choice of LONPOLE/LATPOLE" below). |
| LATPOLE*a* | Used to create the pixel to WCS Mapping. Also stored as a PV attribute for the longitude axis of the WcsMap. | Only written if not equal to the default value defined in paper II (see "Choice of LONPOLE/LATPOLE" below). |
| RADESYS*a* | Used to set the attributes of the SkyFrame. All values supported except that ecliptic coordinates are currently always assumed to be FK5. | Always written. Determined by the System attribute of the SkyFrame. |
| EQUINOX*a* | Used to set the Equinox attribute of the SkyFrame. | Written if relevant. Determined by the Equinox attribute of the SkyFrame. |
| EPOCH | Used to set the Equinox attribute of the SkyFrame. | Only written if using FITS-AIPS and FITS-AIPS++ encodings. Determined by the Equinox attribute of the SkyFrame. |
| MJD-OBS | Used to set the Epoch attribute of the SkyFrame. DATE-OBS is used if MJD-OBS is not present. A default value based on RADESYS and EQUINOX is used if DATE-OBS is not present either. | Determined by the Epoch attribute of the SkyFrame. Only written if this attribute has been set to an explicit value (in which case DATE-OBS is also written). |

### Requirements for a Successful Write Operation

When writing a FrameSet in which the WCS Frame is a SkyFrame to a FitsChan, success depends on the following conditions being met:

1. The Mapping from pixel coordinates (the base Frame in the FrameSet) to the WCS SkyFrame includes a WcsMap.
2. The Mapping prior to the WcsMap (*i.e.* from pixel to IWC) is linear.
3. The Mapping after the WcsMap (*i.e.* from native spherical to celestial coordinates) is a spherical rotation for the celestial axes, and linear for any other axes.
4. The TabOK attribute ([`tab_ok`](crate::FitsChan::tab_ok)) is set to a positive value in the FitsChan, and the longitude and latitude axes are separable.
   In this case the Mapping will be described by a pair of 1-dimensional look-up tables, using the "-TAB" algorithm described in FITS-WCS paper III.

If none of the above conditions hold, the write operation will be unsuccessful.

### Choice of LONPOLE/LATPOLE

When writing a FrameSet to a FitsChan, the choice of LONPOLE and LATPOLE values is determined as follows:

1. If the projection represented by the WcsMap is azimuthal, then any values set for attributes "PV*i*_3" and "PV*i*_4" (where "*i*" is the index of the longitude axis) within the WcsMap are used as the LONPOLE and LATPOLE values.
   Reading a FrameSet from a FITS-WCS header results in the original LONPOLE and LATPOLE values being stored within a WcsMap within the FrameSet.
   Consequently, if a FrameSet is read from a FITS-WCS header and it is subsequently written out to a new FITS-WCS header, the original LONPOLE and LATPOLE values will usually be used in the new header (the exception being if the WcsMap has been explicitly modified before being written out again).
   Any extra rotation of the sky is absorbed into the CD*i_j* or PC*i_j* matrix (this is possible only if the projection is azimuthal).
2. If the projection represented by the WcsMap is azimuthal but no values have been set for the "PV*i*_3" and "PV*i*_4" attributes within the WcsMap, then the default LONPOLE and LATPOLE values are used.
   This results in no LONPOLE or LATPOLE keywords being stored in the header since default values are never stored.
   Any extra rotation of the sky is absorbed into the CD*i_j* or PC*i_j* matrix (this is possible only if the projection is azimuthal).
3. If the projection represented by the WcsMap is not azimuthal, then the values of LONPOLE and LATPOLE are found by transforming the coordinates of the celestial north pole (*i.e.* longitude zero, latitude +π/2) into native spherical coordinates using the inverse of the Mapping which follows the WcsMap.

### User Defined Fiducial Points

When reading a FrameSet from a FitsChan, projection parameters PV*i*_0, PV*i*_1 and PV*i*_2 (for longitude axis "*i*") are used to indicate a user-defined fiducial point as described in section 2.5 of paper II.
This results in a shift of IWC origin being applied *before* the WcsMap which converts IWC into native spherical coordinates.
The values of these projection parameters, if supplied, are stored as the corresponding PV*i_m* attributes of the WcsMap.

When writing a FrameSet to a FitsChan, the PV attributes of the WcsMap determine the native coordinates of the fiducial point (the fixed defaults for each projection described in paper II are used if the PV attributes of the WcsMap have not been assigned a value).
The corresponding celestial coordinates are used as the CRVAL*i* keywords and the corresponding pixel coordinates as the CRPIX*j* keywords.

### Common Non-Standard Features

A collection of common non-standard features are supported when reading a FrameSet from a FitsChan, in addition to those embodied within the available encodings of the FitsChan class.
These are translated into the equivalent standard features before being used to create a FrameSet.
Note, the reverse operation is never performed: it is not possible to produce non-standard features when writing a FrameSet to a FitsChan (other than those embodied in the available encodings of the FitsChan class).
The supported non-standard features include:

- EQUINOX keywords with string values equal to a date preceded by the letter B or J (*e.g.* "B1995.0").
- EQUINOX or EPOCH keywords with value zero (these are converted to B1950).
- The IRAF "ZPX" projection is represented by a WcsMap with a projection of [`WcsProjection::Zpn`](crate::mappings::WcsProjection::Zpn).
  Projection parameter values are read from any WAT*i_nnn* keywords, and corresponding PV*i_m* attributes are set in the WcsMap.
  The WAT*i_nnn* keywords may specify corrections to the basic ZPN projection by including "lngcor" or "latcor" terms.
  These are supported if they use half cross-terms, in either simple or Chebyshev representation.
- The IRAF "TNX" projection is represented by a WcsMap with a projection of [`WcsProjection::Tpn`](crate::mappings::WcsProjection::Tpn) (a distorted TAN projection retained within the WcsMap class from an early draft of the FITS-WCS paper II).
  Projection parameter values are read from any WAT*i_nnn* keywords, and corresponding PV attributes are set in the WcsMap.
  If the TNX projection cannot be converted exactly into a TPN projection, ASTWARN keywords are added to the FitsChan containing a warning message (but only if the Warnings attribute of the FitsChan, [`warning_conditions`](crate::FitsChan::warning_conditions), is set appropriately).
  Currently, TNX projections that use half cross-terms, in either simple or Chebyshev representation, are supported.
- "QV" parameters for TAN projections (as produced by AUTOASTROM, SUN/242) are renamed to the equivalent "PV" parameters.
- TAN projections that have associated "PV" parameters on the latitude axis are converted to the corresponding TPN (distorted TAN) projections.
  This conversion can be controlled using the PolyTan attribute of the FitsChan class ([`poly_tan`](crate::FitsChan::poly_tan)).

## Paper III - Spectral Coordinates

These conventions are used when reading a FrameSet from a FitsChan which includes appropriate CTYPE*i* values, and when writing a FrameSet in which the WCS Frame is a SpecFrame.

The table below describes the use made by AST of each keyword whose meaning is defined or extended by FITS-WCS paper III.

**Use of FITS-WCS Paper III keywords**

| Keyword | Read | Write |
|---|---|---|
| CTYPE*ia* | All coordinate systems and projection types listed in paper III are supported (the "-LOG" algorithm may also be applied to non-spectral linear axes; the "-TAB" algorithm requires the TabOK attribute to be set in the FitsChan). | Determined by the System attribute of the SpecFrame and the nature of the pixel to SpecFrame Mapping. |
| CUNIT*ia* | Used to set the Unit attribute of the SpecFrame (note, SpecFrames always have an "active" Unit attribute; see [`set_active_unit`](crate::frames::Frame::set_active_unit)). | Always written. |
| PV*i_ma* | Used to create the pixel to WCS Mapping (values are stored as attributes of a GrismMap). | Set from the attributes of the GrismMap, if present, and if set explicitly. |
| SPECSYS*a* | Used to set the StdOfRest attribute of the SpecFrame (all systems are supported except CMBDIPOL). | Set from the StdOfRest attribute of the SpecFrame, but only if it has been set explicitly. |
| SSYSOBS*a* | Ignored. | Never written. |
| OBSGEO-X/Y/Z | Used to set the ObsLon and ObsLat attributes of the Frame (the observer's height above sea level is ignored). | Set from the ObsLon and ObsLat attributes of the Frame, if they have been set explicitly (it is assumed that the observer is at sea level). |
| MJD-AVG | Used to set the Epoch attribute of the SpecFrame. | Set from the Epoch attribute of the SpecFrame, if it has been set explicitly. |
| SSYSSRC*a* | Used to set the SourceVRF attribute of the SpecFrame (all systems are supported except CMBDIPOL). | Set from the SourceVRF attribute of the SpecFrame. |
| ZSOURCE*a* | Used to set the SourceVel attribute of the SpecFrame (the SourceVRF attribute is first set to the system indicated by the SSYSSRC keyword, and the ZSOURCE value is then converted to an apparent radial velocity and stored as the SourceVel attribute). | Set from the SourceVel attribute of the SpecFrame, if it has been set explicitly (the SourceVel value is first converted from apparent radial velocity to redshift). |
| VELOSYS*a* | Ignored. | Set from the attributes of the SpecFrame that define the standard of rest and the observer's position. |
| RESTFRQ*a* | Used to set the RestFreq attribute of the SpecFrame. | Set from the RestFreq attribute of the SpecFrame, but only if the System attribute is not set to "WAVE", "VOPT", "ZOPT" or "AWAV", and only if RestFreq has been set explicitly. |
| RESTWAV*a* | Used to set the RestFreq attribute of the SpecFrame (after conversion from wavelength to frequency). | Set from the RestFreq attribute of the SpecFrame (after conversion), but only if the System attribute is set to "WAVE", "VOPT", "ZOPT" or "AWAV", and only if RestFreq has been set explicitly. |
| CNAME*ia* | Used to set the Label attributes of the WCS Frame. | Set from the Label attributes of the WCS Frame, if they have been set explicitly. |

### Requirements for a Successful Write Operation

When writing a FrameSet in which the WCS Frame is a SpecFrame to a FitsChan, the write operation is successful only if the Mapping from pixel coordinates (the base Frame in the FrameSet) to the SpecFrame satisfies one of the following conditions:

1. It is linear.
2. It is logarithmic.
3. It is linear if the SpecFrame were to be re-mapped into one of the other spectral systems supported by FITS-WCS paper III.
4. It contains a GrismMap, and the Mapping before the GrismMap (from pixel coordinates to grism parameter) is linear, and the Mapping after the GrismMap is either null or represents a change of spectral system from wavelength (air or vacuum) to one of the supported spectral systems.
5. The TabOK attribute is set to a positive value in the FitsChan.

If none of the above conditions hold, the write operation will be unsuccessful.
Note, if the FitsChan's TabOK attribute is set to a positive value then any Mapping that does not meet any of the earlier conditions will be written out as a look-up table, using the "-TAB" algorithm described in FITS-WCS paper III.
If the TabOK attribute is zero (the default) or negative in the FitsChan, then the write operation will be unsuccessful unless one of the earlier conditions is met.
(If the -TAB algorithm is used, the positive value of the TabOK attribute is used as the table version number (the EXTVER header) in the associated FITS binary table.)

### Common Non-Standard Features

The following non-standard features are supported when reading spectral axes from a FitsChan:

- Conversion of "-WAV", "-FRQ" and "-VEL" algorithm codes (specified in early drafts of paper III) to the corresponding "-X2P" form.
- Conversion of "RESTFREQ" to "RESTFRQ".

## Paper IV - Coordinate Distortions

This paper proposes that an additional 4 character code be appended to the end of the CTYPE*i* keyword to specify the nature of any distortion away from the basic algorithm described by the first 8 characters of the CTYPE*i* value.
Currently AST ignores all such codes when reading a FrameSet from a FitsChan (except for the "-SIP" code defined by the Spitzer Space Telescope project - see below).
This means that a FrameSet can still be read from such headers, but the Mapping which gives the WCS position associated with a given pixel position will reflect only the basic algorithm and will not include the effects of the distortion.

If such a FrameSet is then written out to a FitsChan, the resulting CTYPE*i* keywords will include no distortion code.

### The "-SIP" distortion code

The Spitzer Space Telescope project (<http://www.spitzer.caltech.edu/>) has developed its own system for encoding 2-dimensional image distortion within a FITS header, based on the proposals of paper IV.
In this system, the presence of distortion is indicated by appending the distortion code "-SIP" to the CTYPE*i* keyword values for the celestial axes.
The distortion takes the form of a polynomial function which is applied to the pixel coordinates, after subtraction of the CRPIX*j* values.

This system is a strictly 2 dimensional system.
When reading a FrameSet from a FitsChan which includes the "-SIP" distortion code, AST assumes that it is only applied to the first 2 WCS axes in a FITS header (i.e. CTYPE1 and CTYPE2).
If the "-SIP" distortion code is attached to other axes, it will be ignored.
The distortion itself is represented by a PolyMap within the resulting FrameSet.

If a FrameSet which includes such a PolyMap is written out to a FitsChan, SIP keywords are written if the SipOK attribute of the FitsChan ([`sip_ok`](crate::FitsChan::sip_ok)) is `true` (the default) and the Mapping can be arranged as the SIP scheme needs.
Otherwise, the write operation will fail unless the distortion is insignificant (*i.e.* is so small that the tests for linearity built into AST are passed).
In this case, no distortion code will be appended to the resulting CTYPE*i* keyword values.
