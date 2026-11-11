<!--
Copyright 2026 The DreamCarve Contributors
SPDX-License-Identifier: Apache-2.0
-->
# Golden case: photo-raster-inch

Captured from RasterCarve 1.0.8, commit `b3b844e7dbc6e560e361dbff7937b6e5ed81746b`, with
`tools/parity/photo/capture.py --case photo-raster-inch`
(the checkout's `rastercarve/` package, the command-line program, clean at
that commit).

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

The design spec's `y_dc = y_mm + height_mm − y_rc` read RasterCarve's Y as
growing down the picture; the captured program's Y is negative down the
picture, so the sign is `+` (#647).
