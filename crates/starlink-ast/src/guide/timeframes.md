# Time Systems (TimeFrames)

The [`TimeFrame`](crate::frames::TimeFrame) is a Frame which is specialized for representing moments in time.
In this section we examine the additional properties and behavior of a TimeFrame that distinguish it from a basic Frame ([Representing Coordinate Systems](crate::guide::frames)).

## The TimeFrame Model

As for a SkyFrame, a TimeFrame is a Frame ([Representing Coordinate Systems](crate::guide::frames)), so it has all the properties and behavior of a basic Frame.

A TimeFrame represents a wide range of different 1-dimensional coordinate systems which can be used to describe moments in time.
Absolute times and relative (i.e. elapsed) times are supported (attribute TimeOrigin), as are a range of different time scales (attribute TimeScale).
An absolute or relative value in any time scale can be represented in different forms such as Modified Julian Date, Julian Epoch, *etc* (attribute System).
AST extends the definition of these systems to allow them to be used with any unit of time (attribute Unit).
The TimeFrame class also allows times to be formatted as either a simple floating point value or as a Gregorian date and time of day (attribute Format).

## Creating a TimeFrame

A TimeFrame with default attributes is created as follows:

```rust
use starlink_ast::frames::TimeFrame;

let timeframe = TimeFrame::unset_system();
# use starlink_ast::frames::{Frame, TimeScale, TimeSystem};
# assert_eq!((timeframe.system(), timeframe.timescale()), (TimeSystem::Mjd, TimeScale::Tai));
# assert_eq!(timeframe.unit(1)?, "d");
# Ok::<(), starlink_ast::error::AstError>(())
```

Such a TimeFrame would represent the default coordinate system which is Modified Julian Date (with the usual units of days) in the International Atomic Time (TAI) time scale.

## Specifying a Particular Time System

By setting the System attribute appropriately, the TimeFrame can represent Julian Date, Modified Julian Date, Julian Epoch or Besselian Epoch (the time scale is specified by a separate attribute called TimeScale).

Selection of a particular coordinate system is performed simply by setting a value for the TimeFrame's System attribute, a [`TimeSystem`](crate::frames::TimeSystem).
This setting is most conveniently done when the TimeFrame is created.
For example, a TimeFrame representing Julian Epoch would be created by:

```rust
use starlink_ast::frames::{TimeFrame, TimeSystem};

let timeframe = TimeFrame::new(TimeSystem::Jepoch);
# use starlink_ast::frames::Frame;
# assert_eq!(timeframe.unit(1)?, "yr");
# Ok::<(), starlink_ast::error::AstError>(())
```

Note that specifying Julian Epoch also changes the associated default Unit (from days to years).
This is because the default value of the TimeFrame's Unit attribute depends on the System attribute setting.

You may change the System value at any time with [`set_system`](crate::frames::TimeFrame::set_system), although this is not usually needed.
The values supported are set out in the description of [`TimeSystem`](crate::frames::TimeSystem).

## Attributes which Qualify Time Coordinate Systems

Time coordinate systems require some additional free parameters to identify a particular coordinate system from amongst a broader class of related coordinate systems.
For example, all TimeFrames are qualified by the time scale (that is, the physical process used to define the flow of time), and some require the position of the observer's clock.

In AST, these free parameters are represented by additional TimeFrame attributes, each of which has a default appropriate to (*i.e.* defined by) the setting of the main System attribute.
Each of these *qualifying attributes* may, however, be assigned an explicit value so as to select a particular coordinate system.
Note, it is usually best to assign explicit values whenever possible rather than relying on defaults.
Attributes should only be left at their default value if you "don't care" what value is used.
In certain circumstances (particularly, when aligning two Frames), a default value for an attribute may be replaced by the value from another similar Frame.
Such value replacement can be prevented by assigning an explicit value to the attribute, rather than simply relying on the default.

The main TimeFrame attributes which qualify the System attribute are:

- **TimeScale**:
  This specifies the time scale ([`timescale`](crate::frames::TimeFrame::timescale), a [`TimeScale`](crate::frames::TimeScale)).
- **LTOffset**:
  This specifies the offset from Local Time to UTC in hours (time zones east of Greenwich have positive values) ([`lt_offset`](crate::frames::TimeFrame::lt_offset)).
  Note, AST uses the value as supplied without making any correction for daylight saving.
- **TimeOrigin**:
  This specifies the zero point from which time values are measured, within the system specified by the System attribute ([`time_origin`](crate::frames::TimeFrame::time_origin)).
  Thus, a value of zero (the default) indicates that time values represent absolute times.
  Non-zero values may be used to indicate that the TimeFrame represents elapsed time since the specified origin.

For further details of these attributes you should consult their descriptions, and for details of the System settings for which they are relevant, see the description of [`TimeSystem`](crate::frames::TimeSystem).

Note that it does no harm to assign values to qualifying attributes which are not relevant to the main System or TimeScale value.
Any such values are stored, but are not used unless the System and/or TimeScale value is later set so that they become relevant.
