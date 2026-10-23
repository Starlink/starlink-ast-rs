# Representing Coordinate Systems (Frames)

An AST Frame is an Object that is used to represent a coordinate system.
Contrast this with a Mapping ([Inter-Relating Coordinate Systems](crate::guide::mappings)), which is used to describe how to convert between coordinate systems.
The two concepts are complementary and we will see how they work together in [Coordinate System Networks](crate::guide::framesets).

In this section we will discuss only basic Frames, which represent Cartesian coordinate systems.
More specialized types of Frame (*e.g.* the SkyFrame, which represents celestial coordinate systems, and the SpecFrame, which represents spectral coordinate systems) are covered later ([Celestial Coordinate Systems](crate::guide::skyframes) and [Spectral Coordinate Systems](crate::guide::specframes)) and, naturally, share the properties and behavior of the simple Frames discussed here.
What every kind of Frame has in common is the [`Frame`](crate::frames::Frame) trait; a basic Frame is a [`BasicFrame`](crate::frames::BasicFrame).

## The Frame Model

The best way to think about a Frame is like the frame that you would plot around a graph.
In two dimensions, you would have an "x" and a "y" axis, a title on the graph and labels on the axes, together with an indication of the physical units being plotted.
The values marked along each axis would be formatted in a human-readable way.
The frame around a graph therefore defines a coordinate space within which you can locate points, draw lines, calculate distances, *etc.*

An AST Frame works in much the same way, embodying all of these concepts and a few more.
It also allows any number of axes, which means that a Frame can represent coordinate systems with any number of dimensions.
You specify how many when you create it.

Remember that the basic Frame we are considering here is completely general.
It knows nothing of celestial coordinates, for example, and all its axes are equivalent.
It can be adapted to describe any general purpose Cartesian coordinate system by setting its attributes, such as its Title and axis Labels, *etc.* to appropriate values.

## Creating a Frame

Creating a Frame is straightforward and follows the usual pattern:

```rust
use ast_core::frames::BasicFrameBuilder;

let frame = BasicFrameBuilder::new(2).build()?;
# Ok::<(), ast_core::error::AstError>(())
```

The argument of [`BasicFrameBuilder::new`](crate::frames::BasicFrameBuilder::new) specifies the number of axes which the Frame should have.
The builder can also set the Frame's attributes before it is built, as we will see below.

## Using a Frame as a Mapping

