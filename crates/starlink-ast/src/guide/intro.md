# Introduction

This crate is a Rust port of the Starlink AST library, which is written in C.
This guide is adapted from SUN/211, the guide to the C library, by Rodney Warren-Smith and David Berry.

Copyright (C) 2026 East Asian Observatory and the Association of Universities for Research in Astronomy.

Welcome to the AST library.
If you are writing software for astronomy and need to use celestial coordinates (*e.g.* RA and Dec), spectral coordinates (*e.g.* wavelength, frequency, *etc.*), or other coordinate system information, then this library should be of interest.
It provides solutions for most of the problems you will meet and allows you to write robust and flexible software.
It is able to read and write WCS information in a variety of formats, including [FITS-WCS](http://fits.gsfc.nasa.gov/fits_wcs.html).

## What Problems Does AST Tackle?

Here are some of the main problems you may face when handling world coordinate system (WCS) information and the solutions that AST provides:

1. **The Variety of Coordinate Systems**

   Astronomers use a wide range of differing coordinate systems to describe positions within a variety of physical domains.
   For instance, there are a large number of celestial coordinate systems in use within astronomy to describe positions on the sky.
   Understanding these, and knowing how to convert coordinates between them, can require considerable expertise.
   It can also be difficult to decide which of them your software should support.
   The same applies to coordinate systems describing other domains, such as position within an electro-magnetic spectrum.

   **Solution.**
   AST has built-in knowledge of many coordinate systems and allows you to convert freely between them without specialist knowledge.
   This avoids the need to embed details of specific coordinate systems in your software.
   You also benefit automatically when new coordinate systems are added to AST.

2. **Storing and Retrieving WCS Information**

   Storing coordinate system information in astronomical datasets and retrieving it later can present a considerable challenge.
   Typically, it requires knowledge of rather complex conventions (*e.g.* FITS) which are low-level, often misinterpreted and may be subject to change.
   Exchanging information with other software systems is further complicated by the number of different conventions in use.

   **Solution.**
   AST combines a unifying high-level description of WCS information with the ability to save and restore this using a variety of formats.
   Details of the formats, which include FITS, are handled internally by AST.
   This frees you from the need to understand them or embed the details in your software.
   Again, you benefit automatically when new formats are added to AST.

3. **Aligning Data from Different Sources**

   One of the main uses of coordinate systems is to facilitate the inter-comparison of data from different sources.
   A typical use might be to plot (say) radio contours over an optical image.
   In practice, however, different celestial coordinate systems may have been used, making accurate alignment far from simple.

   **Solution.**
   AST provides a one-step method of aligning datasets, searching for all possible intermediate coordinate systems.
   This makes it simple to directly inter-relate the pixel coordinates of different datasets.

4. **Handling Different Types of Coordinate System**

   Not all coordinate systems used in astronomy are celestial ones, so if you are writing general-purpose software such as (say) a display tool, you may also need to handle axes representing wavelength, distance, time or whatever else comes along.
   Obviously, you would prefer not to handle each one as a special case.

   **Solution.**
   AST uses the same flexible high-level model to describe all types of coordinate system.
   This allows you to write software that handles different kinds of coordinate axis without introducing special cases.

## Other Design Objectives

As well as its scientific objectives, the AST library's design includes a number of technical criteria intended to make it applicable to as wide a range of projects as possible.
The main considerations are described here:

1. **Minimum Software Dependencies.**
   The AST library depends on no other software.
   It comes with bundled copies of the ERFA and Starlink PAL libraries, which are built at the same time as the library itself.

2. **Environment Independence.**
   AST is designed so that it can operate in a variety of "programming environments" and is not tied to any particular one.
   To allow this, it uses simple, flexible interfaces to obtain the following services:

   - **Data Storage.**
     Data I/O operations are based on text and/or FITS headers.
     This makes it easy to interface to a wide variety of astronomical data formats in a machine-independent way.

   - **Error Handling.**
     Errors are returned to the caller as values, which the caller can report by whatever route suits it (*e.g.* in a graphical interface).

3. **Object Oriented Design.**
   AST uses "object oriented" techniques in order to provide a flexible and easily-extended programming model.

## What Does "AST" Stand For?

The library name "AST" stands for "ASTrometry Library".
The name arose when it was thought that knowledge of "astrometry" (*i.e.* celestial coordinate systems) would form the bulk of the library.
In fact, it turns out that astrometry forms only a minor component, but the name AST has stuck.
