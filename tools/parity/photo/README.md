<!--
Copyright 2026 The DreamCarve Contributors
SPDX-License-Identifier: Apache-2.0
-->
# RasterCarve capture

Dev-only tool. Captures RasterCarve's program for a Photo V-Carve golden
case into `tests/golden/<case>/`, where `cargo test`
(`golden::photo_raster_inch`, `golden::photo_raster_metric`) compares
DreamCarve's grooves with it. Contributors without Python or RasterCarve
never need this: the golden files are committed.

## Licence

RasterCarve is GPLv2+, which is not Apache-2.0-compatible. It is a
behaviour and test reference only (security review M5):

- `capture.py` runs it as an external program. It never imports anything
  from the checkout, never adds it to its own `sys.path`, and copies none
  of its source.
- The only thing of RasterCarve's that enters this repository is the
  G-code its own command line writes on DreamCarve's own synthetic
  pictures (`tests/fixtures/photo/ramp.png` and `radial.png`), as
  `program.nc`, with a README saying how it was made. The GPL does not
  cover a program's output, and the file carries no RasterCarve source
  text.
- Nothing here drives RasterCarve's Python engine from a script: that
  would be a combined work, which is why the draft's hatch case was
  dropped. Hatch is covered by `dc-cam`'s unit tests alone.

## Requirements

- Python 3.9 or newer for `capture.py` (standard library only).
- A RasterCarve checkout and an interpreter with its command line's
  dependencies (`numpy`, `opencv-python` or `opencv-python-headless`,
  `tqdm`). By default the checkout's own virtual environment,
  `<checkout>/.venv/bin/python`; `--python` names another. The child
  process's `PYTHONPATH` is the checkout, so the code that runs is the
  checkout's whether or not it is installed in that interpreter.

The goldens were captured from RasterCarve 1.0.8, commit `b3b844e`
(`b3b844e7dbc6e560e361dbff7937b6e5ed81746b`), by Python 3.14.3 with
OpenCV 5.0.0 (`opencv-python-headless`), NumPy 2.5.4 and tqdm in a
separate virtual environment (`--python`), the checkout's own lacking
OpenCV. Each case's README records the versions, which the program
depends on: OpenCV resamples the picture.

## Usage

```sh
python3 -I tools/parity/photo/capture.py \
  --rastercarve /path/to/rastercarve --case photo-raster-inch
```

`--case` must match `^[a-z0-9-]+$` and name a case in `capture.py`'s
`CASES`; anything else is refused before any path is built. The script:

1. copies the case's picture into a fresh temporary directory;
2. runs `python -m rastercarve <args> <picture>` there, by the
   interpreter's absolute path, with an argument list and no shell;
3. writes `tests/golden/<case>/program.nc`, RasterCarve's standard output
   byte for byte, and `README.md`: the command line, RasterCarve's
   version and commit, its statistics block (from its standard error,
   without the progress bar), the frame mapping and
   `tools/parity/photo/<case>.notes.md`.

It refuses a checkout whose `rastercarve/` package has uncommitted
changes, since the README could not name the code that ran. Run it twice:
the output must be byte-identical. The DreamCarve project,
`tests/golden/<case>/dreamcarve.dcv`, is written by hand (`dreamcarve
init <picture> --width …`, then the case's bit, depth and step); the
capture never writes it.

`test_capture.py` tests the argument check:

```sh
python3 -I tools/parity/photo/test_capture.py
```

## Cases

| Case | Picture | RasterCarve arguments | DreamCarve project |
| --- | --- | --- | --- |
| `photo-raster-inch` | `ramp.png` (64 × 32, black left to white right) | `--width 0.1` (defaults: 30°, 0.08 in, 22.5°, 110 %, 0.01 in, safe 0.1, feeds 100/30) | 2.54 mm wide at (10, 10), `max_depth_mm` 2.032, `sample_mm` 0.254; the harness scales the golden by 25.4 |
| `photo-raster-metric` | `radial.png` (64 × 64, white centre to black edge) | `--metric --width 100 -t 30 -d 2 -a 22.5 -s 110 -r 0.25 -z 5 -f 1000 -p 300` | 100 mm wide at (10, 10), `max_depth_mm` 2, `sample_mm` 0.25 |

## What is compared, and what is not

The frame: RasterCarve's origin is the picture's top-left corner, Y up,
so the picture lies at `Y ≤ 0`, and Z is zero at the stock top. With `k`
its unit in mm, `x_dc = x_mm + k · x_rc`, `y_dc = y_mm + height_mm + k ·
y_rc`, `z_dc = thickness_mm + k · z_rc`.

Compared, groove for groove in order (a groove is the plunge's end and
the cuts after it, down to the lift): every sample within 0.01 mm in XY
of the other program's groove and 0.05 mm in Z of its Z there; the
grooves' ends within 0.01 mm; and each point DreamCarve keeps after
merging collinear samples on one of RasterCarve's samples, since both
sample from the groove's start.

Not compared: rapids, the lifts and links between grooves (RasterCarve
lifts to its safe Z, DreamCarve to its link height, the stock top plus
`stepover_retract_mm`), RasterCarve's final traverse to `--end-z`, feeds,
and the header and footer.

Two of RasterCarve's own behaviours are allowed for in one case each,
never globally (`Difference` in `crates/dc-gcode/tests/golden/photo.rs`;
each case's README):

- `Resampled` (`photo-raster-inch`): RasterCarve resamples the picture to
  twice its output resolution and reads depth from that grid; shrinking
  64 pixels into 20 makes its depth lag the ramp by up to one resampled
  pixel (0.107 mm), so a Z that misses by more than 0.05 mm passes when
  it is within 0.05 mm of the picture's depth within 0.127 mm.
- `LastLineUncut` (`photo-raster-metric`): RasterCarve leaves the line
  through the far corner uncut at some widths; DreamCarve cuts it.
