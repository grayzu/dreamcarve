# Pocket operation

A **Pocket** clears all the material inside a closed shape down to a
depth: a recess for an inlay, a tray, a lid seat. Shapes inside the pocket
are **islands**, left standing at full height; a shape inside an island is
a pocket again. Two clearance strategies, named as VCarve names them:
**offset**, concentric rings stepping in from the wall by the stepover,
and **raster**, parallel straight lines at an angle you choose. See
[Raster](#raster).

Add it to the project's `operations` list by hand; `init` writes only a
Profile. A complete Pocket:

```json
{
  "kind": "pocket",
  "id": "op2",
  "name": "Recess",
  "enabled": true,
  "targets": [ "i1" ],
  "tool": "t1",
  "strategy": "offset",
  "wall_pass": "last",
  "depth_mm": 3.0,
  "step_down_mm": null,
  "step_over_fraction": 0.4,
  "direction": "climb",
  "allowance_mm": 0.0,
  "rest_of": null,
  "entry": "plunge",
  "corner_relief": "none",
  "machining_order": "full_depth_first",
  "through_cut_allowance_mm": 0.0,
  "safe_z_mm": 5.0
}
```

Put a Pocket **before** the Profile that cuts the part out: once the part
is free, the pocket would be cut in a loose part (`check` warns
`cut_inside_freed_part`).

## Settings

| Field | Meaning |
| --- | --- |
| `id` | The operation's id, shown in messages. |
| `name` | Your name for it, written in the program. |
| `enabled` | `false` leaves the operation out of the program, with a warning. |
| `targets` | The ids of the instances to pocket. Every closed outline of each one takes part; open paths are skipped with a warning. |
| `tool` | The id of the tool. It must be an `"end_mill"`: the pocket is offset by half the diameter, which only a flat end mill cuts at every depth. |
| `strategy` | `"offset"` for concentric rings, or `{ "raster": { "angle_deg": 0.0 } }` for parallel lines at that angle, 0 or more and less than 180. See [Raster](#raster). |
| `wall_pass` | `"last"` (as `init` would write it), `"first"` or `"none"`. See [Wall pass](#wall-pass). |
| `depth_mm` | How deep to cut, measured down from the stock top. **Required.** |
| `step_down_mm` | The most to cut in one pass. `null` uses the tool's `step_down_mm`; if both are `null`, export refuses. |
| `step_over_fraction` | The stepover as a fraction of the tool diameter, more than 0 and at most 1. `null` uses the tool's `step_over_fraction`; if both are `null`, export refuses. See [Stepover](#stepover). |
| `direction` | `"climb"` or `"conventional"`, for the rings. Raster lines ignore it. See [Direction](#direction). |
| `allowance_mm` | Material to leave on the walls and round the islands, mm, 0 or more. See [Allowance](#allowance). |
| `rest_of` | `null`, or the id of an earlier Pocket of the same targets whose leftovers this one clears. See [Rest machining](#rest-machining). |
| `entry` | How the bit gets down to each pass: `"plunge"`, `{ "ramp": { "angle_deg": 15.0 } }` or `{ "helix": { "diameter_fraction": 0.8, "angle_deg": 5.0 } }`. See [Entry](#entry). |
| `corner_relief` | `"none"`, `"dogbone"` or `"t_bone"`: a notch in each corner the round bit cannot reach. See [Corner relief](#corner-relief). |
| `machining_order` | `"full_depth_first"` or `"pass_first"`. See [Machining order](machining-order.md#machining-order). |
| `through_cut_allowance_mm` | How far below the stock bottom the cut may go. A pocket deeper than the stock needs it, as a through cut does. |
| `safe_z_mm` | The height above the stock top for moves between cuts. |

## How the pocket is cleared

The bit's centre can come no nearer the wall than half its diameter, so
DreamCarve first shrinks the pocket by the tool radius and grows each
island by it. What is left is where the bit's centre may go: the
**tool-centre area**. If nothing is left, the bit is wider than the pocket;
that shape is skipped with `pocket_no_room`.

The tool-centre area's own outline is the **wall ring**: the bit runs
along it with its edge on the wall, and round each island with its edge
on the island. Inside, rings step in by the stepover until nothing is
left. Each pass cuts the innermost ring first and works outward, so every
ring cuts into material the ring inside it has opened up, and the wall is
cut last, with the least load on the bit (PureCutCNC's "inner first").

Between two rings of one pass the bit stays down and cuts straight across
when the move stays inside the tool-centre area; otherwise it lifts to
safe Z. Each ring starts at its point nearest where the previous one
ended: a corner, or on a curve the nearest point of the curve, so those
links are short. Between passes, and between separate
pockets, the bit lifts to safe Z and comes back down by the operation's
[entry](#entry): a plunge at the plunge rate, a ramp or a helix.

A 100 × 50 mm rectangle with a 6.35 mm bit at 40 % stepover gives a wall
ring 93.65 × 43.65 mm across its centre line and eight more rings inside
it, each 2.54 mm in from the one outside it.

### Islands

An island grown by the tool radius may reach the pocket wall, or another
island. Then the tool-centre area splits into separate pieces, each with
its own rings, and the bit lifts to safe Z to go from one to the other:
it never cuts across an island. The material in the gap is narrower than
the bit and stays; `pocket_island_too_close` names the island and the
gap. With an [allowance](#allowance) the bit needs its diameter plus
twice the allowance to pass, so a gap narrower than that warns too.

A shape inside an island is a pocket of its own, cut after the pocket
around the island.

## Raster

`{ "raster": { "angle_deg": A } }` clears each tool-centre area with
straight lines one stepover apart, then cuts the wall ring (see
[Wall pass](#wall-pass)).

**The angle** is in degrees from the +X axis, counter-clockwise, seen
from above with Y pointing up, as on every drawing in DreamCarve. `0`
runs the lines along X, `90` along Y, `45` up and to the right, `135` up
and to the left. 0 to 180 covers every direction a line can have, so
180 and more, and anything below 0, are refused (`check` names
`strategy.raster.angle_deg`). PureCutCNC draws Y downwards, so its 45° is
DreamCarve's 135°.

**The lines** start half a stepover in from the edge of the tool-centre
area and step across it by the stepover. At 0° the first line is half a
stepover below the top edge, as in PureCutCNC. Where the area is
narrower than one stepover, there is one line down its middle. If the
last line would end up more than a stepover from the far edge, one more
line goes half a stepover from it, so no strip is left between the lines
and the wall ring. Lines run in alternate directions, a zigzag. An
island cuts a line into pieces, one on each side of it.

**Between lines** the bit stays down and cuts straight across to the
start of the next line when that move is shorter than the bit's diameter
plus the stepover and stays inside the tool-centre area, through
material the two lines have just cut. Otherwise, as across an island, it
lifts to safe Z and plunges again. A rectangle at 0° is cut in one
zigzag per pass with no lift.

**Round an island** the pieces are cut nearest first: from the end of
one piece the bit goes to the nearest piece it can reach without
lifting, so it zigzags down one side of the island, along the island's
edge, before it lifts once to cross to the other side. Next to a round
island no straight move from one line's end to the next stays clear of
the island. With a wall pass (`"last"` or `"first"`) the bit then
follows the island's curve instead, at the cutting feed and the pass's
depth, with its edge on the island, as the island ring will: it cuts
nothing the island ring does not, and never into the island. On the
100 × 50 mm rectangle with a 20 mm round island and a 6.35 mm bit, that
is 1 lift per pass with the wall pass last (2 with it first), where a
straight-only rule lifts at all 11 lines the island splits. With
`"none"` the band along the island is not the pocket's to cut, so the
bit lifts at about every line that the island splits, whatever the
order; there the lines keep their scan order.

**Along a wall that curves inward** the same holds. Where the pocket's
wall bulges into it, as at a rounded bay or the inside corner of an L,
a straight move between two line ends would cut into the wall, so with
a wall pass the bit follows the wall's curve instead, with its edge on
the wall, as the wall ring will. On a 100 × 50 mm rectangle with a
30 mm half-round bay pushed into one long side, that is 1 lift per pass
where a straight-only rule lifts 6 times; on a 100 × 50 mm L, none
where it lifts once. With `"none"` the bit lifts there, as before.

**Scallops.** Each line leaves a shallow ridge on the floor between it and
the next, and the zigzag cuts alternately climb and conventional, so a
raster floor shows faint lines along the raster angle, more visibly in
soft woods. A smaller stepover makes them finer. If the floor finish
matters more than time, use `"offset"`, which follows the wall all the
way in.

**Wall pass.** With `"last"`, the default, the wall and island rings are
cut after the lines and clean up the scallops and line ends along every
wall. `"first"` cuts them before the lines; `"none"` leaves the walls with
the line ends on them, and the pocket up to one stepover undersize on the
walls that run along the lines.
The wall rings follow `direction`; the lines do not.

A 100 × 50 mm rectangle with a 6.35 mm bit at 40 % stepover and 0° gives
17 lines per pass, 2.54 mm apart, each 93.65 mm long, then the wall ring.

## Wall pass

- `"last"`: each pass ends with the wall ring and the island rings, so
  the walls are cut once more after the clearing, cleanly.
- `"first"`: each pass starts with them.
- `"none"`: they are not cut. With `"offset"` the pocket comes out one
  stepover undersize on every wall, and the walls keep the clearing
  rings' marks (VCarve's "no profile pass"); a pocket too small for a
  second ring is then not cut at all. With `"raster"` the walls keep the
  line ends, and the walls along the lines are up to one stepover
  undersize.

## Stepover

The stepover is the distance from one ring to the next, as a fraction of
the bit's diameter: 0.4 with a 6.35 mm bit is 2.54 mm. Smaller is slower
and leaves a smoother floor. Above about 0.7 the corners of neighbouring
rings are further apart diagonally than the bit is wide, so small ridges
stay on the floor there. The operation's `step_over_fraction` wins over the
tool's.

## Direction

Seen from above with the spindle turning clockwise, **climb** keeps the
material on the bit's right. In a pocket the wall is outside the ring, so
climb runs the wall ring and every ring inside it **counter-clockwise**;
round an island the material is inside the ring, so climb runs it
**clockwise**. **Conventional** reverses both. The island and the wall
always turn opposite ways for one setting; that is what keeps the cut the
same kind on both. Raster lines zigzag whatever `direction` says; the
wall rings of a raster pocket follow it.

## Entry

`entry` says how the bit gets down to the depth of each pass, wherever it
arrives from safe Z (VCarve's "Ramps"; Fusion's "Ramp Type"). A ring or
raster line reached by a stay-down link from the one before is already at
depth, so it has no entry.

- `"plunge"`: straight down at the tool's plunge rate, from 1 mm above the
  stock or the floor already cut there (the bit rapids that far). A bit that does
  not cut well at its centre, as most do not, takes this hard.
- `{ "ramp": { "angle_deg": 15.0 } }`: a zigzag. The bit runs back and
  forth along a short straight line through the point where the ring or
  raster line starts, going down `angle_deg` from the horizontal, until it reaches
  the pass depth, then moves to that start. The line is the longest
  one through that point that fits in the tool-centre area, kept a tenth
  of the bit's diameter in from its edge so the ramp does not mark the
  wall, and at most three diameters long. When the depth is a hair over
  a whole number of legs, the last two legs share what is left, so no
  leg is too short for the program to write; no leg is steeper than
  `angle_deg`.
- `{ "helix": { "diameter_fraction": 0.8, "angle_deg": 5.0 } }`: a spiral
  down, `diameter_fraction` of the bit's diameter across (5.08 mm for a
  6.35 mm bit at 0.8), at `angle_deg`, then one flat turn at the pass
  depth to clean the floor, then on to the ring's or line's start. The helix is
  placed next to that start, where it fits with the same margin from the
  edge. A fraction at most 1 leaves no core standing in the middle.

The angle must be more than 0 and at most 45 degrees, the fraction more
than 0 and at most 1. A climb pocket turns its helix counter-clockwise,
seen from above, as its rings run; conventional turns it clockwise.

The bit rapids straight down from safe Z to 1 mm above the stock top,
and the first pass's ramp or helix starts there; each pass after starts
1 mm above the floor of the pass before, so the bit does not ramp or
spiral down through air. Where that pass's cut went through the start,
the bit rapids down to it; elsewhere it rapids to 1 mm above the stock
and comes down the rest at the plunge rate.

**Feed rate.** A ramp or helix moves at the tool's feed rate, except
that its speed straight down is never faster than the plunge rate: when
`feed × sin(angle)` would exceed the plunge rate, the feed of each
descending move is reduced until it does not. A 5° helix with a 1500
mm/min feed and a 100 mm/min plunge runs at about 1147 mm/min.

**When it does not fit.** A helix that does not fit at full size is made
smaller, halving up to three times, and `helix_diameter_reduced` says how
small. A helix that does not fit at all, in a slot less than about 1.3
bit diameters wide, falls back to a ramp; a ramp that does not fit, in a
hole barely wider than the bit, falls back to a plunge. Each fallback
warns `entry_fallback`, naming the shape and both entries. A very shallow
angle on a deep pass that would take more than 20 000 moves falls back
the same way, and so does a helix too small for the program to write:
one whose moves would each fall less than the last decimal place
(by default 0.001 mm, or 0.0001 in in an inch program), or run less than about
one and a half of it sideways, as a 5 % helix of a 0.5 mm bit does. A
shape that can only be plunged warns on every export until its `entry`
is `"plunge"`.

The ramp or helix is inside the tool-centre area of the pass, and out of
every island grown by the bit's radius, so the whole bit stays in the
material the pocket removes: the [safety](#safety) check covers it.

## Allowance

`allowance_mm` leaves that much material on every wall and round every
island, for a finishing Pocket to take off later (VCarve's "Pocket
Allowance"; the sign is VCarve's: a positive value leaves material). The
bit's centre stays its radius plus the allowance from the wall, so the
wall ring, the clearing rings or raster lines, and the ramp or helix all
move in by the allowance, and the floor is cut as usual. A negative
allowance, which would cut past the drawn wall, is refused.

A 100 × 50 mm rectangle with a 6.35 mm bit at an allowance of 0.5 mm is
cleared to 99 × 49 mm, with the wall ring 92.65 × 42.65 mm across its
centre line.

The usual use is roughing: clear the pocket quickly with a large bit and
an allowance, then finish it with a smaller bit, either a full Pocket at
0 allowance or, quicker, a [rest machining](#rest-machining) Pocket.

## Rest machining

A Pocket with `rest_of` set to the id of an earlier Pocket clears only the
material that Pocket left (Fusion's "Rest Machining"; VCarve's "Use
Larger Area Clearance Tool" does the same in one toolpath): the band its
allowance left on the walls, the corners its bit was too round to reach,
and slots and notches too narrow for it. It uses its own bit, stepover,
strategy, entry and allowance. Everything else in the middle of the
pocket, which the earlier Pocket already cut, it leaves alone.

```json
{ "kind": "pocket", "id": "op3", "name": "Rest", "tool": "t2",
  "targets": [ "i1" ], "rest_of": "op2", "allowance_mm": 0.0, … }
```

The earlier Pocket must come before this one in `operations`, be enabled,
and have the same `targets`; `check` and `export` refuse anything else
(`check` names `rest_of`). Chains work: a third Pocket may rest on the
second, and then clears what the second's bit could not reach.

**How the leftover is worked out.** DreamCarve does not look at the
earlier Pocket's toolpath. It takes that Pocket's bit diameter and
allowance from the project and works out, from the shape alone, where
that bit could reach: the pocket shrunk by the bit's radius plus the
allowance, grown back by the radius. What is outside that, less this
Pocket's own allowance, is the leftover. This Pocket's bit then runs
wherever it would touch the leftover: along every wall, which also takes
off the earlier allowance, into each corner as far as it can, and through
each narrow slot, overlapping the cleared area by up to its own width so
no ridge is left at the edge. It does not run along the inner edge of
that band, where its bit would only touch air the earlier Pocket cleared.

**Safety.** A rest machining Pocket is checked exactly as any Pocket: its cut
stays inside its own shape, the allowance off the walls and out of every
island (safety rule S14). If the earlier Pocket's settings were wrong,
the rest machining Pocket cuts more air or leaves material; it never cuts outside
the shape.

**When there is nothing to do.** If this Pocket's bit reaches nothing the
earlier one left (it is the same bit or a larger one, at the same or a
larger allowance), it warns `rest_of_clears_nothing` and cuts nothing.

A 100 × 50 mm rectangle with a 10 × 10 mm notch, roughed with a 12.7 mm
bit at 0.5 mm allowance, then rest-machined with a 3.175 mm bit: the rest
Pocket cuts a band about 2 mm wide along the walls, the four corners and
the notch, under a tenth of the length a full 3.175 mm Pocket would cut.

## Corner relief

A round bit leaves a radius in every inside corner of a pocket, so a
square part, a hinge leaf or the tenon of a box joint does not sit flat in
it. `corner_relief` cuts a short notch into each such corner after the
pocket is cleared, so the square part fits. The names are VCarve's ("Create
Fillets": Dog-Bone and T-Bone):

- `"dogbone"`: the notch runs along the corner's bisector, the line that
  splits the corner in half, until the bit just touches the corner. It
  takes the same small bite out of both walls (0.29 × the bit's radius in a
  square corner). Use it where both walls show equally, or where neither
  matters: the usual choice for joints and inlays.
- `"t_bone"`: the notch runs along one wall, the corner's **longer** edge
  (as VCarve places it; on equal edges the one nearer the X axis), until
  the bit reaches the corner. It cuts a slot a bit's radius deep into that
  wall and leaves the other wall straight (exactly so in a square corner).
  Use it to keep a visible face clean, or where a slot along the long side
  looks or holds better than two bites.

Corners relieved: every corner of the pocket's outline where the outline
turns by more than 15°, and the inward-pointing corners of islands (the
inside corner of an L-shaped island traps the bit just like a pocket
corner). An island's outward corners are not relieved: the bit already
cuts round them. Arcs and fillets have no corners, so a circle or a
rounded rectangle gets no notches. Each edge beside a corner must be at
least the bit's diameter long, or the notch would not fit; such a corner
is skipped with `relief_corner_too_tight`.

**How it is cut.** After all the pocket's passes, the bit visits the
corners, nearest first from where it finished. At each it plunges where
the wall pass turned the corner (material the pocket already cut to full
depth), cuts out to the corner and back, and steps down by the **tool's**
`step_down_mm` until it reaches the pocket's depth; then it lifts to safe Z
and moves to the next corner. The operation's own `step_down_mm` is not
used. A tool with no `step_down_mm` skips the relief with `relief_skipped`
(`check` warns `relief_needs_step_down` first).

A corner whose wall the pocket did not cut to full depth is skipped with
`relief_corner_not_cut`, rather than plunged into: with `wall_pass:
"none"` every corner is. With an allowance the notches are sized to the
wall the pocket leaves, the allowance in from the drawn one; a [rest
machining](#rest-machining) Pocket with corner relief is the usual way to
finish a roughed pocket with square corners.

A 40 × 30 mm pocket with a 6.35 mm bit and `"dogbone"` gets four notches,
each ending 3.175 mm from its corner along the bisector, so a 40 × 30 mm
block fits; with `"t_bone"` each notch runs along a 40 mm edge instead.

## Safety

Before a Pocket's toolpath is handed to the post-processor, DreamCarve
sweeps the bit along every cutting move and checks that the swept area
stays inside the pocket, the allowance off its walls, and out of every
island (safety rule S9), and that
it stays on the stock. Ramp and helix moves are checked on the bit's
centre, which must stay in the tool-centre area: the same rule, measured
the cheap way. A corner relief notch cuts past the walls on purpose, so
each may also cut within its own corner's notch: the bit at most touches
the corner, and bites at most its radius past one of the corner's two
walls, nowhere else. If not, export fails and nothing is written.

## Warnings

| Code | Meaning |
| --- | --- |
| `pocket_no_room` | The bit is wider than the pocket everywhere; that shape is not cut. |
| `pocket_island_too_close` | An island is nearer the wall or another island than the bit needs to pass (its diameter plus twice the allowance); the material between them stays. |
| `entry_fallback` | The helix or ramp asked for does not fit a shape; it uses a ramp or a plunge instead. |
| `helix_diameter_reduced` | The helix fits a shape only smaller than asked; it is cut at the size that fits. |
| `relief_skipped` | The tool has no `step_down_mm`, so no corner is relieved. |
| `relief_corner_too_tight` | A corner's edge is shorter than the bit is wide, or the bit cannot reach it; it keeps the bit's radius. |
| `relief_corner_not_cut` | The pocket did not cut to full depth where a corner's notch would start; that corner is skipped. |
| `rest_of_clears_nothing` | A rest machining Pocket's bit reaches nothing the earlier Pocket left; it cuts nothing. |

All codes are listed under [Warnings](warnings.md).
