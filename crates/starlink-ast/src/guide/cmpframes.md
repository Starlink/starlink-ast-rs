# Compound Frames (CmpFrames)

We now turn to a rather special form of Frame, the CmpFrame.
The Frames we have considered so far have been atomic, in the sense that they represent pre-defined elementary physical domains.
A [`CmpFrame`](crate::frames::CmpFrame), however, is a compound Frame.
In essence, it is a structure for containing other Frames and its purpose is to allow those Frames to work together in various combinations while appearing as a single Object.
A CmpFrame's behavior is therefore not pre-defined, but is determined by the other Frames it contains (its "component" Frames).

As with compound Mappings, compound Frames can be nested within each other, forming arbitrarily complex Frames.

## Creating a CmpFrame

A very common use for a CmpFrame within astronomy is to represent a "spectral cube".
This is a 3-dimensional Frame in which one of the axes represents position within a spectrum, and the other two axes represent position on the sky (or some other spatial domain such as the focal plane of a telescope).
As an example, we create such a CmpFrame in which axes 1 and 2 represent Right Ascension and Declination (ICRS), and axis 3 represents wavelength (these are the default coordinate Systems represented by a SkyFrame and a SpecFrame respectively):

```rust
use starlink_ast::frames::{CmpFrame, SkyFrame, SpecFrame};

let skyframe = SkyFrame::unset_system();
let specframe = SpecFrame::unset_system();
let cmpframe = CmpFrame::new(Box::new(skyframe), Box::new(specframe));
# use starlink_ast::frames::Frame;
# assert_eq!(cmpframe.label(3)?, "Wavelength");
# Ok::<(), starlink_ast::error::AstError>(())
```

The CmpFrame takes ownership of its two component Frames.

If it was desired to make RA and Dec correspond to axes 1 and 3, with axis 2 being the spectral axis, then the axes of the CmpFrame created above would need to be permuted as follows:

```rust
use starlink_ast::frames::Frame;
# use starlink_ast::frames::{CmpFrame, SkyFrame, SpecFrame};
# let mut cmpframe = CmpFrame::new(Box::new(SkyFrame::unset_system()), Box::new(SpecFrame::unset_system()));

let perm = [1, 3, 2];
cmpframe.perm_axes(&perm)?;
# assert_eq!(cmpframe.label(2)?, "Wavelength");
# assert_eq!(cmpframe.label(3)?, "Declination");
# Ok::<(), starlink_ast::error::AstError>(())
```

## The Attributes of a CmpFrame

A CmpFrame *is a* Frame and so has all the attributes of a Frame.
The default value for the Domain attribute for a CmpFrame is formed by concatenating the Domains of the two component Frames, separated by a minus sign ("-").
(If both component Frames have blank Domains, then the default Domain for the CmpFrame is the string "CMP".)
The (fixed) value for its System attribute is "Compound" (any attempt to change the System value of a CmpFrame is ignored).
A CmpFrame has no further attributes over and above those common to all Frames.
However, attributes of the two component Frames can be accessed, as described below.

Frame attributes which are specific to individual axes (such as the Label of axis 2, the Format of axis 1, *etc*) simply mirror the corresponding axes of the relevant component Frame.
That is, if the Label of axis 2 of a CmpFrame is accessed, the CmpFrame will forward the access request to the component Frame which contains axis 2.
Thus, default values for axis attributes will be the same as those provided by the component Frames.

```rust
use starlink_ast::frames::{CmpFrame, Frame, SkyFrame, SpecFrame};

let cmpframe = CmpFrame::new(Box::new(SkyFrame::unset_system()), Box::new(SpecFrame::unset_system()));
assert_eq!(cmpframe.domain(), "SKY-SPECTRUM");
assert_eq!(cmpframe.label(2)?, "Declination");
# Ok::<(), starlink_ast::error::AstError>(())
```

The attributes of a component Frame, including those which are not held by other classes of Frame, are reached through the component itself: [`frame1`](crate::frames::CmpFrame::frame1) and [`frame2`](crate::frames::CmpFrame::frame2) give the two components, and [`frame1_mut`](crate::frames::CmpFrame::frame1_mut) and [`frame2_mut`](crate::frames::CmpFrame::frame2_mut) let them be changed.
For instance, the SkyFrame class defines an attribute called Equinox which is not held by other classes of Frames.
To set a value for the Equinox attribute of the SkyFrame contained within a CmpFrame which holds a SpecFrame and a SkyFrame in that order, change the second component:

```rust
use starlink_ast::frames::{CmpFrame, SkyFrame, SkySystem, SpecFrame};

let mut cmpframe = CmpFrame::new(
    Box::new(SpecFrame::unset_system()),
    Box::new(SkyFrame::new(SkySystem::Fk5)),
);
let skyframe = cmpframe
    .frame2_mut()
    .as_any_mut()
    .downcast_mut::<SkyFrame>()
    .expect("the second component is a SkyFrame");
skyframe.set_equinox_year(2010.0);
# let sky = cmpframe.frame2().as_any().downcast_ref::<SkyFrame>().unwrap();
# assert!((sky.equinox_year() - 2010.0).abs() < 1e-9);
```

Frame attributes that are not specific to an axis, such as the Epoch and the observer's position, are shared with the components: setting or clearing one on the CmpFrame sets or clears it in both components.
