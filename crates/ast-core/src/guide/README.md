# User guide provenance

This file is not part of the rendered documentation.
It records where the guide in `ast_core::guide` came from, so that a later change to the C library's guide can be found and carried over.

## Source

- SUN/211 version 211.34, from `sun_master.tex` in the `reference/starlink-ast` submodule at commit `c6c60753`.
- Only the C edition is ported: text inside `f+` … `f-` blocks is the Fortran edition's and is ignored, as `selectfc` does.

On an upstream sync, `git -C reference/starlink-ast diff c6c60753..<new pin> -- sun_master.tex` shows what changed, and the section table below says where each changed section lives in the guide.

`sections.tsv` lists the C edition's `\section` and `\subsection` headings (line, level, title) between the Introduction and the Release Notes.
It was generated with:

```shell
perl -ne 'BEGIN{$s=1} if(/^[fc]\+ *$/){$s=/^c\+/?1:0; next} if(/^[fc]- *$/){$s=1; next} if($s && $.>=97 && $.<=18621 && /^\s*\\(sub)?section\*?\{(.*)\}\s*$/){my($l,$t)=($1?"2":"1",$2); $t=~s/\\x?label\{[^}]*\}//g; $t=~s/\\ldots/.../g; $t=~s/\\_/_/g; $t=~s/``|'"''"'/"/g; $t=~s/---/ — /g; print "$.\t$l\t$t\n"}' \
    reference/starlink-ast/sun_master.tex > crates/ast-core/src/guide/sections.tsv
```

## Figures

`figures/*.svg` are converted from the PDFs in `reference/starlink-ast/sun211_figures/` with Poppler's `pdftocairo`:

```shell
conda create -y -p <scratch>/poppler -c conda-forge poppler
<scratch>/poppler/bin/pdftocairo -svg reference/starlink-ast/sun211_figures/<name>.pdf crates/ast-core/src/guide/figures/<name>.svg
```

A chapter refers to a figure with an ordinary Markdown image, `![caption](figures/<name>.svg)`.
The build script copies each chapter into `$OUT_DIR/guide/` with every such link replaced by a `data:` URI holding the SVG, and the chapter's module documentation is that copy, so the figures travel inside the rendered pages.

## Sections

Each row's fate is one of:

- `guide::<chapter>` or `guide::<chapter>#<anchor>`: the chapter or heading it became;
- `merged into guide::<chapter>[#<anchor>]`: its content is part of another heading;
- `dropped: <reason>`;
- `pending`: not yet ported.

The `test_guide` gate checks that every row of `sections.tsv` has a row here and counts the `pending` ones.

