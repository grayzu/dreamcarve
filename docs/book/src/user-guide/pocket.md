# Pocket operation

A **Pocket** clears all the material inside a closed shape down to a
depth: a recess for an inlay, a tray, a lid seat. Shapes inside the pocket
are **islands**, left standing at full height; a shape inside an island is
a pocket again. This build clears with the **offset** strategy: concentric
rings stepping in from the wall by the stepover, as VCarve's "Offset"
pocketing does.

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
| `strategy` | `"offset"`. `{ "raster": { "angle_deg": … } }` loads and validates, but this build refuses to export it; see [Not yet in this build](#not-yet-in-this-build). |
| `wall_pass` | `"last"` (as `init` would write it), `"first"` or `"none"`. See [Wall pass](#wall-pass). |
| `depth_mm` | How deep to cut, measured down from the stock top. **Required.** |
| `step_down_mm` | The most to cut in one pass. `null` uses the tool's `step_down_mm`; if both are `null`, export refuses. |
| `step_over_fraction` | The stepover as a fraction of the tool diameter, more than 0 and at most 1. `null` uses the tool's `step_over_fraction`; if both are `null`, export refuses. See [Stepover](#stepover). |
| `direction` | `"climb"` or `"conventional"`. See [Direction](#direction). |
| `allowance_mm` | Material to leave on the walls. Must be 0 in this build. |
| `rest_of` | Rest machining. Must be `null` in this build. |
| `entry` | `"plunge"`. A ramp or helix plunges instead, with a warning. |
| `corner_relief` | `"none"`. Dogbone or T-bone is skipped, with a warning. |
| `machining_order` | `"full_depth_first"` or `"pass_first"`, as for [Profile](profile.md#machining-order). |
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
safe Z. Each ring starts at its corner nearest where the previous one
ended, so those links are short. Between passes, and between separate
pockets, the bit lifts to safe Z and plunges at the plunge rate.

A 100 × 50 mm rectangle with a 6.35 mm bit at 40 % stepover gives a wall
ring 93.65 × 43.65 mm across its centre line and eight more rings inside
it, each 2.54 mm in from the one outside it.

### Islands

An island grown by the tool radius may reach the pocket wall, or another
island. Then the tool-centre area splits into separate pieces, each with
its own rings, and the bit lifts to safe Z to go from one to the other:
it never cuts across an island. The material in the gap is narrower than
the bit and stays; `pocket_island_too_close` names the island and the
gap.

A shape inside an island is a pocket of its own, cut after the pocket
around the island.

## Wall pass

- `"last"`: each pass ends with the wall ring and the island rings, so
  the walls are cut once more after the clearing, cleanly.
- `"first"`: each pass starts with them.
- `"none"`: they are not cut. The pocket comes out one stepover undersize
  on every wall, and the walls keep the clearing rings' marks (VCarve's
  "no profile pass"). A pocket too small for a second ring is then not
  cut at all.

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
same kind on both.

## Not yet in this build

These load and validate, so a project can carry them, but:

- `strategy` `raster`, `allowance_mm` above 0 and `rest_of` are refused
  at `check` and `export` ("not supported by this build yet"): cutting
  something else would make the wrong pocket. Set them to `"offset"`, 0
  and `null` for now.
- `entry` `ramp` or `helix` plunges instead (`entry_fallback`), and
  `corner_relief` `dogbone` or `t_bone` is skipped (`relief_skipped`).
  The pocket is still the one asked for.

## Safety

Before a Pocket's toolpath is handed to the post-processor, DreamCarve
sweeps the bit along every cutting move and checks that the swept area
stays inside the pocket and out of every island (safety rule S9), and that
it stays on the stock. If not, export fails and nothing is written.

## Warnings

| Code | Meaning |
| --- | --- |
| `pocket_no_room` | The bit is wider than the pocket everywhere; that shape is not cut. |
| `pocket_island_too_close` | An island is nearer the wall or another island than the bit is wide; the material between them stays. |
| `entry_fallback` | The entry asked for is not in this build; the pass plunges. |
| `relief_skipped` | The corner relief asked for is not in this build. |

All codes are listed under [Warnings](warnings.md).
