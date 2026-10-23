# Creating Your Own Private Mappings (IntraMaps)

## The Need for Extensibility

However many Mapping classes are provided by AST, sooner or later you will want to transform coordinates in some way that has not been foreseen.
You might want to work in some novel curvilinear coordinate system (perhaps you already have a WCS system in your software and just want to use AST for some of its other capabilities).
Alternatively, you might need to calibrate a complex dataset (like an objective prism plate) where each position must be converted to world coordinates with reference to calibration data under the control of an elaborate algorithm.

In such cases, it is clear that the basic pre-formed components provided by AST for building Mappings are just not enough.
What you need is access to a programming language.
However, if you write your own software to transform coordinate values, then it must be made available in the form of an AST class (from which you can create Objects) before it can be used in conjunction with other AST facilities.

At this point you might consider writing your own AST class, but this is not recommended.
Not only would the internal conventions used by AST take some time to master, but you might also find yourself having to change your software whenever a new version of AST was released.
Fortunately, there is a much easier route provided by the IntraMap class.

## The IntraMap Model

To allow you to write your own Mappings, AST provides a special kind of Mapping called an [`IntraMap`](crate::mappings::IntraMap).
An IntraMap is a sort of "wrapper" for a coordinate transformation written in Rust.
You write this transformation yourself, as a type that implements the [`IntraTransform`](crate::mappings::IntraTransform) trait.
This, in effect, creates a new class from which you can create Mappings (*i.e.* IntraMaps) which will transform coordinates in whatever way your transformation specifies.

