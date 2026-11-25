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
  "enter": "auto",
  "leave": "auto",
  "machining_order": "full_depth_first",
  "through_cut_allowance_mm": 0.0,
  "safe_z_mm": 5.0
}
```

## In the app

Select the shapes, closed or open, then choose **Add ▸ Engrave** on the
Operations step
([Operations in the app](operations.md#adding-an-operation)). Engrave is
one of the two kinds the menu offers for an open path; the other is a
Profile.

![The plaque's Border and squiggle Engrave in the Inspector: Targets 2 shapes, Tool 1/8 end mill (T2), Depth 1 mm; the Depth group open with Step-down 1 mm and Through-cut allowance 0 mm; the Strategy group open with Alternate direction unticked and Enter at and Leave at both Auto; the Order group open with Machining order Full depth first; and the Heights group open with Safe Z 15 mm; on the Viewport the red cut follows the border and the squiggle](figures/app/op-engrave.png)

The editor's rows, and the field of the project file each one sets:

| Group | Row | Field |
| --- | --- | --- |
| **Setup** | **Targets** | `targets` |
| | **Tool** | `tool` |
| | **Depth** | `depth_mm` |
| **Depth** | **Step-down** | `step_down_mm`. Empty is `null`, the tool's |
| | **Through-cut allowance** | `through_cut_allowance_mm` |
| **Strategy** | **Alternate direction** | `alternate_direction`. Not shown while **Enter at** or **Leave at** is other than Auto |
| | **Enter at**, **Leave at**: Auto, Percentage along, From the start, From the end, Near a point | `enter`, `leave` ([Where the bit enters and leaves](#where-the-bit-enters-and-leaves)) |
| | **Percentage** | `enter.fraction.length_fraction` (and `leave.…`), as a percentage |
| | **Distance** | `enter.from_start.distance_mm` or `enter.from_end.distance_mm` (and `leave.…`) |
| | **Enter on the view**, **Leave on the view**: *Pick on the view* | `enter.near`, `leave.near`: the next click on the Viewport |
| | **Enter X**, **Enter Y**, **Leave X**, **Leave Y** | `enter.near.x_mm`, `enter.near.y_mm`, `leave.near.x_mm`, `leave.near.y_mm` |
| **Order** | **Machining order**: Full depth first, Pass first | `machining_order` |
| **Heights** | **Safe Z** | `safe_z_mm` |

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
| `enter` | Where the bit goes into each path: `"auto"`, the path's own end; `{ "fraction": { "length_fraction": 0.25 } }`, a fraction of its length from its first point, 0 to 1; `{ "from_start": { "distance_mm": 10 } }` or `{ "from_end": { "distance_mm": 10 } }`, a distance from its first or back from its last point, 0 or more; or `{ "near": { "x_mm": 40, "y_mm": 20 } }`, its point nearest that one. See [Where the bit enters and leaves](#where-the-bit-enters-and-leaves). From format version 31 (version 30 had `enter_fraction`). |
| `leave` | Where the bit comes out of each path, the same way. `"auto"` is where the cut ends of itself. |
| `machining_order` | `"full_depth_first"` or `"pass_first"`. See [Machining order](machining-order.md#machining-order). |
| `through_cut_allowance_mm` | How far below the stock bottom a cut as deep as the stock goes, as for [Profile](profile.md#cutting-through). Engraving rarely needs it; leave it at 0. |
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
lifts to safe Z, rapids back to the start and down to 1 mm above the floor
the pass before cut, and plunges from there, so every pass runs
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

### Where the bit enters and leaves

A path is normally entered at one end and left at the other, and the
plunge leaves its mark there. **Enter at** and **Leave at** move the two
to points of your choosing along the path. Each is one of:

| Choice | The point | In the file |
| --- | --- | --- |
| **Auto** | An end of the path, as without it | `"auto"` |
| **Percentage along** | That percentage of the path's length from its first point, 0 to 100 | `{ "fraction": { "length_fraction": 0.25 } }` |
| **From the start** | That distance along the path from its first point | `{ "from_start": { "distance_mm": 10.0 } }` |
| **From the end** | That distance back along the path from its last point | `{ "from_end": { "distance_mm": 10.0 } }` |
| **Near a point** | The point of the path nearest the one given, typed as X and Y or clicked on the view | `{ "near": { "x_mm": 40.0, "y_mm": 20.0 } }` |

Take a straight line 40 mm long with **Enter at** 25 % and **Leave at**
75 %, or **From the start** 10 mm and **From the end** 10 mm, which are
the same points. The bit plunges a quarter of the way along, cuts back to
the start, runs the whole line to its far end, and comes back to the
three-quarter point, where it lifts:

| Enter at | Leave at | The bit goes |
| --- | --- | --- |
| 25 % | 75 % | 25 → 0 → 100 → 75 |
| 75 % | 25 % | 75 → 100 → 0 → 25 |
| 25 % | Auto | 25 → 0 → 100 |
| Auto | 75 % | 0 → 100 → 75 |
| 100 % | Auto | 100 → 0: the path run from its other end |
| From the end 0 | Auto | the same: 0 back from the end is the end |

- **The whole path is still cut.** The stretch before the entry and the
  stretch after the exit are cut twice, once each way.
- The route is the shorter of the two that cover the path: out to the
  nearer end first.
- **Every pass** enters and leaves at the same two points. The bit lifts
  at the exit and comes back to the entry for the next pass, so
  **Alternate direction**, which would plunge at both, is not used and
  its row is not shown.
- With several paths in one Engrave, the point applies to each: the same
  percentage or distance along each, or, for **Near a point**, each
  path's own point nearest the one given. A point between two parallel
  lines enters each at the place across from it.
- A **distance longer than a path** is that path's far end: from the
  start, its last point; from the end, its first. The path is still cut
  whole, and `check` and the Warnings drawer say so,
  `engrave_point_past_end` ([Warnings](warnings.md)), naming
  the path and its length, with the glyph on the **Distance** field.
  Choose **Percentage along** to put the point at the same share of
  paths of different lengths.
- **Near a point** takes the nearest point however far the point is from
  the path; where two are as near, the first along the path.
- On a **closed** path the first point is also the last. **Enter at** is
  where the loop starts and ends, and **Leave at** carries the cut on
  past that point, cutting that stretch twice. **From the end** counts
  back round from the start.
- The first point of a path is where it was drawn from. Turn on **Start
  points** in the Viewport's **Layers** to see where each cut begins.

To pick the point on the view, choose **Near a point**, press *Pick on
the view* beside **Enter on the view** (or **Leave on the view**) and
click on the Viewport: the point under the click is written to X and Y,
one undo step, and the toolpath preview shows the new plunge. Choosing
**Near a point** first puts the point at the stock's centre.

A percentage or a distance field takes a sum, so `100/4` is a quarter
([Formulas](keyboard.md#formulas)). Choosing **Percentage along** starts
at 0 % for **Enter at** and 100 % for **Leave at**, a distance at 0: the
path's own ends, until you type another.

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
lettering does, is a [V-Carve](v-carve.md), not an Engrave.

## Warnings

| Code | Meaning |
| --- | --- |
| `engrave_path_degenerate` | A target path has fewer than two distinct points (a single point, or every point in the same place). There is nothing to trace, so it is skipped. Delete it from the drawing. |
| `engrave_point_past_end` | **Enter at** or **Leave at** is a distance from an end longer than this path. The bit enters or leaves it at its far end and still cuts it whole. Type a shorter distance, or use a percentage. |
| `engrave_deeper_than_cone` | The V-bit's cone is shorter than `depth_mm`. Use a shallower depth or a wider bit. |

An open path targeted by an Engrave is not reported as `open_profile`:
Engrave cuts it. An `on_line` [Profile](profile.md#open-paths) cuts open
paths too, with a ramp entry and tabs; Engrave plunges and has no tabs,
but can cut successive passes back and forth (`alternate_direction`).

An Engrave never warns `toolpaths_overlap`: it cuts every line as drawn,
so lines that cross or meet, as in lettering, a hatch or a border, are the
design, not two shapes drawn too close. An Engrave inside a part that an
earlier Profile cut free warns `cut_inside_freed_part`, for an open path
as for a closed one, when all of the path lies inside the freed outline.
Inside a part that the Profile cut through round with tabs, an Engrave
deeper than half the stock warns `cut_inside_tab_held_part`: the part is
held only by them. A shallow one there is quiet.

The errors are those of every operation: a missing feed, plunge rate or
step-down, a depth below the stock bottom plus the allowance, and a cut
whose bit would reach past the stock's edge.
