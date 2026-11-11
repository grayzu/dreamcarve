# Carving a photo

This chapter takes a picture to a G-code program that carves it with a
V-bit, as a **Photo V-Carve**: rows of parallel grooves, deep where the
picture is dark and shallow where it is light, so the picture shows in the
shadows of the grooves.

It uses a small test picture, `gradient.png`, so that every number below
is one you can check. Once it works, do the same with your own picture:
[Images](../user-guide/images.md) says how to choose and prepare one, and
[Photo V-Carve](../user-guide/photo-v-carve.md) what every setting does.

You use one bit: a 60° V-bit, 6.35 mm (1/4") across. Any V-bit works; the
chapter says what to change for yours.

Every command here is run by DreamCarve's test suite exactly as written,
and the excerpts are checked against the files it writes, so if it does
not work for you, that is a bug.

## What you need

- DreamCarve installed and on your `PATH`. See
  [Installing](installing.md).
- A text editor.
- The picture,
  [`gradient.png`](https://raw.githubusercontent.com/grayzu/dreamcarve/main/tests/fixtures/photo/gradient.png).
  It is also in the repository at `tests/fixtures/photo/gradient.png`.
- For the cut: a board of a light, even-grained wood such as maple,
  birch or poplar, or MDF, at least 120 × 70 mm and about 12 mm thick; the
  V-bit; and a router controlled by grblHAL.

## 1. Put the picture in a folder

Make an empty folder and put `gradient.png` in it:

```sh
mkdir photo
cd photo
```

Run every command below from that folder.

The picture is 64 × 32 pixels. It runs from black at the left edge to
white at the right, with two 8 × 8 blocks a third of the way down: a
white one in the dark half and a black one in the light half. Each block
shows whether the carve follows the picture: the white block is not
carved at all, the black one is carved to the full depth.

## 2. Create the project

<!-- doc-test: run -->
```sh
dreamcarve init gradient.png --width 100
```

```text
Imported a 64 × 32 px picture from gradient.png.
Wrote gradient.dcv: stock 120.00 × 70.00 mm, post-processor grblhal, picture b1 (64 × 32 px) on 100.000 × 50.000 mm at (10.000, 10.000), carved by one Photo V-Carve.
Values to set; export refuses until every one is set:
gradient.dcv:10:21: stock.thickness_mm is not set (mm)
gradient.dcv:35:22: tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`
gradient.dcv:36:24: tools[t1].plunge_mm_min is not set (mm/min) for tool `Set me`
gradient.dcv:37:14: tools[t1].rpm is not set (RPM) for tool `Set me`
gradient.dcv:50:23: operations[op1].max_depth_mm is not set (mm) for operation `Photo V-Carve`
Next: edit gradient.dcv and replace each `null` listed above; the tool is a 60° V-bit, so set the one you have. Then run `dreamcarve check gradient.dcv`.
```

`--width 100` makes the picture 100 mm wide on the stock; its height
follows its proportions, 50 mm. (`--height 50` would give the same.) The
picture is stored inside `gradient.dcv` as a **bitmap**, `b1`, so the
project file is all you need from now on. The stock is the picture plus
10 mm all round, with the picture's bottom-left corner at (10, 10).

`init` also wrote a 60° V-bit, `t1`, and one Photo V-Carve, `op1`, that
carves `b1` with it:

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

The grooves run at 22.5° to the X axis. A black pixel is cut
`max_depth_mm` deep, a white one not at all, and a gray one in between.
The grooves are spaced 1.1 times as far apart as a groove is wide at that
depth, so a thin ridge stays between even the darkest two. The other
settings are explained in [Photo V-Carve](../user-guide/photo-v-carve.md).

## 3. Set the three values

`init` leaves three things for you to set, because only you know them:
the board, the depth, and your bit's feeds. In `gradient.dcv`, change
these lines, and save:

<!-- doc-test: edit gradient.dcv -->
```json
"thickness_mm": 12.0,
"feed_mm_min": 1000.0,
"plunge_mm_min": 300.0,
"rpm": 18000,
"max_depth_mm": 1.5,
```

- **`thickness_mm`**: measure your board with calipers and use your own
  measurement. A photo is carved into the top only, so the thickness only
  has to be right enough that no cut reaches the bottom.
- **`feed_mm_min`, `plunge_mm_min`, `rpm`**: the values here are example
  values for a 60° 6.35 mm V-bit in hardwood, not defaults. Use the
  feeds your bit's maker gives, or the ones you already cut with. If your
  bit is not 60°, also change the tool's `"included_angle_deg": 60.0`.
- **`max_depth_mm`**: how deep black is carved. 1.5 mm suits any common
  1/4" V-bit from 15° to 90°; a 90° bit can go no deeper than 3.175 mm,
  where its sides reach its full width. Deeper gives more contrast and
  wider grooves; see [Picking a bit and a
  depth](../user-guide/photo-v-carve.md#picking-a-bit-and-a-depth).

## 4. Check it and look at it

<!-- doc-test: run -->
```sh
dreamcarve check gradient.dcv --preview gradient.svg
```

```text
Photo V-Carve op1 `Photo V-Carve`: picture `gradient` (b1), 100.000 mm by 50.000 mm at (10.000 mm, 10.000 mm), 45 grooves 1.732 mm wide at the full depth of 1.500 mm.
Extents (machine coordinates, mm, tool centre): X 10.000..110.000, Y 10.000..60.000, Z -1.500..5.000
  The cutter's edge reaches X 6.825..113.175, Y 6.825..63.175; keep clamps outside that.
Estimated time: 3 min 12 s (cutting 3 min 9 s, dwells 3 s); 9 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Wrote preview gradient.svg.
gradient.dcv: 0 error(s), 0 warning(s)
```

The first line is the one to read for a Photo V-Carve. It says where the
picture is on the stock and how big, that it is cut as 45 grooves, and
that a groove is 1.732 mm wide where it is cut the full 1.5 mm deep:
`2 × 1.5 × tan(30°)` for the 60° bit. The grooves are 1.1 times that,
1.905 mm, apart.

Open `gradient.svg` in a web browser. The picture is drawn on the stock,
and over it every groove: you can see them cross the picture at 22.5°.

## 5. Export

<!-- doc-test: run -->
```sh
dreamcarve export gradient.dcv
```

```text
Wrote gradient.nc (828 lines, post-processor grblhal).
  op1 `Photo V-Carve`: tool `t1` (Set me), picture `gradient` (b1), 1.500 mm deep at most
Extents (machine coordinates, mm, tool centre): X 10.000..110.000, Y 10.000..60.000, Z -1.500..5.000
  The cutter's edge reaches X 6.825..113.175, Y 6.825..63.175; keep clamps outside that.
Estimated time: 3 min 12 s (cutting 3 min 9 s, dwells 3 s); 9 mm of rapids not included: set [estimate] rapid_mm_min in config.toml. Excludes acceleration.
Before you start: fit T1 Set me and set work zero with it.
This program does not change the tool length offset (G43.1). If your sender sets one, set it before you set Z zero.
If you used G51 on this controller, soft-reset it and check that $G shows G50 before you start: grblHAL's G50 does not clear a scale that leaves X at 1, such as G51 Z-1.
Photo V-Carve op1 `Photo V-Carve`: picture `gradient` (b1), 100.000 mm by 50.000 mm at (10.000 mm, 10.000 mm), 45 grooves 1.732 mm wide at the full depth of 1.500 mm.
```

The program starts like this:

<!-- doc-test: excerpt gradient.nc -->
```text
( Project: gradient )
( Generated by DreamCarve on 2026-11-11 )
G90
G17
G94
G80
G40
G50
G21
G54
( Operation 1: Photo V-Carve )
( Tool 1: Set me )
G0 Z5.000
M3 S18000
G4 P3.000
X10.000 Y60.000
Z1.000
G1 Z-1.500 F300.000
Z1.000 F1000.000
X14.979
Z-1.437 F300.000
X10.924 Y58.320 Z-1.498 F1000.000
X10.693 Y58.225 Z-1.500
X10.000 Y57.938
Z1.000
```

The first groove passes through the picture's top-left corner, (10, 60),
and touches the picture only there: the bit plunges to 1.5 mm, since that
corner is black, and lifts. Then it rises to 1 mm above the board
(`stepover_retract_mm`) and moves over to the next groove at the feed
rate, not as a rapid, plunges, and cuts down to the left edge, getting
deeper as the picture gets darker. The grooves take turns, one down to
the left, the next up to the right. Every groove stays inside the
picture, and nothing is cut deeper than `max_depth_mm`; `export` checks
both before it writes the program (see [Safety](../user-guide/safety.md),
rule 16).

## 6. Cut

1. Clamp the board outside the extents `export` printed: the bit's edge
   reaches 6.825 to 113.175 mm in X and 6.825 to 63.175 mm in Y.
2. Fit the V-bit and set work zero with it: X and Y at the front-left
   corner of the stock, Z on the top of the board. Touch the tip off
   gently: a V-bit's tip set too low cuts every groove too deep and too
   wide.
3. Do an air cut first, with Z zero 20 mm above the board.
4. Run `gradient.nc`.
5. Brush the dust out of the grooves, and sand lightly with a block, along
   the grooves, to take off the fuzz.

The left end should be the darkest, with the grooves nearly touching; the
right end should be uncut; the white block should be an uncut patch in the
dark half, and the black block a patch of full-depth grooves in the light
half.

## Next steps

- **Your own picture.** Crop it, make it grayscale and raise its
  contrast first; see [Images](../user-guide/images.md#choosing-a-picture).
  Then `dreamcarve init your-picture.jpg --width 100` and the same three
  values.
- **A dark wood.** On walnut the cut shows lighter than the surface, so
  a dark picture would come out as its negative. Set `"invert": true`:
  white is then carved deepest and black not at all.
- **More detail, or less time.** Every setting trades detail against
  time: see [Photo V-Carve](../user-guide/photo-v-carve.md#detail-and-time).
