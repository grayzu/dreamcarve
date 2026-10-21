# Importing DXF

DreamCarve reads DXF drawings from CAD programs such as LibreCAD, QCAD,
FreeCAD and Fusion. `init` tells a DXF from an SVG by the file extension:

```sh
dreamcarve init plaque.dxf
dreamcarve init plaque.dxf --units mm --join-tolerance 0.05 --layer outline --layer holes
```

The three options after the file name are for DXF only; `init` refuses them
with an SVG. Everything else, the project `init` writes, the values it
leaves for you to set and the commands that follow, is the same as for an
SVG: see the [quick start](../quick-start/first-cut.md) and
[Overview](overview.md#dreamcarve-init-svgdxf).

## What is read

DreamCarve reads the entities in **model space**. Anything on a paper
space layout is left out.

| Entity | Becomes |
| --- | --- |
| `LINE` | An open shape of one segment. |
| `ARC`, `CIRCLE` | Arcs. They stay arcs in the project, with the centre and radius the file states, and are written as `G2`/`G3` where the post-processor allows. |
| `LWPOLYLINE`, `POLYLINE` | A shape with the polyline's vertices and bulges (arc segments). The polyline's closed flag closes the shape. Polygon and polyface meshes are skipped. A polyline smoothed with "spline fit" imports as the smoothed curve, not the frame it was fitted to; one smoothed with "curve fit" keeps its fitted arcs. |
| `SPLINE` | Short straight lines within the project's tolerance (0.01 mm) of the curve. Rational splines and fit-point-only splines are read. |
| `ELLIPSE` | Short straight lines within the tolerance of the ellipse or elliptical arc. |
| `INSERT` | The block's contents, moved, rotated and scaled as the insert says, up to 12 blocks deep. A mirrored insert mirrors its arcs; an insert scaled differently in X and Y turns arcs into short lines. An array insert (`MINSERT`, rows and columns of one block) places the block in every cell. |

Everything else, `TEXT`, `MTEXT`, `DIMENSION`, `HATCH`, block attributes
(`ATTRIB`), 3D solids and meshes, images, points, is **skipped and
counted**. The count is printed as one warning, so nothing disappears
silently:

```text
warning: dxf_entities_skipped: 2 entity(ies) DreamCarve does not read were skipped: DIMENSION: 1, TEXT: 1
```

### Mirrored and tilted entities

Arcs, circles, polylines, ellipses and inserts carry the direction they
face, their *extrusion direction*. Many CAD programs mirror a shape by
turning it over, so that it faces down (−Z) instead of up. DreamCarve
applies this: a shape facing down imports mirrored left to right, where
the CAD program shows it, with its arcs turning the other way. A shape
facing any other way stands at an angle to the drawing's plane; it cannot
be cut from above without distorting it, so it is skipped and counted, by
type, as not in the XY plane:

```text
warning: dxf_entities_skipped: 1 entity(ies) DreamCarve does not read were skipped: CIRCLE (not in the XY plane): 1
```

Rotate such a shape flat in the CAD program, or project it onto the XY
plane ("flatten"), before importing.

To cut text, explode it to outlines in the CAD program first (most call
this "explode text" or "convert to polyline").

Coordinates are used as they are: DXF and DreamCarve both have X to the
right and Y away from you, so the drawing's origin is the project's origin
and nothing is flipped. `init` then moves the drawing so it sits 10 mm in
from the stock's bottom-left corner, as for an SVG.

## Units

A DXF carries its units in the `$INSUNITS` header variable, and most CAD
programs write it. DreamCarve scales from inches, feet, miles, millimetres,
centimetres and metres to the project's millimetres. When the file does
not say (the variable is missing or `0`, "unitless"), `init` stops:

```text
error: cannot import part.dxf: DXF declares no usable units ($INSUNITS is unitless); say which units it is drawn in; pass `--units mm|inch`
```

Run it again with the units the drawing was made in. `--units` also
overrides a file that does say, for the drawing that was saved with the
wrong setting, and it sets the project's display units, which the G-code is
written in, as it does for an SVG. A DXF in inches imported with `--units
inch` makes a project whose programs are in inches; edit `display_units`
in the project file to change that.

`init` prints what it decided:

```text
Imported 10 shape(s) from plaque.dxf: units mm (from $INSUNITS), 1 drawing unit = 1.0000 mm.
```

## Layers and names

Each shape is named after its **layer**: a layer with one shape gives a
shape of that name, and a layer with several gives `holes-1`, `holes-2`,
and so on, in the order the file lists them. Geometry inside a block that
sits on layer `0` takes the layer of the insert that places it, as CAD
programs display it.

`--layer <name>` imports only that layer; repeat it for several. Layer `0`
is named `0`. Without the option every layer is imported. Layers are a
convenient way to keep construction lines out of the project, or to make
one project per layer.

## Stitching

CAD drawings are often made of separate lines and arcs that only touch at
their ends. DreamCarve **joins** open segments on the same layer whose ends
lie within the join tolerance, 0.01 mm by default, nearest pair first, and
**closes** a chain whose two ends meet within it. Four `LINE` entities
around a rectangle become one closed rectangle; a side drawn as two lines
and a corner arc becomes part of it. Closed shapes, such as a circle or a
closed polyline, are left alone. Segments on different layers are never
joined.

A drawing whose ends do not quite meet, because it was traced by hand or
snapped to a coarse grid, imports as open chains, which a Pocket and an
Inside or Outside Profile do not cut; `dreamcarve check` lists them. Raise the tolerance to join them:

```sh
dreamcarve init part.dxf --join-tolerance 0.5
```

The joined piece is moved by the gap, so a large tolerance can move a line
end by up to that distance. Keep it small; fix the drawing when it needs
more than a few tenths of a millimetre.

Two identical shapes on one layer, which some programs write when a shape
is copied over itself, import once.

## Limits

A drawing is refused when any shape reaches more than 6000 mm (6 m) from
the origin after conversion to millimetres, when a coordinate is not a
number, or when a block is inserted into itself without end (the first 12
levels are read and a warning names the block). An insert of a block the
file does not define is skipped with a warning. Inserts and arrays may
place at most 500 000 block entities in all; a drawing of arrays nested
in arrays that places more is refused (explode the arrays in the CAD
program, or simplify them). A file that is cut off or
garbled, so that its codes and values no longer pair up, is refused with
the line (or, in a binary DXF, the byte) where it goes wrong.

ASCII and binary DXF are both read, from every AutoCAD version. Names in
files from before AutoCAD 2007 that are not UTF-8 are read in the Western
European code page (Windows-1252); a file saved in another code page, such
as a Cyrillic or Japanese one, may show wrong characters in its layer
names. The geometry is not affected; save the file as a 2007 or later DXF
to fix the names.