Because IntraMaps are Mappings, they may be used in the same way as any other Mapping.
For instance, they may be combined in series or parallel with other Mappings using a CmpMap ([Compound Mappings](crate::guide::cmpmaps)), they may be inverted ([Inverting Mappings](crate::guide::mappings#inverting-mappings)), you may inquire about their attributes ([Getting Attribute Values](crate::guide::objects#getting-attribute-values)), they may be inserted into FrameSets ([Coordinate System Networks](crate::guide::framesets)), *etc.*
They do, however, have some important limitations of which you should be aware before we go on to consider how to create them.

## Limitations of IntraMaps

By now, you might be wondering why any other kind of Mapping is required at all.
After all, why not simply write your own coordinate transformations, wrap them up in IntraMaps and do away with all the other Mapping classes in AST?

The reason is not too hard to find.
Any transformation you write is created solely by you, so it is a private extension which does not form a permanent part of AST.
If you use it to calibrate some data and then pass that data to someone else, who has only the standard version of AST, then they will not be able to interpret it.

Thus, while an IntraMap is fine for use by you and your collaborators (who we assume have access to the same transformations), it does not address the need for universal data exchange like other AST Mappings do.
This is where the "Intra" in the class name "IntraMap" comes from, implying private or internal usage.

For this reason, it is unwise to store IntraMaps in datasets, unless they will be used solely for communication between collaborating items of software which share conventions about their use.
A private database describing coordinate systems on a graphics device might be an example where IntraMaps would be suitable, because the data would probably never be accessed by anyone else's software.
Restricting IntraMap usage to within a single program (*i.e.* never writing it out) is, of course, completely safe.

If, by accident, an IntraMap should happen to escape as part of a dataset, then the unsuspecting recipient is likely to receive an error when they attempt to read the data.
However, AST will associate details of the IntraMap's transformation and its author (if provided) with the data, so that the recipient can make an intelligent inquiry to obtain the necessary software if this proves essential.

## Writing a Transformation

The first stage in creating an IntraMap is to write the coordinate transformation.
This is a type implementing [`IntraTransform`](crate::mappings::IntraTransform), whose [`transform`](crate::mappings::IntraTransform::transform) method is given the points to transform, one slice per coordinate, and fills in the transformed points.
Here is a simple example of a suitable transformation which transforms coordinates by squaring them:

```rust
use ast_core::constants::BAD_VALUE;
use ast_core::coords::{Points, PointsMut};
use ast_core::error::AstResult;
use ast_core::mappings::{IntraFlags, IntraMap, IntraTransform};

struct SqrTran;

impl IntraTransform for SqrTran {
    fn name(&self) -> &str { "SqrTran" }
    fn nin(&self) -> Option<usize> { Some(2) }
    fn nout(&self) -> Option<usize> { Some(2) }
    fn flags(&self) -> IntraFlags { IntraFlags::default() }
    fn purpose(&self) -> &str { "Square each coordinate value" }
    fn author(&self) -> &str { "R.F. Warren-Smith & D.S. Berry" }
    fn contact(&self) -> &str { "https://www.starlink.ac.uk/docs/sun211.htx/sun211.html" }

    fn transform(
        &self,
        _map: &IntraMap,
        input: &Points<'_>,
        forward: bool,
        out: &mut PointsMut<'_>,
    ) -> AstResult<()> {
        for coord in 0..input.ncoord() {
            let pairs = out.axis_mut(coord).iter_mut().zip(input.axis(coord));
            for (y, &x) in pairs {
                *y = if x == BAD_VALUE {
                    BAD_VALUE
                } else if forward {
                    // Forward transformation.
                    x * x
                } else if x < 0.0 {
                    // Inverse transformation.
                    BAD_VALUE
                } else {
                    x.sqrt()
                };
            }
        }
        Ok(())
    }
}
```

As you can see, the transformation comes in two halves which implement the forward and inverse coordinate transformations.
The points to be transformed, and the numbers of input and output coordinates per point (in this case both are assumed equal), are given by `input` and `out`.
A pair of loops then accesses all the coordinate values.
Note that it is legitimate to omit one or other of the forward/inverse transformations and simply not to implement it, if it will not be required.
It is also permissible to require that the numbers of input and output coordinates be fixed (*e.g.* at 2), or to write the transformation so that it can handle arbitrary dimensionality, as here.

Before using an incoming coordinate, the transformation must first check that it is not set to the value [`BAD_VALUE`](crate::constants::BAD_VALUE), which indicates missing data ([Handling Missing (Bad) Coordinate Values](crate::guide::mappings#handling-missing-bad-coordinate-values)).
If it is, the same value is also assigned to any affected output coordinates.
The value `BAD_VALUE` is also generated if any coordinates cannot be transformed.
In this example, this can happen with the inverse transformation if negative values are encountered, so that the square root cannot be taken.

There are very few restrictions on what a coordinate transformation may do.
For example, it may freely perform I/O to access any external data needed, it may invoke other AST facilities (but beware of unwanted recursion), *etc.*
It may also hold data of its own, in the fields of the type that implements `IntraTransform`.
Remember, however, that whatever facilities the transformation requires must be available in every program which uses it.
A transformation is shared between threads, so it must be [`Send`] and [`Sync`].

Generally, it is not a good idea to retain context information within a transformation.
That is, it should transform each set of coordinates as a single point and retain no memory of the points it has transformed before.
This is in order to conform with the AST model of a Mapping.

If an error occurs within a transformation, it should return it as an `Err`.
This will abort the current operation, and the error will be passed on to the caller of the method that used the IntraMap.
The error code [`ErrorCode::Itfer`](crate::error::ErrorCode::Itfer) is available for this purpose, but other codes may also be used (*e.g.* if you wish to distinguish different types of error).

The other methods of `IntraTransform` describe the transformation.
[`name`](crate::mappings::IntraTransform::name) gives a name by which the transformation will be known.
This will be used when an IntraMap is written out and read back, and is case sensitive.
We recommend that you use the type's name here and make this sufficiently unusual that it is unlikely to clash with any other transformations in most people's software.

[`nin`](crate::mappings::IntraTransform::nin) and [`nout`](crate::mappings::IntraTransform::nout) give the number of input and output coordinates which the transformation will handle.
These correspond with the Nin and Nout attributes of the IntraMaps we will create.
Here, we have set them both to 2, which means that we will only be able to create IntraMaps with 2 input and 2 output coordinates (despite the fact that the transformation can actually handle other dimensionalities).
We will see later ([Variable Numbers of Coordinates](#variable-numbers-of-coordinates)) how to remove this restriction.

[`flags`](crate::mappings::IntraTransform::flags) gives a set of flags which describe the transformation in a little more detail.
We will return to this shortly ([Restricted Implementations of Transformations](#restricted-implementations-of-transformations) and [Simplifying IntraMaps](#simplifying-intramaps)).
For now, we give the default, with no flags set.

The remaining methods return character strings which document the transformation, mainly for the benefit of anyone who is unfortunate enough to encounter a reference to it in their data which they cannot interpret.
As explained above ([Limitations of IntraMaps](#limitations-of-intramaps)), you should try and avoid this, but accidents will happen, so you should always provide strings containing the following:

1. A short description of what the transformation is for.
2. The name of the author.
3. Contact details, such as an e-mail or WWW address.

The idea is that anyone finding an IntraMap in their data, but lacking the necessary transformation, should be able to contact the author and make a sensible inquiry in order to obtain it.
If you expect many inquiries, you may like to set up a World Wide Web page and use that instead (in the example above, we use the WWW address of SUN/211).

## Creating an IntraMap

Once a transformation has been written, creating an IntraMap from it is simple:

```rust
use std::sync::Arc;
use ast_core::mappings::IntraMap;
# use ast_core::constants::BAD_VALUE;
# use ast_core::coords::{Points, PointsMut};
# use ast_core::error::AstResult;
# use ast_core::mappings::{IntraFlags, IntraTransform, Mapping};
# struct SqrTran;
# impl IntraTransform for SqrTran {
#     fn name(&self) -> &str { "SqrTran" }
#     fn nin(&self) -> Option<usize> { Some(2) }
#     fn nout(&self) -> Option<usize> { Some(2) }
#     fn flags(&self) -> IntraFlags { IntraFlags::default() }
#     fn purpose(&self) -> &str { "Square each coordinate value" }
#     fn author(&self) -> &str { "R.F. Warren-Smith & D.S. Berry" }
#     fn contact(&self) -> &str { "" }
#     fn transform(&self, _map: &IntraMap, input: &Points<'_>, forward: bool,
#                  out: &mut PointsMut<'_>) -> AstResult<()> {
#         for coord in 0..input.ncoord() {
#             for (y, &x) in out.axis_mut(coord).iter_mut().zip(input.axis(coord)) {
#                 *y = if x == BAD_VALUE { BAD_VALUE } else if forward { x * x }
#                      else if x < 0.0 { BAD_VALUE } else { x.sqrt() };
#             }
#         }
#         Ok(())
#     }
# }

let intramap = IntraMap::new(Arc::new(SqrTran), 2, 2)?;
# let (x, y) = intramap.tran2(&[3.0, BAD_VALUE], &[-2.0, 1.0], true)?;
# assert_eq!((x, y), (vec![9.0, BAD_VALUE], vec![4.0, 1.0]));
# let (x, _) = intramap.tran2(&[9.0, -1.0], &[4.0, 1.0], false)?;
# assert_eq!(x, vec![3.0, BAD_VALUE]);
# Ok::<(), ast_core::error::AstError>(())
```

We simply use [`IntraMap::new`](crate::mappings::IntraMap::new) and pass it the transformation to use.
The transformation is held as an `Arc<dyn IntraTransform>`, because one transformation may serve any number of IntraMaps.

You can, of course, write any number of transformations and select which one to use whenever you create an IntraMap.
You can also create any number of independent IntraMaps using each transformation.
In this sense, each transformation you write effectively creates a new "sub-class" of IntraMap, from which you can create Objects just like any other class.

The second and third arguments to `IntraMap::new` are the numbers of input and output coordinates.
These define the Nin and Nout attributes for the IntraMap that is created and they must match the corresponding numbers given by the transformation's `nin` and `nout`.

You may set attribute values for an IntraMap in exactly the same way as for any other Mapping ([Setting Attribute Values](crate::guide::objects#setting-attribute-values), and also see [Adapting a Transformation to Individual IntraMaps](#adapting-a-transformation-to-individual-intramaps)).

## Restricted Implementations of Transformations

You may not always want to use both the forward and inverse transformations when you create an IntraMap, so it is possible to omit either from the underlying coordinate transformation.
Consider the following, for example:

```rust
use ast_core::constants::BAD_VALUE;
use ast_core::coords::{Points, PointsMut};
use ast_core::error::AstResult;
use ast_core::mappings::{IntraFlags, IntraMap, IntraTransform};

struct Poly3Tran;

impl IntraTransform for Poly3Tran {
    fn name(&self) -> &str { "Poly3Tran" }
    fn nin(&self) -> Option<usize> { Some(1) }
    fn nout(&self) -> Option<usize> { Some(1) }
    fn flags(&self) -> IntraFlags { IntraFlags { no_inv: true, ..IntraFlags::default() } }
    fn purpose(&self) -> &str { "A cubic polynomial" }
    fn author(&self) -> &str { "R.F. Warren-Smith & D.S. Berry" }
    fn contact(&self) -> &str { "" }

    fn transform(
        &self,
        _map: &IntraMap,
        input: &Points<'_>,
        _forward: bool,
        out: &mut PointsMut<'_>,
    ) -> AstResult<()> {
        // Forward transformation.
        for (y, &x) in out.axis_mut(0).iter_mut().zip(input.axis(0)) {
            *y = if x == BAD_VALUE {
                BAD_VALUE
            } else {
                6.18 + x * (0.12 + x * (-0.003 + x * 0.0000101))
            };
        }
        Ok(())
    }
}
# use ast_core::mappings::Mapping;
# let map = IntraMap::new(std::sync::Arc::new(Poly3Tran), 1, 1)?;
# assert!(map.has_forward() && !map.has_inverse());
# assert!(map.tran1(&[1.0], false).is_err());
# Ok::<(), ast_core::error::AstError>(())
```

This implements a 1-dimensional cubic polynomial transformation.
Since this is somewhat awkward to invert, however, we have only implemented the forward transformation.
This is indicated by its flags, which set [`no_inv`](crate::mappings::IntraFlags::no_inv) to indicate the lack of an inverse.
If the forward transformation were absent, we would set [`no_fwd`](crate::mappings::IntraFlags::no_fwd) instead.

## Variable Numbers of Coordinates

In our earlier examples, we have used a fixed number of input and output coordinates for a coordinate transformation.
It is not necessary to impose this restriction, however, if the transformation can cope with a variable number of coordinates (as with the example in [Writing a Transformation](#writing-a-transformation)).
We indicate the acceptability of a variable number by returning `None` from the transformation's `nin` and/or `nout`, as follows:

```rust
# use ast_core::mappings::{IntraFlags, IntraMap, IntraTransform};
# use ast_core::coords::{Points, PointsMut};
# use ast_core::error::AstResult;
# struct SqrTran;
# impl IntraTransform for SqrTran {
#     fn name(&self) -> &str { "SqrTran" }
fn nin(&self) -> Option<usize> { None }
fn nout(&self) -> Option<usize> { None }
#     fn flags(&self) -> IntraFlags { IntraFlags::default() }
#     fn purpose(&self) -> &str { "" }
#     fn author(&self) -> &str { "" }
#     fn contact(&self) -> &str { "" }
#     fn transform(&self, _: &IntraMap, _: &Points<'_>, _: bool, _: &mut PointsMut<'_>) -> AstResult<()> { Ok(()) }
# }
```

The result is that an IntraMap may now be created with any number of input and output coordinates.
For example:

```rust
use std::sync::Arc;
use ast_core::mappings::{IntraMap, Mapping};
# use ast_core::mappings::{IntraFlags, IntraTransform};
# use ast_core::coords::{Points, PointsMut};
# use ast_core::error::AstResult;
# struct SqrTran;
# impl IntraTransform for SqrTran {
#     fn name(&self) -> &str { "SqrTran" }
#     fn nin(&self) -> Option<usize> { None }
#     fn nout(&self) -> Option<usize> { None }
#     fn flags(&self) -> IntraFlags { IntraFlags::default() }
#     fn purpose(&self) -> &str { "" }
#     fn author(&self) -> &str { "" }
#     fn contact(&self) -> &str { "" }
#     fn transform(&self, _: &IntraMap, _: &Points<'_>, _: bool, _: &mut PointsMut<'_>) -> AstResult<()> { Ok(()) }
# }

let sqrtran = Arc::new(SqrTran);
let intramap1 = IntraMap::new(sqrtran.clone(), 1, 1)?;
let mut intramap2 = IntraMap::new(sqrtran.clone(), 3, 3)?;
intramap2.invert();
# Ok::<(), ast_core::error::AstError>(())
```

It is possible to fix either the number of input or output coordinates (by returning an explicit number from `nin` or `nout`), but more subtle restrictions on the number of coordinates, such as requiring that Nin and Nout be equal, are not supported.
This means that:

```rust
# use std::sync::Arc;
# use ast_core::mappings::{IntraFlags, IntraMap, IntraTransform};
# use ast_core::coords::{Points, PointsMut};
# use ast_core::error::AstResult;
# struct SqrTran;
# impl IntraTransform for SqrTran {
#     fn name(&self) -> &str { "SqrTran" }
#     fn nin(&self) -> Option<usize> { None }
#     fn nout(&self) -> Option<usize> { None }
#     fn flags(&self) -> IntraFlags { IntraFlags::default() }
#     fn purpose(&self) -> &str { "" }
#     fn author(&self) -> &str { "" }
#     fn contact(&self) -> &str { "" }
#     fn transform(&self, _: &IntraMap, _: &Points<'_>, _: bool, _: &mut PointsMut<'_>) -> AstResult<()> { Ok(()) }
# }
let intramap = IntraMap::new(Arc::new(SqrTran), 1, 2)?;
# Ok::<(), ast_core::error::AstError>(())
```

will be accepted without error, although the transformation cannot actually handle such a combination sensibly.
If this is important, it would be worth adding a check within the transformation itself, so that the error would be detected when it came to be used.

## Adapting a Transformation to Individual IntraMaps

In the examples given so far, our coordinate transformations have not made use of the `map` argument passed to them (which identifies the IntraMap whose transformation we are implementing).
In practice, this will often be the case.
However, the presence of the `map` argument allows the transformation to inquire about the IntraMap's attributes.
The transformation's behavior can therefore be modified according to any attribute values which are set.
This turns out to be a useful thing to do, so each IntraMap has a special IntraFlag attribute ([`intra_flag`](crate::mappings::IntraMap::intra_flag)) reserved for exactly this purpose.

Consider, for instance, the case where the transformation has access to several alternative sets of internally-stored data which it may apply to perform its transformation.
Rather than implement many different versions of the transformation, you may switch between them by setting a value for the IntraFlag attribute when you create an instance of an IntraMap, for example:

```rust
# use std::sync::Arc;
# use ast_core::mappings::{IntraFlags, IntraMap, IntraTransform};
# use ast_core::coords::{Points, PointsMut};
# use ast_core::error::AstResult;
# struct MyTran;
# impl IntraTransform for MyTran {
#     fn name(&self) -> &str { "MyTran" }
#     fn nin(&self) -> Option<usize> { Some(2) }
#     fn nout(&self) -> Option<usize> { Some(2) }
#     fn flags(&self) -> IntraFlags { IntraFlags::default() }
#     fn purpose(&self) -> &str { "" }
#     fn author(&self) -> &str { "" }
#     fn contact(&self) -> &str { "" }
#     fn transform(&self, _: &IntraMap, _: &Points<'_>, _: bool, _: &mut PointsMut<'_>) -> AstResult<()> { Ok(()) }
# }
# let mytran = Arc::new(MyTran);
let mut intramap1 = IntraMap::new(mytran.clone(), 2, 2)?;
intramap1.set_intra_flag("A");
let mut intramap2 = IntraMap::new(mytran.clone(), 2, 2)?;
intramap2.set_intra_flag("B");
# Ok::<(), ast_core::error::AstError>(())
```

The transformation may then inquire the value of the IntraFlag attribute (using `map.intra_flag()`) and use whichever dataset is required for that particular IntraMap.

This approach is particularly useful when the number of possible transformations is unbounded or not known in advance, in which case the IntraFlag attribute may be used to hold numerical values encoded as part of a character string (effectively using them as data for the IntraMap).
It is also superior to the use of a global switch for communication (*e.g.* setting an index to select the "current" data before using the IntraMap), because it continues to work when several IntraMaps are embedded within a more complex compound Mapping, when you may have no control over the order in which they are used.

## Simplifying IntraMaps

A notable disadvantage of IntraMaps is that they are "black boxes" as far as AST is concerned.
This means that they have limited ability to participate in the simplification of compound Mappings performed, *e.g.*, by [`simplify`](crate::mappings::Mapping#method.simplify) ([Simplifying Compound Mappings](crate::guide::cmpmaps#simplifying-compound-mappings)), because AST cannot know how they interact with other Mappings.
In reality, of course, they will often implement such specialized coordinate transformations that the simplification possibilities will be rather limited anyway.

One important simplification, however, is the ability of a Mapping to cancel with its own inverse to yield a unit Mapping (a UnitMap).
This is important because Mappings are frequently used to relate a dataset to some external standard (a celestial coordinate system, for example).
When inter-relating two similar datasets calibrated using the same standard, part of the Mapping often cancels, because it is applied first in one direction and then the other, effectively eliminating the reference to the standard.
This is often a useful simplification and can lead to greater efficiency.

Many transformations have this property of cancelling with their own inverse, but not necessarily all.
Consider the following transformation, for example:

```rust
use ast_core::constants::BAD_VALUE;
use ast_core::coords::{Points, PointsMut};
use ast_core::error::AstResult;
use ast_core::mappings::{IntraFlags, IntraMap, IntraTransform};

struct MaxTran;

impl IntraTransform for MaxTran {
    fn name(&self) -> &str { "MaxTran" }
    fn nin(&self) -> Option<usize> { None }
    fn nout(&self) -> Option<usize> { Some(1) }
    fn flags(&self) -> IntraFlags { IntraFlags { simp_if: true, ..IntraFlags::default() } }
    fn purpose(&self) -> &str { "The largest coordinate" }
    fn author(&self) -> &str { "R.F. Warren-Smith & D.S. Berry" }
    fn contact(&self) -> &str { "" }

    fn transform(
        &self,
        _map: &IntraMap,
        input: &Points<'_>,
        forward: bool,
        out: &mut PointsMut<'_>,
    ) -> AstResult<()> {
        if forward {
            // Forward transformation.
            for point in 0..input.npoint() {
                let mut hi = BAD_VALUE;
                for coord in 0..input.ncoord() {
                    let x = input.axis(coord)[point];
                    if x != BAD_VALUE && (x > hi || hi == BAD_VALUE) {
                        hi = x;
                    }
                }
                out.axis_mut(0)[point] = hi;
            }
        } else {
            // Inverse transformation.
            for coord in 0..out.ncoord() {
                out.axis_mut(coord).copy_from_slice(input.axis(0));
            }
        }
        Ok(())
    }
}
# use ast_core::mappings::Mapping;
# let map = IntraMap::new(std::sync::Arc::new(MaxTran), 3, 1)?;
# assert_eq!(map.transform(&ast_core::coords::PointSet::from_rows([[1.0, 5.0, 2.0]]), true)?.axis(0), [5.0]);
# Ok::<(), ast_core::error::AstError>(())
```

This transformation takes any number of input coordinates and returns a single output coordinate which is the maximum value of the input coordinates.
Its inverse (actually a "pseudo-inverse") sets all the input coordinates to the value of the output coordinate.
(Remember that `input` holds the original "output" coordinates when applying the inverse transformation and `out` receives the original "input" coordinates.)

If this transformation is applied in the forward direction and then in the inverse direction, it does **not** in general restore the original coordinate values.
However, if applied in the inverse direction and then the forward direction, it does.
Hence, replacing the sequence of operations with an equivalent UnitMap is possible in the latter case, but not in the former.

To distinguish these possibilities, two flags are provided to indicate what simplification (if any) is possible.
Here, the flag [`simp_if`](crate::mappings::IntraFlags::simp_if) indicates that simplification is possible if the transformation is applied in the inverse direction followed by the forward direction.
To indicate the complementary case, the flag [`simp_fi`](crate::mappings::IntraFlags::simp_fi) would be set instead.
If both simplifications are possible (as with the SqrTran transformation in [Writing a Transformation](#writing-a-transformation)), then we would set both.

```rust
# use std::sync::Arc;
# use ast_core::constants::BAD_VALUE;
# use ast_core::coords::{Points, PointsMut};
# use ast_core::error::AstResult;
# use ast_core::mappings::{IntraFlags, IntraTransform};
use ast_core::mappings::{CmpMap, Composition, DynMapping, IntraMap, Mapping, UnitMap};
# struct SqrTran;
# impl IntraTransform for SqrTran {
#     fn name(&self) -> &str { "SqrTran" }
#     fn nin(&self) -> Option<usize> { None }
#     fn nout(&self) -> Option<usize> { None }
fn flags(&self) -> IntraFlags {
    IntraFlags { simp_fi: true, simp_if: true, ..IntraFlags::default() }
}
#     fn purpose(&self) -> &str { "" }
#     fn author(&self) -> &str { "" }
#     fn contact(&self) -> &str { "" }
#     fn transform(&self, _: &IntraMap, _: &Points<'_>, _: bool, _: &mut PointsMut<'_>) -> AstResult<()> { Ok(()) }
# }

# let intramap = IntraMap::new(Arc::new(SqrTran), 2, 2)?;
// An IntraMap followed by its own inverse simplifies to a UnitMap.
let mut inverse = intramap.clone();
inverse.invert();
let pair: DynMapping = Box::new(CmpMap::new(Box::new(intramap), Box::new(inverse), Composition::Series)?);
assert!(pair.simplify()?.as_any().is::<UnitMap>());
# Ok::<(), ast_core::error::AstError>(())
```

In practice, some judgement is usually necessary when deciding whether to allow simplification.
For example, seen in one light our SqrTran transformation ([Writing a Transformation](#writing-a-transformation)) does not cancel with its own inverse, because squaring a coordinate value and then taking its square root can change the original value, if this was negative.
Therefore, replacing this combination with a UnitMap will change the behavior of a compound Mapping and should not be allowed.
Seen in another light, however, where the coordinates being processed are intrinsically all positive, it is a permissible and probably useful simplification.

If such distinctions are ever important in practice, it is simple to write two versions of the transformation with different flags (use a separate name for each) and then use whichever is appropriate when creating an IntraMap.

## Writing and Reading IntraMaps

It is most important to realize that when you write an IntraMap to a Channel ([Writing Objects to a Channel](crate::guide::channels#writing-objects-to-a-channel)), the transformation which it uses is not stored with it.
To do so is impossible, because the transformation has been compiled into your program.
However, AST does store the name of the transformation and various details about the IntraMap itself:

```text
 Begin IntraMap 	# Map points using a private transformation function
    Nin = 2 	# Number of input coordinates
 IsA Mapping 	# Mapping between coordinate systems
    Fname = "SqrTran" 	# Name of transformation function
    Purp = "Square each coordinate value" 	# Purpose of function
    Auth = "R.F. Warren-Smith & D.S. Berry" 	# Author's name
    Cntact = "https://www.starlink.ac.uk/docs/sun211.htx/sun211.html" 	# Contact address
 End IntraMap
```

This means that any program attempting to read the IntraMap ([Reading Objects from a Channel](crate::guide::channels#reading-objects-from-a-channel)) cannot make use of it unless it also has independent access to the original transformation.
If it does not have access to this transformation, an error will occur at the point where the IntraMap is read and the associated error message will direct the user to the author of the transformation for more information.

However, if the necessary transformation is available, and has been *registered* with the Channel before the read operation takes place, then AST is able to re-create the original IntraMap and will do so.
A transformation is registered with [`register_intra`](crate::Channel::register_intra).
Registration of the transformation must, of course, use the same name (and, in fact, be identical in most particulars) as was used in the original program which wrote the data.

```rust
# use std::sync::Arc;
# use ast_core::constants::BAD_VALUE;
# use ast_core::coords::{Points, PointsMut};
# use ast_core::error::AstResult;
# use ast_core::mappings::{IntraFlags, IntraTransform};
use ast_core::mappings::IntraMap;
use ast_core::{AnyObject, Channel, NativeChannel};
# struct SqrTran;
# impl IntraTransform for SqrTran {
#     fn name(&self) -> &str { "SqrTran" }
#     fn nin(&self) -> Option<usize> { None }
#     fn nout(&self) -> Option<usize> { None }
#     fn flags(&self) -> IntraFlags { IntraFlags::default() }
#     fn purpose(&self) -> &str { "Square each coordinate value" }
#     fn author(&self) -> &str { "R.F. Warren-Smith & D.S. Berry" }
#     fn contact(&self) -> &str { "" }
#     fn transform(&self, _: &IntraMap, _: &Points<'_>, _: bool, _: &mut PointsMut<'_>) -> AstResult<()> { Ok(()) }
# }

let sqrtran = Arc::new(SqrTran);
let mut out = NativeChannel::with_string_sink();
out.write(&AnyObject::IntraMap(IntraMap::new(sqrtran.clone(), 2, 2)?))?;
let text = out.into_string();

// Without the transformation, the IntraMap cannot be read.
let mut channel = NativeChannel::from_string(&text);
assert!(channel.read().is_err());

// With it registered, the IntraMap is re-created.
let mut channel = NativeChannel::from_string(&text);
channel.register_intra(sqrtran)?;
let object = channel.read()?;
# assert_eq!(object.unwrap().class_name(), "IntraMap");
# Ok::<(), ast_core::error::AstError>(())
```

This means that a set of co-operating programs which all have access to the same set of transformations and register them in identical fashion (see [Managing Transformations in Libraries](#managing-transformations-in-libraries) for how this can best be achieved) can freely exchange data that contain IntraMaps.
The need to avoid exporting such data to unsuspecting third parties ([Limitations of IntraMaps](#limitations-of-intramaps)) must, however, be re-iterated.

## Managing Transformations in Libraries

If you are developing a large suite of data reduction software, you may have a need to use IntraMaps at various points within it.
Very probably this will occur in unrelated modules which are compiled separately and then stored in a library.
Since the transformations required must be registered before IntraMaps that use them can be read, this makes it difficult to decide where to perform this registration, especially since any particular data reduction program may use an arbitrary subset of the modules in your library.

To assist with this problem, AST allows you to perform the same registration of a transformation any number of times, so long as it is the same transformation (the same `Arc`) on each occasion.
This means you do not have to keep track of whether a particular transformation has already been registered but could, in fact, register it on each occasion immediately before it is required (wherever that may be).
In order that all registrations are identical, however, it is recommended that you group them all together into a single [`IntraRegistry`](crate::mappings::IntraRegistry), perhaps made as follows:

```rust
use std::sync::{Arc, OnceLock};
use ast_core::error::AstResult;
use ast_core::mappings::{IntraRegistry, IntraTransform};
# use ast_core::coords::{Points, PointsMut};
# use ast_core::mappings::{IntraFlags, IntraMap};
# macro_rules! tran { ($t:ident) => {
#     struct $t;
#     impl IntraTransform for $t {
#         fn name(&self) -> &str { stringify!($t) }
#         fn nin(&self) -> Option<usize> { None }
#         fn nout(&self) -> Option<usize> { None }
#         fn flags(&self) -> IntraFlags { IntraFlags::default() }
#         fn purpose(&self) -> &str { "" }
#         fn author(&self) -> &str { "" }
#         fn contact(&self) -> &str { "" }
#         fn transform(&self, _: &IntraMap, _: &Points<'_>, _: bool, _: &mut PointsMut<'_>) -> AstResult<()> { Ok(()) }
#     }
# } }
# tran!(MaxTran);
# tran!(Poly3Tran);
# tran!(SqrTran);

fn my_trans() -> AstResult<IntraRegistry> {
    static TRANSFORMS: OnceLock<[Arc<dyn IntraTransform>; 3]> = OnceLock::new();
    let transforms = TRANSFORMS.get_or_init(|| [Arc::new(MaxTran), Arc::new(Poly3Tran), Arc::new(SqrTran)]);
    let mut registry = IntraRegistry::new();
    for transform in transforms {
        registry.register(transform.clone())?;
    }
    Ok(registry)
}
# let registry = my_trans()?;
# assert!(registry.get("SqrTran").is_some());
# Ok::<(), ast_core::error::AstError>(())
```

You can then simply use this registry wherever necessary: [`from_native_string_with`](crate::from_native_string_with) reads an Object using it, and each transformation it holds can be registered with a Channel.
It is, in fact, particularly important to register all relevant transformations in this way before you attempt to read an Object that might be (or contain) an IntraMap ([Writing and Reading IntraMaps](#writing-and-reading-intramaps)).
This is because you may not know in advance which of these transformations the IntraMap will use, so they must all be available in order to avoid an error.
