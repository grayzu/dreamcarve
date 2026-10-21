# Warnings

`check` and `export` print every warning as

```text
warning: <code>: <message>
```

and, when the warning is about one value in the project file, after that
value's position, as in `coaster.dcv:26:14: warning: spindle_speed_high: …`.
The code is a stable `snake_case` word you can search for here; the
message names the operation, shape or tool concerned and says what to do.
A warning never stops export: the program is still written and still
passes the [safety rules](safety.md). An error does stop it; errors are
described in the chapter of the thing they are about.

The tables list every code, grouped by where it comes from.

## Cut order between operations

Operations always run in the order the project lists them, never
regrouped by tool or reordered ([Profile: cut order between
operations](profile.md#cut-order-between-operations)). These warnings
say when that order is likely a mistake.

| Code | Message says | What to do |
| --- | --- | --- |
| `cut_inside_freed_part` | "`<shape>` lies inside `<outline>`, which operation `<id>` cuts free of the stock first, so this cut would run in a loose part" | An earlier Profile cuts through the stock around (or inside) an outline, with no tabs, and this operation then cuts a shape inside it: the cut runs in a part or slug that is no longer held. Move this operation before the one named, or give that Profile tabs. |
| `tool_change_avoidable` | "tool `<id>` (`<name>`) is used by operations `<a>`, `<b>`, which do not run one after another, so it is fitted more than once" | Operations using one tool are separated by an operation using another, so the tool is fitted twice. Put them next to each other if the cut order allows; DreamCarve never moves them for you. |

## Toolpath generation

Printed after the operation's id: `warning: <code>: operation `op1`: …`.

| Code | Message says | What to do |
| --- | --- | --- |
| `open_path_skipped` | "… is an open path; Profile and Pocket cut only closed shapes, so it is not cut" | A target is not a closed outline. Close it in the drawing, or leave it untargeted. |
| `pocket_no_room` | "tool … is wider than the pocket of … somewhere it must pass, so that shape is not cut" | The bit does not fit in the pocket at all. Use a smaller bit or widen the shape ([Pocket](pocket.md#warnings)). |
| `pocket_island_too_close` | "in the pocket of …, island profile … is … mm from …, closer than the … mm tool … is wide, so the material between them is left uncut" | An island is nearer the pocket wall or another island than the bit is wide. Use a smaller bit or move the island ([Pocket](pocket.md#warnings)). |
| `entry_fallback` | "there is no room for a `helix` entry in …, so its passes start with a zigzag ramp instead", or "… with a straight plunge at the plunge rate instead; set the operation's entry to `plunge` to stop this warning" | The Pocket's helix or ramp does not fit that shape (a narrow slot, a hole barely wider than the bit), or at its angle would take too many moves to reach depth ("… would take too many moves …"). The next entry that fits is used: helix, then ramp, then plunge. Use a smaller bit, a steeper angle, or set `entry` to the one that is used ([Pocket: entry](pocket.md#entry)). |
| `helix_diameter_reduced` | "the helix entry in … is … mm across, smaller than the … mm asked for, because a larger one does not fit there" | The helix is cut at the size that fits. Nothing to do, or lower `diameter_fraction` to that size to stop the warning ([Pocket: entry](pocket.md#entry)). |
| `relief_skipped` | "corner relief `…` is not supported by this build yet, so no corners were relieved" | The Pocket asks for dogbone or T-bone corners, which a later release adds. The corners keep the bit's radius ([Pocket](pocket.md#not-yet-in-this-build)). |
| `tool_too_large` | "tool … does not fit …, so that shape is not cut" | The bit does not fit inside the shape (an Inside cut, or a hole of an Outside cut). Use a smaller bit or enlarge the shape. |
| `duplicate_shape_dropped` | "… is the same outline in the same place as …, so it is cut only once" | Two targets are the same outline in the same place. Remove one. |
| `tabs_overlap` | "tabs on … touch or overlap on its … mm tool path, so they were joined into longer tabs" | Too many or too wide tabs for the outline. Use fewer or narrower tabs ([Tabs](tabs.md#warnings)). |
| `tabs_outside_cut_range` | "the tab top is … mm above the stock bottom, but the deepest pass stops at … mm" or "… higher than the … mm stock top" | The tabs are above every pass (they change nothing) or above the stock top (every pass lifts over them). Lower `height_mm` ([Tabs](tabs.md#warnings)). |
| `tabs_cover_most_of_contour` | "tabs on … cover more than half of its tool path …, so the part or hole may not cut free" | The tabs leave less than half the outline cut. The message suggests a count or width; or remove tabs from a small hole ([Tabs](tabs.md#warnings)). |
| `engrave_path_degenerate` | "… has fewer than two distinct points, so there is nothing to engrave; it is skipped" | An Engrave target is a single point, or all its points are in one place. Delete it from the drawing ([Engrave](engrave.md#warnings)). |
| `drill_target_not_circle` | "… is not a circle; Drill plunges only at the centre of circles, so it is not drilled" | A Drill target is not a closed circle. Draw a circle, or remove it from the targets ([Drill](drill.md#which-outlines-are-holes)). |
| `drill_tool_wider_than_hole` | "the … mm tool is wider than the … mm hole …, so the hole comes out the tool's size" | Use a smaller bit, or draw the circle the bit's size ([Drill](drill.md#which-outlines-are-holes)). |
| `drill_peck_deeper_than_hole` | "the … mm peck is at least the … mm depth of the holes, so each is drilled in one plunge" | Use a smaller `peck_depth_mm`, or the `"simple"` cycle ([Drill: peck](drill.md#peck)). |
| `drill_retract_below_stock_top` | "retract_mm is … mm, below the stock top; the tool retracts to the stock top instead" | Set `retract_mm` to 0 or more ([Drill: retract height](drill.md#retract-height)). |
| `toolpaths_overlap` | "the tool paths of … and … cut into each other: the shapes are … mm apart" | Two shapes in one operation are closer than the bit needs between them. Move them apart or use a smaller bit ([Profile: shapes too close together](profile.md#shapes-too-close-together)). |

## Export

| Code | Message says | What to do |
| --- | --- | --- |
| `arcs_flattened` | "… arc(s) written as short straight moves because the controller would reject them once rounded" | Tiny arcs whose rounded centre no longer fits were written as lines. The motion is the same; nothing to do. |
| `arc_fit_fallback` | "… run(s) of straight moves kept as written because the controller would reject the arc fitted to them once rounded" | Arc fitting left some runs as the original straight moves. The motion is the same, the program a little longer; nothing to do (see [The project file](project-file.md), `arc_fit_tolerance_mm`). |
| `disabled_operation_skipped` | "operation … is disabled and was left out" | As intended; set `enabled` to `true` to cut it. |

## Project values

These come from checking the project file and start with the value's
position. They are listed with their meaning under [Profile: values that
look wrong](profile.md#values-that-look-wrong) and in [The project
file](project-file.md).

| Code | Message says | What to do |
| --- | --- | --- |
| `operation_disabled` | "operation … is disabled and will not be exported" | As intended, or set `enabled` to `true`. |
| `unused_tool` | "tool … is defined but not used by any enabled operation" | Remove the tool, or target it from an operation. Its unset rates are not reported. |
| `open_profile` | "… is an open path; Profile cuts only closed shapes, so it is not cut" | A drawing has an open path. Close it, ignore the warning if it is a guide line, or cut it with an [Engrave](engrave.md). Not reported for a drawing an enabled Engrave targets. |
| `step_down_above_diameter` | "… cuts … mm per pass, deeper than the … mm diameter of tool …" | Passes deeper than the bit is wide can break it. Lower `step_down_mm` on the operation or tool. |
| `plunge_above_feed` | "… plunges at … mm/min, faster than it cuts" | `plunge_mm_min` is above `feed_mm_min`; they may be swapped. |
| `spindle_speed_high` | "… is set to … RPM, faster than the 60000 RPM …" | Check `rpm` for a typo. |
| `large_diameter` | "… is … mm in diameter, larger than the 50 mm …" | Check `diameter_mm` for a typo. |
| `drill_cutter_unusual` | "… drills with tool …, which makes a cone rather than a flat-bottomed hole" | Drill with a drill or an end mill, or accept the cone. |
| `relief_needs_step_down` | "… asks for corner relief, but tool … has no step_down_mm" | Set the tool's `step_down_mm`; until then the relief is skipped. |
| `engrave_deeper_than_cone` | "… engraves … mm deep with tool …, whose cone is only … mm tall" | Use a shallower depth or a wider V-bit; below the cone the shank cuts ([Engrave: V-bits](engrave.md#v-bits)). |

## Import

`init` prints what it could not import before writing the project. DXF
warnings carry a code (see [Importing DXF](importing-dxf.md) for
`dxf_entities_skipped` and the others); SVG warnings are plain sentences
naming the element skipped.
