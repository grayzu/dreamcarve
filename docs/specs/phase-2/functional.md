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
shape an earlier operation has already cut free, and hints
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
concave corners. The notch pass runs after the main passes, stepping
down at the tool's step-down.
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

## Out of scope

V-Carve and V-bit countersinking (phase 3). Helical boring of holes wider
than the tool (a Pocket on the circle does it). Trochoidal clearance,
seeded offset, engagement-based feed reduction, slot feed reduction,
corner rounding of toolpaths, wall corner clean-up (PureCutCNC features not
in VCarve's pocket form; post-1.0 or phase 8). Pocket fill patterns for
Engrave (VCarve's hatch and offset fill). Straight-line leads. Start depth
(every operation starts at the stock top). Separate clearance, tool-change
and park heights (#73; safe Z remains the one height). Drill tip
compensation and "drill through". Tabs as placed objects (#105, with the
GUI). Region masks and clamps as obstacles. A GUI, simulation, text,
toolpath preview images. Reading Vectric, Carveco or Carbide Create tool databases.

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
    first move into each pass is a zigzag in the stock around the part
    whose footprint never enters the part or another shape, with a slope
    of 15° within 0.1°, at the feed rate, with its Z component never
    faster than the plunge rate; no vertical plunge occurs below the
    stock top. An On line Profile of a 100 mm line ramps back and forth
    along the line itself. An inside Profile of a 7 mm hole with a
    6.35 mm tool has no room, plunges, and warns `entry_fallback`.
    (changed in #175: the 100 mm line is an open path, which an On line
    Profile now cuts, as VCarve's Profile "On" does; Inside and Outside
    still skip open paths.)
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
29. **Dogbone and T-bone.** A 40×30 mm pocket with `corner_relief:  dogbone`: after the main passes, four notches along the corner
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
check between shapes.
- **S10 Entry descent rate.** A ramp or helix descends with a vertical
component no faster than the plunge rate: the move's feed is scaled so
that `feed × sin(slope) ≤ plunge`. Checked on the parsed program by
recomputing the slope of every descending `G1` with XY motion.
- **S11 Canned cycles mean what the plain moves mean.** When a program uses
`G81`/`G82`/`G83`/`G73`, the safety pass expands each cycle line into the
motion the controller performs (per that controller's documentation,
with `G98` retract) and checks S1, S2 and S6 against that expansion, so
a canned-cycle program is accepted only if its expansion is the Drill
toolpath move for move.
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
updated for every new field and version.
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
