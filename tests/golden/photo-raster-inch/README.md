<!--
Copyright 2026 The DreamCarve Contributors
SPDX-License-Identifier: Apache-2.0
-->
# Golden case: photo-raster-inch

Captured from RasterCarve 1.0.8, commit `b3b844e7dbc6e560e361dbff7937b6e5ed81746b`, with
`tools/parity/photo/capture.py --case photo-raster-inch`
(the checkout's `rastercarve/` package, the command-line program, clean at
that commit), run by Python 3.14.3, OpenCV 5.0.0, NumPy 2.5.4.

- Picture: `tests/fixtures/photo/ramp.png` (synthetic; see
  `tests/fixtures/README.md`).
- Command line, run in an empty directory holding a copy of the picture:

  ```sh
  python -m rastercarve --width 0.1 ramp.png
  ```

- `program.nc` is RasterCarve's standard output, byte for byte. It is the
  output of a GPLv2+ program on DreamCarve's own fixture picture: the GPL
  does not cover a program's output, and the file carries no RasterCarve
  source text (security review M5). Nothing else of RasterCarve's is in
  this repository.
- DreamCarve project: `dreamcarve.dcv`, written by hand (the capture never
  writes it).

## RasterCarve's statistics

```text
=== Statistics ===
Input resolution: 32x64 px
Output dimensions: 0.10" wide by 0.05" tall = 0.0 in^2
Max line depth: 0.080 in
Max line width: 0.043 in (30.0 deg V-bit)
Line spacing: 0.047 in (110% stepover)
Line angle: 22.5 deg
Number of lines: 2
Input resolution:  640.0 PPI
Output resolution: 100.0 PPI
Scaled image by f=0.31 (200.0 PPI)
Total toolpath length: 2.6 in
 - Rapids:  0.2 in (0.0 sec)
 - Plunges: 2.4 in (4.7 sec)
 - Moves:   0.1 in (0.1 sec)
Feed rate: 100.0 in/min
Plunge rate: 30.0 in/min
Estimated machining time: 4.8 sec
```

## Frame

RasterCarve's origin is the picture's top-left corner with X to the right
and Y up, so the picture lies at Y ≤ 0 (its rows run down to
`Y = −height`), and Z is zero at the stock top. DreamCarve is Y-up with
the picture's bottom-left corner at (`x_mm`, `y_mm`) (ADR-0001), so, with
`k` the scale × 25.4 (RasterCarve writes inches, `G20`):

- `x_dc = x_mm + k · x_rc`
- `y_dc = y_mm + height_mm + k · y_rc`
- `z_dc = thickness_mm + k · z_rc`

RasterCarve's Y is negative down the picture, so the sign is `+`; the
design spec's draft had `− y_rc`, corrected from the captured program
(#647).

## Comparison

`crates/dc-gcode/tests/golden.rs` (`photo_raster_inch`, the harness in
`crates/dc-gcode/tests/golden/photo.rs`) scales the program by 25.4,
maps it as above and compares the two grooves in order: the one-point
groove at the picture's top-left corner, then the groove across it from
the right edge to the bottom edge. Rapids, the lifts between grooves
(RasterCarve's to its safe Z, DreamCarve's to its link height) and the
final traverse to `--end-z` are not compared.

- DreamCarve project: the picture 2.54 × 1.27 mm at (10, 10) on 22.54 ×
  21.27 × 6 mm stock (`init`'s 10 mm margin: at the stock's origin the
  full-depth groove's half-width would cross the stock edge, S7), a
  30° V-bit at 2540 mm/min and 762 mm/min plunge (100 and 30 in/min),
  `max_depth_mm` 2.032 (0.08 in), lines at 22.5°, `line_spacing_fraction`
  1.1, `sample_mm` 0.254 (0.01 in).
- XY: every sample within 0.01 mm of the other program's groove, the
  grooves' ends within 0.01 mm, and each point DreamCarve keeps after
  merging on one of RasterCarve's samples. Both sample every 0.254 mm
  from the end a groove is cut from (#647 fixed DreamCarve's odd
  grooves, which were sampled from their far end).
- Z: within 0.05 mm, or, where not, within 0.05 mm of the picture's depth
  somewhere within one resampled pixel, 0.127 mm (`Difference::Resampled`).
  RasterCarve resamples the picture to twice its output resolution
  ("Scaled image by f=0.31 (200.0 PPI)": 64 pixels into 20) and reads
  each sample's depth from that grid, so its depth along the groove steps
  by one or two resampled pixels per sample (Z steps of 0.0041 and
  0.0081 in alternate in `program.nc`) and lags the ramp by up to one
  resampled pixel: 0.107 mm at most, at the dark end, against
  DreamCarve's bilinear depth. The design spec expected the resampling
  difference under 0.01 mm; that holds only where RasterCarve enlarges
  the picture, as in `photo-raster-metric`.
- RasterCarve quantises depth to 256 levels: black is 255/256 of the
  maximum depth (−0.078438 in at the corner, 0.04 mm shallower than
  DreamCarve's 2.032 mm).
