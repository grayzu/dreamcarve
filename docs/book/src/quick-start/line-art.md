# From line art

This chapter takes line art, a drawing made of strokes, to an inlay. The
art is a compass: a ring with eight spokes, every stroke 2.5 mm wide, 70 mm
tall. You prepare it with `dreamcarve image-prep`, draw an oval round it
for the male board's edge, and export the two boards that make the inlay.
It is the same V-bit inlay as [An inlay pair](inlay.md), with a drawing
that needed preparing first.

Line art is the case [Preparing
artwork](../user-guide/preparing-artwork.md) calls **regions mode**. A
stroke is a line with a width, and `init` imports only the line's centre,
which no inlay can cut. `image-prep` turns each stroke into the area it
covers, and with `--regions` it then makes the spaces **between** the lines
the shapes to inlay: here, the eight pockets between the spokes and inside
the ring. The lines stay as the board between the pockets: ridges of the
light board 2.5 mm wide, between plugs of dark wood.

If you have not done [An inlay pair](inlay.md), do it first: this chapter
assumes you know the tool library, pasting operations and what the Inlay's
values do.

Every command here is run by DreamCarve's test suite exactly as written,
and the oval is drawn into the file by the same step, so if it does not work
for you, that is a bug.

## What you need

- DreamCarve installed and on your `PATH`. See [Installing](installing.md).
- A text editor. A drawing program such as Inkscape is useful in step 3,
  but the chapter gives you the oval to paste instead.
