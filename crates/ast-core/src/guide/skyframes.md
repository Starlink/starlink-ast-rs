# Celestial Coordinate Systems (SkyFrames)

A Frame which is specialized for representing coordinate systems on the celestial sphere is obviously of great importance in astronomy.
The [`SkyFrame`](crate::frames::SkyFrame) is such a Frame.
In this section we examine the additional properties and behavior of a SkyFrame that distinguish it from a basic Frame ([Representing Coordinate Systems](crate::guide::frames)).

## The SkyFrame Model

A SkyFrame is, of course, a Frame ([Representing Coordinate Systems](crate::guide::frames)), so it has all the properties and behavior of a basic Frame.

A SkyFrame represents a 2-dimensional *spherical* coordinate system, in which the shortest distance between two points is a great circle.
A SkyFrame therefore always has exactly two axes which represent the longitude and latitude of a coordinate system residing on the celestial sphere.
Many such coordinate systems can be represented by a SkyFrame, as we will see shortly.

A SkyFrame can represent any of the commonly used celestial coordinate systems.
Optionally, the origin of the longitude/latitude system can be moved to any specified point in the standard celestial system, allowing a SkyFrame to represent offsets from a specified sky position.

When it is first created, a SkyFrame's axes are always in the order (longitude, latitude) but this can be changed, if required, by using [`perm_axes`](crate::frames::Frame::perm_axes) ([Permuting Frame Axes](crate::guide::frames#permuting-frame-axes)).
The order of the axes can be determined at any time using the LatAxis and LonAxis attributes ([`lat_axis`](crate::frames::SkyFrame::lat_axis) and [`lon_axis`](crate::frames::SkyFrame::lon_axis)).
A SkyFrame's coordinate values are always stored as angles in radians, regardless of the setting of the Unit attribute.
(The units used for the internal floating-point representation of an axis value can be determined by examining the InternalUnit attribute of the Frame, [`internal_unit`](crate::frames::Frame::internal_unit).
For most Frames, the Unit and InternalUnit attributes will be equal, but InternalUnit is always "rad" for SkyFrames.)

## Creating a SkyFrame

A SkyFrame with default attributes is created as follows:

```rust
use ast_core::frames::SkyFrame;

let skyframe = SkyFrame::unset_system();
```

Such a SkyFrame would represent the default celestial coordinate system which, at present, is the ICRS system.
Its System attribute is un-set, which matters when it is used as a template to search for other Frames.
If the ICRS system is what you need, it is best to ask for it explicitly, as described next.

## Specifying a Particular Celestial Coordinate System

For many purposes, the ICRS coordinate system is perfectly adequate.
In order to support conversion between a variety of celestial coordinate systems, however, you can create SkyFrames that represent any of these.

Selection of a particular coordinate system is performed simply by setting a value for the SkyFrame's System attribute, a [`SkySystem`](crate::frames::SkySystem).
This setting is most conveniently done when the SkyFrame is created.
For example, a SkyFrame representing the old FK4 (B1950.0) coordinate system would be created by:

```rust
use ast_core::frames::{SkyFrame, SkySystem};

let skyframe = SkyFrame::new(SkySystem::Fk4);
# assert_eq!(skyframe.equinox_year(), 1950.0);
```

Note that specifying the FK4 system also changes the associated equinox (from J2000.0 to B1950.0).
This is because the default value of the SkyFrame's Equinox attribute ([Attributes which Qualify Celestial Coordinate Systems](#attributes-which-qualify-celestial-coordinate-systems)) depends on the System attribute setting.

You may change the System value at any time with [`set_system`](crate::frames::SkyFrame::set_system), although this is not usually needed.
The values supported are set out in the description of [`SkySystem`](crate::frames::SkySystem) and include a variety of equatorial coordinate systems, together with ecliptic and galactic coordinates.

