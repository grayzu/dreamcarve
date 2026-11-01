# A V-carved sign from SVG outlines

This chapter takes a sign drawn as SVG outlines to four G-code programs,
and then to a carved sign. The sign is 200 × 80 mm, cut from hardwood or
MDF sold as 12 mm:

- the **background**, a 190 × 70 mm panel 5 mm inside the edge, is cut
  away to a flat floor 2.5 mm deep, leaving a raised **oval** with a 45°
  sloped edge standing in it. An end mill clears the floor first; the
  V-bit cuts the slope and the corners the end mill cannot reach;
- the word **"DreamCarve"** is V-carved into the oval, every serif and
  corner cut to a point;
- two **hanging holes**, 3.175 mm, are drilled through in the
  background's top corners, and **countersunk** 6 mm across on the
  floor, so a screw head sits in each;
- the **outline** is cut out with four tabs.

You use three bits: a 6.35 mm (1/4") end mill, a 3.175 mm (1/8") end mill
and a 90° 6.35 mm V-bit. If you have not done [A pocketed plaque from
DXF](plaque.md), do it first: this chapter assumes you know the tool
library, pasting operations and running programs one bit at a time.

Every command here is run by DreamCarve's test suite exactly as written,
and the excerpts are checked against the programs it writes, so if it does
not work for you, that is a bug.

## What you need

