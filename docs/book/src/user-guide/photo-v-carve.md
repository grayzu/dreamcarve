# Photo V-Carve operation

A **Photo V-Carve** carves a picture with a V-bit, as rows of straight,
parallel grooves whose depth follows the picture's brightness: deep where
it is dark, not cut at all where it is white. A deep groove is wide and
casts a dark shadow; a shallow one is narrow and leaves the wood's own
colour, so from a step back the grooves read as the picture. It is
VCarve's "Photo V-Carve Toolpath", and what RasterCarve does.

It carves one picture in the project, a **bitmap**: [Images](images.md)
says how to choose and prepare a picture and put it in a project.
[Carving a photo](../quick-start/photo.md) goes through it once, end to
end.

The commands in this chapter are run by the test suite as written, on
the synthetic picture `tests/fixtures/photo/gradient.png`: black at its
left edge to white at its right, with a white 8 × 8 pixel block in its
dark half and a black one in its light half.

## Adding one by hand

`init` on a picture writes a Photo V-Carve for it:

<!-- doc-test: run -->
```sh
dreamcarve init gradient.png --width 100
```

<!-- doc-test: contains gradient.dcv -->
```json
    {
      "kind": "photo_v_carve",
      "id": "op1",
      "name": "Photo V-Carve",
      "enabled": true,
      "image": "b1",
      "tool": "t1",
      "max_depth_mm": null,
      "start_depth_mm": 0.0,
      "start_plane_cleared_externally": false,
      "line_angle_deg": 22.5,
      "line_spacing_fraction": 1.1,
      "strategy": "raster",
      "invert": false,
      "sample_mm": 0.25,
      "stepover_retract_mm": 1.0,
      "through_cut_allowance_mm": 0.0,
      "safe_z_mm": 5.0
    }
```

To carve a picture you added to another project with `dreamcarve image
add`, copy this block into its `operations` list, with an `id` no other
operation has, `image` set to the bitmap's id (`image list` shows it) and
`tool` to a V-bit of the project. A Photo V-Carve has no `targets`: it
carves the whole picture.

Set the values `init` leaves unset. This chapter's are example values for
a 60° 6.35 mm V-bit:

<!-- doc-test: edit gradient.dcv -->
```json
"thickness_mm": 12.0,
"feed_mm_min": 1000.0,
"plunge_mm_min": 300.0,
"rpm": 18000,
"max_depth_mm": 1.5,
```

## Settings

