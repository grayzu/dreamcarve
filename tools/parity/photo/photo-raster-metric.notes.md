## Comparison

`crates/dc-gcode/tests/golden.rs` (`photo_raster_metric`, the harness in
`crates/dc-gcode/tests/golden/photo.rs`) maps the program as above and
compares the grooves in order. Rapids, the lifts between grooves
(RasterCarve's to its 5 mm safe Z, DreamCarve's to its link height) and
the final traverse to `--end-z` are not compared.

- DreamCarve project: the picture 100 × 100 mm at (10, 10) on 120 × 120
  × 6 mm stock (`init`'s 10 mm margin, for S7), a 30° V-bit at 1000
  mm/min and 300 mm/min plunge, `max_depth_mm` 2, lines at 22.5°,
  `line_spacing_fraction` 1.1, `sample_mm` 0.25.
- XY: every sample within 0.01 mm of the other program's groove, the
  grooves' ends within 0.01 mm, and each point DreamCarve keeps after
  merging on one of RasterCarve's samples.
- Z: within 0.05 mm. RasterCarve enlarges the picture here ("Scaled
  image by f=12.50": 64 pixels into 800), so its resampling and
  DreamCarve's bilinear sampling agree within 0.021 mm, most of it
  RasterCarve's 256-level depth quantisation (0.008 mm a level).
- Grooves (`Difference::LastLineUncut`): RasterCarve's statistics count
  111 lines, but its program cuts 110: it stops one line short of the
  picture's bottom-right corner, leaving a triangle about 2.2 mm deep
  uncut. DreamCarve cuts every line that meets the footprint (design
  spec, "Photo V-Carve", step 2), so its 111th groove, 2.7 mm long across
  that corner, is its own: the harness checks only that it lies one
  spacing (1.179 mm) past the 110th and inside the picture. Run with
  other widths, RasterCarve's command line does the same at some
  (100 and 20 mm on `radial.png`, 20 mm on `ramp.png`) and not at others
  (50, 30 and 77 mm), so this is RasterCarve's, not the case's.
