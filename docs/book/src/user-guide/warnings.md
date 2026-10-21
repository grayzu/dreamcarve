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

## Operation order

Operations always run in the order the project lists them, never
regrouped by tool or reordered ([Machining order and operation
order](machining-order.md#operations-run-in-list-order)). These warnings
say when that order is likely a mistake.

| Code | Message says | What to do |
| --- | --- | --- |
| `cut_inside_freed_part` | "`<shape>` lies inside `<outline>`, which operation `<id>` cuts free of the stock first, so this cut would run in a loose part" | An earlier Profile cuts through the stock around (or inside) an outline, with no tabs, and this operation then cuts a shape inside it: the cut runs in a part or slug that is no longer held. An open path an Engrave or an `on_line` Profile cuts counts when all of it lies inside the outline. Move this operation before the one named, or give that Profile tabs. |
| `tool_change_avoidable` | "tool `<id>` (`<name>`) is used by operations `<a>`, `<b>`, which do not run one after another, so it is fitted more than once" | Operations using one tool are separated by an operation using another, so the tool is fitted twice. Put them next to each other if the order allows; DreamCarve never moves them for you. |

## Toolpath generation

Printed after the operation's id: `warning: <code>: operation `op1`: …`.

| Code | Message says | What to do |
| --- | --- | --- |
| `open_path_skipped` | "… is an open path, which has no inside or outside, so it is not cut; an On line Profile or an Engrave cuts along it", or "… is an open path with no length, so there is nothing to cut; it is skipped" | A Pocket's or an Inside or Outside Profile's target is not a closed outline. Close it in the drawing, cut it with an `on_line` [Profile](profile.md#open-paths) or an [Engrave](engrave.md), or leave it untargeted. The second message is an On line Profile's open path whose points are all in one place: delete it from the drawing. |
| `pocket_no_room` | "tool … is wider than the pocket of … somewhere it must pass, so that shape is not cut" | The bit does not fit in the pocket at all. Use a smaller bit or widen the shape ([Pocket](pocket.md#warnings)). |
| `pocket_island_too_close` | "in the pocket of …, island profile … is … mm from …, closer than the … mm tool … is wide, so the material between them is left uncut"; with an allowance, "closer than tool … needs with a … mm allowance (… mm)" | An island is nearer the pocket wall or another island than the bit needs to pass: its diameter plus twice the allowance. Use a smaller bit or allowance, or move the island ([Pocket](pocket.md#warnings)). |
| `entry_fallback` | "there is no room for a `helix` entry in …, so its passes start with a zigzag ramp instead", or "… with a straight plunge at the plunge rate instead; set the operation's entry to `plunge` to stop this warning" | The Pocket's or Profile's helix or ramp does not fit that shape (a narrow slot, a hole barely wider than the bit, waste pinched between two parts, a tab over the start of an open path cut On line, or of a Profile outline whose every stretch between tabs is too short), or at its angle would take too many moves to reach depth ("… would take too many moves …"), or is a helix so small that its moves are shorter than the program's last decimal place ("a `helix` entry this small has moves too short for the program's decimal places in …"). The next entry that fits is used: helix, then ramp, then plunge. Use a smaller bit, a steeper angle, or set `entry` to the one that is used ([Pocket: entry](pocket.md#entry), [Profile: entry](profile.md#entry)). |
| `helix_diameter_reduced` | "the helix entry in … is … mm across, smaller than the … mm asked for, because a larger one does not fit there" | The helix is cut at the size that fits. Nothing to do, or lower `diameter_fraction` to that size to stop the warning ([Pocket: entry](pocket.md#entry), [Profile: entry](profile.md#entry)). |
| `lead_shrunk` | "the lead arc on … is … mm in radius over …°, smaller than the … mm over …° asked for, because a larger one does not fit there" | The Profile's lead fits that outline only smaller (a small hole, waste pinched between parts or by the stock edge, tabs). It is cut at the size given. Nothing to do, or set `lead` to that size to stop the warning ([Profile: leads](profile.md#leads)). |
| `lead_dropped` | "there is no room for a lead-in arc on …, even at a quarter of the radius, so its passes start on the outline without one", "there is no room for a lead-out arc where the cut on … ends, so it stops on the outline", or "operation … cuts on the line, which has no waste side for a lead, so it cuts without one" | No lead fits that outline (a hole barely wider than the bit; when only the tabs are in the way, just the passes below the tab top go without one), no lead-out fits where its cut ends, or the Profile is On line. Use a smaller `radius_mm`, a smaller bit, or remove the lead ([Profile: leads](profile.md#leads)). |
| `relief_skipped` | "corner relief `…` is skipped because the tool has no step_down_mm, …", or "… is skipped: an On line Profile has no waste side to cut a notch into, …" | No corner of the operation is relieved. Set the tool's `step_down_mm`, which the notches step down at; on an On line Profile, cut Inside or Outside, or set `corner_relief` to `"none"` ([Pocket: corner relief](pocket.md#corner-relief), [Profile: corner relief](profile.md#corner-relief)). |
| `relief_corner_too_tight` | "the corner of … at (…, …) mm is not relieved: an edge beside it is shorter than the bit is wide, …" or "… the tool path does not turn it within reach, …" | That corner keeps the bit's radius: a notch needs a bit's diameter of wall on each side of the corner. Use a smaller bit, or ignore it if that corner need not be square ([Pocket: corner relief](pocket.md#corner-relief)). |
| `relief_corner_not_cut` | "the corner of … at (…, …) mm is not relieved because the main passes did not cut to full depth where its notch starts (a tab over it, or no wall pass), …" | A notch only plunges where the operation already cut to full depth. Move the tab off the corner, or set the Pocket's `wall_pass` to `"last"` or `"first"` ([Profile: corner relief](profile.md#corner-relief), [Pocket: corner relief](pocket.md#corner-relief)). |
| `rest_of_clears_nothing` | "operation `…`, which this one rests on, already reaches everything tool `…` can, so this operation cuts nothing" | A rest machining Pocket uses a bit no smaller than the earlier Pocket's, at no smaller an allowance, so nothing is left that it reaches. Use a smaller bit, or remove the operation ([Pocket: rest machining](pocket.md#rest-machining)). |
| `tool_too_large` | "tool … does not fit …, so that shape is not cut" | The bit does not fit inside the shape (an Inside cut, or a hole of an Outside cut). Use a smaller bit or enlarge the shape. |
| `duplicate_shape_dropped` | "… is the same outline in the same place as …, so it is cut only once" | Two targets are the same outline in the same place, or (On line Profile) the same open path, drawn either way. Remove one. |
| `tabs_overlap` | "tabs on … touch or overlap on its … mm tool path, so they were joined into longer tabs" | Too many or too wide tabs for the outline. Use fewer or narrower tabs ([Tabs](tabs.md#warnings)). |
| `tabs_outside_cut_range` | "the tab top is … mm above the stock bottom, but the deepest pass stops at … mm" or "… higher than the … mm stock top" | The tabs are above every pass (they change nothing) or above the stock top (every pass lifts over them). Lower `height_mm` ([Tabs](tabs.md#warnings)). |
| `tabs_cover_most_of_contour` | "tabs on … would cover more than half of its tool path …, so the part or hole would not cut free. Only … of them are cut there" | The outline is too short for the tabs: it gets the most that cover at most half of it, possibly none. Use narrower tabs (the message gives the width) to keep them all ([Tabs](tabs.md#outlines-too-short-for-the-tabs)). |
| `engrave_path_degenerate` | "… has fewer than two distinct points, so there is nothing to engrave; it is skipped" | An Engrave target is a single point, or all its points are in one place. Delete it from the drawing ([Engrave](engrave.md#warnings)). |
| `drill_target_not_circle` | "… is not a circle; Drill plunges only at the centre of circles, so it is not drilled" | A Drill target is not a closed circle. Draw a circle, or remove it from the targets ([Drill](drill.md#which-outlines-are-holes)). |
| `drill_tool_wider_than_hole` | "the … mm tool is wider than the … mm hole …, so the hole comes out the tool's size" | Use a smaller bit, or draw the circle the bit's size ([Drill](drill.md#which-outlines-are-holes)). |
| `drill_peck_deeper_than_hole` | "the … mm peck is at least the … mm depth of the holes, so each is drilled in one plunge" | Use a smaller `peck_depth_mm`, or the `"simple"` cycle ([Drill: peck](drill.md#peck)). |
| `drill_retract_below_stock_top` | "retract_mm is … mm, below the stock top; the tool retracts to the stock top instead" | Set `retract_mm` to 0 or more ([Drill: retract height](drill.md#retract-height)). |
| `toolpaths_overlap` | "the tool paths of … and … cut into each other: the shapes are … mm apart" | Two shapes in one Profile are closer than the bit needs between them, or an open path cut `on_line` comes within half a bit of another shape's line or crosses it, or an open path cut `on_line` outside any part comes within half a bit of itself far along it (a tight spiral or a hairpin; the warning names it twice). Move them apart or use a smaller bit ([Profile: shapes too close together](profile.md#shapes-too-close-together)). An Engrave never gives it. |

## Export

| Code | Message says | What to do |
| --- | --- | --- |
| `arcs_flattened` | "… arc(s) written as short straight moves because the controller would reject them once rounded" | Tiny arcs whose rounded centre no longer fits were written as lines. The motion is the same; nothing to do. |
| `arc_fit_fallback` | "… run(s) of straight moves kept as written because the controller would reject the arc fitted to them once rounded" | Arc fitting left some runs as the original straight moves. The motion is the same, the program a little longer; nothing to do (see [The project file](project-file.md), `arc_fit_tolerance_mm`). |
| `disabled_operation_skipped` | "operation … is disabled and was left out" | As intended; set `enabled` to `true` to cut it. |
| `canned_cycle_expanded` | "operation …: written as plain moves, not canned cycles, because …" | `use_canned_cycles` is on, but this Drill was written as `G0`/`G1` moves, all of it. The reason says why: a peck with a dwell, or a cycle the post-processor lacks. The holes are the same either way ([Drill: canned cycles](drill.md#canned-cycles)). |
| `units_set_by_controller` | "post-processor `…` has no units words (G20/G21), so the program has none; it is in …, so run it only on a machine set up in …" | UCCNC reads every number in the units of its profile. Run the program with a profile in the units the message names, or change the project's `display_units` ([Post-processors: controllers compared](post-processors.md#controllers-compared)). |

## Project values

These come from checking the project file and start with the value's
position. They are listed with their meaning under [Profile: values that
look wrong](profile.md#values-that-look-wrong) and in [The project
file](project-file.md).

| Code | Message says | What to do |
| --- | --- | --- |
| `operation_disabled` | "operation … is disabled and will not be exported" | As intended, or set `enabled` to `true`. |
| `unused_tool` | "tool … is defined but not used by any enabled operation" | Remove the tool, or target it from an operation. Its unset rates are not reported. |
| `open_profile` | "… is an open path and no operation cuts it; only an On line Profile or an Engrave cuts an open path" | A drawing has an open path. Close it, ignore the warning if it is a guide line, or cut it with an `on_line` [Profile](profile.md#open-paths) or an [Engrave](engrave.md). Not reported for a drawing an enabled Engrave or On line Profile targets. |
| `step_down_above_diameter` | "… cuts … mm per pass, deeper than the … mm diameter of tool …" | Passes deeper than the bit is wide can break it. Lower `step_down_mm` on the operation or tool. |
| `plunge_above_feed` | "… plunges at … mm/min, faster than it cuts" | `plunge_mm_min` is above `feed_mm_min`; they may be swapped. |
| `spindle_speed_high` | "… is set to … RPM, faster than the 60000 RPM …" | Check `rpm` for a typo. |
| `large_diameter` | "… is … mm in diameter, larger than the 50 mm …" | Check `diameter_mm` for a typo. |
| `drill_cutter_unusual` | "… drills with tool …, which makes a cone rather than a flat-bottomed hole" | Drill with a drill or an end mill, or accept the cone. |
| `relief_needs_step_down` | "… asks for corner relief, but tool … has no step_down_mm" | Set the tool's `step_down_mm`; until then the relief is skipped. |
| `engrave_deeper_than_cone` | "… engraves … mm deep with tool …, whose cone is only … mm tall" | Use a shallower depth or a wider V-bit; below the cone the shank cuts ([Engrave: V-bits](engrave.md#v-bits)). |

## Import

`init` prints what it could not import before writing the project, as
`warning: <message>`. One carries a code; the others are plain sentences
naming what was skipped.

| Message says | What to do |
| --- | --- |
| "`dxf_entities_skipped`: … entity(ies) DreamCarve does not read were skipped: TEXT: 1, …" | A DXF holds text, dimensions, hatches, meshes or other entities DreamCarve does not read. Explode text to outlines in the CAD program if you want to cut it ([Importing DXF](importing-dxf.md#what-is-read)). |
| "block `…` is inserted more than 12 levels deep; the deeper levels were skipped" | A DXF block inserts itself, or nests too deep. Fix the block in the CAD program. |
| "INSERT of undefined block `…` was skipped" | The DXF places a block it does not define. Re-export the drawing. |
| "the drawing's text is in code page `…`, which DreamCarve cannot read; layer names were read as Windows-1252 and may show wrong characters" | A DXF from before AutoCAD 2007 is saved in a code page DreamCarve has no table for, such as a Japanese, Chinese or Korean one. The shapes are right; save the drawing as a 2007 or later DXF to fix the names ([Importing DXF](importing-dxf.md#limits)). |
| "… text element(s) skipped; convert text to paths to import it" | An SVG holds text. Convert it to paths in Inkscape (Path → Object to Path). |
| "… image(s) skipped" | An SVG holds pictures, which have no outlines to cut. |
| "path `…` uses a gradient or pattern; its outline was imported" | Nothing to do: paint never changes the shape. |
| "clip path on group `…` ignored …", "mask on group `…` ignored …" | The shapes were imported whole, unclipped. Apply the clip in the drawing program if it should cut the shapes. |
| "closed subpath … of path `…` encloses no area and was dropped", "path `…` has no usable geometry and was skipped" | A path is a point or a line folded back on itself. Delete it from the drawing. |
| "page size is in … but there is no viewBox, so coordinates are read as CSS pixels (96 per inch)" | Check the size `init` reports; add a `viewBox` to the SVG if it is wrong. |
