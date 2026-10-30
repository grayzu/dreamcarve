# Drill operation

A **Drill** plunges the bit straight down at the centre of each circle:
dowel holes, screw holes, pilot holes. The bit never moves sideways in the
material, so the hole comes out the size of the bit. A circle smaller than
the bit is not drilled unless you say so ([Holes smaller than the
bit](#holes-smaller-than-the-bit)). VCarve calls this a "Drilling Toolpath"; Fusion calls it
"Drill".

For a hole bigger than the bit, use a [Bore](bore.md): it spirals an end
mill down inside the hole and cuts it to the size you drew. To open a cone
round the top of a hole, a seat for a screw head, drill it again with a
V-bit and the [countersink](#countersink) cycle.

Add it to the project's `operations` list by hand; `init` writes only a
Profile. A complete Drill, pecking 4 mm at a time with a full retract and
a short pause at the bottom of each peck:

```json
{
  "kind": "drill",
  "id": "op3",
  "name": "Dowel holes",
  "enabled": true,
  "targets": [ "i4", "i5", "i6", "i7" ],
  "tool": "t3",
  "cycle": { "peck": { "peck_depth_mm": 4.0, "retract": "full" } },
  "dwell_s": 0.5,
  "depth_mm": 12.2,
  "retract_mm": 1.0,
  "through_cut_allowance_mm": 0.2,
  "safe_z_mm": 5.0,
  "allow_tool_wider_than_hole": false
}
```

## Settings

| Field | Meaning |
| --- | --- |
| `id` | The operation's id, shown in messages. |
| `name` | Your name for it, written in the program. |
| `enabled` | `false` leaves the operation out of the program, with a warning. |
| `targets` | The ids of the instances to drill. Every circle among their outlines is a hole; any other outline is skipped with a warning. See [Which outlines are holes](#which-outlines-are-holes). |
| `tool` | The id of the tool to drill with. A twist drill (`"drill"`) or a flat end mill (`"end_mill"`); any other cutter is a warning, because its tip leaves a cone. A countersink needs a V-bit. |
| `cycle` | `"simple"`, `{ "peck": { "peck_depth_mm": …, "retract": … } }`, or `{ "countersink": { "diameter_mm": …, "start_depth_mm": …, "start_plane_cleared_externally": … } }`. See [Cycles](#cycles) and [Countersink](#countersink). |
| `dwell_s` | Seconds to wait at the bottom of every plunge, 0 or more. 0, the usual value, is no wait. See [Dwell](#dwell). |
| `depth_mm` | How deep to drill, measured down from the stock top. **Required**, except for a countersink, which works out its own depth and does not use it (`null` is usual). To drill through, use the stock's `thickness_mm` plus `through_cut_allowance_mm`. |
| `retract_mm` | How far above the stock top the bit lifts between pecks, and where it starts each hole from, 0 or more. Usually 1. See [Retract height](#retract-height). |
| `through_cut_allowance_mm` | How far below the stock bottom the hole may go, as for [Profile](profile.md#cutting-through). 0 by default. |
| `safe_z_mm` | The height above the stock top for moves between holes. Must be more than 0. |
| `allow_tool_wider_than_hole` | `false`, the default, skips a circle smaller than the bit, with a warning. `true` drills it anyway, at the bit's size. See [Holes smaller than the bit](#holes-smaller-than-the-bit). No effect on a countersink, whose bit is wider than its hole by design; `true` there warns `countersink_ignores_tool_width`. |

The tool needs a `plunge_mm_min`: every move a Drill cuts is a plunge.
Export still needs the tool's `feed_mm_min` and `rpm` set, as for every
operation.

## Which outlines are holes

A target outline is a hole when it is closed and every point of it lies
on one circle, within the project's `flatten_tolerance_mm`. That takes a
circle however it was drawn: an SVG `<circle>`, a DXF `CIRCLE`, four
quarter arcs, or a polygon of many short lines from a curve that was
flattened on import. The hole is drilled at the circle's centre.

Anything else, a rectangle, a slot, an ellipse or an open path, is skipped
and `check` warns `drill_target_not_circle`, naming it. The circles of the
same operation are still drilled.

The circle's size is not the hole's size: the bit is. Draw each hole the
size of the bit you will drill it with, so the drawing shows what you will
get. To cut a hole to a size bigger than the bit, use a [Bore](bore.md).

## Holes smaller than the bit

A Drill never cuts a hole larger than you drew it. A circle smaller than
the bit, by more than the project's `flatten_tolerance_mm` (0.01 mm
unless you changed it), is skipped: `check` and `export` warn
`drill_hole_too_small`, naming the circle and both sizes, and the
operation's other holes are still drilled. A 5 mm circle is not drilled
with a 6.35 mm bit; a 6.35 mm circle is, and so is one drawn 6.345 mm,
which is the bit's size as closely as the drawing is read.

```text
warning: drill_hole_too_small: operation `op3`: the 5 mm hole `holes-1` (instance `i5`, profile 0) is narrower than the 6.35 mm tool, so it is not drilled: a Drill never moves sideways, so the hole would come out the tool's size; cut it to size with a Bore and an end mill smaller than the hole (a Bore cuts holes bigger than its bit), drill it with a smaller drill, or set allow_tool_wider_than_hole to drill it at the tool's size
```

To cut that circle at the size you drew it, bore it with an end mill
smaller than the circle: see [Bore](bore.md). A 5 mm circle bores with a
3.175 mm (1/8") end mill.

Sometimes a hole the bit's size is what you want: a clearance hole drawn
at the screw's size, say. Set `"allow_tool_wider_than_hole": true` on
that Drill and it drills every such circle at the bit's size, warning
`drill_tool_wider_than_hole` for each so the program still tells you. The
setting is saved in the project, per operation.

If every target is skipped, for this reason or because none is a circle,
the operation drills nothing and warns `drill_no_holes`. The program is
still written with the other operations.

**Changed before v0.2.0.** Earlier builds drilled a circle smaller than
the bit at the bit's size, with `drill_tool_wider_than_hole`. A project
saved by them loads with `allow_tool_wider_than_hole: false`, so such
holes are now skipped. Set it to `true` to drill them as before.

## Cycles

### Simple

`"cycle": "simple"` drills each hole in one plunge, at the tool's plunge
rate, then lifts straight out. Use it for shallow holes, and for holes in
wood with a brad-point or twist drill that clears its own chips.

### Peck

`{ "peck": { "peck_depth_mm": 4.0, "retract": "full" } }` drills in pecks
of `peck_depth_mm`, lifting between them to clear the chips and let the
bit cool. The pecks are counted from the
[retract height](#retract-height), where each hole starts, as a
controller's own peck cycle (`G83`, `G73`) counts them. The first peck
starts in the air, so it cuts `peck_depth_mm` less `retract_mm` into the
material; every later peck goes one full `peck_depth_mm` deeper, and the
last one may be shorter. No peck ever goes more than `peck_depth_mm`
below the depth already drilled.

With `retract_mm: 1`, 12 mm in 4 mm pecks is four plunges: to 3, 7 and
11 mm below the stock top, then to 12. With `retract_mm: 0` it is three:
to 4, 8 and 12. A `retract_mm` larger than `peck_depth_mm` makes the first
peck stop in the air, as the controller's cycle would; keep the retract
height below the peck depth.

`peck_depth_mm: null` uses the tool's `step_down_mm`, as VCarve uses the
tool's pass depth; with both unset, `check` reports it and export refuses.
A peck that reaches the bottom of the hole from the retract height in one
go drills it in one plunge and warns `drill_peck_deeper_than_hole`.

**Changed in phase 2.** Earlier builds counted the pecks from the stock
top ([#153](https://github.com/grayzu/dreamcarve/issues/153)). With a
retract height above the stock, each peck now ends `retract_mm` higher
than it did, and a hole may take one more peck. The holes' depth is the
same. A project exported again gives a different peck program.

`retract` says how far the bit lifts between pecks:

- `"full"` lifts all the way out, to the retract height above the stock
  (VCarve "Retract above cutting start", Fusion "Deep drilling"). The
  chips leave with the bit. The bit then drops back down quickly, at rapid
  speed, to 0.5 mm above the depth it reached, and feeds on from there.
  Use it for deep holes and for materials that pack chips, such as MDF
  and aluminium.
- `{ "chip_break": { "gap_mm": 0.5 } }` lifts only `gap_mm`, more than 0,
  then feeds on (VCarve "Retract above the height of the previous pass",
  Fusion "Chip breaking"). The short lift snaps a long chip without the
  time of a full retract. Use it in metals and plastics that make
  stringy chips. A gap that would lift above the retract height lifts only
  to the retract height.

### Dwell

`dwell_s` waits that many seconds at the bottom of every plunge, simple or
peck, before lifting. A short dwell (0.2 to 0.5 s) cleans up the bottom
of a flat-bottomed hole drilled with an end mill. It is written as a `G4`
line: `G4 P0.500` on grblHAL and GRBL, which take seconds.

## Countersink

A **countersink** is a cone cut round the top of a hole, so a flat-head
screw sits flush. The `countersink` cycle plunges a V-bit once at the
centre of each circle, just deep enough for its sides to open a cone of
the width you ask for. It is a Drill of its own, usually on the same
circles as an earlier Drill that made the holes. PureCutCNC calls it the
`countersink` drill type; VCarve has no countersink cycle, and Fusion's
"Countersink" needs a countersink tool. A complete one, opening 6 mm
mouths with a 90° V-bit on holes in a floor 2.5 mm deep:

```json
{
  "kind": "drill",
  "id": "op5",
  "name": "Countersinks",
  "enabled": true,
  "targets": [ "i4", "i5" ],
  "tool": "t4",
  "cycle": {
    "countersink": { "diameter_mm": 6.0, "start_depth_mm": 2.5, "start_plane_cleared_externally": false }
  },
  "dwell_s": 0.0,
  "depth_mm": null,
  "retract_mm": 1.0,
  "through_cut_allowance_mm": 0.0,
  "safe_z_mm": 15.0,
  "allow_tool_wider_than_hole": false
}
```

This is the countersink of the [sign quick start](../quick-start/sign.md).

| Field of `countersink` | Meaning |
| --- | --- |
| `diameter_mm` | The width of the cone's **mouth**, where it meets the hole's top. Above 0, and at most the V-bit's `diameter_mm`. |
| `start_depth_mm` | Where the hole's top is, 0 or more below the stock top. 0 for a hole in the stock top; the floor's depth for a hole in a pocket or a V-Carve floor. See [Start depth](#start-depth). |
| `start_plane_cleared_externally` | `true` if something outside the project cleared the start plane. See [Start depth](#start-depth). |

### The depth

You never set the depth of a countersink: its mouth sets it. A V-bit of
included angle `α` opens a cone `2 × d × tan(α / 2)` wide at depth `d`
below its tip's start, so to open a mouth `diameter_mm` wide its tip goes

```text
depth = diameter_mm / (2 × tan(α / 2))
```

below the hole's top, the **start plane**. The Drill's `depth_mm` is not
used, and `check` does not ask for it. For a 6 mm mouth with the 1/4 in
(6.35 mm) V-bits:

| Bit angle | Depth for a 6 mm mouth | Cone height |
| --- | --- | --- |
| 90° | 3.000 mm | 3.175 mm |
| 60° | 5.196 mm | 5.499 mm |
| 30° | 11.196 mm | 11.849 mm |
| 15° | 22.787 mm | 24.117 mm |

A 90° bit matches metric countersunk screws, whose heads are 90°; an 82°
bit, the angle of inch flat-head screws, opens 6 mm at 3.451 mm. The **cone height**, `diameter / (2 × tan(α / 2))` for the bit's
own diameter, is how far the bit's sides reach before the straight shank
starts: a mouth the bit's full width puts the tip exactly one cone height
down, and a wider mouth would need the shank to cut, so it is the error
`countersink_mouth_exceeds_tool`.

A circle at least as wide as the mouth has no seat to cut: the cone's rim
would lie inside the hole. That circle is skipped with the warning
`countersink_mouth_not_wider_than_hole`, and the others are cut. The rule
that skips a circle narrower than a Drill's bit
([Holes smaller than the bit](#holes-smaller-than-the-bit)) does not
apply: a countersink's bit is wider than its hole by design, so
`allow_tool_wider_than_hole` has no effect, and `true` there warns
`countersink_ignores_tool_width`.

The tool must be a V-bit (`countersink_needs_v_bit`), with an included
angle above 0° and below 180° (`v_bit_angle_range`). Like every Drill it
needs a `plunge_mm_min`.

### Start depth

The start plane is `start_depth_mm` below the stock top. 0 countersinks
holes in the stock top. A hole in the floor of a pocket needs its seat
at the floor, so set `start_depth_mm` to the floor's depth: the tip then
goes `depth` below the floor, `start_depth_mm + depth` below the stock
top. The project must be able to see that floor:

- an enabled **Pocket** earlier in the list, cut to that depth, whose
  area holds the whole V-bit at every hole; or
- an enabled **V-Carve** earlier in the list whose flat floor, its start
  depth plus its flat depth below the stock top, holds the whole V-bit at
  every hole. The floor is the carved shapes inset by `F × tan(α / 2)`
  with that carve's own V-bit, for flat depth `F` (see
  [V-Carve: start depth](v-carve.md#start-depth)).

"The whole V-bit" is the disc of the countersink tool's diameter at each
circle's centre, not the circle or the mouth: the bit is that wide above
the mouth, and its shank must not touch a wall. Every circle among the
targets counts, including one the mouth rule skips. Of a Pocket and a
V-Carve that both hold the bit, the one later in the list counts.

- Such a floor at another depth warns `start_depth_mismatch`, naming the
  operation and both depths. Set `start_depth_mm` to the floor's depth.
- A start depth above 0 with no such floor is the error
  `start_depth_uncleared` ("… contains the whole of its V-bit at each
  hole"), at the countersink's `start_depth_mm`.
- If another program, or hand work, cleared the start plane, set
  `start_plane_cleared_externally: true` and it exports. The project
  cannot see that pocket, so the bit's flank is then counted from the
  stock top: `start_depth_mm` plus the depth must fit within the cone
  height. The same holds with a mismatched floor.

The cone check: the depth of a countersink from where the bit's sides
first meet wood, the start plane when it is cleared and the stock top
when it is not, must be at most the bit's cone height, or `check` stops
with `v_bit_cone_exceeded`, at the countersink's `diameter_mm`. With the
90° 6.35 mm bit, a 6 mm countersink on a cleared floor engages 3 mm of
its 3.175 mm cone; on a 2.5 mm floor nothing clears, it would engage
5.5 mm, and is refused.

**Ridges on a V-Carve floor.** Where an end mill's clearing pass cleared
the floor, it is flat. Where the V-bit alone cut it, it leaves low ridges
between its rings, up to `step_over_fraction × F` high (see
[V-Carve: the V-bit floor](v-carve.md#the-v-bit-floor)). The check treats
the floor as flat, so a countersink there cuts through the ridges with
its sides, up to that much above its start plane, and a mouth the bit's
full width could brush the ridge tops with the shank. Put countersinks
where a clearing pass reached, or keep the carve's `step_over_fraction`
small (0.1) so the ridges are low. In the sign quick start both
countersinks sit where the end mill cleared the floor.

### What the program does

For each hole, nearest first as for every Drill:

1. Rapid at safe Z to above the hole.
2. Rapid down to the retract height, `retract_mm` above the **stock
   top**, not above the start plane: a rapid to 1 mm above a floor would
   end in the stock wherever the floor was not cleared.
3. Plunge at the plunge rate, straight down at the hole's centre, through
   the air over the floor and on to `depth` below the start plane. Wait
   `dwell_s` if set.
4. Rapid straight up to safe Z.

One plunge per hole: a countersink has no pecks, and the tool's
`step_down_mm` is not used. The deepest point, `start_depth_mm + depth`,
must not be below the stock bottom plus `through_cut_allowance_mm`, as
for every operation. `export` states the mouth and that depth in its
summary, as ``op5 `Countersinks`: …, 6.000 mm countersinks, 5.500 mm deep``.

**Never a canned cycle.** A countersink is always written as plain `G0`
and `G1` moves (and `G4` for a dwell), on every controller, with
`use_canned_cycles` on or off, and with no `canned_cycle_expanded`
warning: the motion is a simple plunge, but its depth is derived from the
mouth and the bit, which a controller's `G81` cannot say. The safety
check holds every countersink plunge to its hole's centre and to exactly
that depth ([Safety](safety.md#what-dreamcarve-checks), rule 15).

## Retract height

The bit travels between holes at safe Z. At each hole it drops quickly
to the retract height, `retract_mm` above the stock top, and starts
plunging there, so it does not crawl through the air at the plunge rate
from safe Z. Between pecks with `"full"` it lifts back to the retract
height, not to safe Z. After the last plunge it lifts straight to safe Z.

The retract height is never above safe Z: a larger `retract_mm` lifts only
to safe Z. It is never below the stock top: the bit must not move fast
into wood. A negative `retract_mm` is an error at `check`.

## Order of holes

The holes are drilled nearest first, starting from the first one in
drawing order (the order of `targets`, then the order of each drawing's
outlines). From each hole the bit goes to the nearest hole not yet
drilled; two holes the same distance away, to within 0.001 mm, are taken
in drawing order. The
result is the same every time you export. `machining_order` does not
apply: each hole is one plunge or one set of pecks, drilled to depth
before the bit moves on. See [Machining order and operation
order](machining-order.md).

## What the program does

For each hole:

1. Rapid at safe Z to above the hole.
2. Rapid down to the retract height.
3. Plunge at the plunge rate to the first peck depth, `peck_depth_mm`
   below the retract height (or the full depth for a simple cycle), then
   wait `dwell_s` if set.
4. For each further peck: lift (to the retract height and back down to
   0.5 mm above the depth reached, or by the chip-break gap), plunge to
   the next depth, wait.
5. Rapid up to safe Z.

Every fast move down ends either above the stock or inside the part of
the hole already drilled, never in uncut material. `export` checks this on
the written program ([Safety](safety.md#what-dreamcarve-checks), rule 2),
along with the depth, the feeds and every other safety rule.

## Canned cycles

Some controllers have drilling cycles that write a whole hole in one
line. They are off by default. To use them, set
`"use_canned_cycles": true` in the project's `post_processor` block, with
a post-processor that has them ([Post-processors](post-processors.md#canned-cycles)).
DreamCarve writes `G81` and `G82` on LinuxCNC and Mach3, and no peck
cycles with any bundled post-processor
([why](post-processors.md#controllers-compared)). grblHAL, GRBL, UCCNC and generic have none, so `check`
reports `true` as an error there.

With canned cycles on, a Drill is written as:

```text
G0 Z5.000
X20.000 Y20.000
G98
G81 X20.000 Y20.000 Z-5.000 R1.000 F300.000
X50.000 Y70.000 Z-5.000 R1.000
X80.000 Y30.000 Z-5.000 R1.000
G80
```

a rapid over the first hole at safe Z, `G98` (go back to the starting Z,
safe Z, after each hole), one line per hole, then `G80` to end the cycles.
Each line has the hole's `X Y`, its bottom `Z`, the retract height `R`,
and the peck `Q` or the dwell `P` where the cycle has one. `F`, the plunge
rate, is on the first. The cycle is:

| Drill | Cycle |
| --- | --- |
| `"simple"` | `G81` |
| `"simple"` with `dwell_s` | `G82` with `P` (seconds, or milliseconds where the controller takes them) |
| peck, `retract: "full"` | `G83` with `Q` |
| peck, `chip_break` | `G73` with `Q` |
| peck with `dwell_s` | none: always plain moves |
| `countersink` | none: always plain moves, with no warning ([why](#countersink)) |

The holes, their order and their depths are the same as with plain moves.
`export` reads each cycle line back as the moves the controller makes for
it and checks them against the same rules as plain moves, and against the
Drill's own moves: the depth, the fast moves, and every hole. If the cycle
would not drill what the plain moves drill, the whole Drill is written as
plain moves instead, with the warning `canned_cycle_expanded` saying why.
One operation is never part cycles and part moves. This happens when:

- a peck also has a dwell (no cycle does both);
- the post-processor has no command for the cycle, or does not state how
  far its `G83` or `G73` backs off between pecks.

**With a clearance height.** When every hole is a part of its own, as for
circles drawn one by one or imported from SVG or DXF, the Drill crosses
between them at the clearance height. The cycles then start from it: the
rapid over the first hole is at the clearance height, and `G98` brings the
bit back up to it after every hole. With a clearance height of 20 mm:

```text
G0 Z20.000
X20.000 Y20.000
G98
G81 X20.000 Y20.000 Z-5.000 R1.000 F300.000
X50.000 Y70.000 Z-5.000 R1.000
X80.000 Y30.000 Z-5.000 R1.000
G80
```

The bit goes straight from the clearance height down to `R` and straight
back up, where the plain moves stop at safe Z on the way. The holes are
the same. When all the holes are of one part, the cycles start from safe
Z, as without a clearance height.

**Separate parts and parts with several holes.** A part can have several
holes, for example a compound SVG path or a DXF block of circles. A Drill
with such a part and other parts crosses between parts at the clearance
height and between the holes of one part at safe Z. `G98` goes back to one
height for every hole of a series, so the Drill is written as one series
per part. Each series starts from safe Z over the part's first hole and
ends with `G80`. Then the bit rises to the clearance height, crosses to
the next part, and comes down to safe Z there. With a part of two holes
at (20, 40) and (20, 50) between two single holes:

```text
G0 Z20.000
X20.000 Y20.000
Z5.000
G98
G81 X20.000 Y20.000 Z-5.000 R1.000 F300.000
G80
G0 Z20.000
Y40.000
Z5.000
G98
G81 X20.000 Y40.000 Z-5.000 R1.000 F300.000
X20.000 Y50.000 Z-5.000 R1.000
G80
G0 Z20.000
X50.000 Y70.000
Z5.000
G98
G81 X50.000 Y70.000 Z-5.000 R1.000 F300.000
G80
```

This is the plain program's motion, move for move. Every series states all
its words again, `F` included.

A post-processor that has `G83` and `G73` writes them with any retract
height: DreamCarve counts the pecks from the retract height `R`, as the
controller does, so the cycle and the plain moves are the same pecks.

## Warnings

| Code | Meaning |
| --- | --- |
| `drill_target_not_circle` | A target outline is not a circle, or is open, so it is not drilled. Draw a circle, or remove the target. |
| `drill_hole_too_small` | The circle is smaller than the bit, so it is not drilled. Bore it to size with a smaller end mill ([Bore](bore.md)), use a smaller drill, draw the circle the bit's size, or set `allow_tool_wider_than_hole`. See [Holes smaller than the bit](#holes-smaller-than-the-bit). |
| `drill_tool_wider_than_hole` | `allow_tool_wider_than_hole` is `true` and the bit is wider than the circle drawn, so the hole comes out the bit's size. |
| `drill_no_holes` | Every target was skipped, so the operation drills nothing. |
| `drill_peck_deeper_than_hole` | `peck_depth_mm` (or the tool's step-down) reaches the hole's bottom from the retract height, so it is drilled in one plunge. Use a smaller peck, or the `"simple"` cycle. |
| `drill_retract_below_stock_top` | `retract_mm` is below the stock top; the bit retracts to the stock top instead. |
| `drill_cutter_unusual` | The tool is not a drill or an end mill, so the hole's bottom is a cone. Not reported for a countersink. |
| `countersink_mouth_not_wider_than_hole` | A countersink's circle is at least as wide as its mouth, so there is no seat to cut and that hole is skipped. Use a wider `diameter_mm` (at most the bit's diameter), or remove the circle. See [Countersink](#the-depth). |
| `countersink_ignores_tool_width` | A countersink sets `allow_tool_wider_than_hole: true`, which does nothing for it. Set it to `false`. |
| `start_depth_mismatch` | An earlier Pocket or V-Carve floor holds the countersink's bit at every hole, at another depth than its `start_depth_mm`. Set the start depth to the floor's depth. See [Countersink: start depth](#start-depth). |

The errors are those of every operation: a missing plunge rate, a depth
below the stock bottom plus the allowance, and a hole whose bit would reach
past the stock's edge. A peck cycle with no `peck_depth_mm` and no tool
`step_down_mm`, and a chip-break `gap_mm` of 0 or less, are errors too.

A countersink adds its own errors, which stop `check` and `export`:

| Code | When | What to do |
| --- | --- | --- |
| `countersink_needs_v_bit` | The tool is not a V-bit. | A countersink's depth follows the bit's angle: use a V-bit. |
| `v_bit_angle_range` | The V-bit's `included_angle_deg` is not above 0 and below 180. | Fix the tool's angle. |
| `countersink_diameter_positive` | `diameter_mm` is 0 or less. | Give the mouth's width. |
| `countersink_mouth_exceeds_tool` | `diameter_mm` is wider than the V-bit. | Past its diameter a V-bit has no cutting edge: use a mouth at most the bit's diameter, or a wider bit. |
| `start_depth_negative` | `start_depth_mm` is below 0. | The start plane is a depth below the stock top, 0 or more. |
| `start_depth_uncleared` | `start_depth_mm` is above 0, no enabled Pocket or V-Carve floor before the countersink holds the whole bit at each hole, and `start_plane_cleared_externally` is `false`. | Add that Pocket, move the countersink after the carve whose floor it sits on, or, if another program clears the start plane, set `start_plane_cleared_externally`. |
| `v_bit_cone_exceeded` | The countersink would engage the bit deeper than its cone is tall: the depth, plus a start depth no floor clears. | Use a smaller mouth or a wider bit, or clear the start plane with an earlier Pocket. |

Export reports the same problems from the toolpath generator, if they
reach it, as "operation … countersinks with its V-bit's flanks engaged …
mm deep, but the bit's cone is only … mm tall, so its shank would cut" and
the like.
