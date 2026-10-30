# An inlay pair

This chapter takes one drawing, a four-point star with a circle at its
centre, to two projects and five G-code programs, and then to a maple
star inlaid in a walnut board, with a walnut dot at its centre. It is the
V-bit inlay method Corbin's Workshop documents for cutting boards
([Inlay](../user-guide/inlay.md) says more), done with two boards:

- the **female** board, walnut, gets a **pocket** the shape of the star,
  4 mm deep, with walls sloping at the V-bit's angle. The circle is an
  island: it stays standing in the pocket;
- the **male** board, maple, gets a **plug**: the star, mirrored, standing
  5 mm proud of a flat band carved round it, with walls sloping the other
  way, and a hole where the circle is; the end mill then cuts it free of
  the board, on a rim of the band. Turned over, the plug fits the
  pocket: its walls meet the pocket's walls, and its face stops 1 mm short
  of the pocket's floor, which leaves room for glue.

You glue the plug into the pocket, saw off the maple left above the
walnut's surface, and plane the board flat.

You use two bits: a 60° 6.35 mm (1/4") V-bit and a 3.175 mm (1/8") end
mill. If you have not done [A V-carved sign from SVG
outlines](sign.md), do it first: this chapter assumes you know the tool
library, pasting operations, V-bits and clearing passes.

Every command here is run by DreamCarve's test suite exactly as written,
and the excerpts are checked against the programs it writes, so if it does
not work for you, that is a bug.

## What you need