In the C library, a Frame is also a Mapping, which implements an identity transformation just like a UnitMap ([Example — the UnitMap](crate::guide::mappings#example--the-unitmap)), with Nin and Nout attributes both equal to the number of Frame axes.
In Rust, [`Frame`](crate::frames::Frame) and [`Mapping`](crate::mappings::Mapping) are separate traits, and a basic Frame, a SkyFrame, a SpecFrame, a TimeFrame or a CmpFrame cannot be used as a Mapping.
Where you need the identity transformation such a Frame would have given, use a [`UnitMap`](crate::mappings::UnitMap) with the Frame's number of axes:

```rust
use ast_core::frames::{BasicFrameBuilder, Frame};
use ast_core::mappings::{Mapping, UnitMap};

let frame = BasicFrameBuilder::new(2).build()?;
let identity = UnitMap::new(frame.naxes());
let (x, y) = identity.tran2(&[1.0, 2.0], &[2.0, 4.0], true)?;
assert_eq!((x, y), (vec![1.0, 2.0], vec![2.0, 4.0]));
# Ok::<(), ast_core::error::AstError>(())
```

A FrameSet ([Using a FrameSet as a Mapping](crate::guide::framesets#using-a-frameset-as-a-mapping)) and every Region are both Frames and Mappings, and when we consider these we will see that using them as Mappings can be very useful indeed.

## Frame Axis Attributes

Frames have a number of attributes which can take multiple values, one for each axis.
These separate values are identified by an axis number, which the method for the attribute takes as its first argument.
For example, the Label attribute of the first axis is a character string containing the label which appears on that axis.
Axes are numbered from 1.

Axis attributes are accessed in the same way as all other attributes ([Getting Attribute Values](crate::guide::objects#getting-attribute-values), [Setting Attribute Values](crate::guide::objects#setting-attribute-values) and [Testing, Clearing and Defaulting Attributes](crate::guide::objects#testing-clearing-and-defaulting-attributes)).
For example, the Label on the second axis might be obtained as follows:

```rust
use ast_core::frames::{BasicFrameBuilder, Frame};

# let frame = BasicFrameBuilder::new(2).build()?;
let label = frame.label(2)?;
# Ok::<(), ast_core::error::AstError>(())
```

The other attribute access methods ([`set_label`](crate::frames::Frame::set_label), [`test_label`](crate::frames::Frame::test_label) and [`clear_label`](crate::frames::Frame::clear_label)) take an axis number in the same way.
Because an axis number may not be valid for the Frame, each returns an `AstResult`.

The following will print out the Label value for each axis of a Frame:

```rust
use ast_core::frames::{BasicFrameBuilder, Frame};

# let frame = BasicFrameBuilder::new(2).build()?;
let naxes = frame.naxes();
for iaxis in 1..=naxes {
    let label = frame.label(iaxis)?;
    println!("Label {iaxis:2}: {label}");
}
# assert_eq!(frame.label(1)?, "Axis 1");
# Ok::<(), ast_core::error::AstError>(())
```

Note the use of [`naxes`](crate::frames::Frame::naxes), the Naxes attribute, to determine the number of Frame axes.

The output from this might look like the following:

```text
Label  1: Axis 1
Label  2: Axis 2
```

In this case, the Frame's default axis Labels have been revealed as rather un-exciting.
Normally, you would set much more useful values, typically when you create the Frame — perhaps something like:

```rust
use ast_core::frames::BasicFrameBuilder;

let frame = BasicFrameBuilder::new(2)
    .label(1, "Offset from centre of field")
    .unit(1, "mm")
    .label(2, "Transmission coefficient")
    .unit(2, "%")
    .build()?;
# Ok::<(), ast_core::error::AstError>(())
```

Here, we have also set the (character string) Unit attribute for each axis to describe the physical units represented on that axis.

## Frame Attributes

We will now briefly outline the various attributes associated with a Frame.
We will not delve too deeply into the details of each attribute, for which you should consult the documentation of the [`Frame`](crate::frames::Frame) trait.
Instead, we aim simply to sketch the range of facilities available:

- **Naxes**:
  The number of Frame axes, which cannot be changed ([`naxes`](crate::frames::Frame::naxes)).
- **Title**:
  A string describing the coordinate system which the Frame represents ([`title`](crate::frames::Frame::title)).
- **Label(axis)**:
  A label string for each axis ([`label`](crate::frames::Frame::label)).
- **Unit(axis)**:
  A string describing the physical units on each axis ([`unit`](crate::frames::Frame::unit)).
  You can choose whether to make this attribute "active" or "passive" (using [`set_active_unit`](crate::frames::Frame::set_active_unit)).
  If active, its value will be taken into account when finding the Mapping between two Frames (*e.g.* a scaling of 0.001 would be used to connect two axes with units of "km" and "m").
  If passive, its value is ignored.
  Its use is described in more detail in [The Unit Attribute](#the-unit-attribute).
- **Symbol(axis)**:
  A string containing a "short form" symbol (*e.g.* like "X" or "Y") used to represent the quantity plotted on each axis ([`symbol`](crate::frames::Frame::symbol)).
- **Digits/Digits(axis)**:
  The preferred number of digits of precision to be used when formatting values for display on each axis ([`digits`](crate::frames::Frame::digits) and [`axis_digits`](crate::frames::Frame::axis_digits)).
- **Format(axis)**:
  A string containing a *format specifier* which determines exactly how values should be formatted for display on each axis ([`format_str`](crate::frames::Frame::format_str); see [Formatting Axis Values](#formatting-axis-values)).
  If this attribute is un-set, the formatting is based on the Digits value, otherwise the Format string over-rides the Digits value.
- **Direction(axis)**:
  A boolean value which indicates in which direction each axis should be plotted ([`direction`](crate::frames::Frame::direction)).
  If it is `true` (the default), the axis should be plotted in the conventional direction — *i.e.* increasing to the right for the abscissa and increasing upwards for the ordinate.
  If it is `false`, the axis should be plotted in reverse.
  This attribute is provided as a hint only and programs are free to ignore it if they wish.
- **Domain**:
  A character string which identifies the *physical domain* to which the Frame's coordinate system applies ([`domain`](crate::frames::Frame::domain)).
  The primary purpose of this attribute is to prevent unwanted conversions from occurring between coordinate systems which are not related.
  Its use is described in more detail in [The Domain Attribute](#the-domain-attribute).
- **System**:
  Identifies the specific coordinate system used to describe positions within the physical domain represented by the Frame.
  For a simple Frame, this attribute currently has a fixed value of "Cartesian", but could in principle be extended to include options such as "Polar", "Cylindrical", *etc*.
  More specialized Frames such as the SkyFrame, TimeFrame and SpecFrame, re-define the allowed values to be appropriate to the domain which they describe.
  For instance, the SkyFrame allows values such as "FK4" and "Galactic", and the SpecFrame allows values such as "frequency" and "wavelength".
  Each kind of Frame has its own type for the value, such as [`SkySystem`](crate::frames::SkySystem) for a SkyFrame.
- **Epoch**:
  This value is used to qualify a coordinate system by giving the moment in time when the coordinates are correct ([`epoch_mjd`](crate::frames::Frame::epoch_mjd)).
  Usually, this will be the date of observation.
  The Epoch value is important in cases where coordinate systems move with respect to each other over time.
  An example of two such coordinate systems are the FK4 and FK5 celestial coordinate systems.
- **ObsLon**:
  Specifies the longitude of the observer (assumed to be on the surface of the earth) ([`obs_lon`](crate::frames::Frame::obs_lon)).
  The basic Frame class does not use this value, but specialized Frames may.
  For instance, the SpecFrame class uses it to calculate the relative velocity of the observer and the center of the earth for use in converting between standards of rest.
- **ObsLat**:
  Specifies the latitude of the observer ([`obs_lat`](crate::frames::Frame::obs_lat)).
  Use in conjunction with ObsLon.

There are also some further Frame attributes, not described above, which are important when Frames are used as templates to search for other Frames.
Their use goes beyond the present discussion.

## Formatting Axis Values

The coordinate values associated with each axis of a Frame are stored (*e.g.* within your program) as `f64` values.
The Frame trait therefore provides a method, [`format`](crate::frames::Frame::format), to convert these values into formatted strings for display:

```rust
use ast_core::frames::{BasicFrameBuilder, Frame};

# let frame = BasicFrameBuilder::new(2).build()?;
# let (iaxis, value) = (1, std::f64::consts::PI);
let string = frame.format(iaxis, value)?;
# assert_eq!(string, "3.141593");
# Ok::<(), ast_core::error::AstError>(())
```

Here, `format` is passed the number of an axis (`iaxis`) and a value to format (`value`).
It returns a string containing the formatted value.

By default, the formatting applied will be determined by the Frame's Digits attribute and will normally display results with seven digits of precision.
Setting a different Digits value, however, allows you to adjust the precision as necessary to suit the accuracy of the coordinate data you are processing.
If finer control is needed, it is also possible to set a Digits value for each individual axis with [`set_axis_digits`](crate::frames::Frame::set_axis_digits).
If this is done, it over-rides the effect of the Frame's main Digits value for that axis.

Even finer control is possible by setting the (character string) Format attribute for a Frame axis.
The string given should contain a C-style *format specifier* which explicitly determines how the values on that axis should be formatted.
This will over-ride the effects of any Digits value (the exception to this rule is that if the Format value includes a precision of ".\*", then Digits will be used to determine the actual precision used).
Any valid "printf" format specifier may be used so long as it consumes exactly one double value.

```rust
use ast_core::frames::{BasicFrameBuilder, Frame};

let mut frame = BasicFrameBuilder::new(2).build()?;
frame.set_axis_digits(1, 3)?;
assert_eq!(frame.format(1, std::f64::consts::PI)?, "3.14");
frame.set_format_str(2, "%10.5f")?;
assert_eq!(frame.format(2, std::f64::consts::PI)?, "   3.14159");
# Ok::<(), ast_core::error::AstError>(())
```

It is recommended that you use `format` whenever you display formatted coordinate values, even although you could format them yourself.
This is because it puts the Frame in control of formatting.
When you start to handle more elaborate Frames (representing, say, celestial coordinates), you will need different formatting methods.
This approach delivers them without any change to your software.

You should also consider regularly using the [`norm`](crate::frames::Frame::norm) method, described below ([Normalizing Frame Coordinates](#normalizing-frame-coordinates)), for any values that will be made visible to the user of your software.

## Normalizing Frame Coordinates

The method [`norm`](crate::frames::Frame::norm) is provided to cope with the fact that some coordinate systems do not extend indefinitely in all directions.
Some may have boundaries, outside which coordinates are meaningless, while others wrap around on themselves, so that after a certain distance you return to the beginning again (coordinate systems based on circles and spheres, for instance).
A basic Frame has no such complications, but other more specialized Frames (such as SkyFrames, representing the celestial sphere — [Celestial Coordinate Systems](crate::guide::skyframes)) do.

The role played by `norm` is to *normalize* any arbitrary set of coordinates by converting them into a set which is "within bounds", interpreted according to the particular Frame in question.
For example, on the celestial sphere, a right ascension value of 24 hours or more can have a suitable multiple of 24 hours subtracted without affecting its meaning and `norm` would perform this task.
Similarly, negative values of right ascension would have a multiple of 24 hours added, so that the result lies in the range zero to 24 hours.
The coordinates in question are modified in place by `norm`, as follows:

```rust
use ast_core::frames::{BasicFrameBuilder, Frame};

# let frame = BasicFrameBuilder::new(2).build()?;
let mut point = [1.0, 2.0];
frame.norm(&mut point)?;
# assert_eq!(point, [1.0, 2.0]);
# Ok::<(), ast_core::error::AstError>(())
```

If the coordinates supplied are initially OK, as they would always be with a basic Frame, then they are returned unchanged.

Because the main purpose of `norm` is to convert coordinates into the preferred range for human consumption, its use is almost always appropriate immediately before formatting coordinate values for display using `format` ([Formatting Axis Values](#formatting-axis-values)).
Even if the Frame in question does not restrict the range of coordinates, so that `norm` does nothing, using it will allow you to process other more specialized Frames, where normalization is important, without changing your software.

## Reading Formatted Axis Values

The process of converting a formatted coordinate value for a Frame axis, such as might be produced by `format` ([Formatting Axis Values](#formatting-axis-values)), back into a numerical value ready for processing is performed by [`unformat`](crate::frames::Frame::unformat).
However, although this process is essentially the inverse of that performed by `format`, there are a number of additional difficulties that must be addressed in practice.

The main use for `unformat` is in reading formatted coordinate values which have been entered by the user of a program, or read from a file.
As such, we can rarely assume that the values are neatly formatted in the way that `format` would produce.
Instead, it is usually desirable to allow considerable flexibility in the form of input that can be accommodated, so as to permit "free-format" data input by the user.
In addition, we may need to extract individual coordinate values embedded in other textual data.

Underlying these requirements is the root difficulty that the textual format used to represent a coordinate value will depend on the class of Frame we are considering.
For example, for a basic Frame, `unformat` may have to read a value like "1.25e-6", whereas for a more specialized Frame representing celestial coordinates it may have to handle a value like "-07d 49m 13s".
Of course, the format might also depend on which axis is being considered.

Ideally, we would like to write software that can handle any kind of Frame.
However, this makes it a little more difficult to analyze textual input data to extract individual coordinate values, since we cannot make assumptions about how the values are formatted.
It would not be safe, for example, simply to assume that the values being read are separated by white space.
This is not just because they might be separated by some other character, but also because celestial coordinate values might themselves contain spaces.
In fact, to be completely safe, we cannot make any assumptions about how a formatted coordinate value is separated from the surrounding text, except that it should be separated in some way which is not ambiguous.

This is the very basic assumption upon which `unformat` works.
It is invoked as follows:

```rust
use ast_core::frames::{BasicFrameBuilder, Frame};

# let frame = BasicFrameBuilder::new(2).build()?;
# let (iaxis, string) = (1, "1.0");
let (value, n) = frame.unformat(iaxis, string)?;
# Ok::<(), ast_core::error::AstError>(())
```

It is supplied with the number of an axis (`iaxis`) and a string to be read (`string`).
If it succeeds in reading a value, `unformat` returns the resulting coordinate as `value`, together with `n`, the number of bytes that were read from the string in order to obtain this result.

The string is read as follows:

1. Any white space at the start is skipped over.
2. Further characters are considered, one at a time, until the next character no longer matches any of the acceptable forms of input (given the characters that precede it).
   The longest sequence of characters which matches is then considered "read".
3. If a suitable sequence of characters was read successfully, it is converted into a coordinate value which is returned.
   Any white space following this sequence is then skipped over and the total number of characters consumed is returned.
4. If the sequence of characters read is empty, or insufficient to define a coordinate value, then the string does not contain a value to read.
   In this case, the read is aborted and `unformat` returns zero characters read, and no meaningful coordinate value.
   However, it returns without error.

Note that failing to read a coordinate value does not constitute an error, at least so far as `unformat` is concerned.
However, an error can occur if the sequence of characters read appears to have the correct form but cannot be converted into a valid coordinate value.
Typically, this will be because it violates some constraint, such as a limit on the value of one of its fields.
The resulting error will give details.

For any given Frame axis, `unformat` does not necessarily always use the same algorithm for converting the sequence of characters it reads into a coordinate value.
This is because some forms of input (particularly free-format input) can be ambiguous and might be interpreted in several ways depending on the context.
For example, the celestial longitude "12:34:56.7" could represent an angle in degrees or a right ascension in hours.
To decide which to use, `unformat` may examine the Frame's attributes and, in particular, the appropriate Format(axis) string which is used by `format` when formatting coordinate values ([Formatting Axis Values](#formatting-axis-values)).
This is done in order that `format` and `unformat` should complement each other — so that formatting a value and then un-formatting it will yield the original value, subject to any rounding error.

To give a simple (but crucially incomplete!) example, consider reading a value for the axis of a basic Frame, as follows:

```rust
# use ast_core::frames::{BasicFrameBuilder, Frame};
# let frame = BasicFrameBuilder::new(2).build()?;
# let iaxis = 1;
let (value, n) = frame.unformat(iaxis, " 1.5e6   -99.0")?;
# assert_eq!((value, n), (1.5e6, 9));
# Ok::<(), ast_core::error::AstError>(())
```

`unformat` will skip over the initial space in the string supplied and then examine each successive character.
It will accept the sequence "1.5e6" as input, but reject the space which follows because it does not form part of the format of a floating point number.
It will then convert the characters "1.5e6" into a coordinate value and skip over the three spaces which follow them.
The number of characters read will therefore be 9, equal to the total number of characters consumed.
This result may be used to address the string during a subsequent read, so as to commence reading at the start of "-99.0".

Most importantly, however, note that if the user of a program mistakenly enters the string " 1.5r6…" instead of " 1.5e6…", a coordinate value of 1.5 and a count of 4 will be returned, because the "r" would prematurely terminate the attempt to read the value.
Because this sort of mistake does not automatically result in an error but can produce incorrect results, it is **vital** to check the returned count to ensure that the expected number of characters have been read.
For example, if the string is expected to contain exactly one value, and nothing else, then the following would suffice:

```rust
# use ast_core::frames::{BasicFrameBuilder, Frame};
# let frame = BasicFrameBuilder::new(2).build()?;
# let (iaxis, string) = (1, " 1.5r6");
let (value, n) = frame.unformat(iaxis, string)?;
if n < string.len() || n == 0 {
    // <error in input data>
#   assert_eq!((value, n), (1.5, 4));
} else {
    // <value read correctly>
#   unreachable!();
}
# Ok::<(), ast_core::error::AstError>(())
```

If `unformat` does not detect an error itself, we check that it has read to the end-of-string and consumed at least one character (which traps the case of a zero-length input string).
If this reveals an error, the value of `n` indicates where it occurred.

Another common requirement is to obtain a position by reading a list of coordinates from a string which contains one value for each axis of a Frame.
We assume that the values are separated in some unambiguous manner, perhaps using white space and/or some unspecified single-character separator.
The choice of separator is up to the data supplier, who must choose it so as not to conflict with the format of the coordinate values, but our software does not need to know what it is.
The following is a template algorithm for reading data in this form:

```rust
use ast_core::frames::{BasicFrameBuilder, Frame};

# let frame = BasicFrameBuilder::new(3).build()?;
# for string in ["1 2.5 3", "3.1,3.2,3.3", "1.5, 2.6, -9.9e2", "-1.1+0.4-1.8", "    .1/.2/.3", " 44.0 ; 55.1 -14"] {
// Initialise a string slice.
let mut s = string;

// Obtain the number of Frame axes and loop through them.
let naxes = frame.naxes();
let mut values = vec![0.0; naxes];
let mut n = 0;
for iaxis in 1..=naxes {
    // Attempt to read a value for this axis.
    (values[iaxis - 1], n) = frame.unformat(iaxis, s)?;

    // If nothing was read and this is not the first axis or the
    // end-of-string, try stepping over a separator and reading again.
    if n == 0 && iaxis > 1 && !s.is_empty() {
        s = &s[1..];
        (values[iaxis - 1], n) = frame.unformat(iaxis, s)?;
    }

    // Quit if nothing was read, otherwise move on to the next value.
    if n == 0 {
        break;
    }
    s = &s[n..];
}

// Check for possible errors.
if !s.is_empty() || n == 0 {
    // <error in input data>
#   unreachable!("{string:?}");
} else {
    // <values read correctly>
}
# }
# Ok::<(), ast_core::error::AstError>(())
```

In this case, `s` will start at the location of any input error.

Note that this algorithm is insensitive to the precise format of the data and will therefore work with any class of Frame and any reasonably unambiguous input data.
For example, here is a range of suitable input data for a 3-dimensional basic Frame:

```text
1 2.5 3
3.1,3.2,3.3
1.5, 2.6, -9.9e2
-1.1+0.4-1.8
    .1/.2/.3
 44.0 ; 55.1 -14
```

## Permuting Frame Axes

Once a Frame has been created, it is not possible to change the number of axes it contains, but it is possible to change the order in which these axes occur.
To do so, an integer *permutation array* is filled with the numbers of the axes so as to specify the new order, *e.g.*:

```rust
let perm = [2, 1];
```

In this case, the axes of a 2-dimensional Frame could be interchanged by passing this permutation array to [`perm_axes`](crate::frames::Frame::perm_axes).
That is, an (x₁,x₂) coordinate system would be changed into an (x₂,x₁) coordinate system by:

```rust
# use ast_core::frames::{BasicFrameBuilder, Frame};
# let mut frame = BasicFrameBuilder::new(2).label(1, "X").build()?;
# let perm = [2, 1];
frame.perm_axes(&perm)?;
# assert_eq!(frame.label(2)?, "X");
# Ok::<(), ast_core::error::AstError>(())
```

If the axes are permuted more than once, the effects are cumulative.
You are, of course, not restricted to Frames with only two axes.

## Selecting Frame Axes

An alternative to changing the number of Frame axes, which is not allowed, is to create a new Frame by selecting axes from an existing one.
The method of doing this is very similar to the way `perm_axes` is used ([Permuting Frame Axes](#permuting-frame-axes)), in that we supply an integer array filled with the numbers of the axes we want, in their new order.
In this case, however, the number of array elements need not equal the number of Frame axes.

For example, we could select axes 3 and 2 (in that order) from a 3-dimensional Frame as follows:

```rust
use ast_core::frames::{BasicFrameBuilder, pick_axes};

# let frame1 = BasicFrameBuilder::new(3).build()?;
let pick = [3, 2];
let (frame2, mapping) = pick_axes(&frame1, &pick)?;
# Ok::<(), ast_core::error::AstError>(())
```

This would return a 2-dimensional Frame (`frame2`) which contains the information associated with axes 3 and 2, in that order, from the original Frame (`frame1`).
The original Frame is not altered by this process, and the new Frame is independent of it.

In addition to the new Frame, [`pick_axes`](crate::frames::pick_axes) also returns a new Mapping.
This Mapping will inter-relate the two Frames.
By this we mean that its forward transformation will convert coordinates originally in the coordinate system represented by `frame1` into that represented by `frame2`, while its inverse transformation will convert in the opposite direction.
In this particular case, the Mapping would be a PermMap ([Example — the PermMap](crate::guide::mappings#example--the-permmap)) and would implement the following transformations:

```text
Forward:
   (1, 2, 3) --> (3, 2)
   (2, 4, 6) --> (6, 4)
   (3, 6, 9) --> (9, 6)
   (4, 8, 12) --> (12, 8)
   (5, 10, 15) --> (15, 10)

Inverse:
   (3, 2) --> (<bad>, 2, 3)
   (6, 4) --> (<bad>, 4, 6)
   (9, 6) --> (<bad>, 6, 9)
   (12, 8) --> (<bad>, 8, 12)
   (15, 10) --> (<bad>, 10, 15)
```

```rust
# use ast_core::frames::{BasicFrameBuilder, pick_axes};
use ast_core::constants::BAD_VALUE;
use ast_core::coords::PointSet;
use ast_core::mappings::Mapping;

# let frame1 = BasicFrameBuilder::new(3).build()?;
# let (frame2, mapping) = pick_axes(&frame1, &[3, 2])?;
let picked = PointSet::from_rows([[3.0, 2.0]]);
let back = mapping.transform(&picked, false)?;
assert_eq!(&*back.point(0), [BAD_VALUE, 2.0, 3.0]);
# Ok::<(), ast_core::error::AstError>(())
```

This is our first introduction to the idea of inter-relating pairs of Frames *via* a Mapping, but this will assume a central role later on.

Note that when using `pick_axes`, it is also possible to request more axes than there were in the original Frame.
This will involve selecting axes from the original Frame that do not exist.
To do this, the corresponding axis number (in the `pick` array) should be set to zero and the effect is to introduce an additional new axis which is not derived from the original Frame.
This axis will have default values for all its attributes.
You will need to do this because `pick_axes` does not allow you to select any of the original axes more than once.
(It will probably not be obvious why this restriction is necessary, but consider creating a Frame with one longitude axis and two latitude axes.
Which latitude axis should be associated with the longitude axis?)

## Calculating Distances, Angles and Offsets

Some complementary methods are provided for use with Frames to allow you to perform geometric operations without needing to know the nature of the coordinate system represented by the Frame.

Methods can be used to find the distance between two points, and to offset a specified distance along a line joining two points, *etc.*
In essence, these define the metric of the coordinate space which the Frame represents.
In the case of a basic Frame, this is a Cartesian metric.

The first of these methods, [`distance`](crate::frames::Frame::distance), returns a distance value when supplied with the Frame coordinates of two points.
For example:

```rust
use ast_core::frames::{BasicFrameBuilder, Frame};

# let frame = BasicFrameBuilder::new(2).build()?;
let point1 = [0.0, 0.0];
let point2 = [1.0, 1.0];
let dist = frame.distance(&point1, &point2)?;
# assert_eq!(dist, 2.0_f64.sqrt());
# Ok::<(), ast_core::error::AstError>(())
```

This calculates the distance between the origin (0,0) and a point at position (1,1).
In this case, the result, as you would expect, is √2.
However, this is only true for the Cartesian coordinate system which a basic Frame represents.
In general, `distance` will calculate the geodesic distance between the two points, so that with a more specialized Frame (such as a SkyFrame, representing the celestial sphere) a great-circle distance might be returned.

The [`offset`](crate::frames::Frame::offset) method is really the inverse of `distance`.
Given two points in a Frame, it calculates the coordinates of a third point which is offset a specified distance away from the first point along the geodesic joining it to the second one.
For example:

```rust
# use ast_core::frames::{BasicFrameBuilder, Frame};
# let frame = BasicFrameBuilder::new(2).build()?;
let point1 = [0.0, 0.0];
let point2 = [1.0, 1.0];
let point3 = frame.offset(&point1, &point2, 0.5)?;
# assert!((point3[0] - 0.5_f64.sqrt() / 2.0).abs() < 1e-15);
# Ok::<(), ast_core::error::AstError>(())
```

This would return in `point3` the coordinates of a point which is offset 0.5 units away from the origin (0,0) in the direction of the position (1,1).
Again, this is a simple result in a Cartesian Frame, as varying the offset will trace out a straight line.
On the celestial sphere, however (*e.g.* using a SkyFrame), it would trace out a great circle.

The methods [`ax_distance`](crate::frames::Frame::ax_distance) and [`ax_offset`](crate::frames::Frame::ax_offset) are similar to `distance` and `offset`, except that the curves which they use as "straight lines" are not geodesics, but curves parallel to a specified axis (for instance, a line of constant Declination is not a geodesic).
One reason for using these methods is to deal with the cyclic ambiguity of longitude and latitude axes.

The [`offset2`](crate::frames::Frame::offset2) method is similar to `offset`, but instead of using the geodesic which passes through two positions, it uses the geodesic which passes at a given position angle through the starting position.

Position angles are always measured from the positive direction of the second Frame axis to the required line, with positive angles being in the same sense as rotation from the positive direction of the second axis to the positive direction of the first Frame axis.
This definition applies to all classes of Frame, including SkyFrame.
The default ordering of axes in a SkyFrame makes the second axis equivalent to north, and so the definition of position angle given above corresponds to the normal astronomical usage, "from north, through east".
However, it should be remembered that it is possible to permute the axes of a SkyFrame (or indeed any Frame), so that north becomes axis 1.
In this case, an AST "position angle" would be the angle "from east, through north".
Always take the axis ordering into account when deriving an astronomical position angle from an AST position angle.

Within a Cartesian coordinate system, the position angle of a geodesic (*i.e.* a straight line) is constant along its entire length, but this is not necessarily true of other coordinate systems.
Within a spherical coordinate system, for instance, the position angle of a geodesic will vary along its length (except for the special cases of a meridian and the equator).
In addition to returning the required offset position, `offset2` returns the position angle of the geodesic at the offset position.
This is useful if you want to trace out a path which involves turning through specified angles.
For instance, tracing out a rectangle in which each side is a geodesic involves turning through 90 degrees at the corners.
To do this, use `offset2` to calculate the position of each corner, and then add (or subtract) 90 degrees from the position angle returned by `offset2`.

The [`angle`](crate::frames::Frame::angle) method calculates the angle subtended by two points, at a third point.
If used with a 2-dimensional Frame the returned angle is signed to indicate the sense of rotation (clockwise or anti-clockwise) in taking the "shortest route" from the first point to the second.
If the Frame has more than 2 axes, the result is un-signed and is always in the range zero to π.

The [`ax_angle`](crate::frames::Frame::ax_angle) method is similar to `angle`, but the "reference direction", from which angles are measured, is a specified axis.

The [`resolve`](crate::frames::Frame::resolve) method resolves a given displacement within a Frame into two components, parallel and perpendicular to a given reference direction.

The displacement is specified by two positions within the Frame; the starting and ending positions.
The reference direction is defined by the geodesic curve passing through the starting position and a third specified position.
The lengths of the two components are returned, together with the position on the reference geodesic which is closest to the third supplied point.

## The Domain Attribute

The Domain attribute is one of the most important properties of a Frame, although the concept it expresses can sometimes seem a little subtle.
We will introduce it here, but its true value will probably not become apparent until later (*Converting between FrameSet Coordinate Systems*<!-- link: crate::guide::frameset_ops#converting-between-frameset-coordinate-systems -->).

To understand the need for the Domain attribute, consider using different Frames to represent the following different coordinate systems associated with a CCD image:

1. A coordinate system based on pixel numbers.
2. Positions on the CCD chip, measured in μm.
3. Positions in the focal plane of the telescope, measured in mm.
4. A celestial coordinate system, measured in radians.

If we had two such CCD images, we might legitimately want to align them pixel-for-pixel (*i.e.* using the coordinate system based on pixel numbers) in order to, say, divide by a flat-field exposure.
We might similarly consider aligning them using any of the other coordinate systems so as to achieve different results.
For example, we might consider merging separate images from a CCD mosaic by using focal plane positions.

It would obviously not be legitimate, however, to directly compare positions in one image measured in pixels with positions in the other measured in mm, nor to equate chip positions in μm with sky coordinates in radians.
If we wanted to inter-compare these coordinates, we would need to do it indirectly, using other information based on the experimental set-up.
For instance, we might need to know the size of the pixels expressed in mm and the orientation of the CCD chip in the focal plane.

Note that it is not simply the difference in physical units which prevents certain coordinates from being directly inter-compared (because the appropriate unit scaling factors could be included without any additional information).
Neither is it the fact that different coordinate systems are in use (because we could legitimately inter-compare two different celestial coordinate systems without any extra information).
Instead, it is the different nature of the coordinate spaces to which these coordinate systems have been applied.

We normally express this by saying that the coordinate systems apply to different *physical domains*.
Although we may establish *ad hoc* relationships between coordinates in different physical domains, they are not intrinsically related to each other and we need to supply extra information before we can convert coordinates between them.

In AST, the role of the (character string) Domain attribute is to assign Frames to their respective physical domains.
The way it operates is as follows:

- Coordinate systems which apply to the same physical domain (*i.e.* whose Frames have the same Domain value) can be directly inter-compared.

  If the domain has several coordinate systems associated with it (*e.g.* the celestial sphere), then a coordinate conversion may be involved.
  Otherwise, coordinate values may simply be equated.
- Coordinate systems which apply to different physical domains (*i.e.* whose Frames have different Domain values) cannot be directly inter-compared.

  If any relationship does exist between such coordinate systems — and it need not — then additional information must be supplied in order to establish the relationship between them in any particular case.
  We will see later ([Coordinate System Networks](crate::guide::framesets)) how to establish such relationships between Frames in different domains.

With the basic Frames we are considering here, each physical domain only has a single (Cartesian) coordinate system associated with it, so that if two such Frames have the same Domain value, their coordinate systems will be identical and may simply be equated.
With more specialized Frames, however, more than one coordinate system may apply to each domain.
In such cases, a coordinate conversion may need to be performed.

When a basic Frame is created, its Domain attribute defaults to an empty string.
This means that all such Frames belong to the same (null) domain by default and therefore describe the same unspecified physical coordinate space.
In order to assign a Frame to a different domain, you simply need to set its Domain value.
This is normally most conveniently done when it is created, as follows:

```rust
use ast_core::frames::BasicFrameBuilder;

let frame1 = BasicFrameBuilder::new(2)
    .domain("CCD_CHIP")
    .unit(1, "micron")
    .unit(2, "micron")
    .build()?;
let frame2 = BasicFrameBuilder::new(2)
    .domain("FOCAL_PLANE")
    .unit(1, "mm")
    .unit(2, "mm")
    .build()?;
# Ok::<(), ast_core::error::AstError>(())
```

Here, we have created two Frames in different physical domains.
Although their coordinate values all have units of length, they cannot be directly inter-compared (because their axes may be rotated with respect to each other, for instance).

All Domain values are automatically converted to upper case and white space is removed, but there are no other restrictions on the names you may use to label different physical domains.
From a practical point of view, however, it is worth following a few conventions ([Conventions for Domain Names](#conventions-for-domain-names)).

## Conventions for Domain Names

When choosing a value for the Domain attribute of a Frame, it obviously makes sense to avoid generic names which might clash with those used for similar (but subtly different!) purposes by other programmers.
If you are developing software for an instrument, for example, and want to identify an instrumental coordinate system, then it is sensible to add a distinguishing prefix.
For instance, you might use `<INST>_FOCAL_PLANE`, where `<INST>` (*e.g.* an acronym) identifies your instrument.

For some purposes, however, a standard choice of Domain name is desirable so that different items of software can communicate.
For this purpose, the following Domain names are reserved by AST and the use recommended below should be carefully observed:

- **GRAPHICS**:
  Identifies the coordinate space used by an underlying computer graphics system to specify plotting operations.
  Typically, when performing graphical operations, AST is used to define additional coordinate systems which are related to these "native" graphical coordinates.
  Plotting may be carried out in any of these coordinate systems, but the GRAPHICS domain identifies the native coordinates through which AST communicates with the underlying graphics system.
- **GRID**:
  Identifies the instantaneous *data grid* used to store and handle data, together with an associated coordinate system.
  In this coordinate system, the first element stored in an array of data always has a coordinate value of unity at its center and all elements have unit extent.
  This applies to all dimensions.

  If data are copied or transformed to a new data grid (by whatever means), or a subset of the original grid is extracted, then the same rules apply to the copy or subset.
  Its first element therefore has GRID coordinate values of unity at its center.
  Note that this means that GRID coordinates remain attached to the first element of the data grid and not to its data content (*e.g.* the features in an image).
- **PIXEL**:
  Identifies an array of pixels and an associated *pixel-based* coordinate system which is related to the GRID coordinate system (above) simply by a shift of origin along each axis.
  This shift may be integral, fractional, positive, negative or zero.
  The data elements retain their unit extent along each axis.

  Because the amount of shift is unspecified, the PIXEL domain is distinct from the GRID domain.
  The relationship between them contains a degree of uncertainty, such as typically arises from the different conventions used by different software systems.
  For instance, in some software the first pixel is regarded as being centered at (1,1), while in other software it is at (0.5,0.5).
  In addition, some software packages implement a "pixel origin" which allows pixel coordinates to start at an arbitrary value.

  The GRID domain (which corresponds with the pixel-numbering convention used by FITS) is a special case of the PIXEL domain and avoids this uncertainty.
  In general, additional information is required in order to convert from one to the other.
- **SKY**:
  Identifies the domain which contains all equivalent celestial coordinate systems.
  Because these are represented in AST by SkyFrames ([Celestial Coordinate Systems](crate::guide::skyframes)), it should be no surprise that the default Domain value for a SkyFrame is SKY.
  Since there is only one sky, you probably won't need to change this very often.
- **SPECTRUM**:
  Identifies the domain used to describe positions within an electro-magnetic spectrum.
  The AST SpecFrame ([Spectral Coordinate Systems](crate::guide::specframes)) class describes positions within this domain, allowing a wide range of different coordinate systems to be used (frequency, wavelength, *etc*).
  The default Domain value for a SpecFrame is SPECTRUM.
- **TIME**:
  Identifies the domain used to describe moments in time.
  The AST TimeFrame class describes positions within this domain, allowing a wide range of different coordinate systems and timescales to be used.
  The default Domain value for a TimeFrame is TIME.

Although we have drawn a necessary distinction here between the GRID and PIXEL domains, we will continue to refer in general terms to image "pixels" and "pixel coordinates" whenever this distinction is not important.
This should not be taken to imply that the GRID convention for numbering pixels is excluded — in fact, it is usually to be preferred (at the level of data handling being discussed in this document) and we recommend it.

## The Unit Attribute

Each axis of a Frame has a Unit attribute which holds the physical units used to describe positions on the axis.

The Unit attribute may be *passive*, so that no part of the AST system uses the Unit string for any purpose and no account is taken of it when finding the Mapping between two Frames.
Thus if the conversion between a pair of 1-dimensional Frames representing velocity was found (using [`convert_to`](crate::frames::FrameSearch::convert_to)) the returned Mapping would always be a UnitMap, even if the Unit attributes of the two Frames were "km/h" and "m/s".
Alternatively, the Unit attribute may be *active*; that is, differences in the Unit attribute may be taken into account when finding the Mapping between two Frames.
Basic Frames and SkyFrames have passive Unit attributes by default, but the methods [`set_active_unit`](crate::frames::Frame::set_active_unit) and [`get_active_unit`](crate::frames::Frame::get_active_unit) allow this default behavior to be changed.
The SpecFrame and TimeFrame classes *always* have an active Unit attribute (attempts to change this are ignored).

For instance, consider the above example of two 1-dimensional Frames describing velocity.
These Frames can be created as follows:

```rust
use ast_core::frames::BasicFrameBuilder;

let mut frame1 = BasicFrameBuilder::new(1).domain("VELOCITY").unit(1, "km/h").build()?;
let mut frame2 = BasicFrameBuilder::new(1).domain("VELOCITY").unit(1, "m/s").build()?;
# Ok::<(), ast_core::error::AstError>(())
```

By default, these Frames have passive Unit attributes, and so an attempt to find a Mapping between them would ignore the difference in their Unit attributes and return a unit Mapping.
To avoid this, we indicate that we want these Frames to have *active* Unit attributes, as follows:

```rust
# use ast_core::frames::{BasicFrameBuilder, Frame};
# let mut frame1 = BasicFrameBuilder::new(1).domain("VELOCITY").unit(1, "km/h").build()?;
# let mut frame2 = BasicFrameBuilder::new(1).domain("VELOCITY").unit(1, "m/s").build()?;
frame1.set_active_unit(true);
frame2.set_active_unit(true);
# Ok::<(), ast_core::error::AstError>(())
```

If we then find the Mapping between them as follows:

```rust
use ast_core::frames::{Frame, FrameRef, FrameSearch};
# use ast_core::frames::BasicFrameBuilder;
# use ast_core::mappings::{Mapping, ZoomMap};
# let mut frame1 = BasicFrameBuilder::new(1).domain("VELOCITY").unit(1, "km/h").build()?;
# let mut frame2 = BasicFrameBuilder::new(1).domain("VELOCITY").unit(1, "m/s").build()?;
# frame2.set_active_unit(true);

let cvt = frame1.convert_to(&mut frame2, &[])?.expect("conversion exists");
# let map = cvt.mapping(FrameRef::Base, FrameRef::Current)?.simplify()?;
# let zoom = map.as_any().downcast_ref::<ZoomMap>().expect("a ZoomMap").zoom();
# assert!((zoom - 1.0 / 3.6).abs() < 1e-15);
# Ok::<(), ast_core::error::AstError>(())
```

the Mapping contained within the FrameSet returned by `convert_to` will be a one-dimensional ZoomMap which simply scales its input (a velocity in km/h) by a factor of 0.278 to create its output (a velocity in m/s).

In fact we need not have set the Unit attribute active in `frame1` since the behavior of `convert_to` is determined by its "to" Frame (the argument).

### The Syntax for Unit Strings

Conversion between units systems relies on the use of a specific syntax for the Unit attribute.
If the value of the Unit attribute does not conform to this syntax, then an error will be reported if an attempt is made to use it to determine an inter-unit Mapping (this will never happen if the Unit attribute is *passive*).

The adopted syntax is that described in FITS-WCS paper I "Representation of World Coordinate in FITS" by Greisen & Calabretta.
We distinguish here between "basic" units and "derived" units: derived units are defined in terms of other units (either derived or basic), whereas basic units have no such definitions.
Derived units may be represented by their own *symbol* (*e.g.* "Jy" — the Jansky) or by a *mathematical expression* which combines other symbols and constants to form a definition of the unit (*e.g.* "km/s" — kilometers per second).
Unit symbols may be prefixed by a string representing a standard multiple or sub-multiple.

In addition to the unit symbols listed in FITS-WCS Paper I, any other arbitrary unit symbol may be used, with the proviso that it will not be possible to convert between Frames using such units.
The exception to this is if both Frames refer to the same unknown unit string.
For instance, an axis with unknown unit symbol "flop" *could* be converted to an axis with unit "Mflop" (Mega-flop).

Unit symbols (optionally prefixed with a multiple or sub-multiple) can be combined together using a limited range of mathematical operators and functions, to produce new units.
Such expressions may also contain parentheses and numerical constants (these may optionally use "scientific" notation including an "E" character to represent the power of 10).

The following tables list the symbols for the basic and derived units which may be included in a units string, the standard prefixes for multiples and sub-multiples, and the strings which may be used to represent mathematical operators and functions.

**Basic units**

| Quantity | Symbol | Full Name |
|---|---|---|
| length | m | meter |
| mass | g | gram |
| time | s | second |
| plane angle | rad | radian |
| solid angle | sr | steradian |
| temperature | K | Kelvin |
| electric current | A | Ampere |
| amount of substance | mol | mole |
| luminous intensity | cd | candela |

**Derived units**

| Quantity | Symbol | Full Name | Definition |
|---|---|---|---|
| area | barn | barn | 1.0E-28 m\*\*2 |
| area | pix | pixel | |
| area | pixel | pixel | |
| electric capacitance | F | Farad | C/V |
| electric charge | C | Coulomb | A s |
| electric conductance | S | Siemens | A/V |
| electric potential | V | Volt | J/C |
| electric resistance | Ohm | Ohm | V/A |
| energy | J | Joule | N m |
| energy | Ry | Rydberg | 13.605692 eV |
| energy | eV | electron-Volt | 1.60217733E-19 J |
| energy | erg | erg | 1.0E-7 J |
| events | count | count | |
| events | ct | count | |
| events | ph | photon | |
| events | photon | photon | |
| flux density | Jy | Jansky | 1.0E-26 W /m\*\*2 /Hz |
| flux density | R | Rayleigh | 1.0E10/(4\*PI) photon.m\*\*-2 /s/sr |
| flux density | mag | magnitude | |
| force | N | Newton | kg m/s\*\*2 |
| frequency | Hz | Hertz | 1/s |
| illuminance | lx | lux | lm/m\*\*2 |
| inductance | H | Henry | Wb/A |
| length | AU | astronomical unit | 1.49598E11 m |
| length | Angstrom | Angstrom | 1.0E-10 m |
| length | lyr | light year | 9.460730E15 m |
| length | pc | parsec | 3.0867E16 m |
| length | solRad | solar radius | 6.9599E8 m |
| luminosity | solLum | solar luminosity | 3.8268E26 W |
| luminous flux | lm | lumen | cd sr |
| magnetic field | G | Gauss | 1.0E-4 T |
| magnetic flux | Wb | Weber | V s |
| mass | solMass | solar mass | 1.9891E30 kg |
| mass | u | unified atomic mass unit | 1.6605387E-27 kg |
| magnetic flux density | T | Tesla | Wb/m\*\*2 |
| plane angle | arcmin | arc-minute | 1/60 deg |
| plane angle | arcsec | arc-second | 1/3600 deg |
| plane angle | mas | milli-arcsecond | 1/3600000 deg |
| plane angle | deg | degree | pi/180 rad |
| power | W | Watt | J/s |
| pressure, stress | Pa | Pascal | N/m\*\*2 |
| time | a | year | 31557600 s |
| time | d | day | 86400 s |
| time | h | hour | 3600 s |
| time | yr | year | 31557600 s |
| time | min | minute | 60 s |
| | D | Debye | 1.0E-29/3 C.m |

**Prefixes for multiples & sub-multiples**

| Sub-multiple | Name | Prefix | Multiple | Name | Prefix |
|---|---|---|---|---|---|
| 10⁻¹ | deci | d | 10 | deca | da |
| 10⁻² | centi | c | 10² | hecto | h |
| 10⁻³ | milli | m | 10³ | kilo | k |
| 10⁻⁶ | micro | u | 10⁶ | mega | M |
| 10⁻⁹ | nano | n | 10⁹ | giga | G |
| 10⁻¹² | pico | p | 10¹² | tera | T |
| 10⁻¹⁵ | femto | f | 10¹⁵ | peta | P |
| 10⁻¹⁸ | atto | a | 10¹⁸ | exa | E |
| 10⁻²¹ | zepto | z | 10²¹ | zetta | Z |
| 10⁻²⁴ | yocto | y | 10²⁴ | yotta | Y |

**Mathematical operators & functions**

| String | Meaning |
|---|---|
| `sym1 sym2` | multiplication (a space) |
| `sym1*sym2` | multiplication (an asterisk) |
| `sym1.sym2` | multiplication (a dot) |
| `sym1/sym2` | division |
| `sym1**y` | exponentiation (y must be a numerical constant) |
| `sym1^y` | exponentiation (y must be a numerical constant) |
| `log(sym1)` | common logarithm |
| `ln(sym1)` | natural logarithm |
| `exp(sym1)` | exponential |
| `sqrt(sym1)` | square root |

### Side-effects of Changing the Unit attribute

If an Axis has an active Unit attribute, changing its value (either by setting a new value or by clearing it so that the default value is re-instated) may cause the Label and Symbol attributes to be changed accordingly.
For instance, if an Axis has Unit, Label and Symbol of "Hz", "Frequency" and "nu", then changing its Unit attribute to "log(Hz)" will cause AST to change its Label and Symbol to "log( Frequency )" and "log( nu )":

```rust
use ast_core::frames::{BasicFrameBuilder, Frame};

let mut frame = BasicFrameBuilder::new(1)
    .unit(1, "Hz")
    .label(1, "Frequency")
    .symbol(1, "nu")
    .build()?;
frame.set_active_unit(true);
frame.set_unit(1, "log(Hz)")?;
assert_eq!(frame.label(1)?, "log( Frequency )");
assert_eq!(frame.symbol(1)?, "log( nu )");
# Ok::<(), ast_core::error::AstError>(())
```

These changes are only made if the Unit attribute is active, and a Mapping can be found from the old units to the new units.
On the other hand, changing the Unit from "Hz" to "MHz" would not cause any change to the Label or Symbol attributes.
