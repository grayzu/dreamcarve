# Your first cut from an SVG

This chapter takes a drawing of a coaster from SVG to G-code, and then to a
cut part. The coaster is a 90 mm circle cut out of plywood sold as 12 mm. Four small
**tabs** (bridges of uncut material) hold it to the board until you are done.

You will run four commands and edit one file by hand. Every command here is
run by DreamCarve's test suite exactly as written, so if it does not work for
you, that is a bug.

## What you need

- DreamCarve installed and on your `PATH`. See [Installing](installing.md).
- A text editor.
- The drawing,
  [`coaster.svg`](https://raw.githubusercontent.com/grayzu/dreamcarve/main/fixtures/coaster.svg).
  It is also in the repository at `fixtures/coaster.svg`.
- Calipers, to measure the plywood.
- For the cut: a piece of plywood sold as 12 mm, at least 110 × 110 mm, on
  a spoilboard; a 6.35 mm (1/4") two-flute upcut end mill; and a router
  controlled by grblHAL. GRBL works too; see
  [Post-processors](../user-guide/post-processors.md).

## 1. Put the drawing in a folder

Make an empty folder and put `coaster.svg` in it:

```sh
mkdir coaster
cd coaster
```

Run every command below from that folder.

## 2. Create the project

<!-- doc-test: run -->
```sh
dreamcarve init coaster.svg
```

`init` reads the drawing and writes a project file, `coaster.dcv`:

```text
Imported 1 shape(s) from coaster.svg: page units mm, 1 user unit = 1.0000 mm.
Wrote coaster.dcv: stock 110.00 × 110.00 mm, post-processor grblhal, 1 shape(s) targeted by Profile.
Values to set; export refuses until every one is set:
coaster.dcv:10:21: stock.thickness_mm is not set (mm)
coaster.dcv:24:22: tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`
coaster.dcv:25:24: tools[t1].plunge_mm_min is not set (mm/min) for tool `Set me`
coaster.dcv:26:14: tools[t1].rpm is not set (RPM) for tool `Set me`
coaster.dcv:27:23: tools[t1].step_down_mm is not set (mm) for tool `Set me`
coaster.dcv:42:19: operations[op1].depth_mm is not set (mm) for operation `Profile`
Next: edit coaster.dcv and replace each `null` listed above. To cut through, `depth_mm` is the thickness plus `through_cut_allowance_mm`. Then run `dreamcarve check coaster.dcv`.
```

Each `coaster.dcv:10:21:` is a file, a line and a column: here line 10,
column 21, where the value to replace starts. Many editors and terminals
open the file at that spot when you click it, or with a command such as
`code -g coaster.dcv:10:21`.

The project holds everything about the job: the shapes, the **stock** (the
block of material you cut from), the tool, the operation, and the
**post-processor** (the definition of the G-code dialect your controller
reads). `init` fills it with these defaults:

| Setting | Default |
| --- | --- |
| Stock | The drawing's bounds plus 10 mm on each side: 110 × 110 mm here, **no thickness** |
| Work zero | X and Y at the stock's bottom-left corner, Z at the stock top |
| Tool `t1`, named "Set me" | 6.35 mm end mill, 2 flutes, **no feed, plunge rate, spindle speed or depth per pass** |
| Operation `op1` | A Profile of every closed shape: outside the line, climb, **no depth**, passes as deep as the tool's depth per pass, a 15° ramp into each pass, safe Z 5 mm |
| Tabs | 4 tabs, 8 mm wide and 3 mm tall, square |
| Post-processor | grblHAL, millimetres |

A **Profile** cuts along a shape's outline. "Outside" puts the cutter outside
the line, so the part comes out at the size you drew. The
[Profile operation](../user-guide/profile.md) chapter explains each setting.

## 3. Try to export

<!-- doc-test: run, exit 1 -->
```sh
dreamcarve export coaster.dcv
```

This fails, on purpose:

```text
coaster.dcv:10:21: error: stock.thickness_mm is not set (mm)
coaster.dcv:24:22: error: tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`
coaster.dcv:25:24: error: tools[t1].plunge_mm_min is not set (mm/min) for tool `Set me`
coaster.dcv:26:14: error: tools[t1].rpm is not set (RPM) for tool `Set me`
coaster.dcv:27:23: error: tools[t1].step_down_mm is not set (mm) for tool `Set me`
coaster.dcv:42:19: error: operations[op1].depth_mm is not set (mm) for operation `Profile`
error: export of coaster.dcv failed; nothing was written
```

DreamCarve never guesses your material's thickness, the cut depth, the
depth per pass, a feed rate or a spindle speed. A wrong thickness puts every pass, and sometimes
every rapid move, at the wrong height; a wrong feed can break a bit or
start a fire. So `export` refuses and writes nothing until you supply every
value yourself. It lists all of them at once, so one edit can fix them all.

## 4. Set the material and the feeds

Open `coaster.dcv` in your text editor. It is a JSON file. Everything you
set is near the top, before the shapes. The file starts like this:

<!-- doc-test: excerpt coaster.dcv -->
```json
{
  "version": 8,
  "name": "coaster",
  "display_units": "mm",
  "flatten_tolerance_mm": 0.01,
  "arc_fit_tolerance_mm": 0.01,
  "stock": {
    "width_mm": 110.0,
    "height_mm": 110.0,
    "thickness_mm": null
  },
  "origin": {
    "xy": "bottom_left",
    "z": "stock_top"
  },
  "tools": [
    {
      "id": "t1",
      "name": "Set me",
      "number": null,
      "cutter": "end_mill",
      "diameter_mm": 6.35,
      "flutes": 2,
      "feed_mm_min": null,
      "plunge_mm_min": null,
      "rpm": null,
      "step_down_mm": null,
```

The operations follow the tools, then the placed shapes and the
`post_processor` block, which names grblHAL and holds its settings, and
last the shapes themselves.

### The material

Plywood sold as 12 mm is often thinner. Measure yours with calipers in a
few places. This chapter uses 11.8 mm; use your own measurement.

In the file, `"thickness_mm"` under `"stock"` and `"depth_mm"` under
`"operations"` are `null`, and `"through_cut_allowance_mm"` is `0.0`.
Change those three lines to:

<!-- doc-test: edit coaster.dcv -->
```json
"thickness_mm": 11.8,
"depth_mm": 12.0,
"through_cut_allowance_mm": 0.2,
```

- `thickness_mm` is your measurement, in millimetres.
- `depth_mm` is how deep the Profile cuts, measured down from the top of
  the plywood: the thickness plus 0.2 mm. Plywood cut exactly to its bottom
  often keeps a thin skin of fibres; going 0.2 mm into the spoilboard (the
  board under the plywood) cuts the part free.
- `through_cut_allowance_mm` allows that 0.2 mm. DreamCarve refuses any cut
  that goes further below the bottom of the stock than this.

If your measurement differs, set `thickness_mm` to it and `depth_mm` to it
plus 0.2.

### The bit

Find the tool, under `"tools"`. Its three rates and its depth per pass
are `null`:

```json
"feed_mm_min": null,
"plunge_mm_min": null,
"rpm": null,
"step_down_mm": null,
```

Change those four lines to:

<!-- doc-test: edit coaster.dcv -->
```json
"feed_mm_min": 1500.0,
"plunge_mm_min": 500.0,
"rpm": 18000,
"step_down_mm": 4.0,
```

Save the file.

- `feed_mm_min` is the **feed rate**: how fast the bit moves along the
  outline while cutting, in millimetres per minute.
- `plunge_mm_min` is the **plunge rate**: how fast it moves straight down
  into the material, in millimetres per minute. The profile ramps into
  each pass rather than plunging, but never descends faster than this.
- `rpm` is the spindle speed, in revolutions per minute.
- `step_down_mm` is the **depth per pass**: the most the bit cuts deeper
  on each trip around the outline, in millimetres. The 12 mm cut takes
  three passes of 4 mm. Keep it at most the bit's diameter, 6.35 mm here;
  `check` warns about a deeper one, which can break the bit.

The operation has a `step_down_mm` of its own, also `null`. Leave it: an
operation's value overrides the tool's, and `null` means it uses the
tool's.

These are starting values for this bit in plywood. Your machine may need
others. If you are not sure, ask the maker of your bit or your machine.

The [project file](../user-guide/project-file.md) chapter lists every
field.

## 5. Check the project

<!-- doc-test: run -->
```sh
dreamcarve check coaster.dcv
```

`check` runs every step of `export` except writing the file, and prints what
it found:

```text
Extents (machine coordinates, mm, tool centre): X 6.825..103.175, Y 6.825..103.175, Z -12.000..5.000
  The cutter's edge reaches X 3.650..106.350, Y 3.650..106.350; keep clamps outside that.
coaster.dcv: 0 error(s), 0 warning(s)
```

One of the things it checks is that the cut stays on the stock. If you move
or scale the circle so that the bit would leave the 110 × 110 mm square,
`check` reports an error naming the operation and how far past which edge
it goes, and `export` writes nothing.

## 6. Export the program

<!-- doc-test: run -->
```sh
dreamcarve export coaster.dcv
```

```text
Wrote coaster.nc (45 lines, post-processor grblhal).
  op1 `Profile`: tool `t1` (Set me), 1 target(s), 12 mm deep
Extents (machine coordinates, mm, tool centre): X 6.825..103.989, Y 6.825..103.175, Z -12.000..5.000
  The cutter's edge reaches X 3.650..107.164, Y 3.650..106.350; keep clamps outside that.
Before you start: fit T1 Set me and set work zero with it.
```

`coaster.nc` is the G-code program. The **extents** are where it takes the
bit, in the coordinates your controller shows once work zero is set: the
centre of the bit stays between X 6.825 and 103.989 mm, and its edge, half
the 6.35 mm diameter further out, between X 3.65 and 107.16 mm (Y between
3.65 and 106.35 mm: the ramp into each pass, step 7, reaches a little past
the circle on the right). Z runs from −12, the deepest cut, up to 5, safe Z. You place
clamps from these numbers in step 8.

The program does not stop for its first bit: you fit it and set work zero
with it before you start, as the last line says. A program that stops, for a second bit or for a router you
switch by hand, also lists every place where it stops and waits for you.
Your sender may not show why it stopped, so note them.

## 7. Read the start of the program

Open `coaster.nc`. It starts like this (your date will differ):

<!-- doc-test: excerpt coaster.nc -->
```text
( Project: coaster )
( Generated by DreamCarve on 2026-10-16 )
G90
G17
G94
G21
G54
( Operation 1: Profile )
( Tool 1: Set me )
G0 Z5.000
M3 S18000
G4 P3.000
X103.175 Y55.000
Z1.000
G1 X103.989 Y73.642 Z-4.000 F1500.000
X103.175 Y55.000
G2 X6.825 Y55.000 I-48.175 J0.000
```

| Lines | What they do |
| --- | --- |
| `G90 G17 G94 G21 G54` | Set the controller's state: absolute coordinates, the XY plane, feed in units per minute, millimetres, and the first work coordinate system. A controller left in another state by an earlier job is corrected. |
| `G0 Z5.000` | Rapid up to **safe Z**, 5 mm above the stock top. Rapids (fast, non-cutting moves) that go sideways happen only at or above this height. |
| `M3 S18000`, `G4 P3.000` | Start the spindle at 18000 RPM, then wait 3 seconds for it to reach speed. |
| `X103.175 Y55.000` | Rapid to the start of the cut, still at safe Z. |
| `Z1.000` | Rapid straight down to 1 mm above the stock top, where the ramp starts, so the bit does not ramp through the air. `G0` is still in effect from `G0 Z5.000`. |
| `G1 X103.989 Y73.642 Z-4.000 F1500.000` | **Ramp** down to the first pass, 4 mm deep: one straight move at 15° in the waste outside the coaster, at the feed rate. The bit enters the wood on a slope instead of drilling straight down, which is easier on the bit and leaves no plunge mark. `G1` stays in effect, so the next lines are cuts too. |
| `X103.175 Y55.000` | Back to the start of the cut, at the pass depth. |
| `G2 X6.825 Y55.000 I-48.175 J0.000` | Cut along the outline at the feed rate, as a clockwise arc (`G2`) whose centre is `I`, `J` from the start: half the circle, 48.175 mm from the centre, the circle's 45 mm radius plus half the bit. |

The drawing's `<circle>` is imported as the circle it is, two half-circle
arcs, so each lap is two `G2` moves on the exact circle the bit's centre
follows. Curves that are not circles, such as ellipses and most Bézier
curves, are imported as short straight edges instead, and `export` turns
runs of those back into arcs where it can
([Arc fitting](../user-guide/project-file.md#arc-fitting)).

The circle is cut in three passes: Z −4, −8 and −12, each ramped into
from the floor of the pass before. The bottom of the
plywood is at Z −11.8, so the last pass goes 0.2 mm into the spoilboard. On
that pass the bit rises to Z −8.8 over each tab, leaving 3 mm of wood. Then
it retracts to safe Z, stops the spindle (`M5`) and ends (`M30`).

## 8. Before you cut

Read [Safety](../user-guide/safety.md) once before your first cut. For this
job:

1. **Fix the stock.** The program uses the 110 × 110 mm square that starts
   at your XY zero. Use the extents `export` printed (step 6) to place
   clamps: the bit's edge reaches X 3.65 to 107.16 mm and Y 3.65 to
   106.35 mm from your XY zero, so keep clamps and screws outside that area,
   with room to spare for the collet nut. Your board can be bigger. If you
   change the project, export again and use the new extents.
2. **Use a spoilboard.** The last pass goes 0.2 mm below the plywood,
   into the board under it.
3. **Fit the bit and set work zero with it.** Fit the 6.35 mm bit first: a
   different bit, or the same one seated deeper, has a different length.
   Then set X and Y at the front-left corner of the square (the corner
   nearest you on the left), and Z at the top of the plywood. The program
   does not stop for this; it starts cutting from the zero you set.
4. **Do an air cut.** Before you start, set Z zero 20 mm above the
   plywood instead of on it, and run the whole program. The deepest move,
   Z −12, then stays 8 mm above the wood. Watch that the path is where you
   expect and nothing hits a clamp. Then set Z zero back on the plywood.

## 9. Cut

1. Check that the 6.35 mm bit is in and that work zero is set with it on
   the top of the plywood (step 8).
2. Load `coaster.nc` in your sender and start it. The program lifts to
   safe Z, the spindle starts, waits 3 seconds, and the cut begins. It
   does not stop until the end. Stay at the machine with the stop button
   in reach.
3. When it ends, cut the four tabs with a knife or a flush-cut saw, and sand
   the stubs.

## Next steps

- Change the depth per pass, the tab count, or the cut direction: see
  [Profile operation](../user-guide/profile.md) and
  [Tabs](../user-guide/tabs.md).
- Use GRBL, or a router you switch on by hand: see
  [Post-processors](../user-guide/post-processors.md).
- Draw your own part: see
  [Shapes, holes, and islands](../user-guide/shapes-and-holes.md) for how
  DreamCarve decides what is a part and what is a hole.
