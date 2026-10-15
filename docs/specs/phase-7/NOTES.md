# Phase 7 — requirements captured ahead of the spec

Items agreed before the phase 7 specs were written. Fold each into
`functional.md` and `design.md` when the phase starts, then delete this file.

## Grayscale heightmap import

A user can import a grayscale image (PNG, JPEG, TIFF; 8- or 16-bit) as a
**Heightmap** model and machine it with the same Surface Rough and Surface
Finish operations as a mesh. Motivating case: a lake bathymetry chart, where
darker pixels are deeper water, carved as a relief without ever building a 3D
model.

**Import parameters** (all editable afterwards, so the model stays
parametric):

- Footprint: width and height in mm. Default preserves the image aspect
  ratio from a user-entered width.
- Z range: Z of white and Z of black, in mm relative to the model's
  `z_top`/`z_bottom` on its instance. Either may be the higher value, so a
  depth chart (dark = deep) and a terrain tile (light = high) both import
  without pre-processing.
- Invert toggle as a convenience for the above.
- Mapping curve: linear by default; optional gamma, since depth charts are
  often perceptually scaled.
- Smoothing radius in pixels (default 0) for noisy scans; applied at import
  into the cached height grid, never to the stored image.
- Colour images convert to luminance with a warning; alpha is ignored.

**Representation.** The project stores the image bytes (embedded, like
PureCutCNC embeds mesh data in `.camj`) plus the parameters. The machining
input is a sampled height grid at a cell size chosen the same way the mesh
path chooses it. This is the seam: PureCutCNC's surface strategies rasterize
the mesh into an internal height map (`getCachedHeightMap`,
`chooseHeightMapCellSize`, `safeToolTipZAt` in `finishSurfaceParallel.ts`)
and sample against it. DreamCarve's design should make the height grid the
common input to surface strategies, with mesh and heightmap as two producers
of it. Then the lake carves through exactly the code path the mesh does.
Bilinear sampling between pixels; the footprint edge is the model's
silhouette (a rectangle), so retained-material logic needs no special case.

**Resolution.** A 4000×4000 scan at 0.1 mm cells over a 300 mm footprint is
fine; the cell-size chooser must downsample rather than refuse, with the
existing `surfaceHeightMapReduced` warning ported.

**Simulation and preview.** The heightmap displays in the 3D view as a
heightfield directly (ADR-0003 makes this the natural fit) and in the 2D
view as its grayscale image under the instance transform, like PureCutCNC's
STL top-view image.

**Test part:** a 200×150 mm lake-bed relief from a public-domain bathymetry
image, ball-nose finish after end-mill rough, in a soft wood. Success: the
deepest point measures within 0.3 mm of the configured black Z.

**Golden corpus:** none from PureCutCNC for the import itself; the surface
strategy goldens apply once the height grid is built. Unit tests cover the
image-to-grid mapping (white/black Z, inversion, gamma, aspect ratio,
bilinear sample at a half-pixel).

## Mesh import

Unchanged from the plan: STL and OBJ with orientation and silhouette
extraction. STEP stays post-1.0.
