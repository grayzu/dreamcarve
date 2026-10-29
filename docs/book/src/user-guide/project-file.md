# The project file

A DreamCarve project is one `.dcv` file. It holds the shapes, the stock,
the tools, the operations, and which post-processor to use, with your
settings for it. The tools are copies that travel with the project, and a
post-processor version never changes once released, so a DreamCarve
upgrade never changes what a saved project exports.

The file is JSON. `init` writes it; you edit it in a text editor.

DreamCarve writes the settings you edit first and the shapes last, so every
value you must fill in is in the first screen or two of the file. You may
reorder, reindent or reformat the file as you like: any layout of the same
JSON reads the same, and the line numbers DreamCarve reports follow your
layout.

## Rules for editing

- Every field must be present. A field DreamCarve does not know is an
  error, so a typo in a name is caught instead of ignored.
- An unset value is `null`, never left out.
- A number in a length or a feed is in the unit in the field's name:
  millimetres for `depth_mm`, millimetres per minute for `feed_mm_min`,
  even in an inch project. To use another unit, write the value as a
  string with its unit; see [Values with units](#values-with-units).
- JSON has no comments, and no comma after the last item in a list or
  object.
- Run `dreamcarve check` after each edit. It names the field and the value
  it does not accept, and where it is in the file (see
  [Finding a value](#finding-a-value)).

## Values with units

Any length (a field ending `_mm`) or feed (ending `_mm_min`) may be written
as a string with its unit, whatever the project's `display_units`:

```json
"depth_mm": "0.5 in",
"step_down_mm": "1/8 in",
"safe_z_mm": "5 mm",
"feed_mm_min": "60 in/min",
"plunge_mm_min": "500 mm/min",
```

- The number is a decimal (`0.5`, `-2`, `.25`), a fraction (`1/4`) or a
  whole number and a fraction (`1 1/4`). No exponent, no space around the
  `/`.
- A length takes `mm` or `in`; a feed takes `mm/min` or `in/min`. The space
  before the unit is optional. A length unit on a feed, or a feed unit on a
  length, is an error.
- A bare number is still in the field's unit: `"depth_mm": 12.7` and
  `"depth_mm": "12.7 mm"` are the same. A string without a unit is an
  error.
- Inches are converted to millimetres when the file is read (`× 25.4`),
  then rounded to 6 decimal places for a length and 3 for a feed, so the
  value is the exact decimal: `"0.5 in"` is 12.7 mm, `"3/16 in"` 4.7625 mm
  (not 4.762499999999999), `"1/64 in"` 0.396875 mm, and `"60 in/min"` 1524
  mm/min. Every check and every program is the same as with those
  numbers. A value in `mm` is kept exactly as you wrote it.
- Fields in other units take numbers only: `angle_deg`,
  `step_over_fraction`, `rpm`, `dwell_s`, and the vertices of the shapes.
- A tool library file takes units in its tools' fields too.

A command that saves the project, `export --post` or `tools add`, writes the
millimetre numbers back: `"0.5 in"` becomes `12.7`. Keep a copy if you want
your inch values.

An older DreamCarve does not read a value with a unit: it stops at that
field, naming its line and column.

## Finding a value

`init`, `check` and `export` start each message about one value with the
file, the line and the column of that value:

```text
coaster.dcv:10:21: error: stock.thickness_mm is not set (mm)
coaster.dcv:31:22: error: tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`
```

Line 10, column 21 is where the value starts, here the `null` to replace.
Lines and columns count from 1, and a tab counts as one column. Most
editors and terminals open the file at that spot when you click the
`coaster.dcv:10:21` part, or from the command line, as `code -g
coaster.dcv:10:21` or `vim +10 coaster.dcv`.

The field after the location says the same in words: `stock.thickness_mm`
is `thickness_mm` inside `stock`, and `tools[t1].feed_mm_min` is
`feed_mm_min` in the tool whose `id` is `t1`. `init` lists every value you
still have to set, and `check` and `export` report all of them in one run,
so you can fix them all in one edit. A message about no one value, such as
a tool id used twice, has no location.

A file DreamCarve cannot read at all, because a field is misspelled,
missing, or of the wrong kind, stops at the first such field. The message
names where it is, as keys and list positions counted from 0, and its line
and column:

```text
error: cannot load coaster.dcv: file does not match the project format at `operations[0].tabs.placement` (line 53, column 32): unknown field `bogus`, expected `count`
```

A missing field is reported at the object it belongs in, here
`post_processor`:

```text
error: cannot load coaster.dcv: file does not match the project format at `post_processor` (line 80, column 21): missing field `spindle_on_dwell_s`
```

## Coordinates

Project coordinates follow the machine, seen from above: X to the right, Y
away from you, Z up. The stock occupies X from 0 to `width_mm`, Y from 0 to
`height_mm`, and Z from 0 (the stock bottom) to `thickness_mm` (the stock
top). Heights such as a tab's height are measured up from the stock bottom.
Depths are measured down from the stock top.

No coordinate may lie more than 6000 mm (6 m) from the origin, after an
instance's transform is applied, and no stock side may be longer than
6000 mm. Each of the six transform numbers must also be between −6000 and
6000. The limit is there to catch typing mistakes, such as `1e12` for `12`,
and to keep the arithmetic exact; it is far larger than any hobby machine.
It is not your machine's travel: set that with soft limits on the
controller.

## Top level

The fields, in the order DreamCarve writes them:

| Field | Meaning |
| --- | --- |
| `version` | Format version. This build writes `20`. |
| `name` | The project name, written at the top of the program. `init` uses the drawing's file name. |
| `display_units` | `"mm"` or `"inch"`: the units of the G-code (`G21` or `G20`), and of the depths `export` prints. A bare number in this file stays in millimetres either way; write `"0.5 in"` to give a value in inches ([Values with units](#values-with-units)). |
| `flatten_tolerance_mm` | How far, at most, short straight moves may stray from a curve: more than 0 and at most 0.1. Default 0.01. `init` uses it when it imports the drawing; export uses it for the ramps of smooth tabs on curves. |
| `arc_fit_tolerance_mm` | How far, at most, an arc written in place of a run of short straight moves may stray from their corners. `init` writes 0.01; `null` turns arc fitting off. See [Arc fitting](#arc-fitting). |
| `stock` | The block of material. |
| `origin` | Where work zero is on the stock. |
| `heights` | The clearance, tool-change and park heights, each `null` until you set it. See [`heights`](#heights). |
| `tools` | The tools. |
| `operations` | The operations, in cutting order. |
| `tabs` | The project's placed tabs. See [`tabs`](#tabs). |
| `instances` | Placed copies of the shapes. |
| `post_processor` | Which bundled post-processor and version, and your settings for it. See [`post_processor`](#post_processor). |
| `definitions` | The shapes, keyed by id. Last, because it is most of the file. |

## Arc fitting

Circles and circular arcs in an SVG or DXF are imported as arcs and cut
as arcs ([Overview](overview.md#dreamcarve-init-svgdxf)). Other curves,
such as ellipses, Bézier curves and DXF splines, are imported as short
straight edges, within `flatten_tolerance_mm` of the curve, so a curve
becomes many `G1` moves. With `arc_fit_tolerance_mm` set, `export` looks
for runs of straight cutting moves, at one depth and one feed, whose
corners all lie on one circle within that distance, and writes each run
as `G2`/`G3` arcs instead: a 90 mm circle drawn as Bézier curves, 160
edges a lap, becomes 9 arcs.
Each arc sweeps at most 90°, starts and ends exactly on corners of the
run, and passes within the tolerance of every corner between and of
every straight move it replaces, along the whole move: a square's four
corners lie on one circle, but its sides are far from it, so a square
stays four lines. Curves flattened at `flatten_tolerance_mm` no coarser
than `arc_fit_tolerance_mm` have edges close enough to fit. Rapids,
plunges, moves that change depth, and arcs the toolpath already has are
left as they are, and the post-processor must write arcs
(`arc_interpolation`); a definition that cannot gets the straight moves.

A fitted arc is checked twice. Before it is written, DreamCarve rounds its
numbers as the post-processor will and runs the controller's own arc test
on them (GRBL and grblHAL reject an arc whose start and end are not the
same distance from its centre, beyond 0.005 mm). To pass that test,
DreamCarve may move the centre it writes by a unit or two in the last
place, so it then checks the arc as written against the straight moves
again, allowing the tolerance plus one unit in the last place, and checks
that the arc still stands for at least three straight moves the program
would write (a move too short to write doesn't count). An arc
that fails any of these checks is not written, and its whole run goes out as the
original straight moves, with the warning `arc_fit_fallback` naming the
operation and how many runs. After the program is written, the safety pass reads every arc back
and refuses the export unless it follows the toolpath's corners as above
(rule 8 in [Safety](safety.md)).

`init` writes `0.01`. Set it to `null` to export the straight moves as
they are; the program is then exactly what earlier versions wrote. A
larger value fits more freely but lets the cut stray further from the
drawing's corners, so leave it at or below `flatten_tolerance_mm`.

## `stock`

```json
"stock": {
  "width_mm": 110.0,
  "height_mm": 110.0,
  "thickness_mm": 11.8
}
```

| Field | Meaning |
| --- | --- |
| `width_mm` | Size along X. |
| `height_mm` | Size along Y. |
| `thickness_mm` | Size along Z. **Required.** Measure your material and enter the real value. |

`init` sizes the stock to the drawing plus 10 mm on each side. It rounds
the size and the shapes' placement up to 0.01 mm, so the file holds
`110.0`, not `110.0000025`. It cannot know your material, so it leaves
`thickness_mm` as `null`, and export refuses until you set it. Set the
operation's `depth_mm` with it: to cut through, the thickness plus the
operation's `through_cut_allowance_mm`.

## `origin`

Work zero: the point you set as X0 Y0 Z0 on the machine.

```json
"origin": {
  "xy": "bottom_left",
  "z": "stock_top"
}
```

`xy` is one of:

| Value | Work zero in X and Y |
| --- | --- |
| `bottom_left` | The corner at X 0, Y 0: front left. The default. |
| `bottom_right` | Front right. |
| `top_left` | Back left. |
| `top_right` | Back right. |
| `centre` | The centre of the stock. |
| `custom` | Any point, written as an object: `{ "custom": { "x_mm": 20.0, "y_mm": 5.0 } }`. |

`z` is `"stock_top"` (the default) or `"stock_bottom"`. With
`"stock_top"`, cutting moves have negative Z.

## `heights`

Where the program crosses between parts, changes tools and ends. `init`
writes all three as `null`, and a project with all three `null` exports
as it did before format version 12.

```json
"heights": {
  "clearance_mm": 20.0,
  "tool_change_machine_z_mm": -5.0,
  "park": { "x_mm": 0.0, "y_mm": 0.0 }
}
```

| Field | Meaning |
| --- | --- |
| `clearance_mm` | Millimetres above the stock top. Moves between parts and between operations, a tool change, and the end rise to it; moves between the passes of one part stay at the operation's `safe_z_mm`. At least every enabled operation's `safe_z_mm`, or `check` reports `clearance_below_safe_z`. `null`: each operation's `safe_z_mm`, as before. |
| `tool_change_machine_z_mm` | Millimetres, machine coordinates. Before each tool-change stop and at the end of each program, the program lifts to it with `G53 G0 Z…`. Below 0 (`machine_z_not_below_home`), and only on a post-processor that documents `G53`: grblHAL, GRBL and LinuxCNC (`machine_z_needs_g53` on Mach3, UCCNC and generic). Needs a homed machine. |
| `park` | `{ "x_mm": …, "y_mm": … }`, project coordinates. Where the program ends, at the clearance height (or the highest safe Z), before the lift. `null`: the program ends over the last cut. |

See [Post-processors](post-processors.md#heights-clearance-tool-change-and-park)
for what the program does with them and how to choose the machine Z.

## `definitions` and `instances`

A **definition** is a shape as it was drawn. An **instance** places a
definition on the stock. `init` makes one of each per SVG path or DXF shape, and moves
the instances so the drawing sits 10 mm in from the stock's bottom-left
corner.

```json
"d1": {
  "id": "d1",
  "name": "coaster",
  "profiles": [
    {
      "vertices": [
        { "x": 95.0, "y": 50.0, "bulge": -1.0 },
        { "x": 5.0, "y": 50.0, "bulge": -1.0 }
      ],
      "closed": true
    }
  ]
}
```

(Numbers shortened here; the file holds them in full, with the few
millionths of a millimetre the SVG reader's rounding leaves. The coaster's
circle is two half-circle arcs, clockwise.)

DreamCarve writes each vertex on one line, as above, to keep a drawing
short; the numbers keep their full precision. A vertex spread over several
lines, as other JSON tools write it, reads the same.

A definition's `profiles` are its outlines. Each vertex has `x` and `y` in
millimetres and a `bulge`: 0 for a straight line to the next vertex, or the
tangent of a quarter of the arc's sweep angle, positive for
counter-clockwise. `closed` says whether the outline returns to its start;
Pocket and an Inside or Outside Profile cut only closed outlines, an On
line Profile and an Engrave cut open ones too, and a Left or Right
Profile cuts only open ones.

| Instance field | Meaning |
| --- | --- |
| `id` | The id operations use to target it. |
| `definition` | The id of the definition it places. |
| `transform` | Six numbers `[a, b, c, d, e, f]` that move a point (x, y) to (a·x + c·y + e, b·x + d·y + f). `[1, 0, 0, 1, e, f]` moves the shape by `e` in X and `f` in Y. |
| `name` | Shown in messages. |
| `visible`, `locked` | For the future editor. This build ignores them. |
| `z_top_mm`, `z_bottom_mm` | The feature's top and bottom, mm above the material bottom, for future operations. `null` means not set, which is how `init` writes them. This build does not use them, but when both are set, `z_bottom_mm` above `z_top_mm` is an error. |

To move a shape on the stock, change `e` and `f`.

## `tools`

```json
{
  "id": "t1",
  "name": "Set me",
  "vendor": null,
  "product_id": null,
  "number": null,
  "cutter": "end_mill",
  "diameter_mm": 6.35,
  "flutes": 2,
  "feed_mm_min": 1500.0,
  "plunge_mm_min": 500.0,
  "rpm": 18000,
  "step_down_mm": 4.0,
  "step_over_fraction": null
}
```

| Field | Meaning |
| --- | --- |
| `id` | The id operations use. |
| `name` | Your name for it, shown in messages and the program. `init` names it "Set me". |
| `vendor` | Who makes the tool, such as `"IDC Woodcraft"`, or `null`. [Fusion import](tool-library.md#fusion-360-tool-libraries) fills it in; `init` writes `null`. No program uses it. |
| `product_id` | The vendor's part number for it, such as `"DC-14"`, or `null`. Fusion import fills it in; `init` writes `null`. No program uses it. |
| `number` | The tool's number, for `T<n>` in programs and `-T<n>` in file names: a whole number of 1 or more, or `null` for the tool's position in this list (the first tool is 1). `init` writes `null`. Two tools used by enabled operations that end up with the same number are an error; so is 0. Set it to match your machine's tool table: with `5`, programs write `T5` (`M61 Q5`, `T5 M6`, the `( Tool 5: … )` comment, `Fit T5`) and, with one program per tool run, name its programs `<stem>-<k>-T5.nc`. |
| `cutter` | `"end_mill"`. Also accepted: `"ball_nose"`, `"engraver"`, `"drill"`, and `{ "v_bit": { "included_angle_deg": 60.0 } }`, but an Outside, Inside, Left or Right Profile, a Pocket, and a V-Carve's clearing pass, cut only with `"end_mill"`: they offset by half the diameter, which is wrong for any other bit. A V-Carve cuts only with a `v_bit`, whose `included_angle_deg` (above 0, below 180) sets its depth and its cone height, `diameter_mm / (2 × tan(angle / 2))`. On line, Drill and Engrave allow any cutter. |
| `diameter_mm` | Cutting diameter. Profile and Pocket offset the path by half of it. Above 50 mm is a warning. |
| `flutes` | Number of flutes. Not used yet. |
| `feed_mm_min` | Feed rate while cutting. **Required.** At most 20 000. |
| `plunge_mm_min` | Feed rate straight down, and onto and off square tabs. **Required.** At most 20 000; above `feed_mm_min` is a warning. |
| `rpm` | Spindle speed. **Required**, except with [manual spindle control](post-processors.md#manual-spindle-control), where it is only shown in the pause message. Above 60 000 is a warning. |
| `step_down_mm` | Depth per pass, used when the operation sets none. **Required** when an enabled operation that uses the tool sets none (a Profile, Pocket or Engrave without its own `step_down_mm`, or a peck Drill without `peck_depth_mm`); `init` leaves it `null`. Corner relief also steps down at this value: with it unset the relief is skipped, with a warning. |
| `step_over_fraction` | Greater than 0 and at most 1, used when the operation sets none. For an end mill, the step-over of a Pocket, or of a V-Carve's clearing pass, as a fraction of the diameter. For a V-bit, the step-over of a V-Carve's floor rings, as a fraction of the bit's cut width at the flat depth: the ridges it leaves are this times the flat depth high, so 0.1 suits a V-bit better than an end mill's 0.4 ([V-Carve: the V-bit floor](v-carve.md#the-v-bit-floor)). `init` leaves it `null`. |

DreamCarve has no default feeds. A tool that an enabled operation uses and
that has an unset feed rate, plunge rate or (with automatic spindle control)
spindle speed is an error in `check`, and `export` refuses until it is set.
So is an unset `step_down_mm` when an enabled operation using the tool sets
no `step_down_mm` of its own; it is reported once, at the tool.
A tool no enabled operation uses is a warning instead, and its unset values
are not reported; enable an operation that uses it and they are errors
again. The limits above also apply only to a tool an enabled operation
uses; see [Values that look wrong](profile.md#values-that-look-wrong).

## `operations`

A list of operations, cut in order. `kind` says which: `"profile"`,
`"pocket"`, `"drill"`, `"bore"` (from format version 18), `"engrave"` or
`"v_carve"` (from format version 20). `init` writes one Profile:

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

Each field is explained in [Profile operation](profile.md). `side` is
`"outside"`, `"inside"` or `"on_line"`, or, from format version 10,
`"left"` or `"right"`, which cut beside open paths (see
[Left and right](profile.md#left-and-right)). A Profile's
`tabs` is a **tab request**: tabs to place on every outline it cuts, or
`null` for none. It is cut as the tabs it places, and the first
`dreamcarve tabs` command that saves the project writes them into the
project's [`tabs`](#tabs) list and sets the request to `null`
([Tabs: tab requests](tabs.md#tab-requests)). `tabs.on_holes` came with
format version 9: `false`
leaves the holes among the operation's outlines without tabs, which is
what `init` writes; a project upgraded from version 8 gets `true`, so its
holes keep their tabs. `tabs.avoid_corners` came with format version 14:
`true`, as `init` writes it, slides each tab off a corner of the bit's
path; a project upgraded from version 13 gets `false`, so its tabs stay
where they were ([Tabs: tabs off corners](tabs.md#tabs-off-corners)). `init` leaves `depth_mm` as `null`; export refuses until
you set it. It also leaves `step_down_mm` as `null`, so the operation uses
the tool's depth per pass; set it only to override the tool's.

`machining_order`, `entry`, `lead` and `corner_relief` came with format
version 8 and are shared with the other kinds; `last_pass`, Profile only,
came with format version 13, and `start`, Profile only, with version 14. This build reads and checks them all, and a
Profile cuts them as set:

| Field | Values |
| --- | --- |
| `machining_order` | `"full_depth_first"` (each shape to full depth before the next) or `"pass_first"` (each pass across every shape before the next, deeper pass). |
| `entry` | How the tool reaches each pass depth: `"plunge"`, `{ "ramp": { "angle_deg": 15.0 } }` (angle greater than 0 and at most 45), or `{ "helix": { "diameter_fraction": 0.8, "angle_deg": 5.0 } }` (fraction greater than 0 and at most 1, same angle range). A helix needs material the cut removes, so it is an error on an On line, Left or Right Profile. `init` writes the 15° ramp; a project upgraded from version 7 keeps `"plunge"`, so it cuts as before. See [Entry](profile.md#entry). |
| `lead` | `null`, or `{ "radius_mm": 2.0, "sweep_deg": 90.0, "lead_out": true, "overcut_mm": 0.0 }`: an arc tangent to the cut on the way in and, with `lead_out`, out. Radius greater than 0; sweep greater than 0 and at most 180; overcut 0 or more. Profile only; not cut On line, Left or Right. See [Leads](profile.md#leads). |
| `corner_relief` | `"none"`, `"dogbone"` or `"t_bone"`: a notch in each internal corner a round tool cannot reach. Pocket and Profile. Steps down at the tool's `step_down_mm`, so that must be set. See [Pocket: corner relief](pocket.md#corner-relief) and [Profile: corner relief](profile.md#corner-relief). |
| `last_pass` | `null`, or `{ "stock_to_leave_mm": 0.5, "thickness_mm": null, "reverse": false }`: every pass but the last leaves `stock_to_leave_mm` on the wall (more than 0, at most a third of the bit's diameter), and the last pass cuts to size at full depth. `thickness_mm` is `null` for the usual last pass, or how deep the last pass cuts (more than 0, at most `depth_mm`); It may be no thicker than the step-down in effect. `reverse` cuts the last pass the other way round. All three keys are needed. Inside, Outside, Left and Right Profiles; not On line. `init` writes `null`, and a project upgraded from version 12 gets `null`, so it cuts as before. See [Last pass](profile.md#last-pass). |
| `start` | `"auto"`, `"first_vertex"` or `{ "near": { "x_mm": 0.0, "y_mm": 0.0 } }`: where each closed outline's cut starts, at the point of the bit's path nearest the outline's first vertex as drawn or nearest that project point. Tabs a request lays are measured from there, a lead arrives there or is dropped, and an entry with no room there falls back rather than moving. Open paths ignore it. `init` writes `"auto"`, and a project upgraded from version 13 gets `"auto"`, so it cuts as before. Profile only. See [Start point](profile.md#start-point). |

A Pocket, Drill, Bore or Engrave has `kind`, `id`, `name`, `enabled`, `targets`,
`tool`, `depth_mm`, `through_cut_allowance_mm` and `safe_z_mm` with the
same meaning, plus its own fields:

| Kind | Own fields |
| --- | --- |
| `"pocket"` | `strategy`: `"offset"` or `{ "raster": { "angle_deg": 45.0 } }` (0 or more, less than 180); `wall_pass`: `"last"`, `"first"` or `"none"`; `step_down_mm` and `step_over_fraction` (`null` for the tool's); `direction`; `stock_to_leave_mm` (0 or more, material left on the walls); `rest_of` (`null`, or the id of an earlier enabled Pocket with the same `targets`, whose leftovers this one clears); `entry`; `corner_relief`; `machining_order`. The tool must be an `"end_mill"`. |
| `"drill"` | `cycle`: `"simple"` or `{ "peck": { "peck_depth_mm": 2.0, "retract": "full" } }`, where `peck_depth_mm` may be `null` for the tool's `step_down_mm` and `retract` is `"full"` or `{ "chip_break": { "gap_mm": 0.5 } }` (greater than 0); `dwell_s` (0 or more; 0 is none); `retract_mm` (0 or more, above the hole top); `allow_tool_wider_than_hole` (`false` skips a circle smaller than the bit, with a warning; `true` drills it at the bit's size; from format version 17, `false` on upgrade). A cutter other than `"drill"` or `"end_mill"` is a warning. See [Drill: holes smaller than the bit](drill.md#holes-smaller-than-the-bit). |
| `"bore"` | `pitch_mm` (how far the helix goes down a turn; `null` for the tool's `step_down_mm`, and no more than it); `direction`; `stock_to_leave_mm` (0 or more, material left on the wall). The tool must be an `"end_mill"`. From format version 18. See [Bore](bore.md). |
| `"engrave"` | `step_down_mm` (`null` for the tool's); `alternate_direction` (`true` or `false`); `machining_order`. Any cutter; a V-bit deeper than its cone is tall is a warning. |
| `"v_carve"` | No `depth_mm`: the depth follows the shape and the bit's angle. `strategy`: `"medial"` or `{ "offset": { "step_over_mm": 0.5, "direction": "climb" } }` (step-over greater than 0); `clearing`: a list of clearing passes, each `{ "tool": "t2", "strategy": "offset", "direction": "climb", "entry": "plunge" }` with a Pocket's `strategy`, `direction` and `entry`, `[]` for none; `flat_depth_mm` (`null`, or greater than 0, within the bit's cone height); `start_depth_mm` (0 or more); `start_plane_cleared_externally` (`true` or `false`); `step_over_fraction` (`null` for the tool's; greater than 0 and at most 1); `machining_order`. The tool must be a V-bit; each clearing pass's tool an `"end_mill"`. From format version 20. See [V-Carve](v-carve.md). |

Each kind has a chapter of its own: [Pocket](pocket.md), [Drill](drill.md),
[Bore](bore.md), [Engrave](engrave.md) and [V-Carve](v-carve.md).

## `tabs`

The project's placed tabs: short stretches of material left uncut on a
part's outline to hold it in the stock. `init` writes an empty list, `[]`; `dreamcarve
tabs place` fills it. Each tab names one outline of one instance and a
point, and every enabled Profile that cuts that outline rises over it:

```json
"tabs": [
  {
    "id": "tab1",
    "instance": "i1",
    "profile": 0,
    "x_mm": 84.0648714595208,
    "y_mm": 15.935129303418655,
    "width_mm": 8.0,
    "height_mm": 3.0,
    "shape": "rect",
    "holds_slug": false
  }
]
```

| Field | Meaning |
| --- | --- |
| `id` | The tab's id, used by `tabs clear --tab`. Two tabs may not share one. |
| `instance` | The id of the instance whose outline holds the tab. It must exist. |
| `profile` | Which of that instance's definition's `profiles`, counted from 0. It must exist. |
| `x_mm`, `y_mm` | The tab's point, in the definition's coordinates, before the instance's `transform`: the tab moves with the instance. A Profile centres the tab at the point of its tool path nearest this point. |
| `width_mm` | Length of the tab along the outline. More than 0. |
| `height_mm` | Height of the tab top above the stock bottom. More than 0. |
| `shape` | `"rect"` (square shoulders) or `"smooth"` (a triangle). |
| `holds_slug` | Whether the tab is meant to hold the slug of the hole it is on: `true`, `false` or `null`. It changes no cut, only the `tab_on_hole` warning. `tabs place --on-holes`, and a tab request with `on_holes`, write `true` for each tab they put on a hole, and `check` and `export` then do not warn for it; every other tab they place gets `false`, and a `false` tab on a hole warns on every `check` and `export`. `null` is what a project upgraded from version 14 gets: such a tab warns as it did before, unless it lies on the bit's path round the hole of the Profile that cuts it, where only placing with `on_holes` put one. Set it to `true` by hand to keep a hole's slug without the warning. See [Tabs: parts with holes](tabs.md#parts-with-holes). |

[Tabs](tabs.md) explains placing, listing and removing them.

## `post_processor`

Which bundled post-processor the project uses, and your settings for it:

```json
"post_processor": {
  "id": "grblhal",
  "definition_version": 4,
  "file_extension": "nc",
  "spindle_control": "automatic",
  "spindle_on_dwell_s": 3.0,
  "tool_change_mode": "separate_programs",
  "decimal_places": {
    "mm": 3,
    "inch": 4
  },
  "use_canned_cycles": false
}
```

The definition itself, the controller's dialect, is built into DreamCarve,
one unchanging file per version. To switch to another post-processor, or to
the latest version of this one, run `dreamcarve export <dcv> --post <id>`
rather than editing `id` or `definition_version`. Each field is explained in
[Post-processors](post-processors.md#settings).

## Versions

`version` is the format version. When this build of DreamCarve opens an
older file, it upgrades it in memory; the file on disk changes only when
DreamCarve saves it (`export --post`, `tools add`, or a `tabs` command
that changes the project). A file from a newer DreamCarve is refused.

| Version | Change |
| --- | --- |
| 0 | Before the first release. |
| 1 | Adds `flatten_tolerance_mm`, set to 0.01 on upgrade. |
| 2 | Adds `post_processor.feed_speed.spindle_control`, set to `"automatic"` on upgrade. |
| 3 | Writes `origin.xy` and each tool's `cutter` as a plain string (`"bottom_left"`, `"end_mill"`), or as a one-key object when the choice carries values (`{ "custom": { "x_mm": 20.0, "y_mm": 5.0 } }`). Older files tagged them with `kind`, as in `{ "kind": "bottom_left" }`; the upgrade rewrites them. |
| 4 | Lets `stock.thickness_mm` and each operation's `depth_mm` be `null` (not set), which is how `init` writes them. The upgrade keeps the values an older file holds. |
| 5 | Adds `post_processor.tool_change.mode`, set to `"separate_programs"` on upgrade: a project with more than one tool exports one program per tool run. Set `"m6"` (grblHAL only) for the one program with `M6` that older versions wrote. See [Post-processors](post-processors.md#projects-with-more-than-one-tool). |
| 6 | Lets each instance's `z_top_mm` and `z_bottom_mm` be `null` (not set), which is how `init` writes them. The upgrade keeps the values an older file holds, so a file `init` wrote before keeps its placeholder `z_top_mm` of 12.0 until you change it. Phase 1 ignores both. |
| 7 | `post_processor` names a bundled post-processor and its version, and holds only your five settings, instead of a full copy of the definition. The upgrade keeps your settings, and works only when the rest of the copy is exactly version 1 of the bundled definition; otherwise the file is not loaded, and the message lists each difference and the block to paste instead. See [Post-processors](post-processors.md#files-from-before-version-7). |
| 8 | Phase 2. Adds `arc_fit_tolerance_mm` (set to 0.01 on upgrade), each tool's `number` (`null`), each Profile's `machining_order` (`"full_depth_first"`), `entry` (`"plunge"`), `lead` (`null`) and `corner_relief` (`"none"`), and `post_processor.use_canned_cycles` (`false`): the values that make an upgraded project cut exactly as before. Adds the `"pocket"`, `"drill"` and `"engrave"` operation kinds. |
| 9 | Adds `on_holes` to each Profile's `tabs`, set to `true` on upgrade (#109). A Profile now cuts its holes without tabs unless `on_holes` is `true`; before, every outline it cut had tabs, so the upgrade keeps them. See [Tabs: parts with holes](tabs.md#parts-with-holes). |
| 10 | Adds the `"left"` and `"right"` values of a Profile's `side` (#207), which cut beside open paths. An older file holds neither, so the upgrade changes nothing else; an older DreamCarve refuses a version 10 file rather than misread a side it does not know. See [Left and right](profile.md#left-and-right). |
| 11 | Adds `tabs`, the project's placed tabs, empty on upgrade (#105). A Profile's `tabs` setting stays as a tab request: it is cut as before, and the next `dreamcarve tabs` command that saves the project places it. See [Tabs](tabs.md). |
| 12 | Adds `heights` after `origin`, with `clearance_mm`, `tool_change_machine_z_mm` and `park` all `null` on upgrade (#283), so the project exports as before. See [`heights`](#heights). |
| 13 | Adds each Profile's `last_pass`, `null` on upgrade (#284), so every pass cuts on the part's path as before. See [Last pass](profile.md#last-pass). |
| 14 | Adds `avoid_corners` to each Profile's tab request, `false` on upgrade, and each Profile's `start`, `"auto"` on upgrade (#285), so the tabs are laid and each cut starts where it did. See [Tabs: tabs off corners](tabs.md#tabs-off-corners) and [Profile: start point](profile.md#start-point). |
| 15 | Adds `holds_slug` to each placed tab, `null` on upgrade (#302): whether a tab on a hole is meant to hold its slug. A version 14 file did not record it, so `null` keeps the warnings it gave, and the program does not change. See [`tabs`](#tabs). |
| 16 | Adds each tool's `vendor` and `product_id` after its `name`, `null` on upgrade: who makes the tool and the maker's part number, which Fusion import fills in. No program uses them, so the project exports as before. See [`tools`](#tools). |
| 17 | Adds `allow_tool_wider_than_hole` to each Drill, `false` on upgrade. A Drill now skips a circle smaller than its bit, with the warning `drill_hole_too_small`, where an older build drilled it at the bit's size: an upgraded project with such holes gives a different program. Set it to `true` to drill them as before. See [Drill: holes smaller than the bit](drill.md#holes-smaller-than-the-bit). |
| 18 | Adds the `"bore"` operation kind, which cuts circles bigger than the bit to size. An older file holds none, so the upgrade changes nothing else; an older DreamCarve refuses a version 18 file rather than misread a kind it does not know. See [Bore](bore.md). |
| 19 | Renames `allowance_mm` to `stock_to_leave_mm` in each Pocket, each Bore and each Profile's `last_pass` (#394): the material they leave on the wall is their stock to leave, and `through_cut_allowance_mm` is the only allowance. The upgrade keeps each value, so the project exports as before; saving writes the new name. |
| 20 | Adds the `"v_carve"` operation kind, with its `clearing` passes, flat depth and start depth ([V-Carve](v-carve.md)). An older file holds none, so the upgrade changes nothing else; an older DreamCarve refuses a version 20 file rather than misread a kind it does not know. This build also refuses a `flatten_tolerance_mm` above 0.1 mm, in a file of any version. |

## What `check` reports

Errors stop export. Warnings do not.

| Errors | Warnings |
| --- | --- |
| Two tools, instances, operations or tabs with the same id | An operation is disabled |
| An id that refers to nothing, or a tab on an outline its instance does not have | An open path that no enabled Engrave or On line, Left or Right Profile targets |
| A value you must set that is `null`: the stock's `thickness_mm`, an operation's `depth_mm`, and for a tool an enabled operation uses, its `feed_mm_min` and `plunge_mm_min`, and its `rpm` unless spindle control is manual | A tool no enabled operation uses (its unset values are not reported) |
| A size, depth, rate, safe Z, or tab size that is 0 or less where it must be more | |
| A value that is not a number | Any [toolpath warning](warnings.md#toolpath-generation): a shape the tool does not fit, an entry or lead that does not fit, a duplicate shape, tabs that overlap or do nothing, a tab on a hole or one no enabled Profile cuts round |
| A coordinate, transform number or stock side beyond 6000 mm (see [Coordinates](#coordinates)) | |
| `post_processor.tool_change_mode` is `"m6"` but the post-processor has no `M6` (every one but grblHAL) | |
| A cut deeper than the stock plus its through-cut allowance | |
| Shapes that cross each other, or a shape that crosses itself | |
| A Profile whose tool does not fit between two lines of one shape, Inside or On line, or that would remove a whole part narrower than the bit On line ([Bands narrower than the bit](profile.md#bands-narrower-than-the-bit)) | |
| A Left or Right Profile whose cut would cross another of its lines or reach the part side of one, or one whose path ends in a segment too short for the bit, or for the bit plus the last pass stock to leave, on the inside of the turn before it ([Left and right](profile.md#left-and-right)) | |
| A post-processor `file_extension` that is not 1 to 8 letters or digits, or is `dcv` | |
| A feed or plunge rate above 20 000 mm/min | A step-down deeper than the tool's diameter |
| An Outside, Inside, Left or Right Profile, or a Pocket, whose tool is not an `"end_mill"` | A plunge rate above the feed rate |
| An enabled operation with no `targets` | A spindle speed above 60 000 RPM |
| Two tools used by enabled operations with the same number, set or by position; a tool `number` of 0 | A tool diameter above 50 mm |
| A Pocket's `rest_of` that is not an earlier enabled Pocket with the same `targets` | A Drill whose cutter is not `"drill"` or `"end_mill"` |
| A `step_over_fraction`, lead, entry, raster angle, dwell, retract, peck or chip-break value outside its range (see [`operations`](#operations)) | `corner_relief` on a tool with no `step_down_mm` (the relief is skipped) |
| A helix `entry` on an On line, Left or Right Profile | An Engrave with a V-bit deeper than the bit's cone |
| A `last_pass` on an On line Profile; a `last_pass.thickness_mm` that is 0 or less, deeper than `depth_mm` or (for an enabled Profile) thicker than its step-down; a `last_pass.stock_to_leave_mm` that is 0 or less or more than a third of the tool's diameter | |
| A Profile with a `last_pass` whose earlier passes cannot follow the drawing: a band, neck, slot, notch or hole that fits the bit but not the bit plus the stock to leave on both sides, a tip so narrow that the last pass would take off more than a third of the bit, or a stock pass that would come within the bit's radius of another part or run inside it; on a Left or Right Profile, a stock pass that would come within the bit's radius plus the stock to leave of another line or the strip it keeps ([Last pass](profile.md#last-pass)) | |
| `post_processor.use_canned_cycles` is `true` but the post-processor has no drilling cycles (grblHAL, GRBL, UCCNC and generic) | |
| A `flatten_tolerance_mm` above 0.1 | A V-Carve's clearing pass with an end mill wider than the pass before it (`clearing_not_descending`) |
| A V-Carve whose tool is not a V-bit, or a V-bit angle not above 0 and below 180 | An earlier Pocket containing a V-Carve's targets at a depth other than its start depth (`start_depth_mismatch`) |
| A V-Carve whose flat depth, plus a start depth no earlier Pocket clears, is deeper than the bit's cone height (`v_bit_cone_exceeded`) | |
| A V-Carve with clearing passes and no flat depth, or a pass whose tool is not an `"end_mill"` or does not exist | |
| A `start_depth_mm` below 0, or above 0 with no enabled earlier Pocket containing the targets and `start_plane_cleared_externally` `false` (`start_depth_uncleared`) | |

The checks in the rows from "A feed or plunge rate" down look only at
enabled operations and the tools they use. See [Values that look
wrong](profile.md#values-that-look-wrong).
