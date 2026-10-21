# Profile operation

A **Profile** cuts along the outline of closed shapes: to cut a part out of
the stock, or to cut a hole through it. It is the only operation in
phase 1.

```json
{
  "kind": "profile",
  "id": "op1",
  "name": "Profile",
  "enabled": true,
  "targets": [ "i1" ],
  "tool": "t1",
  "side": "outside",
  "depth_mm": 12.0,
  "step_down_mm": null,
  "direction": "climb",
  "through_cut_allowance_mm": 0.2,
  "safe_z_mm": 5.0,
  "tabs": { … }
}
```

## Settings

| Field | Meaning |
| --- | --- |
| `id` | The operation's id, shown in messages. |
| `name` | Your name for it, written in the program. |
| `enabled` | `false` leaves the operation out of the program, with a warning. |
| `targets` | The ids of the instances to cut. `init` lists every instance with a closed outline. |
| `tool` | The id of the tool to cut with. |
| `side` | `"outside"`, `"inside"`, or `"on_line"`. See [Side](#side). |
| `depth_mm` | How deep to cut, measured down from the stock top. **Required:** `init` leaves it `null`, and export refuses until you set it. To cut through, use the stock's `thickness_mm` plus `through_cut_allowance_mm`. |
| `step_down_mm` | The most to cut in one pass. `null`, as `init` writes it, uses the tool's `step_down_mm`; if both are `null`, `check` reports the tool's as not set and export refuses. More than the tool's diameter is a warning. |
| `direction` | `"climb"` or `"conventional"`. See [Direction](#direction). |
| `through_cut_allowance_mm` | How far below the stock bottom the cut may go. 0 by default. See [Cutting through](#cutting-through). |
| `safe_z_mm` | The height above the stock top for moves between cuts. Must be more than 0. |
| `tabs` | Holding tabs, or `null` for none. See [Tabs](tabs.md). |
| `machining_order` | `"full_depth_first"` or `"pass_first"`. See [Machining order](#machining-order). |

## Side

The bit has a width, so its centre cannot follow the line you drew. The
side says where the bit's edge runs:

- `"outside"`: the bit runs outside the outline, offset by half its
  diameter. The part keeps the size you drew. Use it to cut a part out.
- `"inside"`: the bit runs inside the outline. The hole has the size you
  drew. Use it to cut a hole or an opening.
- `"on_line"`: the bit's centre follows the line. The cut is one bit wide,
  centred on the line.

Inside and outside are measured from the part, not from each shape. A shape
inside another is a hole in it, so on a ring with an outside Profile, the
outer circle is cut on its outside and the inner circle on its inside. Both
edges of the ring keep the size you drew.
[Shapes, holes, and islands](shapes-and-holes.md) has the rules.

This is decided among one operation's targets only. A hole cut by an
operation of its own is a part to that operation, so cut it `"inside"`:
[Tabs](tabs.md#parts-with-holes) shows how, for a part whose hole should
get no tabs.

A 100 × 50 mm rectangle cut outside with a 6.35 mm bit gives a tool path
106.35 × 56.35 mm across its centre line; cut inside, 93.65 × 43.65 mm.

Offsetting a corner outward rounds it, so an outside path has a small arc at
each corner of the outline. A curve in the SVG reaches DreamCarve as many
short straight lines, so an outside cut of a circle is many short lines
joined by tiny arcs (`G1` and `G2` moves in turn).

## Depth and passes

A bit cannot safely cut the whole depth at once. The cut goes down in
**passes**, each at most `step_down_mm` deeper than the last. Only the
final pass can be shallower than a full step; it is never deeper.

| Stock | `depth_mm` | `step_down_mm` | Passes, Z above the stock bottom |
| --- | --- | --- | --- |
| 12 mm | 12 | 4 | 8, 4, 0 |
| 12 mm | 12 | 5 | 7, 2, 0 |
| 12 mm | 10 | 4 | 8, 4, 2 |

## Cutting through

With `through_cut_allowance_mm` at 0, `depth_mm` may be at most the stock
thickness: the cut stops exactly at the stock bottom. Plywood often leaves a
thin skin of fibres there. To cut slightly into the spoilboard (the
sacrificial board under the stock), set both:

```json
"depth_mm": 12.2,
"through_cut_allowance_mm": 0.2,
```

A depth greater than the thickness plus the allowance is an error, and
nothing is written.

## Direction

The bit turns clockwise seen from above. **Climb** milling moves the bit so
its teeth enter the material at full thickness and leave it thin; it usually
leaves a cleaner edge on a hobby router. **Conventional** milling is the
reverse; it pulls the bit less hard into the material on a machine with
backlash.

| The bit runs | Climb | Conventional |
| --- | --- | --- |
| Outside the outline it cuts | Clockwise | Counter-clockwise |
| Inside the outline it cuts | Counter-clockwise | Clockwise |

Directions are seen from above, with Y pointing away from you. An outside
Profile runs outside a part's outline and inside its holes. An `on_line`
cut counts as outside on a part's outline and inside on a hole.

## Machining order

`machining_order` says how the passes of several shapes are interleaved:

- `"full_depth_first"` (the default, and how `init` writes it): each shape
  is cut to its full depth before the tool moves to the next shape.
- `"pass_first"`: the first pass is cut on every shape, then the second
  pass on every shape, and so on to the final depth.

Full depth first keeps the tool on one shape, so there are fewer moves
between shapes. Pass first keeps every part held by full-thickness material
until the final pass, which can matter for small parts or thin stock.
Either way the same cuts are made at the same depths; only their order
changes, and an operation with one shape cuts exactly the same under both.

Within an operation the shapes are visited nearest first: the first shape
in the drawing is cut first, then whichever uncut shape starts nearest to
where the last cut ended, and so on. Under pass first each pass starts from
where the previous pass ended, so successive passes tend to run in opposite
directions. Two shapes equally near are taken in drawing order. One rule
overrides nearness: a shape is always cut before any shape that encloses
it (a hole before its outline, a part inside a hole before that hole), so a
part is freed only by its last contour. See [Shapes and holes, rule
6](shapes-and-holes.md#6-order-follows-the-drawing).

## What the program does

For each part, the holes are cut first and the outline last, so a part cut
through is freed only at the end. A part inside another part's hole is cut
before that hole, however deep the nesting, so it is never cut out of loose
waste. Otherwise shapes are visited nearest first, starting from the first
one drawn (see [Machining order](#machining-order)). For each outline:

1. Rapid to safe Z, then to the start of the outline.
2. Plunge to the first pass at the plunge rate.
3. Cut one full lap at the feed rate, ending where it began.
4. Plunge to the next pass and cut another lap, until the last pass. Under
   `"pass_first"` the tool instead rapids up to safe Z and goes on to the
   next shape, coming back for the next pass once every shape has had this
   one.
5. Rapid back up to safe Z.

Every move between shapes is at safe Z; the tool never travels through
material from one shape to another.

## Warnings

These still give a safe program. `check` and `export` print each one as a
sentence, after the operation's id, with lengths rounded to 0.01 mm.

| Warning says | Meaning |
| --- | --- |
| "... is an open path" | A target is not a closed outline; it is not cut. |
| "tool ... does not fit ..." | The bit does not fit inside the shape; that shape is not cut. |
| "... is the same outline in the same place as ..." | Two shapes are the same outline in the same place; the second is not cut. |
| "tabs on ...", "the tab top is ..." | See [Tabs](tabs.md#warnings). |
| "the tool paths of ... and ... cut into each other" | Two shapes are closer than the bit needs to cut between them, so cutting one gouges the other. See below. |

### Values that look wrong

`check` and `export` also look for values that are allowed but probably a
mistake. They check only enabled operations and the tools those use. Each
finding names the operation or tool and starts with where the value is in
the file, as in `coaster.dcv:24:22:`. DreamCarve never changes the value
for you.

| Message says | Kind | Meaning |
| --- | --- | --- |
| "... cuts ... mm per pass, deeper than the ... mm diameter of tool ..." | Warning | The step-down in effect (the operation's, or else the tool's) is more than the bit's diameter. Passes that deep can break the bit. |
| "... plunges at ... mm/min, faster than it cuts ..." | Warning | The tool's `plunge_mm_min` is above its `feed_mm_min`. They may be swapped. |
| "... is set to ... RPM, faster than the 60000 RPM ..." | Warning | The spindle speed is above 60 000 RPM. GRBL and grblHAL run the spindle at its maximum instead, and with manual spindle control the speed is only a hint, so this does not stop export. |
| "... is ... mm in diameter, larger than the 50 mm ..." | Warning | The tool's `diameter_mm` is above 50 mm. Check it for a typo. |
| "feed_mm_min is ..., expected a value greater than 0 and at most 20000 mm/min" (or `plunge_mm_min`) | Error | No hobby machine moves faster than 20 000 mm/min, so the value is almost surely a typo. Export fails. |
| "... cuts with tool ..., a V-bit; an Inside or Outside Profile and a Pocket offset the path by half the diameter ..." | Error | With `side` `"outside"` or `"inside"`, Profile needs a flat end mill (`"cutter": "end_mill"`): it offsets the path by half the diameter, so with a V-bit, ball-nose or engraving bit the part comes out the wrong size. Export fails. With `"on_line"` the bit's centre follows the line, so any cutter is allowed. |
| "... is enabled but has no targets ..." | Error | The operation would cut nothing. Add targets or set `enabled` to `false`. |

### Shapes too close together

DreamCarve cuts each shape on its own. When two shapes in one operation are
closer than the bit needs to pass between them, the bit cutting one also
cuts into the other. With **Outside** both cuts run in the gap, so the
shapes need at least one bit diameter between them; with **On line** each
cut reaches half a bit past its line, so they need one radius. **Inside**
cuts stay within their own shapes and never warn.

The warning names both shapes and the smallest gap between them, listed in
the order the shapes are cut. It also applies to a part inside another
part's hole: a coaster 1 mm from the wall of a frame's hole warns. A part
and its own holes are never compared: a hole close to its outline leaves a
thin wall, not a gouge. The program is still written as drawn. Move the
shapes apart, or use a smaller bit.

Shapes that cross each other, or a shape that crosses itself, are an error:
DreamCarve cannot tell which side is material, so it cuts nothing. See
[Shapes, holes, and islands](shapes-and-holes.md), rules 10 and 11.
