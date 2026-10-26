#!/usr/bin/env python3
# Copyright 2026 Association of Universities for Research in Astronomy (AURA)
# SPDX-License-Identifier: LGPL-3.0-or-later

"""Read the WCS of one HDU of a FITS file and report it, with starlink-pyast.

Usage: fits_wcs.py [--dump] <file.fits> [<hdu>]

The twin of crates/starlink-ast/examples/fits_wcs.rs, written against the
C library through starlink-pyast so that the two outputs can be diffed.
The header of HDU <hdu> (0, the default, is the primary HDU) is read with
astropy and handed to a FitsChan through starlink.Atl.PyFITSAdapter, which
reads a FrameSet from it.
Without --dump the program describes the base and current Frames and
transforms the corners of the image, GRID position 0.5 and NAXISn + 0.5
on every axis, and its centre into the current Frame and back again.
With --dump it prints the FrameSet's Native description instead.
astropy presents a tile-compressed image with the image's own BITPIX,
NAXIS and NAXISn, as CFITSIO does, and the Rust twin does the same.
"""

import argparse

import numpy as np
from astropy.io import fits
from starlink import Ast, Atl


def axis_lengths(header):
    """The length of every data axis, NAXIS1 first; a missing NAXISn counts as 0."""
    return [header.get(f"NAXIS{i}", 0) for i in range(1, header.get("NAXIS", 0) + 1)]


def sci(v):
    """The value with 17 significant digits, so that every bit of a double is visible."""
    return f"{v:.16e}"


def numbers(values, write):
    """The values separated by spaces, each written by `write`, the bad value as <bad>."""
    return " ".join("<bad>" if v == Ast.BAD else write(v) for v in values)


def describe(which, frame):
    """Print the Frame's identity and its axes."""
    print(
        f'{which} frame: Domain="{frame.Domain}" Naxes={frame.Naxes} Title="{frame.Title}"'
    )
    for axis in range(1, frame.Naxes + 1):
        label = frame.get(f"Label({axis})")
        symbol = frame.get(f"Symbol({axis})")
        unit = frame.get(f"Unit({axis})")
        print(f'  axis {axis}: Label="{label}" Symbol="{symbol}" Unit="{unit}"')


def report(path, hdu, header, fs):
    """Describe the FrameSet and transform the image corners through it."""
    lengths = axis_lengths(header)
    print(f"file: {path}")
    print(f"hdu: {hdu}")
    print("data array: " + (" x ".join(str(n) for n in lengths) if lengths else "none"))
    print(f"frames: {fs.Nframe}  base: {fs.Base}  current: {fs.Current}")
    base = fs.getframe(Ast.BASE)
    current = fs.getframe(Ast.CURRENT)
    describe("base", base)
    describe("current", current)

    # The image corners in GRID coordinates, axis 1 varying fastest, then
    # the centre. A grid axis the data array does not have is one pixel long.
    nin = base.Naxes

    def size(axis):
        return float(max(lengths[axis] if axis < len(lengths) else 1, 1))

    points = [
        [size(a) + 0.5 if (k >> a) & 1 else 0.5 for a in range(nin)]
        for k in range(1 << nin)
    ]
    points.append([(size(a) + 1.0) / 2.0 for a in range(nin)])
    grid = np.array(points, dtype=float).T

    world = fs.tran(grid, True)
    back = fs.tran(world, False)
    print("corners: grid -> current [formatted] -> grid")
    for p in range(grid.shape[1]):
        normed = current.norm(world[:, p].copy())
        formatted = [
            current.format(axis, normed[axis - 1])
            for axis in range(1, current.Naxes + 1)
        ]
        print(
            f"  {numbers(grid[:, p], lambda v: f'{v:.1f}')} -> {numbers(world[:, p], sci)}"
            f" [{', '.join(formatted)}] -> {numbers(back[:, p], sci)}"
        )


def main():
    parser = argparse.ArgumentParser(
        description="Read the WCS of one HDU of a FITS file and report it."
    )
    parser.add_argument(
        "--dump", action="store_true", help="print the FrameSet's Native description"
    )
    parser.add_argument("file", help="the FITS file")
    parser.add_argument(
        "hdu",
        nargs="?",
        type=int,
        default=0,
        help="the HDU, 0 being the primary (default 0)",
    )
    args = parser.parse_args()
    if args.hdu < 0:
        parser.error("the HDU must be a non-negative integer")

    with fits.open(args.file) as hdus:
        if args.hdu >= len(hdus):
            raise SystemExit(f"fits_wcs: the file has fewer than {args.hdu + 1} HDUs")
        hdu = hdus[args.hdu]
        fc = Ast.FitsChan(Atl.PyFITSAdapter(hdu))
        fs = fc.read()
        if fs is None:
            raise SystemExit("fits_wcs: no FrameSet could be read from the header")
        if not isinstance(fs, Ast.FrameSet):
            raise SystemExit(f"fits_wcs: the header holds a {fs.Class}, not a FrameSet")
        if args.dump:
            print(fs)
        else:
            report(args.file, args.hdu, hdu.header, fs)


if __name__ == "__main__":
    main()
