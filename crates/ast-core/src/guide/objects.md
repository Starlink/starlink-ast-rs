# An AST Object Primer

The AST library deals throughout with entities called Objects and a basic understanding of how to handle these is needed before you can use the library effectively.
If you are already familiar with object-oriented programming, few of the concepts should seem new to you.

If you are not already familiar with object-oriented programming, then don't worry — we will not emphasise this aspect more than is necessary and will not assume any background knowledge.
Instead, this section concentrates on presenting all the fundamental information you will need, explaining how AST Objects behave and how to manipulate them.

If you like to read documents from cover to cover, then you can consider this section as an introduction to the programming techniques used in the rest of the document.
Otherwise, you may prefer to skim through it on a first reading and return to it later as reference material.

## AST Objects

An AST Object is an entity which is used to store information and Objects come in various kinds, called *classes*, according to the sort of information they hold.
Throughout this section, we will make use of a simple Object belonging to the "ZoomMap" class to illustrate many of the basic concepts.

A [`ZoomMap`](crate::mappings::ZoomMap) is an Object that contains a recipe for converting coordinates between two hypothetical coordinate systems.
It does this by multiplying all the coordinate values by a constant called the *Zoom factor*.
A ZoomMap is a very simple Object which exists mainly for use in examples.
It allows us to illustrate the ways in which Objects are manipulated and to introduce the concept of a Mapping — a recipe for converting coordinates — which is fundamental to the way the AST library works.

## Object Creation

Let us first consider how to create a ZoomMap.
This is done very simply as follows:

```rust
use ast_core::mappings::ZoomMap;

let zoommap = ZoomMap::new(2, 5.0)?;
# Ok::<(), ast_core::error::AstError>(())
```

Each class of AST Object is a Rust type of the same name, and the function that creates an Object is usually that type's `new`.
These functions are called *constructors*, and the arguments passed to the constructor are used to initialise the new Object.
In this case, we specify 2 as the number of coordinates (*i.e.* we are going to work in a 2-dimensional space) and 5.0 as the Zoom factor to be applied.
A constructor checks its arguments, which is why [`ZoomMap::new`](crate::mappings::ZoomMap::new) returns an [`AstResult`](crate::error::AstResult): a Zoom factor of zero is an error.

The value returned by the constructor is the Object itself, which you own.
You perform all subsequent operations on the Object by calling its methods.

## The Object Hierarchy

Now that we have created our first ZoomMap, let us examine how it relates to other kinds of Object before investigating what we can do with it.

We have so far indicated that a ZoomMap is a kind of Object and have also mentioned that it is a kind of Mapping as well.
In Rust, what it means to be a Mapping is the [`Mapping`](crate::mappings::Mapping) trait, which every kind of Mapping implements.
Other traits describe the other families of Object: [`Frame`](crate::frames::Frame) for coordinate systems, [`Region`](crate::regions::Region) for areas within them, and [`Channel`](crate::Channel) for reading and writing Objects.

