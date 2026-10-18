# The project file

A DreamCarve project is one `.dcv` file. It holds the shapes, the stock,
the tools, the operations, and a full copy of the post-processor. Because
the copy travels with the project, a DreamCarve upgrade never changes what
a saved project exports.

The file is JSON. `init` writes it; you edit it in a text editor.

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
  it does not accept.

## Coordinates

Project coordinates follow the machine, seen from above: X to the right, Y
away from you, Z up. The stock occupies X from 0 to `width_mm`, Y from 0 to
`height_mm`, and Z from 0 (the stock bottom) to `thickness_mm` (the stock
top). Heights such as a tab's height are measured up from the stock bottom.
Depths are measured down from the stock top.

## Top level

| Field | Meaning |
| --- | --- |
| `version` | Format version. This build writes `2`. |
| `name` | The project name, written at the top of the program. `init` uses the SVG's file name. |
| `display_units` | `"mm"` or `"inch"`: the units of the G-code (`G21` or `G20`). The values in this file stay in millimetres either way. |
| `flatten_tolerance_mm` | How far, at most, short straight moves may stray from a curve. Default 0.01. `init` uses it when it imports the drawing; export uses it for the ramps of smooth tabs. |
| `stock` | The block of material. |
| `origin` | Where work zero is on the stock. |
| `definitions` | The shapes, keyed by id. |
| `instances` | Placed copies of the shapes. |
| `tools` | The tools. |
| `operations` | The operations, in cutting order. |
| `post_processor` | The G-code dialect. See [Post-processors](post-processors.md). |

## `stock`

```json
"stock": {
  "width_mm": 110.0,
  "height_mm": 110.0,
  "thickness_mm": 12.0
}
```

| Field | Meaning |
| --- | --- |
| `width_mm` | Size along X. |
| `height_mm` | Size along Y. |
| `thickness_mm` | Size along Z. Measure your material and enter the real value. |

`init` sizes the stock to the drawing plus 10 mm on each side, 12 mm thick.
It rounds the size and the shapes' placement up to 0.01 mm, so the file
holds `110.0`, not `110.0000025`.
If you change `thickness_mm`, change the operation's `depth_mm` to match.

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
  "step_down_mm": null,
  "step_over_fraction": null
}
```

| Field | Meaning |
| --- | --- |
| `id` | The id operations use. |
| `name` | Your name for it, shown in messages and the program. `init` names it "Set me". |
| `cutter` | `"end_mill"`. Also accepted: `"ball_nose"`, `"engraver"`, and `{ "v_bit": { "included_angle_deg": 60.0 } }`; Profile uses only the diameter. |
| `diameter_mm` | Cutting diameter. Profile offsets the path by half of it. |
| `flutes` | Number of flutes. Not used in phase 1. |
| `feed_mm_min` | Feed rate while cutting. **Required.** |
| `plunge_mm_min` | Feed rate straight down, and onto and off square tabs. **Required.** |
| `rpm` | Spindle speed. **Required**, except with [manual spindle control](post-processors.md#manual-spindle-control), where it is only shown in the pause message. |
| `step_down_mm` | Default depth per pass, used when the operation sets none. |
| `step_over_fraction` | For future operations. Phase 1 ignores it. |

DreamCarve has no default feeds. A tool with an unset rate is a warning in
`check`, and `export` fails when an operation uses that tool.

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
  "step_down_mm": 4.0,
  "direction": "climb",
  "through_cut_allowance_mm": 0.0,
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
[Tabs](tabs.md).

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

## What `check` reports

Errors stop export. Warnings do not.

| Errors | Warnings |
| --- | --- |
| Two tools, instances or operations with the same id | An operation is disabled |
| An id that refers to nothing | A tool has no feed rate, plunge rate or spindle speed (export still fails if the tool is used) |
| A size, depth, rate, safe Z, or tab size that is 0 or less where it must be more | An open path, which Profile does not cut |
| A value that is not a number | A shape too small for the tool, a duplicate shape, or tabs that overlap or do nothing |
| A cut deeper than the stock plus its through-cut allowance | |
| Shapes that cross each other, or a shape that crosses itself | |
| A post-processor `file_extension` that is not 1 to 8 letters or digits, or is `dcv` | |