- The drawing,
  [`compass-art.svg`](https://raw.githubusercontent.com/grayzu/dreamcarve/main/tests/fixtures/prep/compass-art.svg).
  It is also in the repository at `tests/fixtures/prep/compass-art.svg`.
  A script draws it; it is nobody's art.
- For the cut: the same boards and bits as [An inlay pair](inlay.md): two
  boards of contrasting hardwoods sold as 12 mm, at least 116 × 104 mm, a
  60° 6.35 mm V-bit, a 3.175 mm two-flute upcut end mill with at least
  14.2 mm of flute, and a router controlled by grblHAL.

## 1. Put the drawing in a folder

Make an empty folder and put `compass-art.svg` in it:

```sh
mkdir compass
cd compass
```

Run every command below from that folder.

The drawing has two paths, both stroked 2.5 mm wide with no fill: the
`ring`, a circle, and the `spokes`, eight lines from its centre to the
ring. It has no oval yet.

## 2. Prepare it

Run `image-prep` with `--regions`:

<!-- doc-test: run -->
```sh
dreamcarve image-prep compass-art.svg --regions
```

```text
warning: svg_stroke_only_path: 2 path(s) have a stroke and no fill; the first is `ring`, stroked 2.500 mm wide. Their strokes were made into areas
Prepared compass-art.svg → compass-art-inlay.svg (regions)
  Read: 2 paths; 0 text, 0 images, 0 clip paths, 0 masks
  Strokes made into areas: 2, 2.50 mm wide
  Design height: 70.00 mm
  Result: 8 pieces, 0 holes
  Dropped: smoothed within 0.05 % (0.035 mm)
  Thinnest piece: 8.35 mm (11.93 % of the height)
  Narrowest gap, the board ridge between pockets: 2.50 mm (3.57 %)
  At 100 / 200 / 300 mm tall: thinnest piece 11.93 / 23.86 / 35.79 mm; narrowest gap 3.57 / 7.14 / 10.71 mm
  The narrowest gap reaches 1.5 mm at 42 mm tall.
Wrote compass-art-inlay.svg
```

It wrote `compass-art-inlay.svg` beside the drawing and left
`compass-art.svg` as it was. The warning is a note: the strokes were made
into areas, which is what you asked for. Read the report:

- **8 pieces, 0 holes**: the eight pockets between the spokes. Each is one
  shape.
- **Design height 70.00 mm**: the ring's outer edge, strokes included. Every
  threshold is a percentage of this height.
- **Narrowest gap, the board ridge between pockets: 2.50 mm**: the stroke
  width, the ridge of light board that stands between two pockets in the
  cut. This is the number to check against your bit: below about 1.5 mm a
  ridge is hard to cut and to glue, and the report says at what height your
  art would reach it.
- **Dropped: smoothed**: the pockets' edges are within a few hundredths of
  a millimetre of the drawn ones. No piece or hole was dropped.

[Preparing artwork](../user-guide/preparing-artwork.md#the-report) reads
every line of the report.

## 3. Draw the boundary oval

The male board's band needs an outer edge. Draw it as an oval round the
compass: the Inlay's `boundary` ([Inlay](../user-guide/inlay.md#the-boundary)
says how it works). The oval is a thin line, and `image-prep` drops a line
that thin as a hairline, so you do not draw it in the drawing you prepare.
You draw it into the **prepared** drawing, after `image-prep`.

In a drawing program such as Inkscape, open `compass-art-inlay.svg`, draw an
ellipse 96 mm wide and 84 mm tall with its centre on the compass's centre,
no fill and a thin stroke, set its id to `oval` in the XML editor (Edit →
XML Editor), and save. Or add the same ellipse to the file as text, on a new
line before the last line, `</svg>`:

<!-- doc-test: insert compass-art-inlay.svg -->
```xml
  <ellipse id="oval" cx="48" cy="42" rx="48" ry="42" fill="none" stroke="#000000" stroke-width="0.2"/>
```

The page is 96 × 84 mm and the compass's centre is at (48, 42), so this oval
reaches the page's edges and stands 9.5 mm clear of the pockets (42 mm from
the centre, less the pockets' 32.5 mm reach). Any closed shape round the art
does; an oval is a good choice because it has no corner to round.

## 4. Create the project

<!-- doc-test: run -->
```sh
dreamcarve init compass-art-inlay.svg -o compass.dcv
```

```text
Imported 9 shape(s) from compass-art-inlay.svg: page units mm, 1 user unit = 1.0000 mm.
warning: svg_stroke_only_path: 1 path(s) have a stroke and no fill; the first is `oval`, stroked 0.200 mm wide. Only its centreline is imported, an open path no Inlay or V-Carve can cut; turn strokes into shapes; `dreamcarve image-prep` would fix this
Wrote compass.dcv: stock 116.00 × 104.00 mm, post-processor grblhal, 9 shape(s) targeted by Profile.
Values to set; export refuses until every one is set:
compass.dcv:10:21: stock.thickness_mm is not set (mm)
compass.dcv:31:22: tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`
compass.dcv:32:24: tools[t1].plunge_mm_min is not set (mm/min) for tool `Set me`
compass.dcv:33:14: tools[t1].rpm is not set (RPM) for tool `Set me`
compass.dcv:34:23: tools[t1].step_down_mm is not set (mm) for tool `Set me`
compass.dcv:57:19: operations[op1].depth_mm is not set (mm) for operation `Profile`
Next: edit compass.dcv and replace each `null` listed above. To cut through, `depth_mm` is the thickness plus `through_cut_allowance_mm`. Then run `dreamcarve check compass.dcv`.
```

The prepared drawing has none of the problems the [import
checks](../user-guide/preparing-artwork.md#the-checks-init-prints) look for,
except the one you just drew: the oval is a line with no fill, so `init`
says `svg_stroke_only_path` (its message calls the line an open path; the
oval is closed). For a boundary that is expected and harmless: the Inlay
uses the oval's outline, and the outline is exactly what `init` imported.
Ignore the warning for this shape.

The project has nine shapes, in the order of the file:

| Id | Name | What it is |
| --- | --- | --- |
| `i1` to `i8` | `piece-1` to `piece-8` | The eight pockets |
| `i9` | `oval` | The boundary; not a pocket |

The stock is the drawing plus 10 mm all round: 116 × 104 mm, with the oval
10 mm from each edge.

## 5. Add the tools from your library

The tools are those of [An inlay pair](inlay.md): a 3.175 mm end mill and a
60° 6.35 mm V-bit. Save this as `my-tools.json` in the folder:

<!-- doc-test: write my-tools.json -->
```json
{
  "version": 2,
  "tools": [
    {
      "id": "lt1",
      "source": null,
      "tool": {
        "id": "lt1",
        "name": "1/8 end mill",
        "vendor": null,
        "product_id": null,
        "number": 2,
        "cutter": "end_mill",
        "diameter_mm": 3.175,
        "flutes": 2,
        "feed_mm_min": 1000.0,
        "plunge_mm_min": 300.0,
        "rpm": 18000,
        "step_down_mm": 2.0,
        "step_over_fraction": 0.4
      }
    },
    {
      "id": "lt2",
      "source": null,
      "tool": {
        "id": "lt2",
        "name": "1/4 60 degree V-bit",
        "vendor": null,
        "product_id": null,
        "number": 1,
        "cutter": { "v_bit": { "included_angle_deg": 60.0 } },
        "diameter_mm": 6.35,
        "flutes": 2,
        "feed_mm_min": 1000.0,
        "plunge_mm_min": 300.0,
        "rpm": 18000,
        "step_down_mm": 2.0,
        "step_over_fraction": 0.4
      }
    }
  ]
}
```

If you did the inlay pair, your library already has these tools: `tools
import` says so, and the ids differ, so use the ids `tools list` shows. Add
the file's tools to your library, list it, and copy both tools into the
project:

<!-- doc-test: run -->
```sh
dreamcarve tools import my-tools.json
dreamcarve tools list
dreamcarve tools add compass.dcv lt1
dreamcarve tools add compass.dcv lt2
```

```text
`1/8 end mill`: imported (lt1)
`1/4 60 degree V-bit`: imported (lt2)
2 imported, 0 already present, 0 replaced; library: /Users/you/Library/Application Support/dreamcarve/tools.json
ID   NAME                 VENDOR  PRODUCT  CUTTER     DIAMETER_MM  FEED_MM_MIN  PLUNGE_MM_MIN  RPM    NUMBER  SOURCE
lt1  1/8 end mill         -       -        end mill   3.175        1000         300            18000  2       -
lt2  1/4 60 degree V-bit  -       -        V-bit 60°  6.35         1000         300            18000  1       -
Added `1/8 end mill` (lt1) to compass.dcv as tool t2.
Added `1/4 60 degree V-bit` (lt2) to compass.dcv as tool t3.
```

The end mill is `t2` in the project and the V-bit `t3`.

## 6. Set the boards and paste the operation

The project's stock is the female board, the light one. Measure it and set
the stock's `thickness_mm` line to:

<!-- doc-test: edit compass.dcv -->
```json
"thickness_mm": 12.0,
```

Then replace the whole `"operations"` block, from `"operations": [` down to
its closing `],`, with this, and save:

<!-- doc-test: paste compass.dcv operations -->
```json
"operations": [
  {
    "kind": "inlay",
    "id": "op1",
    "name": "compass",
    "enabled": true,
    "targets": [ "i1", "i2", "i3", "i4", "i5", "i6", "i7", "i8" ],
    "tool": "t3",
    "male_board": { "width_mm": null, "height_mm": null, "thickness_mm": null },
    "depth_mm": 4.0,
    "glue_gap_mm": 1.0,
    "plug_depth_mm": 2.0,
    "margin_mm": null,
    "outline_rounding_mm": null,
    "boundary": [ "i9" ],
    "clearing": [
      { "tool": "t2", "strategy": "offset", "direction": "climb", "entry": { "ramp": { "angle_deg": 15.0 } } }
    ],
    "step_over_fraction": 0.1,
    "machining_order": "full_depth_first",
    "through_cut_allowance_mm": 0.0,
    "safe_z_mm": 10.0,
    "cut_out": { "tool": "t2", "tabs": { "placement": { "kind": "count", "count": 4 }, "width_mm": 6.0, "height_mm": 3.0, "shape": "rect", "on_holes": false, "avoid_corners": true }, "through_cut_allowance_mm": 0.2 }
  }
],
```

This is the inlay pair's Inlay with four changes:

| Field | Value | Why |
| --- | --- | --- |
| `targets` | `i1` to `i8` | The eight pockets `image-prep` made: one female pocket each, and one male board carrying all eight plugs. |
| `boundary` | `i9` | The oval, as the male band's outer edge. |
| `margin_mm` | `null` | A boundary replaces the margin. |
| `outline_rounding_mm` | `null` | And the rounding. |

Everything else is the inlay pair's: a 4 mm deep pocket, a 1 mm glue gap,
the end mill clearing the floor and then cutting the plug free along the
oval. [Inlay](../user-guide/inlay.md) explains each value. The cut-out runs
1.588 mm inside the oval, so the freed plug is the oval less 3.175 mm on
every side, about 89.7 × 77.7 mm: draw the oval larger if the plug's size
matters to you.

The male board is the dark one. Set its thickness; the operation's
`male_board` line becomes:

<!-- doc-test: edit compass.dcv -->
```json
"male_board": { "width_mm": null, "height_mm": null, "thickness_mm": 12.0 },
```

## 7. Check and export

<!-- doc-test: run -->
```sh
dreamcarve check compass.dcv
dreamcarve export compass.dcv
```

`check` prints the warnings, a line for each board's Inlay and the programs
`export` will write (long lines shortened here):

```text
compass.dcv:22:5: warning: unused_tool: tool `t1` (`Set me`) is defined but not used by any enabled operation
warning: inlay_male_mirrored: operation `op1` (male board): it is a male inlay, so its design is cut mirrored about X = 58 mm, the middle of its targets, to seat face down in the female
warning: tool_change_avoidable: tool `t2` (`1/8 end mill`) is used by operations `op1`, which do not run one after another, so it is fitted more than once; put those operations next to each other …
Inlay op1 `compass` (female board): deepest cut 4.000 mm below the stock top, a flat floor at depth_mm 4.000 mm; V-bit floor ridges 0.400 mm along the rings and up to 0.800 mm at their corners …
Inlay op1 `compass` (male board): deepest cut 5.000 mm below the male board's top, the plug's start plane 3.000 mm below the male board's top and the surround's flat floor plug_depth_mm 2.000 mm …
Female board, the stock, 116.000 mm × 104.000 mm × 12.000 mm:
Program compass-female-1-T2.nc (T2):
  Extents (machine coordinates, mm, tool centre): X 29.866..86.134, Y 23.866..80.134, Z -4.000..10.000
    The cutter's edge reaches X 28.279..87.722, Y 22.279..81.722; keep clamps outside that.
  Estimated time: 1 min 57 s (cutting 1 min 54 s, dwells 3 s); 541 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program compass-female-2-T1.nc (T1):
  Extents (machine coordinates, mm, tool centre): X 25.524..90.476, Y 19.524..84.476, Z -4.000..10.000
    The cutter's edge reaches X 22.349..93.651, Y 16.349..87.651; keep clamps outside that.
  Estimated time: 2 min 0 s (cutting 1 min 57 s, dwells 3 s); 1.9 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Male board, 116.000 mm × 104.000 mm × 12.000 mm:
Program compass-male-1-T2.nc (T2):
  Extents (machine coordinates, mm, tool centre): X 11.588..104.412, Y 11.584..92.417, Z -5.000..10.000
    The cutter's edge reaches X 10.000..106.000, Y 9.997..94.004; keep clamps outside that.
  Estimated time: 6 min 1 s (cutting 5 min 58 s, dwells 3 s); 920 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program compass-male-2-T1.nc (T1):
  Extents (machine coordinates, mm, tool centre): X 24.369..91.631, Y 18.369..85.631, Z -5.000..10.000
    The cutter's edge reaches X 21.194..94.806, Y 15.194..88.806; keep clamps outside that.
  Estimated time: 1 min 49 s (cutting 1 min 46 s, dwells 3 s); 1.3 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program compass-male-3-T2.nc (T2):
  Extents (machine coordinates, mm, tool centre): X 11.586..104.414, Y 11.587..92.417, Z -12.200..10.000
    The cutter's edge reaches X 9.998..106.002, Y 9.999..94.004; keep clamps outside that.
  Estimated time: 1 min 20 s (cutting 1 min 17 s, dwells 3 s); 31 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Estimated total time: 13 min 7 s (cutting 12 min 52 s, dwells 15 s); 4.7 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
compass.dcv: 0 error(s), 3 warning(s)
```

The warnings are notes: `unused_tool` is the "Set me" tool `init` made;
`inlay_male_mirrored` says the male is cut mirrored, about the middle of the
stock, so that it seats face down in the female (you never mirror the drawing
yourself); `tool_change_avoidable` says the end mill is fitted twice on the
male, which the order requires.

The male's Inlay line, cut short above, ends by saying where its band
comes from: `band: drawn boundary i9, narrowest band 9.505 mm (open edge,
edge pass t2)`. That is the distance from the pockets to the oval at its
closest, the 9.5 mm of step 3. There is no `inlay_boundary_*` warning.

`export` wrote five programs, as for the inlay pair: the female's
`compass-female-1-T2.nc` and `compass-female-2-T1.nc`, and the male's
`compass-male-1-T2.nc`, `compass-male-2-T1.nc` and `compass-male-3-T2.nc`.
Every one passed the safety rules. Cut them as [An inlay
pair](inlay.md#11-cut-the-female) says: the female first, then the male,
free the plug, test fit, glue, clamp, saw off the backing and plane flush.
The ridges of the finished inlay are the compass's lines, 2.5 mm wide, as
the report said.

## Your own line art

Draw it as strokes, or trace a picture first in Inkscape
(Path → Trace Bitmap; `image-prep` reads SVG only), and follow this chapter
from step 2 with your own file:

1. `dreamcarve image-prep your-art.svg --regions`, and read the report's
   narrowest gap. If it is under about 1.5 mm at the size you will cut,
   make the art larger or its lines thicker.
2. Draw the boundary into the prepared file: an oval or any closed shape
   round everything, no fill, an id you can name.
3. `init` the prepared file, add the Inlay as above with your pockets in
   `targets` and the boundary's id.

[Preparing artwork](../user-guide/preparing-artwork.md) covers the choices:
the thresholds, gaps in the lines, page backgrounds and tracing a picture
in Inkscape.
