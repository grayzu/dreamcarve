# V-Carve operation

A **V-Carve** drives a V-bit along the middle of a closed shape, at the
depth where the bit's two sloping sides touch both of the shape's walls.
Where the shape is narrow the bit runs shallow; where it is wide it runs
deep; at a sharp corner it rises to the surface. So the cut is exactly
the shape you drew, with walls that slope at the bit's angle, and every
corner and serif comes out sharp, which no end mill can do. It is how
signs and lettering are carved.

A V-Carve can also stop at a **flat depth**: the walls slope down to it
and the wide parts get a flat floor. That is how a sign's background is
cut away to leave a raised panel or raised letters; one or more end mills
can clear that floor first (**clearing passes**).

Add it to the project's `operations` list by hand; `init` writes only a
Profile. A complete V-Carve, the sign background of the
[V-carved sign quick start](../quick-start/sign.md):

```json
{
  "kind": "v_carve",
  "id": "op2",
  "name": "Background",
  "enabled": true,
  "targets": [ "i2", "i3" ],
  "tool": "t4",
  "strategy": "medial",
  "clearing": [
    { "tool": "t2", "strategy": "offset", "direction": "climb", "entry": { "ramp": { "angle_deg": 15.0 } } }
  ],
  "flat_depth_mm": 2.5,
  "start_depth_mm": 0.0,
  "start_plane_cleared_externally": false,
  "step_over_fraction": 0.1,
  "machining_order": "full_depth_first",
  "through_cut_allowance_mm": 0.0,
  "safe_z_mm": 15.0
}
```

A V-Carve of lettering with no flat depth is the same with
`"clearing": []`, `"flat_depth_mm": null` and `"step_over_fraction": null`.

Put a V-Carve **before** the Profile that cuts the part out, as any cut
inside the part. A V-Carve has no `depth_mm`: its depth comes from the
shape and the bit's angle.

## Settings

