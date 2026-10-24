# Phase 2: Full 2.5D from the command line — Functional Spec

**Status:** draft

## Goal

A hobbyist can take a drawing from Inkscape (SVG) or a CAD program (DXF) and
produce every 2.5D cut a VCarve user expects except V-carving, from the
command line, for any of six controllers: Pocket (Offset or Raster
clearance), Drill (simple, peck, dwell), Engrave (open and closed paths at a
fixed depth), and Profile with ramp or helix entry, arc lead-in and lead-out,
and dogbone or T-bone corner relief. Projects hold several operations in the
order they cut. Tools come from a library in the user's config directory,
filled by hand or by importing a Fusion tool library. The program they run
is one file per tool, or one file with tool changes, as they prefer.

Where VCarve Pro and PureCutCNC differ in how a feature is presented, this
spec follows VCarve's vocabulary and option set and PureCutCNC's algorithm.
Sources are cited per capability so the choice can be checked.

## Users and scenarios

- **Sam** draws a plaque in Inkscape: an outline, a recessed panel with a
raised circle in it, four screw holes and a decorative line. They run
`dreamcarve init plaque.svg`, which writes a `.dcv` with one Profile
operation as in phase 1. They add a Pocket, a Drill and an Engrave
operation to the file by hand, following the chapter for each, pick a
6.35 mm end mill and a 3.175 mm end mill from their library with
`dreamcarve tools add`, and run `dreamcarve export`. Two programs come
out, one per tool, in the order to run them. Sam cuts the plaque.
- **Sam, later,** finds the raster scallops on the pocket floor visible in
soft pine and switches the Pocket's clearance strategy from `raster` to
`offset`. Only that operation's moves change.
- **Alex** has a LinuxCNC router and a Fusion tool library their club
maintains. They run `dreamcarve tools import club.tools`, see the 23 tools
that imported and the 2 that did not (and why), set `--post linuxcnc`,
and export a `.ngc` whose header, tool change and footer match LinuxCNC's
documentation.
- **Alex** drills 20 holes with a simple cycle. On LinuxCNC they opt in
to canned cycles and get 20 `G81` lines; on grblHAL they get the same
holes as plain moves. The two programs drill identical holes. (changed
in slice 17, #172: LinuxCNC's `G83` back-off differs between versions,
so a peck cycle is plain moves there.)
- **Jo** imports a DXF from LibreCAD with blocks and arcs. The arcs stay
arcs (no facets), the blocks are expanded, the layer names become shape
names, and the inch drawing lands at the right size because the file says
it is in inches.
- **Jo** cuts six small parts from one sheet. Each part is cut to full depth
before the next (the default), or, for a vacuum table, every part gets
pass one before any gets pass two (`pass_first`). The parts are visited
nearest-first, and a part inside a hole is always cut before the hole.

## Capabilities

Numbering continues from phase 1's nine capabilities, which still hold.
Vocabulary is `CONTEXT.md`'s. "The user can" means by editing the `.dcv`
or running the CLI; there is no GUI in this phase.

### Operations

