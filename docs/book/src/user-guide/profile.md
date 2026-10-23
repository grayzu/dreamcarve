# Profile operation

A **Profile** cuts along the outline of closed shapes: to cut a part out of
the stock, or to cut a hole through it. An `on_line` Profile also cuts
along open paths ([Open paths](#open-paths)). To follow a line with the
bit's centre on it without an entry or tabs, use [Engrave](engrave.md).

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
  "tabs": {
    "placement": { "kind": "count", "count": 4 },
    "width_mm": 8.0,
    "height_mm": 3.0,
    "shape": "rect",
    "on_holes": false,
    "avoid_corners": true
  },
  "machining_order": "full_depth_first",
  "entry": { "ramp": { "angle_deg": 15.0 } },
  "lead": null,
  "corner_relief": "none",
  "last_pass": null,
  "start": "auto"
}
```

This is the Profile `init` writes, with `depth_mm` and
`through_cut_allowance_mm` set for 11.8 mm stock.

## Settings

| Field | Meaning |
| --- | --- |
| `id` | The operation's id, shown in messages. |
| `name` | Your name for it, written in the program. |
| `enabled` | `false` leaves the operation out of the program, with a warning. |
| `targets` | The ids of the instances to cut. `init` lists every instance with a closed outline. |
| `tool` | The id of the tool to cut with. |
| `side` | `"outside"`, `"inside"`, or `"on_line"`; for open paths also `"left"` or `"right"`. See [Side](#side). |
| `depth_mm` | How deep to cut, measured down from the stock top. **Required:** `init` leaves it `null`, and export refuses until you set it. To cut through, use the stock's `thickness_mm` plus `through_cut_allowance_mm`. |
| `step_down_mm` | The most to cut in one pass. `null`, as `init` writes it, uses the tool's `step_down_mm`; if both are `null`, `check` reports the tool's as not set and export refuses. More than the tool's diameter is a warning. |
| `direction` | `"climb"` or `"conventional"`. See [Direction](#direction). |
| `through_cut_allowance_mm` | How far below the stock bottom the cut may go. 0 by default. See [Cutting through](#cutting-through). |
| `safe_z_mm` | The height above the stock top for moves between cuts. Must be more than 0. |
| `tabs` | A tab request: tabs to place on every outline this Profile cuts, or `null` for none. The project's placed tabs, in its own `tabs` list, hold the part whatever this says: every enabled Profile that cuts a tab's outline rises over it. See [Tabs](tabs.md). |
| `machining_order` | `"full_depth_first"` or `"pass_first"`. See [Machining order](#machining-order). |
| `entry` | How the bit reaches each pass depth: `"plunge"`, a ramp, or a helix. `init` writes a 15° ramp. See [Entry](#entry). |
| `lead` | An arc onto the outline at the start of each pass and off it at the end, or `null` for none. See [Leads](#leads). |
| `corner_relief` | `"none"` (as `init` writes it), `"dogbone"` or `"t_bone"`: a notch in each corner the round bit cannot reach. See [Corner relief](#corner-relief). |
| `last_pass` | `null` (as `init` writes it), or a last pass allowance: every pass but the last leaves a thin skin on the wall, and the last pass cuts it off at full depth. Inside, Outside, Left and Right; not On line. See [Last pass](#last-pass). |
| `start` | Where each closed outline's cut starts: `"auto"` (as `init` writes it), `"first_vertex"`, or `{ "near": { "x_mm": …, "y_mm": … } }`. See [Start point](#start-point). |

## Side

The bit has a width, so its centre cannot follow the line you drew. The
side says where the bit's edge runs:

- `"outside"`: the bit runs outside the outline, offset by half its
  diameter. The part keeps the size you drew. Use it to cut a part out.
- `"inside"`: the bit runs inside the outline. The hole has the size you
  drew. Use it to cut a hole or an opening.
- `"on_line"`: the bit's centre follows the line. The cut is one bit wide,
  centred on the line.
- `"left"` and `"right"`: for an open path only. The bit runs beside the
  path, on its left or its right looking from its first vertex to its
  last. See [Left and right](#left-and-right).

Inside and outside are measured from the part, not from each shape. A shape
inside another is a hole in it, so on a ring with an outside Profile, the
outer circle is cut on its outside and the inner circle on its inside. Both
edges of the ring keep the size you drew.
[Shapes, holes, and islands](shapes-and-holes.md) has the rules.

This is decided among one operation's targets only. A hole cut by an
operation of its own is a part to that operation, so cut it `"inside"`.
`tabs place` puts no tabs on holes unless you ask for them, so one
operation can cut a part and its holes: see [Tabs](tabs.md#parts-with-holes).

A 100 × 50 mm rectangle cut outside with a 6.35 mm bit gives a tool path
106.35 × 56.35 mm across its centre line; cut inside, 93.65 × 43.65 mm.

Offsetting a corner outward rounds it, so an outside path has a small arc at
each corner of the outline. A circle or circular arc in the drawing stays
an arc, and its path is the arc offset by the bit's radius. Other curves,
such as ellipses and Bézier curves, reach DreamCarve as many short straight
lines, so an outside cut of one is many short lines joined by tiny arcs
(`G1` and `G2` moves in turn) until
[arc fitting](project-file.md#arc-fitting) writes runs of them as arcs.

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

Climb keeps the material you keep on the bit's right as it moves;
conventional keeps it on the bit's left.

| The bit runs | Climb | Conventional |
| --- | --- | --- |
| Outside the outline it cuts | Clockwise | Counter-clockwise |
| Inside the outline it cuts | Counter-clockwise | Clockwise |
| `"left"` of an open path | From its first vertex | From its last vertex |
| `"right"` of an open path | From its last vertex | From its first vertex |
| `on_line` along an open path | From its last vertex | From its first vertex |

Directions are seen from above, with Y pointing away from you. An outside
Profile runs outside a part's outline and inside its holes. An `on_line`
cut counts as outside on a part's outline and inside on a hole.

Beside an open path, `"left"` or `"right"`, the part is on the far side
of the line from the bit, so climb and conventional mean what they mean
on an outline. With `"left"` the part is on the bit's right as it runs
from the first vertex to the last, so that is climb; `"right"` is the
mirror.

An open path cut `on_line` is a slot: both sides of the bit cut, so
there is no climb or conventional. There the direction only picks the
end the cut starts at, as VCarve does:
"Conventional starts cutting at the Start point" and "Climb cutting
starts cutting at the end point".

## Machining order

`machining_order` says how the passes of several shapes are interleaved:
`"full_depth_first"` (the default, and how `init` writes it) cuts each
shape to full depth before the next; `"pass_first"` cuts each pass on
every shape before the next, deeper pass. Shapes are visited nearest
first from the first one drawn, and a shape is always cut before any shape
that encloses it: a hole before its outline, a part inside a hole before
that hole, so a part is freed only by its last contour. The same cuts are
made either way, and an operation with one shape cuts exactly the same
under both. See [Machining order and operation
order](machining-order.md).

## Entry

`entry` says how the bit gets down to the depth of each pass at the start
of an outline (VCarve's "Ramps"; Fusion's "Ramp Type"). `init` writes a
15° ramp; a project upgraded from format version 7 keeps `"plunge"`, so it
cuts exactly as before.

- `"plunge"`: straight down at the tool's plunge rate, from 1 mm above the
  stock or the floor already cut there (the bit rapids that far). A bit that does
  not cut well at its centre, as most do not, takes this hard, and it
  leaves a mark on the wall where it went in.
- `{ "ramp": { "angle_deg": 15.0 } }`: a zigzag, going down `angle_deg`
  from the horizontal until it reaches the pass depth.
- `{ "helix": { "diameter_fraction": 0.8, "angle_deg": 5.0 } }`: a spiral
  down, `diameter_fraction` of the bit's diameter across (5.08 mm for a
  6.35 mm bit at 0.8), at `angle_deg`, then one flat turn at the pass
  depth, then on to the start of the outline. Inside and Outside only.

The angle must be more than 0 and at most 45 degrees, the fraction more
than 0 and at most 1.

**Where it goes.** The ramp or helix is in material the operation cuts
away anyway, never in the part:

- **Inside:** in the hole, inside the tool-centre path.
- **Outside:** in the waste around the part, kept on the stock, out of
  every other part this operation cuts and out of the path the bit takes
  round it. A hole in an Outside part is entered inside the hole.
- **On line:** the bit removes only the width of its own cut, so the ramp
  runs back and forth along the line itself, from the start of the outline
  forward three bit diameters (less on a shorter outline) and back. Curves
  are followed in short straight moves, 0.01 mm from the curve at most
  (the project's `flatten_tolerance_mm`); one too short for the program
  to write, as where a leg ends just past the end of one, is joined to
  the move before it. When the depth is a hair over a whole number of
  legs, the last two legs share what is left, each at `angle_deg`. Once
  at depth the bit goes back to the start and cuts the whole outline, so
  the ramped stretch is cut again at full depth. A helix is an error on
  On line: `check` reports it, and export refuses.

An Inside or Outside ramp starts at the start of the outline and runs into
the waste along the longest straight line that fits, at most three bit
diameters, stopping a tenth of the bit's diameter short of anything it must
not touch; then it comes back. A helix is placed next to the start of the
outline, where it fits with the same margin. A climb cut turns its helix
the way the outline runs: counter-clockwise inside a hole, clockwise
round a part; conventional turns it the other way.

The bit rapids straight down from safe Z to 1 mm above the stock top,
and the first pass's ramp or helix starts there. Each pass after starts
where the lap before it ended, at the start of the outline on the floor
of that pass, so the bit does not ramp down through air.

**Tabs.** On every pass below the tab top, the ramp or helix keeps clear
of the tabs, and an On line ramp stops short of the first tab. When a tab
covers the start of the outline, or sits so close after it that the ramp
or helix has no room, the cut starts somewhere else: in the middle of the
longest stretch between tabs where the entry fits, then the next longest,
and so on. That start is used on every pass, and the tabs stay where you
put them: their distances are still measured from where the outline
started. Only when no stretch has room (or on an open path, whose start
cannot move) does the bit plunge there, as without an entry, and
`entry_fallback` says so. A Profile with a [lead](#leads) starts where
the lead arrives instead, which is already off the tabs.

**Every ramp stays in the trench.** An Inside or Outside Profile cuts
only its own trench and the line its ramp runs on; the waste beside them
stands full height. So each pass after the first ramps along the line
the pass above ramped on, never a new one. When the tabs stand in the
way of that line below the tab top, the ramp runs along it only as far
as the tabs leave room. When there is no room at all, or the pass above
did not ramp there (it plunged or went down by helix, or it cut another
path, as the passes before a [last pass](#last-pass) do), the pass plunges
at its start, off the tabs, one step down from the floor of the pass
above, and `entry_fallback` says so (`no_room`). A ramp also runs no
farther along its line than the pass above cut below the stock top. The
first pass starts its ramp 1 mm above the stock, so its first legs cut
air, and on a shallow ramp the trench it leaves can be shorter than the
next pass needs; that pass then zigzags back and forth within the trench
instead of running past its end into uncut stock. A pass that would go
down less than 0.05 mm plunges that little instead of ramping, since its
one leg would run the whole line (`too_small`). The export's safety
check refuses a ramp that leaves the trench (rule 12 in
[Safety](safety.md)).

**Seeing where the path starts.** `dreamcarve check part.dcv --preview
part-preview.svg` draws the toolpath ([Toolpath
preview](overview.md#toolpath-preview)). Each pass starts at a circle the
size of the bit, with a tick pointing the way the cut goes, and the tabs
show as heavy marks, so you can check where a moved start or an
`at_distances` tab landed before you cut.

**Feed rate.** A ramp or helix moves at the tool's feed rate, except that
its speed straight down is never faster than the plunge rate: when
`feed × sin(angle)` would exceed the plunge rate, the feed of each
descending move is reduced until it does not. A 15° ramp with a 1500
mm/min feed and a 500 mm/min plunge runs at the full 1500 (its speed down
is 388 mm/min); with a 100 mm/min plunge it runs at 386 mm/min.

**When it does not fit.** A helix that does not fit at full size is made
smaller, halving up to three times, and `helix_diameter_reduced` says how
small. A helix that does not fit at all falls back to a ramp, and a ramp
that does not fit, as in a hole barely wider than the bit (a 7 mm hole
with a 6.35 mm bit), falls back to a plunge. A helix too small for the
program to write, whose moves would each fall less than the last decimal
place or run less than about one and a half of it sideways, falls back
to a ramp too, and an On line ramp that small (every leg falling less
than the last decimal place) to a plunge. Each fallback warns `entry_fallback`, naming the shape
and both entries. A shape that can only be plunged warns on every export
until its `entry` is `"plunge"`.

## Open paths

An open path (a line, an arc, a polyline that does not return to its
start) has no inside or outside, so an Inside or Outside Profile skips it
with `open_path_skipped`. An `on_line` Profile cuts it, as VCarve's
Profile "On" cuts an open vector: a slot one bit wide centred on the
line.

- Each pass runs the whole path at the pass depth, from its first vertex
  to its last under conventional, or from its last to its first under
  climb. Then the bit lifts to safe Z and goes back to the start
  for the next pass.
- A ramp runs back and forth along the path from the start, as on a
  closed outline, turning at the path's end when the path is shorter than
  three bit diameters. A later pass first comes straight down the slot
  the pass above cut, at the plunge rate, to that pass's depth, then
  ramps from there.
- [Tabs](tabs.md) placed at distances (`tabs place --at`, or a
  request's `at_distances`) are measured from the path's first vertex as
  you drew it, whichever end the cut starts at, and stop at the path's
  ends. A tab over the start of the cut makes the bit plunge there with
  `entry_fallback`: an open path's start cannot move.
- There is no lead (`lead_dropped`, as for any On line Profile) and no
  corner relief (`relief_skipped`), and an open path frees nothing.
- An open path drawn twice in the same place, in either direction, is
  cut once, and the copy warns `duplicate_shape_dropped`, as a closed
  shape drawn twice does.
- An open path is a shape of its own for
  [shapes too close together](#shapes-too-close-together): one that comes
  within half a bit of another shape's line, or crosses it, warns
  `toolpaths_overlap`. So does one drawn outside any part that comes
  within half a bit of itself, far along it, as a tight spiral or a
  hairpin does: the warning names the path twice. One that lies wholly in a part, as a slot cut into
  it, is not compared with that part's own lines there; instead, one that
  runs closer than a bit's width beside them stops the operation (see
  [Bands narrower than the bit](#bands-narrower-than-the-bit)). Cut after
  a Profile that freed the part it lies in, it warns
  `cut_inside_freed_part`; after one that left that part held only by
  tabs, `cut_inside_tab_held_part` when it cuts deeper than half the
  stock.

A 100 mm line cut `on_line` 6 mm deep in 2 mm passes with a 15° ramp and a
6.35 mm bit: each pass ramps out 19.05 mm along the line and back, then
cuts the 100 mm to the line's far end, and lifts.

### Left and right

`"left"` and `"right"` cut beside an open path, as VCarve's Profile cuts
"to the Left" or "to the Right" of an open vector. Left and right are as
you look along the path from its first vertex to its last, the way you
drew it. The importers keep the drawing as you see it, so left is the
left you see in your drawing program; draw the path the other way and
the sides swap. The bit runs one radius to that side, so its edge follows
the line: the side it runs on is waste, and the other side is the part,
which keeps the line you drew. Use it for an edge that is not a closed
outline, such as a cut across the stock that trims it, or one edge of a
part.

A 100 mm line drawn from (20, 20) to (120, 20) and cut `"left"` with a
6.35 mm bit: the bit's centre runs at Y 23.175, from X 20 to X 120 under
climb, and the material below the line is kept. Cut `"right"`, it runs
at Y 16.825, from X 120 to X 20 under climb, and the material above is
kept.

- **Turns.** On the outside of a turn the bit rounds the corner, as an
  outside cut does; on the inside it stops where its two passes meet, so
  that corner keeps the bit's radius. A curve tighter than the bit's
  radius on the cut side leaves no room: that stretch is not cut, and a
  path with nothing left warns `tool_too_large`.
- **Ends.** The cut ends square: the bit's centre stops on the line
  square to the path through its end vertex. Its edge touches the end
  vertex and reaches one radius past it on the cut side only, so the
  part side is never touched past the end. VCarve's manual does not say
  how its open cut ends; this keeps the part as drawn.
- **A short end segment.** Inside a turn the bit cannot cut beside the
  start of the next segment: it stops where its two passes meet, which
  is the bit's radius times `cot(θ/2)` along each segment, for a turn
  whose two segments meet at θ (3.175 mm at 90° with a 6.35 mm bit,
  11.8 mm at 30°). When the path's last segment, or its first, is
  shorter than that, the bit has no room beside it: the cut beside the
  segment before it would run on past the line through the end vertex,
  into the part. That is an error, and nothing is exported:

  ``operation `o1` cuts beside `Edge` (instance `i1`, profile 0), but the
  segment ending at its vertex 2 is 2.000 mm long, too short for its
  6.35 mm tool to cut beside it: the cut would reach past the path's
  end into the part; ...``

  Vertices count from 0 in the order you drew them. Lengthen or remove
  that segment, cut the path on its other side, or use a smaller bit. A
  30° V drawn with a 2 mm last segment is refused cut inside and cut
  round its outside.
- **Direction** is climb or conventional milling, as on an outline (see
  [Direction](#direction)): `"left"` climb and `"right"` conventional
  start beside the first vertex, `"left"` conventional and `"right"`
  climb beside the last.
- **Last pass.** A [last pass](#last-pass-beside-an-open-path) works
  here too.
- **Ramps and tabs** are as for `on_line` above, along the path the
  bit's centre follows: a ramp runs back and forth from the start; tabs
  are measured from the end beside the first vertex, whichever end the
  cut starts at. A `helix` entry is an error: the cut removes only its
  own width. There is no lead (`lead_dropped`) and no corner relief
  (`relief_skipped`), as VCarve gives an open profile none.
- **Closed shapes** are not cut: an outline has no first vertex you can
  see, so its left is not shown in the drawing. Each warns
  `side_needs_open_path`; cut it in an Inside, Outside or On line
  operation.
- **Lines too close.** The bit must not cross another of the operation's
  lines, nor reach the strip one bit radius wide that a line keeps on its
  part side, past its ends included: the strip ends square at each end
  vertex, like the cut. A cut that would is an error, and nothing is
  exported:

  ``operation `o1` cuts beside `Edge` (instance `i1`, profile 0) and
  `Edge` (instance `i1`, profile 1), but a cut comes within the 6.35 mm
  tool's radius of one of them or of the material it keeps, ...``

  Two lines 4 mm apart, or a line drawn twice in opposite directions
  (each copy keeps what the other cuts), are refused. So is a part drawn
  as four separate lines and cut on its inside: each line's square end
  runs onto the next line. Join lines that meet into one path, so the
  corner is turned, not ended. The same four lines cut on their outside
  are cut: the part's corners come out sharp, and a little waste is left
  beyond each one, where neither square end reaches. A line drawn twice the same way is cut once, with
  `duplicate_shape_dropped`. The same check covers one line against
  itself, such as a spiral whose cut side faces the part side of the turn
  beside it.


## Leads

Without a lead, each pass starts and ends on the outline itself: the bit
comes down (or ramps in) on the line it is about to cut and starts
moving sideways from a standstill against the wall, and at the end it
stops against the wall and lifts. Both leave a mark. `lead` moves the
start and the end off the wall onto short arcs (VCarve's "Leads",
circular; Fusion's "Lead-In" and "Lead-Out"):

```json
"lead": { "radius_mm": 3.0, "sweep_deg": 90.0, "lead_out": true, "overcut_mm": 1.0 }
```

| Field | Meaning |
| --- | --- |
| `radius_mm` | The arc's radius. More than 0. A larger arc takes up the cut more gently. |
| `sweep_deg` | How much of a circle the arc turns through: 90 is a quarter circle. More than 0, at most 180. |
| `lead_out` | `true` adds the matching arc off the wall at the end of each pass; `false` ends on the outline. |
| `overcut_mm` | How far to keep cutting past the start before leading out, so the start and the end of the cut overlap. 0 or more; at most one more lap is cut. |

Each pass then:

1. comes down (by the [entry](#entry)) at the far end of the lead-in arc,
   in the waste beside the outline: with a 90° sweep that is `radius_mm`
   out from the bit's path;
2. moves along the arc at the cutting feed and arrives on the outline
   tangent to it, already moving;
3. cuts the whole outline from there, then `overcut_mm` more;
4. leaves along the lead-out arc, tangent to the outline, and lifts.

The arcs are written as `G2`/`G3` arcs at the pass depth. Because every
pass starts at the lead-in, the bit lifts to safe Z between passes and
comes down at the same place each time, through the hole the pass above
cut there.

**Where the arcs go.** In the same waste the entry uses (see
[Entry](#entry)): inside the hole for Inside, round the part for Outside,
on the stock, out of every other part and its cut. The arc touches the
outline only where it joins it; elsewhere it keeps a tenth of the bit's
diameter clear of anything it must not touch. Below the tab top it keeps
off the tabs too. The tabs stay where they are along the outline.

**Where the cut starts.** Not where the outline happens to begin. Every
edge of the bit's path is tried at its start, middle, quarter and three
quarters; the place with the least turning within one bit diameter
either way wins, so a rectangle starts partway along a side, away from its
corners; among places as straight as each other, the one nearest where
the previous outline ended; after that, the first along the outline.
With a [start point](#start-point) set, the lead arrives there or not at
all.

**When it does not fit.** The arc is tried at `radius_mm`, then at ¾, ½
and ¼ of it, each at `sweep_deg` and then at 60° and 45° (when smaller).
The largest that fits is used, and `lead_shrunk` gives the size cut. When
none fits, as in a 7 mm hole with a 6.35 mm bit, the outline is cut
without a lead and `lead_dropped` says so; when only the tabs are in the
way, the passes below the tab top go without one. A lead-out that does
not fit where the cut ends is left off (`lead_dropped`, reason `exit`).

**On line.** An On line Profile removes only its own cut, so there is no
waste beside it for an arc: a `lead` there is not cut, and `lead_dropped`
says so once for the operation. The same goes for `"left"` and
`"right"`, which cut open paths: VCarve gives an open profile no lead.

## Corner relief

A round bit leaves a radius in every corner it cannot get into: the
corners of a hole cut **inside**, and the inward-pointing corners of a
part cut **outside** (the inside corner of an L). A square part does not
seat in such a hole, and a square slot does not take such a part.
`corner_relief` cuts a short notch into each of those corners after the
outline is cut, as on a [Pocket](pocket.md#corner-relief); the names are
VCarve's ("Create Fillets": Dog-Bone and T-Bone).

- `"dogbone"`: the notch runs along the corner's bisector until the bit
  just touches the corner, taking the same small bite out of both walls.
  The usual choice for joints, where neither wall shows.
- `"t_bone"`: the notch runs along the corner's longer edge (on equal
  edges, the one nearer the X axis) until the bit reaches the corner,
  slotting that wall a bit's radius deep and leaving the other straight.
  Use it to keep a visible face clean.

An outside Profile's outward corners need no relief: the bit already cuts
round them, so a rectangle cut outside gets no notches at all. An On line
Profile has no waste side to put a notch in: it skips the relief with
`relief_skipped`. So does a `"left"` or `"right"` Profile: an open path
has no inside or outside corner.

Each notch plunges where the outline's path turned that corner and steps
down by the **tool's** `step_down_mm` to the operation's depth. The
notches are cut among the outline's own passes, nearest corner first: a
notch level comes right after the pass that cut down to it, so the bit
plunges into the slot that pass opened. When that pass ended on the
corner's plunge point, as it often does on a rectangle, the bit goes
straight on into the notch from where it is, up or down in the slot it
has just cut, without lifting to safe Z first. A corner under a [tab](tabs.md)
is skipped with `relief_corner_not_cut`: the path rises over the tab
there, so the notch would plunge into uncut material. Edges shorter than
the bit is wide skip their corner with `relief_corner_too_tight`, and a
tool with no `step_down_mm` skips the relief with `relief_skipped`. So
does a corner whose notch would come nearer another wall than the bit's
radius (`relief_corner_too_tight`): a T-bone reaches the bit's radius
past its wall, so in a hole whose part is thinner than that beyond the
wall it would cut through the part.

On a through cut the last pass frees the part (or the slug of a hole),
so its notch levels are cut from inside that last lap instead, never
after it. When the lap reaches a corner at full depth, the bit rises in
the slot it has just cut to the first of those levels, cuts the notch out
and back, steps down in the same slot to the next level, and carries on
along the lap. It never plunges into uncut material, so a ramp or helix
[entry](#entry) is honoured, and the rest of the lap still holds the
piece while each notch is cut, so tabs are not needed for the notches.
Tabs still hold the part through the last pass itself, as without relief.

An inside Profile of a 40 × 30 mm rectangle with a 6.35 mm bit and
`"dogbone"` gets four notches, each ending 3.175 mm from its corner along
the bisector, the same four a Pocket of that rectangle gets; an outside
Profile of an L-shaped part gets one, in the inside corner of the L.

## Last pass

Every pass of a Profile cuts the finished wall, so each one leaves a faint
step where it ended, and the bit is pushed off the line by the full depth
of material on every pass. `last_pass` is VCarve's **Last Pass
Allowance**: the passes before the last run a little farther from the
part, the **allowance**, and the last pass runs on the part's path at the
full depth. It takes an even skin off the whole wall in one go, so the
wall is cut by one pass from top to bottom and comes out clean and to
size. (Fusion calls the same thing radial stock to leave with a
finishing pass.)

```json
"last_pass": {
  "allowance_mm": 0.5,
  "thickness_mm": null,
  "reverse": false
}
```

| Field | Meaning |
| --- | --- |
| `allowance_mm` | How much every pass but the last leaves on the wall, in mm. More than 0 and at most a third of the bit's diameter (2.117 mm for a 6.35 mm bit), as VCarve advises: the last pass cuts the full depth, so keep its skin thin. 0.2 to 0.5 mm is usual. |
| `thickness_mm` | `null` for the usual depth passes, the last of which is the last pass. A number sets how deep the last pass cuts (VCarve's "Set Last Pass Thickness"): the other passes step down to `depth_mm` less it, then the last pass cuts the whole depth. More than 0 and at most `depth_mm` and the step-down in effect (the operation's `step_down_mm`, else the tool's): the last pass is never thicker than a pass. |
| `reverse` | `true` cuts the last pass the other way round (VCarve's "Reverse direction"): conventional after climbing passes, or climb after conventional ones. Each has its finish; try both on scrap. |

All three keys are needed. An outside Profile of a 60 × 40 mm rectangle,
12 mm deep in 4 mm passes with a 6.35 mm bit and a 0.5 mm allowance, cuts
its first two passes 0.5 mm farther out (a 67.35 × 47.35 mm path across
its centre line) and the third on the 66.35 × 46.35 mm path it cuts with
no last pass. With `thickness_mm` 1 the passes are 4, 8, 11 and 12 mm
deep, and only the 12 mm pass is on the part's path. Cut inside, the
earlier passes run 0.5 mm farther into the hole instead.

**How it cuts.** The allowance stands uncut from the stock top down
beside the part, so the last pass never goes straight down at its own
start. It enters the way one more earlier pass would: where the earlier
passes ended, it goes down into their floor with the operation's
[entry](#entry), then steps across the allowance onto its own path.

- A **plunge** goes one step-down deep (or `thickness_mm` deep), into
  the floor of the earlier passes' cut.
- A **ramp** runs back and forth along the earlier passes' path, inside
  the slot they cut, at the ramp angle, and comes back to where it
  started.
- A **helix** goes back into the bore the earlier passes' helix cut and
  helixes down one more step.
- Then, at the full depth, the bit moves at the feed rate straight across
  the allowance onto the last pass's path, which starts beside where the
  earlier passes start, and cuts its lap. The step is short: the
  allowance, or the wedge in a corner.
- With a [lead](#leads), the earlier passes are led in beside where the
  last pass's lead comes in, so the bit goes down at their lead's start
  and steps across to the last pass's. If that is too far to step (the
  earlier passes' lead had to be made smaller), the last pass's lead-in
  is left off and `lead_dropped` warns; its lead-out is kept.

- If the last pass's path starts on a tab, or its step would pass near
  one, the step runs above the tab, at the height the lap leaves it. On a smooth tab's
  slope, where the earlier passes ran higher, it steps across at their
  height and comes down onto the slope.

So no plunge goes deeper into uncut material than a step-down, whatever
the entry. One case is left: with a tab taller than the step-down, the
last pass still comes down the tab's height in one move at a rect tab's
far end, or at a start low on a smooth tab's slope
([#348](https://github.com/grayzu/dreamcarve/issues/348)). Keep tabs
no taller than the step-down when you use a last pass. (Changed in
#339: the last pass plunged at its own start
from 1 mm above the stock down to the earlier passes' floor, through the
allowance, which with a plunge entry meant the full depth.)

**With tabs**, the tabs are laid on the last pass's path, so they are
where they would be without a last pass. Each earlier pass rises over
every point where its cutter could reach the tab the last pass leaves
standing, so the farther-out cut never bites into it: on a straight edge
it rises about 1.5 mm sooner and comes down that much later with a
6.35 mm bit and 0.5 mm; round a sharp corner, where the earlier passes
run beside the next edge's tab, sooner still. A smooth tab's earlier
passes ramp up at least as high as the tab's slope wherever they reach
it. [Leads](#leads)
and [entries](#entry) are planned for each path, the earlier passes' kept
the allowance farther from the part. [Corner relief](#corner-relief)
notches are cut from the last pass only, the one that cuts the corner:
an earlier pass does not reach it, so a notch after it would plunge into
the skin.

**Round holes and inside curves.** Cutting inside a curve, the bit's
centre runs a smaller circle than the wall it cuts, so its cutting edge
moves faster than its centre: in an 8 mm hole with a 6.35 mm bit the
centre runs a circle of 0.825 mm round a wall of 4 mm, and the edge
moves 4.8 times as fast. The earlier passes cut a full slot there, as
any Profile does, but the last pass is meant to take a thin skin, and at
the feed it would take nearly five times the skin a straight wall's last
pass takes in a minute. So the last pass runs each inside arc of its path
slower, at the feed times the arc's radius over the wall's,
`feed × ρ / (ρ + r)`: 206.25 mm/min at a 1000 mm/min feed in the 8 mm
hole, 577 mm/min in a 15 mm one. Its cutting edge then moves at the feed,
and it removes a little less per minute than a straight wall's last pass.
Straight walls, outside corners and the earlier passes keep the feed, and
the lowered feed is written rounded down, never up. The feed is lowered
no further than the smallest hole the last pass cuts needs,
`feed × a / (a + r)` (136 mm/min at 0.5 mm with a 6.35 mm bit). An inside
corner rounded to less than the bit's radius plus the allowance (a 3.5 mm
corner radius with a 6.35 mm bit at 0.5 mm) needs more, so there the
edge moves faster than the feed, and `last_pass_edge_faster` says so.
Only true arcs are slowed: a curve drawn as many short lines (an ellipse
or spline from a DXF) runs at the feed
([#350](https://github.com/grayzu/dreamcarve/issues/350)).

**Sharp corners.** Where the bit's path turns a corner sharper than
90° (the points of a triangle or star cut inside, an inward V notch cut
outside, the corners of letters such as A, V, W, M and K), the earlier
passes cut both walls and stop short of where they meet. The last pass
then takes the wedge between, `a / sin(θ/2)` thick at a corner of `θ`:
2a in a 60° corner, 2.6a in a 45° one. That is allowed up to a third of
the bit's diameter, the thickest skin the allowance itself may be (or √2
times the allowance, a square corner's, where that is more). With a
6.35 mm bit that is 2.1 mm, so corners down to about 27° are cut at
0.5 mm, and down to about 11° at 0.2 mm. A square corner is cut at any
allowance.

**When it does not fit.** The earlier passes need the bit plus the
allowance on both sides wherever the last pass goes, and the last pass
may take off no more than a square corner's √2 times the allowance
(plus 0.05 mm) except in a corner as above. A drawing where that is not
so is refused, naming the shapes, and nothing is exported:

- A notch, slot, neck or small hole that the bit fits but the bit plus
  the allowance does not: the earlier passes bridge it and skip it, and
  the last pass would cut it whole, about as wide as the bit, at full
  depth. A 7 × 3 mm notch in a part cut outside with a 6.35 mm bit and
  0.5 mm is refused, though the last pass would take only 1 mm off
  there, and so is a 10 × 4 mm one with 2.1 mm.
- A tip so narrow that its wedge is thicker than a third of the bit: a
  20° tip cut inside with a 6.35 mm bit and 0.5 mm, whose wedge is
  2.9 mm. A smaller allowance helps: the same tip is cut at 0.2 mm.
- Inside, an island closer to the outline than the bit plus twice the
  allowance (a 7 mm band with a 6.35 mm bit and 0.5 mm, which needs
  7.35 mm).
- A part closer to another than the bit plus the allowance, so that the
  earlier passes would come within the bit's radius of it or run inside
  it: a part 6.6 mm from the wall of a frame's hole with a 6.35 mm bit
  and 0.5 mm. An earlier pass that comes nearer another part than the
  allowance but no nearer than the radius only cuts that part's skin,
  which its own last pass takes off anyway, and is allowed.

Make `allowance_mm` smaller, or set `last_pass` to `null`. An On line
Profile cuts with both sides of the bit and has no wall for the
allowance, so a `last_pass` there is an error; so is an allowance of 0
or less or above a third of the bit, or a `thickness_mm` of 0 or less,
deeper than `depth_mm` or thicker than the step-down. A cut of only one
pass is the last pass, on the part's path, and warns `last_pass_unused`.

### Last pass beside an open path

A [Left or Right](#left-and-right) Profile cuts one side of the bit
against the part, as an outline cut does, so it takes a last pass too,
as VCarve's Profile of an open vector does. The earlier passes run the
allowance farther to the cut side, and the last pass on the bit's usual
path beside the line.

A 60 mm line from (20, 20) to (80, 20) cut `"left"`, 12 mm deep in 4 mm
passes with a 6.35 mm bit and a 0.5 mm allowance: the first two passes
run at Y 23.675 and the third at Y 23.175, the path it cuts with no last
pass. Every pass runs from X 20 to X 80.

- **Ends.** Every pass ends square on the same lines through the path's
  end vertices, square to the path. The earlier passes are neither
  shorter nor longer than the last: none cuts past an end on the part
  side, and the earlier passes leave the allowance standing right up to
  each end, where the last pass takes it off.
- **Direction.** The earlier passes and the last run the way
  [Direction](#direction) says, from the same end. With `reverse` the
  last pass starts at the other end, where the earlier passes finished,
  and mills the other way: conventional after climbing passes, or climb
  after conventional ones.
- **Ramps, tabs and leads.** Each earlier pass ramps along its own path.
  The last pass goes down on the earlier passes' path, ramping along it
  (or plunging one step), then steps across the allowance on the line
  square to the path at its starting end, as on an outline; with
  `reverse` it goes down at the end where the earlier passes finished,
  so it does not retract first. Tabs are
  laid on the last pass's path, measured from the end beside the first
  vertex as always, and each earlier pass rises over every point where
  its cutter could reach them, as on an outline; a tab at an end of the
  path is held at its height right to that end. There is still no lead
  and no corner relief.
- **Corners.** Inside a turn the last pass takes the wedge between the
  earlier passes' two arms, as in a sharp corner of an outline: a 60° V
  cut inside is cut at 0.5 mm, a 20° one only at 0.2 mm. Round a turn it
  takes an even skin.
- **When it does not fit.** The rules above apply to the path: a U, slot
  or notch the bit fits but the bit plus the allowance does not (the
  inside of a U 7 mm wide, with a 6.35 mm bit and 0.5 mm), a tip whose
  wedge is thicker than a third of the bit, or a stretch at an end that
  the earlier passes stop short of, is refused naming the path. So is an
  end segment with room for the bit but not for the bit plus the
  allowance (see [A short end segment](#left-and-right)): the error
  says "too short for its 6.35 mm tool and its 0.5 mm last pass
  allowance". An L 3.4 mm long on its last leg, cut inside its corner,
  is cut at 0.2 mm and refused at 0.5 mm. The
  earlier passes must also keep the bit's radius plus the allowance from
  every other line of the operation and from the strip each line keeps,
  so they never take another line's skin: two lines 6.85 mm apart cut
  towards each other, which the bit alone fits, are refused at 0.5 mm
  and cut at 0.2 mm.

  ``operation `o1` cuts beside `Edge` (instance `i1`, profile 0) and
  `Edge` (instance `i1`, profile 1) with a 0.5 mm last pass allowance,
  but a pass before the last comes within the 6.35 mm tool's radius plus
  the allowance of one of them or of the material it keeps, ...``

  A part drawn as four separate lines and cut on its outside, whose
  lines meet at its corners, is cut.

## Start point

Each closed outline's cut starts somewhere on the bit's path round it, and
that is where the bit comes down, where a mark is most likely, and where
`at_distances` tabs are measured from. `start` says where (VCarve's 2D
Profile "Use vector start point" and "Start At"; Fusion's "Entry
Positions"):

| Value | Where each outline's cut starts |
| --- | --- |
| `"auto"` | As DreamCarve chooses: from the outline's first vertex, moved by a [lead](#leads) to a straight stretch, or by a ramp or helix that has no room beside a tab. `init` writes this, and files from before format version 14 open with it. |
| `"first_vertex"` | At the point of the bit's path nearest the outline's first vertex as you drew it, as VCarve's "Use vector start point" plunges at the shape's first point. Cut outside, a corner's path turns round it on an arc, and the cut starts in the middle of that arc. |
| `{ "near": { "x_mm": 0.0, "y_mm": 0.0 } }` | At the point of each outline's bit path nearest that point, in [project coordinates](project-file.md#coordinates) (the stock's bottom left corner is 0, 0). A point off the stock is fine; inch input works as for any length. |

```json
"start": { "near": { "x_mm": 0.0, "y_mm": 0.0 } }
```

With `"first_vertex"` or `"near"`, the start you set wins over the
conveniences `"auto"` allows:

- A tab request's `at_distances` count from the start, and `count` spaces
  its tabs from it: the first of four is an eighth of the way round
  ([Tabs: tab requests](tabs.md#tab-requests)). Tabs already placed stay
  where they are.
- A [lead](#leads) arrives at the start, at the largest size that fits
  there, or is left off with `lead_dropped` (reason `no_room`); it is not
  moved to where it would fit.
- A ramp or helix [entry](#entry) that has no room at the start falls
  back to a ramp, then a plunge, with `entry_fallback`, instead of the
  start moving. On a start under a tab, the first pass below the tab top
  plunges only to the tab's top there, and later passes start on the tab
  where the one before ended. Each pass gets down to its depth past the
  tab's end, as it leaves every tab (a rect tab's step down, a smooth
  tab's slope), so no pass cuts into the tab.
- A [last pass](#last-pass)'s earlier passes start at their own path's
  point nearest the same place, and a reversed last pass starts where the
  others do.

Each pass of an outline starts at the same place. Open paths ignore
`start`: their cut starts at the end the `direction` sets
([Open paths](#open-paths)). Draw the toolpath with `dreamcarve check
part.dcv --preview part-preview.svg` to see each start, a circle the size
of the bit ([Toolpath preview](overview.md#toolpath-preview)).

## Order of operations

Operations run in the order the project lists them, whichever tools they
use; DreamCarve never reorders them or groups them by tool. `check` warns
`cut_inside_freed_part` when an operation cuts inside a part an earlier
Profile without tabs has already cut free, `cut_inside_tab_held_part` when
it cuts deeper than half the stock inside a part an earlier Profile has
left held only by its tabs,
and hints
`tool_change_avoidable` when one tool's operations are split by another's.
The program is still written as listed. See [Machining order and
operation order](machining-order.md#checks-on-the-operation-order).

## What the program does

For each part, the holes are cut first and the outline last, so a part cut
through is freed only at the end. A part inside another part's hole is cut
before that hole, however deep the nesting, so it is never cut out of loose
waste. Otherwise shapes are visited nearest first, starting from the first
one drawn (see [Machining order](machining-order.md)). For each outline:

1. Rapid to safe Z, then to the start of the outline (or of its helix).
2. Go down to the first pass by the [entry](#entry): ramp, helix or
   plunge.
3. Cut one full lap at the feed rate, ending where it began.
4. Go down to the next pass the same way, from the floor of the last, and
   cut another lap, until the last pass. With a [last pass](#last-pass)
   the passes before it run on the allowance path, and the tool rapids up
   to safe Z before the last, which it reaches through the slot already
   cut. Under
   `"pass_first"` the tool instead rapids up to safe Z and goes on to the
   next shape, coming back for the next pass once every shape has had this
   one.
5. Rapid back up to safe Z.

Every move between shapes is at safe Z; the tool never travels through
material from one shape to another. With [corner relief](#corner-relief)
each pass is followed by its notches, each corner reached at safe Z
(except one where the pass ended, which is cut straight from there); on
a through cut the notches of the last pass are cut from within it, at
each corner as the lap gets there.

## Warnings

These still give a safe program. `check` and `export` print each one as
`warning: <code>: operation `op1`: <sentence>`, with lengths rounded to
0.01 mm; the [Warnings](warnings.md) chapter lists every code.

| Warning says | Meaning |
| --- | --- |
| "... is an open path" | An Inside or Outside target is not a closed outline; it is not cut. See [Open paths](#open-paths). |
| "... is a closed outline, and Left and Right are sides of an open path ..." | A `"left"` or `"right"` target is a closed outline; it is not cut. See [Left and right](#left-and-right). |
| "tool ... does not fit ..." | The bit does not fit inside the shape; that shape is not cut. |
| "... is the same outline in the same place as ..." | Two shapes are the same outline, or the same open path in either direction, in the same place; the second is not cut. |
| "tabs on ...", "the tab top is ...", "tab ... is on ..." | See [Tabs](tabs.md#warnings). |
| "there is no room for a `helix` entry ...", "the helix entry in ... is ... smaller than ..." | See [Entry](#entry), "When it does not fit". |
| "the lead arc on ... is ... smaller than ...", "there is no room for a lead-in arc ..." | See [Leads](#leads). |
| "the tool paths of ... and ... cut into each other" | Two shapes are closer than the bit needs to cut between them, so cutting one gouges the other. See below. |
| "corner relief ... is skipped", "the corner of ... is not relieved" | See [Corner relief](#corner-relief). |

### Values that look wrong

`check` and `export` also look for values that are allowed but probably a
mistake. They check only enabled operations and the tools those use. Each
finding names the operation or tool and starts with where the value is in
the file, as in `coaster.dcv:29:22:`. DreamCarve never changes the value
for you.

| Message says | Kind | Meaning |
| --- | --- | --- |
| "... cuts ... mm per pass, deeper than the ... mm diameter of tool ..." | Warning | The step-down in effect (the operation's, or else the tool's) is more than the bit's diameter. Passes that deep can break the bit. |
| "... plunges at ... mm/min, faster than it cuts ..." | Warning | The tool's `plunge_mm_min` is above its `feed_mm_min`. They may be swapped. |
| "... is set to ... RPM, faster than the 60000 RPM ..." | Warning | The spindle speed is above 60 000 RPM. GRBL and grblHAL run the spindle at its maximum instead, and with manual spindle control the speed is only a hint, so this does not stop export. |
| "... is ... mm in diameter, larger than the 50 mm ..." | Warning | The tool's `diameter_mm` is above 50 mm. Check it for a typo. |
| "feed_mm_min is ..., expected a value greater than 0 and at most 20000 mm/min" (or `plunge_mm_min`) | Error | No hobby machine moves faster than 20 000 mm/min, so the value is almost surely a typo. Export fails. |
| "... cuts with tool ..., a V-bit; an Inside, Outside, Left or Right Profile and a Pocket offset the path by half the diameter ..." | Error | With `side` `"outside"`, `"inside"`, `"left"` or `"right"`, Profile needs a flat end mill (`"cutter": "end_mill"`): it offsets the path by half the diameter, so with a V-bit, ball-nose or engraving bit the part comes out the wrong size. Export fails. With `"on_line"` the bit's centre follows the line, so any cutter is allowed. |
| "... is enabled but has no targets ..." | Error | The operation would cut nothing. Add targets or set `enabled` to `false`. |

### Shapes too close together

DreamCarve cuts each shape on its own. When two shapes in one operation are
closer than the bit needs to pass between them, the bit cutting one also
cuts into the other. With **Outside** both cuts run in the gap, so the
shapes need at least one bit diameter between them; with **On line** each
cut reaches half a bit past its line, so they need one radius. **Inside**
cuts stay within their own shapes and never warn; a band inside one shape
that is too narrow for the bit is an error instead (see below).

The warning names both shapes and the smallest gap between them, listed in
the order the shapes are cut. It also applies to a part inside another
part's hole: a coaster 1 mm from the wall of a frame's hole warns, and
to an [open path](#open-paths) cut **on line**: a line 2 mm from a part,
or crossing it, warns, as do two lines that cross. A part and its own
holes are never compared here, nor a line lying in a part with that
part's lines: cut **outside**, a hole close
to its outline leaves a thin wall, not a gouge, and cut **inside** or **on
line**, a hole too close is an error, and so is a line in a part that runs
too close beside its lines (see below). The program is still
written as drawn. Move the
shapes apart, or use a smaller bit.

### Bands narrower than the bit

Cut **inside**, a part with an island in it (a ring, a frame around a
round boss) cuts the band of waste between the outline and the island,
and keeps the material beyond both. When that band, or the gap between
two islands, is narrower than the bit anywhere, the cut along one line
would bite into the material beyond the other: a 3 mm band with a
6.35 mm bit gouges the part by 3.35 mm. That is an error, not a warning,
and nothing is exported:

``operation `o1` cuts `Ring` (instance `i1`, profile 0) and `Ring`
(instance `i1`, profile 1), but the band between them is 3.000 mm wide,
narrower than its 6.35 mm tool, ...``

Use a bit no wider than the band, widen the band, or cut the ring with a
[Pocket](pocket.md), which clears only where the bit fits. A shape the
bit does not fit inside at all is different: it is skipped with the
"tool ... does not fit ..." warning, because nothing is cut there. An
island in such a shape is still an error, since its lap would run
outside the outline.

Cut **on line**, the band between a part's outline and its hole (or
between two of its holes) is the part itself. The bit's centre follows
each line and cuts half a bit into the band beside it, so when the band
is narrower than the bit the two cuts together cut right through it,
and the part falls apart there. That is the same error: a 4.5 mm ring
with a 6.35 mm bit is refused, even though neither cut alone reaches the
other line. A band at least as wide as the bit is cut as drawn. To cut a
double line on purpose, use [Engrave](engrave.md).

The same holds for an [open path](#open-paths) drawn in a part. A line
1 mm inside the outline, 3 mm from a hole, or 2 mm from another line in
the part is refused with that error: its cut and the other line's cut
the wall between them away. A line that **meets** another, though, is
cut as drawn: a kerf slot drawn from the part's edge, a saw cut from
edge to edge, a slot from a hole's edge, two slots joined at a corner,
or two crossing. Near the point where they meet (within 1.41 bit
widths, 9 mm for a 6.35 mm bit) the two lines are not compared, so a
slot that meets the edge at 45° or more is fine. Past that the line must
keep a bit's width from the other: a slot from the bottom edge that runs
up 2 mm beside the side edge, turns to run along the bottom edge, or
leaves it at a shallow 10° is refused, and the error gives the narrowest
gap past the meeting point. Lines meet when they touch, cross, or stop
within `flatten_tolerance_mm` of each other; a slot that stops 1 mm short
of the edge leaves a 1 mm wall and is refused. Draw the slot to the edge,
move the line a bit's width away, or cut it with Engrave.

One line can also come too close to **itself**. A U-shaped line in a
part whose legs are 2 mm apart, a spiral whose turns are 4 mm apart, or
an outline with a 3 mm neck between two wider ends (a dumbbell) cut **on
line** with a 6.35 mm bit cuts the part between the two stretches away.
That is the same error, naming the shape twice. Only stretches more than
1.41 bit widths apart along the line are compared, so a smooth curve
and a corner of 90° or more are cut as drawn. A sharper corner is
treated as two lines meeting there: a V of 45° or more is cut, and a
sharper one, such as a 10° spike or the points of a star sharper than
45°, is refused. Only the part counts: a notch narrower than the bit cut
into an outline, or a hole with a narrow neck, has waste between its two
sides, and is cut. Cut a star or a spike **outside** to keep its points.

A part narrower than the bit **everywhere**, such as a round dot of
2.5 mm radius or a 3 mm square cut **on line** with a 6.35 mm bit, is
removed whole: every point of it is within half a bit of its line. That
is an error too, however small the part:

``operation `o1` cuts `Dot` (instance `i1`, profile 0) On line, but the
part is nowhere as wide as its 6.35 mm tool, so the bit would remove the
whole part; ...``

A part a little wider than the bit, a 3.3 mm radius circle, is cut as
drawn. A small **hole** cut on line is fine: its inside is waste. To mark
a dot on purpose, use [Engrave](engrave.md); to keep the part, cut it
**outside**.

Cut **outside**, a hole close to its outline only leaves a thin wall and
is cut as drawn.

Shapes that cross each other, or a shape that crosses itself, are an error:
DreamCarve cannot tell which side is material, so it cuts nothing. See
[Shapes, holes, and islands](shapes-and-holes.md), rules 10 and 11.