| line | title | fate |
|---:|---|---|
| 97 | Introduction | guide::intro |
| 110 | &nbsp;&nbsp;What Problems Does AST Tackle? | guide::intro#what-problems-does-ast-tackle |
| 187 | &nbsp;&nbsp;Other Design Objectives | guide::intro#other-design-objectives |
| 277 | &nbsp;&nbsp;What Does "AST" Stand For? | guide::intro#what-does-ast-stand-for |
| 286 | Overview of AST Concepts | guide::concepts |
| 292 | &nbsp;&nbsp;Relationships Between Coordinate Systems | guide::concepts#relationships-between-coordinate-systems |
| 334 | &nbsp;&nbsp;Mappings Available | guide::concepts#mappings-available |
| 399 | &nbsp;&nbsp;Compound Mappings | guide::concepts#compound-mappings |
| 482 | &nbsp;&nbsp;Representing Coordinate Systems | guide::concepts#representing-coordinate-systems |
| 573 | &nbsp;&nbsp;Networks of Coordinate Systems | guide::concepts#networks-of-coordinate-systems |
| 619 | &nbsp;&nbsp;Input/Output Facilities | guide::concepts#inputoutput-facilities |
| 681 | &nbsp;&nbsp;Producing Graphical Output | dropped: Plot is not ported |
| 750 | How To... | guide::howto |
| 767 | &nbsp;&nbsp;... Obtain and Install AST | merged into guide::cargo |
| 777 | &nbsp;&nbsp;... Structure an AST Program | guide::howto#-structure-an-ast-program |
| 841 | &nbsp;&nbsp;... Build an AST Program | merged into guide::cargo |
| 899 | &nbsp;&nbsp;... Read a WCS Calibration from a Dataset | guide::howto#-read-a-wcs-calibration-from-a-dataset |
| 1016 | &nbsp;&nbsp;... Validate WCS Information | guide::howto#-validate-wcs-information |
| 1071 | &nbsp;&nbsp;... Display AST Data | guide::howto#-display-ast-data |
| 1106 | &nbsp;&nbsp;... Convert Between Pixel and World Coordinates | guide::howto#-convert-between-pixel-and-world-coordinates |
| 1182 | &nbsp;&nbsp;... Test if a WCS is a Celestial Coordinate System | guide::howto#-test-if-a-wcs-is-a-celestial-coordinate-system |
| 1245 | &nbsp;&nbsp;... Test if a WCS is a Spectral Coordinate System | guide::howto#-test-if-a-wcs-is-a-spectral-coordinate-system |
| 1263 | &nbsp;&nbsp;... Format Coordinates for Display | guide::howto#-format-coordinates-for-display |
| 1338 | &nbsp;&nbsp;... Display Coordinates as they are Transformed | guide::howto#-display-coordinates-as-they-are-transformed |
| 1401 | &nbsp;&nbsp;... Read Coordinates Entered by a User | guide::howto#-read-coordinates-entered-by-a-user |
| 1565 | &nbsp;&nbsp;... Create a New WCS Calibration | guide::howto#-create-a-new-wcs-calibration |
| 1668 | &nbsp;&nbsp;... Modify a WCS Calibration | guide::howto#-modify-a-wcs-calibration |
| 1882 | &nbsp;&nbsp;... Write a Modified WCS Calibration to a Dataset | guide::howto#-write-a-modified-wcs-calibration-to-a-dataset |
| 2138 | &nbsp;&nbsp;... Display a Graphical Coordinate Grid | dropped: Plot is not ported |
| 2409 | &nbsp;&nbsp;... Switch to Plot a Different Celestial Coordinate Grid | dropped: Plot is not ported |
| 2466 | &nbsp;&nbsp;... Give a User Control Over the Appearance of a Plot | dropped: Plot is not ported |
| 2587 | An AST Object Primer | pending |
| 2631 | &nbsp;&nbsp;AST Objects | pending |
| 2648 | &nbsp;&nbsp;Object Creation and Pointers | pending |
| 2733 | &nbsp;&nbsp;The Object Hierarchy | pending |
| 2808 | &nbsp;&nbsp;Displaying Objects | pending |
| 2884 | &nbsp;&nbsp;Getting Attribute Values | pending |
| 3011 | &nbsp;&nbsp;Setting Attribute Values | pending |
| 3240 | &nbsp;&nbsp;Testing, Clearing and Defaulting Attributes | pending |
| 3340 | &nbsp;&nbsp;Transforming Coordinates | pending |
| 3541 | &nbsp;&nbsp;Managing Object Pointers | pending |
| 3609 | &nbsp;&nbsp;AST Pointer Contexts — Begin and End | pending |
| 3723 | &nbsp;&nbsp;Exporting, Importing and Exempting AST Pointers | pending |
| 3832 | &nbsp;&nbsp;AST Objects within Multi-threaded Applications | pending |
| 3913 | &nbsp;&nbsp;Copying Objects | pending |
| 3973 | &nbsp;&nbsp;C Pointer Types | pending |
| 4090 | &nbsp;&nbsp;Error Detection | pending |
| 4350 | &nbsp;&nbsp;Sharing the Error Status | pending |
| 4387 | Inter-Relating Coordinate Systems (Mappings) | pending |
| 4402 | &nbsp;&nbsp;The Mapping Class | pending |
| 4454 | &nbsp;&nbsp;The Mapping Model | pending |
| 4463 | &nbsp;&nbsp;Changing Attributes of a Mapping | pending |
| 4486 | &nbsp;&nbsp;Input and Output Coordinate Numbers | pending |
| 4543 | &nbsp;&nbsp;Forward and Inverse Transformations | pending |
| 4565 | &nbsp;&nbsp;Inverting Mappings | pending |
| 4629 | &nbsp;&nbsp;Finding the Rate of Change of a Mapping Output | pending |
| 4644 | &nbsp;&nbsp;Reporting Coordinate Transformations | pending |
| 4679 | &nbsp;&nbsp;Handling Missing (Bad) Coordinate Values | pending |
| 4768 | &nbsp;&nbsp;Example — the UnitMap | pending |
| 4817 | &nbsp;&nbsp;Example — the PermMap | pending |
| 5055 | Compound Mappings (CmpMaps) | pending |
| 5066 | &nbsp;&nbsp;Combining Mappings in Series | pending |
| 5161 | &nbsp;&nbsp;Combining Mappings in Parallel | pending |
| 5217 | &nbsp;&nbsp;The Component Mappings | pending |
| 5260 | &nbsp;&nbsp;Creating More Complex Mappings | pending |
| 5318 | &nbsp;&nbsp;Example — Transforming Between Two Calibrated Images | pending |
| 5390 | &nbsp;&nbsp;Over-Complex Compound Mappings | pending |
| 5438 | &nbsp;&nbsp;Simplifying Compound Mappings | pending |
| 5565 | Representing Coordinate Systems (Frames) | pending |
| 5581 | &nbsp;&nbsp;The Frame Model | pending |
| 5602 | &nbsp;&nbsp;Creating a Frame | pending |
| 5642 | &nbsp;&nbsp;Using a Frame as a Mapping | pending |
| 5708 | &nbsp;&nbsp;Frame Axis Attributes | pending |
| 5834 | &nbsp;&nbsp;Frame Attributes | pending |
| 5935 | &nbsp;&nbsp;Formatting Axis Values | pending |
| 6084 | &nbsp;&nbsp;Normalising Frame Coordinates | pending |
| 6178 | &nbsp;&nbsp;Reading Formatted Axis Values | pending |
| 6610 | &nbsp;&nbsp;Permuting Frame Axes | pending |
| 6665 | &nbsp;&nbsp;Selecting Frame Axes | pending |
| 6806 | &nbsp;&nbsp;Calculating Distances, Angles and Offsets | pending |
| 7048 | &nbsp;&nbsp;The Domain Attribute | pending |
| 7186 | &nbsp;&nbsp;Conventions for Domain Names | pending |
| 7280 | &nbsp;&nbsp;The Unit Attribute | pending |
| 7589 | Celestial Coordinate Systems (SkyFrames) | pending |
| 7597 | &nbsp;&nbsp;The SkyFrame Model | pending |
| 7640 | &nbsp;&nbsp;Creating a SkyFrame | pending |
| 7676 | &nbsp;&nbsp;Specifying a Particular Celestial Coordinate System | pending |
| 7720 | &nbsp;&nbsp;Attributes which Qualify Celestial Coordinate Systems | pending |
| 7816 | &nbsp;&nbsp;Using Default SkyFrame Attributes | pending |
| 7912 | &nbsp;&nbsp;Formatting Celestial Coordinates | pending |
| 8023 | &nbsp;&nbsp;Reading Formatted Celestial Coordinates | pending |
| 8255 | &nbsp;&nbsp;Representing Offsets from a Specified Sky Position | pending |
| 8283 | Spectral Coordinate Systems (SpecFrames) | pending |
| 8290 | &nbsp;&nbsp;The SpecFrame Model | pending |
| 8305 | &nbsp;&nbsp;Creating a SpecFrame | pending |
| 8342 | &nbsp;&nbsp;Specifying a Particular Spectral Coordinate System | pending |
| 8374 | &nbsp;&nbsp;Attributes which Qualify Spectral Coordinate Systems | pending |
| 8451 | &nbsp;&nbsp;Using Default SpecFrame Attributes | pending |
| 8576 | &nbsp;&nbsp;Creating Spectral Cubes | pending |
| 8651 | &nbsp;&nbsp;Handling Dual-Sideband Spectra | pending |
| 9095 | Time Systems (TimeFrames) | pending |
| 9101 | &nbsp;&nbsp;The TimeFrame Model | pending |
| 9121 | &nbsp;&nbsp;Creating a TimeFrame | pending |
| 9157 | &nbsp;&nbsp;Specifying a Particular Time System | pending |
| 9191 | &nbsp;&nbsp;Attributes which Qualify Time Coordinate Systems | pending |
| 9246 | Compound Frames (CmpFrames) | pending |
| 9260 | &nbsp;&nbsp;Creating a CmpFrame | pending |
| 9328 | &nbsp;&nbsp;The Attributes of a CmpFrame | pending |
| 9385 | An Introduction to Coordinate System Conversions | pending |
| 9395 | &nbsp;&nbsp;Converting between Celestial Coordinate Systems | pending |
| 9706 | &nbsp;&nbsp;Converting between Spectral Coordinate Systems | pending |
| 9882 | &nbsp;&nbsp;Converting between Time Coordinate Systems | pending |
| 9887 | &nbsp;&nbsp;Handling SkyFrame Axis Permutations | pending |
| 9991 | &nbsp;&nbsp;Converting Between Frames | pending |
| 10125 | &nbsp;&nbsp;The Choice of Alignment System | pending |
| 10182 | Coordinate System Networks (FrameSets) | pending |
| 10233 | &nbsp;&nbsp;The FrameSet Model | pending |
| 10268 | &nbsp;&nbsp;Creating a FrameSet | pending |
| 10329 | &nbsp;&nbsp;Adding New Frames to a FrameSet | pending |
| 10481 | &nbsp;&nbsp;The Base and Current Frames | pending |
| 10537 | &nbsp;&nbsp;Referring to the Base and Current Frames | pending |
| 10592 | &nbsp;&nbsp;Using a FrameSet as a Mapping | pending |
| 10672 | &nbsp;&nbsp;Extracting a Mapping from a FrameSet | pending |
| 10750 | &nbsp;&nbsp;Using a FrameSet as a Frame | pending |
| 10818 | &nbsp;&nbsp;Extracting a Frame from a FrameSet | pending |
| 10853 | &nbsp;&nbsp;Removing a Frame from a FrameSet | pending |
| 10914 | Higher Level Operations on FrameSets | pending |
| 10917 | &nbsp;&nbsp;Creating FrameSets with astConvert | pending |
| 11067 | &nbsp;&nbsp;Converting between FrameSet Coordinate Systems | pending |
| 11293 | &nbsp;&nbsp;Example — Registering Two Images | pending |
| 11474 | &nbsp;&nbsp;Re-Defining a FrameSet Coordinate System | pending |
| 11578 | &nbsp;&nbsp;Example — Binning an Image | pending |
| 11689 | &nbsp;&nbsp;Maintaining the Integrity of FrameSets | pending |
| 11897 | &nbsp;&nbsp;Merging FrameSets | pending |
| 12012 | Saving and Restoring Objects (Channels) | pending |
| 12035 | &nbsp;&nbsp;The Channel Model | pending |
| 12072 | &nbsp;&nbsp;Creating a Channel | pending |
| 12145 | &nbsp;&nbsp;Writing Objects to a Channel | pending |
| 12205 | &nbsp;&nbsp;Reading Objects from a Channel | pending |
| 12264 | &nbsp;&nbsp;Saving and Restoring Multiple Objects | pending |
| 12293 | &nbsp;&nbsp;Validating Input | pending |
| 12356 | &nbsp;&nbsp;Storing an ID String with an Object | pending |
| 12424 | &nbsp;&nbsp;The Textual Output Format | pending |
| 12566 | &nbsp;&nbsp;Controlling the Amount of Output | pending |
| 12710 | &nbsp;&nbsp;Controlling Commenting | pending |
| 12755 | &nbsp;&nbsp;Editing Textual Output | pending |
| 12817 | &nbsp;&nbsp;Mixing Objects with other Text | pending |
| 12896 | &nbsp;&nbsp;Reading Objects from Files | pending |
| 13076 | &nbsp;&nbsp;Writing Objects to Files | pending |
| 13239 | &nbsp;&nbsp;Reading and Writing Objects to other Places | pending |
| 13269 | Storing AST Objects in FITS Headers (FitsChans) | pending |
| 13298 | &nbsp;&nbsp;The Native FITS Encoding | pending |
| 13325 | &nbsp;&nbsp;The FitsChan Model | pending |
| 13465 | &nbsp;&nbsp;Creating a FitsChan | pending |
| 13521 | &nbsp;&nbsp;Addressing Cards in a FitsChan | pending |
| 13638 | &nbsp;&nbsp;Writing Native Objects to a FitsChan | pending |
| 13732 | &nbsp;&nbsp;Extracting Individual Cards from a FitsChan | pending |
| 13815 | &nbsp;&nbsp;The Native FitsChan Output Format | pending |
| 13910 | &nbsp;&nbsp;Adding Individual Cards to a FitsChan | pending |
| 14011 | &nbsp;&nbsp;Adding Concatenated Cards to a FitsChan | pending |
| 14064 | &nbsp;&nbsp;Reading Native Objects From a FitsChan | pending |
| 14156 | &nbsp;&nbsp;Saving and Restoring Multiple Objects in a FitsChan | pending |
| 14176 | &nbsp;&nbsp;Mixing Native Objects with Other FITS Cards | pending |
| 14196 | &nbsp;&nbsp;Finding and Changing Cards in a FitsChan | pending |
| 14359 | &nbsp;&nbsp;Source and Sink Functions for FitsChans | pending |
| 14475 | Using Foreign FITS Encodings | pending |
| 14490 | &nbsp;&nbsp;The Foreign FITS Encodings | pending |
| 14575 | &nbsp;&nbsp;Limitations of Foreign Encodings | pending |
| 14634 | &nbsp;&nbsp;Identifying Foreign Encodings on Input | pending |
| 14792 | &nbsp;&nbsp;Reading Foreign WCS Information from a FITS Header | pending |
| 14945 | &nbsp;&nbsp;Removing WCS Information from FITS Headers — the Destructive Read | pending |
| 15059 | &nbsp;&nbsp;Propagating WCS Information through Data Processing Steps | pending |
| 15104 | &nbsp;&nbsp;Writing Foreign WCS Information to a FITS Header | pending |
| 15289 | Storing AST Objects as XML (XmlChan) | dropped: XmlChan is not ported |
| 15402 | &nbsp;&nbsp;Reading IVOA Space-Time-Coordinates XML (STC-X) Descriptions | dropped: XmlChan is not ported |
| 15424 | Reading and writing STC-S descriptions (StcsChans) | dropped: StcsChan is not ported |
| 15438 | Creating Your Own Private Mappings (IntraMaps) | pending |
| 15440 | &nbsp;&nbsp;The Need for Extensibility | pending |
| 15467 | &nbsp;&nbsp;The IntraMap Model | pending |
| 15497 | &nbsp;&nbsp;Limitations of IntraMaps | pending |
| 15571 | &nbsp;&nbsp;Writing a Transformation Function | pending |
| 15773 | &nbsp;&nbsp;Registering a Transformation Function | pending |
| 15937 | &nbsp;&nbsp;Creating an IntraMap | pending |
| 16032 | &nbsp;&nbsp;Restricted Implementations of Transformation Functions | pending |
| 16149 | &nbsp;&nbsp;Variable Numbers of Coordinates | pending |
| 16263 | &nbsp;&nbsp;Adapting a Transformation Function to Individual IntraMaps | pending |
| 16351 | &nbsp;&nbsp;Simplifying IntraMaps | pending |
| 16598 | &nbsp;&nbsp;Writing and Reading IntraMaps | pending |
| 16674 | &nbsp;&nbsp;Managing Transformation Functions in Libraries | pending |
| 16797 | Producing Graphical Output (Plots) | dropped: Plot is not ported |
| 16805 | &nbsp;&nbsp;The Plot Model | dropped: Plot is not ported |
| 16848 | &nbsp;&nbsp;Plotting Symbols | dropped: Plot is not ported |
| 16911 | &nbsp;&nbsp;Plotting Geodesic Curves | dropped: Plot is not ported |
| 17030 | &nbsp;&nbsp;Plotting Curves Parallel to Axes | dropped: Plot is not ported |
| 17098 | &nbsp;&nbsp;Plotting Generalized Curves | dropped: Plot is not ported |
| 17134 | &nbsp;&nbsp;Clipping | dropped: Plot is not ported |
| 17210 | &nbsp;&nbsp;Using a Plot as a Mapping | dropped: Plot is not ported |
| 17265 | &nbsp;&nbsp;Using a Plot as a Frame | dropped: Plot is not ported |
| 17293 | &nbsp;&nbsp;Regions of Valid Physical Coordinates | dropped: Plot is not ported |
| 17317 | &nbsp;&nbsp;Plotting Borders | dropped: Plot is not ported |
| 17389 | &nbsp;&nbsp;Plotting Text | dropped: Plot is not ported |
| 17466 | &nbsp;&nbsp;Plotting a Grid | dropped: Plot is not ported |
| 17507 | &nbsp;&nbsp;Controlling the Appearance of Sub-strings | dropped: Plot is not ported |
| 17547 | &nbsp;&nbsp;Producing Logarithmic Axes | dropped: Plot is not ported |
| 17596 | &nbsp;&nbsp;Choosing a Graphics Package | dropped: Plot is not ported |
| 17652 | Compiling and Linking Software that Uses AST | guide::cargo |
| 17663 | &nbsp;&nbsp;Accessing the "ast.h" Header File | dropped: C build mechanics; see guide::cargo |
| 17750 | &nbsp;&nbsp;Linking with AST Facilities | dropped: C build mechanics; see guide::cargo |
| 17827 | &nbsp;&nbsp;Building ADAM Applications that Use AST | dropped: C build mechanics; see guide::cargo |
| 17868 | The AST Class Hierarchy | dropped: reference material generated by rustdoc |
| 17944 | AST Function Descriptions | dropped: reference material generated by rustdoc |
| 17957 | AST Attribute Descriptions | dropped: reference material generated by rustdoc |
| 17968 | AST Class Descriptions | dropped: reference material generated by rustdoc |
| 17979 | UNIX Command Descriptions | dropped: reference material generated by rustdoc |
| 17996 | AST Memory Management and Utility Functions | dropped: reference material generated by rustdoc |
| 18055 | FITS-WCS Coverage | pending |
| 18093 | &nbsp;&nbsp;Paper I - General Linear Coordinates | pending |
| 18258 | &nbsp;&nbsp;Paper II - Celestial Coordinates | pending |
| 18450 | &nbsp;&nbsp;Paper III - Spectral Coordinates | pending |
| 18577 | &nbsp;&nbsp;Paper IV - Coordinate Distortions | pending |

## Authorship

Each chapter's commit is credited from `git blame` on its line range, computed by `cargo run -p audit-tools --bin guide-authors -- <start> <end>`.
A writer with 10% or more of the counted lines is credited, the larger share as author.

| chapter | lines | author | co-author |
|---|---|---|---|
| intro | 97–285 | Rodney Warren-Smith | David Berry |
| concepts | 286–749 | Rodney Warren-Smith | David Berry |
| howto | 750–2137 | Rodney Warren-Smith | David Berry |
