# The project file

A DreamCarve project is one `.dcv` file. It holds the shapes, the stock,
the tools, the operations, and a full copy of the post-processor. Because
the copy travels with the project, a DreamCarve upgrade never changes what
a saved project exports.

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
coaster.dcv:9:21: error: stock.thickness_mm is not set (mm)
coaster.dcv:22:22: error: tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`
```

Line 9, column 21 is where the value starts, here the `null` to replace.
Lines and columns count from 1, and a tab counts as one column. Most
editors and terminals open the file at that spot when you click the
`coaster.dcv:9:21` part, or from the command line, as `code -g
coaster.dcv:9:21` or `vim +9 coaster.dcv`.

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
error: cannot load coaster.dcv: file does not match the project format at `post_processor` (line 75, column 21): missing field `vendor`
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
| `version` | Format version. This build writes `5`. |
| `name` | The project name, written at the top of the program. `init` uses the SVG's file name. |
| `display_units` | `"mm"` or `"inch"`: the units of the G-code (`G21` or `G20`). The values in this file stay in millimetres either way. |
| `flatten_tolerance_mm` | How far, at most, short straight moves may stray from a curve. Default 0.01. `init` uses it when it imports the drawing; export uses it for the ramps of smooth tabs on curves. |
| `stock` | The block of material. |
| `origin` | Where work zero is on the stock. |
| `tools` | The tools. |
| `operations` | The operations, in cutting order. |
| `instances` | Placed copies of the shapes. |
| `post_processor` | The G-code dialect. See [Post-processors](post-processors.md). |
| `definitions` | The shapes, keyed by id. Last, because it is most of the file. |

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
definition on the stock. `init` makes one of each per SVG path, and moves
the instances so the drawing sits 10 mm in from the stock's bottom-left
corner.

```json
"d1": {
  "id": "d1",
  "name": "coaster",
  "profiles": [
    {
      "vertices": [
        { "x": 95.0, "y": 50.0, "bulge": 0.0 },
        { "x": 94.964, "y": 48.2, "bulge": 0.0 },
        …
      ],
      "closed": true
    }
  ]
}
```

(Numbers shortened here; the file holds them in full. The coaster's circle
is 160 short straight lines.)

DreamCarve writes each vertex on one line, as above, to keep a drawing
short; the numbers keep their full precision. A vertex spread over several
lines, as other JSON tools write it, reads the same.

A definition's `profiles` are its outlines. Each vertex has `x` and `y` in
millimetres and a `bulge`: 0 for a straight line to the next vertex, or the
tangent of a quarter of the arc's sweep angle, positive for
counter-clockwise. `closed` says whether the outline returns to its start;
Profile cuts only closed outlines.

| Instance field | Meaning |
| --- | --- |
| `id` | The id operations use to target it. |
| `definition` | The id of the definition it places. |
| `transform` | Six numbers `[a, b, c, d, e, f]` that move a point (x, y) to (a·x + c·y + e, b·x + d·y + f). `[1, 0, 0, 1, e, f]` moves the shape by `e` in X and `f` in Y. |
| `name` | Shown in messages. |
| `visible`, `locked` | For the future editor. Phase 1 ignores them. |
| `z_top_mm`, `z_bottom_mm` | For future operations. Phase 1 ignores them. |

To move a shape on the stock, change `e` and `f`.

## `tools`

```json
{
  "id": "t1",
  "name": "Set me",
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
| `cutter` | `"end_mill"`. Also accepted: `"ball_nose"`, `"engraver"`, and `{ "v_bit": { "included_angle_deg": 60.0 } }`, but an Outside or Inside Profile cuts only with `"end_mill"`: it offsets by half the diameter, which is wrong for any other bit. On line, any cutter is allowed. |
| `diameter_mm` | Cutting diameter. Profile offsets the path by half of it. Above 50 mm is a warning. |
| `flutes` | Number of flutes. Not used in phase 1. |
| `feed_mm_min` | Feed rate while cutting. **Required.** At most 20 000. |
| `plunge_mm_min` | Feed rate straight down, and onto and off square tabs. **Required.** At most 20 000; above `feed_mm_min` is a warning. |
| `rpm` | Spindle speed. **Required**, except with [manual spindle control](post-processors.md#manual-spindle-control), where it is only shown in the pause message. Above 60 000 is a warning. |
| `step_down_mm` | Depth per pass, used when the operation sets none. **Required** when an enabled operation that uses the tool sets none; `init` leaves it `null`. |
| `step_over_fraction` | For future operations. Phase 1 ignores it. |

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

A list of operations, cut in order. Phase 1 has one kind, Profile:

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
  }
}
```

Each field is explained in [Profile operation](profile.md), and `tabs` in
[Tabs](tabs.md). `init` leaves `depth_mm` as `null`; export refuses until
you set it. It also leaves `step_down_mm` as `null`, so the operation uses
the tool's depth per pass; set it only to override the tool's.

## `post_processor`

A full copy of the post-processor definition. To switch to another bundled
one, run `dreamcarve export <dcv> --post <id>` rather than editing it. The
fields you may want to change are in [Post-processors](post-processors.md).

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

## What `check` reports

Errors stop export. Warnings do not.

| Errors | Warnings |
| --- | --- |
| Two tools, instances or operations with the same id | An operation is disabled |
| An id that refers to nothing | An open path, which Profile does not cut |
| A value you must set that is `null`: the stock's `thickness_mm`, an operation's `depth_mm`, and for a tool an enabled operation uses, its `feed_mm_min` and `plunge_mm_min`, and its `rpm` unless spindle control is manual | A tool no enabled operation uses (its unset values are not reported) |
| A size, depth, rate, safe Z, or tab size that is 0 or less where it must be more | |
| A value that is not a number | A shape too small for the tool, a duplicate shape, or tabs that overlap or do nothing |
| A coordinate, transform number or stock side beyond 6000 mm (see [Coordinates](#coordinates)) | |
| `post_processor.tool_change.mode` is `"m6"` but the post-processor has no `M6` (GRBL) | |
| A cut deeper than the stock plus its through-cut allowance | |
| Shapes that cross each other, or a shape that crosses itself | |
| A post-processor `file_extension` that is not 1 to 8 letters or digits, or is `dcv` | |
| A feed or plunge rate above 20 000 mm/min | A step-down deeper than the tool's diameter |
| An Outside or Inside Profile whose tool is not an `"end_mill"` | A plunge rate above the feed rate |
| An enabled operation with no `targets` | A spindle speed above 60 000 RPM |
| A post-processor `line_number_increment` of 0 | A tool diameter above 50 mm |

Apart from the line-number increment, the checks in the last four rows look
only at enabled operations and the tools they use. See [Values that look
wrong](profile.md#values-that-look-wrong).
