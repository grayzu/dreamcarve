# Drill operation

A **Drill** plunges the bit straight down at the centre of each circle:
dowel holes, screw holes, pilot holes. The bit never moves sideways in the
material, so the hole comes out the size of the bit, whatever size you drew
the circle. VCarve calls this a "Drilling Toolpath"; Fusion calls it
"Drill".

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
  "safe_z_mm": 5.0
}
```

## Settings

| Field | Meaning |
| --- | --- |
| `id` | The operation's id, shown in messages. |
| `name` | Your name for it, written in the program. |
| `enabled` | `false` leaves the operation out of the program, with a warning. |
| `targets` | The ids of the instances to drill. Every circle among their outlines is a hole; any other outline is skipped with a warning. See [Which outlines are holes](#which-outlines-are-holes). |
| `tool` | The id of the tool to drill with. A twist drill (`"drill"`) or a flat end mill (`"end_mill"`); any other cutter is a warning, because its tip leaves a cone. |
| `cycle` | `"simple"`, or `{ "peck": { "peck_depth_mm": …, "retract": … } }`. See [Cycles](#cycles). |
| `dwell_s` | Seconds to wait at the bottom of every plunge, 0 or more. 0, the usual value, is no wait. See [Dwell](#dwell). |
| `depth_mm` | How deep to drill, measured down from the stock top. **Required.** To drill through, use the stock's `thickness_mm` plus `through_cut_allowance_mm`. |
| `retract_mm` | How far above the stock top the bit lifts between pecks, and where it starts each hole from, 0 or more. Usually 1. See [Retract height](#retract-height). |
| `through_cut_allowance_mm` | How far below the stock bottom the hole may go, as for [Profile](profile.md#cutting-through). 0 by default. |
| `safe_z_mm` | The height above the stock top for moves between holes. Must be more than 0. |

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

The circle's size is not the hole's size: the bit is. A circle smaller
than the bit warns `drill_tool_wider_than_hole` and is still drilled, the
bit's size. Draw each hole the size of the bit you will drill it with, so
the drawing shows what you will get.

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
drilled; two holes the same distance away are taken in drawing order. The
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

A post-processor that has `G83` and `G73` writes them with any retract
height: DreamCarve counts the pecks from the retract height `R`, as the
controller does, so the cycle and the plain moves are the same pecks.

## Warnings

| Code | Meaning |
| --- | --- |
| `drill_target_not_circle` | A target outline is not a circle, or is open, so it is not drilled. Draw a circle, or remove the target. |
| `drill_tool_wider_than_hole` | The bit is wider than the circle drawn. The hole comes out the bit's size. Use a smaller bit, or draw the circle the bit's size. |
| `drill_peck_deeper_than_hole` | `peck_depth_mm` (or the tool's step-down) reaches the hole's bottom from the retract height, so it is drilled in one plunge. Use a smaller peck, or the `"simple"` cycle. |
| `drill_retract_below_stock_top` | `retract_mm` is below the stock top; the bit retracts to the stock top instead. |
| `drill_cutter_unusual` | The tool is not a drill or an end mill, so the hole's bottom is a cone. |

The errors are those of every operation: a missing plunge rate, a depth
below the stock bottom plus the allowance, and a hole whose bit would reach
past the stock's edge. A peck cycle with no `peck_depth_mm` and no tool
`step_down_mm`, and a chip-break `gap_mm` of 0 or less, are errors too.