General spherical coordinates are supported by specifying [`SkySystem::Unknown`](crate::frames::SkySystem::Unknown).
You should note, though, that no Mapping can be created to convert between "unknown" coordinates and any of the other celestial coordinate systems (see *An Introduction to Coordinate System Conversions*<!-- link: crate::guide::conversions -->).

## Attributes which Qualify Celestial Coordinate Systems

Many celestial coordinate systems have some additional free parameters which serve to identify a particular coordinate system from amongst a broader class of related coordinate systems.
For example, the FK5 (J2010.0) system is distinguished from the FK5 (J2000.0) system by a different equinox — and the coordinates of a fixed astronomical source would have different values when expressed in these two systems.

In AST, these free parameters are represented by additional SkyFrame attributes, each of which has a default appropriate to (*i.e.* defined by) the setting of the main System attribute.
Each of these *qualifying attributes* may, however, be assigned an explicit value so as to select a particular coordinate system.
Note, it is usually best to assign explicit values whenever possible rather than relying on defaults.
Attributes should only be left at their default value if you "don't care" what value is used.
In certain circumstances (particularly, when aligning two Frames), a default value for an attribute may be replaced by the value from another similar Frame.
Such value replacement can be prevented by assigning an explicit value to the attribute, rather than simply relying on the default.

The main SkyFrame attributes which qualify the System attribute are:

- **Epoch**:
  This attribute is shared by every Frame.
  It gives the moment in time when the coordinates are correct for the astronomical source under study (usually the date of observation).
- **Equinox**:
  This value is used to qualify celestial coordinate systems that are notionally based on the Earth's equator and/or the ecliptic (the plane of the Earth's orbit around the Sun).
  The position of either of these planes is difficult to specify precisely, so in practice a model *mean* equator and/or ecliptic are used instead.
  These, together with the point on the sky that defines the coordinate origin (termed the *mean equinox*) move with time according to some model which smooths out the more rapid fluctuations.
  The SkyFrame class supports both the old FK4 model and the newer FK5 one.

  Coordinates expressed in any of these systems vary with time due to movement (by definition) of the coordinate system itself, and must therefore be qualified by a moment in time (the *epoch of the mean equinox*, or "equinox" for short) which specifies the position of the model coordinate system on the sky.
  This is the role of the Equinox attribute.

  Note that it is quite valid and common to relate the position of a source to an equinox other than the date of observation.
  Usually a standard equinox such as J2000.0 is used, meaning that the coordinates are referred to axes defined by where the model mean equator and ecliptic would lie on the sky at the Julian epoch J2000.0.

For further details of these attributes you should consult their descriptions ([`epoch_mjd`](crate::frames::Frame::epoch_mjd) and [`equinox_mjd`](crate::frames::SkyFrame::equinox_mjd)), and for details of the System settings for which they are relevant, see the description of [`SkySystem`](crate::frames::SkySystem).
For the interested reader, an excellent overview of celestial coordinate systems can also be found in the documentation for the SLALIB library (SUN/67).

The value of these qualifying attributes is most conveniently set at the same time as the System value, *e.g.* when a SkyFrame is created.
For instance:

```rust
use ast_core::frames::{SkyFrame, SkySystem};

let mut skyframe = SkyFrame::new(SkySystem::Ecliptic);
skyframe.set_equinox_year(2005.5);
# assert!((skyframe.equinox_year() - 2005.5).abs() < 1e-9);
```

would create a SkyFrame representing an ecliptic coordinate system referred to the mean equinox and ecliptic of Julian epoch J2005.5.

Note that it does no harm to assign values to qualifying attributes which are not relevant to the main System value.
Any such values are stored, but are not used unless the System value is later set so that they become relevant.

## Using Default SkyFrame Attributes

The default values supplied for many SkyFrame attributes will depend on the value of the SkyFrame's System attribute.
In practice, this means that there is usually little need to specify many of these attributes explicitly unless you have some special requirement.
This can be illustrated by using `show` to examine a SkyFrame, as follows:

