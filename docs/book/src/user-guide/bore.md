# Bore operation

A **Bore** cuts a round hole bigger than the end mill to the size you drew
it: screw holes, dowel holes, holes for a bolt or a bearing. For each
circle, the bit spirals down a helix inside the hole, close to the wall,
to the depth. Then it cuts one full circle at the bottom, so the wall is
round and to size and the floor is flat. Fusion calls this "2D Bore". A
[Drill](drill.md) only plunges, so its hole is always the bit's size. Use
a Bore when the hole is bigger than the bit.

Add it to the project's `operations` list by hand; `init` writes only a
Profile. A complete Bore, cutting four 5 mm screw holes through 11.8 mm
plywood with a 3.175 mm end mill:

```json
{
  "kind": "bore",
  "id": "op3",
  "name": "Screw holes",
  "enabled": true,
  "targets": [ "i5", "i6", "i7", "i8" ],
  "tool": "t3",
  "depth_mm": 12.0,
  "pitch_mm": null,
  "direction": "climb",
  "stock_to_leave_mm": 0.0,
  "through_cut_allowance_mm": 0.2,
  "safe_z_mm": 15.0
}
```

## Settings

| Field | Meaning |
| --- | --- |
| `id` | The operation's id, shown in messages. |
| `name` | Your name for it, written in the program. |
| `enabled` | `false` leaves the operation out of the program, with a warning. |
| `targets` | The ids of the instances to bore. Every circle among their outlines is a hole; any other outline is skipped with a warning, as for a [Drill](drill.md#which-outlines-are-holes). |
| `tool` | The id of the tool to bore with: a flat end mill (`"end_mill"`). Any other cutter is an error at `check`, because a Bore cuts the wall with the side of the bit. |
| `depth_mm` | How deep to cut, measured down from the stock top. **Required.** To cut through, use the stock's `thickness_mm` plus `through_cut_allowance_mm`. |
| `pitch_mm` | How far the helix goes down in each turn. `null` uses the tool's `step_down_mm`. It must be more than 0 and no more than the tool's `step_down_mm`; a larger pitch is an error at `check`. See [Pitch](#pitch). |
| `direction` | `"climb"` (the usual) or `"conventional"`, as for [Pocket](pocket.md) and [Profile](profile.md). Climb turns counter-clockwise seen from above, conventional clockwise. |
| `stock_to_leave_mm` | Material to leave on the wall, 0 or more: the hole comes out this much smaller all round. 0 by default. See [Finishing](#finishing). |
| `through_cut_allowance_mm` | How far below the stock bottom the hole may go, as for [Profile](profile.md#cutting-through). 0 by default. |
| `safe_z_mm` | The height above the stock top for moves between holes. Must be more than 0. |

The tool needs a `feed_mm_min` and a `plunge_mm_min`, and, unless
`pitch_mm` is set, a `step_down_mm`. Export needs its `rpm` set too, as for
every operation.

## How a hole is cut

For each hole:

1. The bit rapids at safe Z to its start point, on the helix at the
   hole's east side (+X), then straight down to 1 mm above the stock
   top. It never plunges into the material.
2. It spirals down a helix centred on the hole. The helix radius is the
   hole's radius less the bit's radius and less `stock_to_leave_mm`, so the
   edge of the bit runs on the wall. Each turn goes down `pitch_mm`.
   The helix is written as short straight `G1` moves, at least 48 a turn,
   each with its own Z. Each move bows in from the true circle by no
   more than the project's `flatten_tolerance_mm`.
3. At the depth it cuts one full circle at the same radius, as four
   quarter arcs (`G3` for climb, `G2` for conventional). This flattens
   the floor the helix left and brings the wall to size.
4. It moves to the centre of the hole, off the wall, and lifts straight
   up to safe Z, so it does not drag up the finished wall.

A 5 mm hole with a 3.175 mm bit has a helix 0.9125 mm from the centre:
2.5 mm, the hole's radius, less 1.5875 mm, the bit's.

The helix goes down at the cutting feed, slowed where needed so the bit
never goes down faster than the plunge rate. The circle and the move to
the centre are at the cutting feed. Between holes the bit travels at safe
Z, or at the [clearance height](project-file.md#heights) between separate
parts. The holes are cut nearest first, starting from the first one in
drawing order, as a Drill's are. `machining_order` does not apply: each
hole is cut to depth before the bit moves on.

## Pitch

`pitch_mm` is how far down each turn of the helix goes (Fusion's
"Pitch"). One turn removes a full ring of material that deep, so it is a
step-down: `null` uses the tool's `step_down_mm`, and a larger pitch is an
error. A smaller pitch takes more turns, and gives a smoother wall and a
lighter cut in hard materials or with a small bit.

A pitch so fine that one move of the helix would go down less than the
program's last decimal place cannot be written, and export refuses it,
naming `pitch_mm`. In millimetres that is under about 0.05 mm a turn.

## Holes it skips

A Bore cuts only what it can cut as drawn. It skips these holes, and
`check` and `export` warn for each. The other holes of the operation are
still cut, and the program is still written:

- **Too small for the bit** (`bore_no_room`). The bit, with the
  stock to leave, has no room to spiral: the helix radius would be under
  0.05 mm. A hole smaller than the bit cannot be cut with it at all; use a
  smaller end mill. A hole about the bit's size is a Drill's job, and the
  message says so:

  ```text
  warning: bore_no_room: operation `op3`: the 3.175 mm hole `hole` (instance `i5`, profile 0) leaves the 3.175 mm tool no room to spiral, so it is not bored; a hole the bit's size is a Drill's: drill it with a Drill
  ```

- **More than twice the bit** (`bore_leaves_core`). A helix along the
  wall of a hole wider than twice the bit's diameter misses the middle,
  and leaves a round core standing. In a through hole the core comes
  loose at the last turn, and the spinning bit can throw it, so a Bore
  never cuts such a hole. Clear it with a [Pocket](pocket.md), which
  clears the middle in rings, or use a bigger end mill. A hole exactly
  twice the bit is bored: the edge of the bit just passes the centre.

  ```text
  warning: bore_leaves_core: operation `op3`: the 10 mm hole `hole` (instance `i5`, profile 0) is more than twice the 3.175 mm tool: a helix along its wall would leave a 3.65 mm core standing, which comes loose in a through hole, so it is not bored; clear it with a Pocket, or use a bigger end mill
  ```

- **Not a circle** (`bore_target_not_circle`): a rectangle, a slot, an
  ellipse or an open path.

If every target is skipped, the operation cuts nothing and warns
`bore_no_holes`.

With a 3.175 mm end mill a Bore cuts holes from about 3.28 mm to 6.35 mm
across; with a 6.35 mm one, from about 6.45 mm to 12.7 mm. A 0.1 mm
stock to leave takes 0.2 mm off both ends.

## Finishing

One helix and one circle cut the hole. There are no separate finishing
passes. For a cleaner wall in a hard material, use two Bores of the same
holes: the first with an `stock_to_leave_mm` of, say, 0.2 to rough them, and
the second with `stock_to_leave_mm: 0` to take the last 0.2 mm off the wall.

## Safety

`export` checks the written program of every Bore: every move stays
within the helix radius of one of its holes' centres, so nothing cuts
past the wall you drew, and nothing goes below the stock bottom less
`through_cut_allowance_mm` ([Safety](safety.md#what-dreamcarve-checks)).
A Bore is an inside cut, so put it before the Profile that cuts its part
out; after it, `check` warns `cut_inside_freed_part` or
`cut_inside_tab_held_part` ([Machining order](machining-order.md)).

## Warnings

| Code | Meaning |
| --- | --- |
| `bore_target_not_circle` | A target outline is not a circle, or is open, so it is not bored. |
| `bore_no_room` | The bit, with the stock to leave, does not fit the hole or fits it too closely to spiral, so it is not bored. Use a smaller end mill, or a Drill for a hole the bit's size. |
| `bore_leaves_core` | The hole is more than twice the bit's diameter, so a helix would leave a core standing; it is not bored. Use a Pocket, or a bigger end mill. |
| `bore_no_holes` | Every target was skipped, so the operation cuts nothing. |

The errors are those of every operation (a missing feed or plunge rate,
a depth below the stock bottom plus the allowance), and a pitch that is
not more than 0, more than the tool's step-down, or too fine to write.
