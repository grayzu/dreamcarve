# Your first cut from an SVG

This chapter takes a drawing of a coaster from SVG to G-code, and then to a
cut part. The coaster is a 90 mm circle cut out of 12 mm plywood. Four small
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
- For the cut: a piece of 12 mm plywood at least 110 × 110 mm, a 6.35 mm
  (1/4") two-flute upcut end mill, and a router controlled by grblHAL. GRBL
  works too; see [Post-processors](../user-guide/post-processors.md).

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
Wrote coaster.dcv: stock 110.00 × 110.00 × 12.00 mm, post-processor grblhal, 1 shape(s) targeted by Profile.
Next: edit coaster.dcv to set the stock thickness and the Profile depth (`operations[op1].depth_mm`) to match your material, and tool `t1` (Set me): feed_mm_min, plunge_mm_min and rpm. Export refuses until they are set.
```

The project holds everything about the job: the shapes, the **stock** (the
block of material you cut from), the tool, the operation, and the
**post-processor** (the definition of the G-code dialect your controller
reads). `init` fills it with these defaults:

| Setting | Default |
| --- | --- |
| Stock | The drawing's bounds plus 10 mm on each side, 12 mm thick: 110 × 110 × 12 mm here |
| Work zero | X and Y at the stock's bottom-left corner, Z at the stock top |
| Tool `t1`, named "Set me" | 6.35 mm end mill, 2 flutes, **no feed, plunge rate or spindle speed** |
| Operation `op1` | A Profile of every closed shape: outside the line, climb, 12 mm deep in 4 mm passes, safe Z 5 mm |
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
warning: tool `Set me` (`t1`) has no feed_mm_min; export fails until it is set
warning: tool `Set me` (`t1`) has no plunge_mm_min; export fails until it is set
warning: tool `Set me` (`t1`) has no rpm; export fails until it is set
error: tool `Set me` has no usable feed rate (feed_mm_min)
error: export of coaster.dcv failed; nothing was written
```

DreamCarve never guesses a feed rate or spindle speed. A wrong guess can
break a bit or start a fire, so `export` refuses and writes nothing until
you supply every value yourself.

## 4. Set the feeds

Open `coaster.dcv` in your text editor. It is a JSON file. Find the tool,
under `"tools"`. Its three rates are `null`:

```json
"feed_mm_min": null,
"plunge_mm_min": null,
"rpm": null,
```

Change those three lines to:

<!-- doc-test: edit coaster.dcv -->
```json
"feed_mm_min": 1500.0,
"plunge_mm_min": 500.0,
"rpm": 18000,
```

Save the file.

- `feed_mm_min` is the **feed rate**: how fast the bit moves along the
  outline while cutting, in millimetres per minute.
- `plunge_mm_min` is the **plunge rate**: how fast it moves straight down
  into the material, in millimetres per minute.
- `rpm` is the spindle speed, in revolutions per minute.

These are starting values for this bit in plywood. Your machine may need
others. If you are not sure, ask the maker of your bit or your machine.

### If your plywood is not 12.0 mm

Plywood sold as 12 mm is often thinner. Measure yours with calipers. If it
differs, change two more lines: `"thickness_mm"` under `"stock"`, and
`"depth_mm"` under `"operations"`, both to your measurement. The
[project file](../user-guide/project-file.md) chapter lists every field.

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
Wrote coaster.nc (995 lines, post-processor grblhal).
  op1 `Profile`: tool `t1` (Set me), 1 target(s), 12 mm deep
Extents (machine coordinates, mm, tool centre): X 6.825..103.175, Y 6.825..103.175, Z -12.000..5.000
  The cutter's edge reaches X 3.650..106.350, Y 3.650..106.350; keep clamps outside that.
Before you start: fit T1 Set me and set work zero with it.
```

`coaster.nc` is the G-code program. The **extents** are where it takes the
bit, in the coordinates your controller shows once work zero is set: the
centre of the bit stays between X 6.825 and 103.175 mm, and its edge, half
the 6.35 mm diameter further out, between X 3.65 and 106.35 mm (Y the
same). Z runs from −12, the deepest cut, up to 5, safe Z. You place clamps
from these numbers in step 8.

The program does not stop for its first bit: you fit it and set work zero
with it before you start, as the last line says. A program that stops, for a second bit or for a router you
switch by hand, also lists every place where it stops and waits for you.
Your sender may not show why it stopped, so note them.

## 7. Read the start of the program

Open `coaster.nc`. It starts like this (your date will differ):

<!-- doc-test: excerpt coaster.nc -->
```text
( coaster )
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
X103.139 Y56.863
G1 Z-4.000 F500.000
X103.174 Y55.063 F1500.000
```

| Lines | What they do |
| --- | --- |
| `G90 G17 G94 G21 G54` | Set the controller's state: absolute coordinates, the XY plane, feed in units per minute, millimetres, and the first work coordinate system. A controller left in another state by an earlier job is corrected. |
| `G0 Z5.000` | Rapid up to **safe Z**, 5 mm above the stock top. Rapids (fast, non-cutting moves) happen only at or above this height. |
| `M3 S18000`, `G4 P3.000` | Start the spindle at 18000 RPM, then wait 3 seconds for it to reach speed. |
| `X103.139 Y56.863` | Rapid to the start of the cut, still at safe Z. |
| `G1 Z-4.000 F500.000` | Plunge to the first pass, 4 mm deep, at the plunge rate. |
| `X103.174 Y55.063 F1500.000` | Cut along the outline at the feed rate. |

The circle is cut in three passes: Z −4, −8 and −12. On the last pass the
bit rises to Z −9 over each tab, leaving 3 mm of wood. Then it retracts to
safe Z, stops the spindle (`M5`) and ends (`M30`).

## 8. Before you cut

Read [Safety](../user-guide/safety.md) once before your first cut. For this
job:

1. **Fix the stock.** The program uses the 110 × 110 mm square that starts
   at your XY zero. Use the extents `export` printed (step 6) to place
   clamps: the bit's edge reaches X 3.65 to 106.35 mm and Y 3.65 to
   106.35 mm from your XY zero, so keep clamps and screws outside that area,
   with room to spare for the collet nut. Your board can be bigger. If you
   change the project, export again and use the new extents.
2. **Use a spoilboard.** The cut goes right down to the bottom of the
   plywood.
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