| Field | Meaning |
| --- | --- |
| `id` | The operation's id, shown in messages. |
| `name` | Your name for it, written in the program. |
| `enabled` | `false` leaves the operation out of the program, with a warning. |
| `targets` | The ids of the instances to carve. Every closed outline of each one takes part, nested as for a Pocket: an outline inside another is an island, left standing. Open paths are skipped with the warning `vcarve_target_open`. Outlines that cross are an error. |
| `tool` | The id of the tool. It must be a V-bit, `"cutter": { "v_bit": { "included_angle_deg": 90.0 } }`, with an angle above 0° and below 180°. See [Choosing a bit angle](#choosing-a-bit-angle). |
| `strategy` | `"medial"`, along the shape's middle (see [Medial lines and depth](#medial-lines-and-depth)), or `{ "offset": { "step_over_mm": 0.5, "direction": "climb" } }`, in rings stepping in from the outline (see [Offset strategy](#offset-strategy)). |
| `clearing` | The clearing passes, in the order they cut: a list of `{ "tool", "strategy", "direction", "entry" }`, largest end mill first. `[]` for none. Needs a flat depth. See [Clearing passes](#clearing-passes). |
| `flat_depth_mm` | `null` to carve every part of the shape to its full depth, or the depth of a flat floor below the start plane, more than 0. See [Flat depth](#flat-depth). |
| `start_depth_mm` | How far below the stock top the carve starts, 0 or more. 0 carves the stock top. See [Start depth](#start-depth). |
| `start_plane_cleared_externally` | `true` says another program cleared the start plane, when no Pocket in this project does. `false` otherwise. See [Start depth](#start-depth). |
| `step_over_fraction` | How far apart the V-bit's rings on the flat floor are, as a fraction of the bit's cut width at the flat depth, more than 0 and at most 1. `null` uses the tool's `step_over_fraction`. Needed only with a flat depth. See [The V-bit floor](#the-v-bit-floor). |
| `machining_order` | `"full_depth_first"` or `"pass_first"`, as for a Pocket. A V-Carve is cut in one pass, so both give the same program. |
| `through_cut_allowance_mm` | How far below the stock bottom the cut may go, as every operation has. 0 for a carve. |
| `safe_z_mm` | The height above the stock top the bit travels at between cuts. |

## Medial lines and depth

A shape's **medial lines** are the lines that run down its middle, every
point the same distance from the two nearest walls. The bit follows them.
At each point it goes down until its sides touch both walls: for a bit of
included angle `α`, a point whose walls are `w` apart is cut

```text
depth = w / (2 × tan(α / 2))
```

deep. A 5 mm wide stroke is cut 2.5 mm deep with a 90° bit and 4.330 mm
deep with a 60° bit. So:

- the cut is as wide as the shape at every point, and its walls slope at
  the bit's angle;
- at every sharp outside corner, the medial lines run up into the corner
  and the bit rises to the surface there: a serif ends in a point;
- a round end is cut deepest at its centre, where the bit comes down;
- a circle is one plunge at its centre.

Outlines nested inside a target are **islands**, left standing, and an
outline inside an island is carved again, as a Pocket nests shapes: the
letter O carves its ring and leaves its middle; a rectangle drawn round
lettering, with the letters as targets too, carves the background and
leaves the letters standing.

Within one operation the shapes are cut nearest first, starting from the
first in drawing order, and a shape before any shape round it. Between
two lines of one shape, the bit stays down if the move between them runs
only through material the carve has already removed, and lifts to safe Z
otherwise.

`check` and `export` print the deepest cut of each V-Carve:

```text
V-Carve op3 `Lettering`: deepest cut 1.658 mm below the stock top, no flat depth.
```

## Choosing a bit angle

The angle sets how deep a given width is cut, and the bit's diameter and
angle set how deep it can cut at all. The **cone height** is the depth at
which the bit's sides reach its full diameter:

```text
cone height = diameter / (2 × tan(α / 2))
```

Below that, the straight shank would cut, so DreamCarve refuses any carve
that would go deeper (`v_bit_cone_exceeded`). Put the other way, without
a flat depth no part of a shape may be wider than the bit's diameter.

The common 1/4 in (6.35 mm) bits:

| Angle | Cone height | Cut width per mm of depth | A 5 mm stroke is cut |
| --- | --- | --- | --- |
| 15° | 24.1 mm | 0.26 mm | 19.0 mm deep (too deep for most stock) |
| 30° | 11.8 mm | 0.54 mm | 9.33 mm deep |
| 60° | 5.5 mm | 1.15 mm | 4.330 mm deep |
| 90° | 3.175 mm | 2.00 mm | 2.5 mm deep |

- **90°** is the usual sign bit: shallow, wide cuts, and the bit's cone
  covers the most floor at a flat depth. Strokes up to 6.35 mm wide carve
  without a flat depth.
- **60°** carves the same lettering deeper, with steeper walls that
  catch more shadow. Good for lettering 10 to 25 mm tall.
- **30°** and **15°** are for small, fine lettering. Their cuts get deep
  fast: check the deepest cut `check` prints against your stock.

Lettering wider than the bit at any point (a large, bold face) needs a
flat depth, a wider bit, or a larger angle.

## Flat depth

With `flat_depth_mm` set to `F`, no point is cut deeper than `F` below the
start plane. Where a shape is wider than `2 × F × tan(α / 2)` (5 mm for a
90° bit at 2.5 mm), the walls slope down to `F` and the rest is a flat
floor:

- the **floor boundary** is the shape inset by `F × tan(α / 2)`; the bit
  runs along it at `F`, so the sloped wall meets the floor in a clean
  line (the **floor trace**);
- from each corner of the floor, the bit runs up the corner's medial line
  to the shape's corner at the surface, so corners are still sharp;
- narrower parts of the shape are carved as with no flat depth;
- the floor inside the boundary is cleared, by the clearing passes if
  there are any, and by the V-bit itself where they cannot reach.

The cone height check applies to `F`: a flat depth deeper than the cone
height is refused, whatever the shape.

### The V-bit floor

With no clearing passes, the V-bit clears the floor itself, in rings at
`F`: the bit cuts `2 × F × tan(α / 2)` wide there (5 mm for a 90° bit at
2.5 mm), and the rings are `step_over_fraction` of that apart. A pointed
bit leaves a small ridge between two rings, of height

```text
ridge = step_over_fraction × F
```

So the step-over is required whenever there is a floor to cut: the
operation's `step_over_fraction`, or else the tool's; with neither,
export stops with an error. `check` prints the ridge height:

```text
V-Carve op2 `Background`: deepest cut 2.500 mm below the stock top, clamped at a flat depth of 2.500 mm; V-bit floor ridges 1.000 mm; a clearing pass or a smaller step_over_fraction flattens them.
```

That is the sign's background with the tool's 0.4, which is sized for an
end mill: 1 mm ridges. **Use 0.1** on the V-Carve: at `F` 2.5 mm that
leaves ridges 0.25 mm high, which sanding removes. Near a
corner of a ring, where two rings are farther apart, the ridge can be up
to about a sixth higher than the figure printed. A V-bit floor at 0.1 is
many rings, and slow over a large floor: clear large floors with an end
mill instead.

## Clearing passes

A clearing pass clears the flat floor with a flat end mill before the
V-bit carves, as VCarve's "clearance tools" do. Each pass names a tool and
three settings, as a Pocket has them:

```json
"clearing": [
  { "tool": "t2", "strategy": "offset", "direction": "climb", "entry": { "ramp": { "angle_deg": 15.0 } } },
  { "tool": "t3", "strategy": { "raster": { "angle_deg": 0.0 } }, "direction": "climb", "entry": "plunge" }
],
```

| Field | Meaning |
| --- | --- |
| `tool` | The id of a flat end mill (`"end_mill"`). Any other cutter is the error `clearing_needs_end_mill`; an id with no tool is `clearing_dangling_tool`. |
| `strategy` | `"offset"` or `{ "raster": { "angle_deg": 0.0 } }`, as a [Pocket's](pocket.md#settings). |
| `direction` | `"climb"` or `"conventional"`. |
| `entry` | `"plunge"`, `{ "ramp": { "angle_deg": 15.0 } }` or `{ "helix": { "diameter_fraction": 0.8, "angle_deg": 5.0 } }`, as a [Pocket's](pocket.md#entry). |

Each pass is cut as a Pocket of the floor: the floor boundary, at the
start depth plus `F`, with its wall pass last and no stock to leave. Its
step-down and step-over come from its tool, so set the tool's
`step_down_mm` and `step_over_fraction`.

- **Largest end mill first.** The passes cut in list order, and each one
  cuts only what the passes before it could not reach, as [rest
  machining](pocket.md#rest-machining) does: a 6.35 mm end mill clears
  the floor, then a 3.175 mm one only the corners it left.
- **A wider pass after a narrower one reaches nothing.** A wide end mill
  fits nowhere a narrow one before it could not. Such a list warns
  `clearing_not_descending`, and the wide pass, which cuts nothing, also
  warns `clearing_no_room`.
- **A pass that can reach nothing** (an end mill too wide for the floor)
  warns `clearing_no_room` and has no program.
- **The V-bit still traces the whole floor boundary**, so the floor and
  the sloped walls meet cleanly, and clears only what no end mill
  reached: the floor's corners and parts narrower than the smallest end
  mill. `check` then prints the ridge height "where no clearing pass
  reaches".
- A carve with no flat depth has no floor, so clearing passes are the
  error `clearing_needs_flat_depth`.

Export writes each pass as a run of its own tool, before the V-bit's run.
A carve with one clearing pass is two tool runs: `op2 (clearing t2)`, then
`op2`, in the summary and the preview's legend, and `Background clearing
t2` in the program's comment. With one program per tool run (the
default), each pass is a program of its own, so you change bits between
them; see [Post-processors](post-processors.md#projects-with-more-than-one-tool).

## Start depth

Every depth of a carve counts from its **start plane**, `start_depth_mm`
below the stock top. 0 carves the stock top. To carve the floor of a
pocket, cut the pocket first with a Pocket of the same depth, earlier in
the list, and set `start_depth_mm` to that depth: the carve's outline is
then where the bit's sides meet the pocket's floor.

- An enabled Pocket before the carve whose area contains the carve's
  targets, at another depth, warns `start_depth_mismatch`, naming the
  Pocket and both depths. The carve still exports, but its outline is not
  where you drew it: set the start depth to the Pocket's depth.
- A start depth above 0 with no such Pocket is the error
  `start_depth_uncleared`: the project cannot see the material the carve
  would cut into.
- If another program cleared the start plane (a separate project, or
  hand work), set `start_plane_cleared_externally` to `true` and it
  exports. Then, since the project cannot see that pocket, the bit's
  flank is counted from the stock top: the start depth plus the carve's
  own depth must fit within the cone height. The same holds with a
  mismatched Pocket.

Whatever the start depth, the bit rapids down to 1 mm above the stock top
and plunges from there at the plunge rate, never by rapid into the
pocket.

## One pass

A carve is cut in one pass, whatever its depth, as PureCutCNC cuts. There
is no step-down: the tool's `step_down_mm` is not used. For a deep carve
in hard material, slow the feed, or use the offset strategy, which takes
the same V in lighter rings.

## Offset strategy

With `"strategy": { "offset": { "step_over_mm": 0.5, "direction": "climb" } }`
the carve is cut in rings stepping in from the outline, `step_over_mm`
apart, as PureCutCNC's "V-Carve offset" does. Each ring is cut at the
depth where the bit's side touches the outline: a ring inset `d` is
`d / tan(α / 2)` deep, so a 90° bit cuts the ring 0.5 mm in at 0.5 mm, the
next at 1 mm, and so on, until the rings vanish or reach the flat depth.
`direction` sets the way round every ring.

The finished shape is the same V as the medial strategy's, cut as a
series of light passes, for hard material or a deep carve. But its
corners are rounded by the step-over: the rings do not run to a point.
Use a small `step_over_mm` for crisp corners. With a flat depth, the rings
stop at `F`, and the floor trace and the V-bit floor (or the clearing
passes) clear the floor, as for the medial strategy.

Between rings the bit stays down where the move between them runs along
the V surface (from one ring's corner to the next ring's, down the corner
of a straight-walled shape), and lifts otherwise, such as across a
narrow neck. A step-over of 0 or less, or so small that a shape would
need more than 10000 rings, is an error.

## Safety

Every V-Carve program passes the [safety](safety.md) rules, and one more,
S17, which holds the V-bit's cone, not just its tip, to the carve: the
cone never passes through a wall of the shape below the start plane, the
tip never goes below the flat depth, the flank is never engaged deeper
than the cone height, and the only cut above the start plane is a
straight descent. A clearing pass is held as a Pocket of the floor. See
[Safety](safety.md), rule 14.

## Errors

These stop `check` and `export`. Each names the operation, and the field,
shape or tool concerned.

| Code | Message says | What to do |
| --- | --- | --- |
| `v_carve_needs_v_bit` | "operation … carves with tool …, an end mill; a V-Carve's depth follows the angle of a V-bit, so use a V-bit" | Give the operation a V-bit. |
| `v_bit_angle_range` | "tool … has an included angle of … degrees; a V-Carve needs a V-bit angle above 0 and below 180 degrees …" | Fix the tool's `included_angle_deg`. |
| `v_bit_cone_exceeded` | "operation … engages tool … … mm deep, but the bit's cone is only … mm tall; below that the shank cuts …", or at export "operation … carves … with its V-bit's flanks engaged … mm deep …" | The flat depth (plus a start depth no Pocket clears), or with no flat depth the widest part of a shape, needs more than the cone. Set or lower `flat_depth_mm`, use a wider bit or a larger angle, or clear the start depth with a Pocket. |
| `clearing_needs_flat_depth` | "operation … has clearing passes but no flat_depth_mm …" | Set a flat depth, or remove the passes. |
| `clearing_needs_end_mill` | "clearing pass … of operation … cuts with tool …, …; a clearing pass is cut as a Pocket, which only a flat end mill cuts to size …" | Use an end mill for the pass. |
| `clearing_dangling_tool` | "clearing pass … of operation … uses tool …, which does not exist" | Fix the pass's `tool`. |
| `start_depth_negative` | "operation … has start_depth_mm … mm; the start plane is a depth below the stock top, 0 or more" | Set 0 or more. |
| `start_depth_uncleared` | "operation … starts … mm below the stock top, but no enabled Pocket before it contains its targets; …" | Add a Pocket of that depth before the carve, or set `start_plane_cleared_externally` to `true` if another program clears it. |

Export also stops when a carve with a floor to cut has no step-over
("operation … has no step_over_fraction and tool … sets none"), and when
an offset carve's `step_over_mm` is unusable.

## Warnings

| Code | When |
| --- | --- |
| `vcarve_target_open` | A target is an open path, which has no area; it is skipped. |
| `vcarve_no_medial_axis` | A shape is too thin for a medial line to be found; it is not carved. |
| `vcarve_sampling_budget` | A shape is so long for its width that its medial lines are found coarsely; fine details may be smoothed. |
| `clearing_no_room` | A clearing pass can reach nothing the passes before it left; it has no program. |
| `clearing_not_descending` | A clearing pass's end mill is wider than the one before it. |
| `start_depth_mismatch` | An earlier Pocket contains the targets, at a depth other than the start depth. |

[Warnings](warnings.md) says what to do about each.

## Limits

- The bit is an ideal point. A real V-bit's tip has a small flat, so the
  finest corners come out a little rounded and the carve a little deeper
  than its width needs; nothing models it yet.
- Open paths are not carved: cut a line at a fixed depth with an
  [Engrave](engrave.md).
- No depth passes (VCarve's "Multiple Depths"); one pass, whatever the
  depth.
- No corner sharpening, and no clearing with a ball nose or engraving
  bit.
- Text is not drawn by DreamCarve: convert lettering to outlines in your
  drawing program (Inkscape: Path → Object to Path) before `init`.
