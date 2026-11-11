# An inlay pair

This chapter takes one drawing, the monogram "DC" (for DreamCarve), to one
project and five G-code programs, and then to dark letters inlaid in a
light board. It is the V-bit inlay method Corbin's Workshop documents for
cutting boards ([Inlay](../user-guide/inlay.md) says more), done with two
boards cut from one project:

- the **female** board, the light one, is the project's stock. It gets a
  **pocket** the shape of the letters, 4 mm deep, with walls sloping at
  the V-bit's angle. The D's counter, the hole in the middle of the D, is
  an island: it stays standing in the pocket;
- the **male** board, the dark one, is the Inlay's **male board**. It gets
  a **plug**: the letters, mirrored, so that they read as a reversed
  "DC", standing 5 mm proud of a flat band carved round them, with walls
  sloping the other way, and a hole where the D's counter is. The band's
  outer edge is an upright wall the end mill cuts, its inward curves
  rounded off; the end mill then cuts the plug free of the board, on a
  rim of the band. Turned over, the plug fits the pocket: its walls meet
  the pocket's walls, and its face stops 1 mm short of the pocket's
  floor, which leaves room for glue.

You glue the plug into the pocket, saw off what is left of the dark board
above the light one's surface, and plane the board flat.

One operation describes both boards with one set of numbers, so they
always agree: change the fit, and both boards' programs follow.

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
  [`dc-inlay.svg`](https://raw.githubusercontent.com/grayzu/dreamcarve/main/tests/fixtures/dc-inlay.svg).
  It is also in the repository at `tests/fixtures/dc-inlay.svg`.
- Calipers, to measure the boards.
- For the cut: two boards sold as 12 mm, at least 110 × 80 mm, of
  contrasting hardwoods, a light one for the female and a dark one for
  the male: maple and walnut, say, or oak and walnut. Any pair works; the
  plug shows best dark in a light board. A 60° 6.35 mm V-bit, a
  3.175 mm two-flute upcut end mill with at least 14.2 mm of flute (see
  [step 5](#5-paste-the-operation)), a router controlled by grblHAL, and
  a spoilboard.
- For the glue-up: wood glue, a clamp or a press with two flat cauls, a
  bandsaw, and a hand plane or a sander.

## 1. Put the drawing in a folder

Make an empty folder and put `dc-inlay.svg` in it:

```sh
mkdir inlay
cd inlay
```

Run every command below from that folder.

The drawing has three shapes: the letters D and C, in Droid Serif Bold
converted to outlines, 40 mm tall (the C's curves reach a little above and
below), 3 mm apart and 73.4 mm across together; and a 90 × 60 mm
rectangle round them, the **frame**. Nothing cuts the frame. It is there
because `init` makes the stock the drawing plus 10 mm all round: with the
frame, the stock is 110 × 80 mm with the letters at its centre, and the
male's band, which reaches 8 mm beyond the letters, keeps more than 10 mm
of board at every edge.

## 2. Create the project

One project cuts both boards. Make it, naming it with `-o`:

<!-- doc-test: run -->
```sh
dreamcarve init dc-inlay.svg -o dc.dcv
```

```text
Imported 3 shape(s) from dc-inlay.svg: page units mm, 1 user unit = 1.0000 mm.
Wrote dc.dcv: stock 110.00 × 80.00 mm, post-processor grblhal, 3 shape(s) targeted by Profile.
Values to set; export refuses until every one is set:
dc.dcv:10:21: stock.thickness_mm is not set (mm)
dc.dcv:31:22: tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`
dc.dcv:32:24: tools[t1].plunge_mm_min is not set (mm/min) for tool `Set me`
dc.dcv:33:14: tools[t1].rpm is not set (RPM) for tool `Set me`
dc.dcv:34:23: tools[t1].step_down_mm is not set (mm) for tool `Set me`
dc.dcv:51:19: operations[op1].depth_mm is not set (mm) for operation `Profile`
Next: edit dc.dcv and replace each `null` listed above. To cut through, `depth_mm` is the thickness plus `through_cut_allowance_mm`. Then run `dreamcarve check dc.dcv`.
```

The project has three shapes:

| Id | Name | What it is |
| --- | --- | --- |
| `i1` | `letter-D` | The D, with its counter; on the stock from X 18.3 to 56.6 mm |
| `i2` | `letter-C` | The C; from X 59.6 to 91.7 mm |
| `i3` | `frame` | The 90 × 60 mm rectangle; not cut |

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
reach their full 6.35 mm, is 5.5 mm, and the deepest either board of
this inlay is cut is 5 mm, on the male board (see [Choosing the bit](../user-guide/inlay.md#choosing-the-bit)).
Use a 60° or 90° bit for inlays: narrower bits fit less closely, as
[Choosing a V-bit for an inlay](../user-guide/inlay.md#choosing-a-v-bit-for-an-inlay)
explains.

Add the file's tools to your library, list it, and copy both tools into
the project:

<!-- doc-test: run -->
```sh
dreamcarve tools import my-tools.json
dreamcarve tools list
dreamcarve tools add dc.dcv lt1
dreamcarve tools add dc.dcv lt2
```

```text
`1/8 end mill`: imported (lt1)
`1/4 60 degree V-bit`: imported (lt2)
2 imported, 0 already present, 0 replaced; library: /Users/you/Library/Application Support/dreamcarve/tools.json
ID   NAME                 VENDOR  PRODUCT  CUTTER     DIAMETER_MM  FEED_MM_MIN  PLUNGE_MM_MIN  RPM    NUMBER  SOURCE
lt1  1/8 end mill         -       -        end mill   3.175        1000         300            18000  2       -
lt2  1/4 60 degree V-bit  -       -        V-bit 60°  6.35         1000         300            18000  1       -
Added `1/8 end mill` (lt1) to dc.dcv as tool t2.
Added `1/4 60 degree V-bit` (lt2) to dc.dcv as tool t3.
```

The library gives each tool it imports the next free id. If your library
already has tools (from the earlier chapters), `import` says the end mill
is already present and the ids differ: use the ids `tools list` shows.
The project now has `t1` ("Set me", unused), `t2` (the end mill) and
`t3` (the V-bit).

## 4. Set the female board

The project's stock is the female board, the light one. Measure it with
calipers and set its thickness: change the stock's `thickness_mm` line
to:

<!-- doc-test: edit dc.dcv -->
```json
"thickness_mm": 12.0,
```

This chapter uses 12 mm; boards sold as 12 mm are rarely exactly that,
so measure yours and set what you measure. The male board gets its own
thickness in [step 6](#6-set-the-male-board).

## 5. Paste the operation

Replace the whole `"operations"` block, from `"operations": [` down to
its closing `],`, with this, and save:

<!-- doc-test: paste dc.dcv operations -->
```json
"operations": [
  {
    "kind": "inlay",
    "id": "op1",
    "name": "DC",
    "enabled": true,
    "targets": [ "i1", "i2" ],
    "tool": "t3",
    "male_board": { "width_mm": null, "height_mm": null, "thickness_mm": null },
    "depth_mm": 4.0,
    "glue_gap_mm": 1.0,
    "plug_depth_mm": 2.0,
    "margin_mm": 8.0,
    "outline_rounding_mm": 10.0,
    "boundary": [],
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

One **Inlay** operation describes both boards with the same values:

| Field | Value | What it does here |
| --- | --- | --- |
| `targets` | `i1`, `i2` | The two letters. The D's counter, inside the D, is an island in the pocket, and a hole in the plug. |
| `tool` | `t3` | The 60° V-bit, on both boards. |
| `male_board` | all `null` | The male board: its width and height `null` are the stock's, 110 × 80 mm, sharing its front-left corner; its thickness is its own, set in the next step. Until it is set, only the female is cut. |
| `depth_mm` | 4 | The pocket is 4 mm deep (Corbin's "Inlay Depth"). |
| `glue_gap_mm` | 1 | The plug's face stops 1 mm above the pocket's floor (Corbin's "Inlay Fit Adjustment"). It is the value you change to adjust the fit. |
| `plug_depth_mm` | 2 | The male's flat band is cut 2 mm deeper than the plug's walls need, so the seated plug's backing stands 2 mm clear of the female, room to saw it off. |
| `margin_mm` | 8 | The male's band reaches 8 mm beyond the letters (Corbin's "Plug Stock Offset"). |
| `outline_rounding_mm` | 10 | The male's band is rounded: its outer edge curves in no tighter than a 10 mm radius, so it runs smoothly over the dips between and round the letters instead of following every notch, and the gap between the letters is filled. |
| `boundary` | `[]` | No drawn edge: the band is the margin and rounding above. Leave it empty. |
| `clearing` | `t2` | The end mill clears the flat floor first, on both boards; the V-bit then cuts the walls and the corners the end mill cannot reach. On the male it also cuts the band's outer edge, upright. |
| `step_over_fraction` | 0.1 | Where the V-bit clears floor itself, its rings are close enough to leave ridges only a tenth of the floor's depth, and a fifth at most where they turn a corner: under the glue gap. |
| `safe_z_mm` | 10 | Travel 10 mm above the board. |
| `cut_out` | `t2`, four tabs | The male's last pass cuts the plug free of its board (below). |

The **cut-out** is the male's last pass: the end mill (`t2`) cuts the
plug free of its board, so you need not saw it out. It runs along the
outer edge of the band's flat floor, its cutter's edge on the band's
upright outer edge, so it cuts nothing the carve left standing: its
centre runs 1.588 mm (its radius) inside that edge, at least 6.412 mm
out from the mirrored letters and rounded as the band is, from the
floor 5 mm down through the male board and 0.2 mm into the spoilboard
(`through_cut_allowance_mm`). Four **tabs**, 6 mm long and 3 mm tall
from the bottom of the board, hold the plug in place until you cut
them. The female ignores the cut-out: its pocket stays in its board.

**Check the end mill's flute length.** The cut-out goes through the
male board, so the end mill's flutes must be at least the board's
thickness plus the through-cut allowance plus 2 mm long: 12 + 0.2 + 2 =
14.2 mm here. Many 1/8 in end mills have only 12.7 mm (1/2 in) of
flute. If yours is shorter, use one with longer flutes for the cut-out;
or a 6.35 mm end mill, which the 8 mm band still holds (it needs
`margin_mm` at least 7.505 while the 3.175 mm end mill clears the floor:
see [Cutting the plug free](../user-guide/inlay.md#cutting-the-plug-free));
or set `"cut_out": null` and saw the plug free.

See [Inlay](../user-guide/inlay.md) for every setting.

Check the project:

<!-- doc-test: run -->
```sh
dreamcarve check dc.dcv
```

```text
dc.dcv:22:5: warning: unused_tool: tool `t1` (`Set me`) is defined but not used by any enabled operation
warning: entry_fallback: operation `op1`: there is no room for a `ramp` entry in `letter-C` (instance `i2`, profile 0), so its passes start with a straight plunge at the plunge rate instead; set the operation's entry to `plunge` to stop this warning
dc.dcv:80:76: warning: inlay_male_board_unset: operation `op1` (male board): its male board has no thickness set, so only the female is cut; measure the male board and set male_board.thickness_mm to cut the male too
warning: arc_fit_fallback: operation `op1`: 1 run(s) of straight moves kept as written because, once rounded, the arc fitted to them would be rejected by the controller or stray from them
Inlay op1 `DC` (female board): deepest cut 4.000 mm below the stock top, a flat floor at depth_mm 4.000 mm; V-bit floor ridges 0.400 mm along the rings and up to 0.800 mm at their corners where no clearing pass reaches.
Female board, the stock, 110.000 mm × 80.000 mm × 12.000 mm:
Program dc-female-1-T2.nc (T2):
  Extents (machine coordinates, mm, tool centre): X 27.149..87.772, Y 23.897..56.103, Z -4.000..10.000
    The cutter's edge reaches X 25.562..89.360, Y 22.309..57.691; keep clamps outside that.
  Estimated time: 30 s (cutting 27 s, dwells 3 s); 362 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program dc-female-2-T1.nc (T1):
  Extents (machine coordinates, mm, tool centre): X 18.319..91.093, Y 20.001..60.000, Z -4.000..10.000
    The cutter's edge reaches X 15.144..94.268, Y 16.826..63.175; keep clamps outside that.
  Estimated time: 1 min 16 s (cutting 1 min 13 s, dwells 3 s); 1.1 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Estimated total time: 1 min 46 s (cutting 1 min 40 s, dwells 6 s); 1.4 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
dc.dcv: 0 error(s), 4 warning(s)
```

The female board is ready; the male is not cut yet. `inlay_male_board_unset`
points at the male board's thickness, the one value left to set.

## 6. Set the male board

Measure the male board, the dark one, and set its thickness. Change the
operation's `male_board` line to:

<!-- doc-test: edit dc.dcv -->
```json
"male_board": { "width_mm": null, "height_mm": null, "thickness_mm": 12.0 },
```

The male board's thickness matters most: the plug's surround is cut
5 mm deep, and the board under it holds the plug together until the
glue dries. A male board under 11 mm leaves less than 6 mm, and `check`
warns `inlay_plug_base_thin`. The cut-out also goes through the board by
its thickness: set too thin, it leaves the plug attached; set too thick,
it cuts deeper into the spoilboard.

The male board shares the stock's front-left corner and, with `width_mm`
and `height_mm` left `null`, its size. If your dark board is smaller or
larger, set its size too: the male's band is clipped to the board where
it runs past it, as long as the letters leave the band room there.

## 7. Check the project

<!-- doc-test: run -->
```sh
dreamcarve check dc.dcv
```

```text
dc.dcv:22:5: warning: unused_tool: tool `t1` (`Set me`) is defined but not used by any enabled operation
warning: entry_fallback: operation `op1`: there is no room for a `ramp` entry in `letter-C` (instance `i2`, profile 0), so its passes start with a straight plunge at the plunge rate instead; set the operation's entry to `plunge` to stop this warning
warning: inlay_male_mirrored: operation `op1` (male board): it is a male inlay, so its design is cut mirrored about X = 55 mm, the middle of its targets, to seat face down in the female
warning: tool_change_avoidable: tool `t2` (`1/8 end mill`) is used by operations `op1`, which do not run one after another, so it is fitted more than once; put those operations next to each other to save a tool change (operations always run in the order listed)
warning: arc_fit_fallback: operation `op1`: 1 run(s) of straight moves kept as written because, once rounded, the arc fitted to them would be rejected by the controller or stray from them
Inlay op1 `DC` (female board): deepest cut 4.000 mm below the stock top, a flat floor at depth_mm 4.000 mm; V-bit floor ridges 0.400 mm along the rings and up to 0.800 mm at their corners where no clearing pass reaches.
Inlay op1 `DC` (male board): deepest cut 5.000 mm below the male board's top, the plug's start plane 3.000 mm below the male board's top and the surround's flat floor plug_depth_mm 2.000 mm below that; V-bit floor ridges 0.200 mm along the rings and up to 0.400 mm at their corners where no clearing pass reaches; clearing pass `t2` cuts the band's outer edge upright, its inward curves rounded to 10.000 mm; the cut-out frees the plug along the floor's outer edge, its centre 1.587 mm inside the band's edge and at least 6.412 mm out from the design, through the board to 0.200 mm below it.
Female board, the stock, 110.000 mm × 80.000 mm × 12.000 mm:
Program dc-female-1-T2.nc (T2):
  Extents (machine coordinates, mm, tool centre): X 27.149..87.772, Y 23.897..56.103, Z -4.000..10.000
    The cutter's edge reaches X 25.562..89.360, Y 22.309..57.691; keep clamps outside that.
  Estimated time: 30 s (cutting 27 s, dwells 3 s); 362 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program dc-female-2-T1.nc (T1):
  Extents (machine coordinates, mm, tool centre): X 18.319..91.093, Y 20.001..60.000, Z -4.000..10.000
    The cutter's edge reaches X 15.144..94.268, Y 16.826..63.175; keep clamps outside that.
  Estimated time: 1 min 16 s (cutting 1 min 13 s, dwells 3 s); 1.1 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Male board, 110.000 mm × 80.000 mm × 12.000 mm:
Program dc-male-1-T2.nc (T2):
  Extents (machine coordinates, mm, tool centre): X 11.915..98.095, Y 13.019..66.974, Z -5.000..10.000
    The cutter's edge reaches X 10.328..99.682, Y 11.432..68.562; keep clamps outside that.
  Estimated time: 5 min 26 s (cutting 5 min 23 s, dwells 3 s); 864 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program dc-male-2-T1.nc (T1):
  Extents (machine coordinates, mm, tool centre): X 17.160..92.836, Y 18.278..61.723, Z -5.000..10.000
    The cutter's edge reaches X 13.985..96.011, Y 15.103..64.898; keep clamps outside that.
  Estimated time: 56 s (cutting 53 s, dwells 3 s); 605 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program dc-male-3-T2.nc (T2):
  Extents (machine coordinates, mm, tool centre): X 11.911..98.095, Y 13.018..66.979, Z -12.200..10.000
    The cutter's edge reaches X 10.324..99.682, Y 11.431..68.567; keep clamps outside that.
  Estimated time: 1 min 19 s (cutting 1 min 16 s, dwells 3 s); 31 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Estimated total time: 9 min 26 s (cutting 9 min 11 s, dwells 15 s); 3.0 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
dc.dcv: 0 error(s), 5 warning(s)
```

No errors. The warnings:

- `unused_tool`: the "Set me" tool `init` made. Delete its block from
  `"tools"` if you like.
- `entry_fallback`, on the female board: the end mill's clearing pass
  ramps into the floor at 15°, but the C's stroke is too narrow for a
  ramp at 4 mm deep, so there it plunges straight down at the plunge
  rate, which a 3.175 mm upcut end mill does well. The male's band is
  wide enough everywhere, so it ramps. Ignore it here.
- `inlay_male_mirrored`, on the male board: a note, not a problem. The
  plug is glued in face down, so the male cuts the letters mirrored left
  to right, about the vertical line X = 55 mm through the middle of the
  monogram: it carves a reversed "DC", the C on the left and the D on
  the right, both facing left. Draw the design as it should read in the
  finished board; never mirror it yourself.
- `tool_change_avoidable`, on the male board: the end mill clears the
  floor first and cuts the plug out last, with the V-bit between, so you
  fit it twice. That order is required: the plug must stay in its board
  until it is carved. Ignore it here.
- `arc_fit_fallback`, on the female board: one short run of the floor's
  centre rings is written as straight moves; it cuts the same.

The Inlay lines give each board's depths:

- the **female** pocket is 4 mm deep, with a flat floor;
- the **male**'s plug is cut from its **start plane**, 3 mm below the
  male board's top (`depth_mm − glue_gap_mm`): there, the plug's walls
  pass exactly through the mirrored letters. Above it, they slope in at
  30° from the vertical, as the pocket's walls do going down. The band
  round the plug is cut flat 2 mm deeper (`plug_depth_mm`), 5 mm below
  the top in all.

Turned over, the plug enters the pocket 3 mm, until its walls meet the
pocket's, and its face stops 1 mm short of the 4 mm floor: the glue gap.
The band is then 2 mm above the female's surface.

## 8. Export the programs

<!-- doc-test: run -->
```sh
dreamcarve export dc.dcv
```

```text
dc.dcv:22:5: warning: unused_tool: tool `t1` (`Set me`) is defined but not used by any enabled operation
warning: entry_fallback: operation `op1`: there is no room for a `ramp` entry in `letter-C` (instance `i2`, profile 0), so its passes start with a straight plunge at the plunge rate instead; set the operation's entry to `plunge` to stop this warning
warning: inlay_male_mirrored: operation `op1` (male board): it is a male inlay, so its design is cut mirrored about X = 55 mm, the middle of its targets, to seat face down in the female
warning: tool_change_avoidable: tool `t2` (`1/8 end mill`) is used by operations `op1`, which do not run one after another, so it is fitted more than once; put those operations next to each other to save a tool change (operations always run in the order listed)
warning: arc_fit_fallback: operation `op1`: 1 run(s) of straight moves kept as written because, once rounded, the arc fitted to them would be rejected by the controller or stray from them
Wrote 5 programs; run them in this order:
Female board, the stock, 110.000 mm × 80.000 mm × 12.000 mm:
  1. dc-female-1-T2.nc  Before you start: fit T2 1/8 end mill and set work zero (X, Y and Z) with it.
  2. dc-female-2-T1.nc  Before you start: fit T1 1/4 60 degree V-bit and set Z zero with it. Keep X and Y zero.
Male board, 110.000 mm × 80.000 mm × 12.000 mm:
  1. dc-male-1-T2.nc  Before you start: fit T2 1/8 end mill and set work zero (X, Y and Z) with it.
  2. dc-male-2-T1.nc  Before you start: fit T1 1/4 60 degree V-bit and set Z zero with it. Keep X and Y zero.
  3. dc-male-3-T2.nc  Before you start: fit T2 1/8 end mill and set Z zero with it. Keep X and Y zero.
This program does not change the tool length offset (G43.1). If your sender sets one, set it before you set Z zero.
If you used G51 on this controller, soft-reset it and check that $G shows G50 before you start: grblHAL's G50 does not clear a scale that leaves X at 1, such as G51 Z-1.
Post-processor grblhal.
  op1 (clearing t2) `DC`: tool `t2` (1/8 end mill), 2 target(s), 4.000 mm deep
  op1 `DC`: tool `t3` (1/4 60 degree V-bit), 2 target(s), 4.000 mm deep
  op1 (clearing t2) `DC`: tool `t2` (1/8 end mill), 2 target(s), 5.000 mm deep on the male board
  op1 `DC`: tool `t3` (1/4 60 degree V-bit), 2 target(s), 5.000 mm deep on the male board
  op1 (cut-out t2) `DC`: tool `t2` (1/8 end mill), 2 target(s), 12.200 mm deep on the male board
Female board, the stock, 110.000 mm × 80.000 mm × 12.000 mm:
dc-female-1-T2.nc (137 lines):
  Extents (machine coordinates, mm, tool centre): X 27.149..87.772, Y 23.897..56.103, Z -4.000..10.000
    The cutter's edge reaches X 25.562..89.360, Y 22.309..57.691; keep clamps outside that.
  Estimated time: 30 s (cutting 27 s, dwells 3 s); 362 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
dc-female-2-T1.nc (1211 lines):
  Extents (machine coordinates, mm, tool centre): X 18.319..91.093, Y 20.001..60.000, Z -4.000..10.000
    The cutter's edge reaches X 15.144..94.268, Y 16.826..63.175; keep clamps outside that.
  Estimated time: 1 min 16 s (cutting 1 min 13 s, dwells 3 s); 1.1 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Estimated female board time: 1 min 46 s (cutting 1 min 40 s, dwells 6 s); 1.4 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Male board, 110.000 mm × 80.000 mm × 12.000 mm:
dc-male-1-T2.nc (8822 lines):
  Extents (machine coordinates, mm, tool centre): X 11.915..98.095, Y 13.019..66.974, Z -5.000..10.000
    The cutter's edge reaches X 10.328..99.682, Y 11.432..68.562; keep clamps outside that.
  Estimated time: 5 min 26 s (cutting 5 min 23 s, dwells 3 s); 864 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
dc-male-2-T1.nc (680 lines):
  Extents (machine coordinates, mm, tool centre): X 17.160..92.836, Y 18.278..61.723, Z -5.000..10.000
    The cutter's edge reaches X 13.985..96.011, Y 15.103..64.898; keep clamps outside that.
  Estimated time: 56 s (cutting 53 s, dwells 3 s); 605 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
dc-male-3-T2.nc (218 lines):
  Extents (machine coordinates, mm, tool centre): X 11.911..98.095, Y 13.018..66.979, Z -12.200..10.000
    The cutter's edge reaches X 10.324..99.682, Y 11.431..68.567; keep clamps outside that.
  Estimated time: 1 min 19 s (cutting 1 min 16 s, dwells 3 s); 31 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Estimated male board time: 7 min 40 s (cutting 7 min 31 s, dwells 9 s); 1.5 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Estimated total time: 9 min 26 s (cutting 9 min 11 s, dwells 15 s); 3.0 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Inlay op1 `DC` (female board): deepest cut 4.000 mm below the stock top, a flat floor at depth_mm 4.000 mm; V-bit floor ridges 0.400 mm along the rings and up to 0.800 mm at their corners where no clearing pass reaches.
Inlay op1 `DC` (male board): deepest cut 5.000 mm below the male board's top, the plug's start plane 3.000 mm below the male board's top and the surround's flat floor plug_depth_mm 2.000 mm below that; V-bit floor ridges 0.200 mm along the rings and up to 0.400 mm at their corners where no clearing pass reaches; clearing pass `t2` cuts the band's outer edge upright, its inward curves rounded to 10.000 mm; the cut-out frees the plug along the floor's outer edge, its centre 1.587 mm inside the band's edge and at least 6.412 mm out from the design, through the board to 0.200 mm below it.
```

The programs are listed by board, each board a setup of its own: you
set work zero again on the male board with its first program. Each
**tool run** is a program: on either board the end mill's clearing pass
first, then the V-bit, and on the male the end mill's cut-out last. The
female takes about a minute and a half of cutting, the male about seven
and a half minutes, not counting rapids or tool changes.

| Program | Bit | What it cuts |
| --- | --- | --- |
| `dc-female-1-T2.nc` | 3.175 mm end mill | The pocket's flat floor, 4 mm deep, in the wide parts of both letters, round the D's counter |
| `dc-female-2-T1.nc` | 60° V-bit | The pocket's walls, from the surface down to the floor, the serifs and thin strokes the end mill cannot enter, and the floor's corners |
| `dc-male-1-T2.nc` | 3.175 mm end mill | The band's flat floor round the plug, 5 mm deep, out to the band's outer edge, which it cuts upright, and the floor of the hole where the D's counter is |
| `dc-male-2-T1.nc` | 60° V-bit | The plug's walls, the narrow gap between the letters, and the floor's corners by the plug's foot |
| `dc-male-3-T2.nc` | 3.175 mm end mill | The cut-out: round both letters along the band's outer edge, about 6.4 mm out from them, from the floor through the board, leaving four tabs 3 mm tall |

Each program passed the safety checks before it was written, each
against its own board: the female's against the stock, the male's
against the male board, its thickness and its size. They include the one
that holds the V-bit's whole cone, not just its tip, to the material its
board may cut: the letters for the female, the band round the mirrored
letters for the male ([Safety](../user-guide/safety.md), rule 14). The
V-bit is also held clear of the band's outer edge: beyond it the board
stands to its full height, so no part of the bit's cone may cross that
edge below the top. The cut-out is held to its path along the band's
outer edge, its whole cutter on the band's flat floor, clear of the
plug's wall and inside the band's edge, and round its tabs, and to the
male board's bottom less its 0.2 mm allowance.

## 9. Read the programs

The clearing passes are named after the operation:

<!-- doc-test: contains dc-male-1-T2.nc -->
```text
( Operation 1: DC clearing t2 )
( Tool 2: 1/8 end mill )
```

The male's V-bit program starts in a corner by the reversed D's stem:

<!-- doc-test: excerpt dc-male-2-T1.nc -->
```text
( Project: dc-inlay )
( Generated by DreamCarve on 2026-10-31 )
G90
G17
G94
G80
G40
G50
G21
G54
( Operation 1: DC )
( Tool 1: 1/4 60 degree V-bit )
G0 Z10.000
M3 S18000
G4 P3.000
X89.362 Y24.099
Z1.000
G1 Z-5.000 F300.000
```

The bit comes down at (89.362, 24.099), at the floor depth, Z −5, in the
corner of the floor between the reversed D's stem and its bottom serif,
where the end mill's round end could not reach. It clears the corners like it, and the 3 mm
strip of floor between the reversed C on the left and the reversed D on
the right, too narrow for the end mill, then traces the plug's foot
round both letters and the D's counter, 2 × tan 30° = 1.155 mm out from
them at the floor, so that its side cuts the plug's wall at 30° from the
vertical up to the top. It never goes near the band's outer **edge**: the
end mill cut that upright in the first program, out to 8 mm beyond the
letters and round the dips between them in curves no tighter than
10 mm.

The cut-out comes down onto the floor at the band's left edge, beside
the reversed C, and cuts its first lap 2 mm below the floor, Z −7:

<!-- doc-test: contains dc-male-3-T2.nc -->
```text
( Operation 1: DC cut-out t2 )
( Tool 2: 1/8 end mill )
G0 Z10.000
M3 S18000
G4 P3.000
X13.550 Y32.215
Z1.000
G1 Z-7.000 F300.000
```

It runs along the band's outer edge, its cutter's edge on the wall the
end mill cut in the first program, all the way round both letters as
one loop: across the gap between the letters it follows the edge's
smooth curve over the dip. It never runs round the D's counter: that hole
stays in the plug. On its last two passes, at Z −11 and Z −12.2, it
rises to Z −9, 3 mm above the bottom of the board, over each of the four
tabs.

## 10. Before you cut

Read [Safety](../user-guide/safety.md) once before your first cut. For
this inlay:

1. **Mark the boards.** Pencil "female, top" on the light board and
   "male, top" on the dark one, on the edge that will be at the back of
   the machine (+Y). You need the male's top edge when you turn the plug
   over.
2. **Fix the stock.** The `Extents` lines are boxes round the cuts. On
   the female, the cut stays inside the box from X 15.1 to 94.3 and Y 16.8
   to 63.2 mm. On the male, the end mill's first program reaches widest:
   to within 10.3 mm of the left and right edges and 11.4 mm of the front
   and back. Clamp at the four corners, with low clamps, or hold the
   whole underside with double-sided tape. The male's V-bit and cut-out
   run inside the band, so clamps that clear the first program clear
   them too; the plug it frees is held by its four tabs, not by the
   clamps.
3. **Use a spoilboard**: the cut-out goes 0.2 mm into it. Do an air cut
   of each program with Z zero 20 mm above the board.

## 11. Cut the female

1. Fit the 3.175 mm end mill and set work zero with it: X and Y at the
   front-left corner of the light board, Z on its top. Run
   `dc-female-1-T2.nc`: it clears the pocket's floor 4 mm deep in the
   wide parts of the letters. Vacuum the chips out.
2. Fit the V-bit. Set Z zero with it on the top of the board, outside
   the pocket; touch it off gently. Run `dc-female-2-T1.nc`: the walls,
   the serifs, and the floor into each corner.

The pocket should read "DC", with clean walls sloping out at 30° from the
vertical, from the floor to the surface, each serif running up to the
surface in a sharp V, and the D's counter standing in the D with its own
sloped wall.

## 12. Cut the male

The same, on the dark board: the end mill with work zero at the
front-left corner and Z on the top, `dc-male-1-T2.nc`; then the V-bit
with Z zero on the top, `dc-male-2-T1.nc`.

The plug is the reversed "DC" standing 5 mm proud of a flat band, its
walls sloping out towards the band at the same angle, with a hole the
shape of the D's counter in the D. The band's outer edge is an upright
wall, 5 mm high, that curves smoothly round the letters. The board
outside the band is still at full thickness.

## 13. Cut the plug free

Outside the band the dark board is still at full height, and turned over
it would rest on the light board before the plug could enter, so the
plug must come out of its board. The third program does that.

1. Fit the end mill again and set Z zero with it on the top of the
   board, outside the band. Keep X and Y zero. Run `dc-male-3-T2.nc`:
   the end mill comes down onto the band's floor at its outer edge,
   about 6.4 mm out from the letters, and cuts round them, 2 mm a pass, through the board and
   0.2 mm into the spoilboard. Below 3 mm from the bottom it lifts over
   four tabs, each 6 mm long, which hold the piece in the board.
2. Cut the four tabs with a knife or a fine saw, and lift the piece out.
   Pare or sand what is left of each tab off its edge.

What you keep is the plug on a backing 7 mm thick (12 mm less the 5 mm
band), with a rim of the band's floor round it, reaching about 4.8 mm out
from the letters. The rim is the
band's flat floor, 2 mm below the plug's start plane, so it clears the
light board's surface when the plug is seated.

Without a cut-out (`"cut_out": null`), saw the plug free instead: round
the plug, through the board, inside the band, anywhere between the
plug's foot and the band's outer edge. The band's flat floor is at
least 6.8 mm wide; you need not follow the plug closely, only stay off
the plug's wall and the band's edge.

## 14. Test the fit

The plug reads as a reversed "DC". Turn it over **left to right**, about
a vertical line, like turning the page of a book: hold it by its left
and right ends and roll it over its left or right edge, the plug's short
side, so that the edge you marked "top" is still at the back. Face down,
it now reads "DC" through the wood, and fits the pocket. The male was
cut mirrored about a vertical line, so only that flip gives back the
design as drawn: turned over its long edge, top to bottom, the letters
come out upside down, and the plug fits only once you spin it half a
turn.

Press the plug into the pocket dry, by hand or with a light tap, without
glue. It should go in until its walls meet the pocket's, with no gap at
the surface round the letters or the D's counter, and its backing should
stop about 2 mm above the light board. Pull it out again (a thin blade
in the band helps).

- **A gap at the top, all round,** with the plug's backing still clear
  of the light board: the plug's face is on the pocket's floor before its
  walls meet. Raise `glue_gap_mm` by 0.25 mm at a time, and cut a new
  male. The female does not change.
- **The plug will not go in**, or its backing stops well above the 2 mm:
  lower `glue_gap_mm`, by 0.25 mm at a time, and cut a new male.

`glue_gap_mm` changes only the male, so after changing it re-export the
male board's programs alone, on a new dark board, with `--board male`:

<!-- doc-test: run -->
```sh
dreamcarve export dc.dcv --board male
```

```text
dc.dcv:22:5: warning: unused_tool: tool `t1` (`Set me`) is defined but not used by any enabled operation
warning: inlay_male_mirrored: operation `op1` (male board): it is a male inlay, so its design is cut mirrored about X = 55 mm, the middle of its targets, to seat face down in the female
warning: tool_change_avoidable: tool `t2` (`1/8 end mill`) is used by operations `op1`, which do not run one after another, so it is fitted more than once; put those operations next to each other to save a tool change (operations always run in the order listed)
Wrote 3 programs; run them in this order:
Male board, 110.000 mm × 80.000 mm × 12.000 mm:
  1. dc-male-1-T2.nc  Before you start: fit T2 1/8 end mill and set work zero (X, Y and Z) with it.
  2. dc-male-2-T1.nc  Before you start: fit T1 1/4 60 degree V-bit and set Z zero with it. Keep X and Y zero.
  3. dc-male-3-T2.nc  Before you start: fit T2 1/8 end mill and set Z zero with it. Keep X and Y zero.
```

(The output goes on with the male's summary, as in step 8.) The female's
programs are left as they were, and the project file is not changed:
`--board` is for this export only. Because one operation describes both
boards, the new male is cut with the same depth, plug depth, margin and
bit as the pocket it must fit.

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
   light board, on a bandsaw with the light board flat on the table.
4. Plane or sand the board flat, down to the light board's surface. The
   letters appear as you reach it.

After planing, there should be no gap round the letters or the D's
counter (a 0.1 mm feeler gauge should not enter), the serifs should be
sharp, and the monogram should read "DC", the right way round.

## Preparing your own design

Any drawing of closed shapes works, as for a
[V-Carve](../user-guide/v-carve.md). Draw it as it should read in the
finished board, and prepare it in your drawing program (the menu names
are Inkscape's) so that DreamCarve sees the outlines you mean:

- **Closed paths only.** An open path is skipped, with the warning
  `vcarve_target_open`.
- **Text converted to paths**: select the text, then Path → Object to
  Path, then ungroup it, so that each letter is a path of its own.
  DreamCarve does not read fonts.
- **Strokes converted to outlines**: Path → Stroke to Path. DreamCarve
  ignores stroke width and cuts the centre line of a stroke, so a line
  drawn 3 mm wide is cut as a hairline unless you convert it.
- **No overlapping or touching shapes.** Where letters or shapes touch or
  overlap, select them and Path → Union, so that they are one outline.
  A shape inside another is a hole in it (the D's counter), not a second
  inlay.
- **Millimetres, and a size that fits.** Set the document's units to mm,
  and scale the design to fit the board with room round it for the
  male's band: `margin_mm` plus about 2 mm beyond the design on every
  side, plus clamping room. The frame trick here (a rectangle round the
  design that nothing targets) makes `init` size the stock for you.
- **Nothing too fine.** Keep strokes, and the gaps between shapes, at
  least about 2 mm wide. A finer stroke comes out as a shallow V on both
  sides that glues poorly, and a narrower gap leaves too little wood
  between two plugs.
- **Do not mirror the design yourself.** The male mirrors it; mirrored
  twice, the plug would not fit.

Then follow this chapter with your own file: `init` it, paste the
operation, and set each board's measured thickness, the stock's and
the male board's.

## Next steps

- Every setting, the cone-height rule for choosing the bit, and what each
  of the four values changes: [Inlay](../user-guide/inlay.md).
- A cutting board: Corbin's Workshop uses about 6 mm of inlay depth for
  a board that will be planed hard and used. That is past the 60° bit's
  5.5 mm cone, so use a 60° bit 12.7 mm (1/2 in) across (an 11 mm
  cone), and a male board thick enough to keep 6 mm under its band. A
  30° 6.35 mm bit (an 11.8 mm cone) reaches too, but fits less closely.
  See [Choosing the bit](../user-guide/inlay.md#choosing-the-bit) and
  [Choosing a V-bit for an inlay](../user-guide/inlay.md#choosing-a-v-bit-for-an-inlay).