10. **Several operations.** A project holds any number of Profile, Pocket,
Drill and Engrave operations. Export cuts the enabled ones in list
order, never reordered or regrouped by tool (research decision D7).
`check` warns `cut_inside_freed_part` when an operation cuts inside a
shape an earlier operation has already cut free,
`cut_inside_tab_held_part` when it cuts deeper than half the stock
inside a part an earlier Profile through the stock holds only by its tabs
(changed in #269), and hints
`tool_change_avoidable` when the same tool is used in two operations
that are not adjacent.
11. **Machining order** (`machining_order`) on Profile, Pocket and Engrave:
`full_depth_first` (default; each shape is finished before the next) or
`pass_first` (each pass across every shape before the next, deeper
pass). It changes the order of cuts, never which cuts are made (D3, D4).
Within an operation the shapes are visited nearest-neighbour from the
first shape in drawing order, with the fixed rule that a shape is cut
before any shape that encloses it (D1, D5). Drill ignores the setting:
every hole is one plunge sequence, visited nearest-first.
12. **Pocket** clears all material inside a closed shape to `depth_mm`, in
passes of `step_down_mm`. Shapes nested inside a pocket are islands;
shapes inside an island are pockets again. Two **clearance
strategies**, named as VCarve names them (VCarve Pro Reference Manual,
"Pocketing Toolpath"):
    - **`offset`**: concentric passes stepping inward from the wall by the
    stepover; islands are handled as holes.
    - **`raster`**: parallel straight passes at `raster_angle_deg` from the
     X axis (0 to 180 exclusive; VCarve offers 0 to 90), spaced by the
     stepover, clipped to the region, linked in a zigzag where the link
     stays inside the region and by a retract otherwise.
      Both finish with a **wall pass** along the wall and each island
      (`wall_pass`: `last` default, `first`, or `none`, VCarve's "Profile
      Pass"), so raster scallops and offset cusps at the wall are cleaned.
      The **stepover** is a percentage of the tool diameter
      (`step_over_fraction`, from the tool by default, overridable on the
      operation like `step_down_mm`; `init` and the library default to 0.4).
      `direction` (climb or conventional) applies to every ring and to the
      wall pass; raster lines alternate. **Pocket allowance** (`allowance_mm`,
      VCarve's "Pocket Allowance"): a positive value leaves that much
      material on the walls, so a roughing pocket with a large tool can be
      followed by a finishing pocket with a small one.
13. **Rest machining.** A Pocket may name an earlier Pocket in `rest_of`.
It then clears only the material that pocket's tool could not reach
(corners narrower than that tool, plus the allowance it left), as
Fusion's "Rest Machining" and VCarve's "Use Larger Area Clearance
Tool" do. Rough plus rest leaves no material the smaller tool can
reach, which is what the coverage test checks.
14. **Drill** plunges a tool at the centre of each circular target to
`depth_mm`. A target is circular when its outline lies on one circle
within the project's flatten tolerance; other targets are skipped with
a warning. Cycles (VCarve "Drilling Toolpath"; Fusion "Drill",
cycle types):
    - **simple**: one plunge at the plunge rate, then a retract;
    - **peck**: plunges of `peck_depth_mm` (the tool's step-down by
     default, as VCarve uses the tool's pass depth, counted from the
     retract height as a controller's `G83`/`G73` counts them from `R`),
     retracting between
     pecks either fully to the retract height above the hole top
     (`retract: full`, VCarve "Retract above cutting start", Fusion
     "Deep drilling") or by a small gap (`retract: chip_break` with
     `gap_mm`, VCarve "Retract above the height of the previous pass",
     Fusion "Chip breaking");
    - **dwell**: `dwell_s` seconds at the bottom of every plunge before
     retracting (both products use seconds), on its own or with pecking.
      `retract_mm` is the height above the hole top the tool rapids down
      to before each hole and returns to between pecks, when that is below
      safe Z; each hole ends with a retract to safe Z, as a canned cycle's
      `G98` does (changed in slice 7, #145). Where the
      post-processor supports canned cycles and the user has turned them on,
      a simple cycle writes `G81`, dwell alone `G82`, peck with full retract
      `G83`, and chip-break `G73`; every other combination, and every
      controller without them, writes the same motion as plain moves. The
      holes are drilled in the same order and to the same depth either way.
      `G83` and `G73` need a controller that documents a fixed re-entry
      distance (changed in #153: they no longer need a retract height of
      0); no bundled definition has them,
      so peck and chip-break Drills are plain moves on all six (changed in
      slice 17, #172).
15. **Engrave** follows each target path, open or closed, with the tool
centre on the line, to `depth_mm` in passes of `step_down_mm`
(VCarve "Quick Engraving", Outline strategy; Fusion "2D Trace"). Any
cutter is allowed, as with Profile's On line. `alternate_direction`
(default off) cuts successive passes of an open path in opposite
directions, saving a retract per pass (D9).
16. **Entry** (`entry`) on Profile and Pocket (VCarve "Ramps", Zig Zag;
    Fusion "Ramp Type"): `plunge`, `ramp` (default; `init` writes it) at
    `angle_deg` (default 15°), or `helix` of `diameter_fraction` of the
    tool diameter (default 0.8) at `angle_deg`. A ramp zigzags back and
    forth in material the operation removes: inside the cleared region
    for a Pocket, in the hole for an inside Profile, in the open stock
    around the part for an outside Profile, and along the cut path itself
    for an On line Profile (its kerf is the only material it removes). A
    helix sits in the same material, never in the part. When a helix does
    not fit it falls back to a ramp, and a ramp that does not fit to a
    plunge, each with a warning naming the operation and shape. The
    descent never exceeds the plunge rate in Z. It starts 1 mm above the
    stock top, or on a Pocket's later pass 1 mm above the floor the pass
    before cut, and the bit rapids straight down from safe Z to there
    (S2; changed in #214: it started at safe Z and ramped or spiralled
    through the air). Projects from phase 1 keep `plunge` when migrated,
    so they cut the same.
17. **Lead-in and lead-out** (`lead`) on Profile (VCarve "Leads", circular;
Fusion "Lead-In (Entry)"): an arc of `radius_mm` tangent to the path,
so the tool enters and leaves the wall without a dwell mark. `lead_out`
(default on) adds the matching exit arc; `overcut_mm` (default 0)
continues past the start point before leading out, as VCarve's
"Overcut Distance". The start point is chosen away from corners and
near the previous cut. A lead that would cut into the part, another
shape, or off the stock is shrunk and, if it still does not fit,
dropped with a warning.
18. **Corner relief** (`corner_relief`) on Pocket and Profile: `none`
(default), `dogbone` or `t_bone`. Each internal corner that the round
tool cannot reach gets a short out-and-back notch so a square part
fits the recess (VCarve "Create Fillets": Dog-Bone, T-Bone; T-bone on
the longer edge, as VCarve places it). On a Pocket the notches are in
the pocket's corners and the concave corners of islands; on an inside
Profile in the hole's corners; on an outside Profile in the part's
concave corners. Notches are cut inline, on the pass that cuts the
final wall: the Pocket's wall and island rings, or the Profile's lap
at final size. At every level that path reaches a corner, it goes out
to the corner and back and carries on, with no lift. The notch still
steps down at the tool's step-down: a wall level deeper than one tool
step below the last notch level cuts the levels between in the same
column, stepping down at the corner. Where there is no final-wall path
to cut from, the notches fall back to the separate notch pass: on a
Pocket with an allowance (its rings are not the final wall), and for
any corner whose final-wall path does not reach it at depth. Allowance
paths (a Pocket's allowance, a Profile's passes before its last pass)
never get notches. Corners under tabs stay skipped, and S9's notch rule
is unchanged. (changed: maintainer decision 2026-10-24. The notch pass
ran after the main passes, or among a Profile's passes, lifting and
plunging at every corner, as PureCutCNC cuts it.)
19. **Multi-tool export.** A project whose enabled operations use more
than one tool exports either one program per tool run (phase 1,
default) or one program with tool changes (`tool_change.mode: "m6"`,
on a post-processor whose tool change has `M6`; of the six, grblHAL).
The user sets a
default once in the user config file, which `init` and `--post` write
into new projects' post-processor settings, and overrides it per
export with `--programs-per-tool` or `--one-program` without changing
the file. `--one-program` on a post-processor without `M6` is an error.

### Input

20. **DXF import.** `dreamcarve init part.dxf` reads LINE, ARC, CIRCLE,
LWPOLYLINE and POLYLINE (with bulges), SPLINE and ELLIPSE (flattened to
the project tolerance), and INSERT (blocks, expanded with their
transform). Arcs stay arcs. Units come from the file's `$INSUNITS`;
when the file has none, `--units` is required and `init` says so.
Layer names become shape names; `--layer` restricts the import to
named layers. Open segments whose ends meet within `--join-tolerance`
(default 0.01 mm) are joined, and a chain that closes becomes a closed
shape. Entities DreamCarve does not read (TEXT, MTEXT, DIMENSION,
HATCH, 3D meshes) are listed in a warning with their counts, never
dropped silently.

### Tools

21. **Tool library.** `dreamcarve tools` manages a library in the user's
config directory (`dreamcarve config path` prints where). `tools  list` shows every tool; `tools add <project.dcv> <tool>` copies a
library tool into a project (by id or unique name); `tools export  <file>` writes the library as DreamCarve JSON; `tools import <file>`
reads a DreamCarve JSON or a Fusion library and reports, per tool,
imported, skipped (with the reason) or already present. Projects keep
embedding copies of their tools (ADR-0002): a library edit never
changes an existing project.
21a. **Tool numbers.** A tool may carry a `number` (`null` by default).
    Export writes `T<number>` when it is set and the tool's position in
    the project list otherwise; two enabled operations' tools with the
    same number are an error. Fusion import copies the file's tool
    number when it is non-zero. VCarve's Tool Database and Fusion's tool
    editor both carry one.
22. **Fusion import** reads `.tools` (a zip holding `tools.json`) and
`.json` libraries as Fusion exports them. Flat end mills, ball end
mills, chamfer mills (as V-bits, included angle twice the file's taper
angle, as Fusion's own 90° chamfer mills carry 45°), drills, spot
drills, countersinks, counterbores and reamers import with their
diameter, flutes, angle, units and tool number; each cutting preset
becomes one library tool carrying the preset's spindle speed, feed,
plunge rate, step-down and stepover, named `<tool> (<preset>)` when a
tool has more than one. Tool types DreamCarve has no cutter for
(thread mills, face mills, taps, turning tools) are skipped by name.
A preset without a feed or plunge rate imports with that value unset,
so export still refuses it until the user supplies one.
22a. **Vendor and product id.** Each imported tool keeps the file's
    vendor and product id (`vendor`, `product_id`; `null` when the file
    has none, and in every older project and library file), and its name
    starts with the vendor: `IDC Woodcraft 1/4" Downcut`, never `IDC
    Woodcraft IDC Woodcraft …` when the description already starts with
    it. When two tools would take one name, the second gets its product
    id, `IDC Woodcraft Widget Works Diamond Engraver (ED-14)`, and ` (2)`
    only when it has none or that name is taken too. `tools list` shows
    VENDOR and PRODUCT columns; `tools add` and `tools remove` find tools
    by the new names. Re-importing the file still recognises each tool.
    (changed: maintainer decision 2026-10-24. Tools were named by
    description alone, so IDC Woodcraft's ED-12 and ED-14 imported as
    `Widget Works Diamond Engraver` and `… (2)`.)
22b. **Exact inch values.** Values converted from inches are rounded,
    lengths to 6 decimal places of a millimetre and feeds to 3 of mm/min,
    so 3/16" is 4.7625 mm and 60 in/min is 1524 mm/min, not
    `4.762499999999999` and `1524.0000000000005`. This covers Fusion
    import and inch strings in project and library files (capability
    32); values in millimetres are kept as written. Tools already in the
    library are not rounded when it loads: `tools import --replace
    <file>` updates them. (changed: maintainer decision 2026-10-24.
    Converted values carried floating-point noise.)

### Output

23. **Six post-processors**: grblHAL, GRBL, LinuxCNC (`.ngc`), Mach3
(`.tap`), UCCNC (`.txt`) and generic (`.nc`). Each new definition is
verified line by line against the controller's own documentation and
the phase 1 safety rules, with the sources cited in the definitions
README. The same `.dcv` exported for any two of them produces identical
XYZ motion and differs only in dialect lines (acceptance 13 holds
across all six).
24. **Arc fitting at export.** Runs of straight moves (with any corner
arcs shallower than the tolerance between them) that lie on one circle
within `arc_fit_tolerance_mm` (project setting, default 0.01;
`null` turns fitting off) are written as `G2`/`G3` where the
post-processor supports arcs, so an imported circle exports as a few
arcs instead of hundreds of lines. Arcs the controller would reject
after number rounding are written as the original lines. Fitting never
moves the tool more than the tolerance from the original path.
25. **Structured warnings.** Every warning from `check` and `export` has a
stable snake_case code and named parameters, printed as
`code: message` with the operation and shape named, and located at the
field when it concerns one value. The codes are listed in the
`Warnings` chapter.

### Tabs

26. **Tabs per outline** (#109). A tab request (a Profile's `tabs`, and
`dreamcarve tabs place --count`) lays its tabs on each outline the
Profile cuts on its own, as VCarve applies tabs per vector. Holes
(outlines nested as holes among the operation's targets) get no tabs
unless `tabs.on_holes` is `true` (`--on-holes`): a hole's slug is waste.
`init` writes `false`; a project from format version 8 or earlier
upgrades to `true`, so it cuts as before. An outline whose tabs would
cover more than half of its tool path gets the most tabs that cover at
most half, and `tabs_cover_most_of_contour` names it with the tabs asked
and cut. A part's outline keeps at least one tab, even one that covers
more than half (the warning says the part may need cutting free by
hand); a hole with `on_holes` may get none. So one operation cuts a part and its holes, holes first:
a star with 4 tabs and its 7 mm hole, with a 6.35 mm bit, no longer
needs a second operation. (changed in #109: one setting tabbed every
outline, and too-short outlines kept tabs covering most of them.)
(changed in #105: these are now the rules for placing tabs from a
request, capability 26a; the tabs placed are cut as placed tabs.)
26a. **Placed tabs** (#105, ADR-0005). A tab is a project object in
`Project::tabs`: an instance, one of its definition's profiles, a point
in definition space (`x_mm`, `y_mm`), and its own `width_mm`,
`height_mm` (tab top above the stock bottom) and `shape` (`rect` or
`smooth`). It follows its instance when the instance moves, rotates,
mirrors or scales. Every enabled Profile that cuts that outline rises
over the tab, centred at its tool path's point nearest the tab, with the
phase 1 span (the tab width plus the tool diameter) and shapes. A
Profile crosses a tab when its path passes within the tool radius plus
half the tab width of where the tab meets the outline and its cut goes
below the tab's top, so a shallow score pass of a tabbed outline crosses
none. A pass rises over
a tab only below that tab's top; overlapping spans merge, taking the
highest top, smooth only if every tab in them is, and warn
`tabs_overlap`; one tab alone whose span covers its whole path overlaps
nothing and warns only its coverage. Placed tabs are cut as placed: export never drops one,
and a layout that covers more than half its path warns
`tabs_cover_most_of_contour` ("All of them are cut, as placed"). Two new
warnings: `tab_not_crossed`, project-level, for a tab no enabled Profile
crosses (its outline is not cut by an enabled Profile, or only by a
Pocket or a disabled Profile, or every path runs too far from it or stops
above its top), and
`tab_on_hole` for a tab on an outline a Profile cuts as a hole: the tab
is cut and holds the slug. `tabs place` warns `tab_on_hole` once for each
tab it places on a hole; `check` and `export` do not warn again for a
tab placed there with `--on-holes` (`--count` or `--at`) or from a
request with `on_holes`, such as a version 8 file's migrated one, which
is saved with `holds_slug: true`, only for others (changed in #300:
every hole tab warned on every export; changed in #302: a tab placed
with `--at --on-holes`, or whose Profile changed its bit or side, warned
on every export). Format version 15 adds `holds_slug` to each placed tab
(`true`, `false`, or `null` from the migration, which warns as format 14
did), and `tabs list` shows it as `HOLDS_SLUG`. A crossed tab
whose point is farther from its outline than its width plus the widest
bit that cuts round it warns `tab_off_outline`, project-level, with the
distance; it is cut at the outline's nearest point (#300). The CLI gains `dreamcarve tabs place <dcv>
<op> [--count N | --at d1,d2,…] [--width] [--height] [--shape]
[--on-holes]` (with neither `--count` nor `--at`, the operation's own
request), `tabs list <dcv>` (each tab, its distance along the outline,
how far its point is off the outline (`OFF_MM`, #300), and the enabled
Profiles that cross it) and `tabs clear <dcv> [<op> |
--tab <id>…]`, where `<op>` must be a Profile and only the tabs it
crosses are removed, with a note naming any part a through cut then
frees with no tab (changed in #105 review: `<op>` removed every tab on
its targets, whatever the operation). `--count` is at least 1, and
`--width`, `--height`, `--shape` and `--on-holes` need `--count` or
`--at`. `tabs place` adds to the tabs already there, and notes each
outline that had some; `--replace` (which needs `--count` or `--at`)
first removes the tabs the Profile crosses, the set `tabs clear <dcv>
<op>` removes, prints each tab removed and placed, and saves once, with
`tabs clear`'s note for a part it leaves with no tab (changed in #293:
changing the count took `tabs clear` first). `--count` follows capability 26; `--at` puts a tab at
each distance along the outline from its first vertex as drawn. Format
version 11 adds `"tabs": []`; a Profile's `tabs` stays as a tab
request, which generation places before it cuts, so older files cut
byte-identically. Each `tabs` command that saves the project places the
requests of every enabled Profile first and clears them; a disabled
Profile keeps its request. `init` writes a request, as before, and an
empty list. Once placed, two Profiles that cut one outline both rise
over its tabs. (changed in #105: tabs were an operation setting, so a
change of tool or side moved every tab and two Profiles of one outline
each rose over their own tabs only; tabs as placed objects were out of
scope.)

### Open paths

37. **Left and Right of an open path** (#207; numbered after the #73
addendum's 27–36). A Profile's `side` may be
`left` or `right` (format version 10), as VCarve's Profile cuts "to the
Left, to the Right or On the selected vectors". The tool centre runs one
radius to that side of each open target, looking along it from its first
vertex to its last as drawn; the cut ends square on the normals through
the path's ends, so the bit's edge reaches the end vertex and goes no
further on the part side. An end segment too short for the bit on the
inside of the turn before it is an error naming its end vertex (changed
in #342: the cut beside the segment before ran past the end normal into
the part). The side the bit runs on is waste and the
other side is the part: a cut that would cross another of the
operation's lines, or reach the strip one radius wide that a line keeps
on its part side, is an error. Direction is the milling mode, as on a
closed shape: climb keeps the part on the right of travel, so Left climb
starts beside the first vertex and Right climb beside the last, and
conventional is the reverse (changed in #287: #207 used On line's
start-point rule, which made a Left `climb` mill conventional);
ramps run along the offset path and tabs are measured along it from the
end beside the first vertex; there is no lead, no corner relief and no
helix, as VCarve gives open profiles none. Closed targets are skipped
with `side_needs_open_path`, since a closed path's direction is not
shown in the drawing. S9 holds the cut to the offset path and clear of
every line and kept strip, the strip closed at each end by the normal
through the end vertex (#342). (added in #207: before, an open path could
be cut only On line, and cutting beside one meant redrawing it a bit
radius over. Changed in #311: a Left or Right Profile takes a last pass,
capability 34.)

## Out of scope

V-Carve and V-bit countersinking (phase 3). Helical boring of holes wider
than the tool (a Pocket on the circle does it). Trochoidal clearance,
seeded offset, engagement-based feed reduction, slot feed reduction,
corner rounding of toolpaths, wall corner clean-up (PureCutCNC features not
in VCarve's pocket form; post-1.0 or phase 8). Pocket fill patterns for
Engrave (VCarve's hatch and offset fill). Straight-line leads. Start depth
(every operation starts at the stock top). Drill tip
compensation and "drill through". Region masks and clamps as obstacles. A
GUI, simulation, text. Reading Vectric, Carveco or Carbide Create tool
databases. (changed in the #73 addendum: separate clearance, tool-change
and park heights and an SVG toolpath preview moved in scope; see "Phase 2
addendum (#73)".) (changed in #105: tabs as placed objects were out of
scope, waiting for the GUI; the maintainer asked for ready-for-agent work
to be picked up, so they are capability 26a, placed and edited from the
CLI.)

## Acceptance tests

Numbering continues from phase 1's seventeen.

18. **Four operations, two programs.** The plaque `.dcv` (see Test part)
exports two programs, `plaque-1-T1.nc` (Profile, Pocket, Drill with the
6.35 mm end mill) and `plaque-2-T2.nc` (Engrave with the 3.175 mm end
mill), and lists them in that order. Each passes S1–S8 on its own.
19. **Offset pocket.** A 100×50 mm rectangle pocketed 6 mm deep in 3 mm
passes with a 6.35 mm tool at 40 % stepover: every cutting move's tool
footprint lies inside the rectangle; the union of footprints at each
pass covers the rectangle inset by the tool radius (the reachable
area) within 0.05 mm; the last ring of each pass is the wall; rings
are concentric and spaced by 2.54 mm.
20. **Island.** The same rectangle with a 20 mm circle inside it: no
cutting move's footprint enters the circle; the wall pass runs along
the rectangle and around the circle; the circle's wall is traversed in
the opposite sense to the rectangle's for the same `direction`.
21. **Raster at 0° and 45°.** The rectangle rastered at 0°: every
clearance line is parallel to X and lines are 2.54 mm apart; at 45°
every line is at 45°. Consecutive lines alternate direction and are
joined by a cut inside the region, not a retract. The wall pass is
last by default and first with `wall_pass: first`.
22. **Rough then rest.** The rectangle with a 10 mm × 10 mm notch,
pocketed with a 12.7 mm tool at `allowance_mm` 0.5, then a rest Pocket
with a 3.175 mm tool: the rest operation cuts only the notch and the
wall band; the union of both operations' footprints covers the region
inset by 1.5875 mm (the small tool's radius) within 0.05 mm; the rest
operation's cutting length is under a quarter of a full 3.175 mm
pocket's.
23. **Drill cycles.** Four 5 mm circles, 12 mm deep: `simple` gives one
plunge per hole at the plunge rate and one retract to safe Z; `peck`
with 4 mm pecks and `retract: full` gives three plunges per hole at
`retract_mm: 0` and four at `retract_mm: 1` (pecks count from the
retract height; changed in #153), each retracting to the retract height; `chip_break` with a 0.5 mm gap retracts
0.5 mm between pecks; `dwell_s: 0.5` writes a 0.5 s `G4` (`G4 P0.500` on grblHAL) at the bottom of
each plunge. Holes are visited nearest-first from the first in drawing
order. The two 5 mm circles and a 5×8 mm slot targeted together drill
two holes and warn `drill_target_not_circle` naming the slot.
24. **Canned cycles.** The simple case on LinuxCNC with canned cycles on
writes one `G81` line per hole after `G98`, ends with `G80`, and the
program's parsed motion is identical to the plain-moves program's
motion. With canned cycles off, or on grblHAL, no cycle word (`G73`,
`G81`–`G83`, `G98`, `G99`) appears; LinuxCNC's header `G80` only
cancels, so it stays. A peck-with-dwell cycle writes plain moves even
with canned cycles on. (changed in slice 17, #172: the LinuxCNC header
has `G80`.)
25. **Engrave open and closed.** An open polyline and a closed rectangle,
1 mm deep in 0.5 mm passes with a 60° V-bit: both are traced with the
tool centre on the line (every cut vertex is on the drawing within
0.001 mm), in two passes each; with `alternate_direction` the open
path's second pass runs backwards and there is no retract between its
passes; the rectangle's passes run the same way both times.
26. **Ramp entry.** An outside Profile with `entry: ramp` at 15°: the
    first move into each pass is a ramp in the stock around the part
    whose footprint never enters the part or another shape, with a slope
    of 15° within 0.1°, at the feed rate, with its Z component never
    faster than the plunge rate; no vertical plunge occurs below the
    stock top. An On line Profile of a 100 mm line ramps back and forth
    along the line itself. An inside Profile of a 7 mm hole with a
    6.35 mm tool has no room, plunges, and warns `entry_fallback`.
    (changed in #175: the 100 mm line is an open path, which an On line
    Profile now cuts, as VCarve's Profile "On" does; Inside and Outside
    still skip open paths.) (changed in #257: a ramp turns back only when
    one run, at most 3·D long, cannot reach the pass depth, as PureCutCNC's
    `emitRamp` does; at 15° one 19.05 mm run drops 5.1 mm, more than a
    2–3 mm pass needs, so each pass is entered by one leg. A zigzag is
    the same ramp with more legs, not a different requirement.)
27. **Helix entry.** A Pocket with `entry: helix` at 80 %: the descent is
a helix of diameter 5.08 mm (for a 6.35 mm tool) whose footprint lies
inside the region, followed by one flat revolution at the pass depth;
in a 7 mm wide slot (tool-centre region 0.65 mm wide) the helix does not
fit, a ramp along the slot is used instead and `entry_fallback` is
warned naming the operation and shape; in a 7 mm hole (tool-centre
circle 0.65 mm across) neither fits and the warning says `plunge`.
(changed in slice 9, #156: 6 mm and 2 mm are narrower than the tool, so
the Pocket skipped them with `pocket_no_room`. With the ported constants
a helix fails in a slot narrower than about 1.3 tool diameters and a
ramp in a hole narrower than about 1.22; 7 mm is inside both bands.)
28. **Arc lead.** An inside Profile of a 60×40 mm rectangle with a 3 mm
lead radius: the tool reaches each pass depth at a point 3 mm or more
from the wall, arrives on the wall along an arc tangent to it, leaves
along a tangent arc after overcutting `overcut_mm`, and the start
point is more than one tool diameter from every corner. With a 30 mm
radius the lead is shrunk and `lead_shrunk` is warned; in a 7 mm hole
with a 6.35 mm tool it is dropped and `lead_dropped` is warned.
29. **Dogbone and T-bone.** A 40×30 mm pocket with `corner_relief:  dogbone`: four notches, cut from the wall ring of each pass as it
turns each corner, with no lift between them (changed: maintainer
decision 2026-10-24: they followed the main passes), along the corner
bisectors each end exactly one tool radius past the corner, so a
40×30 mm square test block fits; `t_bone` puts each notch along the
longer (40 mm) edge; an inside Profile of the same rectangle gets the
same four notches; an outside Profile of an L-shaped part gets one
notch, in its concave corner.
30. **Pass first.** Three 30 mm squares cut 6 mm deep in 2 mm passes under
`pass_first`: the Z of successive plunges is 4, 4, 4, 2, 2, 2, 0, 0, 0
(project space); under `full_depth_first` it is 4, 2, 0 three times.
The set of cutting moves at each Z is identical in both. A square
drawn inside a hole in another square is cut before the hole in both
orders, whichever is drawn first.
31. **Nearest neighbour.** Squares drawn at X = 10, 50, 30 are cut at 10,
30, 50. Two squares equidistant from the current one are visited in
drawing order. A one-shape operation's program is byte-identical to
phase 1's.
32. **Order warnings.** An outside Profile through the stock followed by a
Pocket inside that outline warns `cut_inside_freed_part` naming both
operations; the reverse order is quiet. Operations using T1, T2, T1
hint `tool_change_avoidable`.
33. **One program with tool changes.** The plaque exported with
`--one-program` on grblHAL yields one file with `T2 M6` between the
Profile and the Engrave (changed in #252: it said the Drill, but the
plaque's operations are Pocket, Pocket, Drill, Profile, Engrave, so the
only tool change follows the Profile); on GRBL the flag is an error naming the
post-processor. With `--programs-per-tool` on a project whose mode is
`"m6"`, two files are written and the project file is unchanged. A
user config setting `tool_change_mode = "m6"` makes `init` write that
mode into a new project's post-processor.
34. **DXF round trip.** `fixtures/plaque.dxf` (mm, with an ARC, a CIRCLE,
an LWPOLYLINE with bulges, a SPLINE, and a block inserted twice)
imports to the same shapes as `fixtures/plaque.svg` within 0.01 mm;
arcs import as arc segments with the same centre and radius as the
file states; the block's two instances land at their insertion
points with rotation and scale applied; names are the layer names. A
file with no `$INSUNITS` fails `init` asking for `--units`; with
`--units inch` a 1-unit line is 25.4 mm long. Four LINE entities whose
ends meet form one closed shape; three that leave a 0.5 mm gap stay
open and `check` reports them. A file with TEXT and a DIMENSION
imports its shapes and warns `dxf_entities_skipped` with
`TEXT: 1, DIMENSION: 1`.
35. **Tool library.** `tools import fixtures/fusion/sample.tools` (a
hand-written library in Autodesk's schema) reports each tool as
imported or skipped with a reason; a flat end mill imports with its
diameter, flutes, units, tool number, RPM, feed, plunge rate,
step-down and stepover from its preset; a chamfer mill with `TA` 45
imports as a 90° V-bit; a tool with two presets becomes two library
tools; a thread mill is skipped naming its type; importing the same file again reports every tool as
already present. `tools export` then `tools import` of the result
round-trips byte-identically. `tools add plaque.dcv "1/4 end mill"`
copies the tool into the project with a new project tool id; editing
the library afterwards leaves the project unchanged.
36. **Four more post-processors.** The coaster exported for LinuxCNC,
Mach3, UCCNC and generic has XYZ motion identical to its grblHAL
program; LinuxCNC ends with `M2`; Mach3 begins with an unnumbered `%`,
numbers every line after it, and has no `O0001`; UCCNC has no line numbers, `%`,
`O0001` or units word; generic has no trailing zeros. Each bundled
definition's README cites the controller document each line was checked
against. (changed in slice 17, #172: Mach3 reads an `O` word as a
subroutine label and "The O line may not have a line number" (Using
Mach3Mill 1.84, 10.5.2, 10.8.7); UCCNC's manual documents none of the
three and has no `G20`/`G21`.)
37. **Arc fitting.** The 90 mm coaster circle, imported as 160 facets,
exports as at most 8 `G2`/`G3` moves per full lap on grblHAL (a tabbed
lap is one run per segment between tabs: 4 tab tops and 5 segments give
9 runs, so at least 9 arcs; changed in slice 14, #133) and the same
number of `G1` moves as before on a post-processor without arc
support; every original vertex lies on its fitted arc within 0.01 mm;
an ellipse is not fitted; the program with `arc_fit_tolerance_mm:  null` equals the phase 1 program. The fitted program passes S6.
(changed in #70: `init` now imports an SVG circle as two exact bulge
arcs, so the 160-facet coaster is built in the test, as phase 1 imported
it, and the CLI check uses the coaster drawn as Béziers,
`tests/fixtures/coaster-bezier.svg`.)
38. **Golden parity.** For each case in `tests/golden/` listed in the
design spec, the cutting moves at each Z match PureCutCNC's exported
G-code within 0.01 mm after start-point alignment; for the two
multi-part cases the order of shapes is compared too.
38a. **Placed tabs.** A star and its 7 mm hole cut by one Outside Profile
with a 6.35 mm bit, with tabs placed only on the star by `tabs place
--at`: the hole is cut free (no pass over it rises) and only the star's
last passes rise over the tabs; the program passes S1–S9. A tab
follows its instance when the instance is moved and turned. Two
Profiles of one outline both rise over its tabs. A tab on an outline no
enabled Profile cuts warns `tab_not_crossed`. A version 9 file with
`count: 4` tabs exports byte-identical G-code as loaded and after `tabs
place` has saved its tabs as placed tabs, for each project of the
`crates/dreamcarve/tests/fixtures/tabs-migration/` corpus, the golden
cases with tabs and the coaster `init` writes. (added in #105.)

## Safety requirements

S1–S8 from phase 1 apply to every operation and every post-processor,
S2 as changed in #214 and #227: a rapid straight down from safe Z may stop over
the start of a ramp, helix or straight plunge at or above the stock top plus the 1 mm
entry clearance, or at or above that clearance over the lowest Z a cut
of the same operation has already reached through that XY. New in this
phase:

- **S9 Every operation's cuts stay in its own material.** A Pocket's
footprint stays inside its region and outside its islands; an entry
helix, ramp, lead arc or corner-relief notch stays in material the
operation removes, never in the part. Checked on the toolpath by a
footprint-containment test per operation, and by the existing overlap
check between shapes. An inside or outside Profile removes only its
trench, so its ramp or helix below the stock top also runs only on the
bit's path round its targets (widened by a last pass's allowance) or
along an earlier cut of the operation where that cut ran below the stock
top, unless the run of falling moves started at or above the stock top;
checked on the parsed program (changed in #352: a ramp below the tab top
into full-height stock passed, since it stayed in the waste; changed in
#356: an earlier cut counted along its whole length once any of it ran
below the stock top, so a later ramp past a first pass's trench passed
with arc fitting off).
- **S10 Entry descent rate.** A ramp or helix descends with a vertical
component no faster than the plunge rate: the move's feed is scaled so
that `feed × sin(slope) ≤ plunge`. Checked on the parsed program by
recomputing the slope of every descending `G1` with XY motion. Changed in
#325: nothing above the plunge rate is allowed for rounding, and the
emitter rounds the feed of a cut the toolpath holds to the plunge rate
down until its rate, from the written coordinates and feed, is at most
the plunge rate.
- **S11 Canned cycles mean what the plain moves mean.** When a program uses
`G81`/`G82`/`G83`/`G73`, the safety pass expands each cycle line into the
motion the controller performs (per that controller's documentation,
with `G98` retract) and checks S1, S2 and S6 against that expansion, so
a canned-cycle program is accepted only if its expansion is the Drill
toolpath move for move. Rapids along one vertical line in one direction
count as one move there: from a clearance height, `G98`'s return and the
rapid down to `R` pass safe Z without stopping (changed in #305). A Drill
linked at the clearance height between parts and at safe Z within one is
one `G98` series per part, each from safe Z and ended by `G80`; a series
must end with `G80` before any other motion (changed in #314: such a
Drill was written as plain moves).
- **S12 Arc fitting is a refinement, not a change.** A fitted run of arcs
replaces only straight moves, and corner arcs shallower than the
tolerance, whose vertices and shallow-arc midpoints all lie on the
circle within the tolerance, and it lies within the tolerance of every
move it replaces along the move's whole length (for a chord between
points on the circle, its sagitta); it keeps the run's start and end points
exactly, never sweeps more than 90° per arc, and is rejected (the
original moves kept) when the controller's arc-radius check would fail
after number rounding. S6 accepts, where the toolpath has those moves, a
chain of consecutive `G2`/`G3` on one circle (each piece's centre within
the tolerance plus one unit in the last place of the previous piece's),
each at most 90°, starting and ending
on toolpath vertices and covering at least three moves, that lies within
the tolerance plus one unit in the last place of every move it covers
along the move's whole length, and nothing else
(changed in slice 14, #133: a run is split at points on the circle, so
only its ends are vertices; changed in #226, #229 and #233: only the
vertices were checked, so a square's four corners, or a 40 mm wall side
after a 0.006 mm fragment, made a circle that bowed 1.45 mm and 1.3 mm
off the straight moves, which the fitter wrote and S6 accepted).
- **S13 Order invariants** O1–O10 from `docs/research/toolpath-ordering.md`
§6: containment order, permutation (the same cuts in every order),
links at safe Z unless provably inside cleared material, pass monotony,
determinism, one-shape identity, post-processor order preservation,
operation order preserved, tabs survive reordering, S1 in every order.
- **S14 Rest machining never trusts the earlier operation.** A rest
Pocket's footprint is bounded by its own region exactly as any Pocket's;
the reference operation only shrinks what it clears.
- **S15 Drill depth.** A Drill's `depth_mm` obeys S1 with its own
through-cut allowance; every peck retract is a rapid only from a Z the
tool has already cut to, so a rapid never descends into uncut material
below the retract height.

## Test part

**Plaque.** 150 × 100 mm, outside Profile with tabs, one raster pocket with
an island, one offset pocket with dogbones, four drilled holes, an engraved
border and squiggle. Drawn once as `fixtures/plaque.svg` and once as
`fixtures/plaque.dxf`; either imports to the same project.


| Item             | Value                                                                                                                |
| ---------------- | -------------------------------------------------------------------------------------------------------------------- |
| Material         | Baltic birch plywood, nominal 12 mm, measured thickness entered                                                      |
| Stock            | 170 × 120 mm, origin bottom-left, Z zero at stock top                                                                |
| T1               | 6.35 mm two-flute upcut end mill: 1500 mm/min, plunge 500, 18000 RPM, step-down 4 mm, stepover 40 %                  |
| T2               | 3.175 mm two-flute upcut end mill: 1000 mm/min, plunge 300, 18000 RPM, step-down 2 mm                                |
| op1 Pocket (T1)  | 100 × 50 mm rounded rectangle, `raster` at 0°, 4 mm deep, wall pass last, `entry: helix`, with a 20 mm circle island |
| op2 Pocket (T1)  | 30 × 30 mm square, `offset`, 6 mm deep, `corner_relief: dogbone`, `entry: ramp`                                      |
| op3 Drill (T1)   | four 5 mm circles at the corners, 12 mm deep, `peck` 4 mm, `retract: full`, through-cut allowance 0.2 mm             |
| op4 Profile (T1) | outside, 12 mm deep, 4 tabs 8 × 3 mm, climb, `entry: ramp`, `lead` 3 mm radius, through-cut allowance 0.2 mm         |
| op5 Engrave (T2) | closed border 5 mm inside the outline and an open squiggle, 1 mm deep in one pass                                    |
| Safe Z           | 15 mm                                                                                                                |
| Controller       | grblHAL, two programs                                                                                                |


**Success:** the plaque releases after the tabs are cut; the raster pocket
floor is flat with no ridge at the island; a 30 mm square block fits the
dogboned pocket; the four holes take an M5 screw; the engraved line is
continuous with no plunge marks at the start of each pass; the ramp and
lead leave no dwell mark on the outside wall. Record the date, outcome and
deviations below.

**Cut record:** not yet cut.

## Documentation deliverables

- User Guide: Pocket operation (both strategies, islands, allowance, rest
machining); Drill operation (cycles, canned cycles per controller);
Engrave operation; Profile operation updated (entry, leads, corner
relief); Machining order and cut order (the fixed rules and the one
setting); Multi-tool export updated (the user default and the flags);
Tool library (commands, the library file, Fusion import and its
mapping table); Importing DXF; Post-processors updated (six
controllers, what each verifies, canned cycles, tool changes on each);
Warnings (every code with its message and what to do); The project file
updated for every new field and version; Tabs updated for placed tabs and
the `tabs` commands (#105).
- Quick start: "A pocketed plaque from DXF", end to end on the test part.
- Contributing: Crate map (new crates and modules); Porting from
PureCutCNC updated with the Y-up rules for pocket direction, helix
sense and raster angle; Golden corpus how-to for multi-operation cases;
Post-processor verification checklist.

## Exit criteria

- [ ] All acceptance tests and safety tests green on all three OSes.
- [ ] Golden parity cases captured and passing, including the two

  order-compared cases.
- [ ] Each bundled post-processor's README cites its controller sources.
- [ ] Test part cut on grblHAL and recorded above.
- [ ] Quick start and every chapter above published.
- [ ] `v0.2.0` tagged with binaries.

## Open questions

Answered by the maintainer on 2026-10-20. The decisions are folded into
the capabilities above and the design spec; the record stays here.

| # | Question | Decision |
| --- | --- | --- |
| 1 | Fusion `.tools` format source | Autodesk publishes a schema and sample libraries (`cam.autodesk.com/tools/tools/Tooling Schema/JSON Schema.zip`); the importer is written against it. A sample library from the maintainer's own Fusion is still welcome as a second fixture. |
| 2 | Canned cycles default | Off on every controller; opt-in per project. |
| 3 | User config | TOML at `<config dir>/dreamcarve/config.toml` beside `tools.json`; `separate_programs` when absent. |
| 4 | Pocket `direction` in the chapter's JSON | `climb`. |
| 5 | Corner relief on Profile | Yes, on Pocket and Profile. |
| 6 | Profile ramp style | Zigzag on both, as PureCutCNC does; On line ramps along the path. Default entry is `ramp` at 15° for new projects. |
| 7 | Tool numbers | Add `Tool::number: Option<u32>` in v7 (built as v8: #106 took v7); Fusion import reads it; export uses it when set (capability 21a). |
| 10 | Fusion sample libraries | Autodesk's sample files are reference only, not committed; the fixtures are hand-written in the schema's structure. `TA` is the half angle (a 90° V-bit has `TA` 45), confirmed against IDC Woodcraft's Fusion library. |
| 8 | `machining_order` migration | A v6 → v7 migration, not a serde default (built as v7 → v8: #106 took v7). |
| 9 | Rest machining | In scope. |

## Phase 2 addendum (#73)

**Status:** draft. Added 2026-10-21 from the workflow-gap review in #73
(main @ d3cc0ca, phase 1). The capabilities, acceptance tests and safety
rules above still hold; this section adds to them, and its numbers
continue theirs. The design is in `design.md`, "Phase 2 addendum (#73)".

#73 listed eleven gaps. Phase 2 has already closed these:

| #73 item | Closed by |
| --- | --- |
| Plunge strategy: ramp and helix entry, lead-in | PRs #159 (Pocket entry), #174 (Profile entry, ramp by default), #185 (arc leads), #228 (descent starts 1 mm above the stock) |
| Rect tabs step vertically on the wall | `shape: "smooth"` tabs (phase 1, PRs #32, #94) |
| The jog-blocked tool-change pause | #57 (PR #81) |
| Tool library, Fusion library import | PRs #143 (library and `tools` CLI), #160 (Fusion `.tools`/`.json` import) |
| Operation order not checked | PRs #139 (`cut_inside_freed_part`, `tool_change_avoidable`), #274 (`cut_inside_tab_held_part`) |
| Tabs at manual positions | not here: #105 (placed tabs, with the GUI) |

The rest are specified below, one subsection each, in the order they are
built (design addendum, "Order and format versions").

### A1. `export --post` keeps your settings (#277)

27. **Switching post-processor keeps customised settings.** Today
    `export --post <id>` resets all six post-processor settings to the
    definition's defaults, so a raised `spindle_on_dwell_s` is lost when a
    project moves from grblHAL v1 to v2. From now on each setting is kept
    or reset by one rule: a setting still at the old definition's default
    takes the new definition's default (so grblHAL → LinuxCNC still
    changes `file_extension` from `nc` to `ngc`); a setting the user
    changed is kept, unless the new definition cannot use it
    (`tool_change_mode: "m6"` without an `M6` line, `use_canned_cycles:
    true` without canned cycles), in which case it is reset and `export`
    says so, naming the setting, both values and why. The user config's
    `tool_change_mode` applies only to a setting that is reset.
    `--reset-settings` restores today's behaviour: every setting from the
    definition's defaults with the config on top. So `export --post` with
    the project's own id, to upgrade the version, changes only the
    version. No format change.

### A2. Comment transliteration (#278)

28. **Names keep their letters.** Text DreamCarve puts in a comment (the
    project, operation and tool names) is reduced to printable ASCII, as
    before, but a letter with an accent or a common symbol is
    transliterated instead of dropped: "Café" is written `Cafe`, not
    `Caf`; "Ø6 mm" `O6 mm`; "½ in" `1/2 in`; "Straße" `Strasse`; "—" `-`;
    "°" `deg`. A character with no entry in DreamCarve's table is dropped,
    as today, and the `( ) ; ! ? ~ { } %` rule is unchanged, so a
    transliteration never produces one of them. Comments only: no
    coordinate, word or motion changes. Applies to every post-processor
    and every definition version.

### A3. Header modal state on grblHAL and GRBL (#279)

29. **Programs cancel stale modes before they move.** grblHAL v3 and GRBL
    v3 add `G80` (cancel a canned-cycle motion mode) and then `G40`
    (cancel cutter radius compensation) to the header, after `G94`, so a
    mode left by MDI, or by a program stopped without `M2`/`M30`, cannot
    change what the program does. Each was checked against the
    controller's source (design addendum, A3): both only cancel, and on
    GRBL, which has neither mode, both are accepted and do nothing, so a
    GRBL program run on grblHAL is covered too. `G49` (cancel a tool
    length offset: the `G43.1` cancel) is **not** added on either. Both
    controllers fold the active offset into the work zero the user sets
    (`G10 L20`, which senders' "zero Z" sends), so cancelling it after
    work zero is set moves every Z by the offset and can cut deeper than
    the drawing. Instead `export` tells grblHAL and GRBL users: "This
    program does not change the tool length offset (G43.1). If your sender
    sets one, set it before you set Z zero." New projects get v3; a v2
    project keeps v2 until `export --post` (which, with A1, keeps its
    settings). (changed in #312: Mach3 v3 and UCCNC v3
    likewise cancel a stale scale and rotation. Both controllers keep a
    `G51` scale and a `G68` rotation until cancelled, and `M30` does not
    reset them, so a `G51 Z-1` left by MDI would mirror every Z. Their
    headers add `G50` and then `G69` after the modal line, and the setup
    after every pause repeats them; the safety pass accepts them only
    there, alone on a line, and refuses `G51` and `G68` everywhere.
    LinuxCNC and generic have no such codes and do not write them. A v2
    project keeps v2 until `export --post`.) (changed in #316: grblHAL
    also keeps a `G51` scale past `M30`, so grblHAL v4 adds `G50` after
    `G40`, again after every pause and tool change, under the same
    safety rule. grblHAL has no rotation, so there is no `G69`. GRBL has
    no scaling and stays at v3. New projects get grblHAL v4. A v3 project
    keeps v3 until `export --post`. At the checked commit grblHAL's `G50`
    leaves a scale whose X factor is 1, such as `G51 Z-1`, in force. The
    post-processors chapter tells the user to soft-reset after any `G51`
    and check `$G`.) (changed in #320: `export` says so too, once per
    export, after the tool length note, for a grblHAL definition whose
    header writes `G50`: "If you used G51 on this controller, soft-reset
    it and check that $G shows G50 before you start: grblHAL's G50 does
    not clear a scale that leaves X at 1, such as G51 Z-1." The note is
    printed, not written into the program.)

### A4. Cycle-time estimate (#280)

30. **Export says how long the job takes.** After writing, `export`
    prints for each program, and in total, an estimate as VCarve's
    "Estimating Machining Times" makes one
    (<https://docs.vectric.com/docs/V12.5/VCarvePro/ENU/Help/form/Toolpaths%20Summary/index.html>):
    feed moves (lines and arcs, at their programmed `F`), dwells (spindle
    spin-up and Drill `G4`), and rapids at the user's rapid rate, all
    times a scale factor. The rapid rate and scale factor are machine
    facts, so they live in the user config (`[estimate] rapid_mm_min`,
    `scale_factor`, default 1.0); with no rapid rate the estimate gives the
    rapid distance and leaves its time out rather than guess one. The
    estimate never changes a program, and it says what it leaves out:
    acceleration, and the time at pauses and tool changes (it prints their
    count). For example: `Estimated time: 14 min 5 s (cutting 13 min 40 s,
    dwells 25 s); 4.3 m of rapids not included: set [estimate]
    rapid_mm_min in config.toml. Excludes acceleration and 1 pause.`

### A5. Toolpath preview as SVG (#281)

31. **See the toolpaths before phase 4.** `export --preview <file.svg>`
    and `check --preview <file.svg>` write a picture of every enabled
    operation's toolpath over the stock and the shapes, at 1:1 in
    millimetres, seen from above with Y up as drawn. Each operation has a
    colour and a legend entry with its id and name. Cuts are solid, rapids
    dashed, a stretch raised over a tab is drawn as a heavy tab mark, and
    each pass's start is a circle the size of the bit with a tick in the
    cut direction, so `at_distances` tabs and start points (A9) can be
    checked. It is the toolpath DreamCarve generated, not the parsed
    G-code, so arc fitting does not show. `check --preview` writes the
    file when every operation generates, whatever the warnings; `export
    --preview` writes it with the programs, all or none.

### A6. Inch input (#282)

32. **Inch projects take inch values.** Any length (`*_mm`) or feed
    (`*_mm_min`) value in a `.dcv` or a tool library file may be written
    as a string with its unit, as Fusion reads a value with a unit
    whatever the document's units ("6.25 in" is always inches,
    <https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/fusion_Parameter_expression.htm>):
    `"depth_mm": "0.5 in"`, `"depth_mm": "1 1/4 in"`, `"feed_mm_min": "60
    in/min"`, `"safe_z_mm": "5 mm"`. A bare number is in the field's unit,
    as before. DreamCarve converts on load (×25.4, then rounded to 6
    decimal places for a length and 3 for a feed, capability 22b; changed:
    maintainer decision 2026-10-24), so the model, every check and every
    program are unchanged; a command that saves the
    project (`export --post`, `tools add`) writes millimetre numbers back.
    The CLI's own lines that state a length (export's per-operation `…
    deep`, the program extents) use the project's `display_units`, as
    VCarve's job units do
    (<https://docs.vectric.com/docs/V9.0/VCarvePro/ENU/Help/Drawing/uiSheetForm%5CJob%20Setup.html>):
    an inch project's summary says `0.4724 in deep`, not `12 mm deep`.
    Warning messages stay in millimetres in this addendum (their
    parameters are named `_mm`). No format change: DreamCarve still writes
    numbers.

### A7. Separate heights (#283)

33. **Clearance, tool-change and park heights** (`heights` on the
    project; VCarve Material Setup, "Rapid Z Gaps above Material" and
    "Home / Start Position",
    <https://docs.vectric.com/docs/V12.5/VCarvePro/ENU/Help/form/material-setup-flat/index.html>;
    Fusion "Clearance height",
    <https://help.autodesk.com/cloudhelp/ENU/Fusion-CAM/files/MFG-REF-CLEARANCE-HEIGHT.htm>,
    and "Retract height",
    <https://help.autodesk.com/cloudhelp/ENU/Fusion-CAM/files/MFG-REF-RETRACT-HEIGHT.htm>).
    Each is optional; unset, the program is as today.
    - **`clearance_mm`**, above the stock top: the height for moves
      between parts (feature instances) within an operation, between
      operations, at a tool change and at the end. Each operation's
      `safe_z_mm` stays the retract between passes of one part, like
      Fusion's retract height. It must be at least every enabled
      operation's `safe_z_mm`.
    - **`tool_change_machine_z_mm`**, in machine coordinates: before each
      tool-change stop (`T<n> M6`) and at the end of every program, the
      program lifts to this machine Z with `G53 G0 Z…`, so the bit clears
      clamps and is easy to reach. A program-per-tool export ends each
      program there, which serves as the tool-change height on GRBL. It
      must be below 0 (both controllers home Z to a negative machine
      position) and is accepted only on a post-processor whose definition
      documents `G53` (grblHAL and GRBL in this addendum; LinuxCNC too,
      changed in #306). The machine must
      be homed, which DreamCarve cannot check, so `export` says so. It
      also says, as the program's header does, what keeps a lift outside
      the travel from moving: on grblHAL and GRBL, Z homing upward or soft
      limits on (`$20=1`), since where Z homes downward every negative
      machine Z is below the travel; on LinuxCNC, homing, after which its
      soft limits always apply (changed in #308).
    - **`park`** `{ "x_mm": …, "y_mm": … }`, in project coordinates: at the
      end the bit rises to the clearance height (or the highest safe Z of
      the program's operations; changed in #283: the last operation's),
      rapids to this point, then makes the machine-Z lift if set.
      This is VCarve's Home position without its Z (the lift above is the
      Z).
    Projects from earlier versions get all three unset and export the same
    bytes. Today's two-tool export, `G0 Z5.000`, `M5`, `Z2.000`, then the
    stop, becomes `G0 Z<clearance>`, `G53 G0 Z<machine z>`, `M5`, then the
    stop.

### A8. Profile last pass (#284)

34. **Last pass allowance** (`last_pass` on Profile; VCarve 2D Profile
    "Last Pass Allowance",
    <https://docs.vectric.com/docs/V9.5/VCarvePro/ENU/Help/Reference%20Manual/Toolpaths/2D%20Profile%20Toolpath/2D%20Profile%20Toolpath.html>;
    Fusion 2D Contour "Stock to Leave" and finishing passes,
    <https://help.autodesk.com/cloudhelp/ENU/Fusion-CAM/files/GUID75B6821B-DE26-4E3B-AF10-4A54131CD9E4.htm>).
    Every pass but the last leaves `allowance_mm` on the wall; the last
    pass cuts to size, so it takes a thin, even skin off the whole wall
    and leaves no step marks. `thickness_mm` (VCarve "Set Last Pass
    Thickness") sets the depth of that last pass; unset, it is the normal
    last depth pass. `reverse` (VCarve "Reverse direction") cuts the last
    pass the other way. The allowance must be more than 0 and at most a
    third of the bit diameter, as VCarve advises. Inside and Outside only:
    On line has no wall to leave material on. Unset (`null`) by default,
    in `init` and in migrated projects. (changed in #284: Left and Right
    are refused as On line is. A drawing the earlier passes cannot follow,
    where it fits the bit but not the bit plus the allowance on both sides
    or an earlier pass would come within the bit's radius of another
    part, is refused naming the shapes. Changed again in the #310 review:
    `thickness_mm` may be no more than the step-down; the last pass may
    take off no more than the allowance, √2 times it in a square corner,
    so a corner sharper than 90° is refused; and a one-pass cut warns
    `last_pass_unused`. Changed again in #324: a sharp corner is cut. The
    earlier passes cut both its walls and stop short of where they meet,
    so the last pass takes the wedge between, `a / sin(θ/2)` thick at a
    corner of `θ`; that may be up to a third of the bit diameter, the
    thickest skin the allowance itself may be, or √2 times the allowance
    where that is more. With a 6.35 mm bit that cuts corners down to
    about 27° at 0.5 mm and 11° at 0.2 mm: triangles and stars cut
    inside, V notches cut outside, and the corners of letters such as A,
    V, W, M and K. A narrower tip is refused, and
    so is a notch, slot or inlet that the earlier passes bridge and skip,
    however thin the last pass's cut there, once it is thicker than a
    square corner's. Changed again in #311: Left and Right take a last
    pass, as VCarve's Profile of an open vector does; they cut one flank
    of the bit against the part, as Inside and Outside do, and only On
    line, which cuts with both, is refused. The earlier passes run the
    allowance farther to the cut side and end square on the same end
    normals as the last pass, so no pass cuts past an end on the part
    side and the last pass takes the skin off right up to each end;
    `reverse` starts the last pass at the path's other end, where the
    earlier passes end, so it mills the other way. The refusals above
    apply to the path, a stretch at an end the earlier passes stop short
    of counting as no corner, and the earlier passes must keep the bit's
    radius plus the allowance from every other line and every kept
    strip. Changed again in #339: the last pass enters as one more
    earlier pass would, never straight down through the allowance,
    which stands uncut to the stock top beside the part. It goes down
    into the earlier passes' floor where they ended, with the
    operation's entry (a plunge one step-down or `thickness_mm` deep, a
    ramp along the earlier passes' own path inside their kerf, or a
    helix in their bore), then steps across the allowance at the feed
    onto its own path, whose start is beside theirs. So every plunge
    into uncut material is at most the step-down, whatever the entry.
    With a lead, the earlier passes are led in beside the last pass's
    lead and the step joins the two staging points; a last pass lead-in
    that starts too far from their cut is left off with
    `lead_dropped`. Changed again in #335: round an inside arc of its
    path, of radius `ρ` round a wall of `ρ + r`, the last pass runs at
    `feed · ρ / (ρ + r)`, rounded down, so its cutting edge moves at the
    feed (the issue's `k` is 1) and it removes no more per minute than a
    straight wall's last pass; never below `feed · a / (a + r)`, the
    smallest hole's, where a tighter arc warns `last_pass_edge_faster`.
    Straight cuts, outside corners, lead arcs and the earlier passes keep
    the feed, and so does a curve drawn as straight segments. Changed
    in #350: where such a curve bends so that the edge moves more than
    1.5 times the feed, the operation warns `last_pass_flattened_curve`;
    its feeds are unchanged.)

### A9. Tabs that avoid corners, and start-point control (#285)

35. **Tabs off corners** (`tabs.avoid_corners`). A tab whose stretch (tab
    width plus bit diameter) would lie on a corner, where the bit's path
    turns 30° or more within one bit diameter, slides along the path to
    the nearest place where it does not, never more than half the gap to
    its neighbours and never onto another tab. One that cannot is left
    where it was, with a `tab_on_corner` warning naming it. A rotated
    square's four evenly spaced tabs then sit on its straight sides, not
    on the arcs round its corners; a circle's tabs never move. Fusion's
    "Segment" tab positioning places tabs between points of the contour
    (<https://help.autodesk.com/cloudhelp/ENU/Fusion-CAM/files/MFG-REF-2D-CONTOUR-TABS.htm>);
    VCarve documents no corner rule, so the 30° rule is DreamCarve's.
    `init` writes `true`; migrated projects get `false`, so they cut as
    before. (changed in #285: it acts wherever a request is laid, at
    export and in `tabs place` with the request or `--count`, which avoids
    corners unless `--allow-corners`; tabs placed with `--at` or by their
    point are cut as placed and never move. "Half the gap" is measured to
    the neighbours as the layout put them, before any tab moved. A tab
    left on a corner is kept, so the #109 floor holds.)
36. **Start point** (`start` on Profile; VCarve 2D Profile "Use vector
    start point" and "Start At", on the page A8 cites; Fusion "Entry
    Positions"):
    - `"auto"` (default, and migrated projects): as today. The start
      follows from the drawing's first vertex, moved by a lead or by an
      entry that has no room beside a tab.
    - `"first_vertex"`: each closed outline's cut starts at the point of
      the bit's path nearest the drawing's first vertex, as VCarve's "Use
      vector start point" plunges at the shape's first point.
    - `{ "near": { "x_mm": …, "y_mm": … } }`: each closed outline's cut
      starts at the point of its bit path nearest that project point, a
      fixed-point form of VCarve's "Closest on Bounding Box".
    With `first_vertex` or `near`, a lead arrives at that point or is
    dropped, an entry with no room there falls back (ramp, then plunge)
    instead of moving, and `at_distances` tabs are measured from that
    point, so the user knows where they are. Open paths ignore `start`:
    their start is the end the cut direction sets (#242). (changed in
    #285: "nearest" is `nearest_point`'s, which takes the middle of an arc
    centred on the target, so `first_vertex` on a convex corner cut
    outside starts mid-arc round it; a last pass's allowance passes start
    at their own path's point nearest the same target. Changed again in
    #337: on a start under a tab, the first pass below the tab top plunges
    only to the tab top there, warned `entry_fallback`; each later pass
    starts on the tab where the one before ended, never lower, and every
    pass comes down to its depth past the tab's end, as a lap leaves any
    tab: a rect tab's step, a smooth tab's slope. This holds for `auto`
    too, where a plunge entry keeps its start.)

### Acceptance tests (addendum)

39. **`--post` keeps settings.** A grblHAL v1 project with
    `spindle_on_dwell_s: 6` exported with `--post grblhal` is saved at the
    latest grblHAL version with `spindle_on_dwell_s: 6` and every other
    setting unchanged. With `--post grbl` and `tool_change_mode: "m6"`
    the mode is reset to `"separate_programs"` and the summary names it
    and why. `--reset-settings` gives the definition's defaults.
40. **Transliteration.** A project named "Café Ø6 ½" writes `( Project:
    Cafe O6 1/2 )` on grblHAL; every byte of every program is printable
    ASCII; a name made only of unmapped characters writes an empty name,
    as today.
41. **Header.** grblHAL v3 and GRBL v3 programs have `G80` then `G40`
    after `G94`, in the header and in the setup written again after each
    stop; neither has `G49` or `G43`; the first motion line after each
    carries its motion word; v2 projects export their v2 bytes.
42. **Estimate.** A program of one 100 mm `G1` at F600, a 3 s spin-up
    dwell and 50 mm of rapids reports 10 s cutting and 3 s dwell, leaves
    the 50 mm out with no rapid rate and adds 1 s with `rapid_mm_min =
    3000`; `scale_factor = 2` doubles every time; the program bytes are
    the same with and without the config.
43. **Preview.** The coaster's preview is one SVG whose viewBox is the
    stock and whose cut paths are the toolpath's XY with Y flipped
    (`y_svg = height − y`) within 0.001 mm, with one tab mark per tab per
    raised pass and one start circle per pass; a CCW arc is drawn with
    sweep flag 0; the same project gives the same bytes twice.
44. **Inch input.** `"depth_mm": "0.5 in"` loads as 12.7, `"1 1/4 in"` as
    31.75 and `"feed_mm_min": "60 in/min"` as 1524; the program equals the
    one from the millimetre numbers; `"0.5 furlong"` fails naming the
    field, line and column; an inch project's summary says `0.5000 in
    deep`.
45. **Heights.** The two-tool plaque on grblHAL in one program, with
    `clearance_mm: 20`, `tool_change_machine_z_mm: -5` and `park: {x_mm:
    0, y_mm: 0}`: links between parts and between operations rise to Z
    20 (work zero at the stock top) and links between passes of one part
    to its safe Z; the tool change is `G0 Z20.000`, `G53 G0 Z-5.000`,
    `M5`, `T2 M6`; the program ends with a rapid to the park point at Z
    20, then `G53 G0 Z-5.000`, then the footer. On Mach3
    `tool_change_machine_z_mm` is an error naming the setting (changed in
    #306: on LinuxCNC, which now documents `G53`); a
    `clearance_mm` below an enabled operation's `safe_z_mm` is an error;
    with all three unset the program is byte-identical to before.
46. **Last pass.** An outside Profile of a 60 × 40 mm rectangle, 12 mm
    deep in 4 mm passes, with `last_pass: {allowance_mm: 0.5}`: the first
    two passes run 0.5 mm farther from the part than the third, which is
    on the phase 1 path. With `thickness_mm: 1` the passes are 4, 8, 11
    and 12 mm deep and only the 12 mm pass is on the part's path. With
    `reverse` the last pass runs the other way. An allowance of 2.2 mm
    with a 6.35 mm bit, or any `last_pass` on an On line Profile, is an
    error.
47. **Corners and start.** A 50 mm square rotated 45°, cut outside with 4
    tabs and `avoid_corners: true`: every tab stretch lies on a straight
    side; with `false` the tabs are where they were. The coaster's program
    is byte-identical either way. `start: {near: {x_mm: 0, y_mm: 0}}`
    starts each outline's cut at its path point nearest the origin, and
    `at_distances: [10]` puts the tab 10 mm along the path from there.
    (changed in #285: from the `auto` start, which is the start of a side,
    four tabs already sit mid-side, since the path's period is a quarter
    of it; with the `near` start, mid-side, they sit mid-arc without
    `avoid_corners` and on the sides with it. The test checks both, and
    three tabs from the `auto` start.)

### Safety (addendum)

- **S16 Machine-coordinate moves.** A program writes `G53` only as `G53
  G0 Z<tool_change_machine_z_mm>`, alone on its line, only on a
  definition that documents `G53`, only when the project sets the
  height, and only before a tool-change stop or at the program end, at
  the XY where the bit already is, after a retract to at least safe Z.
  The safety pass treats work Z as unknown after it, as after `M0` (S4),
  so the next XY rapid needs a retract in work coordinates first. It
  refuses every other `G53`, except the same lift again right after a stop
  that followed one, with nothing moved between (changed in #304 review:
  with manual spindle control the user may jog at the router-off pause
  before the tool change).
- **S2 and S6, changed.** S6's "only other motion in an operation" adds
  the rise to the clearance height before an operation's first move and
  after its last. After the last operation a program may rapid, at or
  above the clearance height (the highest safe Z of the program if
  unset; changed in #283: safe Z), to the park point, then
  make the S16 lift; S2 holds for that rapid. A link between parts rises
  to the clearance height; a link between passes of one part, to safe Z.
- **S9 with a last pass.** Every pass but the last keeps `allowance_mm`
  more than `r` from the part; the last keeps `r`, as now. (changed in
  the #310 review: every pass but the last also keeps at least `r` from,
  and stays out of, the operation's other parts; nearer than `r + a` it
  cuts only their skin, which is allowed. The last pass takes off at most
  `√2 · allowance_mm` plus 0.05 mm anywhere, and no pass before it cuts
  into stock a tab leaves. Changed in #324: at a corner, where the
  earlier passes cut both walls, the last pass may take off up to a third
  of the bit diameter, or that bound where it is more; nowhere else more
  than the square corner's. Changed in #311: on a Left or Right Profile
  every pass but the last runs on the targets' open paths offset by
  `r + allowance_mm` to the side, and every pass keeps `r` from every open
  path and kept edge, as without a last pass. Changed in #339: the last
  pass's step across the allowance, from the path the earlier passes cut
  to its own, may run between the two, on Left and Right too, still `r`
  from every open path and kept edge: one level move from one path to the
  other, no longer than the generator's bound.)
- **S3 with a last pass** (#335). A level cut of a Profile with a last
  pass may run slower than its tool's feed, down to
  `feed · a / (a + r)`, the allowance `a` over the allowance plus the
  tool radius: the last pass's inside arcs run at `feed · ρ / (ρ + r)`,
  no lower than that. Slower, or faster than the feed, is still
  `feed_not_from_tool`; so is a slower level cut of any other operation.
  The emitter writes such a feed rounded down.
