# A pocketed plaque from DXF

This chapter takes a plaque drawn in a CAD program from DXF to two G-code
programs, and then to a cut part. The plaque is 150 × 100 mm, cut out of
plywood sold as 12 mm, with five operations:

- a recessed **panel**, 4 mm deep, with a raised circle (an **island**)
  left standing in it;
- a 30 mm **square recess**, 6 mm deep, with square corners so a square
  block fits it;
- four **screw holes**, drilled through;
- the **outline**, cut out with four tabs;
- an engraved **border** and **squiggle**, with a smaller bit.

You will take two tools from a tool library, paste the operations into
the project, and run two programs, one per bit. If you have not cut
anything with DreamCarve yet, do [Your first cut from an SVG](first-cut.md)
first: this chapter assumes you know what `init`, `check` and `export` do
and how to read a `check` message.

Every command here is run by DreamCarve's test suite exactly as written,
and the excerpts are checked against the programs it writes, so if it does
not work for you, that is a bug.

## What you need

- DreamCarve installed and on your `PATH`. See [Installing](installing.md).
- A text editor.
- The drawing,
  [`plaque.dxf`](https://raw.githubusercontent.com/grayzu/dreamcarve/main/tests/fixtures/plaque.dxf).
  It is also in the repository at `tests/fixtures/plaque.dxf`, beside
  `plaque.svg`, the same plaque drawn as an SVG.
- Calipers, to measure the plywood.
- For the cut: a piece of Baltic birch plywood sold as 12 mm, at least
  170 × 120 mm, on a spoilboard; a 6.35 mm (1/4") and a 3.175 mm (1/8")
  two-flute upcut end mill; and a router controlled by grblHAL.

## 1. Put the drawing in a folder

Make an empty folder and put `plaque.dxf` in it:

```sh
mkdir plaque
cd plaque
```

Run every command below from that folder.

## 2. Create the project

<!-- doc-test: run -->
```sh
dreamcarve init plaque.dxf --units mm
```

```text
Imported 10 shape(s) from plaque.dxf: units mm (from --units), 1 drawing unit = 1.0000 mm.
Wrote plaque.dcv: stock 170.00 × 120.00 mm, post-processor grblhal, 9 shape(s) targeted by Profile.
1 shape(s) are open paths and are not cut; `dreamcarve check` lists them.
Values to set; export refuses until every one is set:
plaque.dcv:10:21: stock.thickness_mm is not set (mm)
plaque.dcv:24:22: tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`
plaque.dcv:25:24: tools[t1].plunge_mm_min is not set (mm/min) for tool `Set me`
plaque.dcv:26:14: tools[t1].rpm is not set (RPM) for tool `Set me`
plaque.dcv:27:23: tools[t1].step_down_mm is not set (mm) for tool `Set me`
plaque.dcv:50:19: operations[op1].depth_mm is not set (mm) for operation `Profile`
Next: edit plaque.dcv and replace each `null` listed above. To cut through, `depth_mm` is the thickness plus `through_cut_allowance_mm`. Then run `dreamcarve check plaque.dcv`.
```

A DXF says which units it was drawn in, in a header value called
`$INSUNITS`. `plaque.dxf` says millimetres, so `--units mm` changes
nothing here; it is in the command because many CAD programs leave the
value out, and then `init` stops and asks for it. With `--units` you say
what you drew in, and the project is in millimetres either way.
[Importing DXF](../user-guide/importing-dxf.md) has the details.

Each shape is named after its DXF layer, and gets an **instance** id
(`i1`, `i2`, …) in the order the file lists them. The operations below
refer to the shapes by these ids:

| Id | Name | What it is |
| --- | --- | --- |
| `i1` | `outline` | The plaque's 150 × 100 mm outline, with rounded corners |
| `i2` | `pocket` | The 100 × 50 mm panel, with rounded corners |
| `i3` | `island` | The 20 mm circle inside the panel |
| `i4` | `square` | The 30 × 30 mm square |
| `i5` to `i8` | `holes-1` to `holes-4` | Four 5 mm circles, one near each corner: one block inserted four times |
| `i9` | `border` | A rounded rectangle 5 mm inside the outline |
| `i10` | `squiggle` | An open curve, a DXF `SPLINE` |

`init` wrote one Profile of every closed shape, and a tool named "Set me"
with no feeds, as for the coaster. You replace the Profile with five
operations in step 5, and use two tools from your library instead of
"Set me".

## 3. Add the tools from your library

The **tool library** is where you keep the tools you own, set up once, so
each project takes a copy instead of having every value typed in again
([Tool library](../user-guide/tool-library.md)). Yours is empty, so make
a library file with your two end mills. Save this as `my-tools.json` in
the folder:

<!-- doc-test: write my-tools.json -->
```json
{
  "version": 1,
  "tools": [
    {
      "id": "lt1",
      "source": null,
      "tool": {
        "id": "lt1",
        "name": "1/4 end mill",
        "number": 1,
        "cutter": "end_mill",
        "diameter_mm": 6.35,
        "flutes": 2,
        "feed_mm_min": 1500.0,
        "plunge_mm_min": 500.0,
        "rpm": 18000,
        "step_down_mm": 4.0,
        "step_over_fraction": 0.4
      }
    },
    {
      "id": "lt2",
      "source": null,
      "tool": {
        "id": "lt2",
        "name": "1/8 end mill",
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
    }
  ]
}
```

Each tool has the values the coaster's tool needed (feed, plunge rate,
spindle speed, depth per pass), and two more:

- `step_over_fraction` is the **stepover** a Pocket uses: how far apart
  its passes across the floor are, as a fraction of the bit's diameter.
  0.4 with the 6.35 mm bit is 2.54 mm.
- `number` is the tool's number on your machine. Programs name the bit as
  `T1` or `T2` from it, and so do the file names.

These are starting values for these bits in plywood. Your machine may
need others.

Add the file's tools to your library, and list the library:

<!-- doc-test: run -->
```sh
dreamcarve tools import my-tools.json
dreamcarve tools list
```

```text
`1/4 end mill`: imported (lt1)
`1/8 end mill`: imported (lt2)
2 imported, 0 already present, 0 replaced; library: /Users/you/Library/Application Support/dreamcarve/tools.json
ID   NAME          CUTTER    DIAMETER_MM  FEED_MM_MIN  PLUNGE_MM_MIN  RPM    NUMBER  SOURCE
lt1  1/4 end mill  end mill  6.35         1500         500            18000  1       -
lt2  1/8 end mill  end mill  3.175        1000         300            18000  2       -
```

The library is a file in your config directory; `dreamcarve config path`
prints where. If you keep your tools in Fusion 360, export that library
instead and import it the same way: `dreamcarve tools import` reads a
Fusion `.tools` or `.json` file as well, with each tool's cutting presets
([Fusion 360 tool libraries](../user-guide/tool-library.md#fusion-360-tool-libraries)).

Now copy both tools into the project, by their library ids:

<!-- doc-test: run -->
```sh
dreamcarve tools add plaque.dcv lt1
dreamcarve tools add plaque.dcv lt2
```

```text
Added `1/4 end mill` (lt1) to plaque.dcv as tool t2.
Added `1/8 end mill` (lt2) to plaque.dcv as tool t3.
```

A tool's name works too, in quotes when it has spaces:
`dreamcarve tools add plaque.dcv "1/4 end mill"`. The project now has
three tools: `t1` ("Set me", which nothing will use), `t2` (the 6.35 mm
bit) and `t3` (the 3.175 mm bit). The project keeps its own copies:
changing your library later never changes this project.

## 4. Set the material

Measure the plywood with calipers in a few places. This chapter uses
11.8 mm; use your own measurement. Open `plaque.dcv` and change the
stock's `thickness_mm` line to:

<!-- doc-test: edit plaque.dcv -->
```json
"thickness_mm": 11.8,
```

The Drill and the cut-out in the next step go 12 mm deep: 11.8 mm plus
0.2 mm into the spoilboard, so the holes and the outline go right
through. If your measurement differs, set both `depth_mm` values of 12.0
to it plus 0.2 after you paste them.

## 5. Paste the operations

In `plaque.dcv`, select the whole `"operations"` block: from the line
`"operations": [` down to its closing `],` (the line before
`"instances"`). Replace it with this, and save:

<!-- doc-test: paste plaque.dcv operations -->
```json
"operations": [
  {
    "kind": "pocket",
    "id": "op1",
    "name": "Panel",
    "enabled": true,
    "targets": [ "i2", "i3" ],
    "tool": "t2",
    "strategy": { "raster": { "angle_deg": 0.0 } },
    "wall_pass": "last",
    "depth_mm": 4.0,
    "step_down_mm": null,
    "step_over_fraction": null,
    "direction": "climb",
    "allowance_mm": 0.0,
    "rest_of": null,
    "entry": { "helix": { "diameter_fraction": 0.8, "angle_deg": 15.0 } },
    "corner_relief": "none",
    "machining_order": "full_depth_first",
    "through_cut_allowance_mm": 0.0,
    "safe_z_mm": 15.0
  },
  {
    "kind": "pocket",
    "id": "op2",
    "name": "Square recess",
    "enabled": true,
    "targets": [ "i4" ],
    "tool": "t2",
    "strategy": "offset",
    "wall_pass": "last",
    "depth_mm": 6.0,
    "step_down_mm": null,
    "step_over_fraction": null,
    "direction": "climb",
    "allowance_mm": 0.0,
    "rest_of": null,
    "entry": { "ramp": { "angle_deg": 15.0 } },
    "corner_relief": "dogbone",
    "machining_order": "full_depth_first",
    "through_cut_allowance_mm": 0.0,
    "safe_z_mm": 15.0
  },
  {
    "kind": "drill",
    "id": "op3",
    "name": "Screw holes",
    "enabled": true,
    "targets": [ "i5", "i6", "i7", "i8" ],
    "tool": "t2",
    "cycle": { "peck": { "peck_depth_mm": 4.0, "retract": "full" } },
    "dwell_s": 0.0,
    "depth_mm": 12.0,
    "retract_mm": 1.0,
    "through_cut_allowance_mm": 0.2,
    "safe_z_mm": 15.0
  },
  {
    "kind": "profile",
    "id": "op4",
    "name": "Cut out",
    "enabled": true,
    "targets": [ "i1" ],
    "tool": "t2",
    "side": "outside",
    "depth_mm": 12.0,
    "step_down_mm": null,
    "direction": "climb",
    "through_cut_allowance_mm": 0.2,
    "safe_z_mm": 15.0,
    "tabs": {
      "placement": { "kind": "count", "count": 4 },
      "width_mm": 8.0,
      "height_mm": 3.0,
      "shape": "rect",
      "on_holes": false
    },
    "machining_order": "full_depth_first",
    "entry": { "ramp": { "angle_deg": 15.0 } },
    "lead": { "radius_mm": 3.0, "sweep_deg": 90.0, "lead_out": true, "overcut_mm": 1.0 },
    "corner_relief": "none"
  },
  {
    "kind": "engrave",
    "id": "op5",
    "name": "Border and squiggle",
    "enabled": true,
    "targets": [ "i9", "i10" ],
    "tool": "t3",
    "depth_mm": 1.0,
    "step_down_mm": 1.0,
    "alternate_direction": false,
    "machining_order": "full_depth_first",
    "through_cut_allowance_mm": 0.0,
    "safe_z_mm": 15.0
  }
],
```

Every operation has the fields its chapter lists; these are the ones that
matter here:

| Operation | What it cuts | Settings worth knowing |
| --- | --- | --- |
| `op1` Pocket "Panel" | Clears the panel 4 mm deep, round the circle | `targets` names the panel and the circle: a shape inside a Pocket's shape is an island and is left standing. `raster` at 0° clears in straight lines along X; the wall pass (`"last"`) then runs round the wall and the island to clean up the line ends. The bit spirals down (`helix`) instead of plunging. See [Pocket operation](../user-guide/pocket.md). |
| `op2` Pocket "Square recess" | Clears the square 6 mm deep | `offset` clears in rings stepping in from the wall. `dogbone` notches each corner so a square block fits; a round bit alone leaves a 3.175 mm radius there. A 15° ramp takes the bit down. |
| `op3` Drill "Screw holes" | Drills the four circles through | 12 mm deep in pecks of 4 mm, lifting fully out of the hole between pecks (`retract: "full"`) to clear the chips. Each hole starts at the retract height, 1 mm above the plywood. See [Drill operation](../user-guide/drill.md). |
| `op4` Profile "Cut out" | Cuts the plaque out, outside its outline | Through the plywood into the spoilboard, held by four tabs. A 15° ramp in the waste takes the bit down, and a 3 mm arc (`lead`) brings it onto the outline and off it, so the edge has no mark where each pass started. See [Profile operation](../user-guide/profile.md). |
| `op5` Engrave "Border and squiggle" | Grooves along the border and the squiggle, 1 mm deep | The bit's centre follows the line, open or closed, in one pass. It uses `t3`, the 3.175 mm bit. See [Engrave operation](../user-guide/engrave.md). |

Every operation uses safe Z 15 mm: the bit travels 15 mm above the
plywood between cuts. The first four use `t2`, the last `t3`.

**The order matters.** Export cuts the operations in the order they are
listed, never regrouped. The pockets and holes come before the outline:
they are cut while the plaque is still part of the board. The Engrave
comes last because it is the only cut with the small bit, so you change
bits once. See [Machining order and cut
order](../user-guide/machining-order.md).

## 6. Check the project

<!-- doc-test: run -->
```sh
dreamcarve check plaque.dcv
```

```text
plaque.dcv:17:5: warning: unused_tool: tool `t1` (`Set me`) is defined but not used by any enabled operation
warning: drill_tool_wider_than_hole: operation `op3`: the 6.35 mm tool is wider than the 5 mm hole `holes-1` (instance `i5`, profile 0), so the hole comes out the tool's size
warning: drill_tool_wider_than_hole: operation `op3`: the 6.35 mm tool is wider than the 5 mm hole `holes-2` (instance `i6`, profile 0), so the hole comes out the tool's size
warning: drill_tool_wider_than_hole: operation `op3`: the 6.35 mm tool is wider than the 5 mm hole `holes-3` (instance `i7`, profile 0), so the hole comes out the tool's size
warning: drill_tool_wider_than_hole: operation `op3`: the 6.35 mm tool is wider than the 5 mm hole `holes-4` (instance `i8`, profile 0), so the hole comes out the tool's size
Program 1 (T1):
  Extents (machine coordinates, mm, tool centre): X 6.825..163.175, Y 6.825..116.175, Z -12.000..15.000
    The cutter's edge reaches X 3.650..166.350, Y 3.650..119.350; keep clamps outside that.
  Estimated time: 3 min 2 s (cutting 2 min 59 s, dwells 3 s); 1.4 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program 2 (T2):
  Extents (machine coordinates, mm, tool centre): X 15.000..155.000, Y 15.000..105.000, Z -1.000..15.000
    The cutter's edge reaches X 13.412..156.588, Y 13.412..106.588; keep clamps outside that.
  Estimated time: 34 s (cutting 31 s, dwells 3 s); 179 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Estimated total time: 3 min 35 s (cutting 3 min 29 s, dwells 6 s); 1.6 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
plaque.dcv: 0 error(s), 5 warning(s)
```

No errors: the project exports. Each warning names what it is about:

- `unused_tool`: the "Set me" tool `init` made is not used by any
  operation. It does no harm; delete its block from `"tools"` if you like
  (and the warning goes away).
- `drill_tool_wider_than_hole`: the drawing's circles are 5 mm across and
  the bit is 6.35 mm, so each hole comes out 6.35 mm: clearance for an M5
  screw. A Drill never moves sideways, so a hole is always the bit's size.

`check` gives the extents of each program separately, because you fit a
different bit for each.

## 7. Export the programs

<!-- doc-test: run -->
```sh
dreamcarve export plaque.dcv
```

```text
plaque.dcv:17:5: warning: unused_tool: tool `t1` (`Set me`) is defined but not used by any enabled operation
warning: drill_tool_wider_than_hole: operation `op3`: the 6.35 mm tool is wider than the 5 mm hole `holes-1` (instance `i5`, profile 0), so the hole comes out the tool's size
warning: drill_tool_wider_than_hole: operation `op3`: the 6.35 mm tool is wider than the 5 mm hole `holes-2` (instance `i6`, profile 0), so the hole comes out the tool's size
warning: drill_tool_wider_than_hole: operation `op3`: the 6.35 mm tool is wider than the 5 mm hole `holes-3` (instance `i7`, profile 0), so the hole comes out the tool's size
warning: drill_tool_wider_than_hole: operation `op3`: the 6.35 mm tool is wider than the 5 mm hole `holes-4` (instance `i8`, profile 0), so the hole comes out the tool's size
Wrote 2 programs; run them in this order:
  1. plaque-1-T1.nc  Before you start: fit T1 1/4 end mill and set work zero (X, Y and Z) with it.
  2. plaque-2-T2.nc  Before you start: fit T2 1/8 end mill and set Z zero with it. Keep X and Y zero.
Post-processor grblhal.
  op1 `Panel`: tool `t2` (1/4 end mill), 2 target(s), 4 mm deep
  op2 `Square recess`: tool `t2` (1/4 end mill), 1 target(s), 6 mm deep
  op3 `Screw holes`: tool `t2` (1/4 end mill), 4 target(s), 12 mm deep
  op4 `Cut out`: tool `t2` (1/4 end mill), 1 target(s), 12 mm deep
  op5 `Border and squiggle`: tool `t3` (1/8 end mill), 2 target(s), 1 mm deep
plaque-1-T1.nc (514 lines):
  Extents (machine coordinates, mm, tool centre): X 6.825..163.175, Y 6.825..116.175, Z -12.000..15.000
    The cutter's edge reaches X 3.650..166.350, Y 3.650..119.350; keep clamps outside that.
  Estimated time: 3 min 2 s (cutting 2 min 59 s, dwells 3 s); 1.4 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
plaque-2-T2.nc (49 lines):
  Extents (machine coordinates, mm, tool centre): X 15.000..155.000, Y 15.000..105.000, Z -1.000..15.000
    The cutter's edge reaches X 13.412..156.588, Y 13.412..106.588; keep clamps outside that.
  Estimated time: 34 s (cutting 31 s, dwells 3 s); 179 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Estimated total time: 3 min 35 s (cutting 3 min 29 s, dwells 6 s); 1.6 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
```

The project uses two bits, so export writes two programs, one per bit,
numbered in the order to run them: `plaque-1-T1.nc` with the 6.35 mm bit
(the two pockets, the holes and the cut-out), then `plaque-2-T2.nc` with
the 3.175 mm bit (the engraving). `T1` and `T2` are the tools' numbers.
Each program's estimated time, and the total, leave out the rapids and
the bit change between programs; see
[Estimated run time](../user-guide/configuration.md#estimated-run-time).

The **Before you start** line of each program says what to do before you
press start:

- **Program 1:** "fit T1 1/4 end mill and set work zero (X, Y and Z) with
  it". Set X and Y at the front-left corner of the stock, and Z on the
  top of the plywood, with that bit in the collet. The program does not
  stop for this; it starts cutting from the zero you set.
- **Program 2:** "fit T2 1/8 end mill and set Z zero with it. Keep X and
  Y zero." The second bit is a different length, so Z must be set again,
  on the top of the plywood, with the new bit. X and Y stay where they
  were: moving them would put the engraving in the wrong place. Do not
  set Z on the panel or the recess; they are lower than the top.

On grblHAL you can have one program instead, which stops at `M6` for you
to change the bit:
`dreamcarve export plaque.dcv --one-program` writes `plaque.nc`. See
[One program with M6](../user-guide/post-processors.md#one-program-with-m6)
for what your sender has to support. This chapter uses two programs,
which work with any sender.

## 8. Read the programs

`plaque-1-T1.nc` starts like this (your date will differ):

<!-- doc-test: excerpt plaque-1-T1.nc -->
```text
( Project: plaque )
( Generated by DreamCarve on 2026-10-21 )
G90
G17
G94
G21
G54
( Operation 1: Panel )
( Tool 1: 1/4 end mill )
G0 Z15.000
M3 S18000
G4 P3.000
X41.752 Y97.949
Z1.000
G1 X41.425 Y97.950 Z0.912 F1500.000
X41.101 Y97.908 Z0.825
X40.784 Y97.825 Z0.737
X40.481 Y97.702 Z0.649
X40.196 Y97.541 Z0.561
X39.934 Y97.345 Z0.474
X39.700 Y97.117 Z0.386
X39.497 Y96.861 Z0.298
X39.328 Y96.580 Z0.211
X39.197 Y96.281 Z0.123
```

The first lines set up the controller and start the spindle, as in the
coaster's program. The bit rapids above the panel's helix at safe Z, then
straight down to 1 mm above the plywood (`Z1.000`), where the helix
begins: each line goes a little further round a 5.08 mm circle (0.8 of
the bit's diameter) and a little lower, 15° down from the horizontal, at
the feed rate. A ramp or helix always starts 1 mm above the wood, or 1 mm
above the floor the pass before cut, so the bit does not spiral down
through air. The bit comes down three times in the panel, once on each
side of the circle and once for the ring round the circle; it follows
the last line into the ring round the panel without lifting.

The holes are drilled like this, the first of the four:

<!-- doc-test: contains plaque-1-T1.nc -->
```text
( Operation 3: Screw holes )
( Tool 1: 1/4 end mill )
X27.000 Y27.000
Z1.000
G1 Z-3.000 F500.000
G0 Z1.000
Z-2.500
G1 Z-7.000
G0 Z1.000
Z-6.500
G1 Z-11.000
G0 Z1.000
Z-10.500
G1 Z-12.000
G0 Z15.000
```

The bit rapids to the retract height, 1 mm above the plywood, and drills
the first peck at the plunge rate, 500 mm/min: 4 mm below the retract
height, 3 mm into the wood. It lifts back to the retract height to clear
the chips, drops quickly to 0.5 mm above where it stopped, and drills the
next peck, and so on to 12 mm below the top: 0.2 mm into the spoilboard.
Then it rises to safe Z for the next hole.

`plaque-2-T2.nc` starts like this:

<!-- doc-test: excerpt plaque-2-T2.nc -->
```text
( Project: plaque )
( Generated by DreamCarve on 2026-10-21 )
G90
G17
G94
G21
G54
( Operation 1: Border and squiggle )
( Tool 2: 1/8 end mill )
G0 Z15.000
M3 S18000
G4 P3.000
X20.000 Y105.000
Z1.000
G1 Z-1.000 F300.000
X150.000 F1000.000
G2 X155.000 Y100.000 I0.000 J-5.000
G1 Y20.000
G2 X150.000 Y15.000 I-5.000 J0.000
```

It has the same header and starts the spindle again: a program never
assumes another ran before it. The engraving plunges straight down, so
the bit rapids to 1 mm above the plywood (`Z1.000`) and plunges at the
plunge rate only from there, as it does before a ramp or helix.

## 9. Before you cut

Read [Safety](../user-guide/safety.md) once before your first cut. For
this plaque:

1. **Fix the stock.** Both programs use the 170 × 120 mm rectangle that
   starts at your XY zero. Place clamps from the extents `export` printed
   for both programs: the edge of the 6.35 mm bit reaches X 3.65 to
   166.35 mm and Y 3.65 to 119.35 mm, so it passes within 3.65 mm of the
   rectangle's front, left and right edges and within 0.65 mm of its back
   edge (the lead-in arcs and ramps are in the waste round the outline).
   There is no room for clamps on a 170 × 120 mm piece: use a bigger
   board and clamp or screw it outside those extents, with room to spare
   for the collet nut.
2. **Use a spoilboard.** The holes and the cut-out go 0.2 mm below the
   plywood.
3. **Do an air cut.** Set Z zero 20 mm above the plywood instead of on
   it, and run each program. The deepest move, Z −12, then stays 8 mm
   above the wood. Watch that the path is where you expect and nothing
   hits a clamp. Then set Z zero back on the plywood.

## 10. Cut

1. Fit the 6.35 mm bit and set work zero with it: X and Y at the
   front-left corner of the stock, Z on the top of the plywood.
2. Load `plaque-1-T1.nc` and start it. It clears the panel and the
   square, drills the four holes, and cuts the outline. It does not stop
   until the end. Stay at the machine with the stop button in reach.
3. When it ends, fit the 3.175 mm bit. Set Z zero with it on the top of
   the plywood (not on the panel). Leave X and Y alone.
4. Load `plaque-2-T2.nc` and start it.
5. When it ends, cut the four tabs with a knife or a flush-cut saw, and
   sand the stubs.

The plaque should release after the tabs are cut; the panel floor should
be flat with no ridge round the island; a 30 mm square block should fit
the square recess; the holes should take an M5 screw; the engraved line
should be continuous; and the outside edge should show no mark where the
passes started.

## Next steps

- Clear the panel in rings instead of lines: change op1's `strategy` to
  `"offset"` and export again. Only that operation's moves change. See
  [Pocket operation](../user-guide/pocket.md).
- Rough a pocket with a large bit and finish its corners with a small one:
  see [Rest machining](../user-guide/pocket.md#rest-machining).
- Use another controller: see
  [Post-processors](../user-guide/post-processors.md).
- Every warning `check` can give, and what to do about it:
  [Warnings](../user-guide/warnings.md).