| Field | VCarve | RasterCarve | Meaning |
| --- | --- | --- | --- |
| `kind` | | | `"photo_v_carve"`. |
| `id`, `name`, `enabled` | | | As every operation's. |
| `image` | | the picture | The id of the bitmap to carve, such as `"b1"`. |
| `tool` | | | The id of a V-bit, with an angle above 0° and below 180°. |
| `max_depth_mm` | Max Carving Depth | `MAX_DEPTH` | How deep black is carved below the start plane, above 0 and at most the bit's cone height. Never defaulted: `init` leaves it `null`. See [Picking a bit and a depth](#picking-a-bit-and-a-depth). |
| `start_depth_mm` | | | How far below the stock top the carve starts, 0 or more. 0 carves the stock top. See [Start depth](#start-depth). |
| `start_plane_cleared_externally` | | | `true` says another program cleared the start plane. See [Start depth](#start-depth). |
| `line_angle_deg` | Line Angle | `LINE_ANGLE` | The grooves' direction, counterclockwise from the X axis: 0 is along X, 90 along Y. At least 0 and less than 180. `init` writes 22.5. |
| `line_spacing_fraction` | Line Spacing (a percentage) | `STEPOVER` (a percentage) | How far apart the grooves are, as a fraction of a groove's width at `max_depth_mm`: 1.1 is 110 %. Above 0; below 1 warns. `init` writes 1.1. |
| `strategy` | Raster, Hatch | the carve strategy | `"raster"`, one set of grooves, or `"hatch"`, a second set at right angles after the first. |
| `invert` | | | `false`: black is deepest. `true`: white is deepest, for a dark wood. See [Dark and light wood](#dark-and-light-wood). |
| `sample_mm` | | `LINEAR_RESOLUTION` | How far apart along a groove the picture is read, above 0. `init` writes 0.25. |
| `stepover_retract_mm` | Stepover Retract | | How far above the start plane the bit lifts between grooves, above 0. `init` writes 1. |
| `through_cut_allowance_mm` | | | As every operation's; 0 for a carve. |
| `safe_z_mm` | | | The height above the stock top the bit travels at before the first groove and after the last. |

## How the grooves are laid

- **Straight and parallel**, at `line_angle_deg`, across the whole
  picture and clipped to its footprint: nothing is cut outside the
  picture, and no part of it is missed.
- **One groove through the picture's top-left corner**, the others
  spaced `line_spacing_fraction` times the full-depth width apart,
  measured at right angles to them. At 22.5° the groove through the
  corner touches the picture only at that corner: it is a single plunge.
- **Cut alternately each way**, one groove one way and the next back, so
  the bit never travels back empty.
- **Between grooves the bit rises** `stepover_retract_mm` above the start
  plane and moves over to the next groove's start at the feed rate, then
  plunges at the plunge rate. It is never a rapid: a rapid only runs at
  safe Z ([Safety](safety.md), rule 2), and the bit is just above the
  wood.
- **With `"hatch"`**, a second set of grooves at `line_angle_deg + 90°`
  follows the first, laid the same way. It fills in the ridges between
  the first set's grooves for a denser, darker picture, at about twice
  the time.

## Depth and brightness

Along each groove the picture is read every `sample_mm`, and at the
groove's end. The brightness at a point is blended from the four nearest
pixels, so a groove runs smoothly across pixels rather than in steps. A
point of brightness `g` (0 black to 255 white) is cut

```text
depth = (1 − g / 255) × max_depth_mm
```

below the start plane: black the full depth, 50 % gray half of it, white
not at all, where the bit just touches the surface. Points in line are
joined into one move, so a stretch of even brightness is one straight
cut.

### Dark and light wood

On a light wood (maple, birch, poplar, MDF) a groove reads darker than
the surface: its walls are in shadow. So black is carved deepest by
default, and the carving looks like the picture.

On a dark wood (walnut), or a painted board whose cuts show the light
wood under the paint, the cut reads lighter than the surface. Set
`"invert": true`: then white is carved deepest and black not at all,

```text
depth = (g / 255) × max_depth_mm
```

and the carving again looks like the picture, not its negative.

## Picking a bit and a depth

A groove `d` deep with a bit of included angle `α` is `2 · d · tan(α /
2)` wide. At the full depth, the **full-depth groove width** is

```text
W = 2 · max_depth_mm · tan(α / 2)
```

and the grooves are `line_spacing_fraction × W` apart. So the bit's angle
and `max_depth_mm` together set how wide the darkest grooves are, how
many grooves there are, and how long the carve takes.

The bit's **cone height**, `diameter / (2 × tan(α / 2))`, is as deep as
its sloped sides go; below it the straight shank would cut. `max_depth_mm`
deeper than the cone height is refused with `v_bit_cone_exceeded`. For
the common 1/4" (6.35 mm) bits:

| Angle | Cone height | Groove width per mm of depth | `W` at 1.5 mm |
| --- | --- | --- | --- |
| 15° | 24.1 mm | 0.26 mm | 0.395 mm |
| 30° | 11.8 mm | 0.54 mm | 0.804 mm |
| 60° | 5.5 mm | 1.15 mm | 1.732 mm |
| 90° | 3.175 mm | 2.00 mm | 3.000 mm |

- **Start at 1 to 2 mm deep.** That is enough for a clear picture in a
  light wood, and within every common bit's cone. Deeper gives darker
  shadows and wider grooves, not more detail.
- **A narrow bit (30°, 15°) gives finer grooves**, more of them, closer
  together: more detail, more time. Its fine tip is fragile, so keep the
  feed modest.
- **A wide bit (90°) gives coarse grooves**, fewer and faster: good for a
  large picture seen from across a room. Its cone is 3.175 mm tall, so
  keep `max_depth_mm` below that.
- **The ridge between grooves.** At `line_spacing_fraction` 1 two
  full-depth grooves meet in a knife edge at the surface. Above 1 a flat
  strip `(line_spacing_fraction − 1) × W` wide is left between them,
  0.173 mm for the 60° bit at 1.5 mm and 1.1. Below 1 the darkest grooves
  overlap, cut each other's ridge away, and the darkest parts lose
  contrast: `photo_lines_overlap` warns.

## Detail and time

Every setting trades detail against time. For the 100 × 50 mm gradient,
1.5 mm deep at 1000 mm/min:

| Change from the quick start's settings | Grooves | Cutting time |
| --- | --- | --- |
| None: 60° bit, spacing 1.1, 22.5°, raster | 45 | 3 min 9 s |
| A 90° bit | 26 | 1 min 52 s |
| A 30° bit | 96 | 6 min 36 s |
| `line_spacing_fraction` 1.5 | 33 | 2 min 21 s |
| `"hatch"` | 104 | 6 min 24 s |

The time grows with the area of the picture and falls with the spacing:
twice the width and height is about four times the time, and half the
spacing about twice it. `check` and `export` print the estimate.

`sample_mm` sets how finely the picture is read along each groove, not
how many grooves there are: a smaller step follows the picture more
closely and makes a longer program, but hardly changes the cutting time.
The default, 0.25 mm, is finer than a common bit's groove is wide at
1 mm deep or more. A step too small for
the picture is refused, so that a program cannot grow without bound: no
Photo V-Carve may take more than 2 000 000 moves (see [When a Photo
V-Carve is refused](#when-a-photo-v-carve-is-refused)).

## What `check` prints

<!-- doc-test: run -->
```sh
dreamcarve check gradient.dcv
```

```text
Photo V-Carve op1 `Photo V-Carve`: picture `gradient` (b1), 100.000 mm by 50.000 mm at (10.000 mm, 10.000 mm), 45 grooves 1.732 mm wide at the full depth of 1.500 mm.
Extents (machine coordinates, mm, tool centre): X 10.000..110.000, Y 10.000..60.000, Z -1.500..5.000
  The cutter's edge reaches X 6.825..113.175, Y 6.825..63.175; keep clamps outside that.
Estimated time: 3 min 12 s (cutting 3 min 9 s, dwells 3 s); 9 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
gradient.dcv: 0 error(s), 0 warning(s)
```

The first line is the Photo V-Carve's, and `export` prints it too:

- the picture it carves, by name and id;
- its **footprint**: its width and height on the stock, and its
  bottom-left corner;
- the **number of grooves** (with `"hatch"`, both sets together);
- the **full-depth groove width**, `W`, and the full depth,
  `max_depth_mm`. The grooves are `line_spacing_fraction × W` apart.

Then, as for every project, the extents (the tool centre stays on the
picture; the cutter's edge reaches the bit's radius beyond it) and the
time.

`check --preview <svg>` draws the picture on the stock with the grooves
over it ([Toolpath preview](overview.md#toolpath-preview)).

## Start depth

The depths are measured from the **start plane**, `start_depth_mm` below
the stock top. 0 carves the stock top, which is what you want for a
picture on a board. To carve a picture on the floor of a pocket, put a
Pocket of that depth, whose area holds the whole picture, before the
Photo V-Carve, and set `start_depth_mm` to its depth, as a [V-Carve's
start depth](v-carve.md#start-depth) works:

- A Pocket, or a V-Carve's flat floor, earlier in the list that holds the
  picture's footprint at another depth warns `start_depth_mismatch`. Set
  `start_depth_mm` to the floor's depth.
- A start depth above 0 with no such floor is the error
  `start_depth_uncleared`.
- If another program cleared the start plane, set
  `start_plane_cleared_externally` to `true`. The project cannot see that
  material, so the bit's flank is counted from the stock top, the start
  depth plus `max_depth_mm` must fit the cone, and the bit crosses
  between grooves no lower than 1 mm above the stock top.

## Arc fitting does not apply

Arc fitting (`arc_fit_tolerance_mm`, [The project
file](project-file.md#arc-fitting)) is never applied to a Photo V-Carve.
Its grooves are straight, and their points change depth from one to the
next, so they lie on no arc: the program is straight `G1` moves only.

## The picture's edge and the stock

A groove at the edge of the picture is centred on the edge, so at full
depth it reaches **half its width** past the picture, `W / 2`. The rest
of the bit's cone, above the cut, reaches up to the bit's radius past
it. That is why `init` puts the picture 10 mm in from the stock's edge.

The rule that keeps every cut on the stock (S7, [Safety](safety.md),
rule 9) holds each groove to the stock at the width it is cut at the
stock top. A picture placed so close to the edge that a dark groove would
run off it is refused, naming the edge and how far past it the groove
goes. A light edge, carved shallow, needs less room than a dark one.

## When a Photo V-Carve is refused

`check` and `export` refuse a Photo V-Carve that would be unsafe, and
`export` then writes nothing. Each refusal names what to change. Here is
each one, made on purpose and put right again.

**Too many moves.** No Photo V-Carve may take more than 2 000 000 moves,
so that a mistyped setting, or a shared project with a tiny step, cannot
build a program that never ends. Set `sample_mm` to 0.001:

<!-- doc-test: edit gradient.dcv -->
```json
"sample_mm": 0.001,
```

<!-- doc-test: run, exit 1 -->
```sh
dreamcarve check gradient.dcv
```

```text
error: operation `op1` would carve its picture in about 2031646 moves with sample_mm 0.001, more than the 2000000 a Photo V-Carve may make; raise sample_mm or line_spacing_fraction, or make the picture smaller
gradient.dcv: 1 error(s), 0 warning(s)
```

Raise `sample_mm` (0.1 to 0.5 mm suits any picture) or
`line_spacing_fraction`, or carve the picture smaller. Put it back:

<!-- doc-test: edit gradient.dcv -->
```json
"sample_mm": 0.25,
```

**A start plane nothing clears.** Set `start_depth_mm` to 1:

<!-- doc-test: edit gradient.dcv -->
```json
"start_depth_mm": 1.0,
```

<!-- doc-test: run, exit 1 -->
```sh
dreamcarve check gradient.dcv
```

```text
gradient.dcv:51:25: error: operation `Photo V-Carve` (`op1`) starts 1 mm below the stock top, but no enabled Pocket, nor the flat floor of an enabled V-Carve, before it contains its picture's footprint; add a Pocket of that depth before it or, if another program clears the start plane, set start_plane_cleared_externally to true
gradient.dcv: 1 error(s), 0 warning(s)
```

The project has nothing that removes the top millimetre over the
picture, so the bit would be plunged and moved across in wood the
project cannot see (`start_depth_uncleared`). Add a Pocket 1 mm deep over
the picture before the Photo V-Carve; or, if you cleared it some other
way, say so:

<!-- doc-test: edit gradient.dcv -->
```json
"start_plane_cleared_externally": true,
```

<!-- doc-test: run -->
```sh
dreamcarve check gradient.dcv
```

```text
Photo V-Carve op1 `Photo V-Carve`: picture `gradient` (b1), 100.000 mm by 50.000 mm at (10.000 mm, 10.000 mm), 45 grooves 1.732 mm wide at the full depth of 1.500 mm.
Extents (machine coordinates, mm, tool centre): X 10.000..110.000, Y 10.000..60.000, Z -2.500..5.000
  The cutter's edge reaches X 6.825..113.175, Y 6.825..63.175; keep clamps outside that.
Estimated time: 3 min 23 s (cutting 3 min 20 s, dwells 3 s); 10 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
gradient.dcv: 0 error(s), 0 warning(s)
```

It is accepted, and DreamCarve trusts you: the bit now goes 2.5 mm below
the stock top. Put both back:

<!-- doc-test: edit gradient.dcv -->
```json
"start_depth_mm": 0.0,
"start_plane_cleared_externally": false,
```

**A groove off the stock.** Move the picture to the stock's left edge:

<!-- doc-test: edit gradient.dcv -->
```json
"x_mm": 0.0,
```

<!-- doc-test: run, exit 1 -->
```sh
dreamcarve check gradient.dcv
```

```text
error: operation `op1` cuts off the stock: 0.866 mm past the -X (left) edge
gradient.dcv: 1 error(s), 0 warning(s)
```

The picture itself is on the stock, but its left edge is black, carved
1.5 mm deep, and a groove there is 1.732 mm wide: half of it, 0.866 mm,
is past the edge (S7). Move the picture in, or make the stock bigger. Put
it back:

<!-- doc-test: edit gradient.dcv -->
```json
"x_mm": 10.0,
```

**Deeper than the cone.** Set `max_depth_mm` to 6:

<!-- doc-test: edit gradient.dcv -->
```json
"max_depth_mm": 6.0,
```

<!-- doc-test: run, exit 1 -->
```sh
dreamcarve check gradient.dcv
```

```text
gradient.dcv:50:23: error: operation `Photo V-Carve` (`op1`) engages tool `Set me` 6 mm deep, but the bit's cone is only 5.499 mm tall; below that the shank cuts, so use a shallower depth or a wider bit
gradient.dcv: 1 error(s), 0 warning(s)
```

The 60° bit's sides reach its full width 5.5 mm down; deeper, its shank
would cut (`v_bit_cone_exceeded`). Use a shallower depth, or a bit with
a taller cone. Put it back:

<!-- doc-test: edit gradient.dcv -->
```json
"max_depth_mm": 1.5,
```

**Grooves that overlap.** This one is a warning, not a refusal. Set
`line_spacing_fraction` to 0.9:

<!-- doc-test: edit gradient.dcv -->
```json
"line_spacing_fraction": 0.9,
```

<!-- doc-test: run -->
```sh
dreamcarve check gradient.dcv
```

```text
gradient.dcv:54:32: warning: photo_lines_overlap: operation `Photo V-Carve` (`op1`) has line_spacing_fraction 0.9, less than 1, so its grooves overlap where the picture is darkest and no ridge is left between them; use 1 or more to keep the ridges
Photo V-Carve op1 `Photo V-Carve`: picture `gradient` (b1), 100.000 mm by 50.000 mm at (10.000 mm, 10.000 mm), 55 grooves 1.732 mm wide at the full depth of 1.500 mm.
Extents (machine coordinates, mm, tool centre): X 10.000..110.000, Y 10.000..60.000, Z -1.500..5.000
  The cutter's edge reaches X 6.825..113.175, Y 6.825..63.175; keep clamps outside that.
Estimated time: 3 min 52 s (cutting 3 min 49 s, dwells 3 s); 9 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
gradient.dcv: 0 error(s), 1 warning(s)
```

The program is still written. Put it back to 1.1 for a ridge between
the darkest grooves:

<!-- doc-test: edit gradient.dcv -->
```json
"line_spacing_fraction": 1.1,
```

**A groove off the picture, or a rapid between grooves (S19).** The
last check is on the written program, and DreamCarve's own programs
always pass it: every groove stays inside the picture, between the start
plane and `max_depth_mm` below it, and every move between two grooves is
fed at the link height. If `export` ever refuses a program with "emitted
line … takes operation …'s V-bit to (…, …) mm, outside its picture or
deeper than its max_depth_mm" or "emitted line … moves operation …'s
V-bit between two grooves as a rapid or away from its link height", that
is a bug in DreamCarve: report it, with the project.

## Safety

Every Photo V-Carve program passes the [safety](safety.md) rules. Those
that matter most here:

- **S19, the grooves stay in the picture** ([Safety](safety.md), rule
  16): every cut stays inside the picture's footprint and between the
  start plane and `max_depth_mm` below it, and every move between grooves
  is a feed move at `stepover_retract_mm` above the start plane.
- **S17, the shank stays out** (rule 14): the bit's flank is engaged no
  deeper than its cone, counted from the start plane when an earlier
  Pocket or V-Carve floor clears it, and from the stock top otherwise.
  The picture's edge is not a wall: the groove may run past it by half
  its width, which S7 keeps on the stock.
- **S7, the cut stays on the stock** (rule 9): each groove, widened to its
  width at the stock top.

## Errors

These stop `check` and `export`. Each names the operation and the field.

| Code | Message says | What to do |
| --- | --- | --- |
| `photo_unknown_bitmap` | "operation … carves bitmap …, which does not exist" | Set `image` to a bitmap of the project; `dreamcarve image list` shows them. |
| `operation_value_unset` | "… max_depth_mm is not set" | Set `max_depth_mm`. |
| `v_bit_cone_exceeded` | "operation … engages tool … … mm deep, but the bit's cone is only … mm tall …", with "from the stock top (its start depth, which no Pocket or V-Carve floor of the project clears, plus its max_depth_mm)" when the start plane is not cleared | Use a smaller `max_depth_mm` or a bit with a taller cone; clear the start depth with a Pocket. |
| `v_carve_needs_v_bit`, `v_bit_angle_range` | The tool is not a V-bit, or its angle is not above 0 and below 180. | Use a V-bit. |
| `photo_line_angle_range` | "… has line_angle_deg …; the grooves' angle is 0 degrees or more and less than 180" | Use 0 to 179.99; 180 is the same as 0. |
| `photo_line_spacing_positive` | "… has line_spacing_fraction …; the spacing of the grooves is a fraction of a groove's width, above 0" | Use 1 or more, usually 1.1. |
| `photo_sample_positive` | "… has sample_mm … mm; the distance between samples along a groove is above 0" | Use 0.1 to 0.5. |
| `photo_retract_positive` | "… has stepover_retract_mm … mm; the lift between grooves is above 0" | Use 1. |
| `start_depth_negative` | "… has start_depth_mm … mm; the start plane is a depth below the stock top, 0 or more" | Use 0 or more. |
| `start_depth_uncleared` | "… starts … mm below the stock top, but no enabled Pocket, nor the flat floor of an enabled V-Carve, before it contains its picture's footprint; …" | See [Start depth](#start-depth). |

Export also stops at a picture that would take more than 2 000 000
moves, and at a groove off the stock, as shown above. Problems with the
picture itself (its size, its PNG) are the bitmap's, listed in
[Warnings: Photo V-Carve](warnings.md#photo-v-carve).

## Warnings

| Code | When |
| --- | --- |
| `photo_lines_overlap` | `line_spacing_fraction` is below 1, so the darkest grooves overlap. |
| `start_depth_mismatch` | An earlier Pocket or V-Carve floor holds the picture's footprint at another depth than `start_depth_mm`: "… the Pocket before it whose area contains its picture's footprint, is … mm deep …". |

## Limits

- One picture per operation, carved whole: no part of a picture, and no
  vector boundary to carve it inside (PhotoVCarve's "Selected Vectors"
  strategy).
- No picture editing: contrast, cropping and rotation are done in an
  image editor first ([Images](images.md#choosing-a-picture)).
- The picture cannot be rotated: turn it before you add it. The grooves
  can run at any angle.
- Brightness is a groove's depth, never a surface: carving a picture as
  a relief is a later phase's.
- No ball-nose or engraving bit; a V-bit only.
- One pass: there is no step-down, so a deep picture in a hard wood is
  cut in one pass at its full depth. Keep `max_depth_mm` modest in hard
  woods, or slow the feed.
