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
    "shape": "rect"
  },
  "machining_order": "full_depth_first",
  "entry": { "ramp": { "angle_deg": 15.0 } },
  "lead": null,
  "corner_relief": "none"
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
| `side` | `"outside"`, `"inside"`, or `"on_line"`. See [Side](#side). |
| `depth_mm` | How deep to cut, measured down from the stock top. **Required:** `init` leaves it `null`, and export refuses until you set it. To cut through, use the stock's `thickness_mm` plus `through_cut_allowance_mm`. |
| `step_down_mm` | The most to cut in one pass. `null`, as `init` writes it, uses the tool's `step_down_mm`; if both are `null`, `check` reports the tool's as not set and export refuses. More than the tool's diameter is a warning. |
| `direction` | `"climb"` or `"conventional"`. See [Direction](#direction). |
| `through_cut_allowance_mm` | How far below the stock bottom the cut may go. 0 by default. See [Cutting through](#cutting-through). |
| `safe_z_mm` | The height above the stock top for moves between cuts. Must be more than 0. |
| `tabs` | Tabs that hold the part to the stock, or `null` for none. See [Tabs](tabs.md). |
| `machining_order` | `"full_depth_first"` or `"pass_first"`. See [Machining order](#machining-order). |
| `entry` | How the bit reaches each pass depth: `"plunge"`, a ramp, or a helix. `init` writes a 15° ramp. See [Entry](#entry). |
| `lead` | An arc onto the outline at the start of each pass and off it at the end, or `null` for none. See [Leads](#leads). |
| `corner_relief` | `"none"` (as `init` writes it), `"dogbone"` or `"t_bone"`: a notch in each corner the round bit cannot reach. See [Corner relief](#corner-relief). |

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

| The bit runs | Climb | Conventional |
| --- | --- | --- |
| Outside the outline it cuts | Clockwise | Counter-clockwise |
| Inside the outline it cuts | Counter-clockwise | Clockwise |

Directions are seen from above, with Y pointing away from you. An outside
Profile runs outside a part's outline and inside its holes. An `on_line`
cut counts as outside on a part's outline and inside on a hole. An open
path has no inside or outside, so the direction picks the end its cut
starts at: climb from its first vertex, conventional from its last (as
VCarve does).

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

- `"plunge"`: straight down at the tool's plunge rate. A bit that does
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
  (the project's `flatten_tolerance_mm`). Once at depth the bit goes back
  to the start and cuts the whole outline, so the ramped stretch is cut
  again at full depth. A helix is an error on On line: `check` reports
  it, and export refuses.

An Inside or Outside ramp starts at the start of the outline and runs into
the waste along the longest straight line that fits, at most three bit
diameters, stopping a tenth of the bit's diameter short of anything it must
not touch; then it comes back. A helix is placed next to the start of the
outline, where it fits with the same margin. A climb cut turns its helix
the way the outline runs: counter-clockwise inside a hole, clockwise
round a part; conventional turns it the other way.

The first pass descends from safe Z. Each pass after starts where the lap
before it ended, at the start of the outline on the floor of that pass, so
the bit does not ramp down through air it has already cut.

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
with a 6.35 mm bit), falls back to a plunge. Each fallback warns
`entry_fallback`, naming the shape and both entries. A shape that can
only be plunged warns on every export until its `entry` is `"plunge"`.

## Open paths

An open path (a line, an arc, a polyline that does not return to its
start) has no inside or outside, so an Inside or Outside Profile skips it
with `open_path_skipped`. An `on_line` Profile cuts it, as VCarve's
Profile "On" cuts an open vector: a slot one bit wide centred on the
line.

- Each pass runs the whole path at the pass depth, from its first vertex
  to its last under climb, or from its last to its first under
  conventional. Then the bit lifts to safe Z and goes back to the start
  for the next pass.
- A ramp runs back and forth along the path from the start, as on a
  closed outline, turning at the path's end when the path is shorter than
  three bit diameters. A later pass first comes straight down the slot
  the pass above cut, at the plunge rate, to that pass's depth, then
  ramps from there.
- [Tabs](tabs.md) are placed by distance from the path's first vertex as
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
  `toolpaths_overlap`. One that lies wholly in a part, as a slot cut into
  it, is not compared with that part's own lines there; instead, one that
  runs closer than a bit's width beside them stops the operation (see
  [Bands narrower than the bit](#bands-narrower-than-the-bit)). Cut after
  a Profile that freed the part it lies in, it warns
  `cut_inside_freed_part`.

A 100 mm line cut `on_line` 6 mm deep in 2 mm passes with a 15° ramp and a
6.35 mm bit: each pass ramps out 19.05 mm along the line and back, then
cuts the 100 mm to the line's far end, and lifts.

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

**When it does not fit.** The arc is tried at `radius_mm`, then at ¾, ½
and ¼ of it, each at `sweep_deg` and then at 60° and 45° (when smaller).
The largest that fits is used, and `lead_shrunk` gives the size cut. When
none fits, as in a 7 mm hole with a 6.35 mm bit, the outline is cut
without a lead and `lead_dropped` says so; when only the tabs are in the
way, the passes below the tab top go without one. A lead-out that does
not fit where the cut ends is left off (`lead_dropped`, reason `exit`).

**On line.** An On line Profile removes only its own cut, so there is no
waste beside it for an arc: a `lead` there is not cut, and `lead_dropped`
says so once for the operation.

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
`relief_skipped`.

Each notch plunges where the outline's path turned that corner and steps
down by the **tool's** `step_down_mm` to the operation's depth. The
notches are cut among the outline's own passes, nearest corner first: a
notch level comes right after the pass that cut down to it, so the bit
plunges into the slot that pass opened. A corner under a [tab](tabs.md)
is skipped with `relief_corner_not_cut`: the path rises over the tab
there, so the notch would plunge into uncut material. Edges shorter than
the bit is wide skip their corner with `relief_corner_too_tight`, and a
tool with no `step_down_mm` skips the relief with `relief_skipped`.

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

## Order of operations

Operations run in the order the project lists them, whichever tools they
use; DreamCarve never reorders them or groups them by tool. `check` warns
`cut_inside_freed_part` when an operation cuts inside a part an earlier
Profile without tabs has already cut free, and hints
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
   cut another lap, until the last pass. Under
   `"pass_first"` the tool instead rapids up to safe Z and goes on to the
   next shape, coming back for the next pass once every shape has had this
   one.
5. Rapid back up to safe Z.

Every move between shapes is at safe Z; the tool never travels through
material from one shape to another. With [corner relief](#corner-relief)
each pass is followed by its notches, each corner reached at safe Z; on
a through cut the notches of the last pass are cut from within it, at
each corner as the lap gets there.

## Warnings

These still give a safe program. `check` and `export` print each one as
`warning: <code>: operation `op1`: <sentence>`, with lengths rounded to
0.01 mm; the [Warnings](warnings.md) chapter lists every code.

| Warning says | Meaning |
| --- | --- |
| "... is an open path" | An Inside or Outside target is not a closed outline; it is not cut. See [Open paths](#open-paths). |
| "tool ... does not fit ..." | The bit does not fit inside the shape; that shape is not cut. |
| "... is the same outline in the same place as ..." | Two shapes are the same outline, or the same open path in either direction, in the same place; the second is not cut. |
| "tabs on ...", "the tab top is ..." | See [Tabs](tabs.md#warnings). |
| "there is no room for a `helix` entry ...", "the helix entry in ... is ... smaller than ..." | See [Entry](#entry), "When it does not fit". |
| "the lead arc on ... is ... smaller than ...", "there is no room for a lead-in arc ..." | See [Leads](#leads). |
| "the tool paths of ... and ... cut into each other" | Two shapes are closer than the bit needs to cut between them, so cutting one gouges the other. See below. |
| "corner relief ... is skipped", "the corner of ... is not relieved" | See [Corner relief](#corner-relief). |

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

Cut **outside**, a hole close to its outline only leaves a thin wall and
is cut as drawn.

Shapes that cross each other, or a shape that crosses itself, are an error:
DreamCarve cannot tell which side is material, so it cuts nothing. See
[Shapes, holes, and islands](shapes-and-holes.md), rules 10 and 11.
