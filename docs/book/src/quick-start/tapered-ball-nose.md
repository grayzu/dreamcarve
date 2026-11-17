<!--
Copyright 2026 The DreamCarve Contributors
SPDX-License-Identifier: Apache-2.0
-->
# Carving and inlaying with a tapered ball nose

A **tapered ball nose** is a narrow cone with a small ball at its tip.
DreamCarve carves with one wherever it carves with a V-bit, in a
[V-Carve](../user-guide/v-carve.md) or an [Inlay](../user-guide/inlay.md):
its steep sides make deep, narrow cuts, and its round tip is stronger
than a V-bit's point and clears chips better.

This chapter does three jobs, in the order you should do them with a new
bit:

1. a **coupon pair**: a 20 mm rounded square, inlaid with the same depths
   as the real inlay, to find the glue gap your bit needs;
2. a **carve**: the monogram "DC" V-carved with a 2 mm flat floor;
3. an **inlay pair**: the same "DC", inlaid 6 mm deep.

It uses three bits: two tapered ball noses 6.35 mm (1/4 in) across with
6.7° per side and 19.05 mm of flute, one with a 2.286 mm tip and one with
a 0.762 mm tip, and a 3.175 mm (1/8 in) end mill. Those numbers are an
example, a common pair of 1/4 in bits. Tapered ball noses differ a lot,
and the bit you hold may not match its catalogue, so enter your own bit's
numbers: [Tapered ball nose](../user-guide/tapered-ball-nose.md) says
what each one is and how to measure it.

If you have not done [An inlay pair](inlay.md), do it first: this chapter
assumes you know how an Inlay cuts its two boards, fits and is glued.

Every command here is run by DreamCarve's test suite exactly as written,
so if it does not work for you, that is a bug.

## What you need

