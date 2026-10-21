# Machining order and cut order

A project with several shapes and several operations is cut in an order
DreamCarve works out from fixed rules, with one setting you choose. This
chapter gives the rules, the setting, and the two checks that warn when
the order you wrote is probably a mistake.

In short:

1. Operations run in the order the project lists them. Never regrouped.
2. Within an operation, `machining_order` says whether each shape is cut
   to full depth before the next, or each pass is cut across every shape
   before the next pass.
3. Within that, shapes are visited nearest first, except that a shape
   inside another is always cut before the shape around it.
4. Between shapes the tool lifts to safe Z, unless it can stay down in
   material the operation has already cut.

Whatever the order, the same cuts are made at the same depths. The order
never changes what is cut, only when.

## Operations run in list order

Export cuts the enabled operations in the order of the `operations` list,
whichever tools they use. DreamCarve never reorders them and never groups
them by tool. The order carries intent it cannot infer: rough before
finish, holes and pockets before the outline that frees the part.

With more than one tool, each run of consecutive operations with the same
tool is one **tool run**. T1, T2, T1 is three tool runs. By default each
is its own program, named in order (`plaque-1-T1.nc`, `plaque-2-T2.nc`,
…), and `export` lists them in the order to run them; with one program
with `M6`, the tool changes come in the same order. See [Projects with
more than one tool](post-processors.md#projects-with-more-than-one-tool).

A disabled operation is left out and does not split a tool run.

## Machining order

`machining_order` is a setting of each Profile, Pocket and Engrave. It says
how the passes of several shapes in one operation are interleaved:

- `"full_depth_first"` (the default, and how `init` writes it): each shape
  is cut to its full depth, every pass, before the tool moves to the next
  shape.
- `"pass_first"`: the first pass is cut on every shape, then the second
  pass on every shape, and so on to the final depth.

Full depth first keeps the tool on one shape, so there are fewer moves
between shapes. Pass first keeps every part held by full-thickness material
until the final pass, which can matter for small parts, thin stock or a
vacuum table.

Three 30 mm squares cut 6 mm deep in 2 mm passes: under
`"full_depth_first"` the passes go 2, 4, 6 mm deep on the first square,
then on the second, then on the third. Under `"pass_first"` they go 2 mm
on all three, then 4 mm on all three, then 6 mm on all three. Each pass
of pass first starts from where the previous pass ended, so successive
passes tend to visit the shapes in opposite order.

An operation with one shape cuts exactly the same under both.

A Drill has no `machining_order`: each hole is one plunge or one set of
pecks, drilled to depth before the tool moves on.

## Which shape comes next

Within an operation, and within each pass under pass first:

1. **The first shape in drawing order is cut first**: the first of the
   operation's `targets`, and of that instance's outlines.
2. **Then the nearest.** From where the last cut ended, the tool goes to
   the uncut shape whose start is nearest. Parts scattered across the stock
   are visited in a short route rather than in the order they were drawn.
3. **Ties go to drawing order.** Two shapes equally near are taken in the
   order they were drawn, so the order is the same on every export.
   Distances within 0.001 mm of each other count as equal, so the tiny
   rounding an SVG import adds to coordinates does not change the order:
   an SVG and a DXF of the same drawing are cut in the same order. The
   same rule orders corner relief notches, drilled holes, and a Pocket's
   rings and raster lines.

Squares drawn at X 10, 50 and 30 are cut at 10, 30, then 50.

One rule overrides nearness:

- **A shape inside another is cut before the shape around it.** In a
  Profile, each part's holes come before its outline, and a part inside
  another part's hole is cut before that hole, however deep the nesting.
  Cutting a hole frees the waste in it, and cutting an outline frees the
  part, so whatever sits inside must already be cut. For a frame with a
  coaster in its opening, the coaster is cut first, then the opening, then
  the frame's outline, whichever was drawn first. See [Shapes, holes, and
  islands](shapes-and-holes.md#6-order-follows-the-drawing).
- Under pass first, a shape's pass waits for the same pass of every shape
  inside it, and the pass that frees a part or a hole (the last one, on a
  through cut) waits until every shape inside it is finished.
- In a Pocket, the islands are not cut; a pocket drawn inside an island is
  cut after the pocket around the island.
- In an Engrave, a closed path inside another closed path is cut first.
  Open paths are visited nearest first wherever they lie.
- A Drill visits its holes nearest first from the first hole in drawing
  order.

## Links between cuts

The tool's move from the end of one cut to the start of the next is a
**link**. Between shapes, it is:

- a lift to safe Z, a rapid across at safe Z, and the operation's
  [entry](profile.md#entry) down (a ramp, a helix or a plunge); or
- a straight cut at the feed rate that stays down, only where the whole
  move lies in material this operation has already cut at that depth and
  is no longer than the tool's diameter plus its stepover. Pocket rings
  and raster lines link this way.

The tool never travels through uncut material, and never rapids below
safe Z, except inside a [Drill](drill.md#retract-height). The next pass of
the same shape starts from the floor of the pass before when it can (a
Profile ramps down from where its lap ended), and otherwise lifts and
comes back down in the same place.

## Checks on the operation order

The program is always written as listed. Three checks look at the order and
warn when it is probably wrong; `check` and `export` print them.

- **`cut_inside_freed_part`.** A Profile that cuts through the stock with
  no tabs cuts its parts free; an Inside Profile through the stock frees
  the slug in its hole. Any later operation that cuts a shape inside one
  of those outlines, a hole, a pocket, an engraving, runs in a loose
  piece. An open path an Engrave or an `on_line` Profile cuts counts when
  all of it lies inside the outline; one that crosses the outline does
  not. The warning names both operations and the shape. Move the later
  operation before the Profile. Cutting the details first and the
  outline last is quiet.
- **`cut_inside_tab_held_part`.** A Profile that cuts through the stock
  with [tabs](tabs.md) leaves its parts held only by them. A later
  operation that cuts deeper than half the stock inside one of those
  outlines (a hole through it, a deep pocket) works on a piece held by a
  few small bridges, which can flex, chatter or break a tab. A lighter
  cut, such as a shallow engraving, is quiet: it is normal practice. The
  operation's `depth_mm` is compared with the stock thickness. The same
  shapes count as for `cut_inside_freed_part`, and the warning names
  both operations, the shape and the innermost tabbed outline round it.
  Move the later operation before the Profile, or, for a hole, cut it in
  the same operation as its part, which cuts holes first ([Tabs: parts
  with holes](tabs.md#parts-with-holes)). A shape inside an outline the
  tabbed Profile cuts free with no tab, such as a hole's slug, warns
  `cut_inside_freed_part` instead.
- **`tool_change_avoidable`.** When one tool's operations are separated by
  another tool's (T1, T2, T1), the tool is fitted twice. `check` hints so,
  listing the operations, so you can put them together if the order
  allows. DreamCarve never moves them for you.

The messages and what to do are under [Warnings](warnings.md#operation-order).

## The plaque

The [plaque quick start](../quick-start/plaque.md) lists a raster Pocket,
an offset Pocket, a Drill and an outside Profile with the 6.35 mm end mill,
then an Engrave with the 3.175 mm end mill. That is two tool runs, so two
programs in that order. The Profile has tabs, so the Engrave after it
cuts in a part that is still held. It is a 1 mm groove in 11.8 mm
plywood, a light cut, so neither check warns.
