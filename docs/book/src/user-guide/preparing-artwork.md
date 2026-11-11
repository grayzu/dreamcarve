# Preparing artwork

Preparing the drawing is the hardest part of making an inlay. A drawing
that looks right on screen can still trip DreamCarve: letters overlap, the
hole in a "D" is a separate path, line art is drawn as strokes, and hidden
layers, specks and hairlines come along unseen. DreamCarve helps in two
ways:

- `init` and `check` **tell you** what they found, in the drawing, as
  warnings. They change nothing ([the checks](#the-checks-init-prints)).
- `dreamcarve image-prep` **fixes it**, if you ask: it reads one SVG and
  writes a clean one ([`image-prep`](#image-prep)).

Nothing requires `image-prep`. `init` takes any SVG, and a clean drawing
from a CAD or drawing program needs none of it. It changes the art (it
merges shapes, drops specks, turns lines into areas), so you choose when to
run it.

To follow the chapter on a drawing that has every problem in it, use
`tests/fixtures/prep/messy.svg` from the repository, a synthetic 100 mm
drawing: two overlapping squares, a square inside a square drawn as two
paths, a stroked line, a hidden layer, a line of text, a picture, a clipped
group and a speck. Put it in an empty folder and run the commands from there.
For a whole project, see the quick start [From line art](../quick-start/line-art.md).

## What goes wrong in drawings

| In the drawing | What DreamCarve sees | Fix |
| --- | --- | --- |
| Letters or shapes overlap | Outlines that cross, which are an error where an operation cuts both | Unite them in the drawing program, or `image-prep` |
| The hole in a "D" is its own path, filled | Two shapes; the inner is a hole only if an operation targets both | Make it one path with a hole, or `image-prep` |
| Line art is drawn as strokes | Only each stroke's centre line, an open line no inlay can cut | Turn strokes into areas: Path → Stroke to Path, or `image-prep` |
| A hidden layer, or an object with no paint | Its geometry is imported anyway and would be cut | Delete it, or `image-prep` |
| Live text, a picture, a clip path or a mask | They cannot become shapes; they are skipped, or imported unclipped | Convert text to paths; trace pictures; expand clips and masks |
| Specks and hairlines, as traced art has | Tiny shapes, a few hundredths of a millimetre across | `image-prep` drops them |
| A page background, a rectangle behind the art | One shape that covers the art | Delete it in the drawing program |
| Gaps in the outlines of line art | Pockets that are not closed enclose nothing | Close the line in the drawing program |

## The checks `init` prints

Every time it reads an SVG, `init` looks for the problems of the table and
prints one warning for each kind it finds, as `warning: <code>: <message>`,
before it writes the project. The message says what it found, where, and
what to do. A clean drawing prints none. The checks change nothing: `init`
writes the same project, and exits with the same status, with or without
them.

<!-- doc-test: run -->
```sh
dreamcarve init messy.svg -o messy.dcv
```

```text
Imported 9 shape(s) from messy.svg: page units mm, 1 user unit = 1.0000 mm.
warning: shapes_overlap: 1 pair(s) of shapes overlap; the first is `sq-a` and `sq-b`, meeting near (18.00, 95.00) mm; unite them or move them apart; `dreamcarve image-prep` would fix this
warning: svg_hole_as_separate_path: 1 filled path(s) lie wholly inside another filled path; the first is `inner` inside `outer`. They are separate shapes, a hole only where an operation targets both; make each one path with a hole; `dreamcarve image-prep` would fix this
warning: svg_stroke_only_path: 1 path(s) have a stroke and no fill; the first is `centreline`, stroked 1.200 mm wide. Only its centreline is imported, an open path no Inlay or V-Carve can cut; turn strokes into shapes; `dreamcarve image-prep` would fix this
warning: svg_hidden_object: 1 object(s) are not drawn (display:none) but are imported and would be cut; the first is `old-layer`; skip them; `dreamcarve image-prep` would fix this
warning: svg_object_not_convertible: 1 text element(s) skipped; convert text to paths in your drawing program
warning: svg_object_not_convertible: 1 image(s) skipped; trace images in Inkscape first
warning: svg_object_not_convertible: 1 clip path(s) ignored, the first on `clipped`; their shapes are imported unclipped; expand clips and masks in your drawing program
warning: feature_below_size: 1 piece(s) or hole(s) are narrower than 0.200 mm at their widest, 0.2% of the design's 100.0 mm height; the smallest is 0.087 mm. Specks that small are not worth carving; `dreamcarve image-prep` would fix this
```

(The warnings print on standard error and the `Imported` line on standard
output, so a terminal may interleave them differently.) Each code, and what
to do about it:

| Code | Found | What to do |
| --- | --- | --- |
| `shapes_overlap` | Outlines of different paths cross. The message names the first pair and where they meet, and counts the pairs. | Unite the shapes (Path → Union in Inkscape) or move them apart; or `image-prep`, which merges paths that cross. An outline that crosses itself is already an error ([Shapes, holes, and islands](shapes-and-holes.md#11-a-shape-must-not-cross-itself)); this warns earlier, at `init`, before an operation exists. |
| `svg_hole_as_separate_path` | A filled path lies wholly inside another filled path. The message names both. They are two shapes in the project; the inner is a hole only in an operation that targets both, as [Shapes, holes, and islands](shapes-and-holes.md) says of every nested outline, and a Pocket of the outer path alone would cut across it. An unfilled frame drawn round a design is not reported. | Make each one path with a hole (Path → Combine in Inkscape), or `image-prep`, which does. |
| `svg_stroke_only_path` | A path has a stroke and no fill. `init` imports its centre line, an open path that no Inlay or V-Carve can cut. The message gives the stroke's width. | Path → Stroke to Path, or `image-prep`, which turns every stroke into the area it covers, with its own width, caps and joins. |
| `svg_hidden_object` | An object is not drawn: `display:none`, `visibility:hidden`, zero opacity, or neither filled nor stroked. `init` imports geometry whatever its paint, so a hidden layer would be cut. | Delete it, or `image-prep`, which skips such objects. |
| `svg_object_not_convertible` | Live text, an image, a clip path or a mask. Text and images are skipped; shapes inside a clip path or mask are imported whole. | Convert text to paths (Path → Object to Path); trace pictures first ([Tracing a picture](#tracing-a-picture-in-inkscape)); apply clips and masks in the drawing program (Object → Clip → Release, then cut the shapes). `image-prep` skips these too, and counts them. |
| `feature_below_size` | Pieces or holes narrower, at their widest, than the default `--min-detail` ([below](#the-two-thresholds)) of the design's height. The message counts them and gives the smallest. | Nothing is cut wrongly, but a speck that small is never worth carving, and traced art is full of them. `image-prep` drops them. |

`check` repeats the two that can still be read from a project's shapes,
`shapes_overlap` and `feature_below_size`; the others need the drawing's
paint, so only `init` can print them. `check` on a freshly made project
also stops at the values `init` left for you to set, and exits 1 until you
set them.

The same codes are listed, with every other warning, in
[Warnings](warnings.md#import).

## `image-prep`

```text
dreamcarve image-prep design.svg [-o design-inlay.svg] [--regions]
                      [--min-detail <pct>] [--smooth <pct>]
```

`image-prep` reads one SVG and writes one SVG that is easy to inlay, and
prints a report. It makes a clean drawing and nothing else: it has no tool,
stock, depth, feed or size option, and what it writes is the same whatever
project you use it in later. Scale the result in a drawing program, or with
the margin `init` gives the stock, and run `init` on it as always.

What it does, in this order:

1. **Visible geometry only.** Hidden layers, unpainted and zero-opacity
   objects are skipped, and counted. Text, images, clip paths and masks
   cannot be converted: they are skipped, counted and named in warnings.
2. **Strokes become areas.** Each stroked path becomes the filled outline
   of its stroke, using the stroke's own width, caps, joins, miter limit and
   dashes, under the path's transform.
3. **Fills are resolved.** Each path's own fill rule (nonzero or even-odd)
   decides what it fills. Paths that cross are merged. A path wholly inside
   another's area is a hole of it, as `init` reads every nested outline, so
   a ring drawn as two paths comes out as one piece with a hole. Colour
   never decides what is a shape. What remains are separate **pieces**, each
   with its holes.
4. **The mode.** Shapes mode, the default, makes the filled shapes the
   inlay: lettering and logos. Regions mode, `--regions`, makes the spaces
   between the lines the inlay ([below](#line-art-and---regions)).
5. **Clean-up.** Specks and hairlines are dropped, tiny holes filled, and
   jagged edges straightened ([the thresholds](#the-two-thresholds)).
6. **Write.** One filled `<path>` per piece, holes included. No strokes,
   text, images, clip paths, masks, gradients or hidden objects remain.

The file goes where `-o` says; by default it is the drawing's name with
`-inlay` added, in the same folder. It **never overwrites** a file that
exists: it stops and says so. The output has the input's page size, in
millimetres, and every shape keeps its place, so the result lies exactly
over the original.

<!-- doc-test: run -->
```sh
dreamcarve image-prep messy.svg
```

```text
warning: svg_hole_as_separate_path: 1 filled path(s) lie wholly inside another filled path; the first is `inner` inside `outer`. Each is a hole of the path that contains it, as `init` reads nested paths
warning: svg_stroke_only_path: 1 path(s) have a stroke and no fill; the first is `centreline`, stroked 1.200 mm wide. Their strokes were made into areas
warning: svg_hidden_object: 1 object(s) are not drawn (display:none) and were skipped; the first is `old-layer`
warning: svg_object_not_convertible: 1 text element(s) skipped; convert text to paths in your drawing program
warning: svg_object_not_convertible: 1 image(s) skipped; trace images in Inkscape first
warning: svg_object_not_convertible: 1 clip path(s) ignored, the first on `clipped`; their shapes are imported unclipped; expand clips and masks in your drawing program
warning: prep_speck_dropped: 1 piece(s) narrower than 0.200 mm were dropped, the widest 0.093 mm; lower `--min-detail` to keep them
Prepared messy.svg → messy-inlay.svg (shapes)
  Read: 8 paths, of them 1 in hidden layers skipped; 1 text, 1 images, 1 clip paths, 0 masks
  Strokes made into areas: 1, 1.20 mm wide
  Design height: 100.00 mm
  Result: 4 pieces, 1 hole
  Dropped: 1 piece below 0.20 % of the height (0.200 mm)
  Thinnest piece: 1.20 mm (1.20 % of the height)
  Narrowest gap: 19.52 mm (19.52 %)
  At 100 / 200 / 300 mm tall: thinnest piece 1.20 / 2.40 / 3.60 mm; narrowest gap 19.52 / 39.04 / 58.56 mm
  The thinnest piece reaches 1.5 mm at 125 mm tall.
Wrote messy-inlay.svg
```

The warnings come first, then the report. The same `init` on the result
prints none of the checks, and imports one shape per piece:

<!-- doc-test: run -->
```sh
dreamcarve init messy-inlay.svg -o clean.dcv
```

```text
Imported 4 shape(s) from messy-inlay.svg: page units mm, 1 user unit = 1.0000 mm.
Wrote clean.dcv: stock 100.36 × 120.00 mm, post-processor grblhal, 4 shape(s) targeted by Profile.
Values to set; export refuses until every one is set:
…
```

(The overlapping squares are one piece, the stroke is a piece, the square
with its hole is a piece, and the speck is gone.)

Running it again for the same output stops, so nothing you edited is lost:

<!-- doc-test: run, exit 1 -->
```sh
dreamcarve image-prep messy.svg
```

```text
error: messy-inlay.svg already exists; remove it or choose another file with -o
```

### Circles come back as polygons

The output writes curves as straight segments, within 0.005 % of the
design's height (0.01 mm on a 200 mm tall design). A circle in the input is
a many-sided polygon in the output. That is invisible in a cut, but it
matters for one thing: **a drill hole is not a polygon**. A [Drill](drill.md)
takes its holes from circles in the drawing, so take them from the
original drawing, not the prepared one: keep the original, and `init` it
for the drilling, or add the circles to the prepared drawing again.

### A page background

If one filled shape covers nearly all of the drawing and has the art inside
or in front of it, probably a page background, `image-prep` warns
`prep_background_shape` and names it. It is not removed: it is a shape like
any other, so the art inside it comes out as holes of one big rectangle,
which is rarely what you want. Delete the rectangle in the drawing program
and run `image-prep` again. (There is no option to skip a shape by id yet.)

## Line art and `--regions`

Line art is a drawing made of lines: a trace, an engraving, a
compass. In shapes mode the inlay is the ink itself, the lines, and a
line 1 mm wide is a thin sliver of dark wood in a light board, which
glues poorly. Usually what you want is the other way round: dark pockets
**between** the lines, and the lines standing as ridges of the light
board. That is what `--regions` does:

- the art's outline, the ink's outer boundary with its holes filled, less
  the ink, is the inlay: one **piece** per enclosed space;
- the ink stays as the board between the pockets, so the **narrowest gap**
  in the report is the ridge between two pockets;
- the Inlay cuts the result unchanged: one female pocket per piece, and one
  male board carrying all the plugs.

When the ink is mostly thin line work, with a median width under 2 % of
the design's height, `image-prep` in the default mode suggests it:
`prep_regions_suggested`.

<!-- doc-test: run -->
```sh
dreamcarve image-prep messy.svg --regions -o messy-regions.svg
```

```text
…
warning: prep_regions_open_ink: 4 piece(s) of ink enclose no space, so regions mode leaves nothing to cut there; close the lines in your drawing program if a space was meant
Prepared messy.svg → messy-regions.svg (regions)
  Read: 8 paths, of them 1 in hidden layers skipped; 1 text, 1 images, 1 clip paths, 0 masks
  Strokes made into areas: 1, 1.20 mm wide
  Design height: 100.00 mm
  Result: 1 piece, 0 holes
  Thinnest piece: 5.00 mm (5.00 % of the height)
  At 100 / 200 / 300 mm tall: thinnest piece 5.00 / 10.00 / 15.00 mm
  The thinnest piece reaches 1.5 mm at 30 mm tall.
Wrote messy-regions.svg
```

The messy drawing is not line art, which is why `--regions` finds almost
nothing in it, and says so. In a real line drawing the same warning means
what it says.

### Gaps in the lines

A pocket exists only where lines close it. A line with a gap in it, a ring
broken in one place, encloses nothing: the space inside is joined to the
outside, and `--regions` makes no pocket there. The report counts the pieces
of ink that enclose no space, in `prep_regions_open_ink`. If a space was
meant, **close the line in the drawing program** (join the ends, or draw a
short stroke over the gap) and prepare again. There is no option to close
gaps automatically yet.

### Drawing the boundary after

An inlay of line art usually has a drawn boundary, a closed shape round the
art that is the male board's band edge ([Inlay](inlay.md#the-boundary)). The
boundary is a thin line, and `image-prep` would drop a line that thin as a
hairline (and, in regions mode, the space between the oval and the art
would become a piece of its own). So prepare the line art **without** the
boundary, and draw the boundary into the **prepared** file:

1. `dreamcarve image-prep art.svg --regions` on the line art alone.
2. In a drawing program, open `art-inlay.svg` and draw a closed shape
   round the art: an ellipse with no fill and a thin stroke, with an id you
   can name (in Inkscape: Edit → XML Editor).
3. `init` the prepared file; the boundary is the shape with that id.
4. List it as the Inlay's `boundary`.

`init` warns `svg_stroke_only_path` for the boundary, because a line with no
fill is a stroke. That is expected here: the Inlay uses the shape's outline,
and the outline is what `init` imports. [From line
art](../quick-start/line-art.md) does exactly this.

## The two thresholds

Both thresholds are a percentage of the **design's height**, the height of
the visible geometry's bounding box, strokes included; the report prints
that height, so you can see what the percentage came to in millimetres.
Each runs from 0 to 5, and 0 turns it off. A value outside the range is the
error `prep_threshold_range`, exit status 2, and nothing is written.

<!-- doc-test: run, exit 2 -->
```sh
dreamcarve image-prep messy.svg --min-detail 6 -o messy-coarse.svg
```

```text
error: prep_threshold_range: --min-detail 6 is not between 0 and 5 percent of the design's height
```

**`--min-detail <pct>`** drops a piece, and fills a hole, whose widest
point is under the threshold. The widest point is the diameter of the
largest circle that fits in it, so a speck is dropped, and a hairline,
which is nowhere wider than its thread, is dropped with it. Nothing above
the threshold is thinned or widened: a narrow neck inside a bigger piece is
reported ([below](#the-report)), not changed. The report counts what was
dropped and filled (`prep_speck_dropped`, `prep_hole_filled`) and gives the
widest one, so you see whether the threshold took something that mattered.
The default is 0.2 %: 0.2 mm on a 100 mm tall design. Raise it for art
that traced rough; lower it, or give 0, to keep fine detail.

**`--smooth <pct>`** replaces runs of tiny segments with a straight one
where the result stays within the threshold of the original outline. That
takes the staircase out of traced edges, which would otherwise be cut as a
staircase. It never rounds a corner: a corner between two long segments
stays, and the result never crosses itself or another piece. A piece where
smoothing would make it cross is left as it was, and `prep_smooth_skipped`
counts those. The default is 0.05 %, 0.05 mm on a 100 mm tall design.
A `--smooth` above half of a non-zero `--min-detail` warns
`prep_smooth_exceeds_detail`, because smoothing that large could remove
what the other step keeps.

Because the thresholds are relative, the same art gives the same result at
any size you draw it, and the report tells you what they come to.

## The report

`image-prep` prints this after its warnings. It is information: nothing in
it is enforced.

- **Read** says what was in the file and what was left out: hidden objects,
  and the counts of text, images, clip paths and masks.
- **Strokes made into areas** counts them and gives the range of their
  widths.
- **Design height** is the height all percentages are of.
- **Result** counts the pieces and their holes.
- **Dropped** says what the thresholds took: pieces below the minimum
  detail, holes filled, and how far smoothing moved edges.
- **Thinnest piece** is how wide the narrowest piece is typically, the
  median of its widths along its length, so a tapering tip does not count,
  in millimetres and as a share of the height.
- **Narrowest gap** is the narrowest space between pieces. In regions mode
  it is called **the board ridge between pockets**: the narrowest strip of
  light board that will stand between two plugs.
- **At 100 / 200 / 300 mm tall** gives the same two widths at those
  heights, because you decide the size afterwards.
- The last line says **at what height each width reaches 1.5 mm**.

### Reading the numbers

The 1.5 mm is a guide, not a limit. It is about what a V-bit inlay holds
well: a plug narrower than that is fragile to cut and to glue, and a ridge
of board narrower than that chips. Below it the cut still works if the
bit, the wood and the glue allow, but the report is telling you the
feature is narrower than you probably want. Use it like this:

- Decide how tall the finished design will be. Look at the line for the
  nearest of 100, 200 or 300 mm, or scale: widths grow in proportion to
  the height.
- In regions mode, read the **narrowest gap**: it is your thinnest ridge.
  In shapes mode, read the **thinnest piece**.
- If the number at your size is under about 1.5 mm, make the design larger,
  thicken the lines in the drawing, or accept a more delicate cut and test
  it on scrap first.
- The compass in [From line art](../quick-start/line-art.md) has a gap of
  2.50 mm, 3.57 % of its 70 mm height: the report says it reaches 1.5 mm
  at 42 mm tall, so it cuts well at its 70 mm size.

## Tracing a picture in Inkscape

`image-prep` reads SVG only. A PNG or JPEG, known by its extension or by
the first bytes of the file, is refused with the exit status 2 and the
code `prep_not_svg`, and the message says to trace it first. Tracing is the
conversion of a picture to outlines, and DreamCarve does not do it yet.
(To carve the picture itself, its shades as V-bit grooves rather than its
outlines as shapes, do not trace it: `dreamcarve init` takes a PNG or
JPEG for a [Photo V-Carve](photo-v-carve.md); see [Images](images.md).)
To trace it:

1. Open the picture in Inkscape (File → Open), and select it.
2. Path → Trace Bitmap. Try "Brightness cutoff" for a black and white
   drawing, with a threshold that makes the lines solid, or "Multiple
   scans" for several levels; the preview shows the result.
3. Click Apply, close the dialog, and delete the original picture, which
   Inkscape leaves under the trace.
4. Save as plain SVG, and run `image-prep` on it. A trace is full of
   specks and ragged edges, which the thresholds are for.

A trace of line art is usually a set of closed outlines of the ink, so
`--regions` works on it directly.

## Errors

| Code | Meaning | What to do |
| --- | --- | --- |
| `prep_not_svg` | A PNG or JPEG was given (usage error, exit 2). | Trace it in Inkscape first. |
| `prep_threshold_range` | `--min-detail` or `--smooth` is outside 0 to 5 (usage error, exit 2). | Give a value from 0 to 5. |
| `prep_nothing_left` | Nothing visible is left to prepare: the drawing is empty, hidden, or every piece fell below `--min-detail`. | Check the drawing's layers; lower `--min-detail`. |
| `prep_output_unreadable` | The prepared shapes would not read back as the shapes they were made as. This is a defect in DreamCarve, not in your drawing, and nothing is written. | Report it, with the drawing if you can share it. |

All of `image-prep`'s warnings, and the checks above, are in
[Warnings](warnings.md#import).

## Then

Scale the result in a drawing program to the size you will cut (or give
`init` its margin), run `init`, and add the operation: an [Inlay](inlay.md)
for the shapes or regions, with a [boundary](inlay.md#the-boundary) if you
want one; or a [V-Carve](v-carve.md) for lettering.
