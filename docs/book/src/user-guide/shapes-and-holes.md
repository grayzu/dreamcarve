# Shapes, holes, and islands

A drawing is a set of closed shapes. Before DreamCarve can cut them, it
decides which shapes are **parts** (material you keep) and which are
**holes** cut out of a part. That is how a ring cuts as a ring: the outer
circle is cut on its outside, and the inner circle on its inside.

Each group of one part and its holes is a **region**. This page states the
rules, with a picture of each one.

## Reading the figures

Every figure on this page is generated from the real code: `nest` runs on
the shapes shown, and the picture draws what it returned. A test fails if a
figure no longer matches the code, so the pictures can't drift from it.

- **Shading** is the material of a region. Each region has its own colour,
  matching the `R1`, `R2`, … rows of the legend under the figure.
- **Solid blue** outlines are parts. **Dashed red** outlines are holes.
- The **arrowhead** shows which way the outline runs once nesting is done,
  seen from above with Y pointing away from you. Parts always run
  counter-clockwise (CCW) and holes clockwise (CW). The cutting direction
  (climb or conventional) is worked out from this.
- **`#n`** is the shape's position in the drawing, starting at `#0`. Two
  labels side by side, like `#0, #1`, are two shapes in the same place.

## The rules

### 1. A shape inside nothing is a part

Shapes that don't enclose each other are separate parts, and each becomes
its own region with no holes.

![Two separate rectangles become two regions](figures/nesting/disjoint.svg)

### 2. Count the shapes around it: even is a part, odd is a hole

A shape's depth is the number of other shapes that enclose it. Depth 0 is
a part, depth 1 is a hole, depth 2 is a part again (an island inside a
hole), and so on.

![A circle inside a circle is a ring: one part with one hole](figures/nesting/ring.svg)

![Three circles: part, hole, and an island that is a new part](figures/nesting/island.svg)

![Four nested squares alternate part, hole, part, hole](figures/nesting/four-levels.svg)

### 3. A hole belongs to the smallest part around it

In the four-squares figure, `#3` sits inside both `#0` and `#2`. It's a hole
of `#2`, the smaller one. When there are several parts, each hole goes to
the part it actually sits in, wherever the shapes appear in the drawing:

![Two parts, each with a hole, listed in mixed order](figures/nesting/two-parts-two-holes.svg)

### 4. Curves count by their real shape

Whether a shape is inside another is decided against the true curve, not
the straight line between its end points. A small circle inside the bulge of
a larger circle is a hole. A small circle that falls inside the larger
circle's bounding box but outside the curve is a separate part.

![A small circle in the upper bulge of a circle is a hole](figures/nesting/inside-arc-bulge.svg)

![A small circle beside the curve, inside its bounding box, is a separate part](figures/nesting/outside-arc-bulge.svg)

### 5. The direction you drew a shape in doesn't matter

Parts come out counter-clockwise and holes clockwise, whichever way they
were drawn. Here the outer square was drawn clockwise:

![A clockwise outer square still becomes a counter-clockwise part](figures/nesting/clockwise-input.svg)

### 6. Order follows the drawing

Regions are listed in the order their parts appear in the drawing, and holes
in the order they appear. In the second figure the hole `#0` comes first in
the drawing, yet the regions are ordered by their parts, `#1` then `#2`.

![One part with three holes, listed in drawing order](figures/nesting/several-holes.svg)

![Regions ordered by where their parts appear](figures/nesting/region-order.svg)

### 7. Reordering the drawing doesn't change which shapes are holes

Shuffling the shapes changes only the order of the results, never which
shapes are parts and which are holes. The test runs these shapes in four
different orders and gets the same regions each time.

![Three nested circles and a separate square](figures/nesting/order-independent.svg)

### 8. A hole may touch its part's edge

A hole that shares part of an edge with the part around it is still a hole.

![A rectangle sharing the left edge of a square is still a hole](figures/nesting/touching-edge.svg)

## Not yet decided

These cases are drawn from what the code does today, but no rule settles
them yet.

### Two identical shapes

Two copies of the same shape in the same place don't enclose each other, so
each becomes its own region. A Profile would cut the same outline twice.

![Two identical rectangles become two regions on top of each other](figures/nesting/identical.svg)

### Shapes that cross

Nesting assumes shapes don't cross. When they do, the result is wrong:
below, the smaller circle becomes a "hole" of the larger even though part of
it lies outside. Until this is handled, don't let closed shapes cross in a
drawing you cut.

![Two overlapping circles: the smaller is wrongly treated as a hole](figures/nesting/crossing.svg)

## For contributors

The rules live in `dc_geometry::nest` (`crates/dc-geometry/src/region.rs`).
The figures are generated by `tools/doc-figures`. After changing nesting,
run `cargo run -p doc-figures` and review the changed SVGs. The
`committed_figures_match_engine_output` test fails until you do.

| Rule | Figure | Unit test in `region::tests` |
| --- | --- | --- |
| 1. Separate parts | `disjoint` | `disjoint_shapes_are_separate_regions`, `lone_contour_is_one_region_without_holes`, `no_contours_give_no_regions` |
| 2. Even part, odd hole | `ring`, `island`, `four-levels` | `circle_in_circle_becomes_hole`, `island_in_hole_becomes_its_own_region`, `four_levels_alternate_outer_and_hole` |
| 3. Smallest enclosing part | `four-levels`, `two-parts-two-holes` | `four_levels_alternate_outer_and_hole`; **no test for several parts with holes** |
| 4. Curves by their real shape | `inside-arc-bulge`, `outside-arc-bulge` | `circle_inside_arc_bulge_is_hole`, `circle_outside_arc_bulge_is_separate`, `winding_counts_arc_bulge_not_chord`, `winding_of_rect_is_one_inside_zero_outside` |
| 5. Direction normalised | `clockwise-input` | `clockwise_input_is_normalised` |
| 6. Drawing order | `several-holes`, `region-order` | `several_holes_in_one_outer_keep_input_order`, `regions_follow_input_order_of_outers` |
| 7. Order-independent | `order-independent` | `containment_does_not_depend_on_input_order` |
| 8. Touching edge | `touching-edge` | `hole_touching_outer_edge_is_still_a_hole` |
| Identical shapes | `identical` | **none** |
| Crossing shapes | `crossing` | **none** (behaviour is wrong) |