- DreamCarve installed and on your `PATH`. See [Installing](installing.md).
- A text editor.
- The drawing,
  [`star.svg`](https://raw.githubusercontent.com/grayzu/dreamcarve/main/tests/fixtures/star.svg).
  It is also in the repository at `tests/fixtures/star.svg`.
- Calipers, to measure the boards.
- For the cut: two boards sold as 12 mm, at least 100 × 100 mm, of
  contrasting hardwoods: walnut for the female, maple for the male (any
  pair works; a dark and a light wood show the inlay best). A 60°
  6.35 mm V-bit and a 3.175 mm two-flute upcut end mill, a router
  controlled by grblHAL, and a spoilboard.
- For the glue-up: wood glue, a clamp or a press with two flat cauls, a
  bandsaw or scroll saw, and a hand plane or a sander.

## 1. Put the drawing in a folder

Make an empty folder and put `star.svg` in it:

```sh
mkdir inlay
cd inlay
```

Run every command below from that folder.

The drawing has three shapes: the star, 60 mm from tip to tip, its tips
30 mm out from the centre and its inner corners 12 mm out; a circle
12 mm across at the centre; and an 80 × 80 mm square round them, the
**frame**. Nothing cuts the frame. It is there because `init` makes the
stock the drawing plus 10 mm all round: with the frame, the stock is
100 × 100 mm with the star at its centre.

## 2. Create the two projects

An inlay is two projects made from the same drawing, one for each board.
Make both now, naming each with `-o`:

<!-- doc-test: run -->
```sh
dreamcarve init star.svg -o star-female.dcv
dreamcarve init star.svg -o star-male.dcv
```

```text
Imported 3 shape(s) from star.svg: page units mm, 1 user unit = 1.0000 mm.
Wrote star-female.dcv: stock 100.00 × 100.00 mm, post-processor grblhal, 3 shape(s) targeted by Profile.
Values to set; export refuses until every one is set:
star-female.dcv:10:21: stock.thickness_mm is not set (mm)
star-female.dcv:31:22: tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`
star-female.dcv:32:24: tools[t1].plunge_mm_min is not set (mm/min) for tool `Set me`
star-female.dcv:33:14: tools[t1].rpm is not set (RPM) for tool `Set me`
star-female.dcv:34:23: tools[t1].step_down_mm is not set (mm) for tool `Set me`
star-female.dcv:51:19: operations[op1].depth_mm is not set (mm) for operation `Profile`
Next: edit star-female.dcv and replace each `null` listed above. To cut through, `depth_mm` is the thickness plus `through_cut_allowance_mm`. Then run `dreamcarve check star-female.dcv`.
Imported 3 shape(s) from star.svg: page units mm, 1 user unit = 1.0000 mm.
Wrote star-male.dcv: stock 100.00 × 100.00 mm, post-processor grblhal, 3 shape(s) targeted by Profile.
Values to set; export refuses until every one is set:
star-male.dcv:10:21: stock.thickness_mm is not set (mm)
star-male.dcv:31:22: tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`
star-male.dcv:32:24: tools[t1].plunge_mm_min is not set (mm/min) for tool `Set me`
star-male.dcv:33:14: tools[t1].rpm is not set (RPM) for tool `Set me`
star-male.dcv:34:23: tools[t1].step_down_mm is not set (mm) for tool `Set me`
star-male.dcv:51:19: operations[op1].depth_mm is not set (mm) for operation `Profile`
Next: edit star-male.dcv and replace each `null` listed above. To cut through, `depth_mm` is the thickness plus `through_cut_allowance_mm`. Then run `dreamcarve check star-male.dcv`.
```

Both projects have the same shapes, with the same ids:

| Id | Name | What it is |
| --- | --- | --- |
| `i1` | `star` | The four-point star, centred at (50, 50) on the stock |
| `i2` | `circle` | The 12 mm circle at its centre |
| `i3` | `frame` | The 80 × 80 mm square; not cut |

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

The end mill is the sign's 3.175 mm end mill, T2 on the machine. The
V-bit is a 60° bit, T1. Its **cone height**, the depth at which its sides
reach their full 6.35 mm, is 5.5 mm, and the deepest either side of this
inlay goes is 5 mm (see [Choosing the bit](../user-guide/inlay.md#choosing-the-bit)).

Add the file's tools to your library, list it, and copy both tools into
both projects:

<!-- doc-test: run -->
```sh
dreamcarve tools import my-tools.json
dreamcarve tools list
dreamcarve tools add star-female.dcv lt1
dreamcarve tools add star-female.dcv lt2
dreamcarve tools add star-male.dcv lt1
dreamcarve tools add star-male.dcv lt2
```

```text
`1/8 end mill`: imported (lt1)
`1/4 60 degree V-bit`: imported (lt2)
2 imported, 0 already present, 0 replaced; library: /Users/you/Library/Application Support/dreamcarve/tools.json
ID   NAME                 VENDOR  PRODUCT  CUTTER     DIAMETER_MM  FEED_MM_MIN  PLUNGE_MM_MIN  RPM    NUMBER  SOURCE
lt1  1/8 end mill         -       -        end mill   3.175        1000         300            18000  2       -
lt2  1/4 60 degree V-bit  -       -        V-bit 60°  6.35         1000         300            18000  1       -
Added `1/8 end mill` (lt1) to star-female.dcv as tool t2.
Added `1/4 60 degree V-bit` (lt2) to star-female.dcv as tool t3.
Added `1/8 end mill` (lt1) to star-male.dcv as tool t2.
Added `1/4 60 degree V-bit` (lt2) to star-male.dcv as tool t3.
```

The library gives each tool it imports the next free id. If your library
already has tools (from the earlier chapters), `import` says the end mill
is already present and the ids differ: use the ids `tools list` shows.
Each project now has `t1` ("Set me", unused), `t2` (the end mill) and
`t3` (the V-bit).

## 4. Set the material

Measure each board with calipers. This chapter uses 12 mm for both; use
your own measurements, the walnut's in `star-female.dcv` and the maple's
in `star-male.dcv`. In each file, change the stock's `thickness_mm` line
to:

<!-- doc-test: edit star-female.dcv star-male.dcv -->
```json
"thickness_mm": 12.0,
```

The male's thickness matters most: the plug's surround is cut 5 mm deep,
and the board under it holds the plug together until the glue dries. A
male board under 11 mm leaves less than 6 mm, and `check` warns
`inlay_plug_base_thin`.

## 5. Paste the operation into both projects

In both files, replace the whole `"operations"` block, from
`"operations": [` down to its closing `],`, with this, and save:

<!-- doc-test: paste star-female.dcv star-male.dcv operations -->
```json
"operations": [
  {
    "kind": "inlay",
    "id": "op1",
    "name": "Star",
    "enabled": true,
    "targets": [ "i1", "i2" ],
    "tool": "t3",
    "side": "female",
    "depth_mm": 4.0,
    "glue_gap_mm": 1.0,
    "plug_depth_mm": 2.0,
    "margin_mm": 8.0,
    "clearing": [
      { "tool": "t2", "strategy": "offset", "direction": "climb", "entry": { "ramp": { "angle_deg": 15.0 } } }
    ],
    "step_over_fraction": 0.1,
    "machining_order": "full_depth_first",
    "through_cut_allowance_mm": 0.0,
    "safe_z_mm": 10.0,
    "cut_out": null
  }
],
```

One **Inlay** operation describes both sides with the same four values;
`side` says which one the project cuts.

| Field | Value | What it does here |
| --- | --- | --- |
| `targets` | `i1`, `i2` | The star and the circle. The circle, inside the star, is an island in the pocket, and a hole in the plug. |
| `tool` | `t3` | The 60° V-bit. Both sides must use the same angle. |
| `depth_mm` | 4 | The pocket is 4 mm deep (Corbin's "Inlay Depth"). |
| `glue_gap_mm` | 1 | The plug's face stops 1 mm above the pocket's floor (Corbin's "Inlay Fit Adjustment"). It is the value you change to adjust the fit. |
| `plug_depth_mm` | 2 | The male's flat band is cut 2 mm deeper than the plug's walls need, so the seated plug's backing stands 2 mm clear of the walnut, room to saw it off. |
| `margin_mm` | 8 | The male's band reaches 8 mm beyond the star (Corbin's "Plug Stock Offset"). The female ignores it. |
| `clearing` | `t2` | The end mill clears the flat floor first, on either side; the V-bit then cuts the walls and the corners the end mill cannot reach. |
| `step_over_fraction` | 0.1 | Where the V-bit clears floor itself, its rings are close enough to leave ridges only a tenth of the floor's depth. |
| `safe_z_mm` | 10 | Travel 10 mm above the board. |
| `cut_out` | `null` | No cut-out here; the male gets one in the next step. |

See [Inlay](../user-guide/inlay.md) for every setting.

## 6. Make the second project the male

In `star-male.dcv`, change the operation's `side` and `cut_out` lines
to:

<!-- doc-test: edit star-male.dcv -->
```json
"side": "male",
"cut_out": { "tool": "t2", "tabs": { "placement": { "kind": "count", "count": 4 }, "width_mm": 6.0, "height_mm": 3.0, "shape": "rect", "on_holes": false, "avoid_corners": true }, "through_cut_allowance_mm": 0.2 }
```

`side` is the only difference that changes what is carved. Everything
else, the drawing and the four values, must stay the same in both:
change a value in one, change it in the other.

The **cut-out** is the male's last pass: the end mill (`t2`) cuts the
plug free of its board, so you need not saw it out. It runs in the
middle of the band's flat floor, half the margin (4 mm) out from the
mirrored star, from the floor 5 mm down through the board and 0.2 mm
into the spoilboard (`through_cut_allowance_mm`). Four **tabs**, 6 mm
long and 3 mm tall from the bottom of the board, hold the plug in place
until you cut them. Only the male has a cut-out; on the female it is an
error.

## 7. Check the projects

<!-- doc-test: run -->
```sh
dreamcarve check star-female.dcv
dreamcarve check star-male.dcv
```

```text
star-female.dcv:22:5: warning: unused_tool: tool `t1` (`Set me`) is defined but not used by any enabled operation
Inlay op1 `Star` (female): deepest cut 4.000 mm below the stock top, a flat floor at depth_mm 4.000 mm; V-bit floor ridges 0.400 mm where no clearing pass reaches.
Program 1 (T2):
  Extents (machine coordinates, mm, tool centre): X 30.621..69.379, Y 30.621..69.379, Z -4.000..10.000
    The cutter's edge reaches X 29.034..70.967, Y 29.034..70.967; keep clamps outside that.
  Estimated time: 35 s (cutting 32 s, dwells 3 s); 294 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program 2 (T1):
  Extents (machine coordinates, mm, tool centre): X 20.000..80.000, Y 20.000..80.000, Z -4.000..10.000
    The cutter's edge reaches X 16.825..83.175, Y 16.825..83.175; keep clamps outside that.
  Estimated time: 54 s (cutting 51 s, dwells 3 s); 708 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Estimated total time: 1 min 29 s (cutting 1 min 23 s, dwells 6 s); 1.0 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
star-female.dcv: 0 error(s), 1 warning(s)
star-male.dcv:22:5: warning: unused_tool: tool `t1` (`Set me`) is defined but not used by any enabled operation
warning: inlay_male_mirrored: operation `op1`: it is a male inlay, so its design is cut mirrored about X = 50 mm, the middle of its targets, to seat face down in the female
warning: tool_change_avoidable: tool `t2` (`1/8 end mill`) is used by operations `op1`, which do not run one after another, so it is fitted more than once; put those operations next to each other to save a tool change (operations always run in the order listed)
Inlay op1 `Star` (male): deepest cut 5.000 mm below the stock top, the plug's start plane 3.000 mm below the stock top and the surround's flat floor plug_depth_mm 2.000 mm below that; V-bit floor ridges 0.200 mm where no clearing pass reaches; the cut-out frees the plug 4.000 mm out from the design, in the floor, through the board to 0.200 mm below it.
Program 1 (T2):
  Extents (machine coordinates, mm, tool centre): X 14.745..85.255, Y 14.745..85.255, Z -5.000..10.000
    The cutter's edge reaches X 13.158..86.842, Y 13.158..86.842; keep clamps outside that.
  Estimated time: 1 min 48 s (cutting 1 min 45 s, dwells 3 s); 648 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program 2 (T1):
  Extents (machine coordinates, mm, tool centre): X 13.155..86.845, Y 13.156..86.845, Z -5.000..10.000
    The cutter's edge reaches X 9.980..90.020, Y 9.981..90.020; keep clamps outside that.
  Estimated time: 33 s (cutting 30 s, dwells 3 s); 74 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program 3 (T2):
  Extents (machine coordinates, mm, tool centre): X 16.000..84.000, Y 16.000..84.000, Z -12.200..10.000
    The cutter's edge reaches X 14.412..85.588, Y 14.412..85.588; keep clamps outside that.
  Estimated time: 1 min 4 s (cutting 1 min 1 s, dwells 3 s); 31 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Estimated total time: 3 min 25 s (cutting 3 min 16 s, dwells 9 s); 753 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
star-male.dcv: 0 error(s), 3 warning(s)
```

No errors. The warnings:

- `unused_tool`, in both: the "Set me" tool `init` made. Delete its
  block from `"tools"` if you like.
- `inlay_male_mirrored`, in the male: a note, not a problem. The plug is
  glued in face down, so the male cuts the star mirrored left to right,
  about the vertical line X = 50 mm through the middle of the star. Draw
  the design as it should read in the finished board; never mirror it
  yourself.
- `tool_change_avoidable`, in the male: the end mill clears the floor
  first and cuts the plug out last, with the V-bit between, so you fit it
  twice. That order is required: the plug must stay in its board until
  it is carved. Ignore it here.

The Inlay lines give each side's depths:

- the **female** pocket is 4 mm deep, with a flat floor;
- the **male**'s plug is cut from its **start plane**, 3 mm below the
  board's top (`depth_mm − glue_gap_mm`): there, the plug's walls pass
  exactly through the mirrored star. Above it, they slope in at 30° from the vertical,
  as the pocket's walls do going down. The band round the plug is cut
  flat 2 mm deeper (`plug_depth_mm`), 5 mm below the top in all.

Turned over, the plug enters the pocket 3 mm, until its walls meet the
pocket's, and its face stops 1 mm short of the 4 mm floor: the glue gap.
The band is then 2 mm above the walnut's surface.

## 8. Export the programs

<!-- doc-test: run -->
```sh
dreamcarve export star-female.dcv
dreamcarve export star-male.dcv
```

```text
star-female.dcv:22:5: warning: unused_tool: tool `t1` (`Set me`) is defined but not used by any enabled operation
Wrote 2 programs; run them in this order:
  1. star-female-1-T2.nc  Before you start: fit T2 1/8 end mill and set work zero (X, Y and Z) with it.
  2. star-female-2-T1.nc  Before you start: fit T1 1/4 60 degree V-bit and set Z zero with it. Keep X and Y zero.
This program does not change the tool length offset (G43.1). If your sender sets one, set it before you set Z zero.
If you used G51 on this controller, soft-reset it and check that $G shows G50 before you start: grblHAL's G50 does not clear a scale that leaves X at 1, such as G51 Z-1.
Post-processor grblhal.
  op1 (clearing t2) `Star`: tool `t2` (1/8 end mill), 2 target(s), 4.000 mm deep
  op1 `Star`: tool `t3` (1/4 60 degree V-bit), 2 target(s), 4.000 mm deep
star-female-1-T2.nc (153 lines):
  Extents (machine coordinates, mm, tool centre): X 30.621..69.379, Y 30.621..69.379, Z -4.000..10.000
    The cutter's edge reaches X 29.034..70.967, Y 29.034..70.967; keep clamps outside that.
  Estimated time: 35 s (cutting 32 s, dwells 3 s); 294 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
star-female-2-T1.nc (408 lines):
  Extents (machine coordinates, mm, tool centre): X 20.000..80.000, Y 20.000..80.000, Z -4.000..10.000
    The cutter's edge reaches X 16.825..83.175, Y 16.825..83.175; keep clamps outside that.
  Estimated time: 54 s (cutting 51 s, dwells 3 s); 708 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Estimated total time: 1 min 29 s (cutting 1 min 23 s, dwells 6 s); 1.0 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Inlay op1 `Star` (female): deepest cut 4.000 mm below the stock top, a flat floor at depth_mm 4.000 mm; V-bit floor ridges 0.400 mm where no clearing pass reaches.
star-male.dcv:22:5: warning: unused_tool: tool `t1` (`Set me`) is defined but not used by any enabled operation
warning: inlay_male_mirrored: operation `op1`: it is a male inlay, so its design is cut mirrored about X = 50 mm, the middle of its targets, to seat face down in the female
warning: tool_change_avoidable: tool `t2` (`1/8 end mill`) is used by operations `op1`, which do not run one after another, so it is fitted more than once; put those operations next to each other to save a tool change (operations always run in the order listed)
Wrote 3 programs; run them in this order:
  1. star-male-1-T2.nc  Before you start: fit T2 1/8 end mill and set work zero (X, Y and Z) with it.
  2. star-male-2-T1.nc  Before you start: fit T1 1/4 60 degree V-bit and set Z zero with it. Keep X and Y zero.
  3. star-male-3-T2.nc  Before you start: fit T2 1/8 end mill and set Z zero with it. Keep X and Y zero.
This program does not change the tool length offset (G43.1). If your sender sets one, set it before you set Z zero.
If you used G51 on this controller, soft-reset it and check that $G shows G50 before you start: grblHAL's G50 does not clear a scale that leaves X at 1, such as G51 Z-1.
Post-processor grblhal.
  op1 (clearing t2) `Star`: tool `t2` (1/8 end mill), 2 target(s), 5.000 mm deep
  op1 `Star`: tool `t3` (1/4 60 degree V-bit), 2 target(s), 5.000 mm deep
  op1 (cut-out t2) `Star`: tool `t2` (1/8 end mill), 2 target(s), 12.200 mm deep
star-male-1-T2.nc (296 lines):
  Extents (machine coordinates, mm, tool centre): X 14.745..85.255, Y 14.745..85.255, Z -5.000..10.000
    The cutter's edge reaches X 13.158..86.842, Y 13.158..86.842; keep clamps outside that.
  Estimated time: 1 min 48 s (cutting 1 min 45 s, dwells 3 s); 648 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
star-male-2-T1.nc (66 lines):
  Extents (machine coordinates, mm, tool centre): X 13.155..86.845, Y 13.156..86.845, Z -5.000..10.000
    The cutter's edge reaches X 9.980..90.020, Y 9.981..90.020; keep clamps outside that.
  Estimated time: 33 s (cutting 30 s, dwells 3 s); 74 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
star-male-3-T2.nc (104 lines):
  Extents (machine coordinates, mm, tool centre): X 16.000..84.000, Y 16.000..84.000, Z -12.200..10.000
    The cutter's edge reaches X 14.412..85.588, Y 14.412..85.588; keep clamps outside that.
  Estimated time: 1 min 4 s (cutting 1 min 1 s, dwells 3 s); 31 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Estimated total time: 3 min 25 s (cutting 3 min 16 s, dwells 9 s); 753 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Inlay op1 `Star` (male): deepest cut 5.000 mm below the stock top, the plug's start plane 3.000 mm below the stock top and the surround's flat floor plug_depth_mm 2.000 mm below that; V-bit floor ridges 0.200 mm where no clearing pass reaches; the cut-out frees the plug 4.000 mm out from the design, in the floor, through the board to 0.200 mm below it.
```

Each **tool run** is a program: on either side the end mill's clearing
pass first, then the V-bit, and on the male the end mill's cut-out last.

| Program | Bit | What it cuts |
| --- | --- | --- |
| `star-female-1-T2.nc` | 3.175 mm end mill | The pocket's flat floor, 4 mm deep, in rings round the circle |
| `star-female-2-T1.nc` | 60° V-bit | The pocket's walls, from the surface down to the floor, and the floor's corners, up into each of the star's points |
| `star-male-1-T2.nc` | 3.175 mm end mill | The band's flat floor round the plug, 5 mm deep, and the floor of the hole where the circle is |
| `star-male-2-T1.nc` | 60° V-bit | The plug's walls, the band's outer wall, and the floor's corners |
| `star-male-3-T2.nc` | 3.175 mm end mill | The cut-out: round the plug in the band's floor, 4 mm out from the star, from the floor through the board, leaving four tabs 3 mm tall |

Each program passed the safety checks before it was written, including
the one that holds the V-bit's whole cone, not just its tip, to the
material its side may cut: the star for the female, the band round the
mirrored star for the male ([Safety](../user-guide/safety.md), rule 14).
The cut-out is held to the band's flat floor, its whole cutter at least
clear of the plug's wall and the band's outer wall, and round its tabs.

## 9. Read the programs

The clearing passes are named after the operation:

<!-- doc-test: contains star-male-1-T2.nc -->
```text
( Operation 1: Star clearing t2 )
( Tool 2: 1/8 end mill )
```

The male's V-bit program starts round the outside of the band:

<!-- doc-test: excerpt star-male-2-T1.nc -->
```text
( Project: star )
( Generated by DreamCarve on 2026-10-30 )
G90
G17
G94
G80
G40
G50
G21
G54
( Operation 1: Star )
( Tool 1: 1/4 60 degree V-bit )
G0 Z10.000
M3 S18000
G4 P3.000
X50.000 Y13.156
Z1.000
G1 Z-5.000 F300.000
G3 X56.378 Y17.515 I-0.003 J6.850 F1000.000
G1 X63.578 Y35.771
```

The bit comes down at (50, 13.156), below the star's bottom tip, onto the
floor the end mill cleared, and plunges at the plunge rate to Z −5. From
there it runs round the band's outer edge: an arc round the tip, then
straight along the tip's side. The band reaches 8 mm beyond the star at
the start plane; at the floor, 2 mm deeper, the bit's tip runs
2 × tan 30° = 1.155 mm inside that, 36.845 mm from the centre, so that
its side cuts the band's outer wall at 30° from the vertical. The program then traces the
plug's foot round the star and the circle, and clears the floor's
corners.

The cut-out comes down onto the floor beside the star's left tip and
cuts its first lap 2 mm below the floor, Z −7:

<!-- doc-test: contains star-male-3-T2.nc -->
```text
( Operation 1: Star cut-out t2 )
( Tool 2: 1/8 end mill )
G0 Z10.000
M3 S18000
G4 P3.000
X18.532 Y46.279
Z1.000
G1 Z-7.000 F300.000
G2 X18.532 Y53.721 I1.468 J3.721 F1000.000
```

It runs 4 mm out from the star all the way round: round each tip on an
arc of 4 mm radius, and into each inner corner to a point 4 mm from both
edges. It never runs round the circle: the circle's hole stays in the
plug. On its last two passes, at Z −11 and Z −12.2, it rises to Z −9,
3 mm above the bottom of the board, over each of the four tabs.

## 10. Before you cut

Read [Safety](../user-guide/safety.md) once before your first cut. For
this inlay:

1. **Mark the boards.** Pencil "female, top" on the walnut and "male,
   top" on the maple, on the edge that will be at the back of the
   machine (+Y). You need the male's top edge when you turn the plug
   over.
2. **Fix the stock.** The `Extents` lines are boxes; the cuts are
   star-shaped. On the female, the cut stays inside the box from 16.8 to
   83.2 mm. On the male, the V-bit's box reaches to within 10 mm of the
   board's edges, but only at the star's tips, on the board's centre
   lines. Along the diagonals the band reaches only about 23 mm from the
   centre, so clamp at the board's four corners, with low clamps, or
   hold the whole underside with double-sided tape. Keep everything off
   the middle of each edge. The male's cut-out runs inside the band, so
   clamps that clear the V-bit clear it too; the plug it frees is held
   by its four tabs, not by the clamps.
3. **Use a spoilboard**: the cut-out goes 0.2 mm into it. Do an air cut
   of each program with Z zero 20 mm above the board.

## 11. Cut the female

1. Fit the 3.175 mm end mill and set work zero with it: X and Y at the
   front-left corner of the walnut, Z on its top. Run
   `star-female-1-T2.nc`: it clears the pocket's floor 4 mm deep round
   the circle. Vacuum the chips out.
2. Fit the V-bit. Set Z zero with it on the top of the board, outside
   the pocket; touch it off gently. Run `star-female-2-T1.nc`: the walls,
   and the floor into each point of the star.

The pocket should have clean walls sloping out at 30° from the vertical,
from the floor to the surface, each point of the star running up to the surface in a sharp
V, and the circle standing in the middle with its own sloped wall.

## 12. Cut the male

The same, on the maple: the end mill with work zero at the front-left
corner and Z on the top, `star-male-1-T2.nc`; then the V-bit with Z
zero on the top, `star-male-2-T1.nc`.

The plug is the star standing 5 mm proud of a flat band, its walls
sloping out towards the band at the same angle, with a hole the shape of the circle
in its middle. The board outside the band is still at full thickness.

## 13. Cut the plug free

Outside the band the maple is still at full height, and turned over it
would rest on the walnut before the plug could enter, so the plug must
come out of its board. The third program does that.

1. Fit the end mill again and set Z zero with it on the top of the
   board, outside the band. Keep X and Y zero. Run `star-male-3-T2.nc`:
   the end mill comes down onto the band's floor 4 mm out from the
   star and cuts round it, 2 mm a pass, through the board and 0.2 mm
   into the spoilboard. Below 3 mm from the bottom it lifts over four
   tabs, each 6 mm long, which hold the piece in the board.
2. Cut the four tabs with a knife or a fine saw, and lift the piece out.
   Pare or sand what is left of each tab off its edge.

What you keep is the plug on a backing 7 mm thick (12 mm less the 5 mm
band), with a rim of the band's floor 4 mm wide round it. The rim is the
band's flat floor, 2 mm below the plug's start plane, so it clears the
walnut's surface when the plug is seated.

Without a cut-out (`"cut_out": null`), saw the plug free instead: round
the plug, through the board, inside the band, anywhere between the
plug's foot and the band's outer wall. The band's flat floor is at
least 5.7 mm wide; you need not follow the plug closely, only stay off
both walls.

## 14. Test the fit

Turn the plug over **left to right**, about a vertical line, like
turning the page of a book, so that the edge you marked "top" is still
at the back. The male was cut mirrored about a vertical line, so only
that flip gives back the design as drawn. This star is symmetric, so it
fits either way; most designs fit only one way.

Press the plug into the pocket dry, by hand or with a light tap, without
glue. It should go in until its walls meet the pocket's, with no gap at
the surface round the star or the circle, and its backing should stop
about 2 mm above the walnut. Pull it out again (a thin blade in the band
helps).

- **A gap at the top, all round,** with the plug's backing still clear of
  the walnut: the plug's face is on the pocket's floor before its walls
  meet. Raise `glue_gap_mm` in both projects, by 0.25 mm at a time, and
  cut a new male. The female does not change.
- **The plug will not go in**, or its backing stops well above the 2 mm:
  lower `glue_gap_mm`, by 0.25 mm at a time, and cut a new male.

[Inlay: the fit](../user-guide/inlay.md#the-fit) says why. Once you
have the value for a bit and a wood, write it down: it holds for later
inlays with that bit.

## 15. Glue, clamp, and plane

1. Spread glue on the plug's walls and in the pocket. Too much is better
   than too little: the 1 mm gap under the plug takes the excess.
2. Seat the plug, top edge to the back, and press it home with a clamp or
   a press, with a flat caul on each side. Clamp until the glue squeezes
   out all round. Leave it to dry as the glue says.
3. Saw off the backing through the 2 mm gap between the band and the
   walnut, on a bandsaw with the walnut flat on the table.
4. Plane or sand the board flat, down to the walnut's surface. The star
   and the circle appear as you reach it.

After planing, there should be no gap round the star or the circle (a
0.1 mm feeler gauge should not enter), and the star's points should be
sharp.

## Next steps

- Every setting, the cone-height rule for choosing the bit, and what each
  of the four values changes: [Inlay](../user-guide/inlay.md).
- Draw your own design: any closed shapes, with holes, as for a
  [V-Carve](../user-guide/v-carve.md). Keep its narrowest parts wider than
  the bit's point can reach into, or they come out as fine V's that glue
  poorly.
- A cutting board: Corbin's Workshop uses about 6 mm of inlay depth for
  a board that will be planed hard and used. That is past the 60° bit's
  5.5 mm cone, so use a 30° 6.35 mm bit (an 11.8 mm cone), and a male
  board thick enough to keep 6 mm under its band. See [Choosing the
  bit](../user-guide/inlay.md#choosing-the-bit).