- DreamCarve installed and on your `PATH`. See [Installing](installing.md).
- A text editor.
- The two drawings,
  [`tbn-coupon.svg`](https://raw.githubusercontent.com/grayzu/dreamcarve/main/tests/fixtures/tbn-coupon.svg)
  and
  [`dc-inlay.svg`](https://raw.githubusercontent.com/grayzu/dreamcarve/main/tests/fixtures/dc-inlay.svg).
  They are also in the repository, in `tests/fixtures/`.
- Calipers, to measure the boards and the bits.
- For the cuts: the three bits; a router controlled by grblHAL and a
  spoilboard; for the coupon, two offcuts at least 60 × 60 mm; for the
  carve, a board at least 110 × 80 mm; for the inlay, a light board sold
  as 12 mm and a dark one sold as 19 mm, each at least 110 × 80 mm. The
  end mill needs at least 21.2 mm of flute for the cut-out through the
  19 mm board (see [step 5](#5-paste-the-coupons-operation)).

## 1. Put the drawings in a folder

Make an empty folder and put both drawings in it:

```sh
mkdir tapered
cd tapered
```

Run every command below from that folder.

`tbn-coupon.svg` has two shapes: the coupon, a 20 × 20 mm square with
3 mm corners, and a 40 × 40 mm rectangle round it, the frame, which
nothing cuts. As in [An inlay pair](inlay.md), the frame is there so that
`init` makes a stock with room round the design: 60 × 60 mm, the coupon
at its centre. `dc-inlay.svg` is the inlay pair's drawing, the letters D
and C, 40 mm tall, in a 90 × 60 mm frame.

## 2. Enter your bits

Save this as `my-tools.json` in the folder:

<!-- doc-test: write my-tools.json -->
```json
{
  "version": 3,
  "tools": [
    {
      "id": "lt1",
      "source": null,
      "tool": {
        "id": "lt1",
        "name": "2.286 mm tip tapered ball nose",
        "vendor": null,
        "product_id": null,
        "number": 1,
        "cutter": { "tapered_ball_nose": { "tip_diameter_mm": 2.286, "taper_angle_deg": 6.7, "flute_length_mm": 19.05 } },
        "diameter_mm": 6.35,
        "flutes": 2,
        "feed_mm_min": 1524.0,
        "plunge_mm_min": 635.0,
        "rpm": 19000,
        "step_down_mm": null,
        "step_over_fraction": 0.1
      }
    },
    {
      "id": "lt2",
      "source": null,
      "tool": {
        "id": "lt2",
        "name": "0.762 mm tip tapered ball nose",
        "vendor": null,
        "product_id": null,
        "number": 2,
        "cutter": { "tapered_ball_nose": { "tip_diameter_mm": 0.762, "taper_angle_deg": 6.7, "flute_length_mm": 19.05 } },
        "diameter_mm": 6.35,
        "flutes": 2,
        "feed_mm_min": 1524.0,
        "plunge_mm_min": 635.0,
        "rpm": 19000,
        "step_down_mm": null,
        "step_over_fraction": 0.1
      }
    },
    {
      "id": "lt3",
      "source": null,
      "tool": {
        "id": "lt3",
        "name": "1/8 end mill",
        "vendor": null,
        "product_id": null,
        "number": 3,
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

A tapered ball nose is described by four numbers. Enter your own bit's,
from its maker's table or measured:

| Field | This example | What it is |
| --- | --- | --- |
| `tip_diameter_mm` | 2.286 and 0.762 | The ball's **diameter**: twice the tip radius. A bit sold as "0.015 in radius" has a 0.762 mm tip diameter. |
| `taper_angle_deg` | 6.7 | The taper **per side**, from the bit's axis to its flank: half the included angle. A V-bit's angle is the included angle, so do not double this one, or halve a V-bit's. |
| `flute_length_mm` | 19.05 | How far up from the tip the bit cuts. Above it the shank would rub. |
| `diameter_mm` | 6.35 | The **cutting diameter**, the widest the bit cuts (the catalogue's "cut diameter"): where the taper reaches it, the bit is a plain cylinder. |

The feeds are this example's too; use what your bit's maker gives for
your wood. DreamCarve never invents one. A tapered ball nose carves in
one pass, so its `step_down_mm` is left `null`.

Add the file's tools to your library and list it:

<!-- doc-test: run -->
```sh
dreamcarve tools import my-tools.json
dreamcarve tools list
```

```text
`2.286 mm tip tapered ball nose`: imported (lt1)
`0.762 mm tip tapered ball nose`: imported (lt2)
`1/8 end mill`: imported (lt3)
3 imported, 0 already present, 0 replaced; library: /Users/you/Library/Application Support/dreamcarve/tools.json
ID   NAME                            VENDOR  PRODUCT  CUTTER             DIAMETER_MM  FEED_MM_MIN  PLUNGE_MM_MIN  RPM    NUMBER  SOURCE
lt1  2.286 mm tip tapered ball nose  -       -        tapered ball nose  6.35         1524         635            19000  1       -
lt2  0.762 mm tip tapered ball nose  -       -        tapered ball nose  6.35         1524         635            19000  2       -
lt3  1/8 end mill                    -       -        end mill           3.175        1000         300            18000  3       -
lt1 2.286 mm tip tapered ball nose: tapered ball nose, 6.350 mm cut, tip 2.286 mm (radius 1.143 mm), 6.7° per side, 19.050 mm flutes
lt2 0.762 mm tip tapered ball nose: tapered ball nose, 6.350 mm cut, tip 0.762 mm (radius 0.381 mm), 6.7° per side, 19.050 mm flutes
```

`tools list` adds a line for each tapered ball nose, with its four
numbers: check them against your bit. If your library already has tools,
the ids differ: use the ids it shows. A Fusion 360 library with your bits
in it imports them too ([Tool library: tapered
mills](../user-guide/tool-library.md#tapered-mills)); it may leave a value
unset, which `check` then lists until you set it.

## Part 1: the coupon

Your bit's real tip and taper are rarely exactly its catalogue's, and a
tapered ball nose's steep sides turn a small difference into a large
one in height: a tip 0.05 mm off moves where the plug seats by about
0.76 mm. The coupon pair finds the glue gap that seats with your bit,
before you cut the real inlay.

## 3. Create the coupon project

<!-- doc-test: run -->
```sh
dreamcarve init tbn-coupon.svg
dreamcarve tools add tbn-coupon.dcv lt2
dreamcarve tools add tbn-coupon.dcv lt3
```

```text
Imported 2 shape(s) from tbn-coupon.svg: page units mm, 1 user unit = 1.0000 mm.
Wrote tbn-coupon.dcv: stock 60.00 × 60.00 mm, post-processor grblhal, 2 shape(s) targeted by Profile.
Values to set; export refuses until every one is set:
tbn-coupon.dcv:10:21: stock.thickness_mm is not set (mm)
tbn-coupon.dcv:31:22: tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`
tbn-coupon.dcv:32:24: tools[t1].plunge_mm_min is not set (mm/min) for tool `Set me`
tbn-coupon.dcv:33:14: tools[t1].rpm is not set (RPM) for tool `Set me`
tbn-coupon.dcv:34:23: tools[t1].step_down_mm is not set (mm) for tool `Set me`
tbn-coupon.dcv:50:19: operations[op1].depth_mm is not set (mm) for operation `Profile`
Next: edit tbn-coupon.dcv and replace each `null` listed above. To cut through, `depth_mm` is the thickness plus `through_cut_allowance_mm`. Then run `dreamcarve check tbn-coupon.dcv`.
Added `0.762 mm tip tapered ball nose` (lt2) to tbn-coupon.dcv as tool t2.
Added `1/8 end mill` (lt3) to tbn-coupon.dcv as tool t3.
```

The coupon is cut with the bit you will inlay with, the 0.762 mm tip,
`t2` in this project, and the end mill, `t3`. The shapes are `i1`, the
coupon, and `i2`, the frame.

## 4. Set the female board

The coupon's female is cut in the stock. Measure the offcut and set its
thickness: change the stock's `thickness_mm` line to:

<!-- doc-test: edit tbn-coupon.dcv -->
```json
"thickness_mm": 12.0,
```

## 5. Paste the coupon's operation

Replace the whole `"operations"` block, from `"operations": [` down to
its closing `],`, with this, and save:

<!-- doc-test: paste tbn-coupon.dcv operations -->
```json
"operations": [
  {
    "kind": "inlay",
    "id": "op1",
    "name": "Coupon",
    "enabled": true,
    "targets": [ "i1" ],
    "tool": "t2",
    "male_board": { "width_mm": null, "height_mm": null, "thickness_mm": 19.0 },
    "depth_mm": 6.0,
    "glue_gap_mm": 0.85,
    "plug_depth_mm": 2.0,
    "margin_mm": 8.0,
    "outline_rounding_mm": null,
    "boundary": [],
    "clearing": [
      { "tool": "t3", "strategy": "offset", "direction": "climb", "entry": { "ramp": { "angle_deg": 15.0 } } }
    ],
    "step_over_fraction": 0.1,
    "machining_order": "full_depth_first",
    "through_cut_allowance_mm": 0.0,
    "safe_z_mm": 10.0,
    "cut_out": { "tool": "t3", "tabs": { "placement": { "kind": "count", "count": 4 }, "width_mm": 6.0, "height_mm": 3.0, "shape": "rect", "on_holes": false, "avoid_corners": true }, "through_cut_allowance_mm": 0.2 }
  }
],
```

It is the "DC" pair's Inlay, on the coupon, with the tapered ball nose:
the same `depth_mm`, `glue_gap_mm` and `plug_depth_mm` as the real inlay
in [Part 3](#part-3-the-inlay-pair), so the fit it finds is the one the
real inlay needs. The male board, 19 mm thick, is set here too.

- `depth_mm` **6**. A tapered ball nose's steep sides make a deep pocket
  narrow at the surface, so it can go deeper than a V-bit inlay of the
  same design: 6 mm survives a hard planing, as a cutting board needs.
- `glue_gap_mm` **0.85**, the place to start: the bit's **tangent
  height** plus 0.5 mm. The tangent height is where the ball meets the
  taper, `tip radius × (1 − sin(taper))`, 0.337 mm for a 0.762 mm tip at
  6.7°. The pocket's floor is rounded by the ball up to that height, and
  the plug's face must stay above it. For your bit, work it out, or set
  0.25 and let `check` tell you (`inlay_glue_gap_below_ball` names it).
- `plug_depth_mm` **2**, above the tangent height too, so the plug's
  wall at the surface is cut by the taper, not the ball.

The male cuts 6 − 0.85 + 2 = 7.15 mm into the 19 mm board, so the
end mill's cut-out goes 19.2 mm deep: give it at least 19.2 + 2 =
21.2 mm of flute, or set `"cut_out": null` and saw the plug free.

## 6. Check the coupon

<!-- doc-test: run -->
```sh
dreamcarve check tbn-coupon.dcv
```

```text
tbn-coupon.dcv:22:5: warning: unused_tool: tool `t1` (`Set me`) is defined but not used by any enabled operation
warning: inlay_male_mirrored: operation `op1` (male board): it is a male inlay, so its design is cut mirrored about X = 30 mm, the middle of its targets, to seat face down in the female
warning: tool_change_avoidable: tool `t3` (`1/8 end mill`) is used by operations `op1`, which do not run one after another, so it is fitted more than once; put those operations next to each other to save a tool change (operations always run in the order listed)
t2 0.762 mm tip tapered ball nose: tapered ball nose, 6.350 mm cut, tip 0.762 mm (radius 0.381 mm), 6.7° per side, 19.050 mm flutes
Inlay op1 `Coupon` (female board): deepest cut 6.000 mm below the stock top, a flat floor at depth_mm 6.000 mm; V-bit floor ridges 0.015 mm where no clearing pass reaches.
Inlay op1 `Coupon` (male board): deepest cut 7.150 mm below the male board's top, the plug's start plane 5.150 mm below the male board's top and the surround's flat floor plug_depth_mm 2.000 mm below that; V-bit floor ridges 0.004 mm where no clearing pass reaches; clearing pass `t3` cuts the band's outer edge upright, its inward curves rounded to 1.587 mm; the cut-out frees the plug along the floor's outer edge, its centre 1.587 mm inside the band's edge and at least 6.412 mm out from the design, through the board to 0.200 mm below it.
op1 `Coupon` seat: moves 0.55 mm per 0.5° of taper, 0.76 mm per 0.05 mm of tip radius; test-fit before gluing
Female board, the stock, 60.000 mm × 60.000 mm × 12.000 mm:
Program tbn-coupon-female-1-T3.nc (T3):
  Extents (machine coordinates, mm, tool centre): X 22.631..37.369, Y 22.631..37.369, Z -6.000..10.000
    The cutter's edge reaches X 21.044..38.956, Y 21.044..38.956; keep clamps outside that.
  Estimated time: 44 s (cutting 41 s, dwells 3 s); 97 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program tbn-coupon-female-2-T2.nc (T2):
  Extents (machine coordinates, mm, tool centre): X 20.039..39.961, Y 20.039..39.961, Z -6.000..10.000
    The cutter's edge reaches X 16.864..43.136, Y 16.864..43.136; keep clamps outside that.
  Estimated time: 26 s (cutting 23 s, dwells 3 s); 426 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Male board, 60.000 mm × 60.000 mm × 19.000 mm:
Program tbn-coupon-male-1-T3.nc (T3):
  Extents (machine coordinates, mm, tool centre): X 13.587..46.413, Y 13.587..46.413, Z -7.150..10.000
    The cutter's edge reaches X 11.999..48.001, Y 11.999..48.001; keep clamps outside that.
  Estimated time: 1 min 48 s (cutting 1 min 45 s, dwells 3 s); 109 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program tbn-coupon-male-2-T2.nc (T2):
  Extents (machine coordinates, mm, tool centre): X 19.426..40.574, Y 19.426..40.574, Z -7.150..10.000
    The cutter's edge reaches X 16.251..43.749, Y 16.251..43.749; keep clamps outside that.
  Estimated time: 7 s (cutting 4 s, dwells 3 s); 26 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program tbn-coupon-male-3-T3.nc (T3):
  Extents (machine coordinates, mm, tool centre): X 13.587..46.413, Y 13.587..46.413, Z -19.200..10.000
    The cutter's edge reaches X 11.999..48.001, Y 11.999..48.001; keep clamps outside that.
  Estimated time: 1 min 7 s (cutting 1 min 4 s, dwells 3 s); 38 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Estimated total time: 4 min 12 s (cutting 3 min 57 s, dwells 15 s); 697 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
tbn-coupon.dcv: 0 error(s), 3 warning(s)
```

No errors, and the warnings are the ones [An inlay
pair](inlay.md#7-check-the-project) explains. Two lines are new:

- after the warnings, the bit's own line, as `tools list` prints it, so
  you can check the numbers the programs are made with;
- the **seat line**, which matters most with a tapered ball nose:

```text
op1 `Coupon` seat: moves 0.55 mm per 0.5° of taper, 0.76 mm per 0.05 mm of tip radius; test-fit before gluing
```

The male's line also shows how deep the bit is engaged: 7.15 mm, well
inside its 19.05 mm of flute.

It says how far the plug's seat moves for a small error in the bit's
numbers: half a degree of taper moves it 0.55 mm at these depths, and a
tip radius 0.05 mm off, 0.76 mm. Measuring a bit to that precision is
hard; cutting a coupon is not.

## 7. Cut the coupon pair and fit it

<!-- doc-test: run -->
```sh
dreamcarve export tbn-coupon.dcv
```

It writes five programs, two for the female and three for the male, in
the order [An inlay pair](inlay.md#8-export-the-programs) describes: on
each board the end mill's clearing pass first, then the tapered ball
nose, then on the male the end mill's cut-out. Cut both boards as that
chapter does, cut the plug free, and press it into the pocket dry.

- **It seats flush**, its walls meeting the pocket's with no gap at the
  surface: this glue gap fits your bit. Keep it.
- **A gap at the surface, all round**: the plug's face reached the
  pocket's floor first. Raise `glue_gap_mm` by 0.25 mm.
- **The plug will not go in**, or stops well short: lower `glue_gap_mm`
  by 0.25 mm, but not below the tangent height.

Change `glue_gap_mm` in the operation, export the male alone, and cut a
new male coupon:

<!-- doc-test: run -->
```sh
dreamcarve export tbn-coupon.dcv --board male
```

Repeat until it seats flush; a bit close to its numbers takes one or
two tries. Write the glue gap down: it holds for this bit at these
depths. This chapter goes on with 0.85 mm; use yours.

## Part 2: the carve

## 8. Create the "DC" project

One project holds the carve and the inlay pair. Make it from the
lettering, and add all three bits:

<!-- doc-test: run -->
```sh
dreamcarve init dc-inlay.svg -o dc-tbn.dcv
dreamcarve tools add dc-tbn.dcv lt1
dreamcarve tools add dc-tbn.dcv lt2
dreamcarve tools add dc-tbn.dcv lt3
```

```text
Imported 3 shape(s) from dc-inlay.svg: page units mm, 1 user unit = 1.0000 mm.
Wrote dc-tbn.dcv: stock 110.00 × 80.00 mm, post-processor grblhal, 3 shape(s) targeted by Profile.
Values to set; export refuses until every one is set:
dc-tbn.dcv:10:21: stock.thickness_mm is not set (mm)
dc-tbn.dcv:31:22: tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`
dc-tbn.dcv:32:24: tools[t1].plunge_mm_min is not set (mm/min) for tool `Set me`
dc-tbn.dcv:33:14: tools[t1].rpm is not set (RPM) for tool `Set me`
dc-tbn.dcv:34:23: tools[t1].step_down_mm is not set (mm) for tool `Set me`
dc-tbn.dcv:51:19: operations[op1].depth_mm is not set (mm) for operation `Profile`
Next: edit dc-tbn.dcv and replace each `null` listed above. To cut through, `depth_mm` is the thickness plus `through_cut_allowance_mm`. Then run `dreamcarve check dc-tbn.dcv`.
Added `2.286 mm tip tapered ball nose` (lt1) to dc-tbn.dcv as tool t2.
Added `0.762 mm tip tapered ball nose` (lt2) to dc-tbn.dcv as tool t3.
Added `1/8 end mill` (lt3) to dc-tbn.dcv as tool t4.
```

The 2.286 mm tip is `t2`, the 0.762 mm tip `t3` and the end mill `t4`.
The letters are `i1` and `i2`; the frame, `i3`, is not cut. Set the
stock's thickness, the board you carve:

<!-- doc-test: edit dc-tbn.dcv -->
```json
"thickness_mm": 12.0,
```

## 9. Paste the carve

Replace the `"operations"` block with this:

<!-- doc-test: paste dc-tbn.dcv operations -->
```json
"operations": [
  {
    "kind": "v_carve",
    "id": "op1",
    "name": "DC carve",
    "enabled": true,
    "targets": [ "i1", "i2" ],
    "tool": "t2",
    "strategy": "medial",
    "clearing": [],
    "flat_depth_mm": 2.0,
    "start_depth_mm": 0.0,
    "start_plane_cleared_externally": false,
    "step_over_fraction": 0.1,
    "machining_order": "full_depth_first",
    "through_cut_allowance_mm": 0.0,
    "safe_z_mm": 10.0
  }
],
```

A V-Carve with the 2.286 mm tip, 2 mm deep at most: the narrow parts of
the letters are carved as a V-bit carves them, each point as deep as the
bit's width there needs, and the wide stems get a flat floor 2 mm down.
With no clearing pass the bit clears that floor itself, in rings a tenth
of its width apart.

<!-- doc-test: run -->
```sh
dreamcarve check dc-tbn.dcv
```

```text
dc-tbn.dcv:22:5: warning: unused_tool: tool `t1` (`Set me`) is defined but not used by any enabled operation
dc-tbn.dcv:58:5: warning: unused_tool: tool `t3` (`0.762 mm tip tapered ball nose`) is defined but not used by any enabled operation
dc-tbn.dcv:79:5: warning: unused_tool: tool `t4` (`1/8 end mill`) is defined but not used by any enabled operation
warning: tapered_corners_softened: operation `op1`: its tapered ball nose cannot follow 6 corners sharper than its tip below the surface, so their walls meet in a round valley instead of a crease, up to 0.877 mm from the corner as drawn at (90.633, 29.194); round the corners by at least 1.135 mm to cut them as the taper would
t2 2.286 mm tip tapered ball nose: tapered ball nose, 6.350 mm cut, tip 2.286 mm (radius 1.143 mm), 6.7° per side, 19.050 mm flutes
t3 0.762 mm tip tapered ball nose: tapered ball nose, 6.350 mm cut, tip 0.762 mm (radius 0.381 mm), 6.7° per side, 19.050 mm flutes
V-Carve op1 `DC carve`: deepest cut 2.000 mm below the stock top, clamped at a flat depth of 2.000 mm; V-bit floor ridges 0.007 mm; a clearing pass or a smaller step_over_fraction flattens them.
Extents (machine coordinates, mm, tool centre): X 18.320..91.585, Y 20.001..59.999, Z -2.000..10.000
  The cutter's edge reaches X 15.145..94.760, Y 16.826..63.174; keep clamps outside that.
Estimated time: 2 min 49 s (cutting 2 min 46 s, dwells 3 s); 482 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
dc-tbn.dcv: 0 error(s), 4 warning(s)
```

The `unused_tool` warnings are the inlay's bits, which Part 3 uses. Two
lines are new:

- `tapered_corners_softened`: at the surface the bit runs into every
  corner, so the outline is cut as drawn, but below it the ball cannot
  follow a corner sharper than itself: the two walls of a serif meet in
  a round valley, the shape of the tip, instead of a crease. That is the
  soft look a tapered ball nose gives a carve. It exports anyway; to cut
  a corner exactly as the taper would, round it in the drawing by at
  least the radius the warning gives.
- the floor's ridges, 0.007 mm: between two rings the round tip leaves
  only a shallow scallop, where a V-bit's point at the same step-over
  would leave 0.2 mm. The floor meets each wall in a fillet as round as
  the tip.

## 10. Export the carve

<!-- doc-test: run -->
```sh
dreamcarve export dc-tbn.dcv
```

It writes one program, `dc-tbn.nc`. Fit the 2.286 mm tip, set work zero
on the board's front-left corner and its top, and run it.

## Part 3: the inlay pair

## 11. Paste the inlay

The carve and the inlay share the lettering, so the inlay pair goes in
the same project, and the carve is switched off, so that the stock is
the inlay's female board. Replace the `"operations"` block again:

<!-- doc-test: paste dc-tbn.dcv operations -->
```json
"operations": [
  {
    "kind": "v_carve",
    "id": "op1",
    "name": "DC carve",
    "enabled": false,
    "targets": [ "i1", "i2" ],
    "tool": "t2",
    "strategy": "medial",
    "clearing": [],
    "flat_depth_mm": 2.0,
    "start_depth_mm": 0.0,
    "start_plane_cleared_externally": false,
    "step_over_fraction": 0.1,
    "machining_order": "full_depth_first",
    "through_cut_allowance_mm": 0.0,
    "safe_z_mm": 10.0
  },
  {
    "kind": "inlay",
    "id": "op2",
    "name": "DC inlay",
    "enabled": true,
    "targets": [ "i1", "i2" ],
    "tool": "t3",
    "male_board": { "width_mm": null, "height_mm": null, "thickness_mm": 19.0 },
    "depth_mm": 6.0,
    "glue_gap_mm": 0.85,
    "plug_depth_mm": 2.0,
    "margin_mm": 8.0,
    "outline_rounding_mm": 10.0,
    "boundary": [],
    "clearing": [
      { "tool": "t4", "strategy": "offset", "direction": "climb", "entry": { "ramp": { "angle_deg": 15.0 } } }
    ],
    "step_over_fraction": 0.1,
    "machining_order": "full_depth_first",
    "through_cut_allowance_mm": 0.0,
    "safe_z_mm": 10.0,
    "cut_out": { "tool": "t4", "tabs": { "placement": { "kind": "count", "count": 4 }, "width_mm": 6.0, "height_mm": 3.0, "shape": "rect", "on_holes": false, "avoid_corners": true }, "through_cut_allowance_mm": 0.2 }
  }
],
```

The Inlay is the coupon's with the letters as targets, the band rounded
by 10 mm as in [An inlay pair](inlay.md#5-paste-the-operation), and the
0.762 mm tip, `t3`. Put the glue gap your coupon found in
`glue_gap_mm`. The stock's 12 mm is the light board; the male board is
the dark one, 19 mm, the stock's size.

<!-- doc-test: run -->
```sh
dreamcarve check dc-tbn.dcv
```

```text
dc-tbn.dcv:22:5: warning: unused_tool: tool `t1` (`Set me`) is defined but not used by any enabled operation
dc-tbn.dcv:37:5: warning: unused_tool: tool `t2` (`2.286 mm tip tapered ball nose`) is defined but not used by any enabled operation
dc-tbn.dcv:100:18: warning: operation_disabled: operation `op1` is disabled and will not be exported
warning: inlay_rounded_for_bit: operation `op2`: its tapered ball nose cannot cut a point or a part narrower than its tip and taper allow, so both boards round the design's points to a radius of 0.983 mm; the outline moves up to 0.689 mm, at (90.633, 29.194). Round the drawing yourself, carve less deep, or use a finer tip to keep it
warning: inlay_rounded_for_bit: operation `op2`: its tapered ball nose cannot cut an inward corner or a gap tighter than its tip, so both boards round the design's notches to a radius of 0.378 mm; the outline moves up to 0.155 mm, at (32.391, 56.573). Round the drawing yourself, or use a finer tip to keep it
warning: inlay_male_mirrored: operation `op2` (male board): it is a male inlay, so its design is cut mirrored about X = 55 mm, the middle of its targets, to seat face down in the female
warning: tool_change_avoidable: tool `t4` (`1/8 end mill`) is used by operations `op2`, which do not run one after another, so it is fitted more than once; put those operations next to each other to save a tool change (operations always run in the order listed)
t2 2.286 mm tip tapered ball nose: tapered ball nose, 6.350 mm cut, tip 2.286 mm (radius 1.143 mm), 6.7° per side, 19.050 mm flutes
t3 0.762 mm tip tapered ball nose: tapered ball nose, 6.350 mm cut, tip 0.762 mm (radius 0.381 mm), 6.7° per side, 19.050 mm flutes
Inlay op2 `DC inlay` (female board): deepest cut 6.000 mm below the stock top, a flat floor at depth_mm 6.000 mm; V-bit floor ridges 0.015 mm where no clearing pass reaches.
Inlay op2 `DC inlay` (male board): deepest cut 7.150 mm below the male board's top, the plug's start plane 5.150 mm below the male board's top and the surround's flat floor plug_depth_mm 2.000 mm below that; V-bit floor ridges 0.004 mm where no clearing pass reaches; clearing pass `t4` cuts the band's outer edge upright, its inward curves rounded to 10.000 mm; the cut-out frees the plug along the floor's outer edge, its centre 1.587 mm inside the band's edge and at least 6.412 mm out from the design, through the board to 0.200 mm below it.
op2 `DC inlay` seat: moves 0.55 mm per 0.5° of taper, 0.76 mm per 0.05 mm of tip radius; test-fit before gluing
Female board, the stock, 110.000 mm × 80.000 mm × 12.000 mm:
Program dc-tbn-female-1-T3.nc (T3):
  Extents (machine coordinates, mm, tool centre): X 25.277..89.053, Y 22.631..57.389, Z -6.000..10.000
    The cutter's edge reaches X 23.690..90.640, Y 21.044..58.977; keep clamps outside that.
  Estimated time: 1 min 31 s (cutting 1 min 28 s, dwells 3 s); 617 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program dc-tbn-female-2-T2.nc (T2):
  Extents (machine coordinates, mm, tool centre): X 18.320..91.455, Y 20.001..59.999, Z -6.000..10.000
    The cutter's edge reaches X 15.145..94.630, Y 16.826..63.174; keep clamps outside that.
  Estimated time: 1 min 22 s (cutting 1 min 19 s, dwells 3 s); 2.0 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Male board, 110.000 mm × 80.000 mm × 19.000 mm:
Program dc-tbn-male-1-T3.nc (T3):
  Extents (machine coordinates, mm, tool centre): X 11.915..98.094, Y 13.019..66.974, Z -7.150..10.000
    The cutter's edge reaches X 10.328..99.682, Y 11.432..68.562; keep clamps outside that.
  Estimated time: 7 min 46 s (cutting 7 min 43 s, dwells 3 s); 1.5 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program dc-tbn-male-2-T2.nc (T2):
  Extents (machine coordinates, mm, tool centre): X 17.746..92.255, Y 18.861..61.139, Z -7.150..10.000
    The cutter's edge reaches X 14.571..95.430, Y 15.686..64.314; keep clamps outside that.
  Estimated time: 45 s (cutting 42 s, dwells 3 s); 705 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Program dc-tbn-male-3-T3.nc (T3):
  Extents (machine coordinates, mm, tool centre): X 11.910..98.099, Y 13.019..66.982, Z -19.200..10.000
    The cutter's edge reaches X 10.322..99.686, Y 11.432..68.570; keep clamps outside that.
  Estimated time: 2 min 12 s (cutting 2 min 9 s, dwells 3 s); 38 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Estimated total time: 13 min 36 s (cutting 13 min 21 s, dwells 15 s); 4.8 m of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
dc-tbn.dcv: 0 error(s), 7 warning(s)
```

`operation_disabled` and `unused_tool` say the carve and its bit are
switched off, as meant. The new lines:

- `inlay_rounded_for_bit`, twice. A tapered ball nose cannot cut a sharp
  point of the design to the plug's full height, nor a sharp inward
  corner below the surface, so the Inlay rounds the design for the bit,
  the same on both boards: its points to 0.983 mm here, its inward
  corners to 0.378 mm. The serifs' tips come out round by up to 0.69 mm.
  Without the rounding the pocket would keep wood in each point that the
  plug's sharper point would hit, and the plug would stand proud. To keep
  sharper points, use a finer tip or a smaller `depth_mm − glue_gap_mm`.
- the seat line, as for the coupon: the same depths, so the same
  figures.

If a stroke of your own design is too narrow for the bit even rounded,
`check` refuses it with `inlay_too_narrow_for_bit`, naming where and how
wide it must be.

## 12. Export and cut the pair

<!-- doc-test: run -->
```sh
dreamcarve export dc-tbn.dcv
```

```text
Wrote 5 programs; run them in this order:
Female board, the stock, 110.000 mm × 80.000 mm × 12.000 mm:
  1. dc-tbn-female-1-T3.nc  Before you start: fit T3 1/8 end mill and set work zero (X, Y and Z) with it.
  2. dc-tbn-female-2-T2.nc  Before you start: fit T2 0.762 mm tip tapered ball nose and set Z zero with it. Keep X and Y zero.
Male board, 110.000 mm × 80.000 mm × 19.000 mm:
  1. dc-tbn-male-1-T3.nc  Before you start: fit T3 1/8 end mill and set work zero (X, Y and Z) with it.
  2. dc-tbn-male-2-T2.nc  Before you start: fit T2 0.762 mm tip tapered ball nose and set Z zero with it. Keep X and Y zero.
  3. dc-tbn-male-3-T3.nc  Before you start: fit T3 1/8 end mill and set Z zero with it. Keep X and Y zero.
```

(The warnings come first and the boards' summaries after, as for
`check`.)

Cut the female on the light board and the male on the dark one, cut the
plug free and test it dry, as in [An inlay pair](inlay.md#11-cut-the-female)
from step 11 on. With the coupon's glue gap it should seat flush at the
first try. Then glue, clamp, saw off the backing and plane, as that
chapter's last step says. The backing stands 2 mm above the light board
(`plug_depth_mm`), and 11.85 mm of the dark board is left under the band.

## Using your own bit

- Enter your bit's four numbers, measured if you can: [Tapered ball
  nose: measuring the taper](../user-guide/tapered-ball-nose.md#measuring-the-taper)
  shows how with two plunge holes.
- Cut a coupon at your inlay's depths first, starting `glue_gap_mm` at
  your bit's tangent height plus 0.5 mm, and adjust it in 0.25 mm steps.
- Keep `depth_mm − glue_gap_mm + plug_depth_mm` under your bit's flute
  length, or the Inlay is refused with `v_bit_cone_exceeded`.

## Next steps

- What each number of the bit does, the depth law, soft corners, and the
  rules that refuse a tapered ball nose: [Tapered ball
  nose](../user-guide/tapered-ball-nose.md).
- Every Inlay setting: [Inlay](../user-guide/inlay.md).