```rust
use ast_core::frames::{SkyFrame, SkySystem};

let mut skyframe = SkyFrame::new(SkySystem::Fk4NoE);
skyframe.set_epoch_year(1958.0);
skyframe.show()?;
# Ok::<(), ast_core::error::AstError>(())
```

The output from this might look like the following:

```text
 Begin SkyFrame 	# Description of celestial coordinate system
#   Title = "FK4 equatorial coordinates; no E-terms; mean equinox B1950.0; epoch B1958.0" 	# Title of coordinate system
    Naxes = 2 	# Number of coordinate axes
#   Domain = "SKY" 	# Coordinate system domain
    Epoch = 1958 	# Besselian epoch of observation
#   Lbl1 = "Right ascension" 	# Label for axis 1
#   Lbl2 = "Declination" 	# Label for axis 2
    System = "FK4-NO-E" 	# Coordinate system type
#   Uni1 = "hh:mm:ss.s" 	# Units for axis 1
#   Uni2 = "ddd:mm:ss" 	# Units for axis 2
#   Dir1 = 0 	# Plot axis 1 in reverse direction
#   Bot2 = -1.5707963267948966 	# Lowest legal axis value
#   Top2 = 1.5707963267948966 	# Highest legal axis value
    Ax1 = 	# Axis number 1
       Begin SkyAxis 	# Celestial coordinate axis
       End SkyAxis
    Ax2 = 	# Axis number 2
       Begin SkyAxis 	# Celestial coordinate axis
       End SkyAxis
 IsA Frame 	# Coordinate system description
#   SkyTol = 0.001 	# Smallest significant separation [arc-sec]
#   Eqnox = 1950 	# Besselian epoch of mean equinox
 End SkyFrame
```

Note that the defaults (indicated by the "#" comment character at the start of the line) for attributes such as the Title, axis Labels and Format specifiers are all set to values appropriate for the particular equatorial coordinate system that the SkyFrame represents.