If we look at some of the other Objects used by the AST library, we can see how these are related (don't worry about what they do at this stage):

- A [`UnitMap`](crate::mappings::UnitMap) and a [`ZoomMap`](crate::mappings::ZoomMap) are Mappings.
- A [`BasicFrame`](crate::frames::BasicFrame) is a Frame.
- A [`FrameSet`](crate::frames::FrameSet) is both a Frame and a Mapping.
- A [`FitsChan`](crate::FitsChan) is a Channel, and is not a Mapping.

The general rule is that an Object may be used wherever a trait it implements is called for.
We can therefore use our ZoomMap whenever a Mapping is called for.

Sometimes, this can lead to some spectacular short-cuts by avoiding the need to break large Objects down in order to access their components.
With some practice and a little lateral thinking you should soon be able to spot opportunities for this.

When the class of an Object is not known in advance, such as when it is read from a Channel, it is held as an [`AnyObject`](crate::AnyObject), which has one variant for each class.

## Displaying Objects

Let us now return to the ZoomMap that we created earlier and examine what it's made of.
There is a method for doing this, called [`show`](crate::mappings::ZoomMap::show), which is provided mainly for looking at Objects while you are debugging programs.
Every class of Object has it:

```rust
# use ast_core::mappings::ZoomMap;
# let zoommap = ZoomMap::new(2, 5.0)?;
zoommap.show()?;
# Ok::<(), ast_core::error::AstError>(())
```

The output from this will appear on the standard output stream and should look like the following:

```text
 Begin ZoomMap 	# Zoom about the origin
    Nin = 2 	# Number of input coordinates
 IsA Mapping 	# Mapping between coordinate systems
    Zoom = 5 	# Zoom factor
 End ZoomMap
```

Here, the "Begin" and "End" lines mark the beginning and end of the ZoomMap, while the values 2 and 5 are simply the values we supplied to initialise it.
These have been given simple names to make them easy to refer to, and each line carries a comment saying what it holds.

The line in the middle which says "IsA Mapping" is a dividing line between the two values.
It indicates that the "Nin" value is a property shared by all Mappings, so the ZoomMap has inherited this from its *parent class* (Mapping).
The "Zoom" value, however, is specific to a ZoomMap and isn't shared by other kinds of Mappings.

The same text is returned as a string by [`to_native_string`](crate::mappings::ZoomMap::to_native_string).

## Getting Attribute Values

We saw above how to display the internal values of an Object, but what about accessing these values from a program?
Not all internal Object values are accessible in this way, but many are.
Those that are, are called *attributes*.

Each attribute has a method that returns its value, named after the attribute, and the value has the attribute's type.
As an example, consider obtaining the value of the Nin attribute for the ZoomMap created earlier.
This could be done as follows:

```rust
use ast_core::mappings::Mapping;
# use ast_core::mappings::ZoomMap;
# let zoommap = ZoomMap::new(2, 5.0)?;

let nin = zoommap.nin();
# assert_eq!(nin, 2);
# Ok::<(), ast_core::error::AstError>(())
```

Here, [`nin`](crate::mappings::Mapping::nin) belongs to the `Mapping` trait, because every Mapping has a Nin attribute, so the trait has to be in scope.
If we had wanted the value of the Zoom attribute, we would have used [`zoom`](crate::mappings::ZoomMap::zoom), which belongs to the ZoomMap itself:

```rust
# use ast_core::mappings::ZoomMap;
# let zoommap = ZoomMap::new(2, 5.0)?;
let zoom = zoommap.zoom();
# assert_eq!(zoom, 5.0);
# Ok::<(), ast_core::error::AstError>(())
```

## Setting Attribute Values

Some attribute values are read-only and cannot be altered after an Object has been created.
The Nin attribute of a ZoomMap (describing the number of coordinates) is like this.
It is defined when the ZoomMap is created, but cannot then be altered.

Other attributes, however, can be modified whenever you want.
A ZoomMap's Zoom attribute is like this.
If we wanted to change it, this could be done simply as follows:

```rust
# use ast_core::mappings::ZoomMap;
# let mut zoommap = ZoomMap::new(2, 5.0)?;
zoommap.set_zoom(99.6)?;
# assert_eq!(zoommap.zoom(), 99.6);
# Ok::<(), ast_core::error::AstError>(())
```

which sets the value to 99.6.
A method that sets an attribute is named after it with `set_` in front, and changes the Object in place, so the Object has to be declared `mut`.
Where a value can be invalid, as a Zoom factor of zero is, the setter returns an `AstResult`.

## Testing, Clearing and Defaulting Attributes

You can get a value for any Object attribute at any time, regardless of whether a value has previously been set for it.
If no value has been set, the AST library will generate a suitable default value.

Often, the default value of an attribute will not simply be trivial (zero or blank) but may involve considerable processing to calculate.
Wherever possible, defaults are designed to be real-life, sensible values that convey information about the state of the Object.
In particular, they may often be based on the values of other attributes, so their values may change in response to changes in these other attributes.
The ZoomMap class that we have studied so far is a little too simple to show this behaviour, but we will meet it later on.

An attribute that returns a default value in this way is said to be *un-set*.
Conversely, once an explicit value has been assigned to an attribute, it becomes *set* and will always return precisely that value, never a default.

The distinction between set and un-set attributes is important and affects the behaviour of several key routines in the AST library.
You can test if an attribute is set using the method named after it with `test_` in front, which returns a boolean result, as in:

```rust
use ast_core::mappings::Mapping;
# use ast_core::mappings::ZoomMap;
# let mut zoommap = ZoomMap::new(2, 5.0)?;
# zoommap.set_report(true);

if zoommap.test_report() {
    // <the Report attribute is set>
}
# assert!(zoommap.test_report());
# Ok::<(), ast_core::error::AstError>(())
```

Once an attribute is set, you can return it to its un-set state using the method with `clear_` in front.
The effect is as if it had never been set in the first place.
For example:

```rust
# use ast_core::mappings::{Mapping, ZoomMap};
# let mut zoommap = ZoomMap::new(2, 5.0)?;
# zoommap.set_report(true);
zoommap.clear_report();
# assert!(!zoommap.test_report());
# Ok::<(), ast_core::error::AstError>(())
```

would ensure that the default value of the Report attribute is used subsequently.

## Transforming Coordinates

We now have the necessary apparatus to start using our ZoomMap to show what it is really for.

The purpose of a ZoomMap is to multiply coordinates by a constant zoom factor.
To witness this in action, we will first set the Report attribute for our ZoomMap to `true`:

```rust
# use ast_core::mappings::{Mapping, ZoomMap};
# let mut zoommap = ZoomMap::new(2, 5.0)?;
zoommap.set_report(true);
# Ok::<(), ast_core::error::AstError>(())
```

This boolean attribute, which is present in all Mappings (and a ZoomMap is a Mapping), causes the automatic display of all coordinate values that the Mapping converts.
It is not a good idea to leave this feature turned on in a finished program, but it can save a lot of work during debugging.

Our next step is to set up some coordinates for the ZoomMap to work on, using two arrays `xin` and `yin`.
Note that these are arrays of `f64`, as are all coordinate data processed by the AST library:

```rust
let xin = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
let yin = [0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 18.0];
```

We will now use the method [`tran2`](crate::mappings::Mapping::tran2) to transform the input coordinates.
This is the most commonly-used (2-dimensional) coordinate transformation method.
It belongs to the `Mapping` trait, so it can only be used with an Object that is a Mapping.

Fortunately, a ZoomMap is a Mapping, so we can use it with `tran2` to transform our coordinates, as follows:

```rust
# use ast_core::mappings::{Mapping, ZoomMap};
# let mut zoommap = ZoomMap::new(2, 5.0)?;
# zoommap.set_report(true);
# let xin = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
# let yin = [0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 18.0];
let (xout, yout) = zoommap.tran2(&xin, &yin, true)?;
# assert_eq!(xout[9], 45.0);
# assert_eq!(yout[9], 90.0);
# Ok::<(), ast_core::error::AstError>(())
```

Here, the third argument value of `true` indicates that we want to transform in the *forward* direction (from input to output).

Because our ZoomMap's Report attribute is set, this will cause the effects of the ZoomMap on the coordinates to be displayed on the standard output stream:

```text
(0, 0) --> (0, 0)
(1, 2) --> (5, 10)
(2, 4) --> (10, 20)
(3, 6) --> (15, 30)
(4, 8) --> (20, 40)
(5, 10) --> (25, 50)
(6, 12) --> (30, 60)
(7, 14) --> (35, 70)
(8, 16) --> (40, 80)
(9, 18) --> (45, 90)
```

This shows the coordinate values of each point both before and after the ZoomMap is applied.
You can see that each coordinate value has been multiplied by the factor 5 determined by the Zoom attribute value.
The transformed coordinates are returned in `xout` and `yout`.

If we wanted to transform in the opposite direction, we need simply change the third argument of `tran2` from `true` to `false`.
We can also feed the output coordinates from the above back into the method:

```rust
# use ast_core::mappings::{Mapping, ZoomMap};
# let mut zoommap = ZoomMap::new(2, 5.0)?;
# zoommap.set_report(true);
# let (xout, yout) = zoommap.tran2(&[0.0, 1.0], &[0.0, 2.0], true)?;
let (xin, yin) = zoommap.tran2(&xout, &yout, false)?;
# assert_eq!((xin, yin), (vec![0.0, 1.0], vec![0.0, 2.0]));
# Ok::<(), ast_core::error::AstError>(())
```

The output would then look like:

```text
(0, 0) --> (0, 0)
(5, 10) --> (1, 2)
(10, 20) --> (2, 4)
(15, 30) --> (3, 6)
(20, 40) --> (4, 8)
(25, 50) --> (5, 10)
(30, 60) --> (6, 12)
(35, 70) --> (7, 14)
(40, 80) --> (8, 16)
(45, 90) --> (9, 18)
```

This is termed the *inverse* transformation (we have converted from output to input) and you can see that the original coordinates have been recovered by dividing by the Zoom factor.

## Disposing of Objects

So far, we have looked at creating Objects and using them in various simple ways but have not yet considered how to get rid of them again.

Every Object consumes various computer resources (principally memory).
An Object is an ordinary Rust value, so it is freed when its owner goes out of scope, and there is nothing you need to do to dispose of it.

## AST Objects within Multi-threaded Applications

An Object may be moved from one thread to another, since every Object is [`Send`].
Objects are not [`Sync`], however, so a reference to one cannot be shared between threads, and two threads can never access the same Object at once.

If two or more threads need access to an Object, a copy of the Object should be made for each thread (see [Copying Objects](#copying-objects)) and the copies moved to the other threads.
Note, if a thread modifies its Object, the modification will have no effect on the other threads, because the Object copies are independent of each other.

## Copying Objects

Some classes of Object are designed to hold collections of other Objects.
Each holds its own copies of the Objects it contains, so no two Objects ever share a component, and changing one Object (say assigning an attribute value) never affects another.

If you require a second, independent Object like an existing one, simply use `clone` to make a copy of it, *e.g.*:

```rust
# use ast_core::mappings::ZoomMap;
# let zoommap1 = ZoomMap::new(2, 5.0)?;
let mut zoommap2 = zoommap1.clone();
# zoommap2.set_zoom(2.0)?;
# assert_eq!(zoommap1.zoom(), 5.0);
# Ok::<(), ast_core::error::AstError>(())
```

This process will create a true copy of any Object.
This copy will not share any component with the original Object (everything is duplicated), so you can then modify it safely, without fear of affecting either the original or any other Object.

## Error Detection

If an error occurs in an AST method (for example, if you supply an invalid argument), the method returns an error, as the `Err` variant of an [`AstResult`](crate::error::AstResult), and has no other effect.
The error is an [`AstError`](crate::error::AstError), which describes what went wrong and carries an [`ErrorCode`](crate::error::ErrorCode) identifying the kind of error, which allows you to detect it, as follows:

```rust
use ast_core::error::ErrorCode;
use ast_core::mappings::ZoomMap;

match ZoomMap::new(2, 0.0) {
    Ok(zoommap) => {
        // <use the ZoomMap>
#       unreachable!("{}", zoommap.zoom());
    }
    Err(error) => {
        // <an error has occurred>
        assert_eq!(error.code(), ErrorCode::Zoomi);
        eprintln!("{error}");
    }
}
```

In this example an error occurs, because we have attempted to create a ZoomMap with a Zoom factor of zero, which is not allowed.
The code of the error is obtained using [`code`](crate::error::AstError::code), and displaying the error gives a message describing it.

Usually, you do not need to check for errors after every method.
Instead, you can invoke a succession of AST methods in a function which itself returns an `AstResult`, using `?` after each.
If an error occurs in any of them, the following ones are not called and the error is returned from the function, for example:

```rust
use ast_core::error::AstResult;
use ast_core::mappings::{Mapping, ZoomMap};

fn zoom_a_point() -> AstResult<(Vec<f64>, Vec<f64>)> {
    let zoommap = ZoomMap::new(2, 5.0)?;
    let (x, y) = zoommap.tran2(&[1.0], &[2.0], true)?;
    zoommap.tran2(&x, &y, false)
}

match zoom_a_point() {
    Ok(_) => {}
    Err(error) => {
        // <an error has occurred>
#       panic!("{error}");
    }
}
```

An error never leaves any state behind, so once you have handled one, AST methods execute normally again.
