# Inter-Relating Coordinate Systems (Mappings)

In [An AST Object Primer](crate::guide::objects) we used the ZoomMap as an example of a Mapping.
We saw how it could be used to transform coordinates from its input to its output and back again ([Transforming Coordinates](crate::guide::objects#transforming-coordinates)).
We also saw how its behavior could be controlled by setting various attributes, such as the Zoom factor and the Report attribute that made it display coordinate values as it transformed them.

In this section, we will look at Mappings a bit more thoroughly and explore the behavior which is common to all the Mappings provided by AST.
This is good background for what follows, because many of the Objects we discuss later will also turn out to be Mappings in various disguises.

## The Mapping Class

Before we start, it is worth taking a quick look at the Mapping class as a whole and some of the classes it contains:

- [`CmpMap`](crate::mappings::CmpMap)
- [`DssMap`](crate::mappings::DssMap)
- [`GrismMap`](crate::mappings::GrismMap)
- [`IntraMap`](crate::mappings::IntraMap)
- [`LutMap`](crate::mappings::LutMap)
- [`MathMap`](crate::mappings::MathMap)
- [`MatrixMap`](crate::mappings::MatrixMap)
- [`PermMap`](crate::mappings::PermMap)
- [`PolyMap`](crate::mappings::PolyMap) and [`ChebyMap`](crate::mappings::ChebyMap)
- [`SlaMap`](crate::mappings::SlaMap)
- [`SpecMap`](crate::mappings::SpecMap)
- [`TimeMap`](crate::mappings::TimeMap)
- [`UnitMap`](crate::mappings::UnitMap)
- [`WcsMap`](crate::mappings::WcsMap)
- [`ZoomMap`](crate::mappings::ZoomMap)

AST does not provide a way to create a basic Mapping.
This is because the Mapping class itself is "virtual" and basic Mappings are of no use in themselves.
In Rust, the Mapping class is the [`Mapping`](crate::mappings::Mapping) trait, which each of these classes implements.
However, it provides more than just a convenient heading for them because it bestows all classes of Mapping with common properties (*e.g.* attributes) and behavior.
By examining the Mapping trait, we are therefore examining the things that all other Mappings have in common.

## The Mapping Model

The concept of a Mapping was illustrated in [Figure 1](crate::guide::concepts#relationships-between-coordinate-systems).
It is a black box which you can supply with a set of coordinate values in return for a set of transformed coordinates.
The two sets are termed *input* and *output* coordinates.
You can also go back the other way and transform output coordinates back into input coordinates, as we saw in [Transforming Coordinates](crate::guide::objects#transforming-coordinates).

## Changing Attributes of a Mapping

Many classes of Mapping have attributes that provide values for parameters used within the transformation.
For instance, the ZoomMap class has an attribute called "Zoom" that gives the scalar value by which each coordinate is to be multiplied.
These attribute values may be changed at any time.
A Mapping that is included within another Object, such as a FrameSet or a CmpMap, is the containing Object's own copy, so changing the original Mapping afterwards has no effect on the containing Object.

## Input and Output Coordinate Numbers

In general, the number of coordinates you feed into a Mapping to represent a single point need not be the same as the number that comes out.
Often these numbers will be the same, and often they will both equal 2 (because 2-dimensional coordinate systems are common), but this needn't necessarily be the case.

The number of coordinates required to specify an input point is represented by the attribute Nin and the number required to specify an output point is represented by Nout.
These are read-only attributes common to all Mappings.
Generally, their values are fixed when a Mapping is created.

In [Object Creation](crate::guide::objects#object-creation), we saw how the Nin attribute for a ZoomMap was initialized by the constructor which created it.
In this case, the Nout attribute was not needed and it implicitly took the same value as Nin, but we could have inquired about its value had we wanted, as follows:

```rust
use starlink_ast::mappings::{Mapping, ZoomMap};

# let zoommap = ZoomMap::new(2, 5.0)?;
let nout = zoommap.nout();
# assert_eq!(nout, 2);
# Ok::<(), starlink_ast::error::AstError>(())
```

## Forward and Inverse Transformations

We stated earlier that a Mapping may be used to transform coordinates either from input to output, or *vice versa*.
These are termed its *forward* and *inverse* transformations.

This statement was not quite accurate, however, because in general Mappings are only **potentially** capable of working in both directions.
In practice, coordinate transformation may only be feasible in one direction or the other because some functions are not easily inverted (they may be multi-valued, for instance).
Allowance must be made for this, so each Mapping has two read-only boolean attributes, TranForward and TranInverse, which indicate whether each transformation is available, and which are given by [`has_forward`](crate::mappings::Mapping::has_forward) and [`has_inverse`](crate::mappings::Mapping::has_inverse).

A transformation is available if the corresponding attribute is `true`, otherwise it is not.
Most of the Mappings provided by the AST library work in both directions, although the LutMap can behave otherwise.
Attempting to use a Mapping to apply a transformation which is not available will result in an error.

## Inverting Mappings

An important attribute, common to all Mappings, is the Invert flag.
This is a boolean attribute that can be assigned a new value at any time.
If it is `true`, it has the effect of interchanging the Mapping's input and output coordinates and the Mapping is then said to be *inverted*.
By default, the Invert attribute is `false`.

There is no magic in this.
There is no fancy arithmetic involved in inverting mathematical functions, for instance.
The Invert flag is simply a switch that interchanges a Mapping's input and output ports.
If it is `true`, the Mapping's Nin and Nout attributes are swapped, its TranForward and TranInverse attributes are swapped, and when you ask for what was once the forward transformation you get the inverse transformation instead (and *vice versa*).
When you return the Invert attribute to `false`, the Mapping returns to its original behavior.

Often, the actual value of the Invert attribute is unimportant and you simply wish to invert its boolean sense, so that what was the Mapping's input becomes its output and *vice versa*.
This is most easily accomplished using [`invert`](crate::mappings::Mapping::invert), as follows:

```rust
# use starlink_ast::mappings::{Mapping, ZoomMap};
# let mut mapping = ZoomMap::new(2, 5.0)?;
mapping.invert();
# assert!(mapping.is_inverted());
# assert_eq!(mapping.tran2(&[5.0], &[5.0], true)?, (vec![1.0], vec![1.0]));
# Ok::<(), starlink_ast::error::AstError>(())
```

If the Mapping you have happens to be the wrong way around, `invert` allows you to correct the problem.

## Finding the Rate of Change of a Mapping Output

The [`rate`](crate::mappings::Mapping::rate) method can be used to find the rate of change of any Mapping output with respect to any Mapping input, at a given input position.
The method used produces good accuracy (typically a relative error of 10E-10 or less) but may require the Mapping to be evaluated 100 or more times.

## Reporting Coordinate Transformations

We have already seen ([Transforming Coordinates](crate::guide::objects#transforming-coordinates)) how the boolean Report attribute of a Mapping works.
If it is `true`, the operation of transforming a set of coordinates will result in a report being written to standard output.
This will display the coordinate values before and after transformation.
It can save considerable time during program development by eliminating the need to add loops and output statements to your program.

In a finished program, however, you should be careful that the Report attribute is not set unless you want to see the output (there may often be rather a lot of this!).
To help prevent unwanted output being produced by accident, the Report attribute is unusual in that its value is not preserved when a Mapping is copied using `clone` ([Copying Objects](crate::guide::objects#copying-objects)).
Instead, it reverts to its default of `false` (*i.e.* un-set) in the copy.

## Handling Missing (Bad) Coordinate Values

Even when coordinates can, in principle, be transformed in either direction by a Mapping, there may still be instances where specific coordinate values cannot be handled.
For example, the Mapping may be mathematically intractable (*e.g.* singular) in certain places, or it may map a subset of one space on to another, so that some points in one space are not represented in the other.
Sky projections often show this behavior, since it is quite common to project only half of the celestial sphere on to two dimensions, omitting points on the opposite side of the sky.
There are many other examples.

To indicate when coordinates cannot be transformed, for whatever reason, AST substitutes a special output coordinate value given by the constant [`BAD_VALUE`](crate::constants::BAD_VALUE).
Before making use of coordinates generated by any of the AST transformation methods, therefore, you may need to check for the presence of this value.

Because coordinates with the value `BAD_VALUE` can be generated in this way, all other AST methods are also capable of recognizing this value and handling it appropriately.
The coordinate transformation methods do this by propagating any missing input coordinate information through to their output.
This means that if you supply coordinates with the value `BAD_VALUE`, the returned coordinates are also likely to contain this value.
Here, for example, is what happens if you use a ZoomMap (with Zoom factor 5) to transform such a set of coordinates:

```text
(0, 0) --> (0, 0)
(<bad>, 2) --> (<bad>, 10)
(2, 4) --> (10, 20)
(3, 6) --> (15, 30)
(4, <bad>) --> (20, <bad>)
(5, 10) --> (25, 50)
(<bad>, <bad>) --> (<bad>, <bad>)
(7, 14) --> (35, 70)
(8, 16) --> (40, 80)
(9, 18) --> (45, 90)
```

The `BAD_VALUE` value is represented by the string "&lt;bad&gt;".
This is a case of "garbage in, garbage out" but at least it's consistent garbage that you can recognize!

```rust
use starlink_ast::constants::BAD_VALUE;
use starlink_ast::mappings::{Mapping, ZoomMap};

let zoommap = ZoomMap::new(2, 5.0)?;
let (x, y) = zoommap.tran2(&[BAD_VALUE, 4.0], &[2.0, BAD_VALUE], true)?;
assert_eq!(x, [BAD_VALUE, 20.0]);
assert_eq!(y, [10.0, BAD_VALUE]);
# Ok::<(), starlink_ast::error::AstError>(())
```

A coordinate given as `f64::NAN` is also taken as missing, though the result always uses `BAD_VALUE`.

Note how the presence of the `BAD_VALUE` value in one input dimension does not necessarily result in the loss of information for all output dimensions.
Sometimes, such loss will be unavoidable, but in general an attempt is made to preserve information as far as possible.
The exact behavior will depend on the Mapping involved.

## Example — the UnitMap

The [`UnitMap`](crate::mappings::UnitMap) is the simplest of Mappings.
It is a null Mapping.
Its purpose is simply to copy coordinate values, unaltered, from its input to its output and *vice versa*.

A UnitMap has no additional attributes beyond those of a basic Mapping.
Its Nin and Nout attributes are always equal and are specified by the argument supplied to its constructor.
For example:

```rust
use starlink_ast::mappings::UnitMap;

let unitmap = UnitMap::new(2);
```

will create a UnitMap that copies 2-dimensional coordinates.
Inverting a UnitMap has no effect beyond changing the value of its Invert attribute.

The main use of a UnitMap is to allow a Mapping to be supplied when one is required (as an argument to a function, for example) but you wish it to leave coordinate values unchanged.

## Example — the PermMap

The [`PermMap`](crate::mappings::PermMap) is a rather more complicated Mapping than we have met previously.
Its purpose is to change the order, or number, of coordinates.
It is also able to substitute fixed values for coordinates.

To illustrate its action, suppose our input coordinates are denoted by (x₁,x₂,x₃,x₄) in a 4-dimensional space and suppose our output coordinates are to be (x₄,x₁,x₂,x₃).
Our PermMap, therefore, should rotate the coordinate values by one position.

To create such a PermMap, we first set up two integer arrays.
One of these, `outperm`, controls the selection of input coordinates for use in the output and the other, `inperm`, controls selection of output coordinates for use in the input:

```rust
let outperm = [4, 1, 2, 3];
let inperm = [2, 3, 4, 1];
```

Note that the numbers we store in these arrays are the indices of the coordinates that we want to select.
We have chosen these so that the forward and inverse transformations will perform complementary permutations on the coordinates.

The PermMap is then created by passing these arrays to its constructor, as follows:

```rust
use starlink_ast::mappings::PermMap;

# let outperm = [4, 1, 2, 3];
# let inperm = [2, 3, 4, 1];
let permmap = PermMap::new(&inperm, &outperm, &[])?;
# Ok::<(), starlink_ast::error::AstError>(())
```

The number of input and output coordinates are the lengths of the two arrays, both 4 in this example.
The resulting PermMap would have the following effect when used to transform coordinates:

```text
Forward:
   (1, 2, 3, 4) --> (4, 1, 2, 3)
   (2, 4, 6, 8) --> (8, 2, 4, 6)
   (3, 6, 9, 12) --> (12, 3, 6, 9)
   (4, 8, 12, 16) --> (16, 4, 8, 12)
   (5, 10, 15, 20) --> (20, 5, 10, 15)

Inverse:
   (4, 1, 2, 3) --> (1, 2, 3, 4)
   (8, 2, 4, 6) --> (2, 4, 6, 8)
   (12, 3, 6, 9) --> (3, 6, 9, 12)
   (16, 4, 8, 12) --> (4, 8, 12, 16)
   (20, 5, 10, 15) --> (5, 10, 15, 20)
```

```rust
# use starlink_ast::mappings::{Mapping, PermMap};
use starlink_ast::coords::PointSet;

# let permmap = PermMap::new(&[2, 3, 4, 1], &[4, 1, 2, 3], &[])?;
let points = PointSet::from_rows([[1.0, 2.0, 3.0, 4.0], [2.0, 4.0, 6.0, 8.0]]);
let forward = permmap.transform(&points, true)?;
assert_eq!(&*forward.point(0), [4.0, 1.0, 2.0, 3.0]);
let inverse = permmap.transform(&forward, false)?;
assert_eq!(&*inverse.point(1), [2.0, 4.0, 6.0, 8.0]);
# Ok::<(), starlink_ast::error::AstError>(())
```

If the number of input and output coordinates are unequal so, also, will be the size of the `outperm` and `inperm` arrays.
This means, however, that we cannot fill them with coordinate indices so that they perform complementary permutations, because one transformation will lose information (discard a coordinate) that the other cannot recover.
To give an example, consider the following:

```rust
let outperm = [4, 3, 2];
let inperm = [-1, 3, 2, 1];
let con = [99.004];
```

In this case, the forward transformation will change (x₁,x₂,x₃,x₄) into (x₄,x₃,x₂) and will discard x₁.
The inverse transformation restores the original coordinate order, but has no value to assign to the first coordinate.
In this case, the number entered in the `inperm` array is −1.

This negative value indicates that the coordinate value should be obtained by addressing the first element of the `con` array (*i.e.* `con[0]`).
This array, ignored in the previous example, may then be used to supply a value for the missing coordinate.

The constructor:

```rust
# use starlink_ast::mappings::PermMap;
# let (outperm, inperm, con) = ([4, 3, 2], [-1, 3, 2, 1], [99.004]);
let permmap = PermMap::new(&inperm, &outperm, &con)?;
# Ok::<(), starlink_ast::error::AstError>(())
```

will then create a PermMap with the following effect when used to transform coordinates:

```text
Forward:
   (1, 2, 3, 4) --> (4, 3, 2)
   (2, 4, 6, 8) --> (8, 6, 4)
   (3, 6, 9, 12) --> (12, 9, 6)
   (4, 8, 12, 16) --> (16, 12, 8)
   (5, 10, 15, 20) --> (20, 15, 10)

Inverse:
   (4, 3, 2) --> (99.004, 2, 3, 4)
   (8, 6, 4) --> (99.004, 4, 6, 8)
   (12, 9, 6) --> (99.004, 6, 9, 12)
   (16, 12, 8) --> (99.004, 8, 12, 16)
   (20, 15, 10) --> (99.004, 10, 15, 20)
```

```rust
# use starlink_ast::mappings::{Mapping, PermMap};
# use starlink_ast::coords::PointSet;
# let permmap = PermMap::new(&[-1, 3, 2, 1], &[4, 3, 2], &[99.004])?;
let points = PointSet::from_rows([[4.0, 3.0, 2.0]]);
let inverse = permmap.transform(&points, false)?;
assert_eq!(&*inverse.point(0), [99.004, 2.0, 3.0, 4.0]);
# Ok::<(), starlink_ast::error::AstError>(())
```

The `con` array may contain more than one value if necessary and may be addressed by both the `inperm` and `outperm` arrays using coordinate indices −1, −2, −3, *etc.* to refer to the first, second, third, *etc.* elements.

If there is no suitable replacement value that can be supplied *via* the `con` array, a value of zero may be entered into the `outperm` and/or `inperm` arrays.
This causes the value `BAD_VALUE` to be used for the affected coordinate, thus indicating a missing coordinate value ([Handling Missing (Bad) Coordinate Values](#handling-missing-bad-coordinate-values)).

The principal use for a PermMap lies in matching a coordinate system to a data array where there is a choice of storage order for the data.
PermMaps are also useful for discarding unwanted coordinates so as to reduce the number of dimensions, such as when selecting a "slice" from a multi-dimensional array.