This means, for example, that if we were to use this SkyFrame to format a right ascension value stored in radians using [`format`](crate::frames::Frame::format) ([Formatting Axis Values](crate::guide::frames#formatting-axis-values)), it would automatically result in a string in sexagesimal notation (such as "12:14:35.7") suitable for display.
If we changed the value of the SkyFrame's Digits attribute (which is shared by every Frame), the number of digits appearing would also change accordingly.

These choices would be appropriate for a System value of FK4-NO-E, but if a different System value were set, the defaults would be correspondingly different.
For example, ecliptic longitude is traditionally expressed in degrees, so setting the System to [`SkySystem::Ecliptic`](crate::frames::SkySystem::Ecliptic) would result in coordinate values being formatted as degrees by default.

Of course, if you do not like any of these defaults, you may always over-ride them by setting explicit attribute values yourself.

## Formatting Celestial Coordinates

SkyFrames use [`format`](crate::frames::Frame::format) for formatting coordinate values in the same way as other Frames ([Formatting Axis Values](crate::guide::frames#formatting-axis-values)).
However, they offer a different set of formatting options more appropriate to celestial coordinates.

The Digits attribute of a SkyFrame behaves in essentially the same way as for a basic Frame, so the precision with which celestial coordinates are displayed can also be adjusted in this way.
However, the range of format specifiers that can be given for the Format(axis) attribute, and the default format resulting from any particular Digits value, is different.

The syntax of SkyFrame format specifiers is detailed under the description of [`format_str`](crate::frames::Frame::format_str).
Briefly, however, it allows celestial coordinates to be expressed either as angles or times and to include one or more of the fields:

- degrees or hours
- arc-minutes or minutes
- arc-seconds or seconds

with a specified number of decimal places for the final field.
A range of field separators is also available, as the following examples show, each formatting the same longitude of 219.0951°:

| Format Specifier | Example Formatted Value |
|---|---|
| `d` | `219` |
| `d.3` | `219.095` |
| `dm` | `219:06` |
| `dm.2` | `219:05.71` |
| `dms` | `219:05:42` |
| `hms.1` | `14:36:22.8` |
| `bdms.2` | `219 05 42.36` |
| `lhms.3` | `14h36m22.824s` |
| `+zlhms` | `+14h36m23s` |
| `ms.1` | `13145:42.4` |
| `lmst.3` | `876m22.824s` |
| `s.2` | `788742.36` |

```rust
use ast_core::frames::{Frame, SkyFrame, SkySystem};

let mut skyframe = SkyFrame::new(SkySystem::Fk5);
let value = 219.0951_f64.to_radians();
skyframe.set_format_str(1, "bdms.2")?;
assert_eq!(skyframe.format(1, value)?, "219 05 42.36");
skyframe.set_format_str(1, "lhms.3")?;
assert_eq!(skyframe.format(1, value)?, "14h36m22.824s");
# Ok::<(), ast_core::error::AstError>(())
```

Note the following key points:

- The required fields are specified using characters chosen from either "dms" or "hms" according to whether the value is to be formatted as an angle (in degrees) or a time (in hours).
- If no degrees or hours field is required, the distinction between angle and time may be made by including "t" to request time.
- The number of decimal places (for the final field) is indicated using "." followed by an integer.
  An asterisk can be used in place of an integer, in which case the number of decimal places is chosen so that the total number of digits in the formatted value is equal to the value of the Digits attribute.
- "b" causes fields to be separated by blanks, while "l" causes them to be separated by the appropriate letters (the default being a colon).
- "z" causes padding with leading zeros.
- "+" causes a plus sign to be prefixed to positive values (negative values always have a minus sign).

The formatting performed by a SkyFrame is also influenced by the AsTime(axis) attribute ([`as_time`](crate::frames::SkyFrame::as_time)), which has a boolean value for each SkyFrame axis.
It determines whether the default format specifier for an axis will present values as angles (*e.g.* in degrees) if it is `false`, or as times (*e.g.* in hours) if it is `true`.

The default AsTime value depends on the celestial coordinate system which the SkyFrame represents which, in turn, depends on its System attribute value.
For example, equatorial longitude values (right ascension) are normally expressed in hours, whereas ecliptic longitudes are normally expressed in degrees, so their default AsTime values will reflect this difference.

The value of the AsTime attribute may be set explicitly to over-ride these defaults if required, with the formatting precision being determined by the Digits/Digits(axis) value.
Alternatively, the Format(axis) attribute may be set explicitly to specify both the format and precision required.
Setting an explicit Format value always over-rides the effects of both the Digits and AsTime attributes (unless the Format value does not specify the required number of decimal places, in which case Digits is used to determine the default number of decimal places).

## Reading Formatted Celestial Coordinates

The process of converting formatted celestial coordinates, such as might be produced by [`format`](crate::frames::Frame::format) ([Formatting Celestial Coordinates](#formatting-celestial-coordinates)), into numerical coordinate values is performed by using [`unformat`](crate::frames::Frame::unformat) ([Reading Formatted Axis Values](crate::guide::frames#reading-formatted-axis-values)) on a SkyFrame.
The use of a SkyFrame means that the range of input formats accepted is appropriate to positions on the sky expressed as angles and/or times, while the returned value is in radians.

The following describes the forms of celestial coordinate which are supported:

- You may supply an optional sign, followed by between one and three fields representing either degrees, arc-minutes, arc-seconds or hours, minutes, seconds (*e.g.* "−12 42 03").
- Each field should consist of a sequence of one or more digits, which may include leading zeros.
  At most one field may contain a decimal point, in which case it is taken to be the final field (*e.g.* decimal degrees might be given as "124.707", while degrees and decimal arc-minutes might be given as "−13 33.8").
- The first field given may take any value, allowing angles and times outside the conventional ranges to be represented.
  However, subsequent fields must have values of less than 60 (*e.g.* "720 45 31" is valid, whereas "11 45 61" is not).
- Fields may be separated by white space or by ":" (colon), but the choice of separator must be used consistently throughout the value.
  Additional white space may be present around fields and separators (*e.g.* "− 2: 04 : 7.1").
- The following field identification characters may be used as separators to replace those above (or may be appended to the final field), in order to identify the field to which they are appended:

  | Character | Field |
  |---|---|
  | d | degrees |
  | h | hours |
  | m | minutes (of arc or time) |
  | s | seconds (of arc or time) |
  | ' | arc-minutes |
  | " | arc-seconds |

  Either lower or upper case may be used.
  Fields must be given in order of decreasing significance (*e.g.* "−11D 3' 14.4"" or "22h14m11.2s").
- The presence of certain field identification characters indicates whether the value is to be interpreted as an angle or a time (with 24 hours corresponding to 360 degrees), as follows:

  | Character | Meaning |
  |---|---|
  | d | angle |
  | ' | angle |
  | " | angle |
  | h | time |

  Incompatible angle/time identification characters may not be mixed (*e.g.* "10h14'3"" is not valid).
  The remaining field identification characters and separators do not specify a preference for an angle or a time and may be used with either.
- If no preference for an angle or a time is expressed anywhere within the value, then it is interpreted as an angle if the Format attribute string associated with the SkyFrame axis generates an angle and as a time otherwise.
  This ensures that values produced by `format` ([Formatting Celestial Coordinates](#formatting-celestial-coordinates)) are correctly interpreted by `unformat`.
- Fields may be omitted, in which case they default to zero.
  The remaining fields may be identified by using appropriate field identification characters (see above) and/or by adding extra colon separators (e.g. "−05m13s" is equivalent to "−:05:13").
  If a field is not identified explicitly, it is assumed that adjacent fields have been given, after taking account of any extra separator characters.
  For example:

  | Input | Fields |
  |---|---|
  | `10d` | degrees |
  | `10d12` | degrees and arc-minutes |
  | `11:14"` | arc-minutes and arc-seconds |
  | `9h13s` | hours and seconds of time |
  | `:45:33` | minutes and seconds (of arc or time) |
  | `:55:` | minutes (of arc or time) |
  | `::13` | seconds (of arc or time) |
  | `-6::2.5` | degrees/hours and seconds (of arc or time) |
  | `07m14` | minutes and seconds (of arc or time) |
  | `-8:14'` | degrees and arc-minutes |
  | `-h3:14` | minutes and seconds of time |
  | `h:2.1` | seconds of time |

- If fields are omitted in such a way that the remaining ones cannot be identified uniquely (e.g. "01:02"), then the first field (either given explicitly or implied by an extra leading colon separator) is taken to be the most significant field that `format` would produce when formatting a value (using the Format attribute associated with the SkyFrame axis).
  By default, this means that the first field will normally be interpreted as degrees or hours.
  However, if this does not result in consistent field identification, then the last field (either given explicitly or implied by an extra trailing colon separator) is taken to be the least significant field that `format` would produce.

```rust
use ast_core::frames::{Frame, SkyFrame, SkySystem};

let skyframe = SkyFrame::new(SkySystem::Fk5);
let degrees = |text: &str| -> ast_core::error::AstResult<f64> {
    Ok(skyframe.unformat(2, text)?.0.to_degrees())
};
assert!((degrees("-12 42 03")? + 12.7008333333).abs() < 1e-9);
assert!((degrees("-13 33.8")? + 13.5633333333).abs() < 1e-9);
assert!((degrees("-11D 3' 14.4\"")? + 11.054).abs() < 1e-9);
// Hours, for an axis that normally holds an angle.
assert!((degrees("22h14m11.2s")? - 333.5466666667).abs() < 1e-9);
// The second field may not exceed 59.
assert!(skyframe.unformat(2, "11 45 61").is_err());
# Ok::<(), ast_core::error::AstError>(())
```

This final convention is intended to ensure that values formatted by `format` which contain less than three fields will be correctly interpreted if read back using `unformat`, even if they do not contain field identification characters.
However, it also affects other forms of input.
For example, if the Format(axis) string were set to "mst.1" (producing two fields representing minutes and seconds of time), then formatted input would be interpreted by `unformat` as follows:

| Input | Fields |
|---|---|
| `12 13` | minutes and seconds |
| `12` | minutes |
| `:13` | seconds |
| `-18:` | minutes |
| `12.8` | minutes |
| `1 2 3` | hours, minutes and seconds |
| `4'` | arc-minutes |
| `60::"` | degrees |
| `-23:"` | arc-minutes |
| `-33h` | hours |

(in the last four cases, explicit field identification has been given which overrides the implicit identification).

Alternatively, if the Format(axis) string were set to "s.3" (producing only an arc-seconds field), then formatted input would be interpreted by `unformat` as follows:

| Input | Fields |
|---|---|
| `12.8` | arc-seconds |
| `12 13` | arc-minutes and arc-seconds |
| `:12` | arc-seconds |
| `13:` | arc-minutes |
| `1 2 3` | degrees, arc-minutes and arc-seconds |

In general, if you are preparing formatted input data containing celestial coordinates and wish to omit certain fields, then you are advised to identify clearly those that you do provide by using the appropriate field identification characters and/or extra colon separators.
This prevents you depending on the implicit field identification described above which, in turn, depends on an appropriate Format(axis) string having been set.

When writing software, it is also a good idea to set the Format(axis) string so that data input will be as simple as possible for the user.
Unless some special effect is desired, this normally means that it should contain "d" or "h" to ensure that the first field entered by the user will be interpreted as degrees or hours, unless otherwise identified.
This is the normal behavior unless an explicit Format(axis) value has been set to override the default.

## Representing Offsets from a Specified Sky Position

A SkyFrame can be modified so that its longitude and latitude axes are referred to an origin at any specified sky position.
Such a coordinate system is referred to as an "offset" coordinate system.
First, the System attribute should be set to represent the celestial coordinate system in which the origin is to be specified.
Then the SkyRef attribute ([`set_sky_ref`](crate::frames::SkyFrame::set_sky_ref)) should be set to hold the coordinates of the origin within the selected celestial coordinate system.

By default, "north" in the new offset coordinate system is parallel to north in the original celestial coordinate system.
However, the direction of north in the offset system can be controlled by assigning a value to the SkyRefP attribute ([`set_sky_ref_p`](crate::frames::SkyFrame::set_sky_ref_p)).
This attribute should be assigned the celestial coordinates of a point which is on the zero longitude meridian and which has non-zero latitude.

By default, the position given by the SkyRef attribute is used as the origin of the new longitude/latitude system, but an option exists to use it as the north pole of the system instead.
This option is controlled by the SkyRefIs attribute ([`set_sky_ref_is`](crate::frames::SkyFrame::set_sky_ref_is)).
The choice of value for SkyRefIs depends on what sort of offset coordinate system you want.
Setting SkyRefIs to [`SkyRefIs::Origin`](crate::frames::SkyRefIs::Origin) (the default) produces an offset coordinate system which is approximately Cartesian close to the specified position.
Setting SkyRefIs to [`SkyRefIs::Pole`](crate::frames::SkyRefIs::Pole) produces an offset coordinate system which is approximately Polar close to the specified position.

```rust
use ast_core::frames::{SkyFrame, SkyRefIs, SkySystem};

// Offsets from RA 10h, Dec +20d (FK5 J2000).
let mut offsets = SkyFrame::new(SkySystem::Fk5);
offsets.set_sky_ref(1, 150.0_f64.to_radians())?;
offsets.set_sky_ref(2, 20.0_f64.to_radians())?;
offsets.set_sky_ref_is(SkyRefIs::Origin);
# Ok::<(), ast_core::error::AstError>(())
```
