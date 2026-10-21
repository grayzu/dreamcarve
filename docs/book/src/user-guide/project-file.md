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
- Every length is in millimetres and every feed in millimetres per minute,
  even in an inch project. The unit is in each field's name: `depth_mm`,
  `feed_mm_min`.
- JSON has no comments, and no comma after the last item in a list or
  object.
- Run `dreamcarve check` after each edit. It names the field and the value
  it does not accept, and where it is in the file (see
  [Finding a value](#finding-a-value)).

## Finding a value

`init`, `check` and `export` start each message about one value with the
file, the line and the column of that value:

```text
coaster.dcv:10:21: error: stock.thickness_mm is not set (mm)
coaster.dcv:24:22: error: tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`
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
error: cannot load coaster.dcv: file does not match the project format at `operations[0].tabs.placement` (line 48, column 32): unknown field `bogus`, expected `count`
```

A missing field is reported at the object it belongs in, here
`post_processor`:

```text
error: cannot load coaster.dcv: file does not match the project format at `post_processor` (line 75, column 21): missing field `spindle_on_dwell_s`
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
| `version` | Format version. This build writes `10`. |
| `name` | The project name, written at the top of the program. `init` uses the drawing's file name. |
| `display_units` | `"mm"` or `"inch"`: the units of the G-code (`G21` or `G20`). The values in this file stay in millimetres either way. |
| `flatten_tolerance_mm` | How far, at most, short straight moves may stray from a curve. Default 0.01. `init` uses it when it imports the drawing; export uses it for the ramps of smooth tabs on curves. |
| `arc_fit_tolerance_mm` | How far, at most, an arc written in place of a run of short straight moves may stray from their corners. `init` writes 0.01; `null` turns arc fitting off. See [Arc fitting](#arc-fitting). |
| `stock` | The block of material. |
| `origin` | Where work zero is on the stock. |
| `tools` | The tools. |
| `operations` | The operations, in cutting order. |
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
same distance from its centre, beyond 0.005 mm); an arc that would fail is
not written, and its whole run goes out as the original straight moves,
with the warning `arc_fit_fallback` naming the operation and how many
runs. After the program is written, the safety pass reads every arc back
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
| `number` | The tool's number, for `T<n>` in programs and `-T<n>` in file names: a whole number of 1 or more, or `null` for the tool's position in this list (the first tool is 1). `init` writes `null`. Two tools used by enabled operations that end up with the same number are an error; so is 0. Set it to match your machine's tool table: with `5`, programs write `T5` (`M61 Q5`, `T5 M6`, the `( Tool 5: … )` comment, `Fit T5`) and, with one program per tool run, name its programs `<stem>-<k>-T5.nc`. |
| `cutter` | `"end_mill"`. Also accepted: `"ball_nose"`, `"engraver"`, `"drill"`, and `{ "v_bit": { "included_angle_deg": 60.0 } }`, but an Outside, Inside, Left or Right Profile, and a Pocket, cut only with `"end_mill"`: they offset by half the diameter, which is wrong for any other bit. On line, Drill and Engrave allow any cutter. |
| `diameter_mm` | Cutting diameter. Profile and Pocket offset the path by half of it. Above 50 mm is a warning. |
| `flutes` | Number of flutes. Not used yet. |
| `feed_mm_min` | Feed rate while cutting. **Required.** At most 20 000. |
| `plunge_mm_min` | Feed rate straight down, and onto and off square tabs. **Required.** At most 20 000; above `feed_mm_min` is a warning. |
| `rpm` | Spindle speed. **Required**, except with [manual spindle control](post-processors.md#manual-spindle-control), where it is only shown in the pause message. Above 60 000 is a warning. |
| `step_down_mm` | Depth per pass, used when the operation sets none. **Required** when an enabled operation that uses the tool sets none (a Profile, Pocket or Engrave without its own `step_down_mm`, or a peck Drill without `peck_depth_mm`); `init` leaves it `null`. Corner relief also steps down at this value: with it unset the relief is skipped, with a warning. |
| `step_over_fraction` | Step-over of a Pocket as a fraction of the diameter, greater than 0 and at most 1, used when the Pocket sets none. `init` leaves it `null`. |

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
`"pocket"`, `"drill"` or `"engrave"`. `init` writes one Profile:

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
    "on_holes": false
  },
  "machining_order": "full_depth_first",
  "entry": { "ramp": { "angle_deg": 15.0 } },
  "lead": null,
  "corner_relief": "none"
}
```

Each field is explained in [Profile operation](profile.md), and `tabs` in
[Tabs](tabs.md). `side` is `"outside"`, `"inside"` or `"on_line"`, or,
from format version 10, `"left"` or `"right"`, which cut beside open paths
(see [Left and right](profile.md#left-and-right)). `tabs.on_holes` came with format version 9: `false`
leaves the holes among the operation's outlines without tabs, which is
what `init` writes; a project upgraded from version 8 gets `true`, so its
holes keep their tabs. `init` leaves `depth_mm` as `null`; export refuses until
you set it. It also leaves `step_down_mm` as `null`, so the operation uses
the tool's depth per pass; set it only to override the tool's.

The last four fields came with format version 8 and are shared with the
other kinds. This build reads and checks them. A Profile cuts
`machining_order`, `entry`, `lead` and `corner_relief` as set:

| Field | Values |
| --- | --- |
| `machining_order` | `"full_depth_first"` (each shape to full depth before the next) or `"pass_first"` (each pass across every shape before the next, deeper pass). |
| `entry` | How the tool reaches each pass depth: `"plunge"`, `{ "ramp": { "angle_deg": 15.0 } }` (angle greater than 0 and at most 45), or `{ "helix": { "diameter_fraction": 0.8, "angle_deg": 5.0 } }` (fraction greater than 0 and at most 1, same angle range). A helix needs material the cut removes, so it is an error on an On line, Left or Right Profile. `init` writes the 15° ramp; a project upgraded from version 7 keeps `"plunge"`, so it cuts as before. See [Entry](profile.md#entry). |
| `lead` | `null`, or `{ "radius_mm": 2.0, "sweep_deg": 90.0, "lead_out": true, "overcut_mm": 0.0 }`: an arc tangent to the cut on the way in and, with `lead_out`, out. Radius greater than 0; sweep greater than 0 and at most 180; overcut 0 or more. Profile only; not cut On line, Left or Right. See [Leads](profile.md#leads). |
| `corner_relief` | `"none"`, `"dogbone"` or `"t_bone"`: a notch in each internal corner a round tool cannot reach. Pocket and Profile. Steps down at the tool's `step_down_mm`, so that must be set. See [Pocket: corner relief](pocket.md#corner-relief) and [Profile: corner relief](profile.md#corner-relief). |

A Pocket, Drill or Engrave has `kind`, `id`, `name`, `enabled`, `targets`,
`tool`, `depth_mm`, `through_cut_allowance_mm` and `safe_z_mm` with the
same meaning, plus its own fields:

| Kind | Own fields |
| --- | --- |
| `"pocket"` | `strategy`: `"offset"` or `{ "raster": { "angle_deg": 45.0 } }` (0 or more, less than 180); `wall_pass`: `"last"`, `"first"` or `"none"`; `step_down_mm` and `step_over_fraction` (`null` for the tool's); `direction`; `allowance_mm` (0 or more, material left on the walls); `rest_of` (`null`, or the id of an earlier enabled Pocket with the same `targets`, whose leftovers this one clears); `entry`; `corner_relief`; `machining_order`. The tool must be an `"end_mill"`. |
| `"drill"` | `cycle`: `"simple"` or `{ "peck": { "peck_depth_mm": 2.0, "retract": "full" } }`, where `peck_depth_mm` may be `null` for the tool's `step_down_mm` and `retract` is `"full"` or `{ "chip_break": { "gap_mm": 0.5 } }` (greater than 0); `dwell_s` (0 or more; 0 is none); `retract_mm` (0 or more, above the hole top). A cutter other than `"drill"` or `"end_mill"` is a warning. |
| `"engrave"` | `step_down_mm` (`null` for the tool's); `alternate_direction` (`true` or `false`); `machining_order`. Any cutter; a V-bit deeper than its cone is tall is a warning. |

Each kind has a chapter of its own: [Pocket](pocket.md), [Drill](drill.md)
and [Engrave](engrave.md).

## `post_processor`

Which bundled post-processor the project uses, and your settings for it:

```json
"post_processor": {
  "id": "grblhal",
  "definition_version": 3,
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
DreamCarve saves it (`export --post`). A file from a newer DreamCarve is
refused.

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

## What `check` reports

Errors stop export. Warnings do not.

| Errors | Warnings |
| --- | --- |
| Two tools, instances or operations with the same id | An operation is disabled |
| An id that refers to nothing | An open path that no enabled Engrave or On line, Left or Right Profile targets |
| A value you must set that is `null`: the stock's `thickness_mm`, an operation's `depth_mm`, and for a tool an enabled operation uses, its `feed_mm_min` and `plunge_mm_min`, and its `rpm` unless spindle control is manual | A tool no enabled operation uses (its unset values are not reported) |
| A size, depth, rate, safe Z, or tab size that is 0 or less where it must be more | |
| A value that is not a number | Any [toolpath warning](warnings.md#toolpath-generation): a shape the tool does not fit, an entry or lead that does not fit, a duplicate shape, tabs that overlap or do nothing |
| A coordinate, transform number or stock side beyond 6000 mm (see [Coordinates](#coordinates)) | |
| `post_processor.tool_change_mode` is `"m6"` but the post-processor has no `M6` (every one but grblHAL) | |
| A cut deeper than the stock plus its through-cut allowance | |
| Shapes that cross each other, or a shape that crosses itself | |
| A Profile whose tool does not fit between two lines of one shape, Inside or On line, or that would remove a whole part narrower than the bit On line ([Bands narrower than the bit](profile.md#bands-narrower-than-the-bit)) | |
| A Left or Right Profile whose cut would cross another of its lines or reach the part side of one ([Left and right](profile.md#left-and-right)) | |
| A post-processor `file_extension` that is not 1 to 8 letters or digits, or is `dcv` | |
| A feed or plunge rate above 20 000 mm/min | A step-down deeper than the tool's diameter |
| An Outside, Inside, Left or Right Profile, or a Pocket, whose tool is not an `"end_mill"` | A plunge rate above the feed rate |
| An enabled operation with no `targets` | A spindle speed above 60 000 RPM |
| Two tools used by enabled operations with the same number, set or by position; a tool `number` of 0 | A tool diameter above 50 mm |
| A Pocket's `rest_of` that is not an earlier enabled Pocket with the same `targets` | A Drill whose cutter is not `"drill"` or `"end_mill"` |
| A `step_over_fraction`, lead, entry, raster angle, dwell, retract, peck or chip-break value outside its range (see [`operations`](#operations)) | `corner_relief` on a tool with no `step_down_mm` (the relief is skipped) |
| A helix `entry` on an On line, Left or Right Profile | An Engrave with a V-bit deeper than the bit's cone |
| `post_processor.use_canned_cycles` is `true` but the post-processor has no drilling cycles (grblHAL, GRBL, UCCNC and generic) | |

The checks in the rows from "A feed or plunge rate" down look only at
enabled operations and the tools they use. See [Values that look
wrong](profile.md#values-that-look-wrong).
