# Storing AST Objects in FITS Headers (FitsChans)

A FITS header is a sequence of 80-character strings, formatted according to particular rules defined by the Flexible Image Transport System ([FITS](http://fits.gsfc.nasa.gov/)).
FITS is a widely-used standard for data interchange in astronomy and has also been adopted as a data processing format in some astronomical data reduction systems.
The individual 80-character strings in a FITS header are usually called *cards* or *header cards* (for entirely anachronistic reasons).

A sequence of FITS cards appears as a header at the start of every FITS data file, and sometimes also at other points within it, and is used to provide ancillary information which qualifies or describes the main array of data stored in the file.
As such, FITS headers are prime territory for storing information about the coordinate systems associated with data held in FITS files.

In this section, we will examine how to store information in FITS headers directly in the form of AST Objects — a process which is supported by a specialized class of Channel called a [`FitsChan`](crate::FitsChan).
Our discussion here will turn out to be a transitional step that emphasizes the similarities between a FitsChan and a Channel ([Saving and Restoring Objects](crate::guide::channels)).
At the same time, it will prepare us for the next section (*Using Foreign FITS Encodings*<!-- link: crate::guide::foreign_fits -->), where we will examine how to use a FitsChan to tackle some of the more difficult problems that FITS headers can present.

## The Native FITS Encoding

As it turns out, we are not the first to have thought of storing WCS information in FITS headers.
In fact, the original FITS standard (1981 vintage) defined a set of header keywords for this purpose which have been widely used, although they have proved too limited for many practical purposes.

At the time of writing, a number of different ways of using FITS headers for storing WCS information are in use, most (although not all) based on the original standard.
We will refer to these alternative ways of storing the information as FITS *encodings* but will defer a discussion of their advantages and limitations until the next section (*Using Foreign FITS Encodings*<!-- link: crate::guide::foreign_fits -->).

Here, we will examine how to store AST Objects directly in FITS headers.
In effect, this defines a new encoding, which we will term the *native encoding*.
This is a special kind of encoding, because not only does it allow us to associate conventional WCS calibration information with FITS data, but it also allows any other information that can be expressed in terms of AST Objects to be stored as well.
In fact, the native encoding provides us with facilities roughly analogous to those of the Channel ([Saving and Restoring Objects](crate::guide::channels)) — *i.e.* a lossless way of transferring AST Objects from program to program — but based on FITS headers instead of free-format text.

## The FitsChan Model

I/O between AST Objects and FITS headers is supported by a specialized form of Channel called a FitsChan.
A FitsChan contains a buffer which may hold any number, including zero, of FITS header cards.
This buffer forms a workspace in which you can assemble FITS cards and manipulate them before writing them out to a file.

By default, when a FitsChan is first created, it contains no cards and there are five ways of inserting cards into it:

1. You may add cards yourself, one at a time, using [`put_fits`](crate::FitsChan::put_fits) ([Adding Individual Cards to a FitsChan](#adding-individual-cards-to-a-fitschan)).
2. You may add cards yourself, supplying all cards concatenated into a single string, using [`put_cards`](crate::FitsChan::put_cards) ([Adding Concatenated Cards to a FitsChan](#adding-concatenated-cards-to-a-fitschan)).
3. You may write an AST Object to the FitsChan (using [`write`](crate::Channel::write)), which will have the effect of creating new cards within the FitsChan which describe the Object ([Writing Native Objects to a FitsChan](#writing-native-objects-to-a-fitschan)).
4. You may assign a value to the SourceFile attribute of the FitsChan ([`set_source_file`](crate::FitsChan::set_source_file)).
   The value should be the path to a text file holding a set of FITS header cards, one per line.
   When the SourceFile value is set, the file is opened and the headers copied from it into the FitsChan.
   The file is then immediately closed.
5. You may give the FitsChan a source which reads data from some external store of FITS cards, just like the source associated with a basic Channel ([Reading Objects from Files](crate::guide::channels#reading-objects-from-files)).
   If you supply a source, it will be called the first time the FitsChan is used in order to fill it with an initial set of cards ([Sources and Sinks for FitsChans](#sources-and-sinks-for-fitschans)).

There are also four ways of removing cards from a FitsChan:

1. You may delete cards yourself, one at a time, using [`del_fits`](crate::FitsChan::del_fits) ([Finding and Changing Cards in a FitsChan](#finding-and-changing-cards-in-a-fitschan)).
2. You may read an AST Object from the FitsChan (using [`read`](crate::Channel::read)), which will have the effect of removing those cards from the FitsChan which describe the Object ([Reading Native Objects From a FitsChan](#reading-native-objects-from-a-fitschan)).
3. You may assign a value to the FitsChan's SinkFile attribute.
   When the FitsChan is dropped, any remaining headers are written out to a text file with path equal to the value of the SinkFile attribute.
4. Alternatively, you may give the FitsChan a sink which writes data to some external store of FITS cards, just like the sink associated with a basic Channel ([Writing Objects to Files](crate::guide::channels#writing-objects-to-files)).
   If you supply a sink, it will be called when the FitsChan is dropped in order to write out any FITS cards that remain in it ([Sources and Sinks for FitsChans](#sources-and-sinks-for-fitschans)).
   Note, the sink is not called if the SinkFile attribute has been set.

Note, in particular, that reading an AST Object from a FitsChan is *destructive*.
That is, it deletes the FITS cards that describe the Object.
The reason for this is explained in *Removing WCS Information from FITS Headers — the Destructive Read*<!-- link: crate::guide::foreign_fits#removing-wcs-information-from-fits-headers--the-destructive-read -->.

In addition to the above, you may also read individual cards from a FitsChan using [`find_fits`](crate::FitsChan::find_fits) (which is not destructive).
This is the main means of writing out FITS cards if you have not supplied a sink.
`find_fits` also provides a means of searching for particular FITS cards (by keyword, for example) and there are other facilities for overwriting cards when required ([Finding and Changing Cards in a FitsChan](#finding-and-changing-cards-in-a-fitschan)).

## Creating a FitsChan

Creating a FitsChan is straightforward:

```rust
use ast_core::{Encoding, FitsChan};

let mut fitschan = FitsChan::new()?;
fitschan.set_encoding(Encoding::Native);
# Ok::<(), ast_core::error::AstError>(())
```

We have also set the FitsChan's Encoding attribute ([`set_encoding`](crate::FitsChan::set_encoding)) to [`Encoding::Native`](crate::Encoding::Native).
This indicates that we will be using the native encoding ([The Native FITS Encoding](#the-native-fits-encoding)) to store and retrieve Objects.
If this was left unspecified, the default would depend on the FitsChan's contents.
An attempt is made to use whatever encoding appears to have been used previously.
For an empty FitsChan, the default is NATIVE, but it does no harm to be sure.

## Addressing Cards in a FitsChan

Because a FitsChan contains an ordered sequence of header cards, a mechanism is needed for addressing them.
This allows you to specify where new cards are to be added, for example, or which card is to be deleted.

This role is filled by the FitsChan's integer Card attribute, which gives the index of the *current card* in the FitsChan.
You can nominate any card you like to be current, simply by setting a new value for the Card attribute with [`set_card`](crate::FitsChan::set_card), for example:

```rust
# use ast_core::FitsChan;
# let mut fitschan = FitsChan::new()?;
# fitschan.put_fits("AIRMASS =                  1.2", false)?;
# let icard = 1;
fitschan.set_card(icard)?;
# Ok::<(), ast_core::error::AstError>(())
```

where `icard` contains the index of the card on which you wish to operate next.
Some methods will update the Card attribute as a means of advancing through the sequence of cards, when reading them for example, or to indicate which card matches a search criterion.

The default value for Card is one, which is the index of the first card.
This means that you can "rewind" a FitsChan to access its first card by clearing the Card attribute:

```rust
# use ast_core::FitsChan;
# let mut fitschan = FitsChan::new()?;
fitschan.clear_card()?;
# Ok::<(), ast_core::error::AstError>(())
```

The total number of cards in a FitsChan is given by the integer Ncard attribute ([`ncard`](crate::FitsChan::ncard)).
This is a read-only attribute whose value is automatically updated as you add or remove cards.
It means you can address all the cards in sequence using a loop such as the following:

```rust
# use ast_core::FitsChan;
# let mut fitschan = FitsChan::new()?;
# fitschan.put_fits("AIRMASS =                  1.2", false)?;
let ncard = fitschan.ncard()?;
for icard in 1..=ncard {
    fitschan.set_card(icard as i64)?;
    // <access the current card>
}
# Ok::<(), ast_core::error::AstError>(())
```

However, it is usually possible to write slightly tidier loops based on `find_fits`, described later ([Extracting Individual Cards from a FitsChan](#extracting-individual-cards-from-a-fitschan) and [Finding and Changing Cards in a FitsChan](#finding-and-changing-cards-in-a-fitschan)).

If you set the Card attribute to a value larger than Ncard, the FitsChan is regarded as being positioned at its *end-of-file*.
In this case there is no current card and an attempt to obtain a value for the Card attribute will always return the value Ncard + 1.
When a FitsChan is empty, it is always at the end-of-file.

## Writing Native Objects to a FitsChan

Having created an empty FitsChan ([Creating a FitsChan](#creating-a-fitschan)), you can write any AST Object to it in the native encoding using `write`.
Let us assume we are writing a SkyFrame (more probably, you would want to write a FrameSet, but for purposes of illustration a SkyFrame contains a more manageable amount of data), as follows:

```rust
use ast_core::frames::{SkyFrame, SkySystem};
use ast_core::{AnyObject, Channel, Encoding, FitsChan};

# let mut fitschan = FitsChan::new()?;
# fitschan.set_encoding(Encoding::Native);
let mut skyframe = SkyFrame::new(SkySystem::Fk4NoE);
skyframe.set_epoch_year(1958.0);
let nobj = fitschan.write(&AnyObject::SkyFrame(skyframe))?;
# assert_eq!(nobj, 1);
# assert_eq!(fitschan.card()?, fitschan.ncard()? + 1);
# Ok::<(), ast_core::error::AstError>(())
```

Since we have selected the native encoding ([The Native FITS Encoding](#the-native-fits-encoding)), there are no restrictions on the class of Object we may write, so `write` should always return a value of one, unless an error occurs.
Unlike a basic Channel ([Writing Objects to a Channel](crate::guide::channels#writing-objects-to-a-channel)), this write operation will not produce any output from our program.
The FITS headers produced are simply stored inside the FitsChan.

After this write operation, the Ncard attribute will be updated to reflect the number of new cards added to the FitsChan and the Card attribute will point at the card immediately after the last one written.
Since our FitsChan was initially empty, the Card attribute will, in this example, point at the end-of-file ([Addressing Cards in a FitsChan](#addressing-cards-in-a-fitschan)).

The FITS standard imposes a limit of 68 characters on the length of strings which may be stored in a single header card.
Sometimes, a description of an AST Object involves the use of strings which exceed this limit (*e.g.* a Frame title can be of arbitrary length).
If this occurs, the long string will be split over two or more header cards.
Each "continuation" card will have the keyword `CONTINUE` in columns 1 to 8, and will contain a space in column 9 (instead of the usual equals sign).
An ampersand ("&") is appended to the end of each of the strings (except the last one) to indicate that the string is continued on the next card.

Note, this splitting of long strings over several cards only occurs when writing AST Objects to a FitsChan using `write` and the *native* encoding.
If a long string is stored in a FitsChan using (for instance) `put_fits` or `put_cards`, it will simply be truncated.

## Extracting Individual Cards from a FitsChan

To examine the contents of the FitsChan after writing the SkyFrame above ([Writing Native Objects to a FitsChan](#writing-native-objects-to-a-fitschan)), we must write a simple loop to extract each card in turn and print it out.
We must also remember to rewind the FitsChan first, *e.g.* using [`clear_card`](crate::FitsChan::clear_card).
The following loop would do:

```rust
# use ast_core::FitsChan;
# let mut fitschan = FitsChan::new()?;
fitschan.clear_card()?;
while let Some(card) = fitschan.find_fits("%f", true)? {
    println!("{card}");
}
# Ok::<(), ast_core::error::AstError>(())
```

Here, we have used `find_fits` to find a FITS card by keyword.
It is given a keyword template of "%f", which matches any FITS keyword, so it always finds the current card, which it returns.
Its second argument is `true`, to indicate that the Card attribute should be incremented afterwards so that the following card will be found the next time around the loop.
`find_fits` returns `None` when it reaches the end-of-file and this terminates the loop.

If we were storing the FITS headers in an output FITS file instead of printing them out, we might use a loop like this but replace `println!` with a suitable data storage operation.
This would only be necessary if we had not provided a sink for the FitsChan ([Sources and Sinks for FitsChans](#sources-and-sinks-for-fitschans)).
[`to_header`](crate::FitsChan::to_header) returns all the cards as one string, one card per line.

## The Native FitsChan Output Format

If we print out the FITS header cards describing the SkyFrame we wrote earlier ([Writing Native Objects to a FitsChan](#writing-native-objects-to-a-fitschan)), we should obtain something like the following:

```text

COMMENT AST ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++ AST
COMMENT AST            Beginning of AST data for SkyFrame object             AST
COMMENT AST ................................................................ AST
BEGAST_A= 'SkyFrame'           / Description of celestial coordinate system
NAXES_A =                    2 / Number of coordinate axes
EPOCH_A =               1958.0 / Besselian epoch of observation
SYSTEM_A= 'FK4-NO-E'           / Coordinate system type
AX1_A   = '        '           / Axis number 1
BEGAST_B= 'SkyAxis '           / Celestial coordinate axis
ENDAST_A= 'SkyAxis '           / End of object definition
AX2_A   = '        '           / Axis number 2
BEGAST_C= 'SkyAxis '           / Celestial coordinate axis
ENDAST_B= 'SkyAxis '           / End of object definition
ISA_A   = 'Frame   '           / Coordinate system description
ENDAST_C= 'SkyFrame'           / End of object definition
COMMENT AST ................................................................ AST
COMMENT AST               End of AST data for SkyFrame object                AST
COMMENT AST ---------------------------------------------------------------- AST
```

As you can see, this resembles the information that would be written to a basic Channel to describe the same SkyFrame ([The Textual Output Format](crate::guide::channels#the-textual-output-format)), except that it has been formatted into 80-character header cards according to FITS conventions.
The blank card at the start separates the AST data from any cards before it.

There are also a number of other differences worth noting:

1. There is no unnecessary information about default values provided for the benefit of the human reader.
   This is because the Full attribute for a FitsChan defaults to −1, thus suppressing this information (*c.f.* [Controlling the Amount of Output](crate::guide::channels#controlling-the-amount-of-output)).
   You can restore the information if you wish by setting Full to 0 or +1, in which case additional COMMENT cards will be generated to hold it.
2. The information is not indented, because FITS does not allow this.
   However, if you change the Full attribute to 0 or +1, comments will be included that are intended to help break up the sequence of headers and highlight its structure.
   This will probably only be of use if you are attempting to track down a problem by examining the FITS cards produced in detail.
3. The FITS keywords which appear to the left of the "=" signs have additional characters ("_A", "_B", *etc.*) appended to them.
   This is done in order to make each keyword unique.

This last point is worth further comment and is necessary because the FITS standard only allows for certain keywords (such as COMMENT and HISTORY) to appear more than once.
`write` therefore appends an arbitrary sequence of two characters to each new keyword it generates in order to ensure that it does not duplicate any already present in the FitsChan.

The main risk from not following this convention is that some software might ignore (say) all but the last occurrence of a keyword before passing the FITS headers on.
Such an event is unlikely, but would obviously destroy the information present, so `write` enforces the uniqueness of the keywords it uses.
The extra characters added are ignored when the information is read back.

As with a basic Channel, you can also suppress the comments produced in a FitsChan by setting the boolean Comment attribute to `false` ([Controlling Commenting](crate::guide::channels#controlling-commenting)).
However, FITS headers are traditionally generously commented, so this is not recommended.

## Adding Individual Cards to a FitsChan

To insert individual cards into a FitsChan, prior to reading them back as Objects for example, you should use [`put_fits`](crate::FitsChan::put_fits).
You can insert a card in front of the current one as follows:

```rust
# use ast_core::FitsChan;
# let mut fitschan = FitsChan::new()?;
# let card = "AIRMASS =                  1.2";
fitschan.put_fits(card, false)?;
# Ok::<(), ast_core::error::AstError>(())
```

where the second argument of `false` indicates that the current card should not be overwritten.
Note that facilities are not provided by AST for formatting the card contents.

After inserting a card, the FitsChan's Card attribute points at the original Card, or at the end-of-file if the FitsChan was originally empty.
Entering a sequence of cards is therefore straightforward.
If `cards` is a list of strings containing FITS header cards, then a loop such as the following will insert the cards in sequence into a FitsChan:

```rust
# use ast_core::FitsChan;
# let mut fitschan = FitsChan::new()?;
# let cards = ["NAXIS   =                    2", "NAXIS1  =                  100"];
for card in cards {
    fitschan.put_fits(card, false)?;
}
# assert_eq!(fitschan.ncard()?, 2);
# Ok::<(), ast_core::error::AstError>(())
```

Note that `put_fits` enforces the validity of a FitsChan by rejecting any cards which do not adhere to the FITS standard.
If any such cards are detected, an error will result.

## Adding Concatenated Cards to a FitsChan

If you have all your cards concatenated together into a single long string, each occupying 80 characters (with no delimiters), you can insert them into a FitsChan in a single call using [`put_cards`](crate::FitsChan::put_cards).
This call first empties the FitsChan of any existing cards, then inserts the new cards, and finally rewinds the FitsChan so that a subsequent call to `read` will start reading from the first supplied card.
`put_cards` uses `put_fits` internally to interpret and store each individual card, and so the caveats in [Adding Individual Cards to a FitsChan](#adding-individual-cards-to-a-fitschan) should be read.

For instance, if you are using the CFITSIO library for access to FITS files, you can use the CFITSIO `fits_hdr2str` function to obtain a string suitable for passing to `put_cards`:

```rust
use ast_core::{Channel, FitsChan};

# let header: String = ["NAXIS   =                    2"].iter().map(|c| format!("{c:80}")).collect();
let mut fitschan = FitsChan::new()?;
fitschan.put_cards(&header)?;
let wcsinfo = fitschan.read()?;
# Ok::<(), ast_core::error::AstError>(())
```

A header held as text, one card to a line, can be read with [`FitsChan::from_header`](crate::FitsChan::from_header).

## Reading Native Objects From a FitsChan

Once you have stored a FITS header description of an Object in a FitsChan using the native encoding ([Writing Native Objects to a FitsChan](#writing-native-objects-to-a-fitschan)), you can read it back using `read` in much the same way as with a basic Channel ([Reading Objects from a Channel](crate::guide::channels#reading-objects-from-a-channel)).
Similar comments about validating the Object you read also apply ([Validating Input](crate::guide::channels#validating-input)).
If you have just written to the FitsChan, you must remember to rewind it first:

```rust
# use ast_core::frames::{SkyFrame, SkySystem};
# use ast_core::{AnyObject, Channel, Encoding, FitsChan};
# let mut fitschan = FitsChan::new()?;
# fitschan.set_encoding(Encoding::Native);
# fitschan.write(&AnyObject::SkyFrame(SkyFrame::new(SkySystem::Fk5)))?;
# let before = fitschan.ncard()?;
fitschan.clear_card()?;
let object = fitschan.read()?;
# assert_eq!(object.unwrap().class_name(), "SkyFrame");
# assert!(fitschan.ncard()? < before);
# Ok::<(), ast_core::error::AstError>(())
```

An important feature of a FitsChan is that read operations are destructive.
This means that if an Object description is found, it will be consumed by `read` which will remove all the cards involved, including associated COMMENT cards, from the FitsChan.
Thus, if you write an Object to a FitsChan, rewind, and read the same Object back, you should end up with the original FitsChan contents.
If you need to circumvent this behavior for any reason, it is a simple matter to make a copy of a FitsChan using `clone` ([Copying Objects](crate::guide::objects#copying-objects)).
If you then read from the copy, the original FitsChan will remain untouched.

After a read completes, the FitsChan's Card attribute identifies the card immediately following the last card read, or the end-of-file if there are no more cards.

Since the *native* encoding is being used, any long strings involved in the object description will have been split into two or more adjacent continuation cards when the Object was stored in the header using `write`.
`read` reverses this process by concatenating any such adjacent continuation cards to re-create the original long string.

## Saving and Restoring Multiple Objects in a FitsChan

When using the native FITS encoding, multiple Objects may be stored and all I/O operations are sequential.
This means that you can simply write a sequence of Objects to a FitsChan.
After each write operation, the Card attribute will be updated so that the next write appends the next Object description to the previous one.

If you then rewind the FitsChan, you can read the Objects back in the original order.
Reading them back will, of course, remove their descriptions from the FitsChan ([Reading Native Objects From a FitsChan](#reading-native-objects-from-a-fitschan)) but the behavior of the Card attribute is such that successive reads will simply return each Object in sequence.

The only thing that may require care, given that a FitsChan can always be addressed randomly by setting its Card attribute, is to avoid writing one Object on top of another.
For obvious reasons, the Object descriptions in a FitsChan must remain separate if they are to make sense when read back.

## Mixing Native Objects with Other FITS Cards

Of course, any real FITS header will contain other information besides AST Objects, if only the mandatory FITS cards that must accompany all FITS data.
When FITS headers are read in from a real dataset, therefore, any native AST Object descriptions will be inter-mixed with many other cards.

Because this is the normal state of affairs, the boolean Skip attribute for a FitsChan defaults to `true`.
This means that when you read an Object from a FitsChan, any irrelevant cards will simply be skipped over until the start of the next Object description, if any, is found.
If you start reading part way through an Object description, no error will result.
The remainder of the description will simply be skipped.

Setting Skip to `false` will change this behavior to resemble that of a basic Channel ([Mixing Objects with other Text](crate::guide::channels#mixing-objects-with-other-text)), where extraneous data are not permitted by default, but this will probably rarely be useful.

## Finding and Changing Cards in a FitsChan

You can search for, and retrieve, particular cards in a FitsChan by keyword, using [`find_fits`](crate::FitsChan::find_fits).
This performs a search, starting at the current card, until it finds a card whose keyword matches the template you supply, or the end-of-file is reached.

If a suitable card is found, `find_fits` returns the card's contents and then sets the FitsChan's Card attribute either to identify the card found, or the one following it.
The way you want the Card attribute to be set is indicated by the final boolean argument to `find_fits`.
If a suitable card cannot be found, `find_fits` returns `None` and sets the FitsChan's Card attribute to the end-of-file.

Requesting that the Card attribute be set to indicate the card that `find_fits` finds is useful if you want to replace that card with a new one, as in this example:

```rust
# use ast_core::FitsChan;
# let mut fitschan = FitsChan::new()?;
# fitschan.put_fits("AIRMASS =                  1.2", false)?;
# fitschan.clear_card()?;
# let newcard = "AIRMASS =                  1.5";
fitschan.find_fits("AIRMASS", false)?;
fitschan.put_fits(newcard, true)?;
# fitschan.clear_card()?;
# assert!(fitschan.find_fits("AIRMASS", false)?.unwrap().contains("1.5"));
# assert_eq!(fitschan.ncard()?, 1);
# Ok::<(), ast_core::error::AstError>(())
```

Here, `find_fits` is used to search for a card with the keyword AIRMASS, and we do not use the card's contents.
If the card is found, `put_fits` then overwrites it with a new card.
Otherwise, the Card attribute ends up pointing at the end-of-file and the new card is simply appended to the end of the FitsChan.

A similar approach can be used to delete selected cards from a FitsChan using [`del_fits`](crate::FitsChan::del_fits), which deletes the current card:

```rust
# use ast_core::FitsChan;
# let mut fitschan = FitsChan::new()?;
# fitschan.put_fits("BSCALE  =                  1.0", false)?;
# fitschan.clear_card()?;
if fitschan.find_fits("BSCALE", false)?.is_some() {
    fitschan.del_fits()?;
}
# assert_eq!(fitschan.ncard()?, 0);
# Ok::<(), ast_core::error::AstError>(())
```

This deletes the first card, if any, with the BSCALE keyword.

Requesting that `find_fits` increments the Card attribute to identify the card following the one found is more useful when writing loops.
For example, the following loop extracts each card whose keyword matches the template "CD%6d" (that is, "CD" followed by six decimal digits):

```rust
# use ast_core::FitsChan;
# let mut fitschan = FitsChan::new()?;
# fitschan.put_fits("CD000001=                  3.0", false)?;
# fitschan.put_fits("CD1_1   =                  2.0", false)?;
# fitschan.clear_card()?;
# let mut found = 0;
while let Some(card) = fitschan.find_fits("CD%6d", true)? {
    // <process the card's contents>
#   found += 1;
}
# assert_eq!(found, 1);
# Ok::<(), ast_core::error::AstError>(())
```

For further details of keyword templates, see the description of [`find_fits`](crate::FitsChan::find_fits).

## Sources and Sinks for FitsChans

The use of a source and sink with a FitsChan is optional.
This is because you can always arrange to explicitly fill a FitsChan with FITS cards ([Adding Individual Cards to a FitsChan](#adding-individual-cards-to-a-fitschan) and [Adding Concatenated Cards to a FitsChan](#adding-concatenated-cards-to-a-fitschan)) and you can also extract any cards that remain and write them out yourself ([Extracting Individual Cards from a FitsChan](#extracting-individual-cards-from-a-fitschan)) before you drop the FitsChan.

If you choose to use them, however, they behave in a very similar manner to those used by a Channel ([Reading Objects from Files](crate::guide::channels#reading-objects-from-files) and [Writing Objects to Files](crate::guide::channels#writing-objects-to-files)).
You give them to the FitsChan with [`set_source`](crate::FitsChan::set_source) and [`set_sink`](crate::FitsChan::set_sink).
The source is read the first time the FitsChan is used, to fill it with FITS cards, and the FitsChan is then rewound, so that the first card becomes current.
The cards that remain are written to the sink when the FitsChan is dropped; since a drop cannot report an error, [`write_fits`](crate::FitsChan::write_fits) writes them at a time of your choosing and reports any error.

The only real difference between the source and sink of a FitsChan and those of a basic Channel is that FITS cards are limited in length to 80 characters.
The `LineSource` and `LineSink` of [Reading Objects from Files](crate::guide::channels#reading-objects-from-files) and [Writing Objects to Files](crate::guide::channels#writing-objects-to-files) could therefore be used to access FITS headers stored in text files.
If you were not accessing a text file, however, appropriate changes to the I/O would be needed since the separating newline characters would be absent.
The details obviously depend on the format of the file you are handling, which need not necessarily be a true FITS file.
