# An Introduction to Coordinate System Conversions

In this section, we start to look at techniques for converting between different coordinate systems.
At this stage, the tools we have available are Frames ([Representing Coordinate Systems](crate::guide::frames)), SkyFrames ([Celestial Coordinate Systems](crate::guide::skyframes)), SpecFrames ([Spectral Coordinate Systems](crate::guide::specframes)), TimeFrames ([Time Systems](crate::guide::timeframes)) and various Mappings ([Inter-Relating Coordinate Systems](crate::guide::mappings)).
These are sufficient to allow us to begin examining the problem, but more sophisticated approaches will also emerge later ([Converting between FrameSet Coordinate Systems](crate::guide::frameset_ops#converting-between-frameset-coordinate-systems)).

## Converting between Celestial Coordinate Systems

We begin by examining how to convert between two celestial coordinate systems represented by SkyFrames, as this is both an illuminating and practical example.
Consider the problem of converting celestial coordinates between:

1. The old FK4 system, with no E terms, a Besselian epoch of 1958.0 and a Besselian equinox of 1960.0.
2. An ecliptic coordinate system based on the mean equinox and ecliptic of Julian epoch 2010.5.

This example is arbitrary but not completely unrealistic.
Unless you already have expertise with such conversions, you are unlikely to find it straightforward.

Using AST, we begin by creating two SkyFrames to represent these coordinate systems, as follows:

```rust
use ast_core::frames::{SkyFrame, SkySystem};

let mut skyframe1 = SkyFrame::new(SkySystem::Fk4NoE);
skyframe1.set_epoch_year(1958.0);
skyframe1.set_equinox_year(1960.0);
let mut skyframe2 = SkyFrame::new(SkySystem::Ecliptic);
skyframe2.set_equinox_year(2010.5);
```

Note how specifying the coordinate systems consists simply of setting the attributes of each SkyFrame appropriately.
The next step is to find a way of converting between these SkyFrames.
This is done using [`convert_to`](crate::frames::FrameSearch::convert_to), as follows:

```rust
use ast_core::frames::FrameSearch;
# use ast_core::frames::{SkyFrame, SkySystem};
# let mut skyframe1 = SkyFrame::new(SkySystem::Fk4NoE);
# let mut skyframe2 = SkyFrame::new(SkySystem::Ecliptic);

match skyframe1.convert_to(&mut skyframe2, &[])? {
    None => {
        // <conversion is not possible>
#       unreachable!();
    }
    Some(cvt) => {
        // <conversion is possible>
    }
}
# Ok::<(), ast_core::error::AstError>(())
```

The second argument of `convert_to` lists the Domains to be searched for a conversion, and is not needed here, so it is empty.

`convert_to` will return `None` if conversion is not possible.
In this example, conversion is possible, so it will return a new Object that describes the conversion.

The Object returned is called a FrameSet.
We have not discussed FrameSets yet ([Coordinate System Networks](crate::guide::framesets)), but for the present purposes we can consider them simply as Objects that can behave both as Mappings and as Frames.
It is the FrameSet's behavior as a Mapping in which we are mainly interested here, because the Mapping it implements is the one we require — *i.e.* it converts between the two celestial coordinate systems ([Creating FrameSets by Conversion](crate::guide::frameset_ops#creating-framesets-by-conversion)).

For example, if `alpha1` and `delta1` are two arrays containing the longitude and latitude, in radians, of points on the sky in the original coordinate system (corresponding to `skyframe1`), then they could be converted into the new coordinate system (represented by `skyframe2`) as follows:

```rust
use ast_core::mappings::Mapping;
# use ast_core::frames::{Frame, FrameSearch, SkyFrame, SkySystem};
# let mut skyframe1 = SkyFrame::new(SkySystem::Fk4NoE);
# skyframe1.set_epoch_year(1958.0);
# skyframe1.set_equinox_year(1960.0);
# let mut skyframe2 = SkyFrame::new(SkySystem::Ecliptic);
# skyframe2.set_equinox_year(2010.5);
# let cvt = skyframe1.convert_to(&mut skyframe2, &[])?.unwrap();
# let alpha1 = [skyframe1.unformat(1, "2:06:03.0")?.0];
# let delta1 = [skyframe1.unformat(2, "34:22:39")?.0];

let (alpha2, delta2) = cvt.tran2(&alpha1, &delta1, true)?;
# assert!((alpha2[0].to_degrees() - 42.10857136588773).abs() < 1e-9);
# assert!((delta2[0].to_degrees() - 20.271822012235653).abs() < 1e-9);
# Ok::<(), ast_core::error::AstError>(())
```

The new coordinates are returned in `alpha2` and `delta2`.
To transform coordinates in the opposite direction, we simply invert the third (boolean) argument to [`tran2`](crate::mappings::Mapping::tran2), as follows:

```rust
# use ast_core::mappings::Mapping;
# use ast_core::frames::{FrameSearch, SkyFrame, SkySystem};
# let mut skyframe1 = SkyFrame::new(SkySystem::Fk4NoE);
# let mut skyframe2 = SkyFrame::new(SkySystem::Ecliptic);
# let cvt = skyframe1.convert_to(&mut skyframe2, &[])?.unwrap();
# let (alpha2, delta2) = ([0.7], [0.35]);
let (alpha1, delta1) = cvt.tran2(&alpha2, &delta2, false)?;
# Ok::<(), ast_core::error::AstError>(())
```

The FrameSet returned by `convert_to` also contains information about the SkyFrames used in the conversion.
As we mentioned above, a FrameSet may be used as a Frame and in this case it behaves like the "destination" Frame used in the conversion (*i.e.* like `skyframe2`).
We could therefore use the `cvt` FrameSet to calculate the distance between two points (with coordinates in radians) in the destination coordinate system, using [`distance`](crate::frames::Frame::distance):

```rust
use ast_core::frames::Frame;
# use ast_core::frames::{FrameSearch, SkyFrame, SkySystem};
# let mut skyframe1 = SkyFrame::new(SkySystem::Fk4NoE);
# let mut skyframe2 = SkyFrame::new(SkySystem::Ecliptic);
# let cvt = skyframe1.convert_to(&mut skyframe2, &[])?.unwrap();
# let (point1, point2) = ([0.7, 0.35], [0.9, 0.5]);

let distance = cvt.distance(&point1, &point2)?;
# assert_eq!(distance, skyframe2.distance(&point1, &point2)?);
# Ok::<(), ast_core::error::AstError>(())
```

and the result would be the same as if the `skyframe2` SkyFrame had been used.

Another way to see how the FrameSet produced by `convert_to` retains information about the coordinate systems involved is to set its Report attribute (shared by every Mapping) so that it displays the coordinates before and after conversion ([Transforming Coordinates](crate::guide::objects#transforming-coordinates)):

```rust
# use ast_core::mappings::Mapping;
# use ast_core::frames::{FrameSearch, SkyFrame, SkySystem};
# let mut skyframe1 = SkyFrame::new(SkySystem::Fk4NoE);
# let mut skyframe2 = SkyFrame::new(SkySystem::Ecliptic);
# let mut cvt = skyframe1.convert_to(&mut skyframe2, &[])?.unwrap();
# let (alpha1, delta1) = ([0.55], [0.6]);
cvt.set_report(true);
let (alpha2, delta2) = cvt.tran2(&alpha1, &delta1, true)?;
# Ok::<(), ast_core::error::AstError>(())
```

The output from this might look like the following:

```text
(2:06:03.0, 34:22:39) --> (42.1086, 20.2718)
(2:08:20.6, 35:31:24) --> (43.0198, 21.1703)
(2:10:38.1, 36:40:09) --> (43.9296, 22.0715)
(2:12:55.6, 37:48:55) --> (44.8383, 22.9753)
(2:15:13.1, 38:57:40) --> (45.7460, 23.8814)
(2:17:30.6, 40:06:25) --> (46.6528, 24.7900)
(2:19:48.1, 41:15:11) --> (47.5589, 25.7013)
(2:22:05.6, 42:23:56) --> (48.4644, 26.6150)
(2:24:23.1, 43:32:41) --> (49.3694, 27.5311)
(2:26:40.6, 44:41:27) --> (50.2741, 28.4500)
```

Here, we see that the input FK4 equatorial coordinate values (given in radians) have been formatted automatically in sexagesimal notation using the conventional hours for right ascension and degrees for declination.
Conversely, the output ecliptic coordinates are shown in decimal degrees, as is conventional for ecliptic coordinates.
Both are displayed using the default precision of 7 digits (the leading digit is zero and is therefore not seen in this particular example).

In fact, the `cvt` FrameSet has access to all the information in the original SkyFrames which were passed to `convert_to`.
If you had set a new Digits attribute value for either of these, the formatting above would reflect the different precision you requested by displaying a greater or smaller number of digits.

## Converting between Spectral Coordinate Systems

The principles described in the previous section for converting between celestial coordinate systems also apply to the task of converting between spectral coordinate systems.
As an example, let's look at how we might convert between frequency measured in GHz as measured in the rest frame of the telescope, and radio velocity measured in km/s measured with respect to the kinematic Local Standard of Rest.

First we create a SpecFrame, and then set its attributes to describe the required radio velocity system.
We then take a copy of this SpecFrame, and change the attribute values so that the copy describes the original frequency system (modifying a copy, rather than creating a new SpecFrame from scratch, avoids the need to specify the epoch, reference position, *etc* a second time since they are all held by the copy):

```rust
use ast_core::frames::{Frame, SkyFrame, SkySystem, SpecFrame, SpecSystem, StdOfRest};

// Read sexagesimal angles with a SkyFrame.
let fk5 = SkyFrame::new(SkySystem::Fk5);
let hours = |text| fk5.unformat(1, text).map(|(value, _)| value);
let degrees = |text| fk5.unformat(2, text).map(|(value, _)| value);

let mut specframe2 = SpecFrame::new(SpecSystem::Vradio);
specframe2.set_unit(1, "km/s")?;
// 1996-Oct-2 12:13:56.985 TDB as a Modified Julian Date.
specframe2.set_epoch_mjd(50358.0 + (12.0 * 3600.0 + 13.0 * 60.0 + 56.985) / 86400.0);
// West longitudes are negative.
specframe2.set_obs_lon(-degrees("155:28:18")?);
specframe2.set_obs_lat(degrees("19:49:34")?);
specframe2.set_ref_ra(hours("18:14:50.6")?);
specframe2.set_ref_dec(degrees("-4:40:49")?);
specframe2.set_rest_freq(230.538e9);
specframe2.set_std_of_rest(StdOfRest::Lsrk)?;

let mut specframe1 = specframe2.clone();
specframe1.set_system(SpecSystem::Freq)?;
specframe1.set_unit(1, "GHz")?;
specframe1.set_std_of_rest(StdOfRest::Topocentric)?;
# Ok::<(), ast_core::error::AstError>(())
```

The other attributes are: the time of observation (Epoch), the geographical position of the telescope (ObsLat & ObsLon), the position of the source on the sky (RefRA & RefDec), the rest frequency (RestFreq) and the standard of rest (StdOfRest).

The next step is to find a way of converting between these SpecFrames.
We use exactly the same code that we did in the previous section where we were converting between celestial coordinate systems:

```rust
# use ast_core::frames::{Frame, SkyFrame, SkySystem, SpecFrame, SpecSystem, StdOfRest};
use ast_core::frames::FrameSearch;
# let mut specframe2 = SpecFrame::new(SpecSystem::Vradio);
# specframe2.set_unit(1, "km/s")?;
# let mut specframe1 = specframe2.clone();
# specframe1.set_system(SpecSystem::Freq)?;
# specframe1.set_unit(1, "GHz")?;

match specframe1.convert_to(&mut specframe2, &[])? {
    None => {
        // <conversion is not possible>
#       unreachable!();
    }
    Some(cvt) => {
        // <conversion is possible>
    }
}
# Ok::<(), ast_core::error::AstError>(())
```

As before, this will give us a FrameSet (assuming conversion is possible, which should always be the case for our example), and we can use the FrameSet to convert between the two spectral coordinate systems.
We use [`tran1`](crate::mappings::Mapping::tran1) in place of `tran2` since a SpecFrame has only one axis (unlike a SkyFrame which has two).

For example, if `frq` is an array containing the observed frequency, in GHz, of spectral channels (described by `specframe1`), then they could be converted into the new coordinate system (represented by `specframe2`) as follows:

```rust
# use ast_core::frames::{Frame, FrameSearch, SkyFrame, SkySystem, SpecFrame, SpecSystem, StdOfRest};
use ast_core::mappings::Mapping;
# let fk5 = SkyFrame::new(SkySystem::Fk5);
# let hours = |text| fk5.unformat(1, text).map(|(value, _)| value);
# let degrees = |text| fk5.unformat(2, text).map(|(value, _)| value);
# let mut specframe2 = SpecFrame::new(SpecSystem::Vradio);
# specframe2.set_unit(1, "km/s")?;
# specframe2.set_epoch_mjd(50358.0 + (12.0 * 3600.0 + 13.0 * 60.0 + 56.985) / 86400.0);
# specframe2.set_obs_lon(-degrees("155:28:18")?);
# specframe2.set_obs_lat(degrees("19:49:34")?);
# specframe2.set_ref_ra(hours("18:14:50.6")?);
# specframe2.set_ref_dec(degrees("-4:40:49")?);
# specframe2.set_rest_freq(230.538e9);
# specframe2.set_std_of_rest(StdOfRest::Lsrk)?;
# let mut specframe1 = specframe2.clone();
# specframe1.set_system(SpecSystem::Freq)?;
# specframe1.set_unit(1, "GHz")?;
# specframe1.set_std_of_rest(StdOfRest::Topocentric)?;
# let cvt = specframe1.convert_to(&mut specframe2, &[])?.unwrap();
let frq = [230.5, 230.54];
let vel = cvt.tran1(&frq, true)?;
# assert_eq!(vel, [37.493068369905266, -14.525146802639203]);
# Ok::<(), ast_core::error::AstError>(())
```

The radio velocity values are returned in `vel`.

## Converting between Time Coordinate Systems

All the principles outlined in the previous section about aligning spectral coordinate systems (SpecFrames) can be applied directly to the problem of aligning time coordinate systems (TimeFrames).

## Handling SkyFrame Axis Permutations

We can illustrate an important point if we swap the axis order of either SkyFrame in the example above ([Converting between Celestial Coordinate Systems](#converting-between-celestial-coordinate-systems)) before identifying the conversion.
Let's assume we use [`perm_axes`](crate::frames::Frame::perm_axes) ([Permuting Frame Axes](crate::guide::frames#permuting-frame-axes)) to do this to the second SkyFrame, before applying `convert_to`, as follows:

```rust
use ast_core::frames::{Frame, FrameSearch};
# use ast_core::frames::{SkyFrame, SkySystem};
# let mut skyframe1 = SkyFrame::new(SkySystem::Fk4NoE);
# let mut skyframe2 = SkyFrame::new(SkySystem::Ecliptic);

let perm = [2, 1];
skyframe2.perm_axes(&perm)?;
let cvt = skyframe1.convert_to(&mut skyframe2, &[])?;
# Ok::<(), ast_core::error::AstError>(())
```

Now, the destination SkyFrame system no longer represents the coordinate system:

> (ecliptic longitude, ecliptic latitude)

but instead represents the transposed system:

> (ecliptic latitude, ecliptic longitude)

As a consequence, when we use the FrameSet returned by `convert_to` to apply a coordinate transformation, we obtain something like the following:

```text
(2:06:03.0, 34:22:39) --> (20.2718, 42.1086)
(2:08:20.6, 35:31:24) --> (21.1703, 43.0198)
(2:10:38.1, 36:40:09) --> (22.0715, 43.9296)
(2:12:55.6, 37:48:55) --> (22.9753, 44.8383)
(2:15:13.1, 38:57:40) --> (23.8814, 45.7460)
(2:17:30.6, 40:06:25) --> (24.7900, 46.6528)
(2:19:48.1, 41:15:11) --> (25.7013, 47.5589)
(2:22:05.6, 42:23:56) --> (26.6150, 48.4644)
(2:24:23.1, 43:32:41) --> (27.5311, 49.3694)
(2:26:40.6, 44:41:27) --> (28.4500, 50.2741)
```

When compared to the original ([Converting between Celestial Coordinate Systems](#converting-between-celestial-coordinate-systems)), the output coordinate order has been swapped to compensate for the different destination SkyFrame axis order.

In all, there are four possible axis combinations, corresponding to two possible axis orders for each of the source and destination SkyFrames, and `convert_to` will convert correctly between any of these.

The point to note is that a SkyFrame contains knowledge about how to convert to and from other SkyFrames.
Since its two axes (longitude and latitude) are distinguishable, the conversion is able to take account of the axis order.

If you need to identify the axes of a SkyFrame explicitly, taking into account any axis permutations, the LatAxis and LonAxis attributes ([`lat_axis`](crate::frames::SkyFrame::lat_axis) and [`lon_axis`](crate::frames::SkyFrame::lon_axis)) can be used.
These are read-only attributes which give the indices of the latitude and longitude axes respectively.

## Converting Between Frames

Having seen how clever SkyFrames are ([Converting between Celestial Coordinate Systems](#converting-between-celestial-coordinate-systems) and [Handling SkyFrame Axis Permutations](#handling-skyframe-axis-permutations)), we will next examine how dumb a basic Frame can be in comparison.
For example, if we create two 2-dimensional Frames and use `convert_to` to derive a conversion between them, as follows:

```rust
use ast_core::frames::{BasicFrameBuilder, FrameSearch};
# use ast_core::mappings::Mapping;

let mut frame1 = BasicFrameBuilder::new(2).build()?;
let mut frame2 = BasicFrameBuilder::new(2).build()?;
let cvt = frame1.convert_to(&mut frame2, &[])?.expect("conversion exists");
# assert_eq!(cvt.tran2(&[1.0, 2.0], &[2.0, 4.0], true)?, (vec![1.0, 2.0], vec![2.0, 4.0]));
# Ok::<(), ast_core::error::AstError>(())
```

then the coordinate transformation which the `cvt` FrameSet performs will be as follows:

```text
(1, 2) --> (1, 2)
(2, 4) --> (2, 4)
(3, 6) --> (3, 6)
(4, 8) --> (4, 8)
(5, 10) --> (5, 10)
```

This is an identity transformation, exactly the same as a UnitMap ([Example — the UnitMap](crate::guide::mappings#example--the-unitmap)).
Even if we permute the axis order of our Frames, as we did above ([Handling SkyFrame Axis Permutations](#handling-skyframe-axis-permutations)), we will fare no better.
The conversion between our two basic Frames will always be an identity transformation.

The reason for this is that, unlike a SkyFrame, all basic Frames start life the same and have axes that are indistinguishable.
Therefore, permuting their axes doesn't make them look any different — they still represent the same coordinate system.

## The Choice of Alignment System

In practice, when AST is asked to find a conversion between two Frames describing two different coordinate systems on a given physical domain, it uses an intermediate "alignment" system.
Thus, when finding a conversion from system A to system B, AST first finds the Mapping from system A to some alignment system, system C, and then finds the Mapping from this system C to the required system B.
It finally concatenates these two Mappings to get the Mapping from system A to system B.

One advantage of this is that it cuts down the number of conversion algorithms required.
If there are N different Systems which may be used to describe positions within the Domain, then this approach requires about 2N conversion algorithms to be written.
The alternative approach of going directly from system A to system B would require about N×N conversion algorithms.

In addition, the use of an intermediate alignment system highlights the nature of the conversion process.
What do we mean by saying that a Mapping "converts a position in one coordinate system into the corresponding position in another"?
In practice, it means that the input and output coordinates correspond to the same coordinates *in some third coordinate system*.
The choice of this third coordinate system, the "alignment" system, can completely alter the nature of the Mapping.
Each kind of Frame has an attribute called AlignSystem which can be used to specify the alignment system, such as [`set_align_system`](crate::frames::SpecFrame::set_align_system) for a SpecFrame.

As an example, consider the case of aligning two spectra calibrated in radio velocity, but each with a different rest frequency (each spectrum will be described by a SpecFrame).
Since the rest frequencies differ, a given velocity will correspond to different frequencies in the two spectra.
So when we come to "align" these two spectra (that is, find a Mapping which converts positions in one SpecFrame to the corresponding positions in the other), we have the choice of aligning the frequencies or aligning the velocities.
Different Mappings will be required to describe these two forms of alignment.
If we set AlignSystem to frequency then the returned Mapping will align the frequencies described by the two SpecFrames.
On the other hand, if we set AlignSystem to radio velocity then the returned Mapping will align the velocities.

Some choices of alignment system are redundant.
For instance, in the above example, changing the alignment system from frequency to wavelength has no effect on the returned Mapping: if two spectra are aligned in frequency they will also be aligned in wavelength (assuming the speed of light doesn't change).

The default value for AlignSystem depends on the class of Frame.
For a SpecFrame, the default is wavelength (or equivalently, frequency) since this is the system in which observations are usually made.
The SpecFrame class also has an attribute called AlignStdOfRest ([`set_align_std_of_rest`](crate::frames::SpecFrame::set_align_std_of_rest)) which allows the standard of rest of the alignment system to be specified.
Similarly, the TimeFrame class has an attribute called AlignTimeScale ([`set_align_timescale`](crate::frames::TimeFrame::set_align_timescale)) which allows the time scale of the alignment system to be specified.
Currently, the SkyFrame uses ICRS as the default for AlignSystem, since this is a close approximation to an inertial frame of rest.
