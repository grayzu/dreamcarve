<!--
Copyright 2026 The DreamCarve Contributors
SPDX-License-Identifier: Apache-2.0
-->
# Golden case: photo-raster-metric

Captured from RasterCarve 1.0.8, commit `b3b844e7dbc6e560e361dbff7937b6e5ed81746b`, with
`tools/parity/photo/capture.py --case photo-raster-metric`
(the checkout's `rastercarve/` package, the command-line program, clean at
that commit).

- Picture: `tests/fixtures/photo/radial.png` (synthetic; see
  `tests/fixtures/README.md`).
- Command line, run in an empty directory holding a copy of the picture:

  ```sh
  python -m rastercarve --metric --width 100 -t 30 -d 2 -a 22.5 -s 110 -r 0.25 -z 5 -f 1000 -p 300 radial.png
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
Input resolution: 64x64 px
Output dimensions: 100.00mm wide by 100.00mm tall = 10000.0 mm^2
Max line depth: 2.000 mm
Max line width: 1.072 mm (30.0 deg V-bit)
Line spacing: 1.179 mm (110% stepover)
Line angle: 22.5 deg
Number of lines: 111
Input resolution:  0.6 PPI
Output resolution: 4.0 PPI
Scaled image by f=12.50 (8.0 PPI)
Total toolpath length: 9589.0 mm
 - Rapids:  332.8 mm (83.2 sec)
 - Plunges: 773.1 mm (154.6 sec)
 - Moves:   8483.0 mm (509.0 sec)
Feed rate: 1000.0 mm/min
Plunge rate: 300.0 mm/min
Estimated machining time: 746.8 sec
```

## Frame

RasterCarve's origin is the picture's top-left corner with X to the right
and Y up, so the picture lies at Y ≤ 0 (its rows run down to
`Y = −height`), and Z is zero at the stock top. DreamCarve is Y-up with
the picture's bottom-left corner at (`x_mm`, `y_mm`) (ADR-0001), so, with
`k` the scale × 1 (RasterCarve writes millimetres, `G21`):

- `x_dc = x_mm + k · x_rc`
- `y_dc = y_mm + height_mm + k · y_rc`
- `z_dc = thickness_mm + k · z_rc`

The design spec's `y_dc = y_mm + height_mm − y_rc` read RasterCarve's Y as
growing down the picture; the captured program's Y is negative down the
picture, so the sign is `+` (#647).
