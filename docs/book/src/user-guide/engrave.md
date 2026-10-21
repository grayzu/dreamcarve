# Engrave operation

An **Engrave** follows lines with the centre of the bit on the line: a
border, a signature, a guide line, the outline of a letter. It cuts open
paths as well as closed ones, and it never offsets the path, so the groove
is centred on the line you drew. VCarve calls this "Quick Engraving" with
the Outline strategy; Fusion calls it "2D Trace".

Add it to the project's `operations` list by hand; `init` writes only a
Profile. A complete Engrave:

```json
{
  "kind": "engrave",
  "id": "op2",
  "name": "Border",
  "enabled": true,
  "targets": [ "i2", "i3" ],
  "tool": "t2",
  "depth_mm": 1.0,
  "step_down_mm": 0.5,
  "alternate_direction": true,
  "machining_order": "full_depth_first",
  "through_cut_allowance_mm": 0.0,
  "safe_z_mm": 5.0
}
```

## Settings

| Field | Meaning |
| --- | --- |
| `id` | The operation's id, shown in messages. |
| `name` | Your name for it, written in the program. |
| `enabled` | `false` leaves the operation out of the program, with a warning. |
| `targets` | The ids of the instances to engrave. Every path of each one is cut, open or closed. |
| `tool` | The id of the tool to cut with. Any cutter: a V-bit, an engraving bit, a ball nose or an end mill. |
| `depth_mm` | How deep to cut, measured down from the stock top. **Required.** |
| `step_down_mm` | The most to cut in one pass. `null` uses the tool's `step_down_mm`; if both are `null`, `check` reports it and export refuses. |
| `alternate_direction` | `true` cuts the passes of an open path back and forth. See [Alternating direction](#alternating-direction). Default `false`. |
| `machining_order` | `"full_depth_first"` or `"pass_first"`. See [Machining order](machining-order.md#machining-order). |
| `through_cut_allowance_mm` | How far below the stock bottom the cut may go, as for [Profile](profile.md#cutting-through). Engraving rarely needs it; leave it at 0. |
| `safe_z_mm` | The height above the stock top for moves between cuts. Must be more than 0. |

## What the program does

For each path, nearest first from the first one drawn:

1. Rapid to safe Z, then to the start of the path.
2. Plunge to the first pass at the plunge rate.
3. Follow the path at the feed rate. A closed path ends where it began.
4. Go down to the next pass and follow the path again, until the last
   pass (see below for how). Under `"pass_first"` the tool instead goes on
   to the next path, and comes back for the next pass once every path has
   had this one.
5. Rapid back up to safe Z.

The passes step down as Profile's do: each at most `step_down_mm` deeper,
only the last one shallower. 1 mm deep in 0.5 mm steps on 12 mm stock cuts
at Z 11.5 and 11 above the stock bottom.

A closed path ends at its start, so the tool plunges straight to the next
pass there. An open path ends at its far end. By default the tool then
lifts to safe Z, rapids back to the start, and plunges, so every pass runs
the same way.

A closed path inside another closed path is cut first, as with Profile.
For an engraving that changes nothing about the result, but it keeps the
order the same every time. Open paths are cut nearest first, whatever
they lie in.

### Alternating direction

With `"alternate_direction": true`, the second pass of an open path runs
from its far end back to its start, the third forwards again, and so on.
The tool plunges straight down where the last pass ended instead of lifting
and travelling back: one retract and one rapid saved per pass. The groove
is the same; only the direction of cut changes, which on most engraving is
not visible. Closed paths are unaffected: they already end where they
start, so every pass runs the same way.

## V-bits

The bit's centre follows the line, so the groove is centred on it. With a
V-bit the groove's width depends on the depth: it is
`2 × depth × tan(angle ÷ 2)` wide at the top. A 60° V-bit 1 mm deep cuts a
groove 1.15 mm wide; a 90° V-bit, 2 mm wide.

A V-bit can only cut as deep as its cone is tall: `(diameter ÷ 2) ÷
tan(angle ÷ 2)`. Below that the straight shank cuts, and the groove's walls
are no longer V-shaped. `check` warns `engrave_deeper_than_cone` when
`depth_mm` is deeper than the cone (see [Warnings](warnings.md)). Set the
tool's `cutter` to `{ "v_bit": { "included_angle_deg": 60 } }` for this
check to apply.

An engraving that should get wider where the shape is wider, as carved
lettering does, is a V-Carve, not an Engrave.

## Warnings

| Code | Meaning |
| --- | --- |
| `engrave_path_degenerate` | A target path has fewer than two distinct points (a single point, or every point in the same place). There is nothing to trace, so it is skipped. Delete it from the drawing. |
| `engrave_deeper_than_cone` | The V-bit's cone is shorter than `depth_mm`. Use a shallower depth or a wider bit. |

An open path targeted by an Engrave is not reported as `open_profile`:
Engrave cuts it. An `on_line` [Profile](profile.md#open-paths) cuts open
paths too, with a ramp entry and tabs; Engrave plunges and has no tabs,
but can cut successive passes back and forth (`alternate_direction`).

The errors are those of every operation: a missing feed, plunge rate or
step-down, a depth below the stock bottom plus the allowance, and a cut
whose bit would reach past the stock's edge.