- DreamCarve installed and on your `PATH`. See [Installing](installing.md).
- A text editor.
- The drawing,
  [`sign.svg`](https://raw.githubusercontent.com/grayzu/dreamcarve/main/tests/fixtures/sign.svg).
  It is also in the repository at `tests/fixtures/sign.svg`.
- Calipers, to measure the board.
- For the cut: a board of poplar, maple or MDF sold as 12 mm, at least
  220 × 100 mm, on a spoilboard (plywood shows its layers in a V-carve);
  the 6.35 mm and 3.175 mm two-flute upcut end mills and the 90° 6.35 mm
  V-bit; and a router controlled by grblHAL.

## 1. Put the drawing in a folder

Make an empty folder and put `sign.svg` in it:

```sh
mkdir sign
cd sign
```

Run every command below from that folder.

The drawing is what you would make in Inkscape: two rectangles, an
ellipse, two small circles, and the word typed in a bold serif face,
then converted to outlines (Path → Object to Path), since DreamCarve
reads shapes, not text. The lettering is Droid Serif Bold, 14 mm cap
height, set on a gentle arc.

## 2. Create the project

<!-- doc-test: run -->
```sh
dreamcarve init sign.svg
```

```text
Imported 15 shape(s) from sign.svg: page units mm, 1 user unit = 1.0000 mm.
Wrote sign.dcv: stock 220.00 × 100.00 mm, post-processor grblhal, 15 shape(s) targeted by Profile.
Values to set; export refuses until every one is set:
sign.dcv:10:21: stock.thickness_mm is not set (mm)
sign.dcv:31:22: tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`
sign.dcv:32:24: tools[t1].plunge_mm_min is not set (mm/min) for tool `Set me`
sign.dcv:33:14: tools[t1].rpm is not set (RPM) for tool `Set me`
sign.dcv:34:23: tools[t1].step_down_mm is not set (mm) for tool `Set me`
sign.dcv:63:19: operations[op1].depth_mm is not set (mm) for operation `Profile`
Next: edit sign.dcv and replace each `null` listed above. To cut through, `depth_mm` is the thickness plus `through_cut_allowance_mm`. Then run `dreamcarve check sign.dcv`.
```

The stock is the drawing plus 10 mm all round, with the drawing's
bottom-left corner at (10, 10). Each shape gets an instance id in the
order the file lists it:

| Id | Name | What it is |
| --- | --- | --- |
| `i1` | `outline` | The sign's 200 × 80 mm outline |
| `i2` | `background` | The 190 × 70 mm rectangle 5 mm inside it |
| `i3` | `oval` | The 150 × 46 mm oval in the middle |
| `i4`, `i5` | `hole-left`, `hole-right` | The two 3.175 mm hanging holes, in the background's top corners |
| `i6` to `i15` | `letter-D` to `letter-e-2` | The ten letters of "DreamCarve", each with its counters (the holes in the D, e and a) |

## 3. Add the tools from your library

Save this as `my-tools.json` in the folder:

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
        "name": "1/4 end mill",
        "vendor": null,
        "product_id": null,
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
      "id": "lt3",
      "source": null,
      "tool": {
        "id": "lt3",
        "name": "1/4 90 degree V-bit",
        "vendor": null,
        "product_id": null,
        "number": 3,
        "cutter": { "v_bit": { "included_angle_deg": 90.0 } },
        "diameter_mm": 6.35,
        "flutes": 2,
        "feed_mm_min": 1200.0,
        "plunge_mm_min": 400.0,
        "rpm": 18000,
        "step_down_mm": 2.5,
        "step_over_fraction": 0.4
      }
    }
  ]
}
```

The two end mills are the plaque's. The third tool is the V-bit: its
`cutter` gives its **included angle**, 90°, the angle between its two
cutting edges. A V-Carve cuts in one pass, so the V-bit's `step_down_mm`
is not used here.

Add the file's tools to your library, list it, and copy the three tools
into the project:

<!-- doc-test: run -->
```sh
dreamcarve tools import my-tools.json
dreamcarve tools list
dreamcarve tools add sign.dcv lt1
dreamcarve tools add sign.dcv lt2
dreamcarve tools add sign.dcv lt3
```

```text
`1/4 end mill`: imported (lt1)
`1/8 end mill`: imported (lt2)
`1/4 90 degree V-bit`: imported (lt3)
3 imported, 0 already present, 0 replaced; library: /Users/you/Library/Application Support/dreamcarve/tools.json
ID   NAME                 VENDOR  PRODUCT  CUTTER     DIAMETER_MM  FEED_MM_MIN  PLUNGE_MM_MIN  RPM    NUMBER  SOURCE
lt1  1/4 end mill         -       -        end mill   6.35         1500         500            18000  1       -
lt2  1/8 end mill         -       -        end mill   3.175        1000         300            18000  2       -
lt3  1/4 90 degree V-bit  -       -        V-bit 90°  6.35         1200         400            18000  3       -
Added `1/4 end mill` (lt1) to sign.dcv as tool t2.
Added `1/8 end mill` (lt2) to sign.dcv as tool t3.
Added `1/4 90 degree V-bit` (lt3) to sign.dcv as tool t4.
```

If you did the plaque, the end mills are already in your library, and
`import` says "already present" for them. The project now has `t1` ("Set
me", unused), `t2` (the 6.35 mm end mill, T1 on your machine), `t3` (the
3.175 mm end mill, T2) and `t4` (the V-bit, T3).

## 4. Set the material

Measure the board with calipers. This chapter uses 11.8 mm; use your own
measurement. In `sign.dcv`, change the stock's `thickness_mm` line to:

<!-- doc-test: edit sign.dcv -->
```json
"thickness_mm": 11.8,
```

The holes and the cut-out go 12 mm deep, 0.2 mm into the spoilboard. If
your measurement differs, set both `depth_mm` values of 12.0 to it plus
0.2 after you paste them.

## 5. Paste the operations

In `sign.dcv`, replace the whole `"operations"` block, from
`"operations": [` down to its closing `],`, with this, and save:

<!-- doc-test: paste sign.dcv operations -->
```json
"operations": [
  {
    "kind": "drill",
    "id": "op1",
    "name": "Hanging holes",
    "enabled": true,
    "targets": [ "i4", "i5" ],
    "tool": "t3",
    "cycle": "simple",
    "dwell_s": 0.0,
    "depth_mm": 12.0,
    "retract_mm": 1.0,
    "through_cut_allowance_mm": 0.2,
    "safe_z_mm": 15.0,
    "allow_tool_wider_than_hole": false
  },
  {
    "kind": "v_carve",
    "id": "op2",
    "name": "Background",
    "enabled": true,
    "targets": [ "i2", "i3" ],
    "tool": "t4",
    "strategy": "medial",
    "clearing": [
      { "tool": "t2", "strategy": "offset", "direction": "climb", "entry": { "ramp": { "angle_deg": 15.0 } } }
    ],
    "flat_depth_mm": 2.5,
    "start_depth_mm": 0.0,
    "start_plane_cleared_externally": false,
    "step_over_fraction": 0.1,
    "machining_order": "full_depth_first",
    "through_cut_allowance_mm": 0.0,
    "safe_z_mm": 15.0
  },
  {
    "kind": "v_carve",
    "id": "op3",
    "name": "Lettering",
    "enabled": true,
    "targets": [ "i6", "i7", "i8", "i9", "i10", "i11", "i12", "i13", "i14", "i15" ],
    "tool": "t4",
    "strategy": "medial",
    "clearing": [],
    "flat_depth_mm": null,
    "start_depth_mm": 0.0,
    "start_plane_cleared_externally": false,
    "step_over_fraction": null,
    "machining_order": "full_depth_first",
    "through_cut_allowance_mm": 0.0,
    "safe_z_mm": 15.0
  },
  {
    "kind": "drill",
    "id": "op5",
    "name": "Countersinks",
    "enabled": true,
    "targets": [ "i4", "i5" ],
    "tool": "t4",
    "cycle": {
      "countersink": { "diameter_mm": 6.0, "start_depth_mm": 2.5, "start_plane_cleared_externally": false }
    },
    "dwell_s": 0.0,
    "depth_mm": null,
    "retract_mm": 1.0,
    "through_cut_allowance_mm": 0.0,
    "safe_z_mm": 15.0,
    "allow_tool_wider_than_hole": false
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
      "on_holes": false,
      "avoid_corners": true
    },
    "machining_order": "full_depth_first",
    "entry": { "ramp": { "angle_deg": 15.0 } },
    "lead": { "radius_mm": 3.0, "sweep_deg": 90.0, "lead_out": true, "overcut_mm": 1.0 },
    "corner_relief": "none",
    "last_pass": null,
    "start": "auto"
  }
],
```

| Operation | What it cuts | Settings worth knowing |
| --- | --- | --- |
| `op1` Drill "Hanging holes" | Plunges the 3.175 mm end mill (`t3`) through at the centre of each hole | The holes are the bit's size, so a Drill, which only goes straight down, cuts them as drawn. See [Drill operation](../user-guide/drill.md). |
| `op2` V-Carve "Background" | Cuts the background away to a floor 2.5 mm deep round the oval | `targets` names the rectangle and the oval: the oval, inside the rectangle, is an island and stands. `flat_depth_mm: 2.5` stops the carve at 2.5 mm; with the 90° V-bit (`t4`) the walls slope at 45° from the oval's outline down to the floor 2.5 mm out from it. The pass under `clearing` clears that floor with the 6.35 mm end mill (`t2`) first, in rings (`offset`), ramping in at 15°. The V-bit then traces the floor's edge and cleans out the four corners the end mill cannot reach, in rings 0.5 mm apart (`step_over_fraction: 0.1` of its 5 mm cut width at 2.5 mm). |
| `op3` V-Carve "Lettering" | Carves the letters into the oval | No flat depth: every stroke is cut to the depth its width needs, at most 1.66 mm here, and every serif runs up to a point. The V-bit's cone is 3.175 mm tall, so it can carve any stroke up to 6.35 mm wide without a flat depth; these are at most 3.3 mm. |
| `op5` Drill "Countersinks" | Opens a 6 mm countersink round the top of each hanging hole, on the background's floor | A Drill with the `countersink` cycle and the V-bit (`t4`). See [The countersinks](#the-countersinks) below. |
| `op4` Profile "Cut out" | Cuts the sign out, outside its outline | Through the board into the spoilboard with the 6.35 mm end mill, held by four tabs, as the plaque's. |

See [V-Carve operation](../user-guide/v-carve.md) for every setting.

### The countersinks

`op5` drills the same two circles as `op1`, but with the V-bit and
`"cycle": { "countersink": … }`. A V-bit plunged into a hole cuts a cone
round its top, a seat for a screw head. You do not give the depth: you
give the width of the cone's **mouth**, `diameter_mm`, and DreamCarve
works out how deep the bit must go to open it, `diameter_mm / (2 ×
tan(α / 2))` for a bit of angle `α`. A 90° bit opens a 6 mm mouth 3 mm
down, so `depth_mm` is `null`: a countersink does not use it.

The holes are on the background's floor, 2.5 mm below the board's top,
so the mouth must open there, not at the top: `start_depth_mm: 2.5` says
where the cone starts, and the bit goes 3 mm below that, to 5.5 mm. That
floor counts as cleared because `op2`, earlier in the list, is a V-Carve
whose 2.5 mm flat floor holds the whole bit at both holes; `check` would
otherwise stop with `start_depth_uncleared`. The end mill's clearing
pass, not the V-bit, cleared the floor round both holes, so each cone
starts on flat wood. See [Drill: countersink](../user-guide/drill.md#countersink).

**The order matters.** The holes come first, while the board is whole.
The background comes before the lettering only so the V-bit's two carves
run one after the other in the same program; either order cuts the same.
The countersinks come after the background, whose floor they start on,
and right after the lettering, so the V-bit does all its work in one
program. The ids are only names: `op5` was added last, but it is listed
before `op4`, and operations run in the order listed. The cut-out is
last. That means the 6.35 mm end mill is fitted twice: once for the
background's clearing pass, which must come before the V-bit, and again
for the cut-out, which must come after it.

## 6. Check the project

<!-- doc-test: run -->
```sh
dreamcarve check sign.dcv
```

```text
sign.dcv:22:5: warning: unused_tool: tool `t1` (`Set me`) is defined but not used by any enabled operation
warning: tool_change_avoidable: tool `t2` (`1/4 end mill`) is used by operations `op2`, `op4`, which do not run one after another, so it is fitted more than once; put those operations next to each other to save a tool change (operations always run in the order listed)
V-Carve op2 `Background`: deepest cut 2.500 mm below the stock top, clamped at a flat depth of 2.500 mm; V-bit floor ridges 0.250 mm where no clearing pass reaches.
V-Carve op3 `Lettering`: deepest cut 1.658 mm below the stock top, no flat depth.
Program 1 (T2):
  Extents (machine coordinates, mm, tool centre): X 25.000..195.000, Y 75.000..75.000, Z -12.000..15.000
    The cutter's edge reaches X 23.413..196.588, Y 73.412..76.588; keep clamps outside that.
  Estimated time: 8 s (cutting 5 s, dwells 3 s); 252 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program 2 (T1):
  Extents (machine coordinates, mm, tool centre): X 20.675..199.325, Y 20.675..79.325, Z -2.500..15.000
    The cutter's edge reaches X 17.500..202.500, Y 17.500..82.500; keep clamps outside that.
  Estimated time: 1 min 14 s (cutting 1 min 11 s, dwells 3 s); 414 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program 3 (T3):
  Extents (machine coordinates, mm, tool centre): X 15.000..205.000, Y 15.000..85.000, Z -5.500..15.000
    The cutter's edge reaches X 11.825..208.175, Y 11.825..88.175; keep clamps outside that.
  Estimated time: 1 min 30 s (cutting 1 min 27 s, dwells 3 s); 3.7 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program 4 (T1):
  Extents (machine coordinates, mm, tool centre): X 3.825..213.175, Y 6.825..93.175, Z -12.000..15.000
    The cutter's edge reaches X 0.650..216.350, Y 3.650..96.350; keep clamps outside that.
  Estimated time: 1 min 21 s (cutting 1 min 18 s, dwells 3 s); 137 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Estimated total time: 4 min 14 s (cutting 4 min 2 s, dwells 12 s); 4.5 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
sign.dcv: 0 error(s), 2 warning(s)
```

No errors. Two warnings:

- `unused_tool`: the "Set me" tool `init` made. Delete its block from
  `"tools"` if you like.
- `tool_change_avoidable`: the 6.35 mm end mill is used by `op2` (its
  clearing pass) and `op4`, with the V-bit between them. Here that is on
  purpose, as step 5 says: the warning is a hint, never an error.

The two V-Carve lines give each carve's deepest cut. The background stops
at its 2.5 mm flat depth; where the V-bit clears floor corners itself, it
leaves ridges 0.25 mm high (`step_over_fraction` × flat depth). The
lettering goes 1.658 mm deep at most, well within the bit's 3.175 mm cone.

`dreamcarve check sign.dcv --preview sign-preview.svg` also draws every
toolpath over the drawing, in your browser: the end mill's rings round
the oval, the V-bit's trace round the floor and its lines into the
corners, and its lines down the middle of every letter and into every
serif.

## 7. Export the programs

<!-- doc-test: run -->
```sh
dreamcarve export sign.dcv
```

```text
sign.dcv:22:5: warning: unused_tool: tool `t1` (`Set me`) is defined but not used by any enabled operation
warning: tool_change_avoidable: tool `t2` (`1/4 end mill`) is used by operations `op2`, `op4`, which do not run one after another, so it is fitted more than once; put those operations next to each other to save a tool change (operations always run in the order listed)
Wrote 4 programs; run them in this order:
  1. sign-1-T2.nc  Before you start: fit T2 1/8 end mill and set work zero (X, Y and Z) with it.
  2. sign-2-T1.nc  Before you start: fit T1 1/4 end mill and set Z zero with it. Keep X and Y zero.
  3. sign-3-T3.nc  Before you start: fit T3 1/4 90 degree V-bit and set Z zero with it. Keep X and Y zero.
  4. sign-4-T1.nc  Before you start: fit T1 1/4 end mill and set Z zero with it. Keep X and Y zero.
This program does not change the tool length offset (G43.1). If your sender sets one, set it before you set Z zero.
If you used G51 on this controller, soft-reset it and check that $G shows G50 before you start: grblHAL's G50 does not clear a scale that leaves X at 1, such as G51 Z-1.
Post-processor grblhal.
  op1 `Hanging holes`: tool `t3` (1/8 end mill), 2 target(s), 12.000 mm deep
  op2 (clearing t2) `Background`: tool `t2` (1/4 end mill), 2 target(s), 2.500 mm deep
  op2 `Background`: tool `t4` (1/4 90 degree V-bit), 2 target(s), cut 2.500 mm deep at most
  op3 `Lettering`: tool `t4` (1/4 90 degree V-bit), 10 target(s), cut 1.658 mm deep at most
  op5 `Countersinks`: tool `t4` (1/4 90 degree V-bit), 2 target(s), 6.000 mm countersinks, 5.500 mm deep
  op4 `Cut out`: tool `t2` (1/4 end mill), 1 target(s), 12.000 mm deep
sign-1-T2.nc (25 lines):
  Extents (machine coordinates, mm, tool centre): X 25.000..195.000, Y 75.000..75.000, Z -12.000..15.000
    The cutter's edge reaches X 23.413..196.588, Y 73.412..76.588; keep clamps outside that.
  Estimated time: 8 s (cutting 5 s, dwells 3 s); 252 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
sign-2-T1.nc (415 lines):
  Extents (machine coordinates, mm, tool centre): X 20.675..199.325, Y 20.675..79.325, Z -2.500..15.000
    The cutter's edge reaches X 17.500..202.500, Y 17.500..82.500; keep clamps outside that.
  Estimated time: 1 min 14 s (cutting 1 min 11 s, dwells 3 s); 414 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
sign-3-T3.nc (5640 lines):
  Extents (machine coordinates, mm, tool centre): X 15.000..205.000, Y 15.000..85.000, Z -5.500..15.000
    The cutter's edge reaches X 11.825..208.175, Y 11.825..88.175; keep clamps outside that.
  Estimated time: 1 min 30 s (cutting 1 min 27 s, dwells 3 s); 3.7 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
sign-4-T1.nc (86 lines):
  Extents (machine coordinates, mm, tool centre): X 3.825..213.175, Y 6.825..93.175, Z -12.000..15.000
    The cutter's edge reaches X 0.650..216.350, Y 3.650..96.350; keep clamps outside that.
  Estimated time: 1 min 21 s (cutting 1 min 18 s, dwells 3 s); 137 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Estimated total time: 4 min 14 s (cutting 4 min 2 s, dwells 12 s); 4.5 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
V-Carve op2 `Background`: deepest cut 2.500 mm below the stock top, clamped at a flat depth of 2.500 mm; V-bit floor ridges 0.250 mm where no clearing pass reaches.
V-Carve op3 `Lettering`: deepest cut 1.658 mm below the stock top, no flat depth.
```

Export writes one program per **tool run**, in the order to run them. The
background is two runs, its clearing pass with the end mill and its carve
with the V-bit, so the five operations make six runs, and the three V-bit
runs, one after the other, share a program:

1. `sign-1-T2.nc`, the 3.175 mm end mill: the hanging holes;
2. `sign-2-T1.nc`, the 6.35 mm end mill: the background's floor;
3. `sign-3-T3.nc`, the V-bit: the background's slopes and corners, the
   lettering, then the countersinks;
4. `sign-4-T1.nc`, the 6.35 mm end mill again: the cut-out.

The summary gives the countersinks' mouth and how deep they go, 5.5 mm
below the board's top: the 2.5 mm floor plus the 3 mm cone. Program 3's
Z range now reaches −5.5 for them.

Each program passed the safety checks before it was written, including
the one that holds the V-bit's whole cone, not just its tip, inside the
shapes it carves ([Safety](../user-guide/safety.md), rule 14), and the
one that holds each countersink to a plunge at its hole's centre to
exactly the depth its mouth needs (rule 15).

## 8. Read the programs

`sign-2-T1.nc` is the clearing pass. Its comment names the operation and
the pass:

<!-- doc-test: contains sign-2-T1.nc -->
```text
( Operation 1: Background clearing t2 )
( Tool 1: 1/4 end mill )
```

`sign-3-T3.nc` starts with the background's corners:

<!-- doc-test: excerpt sign-3-T3.nc -->
```text
( Project: sign )
( Generated by DreamCarve on 2026-10-29 )
G90
G17
G94
G80
G40
G50
G21
G54
( Operation 1: Background )
( Tool 3: 1/4 90 degree V-bit )
G0 Z15.000
M3 S18000
G4 P3.000
X15.000 Y15.000
Z1.000
G1 Z0.000 F400.000
X17.406 Y17.406 Z-2.406 F692.820
X17.474 Y17.474 Z-2.474 F691.607
X17.843 Y17.844 Z-2.500 F1200.000
X30.169 Y30.169
Y69.831
X17.843 Y82.156
X17.474 Y82.526 Z-2.474
X17.406 Y82.594 Z-2.406
X15.000 Y85.000 Z0.000
G0 Z15.000
```

The bit comes down over the background's bottom-left corner, (15, 15) in
machine coordinates: a rapid to 1 mm above the board, then down to the
surface at the plunge rate. From the corner it runs in along the corner's
diagonal, going down as it goes, to the flat depth at Z −2.5, about
2.5 mm in from both walls. The feed of that move, 692.820 mm/min, is
lowered so that the bit goes down no faster than its 400 mm/min plunge
rate. It then runs along the left end of the background at Z −2.5 and
back out along the top-left corner's diagonal, rising to the surface at
the corner (15, 85). A V-bit can reach into a square corner, where the end
mill left a radius. The program then does the other corners, traces the
floor's edge, cleans the corners' floor, and goes on to the lettering:

<!-- doc-test: contains sign-3-T3.nc -->
```text
( Operation 2: Lettering )
```

The program ends with the countersinks:

<!-- doc-test: contains sign-3-T3.nc -->
```text
( Operation 3: Countersinks )
( Tool 3: 1/4 90 degree V-bit )
X25.000 Y75.000
Z1.000
G1 Z-5.500 F400.000
G0 Z15.000
X195.000
Z1.000
G1 Z-5.500
G0 Z15.000
```

Over each hole the bit drops by rapid to 1 mm above the board, the
Drill's `retract_mm`, measured from the board's top, not from the floor,
so the rapid never ends in wood. It then plunges at the plunge rate
straight down the middle of the hole to Z −5.5, through 2.5 mm of air
over the floor and 3 mm into it, and lifts straight out. With its 90°
point 3 mm below the floor, the bit's sides cut a cone 6 mm across at
the floor. A countersink is always written as these plain moves, never
as a controller's drilling cycle, whatever the post-processor.

## 9. Before you cut

Read [Safety](../user-guide/safety.md) once before your first cut. For
this sign:

1. **Fix the stock.** The 6.35 mm end mill's edge reaches X 0.65 to
   216.35 mm and Y 3.65 to 96.35 mm in the cut-out: there is no room for
   clamps on a 220 × 100 mm piece. Use a bigger board, and clamp or screw
   it outside those extents with room for the collet nut.
2. **Use a spoilboard.** The holes and the cut-out go 0.2 mm below the
   board.
3. **Do an air cut** of each program with Z zero 20 mm above the board.

## 10. Cut

1. Fit the 3.175 mm end mill and set work zero with it: X and Y at the
   front-left corner of the stock, Z on the top of the board. Run
   `sign-1-T2.nc`: the two holes.
2. Fit the 6.35 mm end mill and set Z zero on the top of the board. Keep
   X and Y. Run `sign-2-T1.nc`: it clears the background floor 2.5 mm
   deep round the oval. Vacuum the chips out.
3. Fit the V-bit. Set Z zero with it on the top of the board, on the oval
   or the border, not on the floor just cut: the tip of a V-bit is easy
   to set too low, so touch it off gently. Run `sign-3-T3.nc`: the slope
   round the oval, the corners, the lettering, and the countersinks. A Z
   zero set too low makes the countersinks too wide, 2 mm wider for every
   1 mm too low with this bit.
4. Fit the 6.35 mm end mill again, set Z zero on the top of the board,
   and run `sign-4-T1.nc`: the cut-out.
5. Cut the tabs and sand the stubs. A light sanding takes off the fuzz
   on the letters' edges and the small ridges in the floor's corners.

The background floor should be flat with no step where the end mill's
floor meets the V-bit's; the oval's edge should be a clean 45° slope from
the floor to the surface; every serif and corner of the lettering should
end in a sharp point at the surface; the holes should be 3.175 mm across,
each with a countersink 6 mm across at the floor, centred on it and clear
of the oval's slope; and the sign should release after the tabs are cut.

## Next steps

- Try a 60° V-bit for the lettering: add one to your library and the
  project, and set op3's `tool` to it. The letters are cut deeper, up to
  2.87 mm, with steeper walls. See [Choosing a bit
  angle](../user-guide/v-carve.md#choosing-a-bit-angle).
- Cut the background in lighter passes: set op2's `strategy` to
  `{ "offset": { "step_over_mm": 0.5, "direction": "climb" } }`. See
  [Offset strategy](../user-guide/v-carve.md#offset-strategy).
- Every V-Carve setting, flat depths, clearing passes and start depths:
  [V-Carve operation](../user-guide/v-carve.md).
