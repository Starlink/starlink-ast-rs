# Saving and Restoring Objects (Channels)

Facilities are provided by the AST library for performing input and output (I/O) with any kind of Object.
This means it is possible to write any Object into various external representations for storage, and then to read these representations back in, so as to restore the original Object.
Typically, an Object would be written by one program and read back in by another.

We refer to "external representations" in the plural because AST is designed to function independently of any particular data storage system.
This means that Objects may need converting into a number of different external representations in order to be compatible with (say) the astronomical data storage system in which they will reside.

In this section, we discuss the basic I/O facilities which support external representations based on a textual format referred to as the AST "native format".
These are implemented using a new kind of Object — a Channel.
We will examine later how to use another representation, based on the use of FITS headers, for storing Objects.
This is implemented using a more specialized form of Channel called a FitsChan ([Storing AST Objects in FITS Headers](crate::guide::fitschans)).

What every kind of Channel has in common is the [`Channel`](crate::Channel) trait.
The basic Channel, which uses the native format, is a [`NativeChannel`](crate::NativeChannel).

## The Channel Model

The best way to start thinking about a Channel is like a file, and to think of the process of creating a Channel as that of opening a file.
Subsequently, you can read and write Objects *via* the Channel.

This analogy is not quite perfect, however, because a Channel has, in principle, two "files" attached to it.
One is used when reading, and the other when writing.
These are termed the Channel's *source* and *sink* respectively.
In practice, the source and sink may both be the same, in which case the analogy with a file is correct, but this need not always be so.
It is not necessarily so with the basic Channel, as we will now see ([Creating a Channel](#creating-a-channel)).

## Creating a Channel

The process of creating a Channel is straightforward:

```rust
use starlink_ast::NativeChannel;

let channel = NativeChannel::new();
```

A new Channel has no source or sink of its own.
This requests the default behavior, which means that textual input will be read from the program's standard input stream (typically, this means your keyboard) while textual output will go to the standard output stream (typically appearing on your screen).
On UNIX systems, of course, either of these streams can easily be redirected to files.
This default behavior can be changed by giving the Channel a source and a sink ([Reading Objects from Files](#reading-objects-from-files) and [Writing Objects to Files](#writing-objects-to-files)), or by assigning values to the Channel's SinkFile and/or SourceFile attributes.
These attributes specify the paths to text files that are to be used in place of the standard input and output streams.

## Writing Objects to a Channel

The process of saving Objects is very straightforward.
You can simply write any Object to a Channel using [`write`](crate::Channel::write), as follows:

```rust
use starlink_ast::{AnyObject, Channel, NativeChannel};
use starlink_ast::mappings::ZoomMap;

# let mut channel = NativeChannel::with_string_sink();
let object = AnyObject::ZoomMap(ZoomMap::new(2, 5.0)?);
let nobj = channel.write(&object)?;
# assert_eq!(nobj, 1);
# Ok::<(), starlink_ast::error::AstError>(())
```

The Object is given to `write` as an [`AnyObject`](crate::AnyObject), which can hold an Object of any class.
The effect of this will be to produce a textual description of the Object which will appear, by default, on your program's standard output stream.
Any class of Object may be converted into text in this way.

`write` returns a count of the number of Objects written.
Usually, this will be one, unless the Object supplied cannot be represented.
With a basic Channel all Objects can be represented, so a value of one will always be returned unless there has been an error.
We will see later, however, that more specialized forms of Channel may impose restrictions on the kind of Object you can write ([Limitations of Foreign Encodings](crate::guide::foreign_fits#limitations-of-foreign-encodings)).
In such cases, `write` may return zero to indicate that the Object was not acceptable.

## Reading Objects from a Channel

Before discussing the format of the output produced above ([Writing Objects to a Channel](#writing-objects-to-a-channel)), let us consider how to read it back, so as to reconstruct the original Object.
Naturally, we would first need to save the output in a file.
We can do that either by using the SinkFile attribute, or (on UNIX systems), by redirecting standard output to a file using a shell command like:

```text
program1 >file
```

Within a subsequent program, we can read this Object back in by using [`read`](crate::Channel::read), having first created a suitable Channel:

```rust
# use starlink_ast::{Channel, NativeChannel};
# let mut channel = NativeChannel::from_string(" Begin UnitMap\n    Nin = 2\n End UnitMap\n");
let object = channel.read()?;
# assert!(object.is_some());
# Ok::<(), starlink_ast::error::AstError>(())
```

By default, this will read from the standard input stream (the default source for a basic Channel), so we would need to ensure that our second program reads its input from the file in which the Object description is stored.
On UNIX systems, we could again use a shell redirection command such as:

```text
program2 <file
```

Alternatively, we could have assigned a value to the SourceFile attribute before invoking `read`.

## Saving and Restoring Multiple Objects

I/O operations performed on a basic Channel are sequential.
This means that if you write more than one Object to a Channel, each new Object's textual description is simply appended to the previous one.
You can store any number of Objects in this way, subject only to the storage space you have available.

After you read an Object back from a basic Channel, the Channel is "positioned" at the end of that Object's textual description.
If you then perform another read, you will read the next Object's textual description and therefore retrieve the next Object.
This process may be repeated to read each Object in turn.
When there are no more Objects to be read, `read` will return `None` to indicate an *end-of-file*.

```rust
use starlink_ast::mappings::{UnitMap, ZoomMap};
use starlink_ast::{AnyObject, Channel, NativeChannel};

let mut out = NativeChannel::with_string_sink();
out.write(&AnyObject::ZoomMap(ZoomMap::new(2, 5.0)?))?;
out.write(&AnyObject::UnitMap(UnitMap::new(3)))?;

let mut input = NativeChannel::from_string(&out.into_string());
while let Some(object) = input.read()? {
    println!("read a {}", object.class_name());
}
# Ok::<(), starlink_ast::error::AstError>(())
```

## Validating Input

The Object returned by `read` ([Reading Objects from a Channel](#reading-objects-from-a-channel)) could be of any class — this is determined entirely by the external data being read.
If it is necessary to test for any kind of Frame, this may be done with [`as_frame`](crate::AnyObject::as_frame), which gives the Object as a Frame if it is one:

```rust
# use starlink_ast::frames::{SkyFrame, SkySystem};
# use starlink_ast::AnyObject;
# let object = AnyObject::SkyFrame(SkyFrame::new(SkySystem::Fk5));
let ok = object.as_frame().is_some();
# assert!(ok);
```

Note, however, that this will accept any Frame, so would be equally happy with a basic Frame or a SkyFrame.
An alternative validation strategy would be to ask for one particular class, by converting the Object to it with `try_from`, which fails for any other class:

```rust
use starlink_ast::frames::BasicFrame;
# use starlink_ast::frames::{SkyFrame, SkySystem};
# use starlink_ast::AnyObject;
# let object = AnyObject::SkyFrame(SkyFrame::new(SkySystem::Fk5));

let ok = BasicFrame::try_from(object).is_ok();
# assert!(!ok);
```

This would only accept a basic Frame and would reject a SkyFrame.
The class of the Object, as a string, is given by [`class_name`](crate::AnyObject::class_name).

## Storing an ID String with an Object

Occasionally, you may want to store a number of Objects and later retrieve them and use each for a different purpose.
If the Objects are of the same class, you cannot use the class to distinguish them when you read them back (*c.f.* [Validating Input](#validating-input)).
Although relying on the order in which they are stored is a possible solution, this becomes complicated if some of the Objects are optional and may not always be present.
It also makes extending your data format in future more difficult.

To help with this, every AST Object has an ID attribute and an Ident attribute, both of which allow you, in effect, to attach a textual identification label to it.
You simply set the ID or Ident attribute before writing the Object:

```rust
use starlink_ast::frames::{Frame, SkyFrame, SkySystem};
use starlink_ast::{AnyObject, Channel, NativeChannel};

# let mut channel = NativeChannel::with_string_sink();
let mut skyframe = SkyFrame::new(SkySystem::Fk5);
skyframe.set_id("Calibration")?;
let nobj = channel.write(&AnyObject::SkyFrame(skyframe))?;
# let text = channel.into_string();
# let mut channel = NativeChannel::from_string(&text);
# let object = channel.read()?.unwrap();
# assert_eq!(object.as_frame().unwrap().id(), "Calibration");
# Ok::<(), starlink_ast::error::AstError>(())
```

You can then test its value after you read the Object back:

```rust
# use starlink_ast::frames::{Frame, SkyFrame, SkySystem};
# use starlink_ast::{AnyObject, Channel, NativeChannel};
# let mut skyframe = SkyFrame::new(SkySystem::Fk5);
# skyframe.set_id("Calibration")?;
# let mut out = NativeChannel::with_string_sink();
# out.write(&AnyObject::SkyFrame(skyframe))?;
# let mut channel = NativeChannel::from_string(&out.into_string());
let object = channel.read()?.expect("an Object");
if object.as_frame().is_some_and(|frame| frame.id() == "Calibration") {
    // <the Calibration Object has been read>
} else {
    // <some other Object has been read>
#   unreachable!();
}
# Ok::<(), starlink_ast::error::AstError>(())
```

The only difference between the ID and Ident attributes is that the ID attribute is unique to a particular Object and is lost if, for example, you make a copy of the Object.
The Ident attribute, on the other hand, is transferred to the new Object when a copy is made.
Consequently, it is safest to set the value of the ID attribute immediately before you perform the write.

## The Textual Output Format

Let us now examine the format of the textual output produced by writing an Object to a basic Channel ([Writing Objects to a Channel](#writing-objects-to-a-channel)).
To give a concrete example, suppose the Object in question is a SkyFrame, written out as follows:

```rust
use starlink_ast::frames::{SkyFrame, SkySystem};
use starlink_ast::{AnyObject, Channel, NativeChannel};

# let mut channel = NativeChannel::with_string_sink();
let mut skyframe = SkyFrame::new(SkySystem::Fk4NoE);
skyframe.set_epoch_year(1958.0);
let nobj = channel.write(&AnyObject::SkyFrame(skyframe))?;
# assert!(channel.into_string().contains("    Epoch = 1958 \t# Besselian epoch of observation\n"));
# Ok::<(), starlink_ast::error::AstError>(())
```

The output should then look like the following:

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

You will notice that this output is designed both for a human reader, in that it is formatted, and also to be read back by a computer in order to reconstruct the SkyFrame.
In fact, this is precisely the way that `show` works ([Displaying Objects](crate::guide::objects#displaying-objects)), this method being roughly equivalent to writing the Object to a new basic Channel.

Some lines of the output start with a "#" comment character, which turns the rest of the line into a comment.
These lines will be ignored when read back in by `read`.
They typically contain default values, or values that can be derived in some way from the other data present, so that they do not actually need to be stored in order to reconstruct the original Object.
They are provided purely for human information.
The same comment character is also used to append explanatory comments to most output lines.

It is not sensible to attempt a complete description of this output format because every class of Object is potentially different and each can define how its own data should be represented.
However, there are some basic rules, which mean that the following common features will usually be present:

1. Each Object is delimited by matching "Begin" and "End" lines, which also identify the class of Object involved.
2. Within each Object description, data values are represented by a simple "keyword = value" syntax, with one value to a line.
3. Lines beginning "IsA" are used to mark the divisions between data belonging to different levels in the class hierarchy.
   Thus, "IsA Frame" marks the end of data associated with the Frame class and the start of data associated with some more specialized class (a SkyFrame in the above example).
   "IsA" lines may be omitted if associated data values are absent and no confusion arises.
4. Objects may contain other Objects as data.
   This is indicated by an absent value, with the description of the data Object following on subsequent lines.
5. Indentation is used to clarify the overall structure.

Beyond these general principles, the best guide to what a particular line of output represents will generally be the comment which accompanies it together with a general knowledge of the class of Object being described.

## Controlling the Amount of Output

It is not always necessary for the output from `write` ([Writing Objects to a Channel](#writing-objects-to-a-channel)) to be human-readable, so a Channel has attributes that allow the amount of detail in the output to be controlled.
The attributes every Channel has are held in its [`ChannelConfig`](crate::ChannelConfig), reached through [`config_mut`](crate::Channel::config_mut).

The first of these is the integer attribute Full ([`set_full`](crate::ChannelConfig::set_full)), which controls the extent to which optional, commented out, output lines are produced.
By default, Full is zero, and this results in the standard style of output ([The Textual Output Format](#the-textual-output-format)) where default values that may be helpful to humans are included.
To suppress these optional lines, Full should be set to −1:

```rust
# use starlink_ast::frames::{SkyFrame, SkySystem};
# use starlink_ast::{AnyObject, Channel, NativeChannel};
# let mut skyframe = SkyFrame::new(SkySystem::Fk4NoE);
# skyframe.set_epoch_year(1958.0);
# let mut channel = NativeChannel::with_string_sink();
channel.config_mut().set_full(-1);
channel.write(&AnyObject::SkyFrame(skyframe))?;
# assert!(!channel.into_string().contains("#   "));
# Ok::<(), starlink_ast::error::AstError>(())
```

This would result in output containing only the essential information, such as:

```text
 Begin SkyFrame 	# Description of celestial coordinate system
    Naxes = 2 	# Number of coordinate axes
    Epoch = 1958 	# Besselian epoch of observation
    System = "FK4-NO-E" 	# Coordinate system type
    Ax1 = 	# Axis number 1
       Begin SkyAxis 	# Celestial coordinate axis
       End SkyAxis
    Ax2 = 	# Axis number 2
       Begin SkyAxis 	# Celestial coordinate axis
       End SkyAxis
 IsA Frame 	# Coordinate system description
 End SkyFrame
```

In contrast, setting Full to +1 will result in additional output lines which will reveal every last detail of the Object's construction.
Often this will be rather more than you want, especially for more complex Objects, but it can sometimes help when debugging programs.
At this level of detail, a SkyFrame's description also lists, as comments, every attribute it shares with all Mappings and Frames, and every attribute of each of its axes.

## Controlling Commenting

Another way of controlling output from a Channel is *via* the boolean Comment attribute ([`set_comment`](crate::ChannelConfig::set_comment)), which controls whether comments are appended to describe the purpose of each value.
Comment is `true` by default but, if set to `false`, will suppress these comments.
This is normally appropriate only if you wish to minimize the amount of output, for example:

```rust
# use starlink_ast::frames::{SkyFrame, SkySystem};
# use starlink_ast::{AnyObject, Channel, NativeChannel};
# let mut skyframe = SkyFrame::new(SkySystem::Fk4NoE);
# skyframe.set_epoch_year(1958.0);
# let mut channel = NativeChannel::with_string_sink();
channel.config_mut().set_full(-1);
channel.config_mut().set_comment(false);
let nobj = channel.write(&AnyObject::SkyFrame(skyframe))?;
# assert!(!channel.into_string().contains('#'));
# Ok::<(), starlink_ast::error::AstError>(())
```

might result in the following more compact output:

```text
 Begin SkyFrame
    Naxes = 2
    Epoch = 1958
    System = "FK4-NO-E"
    Ax1 =
       Begin SkyAxis
       End SkyAxis
    Ax2 =
       Begin SkyAxis
       End SkyAxis
 IsA Frame
 End SkyFrame
```

## Editing Textual Output

The safest advice about editing the textual output from `write` (or `show`) is "don't!" — unless you know what you are doing.

Having given that warning, however, it is sometimes possible to make changes to the text, or even to write entire Object descriptions from scratch, and to read the results back in to construct new Objects.
Normally, simple changes to numerical values are safest, but be aware that this is a back door method of creating Objects, so you are on your own!
There are a number of potential pitfalls.
In particular:

- `read` is intended for retrieving data written by `write` and not for reading data input by humans.
  As such, the data validation provided is very limited and is certainly not foolproof.
  This makes it quite easy to construct Objects that are internally inconsistent by this means.
  In contrast, the normal programming interface incorporates numerous checks designed to make it impossible to construct invalid Objects.
  You should not necessarily think you have found a bug if your changes to an Object's textual description fail to produce the results you expected!
- In many instances the names associated with values in textual output will correspond with Object attributes.
  Sometimes, however, these names may differ from the attribute name.
  This is mainly because of length restrictions imposed by other common external formats, such as FITS headers.
  Some of the names used do not correspond with attributes at all.
- It is safest to change single numerical or string values.
  Beware of changing the size or shape of Objects (*e.g.* the number of axes in a Frame).
  Often, these values must match others stored elsewhere within the Object and changing them in a haphazard fashion will not produce useful results.
- Be wary about un-commenting default values.
  Sometimes this will work, but often these values are derived from other Objects stored more deeply in the structure and the proper place to insert a new value is not where the default itself appears.

## Mixing Objects with other Text

By default, when you use `read` to read from a basic Channel ([Reading Objects from a Channel](#reading-objects-from-a-channel)), it is assumed that you are reading a stream of text containing only AST Objects, which follow each other end-to-end.
If any extraneous input data are encountered which do not appear to form part of the textual description of an Object, then an error will result.
In particular, the first input line must identify the start of an Object description, so you cannot start reading half way through an Object.

Sometimes, however, you may want to store AST Object descriptions intermixed with other textual data.
You can do this by setting the Channel's boolean Skip attribute ([`set_skip`](crate::ChannelConfig::set_skip)) to `true`.
This will cause every read to skip over extraneous data until the start of a new AST Object description, if any, is found.
So long as your other data do not mimic the appearance of an AST Object description, the two sets of data can co-exist.

For example, by setting Skip to `true`, the following complete program will read all the AST Objects whose descriptions appear in a text file, ignoring the other text.
`show` is used to display those found:

```rust,no_run
// no_run: reads its input from a file named on the command line.
use starlink_ast::{Channel, NativeChannel};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("give a file name")?;
    let mut channel = NativeChannel::from_file(Path::new(&path))?;
    channel.config_mut().set_skip(true);
    while let Some(object) = channel.read()? {
        object.show()?;
    }
    Ok(())
}
```

```rust
# use starlink_ast::{Channel, NativeChannel};
let text = "Some other text\n Begin UnitMap\n    Nin = 2\n End UnitMap\nmore text\n";

let mut channel = NativeChannel::from_string(text);
assert!(channel.read().is_err());

let mut channel = NativeChannel::from_string(text);
channel.config_mut().set_skip(true);
assert_eq!(channel.read()?.expect("one Object").class_name(), "UnitMap");
assert!(channel.read()?.is_none());
# Ok::<(), starlink_ast::error::AstError>(())
```

## Reading Objects from Files

Thus far, we have only considered the default behavior of a Channel in reading and writing Objects through a program's standard input and output streams.
We will now consider how to access Objects stored in files more directly.

The simple approach is to use [`NativeChannel::from_file`](crate::NativeChannel::from_file), which makes a Channel whose source is a file.
For instance, the following will read a pair of Objects from a text file called "fred.txt":

```rust,no_run
// no_run: reads a file that the example does not create.
use starlink_ast::{Channel, NativeChannel};
use std::path::Path;

let mut channel = NativeChannel::from_file(Path::new("fred.txt"))?;
let obj1 = channel.read()?;
let obj2 = channel.read()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The file is closed when the Channel is dropped.
Alternatively, the SourceFile attribute ([`set_source_file`](crate::NativeChannel::set_source_file)) names a file to read from in place of the Channel's source; clearing it closes the file.

More generally, a Channel reads its text from a *source*, any value that implements the [`Source`](crate::Source) trait, given to it with [`set_source`](crate::NativeChannel::set_source).
A source has one method, which returns the next line of text, or `None` when there is no more input.
The library provides [`FileSource`](crate::FileSource) and [`StringSource`](crate::StringSource), but you may write your own to read from anywhere text can be kept.
Consider the following simple source, which reads lines from any buffered reader:

```rust
use starlink_ast::Source;
use std::io::BufRead;

struct LineSource<R: BufRead + Send>(R);

impl<R: BufRead + Send> Source for LineSource<R> {
    fn read_line(&mut self) -> Option<String> {
        let mut line = String::new();
        match self.0.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_string()),
        }
    }
}
# use starlink_ast::{Channel, NativeChannel};
# let text = " Begin UnitMap\n    Nin = 2\n End UnitMap\n";
# let mut channel = NativeChannel::new();
# channel.set_source(Box::new(LineSource(std::io::Cursor::new(text))));
# assert!(channel.read()?.is_some());
# Ok::<(), starlink_ast::error::AstError>(())
```

We give the Channel a `LineSource` before reading an Object from it:

```rust,no_run
// no_run: reads a file that the example does not create.
# use starlink_ast::Source;
# use std::io::BufRead;
# struct LineSource<R: BufRead + Send>(R);
# impl<R: BufRead + Send> Source for LineSource<R> {
#     fn read_line(&mut self) -> Option<String> { None }
# }
use starlink_ast::{Channel, NativeChannel};
use std::fs::File;
use std::io::BufReader;

let input = BufReader::new(File::open("infile.ast")?);
let mut channel = NativeChannel::new();
channel.set_source(Box::new(LineSource(input)));
let object = channel.read()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

When we read an Object from this Channel with `read`, the source will be called to obtain the textual data from the file, the end-of-file being detected when it returns `None`.

Note, if a value is set for the SourceFile attribute, `read` will ignore the Channel's source.

## Writing Objects to Files

As for reading, writing Objects to files can be done in two different ways.
Again, the simple approach is to use [`NativeChannel::to_file`](crate::NativeChannel::to_file), which makes a Channel whose sink is a file.
For instance, the following will write a pair of Objects to a text file called "fred.txt":

```rust,no_run
// no_run: writes a file.
use starlink_ast::mappings::{UnitMap, ZoomMap};
use starlink_ast::{AnyObject, Channel, NativeChannel};
use std::path::Path;

let mut channel = NativeChannel::to_file(Path::new("fred.txt"))?;
let nobj = channel.write(&AnyObject::ZoomMap(ZoomMap::new(2, 5.0)?))?;
let nobj = channel.write(&AnyObject::UnitMap(UnitMap::new(2)))?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The file is closed when the Channel is dropped.
Alternatively, the SinkFile attribute ([`set_sink_file`](crate::NativeChannel::set_sink_file)) names a file to write to in place of the Channel's sink; clearing it closes the file.

More generally, a Channel writes its text to a *sink*, any value that implements the [`Sink`](crate::Sink) trait, given to it with [`set_sink`](crate::NativeChannel::set_sink).
A sink has one method, which is given each line of output text in turn, without a newline:

```rust
use starlink_ast::Sink;
use std::io::Write;

struct LineSink<W: Write + Send>(W);

impl<W: Write + Send> Sink for LineSink<W> {
    fn write_line(&mut self, line: &str) {
        let _ = writeln!(self.0, "{line}");
    }
}
# use starlink_ast::{AnyObject, Channel, NativeChannel};
# use starlink_ast::mappings::UnitMap;
# let mut channel = NativeChannel::new();
# channel.set_sink(Box::new(LineSink(Vec::new())));
# channel.write(&AnyObject::UnitMap(UnitMap::new(2)))?;
# Ok::<(), starlink_ast::error::AstError>(())
```

Note that we must supply the final newline character ourselves.
The library provides [`FileSink`](crate::FileSink) and [`StringSink`](crate::StringSink).

Note that we can give a Channel a source and/or a sink, and that these may use either the same file, or different files according to whether we are reading or writing.
AST has no knowledge of the underlying file system, nor of file positioning.
It just reads and writes sequentially.
If you wish, for example, to reposition a file at the beginning in between reads and writes, then this can be done directly (and completely independently of AST).

Note, if a value is set for the SinkFile attribute, `write` will ignore the Channel's sink.

## Reading and Writing Objects to other Places

It should be obvious from the above ([Reading Objects from Files](#reading-objects-from-files) and [Writing Objects to Files](#writing-objects-to-files)) that a Channel's source and sink provide a flexible means of intercepting textual data that describes AST Objects as it flows in and out of your program.
In fact, you might like to regard a Channel simply as a filter for converting AST Objects to and from a stream of text which is then handled by your source and sink, where the real I/O occurs.

This gives you the ability to store AST Objects in virtually any data system, so long as you can convert a stream of text into something that can be stored (it need no longer be text) and retrieve it again.
There is generally no need to retain comments.
Other possibilities, such as inter-process and network communication, could also be implemented *via* sources and sinks in basically the same way.

For text held in memory, [`NativeChannel::from_string`](crate::NativeChannel::from_string) and [`NativeChannel::with_string_sink`](crate::NativeChannel::with_string_sink) read from and write to a string, and [`from_native_string`](crate::from_native_string) reads one Object from a string directly.
