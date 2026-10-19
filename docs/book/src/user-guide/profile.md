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
  "step_down_mm": 4.0,
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
| `step_down_mm` | The most to cut in one pass. `null` uses the tool's `step_down_mm`; if both are `null`, export fails. |
| `direction` | `"climb"` or `"conventional"`. See [Direction](#direction). |
| `through_cut_allowance_mm` | How far below the stock bottom the cut may go. 0 by default. See [Cutting through](#cutting-through). |
| `safe_z_mm` | The height above the stock top for moves between cuts. Must be more than 0. |
| `tabs` | Holding tabs, or `null` for none. See [Tabs](tabs.md). |

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

## What the program does

For each part, the holes are cut first and the outline last, so a part cut
through is freed only at the end. A part inside another part's hole is cut
before that hole, however deep the nesting, so it is never cut out of loose
waste. Otherwise parts are cut in drawing order. For each outline:

1. Rapid to safe Z, then to the start of the outline.
2. Plunge to the first pass at the plunge rate.
3. Cut one full lap at the feed rate, ending where it began.
4. Plunge to the next pass and cut another lap, until the last pass.
5. Rapid back up to safe Z.

## Warnings

These still give a safe program. `check` and `export` print each one as a
sentence, after the operation's id, with lengths rounded to 0.01 mm.

| Warning says | Meaning |
| --- | --- |
| "... is an open path" | A target is not a closed outline; it is not cut. |
| "tool ... does not fit ..." | The bit does not fit inside the shape; that shape is not cut. |
| "... is the same outline in the same place as ..." | Two shapes are the same outline in the same place; the second is not cut. |
| "tabs on ...", "the tab top is ..." | See [Tabs](tabs.md#warnings). |

Shapes that cross each other, or a shape that crosses itself, are an error:
DreamCarve cannot tell which side is material, so it cuts nothing. See
[Shapes, holes, and islands](shapes-and-holes.md), rules 10 and 11.
