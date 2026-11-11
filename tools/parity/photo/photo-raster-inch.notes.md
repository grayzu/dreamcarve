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
